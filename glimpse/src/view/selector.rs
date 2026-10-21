//! The flow selector overlay.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::{Place, place};
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
/// Also the line index of the cursor entry and, per line, whether it is an entry.
fn rows(app: &App) -> (Vec<Option<Line<'static>>>, Option<usize>, Vec<bool>) {
    let entries = app.selector_entries();
    let mut lines = Vec::with_capacity(entries.len() + 1);
    let mut is_entry = Vec::with_capacity(entries.len() + 1);
    let mut cursor_line = None;
    let first_scope = entries
        .iter()
        .position(|entry| matches!(entry, SelectorEntry::Scope(_)))
        .unwrap_or(entries.len());
    if first_scope == 0 {
        lines.push(Some(Line::from(Span::styled(
            "no flows found",
            app.theme.secondary,
        ))));
        is_entry.push(false);
    }
    for (i, entry) in entries.into_iter().enumerate() {
        if i == first_scope {
            lines.push(None);
            is_entry.push(false);
        }
        if i == app.selector_cursor {
            cursor_line = Some(lines.len());
        }
        lines.push(Some(row(app, i, entry)));
        is_entry.push(true);
    }
    (lines, cursor_line, is_entry)
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let auto = if app.auto_flow { "on" } else { "off" };
    let (rows, cursor_line, is_entry) = rows(app);
    let widest = rows.iter().flatten().map(Line::width).max().unwrap_or(0);
    let all: Vec<Line> = rows
        .into_iter()
        .map(|line| line.unwrap_or_else(|| Line::styled("─".repeat(widest), app.theme.border)))
        .collect();
    let avail = usize::from(area.height.saturating_sub(2));
    let overflow = all.len() > avail;
    let mut lines: Vec<Line> = if overflow {
        let view = avail.saturating_sub(2).max(1);
        let start = place(0, cursor_line, all.len(), view, Place::Jump);
        let end = (start + view).min(all.len());
        app.selector_page = is_entry[start..end].iter().filter(|e| **e).count().max(1);
        let marker = |text: &str, shown: bool| {
            if shown {
                Line::styled(text.to_string(), app.theme.secondary)
            } else {
                Line::raw("")
            }
        };
        let mut window = vec![marker("↑ more", start > 0)];
        window.extend(all[start..end].iter().cloned());
        window.push(marker("↓ more", end < all.len()));
        window
    } else {
        app.selector_page = is_entry.iter().filter(|e| **e).count().max(1);
        all
    };
    let title = format!(" flows · auto-follow {auto} ");
    let width = (widest.max(title.chars().count()) + 4) as u16;
    let height = lines.len() as u16 + 2;
    let rect = area.centered(
        Constraint::Length(width.min(area.width)),
        Constraint::Length(height.min(area.height)),
    );
    frame.render_widget(Clear, rect);
    let block = super::panel::block(&title, &app.theme);
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

    fn draw(app: &mut App) -> Vec<String> {
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
        let text = draw(&mut app).join("\n");
        assert!(text.contains("other"), "{text}");
        assert!(text.contains("2026-09-02"), "{text}");
        assert!(text.contains("▸  other"), "cursor row: {text}");
        assert!(text.contains(" ● demo-flow"), "current flow marked: {text}");
        assert!(text.contains("auto-follow off"), "{text}");
        assert_eq!(selected_slug(&app), Some("other"));

        app.auto_flow = true;
        assert!(draw(&mut app).join("\n").contains("auto-follow on"));
    }

    #[test]
    fn a_long_list_draws_the_window_around_the_cursor() {
        let mut app = app();
        app.flows = (0..40)
            .map(|i| flow(&format!("flow-{i:02}"), "2026-09-01"))
            .collect();
        app.selector_cursor = 30;
        let text = draw(&mut app).join("\n");
        assert!(text.contains("▸"), "cursor row drawn: {text}");
        assert!(text.contains("flow-30"), "{text}");
        assert!(text.contains("↑ more") && text.contains("↓ more"), "{text}");
        assert!(app.selector_page > 1 && app.selector_page < 10, "page");
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
        let lines = draw(&mut app);
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
        assert!(draw(&mut app).join("\n").contains("no flows found"));
    }
}
