//! R21: dispatch — `fn run()`, the `items`/`blocks` sub-dispatchers, plus
//! the stdin/NDJSON argument helpers and the integrity-opts translators
//! that glue clap types to `IntegrityOpts`. Extracted from the former
//! monolithic `cli.rs` so the clap surface (`super::types`) and the
//! output helpers (`crate::output`) each live in their own file.
//!
//! Pure plumbing; no business logic — every `Cmd` / `ItemsOp` / `BlocksOp`
//! arm delegates to `items::` / `blocks::` / `io::` helpers that own the
//! underlying behaviour.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value as JsonValue;
use std::io::{BufRead, IsTerminal, Read};

use super::types::{
    BlocksOp, Cli, Cmd, FEATURES, IntegrityOp, ItemsOp, LegacyShortcuts, QueryArgs,
    ReadIntegrityArgs, SUBCOMMANDS, WriteIntegrityArgs,
};

use crate::blocks::blocks_verify;
use crate::convert::{detable_to_json, maybe_date_coerce, navigate, parse_scalar, set_at_path, toml_to_json};
use crate::dedup::{
    items_find_duplicates, items_find_duplicates_across, items_find_duplicates_across_json,
    items_find_duplicates_json,
};
use crate::integrity::{IntegrityOpts, refresh_sidecar, sidecar_path, verify_integrity};
use crate::io::{
    guard_write_path, mutate_doc, mutate_doc_conditional, mutate_doc_plan, read_doc,
    read_doc_borrowed, read_doc_either, read_toml_str, recheck_claude_containment,
    warn_if_read_outside_claude, with_exclusive_lock,
};
use crate::items::{
    AddManyOutcome, AddOutcome, array_append, compute_apply_mutation, compute_backfill_mutation,
    compute_remove_mutation, dedup_id_disabled, items_add_many, items_add_many_with_dedupe,
    items_add_to, items_add_value_with_dedupe_to, items_get_from, items_get_from_json,
    items_infer_and_next_id, items_next_id, items_update_to, parse_ndjson,
};
use crate::orphans::items_orphans;
use crate::output::{emit_dry_run_plan, emit_list_raw, print_json, print_json_compact, print_raw_value};
use crate::query::{self, Query, ShapeDispatch};

/// Maximum JSON payload accepted from stdin via the `-` sentinel. 32 MiB is
/// well above any realistic review-ledger / flow-context apply-ops payload
/// (typical is < 64 KiB) while being small enough to fail fast if a caller
/// accidentally pipes a log or a binary into `--json -`.
const MAX_STDIN_BYTES: u64 = 32 * 1024 * 1024;

/// R44: maximum number of ops accepted in a single `items apply` batch.
/// `MAX_STDIN_BYTES` alone does not bound op count — a well-formed 32 MiB
/// JSON array of tiny `{"op":"update","id":"Rx"}` records can hold tens of
/// thousands of operations, and `items_apply_to_opts` iterates serially.
/// 10_000 is far above any legitimate batch (typical ledgers have ~50 items
/// and typical apply batches ≤ 60 ops) while still bounded enough that an
/// accidental loop-generated mega-payload fails fast instead of timing out
/// the wrapping shell.
const MAX_OPS_PER_APPLY: usize = 10_000;

/// R32: guard against multiple `-` sentinels consuming stdin in a single
/// invocation (e.g. `--json - --ops -`). The second `read_json_arg("-")` call
/// errors out instead of silently returning an empty string (stdin already at
/// EOF) and corrupting the apply.
///
/// R38: the flag is deliberately a process-global `AtomicBool`:
///
/// - A CLI invocation is exactly one OS process with exactly one stdin
///   handle. "Multiple invocations" means multiple processes, each with
///   their own flag — so the global is semantically scoped to the right
///   thing at runtime.
/// - Threading an `&mut bool` through `run()` → every dispatcher → every
///   `read_json_arg` / `read_json_value_from_arg` call site would touch
///   ~12 functions for no runtime benefit (the flag's "global" reach is
///   already the whole process).
///
/// **Test contract**: unit tests that flip or rely on this flag (e.g.
/// `read_json_arg_dash_second_call_errors_already_consumed`) MUST hold
/// `env_lock()` for the duration of the test. `cargo test` parallelises
/// within a process, so without the lock two tests touching stdin would
/// race on the single flag. The lock is the test-side substitute for the
/// per-invocation isolation the real CLI gets for free.
static STDIN_CONSUMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Resolve an NDJSON source argument. A literal dash reads stdin via
/// `read_json_arg` (preserving the STDIN_CONSUMED guard against a second
/// `-` sentinel on the same invocation); any other value is a file path
/// read verbatim with `fs::read_to_string`. Extracted (R84) so the
/// identical resolution logic doesn't live in both `Cmd::ArrayAppend` and
/// `ItemsOp::AddMany`.
fn read_ndjson_source(src: &str) -> Result<String> {
    if src == "-" {
        read_json_arg("-")
    } else {
        std::fs::read_to_string(src)
            .with_context(|| format!("reading NDJSON file `{}`", src))
    }
}

/// T5: parse the `--dedupe-by` flag value into a `Vec<String>` of field
/// paths. `None` (flag absent) returns an empty Vec — the caller treats
/// that as "dedupe off" and the existing add/add-many code paths run
/// unchanged. `Some("")` or `Some(",,")` (all-empty after split-and-trim)
/// is a fail-loud case: the user typed the flag with no payload, which
/// almost certainly isn't what they meant; we error with a directed
/// message instead of silently disabling dedup.
fn parse_dedupe_fields(raw: Option<&str>) -> Result<Vec<String>> {
    let Some(s) = raw else {
        return Ok(Vec::new());
    };
    let fields: Vec<String> = s
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();
    if fields.is_empty() {
        bail!("--dedupe-by requires at least one field name");
    }
    Ok(fields)
}

/// Resolve a JSON argument: if it's literally "-", read stdin to a String.
/// Otherwise return the argument as-is.
///
/// Stdin handling (R7):
///
/// - Refuses to block on an interactive TTY — a user piping nothing into
///   `--json -` would otherwise hang forever with no prompt or feedback.
/// - Caps the read at `MAX_STDIN_BYTES`; an oversize payload is truncated
///   on the input side so a misrouted log doesn't balloon tomlctl's heap.
fn read_json_arg(arg: &str) -> Result<String> {
    if arg == "-" {
        // R32: a single invocation can only consume stdin once. A second `-`
        // sentinel would read an already-drained handle and silently return an
        // empty payload, which downstream would treat as a no-op or a parse
        // error with a confusing message. `swap(true, SeqCst)` is both the
        // check and the mark, so concurrent calls can't both see `false`.
        if STDIN_CONSUMED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            bail!(
                "stdin already consumed by another flag on this invocation; only one --json/--ops/--ndjson/--defaults-json flag can use the `-` sentinel per call"
            );
        }
        if std::io::stdin().is_terminal() {
            bail!(
                "stdin is a TTY — pipe JSON (e.g. `cat payload.json | tomlctl … --json -`) or pass `--json '<literal>'`"
            );
        }
        let mut buf = String::new();
        std::io::stdin()
            .lock()
            .take(MAX_STDIN_BYTES)
            .read_to_string(&mut buf)
            .context("reading JSON from stdin")?;
        if buf.trim().is_empty() {
            bail!("stdin was empty — expected JSON payload");
        }
        Ok(buf)
    } else {
        Ok(arg.to_string())
    }
}

