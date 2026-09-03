//! dispatch — `fn run()`, the `items`/`blocks` sub-dispatchers, plus
//! the NDJSON source resolver and the integrity-opts translators
//! that glue clap types to `IntegrityOpts`. The clap surface lives in
//! `super::types` and the output helpers in `crate::output`.
//!
//! Pure plumbing; no business logic — every `Cmd` / `ItemsOp` / `BlocksOp`
//! arm delegates to `items::` / `blocks::` / `io::` helpers that own the
//! underlying behaviour.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value as JsonValue;

use super::types::{
    BlocksOp, Cli, Cmd, FEATURES, IntegrityOp, ItemsOp, LegacyShortcuts, ReadIntegrityArgs,
    SUBCOMMANDS, WriteIntegrityArgs,
};

use crate::blocks::blocks_verify;
use crate::convert::{
    detable_to_json, maybe_date_coerce, navigate, parse_scalar, set_at_path, str_field,
    toml_to_json,
};
use crate::dedup::{
    FINGERPRINTED_FIELDS, items_find_duplicates, items_find_duplicates_across,
    items_find_duplicates_across_json, items_find_duplicates_json, tier_b_fingerprint_table,
};
use crate::integrity::{IntegrityOpts, refresh_sidecar, sidecar_path, verify_integrity};
use crate::io::{
    compute_set_json_mutation, compute_set_mutation, dry_run_read_opts, guard_write_path, item_id,
    items_array, mutate_doc, mutate_doc_conditional, mutate_doc_plan, on_missing_for, read_doc,
    read_doc_borrowed, read_doc_either, read_json_arg, read_json_value_from_arg, read_toml_str,
    recheck_claude_containment, strict_read_check, warn_if_created, warn_if_read_outside_claude,
    with_exclusive_lock,
};
use crate::items::{
    AddManyOutcome, AddOutcome, array_append, compute_add_many_mutation, compute_add_mutation,
    compute_apply_mutation, compute_array_append_mutation, compute_backfill_mutation,
    compute_remove_mutation, compute_update_mutation, dedup_id_disabled, items_add_many,
    items_add_many_with_dedupe, items_add_to, items_add_value_with_dedupe_to, items_get_from,
    items_get_from_json, items_infer_and_next_id, items_next_id, items_update_to, parse_ndjson,
};
use crate::orphans::items_orphans;
use crate::output::{
    emit_dry_run_plan, emit_dry_run_scalar, emit_list_raw, print_json, print_json_compact,
    print_raw_value,
};
use crate::query::{self, Query, ShapeDispatch};

/// Maximum number of ops accepted in a single `items apply` batch.
/// The 32 MiB stdin cap alone does not bound op count — a well-formed 32 MiB
/// JSON array of tiny `{"op":"update","id":"Rx"}` records can hold tens of
/// thousands of operations, and `items_apply_to_opts` iterates serially.
/// 10_000 is far above any legitimate batch (typical ledgers have ~50 items
/// and typical apply batches ≤ 60 ops) while still bounded enough that an
/// accidental loop-generated mega-payload fails fast instead of timing out
/// the wrapping shell.
const MAX_OPS_PER_APPLY: usize = 10_000;

/// Resolve an NDJSON source argument. A literal dash reads stdin via
/// `io::read_json_arg` (preserving its guard against a second
/// `-` sentinel on the same invocation); any other value is a file path
/// read verbatim with `fs::read_to_string`. Shared by `Cmd::ArrayAppend`
/// and `ItemsOp::AddMany`.
fn read_ndjson_source(src: &str) -> Result<String> {
    if src == "-" {
        read_json_arg("-")
    } else {
        std::fs::read_to_string(src).with_context(|| format!("reading NDJSON file `{}`", src))
    }
}

/// Parse the `--dedupe-by` flag value into a `Vec<String>` of field
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
        bail!(
            "--dedupe-by requires at least one field name (e.g. `--dedupe-by source,target` for a comma-separated list)"
        );
    }
    Ok(fields)
}

