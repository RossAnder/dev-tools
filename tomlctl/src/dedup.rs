//! R62: `items find-duplicates` tiered dedup logic split out of `main.rs`.
//!
//! Depends on `items_array` (in `items.rs` or still in `main.rs` depending on
//! extraction order), and `str_field`/`i64_field` from `convert.rs` for
//! table-field pulls.

use anyhow::Result;
use clap::ValueEnum;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use toml::Value as TomlValue;

use crate::convert::{i64_field, str_field, toml_to_json};
use crate::integrity::hex_lower;
use crate::items_array;

/// Tier selector for `items find-duplicates`. Each tier has its own grouping
/// heuristic documented on the individual `find_duplicates_tier_*` functions.
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
pub(crate) enum DupTier {
    A,
    B,
    C,
}

pub(crate) fn items_find_duplicates(doc: &TomlValue, tier: DupTier) -> Result<Vec<JsonValue>> {
    // R26: tier fns take `&[TomlValue]`; no need to clone into an owned Vec.
    // R44: items_array now returns &[TomlValue] directly (empty slice when
    // missing) so the prior Err→empty fallback is gone.
    let items: &[TomlValue] = items_array(doc, "items");
    match tier {
        DupTier::A => find_duplicates_tier_a(items),
        DupTier::B => find_duplicates_tier_b(items),
        DupTier::C => find_duplicates_tier_c(items),
    }
}

fn dup_group_json(tier: &str, key: &str, items: &[&TomlValue]) -> JsonValue {
    let mut obj = serde_json::Map::new();
    obj.insert("tier".into(), JsonValue::String(tier.into()));
    obj.insert("key".into(), JsonValue::String(key.into()));
    obj.insert(
        "items".into(),
        JsonValue::Array(items.iter().map(|v| toml_to_json(v)).collect()),
    );
    JsonValue::Object(obj)
}

fn find_duplicates_tier_a(items: &[TomlValue]) -> Result<Vec<JsonValue>> {
    // Group under two key-spaces:
    //   by (file, symbol) when symbol is non-empty
    //   by (file, summary) otherwise
    // An item appears in exactly one group (either symbol-keyed or summary-keyed).
    let mut by_symbol: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    let mut by_summary: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(tbl) = item.as_table() else { continue };
        let file = str_field(tbl, "file");
        let symbol = str_field(tbl, "symbol");
        if !symbol.is_empty() {
            by_symbol
                .entry((file.to_string(), symbol.to_string()))
                .or_default()
                .push(i);
        } else {
            let summary = str_field(tbl, "summary");
            by_summary
                .entry((file.to_string(), summary.to_string()))
                .or_default()
                .push(i);
        }
    }
    let mut out = Vec::new();
    for ((file, symbol), idxs) in &by_symbol {
        if idxs.len() < 2 {
            continue;
        }
        let refs: Vec<&TomlValue> = idxs.iter().map(|&i| &items[i]).collect();
        out.push(dup_group_json(
            "A",
            &format!("file={} symbol={}", file, symbol),
            &refs,
        ));
    }
    for ((file, summary), idxs) in &by_summary {
        if idxs.len() < 2 {
            continue;
        }
        let refs: Vec<&TomlValue> = idxs.iter().map(|&i| &items[i]).collect();
        out.push(dup_group_json(
            "A",
            &format!("file={} summary={}", file, summary),
            &refs,
        ));
    }
    Ok(out)
}

fn find_duplicates_tier_b(items: &[TomlValue]) -> Result<Vec<JsonValue>> {
    let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(tbl) = item.as_table() else { continue };
        let file = str_field(tbl, "file");
        let summary = str_field(tbl, "summary");
        let severity = str_field(tbl, "severity");
        let category = str_field(tbl, "category");
        let symbol = str_field(tbl, "symbol");
        let canonical = format!(
            "{}|{}|{}|{}|{}",
            file, summary, severity, category, symbol
        );
        let full = hex_lower(&Sha256::digest(canonical.as_bytes()));
        let short = full[..16].to_string();
        let basename = Path::new(file)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.to_string());
        groups.entry((short, basename)).or_default().push(i);
    }
    let mut out = Vec::new();
    for ((fingerprint, basename), idxs) in &groups {
        if idxs.len() < 2 {
            continue;
        }
        let refs: Vec<&TomlValue> = idxs.iter().map(|&i| &items[i]).collect();
        out.push(dup_group_json(
            "B",
            &format!(
                "suggestion fingerprint={} basename={}",
                fingerprint, basename
            ),
            &refs,
        ));
    }
    Ok(out)
}

