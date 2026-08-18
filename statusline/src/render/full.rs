//! `full` — the original two-line colour-coded status line, a faithful port of
//! `~/.claude/statusline.ps1`. Kept as the default style so the binary is
//! backwards compatible with an unflagged `statusLine.command`.
//!
//!   Line 1: CWD@Branch (changes) | model | effort | tokens (pct)
//!   Line 2: 5h dots pct @reset | 7d dots pct @reset | agent-busy time
//!
//! **This style deliberately does not measure the width of what it emits.** Its
//! `cols` thresholds are inherited verbatim from the ps1: they decide which
//! *segments* appear, and nothing then checks that the assembled line fits
//! `cols` — there is no `fmt::width` call anywhere in this file. A long branch
//! beside a long directory leaf and a wide model label can therefore overhang at
//! the narrow end of a tier, and Claude Code's own truncation is the only
//! backstop. That is the bug-for-bug fidelity mandate, not an oversight: `min`
//! and `agents` are the styles that budget their output against `cols`, and
//! `full` is the legacy one. Adding a width guard here would change rendered
//! output for existing users.
//!
//! Every value this style draws comes in as an argument: the git facts are
//! resolved by `main` and threaded through `render`, so this module is a pure
//! `payload -> String` with no disk read and no subprocess on the render path.

use crate::ansi::*;
// The legacy SGR-0 vocabulary, deliberately not part of `ansi`'s top level and
// so not reachable through the glob above — see `ansi::legacy`. `full` is the
// one style allowed to emit a full reset.
use crate::ansi::legacy::RESET;
use crate::fmt::{
    EpochStyle, clean_model_name, format_duration, format_epoch, format_tokens, path_leaf,
};
use crate::payload::{LimitWindow, Payload};

/// The width at or above which line 1 carries the `(+n -m)` changes group.
/// Public because `main` reads it to decide whether resolving the diff — the
/// crate's one subprocess spawn — is worth doing at all; keeping the threshold
/// in one place stops the guard and the renderer drifting apart.
pub const CHANGES_COLS: usize = 100;

fn usage_color(pct: i64) -> &'static str {
    match pct {
        90.. => RED,
        70.. => ORANGE,
        50.. => YELLOW,
        _ => GREEN,
    }
}

fn usage_dots(pct: i64) -> String {
    let filled = ((pct as f64 / 20.0).ceil() as i64).clamp(0, 5);
    const COLORS: [&str; 5] = [GREEN, GREEN, YELLOW, ORANGE, RED];
    let mut s = String::new();
    for (i, color) in COLORS.iter().enumerate() {
        if (i as i64) < filled {
            s.push_str(color);
            s.push(DOT_FILL);
        } else {
            s.push_str(DIM);
            s.push(DOT_EMPTY);
        }
        s.push_str(RESET);
    }
    s
}

fn limit_segment(
    label: &str,
    w: Option<&LimitWindow>,
    style: EpochStyle,
    show_resets: bool,
) -> String {
    // DELIBERATE DIVERGENCE FROM `min`, do not "align" the two. `rate_limits`
    // is subscriber-only and absent until the first API response, so an absent
    // window is not the same fact as an untouched one — `min` draws that
    // distinction and omits the bucket entirely. `full` collapses both to 0%
    // with an empty five-dot gauge because that is what the ps1 did, and this
    // style's mandate is bug-for-bug fidelity to it. Pinned by
    // `an_absent_window_renders_exactly_like_a_zeroed_one`.
    let pct = w.and_then(|w| w.used_percentage).unwrap_or(0.0).round() as i64;
    let mut seg = format!(
        "{WHITE}{label}{RESET} {} {}{pct}%{RESET}",
        usage_dots(pct),
        usage_color(pct)
    );
    if show_resets
        && let Some(r) = format_epoch(w.and_then(|w| w.resets_at.as_ref()), style)
    {
        seg.push_str(&format!(" {DIM}@{r}{RESET}"));
    }
    seg
}

