//! Change sets between two successive snapshots.

use std::collections::HashMap;

use crate::model::{AgentStatus, Snapshot, topology_hash};

/// What moved between two snapshots. Ids are in the new snapshot's order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Changes {
    /// An id or an edge was added, removed or rewired.
    pub(crate) topology_changed: bool,
    /// `(task id, old status, new status)` for rows present in both.
    pub(crate) status_changed: Vec<(u32, String, String)>,
    /// Agents absent from the old snapshot.
    pub(crate) agents_started: Vec<String>,
    /// Agents that were not stopped in the old snapshot and are now.
    pub(crate) agents_stopped: Vec<String>,
    /// Record entries absent from the old snapshot.
    pub(crate) record_added: Vec<String>,
}

#[cfg(test)]
impl Changes {
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

pub(crate) fn diff(old: &Snapshot, new: &Snapshot) -> Changes {
    let old_status: HashMap<u32, &str> = old
        .tasks
        .iter()
        .map(|task| (task.id, task.status.as_str()))
        .collect();
    let status_changed = new
        .tasks
        .iter()
        .filter_map(|task| {
            let before = *old_status.get(&task.id)?;
            let after = task.status.as_str();
            (before != after).then(|| (task.id, before.to_string(), after.to_string()))
        })
        .collect();

    let old_agents: HashMap<&str, AgentStatus> = old
        .agents
        .iter()
        .map(|agent| (agent.id.as_str(), agent.status))
        .collect();
    let mut agents_started = Vec::new();
    let mut agents_stopped = Vec::new();
    for agent in &new.agents {
        match old_agents.get(agent.id.as_str()) {
            None => agents_started.push(agent.id.clone()),
            Some(before)
                if *before != AgentStatus::Stopped && agent.status == AgentStatus::Stopped =>
            {
                agents_stopped.push(agent.id.clone());
            }
            Some(_) => {}
        }
    }

    let old_record: std::collections::HashSet<&str> =
        old.record.iter().map(|entry| entry.id.as_str()).collect();
    let record_added = new
        .record
        .iter()
        .filter(|entry| !old_record.contains(entry.id.as_str()))
        .map(|entry| entry.id.clone())
        .collect();

    Changes {
        topology_changed: topology_hash(&old.tasks) != topology_hash(&new.tasks),
        status_changed,
        agents_started,
        agents_stopped,
        record_added,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RecordEntry, TaskStatus, fixture};

    #[test]
    fn identical_snapshots_change_nothing() {
        let snap = fixture();
        assert!(diff(&snap, &snap.clone()).is_empty());
    }

    #[test]
    fn a_status_change_is_reported_without_a_topology_change() {
        let old = fixture();
        let mut new = old.clone();
        let from = new.tasks[4].status.as_str().to_string();
        new.tasks[4].status = TaskStatus::Done;
        let changes = diff(&old, &new);
        assert!(!changes.topology_changed);
        assert_eq!(
            changes.status_changed,
            vec![(new.tasks[4].id, from, "done".to_string())]
        );
    }

    #[test]
    fn an_added_edge_is_a_topology_change() {
        let old = fixture();
        let mut new = old.clone();
        new.tasks[7].needs.push(2);
        let changes = diff(&old, &new);
        assert!(changes.topology_changed);
        assert!(changes.status_changed.is_empty());
    }

    #[test]
    fn agents_start_and_stop() {
        let old = fixture();
        let mut new = old.clone();
        let mut started = new.agents[0].clone();
        started.id = "A9".to_string();
        started.status = AgentStatus::Running;
        new.agents.push(started);
        let running = new
            .agents
            .iter()
            .position(|a| a.status != AgentStatus::Stopped && a.id != "A9")
            .expect("the fixture has a live agent");
        new.agents[running].status = AgentStatus::Stopped;
        let stopped_id = new.agents[running].id.clone();
        let changes = diff(&old, &new);
        assert_eq!(changes.agents_started, ["A9"]);
        assert_eq!(changes.agents_stopped, [stopped_id]);
    }

    #[test]
    fn new_record_entries_are_listed_in_order() {
        let old = fixture();
        let mut new = old.clone();
        for id in ["E90", "E91"] {
            new.record.push(RecordEntry {
                id: id.to_string(),
                ..RecordEntry::default()
            });
        }
        let changes = diff(&old, &new);
        assert_eq!(changes.record_added, ["E90", "E91"]);
        assert!(changes.agents_started.is_empty());
    }
}
