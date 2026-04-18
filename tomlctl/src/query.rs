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

use anyhow::{Result, bail};
use regex::{Regex, RegexBuilder};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use toml::Value as TomlValue;

use crate::convert::{
    TypeHint, compare_typed, json_type_name, parse_typed_value, split_type_hint, toml_to_json,
};
use crate::errors::{ErrorKind, tagged_err};

/// Per-compile memory cap for user-supplied regex patterns (R72). Chosen to
/// bound a pathological pattern's NFA compile / DFA cache at ~1 MiB each,
/// well above anything a ledger field regex would realistically need.
const REGEX_COMPILE_SIZE_LIMIT: usize = 1 << 20;
const REGEX_DFA_SIZE_LIMIT: usize = 1 << 20;

/// Compile a user-supplied regex pattern with memory caps applied so an
/// adversarial pattern can't consume unbounded memory during compilation
/// or DFA construction. Factored out so both the per-predicate hoist and
/// tests can share one configuration (R72).
fn compile_user_regex(pattern: &str) -> Result<Regex> {
    RegexBuilder::new(pattern)
        .size_limit(REGEX_COMPILE_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
        .build()
        .map_err(|e| anyhow::anyhow!("invalid regex `{}`: {}", pattern, e))
}

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
/// aggregation/pluck flags are set. `ndjson` is an *encoding* choice handled
/// at the CLI layer (R82) and no longer appears here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum OutputShape {
    #[default]
    Array,
    Count,
    CountBy(String),
    /// Groups matched items by the value of a field.
    ///
    /// **Ordering invariant**: buckets are emitted in the insertion order of
    /// their first-seen key (i.e., the order of the first matching item for
    /// each group). Callers that need a sorted group-order must pre-sort the
    /// input via `--sort-by` before invoking `--group-by`. This invariant is
    /// load-bearing for `render-from-log` in the execution-record-schema
    /// shared block — do NOT switch to stable-sort-by-key without updating
    /// that routine's spec.
    ///
    /// The invariant is realised by `apply_aggregation_group_by` via
    /// `serde_json::Map` with the `preserve_order` feature (see Cargo.toml) —
    /// removing that feature would also break this invariant.
    GroupBy(String),
    Pluck(String),
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
    /// Output encoding: if `true` the CLI emits one compact JSON value per
    /// line instead of a single pretty-printed JSON array. Only meaningful
    /// when `shape == Array`; `run()` ignores it (it always returns a
    /// `JsonValue::Array`).
    pub ndjson: bool,
}

