//! Cross-axis coordinate assignment for ordered layers.
//!
//! Four stages. Sugiyama's priority method (Sugiyama, Tagawa and Toda 1981) sweeps each
//! slot towards the barycentre of its neighbours, highest priority first, dummies above
//! tasks so long edges come out straight. The sweeps only ever pull, so one far
//! neighbour leaves the subtree below it offset and an empty band beside it; [`realign`]
//! keeps the straight runs they chose as rigid blocks, packs the blocks tight and lets
//! each settle at the weighted median of its neighbours (Gansner et al. 1993 weights).
//! [`separate_dummies`] then moves any dummy off a column where it would join an
//! unrelated edge's vertical. Last, [`compact`] closes remaining bands with cut shifts,
//! which move everything past a cross-axis cut as one piece: rows keep their order,
//! straight segments stay straight, and a shift that lengthens the edges crossing the
//! cut or joins two unrelated verticals is refused. Brandes–Köpf alignment compacts as
//! well, but would re-derive the straight runs the sweeps already chose. [`pack`] sets
//! separately placed components side by side.

use std::cmp::Reverse;
use std::collections::HashMap;

use super::order::{Chain, Ordered, Slot};

/// Down, up, down, up, down: the last pass aligns each chain with its upper endpoint.
const PASSES: usize = 5;
/// Blank cells between two slots when either is a task.
const TASK_GAP: i32 = 2;
/// Blank cells between two adjacent dummies.
const DUMMY_GAP: i32 = 1;
/// Cap on the rounds that move dummies off unrelated columns, since each push can
/// uncover a new clash in the rows next to it.
const SEPARATE_ROUNDS: usize = 8;
/// Cap on full cut sweeps; a shift can open room at a cut already passed.
const COMPACT_ROUNDS: usize = 4;
/// Cap on the median rounds after block packing; alternate rounds run in opposite
/// directions and a round that moves nothing ends them early.
const RELAX_ROUNDS: usize = 16;
/// Blank cells between two packed components.
const COMPONENT_GAP: i32 = 3;

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
    place_all(ordered, label_width, true)
}

