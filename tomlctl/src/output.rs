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

use crate::convert::{navigate_json, project, validate_paths};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::ScalarMutationPlan;
use crate::items::MutationPlan;
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
    pub(crate) rows: Option<String>,
    pub(crate) header: bool,
    pub(crate) max_chars: Option<usize>,
    pub(crate) omit: Option<Vec<String>>,
    pub(crate) filters: WhereFilters,
}

/// The raw values of the global `--where*` flags, one field per flag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WhereFilters {
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
}

/// The `--where*` long names, in `WhereFilters::families` order.
const WHERE_FLAGS: [&str; 13] = [
    "--where",
    "--where-not",
    "--where-in",
    "--where-has",
    "--where-missing",
    "--where-gt",
    "--where-gte",
    "--where-lt",
    "--where-lte",
    "--where-contains",
    "--where-prefix",
    "--where-suffix",
    "--where-regex",
];

impl WhereFilters {
    const EMPTY: WhereFilters = WhereFilters {
        where_eq: Vec::new(),
        where_not: Vec::new(),
        where_in: Vec::new(),
        where_has: Vec::new(),
        where_missing: Vec::new(),
        where_gt: Vec::new(),
        where_gte: Vec::new(),
        where_lt: Vec::new(),
        where_lte: Vec::new(),
        where_contains: Vec::new(),
        where_prefix: Vec::new(),
        where_suffix: Vec::new(),
        where_regex: Vec::new(),
    };

    /// Each flag's long name beside its values.
    fn families(&self) -> [(&'static str, &[String]); 13] {
        let f = WHERE_FLAGS;
        [
            (f[0], &self.where_eq[..]),
            (f[1], &self.where_not[..]),
            (f[2], &self.where_in[..]),
            (f[3], &self.where_has[..]),
            (f[4], &self.where_missing[..]),
            (f[5], &self.where_gt[..]),
            (f[6], &self.where_gte[..]),
            (f[7], &self.where_lt[..]),
            (f[8], &self.where_lte[..]),
            (f[9], &self.where_contains[..]),
            (f[10], &self.where_prefix[..]),
            (f[11], &self.where_suffix[..]),
            (f[12], &self.where_regex[..]),
        ]
    }

    /// Total values across every flag.
    pub(crate) fn len(&self) -> usize {
        self.families().iter().map(|(_, v)| v.len()).sum()
    }
}

/// Refuse `--where*` values given on both sides of the subcommand. For a
/// repeatable global clap keeps only the values after the subcommand, so
/// fewer parsed values than `--where*` tokens in `argv` means some were
/// dropped. Tokens after a bare `--` are positionals and not counted.
pub(crate) fn check_where_placement<I>(argv: I, parsed: &WhereFilters) -> Result<()>
where
    I: IntoIterator,
    I::Item: AsRef<std::ffi::OsStr>,
{
    let mut tokens = 0usize;
    for arg in argv {
        let Some(arg) = arg.as_ref().to_str() else {
            continue;
        };
        if arg == "--" {
            break;
        }
        let name = arg.split_once('=').map_or(arg, |(name, _)| name);
        if WHERE_FLAGS.contains(&name) {
            tokens += 1;
        }
    }
    if parsed.len() < tokens {
        return Err(invalid(
            "give every `--where*` on one side of the subcommand: values before it are dropped when more follow it",
        ));
    }
    Ok(())
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
    rows: None,
    header: false,
    max_chars: None,
    omit: None,
    filters: WhereFilters::EMPTY,
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
        if self.rows.is_some() {
            set.push("--rows");
        }
        if self.header {
            set.push("--header");
        }
        if self.max_chars.is_some() {
            set.push("--max-chars");
        }
        if self.omit.is_some() {
            set.push("--omit");
        }
        set.extend(
            self.filters
                .families()
                .into_iter()
                .filter(|(_, values)| !values.is_empty())
                .map(|(flag, _)| flag),
        );
        set
    }

    fn is_inert(&self) -> bool {
        !self.quiet && self.shaping_flags().is_empty()
    }

