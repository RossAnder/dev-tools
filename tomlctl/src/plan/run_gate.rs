//! The `plan run gate` leaf: records one run of a milestone's gate as a
//! `[[gate_round]]`, keyed on `(milestone, round, seq)`, from one row per
//! command the caller ran, given as NDJSON or parsed from the verification
//! agent's returned text. A call without `--rerun` records a new run of the
//! round; with it, the call merges into the round's latest run, replacing the
//! commands it names. The tail of each non-pass command's log is kept under
//! `.claude/flows/<slug>/gates/`; a log that cannot be read stores `""` and
//! never fails the record. A failing run also records the digests of every
//! file the milestone's rows name. With `--base`, the rows are instead the
//! milestone base's rerun of the latest run's failing commands, matched to
//! them by order, and only the commands' `base_*` fields are written. With
//! `--accept`, a JSON payload records the user's settling of the round's
//! latest run as a `[[gate_accept]]`, checked against that run's commands.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value as JsonValue, json};
use sha2::{Digest, Sha256};

use super::emit::load_milestone_store;
use super::gate_blocks;
use super::schema::{
    AcceptedCommand, CommandOutcome, GateAccept, GateAcceptKind, GateCommand, GateOutcome,
    GateRound, PlanStore, TreeEntry,
};
use super::store;
use crate::cli::PlanRunGateArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::hex_lower;
use crate::io::{read_json_value_from_arg, read_ndjson_source, read_text_arg, repo_or_cwd_root};
use crate::items::{parse_ndjson_batch, row_err, row_string, row_strings};
use crate::output::{Rows, print_json_compact, print_rows_compact};
use crate::tasks::schema::{Store, normal_path};

type Row = Map<String, JsonValue>;

const ROW_KEYS: &[&str] = &[
    "command",
    "outcome",
    "exit",
    "duration_s",
    "summary",
    "failed_ids",
    "log",
];

const SKIP_KEY: &str = "skip";
/// Both digests of a tree entry whose path could not be read as a file.
pub(super) const DELETED: &str = "deleted";

const GATES_DIR: &str = "gates";
const TAIL_LINES: usize = 200;
const TAIL_BYTES: usize = 64 * 1024;

/// One parsed row; `source` is the log path as the caller gave it.
struct Staged {
    command: GateCommand,
    source: String,
}

