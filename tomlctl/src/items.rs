//! R63: `items_*` operations extracted from `main.rs` into a standalone
//! module so the crate root can shrink to pure dispatch plumbing. Every
//! function here operates on a parsed `TomlValue` doc (or a mutable one)
//! and returns either JSON output or an `anyhow::Result` — the I/O layer
//! (`mutate_doc`, `read_doc`, containment guards) stays in `io.rs`.
//!
//! The symmetric `items_*` / `items_*_to` pairs let the test-only wrappers
//! default `array_name = "items"` (the ledger's canonical array-of-tables)
//! while production dispatch always passes the `--array` flag through.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use toml::Value as TomlValue;

use crate::convert::{json_type_name, maybe_date_coerce, str_field, toml_to_json, walk_json_path};
use crate::dedup::{FINGERPRINTED_FIELDS, tier_b_fingerprint_json, tier_b_fingerprint_table};
#[cfg(test)]
use crate::dedup::tier_b_fingerprint;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{capture_row_id, item_id, item_id_json, items_array, items_array_mut};

/// T6b: env-var kill switch for every `dedup_id` auto-populate path. Any
/// value (even empty) disables the hook; the user opts out by simply
/// exporting the variable. Documented in README as the rollback lever.
const DEDUP_ID_KILL_SWITCH: &str = "TOMLCTL_NO_DEDUP_ID";

/// T6b: return `true` iff auto-populate of `dedup_id` should be skipped on
/// this invocation. Checked at every write-funnel hook site.
///
/// T11: exposed to the CLI dispatch layer so `items backfill-dedup-id` can
/// short-circuit to the documented `disabled-by-env` output without touching
/// the ledger. The backfill subcommand is the one explicit-intent write path
/// whose no-op cue is cleaner at the dispatch boundary than inside a compute
/// helper — every other funnel (add / update / apply / add-many) checks the
/// flag inside the per-funnel hook because the flag only gates the
/// auto-populate side-effect, not the whole operation.
pub(crate) fn dedup_id_disabled() -> bool {
    std::env::var(DEDUP_ID_KILL_SWITCH).is_ok()
}

/// T6b: auto-populate `dedup_id` on a single-item add. If the caller already
/// set `dedup_id` on the payload (any non-null JSON value), preserve the
/// explicit value. Otherwise compute `tier_b_fingerprint_json` from the
/// payload's fingerprinted fields and insert it.
///
/// Honours `TOMLCTL_NO_DEDUP_ID`: when set, returns without touching `obj`.
///
/// **Note on PROGRESS-LOG rendering**: `dedup_id` is a string field on the
/// on-disk TOML row. The render templates in `claude/commands/plan-update.md`
/// (lines 211-223 at time of writing) hard-code which columns make it into
/// rendered output, so `dedup_id` never leaks into user-facing progress log
/// lines despite being present on every new row.
///
/// **Ordering vs T5 `--dedupe-by`**: callers go through
/// `items_add_value_with_dedupe_to`, which runs the pre-scan BEFORE
/// delegating to `items_add_value_to` (the single write funnel that hooks
/// into this helper). On a dedupe-match there is no write and no
/// fingerprint computation; on a miss the auto-populate runs as normal.
/// The caller never sees an auto-populated `dedup_id` influence its own
/// pre-scan — preserving `--dedupe-by`'s "raw-equality-on-named-fields"
/// contract from T5.
fn apply_dedup_id_on_add(obj: &mut serde_json::Map<String, JsonValue>) {
    if dedup_id_disabled() {
        return;
    }
    // "Explicit" = caller put a `dedup_id` key in the payload. A null value
    // is treated as "unset this field" per the rest of the codebase (see
    // `is_empty_json`): we preserve the existing behaviour (skip) by
    // leaving the caller's null in place — it gets stripped on merge.
    if obj.contains_key("dedup_id") {
        return;
    }
    let fp = tier_b_fingerprint_json(obj);
    obj.insert("dedup_id".to_string(), JsonValue::String(fp));
}

/// T6b: auto-populate `dedup_id` on a single-item update. Four branches
/// (documented in the README Contracts section):
///   1. Patch explicitly sets `dedup_id` (non-empty string): preserve — no recompute.
///   2. Patch touches a fingerprinted field AND does not set `dedup_id`:
///      recompute from the merged (patch-over-existing) post-patch state.
///   3. Patch does NOT touch a fingerprinted field AND existing item lacks
///      `dedup_id`: leave absent — Task 11's `backfill-dedup-id` is the
///      explicit upgrade path for legacy ledgers.
///   4. Patch does NOT touch a fingerprinted field AND existing item HAS
///      `dedup_id`: preserve existing — the patch can't have changed any
///      input to the fingerprint, so the existing digest is still correct.
///
/// Honours `TOMLCTL_NO_DEDUP_ID`.
///
/// `existing_tbl` is the item as it looks BEFORE the merge runs; the patch
/// is the post-merge delta. Branch 2 builds an in-memory view of "existing
/// plus patch" restricted to the five fingerprinted fields, hashes that,
/// and stashes the result back into the patch so the downstream merge
/// loop writes it as a normal key-value.
///
/// **`{"dedup_id": null}` case**: a JSON null on `dedup_id` is NOT "remove
/// the existing digest" — it's "patch didn't meaningfully touch this
/// field". `is_empty_json` (O51) already filters null/empty values out of
/// the merge loop, so the existing value survives untouched. This helper
/// treats a null or empty-string `dedup_id` as "absent in patch" for
/// branch-classification purposes, matching the downstream merge behaviour.
fn apply_dedup_id_on_update(
    existing_tbl: &toml::Table,
    patch_obj: &mut serde_json::Map<String, JsonValue>,
) {
    if dedup_id_disabled() {
        return;
    }
    // Branch 1: explicit non-empty `dedup_id` in patch — preserve as-is.
    // Treat null / empty-string as "absent" because `is_empty_json` will
    // strip them in the merge loop; preservation of the existing value
    // is what the user ends up seeing either way.
    let explicit_dedup_id = patch_obj
        .get("dedup_id")
        .map(|v| {
            !matches!(v, JsonValue::Null)
                && !matches!(v, JsonValue::String(s) if s.is_empty())
        })
        .unwrap_or(false);
    if explicit_dedup_id {
        return;
    }
    // Branch classification by "does the patch touch any fingerprinted field"?
    // `is_empty_json` would strip null/empty, so those don't count as
    // touches either — a patch with `{"file": null}` is semantically "don't
    // change file" and should not trigger a recompute.
    let touches_fingerprinted = FINGERPRINTED_FIELDS.iter().any(|k| {
        patch_obj
            .get(*k)
            .map(|v| !is_empty_json(v))
            .unwrap_or(false)
    });
    if !touches_fingerprinted {
        // Branches 3 and 4: no-op — existing value (absent or present)
        // stays untouched. Branch 3 intentionally does NOT silently
        // populate on an unrelated update; `items backfill-dedup-id`
        // (Task 11) is the canonical upgrade path for legacy ledgers.
        return;
    }
    // Branch 2: recompute from the patch-over-existing merged view.
    let fp = merged_fingerprint(existing_tbl, patch_obj);
    patch_obj.insert("dedup_id".to_string(), JsonValue::String(fp));
}

/// T6b: build the fingerprint from the merged (patch-over-existing) view of
/// the five fingerprinted fields. For each field: if the patch has it as a
/// non-empty string, use that; otherwise fall back to `existing_tbl`'s
/// value via `str_field` (empty string on missing / non-string). This is
/// the recompute branch of `apply_dedup_id_on_update`.
fn merged_fingerprint(
    existing_tbl: &toml::Table,
    patch_obj: &serde_json::Map<String, JsonValue>,
) -> String {
    // Build a fresh JSON object holding just the five fingerprinted fields
    // with their post-merge values, then feed it to the canonical JSON-side
    // fingerprinter. This stays in sync with `tier_b_fingerprint_json` —
    // same helper, same field order, same truncation.
    let mut merged = serde_json::Map::with_capacity(FINGERPRINTED_FIELDS.len());
    for &key in &FINGERPRINTED_FIELDS {
        let from_patch = patch_obj.get(key).and_then(|v| match v {
            JsonValue::String(s) if !s.is_empty() => Some(s.as_str()),
            _ => None,
        });
        let resolved = from_patch.unwrap_or_else(|| str_field(existing_tbl, key));
        merged.insert(key.to_string(), JsonValue::String(resolved.to_string()));
    }
    tier_b_fingerprint_json(&merged)
}

/// O18: minimum number of `update` ops in a batch before we pay to build
/// an `id → array_index` HashMap. Below this, the per-op linear scan
/// (`items_update_value_to` walks the array) is cheaper than the
/// up-front map build + per-`remove` rebuild.
///
/// O68: threshold tuned for typical 2-3 item apply batches. The original
/// midpoint (5) was calibrated for a 50-row ledger crossover band, but
/// typical review-apply / optimise-apply batches sit at 1-3 update ops,
/// which left the indexed fast path dormant on the most common workload.
/// Combined with the O(1)-per-entry `remove` invalidation below (replacing
/// the prior whole-index drop), the dispatch comparison `update_count >
/// ID_INDEX_BUILD_THRESHOLD` now activates the indexed path for any batch
/// with 3+ update ops — covering the bulk of `items apply` traffic. The
/// spec floor is 2 (no benefit at 1, and 0 always builds).
const ID_INDEX_BUILD_THRESHOLD: usize = 2;

#[cfg(test)]
pub(crate) fn items_get(doc: &TomlValue, id: &str) -> Result<JsonValue> {
    items_get_from(doc, "items", id)
}

/// R57: array-parametric `items get`. See `List --array`.
pub(crate) fn items_get_from(doc: &TomlValue, array_name: &str, id: &str) -> Result<JsonValue> {
    for item in items_array(doc, array_name) {
        if item_id(item) == Some(id) {
            return Ok(toml_to_json(item));
        }
    }
    bail!(
        "no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)",
        id
    )
}

/// O64: JSON-side sibling of `items_get_from`. Walks a `JsonValue` doc's
/// named items array and returns a clone of the first object whose `id`
/// field equals `id`. Used by the borrowed-DeTable fast-path in
/// `ItemsOp::Get`'s non-verify-integrity branch — the doc has already
/// been converted from `DeTable<'a>` to `JsonValue` once at the read
/// boundary via `detable_to_json`, so this walker only needs to find
/// and clone the matching item rather than re-traversing a TOML tree.
///
/// Output byte-identity: the returned `JsonValue` is structurally
/// equal to `toml_to_json(matching_toml_item)` for the same underlying
/// data — the `detable_to_json` parity test in `convert.rs` already
/// pins both converters to the same shape. Error message
/// ("no item with id = X") is emitted verbatim from both paths.
pub(crate) fn items_get_from_json(
    doc: &JsonValue,
    array_name: &str,
    id: &str,
) -> Result<JsonValue> {
    for item in crate::io::items_array_json(doc, array_name) {
        if crate::io::item_id_json(item) == Some(id) {
            return Ok(item.clone());
        }
    }
    bail!(
        "no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)",
        id
    )
}

#[cfg(test)]
pub(crate) fn items_add(doc: &mut TomlValue, json: &str) -> Result<()> {
    items_add_to(doc, "items", json)
}

/// R57: array-parametric `items add`. See `List --array`.
pub(crate) fn items_add_to(doc: &mut TomlValue, array_name: &str, json: &str) -> Result<()> {
    let patch: JsonValue = serde_json::from_str(json)
        .context("parsing --json (expected JSON object, e.g. `{\"id\":\"R1\",\"status\":\"resolved\"}`)")?;
    items_add_value_to(doc, patch, array_name)
}

/// O27: takes `patch` by value so we can destructure a `JsonValue::Object`
/// into its owned `Map<String, JsonValue>` and iterate `(String, JsonValue)`
/// without per-key `.clone()`. `maybe_date_coerce` still takes `&JsonValue`
/// (to avoid a cascade through `convert.rs` callers); the borrow is fine.
///
/// O51: fields whose value is "empty" (`JsonValue::Null`, `""`, or `[]`) are
/// silently skipped on write. This keeps ledger rows clean when agents emit
/// placeholder fields they never filled in. An explicit unset of a field
/// should use the dedicated `--unset` flag on `update` (this helper is also
/// the per-row path for `add`, where "unset an absent field" is trivially a
/// no-op). `Null` was already rejected by `json_to_toml`; we now short-circuit
/// it here before `maybe_date_coerce` so all three empty shapes share one
/// skip path.
pub(crate) fn items_add_value_to(
    doc: &mut TomlValue,
    patch: JsonValue,
    array_name: &str,
) -> Result<()> {
    let got_type = crate::convert::json_type_name(&patch);
    let JsonValue::Object(mut obj) = patch else {
        bail!(
            "--json must be a JSON object (e.g. {{\"id\":\"R1\",\"status\":\"open\"}}); got JSON {}",
            got_type
        );
    };
    // Ledger rows in the `items` array are keyed by `id` (the `[[items]]`
    // schema's primary key). Reject an id-less or empty-id row up front:
    // without this guard a batch (`items add-many` / `apply` add-op) whose
    // rows omit `id` lands unaddressable rows that silently break
    // `items list --pluck id`, every `update`/`apply` by-id op, and the
    // dedupe contract — the failure only surfaces downstream, far from the
    // bad write. Event-log arrays (`vet_events`, `rollback_events`,
    // `events`) carry no `id`, so the check is scoped to the ledger array.
    if array_name == "items"
        && !obj
            .get("id")
            .and_then(JsonValue::as_str)
            .is_some_and(|s| !s.is_empty())
    {
        bail!(
            "ledger row must carry a non-empty string `id` (e.g. {{\"id\":\"R1\", ...}}); \
             mint one with `tomlctl items next-id <file> --prefix <P>` and stamp it onto each row"
        );
    }
    // T6b: auto-populate `dedup_id` from the payload BEFORE conversion to
    // TOML, unless the caller already set it or the env-var kill switch is
    // active. Hooking here covers every single-add path: direct `items add`,
    // `items add-many` (which feeds each merged row through this funnel),
    // `items apply` add-op (via `apply_single_op` / `apply_op_indexed`),
    // and T5's `items_add_value_with_dedupe_to` (which delegates here on a
    // dedupe-miss, so the pre-scan never sees an auto-populated `dedup_id`
    // on its own payload).
    apply_dedup_id_on_add(&mut obj);
    let mut tbl = toml::Table::with_capacity(obj.len());
    for (k, v) in obj {
        if is_empty_json(&v) {
            continue;
        }
        let coerced = maybe_date_coerce(&k, &v)?;
        tbl.insert(k, coerced);
    }
    let arr = items_array_mut(doc, array_name)?;
    arr.push(TomlValue::Table(tbl));
    Ok(())
}

/// O58: walk a candidate ledger item by dotted path and convert ONLY the
/// leaf to JSON, mirroring the narrowing semantics of `walk_json_path`
/// (object descent only — no array-index segments). Used by
/// `find_dedupe_match` to compare a payload-side `walk_json_path` result
/// against the candidate's same-path leaf without materialising the
/// candidate's entire `TomlValue::Table` tree as `JsonValue` first. The
/// returned `Option<JsonValue>` matches `walk_json_path`'s `Option<&JsonValue>`
/// shape under `lhs.as_ref() == rhs` so equality semantics (including the
/// missing-on-both-sides convention) carry through unchanged.
fn narrow_toml_field(item: &TomlValue, path: &str) -> Option<JsonValue> {
    let mut cur = item;
    for seg in path.split('.') {
        match cur {
            TomlValue::Table(t) => {
                cur = t.get(seg)?;
            }
            _ => return None,
        }
    }
    Some(toml_to_json(cur))
}

/// T5 (plan `docs/plans/tomlctl-capability-gaps.md`): scan `doc[array_name]`
/// for the first existing item whose values at every path in `fields` equal
/// the corresponding values in `payload`. Returns the item's `id` on match,
/// `None` if no match.
///
/// Semantics:
///   - Each entry of `fields` is a dotted path interpreted by
///     `walk_json_path` (object descent only; no array-index segments).
///   - O58: each candidate field is narrowed to a leaf-only JSON value via
///     `narrow_toml_field`, so the comparison happens in JSON space without
///     materialising the whole candidate item. This keeps "raw JSON
///     equality" stable regardless of TOML surface differences (e.g. a
///     TOML datetime and a JSON string land as `JsonValue::String(...)`
///     in both the candidate and the payload views after coercion).
///   - Missing-on-both-sides is treated as equal (the shared `None`).
///     Missing-on-only-one-side is unequal.
///   - ALL fields must match (logical AND). Empty `fields` returns `None`;
///     callers use the absent-flag path for "no dedupe".
///
/// Deliberately **not** typed-coerced: the `--where` predicate family
/// supports `@int:5`, `@date:2026-04-18` etc. for typed comparison, but
/// `--dedupe-by` does not. Callers wanting a typed compare should use
/// `--where` to pre-filter and a separate script to decide. The help
/// text on `--dedupe-by` documents this.
///
/// **`--dedupe-by` does NOT implicitly include `dedup_id`**. Callers who
/// want fingerprint-based dedup (once T6 auto-populates `dedup_id`) must
/// pass `--dedupe-by dedup_id` explicitly. Keeping the flag orthogonal
/// to the dedup-id auto-populate lets a caller opt into one without the
/// other, and keeps T5 testable before T6 lands.
pub(crate) fn find_dedupe_match(
    doc: &TomlValue,
    array_name: &str,
    payload: &JsonValue,
    fields: &[String],
) -> Option<String> {
    if fields.is_empty() {
        return None;
    }
    for item in items_array(doc, array_name) {
        // O58: narrow per-dedupe-field to a leaf JSON conversion instead of
        // materialising the whole candidate item via `toml_to_json`. Mirrors
        // the `narrow_toml_to_json` fast-path pattern in `query.rs` (O21/O55):
        // walking the dotted path through `TomlValue::Table` arms and only
        // converting the LEAF to JSON keeps the per-row scan cost O(field
        // depth) rather than O(row size). For batch add-many on an N-item
        // ledger with M dedupe rows, this avoids O(N × ledger_size) full-
        // tree clones.
        let all_match = fields.iter().all(|f| {
            let lhs = narrow_toml_field(item, f);
            let rhs = walk_json_path(payload, f);
            lhs.as_ref() == rhs
        });
        if all_match {
            // Prefer the candidate-item's id (from the table) over walking
            // the JSON again; this matches `item_id`'s contract exactly.
            if let Some(id) = item_id(item) {
                return Some(id.to_string());
            }
            // Fallback: item lacks an id field. Return an empty string
            // sentinel so the caller can still surface "matched but
            // untagged" without panicking. In practice every ledger item
            // carries an id.
            return Some(String::new());
        }
    }
    None
}

