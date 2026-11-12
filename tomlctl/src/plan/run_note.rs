//! Parses the block that opens a worker's run note.
//!
//! A note opens `Status: <word> — <summary>`; blank lines may follow, then one
//! fenced block whose info string is exactly `toml run-note`. A note with no
//! such fence reads as `absent` and keeps its prose `TANGENTIAL:` lines; a
//! fence that is not valid TOML, holds an unknown key or follows prose reads
//! as `malformed` and yields the Status line alone.

use std::fmt;

use toml::Table;
use toml::Value as TomlValue;

use super::schema::{CheckResult, CheckRun, Deviation, NoteBlock, ProposedHandover, Tangential};

const INFO_STRING: &str = "toml run-note";

/// A run note read from its text. Every list is empty, and `handover` `None`,
/// when the block is malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunNote {
    pub(crate) status: String,
    pub(crate) summary: String,
    pub(crate) block: NoteBlock,
    /// Why the block is malformed, `""` otherwise.
    pub(crate) reason: String,
    pub(crate) files_touched: Vec<String>,
    pub(crate) deviations: Vec<Deviation>,
    pub(crate) tangential: Vec<Tangential>,
    pub(crate) checks: Vec<CheckRun>,
    pub(crate) handover: Option<ProposedHandover>,
}

/// The note's first line is not a `Status:` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoStatusLine {
    pub(crate) first_line: String,
}

impl fmt::Display for NoStatusLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the note's first line is not a `Status: <word> — <summary>` line: `{}`",
            self.first_line
        )
    }
}

impl std::error::Error for NoStatusLine {}

/// The parts of a block, before they are copied into a [`RunNote`].
#[derive(Default)]
struct BlockFields {
    files_touched: Vec<String>,
    deviations: Vec<Deviation>,
    tangential: Vec<Tangential>,
    checks: Vec<CheckRun>,
    handover: Option<ProposedHandover>,
}

pub(crate) fn parse(text: &str) -> Result<RunNote, NoStatusLine> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let (status, summary) = status_line(first).ok_or_else(|| NoStatusLine {
        first_line: first.trim_end().to_string(),
    })?;
    let rest: Vec<&str> = lines.collect();

    let mut note = RunNote {
        status,
        summary,
        block: NoteBlock::Absent,
        reason: String::new(),
        files_touched: Vec::new(),
        deviations: Vec::new(),
        tangential: Vec::new(),
        checks: Vec::new(),
        handover: None,
    };

    match find_block(&rest) {
        None => note.tangential = prose_tangential(&rest),
        Some(Err(reason)) => {
            note.block = NoteBlock::Malformed;
            note.reason = reason;
        }
        Some(Ok(body)) => match read_block(&body) {
            Ok(fields) => {
                note.block = NoteBlock::Present;
                note.files_touched = fields.files_touched;
                note.deviations = fields.deviations;
                note.tangential = fields.tangential;
                note.checks = fields.checks;
                note.handover = fields.handover;
            }
            Err(reason) => {
                note.block = NoteBlock::Malformed;
                note.reason = reason;
            }
        },
    }
    Ok(note)
}

/// `Status: <word> — <summary>`, with the dash optional and a hyphen or en
/// dash accepted in its place.
fn status_line(line: &str) -> Option<(String, String)> {
    let rest = line.trim_end().strip_prefix("Status:")?.trim_start();
    let word_len = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(rest.len());
    let word = &rest[..word_len];
    if word.is_empty() || word.starts_with('-') {
        return None;
    }
    let tail = rest[word_len..].trim_start();
    let tail = tail
        .strip_prefix('—')
        .or_else(|| tail.strip_prefix('–'))
        .or_else(|| tail.strip_prefix('-'))
        .unwrap_or(tail);
    Some((word.to_string(), tail.trim().to_string()))
}

/// An opening code fence: its character, its length and its info string.
fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let body = &line[indent..];
    let ch = body.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = body.len() - body.trim_start_matches(ch).len();
    if len < 3 {
        return None;
    }
    let info = body[len..].trim();
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, len, info))
}

fn fence_closes(line: &str, ch: char, len: usize) -> bool {
    let trimmed = line.trim();
    let indent = line.len() - line.trim_start_matches(' ').len();
    indent <= 3 && trimmed.len() >= len && trimmed.chars().all(|c| c == ch)
}

