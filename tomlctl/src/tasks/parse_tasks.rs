//! Parser for a plan's `## Tasks` section.
//!
//! Input is LF-only: callers pass `Section::body_lf`. A numbered heading opens
//! a task and every other heading is a phase label the tasks under it carry.
//! A field line's value continues onto following lines indented two spaces or
//! more, and may start on the first of them rather than after the colon.
//!
//! Inside a fenced block the line grammar is off: no heading, no field, and
//! nothing that ends one. Ownership is settled by the opening marker alone —
//! indented, it continues the open field and the block is that field's value
//! verbatim; at column 0 it ends the field the way any unindented line does,
//! and the block belongs to no task. A fence still open at the end of the
//! section is an error rather than a short task list: `markdown::sections`
//! feeds every later `## ` heading into this body, so what parses is a
//! fraction of the plan.
//!
//! A non-blank line the grammar stores in no field — an unknown field, prose
//! under a task or a phase label, a fence belonging to no field — is reported
//! as `plan/text-unstored`, since `render` rebuilds the section without it.
//!
//! Patterns spell every class out in ASCII. The binary resolves `regex`
//! without its unicode features, so a `\d`/`\s`/`\w` shorthand makes
//! `Regex::new` return `Err` at startup there — while a dev-dependency
//! unifies those features back on, so a test run accepts it. That asymmetry
//! is what `every_pattern_compiles` scans the pattern text for.

use std::sync::OnceLock;

use anyhow::{Result, bail};
use regex::Regex;

use super::finding::{Finding, WARNING, quoted_list};
use super::markdown::FenceState;
use super::schema::Effort;

/// The deepest heading the grammar reads as a task; `render` clamps to the
/// same bound.
const MAX_TASK_DEPTH: u32 = 6;

/// One section's tasks and the findings its headings raised. A finding here is
/// a plan-authoring slip rather than an unbuildable graph, so it never gates
/// the parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedTasks {
    pub(crate) tasks: Vec<ParsedTask>,
    pub(crate) findings: Vec<Finding>,
}

/// One task heading and its field lines. `effort` is `None` for a heading with
/// no `[S|M|L]` tag and no `- **Effort**:` line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedTask {
    pub(crate) id: u32,
    pub(crate) title: String,
    pub(crate) effort: Option<Effort>,
    /// `#` run of this task's own heading.
    pub(crate) depth: u32,
    /// Nearest preceding non-numbered heading, and its `#` run. Empty for a
    /// task under no phase label.
    pub(crate) phase: String,
    pub(crate) phase_depth: u32,
    pub(crate) files: Vec<String>,
    /// The text trailing each path in the `Files` line — `(new)` and the like
    /// — one entry per `files` entry and empty where the author wrote none.
    /// Lines nested under a bulleted path follow as `\n`-joined lines.
    pub(crate) file_notes: Vec<String>,
    pub(crate) needs: Vec<u32>,
    pub(crate) deps_note: String,
    pub(crate) action: String,
    pub(crate) detail: String,
    pub(crate) acceptance: String,
    pub(crate) backlog_closes: Vec<String>,
    pub(crate) backlog_refs: Vec<String>,
}

/// Line numbers relative to `section_body` itself, which is what a fixture
/// standing in for a whole document wants. Every live caller holds the
/// document and goes through `parse_tasks_at`.
#[cfg(test)]
pub(crate) fn parse_tasks(section_body: &str) -> Result<Vec<ParsedTask>> {
    parse_tasks_at(section_body, 1).map(|parsed| parsed.tasks)
}

/// `first_line` is the document line `section_body`'s first line stands on, so
/// every reported number names a line of the plan rather than an offset into a
/// section a reader cannot see.
pub(crate) fn parse_tasks_at(section_body: &str, first_line: usize) -> Result<ParsedTasks> {
    let mut tasks: Vec<ParsedTask> = Vec::new();
    let mut findings: Vec<Finding> = Vec::new();
    let mut current: Option<ParsedTask> = None;
    let mut open: Option<OpenField> = None;
    let mut pending_blank = false;
    let mut fence = FenceState::default();
    let mut fence_line = first_line;
    let mut phase = String::new();
    let mut phase_depth = 0u32;
    let mut phase_line = 0usize;
    let mut unstored: Option<Unstored> = None;
    // Held back until a task has parsed: a section with none is refused as
    // `plan/no-tasks`, so there is no render to lose the text to.
    let mut unstored_findings: Vec<Finding> = Vec::new();
    // Under a heading `plan/heading-too-deep` already reports, whose fields
    // are the dropped task rather than prose of their own.
    let mut quiet = false;

    for (index, line) in section_body.lines().enumerate() {
        let line_no = first_line + index;
        let was_fenced = fence.is_open();
        let fenced = fence.consume(line);
        let opening = fenced && !was_fenced;
        if opening {
            fence_line = line_no;
        }

        if !fenced && line.starts_with('#') {
            close_field(current.as_mut(), open.take(), &mut findings, &mut unstored)?;
            pending_blank = false;
            unstored_findings.extend(text_unstored(
                unstored.take().filter(|_| !quiet),
                current.as_ref(),
                &phase,
                phase_line,
            ));
            tasks.extend(current.take());
            match open_heading(line, line_no)? {
                Some(task) => {
                    quiet = false;
                    current = Some(ParsedTask {
                        phase: phase.clone(),
                        phase_depth,
                        ..task
                    });
                }
                None => {
                    current = None;
                    let deep = deep_heading(line, line_no);
                    quiet = deep.is_some();
                    findings.extend(deep);
                    if let Some(label) = phase_label(line) {
                        phase = label.to_string();
                        phase_depth = heading_depth(line);
                        phase_line = line_no;
                    }
                }
            }
            continue;
        }

        if !fenced
            && let Some(caps) = field_re().captures(line)
            && current.is_some()
        {
            close_field(current.as_mut(), open.take(), &mut findings, &mut unstored)?;
            pending_blank = false;
            let label = caps.get(1).map_or("", |m| m.as_str());
            let inline = caps.get(2).map_or("", |m| m.as_str()).trim_end();
            let mut lines = Vec::new();
            let mut line_nos = Vec::new();
            if !inline.is_empty() {
                lines.push(inline.to_string());
                line_nos.push(line_no);
            }
            open = Some(OpenField {
                kind: Field::from_label(label),
                line_no,
                lines,
                line_nos,
            });
            continue;
        }

        if open.is_none() {
            if is_content(line) {
                note_unstored(&mut unstored, line_no, || describe_unstored(line));
            }
            continue;
        }

        // Anything reached from inside a fence belongs to the field whose
        // continuation opened it, so its own indentation states nothing.
        let continuation = dedent(line).or_else(|| (fenced && !opening).then_some(line));

        if line.trim().is_empty() {
            pending_blank = true;
        } else if let Some(rest) = continuation {
            let field = open.as_mut().expect("field is open");
            if pending_blank {
                field.lines.push(String::new());
                field.line_nos.push(line_no);
                pending_blank = false;
            }
            field.lines.push(rest.to_string());
            field.line_nos.push(line_no);
        } else {
            close_field(current.as_mut(), open.take(), &mut findings, &mut unstored)?;
            pending_blank = false;
            if is_content(line) {
                note_unstored(&mut unstored, line_no, || describe_unstored(line));
            }
        }
    }

    if fence.is_open() {
        bail!(
            "line {fence_line}: fenced block opened here and never closed — every `## ` \
             heading after it reads as part of the Tasks section, so the plan parses to a \
             fraction of its tasks"
        );
    }

    close_field(current.as_mut(), open.take(), &mut findings, &mut unstored)?;
    unstored_findings.extend(text_unstored(
        unstored.take().filter(|_| !quiet),
        current.as_ref(),
        &phase,
        phase_line,
    ));
    tasks.extend(current.take());
    if !tasks.is_empty() {
        findings.extend(unstored_findings);
    }
    Ok(ParsedTasks { tasks, findings })
}