/// T5: outcome of a dedupe-aware `items add`. `Added` = appended a fresh
/// row. `Skipped { matched_id }` = pre-scan found an existing row whose
/// dedupe-field values equal the payload's; the doc is left untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AddOutcome {
    Added,
    Skipped { matched_id: String },
}

/// T5: dedupe-aware wrapper around `items_add_value_to`. Empty `fields`
/// short-circuits to the current behaviour (append unconditionally,
/// return `Added`). A non-empty slice runs the pre-scan under the
/// caller-held lock; on match the outcome reports the matched id and
/// the doc is not mutated; on no-match we delegate to
/// `items_add_value_to` for the real insert.
pub(crate) fn items_add_value_with_dedupe_to(
    doc: &mut TomlValue,
    patch: JsonValue,
    array_name: &str,
    dedupe_fields: &[String],
) -> Result<AddOutcome> {
    if !dedupe_fields.is_empty()
        && let Some(matched_id) = find_dedupe_match(doc, array_name, &patch, dedupe_fields)
    {
        return Ok(AddOutcome::Skipped { matched_id });
    }
    items_add_value_to(doc, patch, array_name)?;
    Ok(AddOutcome::Added)
}

/// O51: "empty" predicate shared by `items_add_value_to` /
/// `items_update_value_to`. Returns `true` for `Null`, `""`, and `[]`.
/// Non-empty arrays, numbers, booleans, and nested objects all pass through.
fn is_empty_json(v: &JsonValue) -> bool {
    match v {
        JsonValue::Null => true,
        JsonValue::String(s) => s.is_empty(),
        JsonValue::Array(a) => a.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
pub(crate) fn items_update(
    doc: &mut TomlValue,
    id: &str,
    json: &str,
    unset: &[String],
) -> Result<()> {
    items_update_to(doc, "items", id, json, unset)
}

/// R57: array-parametric `items update`. See `List --array`.
pub(crate) fn items_update_to(
    doc: &mut TomlValue,
    array_name: &str,
    id: &str,
    json: &str,
    unset: &[String],
) -> Result<()> {
    let patch: JsonValue = serde_json::from_str(json)
        .context("parsing --json (expected JSON object, e.g. `{\"status\":\"resolved\"}`)")?;
    items_update_value_to(doc, array_name, id, patch, unset)
}

/// O27: takes `patch` by value so we can destructure the `Object` into its
/// owned `Map<String, JsonValue>` and consume each `(String, JsonValue)`
/// without per-key `.clone()`. `maybe_date_coerce` still takes `&JsonValue`
/// (avoids a `convert.rs` cascade); the borrow is fine.
///
/// O51: mirrors `items_add_value_to` — patch fields whose value is "empty"
/// (`Null`, `""`, `[]`) are skipped rather than written. To explicitly clear
/// a field on an existing row, use the `unset` array (same on the `apply`
/// batch form). The skip applies only to the merge path; `unset` still
/// removes named fields as before.
pub(crate) fn items_update_value_to(
    doc: &mut TomlValue,
    array_name: &str,
    id: &str,
    patch: JsonValue,
    unset: &[String],
) -> Result<()> {
    let got_type = crate::convert::json_type_name(&patch);
    let JsonValue::Object(mut patch_obj) = patch else {
        bail!(
            "--json must be a JSON object (e.g. {{\"status\":\"resolved\"}}); got JSON {}",
            got_type
        );
    };

    let arr = items_array_mut(doc, array_name)?;
    for item in arr.iter_mut() {
        let Some(tbl) = item.as_table_mut() else { continue };
        let matches = tbl.get("id").and_then(|v| v.as_str()) == Some(id);
        if !matches {
            continue;
        }
        // T6b: decide whether to recompute `dedup_id` before the merge loop
        // runs. `apply_dedup_id_on_update` inspects the existing table +
        // patch and, for branch 2 (fingerprinted-field patch, no explicit
        // `dedup_id`), inserts the freshly-computed digest into `patch_obj`
        // so the downstream merge loop writes it as a normal key. Other
        // branches leave `patch_obj` untouched.
        apply_dedup_id_on_update(tbl, &mut patch_obj);
        for (k, v) in patch_obj {
            if is_empty_json(&v) {
                continue;
            }
            let coerced = maybe_date_coerce(&k, &v)?;
            tbl.insert(k, coerced);
        }
        for key in unset {
            tbl.remove(key);
        }
        return Ok(());
    }
    bail!(
        "no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)",
        id
    )
}

#[cfg(test)]
pub(crate) fn items_apply(doc: &mut TomlValue, ops_json: &str) -> Result<()> {
    items_apply_to(doc, ops_json, "items")
}

#[cfg(test)]
pub(crate) fn items_apply_to(
    doc: &mut TomlValue,
    ops_json: &str,
    array_name: &str,
) -> Result<()> {
    items_apply_to_opts(doc, ops_json, array_name, false)
}

/// Extended variant of `items_apply_to` honouring the `--no-remove` flag (R37).
/// When `no_remove` is true, the batch is scanned up-front for any `remove` op;
/// if present, the whole apply is refused — no partial mutation occurs because
/// the check runs before the mutation loop.
///
/// O27: consumes the parsed `ops` array by value (`.into_iter()`) so each
/// op flows by ownership into `apply_single_op`, eliminating per-op patch
/// clones the previous `arr.iter()` path forced.
///
/// R45: string-parsing wrapper retained for tests that exercise the live
/// mutator from a JSON literal. Production dispatch goes through
/// `compute_apply_mutation` → `items_apply_parsed_to_opts` so the parse
/// happens once at the CLI boundary; this wrapper is kept for the
/// internal test surface only.
///
/// O18: for batches with `> ID_INDEX_BUILD_THRESHOLD` `update` ops the
/// post-parse path (`items_apply_parsed_to_opts`) builds an
/// `id → array_index` `HashMap` once and uses it for O(1) lookups in
/// `apply_op_indexed` (instead of the per-op linear scan inside
/// `items_update_value_to` / `items_remove_from`). `add` ops append to
/// the map; `remove` ops invalidate it and force a rebuild before the
/// next indexed op needs it. Below threshold we keep the simpler
/// linear-scan path — building the map costs a full array walk that
/// doesn't pay off on small batches.
#[cfg(test)]
pub(crate) fn items_apply_to_opts(
    doc: &mut TomlValue,
    ops_json: &str,
    array_name: &str,
    no_remove: bool,
) -> Result<()> {
    let ops: JsonValue = serde_json::from_str(ops_json).context(
        "parsing --ops (expected JSON array of op objects, e.g. `[{\"op\":\"update\",\"id\":\"R1\",\"json\":{\"status\":\"resolved\"}}]`)"
    )?;
    items_apply_parsed_to_opts(doc, ops, array_name, no_remove)
}

/// R45: post-parse sibling of `items_apply_to_opts`. Takes already-parsed
/// `ops` (consumed by-value to feed the existing `.into_iter()` loop) so
/// callers that have already validated the JSON shape — `compute_apply_mutation`
/// (after walking the array for per-op id capture) and the CLI dispatch layer
/// (after the `MAX_OPS_PER_APPLY` length check) — can avoid a second parse.
/// All validation gates (`--no-remove`, op-shape errors, missing ids) and
/// error surfaces are byte-identical to the string-parsing path.
pub(crate) fn items_apply_parsed_to_opts(
    doc: &mut TomlValue,
    ops: JsonValue,
    array_name: &str,
    no_remove: bool,
) -> Result<()> {
    let got_type = crate::convert::json_type_name(&ops);
    let JsonValue::Array(arr) = ops else {
        bail!(
            "--ops must be a JSON array (e.g. [{{\"op\":\"update\",\"id\":\"R1\",\"json\":{{...}}}}]); got JSON {}",
            got_type
        );
    };
    // O54: fail-before-mutate for `--no-remove` is a required property (the
    // flag exists precisely so review-apply/optimise-apply never partially
    // erase audit history before bailing). A separate pre-pass is therefore
    // mandatory — "merge into the main loop" would leak mutations before the
    // first remove op is discovered. We keep the pre-pass but collapse the
    // explicit loop to `iter().position(...)` so the no-remove branch reads
    // as a single short expression.
    if no_remove
        && let Some(i) = arr
            .iter()
            .position(|op| op.get("op").and_then(|v| v.as_str()) == Some("remove"))
    {
        bail!(
            "op[{}] is a remove op, but --no-remove was set; this flag is used by review-apply/optimise-apply to prevent agent-generated payloads from erasing audit history",
            i
        );
    }
    // The O18 threshold depends on `update` op count, so we still do one
    // walk over the array regardless of the no-remove flag.
    let update_count: usize = arr
        .iter()
        .filter(|op| op.get("op").and_then(|v| v.as_str()) == Some("update"))
        .count();

    if update_count > ID_INDEX_BUILD_THRESHOLD {
        // O18 fast path: build the id→index map once, then dispatch each op
        // through `apply_op_indexed`, which performs O(1) lookups for
        // update/remove. The map is owned mutably across the loop and kept
        // in sync (or invalidated on remove) by the helper.
        let mut id_index: Option<HashMap<String, usize>> = Some(build_id_index(doc, array_name)?);
        for (i, op) in arr.into_iter().enumerate() {
            apply_op_indexed(doc, op, array_name, &mut id_index)
                .with_context(|| format!("op[{}] failed", i))?;
        }
    } else {
        for (i, op) in arr.into_iter().enumerate() {
            apply_single_op(doc, op, array_name).with_context(|| format!("op[{}] failed", i))?;
        }
    }
    Ok(())
}

/// O18: build an `id → array_index` map for `array_name` inside `doc`.
/// Returns an empty map if the array is missing or empty (consistent with
/// how `items_array` returns an empty slice).
fn build_id_index(doc: &TomlValue, array_name: &str) -> Result<HashMap<String, usize>> {
    let arr = items_array(doc, array_name);
    let mut map = HashMap::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        if let Some(id) = item_id(item) {
            map.insert(id.to_string(), idx);
        }
    }
    Ok(map)
}

/// O18: indexed sibling of `apply_single_op`. Same op-dispatch semantics
/// (and same error messages) but routes `update` / `remove` through the
/// id-index for O(1) target resolution. The `id_index` is `Option` so
/// `remove` can drop it (`.take()`); the next op that needs it rebuilds
/// before lookup.
fn apply_op_indexed(
    doc: &mut TomlValue,
    op: JsonValue,
    array_name: &str,
    id_index: &mut Option<HashMap<String, usize>>,
) -> Result<()> {
    let got_type = crate::convert::json_type_name(&op);
    let JsonValue::Object(mut obj) = op else {
        bail!(
            "op must be a JSON object (e.g. {{\"op\":\"update\",\"id\":\"R1\",\"json\":{{...}}}}); got JSON {}",
            got_type
        );
    };
    let op_name = obj
        .get("op")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow!(
                "op missing `op` field; required shape is {{\"op\":\"add|update|remove\",\"id\":\"<id>\",\"json\":{{...}}}}"
            )
        })?
        .to_string();
    match op_name.as_str() {
        "add" => {
            let json = obj
                .remove("json")
                .ok_or_else(|| {
                    anyhow!(
                        "add op missing `json` field; required shape is {{\"op\":\"add\",\"json\":{{<row fields>}}}}"
                    )
                })?;
            // Capture the new entry's id (if present + a string) before the
            // value is consumed; on success append it to the index so a
            // later update/remove in the same batch can find it. R21:
            // route through `item_id_json` so the index-insert path shares
            // the same id-extraction predicate as the three `MutationPlan`
            // capture sites (which use `capture_row_id` for their `String`
            // form); divergence here would silently miss the index-insert
            // for ids that those sites would still report in the plan.
            let new_id: Option<String> = item_id_json(&json).map(str::to_string);
            // Capture the array length before the append so the inserted
            // index stays correct even if a future hook (dedupe-skip,
            // batched append) makes `items_add_value_to` grow the array
            // by something other than exactly one element.
            let len_before = items_array(doc, array_name).len();
            items_add_value_to(doc, json, array_name)?;
            if let (Some(id), Some(map)) = (new_id, id_index.as_mut()) {
                map.insert(id, len_before);
            }
            Ok(())
        }
        "update" => {
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow!(
                        "update op missing `id` field; required shape is {{\"op\":\"update\",\"id\":\"<id>\",\"json\":{{<patch>}}}}"
                    )
                })?
                .to_string();
            let json = obj
                .remove("json")
                .ok_or_else(|| {
                    anyhow!(
                        "update op missing `json` field; required shape is {{\"op\":\"update\",\"id\":\"<id>\",\"json\":{{<patch>}}}}"
                    )
                })?;
            let unset = take_unset(obj.remove("unset"))?;
            // Lazy-rebuild the index if a previous remove invalidated it.
            if id_index.is_none() {
                *id_index = Some(build_id_index(doc, array_name)?);
            }
            let map = id_index.as_ref().expect("rebuilt above");
            let Some(&idx) = map.get(&id) else {
                bail!(
                    "no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)",
                    id
                );
            };
            // R57: update honours --array. Direct-index update bypasses
            // the linear scan in `items_update_value_to`.
            update_at_index(doc, array_name, idx, &id, json, &unset)
        }
        "remove" => {
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow!(
                        "remove op missing `id` field; required shape is {{\"op\":\"remove\",\"id\":\"<id>\"}}"
                    )
                })?;
            // R57: remove also follows --array. Order-preserving `Vec::remove`
            // (via `retain`) shifts later indexes by 1.
            //
            // O68: keep the index alive across `remove` ops by patching it in
            // place — drop the removed key and decrement every index that
            // sat AFTER the removed slot. This is still O(map_size) per
            // remove but skips the array walk + per-id `to_string` heap
            // churn that `build_id_index` does on a fresh rebuild. For
            // interleaved remove/update batches the saving compounds: every
            // `update` after the first `remove` previously paid a full
            // rebuild before its O(1) lookup; now it lands on a still-warm
            // index. We capture the removed index BEFORE the call so a stale
            // index entry (id-in-map but not-in-doc, which shouldn't happen
            // post-O18 but defensively allowed) falls through to the legacy
            // full-rebuild path on the next op that needs the map.
            let removed_idx = id_index.as_ref().and_then(|m| m.get(id).copied());
            items_remove_from(doc, array_name, id)?;
            match (id_index.as_mut(), removed_idx) {
                (Some(map), Some(idx)) => {
                    map.remove(id);
                    for v in map.values_mut() {
                        if *v > idx {
                            *v -= 1;
                        }
                    }
                }
                _ => {
                    // No live map (already invalidated) or the removed id
                    // was never tracked there: fall back to the full-rebuild
                    // path so the next indexed op sees a consistent view.
                    *id_index = None;
                }
            }
            Ok(())
        }
        other => bail!("unknown op `{}`; expected one of: add, update, remove", other),
    }
}

/// O18 helper: parse the optional `unset` field of an `update` op into a
/// `Vec<String>`, with the same R36 type-only error messages as
/// `apply_single_op`.
fn take_unset(unset: Option<JsonValue>) -> Result<Vec<String>> {
    match unset {
        None | Some(JsonValue::Null) => Ok(Vec::new()),
        Some(JsonValue::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for (idx, entry) in a.into_iter().enumerate() {
                match entry {
                    JsonValue::String(s) => out.push(s),
                    other => bail!(
                        "update op `unset` must be an array of strings (e.g. `[\"status\",\"resolution\"]`), got JSON {} at index {}",
                        json_type_name(&other),
                        idx
                    ),
                }
            }
            Ok(out)
        }
        Some(other) => bail!(
            "update op `unset` must be a JSON array of strings (e.g. `[\"status\",\"resolution\"]`), got JSON {}",
            json_type_name(&other)
        ),
    }
}

/// O18 helper: O(1) sibling of `items_update_value_to` that takes the
/// already-resolved array index. The `expected_id` parameter is checked
/// defensively against the indexed entry to surface stale-index bugs as a
/// hard error (matches the legacy "no item with id = X" message).
fn update_at_index(
    doc: &mut TomlValue,
    array_name: &str,
    idx: usize,
    expected_id: &str,
    patch: JsonValue,
    unset: &[String],
) -> Result<()> {
    let got_type = crate::convert::json_type_name(&patch);
    let JsonValue::Object(mut patch_obj) = patch else {
        bail!(
            "--json must be a JSON object (e.g. {{\"status\":\"resolved\"}}); got JSON {}",
            got_type
        );
    };
    let arr = items_array_mut(doc, array_name)?;
    let item = arr.get_mut(idx).ok_or_else(|| {
        anyhow!(
            "no item with id = {} (stale id-index; run `tomlctl items list <file> --pluck id` to enumerate available ids)",
            expected_id
        )
    })?;
    let tbl = item.as_table_mut().ok_or_else(|| {
        anyhow!(
            "no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)",
            expected_id
        )
    })?;
    if tbl.get("id").and_then(|v| v.as_str()) != Some(expected_id) {
        bail!(
            "no item with id = {} (id-index drift; run `tomlctl items list <file> --pluck id` to enumerate available ids)",
            expected_id
        );
    }
    // T6b: parity with `items_update_value_to` — run the recompute-branch
    // classifier before the merge loop. The indexed and linear-scan paths
    // share this helper so `dedup_id` never diverges between the two
    // dispatch paths.
    apply_dedup_id_on_update(tbl, &mut patch_obj);
    // O51: parity with `items_update_value_to` — skip empty-valued patch fields
    // so the indexed fast-path doesn't diverge from the linear-scan path.
    for (k, v) in patch_obj {
        if is_empty_json(&v) {
            continue;
        }
        let coerced = maybe_date_coerce(&k, &v)?;
        tbl.insert(k, coerced);
    }
    for key in unset {
        tbl.remove(key);
    }
    Ok(())
}