pub(super) fn run(args: PlanRunGateArgs) -> Result<()> {
    let PlanRunGateArgs {
        target,
        milestone,
        round,
        ndjson,
        blocks,
        rerun,
        base,
        accept,
        json,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    if accept {
        let json = json.ok_or_else(|| refusal("`plan run gate --accept` needs `--json <src>`"))?;
        return record_accept(slug, &milestone, round, &json, &integrity);
    }
    let tasks = load_milestone_store(slug, &milestone, integrity.verify_integrity)?;
    check_file_safe(&milestone)?;
    let rows = match (ndjson, blocks) {
        (_, Some(blocks)) => {
            gate_blocks::parse(&read_blocks_source(&blocks)?, read_temp_log, base)?
        }
        (Some(ndjson), None) => parse_ndjson_batch(&read_ndjson_source(&ndjson)?)?,
        (None, None) => {
            return Err(refusal(
                "`plan run gate` needs `--ndjson <src>` or `--blocks <src>`",
            ));
        }
    };
    let plan_path = store::resolve(slug)?;
    if base {
        return record_base(BaseCall {
            slug,
            plan_path: &plan_path,
            integrity: &integrity,
            tasks: &tasks,
            milestone: &milestone,
            round,
            rows: &rows,
        });
    }
    let staged = parse_rows(&rows)?;
    let files = milestone_files(&tasks, &milestone);
    let root = repo_or_cwd_root()?;
    let gates = store::flow_dir(&plan_path).join(GATES_DIR);
    let relative_gates = format!(".claude/flows/{slug}/{GATES_DIR}");
    let at = crate::time::now_toml_datetime()?;

    let (outcome, commands, unread) = store::mutate(&plan_path, &integrity, |plan| {
        let index = target_round(plan, &milestone, round, rerun, at)?;
        let gate = &mut plan.gate_rounds[index];
        let seq = gate.seq;
        let mut unread = Vec::new();
        for Staged {
            mut command,
            source,
        } in staged
        {
            let position = match gate
                .commands
                .iter()
                .position(|c| c.command == command.command)
            {
                Some(position) => position,
                None => {
                    gate.commands.push(command.clone());
                    gate.commands.len() - 1
                }
            };
            if command.outcome != CommandOutcome::Pass && !source.is_empty() {
                let name = format!("{milestone}-r{round}-s{seq}-c{}.log", position + 1);
                if keep_tail(Path::new(&source), &gates, &name)? {
                    command.log = format!("{relative_gates}/{name}");
                } else {
                    unread.push(command.command.clone());
                }
            }
            gate.commands[position] = command;
        }
        gate.at = at;
        gate.outcome = round_outcome(&gate.commands);
        gate.tree = if gate.outcome == GateOutcome::Fail {
            tree(&root, &files)
        } else {
            Vec::new()
        };
        let commands: Vec<JsonValue> = gate
            .commands
            .iter()
            .map(|c| json!({"command": c.command, "outcome": c.outcome.as_str(), "log": c.log}))
            .collect();
        Ok((gate.outcome, commands, unread))
    })?;

    print_rows_compact(
        &json!({
            "ok": true,
            "milestone": milestone,
            "round": round,
            "outcome": outcome.as_str(),
            "commands": commands,
            "unread": unread,
        }),
        Rows::Field("commands"),
    )
}

fn refusal(message: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

/// The milestone id names the kept log files, so it must not reach outside
/// the gates directory or carry a character a file name cannot.
fn check_file_safe(milestone: &str) -> Result<()> {
    let safe = !milestone.is_empty()
        && !milestone.starts_with('.')
        && milestone
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if safe {
        Ok(())
    } else {
        Err(refusal(format!(
            "`--milestone {milestone}` cannot name a gate log file: use letters, digits, `-`, `_` and `.`"
        )))
    }
}

/// The index of the gate run this call writes: a new one with the next `seq`,
/// or under `rerun` the round's latest, refused when the round has none.
fn target_round(
    plan: &mut PlanStore,
    milestone: &str,
    round: u32,
    rerun: bool,
    at: toml::value::Datetime,
) -> Result<usize> {
    let latest = latest_run(plan, milestone, round);
    if rerun {
        return latest.map(|(index, _)| index).ok_or_else(|| {
            refusal(format!(
                "`--rerun` needs a recorded gate run of milestone {milestone} round {round}, \
                 and there is none: record the round without `--rerun` first"
            ))
        });
    }
    let seq = latest.map_or(0, |(_, seq)| seq).saturating_add(1);
    plan.gate_rounds.push(GateRound {
        milestone: milestone.to_string(),
        round,
        seq,
        at,
        outcome: GateOutcome::Pass,
        commands: Vec::new(),
        tree: Vec::new(),
    });
    Ok(plan.gate_rounds.len() - 1)
}

/// The index and `seq` of the round's latest recorded run.
fn latest_run(plan: &PlanStore, milestone: &str, round: u32) -> Option<(usize, u32)> {
    plan.gate_rounds
        .iter()
        .enumerate()
        .filter(|(_, g)| g.milestone == milestone && g.round == round)
        .max_by_key(|(_, g)| g.seq)
        .map(|(index, g)| (index, g.seq))
}

struct BaseCall<'a> {
    slug: &'a str,
    plan_path: &'a Path,
    integrity: &'a crate::cli::WriteIntegrityArgs,
    tasks: &'a Store,
    milestone: &'a str,
    round: u32,
    rows: &'a [(usize, Row)],
}

/// One row of a base call: a command rerun at base, or the place of one that
/// was not.
enum BaseRow {
    Ran(GateCommand),
    Skip(String),
}

/// Writes the `base_*` fields of every `fail` command of the round's latest
/// run, the i-th row of the call answering the i-th such command.
fn record_base(call: BaseCall<'_>) -> Result<()> {
    let BaseCall {
        slug,
        plan_path,
        integrity,
        tasks,
        milestone,
        round,
        rows,
    } = call;
    let base_rows = rows
        .iter()
        .map(|(line, row)| Ok((*line, parse_base_row(*line, row)?)))
        .collect::<Result<Vec<_>>>()?;
    let gates: BTreeSet<&str> = tasks
        .milestones
        .iter()
        .filter(|m| m.id == milestone)
        .flat_map(|m| m.gates.iter().map(String::as_str))
        .collect();

    let commands = store::mutate(plan_path, integrity, |plan| {
        let (index, seq) = latest_run(plan, milestone, round).ok_or_else(|| {
            refusal(format!(
                "`--base` needs a recorded gate run of milestone {milestone} round {round}, \
                 and there is none: record it first with `tomlctl plan run gate --slug {slug} \
                 --milestone {milestone} --round {round} --blocks <src>`"
            ))
        })?;
        let gate = &mut plan.gate_rounds[index];
        let failing: Vec<usize> = gate
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| c.outcome == CommandOutcome::Fail)
            .map(|(position, _)| position)
            .collect();
        if failing.len() != base_rows.len() {
            return Err(refusal(format!(
                "`--base` gave {} rows, and run {seq} of milestone {milestone} round {round} \
                 has {} failing commands: give one per failing command, in the order the run \
                 lists them, with `skip: <command>` in the place of one not rerun at base",
                base_rows.len(),
                failing.len()
            )));
        }
        for (&position, (line, base_row)) in failing.iter().zip(&base_rows) {
            if let BaseRow::Skip(named) = base_row
                && *named != gate.commands[position].command
            {
                return Err(row_err(
                    *line,
                    format!(
                        "`skip: {named}` stands in the place of `{}`, the run's failing \
                         command at that position",
                        gate.commands[position].command
                    ),
                ));
            }
        }
        for (&position, (_, base_row)) in failing.iter().zip(&base_rows) {
            let command = &mut gate.commands[position];
            let is_gate = gates.contains(command.command.as_str());
            apply_base(command, base_row, is_gate);
        }
        Ok(failing
            .iter()
            .map(|&position| {
                let c = &gate.commands[position];
                json!({
                    "command": c.command,
                    "base_checked": c.base_checked,
                    "base_failed_ids": c.base_failed_ids,
                    "base_whole_failed": c.base_whole_failed,
                    "base_failure": is_base_failure(c),
                })
            })
            .collect::<Vec<JsonValue>>())
    })?;

    print_rows_compact(
        &json!({
            "ok": true,
            "milestone": milestone,
            "round": round,
            "base": true,
            "commands": commands,
        }),
        Rows::Field("commands"),
    )
}

