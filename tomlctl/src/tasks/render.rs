//! Rendering the store back into a plan's policy, tasks and dependency-graph sections.
//!
//! Every id list a section carries — the `Checkpoint after` bullet, a marker's
//! `after` and its `dependency closure` — is derived from the graph on each
//! render and never stored, so a stale list cannot outlive the edges it
//! summarises. A marker's two lists are different sets and are labelled apart:
//! `after tasks` is the group's antichain, which `tasks closure` reports as
//! `maximal` alongside the `members` it closes over, and `dependency closure`
//! is the upward walk the same verb reports as `dependency_closure`.
//!
//! Bodies re-indent two spaces under their label line, which is the
//! continuation form the importer reads back. `(INVALID CUT)` is rendered
//! rather than suppressed so the defect is visible in the document; the
//! importer strips it, so it cannot accumulate across a render→import→render
//! cycle.

use std::collections::BTreeSet;

use anyhow::Result;

use super::graph::{Graph, Group, nodes_of};
use super::markdown::{insert_section_after, insert_section_before, replace_section, sections};
use super::schema::{Checkpoint, Policy, Store, TaskRow};

/// The sections `render` owns; every other byte of the plan is preserved.
pub(crate) const SECTION_TITLES: [&str; 3] = ["Execution Policy", "Tasks", "Dependency Graph"];

const PREAMBLE: &str =
    "Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.";

const INVALID_CUT: &str = "(INVALID CUT)";

/// Names the upward walk rather than the group, so no reader can take it for
/// the member set `tasks closure` reports under `members`. The importer's
/// clause stripper accepts the `dependency ` prefix, so the label stays
/// recomputed-and-discarded rather than accumulating into a rationale.
const CLOSURE_LABEL: &str = "dependency closure";

const EMPTY: &str = "—";

const MIN_HEADING_DEPTH: u32 = 3;
const MAX_HEADING_DEPTH: u32 = 6;

/// Section bodies without their `## ` heading line, which is what
/// `markdown::replace_section` and `insert_section_after` take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rendered {
    pub(crate) policy: String,
    pub(crate) tasks: String,
    pub(crate) graph: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) class: &'static str,
    pub(crate) severity: &'static str,
    pub(crate) ids: Vec<u32>,
    pub(crate) detail: String,
}

/// Errors on a cycle or a dangling edge: checkpoint closures are undefined
/// through one, and a marker naming the wrong tasks is worse than a refusal.
pub(crate) fn render_sections(store: &Store) -> Result<Rendered> {
    let nodes = nodes_of(&store.items);
    let graph = Graph::build(&nodes)?;
    let order: Vec<String> = store
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.clone())
        .collect();
    let groups = graph.groups(&order)?;
    let closures = groups
        .iter()
        .map(|group| closure(&graph, group))
        .collect::<Result<Vec<_>>>()?;

    Ok(Rendered {
        policy: render_policy(&store.policy, &groups),
        tasks: render_tasks(store),
        graph: render_graph(&store.checkpoints, &groups, &closures),
    })
}

pub(crate) fn render_into_plan(store: &Store, plan_src: &str) -> Result<String> {
    let rendered = render_sections(store)?;

    let ordered = reorder_owned_sections(plan_src);
    let mut out = replace_or_insert_before(&ordered, "Execution Policy", "Tasks", &rendered.policy);
    out = replace_or_insert(&out, "Tasks", "Execution Policy", &rendered.tasks);
    out = replace_or_insert(&out, "Dependency Graph", "Tasks", &rendered.graph);
    Ok(out)
}

