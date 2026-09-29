//! Edge routing through the channels between layers.
//!
//! Coordinates are abstract `(layer axis, cross axis)` cells. Layer `r`'s slots sit on
//! row [`Routed::layer_row`]`(r)`, and the channel between layers `r` and `r + 1` fills
//! the `channel_depth[r]` rows after it. Within a channel every edge segment joins the
//! bus of its source-side slot and edge kind; a bus that runs sideways takes one
//! horizontal track. Buses whose spans overlap are ordered by a dependency graph that
//! prefers the order with fewer crossings, and each bus's track is the longest path to
//! it through that graph (Sander 1996; ELK's `OrthogonalRoutingGenerator`). The row
//! after the last track holds the arrowheads of edges entering the lower layer.

use std::collections::{BTreeSet, HashMap};

use super::glyph::{ArmMask, CellOwners, Owner};
use super::order::{EdgeId, EdgeKind, Ordered, Slot};
use super::position::Positions;

/// A `(layer axis, cross axis)` coordinate.
pub(crate) type Cell = (u16, u16);

/// A vertical run landing on another bus's vertical merges two unrelated lines into
/// one, so it outweighs any number of plain crossings.
const OVERLAP_COST: usize = 1_000;

/// Which way an arrowhead points along the layer axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Arrow {
    /// Towards higher layer indices.
    Down,
    /// Towards lower layer indices.
    Up,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Routed {
    /// Rows in the channel after each layer but the last: its tracks plus the arrowhead
    /// row, plus a leading arrowhead row when an edge enters the upper layer.
    pub(crate) channel_depth: Vec<u16>,
    pub(crate) cells: HashMap<Cell, CellOwners>,
    /// Every cell an edge owns, ordered from its source to its target. Layer-row cells
    /// appear only where the edge passes through one of its dummies.
    pub(crate) edge_cells: HashMap<EdgeId, Vec<Cell>>,
    /// Cells drawn as an arrowhead instead of their [`glyph`](super::glyph::glyph).
    pub(crate) arrows: HashMap<Cell, Arrow>,
}

impl Routed {
    /// The layer-axis row holding layer `layer`'s slots.
    pub(crate) fn layer_row(&self, layer: usize) -> u16 {
        self.channel_depth[..layer.min(self.channel_depth.len())]
            .iter()
            .fold(0u16, |row, &depth| {
                row.saturating_add(depth).saturating_add(1)
            })
    }
}

/// All segments in one channel that leave the same source-side slot with one edge kind.
struct Bus {
    group: u32,
    /// Cross-axis cells where the bus meets the upper layer.
    upper: Vec<u16>,
    /// Cross-axis cells where the bus meets the lower layer.
    lower: Vec<u16>,
}

impl Bus {
    fn span(&self) -> (u16, u16) {
        let all = self.upper.iter().chain(&self.lower);
        let min = all.clone().copied().min().unwrap_or(0);
        let max = all.copied().max().unwrap_or(0);
        (min, max)
    }

    fn overlaps(&self, other: &Bus) -> bool {
        let (a, b) = (self.span(), other.span());
        a.0 <= b.1 && b.0 <= a.1
    }
}

/// One chain's segments, top-down: `(channel, upper x, lower x, bus index)`.
struct ChainRoute {
    chain: usize,
    downward: bool,
    xs: Vec<u16>,
    segments: Vec<(usize, u16, u16, usize)>,
}

