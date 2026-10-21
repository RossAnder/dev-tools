//! The layered DAG diagram view: layout cache, orientation mapping and navigation.
//!
//! The layout leaves out the needs edges [`reduce`] finds implied unless asked to show
//! them, splits what remains into weakly connected components, and runs
//! [`order`](order::order) and [`position`](position::position) on each before
//! [`pack`](position::pack) sets them side by side and [`route`](route::route) draws
//! the edges, all in abstract `(layer axis, cross axis)` cells. It then maps them onto
//! the screen. Vertically the layer axis runs down the rows; horizontally it runs
//! across the columns, each layer's column widening to its longest label. A [`Zoom`]
//! sets the label width, and a focus or fit subset lays out only some of the tasks.
//! The layout depends only on task ids, edges, the implied-edge toggle, the zoom, the
//! subset and, at [`Zoom::Titles`], the drawn titles, so [`DiagramCache`] keeps it until
//! one of those or the orientation changes, and a status-only snapshot just repaints.

pub(crate) mod glyph;
pub(crate) mod order;
pub(crate) mod paint;
pub(crate) mod position;
pub(crate) mod reduce;
pub(crate) mod route;
pub(crate) mod subset;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::app::{Dir, Navigator};
use crate::config::Orientation;
use crate::model::{Index, Snapshot};

use glyph::ArmMask;
use order::{Edge, EdgeId, EdgeKind, Slot};
use route::{Arrow, Cell};
use subset::Focus;

/// Corners are drawn `╭╮╰╯` rather than `┌┐└┘`.
const ROUNDED: bool = true;

/// Characters of a title a [`Zoom::Titles`] label shows.
const TITLE_CAP: usize = 24;

/// How much of each task a diagram label shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Zoom {
    /// `[id] title`, the title cut to `TITLE_CAP` characters.
    Titles,
    #[default]
    Ids,
    /// One status glyph, with tasks a single cell apart.
    Tight,
}

impl Zoom {
    fn label_width(self, id: u32, title: &str) -> u16 {
        let digits = id.to_string().len();
        let cells = match self {
            Zoom::Titles => digits + 3 + title.chars().count().min(TITLE_CAP),
            Zoom::Ids => digits + 2,
            Zoom::Tight => 1,
        };
        u16::try_from(cells).unwrap_or(u16::MAX)
    }

    fn task_gap(self) -> u16 {
        match self {
            Zoom::Tight => 1,
            Zoom::Titles | Zoom::Ids => position::TASK_GAP,
        }
    }
}

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
    /// The edges laid out, indexed by [`EdgeId`]; hidden implied edges are absent.
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
    /// The snapshot's tasks a focus or fit subset left out.
    pub(crate) hidden: usize,
}

impl Layout {
    pub(crate) fn node(&self, id: u32) -> Option<&NodeBox> {
        self.node_index.get(&id).map(|&at| &self.nodes[at])
    }
}

/// Everything a stored layout was computed from. `subset` hashes the drawn ids and
/// `titles` the drawn titles, the latter only at [`Zoom::Titles`], since
/// `Index::topology_hash` covers neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LayoutKey {
    topology: u64,
    orientation: Orientation,
    show_implied: bool,
    zoom: Zoom,
    subset: Option<u64>,
    titles: Option<u64>,
}

/// Everything the subset choice depends on; `view` is `None` while fit is off, and
/// `titles` hashes every title at [`Zoom::Titles`], since they set the label widths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FitKey {
    topology: u64,
    orientation: Orientation,
    show_implied: bool,
    zoom: Zoom,
    focus: Focus,
    target: Option<u32>,
    view: Option<(u16, u16)>,
    titles: Option<u64>,
}

/// The ids a subset draws and how many of the snapshot's tasks it leaves out.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Chosen {
    ids: BTreeSet<u32>,
    hidden: usize,
}

