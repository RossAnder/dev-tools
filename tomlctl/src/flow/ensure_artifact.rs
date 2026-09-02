//! `flow ensure-artifact` — report (and optionally bootstrap) a flow
//! artifact under `.claude/flows/<slug>/`.
//!
//! Read-only by default. `--bootstrap` materialises the atomic 2-line
//! `execution-record.toml` skeleton (matching the shared-block contract
//! `/plan-new` documents) and refreshes its `.sha256` sidecar. For every
//! other artifact kind, `--bootstrap` is a no-op — those files are
//! command-specific and bootstrap on first write by their owning command.
//!
//! Read-only report shape:
//! ```json
//! {"exists":true|false,"path":"<rel>","sidecar_path":"<rel>","sidecar_valid":true|false|null}
//! ```
//! - `sidecar_valid=null` when the artifact file does not exist.
//! - `sidecar_valid=false` when the sidecar is missing OR its digest
//!   doesn't match the artifact bytes (no auto-repair).
//! - `sidecar_valid=true` when the digests agree.
//!
//! Bootstrap success additionally surfaces `"bootstrapped":true`. A
//! `--bootstrap` request against a kind other than `execution-record`
//! emits the read-only report plus a `"bootstrap_noop"` marker explaining
//! that the kind's owning command bootstraps it on first write.
//!
//! Containment: a slug that escapes `.claude/` (`..` / absolute / path
//! separator) errors `kind=validation` before any disk touch.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value as JsonValue, json};

use crate::cli::{ArtifactKind, WriteIntegrityArgs, write_integrity_opts};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::{sha256_hex_of_file, sidecar_path};
use crate::io::{
    atomic_write, guard_write_path, recheck_claude_containment, relativise, repo_or_cwd_root,
    with_exclusive_lock,
};
use crate::output::print_json_compact;

pub(crate) fn dispatch(
    slug: String,
    kind: ArtifactKind,
    bootstrap: bool,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    // Containment guard fires before any FS touch — a malformed slug must
    // surface as `kind=validation` regardless of the requested mode.
    validate_slug_lenient(&slug)?;
    let root = repo_or_cwd_root()?;
    let artifact_path = artifact_path_for(&root, &slug, kind);

    // Re-assert containment on the resolved path. A slug that passes the
    // syntactic guard but somehow lands outside `.claude/` (defensive
    // coverage: future schema additions, symlinks at the slug parent)
    // still fails closed.
    assert_under_claude(&root, &artifact_path)?;

    if bootstrap {
        match kind {
            ArtifactKind::ExecutionRecord => {
                bootstrap_execution_record(&root, &artifact_path, dry_run, &integrity)
            }
            _ => emit_bootstrap_noop(&root, &artifact_path, kind),
        }
    } else {
        let report = compute_report(&root, &artifact_path)?;
        print_json_compact(&report)
    }
}

/// Map the artifact kind to its conventional filename under
/// `<root>/.claude/flows/<slug>/`.
fn artifact_filename(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Context => "context.toml",
        ArtifactKind::ExecutionRecord => "execution-record.toml",
        ArtifactKind::ReviewLedger => "review-ledger.toml",
        ArtifactKind::OptimiseFindings => "optimise-findings.toml",
        ArtifactKind::PlanReviewFindings => "plan-review-findings.toml",
    }
}

/// Map kind → user-facing kebab-case label, used in the bootstrap-noop
/// marker prose so callers see the same string they passed at the CLI.
fn artifact_kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Context => "context",
        ArtifactKind::ExecutionRecord => "execution-record",
        ArtifactKind::ReviewLedger => "review-ledger",
        ArtifactKind::OptimiseFindings => "optimise-findings",
        ArtifactKind::PlanReviewFindings => "plan-review-findings",
    }
}

/// Compose `<root>/.claude/flows/<slug>/<filename>`.
fn artifact_path_for(root: &Path, slug: &str, kind: ArtifactKind) -> PathBuf {
    root.join(".claude")
        .join("flows")
        .join(slug)
        .join(artifact_filename(kind))
}

/// Reject slugs containing path separators, parent-dir traversal, absolute
/// roots, or NUL bytes. The init-side sanitiser is stricter
/// (`^[a-z0-9][a-z0-9-]{0,63}$`); this helper is deliberately narrower —
/// `ensure-artifact` is also called against pre-existing flows whose slugs
/// were minted under earlier conventions, so we only reject the actively
/// dangerous shapes rather than the full init-grade allow-list. The
/// `_lenient` suffix keeps it distinguishable from `flow::init`'s
/// strict-regex `validate_slug`.
fn validate_slug_lenient(slug: &str) -> Result<()> {
    if slug.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "invalid slug: empty",
        ));
    }
    if slug.contains('/')
        || slug.contains('\\')
        || slug == ".."
        || slug == "."
        || slug.starts_with("..")
        || slug.contains('\0')
    {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "invalid slug `{slug}`: must not contain path separators, traversal components, or NUL"
            ),
        ));
    }
    Ok(())
}

