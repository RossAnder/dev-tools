//! Paints a routed diagram into a ratatui buffer.
//!
//! Only styles read the statuses and the selection, so a frame after a status-only
//! snapshot reuses the cached layout and repaints over it.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{DiagramCache, Layout, NodeBox, Stroke, char_cells};
use crate::app::App;
use crate::config::Orientation;
use crate::theme::Theme;

/// A drawn node's on-screen cells and its task id.
pub(crate) type Target = (Rect, u32);

/// Draws the diagram into `area` of `buf`, scrolled so the selection is on screen unless
/// the wheel has panned it; a `Frame` caller passes `frame.buffer_mut()`. Returns the
/// visible part of each drawn node, for clicks to select.
pub(crate) fn render(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    orientation: Orientation,
    cache: &mut DiagramCache,
) -> Vec<Target> {
    render_at(buf, area, app, orientation, cache, Instant::now())
}

/// `now` dates the flashes.
fn render_at(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    orientation: Orientation,
    cache: &mut DiagramCache,
    now: Instant,
) -> Vec<Target> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    cache.layout_for(
        &app.snapshot,
        &app.index,
        orientation,
        (area.width, area.height),
        app.selected,
        app.diagram_focus,
        app.diagram_fit,
        app.diagram_zoom,
    );
    let Some(layout) = cache.layout.as_ref() else {
        return Vec::new();
    };
    let focus = app.selected.and_then(|id| layout.node(id));
    let (x, y) = cache.scroll;
    let (dx, dy) = app.scroll_nudge;
    let scroll = match focus {
        // The wheel pans the canvas and leaves it there until the selection changes.
        _ if app.scroll_pinned => (
            pan(x, dx, area.width, layout.width),
            pan(y, dy, area.height, layout.height),
        ),
        Some(node) => (
            follow(x, node.x, node.width, area.width, layout.width),
            follow(y, node.y, 1, area.height, layout.height),
        ),
        None => (
            follow(x, 0, 0, area.width, layout.width),
            follow(y, 0, 0, area.height, layout.height),
        ),
    };
    cache.scroll = scroll;

    let theme = &app.theme;
    let selected = focus.map(|node| node.id);
    for stroke in &layout.strokes {
        let style = edge_style(layout, stroke, selected, theme);
        put(buf, area, scroll, (stroke.x, stroke.y), stroke.ch, style);
    }
    let overlaps = selected.map_or(&[][..], |id| app.index.overlaps(id));
    let mut targets = Vec::new();
    for node in &layout.nodes {
        let Some(task) = app.index.task(&app.snapshot, node.id) else {
            continue;
        };
        if let Some(rect) = on_screen(area, scroll, node) {
            targets.push((rect, node.id));
        }
        let mut style = if app.is_flashing(node.id, now) {
            theme.flash(task.status)
        } else {
            theme.status(task.status.as_str())
        };
        if selected == Some(node.id) {
            style = style.patch(theme.selection);
        } else if overlaps.contains(&node.id) {
            // Overlaps have no edge to draw, so a peer is underlined in the overlap colour.
            style = style.add_modifier(Modifier::UNDERLINED);
            if let Some(colour) = theme.overlap.fg {
                style = style.underline_color(colour);
            }
        }
        let label = app.diagram_zoom.label(task.id, &task.title, task.status);
        let mut x = node.x;
        for ch in label.chars() {
            let cells = char_cells(ch);
            if cells > 0 {
                put(buf, area, scroll, (x, node.y), ch, style);
                x = x.saturating_add(cells);
            }
        }
    }
    if layout.hidden > 0 {
        hidden_chip(buf, area, layout.hidden, &targets, selected, theme);
    }
    targets
}

/// The cells of `node` the viewport shows, in screen coordinates.
fn on_screen(area: Rect, scroll: (u16, u16), node: &NodeBox) -> Option<Rect> {
    let y = node.y.checked_sub(scroll.1)?;
    if y >= area.height {
        return None;
    }
    let start = node.x.max(scroll.0);
    let right = scroll.0.saturating_add(area.width);
    let end = node.x.saturating_add(node.width).min(right);
    let x = area.x + (start - scroll.0);
    (start < end).then(|| Rect::new(x, area.y + y, end - start, 1))
}