/// O35: parse a JSON `--json`/`--ops`/`--defaults-json` argument directly
/// into a `JsonValue`, skipping the intermediate `String` allocation that
/// the `read_json_arg` + `serde_json::from_str(&s)` two-step would incur.
///
/// Mirrors `read_json_arg`'s stdin discipline exactly:
///
/// - Honours STDIN_CONSUMED (R32): a second `-` sentinel on the same
///   invocation bails with the identical "already consumed" message.
/// - Refuses to block on a TTY (R7) with the identical guidance message.
/// - Caps the read at `MAX_STDIN_BYTES` via the same `take(...)` wrapper.
/// - Reports the same "stdin was empty — expected JSON payload" error when
///   stdin closes immediately, rather than letting serde surface its own
///   EOF message (which would silently change the public-facing error
///   text).
///
/// Callers add their own per-flag `.with_context("parsing --json"|"parsing
/// --ops"|"parsing --defaults-json")` so the user-visible error chain stays
/// byte-identical to the pre-O35 behaviour where each call site wrapped
/// `serde_json::from_str(&text).context("parsing --<flag>")`.
fn read_json_value_from_arg(arg: &str) -> Result<JsonValue> {
    if arg == "-" {
        // R32: identical swap-and-mark check as `read_json_arg`.
        if STDIN_CONSUMED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            bail!(
                "stdin already consumed by another flag on this invocation; only one --json/--ops/--ndjson/--defaults-json flag can use the `-` sentinel per call"
            );
        }
        if std::io::stdin().is_terminal() {
            bail!(
                "stdin is a TTY — pipe JSON (e.g. `cat payload.json | tomlctl … --json -`) or pass `--json '<literal>'`"
            );
        }
        let stdin = std::io::stdin();
        let lock = stdin.lock();
        let mut r = std::io::BufReader::new(lock.take(MAX_STDIN_BYTES));
        // Preserve the "stdin was empty" sentinel: peek the first buffered
        // chunk; if it never arrives, stdin closed before sending anything
        // and we want our own message rather than serde's EOF wording.
        let initial = r.fill_buf().context("reading JSON from stdin")?;
        if initial.is_empty() {
            bail!("stdin was empty — expected JSON payload");
        }
        // `from_reader` consumes the BufReader's internal buffer before
        // refilling from the underlying `Take<StdinLock>`, so the peek
        // above does not strand any bytes.
        Ok(serde_json::from_reader(r)?)
    } else {
        Ok(serde_json::from_str(arg)?)
    }
}

/// Translate the flattened integrity-args structs from a subcommand variant
/// into the module-local `IntegrityOpts` bundle. Kept next to the CLI
/// definition (rather than in `integrity.rs`) so the integrity module stays
/// free of the clap-derived types. R74 split `IntegrityArgs` in two — read
/// paths hand us `ReadIntegrityArgs` (only `verify_integrity` matters), write
/// paths hand us `WriteIntegrityArgs` (the full set). Both flow through the
/// same `IntegrityOpts` so every downstream consumer
/// (`maybe_verify_integrity` / `write_toml_with_sidecar`) stays unchanged.
fn read_integrity_opts(args: &ReadIntegrityArgs) -> IntegrityOpts {
    IntegrityOpts {
        // Read-side paths never write a sidecar; default to true so that if
        // a future refactor funnels the same opts into a writer we don't
        // accidentally suppress the sidecar. `write_toml_with_sidecar` is
        // only reached on write paths, which use `write_integrity_opts`.
        write_sidecar: true,
        verify_on_read: args.verify_integrity,
        // Read paths never hit the sidecar-write failure branch, so `strict`
        // has no effect here. Pin it `false` so the opt's semantics stay
        // predictable if the struct is ever inspected after the read.
        strict: false,
    }
}

/// T9: single gate applied at every read dispatch site so `--strict-read`
/// fires BEFORE `read_doc` (and therefore before `maybe_verify_integrity`).
/// This is the ordering guarantee documented in the README's "File state
/// contract" subsection: a missing file under `--strict-read
/// --verify-integrity` surfaces `kind=not_found`, not `kind=integrity`.
///
/// Called at every dispatch arm that flattens `ReadIntegrityArgs`. On
/// paths whose default already errors on missing file (everything except
/// `items next-id --prefix <P>`) the call is a defensive duplicate — a
/// benign extra stat before the real read — and the downstream
/// `read_toml` NotFound tag would fire regardless. Keeping it centralised
/// means future read arms that add a "missing → silent default" fast path
/// (e.g. an eventual `items list --or-default '[]'`) inherit the gate for
/// free.
fn strict_read_check(file: &std::path::Path, strict_read: bool) -> Result<()> {
    if !strict_read || file.exists() {
        return Ok(());
    }
    Err(crate::errors::tagged_err(
        crate::errors::ErrorKind::NotFound,
        Some(file.to_path_buf()),
        format!("file does not exist: {}", file.display()),
    ))
}

fn write_integrity_opts(args: &WriteIntegrityArgs) -> IntegrityOpts {
    IntegrityOpts {
        write_sidecar: !args.no_write_integrity,
        verify_on_read: args.verify_integrity,
        strict: args.strict_integrity,
    }
}

/// R15: trivial field-copy adapter from the two clap-derive types
/// (`LegacyShortcuts`, `QueryArgs`) into the POD `QueryInput` that
/// `query.rs` owns. Lives here — on the cli side of the module boundary —
/// so `query.rs` stays free of any `use crate::cli` import. This is
/// intentionally pure plumbing: every field either `.clone()`s the owned
/// value off `QueryArgs` (the clap-derive layer already holds the
/// `String` / `Vec<String>` / `Option<String>`) or clones out of the
/// `&Option<String>` references on `LegacyShortcuts`. If any logic creeps
/// into this function, move it to `Query::from_query_input` in `query.rs`
/// instead — the POD type's whole job is to keep the cli/query boundary
/// a straight-line data transfer.
fn query_input_from_cli(
    legacy: &LegacyShortcuts<'_>,
    q: &QueryArgs,
) -> crate::query::QueryInput {
    crate::query::QueryInput {
        status: legacy.status.clone(),
        category: legacy.category.clone(),
        file: legacy.file.clone(),
        newer_than: legacy.newer_than.clone(),
        count: legacy.count,
        where_eq: q.where_eq.clone(),
        where_not: q.where_not.clone(),
        where_in: q.where_in.clone(),
        where_has: q.where_has.clone(),
        where_missing: q.where_missing.clone(),
        where_gt: q.where_gt.clone(),
        where_gte: q.where_gte.clone(),
        where_lt: q.where_lt.clone(),
        where_lte: q.where_lte.clone(),
        where_contains: q.where_contains.clone(),
        where_prefix: q.where_prefix.clone(),
        where_suffix: q.where_suffix.clone(),
        where_regex: q.where_regex.clone(),
        select: q.select.clone(),
        exclude: q.exclude.clone(),
        pluck: q.pluck.clone(),
        sort_by: q.sort_by.clone(),
        limit: q.limit,
        offset: q.offset,
        distinct: q.distinct,
        group_by: q.group_by.clone(),
        count_by: q.count_by.clone(),
        count_distinct: q.count_distinct.clone(),
        ndjson: q.ndjson,
        lines: q.lines,
        raw: q.raw,
    }
}