/// Routes every chain of `ordered` orthogonally between the slot centres in `positions`.
///
/// A chain with a slot missing from `positions` is left unrouted.
pub(crate) fn route(ordered: &Ordered, positions: &Positions) -> Routed {
    let rows = ordered.rows.len();
    let channels = rows.saturating_sub(1);
    let mut buses: Vec<Vec<Bus>> = (0..channels).map(|_| Vec::new()).collect();
    let mut bus_index: HashMap<(usize, Slot, EdgeKind), usize> = HashMap::new();
    let mut leading_arrow = vec![false; channels];
    let mut next_group = 0u32;
    let mut routes = Vec::with_capacity(ordered.chains.len());

    for (index, chain) in ordered.chains.iter().enumerate() {
        let slots = &chain.slots;
        if slots.len() < 2 || chain.top + slots.len() > rows {
            continue;
        }
        let Some(xs) = slots
            .iter()
            .map(|&slot| positions.centre(slot))
            .collect::<Option<Vec<u16>>>()
        else {
            continue;
        };
        let downward = slots[0] == Slot::Task(chain.edge.from);
        let mut segments = Vec::with_capacity(slots.len() - 1);
        for k in 0..slots.len() - 1 {
            let channel = chain.top + k;
            let source = if downward { slots[k] } else { slots[k + 1] };
            let bus = *bus_index
                .entry((channel, source, chain.edge.kind))
                .or_insert_with(|| {
                    buses[channel].push(Bus {
                        group: next_group,
                        upper: Vec::new(),
                        lower: Vec::new(),
                    });
                    next_group += 1;
                    buses[channel].len() - 1
                });
            let entry = &mut buses[channel][bus];
            entry.upper.push(xs[k]);
            entry.lower.push(xs[k + 1]);
            segments.push((channel, xs[k], xs[k + 1], bus));
        }
        if !downward {
            leading_arrow[chain.top] = true;
        }
        routes.push(ChainRoute {
            chain: index,
            downward,
            xs,
            segments,
        });
    }
    for bus in buses.iter_mut().flatten() {
        bus.upper.sort_unstable();
        bus.upper.dedup();
        bus.lower.sort_unstable();
        bus.lower.dedup();
    }

    let tracks: Vec<Vec<Option<u16>>> = buses.iter().map(|b| assign_tracks(b)).collect();
    let mut routed = Routed {
        channel_depth: tracks
            .iter()
            .zip(&leading_arrow)
            .map(|(tracks, &leading)| {
                let count = tracks.iter().flatten().map(|&t| t + 1).max().unwrap_or(0);
                count + 1 + u16::from(leading)
            })
            .collect(),
        ..Routed::default()
    };
    let layer_rows: Vec<u16> = (0..rows).map(|r| routed.layer_row(r)).collect();

    let mut owners: HashMap<Cell, Vec<Owner>> = HashMap::new();
    for route in routes {
        let chain = &ordered.chains[route.chain];
        let mut path: Vec<(Cell, u32)> = Vec::new();
        for (k, &(channel, ux, lx, b)) in route.segments.iter().enumerate() {
            let group = buses[channel][b].group;
            let first = layer_rows[channel] + 1;
            let last = layer_rows[channel + 1] - 1;
            match tracks[channel][b] {
                Some(track) if ux != lx => {
                    let row = first + u16::from(leading_arrow[channel]) + track;
                    path.extend((first..=row).map(|r| ((r, ux), group)));
                    if ux < lx {
                        path.extend((ux + 1..=lx).map(|x| ((row, x), group)));
                    } else {
                        path.extend((lx..ux).rev().map(|x| ((row, x), group)));
                    }
                    path.extend((row + 1..=last).map(|r| ((r, lx), group)));
                }
                _ => path.extend((first..=last).map(|r| ((r, ux), group))),
            }
            if k + 1 < route.segments.len() {
                path.push(((layer_rows[channel + 1], lx), group));
            }
        }
        let top = (layer_rows[chain.top], route.xs[0]);
        let bottom = (
            layer_rows[chain.top + route.xs.len() - 1],
            route.xs[route.xs.len() - 1],
        );
        let (source, target) = if route.downward {
            (top, bottom)
        } else {
            path.reverse();
            (bottom, top)
        };

        let dashed = chain.edge.kind == EdgeKind::Coupling;
        for (i, &(cell, group)) in path.iter().enumerate() {
            let prev = if i == 0 { source } else { path[i - 1].0 };
            let next = path.get(i + 1).map_or(target, |&(cell, _)| cell);
            owners.entry(cell).or_default().push(Owner {
                edge_group: group,
                arms: arm(cell, prev).union(arm(cell, next)),
                dashed,
            });
        }
        if let Some(&(cell, _)) = path.last() {
            let arrow = if route.downward {
                Arrow::Down
            } else {
                Arrow::Up
            };
            routed.arrows.insert(cell, arrow);
        }
        routed
            .edge_cells
            .insert(chain.id, path.into_iter().map(|(cell, _)| cell).collect());
    }
    routed.cells = owners
        .into_iter()
        .map(|(cell, list)| (cell, pack(list)))
        .collect();
    routed
}

