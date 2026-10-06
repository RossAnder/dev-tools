//! The layer-list view and its navigator.
//!
//! Tasks are grouped by the snapshot's Kahn layers, taken verbatim. Vertically
//! each layer is a header row followed by one row per task; horizontally each
//! layer is a column of compact cells, sized to the longest title within the
//! app's `column_max` (or to `column_override`), showing only the layers that
//! fit. A checkpoint row follows the layer that completes the checkpoint's group.
//! With a selection, every row is marked by its relation to it and unrelated rows
//! are faded, all but their status mark, which always keeps its colour.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::{Place, initials, place, text_width, truncate};
use crate::app::{App, Dir, ListScroll, Navigator};
use crate::config::{COLUMN_RANGE, Orientation};
use crate::hook::parse_utc;
use crate::model::{AgentStatus, Checkpoint, Index, Snapshot, Task, TaskStatus};

/// A task row's on-screen rect, for mouse hits.
pub(crate) type Target = (Rect, u32);

/// What a frame hands back for `App` to keep.
pub(crate) struct Drawn {
    pub(crate) targets: Vec<Target>,
    pub(crate) scroll: ListScroll,
    /// The column width a horizontal frame chose, before it met the pane.
    pub(crate) column: Option<u16>,
}

/// One layer, plus the checkpoints whose last member sits in it.
struct Group {
    label: String,
    ids: Vec<u32>,
    closes: Vec<usize>,
}

/// Layers holding only ids that name a task; rows missing from every layer
/// form a trailing group so that no task goes undrawn.
fn groups(snapshot: &Snapshot, index: &Index) -> Vec<Group> {
    let mut groups: Vec<Group> = snapshot
        .layers
        .iter()
        .enumerate()
        .map(|(depth, ids)| Group {
            label: format!("L{}", depth + 1),
            ids: ids
                .iter()
                .copied()
                .filter(|id| index.task(snapshot, *id).is_some())
                .collect(),
            closes: Vec::new(),
        })
        .collect();
    let mut unlayered: Vec<u32> = snapshot
        .tasks
        .iter()
        .map(|task| task.id)
        .filter(|id| index.layer_of(*id).is_none())
        .collect();
    if !unlayered.is_empty() {
        unlayered.sort_unstable();
        groups.push(Group {
            label: "unlayered".to_string(),
            ids: unlayered,
            closes: Vec::new(),
        });
    }
    for (pos, checkpoint) in snapshot.checkpoints.iter().enumerate() {
        let last = checkpoint
            .members
            .iter()
            .filter_map(|id| index.layer_of(*id))
            .max()
            .or(groups.len().checked_sub(1));
        if let Some(group) = last.and_then(|depth| groups.get_mut(depth)) {
            group.closes.push(pos);
        }
    }
    groups
}

/// Moves over the layer list: vertically `Up`/`Down` step through the rows in
/// reading order, crossing layer boundaries, and `Left`/`Right` jump to the
/// same slot of the adjacent layer; horizontally `Up`/`Down` stay within the
/// column and `Left`/`Right` cross to the adjacent one.
pub(crate) struct LayerNav {
    layers: Vec<Vec<u32>>,
    orientation: Orientation,
}

/// Built from the same grouping [`render`] draws, so a move lands on a row
/// that is on screen once the view scrolls to it.
pub(crate) fn navigator(
    snapshot: &Snapshot,
    index: &Index,
    orientation: Orientation,
) -> Box<dyn Navigator> {
    let layers = groups(snapshot, index)
        .into_iter()
        .map(|group| group.ids)
        .filter(|ids| !ids.is_empty())
        .collect();
    Box::new(LayerNav {
        layers,
        orientation,
    })
}

impl LayerNav {
    fn position(&self, id: u32) -> Option<(usize, usize)> {
        self.layers.iter().enumerate().find_map(|(layer, ids)| {
            ids.iter()
                .position(|each| *each == id)
                .map(|slot| (layer, slot))
        })
    }

    fn across(&self, layer: usize, slot: usize, forward: bool) -> Option<u32> {
        let target = if forward {
            layer + 1
        } else {
            layer.checked_sub(1)?
        };
        let ids = self.layers.get(target)?;
        ids.get(slot.min(ids.len().checked_sub(1)?)).copied()
    }

    fn within(&self, layer: usize, slot: usize, forward: bool) -> Option<u32> {
        let slot = if forward {
            slot + 1
        } else {
            slot.checked_sub(1)?
        };
        self.layers.get(layer)?.get(slot).copied()
    }

    fn reading(&self, from: u32, forward: bool) -> Option<u32> {
        let order: Vec<u32> = self.layers.iter().flatten().copied().collect();
        let at = order.iter().position(|id| *id == from)?;
        let to = if forward { at + 1 } else { at.checked_sub(1)? };
        order.get(to).copied()
    }
}

