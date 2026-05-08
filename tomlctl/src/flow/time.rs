//! R6/R39: consolidated time helpers for flow leaf modules.
//!
//! Five pre-consolidation sites (`flow::active::now_rfc3339`,
//! `flow::init::now_rfc3339` / `today_toml_date`, `flow::stale`'s
//! `Timestamp::now()` calls, `flow::resolve::compute_staleness`,
//! `flow::ensure_artifact::today_utc_iso`) each constructed `jiff::Timestamp::now()`
//! independently. R6 collapses them to a single module; R39 adds a
//! test-only injection seam (`set_now_for_test`) so unit tests can pin a
//! deterministic clock without touching every call site.
//!
//! Production callers see no behavioural change: `now()` resolves to
//! `jiff::Timestamp::now()` exactly as the per-site helpers did.
//!
//! ## Test injection
//!
//! Tests in this crate can call `set_now_for_test(Some(ts))` to pin the
//! returned `Timestamp` for the duration of the current thread. The seam
//! is a thread-local `Cell<Option<Timestamp>>` so two parallel tests
//! cannot accidentally observe each other's override. Reset with
//! `set_now_for_test(None)` (or rely on `_FixedNowGuard`'s `Drop` impl).

use anyhow::{Context, Result};
use jiff::Timestamp;
use jiff::civil::Date;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static FIXED_NOW: Cell<Option<Timestamp>> = const { Cell::new(None) };
}

/// Resolve the current instant. In production this is
/// `jiff::Timestamp::now()`; under `cfg(test)` the result honours any
/// thread-local override set via `set_now_for_test`.
pub(crate) fn now() -> Timestamp {
    #[cfg(test)]
    {
        if let Some(ts) = FIXED_NOW.with(|c| c.get()) {
            return ts;
        }
    }
    Timestamp::now()
}

/// RFC3339-Z timestamp string (`"YYYY-MM-DDTHH:MM:SSZ"` or fractional).
/// Mirrors the previous-per-site `jiff::Timestamp::now().to_string()`
/// idiom — `Display` on `jiff::Timestamp` always emits UTC-Z.
pub(crate) fn now_rfc3339() -> String {
    now().to_string()
}

/// Today's UTC date as a `jiff::civil::Date`.
pub(crate) fn today_utc_date() -> Result<Date> {
    Ok(now()
        .in_tz("UTC")
        .context("resolving today's UTC date")?
        .date())
}

/// Today's UTC date rendered `YYYY-MM-DD`.
pub(crate) fn today_utc_iso() -> Result<String> {
    Ok(today_utc_date()?.to_string())
}

/// Today as a TOML bare-date `Datetime` literal — convenience for
/// `flow::init::build_seed_doc` which writes a typed date into
/// `context.toml`.
pub(crate) fn today_toml_date() -> Result<toml::value::Datetime> {
    let date_str = today_utc_iso()?;
    date_str.parse::<toml::value::Datetime>().with_context(|| {
        format!("converting today ({date_str}) to TOML date")
    })
}

/// Parse an ISO-8601-ish `updated` value (date-only `YYYY-MM-DD` or full
/// RFC3339 timestamp) into a `Date`. Used by `flow::stale` and
/// `flow::resolve::compute_staleness` to compute age-in-days against
/// `today_utc_date()`. A datetime input is interpreted in UTC and
/// truncated to its date component.
pub(crate) fn parse_iso_to_date(iso: &str) -> Result<Date, ParseDateError> {
    if iso.len() == 10 {
        iso.parse::<Date>().map_err(|_| ParseDateError::Invalid)
    } else {
        let z = iso
            .parse::<Timestamp>()
            .map_err(|_| ParseDateError::Invalid)?
            .in_tz("UTC")
            .map_err(|_| ParseDateError::Invalid)?;
        Ok(z.date())
    }
}

