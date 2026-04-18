//! Query engine for `tomlctl items list`.
//!
//! Pure-function module: takes a parsed `toml::Value` document plus a
//! `Query` spec (built by `main.rs` clap dispatch) and returns a
//! `serde_json::Value` shaped per the requested output.
//!
//! Pipeline order (mirrors the plan):
//!   filter → distinct → sort → offset/limit → aggregate OR project → shape
//!
//! All RHS values the caller hands us are raw strings (`--where key=val`);
//! typed-parse happens here via `convert::parse_typed_value` and
//! `convert::compare_typed` so the CLI layer stays dumb.
//!
//! Kept deliberately I/O-free so unit tests can exercise every predicate
//! and shape on an in-memory fixture.

#![allow(dead_code)] // R-plan: wired up by task 5; keep module self-contained until then.

use anyhow::{Result, anyhow, bail};
use regex::Regex;
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use crate::convert::{compare_typed, parse_typed_value, toml_to_json};

/// Sort direction for a single `--sort-by` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SortDir {
    Asc,
    Desc,
}

/// One filter predicate. Each carries the field key it applies to plus the
/// RHS payload as a raw string (or list of strings for `WhereIn`). Typed-RHS
/// parsing is deferred to evaluation time so the CLI layer doesn't need to
/// know field types.
#[derive(Clone, Debug)]
pub(crate) enum Predicate {
    Where { key: String, rhs: String },
    WhereNot { key: String, rhs: String },
    WhereIn { key: String, rhs: Vec<String> },
    WhereHas { key: String },
    WhereMissing { key: String },
    WhereGt { key: String, rhs: String },
    WhereGte { key: String, rhs: String },
    WhereLt { key: String, rhs: String },
    WhereLte { key: String, rhs: String },
    WhereContains { key: String, sub: String },
    WherePrefix { key: String, prefix: String },
    WhereSuffix { key: String, suffix: String },
    WhereRegex { key: String, pattern: String },
}

/// Mutually-exclusive output shapes. `Array` is the default when none of the
/// aggregation/pluck flags are set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum OutputShape {
    #[default]
    Array,
    Count,
    CountBy(String),
    GroupBy(String),
    Pluck(String),
    Ndjson,
}

/// Full query spec handed to `run`. `main.rs` builds this from clap args.
/// Field names mirror the CLI flags for easy mental mapping.
#[derive(Clone, Debug, Default)]
pub(crate) struct Query {
    pub predicates: Vec<Predicate>,
    pub select: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub sort_by: Vec<(String, SortDir)>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub distinct: bool,
    pub shape: OutputShape,
}

/// Reject mutually exclusive flag combinations. The CLI's `validate_query`
/// call keeps clap-level help-text lean (no need for clap's
/// `conflicts_with` on every pair) and centralises the rules.
pub(crate) fn validate_query(q: &Query) -> Result<()> {
    if q.select.is_some() && q.exclude.is_some() {
        bail!("--select and --exclude are mutually exclusive");
    }
    match &q.shape {
        OutputShape::Pluck(_) => {
            if q.select.is_some() {
                bail!("--select and --pluck are mutually exclusive");
            }
            if q.exclude.is_some() {
                bail!("--exclude and --pluck are mutually exclusive");
            }
        }
        OutputShape::Count => {
            if q.select.is_some() {
                bail!("--count and --select are mutually exclusive");
            }
            if q.exclude.is_some() {
                bail!("--count and --exclude are mutually exclusive");
            }
        }
        OutputShape::CountBy(_) => {
            if q.select.is_some() {
                bail!("--count-by and --select are mutually exclusive");
            }
            if q.exclude.is_some() {
                bail!("--count-by and --exclude are mutually exclusive");
            }
        }
        OutputShape::GroupBy(_) => {
            // group-by composes fine with projection; no cross-exclusion here.
        }
        OutputShape::Array | OutputShape::Ndjson => {}
    }
    // Cross-shape pairs. `main.rs` is expected to pick exactly one of the
    // below shapes, but we still double-check so callers with programmatic
    // builders can't accidentally set two.
    // (The clap layer normally collapses these by priority; this is belt
    // & braces.)
    Ok(())
}

