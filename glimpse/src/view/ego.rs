//! The traversal view: the selected task in a centre card, what it waits on to
//! one side and what waits on it to the other.
//!
//! Horizontal puts upstream on the left and downstream on the right; vertical
//! puts upstream above and downstream below. Upstream is the task's own
//! `needs`, then its `coupling`; downstream is the rows naming it in theirs.
//! Coupling entries are drawn dashed. The centre column is the selected task's
//! layer: moving along it walks those peers, and moving across to a side
//! column selects that column's first entry, which the next frame recentres on.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::app::{App, Dir, Navigator};
use crate::config::Orientation;
use crate::model::{AgentStatus, Index, Snapshot, TaskStatus};

/// Two borders, the status row, a title that may wrap once, and the agent row.
const CARD_HEIGHT: u16 = 6;
/// Blank columns between two cells of a vertical strip.
const GAP: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    id: u32,
    coupling: bool,
}

struct Parts {
    up: Vec<Entry>,
    down: Vec<Entry>,
    peers: Vec<u32>,
}

impl Parts {
    fn of(snap: &Snapshot, index: &Index, id: u32) -> Parts {
        let up = index
            .task(snap, id)
            .map_or_else(Vec::new, |task| merge(&task.needs, &task.coupling));
        Parts {
            up,
            down: merge(index.dependents(id), index.coupled(id)),
            peers: peers(snap, index, id),
        }
    }

    fn peer(&self, centre: u32, forward: bool) -> Option<u32> {
        let pos = self.peers.iter().position(|&id| id == centre)?;
        if forward {
            self.peers.get(pos + 1).copied()
        } else {
            pos.checked_sub(1).and_then(|p| self.peers.get(p)).copied()
        }
    }
}

/// Solid entries first, then dashed, each ascending; an id on both lists
/// stays solid.
fn merge(solid: &[u32], dashed: &[u32]) -> Vec<Entry> {
    let mut solid = solid.to_vec();
    solid.sort_unstable();
    solid.dedup();
    let mut dashed: Vec<u32> = dashed
        .iter()
        .copied()
        .filter(|id| !solid.contains(id))
        .collect();
    dashed.sort_unstable();
    dashed.dedup();
    solid
        .into_iter()
        .map(|id| Entry {
            id,
            coupling: false,
        })
        .chain(dashed.into_iter().map(|id| Entry { id, coupling: true }))
        .collect()
}

/// The layer holding `id`, or `id` alone when no layer lists it.
fn peers(snap: &Snapshot, index: &Index, id: u32) -> Vec<u32> {
    index
        .layer_of(id)
        .and_then(|layer| snap.layers.get(layer))
        .filter(|ids| ids.contains(&id))
        .cloned()
        .unwrap_or_else(|| vec![id])
}

/// The selection, when it still names a task in the snapshot.
fn centre(app: &App) -> Option<u32> {
    app.selected
        .filter(|id| app.index.task(&app.snapshot, *id).is_some())
}

/// Moves over one frame's traversal layout. It answers only for the centre it
/// was built around; any other `from` has no neighbour.
pub(crate) struct EgoNavigator {
    centre: Option<u32>,
    orientation: Orientation,
    parts: Parts,
}

/// The navigator matching what [`render`] draws for the same `app` and
/// `orientation`.
pub(crate) fn navigator(app: &App, orientation: Orientation) -> EgoNavigator {
    let centre = centre(app);
    let parts = match centre {
        Some(id) => Parts::of(&app.snapshot, &app.index, id),
        None => Parts {
            up: Vec::new(),
            down: Vec::new(),
            peers: Vec::new(),
        },
    };
    EgoNavigator {
        centre,
        orientation,
        parts,
    }
}

impl Navigator for EgoNavigator {
    fn neighbor(&self, from: u32, dir: Dir) -> Option<u32> {
        if self.centre != Some(from) {
            return None;
        }
        let (upstream, downstream, prev) = match self.orientation {
            Orientation::Horizontal => (Dir::Left, Dir::Right, Dir::Up),
            Orientation::Vertical => (Dir::Up, Dir::Down, Dir::Left),
        };
        if dir == upstream {
            self.parts.up.first().map(|entry| entry.id)
        } else if dir == downstream {
            self.parts.down.first().map(|entry| entry.id)
        } else {
            self.parts.peer(from, dir != prev)
        }
    }
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App, orientation: Orientation) {
    let Some(id) = centre(app) else {
        frame.render_widget(
            Paragraph::new(Line::styled("no task selected", app.theme.pending)),
            area,
        );
        return;
    };
    let parts = Parts::of(&app.snapshot, &app.index, id);
    let now = Instant::now();
    match orientation {
        Orientation::Horizontal => horizontal(frame, area, app, id, &parts, now),
        Orientation::Vertical => vertical(frame, area, app, id, &parts, now),
    }
}

