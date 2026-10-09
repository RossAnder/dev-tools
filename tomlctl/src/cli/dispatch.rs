//! dispatch — `fn run()`, the `blocks`/`inputs`/`integrity` sub-dispatchers,
//! plus the integrity-opts translators that glue clap types to
//! `IntegrityOpts`. The `items` dispatcher lives in the `items` child and the
//! TOML document writers (`set`, `set-json`, `array-append`) in `doc`. The clap
//! surface lives in `super::types` and the output helpers in
//! `crate::output`.
//!
//! Pure plumbing; no business logic — every `Cmd` / `ItemsOp` / `BlocksOp`
//! arm delegates to `items::` / `blocks::` / `io::` helpers that own the
//! underlying behaviour.

use anyhow::{Context, Result, anyhow, bail};

use super::types::{
    AgentsOp, BlocksOp, Cli, Cmd, ErrorFormat, FEATURES, InputsOp, IntegrityOp, ReadIntegrityArgs,
    SUBCOMMANDS, WriteIntegrityArgs,
};

use crate::blocks::blocks_verify;
use crate::convert::{navigate, toml_to_json};
use crate::integrity::{IntegrityOpts, refresh_sidecar, sidecar_path, verify_integrity};
use crate::io::{
    guard_write_path, read_doc, read_doc_borrowed, read_json_arg, read_json_value_from_arg,
    read_ndjson_source, recheck_claude_containment, repo_or_cwd_root, strict_read_check,
    warn_if_created, with_exclusive_lock,
};
use crate::items::parse_ndjson;
use crate::output::{
    OutputOpts, Rows, print_json, print_json_compact, print_json_line, print_raw_value,
    print_report,
};
use crate::query;
use crate::sweep::{self, SweepOptions};

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

/// The global output options from the parsed root. Empty `--select` and
/// `--omit` segments are kept so `OutputOpts::validate` can refuse them.
fn output_opts(cli: &Cli) -> OutputOpts {
    let out = &cli.output;
    let paths = |list: &Option<String>| {
        list.as_deref()
            .map(|s| s.split(',').map(|p| p.trim().to_string()).collect())
    };
    OutputOpts {
        select: paths(&out.select),
        limit: out.limit,
        lines: out.lines,
        get: out.get.clone(),
        template: out.template.clone(),
        quiet: out.quiet,
        json_errors: cli.error_format == ErrorFormat::Json,
        rows: out.rows.clone(),
        header: out.header,
        max_chars: out.max_chars,
        omit: paths(&out.omit),
        filters: out.filters.to_filters(),
    }
}