/// The first line under a task or a phase heading that no store field holds,
/// and how many such lines the block carries in all.
struct Unstored {
    line_no: usize,
    what: String,
    lines: usize,
}

fn note_unstored(slot: &mut Option<Unstored>, line_no: usize, what: impl FnOnce() -> String) {
    match slot {
        Some(seen) => seen.lines += 1,
        None => {
            *slot = Some(Unstored {
                line_no,
                what: what(),
                lines: 1,
            });
        }
    }
}

fn describe_unstored(line: &str) -> String {
    let label = label_re()
        .captures(line)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str());
    match label {
        Some(label) if field_re().is_match(line) => {
            format!("a `{label}` field under no task heading")
        }
        Some(label) => format!("the field `{label}`, which the store has no column for"),
        None => format!(
            "\"{}\", which stands under no field the store keeps",
            preview(line)
        ),
    }
}

/// A blank line or a thematic break carries nothing a render loses.
fn is_content(line: &str) -> bool {
    let bare: String = line
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    !(bare.is_empty()
        || (bare.len() >= 3
            && ["-", "*", "_"]
                .iter()
                .any(|mark| bare == mark.repeat(bare.len()))))
}

fn preview(line: &str) -> String {
    const WIDTH: usize = 60;
    let line = line.trim();
    match line.char_indices().nth(WIDTH) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_string(),
    }
}