impl Navigator for LayerNav {
    fn neighbor(&self, from: u32, dir: Dir) -> Option<u32> {
        let (layer, slot) = self.position(from)?;
        match (self.orientation, dir) {
            (Orientation::Vertical, Dir::Up) => self.reading(from, false),
            (Orientation::Vertical, Dir::Down) => self.reading(from, true),
            (Orientation::Horizontal, Dir::Up) => self.within(layer, slot, false),
            (Orientation::Horizontal, Dir::Down) => self.within(layer, slot, true),
            (_, Dir::Left) => self.across(layer, slot, false),
            (_, Dir::Right) => self.across(layer, slot, true),
        }
    }
}

/// Draws the layer list into `area` of `buf`; a `Frame` caller passes
/// `frame.buffer_mut()`.
pub(crate) fn render(buf: &mut Buffer, area: Rect, app: &App, orientation: Orientation) -> Drawn {
    render_at(
        buf,
        area,
        app,
        orientation,
        SystemTime::now(),
        Instant::now(),
    )
}

/// The horizontal column width, gutter included, before it meets the pane: the
/// override when set, else the widest task cell clamped to `COLUMN_RANGE` and
/// `app.column_max`. Every layer counts, not just the visible ones, so the width holds
/// still while the view scrolls.
fn column_for(ctx: &Ctx) -> u16 {
    if let Some(width) = ctx.app.column_override {
        return width;
    }
    let widest = ctx
        .app
        .snapshot
        .tasks
        .iter()
        .map(|task| natural_width(ctx, task, None))
        .max()
        .unwrap_or(0);
    let column = u16::try_from(widest + 1).unwrap_or(u16::MAX);
    column.clamp(
        *COLUMN_RANGE.start(),
        ctx.app.column_max.max(*COLUMN_RANGE.start()),
    )
}

/// `wall` dates the agents' elapsed counters and `now` the flashes.
fn render_at(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    orientation: Orientation,
    wall: SystemTime,
    now: Instant,
) -> Drawn {
    if area.width == 0 || area.height == 0 {
        return Drawn {
            targets: Vec::new(),
            scroll: app.layers_scroll,
            column: None,
        };
    }
    let ctx = Ctx::new(app, wall, now);
    let groups = groups(&app.snapshot, &app.index);
    match orientation {
        Orientation::Vertical => render_vertical(buf, area, &ctx, &groups),
        Orientation::Horizontal => render_horizontal(buf, area, &ctx, &groups),
    }
}

/// A drawn row, the styles laid under and over its full width, and the task it shows.
struct Row {
    line: Line<'static>,
    fill: Option<Style>,
    overlay: Option<Style>,
    task: Option<u32>,
}

impl Row {
    fn plain(line: Line<'static>) -> Row {
        Row {
            line,
            fill: None,
            overlay: None,
            task: None,
        }
    }

    fn task(ctx: &Ctx, task: &Task, line: Line<'static>) -> Row {
        Row {
            line,
            fill: ctx.fill(task),
            overlay: ctx.overlay(task),
            task: Some(task.id),
        }
    }
}

fn render_vertical(buf: &mut Buffer, area: Rect, ctx: &Ctx, groups: &[Group]) -> Drawn {
    let width = usize::from(area.width);
    let id_width = ctx
        .app
        .snapshot
        .tasks
        .iter()
        .map(|task| task.id.to_string().len())
        .max()
        .unwrap_or(1);
    let mut rows = Vec::new();
    let mut selected_row = None;
    for group in groups {
        rows.push(Row::plain(layer_rule(ctx, "── ", &group.label, width)));
        for id in &group.ids {
            let Some(task) = ctx.app.index.task(&ctx.app.snapshot, *id) else {
                continue;
            };
            if ctx.is_selected(*id) {
                selected_row = Some(rows.len());
            }
            rows.push(Row::task(
                ctx,
                task,
                task_line(ctx, task, width, Some(id_width)),
            ));
        }
        for pos in &group.closes {
            let checkpoint = &ctx.app.snapshot.checkpoints[*pos];
            rows.push(Row::plain(checkpoint_line(ctx, checkpoint, width, true)));
        }
    }
    let height = usize::from(area.height);
    let from = nudged(ctx.app.layers_scroll.offset, ctx.app.scroll_nudge.1);
    let offset = place(from, selected_row, rows.len(), height, ctx.place());
    Drawn {
        targets: draw_rows(buf, area, &rows[offset..]),
        scroll: ListScroll { offset, rows: 0 },
        column: None,
    }
}

