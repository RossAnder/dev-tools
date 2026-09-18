//! `items clusters`: file-disjoint clusters and dependency batches over ledger items.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::anchor;
use crate::convert::str_field;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{canonical_key, item_id, items_array};
use crate::items::is_terminal_status;
use crate::tasks::{cycle_within, layered_kahn};
use crate::union_find::{components, union};

/// Items are layered over `depends_on` before anything is unioned, and a
/// shared file joins two items only within one layer, so a dependent editing
/// its dependency's file lands in a later batch rather than the same cluster.
/// `item_ids` and cluster numbering follow ledger order; `files` sort lexically.
///
/// A `depends_on` target outside the selection is dropped and reported under
/// `dropped_deps`, split by whether the ledger holds it: `unselected` carries
/// each such id with its status (absent reads as `open`), `unknown` the ids
/// the ledger does not hold at all.
pub(crate) fn items_clusters(doc: &TomlValue, root: &Path, ids: &[String]) -> Result<JsonValue> {
    let items = items_array(doc, "items");
    let selected = select(items, ids)?;
    let n = selected.len();
    let in_ledger: HashMap<&str, &toml::Table> = items
        .iter()
        .filter_map(|item| Some((item_id(item)?, item.as_table()?)))
        .collect();
    let position: HashMap<&str, usize> = selected
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();

    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut key_cache: HashMap<String, String> = HashMap::new();
    let mut key = |file: &str| -> String {
        key_cache
            .entry(file.to_string())
            .or_insert_with(|| file_key(&root, file))
            .clone()
    };
    let files: Vec<BTreeSet<String>> = selected
        .iter()
        .map(|(_, tbl)| {
            let anchors: Vec<anchor::Anchor> = tbl
                .get("instances")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().and_then(anchor::parse))
                .collect();
            let mut set: BTreeSet<String> = anchor::files_of(anchors.iter())
                .into_iter()
                .map(&mut key)
                .collect();
            let file = str_field(tbl, "file");
            if !file.is_empty() {
                set.insert(key(file));
            }
            set
        })
        .collect();

    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut dropped_deps: Vec<JsonValue> = Vec::new();
    for (i, (id, tbl)) in selected.iter().enumerate() {
        let mut unselected: BTreeMap<&str, &str> = BTreeMap::new();
        let mut unknown: BTreeSet<&str> = BTreeSet::new();
        let deps = tbl
            .get("depends_on")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str());
        for dep in deps {
            match position.get(dep) {
                Some(&j) if j == i => {}
                Some(&j) => {
                    if !preds[i].contains(&j) {
                        preds[i].push(j);
                        succs[j].push(i);
                    }
                }
                None => match in_ledger.get(dep) {
                    Some(row) => {
                        let status = str_field(row, "status");
                        unselected.insert(dep, if status.is_empty() { "open" } else { status });
                    }
                    None => {
                        unknown.insert(dep);
                    }
                },
            }
        }
        if !unselected.is_empty() || !unknown.is_empty() {
            let unselected: Vec<JsonValue> = unselected
                .iter()
                .map(|(dep, status)| json!({ "id": dep, "status": status }))
                .collect();
            dropped_deps.push(json!({ "id": id, "unselected": unselected, "unknown": unknown }));
        }
    }

    let (rounds, residual) = layered_kahn(n, &preds, &succs);
    if !residual.is_empty() {
        let cycle: Vec<&str> = cycle_within(&residual, &succs)
            .iter()
            .map(|p| selected[*p].0)
            .collect();
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "refusing items clusters: the dependency graph contains a cycle through items {}",
                cycle.join(", ")
            ),
        ));
    }

    let mut level = vec![0usize; n];
    let mut parent: Vec<usize> = (0..n).collect();
    for (round, members) in rounds.iter().enumerate() {
        let mut first_holder: HashMap<&str, usize> = HashMap::new();
        for &p in members {
            level[p] = round;
            for file in &files[p] {
                match first_holder.entry(file.as_str()) {
                    Entry::Occupied(holder) => union(&mut parent, p, *holder.get()),
                    Entry::Vacant(slot) => {
                        slot.insert(p);
                    }
                }
            }
        }
    }
    let clusters: Vec<Vec<usize>> = components(&mut parent, n, true).into_values().collect();
    let mut cluster_of = vec![0usize; n];
    for (k, members) in clusters.iter().enumerate() {
        for &p in members {
            cluster_of[p] = k;
        }
    }
    let mut cluster_deps: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); clusters.len()];
    for (i, ps) in preds.iter().enumerate() {
        for &j in ps {
            if cluster_of[j] != cluster_of[i] {
                cluster_deps[cluster_of[i]].insert(cluster_of[j]);
            }
        }
    }

    let cluster_id = |k: usize| format!("c{}", k + 1);
    let mut batches: Vec<Vec<String>> = vec![Vec::new(); rounds.len()];
    let clusters_json: Vec<JsonValue> = clusters
        .iter()
        .enumerate()
        .map(|(k, members)| {
            let cluster_files: BTreeSet<&str> = members
                .iter()
                .flat_map(|p| files[*p].iter().map(String::as_str))
                .collect();
            let lite_file_scope = cluster_files.len() <= 2
                || (members.len() == 1
                    && str_field(selected[members[0]].1, "enumeration") == "complete");
            batches[level[members[0]]].push(cluster_id(k));
            json!({
                "id": cluster_id(k),
                "item_ids": members.iter().map(|p| selected[*p].0).collect::<Vec<_>>(),
                "files": cluster_files,
                "depends_on": cluster_deps[k].iter().map(|d| cluster_id(*d)).collect::<Vec<_>>(),
                "lite_file_scope": lite_file_scope,
            })
        })
        .collect();

    Ok(json!({
        "clusters": clusters_json,
        "batches": batches,
        "dropped_deps": dropped_deps,
    }))
}

