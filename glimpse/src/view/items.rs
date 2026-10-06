//! An item surface: the filtered, grouped and sorted list of ledger rows with marks, flashes and saving state.
//!
//! A facet row naming the group-by, sort, closed toggle and filter sits above the list.
//! Each item row reads `▸● ○ R12 warning category effort summary… ↻ ID  file:line`:
//! cursor and mark, then the `item_columns` in their configured order — by default
//! status glyph, id, severity (or backlog kind), category, effort, the summary with the
//! saving mark, pending-input mark and a running agent's chip at its right end, and the
//! anchor in a fixed-width column. The summary takes the width the others leave; the
//! anchor column narrows, then drops, before the summary falls under [`SUMMARY_MIN`].

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use tui_input::Input;

use super::{Place, initials, place, text_width, truncate};
use crate::actions::Overlay;
use crate::app::App;
use crate::config::ItemColumn;
use crate::ledger::{Anchor, ItemRow, Seen, StatusClass};
use crate::model::{AgentStatus, TaskStatus};
use crate::surface::{ItemsState, VisibleRow};

/// An item row's on-screen rect and the id it shows, for mouse hits.
pub(crate) type Target = (Rect, String);

/// The widest the anchor column grows, in cells.
const AREA_MAX: usize = 28;
/// The anchor column is dropped rather than drawn narrower than this.
const AREA_MIN: usize = 10;
/// The summary cells the anchor column gives way to.
const SUMMARY_MIN: usize = 20;

/// Draws the current item surface into `area` of `buf`; a `Frame` caller passes
/// `frame.buffer_mut()`. Returns the drawn item rows and the list's scroll offset.
pub(crate) fn render(buf: &mut Buffer, area: Rect, app: &App) -> (Vec<Target>, usize) {
    render_at(buf, area, app, Instant::now())
}

/// `now` dates the flashes.
fn render_at(buf: &mut Buffer, area: Rect, app: &App, now: Instant) -> (Vec<Target>, usize) {
    if area.width == 0 || area.height == 0 {
        return (Vec::new(), 0);
    }
    let Some(state) = app.current_items() else {
        return (Vec::new(), 0);
    };
    let width = usize::from(area.width);
    buf.set_line(area.x, area.y, &facet_line(app, state), area.width);
    if let Some(input) = prompt_input(app) {
        let field = prompt_field(app, state, area);
        if field.width > 0 {
            let scroll = input.visual_scroll(usize::from(field.width.saturating_sub(1)));
            Paragraph::new(input.value())
                .style(app.theme.facet)
                .scroll((0, u16::try_from(scroll).unwrap_or(u16::MAX)))
                .render(field, buf);
        }
    }
    let list = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    if list.height == 0 {
        return (Vec::new(), state.scroll);
    }

    let visible = state.visible();
    if visible.is_empty() {
        let text = match &state.revision {
            Seen::Unread => "reading the ledger…",
            Seen::Missing => "no ledger file",
            Seen::At(_) if state.rows.is_empty() => "no items",
            Seen::At(_) => "no items match",
        };
        buf.set_line(
            list.x,
            list.y,
            &Line::from(Span::styled(text, app.theme.secondary)),
            list.width,
        );
        return (Vec::new(), 0);
    }

    let mut columns = Columns::measure(state, &visible);
    columns.fit(&app.item_columns, width);
    let view = usize::from(list.height);
    let offset = place(
        state.scroll,
        cursor_at(&visible, state.cursor.as_deref()),
        visible.len(),
        view,
        Place::of_list(state.scroll_pinned),
    );
    let rows: Vec<Row> = visible[offset..]
        .iter()
        .take(view)
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
                Some(Row {
                    line: item_line(app, state, row, &columns, width, is_cursor),
                    fill: is_cursor.then_some(app.theme.selection),
                    overlay: (!is_cursor && state.is_flashing(id, now))
                        .then(|| app.theme.flash(flash_status(row.class))),
                    id: Some(id.clone()),
                })
            }
        })
        .collect();
    (draw_rows(buf, list, &rows), offset)
}

