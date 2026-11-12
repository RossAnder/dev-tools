//! The Plan surface's list: a v2 flow's records in fixed sections.
//!
//! Each section is drawn as `▾ decisions (2)` and then its rows, or `none` when it has
//! none. A row reads `▸ D1 two-way agent-assumed Read the key names…`: the cursor mark,
//! then the row's cells, aligned within its section; the last cell takes the width the
//! others leave. A flow with no plan store draws one line saying so.

use std::collections::HashMap;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::items::Drawn;
use super::table::{Row, draw_rows};
use super::{Place, place, text_width, truncate};
use crate::app::App;
use crate::model::TaskStatus;
use crate::plan_state::{PlanRow, PlanState, Section};
use crate::surface::VisibleRow;
use crate::theme::Theme;

/// The widest any cell but a row's last is drawn, so one long goal or reason cannot
/// push the rest of its section off the pane.
const CELL_MAX: usize = 24;

/// Draws the Plan surface into `area` of `buf`; a `Frame` caller passes
/// `frame.buffer_mut()`. Returns the drawn rows, the list's scroll offset and how many
/// list rows it drew.
pub(crate) fn render(buf: &mut Buffer, area: Rect, app: &App) -> Drawn {
    render_at(buf, area, app, Instant::now())
}

/// One line of the drawn list.
enum Entry<'a> {
    Heading(&'a str, usize),
    /// The line under the heading of a section with no rows.
    Empty,
    Item(&'a PlanRow),
}

/// `now` dates the flashes.
fn render_at(buf: &mut Buffer, area: Rect, app: &App, now: Instant) -> Drawn {
    if area.width == 0 || area.height == 0 {
        return Drawn::default();
    }
    let state = &app.plan;
    if let Some(text) = state.empty_text() {
        buf.set_line(
            area.x,
            area.y,
            &Line::from(Span::styled(text, app.theme.secondary)),
            area.width,
        );
        return Drawn::default();
    }

    let entries = entries(state);
    let widths = widths(state);
    let cursor = state.cursor.as_deref();
    let at = entries
        .iter()
        .position(|entry| matches!(entry, Entry::Item(row) if Some(row.id.as_str()) == cursor));
    let view = usize::from(area.height);
    let offset = place(
        state.scroll,
        at,
        entries.len(),
        view,
        Place::of_list(state.scroll_pinned),
    );
    let width = usize::from(area.width);
    let theme = &app.theme;
    let rows: Vec<Row<String>> = entries[offset..]
        .iter()
        .take(view)
        .map(|entry| match entry {
            Entry::Heading(label, count) => Row::plain(Line::from(Span::styled(
                format!("▾ {label} ({count})"),
                theme.group_header,
            ))),
            Entry::Empty => Row::plain(Line::from(Span::styled("  none", theme.secondary))),
            Entry::Item(row) => {
                let is_cursor = cursor == Some(row.id.as_str());
                let columns = widths.get(&row.section).map_or(&[][..], Vec::as_slice);
                Row {
                    line: row_line(theme, row, columns, width, is_cursor),
                    fill: is_cursor.then_some(theme.selection),
                    overlay: (!is_cursor && state.is_flashing(&row.id, now))
                        .then(|| theme.flash(flash_status(row))),
                    target: Some(row.id.clone()),
                }
            }
        })
        .collect();
    let targets = draw_rows(buf, area, &rows);
    Drawn {
        targets,
        scroll: offset,
        page: rows.len(),
        ..Drawn::default()
    }
}

/// The visible order with an `Empty` line under each heading whose section has no rows.
fn entries(state: &PlanState) -> Vec<Entry<'_>> {
    let visible = state.visible();
    let mut out = Vec::with_capacity(visible.len());
    for entry in &visible {
        match entry {
            VisibleRow::Header { label, count } => {
                let label = Section::ALL
                    .into_iter()
                    .map(Section::label)
                    .find(|known| *known == label.as_str())
                    .unwrap_or_default();
                out.push(Entry::Heading(label, *count));
                if *count == 0 {
                    out.push(Entry::Empty);
                }
            }
            VisibleRow::Item(id) => {
                if let Some(row) = state.row(id) {
                    out.push(Entry::Item(row));
                }
            }
        }
    }
    out
}

/// Each section's cell widths over all its rows, every cell but the last capped at
/// [`CELL_MAX`]; the last cell's width is left to the row.
fn widths(state: &PlanState) -> HashMap<Section, Vec<usize>> {
    let mut out: HashMap<Section, Vec<usize>> = HashMap::new();
    for row in state.rows.iter() {
        let widths = out.entry(row.section).or_default();
        let fixed = row.cells.len().saturating_sub(1);
        if widths.len() < fixed {
            widths.resize(fixed, 0);
        }
        for (at, cell) in row.cells.iter().take(fixed).enumerate() {
            widths[at] = widths[at].max(text_width(cell).min(CELL_MAX));
        }
    }
    out
}