/// The named ids, or every row that is not closed when none are named — in
/// ledger order either way; an absent status reads as `open`, as it does for
/// `items sweep --update`. A row without an id cannot be selected or
/// reported, so it is skipped.
fn select<'a>(items: &'a [TomlValue], ids: &[String]) -> Result<Vec<(&'a str, &'a toml::Table)>> {
    let mut wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut selected = Vec::new();
    for item in items {
        let (Some(tbl), Some(id)) = (item.as_table(), item_id(item)) else {
            continue;
        };
        let take = if ids.is_empty() {
            !is_terminal_status(str_field(tbl, "status"))
        } else {
            wanted.remove(id)
        };
        if take {
            selected.push((id, tbl));
        }
    }
    if let Some(unknown) = ids.iter().find(|id| wanted.contains(id.as_str())) {
        return Err(tagged_err(
            ErrorKind::NotFound,
            None,
            format!("no item with id \"{unknown}\""),
        ));
    }
    Ok(selected)
}

/// `file` resolved on disk and relativised under `root`, so the `.github/`
/// mirror of a `claude/` file is one key. A path that does not lie under the
/// root keeps its verbatim spelling; whether it should have is `items
/// orphans`' answer.
fn file_key(root: &Path, file: &str) -> String {
    canonical_key(root, file).unwrap_or_else(|| file.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TaggedError;

    fn run_in(root: &Path, ledger: &str, ids: &[&str]) -> Result<JsonValue> {
        let doc: TomlValue = toml::from_str(ledger).unwrap();
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        items_clusters(&doc, root, &ids)
    }

    fn run(ledger: &str, ids: &[&str]) -> Result<JsonValue> {
        let tmp = tempfile::tempdir().unwrap();
        run_in(tmp.path(), ledger, ids)
    }

    fn strings(v: &JsonValue) -> Vec<&str> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect()
    }

    fn cluster_field<'a>(out: &'a JsonValue, field: &str) -> Vec<&'a JsonValue> {
        out["clusters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| &c[field])
            .collect()
    }

    fn item_ids(out: &JsonValue) -> Vec<Vec<&str>> {
        cluster_field(out, "item_ids")
            .into_iter()
            .map(strings)
            .collect()
    }

    fn batches(out: &JsonValue) -> Vec<Vec<&str>> {
        out["batches"]
            .as_array()
            .unwrap()
            .iter()
            .map(strings)
            .collect()
    }

    fn lite(out: &JsonValue) -> Vec<bool> {
        cluster_field(out, "lite_file_scope")
            .into_iter()
            .map(|v| v.as_bool().unwrap())
            .collect()
    }

    #[test]
    fn same_level_items_sharing_a_file_cluster_together() {
        let out = run(
            r#"
[[items]]
id = "R1"
status = "open"
file = "src/a.rs"

[[items]]
id = "R2"
status = "open"
file = "src/b.rs"
instances = ["src/a.rs:foo"]

[[items]]
id = "R3"
status = "open"
file = "src/c.rs"
"#,
            &[],
        )
        .unwrap();
        assert_eq!(item_ids(&out), [vec!["R1", "R2"], vec!["R3"]]);
        assert_eq!(
            strings(cluster_field(&out, "files")[0]),
            ["src/a.rs", "src/b.rs"]
        );
        assert_eq!(batches(&out), [["c1", "c2"]]);
        assert_eq!(out["dropped_deps"], json!([]));
    }

    #[test]
    fn a_chain_yields_two_batches() {
        let out = run(
            r#"
[[items]]
id = "R1"
status = "open"
file = "src/a.rs"

[[items]]
id = "R2"
status = "open"
file = "src/b.rs"
depends_on = ["R1"]
"#,
            &[],
        )
        .unwrap();
        assert_eq!(item_ids(&out), [["R1"], ["R2"]]);
        assert_eq!(batches(&out), [["c1"], ["c2"]]);
        assert_eq!(
            strings(cluster_field(&out, "depends_on")[0]),
            [] as [&str; 0]
        );
        assert_eq!(strings(cluster_field(&out, "depends_on")[1]), ["c1"]);
    }

    #[test]
    fn a_chain_sharing_a_file_yields_two_clusters_in_two_batches() {
        let out = run(
            r#"
[[items]]
id = "R1"
status = "open"
file = "src/a.rs"

[[items]]
id = "R2"
status = "open"
file = "src/a.rs"
depends_on = ["R1"]
"#,
            &[],
        )
        .unwrap();
        assert_eq!(item_ids(&out), [["R1"], ["R2"]]);
        assert_eq!(batches(&out), [["c1"], ["c2"]]);
    }

    /// `R3` hangs off the cycle without being on it, so it is stranded in
    /// the Kahn residue but must not be named.
    #[test]
    fn a_cycle_is_refused_before_clustering() {
        let err = run(
            r#"
[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
depends_on = ["R2"]

[[items]]
id = "R2"
status = "open"
file = "src/b.rs"
depends_on = ["R1"]

[[items]]
id = "R3"
status = "open"
file = "src/c.rs"
depends_on = ["R2"]
"#,
            &[],
        )
        .unwrap_err();
        let tagged = err.downcast_ref::<TaggedError>().unwrap();
        assert!(matches!(tagged.kind, ErrorKind::Validation), "{tagged:?}");
        assert_eq!(
            tagged.message,
            "refusing items clusters: the dependency graph contains a cycle through items R1, R2"
        );
    }

    #[test]
    fn a_dependency_outside_the_selection_is_dropped_and_reported() {
        let out = run(
            r#"
[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
depends_on = ["R9", "R1", "R2", "R9", "R3"]

[[items]]
id = "R2"
status = "fixed"
file = "src/b.rs"

[[items]]
id = "R3"
status = "deferred"
file = "src/c.rs"
"#,
            &[],
        )
        .unwrap();
        assert_eq!(item_ids(&out), [["R1"]]);
        assert_eq!(batches(&out), [["c1"]]);
        assert_eq!(
            out["dropped_deps"],
            json!([{
                "id": "R1",
                "unselected": [
                    { "id": "R2", "status": "fixed" },
                    { "id": "R3", "status": "deferred" },
                ],
                "unknown": ["R9"],
            }])
        );
    }

    /// The bare selection follows the schema's fail-soft rule — an absent or
    /// unrecognised status is `open` — so it agrees with the status it reports
    /// for an unselected dependency and with `items sweep --update`.
    #[test]
    fn the_bare_selection_reads_an_absent_or_unknown_status_as_open() {
        let out = run(
            r#"
[[items]]
id = "R1"
file = "src/a.rs"

[[items]]
id = "R2"
status = "triaged"
file = "src/b.rs"

[[items]]
id = "R3"
status = "wontfix"
file = "src/c.rs"
"#,
            &[],
        )
        .unwrap();
        assert_eq!(item_ids(&out), [["R1"], ["R2"]]);
    }

    #[test]
    fn named_ids_select_any_status_in_ledger_order_and_an_unknown_id_is_not_found() {
        let ledger = r#"
[[items]]
id = "R1"
status = "fixed"
file = "src/a.rs"

[[items]]
id = "R2"
status = "open"
file = "src/b.rs"
"#;
        let out = run(ledger, &["R2", "R1"]).unwrap();
        assert_eq!(item_ids(&out), [["R1"], ["R2"]]);

        let err = run(ledger, &["R2", "R7"]).unwrap_err();
        let tagged = err.downcast_ref::<TaggedError>().unwrap();
        assert!(matches!(tagged.kind, ErrorKind::NotFound), "{tagged:?}");
        assert!(tagged.message.contains("R7"), "{}", tagged.message);
    }

    #[test]
    fn lite_file_scope_follows_file_count_or_a_lone_complete_item() {
        let out = run(
            r#"
[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
enumeration = "complete"
instances = ["src/a.rs:foo", "src/b.rs:foo", "src/c.rs:foo"]

[[items]]
id = "R2"
status = "open"
file = "src/d.rs"
enumeration = "incomplete"
instances = ["src/d.rs:foo", "src/e.rs:foo", "src/f.rs:foo"]

[[items]]
id = "R3"
status = "open"
file = "src/g.rs"
instances = ["src/g.rs:foo", "src/h.rs:foo", "src/i.rs:foo"]

[[items]]
id = "R4"
status = "open"
file = "src/j.rs"
enumeration = "complete"
instances = ["src/j.rs:foo", "src/k.rs:foo"]

[[items]]
id = "R5"
status = "open"
file = "src/k.rs"
enumeration = "complete"
instances = ["src/k.rs:foo", "src/l.rs:foo"]

[[items]]
id = "R6"
status = "open"
file = "src/m.rs"
instances = ["src/m.rs:foo", "src/n.rs:foo"]
"#,
            &[],
        )
        .unwrap();
        assert_eq!(
            item_ids(&out),
            [
                vec!["R1"],
                vec!["R2"],
                vec!["R3"],
                vec!["R4", "R5"],
                vec!["R6"]
            ]
        );
        assert_eq!(lite(&out), [true, false, false, false, true]);
    }

    /// A `..` spelling is refused lexically even when it would resolve to an
    /// existing file, so it never shares a cluster with the plain spelling.
    #[test]
    fn file_keys_are_canonical_under_the_root_and_verbatim_otherwise() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("a.rs"), "fn foo() {}\n").unwrap();
        let absolute = root.canonicalize().unwrap().join("src").join("a.rs");
        let out = run_in(
            root,
            &format!(
                r#"
[[items]]
id = "R1"
status = "open"
file = "src/a.rs"

[[items]]
id = "R2"
status = "open"
file = './src\a.rs'

[[items]]
id = "R3"
status = "open"
file = '{}'

[[items]]
id = "R4"
status = "open"
file = "src/../src/a.rs"

[[items]]
id = "R5"
status = "open"
file = "src/missing.rs"
"#,
                absolute.display()
            ),
            &[],
        )
        .unwrap();
        assert_eq!(
            item_ids(&out),
            [vec!["R1", "R2", "R3"], vec!["R4"], vec!["R5"]]
        );
        assert_eq!(strings(cluster_field(&out, "files")[0]), ["src/a.rs"]);
        assert_eq!(
            strings(cluster_field(&out, "files")[1]),
            ["src/../src/a.rs"]
        );
        assert_eq!(strings(cluster_field(&out, "files")[2]), ["src/missing.rs"]);
    }
}
