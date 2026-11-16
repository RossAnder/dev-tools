//! The short number formats every view shares: token counts and durations.

use crate::rollup::{self, Tokens};

/// `n` as `999`, `1.3k` or `2.3M`, rounded half up to one decimal without floats.
pub(crate) fn count(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 999_950 {
        let tenths = (n + 50) / 100;
        format!("{}.{}k", tenths / 10, tenths % 10)
    } else {
        let tenths = (n + 50_000) / 100_000;
        format!("{}.{}M", tenths / 10, tenths % 10)
    }
}

/// `seconds` in its two largest units: `42s`, `58m00s`, `3h05m`, `2d01h`.
pub(crate) fn duration(seconds: u64) -> String {
    let (d, h, m, s) = (
        seconds / 86_400,
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60,
    );
    if d > 0 {
        format!("{d}d{h:02}h")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// A rollup row's token total, marked `partial` when some counted agents have no usage.
pub(crate) fn row_tokens(row: &rollup::Row) -> String {
    let usage = &row.usage;
    let total = usage.input + usage.cache_write + usage.cache_read + usage.output;
    match row.tokens {
        Tokens::Recorded => count(total),
        Tokens::Partial => format!("{} partial", count(total)),
        Tokens::NotRecorded => "not recorded".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_rounds_to_one_decimal_in_k_and_m() {
        assert_eq!(count(999), "999");
        assert_eq!(count(1_250), "1.3k");
        assert_eq!(count(61_872), "61.9k");
        assert_eq!(count(999_949), "999.9k");
        assert_eq!(count(999_950), "1.0M");
        assert_eq!(count(2_340_000), "2.3M");
    }

    #[test]
    fn duration_keeps_the_two_largest_units() {
        assert_eq!(duration(42), "42s");
        assert_eq!(duration(3_480), "58m00s");
        assert_eq!(duration(3 * 3600 + 5 * 60), "3h05m");
        assert_eq!(duration(2 * 86_400 + 3600), "2d01h");
    }
}
