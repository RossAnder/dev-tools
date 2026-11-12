//! The `plan run gate` leaf: records one run of a milestone's gate as a
//! `[[gate_round]]`, keyed on `(milestone, round, seq)`, from one row per
//! command the caller ran. A call without `--rerun` records a new run of the
//! round; with it, the call merges into the round's latest run, replacing the
//! commands it names. The tail of each non-pass command's log is kept under
//! `.claude/flows/<slug>/gates/`; a log that cannot be read stores `""` and
//! never fails the record.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value as JsonValue, json};

use super::emit::load_milestone_store;
use super::schema::{CommandOutcome, GateCommand, GateOutcome, GateRound, PlanStore};
use super::store;
use crate::cli::PlanRunGateArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::read_ndjson_source;
use crate::items::{parse_ndjson_batch, row_err, row_string, row_strings};
use crate::output::{Rows, print_rows_compact};

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
        rerun,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    load_milestone_store(slug, &milestone, integrity.verify_integrity)?;
    check_file_safe(&milestone)?;
    let staged = parse_rows(&read_ndjson_source(&ndjson)?)?;
    let plan_path = store::resolve(slug)?;
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
    let latest = plan
        .gate_rounds
        .iter()
        .enumerate()
        .filter(|(_, g)| g.milestone == milestone && g.round == round)
        .max_by_key(|(_, g)| g.seq)
        .map(|(index, g)| (index, g.seq));
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
    });
    Ok(plan.gate_rounds.len() - 1)
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

/// Every row checked before anything is written, so a refused row persists nothing.
fn parse_rows(text: &str) -> Result<Vec<Staged>> {
    let rows = parse_ndjson_batch(text)?;
    let mut staged: Vec<Staged> = Vec::with_capacity(rows.len());
    for (line, row) in &rows {
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
    fs::write(&kept, tail(&bytes)).with_context(|| format!("writing {}", kept.display()))?;
    Ok(true)
}

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
}
