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
//! Threshold parser is deliberately local (no `humantime` dep) and accepts
//! `<n>{s|m|h|d|w}` only; bare numbers are rejected per the plan's "require
//! explicit suffix" rule. `w` expands to `7d`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::cli::ReadIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::time::{parse_iso_to_date, today_utc_date};
use crate::io::{read_toml, repo_or_cwd_root};
use crate::output::{print_json, print_json_compact};

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
    // R6 / R39: parse + today-resolution route through `flow::time`,
    // sharing the injection seam with `flow::resolve::compute_staleness`.
    let updated_date = parse_iso_to_date(iso).map_err(|_| {
        anyhow::anyhow!("parsing `updated` as a date or timestamp: {iso}")
    })?;
    let today = today_utc_date()?;

    // Compute age in days. `Date::until` against another `Date` defaults to
    // a span in days. `updated_date.until(today)` yields a POSITIVE day count
    // when `updated < today` (the normal "old flow" case) and a NEGATIVE one
    // when `updated > today` (clock skew / hand-edited future date) — clamp
    // the negative path to zero so a future-dated flow reads as fresh, not
    // panickingly-stale.
    let span = updated_date
        .until(today)
        .context("computing date span between updated and today")?;
    let signed_days: i64 = span.get_days() as i64;
    let age_days: u64 = if signed_days > 0 { signed_days as u64 } else { 0 };
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

/// Minimal humantime-style threshold parser. Accepts `<n>{s|m|h|d|w}` and
/// returns a `Duration`. Rejects bare numbers, unknown suffixes, empty
/// inputs, and zero-length integer prefixes — every error is tagged
/// `kind=validation` per the plan's contract.
fn parse_threshold(input: &str) -> Result<Duration> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("invalid threshold: {input}"),
        ));
    }
    let last = trimmed.as_bytes()[trimmed.len() - 1];
    let (num_str, mult): (&str, u64) = match last {
        b's' => (&trimmed[..trimmed.len() - 1], 1),
        b'm' => (&trimmed[..trimmed.len() - 1], 60),
        b'h' => (&trimmed[..trimmed.len() - 1], 3600),
        b'd' => (&trimmed[..trimmed.len() - 1], 86_400),
        b'w' => (&trimmed[..trimmed.len() - 1], 7 * 86_400),
        _ => {
            // Bare number / unknown suffix — both rejected.
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("invalid threshold: {input}"),
            ));
        }
    };
    if num_str.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("invalid threshold: {input}"),
        ));
    }
    let n: u64 = num_str.parse().map_err(|_| {
        tagged_err(
            ErrorKind::Validation,
            None,
            format!("invalid threshold: {input}"),
        )
    })?;
    Ok(Duration::from_secs(n.saturating_mul(mult)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_threshold_accepts_known_suffixes() {
        assert_eq!(parse_threshold("7d").unwrap(), Duration::from_secs(7 * 86_400));
        assert_eq!(parse_threshold("48h").unwrap(), Duration::from_secs(48 * 3600));
        assert_eq!(parse_threshold("1w").unwrap(), Duration::from_secs(7 * 86_400));
        assert_eq!(parse_threshold("60m").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_threshold("300s").unwrap(), Duration::from_secs(300));
    }

    #[test]
    fn parse_threshold_rejects_bare_number() {
        let err = parse_threshold("5").unwrap_err();
        let tagged = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert!(matches!(tagged.kind, ErrorKind::Validation));
    }

    #[test]
    fn parse_threshold_rejects_unknown_suffix() {
        let err = parse_threshold("5x").unwrap_err();
        let tagged = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert!(matches!(tagged.kind, ErrorKind::Validation));
    }

    #[test]
    fn parse_threshold_rejects_empty() {
        assert!(parse_threshold("").is_err());
        assert!(parse_threshold("d").is_err()); // missing integer prefix
    }
}
