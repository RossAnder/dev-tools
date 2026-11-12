//! The v2 records a snapshot carries — a task's spec and links, the milestones, the
//! change trail, the plan store and an agent's token usage — and the [`PlanIndex`]
//! the views look them up through.
//!
//! Every struct reads with `#[serde(default)]`, as the types beside [`Snapshot`] do,
//! so a snapshot from a tomlctl that emits none of these keys loads with them empty.
//! Vocabulary fields stay `String`: tomlctl's writers decide what is in vocabulary,
//! and a value this build does not know is shown rather than refused. Timestamps are
//! the strings tomlctl renders them as.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::Snapshot;

/// A task's spec, present only when the task store holds one for the row's `ref`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Spec {
    pub(crate) outcome: String,
    pub(crate) constraints: Vec<String>,
    pub(crate) means: Vec<Means>,
    pub(crate) executor: String,
    pub(crate) design: Vec<String>,
    pub(crate) precedent: Vec<String>,
    pub(crate) accept: Vec<Accept>,
    pub(crate) premises: Vec<Premise>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Means {
    pub(crate) text: String,
    /// `required` or `suggested`.
    pub(crate) binding: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Accept {
    pub(crate) behaviour: String,
    pub(crate) check: String,
    pub(crate) kind: String,
    pub(crate) positive: String,
    pub(crate) writes_build: bool,
    pub(crate) run_by: String,
    pub(crate) covered_by: String,
    /// Who runs the check, as tomlctl derives it; never re-derived here.
    pub(crate) owner: String,
    /// `false` exactly when the probe emitter skips the entry.
    pub(crate) probeable: bool,
    pub(crate) probe: Option<Probe>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Premise {
    pub(crate) claim: String,
    pub(crate) check: String,
    /// `base` or `after:<id>`.
    pub(crate) at: String,
    pub(crate) probeable: bool,
    pub(crate) probe: Option<Probe>,
}

/// A recorded probe. An rc is `None` for a control that did not run; a premise's
/// `pos_rc` is always `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Probe {
    pub(crate) at: String,
    pub(crate) neg_rc: Option<i64>,
    pub(crate) pos_rc: Option<i64>,
    pub(crate) verdict: String,
}

/// The review findings a task fixes or settles, in the review store at `store`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct FindingLink {
    pub(crate) store: String,
    pub(crate) fixes: Vec<String>,
    pub(crate) settles: Vec<String>,
}

/// One `[[milestones]]` row of the task store, keyed by its checkpoint id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Milestone {
    pub(crate) id: String,
    pub(crate) gates: Vec<String>,
    pub(crate) elaboration: String,
    pub(crate) defer_reason: String,
    pub(crate) gate_probes: Vec<GateProbe>,
}

/// `verdict` is `""` until the probe is recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GateProbe {
    pub(crate) gate: String,
    pub(crate) probe: String,
    pub(crate) verdict: String,
    pub(crate) at: String,
}

/// One entry of the task store's change trail; `at` is `""` when it was stored without one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Change {
    pub(crate) at: String,
    #[serde(rename = "ref")]
    pub(crate) task_ref: String,
    pub(crate) field: String,
    pub(crate) old: String,
    pub(crate) new: String,
    pub(crate) actor: String,
    pub(crate) reason: String,
}

/// An agent's token totals over its transcript, all zero until it stops.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Usage {
    pub(crate) input: u64,
    pub(crate) cache_write: u64,
    pub(crate) cache_read: u64,
    pub(crate) output: u64,
    pub(crate) turns: u64,
}

/// The flow's `plan.toml`, under the keys the store itself uses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Plan {
    pub(crate) schema_version: i64,
    pub(crate) last_updated: String,
    pub(crate) binding: Binding,
    pub(crate) id_high_water: BTreeMap<String, u64>,
    #[serde(rename = "decision")]
    pub(crate) decisions: Vec<Decision>,
    #[serde(rename = "contract")]
    pub(crate) contracts: Vec<Contract>,
    #[serde(rename = "approval")]
    pub(crate) approvals: Vec<Approval>,
    #[serde(rename = "run")]
    pub(crate) runs: Vec<Run>,
    #[serde(rename = "handover")]
    pub(crate) handovers: Vec<Handover>,
    #[serde(rename = "attempt")]
    pub(crate) attempts: Vec<Attempt>,
    #[serde(rename = "gate_round")]
    pub(crate) gate_rounds: Vec<GateRound>,
    #[serde(rename = "report")]
    pub(crate) reports: Vec<Report>,
}

