//! Output helpers. Every `{"ok":true,...}` / pretty-printed-JSON /
//! bare-scalar emitter lives here so the dispatch module stays focused on
//! routing and the render contract (pretty vs compact vs raw) is defined in
//! one file.
//!
//! These helpers don't depend on any clap-derive type. They take `&JsonValue`
//! plus an optional `OutputShape` and write to stdout. That's why they're a
//! top-level sibling of `cli/` rather than scoped under it: nothing about their
//! shape says "CLI".

mod template;

use anyhow::Result;
use serde_json::Value as JsonValue;
use std::io::{BufWriter, Write};
use std::sync::OnceLock;

use crate::errors::{ErrorKind, tagged_err};
use crate::io::ScalarMutationPlan;
use crate::items::MutationPlan;
use crate::json::navigate_json;
use crate::query::{self, OutputShape, RawArrayHint, ShapeDispatch};
use template::{Template, value_text};

/// The global output options, applied by every emitter in this module.
/// `json_errors` mirrors `--error-format json`, under which stderr carries
/// only the error envelope, so the truncation notice is withheld.
#[derive(Clone, Debug, Default)]
pub(crate) struct OutputOpts {
    pub(crate) select: Option<Vec<String>>,
    pub(crate) limit: Option<usize>,
    pub(crate) lines: bool,
    pub(crate) get: Option<String>,
    pub(crate) template: Option<String>,
    pub(crate) quiet: bool,
    pub(crate) json_errors: bool,
}

static OPTS: OnceLock<OutputOpts> = OnceLock::new();

static UNCONFIGURED: OutputOpts = OutputOpts {
    select: None,
    limit: None,
    lines: false,
    get: None,
    template: None,
    quiet: false,
    json_errors: false,
};

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

impl OutputOpts {
    /// The flags other than `-q` that are set, by their CLI spelling.
    fn shaping_flags(&self) -> Vec<&'static str> {
        let mut set = Vec::new();
        if self.select.is_some() {
            set.push("--select");
        }
        if self.limit.is_some() {
            set.push("--limit");
        }
        if self.lines {
            set.push("--lines");
        }
        if self.get.is_some() {
            set.push("--get");
        }
        if self.template.is_some() {
            set.push("--template");
        }
        set
    }

    fn is_inert(&self) -> bool {
        !self.quiet && self.shaping_flags().is_empty()
    }

    /// The flag conflicts clap cannot enforce on a global typed before the
    /// subcommand, and a parse of `--template` so a malformed one fails
    /// before any command runs.
    pub(crate) fn validate(&self) -> Result<()> {
        if self.quiet
            && let Some(other) = self.shaping_flags().first()
        {
            return Err(invalid(format!("`-q` cannot be combined with `{other}`")));
        }
        if self.get.is_some() && self.select.is_some() {
            return Err(invalid("`--get` cannot be combined with `--select`"));
        }
        if self.get.is_some() && self.template.is_some() {
            return Err(invalid("`--get` cannot be combined with `--template`"));
        }
        if self.template.is_some() && self.select.is_some() {
            return Err(invalid("`--template` cannot be combined with `--select`"));
        }
        if let Some(t) = &self.template {
            Template::parse(t)?;
        }
        Ok(())
    }
}

/// Validate and install the process's output options. Call once, before any
/// output; a second call is a bug.
pub(crate) fn configure(opts: OutputOpts) -> Result<()> {
    opts.validate()?;
    let first = OPTS.set(opts).is_ok();
    debug_assert!(first, "output::configure called twice");
    Ok(())
}

/// The installed options, or the inert defaults for a caller that never
/// configured them (the library facade, unit tests).
pub(crate) fn opts() -> &'static OutputOpts {
    OPTS.get().unwrap_or(&UNCONFIGURED)
}

/// How a report is laid out: one value, or rows somewhere in it.
#[derive(Clone, Copy)]
pub(crate) enum Shape {
    One,
    Rows(Rows),
}

/// The JSON encoding used when neither `--lines` nor a text flag applies.
#[derive(Clone, Copy)]
pub(crate) enum Style {
    Pretty,
    Compact,
}

/// What an emitter writes: the stdout bytes, plus the truncation notice that
/// belongs on stderr when `--limit` cut rows from header-less text output.
#[derive(Debug, Default)]
pub(crate) struct Emitted {
    pub(crate) stdout: Vec<u8>,
    pub(crate) notice: Option<String>,
}

fn row_slot(report: &mut JsonValue, shape: Shape) -> Option<&mut Vec<JsonValue>> {
    match shape {
        Shape::One => None,
        Shape::Rows(Rows::Top) => report.as_array_mut(),
        Shape::Rows(Rows::Field(key)) => report.as_object_mut()?.get_mut(key)?.as_array_mut(),
    }
}

/// Keep only `paths` of `row`, each keyed by its path string; a path the row
/// lacks is left out.
pub(crate) fn project(row: &JsonValue, paths: &[String]) -> JsonValue {
    let mut out = serde_json::Map::new();
    for p in paths {
        if let Some(v) = navigate_json(row, p) {
            out.insert(p.clone(), v.clone());
        }
    }
    JsonValue::Object(out)
}

