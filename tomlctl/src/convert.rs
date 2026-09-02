//! TOML↔JSON conversion, scalar parsing, date-coercion, and dotted-path
//! traversal helpers. Pure functions only — no I/O and no CLI coupling.
//!
//! Public surface:
//! - `ScalarType` — explicit scalar-type override for `set`
//! - `parse_scalar` / `infer_type` / `looks_like_date`
//! - `toml_to_json` / `json_to_toml`
//! - `maybe_date_coerce` + `DATE_KEYS`
//! - `navigate` / `set_at_path`
//! - `str_field` / `i64_field`

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum ScalarType {
    Str,
    Int,
    Float,
    Bool,
    Date,
    Datetime,
}

/// Keys whose JSON-string values are automatically coerced to a TOML
/// `Datetime` when they parse as an ISO-8601 date/date-time.
///
/// This encodes ledger/flow schema knowledge (see the `## Ledger Schema`
/// shared-block in `claude/commands/{optimise,review,optimise-apply,review-apply}.md`
/// — the canonical description of every date-bearing field these CLIs know
/// about). When the schema grows, extend this list and update the shared
/// markdown in lockstep.
///
/// The `maybe_date_coerce_*` and `items_add_promotes_iso_date_strings_to_datetime`
/// tests pin the coercion behaviour so a silent regression (e.g. swapping one
/// entry back to a raw TOML string) fails CI.
pub(crate) const DATE_KEYS: &[&str] = &[
    "created",
    "updated",
    "first_flagged",
    "last_updated",
    "resolved",
    "date",
];

/// JSON-side dotted-path walker used by `items add --dedupe-by`.
///
/// Mirrors the `navigate` contract on the TOML side: split `path` on `.`
/// and descend one segment at a time, returning `None` when any segment
/// is missing or the current node isn't an object (nested-field dedup
/// bottoms out on the first non-object parent).
///
/// **Deliberate omissions** versus the TOML `navigate`:
///   - No array-index segments. Dedup is evaluated on ledger items, each
///     of which is already a JSON object; plucking `meta.source_run` from
///     an item needs object-keying only. Supporting `field.0` would invite
///     accidents where two items with divergent array shapes compare
///     "missing-field equal" by walking off the end.
///   - Null-on-missing is treated as `None` here, not `Some(Null)`. The
///     caller (`find_dedupe_match`) normalises both sides to the same
///     `Option<&JsonValue>` shape so missing-on-both is equal and
///     missing-on-one-only is unequal.
///
/// The `--where` predicate family shares none of this: `eval_predicate` in
/// `query.rs` uses flat `tbl.get(key)` lookups and stays single-key.
pub(crate) fn walk_json_path<'a>(v: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut cur = v;
    for seg in path.split('.') {
        match cur {
            JsonValue::Object(m) => {
                cur = m.get(seg)?;
            }
            _ => return None,
        }
    }
    Some(cur)
}

