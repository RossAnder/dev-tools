//! Deserialised form of the `tomlctl tasks snapshot` JSON document, and the
//! [`Index`] the views look rows up through.
//!
//! Every struct reads with `#[serde(default)]`: a missing field reads as its
//! empty value and an unknown one is ignored, so a snapshot from a newer
//! tomlctl degrades rather than failing to load. The cost is that a misspelt
//! field also reads as empty, which the fixture round-trip test guards.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Snapshot {
    pub(crate) schema: u32,
    /// Fingerprint of the input files; equal revisions mean equal content.
    pub(crate) revision: String,
    pub(crate) slug: String,
    pub(crate) plan_path: String,
    /// The flow's `context.toml` status, `""` when it has none.
    pub(crate) flow_status: String,
    pub(crate) policy: Policy,
    pub(crate) tasks: Vec<Task>,
    /// Kahn rounds over `needs` and `coupling`, each ascending.
    pub(crate) layers: Vec<Vec<u32>>,
    pub(crate) frontier: Frontier,
    pub(crate) edges: Vec<Edge>,
    pub(crate) checkpoints: Vec<Checkpoint>,
    pub(crate) record: Vec<RecordEntry>,
    pub(crate) agents: Vec<Agent>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Policy {
    pub(crate) checkpoints: String,
    pub(crate) max_parallel: u32,
    pub(crate) commit_granularity: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Task {
    pub(crate) id: u32,
    #[serde(rename = "ref")]
    pub(crate) task_ref: String,
    pub(crate) title: String,
    pub(crate) effort: String,
    pub(crate) status: TaskStatus,
    pub(crate) checkpoint: String,
    pub(crate) phase: String,
    pub(crate) files: Vec<String>,
    pub(crate) needs: Vec<u32>,
    pub(crate) coupling: Vec<u32>,
    pub(crate) deps_note: String,
    pub(crate) action: String,
    pub(crate) detail: String,
    pub(crate) acceptance: String,
    pub(crate) agent: String,
    pub(crate) commit: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TaskStatus {
    #[default]
    Pending,
    InProgress,
    Done,
    Failed,
    Deferred,
    /// A status outside the vocabulary this build knows.
    #[serde(other)]
    Unknown,
}

impl TaskStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in-progress",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Deferred => "deferred",
            Self::Unknown => "unknown",
        }
    }
}

/// The dispatchable frontier with every in-progress row counted in flight.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Frontier {
    pub(crate) ready: Vec<u32>,
    pub(crate) held: Vec<Held>,
    pub(crate) next: Vec<u32>,
    pub(crate) blocked: Vec<Blocked>,
}

/// Dispatchable by dependency but claiming a file an in-flight row holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Held {
    pub(crate) id: u32,
    pub(crate) blocked_on_file: String,
    pub(crate) holder: u32,
}

/// Unreachable by any later wave while `blocker` keeps its status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Blocked {
    pub(crate) id: u32,
    pub(crate) blocker: u32,
    pub(crate) blocker_status: String,
}