/// The first `toml run-note` fence outside any other fence. `None` when the
/// note has none; `Err` when it follows prose or is never closed.
fn find_block(lines: &[&str]) -> Option<Result<String, String>> {
    let mut prose_before = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let Some((ch, len, info)) = fence_open(line) else {
            prose_before |= !line.trim().is_empty();
            index += 1;
            continue;
        };
        let close = lines[index + 1..]
            .iter()
            .position(|l| fence_closes(l, ch, len))
            .map(|offset| index + 1 + offset);
        if info == INFO_STRING {
            if prose_before {
                return Some(Err(
                    "the `toml run-note` block does not directly follow the Status line"
                        .to_string(),
                ));
            }
            let Some(close) = close else {
                return Some(Err("the `toml run-note` block is never closed".to_string()));
            };
            return Some(Ok(lines[index + 1..close].join("\n")));
        }
        prose_before = true;
        index = close.map_or(lines.len(), |c| c + 1);
    }
    None
}

/// Every `TANGENTIAL: <kind> | <area> | <summary> | <why>` line outside a
/// fence, skipping `TANGENTIAL: none`.
fn prose_tangential(lines: &[&str]) -> Vec<Tangential> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for line in lines {
        if let Some((ch, len)) = fence {
            if fence_closes(line, ch, len) {
                fence = None;
            }
            continue;
        }
        if let Some((ch, len, _)) = fence_open(line) {
            fence = Some((ch, len));
            continue;
        }
        let mut entry = line.trim();
        entry = entry
            .strip_prefix("- ")
            .or_else(|| entry.strip_prefix("* "))
            .unwrap_or(entry)
            .trim();
        if let Some(inner) = entry.strip_prefix('`').and_then(|e| e.strip_suffix('`')) {
            entry = inner.trim();
        }
        let Some(fields) = entry.strip_prefix("TANGENTIAL:") else {
            continue;
        };
        let parts: Vec<&str> = fields.splitn(4, '|').map(str::trim).collect();
        if let [kind, area, summary, why] = parts[..] {
            out.push(Tangential {
                kind: kind.to_string(),
                area: area.to_string(),
                summary: summary.to_string(),
                why: why.to_string(),
            });
        }
    }
    out
}

fn read_block(body: &str) -> Result<BlockFields, String> {
    let table: Table = toml::from_str(body).map_err(|e| {
        format!(
            "the block is not valid TOML: {}",
            e.message().split_whitespace().collect::<Vec<_>>().join(" ")
        )
    })?;
    let mut fields = BlockFields::default();
    for (key, value) in &table {
        match key.as_str() {
            "files_touched" => {
                fields.files_touched = value
                    .as_array()
                    .and_then(|items| {
                        items
                            .iter()
                            .map(|i| i.as_str().map(str::to_string))
                            .collect::<Option<Vec<_>>>()
                    })
                    .ok_or("`files_touched` is not an array of strings")?;
            }
            "deviations" => {
                fields.deviations = tables(value, "deviations", |entry| {
                    let [what, why, means] =
                        strings(entry, "deviations", ["what", "why", "means"])?;
                    Ok(Deviation { what, why, means })
                })?;
            }
            "tangential" => {
                fields.tangential = tables(value, "tangential", |entry| {
                    let [kind, area, summary, why] =
                        strings(entry, "tangential", ["kind", "area", "summary", "why"])?;
                    Ok(Tangential {
                        kind,
                        area,
                        summary,
                        why,
                    })
                })?;
            }
            "checks" => {
                fields.checks = tables(value, "checks", |entry| {
                    let [command, result] = strings(entry, "checks", ["command", "result"])?;
                    let result = CheckResult::parse(&result).ok_or_else(|| {
                        format!(
                            "`checks.result` is `{result}`, expected one of {}",
                            CheckResult::VOCABULARY.join(", ")
                        )
                    })?;
                    Ok(CheckRun { command, result })
                })?;
            }
            "handover" => {
                let entry = value.as_table().ok_or("`handover` is not a table")?;
                let [reason, action, when, done_when] =
                    strings(entry, "handover", ["reason", "action", "when", "done_when"])?;
                fields.handover = Some(ProposedHandover {
                    reason,
                    action,
                    when,
                    done_when,
                });
            }
            other => return Err(format!("the block holds the unknown key `{other}`")),
        }
    }
    Ok(fields)
}

/// Each table of an array of tables, read by `read`.
fn tables<T>(
    value: &TomlValue,
    key: &str,
    read: impl Fn(&Table) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("`{key}` is not an array of tables"))?;
    items
        .iter()
        .map(|item| {
            item.as_table()
                .ok_or_else(|| format!("`{key}` is not an array of tables"))
                .and_then(&read)
        })
        .collect()
}