fn place_all(ordered: &Ordered, label_width: impl Fn(u32) -> u16, compacted: bool) -> Positions {
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
    if compacted {
        realign(&mut xs, &widths, rows, &up, &down);
    }
    separate_dummies(&mut xs, &widths, rows, &up, &down);
    if compacted {
        compact(&mut xs, &widths, rows, &up, &down);
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

/// Closes empty cross-axis bands by cut shifts, sweeping each gap between two distinct
/// slot centres left to right. At a cut, every slot centred at or past it moves left by
/// one amount, bounded by the slack of each row's pair straddling the cut and by the
/// gap itself, so centres never reorder. [`best_shift`] picks the amount.
fn compact(
    xs: &mut [Vec<i32>],
    widths: &[Vec<i32>],
    rows: &[Vec<Slot>],
    up: &[Vec<Vec<usize>>],
    down: &[Vec<Vec<usize>>],
) {
    let mut centres = centres_of(xs, widths);
    let seps = centre_separations(widths, rows);
    for _ in 0..COMPACT_ROUNDS {
        let mut values: Vec<i32> = centres.iter().flatten().copied().collect();
        values.sort_unstable();
        values.dedup();
        let mut moved = false;
        for k in 1..values.len() {
            let cut = values[k];
            let mut limit = cut - values[k - 1] - 1;
            for (row, seps) in centres.iter().zip(&seps) {
                if let Some(i) = (0..seps.len()).find(|&i| row[i] < cut && row[i + 1] >= cut) {
                    limit = limit.min(row[i + 1] - row[i] - seps[i]);
                }
            }
            if limit <= 0 {
                continue;
            }
            let shift = best_shift(&centres, rows, up, down, cut, limit);
            if shift == 0 {
                continue;
            }
            for c in centres.iter_mut().flatten().filter(|c| **c >= cut) {
                *c -= shift;
            }
            for v in &mut values[k..] {
                *v -= shift;
            }
            moved = true;
        }
        if !moved {
            break;
        }
    }
    set_centres(xs, widths, &centres);
}

fn centres_of(xs: &[Vec<i32>], widths: &[Vec<i32>]) -> Vec<Vec<i32>> {
    xs.iter()
        .zip(widths)
        .map(|(xs, ws)| xs.iter().zip(ws).map(|(&x, &w)| x + w / 2).collect())
        .collect()
}

fn set_centres(xs: &mut [Vec<i32>], widths: &[Vec<i32>], centres: &[Vec<i32>]) {
    for ((xs, centres), ws) in xs.iter_mut().zip(centres).zip(widths) {
        for ((x, &c), &w) in xs.iter_mut().zip(centres).zip(ws) {
            *x = c - w / 2;
        }
    }
}

/// `seps[r][i]` is the least distance from slot `i`'s centre to slot `i + 1`'s.
fn centre_separations(widths: &[Vec<i32>], rows: &[Vec<Slot>]) -> Vec<Vec<i32>> {
    rows.iter()
        .zip(widths)
        .map(|(row, ws)| {
            (0..row.len().saturating_sub(1))
                .map(|i| ws[i] - ws[i] / 2 + ws[i + 1] / 2 + gap(row[i], row[i + 1]))
                .collect()
        })
        .collect()
}

/// Makes each straight run of a long edge, or of tasks joined one-to-one, a rigid block
/// and every other slot a block of its own, packs the blocks as far towards the leading
/// edge as their rows allow, then moves each to the weighted median of the slots it
/// joins, within its rows' slack and the packed extent. The packing removes the slack
/// the sweeps left; the medians put back only what the edges ask for, and one far
/// neighbour drags a median less than a barycentre. A lone task takes the midpoint of
/// its two medians, so a parent stays centred over two children, while a block moves
/// no further than it must. No move lands a vertical on an unrelated one.
fn realign(
    xs: &mut [Vec<i32>],
    widths: &[Vec<i32>],
    rows: &[Vec<Slot>],
    up: &[Vec<Vec<usize>>],
    down: &[Vec<Vec<usize>>],
) {
    let centres = centres_of(xs, widths);
    let seps = centre_separations(widths, rows);
    let start: Vec<usize> = rows
        .iter()
        .scan(0, |next, row| {
            let at = *next;
            *next += row.len();
            Some(at)
        })
        .collect();
    let total = start.last().map_or(0, |&s| s + rows[rows.len() - 1].len());
    let mut parent: Vec<usize> = (0..total).collect();
    fn root(parent: &mut [usize], mut v: usize) -> usize {
        while parent[v] != v {
            parent[v] = parent[parent[v]];
            v = parent[v];
        }
        v
    }
    for r in 0..rows.len().saturating_sub(1) {
        for (i, lower) in down[r].iter().enumerate() {
            for &j in lower {
                let dummy = [rows[r][i], rows[r + 1][j]]
                    .iter()
                    .any(|slot| matches!(slot, Slot::Dummy { .. }));
                let link = lower.len() == 1 && up[r + 1][j].len() == 1;
                if (dummy || link) && centres[r][i] == centres[r + 1][j] {
                    let (a, b) = (
                        root(&mut parent, start[r] + i),
                        root(&mut parent, start[r + 1] + j),
                    );
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut block_of: Vec<Vec<usize>> = rows.iter().map(|row| vec![0; row.len()]).collect();
    let mut ids: HashMap<usize, usize> = HashMap::new();
    let mut members: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut pos: Vec<i32> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for i in 0..row.len() {
            let key = root(&mut parent, start[r] + i);
            let b = *ids.entry(key).or_insert_with(|| {
                members.push(Vec::new());
                pos.push(centres[r][i]);
                members.len() - 1
            });
            block_of[r][i] = b;
            members[b].push((r, i));
        }
    }

    // Every left-of relation between blocks runs to a larger centre, so ascending
    // centre order is a topological order of the packing constraints.
    let mut by_centre: Vec<usize> = (0..members.len()).collect();
    by_centre.sort_by_key(|&b| (pos[b], b));
    let bounds = |pos: &[i32], b: usize| {
        let mut lo = i32::MIN;
        let mut hi = i32::MAX;
        for &(r, i) in &members[b] {
            if i > 0 {
                lo = lo.max(pos[block_of[r][i - 1]] + seps[r][i - 1]);
            }
            if i + 1 < rows[r].len() {
                hi = hi.min(pos[block_of[r][i + 1]] - seps[r][i]);
            }
        }
        (lo, hi)
    };
    for &b in &by_centre {
        pos[b] = bounds(&pos, b).0.max(0);
    }
    let extent = pos.iter().copied().max().unwrap_or(0);

    for round in 0..RELAX_ROUNDS {
        let mut moved = false;
        let sweep: Vec<usize> = if round % 2 == 0 {
            by_centre.clone()
        } else {
            by_centre.iter().rev().copied().collect()
        };
        for b in sweep {
            let mut pulls: Vec<(i32, i64)> = Vec::new();
            let mut forbidden: Vec<i32> = Vec::new();
            for &(r, i) in &members[b] {
                let slot = rows[r][i];
                let sides = [
                    r.checked_sub(1).map(|a| (a, &up[r][i], down)),
                    (r + 1 < rows.len()).then(|| (r + 1, &down[r][i], up)),
                ];
                for (a, own, facing) in sides.into_iter().flatten() {
                    for &j in own {
                        if block_of[a][j] != b {
                            pulls.push((pos[block_of[a][j]], weight(slot, rows[a][j])));
                        }
                    }
                    if own.is_empty() {
                        continue;
                    }
                    for (j, edges) in facing[a].iter().enumerate() {
                        if !edges.is_empty() && !own.contains(&j) {
                            forbidden.push(pos[block_of[a][j]]);
                        }
                    }
                }
            }
            let Some((low, high)) = weighted_median(&mut pulls) else {
                continue;
            };
            let here = pos[b];
            let lone_task = members[b].len() == 1
                && matches!(rows[members[b][0].0][members[b][0].1], Slot::Task(_));
            let wanted = if lone_task {
                (low + high).div_euclid(2)
            } else {
                here.clamp(low, high)
            };
            let (lo, hi) = bounds(&pos, b);
            let mut to = wanted.clamp(lo.max(0), hi.min(extent).max(lo.max(0)));
            while to != here && forbidden.contains(&to) {
                to += (here - to).signum();
            }
            if to != here {
                pos[b] = to;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }

    let mut centres = centres;
    for (b, list) in members.iter().enumerate() {
        for &(r, i) in list {
            centres[r][i] = pos[b];
        }
    }
    set_centres(xs, widths, &centres);
}

/// The lower and upper weighted medians of `pulls`, or `None` when it is empty.
fn weighted_median(pulls: &mut [(i32, i64)]) -> Option<(i32, i32)> {
    pulls.sort_unstable();
    let total: i64 = pulls.iter().map(|&(_, w)| w).sum();
    let mut seen = 0;
    let mut low = None;
    for &(c, w) in pulls.iter() {
        seen += w;
        if low.is_none() && 2 * seen >= total {
            low = Some(c);
        }
        if 2 * seen > total {
            return Some((low.unwrap_or(c), c));
        }
    }
    None
}

/// The shift in `1..=limit` for the slots centred at or past `cut`, or 0. The segments
/// crossing the cut are the only ones it changes: it takes the least total weighted
/// length they can reach, the furthest such shift on a tie, and never one longer than
/// they are now. A shift that would centre a vertical leaving one slot on an unrelated
/// vertical entering the same channel from the other side is refused.
fn best_shift(
    centres: &[Vec<i32>],
    rows: &[Vec<Slot>],
    up: &[Vec<Vec<usize>>],
    down: &[Vec<Vec<usize>>],
    cut: i32,
    limit: i32,
) -> i32 {
    let right = |c: i32| c >= cut;
    let mut spans: Vec<(i32, i64)> = Vec::new();
    let mut forbidden: Vec<i32> = Vec::new();
    for r in 0..rows.len().saturating_sub(1) {
        for (i, lower) in down[r].iter().enumerate() {
            let a = centres[r][i];
            for &j in lower {
                let b = centres[r + 1][j];
                if right(a) != right(b) {
                    spans.push(((a - b).abs(), weight(rows[r][i], rows[r + 1][j])));
                }
            }
            if lower.is_empty() {
                continue;
            }
            for (j, upper) in up[r + 1].iter().enumerate() {
                let b = centres[r + 1][j];
                if !upper.is_empty() && !lower.contains(&j) && right(a) != right(b) {
                    forbidden.push((a - b).abs());
                }
            }
        }
    }
    let cost = |shift: i32| -> i64 {
        spans
            .iter()
            .map(|&(d, w)| w * i64::from((d - shift).abs()))
            .sum()
    };
    let near = |v: i32| [v - 1, v, v + 1];
    let candidates = std::iter::once(limit)
        .chain(spans.iter().flat_map(|&(d, _)| near(d)))
        .chain(forbidden.iter().flat_map(|&f| near(f)))
        .filter(|&s| (1..=limit).contains(&s) && !forbidden.contains(&s));
    let baseline = cost(0);
    candidates
        .map(|s| (cost(s), Reverse(s)))
        .min()
        .filter(|&(c, _)| c <= baseline)
        .map_or(0, |(_, Reverse(s))| s)
}

/// Gansner et al.'s weights: a bend costs most between two dummies, so a long edge is
/// the last thing a shift bends.
fn weight(a: Slot, b: Slot) -> i64 {
    match (a, b) {
        (Slot::Dummy { .. }, Slot::Dummy { .. }) => 8,
        (Slot::Task(_), Slot::Task(_)) => 1,
        _ => 2,
    }
}

/// Lays separately positioned components side by side along the slot axis, in the
/// given order. Each goes as far towards the leading edge as it can while staying
/// [`COMPONENT_GAP`] cells clear of everything placed before it in every row and every
/// channel, so a small component tucks in beside a narrow stretch of a large one but
/// never between its slots. All parts must have the same row count.
pub(crate) fn pack(parts: Vec<(Ordered, Positions)>) -> (Ordered, Positions) {
    let count = parts.first().map_or(0, |(ordered, _)| ordered.rows.len());
    let mut rows: Vec<Vec<Slot>> = vec![Vec::new(); count];
    let mut chains: Vec<Chain> = Vec::new();
    let mut packed = Positions {
        x: HashMap::new(),
        width: HashMap::new(),
        layer_extent: 0,
    };
    let levels = 2 * count;
    // Trailing edge of what is placed, per row and per channel.
    let mut contour: Vec<Option<i32>> = vec![None; levels];
    for (ordered, positions) in parts {
        let hull: Vec<Option<(i32, i32)>> = ordered
            .rows
            .iter()
            .map(|row| {
                row.iter().fold(None, |acc: Option<(i32, i32)>, slot| {
                    let x = i32::from(positions.x[slot]);
                    let end = x + i32::from(positions.width[slot]);
                    Some(acc.map_or((x, end), |(lo, hi)| (lo.min(x), hi.max(end))))
                })
            })
            .collect();
        // Level `2r` is row `r`; level `2r + 1` the channel after it, whose lines stay
        // within the two rows' hulls and exist only when both rows hold slots.
        let reach: Vec<Option<(i32, i32)>> = (0..levels)
            .map(|level| {
                let r = level / 2;
                if level % 2 == 0 {
                    return hull[r];
                }
                let ((a, b), (c, d)) = (hull[r]?, (*hull.get(r + 1)?)?);
                Some((a.min(c), b.max(d)))
            })
            .collect();
        let offset = (0..levels)
            .filter_map(|l| Some(contour[l]? + COMPONENT_GAP - reach[l]?.0))
            .max()
            .unwrap_or(0)
            .max(0);
        for (edge, reach) in contour.iter_mut().zip(&reach) {
            if let Some((_, hi)) = reach {
                *edge = Some(edge.map_or(hi + offset, |e| e.max(hi + offset)));
            }
        }
        let shift = u16::try_from(offset).unwrap_or(u16::MAX);
        for (row, slots) in rows.iter_mut().zip(&ordered.rows) {
            row.extend(slots);
        }
        for (&slot, &x) in &positions.x {
            let width = positions.width[&slot];
            let x = x.saturating_add(shift);
            packed.x.insert(slot, x);
            packed.width.insert(slot, width);
            packed.layer_extent = packed.layer_extent.max(x.saturating_add(width));
        }
        chains.extend(ordered.chains);
    }
    (Ordered { rows, chains }, packed)
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

    /// A second root feeding the middle of a chain drags every task below it towards
    /// itself, and the fan at the bottom hangs past everything above it.
    fn dragged_fan() -> Ordered {
        let layers = vec![
            vec![1, 2],
            vec![3, 4, 5, 6],
            vec![7],
            vec![8],
            vec![9, 10, 11, 12],
        ];
        let mut edges: Vec<Edge> = [3, 4, 5, 6].map(|to| needs(1, to)).to_vec();
        edges.extend([needs(6, 7), needs(2, 7), needs(7, 8)]);
        edges.extend([9, 10, 11, 12].map(|to| needs(8, to)));
        order(&layers, &edges)
    }

    #[test]
    fn compaction_closes_the_slack_a_dragged_subtree_leaves() {
        let ordered = dragged_fan();
        for (name, cross) in [("vertical", 3), ("horizontal", 1)] {
            let loose = place_all(&ordered, |_| cross, false);
            let tight = position(&ordered, |_| cross);
            assert!(
                tight.layer_extent * 4 <= loose.layer_extent * 3,
                "{name}: {} is not a quarter under {}",
                tight.layer_extent,
                loose.layer_extent
            );
            for row in gaps(&ordered, &tight) {
                for (left, right, blank) in row {
                    assert!(blank >= 1, "{name}: {left:?} {right:?} gap {blank}");
                }
            }
            assert_eq!(
                dummies_on_foreign_columns(&ordered, &tight),
                vec![],
                "{name}"
            );
        }
    }

    #[test]
    fn a_parent_of_two_stays_centred_after_compaction() {
        let ordered = order(&[vec![1], vec![2, 3]], &[needs(1, 2), needs(1, 3)]);
        let positions = position(&ordered, |_| 3);
        let (a, b) = (
            positions.centre(Slot::Task(2)).unwrap(),
            positions.centre(Slot::Task(3)).unwrap(),
        );
        assert_eq!(positions.centre(Slot::Task(1)), Some((a + b) / 2));
        assert_eq!(b - a, 5, "the children pack at pitch");
    }

    #[test]
    fn packed_components_keep_clear_of_each_other() {
        // A tall chain with a wide foot, and a pair beside its narrow top.
        let big = order(
            &[vec![1], vec![2], vec![3, 4, 5, 6], vec![]],
            &[
                needs(1, 2),
                needs(2, 3),
                needs(2, 4),
                needs(2, 5),
                needs(2, 6),
            ],
        );
        let small = order(&[vec![7], vec![8], vec![], vec![]], &[needs(7, 8)]);
        let lone = order(&[vec![], vec![], vec![], vec![9]], &[]);
        let parts: Vec<(Ordered, Positions)> = [big, small, lone]
            .into_iter()
            .map(|ordered| {
                let positions = position(&ordered, |_| 3);
                (ordered, positions)
            })
            .collect();
        let (ordered, positions) = pack(parts);
        let x = |id| positions.x[&Slot::Task(id)];
        assert_eq!(ordered.rows[0], vec![Slot::Task(1), Slot::Task(7)]);
        assert_eq!(ordered.chains.len(), 6);
        // The pair clears the chain's top and the channel under it, but not the foot.
        assert_eq!(x(7), x(1) + 3 + 3);
        assert!(x(8) < x(6));
        // The lone task shares no row or channel with anything, so it starts the row.
        assert_eq!(x(9), 0);
        let extent = ordered
            .rows
            .iter()
            .flatten()
            .map(|slot| positions.x[slot] + positions.width[slot])
            .max();
        assert_eq!(Some(positions.layer_extent), extent);
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