/// Translate the flattened integrity-args structs from a subcommand variant
/// into the module-local `IntegrityOpts` bundle. Kept next to the CLI
/// definition (rather than in `integrity.rs`) so the integrity module stays
/// free of the clap-derived types. Read paths hand us `ReadIntegrityArgs`
/// (only `verify_integrity` matters), write paths hand us
/// `WriteIntegrityArgs` (the full set). Both flow through the same
/// `IntegrityOpts` so every downstream consumer
/// (`maybe_verify_integrity` / `write_toml_with_sidecar`) sees one type.
pub(crate) fn read_integrity_opts(args: &ReadIntegrityArgs) -> IntegrityOpts {
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

pub(crate) fn write_integrity_opts(args: &WriteIntegrityArgs) -> IntegrityOpts {
    IntegrityOpts {
        write_sidecar: !args.no_write_integrity,
        verify_on_read: args.verify_integrity,
        strict: args.strict_integrity,
    }
}

/// Emit the canonical success envelope for the SIMPLE write arms
/// (`set` / `set-json` / `update` / `remove` / `apply`) and pair it with the
/// `warn_if_created` stderr note. Key order is load-bearing —
/// `serde_json`'s `preserve_order` keeps insertion order, so the emitted
/// order is `ok`→`created`→`path`. The ENRICHED arms
/// (`array-append` / `items add[-many]` / `backfill`) keep their inline
/// envelopes because they interleave arm-specific keys (`appended` / `added` /
/// `skipped_rows` / `backfilled`).
fn write_envelope(file: &std::path::Path, created: bool) -> Result<()> {
    warn_if_created(file, created);
    print_json_compact(&serde_json::json!({
        "ok": true,
        "created": created,
        "path": file.display().to_string(),
    }))
}

/// TOML write subcommands (`set`, `set-json`, `array-append`) refuse `.json`
/// targets and point the caller at `tomlctl json set`. The symmetric half
/// (JSON writers refuse `.toml` targets) lives in
/// `crate::json::refuse_toml_extension`.
/// Pairing the two prevents the silent-write hazard of e.g.
/// `tomlctl set .claude/settings.json key val` parsing the JSON file as
/// TOML and emitting unrelated bytes back into it.
fn refuse_json_extension_for_toml_writers(file: &std::path::Path) -> Result<()> {
    if file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        return Err(crate::errors::tagged_err(
            crate::errors::ErrorKind::Validation,
            Some(file.to_path_buf()),
            format!(
                "use `tomlctl json set {} ...` — TOML writers do not handle .json files",
                file.display()
            ),
        ));
    }
    Ok(())
}

