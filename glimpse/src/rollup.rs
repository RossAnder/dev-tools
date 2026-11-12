//! Time and token totals.
//!
//! [`rollup`] totals a flow's agents from the snapshot alone. A `plan-worker-`
//! agent's segments credit the tasks they name, and through each task its
//! milestone (the task's `checkpoint`) and its spec's `executor`; a segment
//! naming several tasks splits its time and its share of the agent's tokens
//! evenly among them, the agent's tokens first splitting evenly among its
//! segments. Every other agent, and any worker time no known task absorbs,
//! lands in one `other` row per agent type. An agent whose `usage` is still
//! all zero has no recorded tokens, and a total including one is partial.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the plan surface reads these rows")
)]

use std::collections::BTreeMap;

use crate::hook::parse_utc;
use crate::model::Snapshot;
use crate::model::plan::Usage;

const WORKER_PREFIX: &str = "plan-worker-";

/// What one row totals.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RowKind {
    /// A milestone id; `""` collects tasks with no checkpoint.
    Milestone(String),
    /// An executor; `""` collects tasks whose spec names none, or that have no spec.
    Executor(String),
    /// An agent type outside any task.
    Other(String),
}

/// Whether a row's tokens cover every agent it counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tokens {
    Recorded,
    /// Some counted agents have no usage yet; the totals cover the rest.
    Partial,
    /// No counted agent has usage; the totals are not zero but unknown.
    NotRecorded,
}

/// One agent's part of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Share {
    pub(crate) agent_id: String,
    pub(crate) agent_type: String,
    pub(crate) seconds: u64,
    pub(crate) usage: Usage,
    pub(crate) recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) kind: RowKind,
    /// The agents counted, in snapshot order.
    pub(crate) agents: Vec<Share>,
    pub(crate) seconds: u64,
    /// Summed over the agents with recorded usage only.
    pub(crate) usage: Usage,
    pub(crate) tokens: Tokens,
    /// The milestone's gate command durations; `0` on every other kind.
    pub(crate) gate_seconds: u64,
}

#[derive(Default)]
struct Acc {
    shares: BTreeMap<usize, Share>,
    gate_seconds: u64,
}

/// The rows by milestone (in checkpoint order), then by executor, then the
/// `other` rows, each group's remainder by name. `now` is seconds since the
/// Unix epoch and closes every open segment and running agent.
pub(crate) fn rollup(snapshot: &Snapshot, now: u64) -> Vec<Row> {
    let mut rows: BTreeMap<RowKind, Acc> = BTreeMap::new();

    for (pos, agent) in snapshot.agents.iter().enumerate() {
        let mut credit = |kind: RowKind, seconds: u64, usage: &Usage| {
            let share = rows
                .entry(kind)
                .or_default()
                .shares
                .entry(pos)
                .or_insert_with(|| Share {
                    agent_id: agent.id.clone(),
                    agent_type: agent.agent_type.clone(),
                    seconds: 0,
                    usage: Usage::default(),
                    recorded: agent.usage != Usage::default(),
                });
            share.seconds += seconds;
            add(&mut share.usage, usage);
        };
        let other = RowKind::Other(agent.agent_type.clone());

        if !agent.agent_type.starts_with(WORKER_PREFIX) || agent.segments.is_empty() {
            credit(
                other,
                span(&agent.started_at, &agent.ended_at, now),
                &agent.usage,
            );
            continue;
        }

        let segments = agent.segments.len();
        for (s, segment) in agent.segments.iter().enumerate() {
            let seconds = span(&segment.started_at, &segment.ended_at, now);
            let usage = split_usage(&agent.usage, segments, s);
            let tasks = segment.task_ids.len();
            if tasks == 0 {
                credit(other.clone(), seconds, &usage);
                continue;
            }
            for (t, id) in segment.task_ids.iter().enumerate() {
                let (seconds, usage) = (split(seconds, tasks, t), split_usage(&usage, tasks, t));
                match snapshot.tasks.iter().find(|task| task.id == *id) {
                    Some(task) => {
                        let executor = task
                            .spec
                            .as_ref()
                            .map_or_else(String::new, |spec| spec.executor.clone());
                        credit(RowKind::Milestone(task.checkpoint.clone()), seconds, &usage);
                        credit(RowKind::Executor(executor), seconds, &usage);
                    }
                    None => credit(other.clone(), seconds, &usage),
                }
            }
        }
    }

    if let Some(plan) = &snapshot.plan {
        for round in &plan.gate_rounds {
            let seconds: u64 = round.commands.iter().map(|c| c.duration_s).sum();
            rows.entry(RowKind::Milestone(round.milestone.clone()))
                .or_default()
                .gate_seconds += seconds;
        }
    }

    let mut out: Vec<Row> = rows
        .into_iter()
        .map(|(kind, acc)| finish(kind, acc))
        .collect();
    let checkpoint_pos = |id: &str| {
        snapshot
            .checkpoints
            .iter()
            .position(|c| c.id == id)
            .unwrap_or(usize::MAX)
    };
    // Stable: the map's name order breaks ties within each group.
    out.sort_by_key(|row| match &row.kind {
        RowKind::Milestone(id) => (0, checkpoint_pos(id)),
        RowKind::Executor(_) => (1, 0),
        RowKind::Other(_) => (2, 0),
    });
    out
}