/// `+n hidden` at the top-right corner of `area`, for the tasks a focus or fit subset
/// left out. Where that covers a drawn node the chip drops to the bottom-right corner,
/// and where both would it takes the first that leaves the selection uncovered, or is
/// not drawn.
fn hidden_chip(
    buf: &mut Buffer,
    area: Rect,
    hidden: usize,
    targets: &[Target],
    selected: Option<u32>,
    theme: &Theme,
) {
    let text = format!("+{hidden} hidden");
    let width = u16::try_from(text.chars().count()).unwrap_or(u16::MAX);
    let x = area.right().saturating_sub(width).max(area.x);
    let chip = |y: u16| Rect::new(x, y, area.right() - x, 1);
    let covers = |rect: Rect, only: Option<u32>| {
        targets
            .iter()
            .any(|(at, id)| only.is_none_or(|only| only == *id) && at.intersects(rect))
    };
    let rows = [area.y, area.bottom() - 1];
    let Some(rect) = rows
        .iter()
        .map(|&y| chip(y))
        .find(|&rect| !covers(rect, None))
        .or_else(|| {
            let selection = selected?;
            let free = |rect: &Rect| !covers(*rect, Some(selection));
            rows.iter().map(|&y| chip(y)).find(free)
        })
    else {
        return;
    };
    buf.set_stringn(rect.x, rect.y, &text, usize::from(rect.width), theme.strong);
}

/// The selection's in-edges take the needs style and its out-edges the out-edge
/// style; with a selection every other edge is dimmed.
fn edge_style(layout: &Layout, stroke: &Stroke, selected: Option<u32>, theme: &Theme) -> Style {
    let Some(id) = selected else {
        return theme.edge;
    };
    let mut edges = stroke.edges.iter().filter_map(|&e| layout.edges.get(e));
    if edges.clone().any(|edge| edge.to == id) {
        theme.needs_edge
    } else if edges.any(|edge| edge.from == id) {
        theme.out_edge
    } else {
        theme.edge_faded
    }
}

/// Writes one canvas cell, skipping it when the viewport does not show it.
fn put(buf: &mut Buffer, area: Rect, scroll: (u16, u16), at: (u16, u16), ch: char, style: Style) {
    let (Some(dx), Some(dy)) = (at.0.checked_sub(scroll.0), at.1.checked_sub(scroll.1)) else {
        return;
    };
    if dx >= area.width || dy >= area.height {
        return;
    }
    if let Some(cell) = buf.cell_mut((area.x + dx, area.y + dy)) {
        cell.set_char(ch).set_style(style);
    }
}

fn pan(offset: u16, by: i32, view: u16, total: u16) -> u16 {
    let by = i16::try_from(by).unwrap_or(if by < 0 { i16::MIN } else { i16::MAX });
    offset
        .saturating_add_signed(by)
        .min(total.saturating_sub(view))
}