/// The position in `visible` of the item row `cursor` names.
pub(crate) fn cursor_at(visible: &[VisibleRow], cursor: Option<&str>) -> Option<usize> {
    let cursor = cursor?;
    visible
        .iter()
        .position(|entry| matches!(entry, VisibleRow::Item(id) if id == cursor))
}

/// A drawn row, the styles laid under and over its full width, and the item it shows.
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
        // The fill goes under the spans so a chip keeps its own background.
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

/// `group severity · sort id · closed hidden · /cache`; the filter shows only when set.
/// While the filter prompt is open the row ends in `/` and the prompt draws after it.
fn facet_line(app: &App, state: &ItemsState) -> Line<'static> {
    let closed = if state.show_closed { "shown" } else { "hidden" };
    let mut text = format!(
        "group {} · sort {} · closed {closed}",
        state.group.label(),
        state.sort.label()
    );
    if prompt_input(app).is_some() {
        text.push_str(" · /");
    } else if !state.filter.is_empty() {
        text.push_str(&format!(" · /{}", state.filter));
    }
    Line::from(Span::styled(text, app.theme.facet))
}

fn prompt_input(app: &App) -> Option<&Input> {
    match &app.overlay {
        Some(Overlay::Prompt { input, .. }) => Some(input),
        _ => None,
    }
}

/// The cells after the facet text that the filter prompt's input fills.
fn prompt_field(app: &App, state: &ItemsState, area: Rect) -> Rect {
    let used = u16::try_from(facet_line(app, state).width()).unwrap_or(u16::MAX);
    let x = area.x.saturating_add(used).min(area.right());
    Rect::new(x, area.y, area.right() - x, 1)
}

/// Where the terminal cursor goes while the filter prompt is open on `area`.
pub(crate) fn prompt_cursor(area: Rect, app: &App) -> Option<Position> {
    let input = prompt_input(app)?;
    let state = app.current_items()?;
    let field = prompt_field(app, state, area);
    if field.width == 0 || area.height == 0 {
        return None;
    }
    let scroll = input.visual_scroll(usize::from(field.width.saturating_sub(1)));
    let col = u16::try_from(input.visual_cursor().saturating_sub(scroll)).unwrap_or(u16::MAX);
    Some(Position::new(
        field.x.saturating_add(col).min(field.right() - 1),
        field.y,
    ))
}

/// Widths of the aligned columns over the visible rows; a column no row fills is zero
/// and drawn not at all.
struct Columns {
    id: usize,
    class: usize,
    category: usize,
    effort: usize,
    area: usize,
}

impl Columns {
    fn measure(state: &ItemsState, visible: &[VisibleRow]) -> Columns {
        let mut columns = Columns {
            id: 0,
            class: 0,
            category: 0,
            effort: 0,
            area: 0,
        };
        for row in visible.iter().filter_map(|entry| match entry {
            VisibleRow::Item(id) => state.row(id),
            VisibleRow::Header { .. } => None,
        }) {
            columns.id = columns.id.max(text_width(&row.id));
            columns.class = columns.class.max(text_width(classifier(row).0));
            columns.category = columns.category.max(text_width(&row.category));
            columns.effort = columns.effort.max(text_width(&row.effort));
            columns.area = columns.area.max(text_width(&anchor_text(&row.anchor)));
        }
        columns
    }

    /// Sizes the anchor column for a `width`-cell row: at most [`AREA_MAX`], narrowed
    /// to keep [`SUMMARY_MIN`] summary cells, and dropped once under [`AREA_MIN`].
    fn fit(&mut self, order: &[ItemColumn], width: usize) {
        let natural = self.area;
        self.area = 0;
        if natural == 0 || !order.contains(&ItemColumn::Area) {
            return;
        }
        let others: usize = order
            .iter()
            .map(|column| match column {
                ItemColumn::Area => 0,
                ItemColumn::Summary => 1 + SUMMARY_MIN,
                other => match self.width(*other) {
                    0 => 0,
                    w => w + 1,
                },
            })
            .sum();
        let spare = width.saturating_sub(LEAD + others + 1 + 1);
        let area = natural.min(AREA_MAX).min(spare);
        if area >= natural.min(AREA_MIN) {
            self.area = area;
        }
    }

