//! The Inbox surface: user input records and agent questions awaiting an answer.
//!
//! A facet row counts the unanswered questions above the list. A question row reads
//! `▸ ? I5 review  review/demo R3 Which fix?… · a / b`: cursor, glyph, id, author,
//! target, prompt and options. A record row reads `▸ ✓ I2 note handled  review R3
//! text… → note ↻`: cursor, status glyph, id, kind, status, target, a text excerpt,
//! the handling agent's note and the saving mark.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::items::Target;
use crate::app::App;
use crate::ledger::InputRow;
use crate::model::TaskStatus;
use crate::surface::{InboxState, VisibleRow};
use crate::theme::Theme;

/// Draws the Inbox into `area` of `buf`; a `Frame` caller passes `frame.buffer_mut()`.
/// Returns the drawn record rows.
pub(crate) fn render(buf: &mut Buffer, area: Rect, app: &App) -> Vec<Target> {
    render_at(buf, area, app, Instant::now())
}

/// `now` dates the flashes.
fn render_at(buf: &mut Buffer, area: Rect, app: &App, now: Instant) -> Vec<Target> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let state = &app.inbox;
    let width = usize::from(area.width);
    buf.set_line(area.x, area.y, &facet_line(app, state), area.width);
    let list = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    if list.height == 0 {
        return Vec::new();
    }

    let visible = state.visible();
    if visible.is_empty() {
        let text = match &state.revision {
            None => "reading the input store…",
            Some(_) if state.rows.is_empty() => "no input records yet",
            Some(_) => "no open records",
        };
        buf.set_line(
            list.x,
            list.y,
            &Line::from(Span::styled(text, app.theme.secondary)),
            list.width,
        );
        return Vec::new();
    }

    let columns = Columns::measure(state, &visible);
    let rows: Vec<Row> = visible
        .iter()
        .filter_map(|entry| match entry {
            VisibleRow::Header { label, count } => Some(Row {
                line: Line::from(Span::styled(
                    format!("▾ {label} ({count})"),
                    app.theme.group_header,
                )),
                fill: None,
                overlay: None,
                id: None,
            }),
            VisibleRow::Item(id) => {
                let row = state.row(id)?;
                let is_cursor = state.cursor.as_deref() == Some(id.as_str());
                let line = if row.is_unanswered_question() {
                    question_line(app, state, row, &columns, width, is_cursor)
                } else {
                    record_line(app, state, row, &columns, width, is_cursor)
                };
                Some(Row {
                    line,
                    fill: is_cursor.then_some(app.theme.selection),
                    overlay: (!is_cursor && state.is_flashing(id, now))
                        .then(|| app.theme.flash(flash_status(row))),
                    id: Some(id.clone()),
                })
            }
        })
        .collect();
    let at = rows
        .iter()
        .position(|row| row.id.is_some() && row.id == state.cursor);
    let offset = window(at, rows.len(), usize::from(list.height));
    draw_rows(buf, list, &rows[offset..])
}

/// A drawn row, the styles laid under and over its full width, and the record it shows.
struct Row {
    line: Line<'static>,
    fill: Option<Style>,
    overlay: Option<Style>,
    id: Option<String>,
}

fn draw_rows(buf: &mut Buffer, area: Rect, rows: &[Row]) -> Vec<Target> {
    let mut targets = Vec::new();
    for (row, y) in rows.iter().zip(area.y..area.bottom()) {
        let rect = Rect::new(area.x, y, area.width, 1);
        // The fill goes under the spans so a styled span keeps its own colours.
        if let Some(style) = row.fill {
            buf.set_style(rect, style);
        }
        buf.set_line(area.x, y, &row.line, area.width);
        if let Some(style) = row.overlay {
            buf.set_style(rect, style);
        }
        if let Some(id) = &row.id {
            targets.push((rect, id.clone()));
        }
    }
    targets
}

/// The first of `view` rows drawn from `len`, scrolled as little as keeps a third of
/// the window showing below the cursor `at`.
fn window(at: Option<usize>, len: usize, view: usize) -> usize {
    if view == 0 || len <= view {
        return 0;
    }
    let ahead = view / 3;
    let start = at.map_or(0, |at| (at + ahead + 1).saturating_sub(view));
    start.min(len - view)
}

