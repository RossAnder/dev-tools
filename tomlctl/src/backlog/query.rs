//! `backlog list` and `backlog show` — the read paths over the store.
//!
//! `list` narrows the `backlog` array by the three filters this module owns,
//! then hands the narrowed array to the generic query engine, so the whole
//! `--where-*` / projection / aggregation surface still applies. Those three
//! are here because no predicate family expresses them: `--tag` tests array
//! membership, `--area-prefix` matches on path-component boundaries (which
//! `--where-prefix` does not), and `--has-evidence` reads the filesystem.
//!
//! Evidence is derived from the directory on every call. The store records
//! nothing about it, so a listing cannot be stale — and an absent directory
//! stays distinguishable from an empty one.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::evidence;
use super::schema::{
    self, ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_AREA, FIELD_DUPLICATE_OF, FIELD_ID, FIELD_KIND,
    FIELD_RELATED, FIELD_STATUS, FIELD_SUMMARY, FIELD_SUPERSEDES, FIELD_TAGS, STATUS_OPEN,
};
use crate::cli::{
    LegacyShortcuts, QueryArgs, ReadIntegrityArgs, query_input_from_cli, read_integrity_opts,
};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::{IntegrityOpts, maybe_verify_integrity};
use crate::io::{items_array, read_toml, relativise, repo_or_cwd_root};
use crate::output::{emit_list_raw, print_json};
use crate::query::{Query, ShapeDispatch};

const DIRECTION_OUT: &str = "out";
const DIRECTION_IN: &str = "in";

/// The typed edge fields `show` walks, in output-tie-break order.
/// `related` holds an array; the other two hold a single id.
const RELATIONS: &[&str] = &[FIELD_RELATED, FIELD_DUPLICATE_OF, FIELD_SUPERSEDES];

// One parameter per `BacklogOp::List` field, so the dispatch fan-out stays a
// mechanical destructure.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_list(
    status: Option<String>,
    kind: Option<String>,
    tag: Vec<String>,
    open: bool,
    area_prefix: Option<String>,
    has_evidence: bool,
    count: bool,
    query: QueryArgs,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let file = schema::backlog_path()?;
    strict_read_gate(&file, integrity.strict_read)?;
    let doc = read_store(&file, read_integrity_opts(&integrity))?;

    let filters = Filters {
        tags: &tag,
        area_prefix: area_prefix.as_deref(),
        has_evidence,
    };
    let narrowed = narrowed_doc(filter_backlog(
        items_array(&doc, ARRAY_BACKLOG),
        &filters,
    )?);

    let q = build_query(&status, &kind, open, count, &query)?;
    if q.ndjson && q.shape.is_streamable() {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        crate::query::run_streaming(&narrowed, ARRAY_BACKLOG, &q, &mut h)?;
        h.flush()?;
    } else {
        let out = crate::query::run(&narrowed, ARRAY_BACKLOG, &q)?;
        if q.raw {
            emit_list_raw(&out, &q.shape)?;
        } else {
            print_json(&out)?;
        }
    }
    Ok(())
}

pub(crate) fn dispatch_show(id: String, integrity: ReadIntegrityArgs) -> Result<()> {
    let file = schema::backlog_path()?;
    strict_read_gate(&file, integrity.strict_read)?;
    let doc = read_store(&file, read_integrity_opts(&integrity))?;
    print_json(&build_show(&doc, &id)?)
}

/// Filters applied before the query engine sees the array.
struct Filters<'a> {
    tags: &'a [String],
    area_prefix: Option<&'a str>,
    has_evidence: bool,
}

fn filter_backlog(items: &[TomlValue], f: &Filters<'_>) -> Result<Vec<TomlValue>> {
    let mut kept = Vec::new();
    for item in items {
        if !f.tags.iter().all(|t| has_tag(item, t)) {
            continue;
        }
        if let Some(prefix) = f.area_prefix
            && !area_matches(row_str(item, FIELD_AREA).unwrap_or_default(), prefix)
        {
            continue;
        }
        if f.has_evidence
            && !evidence_files(row_str(item, FIELD_ID).unwrap_or_default())?
                .is_some_and(|files| !files.is_empty())
        {
            continue;
        }
        kept.push(item.clone());
    }
    Ok(kept)
}

