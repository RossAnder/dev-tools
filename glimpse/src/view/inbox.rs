//! The Inbox surface: user input records and agent questions awaiting an answer.
//!
//! A facet row counts the unanswered questions above the list, and a header row of
//! column titles sits under it. A question row reads
//! `▸  ? I5 review  review/demo R3 Which fix?… · a / b`: cursor, glyph, id, author,
//! target, prompt and options. A record row reads `▸  ✓ I2 note handled  review R3
//! text… → note ↻`: cursor, status glyph, id, kind, status, target, a text excerpt,
//! the handling agent's note and the saving mark.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::items::{Drawn, cursor_at};
use super::table::{Column, LEAD, Row, cell, draw_rows, header};
use super::{Place, place, text_width, truncate};
use crate::app::App;
use crate::ledger::{InputRow, Seen};
use crate::model::TaskStatus;
use crate::surface::{HeaderKey, InboxColumn, InboxState, VisibleRow};
use crate::theme::Theme;

/// Draws the Inbox into `area` of `buf`; a `Frame` caller passes `frame.buffer_mut()`.
/// Returns the drawn record rows, the list's scroll offset and how many list rows it drew.
pub(crate) fn render(buf: &mut Buffer, area: Rect, app: &App) -> Drawn {
    render_at(buf, area, app, Instant::now())
}

/// `now` dates the flashes.
fn render_at(buf: &mut Buffer, area: Rect, app: &App, now: Instant) -> Drawn {
    if area.width == 0 || area.height == 0 {
        return Drawn::default();
    }
    let state = &app.inbox;
    let width = usize::from(area.width);
    buf.set_line(area.x, area.y, &facet_line(app, state), area.width);
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    if body.height == 0 {
        return Drawn {
            scroll: state.scroll,
            ..Drawn::default()
        };
    }

    let visible = state.visible();
    if visible.is_empty() {
        let text = match &state.revision {
            Seen::Unread => "reading the input store…",
            _ if state.rows.is_empty() => "no input records yet · n captures a note",
            _ => "no open records · c shows closed",
        };
        buf.set_line(
            body.x,
            body.y,
            &Line::from(Span::styled(text, app.theme.secondary)),
            body.width,
        );
        return Drawn::default();
    }

    let columns = Columns::measure(state, &visible);
    let table = columns.table(width);
    let sorted = TABLE
        .iter()
        .enumerate()
        .find_map(|(at, column)| state.header_sort((*column)?).map(|dir| (at, dir)));
    let headers: Vec<(Rect, HeaderKey)> = header(buf, body, &table, sorted, &app.theme)
        .into_iter()
        .filter_map(|(rect, at)| Some((rect, HeaderKey::Inbox(column_at(at)?))))
        .collect();
    let list = Rect::new(
        body.x,
        body.y + 1,
        body.width,
        body.height.saturating_sub(1),
    );
    if list.height == 0 {
        return Drawn {
            scroll: state.scroll,
            headers,
            ..Drawn::default()
        };
    }
    let view = usize::from(list.height);
    let offset = place(
        state.scroll,
        cursor_at(&visible, state.cursor.as_deref()),
        visible.len(),
        view,
        Place::of_list(state.scroll_pinned),
    );
    let rows: Vec<Row<String>> = visible[offset..]
        .iter()
        .take(view)
        .filter_map(|entry| match entry {
            VisibleRow::Header { label, count } => Some(Row::plain(Line::from(Span::styled(
                format!("▾ {label} ({count})"),
                app.theme.group_header,
            )))),
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
                    target: Some(id.clone()),
                })
            }
        })
        .collect();
    let targets = draw_rows(buf, list, &rows);
    let ids = targets
        .iter()
        .map(|(rect, id)| (cell(*rect, &table, ID_AT), id.clone()))
        .filter(|(rect, _)| rect.width > 0)
        .collect();
    Drawn {
        targets,
        scroll: offset,
        page: rows.len(),
        headers,
        ids,
    }
}

/// The header table's columns in drawn order; `None` is the glyph column, which sorts
/// by nothing. [`Columns::table`], header clicks, sort marks and id cells all index it.
const TABLE: [Option<InboxColumn>; 5] = [
    None,
    Some(InboxColumn::Id),
    Some(InboxColumn::Kind),
    Some(InboxColumn::Status),
    Some(InboxColumn::Text),
];