fn text_unstored(
    dropped: Option<Unstored>,
    task: Option<&ParsedTask>,
    phase: &str,
    phase_line: usize,
) -> Option<Finding> {
    let Unstored {
        line_no,
        what,
        lines,
    } = dropped?;
    let more = match lines {
        1 => String::new(),
        2 => " (and 1 more line)".to_string(),
        n => format!(" (and {} more lines)", n - 1),
    };
    let (ids, place, fix) = match task {
        Some(task) => (
            vec![task.id],
            format!(
                "line {line_no}: task {} \"{}\" carries",
                task.id, task.title
            ),
            "move it under the task's `- **Detail**:` label, indented two spaces, or out of the \
             task",
        ),
        None if phase_line > 0 => (
            Vec::new(),
            format!("line {phase_line}: phase \"{phase}\" carries, at line {line_no},"),
            "move it into a task's `- **Detail**:` or out of the `## Tasks` section",
        ),
        None => (
            Vec::new(),
            format!("line {line_no}: the `## Tasks` section carries, ahead of its first heading,"),
            "move it into a task's `- **Detail**:` or out of the `## Tasks` section",
        ),
    };
    Some(Finding {
        class: "plan/text-unstored",
        severity: WARNING,
        ids,
        detail: format!(
            "{place} {what}{more}. The import stores it nowhere, so `tasks render` removes it \
             from the plan; {fix}"
        ),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Files,
    DependsOn,
    Action,
    Detail,
    Acceptance,
    Effort,
    Backlog,
}

impl Field {
    fn from_label(label: &str) -> Self {
        match label {
            "Files" => Self::Files,
            "Depends on" | "Blocked-by" | "Blocked by" => Self::DependsOn,
            "Action" => Self::Action,
            "Detail" => Self::Detail,
            "Acceptance" => Self::Acceptance,
            "Effort" => Self::Effort,
            "Backlog" => Self::Backlog,
            _ => unreachable!("field_re admits only the labels above"),
        }
    }
}

struct OpenField {
    kind: Field,
    line_no: usize,
    lines: Vec<String>,
    /// The plan line each entry of `lines` came from.
    line_nos: Vec<usize>,
}

/// `None` for a phase label; an error for a heading whose id starts with a
/// digit but is not an integer, or whose effort tag is outside the vocabulary.
fn open_heading(line: &str, line_no: usize) -> Result<Option<ParsedTask>> {
    let Some(caps) = heading_re().captures(line) else {
        if let Some(bad) = malformed_id_re().captures(line) {
            let id = bad.get(1).map_or("", |m| m.as_str());
            bail!("line {line_no}: task id `{id}` is not an integer — `{line}`");
        }
        return Ok(None);
    };

    let id: u32 = caps
        .get(1)
        .map_or("", |m| m.as_str())
        .parse()
        .map_err(|_| anyhow::anyhow!("line {line_no}: task id out of range — `{line}`"))?;
    let rest = caps.get(2).map_or("", |m| m.as_str());

    // Located from the right: titles routinely carry ` — `, which a left-to-right
    // scan would take as the trailing-prose group and swallow the tag with it.
    let (title, effort) = match effort_tag_re().captures_iter(rest).last() {
        Some(tag) => {
            let whole = tag.get(0).expect("whole match");
            let raw = tag.get(1).map_or("", |m| m.as_str());
            let effort = Effort::parse(raw).ok_or_else(|| {
                anyhow::anyhow!(
                    "line {line_no}: effort tag `[{raw}]` is not one of {} — `{line}`",
                    Effort::VOCABULARY.join(", ")
                )
            })?;
            (rest[..whole.start()].trim_end(), Some(effort))
        }
        None => (rest.trim_end(), None),
    };

    Ok(Some(ParsedTask {
        id,
        title: title.to_string(),
        effort,
        depth: heading_depth(line),
        ..ParsedTask::default()
    }))
}

/// A numbered heading past the deepest run the grammar reads as a task. It
/// stands as a phase label instead, so without this the number, the fields
/// under it and the task itself leave no trace of having been written.
fn deep_heading(line: &str, line_no: usize) -> Option<Finding> {
    let caps = deep_heading_re().captures(line)?;
    let id: u32 = caps.get(1)?.as_str().parse().ok()?;
    let title = caps.get(2).map_or("", |m| m.as_str()).trim();
    Some(Finding {
        class: "plan/heading-too-deep",
        severity: WARNING,
        ids: vec![id],
        detail: format!(
            "line {line_no}: task {id} \"{title}\" is {} hashes deep, past the \
             {MAX_TASK_DEPTH} a task heading may carry, so it reads as a phase label \
             and the task is dropped",
            heading_depth(line)
        ),
    })
}

/// Length of the leading `#` run.
fn heading_depth(line: &str) -> u32 {
    let run = line.len() - line.trim_start_matches('#').len();
    u32::try_from(run).unwrap_or(u32::MAX)
}

/// Text after the `#` run of a heading that opens no task. `None` when the run
/// is not followed by a space or carries no text, neither of which is a
/// heading — and so neither of which replaces the phase already in force.
fn phase_label(line: &str) -> Option<&str> {
    let run = line.len() - line.trim_start_matches('#').len();
    let label = line[run..].strip_prefix(' ')?.trim();
    (!label.is_empty()).then_some(label)
}

fn close_field(
    task: Option<&mut ParsedTask>,
    open: Option<OpenField>,
    findings: &mut Vec<Finding>,
    unstored: &mut Option<Unstored>,
) -> Result<()> {
    let (Some(task), Some(open)) = (task, open) else {
        return Ok(());
    };
    let OpenField {
        kind,
        line_no,
        lines,
        line_nos,
    } = open;

    match kind {
        Field::Files => {
            let read = read_files(&lines);
            findings.extend(files_span_unclaimed(task, line_no, &read.held));
            if let Some(at) = read.orphan {
                note_unstored(unstored, line_nos[at], || {
                    format!(
                        "the nested `Files` line \"{}\", which has no path above it to annotate",
                        preview(&lines[at])
                    )
                });
            }
            task.files = read.files;
            task.file_notes = read.notes;
        }
        Field::DependsOn => {
            let (needs, note) = parse_depends(&lines);
            task.needs = needs;
            task.deps_note = note;
        }
        Field::Action => task.action = join_prose(&lines),
        Field::Detail => task.detail = join_prose(&lines),
        Field::Acceptance => task.acceptance = join_prose(&lines),
        Field::Effort => {
            let value = lines.join(" ");
            let raw = value.split_whitespace().next().unwrap_or("");
            let effort = Effort::parse(raw).ok_or_else(|| {
                anyhow::anyhow!(
                    "line {line_no}: effort `{raw}` is not one of {}",
                    Effort::VOCABULARY.join(", ")
                )
            })?;
            task.effort = Some(effort);
        }
        Field::Backlog => {
            let (closes, refs) = parse_backlog(&lines);
            task.backlog_closes = closes;
            task.backlog_refs = refs;
        }
    }
    Ok(())
}

/// The paths, and the annotation trailing each of them — the store keeps the
/// two apart, because every consumer of a file claim compares bare paths while
/// the annotation is the author's and only the render puts it back.
pub(crate) fn parse_files(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let read = read_files(lines);
    (read.files, read.notes)
}

#[derive(Default)]
struct FilesRead<'a> {
    files: Vec<String>,
    notes: Vec<String>,
    /// Backticked spans a comma-list line kept inside a dash note.
    held: Vec<&'a str>,
    /// Index into the field's lines of the first nested line with no path
    /// above it to annotate.
    orphan: Option<usize>,
}

/// A bulleted line is one path plus prose and a bare line is a comma list. A
/// line indented deeper than the path bullets is the previous path's note
/// continuing — kept as a further `\n`-joined line of that note, one indent
/// step removed so its own nesting survives — rather than a path: read as a
/// bullet, a nested sub-bullet becomes a claim on a file nobody named.
fn read_files(lines: &[String]) -> FilesRead<'_> {
    let mut read = FilesRead::default();
    let mut bullet_indent: Option<usize> = None;
    let mut owner: Option<usize> = None;
    let mut after_bullet = false;
    let mut pending_blank = false;
    for (index, raw) in lines.iter().enumerate() {
        let body = raw.trim_start();
        if body.trim_end().is_empty() {
            pending_blank = true;
            continue;
        }
        let indent = raw.len() - body.len();
        if let Some(base) = bullet_indent.filter(|base| after_bullet && indent > *base) {
            match owner {
                Some(at) => {
                    let note = &mut read.notes[at];
                    if pending_blank {
                        note.push('\n');
                    }
                    let rest = &raw[base..];
                    note.push('\n');
                    note.push_str(dedent(rest).unwrap_or(rest.trim_start()).trim_end());
                }
                None => {
                    read.orphan.get_or_insert(index);
                }
            }
            pending_blank = false;
            continue;
        }
        pending_blank = false;

        let line = body.trim_end();
        if bullet_re().is_match(line) {
            bullet_indent.get_or_insert(indent);
            after_bullet = true;
            let before = read.files.len();
            push_file(
                &mut read.files,
                &mut read.notes,
                bullet_re().replace(line, "").as_ref(),
            );
            owner = (read.files.len() > before).then(|| read.files.len() - 1);
        } else {
            after_bullet = false;
            owner = None;
            let (entries, held) = scan_entries(line, |ch| ch == ',', Some(opens_on_path));
            read.held.extend(held);
            for raw in entries {
                push_file(&mut read.files, &mut read.notes, raw);
            }
        }
    }
    read
}

/// The em-dash annotation has no closing mark, so it runs to the end of the
/// entry it trails.
const NOTE_DASH: &str = " — ";

/// One entry per item the line lists. A separator standing inside a
/// backticked span or an annotation separates nothing: splitting on it and
/// cutting the annotation afterwards reads `(new, generated)` as a second
/// path, which every consumer of a file claim then treats as one.
fn split_entries(line: &str, separates: impl Fn(char) -> bool) -> Vec<&str> {
    split_entries_resuming(line, separates, None)
}

/// `resume` ends an em-dash annotation at a separator when it accepts the text
/// after that separator, which is how a rendered line lists the entry after an
/// annotated one. Without it the annotation runs to the end of the line and
/// every later entry is read as its prose; with one too loose, the
/// annotation's own `, x` becomes an entry nobody wrote.
fn split_entries_resuming(
    line: &str,
    separates: impl Fn(char) -> bool,
    resume: Option<fn(&str) -> bool>,
) -> Vec<&str> {
    scan_entries(line, separates, resume).0
}