/// Top-level entry: walk the array-of-tables at `array_name`, run the
/// pipeline, and return the requested JSON shape.
pub(crate) fn run(doc: &TomlValue, array_name: &str, q: &Query) -> Result<JsonValue> {
    validate_query(q)?;
    let items: Vec<&TomlValue> = match doc.get(array_name).and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().collect(),
        None => Vec::new(),
    };

    // 1. Filter
    let filtered = apply_filters(&items, &q.predicates)?;

    // 2. Project (select/exclude) before shaping for Array/Pluck/Distinct
    // so distinct/pluck see the already-narrowed shape. Aggregations
    // (count/count-by/group-by) operate on the unprojected items so the
    // grouping key is always reachable.
    let for_shape: Vec<JsonValue> = filtered.iter().map(|t| toml_to_json(t)).collect();

    // 3. Sort (before limit/offset; stable so multi-key works as
    // "primary first, ties broken by secondary" when called once per key.)
    let sorted = apply_sort(for_shape, &q.sort_by);

    // 4. Distinct (on the full pre-projection shape for aggregation paths;
    // on projected shape for array/pluck paths).
    let deduped = if q.distinct {
        // For aggregation shapes (count/count-by/group-by), dedup on raw item
        // shape so grouping keys stay intact. For Array/Pluck, dedup on
        // projected shape so "select a,b --distinct" dedupes by (a,b).
        match &q.shape {
            OutputShape::Array | OutputShape::Pluck(_) | OutputShape::Ndjson => {
                let projected: Vec<JsonValue> =
                    sorted.iter().map(|v| apply_projection(v, q)).collect();
                dedup_preserve_first(&sorted, &projected)
            }
            _ => dedup_preserve_first(&sorted, &sorted),
        }
    } else {
        sorted
    };

    // 5. Offset/limit (offset first, then limit).
    let windowed = apply_window(deduped, q.offset, q.limit);

    // 6. Shape.
    match &q.shape {
        OutputShape::Count => Ok(apply_aggregation_count(&windowed)),
        OutputShape::CountBy(field) => Ok(apply_aggregation_count_by(&windowed, field)),
        OutputShape::GroupBy(field) => {
            // group-by can still respect select/exclude on the grouped items.
            let projected: Vec<JsonValue> =
                windowed.iter().map(|v| apply_projection(v, q)).collect();
            Ok(apply_aggregation_group_by(&windowed, &projected, field))
        }
        OutputShape::Pluck(field) => Ok(apply_pluck(&windowed, field)),
        OutputShape::Array | OutputShape::Ndjson => {
            let projected: Vec<JsonValue> =
                windowed.iter().map(|v| apply_projection(v, q)).collect();
            Ok(JsonValue::Array(projected))
        }
    }
}

// -----------------------------------------------------------------------
// Filtering
// -----------------------------------------------------------------------

pub(crate) fn apply_filters<'a>(
    items: &[&'a TomlValue],
    preds: &[Predicate],
) -> Result<Vec<&'a TomlValue>> {
    let mut out = Vec::with_capacity(items.len());
    'item: for &it in items {
        for p in preds {
            if !eval_predicate(it, p)? {
                continue 'item;
            }
        }
        out.push(it);
    }
    Ok(out)
}