/// Columns keep the width [`column_for`] picks and only as many layers as fit are drawn,
/// scrolled across by [`place`] and down within the selected layer's column the same way;
/// the wheel scrolls across. `‹` on the first column's rule and `›` at the right edge
/// mark layers scrolled off either side.
fn render_horizontal(buf: &mut Buffer, area: Rect, ctx: &Ctx, groups: &[Group]) -> Drawn {
    if groups.is_empty() {
        return Drawn {
            targets: Vec::new(),
            scroll: ListScroll::default(),
            column: None,
        };
    }
    let natural = column_for(ctx);
    let column = natural.min(area.width).max(1);
    let visible = usize::from(area.width / column).max(1);
    let selected_column = groups
        .iter()
        .position(|group| group.ids.iter().any(|id| ctx.is_selected(*id)));
    let from = nudged(ctx.app.layers_scroll.offset, ctx.app.scroll_nudge.0);
    let first = place(from, selected_column, groups.len(), visible, ctx.place());
    let mut scroll = ListScroll {
        offset: first,
        rows: 0,
    };
    let height = usize::from(area.height);
    // The last cell of a column is left blank as the gutter to the next one.
    let cell = usize::from(column).saturating_sub(1).max(1);
    let mut targets = Vec::new();
    for (slot, group) in groups.iter().skip(first).take(visible).enumerate() {
        let x = area.x + column * u16::try_from(slot).unwrap_or(0);
        let lead = if slot == 0 && first > 0 { "‹ " } else { "" };
        let mut rows = vec![Row::plain(layer_rule(ctx, lead, &group.label, cell))];
        let mut selected_row = None;
        for id in &group.ids {
            let Some(task) = ctx.app.index.task(&ctx.app.snapshot, *id) else {
                continue;
            };
            if ctx.is_selected(*id) {
                selected_row = Some(rows.len());
            }
            rows.push(Row::task(ctx, task, task_line(ctx, task, cell, None)));
        }
        for pos in &group.closes {
            let checkpoint = &ctx.app.snapshot.checkpoints[*pos];
            rows.push(Row::plain(checkpoint_line(ctx, checkpoint, cell, false)));
        }
        let offset = if selected_row.is_some() {
            let from = ctx.app.layers_scroll.rows;
            scroll.rows = place(from, selected_row, rows.len(), height, ctx.place());
            scroll.rows
        } else {
            0
        };
        let width = u16::try_from(cell)
            .unwrap_or(u16::MAX)
            .min(area.right().saturating_sub(x));
        targets.extend(draw_rows(
            buf,
            Rect::new(x, area.y, width, area.height),
            &rows[offset..],
        ));
    }
    if first + visible < groups.len() {
        let x = area.right().saturating_sub(2).max(area.x);
        buf.set_string(x, area.y, " ›", ctx.app.theme.badge);
    }
    Drawn {
        targets,
        scroll,
        column: Some(natural),
    }
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
        if let Some(id) = row.task {
            targets.push((rect, id));
        }
    }
    targets
}

fn nudged(offset: usize, by: i32) -> usize {
    offset.saturating_add_signed(isize::try_from(by).unwrap_or(0))
}

/// The selection and the rows related to it.
struct Focus {
    id: u32,
    needs: Vec<u32>,
    dependents: Vec<u32>,
    coupling: Vec<u32>,
    coupled: Vec<u32>,
    overlaps: Vec<u32>,
}

struct Ctx<'a> {
    app: &'a App,
    wall: SystemTime,
    now: Instant,
    focus: Option<Focus>,
}