/// The entries, and the backticked span opening the text after each separator
/// `resume` declined to end an annotation at.
fn scan_entries(
    line: &str,
    separates: impl Fn(char) -> bool,
    resume: Option<fn(&str) -> bool>,
) -> (Vec<&str>, Vec<&str>) {
    let mut entries = Vec::new();
    let mut declined = Vec::new();
    let mut start = 0;
    let mut depth = 0u32;
    let mut quoted = false;
    let mut annotated = false;
    for (at, ch) in line.char_indices() {
        if ch == '`' {
            quoted = !quoted;
            continue;
        }
        if quoted || (annotated && resume.is_none()) {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            _ if separates(ch) => {
                let rest = line[at + ch.len_utf8()..].trim_start();
                if !annotated || resume.is_some_and(|opens| opens(rest)) {
                    entries.push(&line[start..at]);
                    start = at + ch.len_utf8();
                    annotated = false;
                } else if let Some(span) = leading_span(rest) {
                    declined.push(span);
                }
            }
            _ if !annotated && line[at..].starts_with(NOTE_DASH) => annotated = true,
            _ => {}
        }
    }
    entries.push(&line[start..]);
    (entries, declined)
}

/// The non-empty backticked span `rest` opens on.
fn leading_span(rest: &str) -> Option<&str> {
    rest.strip_prefix('`')
        .and_then(|inner| inner.split_once('`'))
        .map(|(span, _)| span)
        .filter(|span| !span.is_empty())
}

/// A backticked span holding a `/` or `.`. A backticked bare identifier after
/// the comma is the annotation's own prose, not a path.
fn opens_on_path(rest: &str) -> bool {
    leading_span(rest).is_some_and(|span| span.contains(['/', '.']))
}

/// Backticked spans a comma-list `Files` line kept inside a dash note. The
/// parse reads each as prose, so an extensionless path written there —
/// `Makefile` — claims no file and drops out of overlap scheduling unseen.
pub(crate) fn note_held_spans(lines: &[String]) -> Vec<&str> {
    read_files(lines).held
}

fn files_span_unclaimed(task: &ParsedTask, line_no: usize, spans: &[&str]) -> Option<Finding> {
    if spans.is_empty() {
        return None;
    }
    Some(Finding {
        class: "plan/files-span-unclaimed",
        severity: WARNING,
        ids: vec![task.id],
        detail: format!(
            "line {line_no}: task {} \"{}\" — the `Files` note carries {} after a comma, \
             which reads as the note's prose and claims no file. If it is a path, list the \
             files as a bulleted sub-list, one path per line; if it is prose, reword the note",
            task.id,
            task.title,
            quoted_list(spans, "")
        ),
    })
}

/// A `Backlog` entry — an optional `refs `, then an id, backticked or bare,
/// with nothing after it but the end of the entry or its annotation. Prose
/// that merely names an id (`B-2 is related`) stays in the note.
fn opens_on_backlog_id(rest: &str) -> bool {
    let rest = rest.strip_prefix("refs ").map_or(rest, str::trim_start);
    let (id, after) = match rest.strip_prefix('`') {
        Some(inner) => match inner.split_once('`') {
            Some(pair) => pair,
            None => return false,
        },
        None => rest.split_at(
            rest.find(|ch: char| ch == ',' || ch == '(' || ch.is_ascii_whitespace())
                .unwrap_or(rest.len()),
        ),
    };
    let after = after.trim_start();
    is_backlog_id(id) && (after.is_empty() || after.starts_with([',', '(', '—']))
}

fn is_backlog_id(token: &str) -> bool {
    token
        .strip_prefix("B-")
        .is_some_and(|hex| !hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn push_file(files: &mut Vec<String>, notes: &mut Vec<String>, raw: &str) {
    let at = annotation_at(raw);
    let entry = raw[..at].replace('`', "").trim().to_string();
    if entry.is_empty() || is_empty_marker(&entry) {
        return;
    }
    files.push(entry);
    notes.push(raw[at..].trim().to_string());
}

/// Where the annotation trailing a path starts — the first `(` or ` — `
/// outside a backticked span — or the entry's length when it carries none.
/// `split_entries` reads the same mark, so a comma the cut drops cannot have
/// split the entry ahead of it.
fn annotation_at(entry: &str) -> usize {
    let mut quoted = false;
    for (at, ch) in entry.char_indices() {
        if ch == '`' {
            quoted = !quoted;
        } else if !quoted && (ch == '(' || entry[at..].starts_with(NOTE_DASH)) {
            return at;
        }
    }
    entry.len()
}

/// The ids the line states and the prose it carries, cut entry by entry
/// rather than at the line's first `(` — an id stated after prose is still an
/// id, and a line read to no id at all schedules the task as though it
/// declared nothing.
///
/// An entry states an edge when it opens on a bare integer, and everything
/// after that integer is the entry's note. An entry opening on anything else
/// is prose whole: `round-2 task 18` names a task outside this plan, and
/// reading its number as an edge would point it at whatever this plan numbers
/// 18.
fn parse_depends(lines: &[String]) -> (Vec<u32>, String) {
    let value = lines.join(" ");
    let mut needs = Vec::new();
    let mut note: Vec<&str> = Vec::new();
    // `4 + 7` is a conjunction wherever a plan writes one, and dropping the
    // second id there loses an edge exactly as a comma would.
    for entry in split_entries(value.trim(), |ch| ch == ',' || ch == '+') {
        let entry = entry.trim();
        if entry.is_empty() || is_empty_marker(entry) {
            continue;
        }
        let (head, rest) = entry
            .split_once(|c: char| c.is_ascii_whitespace())
            .unwrap_or((entry, ""));
        match head.parse::<u32>() {
            Ok(id) => {
                needs.push(id);
                note.push(unwrap_note(rest.trim()));
            }
            // `1-15` declares fifteen edges; reading it as prose would drop
            // every one and hand Kahn a task with no dependencies.
            Err(_) if let Some((lo, hi)) = parse_range(head) => {
                needs.extend(lo..=hi);
                note.push(unwrap_note(rest.trim()));
            }
            // `none (rationale)` states no edge, and the rationale is a note
            // like any other.
            Err(_) if is_empty_marker(head) => note.push(unwrap_note(rest.trim())),
            Err(_) => note.push(unwrap_note(entry)),
        }
    }
    needs.sort_unstable();
    needs.dedup();
    note.retain(|part| !part.is_empty());
    (needs, note.join(", "))
}

/// The ids the task closes and the ids it only `refs`. The qualifier binds the
/// one comma-separated entry it opens, so `refs B-1, B-2` closes `B-2`.
fn parse_backlog(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let value = lines.join(" ");
    let mut closes = Vec::new();
    let mut refs = Vec::new();
    for raw in split_entries_resuming(value.trim(), |ch| ch == ',', Some(opens_on_backlog_id)) {
        let entry = raw[..annotation_at(raw)].replace('`', "");
        let entry = entry.trim();
        let (target, id) = match entry.strip_prefix("refs ") {
            Some(rest) => (&mut refs, rest.trim()),
            None => (&mut closes, entry),
        };
        if !id.is_empty() && !is_empty_marker(id) {
            target.push(id.to_string());
        }
    }
    (closes, refs)
}

/// `lo-hi` (hyphen or en dash) with `lo <= hi`; anything else is not a range.
fn parse_range(token: &str) -> Option<(u32, u32)> {
    let (lo, hi) = token.split_once(['-', '–'])?;
    let (lo, hi) = (
        lo.trim().parse::<u32>().ok()?,
        hi.trim().parse::<u32>().ok()?,
    );
    (lo <= hi).then_some((lo, hi))
}

/// A note's own outer parentheses, dropped so the renderer can re-add them.
fn unwrap_note(text: &str) -> &str {
    text.strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(text)
}

fn is_empty_marker(token: &str) -> bool {
    token.eq_ignore_ascii_case("none") || token == "—" || token == "–" || token == "-"
}

fn join_prose(lines: &[String]) -> String {
    lines
        .join("\n")
        .trim_matches(|c: char| c == '\n' || c == ' ' || c == '\t')
        .to_string()
}

/// One continuation step of indent removed, so nested list levels survive.
/// `None` when the line is not indented far enough to continue a field.
fn dedent(line: &str) -> Option<&str> {
    line.strip_prefix("  ").or_else(|| line.strip_prefix('\t'))
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^#{3,6} ([0-9]+)\. (.+)$").expect("heading regex compiles"))
}

fn deep_heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^#{7,} ([0-9]+)\. (.+)$").expect("deep-heading regex compiles"))
}

fn malformed_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^#{3,6} ([0-9][^ \t]*)\. ").expect("malformed-id regex compiles")
    })
}