impl Plan {
    /// The report the last invocation left; reports are appended, one per invocation.
    pub(crate) fn latest_report(&self) -> Option<&Report> {
        self.reports.last()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Binding {
    pub(crate) branch: String,
    pub(crate) scope: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Decision {
    pub(crate) id: String,
    pub(crate) decision: String,
    pub(crate) why: String,
    pub(crate) provenance: String,
    pub(crate) door: String,
    /// An all-digit entry names a task id; any other is a path.
    pub(crate) binds: Vec<String>,
    pub(crate) rejected: Vec<Rejected>,
    pub(crate) evidence: String,
    pub(crate) if_false: String,
    pub(crate) revisit_if: String,
    pub(crate) check: String,
    pub(crate) surfaced: bool,
    pub(crate) vetoed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Rejected {
    pub(crate) option: String,
    pub(crate) because: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Contract {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) claim: String,
    pub(crate) check: String,
    pub(crate) tasks: Vec<u32>,
    pub(crate) provides: String,
    pub(crate) consumes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Approval {
    pub(crate) commit: String,
    pub(crate) at: String,
    pub(crate) brief_sha256: String,
    pub(crate) by: String,
    pub(crate) review: String,
    pub(crate) reason: String,
}

/// One launched or skipped flow step; `finished` is `""` while it is open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Run {
    pub(crate) step: String,
    pub(crate) workflow: String,
    pub(crate) args_hash: String,
    pub(crate) run_id: String,
    pub(crate) session: String,
    pub(crate) started: String,
    pub(crate) finished: String,
    pub(crate) skip_reason: String,
    pub(crate) outcome: String,
    pub(crate) reason: String,
    pub(crate) tasks: Vec<u32>,
    pub(crate) store_digest: BTreeMap<String, String>,
    pub(crate) digest_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Handover {
    pub(crate) id: String,
    pub(crate) reason: String,
    pub(crate) action: String,
    pub(crate) when: String,
    pub(crate) done_when: String,
    pub(crate) source_task: u32,
    pub(crate) done: bool,
}

/// One worker's run note, recorded from the note the worker wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Attempt {
    pub(crate) task: u32,
    pub(crate) run_id: String,
    pub(crate) milestone: String,
    pub(crate) status: String,
    pub(crate) summary: String,
    /// `present`, `absent` or `malformed`: whether the note's structured block was read.
    pub(crate) block: String,
    pub(crate) files_touched: Vec<String>,
    pub(crate) deviations: Vec<Deviation>,
    pub(crate) tangential: Vec<Tangential>,
    pub(crate) checks: Vec<CheckRun>,
    pub(crate) handover: Option<ProposedHandover>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Deviation {
    pub(crate) what: String,
    pub(crate) why: String,
    /// The suggested means departed from, `""` when none.
    pub(crate) means: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Tangential {
    pub(crate) kind: String,
    pub(crate) area: String,
    pub(crate) summary: String,
    pub(crate) why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct CheckRun {
    pub(crate) command: String,
    /// `pass`, `fail` or `not-run`.
    pub(crate) result: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ProposedHandover {
    pub(crate) reason: String,
    pub(crate) action: String,
    pub(crate) when: String,
    pub(crate) done_when: String,
}

/// One run of a milestone's gate, keyed on `(milestone, round, seq)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GateRound {
    pub(crate) milestone: String,
    /// `0` for the first gate run, `n` for the run after fix round `n`.
    pub(crate) round: u32,
    pub(crate) seq: u32,
    pub(crate) at: String,
    pub(crate) outcome: String,
    pub(crate) commands: Vec<GateCommand>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GateCommand {
    pub(crate) command: String,
    /// `pass`, `flaky`, `fail` or `timeout`.
    pub(crate) outcome: String,
    pub(crate) exit: Option<i64>,
    pub(crate) duration_s: u64,
    pub(crate) summary: String,
    pub(crate) failed_ids: Vec<String>,
    /// The kept log's repo-relative path, `""` when none was kept.
    pub(crate) log: String,
}

/// What one invocation left for a person.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Report {
    pub(crate) at: String,
    pub(crate) milestone: String,
    pub(crate) why: String,
    pub(crate) entries: Vec<ReportEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ReportEntry {
    pub(crate) kind: String,
    /// `0` when the entry names no task.
    pub(crate) task: u32,
    pub(crate) text: String,
    pub(crate) path: String,
    /// `false` for a line that only informs; an absent key reads as `true`.
    pub(crate) needs_person: bool,
}

impl Default for ReportEntry {
    fn default() -> Self {
        Self {
            kind: String::new(),
            task: 0,
            text: String::new(),
            path: String::new(),
            needs_person: true,
        }
    }
}

/// Where a probe that needs attention sits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum HealthSite {
    /// `index` into the task's `spec.accept`.
    Accept { task: u32, index: usize },
    /// `index` into the task's `spec.premises`.
    Premise { task: u32, index: usize },
    /// `index` into the milestone's `gate_probes`.
    GateProbe { milestone: String, index: usize },
}

/// A probe that is due and not good: a probeable accept not recorded `healthy`, a
/// probeable premise or a gate probe not recorded `holds`. `verdict` is `""` when
/// the probe was never recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Health {
    pub(crate) site: HealthSite,
    pub(crate) verdict: String,
}

/// Lookups over a snapshot's v2 records, holding positions as [`super::Index`]
/// does; methods returning records take the snapshot it was built from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PlanIndex {
    decisions: HashMap<u32, Vec<usize>>,
    contracts: HashMap<u32, Vec<usize>>,
    handovers: HashMap<u32, Vec<usize>>,
    attempts: HashMap<u32, Vec<usize>>,
    gate_rounds: HashMap<String, Vec<usize>>,
    changes: HashMap<String, Vec<usize>>,
    health: Vec<Health>,
}

/// The task id an all-digit `binds` entry names.
fn bound_task(entry: &str) -> Option<u32> {
    if entry.is_empty() || !entry.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    entry.parse().ok()
}

fn verdict_of(probe: Option<&Probe>) -> &str {
    probe.map_or("", |probe| probe.verdict.as_str())
}

fn push_once(map: &mut HashMap<u32, Vec<usize>>, id: u32, pos: usize) {
    let slot = map.entry(id).or_default();
    if slot.last() != Some(&pos) {
        slot.push(pos);
    }
}

impl PlanIndex {
    pub(crate) fn new(snapshot: &Snapshot) -> PlanIndex {
        let mut index = PlanIndex::default();

        if let Some(plan) = &snapshot.plan {
            for (pos, decision) in plan.decisions.iter().enumerate() {
                for id in decision.binds.iter().filter_map(|b| bound_task(b)) {
                    push_once(&mut index.decisions, id, pos);
                }
            }
            for (pos, contract) in plan.contracts.iter().enumerate() {
                for id in &contract.tasks {
                    push_once(&mut index.contracts, *id, pos);
                }
            }
            for (pos, handover) in plan.handovers.iter().enumerate() {
                push_once(&mut index.handovers, handover.source_task, pos);
            }
            for (pos, attempt) in plan.attempts.iter().enumerate() {
                push_once(&mut index.attempts, attempt.task, pos);
            }
            for (pos, round) in plan.gate_rounds.iter().enumerate() {
                index
                    .gate_rounds
                    .entry(round.milestone.clone())
                    .or_default()
                    .push(pos);
            }
            for positions in index.gate_rounds.values_mut() {
                positions.sort_by_key(|pos| {
                    let round = &plan.gate_rounds[*pos];
                    (round.round, round.seq)
                });
            }
        }

        for (pos, change) in snapshot.changes.iter().enumerate() {
            index
                .changes
                .entry(change.task_ref.clone())
                .or_default()
                .push(pos);
        }

        for task in &snapshot.tasks {
            let Some(spec) = &task.spec else { continue };
            for (i, accept) in spec.accept.iter().enumerate() {
                let verdict = verdict_of(accept.probe.as_ref());
                if accept.probeable && verdict != "healthy" {
                    index.health.push(Health {
                        site: HealthSite::Accept {
                            task: task.id,
                            index: i,
                        },
                        verdict: verdict.to_string(),
                    });
                }
            }
            for (i, premise) in spec.premises.iter().enumerate() {
                let verdict = verdict_of(premise.probe.as_ref());
                if premise.probeable && verdict != "holds" {
                    index.health.push(Health {
                        site: HealthSite::Premise {
                            task: task.id,
                            index: i,
                        },
                        verdict: verdict.to_string(),
                    });
                }
            }
        }
        for milestone in &snapshot.milestones {
            for (i, probe) in milestone.gate_probes.iter().enumerate() {
                if probe.verdict != "holds" {
                    index.health.push(Health {
                        site: HealthSite::GateProbe {
                            milestone: milestone.id.clone(),
                            index: i,
                        },
                        verdict: probe.verdict.clone(),
                    });
                }
            }
        }

        index
    }

    /// Decisions whose `binds` name task `id`, in store order.
    pub(crate) fn decisions_for<'s>(&self, snapshot: &'s Snapshot, id: u32) -> Vec<&'s Decision> {
        let records = snapshot.plan.as_ref().map_or(&[][..], |p| &p.decisions);
        pick(&self.decisions, &id, records)
    }

    /// Contracts whose `tasks` list `id`, in store order.
    pub(crate) fn contracts_for<'s>(&self, snapshot: &'s Snapshot, id: u32) -> Vec<&'s Contract> {
        let records = snapshot.plan.as_ref().map_or(&[][..], |p| &p.contracts);
        pick(&self.contracts, &id, records)
    }

    /// Handovers whose `source_task` is `id`, in store order.
    pub(crate) fn handovers_for<'s>(&self, snapshot: &'s Snapshot, id: u32) -> Vec<&'s Handover> {
        let records = snapshot.plan.as_ref().map_or(&[][..], |p| &p.handovers);
        pick(&self.handovers, &id, records)
    }

    /// Recorded run notes for task `id`, in store order.
    pub(crate) fn attempts_for<'s>(&self, snapshot: &'s Snapshot, id: u32) -> Vec<&'s Attempt> {
        let records = snapshot.plan.as_ref().map_or(&[][..], |p| &p.attempts);
        pick(&self.attempts, &id, records)
    }

    /// Gate runs of `milestone`, by round and then seq.
    pub(crate) fn gate_rounds_for<'s>(
        &self,
        snapshot: &'s Snapshot,
        milestone: &str,
    ) -> Vec<&'s GateRound> {
        let records = snapshot.plan.as_ref().map_or(&[][..], |p| &p.gate_rounds);
        pick(&self.gate_rounds, milestone, records)
    }

    /// Change-trail entries for the task `task_ref`, in trail order.
    pub(crate) fn changes_for<'s>(
        &self,
        snapshot: &'s Snapshot,
        task_ref: &str,
    ) -> Vec<&'s Change> {
        pick(&self.changes, task_ref, &snapshot.changes)
    }

    /// Every probe needing attention: tasks in snapshot order with each task's
    /// accept entries before its premises, then each milestone's gate probes.
    pub(crate) fn health(&self) -> &[Health] {
        &self.health
    }
}

