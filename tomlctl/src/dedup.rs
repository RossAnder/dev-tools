//! R62: `items find-duplicates` tiered dedup logic split out of `main.rs`.
//!
//! Depends on `items_array` (in `items.rs` or still in `main.rs` depending on
//! extraction order), and `str_field`/`i64_field` from `convert.rs` for
//! table-field pulls.

use anyhow::{Result, bail};
use clap::ValueEnum;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use toml::Value as TomlValue;

use crate::convert::{i64_field, i64_field_json, str_field, str_field_json, toml_to_json};
use crate::integrity::hex_lower;
use crate::io::{items_array, items_array_json};

/// Tier selector for `items find-duplicates`. Each tier has its own grouping
/// heuristic documented on the individual `find_duplicates_tier_*` functions.
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
pub(crate) enum DupTier {
    A,
    B,
    C,
}

/// T6a: canonical fingerprinted-field list, in the order the tier-B hash
/// inlines them. Shared between `tier_b_fingerprint` (and its JSON sibling)
/// and the `items update` / `items apply` auto-populate logic, so the
/// "is this patch touching a fingerprinted field?" check in `items.rs`
/// stays pinned to the exact same set the fingerprint hashes.
///
/// Order matches the pre-extraction inline hashing order at tier-B:
/// `file | summary | severity | category | symbol`. Deviating here would
/// silently break byte-identity of the fingerprint against pre-refactor
/// tier-B output, so this const is the single source of truth.
pub(crate) const FINGERPRINTED_FIELDS: [&str; 5] =
    ["file", "summary", "severity", "category", "symbol"];

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

/// T6c: cross-ledger duplicate detection. Loads the union of `primary_items`
/// and `other_items` (each already extracted from its doc by the caller),
/// tags every emitted item with `source_file` (the basename of its origin
/// ledger), and runs the selected tier over the union. `source_file` is an
/// OUTPUT-ONLY key — it's spliced into each JSON item at emit time and
/// never written back to either on-disk ledger. Tier C is file-scoped by
/// design (its line-window grouping assumes a single source file); passing
/// `DupTier::C` here errors with the exact string documented in the plan.
///
/// O61: takes `primary_items` / `other_items` by value so the source-file
/// tag can be inserted in-place via `tag_with_source_in_place` (a single
/// `BTreeMap::insert` per item) instead of cloning the whole TOML table per
/// entry. Callers in `cli/dispatch.rs` already construct these as owned
/// `Vec<TomlValue>` from the read_doc closures, so by-value passing is
/// natural.
pub(crate) fn items_find_duplicates_across(
    mut primary_items: Vec<TomlValue>,
    primary_file: &str,
    mut other_items: Vec<TomlValue>,
    other_file: &str,
    tier: DupTier,
) -> Result<Vec<JsonValue>> {
    if matches!(tier, DupTier::C) {
        bail!("tier C is file-scoped; use --tier A or --tier B with --across");
    }
    // Build a union vector where each entry remembers its source basename.
    // We carry the `source_file` tag through to emit-time by stashing it as
    // an in-memory TOML field directly on each owned item — no per-item
    // table clone (O61). The tier fns already use `toml_to_json` on emit,
    // so an in-memory field with a reserved name just propagates through
    // the JSON output automatically.
    //
    // Reserved key: `__tomlctl_source_file`. On emit we rename it to
    // `source_file`. If the on-disk data already carries `source_file`,
    // the pre-existing value is preserved under `source_file_orig` — the
    // output key `source_file` will hold the ledger-origin tag and the
    // distinct `source_file_orig` field carries the prior on-disk value.
    // In practice neither ledger schema writes `source_file` today, so the
    // collision branch is defensive-only.
    let primary_basename = Path::new(primary_file)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| primary_file.to_string());
    let other_basename = Path::new(other_file)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| other_file.to_string());
    for item in &mut primary_items {
        tag_with_source_in_place(item, &primary_basename);
    }
    for item in &mut other_items {
        tag_with_source_in_place(item, &other_basename);
    }
    let mut union: Vec<TomlValue> = Vec::with_capacity(primary_items.len() + other_items.len());
    union.append(&mut primary_items);
    union.append(&mut other_items);
    let groups = match tier {
        DupTier::A => find_duplicates_tier_a(&union)?,
        DupTier::B => find_duplicates_tier_b(&union)?,
        DupTier::C => unreachable!("tier C rejected above"),
    };
    // Post-process each group's items to promote the reserved tag to the
    // output `source_file` key.
    Ok(groups.into_iter().map(promote_source_tag).collect())
}