fn finish(kind: RowKind, acc: Acc) -> Row {
    let agents: Vec<Share> = acc.shares.into_values().collect();
    let mut usage = Usage::default();
    let mut seconds = 0;
    for share in &agents {
        seconds += share.seconds;
        if share.recorded {
            add(&mut usage, &share.usage);
        }
    }
    let recorded = agents.iter().filter(|s| s.recorded).count();
    let tokens = if recorded == agents.len() && recorded > 0 {
        Tokens::Recorded
    } else if recorded > 0 {
        Tokens::Partial
    } else {
        Tokens::NotRecorded
    };
    Row {
        kind,
        agents,
        seconds,
        usage,
        tokens,
        gate_seconds: acc.gate_seconds,
    }
}

/// Seconds from `start` to `end`, or to `now` while `end` is `""`; `0` when
/// either end does not parse or the span runs backwards.
fn span(start: &str, end: &str, now: u64) -> u64 {
    let Some(start) = parse_utc(start) else {
        return 0;
    };
    let end = if end.is_empty() {
        Some(now)
    } else {
        parse_utc(end)
    };
    end.map_or(0, |end| end.saturating_sub(start))
}

/// Part `i` of `total` split `n` ways, the remainder going to the first parts
/// so the parts sum to `total`.
fn split(total: u64, n: usize, i: usize) -> u64 {
    let n = n as u64;
    let i = i as u64;
    total / n + u64::from(i < total % n)
}

fn split_usage(usage: &Usage, n: usize, i: usize) -> Usage {
    Usage {
        input: split(usage.input, n, i),
        cache_write: split(usage.cache_write, n, i),
        cache_read: split(usage.cache_read, n, i),
        output: split(usage.output, n, i),
        turns: split(usage.turns, n, i),
    }
}

