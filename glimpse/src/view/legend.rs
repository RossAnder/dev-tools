//! The `?` overlay: what each mark, colour and key means, drawn in the live theme so a
//! customised palette explains itself.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use crate::app::App;
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
    ("q esc", "back, then quit"),
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
    let row = |sample: Span<'static>, text: &str| {
        let pad = SAMPLE.saturating_sub(sample.width());
        Line::from(vec![
            sample,
            Span::raw(" ".repeat(pad)),
            Span::styled(text.to_string(), theme.key_label),
        ])
    };

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

    heading(&mut out, "Keys");
    for (key, text) in KEYS {
        out.push(row(Span::styled(*key, theme.key), text));
    }
    out
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
    fn samples_share_one_column() {
        let theme = Theme::default();
        for line in lines(&theme).iter().filter(|l| l.spans.len() == 3) {
            let sample = line.spans[0].width() + line.spans[1].width();
            assert_eq!(sample, SAMPLE, "{line:?}");
        }
    }
}