/// O27: takes `op` by value so the `add`/`update` arms can hand the inner
/// `json` payload to `items_add_value_to` / `items_update_value_to` by
/// value, eliminating the per-row patch clone the previous `&JsonValue`
/// signature forced. Caller (`items_apply_to_opts`) iterates the parsed
/// ops array via `.into_iter()` to feed owned values here.
pub(crate) fn apply_single_op(
    doc: &mut TomlValue,
    op: JsonValue,
    array_name: &str,
) -> Result<()> {
    let got_type = crate::convert::json_type_name(&op);
    let JsonValue::Object(mut obj) = op else {
        bail!(
            "op must be a JSON object (e.g. {{\"op\":\"update\",\"id\":\"R1\",\"json\":{{...}}}}); got JSON {}",
            got_type
        );
    };
    let op_name = obj
        .get("op")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow!(
                "op missing `op` field; required shape is {{\"op\":\"add|update|remove\",...}}"
            )
        })?
        .to_string();
    match op_name.as_str() {
        "add" => {
            let json = obj
                .remove("json")
                .ok_or_else(|| {
                    anyhow!(
                        "add op missing `json` field; required shape is {{\"op\":\"add\",\"json\":{{<row fields>}}}}"
                    )
                })?;
            items_add_value_to(doc, json, array_name)
        }
        "update" => {
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow!(
                        "update op missing `id` field; required shape is {{\"op\":\"update\",\"id\":\"<id>\",\"json\":{{<patch>}}}}"
                    )
                })?
                .to_string();
            let json = obj
                .remove("json")
                .ok_or_else(|| {
                    anyhow!(
                        "update op missing `json` field; required shape is {{\"op\":\"update\",\"id\":\"<id>\",\"json\":{{<patch>}}}}"
                    )
                })?;
            // R2: share the `unset` parser with `apply_op_indexed` via the
            // `take_unset` helper so the two dispatch paths can't drift on
            // shape errors or `null` handling.
            let unset = take_unset(obj.remove("unset"))?;
            // R57: update now honours the apply-op's --array parameter so a
            // batch targeting e.g. `rollback_events` can update entries there,
            // not just in `[[items]]`.
            items_update_value_to(doc, array_name, &id, json, &unset)
        }
        "remove" => {
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow!(
                        "remove op missing `id` field; required shape is {{\"op\":\"remove\",\"id\":\"<id>\"}}"
                    )
                })?;
            // R57: remove also follows the --array parameter.
            items_remove_from(doc, array_name, id)
        }
        other => bail!("unknown op `{}`; expected one of: add, update, remove", other),
    }
}

#[cfg(test)]
pub(crate) fn items_remove(doc: &mut TomlValue, id: &str) -> Result<()> {
    items_remove_from(doc, "items", id)
}

/// R57: array-parametric `items remove`. See `List --array`.
pub(crate) fn items_remove_from(doc: &mut TomlValue, array_name: &str, id: &str) -> Result<()> {
    let arr = items_array_mut(doc, array_name)?;
    let before = arr.len();
    arr.retain(|item| item_id(item) != Some(id));
    if arr.len() == before {
        bail!(
            "no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)",
            id
        );
    }
    Ok(())
}

pub(crate) fn items_next_id(doc: &TomlValue, prefix: &str) -> Result<String> {
    // T8: both prefix-shape rejections are CLI-surface validation failures —
    // tag them `Validation` so `--error-format json` reports the same `kind`
    // regardless of which rule fired. Text output is byte-identical to the
    // pre-T8 `bail!(...)` form; `tagged_err`'s `TaggedError` renders its
    // message verbatim under `{:#}` (see `errors.rs` Display note).
    if prefix.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "prefix must not be empty — use a letter like R, O, or A",
        ));
    }
    if prefix.chars().all(|c| c.is_ascii_digit()) {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "prefix must not be all-digit — would collide with numeric-suffix parsing",
        ));
    }
    let mut max_n: u64 = 0;
    for item in items_array(doc, "items") {
        if let Some(id) = item_id(item)
            && let Some(rest) = id.strip_prefix(prefix)
            && let Ok(n) = rest.parse::<u64>()
            && n > max_n
        {
            max_n = n;
        }
    }
    Ok(format!("{}{}", prefix, max_n + 1))
}

/// Task 4: sibling of `items_next_id` that scans the ledger's existing ids,
/// infers the (single) letter prefix in use, and delegates to `items_next_id`.
/// Used by `items next-id --infer-from-file` when the caller doesn't want to
/// hard-code the prefix in the invocation — the canonical case being an agent
/// that's handed an arbitrary `<ledger>` path and needs to mint the next id
/// without knowing whether it's an R / O / E (or future) schema.
///
/// Prefix extraction: for each `id` in `[[items]]`, split at the first ASCII
/// digit; everything before the digit is the prefix. Ids with no digit, or
/// ids that start with a digit (empty prefix), are skipped — they can't
/// participate in the monotonic `{prefix}{n}` scheme regardless.
///
/// Error messages are load-bearing (tests assert byte-for-byte):
///
/// - Zero items / zero extractable prefixes: `--infer-from-file requires a
///   non-empty ledger or explicit --prefix`
/// - Multiple distinct prefixes: `--infer-from-file found multiple prefixes
///   ({sorted-csv}); pass --prefix explicitly` — sorted alphabetically so
///   the message is deterministic regardless of item order on disk.
pub(crate) fn items_infer_and_next_id(doc: &TomlValue) -> Result<String> {
    let mut prefixes: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for item in items_array(doc, "items") {
        if let Some(id) = item_id(item)
            && let Some(split) = id.find(|c: char| c.is_ascii_digit())
            && split > 0
        {
            prefixes.insert(id[..split].to_string());
        }
    }
    match prefixes.len() {
        0 => bail!(
            "--infer-from-file requires a non-empty ledger or explicit --prefix (the file has no items with a letter-prefixed `id` to infer from; pass --prefix R/O/A/E directly)"
        ),
        1 => {
            let prefix = prefixes.into_iter().next().expect("len == 1");
            items_next_id(doc, &prefix)
        }
        _ => {
            let joined = prefixes.into_iter().collect::<Vec<_>>().join(", ");
            bail!(
                "--infer-from-file found multiple prefixes ({}); pass --prefix explicitly",
                joined
            )
        }
    }
}

/// Parse NDJSON input (one JSON value per line) into a `Vec<JsonValue>`. Blank
/// lines (after trimming) are skipped but counted in the 1-indexed line number
/// used in error messages, so `line N` here matches the source line the caller
/// typed.
///
/// The function is all-or-nothing: on the first malformed line it returns
/// `Err`, so the caller may rely on receiving either a fully parsed batch or
/// no rows at all. No side effects.
pub(crate) fn parse_ndjson(s: &str) -> Result<Vec<JsonValue>> {
    // O48: pre-size by newline count so the common case (one JSON row per
    // line, no blanks) fills the Vec without any reallocation. Blank lines
    // over-shoot by at most a handful, and a trailing-newline-absent final
    // row under-shoots by one — both are cheap compared with the geometric
    // regrowth cost of starting at capacity 0 on an N-row batch. The SIMD
    // newline scan in `memchr`-backed iterators runs in nanoseconds for the
    // payload sizes tomlctl sees (agent-generated NDJSON, typically <1 MB).
    let mut rows = Vec::with_capacity(s.as_bytes().iter().filter(|&&b| b == b'\n').count());
    for (idx, line) in s.lines().enumerate() {
        let n = idx + 1;
        if line.trim().is_empty() {
            continue;
        }
        let v: JsonValue = serde_json::from_str(line)
            .with_context(|| format!("line {} (expected one JSON object per line; e.g. {{\"id\":\"R1\",\"status\":\"open\"}})", n))?;
        rows.push(v);
    }
    Ok(rows)
}

/// R4: validate `defaults` as a JSON object (or return an empty map when
/// `None`) and clone into an owned `Map` the caller can reuse per row.
/// Shared by `items_add_many` and `items_add_many_with_dedupe` so the two
/// batch funnels can't drift on the shape-error message or the "no
/// defaults" empty-map branch.
fn defaults_base(defaults: Option<&JsonValue>) -> Result<serde_json::Map<String, JsonValue>> {
    match defaults {
        Some(v) => Ok(v
            .as_object()
            .ok_or_else(|| {
                anyhow!(
                    "--defaults-json must be a JSON object (e.g. {{\"status\":\"open\",\"severity\":\"warning\"}}); got JSON {}",
                    crate::convert::json_type_name(v)
                )
            })?
            .clone()),
        None => Ok(serde_json::Map::new()),
    }
}

/// R4: build the per-row merged payload — defaults provide the base layer,
/// per-row keys shallow-overwrite on conflict. Pre-sizes the target map
/// for the sum of both sources (over-allocates when the row shadows a
/// default; cheaper than a re-grow inside `extend`).
///
/// O60: walk `base` once and for each entry choose the row's value when
/// the row shadows the default (cloning ONLY the chosen value, not both),
/// then append row keys absent from base. The previous `extend(base.clone())`
/// plus per-row insert pattern paid a wasted `clone` per shadowed default;
/// for 100 rows times 10 default fields that was ~1000 transient String
/// allocations per batch. Output ordering is preserved byte-identical to
/// the legacy form: defaults appear in their `base.iter()` order (with
/// row-supplied values at shadowed positions), and non-shadowed row keys
/// trail in `row_obj.iter()` order, matching how `serde_json::Map`'s
/// `IndexMap` backing handled the original `extend` plus `insert` sequence.
fn merge_row_over_base(
    base: &serde_json::Map<String, JsonValue>,
    row_obj: &serde_json::Map<String, JsonValue>,
) -> serde_json::Map<String, JsonValue> {
    let mut merged = serde_json::Map::with_capacity(base.len() + row_obj.len());
    for (k, v) in base.iter() {
        match row_obj.get(k.as_str()) {
            Some(rv) => {
                merged.insert(k.clone(), rv.clone());
            }
            None => {
                merged.insert(k.clone(), v.clone());
            }
        }
    }
    for (k, v) in row_obj.iter() {
        if !base.contains_key(k.as_str()) {
            merged.insert(k.clone(), v.clone());
        }
    }
    merged
}

/// Append each row in `rows` to `array_name` inside `doc`, stamping fields
/// from `defaults` first (when `Some`) and shallow-merging per-row keys on
/// top (per-row wins on conflict). Each row must be a JSON object; an
/// array/scalar row is rejected with `row N: must be a JSON object`. Date
/// coercion for `DATE_KEYS` is inherited from `items_add_value_to` — this
/// helper does not reimplement it.
///
/// The batch aborts on the first bad row. No explicit rollback is needed:
/// the caller holds the file lock and all mutation is in-memory until the
/// outer `mutate_doc` persists. Returns the number of rows appended.
pub(crate) fn items_add_many(
    doc: &mut TomlValue,
    array_name: &str,
    rows: &[JsonValue],
    defaults: Option<&JsonValue>,
) -> Result<usize> {
    // O26 / R4: pre-validate defaults once and share the `base` + per-row
    // merge shape with `items_add_many_with_dedupe`.
    let base = defaults_base(defaults)?;
    for (i, row) in rows.iter().enumerate() {
        let row_obj = row.as_object().ok_or_else(|| {
            anyhow!(
                "row {}: must be a JSON object (e.g. {{\"id\":\"R1\",\"summary\":\"...\"}}); got JSON {}",
                i + 1,
                crate::convert::json_type_name(row)
            )
        })?;
        let merged = merge_row_over_base(&base, row_obj);
        items_add_value_to(doc, JsonValue::Object(merged), array_name)
            .with_context(|| {
                format!(
                    "row {} (per-row add failed; row must be a JSON object with at minimum an `id` field)",
                    i + 1
                )
            })?;
    }
    Ok(rows.len())
}

/// Append `rows` to `array_name` with no defaults. Thin wrapper over
/// `items_add_many` so the `array-append` dispatch site (Task 5) stays a
/// one-liner.
pub(crate) fn array_append(
    doc: &mut TomlValue,
    array_name: &str,
    rows: &[JsonValue],
) -> Result<usize> {
    items_add_many(doc, array_name, rows, None)
}

/// T10: structural summary of what a mutation WOULD change, together with
/// the mutated `new_doc` ready for persistence. Produced by
/// `compute_apply_mutation` / `compute_remove_mutation` (pure: no I/O, no
/// lock, no sidecar), consumed by `apply_mutation` (I/O: lock + atomic
/// tempfile + sidecar).
///
/// `added`, `updated`, `removed` are the ids of items touched by the
/// operation in **input order**. For `items apply` with a batch that
/// sequentially adds R5, updates R1, removes R4, the vectors are
/// `added=["R5"], updated=["R1"], removed=["R4"]`. This is the structural
/// guarantee that underpins `--dry-run`'s `would_change` output — the
/// CLI layer serialises these vectors verbatim.
///
/// `skipped` reuses T5's `SkippedRow` and stays empty for the `compute_*`
/// paths in T10 (only the add-many-with-dedupe path populates it, and
/// that path is not covered by `--dry-run` yet — the dedupe decision is
/// inside the mutate closure rather than a pre-split plan).
#[derive(Debug, Clone)]
pub(crate) struct MutationPlan {
    pub(crate) new_doc: TomlValue,
    pub(crate) added: Vec<String>,
    pub(crate) updated: Vec<String>,
    pub(crate) removed: Vec<String>,
    /// Reserved for a future dedupe-aware apply path (T5's
    /// `items_add_many_with_dedupe` already populates a `Vec<SkippedRow>`
    /// today, but that lives on its own `AddManyOutcome`). Keeping the
    /// field here lets a later plan reuse the same `MutationPlan` shape
    /// without an API break. `compute_apply_mutation` /
    /// `compute_remove_mutation` leave it empty.
    pub(crate) skipped: Vec<SkippedRow>,
}

impl MutationPlan {
    /// T10: concatenation `[...added, ...updated, ...removed]` for the
    /// `--dry-run` summary's `ids` field. First-appearance order within
    /// each category is preserved because the three vectors are built
    /// in input-order by `compute_apply_mutation`.
    pub(crate) fn union_ids(&self) -> Vec<String> {
        let mut out =
            Vec::with_capacity(self.added.len() + self.updated.len() + self.removed.len());
        out.extend(self.added.iter().cloned());
        out.extend(self.updated.iter().cloned());
        out.extend(self.removed.iter().cloned());
        out
    }
}