fn glyph(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "○",
        TaskStatus::InProgress => "◐",
        TaskStatus::Done => "✓",
        TaskStatus::Failed => "✗",
        TaskStatus::Deferred => "⏸",
        TaskStatus::Unknown => "?",
    }
}

fn label(app: &App, id: u32) -> String {
    match app.index.task(&app.snapshot, id) {
        Some(task) => format!("{} {} {}", glyph(task.status), task.id, task.title),
        None => format!("? {id}"),
    }
}

fn task_style(app: &App, id: u32, now: Instant) -> Style {
    if app.is_flashing(id, now) {
        return app.theme.flash;
    }
    app.index
        .task(&app.snapshot, id)
        .map_or(app.theme.pending, |task| {
            app.theme.status(task.status.as_str())
        })
}

fn edge_style(app: &App, entry: Entry) -> Style {
    if entry.coupling {
        app.theme.coupling_edge
    } else {
        app.theme.needs_edge
    }
}

/// Cut to `width` characters, ending in `…` when anything was dropped.
fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut cut: String = text.chars().take(width - 1).collect();
    cut.push('…');
    cut
}

fn pad(text: String, width: usize) -> String {
    format!("{text:<width$}")
}

fn line_at(frame: &mut Frame, area: Rect, x: u16, y: u16, width: u16, line: Line<'_>) {
    let rect = Rect::new(x, y, width, 1).intersection(area);
    if !rect.is_empty() {
        frame.render_widget(Paragraph::new(line), rect);
    }
}

fn draw_card(frame: &mut Frame, rect: Rect, app: &App, id: u32, now: Instant) {
    let Some(task) = app.index.task(&app.snapshot, id) else {
        return;
    };
    let theme = &app.theme;
    let status_style = theme.status(task.status.as_str());
    let border = if app.is_flashing(id, now) {
        theme.flash
    } else {
        status_style
    };

    let mut status = vec![Span::styled(
        format!("{} {}", glyph(task.status), task.status.as_str()),
        status_style,
    )];
    if !task.effort.is_empty() {
        status.push(Span::raw(format!("  {}", task.effort)));
    }
    if !task.checkpoint.is_empty() {
        status.push(Span::raw(format!("  cp {}", task.checkpoint)));
    }
    let mut lines = vec![
        Line::from(status),
        Line::styled(task.title.clone(), theme.badge),
    ];
    if let Some(agent) = app.index.agents_for(&app.snapshot, id).first() {
        let stale = app.stale_agents.contains(&agent.id);
        let live = agent.status == AgentStatus::Running && !stale;
        let state = match agent.status {
            AgentStatus::Running if stale => "stale",
            AgentStatus::Running => "running",
            AgentStatus::Idle => "idle",
            AgentStatus::Stopped => "stopped",
            AgentStatus::Unknown => "unknown",
        };
        lines.push(Line::styled(
            format!(" {} {state} ", agent.agent_type),
            if live {
                theme.agent_chip
            } else {
                theme.pending
            },
        ));
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Line::from(Span::styled(
            format!(" {} ", task.id),
            theme.selection,
        )));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(block),
        rect,
    );
}

/// The entries that fit in `slots`, and how many were left out. When some
/// are left out, the last slot is reserved for the `+N more` marker.
fn visible(entries: &[Entry], slots: usize) -> (&[Entry], usize) {
    if entries.len() <= slots {
        return (entries, 0);
    }
    let shown = slots.saturating_sub(1);
    (&entries[..shown], entries.len() - shown)
}

fn horizontal(frame: &mut Frame, area: Rect, app: &App, id: u32, parts: &Parts, now: Instant) {
    let card_w = (area.width / 3).clamp(16, 40).min(area.width);
    let side_w = area.width.saturating_sub(card_w) / 2;
    let right_w = area.width.saturating_sub(card_w + side_w);
    let card_h = CARD_HEIGHT.min(area.height);
    let card = Rect::new(
        area.x + side_w,
        area.y + (area.height - card_h) / 2,
        card_w,
        card_h,
    );
    draw_card(frame, card, app, id, now);

    if let Some(prev) = parts.peer(id, false)
        && card.y > area.y
    {
        let text = truncate(&format!("▲ {}", label(app, prev)), usize::from(card_w));
        line_at(
            frame,
            area,
            card.x,
            card.y - 1,
            card_w,
            Line::styled(text, task_style(app, prev, now)),
        );
    }
    if let Some(next) = parts.peer(id, true)
        && card.bottom() < area.bottom()
    {
        let text = truncate(&format!("▼ {}", label(app, next)), usize::from(card_w));
        line_at(
            frame,
            area,
            card.x,
            card.bottom(),
            card_w,
            Line::styled(text, task_style(app, next, now)),
        );
    }

    let mid_row = card.y + card_h / 2;
    let left = Rect::new(area.x, area.y, side_w, area.height);
    let right = Rect::new(card.right(), area.y, right_w, area.height);
    side_column(frame, left, app, "needs", &parts.up, true, mid_row, now);
    side_column(
        frame,
        right,
        app,
        "dependents",
        &parts.down,
        false,
        mid_row,
        now,
    );
}

