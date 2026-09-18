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
//!
//! A line anchor covers its own line; a symbol anchor covers the block its
//! symbol opens, read from indentation (see `block_end`), so a hit that moves
//! within the block is not reported as `new`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use anyhow::Result;
use regex::bytes::Regex;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::anchor::{self, Anchor, AnchorAt};
use crate::convert::str_field;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{canonical_key, item_id, items_array, join_under, relativise_under};
use crate::items::{MutationPlan, compute_apply_mutation};
use crate::sweep::{self, ScannedFile, SweepOptions, SweepReport};

struct Selected<'a> {
    id: &'a str,
    tbl: &'a toml::Table,
    patterns: Vec<usize>,
}

/// Hit files re-read once each, under the engine's own predicates.
struct Probe<'a> {
    root: &'a Path,
    max_file_bytes: u64,
    keys: HashMap<String, Option<String>>,
    files: HashMap<String, Option<ScannedFile>>,
    symbols: HashMap<String, Option<Regex>>,
}

impl Probe<'_> {
    /// The canonical relative key a recorded path shares with the engine's
    /// hits, or `None` when it does not resolve under the root.
    fn key(&mut self, file: &str) -> Option<String> {
        self.keys
            .entry(file.to_string())
            .or_insert_with(|| canonical_key(self.root, file))
            .clone()
    }

    fn file(&mut self, key: &str) -> Option<&ScannedFile> {
        if !self.files.contains_key(key) {
            let file = join_under(self.root, key)
                .and_then(|path| sweep::scan_file(&path, self.max_file_bytes).ok());
            self.files.insert(key.to_string(), file);
        }
        self.files[key].as_ref()
    }

    /// Inclusive lines of the block the symbol's first occurrence opens,
    /// `None` when it is absent.
    fn symbol_span(&mut self, key: &str, symbol: &str) -> Option<(u64, u64)> {
        let re = self
            .symbols
            .entry(symbol.to_string())
            .or_insert_with(|| anchor::symbol_regex(symbol))
            .clone()?;
        let file = self.file(key)?;
        let start = sweep::line_of(&file.newlines, re.find(&file.bytes)?.start());
        Some((start, block_end(file, start)))
    }

    fn on_disk(&self, key: &str) -> bool {
        join_under(self.root, key).is_some_and(|path| path.exists())
    }
}

/// Last line of the block opened on `line`: the one before the next
/// non-blank line indented no deeper that is not a bare closing bracket, or
/// the file's last line. A one-line construct spans only itself, as does a
/// Markdown heading or a TOML table.
fn block_end(file: &ScannedFile, line: u64) -> u64 {
    let Some(opener) = file.line(line) else {
        return line;
    };
    let indent = indentation(opener);
    let mut end = line;
    while let Some(next) = file.line(end + 1) {
        let trimmed = next.trim_ascii();
        if !trimmed.is_empty() && indentation(next) <= indent && !is_closer(trimmed) {
            break;
        }
        end += 1;
    }
    end
}

fn indentation(line: &[u8]) -> usize {
    line.iter()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count()
}

/// `}`, `)` or `]`, optionally followed by `;` or `,`.
fn is_closer(trimmed: &[u8]) -> bool {
    matches!(
        trimmed,
        [b'}' | b')' | b']'] | [b'}' | b')' | b']', b';' | b',']
    )
}

fn string_array<'a>(tbl: &'a toml::Table, key: &str) -> Vec<&'a str> {
    tbl.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

pub(crate) struct SkippedItem {
    pub(crate) id: String,
    pub(crate) reason: &'static str,
}

/// An anchor with the reason it landed in its list.
pub(crate) struct AnchorReason {
    pub(crate) anchor: String,
    pub(crate) reason: &'static str,
}