fn effort_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r" \[([A-Za-z][A-Za-z-]*)\]").expect("effort-tag regex compiles"))
}

fn field_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^- \*\*(Files|Depends on|Blocked-by|Blocked by|Action|Detail|Acceptance|Effort|Backlog)\*\*:[ \t]*(.*)$",
        )
        .expect("field regex compiles")
    })
}

/// Any bold field label, known to `field_re` or not.
fn label_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[-*] \*\*([^*]+)\*\*:").expect("label regex compiles"))
}

fn bullet_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[-*][ \t]+").expect("bullet regex compiles"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHASED: &str = "\
### Milestone A — store, schema and the graph engine (tomlctl core)

#### 2. Repo part 1 — columns, acceptance criteria, closure gate [L]
- **Files**: `tomlctl/src/tasks/schema.rs` (new), `tomlctl/src/tasks/`,
  `tomlctl/src/main.rs`
- **Depends on**: 1, 3 (sequential commits keep the phases independently
  revertible)
- **Action**: Implement `Store` and `TaskRow`,
  preserving the field order shown in Approach.
- **Detail**: Mirror `tomlctl/src/backlog/mod.rs`.
- **Acceptance**: inline unit tests round-trip a fixture store.

### Phase 2: carrier adoption

#### 4. Wire the dispatch
- **Files**: none
- **Blocked-by**: 3
- **Effort**: M
";

    #[test]
    fn phase_labels_are_skipped_and_h4_tasks_are_kept() {
        let tasks = parse_tasks(PHASED).expect("parses");
        assert_eq!(
            tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![2, 4],
            "phase headings leaked in as tasks"
        );
    }

    #[test]
    fn a_task_carries_its_phase_label_and_both_heading_depths() {
        let tasks = parse_tasks(PHASED).expect("parses");
        assert_eq!(
            tasks[0].phase,
            "Milestone A — store, schema and the graph engine (tomlctl core)"
        );
        assert_eq!(tasks[1].phase, "Phase 2: carrier adoption");
        for task in &tasks {
            assert_eq!((task.phase_depth, task.depth), (3, 4), "task {}", task.id);
        }
    }

    #[test]
    fn a_task_ahead_of_every_phase_label_carries_none() {
        let body = "### 1. Ship it [S]\n\n##### Wave 1\n\n###### 2. Follow up [S]\n";
        let tasks = parse_tasks(body).expect("parses");
        assert_eq!(tasks[0].phase, "");
        assert_eq!((tasks[0].phase_depth, tasks[0].depth), (0, 3));
        assert_eq!(tasks[1].phase, "Wave 1");
        assert_eq!((tasks[1].phase_depth, tasks[1].depth), (5, 6));
    }

    #[test]
    fn a_numbered_heading_five_or_six_hashes_deep_is_a_task() {
        let body = "\
##### Wave 1 (parallel, after task 10)

##### 11. Migrate the review carrier [M]
- **Depends on**: 10

###### 12. Migrate the apply carrier [S]
- **Files**: none
";
        let tasks = parse_tasks(body).expect("parses");
        assert_eq!(
            tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![11, 12],
            "a deep numbered heading was read as a phase label"
        );
        assert_eq!(tasks[0].needs, vec![10]);
    }

    /// The heading still stands as a phase label, so the finding is the only
    /// trace the task leaves — and it warns rather than errors, because a
    /// heading one hash too deep is a slip an import must not refuse over.
    #[test]
    fn a_numbered_heading_past_six_hashes_warns_rather_than_vanishing() {
        let body = "\
### 1. Ship it [S]
- **Files**: none

####### 2. Follow up [S]
- **Files**: none
";
        let parsed = parse_tasks_at(body, 10).expect("parses");
        assert_eq!(
            parsed.tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(parsed.findings.len(), 1, "{:?}", parsed.findings);

        let finding = &parsed.findings[0];
        assert_eq!(finding.class, "plan/heading-too-deep");
        assert_eq!(finding.severity, WARNING);
        assert_eq!(finding.ids, vec![2]);
        assert!(finding.detail.contains("line 13"), "{finding:?}");
        assert!(finding.detail.contains("Follow up"), "{finding:?}");

        let kept = body.replace("####### 2.", "###### 2.");
        let parsed = parse_tasks_at(&kept, 10).expect("parses");
        assert_eq!(parsed.tasks.len(), 2);
        assert!(parsed.findings.is_empty(), "{:?}", parsed.findings);
    }

    #[test]
    fn a_fenced_hash_line_does_not_end_the_task() {
        let body = "\
### 1. Ship it [S]
- **Acceptance**: the suite is green
```sh
# builds clean
cargo build
```
- **Files**: `tomlctl/src/tasks/parse_tasks.rs`

### 2. Follow up [S]
- **Files**: none
";
        let tasks = parse_tasks(body).expect("parses");
        assert_eq!(tasks.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(
            tasks[0].files,
            vec!["tomlctl/src/tasks/parse_tasks.rs"],
            "fields after a fenced block were dropped"
        );
    }

    #[test]
    fn a_fenced_field_line_does_not_open_a_field() {
        let body = "\
### 1. Document the grammar [S]
- **Files**: `docs/plans/README.md`
- **Action**: show the shape a task takes:

```md
- **Files**: `src/lib.rs`
- **Depends on**: 4
- **Acceptance**: the suite is green
```

### 2. Follow up [S]
- **Files**: none
";
        let tasks = parse_tasks(body).expect("parses");
        assert_eq!(tasks.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(
            tasks[0].files,
            vec!["docs/plans/README.md"],
            "a fenced example overwrote the real Files line"
        );
        assert!(tasks[0].needs.is_empty(), "{:?}", tasks[0].needs);
        assert_eq!(tasks[0].acceptance, "");
        assert_eq!(tasks[0].action, "show the shape a task takes:");
    }

    #[test]
    fn a_fenced_block_a_field_owns_keeps_its_column_zero_lines() {
        let body = "\
### 1. Ship it [S]
- **Acceptance**: the command prints
  ```json
{\"ok\": true}
  ```
- **Files**: none
";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(
            task.acceptance, "the command prints\n```json\n{\"ok\": true}\n```",
            "an unindented line inside the field's own fence was dropped"
        );
    }

    #[test]
    fn an_unclosed_fence_names_the_line_it_opened_on() {
        let body = "\
### 1. Ship it [S]
- **Acceptance**: the suite is green

```sh
cargo test

### 2. Follow up [S]
- **Files**: none
";
        let err = parse_tasks(body)
            .expect_err("an unclosed fence is an error")
            .to_string();
        assert!(err.contains("line 4"), "{err}");
        assert!(err.contains("never closed"), "{err}");
    }

    #[test]
    fn an_em_dash_title_keeps_its_effort_tag() {
        let first = &parse_tasks(PHASED).expect("parses")[0];
        assert_eq!(
            first.title,
            "Repo part 1 — columns, acceptance criteria, closure gate"
        );
        assert_eq!(first.effort, Some(Effort::L));
    }

    #[test]
    fn prose_after_the_effort_tag_is_dropped() {
        let body =
            "#### 21. End-to-end smoke test [M] — human-gated checklist (not dispatchable)\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.title, "End-to-end smoke test");
        assert_eq!(task.effort, Some(Effort::M));
    }

    #[test]
    fn a_wrapped_files_line_keeps_directories_and_drops_annotations() {
        let first = &parse_tasks(PHASED).expect("parses")[0];
        assert_eq!(
            first.files,
            vec![
                "tomlctl/src/tasks/schema.rs",
                "tomlctl/src/tasks/",
                "tomlctl/src/main.rs",
            ]
        );
    }

    #[test]
    fn a_bulleted_files_sub_list_yields_one_path_per_line() {
        let body = "### 1. Widen the panel [S]\n\
                    - **Files**:\n\
                    \x20 - `lumina/web/src/api/repo-links.ts` — dual edit: extend `RepoLink`, then `RepoLinkSchema`.\n\
                    \x20 - `lumina/web/src/composables/useSettings.ts` (new) — module-singleton composable.\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(
            task.files,
            vec![
                "lumina/web/src/api/repo-links.ts",
                "lumina/web/src/composables/useSettings.ts",
            ]
        );
    }

    /// Read as a sibling bullet, the nested line claims a file nobody named
    /// and feeds it to overlap scheduling.
    #[test]
    fn a_line_nested_under_a_bulleted_path_is_its_note_and_claims_nothing() {
        let body = "### 1. First task [S]\n\
                    - **Files**:\n\
                    \x20 - `a.rs` — core\n\
                    \x20   - sub-note about a.rs\n\
                    \x20       - deeper\n\
                    \x20 - `b.rs`\n\
                    \x20   wrapped prose\n\
                    - **Depends on**: none\n";
        let parsed = parse_tasks_at(body, 1).expect("parses");
        let task = &parsed.tasks[0];
        assert_eq!(task.files, vec!["a.rs", "b.rs"]);
        assert_eq!(
            task.file_notes,
            vec![
                "— core\n- sub-note about a.rs\n    - deeper",
                "\nwrapped prose"
            ]
        );
        assert!(parsed.findings.is_empty(), "{:?}", parsed.findings);
    }

    #[test]
    fn an_indented_comma_continuation_is_still_a_path() {
        let body = "### 1. First task [S]\n- **Files**: `a.rs`,\n    `b.rs`\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.files, vec!["a.rs", "b.rs"]);
        assert_eq!(task.file_notes, vec!["", ""]);
    }

    #[test]
    fn a_nested_files_line_with_no_path_above_it_is_unstored() {
        let body = "### 4. First task [S]\n- **Files**:\n  - none\n    - orphaned\n";
        let parsed = parse_tasks_at(body, 1).expect("parses");
        assert!(parsed.tasks[0].files.is_empty(), "{:?}", parsed.tasks[0]);
        assert_eq!(parsed.findings.len(), 1, "{:?}", parsed.findings);
        let finding = &parsed.findings[0];
        assert_eq!(finding.class, "plan/text-unstored");
        assert_eq!(finding.ids, vec![4]);
        for fragment in ["line 4", "orphaned", "no path above it"] {
            assert!(finding.detail.contains(fragment), "{fragment}: {finding:?}");
        }
    }

    /// `render` rebuilds the section from the store, so text the store has no
    /// field for would leave the plan at the next render unannounced.
    #[test]
    fn content_no_field_stores_warns_once_per_task() {
        let body = "\
### 1. First task [S]
- **Files**: `a.rs`
- **Acceptance**:
  - cargo test passes
- **Notes**:
  - an unknown field
    - nested

### 2. Second task [S]
- **Files**: `c.rs`
- a stray bullet

---

### 3. Third task [S]
- **Files**: `d.rs`
- **Depends on**: 1, 2
- **Action**: Plain.
  1. numbered
- **Detail**:
  More.
- **Acceptance**: ok
- **Effort**: S
- **Backlog**: none
";
        let parsed = parse_tasks_at(body, 10).expect("parses");
        assert_eq!(parsed.tasks.len(), 3);
        let unstored: Vec<&Finding> = parsed
            .findings
            .iter()
            .filter(|finding| finding.class == "plan/text-unstored")
            .collect();
        assert_eq!(unstored.len(), 2, "{:?}", parsed.findings);

        assert_eq!(unstored[0].severity, WARNING);
        assert_eq!(unstored[0].ids, vec![1]);
        for fragment in [
            "line 14",
            "`Notes`",
            "(and 2 more lines)",
            "`tasks render` removes it",
            "`- **Detail**:`",
        ] {
            assert!(
                unstored[0].detail.contains(fragment),
                "{fragment}: {:?}",
                unstored[0]
            );
        }

        assert_eq!(unstored[1].ids, vec![2]);
        assert!(
            unstored[1].detail.contains("line 20") && unstored[1].detail.contains("a stray bullet"),
            "{:?}",
            unstored[1]
        );
    }

    #[test]
    fn prose_under_a_phase_heading_warns_against_the_heading() {
        let body = "\
Intro before any heading.

### Phase 1 (parallel)

These run together.

#### 1. First task [S]
- **Files**: `a.rs`
";
        let parsed = parse_tasks_at(body, 1).expect("parses");
        assert_eq!(parsed.findings.len(), 2, "{:?}", parsed.findings);
        assert!(parsed.findings.iter().all(|f| f.ids.is_empty()));
        assert!(
            parsed.findings[0]
                .detail
                .contains("line 1: the `## Tasks` section"),
            "{:?}",
            parsed.findings[0]
        );
        assert!(
            parsed.findings[1]
                .detail
                .contains("line 3: phase \"Phase 1 (parallel)\" carries, at line 5,"),
            "{:?}",
            parsed.findings[1]
        );

        let taskless = parse_tasks_at("### Step one\n\nprose\n- a bullet\n", 1).expect("parses");
        assert!(taskless.tasks.is_empty());
        assert!(taskless.findings.is_empty(), "{:?}", taskless.findings);
    }

    #[test]
    fn a_multi_word_parenthetical_stays_with_the_path_it_trails() {
        let body = "### 1. Split the store [S]\n\
                    - **Files**: `src/a.rs` (new stub file), `src/b.rs`\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.files, vec!["src/a.rs", "src/b.rs"]);
        assert_eq!(task.file_notes, vec!["(new stub file)", ""]);
    }

    #[test]
    fn a_comma_inside_an_annotation_does_not_split_the_entry() {
        let body = "### 1. Split the store [S]\n\
                    - **Files**: `src/a.rs` (new, generated), `src/b.rs` (moved, then renamed), \
                    `src/c.rs` — extend `Row`, then the SELECT\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(
            task.files,
            vec!["src/a.rs", "src/b.rs", "src/c.rs"],
            "an annotation's comma opened a claim on a path nobody wrote"
        );
        assert_eq!(
            task.file_notes,
            vec![
                "(new, generated)",
                "(moved, then renamed)",
                "— extend `Row`, then the SELECT",
            ]
        );
    }

    #[test]
    fn a_backticked_identifier_inside_a_dash_note_claims_no_file() {
        let body = "### 1. Split the store [S]\n\
                    - **Files**: `src/a.rs` — touches `foo`, `bar` too\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(
            task.files,
            vec!["src/a.rs"],
            "a note's identifier became a claim"
        );
        assert_eq!(task.file_notes, vec!["— touches `foo`, `bar` too"]);
    }

    #[test]
    fn a_backticked_path_after_a_dash_note_is_the_next_claim() {
        let body = "### 1. Split the store [S]\n\
                    - **Files**: `src/a.rs` — extend `Row`, `src/b.rs` — mirror it, `Cargo.toml`\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.files, vec!["src/a.rs", "src/b.rs", "Cargo.toml"]);
        assert_eq!(task.file_notes, vec!["— extend `Row`", "— mirror it", ""]);
    }

    /// The claim stays dropped, so the warning is the one trace an
    /// extensionless path in a dash note leaves.
    #[test]
    fn a_non_path_span_after_a_comma_in_a_dash_note_warns() {
        let body = "### 3. Wire the build [S]\n- **Files**: `a.rs` — note, `Makefile`\n";
        let parsed = parse_tasks_at(body, 20).expect("parses");
        assert_eq!(parsed.tasks[0].files, vec!["a.rs"]);
        assert_eq!(parsed.findings.len(), 1, "{:?}", parsed.findings);

        let finding = &parsed.findings[0];
        assert_eq!(finding.class, "plan/files-span-unclaimed");
        assert_eq!(finding.severity, WARNING);
        assert_eq!(finding.ids, vec![3]);
        for fragment in ["line 21", "Wire the build", "`Makefile`", "bulleted"] {
            assert!(finding.detail.contains(fragment), "{fragment}: {finding:?}");
        }

        for quiet in [
            "- **Files**: `a.rs` — extend it, then the SELECT",
            "- **Files**: `a.rs` — extend `Row`, `b.rs` — mirror it",
            "- **Files**: `a.rs`, `Makefile`, `src/b.rs`",
            "- **Files**:\n  - `a.rs` — note, `Makefile`",
        ] {
            let body = format!("### 3. Wire the build [S]\n{quiet}\n");
            let parsed = parse_tasks_at(&body, 1).expect("parses");
            assert!(parsed.findings.is_empty(), "{quiet}: {:?}", parsed.findings);
        }
    }

    #[test]
    fn files_none_yields_no_entries() {
        let second = &parse_tasks(PHASED).expect("parses")[1];
        assert!(second.files.is_empty(), "{:?}", second.files);
    }

    #[test]
    fn a_wrapped_depends_on_splits_into_ids_and_a_note() {
        let first = &parse_tasks(PHASED).expect("parses")[0];
        assert_eq!(first.needs, vec![1, 3]);
        assert_eq!(
            first.deps_note,
            "sequential commits keep the phases independently revertible"
        );
    }

    #[test]
    fn blocked_by_is_a_depends_on_synonym() {
        let second = &parse_tasks(PHASED).expect("parses")[1];
        assert_eq!(second.needs, vec![3]);
        assert_eq!(second.deps_note, "");
    }

    #[test]
    fn a_legacy_effort_line_sets_the_effort() {
        let second = &parse_tasks(PHASED).expect("parses")[1];
        assert_eq!(second.effort, Some(Effort::M));
    }

    #[test]
    fn a_backlog_line_splits_closes_from_refs() {
        let body = "### 1. Ship it [S]\n\
                    - **Backlog**: `B-aaaa1111`, refs `B-bbbb2222`, B-cccc3333 (partial), \
                    refs B-dddd4444 — follow-up\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.backlog_closes, vec!["B-aaaa1111", "B-cccc3333"]);
        assert_eq!(task.backlog_refs, vec!["B-bbbb2222", "B-dddd4444"]);
    }

    #[test]
    fn refs_qualifies_only_its_own_entry() {
        let body = "### 1. Ship it [S]\n- **Backlog**: refs B-1, B-2\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.backlog_refs, vec!["B-1"]);
        assert_eq!(task.backlog_closes, vec!["B-2"]);
    }

    #[test]
    fn a_backlog_id_after_a_dash_note_is_still_linked() {
        let body = "### 1. Ship it [S]\n\
                    - **Backlog**: `B-1` — follow-up, `B-2`, B-3 — again, refs `B-4` (partial)\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.backlog_closes, vec!["B-1", "B-2", "B-3"]);
        assert_eq!(task.backlog_refs, vec!["B-4"]);
    }

    #[test]
    fn a_backlog_id_named_in_a_dash_note_is_not_linked() {
        let body = "### 1. Ship it [S]\n\
                    - **Backlog**: `B-1` — follow-up, B-2 is related, `B-note`\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.backlog_closes, vec!["B-1"]);
        assert!(task.backlog_refs.is_empty(), "{:?}", task.backlog_refs);
    }

    #[test]
    fn a_backlog_line_of_none_links_nothing() {
        let body = "### 1. Ship it [S]\n- **Backlog**: none\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert!(task.backlog_closes.is_empty(), "{:?}", task.backlog_closes);
        assert!(task.backlog_refs.is_empty(), "{:?}", task.backlog_refs);
    }

    /// A dropped id is not a parsing nicety: the store then holds no edge for
    /// a task that declares one, and the scheduler dispatches it in the first
    /// batch alongside the work it waits on.
    #[test]
    fn an_id_stated_after_prose_is_still_an_edge() {
        let body = "### 12. Wire the deps [M]\n\
                    - **Depends on**: round-2 task 18 (wire-task-deps creation), 4\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.needs, vec![4], "the plainly-stated `4` was dropped");
        assert_eq!(
            task.deps_note, "round-2 task 18 (wire-task-deps creation)",
            "a reference to another plan's task is prose, not an edge"
        );
    }

    /// `+` joins ids as a comma does, and an entry that is not an id is the
    /// note whether it stands before or after one.
    #[test]
    fn a_plus_joined_line_yields_every_id_and_keeps_the_prose() {
        let body = "### 5. Probe the keystrokes [M]\n\
                    - **Blocked by**: 4 + pre-flight probe (the probe file must exist) + 7\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.needs, vec![4, 7]);
        assert_eq!(
            task.deps_note,
            "pre-flight probe (the probe file must exist)"
        );
    }

    #[test]
    fn a_range_is_every_id_it_spans() {
        let body = "### 16. Wire the whole round [L]\n\
                    - **Depends on**: 1-3 (the foundations), 7–8, 12\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.needs, vec![1, 2, 3, 7, 8, 12]);
        assert_eq!(task.deps_note, "the foundations");
        let body = "### 2. Inverted [S]\n- **Depends on**: 5-3\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert!(task.needs.is_empty(), "{:?}", task.needs);
        assert_eq!(task.deps_note, "5-3");
    }

    #[test]
    fn a_cross_plan_reference_alone_leaves_no_edge_behind() {
        let body = "### 12. Wire the deps [M]\n- **Depends on**: round-2 task 18\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert!(
            task.needs.is_empty(),
            "a task this plan does not number became an edge: {:?}",
            task.needs
        );
        assert_eq!(task.deps_note, "round-2 task 18");
    }

    #[test]
    fn a_rationale_against_none_is_a_note_and_no_edge() {
        let body = "### 1. Scaffold the module tree [L]\n\
                    - **Depends on**: none (foundational)\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert!(task.needs.is_empty(), "{:?}", task.needs);
        assert_eq!(task.deps_note, "foundational");
    }

    #[test]
    fn an_em_dash_depends_on_is_empty() {
        let body = "### 1. Scaffold the module tree [L]\n- **Depends on**: —\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert!(task.needs.is_empty());
        assert_eq!(task.deps_note, "");
    }

    #[test]
    fn prose_fields_keep_their_continuation_lines() {
        let first = &parse_tasks(PHASED).expect("parses")[0];
        assert_eq!(
            first.action,
            "Implement `Store` and `TaskRow`,\npreserving the field order shown in Approach."
        );
        assert_eq!(first.detail, "Mirror `tomlctl/src/backlog/mod.rs`.");
        assert_eq!(
            first.acceptance,
            "inline unit tests round-trip a fixture store."
        );
    }

    #[test]
    fn a_value_starting_on_the_next_line_is_captured() {
        let body = "### 3. Do the thing [S]\n- **Action**:\n  Implement the verb.\n";
        let task = &parse_tasks(body).expect("parses")[0];
        assert_eq!(task.action, "Implement the verb.");
    }

    #[test]
    fn an_effort_tag_outside_the_vocabulary_names_the_line() {
        let err = parse_tasks("### 7. Split the store [M-leaning-L]\n")
            .expect_err("non-vocabulary effort is an error")
            .to_string();
        assert!(err.contains("M-leaning-L"), "{err}");
        assert!(err.contains("line 1"), "{err}");
    }

    /// The body opens at document line 12, so its second line is line 13 —
    /// the line a reader navigates to, not an offset into the section.
    #[test]
    fn a_reported_line_counts_from_the_bodys_place_in_the_document() {
        let err = parse_tasks_at("\n#### 7. Split the store [M-leaning-L]\n", 12)
            .expect_err("non-vocabulary effort is an error")
            .to_string();
        assert!(err.contains("line 13"), "{err}");
    }

    #[test]
    fn a_non_integer_id_names_the_line() {
        let err = parse_tasks("### 12a. Split the store [S]\n")
            .expect_err("non-integer id is an error")
            .to_string();
        assert!(err.contains("12a"), "{err}");
    }

    #[test]
    fn every_pattern_compiles() {
        let patterns = [
            heading_re(),
            deep_heading_re(),
            malformed_id_re(),
            effort_tag_re(),
            field_re(),
            label_re(),
            bullet_re(),
        ];

        // A dev-dependency unifies `regex/unicode` on, so a shorthand class is
        // accepted here and rejected by the dependency-free release build, where
        // `Regex::new` returns `Err` and the `expect` panics. Only a textual scan
        // of the pattern catches that from a test.
        for re in patterns {
            for shorthand in [r"\d", r"\D", r"\s", r"\S", r"\w", r"\W", r"\b", r"\B"] {
                assert!(
                    !re.as_str().contains(shorthand),
                    "`{shorthand}` in `{}` — spell the class out in ASCII",
                    re.as_str()
                );
            }
        }

        assert!(heading_re().is_match("### 1. Scaffold the module tree [L]"));
        assert!(malformed_id_re().is_match("### 12a. Split the store [S]"));
        assert!(effort_tag_re().is_match("Split the store [M-leaning-L]"));
        assert!(field_re().is_match("- **Depends on**: 1, 3"));
        assert!(bullet_re().is_match("- `lumina/web/src/api/repo-links.ts`"));

        let negated = Regex::new(r"^[^0-9]+$").expect("negated ASCII class compiles");
        assert!(negated.is_match("abc"));
        assert!(!negated.is_match("12"));
    }
}
