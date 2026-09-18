//! `items orphans`: ledger rows whose `file`, `symbol`, `instances` or
//! `depends_on` no longer resolve.
//!
//! Reports six orphan classes:
//!   - `missing-file`     — ledger `file` points at a non-existent path
//!   - `symbol-missing`   — file exists but does not contain the `symbol`
//!   - `io-error`         — file exists but cannot be read
//!   - `outside-repo`     — `file` (relative via `..` or absolute) escapes the repo root
//!   - `dangling-dep`     — `depends_on` names an id not in the ledger
//!   - `instance-missing` — an `instances` anchor does not resolve; `reason` is one of
//!     `missing-file`, `symbol-missing`, `io-error`, `outside-repo`, `unparseable`

use anyhow::Result;
use regex::Regex;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

use crate::anchor::{self, AnchorAt};
use crate::convert::str_field;
use crate::io::{item_id, items_array, repo_or_cwd_root};

/// Resolves a ledger path against the repo root and answers, in order,
/// whether it is contained, exists, is readable and holds `symbol`. Every
/// filesystem touch is cached per resolved path, so the `file` check and the
/// `instances` walk naming one file share one `canonicalize`, `exists` and read.
struct FileProbe {
    root: PathBuf,
    // The root is process-invariant, so `canonicalize` is hoisted out of the
    // per-item loop. Falling back to the un-canonicalised root when it fails
    // keeps containment checked against something.
    canonical_root: Option<PathBuf>,
    // `(exists, contained)` per unique resolved path.
    path_cache: HashMap<PathBuf, (bool, bool)>,
    // Holds `Result<String, io::ErrorKind>` rather than
    // `Result<String, io::Error>` because `io::Error` is not `Clone`; the
    // caller only inspects success/failure to choose between `symbol-missing`
    // and `io-error`, so kind-only round-tripping preserves behaviour.
    read_cache: HashMap<PathBuf, Result<String, std::io::ErrorKind>>,
    // Compiled word-boundary regexes keyed on the raw symbol string. `None` is
    // cached for symbols whose regex fails to compile, so the substring
    // fallback reuses it without re-attempting compilation.
    symbol_cache: HashMap<String, Option<Regex>>,
}

impl FileProbe {
    fn new(root: PathBuf) -> Self {
        let canonical_root = root.canonicalize().ok();
        Self {
            root,
            canonical_root,
            path_cache: HashMap::new(),
            read_cache: HashMap::new(),
            symbol_cache: HashMap::new(),
        }
    }

    /// The first failing check wins: `outside-repo`, `missing-file`,
    /// `io-error`, then `symbol-missing`. `None` when everything resolves; a
    /// `None` symbol stops after the existence check.
    fn check(&mut self, file: &str, symbol: Option<&str>) -> Option<&'static str> {
        let resolved = resolve_relative_to_root(&self.root, file);
        // A ledger-item path is attacker-controllable: the ledger author is
        // not always the tool operator, and a crafted ledger can arrive by
        // any supply-chain path. Unchecked, a relative path escaping the root
        // via `..` (`../../etc/passwd`) or an absolute one (`/etc/shadow`,
        // `~/.ssh/id_rsa`) turns `fs::read_to_string` into an
        // existence/symbol-presence oracle over arbitrary host files. Both
        // forms are canonicalised and must satisfy
        // `starts_with(canonical_root)`; anything else surfaces as
        // `outside-repo` with no `exists()` or `read_to_string` call.
        let (exists, contained) = if let Some(hit) = self.path_cache.get(&resolved) {
            *hit
        } else {
            let contained = match (resolved.canonicalize().ok(), self.canonical_root.as_ref()) {
                (Some(c), Some(r)) => c.starts_with(r),
                (Some(c), None) => c.starts_with(&self.root),
                (None, _) => true, // missing target falls through to `missing-file`.
            };
            let exists = resolved.exists();
            self.path_cache
                .insert(resolved.clone(), (exists, contained));
            (exists, contained)
        };
        if !contained {
            return Some("outside-repo");
        }
        if !exists {
            return Some("missing-file");
        }
        let symbol = symbol?;
        // IO errors surface as an `io-error` orphan rather than being treated
        // as an empty file, which would fire `symbol-missing` spuriously for
        // unreadable-but-existing files.
        let cached = self
            .read_cache
            .entry(resolved.clone())
            .or_insert_with(|| fs::read_to_string(&resolved).map_err(|e| e.kind()));
        let Ok(contents) = cached else {
            return Some("io-error");
        };
        // Word-boundary match: a bare `contents.contains` reports a renamed
        // `id` symbol as still present in any file containing `valid`,
        // `paid`, or `lived`. The substring fallback is defensive only —
        // `regex::escape` should make it unreachable. `(?-u:\b)` pins ASCII
        // semantics regardless of crate feature flags.
        let compiled = self
            .symbol_cache
            .entry(symbol.to_string())
            .or_insert_with(|| {
                let pat = format!(r"(?-u:\b){}(?-u:\b)", regex::escape(symbol));
                Regex::new(&pat).ok()
            });
        let present = match compiled {
            Some(re) => re.is_match(contents),
            None => contents.contains(symbol),
        };
        if present {
            None
        } else {
            Some("symbol-missing")
        }
    }
}

