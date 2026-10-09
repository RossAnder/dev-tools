//! The `train` verb — the commit groups for a window of `done`, uncommitted rows.
//!
//! Groups start from the commit granularity and merge on any shared file,
//! across dependency layers, so no file is staged by two commits. A merge can
//! join groups that depend on each other both ways, so each strongly connected
//! component of the group dependency graph is collapsed into one group before
//! the groups are ordered. Dependencies are read over the whole store, so a row
//! outside the window still orders the groups on either side of it.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow};
use serde_json::{Value as JsonValue, json};

use super::graph::{Tense, build_or_refuse, layered_kahn, nodes_of};
use super::parse_policy::GRANULARITY_VALUES;
use super::schema::{Status, Store, TaskRow};
use crate::errors::{ErrorKind, tagged_err};
use crate::union_find::{components, union};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Granularity {
    PerTask,
    PerCheckpoint,
    SingleCommit,
}

impl Granularity {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "per-task" => Some(Self::PerTask),
            "per-checkpoint" => Some(Self::PerCheckpoint),
            "single-commit" => Some(Self::SingleCommit),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::PerTask => "per-task",
            Self::PerCheckpoint => "per-checkpoint",
            Self::SingleCommit => "single-commit",
        }
    }
}

/// `{granularity, groups[]}`, each group `{ids, refs, files, checkpoints,
/// shared_with_pending}`, in commit order: dependency layer by layer, lowest
/// id first within a layer. `checkpoints` and `ids` narrow the candidates to
/// rows in either; both empty means every `done`, uncommitted row.
pub(crate) fn train(
    store: &Store,
    checkpoints: &[String],
    ids: &[u32],
    granularity: Option<&str>,
) -> Result<JsonValue> {
    let granularity = resolve_granularity(store, granularity)?;
    check_window(store, checkpoints, ids)?;

    let nodes = nodes_of(&store.items);
    let graph = build_or_refuse(&nodes, "the commit train", Tense::Stored)?;

    let mut candidates: Vec<&TaskRow> = store
        .items
        .iter()
        .filter(|row| row.status == Status::Done && row.commit.trim().is_empty())
        .filter(|row| {
            (checkpoints.is_empty() && ids.is_empty())
                || checkpoints.contains(&row.checkpoint)
                || ids.contains(&row.id)
        })
        .collect();
    candidates.sort_by_key(|row| row.id);

    let initial = initial_groups(&candidates, granularity);

    let mut owner: BTreeMap<u32, usize> = BTreeMap::new();
    for (g, members) in initial.iter().enumerate() {
        for c in members {
            owner.insert(candidates[*c].id, g);
        }
    }
    let mut depends_on: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); initial.len()];
    for (g, members) in initial.iter().enumerate() {
        for c in members {
            for ancestor in graph.closure_up(candidates[*c].id)? {
                if let Some(h) = owner.get(&ancestor).copied().filter(|h| *h != g) {
                    depends_on[g].insert(h);
                }
            }
        }
    }

    let groups = collapse_cycles(&initial, &depends_on);
    let order = commit_order(&groups, initial.len(), &depends_on)?;

    let pending: Vec<&TaskRow> = store
        .items
        .iter()
        .filter(|row| row.status != Status::Done)
        .collect();
    let rows: Vec<JsonValue> = order
        .iter()
        .map(|m| {
            let mut members: Vec<usize> = groups[*m]
                .iter()
                .flat_map(|g| initial[*g].iter().copied())
                .collect();
            members.sort_unstable();
            group_json(&members, &candidates, &pending)
        })
        .collect();

    Ok(json!({
        "granularity": granularity.as_str(),
        "groups": rows,
    }))
}

fn resolve_granularity(store: &Store, flag: Option<&str>) -> Result<Granularity> {
    let raw = flag.unwrap_or(&store.policy.commit_granularity);
    Granularity::parse(raw).ok_or_else(|| {
        refuse(format!(
            "commit granularity `{raw}` is not one of {}; pass --granularity",
            GRANULARITY_VALUES.join(", ")
        ))
    })
}

/// An id or checkpoint naming nothing in the store is refused rather than
/// read as an empty window, which would print a train with nothing in it.
fn check_window(store: &Store, checkpoints: &[String], ids: &[u32]) -> Result<()> {
    for id in ids {
        if !store.items.iter().any(|row| row.id == *id) {
            return Err(refuse(format!("no task {id} in the store")));
        }
    }
    for checkpoint in checkpoints {
        let declared = store.checkpoints.iter().any(|c| c.id == *checkpoint);
        let claimed = store.items.iter().any(|row| row.checkpoint == *checkpoint);
        if checkpoint.is_empty() || !(declared || claimed) {
            return Err(refuse(format!("no checkpoint `{checkpoint}` in the store")));
        }
    }
    Ok(())
}