/// Holds the last layout under one [`LayoutKey`] and recomputes it only when that key
/// changes, and the last subset choice under its [`FitKey`], so an unchanged frame runs
/// no fit search. The viewport scroll lives here too, since it must survive between
/// frames.
#[derive(Debug, Default)]
pub(crate) struct DiagramCache {
    key: Option<LayoutKey>,
    /// Whether implied needs edges are laid out; hidden by default.
    show_implied: bool,
    layout: Option<Layout>,
    computations: usize,
    fitted: Option<(FitKey, Option<Chosen>)>,
    searches: usize,
    /// Canvas cell at the viewport's top-left corner.
    scroll: (u16, u16),
}

impl DiagramCache {
    /// Layouts computed so far; unchanged by a snapshot that only moves statuses.
    #[cfg(test)]
    pub(crate) fn computations(&self) -> usize {
        self.computations
    }

    /// Subset choices made so far under fit, each of which measures candidate layouts.
    #[cfg(test)]
    fn searches(&self) -> usize {
        self.searches
    }

    /// Shows or hides the implied needs edges from the next layout on; a change re-lays
    /// out the diagram and resets the scroll like any other.
    pub(crate) fn set_implied(&mut self, show: bool) {
        self.show_implied = show;
    }

    /// The full graph at [`Zoom::Ids`].
    #[cfg(test)]
    pub(crate) fn layout(
        &mut self,
        snapshot: &Snapshot,
        index: &Index,
        orientation: Orientation,
    ) -> &Layout {
        self.refresh(snapshot, index, orientation);
        let show = self.show_implied;
        self.layout
            .get_or_insert_with(|| layout(snapshot, orientation, show))
    }

    /// Stores the full graph at [`Zoom::Ids`] unless it is already stored.
    fn refresh(&mut self, snapshot: &Snapshot, index: &Index, orientation: Orientation) {
        let key = LayoutKey {
            topology: index.topology_hash(),
            orientation,
            show_implied: self.show_implied,
            zoom: Zoom::Ids,
            subset: None,
            titles: None,
        };
        if self.key == Some(key) && self.layout.is_some() {
            return;
        }
        self.store(key, layout(snapshot, orientation, self.show_implied));
    }

    fn store(&mut self, key: LayoutKey, layout: Layout) {
        self.layout = Some(layout);
        self.key = Some(key);
        self.computations += 1;
        self.scroll = (0, 0);
    }

    /// The layout drawn into a `view` of `(width, height)` cells. With a `target`,
    /// `focus` other than off keeps only its closure; `fit` shrinks that, or the whole
    /// graph, to the largest neighbourhood of `target` whose layout fits `view`, when
    /// it does not fit already. With no `target` the whole graph is laid out.
    #[expect(clippy::too_many_arguments, reason = "each is one input of the key")]
    pub(crate) fn layout_for(
        &mut self,
        snapshot: &Snapshot,
        index: &Index,
        orientation: Orientation,
        view: (u16, u16),
        target: Option<u32>,
        focus: Focus,
        fit: bool,
        zoom: Zoom,
    ) -> &Layout {
        let titled = zoom == Zoom::Titles;
        let fit_key = FitKey {
            topology: index.topology_hash(),
            orientation,
            show_implied: self.show_implied,
            zoom,
            focus,
            target,
            view: fit.then_some(view),
            titles: titled.then(|| title_hash(snapshot, None)),
        };
        let source = Source::new(snapshot, orientation, self.show_implied, zoom);
        let (chosen, mut measured) = match &self.fitted {
            Some((key, chosen)) if *key == fit_key => (chosen.clone(), None),
            _ => {
                if fit_key.view.is_some() {
                    self.searches += 1;
                }
                let (chosen, measured) = choose(snapshot, &source, &fit_key);
                self.fitted = Some((fit_key, chosen.clone()));
                (chosen, measured)
            }
        };
        let ids = chosen.as_ref().map(|chosen| &chosen.ids);
        let key = LayoutKey {
            topology: fit_key.topology,
            orientation,
            show_implied: self.show_implied,
            zoom,
            subset: ids.map(ids_hash),
            titles: titled.then(|| title_hash(snapshot, ids)),
        };
        if self.key != Some(key) || self.layout.is_none() {
            let mut layout = measured.take().unwrap_or_else(|| source.layout(ids));
            layout.hidden = chosen.as_ref().map_or(0, |chosen| chosen.hidden);
            self.store(key, layout);
        }
        match &self.layout {
            Some(layout) => layout,
            None => unreachable!("a layout is stored above"),
        }
    }
}

