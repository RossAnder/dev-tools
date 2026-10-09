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
use std::borrow::Cow;
use std::io::{BufWriter, Write};
use std::sync::OnceLock;

use crate::convert::{
    WILDCARD, is_wildcard_path, json_type_name, navigate_json, navigate_json_all, project,
    validate_paths,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::ScalarMutationPlan;
use crate::items::MutationPlan;
use crate::query::{self, Cut, OutputShape, Predicate, RawArrayHint, ShapeDispatch, WhereInput};
use template::{Template, truncate_text, value_text};

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

    /// The long name of the first flag that carries a value.
    fn first_set(&self) -> Option<&'static str> {
        self.families()
            .into_iter()
            .find_map(|(flag, values)| (!values.is_empty()).then_some(flag))
    }

    fn input(&self) -> WhereInput<'_> {
        WhereInput {
            where_eq: &self.where_eq,
            where_not: &self.where_not,
            where_in: &self.where_in,
            where_has: &self.where_has,
            where_missing: &self.where_missing,
            where_gt: &self.where_gt,
            where_gte: &self.where_gte,
            where_lt: &self.where_lt,
            where_lte: &self.where_lte,
            where_contains: &self.where_contains,
            where_prefix: &self.where_prefix,
            where_suffix: &self.where_suffix,
            where_regex: &self.where_regex,
        }
    }

    fn predicates(&self) -> Result<Vec<Predicate>> {
        query::predicates_from(&self.input())
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

/// Where `emit` takes a report's rows from: its declared `Rows`, or the
/// dotted `--rows` path.
#[derive(Clone, Copy)]
enum RowsAt<'a> {
    Top,
    Field(&'a str),
}

impl From<Rows> for RowsAt<'static> {
    fn from(rows: Rows) -> Self {
        match rows {
            Rows::Top => RowsAt::Top,
            Rows::Field(key) => RowsAt::Field(key),
        }
    }
}

