//! Neighbourhoods of one task over `needs` edges, and the search for the largest one
//! that fits the view.
//!
//! A neighbourhood is every task within a radius of hops from the target, walked up
//! through prerequisites ([`Focus::Ancestry`]), down through dependents
//! ([`Focus::Subtree`]), both ways but never turning ([`Focus::Both`]), or along any
//! edge ([`Focus::Off`], which reaches siblings and cousins too). [`fit`] grows the
//! radius while a caller-supplied predicate accepts the result, so it needs no layout
//! of its own. Every set is a `BTreeSet`, so the search is deterministic.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::Snapshot;

/// Which way the neighbourhood walk follows `needs` edges from the target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum Focus {
    #[default]
    Off,
    Ancestry,
    Subtree,
    Both,
}

/// The tasks [`fit`] or [`closure`] chose to draw, how many of the snapshot's tasks it
/// left out, and the largest whole radius it kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Subset {
    pub(crate) ids: BTreeSet<u32>,
    pub(crate) hidden: usize,
    pub(crate) radius: usize,
}

/// The tasks within `radius` hops of `target`, the target included; empty when the
/// snapshot has no such task. `usize::MAX` yields the whole closure.
#[cfg(test)]
pub(crate) fn generations(
    snapshot: &Snapshot,
    target: u32,
    focus: Focus,
    radius: usize,
) -> BTreeSet<u32> {
    Links::new(snapshot).walk(target, focus, radius)
}

/// The whole neighbourhood of `target` under `focus`, at radius `usize::MAX`.
pub(crate) fn closure(snapshot: &Snapshot, target: u32, focus: Focus) -> Subset {
    let links = Links::new(snapshot);
    let ids = links.walk(target, focus, usize::MAX);
    Subset {
        hidden: links.ids.len() - ids.len(),
        ids,
        radius: usize::MAX,
    }
}

/// Splits `ring` into families: the tasks sharing one neighbour in `inside`, keyed on
/// the lowest-id such neighbour and ordered by it. A ring task with no neighbour in
/// `inside` forms a family of its own, after the rest.
#[cfg(test)]
pub(crate) fn families(
    snapshot: &Snapshot,
    inside: &BTreeSet<u32>,
    ring: &BTreeSet<u32>,
) -> Vec<BTreeSet<u32>> {
    Links::new(snapshot).families(inside, ring)
}

/// The largest neighbourhood of `target` that `fits` accepts: the radius doubles
/// while it fits, is bisected down to the largest radius that does, and the next
/// ring's families are then added one at a time wherever they still fit. Every set
/// returned was accepted by `fits`, except the floor: the target alone, returned at
/// radius 0 when even that is refused. The bisection assumes a larger set never fits
/// where a smaller one did not.
pub(crate) fn fit(
    snapshot: &Snapshot,
    target: u32,
    focus: Focus,
    mut fits: impl FnMut(&BTreeSet<u32>) -> bool,
) -> Subset {
    let links = Links::new(snapshot);
    let total = links.ids.len();
    let finish = |ids: BTreeSet<u32>, radius: usize| Subset {
        hidden: total - ids.len(),
        ids,
        radius,
    };

    let mut best = links.walk(target, focus, 0);
    if best.is_empty() || !fits(&best) {
        return finish(best, 0);
    }
    // Walks are nested, so an equal length means an equal set: the closure is
    // complete and already accepted.
    let mut lo = 0;
    let mut radius = 1;
    let (mut hi, mut over) = loop {
        let grown = links.walk(target, focus, radius);
        if grown.len() == best.len() {
            return finish(best, lo);
        }
        if !fits(&grown) {
            break (radius, grown);
        }
        lo = radius;
        best = grown;
        radius = radius.saturating_mul(2);
    };
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        let candidate = links.walk(target, focus, mid);
        if candidate.len() == best.len() || fits(&candidate) {
            lo = mid;
            best = candidate;
        } else {
            hi = mid;
            over = candidate;
        }
    }

    let ring: BTreeSet<u32> = over.difference(&best).copied().collect();
    for family in links.families(&best, &ring) {
        let mut candidate = best.clone();
        candidate.extend(family);
        if fits(&candidate) {
            best = candidate;
        }
    }
    finish(best, lo)
}

