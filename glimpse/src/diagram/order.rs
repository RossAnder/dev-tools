//! Within-layer ordering that reduces edge crossings.
//!
//! Every edge spanning more than one layer becomes a chain with one dummy slot per
//! intermediate layer, so each segment joins adjacent rows. Rows are then reordered by
//! alternating weighted-median sweeps, each followed by adjacent-swap transposition
//! (Gansner et al. 1993), and the ordering with the fewest crossings is kept.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

/// Index of an edge in the slice passed to [`order`].
pub(crate) type EdgeId = usize;

const ITERATIONS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum EdgeKind {
    Needs,
    Coupling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Edge {
    pub(crate) from: u32,
    pub(crate) to: u32,
    pub(crate) kind: EdgeKind,
}

/// One position in a row. The derived `Ord` (every task before every dummy, tasks by id)
/// is the tie-break when two slots have the same median.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Slot {
    Task(u32),
    /// `step` is 1 in the layer just below the chain's upper endpoint.
    Dummy {
        edge: EdgeId,
        step: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chain {
    pub(crate) id: EdgeId,
    pub(crate) edge: Edge,
    /// Layer of `slots[0]`; `slots[i]` sits in layer `top + i`.
    pub(crate) top: usize,
    /// Upper endpoint, dummies, lower endpoint — top-down whichever way the edge points,
    /// so the upper endpoint is `edge.to` when the edge points up.
    pub(crate) slots: Vec<Slot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ordered {
    pub(crate) rows: Vec<Vec<Slot>>,
    /// One chain per edge in input order, omitting edges with an endpoint missing from
    /// the layers or with both endpoints in the same layer.
    pub(crate) chains: Vec<Chain>,
}

impl Ordered {
    /// Pairwise segment crossings summed over every channel between adjacent rows.
    #[cfg(test)]
    pub(crate) fn crossings(&self) -> usize {
        let pos: HashMap<Slot, usize> = self
            .rows
            .iter()
            .flat_map(|row| row.iter().enumerate().map(|(i, &slot)| (slot, i)))
            .collect();
        let mut channels = vec![Vec::new(); self.rows.len().saturating_sub(1)];
        for chain in &self.chains {
            for (k, pair) in chain.slots.windows(2).enumerate() {
                channels[chain.top + k].push((pos[&pair[0]], pos[&pair[1]]));
            }
        }
        channels
            .iter()
            .map(|segments| pairwise_crossings(segments))
            .sum()
    }
}

/// Orders each layer's tasks, plus the dummies of long edges, to reduce crossings.
///
/// A task listed in more than one layer keeps only its first occurrence. The result
/// is a pure function of the input: no hash-order iteration reaches the output.
pub(crate) fn order(layers: &[Vec<u32>], edges: &[Edge]) -> Ordered {
    let mut graph = Graph::build(layers, edges);
    let mut best_rows = graph.rows.clone();
    let mut best = graph.crossings();
    for i in 0..ITERATIONS {
        if best == 0 {
            break;
        }
        graph.sweep(i % 2 == 0);
        graph.transpose();
        let crossings = graph.crossings();
        if crossings < best {
            best = crossings;
            best_rows = graph.rows.clone();
        }
    }
    Ordered {
        rows: best_rows
            .iter()
            .map(|row| row.iter().map(|&node| graph.slots[node]).collect())
            .collect(),
        chains: graph.chains,
    }
}

/// Counts pairs of `(upper position, lower position)` segments that cross. Segments
/// sharing an endpoint never cross.
fn pairwise_crossings(segments: &[(usize, usize)]) -> usize {
    let mut count = 0;
    for (i, &(a1, b1)) in segments.iter().enumerate() {
        for &(a2, b2) in &segments[i + 1..] {
            if (a1 < a2 && b1 > b2) || (a1 > a2 && b1 < b2) {
                count += 1;
            }
        }
    }
    count
}

/// Working graph over node indices; `up`/`down` hold neighbours in the adjacent layers.
#[derive(Default)]
struct Graph {
    slots: Vec<Slot>,
    rows: Vec<Vec<usize>>,
    pos: Vec<usize>,
    up: Vec<Vec<usize>>,
    down: Vec<Vec<usize>>,
    chains: Vec<Chain>,
}

impl Graph {
    fn build(layers: &[Vec<u32>], edges: &[Edge]) -> Self {
        let mut graph = Graph {
            rows: vec![Vec::new(); layers.len()],
            ..Graph::default()
        };
        let mut tasks: HashMap<u32, (usize, usize)> = HashMap::new();
        for (layer, ids) in layers.iter().enumerate() {
            for &id in ids {
                if let Entry::Vacant(entry) = tasks.entry(id) {
                    entry.insert((layer, graph.push(layer, Slot::Task(id))));
                }
            }
        }
        for (id, &edge) in edges.iter().enumerate() {
            let (Some(&from), Some(&to)) = (tasks.get(&edge.from), tasks.get(&edge.to)) else {
                continue;
            };
            if from.0 == to.0 {
                continue;
            }
            let ((top, upper), (bottom, lower)) = if from.0 < to.0 {
                (from, to)
            } else {
                (to, from)
            };
            let mut slots = vec![graph.slots[upper]];
            let mut prev = upper;
            for layer in top + 1..bottom {
                let node = graph.push(
                    layer,
                    Slot::Dummy {
                        edge: id,
                        step: layer - top,
                    },
                );
                graph.link(prev, node);
                slots.push(graph.slots[node]);
                prev = node;
            }
            graph.link(prev, lower);
            slots.push(graph.slots[lower]);
            graph.chains.push(Chain {
                id,
                edge,
                top,
                slots,
            });
        }
        graph
    }

    fn push(&mut self, layer: usize, slot: Slot) -> usize {
        let node = self.slots.len();
        self.slots.push(slot);
        self.pos.push(self.rows[layer].len());
        self.rows[layer].push(node);
        self.up.push(Vec::new());
        self.down.push(Vec::new());
        node
    }

    fn link(&mut self, upper: usize, lower: usize) {
        self.down[upper].push(lower);
        self.up[lower].push(upper);
    }

    fn crossings(&self) -> usize {
        (0..self.rows.len().saturating_sub(1))
            .map(|r| {
                let segments: Vec<(usize, usize)> = self.rows[r]
                    .iter()
                    .flat_map(|&u| {
                        self.down[u]
                            .iter()
                            .map(move |&v| (self.pos[u], self.pos[v]))
                    })
                    .collect();
                pairwise_crossings(&segments)
            })
            .sum()
    }

    /// A downward sweep orders each row by its neighbours in the row above; an upward
    /// sweep by the row below.
    fn sweep(&mut self, downward: bool) {
        let count = self.rows.len();
        if downward {
            for r in 1..count {
                self.reorder(r, true);
            }
        } else {
            for r in (0..count.saturating_sub(1)).rev() {
                self.reorder(r, false);
            }
        }
    }

    /// Sorts the row by median, holding nodes with no neighbour on the reference side
    /// at their current positions.
    fn reorder(&mut self, r: usize, downward: bool) {
        let row = &self.rows[r];
        let mut fixed = vec![false; row.len()];
        let mut movable: Vec<(f64, Slot, usize)> = Vec::new();
        for (i, &node) in row.iter().enumerate() {
            let neighbours = if downward {
                &self.up[node]
            } else {
                &self.down[node]
            };
            match self.median(neighbours) {
                Some(median) => movable.push((median, self.slots[node], node)),
                None => fixed[i] = true,
            }
        }
        movable.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut movable = movable.into_iter().map(|(_, _, node)| node);
        let reordered: Vec<usize> = row
            .iter()
            .zip(&fixed)
            .filter_map(|(&node, &fixed)| if fixed { Some(node) } else { movable.next() })
            .collect();
        for (i, &node) in reordered.iter().enumerate() {
            self.pos[node] = i;
        }
        self.rows[r] = reordered;
    }

    /// Gansner's weighted median: an even count interpolates towards the side whose
    /// neighbours are packed more tightly.
    fn median(&self, neighbours: &[usize]) -> Option<f64> {
        let mut p: Vec<f64> = neighbours.iter().map(|&n| self.pos[n] as f64).collect();
        p.sort_by(f64::total_cmp);
        let len = p.len();
        let m = len / 2;
        match len {
            0 => None,
            _ if len % 2 == 1 => Some(p[m]),
            2 => Some((p[0] + p[1]) / 2.0),
            _ => {
                let left = p[m - 1] - p[0];
                let right = p[len - 1] - p[m];
                if left + right == 0.0 {
                    Some((p[m - 1] + p[m]) / 2.0)
                } else {
                    Some((p[m - 1] * right + p[m] * left) / (left + right))
                }
            }
        }
    }

    /// Swaps adjacent pairs while a swap strictly lowers the local crossing count, so
    /// the loop terminates.
    fn transpose(&mut self) {
        loop {
            let mut improved = false;
            for r in 0..self.rows.len() {
                for i in 0..self.rows[r].len().saturating_sub(1) {
                    let (v, w) = (self.rows[r][i], self.rows[r][i + 1]);
                    if self.pair_crossings(w, v) < self.pair_crossings(v, w) {
                        self.rows[r].swap(i, i + 1);
                        self.pos[v] = i + 1;
                        self.pos[w] = i;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }

    /// Crossings between the segments of `left` and `right` when `left` sits
    /// immediately left of `right`.
    fn pair_crossings(&self, left: usize, right: usize) -> usize {
        let count = |a: &[usize], b: &[usize]| {
            a.iter()
                .map(|&x| b.iter().filter(|&&y| self.pos[x] > self.pos[y]).count())
                .sum::<usize>()
        };
        count(&self.up[left], &self.up[right]) + count(&self.down[left], &self.down[right])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needs(from: u32, to: u32) -> Edge {
        Edge {
            from,
            to,
            kind: EdgeKind::Needs,
        }
    }

    fn dummies(ordered: &Ordered) -> usize {
        ordered
            .rows
            .iter()
            .flatten()
            .filter(|slot| matches!(slot, Slot::Dummy { .. }))
            .count()
    }

    fn sample() -> (Vec<Vec<u32>>, Vec<Edge>) {
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
        (layers, edges)
    }

    #[test]
    fn two_layer_crossings_are_removed() {
        let layers = vec![vec![1, 2], vec![3, 4, 5]];
        let edges = vec![needs(1, 5), needs(2, 3), needs(2, 4)];
        assert_eq!(Graph::build(&layers, &edges).crossings(), 2);
        let ordered = order(&layers, &edges);
        assert_eq!(ordered.crossings(), 0);
    }

    #[test]
    fn span_three_edge_gets_two_dummies() {
        let layers = vec![vec![1], vec![2], vec![3], vec![4]];
        let ordered = order(&layers, &[needs(1, 2), needs(1, 4)]);
        assert_eq!(dummies(&ordered), 2);
        let chain = &ordered.chains[1];
        assert_eq!(
            chain.slots,
            vec![
                Slot::Task(1),
                Slot::Dummy { edge: 1, step: 1 },
                Slot::Dummy { edge: 1, step: 2 },
                Slot::Task(4),
            ]
        );
        assert!(ordered.rows[1].contains(&Slot::Dummy { edge: 1, step: 1 }));
        assert!(ordered.rows[2].contains(&Slot::Dummy { edge: 1, step: 2 }));
    }

    #[test]
    fn upward_and_unplaceable_edges() {
        let layers = vec![vec![1, 2], vec![3], vec![4]];
        let edges = vec![needs(4, 1), needs(1, 2), needs(1, 99)];
        let ordered = order(&layers, &edges);
        assert_eq!(ordered.chains.len(), 1);
        let chain = &ordered.chains[0];
        assert_eq!((chain.id, chain.top), (0, 0));
        assert_eq!(chain.slots.first(), Some(&Slot::Task(1)));
        assert_eq!(chain.slots.last(), Some(&Slot::Task(4)));
    }

    #[test]
    fn output_is_identical_across_runs() {
        let (layers, edges) = sample();
        let first = order(&layers, &edges);
        for _ in 0..20 {
            assert_eq!(order(&layers, &edges), first);
        }
    }

    #[test]
    fn layer_membership_is_preserved() {
        let (layers, edges) = sample();
        let ordered = order(&layers, &edges);
        assert_eq!(ordered.rows.len(), layers.len());
        for (row, layer) in ordered.rows.iter().zip(&layers) {
            let mut tasks: Vec<u32> = row
                .iter()
                .filter_map(|slot| match slot {
                    Slot::Task(id) => Some(*id),
                    Slot::Dummy { .. } => None,
                })
                .collect();
            tasks.sort_unstable();
            let mut expected = layer.clone();
            expected.sort_unstable();
            assert_eq!(tasks, expected);
        }
        assert_eq!(dummies(&ordered), 4);
    }

    #[test]
    fn sample_improves_on_input_order() {
        let (layers, edges) = sample();
        let initial = Graph::build(&layers, &edges).crossings();
        assert!(order(&layers, &edges).crossings() < initial);
    }
}