/// T6c helper: insert a reserved `__tomlctl_source_file` field into `item`
/// in place. Non-table items (defensive) are left unchanged — the tier fns
/// already filter non-tables via `as_table()`.
///
/// O61: mutates the existing TOML table directly rather than cloning it,
/// dropping the prior O(items × fields) per-item table-clone cost down to
/// a single `BTreeMap::insert` (plus an optional `remove` on the
/// defensive-only `source_file` collision branch).
fn tag_with_source_in_place(item: &mut TomlValue, source: &str) {
    let Some(tbl) = item.as_table_mut() else {
        return;
    };
    // Preserve any pre-existing `source_file` field under `source_file_orig`
    // so the output tag can reuse the clean name without losing data.
    if let Some(existing) = tbl.remove("source_file") {
        tbl.insert("source_file_orig".to_string(), existing);
    }
    tbl.insert(
        "__tomlctl_source_file".to_string(),
        TomlValue::String(source.to_string()),
    );
}

/// T6c helper: rename the reserved tag to the user-facing `source_file`
/// key on every item inside a group's `items` array.
fn promote_source_tag(mut group: JsonValue) -> JsonValue {
    if let Some(items) = group.get_mut("items").and_then(|v| v.as_array_mut()) {
        for item in items.iter_mut() {
            if let Some(obj) = item.as_object_mut()
                && let Some(src) = obj.remove("__tomlctl_source_file")
            {
                obj.insert("source_file".to_string(), src);
            }
        }
    }
    group
}

/// Truncated SHA-256 over `values` joined with `|` — the separator sits
/// *between* values, so a five-element list carries four separators. Keeps
/// the leading 8 bytes (64 bits; ~4B-item birthday collision bound).
///
/// Values are hashed verbatim: no trimming, case folding or unicode
/// normalisation. A caller wanting a folded field hashes the folded form.
///
/// Feeding the hasher incrementally is what lets the whole family share one
/// core with no intermediate `String`.
#[inline]
pub(crate) fn fingerprint_bytes<'a>(values: impl IntoIterator<Item = &'a str>) -> [u8; 8] {
    let mut h = Sha256::new();
    for (i, value) in values.into_iter().enumerate() {
        if i > 0 {
            h.update(b"|");
        }
        h.update(value.as_bytes());
    }
    let digest = h.finalize();
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

/// `fingerprint_bytes` as 16 lowercase hex chars.
#[inline]
pub(crate) fn fingerprint_hex<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    hex_lower(&fingerprint_bytes(values))
}

/// Digest of `fields` read off a TOML table in the given order, each via
/// `str_field` — missing and non-string fields hash as the empty string.
///
/// **The order of `fields` is load-bearing**: it is the byte order fed to
/// the hasher, so reordering re-keys every `dedup_id` computed with that
/// list. `FINGERPRINTED_FIELDS` is the ledger's frozen order.
pub(crate) fn fingerprint_bytes_from_table(tbl: &toml::Table, fields: &[&str]) -> [u8; 8] {
    fingerprint_bytes(fields.iter().map(|field| str_field(tbl, field)))
}

/// JSON-side `fingerprint_bytes_from_table`. `str_field_json` mirrors
/// `str_field`'s empty-on-missing-or-non-string rule, which is what keeps
/// the two sides byte-identical for the same underlying values — the JSON
/// write funnel and the TOML grouping path must never disagree.
pub(crate) fn fingerprint_bytes_from_json(
    obj: &serde_json::Map<String, JsonValue>,
    fields: &[&str],
) -> [u8; 8] {
    fingerprint_bytes(fields.iter().map(|field| str_field_json(obj, field)))
}

/// Tier-B fingerprint of a whole item, 16 lowercase hex chars.
///
/// R46: the `TomlValue`-wrapping form exists for the cross-path
/// byte-equivalence tests. Production callers hold a `&Table` or a JSON
/// object and go through the siblings, avoiding a per-row clone.
#[cfg(test)]
pub(crate) fn tier_b_fingerprint(item: &TomlValue) -> String {
    hex_lower(&tier_b_fingerprint_bytes(item))
}

pub(crate) fn tier_b_fingerprint_table(tbl: &toml::Table) -> String {
    hex_lower(&fingerprint_bytes_from_table(tbl, &FINGERPRINTED_FIELDS))
}

