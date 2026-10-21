//! The key-hint footer, fitted to the pane's width.
//!
//! Each state lists its hints most useful first, and that order is their priority: a
//! narrow pane drops hints from the end, never cuts one, and marks the drop with `…`.
//! Outside the modals the row always ends `? keys`. [`HintMode`] picks one row, up to
//! [`FULL_ROWS`] wrapped rows, or none; a live notice replaces the hints in every mode,
//! and released mouse capture takes the first hint's place.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::text_width;
use crate::actions::Overlay;
use crate::app::App;
use crate::config::{Density, HintMode, ViewKind};
use crate::surface::Surface;

/// Rows full mode may wrap to.
const FULL_ROWS: usize = 3;

const SEPARATOR: &str = " · ";

#[derive(Debug, Clone)]
struct Hint {
    /// Text drawn as a notice ahead of the key, as in `mouse released · v to capture`.
    lead: Option<&'static str>,
    key: &'static str,
    label: String,
}

impl Hint {
    fn new(key: &'static str, label: impl Into<String>) -> Hint {
        Hint {
            lead: None,
            key,
            label: label.into(),
        }
    }

    fn released() -> Hint {
        Hint {
            lead: Some("mouse released"),
            key: "v",
            label: "to capture".to_string(),
        }
    }

    fn ellipsis() -> Hint {
        Hint::new("…", "")
    }

    fn width(&self) -> usize {
        let lead = self
            .lead
            .map_or(0, |lead| text_width(lead) + text_width(SEPARATOR));
        let label = if self.label.is_empty() {
            0
        } else {
            1 + text_width(&self.label)
        };
        lead + text_width(self.key) + label
    }

    fn push_spans(&self, app: &App, spans: &mut Vec<Span<'static>>) {
        if let Some(lead) = self.lead {
            spans.push(Span::styled(lead, app.theme.notice));
            spans.push(Span::styled(SEPARATOR, app.theme.key_separator));
        }
        if self.label.is_empty() {
            spans.push(Span::styled(self.key, app.theme.key_separator));
            return;
        }
        spans.push(Span::styled(self.key, app.theme.key));
        spans.push(Span::styled(
            format!(" {}", self.label),
            app.theme.key_label,
        ));
    }
}

/// Rows the footer takes at `width`: what [`render`] draws.
pub(crate) fn height(app: &App, width: u16, now: Instant) -> u16 {
    u16::try_from(lines(app, width, now).len()).unwrap_or(u16::MAX)
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    frame.render_widget(Paragraph::new(lines(app, area.width, now)), area);
}

fn lines(app: &App, width: u16, now: Instant) -> Vec<Line<'static>> {
    if let Some(notice) = app.live_notice(now) {
        return vec![Line::from(Span::styled(
            notice.to_string(),
            app.theme.notice,
        ))];
    }
    let (mut hints, tail) = hints(app);
    let released = app.capture_released && !app.mouse_captured;
    if released {
        if hints.is_empty() {
            hints.push(Hint::released());
        } else {
            hints[0] = Hint::released();
        }
    }
    let rows = match app.hints {
        HintMode::Hidden if !released => return Vec::new(),
        HintMode::Hidden => vec![vec![Hint::released()]],
        HintMode::Short => fit(&hints, tail.as_ref(), usize::from(width), 1),
        HintMode::Full => fit(&hints, tail.as_ref(), usize::from(width), FULL_ROWS),
    };
    rows.iter().map(|row| line(app, row)).collect()
}

fn line(app: &App, row: &[Hint]) -> Line<'static> {
    let mut spans = Vec::with_capacity(row.len() * 3);
    for (i, hint) in row.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(SEPARATOR, app.theme.key_separator));
        }
        hint.push_spans(app, &mut spans);
    }
    Line::from(spans)
}

