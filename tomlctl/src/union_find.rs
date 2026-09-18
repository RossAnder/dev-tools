//! Union-find over dense indices, shared by `backlog cluster` and `items clusters`.

use std::collections::BTreeMap;

pub(crate) fn root(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

pub(crate) fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (root(parent, a), root(parent, b));
    if ra == rb {
        return;
    }
    // Lowest index wins, so a component's representative does not depend on
    // the order the edges were discovered in.
    if ra < rb {
        parent[rb] = ra
    } else {
        parent[ra] = rb
    }
}

/// Components keyed by representative, members in index order. With
/// `keep_singletons` false, one-member components are dropped: an item linked
/// to nothing is not a cluster. With it true every index appears, so the
/// result partitions `0..len`.
pub(crate) fn components(
    parent: &mut [usize],
    len: usize,
    keep_singletons: bool,
) -> BTreeMap<usize, Vec<usize>> {
    let mut out: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..len {
        out.entry(root(parent, index)).or_default().push(index);
    }
    if !keep_singletons {
        out.retain(|_, members| members.len() > 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh(len: usize) -> Vec<usize> {
        (0..len).collect()
    }

    #[test]
    fn representative_is_the_lowest_index_whatever_the_edge_order() {
        let mut forward = fresh(4);
        union(&mut forward, 0, 3);
        union(&mut forward, 3, 1);
        let mut backward = fresh(4);
        union(&mut backward, 1, 3);
        union(&mut backward, 3, 0);
        for parent in [&mut forward, &mut backward] {
            assert_eq!(root(parent, 0), 0);
            assert_eq!(root(parent, 1), 0);
            assert_eq!(root(parent, 3), 0);
            assert_eq!(root(parent, 2), 2);
        }
    }

    #[test]
    fn components_drop_singletons_by_default() {
        let mut parent = fresh(4);
        union(&mut parent, 0, 2);
        union(&mut parent, 2, 2);
        let groups = components(&mut parent, 4, false);
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[&0], [0, 2]);
    }

    #[test]
    fn components_keep_singletons_partition_every_index() {
        let mut parent = fresh(5);
        union(&mut parent, 1, 3);
        let groups = components(&mut parent, 5, true);
        assert_eq!(
            groups.keys().copied().collect::<Vec<_>>(),
            [0, 1, 2, 4],
            "{groups:?}"
        );
        assert_eq!(groups[&0], [0]);
        assert_eq!(groups[&1], [1, 3]);
        assert_eq!(groups[&2], [2]);
        assert_eq!(groups[&4], [4]);
        assert_eq!(groups.values().map(Vec::len).sum::<usize>(), 5);
    }
}
