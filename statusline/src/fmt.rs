//! Pure formatting helpers shared by every style.

use std::path::Path;

use serde_json::Value;

pub fn format_tokens(num: i64) -> String {
    // 999_500 rather than 1_000_000: anything above it rounds to "1000k", a
    // fifth character the width budgets do not allow for. Promote it instead.
    if num >= 999_500 {
        format!("{:.1}m", num as f64 / 1_000_000.0)
    } else if num >= 1000 {
        format!("{:.0}k", num as f64 / 1000.0)
    } else {
        num.to_string()
    }
}

pub fn format_duration(ms: i64) -> String {
    if ms <= 0 {
        return "0s".into();
    }
    let s = ms / 1000;
    if s < 60 {
        return format!("{s}s");
    }
    if s < 3600 {
        return format!("{}m{}s", s / 60, s % 60);
    }
    format!("{}h{}m", s / 3600, (s % 3600) / 60)
}

pub enum EpochStyle {
    Time,     // "2:30pm", ":00" stripped → "2pm"
    DateTime, // "15/7, 2:30pm"
}

pub fn format_epoch(val: Option<&Value>, style: EpochStyle) -> Option<String> {
    let secs = match val? {
        Value::Number(n) => n.as_i64()?,
        Value::String(s) => s.parse::<i64>().ok()?,
        _ => return None,
    };
    if secs == 0 {
        return None;
    }
    let dt = chrono::DateTime::from_timestamp(secs, 0)?.with_timezone(&chrono::Local);
    let s = match style {
        EpochStyle::Time => dt.format("%-I:%M%P").to_string(),
        EpochStyle::DateTime => dt.format("%-d/%-m, %-I:%M%P").to_string(),
    };
    Some(s.replace(":00", ""))
}

/// "Claude Fable 5 (extra)" → "Fable 5": strip the leading "Claude " and a
/// trailing parenthesised qualifier, mirroring the ps1 regexes.
pub fn clean_model_name(name: &str) -> String {
    let name = name.strip_prefix("Claude ").unwrap_or(name).trim_end();
    if name.ends_with(')')
        && let Some(i) = name.rfind('(')
    {
        return name[..i].trim_end().to_string();
    }
    name.to_string()
}

pub fn path_leaf(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string())
}

/// Flatten a free-text field to a single row-safe line: newlines, tabs and any
/// other control byte become spaces, runs of whitespace collapse, and the
/// invisible bidi/zero-width formatting characters are dropped. Subagent labels
/// come from live progress summaries and shell command lines, either of which
/// can carry a newline that would otherwise blow the row apart.
pub fn one_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        // `is_control` is category Cc only, so the Cf formatting characters
        // survive it: the zero-width family (U+200B–U+200F), the embedding and
        // override family (U+202A–U+202E) and the isolate family
        // (U+2066–U+2069). An unterminated override reorders every segment
        // after it, and each character costs a `width` cell while occupying
        // none. Dropped outright rather than spaced — they are invisible, so a
        // space would open a gap the label never had.
        if matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            continue;
        }
        if c.is_whitespace() || c.is_control() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
}

/// The segment separator every budgeted renderer joins with. Shared rather than
/// per-renderer so the `min` line and the agent rows cannot drift apart on a
/// glyph they are meant to have in common.
pub const SEP: &str = " \u{00b7} "; // " · "

/// Character count — the width budget for rows we build ourselves, which are
/// ASCII plus a handful of narrow BMP symbols. Deliberately not a full
/// east-asian-width implementation: the consumer truncates as a backstop, so an
/// over-estimate costs a character and an under-estimate costs nothing.
pub fn width(s: &str) -> usize {
    s.chars().count()
}