pub(crate) fn check_render_drift(store: &Store, plan_src: &str) -> Result<Option<Finding>> {
    let want = lf(&render_into_plan(store, plan_src)?);
    let got = lf(plan_src);
    if want == got {
        return Ok(None);
    }

    let drifted: Vec<&str> = SECTION_TITLES
        .iter()
        .copied()
        .filter(|title| body_of(&got, title) != body_of(&want, title))
        .collect();
    let mut parts: Vec<String> = Vec::new();
    if !drifted.is_empty() {
        parts.push(format!(
            "plan sections out of date with the store: {}",
            drifted.join(", ")
        ));
    }
    // A permutation leaves every body byte-identical, so without this the one
    // defect the render corrects would report as an unnamed difference.
    let order = owned_order(&got);
    if order != canonical_order(&order) {
        parts.push(format!(
            "plan sections out of canonical order: {}",
            order.join(", ")
        ));
    }
    if parts.is_empty() {
        parts.push("the plan differs from the store's render".to_string());
    }
    let detail = parts.join("; ");

    Ok(Some(Finding {
        class: "render/drift",
        severity: "warning",
        ids: Vec::new(),
        detail,
    }))
}

/// Everything that must be complete at the group's cut.
fn closure(graph: &Graph<'_>, group: &Group) -> Result<Vec<u32>> {
    let mut ids: BTreeSet<u32> = BTreeSet::new();
    for member in &group.members {
        ids.extend(graph.closure_up(*member)?);
    }
    Ok(ids.into_iter().collect())
}

fn render_policy(policy: &Policy, groups: &[Group]) -> String {
    let after: BTreeSet<u32> = groups
        .iter()
        .flat_map(|group| group.maximal.iter().copied())
        .collect();
    let after: Vec<u32> = after.into_iter().collect();

    let mut body = String::from("\n");
    body.push_str(&format!(
        "- **Checkpoints**: {}{}\n",
        policy.checkpoints,
        clause(&policy.checkpoints_note)
    ));
    body.push_str(&format!(
        "- **Checkpoint after**: {}\n",
        if after.is_empty() {
            EMPTY.to_string()
        } else {
            format!("tasks {}", join_ids(&after))
        }
    ));
    body.push_str(&format!(
        "- **Max parallel agents**: {}{}\n",
        policy.max_parallel,
        clause(&policy.max_parallel_note)
    ));
    body.push_str(&format!(
        "- **Commit granularity**: {}{}\n",
        policy.commit_granularity,
        clause(&policy.commit_granularity_note)
    ));

    let note = policy.note.trim();
    if !note.is_empty() {
        body.push('\n');
        body.push_str(note);
        body.push('\n');
    }
    body.push('\n');
    body
}

/// A bullet's trailing clause, on the line the author wrote it on. One line:
/// a wrapped clause folds, and an embedded blank line would end the bullet
/// list and re-import as prose belonging to no bullet.
fn clause(note: &str) -> String {
    let note = collapse(note);
    match note.is_empty() {
        true => String::new(),
        false => format!(" {note}"),
    }
}

/// Store order, not id order: it is the plan's own task order, and sorting
/// would reshuffle a document whose numbering runs across phases.
///
/// A phase heading is re-emitted only where the label or its depth changes, so
/// a run of rows under one label yields the one heading the author wrote.
fn render_tasks(store: &Store) -> String {
    let rows = &store.items;
    let mut body = String::from("\n");
    let mut phase: Option<(&str, u32)> = None;
    for row in rows {
        let current = (!row.phase.is_empty()).then(|| (row.phase.as_str(), depth(row.phase_depth)));
        if current != phase {
            if let Some((label, at)) = current {
                body.push_str(&format!("{} {label}\n\n", hashes(at)));
            }
            phase = current;
        }
        body.push_str(&format!(
            "{} {}. {} [{}]\n",
            hashes(row.heading_depth),
            row.id,
            row.title,
            row.effort.as_str()
        ));
        body.push_str(&format!("- **Files**: {}\n", file_list(store, row)));
        body.push_str(&format!("- **Depends on**: {}\n", depends_on(row)));
        for (label, text) in [
            ("Action", &row.action),
            ("Detail", &row.detail),
            ("Acceptance", &row.acceptance),
        ] {
            if let Some(line) = field_line(label, text) {
                body.push_str(&line);
            }
        }
        body.push('\n');
    }
    body
}

