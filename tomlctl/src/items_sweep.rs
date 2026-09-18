//! `items sweep`: re-run a ledger item's stored `sweep` strings and diff against `instances`.
//!
//! One engine run over the union of every selected item's patterns; hit files
//! are then re-read so a line several items' patterns share is attributed to
//! each of them, and so a symbol anchor can be probed in the same buffer.
//!
//! An anchor is `kept` when a hit for its item lands on its line (a symbol
//! anchor: anywhere in its file, with the symbol still present), `gone` when
//! its file was scanned and no hit landed, and `unverified` when its file was
//! not scanned — excluded, ignored, skipped, missing, outside the root, or cut
//! by `max_hits` — so absence of a hit is never mistaken for evidence. A
//! bare `file` counts towards `recorded` only; with a `symbol` it is the
//! item's implicit `file:symbol` anchor.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use regex::bytes::Regex;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::anchor::{self, Anchor, AnchorAt};
use crate::convert::str_field;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{item_id, items_array, relativise_under};
use crate::items::{MutationPlan, compute_apply_mutation};
use crate::query::compile_user_bytes_regex;
use crate::repo_files::tracked_files;
use crate::sweep::{self, SweepOptions};

/// Mirrors the engine's binary sniff, which is not exported.
const BINARY_SNIFF_BYTES: usize = 8192;

struct Selected<'a> {
    id: &'a str,
    tbl: &'a toml::Table,
    patterns: Vec<usize>,
}

/// One recorded file, read once under the engine's own predicates.
/// `bytes` is empty whenever `scanned` is false.
struct Probed {
    scanned: bool,
    bytes: Vec<u8>,
    newlines: Vec<usize>,
}

struct Probe<'a> {
    root: &'a Path,
    opts: &'a SweepOptions,
    enumerated: HashSet<String>,
    keys: HashMap<String, Option<String>>,
    files: HashMap<String, Probed>,
    symbols: HashMap<String, Option<Regex>>,
}

impl Probe<'_> {
    /// The canonical relative key a recorded path shares with the engine's
    /// hits, or `None` when it does not resolve under the root.
    fn key(&mut self, file: &str) -> Option<String> {
        if let Some(cached) = self.keys.get(file) {
            return cached.clone();
        }
        let path = Path::new(file);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            let mut joined = self.root.to_path_buf();
            joined.extend(
                file.split(['/', '\\'])
                    .filter(|c| !c.is_empty() && *c != "."),
            );
            joined
        };
        let key = fs::canonicalize(&absolute)
            .ok()
            .and_then(|canonical| relativise_under(self.root, &canonical));
        self.keys.insert(file.to_string(), key.clone());
        key
    }

    fn file(&mut self, key: &str) -> &Probed {
        if !self.files.contains_key(key) {
            let probed = self.read(key);
            self.files.insert(key.to_string(), probed);
        }
        &self.files[key]
    }

    fn read(&self, key: &str) -> Probed {
        let unscanned = Probed {
            scanned: false,
            bytes: Vec::new(),
            newlines: Vec::new(),
        };
        if !self.enumerated.contains(key) {
            return unscanned;
        }
        let mut absolute = self.root.to_path_buf();
        absolute.extend(key.split('/'));
        let Ok(meta) = fs::metadata(&absolute) else {
            return unscanned;
        };
        if meta.is_dir() || meta.len() > self.opts.max_file_bytes {
            return unscanned;
        }
        let Ok(bytes) = fs::read(&absolute) else {
            return unscanned;
        };
        if memchr::memchr(0, &bytes[..bytes.len().min(BINARY_SNIFF_BYTES)]).is_some() {
            return unscanned;
        }
        let newlines = memchr::memchr_iter(b'\n', &bytes).collect();
        Probed {
            scanned: true,
            bytes,
            newlines,
        }
    }

    /// First line the symbol appears on, `None` when it is absent.
    fn symbol_line(&mut self, key: &str, symbol: &str) -> Option<u64> {
        let re = self
            .symbols
            .entry(symbol.to_string())
            .or_insert_with(|| {
                Regex::new(&format!(r"(?-u:\b){}(?-u:\b)", regex::escape(symbol))).ok()
            })
            .clone();
        let probed = self.file(key);
        let offset = match re {
            Some(re) => re.find(&probed.bytes).map(|m| m.start()),
            None => memchr::memmem::find(&probed.bytes, symbol.as_bytes()),
        }?;
        Some(line_of(&probed.newlines, offset))
    }
}