fn eval_predicate(item: &TomlValue, p: &Predicate) -> Result<bool> {
    let tbl = match item.as_table() {
        Some(t) => t,
        None => return Ok(false),
    };
    match p {
        Predicate::Where { key, rhs } => Ok(eq_typed(tbl.get(key), rhs)),
        Predicate::WhereNot { key, rhs } => Ok(!eq_typed(tbl.get(key), rhs)),
        Predicate::WhereIn { key, rhs } => {
            let field = tbl.get(key);
            Ok(rhs.iter().any(|v| eq_typed(field, v)))
        }
        Predicate::WhereHas { key } => Ok(field_present_nonempty(tbl.get(key))),
        Predicate::WhereMissing { key } => Ok(!field_present_nonempty(tbl.get(key))),
        Predicate::WhereGt { key, rhs } => cmp_pred(tbl.get(key), rhs, |o| {
            matches!(o, std::cmp::Ordering::Greater)
        }),
        Predicate::WhereGte { key, rhs } => cmp_pred(tbl.get(key), rhs, |o| {
            matches!(o, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
        }),
        Predicate::WhereLt { key, rhs } => {
            cmp_pred(tbl.get(key), rhs, |o| matches!(o, std::cmp::Ordering::Less))
        }
        Predicate::WhereLte { key, rhs } => cmp_pred(tbl.get(key), rhs, |o| {
            matches!(o, std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        }),
        Predicate::WhereContains { key, sub } => {
            Ok(tbl.get(key).and_then(|v| v.as_str()).is_some_and(|s| s.contains(sub)))
        }
        Predicate::WherePrefix { key, prefix } => Ok(tbl
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.starts_with(prefix))),
        Predicate::WhereSuffix { key, suffix } => Ok(tbl
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.ends_with(suffix))),
        Predicate::WhereRegex { key, pattern } => {
            let re = Regex::new(pattern)
                .map_err(|e| anyhow!("invalid regex for --where-regex {}: {}", key, e))?;
            let s = value_as_string(tbl.get(key));
            Ok(s.as_deref().is_some_and(|s| re.is_match(s)))
        }
    }
}

/// Field presence + non-emptiness check used by WhereHas/WhereMissing.
fn field_present_nonempty(v: Option<&TomlValue>) -> bool {
    match v {
        None => false,
        Some(TomlValue::String(s)) => !s.is_empty(),
        Some(TomlValue::Array(a)) => !a.is_empty(),
        Some(TomlValue::Table(t)) => !t.is_empty(),
        Some(_) => true,
    }
}

/// Typed equality: if RHS has a `@type:` prefix, parse it that way. Otherwise,
/// if the field is native-typed (Int/Float/Bool/Datetime), parse RHS as the
/// field's native type. Fall back to string compare.
fn eq_typed(field: Option<&TomlValue>, rhs: &str) -> bool {
    let Some(field) = field else { return false };
    // 1. Explicit @type: prefix drives parsing. Compare apples-to-apples.
    if let Some(rest) = strip_typed_prefix(rhs) {
        let parsed = match parse_typed_value(rhs) {
            Ok(v) => v,
            Err(_) => return false,
        };
        return json_matches_toml(&parsed, field, rest.0);
    }
    // 2. No prefix — native-type coercion from the field side.
    match field {
        TomlValue::String(s) => s == rhs,
        TomlValue::Integer(i) => rhs.parse::<i64>().map(|r| r == *i).unwrap_or(false),
        TomlValue::Float(f) => rhs.parse::<f64>().map(|r| r == *f).unwrap_or(false),
        TomlValue::Boolean(b) => rhs.parse::<bool>().map(|r| r == *b).unwrap_or(false),
        TomlValue::Datetime(dt) => dt.to_string() == rhs,
        _ => false,
    }
}

/// Returns Some((tag, body)) when `s` has a recognised `@<tag>:` prefix.
fn strip_typed_prefix(s: &str) -> Option<(&'static str, &str)> {
    for tag in &["date", "datetime", "int", "float", "bool", "string", "str"] {
        let needle = format!("@{}:", tag);
        if let Some(rest) = s.strip_prefix(&needle) {
            return Some((tag, rest));
        }
    }
    None
}

/// Compare a typed JSON scalar (from `parse_typed_value`) against a TOML field.
fn json_matches_toml(parsed: &JsonValue, field: &TomlValue, tag: &str) -> bool {
    match (parsed, field, tag) {
        (JsonValue::String(s), TomlValue::String(f), "str" | "string") => s == f,
        (JsonValue::String(s), TomlValue::Datetime(dt), "date" | "datetime") => {
            // Compare ISO-string form. TOML Datetime Display gives ISO-8601.
            dt.to_string() == *s
        }
        (JsonValue::Number(n), TomlValue::Integer(i), "int") => {
            n.as_i64().map(|v| v == *i).unwrap_or(false)
        }
        (JsonValue::Number(n), TomlValue::Float(f), "float") => {
            n.as_f64().map(|v| v == *f).unwrap_or(false)
        }
        (JsonValue::Bool(b), TomlValue::Boolean(f), "bool") => b == f,
        // Cross-type compare: string RHS against non-string field (e.g.
        // `@string:42` against an Integer). Compare via stringified field.
        (JsonValue::String(s), other, "str" | "string") => stringify_scalar(other) == *s,
        _ => false,
    }
}