/// The arm of `cell` facing the orthogonally adjacent cell `towards`.
fn arm(cell: Cell, towards: Cell) -> ArmMask {
    if towards.0 < cell.0 {
        ArmMask::N
    } else if towards.0 > cell.0 {
        ArmMask::S
    } else if towards.1 > cell.1 {
        ArmMask::E
    } else if towards.1 < cell.1 {
        ArmMask::W
    } else {
        ArmMask::NONE
    }
}

/// Merges owners of one group, then fits the rest into the four owner slots. Past four
/// groups the cell collapses to the single owner `glyph` would resolve it to, which
/// draws the same character.
fn pack(list: Vec<Owner>) -> CellOwners {
    let mut merged: Vec<Owner> = Vec::with_capacity(list.len());
    for owner in list {
        match merged.iter_mut().find(|m| m.edge_group == owner.edge_group) {
            Some(m) => {
                m.arms = m.arms.union(owner.arms);
                m.dashed &= owner.dashed;
            }
            None => merged.push(owner),
        }
    }
    if merged.len() > 4 {
        let vertical = ArmMask::N.union(ArmMask::S);
        let any_vertical = merged.iter().any(|o| o.arms.has(vertical));
        let kept: Vec<Owner> = merged
            .iter()
            .copied()
            .filter(|o| !any_vertical || o.arms.has(vertical))
            .collect();
        merged = vec![Owner {
            edge_group: kept[0].edge_group,
            arms: kept.iter().fold(ArmMask::NONE, |m, o| m.union(o.arms)),
            dashed: kept.iter().all(|o| o.dashed),
        }];
    }
    let mut cell = CellOwners::new();
    for owner in merged {
        cell.push(owner.edge_group, owner.arms, owner.dashed);
    }
    cell
}

/// Crossings incurred by drawing `upper`'s track above `lower`'s: `upper`'s descents
/// through `lower`'s span, `lower`'s descents from the layer above through `upper`'s,
/// and any column where one bus's vertical would run along the other's.
fn cost(upper: &Bus, lower: &Bus) -> usize {
    let within = |x: &u16, (min, max): (u16, u16)| (min..=max).contains(x);
    let crossings = upper
        .lower
        .iter()
        .filter(|x| within(x, lower.span()))
        .count()
        + lower
            .upper
            .iter()
            .filter(|x| within(x, upper.span()))
            .count();
    let overlaps = upper
        .lower
        .iter()
        .filter(|x| lower.upper.contains(x))
        .count();
    crossings + OVERLAP_COST * overlaps
}