fn line_of(newlines: &[usize], offset: usize) -> u64 {
    newlines.partition_point(|&nl| nl < offset) as u64 + 1
}

fn string_array<'a>(tbl: &'a toml::Table, key: &str) -> Vec<&'a str> {
    tbl.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

fn skipped(id: &str, reason: &str) -> JsonValue {
    json!({ "id": id, "reason": reason })
}

fn select<'a>(
    items: &'a [TomlValue],
    ids: &[String],
    patterns: &mut Vec<String>,
) -> (Vec<Selected<'a>>, Vec<JsonValue>) {
    let mut by_id: BTreeMap<&str, &toml::Table> = BTreeMap::new();
    let mut order: Vec<&str> = Vec::new();
    for item in items {
        if let (Some(id), Some(tbl)) = (item_id(item), item.as_table())
            && by_id.insert(id, tbl).is_none()
        {
            order.push(id);
        }
    }
    let wanted: Vec<&str> = if ids.is_empty() {
        order
    } else {
        let mut seen = BTreeSet::new();
        ids.iter()
            .map(String::as_str)
            .filter(|id| seen.insert(*id))
            .collect()
    };

    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut selected = Vec::new();
    let mut skipped_items = Vec::new();
    for id in wanted {
        let Some((&id, &tbl)) = by_id.get_key_value(id) else {
            skipped_items.push(skipped(id, "unknown-id"));
            continue;
        };
        let sweep = string_array(tbl, "sweep");
        if sweep.is_empty() {
            skipped_items.push(skipped(id, "no-sweep"));
            continue;
        }
        let owned = sweep
            .into_iter()
            .map(|p| {
                *index.entry(p).or_insert_with(|| {
                    patterns.push(p.to_string());
                    patterns.len() - 1
                })
            })
            .collect();
        selected.push(Selected {
            id,
            tbl,
            patterns: owned,
        });
    }
    (selected, skipped_items)
}

/// The ledger's own path in the form the exclude globs match, so its verbatim
/// `sweep` strings never surface as sites.
fn ledger_exclude(root: &Path, ledger: &Path) -> Option<String> {
    let resolved = fs::canonicalize(ledger).unwrap_or_else(|_| ledger.to_path_buf());
    let rel = relativise_under(root, &resolved)?;
    Some(globset::escape(rel.trim_start_matches("./")))
}

