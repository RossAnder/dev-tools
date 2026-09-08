//! The `list` read verb, over the shared query surface.
//!
//! The store's array is named `items`, so every `items list` predicate,
//! projection, aggregation and output encoding applies to task rows unchanged
//! and this module adds no query semantics of its own. `--count` is wired here
//! because it lives on the CLI variant rather than in `QueryArgs`.

use anyhow::Result;

use super::store;
use crate::cli::{LegacyShortcuts, QueryArgs, ReadIntegrityArgs, TasksTarget, read_integrity_opts};
use crate::io::read_doc;
use crate::output::{emit_list_raw, print_json};
use crate::query::{self, Query, ShapeDispatch};

/// The array the store keeps its rows in, and the reason the whole `items`
/// query machinery applies to them.
const ARRAY_ITEMS: &str = "items";

pub(crate) fn dispatch(
    target: TasksTarget,
    count: bool,
    query_args: QueryArgs,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
    let opts = read_integrity_opts(&integrity);
    let q = build_query(count, &query_args)?;

    if q.ndjson && q.shape.is_streamable() {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        read_doc(&path, opts, |doc| {
            query::run_streaming(doc, ARRAY_ITEMS, &q, &mut h)
        })?;
        h.flush()?;
        return Ok(());
    }

    let out = read_doc(&path, opts, |doc| query::run(doc, ARRAY_ITEMS, &q))?;
    if q.raw {
        emit_list_raw(&out, &q.shape)
    } else {
        print_json(&out)
    }
}

/// The store carries none of the legacy shortcut fields, so `--count` is the
/// only slot filled here; every other predicate arrives through `QueryArgs`.
fn build_query(count: bool, query_args: &QueryArgs) -> Result<Query> {
    let unset: Option<String> = None;
    let legacy = LegacyShortcuts {
        status: &unset,
        category: &unset,
        file: &unset,
        newer_than: &unset,
        count,
    };
    Query::from_query_input(&query_args.to_query_input(&legacy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{self, Effort, Status, Store, TaskRow};
    use clap::Parser;
    use serde_json::Value as JsonValue;
    use toml::Value as TomlValue;

    /// `QueryArgs` is a flattened `Args` bundle, not a `Parser`, so a throwaway
    /// wrapper gives the tests the real clap surface.
    #[derive(Parser)]
    struct QueryHarness {
        #[command(flatten)]
        q: QueryArgs,
    }

    fn query_args(args: &[&str]) -> QueryArgs {
        let mut argv = vec!["harness"];
        argv.extend_from_slice(args);
        QueryHarness::try_parse_from(argv).unwrap().q
    }

    fn row(id: u32, status: Status) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status,
            checkpoint: "A".to_string(),
            files: vec![format!("tomlctl/src/tasks/t{id}.rs")],
            needs: Vec::new(),
            coupling: Vec::new(),
            deps_note: String::new(),
            action: String::new(),
            detail: String::new(),
            acceptance: String::new(),
            agent: String::new(),
            commit: String::new(),
        }
    }

    /// Interleaved statuses, so a filtered pluck cannot accidentally agree with
    /// the unfiltered row order.
    fn doc() -> TomlValue {
        schema::to_toml(&Store {
            items: vec![
                row(1, Status::Done),
                row(2, Status::Pending),
                row(3, Status::InProgress),
                row(4, Status::Pending),
                row(5, Status::Pending),
            ],
            ..Store::default()
        })
    }

    fn run(args: &[&str], count: bool) -> JsonValue {
        let q = build_query(count, &query_args(args)).expect("the query builds");
        query::run(&doc(), ARRAY_ITEMS, &q).expect("the query runs")
    }

    #[test]
    fn a_filtered_pluck_returns_the_matching_ids_in_id_order() {
        let out = run(&["--where", "status=pending", "--pluck", "id"], false);
        assert_eq!(out, serde_json::json!([2, 4, 5]), "{out}");
    }

    #[test]
    fn count_returns_the_row_total() {
        let out = run(&[], true);
        assert_eq!(out, serde_json::json!({ "count": 5 }), "{out}");
    }

    #[test]
    fn the_whole_predicate_surface_reaches_task_rows() {
        let out = run(&["--where-in", "id=2,3", "--select", "id,status"], false);
        assert_eq!(
            out,
            serde_json::json!([
                { "id": 2, "status": "pending" },
                { "id": 3, "status": "in-progress" },
            ]),
            "{out}"
        );
    }
}