/// T10: pure sibling of `items_apply_to_opts`. Clones `doc` into
/// `plan.new_doc`, runs the existing apply pipeline on the clone, and
/// records touched ids per op in input order. No lock, no sidecar, no
/// tempfile — the result is a `MutationPlan` the caller either hands to
/// `apply_mutation` (live path) or serialises for `--dry-run`.
///
/// Errors are byte-identical to the live path: `--no-remove` violation,
/// oversize-ops-count (bounded at the CLI layer via `MAX_OPS_PER_APPLY`
/// BEFORE this helper runs), bad op shapes, missing ids — every error
/// message surfaces at the same point in the computation it would on a
/// real run, just without touching the filesystem.
///
/// The add/update/remove id capture walks the parsed ops list once BEFORE
/// handing it off to `items_apply_parsed_to_opts` (which consumes the list by
/// value via `.into_iter()`). This keeps the capture cost O(ops) with no
/// structural change to the existing dispatch loop.
pub(crate) fn compute_apply_mutation(
    doc: &TomlValue,
    array_name: &str,
    ops: &JsonValue,
    no_remove: bool,
) -> Result<MutationPlan> {
    // R45: caller hands us an already-parsed `JsonValue`. The dispatch
    // layer already parsed the `--ops` payload to enforce
    // `MAX_OPS_PER_APPLY`; threading the parsed value through here (and
    // into `items_apply_parsed_to_opts` below) avoids a second / third
    // pass over the same bytes.
    let JsonValue::Array(arr) = ops else {
        bail!(
            "--ops must be a JSON array (e.g. [{{\"op\":\"update\",\"id\":\"R1\",\"json\":{{...}}}}]); got JSON {}",
            crate::convert::json_type_name(ops)
        );
    };
    // Walk the ops once to capture per-op ids BEFORE mutation. Update
    // and remove ops carry `id`; add ops carry `json.id`. An op that
    // doesn't declare an id still counts as "touched" (represented by
    // an empty string) so counts stay faithful to the live behaviour.
    let mut added: Vec<String> = Vec::new();
    let mut updated: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    for op in arr {
        let Some(obj) = op.as_object() else { continue };
        let op_name = obj.get("op").and_then(|v| v.as_str()).unwrap_or("");
        match op_name {
            "add" => {
                let id = obj
                    .get("json")
                    .and_then(|j| j.as_object())
                    .and_then(|o| o.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                added.push(id);
            }
            "update" => {
                let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                updated.push(id);
            }
            "remove" => {
                let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                removed.push(id);
            }
            _ => {}
        }
    }
    // Run the existing apply pipeline on a cloned doc so validation gates
    // (--no-remove, unknown ops, missing ids, dedup_id auto-populate via
    // items_add_value_to → apply_dedup_id_on_add, etc.) all fire with
    // byte-identical error surfaces. On error, `added/updated/removed`
    // above are discarded — the live path likewise would not have persisted.
    // R45: clone `ops` here (rather than at the call site) so the by-ref
    // signature stays ergonomic for the dispatch layer; the clone is over
    // the parsed JSON tree, not a re-parse of the source bytes.
    let mut new_doc = doc.clone();
    items_apply_parsed_to_opts(&mut new_doc, ops.clone(), array_name, no_remove)?;
    Ok(MutationPlan {
        new_doc,
        added,
        updated,
        removed,
        skipped: Vec::new(),
    })
}

/// T10: pure sibling of `items_remove_from`. Clones `doc`, runs the
/// remove on the clone, and records the removed id. Errors identically
/// to the live path (`no item with id = {id}`). `added` and `updated`
/// stay empty.
pub(crate) fn compute_remove_mutation(
    doc: &TomlValue,
    array_name: &str,
    id: &str,
) -> Result<MutationPlan> {
    let mut new_doc = doc.clone();
    items_remove_from(&mut new_doc, array_name, id)?;
    Ok(MutationPlan {
        new_doc,
        added: Vec::new(),
        updated: Vec::new(),
        removed: vec![id.to_string()],
        skipped: Vec::new(),
    })
}

/// T11: pure compute-phase helper for `items backfill-dedup-id`. Clones the
/// doc, walks every item in `array_name`, and populates `dedup_id` on any
/// item that lacks the field via `tier_b_fingerprint` over the same five
/// fingerprinted fields (`file`, `summary`, `severity`, `category`,
/// `symbol`) that `apply_dedup_id_on_add` uses at the add-path funnel.
///
/// Items that already carry a `dedup_id` (any non-null TOML value) are
/// preserved byte-for-byte — preservation is a hard contract. If a legacy
/// `dedup_id` is "wrong" (e.g. produced by an earlier fingerprint version),
/// the backfill subcommand is NOT the fix — the caller must explicitly
/// `items update --json '{"dedup_id":"..."}'` to rewrite it.
///
/// The returned plan records each newly-populated item's `id` in
/// `plan.updated` (in input order). Items lacking an `id` field also get
/// their fingerprint written but surface in `plan.updated` as an empty
/// string — mirroring `compute_apply_mutation`'s per-op id capture contract
/// so the CLI layer's `would_backfill` / `backfilled` count stays faithful
/// to the number of items actually touched. `plan.added` and `plan.removed`
/// stay empty.
///
/// **Kill-switch handling**: intentionally does NOT check
/// `dedup_id_disabled()` — the CLI dispatch layer handles that branch BEFORE
/// calling this helper, so callers that reach here want the backfill to run
/// regardless of env state. Keeping the check outside this function lets
/// the dispatch emit the documented `{"ok":true,"backfilled":0,"reason":
/// "disabled-by-env"}` shape without an extra layer of plumbing.
///
/// **Error surface**: errors identically to `items_array_mut` (bails when
/// the named array exists but isn't an array-of-tables). Empty-array (array
/// absent or zero items) returns an empty-`updated` plan, letting the
/// dispatch skip the write and emit `{"ok":true,"backfilled":0}`.
pub(crate) fn compute_backfill_mutation(
    doc: &TomlValue,
    array_name: &str,
) -> Result<MutationPlan> {
    let mut new_doc = doc.clone();
    let mut updated: Vec<String> = Vec::new();
    // `items_array_mut` auto-creates the array if missing; this keeps the
    // "empty ledger" path a trivial zero-item walk instead of a bail. Write
    // paths across the rest of this module rely on the same semantics.
    let arr = items_array_mut(&mut new_doc, array_name)?;
    for item in arr.iter_mut() {
        // Only table-shaped items participate. Scalar/array entries (unusual
        // in a [[items]] layout but structurally possible for an arbitrary
        // `--array` target) are skipped — they have no field to write into
        // and no fingerprinted fields to read.
        let Some(tbl) = item.as_table_mut() else {
            continue;
        };
        // Contract: preserve any existing `dedup_id` regardless of its value.
        // A present-but-empty-string `dedup_id` is still a deliberate caller
        // choice (T6b treats empty-string as "absent in patch" for the
        // RECOMPUTE decision, but not for backfill — backfill only ever
        // FILLS IN a missing field, never overwrites). If a legacy empty
        // value needs replacing, the caller uses `items update`.
        if tbl.contains_key("dedup_id") {
            continue;
        }
        // Use the TOML-side `tier_b_fingerprint_table` over the item table
        // so the backfill digest is byte-identical to what `find-duplicates
        // --tier B` produces. The JSON-side sibling in
        // `apply_dedup_id_on_add` is used at the add path because that path
        // starts from a JSON `Map` payload; here we have the parsed TOML
        // table directly, avoiding an intermediate `toml_to_json` clone.
        // R46: `_table` variant takes `&Table` directly, eliminating the
        // per-row `tbl.clone()` the previous `TomlValue::Table(...)` wrap
        // forced — backfill walks every item in the array, so the clone
        // scaled per-row.
        let fp = tier_b_fingerprint_table(tbl);
        let id = tbl
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        tbl.insert("dedup_id".to_string(), TomlValue::String(fp));
        updated.push(id);
    }
    Ok(MutationPlan {
        new_doc,
        added: Vec::new(),
        updated,
        removed: Vec::new(),
        skipped: Vec::new(),
    })
}

/// T5: aggregate result of a dedupe-aware `items add-many` batch. `added`
/// is the number of rows appended; `skipped_rows` is the per-row skip log
/// in INPUT ORDER (ascending by 1-indexed row number to match
/// `items_add_many`'s existing error-messages). `skipped_rows.len()` is
/// the "skipped" count — the CLI layer emits both so the JSON contract
/// stays explicit.
#[derive(Debug, Clone)]
pub(crate) struct AddManyOutcome {
    pub added: usize,
    pub skipped_rows: Vec<SkippedRow>,
}

/// T5: one entry per skipped row in the add-many batch. `row` is
/// **1-indexed** to match the error-message convention elsewhere in
/// `items_add_many` (`row N: must be a JSON object`). `matched_id` is the
/// `id` of the existing item that caused the skip.
#[derive(Debug, Clone)]
pub(crate) struct SkippedRow {
    pub row: usize,
    pub matched_id: String,
}

/// T5: dedupe-aware sibling of `items_add_many`. Empty `dedupe_fields`
/// replicates the existing semantics (append every row, return
/// `added == rows.len()`). A non-empty slice runs `find_dedupe_match` on
/// the current doc state before each row; rows that match an existing
/// item are skipped and recorded.
///
/// Atomicity: the caller holds the exclusive lock for the whole
/// operation via `mutate_doc` / `mutate_doc_conditional`, so the
/// pre-scan, the conditional append, and any subsequent pre-scans (each
/// of which observes freshly-added rows from earlier iterations) all
/// happen inside the same critical section. No concurrent writer can
/// slip an identical row between our scan and our append.
///
/// Intra-batch self-dedup: if two rows in the SAME payload would match
/// each other, the second one dedupes against the first because the
/// first has already been appended to `doc`. This is usually what the
/// caller wants (NDJSON with accidental repeats in one pipe).
pub(crate) fn items_add_many_with_dedupe(
    doc: &mut TomlValue,
    array_name: &str,
    rows: &[JsonValue],
    defaults: Option<&JsonValue>,
    dedupe_fields: &[String],
) -> Result<AddManyOutcome> {
    // R4: share the defaults + row-merge shape with `items_add_many`.
    let base = defaults_base(defaults)?;
    let mut added: usize = 0;
    let mut skipped_rows: Vec<SkippedRow> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let row_num = i + 1;
        let row_obj = row.as_object().ok_or_else(|| {
            anyhow!(
                "row {}: must be a JSON object (e.g. {{\"id\":\"R1\",\"summary\":\"...\"}}); got JSON {}",
                row_num,
                crate::convert::json_type_name(row)
            )
        })?;
        // Build the merged payload first so dedupe sees the same fields
        // `items_add_value_to` would otherwise persist.
        let merged_val = JsonValue::Object(merge_row_over_base(&base, row_obj));
        if !dedupe_fields.is_empty()
            && let Some(matched_id) = find_dedupe_match(doc, array_name, &merged_val, dedupe_fields)
        {
            skipped_rows.push(SkippedRow {
                row: row_num,
                matched_id,
            });
            continue;
        }
        items_add_value_to(doc, merged_val, array_name)
            .with_context(|| {
                format!(
                    "row {} (per-row dedupe-add failed; row must be a JSON object with at minimum an `id` field)",
                    row_num
                )
            })?;
        added += 1;
    }
    Ok(AddManyOutcome {
        added,
        skipped_rows,
    })
}

/// T10b: pure sibling of `items_add_to` / `items_add_value_to`. Clones the
/// doc, runs the existing add pipeline on the clone, and records the new
/// item's `id` (or empty string if the payload omitted one) in
/// `plan.added`. Errors are byte-identical to the live path
/// (`--json must be a JSON object`, date-coerce failures, etc.) since the
/// same `items_add_value_to` funnel is used.
///
/// `apply_dedup_id_on_add` runs inside `items_add_value_to`, so the
/// dry-run preview is byte-equivalent to a live add even when dedup_id
/// auto-population fires — both paths observe the same env state.
///
/// R47: takes a parsed `&JsonValue` so the dispatch layer's dedupe / no-dedupe
/// dry-run branches can share one parse path. Previously the no-dedupe arm
/// fed `&str` here while the dedupe arm parsed independently for
/// `compute_add_many_mutation` — the asymmetry forced stdin reads to differ
/// in cost between branches and made it easy to drift on the
/// `parsing --json` context message. Cloning the patch here (rather than
/// forcing the caller to hand ownership in) keeps the compute helper a
/// pure-by-ref function the tests can call multiple times on the same
/// `JsonValue` fixture.
pub(crate) fn compute_add_mutation(
    doc: &TomlValue,
    array_name: &str,
    patch: &JsonValue,
) -> Result<MutationPlan> {
    // Capture the patch's id (if any) BEFORE mutation so the plan reports
    // the same id the live path would persist. An add with no `id` field
    // surfaces as an empty string, mirroring `compute_apply_mutation`'s
    // contract for ad-hoc add ops.
    // R21: per-plan id capture funnels through `capture_row_id` so this
    // site, the no-dedupe / dedupe arms of `compute_add_many_mutation`,
    // and any future single-row capture share one `unwrap_or("")` rule.
    let id = capture_row_id(patch);
    let mut new_doc = doc.clone();
    // R47: clone here so the caller's borrow is preserved. `items_add_value_to`
    // consumes the patch by-value (O27 — feeds owned `(String, JsonValue)`
    // pairs into the merge loop without per-key clones); the price of a
    // single top-level Value clone here is dominated by the doc clone above.
    items_add_value_to(&mut new_doc, patch.clone(), array_name)?;
    Ok(MutationPlan {
        new_doc,
        added: vec![id],
        updated: Vec::new(),
        removed: Vec::new(),
        skipped: Vec::new(),
    })
}

/// T10b: pure sibling of `items_add_many` / `items_add_many_with_dedupe`.
/// Empty `dedupe_fields` runs `items_add_many` on a cloned doc; non-empty
/// runs `items_add_many_with_dedupe` and threads `AddManyOutcome.added`
/// into `plan.added` (with per-row ids captured from the merged payloads
/// on success) and `AddManyOutcome.skipped_rows` into `plan.skipped`.
///
/// `defaults` carries the same `--defaults-json` shape the live `items
/// add-many` accepts; omitting it would make the dry-run preview diverge
/// from the live path's stamping of default fields.
///
/// Per-row id capture: rows lacking an `id` field surface in `plan.added`
/// as an empty string, matching `compute_apply_mutation`'s convention.
/// On the dedupe path, only rows that successfully appended contribute
/// to `plan.added`; skipped rows surface in `plan.skipped` instead.
pub(crate) fn compute_add_many_mutation(
    doc: &TomlValue,
    array_name: &str,
    rows: &[JsonValue],
    defaults: Option<&JsonValue>,
    dedupe_fields: &[String],
) -> Result<MutationPlan> {
    let mut new_doc = doc.clone();
    if dedupe_fields.is_empty() {
        // Pre-capture ids from the input rows. `items_add_many` returns
        // only a count; we walk the rows once before delegating so the
        // plan can report ids in input order. A row that fails validation
        // inside `items_add_many` (non-object) errors before any partial
        // ids leak — the discarded `Result` is the same surface as the
        // live path.
        let mut added: Vec<String> = Vec::with_capacity(rows.len());
        for row in rows {
            // R21: shared `capture_row_id` keeps this no-dedupe arm in step
            // with `compute_add_mutation` and the dedupe arm below.
            added.push(capture_row_id(row));
        }
        let count = items_add_many(&mut new_doc, array_name, rows, defaults)?;
        // Defensive: trim to the number of rows actually appended so the
        // plan never claims more ids than the live path persisted.
        added.truncate(count);
        Ok(MutationPlan {
            new_doc,
            added,
            updated: Vec::new(),
            removed: Vec::new(),
            skipped: Vec::new(),
        })
    } else {
        // Capture per-row ids up front; we'll filter to only the rows
        // that actually appended after the dedupe outcome lands.
        // R21: shared `capture_row_id` — the dedupe arm uses the same
        // empty-on-missing convention as the no-dedupe arm above.
        let row_ids: Vec<String> = rows.iter().map(capture_row_id).collect();
        let outcome =
            items_add_many_with_dedupe(&mut new_doc, array_name, rows, defaults, dedupe_fields)?;
        // `outcome.skipped_rows` carries 1-indexed row numbers; the
        // remaining indices are the ones that appended in input order.
        // R44: HashSet kept for clarity even though `outcome.skipped_rows`
        // is sorted ascending (T5 contract above) and a two-pointer merge
        // would be O(n) without the hash overhead. The per-row `contains`
        // call below expresses the "did this index get skipped?" intent
        // more directly than threading a pointer through the filter, and
        // the typical batch size is < 1000 rows where the constant-factor
        // difference is dominated by the per-row TOML mutation cost.
        let skipped_set: std::collections::HashSet<usize> =
            outcome.skipped_rows.iter().map(|r| r.row).collect();
        let added: Vec<String> = row_ids
            .into_iter()
            .enumerate()
            .filter_map(|(i, id)| {
                if skipped_set.contains(&(i + 1)) {
                    None
                } else {
                    Some(id)
                }
            })
            .collect();
        // Defensive: the appended-id count must equal `outcome.added`.
        debug_assert_eq!(added.len(), outcome.added);
        Ok(MutationPlan {
            new_doc,
            added,
            updated: Vec::new(),
            removed: Vec::new(),
            skipped: outcome.skipped_rows,
        })
    }
}

/// T10b: pure sibling of `items_update_to` / `items_update_value_to`.
/// Clones the doc, runs the update pipeline on the clone, and records
/// the touched id in `plan.updated`. Errors are byte-identical to the
/// live path (`no item with id = {id}`, `--json must be a JSON object`,
/// date-coerce failures, etc.).
///
/// `apply_dedup_id_on_update` runs inside `items_update_value_to`, so
/// the dry-run preview is byte-equivalent to a live update even when
/// the dedup_id-recompute branch fires.
pub(crate) fn compute_update_mutation(
    doc: &TomlValue,
    array_name: &str,
    id: &str,
    json: &str,
    unset: &[String],
) -> Result<MutationPlan> {
    let mut new_doc = doc.clone();
    items_update_to(&mut new_doc, array_name, id, json, unset)?;
    Ok(MutationPlan {
        new_doc,
        added: Vec::new(),
        updated: vec![id.to_string()],
        removed: Vec::new(),
        skipped: Vec::new(),
    })
}

/// T10b: pure sibling of `array_append`. Thin forward to
/// `compute_add_many_mutation` with no defaults and no dedupe — the
/// `array-append` subcommand deliberately does not expose either.
pub(crate) fn compute_array_append_mutation(
    doc: &TomlValue,
    array_name: &str,
    rows: &[JsonValue],
) -> Result<MutationPlan> {
    compute_add_many_mutation(doc, array_name, rows, None, &[])
}

/// R19: typed error for disposition-specific required-field validation.
///
/// The `[[items]]` ledger schema couples `status` to a small cluster of
/// disposition-specific required fields (see `claude/commands/review.md`
/// `## Ledger Schema → Disposition-specific fields`). Today every read/write
/// path in this module reaches into `TomlValue::Table` directly, so a row
/// with `status = "deferred"` but missing `defer_reason` parses as valid
/// TOML and only surfaces as malformed at render time.
///
/// `Item::validate` (below) is the parse-time check that catches the
/// missing-disposition-field case. It is intentionally additive — no
/// existing call site is wired through it yet; callers opt in. The
/// `#[allow(dead_code)]` annotations match the pattern in `errors.rs`
/// for `ErrorKind` variants reserved for future wiring.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum DispositionError {
    /// The payload is not a JSON object — disposition validation only makes
    /// sense over an object payload.
    NotAnObject { got: &'static str },
    /// `status` is required for disposition-specific validation but absent
    /// or non-string. (An item without `status` is malformed under the
    /// schema; the field is required for every `[[items]]` row.)
    MissingStatus,
    /// `status` is set to a disposition value (`fixed`, `applied`,
    /// `deferred`, `wontfix`, `wontapply`, `verified-clean`) but one or
    /// more of its required companion fields is missing or empty.
    MissingDispositionField {
        status: String,
        field: &'static str,
    },
}

impl std::fmt::Display for DispositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject { got } => {
                write!(f, "item payload must be a JSON object; got {got}")
            }
            Self::MissingStatus => f.write_str("item is missing required field `status`"),
            Self::MissingDispositionField { status, field } => write!(
                f,
                "item with status=\"{status}\" is missing required field `{field}`"
            ),
        }
    }
}

impl std::error::Error for DispositionError {}

/// R19: zero-sized typed wrapper that exposes the disposition-required-field
/// check as `Item::validate`. The call sites in this module continue to
/// operate on `TomlValue::Table` / `JsonValue` directly — `Item` is a
/// namespace handle, not a parsed-row container — so adding this entry
/// point does not perturb the existing `items_*` flow.
#[allow(dead_code)]
pub(crate) struct Item;