    /// A fixed column's width; the summary's is whatever the row has left.
    fn width(&self, column: ItemColumn) -> usize {
        match column {
            ItemColumn::Status => 1,
            ItemColumn::Id => self.id,
            ItemColumn::Kind => self.class,
            ItemColumn::Category => self.category,
            ItemColumn::Effort => self.effort,
            ItemColumn::Area => self.area,
            ItemColumn::Summary => 0,
        }
    }
}

/// The cursor and mark cells every row starts with.
const LEAD: usize = 2;

/// The severity of a finding, or the kind of a backlog row, and whether it is the kind.
fn classifier(row: &ItemRow) -> (&str, bool) {
    if row.kind.is_empty() {
        (&row.severity, false)
    } else {
        (&row.kind, true)
    }
}

fn item_line(
    app: &App,
    state: &ItemsState,
    row: &ItemRow,
    columns: &Columns,
    width: usize,
    is_cursor: bool,
) -> Line<'static> {
    let theme = &app.theme;
    let order = &app.item_columns;
    let mut spans = vec![
        if is_cursor {
            Span::styled("▸", theme.selection_mark)
        } else {
            Span::raw(" ")
        },
        if state.marks.contains(&row.id) {
            Span::styled("●", theme.item_mark)
        } else {
            Span::raw(" ")
        },
    ];
    let marks = row_marks(app, state, &row.id);
    let has_summary = order.contains(&ItemColumn::Summary);
    // The last cell stays blank, so a highlight is padded on both sides.
    let fixed: usize = LEAD
        + 1
        + order
            .iter()
            .map(|column| match columns.width(*column) {
                _ if *column == ItemColumn::Summary => 1,
                0 => 0,
                w => w + 1,
            })
            .sum::<usize>();
    for column in order {
        if *column == ItemColumn::Summary {
            spans.push(Span::raw(" "));
            spans.extend(summary_cell(
                row,
                marks.clone(),
                width.saturating_sub(fixed),
            ));
            continue;
        }
        let w = columns.width(*column);
        if w > 0 {
            spans.push(Span::raw(" "));
            spans.push(cell(app, row, *column, w));
        }
    }
    if !has_summary && !marks.is_empty() {
        spans.push(Span::raw(" "));
        spans.extend(marks);
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// One fixed column's cell, padded to `w`.
fn cell(app: &App, row: &ItemRow, column: ItemColumn, w: usize) -> Span<'static> {
    let theme = &app.theme;
    let padded = |text: &str| format!("{text}{}", " ".repeat(w.saturating_sub(text_width(text))));
    match column {
        ItemColumn::Status => {
            Span::styled(glyph(row.class), theme.item_status(class_name(row.class)))
        }
        ItemColumn::Id => Span::styled(padded(&row.id), theme.item_id),
        ItemColumn::Kind => match classifier(row) {
            (kind, true) => Span::styled(padded(kind), theme.item_kind),
            (severity, false) => Span::styled(padded(severity), theme.severity(severity)),
        },
        ItemColumn::Category => Span::styled(padded(&row.category), theme.facet),
        ItemColumn::Effort => Span::styled(padded(&row.effort), theme.effort),
        ItemColumn::Area => Span::styled(
            padded(&shorten_path(&anchor_text(&row.anchor), w)),
            theme.facet,
        ),
        ItemColumn::Summary => Span::raw(String::new()),
    }
}

/// The summary in `room` cells, cut with `…` to leave the row's `marks` right-aligned
/// at the cell's end.
fn summary_cell(row: &ItemRow, marks: Vec<Span<'static>>, room: usize) -> Vec<Span<'static>> {
    let reserved = if marks.is_empty() {
        0
    } else {
        spans_width(&marks) + 1
    };
    let summary = truncate(&row.summary, room.saturating_sub(reserved));
    let pad = room.saturating_sub(text_width(&summary) + reserved);
    let mut spans = vec![Span::raw(summary), Span::raw(" ".repeat(pad))];
    if !marks.is_empty() {
        spans.push(Span::raw(" "));
        spans.extend(marks);
    }
    spans
}

/// The saving mark, the pending-input mark and a running agent's chip, space-separated.
fn row_marks(app: &App, state: &ItemsState, id: &str) -> Vec<Span<'static>> {
    let saving = state
        .saving
        .contains(id)
        .then(|| Span::styled("↻", app.theme.item_saving));
    let mut spans = Vec::new();
    for mark in [saving, pending_mark(app, id), agent_chip(app, id)]
        .into_iter()
        .flatten()
    {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(mark);
    }
    spans
}