/// The string fields `keys` of `entry`, each `""` when absent; any other key,
/// or a value that is not a string, is an error.
fn strings<const N: usize>(
    entry: &Table,
    parent: &str,
    keys: [&str; N],
) -> Result<[String; N], String> {
    if let Some(unknown) = entry.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(format!(
            "the block holds the unknown key `{parent}.{unknown}`"
        ));
    }
    let mut out: [String; N] = std::array::from_fn(|_| String::new());
    for (slot, key) in out.iter_mut().zip(keys) {
        match entry.get(key) {
            None => {}
            Some(TomlValue::String(s)) => *slot = s.clone(),
            Some(_) => return Err(format!("`{parent}.{key}` is not a string")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"Status: done — the links overlay lists a task's related records

```toml run-note
files_touched = ["glimpse/src/view/links.rs"]

[[deviations]]
what = "split the overlay"
why = "the file was too long"
means = "one file"

[[tangential]]
kind = "debt"
area = "glimpse/src/app.rs"
summary = "a long match"
why = "hard to extend"

[[checks]]
command = "cargo clippy --manifest-path glimpse/Cargo.toml"
result = "pass"

[handover]
reason = "restart"
action = "restart the session"
when = "after landing"
done_when = "the overlay shows"
```

## What changed

TANGENTIAL: bug | ignored.rs | prose is not read | the block wins
"#;

    #[test]
    fn a_full_block_reads_every_field() {
        let note = parse(FULL).expect("parses");
        assert_eq!(note.status, "done");
        assert_eq!(
            note.summary,
            "the links overlay lists a task's related records"
        );
        assert_eq!(note.block, NoteBlock::Present);
        assert_eq!(note.reason, "");
        assert_eq!(note.files_touched, ["glimpse/src/view/links.rs"]);
        assert_eq!(
            note.deviations,
            [Deviation {
                what: "split the overlay".into(),
                why: "the file was too long".into(),
                means: "one file".into(),
            }]
        );
        assert_eq!(note.tangential.len(), 1);
        assert_eq!(note.tangential[0].area, "glimpse/src/app.rs");
        assert_eq!(note.checks[0].result, CheckResult::Pass);
        assert_eq!(
            note.handover.as_ref().map(|h| h.reason.as_str()),
            Some("restart")
        );
    }

    #[test]
    fn an_empty_block_is_present_and_empty() {
        let note = parse("Status: done — x\n\n```toml run-note\n```\n").expect("parses");
        assert_eq!(note.block, NoteBlock::Present);
        assert!(note.files_touched.is_empty() && note.deviations.is_empty());
        assert!(note.tangential.is_empty() && note.checks.is_empty());
        assert_eq!(note.handover, None);
    }

    #[test]
    fn a_status_line_alone_parses_as_absent() {
        let note = parse("Status: parked — waiting on a person").expect("parses");
        assert_eq!(note.status, "parked");
        assert_eq!(note.summary, "waiting on a person");
        assert_eq!(note.block, NoteBlock::Absent);
        assert!(note.tangential.is_empty());
    }

    #[test]
    fn an_old_note_keeps_its_prose_tangential_lines() {
        let text = "Status: done — old style\n\n## What changed\n\nprose\n\n\
            TANGENTIAL: none\n\
            - TANGENTIAL: bug | tomlctl/src/plan/target.rs | a stale test | the gate fails\n\
            TANGENTIAL: debt | a.rs | summary | why with | a pipe\n\
            TANGENTIAL: too | few\n\
            ```\nTANGENTIAL: <kind> | <area> | <summary> | <why>\n```\n";
        let note = parse(text).expect("parses");
        assert_eq!(note.block, NoteBlock::Absent);
        assert_eq!(
            note.tangential,
            [
                Tangential {
                    kind: "bug".into(),
                    area: "tomlctl/src/plan/target.rs".into(),
                    summary: "a stale test".into(),
                    why: "the gate fails".into(),
                },
                Tangential {
                    kind: "debt".into(),
                    area: "a.rs".into(),
                    summary: "summary".into(),
                    why: "why with | a pipe".into(),
                },
            ]
        );
    }

    #[test]
    fn invalid_toml_is_malformed_with_the_status_line_only() {
        let text = "Status: failed — x\n\n```toml run-note\nfiles_touched = [\n```\n\
            TANGENTIAL: bug | a.rs | s | w\n";
        let note = parse(text).expect("parses");
        assert_eq!(note.status, "failed");
        assert_eq!(note.block, NoteBlock::Malformed);
        assert!(
            note.reason.starts_with("the block is not valid TOML"),
            "{}",
            note.reason
        );
        assert!(!note.reason.contains('\n'));
        assert!(note.tangential.is_empty());
    }

    #[test]
    fn an_unknown_key_is_malformed() {
        let top =
            parse("Status: done — x\n```toml run-note\nnotes = \"x\"\n```\n").expect("parses");
        assert_eq!(top.block, NoteBlock::Malformed);
        assert!(top.reason.contains("`notes`"), "{}", top.reason);

        let nested = parse(
            "Status: done — x\n```toml run-note\n[[deviations]]\nwhat = \"a\"\nhow = \"b\"\n```\n",
        )
        .expect("parses");
        assert_eq!(nested.block, NoteBlock::Malformed);
        assert!(
            nested.reason.contains("`deviations.how`"),
            "{}",
            nested.reason
        );
        assert!(nested.deviations.is_empty());
    }

    #[test]
    fn a_wrong_type_or_check_result_is_malformed() {
        let typed = parse("Status: done — x\n```toml run-note\nfiles_touched = \"a.rs\"\n```\n")
            .expect("parses");
        assert_eq!(typed.block, NoteBlock::Malformed);
        let result = parse(
            "Status: done — x\n```toml run-note\n[[checks]]\ncommand = \"c\"\nresult = \"ok\"\n```\n",
        )
        .expect("parses");
        assert_eq!(result.block, NoteBlock::Malformed);
        assert!(result.reason.contains("`ok`"), "{}", result.reason);
    }

    #[test]
    fn a_block_after_prose_is_malformed() {
        let text = "Status: done — x\n\n## What changed\n\n```toml run-note\nfiles_touched = [\"a.rs\"]\n```\n";
        let note = parse(text).expect("parses");
        assert_eq!(note.block, NoteBlock::Malformed);
        assert!(
            note.reason.contains("does not directly follow"),
            "{}",
            note.reason
        );
        assert!(note.files_touched.is_empty());
    }

    #[test]
    fn a_block_after_another_fence_is_malformed() {
        let text = "Status: done — x\n```text\nhello\n```\n```toml run-note\n```\n";
        assert_eq!(parse(text).expect("parses").block, NoteBlock::Malformed);
    }

    #[test]
    fn an_unclosed_block_is_malformed() {
        let note =
            parse("Status: done — x\n```toml run-note\nfiles_touched = []\n").expect("parses");
        assert_eq!(note.block, NoteBlock::Malformed);
        assert!(note.reason.contains("never closed"), "{}", note.reason);
    }

    #[test]
    fn a_fence_with_a_different_info_string_is_not_the_block() {
        let text = "Status: done — x\n\n```toml\nfiles_touched = []\n```\n";
        assert_eq!(parse(text).expect("parses").block, NoteBlock::Absent);
    }

    #[test]
    fn a_block_quoted_inside_a_longer_fence_is_not_the_block() {
        let text = "Status: done — x\n\n````\n```toml run-note\n```\n````\n";
        assert_eq!(parse(text).expect("parses").block, NoteBlock::Absent);
    }

    #[test]
    fn crlf_endings_and_trailing_spaces_on_the_fence_parse() {
        let text = "Status: done — crlf\r\n\r\n```toml run-note   \r\nfiles_touched = [\"a.rs\"]\r\n```  \r\n\r\nprose\r\n";
        let note = parse(text).expect("parses");
        assert_eq!(note.summary, "crlf");
        assert_eq!(note.block, NoteBlock::Present);
        assert_eq!(note.files_touched, ["a.rs"]);
    }

    #[test]
    fn a_bom_and_a_hyphen_separator_are_accepted() {
        let note = parse("\u{feff}Status: done - plain hyphen").expect("parses");
        assert_eq!(note.status, "done");
        assert_eq!(note.summary, "plain hyphen");
    }

    #[test]
    fn a_first_line_that_is_not_a_status_line_is_an_error() {
        let err = parse("# Task 5\n\nStatus: done — x\n").expect_err("rejected");
        assert_eq!(err.first_line, "# Task 5");
        assert!(err.to_string().contains("Status:"));
        assert!(parse("").is_err());
        assert!(parse("Status: — no word").is_err());
    }
}
