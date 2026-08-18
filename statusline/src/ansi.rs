//! ANSI escapes, in one file so a palette change stays a single-file edit.
//!
//! It is **two vocabularies, and they must not mix**:
//!
//! * the modern set, at this module's top level, used by `min` and `agents`.
//!   Spans close with SGR 22 / 39 / 49 ([`NOBOLD`], [`FG`], [`BG`]) and never
//!   with SGR 0. Claude Code wraps unselected agent rows in its own `\x1b[2m`,
//!   and a single SGR 0 in a row body cancels that wrapper for the remainder of
//!   the line.
//! * the legacy set, quarantined in [`legacy`], used by `full` alone. It closes
//!   spans with SGR 0 because the ps1 it ports did.
//!
//! The colour constants are shared; only the *terminators* differ. `legacy` is
//! a module rather than another `pub const` here so that `full`'s
//! `use crate::ansi::*;` cannot pull SGR 0 into scope for anyone else — reaching
//! for it now costs an explicit `ansi::legacy` in the import, which is the point.

pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const CYAN: &str = "\x1b[36m";
pub const WHITE: &str = "\x1b[37m";
// The ps1 defined a distinct $orange that was also ANSI 33; the alias is kept
// so the `full` style's tier tables read the same as the script they mirror.
pub const ORANGE: &str = "\x1b[33m";
pub const DIM: &str = "\x1b[90m";
pub const BOLD: &str = "\x1b[1m";
/// End a bold span (SGR 22) without the collateral of a full reset. `min` emits
/// no colour at all, so a `legacy::RESET` would be the only escape in the line
/// capable of clobbering state the terminal set for itself.
pub const NOBOLD: &str = "\x1b[22m";

/// The legacy `full`-only vocabulary. Everything in here emits SGR 0.
///
/// **Do not import this from `render::min` or `render::agents`.** Claude Code
/// dims unselected agent rows with its own `\x1b[2m`; an SGR 0 anywhere in a row
/// body cancels that for the rest of the line, so the modern styles close their
/// spans with SGR 22 / 39 / 49 instead ([`super::NOBOLD`], [`super::FG`],
/// [`super::BG`]). `full` is exempt only because it is a bug-for-bug port of the
/// ps1, which reset after every span.
pub mod legacy {
    /// Reset all attributes (SGR 0).
    pub const RESET: &str = "\x1b[0m";
}

/// Default foreground / default background (SGR 39 / 49). Used to close a span
/// without a full reset, which would cancel Claude Code's own dim wrapper on
/// unselected agent rows for the remainder of the line.
pub const FG: &str = "\x1b[39m";
pub const BG: &str = "\x1b[49m";
pub const BLACK_FG: &str = "\x1b[30m";

/// Background code for a teammate's assigned colour. Claude Code picks from
/// exactly these eight. ANSI indices rather than truecolor on purpose: the badge
/// then draws from the terminal's own scheme instead of a palette baked in here.
/// `orange` and `pink` have no ANSI slot of their own, so they take the bright
/// variants of their nearest neighbours.
pub fn badge_bg(color: &str) -> Option<&'static str> {
    Some(match color {
        "red" => "\x1b[41m",
        "green" => "\x1b[42m",
        "yellow" => "\x1b[43m",
        "blue" => "\x1b[44m",
        "purple" => "\x1b[45m",
        "cyan" => "\x1b[46m",
        "orange" => "\x1b[103m",
        "pink" => "\x1b[105m",
        _ => return None,
    })
}

pub const DOT_FILL: char = '\u{25CF}'; // ●
pub const DOT_EMPTY: char = '\u{25CB}'; // ○
