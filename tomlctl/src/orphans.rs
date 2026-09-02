//! `items orphans`: ledger rows whose `file`, `symbol` or `depends_on` no
//! longer resolve.
//!
//! Reports five orphan classes:
//!   - `missing-file`     — ledger `file` points at a non-existent path
//!   - `symbol-missing`   — file exists but does not contain the `symbol`
//!   - `io-error`         — file exists but cannot be read
//!   - `outside-repo`     — `file` (relative via `..` or absolute) escapes the repo root
//!   - `dangling-dep`     — `depends_on` names an id not in the ledger

use anyhow::Result;
use regex::Regex;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

use crate::convert::str_field;
use crate::io::{item_id, items_array, repo_or_cwd_root};

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

    let root = repo_or_cwd_root()?;
    // The root is process-invariant, so `canonicalize` is hoisted out of the
    // per-item loop. Falling back to the un-canonicalised root when it fails
    // keeps containment checked against something.
    let canonical_root: Option<PathBuf> = root.canonicalize().ok();
    // `(exists, contained)` per unique resolved path, so repeated ledger
    // entries naming the same file cost one `canonicalize` + one `exists`
    // between them rather than one each.
    let mut path_cache: HashMap<PathBuf, (bool, bool)> = HashMap::new();
    // Sibling cache so `fs::read_to_string` runs at most once per unique
    // resolved path. Holds `Result<String, io::ErrorKind>` rather than
    // `Result<String, io::Error>` because `io::Error` is not `Clone`; the call
    // site only inspects success/failure to choose between `symbol-missing`
    // and `io-error`, so kind-only round-tripping preserves behaviour. Same
    // key (`PathBuf`) as `path_cache`.
    let mut read_cache: HashMap<PathBuf, Result<String, std::io::ErrorKind>> = HashMap::new();
    // Compiled word-boundary regexes keyed on the raw symbol string, so a
    // symbol recurring across many ledger entries compiles once. `None` is
    // cached for symbols whose regex fails to compile, so the substring
    // fallback reuses it without re-attempting compilation.
    let mut symbol_cache: HashMap<String, Option<Regex>> = HashMap::new();

    let mut out = Vec::new();
    for item in items {
        let Some(tbl) = item.as_table() else { continue };
        let id = str_field(tbl, "id");
        let file = str_field(tbl, "file");
        let symbol = str_field(tbl, "symbol");

        // missing-file / symbol-missing classes (mutually exclusive: the first
        // failing check wins).
        if !file.is_empty() {
            let resolved = resolve_relative_to_root(&root, file);
            // A ledger-item `file` field is attacker-controllable: the ledger
            // author is not always the tool operator, and a crafted ledger can
            // arrive by any supply-chain path. Unchecked, a relative path
            // escaping the root via `..` (`../../etc/passwd`) or an absolute
            // one (`/etc/shadow`, `~/.ssh/id_rsa`) turns `fs::read_to_string`
            // into an existence/symbol-presence oracle over arbitrary host
            // files. Both forms are canonicalised and must satisfy
            // `starts_with(canonical_root)`; anything else surfaces as
            // `outside-repo` with no `exists()` or `read_to_string` call.
            let (exists, contained) = if let Some(hit) = path_cache.get(&resolved) {
                *hit
            } else {
                let contained = match (resolved.canonicalize().ok(), canonical_root.as_ref()) {
                    (Some(c), Some(r)) => c.starts_with(r),
                    (Some(c), None) => c.starts_with(&root),
                    (None, _) => true, // missing target falls through to `missing-file`.
                };
                let exists = resolved.exists();
                path_cache.insert(resolved.clone(), (exists, contained));
                (exists, contained)
            };
            if !contained {
                let mut obj = serde_json::Map::new();
                obj.insert("id".into(), JsonValue::String(id.into()));
                obj.insert("class".into(), JsonValue::String("outside-repo".into()));
                obj.insert("file".into(), JsonValue::String(file.into()));
                out.push(JsonValue::Object(obj));
            } else if !exists {
                let mut obj = serde_json::Map::new();
                obj.insert("id".into(), JsonValue::String(id.into()));
                obj.insert("class".into(), JsonValue::String("missing-file".into()));
                obj.insert("file".into(), JsonValue::String(file.into()));
                out.push(JsonValue::Object(obj));
            } else if !symbol.is_empty() {
                // IO errors surface as an `io-error` orphan rather than being
                // treated as an empty file, which would fire `symbol-missing`
                // spuriously for unreadable-but-existing files.
                let cached = read_cache
                    .entry(resolved.clone())
                    .or_insert_with(|| fs::read_to_string(&resolved).map_err(|e| e.kind()));
                match cached {
                    Ok(contents) => {
                        // Word-boundary match: a bare `contents.contains`
                        // reports a renamed `id` symbol as still present in
                        // any file containing `valid`, `paid`, or `lived`.
                        // The substring fallback is defensive only —
                        // `regex::escape` should make it unreachable.
                        // `(?-u:\b)` pins ASCII semantics regardless of crate
                        // feature flags.
                        let compiled =
                            symbol_cache.entry(symbol.to_string()).or_insert_with(|| {
                                let pat = format!(r"(?-u:\b){}(?-u:\b)", regex::escape(symbol));
                                Regex::new(&pat).ok()
                            });
                        let present = match compiled {
                            Some(re) => re.is_match(contents),
                            None => contents.contains(symbol),
                        };
                        if !present {
                            let mut obj = serde_json::Map::new();
                            obj.insert("id".into(), JsonValue::String(id.into()));
                            obj.insert("class".into(), JsonValue::String("symbol-missing".into()));
                            obj.insert("file".into(), JsonValue::String(file.into()));
                            obj.insert("symbol".into(), JsonValue::String(symbol.into()));
                            out.push(JsonValue::Object(obj));
                        }
                    }
                    Err(_) => {
                        let mut obj = serde_json::Map::new();
                        obj.insert("id".into(), JsonValue::String(id.into()));
                        obj.insert("class".into(), JsonValue::String("io-error".into()));
                        obj.insert("file".into(), JsonValue::String(file.into()));
                        out.push(JsonValue::Object(obj));
                    }
                }
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
"#,
                real_file.display(),
                real_file.display(),
                root.display()
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
