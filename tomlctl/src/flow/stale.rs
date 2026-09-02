//! T5: `flow stale` — staleness verdict for a flow's `context.toml`.
//!
//! Reads `<root>/.claude/flows/<slug>/context.toml`, locates its top-level
//! `updated` field (a TOML date), compares the age in seconds against the
//! caller-supplied `--threshold` (default `7d`), and emits a JSON verdict:
//!
//! ```json
//! {"stale":true|false,"last_activity":"2026-05-08T00:00:00Z","age_seconds":86400,"reason":"updated within threshold"}
//! ```
//!
//! Reasons: `"updated within threshold"`, `"updated > <N>d ago"`,
//! `"context.toml missing"`, `"updated field missing"`.
//!
//! Read-only. A missing `context.toml` is a meaningful answer (stale=true,
//! reason="context.toml missing"), NOT a `kind=not_found` error — UNLESS
//! `--strict-read` is set, in which case the missing file becomes a tagged
//! error consistent with the rest of the read-side surface.
//!
//! `--threshold` accepts `<n>{s|m|h|d|w}` only; bare numbers are rejected per
//! the plan's "require explicit suffix" rule. `w` expands to `7d`. The parser
//! itself is `crate::time::parse_threshold` — `backlog compact --older-than`
//! reads the same grammar.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::cli::ReadIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{read_toml, repo_or_cwd_root};
use crate::output::{print_json, print_json_compact};
use crate::time::{parse_iso_to_date, parse_threshold, today_utc_date};

pub(crate) fn dispatch(
    slug: String,
    threshold: String,
    json_out: bool,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let threshold_dur = parse_threshold(&threshold)?;
    let context_path = resolve_context_path(&slug)?;

    let verdict = if !context_path.exists() {
        if integrity.strict_read {
            return Err(tagged_err(
                ErrorKind::NotFound,
                Some(context_path.clone()),
                format!("file does not exist: {}", context_path.display()),
            ));
        }
        json!({
            "stale": true,
            "last_activity": JsonValue::Null,
            "age_seconds": JsonValue::Null,
            "reason": "context.toml missing",
        })
    } else {
        let doc = read_toml(&context_path)?;
        compute_verdict(&doc, &threshold, threshold_dur)?
    };

    if json_out {
        print_json_compact(&verdict)?;
    } else {
        print_json(&verdict)?;
    }
    Ok(())
}

/// Resolve `<root>/.claude/flows/<slug>/context.toml` honouring `TOMLCTL_ROOT`
/// (via `repo_or_cwd_root`) — the same anchor every other read path uses.
fn resolve_context_path(slug: &str) -> Result<PathBuf> {
    let root = repo_or_cwd_root()?;
    Ok(root.join(".claude").join("flows").join(slug).join("context.toml"))
}

/// Compute the staleness verdict given a parsed `context.toml` document.
/// Pure function — separated from `dispatch` so the threshold/age arithmetic
/// is unit-testable without filesystem setup.
fn compute_verdict(
    doc: &TomlValue,
    threshold_label: &str,
    threshold_dur: Duration,
) -> Result<JsonValue> {
    let updated = doc.get("updated");
    let updated_dt = match updated {
        Some(TomlValue::Datetime(dt)) => dt,
        Some(TomlValue::String(s)) => {
            // Be tolerant of a string-shaped `updated` that still parses as
            // ISO-8601 — TOML schemas in this repo always emit a native date,
            // but a hand-edited file might end up with a quoted string.
            match s.parse::<toml::value::Datetime>() {
                Ok(_) => {
                    // Re-derive a string we can normalise downstream by
                    // parsing the JSON-side path.
                    return verdict_from_iso_string(s, threshold_label, threshold_dur);
                }
                Err(_) => {
                    return Ok(json!({
                        "stale": true,
                        "last_activity": JsonValue::Null,
                        "age_seconds": JsonValue::Null,
                        "reason": "updated field missing",
                    }));
                }
            }
        }
        _ => {
            return Ok(json!({
                "stale": true,
                "last_activity": JsonValue::Null,
                "age_seconds": JsonValue::Null,
                "reason": "updated field missing",
            }));
        }
    };

    let dt_str = updated_dt.to_string();
    verdict_from_iso_string(&dt_str, threshold_label, threshold_dur)
}

/// Build a verdict JSON object given the `updated` value rendered as an
/// ISO-8601 string (date or datetime). Synthesises `last_activity` as
/// `<date>T00:00:00Z` for bare-date inputs, or echoes the datetime form back
/// if the input already carries a time component.
fn verdict_from_iso_string(
    iso: &str,
    threshold_label: &str,
    threshold_dur: Duration,
) -> Result<JsonValue> {
    // R6 / R39: parse + today-resolution route through `crate::time`,
    // sharing the injection seam with `flow::resolve::compute_staleness`.
    let updated_date = parse_iso_to_date(iso).map_err(|_| {
        anyhow::anyhow!("parsing `updated` as a date or timestamp: {iso}")
    })?;
    let today = today_utc_date()?;

    // Date-only granularity: a flow updated earlier today is age zero.
    let age_days = crate::time::age_days(updated_date, today);
    let age_seconds: u64 = age_days.saturating_mul(86_400);

    let stale = Duration::from_secs(age_seconds) > threshold_dur;
    let last_activity = if iso.len() == 10 {
        format!("{iso}T00:00:00Z")
    } else {
        iso.to_string()
    };
    let reason = if stale {
        format!("updated > {threshold_label} ago")
    } else {
        "updated within threshold".to_string()
    };

    Ok(json!({
        "stale": stale,
        "last_activity": last_activity,
        "age_seconds": age_seconds,
        "reason": reason,
    }))
}
