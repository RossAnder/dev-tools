//! Cross-axis coordinate assignment for ordered layers.
//!
//! Sugiyama's priority method (Sugiyama, Tagawa and Toda 1981): alternating sweeps
//! move each slot towards the barycentre of its neighbours in the reference row,
//! highest priority first. A slot may push lower-priority slots aside but never one
//! already placed in the same pass. Dummies outrank every task, so a long edge stays
//! straight wherever nothing placed before it blocks the way. Equal priorities are
//! placed from the middle of the row outwards, so a parent settles over its middle
//! child rather than its first. A final step moves any dummy that lines up with an
//! unrelated edge's task or dummy in the next or previous layer, so the two never draw
//! as one line.

use std::cmp::Reverse;
use std::collections::HashMap;

use super::order::{Ordered, Slot};

/// Down, up, down, up, down: the last pass aligns each chain with its upper endpoint.
const PASSES: usize = 5;
/// Blank cells between two slots when either is a task.
const TASK_GAP: i32 = 2;
/// Blank cells between two adjacent dummies.
const DUMMY_GAP: i32 = 1;
/// Cap on the rounds that move dummies off unrelated columns, since each push can
/// uncover a new clash in the rows next to it.
const SEPARATE_ROUNDS: usize = 8;

/// Coordinates along the slot axis, in cells: across the page when layers run down it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Positions {
    /// Leading cell of each slot; the leading cell of the whole diagram is 0.
    pub(crate) x: HashMap<Slot, u16>,
    /// Cells a slot occupies: its label width for a task (at least 1), 1 for a dummy.
    pub(crate) width: HashMap<Slot, u16>,
    /// One past the furthest cell any slot occupies.
    pub(crate) layer_extent: u16,
}

impl Positions {
    /// The cell where an edge meets the slot; for a dummy, the cell its line runs through.
    pub(crate) fn centre(&self, slot: Slot) -> Option<u16> {
        Some(self.x.get(&slot)? + self.width.get(&slot)? / 2)
    }
}

/// Places every slot of `ordered` along the slot axis, keeping each row's order.
///
/// Adjacent slots are separated by at least [`TASK_GAP`] blank cells, or
/// [`DUMMY_GAP`] between two dummies, so a row of tasks has pitch `width + 2` and a
/// run of dummies pitch 2.
pub(crate) fn position(ordered: &Ordered, label_width: impl Fn(u32) -> u16) -> Positions {
    let rows = &ordered.rows;
    let widths: Vec<Vec<i32>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|&slot| match slot {
                    Slot::Task(id) => i32::from(label_width(id).max(1)),
                    Slot::Dummy { .. } => 1,
                })
                .collect()
        })
        .collect();
    let index: HashMap<Slot, (usize, usize)> = rows
        .iter()
        .enumerate()
        .flat_map(|(r, row)| row.iter().enumerate().map(move |(i, &slot)| (slot, (r, i))))
        .collect();

    let mut up: Vec<Vec<Vec<usize>>> = rows.iter().map(|row| vec![Vec::new(); row.len()]).collect();
    let mut down = up.clone();
    for chain in &ordered.chains {
        for pair in chain.slots.windows(2) {
            let (Some(&(ra, ia)), Some(&(rb, ib))) = (index.get(&pair[0]), index.get(&pair[1]))
            else {
                continue;
            };
            if rb == ra + 1 {
                down[ra][ia].push(ib);
                up[rb][ib].push(ia);
            }
        }
    }

    let mut xs: Vec<Vec<i32>> = rows
        .iter()
        .zip(&widths)
        .map(|(row, widths)| {
            let mut next = 0;
            (0..row.len())
                .map(|i| {
                    let x = next;
                    if i + 1 < row.len() {
                        next += widths[i] + gap(row[i], row[i + 1]);
                    }
                    x
                })
                .collect()
        })
        .collect();

    let count = rows.len();
    for pass in 0..PASSES {
        let sweep: Vec<(usize, usize)> = if pass % 2 == 0 {
            (1..count).map(|r| (r, r - 1)).collect()
        } else {
            (0..count.saturating_sub(1))
                .rev()
                .map(|r| (r, r + 1))
                .collect()
        };
        for (r, reference) in sweep {
            let centres: Vec<i32> = xs[reference]
                .iter()
                .zip(&widths[reference])
                .map(|(&x, &w)| x + w / 2)
                .collect();
            let neighbours = if reference < r { &up[r] } else { &down[r] };
            place(&mut xs[r], &widths[r], &rows[r], neighbours, &centres);
        }
    }
    separate_dummies(&mut xs, &widths, rows, &up, &down);

    let shift = xs.iter().flatten().copied().min().unwrap_or(0);
    let to_cell = |v: i32| u16::try_from(v.max(0)).unwrap_or(u16::MAX);
    let mut positions = Positions {
        x: HashMap::with_capacity(index.len()),
        width: HashMap::with_capacity(index.len()),
        layer_extent: 0,
    };
    for ((row, xs), widths) in rows.iter().zip(&xs).zip(&widths) {
        for ((&slot, &x), &w) in row.iter().zip(xs).zip(widths) {
            positions.x.insert(slot, to_cell(x - shift));
            positions.width.insert(slot, to_cell(w));
            positions.layer_extent = positions.layer_extent.max(to_cell(x - shift + w));
        }
    }
    positions
}