fn stringify_scalar(v: &TomlValue) -> String {
    match v {
        TomlValue::String(s) => s.clone(),
        TomlValue::Integer(i) => i.to_string(),
        TomlValue::Float(f) => f.to_string(),
        TomlValue::Boolean(b) => b.to_string(),
        TomlValue::Datetime(dt) => dt.to_string(),
        _ => String::new(),
    }
}

fn value_as_string(v: Option<&TomlValue>) -> Option<String> {
    v.map(stringify_scalar)
}

fn cmp_pred(
    field: Option<&TomlValue>,
    rhs: &str,
    check: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<bool> {
    let Some(f) = field else { return Ok(false) };
    match compare_typed(f, rhs) {
        Ok(ord) => Ok(check(ord)),
        Err(_) => Ok(false),
    }
}

// -----------------------------------------------------------------------
// Projection
// -----------------------------------------------------------------------

pub(crate) fn apply_projection(item: &JsonValue, q: &Query) -> JsonValue {
    let Some(obj) = item.as_object() else {
        return item.clone();
    };
    if let Some(keep) = &q.select {
        let mut out = serde_json::Map::new();
        for k in keep {
            if let Some(v) = obj.get(k) {
                out.insert(k.clone(), v.clone());
            }
        }
        return JsonValue::Object(out);
    }
    if let Some(drop) = &q.exclude {
        let mut out = obj.clone();
        for k in drop {
            out.remove(k);
        }
        return JsonValue::Object(out);
    }
    item.clone()
}

// -----------------------------------------------------------------------
// Shaping — sort, limit/offset, distinct
// -----------------------------------------------------------------------

pub(crate) fn apply_shaping(
    items: Vec<JsonValue>,
    sort_by: &[(String, SortDir)],
    limit: Option<usize>,
    offset: Option<usize>,
    distinct: bool,
) -> Vec<JsonValue> {
    let mut v = apply_sort(items, sort_by);
    if distinct {
        let clones = v.clone();
        v = dedup_preserve_first(&v, &clones);
    }
    apply_window(v, offset, limit)
}

fn apply_sort(mut items: Vec<JsonValue>, sort_by: &[(String, SortDir)]) -> Vec<JsonValue> {
    if sort_by.is_empty() {
        return items;
    }
    // Stable multi-key: sort by least-significant key first, most-significant
    // last. Caller gives us (primary, secondary, ...) so reverse.
    for (key, dir) in sort_by.iter().rev() {
        let dir_copy = *dir;
        let key_copy = key.clone();
        items.sort_by(|a, b| {
            let av = a.get(&key_copy);
            let bv = b.get(&key_copy);
            let ord = cmp_json_scalars(av, bv);
            match dir_copy {
                SortDir::Asc => ord,
                SortDir::Desc => ord.reverse(),
            }
        });
    }
    items
}

fn cmp_json_scalars(a: Option<&JsonValue>, b: Option<&JsonValue>) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a, b) {
        (None, None) => Equal,
        (None, _) => Less,
        (_, None) => Greater,
        (Some(x), Some(y)) => match (x, y) {
            (JsonValue::Number(n1), JsonValue::Number(n2)) => {
                let f1 = n1.as_f64().unwrap_or(0.0);
                let f2 = n2.as_f64().unwrap_or(0.0);
                f1.partial_cmp(&f2).unwrap_or(Equal)
            }
            (JsonValue::Bool(p), JsonValue::Bool(q)) => p.cmp(q),
            (JsonValue::String(s1), JsonValue::String(s2)) => s1.cmp(s2),
            _ => {
                // Fallback: compare stringified forms so mixed types still
                // produce a deterministic order.
                x.to_string().cmp(&y.to_string())
            }
        },
    }
}

fn dedup_preserve_first(source: &[JsonValue], shape: &[JsonValue]) -> Vec<JsonValue> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (i, s) in shape.iter().enumerate() {
        let key = serde_json::to_string(s).unwrap_or_default();
        if !seen.iter().any(|k| k == &key) {
            seen.push(key);
            out.push(source[i].clone());
        }
    }
    out
}