fn add(into: &mut Usage, from: &Usage) {
    into.input += from.input;
    into.cache_write += from.cache_write;
    into.cache_read += from.cache_read;
    into.output += from.output;
    into.turns += from.turns;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::plan::{GateCommand, GateRound, Plan, Spec};
    use crate::model::{Agent, Checkpoint, Segment, Task};

    const NOW: &str = "2026-11-12T12:00:00Z";

    fn now() -> u64 {
        parse_utc(NOW).unwrap()
    }

    fn task(id: u32, checkpoint: &str, executor: &str) -> Task {
        Task {
            id,
            checkpoint: checkpoint.to_string(),
            spec: Some(Spec {
                executor: executor.to_string(),
                ..Spec::default()
            }),
            ..Task::default()
        }
    }

    fn usage(input: u64, output: u64) -> Usage {
        Usage {
            input,
            cache_write: 10,
            cache_read: 20,
            output,
            turns: 4,
        }
    }

    fn agent(id: &str, agent_type: &str, segments: Vec<Segment>, usage: Usage) -> Agent {
        Agent {
            id: id.to_string(),
            agent_type: agent_type.to_string(),
            started_at: "2026-11-12T10:00:00Z".to_string(),
            ended_at: "2026-11-12T11:00:00Z".to_string(),
            usage,
            segments,
            ..Agent::default()
        }
    }

    fn segment(ids: &[u32], start: &str, end: &str) -> Segment {
        Segment {
            task_ids: ids.to_vec(),
            started_at: start.to_string(),
            ended_at: end.to_string(),
            ..Segment::default()
        }
    }

    fn snapshot(tasks: Vec<Task>, agents: Vec<Agent>) -> Snapshot {
        Snapshot {
            tasks,
            agents,
            checkpoints: ["A", "B"]
                .iter()
                .map(|id| Checkpoint {
                    id: (*id).to_string(),
                    ..Checkpoint::default()
                })
                .collect(),
            ..Snapshot::default()
        }
    }

    fn row<'r>(rows: &'r [Row], kind: &RowKind) -> &'r Row {
        rows.iter()
            .find(|row| &row.kind == kind)
            .unwrap_or_else(|| panic!("no {kind:?} row in {rows:?}"))
    }

    fn milestone(id: &str) -> RowKind {
        RowKind::Milestone(id.to_string())
    }

    fn executor(id: &str) -> RowKind {
        RowKind::Executor(id.to_string())
    }

    #[test]
    fn a_worker_on_one_task_credits_its_milestone_and_executor() {
        let snap = snapshot(
            vec![task(1, "A", "deep")],
            vec![agent(
                "a1",
                "plan-worker-deep",
                vec![segment(
                    &[1],
                    "2026-11-12T10:00:00Z",
                    "2026-11-12T10:30:00Z",
                )],
                usage(100, 50),
            )],
        );
        let rows = rollup(&snap, now());
        assert_eq!(rows.len(), 2);
        for kind in [milestone("A"), executor("deep")] {
            let r = row(&rows, &kind);
            assert_eq!(r.seconds, 1_800);
            assert_eq!(r.usage, usage(100, 50));
            assert_eq!(r.tokens, Tokens::Recorded);
            assert_eq!(r.agents.len(), 1);
            assert_eq!(r.agents[0].agent_id, "a1");
        }
        assert_eq!(rows[0].kind, milestone("A"));
        assert_eq!(rows[1].kind, executor("deep"));
    }

    #[test]
    fn a_segment_naming_tasks_in_two_milestones_splits_evenly() {
        let snap = snapshot(
            vec![task(1, "A", "lite"), task(2, "B", "deep")],
            vec![agent(
                "a1",
                "plan-worker-lite",
                vec![segment(
                    &[1, 2],
                    "2026-11-12T10:00:00Z",
                    "2026-11-12T10:20:00Z",
                )],
                usage(101, 40),
            )],
        );
        let rows = rollup(&snap, now());
        let (a, b) = (row(&rows, &milestone("A")), row(&rows, &milestone("B")));
        assert_eq!((a.seconds, b.seconds), (600, 600));
        assert_eq!((a.usage.input, b.usage.input), (51, 50));
        assert_eq!((a.usage.output, b.usage.output), (20, 20));
        assert_eq!((a.usage.turns, b.usage.turns), (2, 2));
        assert_eq!(row(&rows, &executor("lite")).seconds, 600);
        assert_eq!(row(&rows, &executor("deep")).seconds, 600);
        let names: Vec<&RowKind> = rows.iter().map(|r| &r.kind).collect();
        assert_eq!(
            names,
            [
                &milestone("A"),
                &milestone("B"),
                &executor("deep"),
                &executor("lite")
            ]
        );
    }

    #[test]
    fn an_open_segment_runs_to_now() {
        let snap = snapshot(
            vec![task(1, "A", "deep")],
            vec![agent(
                "a1",
                "plan-worker-deep",
                vec![segment(&[1], "2026-11-12T11:15:00Z", "")],
                Usage::default(),
            )],
        );
        let rows = rollup(&snap, now());
        assert_eq!(row(&rows, &milestone("A")).seconds, 2_700);
    }

    #[test]
    fn an_agent_without_usage_makes_the_total_partial() {
        let snap = snapshot(
            vec![task(1, "A", "deep"), task(2, "A", "deep")],
            vec![
                agent(
                    "a1",
                    "plan-worker-deep",
                    vec![segment(
                        &[1],
                        "2026-11-12T10:00:00Z",
                        "2026-11-12T10:10:00Z",
                    )],
                    usage(100, 50),
                ),
                agent(
                    "a2",
                    "plan-worker-deep",
                    vec![segment(
                        &[2],
                        "2026-11-12T10:00:00Z",
                        "2026-11-12T10:10:00Z",
                    )],
                    Usage::default(),
                ),
            ],
        );
        let rows = rollup(&snap, now());
        let a = row(&rows, &milestone("A"));
        assert_eq!(a.tokens, Tokens::Partial);
        assert_eq!(a.agents.len(), 2);
        assert_eq!(a.usage, usage(100, 50));
        assert_eq!(a.seconds, 1_200);
        assert!(!a.agents[1].recorded);
    }

    #[test]
    fn a_flow_whose_agents_all_lack_usage_is_not_recorded() {
        let snap = snapshot(
            vec![task(1, "A", "deep")],
            vec![
                agent(
                    "a1",
                    "plan-worker-deep",
                    vec![segment(
                        &[1],
                        "2026-11-12T10:00:00Z",
                        "2026-11-12T10:10:00Z",
                    )],
                    Usage::default(),
                ),
                agent("v1", "verification", Vec::new(), Usage::default()),
            ],
        );
        let rows = rollup(&snap, now());
        assert!(!rows.is_empty());
        for r in &rows {
            assert_eq!(r.tokens, Tokens::NotRecorded, "{:?}", r.kind);
        }
    }

    #[test]
    fn a_non_worker_agent_lands_in_other_by_type() {
        let mut planner = agent(
            "p1",
            "plan-investigator",
            vec![segment(
                &[1],
                "2026-11-12T10:00:00Z",
                "2026-11-12T10:05:00Z",
            )],
            usage(7, 3),
        );
        planner.ended_at = String::new();
        let snap = snapshot(
            vec![task(1, "A", "deep")],
            vec![
                planner,
                agent("v1", "verification", Vec::new(), usage(9, 1)),
            ],
        );
        let rows = rollup(&snap, now());
        assert_eq!(rows.len(), 2);
        let inv = row(&rows, &RowKind::Other("plan-investigator".to_string()));
        assert_eq!(inv.seconds, 7_200);
        assert_eq!(inv.usage, usage(7, 3));
        let ver = row(&rows, &RowKind::Other("verification".to_string()));
        assert_eq!(ver.seconds, 3_600);
        assert_eq!(rows[1].kind, RowKind::Other("verification".to_string()));
    }

    #[test]
    fn gate_rounds_add_gate_time_to_their_milestone() {
        let mut snap = snapshot(
            vec![task(1, "A", "deep")],
            vec![agent(
                "a1",
                "plan-worker-deep",
                vec![segment(
                    &[1],
                    "2026-11-12T10:00:00Z",
                    "2026-11-12T10:01:00Z",
                )],
                usage(1, 1),
            )],
        );
        let command = |duration_s| GateCommand {
            duration_s,
            ..GateCommand::default()
        };
        snap.plan = Some(Plan {
            gate_rounds: vec![
                GateRound {
                    milestone: "A".to_string(),
                    commands: vec![command(30), command(12)],
                    ..GateRound::default()
                },
                GateRound {
                    milestone: "A".to_string(),
                    round: 1,
                    commands: vec![command(8)],
                    ..GateRound::default()
                },
            ],
            ..Plan::default()
        });
        let rows = rollup(&snap, now());
        let a = row(&rows, &milestone("A"));
        assert_eq!((a.seconds, a.gate_seconds), (60, 50));
        assert_eq!(row(&rows, &executor("deep")).gate_seconds, 0);
    }

    #[test]
    fn no_agents_give_no_rows() {
        let snap = snapshot(vec![task(1, "A", "deep")], Vec::new());
        assert!(rollup(&snap, now()).is_empty());
    }
}
