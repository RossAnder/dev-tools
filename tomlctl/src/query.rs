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

// T1 / R43: test-only invocation counters that let unit tests assert the
// fast-path narrows to the plucked field instead of materialising the whole
// item via `toml_to_json`. Counters are thread-local so parallel cargo-test
// threads don't cross-pollute each other's measurements — each call site
// increments only the counter on the thread that made the call, so a test
// that resets its thread's counters then runs a query will see exactly the
// invocations it caused. Release builds strip the cfg-gated increments and
// the associated helpers entirely; the runtime cost outside `cargo test`
// is zero.
#[cfg(test)]
thread_local! {
    static FAST_PATH_NARROW_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FULL_ITEM_MATERIALISE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_invocation_counters() {
    FAST_PATH_NARROW_CALLS.with(|c| c.set(0));
    FULL_ITEM_MATERIALISE_CALLS.with(|c| c.set(0));
}

#[cfg(test)]
fn snapshot_invocation_counters() -> (usize, usize) {
    (
        FAST_PATH_NARROW_CALLS.with(|c| c.get()),
        FULL_ITEM_MATERIALISE_CALLS.with(|c| c.get()),
    )
}

/// Narrowed `toml_to_json` call used by aggregation fast-paths (Pluck /
/// CountBy / CountDistinct). Bumps the thread-local
/// `FAST_PATH_NARROW_CALLS` counter under cfg(test) so the structural fast-
/// path narrowing contract (R43 / plan T1) can be asserted without relying
/// on timing or memory. In release builds this is a transparent pass-through.
fn narrow_toml_to_json(v: &TomlValue) -> JsonValue {
    #[cfg(test)]
    FAST_PATH_NARROW_CALLS.with(|c| c.set(c.get() + 1));
    toml_to_json(v)
}

/// Full-item `toml_to_json` call used by the slow-path pipeline and any other
/// site that needs a whole-row JSON materialisation. Bumps the thread-local
/// `FULL_ITEM_MATERIALISE_CALLS` counter under cfg(test) so fast-path tests
/// can assert this counter stays at zero.
fn full_item_toml_to_json(v: &TomlValue) -> JsonValue {
    #[cfg(test)]
    FULL_ITEM_MATERIALISE_CALLS.with(|c| c.set(c.get() + 1));
    toml_to_json(v)
}

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
    /// T1: scalar-cardinality aggregate. Emits
    /// `{"count_distinct": N, "field": "<name>"}` where N is the number of
    /// distinct non-null/non-missing values of the field in the filtered
    /// set. Null and missing values are excluded (consistent with `--pluck`
    /// semantics — plucked-null items drop). Type-aware: the canonical
    /// serialisation of each value is hashed, so `42` (Integer) and `"42"`
    /// (String) at the same field name count as two distinct values.
    CountDistinct(String),
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

/// R16: trait dispatch for the three OutputShape-enumerating operations that
/// fanned across six-plus match sites prior to this refactor — the slow-path
/// terminal shape switch in `run()`, the `emit_list_raw` match in `cli.rs`,
/// and the two streaming-guard `matches!` predicates. Keeping one `impl
/// ShapeDispatch for OutputShape` block (inner matches) with three methods
/// collapses those sites into three — one per trait method — so adding a
/// new variant forces exactly one edit per method instead of six-plus
/// scattered enumerations.
///
/// Some enumeration sites — the fast-path in `run()`, the distinct-key
/// branch in `build_pipeline`, the per-variant `validate_query` mutex,
/// and the precedence ladder in `Query::from_query_input` — stay as raw
/// matches. Each needs either variadic per-variant captures, a per-variant
/// construction choice, or per-variant semantics that don't collapse into
/// a shared signature without growing the trait surface past three
/// methods. Those sites are documented at their match arms.
pub(crate) trait ShapeDispatch {
    /// Slow-path terminal shape emit (called by `run()` after `build_pipeline`
    /// has produced the windowed `Vec<JsonValue>`). Each variant either
    /// delegates to an `apply_aggregation_*` / `apply_pluck` helper or, for
    /// Array / GroupBy, projects `windowed` through `apply_projection` first.
    /// Pure — no I/O; the returned `JsonValue` is what `run()` hands back.
    fn compute(&self, windowed: &[JsonValue], q: &Query) -> JsonValue;

    /// Render the bare-scalar form of the JSON value `run()` returned for
    /// this shape. Called from the cli `items list --raw` branch AFTER
    /// `run()` has produced `v`. Count / CountDistinct extract the inner
    /// integer; Pluck enforces N == 1 (with the exact byte-pinned error
    /// wording for N == 0 and N > 1); Array / CountBy / GroupBy error
    /// (validate_query already rejects the two map shapes — the arms here
    /// are defence-in-depth with identical wording). The returned String
    /// is the bytes to emit WITHOUT a trailing newline; the caller adds
    /// `\n` via `print_raw_value`-style wrapping.
    fn raw_emit(&self, v: &JsonValue) -> Result<String>;

    /// Whether the shape supports one-JSON-per-line streaming output. Only
    /// Array and Pluck stream today — an aggregation (single object /
    /// scalar) has no per-line decomposition, so `--ndjson`/`--lines` is a
    /// silent no-op there. Collapses the `matches!(q.shape, Array |
    /// Pluck(_))` guards in `run_streaming` (query.rs) and the `items list`
    /// dispatch arm (cli.rs) into one source of truth.
    fn is_streamable(&self) -> bool;
}

impl ShapeDispatch for OutputShape {
    fn compute(&self, windowed: &[JsonValue], q: &Query) -> JsonValue {
        match self {
            OutputShape::Count => apply_aggregation_count(windowed),
            OutputShape::CountBy(field) => apply_aggregation_count_by(windowed, field),
            OutputShape::CountDistinct(field) => {
                // T1: slow-path tail. `windowed` is `Vec<JsonValue>` — each
                // item is already materialised because sort/distinct/window
                // is engaged. `apply_aggregation_count_distinct` walks once,
                // picks the plucked field per item, and accumulates into a
                // `HashSet<String>` keyed by the canonical serialised form.
                // Null/missing values are excluded from the count (matches
                // `apply_pluck`).
                apply_aggregation_count_distinct(windowed, field)
            }
            OutputShape::GroupBy(field) => {
                // group-by can still respect select/exclude on the grouped items.
                let projected: Vec<JsonValue> =
                    windowed.iter().map(|v| apply_projection(v, q)).collect();
                apply_aggregation_group_by(windowed, &projected, field)
            }
            OutputShape::Pluck(field) => apply_pluck(windowed, field),
            OutputShape::Array => {
                let projected: Vec<JsonValue> =
                    windowed.iter().map(|v| apply_projection(v, q)).collect();
                JsonValue::Array(projected)
            }
        }
    }

    fn raw_emit(&self, v: &JsonValue) -> Result<String> {
        match self {
            OutputShape::Count => {
                // Shape is `{"count": N}` — reach in, render the bare integer.
                let n = v
                    .get("count")
                    .ok_or_else(|| anyhow::anyhow!("internal: --count output missing `count` key"))?;
                emit_raw(n)
            }
            OutputShape::CountDistinct(_) => {
                // Shape is `{"count_distinct": N, "field": "<name>"}` — drop
                // the `field` key (it's echo-only metadata) and render the
                // bare count.
                let n = v.get("count_distinct").ok_or_else(|| {
                    anyhow::anyhow!(
                        "internal: --count-distinct output missing `count_distinct` key"
                    )
                })?;
                emit_raw(n)
            }
            OutputShape::Pluck(_) => {
                // Shape is `[v0, v1, ...]`. N==1 is the only emittable
                // cardinality without `--lines`; anything else errors with
                // the exact task-spec wording (tests assert byte-for-byte).
                let arr = v
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("internal: --pluck output was not a JSON array"))?;
                match arr.len() {
                    0 => bail!("--raw requires single-value output (got 0 items)"),
                    1 => emit_raw(&arr[0]),
                    n => bail!(
                        "--raw requires single-value output (got {} items); use --lines for newline-delimited",
                        n
                    ),
                }
            }
            OutputShape::Array => {
                // Array + raw without `--lines` is defensive — an agent who
                // blanket-added `--raw` to a plain `items list` gets a clear
                // error rather than a corrupt pretty-print of a JSON array
                // with quotes stripped. Byte-for-byte wording isn't pinned
                // by the task spec for this case; keep it descriptive.
                bail!("--raw requires a scalar target; got array");
            }
            OutputShape::CountBy(_) | OutputShape::GroupBy(_) => {
                // Unreachable in practice: `validate_query` rejects these
                // combinations with the canonical message before `run()`
                // returns. Mirror the same message here for defence in depth
                // — if validation is ever restructured and this path fires,
                // the error the user sees is still the pinned one.
                bail!(
                    "--raw is not supported on --count-by / --group-by (output is a map, not a scalar)"
                );
            }
        }
    }

    fn is_streamable(&self) -> bool {
        matches!(self, OutputShape::Array | OutputShape::Pluck(_))
    }
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
    /// T2: bare-scalar output (`--raw`). `run()` itself is oblivious to
    /// this bit — raw-conversion happens at the cli.rs dispatch boundary
    /// AFTER `run()` returns the JSON-shaped result. The exception is
    /// the streaming Pluck path (`--pluck f --lines --raw`) which
    /// short-circuits into `run_streaming` with the bare-value emit inline
    /// so we don't materialise quoted JSON only to strip it per line. The
    /// validation layer also consults this bit (count-by / group-by +
    /// raw is rejected at `validate_query`). Default false keeps every
    /// non-raw path byte-identical.
    pub raw: bool,
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
        OutputShape::CountDistinct(_) => {
            // T1: projection on an aggregation-only shape is ambiguous — the
            // output is a single `{count_distinct, field}` object, not an
            // array of items, so `--select`/`--exclude` has no row shape to
            // narrow. Mirror the CountBy/Count wording so agents reading the
            // error see a consistent style across aggregation shapes.
            if q.select.is_some() {
                validation_bail!(
                    "--select and --count-distinct are mutually exclusive"
                );
            }
            if q.exclude.is_some() {
                validation_bail!(
                    "--exclude and --count-distinct are mutually exclusive"
                );
            }
        }
        OutputShape::GroupBy(_) => {
            // group-by composes fine with projection; no cross-exclusion here.
        }
        OutputShape::Array => {}
    }
    // T2: `--raw` is a scalar-output primitive — only meaningful on shapes
    // that collapse to a single scalar (Count / CountDistinct) or a
    // single-value Pluck. `--count-by` emits a map `{bucket: count, ...}`
    // and `--group-by` emits `{bucket: [items, ...], ...}`; neither has a
    // well-defined bare-scalar conversion, so we fail loud here rather
    // than let the agent see a confusing error from `emit_raw` on the
    // object shape. Wording is load-bearing — tests pin the exact string.
    if q.raw
        && matches!(
            q.shape,
            OutputShape::CountBy(_) | OutputShape::GroupBy(_)
        )
    {
        validation_bail!(
            "--raw is not supported on --count-by / --group-by (output is a map, not a scalar)"
        );
    }
    // Cross-shape pairs. `main.rs` is expected to pick exactly one of the
    // below shapes, but we still double-check so callers with programmatic
    // builders can't accidentally set two.
    // (The clap layer normally collapses these by priority; this is belt
    // & braces.)
    Ok(())
}