/// The subset `key` asks for, and the layout of it when the search already measured
/// that, so it need not be laid out again.
fn choose(snapshot: &Snapshot, source: &Source, key: &FitKey) -> (Option<Chosen>, Option<Layout>) {
    let Some(target) = key
        .target
        .filter(|&target| snapshot.tasks.iter().any(|task| task.id == target))
    else {
        return (None, None);
    };
    let total = snapshot.tasks.len();
    let chosen = |ids: BTreeSet<u32>| Chosen {
        hidden: total.saturating_sub(ids.len()),
        ids,
    };
    let closure = (key.focus != Focus::Off)
        .then(|| subset::generations(snapshot, target, key.focus, usize::MAX));
    let Some((width, height)) = key.view else {
        return (closure.map(chosen), None);
    };
    let fits = |layout: &Layout| layout.width <= width && layout.height <= height;
    let whole = source.layout(closure.as_ref());
    if fits(&whole) {
        return (closure.map(chosen), Some(whole));
    }
    let mut accepted: Option<(BTreeSet<u32>, Layout)> = None;
    let found = subset::fit(snapshot, target, key.focus, |ids| {
        let layout = source.layout(Some(ids));
        let ok = fits(&layout);
        if ok {
            accepted = Some((ids.clone(), layout));
        }
        ok
    });
    let measured = accepted
        .filter(|(ids, _)| *ids == found.ids)
        .map(|(_, layout)| layout);
    (Some(chosen(found.ids)), measured)
}

