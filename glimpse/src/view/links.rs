//! The links list: a modal list of the records the shown details relate to.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::{Place, place, truncate};
use crate::app::App;

/// Draws the open list centred in `area`, windowed around its cursor, and records each
/// drawn row's rect and the page height back into `app`.
pub(crate) fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(list) = &app.links else {
        return;
    };
    let title = format!(" links · {} ", list.links.len());
    let max_label = usize::from(area.width.saturating_sub(6));
    let labels: Vec<String> = list
        .links
        .iter()
        .map(|link| truncate(&link.label, max_label))
        .collect();
    let widest = labels
        .iter()
        .map(|l| super::text_width(l))
        .max()
        .unwrap_or(0);

    let avail = usize::from(area.height.saturating_sub(2));
    let overflow = labels.len() > avail;
    let view = if overflow {
        avail.saturating_sub(2).max(1)
    } else {
        labels.len()
    };
    let start = place(0, Some(list.cursor), labels.len(), view, Place::Jump);
    let end = (start + view).min(labels.len());

    let mut lines: Vec<Line> = Vec::with_capacity(view + 2);
    let marker = |text: &str, shown: bool| {
        if shown {
            Line::styled(text.to_string(), app.theme.secondary)
        } else {
            Line::raw("")
        }
    };
    if overflow {
        lines.push(marker("↑ more", start > 0));
    }
    let first_row = lines.len();
    for (i, label) in labels.iter().enumerate().take(end).skip(start) {
        let selected = i == list.cursor;
        let mark = if selected { "▸ " } else { "  " };
        let line = Line::from(vec![
            Span::styled(mark, app.theme.selection_mark),
            Span::raw(label.clone()),
        ]);
        lines.push(if selected {
            line.style(app.theme.selection)
        } else {
            line
        });
    }
    if overflow {
        lines.push(marker("↓ more", end < labels.len()));
    }

    let width = (widest + 2).max(title.chars().count()) + 4;
    let width = u16::try_from(width).unwrap_or(u16::MAX).min(area.width);
    let height = u16::try_from(lines.len() + 2)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let rect = area.centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, rect);
    let block = super::panel::block(&title, &app.theme);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines), inner);

    let rows: Vec<(Rect, usize)> = (start..end)
        .enumerate()
        .filter_map(|(drawn, index)| {
            let y = inner.y + u16::try_from(first_row + drawn).ok()?;
            (y < inner.bottom()).then(|| (Rect::new(inner.x, y, inner.width, 1), index))
        })
        .collect();
    app.regions.links = rows;
    if let Some(list) = app.links.as_mut() {
        list.page = (end - start).max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Action, Dir};
    use crate::config::Config;
    use crate::keys;
    use crate::model::fixture;
    use crate::nav::Target;
    use crate::surface::Surface;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    fn draw(app: &mut App) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), app))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..20)
            .map(|y| {
                (0..80)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    fn key(app: &mut App, code: KeyCode) {
        let action = keys::map(KeyEvent::new(code, KeyModifiers::NONE)).expect("a mapped key");
        app.apply(action);
    }

    /// An app on Tasks with task 5, which has needs, selected.
    fn on_task_five() -> App {
        let mut app = App::new(fixture(), &Config::default());
        app.apply(Action::Select(5));
        app
    }

    #[test]
    fn links_b_on_a_task_with_needs_lists_them() {
        let mut app = on_task_five();
        key(&mut app, KeyCode::Char('b'));
        let list = app.links.as_ref().expect("the list is up");
        assert_eq!(list.links[0].target, Target::Task(1));
        assert_eq!(list.cursor, 0);
        let text = draw(&mut app).join("\n");
        assert!(text.contains("▸ #1 Scaffold the store"), "{text}");
        assert!(text.contains(" links · "), "{text}");
        assert!(
            !app.regions.links.is_empty(),
            "rows are recorded for the mouse"
        );
    }

    #[test]
    fn links_enter_opens_the_first_and_backspace_returns_with_the_list_closed() {
        let mut app = on_task_five();
        key(&mut app, KeyCode::Char('b'));
        key(&mut app, KeyCode::Enter);
        assert!(app.links.is_none(), "following a link closes the list");
        assert_eq!(app.selected, Some(1));
        assert_eq!(app.back.len(), 1);
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.surface, Surface::Tasks);
        assert_eq!(app.selected, Some(5));
        assert!(app.links.is_none());
    }

    #[test]
    fn links_b_with_no_links_opens_nothing_and_says_so() {
        let mut app = on_task_five();
        app.selected = None;
        key(&mut app, KeyCode::Char('b'));
        assert!(app.links.is_none());
        let notice = app
            .live_notice(std::time::Instant::now())
            .unwrap_or_default();
        assert!(notice.contains("no links"), "{notice}");
    }

    #[test]
    fn links_esc_closes_without_moving() {
        let mut app = on_task_five();
        app.details_open = true;
        key(&mut app, KeyCode::Char('b'));
        key(&mut app, KeyCode::Char('j'));
        key(&mut app, KeyCode::Esc);
        assert!(app.links.is_none());
        assert_eq!(app.selected, Some(5), "the selection stays");
        assert!(app.details_open, "Esc closed only the list");
        assert!(app.back.is_empty());
    }

    #[test]
    fn links_the_list_takes_every_key_while_up() {
        let mut app = on_task_five();
        key(&mut app, KeyCode::Char('b'));
        key(&mut app, KeyCode::Char('j'));
        assert_eq!(app.links.as_ref().map(|list| list.cursor), Some(1));
        assert_eq!(app.selected, Some(5), "j moved the list, not the selection");
        key(&mut app, KeyCode::Char('2'));
        assert_eq!(app.surface, Surface::Tasks, "a digit does nothing");
        key(&mut app, KeyCode::Char('k'));
        assert_eq!(app.links.as_ref().map(|list| list.cursor), Some(0));
    }

    #[test]
    fn links_a_click_on_a_row_opens_it() {
        let mut app = on_task_five();
        key(&mut app, KeyCode::Char('b'));
        draw(&mut app);
        let (rect, index) = app.regions.links[1];
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        };
        let action = keys::mouse(click, &app).expect("the row is a target");
        assert_eq!(action, Action::OpenLink(index));
        let target = app.links.as_ref().expect("up").links[index].target.clone();
        app.apply(action);
        assert!(app.links.is_none());
        let Target::Task(id) = target else {
            panic!("the second link of task 5 is a task: {target:?}");
        };
        assert_eq!(app.selected, Some(id));
    }

    #[test]
    fn links_a_decisions_links_open_its_bound_task_on_the_plan_surface() {
        let mut app = App::new(fixture(), &Config::default());
        app.apply(Action::SwitchSurface(Surface::Plan));
        app.apply(Action::SelectItem("decisions:D1".to_string()));
        key(&mut app, KeyCode::Char('b'));
        let at = app
            .links
            .as_ref()
            .expect("the list is up")
            .links
            .iter()
            .position(|link| link.target == Target::Task(6))
            .expect("D1 binds task 6");
        for _ in 0..at {
            app.apply(Action::Move(Dir::Down));
        }
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.surface, Surface::Tasks);
        assert_eq!(app.selected, Some(6));
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.surface, Surface::Plan);
        assert_eq!(app.plan.cursor.as_deref(), Some("decisions:D1"));
    }
}