/// The longest priority prefix of `hints` that wraps into `max_rows` rows of `width`
/// with `tail` last, and `…` before the tail when any hint was dropped. When not even
/// the tail fits, it is drawn alone and the pane's edge cuts it.
fn fit(hints: &[Hint], tail: Option<&Hint>, width: usize, max_rows: usize) -> Vec<Vec<Hint>> {
    let with = |kept: &[Hint], dropped: bool| -> Vec<Hint> {
        let mut row = kept.to_vec();
        if dropped {
            row.push(Hint::ellipsis());
        }
        row.extend(tail.cloned());
        row
    };
    let all = wrap(&with(hints, false), width);
    if all.len() <= max_rows {
        return all;
    }
    for kept in (0..hints.len()).rev() {
        let candidate = with(&hints[..kept], true);
        let rows = wrap(&candidate, width);
        if rows.len() <= max_rows {
            return rows;
        }
    }
    let last: Vec<Hint> = tail.cloned().into_iter().collect();
    if last.is_empty() {
        vec![vec![Hint::ellipsis()]]
    } else {
        vec![last]
    }
}

/// Greedy rows: each hint goes on the current row while it and a separator fit, else
/// starts the next one.
fn wrap(hints: &[Hint], width: usize) -> Vec<Vec<Hint>> {
    let gap = text_width(SEPARATOR);
    let mut rows: Vec<Vec<Hint>> = Vec::new();
    let mut used = 0;
    for hint in hints {
        let w = hint.width();
        match rows.last_mut() {
            Some(row) if used + gap + w <= width => {
                row.push(hint.clone());
                used += gap + w;
            }
            _ => {
                rows.push(vec![hint.clone()]);
                used = w;
            }
        }
    }
    rows
}

/// The current state's hints in priority order, and the hint pinned after them: `? keys`
/// everywhere but the modals, whose keys go to the modal.
fn hints(app: &App) -> (Vec<Hint>, Option<Hint>) {
    let on_off = |flag: bool| if flag { "on" } else { "off" };
    let compact = app.resolved_density == Density::Compact;
    if let Some(overlay) = &app.overlay {
        let hints = match overlay {
            Overlay::Prompt { .. } => vec![
                Hint::new("enter", "keep filter"),
                Hint::new("esc", "cancel"),
            ],
            Overlay::Menu { .. } | Overlay::Form { .. } => vec![
                Hint::new("tab", "next field"),
                Hint::new("enter", "choose/submit"),
                Hint::new("esc", "cancel"),
            ],
        };
        return (hints, None);
    }
    if app.legend_open {
        return (vec![Hint::new("?", "close legend")], None);
    }
    if app.theme_picker.is_some() {
        let hints = vec![
            Hint::new("j/k", "preview"),
            Hint::new("enter", "keep"),
            Hint::new("esc", "revert"),
        ];
        return (hints, None);
    }
    if app.selector_open {
        let hints = vec![
            Hint::new("j/k", "move"),
            Hint::new("enter", "switch"),
            Hint::new("a", format!("auto-follow {}", on_off(app.auto_flow))),
            Hint::new("esc", "close"),
        ];
        return (hints, None);
    }
    let mut hints = if app.details_open && compact {
        vec![
            Hint::new("J/K", "scroll"),
            Hint::new("hjkl", "move"),
            Hint::new("enter", "close"),
            Hint::new("t", "activity"),
        ]
    } else if app.surface != Surface::Tasks {
        surface_hints(app, compact)
    } else if compact {
        vec![
            Hint::new("enter", "details"),
            Hint::new("tab", "view"),
            Hint::new("d", "density"),
            Hint::new("q", "back"),
            Hint::new("hjkl", "move"),
        ]
    } else {
        tasks_hints(app)
    };
    let copies = app.surface == Surface::Tasks
        || app.surface == Surface::Inbox
        || app.current_items().is_some();
    if copies {
        hints.push(Hint::new("y", "copy id"));
    }
    if app.mouse_captured {
        hints.push(Hint::new("v", "select text"));
    }
    hints.push(Hint::new("H", "hints"));
    (hints, Some(Hint::new("?", "keys")))
}

