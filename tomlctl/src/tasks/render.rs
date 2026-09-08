//! Rendering the store back into a plan's policy, tasks and dependency-graph sections.
//!
//! Every id list a section carries — the `Checkpoint after` bullet, a marker's
//! `after` and its `closure` — is derived from the graph on each render and
//! never stored, so a stale list cannot outlive the edges it summarises.
//!
//! Bodies re-indent two spaces under their label line, which is the
//! continuation form the importer reads back. `(INVALID CUT)` is rendered
//! rather than suppressed so the defect is visible in the document; the
//! importer strips it, so it cannot accumulate across a render→import→render
//! cycle.

use std::collections::BTreeSet;

use anyhow::Result;

use super::graph::{Graph, Group, Node};
use super::markdown::{insert_section_after, replace_section, sections};
use super::schema::{Checkpoint, Policy, Store, TaskRow};

/// The sections `render` owns; every other byte of the plan is preserved.
pub(crate) const SECTION_TITLES: [&str; 3] = ["Execution Policy", "Tasks", "Dependency Graph"];

const PREAMBLE: &str =
    "Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.";

const INVALID_CUT: &str = "(INVALID CUT)";

const EMPTY: &str = "—";

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
    let nodes = nodes(store);
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
        tasks: render_tasks(&store.items),
        graph: render_graph(&store.checkpoints, &groups, &closures),
    })
}

pub(crate) fn render_into_plan(store: &Store, plan_src: &str) -> Result<String> {
    let rendered = render_sections(store)?;
    let before_tasks = section_before(plan_src, "Tasks").unwrap_or_default();

    let mut out = replace_or_insert(
        plan_src,
        "Execution Policy",
        &before_tasks,
        &rendered.policy,
    );
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
    let detail = if drifted.is_empty() {
        "the plan differs from the store's render".to_string()
    } else {
        format!(
            "plan sections out of date with the store: {}",
            drifted.join(", ")
        )
    };

    Ok(Some(Finding {
        class: "render/drift",
        severity: "warning",
        ids: Vec::new(),
        detail,
    }))
}

fn nodes(store: &Store) -> Vec<Node> {
    store
        .items
        .iter()
        .map(|row| Node {
            id: row.id,
            files: row.files.clone(),
            needs: row.needs.clone(),
            coupling: row.coupling.clone(),
            status: row.status.as_str().to_string(),
            checkpoint: row.checkpoint.clone(),
        })
        .collect()
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
    body.push_str(&format!("- **Checkpoints**: {}\n", policy.checkpoints));
    body.push_str(&format!(
        "- **Checkpoint after**: {}\n",
        if after.is_empty() {
            EMPTY.to_string()
        } else {
            format!("tasks {}", join_ids(&after))
        }
    ));
    body.push_str(&format!(
        "- **Max parallel agents**: {}\n",
        policy.max_parallel
    ));
    body.push_str(&format!(
        "- **Commit granularity**: {}\n",
        policy.commit_granularity
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

/// Store order, not id order: it is the plan's own task order, and sorting
/// would reshuffle a document whose numbering runs across phases.
fn render_tasks(rows: &[TaskRow]) -> String {
    let mut body = String::from("\n");
    for row in rows {
        body.push_str(&format!(
            "### {}. {} [{}]\n",
            row.id,
            row.title,
            row.effort.as_str()
        ));
        body.push_str(&format!("- **Files**: {}\n", file_list(&row.files)));
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
            "{EMPTY} CHECKPOINT {} after tasks {} {EMPTY} closure: {}.",
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

fn file_list(files: &[String]) -> String {
    if files.is_empty() {
        return EMPTY.to_string();
    }
    files
        .iter()
        .map(|file| format!("`{file}`"))
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
    if sections(src).iter().any(|section| section.title == title) {
        replace_section(src, title, body)
    } else {
        insert_section_after(src, after, title, body)
    }
}

/// The anchor an absent section inserts after. `None` when `title` opens the
/// document or is absent, which leaves `insert_section_after` appending.
fn section_before(src: &str, title: &str) -> Option<String> {
    let found = sections(src);
    let at = found.iter().position(|section| section.title == title)?;
    (at > 0).then(|| found[at - 1].title.clone())
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
    use crate::tasks::parse_tasks::parse_tasks;
    use crate::tasks::schema::{Effort, Status};
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
                note: "tasks 2 and 3 land in one commit".to_string(),
            },
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

        let tasks = parse_tasks(&section(&plan, "Tasks")).expect("tasks parse");
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
                "CHECKPOINT A after tasks 3 {EMPTY} closure: 1, 2, 3."
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

    #[test]
    fn an_invalid_cut_is_marked_once_and_never_accumulates() {
        let store = fixture();
        let plan = render_into_plan(&store, PLAN).expect("renders");

        assert_eq!(plan.matches(INVALID_CUT).count(), 1, "{plan}");
        assert!(
            plan.contains(&format!(
                "CHECKPOINT B after tasks 4 {EMPTY} closure: 1, 4, 5. the importer {INVALID_CUT}"
            )),
            "{plan}"
        );
        assert_eq!(markers(&plan)[1].rationale, "the importer");
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
    fn a_cycle_refuses_rather_than_rendering_a_wrong_cut() {
        let mut store = fixture();
        store.items[0].needs = vec![6];
        let err = render_sections(&store).expect_err("a cycle has no closure");
        assert!(err.to_string().contains("cycle"), "{err}");
    }
}