/// The cursor mark, a blank, then the cells a cell apart, each padded to its section's
/// width; the last takes what is left, less a blank cell at the row's end. The first
/// cell is drawn plain, the others muted, and a verdict or an open handover warns.
fn row_line(
    theme: &Theme,
    row: &PlanRow,
    widths: &[usize],
    width: usize,
    is_cursor: bool,
) -> Line<'static> {
    let mut spans = vec![
        if is_cursor {
            Span::styled("▸", theme.selection_mark)
        } else {
            Span::raw(" ")
        },
        Span::raw(" "),
    ];
    let mut used = 2;
    let last = row.cells.len().saturating_sub(1);
    for (at, cell) in row.cells.iter().enumerate() {
        if at > 0 {
            spans.push(Span::raw(" "));
            used += 1;
        }
        let text = if at < last {
            let w = widths.get(at).copied().unwrap_or(0);
            let cut = truncate(cell, w);
            let pad = w.saturating_sub(text_width(&cut));
            format!("{cut}{}", " ".repeat(pad))
        } else {
            truncate(cell, width.saturating_sub(used + 1))
        };
        used += text_width(&text);
        spans.push(Span::styled(text, cell_style(theme, row, at, is_cursor)));
    }
    Line::from(spans)
}

fn cell_style(theme: &Theme, row: &PlanRow, at: usize, is_cursor: bool) -> Style {
    if at == 0 {
        return Style::default();
    }
    let warns = match row.section {
        Section::Health => at + 1 == row.cells.len(),
        Section::Handovers => row.state == "open" && row.cells.get(at) == Some(&row.state),
        _ => false,
    };
    if warns {
        theme.warning
    } else {
        theme.on_cursor(theme.facet, is_cursor)
    }
}

/// The task status whose flash colour suits the row's state.
fn flash_status(row: &PlanRow) -> TaskStatus {
    match row.state.as_str() {
        "done" | "pass" | "holds" | "healthy" => TaskStatus::Done,
        "" => TaskStatus::InProgress,
        "open" => TaskStatus::Pending,
        _ => TaskStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Action;
    use crate::config::Config;
    use crate::model::fixture;
    use crate::plan_state::NOT_V2;
    use crate::surface::Surface;

    fn plan_app(snapshot: crate::model::Snapshot) -> App {
        let mut app = App::new(snapshot, &Config::default());
        app.apply(Action::SwitchSurface(Surface::Plan));
        app
    }

    fn draw(app: &App, width: u16, height: u16) -> (Vec<String>, Drawn) {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let drawn = render_at(&mut buf, area, app, Instant::now());
        let rows = (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        (rows, drawn)
    }

    fn row_of(rows: &[String], needle: &str) -> usize {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not drawn:\n{}", rows.join("\n")))
    }

    #[test]
    fn plan_view_draws_every_section_and_a_row_of_each_at_two_widths() {
        let app = plan_app(fixture());
        for width in [100u16, 44] {
            let (rows, drawn) = draw(&app, width, 40);
            let mut last = 0;
            for section in Section::ALL {
                let heading = row_of(&rows, &format!("▾ {} (", section.label()));
                assert!(heading >= last, "{section:?} out of order:\n{rows:#?}");
                last = heading;
            }
            for needle in [
                "D1",
                "C1",
                "H1",
                "the foundation",
                "implement:B",
                "bind-the-keys",
                "Returned warnings",
                "other: implement-lite",
                "gate probe",
            ] {
                row_of(&rows, needle);
            }
            for row in &rows {
                assert!(text_width(row) <= usize::from(width), "{row:?}");
            }
            let ids: Vec<&str> = drawn.targets.iter().map(|(_, id)| id.as_str()).collect();
            assert!(ids.contains(&"decisions:D1"), "{ids:?}");
            let (rect, _) = &drawn.targets[0];
            assert_eq!(
                usize::from(rect.y),
                row_of(&rows, "D1"),
                "the first target is the first row"
            );
        }
    }

    #[test]
    fn plan_view_draws_an_empty_section_as_its_heading_and_none() {
        let app = plan_app(fixture());
        let (rows, _) = draw(&app, 80, 40);
        let brief = row_of(&rows, "▾ brief (0)");
        assert_eq!(rows[brief + 1].trim(), "none", "{rows:#?}");
        assert!(rows[brief + 2].starts_with("▾ decisions (1)"));
    }

    #[test]
    fn plan_view_marks_the_cursor_row_and_scrolls_to_it() {
        let mut app = plan_app(fixture());
        app.plan.select_row("health:gate:B.0");
        let (rows, drawn) = draw(&app, 80, 8);
        let at = row_of(&rows, "gate probe");
        assert!(rows[at].starts_with("▸ B"), "{:?}", rows[at]);
        assert!(drawn.scroll > 0, "the list scrolled to the cursor");
        assert_eq!(drawn.page, 8);
    }

    #[test]
    fn plan_view_without_a_plan_store_says_it_is_not_v2() {
        let mut snapshot = fixture();
        snapshot.plan = None;
        let app = plan_app(snapshot);
        let (rows, drawn) = draw(&app, 80, 5);
        assert_eq!(rows[0].trim_end(), NOT_V2);
        assert!(drawn.targets.is_empty());
    }
}