pub(crate) fn items_orphans(doc: &TomlValue) -> Result<Vec<JsonValue>> {
    // `items_array` yields an empty slice when the array is missing, so an
    // absent ledger produces zero orphans rather than an error.
    let items = items_array(doc, "items");

    // Build set of known IDs for dangling-dep check.
    // `items.len()` is an upper bound on the number of distinct ids.
    let mut known_ids: HashSet<String> = HashSet::with_capacity(items.len());
    for item in items {
        if let Some(id) = item_id(item) {
            known_ids.insert(id.to_string());
        }
    }

    let mut probe = FileProbe::new(repo_or_cwd_root()?);

    let mut out = Vec::new();
    for item in items {
        let Some(tbl) = item.as_table() else { continue };
        let id = str_field(tbl, "id");
        let file = str_field(tbl, "file");
        let symbol = str_field(tbl, "symbol");

        // missing-file / symbol-missing classes (mutually exclusive: the first
        // failing check wins).
        if !file.is_empty() {
            let symbol = (!symbol.is_empty()).then_some(symbol);
            if let Some(class) = probe.check(file, symbol) {
                let mut obj = serde_json::Map::new();
                obj.insert("id".into(), JsonValue::String(id.into()));
                obj.insert("class".into(), JsonValue::String(class.into()));
                obj.insert("file".into(), JsonValue::String(file.into()));
                if let ("symbol-missing", Some(symbol)) = (class, symbol) {
                    obj.insert("symbol".into(), JsonValue::String(symbol.into()));
                }
                out.push(JsonValue::Object(obj));
            }
        }

        // dangling-dep class (independent of the file/symbol axis; an item can
        // be orphaned in both ways and will surface twice).
        if let Some(deps) = tbl.get("depends_on").and_then(|v| v.as_array()) {
            let mut missing: Vec<String> = Vec::new();
            for dep in deps {
                if let Some(d) = dep.as_str()
                    && !known_ids.contains(d)
                {
                    missing.push(d.to_string());
                }
            }
            if !missing.is_empty() {
                let mut obj = serde_json::Map::new();
                obj.insert("id".into(), JsonValue::String(id.into()));
                obj.insert("class".into(), JsonValue::String("dangling-dep".into()));
                obj.insert(
                    "dangling_deps".into(),
                    JsonValue::Array(missing.into_iter().map(JsonValue::String).collect()),
                );
                out.push(JsonValue::Object(obj));
            }
        }

        // instance-missing class: one row per unresolvable anchor, in array
        // order. Line anchors are checked for existence only.
        if let Some(instances) = tbl.get("instances").and_then(|v| v.as_array()) {
            for instance in instances.iter().filter_map(|v| v.as_str()) {
                let reason = match anchor::parse(instance) {
                    None => Some("unparseable"),
                    Some(a) => match &a.at {
                        AnchorAt::Line(_) => probe.check(&a.file, None),
                        AnchorAt::Symbol(symbol) => probe.check(&a.file, Some(symbol)),
                    },
                };
                if let Some(reason) = reason {
                    let mut obj = serde_json::Map::new();
                    obj.insert("id".into(), JsonValue::String(id.into()));
                    obj.insert("class".into(), JsonValue::String("instance-missing".into()));
                    obj.insert("instance".into(), JsonValue::String(instance.into()));
                    obj.insert("reason".into(), JsonValue::String(reason.into()));
                    out.push(JsonValue::Object(obj));
                }
            }
        }
    }
    Ok(out)
}

