//! The task DAG engine — Kahn rounds, cycles, closures, frontier, file overlap and checkpoint groups.
//!
//! Self-contained over its own `Node`, so the schema layer converts rows in and
//! every id handed back is a task id, never an internal position.
//!
//! In-degree is `needs ∪ coupling`; a shared file is advisory and never an
//! edge. Ascending task id breaks every tie — seeds, rounds, successor
//! decrements, pair lists — so two runs over one store agree byte for byte.
//!
//! Reachability is one fixed-width bit row per node, OR-folded along the
//! topological order, which is where the node cap comes from.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};

/// Reachability is `[u64; WORDS]` per node; widening the cap costs a word per
/// 64 tasks and nothing else.
const MAX_NODES: usize = 256;
const WORDS: usize = MAX_NODES / 64;

type Bits = [u64; WORDS];

const STATUS_PENDING: &str = "pending";
const STATUS_DONE: &str = "done";

/// One task as the engine sees it. `status` and `checkpoint` are the raw store
/// spellings; the engine only ever compares them.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) id: u32,
    pub(crate) files: Vec<String>,
    pub(crate) needs: Vec<u32>,
    pub(crate) coupling: Vec<u32>,
    pub(crate) status: String,
    pub(crate) checkpoint: String,
}

/// A ready task withheld because an in-flight task already claims one of its
/// files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Held {
    pub(crate) id: u32,
    pub(crate) blocked_on_file: String,
    pub(crate) holder: u32,
}

/// `ready` excludes everything in `held` — it is the dispatchable set, not the
/// unblocked set. `next` is the wave behind both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frontier {
    pub(crate) ready: Vec<u32>,
    pub(crate) held: Vec<Held>,
    pub(crate) next: Vec<u32>,
}

/// `maximal` is the group's antichain under reachability — the ids a
/// `CHECKPOINT … after` marker names. `valid_cut` covers this group unioned
/// with every earlier one, so an invalid prefix can be repaired by a later
/// group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Group {
    pub(crate) id: String,
    pub(crate) members: Vec<u32>,
    pub(crate) maximal: Vec<u32>,
    pub(crate) valid_cut: bool,
}

#[derive(Debug)]
struct Reach {
    up: Vec<Bits>,
    down: Vec<Bits>,
}

#[derive(Debug)]
pub(crate) struct Graph<'a> {
    /// Sorted ascending by id, so a position and its id sort alike and every
    /// ascending walk over positions is an ascending walk over ids.
    nodes: Vec<&'a Node>,
    pos: BTreeMap<u32, usize>,
    preds: Vec<Vec<usize>>,
    succs: Vec<Vec<usize>>,
    rounds: Vec<Vec<usize>>,
    residual: Vec<usize>,
    /// `None` when Kahn left a residue: reachability is undefined through a
    /// cycle, and a partial answer would be worse than an error.
    reach: Option<Reach>,
}

impl<'a> Graph<'a> {
    /// Rejects a duplicate id and an edge naming an absent task — both leave
    /// the graph ambiguous, and `check` reports each from the store before it
    /// reaches here. A cycle is not rejected: `cycle_members` is how callers
    /// diagnose one.
    pub(crate) fn build(nodes: &'a [Node]) -> Result<Self> {
        if nodes.len() > MAX_NODES {
            bail!("graph exceeds {MAX_NODES} tasks");
        }
        let mut ordered: Vec<&Node> = nodes.iter().collect();
        ordered.sort_by_key(|n| n.id);

        let mut pos = BTreeMap::new();
        for (p, node) in ordered.iter().enumerate() {
            if pos.insert(node.id, p).is_some() {
                bail!("duplicate task id {}", node.id);
            }
        }

        let n = ordered.len();
        let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (p, node) in ordered.iter().enumerate() {
            let mut seen = BTreeSet::new();
            for dep in node.needs.iter().chain(node.coupling.iter()) {
                let d = *pos
                    .get(dep)
                    .ok_or_else(|| anyhow!("task {} depends on absent task {dep}", node.id))?;
                if seen.insert(d) {
                    preds[p].push(d);
                    succs[d].push(p);
                }
            }
            preds[p].sort_unstable();
        }

        let (rounds, residual) = layered_kahn(n, &preds, &succs);
        let reach = residual
            .is_empty()
            .then(|| reachability(n, &preds, &succs, &rounds));

        Ok(Self {
            nodes: ordered,
            pos,
            preds,
            succs,
            rounds,
            residual,
            reach,
        })
    }