/// `2 unanswered · closed hidden`.
fn facet_line(app: &App, state: &InboxState) -> Line<'static> {
    let closed = if state.show_closed { "shown" } else { "hidden" };
    let text = format!("{} unanswered · closed {closed}", state.unanswered());
    Line::from(Span::styled(text, app.theme.facet))
}

/// Widths of the aligned columns over the drawn rows. Questions align their author
/// under the records' kind and status together.
struct Columns {
    id: usize,
    kind: usize,
    status: usize,
    author: usize,
}

impl Columns {
    fn measure(state: &InboxState, visible: &[VisibleRow]) -> Columns {
        let mut columns = Columns {
            id: 0,
            kind: 0,
            status: 0,
            author: 0,
        };
        for row in visible.iter().filter_map(|entry| match entry {
            VisibleRow::Item(id) => state.row(id),
            VisibleRow::Header { .. } => None,
        }) {
            columns.id = columns.id.max(text_width(&row.id));
            if row.is_unanswered_question() {
                columns.author = columns.author.max(text_width(&row.author));
            } else {
                columns.kind = columns.kind.max(text_width(&row.kind));
                columns.status = columns.status.max(text_width(&row.status));
            }
        }
        columns
    }
}

/// The cursor mark, a blank, the glyph and the padded id every row starts with.
fn lead(
    theme: &Theme,
    glyph: Span<'static>,
    id: &str,
    id_width: usize,
    is_cursor: bool,
) -> Vec<Span<'static>> {
    vec![
        if is_cursor {
            Span::styled("▸", theme.selection_mark)
        } else {
            Span::raw(" ")
        },
        Span::raw(" "),
        glyph,
        Span::raw(format!(" {id:<id_width$}")),
    ]
}

fn question_line(
    app: &App,
    state: &InboxState,
    row: &InputRow,
    columns: &Columns,
    width: usize,
    is_cursor: bool,
) -> Line<'static> {
    let theme = &app.theme;
    let mut left = lead(
        theme,
        Span::styled("?", theme.question),
        &row.id,
        columns.id,
        is_cursor,
    );
    left.push(Span::styled(
        format!(" {:<w$}", row.author, w = columns.author),
        theme.question,
    ));
    push_target(&mut left, row, theme);
    left.push(Span::raw(" "));
    let options = row.options.join(" / ");
    finish(
        left,
        state,
        row,
        theme,
        width,
        (row.prompt.as_str(), theme.question),
        &options,
    )
}

fn record_line(
    app: &App,
    state: &InboxState,
    row: &InputRow,
    columns: &Columns,
    width: usize,
    is_cursor: bool,
) -> Line<'static> {
    let theme = &app.theme;
    let style = status_style(theme, &row.status);
    let mut left = lead(
        theme,
        Span::styled(glyph(&row.status), style),
        &row.id,
        columns.id,
        is_cursor,
    );
    left.push(Span::raw(format!(" {:<w$}", row.kind, w = columns.kind)));
    left.push(Span::styled(
        format!(" {:<w$}", row.status, w = columns.status),
        style,
    ));
    push_target(&mut left, row, theme);
    left.push(Span::raw(" "));
    let excerpt = excerpt(row);
    let note = if row.status == "handled" {
        row.handled_note.as_str()
    } else {
        ""
    };
    finish(
        left,
        state,
        row,
        theme,
        width,
        (excerpt.as_str(), Style::default()),
        note,
    )
}