fn render_graph(checkpoints: &[Checkpoint], groups: &[Group], closures: &[Vec<u32>]) -> String {
    let mut body = String::from("\n");
    body.push_str(PREAMBLE);
    body.push_str("\n\n");

    for (group, closure) in groups.iter().zip(closures) {
        // A group with no members has no `after` list the importer can match.
        if group.maximal.is_empty() {
            continue;
        }
        let mut marker = format!(
            "{EMPTY} CHECKPOINT {} after tasks {} {EMPTY} {CLOSURE_LABEL}: {}.",
            group.id,
            join_ids(&group.maximal),
            join_ids(closure)
        );
        let rationale = checkpoints
            .iter()
            .find(|checkpoint| checkpoint.id == group.id)
            .map_or("", |checkpoint| checkpoint.rationale.as_str());
        let rationale = collapse(rationale);
        if !rationale.is_empty() {
            marker.push(' ');
            marker.push_str(&rationale);
        }
        if !group.valid_cut {
            marker.push(' ');
            marker.push_str(INVALID_CUT);
        }
        body.push_str(&marker);
        body.push_str("\n\n");
    }
    body
}

/// The containment guard: two hashes would open a `## ` section and split the
/// one being written, and past six is no heading at all.
fn depth(stored: u32) -> u32 {
    stored.clamp(MIN_HEADING_DEPTH, MAX_HEADING_DEPTH)
}

fn hashes(stored: u32) -> String {
    "#".repeat(depth(stored) as usize)
}

fn field_line(label: &str, text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut lines = text.split('\n');
    let mut out = format!("- **{label}**: {}\n", lines.next().unwrap_or_default());
    for line in lines {
        let line = line.trim_end();
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&format!("  {line}\n"));
        }
    }
    Some(out)
}

fn depends_on(row: &TaskRow) -> String {
    let ids: BTreeSet<u32> = row
        .needs
        .iter()
        .chain(row.coupling.iter())
        .copied()
        .collect();
    let ids: Vec<u32> = ids.into_iter().collect();
    let list = if ids.is_empty() {
        EMPTY.to_string()
    } else {
        join_ids(&ids)
    };
    let note = collapse(&row.deps_note);
    if note.is_empty() {
        list
    } else {
        format!("{list} ({note})")
    }
}