pub(crate) fn items_sweep(
    doc: &TomlValue,
    ledger: &Path,
    root: &Path,
    ids: &[String],
    opts: &SweepOptions,
) -> Result<JsonValue> {
    let mut patterns: Vec<String> = Vec::new();
    let (selected, skipped_items) = select(items_array(doc, "items"), ids, &mut patterns);

    let mut exclude = opts.exclude.clone();
    exclude.extend(ledger_exclude(root, ledger));
    let opts = SweepOptions {
        max_file_bytes: opts.max_file_bytes,
        max_hits: opts.max_hits,
        exclude,
    };
    let report = sweep::run(root, &patterns, &opts)?;
    let regexes = patterns
        .iter()
        .map(|p| compile_user_bytes_regex(p))
        .collect::<Result<Vec<Regex>>>()?;
    let enumerated = tracked_files(root, &opts.exclude)?
        .files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let mut probe = Probe {
        root,
        opts: &opts,
        enumerated,
        keys: HashMap::new(),
        files: HashMap::new(),
        symbols: HashMap::new(),
    };

    // The engine keeps one pattern index per line, so a line two items'
    // patterns share is re-attributed here from the file bytes.
    let mut owners: Vec<Vec<usize>> = vec![Vec::new(); patterns.len()];
    for (i, item) in selected.iter().enumerate() {
        for &p in &item.patterns {
            owners[p].push(i);
        }
    }
    let mut sites: Vec<BTreeSet<(String, u64)>> = vec![BTreeSet::new(); selected.len()];
    let hit_files: BTreeSet<&str> = report.hits.iter().map(|h| h.file.as_str()).collect();
    for key in hit_files {
        let probed = probe.file(key);
        for (p, re) in regexes.iter().enumerate() {
            for m in re.find_iter(&probed.bytes) {
                let line = line_of(&probed.newlines, m.start());
                for &owner in &owners[p] {
                    sites[owner].insert((key.to_string(), line));
                }
            }
        }
    }

    let coverage_complete = report.coverage_complete();
    let mut items_out = Vec::with_capacity(selected.len());
    for (i, item) in selected.iter().enumerate() {
        let hits = &sites[i];
        let file = str_field(item.tbl, "file");
        let symbol = str_field(item.tbl, "symbol");
        let mut anchors: Vec<Anchor> = Vec::new();
        let mut unverified: Vec<String> = Vec::new();
        let mut recorded: BTreeSet<String> = BTreeSet::new();
        if !file.is_empty() {
            recorded.insert(probe.key(file).unwrap_or_else(|| file.to_string()));
            if !symbol.is_empty() {
                anchors.push(Anchor {
                    file: file.to_string(),
                    at: AnchorAt::Symbol(symbol.to_string()),
                });
            }
        }
        // `instances` usually lists the implicit anchor too; one entry per
        // anchor keeps `--update` from appending a copy on every run.
        for instance in string_array(item.tbl, "instances") {
            match anchor::parse(instance) {
                Some(a) if anchors.contains(&a) => {}
                Some(a) => anchors.push(a),
                None => unverified.push(instance.to_string()),
            }
        }

        let mut kept: Vec<String> = Vec::new();
        let mut gone: Vec<JsonValue> = Vec::new();
        let mut covered: BTreeSet<(String, u64)> = BTreeSet::new();
        for a in &anchors {
            let Some(key) = probe.key(&a.file) else {
                recorded.insert(a.file.clone());
                unverified.push(a.to_string());
                continue;
            };
            recorded.insert(key.clone());
            let file_hit = hits
                .range((key.clone(), 0)..=(key.clone(), u64::MAX))
                .next()
                .is_some();
            let verdict = match &a.at {
                AnchorAt::Line(line) => {
                    if hits.contains(&(key.clone(), *line)) {
                        covered.insert((key.clone(), *line));
                        None
                    } else {
                        Some("no-hit")
                    }
                }
                AnchorAt::Symbol(symbol) if file_hit => match probe.symbol_line(&key, symbol) {
                    Some(line) => {
                        covered.insert((key.clone(), line));
                        None
                    }
                    None => Some("symbol-missing"),
                },
                AnchorAt::Symbol(_) => Some("no-hit"),
            };
            match verdict {
                None => kept.push(a.to_string()),
                Some(reason) if !report.truncated && probe.file(&key).scanned => {
                    gone.push(json!({ "anchor": a.to_string(), "reason": reason }));
                }
                Some(_) => unverified.push(a.to_string()),
            }
        }

        let found: BTreeSet<&str> = hits.iter().map(|(f, _)| f.as_str()).collect();
        let new: Vec<String> = hits
            .iter()
            .filter(|site| !covered.contains(site))
            .map(|(f, line)| format!("{f}:{line}"))
            .collect();
        items_out.push(json!({
            "id": item.id,
            "recorded": recorded,
            "found": found,
            "new": new,
            "gone": gone,
            "kept": kept,
            "unverified": unverified,
            "coverage_complete": coverage_complete && unverified.is_empty(),
            "truncated": report.truncated,
        }));
    }

    Ok(json!({
        "items": items_out,
        "skipped_items": skipped_items,
        "files_scanned": report.files_scanned,
        "coverage_complete": coverage_complete,
        "truncated": report.truncated,
    }))
}

