//! `plan emit lane-check`: whether the latest `implement:<id>` run stayed in
//! its lane. A worker may edit only the files of a started row and may write
//! only `status` and `agent` on its own task row, and nothing enforces either,
//! so this compares the working tree and the task store against what the run
//! was licensed to touch.
//!
//! The store side reads two sources. The `[[changes]]` trail names each field
//! an audited verb wrote inside the run's window. The run's `store_digest`
//! catches a write the trail cannot show, section by section, with every
//! dispatched row's `status` and `agent` left out of it. `plan emit
//! launch-args` takes the digest before the Workflow starts and prints when,
//! as `digest_at`, which opens the window; a run whose digest `plan run start`
//! took itself has no `digest_at`, and its window opens at `started`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use jiff::Timestamp;
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use toml::Value as TomlValue;
use toml::value::Datetime;

use super::drift::uncommitted_files;
use super::emit::{WORKER_AGENTS, load_milestone_store};
use super::schema::{Run, RunState};
use super::store;
use crate::cli::{PlanEmitLaneCheckArgs, ReadIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::hex_lower;
use crate::io::repo_or_cwd_root;
use crate::output::print_json_line;
use crate::tasks::schema::{Status, Store, normal_path, to_toml};
use crate::tasks::{load_store, resolve_store_path};

const STEP_PREFIX: &str = "implement:";
const BACKLOG_STORE: &str = ".claude/backlog.toml";
const SIDECAR_SUFFIX: &str = ".sha256";
const DIGEST_HEX_LEN: usize = 16;

/// The store's top-level keys, grouped into the sections a digest names. A key
/// in no group, other than `last_updated`, which every write restamps, falls to
/// `rest`.
const SECTIONS: [(&str, &[&str]); 4] = [
    ("rows", &["items"]),
    ("specs", &["specs"]),
    ("milestones", &["checkpoints", "milestones"]),
    ("changes", &["changes"]),
];
const REST: &str = "rest";
const UNSTAMPED_KEY: &str = "last_updated";

/// The store's digest by section, for a run of `step` dispatching `tasks` that
/// started at `started`. Empty unless `step` is an `implement:` step and the
/// flow's task store reads, so a run it cannot cover is never refused for it.
pub(super) fn run_store_digest(
    slug: &str,
    step: &str,
    tasks: &[u32],
    started: &Datetime,
) -> BTreeMap<String, String> {
    if !step.starts_with(STEP_PREFIX) {
        return BTreeMap::new();
    }
    let read = || -> Result<Store> {
        let path = resolve_store_path(Some(slug), None)?;
        load_store(
            &path,
            &ReadIntegrityArgs {
                verify_integrity: false,
                strict_read: false,
            },
        )
    };
    match read() {
        Ok(store) => store_digest(&store, tasks, started),
        Err(_) => BTreeMap::new(),
    }
}

/// Each section of `store` hashed with the dispatched rows' `status` and
/// `agent` blanked and only the trail rows written before `started`, which are
/// the ones that existed when the digest was taken.
pub(crate) fn store_digest(
    store: &Store,
    dispatched: &[u32],
    started: &Datetime,
) -> BTreeMap<String, String> {
    let mut projected = store.clone();
    projected.last_updated = None;
    for row in &mut projected.items {
        if dispatched.contains(&row.id) {
            row.status = Status::Pending;
            row.agent = String::new();
        }
    }
    let since = instant(started);
    projected
        .changes
        .retain(|change| !written_since(change.at.as_ref(), since));
    let TomlValue::Table(root) = to_toml(&projected) else {
        unreachable!("the task store serialises to a table");
    };
    let mut grouped: BTreeMap<&str, Vec<(&str, &TomlValue)>> = BTreeMap::new();
    for (key, value) in &root {
        if key == UNSTAMPED_KEY {
            continue;
        }
        let section = SECTIONS
            .iter()
            .find(|(_, keys)| keys.contains(&key.as_str()))
            .map_or(REST, |(name, _)| name);
        grouped.entry(section).or_default().push((key, value));
    }
    SECTIONS
        .iter()
        .map(|(name, _)| *name)
        .chain([REST])
        .map(|name| {
            let entries = grouped.remove(name).unwrap_or_default();
            let bytes = serde_json::to_vec(&entries).unwrap_or_default();
            let mut hex = hex_lower(&Sha256::digest(&bytes));
            hex.truncate(DIGEST_HEX_LEN);
            (name.to_string(), hex)
        })
        .collect()
}

/// `store_digest` as one shell token, `<section>:<hex>` pairs joined by commas
/// in section order, the form `plan run start --store-digest` reads back.
pub(super) fn encode_digest(digest: &BTreeMap<String, String>) -> String {
    digest
        .iter()
        .map(|(section, hex)| format!("{section}:{hex}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Reads an [`encode_digest`] token, refusing any that does not name every
/// section exactly once with a lowercase hex digest of the stored length.
pub(super) fn decode_digest(token: &str) -> Result<BTreeMap<String, String>> {
    let expected: BTreeSet<&str> = SECTIONS
        .iter()
        .map(|(name, _)| *name)
        .chain([REST])
        .collect();
    let refuse = |why: String| {
        tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "`--store-digest {token}`: {why}; pass the `store_digest` `plan emit launch-args` printed"
            ),
        )
    };
    let mut digest = BTreeMap::new();
    for pair in token.split(',') {
        let Some((section, hex)) = pair.split_once(':') else {
            return Err(refuse(format!("`{pair}` is not `<section>:<hex>`")));
        };
        if !expected.contains(section) {
            return Err(refuse(format!("`{section}` is not a digest section")));
        }
        let is_hex = hex.len() == DIGEST_HEX_LEN
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !is_hex {
            return Err(refuse(format!(
                "`{section}` is not {DIGEST_HEX_LEN} lowercase hex digits"
            )));
        }
        if digest
            .insert(section.to_string(), hex.to_string())
            .is_some()
        {
            return Err(refuse(format!("`{section}` is named twice")));
        }
    }
    if digest.len() != expected.len() {
        let missing: Vec<&str> = expected
            .iter()
            .filter(|section| !digest.contains_key(**section))
            .copied()
            .collect();
        return Err(refuse(format!("missing {}", missing.join(", "))));
    }
    Ok(digest)
}

/// When a digest of `store` taken at `now` opens its window: `now`, or the
/// second after the newest trail row when that row carries `now`'s second or a
/// later one. Stamps are whole seconds, so such a row was written before the
/// digest yet would read as written during the run; a write later in that
/// second falls before the window and changes the `changes` digest instead.
pub(super) fn window_open(store: &Store, now: &Datetime) -> Result<Datetime> {
    let newest = store
        .changes
        .iter()
        .filter_map(|change| change.at.as_ref().and_then(instant))
        .max();
    match (newest, instant(now)) {
        (Some(newest), Some(taken)) if newest >= taken => {
            let text = Timestamp::from_second(newest.as_second() + 1)?.to_string();
            Ok(text.parse()?)
        }
        _ => Ok(*now),
    }
}

/// Where the run's window opens: when its digest was taken.
fn window_start(run: &Run) -> &Datetime {
    run.digest_at.as_ref().unwrap_or(&run.started)
}

fn instant(at: &Datetime) -> Option<Timestamp> {
    at.to_string().parse().ok()
}

/// A trail row with no readable time is counted as written before the run, so
/// it sits in the digest rather than escaping both checks.
fn written_since(at: Option<&Datetime>, since: Option<Timestamp>) -> bool {
    match (at.and_then(instant), since) {
        (Some(at), Some(since)) => at >= since,
        _ => false,
    }
}

fn written_within(at: Option<&Datetime>, run: &Run) -> bool {
    let since = instant(window_start(run));
    if !written_since(at, since) {
        return false;
    }
    match (
        run.finished.as_ref().and_then(instant),
        at.and_then(instant),
    ) {
        (Some(end), Some(at)) => at <= end,
        _ => true,
    }
}

pub(super) fn lane_check(args: PlanEmitLaneCheckArgs) -> Result<()> {
    let PlanEmitLaneCheckArgs {
        target,
        milestone,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    let tasks = load_milestone_store(slug, &milestone, integrity.verify_integrity)?;
    let plan = store::load(
        &store::resolve(slug)?,
        &ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: false,
        },
    )?;
    let step = format!("{STEP_PREFIX}{milestone}");
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

    let out_of_lane = out_of_lane(&tasks, run, slug)?;
    let mut store_writes = trail_writes(&tasks, run);
    let digest = if run.store_digest.is_empty() {
        "unrecorded"
    } else {
        let now = store_digest(&tasks, &run.tasks, window_start(run));
        let changed: Vec<&String> = run
            .store_digest
            .iter()
            .filter(|(section, hex)| now.get(*section) != Some(*hex))
            .map(|(section, _)| section)
            .collect();
        for section in &changed {
            store_writes.push(json!({ "source": "digest", "section": section }));
        }
        if changed.is_empty() {
            "matched"
        } else {
            "changed"
        }
    };
    let clean = out_of_lane.is_empty() && store_writes.is_empty() && digest == "matched";
    print_json_line(&json!({
        "clean": clean,
        "run_id": run.run_id,
        "out_of_lane": out_of_lane,
        "store_writes": store_writes,
        "digest": digest,
    }))
}

/// Uncommitted paths outside every lane, each with the rows whose `files` hold
/// it. A lane is the files of a dispatched row or of any started row not yet
/// committed, whose edits are the flow's own unfinished work; the backlog store
/// and every review store a finding link names are exempt with their sidecars.
fn out_of_lane(tasks: &Store, run: &Run, slug: &str) -> Result<Vec<JsonValue>> {
    let root = repo_or_cwd_root()?;
    let uncommitted = uncommitted_files(&root, slug)?;
    let mut lane: BTreeSet<String> = tasks
        .items
        .iter()
        .filter(|row| {
            run.tasks.contains(&row.id)
                || (WORKER_AGENTS.contains(&row.agent.as_str()) && row.commit.trim().is_empty())
        })
        .flat_map(|row| row.files.iter().map(|file| normal_path(file)))
        .collect();
    let exempt = std::iter::once(BACKLOG_STORE.to_string())
        .chain(tasks.finding_links.iter().map(|link| link.store.clone()));
    for path in exempt {
        lane.insert(format!("{}{SIDECAR_SUFFIX}", normal_path(&path)));
        lane.insert(normal_path(&path));
    }
    Ok(uncommitted
        .iter()
        .filter(|path| !lane.contains(path.as_str()) && !lane.contains(&normal_path(path)))
        .map(|path| {
            let holders: Vec<u32> = tasks
                .items
                .iter()
                .filter(|row| {
                    row.files
                        .iter()
                        .any(|file| normal_path(file) == normal_path(path))
                })
                .map(|row| row.id)
                .collect();
            json!({ "path": path, "rows": holders })
        })
        .collect())
}

/// Every trail row written while the run was open. The orchestrator writes none
/// then, and `status` and `agent` leave no trail row, so each one is a write
/// beyond a worker's licence.
fn trail_writes(tasks: &Store, run: &Run) -> Vec<JsonValue> {
    tasks
        .changes
        .iter()
        .filter(|change| written_within(change.at.as_ref(), run))
        .map(|change| {
            let task = tasks
                .items
                .iter()
                .find(|row| row.r#ref == change.r#ref)
                .map(|row| row.id);
            json!({
                "source": "trail",
                "ref": change.r#ref,
                "task": task,
                "field": change.field,
                "old": change.old,
                "new": change.new,
                "actor": change.actor,
                "at": change.at.map(|at| at.to_string()).unwrap_or_default(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{Change, Effort, TaskRow, TaskSpec};

    fn at(text: &str) -> Datetime {
        text.parse().expect("a TOML datetime")
    }

    fn row(id: u32, status: Status, agent: &str) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status,
            checkpoint: "A".to_string(),
            phase: String::new(),
            phase_depth: 0,
            heading_depth: 4,
            files: vec![format!("src/{id}.rs")],
            needs: Vec::new(),
            coupling: Vec::new(),
            deps_note: String::new(),
            action: String::new(),
            detail: String::new(),
            acceptance: String::new(),
            agent: agent.to_string(),
            commit: String::new(),
        }
    }

    fn store() -> Store {
        Store {
            items: vec![row(1, Status::Pending, ""), row(2, Status::Pending, "")],
            specs: vec![TaskSpec {
                r#ref: "task-1".to_string(),
                outcome: "One".to_string(),
                constraints: Vec::new(),
                accept: Vec::new(),
                premises: Vec::new(),
                means: Vec::new(),
                design: Vec::new(),
                precedent: Vec::new(),
                executor: "lite".to_string(),
            }],
            ..Store::default()
        }
    }

    #[test]
    fn window_open_steps_past_a_trail_row_of_the_same_second() {
        let now = at("2026-11-11T15:23:04Z");
        let mut tasks = store();
        assert_eq!(window_open(&tasks, &now).unwrap(), now);
        tasks.changes.push(change("2026-11-11T15:23:03Z", "detail"));
        assert_eq!(window_open(&tasks, &now).unwrap(), now);
        tasks.changes.push(change("2026-11-11T15:23:04Z", "detail"));
        let open = window_open(&tasks, &now).unwrap();
        assert_eq!(open, at("2026-11-11T15:23:05Z"));

        let run = Run {
            step: "implement:A".to_string(),
            workflow: String::new(),
            args_hash: String::new(),
            run_id: "wf_1".to_string(),
            session: String::new(),
            started: at("2026-11-11T15:23:09Z"),
            finished: None,
            skip_reason: String::new(),
            outcome: None,
            reason: String::new(),
            tasks: vec![1],
            store_digest: BTreeMap::new(),
            digest_at: Some(open),
        };
        assert!(trail_writes(&tasks, &run).is_empty());
        let taken = store_digest(&tasks, &[1], &open);
        tasks.changes.push(change("2026-11-11T15:23:04Z", "files"));
        assert_ne!(
            store_digest(&tasks, &[1], &open)["changes"],
            taken["changes"]
        );
    }

    fn change(when: &str, field: &str) -> Change {
        Change {
            at: Some(at(when)),
            r#ref: "task-1".to_string(),
            field: field.to_string(),
            old: "a".to_string(),
            new: "b".to_string(),
            actor: "orchestrator".to_string(),
            reason: "r".to_string(),
        }
    }

    const STARTED: &str = "2026-11-11T12:00:00Z";

    fn changed(before: &Store, after: &Store) -> Vec<String> {
        let started = at(STARTED);
        let old = store_digest(before, &[1], &started);
        let new = store_digest(after, &[1], &started);
        old.keys()
            .filter(|section| old.get(*section) != new.get(*section))
            .cloned()
            .collect()
    }

    #[test]
    fn the_digest_names_every_section() {
        let digest = store_digest(&store(), &[1], &at(STARTED));
        assert_eq!(
            digest.keys().collect::<Vec<_>>(),
            ["changes", "milestones", "rest", "rows", "specs"]
        );
        assert!(digest.values().all(|hex| hex.len() == DIGEST_HEX_LEN));
    }

    #[test]
    fn status_and_agent_on_a_dispatched_row_leave_the_digest_alone() {
        let before = store();
        let mut after = store();
        after.items[0].status = Status::Done;
        after.items[0].agent = "plan-worker-lite".to_string();
        after.last_updated = Some(at("2026-11-11T13:00:00Z"));
        assert!(changed(&before, &after).is_empty());
    }

    #[test]
    fn status_on_a_row_the_run_did_not_dispatch_changes_the_rows_section() {
        let before = store();
        let mut after = store();
        after.items[1].status = Status::Done;
        assert_eq!(changed(&before, &after), ["rows"]);
    }

    #[test]
    fn any_other_field_on_a_dispatched_row_changes_the_rows_section() {
        let before = store();
        let mut after = store();
        after.items[0].files.push("src/extra.rs".to_string());
        assert_eq!(changed(&before, &after), ["rows"]);
    }

    #[test]
    fn a_spec_or_milestone_write_names_its_own_section() {
        let before = store();
        let mut after = store();
        after.specs[0].constraints.push("new".to_string());
        assert_eq!(changed(&before, &after), ["specs"]);

        let mut after = store();
        after.checkpoints.push(crate::tasks::schema::Checkpoint {
            id: "B".to_string(),
            rationale: String::new(),
        });
        assert_eq!(changed(&before, &after), ["milestones"]);
    }

    #[test]
    fn trail_rows_after_the_start_stay_out_of_the_digest_and_earlier_ones_count() {
        let mut before = store();
        before.changes.push(change("2026-11-11T11:00:00Z", "files"));
        let mut after = before.clone();
        after.changes.push(change("2026-11-11T12:30:00Z", "files"));
        assert!(changed(&before, &after).is_empty());

        let mut rewritten = before.clone();
        rewritten.changes[0].new = "c".to_string();
        assert_eq!(changed(&before, &rewritten), ["changes"]);
    }

    #[test]
    fn the_trail_window_runs_from_the_start_to_the_finish() {
        let mut tasks = store();
        tasks.changes = vec![
            change("2026-11-11T11:59:59Z", "before"),
            change("2026-11-11T12:00:00Z", "at-start"),
            change("2026-11-11T12:10:00Z", "during"),
            change("2026-11-11T12:20:01Z", "after"),
        ];
        let mut run = Run {
            step: "implement:A".to_string(),
            workflow: String::new(),
            args_hash: String::new(),
            run_id: "wf_1".to_string(),
            session: String::new(),
            started: at(STARTED),
            finished: Some(at("2026-11-11T12:20:00Z")),
            skip_reason: String::new(),
            outcome: None,
            reason: String::new(),
            tasks: vec![1],
            store_digest: BTreeMap::new(),
            digest_at: None,
        };
        let fields = |run: &Run| -> Vec<String> {
            trail_writes(&tasks, run)
                .iter()
                .map(|w| w["field"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(fields(&run), ["at-start", "during"]);
        assert_eq!(trail_writes(&tasks, &run)[0]["task"], json!(1));

        run.digest_at = Some(at("2026-11-11T11:59:00Z"));
        assert_eq!(fields(&run), ["before", "at-start", "during"]);

        run.digest_at = None;
        run.finished = None;
        assert_eq!(fields(&run), ["at-start", "during", "after"]);
    }

    #[test]
    fn a_digest_token_round_trips() {
        let digest = store_digest(&store(), &[1], &at(STARTED));
        let token = encode_digest(&digest);
        assert!(!token.contains(char::is_whitespace), "{token}");
        assert_eq!(decode_digest(&token).expect("decodes"), digest);
    }

    #[test]
    fn a_malformed_digest_token_is_refused() {
        let token = encode_digest(&store_digest(&store(), &[1], &at(STARTED)));
        let dropped = token.split(',').skip(1).collect::<Vec<_>>().join(",");
        let cases = [
            ("", "is not `<section>:<hex>`"),
            (&dropped as &str, "missing changes"),
            (&format!("{token},rows:0123456789abcdef"), "named twice"),
            (
                &format!("{dropped},plans:0123456789abcdef"),
                "not a digest section",
            ),
            (&token.replacen(':', ":X", 1), "lowercase hex"),
            (&token.to_uppercase(), "not a digest section"),
        ];
        for (input, expected) in cases {
            let message = decode_digest(input).expect_err(input).to_string();
            assert!(message.contains(expected), "{input}: {message}");
        }
    }

    #[test]
    fn a_digest_is_taken_only_for_an_implement_step() {
        let digest = run_store_digest("no-such-flow", "review", &[1], &at(STARTED));
        assert!(digest.is_empty());
    }
}