/// Reject mutually exclusive flag combinations. The CLI's `validate_query`
/// call keeps clap-level help-text lean (no need for clap's
/// `conflicts_with` on every pair) and centralises the rules.
pub(crate) fn validate_query(q: &Query) -> Result<()> {
    // T8: every mutex violation in this function is a CLI-surface validation
    // failure — tag the whole `bail!` set with `kind=validation` so
    // `--error-format json` surfaces the same kind regardless of which
    // specific pair collided. The `validation_bail!` macro keeps the prose
    // byte-identical to the pre-T8 `bail!(...)` form — `tagged_err` builds an
    // anyhow::Error with `TaggedError` as the innermost layer whose `Display`
    // is the formatted message, so text-mode `{:#}` rendering is unchanged.
    macro_rules! validation_bail {
        ($($arg:tt)*) => {
            return Err(tagged_err(ErrorKind::Validation, None, format!($($arg)*)))
        };
    }
    if q.select.is_some() && q.exclude.is_some() {
        validation_bail!("--select and --exclude are mutually exclusive");
    }
    match &q.shape {
        OutputShape::Pluck(_) => {
            if q.select.is_some() {
                validation_bail!("--select and --pluck are mutually exclusive");
            }
            if q.exclude.is_some() {
                validation_bail!("--exclude and --pluck are mutually exclusive");
            }
        }
        OutputShape::Count => {
            if q.select.is_some() {
                validation_bail!("--count and --select are mutually exclusive");
            }
            if q.exclude.is_some() {
                validation_bail!("--count and --exclude are mutually exclusive");
            }
        }
        OutputShape::CountBy(_) => {
            if q.select.is_some() {
                validation_bail!("--count-by and --select are mutually exclusive");
            }
            if q.exclude.is_some() {
                validation_bail!("--count-by and --exclude are mutually exclusive");
            }
        }
        OutputShape::GroupBy(_) => {
            // group-by composes fine with projection; no cross-exclusion here.
        }
        OutputShape::Array => {}
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
    let items: &[TomlValue] = match doc.get(array_name).and_then(|v| v.as_array()) {
        Some(arr) => arr.as_slice(),
        None => &[],
    };

    // 1. Filter
    // O47: pass the array slice directly instead of materialising a
    // Vec<&TomlValue> first. `apply_filters` borrows through the slice.
    let filtered = apply_filters(items, &q.predicates)?;

    // O21/O55: Count / Pluck / CountBy fast-paths. When the user wants one
    // of these shapes with no sort/distinct/window mutations downstream,
    // the per-item `toml_to_json` materialisation of every field is pure
    // waste — Count only needs `filtered.len()`; Pluck and CountBy only
    // need ONE field per item. The guard checks every state that could
    // change the post-pipeline composition: sort doesn't (stable
    // permutation) but distinct, offset, and limit all do. validate_query
    // has already rejected --count/--pluck/--count-by + --select/--exclude,
    // so projection can't interact with the output either. GroupBy is not
    // fast-pathed — its grouped item bodies still need full materialisation.
    let window_untouched = q.sort_by.is_empty()
        && !q.distinct
        && q.offset.is_none()
        && q.limit.is_none();
    if window_untouched {
        match &q.shape {
            OutputShape::Count => {
                return Ok(serde_json::json!({ "count": filtered.len() }));
            }
            OutputShape::Pluck(field) => {
                let mut out = Vec::with_capacity(filtered.len());
                for t in &filtered {
                    match t.get(field).map(toml_to_json) {
                        None | Some(JsonValue::Null) => {}
                        Some(v) => out.push(v),
                    }
                }
                return Ok(JsonValue::Array(out));
            }
            OutputShape::CountBy(field) => {
                let mut counts: serde_json::Map<String, JsonValue> = serde_json::Map::new();
                for t in &filtered {
                    let key = match t.get(field).map(toml_to_json) {
                        None | Some(JsonValue::Null) => String::new(),
                        Some(JsonValue::String(s)) => s,
                        Some(other) => other.to_string(),
                    };
                    match counts.get_mut(&key) {
                        Some(JsonValue::Number(n)) => {
                            let new = n.as_u64().unwrap_or(0) + 1;
                            *n = serde_json::Number::from(new);
                        }
                        _ => {
                            counts.insert(key, JsonValue::from(1_u64));
                        }
                    }
                }
                return Ok(JsonValue::Object(counts));
            }
            _ => {}
        }
    }

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
        // shape so grouping keys stay intact. For Array, dedup on projected
        // shape so "select a,b --distinct" dedupes by (a,b). For Pluck, the
        // user expectation is that `--pluck f --distinct` dedupes by the
        // plucked field f — `apply_projection` only honours --select/--exclude
        // and does not narrow to the pluck field, so we build the dedup key
        // from just that field here (R9).
        match &q.shape {
            OutputShape::Array => {
                let projected: Vec<JsonValue> =
                    sorted.iter().map(|v| apply_projection(v, q)).collect();
                dedup_preserve_first(&sorted, &projected)
            }
            OutputShape::Pluck(field) => {
                // Narrow the dedup key to the plucked field. Items whose
                // plucked field is missing/null are dropped downstream by
                // `apply_pluck`; we keep them here so first-occurrence order
                // stays aligned with the pre-dedup sequence, and use
                // `JsonValue::Null` as the sentinel key (identical missing
                // fields dedupe to one, matching scalar-array expectations).
                let projected: Vec<JsonValue> = sorted
                    .iter()
                    .map(|v| v.get(field).cloned().unwrap_or(JsonValue::Null))
                    .collect();
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
        OutputShape::Array => {
            let projected: Vec<JsonValue> =
                windowed.iter().map(|v| apply_projection(v, q)).collect();
            Ok(JsonValue::Array(projected))
        }
    }
}

/// O34: streaming NDJSON sibling of `run()`. For `OutputShape::Array` with
/// `q.ndjson == true`, emits one compact JSON object per line directly to
/// `writer`, avoiding the `Vec<JsonValue>` that `run()` would otherwise
/// materialise only to have the CLI iterate and re-serialise it. For every
/// other shape/encoding combination, this delegates to `run()` and
/// serialises the single resulting value to `writer` in one shot — the
/// caller (cli.rs NDJSON branch) is only invoked on the Array+ndjson path
/// in practice, but the fallback keeps the contract total and testable.
pub(crate) fn run_streaming<W: Write>(
    doc: &TomlValue,
    array_name: &str,
    q: &Query,
    writer: &mut W,
) -> Result<()> {
    // Non-Array shapes (and Array without ndjson encoding) don't benefit
    // from streaming — the final value is a single object/scalar. Delegate
    // to `run()` and serialise once.
    if !q.ndjson || !matches!(q.shape, OutputShape::Array) {
        let out = run(doc, array_name, q)?;
        serde_json::to_writer(writer, &out)?;
        return Ok(());
    }

    // Array + ndjson streaming path. Mirrors the Array arm of `run()`:
    // filter → (project/sort/distinct/window) → emit each element with a
    // trailing newline as it's produced. We still need the full in-memory
    // pipeline up to the final emit because sort/distinct are inherently
    // non-streaming; the win is in avoiding the terminal `Vec<JsonValue>`
    // that `run()` returns when the caller wants line-per-item output.
    validate_query(q)?;
    let items: &[TomlValue] = match doc.get(array_name).and_then(|v| v.as_array()) {
        Some(arr) => arr.as_slice(),
        None => &[],
    };
    let filtered = apply_filters(items, &q.predicates)?;

    // Fast-path: no sort/distinct/window, pure Array shape. Stream directly
    // from the filtered items through projection — one JsonValue per item,
    // emitted and dropped before the next is built. This is the single
    // biggest reduction in peak memory vs `run()`.
    let window_untouched = q.sort_by.is_empty()
        && !q.distinct
        && q.offset.is_none()
        && q.limit.is_none();
    if window_untouched {
        for t in &filtered {
            let v = toml_to_json(t);
            let projected = apply_projection(&v, q);
            serde_json::to_writer(&mut *writer, &projected)?;
            writer.write_all(b"\n")?;
        }
        return Ok(());
    }

    // Slow path (sort/distinct/window touched): mirror the full pipeline
    // from `run()` but emit per-item at the tail rather than collecting.
    let for_shape: Vec<JsonValue> = filtered.iter().map(|t| toml_to_json(t)).collect();
    let sorted = apply_sort(for_shape, &q.sort_by);
    let deduped = if q.distinct {
        let projected: Vec<JsonValue> =
            sorted.iter().map(|v| apply_projection(v, q)).collect();
        dedup_preserve_first(&sorted, &projected)
    } else {
        sorted
    };
    let windowed = apply_window(deduped, q.offset, q.limit);
    for v in &windowed {
        let projected = apply_projection(v, q);
        serde_json::to_writer(&mut *writer, &projected)?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

// -----------------------------------------------------------------------
// Filtering
// -----------------------------------------------------------------------

pub(crate) fn apply_filters<'a>(
    items: &'a [TomlValue],
    preds: &[Predicate],
) -> Result<Vec<&'a TomlValue>> {
    // R72: compile every `WhereRegex` pattern once, up-front, before we
    // touch the item loop. A compiled regex is indexed by its position in
    // `preds` so `eval_predicate` can do an O(1) lookup instead of
    // recompiling per (item × predicate). We also apply memory caps via
    // `RegexBuilder::size_limit` / `dfa_size_limit` so a hostile pattern
    // can't balloon compile-time memory.
    let compiled: Vec<Option<Regex>> = preds
        .iter()
        .map(|p| match p {
            Predicate::WhereRegex { pattern, .. } => compile_user_regex(pattern).map(Some),
            _ => Ok(None),
        })
        .collect::<Result<Vec<_>>>()?;

    // O19/O20: pre-parse every type-coercive RHS once, up-front, parallel to
    // the regex hoist above. A typed-prefix RHS (`@int:5`, `@date:2026-04-18`)
    // is otherwise parsed via `parse_typed_value` once per (item × predicate)
    // inside `eq_typed` / `compare_typed`. With the cache, the parse runs
    // O(P) instead of O(N × P). Bare-RHS predicates fall through to the
    // existing per-item native-type coercion path (encoded as
    // `ParsedRhs::Untyped`); the cache buys nothing there but costs nothing
    // either. WhereIn pre-parses every list element.
    let rhs_cache: Vec<PredicateCache> = preds
        .iter()
        .map(|p| -> Result<PredicateCache> {
            match p {
                Predicate::Where { key, rhs }
                | Predicate::WhereNot { key, rhs }
                | Predicate::WhereGt { key, rhs }
                | Predicate::WhereGte { key, rhs }
                | Predicate::WhereLt { key, rhs }
                | Predicate::WhereLte { key, rhs } => {
                    Ok(PredicateCache::Single(parse_rhs_for_cache(rhs, key)?))
                }
                Predicate::WhereIn { key, rhs } => {
                    let mut v = Vec::with_capacity(rhs.len());
                    for r in rhs {
                        v.push(parse_rhs_for_cache(r, key)?);
                    }
                    Ok(PredicateCache::Multi(v))
                }
                _ => Ok(PredicateCache::None),
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let mut out = Vec::with_capacity(items.len());
    'item: for it in items {
        for (i, p) in preds.iter().enumerate() {
            if !eval_predicate(it, p, compiled[i].as_ref(), &rhs_cache[i])? {
                continue 'item;
            }
        }
        out.push(it);
    }
    Ok(out)
}

/// O19/O20: per-RHS pre-parse. One enum per recognised TypeHint variant plus
/// `Untyped` for the bare-string fallback path. Field-side native-coercion
/// (Integer field + bare RHS → parse as i64) intentionally remains per-item;
/// the cache only short-circuits the typed-prefix path because that's where
/// every RHS parse is identical across items and the only thing varying is
/// the field. See `eq_typed` / `cmp_pred` for the dispatch.
#[derive(Clone, Debug)]
enum ParsedRhs {
    Int(i64),
    Float(f64),
    Bool(bool),
    Datetime(toml::value::Datetime),
    /// Body for `@string:` / `@str:` prefix. Stored separately from the raw
    /// RHS so the eq path can borrow it without re-stripping the prefix.
    Str(String),
    /// No `@type:` prefix; per-item native-coercion path runs as before.
    Untyped,
}

/// O19/O20: per-predicate cache aligned with `preds`. Single-RHS predicates
/// store one entry; WhereIn stores a Vec aligned with its RHS list.
#[derive(Clone, Debug)]
enum PredicateCache {
    Single(ParsedRhs),
    Multi(Vec<ParsedRhs>),
    /// Predicate has no type-coercive RHS (Has/Missing/Contains/Prefix/Suffix/Regex).
    None,
}

/// O19/O20: parse one RHS string into a `ParsedRhs` for the cache. `key` is
/// only used to make `@type:` parse failures actionable (mirrors the error
/// shape `eq_typed` raised before the cache was introduced — R73).
fn parse_rhs_for_cache(rhs: &str, key: &str) -> Result<ParsedRhs> {
    let Some((hint, body)) = split_type_hint(rhs) else {
        return Ok(ParsedRhs::Untyped);
    };
    match hint {
        TypeHint::Int => {
            let n: i64 = body.parse().map_err(|e| {
                anyhow::anyhow!(
                    "invalid typed RHS `{}` for --where predicate on key `{}`: {}",
                    rhs,
                    key,
                    e
                )
            })?;
            Ok(ParsedRhs::Int(n))
        }
        TypeHint::Float => {
            let f: f64 = body.parse().map_err(|e| {
                anyhow::anyhow!(
                    "invalid typed RHS `{}` for --where predicate on key `{}`: {}",
                    rhs,
                    key,
                    e
                )
            })?;
            Ok(ParsedRhs::Float(f))
        }
        TypeHint::Bool => {
            let b: bool = body.parse().map_err(|e| {
                anyhow::anyhow!(
                    "invalid typed RHS `{}` for --where predicate on key `{}`: {}",
                    rhs,
                    key,
                    e
                )
            })?;
            Ok(ParsedRhs::Bool(b))
        }
        TypeHint::Date | TypeHint::DateTime => {
            let dt: toml::value::Datetime = body.parse().map_err(|e| {
                anyhow::anyhow!(
                    "invalid typed RHS `{}` for --where predicate on key `{}`: {}",
                    rhs,
                    key,
                    e
                )
            })?;
            Ok(ParsedRhs::Datetime(dt))
        }
        TypeHint::Str => Ok(ParsedRhs::Str(body.to_string())),
    }
}

fn eval_predicate(
    item: &TomlValue,
    p: &Predicate,
    compiled_regex: Option<&Regex>,
    cache: &PredicateCache,
) -> Result<bool> {
    let tbl = match item.as_table() {
        Some(t) => t,
        None => return Ok(false),
    };
    // O19/O20: pull the pre-parsed RHS out of the cache; helpers below take
    // it instead of re-parsing per item. `expect_single` / `expect_multi`
    // panic on cache/predicate mismatch — that's an internal invariant
    // violation in `apply_filters`, not user input.
    match p {
        Predicate::Where { key, rhs } => eq_typed(tbl.get(key), rhs, key, expect_single(cache)),
        Predicate::WhereNot { key, rhs } => {
            Ok(!eq_typed(tbl.get(key), rhs, key, expect_single(cache))?)
        }
        Predicate::WhereIn { key, rhs } => {
            let field = tbl.get(key);
            let parsed = expect_multi(cache);
            // Any-match with error propagation: bail on the first malformed
            // RHS rather than silently skipping it. (RHS validity was already
            // checked at cache-build time; the per-item call here can only
            // surface fresh errors from the field side.)
            for (raw, pre) in rhs.iter().zip(parsed.iter()) {
                if eq_typed(field, raw, key, pre)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Predicate::WhereHas { key } => Ok(field_present_nonempty(tbl.get(key))),
        Predicate::WhereMissing { key } => Ok(!field_present_nonempty(tbl.get(key))),
        Predicate::WhereGt { key, rhs } => {
            cmp_pred(tbl.get(key), rhs, key, expect_single(cache), |o| {
                matches!(o, std::cmp::Ordering::Greater)
            })
        }
        Predicate::WhereGte { key, rhs } => {
            cmp_pred(tbl.get(key), rhs, key, expect_single(cache), |o| {
                matches!(o, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            })
        }
        Predicate::WhereLt { key, rhs } => {
            cmp_pred(tbl.get(key), rhs, key, expect_single(cache), |o| {
                matches!(o, std::cmp::Ordering::Less)
            })
        }
        Predicate::WhereLte { key, rhs } => {
            cmp_pred(tbl.get(key), rhs, key, expect_single(cache), |o| {
                matches!(o, std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            })
        }
        // O37: stringify non-string scalars (Int/Float/Bool/Datetime) before
        // running substring/prefix/suffix — previously a `--where-contains`
        // against e.g. an Integer field silently returned false for every
        // row because `TomlValue::as_str` only matches TOML Strings. Behaviour
        // change: `--where-contains line=00` against `line=100` now matches
        // where it silently missed before (release-note worthy).
        Predicate::WhereContains { key, sub } => Ok(value_as_string(tbl.get(key))
            .is_some_and(|s| s.contains(sub))),
        Predicate::WherePrefix { key, prefix } => Ok(value_as_string(tbl.get(key))
            .is_some_and(|s| s.starts_with(prefix))),
        Predicate::WhereSuffix { key, suffix } => Ok(value_as_string(tbl.get(key))
            .is_some_and(|s| s.ends_with(suffix))),
        Predicate::WhereRegex { key, .. } => {
            // R72: the regex was compiled once in `apply_filters`; we just
            // look it up here.
            let re = compiled_regex.ok_or_else(|| {
                anyhow::anyhow!(
                    "internal: missing compiled regex for --where-regex on key `{}`",
                    key
                )
            })?;
            let s = value_as_string(tbl.get(key));
            Ok(s.as_deref().is_some_and(|s| re.is_match(s)))
        }
    }
}

fn expect_single(cache: &PredicateCache) -> &ParsedRhs {
    match cache {
        PredicateCache::Single(p) => p,
        _ => panic!("internal: predicate cache expected Single variant"),
    }
}

fn expect_multi(cache: &PredicateCache) -> &[ParsedRhs] {
    match cache {
        PredicateCache::Multi(v) => v.as_slice(),
        _ => panic!("internal: predicate cache expected Multi variant"),
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
///
/// `key` is only used to build a user-facing error when the RHS carries a
/// `@type:` prefix but fails to parse — previously this was swallowed as
/// `return false`, which silently dropped the query row (R73).
///
/// O19/O20: `pre` carries the cached pre-parsed RHS built once in
/// `apply_filters`. When it's a typed variant, dispatch goes through the
/// cached value and skips the per-item `parse_typed_value` round-trip
/// entirely. `Untyped` falls through to the existing native-coercion path.
fn eq_typed(field: Option<&TomlValue>, rhs: &str, key: &str, pre: &ParsedRhs) -> Result<bool> {
    let Some(field) = field else { return Ok(false) };
    // 1. Cached typed-prefix dispatch — no per-item parse.
    match pre {
        ParsedRhs::Int(n) => {
            return Ok(matches!(field, TomlValue::Integer(i) if i == n));
        }
        ParsedRhs::Float(f) => {
            return Ok(matches!(field, TomlValue::Float(g) if g == f));
        }
        ParsedRhs::Bool(b) => {
            return Ok(matches!(field, TomlValue::Boolean(c) if c == b));
        }
        ParsedRhs::Datetime(dt) => {
            return Ok(
                matches!(field, TomlValue::Datetime(field_dt) if field_dt.to_string() == dt.to_string()),
            );
        }
        ParsedRhs::Str(body) => {
            // `@string:` / `@str:` — match TOML String exactly, otherwise
            // stringify the field for cross-type compare (matches the prior
            // `json_matches_toml` Str fallback).
            return Ok(match field {
                TomlValue::String(f) => f == body,
                other => stringify_scalar(other) == *body,
            });
        }
        ParsedRhs::Untyped => {}
    }
    // Defensive: if a typed-prefix RHS slipped past cache-build (shouldn't
    // happen because `parse_rhs_for_cache` validates), still surface a parse
    // error in the same shape as before so the R73 contract holds.
    if let Some((hint, _rest)) = split_type_hint(rhs) {
        let parsed = parse_typed_value(rhs).map_err(|e| {
            anyhow::anyhow!(
                "invalid typed RHS `{}` for --where predicate on key `{}`: {}",
                rhs,
                key,
                e
            )
        })?;
        return Ok(json_matches_toml(&parsed, field, hint));
    }
    // 2. No prefix — native-type coercion from the field side.
    Ok(match field {
        TomlValue::String(s) => s == rhs,
        TomlValue::Integer(i) => rhs.parse::<i64>().map(|r| r == *i).unwrap_or(false),
        TomlValue::Float(f) => rhs.parse::<f64>().map(|r| r == *f).unwrap_or(false),
        TomlValue::Boolean(b) => rhs.parse::<bool>().map(|r| r == *b).unwrap_or(false),
        TomlValue::Datetime(dt) => dt.to_string() == rhs,
        _ => false,
    })
}

/// Compare a typed JSON scalar (from `parse_typed_value`) against a TOML field.
fn json_matches_toml(parsed: &JsonValue, field: &TomlValue, hint: TypeHint) -> bool {
    match (parsed, field, hint) {
        (JsonValue::String(s), TomlValue::String(f), TypeHint::Str) => s == f,
        (JsonValue::String(s), TomlValue::Datetime(dt), TypeHint::Date | TypeHint::DateTime) => {
            // Compare ISO-string form. TOML Datetime Display gives ISO-8601.
            dt.to_string() == *s
        }
        (JsonValue::Number(n), TomlValue::Integer(i), TypeHint::Int) => {
            n.as_i64().map(|v| v == *i).unwrap_or(false)
        }
        (JsonValue::Number(n), TomlValue::Float(f), TypeHint::Float) => {
            n.as_f64().map(|v| v == *f).unwrap_or(false)
        }
        (JsonValue::Bool(b), TomlValue::Boolean(f), TypeHint::Bool) => b == f,
        // Cross-type compare: string RHS against non-string field (e.g.
        // `@string:42` against an Integer). Compare via stringified field.
        (JsonValue::String(s), other, TypeHint::Str) => stringify_scalar(other) == *s,
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
    key: &str,
    pre: &ParsedRhs,
    check: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<bool> {
    use std::cmp::Ordering;
    let Some(f) = field else { return Ok(false) };
    // O19/O20: cached typed-prefix dispatch. When the cache holds a parsed
    // numeric / bool / datetime, compare directly against the field's native
    // representation — no per-item `compare_typed` round-trip through the
    // RHS parser. A type-hint vs field-type mismatch surfaces as the same
    // shape of error that `compare_typed` produced before the cache.
    let ord_opt: Option<Result<Ordering>> = match pre {
        ParsedRhs::Int(n) => match f {
            TomlValue::Integer(i) => Some(Ok(i.cmp(n))),
            _ => Some(Err(anyhow::anyhow!(
                "invalid typed RHS `{}` for --where predicate on key `{}`: type hint `int` doesn't match field type",
                rhs,
                key
            ))),
        },
        ParsedRhs::Float(x) => match f {
            TomlValue::Float(g) => Some(Ok(g.partial_cmp(x).unwrap_or(Ordering::Equal))),
            _ => Some(Err(anyhow::anyhow!(
                "invalid typed RHS `{}` for --where predicate on key `{}`: type hint `float` doesn't match field type",
                rhs,
                key
            ))),
        },
        ParsedRhs::Bool(b) => match f {
            TomlValue::Boolean(c) => Some(Ok(c.cmp(b))),
            _ => Some(Err(anyhow::anyhow!(
                "invalid typed RHS `{}` for --where predicate on key `{}`: bool RHS not comparable against non-bool field",
                rhs,
                key
            ))),
        },
        ParsedRhs::Datetime(dt) => match f {
            TomlValue::Datetime(field_dt) => {
                Some(Ok(field_dt.to_string().cmp(&dt.to_string())))
            }
            _ => Some(Err(anyhow::anyhow!(
                "invalid typed RHS `{}` for --where predicate on key `{}`: datetime RHS not comparable against non-datetime field",
                rhs,
                key
            ))),
        },
        ParsedRhs::Str(body) => match f {
            TomlValue::String(s) => Some(Ok(s.as_str().cmp(body.as_str()))),
            _ => Some(Err(anyhow::anyhow!(
                "invalid typed RHS `{}` for --where predicate on key `{}`: string RHS not comparable against non-string field",
                rhs,
                key
            ))),
        },
        ParsedRhs::Untyped => None,
    };
    if let Some(res) = ord_opt {
        return Ok(check(res?));
    }
    // R73 / Untyped: surface parse errors up to the caller rather than
    // silently treating the item as non-matching. A malformed RHS is a user
    // bug worth failing loudly on.
    let ord = compare_typed(f, rhs).map_err(|e| {
        anyhow::anyhow!(
            "invalid typed RHS `{}` for --where predicate on key `{}`: {}",
            rhs,
            key,
            e
        )
    })?;
    Ok(check(ord))
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
        // O25: avoid the clone-then-remove allocation churn (full Map clone
        // followed by per-key removal). Build the kept-keys map directly with
        // a HashSet membership probe — same allocation shape as the select
        // branch above, and preserves insertion order via the indexmap-backed
        // Map (preserve_order feature on serde_json).
        let drop_set: HashSet<&str> = drop.iter().map(String::as_str).collect();
        let mut out = serde_json::Map::with_capacity(obj.len().saturating_sub(drop.len()));
        for (k, v) in obj {
            if !drop_set.contains(k.as_str()) {
                out.insert(k.clone(), v.clone());
            }
        }
        return JsonValue::Object(out);
    }
    item.clone()
}

// -----------------------------------------------------------------------
// Shaping — sort, limit/offset, distinct
// -----------------------------------------------------------------------

fn apply_sort(mut items: Vec<JsonValue>, sort_by: &[(String, SortDir)]) -> Vec<JsonValue> {
    if sort_by.is_empty() {
        return items;
    }
    // O23: single O(N log N) pass using a composed comparator instead of K
    // separate stable sorts (one per key). Walks the sort_by spec in
    // primary-first order and folds per-key Orderings via `Ordering::then_with`,
    // so a tie on the primary key falls through to the next-most-significant
    // tiebreaker. Behaviourally equivalent to the previous LSB-first repeated
    // sort, but does the work in one pass.
    items.sort_by(|a, b| {
        sort_by
            .iter()
            .fold(std::cmp::Ordering::Equal, |acc, (key, dir)| {
                acc.then_with(|| {
                    let ord = cmp_json_scalars(a.get(key), b.get(key));
                    match dir {
                        SortDir::Asc => ord,
                        SortDir::Desc => ord.reverse(),
                    }
                })
            })
    });
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
                // O24: mixed-type fallback. Compare type-name discriminants
                // (cheap &'static str lex order from `convert::json_type_name`)
                // instead of materialising both values via `to_string()` —
                // saves two allocations per cross-type comparison and still
                // yields a deterministic stable order across runs.
                json_type_name(x).cmp(json_type_name(y))
            }
        },
    }
}

fn dedup_preserve_first(source: &[JsonValue], shape: &[JsonValue]) -> Vec<JsonValue> {
    // R67: `HashSet::insert` returns `true` on first sight of a key, which
    // is exactly the keep-decision we want. O(n) amortised instead of the
    // previous O(n²) Vec-scan.
    //
    // O22: structurally hash each shape value into a u64 instead of fully
    // serialising to a JSON string. `serde_json::to_string` allocates a
    // fresh String per item plus all the formatting machinery; the
    // structural-hash walk just feeds bytes into a DefaultHasher and stores
    // the resulting 64-bit digest. `serde_json::Value` deliberately does not
    // implement `Hash` (Number's float interior + Map's key ordering
    // semantics make a derived impl unsafe), so we walk by hand.
    let mut seen: HashSet<u64> = HashSet::with_capacity(shape.len());
    let mut out = Vec::with_capacity(shape.len());
    for (i, s) in shape.iter().enumerate() {
        let key = json_structural_hash(s);
        if seen.insert(key) {
            out.push(source[i].clone());
        }
    }
    out
}

/// O22: structurally hash a `serde_json::Value` into a `u64` digest. Walks
/// the value recursively, feeding a per-variant tag byte and the contained
/// scalar (or recursively-hashed children) into a `DefaultHasher`. Hash
/// collisions across distinct structural values would only cause dedup to
/// drop a non-equal item; in practice DefaultHasher's 64-bit output makes
/// that vanishingly unlikely on the corpora `tomlctl` operates on (ledger
/// rows in the low thousands).
///
/// Floats hash via `to_bits()` so that `NaN != NaN` doesn't get coerced into
/// equality, matching the existing JSON-string serialisation behaviour where
/// `NaN` simply isn't representable. Object keys are walked in iteration
/// order — with `serde_json/preserve_order` enabled (Cargo.toml) this is
/// insertion order, mirroring how the old `to_string` key was constructed.
fn json_structural_hash(v: &JsonValue) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn walk(v: &JsonValue, h: &mut DefaultHasher) {
        match v {
            JsonValue::Null => {
                0u8.hash(h);
            }
            JsonValue::Bool(b) => {
                1u8.hash(h);
                b.hash(h);
            }
            JsonValue::Number(n) => {
                2u8.hash(h);
                if let Some(i) = n.as_i64() {
                    b'i'.hash(h);
                    i.hash(h);
                } else if let Some(u) = n.as_u64() {
                    b'u'.hash(h);
                    u.hash(h);
                } else if let Some(f) = n.as_f64() {
                    b'f'.hash(h);
                    f.to_bits().hash(h);
                } else {
                    // Numbers that are neither i64/u64/f64 representable
                    // are extremely rare; fall back to the textual form.
                    b's'.hash(h);
                    n.to_string().hash(h);
                }
            }
            JsonValue::String(s) => {
                3u8.hash(h);
                s.hash(h);
            }
            JsonValue::Array(a) => {
                4u8.hash(h);
                (a.len() as u64).hash(h);
                for it in a {
                    walk(it, h);
                }
            }
            JsonValue::Object(m) => {
                5u8.hash(h);
                (m.len() as u64).hash(h);
                for (k, v) in m {
                    k.hash(h);
                    walk(v, h);
                }
            }
        }
    }

    let mut h = DefaultHasher::new();
    walk(v, &mut h);
    h.finish()
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
    // O49: fuse skip+take into a single iterator chain so we only allocate
    // the output Vec once instead of materialising an intermediate `tail`.
    items
        .into_iter()
        .skip(off)
        .take(limit.unwrap_or(usize::MAX))
        .collect()
}

// -----------------------------------------------------------------------
// Aggregation + pluck
// -----------------------------------------------------------------------

pub(crate) fn apply_aggregation_count(items: &[JsonValue]) -> JsonValue {
    serde_json::json!({ "count": items.len() })
}

pub(crate) fn apply_aggregation_count_by(items: &[JsonValue], field: &str) -> JsonValue {
    // R68 + O53: `HashMap<String, u64>` accumulator combined with a
    // parallel `Vec<String>` first-occurrence tracker. The prior
    // `serde_json::Map::get_mut` path still allocated a fresh `String`
    // key per item even on a hit (the `bucket_key` call materialises the
    // owned key before the hash lookup, then drops it). Using the `entry`
    // API on a plain HashMap isn't enough on its own — entry still takes
    // the key by value — so we peek with `get_mut` first and only clone
    // the key on a genuine insert. The `Vec::contains` for order tracking
    // is O(M) where M is distinct buckets (typically <20 for ledger fields
    // like status/category/tier); the net walk is still a large win vs
    // N×`String` alloc-then-drop.
    let mut counts: HashMap<String, u64> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for it in items {
        let key = bucket_key(it.get(field));
        // Track insertion order on the miss path so the rebuild below can
        // reproduce the indexmap-backed Map's insertion-order contract.
        // `order.contains(&key)` is O(M) where M is distinct buckets —
        // typically <20 for ledger fields like status/category/tier — so
        // the linear scan is cheaper than a second HashSet.
        if !order.contains(&key) {
            order.push(key.clone());
        }
        *counts.entry(key).or_insert(0) += 1;
    }
    // Rebuild the serde_json::Map in first-occurrence order — this is the
    // public contract preserved by the earlier indexmap-backed Map impl.
    let mut out: serde_json::Map<String, JsonValue> = serde_json::Map::with_capacity(order.len());
    for k in order {
        let n = counts.get(&k).copied().unwrap_or(0);
        out.insert(k, JsonValue::from(n));
    }
    JsonValue::Object(out)
}

pub(crate) fn apply_aggregation_group_by(
    raw: &[JsonValue],
    projected: &[JsonValue],
    field: &str,
) -> JsonValue {
    // R68: same Map-backed accumulator as `apply_aggregation_count_by`, but
    // the slot value is a Vec of grouped items.
    let mut groups: serde_json::Map<String, JsonValue> = serde_json::Map::new();
    for (i, it) in raw.iter().enumerate() {
        let key = bucket_key(it.get(field));
        let proj = projected[i].clone();
        match groups.get_mut(&key) {
            Some(JsonValue::Array(arr)) => arr.push(proj),
            _ => {
                groups.insert(key, JsonValue::Array(vec![proj]));
            }
        }
    }
    JsonValue::Object(groups)
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

/// Split a `KEY=VAL` string on the first `=`. Empty keys are rejected. The
/// value is returned verbatim (no trimming) so callers that care about
/// whitespace-significant RHS values (e.g. `--where-prefix name= foo`) keep
/// their payload intact. Used by `Query::from_cli_args` for every
/// `--where-*` family.
fn split_kv(s: &str) -> Result<(String, String)> {
    let Some((k, v)) = s.split_once('=') else {
        bail!("expected KEY=VAL, got `{}`", s);
    };
    if k.is_empty() {
        bail!("KEY=VAL has empty key in `{}`", s);
    }
    Ok((k.to_string(), v.to_string()))
}

impl Query {
    /// Build a `Query` from the clap flag values on `ItemsOp::List`.
    /// Validation is handled by `run` itself — the first thing it does is
    /// call `validate_query` on the spec, so callers don't need to (R88).
    /// R69: signature takes two references (a `LegacyShortcuts` for the
    /// back-compat shortcut flags + the full `QueryArgs` bundle) so the
    /// dispatch site is a one-line call rather than a 26-line arg spray.
    ///
    /// R30: moved here from `cli.rs` because the translation from clap args
    /// into domain-level `Predicate` / `OutputShape` values is business
    /// logic tightly coupled to `query`'s types, not pure CLI plumbing.
    pub(crate) fn from_cli_args(
        legacy: &crate::cli::LegacyShortcuts<'_>,
        q: &crate::cli::QueryArgs,
    ) -> Result<Self> {
        // O46: pre-size the predicate vec. The `4` covers the four legacy
        // shortcut slots (`status`, `category`, `file`, `newer_than`); the
        // remaining terms sum the upper bound for every `--where-*` family.
        // Slight over-allocation when legacy shortcuts are absent is fine;
        // this avoids the 4+ realloc-grow cycles of pushing into an empty
        // `Vec::new()` on busy list calls.
        let mut predicates: Vec<Predicate> = Vec::with_capacity(
            4 + q.where_eq.len()
                + q.where_not.len()
                + q.where_in.len()
                + q.where_has.len()
                + q.where_missing.len()
                + q.where_gt.len()
                + q.where_gte.len()
                + q.where_lt.len()
                + q.where_lte.len()
                + q.where_contains.len()
                + q.where_prefix.len()
                + q.where_suffix.len()
                + q.where_regex.len(),
        );

        // Legacy shortcut flags — map onto the new predicate surface so the
        // query engine has a single filter list to evaluate. Duplicating a
        // legacy flag with an equivalent `--where` is a no-op (same predicate
        // runs twice; same result).
        if let Some(v) = legacy.status {
            predicates.push(Predicate::Where {
                key: "status".into(),
                rhs: v.clone(),
            });
        }
        if let Some(v) = legacy.category {
            predicates.push(Predicate::Where {
                key: "category".into(),
                rhs: v.clone(),
            });
        }
        if let Some(v) = legacy.file {
            predicates.push(Predicate::Where {
                key: "file".into(),
                rhs: v.clone(),
            });
        }
        if let Some(v) = legacy.newer_than {
            // `--newer-than` semantically means "first_flagged > v" where v is
            // a YYYY-MM-DD. The `@date:` prefix tells `parse_typed_value` to
            // coerce the RHS to a TOML date rather than comparing as a string.
            predicates.push(Predicate::WhereGt {
                key: "first_flagged".into(),
                rhs: format!("@date:{}", v),
            });
        }

        for s in &q.where_eq {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::Where { key, rhs });
        }
        for s in &q.where_not {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereNot { key, rhs });
        }
        for s in &q.where_in {
            let (key, rhs) = split_kv(s)?;
            let values: Vec<String> = rhs.split(',').map(|s| s.to_string()).collect();
            predicates.push(Predicate::WhereIn { key, rhs: values });
        }
        for s in &q.where_has {
            if s.is_empty() {
                bail!("--where-has expects a KEY, got empty string");
            }
            predicates.push(Predicate::WhereHas { key: s.clone() });
        }
        for s in &q.where_missing {
            if s.is_empty() {
                bail!("--where-missing expects a KEY, got empty string");
            }
            predicates.push(Predicate::WhereMissing { key: s.clone() });
        }
        for s in &q.where_gt {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereGt { key, rhs });
        }
        for s in &q.where_gte {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereGte { key, rhs });
        }
        for s in &q.where_lt {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereLt { key, rhs });
        }
        for s in &q.where_lte {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereLte { key, rhs });
        }
        for s in &q.where_contains {
            let (key, sub) = split_kv(s)?;
            predicates.push(Predicate::WhereContains { key, sub });
        }
        for s in &q.where_prefix {
            let (key, prefix) = split_kv(s)?;
            predicates.push(Predicate::WherePrefix { key, prefix });
        }
        for s in &q.where_suffix {
            let (key, suffix) = split_kv(s)?;
            predicates.push(Predicate::WhereSuffix { key, suffix });
        }
        for s in &q.where_regex {
            let (key, pattern) = split_kv(s)?;
            predicates.push(Predicate::WhereRegex { key, pattern });
        }

        // Projection: parse `--select a,b` / `--exclude a,b` into Vec<String>.
        // `validate_query` enforces `select` / `exclude` / `pluck` mutual
        // exclusion; we just populate the struct.
        let select_fields: Option<Vec<String>> = q
            .select
            .as_deref()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
        let exclude_fields: Option<Vec<String>> = q
            .exclude
            .as_deref()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());

        // Sort: each entry is `FIELD` or `FIELD:asc` or `FIELD:desc`. Unknown
        // suffix defaults to `asc` (matches the plan).
        let mut sort_list: Vec<(String, SortDir)> = Vec::new();
        for entry in &q.sort_by {
            let (field, dir) = match entry.split_once(':') {
                Some((f, d)) => {
                    let dir = match d {
                        "desc" => SortDir::Desc,
                        _ => SortDir::Asc,
                    };
                    (f.to_string(), dir)
                }
                None => (entry.clone(), SortDir::Asc),
            };
            sort_list.push((field, dir));
        }

        // OutputShape priority (plan): count > count-by > group-by > pluck >
        // default Array. `ndjson` is an *encoding* choice (R82), not a shape
        // — it lives on `Query.ndjson` and only applies when the chosen shape
        // is Array. Multiple shape flags would typically collapse to the
        // highest-priority one here; `validate_query` (inside `run`) then
        // rejects any shape-vs-projection conflict with a clear error.
        let shape = if legacy.count {
            OutputShape::Count
        } else if let Some(f) = q.count_by.as_deref() {
            OutputShape::CountBy(f.to_string())
        } else if let Some(f) = q.group_by.as_deref() {
            OutputShape::GroupBy(f.to_string())
        } else if let Some(f) = q.pluck.as_deref() {
            OutputShape::Pluck(f.to_string())
        } else {
            OutputShape::Array
        };

        Ok(Query {
            predicates,
            select: select_fields,
            exclude: exclude_fields,
            sort_by: sort_list,
            limit: q.limit,
            offset: q.offset,
            distinct: q.distinct,
            shape,
            ndjson: q.ndjson,
        })
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

    // -- R72 / R73 regressions ----------------------------------------

    /// R72: a regex pattern whose compiled NFA would exceed our 1 MiB
    /// size_limit must fail *at compile time*, not silently expand
    /// in-process. `a{N}` with N large is the canonical probe.
    #[test]
    fn where_regex_pathological_pattern_rejected_by_size_limit() {
        let doc = fixture();
        // 2**20 = 1 MiB; `a{1048576}` trivially exceeds 1 MiB NFA size.
        let q = q_with(vec![Predicate::WhereRegex {
            key: "id".into(),
            pattern: "a{1048576}".into(),
        }]);
        let err = run(&doc, "items", &q).unwrap_err().to_string();
        assert!(
            err.contains("invalid regex"),
            "error must name the rejection cause; got: {err}"
        );
    }

    /// R73: a malformed typed RHS on `--where` must propagate as a
    /// `Result::Err`, not silently skip every row.
    #[test]
    fn where_eq_bad_typed_rhs_propagates_error() {
        let doc = fixture();
        let q = q_with(vec![Predicate::Where {
            key: "first_flagged".into(),
            rhs: "@date:not-a-date".into(),
        }]);
        let err = run(&doc, "items", &q).unwrap_err().to_string();
        assert!(
            err.contains("first_flagged") && err.contains("not-a-date"),
            "error must name the key + the bad RHS; got: {err}"
        );
    }

    /// R73: the same contract holds for ordered predicates (Gt/Gte/Lt/Lte).
    #[test]
    fn where_gt_bad_typed_rhs_propagates_error() {
        let doc = fixture();
        let q = q_with(vec![Predicate::WhereGt {
            key: "first_flagged".into(),
            rhs: "@date:not-a-date".into(),
        }]);
        let err = run(&doc, "items", &q).unwrap_err().to_string();
        assert!(
            err.contains("first_flagged") && err.contains("not-a-date"),
            "error must name the key + the bad RHS; got: {err}"
        );
    }

    // -- R9 regression ------------------------------------------------

    /// R9: `--pluck <field> --distinct` must dedupe by the plucked field,
    /// not by the full source item. Previously `apply_projection` only
    /// honoured --select/--exclude, so two items sharing `task_ref` but
    /// differing in other fields yielded duplicate values in the output.
    #[test]
    fn pluck_distinct_dedupes_on_plucked_field_not_whole_item() {
        let src = r#"
[[items]]
id = "i1"
task_ref = "alpha"

[[items]]
id = "i2"
task_ref = "alpha"

[[items]]
id = "i3"
task_ref = "beta"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let q = Query {
            distinct: true,
            shape: OutputShape::Pluck("task_ref".into()),
            // Sort keeps the assertion deterministic; the bug is present
            // regardless of sort order.
            sort_by: vec![("task_ref".into(), SortDir::Asc)],
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(out, serde_json::json!(["alpha", "beta"]));
    }
}