fn parse_base_row(line: usize, row: &Row) -> Result<BaseRow> {
    match row.get(SKIP_KEY) {
        None | Some(JsonValue::Bool(false)) => {
            let mut row = row.clone();
            row.remove(SKIP_KEY);
            Ok(BaseRow::Ran(parse_row(line, &row)?.command))
        }
        Some(JsonValue::Bool(true)) => {
            if let Some(key) = row
                .keys()
                .find(|key| !matches!(key.as_str(), "command" | SKIP_KEY))
            {
                return Err(row_err(
                    line,
                    format!("a `skip` row holds only `command` and `skip`, not `{key}`"),
                ));
            }
            row_string(line, "command", row.get("command"))?
                .filter(|c| !c.trim().is_empty())
                .map(BaseRow::Skip)
                .ok_or_else(|| row_err(line, "a `skip` row needs a non-empty `command`"))
        }
        Some(_) => Err(row_err(line, "`skip` must be a boolean")),
    }
}

/// A base result that timed out, like a skip, leaves the command unchecked.
/// A failure listing no ids stands for every id the command failed with.
fn apply_base(command: &mut GateCommand, base_row: &BaseRow, is_gate: bool) {
    command.base_checked = false;
    command.base_failed_ids.clear();
    command.base_whole_failed = false;
    let BaseRow::Ran(result) = base_row else {
        return;
    };
    match result.outcome {
        CommandOutcome::Timeout => {}
        CommandOutcome::Pass | CommandOutcome::Flaky => command.base_checked = true,
        CommandOutcome::Fail if result.failed_ids.is_empty() => {
            command.base_checked = true;
            command.base_failed_ids = command.failed_ids.clone();
            command.base_whole_failed = is_gate;
        }
        CommandOutcome::Fail => {
            command.base_checked = true;
            command.base_failed_ids = command
                .failed_ids
                .iter()
                .filter(|id| result.failed_ids.contains(id))
                .cloned()
                .collect();
        }
    }
}