#[allow(dead_code)]
impl Item {
    /// Validate that a JSON item payload satisfies the disposition-specific
    /// required-field cluster for its `status`. Returns `Ok(())` for the
    /// `open` status (no companion fields required) and for any unknown
    /// status (forward-compatible — new dispositions silently pass until
    /// the taxonomy here is widened). Items without a `status` field error
    /// with `MissingStatus`.
    ///
    /// Required-field clusters mirror `claude/commands/review.md`
    /// `## Ledger Schema → Disposition-specific fields`:
    ///   - `fixed` / `applied`           → `resolved`, `resolution`
    ///   - `deferred`                    → `defer_reason`, `defer_trigger`
    ///   - `wontfix` / `wontapply`       → `wontfix_rationale`
    ///   - `verified-clean`              → `verified_note`
    ///
    /// "Missing" means absent OR present with an empty value (`""`, `[]`,
    /// `null`) — matching `is_empty_json` semantics so a placeholder field
    /// the agent never filled in is detected as a gap rather than papered
    /// over. `resolved` (a date) is required to be present and non-null;
    /// `is_empty_json` returns `false` for non-string non-null types so a
    /// JSON date string or number passes through.
    pub(crate) fn validate(value: &JsonValue) -> std::result::Result<(), DispositionError> {
        let JsonValue::Object(map) = value else {
            return Err(DispositionError::NotAnObject {
                got: json_type_name(value),
            });
        };
        let status = map
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or(DispositionError::MissingStatus)?;
        let required: &[&'static str] = match status {
            "fixed" | "applied" => &["resolved", "resolution"],
            "deferred" => &["defer_reason", "defer_trigger"],
            "wontfix" | "wontapply" => &["wontfix_rationale"],
            "verified-clean" => &["verified_note"],
            // "open" and any forward-compat unknown status: no companion fields.
            _ => &[],
        };
        for field in required {
            let present = map.get(*field).is_some_and(|v| !is_empty_json(v));
            if !present {
                return Err(DispositionError::MissingDispositionField {
                    status: status.to_string(),
                    field,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use crate::convert::{DATE_KEYS, ScalarType, infer_type, json_to_toml, navigate, set_at_path};
    use crate::query::{self, Predicate, Query};

    const LEDGER: &str = r#"schema_version = 1
last_updated = 2026-04-16

[[items]]
id = "R1"
file = "src/a.rs"
line = 10
severity = "warning"
effort = "small"
category = "quality"
summary = "foo"
first_flagged = 2026-04-08
rounds = 1
status = "open"

[[items]]
id = "R4"
file = "src/b.rs"
line = 20
severity = "critical"
effort = "small"
category = "quality"
summary = "bar"
first_flagged = 2026-04-08
rounds = 1
status = "fixed"
resolved = 2026-04-08
resolution = "fix in abc123"
"#;

    const CONTEXT: &str = r#"slug = "x"
plan_path = "docs/plans/x.md"
status = "draft"
created = 2026-04-08
updated = 2026-04-08
scope = ["src/**"]

[tasks]
total = 3
completed = 0
in_progress = 0

[artifacts]
review_ledger = ".claude/flows/x/review-ledger.toml"
optimise_findings = ".claude/flows/x/optimise-findings.toml"
"#;

    fn ctx() -> TomlValue {
        toml::from_str(CONTEXT).unwrap()
    }
    fn led() -> TomlValue {
        toml::from_str(LEDGER).unwrap()
    }

    /// Small helper: run a filter-only query against `doc` and return the
    /// resulting items as a `Vec<JsonValue>`. Unwraps the Array-shape output
    /// for the tests below (R70: migrated from the retired legacy
    /// `items_list(...) / ListFilters` path so we can delete both).
    fn run_filter_query(doc: &TomlValue, preds: Vec<Predicate>) -> Vec<JsonValue> {
        let q = Query {
            predicates: preds,
            ..Default::default()
        };
        match query::run(doc, "items", &q).expect("query succeeds") {
            JsonValue::Array(a) => a,
            other => panic!("expected array shape, got {other:?}"),
        }
    }

    #[test]
    fn navigate_finds_nested_value() {
        let doc = ctx();
        assert_eq!(
            navigate(&doc, "tasks.total").and_then(|v| v.as_integer()),
            Some(3)
        );
        assert_eq!(
            navigate(&doc, "artifacts.review_ledger").and_then(|v| v.as_str()),
            Some(".claude/flows/x/review-ledger.toml")
        );
        assert!(navigate(&doc, "missing.path").is_none());
    }

    #[test]
    fn navigate_indexes_into_array_with_integer_segment() {
        // R49: `items.0.status` walks through the [[items]] array-of-tables,
        // selects index 0, and reads its `status`. Out-of-bounds yields None.
        let doc = led();
        let first_status = navigate(&doc, "items.0.status").and_then(|v| v.as_str());
        assert_eq!(first_status, Some("open"));
        let second_status = navigate(&doc, "items.1.status").and_then(|v| v.as_str());
        assert_eq!(second_status, Some("fixed"));
        // Out-of-bounds and non-numeric segments return None.
        assert!(navigate(&doc, "items.99.status").is_none());
        assert!(navigate(&doc, "items.oops.status").is_none());
    }

    #[test]
    fn set_at_path_preserves_unrelated_fields_and_created() {
        let mut doc = ctx();
        set_at_path(&mut doc, "status", TomlValue::String("review".into())).unwrap();
        set_at_path(&mut doc, "tasks.completed", TomlValue::Integer(2)).unwrap();
        assert_eq!(
            navigate(&doc, "status").and_then(|v| v.as_str()),
            Some("review")
        );
        assert_eq!(
            navigate(&doc, "tasks.completed").and_then(|v| v.as_integer()),
            Some(2)
        );
        assert_eq!(
            navigate(&doc, "created").and_then(|v| v.as_datetime()).map(|d| d.to_string()),
            Some("2026-04-08".into())
        );
        assert_eq!(
            navigate(&doc, "slug").and_then(|v| v.as_str()),
            Some("x")
        );
    }

    #[test]
    fn set_json_replaces_array() {
        let mut doc = ctx();
        let patch: JsonValue = serde_json::from_str(r#"["a/**", "b/**"]"#).unwrap();
        let v = json_to_toml(&patch).unwrap();
        set_at_path(&mut doc, "scope", v).unwrap();
        let scope: Vec<&str> = navigate(&doc, "scope")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(scope, vec!["a/**", "b/**"]);
    }

    #[test]
    fn infer_type_distinguishes_date_int_bool_string() {
        assert!(matches!(infer_type("2026-04-17"), ScalarType::Date));
        assert!(matches!(infer_type("42"), ScalarType::Int));
        assert!(matches!(infer_type("true"), ScalarType::Bool));
        assert!(matches!(infer_type("false"), ScalarType::Bool));
        assert!(matches!(infer_type("review"), ScalarType::Str));
        assert!(matches!(infer_type("2026-4-1"), ScalarType::Str));
    }

    #[test]
    fn items_list_filters_by_status() {
        let doc = led();
        let open = run_filter_query(
            &doc,
            vec![Predicate::Where {
                key: "status".into(),
                rhs: "open".into(),
            }],
        );
        assert_eq!(open.len(), 1);
        assert_eq!(open[0]["id"], "R1");
        let fixed = run_filter_query(
            &doc,
            vec![Predicate::Where {
                key: "status".into(),
                rhs: "fixed".into(),
            }],
        );
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0]["id"], "R4");
    }

    #[test]
    fn items_add_promotes_iso_date_strings_to_datetime() {
        let mut doc = led();
        items_add(
            &mut doc,
            r#"{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        let item = items_get(&doc, "R5").unwrap();
        assert_eq!(item["first_flagged"], "2026-04-17");
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains("first_flagged = 2026-04-17"),
            "expected raw TOML date literal, got:\n{serialised}"
        );
    }

    #[test]
    fn items_update_merges_patch() {
        let mut doc = led();
        items_update(
            &mut doc,
            "R1",
            r#"{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}"#,
            &[],
        )
        .unwrap();
        let item = items_get(&doc, "R1").unwrap();
        assert_eq!(item["status"], "fixed");
        assert_eq!(item["rounds"], 2);
        assert_eq!(item["resolved"], "2026-04-17");
        assert_eq!(item["summary"], "foo", "unrelated field must be preserved");
    }

    #[test]
    fn items_remove_drops_matching_item() {
        let mut doc = led();
        items_remove(&mut doc, "R1").unwrap();
        assert!(items_get(&doc, "R1").is_err());
        assert!(items_get(&doc, "R4").is_ok());
        assert!(items_remove(&mut doc, "R999").is_err());
    }

    #[test]
    fn items_next_id_respects_max_and_prefix() {
        let doc = led();
        assert_eq!(items_next_id(&doc, "R").unwrap(), "R5");
        assert_eq!(items_next_id(&doc, "O").unwrap(), "O1");
    }

    #[test]
    fn items_next_id_rejects_empty_prefix() {
        let doc = led();
        let err = items_next_id(&doc, "").unwrap_err();
        assert!(
            err.to_string().contains("prefix must not be empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn items_next_id_rejects_numeric_prefix() {
        let doc = led();
        let err = items_next_id(&doc, "123").unwrap_err();
        assert!(
            err.to_string().contains("prefix must not be all-digit"),
            "unexpected error: {err}"
        );
        // Single digit should also be rejected.
        assert!(items_next_id(&doc, "1").is_err());
    }

    #[test]
    fn roundtrip_preserves_datetime_and_key_order() {
        let doc = ctx();
        let s = toml::to_string_pretty(&doc).unwrap();
        assert!(s.contains("created = 2026-04-08"));
        let slug_pos = s.find("slug =").unwrap();
        let status_pos = s.find("status =").unwrap();
        assert!(slug_pos < status_pos);
    }

    #[test]
    fn json_to_toml_rejects_null() {
        let v: JsonValue = serde_json::from_str("null").unwrap();
        assert!(json_to_toml(&v).is_err());
    }

    #[test]
    fn date_keys_roundtrip_as_toml_datetime() {
        // R45: exhaustive pin — every entry in DATE_KEYS must round-trip from
        // an ISO-date JSON string through `maybe_date_coerce` into a TOML
        // `Datetime`. If a key is removed from DATE_KEYS or mistyped, this
        // test fails with the offending key named in the assertion message.
        for key in DATE_KEYS {
            let v = JsonValue::String("2026-04-18".into());
            let coerced = maybe_date_coerce(key, &v)
                .unwrap_or_else(|e| panic!("{key}: coerce failed: {e}"));
            match coerced {
                TomlValue::Datetime(dt) => {
                    assert_eq!(dt.to_string(), "2026-04-18", "{key} produced wrong dt");
                }
                other => panic!("DATE_KEYS entry {key} did not coerce to Datetime: {other:?}"),
            }
        }
    }

    #[test]
    fn items_add_does_not_coerce_non_date_keys() {
        let mut doc = led();
        items_add(
            &mut doc,
            r#"{"id":"R99","file":"2026-04-17","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"file name shaped like a date","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        let item = items_get(&doc, "R99").unwrap();
        assert_eq!(item["file"], "2026-04-17");
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains(r#"file = "2026-04-17""#),
            "expected quoted string for non-date key, got:\n{serialised}"
        );
        assert!(
            serialised.contains("first_flagged = 2026-04-17"),
            "expected date literal for date key, got:\n{serialised}"
        );
    }

    #[test]
    fn items_apply_runs_batch_atomically() {
        let batch_ops = r#"[
            {"op":"add","json":{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}},
            {"op":"update","id":"R1","json":{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}},
            {"op":"remove","id":"R4"}
        ]"#;

        let mut doc_batch = led();
        items_apply(&mut doc_batch, batch_ops).unwrap();

        let mut doc_seq = led();
        items_add(
            &mut doc_seq,
            r#"{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        items_update(
            &mut doc_seq,
            "R1",
            r#"{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}"#,
            &[],
        )
        .unwrap();
        items_remove(&mut doc_seq, "R4").unwrap();

        let s_batch = toml::to_string_pretty(&doc_batch).unwrap();
        let s_seq = toml::to_string_pretty(&doc_seq).unwrap();
        assert_eq!(s_batch, s_seq);
    }

    // ----- items update --unset -------------------------------------------

    #[test]
    fn items_update_unset_removes_field() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
status = "deferred"
defer_reason = "blocked"
defer_trigger = "when channel lands"
summary = "something"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        items_update(
            &mut doc,
            "R1",
            r#"{"status":"open"}"#,
            &["defer_trigger".into(), "defer_reason".into()],
        )
        .unwrap();
        let item = items_get(&doc, "R1").unwrap();
        assert_eq!(item["status"], "open");
        assert!(item.get("defer_reason").is_none());
        assert!(item.get("defer_trigger").is_none());
        assert_eq!(item["summary"], "something");

        // No-op for absent key is fine.
        items_update(
            &mut doc,
            "R1",
            r#"{}"#,
            &["nonexistent_key".into()],
        )
        .unwrap();
    }

    #[test]
    fn items_apply_unset_respected_in_batch() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
status = "deferred"
defer_reason = "blocked"
defer_trigger = "when x lands"
summary = "foo"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        items_apply(
            &mut doc,
            r#"[{"op":"update","id":"R1","json":{"status":"open"},"unset":["defer_reason","defer_trigger"]}]"#,
        )
        .unwrap();
        let item = items_get(&doc, "R1").unwrap();
        assert_eq!(item["status"], "open");
        assert!(item.get("defer_reason").is_none());
        assert!(item.get("defer_trigger").is_none());

        // Missing `unset` in a batch op stays back-compat (no-op, no error).
        items_apply(
            &mut doc,
            r#"[{"op":"update","id":"R1","json":{"rounds":2}}]"#,
        )
        .unwrap();
    }

    // ----- items list filters ---------------------------------------------

    #[test]
    fn items_list_filters_combine_with_and() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
category = "quality"
summary = "a"
first_flagged = 2026-04-05
status = "open"

[[items]]
id = "R2"
file = "src/b.rs"
category = "quality"
summary = "b"
first_flagged = 2026-04-15
status = "open"

[[items]]
id = "R3"
file = "src/b.rs"
category = "security"
summary = "c"
first_flagged = 2026-04-15
status = "open"

[[items]]
id = "R4"
file = "src/b.rs"
category = "quality"
summary = "d"
first_flagged = 2026-04-15
status = "fixed"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        // status=open AND category=quality AND first_flagged > 2026-04-10.
        // The `@date:` prefix on the WhereGt RHS mirrors the
        // CLI-layer `--newer-than` translation in `build_query` so we cover
        // the same code path as the production path does.
        let result = run_filter_query(
            &doc,
            vec![
                Predicate::Where {
                    key: "status".into(),
                    rhs: "open".into(),
                },
                Predicate::Where {
                    key: "category".into(),
                    rhs: "quality".into(),
                },
                Predicate::WhereGt {
                    key: "first_flagged".into(),
                    rhs: "@date:2026-04-10".into(),
                },
            ],
        );
        assert_eq!(result.len(), 1, "expected exactly one item, got {result:?}");
        assert_eq!(result[0]["id"], "R2");
    }

    #[test]
    fn items_list_file_filter_matches_exactly() {
        let doc = led();
        let result = run_filter_query(
            &doc,
            vec![Predicate::Where {
                key: "file".into(),
                rhs: "src/a.rs".into(),
            }],
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["id"], "R1");
    }

    #[test]
    fn items_list_newer_than_rejects_bad_date() {
        // Parsing is delegated to the CLI arg handler, which re-uses
        // `toml::value::Datetime::from_str`. Validate that directly.
        let err = "not-a-date".parse::<toml::value::Datetime>().unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    // ----- R1: items list --count -----------------------------------------

    #[test]
    fn items_list_count_matches_filter() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
summary = "a"

[[items]]
id = "R2"
status = "open"
summary = "b"

[[items]]
id = "R3"
status = "fixed"
summary = "c"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let open = run_filter_query(
            &doc,
            vec![Predicate::Where {
                key: "status".into(),
                rhs: "open".into(),
            }],
        );
        // Simulate the dispatch wrapping: count == list.len() for the same filter.
        assert_eq!(open.len(), 2);
        // And a manual-count sanity check using a different filter.
        let fixed = run_filter_query(
            &doc,
            vec![Predicate::Where {
                key: "status".into(),
                rhs: "fixed".into(),
            }],
        );
        assert_eq!(fixed.len(), 1);
    }

    // ----- R57: items add/update/remove/list/get --array ------------------

    #[test]
    fn items_add_to_custom_array_appends_without_touching_items() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
summary = "existing"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        items_add_to(
            &mut doc,
            "rollback_events",
            r#"{"timestamp":"2026-04-18T00:00:00Z","command":"review-apply","cause":"test-R57","items":["R1"],"stash_ref":"stash@{0}"}"#,
        )
        .unwrap();

        // rollback_events has one entry; items untouched.
        let events = doc
            .get("rollback_events")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].as_table().unwrap().get("cause").unwrap().as_str(),
            Some("test-R57")
        );
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn items_update_remove_list_get_honour_custom_array() {
        let src = r#"schema_version = 1

[[items]]
id = "I1"
status = "open"

[[audit]]
id = "A1"
status = "pending"
detail = "one"

[[audit]]
id = "A2"
status = "pending"
detail = "two"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();

        // query::run on `audit` with status=pending returns only the audit
        // rows — the adjacent `[[items]]` row must not leak into the result.
        let q = Query {
            predicates: vec![Predicate::Where {
                key: "status".into(),
                rhs: "pending".into(),
            }],
            ..Default::default()
        };
        let list = match query::run(&doc, "audit", &q).unwrap() {
            JsonValue::Array(a) => a,
            other => panic!("expected array, got {other:?}"),
        };
        assert_eq!(list.len(), 2);

        // items_get_from fetches by id from the named array.
        let got = items_get_from(&doc, "audit", "A1").unwrap();
        assert_eq!(got["detail"], "one");
        assert!(items_get_from(&doc, "audit", "I1").is_err());

        // items_update_to mutates the named array's record, not `items`.
        items_update_to(&mut doc, "audit", "A1", r#"{"status":"closed"}"#, &[]).unwrap();
        assert_eq!(items_get_from(&doc, "audit", "A1").unwrap()["status"], "closed");
        assert_eq!(items_get_from(&doc, "items", "I1").unwrap()["status"], "open");

        // items_remove_from drops from the named array only.
        items_remove_from(&mut doc, "audit", "A2").unwrap();
        let remaining_audit = doc.get("audit").and_then(|v| v.as_array()).unwrap();
        assert_eq!(remaining_audit.len(), 1);
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
    }

    // ----- R14: items apply --array ---------------------------------------

    #[test]
    fn items_apply_to_custom_array_appends_without_touching_items() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
summary = "existing"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        let ops = r#"[{"op":"add","json":{"timestamp":"2026-04-18T00:00:00Z","command":"review-apply","cause":"test","items":["R1"],"stash_ref":"stash@{0}"}}]"#;
        items_apply_to(&mut doc, ops, "rollback_events").unwrap();

        // `rollback_events` now has one entry.
        let events = doc
            .get("rollback_events")
            .and_then(|v| v.as_array())
            .expect("rollback_events array");
        assert_eq!(events.len(), 1);
        let evt = events[0].as_table().unwrap();
        assert_eq!(evt.get("command").unwrap().as_str(), Some("review-apply"));
        assert_eq!(evt.get("cause").unwrap().as_str(), Some("test"));

        // [[items]] is untouched — still exactly the single pre-existing entry.
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].as_table().unwrap().get("id").unwrap().as_str(),
            Some("R1")
        );
    }

    // ----- R37: items apply --no-remove -----------------------------------

    #[test]
    fn items_apply_no_remove_rejects_remove_op() {
        let mut doc = led();
        // Without the flag, a remove op succeeds.
        items_apply(
            &mut doc,
            r#"[{"op":"remove","id":"R1"}]"#,
        )
        .unwrap();
        // Target reset.
        let mut doc2 = led();
        // With --no-remove, the same op errors before any mutation.
        let err = items_apply_to_opts(
            &mut doc2,
            r#"[
                {"op":"update","id":"R1","json":{"status":"fixed"}},
                {"op":"remove","id":"R4"}
            ]"#,
            "items",
            true,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("remove op"), "expected remove-op rejection, got: {msg}");
        assert!(msg.contains("op[1]"), "expected index in error, got: {msg}");
        // Confirm no partial mutation: R1 still `open`, R4 still present.
        assert_eq!(items_get(&doc2, "R1").unwrap()["status"], "open");
        assert!(items_get(&doc2, "R4").is_ok());
    }

    // ----- O18: indexed apply fast-path -----------------------------------

    /// Pin the O18 indexed-apply path's correctness: a batch with > 5
    /// `update` ops triggers the HashMap-backed dispatch, and `add` /
    /// `remove` interleaved with updates must still produce the same
    /// final document as a batch the linear-scan path would produce.
    #[test]
    fn items_apply_indexed_path_matches_linear_for_large_batch() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"