impl<'a> Ctx<'a> {
    fn new(app: &'a App, wall: SystemTime, now: Instant) -> Ctx<'a> {
        let focus = app
            .selected
            .and_then(|id| app.index.task(&app.snapshot, id))
            .map(|task| Focus {
                id: task.id,
                needs: task.needs.clone(),
                dependents: app.index.dependents(task.id).to_vec(),
                coupling: task.coupling.clone(),
                coupled: app.index.coupled(task.id).to_vec(),
                overlaps: app.index.overlaps(task.id).to_vec(),
            });
        Ctx {
            app,
            wall,
            now,
            focus,
        }
    }

    fn is_selected(&self, id: u32) -> bool {
        self.focus.as_ref().is_some_and(|focus| focus.id == id)
    }

    /// `↑` a prerequisite of the selection, `↓` a dependent, `⇡`/`⇣` the
    /// coupling peers before and after it, `≈` a row sharing a file with it.
    fn mark(&self, id: u32) -> Option<(&'static str, Style)> {
        let focus = self.focus.as_ref()?;
        let theme = &self.app.theme;
        if focus.id == id {
            Some(("▸", theme.selection_mark))
        } else if focus.needs.contains(&id) {
            Some(("↑", theme.needs_edge))
        } else if focus.dependents.contains(&id) {
            Some(("↓", theme.needs_edge))
        } else if focus.coupling.contains(&id) {
            Some(("⇡", theme.coupling_edge))
        } else if focus.coupled.contains(&id) {
            Some(("⇣", theme.coupling_edge))
        } else if focus.overlaps.contains(&id) {
            Some(("≈", theme.overlap))
        } else {
            None
        }
    }

    /// With a selection, a row with no relation to it.
    fn unrelated(&self, id: u32) -> bool {
        self.focus.is_some() && self.mark(id).is_none()
    }

    fn place(&self) -> Place {
        if self.app.scroll_pinned {
            Place::Pinned
        } else if self.app.follow {
            Place::Jump
        } else {
            Place::Step
        }
    }

    /// The selection's tint, laid under the row's spans.
    fn fill(&self, task: &Task) -> Option<Style> {
        self.is_selected(task.id)
            .then_some(self.app.theme.selection)
    }

    /// A status change's flash in the colour of the new status, laid over the whole row.
    fn overlay(&self, task: &Task) -> Option<Style> {
        (!self.is_selected(task.id) && self.app.is_flashing(task.id, self.now))
            .then(|| self.app.theme.flash(task.status))
    }

    /// The id and title style: faded when the row is unrelated to the selection.
    fn text_style(&self, id: u32) -> Style {
        if self.unrelated(id) {
            self.app.theme.unrelated
        } else {
            Style::new()
        }
    }

    /// Type initials and elapsed time of a running agent with an open
    /// assignment on `id`; dimmed once its transcript has gone quiet.
    fn chip(&self, id: u32) -> Option<Span<'static>> {
        let (snapshot, index) = (&self.app.snapshot, &self.app.index);
        let (agent, started) = index
            .agents_for(snapshot, id)
            .into_iter()
            .filter(|agent| agent.status == AgentStatus::Running)
            .find_map(|agent| {
                let open = agent
                    .segments
                    .iter()
                    .rev()
                    .find(|seg| seg.ended_at.is_empty() && seg.task_ids.contains(&id))?;
                Some((agent, open.started_at.as_str()))
            })?;
        let elapsed = parse_utc(started)
            .and_then(|at| {
                self.wall
                    .duration_since(UNIX_EPOCH + Duration::from_secs(at))
                    .ok()
            })
            .map(|age| format!(" {}", format_elapsed(age)))
            .unwrap_or_default();
        let style = if self.app.stale_agents.contains(&agent.id) {
            self.app.theme.agent_chip_stale
        } else {
            self.app.theme.agent_chip
        };
        Some(super::chip(
            &format!("{}{elapsed}", initials(&agent.agent_type)),
            style,
        ))
    }

    /// A deferral record is badged only while the status does not already
    /// say deferred, since both draw the same pause glyph.
    fn badges(&self, task: &Task) -> Vec<Span<'static>> {
        let record = self.app.index.record_entries(&self.app.snapshot, task.id);
        let has = |kind: &str| record.clone().any(|entry| entry.entry_type == kind);
        let mut badges = Vec::new();
        if task.status != TaskStatus::Deferred && has("deferral") {
            badges.push(Span::styled(
                TaskStatus::Deferred.glyph(),
                self.app.theme.deferral_badge,
            ));
        }
        badges
    }
}

/// One task row. `id_width` right-aligns the id in a full row, which keeps its last
/// cell blank so a highlight is padded on both sides; `None` gives the compact
/// `◐14 title…` cell with no effort column.
fn task_line(ctx: &Ctx, task: &Task, width: usize, id_width: Option<usize>) -> Line<'static> {
    let (mut left, right) = task_parts(ctx, task, id_width);
    let width = width.saturating_sub(usize::from(id_width.is_some()));
    let used = parts_width(&left, &right);
    let room = width.saturating_sub(used);
    let title = truncate(&task.title, room);
    let pad = room.saturating_sub(text_width(&title));
    left.push(Span::styled(title, ctx.text_style(task.id)));
    left.push(Span::raw(" ".repeat(pad)));
    if !right.is_empty() {
        left.push(Span::raw(" "));
        left.extend(right);
    }
    if id_width.is_some() {
        left.push(Span::raw(" "));
    }
    Line::from(left)
}

/// Cells [`task_line`] needs to show `task`'s whole title.
fn natural_width(ctx: &Ctx, task: &Task, id_width: Option<usize>) -> usize {
    let (left, right) = task_parts(ctx, task, id_width);
    parts_width(&left, &right) + text_width(&task.title) + usize::from(id_width.is_some())
}

fn parts_width(left: &[Span], right: &[Span]) -> usize {
    left.iter().chain(right).map(Span::width).sum::<usize>() + usize::from(!right.is_empty())
}

