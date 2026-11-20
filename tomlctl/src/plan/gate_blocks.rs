//! Parses the `verification` agent's returned text into the rows
//! `plan run gate --ndjson` takes, one per command it reports.
//!
//! A pass is one line, `outcome: pass | command: … | duration: … | summary:
//! … | log: …`; every other outcome is a block opened by a `command:` line
//! directly followed by an `outcome:` line, whose `tail:` runs on as raw log
//! text until the next entry. A command may hold a shell pipe and a summary
//! may hold ` | `, so the pass line is read on its known keys, never split.
//! A base rerun's text may also hold `skip: <command>` lines, each holding the
//! place of a command not rerun at base; they are read only when asked for.

use std::path::Path;

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::schema::CommandOutcome;
use crate::errors::{ErrorKind, tagged_arg_err};

type Row = Map<String, JsonValue>;

const BLOCKS_ARG: &str = "blocks";
const PASS_PREFIX: &str = "outcome: pass | command: ";
const COMMAND_PREFIX: &str = "command: ";
const OUTCOME_PREFIX: &str = "outcome: ";
const NOT_RUN_PREFIX: &str = "not_run:";
const SKIP_PREFIX: &str = "skip: ";

/// Each row with the 1-based line its entry opens on. Text that yields no row
/// is refused. With `skips`, a `skip: <command>` line is a row of its own,
/// `{"command": …, "skip": true}`; without it the line is ordinary text.
pub(super) fn parse(
    text: &str,
    read_ids: impl Fn(&Path) -> Option<Vec<u8>>,
    skips: bool,
) -> Result<Vec<(usize, Row)>> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let mut rows = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some(rest) = line.strip_prefix(PASS_PREFIX) {
            rows.push((index + 1, pass_row(index + 1, rest)?));
            index += 1;
        } else if skips && let Some(command) = line.strip_prefix(SKIP_PREFIX) {
            rows.push((index + 1, skip_row(index + 1, command)?));
            index += 1;
        } else if opens_block(&lines, index) {
            let (row, next) = block_row(&lines, index, &read_ids, skips)?;
            rows.push((index + 1, row));
            index = next;
        } else {
            index += 1;
        }
    }
    if rows.is_empty() {
        return Err(refusal(
            "the text holds no pass line and no `command:` block: pass the verification \
             agent's returned text as it was returned",
        ));
    }
    Ok(rows)
}

fn refusal(message: impl Into<String>) -> anyhow::Error {
    tagged_arg_err(ErrorKind::Validation, BLOCKS_ARG, message)
}

fn line_err(line: usize, message: impl std::fmt::Display) -> anyhow::Error {
    refusal(format!("line {line}: {message}"))
}

fn opens_block(lines: &[&str], index: usize) -> bool {
    lines[index].starts_with(COMMAND_PREFIX)
        && lines
            .get(index + 1)
            .is_some_and(|next| next.starts_with(OUTCOME_PREFIX))
}

/// Whether the line at `index` opens the next entry or ends the report.
fn ends_entry(lines: &[&str], index: usize, skips: bool) -> bool {
    let line = lines[index];
    line.starts_with(PASS_PREFIX)
        || line.starts_with(NOT_RUN_PREFIX)
        || (skips && line.starts_with(SKIP_PREFIX))
        || opens_block(lines, index)
}

fn skip_row(line: usize, command: &str) -> Result<Row> {
    if command.trim().is_empty() {
        return Err(line_err(line, "a `skip:` line names no command"));
    }
    let mut row = Row::new();
    row.insert("command".into(), json!(command));
    row.insert("skip".into(), json!(true));
    Ok(row)
}

/// `rest` is the pass line after its `outcome: pass | command: ` prefix. The
/// command runs to the last ` | duration: ` followed by an integer and
/// ` | summary: `, the summary to the last ` | log: `.
fn pass_row(line: usize, rest: &str) -> Result<Row> {
    let rest = match rest.rfind(" | retried: ") {
        Some(at) if !rest[at + 1..].contains(" | ") => &rest[..at],
        _ => rest,
    };
    let malformed = || {
        line_err(
            line,
            "a pass line reads `outcome: pass | command: <command> | duration: <n> | summary: \
             <text> | log: <path>`",
        )
    };
    let (head, log) = rest.rsplit_once(" | log: ").ok_or_else(malformed)?;
    let (command, duration, summary) = head
        .rmatch_indices(" | duration: ")
        .find_map(|(at, marker)| {
            let after = &head[at + marker.len()..];
            let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let summary = after[digits..].strip_prefix(" | summary: ")?;
            (digits > 0).then(|| (&head[..at], &after[..digits], summary))
        })
        .ok_or_else(malformed)?;
    if command.trim().is_empty() {
        return Err(malformed());
    }
    let mut row = Row::new();
    row.insert("command".into(), json!(command));
    row.insert("outcome".into(), json!(CommandOutcome::Pass.as_str()));
    row.insert("exit".into(), json!(0));
    row.insert(
        "duration_s".into(),
        json!(integer(line, "duration", duration)?),
    );
    row.insert("summary".into(), json!(summary_text(summary)));
    row.insert("log".into(), json!(log.trim()));
    Ok(row)
}

