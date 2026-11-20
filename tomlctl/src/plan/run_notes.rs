//! The `plan run notes` leaf: reads the run note of every task the latest
//! `implement:<id>` run dispatched and upserts one `[[attempt]]` per task,
//! keyed on `(task, run_id)`. A missing note, a note without a block and a
//! malformed block each still record an attempt, so the command never fails on
//! a note; it reports what was wrong in each row's `problem`.

use std::fs;
use std::io::ErrorKind as IoErrorKind;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Value as JsonValue, json};

use super::emit::load_milestone_store;
use super::run_note;
use super::schema::{Attempt, NoteBlock, PlanStore, RunState};
use super::store;
use crate::cli::PlanRunNotesArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::output::{Rows, print_rows_compact};
use crate::tasks::schema::Store;

const STEP_PREFIX: &str = "implement:";

/// What reading one task's note found.
enum NoteRead {
    Missing,
    /// The note resolves outside `docs/flows/<slug>/` once symlinks are followed.
    Outside,
    Text(String),
}

pub(super) fn run(args: PlanRunNotesArgs) -> Result<()> {
    let PlanRunNotesArgs {
        target,
        milestone,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    let tasks = load_milestone_store(slug, &milestone, integrity.verify_integrity)?;
    let authored = store::authored_dir(slug)?;
    let step = format!("{STEP_PREFIX}{milestone}");

    let (run_id, rows) = store::mutate(&store::resolve(slug)?, &integrity, |plan| {
        let (run_id, dispatched) = latest_run(plan, &step)?;
        let mut rows = Vec::with_capacity(dispatched.len());
        for task in dispatched {
            let attempt = read_attempt(&authored, &tasks, task, &run_id, &milestone)?;
            rows.push(row(&attempt));
            upsert(plan, attempt);
        }
        Ok((run_id, rows))
    })?;

    print_rows_compact(
        &json!({
            "ok": true,
            "milestone": milestone,
            "run_id": run_id,
            "rows": rows,
        }),
        Rows::Field("rows"),
    )
}

/// The run id and dispatched task ids of the last unskipped `step` run.
fn latest_run(plan: &PlanStore, step: &str) -> Result<(String, Vec<u32>)> {
    let Some(run) = plan
        .runs
        .iter()
        .rfind(|run| run.step == step && run.state() != RunState::Skipped)
    else {
        return Err(tagged_err(
            ErrorKind::NotFound,
            None,
            format!("no `{step}` run is recorded in the flow's plan.toml"),
        ));
    };
    if run.tasks.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "the latest `{step}` run records no task ids, so nothing says what it \
                 dispatched: it was started without `plan run start --tasks`"
            ),
        ));
    }
    Ok((run.run_id.clone(), run.tasks.clone()))
}

/// The attempt `task`'s note records, its `problem` the one line saying what
/// was wrong with the note. Only an I/O failure other than a missing note is
/// an error.
fn read_attempt(
    authored: &Path,
    tasks: &Store,
    task: u32,
    run_id: &str,
    milestone: &str,
) -> Result<Attempt> {
    let relative = format!("notes/run/task-{task}.md");
    let row_status = tasks
        .items
        .iter()
        .find(|row| row.id == task)
        .map(|row| row.status.as_str());
    let mut attempt = Attempt {
        task,
        run_id: run_id.to_string(),
        milestone: milestone.to_string(),
        status: row_status.unwrap_or_default().to_string(),
        summary: String::new(),
        block: NoteBlock::Absent,
        files_touched: Vec::new(),
        deviations: Vec::new(),
        tangential: Vec::new(),
        checks: Vec::new(),
        sources: Vec::new(),
        handover: None,
        problem: String::new(),
    };

    let text = match read_note(authored, &relative)? {
        NoteRead::Missing => {
            attempt.problem = format!("the run note `{relative}` is missing");
            return Ok(attempt);
        }
        NoteRead::Outside => {
            attempt.problem =
                format!("the run note `{relative}` resolves outside the flow's docs directory");
            return Ok(attempt);
        }
        NoteRead::Text(text) => text,
    };
    let note = match run_note::parse(&text) {
        Ok(note) => note,
        Err(err) => {
            attempt.block = NoteBlock::Malformed;
            attempt.problem = format!("the note is malformed: {err}");
            return Ok(attempt);
        }
    };

    attempt.problem = if note.block == NoteBlock::Malformed {
        format!("the block is malformed: {}", note.reason)
    } else {
        let status = match row_status {
            None => format!("task {task} has no row in the task store"),
            Some(row) if row != note.status => format!(
                "the note's Status is `{}` but the task row is `{row}`",
                note.status
            ),
            Some(_) => String::new(),
        };
        [status, note.reason]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("; ")
    };
    attempt.status = note.status;
    attempt.summary = note.summary;
    attempt.block = note.block;
    attempt.files_touched = note.files_touched;
    attempt.deviations = note.deviations;
    attempt.tangential = note.tangential;
    attempt.checks = note.checks;
    attempt.sources = note.sources;
    attempt.handover = note.handover;
    Ok(attempt)
}

/// The note's text, held inside the authored anchor so that a symlink cannot
/// read another file into the store.
fn read_note(authored: &Path, relative: &str) -> Result<NoteRead> {
    let path = authored.join(relative);
    let resolved = match path.canonicalize() {
        Ok(resolved) => resolved,
        Err(err) if err.kind() == IoErrorKind::NotFound => return Ok(NoteRead::Missing),
        Err(err) => return Err(err).with_context(|| format!("resolving {}", path.display())),
    };
    if !resolved.starts_with(store::authored_anchor(authored)?) {
        return Ok(NoteRead::Outside);
    }
    let bytes = fs::read(&resolved).with_context(|| format!("reading {}", resolved.display()))?;
    Ok(NoteRead::Text(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Replaces the attempt of the same `(task, run_id)` in place, or appends one,
/// so a second call over unchanged notes leaves the store as it was.
fn upsert(plan: &mut PlanStore, attempt: Attempt) {
    match plan
        .attempts
        .iter_mut()
        .find(|a| a.task == attempt.task && a.run_id == attempt.run_id)
    {
        Some(existing) => *existing = attempt,
        None => plan.attempts.push(attempt),
    }
}

fn row(attempt: &Attempt) -> JsonValue {
    json!({
        "task": attempt.task,
        "status": attempt.status,
        "block": attempt.block.as_str(),
        "deviations": attempt.deviations.len(),
        "tangential": attempt.tangential.len(),
        "handover": attempt.handover.is_some(),
        "problem": attempt.problem,
    })
}