fn resolve_relative_to_root(root: &Path, file: &str) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_root;

    #[test]
    fn items_orphans_reports_missing_file_symbol_and_dangling_dep() {
        // Absolute `file` fields get the same containment check as relative
        // ones, so the repo root is pinned to the sandbox — otherwise the
        // absolute `/tmp/.../real.rs` paths would (correctly) surface as
        // `outside-repo`.
        let orphans = with_root(|root| {
            // Create a real source file that contains a specific symbol.
            let real_file = root.join("real.rs");
            fs::write(&real_file, "pub fn present_symbol() {}\n").unwrap();

            let ledger = format!(
                r#"
[[items]]
id = "R1"
file = '{}'
symbol = "present_symbol"
summary = "valid"

[[items]]
id = "R2"
file = '{}'
symbol = "missing_symbol"
summary = "sym gone"

[[items]]
id = "R3"
file = '{}/nope.rs'
summary = "file gone"

[[items]]
id = "R4"
depends_on = ["R99", "R1"]
summary = "dangling dep"

[[items]]
id = "R5"
instances = ['{real}:present_symbol', '{root}/missing/x.rs:12', '{real}:no_such_symbol']
summary = "instances"
"#,
                real_file.display(),
                real_file.display(),
                root.display(),
                real = real_file.display(),
                root = root.display()
            );
            let doc: TomlValue = toml::from_str(&ledger).unwrap();
            items_orphans(&doc).unwrap()
        });
        // Expect three orphan records: symbol-missing, missing-file, dangling-dep.
        let classes: Vec<(&str, &str)> = orphans
            .iter()
            .map(|o| {
                (
                    o.get("id").and_then(|v| v.as_str()).unwrap(),
                    o.get("class").and_then(|v| v.as_str()).unwrap(),
                )
            })
            .collect();
        assert!(classes.contains(&("R2", "symbol-missing")), "{classes:?}");
        assert!(classes.contains(&("R3", "missing-file")), "{classes:?}");
        assert!(classes.contains(&("R4", "dangling-dep")), "{classes:?}");
        // The fully-valid row yields no orphan entry.
        assert!(classes.iter().all(|(id, _)| *id != "R1"));
        // dangling-dep names only the missing ids.
        let r4 = orphans
            .iter()
            .find(|o| o.get("id").and_then(|v| v.as_str()) == Some("R4"))
            .unwrap();
        let deps = r4.get("dangling_deps").and_then(|v| v.as_array()).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0], "R99");
        // instance-missing rows follow the `instances` array order, with the
        // resolving anchor producing nothing.
        let r5: Vec<(&str, &str)> = orphans
            .iter()
            .filter(|o| o.get("id").and_then(|v| v.as_str()) == Some("R5"))
            .map(|o| {
                assert_eq!(
                    o.get("class").and_then(|v| v.as_str()),
                    Some("instance-missing")
                );
                (
                    o.get("instance").and_then(|v| v.as_str()).unwrap(),
                    o.get("reason").and_then(|v| v.as_str()).unwrap(),
                )
            })
            .collect();
        assert_eq!(r5.len(), 2, "{r5:?}");
        assert!(r5[0].0.ends_with("missing/x.rs:12"), "{r5:?}");
        assert_eq!(r5[0].1, "missing-file");
        assert!(r5[1].0.ends_with("real.rs:no_such_symbol"), "{r5:?}");
        assert_eq!(r5[1].1, "symbol-missing");
    }

    /// Absolute-path ledger rows pointing OUTSIDE the repo root must surface
    /// as `outside-repo` rather than triggering an
    /// existence/symbol-presence oracle against arbitrary host files. Pins
    /// the root to one tempdir, then feeds a ledger row whose `file` points
    /// at a sibling tempdir (known-to-exist, outside the pinned root).
    #[test]
    fn items_orphans_absolute_path_outside_root_is_outside_repo() {
        let orphans = with_root(|_root| {
            // The "oracle target" lives in a separate tempdir so it exists on
            // disk but sits outside the pinned root.
            let oracle_dir = tempfile::tempdir().unwrap();
            let oracle_file = oracle_dir.path().canonicalize().unwrap().join("secret.rs");
            fs::write(&oracle_file, "pub fn leak_me() {}\n").unwrap();
            let ledger = format!(
                r#"
[[items]]
id = "R28-probe"
file = '{}'
symbol = "leak_me"
summary = "oracle attempt"
"#,
                oracle_file.display()
            );
            let doc: TomlValue = toml::from_str(&ledger).unwrap();
            items_orphans(&doc).unwrap()
        });
        // The file DOES exist and the symbol IS present, so an implementation
        // without the containment check emits zero orphans and silently reads
        // the file. The row must instead surface as `outside-repo`, with
        // neither `exists()` nor `read_to_string` able to leak information
        // about the target.
        assert_eq!(orphans.len(), 1, "{orphans:?}");
        assert_eq!(
            orphans[0].get("class").and_then(|v| v.as_str()),
            Some("outside-repo"),
            "{orphans:?}"
        );
        assert_eq!(
            orphans[0].get("id").and_then(|v| v.as_str()),
            Some("R28-probe"),
        );
    }
}