pub(crate) fn tier_b_fingerprint_json(obj: &serde_json::Map<String, JsonValue>) -> String {
    hex_lower(&tier_b_fingerprint_bytes_json(obj))
}

/// Raw 8-byte tier-B digest, so the grouping path keys its `BTreeMap` on
/// stack bytes and hex-encodes once per surviving group instead of once per
/// item. A non-table item hashes as five empty fields, keeping the helper
/// total for a grouping pass that has not filtered scalars out yet.
fn tier_b_fingerprint_bytes(item: &TomlValue) -> [u8; 8] {
    match item.as_table() {
        Some(tbl) => fingerprint_bytes_from_table(tbl, &FINGERPRINTED_FIELDS),
        None => fingerprint_bytes(FINGERPRINTED_FIELDS.iter().map(|_| "")),
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
    // O30: borrow keys from `items` (`str_field` returns `&'a str`) rather than
    // allocating a `String` per entry; emit-time `format!` still allocates once
    // per surviving group (O(groups), not O(items)).
    let mut by_symbol: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    let mut by_summary: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(tbl) = item.as_table() else { continue };
        let file = str_field(tbl, "file");
        let symbol = str_field(tbl, "symbol");
        if !symbol.is_empty() {
            by_symbol.entry((file, symbol)).or_default().push(i);
        } else {
            let summary = str_field(tbl, "summary");
            by_summary.entry((file, summary)).or_default().push(i);
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
    // O62: key the grouping map on the raw 8-byte fingerprint plus basename
    // — `[u8; 8]` is `Ord` and lives entirely on the stack, so we drop the
    // per-item 16-char hex `String` allocation that the hex-keyed map paid
    // for. Hex encoding is deferred to `hex_lower` once per surviving group
    // at emit time. The `basename` component of the group key stays local
    // to grouping — it's a display aid, not part of the fingerprint.
    let mut groups: BTreeMap<([u8; 8], String), Vec<usize>> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(tbl) = item.as_table() else { continue };
        // T6a: fingerprint computation shares its core with the
        // `dedup_id` auto-populate path (`items.rs`) — both go through
        // `fingerprint_bytes` so the same five fields hash in the
        // same order with the same truncation. The hex-string form lives
        // in `tier_b_fingerprint`; here we want the raw bytes.
        let short = tier_b_fingerprint_bytes(item);
        let file = str_field(tbl, "file");
        let basename = Path::new(file)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.to_string());
        groups.entry((short, basename)).or_default().push(i);
    }
    let mut out = Vec::new();
    for ((fingerprint_bytes, basename), idxs) in &groups {
        if idxs.len() < 2 {
            continue;
        }
        let refs: Vec<&TomlValue> = idxs.iter().map(|&i| &items[i]).collect();
        out.push(dup_group_json(
            "B",
            &format!(
                "suggestion fingerprint={} basename={}",
                hex_lower(fingerprint_bytes),
                basename
            ),
            &refs,
        ));
    }
    Ok(out)
}