/// `base_whole_failed` is written only for one of the milestone's gates.
fn is_base_failure(command: &GateCommand) -> bool {
    if !command.base_checked {
        return false;
    }
    let ids: BTreeSet<&String> = command.failed_ids.iter().collect();
    let base: BTreeSet<&String> = command.base_failed_ids.iter().collect();
    (!ids.is_empty() && ids == base) || command.base_whole_failed
}

/// A gate acceptance payload, checked for shape before the stores are read.
struct AcceptPayload {
    kind: GateAcceptKind,
    why: String,
    backlog: String,
    commands: Vec<AcceptedCommand>,
}

/// Upserts a `[[gate_accept]]` after checking every entry against the round's
/// latest run. An `accept` and an `extra-round` share the `(milestone, round)`
/// key; a `base` record is keyed beside them on `kind` too.
fn record_accept(
    slug: &str,
    milestone: &str,
    round: u32,
    json: &str,
    integrity: &crate::cli::WriteIntegrityArgs,
) -> Result<()> {
    let payload = parse_accept(&read_json_value_from_arg(json).context("parsing --json")?)?;
    load_milestone_store(slug, milestone, integrity.verify_integrity)?;
    let at = crate::time::now_toml_datetime()?;
    let kind = payload.kind;
    let is_base = kind == GateAcceptKind::Base;

    store::mutate(&store::resolve(slug)?, integrity, |plan| {
        let (index, seq) = latest_run(plan, milestone, round).ok_or_else(|| {
            refusal(format!(
                "`--accept` needs a recorded gate run of milestone {milestone} round {round}, \
                 and there is none"
            ))
        })?;
        check_accept(&plan.gate_rounds[index], seq, &payload)?;
        let record = GateAccept {
            milestone: milestone.to_string(),
            round,
            at,
            kind,
            why: payload.why.clone(),
            backlog: payload.backlog.clone(),
            commands: payload.commands.clone(),
        };
        match plan.gate_accepts.iter_mut().find(|a| {
            a.milestone == milestone
                && a.round == round
                && (a.kind == GateAcceptKind::Base) == is_base
        }) {
            Some(earlier) => *earlier = record,
            None => plan.gate_accepts.push(record),
        }
        Ok(())
    })?;

    print_json_compact(&json!({
        "ok": true,
        "milestone": milestone,
        "round": round,
        "kind": kind.as_str(),
    }))
}

const ACCEPT_KEYS: &[&str] = &["kind", "why", "backlog", "commands"];
const ACCEPT_COMMAND_KEYS: &[&str] = &["command", "failed_ids"];