/// Refuse a path that no row carries, naming the top-level keys that do
/// exist. An empty row set validates nothing.
pub(crate) fn validate_paths<'a>(
    rows: &[JsonValue],
    paths: impl IntoIterator<Item = &'a str>,
    flag: &str,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    for p in paths {
        if rows.iter().any(|r| navigate_json(r, p).is_some()) {
            continue;
        }
        let mut keys: Vec<&str> = Vec::new();
        for r in rows {
            if let Some(obj) = r.as_object() {
                for k in obj.keys() {
                    if !keys.contains(&k.as_str()) {
                        keys.push(k);
                    }
                }
            }
        }
        let available = if keys.is_empty() {
            "(none)".to_string()
        } else {
            keys.join(", ")
        };
        return Err(invalid(format!(
            "{flag} path `{p}` matches no field; available fields: {available}"
        )));
    }
    Ok(())
}

fn push_line(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
    out.push(b'\n');
}

fn push_json(out: &mut Vec<u8>, v: &JsonValue, style: Style) -> Result<()> {
    match style {
        Style::Pretty => serde_json::to_writer_pretty(&mut *out, v)?,
        Style::Compact => serde_json::to_writer(&mut *out, v)?,
    }
    out.push(b'\n');
    Ok(())
}

/// Apply `opts` to one report. Order: `-q`, `--limit`, `--select`, then
/// `--get` / `--template`, then `--lines` or `style`. Paths are validated
/// against every row before `--limit` cuts any.
pub(crate) fn emit(
    mut report: JsonValue,
    shape: Shape,
    style: Style,
    opts: &OutputOpts,
) -> Result<Emitted> {
    if opts.quiet {
        return Ok(Emitted::default());
    }
    let template = opts.template.as_deref().map(Template::parse).transpose()?;
    let rows_shape = match shape {
        Shape::Rows(r) if row_slot(&mut report, shape).is_some() => Some(r),
        _ => None,
    };
    let shape = rows_shape.map_or(Shape::One, Shape::Rows);

    {
        let rows: &[JsonValue] = match row_slot(&mut report, shape) {
            Some(rows) => rows,
            None => std::slice::from_ref(&report),
        };
        if let Some(paths) = &opts.select {
            validate_paths(rows, paths.iter().map(String::as_str), "--select")?;
        }
        if let Some(path) = &opts.get {
            validate_paths(rows, [path.as_str()], "--get")?;
        }
        if let Some(t) = &template {
            validate_paths(rows, t.paths(), "--template")?;
        }
    }

    let mut limited = None;
    if let Some(n) = opts.limit {
        let Some(rows) = row_slot(&mut report, shape) else {
            return Err(invalid("`--limit` applies to row reports"));
        };
        if rows.len() > n {
            limited = Some((n, rows.len()));
            rows.truncate(n);
        }
    }

    if let Some(paths) = &opts.select {
        match row_slot(&mut report, shape) {
            Some(rows) => rows.iter_mut().for_each(|r| *r = project(r, paths)),
            None => report = project(&report, paths),
        }
    }

    let notice = limited.map(|(n, total)| format!("tomlctl: showing {n} of {total} rows"));
    let mut out = Vec::new();
    if opts.get.is_some() || template.is_some() {
        let render = |v: &JsonValue| match (&opts.get, &template) {
            (Some(path), _) => navigate_json(v, path).map(value_text),
            (None, Some(t)) => Some(t.render(v)),
            (None, None) => None,
        };
        match row_slot(&mut report, shape) {
            Some(rows) => rows
                .iter()
                .filter_map(&render)
                .for_each(|l| push_line(&mut out, &l)),
            None => match (
                &opts.get,
                navigate_json(&report, opts.get.as_deref().unwrap_or("")),
            ) {
                (Some(_), Some(JsonValue::Array(items))) => items
                    .iter()
                    .for_each(|v| push_line(&mut out, &value_text(v))),
                _ => {
                    if let Some(l) = render(&report) {
                        push_line(&mut out, &l);
                    }
                }
            },
        }
        return Ok(Emitted {
            stdout: out,
            notice,
        });
    }

    let limited_json =
        limited.map(|(shown, total)| serde_json::json!({ "shown": shown, "total": total }));
    match (shape, limited_json) {
        (Shape::Rows(Rows::Field(_)), Some(l)) => {
            if let Some(obj) = report.as_object_mut() {
                obj.insert("limited".to_string(), l);
            }
        }
        (Shape::Rows(Rows::Top), Some(l)) if !opts.lines => {
            report = serde_json::json!({ "rows": report, "limited": l });
        }
        (Shape::Rows(Rows::Top), Some(l)) => {
            push_json(
                &mut out,
                &serde_json::json!({ "limited": l }),
                Style::Compact,
            )?;
        }
        _ => {}
    }

    if opts.lines {
        let lines = match shape {
            Shape::Rows(rows) => report_lines(report, rows),
            Shape::One => vec![report],
        };
        for line in &lines {
            push_json(&mut out, line, Style::Compact)?;
        }
    } else {
        push_json(&mut out, &report, style)?;
    }
    Ok(Emitted {
        stdout: out,
        notice: None,
    })
}

