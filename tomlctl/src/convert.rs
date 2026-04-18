//! R59: TOML↔JSON conversion, scalar parsing, date-coercion, and dotted-path
//! traversal helpers. Split out of `main.rs` as a pure-function module with no
//! I/O or CLI coupling.
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

/// Read-side dotted-path traversal. Each segment either:
///   - indexes the current table by its key, OR
///   - (R49) when the current value is an array and the segment parses as a
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
        .ok_or_else(|| anyhow!("empty key path"))?;

    let mut cur: &mut TomlValue = root;
    for p in parents {
        // R49: parent traversal also supports integer-indexed arrays, matching
        // `navigate`. Auto-vivification of array slots is NOT supported — the
        // array index must already exist.
        if cur.is_array() {
            let idx: usize = p.parse().with_context(|| {
                format!("path segment `{}` is not a valid array index", p)
            })?;
            cur = cur
                .as_array_mut()
                .and_then(|arr| arr.get_mut(idx))
                .ok_or_else(|| anyhow!("array index `{}` out of bounds", idx))?;
            continue;
        }
        let tbl = cur
            .as_table_mut()
            .ok_or_else(|| anyhow!("path segment `{}` has a non-table parent", p))?;
        cur = tbl
            .entry((*p).to_string())
            .or_insert_with(|| TomlValue::Table(toml::Table::new()));
    }
    // Final segment: if the parent is an array and `last` parses as an index,
    // overwrite that slot; otherwise insert into the parent table by key.
    if cur.is_array() {
        let idx: usize = last.parse().with_context(|| {
            format!("final path segment `{}` is not a valid array index", last)
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
        .ok_or_else(|| anyhow!("target parent is not a table"))?;
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
                .with_context(|| format!("`{}` is not a valid int", input))?,
        )),
        ScalarType::Float => Ok(TomlValue::Float(
            input
                .parse::<f64>()
                .with_context(|| format!("`{}` is not a valid float", input))?,
        )),
        ScalarType::Bool => Ok(TomlValue::Boolean(
            input
                .parse::<bool>()
                .with_context(|| format!("`{}` is not a valid bool", input))?,
        )),
        ScalarType::Date | ScalarType::Datetime => {
            let dt: toml::value::Datetime = input
                .parse()
                .with_context(|| format!("`{}` is not a valid TOML datetime", input))?;
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
        // O39: Vec arm — `slice::Iter::map(...).collect::<Vec<_>>()` already
        // presizes via `size_hint`/ExactSizeIterator, so the iterator form
        // is equivalent to `Vec::with_capacity(a.len())` + push and is left
        // as-is.
        TomlValue::Array(a) => JsonValue::Array(a.iter().map(toml_to_json).collect()),
        TomlValue::Table(t) => {
            // O39: presize the JSON object — `serde_json::Map::with_capacity`
            // is available because `serde_json` is built with `preserve_order`
            // (Cargo.toml), which backs `Map` with `IndexMap`. Saves the
            // grow/rehash chain on every nested table conversion.
            let mut m = serde_json::Map::with_capacity(t.len());
            for (k, v) in t.iter() {
                m.insert(k.clone(), toml_to_json(v));
            }
            JsonValue::Object(m)
        }
    }
}

/// O10: borrowed-lifetime sibling of `toml_to_json`. Walks the
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