/// Top-level dispatch entrypoint. `main.rs` is a one-line wrapper over
/// this; splitting lets the binary target stay trivially small while all
/// the parsing/dispatch/output plumbing lives in a normal module.
///
/// R18: the `Cli` is parsed once in `main.rs` and threaded in here. This
/// eliminates the earlier double-parse (peek via `try_parse()` for
/// `--error-format`, then a full `Cli::parse()` on entry) which silently
/// swallowed errors on the peek path and risked double `--help` rendering.
pub(crate) fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Parse { file, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            // O10: `parse` is the single dispatch arm whose whole output is
            // "the entire TOML doc as JSON" — no dotted-path navigation, no
            // per-item filtering — so it benefits most from the borrowed
            // DeTable fast-path that skips the per-scalar `String` clone
            // done inside `toml::from_str::<TomlValue>`. When
            // `--verify-integrity` is requested we still need the shared
            // lock + sidecar verify dance from `read_doc`, so the owned
            // path is retained for that case. All other read dispatch arms
            // (`get`, `validate`, every `items *` op) stay on the owned
            // path — they either need `navigate` / TomlValue-level helpers
            // or the borrowed-lifetime plumbing doesn't yet cover their
            // downstream consumers.
            let out = if opts.verify_on_read {
                read_doc(&file, opts, |doc| Ok(toml_to_json(doc)))?
            } else {
                let source = read_toml_str(&file)?;
                read_doc_borrowed(&source, |table| Ok(detable_to_json(table)))?
            };
            print_json(&out)?;
        }
        Cmd::Get { file, path, raw, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            let out = read_doc(&file, opts, |doc| {
                Ok(match path.as_deref() {
                    None | Some("") => toml_to_json(doc),
                    Some(p) => toml_to_json(
                        navigate(doc, p).ok_or_else(|| anyhow!("key path `{}` not found", p))?,
                    ),
                })
            })?;
            if raw {
                // T2: bare-scalar emit. `emit_raw` validates the value is a
                // scalar (string / number / bool) and errors byte-for-byte
                // on table/array targets. Null is impossible here — `navigate`
                // returns `None` for a missing path, which we already
                // surface as "key path not found" above; a present TOML
                // scalar cannot map to JSON null via `toml_to_json`.
                print_raw_value(&out)?;
            } else {
                print_json(&out)?;
            }
        }
        Cmd::Set {
            file,
            path,
            value,
            ty,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                let v = parse_scalar(&value, ty)?;
                set_at_path(doc, &path, v)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        Cmd::SetJson { file, path, json, integrity } => {
            let opts = write_integrity_opts(&integrity);
            // O35: parse stdin/literal JSON straight into a `JsonValue`,
            // skipping the intermediate String allocation. The parse moves
            // out of the `mutate_doc` closure, which is a side-benefit:
            // a malformed payload now fails before we open the doc.
            let parsed: JsonValue = read_json_value_from_arg(&json).context("parsing --json")?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                let last_key = path.rsplit_once('.').map(|(_, k)| k).unwrap_or(path.as_str());
                let v = maybe_date_coerce(last_key, &parsed)?;
                set_at_path(doc, &path, v)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        Cmd::Validate { file, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            read_doc(&file, opts, |_doc| Ok(()))?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        Cmd::Items { op } => items_dispatch(op)?,
        Cmd::Blocks { op } => blocks_dispatch(op)?,
        Cmd::ArrayAppend {
            file,
            array,
            json,
            ndjson,
            integrity,
        } => {
            // clap's `conflicts_with` guarantees at most one is set; enforce
            // "at least one" here since clap has no first-class
            // required-exactly-one primitive on optional flags.
            if json.is_none() && ndjson.is_none() {
                bail!("array-append requires one of --json or --ndjson");
            }
            let opts = write_integrity_opts(&integrity);
            let rows: Vec<JsonValue> = if let Some(j) = json {
                // O35: parse straight to `JsonValue`, dropping the prior
                // `read_json_arg` String + `serde_json::from_str` two-step.
                let parsed: JsonValue =
                    read_json_value_from_arg(&j).context("parsing --json")?;
                if !parsed.is_object() {
                    bail!("--json must be a JSON object");
                }
                vec![parsed]
            } else {
                let nd = ndjson.expect("checked above");
                let text = read_ndjson_source(&nd)?;
                parse_ndjson(&text)?
            };
            let mut appended: usize = 0;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                appended = array_append(doc, &array, &rows)?;
                Ok(())
            })?;
            print_json_compact(&serde_json::json!({"ok": true, "appended": appended}))?;
        }
        Cmd::Integrity { op } => integrity_dispatch(op)?,
        Cmd::Capabilities => {
            // T7: pretty-print matches the rest of the read-path surface
            // (`parse`, `get`, `items list`) — `print_json` is the same
            // helper they use. The `version` string is resolved at compile
            // time via `env!("CARGO_PKG_VERSION")`, so it tracks the
            // Cargo.toml bump automatically on the next rebuild. `FEATURES`
            // and `SUBCOMMANDS` are static consts at module scope — see
            // their docstrings for the drift contract.
            let output = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "features": FEATURES,
                "subcommands": SUBCOMMANDS,
            });
            print_json(&output)?;
        }
    }
    Ok(())
}