/// Top-level dispatch entrypoint, called by the library root's `run` (which
/// the binary's `main` wraps); all the dispatch/output plumbing lives in a
/// normal module rather than the crate root.
///
/// The `Cli` is parsed once in the library root's `run` and threaded in
/// here. A second parse here (a `try_parse()` peek for `--error-format`,
/// then a full `Cli::parse()` on entry) would silently swallow errors on
/// the peek path and risk double `--help` rendering.
pub(crate) fn run(cli: Cli) -> Result<()> {
    let opts = output_opts(&cli);
    crate::output::check_where_placement(std::env::args_os().skip(1), &opts.filters)?;
    crate::output::configure(opts)?;
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
                read_doc_borrowed(&file)?
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
                // Bare-scalar emit; `emit_raw` refuses an array or table.
                // Null is impossible here — `navigate` returns `None` for a
                // missing path, which we already surface as "key path not
                // found" above; a present TOML scalar cannot map to JSON null
                // via `toml_to_json`.
                print_raw_value(&out, query::RawArrayHint::Get)?;
            } else {
                print_json(&out)?;
            }
        }
        c @ Cmd::Set { .. } => doc::set(c)?,
        c @ Cmd::SetJson { .. } => doc::set_json(c)?,
        Cmd::Validate { file, integrity } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            read_doc(&file, opts, |_doc| Ok(()))?;
            print_json_line(&serde_json::json!({"ok": true}))?;
        }
        Cmd::Items { op } => items::items_dispatch(op)?,
        Cmd::Blocks { op } => blocks_dispatch(op)?,
        c @ Cmd::ArrayAppend { .. } => doc::array_append(c)?,
        Cmd::Integrity { op } => integrity_dispatch(op)?,
        Cmd::Flow { op } => crate::flow::dispatch(op)?,
        Cmd::Backlog { op } => crate::backlog::dispatch::dispatch(op)?,
        Cmd::Tasks { op } => crate::tasks::dispatch::dispatch(op)?,
        Cmd::Agents { op } => match op {
            AgentsOp::Record {
                harness,
                payload,
                integrity,
            } => crate::agents::dispatch::dispatch_record(&harness, &payload, &integrity)?,
            AgentsOp::List { slug, integrity } => {
                crate::agents::dispatch::dispatch_list(&slug, &integrity)?
            }
        },
        Cmd::Inputs { op } => inputs_dispatch(op)?,
        Cmd::Sweep {
            pattern,
            max_file_bytes,
            max_hits,
            exclude,
        } => {
            let root = repo_or_cwd_root()?;
            let mut opts = SweepOptions {
                max_file_bytes,
                max_hits,
                ..SweepOptions::default()
            };
            opts.exclude.extend(exclude);
            let report = sweep::run(&root, &pattern, &opts)?;
            print_report(sweep::report_json(&report), Rows::Field("hits"))?;
        }
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
                "global_flags": crate::capabilities::build_global_flags(),
                "commands": crate::capabilities::build_agent_context(),
            });
            print_json(&output)?;
        }
    }
    Ok(())
}

fn inputs_dispatch(op: InputsOp) -> Result<()> {
    use crate::inputs;
    let root = repo_or_cwd_root()?;
    let out = match op {
        InputsOp::List {
            pending,
            kind,
            ledger,
            flow,
            scope,
            item,
            integrity,
        } => {
            let path = inputs::path(&root);
            strict_read_check(&path, integrity.strict_read)?;
            // A missing store lists as empty, so there is no sidecar to check.
            if integrity.verify_integrity && path.exists() {
                verify_integrity(&path)?;
            }
            let filter = inputs::Filter {
                pending,
                kinds: kind,
                ledger,
                flow,
                scope,
                item,
            };
            return print_report(inputs::list(&root, &filter)?, Rows::Field("inputs"));
        }
        InputsOp::Add { json, integrity } => {
            let record = read_json_value_from_arg(&json).context("parsing --json")?;
            inputs::add(&root, &record, write_integrity_opts(&integrity))?
        }
        InputsOp::Ack { ids, by, integrity } => {
            inputs::ack(&root, &ids, &by, write_integrity_opts(&integrity))?
        }
        InputsOp::Handle {
            ids,
            by,
            note,
            ndjson,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            match (ndjson, note) {
                (Some(src), _) => {
                    let rows = parse_ndjson(&read_ndjson_source(&src)?)?;
                    inputs::handle_each(&root, &inputs::handle_rows(&rows)?, &by, opts)?
                }
                (None, Some(note)) => inputs::handle(&root, &ids, &by, &note, opts)?,
                (None, None) => unreachable!("clap requires --note without --ndjson"),
            }
        }
        InputsOp::Withdraw { ids, integrity } => {
            inputs::withdraw(&root, &ids, write_integrity_opts(&integrity))?
        }
        InputsOp::Answer {
            question,
            pick,
            text,
            integrity,
        } => inputs::answer(
            &root,
            &question,
            &pick,
            text.as_deref(),
            write_integrity_opts(&integrity),
        )?,
    };
    print_json_compact(&out)
}

fn blocks_dispatch(op: BlocksOp) -> Result<()> {
    match op {
        BlocksOp::Verify { files, block } => {
            let report = blocks_verify(&files, &block)?;
            print_report(report.report, Rows::Field("blocks"))?;
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

mod doc;
mod items;

#[cfg(test)]
mod tests;