fn strings(v: Option<&JsonValue>) -> Vec<&str> {
    v.and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

/// Rewrites each swept item's `instances` to its kept anchors followed by the
/// new `file:line` sites. The implicit `file:symbol` anchor is only written
/// when it was already listed; an item whose list is unchanged gets no op.
pub(crate) fn update_plan(doc: &TomlValue, results: &JsonValue) -> Result<MutationPlan> {
    let items = strings_objects(results.get("items"));
    let blocked: Vec<String> = items
        .iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(|v| v.as_str())?;
            let truncated = item.get("truncated").and_then(|v| v.as_bool()) == Some(true);
            let unverified = strings(item.get("unverified")).len();
            match (truncated, unverified) {
                (true, _) => Some(format!("{id} (truncated)")),
                (false, n) if n > 0 => Some(format!("{id} ({n} unverified)")),
                _ => None,
            }
        })
        .collect();
    if !blocked.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "refusing --update: absence of a hit is not evidence for {}; raise --max-hits or resolve the unverified anchors first",
                blocked.join(", ")
            ),
        ));
    }

    let current: HashMap<&str, Vec<&str>> = items_array(doc, "items")
        .iter()
        .filter_map(|item| Some((item_id(item)?, string_array(item.as_table()?, "instances"))))
        .collect();
    let mut ops = Vec::new();
    for item in &items {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let listed = current.get(id).cloned().unwrap_or_default();
        let instances: Vec<&str> = strings(item.get("kept"))
            .into_iter()
            .filter(|kept| listed.contains(kept))
            .chain(strings(item.get("new")))
            .collect();
        if instances == listed {
            continue;
        }
        // An empty patch value is skipped by `update`, so an emptied list
        // goes through `unset`.
        ops.push(if instances.is_empty() {
            json!({ "op": "update", "id": id, "json": {}, "unset": ["instances"] })
        } else {
            json!({ "op": "update", "id": id, "json": { "instances": instances } })
        });
    }
    compute_apply_mutation(doc, "items", &JsonValue::Array(ops), false)
}