/// The `needs` adjacency of a snapshot, restricted to tasks it carries.
struct Links {
    ids: BTreeSet<u32>,
    up: BTreeMap<u32, BTreeSet<u32>>,
    down: BTreeMap<u32, BTreeSet<u32>>,
}

impl Links {
    fn new(snapshot: &Snapshot) -> Self {
        let ids: BTreeSet<u32> = snapshot.tasks.iter().map(|task| task.id).collect();
        let mut up: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        let mut down: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for task in &snapshot.tasks {
            for &need in &task.needs {
                if need != task.id && ids.contains(&need) {
                    up.entry(task.id).or_default().insert(need);
                    down.entry(need).or_default().insert(task.id);
                }
            }
        }
        Self { ids, up, down }
    }

    fn walk(&self, target: u32, focus: Focus, radius: usize) -> BTreeSet<u32> {
        if !self.ids.contains(&target) {
            return BTreeSet::new();
        }
        match focus {
            Focus::Off => self.ball(target, radius, true, true),
            Focus::Ancestry => self.ball(target, radius, true, false),
            Focus::Subtree => self.ball(target, radius, false, true),
            Focus::Both => {
                let mut both = self.ball(target, radius, true, false);
                both.extend(self.ball(target, radius, false, true));
                both
            }
        }
    }

    /// Breadth-first from `target` over the edges `up` and `down` select, stopping
    /// after `radius` hops or when nothing new is reached.
    fn ball(&self, target: u32, radius: usize, up: bool, down: bool) -> BTreeSet<u32> {
        let mut seen = BTreeSet::from([target]);
        let mut frontier = vec![target];
        let mut hops = 0;
        while hops < radius && !frontier.is_empty() {
            let mut next = Vec::new();
            for id in frontier {
                let ups = self.up.get(&id).filter(|_| up);
                let downs = self.down.get(&id).filter(|_| down);
                for &other in ups.into_iter().chain(downs).flatten() {
                    if seen.insert(other) {
                        next.push(other);
                    }
                }
            }
            frontier = next;
            hops += 1;
        }
        seen
    }