/// The spans before the title (mark, status, id) and after it (badges, chip, effort).
fn task_parts(
    ctx: &Ctx,
    task: &Task,
    id_width: Option<usize>,
) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    let theme = &ctx.app.theme;
    let status = theme.status(task.status.as_str());
    let text = ctx.text_style(task.id);
    let (mark, mark_style) = ctx.mark(task.id).unwrap_or((" ", Style::new()));
    let mut left = vec![Span::styled(mark, mark_style)];
    match id_width {
        Some(w) => {
            left.push(Span::raw(" "));
            left.push(Span::styled(task.status.glyph(), status));
            left.push(Span::styled(format!(" {:>w$} ", task.id), text));
        }
        None => {
            left.push(Span::styled(task.status.glyph(), status));
            left.push(Span::styled(format!("{} ", task.id), text));
        }
    }

    let mut right = ctx.badges(task);
    if let Some(chip) = ctx.chip(task.id) {
        if !right.is_empty() {
            right.push(Span::raw(" "));
        }
        right.push(chip);
    }
    if id_width.is_some() && !task.effort.is_empty() {
        if !right.is_empty() {
            right.push(Span::raw(" "));
        }
        let effort = super::notice_style(ctx.app, task).unwrap_or(theme.effort);
        right.push(Span::styled(task.effort.clone(), effort));
    }
    (left, right)
}

/// `◆ A ✓ a1b2c3d …`: the checkpoint id, its verification verdict and its
/// short commits. A full row is closed with a rule out to `width`.
fn checkpoint_line(ctx: &Ctx, checkpoint: &Checkpoint, width: usize, ruled: bool) -> Line<'static> {
    let theme = &ctx.app.theme;
    let mut spans = Vec::new();
    if ruled {
        spans.push(Span::styled("╌╌ ", theme.layer_rule));
    }
    spans.push(Span::styled(
        format!("◆ {}", checkpoint.id),
        theme.checkpoint,
    ));
    match checkpoint.verification.as_ref().map(|v| v.outcome.as_str()) {
        Some("pass") => spans.push(Span::styled(" ✓", theme.done)),
        Some(_) => spans.push(Span::styled(" ✗", theme.failed)),
        None => {}
    }
    let commits: Vec<&str> = checkpoint
        .commits
        .iter()
        .map(|sha| sha.get(..7).unwrap_or(sha))
        .collect();
    if !commits.is_empty() {
        spans.push(Span::styled(
            format!(" {}", commits.join(" ")),
            theme.commit,
        ));
    }
    let used: usize = spans.iter().map(Span::width).sum();
    if ruled && used + 1 < width {
        spans.push(Span::styled(
            format!(" {}", "╌".repeat(width - used - 1)),
            theme.layer_rule,
        ));
    }
    Line::from(spans)
}

/// `lead`, the layer's `label` and a rule out to `width` cells.
fn layer_rule(ctx: &Ctx, lead: &str, label: &str, width: usize) -> Line<'static> {
    let theme = &ctx.app.theme;
    let rest = width.saturating_sub(text_width(lead) + text_width(label) + 1);
    Line::from(vec![
        Span::styled(lead.to_string(), theme.layer_rule),
        Span::styled(label.to_string(), theme.layer_label),
        Span::styled(format!(" {}", "─".repeat(rest)), theme.layer_rule),
    ])
}