/// Top-level dispatch entrypoint. `main.rs` is a one-line wrapper over
/// this; splitting lets the binary target stay trivially small while all
/// the parsing/dispatch/output plumbing lives in a normal module.
///
/// The `Cli` is parsed once in `main.rs` and threaded in here. A second
/// parse here (a `try_parse()` peek for `--error-format`, then a full
/// `Cli::parse()` on entry) would silently swallow errors on the peek path
/// and risk double `--help` rendering.
pub(crate) fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Parse { file, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            // `parse` is the single dispatch arm whose whole output is
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
        Cmd::Get {
            file,
            path,
            raw,
            integrity,
        } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            let out = read_doc(&file, opts, |doc| {
                Ok(match path.as_deref() {
                    None | Some("") => toml_to_json(doc),
                    Some(p) => toml_to_json(
                        navigate(doc, p).ok_or_else(|| {
                            anyhow!(
                                "key path `{}` not found (run `tomlctl parse <file>` to inspect the document tree, or `tomlctl get <file>` with no --path to print the whole doc)",
                                p
                            )
                        })?,
                    ),
                })
            })?;
            if raw {
                // Bare-scalar emit. `emit_raw` validates the value is a
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
            dry_run,
            integrity,
        } => {
            refuse_json_extension_for_toml_writers(&file)?;
            if dry_run {
                // A caller passing `tomlctl set --dry-run /etc/passwd`
                // would otherwise silently parse the file as TOML and
                // surface its parsed contents in the dry-run plan.
                // Advisory warn (matches the cross-ledger FindDuplicates
                // path); the actual containment refusal lives on the write
                // side via `guard_write_path`.
                warn_if_read_outside_claude(&file);
                // Dry-run path — read-only compute via the same
                // `compute_set_mutation` the live writer would invoke,
                // emitted via the scalar dry-run envelope. Mirrors the
                // `ItemsOp::Apply` reference: never acquire the exclusive
                // lock, never refresh the sidecar.
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_set_mutation(doc, &path, &value, ty)
                })?;
                emit_dry_run_scalar(&plan)?;
                return Ok(());
            }
            let opts = write_integrity_opts(&integrity);
            // Auto-create a missing target (default), or fail with the strict
            // not_found error (`--no-create`).
            let on_missing = on_missing_for(&file, integrity.no_create)?;
            // Surface the `created` signal — `"created"` + `"path"` in the
            // success envelope, plus the one-line stderr guidance when seeded.
            let created = mutate_doc(&file, integrity.allow_outside, opts, on_missing, |doc| {
                let v = parse_scalar(&value, ty)?;
                set_at_path(doc, &path, v)
            })?;
            write_envelope(&file, created)?;
        }
        Cmd::SetJson {
            file,
            path,
            json,
            dry_run,
            integrity,
        } => {
            refuse_json_extension_for_toml_writers(&file)?;
            // Parse stdin/literal JSON straight into a `JsonValue`, skipping
            // the intermediate String allocation. Keeping the parse outside
            // the `mutate_doc` closure means a malformed payload fails
            // before the doc is opened; keeping it above the `if dry_run`
            // branch means a malformed `--json` fails identically in
            // dry-run and live mode.
            let parsed: JsonValue = read_json_value_from_arg(&json).context("parsing --json")?;
            if dry_run {
                // See `Cmd::Set` dry-run for the rationale on this
                // advisory warn. Same threat shape: a caller passing
                // `--dry-run /etc/passwd` to set-json would otherwise
                // silently parse the file as TOML and echo it back in the
                // plan envelope.
                warn_if_read_outside_claude(&file);
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_set_json_mutation(doc, &path, &parsed)
                })?;
                emit_dry_run_scalar(&plan)?;
                return Ok(());
            }
            let opts = write_integrity_opts(&integrity);
            // Auto-create policy; see `Cmd::Set`.
            let on_missing = on_missing_for(&file, integrity.no_create)?;
            // Surface `created` + `path` (see `Cmd::Set`).
            let created = mutate_doc(&file, integrity.allow_outside, opts, on_missing, |doc| {
                let last_key = path
                    .rsplit_once('.')
                    .map(|(_, k)| k)
                    .unwrap_or(path.as_str());
                let v = maybe_date_coerce(last_key, &parsed)?;
                set_at_path(doc, &path, v)
            })?;
            write_envelope(&file, created)?;
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
            dry_run,
            integrity,
        } => {
            refuse_json_extension_for_toml_writers(&file)?;
            // clap's `conflicts_with` guarantees at most one is set; enforce
            // "at least one" here since clap has no first-class
            // required-exactly-one primitive on optional flags.
            if json.is_none() && ndjson.is_none() {
                bail!(
                    "array-append requires one of --json or --ndjson (e.g. `--json '{{\"k\":\"v\"}}'` for a single row, `--ndjson rows.ndjson` for a batch)"
                );
            }
            // The rows parse sits above the dry-run/live split so both paths
            // share parse semantics. `--json` / `--ndjson` resolution
            // (including stdin) happens once.
            let rows: Vec<JsonValue> = if let Some(j) = json {
                // Parse straight to `JsonValue`, avoiding a `read_json_arg`
                // String + `serde_json::from_str` two-step.
                let parsed: JsonValue = read_json_value_from_arg(&j).context("parsing --json")?;
                if !parsed.is_object() {
                    bail!(
                        "--json must be a JSON object (e.g. {{\"k\":\"v\"}}); got JSON {}",
                        crate::convert::json_type_name(&parsed)
                    );
                }
                vec![parsed]
            } else {
                let nd = ndjson.expect("checked above");
                let text = read_ndjson_source(&nd)?;
                parse_ndjson(&text)?
            };
            if dry_run {
                // Advisory warn for dry-run reads outside `.claude/`.
                // Same threat shape as the other dry-run arms — a caller
                // pointing `array-append --dry-run` at an arbitrary file
                // would otherwise leak the parsed TOML through the plan
                // envelope.
                warn_if_read_outside_claude(&file);
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_array_append_mutation(doc, &array, &rows)
                })?;
                emit_dry_run_plan(&plan)?;
                return Ok(());
            }
            let opts = write_integrity_opts(&integrity);
            let mut appended: usize = 0;
            // Auto-create policy.
            let on_missing = on_missing_for(&file, integrity.no_create)?;
            // Surface `created` + `path` alongside the `appended` count.
            let created = mutate_doc(&file, integrity.allow_outside, opts, on_missing, |doc| {
                appended = array_append(doc, &array, &rows)?;
                Ok(())
            })?;
            warn_if_created(&file, created);
            print_json_compact(&serde_json::json!({
                "ok": true,
                "appended": appended,
                "created": created,
                "path": file.display().to_string(),
            }))?;
        }
        Cmd::Integrity { op } => integrity_dispatch(op)?,
        Cmd::Flow { op } => crate::flow::dispatch(op)?,
        Cmd::Backlog { op } => crate::backlog::dispatch::dispatch(op)?,
        Cmd::Json { op } => {
            // Resolve `--json -` stdin sentinel for `json set` at the CLI
            // boundary, mirroring TOML `set-json` / `items add` behaviour.
            // Without this, `json::handle_set` receives the literal string
            // `"-"` and fails with `parsing --json value `-`: EOF while
            // parsing a value`. The plan-new / plan-update / review-plan
            // carriers all instruct callers to write plansDirectory via the
            // stdin-heredoc form (`cat <<'EOF' | tomlctl json set … --json -`).
            // `read_json_arg` honours the STDIN_CONSUMED + TTY + size guards
            // already in force across the TOML write paths.
            let op = match op {
                crate::cli::types::JsonOp::Set {
                    file,
                    path,
                    json,
                    dry_run,
                    integrity,
                } => {
                    let json = read_json_arg(&json).context("parsing --json")?;
                    crate::cli::types::JsonOp::Set {
                        file,
                        path,
                        json,
                        dry_run,
                        integrity,
                    }
                }
                other => other,
            };
            crate::json::dispatch(op)?
        }
        Cmd::Capabilities => {
            // Pretty-print matches the rest of the read-path surface
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
                "commands": crate::capabilities::build_agent_context(),
            });
            print_json(&output)?;
        }
    }
    Ok(())
}

