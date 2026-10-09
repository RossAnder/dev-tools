//! The `items` verb group's dispatcher: one arm per `ItemsOp`, each
//! delegating to `crate::items` / `crate::dedup` / `crate::io` for the work
//! and printing through `crate::output`.

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;

use crate::cli::types::{ItemsOp, LegacyShortcuts, OnStale};
use crate::clusters::items_clusters;
use crate::dedup::{
    items_find_duplicates, items_find_duplicates_across, items_find_duplicates_across_json,
    items_find_duplicates_json,
};
use crate::fields::{self, FieldArgs};
use crate::io::{
    dry_run_read_opts, mutate_doc, mutate_doc_conditional, mutate_doc_plan, on_missing_for,
    read_doc, read_doc_borrowed, read_doc_either, read_json_arg, read_json_value_from_arg,
    read_ndjson_source, repo_or_cwd_root, stamped, stamped_conditional, stamped_plan,
    strict_read_check, warn_if_created, warn_if_read_outside_claude,
};
use crate::items::{
    AddManyOutcome, AddOutcome, StaleOp, StalePolicy, compute_add_many_mutation,
    compute_add_mutation, compute_apply_mutation_with, compute_backfill_mutation,
    compute_remove_mutation, compute_update_mutation, dedup_id_disabled, items_add_many,
    items_add_many_with_dedupe, items_add_to, items_add_value_with_dedupe_to, items_fingerprint,
    items_get_from, items_get_from_json, items_infer_and_next_id, items_next_id, items_update_to,
    parse_apply_ops, parse_ndjson,
};
use crate::items_sweep::{items_sweep, outcome_json, update_plan};
use crate::orphans::items_orphans;
use crate::output::{
    Rows, build_dry_run_plan_envelope, emit_dry_run_plan, print_json, print_json_compact,
    print_query_list, print_report, print_text,
};
use crate::query::Query;
use crate::sweep::SweepOptions;

use super::{read_integrity_opts, write_envelope, write_integrity_opts};

/// Maximum number of ops accepted in a single `items apply` batch.
/// The 32 MiB stdin cap alone does not bound op count — a well-formed 32 MiB
/// JSON array of tiny `{"op":"update","id":"Rx"}` records can hold tens of
/// thousands of operations, and `items_apply_to_opts` iterates serially.
/// 10_000 is far above any legitimate batch (typical ledgers have ~50 items
/// and typical apply batches ≤ 60 ops) while still bounded enough that an
/// accidental loop-generated mega-payload fails fast instead of timing out
/// the wrapping shell.
const MAX_OPS_PER_APPLY: usize = 10_000;

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

fn skipped_stale_json(skipped: &[StaleOp]) -> JsonValue {
    JsonValue::Array(skipped.iter().map(StaleOp::to_json).collect())
}

/// An `items add` / `update` payload: the raw `--json` argument when no field
/// flag is given, so that path reads and parses exactly as before, or the
/// field flags already merged over it. Consumed once, since `-` claims stdin.
enum Payload {
    Arg(String),
    Merged(JsonValue),
}

impl Payload {
    /// `None` only when neither `--json` nor a field flag was given.
    fn resolve(json: Option<String>, fields: &FieldArgs) -> Result<Option<Self>> {
        if fields.is_empty() {
            return Ok(json.map(Payload::Arg));
        }
        let base = json
            .map(|j| read_json_value_from_arg(&j).context("parsing --json"))
            .transpose()?;
        Ok(fields::build(fields, base)?.map(Payload::Merged))
    }

    fn into_value(self) -> Result<JsonValue> {
        match self {
            Payload::Arg(arg) => read_json_value_from_arg(&arg).context("parsing --json"),
            Payload::Merged(v) => Ok(v),
        }
    }

    fn into_text(self) -> Result<String> {
        match self {
            Payload::Arg(arg) => read_json_arg(&arg),
            Payload::Merged(v) => Ok(serde_json::to_string(&v)?),
        }
    }
}