/// `text` in at most `max` cells. A path keeps its tail behind a leading `…`, since the
/// file name says more than the directories above it; other text is cut at the end.
fn shorten_path(text: &str, max: usize) -> String {
    if text_width(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let is_separator = |c: char| matches!(c, '/' | '\\');
    if !text.contains(is_separator) {
        return truncate(text, max);
    }
    let mut start = text.len();
    let mut used = 0;
    for (i, c) in text.char_indices().rev() {
        let w = text_width(c.encode_utf8(&mut [0; 4]));
        if used + w > max - 1 {
            break;
        }
        used += w;
        start = i;
    }
    format!("…{}", text[start..].trim_start_matches(is_separator))
}

/// `file:line` for a finding, the heading or area otherwise.
fn anchor_text(anchor: &Anchor) -> String {
    match anchor {
        Anchor::Code { file, line, .. } if *line > 0 => format!("{file}:{line}"),
        Anchor::Code { file, .. } => file.clone(),
        Anchor::Section(text) | Anchor::Area(text) => text.clone(),
        Anchor::None => String::new(),
    }
}

/// `✉`, coloured by the newest pending input record that names `id`.
fn pending_mark(app: &App, id: &str) -> Option<Span<'static>> {
    let kind = app.surface.ledger_kind()?;
    let newest = *app.inbox.pending_for(kind, id).last()?;
    let style = if newest.status == "new" {
        app.theme.input_new
    } else {
        app.theme.input_acknowledged
    };
    Some(Span::styled("✉", style))
}

/// The type initials of a running agent whose dispatch names `id`, dimmed once its
/// transcript has gone quiet.
fn agent_chip(app: &App, id: &str) -> Option<Span<'static>> {
    let agent = app
        .index
        .agents_for_item(&app.snapshot, id)
        .into_iter()
        .find(|agent| agent.status == AgentStatus::Running)?;
    let style = if app.stale_agents.contains(&agent.id) {
        app.theme.agent_chip_stale
    } else {
        app.theme.agent_chip
    };
    Some(super::chip(&initials(&agent.agent_type), style))
}

pub(crate) fn glyph(class: StatusClass) -> &'static str {
    match class {
        StatusClass::Live => "○",
        StatusClass::Parked => "⏸",
        StatusClass::Done => "✓",
        StatusClass::Declined => "✗",
    }
}

/// The key [`crate::theme::Theme::item_status`] takes.
pub(crate) fn class_name(class: StatusClass) -> &'static str {
    match class {
        StatusClass::Live => "live",
        StatusClass::Parked => "parked",
        StatusClass::Done => "done",
        StatusClass::Declined => "declined",
    }
}