fn parse_accept(value: &JsonValue) -> Result<AcceptPayload> {
    let object = value.as_object().ok_or_else(|| {
        refusal(format!(
            "`--json` must be a JSON object with {}",
            ACCEPT_KEYS.join(", ")
        ))
    })?;
    check_keys(object, ACCEPT_KEYS, "the payload")?;
    let raw_kind = object
        .get("kind")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            refusal(format!(
                "the payload: `kind` must be one of {}",
                GateAcceptKind::VOCABULARY.join(", ")
            ))
        })?;
    let kind = GateAcceptKind::parse(raw_kind).ok_or_else(|| {
        refusal(format!(
            "the payload: unknown `kind` `{raw_kind}` — expected one of {}",
            GateAcceptKind::VOCABULARY.join(", ")
        ))
    })?;
    let why = match object.get("why").and_then(JsonValue::as_str).map(str::trim) {
        Some(why) if !why.is_empty() => why.to_string(),
        _ => return Err(refusal("the payload: `why` must be a non-empty string")),
    };
    let backlog = match object.get("backlog") {
        None | Some(JsonValue::Null) => String::new(),
        Some(value) => value
            .as_str()
            .map(|b| b.trim().to_string())
            .ok_or_else(|| refusal("the payload: `backlog` must be a string"))?,
    };
    match (kind, backlog.is_empty()) {
        (GateAcceptKind::ExtraRound, false) => {
            return Err(refusal(
                "the payload: `backlog` must be empty for an `extra-round`",
            ));
        }
        (GateAcceptKind::Accept | GateAcceptKind::Base, true) => {
            return Err(refusal(format!(
                "the payload: `backlog` must name the follow-up backlog item for `{}`",
                kind.as_str()
            )));
        }
        _ => {}
    }

    let raw_commands = object
        .get("commands")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| refusal("the payload: `commands` must be an array of objects"))?;
    let mut commands: Vec<AcceptedCommand> = Vec::with_capacity(raw_commands.len());
    for (index, entry) in raw_commands.iter().enumerate() {
        let at = format!("commands[{index}]");
        let entry = entry
            .as_object()
            .ok_or_else(|| refusal(format!("{at} must be an object")))?;
        check_keys(entry, ACCEPT_COMMAND_KEYS, &at)?;
        let command = entry
            .get("command")
            .and_then(JsonValue::as_str)
            .filter(|c| !c.trim().is_empty())
            .ok_or_else(|| refusal(format!("{at}: `command` must be a non-empty string")))?
            .to_string();
        if commands.iter().any(|c| c.command == command) {
            return Err(refusal(format!(
                "{at}: command `{command}` is named twice; name each command once"
            )));
        }
        let failed_ids = match entry.get("failed_ids") {
            None | Some(JsonValue::Null) => Vec::new(),
            Some(JsonValue::Array(ids)) => ids
                .iter()
                .map(|id| id.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| refusal(format!("{at}: `failed_ids` must hold only strings")))?,
            Some(_) => {
                return Err(refusal(format!(
                    "{at}: `failed_ids` must be an array of strings"
                )));
            }
        };
        commands.push(AcceptedCommand {
            command,
            failed_ids,
        });
    }
    if commands.is_empty() {
        return Err(refusal(
            "the payload: `commands` must name at least one failing command",
        ));
    }
    Ok(AcceptPayload {
        kind,
        why,
        backlog,
        commands,
    })
}

/// Each entry against the round's latest run: a `fail` command with ids that
/// run recorded, a base failure for `base`, and for `accept` every failing
/// command named. A run holding a `timeout` is refused whatever the kind.
fn check_accept(run: &GateRound, seq: u32, payload: &AcceptPayload) -> Result<()> {
    let milestone = &run.milestone;
    let round = run.round;
    let name = format!("run {seq} of milestone {milestone} round {round}");
    if let Some(timed_out) = run
        .commands
        .iter()
        .find(|c| c.outcome == CommandOutcome::Timeout)
    {
        return Err(refusal(format!(
            "{name} holds `{}` as `timeout`: rerun it with a larger budget and record that \
             before an acceptance",
            timed_out.command
        )));
    }
    for (index, entry) in payload.commands.iter().enumerate() {
        let at = format!("commands[{index}]");
        let recorded = run
            .commands
            .iter()
            .find(|c| c.command == entry.command)
            .ok_or_else(|| {
                refusal(format!(
                    "{at}: `{}` is not a command of {name}",
                    entry.command
                ))
            })?;
        if recorded.outcome != CommandOutcome::Fail {
            return Err(refusal(format!(
                "{at}: `{}` is `{}` in {name}, not `fail`",
                entry.command,
                recorded.outcome.as_str()
            )));
        }
        let (allowed, what) = if payload.kind == GateAcceptKind::Base {
            if !recorded.base_checked
                || (!recorded.base_whole_failed && recorded.base_failed_ids.is_empty())
            {
                return Err(refusal(format!(
                    "{at}: `{}` is not a base or partial base failure of {name}",
                    entry.command
                )));
            }
            if entry.failed_ids.is_empty() && !recorded.base_whole_failed {
                return Err(refusal(format!(
                    "{at}: `failed_ids` must name the base failures `{}` waives; a command \
                     alone is named only when it failed whole at base",
                    entry.command
                )));
            }
            (&recorded.base_failed_ids, "base_failed_ids")
        } else {
            (&recorded.failed_ids, "failed_ids")
        };
        if let Some(stray) = entry.failed_ids.iter().find(|id| !allowed.contains(id)) {
            return Err(refusal(format!(
                "{at}: failed id `{stray}` is not one of `{}`'s {what} in {name}",
                entry.command
            )));
        }
    }
    if payload.kind == GateAcceptKind::Accept
        && let Some(missing) = run.commands.iter().find(|c| {
            c.outcome == CommandOutcome::Fail
                && !payload.commands.iter().any(|e| e.command == c.command)
        })
    {
        return Err(refusal(format!(
            "commands: an `accept` names every failing command of {name}, and `{}` is missing",
            missing.command
        )));
    }
    Ok(())
}