fn gap(left: Slot, right: Slot) -> i32 {
    match (left, right) {
        (Slot::Dummy { .. }, Slot::Dummy { .. }) => DUMMY_GAP,
        _ => TASK_GAP,
    }
}

/// One row of one pass. `neighbours[i]` indexes the reference row, whose slot centres
/// are `centres`. Slots with no neighbour there only move when pushed.
fn place(
    xs: &mut [i32],
    widths: &[i32],
    slots: &[Slot],
    neighbours: &[Vec<usize>],
    centres: &[i32],
) {
    let len = slots.len();
    let seps = separations(widths, slots);
    let mid = len.saturating_sub(1) / 2;
    let mut order: Vec<usize> = (0..len).filter(|&i| !neighbours[i].is_empty()).collect();
    order.sort_by_key(|&i| {
        let priority = match slots[i] {
            Slot::Dummy { .. } => usize::MAX,
            Slot::Task(_) => neighbours[i].len(),
        };
        (Reverse(priority), i.abs_diff(mid), i)
    });

    let mut locked = vec![false; len];
    for v in order {
        let target = barycentre(&neighbours[v], centres) - widths[v] / 2;
        if target > xs[v] {
            let limit = (v + 1..len)
                .find(|&k| locked[k])
                .map(|k| xs[k] - seps[v..k].iter().sum::<i32>());
            xs[v] = limit.map_or(target, |limit| target.min(limit));
            push_right(xs, &seps, v);
        } else if target < xs[v] {
            let limit = (0..v)
                .rev()
                .find(|&k| locked[k])
                .map(|k| xs[k] + seps[k..v].iter().sum::<i32>());
            xs[v] = limit.map_or(target, |limit| target.max(limit));
            push_left(xs, &seps, v);
        }
        locked[v] = true;
    }
}

/// `seps[i]` is the least distance from slot `i`'s leading cell to slot `i + 1`'s.
fn separations(widths: &[i32], slots: &[Slot]) -> Vec<i32> {
    (0..slots.len().saturating_sub(1))
        .map(|i| widths[i] + gap(slots[i], slots[i + 1]))
        .collect()
}

/// Moves the slots after `v` right just far enough to clear it.
fn push_right(xs: &mut [i32], seps: &[i32], v: usize) {
    for j in v + 1..xs.len() {
        let min = xs[j - 1] + seps[j - 1];
        if xs[j] >= min {
            break;
        }
        xs[j] = min;
    }
}

/// Moves the slots before `v` left just far enough to clear it.
fn push_left(xs: &mut [i32], seps: &[i32], v: usize) {
    for j in (0..v).rev() {
        let max = xs[j + 1] - seps[j];
        if xs[j] <= max {
            break;
        }
        xs[j] = max;
    }
}