/// O10 helper: `DeValue` → `JsonValue`. Mirrors `toml_to_json`'s arm shape so
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
        JsonValue::Null => bail!("TOML has no null type"),
        JsonValue::Bool(b) => Ok(TomlValue::Boolean(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(TomlValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(TomlValue::Float(f))
            } else {
                bail!("number `{}` is not representable in TOML", n)
            }
        }
        JsonValue::String(s) => Ok(TomlValue::String(s.clone())),
        JsonValue::Array(a) => {
            // O39: `Result<Vec<_>>::from_iter` short-circuits on `Err` and
            // does NOT honour `size_hint`, so build the Vec explicitly with
            // a presized buffer and push, propagating errors as we go.
            let mut items: Vec<TomlValue> = Vec::with_capacity(a.len());
            for v in a.iter() {
                items.push(json_to_toml(v)?);
            }
            Ok(TomlValue::Array(items))
        }
        JsonValue::Object(m) => {
            // O39: presize via `toml::Table::with_capacity` — available
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

/// O38: jump-table membership test mirroring `DATE_KEYS` exactly. The const
/// is retained because `items.rs` iterates it in the
/// `date_keys_roundtrip_as_toml_datetime` parity test; this helper is the
/// hot-path lookup used per-key on every JSON object inserted. A debug-only
/// assertion pins the two lists to the same set so silent drift between this
/// `matches!` and `DATE_KEYS` fails in tests rather than at runtime.
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
/// missing or the value is not a string. R20: factors a pattern that repeated
/// 15+ times across `items_list`, `items_orphans`, and the duplicate tiers.
pub(crate) fn str_field<'a>(tbl: &'a toml::Table, key: &str) -> &'a str {
    tbl.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Read an integer field out of a TOML table, defaulting to `0` when missing
/// or non-integer. Companion to `str_field` (R20).
pub(crate) fn i64_field(tbl: &toml::Table, key: &str) -> i64 {
    tbl.get(key).and_then(|v| v.as_integer()).unwrap_or(0)
}

/// R36: return the JSON type-name discriminant for a `serde_json::Value`
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
/// sites (R66).
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
            let _dt: toml::value::Datetime = rest
                .parse()
                .with_context(|| format!("`{}` is not a valid ISO date", rest))?;
            Ok(JsonValue::String(rest.to_string()))
        }
        TypeHint::DateTime => {
            let _dt: toml::value::Datetime = rest
                .parse()
                .with_context(|| format!("`{}` is not a valid ISO datetime", rest))?;
            Ok(JsonValue::String(rest.to_string()))
        }
        TypeHint::Int => {
            let n: i64 = rest
                .parse()
                .with_context(|| format!("`{}` is not a valid int", rest))?;
            Ok(JsonValue::from(n))
        }
        TypeHint::Float => {
            let f: f64 = rest
                .parse()
                .with_context(|| format!("`{}` is not a valid float", rest))?;
            Ok(JsonValue::from(f))
        }
        TypeHint::Bool => {
            let b: bool = rest
                .parse()
                .with_context(|| format!("`{}` is not a valid bool", rest))?;
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
                .with_context(|| format!("`{}` is not comparable as int", body))?;
            if hint.is_some() && !matches!(hint, Some(TypeHint::Int)) {
                bail!("type hint `{:?}` doesn't match integer field", hint);
            }
            Ok(i.cmp(&n))
        }
        TomlValue::Float(f) => {
            let x: f64 = body
                .parse()
                .with_context(|| format!("`{}` is not comparable as float", body))?;
            if hint.is_some() && !matches!(hint, Some(TypeHint::Float)) {
                bail!("type hint `{:?}` doesn't match float field", hint);
            }
            Ok(f.partial_cmp(&x).unwrap_or(Ordering::Equal))
        }
        TomlValue::Boolean(b) => {
            let c: bool = body
                .parse()
                .with_context(|| format!("`{}` is not comparable as bool", body))?;
            Ok(b.cmp(&c))
        }
        TomlValue::Datetime(dt) => {
            // Normalise RHS via a round-trip through toml::Datetime so that
            // `2026-04-18` and `2026-04-18T00:00:00` both compare correctly
            // against the stored value's Display form.
            let parsed: toml::value::Datetime = body
                .parse()
                .with_context(|| format!("`{}` is not a valid TOML datetime", body))?;
            Ok(dt.to_string().cmp(&parsed.to_string()))
        }
        TomlValue::String(s) => Ok(s.as_str().cmp(body)),
        _ => bail!("field is not a scalar; cannot compare"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O10 parity: `detable_to_json` over a borrowed `DeTable` must produce
    /// the same JSON shape as `toml_to_json` over an owned `TomlValue` for
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
}