pub(crate) struct ItemSweep {
    pub(crate) id: String,
    /// Canonical keys of every file the item's anchors name.
    pub(crate) recorded: BTreeSet<String>,
    /// Canonical keys of every file a hit landed in.
    pub(crate) found: BTreeSet<String>,
    /// `file:line` sites no anchor covers.
    pub(crate) new: Vec<String>,
    pub(crate) gone: Vec<AnchorReason>,
    pub(crate) kept: Vec<String>,
    /// Anchors whose file the engine did not scan, or that did not parse:
    /// `unparseable`, `outside-repo`, `truncated`, `missing`, `skipped` or
    /// `excluded`.
    pub(crate) unverified: Vec<AnchorReason>,
    pub(crate) coverage_complete: bool,
    pub(crate) truncated: bool,
}

pub(crate) struct SweepOutcome {
    pub(crate) items: Vec<ItemSweep>,
    pub(crate) skipped_items: Vec<SkippedItem>,
    pub(crate) files_scanned: usize,
    pub(crate) coverage_complete: bool,
    pub(crate) truncated: bool,
}

fn skipped(id: &str, reason: &'static str) -> SkippedItem {
    SkippedItem {
        id: id.to_string(),
        reason,
    }
}

fn reasoned(anchor: &str, reason: &'static str) -> AnchorReason {
    AnchorReason {
        anchor: anchor.to_string(),
        reason,
    }
}

/// A disposition `--update` never rewrites; anything else, an absent status
/// included, reads as `open`.
fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "fixed" | "wontfix" | "verified-clean" | "deferred" | "applied" | "wontapply"
    )
}

/// Why an anchor's file gave the engine no say on it. A scanned file only
/// reaches here under truncation; an unreached one under truncation cannot
/// be told from an excluded one, so it reads as `truncated` until the cap
/// is raised.
fn unverified_reason(report: &SweepReport, probe: &Probe<'_>, key: &str) -> &'static str {
    if report.scanned.contains(key) {
        "truncated"
    } else if !probe.on_disk(key) {
        "missing"
    } else if report.skipped.contains(key) {
        "skipped"
    } else if report.truncated {
        "truncated"
    } else {
        "excluded"
    }
}

fn select<'a>(
    items: &'a [TomlValue],
    ids: &[String],
    patterns: &mut Vec<String>,
) -> (Vec<Selected<'a>>, Vec<SkippedItem>) {
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
) -> Result<SweepOutcome> {
    let mut patterns: Vec<String> = Vec::new();
    let (selected, skipped_items) = select(items_array(doc, "items"), ids, &mut patterns);

    let mut exclude = opts.exclude.clone();
    exclude.extend(ledger_exclude(root, ledger));
    let opts = SweepOptions {
        max_file_bytes: opts.max_file_bytes,
        max_hits: opts.max_hits,
        exclude,
    };
    let regexes = sweep::compile(&patterns)?;
    let report = sweep::run_compiled(root, &regexes, &opts)?;
    let mut probe = Probe {
        root,
        max_file_bytes: opts.max_file_bytes,
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
        let Some(file) = probe.file(key) else {
            continue;
        };
        for (p, re) in regexes.iter().enumerate() {
            for m in re.find_iter(&file.bytes) {
                let line = sweep::line_of(&file.newlines, m.start());
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
        let mut unverified: Vec<AnchorReason> = Vec::new();
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
                None => unverified.push(reasoned(instance, "unparseable")),
            }
        }

        let mut kept: Vec<String> = Vec::new();
        let mut gone: Vec<AnchorReason> = Vec::new();
        let mut covered: BTreeSet<(String, u64)> = BTreeSet::new();
        for a in &anchors {
            let Some(key) = probe.key(&a.file) else {
                recorded.insert(a.file.clone());
                unverified.push(reasoned(&a.to_string(), "outside-repo"));
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
                AnchorAt::Symbol(symbol) if file_hit => match probe.symbol_span(&key, symbol) {
                    Some((start, end)) => {
                        let span = (key.clone(), start)..=(key.clone(), end);
                        covered.extend(hits.range(span).cloned());
                        None
                    }
                    None => Some("symbol-missing"),
                },
                AnchorAt::Symbol(_) => Some("no-hit"),
            };
            match verdict {
                None => kept.push(a.to_string()),
                Some(reason) if !report.truncated && report.scanned.contains(&key) => {
                    gone.push(reasoned(&a.to_string(), reason));
                }
                Some(_) => {
                    let reason = unverified_reason(&report, &probe, &key);
                    unverified.push(reasoned(&a.to_string(), reason));
                }
            }
        }

        let found: BTreeSet<String> = hits.iter().map(|(f, _)| f.clone()).collect();
        let new: Vec<String> = hits
            .iter()
            .filter(|site| !covered.contains(site))
            .map(|(f, line)| format!("{f}:{line}"))
            .collect();
        items_out.push(ItemSweep {
            id: item.id.to_string(),
            recorded,
            found,
            new,
            gone,
            kept,
            coverage_complete: coverage_complete && unverified.is_empty(),
            unverified,
            truncated: report.truncated,
        });
    }

    Ok(SweepOutcome {
        items: items_out,
        skipped_items,
        files_scanned: report.files_scanned(),
        coverage_complete,
        truncated: report.truncated,
    })
}