fn check_keys(object: &Row, allowed: &[&str], at: &str) -> Result<()> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(refusal(format!(
            "{at}: unknown key `{key}` — expected one of {}",
            allowed.join(", ")
        ))),
        None => Ok(()),
    }
}

/// Every path a row of the milestone names in its `files`, normalised.
fn milestone_files(tasks: &Store, milestone: &str) -> BTreeSet<String> {
    tasks
        .items
        .iter()
        .filter(|row| row.checkpoint == milestone)
        .flat_map(|row| row.files.iter().map(|file| normal_path(file)))
        .filter(|file| !file.is_empty())
        .collect()
}

fn tree(root: &Path, files: &BTreeSet<String>) -> Vec<TreeEntry> {
    files
        .iter()
        .map(|path| {
            let (raw, stripped) = whitespace_digest(&root.join(path));
            TreeEntry {
                path: path.clone(),
                raw,
                stripped,
            }
        })
        .collect()
}

/// The SHA-256 of a file's bytes, then of those bytes with every ASCII
/// whitespace byte removed; both are `DELETED` when the path cannot be read
/// as a file. Reads the disk only, never git.
pub(super) fn whitespace_digest(path: &Path) -> (String, String) {
    match fs::read(path) {
        Ok(bytes) => {
            let stripped: Vec<u8> = bytes
                .iter()
                .copied()
                .filter(|byte| !byte.is_ascii_whitespace())
                .collect();
            (
                hex_lower(&Sha256::digest(&bytes)),
                hex_lower(&Sha256::digest(&stripped)),
            )
        }
        Err(_) => (DELETED.to_string(), DELETED.to_string()),
    }
}

/// Pass when every command passed or was flaky; timeout when every other
/// command timed out; fail otherwise.
fn round_outcome(commands: &[GateCommand]) -> GateOutcome {
    let mut rest = commands
        .iter()
        .filter(|c| !matches!(c.outcome, CommandOutcome::Pass | CommandOutcome::Flaky))
        .peekable();
    if rest.peek().is_none() {
        GateOutcome::Pass
    } else if rest.all(|c| c.outcome == CommandOutcome::Timeout) {
        GateOutcome::Timeout
    } else {
        GateOutcome::Fail
    }
}

/// The verification agent's text from a file, or from stdin for `-`.
fn read_blocks_source(src: &str) -> Result<String> {
    if src == "-" {
        read_text_arg("-")
    } else {
        fs::read_to_string(src).with_context(|| format!("reading `--blocks` file `{src}`"))
    }
}

/// Every row checked before anything is written, so a refused row persists nothing.
fn parse_rows(rows: &[(usize, Row)]) -> Result<Vec<Staged>> {
    let mut staged: Vec<Staged> = Vec::with_capacity(rows.len());
    for (line, row) in rows {
        let parsed = parse_row(*line, row)?;
        if staged
            .iter()
            .any(|s| s.command.command == parsed.command.command)
        {
            return Err(row_err(
                *line,
                format!(
                    "command `{}` is named twice in the batch",
                    parsed.command.command
                ),
            ));
        }
        staged.push(parsed);
    }
    Ok(staged)
}