/// Appends the body and its trailer in the room `left` leaves, then the saving mark.
/// The trailer (options or the handled note) takes at most half the room, and the
/// last cell stays blank so a highlight is padded on both sides.
fn finish(
    mut left: Vec<Span<'static>>,
    state: &InboxState,
    row: &InputRow,
    theme: &Theme,
    width: usize,
    (body, body_style): (&str, Style),
    trailer: &str,
) -> Line<'static> {
    let saving = state.saving.contains(&row.id);
    let tail = if saving { 2 } else { 0 };
    let room = width.saturating_sub(spans_width(&left) + tail + 1);
    let (body_room, trailer_room) = if trailer.is_empty() {
        (room, 0)
    } else {
        let wanted = text_width(trailer) + 3;
        let trailer_room = wanted.min(room / 2);
        (room - trailer_room, trailer_room)
    };
    let body = truncate(body, body_room);
    let mut used = text_width(&body);
    left.push(Span::styled(body, body_style));
    if trailer_room > 3 {
        let sep = if row.is_unanswered_question() {
            " · "
        } else {
            " → "
        };
        let trailer = truncate(trailer, trailer_room - 3);
        used += 3 + text_width(&trailer);
        left.push(Span::styled(format!("{sep}{trailer}"), theme.facet));
    }
    left.push(Span::raw(" ".repeat(room.saturating_sub(used))));
    if saving {
        left.push(Span::raw(" "));
        left.push(Span::styled("↻", theme.item_saving));
    }
    left.push(Span::raw(" "));
    Line::from(left)
}

/// Two spaces then `review/demo-flow R3,R7`, dimmed; nothing for an untargeted record.
fn push_target(left: &mut Vec<Span<'static>>, row: &InputRow, theme: &Theme) {
    let target = target_text(row);
    if !target.is_empty() {
        left.push(Span::raw("  "));
        left.push(Span::styled(target, theme.facet));
    }
}

/// The ledger, its flow or scope, and the targeted ids; a capture's kind and area.
fn target_text(row: &InputRow) -> String {
    let mut parts = Vec::new();
    let place = if row.flow.is_empty() {
        &row.scope
    } else {
        &row.flow
    };
    match (row.ledger.is_empty(), place.is_empty()) {
        (false, false) => parts.push(format!("{}/{place}", row.ledger)),
        (false, true) => parts.push(row.ledger.clone()),
        (true, false) => parts.push(place.clone()),
        (true, true) => {}
    }
    if !row.items.is_empty() {
        parts.push(row.items.join(","));
    }
    for hint in [&row.capture_kind, &row.area] {
        if !hint.is_empty() {
            parts.push(hint.clone());
        }
    }
    parts.join(" ")
}

/// The record's text; an answer without text reads as the question it answers and
/// the options picked, a question as its prompt.
fn excerpt(row: &InputRow) -> String {
    if !row.text.is_empty() {
        return one_line(&row.text);
    }
    if row.kind == "answer" && !row.answers.is_empty() {
        return format!("{}: {}", row.answers, row.picked.join(", "));
    }
    one_line(&row.prompt)
}

/// `s` with each run of whitespace, newlines included, collapsed to one space.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn status_style(theme: &Theme, status: &str) -> Style {
    match status {
        "new" => theme.input_new,
        "acknowledged" => theme.input_acknowledged,
        "handled" => theme.input_handled,
        _ => theme.secondary,
    }
}

fn glyph(status: &str) -> &'static str {
    match status {
        "new" => "●",
        "acknowledged" => "◐",
        "handled" => "✓",
        "withdrawn" => "✗",
        _ => "·",
    }
}