/// Each path with whatever annotation the plan wrote against it. The parser
/// cuts an entry at its first `(` or ` — `, so an annotation carrying a comma
/// would re-import as a further path.
fn file_list(store: &Store, row: &TaskRow) -> String {
    if row.files.is_empty() {
        return EMPTY.to_string();
    }
    row.files
        .iter()
        .map(|file| {
            match store
                .file_note(&row.r#ref, file)
                .map(collapse)
                .filter(|note| !note.is_empty())
            {
                Some(note) => format!("`{file}` {note}"),
                None => format!("`{file}`"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn join_ids(ids: &[u32]) -> String {
    ids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// One marker or one field value is one paragraph, so an embedded blank line
/// would split it and re-import as two.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn replace_or_insert(src: &str, title: &str, after: &str, body: &str) -> String {
    if has_section(src, title) {
        replace_section(src, title, body)
    } else {
        insert_section_after(src, after, title, body)
    }
}

/// The insertion anchors above place a section that is ABSENT; a section
/// already in the document at the wrong place is out of their reach, and a
/// render that left it there would be a faithful no-op — drift `--check` has
/// nothing to report on.
///
/// The slots the owned sections occupy are refilled in canonical order by
/// rewriting the heading lines in place: every other section keeps its
/// position, and the body left under a rewritten heading is replaced from the
/// store in the same pass, so no stale body survives the move.
fn reorder_owned_sections(src: &str) -> String {
    let slots: Vec<_> = sections(src)
        .into_iter()
        .filter(|section| SECTION_TITLES.contains(&section.title.as_str()))
        .collect();
    let order: Vec<String> = slots.iter().map(|section| section.title.clone()).collect();
    let canonical = canonical_order(&order);
    if order == canonical {
        return src.to_string();
    }

    let mut out = String::with_capacity(src.len());
    let mut at = 0usize;
    for (slot, title) in slots.iter().zip(&canonical) {
        out.push_str(&src[at..slot.start]);
        out.push_str("## ");
        out.push_str(title);
        out.push_str(heading_terminator(&src[slot.start..slot.body_range.start]));
        at = slot.body_range.start;
    }
    out.push_str(&src[at..]);
    out
}

fn owned_order(src: &str) -> Vec<String> {
    sections(src)
        .into_iter()
        .map(|section| section.title)
        .filter(|title| SECTION_TITLES.contains(&title.as_str()))
        .collect()
}

/// The titles a document holds, sorted by their canonical rank: a plan
/// carrying two of the three is judged on their relative order alone, and the
/// sort's stability leaves a title the document holds twice in its own order
/// rather than shuffling the pair.
fn canonical_order(present: &[String]) -> Vec<String> {
    let mut sorted = present.to_vec();
    sorted.sort_by_key(|title| rank(title));
    sorted
}

fn rank(title: &str) -> usize {
    SECTION_TITLES
        .iter()
        .position(|owned| *owned == title)
        .unwrap_or(SECTION_TITLES.len())
}

/// The heading line's own ending, which is empty for a heading standing at EOF
/// with no terminator at all.
fn heading_terminator(heading_line: &str) -> &'static str {
    if heading_line.ends_with("\r\n") {
        "\r\n"
    } else if heading_line.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

/// Anchored on the section it precedes rather than on that section's
/// predecessor, which does not exist when the successor opens the document.
fn replace_or_insert_before(src: &str, title: &str, before: &str, body: &str) -> String {
    if has_section(src, title) {
        replace_section(src, title, body)
    } else {
        insert_section_before(src, before, title, body)
    }
}

fn has_section(src: &str, title: &str) -> bool {
    sections(src).iter().any(|section| section.title == title)
}

fn body_of(src: &str, title: &str) -> Option<String> {
    sections(src)
        .into_iter()
        .find(|section| section.title == title)
        .map(|section| section.body_lf(src))
}

fn lf(src: &str) -> String {
    src.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::parse_policy::{Marker, parse_markers, parse_policy};
    use crate::tasks::parse_tasks::parse_tasks_at;
    use crate::tasks::schema::{DEFAULT_HEADING_DEPTH, Effort, FileNote, Status};
    use crate::tasks::slug::derive_ref;

    const PLAN: &str = "# Plan: Demo\n\n## Approach\n\nprose\n\n## Execution Policy\n\nstale\n\n\
        ## Tasks\n\nstale\n\n## Dependency Graph\n\nstale\n\n## Risks\n\nrisk\n";

    fn row(
        id: u32,
        title: &str,
        effort: Effort,
        checkpoint: &str,
        needs: &[u32],
        coupling: &[u32],
    ) -> TaskRow {
        TaskRow {
            id,
            r#ref: derive_ref(title),
            title: title.to_string(),
            effort,
            status: Status::Pending,
            checkpoint: checkpoint.to_string(),
            phase: String::new(),
            phase_depth: 0,
            heading_depth: DEFAULT_HEADING_DEPTH,
            files: vec![format!("tomlctl/src/tasks/t{id}.rs")],
            needs: needs.to_vec(),
            coupling: coupling.to_vec(),
            deps_note: String::new(),
            action: format!("Do task {id}."),
            detail: String::new(),
            acceptance: format!("Task {id} holds."),
            agent: String::new(),
            commit: String::new(),
        }
    }

    /// Six tasks over three groups. Task 3 couples to 2 without needing it, so
    /// dropping `coupling` from the render loses an edge; group B's only member
    /// depends on a group-C task, so B is not a valid cut; task 6 belongs to no
    /// group.
    fn fixture() -> Store {
        let mut items = vec![
            row(1, "Scaffold the store", Effort::S, "A", &[], &[]),
            row(2, "Build the graph engine", Effort::M, "A", &[1], &[]),
            row(3, "Render the sections", Effort::M, "A", &[1], &[2]),
            row(4, "Import the plan", Effort::S, "B", &[5], &[]),
            row(5, "Parse the policy", Effort::S, "C", &[1], &[]),
            row(6, "Smoke the corpus", Effort::S, "", &[3], &[]),
        ];
        items[2].action = "First line.\n\nSecond line, after a blank one.".to_string();
        items[2].detail = "Bodies re-indent two spaces.".to_string();
        items[3].deps_note = "5 is what this task needs".to_string();
        items[4].files = Vec::new();

        Store {
            plan_path: "docs/plans/demo.md".to_string(),
            policy: Policy {
                checkpoints: "milestones".to_string(),
                max_parallel: 6,
                commit_granularity: "per-task".to_string(),
                note: "The cuts are reviewed by hand while the store is young.".to_string(),
                commit_granularity_note: "— tasks 2 and 3 land in one commit".to_string(),
                ..Policy::default()
            },
            file_notes: vec![FileNote {
                r#ref: derive_ref("Render the sections"),
                file: "tomlctl/src/tasks/t3.rs".to_string(),
                note: "(new)".to_string(),
            }],
            checkpoints: vec![
                Checkpoint {
                    id: "A".to_string(),
                    rationale: "the store and the engine — buildable alone".to_string(),
                },
                Checkpoint {
                    id: "B".to_string(),
                    rationale: "the importer".to_string(),
                },
                Checkpoint {
                    id: "C".to_string(),
                    rationale: "the parsers".to_string(),
                },
            ],
            items,
            ..Store::default()
        }
    }

    fn section(plan: &str, title: &str) -> String {
        body_of(plan, title).unwrap_or_else(|| panic!("`{title}` section in {plan}"))
    }

    fn markers(plan: &str) -> Vec<Marker> {
        parse_markers(&section(plan, "Dependency Graph")).expect("markers parse")
    }

    #[test]
    fn a_rendered_plan_re_parses_to_the_same_store() {
        let store = fixture();
        let plan = render_into_plan(&store, PLAN).expect("renders");

        let tasks = parse_tasks_at(&section(&plan, "Tasks"), 1).expect("tasks parse");
        assert_eq!(
            tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
            store.items.iter().map(|row| row.id).collect::<Vec<_>>()
        );
        for (row, parsed) in store.items.iter().zip(&tasks) {
            let id = row.id;
            assert_eq!(parsed.title, row.title, "title for {id}");
            assert_eq!(derive_ref(&parsed.title), row.r#ref, "ref for {id}");
            assert_eq!(parsed.effort, Some(row.effort), "effort for {id}");
            assert_eq!(parsed.files, row.files, "files for {id}");
            let union: Vec<u32> = row
                .needs
                .iter()
                .chain(row.coupling.iter())
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            assert_eq!(parsed.needs, union, "depends for {id}");
            assert_eq!(parsed.deps_note, row.deps_note, "deps_note for {id}");
            assert_eq!(parsed.action, row.action, "action for {id}");
            assert_eq!(parsed.detail, row.detail, "detail for {id}");
            assert_eq!(parsed.acceptance, row.acceptance, "acceptance for {id}");
        }

        let markers = markers(&plan);
        assert_eq!(
            markers.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );
        for (marker, checkpoint) in markers.iter().zip(&store.checkpoints) {
            assert_eq!(marker.id, checkpoint.id);
            assert_eq!(
                marker.rationale, checkpoint.rationale,
                "rationale for {}",
                marker.id
            );
        }
        assert_eq!(markers[0].after, vec![3]);
        assert_eq!(markers[1].after, vec![4]);
        assert_eq!(markers[2].after, vec![5]);
        assert!(
            plan.contains(&format!(
                "CHECKPOINT A after tasks 3 {EMPTY} {CLOSURE_LABEL}: 1, 2, 3."
            )),
            "{plan}"
        );

        let policy =
            parse_policy(Some(&section(&plan, "Execution Policy"))).expect("policy parses");
        assert_eq!(policy.checkpoints, store.policy.checkpoints);
        assert_eq!(policy.max_parallel, store.policy.max_parallel);
        assert_eq!(policy.commit_granularity, store.policy.commit_granularity);
        assert_eq!(policy.note, store.policy.note);

        let from_markers: Vec<u32> = markers
            .iter()
            .flat_map(|marker| marker.after.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(policy.checkpoint_after, vec![3, 4, 5]);
        assert_eq!(policy.checkpoint_after, from_markers);
    }

    /// A bullet's clause goes back on its own bullet and prose belonging to no
    /// bullet stays a paragraph. Rendering the clause as trailing prose reads
    /// as an orphan, and attaching the paragraph to a bullet would state
    /// something the author did not.
    #[test]
    fn a_bullet_clause_renders_on_its_bullet_and_free_prose_stays_a_paragraph() {
        let mut store = fixture();
        store.policy.checkpoints_note = "— one commit train per milestone group".to_string();
        let plan = render_into_plan(&store, PLAN).expect("renders");
        let policy = section(&plan, "Execution Policy");

        assert!(
            policy
                .contains("- **Checkpoints**: milestones — one commit train per milestone group\n"),
            "{policy}"
        );
        assert!(
            policy.contains(
                "- **Commit granularity**: per-task — tasks 2 and 3 land in one commit\n"
            ),
            "{policy}"
        );
        assert!(
            policy.contains("\nThe cuts are reviewed by hand while the store is young.\n"),
            "{policy}"
        );

        let parsed = parse_policy(Some(&policy)).expect("policy parses");
        assert_eq!(parsed.checkpoints_note, store.policy.checkpoints_note);
        assert_eq!(
            parsed.commit_granularity_note,
            store.policy.commit_granularity_note
        );
        assert_eq!(parsed.max_parallel_note, "");
        assert_eq!(parsed.note, store.policy.note);
    }

    /// The annotation rides the path it was written against, and the row's
    /// `files` stay the bare paths every file-claim comparison reads.
    #[test]
    fn a_file_annotation_renders_against_its_own_path() {
        let mut store = fixture();
        store.items[2].files = vec![
            "tomlctl/src/tasks/t3.rs".to_string(),
            "tomlctl/src/tasks/mod.rs".to_string(),
        ];
        let plan = render_into_plan(&store, PLAN).expect("renders");
        assert!(
            plan.contains(
                "- **Files**: `tomlctl/src/tasks/t3.rs` (new), `tomlctl/src/tasks/mod.rs`\n"
            ),
            "{plan}"
        );

        let parsed = parse_tasks_at(&section(&plan, "Tasks"), 1).expect("tasks parse");
        assert_eq!(parsed[2].files, store.items[2].files);
        assert_eq!(
            parsed[2].file_notes,
            vec!["(new)".to_string(), String::new()]
        );
        assert_eq!(render_into_plan(&store, &plan).expect("re-renders"), plan);
    }

    #[test]
    fn an_invalid_cut_is_marked_once_and_never_accumulates() {
        let store = fixture();
        let plan = render_into_plan(&store, PLAN).expect("renders");

        assert_eq!(plan.matches(INVALID_CUT).count(), 1, "{plan}");
        assert!(
            plan.contains(&format!(
                "CHECKPOINT B after tasks 4 {EMPTY} {CLOSURE_LABEL}: 1, 4, 5. the importer {INVALID_CUT}"
            )),
            "{plan}"
        );
        assert_eq!(markers(&plan)[1].rationale, "the importer");
        assert_eq!(render_into_plan(&store, &plan).expect("re-renders"), plan);
    }

    /// One heading per phase run at the authored depths, and a depth that would
    /// otherwise open a `## ` section clamped back inside the one being written.
    #[test]
    fn phase_headings_survive_and_no_depth_can_split_the_section() {
        let mut store = fixture();
        for (row, (phase, phase_depth, heading_depth)) in store.items.iter_mut().zip([
            ("Milestone A — the store", 3, 4),
            ("Milestone A — the store", 3, 4),
            ("Milestone A — the store", 3, 4),
            ("Milestone B — the verbs", 2, 3),
            ("Milestone B — the verbs", 2, 3),
            ("", 0, 9),
        ]) {
            row.phase = phase.to_string();
            row.phase_depth = phase_depth;
            row.heading_depth = heading_depth;
        }

        let plan = render_into_plan(&store, PLAN).expect("renders");
        let tasks = section(&plan, "Tasks");
        for wanted in [
            "\n### Milestone A — the store\n",
            "\n### Milestone B — the verbs\n",
            "\n#### 1. Scaffold the store [S]\n",
            "\n### 4. Import the plan [S]\n",
            "\n###### 6. Smoke the corpus [S]\n",
        ] {
            assert_eq!(tasks.matches(wanted).count(), 1, "{wanted:?} in {tasks}");
        }

        assert_eq!(
            sections(&plan)
                .into_iter()
                .map(|section| section.title)
                .collect::<Vec<_>>(),
            vec![
                "Approach",
                "Execution Policy",
                "Tasks",
                "Dependency Graph",
                "Risks"
            ],
            "a heading depth escaped the section it was written into"
        );
        assert_eq!(render_into_plan(&store, &plan).expect("re-renders"), plan);
    }

    #[test]
    fn drift_is_none_on_a_fresh_render_and_some_after_one_byte_changes() {
        let store = fixture();
        let plan = render_into_plan(&store, PLAN).expect("renders");
        assert_eq!(check_render_drift(&store, &plan).expect("checks"), None);

        let crlf = plan.replace('\n', "\r\n");
        assert!(crlf.contains("\r\n"), "the CRLF case is vacuous");
        assert_eq!(
            crlf.matches('\n').count(),
            crlf.matches("\r\n").count(),
            "the CRLF fixture has bare LFs"
        );
        assert_eq!(check_render_drift(&store, &crlf).expect("checks"), None);

        let edited = plan.replacen(
            "### 1. Scaffold the store [S]",
            "### 1. Scaffold the store [M]",
            1,
        );
        assert_ne!(edited, plan);
        let finding = check_render_drift(&store, &edited)
            .expect("checks")
            .expect("one byte of the Tasks body drifted");
        assert_eq!(finding.class, "render/drift");
        assert_eq!(finding.severity, "warning");
        assert!(finding.ids.is_empty());
        assert!(finding.detail.contains("Tasks"), "{}", finding.detail);
        assert!(
            !finding.detail.contains("Dependency Graph"),
            "{}",
            finding.detail
        );
    }

    #[test]
    fn a_crlf_plan_keeps_its_endings() {
        let crlf = PLAN.replace('\n', "\r\n");
        let out = render_into_plan(&fixture(), &crlf).expect("renders");
        assert_eq!(
            out.matches('\n').count(),
            out.matches("\r\n").count(),
            "render mixed the line endings"
        );
    }

    #[test]
    fn missing_sections_are_inserted_in_canonical_order() {
        let src = "# Plan: Demo\n\n## Approach\n\nprose\n\n## Tasks\n\nstale\n\n## Risks\n\nrisk\n";
        let out = render_into_plan(&fixture(), src).expect("renders");

        assert_eq!(
            sections(&out)
                .into_iter()
                .map(|section| section.title)
                .collect::<Vec<_>>(),
            vec![
                "Approach",
                "Execution Policy",
                "Tasks",
                "Dependency Graph",
                "Risks"
            ]
        );
        assert!(out.starts_with("# Plan: Demo\n\n## Approach\n\nprose\n"));
        assert!(out.ends_with("## Risks\n\nrisk\n"));
    }

    #[test]
    fn canonical_order_holds_when_tasks_opens_the_document() {
        let src = "## Tasks\n\nstale\n\n## Risks\n\nrisk\n";
        let out = render_into_plan(&fixture(), src).expect("renders");

        assert_eq!(
            sections(&out)
                .into_iter()
                .map(|section| section.title)
                .collect::<Vec<_>>(),
            vec!["Execution Policy", "Tasks", "Dependency Graph", "Risks"]
        );
        assert!(out.starts_with("## Execution Policy\n"), "{out}");
        assert!(out.ends_with("## Risks\n\nrisk\n"), "{out}");
    }

    /// Every owned section is present, so the insert path cannot fire: only
    /// the correction path can put them back in canonical order, and the
    /// unowned section standing between them must not travel with them.
    #[test]
    fn an_owned_section_present_in_the_wrong_place_is_moved_into_canonical_order() {
        let src = "# Plan: Demo\n\n## Tasks\n\nstale\n\n## Notes\n\nkeep\n\n\
            ## Execution Policy\n\nstale\n\n## Dependency Graph\n\nstale\n\n\
            ## Risks\n\nrisk\n";
        let out = render_into_plan(&fixture(), src).expect("renders");

        assert_eq!(
            sections(&out)
                .into_iter()
                .map(|section| section.title)
                .collect::<Vec<_>>(),
            vec![
                "Execution Policy",
                "Notes",
                "Tasks",
                "Dependency Graph",
                "Risks"
            ]
        );
        assert!(
            out.starts_with("# Plan: Demo\n\n## Execution Policy\n"),
            "{out}"
        );
        assert!(out.contains("## Notes\n\nkeep\n"), "{out}");
        assert!(out.ends_with("## Risks\n\nrisk\n"), "{out}");
        assert!(
            !out.contains("stale"),
            "a body left under a moved heading survived: {out}"
        );
        assert_eq!(render_into_plan(&fixture(), &out).expect("re-renders"), out);
    }

    /// A permutation of a current render leaves every section body identical,
    /// which is the shape a body-only comparison reports as no drift at all.
    #[test]
    fn a_permuted_plan_reports_drift_and_renders_back_into_order() {
        let store = fixture();
        let plan = render_into_plan(&store, PLAN).expect("renders");

        let tasks_at = plan.find("## Tasks").expect("the Tasks heading");
        let graph_at = plan
            .find("## Dependency Graph")
            .expect("the Dependency Graph heading");
        let risks_at = plan.find("## Risks").expect("the Risks heading");
        let permuted = format!(
            "{}{}{}{}",
            &plan[..tasks_at],
            &plan[graph_at..risks_at],
            &plan[tasks_at..graph_at],
            &plan[risks_at..]
        );
        assert_ne!(permuted, plan, "the permutation moved nothing");

        let finding = check_render_drift(&store, &permuted)
            .expect("checks")
            .expect("a permuted plan is drift");
        assert!(
            finding
                .detail
                .contains("out of canonical order: Execution Policy, Dependency Graph, Tasks"),
            "{}",
            finding.detail
        );
        assert!(
            !finding.detail.contains("out of date with the store"),
            "the bodies are the store's own render: {}",
            finding.detail
        );

        assert_eq!(render_into_plan(&store, &permuted).expect("renders"), plan);

        // The correction rewrites heading lines, which is a second place a
        // document's ending can be dropped on the floor.
        let crlf = render_into_plan(&store, &permuted.replace('\n', "\r\n")).expect("renders");
        assert_eq!(
            crlf.matches('\n').count(),
            crlf.matches("\r\n").count(),
            "the correction mixed the line endings"
        );
        assert_eq!(lf(&crlf), plan);
    }

    #[test]
    fn a_cycle_refuses_rather_than_rendering_a_wrong_cut() {
        let mut store = fixture();
        store.items[0].needs = vec![6];
        let err = render_sections(&store).expect_err("a cycle has no closure");
        assert!(err.to_string().contains("cycle"), "{err}");
    }
}
