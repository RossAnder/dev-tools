//! The TOML document writers `set`, `set-json` and `array-append`. Each takes
//! the whole `Cmd` value so its variant's fields are destructured here alone.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;

use crate::cli::types::Cmd;
use crate::convert::{ScalarType, maybe_date_coerce, parse_scalar, set_at_path};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{
    compute_set_json_mutation, compute_set_mutation, dry_run_read_opts, mutate_doc, on_missing_for,
    read_doc, read_json_value_from_arg, read_ndjson_source, stamped_at, warn_if_created,
    warn_if_read_outside_claude,
};
use crate::items::{compute_array_append_mutation, parse_ndjson};
use crate::output::{
    build_dry_run_scalar_envelope, emit_dry_run_plan, emit_dry_run_scalar, print_json_compact,
};

use super::{write_envelope, write_integrity_opts};

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

/// One `set` assignment: dotted path, raw value, and the forced type (the
/// positional pair's `--type`; `--set` pairs always infer).
struct SetPair {
    path: String,
    value: String,
    ty: Option<ScalarType>,
}

/// Collects the positional pair followed by every `--set PATH=VALUE`,
/// refusing a malformed pair or a path given twice.
fn collect_set_pairs(
    path: Option<String>,
    value: Option<String>,
    ty: Option<ScalarType>,
    extra: Vec<String>,
) -> Result<Vec<SetPair>> {
    let mut pairs = Vec::with_capacity(extra.len() + 1);
    if let (Some(path), Some(value)) = (path, value) {
        pairs.push(SetPair { path, value, ty });
    }
    for raw in extra {
        let Some((path, value)) = raw.split_once('=') else {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("`--set {raw}` needs the form PATH=VALUE"),
            ));
        };
        if path.split('.').any(str::is_empty) {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("`--set {raw}` has an empty path segment"),
            ));
        }
        pairs.push(SetPair {
            path: path.to_string(),
            value: value.to_string(),
            ty: None,
        });
    }
    let mut seen = HashSet::new();
    for p in &pairs {
        if !seen.insert(p.path.as_str()) {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("path `{}` is set more than once", p.path),
            ));
        }
    }
    Ok(pairs)
}

/// `tomlctl set`. Called only with a `Cmd::Set`.
pub(super) fn set(cmd: Cmd) -> Result<()> {
    let Cmd::Set {
        file,
        path,
        value,
        ty,
        set,
        dry_run,
        integrity,
        stamp,
    } = cmd
    else {
        unreachable!("doc::set dispatched with a non-`set` command");
    };
    refuse_json_extension_for_toml_writers(&file)?;
    let pairs = collect_set_pairs(path, value, ty, set)?;
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
        // Each pair is planned against the doc as the earlier pairs leave
        // it, so a later pair nesting under a table an earlier one creates
        // previews the way the live write applies it.
        let read_opts = dry_run_read_opts(integrity.verify_integrity);
        let plans = read_doc(&file, read_opts, |doc| {
            let mut work = doc.clone();
            let mut plans = Vec::with_capacity(pairs.len());
            for p in &pairs {
                plans.push(compute_set_mutation(&work, &p.path, &p.value, p.ty)?);
                set_at_path(&mut work, &p.path, parse_scalar(&p.value, p.ty)?)?;
            }
            Ok(plans)
        })?;
        if let [plan] = plans.as_slice() {
            return emit_dry_run_scalar(plan);
        }
        let changes: Vec<JsonValue> = plans
            .iter()
            .map(|plan| build_dry_run_scalar_envelope(plan)["would_change"].take())
            .collect();
        return print_json_compact(&serde_json::json!({
            "ok": true,
            "dry_run": true,
            "pairs": changes,
        }));
    }
    let opts = write_integrity_opts(&integrity);
    // Auto-create a missing target (default), or fail with the strict
    // not_found error (`--no-create`).
    let on_missing = on_missing_for(&file, integrity.no_create)?;
    // Surface the `created` signal — `"created"` + `"path"` in the
    // success envelope, plus the one-line stderr guidance when seeded.
    let created = mutate_doc(
        &file,
        integrity.allow_outside,
        opts,
        on_missing,
        stamped_at(&file, !stamp.no_stamp, |doc| {
            for p in &pairs {
                let v = parse_scalar(&p.value, p.ty)?;
                set_at_path(doc, &p.path, v)?;
            }
            Ok(())
        }),
    )?;
    write_envelope(&file, created)
}