fn find_duplicates_tier_c(items: &[TomlValue]) -> Result<Vec<JsonValue>> {
    // Candidates: items with empty/missing symbol AND line > 0.
    #[derive(Clone)]
    struct Candidate {
        idx: usize,
        file: String,
        line: i64,
    }
    let mut by_file: HashMap<String, Vec<Candidate>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(tbl) = item.as_table() else { continue };
        let symbol = str_field(tbl, "symbol");
        if !symbol.is_empty() {
            continue;
        }
        let line = i64_field(tbl, "line");
        if line <= 0 {
            continue;
        }
        let file = str_field(tbl, "file").to_string();
        by_file.entry(file.clone()).or_default().push(Candidate {
            idx: i,
            file,
            line,
        });
    }

    let mut out = Vec::new();
    // Deterministic order: sort file keys.
    let mut files: Vec<&String> = by_file.keys().collect();
    files.sort();
    for file in files {
        let cands = &by_file[file];
        // Sort by line then idx so sweep is deterministic.
        // R26: `to_vec()` (clippy-preferred over `iter().cloned().collect()`)
        // replaces the previous wholesale `cands.clone()` with the same
        // semantics — we still need a mutable owned Vec for in-place sort,
        // but without borrowing `by_file` mutably while iterating its keys.
        let mut sorted: Vec<Candidate> = cands.to_vec();
        sorted.sort_by(|a, b| a.line.cmp(&b.line).then(a.idx.cmp(&b.idx)));
        let n = sorted.len();
        let mut i = 0;
        while i < n {
            // Greedy: start a group at sorted[i], extend while every pair in
            // the growing group satisfies |line_a - line_b| <= 10. Because
            // items are sorted by line, the tightest constraint is between
            // the group's min-line (sorted[i]) and the candidate's line.
            let mut j = i + 1;
            while j < n && sorted[j].line - sorted[i].line <= 10 {
                j += 1;
            }
            if j - i >= 2 {
                let refs: Vec<&TomlValue> = sorted[i..j].iter().map(|c| &items[c.idx]).collect();
                out.push(dup_group_json(
                    "C",
                    &format!(
                        "file={} line_window=[{}..{}]",
                        sorted[i].file, sorted[i].line, sorted[j - 1].line
                    ),
                    &refs,
                ));
                i = j;
            } else {
                i += 1;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_duplicates_tier_a_groups_by_symbol_or_summary() {
        let src = r#"
[[items]]
id = "R1"
file = "src/a.rs"
symbol = "foo::bar"
summary = "thing 1"

[[items]]
id = "R2"
file = "src/a.rs"
symbol = "foo::bar"
summary = "thing 2"

[[items]]
id = "R3"
file = "src/b.rs"
summary = "identical summary"

[[items]]
id = "R4"
file = "src/b.rs"
summary = "identical summary"

[[items]]
id = "R5"
file = "src/c.rs"
symbol = "loner"
summary = "only one"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let groups = items_find_duplicates(&doc, DupTier::A).unwrap();
        assert_eq!(groups.len(), 2, "expected two dup groups, got {groups:?}");
        // Each group has exactly 2 items.
        for g in &groups {
            let items = g.get("items").and_then(|v| v.as_array()).unwrap();
            assert_eq!(items.len(), 2);
        }
    }

    #[test]
    fn find_duplicates_tier_c_uses_line_window() {
        let src = r#"
[[items]]
id = "R1"
file = "src/a.rs"
line = 10
summary = "x"

[[items]]
id = "R2"
file = "src/a.rs"
line = 15
summary = "y"

[[items]]
id = "R3"
file = "src/b.rs"
line = 10
summary = "z"

[[items]]
id = "R4"
file = "src/b.rs"
line = 30
summary = "w"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let groups = items_find_duplicates(&doc, DupTier::C).unwrap();
        // R1+R2 group (lines 10/15 within 10 window); R3+R4 NOT grouped (lines 10/30).
        assert_eq!(groups.len(), 1);
        let items = groups[0].get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 2);
        let ids: Vec<&str> = items
            .iter()
            .map(|i| i.get("id").and_then(|v| v.as_str()).unwrap())
            .collect();
        assert!(ids.contains(&"R1"));
        assert!(ids.contains(&"R2"));
    }

    #[test]
    fn find_duplicates_tier_b_fingerprint_suggestion() {
        // Same canonical → same short fingerprint → grouped.
        let src = r#"
[[items]]
id = "R1"
file = "src/a.rs"
summary = "dup-summary"
severity = "warning"
category = "quality"

[[items]]
id = "R2"
file = "src/a.rs"
summary = "dup-summary"
severity = "warning"
category = "quality"

[[items]]
id = "R3"
file = "src/a.rs"
summary = "different"
severity = "warning"
category = "quality"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let groups = items_find_duplicates(&doc, DupTier::B).unwrap();
        assert_eq!(groups.len(), 1);
        let key = groups[0].get("key").and_then(|v| v.as_str()).unwrap();
        assert!(key.contains("suggestion"));
    }
}