/// The row of the block opening at `start`, and the index of the first line
/// after it.
fn block_row(
    lines: &[&str],
    start: usize,
    read_ids: &impl Fn(&Path) -> Option<Vec<u8>>,
    skips: bool,
) -> Result<(Row, usize)> {
    let line = start + 1;
    let command = &lines[start][COMMAND_PREFIX.len()..];
    if command.trim().is_empty() {
        return Err(line_err(line, "a block's `command:` line names no command"));
    }
    let mut row = Row::new();
    row.insert("command".into(), json!(command));
    let mut listed_ids: Option<&str> = None;
    let mut index = start + 1;
    let mut in_tail = false;
    while index < lines.len() {
        if index > start + 1 && ends_entry(lines, index, skips) {
            break;
        }
        let text = lines[index];
        index += 1;
        if in_tail {
            continue;
        }
        let Some((key, value)) = text.split_once(':') else {
            continue;
        };
        let value = value.strip_prefix(' ').unwrap_or(value).trim_end();
        let key = match key {
            "tail" => {
                in_tail = true;
                continue;
            }
            "failed_ids" => {
                listed_ids.get_or_insert(value);
                continue;
            }
            "outcome" | "exit" | "duration_s" | "log" | "summary" => key,
            _ => continue,
        };
        if row.contains_key(key) {
            continue;
        }
        let parsed = match key {
            "outcome" => {
                let outcome = CommandOutcome::parse(value).ok_or_else(|| {
                    line_err(
                        line,
                        format!(
                            "unknown outcome `{value}` — expected one of {}",
                            CommandOutcome::VOCABULARY.join(", ")
                        ),
                    )
                })?;
                json!(outcome.as_str())
            }
            "exit" if value == "none" => JsonValue::Null,
            "exit" => json!(value.parse::<i64>().map_err(|_| line_err(
                line,
                format!("`exit: {value}` is not an integer or `none`")
            ))?),
            "duration_s" => json!(integer(line, "duration_s", value)?),
            "summary" => json!(summary_text(value)),
            _ => json!(value),
        };
        row.insert(key.into(), parsed);
    }
    let ids = failed_ids(&row, listed_ids, read_ids);
    if !ids.is_empty() {
        row.insert("failed_ids".into(), json!(ids));
    }
    Ok((row, index))
}

fn integer(line: usize, key: &str, value: &str) -> Result<u64> {
    value.trim().parse::<u64>().map_err(|_| {
        line_err(
            line,
            format!("`{key}: {value}` is not a whole number of seconds"),
        )
    })
}

fn summary_text(value: &str) -> &str {
    let value = value.trim();
    if value == "none" { "" } else { value }
}

/// The full list from `<log>.ids` beside the block's log, one id per line,
/// when that file can be read; otherwise the block's own `failed_ids:` value,
/// which the agent caps at twenty and joins with spaces.
fn failed_ids(
    row: &Row,
    listed: Option<&str>,
    read_ids: &impl Fn(&Path) -> Option<Vec<u8>>,
) -> Vec<String> {
    let log = row
        .get("log")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if !log.is_empty()
        && let Some(bytes) = read_ids(Path::new(&format!("{log}.ids")))
    {
        return String::from_utf8_lossy(&bytes)
            .lines()
            .map(|id| id.trim_end_matches('\r'))
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .collect();
    }
    let Some(listed) = listed else {
        return Vec::new();
    };
    let listed = match listed.trim_end().rsplit_once(" (+") {
        Some((ids, more))
            if more
                .strip_suffix(" more)")
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) =>
        {
            ids
        }
        _ => listed,
    };
    listed.split_whitespace().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_ids(_: &Path) -> Option<Vec<u8>> {
        None
    }

    #[test]
    fn a_pass_line_keeps_a_piped_command_and_a_piped_summary() {
        let text = "outcome: pass | command: a | grep b | duration: 7 | summary: Tests  2 failed | 10 passed | log: C:/t/c1.log | retried: transient\n";
        let rows = parse(text, no_ids, false).unwrap();
        let row = &rows[0].1;
        assert_eq!(row["command"], json!("a | grep b"));
        assert_eq!(row["summary"], json!("Tests  2 failed | 10 passed"));
        assert_eq!(row["log"], json!("C:/t/c1.log"));
        assert_eq!(row["exit"], json!(0));
        assert_eq!(row["duration_s"], json!(7));
    }

    #[test]
    fn a_retried_pass_line_keeps_its_log_path_clean() {
        let text = "outcome: pass | command: b | duration: 1 | summary: none | log: C:/t/c1t.log | retried: transient\r\n";
        let row = &parse(text, no_ids, false).unwrap()[0].1;
        assert_eq!(row["log"], json!("C:/t/c1t.log"));
        assert_eq!(row["summary"], json!(""));
    }

    #[test]
    fn listed_ids_drop_the_more_count() {
        let text =
            "command: t\noutcome: fail\nexit: 1\nfailed_ids: a::b c::d (+3 more)\ntail:\nx\n";
        let rows = parse(text, no_ids, false).unwrap();
        assert_eq!(rows[0].1["failed_ids"], json!(["a::b", "c::d"]));
        assert_eq!(rows[0].1["exit"], json!(1));
    }

    #[test]
    fn text_with_no_entry_is_refused() {
        assert!(parse("```\n\nnothing here\n```\n", no_ids, false).is_err());
    }

    #[test]
    fn a_skip_line_is_a_row_only_when_skips_are_read() {
        let text = "command: t\noutcome: fail\nexit: 1\ntail:\nx\nskip: cargo deploy\noutcome: pass | command: u | duration: 1 | summary: none | log: C:/t/u.log\n";
        let rows = parse(text, no_ids, true).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].0, 6);
        assert_eq!(rows[1].1["command"], json!("cargo deploy"));
        assert_eq!(rows[1].1["skip"], json!(true));

        let rows = parse(text, no_ids, false).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|(_, row)| row.get("skip").is_none()));
        assert!(parse("skip: \n", no_ids, true).is_err());
    }
}