pub(crate) fn outcome_json(outcome: &SweepOutcome) -> JsonValue {
    let reasons = |list: &[AnchorReason]| -> Vec<JsonValue> {
        list.iter()
            .map(|r| json!({ "anchor": r.anchor, "reason": r.reason }))
            .collect()
    };
    let items: Vec<JsonValue> = outcome
        .items
        .iter()
        .map(|item| {
            json!({
                "id": item.id,
                "recorded": item.recorded,
                "found": item.found,
                "new": item.new,
                "gone": reasons(&item.gone),
                "kept": item.kept,
                "unverified": reasons(&item.unverified),
                "coverage_complete": item.coverage_complete,
                "truncated": item.truncated,
            })
        })
        .collect();
    let skipped_items: Vec<JsonValue> = outcome
        .skipped_items
        .iter()
        .map(|s| json!({ "id": s.id, "reason": s.reason }))
        .collect();
    json!({
        "items": items,
        "skipped_items": skipped_items,
        "files_scanned": outcome.files_scanned,
        "coverage_complete": outcome.coverage_complete,
        "truncated": outcome.truncated,
    })
}

/// Anchors the refusal names per item before it falls back to a count.
const NAMED_UNVERIFIED: usize = 5;

/// Rewrites each open swept item's `instances` to its retained anchors, in
/// their listed order, then the new `file:line` sites. Retained are the kept
/// anchors and the `excluded` ones, which the engine could never re-add; any
/// other unverified anchor refuses the write. An unlisted implicit anchor, a
/// terminal item, and an unchanged list each get no op.
pub(crate) fn update_plan(doc: &TomlValue, outcome: &SweepOutcome) -> Result<MutationPlan> {
    let current: HashMap<&str, (&str, Vec<&str>)> = items_array(doc, "items")
        .iter()
        .filter_map(|item| {
            let tbl = item.as_table()?;
            let row = (str_field(tbl, "status"), string_array(tbl, "instances"));
            Some((item_id(item)?, row))
        })
        .collect();
    let writable: Vec<&ItemSweep> = outcome
        .items
        .iter()
        .filter(|item| {
            let status = current.get(item.id.as_str()).map_or("", |(s, _)| s);
            !is_terminal(status)
        })
        .collect();

    let blocked: Vec<String> = writable
        .iter()
        .filter_map(|item| {
            if item.truncated {
                return Some(format!("{} (truncated)", item.id));
            }
            let blocking: Vec<String> = item
                .unverified
                .iter()
                .filter(|u| u.reason != "excluded")
                .map(|u| format!("{} {}", u.anchor, u.reason))
                .collect();
            if blocking.is_empty() {
                return None;
            }
            let more = if blocking.len() > NAMED_UNVERIFIED {
                ", ..."
            } else {
                ""
            };
            Some(format!(
                "{} ({} unverified: {}{more})",
                item.id,
                blocking.len(),
                blocking[..blocking.len().min(NAMED_UNVERIFIED)].join(", ")
            ))
        })
        .collect();
    if !blocked.is_empty() {
        let advice = if writable.iter().any(|item| item.truncated) {
            "raise --max-hits"
        } else {
            "re-anchor or resolve the named anchors by hand"
        };
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "refusing --update: absence of a hit is not evidence for {}; {advice}",
                blocked.join(", ")
            ),
        ));
    }

    let mut ops = Vec::new();
    for item in writable {
        let id = item.id.as_str();
        let listed: &[&str] = current
            .get(id)
            .map(|(_, listed)| listed.as_slice())
            .unwrap_or_default();
        let retained: BTreeSet<&str> = item
            .kept
            .iter()
            .map(String::as_str)
            .chain(
                item.unverified
                    .iter()
                    .filter(|u| u.reason == "excluded")
                    .map(|u| u.anchor.as_str()),
            )
            .collect();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let instances: Vec<&str> = listed
            .iter()
            .copied()
            .filter(|entry| retained.contains(entry) && seen.insert(entry))
            .chain(item.new.iter().map(String::as_str))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TaggedError;
    use crate::test_support::{git, git_available, with_root, write};

    const LEDGER: &str = "review-ledger.toml";

    /// `R1` anchors `src/a.rs:alpha` through `file`/`symbol`, keeps a line
    /// anchor in `src/b.rs`, has lost its site in `src/c.rs` and its symbol
    /// in `src/d.rs`; `src/e.rs` is an unrecorded site. The ledger sits in
    /// the repo root, where the engine would enumerate it.
    fn seed(root: &Path) -> (std::path::PathBuf, TomlValue) {
        git(root, &["init", "-q"]);
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

    fn run(root: &Path, ids: &[&str], opts: &SweepOptions) -> (SweepOutcome, TomlValue) {
        let (ledger, doc) = seed(root);
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        (items_sweep(&doc, &ledger, root, &ids, opts).unwrap(), doc)
    }

    /// Every item, a ledger path that is not on disk.
    fn sweep_all(root: &Path, doc: &TomlValue, opts: &SweepOptions) -> SweepOutcome {
        items_sweep(doc, &root.join("x.toml"), root, &[], opts).unwrap()
    }

    fn strs(v: &JsonValue) -> Vec<&str> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect()
    }

    fn pairs(list: &[AnchorReason]) -> Vec<(&str, &str)> {
        list.iter().map(|r| (r.anchor.as_str(), r.reason)).collect()
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
            let (outcome, doc) = run(root, &[], &SweepOptions::default());
            let out = outcome_json(&outcome);
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
            assert_eq!(r1["unverified"], json!([]));
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

            let plan = update_plan(&doc, &outcome).unwrap();
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
            git(root, &["init", "-q"]);
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
            let out = sweep_all(root, &doc, &SweepOptions::default());
            assert_eq!(out.items[0].new, ["a.rs:1"]);
            assert_eq!(out.items[1].new, ["a.rs:1"]);
        });
    }

    #[test]
    fn explicit_ids_report_unknown_and_no_sweep() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            let (outcome, _) = run(root, &["R2", "R9", "R2"], &SweepOptions::default());
            let out = outcome_json(&outcome);
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
            git(root, &["init", "-q"]);
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
            let out = sweep_all(root, &doc, &SweepOptions::default());
            let r1 = &out.items[0];
            assert_eq!(
                pairs(&r1.unverified),
                [("blob.rs:3", "skipped"), ("nope.rs:1", "missing")]
            );
            assert_eq!(r1.kept, ["ok.rs:1"]);
            assert!(r1.gone.is_empty());
            assert!(!r1.coverage_complete);

            let err = update_plan(&doc, &out).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            let msg = err.to_string();
            assert!(
                msg.contains("R1 (2 unverified: blob.rs:3 skipped, nope.rs:1 missing)"),
                "{msg}"
            );
            assert!(
                msg.ends_with("re-anchor or resolve the named anchors by hand"),
                "{msg}"
            );
        });
    }

    #[test]
    fn an_unparseable_or_outside_instance_is_unverified_with_its_reason() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(root, "ok.rs", b"needle\n");
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
instances = ["bare", "../up.rs:1", "ok.rs:1"]
sweep = ["needle"]
"#,
            )
            .unwrap();
            let out = sweep_all(root, &doc, &SweepOptions::default());
            assert_eq!(
                pairs(&out.items[0].unverified),
                [("bare", "unparseable"), ("../up.rs:1", "outside-repo")]
            );
        });
    }

    /// The refusal names a bounded prefix of the blocking anchors and keeps
    /// the full count.
    #[test]
    fn the_refusal_names_at_most_five_anchors() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(root, "ok.rs", b"needle\n");
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
instances = ["a.rs:1", "b.rs:1", "c.rs:1", "d.rs:1", "e.rs:1", "f.rs:1"]
sweep = ["needle"]
"#,
            )
            .unwrap();
            let out = sweep_all(root, &doc, &SweepOptions::default());
            let err = update_plan(&doc, &out).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("R1 (6 unverified: a.rs:1 missing, b.rs:1 missing, c.rs:1 missing, d.rs:1 missing, e.rs:1 missing, ...)"),
                "{msg}"
            );
            assert!(!msg.contains("f.rs"), "{msg}");
        });
    }

    /// `gone` needs the engine's own word that the file was searched: a
    /// no-hit anchor on an oversize or excluded file is still `unverified`.
    #[test]
    fn a_missing_hit_is_gone_only_on_a_file_the_engine_scanned() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(root, "small.rs", b"fn a() {}\n");
            write(root, "big.rs", b"fn a() {}\nfn b() {}\n");
            write(root, "docs/plans/p.md", b"x\n");
            write(root, "hit.rs", b"needle\n");
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
instances = ["small.rs:1", "big.rs:1", "docs/plans/p.md:1", "hit.rs:1"]
sweep = ["needle"]
"#,
            )
            .unwrap();
            let opts = SweepOptions {
                max_file_bytes: 12,
                ..SweepOptions::default()
            };
            let out = sweep_all(root, &doc, &opts);
            let r1 = &out.items[0];
            assert_eq!(pairs(&r1.gone), [("small.rs:1", "no-hit")]);
            assert_eq!(
                pairs(&r1.unverified),
                [("big.rs:1", "skipped"), ("docs/plans/p.md:1", "excluded")]
            );
            assert_eq!(r1.kept, ["hit.rs:1"]);
            assert!(!r1.coverage_complete);
        });
    }

    /// The engine never enumerates an excluded file, so its anchor can only
    /// survive by being carried over in place.
    #[test]
    fn an_excluded_anchor_is_retained_in_place_and_does_not_block_update() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(root, "docs/plans/p.md", b"needle\n");
            write(root, "hit.rs", b"needle\nneedle\n");
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
instances = ["docs/plans/p.md:1", "hit.rs:1"]
sweep = ["needle"]
"#,
            )
            .unwrap();
            let out = sweep_all(root, &doc, &SweepOptions::default());
            let r1 = &out.items[0];
            assert_eq!(pairs(&r1.unverified), [("docs/plans/p.md:1", "excluded")]);
            assert!(!r1.coverage_complete);

            let plan = update_plan(&doc, &out).unwrap();
            assert_eq!(plan.updated, ["R1"]);
            assert_eq!(
                string_array(plan.new_doc["items"][0].as_table().unwrap(), "instances"),
                ["docs/plans/p.md:1", "hit.rs:1", "hit.rs:2"]
            );
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
            let r1 = &out.items[0];
            assert!(r1.truncated);
            assert!(r1.gone.is_empty());
            assert!(
                pairs(&r1.unverified).contains(&("src/c.rs:1", "truncated")),
                "{:?}",
                pairs(&r1.unverified)
            );
            let err = update_plan(&doc, &out).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            let msg = err.to_string();
            assert!(msg.contains("R1 (truncated)"), "{msg}");
            assert!(msg.ends_with("raise --max-hits"), "{msg}");
        });
    }

    /// A terminal item is still swept and reported, but `--update` leaves
    /// its `instances` alone and its unverified anchors do not block the
    /// open items' write.
    #[test]
    fn a_terminal_item_is_reported_but_never_rewritten() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(root, "a.rs", b"fn a() {}\n");
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
status = "open"
instances = ["a.rs:1"]
sweep = ["absent"]