/// `lumina/server` selects `lumina/server/pty/x.rs` and `lumina/server`
/// itself, never `lumina/server-extras/y.rs`. An empty prefix selects
/// everything.
fn area_matches(area: &str, prefix: &str) -> bool {
    let prefix = normalise_path(prefix);
    if prefix.is_empty() {
        return true;
    }
    let area = normalise_path(area);
    area == prefix
        || area
            .strip_prefix(&prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn normalise_path(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_string()
}

fn has_tag(item: &TomlValue, tag: &str) -> bool {
    item.get(FIELD_TAGS)
        .and_then(TomlValue::as_array)
        .is_some_and(|tags| tags.iter().any(|t| t.as_str() == Some(tag)))
}

/// `--status` and `--open` both land on the legacy `status` slot's equality
/// predicate; `--kind` rides the same `--where` family. Passing `--open`
/// alongside a contradictory `--status` ANDs the two, as every other filter
/// pair does, and yields nothing.
fn build_query(
    status: &Option<String>,
    kind: &Option<String>,
    open: bool,
    count: bool,
    query: &QueryArgs,
) -> Result<Query> {
    let unset: Option<String> = None;
    let legacy = LegacyShortcuts {
        status,
        category: &unset,
        file: &unset,
        newer_than: &unset,
        count,
    };
    let mut input = query_input_from_cli(&legacy, query);
    if open {
        input.where_eq.push(format!("{FIELD_STATUS}={STATUS_OPEN}"));
    }
    if let Some(k) = kind {
        input.where_eq.push(format!("{FIELD_KIND}={k}"));
    }
    Query::from_query_input(&input)
}

fn narrowed_doc(items: Vec<TomlValue>) -> TomlValue {
    let mut table = toml::map::Map::new();
    table.insert(ARRAY_BACKLOG.to_string(), TomlValue::Array(items));
    TomlValue::Table(table)
}

fn build_show(doc: &TomlValue, id: &str) -> Result<JsonValue> {
    let resolved = evidence::resolve_id(doc, id)?;
    let Some(row) = stored_rows(doc).find(|r| row_str(r, FIELD_ID) == Some(resolved.as_str()))
    else {
        return Err(tagged_err(
            ErrorKind::NotFound,
            schema::backlog_path().ok(),
            format!("no backlog item with id \"{resolved}\""),
        ));
    };

    let mut edges: BTreeSet<(String, &'static str, &'static str)> = BTreeSet::new();
    for &relation in RELATIONS {
        for peer in relation_targets(row, relation) {
            if peer != resolved {
                edges.insert((peer, relation, DIRECTION_OUT));
            }
        }
    }
    for other in stored_rows(doc) {
        let Some(other_id) = row_str(other, FIELD_ID) else {
            continue;
        };
        if other_id == resolved {
            continue;
        }
        for &relation in RELATIONS {
            if relation_targets(other, relation).contains(&resolved) {
                edges.insert((other_id.to_string(), relation, DIRECTION_IN));
            }
        }
    }

    let mut neighbours = Vec::with_capacity(edges.len());
    for (peer_id, relation, direction) in edges {
        let peer = stored_rows(doc).find(|r| row_str(r, FIELD_ID) == Some(peer_id.as_str()));
        neighbours.push(json!({
            "id": peer_id,
            "relation": relation,
            "direction": direction,
            "summary": optional_str(peer.and_then(|p| row_str(p, FIELD_SUMMARY))),
            "status": optional_str(peer.and_then(|p| row_str(p, FIELD_STATUS))),
            "evidence": evidence_shape(&peer_id)?,
        }));
    }

    Ok(json!({
        "item": toml_to_json(row),
        "evidence": evidence_shape(&resolved)?,
        "neighbours": neighbours,
    }))
}

/// `null` when the directory is absent, `files: []` when only the marker is
/// there, one entry per file otherwise.
fn evidence_shape(id: &str) -> Result<JsonValue> {
    let Some(files) = evidence_files(id)? else {
        return Ok(JsonValue::Null);
    };
    let dir = evidence::dir_for(id)?;
    Ok(json!({
        "dir": relativise(&repo_or_cwd_root()?, &dir),
        "files": files
            .into_iter()
            .map(|(name, bytes)| json!({ "name": name, "bytes": bytes }))
            .collect::<Vec<_>>(),
    }))
}

fn evidence_files(id: &str) -> Result<Option<Vec<(String, u64)>>> {
    // An id that is not a single path component can own no directory, so it
    // reads as absent rather than as an error — one hand-edited row must not
    // fail the whole listing.
    let Ok(dir) = evidence::dir_for(id) else {
        return Ok(None);
    };
    evidence::list_dir(&dir)
}

fn relation_targets(row: &TomlValue, relation: &str) -> Vec<String> {
    if relation == FIELD_RELATED {
        return row
            .get(FIELD_RELATED)
            .and_then(TomlValue::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(TomlValue::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
    }
    row_str(row, relation)
        .filter(|s| !s.is_empty())
        .map(|s| vec![s.to_string()])
        .unwrap_or_default()
}

fn stored_rows(doc: &TomlValue) -> impl Iterator<Item = &TomlValue> {
    items_array(doc, ARRAY_BACKLOG)
        .iter()
        .chain(items_array(doc, ARRAY_COMPACTED).iter())
}

fn row_str<'a>(row: &'a TomlValue, field: &str) -> Option<&'a str> {
    row.get(field).and_then(TomlValue::as_str)
}

fn optional_str(s: Option<&str>) -> JsonValue {
    s.map_or(JsonValue::Null, |s| JsonValue::String(s.to_string()))
}

/// A missing store lists as empty. `--strict-read` is what turns that into
/// `kind=not_found`; `show` errors either way, through `resolve_id`.
fn read_store(file: &Path, integrity: IntegrityOpts) -> Result<TomlValue> {
    if !file.exists() {
        return Ok(TomlValue::Table(toml::map::Map::new()));
    }
    maybe_verify_integrity(file, integrity)?;
    read_toml(file)
}

fn strict_read_gate(file: &Path, strict_read: bool) -> Result<()> {
    if !strict_read || file.exists() {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::NotFound,
        Some(file.to_path_buf()),
        format!("file does not exist: {}", file.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::fs;
    use std::path::PathBuf;

    /// `QueryArgs` is a flattened `Args` bundle, not a `Parser`, so a
    /// throwaway wrapper gives the tests the real clap surface instead of a
    /// 26-field hand-built literal.
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

    fn doc(s: &str) -> TomlValue {
        toml::from_str(s).unwrap()
    }

    /// Drop the override before any assertion runs — a panic inside would
    /// otherwise leak `TOMLCTL_ROOT` into every later test in the process.
    fn under_root<T>(f: impl FnOnce(&Path) -> T) -> T {
        let _guard = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // SAFETY: set_var is unsafe in edition 2024; acceptable inside tests
        // where we hold the env lock.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", root.as_os_str());
        }
        let out = f(&root);
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
        out
    }

    fn evidence_dir(root: &Path, id: &str) -> PathBuf {
        let dir = root.join(".claude").join("backlog-evidence").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(evidence::MARKER_NAME), "marker").unwrap();
        dir
    }

    fn ids(items: &[TomlValue]) -> Vec<&str> {
        items
            .iter()
            .filter_map(|i| row_str(i, FIELD_ID))
            .collect()
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    const AREAS: &str = r#"
[[backlog]]
id = "B-1"
summary = "under the server tree"
status = "open"
area = "lumina/server/pty/x.rs"

[[backlog]]
id = "B-2"
summary = "a different subtree"
status = "open"
area = "lumina/web/y.ts"

[[backlog]]
id = "B-3"
summary = "sibling sharing a textual prefix"
status = "open"
area = "lumina/server-extras/z.rs"

[[backlog]]
id = "B-4"
summary = "the prefix directory itself"
status = "open"
area = "lumina/server"

[[backlog]]
id = "B-5"
summary = "no area at all"
status = "open"
"#;

    #[test]
    fn area_prefix_matches_on_component_boundaries() {
        let d = doc(AREAS);
        let f = Filters {
            tags: &[],
            area_prefix: Some("lumina/server"),
            has_evidence: false,
        };
        let kept = filter_backlog(items_array(&d, ARRAY_BACKLOG), &f).unwrap();
        assert_eq!(ids(&kept), ["B-1", "B-4"]);
    }

    #[test]
    fn area_prefix_boundary_cases_are_pinned() {
        assert!(area_matches("lumina/server/pty/x.rs", "lumina/server"));
        assert!(area_matches("lumina/server", "lumina/server"));
        assert!(area_matches("lumina/server/pty/x.rs", "lumina/server/"));
        assert!(!area_matches("lumina/server-extras/z.rs", "lumina/server"));
        assert!(!area_matches("lumina/web/y.ts", "lumina/server"));
        assert!(!area_matches("", "lumina/server"));
        assert!(area_matches("lumina/web/y.ts", ""));
        // A Windows-authored `area` still resolves against a repo prefix.
        assert!(area_matches("lumina\\server\\pty\\x.rs", "lumina/server"));
    }

    const TAGGED: &str = r#"
[[backlog]]
id = "B-1"
summary = "carries both"
status = "open"
tags = ["ci", "windows", "flake"]

[[backlog]]
id = "B-2"
summary = "carries only one"
status = "open"
tags = ["ci"]

[[backlog]]
id = "B-3"
summary = "carries neither"
status = "open"
tags = ["macos"]

[[backlog]]
id = "B-4"
summary = "carries no tags field"
status = "open"
"#;

    #[test]
    fn repeated_tags_and_across_repeats() {
        let d = doc(TAGGED);
        let both = [String::from("ci"), String::from("windows")];
        let f = Filters {
            tags: &both,
            area_prefix: None,
            has_evidence: false,
        };
        let kept = filter_backlog(items_array(&d, ARRAY_BACKLOG), &f).unwrap();
        assert_eq!(ids(&kept), ["B-1"]);

        let one = [String::from("ci")];
        let f = Filters {
            tags: &one,
            area_prefix: None,
            has_evidence: false,
        };
        let kept = filter_backlog(items_array(&d, ARRAY_BACKLOG), &f).unwrap();
        assert_eq!(ids(&kept), ["B-1", "B-2"]);
    }

    const STATUSES: &str = r#"
[[backlog]]
id = "B-1"
summary = "still open"
status = "open"
kind = "flaky-test"

[[backlog]]
id = "B-2"
summary = "already dealt with"
status = "resolved"
kind = "flaky-test"
resolved = 2026-09-01
resolution = "fixed upstream"

[[backlog]]
id = "B-3"
summary = "open but another kind"
status = "open"
kind = "debt"
"#;

    fn run_ids(d: &TomlValue, q: &Query) -> Vec<String> {
        crate::query::run(d, ARRAY_BACKLOG, q)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v[FIELD_ID].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn open_shorthand_excludes_a_resolved_item() {
        let d = doc(STATUSES);
        let q = build_query(&None, &None, true, false, &query_args(&[])).unwrap();
        assert_eq!(run_ids(&d, &q), ["B-1", "B-3"]);
    }

    #[test]
    fn status_and_kind_reach_the_predicate_surface() {
        let d = doc(STATUSES);
        let q = build_query(&Some("resolved".into()), &None, false, false, &query_args(&[])).unwrap();
        assert_eq!(run_ids(&d, &q), ["B-2"]);

        let q = build_query(&None, &Some("flaky-test".into()), true, false, &query_args(&[])).unwrap();
        assert_eq!(run_ids(&d, &q), ["B-1"]);
    }

    #[test]
    fn the_generic_query_surface_still_applies() {
        let d = doc(STATUSES);
        let q = build_query(&None, &None, true, false, &query_args(&["--limit", "1"])).unwrap();
        assert_eq!(run_ids(&d, &q), ["B-1"]);

        let q = build_query(&None, &None, false, false, &query_args(&["--count-by", "status"])).unwrap();
        let out = crate::query::run(&d, ARRAY_BACKLOG, &q).unwrap();
        assert_eq!(out["open"], json!(2));
        assert_eq!(out["resolved"], json!(1));
    }

    #[test]
    fn count_reaches_the_aggregation_shape() {
        let d = doc(STATUSES);
        let q = build_query(&None, &None, true, true, &query_args(&[])).unwrap();
        assert_eq!(
            crate::query::run(&d, ARRAY_BACKLOG, &q).unwrap(),
            json!({ "count": 2 })
        );

        let q = build_query(&None, &None, true, false, &query_args(&[])).unwrap();
        assert!(
            crate::query::run(&d, ARRAY_BACKLOG, &q).unwrap().is_array(),
            "without --count the same filters still list rows"
        );
    }

    const RELATED: &str = r#"
[[backlog]]
id = "B-1"
summary = "the subject"
status = "open"
related = ["B-3"]

[[backlog]]
id = "B-2"
summary = "peer that points at the subject"
status = "open"
related = ["B-1"]

[[backlog]]
id = "B-3"
summary = "peer the subject points at"
status = "open"

[[backlog]]
id = "B-4"
summary = "peer that supersedes the subject"
status = "open"
supersedes = "B-1"

[[compacted]]
id = "B-5"
summary = "aged-out duplicate of the subject"
status = "dismissed"
duplicate_of = "B-1"
"#;

    fn neighbour_keys(out: &JsonValue) -> Vec<(String, String, String)> {
        out["neighbours"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                (
                    n["id"].as_str().unwrap().to_string(),
                    n["relation"].as_str().unwrap().to_string(),
                    n["direction"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn show_reports_both_edge_directions() {
        let out = under_root(|_| build_show(&doc(RELATED), "B-1").unwrap());
        assert_eq!(out["item"][FIELD_ID], json!("B-1"));
        assert_eq!(
            neighbour_keys(&out),
            [
                ("B-2".into(), "related".into(), "in".into()),
                ("B-3".into(), "related".into(), "out".into()),
                ("B-4".into(), "supersedes".into(), "in".into()),
                ("B-5".into(), "duplicate_of".into(), "in".into()),
            ]
        );
        let inbound = &out["neighbours"][0];
        assert_eq!(inbound["summary"], json!("peer that points at the subject"));
        assert_eq!(inbound["status"], json!("open"));
    }

    #[test]
    fn show_resolves_a_compacted_row_and_its_reverse_edge() {
        let out = under_root(|_| build_show(&doc(RELATED), "B-5").unwrap());
        assert_eq!(out["item"][FIELD_ID], json!("B-5"));
        assert_eq!(
            neighbour_keys(&out),
            [("B-1".into(), "duplicate_of".into(), "out".into())]
        );
    }

    #[test]
    fn show_errors_not_found_on_an_unknown_id() {
        let err = under_root(|_| build_show(&doc(RELATED), "B-deadbeef").unwrap_err());
        assert_eq!(kind_of(&err), "not_found");
        assert!(format!("{err:#}").contains("B-deadbeef"));
    }

    #[test]
    fn show_reads_the_three_evidence_shapes_from_the_directory() {
        let out = under_root(|root| {
            evidence_dir(root, "B-2");
            let dir = evidence_dir(root, "B-3");
            fs::write(dir.join("trace.har"), b"12345").unwrap();
            fs::write(dir.join("shot.png"), b"1234").unwrap();
            build_show(&doc(RELATED), "B-1").unwrap()
        });

        // The subject has no directory at all.
        assert_eq!(out["evidence"], JsonValue::Null);

        let marker_only = &out["neighbours"][0]["evidence"];
        assert_eq!(
            marker_only["dir"],
            json!(".claude/backlog-evidence/B-2")
        );
        assert_eq!(marker_only["files"], json!([]));

        let populated = &out["neighbours"][1]["evidence"];
        assert_eq!(
            populated["files"],
            json!([
                { "name": "shot.png", "bytes": 4 },
                { "name": "trace.har", "bytes": 5 },
            ])
        );
    }

    #[test]
    fn has_evidence_keeps_only_populated_directories() {
        let kept = under_root(|root| {
            evidence_dir(root, "B-2");
            let dir = evidence_dir(root, "B-3");
            fs::write(dir.join("shot.png"), b"1234").unwrap();
            let d = doc(RELATED);
            let f = Filters {
                tags: &[],
                area_prefix: None,
                has_evidence: true,
            };
            filter_backlog(items_array(&d, ARRAY_BACKLOG), &f).unwrap()
        });
        assert_eq!(ids(&kept), ["B-3"]);
    }

    #[test]
    fn a_missing_store_lists_empty_and_gates_on_strict_read() {
        under_root(|root| {
            let file = root.join(".claude").join("backlog.toml");
            assert!(strict_read_gate(&file, false).is_ok());
            let err = strict_read_gate(&file, true).unwrap_err();
            assert_eq!(kind_of(&err), "not_found");

            let d = read_store(&file, read_integrity_opts_default()).unwrap();
            assert!(items_array(&d, ARRAY_BACKLOG).is_empty());
            let q = build_query(&None, &None, true, false, &query_args(&[])).unwrap();
            assert_eq!(
                crate::query::run(&d, ARRAY_BACKLOG, &q).unwrap(),
                json!([])
            );
        });
    }

    fn read_integrity_opts_default() -> IntegrityOpts {
        IntegrityOpts {
            write_sidecar: true,
            verify_on_read: false,
            strict: false,
        }
    }
}