fn write_stdout(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    out.write_all(bytes)?;
    out.flush()?;
    Ok(())
}

/// Write `emitted`; the notice goes straight to stderr rather than through
/// `io::advise!`, which stays silent when stderr is not a terminal.
fn write_emitted(emitted: Emitted, opts: &OutputOpts) -> Result<()> {
    write_stdout(&emitted.stdout)?;
    if let Some(notice) = emitted.notice
        && !opts.json_errors
    {
        eprintln!("{notice}");
    }
    Ok(())
}

fn write_json(v: &JsonValue, style: Style) -> Result<()> {
    let mut out = Vec::new();
    push_json(&mut out, v, style)?;
    write_stdout(&out)
}

pub(crate) fn print_json(v: &JsonValue) -> Result<()> {
    let o = opts();
    if o.is_inert() {
        return write_json(v, Style::Pretty);
    }
    write_emitted(emit(v.clone(), Shape::One, Style::Pretty, o)?, o)
}

/// `print_json_compact`'s core. A shaping failure yields the unshaped value
/// and a warning instead of an error: this emitter carries write envelopes,
/// and an exit of 1 after a landed write invites a retry that applies it
/// twice.
fn compact_lenient(v: &JsonValue, opts: &OutputOpts) -> Result<(Emitted, Option<String>)> {
    match emit(v.clone(), Shape::One, Style::Compact, opts) {
        Ok(emitted) => Ok((emitted, None)),
        Err(err) => {
            let mut stdout = Vec::new();
            push_json(&mut stdout, v, Style::Compact)?;
            let warning = format!("tomlctl: warning: {err:#}; printed the unshaped output");
            Ok((
                Emitted {
                    stdout,
                    notice: None,
                },
                Some(warning),
            ))
        }
    }
}

/// Run the query engine's streaming writer against stdout, or against a sink
/// under `-q` so its errors still decide the exit code. Under `--get` or
/// `--template` the caller takes the non-streaming path through
/// `print_query` instead.
pub(crate) fn stdout_stream(f: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
    if opts().quiet {
        return f(&mut std::io::sink());
    }
    let stdout = std::io::stdout();
    let mut h = stdout.lock();
    f(&mut h)?;
    h.flush()?;
    Ok(())
}

/// Whether the output options leave a query list free to stream.
pub(crate) fn streaming_allowed() -> bool {
    let o = opts();
    o.get.is_none() && o.template.is_none()
}

/// The options that still apply to a query list's output once its engine
/// has consumed `--select`, `--limit` and `--lines`.
fn query_opts(opts: &OutputOpts) -> OutputOpts {
    OutputOpts {
        get: opts.get.clone(),
        template: opts.template.clone(),
        quiet: opts.quiet,
        json_errors: opts.json_errors,
        ..OutputOpts::default()
    }
}

/// Emit a query list's engine output: rows when it is an array, one value
/// otherwise.
pub(crate) fn print_query(out: JsonValue) -> Result<()> {
    let o = query_opts(opts());
    let shape = if out.is_array() {
        Shape::Rows(Rows::Top)
    } else {
        Shape::One
    };
    let emitted = emit(out, shape, Style::Pretty, &o)?;
    write_emitted(emitted, &o)
}

/// Whether text output may be written: `false` under `-q`, an error for any
/// flag in `refused`.
fn text_allowed(opts: &OutputOpts, refused: &[&str]) -> Result<bool> {
    if opts.quiet {
        return Ok(false);
    }
    if let Some(flag) = opts
        .shaping_flags()
        .into_iter()
        .find(|f| refused.contains(f))
    {
        return Err(invalid(format!(
            "`{flag}` does not apply to this command's text output"
        )));
    }
    Ok(true)
}

const ALL_SHAPING: &[&str] = &["--select", "--limit", "--lines", "--get", "--template"];

/// Write non-JSON output verbatim; the caller supplies any trailing newline.
pub(crate) fn print_text(text: &str) -> Result<()> {
    if text_allowed(opts(), ALL_SHAPING)? {
        write_stdout(text.as_bytes())?;
    }
    Ok(())
}