fn find_duplicates_tier_c(items: &[TomlValue]) -> Result<Vec<JsonValue>> {
    // Candidates: items with empty/missing symbol AND line > 0.
    // O30: `Candidate.file` borrows from `items` via `str_field`'s `&'a str`
    // return, and the `by_file` map keys off the same borrow — drops the
    // per-item `String` allocation plus the prior `file.clone()` into the key.
    #[derive(Clone, Copy)]
    struct Candidate<'a> {
        idx: usize,
        file: &'a str,
        line: i64,
    }
    let mut by_file: HashMap<&str, Vec<Candidate>> = HashMap::new();
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
        let file = str_field(tbl, "file");
        by_file.entry(file).or_default().push(Candidate {
            idx: i,
            file,
            line,
        });
    }

    // O52: sort each per-file Vec in place during the build pass, then iterate
    // `by_file` read-only at emit time — drops the per-file `to_vec()` clone
    // and preserves the prior `(line, idx)` sort key exactly.
    for v in by_file.values_mut() {
        v.sort_by(|a, b| a.line.cmp(&b.line).then(a.idx.cmp(&b.idx)));
    }

    let mut out = Vec::new();
    // Deterministic order: sort file keys.
    let mut files: Vec<&&str> = by_file.keys().collect();
    files.sort();
    for file in files {
        let sorted = &by_file[file];
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

// =====================================================================
// O64: JSON-side dedup family — borrowed-DeTable fast-path siblings.
//
// These functions mirror the TOML-side `items_find_duplicates*` family
// byte-for-byte (same hashing, same field order, same emit shape) so the
// non-verify-integrity read path can skip the owned `TomlValue`
// intermediate. The shared helper `fingerprint_bytes` is
// re-used directly from the TOML side, which is the fingerprint-parity
// guarantee — both paths feed identical 8-byte digests into their
// grouping maps for the same field values.
//
// The owned `TomlValue` path is unchanged. `--verify-integrity` reads
// stay on the owned path because `read_doc_either` only swings to JSON
// when integrity verification is OFF; the integrity contract is intact.
// =====================================================================

/// O64: JSON-side sibling of `items_find_duplicates`. Reads the named
/// items array from a `JsonValue` doc and dispatches the requested tier.
/// Returns the same `Vec<JsonValue>` shape `items_find_duplicates` does
/// for the same underlying data — the parity test pins this.
pub(crate) fn items_find_duplicates_json(
    doc: &JsonValue,
    tier: DupTier,
) -> Result<Vec<JsonValue>> {
    let items: &[JsonValue] = items_array_json(doc, "items");
    match tier {
        DupTier::A => find_duplicates_tier_a_json(items),
        DupTier::B => find_duplicates_tier_b_json(items),
        DupTier::C => find_duplicates_tier_c_json(items),
    }
}

/// O64: JSON-side sibling of `items_find_duplicates_across`. Identical
/// semantics: error on `DupTier::C`, tag each item with its source
/// basename via the reserved `__tomlctl_source_file` key, run the union
/// through the requested tier, then promote the reserved tag to
/// `source_file` at emit time. The owned-side `promote_source_tag`
/// helper is reused as-is — it operates on JsonValue groups, which both
/// paths produce.
pub(crate) fn items_find_duplicates_across_json(
    mut primary_items: Vec<JsonValue>,
    primary_file: &str,
    mut other_items: Vec<JsonValue>,
    other_file: &str,
    tier: DupTier,
) -> Result<Vec<JsonValue>> {
    if matches!(tier, DupTier::C) {
        bail!("tier C is file-scoped; use --tier A or --tier B with --across");
    }
    let primary_basename = Path::new(primary_file)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| primary_file.to_string());
    let other_basename = Path::new(other_file)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| other_file.to_string());
    for item in &mut primary_items {
        tag_with_source_in_place_json(item, &primary_basename);
    }
    for item in &mut other_items {
        tag_with_source_in_place_json(item, &other_basename);
    }
    let mut union: Vec<JsonValue> = Vec::with_capacity(primary_items.len() + other_items.len());
    union.append(&mut primary_items);
    union.append(&mut other_items);
    let groups = match tier {
        DupTier::A => find_duplicates_tier_a_json(&union)?,
        DupTier::B => find_duplicates_tier_b_json(&union)?,
        DupTier::C => unreachable!("tier C rejected above"),
    };
    Ok(groups.into_iter().map(promote_source_tag).collect())
}

/// O64: JSON-side `tag_with_source_in_place`. Mutates a JSON object
/// item in place to carry its source-file tag under the reserved
/// `__tomlctl_source_file` key (renamed to `source_file` at emit time
/// by `promote_source_tag`).
fn tag_with_source_in_place_json(item: &mut JsonValue, source: &str) {
    let Some(obj) = item.as_object_mut() else {
        return;
    };
    if let Some(existing) = obj.remove("source_file") {
        obj.insert("source_file_orig".to_string(), existing);
    }
    obj.insert(
        "__tomlctl_source_file".to_string(),
        JsonValue::String(source.to_string()),
    );
}

/// O64: JSON-side `dup_group_json`. The TOML-side helper takes
/// `&[&TomlValue]` and calls `toml_to_json` on each; here we already
/// have JSON, so we clone each item value into the output array.
fn dup_group_json_json(tier: &str, key: &str, items: &[&JsonValue]) -> JsonValue {
    let mut obj = serde_json::Map::new();
    obj.insert("tier".into(), JsonValue::String(tier.into()));
    obj.insert("key".into(), JsonValue::String(key.into()));
    obj.insert(
        "items".into(),
        JsonValue::Array(items.iter().map(|v| (*v).clone()).collect()),
    );
    JsonValue::Object(obj)
}