/// R5: shared slow-path pipeline for `run()` and `run_streaming()`. Both
/// entry points historically re-implemented the same
/// materialise → sort → distinct → window sequence with only the terminal
/// emit differing, and the two distinct-key branches (Array via projection,
/// Pluck via field) had to stay byte-identical. Factoring here keeps the
/// invariant in one place: callers get back the fully-windowed
/// `Vec<JsonValue>` and are responsible only for per-shape terminal emit
/// (build_pipeline does NOT run the fast-path guard — each caller decides
/// how to handle the `window_untouched` short-circuit since the emit shape
/// varies, and bypassing this helper for the fast-path keeps the per-item
/// cost of CountDistinct / Pluck / CountBy at O(field size) as before).
///
/// Note: materialisation uses `full_item_toml_to_json` so the R43 counters
/// record one "full-item" hit per filtered row. This mirrors the behaviour
/// of the pre-refactor inline code verbatim.
fn build_pipeline(filtered: &[&TomlValue], q: &Query) -> Vec<JsonValue> {
    // 2. Project (select/exclude) before shaping for Array/Pluck/Distinct
    // so distinct/pluck see the already-narrowed shape. Aggregations
    // (count/count-by/group-by) operate on the unprojected items so the
    // grouping key is always reachable.
    let for_shape: Vec<JsonValue> = filtered.iter().map(|t| full_item_toml_to_json(t)).collect();

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
    apply_window(deduped, q.offset, q.limit)
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
                    match t.get(field).map(narrow_toml_to_json) {
                        None | Some(JsonValue::Null) => {}
                        Some(v) => out.push(v),
                    }
                }
                return Ok(JsonValue::Array(out));
            }
            OutputShape::CountBy(field) => {
                let mut counts: serde_json::Map<String, JsonValue> = serde_json::Map::new();
                for t in &filtered {
                    let key = match t.get(field).map(narrow_toml_to_json) {
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
            OutputShape::CountDistinct(field) => {
                // T1: structural analogue of the CountBy fast-path — one
                // `toml_to_json` call per item, applied to the plucked
                // field only. We never materialise the rest of the item,
                // so per-item cost stays O(field size) rather than O(row
                // size). Null/missing values are dropped (consistent with
                // `--pluck`'s apply_pluck behaviour). Canonicalisation:
                // `toml_to_json(v).to_string()` feeds each distinct value
                // into a `HashSet<String>`. This preserves TYPE
                // distinctness — integer `42` serialises as `42`, string
                // `"42"` serialises as `"42"` (with quotes), so they
                // count as two. Documented in the enum doc-comment.
                //
                // R43: `narrow_toml_to_json` increments the fast-path
                // narrowing counter under cfg(test) so the structural
                // contract ("fast-path must NOT materialise the whole
                // item") is asserted directly rather than inferred from
                // timing. `full_item_toml_to_json` on the slow path
                // increments a parallel counter; the test verifies the
                // full-item counter stays at 0 on the fast path.
                let mut seen: HashSet<String> = HashSet::with_capacity(filtered.len());
                for t in &filtered {
                    match t.get(field).map(narrow_toml_to_json) {
                        None | Some(JsonValue::Null) => {}
                        Some(v) => {
                            seen.insert(v.to_string());
                        }
                    }
                }
                return Ok(serde_json::json!({
                    "count_distinct": seen.len(),
                    "field": field,
                }));
            }
            _ => {}
        }
    }

    // R5: slow-path pipeline (materialise → sort → distinct → window) lives
    // in `build_pipeline` so `run()` and `run_streaming()` share one
    // implementation. Only the per-shape terminal emit below differs.
    let windowed = build_pipeline(&filtered, q);

    // 6. Shape. R16: single-arm dispatch via `ShapeDispatch::compute` —
    // per-variant bodies (including the project-then-aggregate branches
    // for Array and GroupBy) live in the `impl ShapeDispatch for OutputShape`
    // block so adding a new variant here is one `match` arm, not six.
    Ok(q.shape.compute(&windowed, q))
}