/// The scroll offset along one axis that keeps `start..start + len` on screen, moving
/// no further than it has to and a couple of cells clear of the edge when there is room.
fn follow(offset: u16, start: u16, len: u16, view: u16, total: u16) -> u16 {
    if total <= view {
        return 0;
    }
    let max = total - view;
    let margin = (view / 4).min(2);
    let mut offset = offset.min(max);
    let end = start.saturating_add(len).saturating_add(margin);
    if start < offset.saturating_add(margin) {
        offset = start.saturating_sub(margin);
    } else if end > offset.saturating_add(view) {
        offset = end - view;
    }
    offset.min(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::diagram::Zoom;
    use crate::diagram::order::EdgeKind;
    use crate::model::{TaskStatus, fixture};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn draw(
        app: &App,
        cache: &mut DiagramCache,
        orientation: Orientation,
        width: u16,
        height: u16,
    ) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_at(
                    frame.buffer_mut(),
                    area,
                    app,
                    orientation,
                    cache,
                    Instant::now(),
                );
            })
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn text(buf: &Buffer) -> String {
        let area = buf.area;
        (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_task_label_is_drawn_in_both_orientations() {
        let app = App::new(fixture(), &Config::default());
        for (orientation, width, height) in [
            (Orientation::Vertical, 60, 40),
            (Orientation::Horizontal, 100, 30),
        ] {
            let mut cache = DiagramCache::default();
            let screen = text(&draw(&app, &mut cache, orientation, width, height));
            for task in &app.snapshot.tasks {
                assert!(
                    screen.contains(&format!("[{}]", task.id)),
                    "{orientation:?} lacks [{}]:\n{screen}",
                    task.id
                );
            }
        }
    }

    /// The canvas position of a cell drawn only for edges matching `only`.
    fn sole_cell(cache: &DiagramCache, only: impl Fn(u32, u32) -> bool) -> (u16, u16) {
        let layout = cache.layout.as_ref().expect("drawn once");
        let stroke = layout
            .strokes
            .iter()
            .find(|s| {
                !s.edges.is_empty()
                    && s.edges.iter().all(|&e| {
                        let edge = layout.edges[e];
                        only(edge.from, edge.to)
                    })
            })
            .expect("a matching cell is drawn");
        (stroke.x, stroke.y)
    }

    #[test]
    fn the_selection_colours_its_edges_and_dims_the_rest() {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = Some(5);
        for orientation in [Orientation::Vertical, Orientation::Horizontal] {
            let mut cache = DiagramCache::default();
            let buf = draw(&app, &mut cache, orientation, 120, 60);
            assert_eq!(cache.scroll, (0, 0), "the canvas fits");
            let layout = cache.layout.as_ref().expect("drawn");
            assert!(
                layout
                    .edges
                    .iter()
                    .any(|e| e.from == 5 && e.to == 8 && e.kind == EdgeKind::Coupling)
            );

            let (x, y) = sole_cell(&cache, |from, to| (from, to) == (1, 5));
            assert_eq!(
                Some(buf[(x, y)].fg),
                app.theme.needs_edge.fg,
                "{orientation:?} in-edge"
            );
            let (x, y) = sole_cell(&cache, |from, _| from == 5);
            assert_eq!(
                Some(buf[(x, y)].fg),
                app.theme.out_edge.fg,
                "{orientation:?} out-edge"
            );
            let (x, y) = sole_cell(&cache, |from, to| (from, to) == (6, 8));
            assert_eq!(
                Some(buf[(x, y)].fg),
                app.theme.edge_faded.fg,
                "{orientation:?}"
            );

            let node = layout.node(5).expect("drawn");
            assert_eq!(Some(buf[(node.x, node.y)].bg), app.theme.selection.bg);
            let other = layout.node(6).expect("drawn");
            assert_ne!(Some(buf[(other.x, other.y)].bg), app.theme.selection.bg);
            let overlap = layout.node(4).expect("drawn");
            assert!(
                buf[(overlap.x, overlap.y)]
                    .modifier
                    .contains(Modifier::UNDERLINED),
                "4 shares a file with 5"
            );
        }
    }

    #[test]
    fn a_flashing_node_takes_the_flash_of_its_new_status() {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = None;
        app.flashes.insert(7, Instant::now());
        let mut cache = DiagramCache::default();
        let buf = draw(&app, &mut cache, Orientation::Vertical, 60, 40);
        let node = cache
            .layout
            .as_ref()
            .and_then(|l| l.node(7))
            .cloned()
            .unwrap();
        let status = app.snapshot.tasks[6].status;
        let flash = app.theme.flash(status);
        assert_eq!(Some(buf[(node.x, node.y)].bg), flash.bg);
        assert_eq!(Some(buf[(node.x, node.y)].fg), flash.fg);
    }

    #[test]
    fn the_viewport_scrolls_to_keep_the_selection_visible() {
        let mut app = App::new(fixture(), &Config::default());
        let mut cache = DiagramCache::default();
        for (orientation, width, height) in [
            (Orientation::Vertical, 30, 5),
            (Orientation::Horizontal, 12, 30),
        ] {
            for id in [8, 1, 7] {
                app.selected = Some(id);
                let screen = text(&draw(&app, &mut cache, orientation, width, height));
                assert!(
                    screen.contains(&format!("[{id}]")),
                    "{orientation:?} hides [{id}]:\n{screen}"
                );
            }
        }
    }

    #[test]
    fn a_tight_node_is_one_cell_showing_its_status_glyph() {
        let mut app = App::new(fixture(), &Config::default());
        app.diagram_zoom = Zoom::Tight;
        let mut cache = DiagramCache::default();
        let buf = draw(&app, &mut cache, Orientation::Vertical, 60, 40);
        let screen = text(&buf);
        assert!(!screen.contains('['), "no bracketed label:\n{screen}");
        let layout = cache.layout.as_ref().expect("drawn");
        for task in &app.snapshot.tasks {
            let node = layout.node(task.id).expect("every task is drawn");
            assert_eq!(node.width, 1, "[{}]", task.id);
            assert_eq!(buf[(node.x, node.y)].symbol(), task.status.glyph());
        }
    }

    #[test]
    fn a_titled_label_cuts_its_title_by_chars_to_fill_its_box() {
        let mut snap = fixture();
        snap.tasks[3].title = "é".repeat(30);
        let mut app = App::new(snap, &Config::default());
        app.diagram_zoom = Zoom::Titles;
        let mut cache = DiagramCache::default();
        let screen = text(&draw(&app, &mut cache, Orientation::Vertical, 200, 40));
        let cut = format!("[4 {}…]", "é".repeat(23));
        assert!(screen.contains(&cut), "{screen}");
        let layout = cache.layout.as_ref().expect("drawn");
        let node = layout.node(4).expect("drawn");
        assert_eq!(usize::from(node.width), cut.chars().count());
    }

    #[test]
    fn a_titled_label_draws_control_chars_as_spaces() {
        let title = "a\u{1b}]52;c;x\u{7}b\tc\u{1b}[2J";
        let label = Zoom::Titles.label(4, title, TaskStatus::Pending);
        assert!(!label.chars().any(char::is_control), "{label:?}");
        let raw = format!("[4 {title}]");
        assert_eq!(label.chars().count(), raw.chars().count());

        let mut snap = fixture();
        snap.tasks[3].title = title.into();
        let mut app = App::new(snap, &Config::default());
        app.diagram_zoom = Zoom::Titles;
        let mut cache = DiagramCache::default();
        let buf = draw(&app, &mut cache, Orientation::Vertical, 200, 40);
        let screen = text(&buf);
        assert!(screen.contains(&label), "{screen}");
        assert!(screen.lines().all(|l| !l.chars().any(char::is_control)));
        let node = cache.layout.as_ref().and_then(|l| l.node(4)).unwrap();
        assert_eq!(usize::from(node.width), label.chars().count());
    }

    #[test]
    fn a_fit_limited_frame_marks_the_hidden_tasks() {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = Some(7);
        let full = DiagramCache::default()
            .layout(&app.snapshot, &app.index, Orientation::Horizontal)
            .clone();
        let (width, height) = (full.width - 1, full.height);
        let mut cache = DiagramCache::default();
        let buf = draw(&app, &mut cache, Orientation::Horizontal, width, height);
        let hidden = cache.layout.as_ref().expect("drawn").hidden;
        assert!(hidden > 0, "the whole graph overflows by a column");
        let chip = format!("+{hidden} hidden");
        let screen = text(&buf);
        let lines: Vec<&str> = screen.lines().collect();
        let y = [0, lines.len() - 1]
            .into_iter()
            .find(|&y| lines[y].ends_with(&chip))
            .unwrap_or_else(|| panic!("no {chip:?} in a right-hand corner:\n{screen}"));
        let corner = (width - 1, u16::try_from(y).expect("a small view"));
        assert_eq!(buf[corner].symbol(), "n");
        assert_eq!(Some(buf[corner].fg), app.theme.strong.fg);
        let area = Rect::new(0, 0, width, height);
        let targets = render_at(
            &mut Buffer::empty(area),
            area,
            &app,
            Orientation::Horizontal,
            &mut DiagramCache::default(),
            Instant::now(),
        );
        let len = u16::try_from(chip.len()).expect("a short chip");
        let cells = Rect::new(width - len, corner.1, len, 1);
        assert!(
            targets.iter().all(|(rect, _)| !rect.intersects(cells)),
            "the chip covers no node:\n{screen}"
        );

        app.diagram_fit = false;
        let mut cache = DiagramCache::default();
        let top = text(&draw(
            &app,
            &mut cache,
            Orientation::Horizontal,
            width,
            height,
        ));
        assert!(!top.contains("hidden"), "fit off draws everything:\n{top}");
    }

    #[test]
    fn every_drawn_node_is_a_click_target_at_its_screen_cells() {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = Some(8);
        let mut cache = DiagramCache::default();
        let area = Rect::new(3, 2, 14, 4);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        let targets = render_at(
            &mut buf,
            area,
            &app,
            Orientation::Vertical,
            &mut cache,
            Instant::now(),
        );
        let layout = cache.layout.as_ref().expect("drawn");
        let node = layout.node(8).expect("the selection is drawn");
        let (sx, sy) = cache.scroll;
        let (rect, _) = targets
            .iter()
            .find(|(_, id)| *id == 8)
            .expect("the selection is a target");
        assert_eq!(
            (rect.x, rect.y),
            (area.x + node.x - sx, area.y + node.y - sy)
        );
        assert_eq!(buf[(rect.x, rect.y)].symbol(), "[");
        for (rect, _) in &targets {
            assert!(area.contains(rect.as_position()), "{rect:?} in {area:?}");
            assert!(rect.right() <= area.right(), "{rect:?} is clipped");
        }
    }

    #[test]
    fn follow_moves_only_as_far_as_it_must() {
        assert_eq!(follow(0, 50, 3, 20, 100), 35);
        assert_eq!(follow(35, 40, 3, 20, 100), 35, "already visible");
        assert_eq!(follow(35, 10, 3, 20, 100), 8);
        assert_eq!(follow(35, 99, 1, 20, 100), 80, "clamped at the far end");
        assert_eq!(
            follow(35, 5, 3, 200, 100),
            0,
            "a canvas that fits never scrolls"
        );
    }
}
