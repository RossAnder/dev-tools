//! The flow selector overlay.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;

/// The slug under the selector cursor, if any flow is listed.
pub(crate) fn selected_slug(app: &App) -> Option<&str> {
    app.flows
        .get(app.selector_cursor)
        .map(|flow| flow.slug.as_str())
}

fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    cell
}

fn rows(app: &App) -> Vec<Line<'static>> {
    if app.flows.is_empty() {
        return vec![Line::from(Span::styled(
            "no flows found",
            app.theme.pending,
        ))];
    }
    app.flows
        .iter()
        .enumerate()
        .map(|(i, flow)| {
            let cursor = if i == app.selector_cursor { "▸" } else { " " };
            let current = if flow.slug == app.snapshot.slug {
                "●"
            } else {
                " "
            };
            let line = Line::from(vec![
                Span::raw(format!("{cursor}{current} {}  ", flow.slug)),
                Span::styled(flow.status.clone(), app.theme.status(&flow.status)),
                Span::raw(format!("  {}", flow.updated)),
            ]);
            if i == app.selector_cursor {
                line.style(app.theme.selection)
            } else {
                line
            }
        })
        .collect()
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App) {
    let auto = if app.auto_flow { "on" } else { "off" };
    let mut lines = rows(app);
    let widest = lines.iter().map(Line::width).max().unwrap_or(0);
    let title = format!(" flows · auto-follow {auto} ");
    let width = (widest.max(title.chars().count()) + 4) as u16;
    let height = lines.len() as u16 + 2;
    let rect = centred(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.theme.badge)
        .title(title);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(std::mem::take(&mut lines)), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::flows::FlowEntry;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::time::SystemTime;

    fn flow(slug: &str, updated: &str) -> FlowEntry {
        FlowEntry {
            slug: slug.to_string(),
            status: "in-progress".to_string(),
            updated: updated.to_string(),
            plan_path: String::new(),
            tasks_mtime: SystemTime::UNIX_EPOCH,
        }
    }

    fn draw(app: &App) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), app))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..10)
            .map(|y| {
                (0..60)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    fn app() -> App {
        let mut app = App::new(fixture(), &Config::default());
        app.flows = vec![flow("other", "2026-09-02"), flow("demo-flow", "2026-09-01")];
        app.selector_open = true;
        app
    }

    #[test]
    fn it_lists_flows_marking_the_current_one_and_the_cursor() {
        let mut app = app();
        app.selector_cursor = 0;
        let text = draw(&app).join("\n");
        assert!(text.contains("other"), "{text}");
        assert!(text.contains("2026-09-02"), "{text}");
        assert!(text.contains("▸  other"), "cursor row: {text}");
        assert!(text.contains(" ● demo-flow"), "current flow marked: {text}");
        assert!(text.contains("auto-follow off"), "{text}");
        assert_eq!(selected_slug(&app), Some("other"));

        app.auto_flow = true;
        assert!(draw(&app).join("\n").contains("auto-follow on"));
    }

    #[test]
    fn an_empty_list_says_so() {
        let mut app = app();
        app.flows.clear();
        assert_eq!(selected_slug(&app), None);
        assert!(draw(&app).join("\n").contains("no flows found"));
    }
}