/// O64: JSON-side sibling of `find_duplicates_tier_a`. Field-extraction
/// goes through `str_field_json` so missing/non-string fields hash as
/// "" in lockstep with the TOML side. Map keys borrow from the items
/// array's lifetime (`&str`), mirroring O30 on the owned side.
fn find_duplicates_tier_a_json(items: &[JsonValue]) -> Result<Vec<JsonValue>> {
    let mut by_symbol: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    let mut by_summary: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(obj) = item.as_object() else { continue };
        let file = str_field_json(obj, "file");
        let symbol = str_field_json(obj, "symbol");
        if !symbol.is_empty() {
            by_symbol.entry((file, symbol)).or_default().push(i);
        } else {
            let summary = str_field_json(obj, "summary");
            by_summary.entry((file, summary)).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for ((file, symbol), idxs) in &by_symbol {
        if idxs.len() < 2 {
            continue;
        }
        let refs: Vec<&JsonValue> = idxs.iter().map(|&i| &items[i]).collect();
        out.push(dup_group_json_json(
            "A",
            &format!("file={} symbol={}", file, symbol),
            &refs,
        ));
    }
    for ((file, summary), idxs) in &by_summary {
        if idxs.len() < 2 {
            continue;
        }
        let refs: Vec<&JsonValue> = idxs.iter().map(|&i| &items[i]).collect();
        out.push(dup_group_json_json(
            "A",
            &format!("file={} summary={}", file, summary),
            &refs,
        ));
    }
    Ok(out)
}

/// O64: JSON-side sibling of `find_duplicates_tier_b`. Reuses the
/// shared `fingerprint_bytes` core so the 8-byte digest is
/// byte-identical to the TOML-side digest for the same field values.
fn find_duplicates_tier_b_json(items: &[JsonValue]) -> Result<Vec<JsonValue>> {
    let mut groups: BTreeMap<([u8; 8], String), Vec<usize>> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(obj) = item.as_object() else { continue };
        let short = tier_b_fingerprint_bytes_json(obj);
        let file = str_field_json(obj, "file");
        let basename = Path::new(file)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.to_string());
        groups.entry((short, basename)).or_default().push(i);
    }
    let mut out = Vec::new();
    for ((fingerprint_bytes, basename), idxs) in &groups {
        if idxs.len() < 2 {
            continue;
        }
        let refs: Vec<&JsonValue> = idxs.iter().map(|&i| &items[i]).collect();
        out.push(dup_group_json_json(
            "B",
            &format!(
                "suggestion fingerprint={} basename={}",
                hex_lower(fingerprint_bytes),
                basename
            ),
            &refs,
        ));
    }
    Ok(out)
}

fn tier_b_fingerprint_bytes_json(obj: &serde_json::Map<String, JsonValue>) -> [u8; 8] {
    fingerprint_bytes_from_json(obj, &FINGERPRINTED_FIELDS)
}