/// One side column, vertically centred on `mid_row`. The arrow sits on the
/// edge that faces the card: trailing on the left column, leading on the right.
#[allow(clippy::too_many_arguments)]
fn side_column(
    frame: &mut Frame,
    rect: Rect,
    app: &App,
    heading: &str,
    entries: &[Entry],
    left: bool,
    mid_row: u16,
    now: Instant,
) {
    if rect.width < 4 || rect.height == 0 {
        return;
    }
    let width = usize::from(rect.width);
    let text_w = width - 3;
    let (shown, hidden) = visible(entries, usize::from(rect.height - 1));
    let mut lines = vec![Line::styled(
        if left {
            format!("{heading:>width$}")
        } else {
            heading.to_string()
        },
        app.theme.pending,
    )];
    if entries.is_empty() {
        let none = if left {
            format!("{:>width$}", "none")
        } else {
            "none".to_string()
        };
        lines.push(Line::styled(none, app.theme.pending));
    }
    for entry in shown {
        let text = pad(truncate(&label(app, entry.id), text_w), text_w);
        let arrow = Span::styled(
            if entry.coupling { "┄▶" } else { "─▶" },
            edge_style(app, *entry),
        );
        let text = Span::styled(text, task_style(app, entry.id, now));
        lines.push(if left {
            Line::from(vec![text, Span::raw(" "), arrow])
        } else {
            Line::from(vec![arrow, Span::raw(" "), text])
        });
    }
    if hidden > 0 {
        let more = format!("+{hidden} more");
        lines.push(Line::styled(
            if left {
                format!("{more:>width$}")
            } else {
                more
            },
            app.theme.pending,
        ));
    }

    let block_h = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(rect.height);
    let top = mid_row
        .saturating_sub(block_h / 2)
        .clamp(rect.y, rect.bottom() - block_h);
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(rect.x, top, rect.width, block_h),
    );
}

fn vertical(frame: &mut Frame, area: Rect, app: &App, id: u32, parts: &Parts, now: Instant) {
    let strip_h = 3.min(area.height / 2);
    let top = Rect::new(area.x, area.y, area.width, strip_h);
    let bottom = Rect::new(area.x, area.bottom() - strip_h, area.width, strip_h);
    let band = Rect::new(
        area.x,
        area.y + strip_h,
        area.width,
        area.height - 2 * strip_h,
    );

    let card_w = area.width.min(44);
    let card_h = CARD_HEIGHT.min(band.height);
    let card = Rect::new(
        area.x + (area.width - card_w) / 2,
        band.y + (band.height - card_h) / 2,
        card_w,
        card_h,
    );
    draw_card(frame, card, app, id, now);

    let mid_row = card.y + card_h / 2;
    let margin = card.x - area.x;
    if margin >= 4 {
        let width = usize::from(margin - 1);
        if let Some(prev) = parts.peer(id, false) {
            let text = truncate(&format!("◀ {}", label(app, prev)), width);
            line_at(
                frame,
                area,
                area.x,
                mid_row,
                margin - 1,
                Line::styled(text, task_style(app, prev, now)),
            );
        }
        if let Some(next) = parts.peer(id, true) {
            let text = truncate(&format!("{} ▶", label(app, next)), width);
            let x = card.right() + 1;
            let line = Line::styled(format!("{text:>width$}"), task_style(app, next, now));
            line_at(
                frame,
                area,
                x,
                mid_row,
                area.right().saturating_sub(x),
                line,
            );
        }
    }

    strip(frame, top, app, "needs", &parts.up, true, now);
    strip(frame, bottom, app, "dependents", &parts.down, false, now);
}