fn items_dispatch(op: ItemsOp) -> Result<()> {
    match op {
        ItemsOp::List {
            file,
            status,
            category,
            newer_than,
            file_filter,
            count,
            array,
            query,
            integrity,
        } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            let legacy = LegacyShortcuts {
                status: &status,
                category: &category,
                file: &file_filter,
                newer_than: &newer_than,
                count,
            };
            let q = Query::from_query_input(&query_input_from_cli(&legacy, &query))?;
            // R82: `ndjson` is an output-encoding choice, not a shape. Only
            // the Array and Pluck shape + ndjson encoding combinations are
            // meaningful; for aggregation shapes (Count/CountBy/
            // CountDistinct/GroupBy) the ndjson bit is silently ignored
            // since the output is a single JSON value that has no per-line
            // decomposition.
            //
            // T3: added Pluck to the streaming-eligible set. `--pluck f
            // --lines` (or `--pluck f --ndjson`) streams one plucked JSON
            // value per line; `run_streaming` mirrors `apply_pluck`'s
            // null/missing-drop so the set of emitted values is identical
            // to the non-streaming path.
            if q.ndjson && q.shape.is_streamable() {
                // O34: stream one compact JSON value per line directly via
                // `query::run_streaming`, avoiding the `Vec<JsonValue>` that
                // `query::run` would otherwise materialise only for us to
                // iterate and re-serialise. The streaming path walks the
                // same pipeline and emits per-item — peak memory scales with
                // the filtered set, not the full output array.
                //
                // T2: `--pluck foo --lines --raw` flows through here too;
                // `run_streaming` reads `q.raw` and emits bare values per
                // line instead of quoted JSON. The Array variant of this
                // branch (full-row ndjson) does not honour `--raw` — each
                // row is a JSON object, not a scalar — and that combo has
                // no meaningful raw form. `validate_query` does not reject
                // it (Array + raw is a no-op, not an error) for the same
                // reason `--lines` on Count is a silent no-op: agents
                // blanket-add flags, and inducing an error for an
                // ambiguous-but-harmless combo would be user-hostile.
                use std::io::Write;
                let stdout = std::io::stdout();
                let mut h = stdout.lock();
                read_doc(&file, opts, |doc| query::run_streaming(doc, &array, &q, &mut h))?;
                h.flush()?;
            } else {
                let out = read_doc(&file, opts, |doc| query::run(doc, &array, &q))?;
                if q.raw {
                    emit_list_raw(&out, &q.shape)?;
                } else {
                    print_json(&out)?;
                }
            }
        }
        ItemsOp::Get { file, id, array, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            let out = read_doc_either(
                &file,
                opts,
                |doc| items_get_from(doc, &array, &id),
                |doc| items_get_from_json(doc, &array, &id),
            )?;
            print_json(&out)?;
        }
        ItemsOp::Add { file, json, array, dedupe_by, integrity } => {
            let opts = write_integrity_opts(&integrity);
            let dedupe_fields = parse_dedupe_fields(dedupe_by.as_deref())?;
            if dedupe_fields.is_empty() {
                // No-dedupe path: byte-identical behaviour to pre-T5 —
                // same helper, same `{"ok":true}` output, same
                // `mutate_doc` (always-write) pipeline. The T5 plan
                // suggested emitting `{"ok":true,"added":1}` even in the
                // no-dedupe case, but that would break the byte-identity
                // constraint ("absent --dedupe-by → current behaviour
                // byte-identical") since today's shape is plain
                // `{"ok":true}`. Keep the legacy shape for back-compat
                // and reserve the enriched shape for the `--dedupe-by`
                // branch below.
                let json = read_json_arg(&json)?;
                mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                    items_add_to(doc, &array, &json)
                })?;
                print_json_compact(&serde_json::json!({"ok": true}))?;
            } else {
                // Dedupe path: parse JSON once up-front so we can feed it
                // to the pre-scan inside the lock without a re-parse.
                // `mutate_doc_conditional` elides the write-and-sidecar
                // bump when the scan returns a match; the caller sees
                // `added:0,matched_id:...` and the on-disk file + sidecar
                // are untouched.
                let patch: JsonValue =
                    read_json_value_from_arg(&json).context("parsing --json")?;
                let mut outcome: Option<AddOutcome> = None;
                mutate_doc_conditional(&file, integrity.allow_outside, opts, |doc| {
                    let result = items_add_value_with_dedupe_to(
                        doc,
                        patch,
                        &array,
                        &dedupe_fields,
                    )?;
                    let mutated = matches!(result, AddOutcome::Added);
                    outcome = Some(result);
                    Ok(mutated)
                })?;
                match outcome.expect("closure always sets outcome on success") {
                    AddOutcome::Added => {
                        print_json_compact(&serde_json::json!({"ok": true, "added": 1}))?;
                    }
                    AddOutcome::Skipped { matched_id } => {
                        print_json_compact(&serde_json::json!({
                            "ok": true,
                            "added": 0,
                            "matched_id": matched_id,
                        }))?;
                    }
                }
            }
        }
        ItemsOp::AddMany {
            file,
            ndjson,
            defaults_json,
            array,
            dedupe_by,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            let dedupe_fields = parse_dedupe_fields(dedupe_by.as_deref())?;
            // NDJSON source resolution factored into `read_ndjson_source` (R84);
            // the STDIN_CONSUMED guard in `read_json_arg` still refuses a second
            // `-` when `--defaults-json -` also wants stdin on the same call.
            let ndjson_text = read_ndjson_source(&ndjson)?;
            let rows = parse_ndjson(&ndjson_text)?;
            let defaults: Option<JsonValue> = match defaults_json.as_deref() {
                // O35: parse straight to `JsonValue`, dropping the prior
                // `read_json_arg` String + `serde_json::from_str` two-step.
                Some(s) => Some(
                    read_json_value_from_arg(s).context("parsing --defaults-json")?,
                ),
                None => None,
            };
            if dedupe_fields.is_empty() {
                // No-dedupe path: byte-identical to pre-T5. Same helper,
                // same output shape (`{"ok":true,"added":N}`), same
                // always-write pipeline.
                let mut added: usize = 0;
                mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                    added = items_add_many(doc, &array, &rows, defaults.as_ref())?;
                    Ok(())
                })?;
                print_json_compact(&serde_json::json!({"ok": true, "added": added}))?;
            } else {
                // Dedupe path: run the pre-scan + append loop inside the
                // lock via `mutate_doc_conditional`. Skip the file write
                // entirely when the batch added zero rows — the doc is
                // untouched and the sidecar must not bump for a pure-
                // skip batch. Any `added > 0` takes the write branch.
                let mut outcome: Option<AddManyOutcome> = None;
                mutate_doc_conditional(&file, integrity.allow_outside, opts, |doc| {
                    let result = items_add_many_with_dedupe(
                        doc,
                        &array,
                        &rows,
                        defaults.as_ref(),
                        &dedupe_fields,
                    )?;
                    let mutated = result.added > 0;
                    outcome = Some(result);
                    Ok(mutated)
                })?;
                let outcome = outcome.expect("closure always sets outcome on success");
                let skipped_rows_json: Vec<JsonValue> = outcome
                    .skipped_rows
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "row": s.row,
                            "matched_id": s.matched_id,
                        })
                    })
                    .collect();
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "added": outcome.added,
                    "skipped": outcome.skipped_rows.len(),
                    "skipped_rows": skipped_rows_json,
                }))?;
            }
        }
        ItemsOp::Update {
            file,
            id,
            json,
            unset,
            array,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            let json = read_json_arg(&json)?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_update_to(doc, &array, &id, &json, &unset)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        ItemsOp::Remove { file, id, array, dry_run, integrity } => {
            let opts = write_integrity_opts(&integrity);
            if dry_run {
                // T10: dry-run path — compute the plan on a locally-read
                // doc (no exclusive lock) and emit the would_change
                // summary. The compute phase runs the same validation
                // as the live path (`compute_remove_mutation` delegates
                // to `items_remove_from` on a cloned doc), so a missing
                // id bails with the identical "no item with id = X"
                // error a real remove would surface.
                let read_opts = IntegrityOpts {
                    write_sidecar: false,
                    verify_on_read: integrity.verify_integrity,
                    strict: false,
                };
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_remove_mutation(doc, &array, &id)
                })?;
                emit_dry_run_plan(&plan)?;
            } else {
                // Live path: compute + apply via the split helpers so
                // the "live" and "dry-run" branches share the compute
                // stage byte-for-byte. The read happens inside the
                // exclusive lock via `mutate_doc_plan` so the same
                // TOCTOU narrowing as `mutate_doc` holds.
                mutate_doc_plan(&file, integrity.allow_outside, opts, |doc| {
                    compute_remove_mutation(doc, &array, &id)
                })?;
                print_json_compact(&serde_json::json!({"ok": true}))?;
            }
        }
        ItemsOp::Apply { file, ops, array, no_remove, dry_run, integrity } => {
            let opts = write_integrity_opts(&integrity);
            let ops = read_json_arg(&ops)?;
            // R44: bound the ops count at the CLI boundary. `MAX_STDIN_BYTES`
            // only caps the raw payload size; a 32 MiB JSON array of minimal
            // `{"op":"update","id":"Rx"}` records can still hold tens of
            // thousands of ops, which `items_apply_to_opts` iterates serially.
            // Check length here (before locking + parsing inside the mutator)
            // so an over-large payload fails fast with a directed message,
            // and the user-visible error predates any disk mutation.
            // T10: the check also gates `--dry-run`, so an over-large preview
            // refuses with the same message a real run would emit.
            let parsed_for_count: JsonValue =
                serde_json::from_str(&ops).context("parsing --ops")?;
            if let JsonValue::Array(arr) = &parsed_for_count
                && arr.len() > MAX_OPS_PER_APPLY
            {
                bail!(
                    "--ops contains {} operations, which exceeds the cap of {}; \
                     split the batch into smaller /review-apply or /optimise-apply \
                     invocations",
                    arr.len(),
                    MAX_OPS_PER_APPLY
                );
            }
            if dry_run {
                // T10: same compute phase as the live path, but we stop
                // before the I/O stage. `compute_apply_mutation` runs
                // `items_apply_to_opts` on a cloned doc, so every
                // validation gate — `--no-remove`, op-shape, missing id,
                // dedup_id auto-populate — fires with a byte-identical
                // error surface.
                let read_opts = IntegrityOpts {
                    write_sidecar: false,
                    verify_on_read: integrity.verify_integrity,
                    strict: false,
                };
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_apply_mutation(doc, &array, &ops, no_remove)
                })?;
                emit_dry_run_plan(&plan)?;
            } else {
                mutate_doc_plan(&file, integrity.allow_outside, opts, |doc| {
                    compute_apply_mutation(doc, &array, &ops, no_remove)
                })?;
                print_json_compact(&serde_json::json!({"ok": true}))?;
            }
        }
        ItemsOp::NextId { file, prefix, infer_from_file, integrity } => {
            // The clap ArgGroup `id_source` guarantees exactly one of
            // `--prefix` / `--infer-from-file` reaches us; no runtime
            // "both unset" or "both set" check is needed.
            //
            // T9: `--strict-read` fires BEFORE R19's missing-file fast path,
            // so a caller who opted out of the bootstrap default on this
            // subcommand gets `kind=not_found` instead of the `"<prefix>1"`
            // fallback. `strict_read_check` returns `Ok(())` when the flag
            // is absent OR the file exists, so the default (non-strict)
            // invocation flows straight into the R19 branch below unchanged.
            strict_read_check(&file, integrity.strict_read)?;
            // R19: if the target ledger doesn't exist yet, there's nothing to
            // parse or verify — the "next" id is trivially `<prefix>1`. This
            // lets flows call `items next-id` before the ledger is initialised
            // (e.g. during bootstrap of a new flow directory). When the caller
            // passed `--infer-from-file` and the file is absent, inference has
            // no corpus to work from, which is indistinguishable from the
            // "empty ledger" failure case — surface the same error so the
            // caller's remediation is the same either way.
            if !file.exists() {
                if infer_from_file {
                    bail!("--infer-from-file requires a non-empty ledger or explicit --prefix");
                }
                let prefix = prefix.as_deref().expect("clap required_unless_present guarantees prefix is Some when infer_from_file is false");
                // R26: route the missing-file prefix validation through
                // `items_next_id` on an empty doc so the empty-prefix and
                // all-digit-prefix rejections are tagged `ErrorKind::Validation`
                // consistently with the file-exists branch below. Prior to
                // the extraction this branch used bare `bail!` → `kind=other`
                // in `--error-format json`, producing different kinds for the
                // same input depending on whether the ledger existed.
                let empty_doc = toml::Value::Table(toml::Table::new());
                let id = items_next_id(&empty_doc, prefix)?;
                print_json_compact(&serde_json::Value::from(id))?;
            } else {
                let opts = read_integrity_opts(&integrity);
                let id = read_doc(&file, opts, |doc| {
                    if infer_from_file {
                        items_infer_and_next_id(doc)
                    } else {
                        let prefix =
                            prefix.as_deref().expect("clap required_unless_present guarantees prefix is Some when infer_from_file is false");
                        items_next_id(doc, prefix)
                    }
                })?;
                print_json_compact(&serde_json::Value::from(id))?;
            }
        }
        ItemsOp::FindDuplicates { file, tier, across, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            if let Some(other) = across.as_ref() {
                strict_read_check(other, integrity.strict_read)?;
                // R38: the `--across` path is a new read surface added by T6c;
                // unlike the primary ledger (which flows through the write-side
                // `guard_write_path` before any mutation), the cross-ledger
                // read has no containment check. A caller passing
                // `--across <arbitrary.toml>` could coax tomlctl into reading
                // any file the process can see, and the TOML parser's error
                // output would echo the path + a caret snippet of the content
                // — a parsing oracle. Advisory warn only (matches the
                // `--allow-outside` spirit on the write side); we don't refuse
                // the read because legitimate cross-repo comparisons exist.
                warn_if_read_outside_claude(other);
            }
            let opts = read_integrity_opts(&integrity);
            let groups = match across {
                None => read_doc_either(
                    &file,
                    opts,
                    |doc| items_find_duplicates(doc, tier),
                    |doc| items_find_duplicates_json(doc, tier),
                )?,
                Some(other_path) => {
                    // T6c: load both ledgers under the same integrity
                    // contract; errors propagate for either. Clone the
                    // primary's items out of the locked closure so the
                    // second read can fire sequentially without nesting
                    // locks (nesting them would risk lock-order inversion
                    // against any concurrent writer).
                    let primary_file = file.to_string_lossy().into_owned();
                    let other_file = other_path.to_string_lossy().into_owned();
                    if opts.verify_on_read {
                        let primary_items: Vec<toml::Value> = read_doc(&file, opts, |doc| {
                            Ok(crate::io::items_array(doc, "items").to_vec())
                        })?;
                        let other_items: Vec<toml::Value> = read_doc(&other_path, opts, |doc| {
                            Ok(crate::io::items_array(doc, "items").to_vec())
                        })?;
                        items_find_duplicates_across(
                            primary_items,
                            &primary_file,
                            other_items,
                            &other_file,
                            tier,
                        )?
                    } else {
                        // O64: borrowed-DeTable fast-path. Both ledgers go
                        // through the borrowed parse + detable_to_json
                        // boundary; the cross-ledger join then runs in
                        // JsonValue space via items_find_duplicates_across_json.
                        let primary_items: Vec<JsonValue> = {
                            let source = read_toml_str(&file)?;
                            read_doc_borrowed(&source, |table| {
                                let json = detable_to_json(table);
                                Ok(crate::io::items_array_json(&json, "items").to_vec())
                            })?
                        };
                        let other_items: Vec<JsonValue> = {
                            let source = read_toml_str(&other_path)?;
                            read_doc_borrowed(&source, |table| {
                                let json = detable_to_json(table);
                                Ok(crate::io::items_array_json(&json, "items").to_vec())
                            })?
                        };
                        items_find_duplicates_across_json(
                            primary_items,
                            &primary_file,
                            other_items,
                            &other_file,
                            tier,
                        )?
                    }
                }
            };
            print_json(&JsonValue::Array(groups))?;
        }
        ItemsOp::Orphans { file, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            let orphans = read_doc(&file, opts, items_orphans)?;
            print_json(&JsonValue::Array(orphans))?;
        }
        ItemsOp::BackfillDedupId { file, array, dry_run, integrity } => {
            // T11: kill-switch short-circuit. Checked at the dispatch
            // boundary (rather than inside `compute_backfill_mutation`) so
            // both live and dry-run paths surface the documented
            // `disabled-by-env` output WITHOUT touching the filesystem —
            // the user's rollback lever should leave no I/O trace. The
            // other funnels (add / update / apply / add-many) check the
            // flag inside the per-funnel hook because the flag only gates
            // the auto-populate side-effect there, not the whole operation.
            if dedup_id_disabled() {
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "backfilled": 0,
                    "reason": "disabled-by-env",
                }))?;
                return Ok(());
            }
            let opts = write_integrity_opts(&integrity);
            // Pre-read outside the exclusive lock to detect the no-op case
            // (every item already has `dedup_id`) so we can skip the lock
            // + rewrite + sidecar bump entirely. The read itself honours
            // `--verify-integrity` under a shared lock via `read_doc`, so
            // the integrity contract stays intact. Benign TOCTOU: if
            // another writer backfills between our pre-read and our
            // in-lock re-compute, the in-lock path just sees fewer items
            // to touch and writes byte-identical bytes — no data
            // corruption, just one redundant write. The common case
            // (genuine no-op) avoids the write altogether.
            let read_opts = IntegrityOpts {
                write_sidecar: false,
                verify_on_read: integrity.verify_integrity,
                strict: false,
            };
            let preview = read_doc(&file, read_opts, |doc| {
                compute_backfill_mutation(doc, &array)
            })?;
            if dry_run {
                // Dry-run: emit the preview and stop — never acquires the
                // exclusive lock, never writes, never bumps the sidecar.
                // `ids` mirrors `plan.updated` verbatim so downstream
                // callers can diff the preview against a later run.
                let summary = serde_json::json!({
                    "ok": true,
                    "dry_run": true,
                    "would_backfill": preview.updated.len(),
                    "ids": preview.updated,
                });
                print_json_compact(&summary)?;
            } else if preview.updated.is_empty() {
                // No-op fast path: skip the write entirely. The sidecar
                // does NOT re-hash, the file mtime does NOT bump, the
                // exclusive lock is never taken — the ledger is
                // byte-identical and the caller sees `backfilled:0`.
                // Mirrors T5's `mutate_doc_conditional` "no-mutation →
                // no-write" contract without needing a new wrapper.
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "backfilled": 0,
                }))?;
            } else {
                // Live path: re-read inside the exclusive lock via
                // `mutate_doc_plan` and recompute. Recomputing (rather
                // than reusing the pre-read plan) closes the TOCTOU
                // window against a concurrent writer. The count we
                // emit comes from the IN-LOCK plan so the output
                // reflects what actually landed on disk, not the
                // pre-read snapshot.
                let mut written: usize = 0;
                mutate_doc_plan(&file, integrity.allow_outside, opts, |doc| {
                    let plan = compute_backfill_mutation(doc, &array)?;
                    written = plan.updated.len();
                    Ok(plan)
                })?;
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "backfilled": written,
                }))?;
            }
        }
    }
    Ok(())
}

