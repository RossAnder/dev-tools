//! Paints a routed diagram into a ratatui buffer.
//!
//! Only styles read the statuses and the selection, so a frame after a status-only
//! snapshot reuses the cached layout and repaints over it.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{DiagramCache, Layout, Stroke};
use crate::app::App;
use crate::config::Orientation;
use crate::theme::Theme;

/// Draws the diagram into `area` of `buf`, scrolled so the selection is on screen; a
/// `Frame` caller passes `frame.buffer_mut()`.
pub(crate) fn render(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    orientation: Orientation,
    cache: &mut DiagramCache,
) {
    render_at(buf, area, app, orientation, cache, Instant::now());
}

/// `now` dates the flashes.
fn render_at(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    orientation: Orientation,
    cache: &mut DiagramCache,
    now: Instant,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    cache.refresh(&app.snapshot, &app.index, orientation);
    let Some(layout) = cache.layout.as_ref() else {
        return;
    };
    let focus = app.selected.and_then(|id| layout.node(id));
    let (x, y) = cache.scroll;
    let scroll = match focus {
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
    for node in &layout.nodes {
        let Some(task) = app.index.task(&app.snapshot, node.id) else {
            continue;
        };
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
        for (x, ch) in (node.x..).zip(format!("[{}]", node.id).chars()) {
            put(buf, area, scroll, (x, node.y), ch, style);
        }
    }
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
    use crate::diagram::order::EdgeKind;
    use crate::model::fixture;
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