    fn families(&self, inside: &BTreeSet<u32>, ring: &BTreeSet<u32>) -> Vec<BTreeSet<u32>> {
        let mut by_anchor: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        let mut loose = Vec::new();
        for &id in ring {
            let anchor = [self.up.get(&id), self.down.get(&id)]
                .into_iter()
                .flatten()
                .flatten()
                .filter(|other| inside.contains(other))
                .min();
            match anchor {
                Some(&anchor) => {
                    by_anchor.entry(anchor).or_default().insert(id);
                }
                None => loose.push(BTreeSet::from([id])),
            }
        }
        by_anchor.into_values().chain(loose).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::tests::{shortcut, two_components, wide};

    const FOCUSES: [Focus; 4] = [Focus::Off, Focus::Ancestry, Focus::Subtree, Focus::Both];

    fn set(ids: &[u32]) -> BTreeSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn radius_one_ancestry_is_the_direct_prerequisites() {
        let snap = wide();
        assert_eq!(generations(&snap, 7, Focus::Ancestry, 0), set(&[7]));
        assert_eq!(generations(&snap, 7, Focus::Ancestry, 1), set(&[4, 5, 7]));
        assert_eq!(
            generations(&snap, 7, Focus::Ancestry, 2),
            set(&[1, 2, 4, 5, 7])
        );
        assert_eq!(
            generations(&shortcut(), 7, Focus::Ancestry, 1),
            set(&[1, 4, 5, 7]),
            "a shortcut edge is a direct prerequisite"
        );
    }

    #[test]
    fn both_walks_each_way_without_turning() {
        let snap = wide();
        assert_eq!(generations(&snap, 4, Focus::Both, 2), set(&[1, 4, 7]));
        assert_eq!(
            generations(&snap, 4, Focus::Off, 2),
            set(&[1, 4, 5, 7, 8]),
            "off turns at a shared parent or child"
        );
        assert_eq!(
            generations(&two_components(), 2, Focus::Both, 2),
            set(&[1, 2, 3, 4, 5, 6]),
            "the other component stays out"
        );
    }

    #[test]
    fn an_unknown_target_has_no_neighbourhood() {
        let snap = wide();
        assert!(generations(&snap, 99, Focus::Off, usize::MAX).is_empty());
        let subset = fit(&snap, 99, Focus::Off, |_| true);
        assert!(subset.ids.is_empty());
        assert_eq!(subset.hidden, 9);
    }

    #[test]
    fn families_group_ring_tasks_by_their_shared_neighbour() {
        let snap = two_components();
        assert_eq!(
            families(&snap, &set(&[1, 2]), &set(&[3, 4, 5, 6])),
            vec![set(&[3, 4, 5, 6])]
        );
        assert_eq!(
            families(&wide(), &set(&[4, 5, 7]), &set(&[1, 2])),
            vec![set(&[1]), set(&[2])]
        );
    }

    #[test]
    fn fit_stops_at_the_largest_radius_the_predicate_accepts() {
        let snap = two_components();
        let subset = fit(&snap, 1, Focus::Subtree, |ids| ids.len() <= 3);
        assert_eq!(subset.ids, set(&[1, 2]));
        assert_eq!(subset.radius, 1);
        assert_eq!(subset.hidden, 6, "every task not drawn is hidden");

        let whole = fit(&snap, 1, Focus::Subtree, |_| true);
        assert_eq!(whole.ids, set(&[1, 2, 3, 4, 5, 6]));
        assert_eq!(whole.radius, 2);
        assert_eq!(whole.hidden, 2, "the other component is hidden");
    }

    #[test]
    fn fit_adds_the_next_rings_families_that_still_fit() {
        let subset = fit(&wide(), 7, Focus::Ancestry, |ids| ids.len() <= 4);
        assert_eq!(subset.ids, set(&[1, 4, 5, 7]));
        assert_eq!(subset.radius, 1);
        assert_eq!(subset.hidden, 5);
    }

    #[test]
    fn fit_falls_back_to_the_target_alone() {
        let subset = fit(&wide(), 4, Focus::Both, |_| false);
        assert_eq!(subset.ids, set(&[4]));
        assert_eq!(subset.radius, 0);
        assert_eq!(subset.hidden, 8);
    }

    #[test]
    fn fit_returns_only_accepted_sets_at_the_largest_fitting_radius() {
        let snap = wide();
        let ids: Vec<u32> = snap.tasks.iter().map(|task| task.id).collect();
        for &target in &ids {
            for focus in FOCUSES {
                for cap in 0..=ids.len() {
                    let mut accepted = Vec::new();
                    let subset = fit(&snap, target, focus, |ids| {
                        let ok = ids.len() <= cap;
                        if ok {
                            accepted.push(ids.clone());
                        }
                        ok
                    });
                    let case = format!("target {target}, {focus:?}, cap {cap}");
                    assert!(
                        accepted.contains(&subset.ids) || subset.ids == set(&[target]),
                        "{case}: returned a set the predicate never accepted"
                    );
                    let inner = generations(&snap, target, focus, subset.radius);
                    let outer = generations(&snap, target, focus, subset.radius + 1);
                    assert!(subset.ids.is_superset(&inner), "{case}");
                    assert!(subset.ids.is_subset(&outer), "{case}");
                    if cap > 0 && outer != inner {
                        assert!(outer.len() > cap, "{case}: a larger radius fit");
                    }
                    assert_eq!(subset.hidden, ids.len() - subset.ids.len(), "{case}");
                }
            }
        }
    }
}