fn parse_row(line: usize, row: &Row) -> Result<Staged> {
    if row.contains_key(SKIP_KEY) {
        return Err(row_err(line, "a `skip` row is accepted only with `--base`"));
    }
    if let Some(key) = row.keys().find(|key| !ROW_KEYS.contains(&key.as_str())) {
        return Err(row_err(
            line,
            format!(
                "unknown key `{key}` — expected one of {}",
                ROW_KEYS.join(", ")
            ),
        ));
    }
    let command = row_string(line, "command", row.get("command"))?
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| row_err(line, "a row needs a non-empty `command`"))?;
    let raw = row_string(line, "outcome", row.get("outcome"))?
        .ok_or_else(|| row_err(line, "a row needs `outcome`"))?;
    let outcome = CommandOutcome::parse(&raw).ok_or_else(|| {
        row_err(
            line,
            format!(
                "unknown outcome `{raw}` — expected one of {}",
                CommandOutcome::VOCABULARY.join(", ")
            ),
        )
    })?;
    let exit = match row.get("exit") {
        None | Some(JsonValue::Null) => None,
        Some(value) => Some(
            value
                .as_i64()
                .ok_or_else(|| row_err(line, "`exit` must be an integer"))?,
        ),
    };
    let duration_s = match row.get("duration_s") {
        None => 0,
        Some(value) => duration(value)
            .ok_or_else(|| row_err(line, "`duration_s` must be a non-negative number"))?,
    };
    let source = row_string(line, "log", row.get("log"))?.unwrap_or_default();
    Ok(Staged {
        command: GateCommand {
            command,
            outcome,
            exit,
            duration_s,
            summary: row_string(line, "summary", row.get("summary"))?.unwrap_or_default(),
            failed_ids: row_strings(line, "failed_ids", row.get("failed_ids"))?.unwrap_or_default(),
            log: String::new(),
            base_checked: false,
            base_failed_ids: Vec::new(),
            base_whole_failed: false,
        },
        source,
    })
}

/// Whole seconds; a fractional count rounds to the nearest.
fn duration(value: &JsonValue) -> Option<u64> {
    if let Some(n) = value.as_u64() {
        return Some(n);
    }
    let f = value.as_f64()?;
    if !f.is_finite() || f < 0.0 {
        return None;
    }
    // A finite, non-negative f64 rounded; the cast saturates past u64::MAX.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(f.round() as u64)
}

/// Writes the tail of `source` to `gates/name` and reports whether it did.
/// A source outside the OS temp directory, or one that cannot be read, is
/// skipped: the kept tail is committed with the flow's stores, so the verb
/// copies only what the verification agent's `mktemp -d` could have written.
/// Only a failure to write the kept file is an error.
fn keep_tail(source: &Path, gates: &Path, name: &str) -> Result<bool> {
    let Some(bytes) = read_temp_log(source) else {
        return Ok(false);
    };
    fs::create_dir_all(gates).with_context(|| format!("creating {}", gates.display()))?;
    let kept: PathBuf = gates.join(name);
    let failures = without_passes(&bytes);
    fs::write(&kept, tail(&failures)).with_context(|| format!("writing {}", kept.display()))?;
    Ok(true)
}

/// `bytes` less the lines that only report a passing test: nextest's `PASS [`
/// and libtest's `test <name> ... ok`. A suite's passes would otherwise fill the
/// whole tail and push its failure detail out.
fn without_passes(bytes: &[u8]) -> Vec<u8> {
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| {
            let line = line.trim_ascii();
            let pass = line.starts_with(b"PASS [")
                || (line.starts_with(b"test ") && line.ends_with(b" ... ok"));
            !pass
        })
        .flatten()
        .copied()
        .collect()
}

