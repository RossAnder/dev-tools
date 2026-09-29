//! The traversal view: the selected task in a centre card, what it waits on to
//! one side and what waits on it to the other.
//!
//! Horizontal puts upstream on the left and downstream on the right; vertical
//! puts upstream above and downstream below. Upstream is the task's own
//! `needs`, then its `coupling`; downstream is the rows naming it in theirs.
//! Coupling entries are drawn dashed. Every band is as long as its content and
//! the whole stands at the top of the pane; rows left over go to the card's
//! action summary, then to each side's second hop.
//!
//! Moving across selects that side's first entry and records the crossing in
//! `App::trail`. Moving along then walks the entries of the side just entered,
//! and moving back across returns to where the crossing started. With no trail,
//! moving along walks the centre's layer. The card's bottom border names which.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};

use super::layers::Target;
use crate::app::{App, Crossing, Dir, Navigator, Side};
use crate::config::{Density, Orientation};
use crate::model::{AgentStatus, Index, Snapshot};

/// Blank columns between a first-hop column and its second-hop column.
const GAP: u16 = 2;
/// Most rows the card's action summary and files take, by density.
const SUMMARY_COMPACT: u16 = 3;
const SUMMARY_COMFORTABLE: u16 = 8;
/// Widest the vertical stack grows at comfortable density; compact takes the full width.
const STACK_MAX: u16 = 72;
/// Widest the horizontal card grows, by density.
const CARD_MAX_COMPACT: u16 = 32;
const CARD_MAX_COMFORTABLE: u16 = 44;
const HOP_HEADING: &str = "2 hops";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    id: u32,
    coupling: bool,
}

