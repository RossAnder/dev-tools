//! The flow selector overlay.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use crate::app::{App, SelectorEntry};

/// The slug under the selector cursor, if a flow is there.
#[cfg(test)]
pub(crate) fn selected_slug(app: &App) -> Option<&str> {
    match app.selector_entries().get(app.selector_cursor)? {
        SelectorEntry::Flow(flow) => Some(flow.slug.as_str()),
        SelectorEntry::LedgerOnly(slug) => Some(slug),
        SelectorEntry::Scope(_) => None,
    }
}

fn row(app: &App, i: usize, entry: SelectorEntry<'_>) -> Line<'static> {
    let cursor = if i == app.selector_cursor { "▸" } else { " " };
    let current = if app.is_current(entry) { "●" } else { " " };
    let mut spans = vec![
        Span::styled(cursor, app.theme.selection_mark),
        Span::styled(current, app.theme.in_progress),
    ];
    match entry {
        SelectorEntry::Flow(flow) => spans.extend([
            Span::raw(format!(" {}  ", flow.slug)),
            Span::styled(flow.status.clone(), app.theme.status(&flow.status)),
            Span::styled(format!("  {}", flow.updated), app.theme.secondary),
        ]),
        SelectorEntry::LedgerOnly(slug) => spans.extend([
            Span::raw(format!(" {slug}  ")),
            Span::styled("no tasks", app.theme.secondary),
        ]),
        SelectorEntry::Scope(scope) => {
            spans.push(Span::raw(format!(
                " {}: {}",
                scope.kind.as_str(),
                scope.scope
            )));
        }
    }
    let line = Line::from(spans);
    if i == app.selector_cursor {
        line.style(app.theme.selection)
    } else {
        line
    }
}

/// The flow rows, then a separator line (as `None`) and the flow-less ledgers.
fn rows(app: &App) -> Vec<Option<Line<'static>>> {
    let entries = app.selector_entries();
    let mut lines = Vec::with_capacity(entries.len() + 1);
    let first_scope = entries
        .iter()
        .position(|entry| matches!(entry, SelectorEntry::Scope(_)))
        .unwrap_or(entries.len());
    if first_scope == 0 {
        lines.push(Some(Line::from(Span::styled(
            "no flows found",
            app.theme.secondary,
        ))));
    }
    for (i, entry) in entries.into_iter().enumerate() {
        if i == first_scope {
            lines.push(None);
        }
        lines.push(Some(row(app, i, entry)));
    }
    lines
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App) {
    let auto = if app.auto_flow { "on" } else { "off" };
    let rows = rows(app);
    let widest = rows.iter().flatten().map(Line::width).max().unwrap_or(0);
    let mut lines: Vec<Line> = rows
        .into_iter()
        .map(|line| line.unwrap_or_else(|| Line::styled("─".repeat(widest), app.theme.border)))
        .collect();
    let title = format!(" flows · auto-follow {auto} ");
    let width = (widest.max(title.chars().count()) + 4) as u16;
    let height = lines.len() as u16 + 2;
    let rect = area.centered(
        Constraint::Length(width.min(area.width)),
        Constraint::Length(height.min(area.height)),
    );
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.theme.border)
        .title(Line::styled(title, app.theme.border_title))
        .padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(std::mem::take(&mut lines)), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::flows::{FlowEntry, ScopeEntry, Scopes};
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::time::SystemTime;
    use tomlctl::LedgerKind;

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
    fn the_selector_lists_flowless_scopes_after_a_separator() {
        let mut app = app();
        app.scopes = Scopes {
            ledger_only: vec!["loose".to_string()],
            scopes: vec![
                ScopeEntry {
                    kind: LedgerKind::Review,
                    scope: "core".to_string(),
                },
                ScopeEntry {
                    kind: LedgerKind::PlanReview,
                    scope: "draft".to_string(),
                },
            ],
        };
        app.selector_cursor = 2;
        let lines = draw(&app);
        let row = |needle: &str| {
            lines
                .iter()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("no `{needle}` row: {}", lines.join("\n")))
        };
        let separator = row("│ ───");
        assert!(
            row("demo-flow") < row("loose"),
            "task-store flows come first"
        );
        assert!(row("loose") < separator);
        assert!(lines[row("loose")].contains("no tasks"));
        assert!(lines[row("loose")].contains("▸"), "the cursor is on it");
        assert_eq!(selected_slug(&app), Some("loose"));
        assert!(separator < row("review: core"));
        assert!(row("review: core") < row("plan-review: draft"));
        assert!(!lines[separator].contains("▸"));
    }

    #[test]
    fn an_empty_list_says_so() {
        let mut app = app();
        app.flows.clear();
        assert_eq!(selected_slug(&app), None);
        assert!(draw(&app).join("\n").contains("no flows found"));
    }
}
