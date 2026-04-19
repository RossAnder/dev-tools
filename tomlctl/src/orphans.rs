//! R62: `items orphans` logic split out of `main.rs`.
//!
//! Reports three orphan classes:
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
    // R44: items_array returns an empty slice when missing, so the early-return
    // on error disappears — an empty ledger naturally produces zero orphans.
    let items = items_array(doc, "items");

    // Build set of known IDs for dangling-dep check.
    // O28 freebie: pre-size to items.len() — upper bound on distinct ids.
    let mut known_ids: HashSet<String> = HashSet::with_capacity(items.len());
    for item in items {
        if let Some(id) = item_id(item) {
            known_ids.insert(id.to_string());
        }
    }

    let root = repo_or_cwd_root()?;
    // O42: hoist `root.canonicalize()` out of the per-item loop. The root is
    // process-invariant; canonicalising it once per call removes a syscall
    // per item. Fall back to the un-canonicalised root if canonicalize fails
    // (matches the pre-O42 (Some(c), None) arm).
    let canonical_root: Option<PathBuf> = root.canonicalize().ok();
    // O42: cache `(exists, contained)` per unique resolved path so repeated
    // ledger entries pointing at the same file each cost one `canonicalize` +
    // one `exists` regardless of how many items reference them.
    let mut path_cache: HashMap<PathBuf, (bool, bool)> = HashMap::new();
    // O28: sibling cache so `fs::read_to_string` runs at most once per unique
    // resolved path. We store `Result<String, io::ErrorKind>` rather than
    // `Result<String, io::Error>` because `io::Error` is not `Clone`; the
    // existing call site only inspects success/failure to choose between
    // `symbol-missing` and `io-error`, so kind-only round-tripping preserves
    // behaviour. Same key (`PathBuf`) as the path_cache.
    let mut read_cache: HashMap<PathBuf, Result<String, std::io::ErrorKind>> = HashMap::new();
    // O29: per-call cache of compiled word-boundary regexes keyed on the raw
    // symbol string. Compiling once per distinct symbol keeps the cost flat
    // even when the same symbol recurs across many ledger entries. `None` is
    // cached for symbols whose regex compilation fails so we fall back to the
    // legacy `contents.contains` substring check on every reuse without
    // re-attempting compilation.
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
            // R38: a RELATIVE ledger-item `file` field that escapes the root
            // via `..` (e.g. `../../etc/passwd`) turns `fs::read_to_string`
            // into an existence/symbol-presence oracle on arbitrary host
            // paths. Canonicalise and assert containment for relative inputs.
            //
            // R28: the same oracle exists for ABSOLUTE ledger-item `file`
            // values (e.g. `/etc/shadow`, `~/.ssh/id_rsa`) — the ledger
            // author is not always the tool operator (a crafted
            // review-ledger.toml can arrive via any supply-chain path), so
            // absolute paths must be subjected to the same containment
            // check. Mirroring the relative-branch idiom: canonicalise,
            // require `starts_with(canonical_root)`, otherwise surface as
            // `outside-repo` and skip `exists()` / `read_to_string`. Does
            // not gate behind a new `--allow-outside` flag — that'd widen
            // the public API for a closed-class hardening fix.
            // O42: probe cache; on miss compute (exists, contained) and insert.
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
                // R27: explicit match — IO errors surface as an `io-error`
                // orphan instead of silently treating the file as empty
                // (which would fire `symbol-missing` spuriously for
                // unreadable-but-existing files).
                // O28: probe read_cache; populate on miss so duplicate ledger
                // entries pointing at the same file each pay one read.
                let cached = read_cache.entry(resolved.clone()).or_insert_with(|| {
                    fs::read_to_string(&resolved).map_err(|e| e.kind())
                });
                match cached {
                    Ok(contents) => {
                        // O29: word-boundary match. The previous
                        // `contents.contains(symbol)` produced false-positives
                        // when `symbol` appeared as a substring of an unrelated
                        // identifier, comment, or string literal — a freshly
                        // renamed `id` symbol would still appear "present" in
                        // any file containing words like `valid`, `paid`, or
                        // `lived`. Compile once per distinct symbol, cache the
                        // Regex, and fall back to the legacy substring check
                        // when compilation fails (defensive — `regex::escape`
                        // should make this unreachable). `(?-u:\b)` pins ASCII
                        // semantics regardless of crate feature flags.
                        let compiled = symbol_cache
                            .entry(symbol.to_string())
                            .or_insert_with(|| {
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
    if p.is_absolute() { p.to_path_buf() } else { root.join(p) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;

    #[test]
    fn items_orphans_reports_missing_file_symbol_and_dangling_dep() {
        // R28: absolute `file` fields now get the same containment check as
        // relative ones, so we pin the repo root to the tempdir via
        // `TOMLCTL_ROOT` — otherwise the absolute `/tmp/.../real.rs` paths
        // would (correctly) surface as `outside-repo`. Hold `env_lock()`
        // since we mutate process env.
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        // SAFETY: set_var is unsafe in edition 2024; acceptable inside tests
        // where we hold the env lock.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", canonical_dir.as_os_str());
        }
        // Create a real source file that contains a specific symbol.
        let real_file = canonical_dir.join("real.rs");
        fs::write(&real_file, "pub fn present_symbol() {}\n").unwrap();

        let ledger = format!(
            r#"
[[items]]
id = "R1"
file = "{}"
symbol = "present_symbol"
summary = "valid"

[[items]]
id = "R2"
file = "{}"
symbol = "missing_symbol"
summary = "sym gone"

[[items]]
id = "R3"
file = "{}/nope.rs"
summary = "file gone"

[[items]]
id = "R4"
depends_on = ["R99", "R1"]
summary = "dangling dep"
"#,
            real_file.display(),
            real_file.display(),
            canonical_dir.display()
        );
        let doc: TomlValue = toml::from_str(&ledger).unwrap();
        let orphans = items_orphans(&doc).unwrap();
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
        // Expect three orphan records: R2 symbol-missing, R3 missing-file, R4 dangling-dep.
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
        // R1 is valid — no orphan entry for it.
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

    /// R28 regression: absolute-path ledger rows pointing OUTSIDE the repo
    /// root must surface as `outside-repo` rather than triggering an
    /// existence/symbol-presence oracle against arbitrary host files. Pins
    /// the root to one tempdir, then feeds a ledger row whose `file` points
    /// at a sibling tempdir (known-to-exist, outside the pinned root).
    #[test]
    fn items_orphans_absolute_path_outside_root_is_outside_repo() {
        let _guard = env_lock();
        let root_dir = tempfile::tempdir().unwrap();
        let canonical_root = root_dir.path().canonicalize().unwrap();
        // The "oracle target" lives in a separate tempdir so it exists on
        // disk but sits outside the pinned root.
        let oracle_dir = tempfile::tempdir().unwrap();
        let oracle_file = oracle_dir.path().canonicalize().unwrap().join("secret.rs");
        fs::write(&oracle_file, "pub fn leak_me() {}\n").unwrap();
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", canonical_root.as_os_str());
        }
        let ledger = format!(
            r#"
[[items]]
id = "R28-probe"
file = "{}"
symbol = "leak_me"
summary = "oracle attempt"
"#,
            oracle_file.display()
        );
        let doc: TomlValue = toml::from_str(&ledger).unwrap();
        let orphans = items_orphans(&doc).unwrap();
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
        // The file DOES exist and the symbol IS present, so the pre-R28
        // behaviour would emit zero orphans (silently reading the file).
        // Post-R28 the row surfaces as `outside-repo` and neither `exists()`
        // nor `read_to_string` get to leak information about the target.
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
