//! The `edges` read verb and its Graphviz emitter.
//!
//! `from` is the prerequisite and `to` the row that waits on it, so a JSON
//! edge and its DOT arrow read the same way round. `needs` and `coupling` come
//! straight off the rows; `overlap` is computed — two rows sharing a file with
//! no directed path either way — and so is the one kind that needs an acyclic
//! graph to answer.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::graph::{Graph, Node};
use super::schema::Store;
use super::store;
use crate::cli::{EdgeKind, ReadIntegrityArgs, TasksTarget};
use crate::output::print_json;

/// One kind's label and its pairs, ordered as the output emits them.
type EdgeGroup = (&'static str, Vec<(u32, u32)>);

const KIND_NEEDS: &str = "needs";
const KIND_COUPLING: &str = "coupling";
const KIND_OVERLAP: &str = "overlap";

pub(crate) fn dispatch(
    target: TasksTarget,
    kind: Option<EdgeKind>,
    dot: bool,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
    let store = store::load(&path, &integrity)?;
    if dot {
        print!("{}", dot_source(&store, kind)?);
        use std::io::Write;
        std::io::stdout().flush().ok();
        return Ok(());
    }
    print_json(&edge_list(&store, kind)?)
}

/// Edges as `{kind, from, to}`, grouped by kind in declaration order and
/// ascending within each.
pub(crate) fn edge_list(store: &Store, kind: Option<EdgeKind>) -> Result<JsonValue> {
    let mut out = Vec::new();
    for (label, pairs) in collect(store, kind)? {
        out.extend(
            pairs
                .into_iter()
                .map(|(from, to)| json!({ "kind": label, "from": from, "to": to })),
        );
    }
    Ok(JsonValue::Array(out))
}

/// One node per task whatever `--kind` selects, so a filtered graph still
/// renders every task.
pub(crate) fn dot_source(store: &Store, kind: Option<EdgeKind>) -> Result<String> {
    let mut out = String::from("digraph tasks {\n");
    for row in &store.items {
        let _ = writeln!(
            out,
            "  {} [label=\"{}: {}\"];",
            row.id,
            row.id,
            escape(&row.title)
        );
    }
    for (label, pairs) in collect(store, kind)? {
        let attrs = match label {
            KIND_COUPLING => " [style=dashed]",
            KIND_OVERLAP => " [style=dotted, dir=none]",
            _ => "",
        };
        for (from, to) in pairs {
            let _ = writeln!(out, "  {from} -> {to}{attrs};");
        }
    }
    out.push_str("}\n");
    Ok(out)
}

/// The requested kinds in a fixed order. `overlap` builds the graph; the two
/// stored kinds do not, so a store whose edges do not yet form a DAG can still
/// list them.
fn collect(store: &Store, kind: Option<EdgeKind>) -> Result<Vec<EdgeGroup>> {
    let mut out = Vec::new();
    if matches!(kind, None | Some(EdgeKind::Needs)) {
        out.push((KIND_NEEDS, stored(store, |row| &row.needs)));
    }
    if matches!(kind, None | Some(EdgeKind::Coupling)) {
        out.push((KIND_COUPLING, stored(store, |row| &row.coupling)));
    }
    if matches!(kind, None | Some(EdgeKind::Overlap)) {
        let nodes = nodes(store);
        out.push((KIND_OVERLAP, Graph::build(&nodes)?.overlap_pairs()?));
    }
    Ok(out)
}

fn stored(store: &Store, field: impl Fn(&super::schema::TaskRow) -> &Vec<u32>) -> Vec<(u32, u32)> {
    let mut pairs: BTreeSet<(u32, u32)> = BTreeSet::new();
    for row in &store.items {
        for target in field(row) {
            pairs.insert((*target, row.id));
        }
    }
    pairs.into_iter().collect()
}

/// A title reaches here from a plan heading, so it can carry a quote or a
/// backslash that would otherwise end the DOT label early.
fn escape(title: &str) -> String {
    title.replace('\\', "\\\\").replace('"', "\\\"")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{Effort, Status, TaskRow};

    fn row(id: u32, title: &str, needs: &[u32], coupling: &[u32], files: &[&str]) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: title.to_string(),
            effort: Effort::S,
            status: Status::Pending,
            checkpoint: String::new(),
            files: files.iter().map(|f| (*f).to_string()).collect(),
            needs: needs.to_vec(),
            coupling: coupling.to_vec(),
            deps_note: String::new(),
            action: String::new(),
            detail: String::new(),
            acceptance: String::new(),
            agent: String::new(),
            commit: String::new(),
        }
    }

    /// Tasks 2 and 3 share `shared.rs` with no path between them — the one
    /// overlap. Tasks 1 and 2 share `chain.rs` too, but 2 needs 1, so a "any
    /// shared file" answer would report that pair and a correct one does not.
    fn fixture() -> Store {
        Store {
            items: vec![
                row(1, "Scaffold the store", &[], &[], &["chain.rs"]),
                row(2, "Build the engine", &[1], &[], &["chain.rs", "shared.rs"]),
                row(3, "Render the \"sections\"", &[], &[1], &["shared.rs"]),
            ],
            ..Store::default()
        }
    }

    #[test]
    fn overlap_is_a_shared_file_with_no_path_either_way() {
        let out = edge_list(&fixture(), Some(EdgeKind::Overlap)).expect("edges list");
        assert_eq!(
            out,
            serde_json::json!([{ "kind": "overlap", "from": 2, "to": 3 }]),
            "{out}"
        );
    }

    #[test]
    fn stored_edges_run_from_the_prerequisite_to_the_waiting_row() {
        let out = edge_list(&fixture(), None).expect("edges list");
        assert_eq!(
            out,
            serde_json::json!([
                { "kind": "needs", "from": 1, "to": 2 },
                { "kind": "coupling", "from": 1, "to": 3 },
                { "kind": "overlap", "from": 2, "to": 3 },
            ]),
            "{out}"
        );
    }

    #[test]
    fn dot_draws_every_task_and_styles_each_kind() {
        let dot = dot_source(&fixture(), None).expect("dot renders");
        assert!(dot.starts_with("digraph tasks {\n"), "{dot}");
        assert!(
            dot.contains(r#"  3 [label="3: Render the \"sections\""];"#),
            "a quoted title must not end the label early: {dot}"
        );
        assert!(dot.contains("  1 -> 2;\n"), "{dot}");
        assert!(dot.contains("  1 -> 3 [style=dashed];\n"), "{dot}");
        assert!(
            dot.contains("  2 -> 3 [style=dotted, dir=none];\n"),
            "{dot}"
        );
        assert!(dot.ends_with("}\n"), "{dot}");
    }

    #[test]
    fn a_kind_filter_drops_the_other_two() {
        let dot = dot_source(&fixture(), Some(EdgeKind::Needs)).expect("dot renders");
        assert!(dot.contains("  1 -> 2;\n"), "{dot}");
        assert!(!dot.contains("style="), "{dot}");
        assert!(
            dot.contains("  3 [label="),
            "every task keeps its node: {dot}"
        );
    }
}