/// Parse-error sentinel for `parse_iso_to_date`. Deliberately opaque so
/// callers convert the failure to their own JSON-shaped error reason
/// (`stale.rs` and `resolve.rs` each emit slightly different wording —
/// the variant exists, the prose is the caller's business).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseDateError {
    Invalid,
}

#[cfg(test)]
pub(crate) fn set_now_for_test(ts: Option<Timestamp>) {
    FIXED_NOW.with(|c| c.set(ts));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard helper — a test sets `_g = FixedNowGuard::pin(ts)` and the
    /// override is cleared on drop. Wrapping the unsafe-feeling raw
    /// `set_now_for_test` calls keeps the failure mode bounded if a
    /// test panics mid-assertion.
    struct FixedNowGuard;
    impl FixedNowGuard {
        fn pin(ts: Timestamp) -> Self {
            set_now_for_test(Some(ts));
            FixedNowGuard
        }
    }
    impl Drop for FixedNowGuard {
        fn drop(&mut self) {
            set_now_for_test(None);
        }
    }

    #[test]
    fn parse_iso_to_date_accepts_bare_date() {
        let d = parse_iso_to_date("2026-05-08").expect("date parses");
        assert_eq!(d.to_string(), "2026-05-08");
    }

    #[test]
    fn parse_iso_to_date_accepts_rfc3339_timestamp() {
        let d = parse_iso_to_date("2026-05-08T14:32:00Z").expect("date parses");
        assert_eq!(d.to_string(), "2026-05-08");
    }

    #[test]
    fn parse_iso_to_date_rejects_garbage() {
        assert!(matches!(
            parse_iso_to_date("not-a-date"),
            Err(ParseDateError::Invalid)
        ));
    }

    #[test]
    fn fixed_now_pins_clock_for_test_thread() {
        let ts: Timestamp = "2026-05-08T00:00:00Z".parse().unwrap();
        let _g = FixedNowGuard::pin(ts);
        assert_eq!(now_rfc3339(), "2026-05-08T00:00:00Z");
        assert_eq!(today_utc_iso().unwrap(), "2026-05-08");
    }

    /// R39 (a): age_seconds-at-boundary — pin "today" to noon and verify
    /// age computation against an `updated` from earlier the same day
    /// resolves to 0 days (date-only granularity).
    #[test]
    fn boundary_same_day_yields_zero_age() {
        let now_ts: Timestamp = "2026-05-08T12:00:00Z".parse().unwrap();
        let _g = FixedNowGuard::pin(now_ts);
        let today = today_utc_date().unwrap();
        let updated = parse_iso_to_date("2026-05-08T00:00:00Z").unwrap();
        assert_eq!(today, updated);
    }

    /// R39 (b): UTC-midnight crossing — fixing "now" to one second before
    /// midnight yields the same date as fixing it to one second past
    /// midnight on the SAME calendar day. The transition tested here
    /// pins both sides of the boundary explicitly.
    #[test]
    fn utc_midnight_crossing_changes_date() {
        // Just before midnight on the 8th.
        let pre: Timestamp = "2026-05-08T23:59:59Z".parse().unwrap();
        let _g_pre = FixedNowGuard::pin(pre);
        assert_eq!(today_utc_iso().unwrap(), "2026-05-08");
        drop(_g_pre);
        // One second later — UTC date rolls over.
        let post: Timestamp = "2026-05-09T00:00:00Z".parse().unwrap();
        let _g_post = FixedNowGuard::pin(post);
        assert_eq!(today_utc_iso().unwrap(), "2026-05-09");
    }

    /// R39 (c): malformed input surfaces as `ParseDateError::Invalid`,
    /// not a panic.
    #[test]
    fn malformed_input_returns_error_not_panic() {
        assert!(parse_iso_to_date("").is_err());
        assert!(parse_iso_to_date("garbage").is_err());
        assert!(parse_iso_to_date("2026-13-01").is_err());
    }
}
