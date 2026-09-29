//! The layer-list view and its navigator.
//!
//! Tasks are grouped by the snapshot's Kahn layers, taken verbatim. Vertically
//! each layer is a header row followed by one row per task; horizontally each
//! layer is a column of compact cells. A checkpoint row follows the layer that
//! completes the checkpoint's group. With a selection, every row is marked by
//! its relation to it and unrelated rows are dimmed.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{App, Dir, Navigator};
use crate::config::Orientation;
use crate::model::{AgentStatus, Checkpoint, Index, Snapshot, Task, TaskStatus};

/// Narrowest horizontal column; past it the columns scroll instead of shrinking.
const MIN_COLUMN: u16 = 18;

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
pub(crate) fn render(buf: &mut Buffer, area: Rect, app: &App, orientation: Orientation) {
    render_at(
        buf,
        area,
        app,
        orientation,
        SystemTime::now(),
        Instant::now(),
    );
}

/// `wall` dates the agents' elapsed counters and `now` the flashes.
fn render_at(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    orientation: Orientation,
    wall: SystemTime,
    now: Instant,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let ctx = Ctx::new(app, wall, now);
    let groups = groups(&app.snapshot, &app.index);
    match orientation {
        Orientation::Vertical => render_vertical(buf, area, &ctx, &groups),
        Orientation::Horizontal => render_horizontal(buf, area, &ctx, &groups),
    }
}

/// A drawn row and the style laid over its full width.
struct Row {
    line: Line<'static>,
    overlay: Option<Style>,
}

fn render_vertical(buf: &mut Buffer, area: Rect, ctx: &Ctx, groups: &[Group]) {
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
        rows.push(Row {
            line: rule(&format!("── {} ", group.label), '─', width, dim()),
            overlay: None,
        });
        for id in &group.ids {
            let Some(task) = ctx.app.index.task(&ctx.app.snapshot, *id) else {
                continue;
            };
            if ctx.is_selected(*id) {
                selected_row = Some(rows.len());
            }
            rows.push(Row {
                line: task_line(ctx, task, width, Some(id_width)),
                overlay: ctx.overlay(*id),
            });
        }
        for pos in &group.closes {
            rows.push(Row {
                line: checkpoint_line(ctx, &ctx.app.snapshot.checkpoints[*pos], width, true),
                overlay: None,
            });
        }
    }
    let height = usize::from(area.height);
    let offset = scroll(selected_row, rows.len(), height);
    draw_rows(buf, area, &rows[offset..]);
}

fn render_horizontal(buf: &mut Buffer, area: Rect, ctx: &Ctx, groups: &[Group]) {
    if groups.is_empty() {
        return;
    }
    let count = u16::try_from(groups.len()).unwrap_or(u16::MAX);
    let column = (area.width / count).max(MIN_COLUMN.min(area.width));
    let visible = usize::from(area.width / column).max(1);
    let selected_column = groups
        .iter()
        .position(|group| group.ids.iter().any(|id| ctx.is_selected(*id)));
    let first = selected_column.map_or(0, |at| at.saturating_sub(visible - 1));
    let height = usize::from(area.height);
    // The last cell of a column is left blank as the gutter to the next one.
    let cell = usize::from(column).saturating_sub(1).max(1);
    for (slot, group) in groups.iter().skip(first).take(visible).enumerate() {
        let x = area.x + column * u16::try_from(slot).unwrap_or(0);
        let mut rows = vec![Row {
            line: rule(&format!("{} ", group.label), '─', cell, dim()),
            overlay: None,
        }];
        let mut selected_row = None;
        for id in &group.ids {
            let Some(task) = ctx.app.index.task(&ctx.app.snapshot, *id) else {
                continue;
            };
            if ctx.is_selected(*id) {
                selected_row = Some(rows.len());
            }
            rows.push(Row {
                line: task_line(ctx, task, cell, None),
                overlay: ctx.overlay(*id),
            });
        }
        for pos in &group.closes {
            rows.push(Row {
                line: checkpoint_line(ctx, &ctx.app.snapshot.checkpoints[*pos], cell, false),
                overlay: None,
            });
        }
        let offset = scroll(selected_row, rows.len(), height);
        let width = u16::try_from(cell)
            .unwrap_or(u16::MAX)
            .min(area.right().saturating_sub(x));
        draw_rows(
            buf,
            Rect::new(x, area.y, width, area.height),
            &rows[offset..],
        );
    }
}

fn draw_rows(buf: &mut Buffer, area: Rect, rows: &[Row]) {
    for (row, y) in rows.iter().zip(area.y..area.bottom()) {
        buf.set_line(area.x, y, &row.line, area.width);
        if let Some(style) = row.overlay {
            buf.set_style(Rect::new(area.x, y, area.width, 1), style);
        }
    }
}