/// Read-side dotted-path traversal. Each segment either:
///   - indexes the current table by its key, OR
///   - when the current value is an array and the segment parses as a
///     `usize`, indexes the array. No negative indices, no slice syntax —
///     an out-of-bounds index returns `None` like a missing key does.
pub(crate) fn navigate<'a>(root: &'a TomlValue, path: &str) -> Option<&'a TomlValue> {
    let mut cur = root;
    for part in path.split('.') {
        cur = match cur {
            TomlValue::Table(tbl) => tbl.get(part)?,
            TomlValue::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

pub(crate) fn set_at_path(root: &mut TomlValue, path: &str, value: TomlValue) -> Result<()> {
    let parts: Vec<&str> = path.split('.').collect();
    let (last, parents) = parts
        .split_last()
        .ok_or_else(|| anyhow!("empty key path; path must be a non-empty `.`-separated dotted path (e.g. `nested.inner.key`)"))?;

    let mut cur: &mut TomlValue = root;
    for p in parents {
        // Parent traversal also supports integer-indexed arrays, matching
        // `navigate`. Auto-vivification of array slots is NOT supported — the
        // array index must already exist.
        if cur.is_array() {
            let idx: usize = p.parse().with_context(|| {
                format!("path segment `{}` is not a valid array index (must be a non-negative integer; e.g. `items.0.id`)", p)
            })?;
            cur = cur
                .as_array_mut()
                .and_then(|arr| arr.get_mut(idx))
                .ok_or_else(|| anyhow!("array index `{}` out of bounds (use `tomlctl get <file> --path <parent>` to inspect array length first)", idx))?;
            continue;
        }
        let tbl = cur
            .as_table_mut()
            .ok_or_else(|| anyhow!("path segment `{}` has a non-table parent (intermediate path segments must resolve to TOML tables; only the array-index form `parent.<n>` may descend into an array)", p))?;
        cur = tbl
            .entry((*p).to_string())
            .or_insert_with(|| TomlValue::Table(toml::Table::new()));
    }
    // Final segment: if the parent is an array and `last` parses as an index,
    // overwrite that slot; otherwise insert into the parent table by key.
    if cur.is_array() {
        let idx: usize = last.parse().with_context(|| {
            format!("final path segment `{}` is not a valid array index (must be a non-negative integer; e.g. `items.0`)", last)
        })?;
        let arr = cur
            .as_array_mut()
            .ok_or_else(|| anyhow!("array lost during traversal"))?;
        if idx >= arr.len() {
            bail!("array index `{}` out of bounds (len {})", idx, arr.len());
        }
        arr[idx] = value;
        return Ok(());
    }
    let tbl = cur
        .as_table_mut()
        .ok_or_else(|| anyhow!("target parent is not a table (cannot insert by key into a TOML scalar/array — final segment must address a table)"))?;
    tbl.insert((*last).to_string(), value);
    Ok(())
}

pub(crate) fn parse_scalar(input: &str, explicit: Option<ScalarType>) -> Result<TomlValue> {
    let ty = explicit.unwrap_or_else(|| infer_type(input));
    match ty {
        ScalarType::Str => Ok(TomlValue::String(input.to_string())),
        ScalarType::Int => Ok(TomlValue::Integer(
            input
                .parse::<i64>()
                .with_context(|| format!("`{}` is not a valid int (must parse as i64; range -9_223_372_036_854_775_808..=9_223_372_036_854_775_807)", input))?,
        )),
        ScalarType::Float => Ok(TomlValue::Float(
            input
                .parse::<f64>()
                .with_context(|| format!("`{}` is not a valid float (must parse as a finite f64; e.g. `1.5`, `-2.0e3`)", input))?,
        )),
        ScalarType::Bool => Ok(TomlValue::Boolean(
            input
                .parse::<bool>()
                .with_context(|| format!("`{}` is not a valid bool (expected `true` or `false`)", input))?,
        )),
        ScalarType::Date | ScalarType::Datetime => {
            let dt: toml::value::Datetime = input
                .parse()
                .with_context(|| format!("`{}` is not a valid TOML datetime (expected ISO-8601 date `YYYY-MM-DD` or datetime `YYYY-MM-DDTHH:MM:SSZ`)", input))?;
            Ok(TomlValue::Datetime(dt))
        }
    }
}

pub(crate) fn infer_type(s: &str) -> ScalarType {
    if s == "true" || s == "false" {
        ScalarType::Bool
    } else if looks_like_date(s) {
        ScalarType::Date
    } else if s.parse::<i64>().is_ok() {
        ScalarType::Int
    } else {
        ScalarType::Str
    }
}

pub(crate) fn looks_like_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
}

pub(crate) fn toml_to_json(v: &TomlValue) -> JsonValue {
    match v {
        TomlValue::String(s) => JsonValue::String(s.clone()),
        TomlValue::Integer(i) => JsonValue::from(*i),
        TomlValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        TomlValue::Boolean(b) => JsonValue::Bool(*b),
        TomlValue::Datetime(dt) => JsonValue::String(dt.to_string()),
        // `collect::<Vec<_>>()` presizes from the `ExactSizeIterator` hint, so
        // this arm needs no explicit `with_capacity`.
        TomlValue::Array(a) => JsonValue::Array(a.iter().map(toml_to_json).collect()),
        TomlValue::Table(t) => {
            // `serde_json::Map::with_capacity` is available only because
            // `serde_json` is built with `preserve_order` (Cargo.toml), which
            // backs `Map` with `IndexMap`. Saves the grow/rehash chain on
            // every nested table conversion.
            let mut m = serde_json::Map::with_capacity(t.len());
            for (k, v) in t.iter() {
                m.insert(k.clone(), toml_to_json(v));
            }
            JsonValue::Object(m)
        }
    }
}

/// Borrowed-lifetime sibling of `toml_to_json`. Walks the
/// `toml::de::DeTable<'a>` produced by `io::read_doc_borrowed` and emits an
/// owned `serde_json::Value`. The key win over `toml_to_json` is that
/// `DeTable` leaves unescaped strings as `Cow::Borrowed(&'a str)`; here we
/// `.to_string()` them only once at the leaf (into the owned `JsonValue`),
/// avoiding the intermediate `String` clone that `toml::from_str::<TomlValue>`
/// makes unconditionally on every string node. Integers and floats are
/// preserved by round-tripping through their text representation — `DeInteger`
/// / `DeFloat` expose `as_str()` + `radix()` rather than a decoded numeric
/// value, so the cheapest parser-faithful path is a single `i64::from_str`
/// / `f64::from_str` per scalar, which matches what `toml::from_str` does
/// internally.
pub(crate) fn detable_to_json(table: &toml::de::DeTable<'_>) -> JsonValue {
    let mut m = serde_json::Map::with_capacity(table.len());
    for (k, v) in table.iter() {
        // `k` is `Spanned<DeString<'_>>` where `DeString = Cow<'_, str>`.
        // `get_ref()` returns the inner `Cow`; deref to `&str` then own once.
        let key: &str = k.get_ref();
        m.insert(key.to_string(), devalue_to_json(v.get_ref()));
    }
    JsonValue::Object(m)
}

/// `DeValue` → `JsonValue`. Mirrors `toml_to_json`'s arm shape so
/// JSON output for a borrowed parse is byte-identical to the owned parse.
fn devalue_to_json(v: &toml::de::DeValue<'_>) -> JsonValue {
    use toml::de::DeValue;
    match v {
        DeValue::String(s) => {
            // `DeString<'i> = Cow<'i, str>`; `.as_ref()` yields `&str`.
            JsonValue::String((s.as_ref() as &str).to_string())
        }
        DeValue::Integer(n) => {
            // `DeInteger` stores the text + radix; parse once per leaf.
            // Match the existing serde-driven parse path by trusting i64.
            let txt = n.as_str();
            let radix = n.radix();
            // `i64::from_str_radix` doesn't accept a leading `+` or an
            // underscore separator; `DeInteger::as_str()` strips those per
            // the crate's own serde deserializer. If parsing fails for any
            // exotic case, fall back to JsonValue::Null rather than panic —
            // the owned `toml_to_json` does not crash either, and a test
            // exercising round-trip against the owned path would catch a
            // divergence.
            match i64::from_str_radix(txt, radix) {
                Ok(i) => JsonValue::from(i),
                Err(_) => JsonValue::Null,
            }
        }
        DeValue::Float(f) => {
            let txt = f.as_str();
            match txt.parse::<f64>() {
                Ok(x) => serde_json::Number::from_f64(x)
                    .map(JsonValue::Number)
                    .unwrap_or(JsonValue::Null),
                Err(_) => JsonValue::Null,
            }
        }
        DeValue::Boolean(b) => JsonValue::Bool(*b),
        DeValue::Datetime(dt) => JsonValue::String(dt.to_string()),
        DeValue::Array(arr) => {
            let mut out: Vec<JsonValue> = Vec::with_capacity(arr.len());
            for item in arr.iter() {
                out.push(devalue_to_json(item.get_ref()));
            }
            JsonValue::Array(out)
        }
        DeValue::Table(tbl) => detable_to_json(tbl),
    }
}

pub(crate) fn json_to_toml(v: &JsonValue) -> Result<TomlValue> {
    match v {
        JsonValue::Null => bail!(
            "TOML has no null type; remove the field or replace with an explicit empty value (`\"\"` for strings, `[]` for arrays)"
        ),
        JsonValue::Bool(b) => Ok(TomlValue::Boolean(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(TomlValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(TomlValue::Float(f))
            } else {
                bail!(
                    "JSON number `{}` is not representable as TOML int or float (must fit i64 or be finite f64)",
                    n
                )
            }
        }
        JsonValue::String(s) => Ok(TomlValue::String(s.clone())),
        JsonValue::Array(a) => {
            // `Result<Vec<_>>::from_iter` short-circuits on `Err` and
            // does NOT honour `size_hint`, so build the Vec explicitly with
            // a presized buffer and push, propagating errors as we go.
            let mut items: Vec<TomlValue> = Vec::with_capacity(a.len());
            for v in a.iter() {
                items.push(json_to_toml(v)?);
            }
            Ok(TomlValue::Array(items))
        }
        JsonValue::Object(m) => {
            // Presize via `toml::Table::with_capacity` — available
            // because `toml` is built with `preserve_order` (Cargo.toml),
            // backing `Table` with `IndexMap`.
            let mut t = toml::Table::with_capacity(m.len());
            for (k, v) in m.iter() {
                t.insert(k.clone(), json_to_toml(v)?);
            }
            Ok(TomlValue::Table(t))
        }
    }
}

/// Jump-table membership test mirroring `DATE_KEYS` exactly — the hot-path
/// lookup used per key on every JSON object inserted. `DATE_KEYS` remains the
/// enumerable form, iterated by `items.rs` in the
/// `date_keys_roundtrip_as_toml_datetime` parity test. Extend this `matches!`
/// and `DATE_KEYS` together: the `debug_assert_eq!` in `maybe_date_coerce`
/// turns drift between them into a test failure rather than a runtime one.
#[inline]
pub(crate) fn is_date_key(key: &str) -> bool {
    matches!(
        key,
        "created" | "updated" | "first_flagged" | "last_updated" | "resolved" | "date"
    )
}

pub(crate) fn maybe_date_coerce(key: &str, v: &JsonValue) -> Result<TomlValue> {
    debug_assert_eq!(
        is_date_key(key),
        DATE_KEYS.contains(&key),
        "is_date_key must stay in sync with DATE_KEYS (key = {key:?})"
    );
    if is_date_key(key)
        && let JsonValue::String(s) = v
        && let Ok(dt) = s.parse::<toml::value::Datetime>()
    {
        return Ok(TomlValue::Datetime(dt));
    }
    json_to_toml(v)
}

/// Read a string field out of a TOML table, defaulting to `""` when the key is
/// missing or the value is not a string.
pub(crate) fn str_field<'a>(tbl: &'a toml::Table, key: &str) -> &'a str {
    tbl.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Read an integer field out of a TOML table, defaulting to `0` when missing
/// or non-integer. Companion to `str_field`.
pub(crate) fn i64_field(tbl: &toml::Table, key: &str) -> i64 {
    tbl.get(key).and_then(|v| v.as_integer()).unwrap_or(0)
}

/// JSON-side sibling of `str_field`. Returns `""` when the key is missing or
/// the value is not a JSON string. Used by the borrowed-DeTable fast-path's
/// dedup tiers in `dedup.rs`, mirroring the TOML-side "empty string on
/// missing / non-string" semantics so the two paths produce byte-identical
/// fingerprints and grouping keys for the same underlying data.
pub(crate) fn str_field_json<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> &'a str {
    obj.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// JSON-side sibling of `i64_field`. Returns `0` when the key is
/// missing or the value is not a JSON integer. Used by `find_duplicates_tier_c_json`
/// to read the `line` field; the TOML side reads `i64_field(tbl, "line")`
/// and the JSON side must match its "non-integer / missing → 0" semantics
/// byte-for-byte so `--tier C` produces the same line-window grouping
/// regardless of which read path delivered the doc.
pub(crate) fn i64_field_json(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> i64 {
    obj.get(key).and_then(|v| v.as_i64()).unwrap_or(0)
}

/// Return the JSON type-name discriminant for a `serde_json::Value`
/// without echoing any user-supplied content. Used in error messages on
/// apply-op parse failures, where the value could be an agent-generated
/// `resolution` / `wontfix_rationale` string and would otherwise land on
/// stderr verbatim.
pub(crate) fn json_type_name(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// Recognised `@type:` prefix tags for the query-engine RHS grammar.
/// Single source of truth shared by `parse_typed_value`, `compare_typed`,
/// and `query::eq_typed` so the tag list doesn't drift across three call
/// sites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeHint {
    Date,
    DateTime,
    Int,
    Float,
    Bool,
    Str,
}

/// If `s` opens with a recognised `@<tag>:` prefix, return
/// `Some((hint, rest_after_prefix))`. Otherwise `None`.
///
/// Tags recognised: `date`, `datetime`, `int`, `float`, `bool`,
/// `string`, `str`. Both `@string:` and `@str:` map to `TypeHint::Str`.
pub(crate) fn split_type_hint(s: &str) -> Option<(TypeHint, &str)> {
    if let Some(rest) = s.strip_prefix("@date:") {
        return Some((TypeHint::Date, rest));
    }
    if let Some(rest) = s.strip_prefix("@datetime:") {
        return Some((TypeHint::DateTime, rest));
    }
    if let Some(rest) = s.strip_prefix("@int:") {
        return Some((TypeHint::Int, rest));
    }
    if let Some(rest) = s.strip_prefix("@float:") {
        return Some((TypeHint::Float, rest));
    }
    if let Some(rest) = s.strip_prefix("@bool:") {
        return Some((TypeHint::Bool, rest));
    }
    if let Some(rest) = s.strip_prefix("@string:") {
        return Some((TypeHint::Str, rest));
    }
    if let Some(rest) = s.strip_prefix("@str:") {
        return Some((TypeHint::Str, rest));
    }
    None
}

/// Parse a query-engine RHS string into a JSON scalar using the `@type:`
/// prefix convention documented in the plan:
///
/// * `@date:YYYY-MM-DD` / `@datetime:…` → JSON string (normalised ISO form —
///   the query engine compares this against TOML `Datetime::to_string()`).
/// * `@int:N`                            → JSON integer.
/// * `@float:X`                          → JSON number (float).
/// * `@bool:true|false`                  → JSON bool.
/// * `@string:…` / `@str:…`              → JSON string (explicit opt-out of
///   native-type coercion on the field side).
/// * No prefix                           → JSON string; the caller handles
///   native-type coercion based on the field's actual TOML type.
pub(crate) fn parse_typed_value(s: &str) -> Result<JsonValue> {
    let Some((hint, rest)) = split_type_hint(s) else {
        return Ok(JsonValue::String(s.to_string()));
    };
    match hint {
        TypeHint::Date => {
            let _dt: toml::value::Datetime = rest.parse().with_context(|| {
                format!(
                    "`{}` is not a valid ISO date (expected `YYYY-MM-DD` after `@date:`)",
                    rest
                )
            })?;
            Ok(JsonValue::String(rest.to_string()))
        }
        TypeHint::DateTime => {
            let _dt: toml::value::Datetime = rest
                .parse()
                .with_context(|| format!("`{}` is not a valid ISO datetime (expected `YYYY-MM-DDTHH:MM:SSZ` after `@datetime:`)", rest))?;
            Ok(JsonValue::String(rest.to_string()))
        }
        TypeHint::Int => {
            let n: i64 = rest.parse().with_context(|| {
                format!(
                    "`{}` is not a valid int (expected an integer after `@int:`, e.g. `@int:42`)",
                    rest
                )
            })?;
            Ok(JsonValue::from(n))
        }
        TypeHint::Float => {
            let f: f64 = rest
                .parse()
                .with_context(|| format!("`{}` is not a valid float (expected a finite float after `@float:`, e.g. `@float:1.5`)", rest))?;
            Ok(JsonValue::from(f))
        }
        TypeHint::Bool => {
            let b: bool = rest.parse().with_context(|| {
                format!(
                    "`{}` is not a valid bool (expected `true` or `false` after `@bool:`)",
                    rest
                )
            })?;
            Ok(JsonValue::Bool(b))
        }
        TypeHint::Str => Ok(JsonValue::String(rest.to_string())),
    }
}

/// Ordered comparison between a TOML field and a raw RHS string. Used by the
/// query engine's Gt/Gte/Lt/Lte predicates.
///
/// Dispatch:
///   * RHS has an `@type:` prefix → parse RHS per the prefix, coerce to the
///     field's native type if possible, compare.
///   * RHS has no prefix → use the field's native type to drive parsing
///     (Integer → parse RHS as i64, Datetime → parse RHS as Datetime, etc.).
///     Strings compare lexicographically.
pub(crate) fn compare_typed(field: &TomlValue, rhs_raw: &str) -> Result<std::cmp::Ordering> {
    use std::cmp::Ordering;

    // Strip any @type: prefix first so we treat `@int:5` the same as bare
    // `5` when the field is an Integer.
    let (hint, body): (Option<TypeHint>, &str) = match split_type_hint(rhs_raw) {
        Some((h, rest)) => (Some(h), rest),
        None => (None, rhs_raw),
    };

    match field {
        TomlValue::Integer(i) => {
            let n: i64 = body
                .parse()
                .with_context(|| format!("`{}` is not comparable as int (RHS must parse as i64 to compare against an Integer field)", body))?;
            if hint.is_some() && !matches!(hint, Some(TypeHint::Int)) {
                bail!(
                    "type hint `{:?}` rejected; expected one of: int, float (TOML's only numeric types). Field is Integer — use `@int:` or omit the prefix.",
                    hint
                );
            }
            Ok(i.cmp(&n))
        }
        TomlValue::Float(f) => {
            let x: f64 = body
                .parse()
                .with_context(|| format!("`{}` is not comparable as float (RHS must parse as a finite f64 to compare against a Float field)", body))?;
            if hint.is_some() && !matches!(hint, Some(TypeHint::Float)) {
                bail!(
                    "type hint `{:?}` rejected; expected one of: int, float (TOML's only numeric types). Field is Float — use `@float:` or omit the prefix.",
                    hint
                );
            }
            Ok(f.partial_cmp(&x).unwrap_or(Ordering::Equal))
        }
        TomlValue::Boolean(b) => {
            let c: bool = body.parse().with_context(|| {
                format!(
                    "`{}` is not comparable as bool (expected `true` or `false`)",
                    body
                )
            })?;
            Ok(b.cmp(&c))
        }
        TomlValue::Datetime(dt) => {
            // Normalise RHS via a round-trip through toml::Datetime so that
            // `2026-04-18` and `2026-04-18T00:00:00` both compare correctly
            // against the stored value's Display form.
            let parsed: toml::value::Datetime = body
                .parse()
                .with_context(|| format!("`{}` is not a valid TOML datetime (expected ISO-8601 date `YYYY-MM-DD` or datetime `YYYY-MM-DDTHH:MM:SSZ`)", body))?;
            Ok(dt.to_string().cmp(&parsed.to_string()))
        }
        TomlValue::String(s) => Ok(s.as_str().cmp(body)),
        _ => bail!(
            "field is not a scalar; cannot compare with --where-gt/gte/lt/lte (only String, Integer, Float, Boolean, Datetime fields are orderable)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `detable_to_json` over a borrowed `DeTable` must produce the same JSON
    /// shape as `toml_to_json` over an owned `TomlValue` for
    /// every scalar kind the flow schemas exercise (string, integer, float,
    /// bool, date, nested table, array-of-tables). Pins the borrowed
    /// fast-path byte-identical to the owned path so a regression in either
    /// converter surfaces immediately.
    #[test]
    fn detable_to_json_matches_toml_to_json_shape_for_every_scalar() {
        let src = r#"
schema_version = 1
last_updated = 2026-04-18
title = "mixed"
ratio = 1.25
ok = true
tags = ["a", "b"]

[[items]]
id = "R1"
file = "src/a.rs"
line = 10
first_flagged = 2026-04-08

[[items]]
id = "R2"
file = "src/b.rs"
line = 20
first_flagged = 2026-04-09

[nested.inner]
key = "value"
count = 3
"#;
        let owned: TomlValue = toml::from_str(src).unwrap();
        let owned_json = toml_to_json(&owned);

        let spanned = toml::de::DeTable::parse(src).unwrap();
        let borrowed_json = detable_to_json(spanned.get_ref());

        assert_eq!(
            owned_json, borrowed_json,
            "detable_to_json must match toml_to_json byte-for-byte; \
             owned={owned_json}, borrowed={borrowed_json}"
        );
    }

    /// A type-coercion error must enumerate the acceptable types (or the
    /// parse contract) so an agent reading it knows which RHS forms are
    /// accepted. `parse_scalar` for `ScalarType::Int` on a non-integer input
    /// names the i64 contract directly.
    #[test]
    fn error_message_type_coercion_enumerates_int_contract() {
        let err = parse_scalar("not-a-number", Some(ScalarType::Int))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("not-a-number")
                && err.contains("is not a valid int")
                && err.contains("i64"),
            "type-coercion error must name the bad input AND the i64 contract; got: {err}"
        );
    }

    /// `compare_typed` must reject a type-hint mismatch with an enumeration
    /// of the acceptable hints. Comparing an Integer field with `@string:42`
    /// is the canonical probe — the body parses as i64 (passes the parse
    /// step) but the type hint is rejected by the post-parse `matches!`
    /// check, whose bail message enumerates the acceptable numeric prefixes
    /// (`int, float`).
    #[test]
    fn error_message_type_coercion_enumerates_acceptable_type_hints() {
        let field = TomlValue::Integer(42);
        let err = compare_typed(&field, "@string:42").unwrap_err().to_string();
        assert!(
            err.contains("rejected") && err.contains("int") && err.contains("float"),
            "type-coercion error must enumerate the acceptable numeric hints; got: {err}"
        );
    }

    /// A path-shape error must quote the expected form. `set_at_path` on a
    /// non-numeric segment under an array parent names the array-index shape
    /// (`items.0.id`) so the caller sees a concrete example.
    #[test]
    fn error_message_path_shape_quotes_expected_array_index_form() {
        let mut root: TomlValue = toml::from_str(
            r#"
[[items]]
id = "R1"
"#,
        )
        .unwrap();
        // `items.notanindex.id` triggers the parent-array index-parse branch
        // because the parent traversal must convert `notanindex` to a usize
        // to descend into the array.
        let err = set_at_path(
            &mut root,
            "items.notanindex.id",
            TomlValue::String("x".into()),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("not a valid array index") && err.contains("items.0"),
            "path-shape error must quote the expected array-index form; got: {err}"
        );
    }

    /// A partial-enum error extends the original message with actionable
    /// context: `set_at_path` on an out-of-bounds array index suggests the
    /// discovery command (`tomlctl get <file> --path <parent>`).
    #[test]
    fn error_message_partial_enum_array_oob_suggests_discovery_command() {
        // Build a doc with a small array under `items`, then attempt to set
        // an element at index 99 via a parent traversal that hits the
        // pre-final-segment OOB branch.
        let mut root: TomlValue = toml::from_str(
            r#"
[[items]]
id = "R1"
"#,
        )
        .unwrap();
        // Path `items.99.id` triggers the parent-array OOB branch because
        // the traversal must descend into `items[99]` to reach `id`.
        let err = set_at_path(&mut root, "items.99.id", TomlValue::String("x".into()))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("out of bounds") && err.contains("tomlctl get"),
            "partial-enum error must extend with discovery suggestion; got: {err}"
        );
    }
}
