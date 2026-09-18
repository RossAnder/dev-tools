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
use regex::bytes::Regex;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use toml::Value as TomlValue;

use crate::anchor::{self, AnchorAt};
use crate::convert::str_field;
use crate::io::{item_id, items_array, join_under, repo_or_cwd_root};

/// Resolves a ledger path against the repo root and answers, in order,
/// whether it is contained, exists, is readable and holds `symbol`. Every
/// filesystem touch is cached per resolved path, so the `file` check and the
/// `instances` walk naming one file share one `exists` and read.
struct FileProbe {
    root: PathBuf,
    exists_cache: HashMap<PathBuf, bool>,
    // Holds `Result<_, io::ErrorKind>` rather than `Result<_, io::Error>`
    // because `io::Error` is not `Clone`; the caller only inspects
    // success/failure to choose between `symbol-missing` and `io-error`.
    read_cache: HashMap<PathBuf, Result<Vec<u8>, std::io::ErrorKind>>,
    // Keyed on the raw symbol string; `None` is cached for a symbol whose
    // regex fails to compile, so compilation is not re-attempted.
    symbol_cache: HashMap<String, Option<Regex>>,
}

impl FileProbe {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            exists_cache: HashMap::new(),
            read_cache: HashMap::new(),
            symbol_cache: HashMap::new(),
        }
    }

    /// The first failing check wins: `outside-repo`, `missing-file`,
    /// `io-error`, then `symbol-missing`. `None` when everything resolves; a
    /// `None` symbol stops after the existence check.
    fn check(&mut self, file: &str, symbol: Option<&str>) -> Option<&'static str> {
        // A ledger-item path is attacker-controllable: the ledger author is
        // not always the tool operator, and a crafted ledger can arrive by
        // any supply-chain path. Unchecked, a relative path escaping the root
        // via `..` (`../../etc/passwd`), an absolute one (`/etc/shadow`,
        // `~/.ssh/id_rsa`) or a UNC one (`\\host\share\x`, which opens SMB)
        // turns `exists` and the read into an existence/symbol-presence
        // oracle over arbitrary host files. `join_under` decides containment
        // lexically, so `outside-repo` is reported before the disk is
        // consulted and every later verdict names a path under the root.
        let Some(resolved) = join_under(&self.root, file) else {
            return Some("outside-repo");
        };
        let exists = *self
            .exists_cache
            .entry(resolved.clone())
            .or_insert_with(|| resolved.exists());
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
            .or_insert_with(|| fs::read(&resolved).map_err(|e| e.kind()));
        let Ok(contents) = cached else {
            return Some("io-error");
        };
        let compiled = self
            .symbol_cache
            .entry(symbol.to_string())
            .or_insert_with(|| anchor::symbol_regex(symbol));
        match compiled {
            Some(re) if re.is_match(contents) => None,
            _ => Some("symbol-missing"),
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
instances = ['{real}:present_symbol', '{root}/missing/x.rs:12', '{real}:no_such_symbol', 'src/a.rs']
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
        assert_eq!(r5.len(), 3, "{r5:?}");
        assert!(r5[0].0.ends_with("missing/x.rs:12"), "{r5:?}");
        assert_eq!(r5[0].1, "missing-file");
        assert!(r5[1].0.ends_with("real.rs:no_such_symbol"), "{r5:?}");
        assert_eq!(r5[1].1, "symbol-missing");
        assert_eq!(r5[2], ("src/a.rs", "unparseable"));
    }

    /// Ledger rows pointing OUTSIDE the repo root must surface as
    /// `outside-repo` rather than triggering an existence/symbol-presence
    /// oracle against arbitrary host files: the verdict has to be the same
    /// whether the target exists or not, and the same for the `file` field
    /// and an `instances` anchor. Pins the root to one tempdir, then feeds
    /// rows whose paths point at a sibling tempdir — one file there exists,
    /// one does not — and a `..` escape.
    #[test]
    fn items_orphans_path_outside_root_is_outside_repo_whether_or_not_it_exists() {
        let orphans = with_root(|_root| {
            let oracle_dir = tempfile::tempdir().unwrap();
            let oracle_dir = oracle_dir.path().canonicalize().unwrap();
            let present = oracle_dir.join("secret.rs");
            fs::write(&present, "pub fn leak_me() {}\n").unwrap();
            let absent = oracle_dir.join("absent.rs");
            let ledger = format!(
                r#"
[[items]]
id = "present"
file = '{present}'
symbol = "leak_me"
instances = ['{present}:leak_me', '{absent}:1']
summary = "oracle attempt"

[[items]]
id = "absent"
file = '{absent}'
summary = "oracle attempt"

[[items]]
id = "climb"
file = '../escape.rs'
summary = "oracle attempt"
"#,
                present = present.display(),
                absent = absent.display(),
            );
            let doc: TomlValue = toml::from_str(&ledger).unwrap();
            items_orphans(&doc).unwrap()
        });
        // A disk-consulting check answers differently for the present and
        // the absent target (no orphan, or `missing-file`), which is the
        // leak. Every row must instead read `outside-repo`.
        let verdicts: Vec<(&str, &str)> = orphans
            .iter()
            .map(|o| {
                let id = o.get("id").and_then(|v| v.as_str()).unwrap();
                let class = o.get("class").and_then(|v| v.as_str()).unwrap();
                let verdict = match class {
                    "instance-missing" => o.get("reason").and_then(|v| v.as_str()).unwrap(),
                    _ => class,
                };
                (id, verdict)
            })
            .collect();
        assert_eq!(
            verdicts,
            [
                ("present", "outside-repo"),
                ("present", "outside-repo"),
                ("present", "outside-repo"),
                ("absent", "outside-repo"),
                ("climb", "outside-repo"),
            ],
            "{orphans:?}"
        );
    }
}