fn json_path_mut<'a>(root: &'a mut JsonValue, path: &str) -> Option<&'a mut JsonValue> {
    let mut cur = root;
    for seg in path.split('.') {
        cur = match cur {
            JsonValue::Object(map) => map.get_mut(seg)?,
            JsonValue::Array(arr) => arr.get_mut(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn row_slot<'r>(report: &'r mut JsonValue, at: RowsAt<'_>) -> Option<&'r mut Vec<JsonValue>> {
    match at {
        RowsAt::Top => report.as_array_mut(),
        RowsAt::Field(path) => json_path_mut(report, path)?.as_array_mut(),
    }
}

/// Remove the value at `path`, leaving the report's header.
fn detach(report: &mut JsonValue, path: &str) {
    let (parent, last) = match path.rsplit_once('.') {
        Some((parent, last)) => (json_path_mut(report, parent), last),
        None => (Some(report), path),
    };
    match parent {
        Some(JsonValue::Object(map)) => {
            map.shift_remove(last);
        }
        Some(JsonValue::Array(arr)) => {
            if let Ok(i) = last.parse::<usize>()
                && i < arr.len()
            {
                arr.remove(i);
            }
        }
        _ => {}
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

const HEADER_ON_ONE: &str =
    "`--header` applies to a report with rows beside a header; this command prints a single object";

const HEADER_ON_TOP: &str =
    "`--header` applies to a report with rows beside a header; this report is a bare row array";

/// `--rows` given for a report whose rows are already declared.
fn rows_on_row_report(rows: Rows) -> anyhow::Error {
    let what = match rows {
        Rows::Top => "this report is already a row array".to_string(),
        Rows::Field(key) => format!("this report's rows are already its `{key}` field"),
    };
    invalid(format!(
        "`--rows` applies to a single-object report; {what}"
    ))
}

/// The array `--rows` names on a single report.
fn check_rows_path(report: &JsonValue, path: &str) -> Result<()> {
    validate_paths(std::slice::from_ref(report), [path], "--rows")?;
    if is_wildcard_path(path) {
        return Err(invalid(format!(
            "--rows path `{path}`: `*` is not allowed; name one array"
        )));
    }
    match navigate_json(report, path) {
        Some(JsonValue::Array(_)) => Ok(()),
        Some(v) => Err(invalid(format!(
            "--rows path `{path}` is not an array (found {})",
            json_type_name(v)
        ))),
        None => Err(invalid(format!("--rows path `{path}` matches no field"))),
    }
}

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

/// `--omit` paths are valid when some row carries one, or failing that the
/// header does: either is a place the paths are dropped from.
fn validate_omit_paths(
    rows: &[JsonValue],
    header: Option<&JsonValue>,
    paths: &[String],
) -> Result<()> {
    let names = || paths.iter().map(String::as_str);
    let on_rows = validate_paths(rows, names(), "--omit");
    match header {
        Some(h)
            if on_rows.is_err()
                && validate_paths(std::slice::from_ref(h), names(), "--omit").is_ok() =>
        {
            Ok(())
        }
        _ => on_rows,
    }
}

/// Remove the value at `segs` from `v`; a `*` segment walks every element
/// or value, and a branch that does not resolve is left alone.
fn omit_path(v: &mut JsonValue, segs: &[&str]) {
    let Some((&seg, rest)) = segs.split_first() else {
        return;
    };
    if rest.is_empty() {
        match v {
            JsonValue::Object(map) if seg == WILDCARD => map.clear(),
            JsonValue::Object(map) => {
                map.shift_remove(seg);
            }
            JsonValue::Array(arr) if seg == WILDCARD => arr.clear(),
            JsonValue::Array(arr) => {
                if let Ok(i) = seg.parse::<usize>()
                    && i < arr.len()
                {
                    arr.remove(i);
                }
            }
            _ => {}
        }
        return;
    }
    match v {
        JsonValue::Object(map) if seg == WILDCARD => {
            map.values_mut().for_each(|c| omit_path(c, rest));
        }
        JsonValue::Object(map) => {
            if let Some(c) = map.get_mut(seg) {
                omit_path(c, rest);
            }
        }
        JsonValue::Array(arr) if seg == WILDCARD => {
            arr.iter_mut().for_each(|c| omit_path(c, rest));
        }
        JsonValue::Array(arr) => {
            if let Some(c) = seg.parse::<usize>().ok().and_then(|i| arr.get_mut(i)) {
                omit_path(c, rest);
            }
        }
        _ => {}
    }
}

fn omit_paths(v: &mut JsonValue, paths: &[String]) {
    for p in paths {
        omit_path(v, &p.split('.').collect::<Vec<_>>());
    }
}

/// Cut every string inside `v` to `n` Unicode scalars, marking how many were
/// cut. Not idempotent (the marker lengthens a cut string), so each value is
/// walked once.
fn cap_strings(v: &mut JsonValue, n: usize) {
    match v {
        JsonValue::String(s) => {
            if s.char_indices().nth(n).is_some() {
                *s = truncate_text(s, n).into_owned();
            }
        }
        JsonValue::Array(arr) => arr.iter_mut().for_each(|c| cap_strings(c, n)),
        JsonValue::Object(map) => map.values_mut().for_each(|c| cap_strings(c, n)),
        _ => {}
    }
}

/// `--omit` then `--max-chars` over one row, header or single report.
fn trim(v: &mut JsonValue, opts: &OutputOpts) {
    if let Some(paths) = &opts.omit {
        omit_paths(v, paths);
    }
    if let Some(n) = opts.max_chars {
        cap_strings(v, n);
    }
}

/// `emit` for a single-object report, by reference: only `--select`,
/// `--omit`, `--max-chars` and `--rows` build a new value, so the report
/// itself is otherwise never cloned.
fn emit_one(report: &JsonValue, style: Style, opts: &OutputOpts) -> Result<Emitted> {
    if opts.quiet {
        return Ok(Emitted::default());
    }
    if opts.header {
        return Err(invalid(HEADER_ON_ONE));
    }
    if let Some(path) = &opts.rows {
        check_rows_path(report, path)?;
        let at = RowsAt::Field(path);
        let mut report = report.clone();
        let rows = row_slot(&mut report, at)
            .map(std::mem::take)
            .unwrap_or_default();
        return emit_rows(report, at, rows, style, opts, None);
    }
    let template = opts.template.as_deref().map(Template::parse).transpose()?;
    validate_shaping_paths(std::slice::from_ref(report), opts, template.as_ref())?;
    if let Some(paths) = &opts.omit {
        validate_omit_paths(std::slice::from_ref(report), None, paths)?;
    }
    if opts.limit.is_some() {
        return Err(invalid(LIMIT_ON_ONE));
    }
    if let Some(flag) = opts.filters.first_set() {
        return Err(invalid(format!(
            "`{flag}` applies to row reports; this command prints a single object (use --rows <PATH> to filter one of its arrays)"
        )));
    }
    let mut shaped = match &opts.select {
        Some(paths) => Cow::Owned(project(report, paths)),
        None => Cow::Borrowed(report),
    };
    if opts.omit.is_some() || opts.max_chars.is_some() {
        trim(shaped.to_mut(), opts);
    }
    let report: &JsonValue = &shaped;
    let mut out = Vec::new();
    if let Some(path) = &opts.get {
        // A plain path naming an array spreads it one element per line; a
        // `*` path prints each match on its own line, an array match whole.
        let wildcard = is_wildcard_path(path);
        for v in navigate_json_all(report, path) {
            match v {
                JsonValue::Array(items) if !wildcard => items
                    .iter()
                    .for_each(|v| push_line(&mut out, &value_text(v))),
                v => push_line(&mut out, &value_text(v)),
            }
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

/// Apply `opts` to one report. Order: `-q`, `--rows` / `--header`, path
/// validation against every row, `--where*`, `--limit`, `--omit` /
/// `--select`, `--max-chars`, then `--get` / `--template`, then `--lines` or
/// `style`. A report without its declared rows is emitted as one object.
///
/// The list verbs filter in their query engine and reach this through
/// `print_query_list`, whose `query_opts` drops `--where*`. A list-like verb
/// that calls `print_report` over rows already filtered by its `QueryArgs`
/// would filter twice, and after a `--select` projection the second pass
/// could drop every row.
pub(crate) fn emit(
    mut report: JsonValue,
    shape: Shape,
    style: Style,
    opts: &OutputOpts,
) -> Result<Emitted> {
    let (declared, rows) = match shape {
        Shape::Rows(r) => match row_slot(&mut report, r.into()) {
            Some(slot) => (r, std::mem::take(slot)),
            None => return emit_one(&report, style, opts),
        },
        Shape::One => return emit_one(&report, style, opts),
    };
    if opts.quiet {
        return Ok(Emitted::default());
    }
    if opts.rows.is_some() {
        return Err(rows_on_row_report(declared));
    }
    if opts.header {
        let Rows::Field(key) = declared else {
            return Err(invalid(HEADER_ON_TOP));
        };
        detach(&mut report, key);
        let opts = OutputOpts {
            header: false,
            ..opts.clone()
        };
        return emit_one(&report, style, &opts);
    }
    emit_rows(report, declared.into(), rows, style, opts, None)
}

/// `emit` once the rows are out of `report`; `at` is where they go back.
/// `precut` reports rows a query engine's `--limit` already dropped, in the
/// same `limited` form a cut made here takes.
fn emit_rows(
    mut report: JsonValue,
    at: RowsAt<'_>,
    mut rows: Vec<JsonValue>,
    style: Style,
    opts: &OutputOpts,
    precut: Option<Cut>,
) -> Result<Emitted> {
    let template = opts.template.as_deref().map(Template::parse).transpose()?;
    validate_shaping_paths(&rows, opts, template.as_ref())?;
    if let Some(paths) = &opts.omit {
        let header = matches!(at, RowsAt::Field(_)).then_some(&report);
        validate_omit_paths(&rows, header, paths)?;
    }
    query::json_rows::filter(&mut rows, &opts.filters.predicates()?)?;

    let mut limited = precut;
    if let Some(n) = opts.limit
        && rows.len() > n
    {
        limited = Some(Cut {
            shown: n,
            total: rows.len(),
        });
        rows.truncate(n);
    }

    if let Some(paths) = &opts.select {
        rows.iter_mut().for_each(|r| *r = project(r, paths));
    }
    let trims = opts.omit.is_some() || opts.max_chars.is_some();
    if trims {
        rows.iter_mut().for_each(|r| trim(r, opts));
    }

    let notice = limited.map(Cut::notice);
    let mut out = Vec::new();
    if opts.get.is_some() || template.is_some() {
        // A row lacking a plain `--get` path prints an empty line, as
        // `--template` renders a missing key, so output stays one line per
        // row. A `*` path prints one line per match instead, none for a row
        // without any.
        for r in &rows {
            match (&opts.get, &template) {
                (Some(path), _) if is_wildcard_path(path) => navigate_json_all(r, path)
                    .into_iter()
                    .for_each(|v| push_line(&mut out, &value_text(v))),
                (Some(path), _) => push_line(
                    &mut out,
                    &navigate_json(r, path).map(value_text).unwrap_or_default(),
                ),
                (None, Some(t)) => push_line(&mut out, &t.render(r)),
                (None, None) => {}
            }
        }
        return Ok(Emitted {
            stdout: out,
            notice,
        });
    }
    // The header is trimmed while its row slot is still empty, so no row is
    // walked twice. Omitting the row field itself drops the rows with it.
    if trims && matches!(at, RowsAt::Field(_)) {
        trim(&mut report, opts);
    }
    if let Some(slot) = row_slot(&mut report, at) {
        *slot = rows;
    }

    match (at, limited.map(Cut::header)) {
        (RowsAt::Field(_), Some(l)) => {
            if let Some(obj) = report.as_object_mut() {
                obj.insert("limited".to_string(), l);
            }
        }
        (RowsAt::Top, Some(l)) if !opts.lines => {
            report = serde_json::json!({ "rows": report, "limited": l });
        }
        (RowsAt::Top, Some(l)) => {
            push_json(
                &mut out,
                &serde_json::json!({ "limited": l }),
                Style::Compact,
            )?;
        }
        (_, None) => {}
    }

    if opts.lines {
        for line in &lines_at(report, at) {
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
pub(crate) fn stdout_stream<T>(f: impl FnOnce(&mut dyn Write) -> Result<T>) -> Result<T> {
    if opts().quiet {
        return f(&mut std::io::sink());
    }
    let stdout = std::io::stdout();
    let mut h = stdout.lock();
    let out = f(&mut h)?;
    h.flush()?;
    Ok(out)
}

/// Report on stderr a `--limit` cut that a query list's output has no place
/// for; withheld under `-q`, and under `--error-format json` as in
/// `write_emitted`.
fn report_cut(cut: Option<Cut>, opts: &OutputOpts) -> Result<()> {
    if opts.quiet {
        return Ok(());
    }
    let emitted = Emitted {
        stdout: Vec::new(),
        notice: cut.map(Cut::notice),
    };
    write_emitted(emitted, opts)
}

/// Whether the output options leave a query list free to stream: every
/// option the engine does not consume needs the whole row set.
pub(crate) fn streaming_allowed() -> bool {
    let o = opts();
    o.get.is_none() && o.template.is_none() && o.omit.is_none() && o.max_chars.is_none()
}

/// The options that still apply to a query list's output once its engine
/// has consumed `--select`, `--limit`, `--lines` and the `--where*` filters,
/// which reach it through the list verb's merged `QueryArgs`. `lines` is the
/// engine's own line encoding, which this output has to reproduce when the
/// options kept it from streaming.
fn query_opts(opts: &OutputOpts, lines: bool) -> OutputOpts {
    OutputOpts {
        get: opts.get.clone(),
        template: opts.template.clone(),
        omit: opts.omit.clone(),
        max_chars: opts.max_chars,
        quiet: opts.quiet,
        json_errors: opts.json_errors,
        lines,
        ..OutputOpts::default()
    }
}

/// A list verb's rows are its listed items, so `--rows` and `--header` have
/// nothing to reshape; refused rather than dropped by `query_opts`.
fn refuse_row_flags_on_list(opts: &OutputOpts) -> Result<()> {
    let flag = if opts.rows.is_some() {
        "--rows"
    } else if opts.header {
        "--header"
    } else {
        return Ok(());
    };
    Err(invalid(format!(
        "`{flag}` does not apply to a list verb: its rows are the listed items"
    )))
}

/// Emit a query list's engine output: rows when it is an array, one value
/// otherwise; `lines` writes a streamable shape one row per line. `cut` is
/// the rows the engine's `--limit` dropped: a `limited` header when
/// `in_band` (wrapping pretty rows, or leading the lines), a stderr notice
/// otherwise.
pub(crate) fn print_query(
    out: JsonValue,
    lines: bool,
    cut: Option<Cut>,
    in_band: bool,
) -> Result<()> {
    let o = query_opts(opts(), lines);
    let emitted = match out {
        JsonValue::Array(rows) if in_band && cut.is_some() && !o.quiet => emit_rows(
            JsonValue::Array(Vec::new()),
            RowsAt::Top,
            rows,
            Style::Pretty,
            &o,
            cut,
        )?,
        out => {
            let shape = if out.is_array() {
                Shape::Rows(Rows::Top)
            } else {
                Shape::One
            };
            let mut emitted = emit(out, shape, Style::Pretty, &o)?;
            if !in_band && !o.quiet && emitted.notice.is_none() {
                emitted.notice = cut.map(Cut::notice);
            }
            emitted
        }
    };
    write_emitted(emitted, &o)
}

/// Run `q` over `doc`'s `array` and emit the result: streamed line by line
/// when `--ndjson`/`--lines` meets a streamable shape and no output option
/// needs the whole row set, otherwise buffered through `emit_list_raw` or
/// `print_query`. A `--limit` cut of row or pluck output is reported per
/// `Query::cut_in_band`; an aggregate's is not reported.
pub(crate) fn print_query_list(doc: &toml::Value, array: &str, q: &query::Query) -> Result<()> {
    refuse_row_flags_on_list(opts())?;
    // Aggregation shapes ignore `--ndjson`: their output is one value. A
    // streamed `--pluck` mirrors `apply_pluck`'s null/missing drop, and under
    // `--raw` writes bare values per line.
    let line_rows = q.per_line() && q.shape.is_streamable();
    if line_rows && streaming_allowed() {
        let cut = stdout_stream(|mut w| query::run_streaming(doc, array, q, &mut w))?;
        if q.cut_in_band() {
            return Ok(());
        }
        return report_cut(cut, opts());
    }
    let (out, cut) = query::run_counted(doc, array, q)?;
    let cut = cut.filter(|_| q.shape.is_streamable());
    if q.raw {
        emit_list_raw(&out, &q.shape)?;
        report_cut(cut, opts())
    } else {
        print_query(out, line_rows, cut, q.cut_in_band())
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

/// The `--lines` encoding of a report: with rows at a field, the rest of the
/// report as one header line first (so a truncated read keeps the totals and
/// hazards), omitted when it is an empty object; then one line per row. A
/// report that does not have the declared shape is one line.
fn lines_at(mut report: JsonValue, at: RowsAt<'_>) -> Vec<JsonValue> {
    let RowsAt::Field(path) = at else {
        return match report {
            JsonValue::Array(rows) => rows,
            report => vec![report],
        };
    };
    let Some(rows) = row_slot(&mut report, at).map(std::mem::take) else {
        return vec![report];
    };
    detach(&mut report, path);
    let mut lines = Vec::with_capacity(rows.len() + 1);
    if !report.as_object().is_some_and(serde_json::Map::is_empty) {
        lines.push(report);
    }
    lines.extend(rows);
    lines
}

/// Emit a read verb's report: pretty JSON, or under `--lines` the
/// `lines_at` encoding.
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
    let o = opts();
    let print = text_allowed(o, &["--get", "--template", "--omit"])?;
    let capped;
    let v = match o.max_chars {
        Some(n) => {
            let mut c = v.clone();
            cap_strings(&mut c, n);
            capped = c;
            &capped
        }
        None => v,
    };
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

    fn report_lines(report: JsonValue, rows: Rows) -> Vec<JsonValue> {
        lines_at(report, rows.into())
    }

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
        let o = query_opts(
            &with(|o| {
                o.select = Some(vec!["x".into()]);
                o.limit = Some(1);
                o.lines = true;
                o.get = Some("id".into());
            }),
            false,
        );
        assert_eq!(out(rows3(), TOP, Style::Pretty, &o), "1\n2\n3\n");
    }

    fn long_rows() -> JsonValue {
        json!([
            { "id": 1, "detail": "abcdefghij", "tags": ["0123456789"], "n": { "s": "xyzxyzxyz" } },
            { "id": 2, "detail": "short" }
        ])
    }

    #[test]
    fn max_chars_cuts_every_string_after_projection() {
        let o = with(|o| {
            o.max_chars = Some(4);
            o.lines = true;
        });
        assert_eq!(
            out(long_rows(), TOP, Style::Pretty, &o),
            "{\"id\":1,\"detail\":\"abcd…(+6)\",\"tags\":[\"0123…(+6)\"],\"n\":{\"s\":\"xyzx…(+5)\"}}\n\
             {\"id\":2,\"detail\":\"shor…(+1)\"}\n"
        );
        let o = with(|o| {
            o.max_chars = Some(3);
            o.select = Some(vec!["detail".into()]);
        });
        assert_eq!(
            out(long_rows(), TOP, Style::Compact, &o),
            "[{\"detail\":\"abc…(+7)\"},{\"detail\":\"sho…(+2)\"}]\n"
        );
    }

    #[test]
    fn max_chars_caps_get_output_and_the_header() {
        let o = with(|o| {
            o.max_chars = Some(2);
            o.get = Some("detail".into());
        });
        assert_eq!(
            out(long_rows(), TOP, Style::Pretty, &o),
            "ab…(+8)\nsh…(+3)\n"
        );
        let o = with(|o| {
            o.max_chars = Some(2);
            o.get = Some("title".into());
        });
        assert_eq!(
            out(json!({ "title": "héllo" }), Shape::One, Style::Pretty, &o),
            "hé…(+3)\n"
        );
        let report = json!({ "note": "header text", "items": long_rows() });
        let o = with(|o| {
            o.max_chars = Some(3);
            o.lines = true;
            o.select = Some(vec!["id".into()]);
        });
        assert_eq!(
            out(report, FIELD, Style::Pretty, &o),
            "{\"note\":\"hea…(+8)\"}\n{\"id\":1}\n{\"id\":2}\n"
        );
    }

    #[test]
    fn omit_drops_paths_from_rows_and_the_header() {
        let o = with(|o| {
            o.omit = Some(vec!["detail".into(), "n.s".into(), "note".into()]);
            o.lines = true;
        });
        let report = json!({ "note": "x", "total": 2, "items": long_rows() });
        assert_eq!(
            out(report, FIELD, Style::Pretty, &o),
            "{\"total\":2}\n{\"id\":1,\"tags\":[\"0123456789\"],\"n\":{}}\n{\"id\":2}\n"
        );
        let o = with(|o| o.omit = Some(vec!["deps.*.files".into(), "title".into()]));
        assert_eq!(
            out(show_report(), Shape::One, Style::Compact, &o),
            "{\"id\":3,\"deps\":[{\"id\":1,\"ref\":\"a\"},{\"id\":2,\"ref\":\"b\"}]}\n"
        );
    }

    #[test]
    fn omit_refuses_a_path_nothing_carries() {
        let o = with(|o| o.omit = Some(vec!["nope".into()]));
        let e = emit(long_rows(), TOP, Style::Pretty, &o).unwrap_err();
        assert_eq!(tag(&e), "validation");
        assert_eq!(
            format!("{e:#}"),
            "--omit path `nope` matches no field; available fields: id, detail, tags, n"
        );
        assert!(
            err(show_report(), Shape::One, &o).starts_with("--omit path `nope` matches no field")
        );
        let report = json!({ "note": "x", "items": long_rows() });
        let o = with(|o| o.omit = Some(vec!["note".into()]));
        emit(report, FIELD, Style::Pretty, &o).unwrap();
    }

    #[test]
    fn query_output_keeps_omit_and_max_chars() {
        let o = query_opts(
            &with(|o| {
                o.omit = Some(vec!["n".into(), "tags".into()]);
                o.max_chars = Some(1);
                o.limit = Some(1);
            }),
            true,
        );
        assert_eq!(
            out(long_rows(), TOP, Style::Pretty, &o),
            "{\"id\":1,\"detail\":\"a…(+9)\"}\n{\"id\":2,\"detail\":\"s…(+4)\"}\n"
        );
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

    fn show_report() -> JsonValue {
        json!({
            "id": 3,
            "title": "t",
            "deps": [
                { "id": 1, "ref": "a", "files": ["x.rs", "y.rs"] },
                { "id": 2, "ref": "b", "files": ["z.rs"] }
            ]
        })
    }

    fn tag(e: &anyhow::Error) -> &'static str {
        e.downcast_ref::<crate::errors::TaggedError>()
            .map_or("untagged", |t| t.kind.as_str())
    }

    #[test]
    fn rows_reroots_a_single_report_onto_a_nested_array() {
        let o = with(|o| {
            o.rows = Some("deps".into());
            o.get = Some("ref".into());
        });
        assert_eq!(out(show_report(), Shape::One, Style::Pretty, &o), "a\nb\n");
        let o = with(|o| {
            o.rows = Some("deps".into());
            o.lines = true;
            o.select = Some(vec!["id".into()]);
        });
        assert_eq!(
            out(show_report(), Shape::One, Style::Pretty, &o),
            "{\"id\":3,\"title\":\"t\"}\n{\"id\":1}\n{\"id\":2}\n"
        );
        let o = with(|o| {
            o.rows = Some("deps".into());
            o.limit = Some(1);
        });
        let v: JsonValue =
            serde_json::from_str(&out(show_report(), Shape::One, Style::Compact, &o)).unwrap();
        assert_eq!(
            v["deps"],
            json!([{ "id": 1, "ref": "a", "files": ["x.rs", "y.rs"] }])
        );
        assert_eq!(v["limited"], json!({ "shown": 1, "total": 2 }));
    }

    #[test]
    fn rows_takes_a_dotted_path() {
        let report = json!({ "ok": true, "plan": { "n": 2, "steps": [{ "s": 1 }, { "s": 2 }] } });
        let o = with(|o| {
            o.rows = Some("plan.steps".into());
            o.lines = true;
        });
        assert_eq!(
            out(report, Shape::One, Style::Pretty, &o),
            "{\"ok\":true,\"plan\":{\"n\":2}}\n{\"s\":1}\n{\"s\":2}\n"
        );
    }

    #[test]
    fn rows_is_refused_on_a_row_report_naming_its_row_field() {
        let o = with(|o| o.rows = Some("deps".into()));
        let report = json!({ "total": 3, "items": rows3() });
        let e = emit(report, FIELD, Style::Pretty, &o).unwrap_err();
        assert_eq!(tag(&e), "validation");
        assert_eq!(
            format!("{e:#}"),
            "`--rows` applies to a single-object report; this report's rows are already its `items` field"
        );
        assert!(err(rows3(), TOP, &o).ends_with("this report is already a row array"));
    }

    #[test]
    fn rows_must_name_one_array() {
        let msg = |path: &str| {
            let o = with(|o| o.rows = Some(path.into()));
            let e = emit(show_report(), Shape::One, Style::Pretty, &o).unwrap_err();
            assert_eq!(tag(&e), "validation");
            format!("{e:#}")
        };
        assert_eq!(
            msg("title"),
            "--rows path `title` is not an array (found string)"
        );
        assert!(
            msg("nope").starts_with(
                "--rows path `nope` matches no field; available fields: id, title, deps"
            )
        );
        assert!(msg("deps.*.files").contains("`*` is not allowed"));
    }

    #[test]
    fn header_shapes_the_report_minus_its_rows() {
        let report = json!({ "verdict": "novel", "dedup_id": "d1", "items": rows3() });
        let o = with(|o| {
            o.header = true;
            o.get = Some("verdict".into());
        });
        assert_eq!(out(report.clone(), FIELD, Style::Pretty, &o), "novel\n");
        let o = with(|o| o.header = true);
        assert_eq!(
            out(report.clone(), FIELD, Style::Compact, &o),
            "{\"verdict\":\"novel\",\"dedup_id\":\"d1\"}\n"
        );
        let o = with(|o| {
            o.header = true;
            o.get = Some("id".into());
        });
        assert!(err(report, FIELD, &o).starts_with("--get path `id` matches no field"));
    }

    #[test]
    fn header_is_refused_on_a_bare_row_array_and_a_single_report() {
        let o = with(|o| o.header = true);
        let e = emit(rows3(), TOP, Style::Pretty, &o).unwrap_err();
        assert_eq!(tag(&e), "validation");
        assert_eq!(format!("{e:#}"), HEADER_ON_TOP);
        let e = emit(json!({ "a": 1 }), Shape::One, Style::Pretty, &o).unwrap_err();
        assert_eq!(tag(&e), "validation");
        assert_eq!(format!("{e:#}"), HEADER_ON_ONE);
    }

    #[test]
    fn where_filters_rows_before_limit_counts_them() {
        let o = with(|o| {
            o.filters.where_not = vec!["ref=a".into()];
            o.limit = Some(1);
            o.get = Some("id".into());
        });
        let e = emit(rows3(), TOP, Style::Pretty, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap(), "2\n");
        assert_eq!(e.notice.as_deref(), Some("tomlctl: showing 1 of 2 rows"));
        let o = with(|o| {
            o.filters.where_eq = vec!["deps=2".into()];
            o.get = Some("ref".into());
        });
        assert_eq!(out(rows3(), TOP, Style::Pretty, &o), "a\n");
        let o = with(|o| {
            o.rows = Some("deps".into());
            o.filters.where_eq = vec!["files=z.rs".into()];
            o.get = Some("ref".into());
        });
        assert_eq!(out(show_report(), Shape::One, Style::Pretty, &o), "b\n");
    }

    #[test]
    fn where_on_a_single_report_without_rows_names_rows() {
        let o = with(|o| o.filters.where_has = vec!["deps".into()]);
        let e = emit(show_report(), Shape::One, Style::Pretty, &o).unwrap_err();
        assert_eq!(tag(&e), "validation");
        let msg = format!("{e:#}");
        assert!(
            msg.starts_with("`--where-has` applies to row reports"),
            "{msg}"
        );
        assert!(msg.contains("--rows"), "{msg}");
        let (e, warning) = compact_lenient(&json!({ "ok": true }), Shape::One, &o).unwrap();
        assert_eq!(String::from_utf8(e.stdout).unwrap(), "{\"ok\":true}\n");
        assert!(
            warning
                .unwrap()
                .contains("`--where-has` applies to row reports")
        );
    }

    #[test]
    fn wildcard_get_prints_each_match() {
        let o = with(|o| o.get = Some("deps.*.ref".into()));
        assert_eq!(out(show_report(), Shape::One, Style::Pretty, &o), "a\nb\n");
        let o = with(|o| o.get = Some("deps.*.files".into()));
        assert_eq!(
            out(show_report(), Shape::One, Style::Pretty, &o),
            "[\"x.rs\",\"y.rs\"]\n[\"z.rs\"]\n"
        );
        let o = with(|o| {
            o.rows = Some("deps".into());
            o.get = Some("files.*".into());
        });
        assert_eq!(
            out(show_report(), Shape::One, Style::Pretty, &o),
            "x.rs\ny.rs\nz.rs\n"
        );
        let o = with(|o| o.get = Some("deps.ref".into()));
        assert!(
            err(show_report(), Shape::One, &o)
                .starts_with("--get path `deps.ref` matches no field")
        );
    }

    #[test]
    fn list_verbs_refuse_rows_and_header() {
        let e = refuse_row_flags_on_list(&with(|o| o.rows = Some("deps".into()))).unwrap_err();
        assert_eq!(tag(&e), "validation");
        assert!(format!("{e:#}").starts_with("`--rows` does not apply to a list verb"));
        let e = refuse_row_flags_on_list(&with(|o| o.header = true)).unwrap_err();
        assert!(format!("{e:#}").starts_with("`--header` does not apply to a list verb"));
        refuse_row_flags_on_list(&with(|o| o.limit = Some(1))).unwrap();
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