[[items]]
id = "R2"
status = "open"

[[items]]
id = "R3"
status = "open"

[[items]]
id = "R4"
status = "open"

[[items]]
id = "R5"
status = "open"

[[items]]
id = "R6"
status = "open"

[[items]]
id = "R7"
status = "open"
"#;
        // 7 updates (> ID_INDEX_BUILD_THRESHOLD = 5) trigger the indexed
        // path. Plus an add and a remove to exercise the post-add map
        // bump and post-remove map invalidation.
        let ops = r#"[
            {"op":"update","id":"R1","json":{"status":"fixed"}},
            {"op":"update","id":"R2","json":{"status":"fixed"}},
            {"op":"update","id":"R3","json":{"status":"fixed"}},
            {"op":"update","id":"R4","json":{"status":"fixed"}},
            {"op":"add","json":{"id":"R8","status":"open"}},
            {"op":"remove","id":"R5"},
            {"op":"update","id":"R6","json":{"status":"fixed"}},
            {"op":"update","id":"R8","json":{"status":"fixed"}},
            {"op":"update","id":"R7","json":{"status":"fixed"}}
        ]"#;
        let mut doc_indexed: TomlValue = toml::from_str(src).unwrap();
        items_apply(&mut doc_indexed, ops).unwrap();

        // Build the expected end state by replaying the same ops sequentially
        // through the per-op helpers (which take the linear-scan path).
        let mut doc_linear: TomlValue = toml::from_str(src).unwrap();
        items_update(&mut doc_linear, "R1", r#"{"status":"fixed"}"#, &[]).unwrap();
        items_update(&mut doc_linear, "R2", r#"{"status":"fixed"}"#, &[]).unwrap();
        items_update(&mut doc_linear, "R3", r#"{"status":"fixed"}"#, &[]).unwrap();
        items_update(&mut doc_linear, "R4", r#"{"status":"fixed"}"#, &[]).unwrap();
        items_add(&mut doc_linear, r#"{"id":"R8","status":"open"}"#).unwrap();
        items_remove(&mut doc_linear, "R5").unwrap();
        items_update(&mut doc_linear, "R6", r#"{"status":"fixed"}"#, &[]).unwrap();
        items_update(&mut doc_linear, "R8", r#"{"status":"fixed"}"#, &[]).unwrap();
        items_update(&mut doc_linear, "R7", r#"{"status":"fixed"}"#, &[]).unwrap();

        assert_eq!(
            toml::to_string_pretty(&doc_indexed).unwrap(),
            toml::to_string_pretty(&doc_linear).unwrap(),
            "indexed-apply path must produce byte-identical output to linear-scan path"
        );
    }

    /// O18: an `update` op for an unknown id under the indexed path must
    /// surface the same `no item with id = X` error as the linear-scan
    /// path does, so callers that rely on the error message keep working.
    #[test]
    fn items_apply_indexed_path_rejects_unknown_update_id() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"

[[items]]
id = "R2"
status = "open"
"#;
        // 6 updates push us over the threshold. Last update targets a
        // missing id; expect the same error message the linear path emits.
        let ops = r#"[
            {"op":"update","id":"R1","json":{"status":"fixed"}},
            {"op":"update","id":"R1","json":{"status":"fixed"}},
            {"op":"update","id":"R1","json":{"status":"fixed"}},
            {"op":"update","id":"R1","json":{"status":"fixed"}},
            {"op":"update","id":"R1","json":{"status":"fixed"}},
            {"op":"update","id":"DOES_NOT_EXIST","json":{"status":"fixed"}}
        ]"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        let err = items_apply(&mut doc, ops).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no item with id = DOES_NOT_EXIST"),
            "expected unknown-id error, got: {msg}"
        );
    }

    // ----- Audit-ledger error-message rewrites (Phase 2 T6c): enum-rejection
    // and state-precondition coverage on the categories T5 couldn't cover
    // from its 4 files. -----------------------------------------------------

    /// Audit-ledger row items.rs:688/815: an unknown op token must enumerate
    /// the valid set in the message so an agent's malformed apply payload
    /// gets a directed remediation instead of an opaque "unknown op" line.
    #[test]
    fn error_message_enum_rejection_lists_valid_ops() {
        let src = r#"schema_version = 1
[[items]]
id = "R1"
status = "open"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        // A single bad-op batch flows through `apply_single_op` (linear path)
        // because the update_count is below the O18 threshold.
        let ops = r#"[{"op":"frobnicate","id":"R1","json":{"status":"fixed"}}]"#;
        let err = items_apply(&mut doc, ops).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("unknown op `frobnicate`"),
            "expected unknown-op echo of the bad token, got: {msg}"
        );
        assert!(
            msg.contains("expected one of: add, update, remove"),
            "expected enumeration of the valid op set, got: {msg}"
        );
    }

    /// Audit-ledger row items.rs:198/224/483/642/737/740/742/830: a missing
    /// item id must include the `tomlctl items list ... --pluck id`
    /// discovery hint so the caller can enumerate the available ids without
    /// digging into the source file.
    #[test]
    fn error_message_state_precondition_suggests_discovery_command() {
        let src = r#"schema_version = 1
[[items]]
id = "R1"
status = "open"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        // `items_remove_from` (and every other "no item with id" site)
        // routes through the same prose. Pick the simplest funnel that
        // exercises the rewrite.
        let err = items_remove_from(&mut doc, "items", "R999").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no item with id = R999"),
            "expected missing-id echo, got: {msg}"
        );
        assert!(
            msg.contains("tomlctl items list"),
            "expected discovery-command suggestion, got: {msg}"
        );
        assert!(
            msg.contains("--pluck id"),
            "expected `--pluck id` hint for enumeration, got: {msg}"
        );
    }

    /// R28: audit-ledger row items.rs:266 (and parallel sites at 142, 184,
    /// 219, 261, 320 — every funnel that demands a JSON object payload):
    /// when the caller hands a non-object JSON value (array, number, string,
    /// bool, null) the error must enumerate the actual JSON type so the
    /// caller can correct the shape mismatch without re-reading the source.
    /// The Phase 2 T6c partial-enum class accounted for ~25 of ~70
    /// audit-ledger rewrite rows yet had zero unit-level coverage; this
    /// test pins the convention end-to-end through the `items_add_to`
    /// funnel (which feeds `items_add_value_to` at the line cited).
    #[test]
    fn error_message_partial_enum_rewrite_echoes_actual_json_type() {
        let src = r#"schema_version = 1
[[items]]
id = "R1"
status = "open"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();

        // Sub-case A: JSON array (most common agent-mistake — wrapping the
        // intended payload in a redundant outer array).
        let err = items_add_to(&mut doc, "items", r#"[{"id":"R2"}]"#).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("--json must be a JSON object"),
            "expected the `--json must be a JSON object` lead-in, got: {msg}"
        );
        assert!(
            msg.contains("got JSON array"),
            "expected the actual JSON type echoed (`array`) so the caller can spot the shape mismatch, got: {msg}"
        );

        // Sub-case B: JSON scalar (number) — the rewrite must echo
        // `number` rather than the prior opaque "expected object" text.
        let err = items_add_to(&mut doc, "items", "42").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("got JSON number"),
            "expected `got JSON number` for scalar payload, got: {msg}"
        );

        // Sub-case C: JSON string — pins that the enumeration covers
        // every JSON type, not just array/number. Same site as A/B
        // (`json_type_name` produces a stable lowercase token per type).
        let err = items_add_to(&mut doc, "items", r#""not an object""#).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("got JSON string"),
            "expected `got JSON string` for string payload, got: {msg}"
        );
    }

    // ----- Task 4: items_infer_and_next_id --------------------------------

    #[test]
    fn items_infer_and_next_id_single_prefix_returns_max_plus_one() {
        // Led() carries two R-prefixed rows (R1, R4). Inference should pick
        // `R` as the sole prefix and hand off to items_next_id → "R5".
        let doc = led();
        assert_eq!(items_infer_and_next_id(&doc).unwrap(), "R5");
    }

    #[test]
    fn items_infer_and_next_id_picks_non_r_prefix() {
        // E-only fixture: pin that inference isn't hard-coded to R.
        let src = r#"schema_version = 1

[[items]]
id = "E1"
summary = "first"

[[items]]
id = "E2"
summary = "second"

[[items]]
id = "E5"
summary = "out of order"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        assert_eq!(items_infer_and_next_id(&doc).unwrap(), "E6");
    }

    #[test]
    fn items_infer_and_next_id_empty_ledger_errors() {
        let doc: TomlValue = toml::from_str("schema_version = 1\n").unwrap();
        let err = items_infer_and_next_id(&doc).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--infer-from-file requires a non-empty ledger or explicit --prefix"),
            "expected canonical empty-ledger error prose, got: {msg}"
        );
    }

    #[test]
    fn items_infer_and_next_id_multiple_prefixes_errors_alpha_sorted() {
        // Deliberately insert out of alphabetical order (R, E, F) — the error
        // message must still list them sorted (E, F, R) so the output is
        // deterministic regardless of ledger row order.
        let src = r#"schema_version = 1

[[items]]
id = "R1"
summary = "review"

[[items]]
id = "E2"
summary = "execution"

[[items]]
id = "F3"
summary = "future"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let err = items_infer_and_next_id(&doc).unwrap_err();
        assert_eq!(
            err.to_string(),
            "--infer-from-file found multiple prefixes (E, F, R); pass --prefix explicitly"
        );
    }

    #[test]
    fn items_infer_and_next_id_skips_digit_leading_and_no_digit_ids() {
        // Malformed ids that can't participate in {prefix}{n} must not
        // contribute to the prefix set: `123` (all digits, empty prefix),
        // `xyz` (no digit). The E-prefixed id is the only legitimate entry;
        // inference picks E.
        let src = r#"schema_version = 1

[[items]]
id = "123"
summary = "digit-only, empty prefix"

[[items]]
id = "xyz"
summary = "no digit"

[[items]]
id = "E7"
summary = "legit"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        assert_eq!(items_infer_and_next_id(&doc).unwrap(), "E8");
    }

    #[test]
    fn items_infer_and_next_id_extracts_multichar_prefix() {
        // Not all prefixes are single chars. `DF` (from a hypothetical
        // "design-finding" schema) must come out whole — prefix extraction
        // splits at the FIRST digit, not after one character.
        let src = r#"schema_version = 1

[[items]]
id = "DF1"
summary = "design finding"

[[items]]
id = "DF4"
summary = "design finding"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        assert_eq!(items_infer_and_next_id(&doc).unwrap(), "DF5");
    }

    // ----- R19: items_next_id on empty doc --------------------------------

    #[test]
    fn items_next_id_on_empty_doc_returns_prefix_one() {
        // Stand-in for a ledger that exists but has no items yet. The
        // handler's pre-existence check in main.rs covers the "file missing"
        // case without invoking items_next_id at all; this test pins the
        // direct-call behaviour for an empty doc.
        let empty: TomlValue = toml::from_str("schema_version = 1\n").unwrap();
        assert_eq!(items_next_id(&empty, "R").unwrap(), "R1");
    }

    // ----- Task 2: items add-many + array-append helpers ------------------

    #[test]
    fn items_add_many_merges_defaults() {
        let mut doc = led();
        let defaults: JsonValue = serde_json::from_str(
            r#"{"status":"open","rounds":1,"severity":"warning"}"#,
        )
        .unwrap();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"id":"R10","file":"a.rs","line":1,"summary":"a","category":"quality","effort":"small","first_flagged":"2026-04-18"}"#).unwrap(),
            serde_json::from_str(r#"{"id":"R11","file":"b.rs","line":2,"summary":"b","category":"quality","effort":"small","first_flagged":"2026-04-18","severity":"critical"}"#).unwrap(),
        ];
        let n = items_add_many(&mut doc, "items", &rows, Some(&defaults)).unwrap();
        assert_eq!(n, 2);
        let r10 = items_get(&doc, "R10").unwrap();
        assert_eq!(r10["status"], "open");
        assert_eq!(r10["rounds"], 1);
        assert_eq!(r10["severity"], "warning");
        let r11 = items_get(&doc, "R11").unwrap();
        // Per-row severity wins over default.
        assert_eq!(r11["severity"], "critical");
        // Default still stamps non-conflicting fields.
        assert_eq!(r11["status"], "open");
    }

    #[test]
    fn items_add_many_rejects_non_object_row() {
        let mut doc = led();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"id":"R10","file":"a.rs","line":1,"summary":"a","category":"quality","effort":"small","severity":"warning","first_flagged":"2026-04-18","rounds":1,"status":"open"}"#).unwrap(),
            serde_json::from_str(r#"[1,2]"#).unwrap(),
        ];
        let err = items_add_many(&mut doc, "items", &rows, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("row 2"),
            "expected error to mention row 2, got: {msg}"
        );
    }

    #[test]
    fn items_add_many_rejects_id_less_ledger_row() {
        // Regression: an `items` batch whose rows omit `id` (e.g. an agent
        // that put non-id fields in --defaults-json but forgot to thread the
        // minted ids into the NDJSON rows) must fail loudly at write time,
        // not silently land unaddressable rows. The bad row is row 2; row 1
        // carries an id and would otherwise have been appended.
        let mut doc = led();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"id":"R10","summary":"has id"}"#).unwrap(),
            serde_json::from_str(r#"{"summary":"no id","severity":"warning"}"#).unwrap(),
        ];
        let err = items_add_many(&mut doc, "items", &rows, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("non-empty string `id`"),
            "expected id-required error, got: {msg}"
        );
    }

    #[test]
    fn items_add_value_to_allows_id_less_event_log_row() {
        // The id guard is scoped to the `items` ledger array: event-log
        // arrays (vet_events / rollback_events / events) legitimately carry
        // no `id`, so array-append of an id-less row must still succeed.
        let mut doc = led();
        let row: JsonValue =
            serde_json::from_str(r#"{"timestamp":"2026-05-22T00:00:00Z","agent_index":1}"#).unwrap();
        items_add_value_to(&mut doc, row, "vet_events").unwrap();
        let events = items_array(&doc, "vet_events");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn items_add_many_preserves_date_coercion_for_first_flagged() {
        let mut doc = led();
        let defaults: JsonValue = serde_json::from_str(
            r#"{"first_flagged":"2026-04-18","status":"open","rounds":1}"#,
        )
        .unwrap();
        let rows: Vec<JsonValue> = vec![serde_json::from_str(
            r#"{"id":"R20","file":"c.rs","line":3,"severity":"warning","effort":"small","category":"quality","summary":"c"}"#,
        )
        .unwrap()];
        items_add_many(&mut doc, "items", &rows, Some(&defaults)).unwrap();
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains("first_flagged = 2026-04-18"),
            "expected raw TOML date literal for first_flagged, got:\n{serialised}"
        );
    }

    #[test]
    fn items_add_many_into_rollback_events_array() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
summary = "existing"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"timestamp":"2026-04-18T00:00:00Z","command":"review-apply","cause":"one","items":["R1"],"stash_ref":"stash@{0}"}"#).unwrap(),
            serde_json::from_str(r#"{"timestamp":"2026-04-18T00:01:00Z","command":"optimise-apply","cause":"two","items":["R2"],"stash_ref":"stash@{1}"}"#).unwrap(),
        ];
        let n = items_add_many(&mut doc, "rollback_events", &rows, None).unwrap();
        assert_eq!(n, 2);
        let events = doc
            .get("rollback_events")
            .and_then(|v| v.as_array())
            .expect("rollback_events array");
        assert_eq!(events.len(), 2);
        let first = events[0].as_table().unwrap();
        assert_eq!(first.get("cause").unwrap().as_str(), Some("one"));
        // `timestamp` is not in DATE_KEYS, so it stays a plain string (JSON
        // strings pass through `json_to_toml` as TOML strings). This pins
        // that rollback_events.timestamp is never date-coerced by this path.
        assert_eq!(
            first.get("timestamp").unwrap().as_str(),
            Some("2026-04-18T00:00:00Z")
        );
        // `items` array untouched.
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains("[[rollback_events]]"),
            "expected [[rollback_events]] header, got:\n{serialised}"
        );
    }

    #[test]
    fn array_append_matches_items_add_many_with_no_defaults() {
        // T6b: both paths funnel through `items_add_value_to`, which
        // reads the `TOMLCTL_NO_DEDUP_ID` env var. A parallel test that
        // toggles the kill switch would cause one of the two adds here to
        // observe a different env state and emit a divergent `dedup_id`
        // key. Holding the dedup-env lock for the whole test keeps the
        // byte-identity assertion deterministic under `cargo test`.
        let _guard = env_lock();
        let src = r#"schema_version = 1
"#;
        let mut doc_a: TomlValue = toml::from_str(src).unwrap();
        let mut doc_b: TomlValue = toml::from_str(src).unwrap();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"id":"E1","kind":"note"}"#).unwrap(),
            serde_json::from_str(r#"{"id":"E2","kind":"note"}"#).unwrap(),
        ];
        let n_a = array_append(&mut doc_a, "events", &rows).unwrap();
        let n_b = items_add_many(&mut doc_b, "events", &rows, None).unwrap();
        assert_eq!(n_a, n_b);
        assert_eq!(
            toml::to_string_pretty(&doc_a).unwrap(),
            toml::to_string_pretty(&doc_b).unwrap(),
            "array_append must be byte-identical to items_add_many(.., None)"
        );
    }

    #[test]
    fn parse_ndjson_reports_line_number_on_bad_json() {
        let input = "{\"id\":\"R1\"}\n{\"id\":\"R2\"}\n{not json\n";
        let err = parse_ndjson(input).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("line 3"), "expected 'line 3', got: {msg}");
    }

    #[test]
    fn parse_ndjson_skips_blank_lines_but_keeps_line_numbering() {
        // Line 1: valid, line 2: blank (skipped), line 3: malformed.
        // Error must still name line 3, not line 2.
        let input = "{\"id\":\"R1\"}\n\n{bad\n";
        let err = parse_ndjson(input).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("line 3"), "expected 'line 3', got: {msg}");

        // Happy path with a blank line in the middle: 2 rows out.
        let ok_input = "{\"id\":\"R1\"}\n\n{\"id\":\"R2\"}\n";
        let rows = parse_ndjson(ok_input).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "R1");
        assert_eq!(rows[1]["id"], "R2");
    }

    // ----- T5: --dedupe-by (find_dedupe_match unit tests) ----------------

    /// Build a small ledger fixture for dedupe tests with two existing
    /// items. Re-used across the cases below so each test asserts one
    /// branch of the `find_dedupe_match` logic in isolation. R1 carries
    /// a nested `meta` object (via `[items.meta]`-style inline table) so
    /// the dotted-path walker is exercised on an item with nested depth.
    fn dedupe_fixture() -> TomlValue {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "alpha"
status = "open"
meta = { source_run = "run-42" }

[[items]]
id = "R2"
file = "src/b.rs"
summary = "beta"
status = "open"
"#;
        toml::from_str(src).unwrap()
    }

    #[test]
    fn find_dedupe_match_empty_fields_returns_none() {
        // An empty fields slice must behave as "dedupe off" — no scan,
        // no match. The dispatcher uses this to skip the flag entirely;
        // the helper itself pins the contract so a careless refactor
        // can't flip it to "match on everything".
        let doc = dedupe_fixture();
        let payload: JsonValue = serde_json::from_str(
            r#"{"file":"src/a.rs","summary":"alpha"}"#,
        )
        .unwrap();
        let got = find_dedupe_match(&doc, "items", &payload, &[]);
        assert_eq!(got, None, "empty fields must never match");
    }

    #[test]
    fn find_dedupe_match_multi_field_all_must_match() {
        let doc = dedupe_fixture();
        // Both `file` and `summary` match R1 — hit.
        let hit: JsonValue = serde_json::from_str(
            r#"{"file":"src/a.rs","summary":"alpha","status":"new"}"#,
        )
        .unwrap();
        assert_eq!(
            find_dedupe_match(
                &doc,
                "items",
                &hit,
                &["file".to_string(), "summary".to_string()],
            ),
            Some("R1".to_string())
        );

        // `file` matches R1 but `summary` differs — miss.
        let miss: JsonValue = serde_json::from_str(
            r#"{"file":"src/a.rs","summary":"different"}"#,
        )
        .unwrap();
        assert_eq!(
            find_dedupe_match(
                &doc,
                "items",
                &miss,
                &["file".to_string(), "summary".to_string()],
            ),
            None,
            "partial field match must not count as a dedupe hit"
        );
    }

    #[test]
    fn find_dedupe_match_missing_on_both_sides_is_equal() {
        // Neither R1/R2 nor the payload carries `nonexistent_field`, so
        // both `walk_json_path` calls return `None` — equal by
        // definition. A `--dedupe-by nonexistent_field,file` predicate
        // therefore reduces to the `file` field alone in practice.
        let doc = dedupe_fixture();
        let payload: JsonValue = serde_json::from_str(r#"{"file":"src/a.rs"}"#).unwrap();
        let got = find_dedupe_match(
            &doc,
            "items",
            &payload,
            &["nonexistent_field".to_string(), "file".to_string()],
        );
        assert_eq!(got, Some("R1".to_string()));
    }

    #[test]
    fn find_dedupe_match_missing_on_one_side_is_unequal() {
        // Payload has `file` = src/a.rs (matches R1) AND `extra_key` =
        // "x" (R1 lacks it). `extra_key` is missing on the candidate
        // side and present on the payload side → unequal → miss.
        let doc = dedupe_fixture();
        let payload: JsonValue = serde_json::from_str(
            r#"{"file":"src/a.rs","extra_key":"x"}"#,
        )
        .unwrap();
        let got = find_dedupe_match(
            &doc,
            "items",
            &payload,
            &["file".to_string(), "extra_key".to_string()],
        );
        assert_eq!(got, None, "one-sided presence must be unequal");
    }

    #[test]
    fn items_add_value_with_dedupe_to_appends_on_miss_and_skips_on_hit() {
        let mut doc = dedupe_fixture();
        let patch: JsonValue =
            serde_json::from_str(r#"{"id":"R3","file":"src/c.rs","summary":"gamma"}"#)
                .unwrap();
        // First call: no existing row with file=src/c.rs — append.
        let outcome = items_add_value_with_dedupe_to(
            &mut doc,
            patch.clone(),
            "items",
            &["file".to_string(), "summary".to_string()],
        )
        .unwrap();
        assert!(matches!(outcome, AddOutcome::Added));
        // Second call with the same patch: R3 now exists → skip, report
        // `matched_id=R3`. Doc unchanged relative to the post-first-call
        // state.
        let outcome = items_add_value_with_dedupe_to(
            &mut doc,
            patch,
            "items",
            &["file".to_string(), "summary".to_string()],
        )
        .unwrap();
        match outcome {
            AddOutcome::Skipped { matched_id } => assert_eq!(matched_id, "R3"),
            other => panic!("expected Skipped(R3), got {other:?}"),
        }
        // Array length still 3 (original 2 + the single R3 add).
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn items_add_many_with_dedupe_records_skipped_rows_in_input_order() {
        let mut doc = dedupe_fixture();
        let rows: Vec<JsonValue> = vec![
            // Row 1: new → added.
            serde_json::from_str(r#"{"id":"R3","file":"src/c.rs","summary":"gamma"}"#)
                .unwrap(),
            // Row 2: duplicate of R1 → skipped.
            serde_json::from_str(r#"{"id":"R99","file":"src/a.rs","summary":"alpha"}"#)
                .unwrap(),
            // Row 3: new → added.
            serde_json::from_str(r#"{"id":"R4","file":"src/d.rs","summary":"delta"}"#)
                .unwrap(),
        ];
        let outcome = items_add_many_with_dedupe(
            &mut doc,
            "items",
            &rows,
            None,
            &["file".to_string(), "summary".to_string()],
        )
        .unwrap();
        assert_eq!(outcome.added, 2);
        assert_eq!(outcome.skipped_rows.len(), 1);
        assert_eq!(outcome.skipped_rows[0].row, 2);
        assert_eq!(outcome.skipped_rows[0].matched_id, "R1");
        // Doc state: original 2 + 2 added = 4.
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 4);
    }

    // ----- T6b: dedup_id auto-populate (helper-level, no I/O) ------------

    /// Helper: build a patch `Map<String, JsonValue>` from a JSON string for
    /// the dedup_id branch tests. Avoids `.unwrap()` sprawl on each case.
    fn patch_obj(json: &str) -> serde_json::Map<String, JsonValue> {
        match serde_json::from_str::<JsonValue>(json).unwrap() {
            JsonValue::Object(m) => m,
            _ => panic!("expected object"),
        }
    }

    /// Existing-table fixture for the update branch tests — carries all
    /// five fingerprinted fields plus `status` and an existing
    /// `dedup_id` placeholder so every branch has concrete data to diff.
    fn existing_with_dedup_id() -> toml::Table {
        toml::toml! {
            id = "R1"
            file = "src/a.rs"
            summary = "existing summary"
            severity = "minor"
            category = "quality"
            symbol = "foo::bar"
            status = "open"
            dedup_id = "pre_existing_id"
        }
    }

    fn existing_without_dedup_id() -> toml::Table {
        toml::toml! {
            id = "R1"
            file = "src/a.rs"
            summary = "existing summary"
            severity = "minor"
            category = "quality"
            symbol = "foo::bar"
            status = "open"
        }
    }

    /// T6b branch 1: explicit `dedup_id` in the patch (non-empty string) is
    /// preserved regardless of whether other fingerprinted fields are in
    /// the patch. This is the "caller knows best" override path.
    #[test]
    fn dedup_id_update_branch_1_explicit_preserved_even_with_fingerprint_patch() {
        let _guard = env_lock();
        let existing = existing_with_dedup_id();
        let mut patch = patch_obj(
            r#"{"summary":"new-summary","dedup_id":"caller_provided"}"#,
        );
        apply_dedup_id_on_update(&existing, &mut patch);
        assert_eq!(
            patch.get("dedup_id").and_then(|v| v.as_str()),
            Some("caller_provided"),
            "explicit dedup_id must survive a fingerprint-field patch"
        );
    }

    /// T6b branch 2: a fingerprinted-field patch with no explicit
    /// `dedup_id` triggers recompute from the merged (patch-over-existing)
    /// view. The resulting digest must equal `tier_b_fingerprint_json`
    /// on the merged view (that's the exact contract of this branch).
    #[test]
    fn dedup_id_update_branch_2_fingerprint_field_patch_recomputes() {
        let _guard = env_lock();
        let existing = existing_with_dedup_id();
        let mut patch = patch_obj(r#"{"summary":"new summary"}"#);
        apply_dedup_id_on_update(&existing, &mut patch);
        let got = patch.get("dedup_id").and_then(|v| v.as_str()).unwrap();
        // Compute expected: merged view's fingerprint.
        let expected_merged: JsonValue = serde_json::json!({
            "file": "src/a.rs",
            "summary": "new summary",
            "severity": "minor",
            "category": "quality",
            "symbol": "foo::bar",
        });
        let expected = crate::dedup::tier_b_fingerprint_json(expected_merged.as_object().unwrap());
        assert_eq!(
            got, expected,
            "branch 2 must recompute from the merged view"
        );
        assert_ne!(
            got, "pre_existing_id",
            "recompute must actually change the digest (summary did change)"
        );
    }

    /// T6b branch 3: non-fingerprint patch on an item that LACKS
    /// `dedup_id` must leave the patch alone — no silent auto-populate
    /// (that's Task 11's `backfill-dedup-id`).
    #[test]
    fn dedup_id_update_branch_3_non_fingerprint_patch_legacy_item_preserves_absence() {
        let _guard = env_lock();
        let existing = existing_without_dedup_id();
        let mut patch = patch_obj(r#"{"status":"fixed"}"#);
        apply_dedup_id_on_update(&existing, &mut patch);
        assert!(
            !patch.contains_key("dedup_id"),
            "branch 3 must NOT auto-populate a legacy item on an unrelated patch"
        );
    }

    /// T6b branch 4: non-fingerprint patch on an item that already has
    /// `dedup_id` must leave the existing digest intact (no recompute,
    /// no patch mutation — the merge loop skips absent keys).
    #[test]
    fn dedup_id_update_branch_4_non_fingerprint_patch_existing_digest_preserved() {
        let _guard = env_lock();
        let existing = existing_with_dedup_id();
        let mut patch = patch_obj(r#"{"status":"fixed"}"#);
        apply_dedup_id_on_update(&existing, &mut patch);
        assert!(
            !patch.contains_key("dedup_id"),
            "branch 4 must leave the patch alone so existing dedup_id stays untouched"
        );
    }

    /// T6b: `dedup_id: null` in the patch is treated as "patch didn't
    /// mention the field" (preservation path), NOT "remove the existing
    /// digest". Documented as the less-surprising semantics.
    #[test]
    fn dedup_id_update_null_patch_treated_as_absent() {
        let _guard = env_lock();
        let existing = existing_with_dedup_id();
        // Non-fingerprint patch with explicit null dedup_id — both the
        // "explicit" check and the "touches fingerprinted" check should
        // skip, and the patch should end up with null dedup_id that the
        // merge loop will strip.
        let mut patch = patch_obj(r#"{"status":"fixed","dedup_id":null}"#);
        apply_dedup_id_on_update(&existing, &mut patch);
        assert!(
            matches!(patch.get("dedup_id"), Some(JsonValue::Null)),
            "null dedup_id must survive so the merge loop can skip it; got {:?}",
            patch.get("dedup_id")
        );
    }

    /// T6b: kill-switch env var short-circuits every hook.
    #[test]
    fn dedup_id_kill_switch_disables_auto_populate_on_add() {
        let _guard_lock = env_lock();
        // Serialise against other env-mutating tests — use a guard that
        // restores the var on drop so cargo-test parallel runs stay
        // deterministic.
        struct EnvGuard {
            key: &'static str,
            prev: Option<String>,
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match &self.prev {
                    Some(v) => unsafe { std::env::set_var(self.key, v) },
                    None => unsafe { std::env::remove_var(self.key) },
                }
            }
        }
        let prev = std::env::var(DEDUP_ID_KILL_SWITCH).ok();
        unsafe { std::env::set_var(DEDUP_ID_KILL_SWITCH, "1") };
        let _guard = EnvGuard {
            key: DEDUP_ID_KILL_SWITCH,
            prev,
        };

        let mut obj = patch_obj(r#"{"file":"src/a.rs","summary":"x"}"#);
        apply_dedup_id_on_add(&mut obj);
        assert!(
            !obj.contains_key("dedup_id"),
            "kill switch must suppress auto-populate on add"
        );

        let existing = existing_without_dedup_id();
        let mut patch = patch_obj(r#"{"summary":"new"}"#);
        apply_dedup_id_on_update(&existing, &mut patch);
        assert!(
            !patch.contains_key("dedup_id"),
            "kill switch must suppress recompute on update"
        );
    }

    /// T6b: add with no `dedup_id` auto-populates from the payload's
    /// fingerprinted fields. Re-adding the same payload produces an
    /// identical digest (pure function of fields — idempotent).
    #[test]
    fn dedup_id_add_auto_populates_deterministically() {
        let _guard = env_lock();
        let mut obj = patch_obj(
            r#"{"file":"src/a.rs","summary":"x","severity":"warning","category":"bug","symbol":""}"#,
        );
        apply_dedup_id_on_add(&mut obj);
        let fp1 = obj.get("dedup_id").and_then(|v| v.as_str()).unwrap().to_string();
        assert_eq!(fp1.len(), 16);

        let mut obj2 = patch_obj(
            r#"{"file":"src/a.rs","summary":"x","severity":"warning","category":"bug","symbol":""}"#,
        );
        apply_dedup_id_on_add(&mut obj2);
        let fp2 = obj2.get("dedup_id").and_then(|v| v.as_str()).unwrap().to_string();
        assert_eq!(fp1, fp2, "same payload must produce the same digest");
    }

    /// T6b: explicit `dedup_id` on the add payload is preserved (no
    /// fingerprint override). Mirrors branch 1 on the add side.
    #[test]
    fn dedup_id_add_preserves_explicit_value() {
        let _guard = env_lock();
        let mut obj = patch_obj(
            r#"{"file":"src/a.rs","summary":"x","dedup_id":"caller_provided"}"#,
        );
        apply_dedup_id_on_add(&mut obj);
        assert_eq!(
            obj.get("dedup_id").and_then(|v| v.as_str()),
            Some("caller_provided"),
            "explicit dedup_id on add must survive"
        );
    }

    /// T6b: `items_add_value_to` funnels through `apply_dedup_id_on_add`,
    /// so a JSON payload without `dedup_id` lands on disk with one set.
    /// Integration-style coverage of the full single-add write path.
    #[test]
    fn items_add_value_to_writes_dedup_id_onto_disk() {
        let _guard = env_lock();
        let mut doc: TomlValue = toml::from_str("schema_version = 1\n").unwrap();
        items_add_to(
            &mut doc,
            "items",
            r#"{"id":"R1","file":"src/a.rs","summary":"x","severity":"warning","category":"quality"}"#,
        )
        .unwrap();
        let item = items_get(&doc, "R1").unwrap();
        let fp = item["dedup_id"].as_str().expect("dedup_id auto-populated");
        assert_eq!(fp.len(), 16, "dedup_id must be 16 hex chars; got {fp:?}");
    }

    // ----- T10: compute/apply split invariance ----------------------------

    /// T10 (e) INVARIANCE (unit-level): `compute_apply_mutation`'s
    /// `plan.new_doc`, when serialised through the same
    /// `toml::to_string_pretty` emit path that `write_toml_with_sidecar`
    /// uses, must be byte-identical to the in-place mutated doc produced
    /// by `items_apply_to_opts` (the live path's mutator) serialised the
    /// same way. This is the structural guarantee that underpins
    /// `--dry-run`: if these two bytes ever diverged, the dry-run preview
    /// would lie about what a real run would write.
    ///
    /// Covered op set: mixed add + update + remove, hitting every branch
    /// in `apply_single_op` and the indexed fast-path trigger (threshold
    /// is 5 update ops today, so we exercise the linear-scan path only).
    #[test]
    fn compute_apply_mutation_new_doc_matches_live_apply_bytes() {
        let _guard = env_lock();
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "first"
severity = "warning"
category = "quality"
status = "open"

[[items]]
id = "R2"
file = "src/b.rs"
summary = "second"
severity = "warning"
category = "quality"
status = "open"
"#;
        let ops = r#"[
            {"op":"add","json":{"id":"R3","file":"src/c.rs","summary":"third","severity":"warning","category":"quality","status":"open"}},
            {"op":"update","id":"R1","json":{"status":"fixed","resolution":"fix in xyz","resolved":"2026-04-18"}},
            {"op":"remove","id":"R2"}
        ]"#;

        // Live path: clone + mutate in place.
        let mut live_doc: TomlValue = toml::from_str(fixture).unwrap();
        items_apply_to_opts(&mut live_doc, ops, "items", false).unwrap();
        let live_bytes = toml::to_string_pretty(&live_doc).unwrap();

        // Compute path: pure, returns MutationPlan.
        let plan_doc: TomlValue = toml::from_str(fixture).unwrap();
        let parsed_ops: JsonValue = serde_json::from_str(ops).unwrap();
        let plan = compute_apply_mutation(&plan_doc, "items", &parsed_ops, false).unwrap();
        let plan_bytes = toml::to_string_pretty(&plan.new_doc).unwrap();

        assert_eq!(
            live_bytes, plan_bytes,
            "compute_apply_mutation new_doc must serialise byte-identically to live apply"
        );

        // ids tracking must match the intended semantics.
        assert_eq!(plan.added, vec!["R3".to_string()]);
        assert_eq!(plan.updated, vec!["R1".to_string()]);
        assert_eq!(plan.removed, vec!["R2".to_string()]);
        assert_eq!(
            plan.union_ids(),
            vec!["R3".to_string(), "R1".to_string(), "R2".to_string()],
            "union_ids concats added, updated, removed in that order"
        );
    }

    /// T10: `compute_remove_mutation` serialises byte-identically to a
    /// live `items_remove_from` on the same fixture. Mirrors the
    /// `compute_apply_mutation` invariance above for the single-item
    /// remove path.
    #[test]
    fn compute_remove_mutation_matches_live_remove_bytes() {
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"

[[items]]
id = "R2"
summary = "second"
status = "open"
"#;
        let mut live_doc: TomlValue = toml::from_str(fixture).unwrap();
        items_remove_from(&mut live_doc, "items", "R1").unwrap();
        let live_bytes = toml::to_string_pretty(&live_doc).unwrap();

        let plan_doc: TomlValue = toml::from_str(fixture).unwrap();
        let plan = compute_remove_mutation(&plan_doc, "items", "R1").unwrap();
        let plan_bytes = toml::to_string_pretty(&plan.new_doc).unwrap();

        assert_eq!(live_bytes, plan_bytes);
        assert_eq!(plan.removed, vec!["R1".to_string()]);
        assert!(plan.added.is_empty());
        assert!(plan.updated.is_empty());
    }

    /// T10: `compute_apply_mutation` with `--no-remove` AND a remove op
    /// errors with the canonical `--no-remove` message — the gate lives
    /// inside the compute phase, so dry-run and live apply surface it
    /// identically.
    #[test]
    fn compute_apply_mutation_no_remove_errors_identically() {
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
"#;
        let doc: TomlValue = toml::from_str(fixture).unwrap();
        let ops: JsonValue = serde_json::from_str(r#"[{"op":"remove","id":"R1"}]"#).unwrap();
        let err = compute_apply_mutation(&doc, "items", &ops, true).unwrap_err();
        assert!(
            err.to_string().contains("is a remove op, but --no-remove was set"),
            "expected --no-remove gate message; got: {err}"
        );
    }

    // ----- T11: compute_backfill_mutation ----------------------------------

    /// T11 (unit): mixed-state ledger where some items already carry
    /// `dedup_id` and others don't. The backfill must touch ONLY the
    /// missing ones, preserving the pre-existing values exactly, and
    /// `plan.updated` must list the newly-populated items' ids in input
    /// order. `plan.new_doc` carries the freshly-populated digests on
    /// the previously-bare items.
    #[test]
    fn compute_backfill_mutation_only_touches_missing() {
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "alpha"
severity = "warning"
category = "quality"

[[items]]
id = "R2"
file = "src/b.rs"
summary = "beta"
severity = "warning"
category = "quality"
dedup_id = "preexisting-value"

[[items]]
id = "R3"
file = "src/c.rs"
summary = "gamma"
severity = "warning"
category = "quality"
"#;
        let doc: TomlValue = toml::from_str(fixture).unwrap();
        let plan = compute_backfill_mutation(&doc, "items").unwrap();
        // Only the two items lacking `dedup_id` get updated.
        assert_eq!(
            plan.updated,
            vec!["R1".to_string(), "R3".to_string()],
            "only items missing dedup_id should land in plan.updated"
        );
        assert!(plan.added.is_empty());
        assert!(plan.removed.is_empty());
        // Inspect the new_doc to confirm:
        //   - R1 now has a dedup_id matching `tier_b_fingerprint`,
        //   - R2's preserved exactly,
        //   - R3 now has a dedup_id matching `tier_b_fingerprint`.
        let items = plan.new_doc.get("items").and_then(|v| v.as_array()).unwrap();
        let r1 = items[0].as_table().unwrap();
        let r1_fp = r1.get("dedup_id").and_then(|v| v.as_str()).unwrap();
        assert_eq!(
            r1_fp,
            &tier_b_fingerprint(&TomlValue::Table(r1.clone())),
            "R1's backfilled dedup_id must match tier_b_fingerprint of the item"
        );
        let r2 = items[1].as_table().unwrap();
        assert_eq!(
            r2.get("dedup_id").and_then(|v| v.as_str()),
            Some("preexisting-value"),
            "R2's pre-existing dedup_id must be preserved byte-for-byte"
        );
        let r3 = items[2].as_table().unwrap();
        let r3_fp = r3.get("dedup_id").and_then(|v| v.as_str()).unwrap();
        assert_eq!(
            r3_fp,
            &tier_b_fingerprint(&TomlValue::Table(r3.clone())),
            "R3's backfilled dedup_id must match tier_b_fingerprint of the item"
        );
    }

    /// T11 (unit): idempotence — a ledger where every item already has
    /// `dedup_id` produces an empty `plan.updated`. The CLI dispatch uses
    /// this signal to skip the write entirely (no sidecar churn).
    #[test]
    fn compute_backfill_mutation_idempotent_when_all_present() {
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "alpha"
dedup_id = "0123456789abcdef"

[[items]]
id = "R2"
file = "src/b.rs"
summary = "beta"
dedup_id = "fedcba9876543210"
"#;
        let doc: TomlValue = toml::from_str(fixture).unwrap();
        let plan = compute_backfill_mutation(&doc, "items").unwrap();
        assert!(
            plan.updated.is_empty(),
            "no items missing dedup_id → plan.updated empty"
        );
    }

    /// T11 (unit): empty-array ledger. `items_array_mut` auto-creates the
    /// array if missing, so the walk is a no-op and `plan.updated` is empty.
    #[test]
    fn compute_backfill_mutation_empty_ledger() {
        let fixture = "schema_version = 1\n";
        let doc: TomlValue = toml::from_str(fixture).unwrap();
        let plan = compute_backfill_mutation(&doc, "items").unwrap();
        assert!(plan.updated.is_empty());
        assert!(plan.added.is_empty());
        assert!(plan.removed.is_empty());
    }

    // ----- T10b: compute_add / add_many / update / array_append -------------

    /// T10b: `compute_add_mutation` produces a `new_doc` whose serialised
    /// bytes are byte-identical to a live `items_add_to` on the same
    /// fixture. Mirrors the byte-equivalence guarantee that underpins
    /// `--dry-run` for the apply path.
    #[test]
    fn compute_add_mutation_matches_live_add_bytes() {
        let _guard = env_lock();
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "first"
severity = "warning"
category = "quality"
status = "open"
"#;
        let json = r#"{"id":"R2","file":"src/b.rs","summary":"second","severity":"warning","category":"quality","status":"open"}"#;

        let mut live_doc: TomlValue = toml::from_str(fixture).unwrap();
        items_add_to(&mut live_doc, "items", json).unwrap();
        let live_bytes = toml::to_string_pretty(&live_doc).unwrap();

        let plan_doc: TomlValue = toml::from_str(fixture).unwrap();
        let patch: JsonValue = serde_json::from_str(json).unwrap();
        let plan = compute_add_mutation(&plan_doc, "items", &patch).unwrap();
        let plan_bytes = toml::to_string_pretty(&plan.new_doc).unwrap();

        assert_eq!(
            live_bytes, plan_bytes,
            "compute_add_mutation new_doc must serialise byte-identically to live add"
        );
        assert_eq!(plan.added, vec!["R2".to_string()]);
        assert!(plan.updated.is_empty());
        assert!(plan.removed.is_empty());
    }

    /// T10b: `compute_add_many_mutation` with empty `dedupe_fields`
    /// (delegating to `items_add_many`) and with non-empty `dedupe_fields`
    /// (delegating to `items_add_many_with_dedupe`) both produce
    /// `new_doc` bytes equal to the corresponding live mutator on the
    /// same fixture. Covers the defaults-stamping path so the dry-run
    /// preview is faithful to `--defaults-json`.
    #[test]
    fn compute_add_many_mutation_matches_live_add_many_bytes() {
        let _guard = env_lock();
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "first"
severity = "warning"
category = "quality"
status = "open"
"#;
        let rows: Vec<JsonValue> = vec![
            serde_json::json!({"id":"R2","file":"src/b.rs","summary":"second","severity":"warning","category":"quality","status":"open"}),
            serde_json::json!({"id":"R3","file":"src/c.rs","summary":"third","severity":"warning","category":"quality","status":"open"}),
        ];
        let defaults = serde_json::json!({"first_flagged":"2026-04-08","rounds":1});

        // --- Sub-case A: empty dedupe_fields → items_add_many path ---
        let mut live_doc: TomlValue = toml::from_str(fixture).unwrap();
        items_add_many(&mut live_doc, "items", &rows, Some(&defaults)).unwrap();
        let live_bytes = toml::to_string_pretty(&live_doc).unwrap();

        let plan_doc: TomlValue = toml::from_str(fixture).unwrap();
        let plan =
            compute_add_many_mutation(&plan_doc, "items", &rows, Some(&defaults), &[]).unwrap();
        let plan_bytes = toml::to_string_pretty(&plan.new_doc).unwrap();
        assert_eq!(
            live_bytes, plan_bytes,
            "compute_add_many_mutation (no dedupe) must byte-match live items_add_many"
        );
        assert_eq!(plan.added, vec!["R2".to_string(), "R3".to_string()]);
        assert!(plan.skipped.is_empty());

        // --- Sub-case B: non-empty dedupe_fields → items_add_many_with_dedupe path ---
        // Add a duplicate of R2 (by `summary`) to force a skip on the
        // second pass; both rows would otherwise append.
        let dup_rows: Vec<JsonValue> = vec![
            serde_json::json!({"id":"R4","file":"src/d.rs","summary":"first","severity":"warning","category":"quality","status":"open"}),
            serde_json::json!({"id":"R5","file":"src/e.rs","summary":"fifth","severity":"warning","category":"quality","status":"open"}),
        ];
        let dedupe = vec!["summary".to_string()];

        let mut live_doc: TomlValue = toml::from_str(fixture).unwrap();
        let live_outcome =
            items_add_many_with_dedupe(&mut live_doc, "items", &dup_rows, None, &dedupe).unwrap();
        let live_bytes = toml::to_string_pretty(&live_doc).unwrap();

        let plan_doc: TomlValue = toml::from_str(fixture).unwrap();
        let plan =
            compute_add_many_mutation(&plan_doc, "items", &dup_rows, None, &dedupe).unwrap();
        let plan_bytes = toml::to_string_pretty(&plan.new_doc).unwrap();
        assert_eq!(
            live_bytes, plan_bytes,
            "compute_add_many_mutation (dedupe) must byte-match live items_add_many_with_dedupe"
        );
        // R4 is the duplicate of R1 by `summary`; it should be skipped.
        assert_eq!(plan.added, vec!["R5".to_string()]);
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].row, 1);
        assert_eq!(plan.skipped[0].matched_id, "R1");
        assert_eq!(plan.added.len(), live_outcome.added);
    }

    /// T10b: `compute_update_mutation` produces a `new_doc` whose
    /// serialised bytes are byte-identical to a live `items_update_to`
    /// on the same fixture, including the `unset` keys path.
    #[test]
    fn compute_update_mutation_matches_live_update_bytes() {
        let _guard = env_lock();
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "first"
severity = "warning"
category = "quality"
status = "open"
notes = "to be cleared"

[[items]]
id = "R2"
file = "src/b.rs"
summary = "second"
severity = "warning"
category = "quality"
status = "open"
"#;
        let json = r#"{"status":"fixed","resolution":"fix in xyz","resolved":"2026-04-18"}"#;
        let unset = vec!["notes".to_string()];

        let mut live_doc: TomlValue = toml::from_str(fixture).unwrap();
        items_update_to(&mut live_doc, "items", "R1", json, &unset).unwrap();
        let live_bytes = toml::to_string_pretty(&live_doc).unwrap();

        let plan_doc: TomlValue = toml::from_str(fixture).unwrap();
        let plan = compute_update_mutation(&plan_doc, "items", "R1", json, &unset).unwrap();
        let plan_bytes = toml::to_string_pretty(&plan.new_doc).unwrap();

        assert_eq!(
            live_bytes, plan_bytes,
            "compute_update_mutation new_doc must serialise byte-identically to live update"
        );
        assert_eq!(plan.updated, vec!["R1".to_string()]);
        assert!(plan.added.is_empty());
        assert!(plan.removed.is_empty());
    }

    /// T10b: `compute_array_append_mutation` threads the array name
    /// through to `array_append` faithfully — appending to a
    /// non-default array (`rollback_events`) produces a `new_doc` with
    /// bytes equal to a live `array_append` on the same target. This
    /// exercises the "deliberately no defaults / no dedupe" forwarding
    /// path of `compute_array_append_mutation`.
    #[test]
    fn compute_array_append_mutation_targets_named_array() {
        let _guard = env_lock();
        let fixture = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "first"

[[rollback_events]]
ts = "2026-04-01T00:00:00Z"
note = "baseline"
"#;
        let rows: Vec<JsonValue> = vec![
            serde_json::json!({"ts":"2026-04-18T12:00:00Z","note":"first rollback"}),
            serde_json::json!({"ts":"2026-04-19T12:00:00Z","note":"second rollback"}),
        ];

        let mut live_doc: TomlValue = toml::from_str(fixture).unwrap();
        array_append(&mut live_doc, "rollback_events", &rows).unwrap();
        let live_bytes = toml::to_string_pretty(&live_doc).unwrap();

        let plan_doc: TomlValue = toml::from_str(fixture).unwrap();
        let plan =
            compute_array_append_mutation(&plan_doc, "rollback_events", &rows).unwrap();
        let plan_bytes = toml::to_string_pretty(&plan.new_doc).unwrap();

        assert_eq!(
            live_bytes, plan_bytes,
            "compute_array_append_mutation new_doc must serialise byte-identically to live array_append on a non-default array"
        );
        // rollback_events rows have no `id` field; ids surface as empty
        // strings — same convention as `compute_apply_mutation` for ad-hoc add ops.
        assert_eq!(plan.added, vec![String::new(), String::new()]);
        assert!(plan.updated.is_empty());
        assert!(plan.removed.is_empty());
        assert!(plan.skipped.is_empty());

        // Sanity: the items array in the original fixture is untouched.
        let items = plan.new_doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
        let events = plan
            .new_doc
            .get("rollback_events")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(
            events.len(),
            3,
            "1 baseline + 2 appended rows in the named array"
        );
    }

    // R19: disposition-field validation tests for `Item::validate`.

    #[test]
    fn item_validate_rejects_non_object_payload() {
        let arr = serde_json::json!(["R1"]);
        let err = Item::validate(&arr).unwrap_err();
        assert!(matches!(err, DispositionError::NotAnObject { .. }));
    }

    #[test]
    fn item_validate_rejects_payload_missing_status() {
        let v = serde_json::json!({"id": "R1", "summary": "x"});
        assert_eq!(Item::validate(&v), Err(DispositionError::MissingStatus));
    }

    #[test]
    fn item_validate_accepts_open_status_with_no_companion_fields() {
        let v = serde_json::json!({"id": "R1", "status": "open"});
        assert!(Item::validate(&v).is_ok());
    }

    #[test]
    fn item_validate_accepts_unknown_status_forward_compat() {
        // Forward-compat: a status the validator doesn't know about passes.
        let v = serde_json::json!({"id": "R1", "status": "rumination-pending"});
        assert!(Item::validate(&v).is_ok());
    }

    #[test]
    fn item_validate_flags_fixed_missing_resolution() {
        let v = serde_json::json!({"id": "R1", "status": "fixed", "resolved": "2026-04-08"});
        let err = Item::validate(&v).unwrap_err();
        assert_eq!(
            err,
            DispositionError::MissingDispositionField {
                status: "fixed".to_string(),
                field: "resolution"
            }
        );
    }

    #[test]
    fn item_validate_flags_applied_missing_resolved() {
        let v = serde_json::json!({"id": "O1", "status": "applied", "resolution": "fix in abc"});
        let err = Item::validate(&v).unwrap_err();
        assert_eq!(
            err,
            DispositionError::MissingDispositionField {
                status: "applied".to_string(),
                field: "resolved"
            }
        );
    }

    #[test]
    fn item_validate_flags_deferred_missing_defer_trigger() {
        let v =
            serde_json::json!({"id": "R1", "status": "deferred", "defer_reason": "blocked"});
        let err = Item::validate(&v).unwrap_err();
        assert_eq!(
            err,
            DispositionError::MissingDispositionField {
                status: "deferred".to_string(),
                field: "defer_trigger"
            }
        );
    }

    #[test]
    fn item_validate_accepts_complete_deferred() {
        let v = serde_json::json!({
            "id": "R1",
            "status": "deferred",
            "defer_reason": "blocked",
            "defer_trigger": "when channel lands",
        });
        assert!(Item::validate(&v).is_ok());
    }

    #[test]
    fn item_validate_flags_wontfix_missing_rationale() {
        let v = serde_json::json!({"id": "R1", "status": "wontfix"});
        let err = Item::validate(&v).unwrap_err();
        assert_eq!(
            err,
            DispositionError::MissingDispositionField {
                status: "wontfix".to_string(),
                field: "wontfix_rationale"
            }
        );
    }

    #[test]
    fn item_validate_flags_wontapply_missing_rationale() {
        let v = serde_json::json!({"id": "O1", "status": "wontapply"});
        let err = Item::validate(&v).unwrap_err();
        assert_eq!(
            err,
            DispositionError::MissingDispositionField {
                status: "wontapply".to_string(),
                field: "wontfix_rationale"
            }
        );
    }

    #[test]
    fn item_validate_flags_verified_clean_missing_note() {
        let v = serde_json::json!({"id": "R1", "status": "verified-clean"});
        let err = Item::validate(&v).unwrap_err();
        assert_eq!(
            err,
            DispositionError::MissingDispositionField {
                status: "verified-clean".to_string(),
                field: "verified_note"
            }
        );
    }

    #[test]
    fn item_validate_treats_empty_string_as_missing() {
        // A placeholder field the agent left blank counts as missing — mirrors
        // `is_empty_json` so the validator catches the gap rather than papers
        // over an empty rationale.
        let v = serde_json::json!({"id": "R1", "status": "wontfix", "wontfix_rationale": ""});
        let err = Item::validate(&v).unwrap_err();
        assert_eq!(
            err,
            DispositionError::MissingDispositionField {
                status: "wontfix".to_string(),
                field: "wontfix_rationale"
            }
        );
    }

    #[test]
    fn item_validate_accepts_complete_fixed() {
        let v = serde_json::json!({
            "id": "R1",
            "status": "fixed",
            "resolved": "2026-04-08",
            "resolution": "fixed in abc123",
        });
        assert!(Item::validate(&v).is_ok());
    }
}