    /// The flag conflicts clap cannot enforce on a global typed before the
    /// subcommand, and a parse of `--template` so a malformed one fails
    /// before any command runs.
    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(paths) = &self.select {
            if paths.is_empty() {
                return Err(invalid("--select: empty path list"));
            }
            if let Some(i) = paths.iter().position(String::is_empty) {
                return Err(invalid(format!(
                    "--select: empty path at position {}",
                    i + 1
                )));
            }
        }
        if let Some(paths) = &self.omit {
            if paths.is_empty() {
                return Err(invalid("--omit: empty path list"));
            }
            if let Some(i) = paths.iter().position(String::is_empty) {
                return Err(invalid(format!("--omit: empty path at position {}", i + 1)));
            }
        }
        if self.get.as_deref() == Some("") {
            return Err(invalid("--get: empty path"));
        }
        if self.rows.as_deref() == Some("") {
            return Err(invalid("--rows: empty path"));
        }
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
        if self.rows.is_some() && self.header {
            return Err(invalid("`--rows` cannot be combined with `--header`"));
        }
        if self.omit.is_some() {
            let other = [
                (self.select.is_some(), "--select"),
                (self.get.is_some(), "--get"),
                (self.template.is_some(), "--template"),
            ]
            .into_iter()
            .find_map(|(set, flag)| set.then_some(flag));
            if let Some(other) = other {
                return Err(invalid(format!(
                    "`--omit` cannot be combined with `{other}`"
                )));
            }
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

const LIMIT_ON_ONE: &str =
    "`--limit` applies to row reports; this command prints a single object (use --get or --select)";

fn validate_shaping_paths(
    rows: &[JsonValue],
    opts: &OutputOpts,
    template: Option<&Template>,
) -> Result<()> {
    if let Some(paths) = &opts.select {
        validate_paths(rows, paths.iter().map(String::as_str), "--select")?;
    }
    if let Some(path) = &opts.get {
        validate_paths(rows, [path.as_str()], "--get")?;
    }
    if let Some(t) = template {
        validate_paths(rows, t.paths(), "--template")?;
    }
    Ok(())
}

/// `emit` for a single-object report, by reference: only `--select` builds
/// a new value, so the report itself is never cloned.
fn emit_one(report: &JsonValue, style: Style, opts: &OutputOpts) -> Result<Emitted> {
    if opts.quiet {
        return Ok(Emitted::default());
    }
    let template = opts.template.as_deref().map(Template::parse).transpose()?;
    validate_shaping_paths(std::slice::from_ref(report), opts, template.as_ref())?;
    if opts.limit.is_some() {
        return Err(invalid(LIMIT_ON_ONE));
    }
    let projected;
    let report = match &opts.select {
        Some(paths) => {
            projected = project(report, paths);
            &projected
        }
        None => report,
    };
    let mut out = Vec::new();
    if let Some(path) = &opts.get {
        match navigate_json(report, path) {
            Some(JsonValue::Array(items)) => items
                .iter()
                .for_each(|v| push_line(&mut out, &value_text(v))),
            Some(v) => push_line(&mut out, &value_text(v)),
            None => {}
        }
    } else if let Some(t) = &template {
        push_line(&mut out, &t.render(report));
    } else {
        let style = if opts.lines { Style::Compact } else { style };
        push_json(&mut out, report, style)?;
    }
    Ok(Emitted {
        stdout: out,
        notice: None,
    })
}

/// Apply `opts` to one report. Order: `-q`, `--limit`, `--select`, then
/// `--get` / `--template`, then `--lines` or `style`. Paths are validated
/// against every row before `--limit` cuts any. A report without its
/// declared rows is emitted as one object.
pub(crate) fn emit(
    mut report: JsonValue,
    shape: Shape,
    style: Style,
    opts: &OutputOpts,
) -> Result<Emitted> {
    let (rows_shape, mut rows) = match (shape, row_slot(&mut report, shape)) {
        (Shape::Rows(r), Some(slot)) => (r, std::mem::take(slot)),
        _ => return emit_one(&report, style, opts),
    };
    if opts.quiet {
        return Ok(Emitted::default());
    }
    let template = opts.template.as_deref().map(Template::parse).transpose()?;
    validate_shaping_paths(&rows, opts, template.as_ref())?;

    let mut limited = None;
    if let Some(n) = opts.limit
        && rows.len() > n
    {
        limited = Some((n, rows.len()));
        rows.truncate(n);
    }

    if let Some(paths) = &opts.select {
        rows.iter_mut().for_each(|r| *r = project(r, paths));
    }

    let notice = limited.map(|(n, total)| format!("tomlctl: showing {n} of {total} rows"));
    let mut out = Vec::new();
    if opts.get.is_some() || template.is_some() {
        // A row lacking the `--get` path prints an empty line, as
        // `--template` renders a missing key, so output stays one line per
        // row.
        for r in &rows {
            let line = match (&opts.get, &template) {
                (Some(path), _) => navigate_json(r, path).map(value_text),
                (None, Some(t)) => Some(t.render(r)),
                (None, None) => None,
            };
            push_line(&mut out, &line.unwrap_or_default());
        }
        return Ok(Emitted {
            stdout: out,
            notice,
        });
    }
    if let Some(slot) = row_slot(&mut report, Shape::Rows(rows_shape)) {
        *slot = rows;
    }

    let limited_json =
        limited.map(|(shown, total)| serde_json::json!({ "shown": shown, "total": total }));
    match (rows_shape, limited_json) {
        (Rows::Field(_), Some(l)) => {
            if let Some(obj) = report.as_object_mut() {
                obj.insert("limited".to_string(), l);
            }
        }
        (Rows::Top, Some(l)) if !opts.lines => {
            report = serde_json::json!({ "rows": report, "limited": l });
        }
        (Rows::Top, Some(l)) => {
            push_json(
                &mut out,
                &serde_json::json!({ "limited": l }),
                Style::Compact,
            )?;
        }
        (_, None) => {}
    }

    if opts.lines {
        for line in &report_lines(report, rows_shape) {
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
    write_emitted(emit_one(v, Style::Pretty, o)?, o)
}

/// Compact single-line `print_json` for read verbs: a shaping failure is an
/// error, as in `print_json`. Write envelopes use `print_json_compact`.
pub(crate) fn print_json_line(v: &JsonValue) -> Result<()> {
    let o = opts();
    if o.is_inert() {
        return write_json(v, Style::Compact);
    }
    write_emitted(emit_one(v, Style::Compact, o)?, o)
}

/// `print_json_compact`'s core. A shaping failure yields the unshaped value
/// and a warning instead of an error: this emitter carries write envelopes,
/// and an exit of 1 after a landed write invites a retry that applies it
/// twice. Under `json_errors` the warning is one compact JSON line, since
/// stderr then carries only JSON.
fn compact_lenient(
    v: &JsonValue,
    shape: Shape,
    opts: &OutputOpts,
) -> Result<(Emitted, Option<String>)> {
    let shaped = match shape {
        Shape::One => emit_one(v, Style::Compact, opts),
        Shape::Rows(_) => emit(v.clone(), shape, Style::Compact, opts),
    };
    match shaped {
        Ok(emitted) => Ok((emitted, None)),
        Err(err) => {
            let mut stdout = Vec::new();
            push_json(&mut stdout, v, Style::Compact)?;
            let message = format!("{err:#}; printed the unshaped output");
            let warning = if opts.json_errors {
                let kind = err
                    .downcast_ref::<crate::errors::TaggedError>()
                    .map_or("other", |t| t.kind.as_str());
                serde_json::json!({ "warning": { "kind": kind, "message": message } }).to_string()
            } else {
                format!("tomlctl: warning: {message}")
            };
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
/// has consumed `--select`, `--limit`, `--lines` and the `--where*` filters,
/// which reach it through the list verb's merged `QueryArgs`.
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

/// Run `q` over `doc`'s `array` and emit the result: streamed line by line
/// when `--ndjson`/`--lines` meets a streamable shape and no `--get` or
/// `--template` needs the whole row set, otherwise buffered through
/// `emit_list_raw` or `print_query`.
pub(crate) fn print_query_list(doc: &toml::Value, array: &str, q: &query::Query) -> Result<()> {
    // Aggregation shapes ignore `--ndjson`: their output is one value. A
    // streamed `--pluck` mirrors `apply_pluck`'s null/missing drop, and under
    // `--raw` writes bare values per line.
    if q.ndjson && q.shape.is_streamable() && streaming_allowed() {
        return stdout_stream(|mut w| query::run_streaming(doc, array, q, &mut w));
    }
    let out = query::run(doc, array, q)?;
    if q.raw {
        emit_list_raw(&out, &q.shape)
    } else {
        print_query(out)
    }
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

const ALL_SHAPING: &[&str] = &[
    "--select",
    "--limit",
    "--lines",
    "--get",
    "--template",
    "--rows",
    "--header",
    "--max-chars",
    "--omit",
    "--where",
    "--where-not",
    "--where-in",
    "--where-has",
    "--where-missing",
    "--where-gt",
    "--where-gte",
    "--where-lt",
    "--where-lte",
    "--where-contains",
    "--where-prefix",
    "--where-suffix",
    "--where-regex",
];

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
/// terminal status lines emitted by write-path and dry-run dispatch arms.
/// Integration tests assert on these compact bytes with
/// `.contains(r#"{"ok":true,"added":N}"#)`, so the single-line form is
/// load-bearing. A shaping failure prints the unshaped value with a stderr
/// warning and returns `Ok` (see `compact_lenient`); a read verb that prints
/// compact JSON uses `print_json_line`, which fails instead.
pub(crate) fn print_json_compact(v: &JsonValue) -> Result<()> {
    let o = opts();
    if o.is_inert() {
        return write_json(v, Style::Compact);
    }
    let (emitted, warning) = compact_lenient(v, Shape::One, o)?;
    if let Some(w) = warning {
        eprintln!("{w}");
    }
    write_emitted(emitted, o)
}

/// `print_json_compact` for a write envelope that carries per-row results in
/// one field, so `--select` / `--get` / `--template` / `--limit` act on those
/// rows and `--lines` emits the rest of the envelope as a header line first.
pub(crate) fn print_rows_compact(v: &JsonValue, rows: Rows) -> Result<()> {
    let o = opts();
    if o.is_inert() {
        return write_json(v, Style::Compact);
    }
    let (emitted, warning) = compact_lenient(v, Shape::Rows(rows), o)?;
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
        let msg = "`--limit` applies to row reports; this command prints a single object (use --get or --select)";
        assert_eq!(err(json!({ "a": 1 }), Shape::One, &o), msg);
        assert_eq!(err(json!({ "a": 1 }), FIELD, &o), msg);
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
    fn select_keeps_a_path_no_row_carries_beside_one_that_matches() {
        let o = with(|o| {
            o.select = Some(vec!["id".into(), "promoted_to".into()]);
            o.lines = true;
        });
        assert_eq!(
            out(rows3(), TOP, Style::Pretty, &o),
            "{\"id\":1}\n{\"id\":2}\n{\"id\":3}\n"
        );
        let o = with(|o| o.select = Some(vec!["nope".into(), "also_nope".into()]));
        assert!(err(rows3(), TOP, &o).starts_with("--select path `nope` matches no field"));
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
        assert_eq!(out(rows3(), TOP, Style::Pretty, &o), "[2]\n\n\n");
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
        let (e, warning) = compact_lenient(&v, Shape::One, &o).unwrap();
        assert_eq!(
            String::from_utf8(e.stdout).unwrap(),
            "{\"ok\":true,\"added\":1}\n"
        );
        assert!(warning.unwrap().contains("--get path `missing`"));
        let o = with(|o| o.limit = Some(1));
        let (_, warning) = compact_lenient(&v, Shape::One, &o).unwrap();
        assert!(warning.unwrap().contains(LIMIT_ON_ONE));
        let o = with(|o| o.get = Some("added".into()));
        let (e, warning) = compact_lenient(&v, Shape::One, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap(), "1\n");
        assert!(warning.is_none());
    }

    #[test]
    fn strict_compact_shaping_failure_is_a_validation_error() {
        let v = json!({ "ok": true, "added": 1 });
        let o = with(|o| o.get = Some("missing".into()));
        let err = emit_one(&v, Style::Compact, &o).unwrap_err();
        let tagged = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tagged.kind.as_str(), "validation");
        let o = with(|o| o.lines = true);
        let e = emit_one(&json!({ "a": [1, 2] }), Style::Pretty, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap(), "{\"a\":[1,2]}\n");
    }

    #[test]
    fn compact_rows_shape_the_row_field_and_stay_lenient() {
        let v = json!({
            "ok": true,
            "added": ["B-1"],
            "rows": [{ "line": 1, "action": "added", "id": "B-1" }]
        });
        let rows = Shape::Rows(Rows::Field("rows"));
        let o = with(|o| o.template = Some("{line} {action} {id}".into()));
        let (e, warning) = compact_lenient(&v, rows, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap(), "1 added B-1\n");
        assert!(warning.is_none());
        let o = with(|o| o.lines = true);
        let (e, _) = compact_lenient(&v, rows, &o).unwrap();
        assert_eq!(
            String::from_utf8(e.stdout).unwrap(),
            "{\"ok\":true,\"added\":[\"B-1\"]}\n{\"line\":1,\"action\":\"added\",\"id\":\"B-1\"}\n"
        );
        let o = with(|o| o.get = Some("added".into()));
        let (e, warning) = compact_lenient(&v, rows, &o).unwrap();
        assert_eq!(
            String::from_utf8(e.stdout).unwrap(),
            format!("{}\n", serde_json::to_string(&v).unwrap())
        );
        assert!(warning.unwrap().contains("--get path `added`"));
    }

    #[test]
    fn compact_shaping_warning_is_json_under_json_errors() {
        let v = json!({ "ok": true });
        let o = with(|o| {
            o.get = Some("missing".into());
            o.json_errors = true;
        });
        let (_, warning) = compact_lenient(&v, Shape::One, &o).unwrap();
        let warning = warning.unwrap();
        assert!(!warning.contains('\n'), "{warning}");
        let parsed: JsonValue = serde_json::from_str(&warning).unwrap();
        assert_eq!(parsed["warning"]["kind"], "validation");
        let message = parsed["warning"]["message"].as_str().unwrap();
        assert!(message.starts_with("--get path `missing`"), "{message}");
        assert!(
            message.ends_with("; printed the unshaped output"),
            "{message}"
        );
    }

    #[test]
    fn empty_select_and_get_paths_are_refused() {
        let msg = |f: fn(&mut OutputOpts)| {
            let e = with(f).validate().unwrap_err();
            let tag = e.downcast_ref::<crate::errors::TaggedError>().unwrap();
            assert_eq!(tag.kind.as_str(), "validation");
            format!("{e:#}")
        };
        assert_eq!(
            msg(|o| o.select = Some(vec!["id".into(), String::new()])),
            "--select: empty path at position 2"
        );
        assert_eq!(
            msg(|o| o.select = Some(vec![])),
            "--select: empty path list"
        );
        assert_eq!(msg(|o| o.get = Some(String::new())), "--get: empty path");
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

    fn conflict(f: impl FnOnce(&mut OutputOpts)) -> String {
        let e = with(f).validate().unwrap_err();
        let tag = e.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tag.kind.as_str(), "validation");
        format!("{e:#}")
    }

    #[test]
    fn global_flag_conflict_rows_with_header() {
        assert_eq!(
            conflict(|o| {
                o.rows = Some("deps".into());
                o.header = true;
            }),
            "`--rows` cannot be combined with `--header`"
        );
        assert!(with(|o| o.rows = Some("deps".into())).validate().is_ok());
        assert!(with(|o| o.header = true).validate().is_ok());
    }

    #[test]
    fn global_flag_conflict_omit_with_select_get_and_template() {
        type Setter = fn(&mut OutputOpts);
        let cases: [(Setter, &str); 3] = [
            (|o| o.select = Some(vec!["a".into()]), "--select"),
            (|o| o.get = Some("a".into()), "--get"),
            (|o| o.template = Some("{a}".into()), "--template"),
        ];
        for (set, flag) in cases {
            let msg = conflict(|o| {
                o.omit = Some(vec!["b".into()]);
                set(o);
            });
            assert_eq!(msg, format!("`--omit` cannot be combined with `{flag}`"));
        }
        assert!(
            with(|o| {
                o.omit = Some(vec!["b".into()]);
                o.max_chars = Some(10);
                o.limit = Some(1);
                o.lines = true;
            })
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn global_flag_conflict_quiet_with_each_new_flag() {
        type Setter = fn(&mut OutputOpts);
        let cases: [(Setter, &str); 7] = [
            (|o| o.rows = Some("deps".into()), "--rows"),
            (|o| o.header = true, "--header"),
            (|o| o.max_chars = Some(5), "--max-chars"),
            (|o| o.omit = Some(vec!["a".into()]), "--omit"),
            (|o| o.filters.where_eq = vec!["a=1".into()], "--where"),
            (|o| o.filters.where_not = vec!["a=1".into()], "--where-not"),
            (
                |o| o.filters.where_regex = vec!["a=x".into()],
                "--where-regex",
            ),
        ];
        for (set, flag) in cases {
            let msg = conflict(|o| {
                o.quiet = true;
                set(o);
            });
            assert_eq!(msg, format!("`-q` cannot be combined with `{flag}`"));
        }
    }

    #[test]
    fn global_flag_conflict_empty_omit_and_rows_paths() {
        assert_eq!(
            conflict(|o| o.omit = Some(vec!["a".into(), String::new()])),
            "--omit: empty path at position 2"
        );
        assert_eq!(
            conflict(|o| o.omit = Some(vec![])),
            "--omit: empty path list"
        );
        assert_eq!(
            conflict(|o| o.rows = Some(String::new())),
            "--rows: empty path"
        );
    }

    #[test]
    fn global_flags_are_refused_on_text_output() {
        let o = with(|o| o.filters.where_has = vec!["a".into()]);
        let e = text_allowed(&o, ALL_SHAPING).unwrap_err();
        assert_eq!(
            format!("{e:#}"),
            "`--where-has` does not apply to this command's text output"
        );
        let o = with(|o| o.max_chars = Some(3));
        assert!(text_allowed(&o, ALL_SHAPING).is_err());
    }

    /// The root-level `--where` values and the `tasks list` engine's own,
    /// read back from one parse of `argv`.
    fn parse_where(argv: &[&str]) -> (WhereFilters, Vec<String>) {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        crate::test_support::on_cli_stack(move || {
            use clap::{CommandFactory as _, Parser as _};
            let cli = crate::cli::Cli::try_parse_from(&argv)
                .unwrap_or_else(|e| panic!("{argv:?} must parse: {e}"));
            let matches = crate::cli::Cli::command()
                .try_get_matches_from(&argv)
                .unwrap();
            let list = matches
                .subcommand_matches("tasks")
                .and_then(|m| m.subcommand_matches("list"))
                .expect("tasks list matches");
            let engine = list
                .get_many::<String>("where_eq")
                .map(|v| v.cloned().collect())
                .unwrap_or_default();
            (cli.output.filters.to_filters(), engine)
        })
    }

    #[test]
    fn global_where_parses_identically_before_and_after_subcommand() {
        let before = [
            "tomlctl",
            "--where",
            "status=done",
            "tasks",
            "list",
            "--slug",
            "s",
        ];
        let after = [
            "tomlctl",
            "tasks",
            "list",
            "--slug",
            "s",
            "--where",
            "status=done",
        ];
        let (global_before, engine_before) = parse_where(&before);
        let (global_after, engine_after) = parse_where(&after);
        assert_eq!(global_before.where_eq, ["status=done"]);
        assert_eq!(engine_before, ["status=done"]);
        assert_eq!(global_before, global_after);
        assert_eq!(engine_before, engine_after);
        check_where_placement(&before[1..], &global_before).unwrap();
        check_where_placement(&after[1..], &global_after).unwrap();
    }

    #[test]
    fn global_where_split_across_subcommand_is_refused() {
        let split = [
            "tomlctl",
            "--where",
            "a=1",
            "tasks",
            "list",
            "--slug",
            "s",
            "--where=b=2",
        ];
        let (global, engine) = parse_where(&split);
        assert_eq!(global.where_eq, ["b=2"]);
        assert_eq!(engine, ["b=2"]);
        let e = check_where_placement(&split[1..], &global).unwrap_err();
        let tag = e.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tag.kind.as_str(), "validation");
        assert!(
            format!("{e:#}").contains("one side of the subcommand"),
            "{e:#}"
        );

        let one_side = [
            "tomlctl",
            "tasks",
            "list",
            "--slug",
            "s",
            "--where",
            "a=1",
            "--where-not",
            "b=2",
        ];
        let (global, _) = parse_where(&one_side);
        assert_eq!(global.len(), 2);
        check_where_placement(&one_side[1..], &global).unwrap();
        let positional = ["--", "--where"];
        check_where_placement(positional, &WhereFilters::default()).unwrap();
    }
}
