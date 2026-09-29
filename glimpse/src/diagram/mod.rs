//! The layered DAG diagram view: layout cache, orientation mapping and navigation.
//!
//! The layout runs [`order`](order::order), [`position`](position::position) and
//! [`route`](route::route) in abstract `(layer axis, cross axis)` cells, then maps them
//! onto the screen. Vertically the layer axis runs down the rows; horizontally it runs
//! across the columns, each layer's column widening to its longest `[id]` label. The
//! layout depends only on task ids and edges, so [`DiagramCache`] keeps it until the
//! topology or the orientation changes, and a status-only snapshot just repaints.

pub(crate) mod glyph;
pub(crate) mod order;
pub(crate) mod paint;
pub(crate) mod position;
pub(crate) mod route;

use std::collections::{HashMap, HashSet};

use crate::app::{Dir, Navigator};
use crate::config::Orientation;
use crate::model::{Index, Snapshot};

use glyph::ArmMask;
use order::{Edge, EdgeId, EdgeKind, Slot};
use route::{Arrow, Cell};

/// Corners are drawn `╭╮╰╯` rather than `┌┐└┘`.
const ROUNDED: bool = true;

/// A task's `[id]` label, in canvas cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NodeBox {
    pub(crate) id: u32,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) width: u16,
}

/// One edge cell on the canvas and the edges drawn through it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stroke {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) ch: char,
    pub(crate) edges: Vec<EdgeId>,
}

/// A laid-out diagram in canvas cells, origin top-left, before any scrolling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) orientation: Orientation,
    /// Indexed by [`EdgeId`].
    pub(crate) edges: Vec<Edge>,
    pub(crate) nodes: Vec<NodeBox>,
    node_index: HashMap<u32, usize>,
    pub(crate) strokes: Vec<Stroke>,
    pub(crate) width: u16,
    pub(crate) height: u16,
    /// Tasks of each non-empty layer in drawn order, dummies skipped.
    rows: Vec<Vec<u32>>,
    /// Cross-axis centre of each task.
    centre: HashMap<u32, u16>,
}

impl Layout {
    pub(crate) fn node(&self, id: u32) -> Option<&NodeBox> {
        self.node_index.get(&id).map(|&at| &self.nodes[at])
    }
}

/// Holds the last layout and recomputes it only when `(topology_hash, orientation)`
/// changes. The viewport scroll lives here too, since it must survive between frames.
#[derive(Debug, Default)]
pub(crate) struct DiagramCache {
    key: Option<(u64, Orientation)>,
    layout: Option<Layout>,
    computations: usize,
    /// Canvas cell at the viewport's top-left corner.
    scroll: (u16, u16),
}

impl DiagramCache {
    /// Layouts computed so far; unchanged by a snapshot that only moves statuses.
    pub(crate) fn computations(&self) -> usize {
        self.computations
    }

    pub(crate) fn layout(
        &mut self,
        snapshot: &Snapshot,
        index: &Index,
        orientation: Orientation,
    ) -> &Layout {
        self.refresh(snapshot, index, orientation);
        self.layout
            .get_or_insert_with(|| layout(snapshot, orientation))
    }

    fn refresh(&mut self, snapshot: &Snapshot, index: &Index, orientation: Orientation) {
        let key = (index.topology_hash(), orientation);
        if self.key == Some(key) && self.layout.is_some() {
            return;
        }
        self.layout = Some(layout(snapshot, orientation));
        self.key = Some(key);
        self.computations += 1;
        self.scroll = (0, 0);
    }
}

/// The navigator for what [`paint::render`] draws in `orientation`, built from the cached
/// layout so a move lands on a drawn node.
pub(crate) fn navigator(
    cache: &mut DiagramCache,
    snapshot: &Snapshot,
    index: &Index,
    orientation: Orientation,
) -> Box<dyn Navigator> {
    let layout = cache.layout(snapshot, index, orientation);
    Box::new(DiagramNav {
        rows: layout.rows.clone(),
        centre: layout.centre.clone(),
        orientation,
    })
}

/// Moves over `(layer, slot)`. Along the layer axis a move jumps to the task in the
/// adjacent layer nearest on the cross axis; across it, a move steps through the tasks
/// in reading order, continuing into the next layer at a layer's end.
struct DiagramNav {
    rows: Vec<Vec<u32>>,
    centre: HashMap<u32, u16>,
    orientation: Orientation,
}