/// Tier-B digest of one stored row, with the field values it hashed.
///
/// The `fields` keys are read off `FINGERPRINTED_FIELDS` rather than
/// transcribed, so the emitted object cannot name a set the hash does not
/// use. The not-found wording is the one `items_get_from` bails with —
/// duplicated verbatim, as `items_get_from_json` duplicates it.
fn items_fingerprint(doc: &toml::Value, id: &str) -> Result<JsonValue> {
    for item in items_array(doc, "items") {
        if item_id(item) != Some(id) {
            continue;
        }
        let Some(tbl) = item.as_table() else { continue };
        let fields: serde_json::Map<String, JsonValue> = FINGERPRINTED_FIELDS
            .iter()
            .map(|f| ((*f).to_string(), JsonValue::from(str_field(tbl, f))))
            .collect();
        return Ok(serde_json::json!({
            "id": id,
            "tier": "B",
            "dedup_id": tier_b_fingerprint_table(tbl),
            "fields": fields,
        }));
    }
    bail!(
        "no item with id = {} (run `tomlctl items list <file> --pluck id` to enumerate available ids)",
        id
    )
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
            let q = Query::from_query_input(&query.to_query_input(&legacy))?;
            // `ndjson` is an output-encoding choice, not a shape. Only
            // the Array and Pluck shape + ndjson encoding combinations are
            // meaningful; for aggregation shapes (Count/CountBy/
            // CountDistinct/GroupBy) the ndjson bit is silently ignored
            // since the output is a single JSON value that has no per-line
            // decomposition.
            //
            // Pluck is streaming-eligible too: `--pluck f
            // --lines` (or `--pluck f --ndjson`) streams one plucked JSON
            // value per line; `run_streaming` mirrors `apply_pluck`'s
            // null/missing-drop so the set of emitted values is identical
            // to the non-streaming path.
            if q.ndjson && q.shape.is_streamable() {
                // Stream one compact JSON value per line directly via
                // `query::run_streaming`, avoiding the `Vec<JsonValue>` that
                // `query::run` would otherwise materialise only for us to
                // iterate and re-serialise. The streaming path walks the
                // same pipeline and emits per-item — peak memory scales with
                // the filtered set, not the full output array.
                //
                // `--pluck foo --lines --raw` flows through here too;
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
                read_doc(&file, opts, |doc| {
                    query::run_streaming(doc, &array, &q, &mut h)
                })?;
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
        ItemsOp::Get {
            file,
            id,
            array,
            integrity,
        } => {
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
        ItemsOp::Add {
            file,
            json,
            array,
            dedupe_by,
            dry_run,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            let dedupe_fields = parse_dedupe_fields(dedupe_by.as_deref())?;
            if dry_run {
                // A caller passing `tomlctl items add --dry-run
                // /etc/passwd` would otherwise silently parse the file as
                // TOML and surface its parsed contents in the dry-run plan.
                // Advisory warn (matches the cross-ledger FindDuplicates
                // path); the actual containment refusal lives on the write
                // side via `guard_write_path`.
                warn_if_read_outside_claude(&file);
                // Dry-run path mirrors the live arm's two-branch
                // structure — `compute_add_mutation` for the no-dedupe
                // case, `compute_add_many_mutation` with a single-row vec
                // for the dedupe case. Both go through the same
                // `items_add_value_to` / `items_add_many_with_dedupe`
                // funnels the live path uses, so validation surfaces and
                // dedup_id auto-population are byte-identical.
                //
                // Parse `--json` once at the top so both branches share the
                // parse semantics (and the stdin cost). A per-branch parse
                // (`read_json_arg` String for no-dedupe vs
                // `read_json_value_from_arg` JsonValue for dedupe) makes the
                // stdin behaviour and the `parsing --json` error site
                // asymmetric.
                let patch: JsonValue = read_json_value_from_arg(&json).context("parsing --json")?;
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = if dedupe_fields.is_empty() {
                    read_doc(&file, read_opts, |doc| {
                        compute_add_mutation(doc, &array, &patch)
                    })?
                } else {
                    let rows = vec![patch];
                    read_doc(&file, read_opts, |doc| {
                        compute_add_many_mutation(doc, &array, &rows, None, &dedupe_fields)
                    })?
                };
                emit_dry_run_plan(&plan)?;
                return Ok(());
            }
            if dedupe_fields.is_empty() {
                // No-dedupe path: the envelope stays plain `{"ok":true}` and
                // the always-write `mutate_doc` pipeline runs unconditionally.
                // Absent `--dedupe-by` the output must not gain an `added`
                // count; the enriched shape belongs to the `--dedupe-by`
                // branch below.
                let json = read_json_arg(&json)?;
                // Auto-create policy.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                // Surface `created` + `path`. Purely additive on top of the
                // plain `{"ok":true}` shape, so consumers reading no extra
                // keys are unaffected.
                let created =
                    mutate_doc(&file, integrity.allow_outside, opts, on_missing, |doc| {
                        items_add_to(doc, &array, &json)
                    })?;
                write_envelope(&file, created)?;
            } else {
                // Dedupe path: parse JSON once up-front so we can feed it
                // to the pre-scan inside the lock without a re-parse.
                // `mutate_doc_conditional` elides the write-and-sidecar
                // bump when the scan returns a match; the caller sees
                // `added:0,matched_id:...` and the on-disk file + sidecar
                // are untouched.
                let patch: JsonValue = read_json_value_from_arg(&json).context("parsing --json")?;
                let mut outcome: Option<AddOutcome> = None;
                // Auto-create policy. On a dedupe hit against a
                // freshly-seeded missing file the closure returns `Ok(false)`,
                // so `mutate_doc_conditional` skips the write and leaves no
                // stray file — and reports `created=false` (nothing persisted),
                // which is exactly what we surface below.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                // `created` from `mutate_doc_conditional` is true only when
                // the seed fired AND a write actually landed. Surface it
                // (plus `path`) on BOTH the `Added` and `Skipped` arms,
                // alongside each arm's own keys.
                let created = mutate_doc_conditional(
                    &file,
                    integrity.allow_outside,
                    opts,
                    on_missing,
                    |doc| {
                        let result =
                            items_add_value_with_dedupe_to(doc, patch, &array, &dedupe_fields)?;
                        let mutated = matches!(result, AddOutcome::Added);
                        outcome = Some(result);
                        Ok(mutated)
                    },
                )?;
                warn_if_created(&file, created);
                match outcome.expect("closure always sets outcome on success") {
                    AddOutcome::Added => {
                        print_json_compact(&serde_json::json!({
                            "ok": true,
                            "added": 1,
                            "created": created,
                            "path": file.display().to_string(),
                        }))?;
                    }
                    AddOutcome::Skipped { matched_id } => {
                        print_json_compact(&serde_json::json!({
                            "ok": true,
                            "added": 0,
                            "matched_id": matched_id,
                            "created": created,
                            "path": file.display().to_string(),
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
            dry_run,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            let dedupe_fields = parse_dedupe_fields(dedupe_by.as_deref())?;
            // The STDIN_CONSUMED guard inside `read_json_arg` refuses a second
            // `-` when `--defaults-json -` also wants stdin on the same call.
            let ndjson_text = read_ndjson_source(&ndjson)?;
            let rows = parse_ndjson(&ndjson_text)?;
            let defaults: Option<JsonValue> = match defaults_json.as_deref() {
                // Parse straight to `JsonValue`, avoiding a `read_json_arg`
                // String + `serde_json::from_str` two-step.
                Some(s) => Some(read_json_value_from_arg(s).context("parsing --defaults-json")?),
                None => None,
            };
            if dry_run {
                // Advisory warn for dry-run reads outside `.claude/`.
                // Same threat shape as the other dry-run arms — a caller
                // pointing `items add-many --dry-run` at an arbitrary file
                // would otherwise leak the parsed TOML through the plan
                // envelope.
                warn_if_read_outside_claude(&file);
                // Dry-run flows through the same `compute_add_many_mutation`
                // helper the live dedupe path's compute-side mirrors, with
                // `dedupe_fields` honoured (empty slice → `items_add_many`
                // funnel inside the helper; non-empty → the dedupe funnel).
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_add_many_mutation(doc, &array, &rows, defaults.as_ref(), &dedupe_fields)
                })?;
                emit_dry_run_plan(&plan)?;
                return Ok(());
            }
            if dedupe_fields.is_empty() {
                // No-dedupe path: output shape is `{"ok":true,"added":N}` and
                // the always-write pipeline runs unconditionally.
                let mut added: usize = 0;
                // Auto-create policy.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                // Surface `created` + `path` alongside the `added` count.
                let created =
                    mutate_doc(&file, integrity.allow_outside, opts, on_missing, |doc| {
                        added = items_add_many(doc, &array, &rows, defaults.as_ref())?;
                        Ok(())
                    })?;
                warn_if_created(&file, created);
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "added": added,
                    "created": created,
                    "path": file.display().to_string(),
                }))?;
            } else {
                // Dedupe path: run the pre-scan + append loop inside the
                // lock via `mutate_doc_conditional`. Skip the file write
                // entirely when the batch added zero rows — the doc is
                // untouched and the sidecar must not bump for a pure-
                // skip batch. Any `added > 0` takes the write branch.
                let mut outcome: Option<AddManyOutcome> = None;
                // Auto-create policy. A pure-skip batch (added == 0)
                // against a freshly-seeded missing file returns `Ok(false)`, so
                // the write is skipped, no stray file lands, and `created` comes
                // back `false` — exactly what we surface below.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                // `created` from `mutate_doc_conditional` is true only when
                // the seed fired AND a write landed. Surface it (plus `path`)
                // alongside the batch-count keys.
                let created = mutate_doc_conditional(
                    &file,
                    integrity.allow_outside,
                    opts,
                    on_missing,
                    |doc| {
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
                    },
                )?;
                warn_if_created(&file, created);
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
                    "created": created,
                    "path": file.display().to_string(),
                }))?;
            }
        }
        ItemsOp::Update {
            file,
            id,
            json,
            unset,
            array,
            dry_run,
            integrity,
        } => {
            // Enforce "at least one of --json / --unset" here rather than as a
            // required ArgGroup, matching `array-append`'s --json / --ndjson
            // pair: this `bail!` surfaces as an `--error-format json` envelope,
            // a clap refusal as exit-2 usage prose. An update naming neither
            // would rewrite the ledger and its sidecar for no field change.
            if json.is_none() && unset.is_empty() {
                bail!(
                    "items update requires one of --json or --unset (e.g. `--json '{{\"status\":\"fixed\"}}'` to merge fields, `--unset notes` to remove one)"
                );
            }
            let opts = write_integrity_opts(&integrity);
            // The json arg parse sits above the dry-run/live split.
            // `compute_update_mutation` takes the raw &str and parses
            // internally (same surface as `items_update_to`), so both
            // branches share the resolved string.
            //
            // An absent `--json` defaults to an empty patch, so the merge loop
            // has no keys and the `--unset` removals are the whole field
            // mutation — but not the whole write: an `--unset` naming a
            // fingerprinted field the row carries still drives
            // `apply_dedup_id_on_update`'s recompute over the post-unset row.
            // The guard above is what keeps that default from degenerating
            // into a no-op write.
            let json = match json {
                Some(arg) => read_json_arg(&arg)?,
                None => "{}".to_string(),
            };
            if dry_run {
                // Advisory warn for dry-run reads outside `.claude/`.
                // Same threat shape as the other dry-run arms — a caller
                // pointing `items update --dry-run` at an arbitrary file
                // would otherwise leak the parsed TOML through the plan
                // envelope.
                warn_if_read_outside_claude(&file);
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_update_mutation(doc, &array, &id, &json, &unset)
                })?;
                emit_dry_run_plan(&plan)?;
                return Ok(());
            }
            // Auto-create policy. An `update` against a freshly-seeded
            // missing file finds no matching id and the closure errors out
            // BEFORE the persist (`mutate_doc`'s `?`), so nothing is written —
            // no stray seeded file. This preserves the transactional
            // "write-only-on-closure-success" property the plan calls out, and
            // means `created` is only ever `true` here when the update landed
            // into a freshly-seeded file (which requires the id to already be
            // present — impossible on a 2-key skeleton — so in practice this
            // surfaces `created=false` or errors out first).
            let on_missing = on_missing_for(&file, integrity.no_create)?;
            // Surface `created` + `path`.
            let created = mutate_doc(&file, integrity.allow_outside, opts, on_missing, |doc| {
                items_update_to(doc, &array, &id, &json, &unset)
            })?;
            write_envelope(&file, created)?;
        }
        ItemsOp::Remove {
            file,
            id,
            array,
            dry_run,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            if dry_run {
                // Advisory warn for dry-run reads outside `.claude/`.
                // Same threat shape as the other dry-run arms — a caller
                // pointing `items remove --dry-run` at an arbitrary file
                // would otherwise leak the parsed TOML through the plan
                // envelope.
                warn_if_read_outside_claude(&file);
                // Dry-run path — compute the plan on a locally-read
                // doc (no exclusive lock) and emit the would_change
                // summary. The compute phase runs the same validation
                // as the live path (`compute_remove_mutation` delegates
                // to `items_remove_from` on a cloned doc), so a missing
                // id bails with the identical "no item with id = X"
                // error a real remove would surface.
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
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
                // Auto-create policy. A `remove` against a freshly-seeded
                // missing file finds no matching id and `compute_remove_mutation`
                // errors out BEFORE the persist (`mutate_doc_plan`'s `?`), so
                // nothing is written — so in practice `created` here surfaces
                // `false` or the call errors out first.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                // Surface `created` + `path`.
                let created =
                    mutate_doc_plan(&file, integrity.allow_outside, opts, on_missing, |doc| {
                        compute_remove_mutation(doc, &array, &id)
                    })?;
                write_envelope(&file, created)?;
            }
        }
        ItemsOp::Apply {
            file,
            ops,
            array,
            no_remove,
            dry_run,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            // Parse `--ops` ONCE at the CLI boundary and thread the parsed
            // `JsonValue` through both the `MAX_OPS_PER_APPLY` length check
            // and `compute_apply_mutation`. Parsing separately in each
            // (here for the count cap, again inside
            // `compute_apply_mutation` → `items_apply_to_opts`) doubles the
            // JSON parse cost on every Apply invocation, proportional to the
            // ops payload size. `read_json_value_from_arg` encapsulates the
            // same stdin / TTY / `MAX_STDIN_BYTES` discipline as
            // `read_json_arg`, so the parse semantics stay identical.
            let parsed_ops: JsonValue = read_json_value_from_arg(&ops).context("parsing --ops")?;
            // Bound the ops count at the CLI boundary. `MAX_STDIN_BYTES`
            // only caps the raw payload size; a 32 MiB JSON array of minimal
            // `{"op":"update","id":"Rx"}` records can still hold tens of
            // thousands of ops, which `items_apply_to_opts` iterates serially.
            // Check length here (before locking + the mutator runs) so an
            // over-large payload fails fast with a directed message, and the
            // user-visible error predates any disk mutation.
            // The check also gates `--dry-run`, so an over-large preview
            // refuses with the same message a real run would emit.
            if let JsonValue::Array(arr) = &parsed_ops
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
                // A caller passing `tomlctl items apply --dry-run
                // /etc/passwd` would otherwise silently parse the file as
                // TOML and surface its parsed contents in the dry-run plan.
                // Advisory warn (matches the cross-ledger FindDuplicates
                // path); the actual containment refusal lives on the write
                // side via `guard_write_path`.
                warn_if_read_outside_claude(&file);
                // Same compute phase as the live path, but we stop
                // before the I/O stage. `compute_apply_mutation` runs
                // `items_apply_parsed_to_opts` on a cloned doc, so every
                // validation gate — `--no-remove`, op-shape, missing id,
                // dedup_id auto-populate — fires with a byte-identical
                // error surface.
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_apply_mutation(doc, &array, &parsed_ops, no_remove)
                })?;
                emit_dry_run_plan(&plan)?;
            } else {
                // Auto-create policy. An all-`update`/all-`remove` batch
                // against a freshly-seeded missing file errors in
                // `compute_apply_mutation` (no matching id) BEFORE the persist,
                // so nothing is written. Batches with `add` ops seed-then-append
                // into the new file as expected — `created=true` on that path.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                // Surface `created` + `path`.
                let created =
                    mutate_doc_plan(&file, integrity.allow_outside, opts, on_missing, |doc| {
                        compute_apply_mutation(doc, &array, &parsed_ops, no_remove)
                    })?;
                write_envelope(&file, created)?;
            }
        }
        ItemsOp::NextId {
            file,
            prefix,
            infer_from_file,
            integrity,
        } => {
            // The clap ArgGroup `id_source` guarantees exactly one of
            // `--prefix` / `--infer-from-file` reaches us; no runtime
            // "both unset" or "both set" check is needed.
            //
            // `--strict-read` fires BEFORE the missing-file fast path below,
            // so a caller who opted out of the bootstrap default on this
            // subcommand gets `kind=not_found` instead of the `"<prefix>1"`
            // fallback. `strict_read_check` returns `Ok(())` when the flag
            // is absent OR the file exists, so the default (non-strict)
            // invocation flows straight into that branch.
            strict_read_check(&file, integrity.strict_read)?;
            // If the target ledger doesn't exist yet, there's nothing to
            // parse or verify — the "next" id is trivially `<prefix>1`. This
            // lets flows call `items next-id` before the ledger is initialised
            // (e.g. during bootstrap of a new flow directory). When the caller
            // passed `--infer-from-file` and the file is absent, inference has
            // no corpus to work from, which is indistinguishable from the
            // "empty ledger" failure case — surface the same error so the
            // caller's remediation is the same either way.
            if !file.exists() {
                if infer_from_file {
                    bail!(
                        "--infer-from-file requires a non-empty ledger or explicit --prefix (the file does not exist yet; pass --prefix R/O/A/E directly to bootstrap)"
                    );
                }
                let prefix = prefix.as_deref().expect("clap required_unless_present guarantees prefix is Some when infer_from_file is false");
                // Route the missing-file prefix validation through
                // `items_next_id` on an empty doc so the empty-prefix and
                // all-digit-prefix rejections are tagged `ErrorKind::Validation`
                // consistently with the file-exists branch below. A bare
                // `bail!` here would surface `kind=other` under
                // `--error-format json`, making the kind depend on whether
                // the ledger existed.
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
        ItemsOp::FindDuplicates {
            file,
            tier,
            across,
            integrity,
        } => {
            strict_read_check(&file, integrity.strict_read)?;
            if let Some(other) = across.as_ref() {
                strict_read_check(other, integrity.strict_read)?;
                // Unlike the primary ledger (which flows through the write-side
                // `guard_write_path` before any mutation), the cross-ledger
                // `--across` read has no containment check. A caller passing
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
                    // Load both ledgers under the same integrity
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
                        // Borrowed-DeTable fast-path. Both ledgers go
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
        ItemsOp::Fingerprint {
            file,
            id,
            integrity,
        } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            let out = read_doc(&file, opts, |doc| items_fingerprint(doc, &id))?;
            print_json(&out)?;
        }
        ItemsOp::Orphans { file, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            let orphans = read_doc(&file, opts, items_orphans)?;
            print_json(&JsonValue::Array(orphans))?;
        }
        ItemsOp::BackfillDedupId {
            file,
            array,
            dry_run,
            integrity,
        } => {
            // Kill-switch short-circuit. Checked at the dispatch
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
            let read_opts = dry_run_read_opts(integrity.verify_integrity);
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
                // Mirrors `mutate_doc_conditional`'s "no-mutation →
                // no-write" contract without needing a new wrapper.
                // Carry `created`/`path` for envelope-shape parity with
                // the live branch and every other write site. Backfill never
                // creates — the pre-read above errors `kind=not_found` on a
                // missing ledger first — so `created` is always `false` here.
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "backfilled": 0,
                    "created": false,
                    "path": file.display().to_string(),
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
                // Thread the policy for consistency with the other write
                // sites, though the seed branch is unreachable here — the
                // pre-read above (`read_doc`) already errors `kind=not_found`
                // on a missing ledger before we reach this in-lock recompute,
                // so a backfill never seeds a file.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                // Surface `created` + `path` for parity with the other
                // write sites. `created` is structurally always `false` here
                // (the pre-read short-circuits a missing ledger), so
                // `warn_if_created` never fires — but threading it keeps the
                // envelope shape uniform across every write arm.
                let created =
                    mutate_doc_plan(&file, integrity.allow_outside, opts, on_missing, |doc| {
                        let plan = compute_backfill_mutation(doc, &array)?;
                        written = plan.updated.len();
                        Ok(plan)
                    })?;
                warn_if_created(&file, created);
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "backfilled": written,
                    "created": created,
                    "path": file.display().to_string(),
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
            // `integrity refresh` flattens `WriteIntegrityArgs` for parity
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
            let _ = integrity.strict_integrity; // Silently ignored; see above.
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
                // `--verify-integrity` on refresh means "verify the
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
                // In-lock pre-persist containment re-check, mirroring
                // `mutate_doc` — the inside-lock `guard_write_path`
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
mod tests;