/// `tomlctl set-json`. Called only with a `Cmd::SetJson`.
pub(super) fn set_json(cmd: Cmd) -> Result<()> {
    let Cmd::SetJson {
        file,
        path,
        json,
        dry_run,
        integrity,
        stamp,
    } = cmd
    else {
        unreachable!("doc::set_json dispatched with a non-`set-json` command");
    };
    refuse_json_extension_for_toml_writers(&file)?;
    // Parse stdin/literal JSON straight into a `JsonValue`, skipping
    // the intermediate String allocation. Keeping the parse outside
    // the `mutate_doc` closure means a malformed payload fails
    // before the doc is opened; keeping it above the `if dry_run`
    // branch means a malformed `--json` fails identically in
    // dry-run and live mode.
    let parsed: JsonValue = read_json_value_from_arg(&json).context("parsing --json")?;
    if dry_run {
        // See `set` for the rationale on this advisory warn. Same
        // threat shape: a caller passing `--dry-run /etc/passwd` to
        // set-json would otherwise silently parse the file as TOML and
        // echo it back in the plan envelope.
        warn_if_read_outside_claude(&file);
        let read_opts = dry_run_read_opts(integrity.verify_integrity);
        let plan = read_doc(&file, read_opts, |doc| {
            compute_set_json_mutation(doc, &path, &parsed)
        })?;
        return emit_dry_run_scalar(&plan);
    }
    let opts = write_integrity_opts(&integrity);
    // Auto-create policy; see `set`.
    let on_missing = on_missing_for(&file, integrity.no_create)?;
    // Surface `created` + `path` (see `set`).
    let created = mutate_doc(
        &file,
        integrity.allow_outside,
        opts,
        on_missing,
        stamped_at(&file, !stamp.no_stamp, |doc| {
            let last_key = path
                .rsplit_once('.')
                .map(|(_, k)| k)
                .unwrap_or(path.as_str());
            let v = maybe_date_coerce(last_key, &parsed)?;
            set_at_path(doc, &path, v)
        }),
    )?;
    write_envelope(&file, created)
}

/// `tomlctl array-append`. Called only with a `Cmd::ArrayAppend`.
pub(super) fn array_append(cmd: Cmd) -> Result<()> {
    let Cmd::ArrayAppend {
        file,
        array,
        json,
        ndjson,
        fields,
        dry_run,
        integrity,
        stamp,
    } = cmd
    else {
        unreachable!("doc::array_append dispatched with a non-`array-append` command");
    };
    refuse_json_extension_for_toml_writers(&file)?;
    // clap's `conflicts_with` keeps `--ndjson` apart from the single-row
    // sources; enforce "at least one" here since clap has no first-class
    // required-one-of primitive across a flattened group.
    if json.is_none() && ndjson.is_none() && fields.is_empty() {
        bail!(
            "array-append requires --json, a field flag (--set / --set-json / --set-file) or --ndjson (e.g. `--set kind=x` or `--json '{{\"k\":\"v\"}}'` for a single row, `--ndjson rows.ndjson` for a batch)"
        );
    }
    // The rows parse sits above the dry-run/live split so both paths
    // share parse semantics. `--json` / `--ndjson` resolution
    // (including stdin) happens once.
    let rows: Vec<JsonValue> = if let Some(nd) = ndjson {
        let text = read_ndjson_source(&nd)?;
        parse_ndjson(&text)?
    } else {
        // Parse straight to `JsonValue`, avoiding a `read_json_arg`
        // String + `serde_json::from_str` two-step.
        let base = json
            .map(|j| read_json_value_from_arg(&j).context("parsing --json"))
            .transpose()?;
        if let Some(parsed) = base.as_ref().filter(|v| !v.is_object()) {
            bail!(
                "--json must be a JSON object (e.g. {{\"k\":\"v\"}}); got JSON {}",
                crate::convert::json_type_name(parsed)
            );
        }
        let row = crate::fields::build(&fields, base)?.expect("a source is present: checked above");
        vec![row]
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
        return emit_dry_run_plan(&plan);
    }
    let opts = write_integrity_opts(&integrity);
    let mut appended: usize = 0;
    // Auto-create policy.
    let on_missing = on_missing_for(&file, integrity.no_create)?;
    // Surface `created` + `path` alongside the `appended` count.
    let created = mutate_doc(
        &file,
        integrity.allow_outside,
        opts,
        on_missing,
        stamped_at(&file, !stamp.no_stamp, |doc| {
            appended = crate::items::array_append(doc, &array, &rows)?;
            Ok(())
        }),
    )?;
    warn_if_created(&file, created);
    print_json_compact(&serde_json::json!({
        "ok": true,
        "appended": appended,
        "created": created,
        "path": file.display().to_string(),
    }))
}
