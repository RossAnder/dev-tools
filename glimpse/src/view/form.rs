//! The form overlay, drawn as a centred modal over the active view.

use ratatui::Frame;
use ratatui::layout::{Constraint, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph};
use tui_input::Input;

use crate::form::{Field, Form};
use crate::theme::Theme;

const INDENT: u16 = 2;
const MIN_WIDTH: usize = 40;
const MAX_WIDTH: usize = 72;
const OTHER: &str = "other…";

fn widest(form: &Form) -> usize {
    let fields = form.fields.iter().map(|field| {
        let options = match field {
            Field::Select { options, .. } | Field::Multi { options, .. } => options
                .iter()
                .map(|o| o.chars().count() + 4)
                .max()
                .unwrap_or(0),
            Field::Text { .. } => 0,
        };
        (field.label().chars().count() + 2).max(options)
    });
    let prompt = form.prompt.iter().map(|p| p.chars().count());
    let title = std::iter::once(form.title.chars().count() + 2);
    fields.chain(prompt).chain(title).max().unwrap_or(0)
}

fn body_height(field: &Field) -> u16 {
    let rows = match field {
        Field::Text { .. } => 1,
        Field::Select {
            options,
            open,
            free,
            ..
        } => {
            if *open {
                options.len() + usize::from(free.is_some())
            } else {
                1
            }
        }
        Field::Multi { options, .. } => options.len().max(1),
    };
    rows as u16
}

fn height(form: &Form) -> u16 {
    let prompt = form.prompt.as_ref().map_or(0, |_| 2);
    let fields: u16 = form.fields.iter().map(|f| 1 + body_height(f)).sum();
    let error = form.error.as_ref().map_or(0, |_| 1);
    prompt + fields + error + 2
}

fn slice(inner: Rect, y: u16, rows: u16) -> Rect {
    Rect::new(inner.x, inner.y.saturating_add(y), inner.width, rows).intersection(inner)
}

fn row_style(theme: &Theme, current: bool) -> Style {
    if current {
        theme.selection
    } else {
        Style::default()
    }
}

/// Draws `input` right of `prefix` columns in `rect` and returns where the
/// terminal cursor belongs.
fn draw_input(frame: &mut Frame, rect: Rect, prefix: u16, input: &Input) -> Position {
    let field = Rect::new(
        rect.x.saturating_add(prefix),
        rect.y,
        rect.width.saturating_sub(prefix).max(1),
        rect.height,
    )
    .intersection(rect);
    let scroll = input.visual_scroll(usize::from(field.width.saturating_sub(1)));
    frame.render_widget(
        Paragraph::new(input.value()).scroll((0, scroll as u16)),
        field,
    );
    let col = input.visual_cursor().saturating_sub(scroll) as u16;
    Position::new(
        field
            .x
            .saturating_add(col)
            .min(rect.right().saturating_sub(1)),
        field.y,
    )
}

fn draw_field(
    frame: &mut Frame,
    rect: Rect,
    field: &Field,
    focused: bool,
    theme: &Theme,
) -> Option<Position> {
    let body = Rect::new(
        rect.x.saturating_add(INDENT),
        rect.y,
        rect.width.saturating_sub(INDENT),
        rect.height,
    );
    match field {
        Field::Text { input, .. } => focused.then_some(draw_input(frame, body, 0, input)),
        Field::Select {
            options,
            cursor,
            open,
            free,
            ..
        } => {
            if *open {
                let mut items: Vec<ListItem> =
                    options.iter().map(|o| ListItem::new(o.as_str())).collect();
                if free.is_some() {
                    items.push(ListItem::new(OTHER));
                }
                let list = List::new(items)
                    .highlight_style(theme.selection)
                    .highlight_symbol("▸ ");
                let mut state = ListState::default().with_selected(Some(*cursor));
                frame.render_stateful_widget(list, body, &mut state);
                return None;
            }
            if field.on_other()
                && let Some(free) = free
            {
                let label = format!("{OTHER} ");
                let prefix = label.chars().count() as u16;
                frame.render_widget(Paragraph::new(Span::styled(label, theme.secondary)), body);
                return focused.then_some(draw_input(frame, body, prefix, free));
            }
            let shown = options.get(*cursor).map_or("", String::as_str);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("▾ ", theme.secondary),
                    Span::raw(shown.to_owned()),
                ])),
                body,
            );
            None
        }
        Field::Multi {
            options,
            checked,
            cursor,
            ..
        } => {
            let lines: Vec<Line> = options
                .iter()
                .zip(checked)
                .enumerate()
                .map(|(i, (option, on))| {
                    let mark = if *on { "[x]" } else { "[ ]" };
                    Line::styled(
                        format!("{mark} {option}"),
                        row_style(theme, focused && i == *cursor),
                    )
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), body);
            None
        }
    }
}