fn tasks_hints(app: &App) -> Vec<Hint> {
    let on_off = |flag: bool| if flag { "on" } else { "off" };
    let details = if !app.details_open {
        "details"
    } else if !app.details_fullscreen {
        "full-screen"
    } else {
        "close details"
    };
    let mut hints = vec![
        Hint::new("hjkl", "move"),
        Hint::new("tab", "view"),
        Hint::new("enter", details),
    ];
    if app.details_open {
        hints.push(Hint::new("J/K", "scroll"));
    }
    hints.extend([
        Hint::new("o", "orient"),
        Hint::new("|", "split"),
        Hint::new("t", "activity"),
        Hint::new("f", format!("follow {}", on_off(app.follow))),
    ]);
    if app.view == ViewKind::Diagram {
        hints.push(Hint::new(
            "i",
            format!("implied {}", on_off(app.show_implied)),
        ));
    }
    hints.extend([
        Hint::new("s", "flows"),
        Hint::new("d", "density"),
        Hint::new("q", "back"),
    ]);
    hints
}

/// An item surface's hints; Esc reads "clear marks" while any are set, since that is
/// what it does first.
fn surface_hints(app: &App, compact: bool) -> Vec<Hint> {
    if app.surface == Surface::Inbox {
        return inbox_hints(app, compact);
    }
    let Some(state) = app.current_items() else {
        return vec![
            Hint::new("1-6", "surface"),
            Hint::new("s", "flows"),
            Hint::new("q", "back"),
        ];
    };
    let mut hints = vec![
        Hint::new("j/k", "move"),
        Hint::new("space", "mark"),
        Hint::new("enter", "details"),
    ];
    if !state.marks.is_empty() {
        hints.push(Hint::new(
            "esc",
            format!("clear {} marks", state.marks.len()),
        ));
    }
    hints.push(Hint::new("m", "actions"));
    if compact {
        hints.extend([Hint::new("g", "group"), Hint::new("q", "back")]);
        return hints;
    }
    if matches!(app.surface, Surface::Review | Surface::Optimise) {
        hints.push(Hint::new("e", "classify"));
    }
    hints.extend([
        Hint::new("r", "request"),
        Hint::new("n", "capture"),
        Hint::new("u", "undo"),
        Hint::new("/", "filter"),
        Hint::new("V", "mark all"),
        Hint::new("g", "group"),
        Hint::new("S", "sort"),
        Hint::new("c", "closed"),
        Hint::new("1-6", "surface"),
        Hint::new("s", "flows"),
        Hint::new("q", "back"),
    ]);
    hints
}