impl DiagramNav {
    fn layer_of(&self, id: u32) -> Option<usize> {
        self.rows.iter().position(|row| row.contains(&id))
    }

    fn across(&self, from: u32, forward: bool) -> Option<u32> {
        let layer = self.layer_of(from)?;
        let target = if forward {
            layer + 1
        } else {
            layer.checked_sub(1)?
        };
        let at = self.centre.get(&from).copied().unwrap_or(0);
        self.rows
            .get(target)?
            .iter()
            .copied()
            .min_by_key(|id| self.centre.get(id).map_or(u16::MAX, |c| c.abs_diff(at)))
    }

    fn reading(&self, from: u32, forward: bool) -> Option<u32> {
        let order: Vec<u32> = self.rows.iter().flatten().copied().collect();
        let at = order.iter().position(|&id| id == from)?;
        let to = if forward { at + 1 } else { at.checked_sub(1)? };
        order.get(to).copied()
    }
}

impl Navigator for DiagramNav {
    fn neighbor(&self, from: u32, dir: Dir) -> Option<u32> {
        let (layer_axis, forward) = match (self.orientation, dir) {
            (Orientation::Vertical, Dir::Up) | (Orientation::Horizontal, Dir::Left) => {
                (true, false)
            }
            (Orientation::Vertical, Dir::Down) | (Orientation::Horizontal, Dir::Right) => {
                (true, true)
            }
            (Orientation::Vertical, Dir::Left) | (Orientation::Horizontal, Dir::Up) => {
                (false, false)
            }
            (Orientation::Vertical, Dir::Right) | (Orientation::Horizontal, Dir::Down) => {
                (false, true)
            }
        };
        if layer_axis {
            self.across(from, forward)
        } else {
            self.reading(from, forward)
        }
    }
}

fn label_width(id: u32) -> u16 {
    u16::try_from(id.to_string().len() + 2).unwrap_or(u16::MAX)
}

/// The snapshot's layers verbatim, keeping only ids that name a task, plus a trailing
/// layer for tasks no layer lists so that every task is drawn.
fn layers(snapshot: &Snapshot) -> Vec<Vec<u32>> {
    let known: HashSet<u32> = snapshot.tasks.iter().map(|task| task.id).collect();
    let mut layers: Vec<Vec<u32>> = snapshot
        .layers
        .iter()
        .map(|ids| {
            ids.iter()
                .copied()
                .filter(|id| known.contains(id))
                .collect()
        })
        .collect();
    let layered: HashSet<u32> = layers.iter().flatten().copied().collect();
    let mut rest: Vec<u32> = known.difference(&layered).copied().collect();
    if !rest.is_empty() {
        rest.sort_unstable();
        layers.push(rest);
    }
    layers
}

/// Edges read from the same `needs` and `coupling` lists the topology hash covers,
/// so a cached layout can never disagree with its key.
fn edges(snapshot: &Snapshot) -> Vec<Edge> {
    let mut tasks: Vec<_> = snapshot.tasks.iter().collect();
    tasks.sort_by_key(|task| task.id);
    let mut edges = Vec::new();
    for task in tasks {
        for (list, kind) in [
            (&task.needs, EdgeKind::Needs),
            (&task.coupling, EdgeKind::Coupling),
        ] {
            let mut from = list.clone();
            from.sort_unstable();
            from.dedup();
            edges.extend(from.into_iter().map(|from| Edge {
                from,
                to: task.id,
                kind,
            }));
        }
    }
    edges
}