fn is_required(field: &Field) -> bool {
    match field {
        Field::Text { required, .. }
        | Field::Select { required, .. }
        | Field::Multi { required, .. } => *required,
    }
}

pub(crate) fn render(frame: &mut Frame, area: Rect, form: &Form, theme: &Theme) {
    let width = (widest(form) + 4).clamp(MIN_WIDTH, MAX_WIDTH) as u16;
    let rect = area.centered(
        Constraint::Length(width.min(area.width)),
        Constraint::Length(height(form).min(area.height)),
    );
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(Line::styled(
            format!(" {} ", form.title),
            theme.border_title,
        ))
        .padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut y = 0u16;
    if let Some(prompt) = &form.prompt {
        frame.render_widget(Paragraph::new(prompt.as_str()), slice(inner, y, 1));
        y += 2;
    }
    let mut cursor = None;
    for (i, field) in form.fields.iter().enumerate() {
        let focused = i == form.focus;
        let style = if focused {
            theme.form_focus
        } else {
            theme.form_label
        };
        let mark = if is_required(field) { " *" } else { "" };
        frame.render_widget(
            Paragraph::new(Span::styled(format!("{}{mark}", field.label()), style)),
            slice(inner, y, 1),
        );
        y += 1;
        let rows = body_height(field);
        let at = draw_field(frame, slice(inner, y, rows), field, focused, theme);
        cursor = cursor.or(at);
        y += rows;
    }
    if let Some(error) = &form.error {
        frame.render_widget(
            Paragraph::new(Span::styled(error.as_str(), theme.form_error)),
            slice(inner, y, 1),
        );
    }
    if let Some(at) = cursor
        && form.active_input().is_some()
        && inner.contains(at)
    {
        frame.set_cursor_position(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::Field;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    const W: u16 = 60;
    const H: u16 = 24;

    fn draw(form: &Form) -> (Vec<String>, Position) {
        let mut terminal = Terminal::new(TestBackend::new(W, H)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), form, &Theme::default()))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let rows = (0..H)
            .map(|y| {
                (0..W)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();
        (rows, terminal.get_cursor_position().expect("cursor"))
    }

    fn options(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_form_shows_its_fields_and_error() {
        let mut form = Form::new(
            "defer",
            vec![
                Field::text("reason").required().with_value(&["ab"]),
                Field::select("severity", options(&["critical", "warning"])).opened(),
                Field::multi("tags", options(&["x", "y"])).with_value(&["y"]),
            ],
        )
        .with_prompt("Why defer?");
        form.error = Some("reason is required".to_owned());
        let (rows, cursor) = draw(&form);
        let text = rows.join("\n");
        assert!(text.contains("defer"), "{text}");
        assert!(text.contains("Why defer?"), "{text}");
        assert!(text.contains("reason *"), "required mark: {text}");
        assert!(text.contains("▸ critical"), "open dropdown cursor: {text}");
        assert!(text.contains("warning"), "{text}");
        assert!(text.contains("[ ] x"), "{text}");
        assert!(text.contains("[x] y"), "checked box: {text}");
        assert!(text.contains("reason is required"), "error line: {text}");
        let typed = rows
            .iter()
            .position(|r| r.contains("ab"))
            .expect("value row");
        assert_eq!(usize::from(cursor.y), typed, "cursor on the input row");
    }
}
