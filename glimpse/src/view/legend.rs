//! The `?` overlay: what each mark, colour and key means, drawn in the live theme so a
//! customised palette explains itself.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use super::items;
use crate::app::App;
use crate::ledger::StatusClass;
use crate::model::TaskStatus;
use crate::theme::Theme;

/// Width of the left column, where each sample sits.
const SAMPLE: usize = 15;

const KEYS: &[(&str, &str)] = &[
    ("hjkl ←↓↑→", "move the selection"),
    ("tab S-tab", "next / previous view"),
    ("enter", "details, full-screen, close"),
    ("J K PgUp PgDn", "scroll details"),
    ("t", "activity panel"),
    ("o", "flip the layer orientation"),
    ("|", "panels beside / below"),
    ("[ ]", "shrink / grow the panel (or drag its edge)"),
    ("- =", "narrow / widen layer columns"),
    ("i", "diagram: show implied edges"),
    ("f", "follow the frontier"),
    ("s  a", "flows / auto-follow flows"),
    ("d", "density: auto, compact, comfortable"),
    (
        "1-6",
        "surface: tasks review optimise plan-review backlog inbox",
    ),
    ("space", "mark the item and move on"),
    ("V", "mark every visible item"),
    ("g  S", "items: next group-by / sort"),
    ("c", "items, inbox: show / hide closed"),
    ("m  e", "items: action menu / classify"),
    ("r  n", "request or note on the items / capture one"),
    ("u  /", "undo glimpse's last write / filter items"),
    ("w  enter", "inbox: withdraw your record / answer"),
    (
        "tab enter esc",
        "in a form: field, choose or submit, cancel",
    ),
    ("q esc", "clear marks, then back, then quit"),
];

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App) {
    let lines = lines(&app.theme);
    let widest = lines.iter().map(Line::width).max().unwrap_or(0);
    let width = u16::try_from(widest + 4).unwrap_or(u16::MAX);
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let rect = area.centered(
        Constraint::Length(width.min(area.width)),
        Constraint::Length(height.min(area.height)),
    );
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.theme.border)
        .title(Line::styled(" legend ", app.theme.border_title))
        .padding(Padding::horizontal(1));
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

fn lines(theme: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let heading = |out: &mut Vec<Line<'static>>, text: &str| {
        if !out.is_empty() {
            out.push(Line::default());
        }
        out.push(Line::styled(text.to_string(), theme.section));
    };
    let row = |sample: Span<'static>, text: &str| glyphs(theme, vec![sample], text);

    heading(&mut out, "Status");
    for status in [
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Done,
        TaskStatus::Failed,
        TaskStatus::Deferred,
    ] {
        out.push(row(
            Span::styled(status.glyph(), theme.status(status.as_str())),
            status.as_str(),
        ));
    }
    out.push(row(
        Span::styled(" done ", theme.flash(TaskStatus::Done)),
        "a status just changed, in the new status's colour",
    ));

    heading(&mut out, "Around the selection");
    for (mark, style, text) in [
        ("▸", theme.selection_mark, "the selected task"),
        ("↑", theme.needs_edge, "a task it needs"),
        ("↓", theme.needs_edge, "a task that needs it"),
        ("⇡ ⇣", theme.coupling_edge, "coupled before / after it"),
        (
            "≈",
            theme.overlap,
            "shares a file with it (underlined in the diagram)",
        ),
        ("title", theme.unrelated, "unrelated to it"),
    ] {
        out.push(row(Span::styled(mark, style), text));
    }

    heading(&mut out, "Diagram edges");
    for (sample, style, text) in [
        ("──▶", theme.needs_edge, "into the selection"),
        ("──▶", theme.out_edge, "out of the selection"),
        ("┄┄▶", theme.coupling_edge, "coupling"),
        ("──▶", theme.edge_faded, "elsewhere, with a selection"),
    ] {
        out.push(row(Span::styled(sample, style), text));
    }

    heading(&mut out, "Chips and marks");
    out.push(row(
        super::chip("ID 12m", theme.agent_chip),
        "running agent: type initials, elapsed",
    ));
    out.push(row(
        super::chip("ID 12m", theme.agent_chip_stale),
        "running agent gone quiet",
    ));
    out.push(row(
        Span::styled("◆ A", theme.checkpoint),
        "checkpoint, its verdict and commits",
    ));
    for (style, text) in [
        (theme.effort, "effort S/M/L"),
        (
            theme.effort_warning,
            "effort: a deviation, deferral, retry or escalation",
        ),
        (theme.effort_danger, "effort: a failure"),
    ] {
        out.push(row(Span::styled("M", style), text));
    }

    heading(&mut out, "Items");
    for (class, text) in [
        (StatusClass::Live, "open"),
        (StatusClass::Parked, "deferred, promoted"),
        (
            StatusClass::Done,
            "fixed, applied, merged, resolved, verified-clean",
        ),
        (
            StatusClass::Declined,
            "wontfix, wontapply, discarded, dismissed",
        ),
    ] {
        out.push(row(
            Span::styled(
                items::glyph(class),
                theme.item_status(items::class_name(class)),
            ),
            text,
        ));
    }
    for severity in ["critical", "warning", "suggestion"] {
        out.push(row(
            Span::styled(severity, theme.severity(severity)),
            "severity",
        ));
    }
    out.push(row(Span::styled("●", theme.item_mark), "marked"));
    out.push(row(
        Span::styled("↻", theme.item_saving),
        "saving: not yet in the ledger",
    ));
    out.push(row(
        super::chip("ID", theme.agent_chip),
        "running agent whose dispatch names the item",
    ));
    out.push(row(
        Span::styled("▾ warning (2)", theme.group_header),
        "a group and its count",
    ));
    out.push(row(
        Span::styled("+3", theme.arrival_badge),
        "on a surface tab: changes since you last viewed it",
    ));
    out.push(glyphs(
        theme,
        vec![
            Span::styled("✉", theme.input_new),
            Span::styled("✉", theme.input_acknowledged),
        ],
        "an input names it: new / acknowledged",
    ));
    out.push(glyphs(
        theme,
        vec![
            Span::styled("?", theme.question),
            Span::styled("●", theme.input_new),
            Span::styled("◐", theme.input_acknowledged),
            Span::styled("✓", theme.input_handled),
            Span::styled("✗", theme.secondary),
        ],
        "inbox: question, new, acknowledged, handled, withdrawn",
    ));

    heading(&mut out, "Keys");
    for (key, text) in KEYS {
        out.push(row(Span::styled(*key, theme.key), text));
    }
    out
}