fn pick<'s, K, Q, T>(map: &HashMap<K, Vec<usize>>, key: &Q, records: &'s [T]) -> Vec<&'s T>
where
    K: std::borrow::Borrow<Q> + std::hash::Hash + Eq,
    Q: std::hash::Hash + Eq + ?Sized,
{
    map.get(key).map_or_else(Vec::new, |positions| {
        positions
            .iter()
            .filter_map(|pos| records.get(*pos))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{TaskStatus, fixture};

    #[test]
    fn the_fixture_holds_a_complete_plan() {
        let snap = fixture();
        let plan = snap.plan.as_ref().expect("the fixture carries a plan");
        assert_eq!(plan.schema_version, 3);
        assert_eq!(plan.decisions[0].rejected.len(), 1);
        assert_eq!(plan.contracts[0].tasks, [3, 6]);
        assert_eq!(snap.milestones.len(), 2);
        assert!(plan.handovers.iter().any(|h| h.done));
        assert!(plan.handovers.iter().any(|h| !h.done));
        assert_eq!(plan.attempts[0].deviations.len(), 1);
        let failing = plan.gate_rounds.iter().flat_map(|r| &r.commands);
        assert!(
            failing
                .clone()
                .any(|c| c.outcome == "fail" && c.exit.is_some())
        );
        assert!(snap.tasks.iter().any(|t| t.status == TaskStatus::Parked));
        assert!(snap.agents.iter().any(|a| a.usage.turns > 0));
    }

    #[test]
    fn only_the_latest_report_names_the_parked_task() {
        let snap = fixture();
        let plan = snap.plan.as_ref().expect("plan");
        assert_eq!(plan.reports.len(), 2);
        let latest = plan.latest_report().expect("a report");
        let gate = latest
            .entries
            .iter()
            .find(|e| e.kind == "gate")
            .expect("a gate entry");
        assert!(!gate.path.is_empty());
        let parked = latest
            .entries
            .iter()
            .find(|e| e.kind == "parked")
            .expect("a parked entry");
        let task = snap
            .tasks
            .iter()
            .find(|t| t.id == parked.task)
            .expect("row");
        assert_eq!(task.status, TaskStatus::Parked);
        assert!(plan.reports[0].entries.iter().all(|e| e.kind != "parked"));
    }

    #[test]
    fn records_are_found_by_task_milestone_and_ref() {
        let snap = fixture();
        let index = snap.index();
        let plan = index.plan();
        let ids = |decisions: Vec<&Decision>| -> Vec<String> {
            decisions.iter().map(|d| d.id.clone()).collect()
        };
        assert_eq!(ids(plan.decisions_for(&snap, 6)), ["D1"]);
        assert!(
            plan.decisions_for(&snap, 5).is_empty(),
            "a path entry names no task"
        );
        assert_eq!(plan.contracts_for(&snap, 3)[0].id, "C1");
        assert_eq!(plan.handovers_for(&snap, 3)[0].id, "H1");
        assert_eq!(plan.handovers_for(&snap, 2)[0].id, "H2");
        assert_eq!(plan.attempts_for(&snap, 3).len(), 1);
        let parked: Vec<&str> = plan
            .attempts_for(&snap, 8)
            .iter()
            .map(|a| a.status.as_str())
            .collect();
        assert_eq!(parked, ["parked"]);
        assert!(plan.attempts_for(&snap, 2).is_empty());
        let rounds: Vec<(u32, u32)> = plan
            .gate_rounds_for(&snap, "A")
            .iter()
            .map(|r| (r.round, r.seq))
            .collect();
        assert_eq!(rounds, [(0, 1), (1, 1)]);
        assert_eq!(plan.gate_rounds_for(&snap, "B").len(), 1);
        assert!(plan.gate_rounds_for(&snap, "C").is_empty());
        assert_eq!(plan.changes_for(&snap, "bind-the-keys").len(), 1);
        assert!(plan.changes_for(&snap, "no-such-ref").is_empty());
    }

    #[test]
    fn health_lists_each_due_probe_that_is_not_good() {
        let snap = fixture();
        let index = snap.index();
        let sites: Vec<&HealthSite> = index.plan().health().iter().map(|h| &h.site).collect();
        assert_eq!(
            sites,
            [
                &HealthSite::Accept { task: 6, index: 1 },
                &HealthSite::Premise { task: 6, index: 0 },
                &HealthSite::GateProbe {
                    milestone: "B".to_string(),
                    index: 0
                },
            ]
        );
        let verdicts: Vec<&str> = index
            .plan()
            .health()
            .iter()
            .map(|h| h.verdict.as_str())
            .collect();
        assert_eq!(verdicts, ["vacuous", "", ""]);
    }

    #[test]
    fn a_snapshot_without_the_new_keys_loads_empty() {
        let snap: Snapshot =
            serde_json::from_str(r#"{"tasks":[{"id":3}],"agents":[{"id":"A1"}]}"#).expect("parses");
        assert!(snap.plan.is_none());
        assert!(snap.milestones.is_empty() && snap.changes.is_empty());
        let task = &snap.tasks[0];
        assert!(task.spec.is_none() && task.finding.is_none());
        assert!(task.file_notes.is_empty() && task.closes.is_empty() && task.refs.is_empty());
        assert_eq!(snap.agents[0].usage, Usage::default());
        let index = snap.index();
        assert!(index.plan().health().is_empty());
        assert!(index.plan().decisions_for(&snap, 3).is_empty());
    }

    #[test]
    fn an_absent_needs_person_reads_as_true() {
        let entry: ReportEntry = serde_json::from_str(r#"{"kind":"gate"}"#).expect("parses");
        assert!(entry.needs_person);
        let entry: ReportEntry =
            serde_json::from_str(r#"{"kind":"gate","needs_person":false}"#).expect("parses");
        assert!(!entry.needs_person);
    }

    #[test]
    fn only_an_all_digit_binds_entry_names_a_task() {
        assert_eq!(bound_task("12"), Some(12));
        assert_eq!(bound_task("src/a.rs"), None);
        assert_eq!(bound_task(""), None);
        assert_eq!(bound_task("+3"), None);
    }
}
