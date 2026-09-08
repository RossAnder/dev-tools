//! Parser for a plan's `## Tasks` section.
//!
//! Input is LF-only: callers pass `Section::body_lf`. A numbered heading opens
//! a task and every other heading is a phase label the tasks under it carry;
//! a `#` line inside a fenced block is neither, so a fenced field value cannot
//! end a task. A field line's value continues onto following lines indented
//! two spaces or more, and may start on the first of them rather than after
//! the colon.
//!
//! Patterns spell every class out in ASCII. The binary resolves `regex`
//! without its unicode features, so a `\d`/`\s`/`\w` shorthand makes
//! `Regex::new` return `Err` at startup there — while a dev-dependency
//! unifies those features back on, so a test run accepts it. That asymmetry
//! is what `every_pattern_compiles` scans the pattern text for.

use std::sync::OnceLock;

use anyhow::{Result, bail};
use regex::Regex;

use super::markdown::FenceState;
use super::schema::Effort;

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
    pub(crate) needs: Vec<u32>,
    pub(crate) deps_note: String,
    pub(crate) action: String,
    pub(crate) detail: String,
    pub(crate) acceptance: String,
}

/// Line numbers relative to `section_body` itself, which is what a fixture
/// standing in for a whole document wants. Every live caller holds the
/// document and goes through `parse_tasks_at`.
#[cfg(test)]
pub(crate) fn parse_tasks(section_body: &str) -> Result<Vec<ParsedTask>> {
    parse_tasks_at(section_body, 1)
}

/// `first_line` is the document line `section_body`'s first line stands on, so
/// every reported number names a line of the plan rather than an offset into a
/// section a reader cannot see.
pub(crate) fn parse_tasks_at(section_body: &str, first_line: usize) -> Result<Vec<ParsedTask>> {
    let mut tasks: Vec<ParsedTask> = Vec::new();
    let mut current: Option<ParsedTask> = None;
    let mut open: Option<OpenField> = None;
    let mut pending_blank = false;
    let mut fence = FenceState::default();
    let mut phase = String::new();
    let mut phase_depth = 0u32;

    for (index, line) in section_body.lines().enumerate() {
        let line_no = first_line + index;
        let fenced = fence.consume(line);

        if !fenced && line.starts_with('#') {
            close_field(current.as_mut(), open.take())?;
            pending_blank = false;
            tasks.extend(current.take());
            match open_heading(line, line_no)? {
                Some(task) => {
                    current = Some(ParsedTask {
                        phase: phase.clone(),
                        phase_depth,
                        ..task
                    });
                }
                None => {
                    current = None;
                    if let Some(label) = phase_label(line) {
                        phase = label.to_string();
                        phase_depth = heading_depth(line);
                    }
                }
            }
            continue;
        }

        if let Some(caps) = field_re().captures(line)
            && current.is_some()
        {
            close_field(current.as_mut(), open.take())?;
            pending_blank = false;
            let label = caps.get(1).map_or("", |m| m.as_str());
            let inline = caps.get(2).map_or("", |m| m.as_str()).trim_end();
            let mut lines = Vec::new();
            if !inline.is_empty() {
                lines.push(inline.to_string());
            }
            open = Some(OpenField {
                kind: Field::from_label(label),
                line_no,
                lines,
            });
            continue;
        }

        if open.is_none() {
            continue;
        }

        if line.trim().is_empty() {
            pending_blank = true;
        } else if let Some(rest) = dedent(line) {
            let field = open.as_mut().expect("field is open");
            if pending_blank {
                field.lines.push(String::new());
                pending_blank = false;
            }
            field.lines.push(rest.to_string());
        } else {
            close_field(current.as_mut(), open.take())?;
            pending_blank = false;
        }
    }

    close_field(current.as_mut(), open.take())?;
    tasks.extend(current.take());
    Ok(tasks)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Files,
    DependsOn,
    Action,
    Detail,
    Acceptance,
    Effort,
    Unknown,
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
            _ => Self::Unknown,
        }
    }
}

struct OpenField {
    kind: Field,
    line_no: usize,
    lines: Vec<String>,
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

fn close_field(task: Option<&mut ParsedTask>, open: Option<OpenField>) -> Result<()> {
    let (Some(task), Some(open)) = (task, open) else {
        return Ok(());
    };
    let OpenField {
        kind,
        line_no,
        lines,
    } = open;

    match kind {
        Field::Files => task.files = parse_files(&lines),
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
        Field::Unknown => {}
    }
    Ok(())
}

fn parse_files(lines: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    for line in lines {
        let line = line.trim();
        // A bulleted line is one path plus prose; a bare line is a comma list.
        if bullet_re().is_match(line) {
            push_file(&mut files, bullet_re().replace(line, "").as_ref());
        } else {
            for raw in line.split(',') {
                push_file(&mut files, raw);
            }
        }
    }
    files
}

fn push_file(files: &mut Vec<String>, raw: &str) {
    let cut = raw.split('(').next().unwrap_or(raw);
    let cut = cut.split(" — ").next().unwrap_or(cut);
    let entry = cut.replace('`', "").trim().to_string();
    if entry.is_empty() || is_empty_marker(&entry) {
        return;
    }
    files.push(entry);
}

/// Integers up to the first `(`; everything after them is the note, with the
/// outer parentheses dropped so the renderer can re-add them.
fn parse_depends(lines: &[String]) -> (Vec<u32>, String) {
    let value = lines.join(" ");
    let value = value.trim();
    let (head, tail) = match value.find('(') {
        Some(at) => (&value[..at], value[at..].trim()),
        None => (value, ""),
    };

    let mut needs = Vec::new();
    let mut trailing: Vec<&str> = Vec::new();
    let mut leading = true;
    for token in head.split(',') {
        let token = token.trim();
        if token.is_empty() || is_empty_marker(token) {
            continue;
        }
        match token.parse::<u32>() {
            Ok(id) if leading => needs.push(id),
            _ => {
                leading = false;
                trailing.push(token);
            }
        }
    }
    needs.sort_unstable();
    needs.dedup();

    let tail = tail
        .strip_prefix('(')
        .map_or(tail, |inner| inner.strip_suffix(')').unwrap_or(inner));
    let mut note = trailing.join(", ");
    if !tail.is_empty() {
        if !note.is_empty() {
            note.push(' ');
        }
        note.push_str(tail);
    }
    (needs, note)
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
            r"^- \*\*(Files|Depends on|Blocked-by|Blocked by|Action|Detail|Acceptance|Effort)\*\*:[ \t]*(.*)$",
        )
        .expect("field regex compiles")
    })
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
            malformed_id_re(),
            effort_tag_re(),
            field_re(),
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