fn format_elapsed(age: Duration) -> String {
    let secs = age.as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3_600 => format!("{}m", secs / 60),
        _ => format!("{}h{:02}", secs / 3_600, secs % 3_600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::{RecordEntry, fixture};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    fn app() -> App {
        App::new(fixture(), &Config::default())
    }

    /// Twelve minutes after the running agent's segment on task 4 opened.
    fn wall() -> SystemTime {
        UNIX_EPOCH
            + Duration::from_secs(parse_utc("2026-09-28T11:16:22Z").expect("valid timestamp"))
    }

    fn draw(app: &App, orientation: Orientation, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_at(
                    frame.buffer_mut(),
                    area,
                    app,
                    orientation,
                    wall(),
                    Instant::now(),
                );
            })
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn lines(buf: &Buffer) -> Vec<String> {
        let area = buf.area;
        (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn row_of<'l>(lines: &'l [String], needle: &str) -> (usize, &'l String) {
        lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.contains(needle))
            .unwrap_or_else(|| panic!("no row contains {needle:?} in {lines:#?}"))
    }

    #[test]
    fn vertical_render_shows_every_task_and_the_checkpoint_rows() {
        let app = app();
        let lines = lines(&draw(&app, Orientation::Vertical, 100, 30));
        for task in &app.snapshot.tasks {
            row_of(
                &lines,
                &format!("{} {} {}", task.status.glyph(), task.id, task.title),
            );
        }
        let (a, row) = row_of(&lines, "◆ A");
        assert!(row.contains("✓ a1b2c3d b2c3d4e c3d4e5f"), "{row}");
        let (three, _) = row_of(&lines, "Load the config");
        let (l2, _) = row_of(&lines, "── L2 ");
        assert!(three < a && a < l2, "A closes after layer 1: {lines:#?}");
        let (b, row) = row_of(&lines, "◆ B");
        assert!(!row.contains('✓') && !row.contains('✗'), "B is unverified");
        assert!(b > row_of(&lines, "Wire the entry point").0);
    }

    #[test]
    fn horizontal_render_shows_every_task_and_the_checkpoint_rows() {
        let app = app();
        let lines = lines(&draw(&app, Orientation::Horizontal, 160, 20));
        assert!(lines[0].contains("L1 ") && lines[0].contains("L3 "));
        for task in &app.snapshot.tasks {
            row_of(
                &lines,
                &format!("{}{} {}", task.status.glyph(), task.id, task.title),
            );
        }
        let (_, row) = row_of(&lines, "◆ A");
        assert!(row.contains("✓ a1b2c3d"), "{row}");
        row_of(&lines, "◆ B");
    }

    #[test]
    fn gutter_marks_follow_the_selection() {
        let mut app = app();
        app.selected = Some(5);
        let lines = lines(&draw(&app, Orientation::Vertical, 100, 30));
        let mark = |title: &str| row_of(&lines, title).1.chars().next();
        assert_eq!(mark("Style the rows"), Some('▸'));
        assert_eq!(mark("Scaffold the store"), Some('↑'));
        assert_eq!(mark("Define the schema"), Some('↑'));
        assert_eq!(mark("Assemble the app"), Some('↓'));
        assert_eq!(mark("Wire the entry point"), Some('⇣'));
        assert_eq!(mark("Render the rows"), Some('≈'), "shares a file with 5");
        assert_eq!(mark("Bind the keys"), Some(' '));

        app.selected = Some(6);
        let lines = self::lines(&draw(&app, Orientation::Vertical, 100, 30));
        let mark = |title: &str| row_of(&lines, title).1.chars().next();
        assert_eq!(mark("Load the config"), Some('⇡'));
        assert_eq!(mark("Wire the entry point"), Some('↓'));
    }

    #[test]
    fn unrelated_rows_fade_but_keep_their_status_colour() {
        let app = app();
        assert_eq!(app.selected, Some(4));
        let buf = draw(&app, Orientation::Vertical, 100, 30);
        let lines = lines(&buf);
        let cell = |title: &str, x: u16| {
            let y = u16::try_from(row_of(&lines, title).0).expect("fits");
            buf[(x, y)].clone()
        };
        let theme = &app.theme;
        // Column 2 holds the status mark, column 4 the id.
        let unrelated = cell("Define the schema", 4);
        assert_eq!(unrelated.fg, theme.unrelated.fg.expect("a colour"));
        assert!(!unrelated.modifier.contains(Modifier::DIM));
        let mark = cell("Define the schema", 2);
        assert_eq!(mark.symbol(), "✓");
        assert_eq!(mark.fg, theme.done.fg.expect("a colour"));
        assert_ne!(cell("Scaffold the store", 4).fg, unrelated.fg);
        let selected = cell("Render the rows", 4);
        assert_eq!(selected.bg, theme.selection.bg.expect("a colour"));
        assert!(!selected.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn a_flash_takes_the_new_status_colour_and_a_row_ends_padded() {
        let mut app = app();
        app.selected = Some(1);
        app.flashes.insert(4, Instant::now());
        let buf = draw(&app, Orientation::Vertical, 100, 30);
        let lines = lines(&buf);
        let y = u16::try_from(row_of(&lines, "Render the rows").0).expect("fits");
        assert_eq!(
            buf[(10, y)].bg,
            app.theme.flash(TaskStatus::InProgress).bg.unwrap()
        );
        assert_eq!(buf[(99, y)].symbol(), " ", "the last cell is padding");
    }

    #[test]
    fn a_running_agent_shows_a_chip_dimmed_while_stale() {
        let mut app = app();
        let buf = draw(&app, Orientation::Vertical, 100, 30);
        let lines = lines(&buf);
        let (y, row) = row_of(&lines, "Render the rows");
        let x = row.chars().position(|c| c == 'I').expect("chip initials");
        let (x, y) = (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
        assert!(row.contains(" ID 12m "), "{row}");
        assert!(buf[(x, y)].modifier.contains(Modifier::DIM), "A2 is stale");
        assert!(
            !row_of(&lines, "Load the config").1.contains(" IL "),
            "idle A1 has no chip"
        );

        app.stale_agents.clear();
        let buf = draw(&app, Orientation::Vertical, 100, 30);
        assert!(!buf[(x, y)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn the_selection_tint_leaves_the_agent_chip_its_background() {
        let app = app();
        let buf = draw(&app, Orientation::Vertical, 100, 30);
        let lines = lines(&buf);
        let (y, row) = row_of(&lines, "Render the rows");
        let x = row.chars().position(|c| c == 'I').expect("chip initials");
        let (x, y) = (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
        assert_eq!(buf[(x, y)].bg, app.theme.agent_chip.bg.expect("a colour"));
        assert_eq!(buf[(10, y)].bg, app.theme.selection.bg.expect("a colour"));
    }

    #[test]
    fn effort_takes_the_colour_of_the_worst_notice() {
        let mut snap = fixture();
        let wire = snap.tasks.iter_mut().find(|t| t.id == 8).expect("task 8");
        wire.status = TaskStatus::Failed;
        let mut app = App::new(snap, &Config::default());
        app.selected = None;
        let buf = draw(&app, Orientation::Vertical, 100, 30);
        let lines = lines(&buf);
        // The effort sits in the last cell before the row's padding.
        let effort = |title: &str| {
            let y = u16::try_from(row_of(&lines, title).0).expect("fits");
            buf[(98, y)].clone()
        };
        let theme = &app.theme;
        assert_eq!(Some(effort("Define the schema").fg), theme.effort.fg);
        assert_eq!(
            Some(effort("Load the config").fg),
            theme.effort_warning.fg,
            "a deviation"
        );
        assert_eq!(
            Some(effort("Wire the entry point").fg),
            theme.effort_danger.fg
        );
    }

    #[test]
    fn a_deviation_is_not_badged_on_its_row() {
        let mut app = app();
        app.selected = None;
        let lines = lines(&draw(&app, Orientation::Vertical, 100, 30));
        assert!(!row_of(&lines, "Load the config").1.contains('⚠'));
    }

    #[test]
    fn a_deferral_draws_one_pause_glyph_whatever_the_status() {
        let mut snap = fixture();
        let (id, title) = (snap.tasks[6].id, snap.tasks[6].title.clone());
        snap.record.push(RecordEntry {
            entry_type: "deferral".to_string(),
            task_id: Some(id),
            ..RecordEntry::default()
        });
        let pauses = |snap: Snapshot| {
            let mut app = App::new(snap, &Config::default());
            app.selected = None;
            let lines = lines(&draw(&app, Orientation::Vertical, 100, 30));
            row_of(&lines, &title).1.matches('⏸').count()
        };
        assert_eq!(pauses(snap.clone()), 1, "a pending row carries the badge");
        snap.tasks[6].status = TaskStatus::Deferred;
        assert_eq!(pauses(snap), 1, "a deferred row drops it");
    }

    #[test]
    fn vertical_navigation_reads_down_and_jumps_across() {
        let app = app();
        let nav = navigator(&app.snapshot, &app.index, Orientation::Vertical);
        assert_eq!(nav.neighbor(3, Dir::Down), Some(4));
        assert_eq!(nav.neighbor(4, Dir::Up), Some(3));
        assert_eq!(nav.neighbor(1, Dir::Up), None);
        assert_eq!(nav.neighbor(8, Dir::Down), None);
        assert_eq!(nav.neighbor(2, Dir::Right), Some(5));
        assert_eq!(nav.neighbor(6, Dir::Right), Some(8), "slot clamps");
        assert_eq!(nav.neighbor(1, Dir::Left), None);
        assert_eq!(nav.neighbor(99, Dir::Down), None);
    }

    #[test]
    fn horizontal_navigation_stays_in_its_column() {
        let app = app();
        let nav = navigator(&app.snapshot, &app.index, Orientation::Horizontal);
        assert_eq!(nav.neighbor(4, Dir::Down), Some(5));
        assert_eq!(nav.neighbor(6, Dir::Down), None);
        assert_eq!(nav.neighbor(4, Dir::Up), None);
        assert_eq!(nav.neighbor(2, Dir::Right), Some(5));
        assert_eq!(nav.neighbor(8, Dir::Left), Some(5));
        assert_eq!(nav.neighbor(8, Dir::Right), None);
    }

    #[test]
    fn scrolling_keeps_the_selection_visible() {
        let mut app = app();
        app.selected = Some(8);
        let lines = lines(&draw(&app, Orientation::Vertical, 60, 6));
        assert!(
            lines.iter().any(|l| l.contains("Wire the entry point")),
            "{lines:#?}"
        );
        assert!(!lines.iter().any(|l| l.contains("── L1 ")));

        app.selected = Some(1);
        let lines = self::lines(&draw(&app, Orientation::Vertical, 60, 6));
        assert!(lines[0].contains("── L1 "));
    }

    #[test]
    fn follow_leads_with_what_comes_next_and_a_step_moves_only_as_needed() {
        assert_eq!(place(0, Some(20), 30, 9, Place::Jump), 17, "a third in");
        assert_eq!(place(0, Some(28), 30, 9, Place::Jump), 21, "clamped");
        assert_eq!(place(10, Some(12), 30, 9, Place::Step), 10, "no need");
        assert_eq!(place(10, Some(16), 30, 9, Place::Step), 11, "3 ahead");
        assert_eq!(place(10, Some(10), 30, 9, Place::Step), 8, "2 behind");
        assert_eq!(place(25, Some(3), 30, 9, Place::Pinned), 21);
        assert_eq!(place(4, Some(3), 5, 9, Place::Jump), 0, "all fits");
    }

    #[test]
    fn a_pinned_list_stays_where_the_wheel_left_it() {
        let mut app = app();
        app.selected = Some(1);
        app.scroll_pinned = true;
        app.scroll_nudge = (0, 4);
        let area = Rect::new(0, 0, 60, 6);
        let mut buf = Buffer::empty(area);
        let Drawn {
            targets, scroll, ..
        } = render_at(
            &mut buf,
            area,
            &app,
            Orientation::Vertical,
            wall(),
            Instant::now(),
        );
        assert_eq!(scroll.offset, 4);
        assert!(targets.iter().all(|(_, id)| *id != 1), "1 scrolled off");
        assert_eq!(app.selected, Some(1));
    }

    #[test]
    fn narrow_columns_scroll_to_the_selected_layer() {
        let mut app = app();
        app.selected = Some(8);
        let lines = lines(&draw(&app, Orientation::Horizontal, 40, 10));
        assert!(lines[0].contains("L3 "), "{lines:#?}");
        assert!(!lines[0].contains("L1 "));
    }

    #[test]
    fn columns_fit_the_longest_title_within_the_configured_max() {
        let mut app = app();
        app.stale_agents.clear();
        let ctx = Ctx::new(&app, wall(), Instant::now());
        let widest = app
            .snapshot
            .tasks
            .iter()
            .map(|task| natural_width(&ctx, task, None))
            .max()
            .expect("tasks");
        let expected = u16::try_from(widest + 1).expect("fits");
        assert!(
            expected > *COLUMN_RANGE.start() && expected < 40,
            "{expected}"
        );
        assert_eq!(column_for(&ctx), expected, "sized to content plus gutter");

        app.column_max = 20;
        assert_eq!(column_for(&Ctx::new(&app, wall(), Instant::now())), 20);
        app.column_max = 5;
        assert_eq!(
            column_for(&Ctx::new(&app, wall(), Instant::now())),
            *COLUMN_RANGE.start(),
            "never below the narrowest column"
        );
        app.column_override = Some(33);
        assert_eq!(column_for(&Ctx::new(&app, wall(), Instant::now())), 33);
    }

    #[test]
    fn a_wide_pane_shows_whole_titles_and_leaves_the_rest_blank() {
        let app = app();
        let lines = lines(&draw(&app, Orientation::Horizontal, 200, 12));
        let longest = app
            .snapshot
            .tasks
            .iter()
            .map(|task| task.title.as_str())
            .max_by_key(|title| title.len())
            .expect("tasks");
        row_of(&lines, longest);
        assert!(
            !lines[0].contains('›') && !lines[0].contains('‹'),
            "{lines:#?}"
        );
    }

    #[test]
    fn off_screen_layers_are_marked_on_both_sides() {
        let mut app = app();
        app.column_override = Some(30);
        app.selected = Some(1);
        let lines = lines(&draw(&app, Orientation::Horizontal, 60, 10));
        assert!(lines[0].starts_with("L1 "), "{lines:#?}");
        assert!(lines[0].trim_end().ends_with('›'), "{lines:#?}");
        assert!(!lines[0].contains('‹'));

        app.selected = Some(8);
        let lines = self::lines(&draw(&app, Orientation::Horizontal, 60, 10));
        assert!(lines[0].starts_with("‹ L2 "), "{lines:#?}");
        assert!(!lines[0].contains('›'));
    }

    #[test]
    fn render_reports_where_each_task_row_landed() {
        let app = app();
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).expect("terminal");
        let mut targets = Vec::new();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 2, 60, 28);
                targets = render_at(
                    frame.buffer_mut(),
                    area,
                    &app,
                    Orientation::Vertical,
                    wall(),
                    Instant::now(),
                )
                .targets;
            })
            .expect("draw");
        assert_eq!(targets.len(), app.snapshot.tasks.len());
        let buf = terminal.backend().buffer().clone();
        let text = |rect: Rect| {
            (rect.x..rect.right())
                .map(|x| buf[(x, rect.y)].symbol())
                .collect::<String>()
        };
        for (rect, id) in &targets {
            let title = &app
                .snapshot
                .tasks
                .iter()
                .find(|t| t.id == *id)
                .expect("task")
                .title;
            assert!(text(*rect).contains(title.as_str()), "row for {id}");
        }
    }

    #[test]
    fn chip_and_title_helpers_are_compact() {
        assert_eq!(format_elapsed(Duration::from_secs(3_725)), "1h02");
        assert_eq!(initials("implement-deep"), "ID");
        assert_eq!(truncate("Render the rows", 8), "Render …");
    }
}