[[items]]
id = "R2"
status = "fixed"
instances = ["a.rs:1", "nope.rs:1"]
sweep = ["absent"]
"#,
            )
            .unwrap();
            let out = sweep_all(root, &doc, &SweepOptions::default());
            assert_eq!(pairs(&out.items[1].gone), [("a.rs:1", "no-hit")]);
            assert_eq!(pairs(&out.items[1].unverified), [("nope.rs:1", "missing")]);

            let plan = update_plan(&doc, &out).unwrap();
            assert_eq!(plan.updated, ["R1"]);
            let items = items_array(&plan.new_doc, "items");
            assert!(items[0].get("instances").is_none(), "{:?}", items[0]);
            assert_eq!(items[1], doc["items"][1]);

            let named = ["R2".to_string()];
            let out = items_sweep(
                &doc,
                &root.join("x.toml"),
                root,
                &named,
                &SweepOptions::default(),
            )
            .unwrap();
            let plan = update_plan(&doc, &out).unwrap();
            assert_eq!(plan.updated, [] as [&str; 0]);
        });
    }

    #[test]
    fn a_symbol_anchor_covers_its_block_but_not_the_next() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(
                root,
                "a.rs",
                b"fn alpha() {\n    needle();\n}\n\nfn beta() {\n    needle();\n}\n",
            );
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
instances = ["a.rs:alpha"]
sweep = ["needle"]
"#,
            )
            .unwrap();
            let r1 = &sweep_all(root, &doc, &SweepOptions::default()).items[0];
            assert_eq!(r1.kept, ["a.rs:alpha"]);
            assert_eq!(r1.new, ["a.rs:6"]);
        });
    }

    #[test]
    fn a_nested_block_ends_at_its_own_closing_bracket() {
        if !git_available() {
            return;
        }
        with_root(|root| {
            git(root, &["init", "-q"]);
            write(
                root,
                "a.rs",
                b"impl S {\n    fn alpha(&self) {\n        needle();\n    }\n    fn beta(&self) {\n        needle();\n    }\n}\n",
            );
            let doc: TomlValue = toml::from_str(
                r#"
[[items]]
id = "R1"
instances = ["a.rs:alpha"]
sweep = ["needle"]
"#,
            )
            .unwrap();
            let r1 = &sweep_all(root, &doc, &SweepOptions::default()).items[0];
            assert_eq!(r1.kept, ["a.rs:alpha"]);
            assert_eq!(r1.new, ["a.rs:6"]);
        });
    }

    #[test]
    fn block_end_stops_at_the_next_shallower_line_that_is_not_a_closer() {
        let scanned = |bytes: &[u8]| ScannedFile {
            bytes: bytes.to_vec(),
            newlines: memchr::memchr_iter(b'\n', bytes).collect(),
        };
        assert_eq!(block_end(&scanned(b"x(\n  a\n);\ny\n"), 1), 3);
        assert_eq!(block_end(&scanned(b"x\n\n  a\n  b"), 1), 4);
        assert_eq!(block_end(&scanned(b"# H\ntext\n"), 1), 1);
        assert_eq!(block_end(&scanned(b"\tx\n\t\ty\n\tz\n"), 1), 2);
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
            assert_eq!(first.items[0].kept, ["src/a.rs:alpha", "src/b.rs:1"]);
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
            git(root, &["init", "-q"]);
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
            let out = sweep_all(root, &doc, &SweepOptions::default());
            let plan = update_plan(&doc, &out).unwrap();
            assert_eq!(plan.updated, ["R1"]);
            let items = items_array(&plan.new_doc, "items");
            assert!(items[0].get("instances").is_none(), "{:?}", items[0]);
            assert_eq!(items[1], doc["items"][1]);
        });
    }
}