/// Clamp to `max` characters, marking the cut with `…`. Returns an empty string
/// when there is not even room for the marker.
pub fn ellipsize(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    // Do not leave the marker dangling off a space.
    while out.ends_with(' ') {
        out.pop();
    }
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_format_tiers() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_000), "1k");
        assert_eq!(format_tokens(85_000), "85k");
        assert_eq!(format_tokens(200_000), "200k");
        assert_eq!(format_tokens(1_234_567), "1.2m");
        assert_eq!(format_tokens(2_960_000), "3.0m");
    }

    #[test]
    fn tokens_promote_at_the_thousands_ceiling() {
        assert_eq!(format_tokens(999_499), "999k");
        assert_eq!(format_tokens(999_500), "1.0m");
        assert_eq!(format_tokens(999_999), "1.0m");
        // No input may render the five-character "1000k".
        for n in [999_499, 999_500, 999_999, 1_000_000] {
            assert!(format_tokens(n).len() <= 4, "{n} rendered too wide");
        }
    }

    #[test]
    fn duration_format_tiers() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(-5), "0s");
        assert_eq!(format_duration(59_000), "59s");
        assert_eq!(format_duration(60_000), "1m0s");
        assert_eq!(format_duration(754_321), "12m34s");
        assert_eq!(format_duration(3_600_000), "1h0m");
        assert_eq!(format_duration(5_430_000), "1h30m");
    }

    #[test]
    fn model_name_cleaning() {
        assert_eq!(clean_model_name("Claude Fable 5"), "Fable 5");
        assert_eq!(clean_model_name("Claude Sonnet 4.5 (thinking)"), "Sonnet 4.5");
        assert_eq!(clean_model_name("Fable 5"), "Fable 5");
        assert_eq!(clean_model_name("Claude "), "");
    }

    #[test]
    fn epoch_rejects_null_forms() {
        assert!(format_epoch(None, EpochStyle::Time).is_none());
        assert!(format_epoch(Some(&Value::Null), EpochStyle::Time).is_none());
        assert!(format_epoch(Some(&Value::String("null".into())), EpochStyle::Time).is_none());
        assert!(format_epoch(Some(&serde_json::json!(0)), EpochStyle::Time).is_none());
        assert!(format_epoch(Some(&serde_json::json!(1752571800)), EpochStyle::Time).is_some());
    }

    #[test]
    fn epoch_renders_lowercase_meridiem_and_drops_the_zero_minute() {
        use chrono::TimeZone;
        // Constructed and rendered in the same local zone, so the offset
        // cancels and the expected strings hold on any machine. 2025-07-15 is a
        // Tuesday and none of these hours sit in a DST transition.
        let at = |h, min| {
            serde_json::json!(
                chrono::Local
                    .with_ymd_and_hms(2025, 7, 15, h, min, 0)
                    .unwrap()
                    .timestamp()
            )
        };
        // Lowercase am/pm: %P, not %p.
        assert_eq!(format_epoch(Some(&at(14, 30)), EpochStyle::Time).unwrap(), "2:30pm");
        assert_eq!(format_epoch(Some(&at(9, 5)), EpochStyle::Time).unwrap(), "9:05am");
        // A whole hour loses the minutes entirely.
        assert_eq!(format_epoch(Some(&at(14, 0)), EpochStyle::Time).unwrap(), "2pm");
        assert_eq!(
            format_epoch(Some(&at(14, 30)), EpochStyle::DateTime).unwrap(),
            "15/7, 2:30pm"
        );
        assert_eq!(
            format_epoch(Some(&at(12, 0)), EpochStyle::DateTime).unwrap(),
            "15/7, 12pm"
        );
    }

    #[test]
    fn one_line_flattens_and_collapses() {
        assert_eq!(one_line("read  src/main.rs\nthen  edit"), "read src/main.rs then edit");
        assert_eq!(one_line("  padded \t "), "padded");
        assert_eq!(one_line(""), "");
        // Payload-supplied ANSI must not survive into a row we width-budget.
        assert_eq!(one_line("a\u{1b}[31mb"), "a [31mb");
    }

    #[test]
    fn one_line_drops_invisible_bidi_and_zero_width_characters() {
        // U+202E (RLO) would reorder every segment after it in the rendered
        // row; U+200B is invisible but still costs a `width` cell.
        let out = one_line("agent\u{202e}gnitset\u{200b} label");
        assert_eq!(out, "agentgnitset label");
        assert_eq!(width(&out), 18);
        // Dropped, not spaced: no gap the label never had.
        assert_eq!(one_line("ab\u{200b}cd"), "abcd");
        // The isolate and embedding families go the same way.
        assert_eq!(one_line("a\u{2066}b\u{2069}c\u{202a}d\u{200d}e"), "abcde");
    }

    #[test]
    fn ellipsize_marks_the_cut() {
        assert_eq!(ellipsize("research-deep", 20), "research-deep");
        assert_eq!(ellipsize("research-deep", 13), "research-deep");
        assert_eq!(ellipsize("research-deep", 8), "researc\u{2026}");
        assert_eq!(ellipsize("a b c d", 4), "a b\u{2026}");
        assert_eq!(ellipsize("abc", 1), "\u{2026}");
        assert_eq!(ellipsize("abc", 0), "");
    }
}