fn layout(snapshot: &Snapshot, orientation: Orientation) -> Layout {
    let horizontal = orientation == Orientation::Horizontal;
    let edges = edges(snapshot);
    let ordered = order::order(&layers(snapshot), &edges);
    // Horizontally a label runs along the layer axis, so it takes one cross-axis cell.
    let positions = position::position(&ordered, |id| if horizontal { 1 } else { label_width(id) });
    let routed = route::route(&ordered, &positions);

    let task_ids = |row: &[Slot]| -> Vec<u32> {
        row.iter()
            .filter_map(|slot| match slot {
                Slot::Task(id) => Some(*id),
                Slot::Dummy { .. } => None,
            })
            .collect()
    };
    let layer_rows: Vec<u16> = (0..ordered.rows.len())
        .map(|r| routed.layer_row(r))
        .collect();
    let band: Vec<u16> = ordered
        .rows
        .iter()
        .map(|row| {
            let widest = task_ids(row).into_iter().map(label_width).max();
            if horizontal { widest.unwrap_or(1) } else { 1 }
        })
        .collect();

    // Each abstract layer-axis cell maps to a run of screen cells: a layer's band, or
    // one cell for a channel row.
    let main_len = layer_rows.last().map_or(0, |&row| usize::from(row) + 1);
    let mut main_start = Vec::with_capacity(main_len);
    let mut main_span = Vec::with_capacity(main_len);
    let mut next = 0u16;
    for a in 0..main_len {
        let span = u16::try_from(a)
            .ok()
            .and_then(|a| layer_rows.binary_search(&a).ok())
            .map_or(1, |r| band[r]);
        main_start.push(next);
        main_span.push(span);
        next = next.saturating_add(span);
    }
    let screen = |main: u16, cross: u16| {
        if horizontal {
            (main, cross)
        } else {
            (cross, main)
        }
    };

    let mut cell_edges: HashMap<Cell, Vec<EdgeId>> = HashMap::new();
    let mut routed_edges: Vec<(&EdgeId, &Vec<Cell>)> = routed.edge_cells.iter().collect();
    routed_edges.sort_unstable_by_key(|(id, _)| **id);
    for (&id, cells) in routed_edges {
        for &cell in cells {
            let list = cell_edges.entry(cell).or_default();
            if !list.contains(&id) {
                list.push(id);
            }
        }
    }

    let mut strokes = Vec::new();
    let mut cells: Vec<&Cell> = routed.cells.keys().collect();
    cells.sort_unstable();
    for &cell in cells {
        let (a, cross) = (usize::from(cell.0), cell.1);
        let (Some(&start), Some(&span)) = (main_start.get(a), main_span.get(a)) else {
            continue;
        };
        let ch = match routed.arrows.get(&cell) {
            Some(&arrow) => arrow_char(arrow, orientation),
            None => {
                let ch = glyph::glyph(&routed.cells[&cell], ROUNDED);
                if horizontal { transpose(ch) } else { ch }
            }
        };
        let through = cell_edges.get(&cell).cloned().unwrap_or_default();
        for main in start..start.saturating_add(span) {
            let (x, y) = screen(main, cross);
            strokes.push(Stroke {
                x,
                y,
                ch,
                edges: through.clone(),
            });
        }
    }

    let mut nodes = Vec::new();
    let mut centre = HashMap::new();
    for (r, row) in ordered.rows.iter().enumerate() {
        let layer_row = layer_rows[r];
        let main = main_start[usize::from(layer_row)];
        for id in task_ids(row) {
            let slot = Slot::Task(id);
            let (Some(&x), Some(mid)) = (positions.x.get(&slot), positions.centre(slot)) else {
                continue;
            };
            let width = label_width(id);
            centre.insert(id, mid);
            let (nx, ny) = screen(main, x);
            nodes.push(NodeBox {
                id,
                x: nx,
                y: ny,
                width,
            });
            // A label shorter than its column is joined to the edges leaving the
            // column's far side by a run of line.
            let below = (layer_row + 1, mid);
            let attached = routed
                .cells
                .get(&below)
                .is_some_and(|owners| owners.iter().any(|o| o.arms.has(ArmMask::N)));
            if horizontal && attached && band[r] > width {
                let touching: Vec<EdgeId> = cell_edges
                    .get(&below)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|&e| edges[e].from == id || edges[e].to == id)
                    .collect();
                for m in main + width..main + band[r] {
                    strokes.push(Stroke {
                        x: m,
                        y: mid,
                        ch: '─',
                        edges: touching.clone(),
                    });
                }
            }
        }
    }

    let node_index = nodes.iter().enumerate().map(|(at, n)| (n.id, at)).collect();
    let (width, height) = screen(next, positions.layer_extent);
    Layout {
        orientation,
        edges,
        nodes,
        node_index,
        strokes,
        width,
        height,
        rows: ordered
            .rows
            .iter()
            .map(|row| task_ids(row))
            .filter(|ids| !ids.is_empty())
            .collect(),
        centre,
    }
}

