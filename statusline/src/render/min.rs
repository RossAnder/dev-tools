//! `min` — one monotone line, modelled on the `claude-usage` statusline
//! (<https://statuslin.es/c/claude-usage-cc6759bb>), with the repo name in
//! place of its "Usage" title and the live session context in place of its
//! per-model weekly bucket:
//!
//! ```text
//! ✻ dev-tools · hour 26% · week 7% · chat 31k · Opus 5 xhigh
//! ```
//!
//! Monotone in the strict sense: the line emits **no colour at all**, so every
//! character lands in the terminal theme's own foreground rather than a mix of
//! greys. Weight is uniform too — one bold span wraps the whole line, so no
//! segment is louder than its neighbours. That holds for the leading icon as
//! much as for the numbers: colouring it was tried and reverted.

use crate::ansi::{BOLD, NOBOLD};
use crate::fmt::{SEP, clean_model_name, ellipsize, format_tokens, width};
use crate::payload::Payload;

/// U+273B, not the reference page's U+2733. emoji-data.txt lists
/// `2733..2734 ; Emoji`, so terminals substitute a colour-emoji font for U+2733
/// and it lands off-weight and off-baseline beside a monospace face. U+273B
/// carries no emoji property, is the mark Claude Code itself uses, and sits at
/// exactly one cell in Iosevka. Override with `--icon`; `--icon ""` drops it.
pub const DEFAULT_ICON: &str = "\u{273B}"; // ✻

/// The width the line stops shrinking at — the icon, a readable repo name and
/// the live context number. Terminals below this are not a real target: an
/// overhanging line that still says which repo it belongs to beats a name
/// clipped down to noise, and Claude Code truncates as a backstop anyway.
const MIN_COLS: usize = 24;

struct Bucket {
    /// Shed identity, carried alongside the text rather than read back out of
    /// it: the model segment's text is whatever the payload names the model,
    /// so the degradation loop cannot key off what is drawn.
    key: &'static str,
    /// The segment exactly as it lands on the line, label included. One field
    /// and not a `label`/`value` pair because `model` has no fixed label —
    /// the model name *is* its leading word.
    text: String,
}

impl Bucket {
    fn width(&self) -> usize {
        width(&self.text)
    }
}

/// `Opus 5 xhigh` — the resolved model, and the live effort level when the
/// model takes the parameter (it reflects a mid-session `/effort`, and the
/// `effort` object is simply absent on models without one).
///
/// One segment rather than two: `full` can colour the effort word by level and
/// so earns it a separator of its own, but `min` emits no colour, and a bare
/// `xhigh` sitting between two dots reads as a fourth usage bucket.
fn model_spec(p: &Payload) -> Option<String> {
    let name = p
        .model
        .as_ref()
        .and_then(|m| m.display_name.as_deref())
        .map(clean_model_name)
        .filter(|n| !n.is_empty())?;
    match p
        .effort
        .as_ref()
        .and_then(|e| e.level.as_deref())
        .filter(|l| !l.is_empty())
    {
        Some(level) => Some(format!("{name} {level}")),
        None => Some(name),
    }
}

fn buckets(p: &Payload) -> Vec<Bucket> {
    let mut out = Vec::with_capacity(4);
    let rl = p.rate_limits.as_ref();
    for (key, window) in [
        ("hour", rl.and_then(|r| r.five_hour.as_ref())),
        ("week", rl.and_then(|r| r.seven_day.as_ref())),
    ] {
        // `rate_limits` is subscriber-only and absent until the first API
        // response; an absent window is silence, not a zero.
        if let Some(pct) = window.and_then(|w| w.used_percentage) {
            out.push(Bucket {
                key,
                text: format!("{key} {}%", pct.round() as i64),
            });
        }
    }

    if let Some(cw) = p.context_window.as_ref() {
        let tokens = cw.tokens();
        if tokens > 0 {
            out.push(Bucket {
                key: "chat",
                text: format!("chat {}", format_tokens(tokens)),
            });
        }
    }

    // Last on the line and first to shed: the usage numbers keep the positions
    // they have always had, and the segment that moves least often is the one
    // a narrow terminal gives up first.
    if let Some(spec) = model_spec(p) {
        out.push(Bucket {
            key: "model",
            text: spec,
        });
    }
    out
}