/// Tracks for one channel's buses, 0 nearest the upper layer; `None` for a bus that
/// runs straight down and needs no track.
///
/// Every overlapping pair is ordered the cheaper way round (the lower index above on
/// a tie). A topological sort that always takes the lowest ready index, and the lowest
/// remaining index when a cycle leaves none ready, fixes a total order; each bus then
/// sits one track below the deepest overlapping bus placed before it.
fn assign_tracks(buses: &[Bus]) -> Vec<Option<u16>> {
    let sideways: Vec<usize> = (0..buses.len())
        .filter(|&i| {
            let (min, max) = buses[i].span();
            min < max
        })
        .collect();
    let mut below: Vec<Vec<usize>> = vec![Vec::new(); buses.len()];
    let mut indegree = vec![0usize; buses.len()];
    for (n, &a) in sideways.iter().enumerate() {
        for &b in &sideways[n + 1..] {
            if !buses[a].overlaps(&buses[b]) {
                continue;
            }
            let (hi, lo) = if cost(&buses[b], &buses[a]) < cost(&buses[a], &buses[b]) {
                (b, a)
            } else {
                (a, b)
            };
            below[hi].push(lo);
            indegree[lo] += 1;
        }
    }

    let mut remaining: BTreeSet<usize> = sideways.iter().copied().collect();
    let mut tracks: Vec<Option<u16>> = vec![None; buses.len()];
    let mut placed: Vec<usize> = Vec::with_capacity(sideways.len());
    while let Some(&v) = remaining
        .iter()
        .find(|&&v| indegree[v] == 0)
        .or_else(|| remaining.first())
    {
        remaining.remove(&v);
        for &w in &below[v] {
            indegree[w] = indegree[w].saturating_sub(1);
        }
        let track = placed
            .iter()
            .filter(|&&u| buses[u].overlaps(&buses[v]))
            .filter_map(|&u| tracks[u])
            .map(|t| t + 1)
            .max()
            .unwrap_or(0);
        tracks[v] = Some(track);
        placed.push(v);
    }
    tracks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::glyph::glyph;
    use crate::diagram::order::{Chain, Edge, order};
    use crate::diagram::position::position;

    fn edge(from: u32, to: u32, kind: EdgeKind) -> Edge {
        Edge { from, to, kind }
    }

    fn needs(from: u32, to: u32) -> Edge {
        edge(from, to, EdgeKind::Needs)
    }

    /// Two-layer diagram with every task one cell wide, so its centre is its `x`.
    fn two_layers(
        upper: &[(u32, u16)],
        lower: &[(u32, u16)],
        edges: &[Edge],
    ) -> (Ordered, Positions) {
        let task = |&(id, _): &(u32, u16)| Slot::Task(id);
        let ordered = Ordered {
            rows: vec![
                upper.iter().map(task).collect(),
                lower.iter().map(task).collect(),
            ],
            chains: edges
                .iter()
                .enumerate()
                .map(|(id, &edge)| {
                    let downward = upper.iter().any(|&(t, _)| t == edge.from);
                    let (a, b) = if downward {
                        (edge.from, edge.to)
                    } else {
                        (edge.to, edge.from)
                    };
                    Chain {
                        id,
                        edge,
                        top: 0,
                        slots: vec![Slot::Task(a), Slot::Task(b)],
                    }
                })
                .collect(),
        };
        let all = upper.iter().chain(lower);
        let positions = Positions {
            x: all.clone().map(|&(id, x)| (Slot::Task(id), x)).collect(),
            width: all.clone().map(|&(id, _)| (Slot::Task(id), 1)).collect(),
            layer_extent: all.map(|&(_, x)| x + 1).max().unwrap_or(0),
        };
        (ordered, positions)
    }

    /// The row an edge runs sideways along, if it has one.
    fn track_row(routed: &Routed, id: EdgeId) -> Option<u16> {
        routed.edge_cells[&id]
            .windows(2)
            .find(|pair| pair[0].0 == pair[1].0)
            .map(|pair| pair[0].0)
    }

    /// Groups of the owners running sideways along an edge's track.
    fn groups(routed: &Routed, id: EdgeId) -> BTreeSet<u32> {
        let sideways = ArmMask::E.union(ArmMask::W);
        let row = track_row(routed, id).expect("edge runs sideways");
        routed.edge_cells[&id]
            .iter()
            .filter(|cell| cell.0 == row)
            .flat_map(|cell| routed.cells[cell].iter())
            .filter(|o| o.arms.has(sideways))
            .map(|o| o.edge_group)
            .collect()
    }

    #[test]
    fn overlapping_buses_get_distinct_tracks() {
        let (ordered, positions) = two_layers(
            &[(1, 0), (2, 4)],
            &[(3, 0), (4, 4)],
            &[needs(1, 4), needs(2, 3)],
        );
        let routed = route(&ordered, &positions);
        let (a, b) = (track_row(&routed, 0), track_row(&routed, 1));
        assert!(a.is_some() && b.is_some());
        assert_ne!(a, b);
    }

    #[test]
    fn channel_depth_is_tracks_plus_one() {
        let depth = |upper: &[(u32, u16)], lower: &[(u32, u16)], edges: &[Edge]| {
            let (ordered, positions) = two_layers(upper, lower, edges);
            route(&ordered, &positions).channel_depth
        };
        // Straight down: no track.
        assert_eq!(depth(&[(1, 2)], &[(2, 2)], &[needs(1, 2)]), vec![1]);
        // One source fanning out shares one bus.
        assert_eq!(
            depth(&[(1, 2)], &[(2, 0), (3, 4)], &[needs(1, 2), needs(1, 3)]),
            vec![2]
        );
        // Disjoint spans share a track.
        assert_eq!(
            depth(
                &[(1, 0), (2, 6)],
                &[(3, 2), (4, 8)],
                &[needs(1, 3), needs(2, 4)]
            ),
            vec![2]
        );
        // Three mutually overlapping buses.
        assert_eq!(
            depth(
                &[(1, 0), (2, 4), (3, 8)],
                &[(4, 2), (5, 6), (6, 10)],
                &[needs(1, 6), needs(2, 5), needs(3, 4)]
            ),
            vec![4]
        );
    }

    fn assert_connected(ordered: &Ordered, positions: &Positions, routed: &Routed) {
        for chain in &ordered.chains {
            let cells = &routed.edge_cells[&chain.id];
            let downward = chain.slots[0] == Slot::Task(chain.edge.from);
            let end = chain.slots.len() - 1;
            let (source_layer, target_layer) = if downward {
                (chain.top, chain.top + end)
            } else {
                (chain.top + end, chain.top)
            };
            let node = |layer: usize, id: u32| {
                (
                    routed.layer_row(layer),
                    positions.centre(Slot::Task(id)).unwrap(),
                )
            };
            let mut path = vec![node(source_layer, chain.edge.from)];
            path.extend(cells);
            path.push(node(target_layer, chain.edge.to));
            for pair in path.windows(2) {
                let step = pair[0].0.abs_diff(pair[1].0) + pair[0].1.abs_diff(pair[1].1);
                assert_eq!(step, 1, "edge {} jumps {:?}", chain.id, pair);
            }
            let arrow = if downward { Arrow::Down } else { Arrow::Up };
            assert_eq!(routed.arrows.get(cells.last().unwrap()), Some(&arrow));
            for cell in cells {
                assert!(routed.cells[cell].iter().next().is_some());
            }
        }
    }

    #[test]
    fn every_edge_is_a_connected_path_from_source_to_target() {
        let layers = vec![vec![1, 2, 3], vec![4, 5, 6, 7], vec![8, 9], vec![10, 11]];
        let edges = vec![
            needs(1, 7),
            needs(3, 4),
            needs(2, 5),
            needs(1, 10),
            needs(3, 11),
            needs(6, 9),
            needs(4, 8),
            needs(7, 8),
            needs(5, 9),
            edge(2, 6, EdgeKind::Coupling),
            needs(9, 10),
            needs(11, 2),
        ];
        let ordered = order(&layers, &edges);
        assert!(ordered.chains.iter().any(|c| c.slots.len() > 2));
        let positions = position(&ordered, |id| if id >= 10 { 4 } else { 3 });
        let routed = route(&ordered, &positions);
        assert_eq!(routed.edge_cells.len(), edges.len());
        assert_connected(&ordered, &positions, &routed);
    }

    #[test]
    fn coupling_buses_are_separate() {
        let (ordered, positions) = two_layers(
            &[(1, 4)],
            &[(2, 0), (3, 8)],
            &[needs(1, 2), edge(1, 3, EdgeKind::Coupling)],
        );
        let routed = route(&ordered, &positions);
        assert_ne!(track_row(&routed, 0), track_row(&routed, 1));
        assert!(groups(&routed, 0).is_disjoint(&groups(&routed, 1)));
        let dashed = |id: EdgeId| {
            routed.edge_cells[&id]
                .iter()
                .all(|cell| routed.cells[cell].iter().any(|o| o.dashed))
        };
        assert!(dashed(1));
        assert!(!dashed(0));
    }

    #[test]
    fn crowded_cell_draws_what_its_owners_would() {
        let owners: Vec<Owner> = (0..6)
            .map(|group| Owner {
                edge_group: group,
                arms: match group {
                    2 => ArmMask::E.union(ArmMask::W),
                    5 => ArmMask::S.union(ArmMask::E),
                    _ => ArmMask::N.union(ArmMask::S),
                },
                dashed: false,
            })
            .collect();
        assert_eq!(glyph(&pack(owners), false), '├');
    }

    #[test]
    fn cheaper_bus_order_wins_over_index_order() {
        // Above the other, the first bus would cross it twice; below it, not at all.
        let (ordered, positions) = two_layers(
            &[(1, 0), (2, 2)],
            &[(3, 4), (4, 6)],
            &[needs(1, 3), needs(2, 4)],
        );
        let routed = route(&ordered, &positions);
        assert!(track_row(&routed, 1) < track_row(&routed, 0));
    }

    #[test]
    fn output_is_identical_across_runs() {
        let (ordered, positions) = two_layers(
            &[(1, 0), (2, 4), (3, 8)],
            &[(4, 2), (5, 6), (6, 10)],
            &[needs(1, 6), needs(2, 4), needs(3, 5), needs(2, 6)],
        );
        let first = route(&ordered, &positions);
        for _ in 0..20 {
            assert_eq!(route(&ordered, &positions), first);
        }
    }
}