/// Build the `--dry-run` JSON envelope for `items remove --dry-run`
/// and `items apply --dry-run`. Kept separate from `emit_dry_run_plan` so the
/// envelope shape is testable without capturing stdout, mirroring the
/// scalar-side `build_dry_run_scalar_envelope` split. The shape is
/// a single compact JSON object:
///
/// ```text
/// {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":N,"updated":N,"removed":N,"skipped":N,"ids":[...]},"...": "..."}
/// ```
///
/// `ids` is the concatenation `[...added, ...updated, ...removed]` in
/// that order, matching `MutationPlan::union_ids`. `N` values are plain
/// integer counts (not arrays) so the output stays stable and terse
/// across both dispatch arms. `skipped` surfaces the dedupe-skipped row
/// count from `MutationPlan.skipped`.
///
/// The `kind` discriminator — placed first inside `would_change` — lets
/// consumers branch on `would_change.kind` rather than on which
/// subcommand they invoked. Items-shape envelopes carry `kind:"items"`;
/// scalar-shape envelopes (built by `build_dry_run_scalar_envelope`)
/// carry `kind:"scalar"`. It is additive to the rest of the envelope:
/// consumers reading `added`/`updated`/`removed`/`skipped`/`ids` are
/// unaffected by it.
/// The common keys above are stable; commands may add further top-level fields.
pub(crate) fn build_dry_run_plan_envelope(plan: &MutationPlan) -> JsonValue {
    serde_json::json!({
        "ok": true,
        "dry_run": true,
        "would_change": {
            "kind": "items",
            "added": plan.added.len(),
            "updated": plan.updated.len(),
            "removed": plan.removed.len(),
            "skipped": plan.skipped.len(),
            "ids": plan.union_ids(),
        },
    })
}

/// Emit the `--dry-run` summary for `items remove --dry-run` and
/// `items apply --dry-run`. Thin I/O wrapper over
/// `build_dry_run_plan_envelope`; companion to `emit_dry_run_scalar`.
/// Both dry-run dispatch arms funnel through `print_json_compact` so the
/// compact-line format is byte-stable across every dry-run emitter.
pub(crate) fn emit_dry_run_plan(plan: &MutationPlan) -> Result<()> {
    let envelope = build_dry_run_plan_envelope(plan);
    print_json_compact(&envelope)
}

/// Compact single-line sibling of `print_json`, used for the `{"ok":true,...}`
/// terminal status lines emitted by write-path dispatch arms. Keeping
/// this separate from `print_json` preserves the pretty-printed contract that
/// downstream consumers rely on for read-path output (tests + humans) while
/// letting every OK-status emitter funnel through a single helper rather
/// than hand-constructing JSON strings. Integration tests assert on these
/// compact bytes with `.contains(r#"{"ok":true,"added":N}"#)`, so the
/// single-line form is load-bearing. A shaping failure prints the unshaped
/// value with a stderr warning and returns `Ok` (see `compact_lenient`).
pub(crate) fn print_json_compact(v: &JsonValue) -> Result<()> {
    let o = opts();
    if o.is_inert() {
        return write_json(v, Style::Compact);
    }
    let (emitted, warning) = compact_lenient(v, o)?;
    if let Some(w) = warning {
        eprintln!("{w}");
    }
    write_emitted(emitted, o)
}