pub fn render(p: &Payload, cols: usize, icon: &str) -> String {
    let title = p.repo_label();
    let mut buckets = buckets(p);

    // Narrow-terminal degradation, slowest-moving segment first, so `chat` —
    // the only number that moves while you work — is last out. `model` leads:
    // it is the one segment you set rather than watch, and it is also the one
    // the payload can make arbitrarily wide. The title never goes: a status
    // line that cannot say which repo it belongs to is worse than a short one.
    for doomed in ["model", "week", "hour"] {
        if line_width(icon, &title, &buckets) <= cols {
            break;
        }
        buckets.retain(|b| b.key != doomed);
    }

    // With nothing sheddable left the title gives ground last — clipped, never
    // dropped, since a repo name is unbounded and would otherwise wrap the line
    // it was budgeted for. `line_width` with an empty title is everything the
    // name has to fit around; keep a column for the marker so the line still
    // names something even when it is only the marker.
    let budget = cols.max(MIN_COLS);
    let title = if line_width(icon, &title, &buckets) > budget {
        let others = line_width(icon, "", &buckets);
        ellipsize(&title, budget.saturating_sub(others).max(1))
    } else {
        title
    };

    paint(icon, &title, &buckets)
}

fn line_width(icon: &str, title: &str, buckets: &[Bucket]) -> usize {
    // icon + space + title, then " · " ahead of every bucket — the title is a
    // segment like the rest, so the separator run is uniform across the line.
    let head = if icon.is_empty() { 0 } else { width(icon) + 1 };
    head + width(title) + buckets.iter().map(|b| width(SEP) + b.width()).sum::<usize>()
}