/// Steps each dummy off any column where a slot in an adjacent row, task or another
/// edge's dummy, meets an edge in the channel between them, unless that slot is the
/// dummy's own chain neighbour: the two verticals would join into one line and read as
/// a single edge. The dummy takes the nearest free column, preferring one that needs no
/// push and then the side its chain bends towards. A push can land a slot on another
/// row's dummy, so rows repeat until nothing moves, at most [`SEPARATE_ROUNDS`] times.
fn separate_dummies(
    xs: &mut [Vec<i32>],
    widths: &[Vec<i32>],
    rows: &[Vec<Slot>],
    up: &[Vec<Vec<usize>>],
    down: &[Vec<Vec<usize>>],
) {
    let centre = |xs: &[Vec<i32>], r: usize, j: usize| xs[r][j] + widths[r][j] / 2;
    for _ in 0..SEPARATE_ROUNDS {
        let mut moved = false;
        for r in 0..rows.len() {
            let seps = separations(&widths[r], &rows[r]);
            for i in 0..rows[r].len() {
                if !matches!(rows[r][i], Slot::Dummy { .. }) {
                    continue;
                }
                let mut taken = Vec::new();
                let mut linked = Vec::new();
                let adjacent = [
                    r.checked_sub(1).map(|a| (a, &up[r][i], down)),
                    (r + 1 < rows.len()).then(|| (r + 1, &down[r][i], up)),
                ];
                for (a, own, crossing) in adjacent.into_iter().flatten() {
                    linked.extend(own.iter().map(|&j| centre(xs, a, j)));
                    for (j, edges) in crossing[a].iter().enumerate() {
                        if !edges.is_empty() && !own.contains(&j) {
                            taken.push(centre(xs, a, j));
                        }
                    }
                }
                let x = centre(xs, r, i);
                if !taken.contains(&x) {
                    continue;
                }
                let bend = linked.iter().map(|&c| c - x).sum::<i32>();
                let lo = (i > 0).then(|| xs[r][i - 1] + seps[i - 1]);
                let hi = (i + 1 < rows[r].len()).then(|| xs[r][i + 1] - seps[i]);
                let reach = i32::try_from(taken.len()).unwrap_or(i32::MAX - 1) + 1;
                let Some(to) = (1..=reach)
                    .flat_map(|k| [x + k, x - k])
                    .filter(|c| !taken.contains(c))
                    .min_by_key(|&c| {
                        let pushes = lo.is_some_and(|lo| c < lo) || hi.is_some_and(|hi| c > hi);
                        let against = (c - x).signum() != bend.signum() && bend != 0;
                        (pushes, c.abs_diff(x), against, c < x)
                    })
                else {
                    continue;
                };
                xs[r][i] += to - x;
                if to > x {
                    push_right(&mut xs[r], &seps, i);
                } else {
                    push_left(&mut xs[r], &seps, i);
                }
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// Mean of the neighbours' centres, rounded half up; `neighbours` is non-empty.
fn barycentre(neighbours: &[usize], centres: &[i32]) -> i32 {
    let n = neighbours.len() as i32;
    let sum: i32 = neighbours.iter().map(|&i| centres[i]).sum();
    (2 * sum + n).div_euclid(2 * n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::order::{Chain, Edge, EdgeKind, order};

    fn needs(from: u32, to: u32) -> Edge {
        Edge {
            from,
            to,
            kind: EdgeKind::Needs,
        }
    }

    fn sample() -> Ordered {
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
            Edge {
                from: 2,
                to: 6,
                kind: EdgeKind::Coupling,
            },
            needs(9, 10),
        ];
        order(&layers, &edges)
    }

    fn width(id: u32) -> u16 {
        if id >= 10 { 4 } else { 3 }
    }

    /// Blank cells between each adjacent pair, per row, in row order.
    fn gaps(ordered: &Ordered, positions: &Positions) -> Vec<Vec<(Slot, Slot, i32)>> {
        ordered
            .rows
            .iter()
            .map(|row| {
                row.windows(2)
                    .map(|pair| {
                        let end = i32::from(positions.x[&pair[0]] + positions.width[&pair[0]]);
                        (pair[0], pair[1], i32::from(positions.x[&pair[1]]) - end)
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn slots_do_not_overlap_and_keep_row_order() {
        let ordered = sample();
        let positions = position(&ordered, width);
        for row in gaps(&ordered, &positions) {
            for (left, right, blank) in row {
                assert!(blank >= 0, "{left:?} overlaps {right:?}");
            }
        }
        let slots = ordered.rows.iter().flatten().count();
        assert_eq!(positions.x.len(), slots);
        assert_eq!(positions.x.values().min(), Some(&0));
        let extent = ordered
            .rows
            .iter()
            .flatten()
            .map(|slot| positions.x[slot] + positions.width[slot])
            .max();
        assert_eq!(Some(positions.layer_extent), extent);
    }

    #[test]
    fn minimum_spacing_is_respected() {
        let ordered = sample();
        let positions = position(&ordered, width);
        for row in gaps(&ordered, &positions) {
            for (left, right, blank) in row {
                let both_dummies =
                    matches!((left, right), (Slot::Dummy { .. }, Slot::Dummy { .. }));
                let min = if both_dummies { 1 } else { 2 };
                assert!(blank >= min, "{left:?} {right:?} gap {blank}");
            }
        }
    }

    #[test]
    fn siblings_pack_at_pitch_under_a_centred_parent() {
        let ordered = order(
            &[vec![1], vec![2, 3, 4]],
            &[needs(1, 2), needs(1, 3), needs(1, 4)],
        );
        let positions = position(&ordered, |_| 4);
        let row = &ordered.rows[1];
        for pair in row.windows(2) {
            assert_eq!(positions.x[&pair[1]] - positions.x[&pair[0]], 6);
        }
        assert_eq!(positions.centre(Slot::Task(1)), positions.centre(row[1]));
    }

    #[test]
    fn unobstructed_dummy_chain_shares_one_x() {
        let dummy = |step| Slot::Dummy { edge: 1, step };
        let chain = |id, edge, top, slots| Chain {
            id,
            edge,
            top,
            slots,
        };
        let ordered = Ordered {
            rows: vec![
                vec![Slot::Task(1)],
                vec![Slot::Task(2), dummy(1)],
                vec![Slot::Task(3), dummy(2)],
                vec![Slot::Task(4)],
            ],
            chains: vec![
                chain(0, needs(1, 2), 0, vec![Slot::Task(1), Slot::Task(2)]),
                chain(
                    1,
                    needs(1, 4),
                    0,
                    vec![Slot::Task(1), dummy(1), dummy(2), Slot::Task(4)],
                ),
                chain(2, needs(2, 3), 1, vec![Slot::Task(2), Slot::Task(3)]),
                chain(3, needs(3, 4), 2, vec![Slot::Task(3), Slot::Task(4)]),
            ],
        };
        let positions = position(&ordered, |_| 4);
        let top = positions.centre(Slot::Task(1));
        assert_eq!(positions.centre(dummy(1)), top);
        assert_eq!(positions.centre(dummy(2)), top);
    }

    /// Pairs `(dummy, slot)` where the dummy sits on the centre of a slot in an adjacent
    /// row that it does not connect to while an edge of that slot crosses the channel
    /// between them, so the two verticals would join into one line.
    fn dummies_on_foreign_columns(ordered: &Ordered, positions: &Positions) -> Vec<(Slot, Slot)> {
        let mut links: HashMap<Slot, Vec<Slot>> = HashMap::new();
        for chain in &ordered.chains {
            for pair in chain.slots.windows(2) {
                links.entry(pair[0]).or_default().push(pair[1]);
                links.entry(pair[1]).or_default().push(pair[0]);
            }
        }
        let row_of: HashMap<Slot, usize> = ordered
            .rows
            .iter()
            .enumerate()
            .flat_map(|(r, row)| row.iter().map(move |&slot| (slot, r)))
            .collect();
        let mut found = Vec::new();
        for (r, row) in ordered.rows.iter().enumerate() {
            for &dummy in row.iter().filter(|s| matches!(s, Slot::Dummy { .. })) {
                let adjacent = [r.checked_sub(1), Some(r + 1)];
                for other in adjacent.into_iter().flatten() {
                    for &slot in ordered.rows.get(other).into_iter().flatten() {
                        let crosses = links
                            .get(&slot)
                            .is_some_and(|ends| ends.iter().any(|end| row_of[end] == r));
                        if crosses
                            && !links[&dummy].contains(&slot)
                            && positions.centre(slot) == positions.centre(dummy)
                        {
                            found.push((dummy, slot));
                        }
                    }
                }
            }
        }
        found
    }

    #[test]
    fn a_dummy_keeps_off_the_column_of_another_edges_dummy_in_both_orientations() {
        // 1 -> 3 and 2 -> 6 both pass through dummies; left alone, the one in layer 2
        // lines up under the one in layer 1 and the two edges draw as one straight line.
        let layers = vec![vec![1], vec![2], vec![3, 4], vec![5, 6, 7]];
        let edges = vec![needs(2, 6), needs(1, 3), needs(2, 7), needs(4, 7)];
        let ordered = order(&layers, &edges);
        for (name, positions) in [
            ("vertical", position(&ordered, |_| 3)),
            ("horizontal", position(&ordered, |_| 1)),
        ] {
            assert_eq!(
                dummies_on_foreign_columns(&ordered, &positions),
                vec![],
                "{name}"
            );
        }
    }

    #[test]
    fn a_dummy_keeps_off_the_column_of_an_unrelated_neighbour_in_both_orientations() {
        // 1 -> 8 passes layer 1 in a dummy that lines up over 7, whose own edges arrive
        // from 4 and 5; drawn straight, it reads as 1 -> 7.
        let layers = vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8]];
        let edges = vec![
            needs(1, 4),
            needs(1, 5),
            needs(2, 5),
            needs(4, 7),
            needs(5, 7),
            needs(1, 8),
            needs(6, 8),
            Edge {
                from: 3,
                to: 6,
                kind: EdgeKind::Coupling,
            },
            Edge {
                from: 5,
                to: 8,
                kind: EdgeKind::Coupling,
            },
        ];
        let ordered = order(&layers, &edges);
        assert!(ordered.chains.iter().any(|c| c.slots.len() > 2));
        let vertical = |_| 3;
        let horizontal = |_| 1;
        for (name, positions) in [
            ("vertical", position(&ordered, vertical)),
            ("horizontal", position(&ordered, horizontal)),
        ] {
            assert_eq!(
                dummies_on_foreign_columns(&ordered, &positions),
                vec![],
                "{name}"
            );
        }
    }

    #[test]
    fn output_is_identical_across_runs() {
        let ordered = sample();
        let first = position(&ordered, width);
        for _ in 0..20 {
            assert_eq!(position(&ordered, width), first);
        }
    }
}