/// T2: convert a single `JsonValue` scalar into its bare on-stdout form.
///
/// - String: emits the underlying `&str` verbatim — no quotes, no escapes.
///   A string containing a newline is a legitimate single logical value
///   that happens to span multiple output lines; agents are responsible
///   for their own escaping if they need it.
/// - Number: `serde_json::Number::to_string` preserves integer / float
///   disambiguation — `42` stays `"42"`, `42.0` stays `"42.0"` — which
///   matches the JSON-output shape exactly except for the surrounding
///   whitespace / commas.
/// - Bool: `true` / `false`.
/// - Null: unreachable on happy paths (Pluck and `get` both reject/drop
///   nulls upstream), but errors cleanly if one ever leaks through —
///   keeps the helper total.
/// - Array / Object: error with the exact load-bearing message that the
///   `Cmd::Get --raw` spec pins. The tests assert byte-for-byte.
///
/// Callers: `Cmd::Get --raw` (via `cli::print_raw_value`) and the
/// `items list --pluck --raw` dispatch branch (after it has asserted
/// N==1). For `--count` / `--count-distinct --raw` the dispatch extracts
/// the inner count integer and feeds just that number in, so the Object
/// arm is never reached from the aggregation paths.
///
/// R14: lives in `query` rather than `cli` so `run_streaming` can emit
/// bare scalars without inverting the module layering (cli → query is
/// the correct direction). Pure `JsonValue → String` transform; the
/// module's "I/O-free" docstring continues to hold at this function.
pub(crate) fn emit_raw(v: &JsonValue) -> Result<String> {
    match v {
        JsonValue::String(s) => Ok(s.clone()),
        JsonValue::Number(n) => Ok(n.to_string()),
        JsonValue::Bool(b) => Ok(b.to_string()),
        JsonValue::Null => {
            bail!("--raw cannot emit null value")
        }
        JsonValue::Array(_) => {
            bail!("--raw requires a scalar target; got array")
        }
        JsonValue::Object(_) => {
            bail!("--raw requires a scalar target; got table")
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
///
/// T3: extended to also stream `OutputShape::Pluck(field)` when ndjson is
/// set. Each surviving plucked value is emitted on its own line using the
/// compact `serde_json::to_writer` encoding (strings quoted, numbers bare,
/// etc.). Null/missing values are dropped to match `apply_pluck`'s
/// semantics byte-for-byte — the streaming path MUST preserve which values
/// land in the output (or we'd break the `--pluck x --lines` vs
/// `--pluck x` parity contract that lets agents blanket-add the flag).
/// Aggregation shapes (Count/CountBy/CountDistinct/GroupBy) still fall
/// through to the single-shot delegation — a single JSON value has no
/// sensible "one per line" decomposition.
pub(crate) fn run_streaming<W: Write>(
    doc: &TomlValue,
    array_name: &str,
    q: &Query,
    writer: &mut W,
) -> Result<()> {
    // Non-streamable shapes (Count/CountBy/CountDistinct/GroupBy, or Array
    // / Pluck without ndjson encoding) don't benefit from streaming — the
    // final value is a single object/scalar, or the caller explicitly
    // chose the batched array encoding. Delegate to `run()` and serialise
    // once.
    if !q.ndjson || !q.shape.is_streamable() {
        let out = run(doc, array_name, q)?;
        serde_json::to_writer(writer, &out)?;
        return Ok(());
    }

    // Array/Pluck + ndjson streaming path. Mirrors the Array/Pluck arms of
    // `run()`: filter → (project/sort/distinct/window) → emit each element
    // with a trailing newline as it's produced. We still need the full
    // in-memory pipeline up to the final emit because sort/distinct are
    // inherently non-streaming; the win is in avoiding the terminal
    // `Vec<JsonValue>` that `run()` returns when the caller wants
    // line-per-item output.
    validate_query(q)?;
    let items: &[TomlValue] = match doc.get(array_name).and_then(|v| v.as_array()) {
        Some(arr) => arr.as_slice(),
        None => &[],
    };
    let filtered = apply_filters(items, &q.predicates)?;

    let window_untouched = q.sort_by.is_empty()
        && !q.distinct
        && q.offset.is_none()
        && q.limit.is_none();

    // T3: Pluck streaming. The structure mirrors the Array branch below —
    // a fast-path that skips the full Vec<JsonValue> materialisation when
    // sort/distinct/window are all disengaged, and a slow-path that walks
    // the full pipeline but emits per-item at the tail. Null/missing
    // plucked values are dropped in both paths so the output is byte-for-
    // byte equivalent (ignoring array brackets and separators) to the
    // array produced by `apply_pluck`.
    if let OutputShape::Pluck(field) = &q.shape {
        if window_untouched {
            // Fast-path: never materialise the full row — pluck the field
            // straight off the TomlValue and convert only the plucked
            // value. This matches the fast-path in `run()` at lines
            // 235-244, keeping streaming and non-streaming membership
            // identical.
            //
            // T2: when `q.raw` is set, emit the bare-scalar form (strings
            // unquoted) instead of the JSON-encoded one. Null/missing
            // drops are identical — raw is purely an encoding choice at
            // the emit point. Re-using the local `emit_raw` keeps the
            // scalar-rendering rules in one place so a table/array leak
            // (shouldn't happen — Pluck flattens to scalars — but a
            // defensive double-check) surfaces with the canonical error.
            for t in &filtered {
                match t.get(field).map(narrow_toml_to_json) {
                    None | Some(JsonValue::Null) => {}
                    Some(v) => {
                        if q.raw {
                            writer.write_all(emit_raw(&v)?.as_bytes())?;
                        } else {
                            serde_json::to_writer(&mut *writer, &v)?;
                        }
                        writer.write_all(b"\n")?;
                    }
                }
            }
            return Ok(());
        }
        // Slow path: share the materialise → sort → distinct → window
        // pipeline with `run()` via `build_pipeline` (R5). Pluck's
        // plucked-field distinct-key branch lives inside that helper so
        // both entry points stay in sync. The final emit below
        // replicates `apply_pluck`'s null/missing drop.
        let windowed = build_pipeline(&filtered, q);
        for v in &windowed {
            match v.get(field) {
                None | Some(JsonValue::Null) => {}
                Some(item_val) => {
                    // T2: same raw-vs-json fork as the fast-path above.
                    if q.raw {
                        writer.write_all(emit_raw(item_val)?.as_bytes())?;
                    } else {
                        serde_json::to_writer(&mut *writer, item_val)?;
                    }
                    writer.write_all(b"\n")?;
                }
            }
        }
        return Ok(());
    }

    // Array fast-path: no sort/distinct/window. Stream directly from the
    // filtered items through projection — one JsonValue per item, emitted
    // and dropped before the next is built. This is the single biggest
    // reduction in peak memory vs `run()`.
    if window_untouched {
        for t in &filtered {
            // R43: Array streaming already materialises the full row
            // because `apply_projection` needs the whole object shape —
            // this is not the fast-path narrowing contract, so we bill
            // it to the full-item counter so test assertions on narrow-
            // only paths stay honest.
            let v = full_item_toml_to_json(t);
            let projected = apply_projection(&v, q);
            serde_json::to_writer(&mut *writer, &projected)?;
            writer.write_all(b"\n")?;
        }
        return Ok(());
    }

    // Array slow path (sort/distinct/window touched): share the pipeline
    // with `run()` via `build_pipeline` (R5), then emit per-item at the
    // tail rather than collecting.
    let windowed = build_pipeline(&filtered, q);
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

/// T1: slow-path `--count-distinct` aggregator. The fast-path (in `run()`
/// when no sort/distinct/window is engaged) operates on `&[&TomlValue]`
/// directly; this variant runs after the pipeline has already materialised
/// items as `Vec<JsonValue>` so dedup walks `v.get(field)` instead of
/// `t.get(field).map(toml_to_json)`. Semantics match the fast-path byte-
/// for-byte: null and missing values excluded, `to_string()` used as the
/// canonical-form key so type distinctness holds (`42` ≠ `"42"`).
pub(crate) fn apply_aggregation_count_distinct(items: &[JsonValue], field: &str) -> JsonValue {
    let mut seen: HashSet<String> = HashSet::with_capacity(items.len());
    for v in items {
        match v.get(field) {
            None | Some(JsonValue::Null) => {}
            Some(item_val) => {
                seen.insert(item_val.to_string());
            }
        }
    }
    serde_json::json!({
        "count_distinct": seen.len(),
        "field": field,
    })
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

/// Plain-old-data input for `Query::from_query_input`. Draws the module
/// boundary between the clap-derive layer (`cli.rs`) and the query engine
/// (this module): query.rs used to reach back into `crate::cli` for
/// `LegacyShortcuts<'_>` and `QueryArgs`, which inverted the intended
/// dependency direction (cli → query). This POD owns only primitive
/// standard-library types — no clap derives, no `'a` lifetimes — so the
/// query module compiles without any `use crate::cli` import. Fields
/// mirror the subset of `LegacyShortcuts` + `QueryArgs` that
/// `from_query_input` actually reads; see
/// `cli::query_input_from_cli` for the trivial field-copy adapter.
pub(crate) struct QueryInput {
    // Legacy shortcut flags (pre-query-engine back-compat).
    pub(crate) status: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) file: Option<String>,
    pub(crate) newer_than: Option<String>,
    pub(crate) count: bool,
    // Filter predicates (repeatable KEY=VAL families).
    pub(crate) where_eq: Vec<String>,
    pub(crate) where_not: Vec<String>,
    pub(crate) where_in: Vec<String>,
    pub(crate) where_has: Vec<String>,
    pub(crate) where_missing: Vec<String>,
    pub(crate) where_gt: Vec<String>,
    pub(crate) where_gte: Vec<String>,
    pub(crate) where_lt: Vec<String>,
    pub(crate) where_lte: Vec<String>,
    pub(crate) where_contains: Vec<String>,
    pub(crate) where_prefix: Vec<String>,
    pub(crate) where_suffix: Vec<String>,
    pub(crate) where_regex: Vec<String>,
    // Projection + shape + pagination.
    pub(crate) select: Option<String>,
    pub(crate) exclude: Option<String>,
    pub(crate) pluck: Option<String>,
    pub(crate) sort_by: Vec<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) offset: Option<usize>,
    pub(crate) distinct: bool,
    pub(crate) group_by: Option<String>,
    pub(crate) count_by: Option<String>,
    pub(crate) count_distinct: Option<String>,
    // Output-encoding bits.
    pub(crate) ndjson: bool,
    pub(crate) lines: bool,
    pub(crate) raw: bool,
}

/// Split a `KEY=VAL` string on the first `=`. Empty keys are rejected. The
/// value is returned verbatim (no trimming) so callers that care about
/// whitespace-significant RHS values (e.g. `--where-prefix name= foo`) keep
/// their payload intact. Used by `Query::from_query_input` for every
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
    /// Build a `Query` from a POD `QueryInput`. Validation is handled by
    /// `run` itself — the first thing it does is call `validate_query` on
    /// the spec, so callers don't need to (R88).
    ///
    /// R15: the input type is a plain-old-data `QueryInput` owned by this
    /// module, NOT `&crate::cli::LegacyShortcuts` + `&crate::cli::QueryArgs`
    /// as earlier revisions used. The cli crate now provides
    /// `query_input_from_cli` as a trivial field-copy adapter, severing
    /// the inverted `query → cli` import. R69's bundling motivation still
    /// holds: the dispatch site is a one-line call rather than a 26-line
    /// arg spray.
    ///
    /// R30: this translation from CLI args into domain-level `Predicate`
    /// / `OutputShape` values is business logic tightly coupled to
    /// `query`'s types, not pure CLI plumbing, so it lives here (not in
    /// cli.rs).
    pub(crate) fn from_query_input(input: &QueryInput) -> Result<Self> {
        // O46: pre-size the predicate vec. The `4` covers the four legacy
        // shortcut slots (`status`, `category`, `file`, `newer_than`); the
        // remaining terms sum the upper bound for every `--where-*` family.
        // Slight over-allocation when legacy shortcuts are absent is fine;
        // this avoids the 4+ realloc-grow cycles of pushing into an empty
        // `Vec::new()` on busy list calls.
        let mut predicates: Vec<Predicate> = Vec::with_capacity(
            4 + input.where_eq.len()
                + input.where_not.len()
                + input.where_in.len()
                + input.where_has.len()
                + input.where_missing.len()
                + input.where_gt.len()
                + input.where_gte.len()
                + input.where_lt.len()
                + input.where_lte.len()
                + input.where_contains.len()
                + input.where_prefix.len()
                + input.where_suffix.len()
                + input.where_regex.len(),
        );

        // Legacy shortcut flags — map onto the new predicate surface so the
        // query engine has a single filter list to evaluate. Duplicating a
        // legacy flag with an equivalent `--where` is a no-op (same predicate
        // runs twice; same result).
        if let Some(v) = &input.status {
            predicates.push(Predicate::Where {
                key: "status".into(),
                rhs: v.clone(),
            });
        }
        if let Some(v) = &input.category {
            predicates.push(Predicate::Where {
                key: "category".into(),
                rhs: v.clone(),
            });
        }
        if let Some(v) = &input.file {
            predicates.push(Predicate::Where {
                key: "file".into(),
                rhs: v.clone(),
            });
        }
        if let Some(v) = &input.newer_than {
            // `--newer-than` semantically means "first_flagged > v" where v is
            // a YYYY-MM-DD. The `@date:` prefix tells `parse_typed_value` to
            // coerce the RHS to a TOML date rather than comparing as a string.
            predicates.push(Predicate::WhereGt {
                key: "first_flagged".into(),
                rhs: format!("@date:{}", v),
            });
        }

        for s in &input.where_eq {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::Where { key, rhs });
        }
        for s in &input.where_not {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereNot { key, rhs });
        }
        for s in &input.where_in {
            let (key, rhs) = split_kv(s)?;
            let values: Vec<String> = rhs.split(',').map(|s| s.to_string()).collect();
            predicates.push(Predicate::WhereIn { key, rhs: values });
        }
        for s in &input.where_has {
            if s.is_empty() {
                bail!("--where-has expects a KEY, got empty string");
            }
            predicates.push(Predicate::WhereHas { key: s.clone() });
        }
        for s in &input.where_missing {
            if s.is_empty() {
                bail!("--where-missing expects a KEY, got empty string");
            }
            predicates.push(Predicate::WhereMissing { key: s.clone() });
        }
        for s in &input.where_gt {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereGt { key, rhs });
        }
        for s in &input.where_gte {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereGte { key, rhs });
        }
        for s in &input.where_lt {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereLt { key, rhs });
        }
        for s in &input.where_lte {
            let (key, rhs) = split_kv(s)?;
            predicates.push(Predicate::WhereLte { key, rhs });
        }
        for s in &input.where_contains {
            let (key, sub) = split_kv(s)?;
            predicates.push(Predicate::WhereContains { key, sub });
        }
        for s in &input.where_prefix {
            let (key, prefix) = split_kv(s)?;
            predicates.push(Predicate::WherePrefix { key, prefix });
        }
        for s in &input.where_suffix {
            let (key, suffix) = split_kv(s)?;
            predicates.push(Predicate::WhereSuffix { key, suffix });
        }
        for s in &input.where_regex {
            let (key, pattern) = split_kv(s)?;
            predicates.push(Predicate::WhereRegex { key, pattern });
        }

        // Projection: parse `--select a,b` / `--exclude a,b` into Vec<String>.
        // `validate_query` enforces `select` / `exclude` / `pluck` mutual
        // exclusion; we just populate the struct.
        let select_fields: Option<Vec<String>> = input
            .select
            .as_deref()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
        let exclude_fields: Option<Vec<String>> = input
            .exclude
            .as_deref()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());

        // Sort: each entry is `FIELD` or `FIELD:asc` or `FIELD:desc`. Unknown
        // suffix defaults to `asc` (matches the plan).
        let mut sort_list: Vec<(String, SortDir)> = Vec::new();
        for entry in &input.sort_by {
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

        // OutputShape priority (plan): count > count-by > count-distinct >
        // group-by > pluck > default Array. `ndjson` is an *encoding* choice
        // (R82), not a shape — it lives on `Query.ndjson` and only applies
        // when the chosen shape is Array. Multiple shape flags would
        // typically collapse to the highest-priority one here; the clap
        // `shape` ArgGroup at the CLI layer makes this impossible in
        // practice (any two shape flags error at parse time), but we keep
        // the priority ladder as a belt-and-braces for programmatic callers
        // that skip clap (`Query::from_query_input` is called from tests too).
        //
        // T1: `--count-distinct` sits at EQUAL precedence to the other
        // aggregation shapes — NOT as a sub-form of Pluck. Risk #2 in the
        // plan: the ArgGroup guarantees exclusivity at parse time; the
        // `count_distinct_and_pluck_are_mutex_at_parse_time` integration
        // test pins this contract.
        let shape = if input.count {
            OutputShape::Count
        } else if let Some(f) = input.count_by.as_deref() {
            OutputShape::CountBy(f.to_string())
        } else if let Some(f) = input.count_distinct.as_deref() {
            OutputShape::CountDistinct(f.to_string())
        } else if let Some(f) = input.group_by.as_deref() {
            OutputShape::GroupBy(f.to_string())
        } else if let Some(f) = input.pluck.as_deref() {
            OutputShape::Pluck(f.to_string())
        } else {
            OutputShape::Array
        };

        Ok(Query {
            predicates,
            select: select_fields,
            exclude: exclude_fields,
            sort_by: sort_list,
            limit: input.limit,
            offset: input.offset,
            distinct: input.distinct,
            shape,
            // T3: `--lines` and `--ndjson` both map onto the same internal
            // boolean — the spellings differ only at the CLI surface. `--lines`
            // is the discoverable spelling for the Pluck case; `--ndjson` is
            // the historic spelling for the Array case. Both enable the
            // streaming per-line encoding for Array and Pluck shapes; for
            // aggregation shapes (Count/CountBy/CountDistinct/GroupBy) the
            // bit is silently ignored (single-value output — "one per line"
            // collapses to the same bytes). This keeps downstream pipeline
            // logic inspecting a single boolean.
            ndjson: input.ndjson || input.lines,
            // T2: propagate `--raw` through to the query spec. Most of the
            // dispatch machinery is oblivious; the cli.rs `items list`
            // branch post-processes the `run()` result when `raw` is set,
            // and the streaming Pluck path threads this bit through to
            // emit bare values per line.
            raw: input.raw,
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

    // -- T1: --count-distinct shape ---------------------------------------

    /// T1: baseline — distinct categories in the 6-row fixture are
    /// security, quality, performance → 3 distinct.
    #[test]
    fn count_distinct_counts_distinct_values() {
        let doc = fixture();
        let q = Query {
            shape: OutputShape::CountDistinct("category".into()),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(
            out,
            serde_json::json!({"count_distinct": 3, "field": "category"})
        );
    }

    /// T1: items lacking the plucked field are excluded from the count —
    /// mirror of `apply_pluck`'s missing-field drop.
    #[test]
    fn count_distinct_excludes_missing_field() {
        // `defer_reason` exists on R4 only (R6's defer_reason is empty
        // string — a value, not missing — so it counts as one distinct).
        let doc = fixture();
        let q = Query {
            shape: OutputShape::CountDistinct("defer_reason".into()),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        // R4: "vendor fix", R6: "" → 2 distinct values; other four rows
        // missing the field are excluded.
        assert_eq!(out["count_distinct"], 2);
        assert_eq!(out["field"], "defer_reason");
    }

    /// T1: explicit TOML-null-equivalents (missing field) are excluded.
    /// Distinct from `count_distinct_excludes_missing_field` above in
    /// that we build the fixture explicitly so the intent is unambiguous.
    #[test]
    fn count_distinct_excludes_null_values() {
        // TOML has no `null` literal at the top level — a field with no
        // value is represented by absence. `toml_to_json` maps absence to
        // None (not JsonValue::Null), so both cases (missing key and an
        // otherwise-constructed Null) fall through to the same branch.
        let src = r#"
[[items]]
id = "a"
ref = "alpha"

[[items]]
id = "b"

[[items]]
id = "c"
ref = "beta"

[[items]]
id = "d"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let q = Query {
            shape: OutputShape::CountDistinct("ref".into()),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(out["count_distinct"], 2);
        assert_eq!(out["field"], "ref");
    }

    /// T1: empty [[items]] → count_distinct = 0.
    #[test]
    fn count_distinct_empty_array() {
        let src = r#"
schema_version = 1
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let q = Query {
            shape: OutputShape::CountDistinct("whatever".into()),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(
            out,
            serde_json::json!({"count_distinct": 0, "field": "whatever"})
        );
    }

    /// T1: 1000 items × 500 distinct values → N=500. Exercises the
    /// HashSet sizing path and confirms dedup fires even on the fast-path
    /// (no sort/distinct/window in this query).
    #[test]
    fn count_distinct_large_cardinality() {
        let mut buf = String::from("schema_version = 1\n");
        for i in 0..1000 {
            buf.push_str(&format!(
                "\n[[items]]\nid = \"i{i}\"\nbucket = \"b{}\"\n",
                i % 500
            ));
        }
        let doc: TomlValue = toml::from_str(&buf).unwrap();
        let q = Query {
            shape: OutputShape::CountDistinct("bucket".into()),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        assert_eq!(out["count_distinct"], 500);
    }

    /// T1: type distinctness contract — integer 42 and string "42" at
    /// the same field count as 2 distinct values. Rationale: different
    /// TOML types round-trip through `toml_to_json().to_string()` to
    /// different canonical forms (`42` vs `"42"`), so the HashSet keeps
    /// them apart. Callers who want string-coerced equality can combine
    /// `--pluck f` with a downstream `jq -r 'tostring'` — but the
    /// transcript audit shows nobody does that; they want type-aware
    /// counts.
    #[test]
    fn count_distinct_different_types() {
        let src = r#"
[[items]]
id = "a"
v = 42

[[items]]
id = "b"
v = "42"

[[items]]
id = "c"
v = 42
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let q = Query {
            shape: OutputShape::CountDistinct("v".into()),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        // Integer 42 (×2, dedupes to 1) + String "42" (×1) = 2 distinct.
        assert_eq!(out["count_distinct"], 2);
    }

    /// T1: filter semantics — `--where` narrows the input set BEFORE
    /// `--count-distinct` counts. Check by restricting to open-status
    /// rows: categories present among open rows of the fixture are
    /// security (R1) and quality (R2, R6) — 2 distinct, not 3.
    #[test]
    fn count_distinct_after_filter() {
        let doc = fixture();
        let q = Query {
            predicates: vec![Predicate::Where {
                key: "status".into(),
                rhs: "open".into(),
            }],
            shape: OutputShape::CountDistinct("category".into()),
            ..Default::default()
        };
        let out = run(&doc, "items", &q).unwrap();
        // Open items: R1 (security), R2 (quality), R5 (performance),
        // R6 (quality) → distinct = {security, quality, performance} = 3.
        assert_eq!(out["count_distinct"], 3);
    }

    /// T1: adding `--sort-by` sends the query through the slow path but
    /// must still yield the same cardinality (sort doesn't change the
    /// distinct-set). Regression guard for the compute-before-sort
    /// assumption.
    #[test]
    fn count_distinct_with_sort_is_valid_but_wasteful() {
        let doc = fixture();
        let q_fast = Query {
            shape: OutputShape::CountDistinct("category".into()),
            ..Default::default()
        };
        let q_slow = Query {
            sort_by: vec![("id".into(), SortDir::Asc)],
            shape: OutputShape::CountDistinct("category".into()),
            ..Default::default()
        };
        let a = run(&doc, "items", &q_fast).unwrap();
        let b = run(&doc, "items", &q_slow).unwrap();
        assert_eq!(a, b);
        assert_eq!(a["count_distinct"], 3);
    }

    /// T1 / R43 structural assertion: the fast-path MUST NOT materialise
    /// the entire item via `toml_to_json(item)` — only the plucked field.
    /// Instrumentation via the thread-local `FAST_PATH_NARROW_CALLS` /
    /// `FULL_ITEM_MATERIALISE_CALLS` counters (see the cfg(test) block at
    /// the top of this module) records which variant fired at each call
    /// site, so this test asserts the structural contract directly rather
    /// than inferring it from timing. Using thread-local counters avoids
    /// cross-pollution from parallel cargo-test threads in the same
    /// binary — each test observes only its own thread's invocations.
    ///
    /// Contract verified here:
    ///   1. Fast-path over N items increments `FAST_PATH_NARROW_CALLS` by
    ///      exactly N and leaves `FULL_ITEM_MATERIALISE_CALLS` at 0.
    ///   2. Slow-path (sort-by engaged) produces the same count but
    ///      increments `FULL_ITEM_MATERIALISE_CALLS` by N — the whole
    ///      row must be materialised to feed the sort comparator.
    #[test]
    fn count_distinct_fast_path_narrows_to_field() {
        let mut buf = String::from("schema_version = 1\n");
        // 200 rows × 5 distinct buckets, each row carrying a 100-element
        // array of integers in `unrelated`.
        let unrelated: String = (0..100)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        for i in 0..200 {
            buf.push_str(&format!(
                "\n[[items]]\nid = \"i{i}\"\nbucket = \"b{}\"\nunrelated = [{}]\n",
                i % 5,
                unrelated
            ));
        }
        let doc: TomlValue = toml::from_str(&buf).unwrap();
        let q_fast = Query {
            shape: OutputShape::CountDistinct("bucket".into()),
            ..Default::default()
        };
        let q_slow = Query {
            sort_by: vec![("id".into(), SortDir::Asc)],
            shape: OutputShape::CountDistinct("bucket".into()),
            ..Default::default()
        };

        // Counters are thread-local, so no mutex is needed — only this
        // thread's invocations accumulate. Reset at entry to clear any
        // stale state left by earlier assertions on the same thread.
        reset_invocation_counters();

        // Fast-path: exactly N narrow calls, zero full-item calls.
        let a = run(&doc, "items", &q_fast).unwrap();
        let (narrow_fast, full_fast) = snapshot_invocation_counters();
        assert_eq!(
            narrow_fast, 200,
            "fast-path must narrow to the plucked field exactly once per item"
        );
        assert_eq!(
            full_fast, 0,
            "fast-path must NOT materialise the full row via toml_to_json"
        );

        // Slow-path: whole-row materialisation for the sort pipeline.
        reset_invocation_counters();
        let b = run(&doc, "items", &q_slow).unwrap();
        let (narrow_slow, full_slow) = snapshot_invocation_counters();
        assert_eq!(
            full_slow, 200,
            "slow-path must materialise the full row (sort needs whole-item shape)"
        );
        assert_eq!(
            narrow_slow, 0,
            "slow-path must NOT hit the fast-path narrow sites"
        );

        assert_eq!(a, b);
        assert_eq!(a["count_distinct"], 5);
    }

    /// T1: mutex with `--select` via `validate_query`.
    #[test]
    fn count_distinct_plus_select_rejected() {
        let q = Query {
            shape: OutputShape::CountDistinct("category".into()),
            select: Some(vec!["id".into()]),
            ..Default::default()
        };
        let err = validate_query(&q).unwrap_err().to_string();
        assert!(
            err.contains("--select") && err.contains("--count-distinct"),
            "expected mutex error naming both flags, got: {err}"
        );
    }

    /// T1: mutex with `--exclude` via `validate_query`.
    #[test]
    fn count_distinct_plus_exclude_rejected() {
        let q = Query {
            shape: OutputShape::CountDistinct("category".into()),
            exclude: Some(vec!["summary".into()]),
            ..Default::default()
        };
        let err = validate_query(&q).unwrap_err().to_string();
        assert!(
            err.contains("--exclude") && err.contains("--count-distinct"),
            "expected mutex error naming both flags, got: {err}"
        );
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

    // -- T3: streaming Pluck ------------------------------------------

    /// T3 fast-path: `run_streaming` on Pluck+ndjson with no
    /// sort/distinct/window should emit one compact JSON value per line,
    /// dropping null/missing plucked fields to match `apply_pluck`.
    #[test]
    fn run_streaming_pluck_fast_path_emits_one_per_line() {
        let src = r#"
[[items]]
id = "R1"
x = "v1"

[[items]]
id = "R2"
# x missing — must drop

[[items]]
id = "R3"
x = "v3"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let q = Query {
            shape: OutputShape::Pluck("x".into()),
            ndjson: true,
            ..Default::default()
        };
        let mut buf: Vec<u8> = Vec::new();
        run_streaming(&doc, "items", &q, &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "\"v1\"\n\"v3\"\n");
    }

    /// T3 slow-path: with sort/distinct engaged, the Pluck streaming
    /// branch falls into the full-pipeline arm. The emitted byte stream
    /// must still be one-per-line, respect sort order, and dedupe by the
    /// plucked field (R9 parity with `run()`).
    #[test]
    fn run_streaming_pluck_slow_path_sort_and_distinct() {
        let src = r#"
[[items]]
id = "R1"
x = "gamma"

[[items]]
id = "R2"
x = "alpha"

[[items]]
id = "R3"
x = "alpha"

[[items]]
id = "R4"
x = "beta"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let q = Query {
            shape: OutputShape::Pluck("x".into()),
            ndjson: true,
            distinct: true,
            sort_by: vec![("x".into(), SortDir::Asc)],
            ..Default::default()
        };
        let mut buf: Vec<u8> = Vec::new();
        run_streaming(&doc, "items", &q, &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "\"alpha\"\n\"beta\"\n\"gamma\"\n"
        );
    }

    /// T3 parity: streaming and non-streaming Pluck must emit the same
    /// set of values in the same order. Proves the null/missing-drop and
    /// windowing logic is byte-aligned with `apply_pluck` + the
    /// non-streaming slow-path.
    #[test]
    fn run_streaming_pluck_matches_non_streaming_run() {
        let src = r#"
[[items]]
id = "R1"
x = "v1"

[[items]]
id = "R2"
# x missing

[[items]]
id = "R3"
x = "v3"

[[items]]
id = "R4"
x = "v4"

[[items]]
id = "R5"
x = "v5"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let q_stream = Query {
            shape: OutputShape::Pluck("x".into()),
            ndjson: true,
            limit: Some(2),
            offset: Some(1),
            sort_by: vec![("x".into(), SortDir::Desc)],
            ..Default::default()
        };
        let mut buf: Vec<u8> = Vec::new();
        run_streaming(&doc, "items", &q_stream, &mut buf).unwrap();
        let streamed_lines: Vec<&str> = std::str::from_utf8(&buf)
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .collect();
        let streamed_values: Vec<JsonValue> = streamed_lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        let q_run = Query { ndjson: false, ..q_stream.clone() };
        let non_streamed = run(&doc, "items", &q_run).unwrap();
        let non_streamed_array: Vec<JsonValue> = non_streamed
            .as_array()
            .expect("non-streaming pluck returns JSON array")
            .clone();

        assert_eq!(
            streamed_values, non_streamed_array,
            "streaming and non-streaming pluck must emit identical value sequences"
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
