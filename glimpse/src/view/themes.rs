//! The theme picker overlay: the sets, the cursor and a sample in the live theme.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::panel;
use crate::app::App;
use crate::theme::sets;

const TITLE: &str = " themes ";

/// Status words drawn in the previewed theme.
const SAMPLE: [&str; 5] = ["pending", "in-progress", "done", "failed", "deferred"];

fn row(app: &App, i: usize, name: &str) -> Line<'static> {
    let at_cursor = app.theme_picker.as_ref().is_some_and(|p| p.cursor == i);
    let cursor = if at_cursor { "▸" } else { " " };
    let current = if app.theme_set == name { "●" } else { " " };
    let line = Line::from(vec![
        Span::styled(cursor, app.theme.selection_mark),
        Span::styled(current, app.theme.in_progress),
        Span::raw(format!(" {name}")),
    ]);
    if at_cursor {
        line.style(app.theme.selection)
    } else {
        line
    }
}

fn sample(app: &App) -> Line<'static> {
    let mut spans = Vec::with_capacity(SAMPLE.len() * 2);
    for (i, status) in SAMPLE.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(*status, app.theme.status(status)));
    }
    Line::from(spans)
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = sets::SETS
        .iter()
        .enumerate()
        .map(|(i, set)| row(app, i, set.name))
        .collect();
    lines.push(Line::raw(""));
    lines.push(sample(app));
    let widest = lines.iter().map(Line::width).max().unwrap_or(0);
    let width = (widest.max(TITLE.chars().count()) + 4) as u16;
    let height = lines.len() as u16 + 2;
    let rect = area.centered(
        Constraint::Length(width.min(area.width)),
        Constraint::Length(height.min(area.height)),
    );
    frame.render_widget(Clear, rect);
    let block = panel::block(TITLE, &app.theme);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ThemePicker;
    use crate::config::Config;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn draw(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), app))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..12)
            .map(|y| {
                (0..60)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_picker_lists_every_set_with_the_cursor_and_current_marks() {
        let mut app = App::new(fixture(), &Config::default());
        app.theme_picker = Some(ThemePicker {
            cursor: 2,
            original: app.theme_set.clone(),
        });
        let text = draw(&app);
        for set in sets::SETS {
            assert!(text.contains(set.name), "{} missing: {text}", set.name);
        }
        assert!(text.contains("▸ ") && text.contains("ansi"), "{text}");
        assert!(text.contains("● kanso-zen"), "current set marked: {text}");
        assert!(text.contains("in-progress"), "sample chips: {text}");
    }
}