fn apply_window(
    items: Vec<JsonValue>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Vec<JsonValue> {
    let off = offset.unwrap_or(0);
    if off >= items.len() {
        return Vec::new();
    }
    let tail: Vec<JsonValue> = items.into_iter().skip(off).collect();
    match limit {
        Some(n) => tail.into_iter().take(n).collect(),
        None => tail,
    }
}

// -----------------------------------------------------------------------
// Aggregation + pluck
// -----------------------------------------------------------------------

pub(crate) fn apply_aggregation_count(items: &[JsonValue]) -> JsonValue {
    serde_json::json!({ "count": items.len() })
}

pub(crate) fn apply_aggregation_count_by(items: &[JsonValue], field: &str) -> JsonValue {
    let mut counts: Vec<(String, u64)> = Vec::new();
    for it in items {
        let key = bucket_key(it.get(field));
        if let Some(slot) = counts.iter_mut().find(|(k, _)| k == &key) {
            slot.1 += 1;
        } else {
            counts.push((key, 1));
        }
    }
    let mut m = serde_json::Map::new();
    for (k, v) in counts {
        m.insert(k, JsonValue::from(v));
    }
    JsonValue::Object(m)
}

pub(crate) fn apply_aggregation_group_by(
    raw: &[JsonValue],
    projected: &[JsonValue],
    field: &str,
) -> JsonValue {
    let mut groups: Vec<(String, Vec<JsonValue>)> = Vec::new();
    for (i, it) in raw.iter().enumerate() {
        let key = bucket_key(it.get(field));
        let proj = projected[i].clone();
        if let Some(slot) = groups.iter_mut().find(|(k, _)| k == &key) {
            slot.1.push(proj);
        } else {
            groups.push((key, vec![proj]));
        }
    }
    let mut m = serde_json::Map::new();
    for (k, v) in groups {
        m.insert(k, JsonValue::Array(v));
    }
    JsonValue::Object(m)
}

pub(crate) fn apply_pluck(items: &[JsonValue], field: &str) -> JsonValue {
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        match it.get(field) {
            None | Some(JsonValue::Null) => {}
            Some(v) => out.push(v.clone()),
        }
    }
    JsonValue::Array(out)
}