pub(super) fn items_dispatch(op: ItemsOp) -> Result<()> {
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
            let q = Query::from_query_input(&query.to_query_input(&legacy, crate::output::opts()))?;
            read_doc(&file, opts, |doc| print_query_list(doc, &array, &q))?;
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
            fields,
            array,
            dedupe_by,
            id_prefix,
            dry_run,
            integrity,
            stamp,
        } => {
            let opts = write_integrity_opts(&integrity);
            let stamp = !stamp.no_stamp;
            let dedupe_fields = parse_dedupe_fields(dedupe_by.as_deref())?;
            let payload = Payload::resolve(json, &fields)?
                .expect("clap requires --json unless a field flag is present");
            if let Some(prefix) = id_prefix.as_deref() {
                let patch = payload.into_value()?;
                // Live and dry-run both run the one-row `items_add_many_with_dedupe`
                // funnel, so their validation errors are byte-identical.
                let rows = vec![patch];
                if dry_run {
                    warn_if_read_outside_claude(&file);
                    let read_opts = dry_run_read_opts(integrity.verify_integrity);
                    let plan = read_doc(&file, read_opts, |doc| {
                        compute_add_many_mutation(
                            doc,
                            &array,
                            &rows,
                            None,
                            &dedupe_fields,
                            Some(prefix),
                        )
                    })?;
                    emit_dry_run_plan(&plan)?;
                    return Ok(());
                }
                // The id is minted inside the lock closure, so two concurrent
                // adds can never observe the same high-water mark.
                let mut outcome: Option<(AddOutcome, String)> = None;
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                let created = mutate_doc_conditional(
                    &file,
                    integrity.allow_outside,
                    opts,
                    on_missing,
                    stamped_conditional(stamp, |doc| {
                        let mut many = items_add_many_with_dedupe(
                            doc,
                            &array,
                            &rows,
                            None,
                            &dedupe_fields,
                            Some(prefix),
                        )?;
                        let result = match many.skipped_rows.pop() {
                            Some(skip) => (
                                AddOutcome::Skipped {
                                    matched_id: skip.matched_id.clone(),
                                },
                                skip.matched_id,
                            ),
                            None => (
                                AddOutcome::Added,
                                many.ids
                                    .pop()
                                    .expect("a one-row add that skips nothing appends"),
                            ),
                        };
                        let mutated = matches!(result.0, AddOutcome::Added);
                        outcome = Some(result);
                        Ok(mutated)
                    }),
                )?;
                warn_if_created(&file, created);
                let path = file.display().to_string();
                match outcome.expect("closure always sets outcome on success") {
                    (AddOutcome::Added, id) => print_json_compact(&serde_json::json!({
                        "ok": true,
                        "added": 1,
                        "id": id,
                        "created": created,
                        "path": path,
                    }))?,
                    (AddOutcome::Skipped { matched_id }, id) => {
                        print_json_compact(&serde_json::json!({
                            "ok": true,
                            "added": 0,
                            "id": id,
                            "matched_id": matched_id,
                            "created": created,
                            "path": path,
                        }))?
                    }
                }
                return Ok(());
            }
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
                let patch = payload.into_value()?;
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = if dedupe_fields.is_empty() {
                    read_doc(&file, read_opts, |doc| {
                        compute_add_mutation(doc, &array, &patch)
                    })?
                } else {
                    let rows = vec![patch];
                    read_doc(&file, read_opts, |doc| {
                        compute_add_many_mutation(doc, &array, &rows, None, &dedupe_fields, None)
                    })?
                };
                emit_dry_run_plan(&plan)?;
                return Ok(());
            }
            if dedupe_fields.is_empty() {
                // No-dedupe path: the always-write `mutate_doc` pipeline runs
                // unconditionally, and the envelope matches the dedupe
                // branch's `Added` arm so `added` reads the same across the
                // add verbs.
                let json = payload.into_text()?;
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                let created = mutate_doc(
                    &file,
                    integrity.allow_outside,
                    opts,
                    on_missing,
                    stamped(stamp, |doc| items_add_to(doc, &array, &json)),
                )?;
                warn_if_created(&file, created);
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "added": 1,
                    "created": created,
                    "path": file.display().to_string(),
                }))?;
            } else {
                // Dedupe path: parse JSON once up-front so we can feed it
                // to the pre-scan inside the lock without a re-parse.
                // `mutate_doc_conditional` elides the write-and-sidecar
                // bump when the scan returns a match; the caller sees
                // `added:0,matched_id:...` and the on-disk file + sidecar
                // are untouched.
                let patch = payload.into_value()?;
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
                    stamped_conditional(stamp, |doc| {
                        let result =
                            items_add_value_with_dedupe_to(doc, patch, &array, &dedupe_fields)?;
                        let mutated = matches!(result, AddOutcome::Added);
                        outcome = Some(result);
                        Ok(mutated)
                    }),
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
            id_prefix,
            dry_run,
            integrity,
            stamp,
        } => {
            let opts = write_integrity_opts(&integrity);
            let stamp = !stamp.no_stamp;
            let dedupe_fields = parse_dedupe_fields(dedupe_by.as_deref())?;
            let id_prefix = id_prefix.as_deref();
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
                    compute_add_many_mutation(
                        doc,
                        &array,
                        &rows,
                        defaults.as_ref(),
                        &dedupe_fields,
                        id_prefix,
                    )
                })?;
                emit_dry_run_plan(&plan)?;
                return Ok(());
            }
            if dedupe_fields.is_empty() && id_prefix.is_none() {
                // No-dedupe path: output shape is `{"ok":true,"added":N}` and
                // the always-write pipeline runs unconditionally.
                let mut added: usize = 0;
                // Auto-create policy.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                // Surface `created` + `path` alongside the `added` count.
                let created = mutate_doc(
                    &file,
                    integrity.allow_outside,
                    opts,
                    on_missing,
                    stamped(stamp, |doc| {
                        added = items_add_many(doc, &array, &rows, defaults.as_ref())?;
                        Ok(())
                    }),
                )?;
                warn_if_created(&file, created);
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "added": added,
                    "created": created,
                    "path": file.display().to_string(),
                }))?;
            } else {
                // Dedupe / id-minting path: run the pre-scan, mint and
                // append loop inside the lock via `mutate_doc_conditional`.
                // `ids` is reported only under `--id-prefix`, the skip keys
                // only under `--dedupe-by`. Skip the file write
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
                    stamped_conditional(stamp, |doc| {
                        let result = items_add_many_with_dedupe(
                            doc,
                            &array,
                            &rows,
                            defaults.as_ref(),
                            &dedupe_fields,
                            id_prefix,
                        )?;
                        let mutated = result.added > 0;
                        outcome = Some(result);
                        Ok(mutated)
                    }),
                )?;
                warn_if_created(&file, created);
                let outcome = outcome.expect("closure always sets outcome on success");
                let mut envelope = serde_json::Map::new();
                envelope.insert("ok".into(), JsonValue::Bool(true));
                envelope.insert("added".into(), outcome.added.into());
                if id_prefix.is_some() {
                    envelope.insert("ids".into(), outcome.ids.into());
                }
                if !dedupe_fields.is_empty() {
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
                    envelope.insert("skipped".into(), outcome.skipped_rows.len().into());
                    envelope.insert("skipped_rows".into(), skipped_rows_json.into());
                }
                envelope.insert("created".into(), created.into());
                envelope.insert("path".into(), file.display().to_string().into());
                print_json_compact(&JsonValue::Object(envelope))?;
            }
        }
        ItemsOp::Update {
            file,
            id,
            json,
            fields,
            unset,
            array,
            dry_run,
            integrity,
            stamp,
        } => {
            // Enforce "at least one of --json / a field flag / --unset" here
            // rather than as a required ArgGroup, matching `array-append`'s
            // source check: this `bail!` surfaces as an `--error-format json`
            // envelope, a clap refusal as exit-2 usage prose. An update naming
            // none would rewrite the ledger and its sidecar for no field change.
            if json.is_none() && fields.is_empty() && unset.is_empty() {
                bail!(
                    "items update requires one of --json, a field flag (--set / --set-json / --set-file) or --unset (e.g. `--set status=fixed` to set a field, `--unset notes` to remove one)"
                );
            }
            let opts = write_integrity_opts(&integrity);
            // The json arg parse sits above the dry-run/live split.
            // `compute_update_mutation` takes the raw &str and parses
            // internally (same surface as `items_update_to`), so both
            // branches share the resolved string.
            //
            // Field flags merge over `--json` and the merged object is
            // serialised back to the same string form.
            //
            // An absent patch defaults to an empty one, so the merge loop
            // has no keys and the `--unset` removals are the whole field
            // mutation — but not the whole write: an `--unset` naming a
            // fingerprinted field the row carries still drives
            // `apply_dedup_id_on_update`'s recompute over the post-unset row.
            // The guard above is what keeps that default from degenerating
            // into a no-op write.
            let json = match Payload::resolve(json, &fields)? {
                Some(payload) => payload.into_text()?,
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
            let created = mutate_doc(
                &file,
                integrity.allow_outside,
                opts,
                on_missing,
                stamped(!stamp.no_stamp, |doc| {
                    items_update_to(doc, &array, &id, &json, &unset)
                }),
            )?;
            write_envelope(&file, created)?;
        }
        ItemsOp::Remove {
            file,
            id,
            array,
            dry_run,
            integrity,
            stamp,
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
                let created = mutate_doc_plan(
                    &file,
                    integrity.allow_outside,
                    opts,
                    on_missing,
                    stamped_plan(!stamp.no_stamp, |doc| {
                        compute_remove_mutation(doc, &array, &id)
                    }),
                )?;
                write_envelope(&file, created)?;
            }
        }
        ItemsOp::Apply {
            file,
            ops,
            array,
            no_remove,
            on_stale,
            dry_run,
            integrity,
            stamp,
        } => {
            let opts = write_integrity_opts(&integrity);
            let policy = match on_stale {
                OnStale::Abort => StalePolicy::Abort,
                OnStale::Skip => StalePolicy::Skip,
            };
            // Parse `--ops` ONCE at the CLI boundary and thread the parsed
            // `JsonValue` through both the `MAX_OPS_PER_APPLY` length check
            // and `compute_apply_mutation_with`. An NDJSON payload arrives here
            // already folded into the array form, so everything below is
            // shared by both encodings.
            let parsed_ops: JsonValue = read_json_arg(&ops)
                .and_then(|text| parse_apply_ops(&text))
                .context("parsing --ops")?;
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
                // before the I/O stage. `compute_apply_mutation_with` runs
                // `items_apply_parsed_to_opts` on a cloned doc, so every
                // validation gate — `--no-remove`, op-shape, missing id,
                // dedup_id auto-populate, `expect` under `--on-stale` — fires
                // with a byte-identical error surface.
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let guarded = read_doc(&file, read_opts, |doc| {
                    compute_apply_mutation_with(doc, &array, &parsed_ops, no_remove, policy)
                })?;
                let mut envelope = build_dry_run_plan_envelope(&guarded.plan);
                envelope["skipped_stale"] = skipped_stale_json(&guarded.skipped_stale);
                print_json_compact(&envelope)?;
            } else {
                // Auto-create policy. An all-`update`/all-`remove` batch
                // against a freshly-seeded missing file errors in
                // `compute_apply_mutation_with` (no matching id) BEFORE the
                // persist, so nothing is written. Batches with `add` ops
                // seed-then-append into the new file — `created=true` there.
                let on_missing = on_missing_for(&file, integrity.no_create)?;
                let mut skipped_stale = Vec::new();
                let created = mutate_doc_plan(
                    &file,
                    integrity.allow_outside,
                    opts,
                    on_missing,
                    stamped_plan(!stamp.no_stamp, |doc| {
                        let guarded = compute_apply_mutation_with(
                            doc,
                            &array,
                            &parsed_ops,
                            no_remove,
                            policy,
                        )?;
                        skipped_stale = guarded.skipped_stale;
                        Ok(guarded.plan)
                    }),
                )?;
                warn_if_created(&file, created);
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "created": created,
                    "path": file.display().to_string(),
                    "skipped_stale": skipped_stale_json(&skipped_stale),
                }))?;
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
            // subcommand gets `kind=not_found` instead of the `<prefix>1`
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
            let id = if !file.exists() {
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
                items_next_id(&empty_doc, prefix)?
            } else {
                let opts = read_integrity_opts(&integrity);
                read_doc(&file, opts, |doc| {
                    if infer_from_file {
                        items_infer_and_next_id(doc)
                    } else {
                        let prefix =
                            prefix.as_deref().expect("clap required_unless_present guarantees prefix is Some when infer_from_file is false");
                        items_next_id(doc, prefix)
                    }
                })?
            };
            // Bare text, not a JSON string, so a shell caller can use the id
            // without stripping quotes.
            print_text(&format!("{id}\n"))?;
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
                        // through the borrowed parse; the cross-ledger join
                        // then runs in JsonValue space via
                        // items_find_duplicates_across_json.
                        let primary = read_doc_borrowed(&file)?;
                        let primary_items = crate::io::items_array_json(&primary, "items").to_vec();
                        let other = read_doc_borrowed(&other_path)?;
                        let other_items = crate::io::items_array_json(&other, "items").to_vec();
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
            print_report(JsonValue::Array(groups), Rows::Top)?;
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
            print_report(JsonValue::Array(orphans), Rows::Top)?;
        }
        ItemsOp::Sweep {
            file,
            ids,
            update,
            dry_run,
            max_file_bytes,
            max_hits,
            integrity,
            stamp,
        } => {
            if dry_run && !update {
                bail!(
                    "items sweep --dry-run requires --update (the read-only sweep writes nothing to preview)"
                );
            }
            let root = repo_or_cwd_root()?;
            let sweep_opts = SweepOptions {
                max_file_bytes,
                max_hits,
                ..SweepOptions::default()
            };
            if !update {
                // `read_doc` never seeds, so a missing ledger surfaces as
                // `kind=not_found` rather than being created.
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let results = read_doc(&file, read_opts, |doc| {
                    items_sweep(doc, &file, &root, &ids, &sweep_opts)
                })?;
                print_report(outcome_json(&results), Rows::Field("items"))?;
                return Ok(());
            }
            if dry_run {
                warn_if_read_outside_claude(&file);
                let read_opts = dry_run_read_opts(integrity.verify_integrity);
                let plan = read_doc(&file, read_opts, |doc| {
                    let results = items_sweep(doc, &file, &root, &ids, &sweep_opts)?;
                    update_plan(doc, &results)
                })?;
                emit_dry_run_plan(&plan)?;
                return Ok(());
            }
            let opts = write_integrity_opts(&integrity);
            // A seeded ledger has no items to sweep, so a missing file is
            // `not_found` here as on the read-only run; `--no-create` is moot.
            let on_missing = crate::io::OnMissing::Error;
            // The sweep walks every tracked file, so it runs once, in-lock;
            // an unchanged ledger skips the write and the sidecar bump.
            let mut updated: Vec<String> = Vec::new();
            let created = mutate_doc_conditional(
                &file,
                integrity.allow_outside,
                opts,
                on_missing,
                stamped_conditional(!stamp.no_stamp, |doc| {
                    let results = items_sweep(doc, &file, &root, &ids, &sweep_opts)?;
                    let plan = update_plan(doc, &results)?;
                    if plan.updated.is_empty() {
                        return Ok(false);
                    }
                    updated = plan.updated;
                    *doc = plan.new_doc;
                    Ok(true)
                }),
            )?;
            warn_if_created(&file, created);
            print_json_compact(&serde_json::json!({
                "ok": true,
                "updated": updated,
                "created": created,
                "path": file.display().to_string(),
            }))?;
        }
        ItemsOp::Clusters {
            file,
            ids,
            integrity,
        } => {
            strict_read_check(&file, integrity.strict_read)?;
            let opts = read_integrity_opts(&integrity);
            let root = repo_or_cwd_root()?;
            let out = read_doc(&file, opts, |doc| items_clusters(doc, &root, &ids))?;
            print_report(out, Rows::Field("clusters"))?;
        }
        ItemsOp::BackfillDedupId {
            file,
            array,
            dry_run,
            integrity,
            stamp,
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
                let created = mutate_doc_plan(
                    &file,
                    integrity.allow_outside,
                    opts,
                    on_missing,
                    stamped_plan(!stamp.no_stamp, |doc| {
                        let plan = compute_backfill_mutation(doc, &array)?;
                        written = plan.updated.len();
                        Ok(plan)
                    }),
                )?;
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
