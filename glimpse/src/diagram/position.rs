//! Cross-axis coordinate assignment for ordered layers.
//!
//! Sugiyama's priority method (Sugiyama, Tagawa and Toda 1981): alternating sweeps
//! move each slot towards the barycentre of its neighbours in the reference row,
//! highest priority first. A slot may push lower-priority slots aside but never one
//! already placed in the same pass. Dummies outrank every task, so a long edge stays
//! straight wherever nothing placed before it blocks the way. Equal priorities are
//! placed from the middle of the row outwards, so a parent settles over its middle
//! child rather than its first.

use std::cmp::Reverse;
use std::collections::HashMap;

use super::order::{Ordered, Slot};

/// Down, up, down, up, down: the last pass aligns each chain with its upper endpoint.
const PASSES: usize = 5;
/// Blank cells between two slots when either is a task.
const TASK_GAP: i32 = 2;
/// Blank cells between two adjacent dummies.
const DUMMY_GAP: i32 = 1;

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
    let seps: Vec<i32> = (0..len.saturating_sub(1))
        .map(|i| widths[i] + gap(slots[i], slots[i + 1]))
        .collect();
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
            for j in v + 1..len {
                let min = xs[j - 1] + seps[j - 1];
                if xs[j] >= min {
                    break;
                }
                xs[j] = min;
            }
        } else if target < xs[v] {
            let limit = (0..v)
                .rev()
                .find(|&k| locked[k])
                .map(|k| xs[k] + seps[k..v].iter().sum::<i32>());
            xs[v] = limit.map_or(target, |limit| target.max(limit));
            for j in (0..v).rev() {
                let max = xs[j + 1] - seps[j];
                if xs[j] <= max {
                    break;
                }
                xs[j] = max;
            }
        }
        locked[v] = true;
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

    #[test]
    fn output_is_identical_across_runs() {
        let ordered = sample();
        let first = position(&ordered, width);
        for _ in 0..20 {
            assert_eq!(position(&ordered, width), first);
        }
    }
}