/// The task status whose flash colour is nearest the record's status colour.
fn flash_status(row: &InputRow) -> TaskStatus {
    match row.status.as_str() {
        "new" => TaskStatus::InProgress,
        "acknowledged" => TaskStatus::Deferred,
        "handled" => TaskStatus::Done,
        _ => TaskStatus::Pending,
    }
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

fn text_width(s: &str) -> usize {
    Span::raw(s).width()
}

/// `s` cut to at most `max` cells, ending in `…` when anything was cut.
fn truncate(s: &str, max: usize) -> String {
    if text_width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = text_width(c.encode_utf8(&mut [0; 4]));
        if used + w > max - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Action;
    use crate::config::Config;
    use crate::ledger::Inputs;
    use crate::model::fixture;
    use crate::surface::Surface;

    fn record(id: &str, kind: &str, author: &str, status: &str) -> InputRow {
        InputRow {
            id: id.to_string(),
            kind: kind.to_string(),
            author: author.to_string(),
            status: status.to_string(),
            ..InputRow::default()
        }
    }

    fn inbox_app(rows: Vec<InputRow>) -> App {
        let mut app = App::new(fixture(), &Config::default());
        app.apply(Action::SwitchSurface(Surface::Inbox));
        let inputs = Inputs {
            path: ".claude/inputs.toml".to_string(),
            revision: Some("v1".to_string()),
            rows,
        };
        app.apply_inputs(inputs, Instant::now());
        app
    }

    fn draw(app: &App, width: u16, height: u16) -> (Vec<String>, Vec<Target>) {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let targets = render_at(&mut buf, area, app, Instant::now());
        let rows = (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        (rows, targets)
    }

    fn row_of<'a>(rows: &'a [String], needle: &str) -> (usize, &'a str) {
        rows.iter()
            .enumerate()
            .find(|(_, row)| row.contains(needle))
            .map(|(y, row)| (y, row.as_str()))
            .unwrap_or_else(|| panic!("{needle:?} not drawn:\n{}", rows.join("\n")))
    }

    #[test]
    fn the_inbox_lists_questions_before_records() {
        let question = InputRow {
            prompt: "Which fix should land?".to_string(),
            options: vec!["guard".to_string(), "revert".to_string()],
            ledger: "review".to_string(),
            flow: "demo-flow".to_string(),
            items: vec!["R3".to_string()],
            ..record("I4", "question", "review", "new")
        };
        let note = InputRow {
            text: "Prefer the\nsmaller patch".to_string(),
            ledger: "review".to_string(),
            items: vec!["R3".to_string(), "R7".to_string()],
            ..record("I1", "note", "user", "new")
        };
        let handled = InputRow {
            text: "Split the module".to_string(),
            handled_note: "filed as B-12".to_string(),
            ..record("I2", "request", "user", "handled")
        };
        let mut app = inbox_app(vec![note, handled, question]);
        let (rows, targets) = draw(&app, 100, 10);
        assert!(
            rows[0].starts_with("1 unanswered · closed hidden"),
            "{:?}",
            rows[0]
        );
        let (questions, _) = row_of(&rows, "▾ questions (1)");
        let (i4, line) = row_of(&rows, "I4");
        assert!(
            line.contains(
                "? I4 review  review/demo-flow R3 Which fix should land? · guard / revert"
            ),
            "{line:?}"
        );
        let (new, _) = row_of(&rows, "▾ new (1)");
        let (i1, line) = row_of(&rows, "I1");
        assert!(
            line.contains("● I1 note new  review R3,R7 Prefer the smaller patch"),
            "{line:?}"
        );
        assert!(questions < i4 && i4 < new && new < i1, "{rows:#?}");
        assert!(!rows.join("\n").contains("I2"), "handled is hidden");
        let ids: Vec<&str> = targets.iter().map(|(_, id)| id.as_str()).collect();
        assert_eq!(ids, ["I4", "I1"]);
        assert_eq!(targets[0].0.y, u16::try_from(i4).expect("row"));
        assert!(line.starts_with("  ●"), "the cursor stays on I4: {line:?}");

        app.inbox.show_closed = true;
        app.inbox.saving.insert("I1".to_string());
        let (rows, _) = draw(&app, 100, 10);
        let (_, line) = row_of(&rows, "I2");
        assert!(
            line.contains("✓ I2 request handled Split the module → filed as B-12"),
            "{line:?}"
        );
        assert!(row_of(&rows, "I1").1.trim_end().ends_with('↻'));
    }

    #[test]
    fn an_empty_store_says_so() {
        let app = inbox_app(Vec::new());
        let (rows, targets) = draw(&app, 60, 4);
        assert!(rows[1].starts_with("no input records yet"), "{rows:#?}");
        assert!(targets.is_empty());

        let app = inbox_app(vec![record("I1", "note", "user", "withdrawn")]);
        let (rows, _) = draw(&app, 60, 4);
        assert!(rows[1].starts_with("no open records"), "{rows:#?}");
    }
}
