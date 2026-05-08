//! R21: output helpers extracted from the former monolithic `cli.rs`.
//! Every `{"ok":true,...}` / pretty-printed-JSON / bare-scalar emitter
//! lives here so the dispatch module stays focused on routing and the
//! render contract (pretty vs compact vs raw) is defined in one file.
//!
//! These helpers don't depend on any clap-derive type. They take `&JsonValue`
//! plus an optional `OutputShape` and write to stdout. That's why they're a
//! top-level sibling of `cli/` rather than scoped under it: nothing about their
//! shape says "CLI".

use anyhow::Result;
use serde_json::Value as JsonValue;
use std::io::{BufWriter, Write};

use crate::io::ScalarMutationPlan;
use crate::items::MutationPlan;
use crate::query::{self, OutputShape, ShapeDispatch};

pub(crate) fn print_json(v: &JsonValue) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    serde_json::to_writer_pretty(&mut out, v)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// T10: emit the `--dry-run` summary for `items remove --dry-run` and
/// `items apply --dry-run`. Output shape is a single compact JSON line:
///
/// ```text
/// {"ok":true,"dry_run":true,"would_change":{"kind":"items","added":N,"updated":N,"removed":N,"skipped":N,"ids":[...]}}
/// ```
///
/// `ids` is the concatenation `[...added, ...updated, ...removed]` in
/// that order, matching `MutationPlan::union_ids`. `N` values are plain
/// integer counts (not arrays) so the output stays stable and terse
/// across both dispatch arms. `skipped` surfaces the dedupe-skipped row
/// count from `MutationPlan.skipped`.
///
/// R10: the `kind` discriminator — placed first inside `would_change` —
/// lets consumers branch on `would_change.kind` rather than on which
/// subcommand they invoked. Items-shape envelopes carry `kind:"items"`;
/// scalar-shape envelopes (built by `build_dry_run_scalar_envelope`)
/// carry `kind:"scalar"`. The discriminator was added alongside the
/// existing keys (additive, no version bump): existing consumers reading
/// `added`/`updated`/`removed`/`skipped`/`ids` continue to work.
pub(crate) fn emit_dry_run_plan(plan: &MutationPlan) -> Result<()> {
    let summary = serde_json::json!({
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
    });
    print_json_compact(&summary)
}

/// Compact single-line sibling of `print_json`, used for the `{"ok":true,...}`
/// terminal status lines emitted by write-path dispatch arms (R83). Keeping
/// this separate from `print_json` preserves the pretty-printed contract that
/// downstream consumers rely on for read-path output (tests + humans) while
/// letting every OK-status emitter funnel through a single helper rather
/// than hand-constructing JSON strings. Compact form also matches the
/// pre-refactor byte-for-byte output so integration tests that do a
/// `.contains(r#"{"ok":true,"added":N}"#)` continue to pass.
pub(crate) fn print_json_compact(v: &JsonValue) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    serde_json::to_writer(&mut out, v)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// T2: emit one bare-scalar value to stdout, followed by exactly one
/// trailing newline. The trailing `\n` is deliberate — bash `read -r N`
/// consumes up to a newline, so agents piping tomlctl output into
/// variable-binding shell loops expect every bare-value emission to end
/// in one. For the `--lines --raw --pluck` streaming path this helper is
/// NOT called per line (that path uses `query::emit_raw` directly into a
/// pre-locked writer for throughput); the semantics are the same.
///
/// R14: the scalar-rendering rules live in `query::emit_raw` — this
/// helper is the I/O wrapper that adds stdout locking, buffering, and
/// the trailing newline. Keeping `emit_raw` in `query` keeps the module
/// layering honest (cli depends on query, not the reverse).
pub(crate) fn print_raw_value(v: &JsonValue) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    out.write_all(query::emit_raw(v)?.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// T2 / R16: `items list --raw` dispatch-side wrapper. The per-shape
/// render logic lives in `ShapeDispatch::raw_emit` on `OutputShape`, so
/// adding a new shape variant forces one edit there rather than here PLUS
/// a second match on `shape` in this file. This function's only job now
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
    let rendered = shape.raw_emit(v)?;
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
/// R10: the `kind` discriminator — placed first inside `would_change` —
/// lets consumers branch on `would_change.kind` rather than on which
/// subcommand they invoked. Scalar-shape envelopes carry `kind:"scalar"`;
/// items-shape envelopes (built by `emit_dry_run_plan`) carry
/// `kind:"items"`. The discriminator was added alongside the existing
/// keys (additive, no version bump): existing consumers reading
/// `path`/`old`/`new` continue to work.
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

/// T6a: emit the `--dry-run` envelope for `set` / `set-json`. Companion to
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
        // R10: `kind` is the first field inside `would_change`.
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
}