/// The navigator for what [`paint::render`] draws in `orientation`, built from the
/// layout the cache last stored so a move lands on a drawn node. Computing a layout
/// here would replace that one and reset the scroll, so the full graph is laid out
/// only when nothing is stored yet.
pub(crate) fn navigator(
    cache: &mut DiagramCache,
    snapshot: &Snapshot,
    index: &Index,
    orientation: Orientation,
) -> Box<dyn Navigator> {
    if cache.layout.is_none() {
        cache.refresh(snapshot, index, orientation);
    }
    let (rows, centre) = cache
        .layout
        .as_ref()
        .map(|layout| (layout.rows.clone(), layout.centre.clone()))
        .unwrap_or_default();
    Box::new(DiagramNav {
        rows,
        centre,
        orientation,
    })
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

fn ids_hash(ids: &BTreeSet<u32>) -> u64 {
    let mut hash = FNV_OFFSET;
    for id in ids {
        fnv(&mut hash, &id.to_le_bytes());
    }
    hash
}

/// FNV-1a over each task's id and title cut to `TITLE_CAP` characters, in id order,
/// over the tasks in `only` or every task.
fn title_hash(snapshot: &Snapshot, only: Option<&BTreeSet<u32>>) -> u64 {
    let mut tasks: Vec<_> = snapshot
        .tasks
        .iter()
        .filter(|task| only.is_none_or(|ids| ids.contains(&task.id)))
        .collect();
    tasks.sort_by_key(|task| task.id);
    let mut hash = FNV_OFFSET;
    for task in tasks {
        fnv(&mut hash, &task.id.to_le_bytes());
        let cut: String = task.title.chars().take(TITLE_CAP).collect();
        let len = u32::try_from(cut.len()).unwrap_or(u32::MAX);
        fnv(&mut hash, &len.to_le_bytes());
        fnv(&mut hash, cut.as_bytes());
    }
    hash
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

/// What every layout of one snapshot at one zoom shares, read once so a fit search
/// lays out each candidate without going back to the snapshot.
struct Source<'s> {
    layers: Vec<Vec<u32>>,
    edges: Vec<Edge>,
    titles: HashMap<u32, &'s str>,
    orientation: Orientation,
    zoom: Zoom,
}

impl<'s> Source<'s> {
    fn new(
        snapshot: &'s Snapshot,
        orientation: Orientation,
        show_implied: bool,
        zoom: Zoom,
    ) -> Self {
        let titles = if zoom == Zoom::Titles {
            snapshot
                .tasks
                .iter()
                .map(|task| (task.id, task.title.as_str()))
                .collect()
        } else {
            HashMap::new()
        };
        Self {
            layers: layers(snapshot),
            edges: edges(snapshot, show_implied),
            titles,
            orientation,
            zoom,
        }
    }

    /// The layout of the tasks in `only`, or of every task. A subset keeps only its
    /// layers' members, drops the layers left empty and keeps only the edges with both
    /// ends inside it.
    fn layout(&self, only: Option<&BTreeSet<u32>>) -> Layout {
        let width = |id: u32| {
            let title = self.titles.get(&id).copied().unwrap_or_default();
            self.zoom.label_width(id, title)
        };
        let Some(ids) = only else {
            return layout_of(
                &self.layers,
                self.edges.clone(),
                self.orientation,
                self.zoom,
                width,
            );
        };
        let layers: Vec<Vec<u32>> = self
            .layers
            .iter()
            .map(|layer| {
                layer
                    .iter()
                    .copied()
                    .filter(|id| ids.contains(id))
                    .collect()
            })
            .filter(|layer: &Vec<u32>| !layer.is_empty())
            .collect();
        let edges = self
            .edges
            .iter()
            .filter(|edge| ids.contains(&edge.from) && ids.contains(&edge.to))
            .cloned()
            .collect();
        layout_of(&layers, edges, self.orientation, self.zoom, width)
    }
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
/// so a cached layout can never disagree with its key. Implied needs edges are left
/// out unless `show_implied`.
fn edges(snapshot: &Snapshot, show_implied: bool) -> Vec<Edge> {
    let hidden: HashSet<(u32, u32)> = if show_implied {
        HashSet::new()
    } else {
        reduce::implied_edges(snapshot).into_iter().collect()
    };
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
            edges.extend(
                from.into_iter()
                    .filter(|&from| {
                        kind == EdgeKind::Coupling || !hidden.contains(&(from, task.id))
                    })
                    .map(|from| Edge {
                        from,
                        to: task.id,
                        kind,
                    }),
            );
        }
    }
    edges
}

/// `layers` split into its weakly connected components over the edges [`order::order`]
/// draws, each as the full list of layers restricted to its tasks. The largest comes
/// first, ties by lowest task id.
fn components(layers: &[Vec<u32>], edges: &[Edge]) -> Vec<Vec<Vec<u32>>> {
    let mut layer_of: HashMap<u32, usize> = HashMap::new();
    for (layer, ids) in layers.iter().enumerate() {
        for &id in ids {
            layer_of.entry(id).or_insert(layer);
        }
    }
    let mut parent: HashMap<u32, u32> = layer_of.keys().map(|&id| (id, id)).collect();
    fn root(parent: &mut HashMap<u32, u32>, mut id: u32) -> u32 {
        while parent[&id] != id {
            let up = parent[&parent[&id]];
            parent.insert(id, up);
            id = up;
        }
        id
    }
    for edge in edges {
        let (Some(a), Some(b)) = (layer_of.get(&edge.from), layer_of.get(&edge.to)) else {
            continue;
        };
        if a != b {
            let (x, y) = (root(&mut parent, edge.from), root(&mut parent, edge.to));
            parent.insert(x.max(y), x.min(y));
        }
    }
    let mut members: BTreeMap<u32, HashSet<u32>> = BTreeMap::new();
    let ids: Vec<u32> = layer_of.keys().copied().collect();
    for id in ids {
        let r = root(&mut parent, id);
        members.entry(r).or_default().insert(id);
    }
    let mut groups: Vec<(u32, HashSet<u32>)> = members.into_iter().collect();
    groups.sort_by_key(|(first, set)| (std::cmp::Reverse(set.len()), *first));
    groups
        .into_iter()
        .map(|(_, set)| {
            layers
                .iter()
                .map(|ids| ids.iter().copied().filter(|id| set.contains(id)).collect())
                .collect()
        })
        .collect()
}