/// The id column's index in [`TABLE`]; evaluation fails to compile if it is absent.
const ID_AT: usize = {
    let mut at = 0;
    while !matches!(TABLE[at], Some(InboxColumn::Id)) {
        at += 1;
    }
    at
};

/// The sortable column at a header table index; the glyph column has none.
fn column_at(index: usize) -> Option<InboxColumn> {
    TABLE.get(index).copied().flatten()
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

    /// The header's columns: the glyph, id, kind (a question's author when no record
    /// has a kind), status and the text that takes the rest of a `width`-cell row.
    fn table(&self, width: usize) -> Vec<Column> {
        let kind = if self.kind > 0 {
            self.kind
        } else {
            self.author
        };
        let fixed = |column: Option<InboxColumn>| match column {
            None => Some(("", 1)),
            Some(InboxColumn::Id) => Some(("id", self.id)),
            Some(InboxColumn::Kind) => Some(("kind", kind)),
            Some(InboxColumn::Status) => Some(("status", self.status)),
            Some(InboxColumn::Text) => None,
        };
        let used: usize = usize::from(LEAD)
            + TABLE
                .into_iter()
                .filter_map(fixed)
                .map(|(_, w)| if w == 0 { 0 } else { w + 1 })
                .sum::<usize>();
        let text = width.saturating_sub(used + 2);
        TABLE
            .into_iter()
            .map(|column| {
                let (title, width) = fixed(column).unwrap_or(("text", text));
                Column { title, width }
            })
            .collect()
    }
}

/// The cursor mark, two blanks, the glyph and the padded id every row starts with; the
/// second blank is the gap the header's cells leave after the lead.
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
        Span::raw("  "),
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
    push_target(&mut left, row, theme, is_cursor);
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
        is_cursor,
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
    push_target(&mut left, row, theme, is_cursor);
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
        is_cursor,
    )
}