fn strings_objects(v: Option<&JsonValue>) -> Vec<&JsonValue> {
    v.and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter(|v| v.is_object()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TaggedError;
    use crate::test_support::with_root;
    use std::process::Command;

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    const LEDGER: &str = "review-ledger.toml";

    /// `R1` anchors `src/a.rs:alpha` through `file`/`symbol`, keeps a line
    /// anchor in `src/b.rs`, has lost its site in `src/c.rs` and its symbol
    /// in `src/d.rs`; `src/e.rs` is an unrecorded site. The ledger sits in
    /// the repo root, where the engine would enumerate it.
    fn seed(root: &Path) -> (std::path::PathBuf, TomlValue) {
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        write(
            root,
            "src/a.rs",
            b"fn alpha() { needle(); }\nfn x() {}\n\nfn beta() { needle(); }\n",
        );
        write(root, "src/b.rs", b"needle();\n");
        write(root, "src/c.rs", b"fn c() {}\n");
        write(root, "src/d.rs", b"needle();\n");
        write(root, "src/e.rs", b"needle();\n");
        let ledger = r#"
[[items]]
id = "R1"
file = "src/a.rs"
symbol = "alpha"
instances = ["src/b.rs:1", "src/c.rs:1", "src/d.rs:zeta"]
sweep = ['(?-u:\bneedle\b)']
enumeration = "complete"

[[items]]
id = "R2"
file = "src/a.rs"
summary = "needle without a sweep"
"#;
        write(root, LEDGER, ledger.as_bytes());
        (root.join(LEDGER), toml::from_str(ledger).unwrap())
    }

    fn run(root: &Path, ids: &[&str], opts: &SweepOptions) -> (JsonValue, TomlValue) {
        let (ledger, doc) = seed(root);
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        (items_sweep(&doc, &ledger, root, &ids, opts).unwrap(), doc)
    }

    /// Every item, default options, a ledger path that is not on disk.
    fn sweep_all(root: &Path, doc: &TomlValue) -> JsonValue {
        items_sweep(
            doc,
            &root.join("x.toml"),
            root,
            &[],
            &SweepOptions::default(),
        )
        .unwrap()
    }

    fn strs(v: &JsonValue) -> Vec<&str> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect()
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    #[test]
    fn partitions_new_gone_and_kept() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            let (out, doc) = run(root, &[], &SweepOptions::default());
            let items = out["items"].as_array().unwrap();
            assert_eq!(items.len(), 1, "{out}");
            let r1 = &items[0];
            assert_eq!(r1["id"], "R1");
            assert_eq!(
                strs(&r1["recorded"]),
                ["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs"]
            );
            assert_eq!(
                strs(&r1["found"]),
                ["src/a.rs", "src/b.rs", "src/d.rs", "src/e.rs"]
            );
            assert_eq!(strs(&r1["new"]), ["src/a.rs:4", "src/d.rs:1", "src/e.rs:1"]);
            assert_eq!(strs(&r1["kept"]), ["src/a.rs:alpha", "src/b.rs:1"]);
            assert_eq!(
                r1["gone"],
                json!([
                    { "anchor": "src/c.rs:1", "reason": "no-hit" },
                    { "anchor": "src/d.rs:zeta", "reason": "symbol-missing" },
                ])
            );
            assert_eq!(strs(&r1["unverified"]), [] as [&str; 0]);
            assert_eq!(r1["coverage_complete"], true);
            assert_eq!(r1["truncated"], false);
            assert_eq!(
                out["skipped_items"],
                json!([{ "id": "R2", "reason": "no-sweep" }])
            );
            assert_eq!(out["files_scanned"], 5);
            assert_eq!(out["coverage_complete"], true);
            assert!(
                !out.to_string().contains(LEDGER),
                "the ledger's own sweep strings surfaced: {out}"
            );

            let plan = update_plan(&doc, &out).unwrap();
            assert_eq!(plan.updated, ["R1"]);
            let items = items_array(&plan.new_doc, "items");
            let r1 = items[0].as_table().unwrap();
            assert_eq!(
                string_array(r1, "instances"),
                ["src/b.rs:1", "src/a.rs:4", "src/d.rs:1", "src/e.rs:1"]
            );
            assert_eq!(str_field(r1, "enumeration"), "complete");
            assert_eq!(items[1], doc["items"][1]);
        });
    }

    #[test]
    fn a_shared_line_is_attributed_to_every_item() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["init", "-q"])
                .output()
                .unwrap();
            write(root, "a.rs", b"foo bar\n");
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
sweep = ["foo"]