/// `from` is the prerequisite and `to` the row that waits on it; an
/// `overlap` pair has no direction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Edge {
    pub(crate) kind: EdgeKind,
    pub(crate) from: u32,
    pub(crate) to: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EdgeKind {
    #[default]
    Needs,
    Coupling,
    /// Two rows sharing a file with no path either way.
    Overlap,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Checkpoint {
    pub(crate) id: String,
    pub(crate) rationale: String,
    pub(crate) members: Vec<u32>,
    /// Members no other member depends on.
    pub(crate) maximal: Vec<u32>,
    pub(crate) valid_cut: bool,
    pub(crate) commits: Vec<String>,
    pub(crate) verification: Option<Verification>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Verification {
    pub(crate) outcome: String,
    pub(crate) summary: String,
}

/// One execution-record entry. The fields are the union over every entry
/// type; each type fills only its own, and the rest read as empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RecordEntry {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) entry_type: String,
    pub(crate) date: String,
    pub(crate) agent: String,
    pub(crate) summary: String,
    pub(crate) task_ref: String,
    /// The row `task_ref` resolves to, `None` when it names none.
    pub(crate) task_id: Option<u32>,
    pub(crate) status: String,
    pub(crate) files: Vec<String>,
    pub(crate) commits: Vec<String>,
    pub(crate) dispatch_tier: String,
    pub(crate) dispatch_agent: String,
    pub(crate) escalation_reason: String,
    pub(crate) vet: String,
    pub(crate) retries: u32,
    pub(crate) command: String,
    pub(crate) outcome: String,
    pub(crate) duration_s: u64,
    pub(crate) failed_ids: Vec<String>,
    pub(crate) original_intent: String,
    pub(crate) rationale: String,
    pub(crate) supersedes_entry: String,
    pub(crate) reason: String,
    pub(crate) reevaluate_when: String,
    pub(crate) direction: String,
    pub(crate) findings_count: u32,
    pub(crate) commits_checked: Vec<String>,
    pub(crate) from_status: String,
    pub(crate) to_status: String,
    /// A `checkpoint` entry's kind, e.g. `commit-train`.
    pub(crate) kind: String,
    pub(crate) scope_delta: String,
    /// On `checkpoint` entries only: the checkpoints its commits landed for.
    pub(crate) checkpoint_ids: Vec<String>,
    pub(crate) dedup_id: String,
}

/// One row of the flow's hook-written `agents.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Agent {
    pub(crate) id: String,
    pub(crate) harness: String,
    pub(crate) session_id: String,
    pub(crate) agent_id: String,
    pub(crate) agent_type: String,
    pub(crate) kind: AgentKind,
    /// Teammate name; `""` for a subagent.
    pub(crate) name: String,
    pub(crate) team: String,
    pub(crate) status: AgentStatus,
    pub(crate) started_at: String,
    pub(crate) updated_at: String,
    /// `""` until the agent stops.
    pub(crate) ended_at: String,
    pub(crate) transcript_path: String,
    pub(crate) summary: String,
    pub(crate) context_tokens: u64,
    /// One per assignment; the last is open while its `ended_at` is `""`.
    pub(crate) segments: Vec<Segment>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AgentKind {
    #[default]
    Subagent,
    Teammate,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AgentStatus {
    #[default]
    Running,
    Idle,
    Stopped,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Segment {
    pub(crate) task_ids: Vec<u32>,
    pub(crate) started_at: String,
    pub(crate) ended_at: String,
}

/// Lookups over one [`Snapshot`], holding positions rather than borrows so an
/// owner can keep both side by side. Methods returning rows take the snapshot
/// the index was built from; given any other they may answer wrongly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Index {
    task_pos: HashMap<u32, usize>,
    layer: HashMap<u32, usize>,
    dependents: HashMap<u32, Vec<u32>>,
    coupled: HashMap<u32, Vec<u32>>,
    agents: HashMap<u32, Vec<usize>>,
    record: HashMap<u32, Vec<usize>>,
    checkpoint: HashMap<u32, usize>,
    topology: u64,
}

impl Snapshot {
    pub(crate) fn index(&self) -> Index {
        let task_pos = self
            .tasks
            .iter()
            .enumerate()
            .map(|(pos, task)| (task.id, pos))
            .collect();

        let layer = self
            .layers
            .iter()
            .enumerate()
            .flat_map(|(depth, ids)| ids.iter().map(move |id| (*id, depth)))
            .collect();

        let mut dependents: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut coupled: HashMap<u32, Vec<u32>> = HashMap::new();
        for task in &self.tasks {
            for need in &task.needs {
                dependents.entry(*need).or_default().push(task.id);
            }
            for peer in &task.coupling {
                coupled.entry(*peer).or_default().push(task.id);
            }
        }
        for ids in dependents.values_mut().chain(coupled.values_mut()) {
            ids.sort_unstable();
            ids.dedup();
        }

        // Newest first by the latest segment start on that task, so a pool
        // teammate re-tasked onto it outranks a subagent that finished earlier.
        let mut agent_hits: HashMap<u32, Vec<(&str, usize)>> = HashMap::new();
        for (pos, agent) in self.agents.iter().enumerate() {
            let mut latest: HashMap<u32, &str> = HashMap::new();
            for segment in &agent.segments {
                for id in &segment.task_ids {
                    let slot = latest.entry(*id).or_default();
                    if segment.started_at.as_str() >= *slot {
                        *slot = segment.started_at.as_str();
                    }
                }
            }
            for (id, started) in latest {
                agent_hits.entry(id).or_default().push((started, pos));
            }
        }
        let agents = agent_hits
            .into_iter()
            .map(|(id, mut hits)| {
                hits.sort_unstable_by(|a, b| b.cmp(a));
                (id, hits.into_iter().map(|(_, pos)| pos).collect())
            })
            .collect();

        let mut record: HashMap<u32, Vec<usize>> = HashMap::new();
        for (pos, entry) in self.record.iter().enumerate() {
            if let Some(id) = entry.task_id {
                record.entry(id).or_default().push(pos);
            }
        }

        let mut checkpoint = HashMap::new();
        for (pos, group) in self.checkpoints.iter().enumerate() {
            for id in &group.members {
                checkpoint.entry(*id).or_insert(pos);
            }
        }

        Index {
            task_pos,
            layer,
            dependents,
            coupled,
            agents,
            record,
            checkpoint,
            topology: topology_hash(&self.tasks),
        }
    }
}

impl Index {
    pub(crate) fn task<'s>(&self, snapshot: &'s Snapshot, id: u32) -> Option<&'s Task> {
        self.task_pos
            .get(&id)
            .and_then(|pos| snapshot.tasks.get(*pos))
    }

    /// Zero-based position of `id`'s layer in `layers`.
    pub(crate) fn layer_of(&self, id: u32) -> Option<usize> {
        self.layer.get(&id).copied()
    }

    /// Rows whose `needs` name `id`, ascending.
    pub(crate) fn dependents(&self, id: u32) -> &[u32] {
        self.dependents.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Rows whose `coupling` names `id`, ascending — the inverse of `id`'s own
    /// `coupling` list, as [`Index::dependents`] is of `needs`.
    pub(crate) fn coupled(&self, id: u32) -> &[u32] {
        self.coupled.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Agents with a segment on `id`, newest assignment first.
    pub(crate) fn agents_for<'s>(&self, snapshot: &'s Snapshot, id: u32) -> Vec<&'s Agent> {
        self.agents.get(&id).map_or_else(Vec::new, |positions| {
            positions
                .iter()
                .filter_map(|pos| snapshot.agents.get(*pos))
                .collect()
        })
    }

    /// Record entries whose `task_ref` resolved to `id`, in record order.
    pub(crate) fn record_for<'s>(&self, snapshot: &'s Snapshot, id: u32) -> Vec<&'s RecordEntry> {
        self.record.get(&id).map_or_else(Vec::new, |positions| {
            positions
                .iter()
                .filter_map(|pos| snapshot.record.get(*pos))
                .collect()
        })
    }

    /// The checkpoint group listing `id` as a member.
    pub(crate) fn checkpoint<'s>(&self, snapshot: &'s Snapshot, id: u32) -> Option<&'s Checkpoint> {
        self.checkpoint
            .get(&id)
            .and_then(|pos| snapshot.checkpoints.get(*pos))
    }

    /// Changes exactly when the id set or an edge does; status and every
    /// other field leave it alone, so a layout keyed on it survives them.
    pub(crate) fn topology_hash(&self) -> u64 {
        self.topology
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a rather than `DefaultHasher`, whose output may change between Rust
/// releases. Each list is length-prefixed so ids cannot slide between them.
fn topology_hash(tasks: &[Task]) -> u64 {
    fn feed(hash: &mut u64, value: u32) {
        for byte in value.to_le_bytes() {
            *hash ^= u64::from(byte);
            *hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    fn feed_list(hash: &mut u64, ids: &[u32]) {
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        feed(hash, u32::try_from(sorted.len()).unwrap_or(u32::MAX));
        for id in sorted {
            feed(hash, id);
        }
    }

    let mut ordered: Vec<&Task> = tasks.iter().collect();
    ordered.sort_by_key(|task| task.id);
    let mut hash = FNV_OFFSET;
    for task in ordered {
        feed(&mut hash, task.id);
        feed_list(&mut hash, &task.needs);
        feed_list(&mut hash, &task.coupling);
    }
    hash
}

#[cfg(test)]
pub(crate) fn fixture() -> Snapshot {
    serde_json::from_str(include_str!("../tests/fixtures/snapshot.json"))
        .expect("the snapshot fixture parses")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Every object key path in `value`, with array elements addressed by
    /// index so an entry-specific key is checked on the entry that carries it.
    fn key_paths(value: &Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    let path = format!("{prefix}.{key}");
                    out.push(path.clone());
                    key_paths(child, &path, out);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    key_paths(child, &format!("{prefix}[{index}]"), out);
                }
            }
            _ => {}
        }
    }

    fn raw_fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/snapshot.json"))
            .expect("fixture is JSON")
    }

    #[test]
    fn fixture_parses_into_the_documented_shape() {
        let snap = fixture();
        assert_eq!(snap.schema, 1);
        assert_eq!(snap.slug, "demo-flow");
        assert_eq!(snap.tasks.len(), 8);
        assert_eq!(snap.layers, vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8]]);
        assert_eq!(snap.tasks[3].status, TaskStatus::InProgress);
        assert_eq!(snap.frontier.held[0].holder, 4);
        assert_eq!(snap.edges.last().map(|e| e.kind), Some(EdgeKind::Overlap));
        assert_eq!(
            snap.checkpoints[0]
                .verification
                .as_ref()
                .map(|v| v.outcome.as_str()),
            Some("pass")
        );
        assert_eq!(snap.checkpoints[1].verification, None);
        assert_eq!(snap.record[6].checkpoint_ids, ["A"]);
        assert_eq!(snap.agents[0].kind, AgentKind::Teammate);
        assert_eq!(snap.agents[1].status, AgentStatus::Running);
    }

    #[test]
    fn every_fixture_key_survives_a_round_trip() {
        let raw = raw_fixture();
        let reserialised = serde_json::to_value(fixture()).expect("serialises");
        let mut before = Vec::new();
        key_paths(&raw, "", &mut before);
        let mut after = Vec::new();
        key_paths(&reserialised, "", &mut after);
        let missing: Vec<&String> = before.iter().filter(|path| !after.contains(path)).collect();
        assert!(missing.is_empty(), "dropped on round trip: {missing:?}");
    }

    #[test]
    fn unknown_keys_and_values_are_ignored() {
        let mut raw = raw_fixture();
        raw["future_top_level"] = serde_json::json!({ "x": 1 });
        raw["tasks"][0]["future_field"] = serde_json::json!(7);
        raw["tasks"][1]["status"] = serde_json::json!("paused");
        raw["agents"][0]["kind"] = serde_json::json!("remote");
        let snap: Snapshot = serde_json::from_value(raw).expect("unknown keys are ignored");
        assert_eq!(snap.tasks.len(), 8);
        assert_eq!(snap.tasks[1].status, TaskStatus::Unknown);
        assert_eq!(snap.agents[0].kind, AgentKind::Unknown);
    }

    #[test]
    fn missing_fields_read_as_empty() {
        let snap: Snapshot =
            serde_json::from_str(r#"{"tasks":[{"id":3}]}"#).expect("sparse snapshot parses");
        assert_eq!(snap.tasks[0].id, 3);
        assert_eq!(snap.tasks[0].status, TaskStatus::Pending);
        assert!(snap.tasks[0].needs.is_empty());
        assert!(snap.agents.is_empty());
    }

    #[test]
    fn topology_hash_is_stable_under_a_status_change() {
        let snap = fixture();
        let before = snap.index().topology_hash();
        let mut changed = snap.clone();
        changed.tasks[4].status = TaskStatus::Done;
        changed.tasks[4].agent = "implement-lite".to_string();
        changed.revision = "0000000000000000".to_string();
        assert_eq!(changed.index().topology_hash(), before);
    }

    #[test]
    fn topology_hash_changes_when_an_edge_is_added() {
        let snap = fixture();
        let before = snap.index().topology_hash();

        let mut needs = snap.clone();
        needs.tasks[7].needs.push(2);
        assert_ne!(needs.index().topology_hash(), before);

        let mut coupling = snap.clone();
        coupling.tasks[6].coupling.push(3);
        assert_ne!(coupling.index().topology_hash(), before);

        let mut moved = snap.clone();
        let edge = moved.tasks[7].needs.pop().expect("task 8 needs 6");
        moved.tasks[7].coupling.push(edge);
        assert_ne!(
            moved.index().topology_hash(),
            before,
            "turning a needs edge into a coupling edge is a topology change"
        );
    }

    #[test]
    fn index_answers_each_lookup() {
        let snap = fixture();
        let index = snap.index();
        assert_eq!(
            index.task(&snap, 6).map(|t| t.title.as_str()),
            Some("Bind the keys")
        );
        assert!(index.task(&snap, 99).is_none());
        assert_eq!(index.layer_of(1), Some(0));
        assert_eq!(index.layer_of(6), Some(1));
        assert_eq!(index.layer_of(8), Some(2));
        assert_eq!(index.dependents(1), [4, 5]);
        assert_eq!(index.dependents(8), [] as [u32; 0]);
        assert_eq!(index.coupled(3), [6]);
        assert_eq!(index.coupled(5), [8]);
        assert_eq!(index.checkpoint(&snap, 7).map(|c| c.id.as_str()), Some("B"));

        let record: Vec<&str> = index
            .record_for(&snap, 3)
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(record, ["E4", "E5", "E6"]);
        assert!(index.record_for(&snap, 8).is_empty());

        let on_four: Vec<&str> = index
            .agents_for(&snap, 4)
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(on_four, ["A2"]);
        let on_three: Vec<&str> = index
            .agents_for(&snap, 3)
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(on_three, ["A1"]);
    }

    #[test]
    fn agents_for_orders_newest_assignment_first() {
        let mut snap = fixture();
        // The subagent row (later in the store) also worked task 3, but before
        // the teammate's second segment started.
        snap.agents[1].segments.insert(
            0,
            Segment {
                task_ids: vec![3],
                started_at: "2026-09-28T09:00:00Z".to_string(),
                ended_at: "2026-09-28T09:50:00Z".to_string(),
            },
        );
        let ids: Vec<&str> = snap
            .index()
            .agents_for(&snap, 3)
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(ids, ["A1", "A2"]);
    }
}