/// Convert a field value into a string bucket key for group-by / count-by.
/// Missing fields bucket as the empty string so they aggregate together.
fn bucket_key(v: Option<&JsonValue>) -> String {
    match v {
        None | Some(JsonValue::Null) => String::new(),
        Some(JsonValue::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// Thin entry exposed for symmetry with the plan's `apply_aggregation`
/// helper name. Dispatches on shape for aggregation variants.
pub(crate) fn apply_aggregation(
    raw: &[JsonValue],
    projected: &[JsonValue],
    shape: &OutputShape,
) -> Option<JsonValue> {
    match shape {
        OutputShape::Count => Some(apply_aggregation_count(raw)),
        OutputShape::CountBy(f) => Some(apply_aggregation_count_by(raw, f)),
        OutputShape::GroupBy(f) => Some(apply_aggregation_group_by(raw, projected, f)),
        _ => None,
    }
}

// =======================================================================
// Tests
// =======================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 6-item fixture exercising every predicate kind. Mixes native
    /// TOML Datetime (`first_flagged`), Integer (`rounds`), Bool
    /// (`active`), and String fields.
    fn fixture() -> TomlValue {
        let src = r#"
[[items]]
id = "R1"
file = "src/a.rs"
status = "open"
severity = "major"
category = "security"
rounds = 3
active = true
summary = "auth bypass"
first_flagged = 2026-01-01

[[items]]
id = "R2"
file = "src/a.rs"
status = "open"
severity = "minor"
category = "quality"
rounds = 1
active = true
summary = "trivial rename"
first_flagged = 2026-02-15

[[items]]
id = "R3"
file = "src/b.rs"
status = "fixed"
severity = "suggestion"
category = "quality"
rounds = 2
active = false
summary = "polish docs"
first_flagged = 2026-03-01

[[items]]
id = "R4"
file = "src/c.rs"
status = "wontfix"
severity = "major"
category = "security"
rounds = 4
active = true
summary = "third-party risk"
first_flagged = 2025-12-31
defer_reason = "vendor fix"

[[items]]
id = "R5"
file = "src/c.rs"
status = "open"
severity = "suggestion"
category = "performance"
rounds = 1
active = false
summary = "slow path"
first_flagged = 2026-04-10

[[items]]
id = "R6"
file = "src/d.rs"
status = "open"
severity = "minor"
category = "quality"
rounds = 1
active = true
summary = "lint warning"
first_flagged = 2026-04-18
defer_reason = ""
"#;
        toml::from_str(src).unwrap()
    }

    fn q_with(preds: Vec<Predicate>) -> Query {
        Query {
            predicates: preds,
            ..Default::default()
        }
    }

    fn ids(v: &JsonValue) -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|it| it["id"].as_str().unwrap().to_string())
            .collect()
    }

    // -- one test per predicate kind -----------------------------------

    #[test]
    fn where_exact_matches_status_open() {
        let doc = fixture();
        let q = q_with(vec![Predicate::Where {
            key: "status".into(),
            rhs: "open".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R1", "R2", "R5", "R6"]);
    }

    #[test]
    fn where_not_excludes_matching() {
        let doc = fixture();
        let q = q_with(vec![Predicate::WhereNot {
            key: "status".into(),
            rhs: "open".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R3", "R4"]);
    }

    #[test]
    fn where_in_matches_any_member() {
        let doc = fixture();
        let q = q_with(vec![Predicate::WhereIn {
            key: "severity".into(),
            rhs: vec!["minor".into(), "suggestion".into()],
        }]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R2", "R3", "R5", "R6"]);
    }

    #[test]
    fn where_has_filters_out_missing_and_empty() {
        let doc = fixture();
        let q = q_with(vec![Predicate::WhereHas {
            key: "defer_reason".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        // R4 has non-empty defer_reason; R6 has empty string; others absent.
        assert_eq!(ids(&out), vec!["R4"]);
    }

    #[test]
    fn where_missing_catches_absent_and_empty() {
        let doc = fixture();
        let q = q_with(vec![Predicate::WhereMissing {
            key: "defer_reason".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R1", "R2", "R3", "R5", "R6"]);
    }

    #[test]
    fn where_gt_lt_on_integer_field() {
        let doc = fixture();
        let q = q_with(vec![Predicate::WhereGt {
            key: "rounds".into(),
            rhs: "1".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R1", "R3", "R4"]);

        let q = q_with(vec![Predicate::WhereLt {
            key: "rounds".into(),
            rhs: "2".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R2", "R5", "R6"]);
    }

    #[test]
    fn where_gte_lte_boundary_behaviour() {
        let doc = fixture();
        let q = q_with(vec![Predicate::WhereGte {
            key: "rounds".into(),
            rhs: "3".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R1", "R4"]);

        let q = q_with(vec![Predicate::WhereLte {
            key: "rounds".into(),
            rhs: "1".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R2", "R5", "R6"]);
    }

    #[test]
    fn where_contains_prefix_suffix_regex() {
        let doc = fixture();

        let q = q_with(vec![Predicate::WhereContains {
            key: "summary".into(),
            sub: "bypass".into(),
        }]);
        assert_eq!(ids(&run(&doc, "items", &q).unwrap()), vec!["R1"]);

        let q = q_with(vec![Predicate::WherePrefix {
            key: "file".into(),
            prefix: "src/c".into(),
        }]);
        assert_eq!(ids(&run(&doc, "items", &q).unwrap()), vec!["R4", "R5"]);

        let q = q_with(vec![Predicate::WhereSuffix {
            key: "file".into(),
            suffix: ".rs".into(),
        }]);
        assert_eq!(run(&doc, "items", &q).unwrap().as_array().unwrap().len(), 6);

        let q = q_with(vec![Predicate::WhereRegex {
            key: "id".into(),
            pattern: r"^R[13]$".into(),
        }]);
        assert_eq!(ids(&run(&doc, "items", &q).unwrap()), vec!["R1", "R3"]);
    }

    #[test]
    fn two_where_filters_and_combine() {
        let doc = fixture();
        let q = q_with(vec![
            Predicate::Where {
                key: "status".into(),
                rhs: "open".into(),
            },
            Predicate::Where {
                key: "category".into(),
                rhs: "quality".into(),
            },
        ]);
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R2", "R6"]);
    }

    // -- sort / limit / offset ----------------------------------------

    #[test]
    fn sort_asc_then_desc_on_string_field() {
        let doc = fixture();
        let q = Query {
            sort_by: vec![("id".into(), SortDir::Desc)],
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R6", "R5", "R4", "R3", "R2", "R1"]);

        let q = Query {
            sort_by: vec![("id".into(), SortDir::Asc)],
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R1", "R2", "R3", "R4", "R5", "R6"]);
    }

    #[test]
    fn sort_multi_key_primary_then_tiebreaker() {
        let doc = fixture();
        // Primary: status asc (fixed, open, open, open, open, wontfix)
        // Tiebreaker within status=open: rounds desc → (R1 rounds=3, R2/R5/R6 rounds=1).
        let q = Query {
            sort_by: vec![
                ("status".into(), SortDir::Asc),
                ("rounds".into(), SortDir::Desc),
            ],
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        let got = ids(&out);
        assert_eq!(got[0], "R3"); // fixed
        assert_eq!(got[1], "R1"); // open rounds=3
        assert_eq!(got[5], "R4"); // wontfix last
    }

    #[test]
    fn sort_by_date_field_ascending() {
        let doc = fixture();
        let q = Query {
            sort_by: vec![("first_flagged".into(), SortDir::Asc)],
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        // 2025-12-31 is R4; earliest.
        assert_eq!(ids(&out)[0], "R4");
        // 2026-04-18 is R6; latest.
        assert_eq!(ids(&out)[5], "R6");
    }

    #[test]
    fn limit_and_offset_window() {
        let doc = fixture();
        let q = Query {
            sort_by: vec![("id".into(), SortDir::Asc)],
            offset: Some(2),
            limit: Some(2),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(ids(&out), vec!["R3", "R4"]);

        // Offset past end → empty array.
        let q = Query {
            offset: Some(100),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert!(out.as_array().unwrap().is_empty());
    }

    // -- distinct / aggregate / pluck ---------------------------------

    #[test]
    fn distinct_on_projected_shape() {
        let doc = fixture();
        let q = Query {
            select: Some(vec!["category".into()]),
            distinct: true,
            sort_by: vec![("category".into(), SortDir::Asc)],
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        // Categories: security, quality, performance → 3 distinct.
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        let cats: Vec<String> = arr
            .iter()
            .map(|v| v["category"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(cats, vec!["performance", "quality", "security"]);
    }

    #[test]
    fn count_shape_returns_scalar_count() {
        let doc = fixture();
        let q = Query {
            predicates: vec![Predicate::Where {
                key: "status".into(),
                rhs: "open".into(),
            }],
            shape: OutputShape::Count,
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(out, serde_json::json!({"count": 4}));
    }

    #[test]
    fn count_by_buckets_values() {
        let doc = fixture();
        let q = Query {
            shape: OutputShape::CountBy("status".into()),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(out["open"], 4);
        assert_eq!(out["fixed"], 1);
        assert_eq!(out["wontfix"], 1);
    }

    #[test]
    fn group_by_buckets_items() {
        let doc = fixture();
        let q = Query {
            shape: OutputShape::GroupBy("file".into()),
            select: Some(vec!["id".into()]),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        // src/a.rs has R1, R2.
        let a_group = out["src/a.rs"].as_array().unwrap();
        let ids: Vec<_> = a_group
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["R1", "R2"]);
        // src/b.rs has R3 only.
        assert_eq!(out["src/b.rs"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn pluck_emits_scalar_array() {
        let doc = fixture();
        let q = Query {
            predicates: vec![Predicate::Where {
                key: "status".into(),
                rhs: "open".into(),
            }],
            shape: OutputShape::Pluck("id".into()),
            sort_by: vec![("id".into(), SortDir::Asc)],
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(
            out,
            serde_json::json!(["R1", "R2", "R5", "R6"])
        );
    }

    // -- validate_query rejections ------------------------------------

    #[test]
    fn select_plus_pluck_rejected() {
        let q = Query {
            select: Some(vec!["id".into()]),
            shape: OutputShape::Pluck("id".into()),
            ..Default::default()
        };
        let err = validate_query(&q).unwrap_err().to_string();
        assert!(
            err.contains("--select") && err.contains("--pluck"),
            "{err}"
        );
    }

    #[test]
    fn count_plus_select_rejected() {
        let q = Query {
            shape: OutputShape::Count,
            select: Some(vec!["id".into()]),
            ..Default::default()
        };
        let err = validate_query(&q).unwrap_err().to_string();
        assert!(
            err.contains("--count") && err.contains("--select"),
            "{err}"
        );
    }

    #[test]
    fn select_plus_exclude_rejected() {
        let q = Query {
            select: Some(vec!["id".into()]),
            exclude: Some(vec!["summary".into()]),
            ..Default::default()
        };
        let err = validate_query(&q).unwrap_err().to_string();
        assert!(
            err.contains("--select") && err.contains("--exclude"),
            "{err}"
        );
    }

    // -- typed RHS / compare_typed ------------------------------------

    #[test]
    fn parse_typed_value_covers_every_prefix() {
        use crate::convert::parse_typed_value;
        // Default: JSON string.
        assert_eq!(
            parse_typed_value("hello").unwrap(),
            JsonValue::String("hello".into())
        );
        // @string: explicit.
        assert_eq!(
            parse_typed_value("@string:hi").unwrap(),
            JsonValue::String("hi".into())
        );
        // @int:
        assert_eq!(
            parse_typed_value("@int:42").unwrap(),
            JsonValue::from(42_i64)
        );
        // @float:
        assert_eq!(
            parse_typed_value("@float:1.5").unwrap(),
            JsonValue::from(1.5_f64)
        );
        // @bool:
        assert_eq!(
            parse_typed_value("@bool:true").unwrap(),
            JsonValue::Bool(true)
        );
        assert_eq!(
            parse_typed_value("@bool:false").unwrap(),
            JsonValue::Bool(false)
        );
        // @date: → string at the JSON layer (TOML datetime compare happens
        // further up via `compare_typed`).
        assert_eq!(
            parse_typed_value("@date:2026-04-18").unwrap(),
            JsonValue::String("2026-04-18".into())
        );
    }

    #[test]
    fn compare_typed_against_native_datetime_and_integer() {
        use crate::convert::compare_typed;
        use std::cmp::Ordering::*;

        // Datetime field vs bare ISO date string.
        let dt: toml::value::Datetime = "2026-04-18".parse().unwrap();
        let field = TomlValue::Datetime(dt);
        assert_eq!(compare_typed(&field, "2026-04-18").unwrap(), Equal);
        assert_eq!(compare_typed(&field, "2026-01-01").unwrap(), Greater);
        assert_eq!(compare_typed(&field, "2026-05-01").unwrap(), Less);

        // Integer field.
        let f = TomlValue::Integer(5);
        assert_eq!(compare_typed(&f, "5").unwrap(), Equal);
        assert_eq!(compare_typed(&f, "1").unwrap(), Greater);
        assert_eq!(compare_typed(&f, "10").unwrap(), Less);

        // @int: prefix also works.
        assert_eq!(compare_typed(&f, "@int:5").unwrap(), Equal);
    }

    #[test]
    fn where_gte_date_compares_chronologically_not_lexically() {
        let doc = fixture();
        // Lexical "2026-04-01" > "2025-12-31" is also true, but pick a
        // boundary that would reorder if we compared naively on strings vs
        // dates: 2026-01-01 vs 2025-12-31.
        let q = q_with(vec![Predicate::WhereGte {
            key: "first_flagged".into(),
            rhs: "2026-01-01".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        // Everything except R4 (2025-12-31).
        let mut got = ids(&out);
        got.sort();
        assert_eq!(got, vec!["R1", "R2", "R3", "R5", "R6"]);
    }

    #[test]
    fn at_date_prefix_compare_returns_date_semantics() {
        let doc = fixture();
        let q = q_with(vec![Predicate::WhereGt {
            key: "first_flagged".into(),
            rhs: "@date:2026-01-01".into(),
        }]);
        let out = run(&doc, "items", &q).unwrap();
        // Strictly greater than 2026-01-01 excludes R1 (exactly that date) and R4.
        let mut got = ids(&out);
        got.sort();
        assert_eq!(got, vec!["R2", "R3", "R5", "R6"]);
    }
}