fn blocks_dispatch(op: BlocksOp) -> Result<()> {
    match op {
        BlocksOp::Verify { files, block } => {
            let report = blocks_verify(&files, &block)?;
            print_json(&report.report)?;
            if !report.ok {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn integrity_dispatch(op: IntegrityOp) -> Result<()> {
    match op {
        IntegrityOp::Refresh { file, integrity } => {
            // R4: `integrity refresh` flattens `WriteIntegrityArgs` for parity
            // with every other write subcommand, but not every flag has a
            // semantic hook on this sidecar-only operation. Surface the
            // semantically-meaningless ones here so composable wrapper scripts
            // fail loud on the truly broken combination and no-op on the
            // harmless one:
            //
            // - `--no-write-integrity`: refresh IS the sidecar write — making
            //   the flag structurally meaningless. Bail with a directed
            //   message rather than silently no-op (which would leave the
            //   caller convinced the sidecar was refreshed).
            // - `--strict-integrity`: refresh has no sidecar-failure
            //   fallback path to strict-ify (we already fail hard on any
            //   `atomic_write` error). Silently ignore so wrapper scripts
            //   that blanket-add the flag across a mix of write subcommands
            //   don't need to special-case refresh.
            if integrity.no_write_integrity {
                bail!(
                    "--no-write-integrity is meaningless on `integrity refresh` — the subcommand's entire purpose is to write the sidecar"
                );
            }
            let _ = integrity.strict_integrity; // R4: silently ignored; see above.
            let allow_outside = integrity.allow_outside;
            let verify_before_overwrite = integrity.verify_integrity;
            // Take the same exclusive lock any write path would, so a
            // concurrent `tomlctl set` / `items add` observes a consistent
            // (TOML, sidecar) pair rather than overlapping our refresh.
            with_exclusive_lock(&file, || {
                // Containment guard mirrors `mutate_doc`: refuse to write
                // the sidecar for a file outside `.claude/` unless the
                // caller explicitly opts out. A malicious artifacts path
                // could otherwise trick us into writing next to an
                // arbitrary target.
                guard_write_path(&file, allow_outside)?;
                // R4: `--verify-integrity` on refresh means "verify the
                // existing sidecar matches before overwriting". This gates
                // the recovery path against clobbering a mismatched sidecar
                // (e.g. if the TOML was tampered with between the previous
                // write and this refresh, the caller wants to know before
                // the sidecar gets regenerated against the tampered bytes).
                // Missing sidecar → proceed silently; bootstrap is the
                // whole point of this subcommand.
                if verify_before_overwrite && sidecar_path(&file).exists() {
                    verify_integrity(&file)?;
                }
                // R2: in-lock pre-persist containment re-check. Mirrors the
                // mutate_doc O17/R3 pattern — the inside-lock `guard_write_path`
                // above is the primary defence; this call is the belt-and-braces
                // TOCTOU narrowing against a parent-symlink swap between the
                // guard and the `atomic_write` inside `refresh_sidecar`.
                if !allow_outside {
                    recheck_claude_containment(&file)?;
                }
                refresh_sidecar(&file)?;
                Ok(())
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{self, blocks_verify, scan_block_names_warn};
    use crate::test_support::env_lock;
    use std::fs;
    use std::path::Path;

    // ----- blocks verify ---------------------------------------------------

    #[test]
    fn blocks_verify_detects_drift() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        let good = "\
<!-- SHARED-BLOCK:flow-context START -->
line one
line two
<!-- SHARED-BLOCK:flow-context END -->
";
        fs::write(&a, good).unwrap();
        fs::write(&b, good).unwrap();
        let report =
            blocks_verify(&[a.clone(), b.clone()], &["flow-context".to_string()]).unwrap();
        assert!(report.ok, "equal content must be ok");

        let drifted = "\
<!-- SHARED-BLOCK:flow-context START -->
line one
DIFFERENT
<!-- SHARED-BLOCK:flow-context END -->
";
        fs::write(&b, drifted).unwrap();
        let report = blocks_verify(&[a, b], &["flow-context".to_string()]).unwrap();
        assert!(!report.ok);
        // drift entries carry per-file hash detail
        let blocks = report.report.get("blocks").and_then(|v| v.as_array()).unwrap();
        assert_eq!(blocks.len(), 1);
        let drift_arr = blocks[0].get("drift").and_then(|v| v.as_array()).unwrap();
        assert_eq!(drift_arr.len(), 2);
        let h0 = drift_arr[0].get("hash").and_then(|v| v.as_str()).unwrap();
        let h1 = drift_arr[1].get("hash").and_then(|v| v.as_str()).unwrap();
        assert_ne!(h0, h1, "drift implies distinct hashes");
    }

    #[test]
    fn blocks_verify_missing_marker_reports_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        let good = "\
<!-- SHARED-BLOCK:x START -->
body
<!-- SHARED-BLOCK:x END -->
";
        fs::write(&a, good).unwrap();
        fs::write(&b, "nothing here\n").unwrap();
        let report = blocks_verify(&[a, b], &["x".to_string()]).unwrap();
        assert!(!report.ok);
        let blocks = report.report.get("blocks").and_then(|v| v.as_array()).unwrap();
        let missing = blocks[0].get("missing").and_then(|v| v.as_array()).unwrap();
        assert_eq!(missing.len(), 1);
    }

    #[test]
    fn scan_block_names_warn_emits_for_typo_but_keeps_canonical() {
        // R53: a line that looks like a SHARED-BLOCK marker but is malformed
        // (missing hyphen, wrong case, trailing whitespace) must NOT be
        // picked up as a block name, AND must NOT break verification — it's
        // only advisory. We can't easily capture stderr from within a unit
        // test without invasive plumbing; assert on the behavioural
        // guarantees instead: canonical names are still discovered and the
        // typo isn't silently treated as a block.
        let contents = "\
<!-- SHAREDBLOCK:typo START -->
should-be-ignored
<!-- SHAREDBLOCK:typo END -->
<!-- SHARED-BLOCK:real START -->
body
<!-- SHARED-BLOCK:real END -->
";
        let names = scan_block_names_warn(contents, Some("synthetic-fixture"));
        assert_eq!(names, vec!["real".to_string()]);
        // A full verify over two files, each with a typo line, still passes
        // for the canonical block.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        fs::write(&a, contents).unwrap();
        fs::write(&b, contents).unwrap();
        let report = blocks_verify(&[a, b], &["real".to_string()]).unwrap();
        assert!(
            report.ok,
            "typo lines must not break verification: {:?}",
            report.report
        );
    }

    #[test]
    fn blocks_verify_reproduces_shell_hashes() {
        // R87: pin hashes for every block enumerated in
        // `scripts/shared-blocks.toml`. The blocks have divergent file
        // coverage — `flow-context` spans 8 command files (all flow-aware
        // commands); `ledger-schema` spans 4 (the review/optimise pair); the
        // three apply-only blocks span 2 (optimise-apply + review-apply).
        // Splitting the assertions this way lets a drift in any one block
        // surface independently with a named hash, instead of a confusing
        // "missing" report from running a narrower block over a wider file
        // list.
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
        let cmd_dir = repo_root.join("claude").join("commands");
        let flow_context_eight = [
            cmd_dir.join("optimise.md"),
            cmd_dir.join("review.md"),
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
            cmd_dir.join("plan-new.md"),
            cmd_dir.join("plan-update.md"),
            cmd_dir.join("implement.md"),
            cmd_dir.join("review-plan.md"),
        ];
        let ledger_schema_four = [
            cmd_dir.join("optimise.md"),
            cmd_dir.join("review.md"),
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
        ];
        let execution_record_three = [
            cmd_dir.join("plan-new.md"),
            cmd_dir.join("plan-update.md"),
            cmd_dir.join("implement.md"),
        ];
        let apply_pair = [
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
        ];

        // Only run when every file is present. The test crate is consumable
        // in isolation; degrade gracefully if someone packages it without
        // the command tree.
        if !flow_context_eight.iter().all(|p| p.exists())
            || !ledger_schema_four.iter().all(|p| p.exists())
            || !execution_record_three.iter().all(|p| p.exists())
        {
            eprintln!(
                "blocks_verify_reproduces_shell_hashes: command files not found, skipping"
            );
            return;
        }

        // R53: on hash-drift the bare `assertion_failed` message is hard to
        // act on — the caller sees "block X hash drift" and has to reverse-
        // engineer both the actual hash and which file(s) moved. Emit a
        // structured multi-line message instead:
        //   - expected (the pinned constant that's now stale)
        //   - actual   (the in-parity hash currently produced by the blocks
        //              under test; absent when parity itself is broken)
        //   - per-file hashes (for parity: the single hash each file maps
        //              to; for drift: every (file, hash) pair so the
        //              operator can spot the outlier file without re-running)
        //   - remediation (the literal pinned-hash constant update to make)
        let expect_hash = |report: &blocks::BlocksReport, name: &str, expected: &str| {
            let blocks_arr = report
                .report
                .get("blocks")
                .and_then(|v| v.as_array())
                .expect("blocks array");
            let block = blocks_arr
                .iter()
                .find(|b| b.get("name").and_then(|v| v.as_str()) == Some(name))
                .unwrap_or_else(|| {
                    panic!(
                        "block `{name}` missing from report: {:?}",
                        report.report
                    )
                });

            // The "happy" shape (`blocks_verify` reports parity): a single
            // `hash` field + a `files` array. Compare the pinned constant
            // against it; on mismatch, print every contributing file so the
            // operator can copy the new hash into the source.
            if let Some(hash) = block.get("hash").and_then(|v| v.as_str()) {
                if hash != expected {
                    let files: Vec<String> = block
                        .get("files")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut msg = String::new();
                    msg.push_str(&format!("block `{name}` pinned-hash drift\n"));
                    msg.push_str(&format!("  expected: {expected}\n"));
                    msg.push_str(&format!("  actual:   {hash}\n"));
                    msg.push_str("  per-file (all match each other):\n");
                    for f in &files {
                        msg.push_str(&format!("    {f}: {hash}\n"));
                    }
                    msg.push_str(&format!(
                        "  fix: update the pinned hash for `{name}` to {hash}"
                    ));
                    panic!("{msg}");
                }
                return;
            }

            // The "sad" shape: `blocks_verify` already detected drift across
            // files — there is no single `hash`, only a `drift` array of
            // per-file hashes. Emit all of them so the operator can see
            // both WHICH file moved and whether the pinned constant is
            // stale as well.
            let drift_arr = block
                .get("drift")
                .and_then(|v| v.as_array())
                .unwrap_or_else(|| {
                    panic!(
                        "block `{name}` has neither `hash` nor `drift`: {:?}",
                        block
                    )
                });
            let mut msg = String::new();
            msg.push_str(&format!(
                "block `{name}` parity broken across files (pre-pinned-hash check)\n"
            ));
            msg.push_str(&format!("  expected (pinned): {expected}\n"));
            msg.push_str("  per-file hashes (should be identical, but differ):\n");
            for entry in drift_arr {
                let f = entry
                    .get("file")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>");
                let h = entry
                    .get("hash")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<no-hash>");
                msg.push_str(&format!("    {f}: {h}\n"));
            }
            msg.push_str(
                "  fix: restore block parity across the listed files first, \
                 then re-run this test to see whether the pinned constant \
                 also needs updating",
            );
            panic!("{msg}");
        };

        // --- 8-file flow-context block ---
        let report = blocks_verify(
            &flow_context_eight,
            &["flow-context".to_string()],
        )
        .unwrap();
        assert!(report.ok, "flow-context block must be parity: {:?}", report.report);
        expect_hash(
            &report,
            "flow-context",
            "6ca86274d31b5a32feaf8ecc3360ce4f773b84c37b46607edae8bef28951f60a",
        );

        // --- 4-file ledger-schema block ---
        let report = blocks_verify(
            &ledger_schema_four,
            &["ledger-schema".to_string()],
        )
        .unwrap();
        assert!(report.ok, "ledger-schema block must be parity: {:?}", report.report);
        expect_hash(
            &report,
            "ledger-schema",
            "e4c4cbe0a0d014535e0a4e42a7be3f43934b004120b01e967037d28c48a345e8",
        );

        // --- 3-file execution-record-schema block ---
        let report = blocks_verify(
            &execution_record_three,
            &["execution-record-schema".to_string()],
        )
        .unwrap();
        assert!(report.ok, "execution-record-schema block must be parity: {:?}", report.report);
        expect_hash(
            &report,
            "execution-record-schema",
            "6cbc2a72dbd05ba7fc68cc19ea2947b9c3df93aa9407f0a371c27f3ee1f0a735",
        );

        // --- 2-file apply-only blocks ---
        let report = blocks_verify(
            &apply_pair,
            &[
                "apply-dependency-sort".to_string(),
                "apply-rollback-protocol".to_string(),
                "apply-constraints".to_string(),
            ],
        )
        .unwrap();
        assert!(
            report.ok,
            "apply-only blocks must be parity across the 2-file subset: {:?}",
            report.report
        );
        expect_hash(
            &report,
            "apply-dependency-sort",
            "482172a20b6f88eef4abf4af93464e8b80825d7719672e390d1d785230be846a",
        );
        expect_hash(
            &report,
            "apply-rollback-protocol",
            "60e5c94601da99267431825e6eae19f68477ad9d7405f5c7c24945b40cfabc8f",
        );
        expect_hash(
            &report,
            "apply-constraints",
            "e136930179c1ac9145e769eaa4389826cfa572432a5a57aba2fba6b591066509",
        );
    }

    // ----- R54: stdin sentinel ------

    #[test]
    fn read_json_arg_returns_literal_when_not_dash() {
        let got = read_json_arg(r#"{"key":"value"}"#).unwrap();
        assert_eq!(got, r#"{"key":"value"}"#);
    }

    #[test]
    fn read_json_arg_literal_roundtrip() {
        // R54 part 1 (stdin sentinel): the pure literal path is tested here;
        // the `-` sentinel path is covered by a subprocess integration test
        // in a future assert_cmd harness — exercising it in unit tests would
        // require rewiring `std::io::stdin()`, which is invasive enough that
        // we defer it rather than carry a test-only file descriptor seam.
        let got = read_json_arg(r#"{"a":1}"#).unwrap();
        assert_eq!(got, r#"{"a":1}"#);
    }

    // R32: a second `-` sentinel on the same invocation must bail rather than
    // silently re-reading stdin (already at EOF) and returning empty. Hold the
    // env lock so we serialise against any other test that might touch the
    // shared STDIN_CONSUMED flag, then restore it for downstream tests.
    #[test]
    fn read_json_arg_dash_second_call_errors_already_consumed() {
        let _guard = env_lock();
        let prev = STDIN_CONSUMED.swap(false, std::sync::atomic::Ordering::SeqCst);
        // First `-` call: either succeeds (stdin readable) or errors (TTY / empty).
        // In both cases it should have set the consumed flag BEFORE returning.
        let _ = read_json_arg("-");
        let second = read_json_arg("-").unwrap_err();
        let msg = format!("{second:#}");
        assert!(
            msg.contains("already consumed"),
            "expected already-consumed error, got: {msg}"
        );
        // Restore for any other test that might run afterwards in this process.
        STDIN_CONSUMED.store(prev, std::sync::atomic::Ordering::SeqCst);
    }
}