/// Every task of the snapshot at [`Zoom::Ids`].
fn layout(snapshot: &Snapshot, orientation: Orientation, show_implied: bool) -> Layout {
    Source::new(snapshot, orientation, show_implied, Zoom::Ids).layout(None)
}

/// Lays out `layers` joined by `edges`, each task `label_width` cells long, spaced for
/// `zoom`.
fn layout_of(
    layers: &[Vec<u32>],
    edges: Vec<Edge>,
    orientation: Orientation,
    zoom: Zoom,
    label_width: impl Fn(u32) -> u16,
) -> Layout {
    let horizontal = orientation == Orientation::Horizontal;
    // Horizontally a label runs along the layer axis, so it takes one cross-axis cell.
    let cross = |id| if horizontal { 1 } else { label_width(id) };
    let parts = components(layers, &edges)
        .into_iter()
        .map(|layers| {
            let ordered = order::order(&layers, &edges);
            let positions = position::position_with_gap(&ordered, cross, zoom.task_gap());
            (ordered, positions)
        })
        .collect();
    let (ordered, positions) = position::pack(parts);
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
            let widest = task_ids(row).into_iter().map(&label_width).max();
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
        hidden: 0,
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
    pub(super) fn wide() -> Snapshot {
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

    /// The fixture plus the needs edge 1 -> 7, which 1 -> 4 -> 7 implies.
    pub(super) fn shortcut() -> Snapshot {
        let mut snap = fixture();
        snap.tasks[6].needs.push(1);
        snap
    }

    fn has_edge(layout: &Layout, from: u32, to: u32) -> bool {
        layout.edges.iter().any(|e| (e.from, e.to) == (from, to))
    }

    #[test]
    fn implied_edges_are_hidden_until_the_toggle_shows_them() {
        let snap = shortcut();
        let index = snap.index();
        let mut cache = DiagramCache::default();
        assert!(!has_edge(
            cache.layout(&snap, &index, Orientation::Vertical),
            1,
            7
        ));
        assert!(has_edge(
            cache.layout(&snap, &index, Orientation::Vertical),
            1,
            4
        ));
        assert_eq!(cache.computations(), 1);

        cache.scroll = (3, 4);
        cache.set_implied(true);
        assert!(has_edge(
            cache.layout(&snap, &index, Orientation::Vertical),
            1,
            7
        ));
        assert_eq!(cache.computations(), 2, "showing implied edges recomputes");
        assert_eq!(cache.scroll, (0, 0), "a re-layout resets the scroll");

        let mut moved = snap.clone();
        moved.tasks[3].status = TaskStatus::Done;
        cache.set_implied(true);
        cache.layout(&moved, &moved.index(), Orientation::Vertical);
        assert_eq!(cache.computations(), 2, "status alone keeps the layout");

        cache.set_implied(false);
        assert!(!has_edge(
            cache.layout(&moved, &moved.index(), Orientation::Vertical),
            1,
            7
        ));
        assert_eq!(cache.computations(), 3, "hiding them again recomputes");
    }

    /// Two components: a chain that widens at the bottom, and a two-task chain beside
    /// its narrow top.
    pub(super) fn two_components() -> Snapshot {
        let task = |id: u32, needs: &[u32]| Task {
            id,
            needs: needs.to_vec(),
            ..Task::default()
        };
        Snapshot {
            tasks: vec![
                task(1, &[]),
                task(2, &[1]),
                task(3, &[2]),
                task(4, &[2]),
                task(5, &[2]),
                task(6, &[2]),
                task(7, &[]),
                task(8, &[7]),
            ],
            layers: vec![vec![1, 7], vec![2, 8], vec![3, 4, 5, 6]],
            ..Snapshot::default()
        }
    }

    #[test]
    fn components_sit_side_by_side_and_tuck_into_free_rows() {
        let snap = two_components();
        for orientation in BOTH {
            let layout = layout(&snap, orientation, false);
            let cross = |id: u32| {
                let node = layout.node(id).expect("drawn");
                match orientation {
                    Orientation::Vertical => (node.x, node.x + node.width),
                    Orientation::Horizontal => (node.y, node.y + 1),
                }
            };
            // The small component follows the large one in every row it shares.
            assert_eq!(layout.rows[0], vec![1, 7], "{orientation:?}");
            assert_eq!(layout.rows[1], vec![2, 8], "{orientation:?}");
            let big_end = [1, 2, 3, 4, 5, 6].map(|id| cross(id).1).into_iter().max();
            let small_start = [7, 8].map(|id| cross(id).0).into_iter().min().unwrap();
            let top_end = [1, 2].map(|id| cross(id).1).into_iter().max().unwrap();
            let big_end = big_end.unwrap();
            assert!(small_start > top_end, "{orientation:?}");
            assert!(
                small_start < big_end,
                "{orientation:?}: [7] and [8] tuck in above the wide row"
            );
            // No stroke of one component strays into the other's cells.
            let small_strokes = layout.strokes.iter().filter(|s| {
                s.edges
                    .iter()
                    .any(|&e| layout.edges[e].from == 7 || layout.edges[e].to == 7)
            });
            for stroke in small_strokes {
                let at = match orientation {
                    Orientation::Vertical => stroke.x,
                    Orientation::Horizontal => stroke.y,
                };
                assert!(at >= small_start, "{orientation:?}");
            }
        }
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

    /// Layouts of `wide()` in `orientation` to hold the drawing invariants to: the
    /// full graph and focus subsets at every zoom.
    fn invariant_cases(orientation: Orientation) -> Vec<(String, Layout)> {
        let snap = wide();
        let mut cases = Vec::new();
        for zoom in [Zoom::Titles, Zoom::Ids, Zoom::Tight] {
            let source = Source::new(&snap, orientation, false, zoom);
            let name = format!("{orientation:?} {zoom:?} whole");
            cases.push((name, source.layout(None)));
            for (target, focus, radius) in [
                (4, Focus::Both, usize::MAX),
                (5, Focus::Off, 1),
                (7, Focus::Ancestry, usize::MAX),
            ] {
                let ids = subset::generations(&snap, target, focus, radius);
                let name = format!("{orientation:?} {zoom:?} {focus:?} of {target}");
                cases.push((name, source.layout(Some(&ids))));
            }
        }
        cases
    }

    /// Every edge's strokes join its two labels as one 4-connected run of cells.
    #[test]
    fn every_edge_connects_its_labels_in_both_orientations() {
        for orientation in BOTH {
            for (case, layout) in invariant_cases(orientation) {
                assert_edges_connect(&layout, &case);
            }
        }
    }

    fn assert_edges_connect(layout: &Layout, case: &str) {
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
            assert!(reached, "{case}: edge {edge:?} is broken");
        }
    }

    /// Every arm a drawn glyph shows points at another stroke or a label, so no line
    /// runs across the axis it was routed along.
    #[test]
    fn glyph_arms_point_at_drawn_cells_in_both_orientations() {
        for orientation in BOTH {
            for (case, layout) in invariant_cases(orientation) {
                assert_arms_drawn(&layout, &case);
            }
        }
    }

    fn assert_arms_drawn(layout: &Layout, case: &str) {
        let mut drawn: HashSet<(u16, u16)> = layout.strokes.iter().map(|s| (s.x, s.y)).collect();
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
                    "{case}: {:?} at {:?} points at blank {cell:?}",
                    stroke.ch,
                    (x, y)
                );
            }
        }
    }

    /// The subset drawn by `cache`'s stored layout.
    fn drawn_ids(cache: &DiagramCache) -> BTreeSet<u32> {
        let layout = cache.layout.as_ref().expect("a layout is stored");
        layout.nodes.iter().map(|node| node.id).collect()
    }

    #[test]
    fn a_subset_keeps_its_members_and_the_edges_inside_it() {
        let snap = wide();
        let ids = subset::generations(&snap, 4, Focus::Both, usize::MAX);
        for orientation in BOTH {
            let layout = Source::new(&snap, orientation, false, Zoom::Ids).layout(Some(&ids));
            let drawn: BTreeSet<u32> = layout.nodes.iter().map(|node| node.id).collect();
            assert_eq!(drawn, ids, "{orientation:?}");
            assert_eq!(layout.rows.len(), 3, "{orientation:?}: one row per layer");
            assert!(
                layout
                    .edges
                    .iter()
                    .all(|e| ids.contains(&e.from) && ids.contains(&e.to)),
                "{orientation:?}"
            );
            assert!(has_edge(&layout, 1, 4) && has_edge(&layout, 4, 7));
        }
    }

    #[test]
    fn tight_is_narrower_than_ids_and_titles_wider() {
        let mut snap = wide();
        for task in &mut snap.tasks {
            task.title = format!("task number {}", task.id);
        }
        let width = |zoom| {
            Source::new(&snap, Orientation::Vertical, false, zoom)
                .layout(None)
                .width
        };
        assert!(width(Zoom::Tight) < width(Zoom::Ids));
        assert!(width(Zoom::Ids) < width(Zoom::Titles));
        assert_eq!(Zoom::Titles.label_width(12, &"x".repeat(40)), 2 + 3 + 24);
        assert_eq!(Zoom::Ids.label_width(12, "anything"), 4);
        assert_eq!(Zoom::Tight.label_width(12, "anything"), 1);
    }

    /// One `layout_for` call's inputs beyond the snapshot.
    #[derive(Clone, Copy)]
    struct Ask {
        orientation: Orientation,
        view: (u16, u16),
        target: Option<u32>,
        focus: Focus,
        fit: bool,
        zoom: Zoom,
    }

    impl Ask {
        /// Vertical, fit off, in a view nothing overflows.
        fn new(target: u32, focus: Focus, zoom: Zoom) -> Self {
            Self {
                orientation: Orientation::Vertical,
                view: (u16::MAX, u16::MAX),
                target: Some(target),
                focus,
                fit: false,
                zoom,
            }
        }

        fn run<'c>(self, cache: &'c mut DiagramCache, snap: &Snapshot) -> &'c Layout {
            cache.layout_for(
                snap,
                &snap.index(),
                self.orientation,
                self.view,
                self.target,
                self.focus,
                self.fit,
                self.zoom,
            )
        }
    }

    #[test]
    fn layout_for_recomputes_on_zoom_subset_and_drawn_titles_only() {
        let snap = wide();
        let mut cache = DiagramCache::default();
        Ask::new(4, Focus::Off, Zoom::Ids).run(&mut cache, &snap);
        assert_eq!(cache.computations(), 1);
        assert_eq!(cache.layout.as_ref().unwrap().hidden, 0);

        Ask::new(5, Focus::Off, Zoom::Ids).run(&mut cache, &snap);
        assert_eq!(cache.computations(), 1, "a new target keeps the graph");

        Ask::new(4, Focus::Off, Zoom::Tight).run(&mut cache, &snap);
        assert_eq!(cache.computations(), 2, "a zoom change recomputes");

        Ask::new(4, Focus::Both, Zoom::Tight).run(&mut cache, &snap);
        assert_eq!(cache.computations(), 3, "a subset change recomputes");
        assert_eq!(drawn_ids(&cache), BTreeSet::from([1, 4, 7]));
        assert_eq!(cache.layout.as_ref().unwrap().hidden, 6);

        let titles = Ask::new(4, Focus::Both, Zoom::Titles);
        titles.run(&mut cache, &snap);
        assert_eq!(cache.computations(), 4);
        let mut retitled = snap.clone();
        retitled.tasks[7].title = "a hidden task's new title".into();
        assert_eq!(retitled.tasks[7].id, 8);
        titles.run(&mut cache, &retitled);
        assert_eq!(cache.computations(), 4, "a hidden title is not drawn");
        retitled.tasks[3].title = "a drawn task's new title".into();
        assert_eq!(retitled.tasks[3].id, 4);
        titles.run(&mut cache, &retitled);
        assert_eq!(cache.computations(), 5, "a drawn title recomputes");

        retitled.tasks[3].status = TaskStatus::Done;
        titles.run(&mut cache, &retitled);
        assert_eq!(cache.computations(), 5, "status alone keeps the layout");
        let ids = Ask::new(4, Focus::Both, Zoom::Ids);
        ids.run(&mut cache, &retitled);
        assert_eq!(cache.computations(), 6);
        retitled.tasks[3].title = "titles do not matter at ids".into();
        ids.run(&mut cache, &retitled);
        assert_eq!(cache.computations(), 6);
    }

    #[test]
    fn unchanged_inputs_run_no_fit_search_and_keep_a_pinned_scroll() {
        let snap = wide();
        let full = layout(&snap, Orientation::Horizontal, false);
        let ask = Ask {
            orientation: Orientation::Horizontal,
            view: (full.width - 1, full.height),
            fit: true,
            ..Ask::new(7, Focus::Off, Zoom::Ids)
        };
        let mut cache = DiagramCache::default();
        let fitted = ask.run(&mut cache, &snap);
        assert!(fitted.width <= ask.view.0 && fitted.height <= ask.view.1);
        assert!(fitted.hidden > 0, "the whole graph overflows by a column");
        let (computations, searches) = (cache.computations(), cache.searches());
        assert_eq!((computations, searches), (1, 1));

        cache.scroll = (3, 4);
        ask.run(&mut cache, &snap);
        assert_eq!(cache.computations(), computations);
        assert_eq!(cache.searches(), searches, "unchanged inputs run no search");
        assert_eq!(cache.scroll, (3, 4), "the pinned scroll survives");

        let roomy = Ask {
            view: (full.width, full.height),
            ..ask
        };
        let whole = roomy.run(&mut cache, &snap).clone();
        assert_eq!(cache.searches(), searches + 1, "a resize searches again");
        assert_eq!(whole.hidden, 0, "the whole graph fits");
        assert_eq!(whole, full);
    }

    #[test]
    fn fit_without_a_selection_draws_the_whole_graph() {
        let snap = wide();
        let mut cache = DiagramCache::default();
        let ask = Ask {
            view: (1, 1),
            target: None,
            fit: true,
            ..Ask::new(0, Focus::Subtree, Zoom::Ids)
        };
        let drawn = ask.run(&mut cache, &snap);
        assert_eq!(drawn.hidden, 0);
        assert_eq!(drawn.nodes.len(), snap.tasks.len());
    }

    #[test]
    fn the_navigator_moves_over_the_layout_last_stored() {
        let snap = wide();
        let index = snap.index();
        for orientation in BOTH {
            let mut cache = DiagramCache::default();
            let ask = Ask {
                orientation,
                ..Ask::new(4, Focus::Both, Zoom::Ids)
            };
            ask.run(&mut cache, &snap);
            cache.scroll = (2, 2);
            let nav = navigator(&mut cache, &snap, &index, orientation);
            assert_eq!(cache.computations(), 1, "{orientation:?}: no re-layout");
            assert_eq!(cache.scroll, (2, 2), "{orientation:?}");
            let mut seen = HashSet::from([1]);
            let mut queue = VecDeque::from([1]);
            while let Some(id) = queue.pop_front() {
                for dir in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
                    if let Some(next) = nav.neighbor(id, dir)
                        && seen.insert(next)
                    {
                        queue.push_back(next);
                    }
                }
            }
            assert_eq!(seen, HashSet::from([1, 4, 7]), "{orientation:?}");
        }
    }

    #[test]
    fn layers_run_down_the_rows_or_across_the_columns() {
        let snap = fixture();
        let vertical = layout(&snap, Orientation::Vertical, false);
        let (one, four) = (vertical.node(1).unwrap(), vertical.node(4).unwrap());
        assert!(one.y < four.y);
        let horizontal = layout(&snap, Orientation::Horizontal, false);
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