    /// Layers in dependency order, each sorted ascending. Empty for a graph
    /// that is entirely a cycle.
    pub(crate) fn kahn_rounds(&self) -> Vec<Vec<u32>> {
        self.rounds
            .iter()
            .map(|round| round.iter().map(|p| self.nodes[*p].id).collect())
            .collect()
    }

    /// One simple cycle, sorted ascending — not the whole residue, which also
    /// holds the tasks stranded downstream of it. Empty when acyclic.
    pub(crate) fn cycle_members(&self) -> Vec<u32> {
        if self.residual.is_empty() {
            return Vec::new();
        }
        let n = self.nodes.len();
        let mut stranded = vec![false; n];
        for p in &self.residual {
            stranded[*p] = true;
        }
        let mut mark = vec![Mark::White; n];

        for start in &self.residual {
            if mark[*start] != Mark::White {
                continue;
            }
            mark[*start] = Mark::Grey;
            let mut stack: Vec<(usize, usize)> = vec![(*start, 0)];
            while let Some(&(p, i)) = stack.last() {
                if i >= self.succs[p].len() {
                    mark[p] = Mark::Black;
                    stack.pop();
                    continue;
                }
                stack.last_mut().expect("frame present").1 = i + 1;
                let s = self.succs[p][i];
                if !stranded[s] {
                    continue;
                }
                match mark[s] {
                    Mark::White => {
                        mark[s] = Mark::Grey;
                        stack.push((s, 0));
                    }
                    Mark::Grey => {
                        let at = stack
                            .iter()
                            .position(|(q, _)| *q == s)
                            .expect("grey node is on the stack");
                        let mut members: Vec<u32> =
                            stack[at..].iter().map(|(q, _)| self.nodes[*q].id).collect();
                        members.sort_unstable();
                        return members;
                    }
                    Mark::Black => {}
                }
            }
        }
        Vec::new()
    }

    /// Transitive dependencies of `id`, inclusive of `id`.
    pub(crate) fn closure_up(&self, id: u32) -> Result<Vec<u32>> {
        let p = self.position(id)?;
        Ok(self.ids(&self.reach()?.up[p]))
    }

    /// Transitive dependents of `id`, inclusive of `id`.
    pub(crate) fn closure_down(&self, id: u32) -> Result<Vec<u32>> {
        let p = self.position(id)?;
        Ok(self.ids(&self.reach()?.down[p]))
    }

    pub(crate) fn frontier(&self, in_flight: &[u32]) -> Result<Frontier> {
        let n = self.nodes.len();
        let mut claimed = vec![false; n];
        let mut holders: Vec<usize> = Vec::new();
        for id in in_flight {
            let p = self.position(*id)?;
            if !claimed[p] {
                claimed[p] = true;
                holders.push(p);
            }
        }
        holders.sort_unstable();

        let mut ready = Vec::new();
        let mut held = Vec::new();
        let mut wave = claimed.clone();
        for p in 0..n {
            if claimed[p] || !self.is_pending(p) {
                continue;
            }
            if !self.preds[p]
                .iter()
                .all(|d| self.nodes[*d].status == STATUS_DONE)
            {
                continue;
            }
            wave[p] = true;
            match self.first_claim(p, &holders) {
                Some((holder, file)) => held.push(Held {
                    id: self.nodes[p].id,
                    blocked_on_file: file,
                    holder,
                }),
                None => ready.push(self.nodes[p].id),
            }
        }

        let mut next = Vec::new();
        for p in 0..n {
            if wave[p] || !self.is_pending(p) {
                continue;
            }
            let reachable = self.preds[p]
                .iter()
                .filter(|d| self.nodes[**d].status != STATUS_DONE)
                .all(|d| wave[*d]);
            if reachable {
                next.push(self.nodes[p].id);
            }
        }

        Ok(Frontier { ready, held, next })
    }