/// First row to draw so that the selected one sits on screen, kept a couple
/// of rows clear of the bottom edge when the pane allows.
fn scroll(selected: Option<usize>, len: usize, height: usize) -> usize {
    let Some(selected) = selected else {
        return 0;
    };
    if len <= height || height == 0 {
        return 0;
    }
    let margin = (height / 4).min(2);
    selected
        .saturating_sub(height - 1 - margin)
        .min(len - height)
}

/// The selection and the rows related to it.
struct Focus {
    id: u32,
    needs: Vec<u32>,
    dependents: Vec<u32>,
    coupling: Vec<u32>,
    coupled: Vec<u32>,
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
    /// coupling peers before and after it.
    fn mark(&self, id: u32) -> Option<(&'static str, Style)> {
        let focus = self.focus.as_ref()?;
        let theme = &self.app.theme;
        if focus.id == id {
            Some(("▸", theme.badge))
        } else if focus.needs.contains(&id) {
            Some(("↑", theme.needs_edge))
        } else if focus.dependents.contains(&id) {
            Some(("↓", theme.needs_edge))
        } else if focus.coupling.contains(&id) {
            Some(("⇡", theme.coupling_edge))
        } else if focus.coupled.contains(&id) {
            Some(("⇣", theme.coupling_edge))
        } else {
            None
        }
    }

    fn overlay(&self, id: u32) -> Option<Style> {
        let theme = &self.app.theme;
        if self.is_selected(id) {
            Some(theme.selection)
        } else if self.app.is_flashing(id, self.now) {
            Some(theme.flash)
        } else if self.focus.is_some() && self.mark(id).is_none() {
            Some(dim())
        } else {
            None
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
            .and_then(|at| self.wall.duration_since(at).ok())
            .map(|age| format!(" {}", format_elapsed(age)))
            .unwrap_or_default();
        let style = if self.app.stale_agents.contains(&agent.id) {
            self.app.theme.agent_chip.add_modifier(Modifier::DIM)
        } else {
            self.app.theme.agent_chip
        };
        Some(Span::styled(
            format!(" {}{elapsed} ", initials(&agent.agent_type)),
            style,
        ))
    }

    fn badges(&self, id: u32) -> Vec<Span<'static>> {
        let record = self.app.index.record_for(&self.app.snapshot, id);
        let has = |kind: &str| record.iter().any(|entry| entry.entry_type == kind);
        let mut badges = Vec::new();
        if has("deviation") {
            badges.push(Span::styled("⚠", self.app.theme.warning));
        }
        if has("deferral") {
            badges.push(Span::styled("⏸", self.app.theme.badge));
        }
        badges
    }
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
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

/// One task row. `id_width` right-aligns the id in a full row; `None` gives
/// the compact `◐14 title…` cell with no effort column.
fn task_line(ctx: &Ctx, task: &Task, width: usize, id_width: Option<usize>) -> Line<'static> {
    let status = ctx.app.theme.status(task.status.as_str());
    let (mark, mark_style) = ctx.mark(task.id).unwrap_or((" ", Style::new()));
    let mut left = vec![Span::styled(mark, mark_style)];
    match id_width {
        Some(w) => {
            left.push(Span::raw(" "));
            left.push(Span::styled(glyph(task.status), status));
            left.push(Span::raw(format!(" {:>w$} ", task.id)));
        }
        None => {
            left.push(Span::styled(glyph(task.status), status));
            left.push(Span::raw(format!("{} ", task.id)));
        }
    }

    let mut right = ctx.badges(task.id);
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
        right.push(Span::styled(task.effort.clone(), dim()));
    }

    let used: usize =
        left.iter().chain(&right).map(Span::width).sum::<usize>() + usize::from(!right.is_empty());
    let room = width.saturating_sub(used);
    let title = truncate(&task.title, room);
    let pad = room.saturating_sub(text_width(&title));
    left.push(Span::raw(format!("{title}{}", " ".repeat(pad))));
    if !right.is_empty() {
        left.push(Span::raw(" "));
        left.extend(right);
    }
    Line::from(left)
}

/// `◆ A ✓ a1b2c3d …`: the checkpoint id, its verification verdict and its
/// short commits. A full row is closed with a rule out to `width`.
fn checkpoint_line(ctx: &Ctx, checkpoint: &Checkpoint, width: usize, ruled: bool) -> Line<'static> {
    let theme = &ctx.app.theme;
    let mut spans = vec![Span::styled(
        format!("{}◆ {}", if ruled { "╌╌ " } else { "" }, checkpoint.id),
        theme.badge,
    )];
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
        spans.push(Span::styled(format!(" {}", commits.join(" ")), dim()));
    }
    let used: usize = spans.iter().map(Span::width).sum();
    if ruled && used + 1 < width {
        spans.push(Span::styled(
            format!(" {}", "╌".repeat(width - used - 1)),
            dim(),
        ));
    }
    Line::from(spans)
}