/// Where a report's rows sit, which is what `--lines` splits on.
#[derive(Clone, Copy)]
pub(crate) enum Rows {
    /// The report is the row array itself.
    Top,
    /// The rows are this field of the report object.
    Field(&'static str),
}

/// The `--lines` encoding of a report: under `Rows::Field`, the other fields
/// as one header line first (so a truncated read keeps the totals and
/// hazards), omitted when there are none; then one line per row. A report
/// that does not have the declared shape is one line.
pub(crate) fn report_lines(report: JsonValue, rows: Rows) -> Vec<JsonValue> {
    match (rows, report) {
        (Rows::Top, JsonValue::Array(rows)) => rows,
        (Rows::Field(key), JsonValue::Object(mut header))
            if header.get(key).is_some_and(JsonValue::is_array) =>
        {
            let rows = match header.shift_remove(key) {
                Some(JsonValue::Array(rows)) => rows,
                _ => Vec::new(),
            };
            let mut lines = Vec::with_capacity(rows.len() + 1);
            if !header.is_empty() {
                lines.push(JsonValue::Object(header));
            }
            lines.extend(rows);
            lines
        }
        (_, report) => vec![report],
    }
}

/// Emit a read verb's report: pretty JSON, or under `--lines` the
/// `report_lines` encoding.
pub(crate) fn print_report(report: JsonValue, rows: Rows) -> Result<()> {
    let o = opts();
    write_emitted(emit(report, Shape::Rows(rows), Style::Pretty, o)?, o)
}

/// Emit one bare-scalar value to stdout, followed by exactly one
/// trailing newline. The trailing `\n` is deliberate — bash `read -r N`
/// consumes up to a newline, so agents piping tomlctl output into
/// variable-binding shell loops expect every bare-value emission to end
/// in one. For the `--lines --raw --pluck` streaming path this helper is
/// NOT called per line (that path uses `query::emit_raw` directly into a
/// pre-locked writer for throughput); the semantics are the same.
///
/// The scalar-rendering rules live in `query::emit_raw` — this
/// helper is the I/O wrapper that adds stdout locking, buffering, and
/// the trailing newline. Keeping `emit_raw` in `query` keeps the module
/// layering honest (cli depends on query, not the reverse). `hint` picks the
/// remedy an array target's error advises; nothing is written on error.
pub(crate) fn print_raw_value(v: &JsonValue, hint: RawArrayHint) -> Result<()> {
    let print = text_allowed(opts(), ALL_SHAPING)?;
    let rendered = query::emit_raw(v, hint)?;
    if !print {
        return Ok(());
    }
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    out.write_all(rendered.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// `items list --raw` dispatch-side wrapper. The per-shape
/// render logic lives in `ShapeDispatch::raw_emit` on `OutputShape`, so
/// adding a new shape variant forces one edit there rather than here PLUS
/// a second match on `shape` in this file. This function's only job
/// is the stdout lock + buffered write + trailing newline — the same
/// I/O discipline `print_raw_value` applies to a single scalar, but
/// called once with the shape-rendered bytes.
///
/// Called only when `q.raw` is set AND the caller did NOT take the
/// streaming path (which handles its own emission inline).
///
/// Error strings are load-bearing: the pluck N==0 and N>1 errors, and
/// the count-by / group-by errors, appear byte-for-byte in integration
/// tests. Those strings are pinned inside `ShapeDispatch::raw_emit` —
/// see the trait impl in `query.rs`.
pub(crate) fn emit_list_raw(v: &JsonValue, shape: &OutputShape) -> Result<()> {
    let print = text_allowed(opts(), &["--get", "--template"])?;
    let rendered = shape.raw_emit(v)?;
    if !print {
        return Ok(());
    }
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    out.write_all(rendered.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// Build the dry-run JSON envelope for a single-scalar mutation (`set` /
/// `set-json` `--dry-run`). Extracted from `emit_dry_run_scalar` so the
/// envelope shape is testable without capturing stdout. The shape is:
///
/// ```json
/// {"ok":true,"dry_run":true,"would_change":{"kind":"scalar","path":"<p>","old":<json|null>,"new":<json>}}
/// ```
///
/// `old_value: None` (auto-vivify case) renders as `"old": null`.
///
/// The `kind` discriminator — placed first inside `would_change` — lets
/// consumers branch on `would_change.kind` rather than on which
/// subcommand they invoked. Scalar-shape envelopes carry `kind:"scalar"`;
/// items-shape envelopes (built by `emit_dry_run_plan`) carry
/// `kind:"items"`. It is additive to the rest of the envelope: consumers
/// reading `path`/`old`/`new` are unaffected by it.
pub(crate) fn build_dry_run_scalar_envelope(plan: &ScalarMutationPlan) -> JsonValue {
    serde_json::json!({
        "ok": true,
        "dry_run": true,
        "would_change": {
            "kind": "scalar",
            "path": plan.path,
            "old": plan.old_value.clone().unwrap_or(JsonValue::Null),
            "new": plan.new_value,
        },
    })
}

/// Emit the `--dry-run` envelope for `set` / `set-json`. Companion to
/// `emit_dry_run_plan` (`items remove` / `items apply`); the two share
/// `print_json_compact` so the compact-line format is byte-stable across
/// every dry-run dispatch arm.
pub(crate) fn emit_dry_run_scalar(plan: &ScalarMutationPlan) -> Result<()> {
    let envelope = build_dry_run_scalar_envelope(plan);
    print_json_compact(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with(f: impl FnOnce(&mut OutputOpts)) -> OutputOpts {
        let mut o = OutputOpts::default();
        f(&mut o);
        o
    }

    fn out(report: JsonValue, shape: Shape, style: Style, o: &OutputOpts) -> String {
        String::from_utf8(emit(report, shape, style, o).unwrap().stdout).unwrap()
    }

    fn err(report: JsonValue, shape: Shape, o: &OutputOpts) -> String {
        format!("{:#}", emit(report, shape, Style::Pretty, o).unwrap_err())
    }

    fn rows3() -> JsonValue {
        json!([
            { "id": 1, "ref": "a", "deps": [2] },
            { "id": 2, "ref": "b" },
            { "id": 3, "ref": "c", "policy": { "mode": "m" } }
        ])
    }

    const TOP: Shape = Shape::Rows(Rows::Top);
    const FIELD: Shape = Shape::Rows(Rows::Field("items"));

    #[test]
    fn inert_options_keep_the_default_bytes() {
        let v = json!({ "a": [1, 2], "b": "x" });
        let o = OutputOpts::default();
        assert_eq!(
            out(v.clone(), Shape::One, Style::Pretty, &o),
            format!("{}\n", serde_json::to_string_pretty(&v).unwrap())
        );
        assert_eq!(
            out(v.clone(), Shape::One, Style::Compact, &o),
            format!("{}\n", serde_json::to_string(&v).unwrap())
        );
        assert_eq!(
            out(rows3(), TOP, Style::Pretty, &o),
            format!("{}\n", serde_json::to_string_pretty(&rows3()).unwrap())
        );
        assert!(opts().is_inert());
    }

    #[test]
    fn limit_applies_before_select_and_select_before_get() {
        let o = with(|o| {
            o.limit = Some(2);
            o.select = Some(vec!["ref".into()]);
            o.lines = true;
        });
        assert_eq!(
            out(rows3(), TOP, Style::Pretty, &o),
            "{\"limited\":{\"shown\":2,\"total\":3}}\n{\"ref\":\"a\"}\n{\"ref\":\"b\"}\n"
        );
        let o = with(|o| {
            o.limit = Some(1);
            o.get = Some("ref".into());
        });
        let e = emit(rows3(), TOP, Style::Pretty, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap(), "a\n");
        assert_eq!(e.notice.as_deref(), Some("tomlctl: showing 1 of 3 rows"));
    }

    #[test]
    fn select_paths_are_validated_before_limit_cuts_rows() {
        let o = with(|o| {
            o.limit = Some(1);
            o.select = Some(vec!["policy.mode".into()]);
        });
        let s = out(rows3(), TOP, Style::Compact, &o);
        assert_eq!(s, "{\"rows\":[{}],\"limited\":{\"shown\":1,\"total\":3}}\n");
    }

    #[test]
    fn limited_header_on_a_field_report() {
        let report = json!({ "total": 3, "items": rows3() });
        let o = with(|o| {
            o.limit = Some(2);
            o.lines = true;
            o.select = Some(vec!["id".into()]);
        });
        assert_eq!(
            out(report.clone(), FIELD, Style::Pretty, &o),
            "{\"total\":3,\"limited\":{\"shown\":2,\"total\":3}}\n{\"id\":1}\n{\"id\":2}\n"
        );
        let o = with(|o| o.limit = Some(1));
        let pretty: JsonValue =
            serde_json::from_str(&out(report, FIELD, Style::Pretty, &o)).unwrap();
        assert_eq!(pretty["limited"], json!({ "shown": 1, "total": 3 }));
        assert_eq!(pretty["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn limited_wraps_a_bare_array_in_pretty_mode() {
        let o = with(|o| o.limit = Some(2));
        let v: JsonValue = serde_json::from_str(&out(rows3(), TOP, Style::Pretty, &o)).unwrap();
        assert_eq!(v["limited"], json!({ "shown": 2, "total": 3 }));
        assert_eq!(v["rows"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_limit_that_cuts_nothing_adds_no_header() {
        let o = with(|o| {
            o.limit = Some(3);
            o.lines = true;
        });
        let e = emit(rows3(), TOP, Style::Pretty, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap().lines().count(), 3);
        assert_eq!(e.notice, None);
    }

    #[test]
    fn template_rows_carry_the_truncation_notice() {
        let o = with(|o| {
            o.limit = Some(2);
            o.template = Some("{id}={ref}".into());
        });
        let e = emit(rows3(), TOP, Style::Pretty, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap(), "1=a\n2=b\n");
        assert_eq!(e.notice.as_deref(), Some("tomlctl: showing 2 of 3 rows"));
    }

    #[test]
    fn limit_on_a_single_report_is_refused() {
        let o = with(|o| o.limit = Some(1));
        assert_eq!(
            err(json!({ "a": 1 }), Shape::One, &o),
            "`--limit` applies to row reports"
        );
        assert_eq!(
            err(json!({ "a": 1 }), FIELD, &o),
            "`--limit` applies to row reports"
        );
    }

    #[test]
    fn select_keys_dotted_paths_by_the_path_string() {
        let o = with(|o| o.select = Some(vec!["policy.mode".into(), "deps.0".into()]));
        let v = json!({ "policy": { "mode": "m" }, "deps": [7], "x": 1 });
        assert_eq!(
            out(v, Shape::One, Style::Compact, &o),
            "{\"policy.mode\":\"m\",\"deps.0\":7}\n"
        );
    }

    #[test]
    fn select_omits_a_key_missing_from_some_rows() {
        let o = with(|o| {
            o.select = Some(vec!["id".into(), "deps".into()]);
            o.lines = true;
        });
        assert_eq!(
            out(rows3(), TOP, Style::Pretty, &o),
            "{\"id\":1,\"deps\":[2]}\n{\"id\":2}\n{\"id\":3}\n"
        );
    }

    #[test]
    fn an_unknown_path_lists_the_available_fields() {
        let o = with(|o| o.select = Some(vec!["nope".into()]));
        assert_eq!(
            err(rows3(), TOP, &o),
            "--select path `nope` matches no field; available fields: id, ref, deps, policy"
        );
        let o = with(|o| o.get = Some("nope".into()));
        assert!(err(json!({ "a": 1 }), Shape::One, &o).contains("available fields: a"));
        let o = with(|o| o.template = Some("{nope}".into()));
        assert!(err(rows3(), TOP, &o).starts_with("--template path `nope`"));
        let tag = emit(rows3(), TOP, Style::Pretty, &o).unwrap_err();
        let tag = tag.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tag.kind.as_str(), "validation");
    }

    #[test]
    fn an_empty_row_set_prints_its_empty_result() {
        for o in [
            with(|o| o.select = Some(vec!["class".into()])),
            with(|o| o.get = Some("class".into())),
            with(|o| o.template = Some("{class}".into())),
        ] {
            let e = emit(json!([]), TOP, Style::Pretty, &o).unwrap();
            let s = String::from_utf8(e.stdout).unwrap();
            assert!(s.is_empty() || s == "[]\n", "{s:?}");
            let report = json!({ "ok": true, "items": [] });
            emit(report, FIELD, Style::Pretty, &o).unwrap();
        }
    }

    #[test]
    fn get_spreads_an_array_on_a_single_report() {
        let o = with(|o| o.get = Some("files".into()));
        let v = json!({ "files": ["a.rs", "b.rs"], "n": 2 });
        assert_eq!(out(v, Shape::One, Style::Pretty, &o), "a.rs\nb.rs\n");
        let o = with(|o| o.get = Some("ref".into()));
        assert_eq!(
            out(json!({ "ref": "x" }), Shape::One, Style::Pretty, &o),
            "x\n"
        );
    }

    #[test]
    fn get_prints_one_line_per_row() {
        let o = with(|o| o.get = Some("deps".into()));
        assert_eq!(out(rows3(), TOP, Style::Pretty, &o), "[2]\n");
        let o = with(|o| o.get = Some("id".into()));
        let report = json!({ "total": 3, "items": rows3() });
        assert_eq!(out(report, FIELD, Style::Pretty, &o), "1\n2\n3\n");
    }

    #[test]
    fn lines_on_a_single_report_is_one_compact_line() {
        let o = with(|o| o.lines = true);
        assert_eq!(
            out(json!({ "a": [1, 2] }), Shape::One, Style::Pretty, &o),
            "{\"a\":[1,2]}\n"
        );
    }

    #[test]
    fn quiet_prints_nothing() {
        let o = with(|o| o.quiet = true);
        let e = emit(rows3(), TOP, Style::Pretty, &o).unwrap();
        assert!(e.stdout.is_empty() && e.notice.is_none());
        assert!(!text_allowed(&o, ALL_SHAPING).unwrap());
    }

    #[test]
    fn quiet_refuses_every_other_flag() {
        type Setter = fn(&mut OutputOpts);
        let cases: [(Setter, &str); 5] = [
            (|o| o.select = Some(vec!["a".into()]), "--select"),
            (|o| o.limit = Some(1), "--limit"),
            (|o| o.lines = true, "--lines"),
            (|o| o.get = Some("a".into()), "--get"),
            (|o| o.template = Some("{a}".into()), "--template"),
        ];
        for (set, flag) in cases {
            let o = with(|o| {
                o.quiet = true;
                set(o);
            });
            let e = format!("{:#}", o.validate().unwrap_err());
            assert_eq!(e, format!("`-q` cannot be combined with `{flag}`"));
        }
    }

    #[test]
    fn get_select_and_template_conflict_pairwise() {
        let both = |f: fn(&mut OutputOpts)| format!("{:#}", with(f).validate().unwrap_err());
        assert_eq!(
            both(|o| {
                o.get = Some("a".into());
                o.select = Some(vec!["a".into()]);
            }),
            "`--get` cannot be combined with `--select`"
        );
        assert_eq!(
            both(|o| {
                o.get = Some("a".into());
                o.template = Some("{a}".into());
            }),
            "`--get` cannot be combined with `--template`"
        );
        assert_eq!(
            both(|o| {
                o.template = Some("{a}".into());
                o.select = Some(vec!["a".into()]);
            }),
            "`--template` cannot be combined with `--select`"
        );
        assert!(with(|o| o.template = Some("{a".into())).validate().is_err());
        assert!(
            with(|o| {
                o.limit = Some(2);
                o.lines = true;
                o.get = Some("a".into());
            })
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn compact_shaping_failure_prints_the_unshaped_value() {
        let v = json!({ "ok": true, "added": 1 });
        let o = with(|o| o.get = Some("missing".into()));
        let (e, warning) = compact_lenient(&v, &o).unwrap();
        assert_eq!(
            String::from_utf8(e.stdout).unwrap(),
            "{\"ok\":true,\"added\":1}\n"
        );
        assert!(warning.unwrap().contains("--get path `missing`"));
        let o = with(|o| o.limit = Some(1));
        let (_, warning) = compact_lenient(&v, &o).unwrap();
        assert!(
            warning
                .unwrap()
                .contains("`--limit` applies to row reports")
        );
        let o = with(|o| o.get = Some("added".into()));
        let (e, warning) = compact_lenient(&v, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap(), "1\n");
        assert!(warning.is_none());
    }

    #[test]
    fn text_output_refuses_shaping_flags() {
        let o = with(|o| o.lines = true);
        let e = text_allowed(&o, ALL_SHAPING).unwrap_err();
        assert_eq!(
            format!("{e:#}"),
            "`--lines` does not apply to this command's text output"
        );
        assert!(text_allowed(&o, &["--get", "--template"]).unwrap());
        let o = with(|o| o.get = Some("a".into()));
        assert!(text_allowed(&o, &["--get", "--template"]).is_err());
    }

    #[test]
    fn query_output_ignores_engine_consumed_flags() {
        let o = query_opts(&with(|o| {
            o.select = Some(vec!["x".into()]);
            o.limit = Some(1);
            o.lines = true;
            o.get = Some("id".into());
        }));
        assert_eq!(out(rows3(), TOP, Style::Pretty, &o), "1\n2\n3\n");
    }

    #[test]
    fn report_lines_lead_with_the_header_then_one_row_per_line() {
        let report = json!({ "rows": [{ "a": 1 }, { "a": 2 }], "total": 2, "ok": true });
        assert_eq!(
            report_lines(report, Rows::Field("rows")),
            [
                json!({ "total": 2, "ok": true }),
                json!({ "a": 1 }),
                json!({ "a": 2 })
            ]
        );
    }

    #[test]
    fn report_lines_omit_an_empty_header_and_split_a_bare_array() {
        assert_eq!(
            report_lines(json!({ "rows": [[1, 2], [3]] }), Rows::Field("rows")),
            [json!([1, 2]), json!([3])]
        );
        assert_eq!(
            report_lines(json!([{ "a": 1 }, { "a": 2 }]), Rows::Top),
            [json!({ "a": 1 }), json!({ "a": 2 })]
        );
        assert_eq!(report_lines(json!([]), Rows::Top), [] as [JsonValue; 0]);
    }

    #[test]
    fn report_lines_keep_an_off_shape_report_whole() {
        let report = json!({ "rows": null, "total": 0 });
        assert_eq!(report_lines(report.clone(), Rows::Field("rows")), [report]);
        assert_eq!(
            report_lines(json!({ "a": 1 }), Rows::Top),
            [json!({ "a": 1 })]
        );
    }

    #[test]
    fn emit_dry_run_scalar_envelope_shape() {
        let plan = ScalarMutationPlan {
            path: "foo.bar".to_string(),
            old_value: Some(serde_json::json!("old")),
            new_value: serde_json::json!("new"),
        };
        let env = build_dry_run_scalar_envelope(&plan);
        // Compact-serialised form must match the documented envelope byte
        // for byte. `serde_json` is built with `preserve_order` (Cargo.toml),
        // so insertion order in the `json!` macro is the on-wire order.
        // `kind` is the first field inside `would_change`.
        let s = serde_json::to_string(&env).unwrap();
        assert_eq!(
            s,
            r#"{"ok":true,"dry_run":true,"would_change":{"kind":"scalar","path":"foo.bar","old":"old","new":"new"}}"#
        );
        // Pin the discriminator independent of byte-for-byte order, so a
        // future cosmetic key reorder still leaves the contract intact.
        assert_eq!(env["would_change"]["kind"], serde_json::json!("scalar"));
    }

    #[test]
    fn emit_dry_run_scalar_envelope_renders_missing_old_as_null() {
        let plan = ScalarMutationPlan {
            path: "foo.absent".to_string(),
            old_value: None,
            new_value: serde_json::json!(42),
        };
        let env = build_dry_run_scalar_envelope(&plan);
        let s = serde_json::to_string(&env).unwrap();
        assert_eq!(
            s,
            r#"{"ok":true,"dry_run":true,"would_change":{"kind":"scalar","path":"foo.absent","old":null,"new":42}}"#
        );
        assert_eq!(env["would_change"]["kind"], serde_json::json!("scalar"));
    }

    /// Shape test for the items-side dry-run envelope, mirroring the
    /// scalar-side test above. Pins the on-wire byte order (insertion
    /// order is preserved by serde_json's `preserve_order` feature in
    /// Cargo.toml) and the `kind:"items"` discriminator.
    #[test]
    fn build_dry_run_plan_envelope_shape() {
        let plan = MutationPlan {
            // The envelope builder reads only `added`/`updated`/`removed`/
            // `skipped`/`union_ids()`; `new_doc` content is irrelevant to
            // this shape test, so any cheap `toml::Value` constructs it.
            new_doc: toml::Value::Integer(0),
            added: vec!["A1".to_string()],
            updated: vec!["U1".to_string(), "U2".to_string()],
            removed: vec!["R1".to_string()],
            skipped: vec![],
        };
        let env = build_dry_run_plan_envelope(&plan);
        let s = serde_json::to_string(&env).unwrap();
        assert_eq!(
            s,
            r#"{"ok":true,"dry_run":true,"would_change":{"kind":"items","added":1,"updated":2,"removed":1,"skipped":0,"ids":["A1","U1","U2","R1"]}}"#
        );
        // Pin the discriminator independent of byte-for-byte order, so a
        // future cosmetic key reorder still leaves the contract intact.
        assert_eq!(env["would_change"]["kind"], serde_json::json!("items"));
    }

    #[test]
    fn dry_run_plan_envelope_counts_id_less_rows_without_listing_them() {
        let plan = MutationPlan {
            new_doc: toml::Value::Integer(0),
            added: vec![String::new(), String::new()],
            updated: vec![],
            removed: vec![],
            skipped: vec![],
        };
        let env = build_dry_run_plan_envelope(&plan);
        assert_eq!(env["would_change"]["added"], serde_json::json!(2));
        assert_eq!(env["would_change"]["ids"], serde_json::json!([]));
    }
}