    /// Pairs sharing a file with no directed path either way — a claim two
    /// agents could take at once.
    pub(crate) fn overlap_pairs(&self) -> Result<Vec<(u32, u32)>> {
        let reach = self.reach()?;
        let files: Vec<BTreeSet<&str>> = self
            .nodes
            .iter()
            .map(|node| node.files.iter().map(String::as_str).collect())
            .collect();

        let mut pairs = Vec::new();
        for a in 0..self.nodes.len() {
            for b in (a + 1)..self.nodes.len() {
                if bits_has(&reach.down[a], b) || bits_has(&reach.down[b], a) {
                    continue;
                }
                if files[a].intersection(&files[b]).next().is_some() {
                    pairs.push((self.nodes[a].id, self.nodes[b].id));
                }
            }
        }
        Ok(pairs)
    }

    /// `order` is the store's checkpoint order; a task whose `checkpoint`
    /// names no entry there belongs to no group and appears in none.
    pub(crate) fn groups(&self, order: &[String]) -> Result<Vec<Group>> {
        let reach = self.reach()?;
        let mut prefix: Bits = [0; WORDS];
        let mut prefix_up: Bits = [0; WORDS];
        let mut groups = Vec::new();

        for id in order {
            let members: Vec<usize> = (0..self.nodes.len())
                .filter(|p| self.nodes[*p].checkpoint == *id)
                .collect();
            for p in &members {
                bits_set(&mut prefix, *p);
                let up = reach.up[*p];
                bits_or(&mut prefix_up, &up);
            }
            let maximal = members
                .iter()
                .filter(|p| {
                    !members
                        .iter()
                        .any(|q| q != *p && bits_has(&reach.down[**p], *q))
                })
                .map(|p| self.nodes[*p].id)
                .collect();
            groups.push(Group {
                id: id.clone(),
                members: members.iter().map(|p| self.nodes[*p].id).collect(),
                maximal,
                valid_cut: bits_subset(&prefix_up, &prefix),
            });
        }
        Ok(groups)
    }

    fn reach(&self) -> Result<&Reach> {
        self.reach.as_ref().ok_or_else(|| {
            let members: Vec<String> = self.cycle_members().iter().map(u32::to_string).collect();
            anyhow!("graph contains a cycle: {}", members.join(", "))
        })
    }

    fn position(&self, id: u32) -> Result<usize> {
        self.pos
            .get(&id)
            .copied()
            .ok_or_else(|| anyhow!("no task {id} in the graph"))
    }

    fn is_pending(&self, p: usize) -> bool {
        self.nodes[p].status == STATUS_PENDING
    }

    /// Lowest holder id first, then the lexicographically first shared file, so
    /// a task blocked on several claims always names the same one.
    fn first_claim(&self, p: usize, holders: &[usize]) -> Option<(u32, String)> {
        let mine: BTreeSet<&str> = self.nodes[p].files.iter().map(String::as_str).collect();
        for h in holders {
            let theirs: BTreeSet<&str> = self.nodes[*h].files.iter().map(String::as_str).collect();
            if let Some(file) = mine.intersection(&theirs).next() {
                return Some((self.nodes[*h].id, (*file).to_string()));
            }
        }
        None
    }

