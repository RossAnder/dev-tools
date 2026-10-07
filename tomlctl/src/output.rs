//! Output helpers. Every `{"ok":true,...}` / pretty-printed-JSON /
//! bare-scalar emitter lives here so the dispatch module stays focused on
//! routing and the render contract (pretty vs compact vs raw) is defined in
//! one file.
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
use crate::query::{self, OutputShape, RawArrayHint, ShapeDispatch};

pub(crate) fn print_json(v: &JsonValue) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    serde_json::to_writer_pretty(&mut out, v)?;
    out.write_all(b"\n")?;
    out.flush()?;
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
/// single-line form is load-bearing.
pub(crate) fn print_json_compact(v: &JsonValue) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    serde_json::to_writer(&mut out, v)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
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
/// `report_lines` encoding, one compact value per line.
pub(crate) fn print_report(report: JsonValue, lines: bool, rows: Rows) -> Result<()> {
    if !lines {
        return print_json(&report);
    }
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for line in report_lines(report, rows) {
        serde_json::to_writer(&mut out, &line)?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(())
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
    let rendered = query::emit_raw(v, hint)?;
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