/// Render the two-line status line.
///
/// `branch` and `diff` are the git facts, resolved by `main` and passed in
/// rather than looked up here — that keeps this a pure `payload -> String` and
/// confines the crate's disk reads and its one subprocess spawn to `main`, the
/// same shape `main::subagent_line` uses for `load_team` and `agents::render`
/// uses for `now_ms`. `diff` is expected to be `None` below [`CHANGES_COLS`],
/// where the changes group is not drawn at all.
pub fn render(
    data: &Payload,
    cols: usize,
    branch: Option<&str>,
    diff: Option<(u64, u64)>,
) -> String {
    let cw = data.context_window.as_ref();
    // Default only when the payload omits it — clamping a smaller reported
    // window up to 200k would disagree with the `pct_used` beside it.
    let size = cw.and_then(|c| c.context_window_size).unwrap_or(200_000);
    let pct_used = cw.and_then(|c| c.used_percentage).unwrap_or(0.0).round() as i64;
    let current = cw.map(|c| c.tokens()).unwrap_or(0);

    // Tokens — force red when past 200k regardless of proportional %. Computed
    // ahead of the layout split because both layouts colour a percentage with it.
    let token_color = if data.exceeds_200k_tokens == Some(true) {
        RED
    } else {
        usage_color(pct_used)
    };

    // `dir()` prefers `workspace.current_dir`, where `min` calls `repo_label()`
    // and prefers the `origin` remote. DELIBERATE, do not reconcile them: `full`
    // answers "where am I" — it pairs the leaf with the branch, so it has to
    // follow a mid-session `cd` — while `min` answers "which project", which
    // must stay stable across one. After a `cd` into a subdirectory the same
    // payload therefore reads `statusline@main` here and `dev-tools` in `min`.
    let cwd = data.dir();

    // Below 50 columns the ps1 drew an entirely different single-line layout.
    // Return it here rather than building line 1 and line 2 first and throwing
    // them away: interleaving the two layouts is how a change to token
    // colouring or percentage rounding gets made in one and not the other.
    if cols < 50 {
        return compact_line(data, cwd, token_color, pct_used);
    }

    // Width tiers, inherited from the ps1 (see the module header on why nothing
    // measures the result). Line 2's own `cols >= 50` tier is implied by having
    // reached this point.
    let show_changes = cols >= CHANGES_COLS;
    let show_model = cols >= 70;
    let show_resets = cols >= 70;

    let sep = format!(" {DIM}|{RESET} ");
    let mut parts1: Vec<String> = Vec::new();

    // Project@branch (changes)
    if let Some(cwd) = cwd {
        let mut seg = format!("{CYAN}{}{RESET}", path_leaf(cwd));
        if let Some(branch) = branch {
            seg.push_str(&format!("{DIM}@{RESET}{GREEN}{branch}{RESET}"));
            if show_changes
                && let Some((added, deleted)) = diff
            {
                seg.push_str(&format!(
                    " {DIM}({RESET}{GREEN}+{added}{RESET} {RED}-{deleted}{RESET}{DIM}){RESET}"
                ));
            }
        }
        parts1.push(seg);
    }

    // Model
    let model_name = data
        .model
        .as_ref()
        .and_then(|m| m.display_name.as_deref())
        .map(clean_model_name)
        .unwrap_or_default();
    if !model_name.is_empty() && show_model {
        parts1.push(format!("{DIM}{model_name}{RESET}"));
    }

    // Effort level — absent when the current model lacks the effort parameter.
    if show_model
        && let Some(level) = data
            .effort
            .as_ref()
            .and_then(|e| e.level.as_deref())
            .filter(|l| !l.is_empty())
    {
        let color = match level {
            "low" => GREEN,
            "medium" => YELLOW,
            "high" => ORANGE,
            "xhigh" => CYAN,
            "max" => RED,
            _ => WHITE,
        };
        parts1.push(format!("{color}{level}{RESET}"));
    }

    parts1.push(format!(
        "{}/{} {DIM}({RESET}{token_color}{pct_used}%{RESET}{DIM}){RESET}",
        format_tokens(current),
        format_tokens(size)
    ));

    let line1 = parts1.join(&sep);

    // Line 2: rate limits
    let mut line2 = String::new();
    if let Some(rl) = data.rate_limits.as_ref() {
        line2 = [
            limit_segment("5h", rl.five_hour.as_ref(), EpochStyle::Time, show_resets),
            limit_segment("7d", rl.seven_day.as_ref(), EpochStyle::DateTime, show_resets),
        ]
        .join(&sep);
    }

    // Agent-busy time (API wait). Appended to line 2, or shown standalone.
    let api_ms = data
        .cost
        .as_ref()
        .and_then(|c| c.total_api_duration_ms)
        .unwrap_or(0.0) as i64;
    if api_ms > 0 {
        let busy = format!("{DIM}busy{RESET} {WHITE}{}{RESET}", format_duration(api_ms));
        line2 = if line2.is_empty() {
            busy
        } else {
            format!("{line2}{sep}{busy}")
        };
    }

    if line2.is_empty() {
        line1
    } else {
        format!("{line1}\n{line2}")
    }
}