/// `id`'s entries on `side`: its needs then coupling upstream, the rows naming
/// it downstream.
fn side_of(snap: &Snapshot, index: &Index, id: u32, side: Side) -> Vec<Entry> {
    match side {
        Side::Needs => index
            .task(snap, id)
            .map_or_else(Vec::new, |task| merge(&task.needs, &task.coupling)),
        Side::Dependents => merge(index.dependents(id), index.coupled(id)),
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Needs => "needs",
        Side::Dependents => "dependents",
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

/// The tasks two hops out on `side`, less the centre and the first hop.
fn second_hop(
    snap: &Snapshot,
    index: &Index,
    centre: u32,
    first: &[Entry],
    side: Side,
) -> Vec<u32> {
    let mut ids: Vec<u32> = first
        .iter()
        .flat_map(|entry| side_of(snap, index, entry.id, side))
        .map(|entry| entry.id)
        .filter(|id| *id != centre && !first.iter().any(|entry| entry.id == *id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The longest tail of `trail` whose crossings still lead, one side at a time,
/// to `centre` in this snapshot. A topology change or a stale trail shortens it.
fn live_trail(snap: &Snapshot, index: &Index, trail: &[Crossing], centre: u32) -> Vec<Crossing> {
    let mut to = centre;
    let mut keep = trail.len();
    for (at, crossing) in trail.iter().enumerate().rev() {
        let leads = side_of(snap, index, crossing.origin, crossing.side)
            .iter()
            .any(|entry| entry.id == to);
        if !leads {
            break;
        }
        to = crossing.origin;
        keep = at;
    }
    trail[keep..].to_vec()
}

/// The selection, when it still names a task in the snapshot.
fn centre(app: &App) -> Option<u32> {
    app.selected
        .filter(|id| app.index.task(&app.snapshot, *id).is_some())
}

/// Everything one frame draws around its centre, and what each move reaches.
struct Traversal {
    centre: u32,
    up: Vec<Entry>,
    down: Vec<Entry>,
    up2: Vec<u32>,
    down2: Vec<u32>,
    /// What moving along walks: the side the trail last entered, else the layer.
    axis: Vec<u32>,
    axis_name: String,
    trail: Vec<Crossing>,
}

impl Traversal {
    fn of(app: &App) -> Option<Traversal> {
        let centre = centre(app)?;
        let (snap, index) = (&app.snapshot, &app.index);
        let trail = live_trail(snap, index, &app.trail, centre);
        let up = side_of(snap, index, centre, Side::Needs);
        let down = side_of(snap, index, centre, Side::Dependents);
        let (axis, axis_name) = match trail.last() {
            Some(last) => (
                side_of(snap, index, last.origin, last.side)
                    .iter()
                    .map(|entry| entry.id)
                    .collect(),
                format!("{} of {}", side_name(last.side), last.origin),
            ),
            None => (
                peers(snap, index, centre),
                index
                    .layer_of(centre)
                    .map_or_else(String::new, |layer| format!("layer {}", layer + 1)),
            ),
        };
        Some(Traversal {
            up2: second_hop(snap, index, centre, &up, Side::Needs),
            down2: second_hop(snap, index, centre, &down, Side::Dependents),
            centre,
            up,
            down,
            axis,
            axis_name,
            trail,
        })
    }

    fn along(&self, forward: bool) -> Option<u32> {
        let pos = self.axis.iter().position(|&id| id == self.centre)?;
        if forward {
            self.axis.get(pos + 1).copied()
        } else {
            pos.checked_sub(1).and_then(|p| self.axis.get(p)).copied()
        }
    }

    /// Back to where the last crossing started when `side` points that way,
    /// else into the first entry on `side`.
    fn cross(&self, side: Side) -> Option<(u32, Vec<Crossing>)> {
        if let Some((last, rest)) = self.trail.split_last()
            && last.side != side
        {
            return Some((last.origin, rest.to_vec()));
        }
        let entries = match side {
            Side::Needs => &self.up,
            Side::Dependents => &self.down,
        };
        let first = entries.first()?;
        let mut trail = self.trail.clone();
        trail.push(Crossing {
            origin: self.centre,
            side,
        });
        Some((first.id, trail))
    }

    /// What moving along walks, and where the centre sits in it.
    fn axis_label(&self) -> String {
        match self.axis.iter().position(|&id| id == self.centre) {
            Some(pos) if self.axis.len() > 1 => {
                format!("{} · {}/{}", self.axis_name, pos + 1, self.axis.len())
            }
            _ => self.axis_name.clone(),
        }
    }
}

/// Moves over one frame's traversal. It answers only for the centre it was
/// built around; any other `from` has no neighbour.
pub(crate) struct EgoNavigator {
    orientation: Orientation,
    traversal: Option<Traversal>,
}

/// The navigator matching what [`render`] draws for the same `app` and
/// `orientation`.
pub(crate) fn navigator(app: &App, orientation: Orientation) -> EgoNavigator {
    EgoNavigator {
        orientation,
        traversal: Traversal::of(app),
    }
}

impl Navigator for EgoNavigator {
    fn neighbor(&self, from: u32, dir: Dir) -> Option<u32> {
        self.walk(from, dir).map(|(to, _)| to)
    }

    fn walk(&self, from: u32, dir: Dir) -> Option<(u32, Vec<Crossing>)> {
        let traversal = self.traversal.as_ref().filter(|t| t.centre == from)?;
        let (upstream, downstream, prev) = match self.orientation {
            Orientation::Horizontal => (Dir::Left, Dir::Right, Dir::Up),
            Orientation::Vertical => (Dir::Up, Dir::Down, Dir::Left),
        };
        if dir == upstream {
            traversal.cross(Side::Needs)
        } else if dir == downstream {
            traversal.cross(Side::Dependents)
        } else {
            let to = traversal.along(dir != prev)?;
            Some((to, traversal.trail.clone()))
        }
    }
}

/// Draws the traversal around the selection and returns a click target for
/// every task it names outside the card.
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    orientation: Orientation,
) -> Vec<Target> {
    let Some(traversal) = Traversal::of(app) else {
        frame.render_widget(
            Paragraph::new(Line::styled("no task selected", app.theme.pending)),
            area,
        );
        return Vec::new();
    };
    let compact = app.resolved_density == Density::Compact;
    let now = Instant::now();
    let mut draw = Draw {
        frame,
        area,
        app,
        now,
        targets: Vec::new(),
    };
    match orientation {
        Orientation::Horizontal => horizontal(&mut draw, &traversal, compact),
        Orientation::Vertical => vertical(&mut draw, &traversal, compact),
    }
    draw.targets
}

/// One frame's drawing state: every row is clipped to `area`, and each task row
/// drawn adds its rect to `targets`.
struct Draw<'f, 'b, 'a> {
    frame: &'f mut Frame<'b>,
    area: Rect,
    app: &'a App,
    now: Instant,
    targets: Vec<Target>,
}

impl Draw<'_, '_, '_> {
    fn line(&mut self, x: u16, y: u16, width: u16, line: Line<'_>) {
        let rect = Rect::new(x, y, width, 1).intersection(self.area);
        if !rect.is_empty() {
            self.frame.render_widget(Paragraph::new(line), rect);
        }
    }

    fn target(&mut self, x: u16, y: u16, width: u16, id: u32) {
        let rect = Rect::new(x, y, width, 1).intersection(self.area);
        if !rect.is_empty() {
            self.targets.push((rect, id));
        }
    }

    fn task_style(&self, id: u32) -> Style {
        let app = self.app;
        if app.is_flashing(id, self.now) {
            return app.theme.flash;
        }
        app.index
            .task(&app.snapshot, id)
            .map_or(app.theme.pending, |task| {
                app.theme.status(task.status.as_str())
            })
    }

    fn pending(&self) -> Style {
        self.app.theme.pending
    }
}

fn label(app: &App, id: u32) -> String {
    match app.index.task(&app.snapshot, id) {
        Some(task) => format!("{} {} {}", task.status.glyph(), task.id, task.title),
        None => format!("? {id}"),
    }
}

/// A task's glyph and id alone, for the second-hop row.
fn chip(app: &App, id: u32) -> String {
    match app.index.task(&app.snapshot, id) {
        Some(task) => format!("{} {}", task.status.glyph(), task.id),
        None => format!("? {id}"),
    }
}

fn edge_style(app: &App, entry: Entry) -> Style {
    if entry.coupling {
        app.theme.coupling_edge
    } else {
        app.theme.needs_edge
    }
}

fn cells(text: &str) -> u16 {
    u16::try_from(Span::raw(text).width()).unwrap_or(u16::MAX)
}

fn count(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
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

/// Greedy word wrap into at most `rows` rows of `width` characters. The last
/// row ends in `…` when words were left over; a word longer than a row is cut.
fn wrap(text: &str, width: usize, rows: usize) -> Vec<String> {
    if width == 0 || rows == 0 {
        return Vec::new();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut next = 0;
    while next < words.len() && out.len() < rows {
        let word = words[next];
        let used = line.chars().count();
        let sep = usize::from(used > 0);
        if used + sep + word.chars().count() <= width {
            if sep == 1 {
                line.push(' ');
            }
            line.push_str(word);
            next += 1;
        } else if used == 0 {
            out.push(truncate(word, width));
            next += 1;
        } else {
            out.push(std::mem::take(&mut line));
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    if next < words.len()
        && let Some(last) = out.last_mut()
    {
        let kept: String = last.chars().take(width - 1).collect();
        *last = format!("{kept}…");
    }
    out
}

/// Markdown emphasis and code marks dropped, for a one-glance summary.
fn plain(markdown: &str) -> String {
    markdown.replace("**", "").replace('`', "")
}

fn summary_max(compact: bool) -> u16 {
    if compact {
        SUMMARY_COMPACT
    } else {
        SUMMARY_COMFORTABLE
    }
}

/// The card's rows inside its border: status, the title over at most two rows,
/// the newest agent, then up to `summary` rows of action text and files.
fn card_lines(app: &App, id: u32, width: usize, summary: u16) -> Vec<Line<'static>> {
    let Some(task) = app.index.task(&app.snapshot, id) else {
        return Vec::new();
    };
    let theme = &app.theme;
    let mut status = vec![Span::styled(
        format!("{} {}", task.status.glyph(), task.status.as_str()),
        theme.status(task.status.as_str()),
    )];
    if !task.effort.is_empty() {
        status.push(Span::raw(format!("  {}", task.effort)));
    }
    if !task.checkpoint.is_empty() {
        status.push(Span::raw(format!("  cp {}", task.checkpoint)));
    }
    let mut lines = vec![Line::from(status)];
    lines.extend(
        wrap(&task.title, width, 2)
            .into_iter()
            .map(|row| Line::styled(row, theme.badge)),
    );
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

    let summary = usize::from(summary);
    let action = plain(&task.action);
    let files = (!task.files.is_empty()).then(|| format!("files {}", task.files.join(", ")));
    let files_row = files.is_some() && summary > 0 && (summary >= 2 || action.trim().is_empty());
    lines.extend(
        wrap(&action, width, summary - usize::from(files_row))
            .into_iter()
            .map(Line::raw),
    );
    if let Some(files) = files.filter(|_| files_row) {
        lines.push(Line::styled(truncate(&files, width), theme.pending));
    }
    lines
}

fn draw_card(draw: &mut Draw, rect: Rect, traversal: &Traversal, lines: Vec<Line<'static>>) {
    let rect = rect.intersection(draw.area);
    if rect.is_empty() {
        return;
    }
    let theme = &draw.app.theme;
    let id = traversal.centre;
    let border = if draw.app.is_flashing(id, draw.now) {
        theme.flash
    } else {
        draw.app
            .index
            .task(&draw.app.snapshot, id)
            .map_or(theme.pending, |task| theme.status(task.status.as_str()))
    };
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Line::from(Span::styled(format!(" {id} "), theme.selection)));
    let axis = traversal.axis_label();
    if !axis.is_empty() {
        block =
            block.title_bottom(Line::styled(format!(" {axis} "), theme.pending).right_aligned());
    }
    draw.frame
        .render_widget(Paragraph::new(lines).block(block), rect);
}

/// The entries that fit in `slots`, and how many were left out. When some
/// are left out, the last slot is reserved for the `+N more` marker.
fn visible<T>(entries: &[T], slots: usize) -> (&[T], usize) {
    if entries.len() <= slots {
        return (entries, 0);
    }
    let shown = slots.saturating_sub(1);
    (&entries[..shown], entries.len() - shown)
}

/// Splits `spare` rows between two lists wanting `a` and `b` rows: each gets
/// what it wants when both fit, else one row each and the rest in turn.
fn share(spare: u16, a: usize, b: usize) -> (u16, u16) {
    let (a, b) = (count(a), count(b));
    if a.saturating_add(b) <= spare {
        return (a, b);
    }
    let mut got = (spare.min(1), spare.saturating_sub(1).min(1));
    let mut left = spare - got.0 - got.1;
    while left > 0 && (got.0 < a || got.1 < b) {
        if got.0 < a && (got.0 <= got.1 || got.1 >= b) {
            got.0 += 1;
        } else {
            got.1 += 1;
        }
        left -= 1;
    }
    got
}

/// Needs, card, a row for the along-axis neighbours and dependents, stacked
/// from the top. Compact spans the full width; comfortable is capped and
/// centred.
fn vertical(draw: &mut Draw, traversal: &Traversal, compact: bool) {
    let area = draw.area;
    let width = if compact {
        area.width
    } else {
        area.width.min(STACK_MAX)
    };
    let x = area.x + (area.width - width) / 2;
    let inner = usize::from(width.saturating_sub(2));
    let app = draw.app;
    let base = count(card_lines(app, traversal.centre, inner, 0).len()) + 2;
    let along = u16::from(traversal.axis.len() > 1);
    let headings = 2;
    let mut spare = area.height.saturating_sub(base + headings + along);
    let (need_rows, dep_rows) = share(
        spare,
        traversal.up.len().max(1),
        traversal.down.len().max(1),
    );
    spare -= need_rows + dep_rows;
    let lines = card_lines(
        app,
        traversal.centre,
        inner,
        spare.min(summary_max(compact)),
    );
    let card_h = count(lines.len()) + 2;
    spare = spare.saturating_sub(card_h - base);
    let hop_up = !traversal.up2.is_empty() && spare > 0;
    spare = spare.saturating_sub(u16::from(hop_up));
    let hop_down = !traversal.down2.is_empty() && spare > 0;

    let mut y = area.y;
    if hop_up {
        hop_row(draw, x, y, width, &traversal.up2);
        y = y.saturating_add(1);
    }
    draw.line(x, y, width, Line::styled("needs", draw.pending()));
    y = y.saturating_add(1);
    list(draw, x, y, width, &traversal.up, need_rows);
    y = y.saturating_add(need_rows);
    draw_card(draw, Rect::new(x, y, width, card_h), traversal, lines);
    y = y.saturating_add(card_h);
    if along == 1 {
        along_row(draw, traversal, x, y, width);
        y = y.saturating_add(1);
    }
    draw.line(x, y, width, Line::styled("dependents", draw.pending()));
    y = y.saturating_add(1);
    list(draw, x, y, width, &traversal.down, dep_rows);
    y = y.saturating_add(dep_rows);
    if hop_down {
        hop_row(draw, x, y, width, &traversal.down2);
    }
}

/// One row per entry behind a gutter, `│` for needs and `┆` for coupling, in
/// at most `slots` rows.
fn list(draw: &mut Draw, x: u16, y: u16, width: u16, entries: &[Entry], slots: u16) {
    if slots == 0 {
        return;
    }
    if entries.is_empty() {
        draw.line(x, y, width, Line::styled("none", draw.pending()));
        return;
    }
    let text_w = usize::from(width.saturating_sub(2));
    let (shown, hidden) = visible(entries, usize::from(slots));
    for (row, entry) in (y..).zip(shown) {
        let gutter = if entry.coupling { "┆ " } else { "│ " };
        let line = Line::from(vec![
            Span::styled(gutter, edge_style(draw.app, *entry)),
            Span::styled(
                truncate(&label(draw.app, entry.id), text_w),
                draw.task_style(entry.id),
            ),
        ]);
        draw.line(x, row, width, line);
        draw.target(x, row, width, entry.id);
    }
    if hidden > 0 {
        let row = y.saturating_add(count(shown.len()));
        draw.line(
            x,
            row,
            width,
            Line::styled(format!("+{hidden} more"), draw.pending()),
        );
    }
}

/// A side's second hop as `glyph id` chips, as many as fit, then `+N`.
fn hop_row(draw: &mut Draw, x: u16, y: u16, width: u16, ids: &[u32]) {
    let lead = format!("{HOP_HEADING} ");
    let mut used = cells(&lead);
    let mut spans = vec![Span::styled(lead, draw.pending())];
    for (at, id) in ids.iter().enumerate() {
        let sep = if at > 0 { 2 } else { 0 };
        let chip = chip(draw.app, *id);
        let chip_w = cells(&chip);
        if used.saturating_add(sep + chip_w) > width {
            spans.push(Span::styled(
                format!("  +{}", ids.len() - at),
                draw.pending(),
            ));
            break;
        }
        spans.push(Span::raw(" ".repeat(usize::from(sep))));
        draw.target(x + used + sep, y, chip_w, *id);
        spans.push(Span::styled(chip, draw.task_style(*id)));
        used += sep + chip_w;
    }
    draw.line(x, y, width, Line::from(spans));
}

/// The previous and next task along the axis, at either end of one row.
fn along_row(draw: &mut Draw, traversal: &Traversal, x: u16, y: u16, width: u16) {
    let half = width.saturating_sub(1) / 2;
    let text_w = usize::from(half);
    let label_w = text_w.saturating_sub(2);
    if let Some(prev) = traversal.along(false) {
        let text = format!("◀ {}", truncate(&label(draw.app, prev), label_w));
        draw.line(x, y, half, Line::styled(text, draw.task_style(prev)));
        draw.target(x, y, half, prev);
    }
    if let Some(next) = traversal.along(true) {
        let text = format!("{} ▶", truncate(&label(draw.app, next), label_w));
        let at = x + width - half;
        draw.line(
            at,
            y,
            half,
            Line::styled(format!("{text:>text_w$}"), draw.task_style(next)),
        );
        draw.target(at, y, half, next);
    }
}

/// The card centred across the pane at its top, the along-axis neighbours
/// above and below it, and each side's columns hugging it, sized to their
/// longest entry. A second hop gets its own column beyond the first where
/// there is room.
fn horizontal(draw: &mut Draw, traversal: &Traversal, compact: bool) {
    let area = draw.area;
    let app = draw.app;
    let card_max = if compact {
        CARD_MAX_COMPACT
    } else {
        CARD_MAX_COMFORTABLE
    };
    let card_w = (area.width / 3).clamp(16, card_max).min(area.width);
    let inner = usize::from(card_w.saturating_sub(2));
    let above = u16::from(traversal.axis.len() > 1);
    let base = count(card_lines(app, traversal.centre, inner, 0).len()) + 2;
    let summary = area
        .height
        .saturating_sub(base + 2 * above)
        .min(summary_max(compact));
    let lines = card_lines(app, traversal.centre, inner, summary);
    let card_h = (count(lines.len()) + 2).min(area.height.saturating_sub(above));
    let left_room = (area.width - card_w) / 2;
    let right_room = area.width - card_w - left_room;
    let card = Rect::new(area.x + left_room, area.y + above, card_w, card_h);
    draw_card(draw, card, traversal, lines);

    if let Some(prev) = traversal.along(false)
        && card.y > area.y
    {
        let text = truncate(&format!("▲ {}", label(app, prev)), inner + 2);
        draw.line(
            card.x,
            card.y - 1,
            card_w,
            Line::styled(text, draw.task_style(prev)),
        );
        draw.target(card.x, card.y - 1, card_w, prev);
    }
    if let Some(next) = traversal.along(true)
        && card.bottom() < area.bottom()
    {
        let text = truncate(&format!("▼ {}", label(app, next)), inner + 2);
        draw.line(
            card.x,
            card.bottom(),
            card_w,
            Line::styled(text, draw.task_style(next)),
        );
        draw.target(card.x, card.bottom(), card_w, next);
    }

    let height = area.bottom().saturating_sub(card.y);
    let left_w = column_width(app, &traversal.up, "needs").min(left_room);
    let right_w = column_width(app, &traversal.down, "dependents").min(right_room);
    let left = Rect::new(card.x - left_w, card.y, left_w, height);
    let right = Rect::new(card.right(), card.y, right_w, height);
    side_column(draw, left, "needs", &traversal.up, true);
    side_column(draw, right, "dependents", &traversal.down, false);

    let left_spare = left_room - left_w;
    if !traversal.up2.is_empty() && left_spare >= GAP + 8 {
        let w = hop_width(app, &traversal.up2).min(left_spare - GAP);
        let rect = Rect::new(left.x - GAP - w, card.y, w, height);
        hop_column(draw, rect, &traversal.up2, true);
    }
    let right_spare = right_room - right_w;
    if !traversal.down2.is_empty() && right_spare >= GAP + 8 {
        let w = hop_width(app, &traversal.down2).min(right_spare - GAP);
        let rect = Rect::new(right.right() + GAP, card.y, w, height);
        hop_column(draw, rect, &traversal.down2, false);
    }
}

/// Wide enough for the longest of the heading and the labels, plus the arrow.
fn column_width(app: &App, entries: &[Entry], heading: &str) -> u16 {
    entries
        .iter()
        .map(|entry| cells(&label(app, entry.id)))
        .max()
        .unwrap_or(0)
        .max(cells(heading))
        .max(cells("none"))
        .saturating_add(3)
}

fn hop_width(app: &App, ids: &[u32]) -> u16 {
    ids.iter()
        .map(|id| cells(&label(app, *id)))
        .max()
        .unwrap_or(0)
        .max(cells(HOP_HEADING))
}

/// A heading then one row per entry, against the card's edge: the left column
/// is right-aligned with a trailing arrow, the right one leads with it, and the
/// heading lines up with the labels rather than the arrows.
fn side_column(draw: &mut Draw, rect: Rect, heading: &str, entries: &[Entry], left: bool) {
    if rect.width < 4 || rect.height == 0 {
        return;
    }
    let text_w = usize::from(rect.width) - 3;
    let align = |text: String| {
        if left {
            format!("{text:>text_w$}")
        } else {
            format!("   {text}")
        }
    };
    draw.line(
        rect.x,
        rect.y,
        rect.width,
        Line::styled(align(heading.to_string()), draw.pending()),
    );
    if entries.is_empty() {
        draw.line(
            rect.x,
            rect.y + 1,
            rect.width,
            Line::styled(align("none".to_string()), draw.pending()),
        );
        return;
    }
    let (shown, hidden) = visible(entries, usize::from(rect.height - 1));
    for (row, entry) in (rect.y + 1..).zip(shown) {
        let text = truncate(&label(draw.app, entry.id), text_w);
        let arrow = Span::styled(
            if entry.coupling { "┄▶" } else { "─▶" },
            edge_style(draw.app, *entry),
        );
        let style = draw.task_style(entry.id);
        let line = if left {
            Line::from(vec![
                Span::styled(format!("{text:>text_w$}"), style),
                Span::raw(" "),
                arrow,
            ])
        } else {
            Line::from(vec![arrow, Span::raw(" "), Span::styled(text, style)])
        };
        draw.line(rect.x, row, rect.width, line);
        draw.target(rect.x, row, rect.width, entry.id);
    }
    if hidden > 0 {
        let row = rect.y + 1 + count(shown.len());
        draw.line(
            rect.x,
            row,
            rect.width,
            Line::styled(align(format!("+{hidden} more")), draw.pending()),
        );
    }
}

/// A second hop as a column of labels under its heading, aligned like its side.
fn hop_column(draw: &mut Draw, rect: Rect, ids: &[u32], left: bool) {
    if rect.width < 4 || rect.height == 0 {
        return;
    }
    let width = usize::from(rect.width);
    let align = |text: String| {
        if left {
            format!("{text:>width$}")
        } else {
            text
        }
    };
    draw.line(
        rect.x,
        rect.y,
        rect.width,
        Line::styled(align(HOP_HEADING.to_string()), draw.pending()),
    );
    let (shown, hidden) = visible(ids, usize::from(rect.height - 1));
    for (row, id) in (rect.y + 1..).zip(shown) {
        let text = align(truncate(&label(draw.app, *id), width));
        draw.line(
            rect.x,
            row,
            rect.width,
            Line::styled(text, draw.task_style(*id)),
        );
        draw.target(rect.x, row, rect.width, *id);
    }
    if hidden > 0 {
        let row = rect.y + 1 + count(shown.len());
        draw.line(
            rect.x,
            row,
            rect.width,
            Line::styled(align(format!("+{hidden} more")), draw.pending()),
        );
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

    fn draw_targets(
        app: &App,
        width: u16,
        height: u16,
        orientation: Orientation,
    ) -> (Vec<String>, Vec<Target>) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        let mut targets = Vec::new();
        terminal
            .draw(|frame| targets = render(frame, frame.area(), app, orientation))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let rows = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect();
        (rows, targets)
    }

    fn draw(app: &App, width: u16, height: u16, orientation: Orientation) -> Vec<String> {
        draw_targets(app, width, height, orientation).0
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

    /// Installs this frame's navigator, as a frame would, then moves.
    fn step(app: &mut App, orientation: Orientation, dir: Dir) {
        app.nav = Some(Box::new(navigator(app, orientation)));
        app.apply(Action::Move(dir));
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
        assert!(
            row_of(&rows, "1 Scaffold").contains("─▶│"),
            "the needs column hugs the card"
        );
        assert!(row_of(&rows, "7 Assemble").contains("│─▶"));
        assert!(rows.iter().any(|row| row.contains("◐ in-progress")));
        assert!(rows.iter().any(|row| row.contains("implement-deep")));
        assert!(
            row_of(&rows, "5 Style the rows").contains('▼'),
            "the next layer peer sits below the card"
        );
        assert!(find(&rows, "layer 2 · 1/3").0 > card_y);
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
        assert!(row_of(&rows, "5 Style").contains("┆ ○ 5 Style"));
        assert!(row_of(&rows, "6 Bind").contains("│ "));
    }

    #[test]
    fn vertical_puts_needs_above_and_dependents_below_the_card() {
        let rows = draw(&app_on(4), 60, 30, Orientation::Vertical);
        let (need_y, _) = find(&rows, "1 Scaffold");
        let (card_y, _) = find(&rows, "Render the rows");
        let (dep_y, _) = find(&rows, "7 Assemble");
        assert!(need_y < card_y && card_y < dep_y, "{}", rows.join("\n"));
        assert!(find(&rows, "needs").0 < need_y);
        let heading = find(&rows, "dependents").0;
        assert!(card_y < heading && heading < dep_y);
    }

    #[test]
    fn vertical_stacks_its_bands_from_the_top_at_compact_width() {
        let mut app = app_on(4);
        app.resolved_density = Density::Compact;
        let rows = draw(&app, 45, 53, Orientation::Vertical);
        let screen = rows.join("\n");
        let (dep_y, _) = find(&rows, "7 Assemble");
        assert!(dep_y < 20, "the traversal is compact:\n{screen}");
        assert!(
            rows[dep_y + 1..].iter().all(|row| row.trim().is_empty()),
            "the rest of the pane is free:\n{screen}"
        );
        assert!(
            screen.contains("Render one line per row."),
            "spare rows show the action:\n{screen}"
        );
        assert!(
            row_of(&rows, "5 Style").contains("▶"),
            "the next peer is on its own row, not dropped for want of a margin"
        );
    }

    #[test]
    fn density_bounds_the_summary_and_the_stack_width() {
        let mut snap = fixture();
        snap.tasks[3].action = "word ".repeat(200);
        let mut app = App::new(snap, &Config::default());
        app.selected = Some(4);
        let rows = |app: &App| {
            draw(app, 120, 50, Orientation::Vertical)
                .iter()
                .filter(|row| row.contains("word"))
                .count()
        };
        assert_eq!(rows(&app), usize::from(SUMMARY_COMFORTABLE) - 1);
        app.resolved_density = Density::Compact;
        assert_eq!(rows(&app), usize::from(SUMMARY_COMPACT) - 1);

        let rows = draw(&app, 120, 50, Orientation::Vertical);
        assert!(find(&rows, "╭").1 == 0, "compact spans the width");
        app.resolved_density = Density::Comfortable;
        let rows = draw(&app, 120, 50, Orientation::Vertical);
        assert_eq!(find(&rows, "╭").1, usize::from((120 - STACK_MAX) / 2));
    }

    #[test]
    fn spare_room_shows_the_second_hop() {
        let app = app_on(7);
        let rows = draw(&app, 60, 40, Orientation::Vertical);
        let hop = row_of(&rows, HOP_HEADING);
        assert!(hop.contains("✓ 1") && hop.contains("✓ 2"), "{hop}");
        assert!(find(&rows, HOP_HEADING).0 < find(&rows, "needs").0);

        let rows = draw(&app, 200, 20, Orientation::Horizontal);
        let (hop_y, hop_x) = find(&rows, HOP_HEADING);
        let (need_y, need_x) = find(&rows, "needs");
        assert_eq!(hop_y, need_y, "{}", rows.join("\n"));
        assert!(hop_x < need_x);
        assert!(find(&rows, "2 Define the schema").1 < find(&rows, "4 Render").1);

        let rows = draw(&app, 40, 9, Orientation::Vertical);
        assert!(
            !rows.join("\n").contains(HOP_HEADING),
            "the second hop gives way first"
        );
    }

    #[test]
    fn every_named_task_is_a_click_target() {
        let app = app_on(7);
        for orientation in [Orientation::Horizontal, Orientation::Vertical] {
            let (rows, targets) = draw_targets(&app, 200, 30, orientation);
            for id in [1, 2, 4, 5, 8] {
                let (rect, _) = targets
                    .iter()
                    .find(|(_, target)| *target == id)
                    .unwrap_or_else(|| panic!("{orientation:?}: no target for {id}"));
                let row: String = rows[usize::from(rect.y)]
                    .chars()
                    .skip(usize::from(rect.x))
                    .take(usize::from(rect.width))
                    .collect();
                assert!(
                    row.contains(&id.to_string()),
                    "{orientation:?} {id}: {row:?}"
                );
            }
            assert!(
                targets.iter().all(|(_, id)| *id != 7),
                "the card is not one"
            );
        }
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
    fn a_side_is_walked_along_and_left_the_way_it_was_entered() {
        let h = Orientation::Horizontal;
        let mut app = app_on(7);
        step(&mut app, h, Dir::Left);
        assert_eq!(app.selected, Some(4));
        step(&mut app, h, Dir::Down);
        assert_eq!(app.selected, Some(5), "the next need of 7, not 4's layer");
        let rows = draw(&app, 120, 20, h);
        assert!(rows.iter().any(|row| row.contains("needs of 7 · 2/2")));
        step(&mut app, h, Dir::Down);
        assert_eq!(app.selected, Some(5), "the side ends there");

        step(&mut app, h, Dir::Left);
        step(&mut app, h, Dir::Down);
        assert_eq!(app.selected, Some(2), "two crossings deep");
        assert_eq!(app.trail.len(), 2);
        step(&mut app, h, Dir::Right);
        assert_eq!(app.selected, Some(5), "back to where the crossing started");
        step(&mut app, h, Dir::Right);
        assert_eq!(app.selected, Some(7));
        assert!(app.trail.is_empty());
        step(&mut app, h, Dir::Down);
        assert_eq!(
            app.selected,
            Some(8),
            "with no trail, along walks the layer"
        );

        let v = Orientation::Vertical;
        let mut app = app_on(4);
        step(&mut app, v, Dir::Down);
        assert_eq!(app.selected, Some(7));
        step(&mut app, v, Dir::Up);
        assert_eq!((app.selected, app.trail.len()), (Some(4), 0));
    }

    #[test]
    fn a_trail_that_no_longer_leads_here_falls_back_to_the_layer() {
        let mut app = app_on(5);
        app.trail = vec![Crossing {
            origin: 4,
            side: Side::Needs,
        }];
        let nav = navigator(&app, Orientation::Horizontal);
        assert_eq!(nav.neighbor(5, Dir::Down), Some(6), "5 is not a need of 4");
        assert_eq!(
            nav.neighbor(5, Dir::Right),
            Some(7),
            "and it is not a way back either"
        );
    }

    #[test]
    fn a_move_across_recentres_the_next_frame() {
        let mut app = app_on(4);
        step(&mut app, Orientation::Horizontal, Dir::Right);
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
        let rows = draw(&app, 40, 12, Orientation::Vertical);
        assert!(
            rows.iter().any(|row| row.contains("more")),
            "{}",
            rows.join("\n")
        );
    }

    #[test]
    fn wrap_fills_rows_and_marks_what_it_drops() {
        assert_eq!(wrap("aa bb cc", 5, 3), ["aa bb", "cc"]);
        assert_eq!(wrap("aa bb cc dd", 5, 1), ["aa b…"]);
        assert_eq!(wrap("abcdefgh", 4, 2), ["abc…"]);
        assert!(wrap("aa", 0, 2).is_empty());
    }

    #[test]
    fn share_gives_each_list_a_row_before_the_longer_takes_the_rest() {
        assert_eq!(share(10, 2, 3), (2, 3));
        assert_eq!(share(4, 1, 7), (1, 3));
        assert_eq!(share(5, 6, 6), (3, 2));
        assert_eq!(share(1, 3, 3), (1, 0));
        assert_eq!(share(0, 3, 3), (0, 0));
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
        let mut app = app_on(7);
        for density in [Density::Compact, Density::Comfortable] {
            app.resolved_density = density;
            for (w, h) in [(1, 1), (5, 2), (20, 3), (3, 20), (0, 0), (45, 4)] {
                draw(&app, w.max(1), h.max(1), Orientation::Horizontal);
                draw(&app, w.max(1), h.max(1), Orientation::Vertical);
            }
        }
    }
}
