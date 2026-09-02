//! Clock and duration helpers.
//!
//! Crate-level infrastructure, owned by no verb group: `flow` and `backlog`
//! both resolve "today" and parse age thresholds, so neither may own the
//! helpers the other needs. Every wall-clock read in the crate goes through
//! `now()` rather than `jiff::Timestamp::now()`, so the test seam below
//! covers all of them.
//!
//! ## Test injection
//!
//! Tests in this crate can call `set_now_for_test(Some(ts))` to pin the
//! returned `Timestamp` for the duration of the current thread. The seam
//! is a thread-local `Cell<Option<Timestamp>>` so two parallel tests
//! cannot accidentally observe each other's override. Reset with
//! `set_now_for_test(None)` (or rely on `_FixedNowGuard`'s `Drop` impl).

use std::time::Duration;

use anyhow::{Context, Result};
use jiff::Timestamp;
use jiff::civil::Date;

use crate::errors::{ErrorKind, tagged_err};

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
    date_str
        .parse::<toml::value::Datetime>()
        .with_context(|| format!("converting today ({date_str}) to TOML date"))
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

/// Whole days from `from` to `today`, clamped at zero so a future date — or a
/// span jiff refuses to compute — reads as age zero rather than wrapping.
pub(crate) fn age_days(from: Date, today: Date) -> u64 {
    from.until(today)
        .map_or(0, |span| u64::try_from(span.get_days()).unwrap_or(0))
}

/// Minimal humantime-style threshold parser (no `humantime` dep). Accepts
/// `<n>{s|m|h|d|w}` and returns a `Duration`; `w` expands to `7d`. Rejects
/// bare numbers, unknown suffixes, empty inputs, and zero-length integer
/// prefixes — every error is tagged `kind=validation`, which `flow stale`
/// and `backlog compact` both surface verbatim.
pub(crate) fn parse_threshold(input: &str) -> Result<Duration> {
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

    /// Age is date-only: an `updated` from earlier the same calendar day
    /// resolves to zero days, never a fraction of one.
    #[test]
    fn boundary_same_day_yields_zero_age() {
        let now_ts: Timestamp = "2026-05-08T12:00:00Z".parse().unwrap();
        let _g = FixedNowGuard::pin(now_ts);
        let today = today_utc_date().unwrap();
        let updated = parse_iso_to_date("2026-05-08T00:00:00Z").unwrap();
        assert_eq!(today, updated);
    }

    /// The calendar date is derived in UTC, not local time: one second
    /// either side of UTC midnight lands on different dates regardless of
    /// the host timezone.
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

    /// Malformed input surfaces as `ParseDateError::Invalid`, not a panic.
    #[test]
    fn malformed_input_returns_error_not_panic() {
        assert!(parse_iso_to_date("").is_err());
        assert!(parse_iso_to_date("garbage").is_err());
        assert!(parse_iso_to_date("2026-13-01").is_err());
    }

    #[test]
    fn parse_threshold_accepts_known_suffixes() {
        assert_eq!(
            parse_threshold("7d").unwrap(),
            Duration::from_secs(7 * 86_400)
        );
        assert_eq!(
            parse_threshold("48h").unwrap(),
            Duration::from_secs(48 * 3600)
        );
        assert_eq!(
            parse_threshold("1w").unwrap(),
            Duration::from_secs(7 * 86_400)
        );
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