/// Candidate positions per group, each ascending, groups ordered by their
/// lowest member. Granularity seeds the groups; a shared file then merges them.
fn initial_groups(candidates: &[&TaskRow], granularity: Granularity) -> Vec<Vec<usize>> {
    let n = candidates.len();
    let mut parent: Vec<usize> = (0..n).collect();

    let mut first_in_checkpoint: BTreeMap<&str, usize> = BTreeMap::new();
    for (c, row) in candidates.iter().enumerate() {
        match granularity {
            Granularity::PerTask => {}
            Granularity::SingleCommit => union(&mut parent, 0, c),
            Granularity::PerCheckpoint => {
                let first = *first_in_checkpoint.entry(&row.checkpoint).or_insert(c);
                union(&mut parent, first, c);
            }
        }
    }

    let mut first_claim: BTreeMap<&str, usize> = BTreeMap::new();
    for (c, row) in candidates.iter().enumerate() {
        for file in &row.files {
            let first = *first_claim.entry(file).or_insert(c);
            union(&mut parent, first, c);
        }
    }

    components(&mut parent, n, true).into_values().collect()
}

/// Merges every set of groups that reach each other through `depends_on`,
/// returning the initial-group indices per merged group, ascending.
fn collapse_cycles(initial: &[Vec<usize>], depends_on: &[BTreeSet<usize>]) -> Vec<Vec<usize>> {
    let n = initial.len();
    let reach: Vec<Vec<bool>> = (0..n).map(|g| reachable_from(g, depends_on)).collect();
    let mut parent: Vec<usize> = (0..n).collect();
    for (g, from_g) in reach.iter().enumerate() {
        for (h, from_h) in reach.iter().enumerate().skip(g + 1) {
            if from_g[h] && from_h[g] {
                union(&mut parent, g, h);
            }
        }
    }
    components(&mut parent, n, true).into_values().collect()
}

fn reachable_from(start: usize, depends_on: &[BTreeSet<usize>]) -> Vec<bool> {
    let mut seen = vec![false; depends_on.len()];
    let mut stack = vec![start];
    while let Some(g) = stack.pop() {
        for h in &depends_on[g] {
            if !std::mem::replace(&mut seen[*h], true) {
                stack.push(*h);
            }
        }
    }
    seen
}

/// Merged-group indices in commit order. Merged groups are indexed by their
/// lowest initial group, whose lowest member is the merged group's lowest id,
/// so Kahn's ascending tie-break is a lowest-id tie-break.
fn commit_order(
    groups: &[Vec<usize>],
    initial_len: usize,
    depends_on: &[BTreeSet<usize>],
) -> Result<Vec<usize>> {
    let mut merged_of = vec![0; initial_len];
    for (m, members) in groups.iter().enumerate() {
        for g in members {
            merged_of[*g] = m;
        }
    }

    let n = groups.len();
    let mut preds: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (g, deps) in depends_on.iter().enumerate() {
        for h in deps {
            if merged_of[g] != merged_of[*h] {
                preds[merged_of[g]].insert(merged_of[*h]);
            }
        }
    }
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (m, deps) in preds.iter().enumerate() {
        for d in deps {
            succs[*d].push(m);
        }
    }
    let preds: Vec<Vec<usize>> = preds.into_iter().map(|p| p.into_iter().collect()).collect();

    let (rounds, residual) = layered_kahn(n, &preds, &succs);
    if !residual.is_empty() {
        return Err(anyhow!(
            "commit groups still form a cycle after collapsing strongly connected components"
        ));
    }
    Ok(rounds.into_iter().flatten().collect())
}

/// `shared_with_pending` names each group file a row not yet `done` also
/// claims: an edit to it may already sit in the working tree, and staging the
/// file would sweep that edit into this commit.
fn group_json(members: &[usize], candidates: &[&TaskRow], pending: &[&TaskRow]) -> JsonValue {
    let rows: Vec<&TaskRow> = members.iter().map(|c| candidates[*c]).collect();
    let files: BTreeSet<&str> = rows
        .iter()
        .flat_map(|row| row.files.iter().map(String::as_str))
        .collect();
    let checkpoints: BTreeSet<&str> = rows
        .iter()
        .map(|row| row.checkpoint.as_str())
        .filter(|checkpoint| !checkpoint.is_empty())
        .collect();
    let shared: BTreeSet<&str> = pending
        .iter()
        .flat_map(|row| row.files.iter().map(String::as_str))
        .filter(|file| files.contains(file))
        .collect();

    json!({
        "ids": rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        "refs": rows.iter().map(|row| row.r#ref.as_str()).collect::<Vec<_>>(),
        "files": files,
        "checkpoints": checkpoints,
        "shared_with_pending": shared,
    })
}

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}