/// `Down` points towards higher layers: `▼` when they run down the page, `▶` across.
fn arrow_char(arrow: Arrow, orientation: Orientation) -> char {
    match (arrow, orientation) {
        (Arrow::Down, Orientation::Vertical) => '▼',
        (Arrow::Up, Orientation::Vertical) => '▲',
        (Arrow::Down, Orientation::Horizontal) => '▶',
        (Arrow::Up, Orientation::Horizontal) => '◀',
    }
}

/// Reflects a box-drawing glyph across the main diagonal. Transposing the resolved
/// glyph, rather than the arms before resolution, keeps the layer-axis line unbroken
/// where two unrelated edges cross, in either orientation.
fn transpose(ch: char) -> char {
    match ch {
        '│' => '─',
        '─' => '│',
        '┆' => '┄',
        '┄' => '┆',
        '└' => '┐',
        '┐' => '└',
        '╰' => '╮',
        '╮' => '╰',
        '├' => '┬',
        '┬' => '├',
        '┤' => '┴',
        '┴' => '┤',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::model::{Task, TaskStatus, fixture};

    const BOTH: [Orientation; 2] = [Orientation::Vertical, Orientation::Horizontal];

    /// The fixture plus a two-digit task in the middle layer, so labels differ in
    /// width, and a `needs` edge from layer 0 to layer 2 that passes through it.
    fn wide() -> Snapshot {
        let mut snap = fixture();
        snap.tasks.push(Task {
            id: 12,
            needs: vec![2],
            ..Task::default()
        });
        snap.tasks[7].needs.push(1);
        snap.layers = vec![vec![1, 2, 3], vec![4, 5, 6, 12], vec![7, 8]];
        snap
    }

    #[test]
    fn a_status_change_reuses_the_layout_and_a_flip_recomputes_it() {
        let snap = fixture();
        let mut cache = DiagramCache::default();
        cache.layout(&snap, &snap.index(), Orientation::Vertical);
        assert_eq!(cache.computations(), 1);

        let mut moved = snap.clone();
        moved.tasks[3].status = TaskStatus::Done;
        moved.tasks[4].status = TaskStatus::InProgress;
        cache.layout(&moved, &moved.index(), Orientation::Vertical);
        assert_eq!(cache.computations(), 1, "status alone keeps the layout");

        cache.layout(&moved, &moved.index(), Orientation::Horizontal);
        assert_eq!(cache.computations(), 2, "an orientation flip recomputes");

        let grown = wide();
        cache.layout(&grown, &grown.index(), Orientation::Horizontal);
        assert_eq!(cache.computations(), 3, "a topology change recomputes");
    }

    #[test]
    fn right_and_down_reach_every_task() {
        for snap in [fixture(), wide()] {
            for orientation in BOTH {
                let mut cache = DiagramCache::default();
                let nav = navigator(&mut cache, &snap, &snap.index(), orientation);
                let start = cache.layout(&snap, &snap.index(), orientation).rows[0][0];
                let mut seen = HashSet::from([start]);
                let mut queue = VecDeque::from([start]);
                while let Some(id) = queue.pop_front() {
                    for dir in [Dir::Right, Dir::Down] {
                        if let Some(next) = nav.neighbor(id, dir)
                            && seen.insert(next)
                        {
                            queue.push_back(next);
                        }
                    }
                }
                let all: HashSet<u32> = snap.tasks.iter().map(|t| t.id).collect();
                assert_eq!(seen, all, "{orientation:?}");
            }
        }
    }

    #[test]
    fn layer_axis_moves_change_layer_and_cross_axis_moves_read_on() {
        let snap = fixture();
        for orientation in BOTH {
            let mut cache = DiagramCache::default();
            let nav = navigator(&mut cache, &snap, &snap.index(), orientation);
            let layout = cache.layout(&snap, &snap.index(), orientation);
            let (deeper, sideways) = match orientation {
                Orientation::Vertical => (Dir::Down, Dir::Right),
                Orientation::Horizontal => (Dir::Right, Dir::Down),
            };
            let first = layout.rows[0][0];
            let below = nav.neighbor(first, deeper).expect("layer 1 is not empty");
            assert!(layout.rows[1].contains(&below), "{orientation:?}");
            assert_eq!(nav.neighbor(first, sideways), Some(layout.rows[0][1]));
            let last_of_first = *layout.rows[0].last().unwrap();
            assert_eq!(
                nav.neighbor(last_of_first, sideways),
                Some(layout.rows[1][0])
            );
            let last = *layout.rows.last().unwrap().last().unwrap();
            assert_eq!(nav.neighbor(last, deeper), None);
            assert_eq!(nav.neighbor(99, deeper), None);
        }
    }

    /// Every edge's strokes join its two labels as one 4-connected run of cells.
    #[test]
    fn every_edge_connects_its_labels_in_both_orientations() {
        let snap = wide();
        for orientation in BOTH {
            let layout = layout(&snap, orientation);
            for (id, edge) in layout.edges.iter().enumerate() {
                let label_cells = |task: u32| -> Vec<(u16, u16)> {
                    let node = layout.node(task).expect("every task is drawn");
                    (node.x..node.x + node.width).map(|x| (x, node.y)).collect()
                };
                let mut open: HashSet<(u16, u16)> = layout
                    .strokes
                    .iter()
                    .filter(|s| s.edges.contains(&id))
                    .map(|s| (s.x, s.y))
                    .collect();
                open.extend(label_cells(edge.to));
                let target: HashSet<(u16, u16)> = label_cells(edge.to).into_iter().collect();
                let mut queue: VecDeque<(u16, u16)> = label_cells(edge.from).into();
                let mut reached = false;
                while let Some((x, y)) = queue.pop_front() {
                    if target.contains(&(x, y)) {
                        reached = true;
                        break;
                    }
                    let steps = [
                        (x.wrapping_sub(1), y),
                        (x + 1, y),
                        (x, y.wrapping_sub(1)),
                        (x, y + 1),
                    ];
                    for step in steps {
                        if open.remove(&step) {
                            queue.push_back(step);
                        }
                    }
                }
                assert!(reached, "{orientation:?} edge {edge:?} is broken");
            }
        }
    }

    /// Every arm a drawn glyph shows points at another stroke or a label, so no line
    /// runs across the axis it was routed along.
    #[test]
    fn glyph_arms_point_at_drawn_cells_in_both_orientations() {
        let snap = wide();
        for orientation in BOTH {
            let layout = layout(&snap, orientation);
            let mut drawn: HashSet<(u16, u16)> =
                layout.strokes.iter().map(|s| (s.x, s.y)).collect();
            for node in &layout.nodes {
                drawn.extend((node.x..node.x + node.width).map(|x| (x, node.y)));
            }
            for stroke in &layout.strokes {
                let (n, e, s, w) = match stroke.ch {
                    '│' | '┆' => (true, false, true, false),
                    '─' | '┄' => (false, true, false, true),
                    '└' | '╰' => (true, true, false, false),
                    '┘' | '╯' => (true, false, false, true),
                    '┌' | '╭' => (false, true, true, false),
                    '┐' | '╮' => (false, false, true, true),
                    '├' => (true, true, true, false),
                    '┤' => (true, false, true, true),
                    '┬' => (false, true, true, true),
                    '┴' => (true, true, false, true),
                    '┼' => (true, true, true, true),
                    _ => continue,
                };
                let (x, y) = (stroke.x, stroke.y);
                for (arm, cell) in [
                    (n, (x, y.wrapping_sub(1))),
                    (e, (x + 1, y)),
                    (s, (x, y + 1)),
                    (w, (x.wrapping_sub(1), y)),
                ] {
                    assert!(
                        !arm || drawn.contains(&cell),
                        "{orientation:?} {:?} at {:?} points at blank {cell:?}",
                        stroke.ch,
                        (x, y)
                    );
                }
            }
        }
    }

    #[test]
    fn layers_run_down_the_rows_or_across_the_columns() {
        let snap = fixture();
        let vertical = layout(&snap, Orientation::Vertical);
        let (one, four) = (vertical.node(1).unwrap(), vertical.node(4).unwrap());
        assert!(one.y < four.y);
        let horizontal = layout(&snap, Orientation::Horizontal);
        let (one, four) = (horizontal.node(1).unwrap(), horizontal.node(4).unwrap());
        assert!(one.x < four.x);
        assert!(horizontal.strokes.iter().any(|s| s.ch == '▶'));
        assert!(vertical.strokes.iter().any(|s| s.ch == '▼'));
    }

    #[test]
    fn transposing_twice_is_the_identity() {
        for ch in "│─┆┄└┐┘┌╰╮╯╭├┬┤┴┼ ".chars() {
            assert_eq!(transpose(transpose(ch)), ch);
        }
        assert_eq!(transpose('├'), '┬');
        assert_eq!(transpose('└'), '┐');
    }
}