/// Belt-and-braces containment guard on the resolved artifact path. We
/// canonicalise the closest existing ancestor and assert it stays under
/// `<root>/.claude/`. A slug that smuggled through `validate_slug` (e.g.
/// future symlink at `flows/<slug>` pointing outside) still fails here.
fn assert_under_claude(root: &Path, artifact: &Path) -> Result<()> {
    let claude_dir = root.join(".claude");
    let claude_canonical = claude_dir.canonicalize().unwrap_or(claude_dir);

    // Walk up to the closest existing ancestor and canonicalise it. The
    // artifact itself often does not exist yet (read-only report on a
    // missing kind, or first-time bootstrap); canonicalising a missing
    // path errors, so we anchor on the nearest real directory.
    let mut anchor: &Path = artifact;
    let canonical_anchor = loop {
        match anchor.canonicalize() {
            Ok(c) => break c,
            Err(_) => match anchor.parent() {
                Some(p) if !p.as_os_str().is_empty() => anchor = p,
                _ => {
                    return Err(tagged_err(
                        ErrorKind::Validation,
                        Some(artifact.to_path_buf()),
                        format!(
                            "invalid slug: artifact path {} could not be anchored under .claude/",
                            artifact.display()
                        ),
                    ));
                }
            },
        }
    };
    if canonical_anchor.starts_with(&claude_canonical) || canonical_anchor == claude_canonical {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::Validation,
        Some(artifact.to_path_buf()),
        format!(
            "invalid slug: artifact path {} resolves outside .claude/",
            artifact.display()
        ),
    ))
}

/// Compute the read-only verdict — does the artifact exist, and (if so) is
/// its sidecar present and digest-matching? Never mutates the filesystem.
fn compute_report(root: &Path, artifact: &Path) -> Result<JsonValue> {
    let sidecar = sidecar_path(artifact);
    let exists = artifact.exists();
    let sidecar_valid: JsonValue = if !exists {
        // Nothing to validate — null per the contract so callers can
        // distinguish "no artifact" from "artifact-with-bad-sidecar".
        JsonValue::Null
    } else if !sidecar.exists() {
        // Missing sidecar with the artifact present is a hard `false`;
        // we deliberately do not auto-repair on the report path.
        JsonValue::Bool(false)
    } else {
        match sidecar_matches(artifact, &sidecar) {
            Ok(ok) => JsonValue::Bool(ok),
            // Any I/O / parse failure during sidecar comparison is
            // treated as "invalid" rather than aborting — the report is
            // an introspection primitive and a malformed sidecar is
            // exactly what we're meant to surface.
            Err(_) => JsonValue::Bool(false),
        }
    };
    Ok(json!({
        "exists": exists,
        "path": relativise(root, artifact),
        "sidecar_path": relativise(root, &sidecar),
        "sidecar_valid": sidecar_valid,
    }))
}

/// True when the `<file>.sha256` sidecar carries a 64-hex-char digest
/// equal to a fresh recompute of `artifact`'s on-disk bytes. Malformed
/// sidecar shape (no digest, wrong length, non-hex) collapses to `false`.
fn sidecar_matches(artifact: &Path, sidecar: &Path) -> Result<bool> {
    let raw = std::fs::read_to_string(sidecar)?;
    let Some(expected) = raw.split_whitespace().next() else {
        return Ok(false);
    };
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(false);
    }
    let actual = sha256_hex_of_file(artifact)?;
    Ok(expected.eq_ignore_ascii_case(&actual))
}