/// Enter answers the cursor question and opens details on any other record; `w` shows
/// only while the cursor is on a record the user may withdraw.
fn inbox_hints(app: &App, compact: bool) -> Vec<Hint> {
    let inbox = &app.inbox;
    let enter = if inbox.cursor_question().is_some() {
        "answer"
    } else {
        "details"
    };
    let mut hints = vec![Hint::new("j/k", "move"), Hint::new("enter", enter)];
    if inbox.cursor_withdrawable().is_some() {
        hints.push(Hint::new("w", "withdraw"));
    }
    hints.push(Hint::new("n", "capture"));
    hints.push(Hint::new("c", "closed"));
    if !compact {
        hints.extend([Hint::new("1-6", "surface"), Hint::new("s", "flows")]);
    }
    hints.push(Hint::new("q", "back"));
    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Action;
    use crate::config::Config;
    use crate::model::fixture;

    fn app() -> App {
        App::new(fixture(), &Config::default())
    }

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    /// Every hint the Tasks surface can show, as drawn, plus the ellipsis.
    fn whole_hints(app: &App) -> Vec<String> {
        let (hints, tail) = hints(app);
        let mut all: Vec<String> = hints
            .iter()
            .chain(tail.as_ref())
            .map(|hint| text(&[line(app, std::slice::from_ref(hint))]).remove(0))
            .collect();
        all.push("…".to_string());
        all
    }

    #[test]
    fn short_hints_fit_the_width_whole_and_end_with_keys() {
        let app = app();
        let whole = whole_hints(&app);
        for width in [60u16, 110, 200] {
            let rows = text(&lines(&app, width, Instant::now()));
            assert_eq!(rows.len(), 1, "{width}: {rows:?}");
            let row = &rows[0];
            assert!(row.ends_with("? keys"), "{width}: {row:?}");
            assert!(text_width(row) <= usize::from(width), "{width}: {row:?}");
            for part in row.split(SEPARATOR) {
                assert!(whole.iter().any(|h| h == part), "{width}: {part:?} cut");
            }
        }
        let narrow = &text(&lines(&app, 60, Instant::now()))[0];
        assert!(narrow.ends_with("… · ? keys"), "{narrow:?}");
        assert!(narrow.starts_with("hjkl move · tab view"), "{narrow:?}");
    }

    #[test]
    fn full_hints_wrap_to_at_most_three_rows() {
        let mut app = app();
        app.hints = HintMode::Full;
        let whole = whole_hints(&app);
        for width in [40u16, 60, 110, 200] {
            let rows = text(&lines(&app, width, Instant::now()));
            assert!((1..=FULL_ROWS).contains(&rows.len()), "{width}: {rows:?}");
            assert!(rows.last().is_some_and(|r| r.ends_with("? keys")));
            for row in &rows {
                assert!(text_width(row) <= usize::from(width), "{width}: {row:?}");
                for part in row.split(SEPARATOR) {
                    assert!(whole.iter().any(|h| h == part), "{width}: {part:?} cut");
                }
            }
            assert_eq!(height(&app, width, Instant::now()), rows.len() as u16);
        }
        assert!(
            text(&lines(&app, 60, Instant::now())).len() > 1,
            "60 columns wrap"
        );
        let tight = text(&lines(&app, 40, Instant::now()));
        assert!(tight[FULL_ROWS - 1].ends_with("… · ? keys"), "{tight:?}");
        let wide = text(&lines(&app, 200, Instant::now())).join(" · ");
        assert!(wide.contains("H hints") && !wide.contains('…'), "{wide}");
    }

    #[test]
    fn hidden_hints_take_no_row_unless_a_notice_is_live() {
        let mut app = app();
        app.hints = HintMode::Hidden;
        assert_eq!(height(&app, 110, Instant::now()), 0);

        app.hints = HintMode::Full;
        app.apply(Action::CycleHints);
        assert_eq!(app.hints, HintMode::Hidden);
        assert_eq!(height(&app, 110, Instant::now()), 1);
        assert_eq!(text(&lines(&app, 110, Instant::now())), ["hints hidden"]);
    }

    #[test]
    fn released_capture_takes_the_first_hint_and_a_hidden_row() {
        let mut app = app();
        app.apply(Action::ToggleCapture);
        app.mouse_captured = false;
        let row = &text(&lines(&app, 110, Instant::now()))[0];
        assert!(
            row.starts_with("mouse released · v to capture · tab view"),
            "{row:?}"
        );
        assert!(!row.contains("v select text"), "{row:?}");
        assert!(row.ends_with("? keys"), "{row:?}");

        app.hints = HintMode::Hidden;
        assert_eq!(
            text(&lines(&app, 110, Instant::now())),
            ["mouse released · v to capture"]
        );
    }

    #[test]
    fn an_uncaptured_start_keeps_its_hints_until_v_releases() {
        let mut app = app();
        app.mouse_captured = false;
        let row = &text(&lines(&app, 110, Instant::now()))[0];
        assert!(row.starts_with("hjkl move · tab view"), "{row:?}");
        assert!(!row.contains("mouse released"), "{row:?}");

        app.hints = HintMode::Hidden;
        assert_eq!(height(&app, 110, Instant::now()), 0);

        app.hints = HintMode::Short;
        app.apply(Action::ToggleCapture);
        app.mouse_captured = true;
        app.apply(Action::ToggleCapture);
        app.mouse_captured = false;
        let row = &text(&lines(&app, 110, Instant::now()))[0];
        assert!(row.starts_with("mouse released · v to capture"), "{row:?}");
    }

    #[test]
    fn modals_pin_no_keys_hint() {
        let mut app = app();
        app.legend_open = true;
        assert_eq!(text(&lines(&app, 110, Instant::now())), ["? close legend"]);
    }

    #[test]
    fn the_theme_picker_lists_its_own_keys() {
        let mut app = app();
        app.apply(Action::ToggleThemes);
        assert!(app.theme_picker.is_some());
        assert_eq!(
            text(&lines(&app, 110, Instant::now())),
            ["j/k preview · enter keep · esc revert"]
        );
    }
}