/// The bytes of `source` when it resolves to a file under the OS temp
/// directory, where the verification agent writes its logs.
fn read_temp_log(source: &Path) -> Option<Vec<u8>> {
    let temp = std::env::temp_dir().canonicalize().ok()?;
    let resolved = source.canonicalize().ok()?;
    if !resolved.starts_with(&temp) || !resolved.is_file() {
        return None;
    }
    fs::read(&resolved).ok()
}

/// The last `TAIL_LINES` lines, then the last `TAIL_BYTES` of those, starting
/// on a UTF-8 character boundary.
fn tail(bytes: &[u8]) -> &[u8] {
    let body = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let mut start = 0;
    let mut seen = 0;
    for (index, byte) in body.iter().enumerate().rev() {
        if *byte == b'\n' {
            seen += 1;
            if seen == TAIL_LINES {
                start = index + 1;
                break;
            }
        }
    }
    if bytes.len() - start > TAIL_BYTES {
        start = bytes.len() - TAIL_BYTES;
        while start < bytes.len() && (bytes[start] & 0b1100_0000) == 0b1000_0000 {
            start += 1;
        }
    }
    &bytes[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(outcome: CommandOutcome) -> GateCommand {
        GateCommand {
            command: "c".to_string(),
            outcome,
            exit: None,
            duration_s: 0,
            summary: String::new(),
            failed_ids: Vec::new(),
            log: String::new(),
            base_checked: false,
            base_failed_ids: Vec::new(),
            base_whole_failed: false,
        }
    }

    #[test]
    fn the_round_outcome_follows_its_commands() {
        use CommandOutcome::*;
        let outcome = |list: &[CommandOutcome]| {
            round_outcome(&list.iter().map(|o| command(*o)).collect::<Vec<_>>())
        };
        assert_eq!(outcome(&[Pass, Flaky]), GateOutcome::Pass);
        assert_eq!(outcome(&[Pass, Timeout]), GateOutcome::Timeout);
        assert_eq!(outcome(&[Timeout, Fail]), GateOutcome::Fail);
    }

    #[test]
    fn the_tail_keeps_the_last_lines_then_the_last_bytes() {
        let text: String = (1..=300).map(|n| format!("line {n}\n")).collect();
        let kept = String::from_utf8(tail(text.as_bytes()).to_vec()).unwrap();
        assert!(kept.starts_with("line 101\n") && kept.ends_with("line 300\n"));

        let long = format!("{}\nend\n", "é".repeat(TAIL_BYTES));
        let kept = tail(long.as_bytes());
        assert!(kept.len() <= TAIL_BYTES);
        assert!(std::str::from_utf8(kept).unwrap().ends_with("\nend\n"));
    }

    #[test]
    fn the_kept_log_drops_pass_lines_so_an_early_failure_survives() {
        let mut log = String::from("    Starting 3002 tests across 1 binary\n");
        log.push_str("        FAIL [   0.120s] it plan_check::a_failing_case\r\n");
        log.push_str("--- STDERR:              it plan_check::a_failing_case ---\n");
        log.push_str("thread 'a_failing_case' panicked at tests/plan_check.rs:9:5\n");
        log.push_str("test plan_check::libtest_case ... FAILED\n");
        for n in 0..3000 {
            log.push_str(&format!("        PASS [   0.010s] it case_{n}\n"));
            log.push_str(&format!("test case_{n} ... ok\r\n"));
        }
        log.push_str("     Summary [  12.000s] 3002 tests run: 3000 passed, 2 failed\n");

        let kept = String::from_utf8(tail(&without_passes(log.as_bytes())).to_vec()).unwrap();
        assert!(!kept.contains("PASS ["), "{kept}");
        assert!(!kept.contains(" ... ok"), "{kept}");
        for line in [
            "FAIL [   0.120s] it plan_check::a_failing_case\r\n",
            "--- STDERR:",
            "panicked at tests/plan_check.rs:9:5",
            "test plan_check::libtest_case ... FAILED",
            "3000 passed, 2 failed\n",
        ] {
            assert!(kept.contains(line), "{line}: {kept}");
        }
    }
}