/// Bootstrap path for `kind=execution-record`. When the file is already
/// present (with or without a sidecar) we EMIT THE REPORT and skip the
/// write — bootstrap is idempotent. Live mode runs an atomic 2-line
/// `Write` plus `integrity refresh` under the standard exclusive-lock
/// pipeline. Dry-run emits a `would_change` plan.
fn bootstrap_execution_record(
    root: &Path,
    artifact: &Path,
    dry_run: bool,
    integrity_args: &WriteIntegrityArgs,
) -> Result<()> {
    let sidecar = sidecar_path(artifact);

    // Idempotent fast path: artifact already on disk → emit the read-only
    // report verbatim. Don't auto-refresh the sidecar (callers asking for
    // a sidecar repair use `tomlctl integrity refresh` directly).
    if artifact.exists() {
        let report = compute_report(root, artifact)?;
        return print_json_compact(&report);
    }

    // Source the skeleton from `io::seed_doc_for` — the SAME helper the
    // auto-create write path and `flow::init::bootstrap_execution_record`
    // use — and render it through `toml::to_string_pretty`, the exact
    // writer `io::write_toml_with_sidecar` runs, so every bootstrap route
    // emits byte-identical skeletons (integer `1`, bare date, trailing
    // newline, that key order).
    let seed = crate::io::seed_doc_for(artifact)?;
    let body = toml::to_string_pretty(&seed).context("serialising TOML")?;

    if dry_run {
        // The `would_change` envelope mirrors the items-side dry-run shape
        // — kind discriminator first, plain integer counters, no FS touch.
        let envelope = json!({
            "ok": true,
            "dry_run": true,
            "would_change": {
                "kind": "ensure-artifact",
                "action": "bootstrap",
                "artifact_kind": "execution-record",
                "path": relativise(root, artifact),
                "sidecar_path": relativise(root, &sidecar),
                "bytes": body.len(),
            },
        });
        return print_json_compact(&envelope);
    }

    // Live write. Mirror `flow::active::mutate_active`'s pipeline: take
    // the exclusive lock, run the in-lock `guard_write_path` (post-lock
    // canonicalisation closes the symlink-swap TOCTOU window), atomic-
    // write the body, recheck containment, then refresh the sidecar.
    let allow_outside = integrity_args.allow_outside;
    let opts = write_integrity_opts(integrity_args);
    with_exclusive_lock(artifact, || {
        guard_write_path(artifact, allow_outside)?;
        atomic_write(artifact, body.as_bytes())?;
        if !allow_outside {
            recheck_claude_containment(artifact)?;
        }
        if opts.write_sidecar {
            // Refresh failures escalate identically to the
            // `write_toml_with_sidecar` strict-integrity contract.
            if let Err(e) = crate::io::write_sidecar_for(artifact, body.as_bytes()) {
                if opts.strict {
                    return Err(e);
                }
                eprintln!(
                    "tomlctl: warning: bootstrap wrote {} but sidecar refresh failed: {:#}",
                    artifact.display(),
                    e
                );
            }
        }
        Ok(())
    })?;

    // Final report — recompute against the on-disk state so the
    // `sidecar_valid` field reflects the actual outcome of the refresh
    // (true under happy path, false if the warn-on-refresh-failure branch
    // fired without escalating).
    let mut report = compute_report(root, artifact)?;
    if let Some(obj) = report.as_object_mut() {
        obj.insert("bootstrapped".to_string(), JsonValue::Bool(true));
    }
    print_json_compact(&report)
}

/// `--bootstrap` against a kind other than `execution-record` returns the
/// read-only report plus a marker explaining that the owning command
/// bootstraps the artifact on first write.
fn emit_bootstrap_noop(root: &Path, artifact: &Path, kind: ArtifactKind) -> Result<()> {
    let mut report = compute_report(root, artifact)?;
    let label = artifact_kind_label(kind);
    if let Some(obj) = report.as_object_mut() {
        obj.insert(
            "bootstrap_noop".to_string(),
            JsonValue::String(format!(
                "kind {label} bootstrap is the owning command's responsibility"
            )),
        );
    }
    print_json_compact(&report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_slug_rejects_traversal() {
        assert!(validate_slug_lenient("..").is_err());
        assert!(validate_slug_lenient("../escape").is_err());
        assert!(validate_slug_lenient("a/b").is_err());
        assert!(validate_slug_lenient("a\\b").is_err());
        assert!(validate_slug_lenient("").is_err());
        let err = validate_slug_lenient("../etc/passwd").unwrap_err();
        let tagged = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert!(matches!(tagged.kind, ErrorKind::Validation));
    }

    #[test]
    fn validate_slug_accepts_normal() {
        assert!(validate_slug_lenient("feature-x").is_ok());
        assert!(validate_slug_lenient("a").is_ok());
        assert!(validate_slug_lenient("flow-tracking-overhaul").is_ok());
    }

    #[test]
    fn artifact_filename_maps_every_variant() {
        assert_eq!(artifact_filename(ArtifactKind::Context), "context.toml");
        assert_eq!(
            artifact_filename(ArtifactKind::ExecutionRecord),
            "execution-record.toml"
        );
        assert_eq!(
            artifact_filename(ArtifactKind::ReviewLedger),
            "review-ledger.toml"
        );
        assert_eq!(
            artifact_filename(ArtifactKind::OptimiseFindings),
            "optimise-findings.toml"
        );
        assert_eq!(
            artifact_filename(ArtifactKind::PlanReviewFindings),
            "plan-review-findings.toml"
        );
    }
}