/// One legend row: `samples` a space apart, padded to the [`SAMPLE`] column, then `text`.
fn glyphs(theme: &Theme, samples: Vec<Span<'static>>, text: &str) -> Line<'static> {
    let mut spans = Vec::with_capacity(samples.len() * 2 + 1);
    for sample in samples {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(sample);
    }
    let used: usize = spans.iter().map(Span::width).sum();
    spans.push(Span::raw(" ".repeat(SAMPLE.saturating_sub(used))));
    spans.push(Span::styled(text.to_string(), theme.key_label));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn the_legend_draws_every_status_in_its_colour() {
        let app = App::new(fixture(), &Config::default());
        let lines = lines(&app.theme);
        for status in ["pending", "in-progress", "done", "failed", "deferred"] {
            let line = lines
                .iter()
                .find(|l| l.spans.last().is_some_and(|s| s.content == status))
                .expect(status);
            assert_eq!(line.spans[0].style, app.theme.status(status));
        }

        let mut terminal = Terminal::new(TestBackend::new(100, 60)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), &app))
            .expect("draw");
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(screen.contains("legend") && screen.contains("shares a file"));
    }

    #[test]
    fn the_legend_draws_item_glyphs_and_severities_in_their_colours() {
        let theme = Theme::default();
        let lines = lines(&theme);
        let sample = |text: &str| {
            lines
                .iter()
                .find(|l| l.spans.last().is_some_and(|s| s.content == text))
                .map(|l| (l.spans[0].content.to_string(), l.spans[0].style))
                .expect(text)
        };
        assert_eq!(
            sample("deferred, promoted"),
            ("⏸".to_string(), theme.item_parked)
        );
        assert_eq!(
            sample("wontfix, wontapply, discarded, dismissed"),
            ("✗".to_string(), theme.item_declined)
        );
        assert_eq!(sample("marked"), ("●".to_string(), theme.item_mark));
        let severities: Vec<_> = lines
            .iter()
            .filter(|l| l.spans.last().is_some_and(|s| s.content == "severity"))
            .map(|l| l.spans[0].style)
            .collect();
        assert_eq!(
            severities,
            [
                theme.severity_critical,
                theme.severity_warning,
                theme.severity_suggestion
            ]
        );
    }

    #[test]
    fn samples_share_one_column() {
        let theme = Theme::default();
        for line in lines(&theme).iter().filter(|l| l.spans.len() >= 3) {
            let (_, samples) = line.spans.split_last().expect("a row");
            let sample: usize = samples.iter().map(Span::width).sum();
            assert_eq!(sample, SAMPLE, "{line:?}");
        }
    }

    #[test]
    fn the_legend_draws_input_marks_in_their_colours() {
        let theme = Theme::default();
        let lines = lines(&theme);
        let samples = |text: &str| -> Vec<(String, ratatui::style::Style)> {
            let line = lines
                .iter()
                .find(|l| l.spans.last().is_some_and(|s| s.content == text))
                .expect(text);
            let (_, samples) = line.spans.split_last().expect("a row");
            samples
                .iter()
                .filter(|s| !s.content.trim().is_empty())
                .map(|s| (s.content.to_string(), s.style))
                .collect()
        };
        assert_eq!(
            samples("an input names it: new / acknowledged"),
            [
                ("✉".to_string(), theme.input_new),
                ("✉".to_string(), theme.input_acknowledged),
            ]
        );
        assert_eq!(
            samples("inbox: question, new, acknowledged, handled, withdrawn"),
            [
                ("?".to_string(), theme.question),
                ("●".to_string(), theme.input_new),
                ("◐".to_string(), theme.input_acknowledged),
                ("✓".to_string(), theme.input_handled),
                ("✗".to_string(), theme.secondary),
            ]
        );
    }

    #[test]
    fn every_bound_letter_key_has_a_legend_row() {
        let keys: String = KEYS.iter().map(|(key, _)| format!(" {key} ")).collect();
        for key in ["m", "e", "r", "n", "u", "/", "w", "c", "enter"] {
            assert!(keys.contains(&format!(" {key} ")), "{key} is missing");
        }
    }
}