/// `prefix` followed by `fill` out to `width` cells.
fn rule(prefix: &str, fill: char, width: usize, style: Style) -> Line<'static> {
    let rest = width.saturating_sub(text_width(prefix));
    Line::from(Span::styled(
        format!("{prefix}{}", fill.to_string().repeat(rest)),
        style,
    ))
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

fn format_elapsed(age: Duration) -> String {
    let secs = age.as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3_600 => format!("{}m", secs / 60),
        _ => format!("{}h{:02}", secs / 3_600, secs % 3_600 / 60),
    }
}

/// An RFC 3339 timestamp, with optional fractional seconds and a `Z` or
/// `±HH:MM` offset; `None` for anything else.
fn parse_utc(s: &str) -> Option<SystemTime> {
    let bytes = s.as_bytes();
    let digits = |from: usize, to: usize| -> Option<i64> {
        let part = bytes.get(from..to)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    let separators = [(4, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    if separators
        .iter()
        .any(|(at, sep)| bytes.get(*at) != Some(sep))
        || !matches!(bytes.get(10), Some(b'T' | b't' | b' '))
    {
        return None;
    }
    let (year, month, day) = (digits(0, 4)?, digits(5, 7)?, digits(8, 10)?);
    let (hour, minute, second) = (digits(11, 13)?, digits(14, 16)?, digits(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }

    let mut rest = &s[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        rest = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first() {
                Some(b'+') => 1,
                Some(b'-') => -1,
                _ => return None,
            };
            let tail = rest.as_bytes();
            if tail.len() != 6 || tail[3] != b':' {
                return None;
            }
            let at = s.len() - 6;
            sign * (digits(at + 1, at + 3)? * 3_600 + digits(at + 4, at + 6)? * 60)
        }
    };

    let secs =
        days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second - offset;
    u64::try_from(secs)
        .ok()
        .map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
}

/// Days since 1970-01-01 of a proleptic Gregorian date, over 400-year eras
/// with March as month 0.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::hook::format_utc;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app() -> App {
        App::new(fixture(), &Config::default())
    }

    /// Twelve minutes after the running agent's segment on task 4 opened.
    fn wall() -> SystemTime {
        parse_utc("2026-09-28T11:16:22Z").expect("valid timestamp")
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
                &format!("{} {} {}", glyph(task.status), task.id, task.title),
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
                &format!("{}{} {}", glyph(task.status), task.id, task.title),
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
        assert_eq!(mark("Bind the keys"), Some(' '));

        app.selected = Some(6);
        let lines = self::lines(&draw(&app, Orientation::Vertical, 100, 30));
        let mark = |title: &str| row_of(&lines, title).1.chars().next();
        assert_eq!(mark("Load the config"), Some('⇡'));
        assert_eq!(mark("Wire the entry point"), Some('↓'));
    }

    #[test]
    fn unrelated_rows_are_dimmed() {
        let app = app();
        assert_eq!(app.selected, Some(4));
        let buf = draw(&app, Orientation::Vertical, 100, 30);
        let lines = lines(&buf);
        let modifier = |title: &str| {
            let y = u16::try_from(row_of(&lines, title).0).expect("fits");
            buf[(3, y)].modifier
        };
        assert!(modifier("Define the schema").contains(Modifier::DIM));
        assert!(!modifier("Scaffold the store").contains(Modifier::DIM));
        assert!(modifier("Render the rows").contains(Modifier::REVERSED));
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
    fn the_deviation_badge_marks_its_task() {
        let mut app = app();
        app.selected = None;
        let lines = lines(&draw(&app, Orientation::Vertical, 100, 30));
        assert!(row_of(&lines, "Load the config").1.contains('⚠'));
        assert!(!row_of(&lines, "Define the schema").1.contains('⚠'));
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
    fn narrow_columns_scroll_to_the_selected_layer() {
        let mut app = app();
        app.selected = Some(8);
        let lines = lines(&draw(&app, Orientation::Horizontal, 40, 10));
        assert!(lines[0].contains("L3 "), "{lines:#?}");
        assert!(!lines[0].contains("L1 "));
    }

    #[test]
    fn timestamps_round_trip_through_format_utc() {
        for secs in [0, 951_782_400, 1_790_000_000, 1_790_000_059] {
            assert_eq!(
                parse_utc(&format_utc(secs)),
                Some(UNIX_EPOCH + Duration::from_secs(secs))
            );
        }
        assert_eq!(
            parse_utc("2026-09-28T13:04:22.5+02:00"),
            parse_utc("2026-09-28T11:04:22Z")
        );
        assert_eq!(parse_utc("2026-09-28"), None);
        assert_eq!(parse_utc("2026-13-28T11:04:22Z"), None);
        assert_eq!(format_elapsed(Duration::from_secs(3_725)), "1h02");
        assert_eq!(initials("implement-deep"), "ID");
        assert_eq!(truncate("Render the rows", 8), "Render …");
    }
}