[[items]]
id = "R2"
sweep = ["bar"]
"#,
            )
            .unwrap();
            let out = sweep_all(root, &doc);
            let items = out["items"].as_array().unwrap();
            assert_eq!(strs(&items[0]["new"]), ["a.rs:1"]);
            assert_eq!(strs(&items[1]["new"]), ["a.rs:1"]);
        });
    }

    #[test]
    fn explicit_ids_report_unknown_and_no_sweep() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            let (out, _) = run(root, &["R2", "R9", "R2"], &SweepOptions::default());
            assert_eq!(out["items"], json!([]));
            assert_eq!(
                out["skipped_items"],
                json!([
                    { "id": "R2", "reason": "no-sweep" },
                    { "id": "R9", "reason": "unknown-id" },
                ])
            );
        });
    }

    #[test]
    fn an_unscanned_file_is_unverified_and_blocks_update() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["init", "-q"])
                .output()
                .unwrap();
            write(root, "blob.rs", b"needle\0\nneedle\nneedle\n");
            write(root, "ok.rs", b"needle\n");
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
instances = ["blob.rs:3", "nope.rs:1", "ok.rs:1"]
sweep = ["needle"]
"#,
            )
            .unwrap();
            let out = sweep_all(root, &doc);
            let r1 = &out["items"][0];
            assert_eq!(strs(&r1["unverified"]), ["blob.rs:3", "nope.rs:1"]);
            assert_eq!(strs(&r1["kept"]), ["ok.rs:1"]);
            assert_eq!(r1["gone"], json!([]));
            assert_eq!(r1["coverage_complete"], false);

            let err = update_plan(&doc, &out).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(err.to_string().contains("R1 (2 unverified)"), "{err}");
        });
    }

    #[test]
    fn a_truncated_run_leaves_missing_anchors_unverified_and_blocks_update() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            let opts = SweepOptions {
                max_hits: 1,
                ..SweepOptions::default()
            };
            let (out, doc) = run(root, &["R1"], &opts);
            let r1 = &out["items"][0];
            assert_eq!(r1["truncated"], true);
            assert_eq!(r1["gone"], json!([]));
            assert!(strs(&r1["unverified"]).contains(&"src/c.rs:1"), "{r1}");
            let err = update_plan(&doc, &out).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(err.to_string().contains("R1 (truncated)"), "{err}");
        });
    }

    #[test]
    fn a_listed_implicit_anchor_is_counted_once_and_update_is_idempotent() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            let (ledger, _) = seed(root);
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
file = "src/a.rs"
symbol = "alpha"
instances = ["src/a.rs:alpha", "src/b.rs:1", "src/a.rs:alpha"]
sweep = ['(?-u:\bneedle\b)']
"#,
            )
            .unwrap();
            let first = items_sweep(&doc, &ledger, root, &[], &SweepOptions::default()).unwrap();
            assert_eq!(
                strs(&first["items"][0]["kept"]),
                ["src/a.rs:alpha", "src/b.rs:1"]
            );
            let plan = update_plan(&doc, &first).unwrap();
            assert_eq!(plan.updated, ["R1"]);
            let once = plan.new_doc;
            let listed = string_array(once["items"][0].as_table().unwrap(), "instances");
            assert_eq!(listed.len(), 5, "{listed:?}");

            let second = items_sweep(&once, &ledger, root, &[], &SweepOptions::default()).unwrap();
            let plan = update_plan(&once, &second).unwrap();
            assert_eq!(plan.updated, [] as [&str; 0]);
            assert_eq!(
                string_array(plan.new_doc["items"][0].as_table().unwrap(), "instances"),
                listed
            );
        });
    }

    #[test]
    fn an_emptied_list_unsets_instances_and_an_unchanged_one_is_left_alone() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["init", "-q"])
                .output()
                .unwrap();
            write(root, "a.rs", b"fn a() {}\n");
            write(root, "b.rs", b"needle\n");
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
instances = ["a.rs:1"]
sweep = ["absent"]

[[items]]
id = "R2"
instances = ["b.rs:1"]
sweep = ["needle"]
"#,
            )
            .unwrap();
            let out = sweep_all(root, &doc);
            let plan = update_plan(&doc, &out).unwrap();
            assert_eq!(plan.updated, ["R1"]);
            let items = items_array(&plan.new_doc, "items");
            assert!(items[0].get("instances").is_none(), "{:?}", items[0]);
            assert_eq!(items[1], doc["items"][1]);
        });
    }
}