/// A row of cells with a heading on the far side from the card and a
/// connector row on the near side: `│` for needs, `┆` for coupling.
fn strip(
    frame: &mut Frame,
    rect: Rect,
    app: &App,
    heading: &str,
    entries: &[Entry],
    above: bool,
    now: Instant,
) {
    if rect.height < 3 || rect.width == 0 {
        return;
    }
    let count = u16::try_from(entries.len().max(1)).unwrap_or(u16::MAX);
    let cell_w = ((rect.width + GAP) / count)
        .saturating_sub(GAP)
        .clamp(10, 28)
        .min(rect.width);
    let fit = usize::from((rect.width + GAP) / (cell_w + GAP)).max(1);
    let (shown, hidden) = visible(entries, fit);
    let cell = usize::from(cell_w);
    let stride = cell + usize::from(GAP);

    let mut cells = Vec::new();
    let mut connectors = Vec::new();
    for entry in shown {
        let text = pad(truncate(&label(app, entry.id), cell), stride);
        cells.push(Span::styled(text, task_style(app, entry.id, now)));
        let connector = if entry.coupling { "┆" } else { "│" };
        connectors.push(Span::styled(
            pad(connector.to_string(), stride),
            edge_style(app, *entry),
        ));
    }
    if hidden > 0 {
        cells.push(Span::styled(format!("+{hidden} more"), app.theme.pending));
    }
    if entries.is_empty() {
        cells.push(Span::styled("none", app.theme.pending));
    }

    let heading = Line::styled(heading.to_string(), app.theme.pending);
    let rows = if above {
        [heading, Line::from(cells), Line::from(connectors)]
    } else {
        [Line::from(connectors), Line::from(cells), heading]
    };
    for (offset, line) in (0u16..).zip(rows) {
        line_at(frame, rect, rect.x, rect.y + offset, rect.width, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Action;
    use crate::config::Config;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app_on(id: u32) -> App {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = Some(id);
        app
    }

    fn draw(app: &App, width: u16, height: u16, orientation: Orientation) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), app, orientation))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    /// Row and column (in cells) of the first occurrence of `needle`.
    fn find(rows: &[String], needle: &str) -> (usize, usize) {
        rows.iter()
            .enumerate()
            .find_map(|(y, row)| {
                row.find(needle)
                    .map(|byte| (y, row[..byte].chars().count()))
            })
            .unwrap_or_else(|| panic!("{needle:?} not drawn in:\n{}", rows.join("\n")))
    }

    fn row_of<'r>(rows: &'r [String], needle: &str) -> &'r str {
        &rows[find(rows, needle).0]
    }

    #[test]
    fn horizontal_puts_needs_left_and_dependents_right_of_the_card() {
        let app = app_on(4);
        let rows = draw(&app, 120, 20, Orientation::Horizontal);
        let (need_y, need_x) = find(&rows, "1 Scaffold the store");
        let (card_y, card_x) = find(&rows, "Render the rows");
        let (dep_y, dep_x) = find(&rows, "7 Assemble the app");
        assert!(need_x < card_x && card_x < dep_x, "{}", rows.join("\n"));
        assert!(need_y.abs_diff(card_y) <= 2 && dep_y.abs_diff(card_y) <= 2);
        assert!(row_of(&rows, "1 Scaffold").contains("─▶"));
        assert!(row_of(&rows, "7 Assemble").contains("─▶"));
        assert!(rows.iter().any(|row| row.contains("◐ in-progress")));
        assert!(rows.iter().any(|row| row.contains("implement-deep")));
        assert!(
            row_of(&rows, "5 Style the rows").contains('▼'),
            "the next layer peer sits below the card"
        );
    }

    #[test]
    fn coupling_entries_are_dashed_on_both_sides() {
        let rows = draw(&app_on(8), 120, 20, Orientation::Horizontal);
        assert!(row_of(&rows, "5 Style the rows").contains("┄▶"));
        let need = row_of(&rows, "6 Bind the keys");
        assert!(need.contains("─▶") && !need.contains('┄'), "{need}");

        let rows = draw(&app_on(3), 120, 20, Orientation::Horizontal);
        let (_, card_x) = find(&rows, "Load the config");
        let (_, dep_x) = find(&rows, "6 Bind the keys");
        assert!(dep_x > card_x, "a coupled row is downstream");
        assert!(row_of(&rows, "6 Bind the keys").contains("┄▶"));

        let rows = draw(&app_on(8), 60, 30, Orientation::Vertical);
        let (cell_y, cell_x) = find(&rows, "5 Style");
        let below: Vec<char> = rows[cell_y + 1].chars().collect();
        assert_eq!(below[cell_x - 2], '┆', "{}", rows.join("\n"));
    }

    #[test]
    fn vertical_puts_needs_above_and_dependents_below_the_card() {
        let rows = draw(&app_on(4), 60, 30, Orientation::Vertical);
        let (need_y, _) = find(&rows, "1 Scaffold");
        let (card_y, _) = find(&rows, "Render the rows");
        let (dep_y, _) = find(&rows, "7 Assemble");
        assert!(need_y < card_y && card_y < dep_y, "{}", rows.join("\n"));
        assert!(find(&rows, "needs").0 < need_y);
        assert!(find(&rows, "dependents").0 > dep_y);
    }

    #[test]
    fn a_task_with_no_neighbours_shows_none() {
        let mut snap = fixture();
        snap.tasks.retain(|task| task.id == 2);
        snap.layers = vec![vec![2]];
        let mut app = App::new(snap, &Config::default());
        app.selected = Some(2);
        let rows = draw(&app, 100, 12, Orientation::Horizontal);
        let nones: usize = rows.iter().map(|row| row.matches("none").count()).sum();
        assert_eq!(nones, 2, "{}", rows.join("\n"));
        let nav = navigator(&app, Orientation::Horizontal);
        for dir in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
            assert_eq!(nav.neighbor(2, dir), None);
        }
    }

    #[test]
    fn the_navigator_crosses_columns_and_walks_the_layer() {
        let app = app_on(4);
        let nav = navigator(&app, Orientation::Horizontal);
        assert_eq!(nav.neighbor(4, Dir::Left), Some(1));
        assert_eq!(nav.neighbor(4, Dir::Right), Some(7));
        assert_eq!(nav.neighbor(4, Dir::Down), Some(5));
        assert_eq!(nav.neighbor(4, Dir::Up), None, "4 opens its layer");
        assert_eq!(nav.neighbor(5, Dir::Down), None, "only the centre moves");

        let nav = navigator(&app, Orientation::Vertical);
        assert_eq!(nav.neighbor(4, Dir::Up), Some(1));
        assert_eq!(nav.neighbor(4, Dir::Down), Some(7));
        assert_eq!(nav.neighbor(4, Dir::Right), Some(5));
        assert_eq!(nav.neighbor(4, Dir::Left), None);

        let nav = navigator(&app_on(8), Orientation::Horizontal);
        assert_eq!(nav.neighbor(8, Dir::Left), Some(6), "needs before coupling");
        assert_eq!(nav.neighbor(8, Dir::Right), None);
        assert_eq!(nav.neighbor(8, Dir::Up), Some(7));
    }

    #[test]
    fn a_move_across_recentres_the_next_frame() {
        let mut app = app_on(4);
        app.nav = Some(Box::new(navigator(&app, Orientation::Horizontal)));
        app.apply(Action::Move(Dir::Right));
        assert_eq!(app.selected, Some(7));

        let rows = draw(&app, 120, 20, Orientation::Horizontal);
        let (_, card_x) = find(&rows, "Assemble the app");
        let (_, need_x) = find(&rows, "4 Render the rows");
        assert!(need_x < card_x, "the old centre is now a need");
        assert!(rows.iter().any(|row| row.contains("dependents")));
    }

    #[test]
    fn an_overfull_column_counts_what_it_hides() {
        let mut snap = fixture();
        for task in &mut snap.tasks {
            if task.id != 1 {
                task.needs = vec![1];
                task.coupling.clear();
            }
        }
        snap.layers = vec![vec![1], vec![2, 3, 4, 5, 6, 7, 8]];
        let mut app = App::new(snap, &Config::default());
        app.selected = Some(1);
        let rows = draw(&app, 120, 5, Orientation::Horizontal);
        assert!(
            rows.iter().any(|row| row.contains("+4 more")),
            "{}",
            rows.join("\n")
        );
        let rows = draw(&app, 40, 20, Orientation::Vertical);
        assert!(
            rows.iter().any(|row| row.contains("more")),
            "{}",
            rows.join("\n")
        );
    }

    #[test]
    fn no_selection_draws_a_placeholder_and_never_moves() {
        let mut app = app_on(4);
        app.selected = None;
        let rows = draw(&app, 60, 10, Orientation::Vertical);
        assert!(rows[0].contains("no task selected"));
        assert_eq!(
            navigator(&app, Orientation::Vertical).neighbor(4, Dir::Up),
            None
        );
    }

    #[test]
    fn a_tiny_area_does_not_panic() {
        let app = app_on(4);
        for (w, h) in [(1, 1), (5, 2), (20, 3), (3, 20)] {
            draw(&app, w, h, Orientation::Horizontal);
            draw(&app, w, h, Orientation::Vertical);
        }
    }
}