/// The task status whose flash colour matches the class's glyph colour.
fn flash_status(class: StatusClass) -> TaskStatus {
    match class {
        StatusClass::Live => TaskStatus::InProgress,
        StatusClass::Parked => TaskStatus::Deferred,
        StatusClass::Done => TaskStatus::Done,
        StatusClass::Declined => TaskStatus::Pending,
    }
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Action;
    use crate::config::Config;
    use crate::ledger::{Kind, Ledger};
    use crate::model::fixture;
    use crate::surface::Surface;

    fn finding(id: &str, severity: &str, summary: &str, file: &str, line: u64) -> ItemRow {
        ItemRow {
            id: id.to_string(),
            status: "open".to_string(),
            class: StatusClass::Live,
            severity: severity.to_string(),
            category: "correctness".to_string(),
            effort: "small".to_string(),
            summary: summary.to_string(),
            anchor: Anchor::Code {
                file: file.to_string(),
                line,
                symbol: String::new(),
            },
            ..ItemRow::default()
        }
    }

    fn review_app() -> App {
        let mut app = App::new(fixture(), &Config::default());
        let rows = vec![
            finding("R1", "warning", "Lock held across await", "src/a.rs", 12),
            finding("R2", "critical", "Path escapes the root", "src/b.rs", 40),
            finding("R3", "warning", "Unbounded read", "src/c.rs", 7),
        ];
        let ledger = Ledger {
            kind: Kind::Review,
            source: tomlctl::LedgerRef::File("review-ledger.toml".into()),
            revision: Some("v1".to_string()),
            rows,
        };
        app.apply_ledger(ledger, Instant::now());
        app.apply(Action::SwitchSurface(Surface::Review));
        app
    }

    fn draw(app: &App, width: u16, height: u16) -> (Vec<String>, Vec<Target>) {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let (targets, _) = render_at(&mut buf, area, app, Instant::now());
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
    fn the_review_surface_lists_rows_grouped_by_severity() {
        let mut app = review_app();
        app.apply(Action::CycleGroup);
        let (rows, targets) = draw(&app, 100, 12);
        assert!(rows[0].contains("group severity · sort id · closed hidden"));
        let (critical, _) = row_of(&rows, "▾ critical (1)");
        let (warning, _) = row_of(&rows, "▾ warning (2)");
        let (r2, line) = row_of(&rows, "R2");
        assert!(critical < r2 && r2 < warning, "{rows:#?}");
        assert!(line.contains("○ R2 critical correctness small Path escapes the root"));
        assert!(line.trim_end().ends_with("src/b.rs:40"), "{line:?}");
        let (r1, _) = row_of(&rows, "R1");
        let (r3, _) = row_of(&rows, "R3");
        assert!(warning < r1 && r1 < r3);
        let ids: Vec<&str> = targets.iter().map(|(_, id)| id.as_str()).collect();
        assert_eq!(ids, ["R2", "R1", "R3"]);
        assert_eq!(targets[0].0.y, u16::try_from(r2).expect("row"));
    }

    #[test]
    fn the_filter_prompt_renders_in_the_facet_row() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent};

        let mut app = review_app();
        app.apply(Action::OpenFilter);
        for c in "R3".chars() {
            app.overlay_key(KeyEvent::from(KeyCode::Char(c)));
        }
        let (rows, _) = draw(&app, 60, 6);
        assert!(
            rows[0].starts_with("group none · sort id · closed hidden · /R3"),
            "{:?}",
            rows[0]
        );
        let at = prompt_cursor(Rect::new(0, 0, 60, 6), &app).expect("cursor");
        assert_eq!((at.x, at.y), (42, 0), "after the typed text");

        let (narrow, _) = draw(&app, 42, 6);
        assert!(narrow[0].trim_end().ends_with("/3"), "{:?}", narrow[0]);
        let at = prompt_cursor(Rect::new(0, 0, 42, 6), &app).expect("cursor");
        assert_eq!(at.x, 41, "kept inside the row");

        app.apply(Action::Back);
        assert!(prompt_cursor(Rect::new(0, 0, 60, 6), &app).is_none());
    }

    #[test]
    fn a_marked_row_shows_its_mark() {
        let mut app = review_app();
        app.apply(Action::ToggleMark);
        let (rows, _) = draw(&app, 100, 8);
        let (_, r1) = row_of(&rows, "R1");
        let (_, r2) = row_of(&rows, "R2");
        assert!(r1.starts_with(" ● ○ R1"), "{r1:?}");
        assert!(r2.starts_with("▸  ○ R2"), "the cursor advanced: {r2:?}");
    }

    #[test]
    fn a_saving_row_shows_the_saving_mark() {
        let mut app = review_app();
        if let Some(state) = app.items.get_mut(&Surface::Review) {
            state.saving.insert("R3".to_string());
        }
        let (rows, _) = draw(&app, 100, 8);
        assert!(row_of(&rows, "R3").1.contains("↻ src/c.rs:7"));
        assert!(!row_of(&rows, "R1").1.contains('↻'));
    }

    #[test]
    fn a_row_with_a_pending_request_shows_the_mark() {
        use crate::ledger::InputRow;

        let mut app = review_app();
        let request = |id: &str, status: &str| InputRow {
            id: id.to_string(),
            kind: "request".to_string(),
            status: status.to_string(),
            ledger: "review".to_string(),
            items: vec!["R3".to_string()],
            ..InputRow::default()
        };
        app.inbox.apply_inputs(
            vec![request("I1", "new"), request("I2", "handled")],
            Some("v1".to_string()),
            true,
            Instant::now(),
        );
        let (rows, _) = draw(&app, 100, 8);
        assert!(row_of(&rows, "R3").1.contains("✉ src/c.rs:7"));
        assert!(!row_of(&rows, "R1").1.contains('✉'));
    }

    #[test]
    fn the_list_scrolls_to_keep_the_cursor_on_screen() {
        let mut app = review_app();
        app.apply(Action::ItemMove(crate::app::Dir::Down));
        app.apply(Action::ItemMove(crate::app::Dir::Down));
        let (rows, targets) = draw(&app, 100, 3);
        assert_eq!(targets.len(), 2, "two list rows under the facet row");
        assert!(rows[2].starts_with("▸"), "{rows:#?}");
    }

    fn backlog_app(rows: Vec<ItemRow>) -> App {
        let mut app = App::new(fixture(), &Config::default());
        app.apply_ledger(
            Ledger {
                kind: Kind::Backlog,
                source: tomlctl::LedgerRef::File(".claude/backlog.toml".into()),
                revision: Some("v1".to_string()),
                rows,
            },
            Instant::now(),
        );
        app.apply(Action::SwitchSurface(Surface::Backlog));
        app
    }

    fn capture(id: &str, kind: &str, summary: &str, area: &str) -> ItemRow {
        ItemRow {
            id: id.to_string(),
            status: "open".to_string(),
            class: StatusClass::Live,
            kind: kind.to_string(),
            summary: summary.to_string(),
            anchor: Anchor::Area(area.to_string()),
            ..ItemRow::default()
        }
    }

    #[test]
    fn the_area_sits_in_an_aligned_column_cut_from_the_left() {
        let app = backlog_app(vec![
            capture(
                "B-2cc7096a",
                "debt",
                "glimpse's write-root conflict check covers only TOML ledgers",
                "glimpse/src/writer.rs",
            ),
            capture(
                "B-778163da",
                "annoyance",
                "Borrowed-parse read paths report parse and range errors",
                "tomlctl/src/io.rs",
            ),
            capture(
                "B-ada06976",
                "debt",
                "The apply-pipeline skill body sits exactly at the cap",
                "claude/skills/flow-contract-apply-pipeline/SKILL.md",
            ),
        ]);
        let (rows, _) = draw(&app, 90, 5);
        let start = |needle: &str| {
            let (_, line) = row_of(&rows, needle);
            let at = line.find(needle).expect("drawn");
            line[..at].chars().count()
        };
        let writer = start("glimpse/src/writer.rs");
        assert_eq!(start("tomlctl/src/io.rs"), writer, "{rows:#?}");
        assert_eq!(start("…act-apply-pipeline/SKILL.md"), writer);
        let (_, line) = row_of(&rows, "B-778163da");
        assert!(
            line.starts_with("▸  ○ B-778163da annoyance Borrowed-parse")
                || line.starts_with("   ○ B-778163da annoyance Borrowed-parse"),
            "{line:?}"
        );
        assert!(line.contains('…'), "the summary gives way: {line:?}");

        let (narrow, _) = draw(&app, 50, 5);
        assert!(
            !narrow.iter().any(|row| row.contains("io.rs")),
            "a narrow pane drops the area: {narrow:#?}"
        );
    }

    #[test]
    fn ids_and_kinds_take_their_dimmed_tokens() {
        let app = backlog_app(vec![capture("B-1", "debt", "Summary", "src/a.rs")]);
        let area = Rect::new(0, 0, 60, 3);
        let mut buf = Buffer::empty(area);
        render_at(&mut buf, area, &app, Instant::now());
        let line: String = (0..60).map(|x| buf[(x, 1)].symbol()).collect();
        let col = |needle: &str| {
            let at = line.find(needle).expect("drawn");
            u16::try_from(line[..at].chars().count()).expect("column")
        };
        assert_eq!(Some(buf[(col("B-1"), 1)].fg), app.theme.item_id.fg);
        assert_eq!(Some(buf[(col("debt"), 1)].fg), app.theme.item_kind.fg);
    }

    #[test]
    fn configured_columns_choose_the_set_and_order() {
        let mut app = backlog_app(vec![capture("B-1", "debt", "Summary", "src/a.rs")]);
        app.item_columns = vec![ItemColumn::Kind, ItemColumn::Id, ItemColumn::Summary];
        let (rows, _) = draw(&app, 60, 3);
        let (_, line) = row_of(&rows, "B-1");
        assert!(line.starts_with("▸  debt B-1 Summary"), "{line:?}");
        assert!(
            !line.contains("src/a.rs") && !line.contains('○'),
            "{line:?}"
        );
    }

    #[test]
    fn paths_keep_their_trailing_segments() {
        assert_eq!(shorten_path("src/writer.rs", 20), "src/writer.rs");
        assert_eq!(shorten_path("glimpse/src/writer.rs", 15), "…src/writer.rs");
        assert_eq!(shorten_path("glimpse/src/writer.rs", 12), "…c/writer.rs");
        assert_eq!(shorten_path("a/very-long-file-name.rs", 8), "…name.rs");
        assert_eq!(shorten_path("Approach and risks", 10), "Approach …");
        assert_eq!(shorten_path("src/a.rs", 0), "");
    }

    #[test]
    fn the_wheel_scrolls_the_list_without_moving_the_cursor() {
        let rows = (1..=9)
            .map(|n| capture(&format!("B-{n}"), "debt", "Summary", "src/a.rs"))
            .collect();
        let mut app = backlog_app(rows);
        // One frame, writing the offset back as `view::render` does.
        let frame = |app: &mut App| -> Vec<String> {
            let area = Rect::new(0, 0, 60, 5);
            let (targets, offset) = render_at(&mut Buffer::empty(area), area, app, Instant::now());
            app.items
                .get_mut(&Surface::Backlog)
                .expect("backlog")
                .scroll = offset;
            targets.into_iter().map(|(_, id)| id).collect()
        };
        app.apply(Action::ScrollView(0, 3));
        assert_eq!(frame(&mut app), ["B-4", "B-5", "B-6", "B-7"]);
        app.apply(Action::ScrollView(0, 30));
        assert_eq!(frame(&mut app), ["B-6", "B-7", "B-8", "B-9"], "clamped");
        let state = &app.items[&Surface::Backlog];
        assert_eq!(state.cursor.as_deref(), Some("B-1"), "the cursor stays");
        assert_eq!(state.scroll, 5);

        app.apply(Action::ItemMove(crate::app::Dir::Down));
        let ids = frame(&mut app);
        assert_eq!(
            ids.first().map(String::as_str),
            Some("B-1"),
            "a cursor move brings it back: {ids:?}"
        );
    }

    #[test]
    fn a_list_taller_than_the_pane_draws_the_tail_around_the_cursor() {
        let mut app = App::new(fixture(), &Config::default());
        let rows = (1..=9)
            .map(|n| finding(&format!("R{n}"), "warning", "Summary", "src/a.rs", n))
            .collect();
        app.apply_ledger(
            Ledger {
                kind: Kind::Review,
                source: tomlctl::LedgerRef::File("review-ledger.toml".into()),
                revision: Some("v1".to_string()),
                rows,
            },
            Instant::now(),
        );
        app.apply(Action::SwitchSurface(Surface::Review));
        for _ in 0..7 {
            app.apply(Action::ItemMove(crate::app::Dir::Down));
        }
        let (rows, targets) = draw(&app, 80, 5);
        let ids: Vec<&str> = targets.iter().map(|(_, id)| id.as_str()).collect();
        assert_eq!(ids, ["R6", "R7", "R8", "R9"]);
        let ys: Vec<u16> = targets.iter().map(|(rect, _)| rect.y).collect();
        assert_eq!(ys, [1, 2, 3, 4]);
        assert!(rows[3].starts_with("▸  ○ R8"), "{rows:#?}");
        assert!(rows[4].contains("R9 warning"), "{rows:#?}");
    }
}