/// O64: JSON-side sibling of `find_duplicates_tier_c`. Identical
/// candidate-filter (empty/missing symbol AND `line > 0`), identical
/// per-file sort key (`(line, idx)`), identical greedy 10-line window
/// extension. Field reads go through `str_field_json` / `i64_field_json`.
fn find_duplicates_tier_c_json(items: &[JsonValue]) -> Result<Vec<JsonValue>> {
    #[derive(Clone, Copy)]
    struct Candidate<'a> {
        idx: usize,
        file: &'a str,
        line: i64,
    }
    let mut by_file: HashMap<&str, Vec<Candidate>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(obj) = item.as_object() else { continue };
        let symbol = str_field_json(obj, "symbol");
        if !symbol.is_empty() {
            continue;
        }
        let line = i64_field_json(obj, "line");
        if line <= 0 {
            continue;
        }
        let file = str_field_json(obj, "file");
        by_file.entry(file).or_default().push(Candidate {
            idx: i,
            file,
            line,
        });
    }
    for v in by_file.values_mut() {
        v.sort_by(|a, b| a.line.cmp(&b.line).then(a.idx.cmp(&b.idx)));
    }
    let mut out = Vec::new();
    let mut files: Vec<&&str> = by_file.keys().collect();
    files.sort();
    for file in files {
        let sorted = &by_file[file];
        let n = sorted.len();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n && sorted[j].line - sorted[i].line <= 10 {
                j += 1;
            }
            if j - i >= 2 {
                let refs: Vec<&JsonValue> = sorted[i..j].iter().map(|c| &items[c.idx]).collect();
                out.push(dup_group_json_json(
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

    // ---- T6a: tier_b_fingerprint helper -----------------------------------

    /// The extracted helper must produce the 16-hex-char fingerprint the
    /// tier-B grouping path already emits. Build an item, hash it, and
    /// parse the tier-B key string — the two must agree byte-for-byte.
    #[test]
    fn tier_b_fingerprint_matches_grouping_key() {
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
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let items = items_array(&doc, "items");
        let fp1 = tier_b_fingerprint(&items[0]);
        let fp2 = tier_b_fingerprint(&items[1]);
        assert_eq!(
            fp1, fp2,
            "identical fingerprinted fields must hash to the same 16-hex digest"
        );
        assert_eq!(fp1.len(), 16, "fingerprint must be 16 hex chars (64 bits)");
        assert!(
            fp1.chars().all(|c| c.is_ascii_hexdigit() && (!c.is_ascii_uppercase())),
            "fingerprint must be lowercase hex: got {fp1:?}"
        );

        // Tier-B grouping's key string carries the fingerprint inline; the
        // extracted helper must produce the identical substring. This is
        // the byte-identity guard against a refactor silently flipping
        // the field order or truncation.
        let groups = items_find_duplicates(&doc, DupTier::B).unwrap();
        assert_eq!(groups.len(), 1);
        let key = groups[0].get("key").and_then(|v| v.as_str()).unwrap();
        assert!(
            key.contains(&format!("fingerprint={fp1}")),
            "tier-B group key must contain the same fingerprint the helper emits; \
             key={key:?} helper={fp1:?}"
        );
    }

    /// Differing values in any fingerprinted field must change the digest.
    /// Enumerate each of the five fields to pin the full surface; a bug
    /// that drops one field from the hash would leave the other four
    /// tests catching the regression.
    #[test]
    fn tier_b_fingerprint_differs_when_any_fingerprinted_field_changes() {
        let base: toml::Table = toml::toml! {
            file = "src/a.rs"
            summary = "s"
            severity = "minor"
            category = "style"
            symbol = "foo::bar"
        };
        let base_fp = tier_b_fingerprint(&TomlValue::Table(base.clone()));
        for key in &FINGERPRINTED_FIELDS {
            let mut changed = base.clone();
            changed.insert((*key).to_string(), TomlValue::String("MUTATED".into()));
            let fp = tier_b_fingerprint(&TomlValue::Table(changed));
            assert_ne!(
                fp, base_fp,
                "changing `{key}` must change the fingerprint"
            );
        }
    }

    /// Missing (or non-string) fingerprinted fields treat as empty string.
    /// This pins the "missing fields hash as empty" branch of `str_field`
    /// without relying on downstream grouping to reveal it.
    #[test]
    fn tier_b_fingerprint_missing_fields_hash_as_empty_strings() {
        // All five fields absent.
        let empty: TomlValue = toml::from_str("").unwrap();
        let fp_empty = tier_b_fingerprint(&empty);
        // Explicit-empty: each field present but empty string.
        let explicit: TomlValue = toml::from_str(
            r#"file = ""
summary = ""
severity = ""
category = ""
symbol = """#,
        )
        .unwrap();
        let fp_explicit = tier_b_fingerprint(&explicit);
        assert_eq!(
            fp_empty, fp_explicit,
            "missing fingerprinted fields must hash as empty strings"
        );
    }

    /// `tier_b_fingerprint` and `tier_b_fingerprint_json` must agree on the
    /// same underlying data. This is the guard against the JSON-side path
    /// (used by `items_add_value_to`) drifting from the TOML-side path
    /// (used by tier-B grouping).
    #[test]
    fn tier_b_fingerprint_json_matches_toml_side() {
        let toml_src: TomlValue = toml::from_str(
            r#"file = "src/a.rs"
summary = "hi"
severity = "warning"
category = "quality"
symbol = "foo""#,
        )
        .unwrap();
        let json_payload: JsonValue = serde_json::from_str(
            r#"{"file":"src/a.rs","summary":"hi","severity":"warning","category":"quality","symbol":"foo"}"#,
        )
        .unwrap();
        let fp_toml = tier_b_fingerprint(&toml_src);
        let fp_json = tier_b_fingerprint_json(json_payload.as_object().unwrap());
        assert_eq!(
            fp_toml, fp_json,
            "JSON and TOML sides must agree on the fingerprint"
        );
    }

    /// Field-order on the TOML side must not affect the fingerprint:
    /// `str_field` reads each key by name, and the helper concatenates in
    /// fixed `FINGERPRINTED_FIELDS` order. Two items with the same field
    /// values but serialised in different TOML orders must fingerprint
    /// identically.
    #[test]
    fn tier_b_fingerprint_stable_across_toml_field_order() {
        let a: TomlValue = toml::from_str(
            r#"file = "x"
summary = "y"
severity = "minor"
category = "bug"
symbol = "z""#,
        )
        .unwrap();
        let b: TomlValue = toml::from_str(
            r#"symbol = "z"
category = "bug"
severity = "minor"
summary = "y"
file = "x""#,
        )
        .unwrap();
        assert_eq!(tier_b_fingerprint(&a), tier_b_fingerprint(&b));
    }

    /// T6c: `--across` with tier C errors with the exact documented message.
    /// This is a unit-level pin; the integration test covers the CLI side.
    #[test]
    fn items_find_duplicates_across_rejects_tier_c() {
        let err = items_find_duplicates_across(Vec::new(), "a.toml", Vec::new(), "b.toml", DupTier::C)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "tier C is file-scoped; use --tier A or --tier B with --across"
        );
    }

    /// T6c: two items (one per ledger) carrying identical fingerprinted
    /// fields group together under tier B, and each emitted item carries
    /// a `source_file` tag naming its origin basename.
    #[test]
    fn items_find_duplicates_across_tier_b_tags_source_file() {
        let primary: TomlValue = toml::from_str(
            r#"[[items]]
id = "R1"
file = "src/a.rs"
summary = "dup"
severity = "warning"
category = "quality"
"#,
        )
        .unwrap();
        let other: TomlValue = toml::from_str(
            r#"[[items]]
id = "O1"
file = "src/a.rs"
summary = "dup"
severity = "warning"
category = "quality"
"#,
        )
        .unwrap();
        let primary_items = items_array(&primary, "items").to_vec();
        let other_items = items_array(&other, "items").to_vec();
        let groups = items_find_duplicates_across(
            primary_items,
            "review.toml",
            other_items,
            "optimise.toml",
            DupTier::B,
        )
        .unwrap();
        assert_eq!(groups.len(), 1, "expected one cross-ledger dup group");
        let items = groups[0].get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 2);
        let sources: Vec<&str> = items
            .iter()
            .map(|i| i.get("source_file").and_then(|v| v.as_str()).unwrap())
            .collect();
        assert!(sources.contains(&"review.toml"));
        assert!(sources.contains(&"optimise.toml"));
    }

    /// Byte-identity pin against digests computed outside this crate with
    /// `printf '<values joined by |>' | sha256sum | cut -c1-16`
    /// (2026-09-01). The second fixture is a row that exists on disk under
    /// `.claude/flows/composed-painting-truffle/review-ledger.toml`, so this
    /// also pins the digest against real stored data rather than only
    /// against a synthetic one. A change to the field order, the separator
    /// or the truncation re-keys every stored `dedup_id`; it fails here
    /// first.
    #[test]
    fn tier_b_fingerprint_matches_digests_pinned_outside_the_crate() {
        let synthetic: TomlValue = toml::from_str(
            r#"file = "src/a.rs"
summary = "dup-summary"
severity = "warning"
category = "quality""#,
        )
        .unwrap();
        assert_eq!(tier_b_fingerprint(&synthetic), "95953a6bf4f9bfb7");

        let stored: TomlValue = toml::from_str(
            r#"file = 'lumina/companion/src/git/shell.rs'
summary = 'detach_worktree NotFound classifier arms are dead code under checkout --detach; comment documents messages git never emits there'
severity = 'warning'
category = 'quality'
symbol = 'ShellGit::detach_worktree'"#,
        )
        .unwrap();
        assert_eq!(tier_b_fingerprint(&stored), "75fc04ebd198cec5");
    }

    /// The generalised core must join with `|` between values for any
    /// arity, not only the ledger's five — the backlog fingerprint passes a
    /// three-field list through the same function.
    #[test]
    fn fingerprint_hex_joins_any_arity_with_a_single_separator() {
        assert_eq!(fingerprint_hex(["a", "b", "c"]), "a52dd81bfd5e4e66");
        assert_eq!(fingerprint_hex(["", "", ""]), "565d240f5343e625");
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