    fn ids(&self, bits: &Bits) -> Vec<u32> {
        (0..self.nodes.len())
            .filter(|p| bits_has(bits, *p))
            .map(|p| self.nodes[p].id)
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    White,
    Grey,
    Black,
}

/// Seeds ascending, emits each round sorted, decrements successors ascending —
/// one O(V+E) pass giving both a stable total order and a stable batch
/// structure. Positions left unemitted are the cycles and whatever they strand.
fn layered_kahn(
    n: usize,
    preds: &[Vec<usize>],
    succs: &[Vec<usize>],
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut indeg: Vec<usize> = preds.iter().map(Vec::len).collect();
    let mut emitted = vec![false; n];
    let mut rounds: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = (0..n).filter(|p| indeg[*p] == 0).collect();

    while !current.is_empty() {
        current.sort_unstable();
        let mut next = Vec::new();
        for p in &current {
            emitted[*p] = true;
            for s in &succs[*p] {
                indeg[*s] -= 1;
                if indeg[*s] == 0 {
                    next.push(*s);
                }
            }
        }
        rounds.push(std::mem::take(&mut current));
        current = next;
    }

    (rounds, (0..n).filter(|p| !emitted[*p]).collect())
}

fn reachability(
    n: usize,
    preds: &[Vec<usize>],
    succs: &[Vec<usize>],
    rounds: &[Vec<usize>],
) -> Reach {
    let topo: Vec<usize> = rounds.iter().flatten().copied().collect();

    let mut up = vec![[0u64; WORDS]; n];
    for p in &topo {
        bits_set(&mut up[*p], *p);
        for d in &preds[*p] {
            let folded = up[*d];
            bits_or(&mut up[*p], &folded);
        }
    }

    let mut down = vec![[0u64; WORDS]; n];
    for p in topo.iter().rev() {
        bits_set(&mut down[*p], *p);
        for s in &succs[*p] {
            let folded = down[*s];
            bits_or(&mut down[*p], &folded);
        }
    }

    Reach { up, down }
}

fn bits_set(bits: &mut Bits, i: usize) {
    bits[i / 64] |= 1u64 << (i % 64);
}

fn bits_has(bits: &Bits, i: usize) -> bool {
    bits[i / 64] & (1u64 << (i % 64)) != 0
}

fn bits_or(into: &mut Bits, from: &Bits) {
    for w in 0..WORDS {
        into[w] |= from[w];
    }
}

fn bits_subset(a: &Bits, b: &Bits) -> bool {
    (0..WORDS).all(|w| a[w] & !b[w] == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, needs: &[u32], files: &[&str], checkpoint: &str) -> Node {
        Node {
            id,
            files: files.iter().map(|f| (*f).to_string()).collect(),
            needs: needs.to_vec(),
            coupling: Vec::new(),
            status: STATUS_PENDING.to_string(),
            checkpoint: checkpoint.to_string(),
        }
    }

    /// Twelve tasks: a five-layer DAG, one isolated task (12), one coupling
    /// edge (7 → 3), two file overlaps with no path between them (2/3 on
    /// `t2.rs`, 5/12 on `t5.rs`) and one file shared *along* an edge (1/3 on
    /// `t1.rs`) that must not count as an overlap.
    fn fixture() -> Vec<Node> {
        let mut nodes = vec![
            node(1, &[], &["t1.rs"], "A"),
            node(2, &[], &["t2.rs"], "A"),
            node(3, &[1], &["t3.rs", "t2.rs", "t1.rs"], "A"),
            node(4, &[1, 2], &["t4.rs"], "A"),
            node(5, &[2], &["t5.rs"], "A"),
            node(6, &[3, 4], &["t6.rs"], "B"),
            node(7, &[4], &["t7.rs"], "B"),
            node(8, &[5], &["t8.rs"], "B"),
            node(9, &[6, 7], &["t9.rs"], "B"),
            node(10, &[8], &["t10.rs"], "B"),
            node(11, &[9, 10], &["t11.rs"], "C"),
            node(12, &[], &["t12.rs", "t5.rs"], "C"),
        ];
        nodes[6].coupling = vec![3];
        nodes
    }

    fn order() -> Vec<String> {
        vec!["A".to_string(), "B".to_string(), "C".to_string()]
    }

    fn find(nodes: &mut [Node], id: u32) -> &mut Node {
        nodes.iter_mut().find(|n| n.id == id).expect("fixture id")
    }

    #[test]
    fn kahn_rounds_are_ascending_layers_forming_a_topological_order() {
        let nodes = fixture();
        let graph = Graph::build(&nodes).expect("acyclic fixture builds");
        let rounds = graph.kahn_rounds();

        assert_eq!(
            rounds,
            vec![
                vec![1, 2, 12],
                vec![3, 4, 5],
                vec![6, 7, 8],
                vec![9, 10],
                vec![11]
            ]
        );
        for round in &rounds {
            assert!(
                round.windows(2).all(|w| w[0] < w[1]),
                "not ascending: {round:?}"
            );
        }

        let flat: Vec<u32> = rounds.concat();
        assert_eq!(flat.len(), nodes.len());
        let rank: BTreeMap<u32, usize> = flat.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        for n in &nodes {
            for dep in n.needs.iter().chain(n.coupling.iter()) {
                assert!(
                    rank[dep] < rank[&n.id],
                    "edge {dep} -> {} is inverted in {flat:?}",
                    n.id
                );
            }
        }
    }

    /// 5 hangs off the cycle without being on it, so reporting the whole
    /// residue would name four tasks.
    #[test]
    fn a_three_cycle_reports_exactly_its_members() {
        let nodes = vec![
            node(1, &[], &[], ""),
            node(2, &[4], &[], ""),
            node(3, &[2], &[], ""),
            node(4, &[3], &[], ""),
            node(5, &[4], &[], ""),
        ];
        let graph = Graph::build(&nodes).expect("a cycle is not a build error");

        assert_eq!(graph.cycle_members(), vec![2, 3, 4]);
        assert_eq!(graph.kahn_rounds(), vec![vec![1]]);
        assert!(graph.closure_up(1).is_err());
    }

    #[test]
    fn closures_run_down_to_dependents_and_up_to_dependencies() {
        let nodes = fixture();
        let graph = Graph::build(&nodes).expect("acyclic fixture builds");

        assert_eq!(graph.closure_down(1).unwrap(), vec![1, 3, 4, 6, 7, 9, 11]);
        assert_eq!(graph.closure_up(1).unwrap(), vec![1]);
        assert_eq!(graph.closure_up(9).unwrap(), vec![1, 2, 3, 4, 6, 7, 9]);
        assert_eq!(graph.closure_down(12).unwrap(), vec![12]);
    }

    #[test]
    fn a_group_missing_a_dependency_from_its_prefix_is_not_a_valid_cut() {
        let nodes = fixture();
        let graph = Graph::build(&nodes).expect("acyclic fixture builds");
        let groups = graph.groups(&order()).unwrap();

        assert_eq!(
            groups.iter().map(|g| g.valid_cut).collect::<Vec<_>>(),
            vec![true, true, true]
        );
        assert_eq!(groups[1].members, vec![6, 7, 8, 9, 10]);
        assert_eq!(groups[0].maximal, vec![3, 4, 5]);
        assert_eq!(groups[1].maximal, vec![9, 10]);

        // 10 needs 8; moving 8 into the later group C leaves A ∪ B open below.
        let mut moved = fixture();
        find(&mut moved, 8).checkpoint = "C".to_string();
        let graph = Graph::build(&moved).expect("acyclic fixture builds");
        let groups = graph.groups(&order()).unwrap();

        assert_eq!(
            groups.iter().map(|g| g.valid_cut).collect::<Vec<_>>(),
            vec![true, false, true]
        );
    }

    #[test]
    fn a_graph_above_the_node_cap_errors() {
        let over: Vec<Node> = (1..=257).map(|id| node(id, &[], &[], "")).collect();
        let err = Graph::build(&over).expect_err("257 nodes exceed the cap");
        assert!(
            err.to_string().contains("exceeds 256 tasks"),
            "unexpected message: {err}"
        );

        let at_cap: Vec<Node> = (1..=256).map(|id| node(id, &[], &[], "")).collect();
        assert!(Graph::build(&at_cap).is_ok());
    }

    #[test]
    fn a_file_claimed_by_an_in_flight_task_holds_a_ready_task() {
        let mut nodes = fixture();
        find(&mut nodes, 1).status = STATUS_DONE.to_string();
        find(&mut nodes, 2).status = "in-progress".to_string();
        let graph = Graph::build(&nodes).expect("acyclic fixture builds");

        let frontier = graph.frontier(&[2]).unwrap();
        assert_eq!(frontier.ready, vec![12]);
        assert_eq!(
            frontier.held,
            vec![Held {
                id: 3,
                blocked_on_file: "t2.rs".to_string(),
                holder: 2,
            }]
        );
        assert_eq!(frontier.next, vec![4, 5]);
        assert!(graph.frontier(&[99]).is_err());
    }

    #[test]
    fn overlap_pairs_skip_files_shared_along_an_edge() {
        let nodes = fixture();
        let graph = Graph::build(&nodes).expect("acyclic fixture builds");

        assert_eq!(graph.overlap_pairs().unwrap(), vec![(2, 3), (5, 12)]);
    }

    #[test]
    fn an_edge_to_an_absent_task_is_a_build_error() {
        let nodes = vec![node(1, &[], &[], ""), node(2, &[99], &[], "")];
        let err = Graph::build(&nodes).expect_err("99 is absent");
        assert!(err.to_string().contains("absent task 99"), "{err}");
    }
}