fn paint(icon: &str, title: &str, buckets: &[Bucket]) -> String {
    let mut out = if icon.is_empty() {
        title.to_string()
    } else {
        format!("{icon} {title}")
    };
    for b in buckets {
        out.push_str(SEP);
        out.push_str(&b.text);
    }
    // One bold span around the lot: uniform weight, and the only escapes in the
    // line. `NOBOLD` (SGR 22) rather than a reset, so nothing else the terminal
    // has set for itself gets cleared on the way out.
    format!("{BOLD}{out}{NOBOLD}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::plain;

    fn payload(json: &str) -> Payload {
        serde_json::from_str(json).expect("payload parses")
    }

    const FULL: &str = r#"{
        "workspace": {"repo": {"name": "dev-tools"}},
        "context_window": {"total_input_tokens": 31000, "used_percentage": 15},
        "rate_limits": {
            "five_hour": {"used_percentage": 26.4},
            "seven_day": {"used_percentage": 7.1}
        }
    }"#;

    /// `FULL` plus the model and effort objects. Kept separate so every
    /// assertion above stays a test of the payload that omits them — the shape
    /// a session has before the first API response.
    const SPEC: &str = r#"{
        "workspace": {"repo": {"name": "dev-tools"}},
        "model": {"display_name": "Claude Opus 5"},
        "effort": {"level": "xhigh"},
        "context_window": {"total_input_tokens": 31000, "used_percentage": 15},
        "rate_limits": {
            "five_hour": {"used_percentage": 26.4},
            "seven_day": {"used_percentage": 7.1}
        }
    }"#;

    #[test]
    fn matches_the_reference_layout() {
        let line = render(&payload(FULL), 120, DEFAULT_ICON);
        assert_eq!(
            plain(&line),
            "\u{273b} dev-tools \u{b7} hour 26% \u{b7} week 7% \u{b7} chat 31k"
        );
    }

    #[test]
    fn width_accounting_matches_the_painted_line() {
        // The degradation loop trusts line_width; if the two ever disagree the
        // painted line overruns the terminal it was budgeted for. Asserted as
        // the invariant itself rather than a replay of the shed loop, which
        // would mirror a bug in that loop instead of catching it. Ordering is
        // pinned separately by sheds_week_then_hour_as_the_terminal_narrows.
        let short = payload(FULL);
        let long = payload(&FULL.replace("dev-tools", &"n".repeat(50)));
        // The model name is payload-supplied and unbounded, so it gets the
        // same treatment as the repo name: a realistic one and a hostile one.
        let spec = payload(SPEC);
        let long_spec = payload(&SPEC.replace("Claude Opus 5", &"m".repeat(60)));
        for p in [&short, &long, &spec, &long_spec] {
            for icon in [DEFAULT_ICON, ""] {
                for cols in 0..=120 {
                    let rendered = plain(&render(p, cols, icon));
                    assert!(
                        rendered.chars().count() <= cols.max(MIN_COLS),
                        "overrun at {cols} cols: {rendered:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn sheds_week_then_hour_as_the_terminal_narrows() {
        let p = payload(FULL);
        let at = |cols| plain(&render(&p, cols, DEFAULT_ICON));
        assert_eq!(at(43), "\u{273b} dev-tools \u{b7} hour 26% \u{b7} week 7% \u{b7} chat 31k");
        assert_eq!(at(42), "\u{273b} dev-tools \u{b7} hour 26% \u{b7} chat 31k");
        assert_eq!(at(30), "\u{273b} dev-tools \u{b7} chat 31k");
        // Past the last shed the line stops shrinking: the repo and the live
        // context number are the floor, not an empty string.
        assert_eq!(at(5), "\u{273b} dev-tools \u{b7} chat 31k");
    }

    #[test]
    fn the_model_and_effort_close_the_line() {
        assert_eq!(
            plain(&render(&payload(SPEC), 120, DEFAULT_ICON)),
            "\u{273b} dev-tools \u{b7} hour 26% \u{b7} week 7% \u{b7} chat 31k \u{b7} Opus 5 xhigh"
        );
    }

    #[test]
    fn a_model_without_an_effort_parameter_draws_its_name_alone() {
        // `effort` is absent on models that do not take the parameter, and a
        // trailing separator or an empty word in its place would read as a
        // dropped value rather than an inapplicable one.
        for json in [
            r#"{"cwd":"/x/proj","model":{"display_name":"Claude Haiku 4.5"}}"#,
            r#"{"cwd":"/x/proj","model":{"display_name":"Claude Haiku 4.5"},"effort":{}}"#,
            r#"{"cwd":"/x/proj","model":{"display_name":"Claude Haiku 4.5"},"effort":{"level":""}}"#,
        ] {
            assert_eq!(
                plain(&render(&payload(json), 120, DEFAULT_ICON)),
                "\u{273b} proj \u{b7} Haiku 4.5",
                "{json}"
            );
        }
    }

    #[test]
    fn an_effort_level_never_stands_on_its_own() {
        // Nothing upstream sends an effort without a model, and if that ever
        // changes a bare `xhigh` beside the usage numbers reads as a fourth
        // bucket. Drop the segment rather than draw a word with no subject.
        let p = payload(r#"{"cwd":"/x/proj","effort":{"level":"xhigh"},
                            "context_window":{"total_input_tokens":8000}}"#);
        assert_eq!(plain(&render(&p, 120, DEFAULT_ICON)), "\u{273b} proj \u{b7} chat 8k");
    }

    #[test]
    fn the_model_sheds_before_any_usage_number() {
        // The point of putting it last: every number that was on the line
        // before the model existed keeps its position and its width tier.
        let p = payload(SPEC);
        let at = |cols| plain(&render(&p, cols, DEFAULT_ICON));
        assert_eq!(
            at(58),
            "\u{273b} dev-tools \u{b7} hour 26% \u{b7} week 7% \u{b7} chat 31k \u{b7} Opus 5 xhigh"
        );
        assert_eq!(at(57), "\u{273b} dev-tools \u{b7} hour 26% \u{b7} week 7% \u{b7} chat 31k");
        // ...and from here the tiers are byte-identical to the model-less
        // fixture's, pinned by sheds_week_then_hour_as_the_terminal_narrows.
        assert_eq!(at(42), "\u{273b} dev-tools \u{b7} hour 26% \u{b7} chat 31k");
        assert_eq!(at(30), "\u{273b} dev-tools \u{b7} chat 31k");
    }

    #[test]
    fn absent_rate_limits_render_as_silence_not_zero() {
        let p = payload(r#"{"cwd":"/x/proj","context_window":{"total_input_tokens":8000}}"#);
        assert_eq!(plain(&render(&p, 120, DEFAULT_ICON)), "\u{273b} proj \u{b7} chat 8k");
    }

    #[test]
    fn the_icon_is_overridable_and_droppable() {
        let p = payload(FULL);
        assert_eq!(
            plain(&render(&p, 120, "\u{2736}")),
            "\u{2736} dev-tools \u{b7} hour 26% \u{b7} week 7% \u{b7} chat 31k"
        );
        // An empty icon drops the glyph *and* its trailing space, and the width
        // accounting has to follow or the tiers fire two columns early.
        assert_eq!(
            plain(&render(&p, 120, "")),
            "dev-tools \u{b7} hour 26% \u{b7} week 7% \u{b7} chat 31k"
        );
        assert_eq!(plain(&render(&p, 41, "")), "dev-tools \u{b7} hour 26% \u{b7} week 7% \u{b7} chat 31k");
        assert_eq!(plain(&render(&p, 40, "")), "dev-tools \u{b7} hour 26% \u{b7} chat 31k");
    }

    #[test]
    fn the_line_is_one_bold_span_and_no_colour_at_all() {
        let line = render(&payload(FULL), 120, DEFAULT_ICON);
        assert!(line.starts_with("\u{1b}[1m"), "must open bold: {line:?}");
        assert!(line.ends_with("\u{1b}[22m"), "must close with SGR 22: {line:?}");
        assert_eq!(line.matches('\u{1b}').count(), 2, "no inner escapes: {line:?}");
        // Every character lands in the terminal theme's own foreground — the
        // icon included, which is where a reverted experiment put a red span.
        for code in ["[30m", "[31m", "[32m", "[33m", "[36m", "[37m", "[39m", "[90m", "[0m"] {
            assert!(!line.contains(code), "unexpected colour {code} in {line:?}");
        }
    }

    #[test]
    fn a_cold_session_still_names_itself() {
        let p = payload(r#"{"workspace":{"repo":{"name":"dev-tools"}}}"#);
        assert_eq!(plain(&render(&p, 120, DEFAULT_ICON)), "\u{273b} dev-tools");
    }

    #[test]
    fn a_saturated_window_is_no_louder_than_any_other_segment() {
        // Weight is uniform by request: a 93% window must not single itself out.
        let hot = render(
            &payload(
                r#"{"workspace":{"repo":{"name":"r"}},
                    "rate_limits":{"five_hour":{"used_percentage":93}},
                    "context_window":{"total_input_tokens":1000,"used_percentage":99},
                    "exceeds_200k_tokens":true}"#,
            ),
            120,
            DEFAULT_ICON,
        );
        let cool = render(
            &payload(
                r#"{"workspace":{"repo":{"name":"r"}},
                    "rate_limits":{"five_hour":{"used_percentage":3}},
                    "context_window":{"total_input_tokens":1000,"used_percentage":1}}"#,
            ),
            120,
            DEFAULT_ICON,
        );
        // Stronger than counting: the two lines must carry the *same* escapes
        // in the same order, so saturation cannot buy a segment any styling at
        // all — not even by tinting the icon, which is what a usage-tracking
        // colour would do and what would split these two sequences apart.
        let escapes = |s: &str| {
            s.split('\u{1b}')
                .skip(1)
                .map(|e| {
                    e.split_inclusive(|c: char| c.is_ascii_alphabetic())
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(escapes(&hot), escapes(&cool), "{hot:?} vs {cool:?}");
        assert_eq!(escapes(&hot), ["[1m", "[22m"], "one bold span, nothing more: {hot:?}");
    }
}