/// The below-50-columns layout: one line, no branch, no model, no dot gauges.
/// A separate layout rather than a shed-down of the wide one — the ps1 wrote it
/// that way and the segments do not correspond.
fn compact_line(
    data: &Payload,
    cwd: Option<&str>,
    token_color: &str,
    pct_used: i64,
) -> String {
    let mut cline = String::new();
    if let Some(cwd) = cwd {
        cline = format!("{CYAN}{}{RESET}", path_leaf(cwd));
    }
    cline.push_str(&format!(" {token_color}{pct_used}%{RESET}"));
    if let Some(rl) = &data.rate_limits {
        let p5 = rl
            .five_hour
            .as_ref()
            .and_then(|w| w.used_percentage)
            .unwrap_or(0.0)
            .round() as i64;
        let p7 = rl
            .seven_day
            .as_ref()
            .and_then(|w| w.used_percentage)
            .unwrap_or(0.0)
            .round() as i64;
        cline.push_str(&format!(
            " {DIM}|{RESET} {}5h:{p5}%{RESET} {}7d:{p7}%{RESET}",
            usage_color(p5),
            usage_color(p7)
        ));
    }
    cline
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::plain;

    fn payload(json: &str) -> Payload {
        serde_json::from_str(json).expect("payload parses")
    }

    /// Everything the wide layout can draw, so a tier test reads as "which of
    /// these appeared". The reset stamps are real epochs but their rendering is
    /// timezone-dependent, so assertions count `@` rather than matching a clock.
    const WIDE: &str = r#"{
        "workspace": {"current_dir": "/x/dev-tools/statusline"},
        "model": {"display_name": "Claude Opus 5"},
        "effort": {"level": "xhigh"},
        "context_window": {"total_input_tokens": 31000, "context_window_size": 200000,
                           "used_percentage": 15},
        "rate_limits": {"five_hour": {"used_percentage": 26, "resets_at": 1750000000},
                        "seven_day": {"used_percentage": 7, "resets_at": 1750500000}}
    }"#;

    /// `WIDE` at a given width, on a branch, with the ANSI stripped. The git
    /// facts are arguments, so no test here touches a repo.
    fn wide(cols: usize, diff: Option<(u64, u64)>) -> String {
        plain(&render(&payload(WIDE), cols, Some("main"), diff))
    }

    const FIVE_EMPTY: &str = "\u{25CB}\u{25CB}\u{25CB}\u{25CB}\u{25CB}";

    #[test]
    fn line_two_appears_at_fifty_columns() {
        let narrow = wide(49, None);
        assert!(!narrow.contains('\n'), "one line below the tier: {narrow:?}");

        let two = wide(50, None);
        let (l1, l2) = two.split_once('\n').expect("two lines at 50 columns");
        assert_eq!(l1, "statusline@main | 31k/200k (15%)");
        assert_eq!(l2, "5h \u{25CF}\u{25CF}\u{25CB}\u{25CB}\u{25CB} 26% | 7d \u{25CF}\u{25CB}\u{25CB}\u{25CB}\u{25CB} 7%");
    }

    #[test]
    fn the_model_effort_and_reset_stamps_appear_at_seventy_columns() {
        let below = wide(69, None);
        assert!(!below.contains("Opus 5"), "{below:?}");
        assert!(!below.contains("xhigh"), "{below:?}");
        // The one `@` below the tier is the cwd@branch separator — no stamps.
        assert_eq!(below.matches('@').count(), 1, "{below:?}");

        let at = wide(70, None);
        assert!(at.contains("Opus 5") && at.contains("xhigh"), "{at:?}");
        assert!(
            at.starts_with("statusline@main | Opus 5 | xhigh | 31k/200k (15%)"),
            "{at:?}"
        );
        // cwd@branch, plus one reset stamp per rate-limit window.
        assert_eq!(at.matches('@').count(), 3, "{at:?}");
    }

    #[test]
    fn the_changes_group_appears_at_a_hundred_columns() {
        // `main` gates the `git diff` spawn on this same constant, so a change
        // here without a change there starts spawning git at every width.
        assert_eq!(CHANGES_COLS, 100);

        let below = wide(99, Some((3, 4)));
        assert!(!below.contains("+3"), "no changes group below the tier: {below:?}");

        let at = wide(100, Some((3, 4)));
        assert!(at.starts_with("statusline@main (+3 -4) |"), "{at:?}");
    }

    // The changes group hangs off the branch group, which is why `main` only
    // resolves the diff once the branch resolved.
    #[test]
    fn without_a_branch_there_is_no_separator_and_no_changes_group() {
        let out = plain(&render(&payload(WIDE), 120, None, Some((3, 4))));
        let l1 = out.lines().next().expect("line 1");
        assert!(l1.starts_with("statusline | Opus 5"), "{l1:?}");
        assert!(!l1.contains("+3"), "{l1:?}");
    }

    #[test]
    fn the_compact_layout_is_its_own_line_not_a_shed_down_wide_one() {
        // Branch and diff are supplied and still dropped; so are the model, the
        // effort, the token counts and the dot gauges.
        let out = plain(&render(&payload(WIDE), 40, Some("main"), Some((3, 4))));
        assert_eq!(out, "statusline 15% | 5h:26% 7d:7%");
    }

    // Pins that both layouts colour the percentage from the same computation —
    // the drift the compact/wide split exists to prevent.
    #[test]
    fn exceeding_200k_forces_the_token_colour_red_in_both_layouts() {
        let p = payload(
            r#"{"workspace":{"current_dir":"/x/proj"},
                "context_window":{"total_input_tokens":1000,"used_percentage":3},
                "exceeds_200k_tokens":true}"#,
        );
        // 3% is green on the proportional scale; the flag has to override it.
        assert_eq!(usage_color(3), GREEN);
        for cols in [120, 40] {
            let out = render(&p, cols, None, None);
            assert!(out.contains(&format!("{RED}3%")), "cols {cols}: {out:?}");
        }
    }

    #[test]
    fn the_busy_segment_stands_alone_when_no_rate_limits_are_reported() {
        let p = payload(
            r#"{"workspace":{"current_dir":"/x/proj"},
                "context_window":{"total_input_tokens":1000},
                "cost":{"total_api_duration_ms":65000}}"#,
        );
        let out = plain(&render(&p, 120, None, None));
        let (l1, l2) = out.split_once('\n').expect("two lines");
        assert_eq!(l1, "proj | 1k/200k (0%)");
        assert_eq!(l2, "busy 1m5s");
    }

    /// R17, pinned so it cannot be quietly "fixed": `full` collapses "no window
    /// reported" and "window sitting at 0%" onto the same 0% and empty gauge,
    /// where `min` omits an absent bucket entirely. Deliberate ps1 fidelity.
    #[test]
    fn an_absent_window_renders_exactly_like_a_zeroed_one() {
        let zero = LimitWindow {
            used_percentage: Some(0.0),
            ..Default::default()
        };
        let absent = limit_segment("5h", None, EpochStyle::Time, false);
        assert_eq!(
            absent,
            limit_segment("5h", Some(&zero), EpochStyle::Time, false)
        );
        assert_eq!(plain(&absent), format!("5h {FIVE_EMPTY} 0%"));
    }

    #[test]
    fn a_limit_segment_rounds_gauges_and_optionally_stamps() {
        let w = LimitWindow {
            used_percentage: Some(29.6),
            resets_at: Some(serde_json::Value::from(1_750_000_000i64)),
        };
        let bare = plain(&limit_segment("5h", Some(&w), EpochStyle::Time, false));
        assert_eq!(bare, "5h \u{25CF}\u{25CF}\u{25CB}\u{25CB}\u{25CB} 30%");
        assert!(!bare.contains('@'), "no stamp when resets are off: {bare:?}");

        let stamped = plain(&limit_segment("5h", Some(&w), EpochStyle::Time, true));
        assert!(stamped.starts_with(&format!("{bare} @")), "{stamped:?}");
    }

    // A window with no `resets_at` draws no stamp even inside the tier that
    // shows them — the segment must not end in a dangling "@".
    #[test]
    fn a_window_without_a_reset_time_draws_no_stamp() {
        let w = LimitWindow {
            used_percentage: Some(50.0),
            ..Default::default()
        };
        let seg = plain(&limit_segment("7d", Some(&w), EpochStyle::DateTime, true));
        assert_eq!(seg, "7d \u{25CF}\u{25CF}\u{25CF}\u{25CB}\u{25CB} 50%");
    }

    #[test]
    fn dots_fill_counts() {
        let filled = |pct| usage_dots(pct).matches(DOT_FILL).count();
        assert_eq!(filled(0), 0);
        assert_eq!(filled(1), 1);
        assert_eq!(filled(63), 4);
        assert_eq!(filled(100), 5);
    }

    #[test]
    fn usage_color_boundaries() {
        assert_eq!(usage_color(49), GREEN);
        assert_eq!(usage_color(50), YELLOW);
        assert_eq!(usage_color(70), ORANGE);
        assert_eq!(usage_color(90), RED);
    }
}
