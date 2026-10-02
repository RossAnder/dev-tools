//! An item surface: the filtered, grouped and sorted list of ledger rows with marks, flashes and saving state.
//!
//! A facet row naming the group-by, sort, closed toggle and filter sits above the list.
//! Each item row reads `▸● ○ R12 warning category effort summary… ↻ ID file:line`:
//! cursor, mark, status glyph, id, severity (or backlog kind), category, effort and
//! summary, then the saving mark, a running agent's chip and the dimmed anchor.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::ledger::{Anchor, ItemRow, StatusClass};
use crate::model::{AgentStatus, TaskStatus};
use crate::surface::{ItemsState, VisibleRow};

/// An item row's on-screen rect and the id it shows, for mouse hits.
pub(crate) type Target = (Rect, String);

/// Draws the current item surface into `area` of `buf`; a `Frame` caller passes
/// `frame.buffer_mut()`. Returns the drawn item rows.
pub(crate) fn render(buf: &mut Buffer, area: Rect, app: &App) -> Vec<Target> {
    render_at(buf, area, app, Instant::now())
}

/// `now` dates the flashes.
fn render_at(buf: &mut Buffer, area: Rect, app: &App, now: Instant) -> Vec<Target> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let Some(state) = app.current_items() else {
        return Vec::new();
    };
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
            None => "reading the ledger…",
            Some(None) => "no ledger file",
            Some(Some(_)) if state.rows.is_empty() => "no items",
            Some(Some(_)) => "no items match",
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
    let at = rows
        .iter()
        .position(|row| row.id.is_some() && row.id == state.cursor);
    let offset = window(at, rows.len(), usize::from(list.height));
    draw_rows(buf, list, &rows[offset..])
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

/// The first of `view` rows drawn from `len`: from the top until the cursor `at` would
/// come within a third of the window of its bottom edge, then as little scrolled as
/// keeps that third showing below it.
fn window(at: Option<usize>, len: usize, view: usize) -> usize {
    if view == 0 || len <= view {
        return 0;
    }
    let ahead = view / 3;
    let start = at.map_or(0, |at| (at + ahead + 1).saturating_sub(view));
    start.min(len - view)
}

/// `group severity · sort id · closed hidden · /cache`; the filter shows only when set.
fn facet_line(app: &App, state: &ItemsState) -> Line<'static> {
    let closed = if state.show_closed { "shown" } else { "hidden" };
    let mut text = format!(
        "group {} · sort {} · closed {closed}",
        state.group.label(),
        state.sort.label()
    );
    if !state.filter.is_empty() {
        text.push_str(&format!(" · /{}", state.filter));
    }
    Line::from(Span::styled(text, app.theme.facet))
}

/// Widths of the aligned columns over the drawn rows; a column no row fills is zero
/// and drawn not at all.
struct Columns {
    id: usize,
    class: usize,
    category: usize,
    effort: usize,
}

impl Columns {
    fn measure(state: &ItemsState, visible: &[VisibleRow]) -> Columns {
        let mut columns = Columns {
            id: 0,
            class: 0,
            category: 0,
            effort: 0,
        };
        for row in visible.iter().filter_map(|entry| match entry {
            VisibleRow::Item(id) => state.row(id),
            VisibleRow::Header { .. } => None,
        }) {
            columns.id = columns.id.max(text_width(&row.id));
            columns.class = columns.class.max(text_width(classifier(row).0));
            columns.category = columns.category.max(text_width(&row.category));
            columns.effort = columns.effort.max(text_width(&row.effort));
        }
        columns
    }
}

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
    let mut left = vec![
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
        Span::raw(" "),
        Span::styled(glyph(row.class), theme.item_status(class_name(row.class))),
        Span::raw(format!(" {:<w$}", row.id, w = columns.id)),
    ];
    let (class, is_kind) = classifier(row);
    let class_style = if is_kind {
        theme.facet
    } else {
        theme.severity(class)
    };
    for (text, w, style) in [
        (class, columns.class, class_style),
        (row.category.as_str(), columns.category, theme.facet),
        (row.effort.as_str(), columns.effort, theme.effort),
    ] {
        if w > 0 {
            left.push(Span::raw(" "));
            left.push(Span::styled(format!("{text:<w$}"), style));
        }
    }
    left.push(Span::raw(" "));

    let mut right = Vec::new();
    if state.saving.contains(&row.id) {
        right.push(Span::styled("↻", theme.item_saving));
    }
    if let Some(chip) = agent_chip(app, &row.id) {
        if !right.is_empty() {
            right.push(Span::raw(" "));
        }
        right.push(chip);
    }
    let anchor = anchor_text(&row.anchor);
    let used = spans_width(&left) + spans_width(&right);
    // The anchor gives way before the summary drops under a dozen cells.
    if !anchor.is_empty() && used + text_width(&anchor) + 14 <= width {
        if !right.is_empty() {
            right.push(Span::raw(" "));
        }
        right.push(Span::styled(anchor, theme.facet));
    }
    // The last cell stays blank, so a highlight is padded on both sides.
    let room = width.saturating_sub(spans_width(&left) + spans_width(&right) + 1);
    let room = room.saturating_sub(usize::from(!right.is_empty()));
    let summary = truncate(&row.summary, room);
    let pad = room.saturating_sub(text_width(&summary));
    left.push(Span::raw(summary));
    left.push(Span::raw(" ".repeat(pad)));
    if !right.is_empty() {
        left.push(Span::raw(" "));
        left.extend(right);
    }
    left.push(Span::raw(" "));
    Line::from(left)
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

/// `implement-deep` gives `ID`, `Explore` gives `E`; at most two letters.
fn initials(agent_type: &str) -> String {
    let letters: String = agent_type
        .split(['-', '_', ' '])
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .take(2)
        .collect();
    if letters.is_empty() {
        "?".to_string()
    } else {
        letters
    }
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
            path: "review-ledger.toml".to_string(),
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
    fn the_list_scrolls_to_keep_the_cursor_on_screen() {
        let mut app = review_app();
        app.apply(Action::ItemMove(crate::app::Dir::Down));
        app.apply(Action::ItemMove(crate::app::Dir::Down));
        let (rows, targets) = draw(&app, 100, 3);
        assert_eq!(targets.len(), 2, "two list rows under the facet row");
        assert!(rows[2].starts_with("▸"), "{rows:#?}");
        assert_eq!(window(Some(9), 10, 4), 6);
        assert_eq!(window(Some(1), 10, 4), 0);
    }
}