/// Appends the body and its trailer in the room `left` leaves, then the saving mark.
/// The trailer (options or the handled note) takes at most half the room, and the
/// last cell stays blank so a highlight is padded on both sides. On the cursor row the
/// trailer takes `selection_text`.
#[allow(clippy::too_many_arguments)]
fn finish(
    mut left: Vec<Span<'static>>,
    state: &InboxState,
    row: &InputRow,
    theme: &Theme,
    width: usize,
    (body, body_style): (&str, Style),
    trailer: &str,
    is_cursor: bool,
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
        left.push(Span::styled(
            format!("{sep}{trailer}"),
            muted(theme, is_cursor),
        ));
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
fn push_target(left: &mut Vec<Span<'static>>, row: &InputRow, theme: &Theme, is_cursor: bool) {
    let target = target_text(row);
    if !target.is_empty() {
        left.push(Span::raw("  "));
        left.push(Span::styled(target, muted(theme, is_cursor)));
    }
}

/// The dimmed style, lifted to `selection_text` on the cursor row.
fn muted(theme: &Theme, is_cursor: bool) -> Style {
    theme.on_cursor(theme.facet, is_cursor)
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

#[cfg(test)]
mod tests {
    use super::super::items::Target;
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
        let drawn = render_at(&mut buf, area, app, Instant::now());
        let rows = (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        (rows, drawn.targets)
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
        assert!(line.starts_with("   ●"), "the cursor stays on I4: {line:?}");

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
    fn a_list_taller_than_the_pane_draws_the_tail_around_the_cursor() {
        let rows = (1..=9)
            .map(|n| record(&format!("I{n}"), "note", "user", "new"))
            .collect();
        let mut app = inbox_app(rows);
        app.inbox.move_cursor(7);
        assert_eq!(app.inbox.cursor.as_deref(), Some("I8"));
        let (rows, targets) = draw(&app, 60, 6);
        let ids: Vec<&str> = targets.iter().map(|(_, id)| id.as_str()).collect();
        assert_eq!(ids, ["I6", "I7", "I8", "I9"], "the header scrolled off");
        let ys: Vec<u16> = targets.iter().map(|(rect, _)| rect.y).collect();
        assert_eq!(ys, [2, 3, 4, 5]);
        assert!(rows[4].starts_with("▸  ● I8"), "{rows:#?}");
    }

    #[test]
    fn an_empty_store_says_so() {
        let app = inbox_app(Vec::new());
        let (rows, targets) = draw(&app, 60, 4);
        assert_eq!(
            rows[1].trim_end(),
            "no input records yet · n captures a note"
        );
        assert!(targets.is_empty());

        let app = inbox_app(vec![record("I1", "note", "user", "withdrawn")]);
        let (rows, _) = draw(&app, 60, 4);
        assert_eq!(rows[1].trim_end(), "no open records · c shows closed");
    }

    #[test]
    fn a_header_row_titles_the_columns_and_marks_the_sort() {
        let mut app = inbox_app(vec![record("I1", "note", "user", "new")]);
        let (rows, _) = draw(&app, 60, 4);
        assert!(rows[1].contains(" id▲"), "{:?}", rows[1]);
        for title in ["kind", "st…", "text"] {
            assert!(rows[1].contains(title), "{title}: {:?}", rows[1]);
        }
        assert!(
            rows[2].contains("▾ new"),
            "the list starts under it: {rows:#?}"
        );

        app.inbox.cycle_header(InboxColumn::Kind);
        let (rows, _) = draw(&app, 60, 4);
        assert!(rows[1].contains("kind▲"), "{:?}", rows[1]);
        assert!(!rows[1].contains("id▲"), "{:?}", rows[1]);
    }

    #[test]
    fn header_and_id_cells_are_handed_back_for_clicks() {
        let app = inbox_app(vec![
            record("I1", "note", "user", "new"),
            record("I2", "note", "user", "new"),
        ]);
        let area = Rect::new(0, 0, 60, 6);
        let mut buf = Buffer::empty(area);
        let drawn = render_at(&mut buf, area, &app, Instant::now());
        let keys: Vec<HeaderKey> = drawn.headers.iter().map(|(_, key)| *key).collect();
        assert_eq!(
            keys,
            InboxColumn::ALL.map(HeaderKey::Inbox),
            "the glyph column sorts by nothing"
        );
        assert!(drawn.headers.iter().all(|(rect, _)| rect.y == 1));
        let (id_header, _) = drawn
            .headers
            .iter()
            .find(|(_, key)| *key == HeaderKey::Inbox(InboxColumn::Id))
            .expect("id header");
        let ids: Vec<&str> = drawn.ids.iter().map(|(_, id)| id.as_str()).collect();
        assert_eq!(ids, ["I1", "I2"]);
        for ((rect, id), (row, _)) in drawn.ids.iter().zip(&drawn.targets) {
            assert_eq!((rect.x, rect.width), (id_header.x, id_header.width));
            assert_eq!(rect.y, row.y);
            let text: String = (rect.x..rect.right())
                .map(|x| buf[(x, rect.y)].symbol())
                .collect();
            assert_eq!(&text, id);
        }
    }

    #[test]
    fn every_column_maps_to_one_header_cell_and_back() {
        for column in InboxColumn::ALL {
            let at: Vec<usize> = (0..TABLE.len())
                .filter(|at| column_at(*at) == Some(column))
                .collect();
            assert_eq!(at.len(), 1, "{column:?} at {at:?}");
            assert_eq!(column_at(at[0]), Some(column));
        }
        assert_eq!(column_at(ID_AT), Some(InboxColumn::Id));
        let table = Columns {
            id: 2,
            kind: 4,
            status: 3,
            author: 0,
        }
        .table(60);
        assert_eq!(table.len(), TABLE.len());
        assert_eq!(table[ID_AT].title, "id");
        let titled = (0..TABLE.len()).filter(|at| column_at(*at).is_some());
        for at in titled {
            let column = column_at(at).expect("sortable");
            assert_eq!(table[at].title, column.label(), "{column:?}");
        }
        assert_eq!(column_at(TABLE.len()), None);
    }

    #[test]
    fn the_cursor_row_lifts_its_dimmed_spans() {
        let row = InputRow {
            ledger: "review".to_string(),
            items: vec!["R3".to_string()],
            ..record("I1", "note", "user", "new")
        };
        let app = inbox_app(vec![row, record("I2", "note", "user", "new")]);
        let area = Rect::new(0, 0, 60, 5);
        let mut buf = Buffer::empty(area);
        render_at(&mut buf, area, &app, Instant::now());
        let line: String = (0..60).map(|x| buf[(x, 3)].symbol()).collect();
        let at = line.find("review R3").expect("target");
        let x = u16::try_from(line[..at].chars().count()).expect("column");
        assert_eq!(Some(buf[(x, 3)].fg), app.theme.selection_text.fg);
    }
}
