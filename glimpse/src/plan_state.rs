//! The Plan surface's state: its rows, sections and how rows are built.
//!
//! [`rows`] turns a v2 flow's records into one list in eleven fixed sections and
//! reads nothing but its arguments. A row's id is `<section>:<key>`, built from the
//! record's own id, path or store position, so a rebuild keeps the cursor. The
//! state is read-only: it holds no marks, offers no menu and has no write target.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the plan surface's wiring and view read these")
)]

use std::collections::BTreeSet;
use std::ops::{Deref, DerefMut};
use std::time::Instant;

use crate::app::Scroll;
use crate::authored::{AuthoredFile, AuthoredKind};
use crate::hook::parse_utc;
use crate::model::plan::{Health, HealthSite, Probe};
use crate::model::{Snapshot, TaskStatus};
use crate::nav::Target;
use crate::rollup::{self, RowKind, Tokens};
use crate::surface::{CursorList, Tracked, Tracking, VisibleRow, item_ids, reposition};

/// What the surface says in place of its sections when the flow has no plan store.
pub(crate) const NOT_V2: &str = "not a v2 flow: it has no plan store";

/// The surface's sections, in the order they are drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Section {
    #[default]
    Brief,
    Decisions,
    Contracts,
    Handovers,
    Milestones,
    Timeline,
    Notes,
    Changes,
    Departures,
    Cost,
    Health,
}

impl Section {
    pub(crate) const ALL: [Section; 11] = [
        Section::Brief,
        Section::Decisions,
        Section::Contracts,
        Section::Handovers,
        Section::Milestones,
        Section::Timeline,
        Section::Notes,
        Section::Changes,
        Section::Departures,
        Section::Cost,
        Section::Health,
    ];

    /// The heading, and the prefix of every row id in the section.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Decisions => "decisions",
            Self::Contracts => "contracts",
            Self::Handovers => "handovers",
            Self::Milestones => "milestones",
            Self::Timeline => "timeline",
            Self::Notes => "notes",
            Self::Changes => "changes",
            Self::Departures => "departures",
            Self::Cost => "cost",
            Self::Health => "health",
        }
    }
}

/// The record a row stands for, so its details can show the whole of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Record {
    /// A listed path, for a Brief or Notes row.
    Authored(String),
    Decision(String),
    Contract(String),
    Handover(String),
    Milestone(String),
    /// An index into the plan's run records.
    Run(usize),
    /// An index into the snapshot's change trail.
    Change(usize),
    /// Indices into the plan's attempts and that attempt's deviations.
    Departure {
        attempt: usize,
        deviation: usize,
    },
    Cost(rollup::Row),
    Health(Health),
}

/// `Default` only because [`Tracking`] derives it; no built row is a default one.
impl Default for Record {
    fn default() -> Self {
        Record::Authored(String::new())
    }
}

/// One row of the Plan surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PlanRow {
    /// `<section>:<key>`.
    pub(crate) id: String,
    pub(crate) section: Section,
    /// The texts the table shows, in column order.
    pub(crate) cells: Vec<String>,
    /// Where `Enter` leads; `None` when the row names nothing glimpse can show.
    pub(crate) target: Option<Target>,
    pub(crate) record: Record,
    /// A handover's `done` or `open`, a run's outcome, a health row's verdict; `""` otherwise.
    pub(crate) state: String,
}

impl Tracked for PlanRow {
    fn id(&self) -> &str {
        &self.id
    }
    fn status(&self) -> &str {
        &self.state
    }
}

/// The Plan surface's list and the user's view of it.
#[derive(Debug, Clone, Default)]
pub(crate) struct PlanState {
    /// Reached through `Deref`, so `state.rows` and `state.cursor` read as its own fields.
    pub(crate) tracking: Tracking<PlanRow>,
    /// Whether the last rebuild's snapshot carried a plan store.
    pub(crate) has_plan: bool,
}

impl Deref for PlanState {
    type Target = Tracking<PlanRow>;
    fn deref(&self) -> &Tracking<PlanRow> {
        &self.tracking
    }
}

impl DerefMut for PlanState {
    fn deref_mut(&mut self) -> &mut Tracking<PlanRow> {
        &mut self.tracking
    }
}

impl PlanState {
    /// Rebuilds the rows from the snapshot, the authored listing and the rollups,
    /// keeping the cursor on its id; a cursor whose row vanished takes the row at its
    /// visible position.
    pub(crate) fn rebuild(
        &mut self,
        snapshot: &Snapshot,
        authored: &[AuthoredFile],
        cost: &[rollup::Row],
        viewing: bool,
        now: Instant,
    ) {
        let old_position = self.cursor_position();
        self.has_plan = snapshot.plan.is_some();
        let cursor_lost = self
            .tracking
            .refresh(rows(snapshot, authored, cost), None, viewing, now);
        if cursor_lost {
            self.tracking.cursor = reposition(self.visible_ids(), old_position);
        }
    }

    /// The line drawn in place of the sections, when there is one.
    pub(crate) fn empty_text(&self) -> Option<&'static str> {
        (!self.has_plan).then_some(NOT_V2)
    }

    /// The tab's count: the Health rows plus the open handovers.
    pub(crate) fn count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| {
                row.section == Section::Health
                    || (row.section == Section::Handovers && row.state == OPEN)
            })
            .count()
    }

    /// Every section under its heading, an empty one included; nothing without a plan store.
    pub(crate) fn visible(&self) -> Vec<VisibleRow> {
        if !self.has_plan {
            return Vec::new();
        }
        let mut out = Vec::new();
        for section in Section::ALL {
            let members: Vec<&PlanRow> =
                self.rows.iter().filter(|r| r.section == section).collect();
            out.push(VisibleRow::Header {
                label: section.label().to_owned(),
                count: members.len(),
            });
            out.extend(members.into_iter().map(|r| VisibleRow::Item(r.id.clone())));
        }
        out
    }

    pub(crate) fn move_cursor(&mut self, delta: isize) {
        let items = self.visible_ids();
        self.tracking.step_cursor(&items, delta);
    }

    pub(crate) fn page_cursor(&mut self, scroll: Scroll) {
        let items = self.visible_ids();
        self.tracking.page_through(&items, scroll);
    }

    /// Moves the cursor to the first row of the next section that has rows; at the
    /// last such section it stays. A cursor on no row lands on the first row.
    pub(crate) fn next_section(&mut self) {
        let firsts = self.section_firsts();
        let next = match self.cursor_section() {
            Some(at) => firsts.iter().find(|(section, _)| *section > at),
            None => firsts.first(),
        };
        if let Some((_, id)) = next {
            self.tracking.cursor = Some(id.clone());
            self.tracking.scroll_pinned = false;
        }
    }

    /// Moves the cursor to the first row of the previous section that has rows; in
    /// the first such section it stays. A cursor on no row lands on the first row.
    pub(crate) fn prev_section(&mut self) {
        let firsts = self.section_firsts();
        let prev = match self.cursor_section() {
            Some(at) => firsts.iter().rev().find(|(section, _)| *section < at),
            None => firsts.first(),
        };
        if let Some((_, id)) = prev {
            self.tracking.cursor = Some(id.clone());
            self.tracking.scroll_pinned = false;
        }
    }

    fn cursor_section(&self) -> Option<Section> {
        self.tracking.cursor_row().map(|row| row.section)
    }

    /// Each section with rows, and its first row's id, in drawn order.
    fn section_firsts(&self) -> Vec<(Section, String)> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for id in self.visible_ids() {
            let Some(row) = self.tracking.row(&id) else {
                continue;
            };
            if seen.insert(row.section) {
                out.push((row.section, id));
            }
        }
        out
    }

    fn visible_ids(&self) -> Vec<String> {
        item_ids(self.visible())
    }

    fn cursor_position(&self) -> Option<usize> {
        self.tracking.position_in(&self.visible_ids())
    }
}

impl CursorList for PlanState {
    fn move_cursor(&mut self, delta: isize) {
        PlanState::move_cursor(self, delta);
    }
    fn page_cursor(&mut self, scroll: Scroll) {
        PlanState::page_cursor(self, scroll);
    }
    fn scroll_by(&mut self, rows: i32) {
        self.tracking.scroll_by(rows);
    }
    fn viewed(&mut self) {
        self.tracking.viewed();
    }
    fn select_row(&mut self, id: &str) {
        self.tracking.select_row(id);
    }
    fn cursor(&self) -> Option<&str> {
        self.tracking.cursor.as_deref()
    }
    fn cursor_row_id(&self) -> Option<&str> {
        self.tracking.cursor_row().map(|row| row.id.as_str())
    }
    fn unpin(&mut self) {
        self.tracking.scroll_pinned = false;
    }
}

const OPEN: &str = "open";
const DONE: &str = "done";
const UNPROBED: &str = "unprobed";

/// The Plan surface's rows in section order; none when the snapshot has no plan store.
pub(crate) fn rows(
    snapshot: &Snapshot,
    authored: &[AuthoredFile],
    cost: &[rollup::Row],
) -> Vec<PlanRow> {
    let Some(plan) = &snapshot.plan else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push = |section: Section,
                    key: &str,
                    cells: Vec<String>,
                    target: Option<Target>,
                    record: Record,
                    state: &str| {
        out.push(PlanRow {
            id: format!("{}:{key}", section.label()),
            section,
            cells,
            target,
            record,
            state: state.to_owned(),
        });
    };
    let task_target = |id: u32| {
        snapshot
            .tasks
            .iter()
            .any(|task| task.id == id)
            .then_some(Target::Task(id))
    };

    let mut brief: Vec<&AuthoredFile> = authored
        .iter()
        .filter(|f| matches!(f.kind, AuthoredKind::Brief | AuthoredKind::Design))
        .collect();
    brief.sort_by_key(|f| f.kind != AuthoredKind::Brief);
    for file in brief {
        let title = if file.headline.is_empty() {
            &file.path
        } else {
            &file.headline
        };
        push(
            Section::Brief,
            &file.path,
            vec![title.clone()],
            Some(Target::Authored(file.path.clone())),
            Record::Authored(file.path.clone()),
            "",
        );
    }

    for decision in &plan.decisions {
        let first = decision.decision.lines().next().unwrap_or_default();
        push(
            Section::Decisions,
            &decision.id,
            vec![
                decision.id.clone(),
                decision.door.clone(),
                decision.provenance.clone(),
                first.to_owned(),
            ],
            Some(Target::Decision(decision.id.clone())),
            Record::Decision(decision.id.clone()),
            "",
        );
    }

    for contract in &plan.contracts {
        push(
            Section::Contracts,
            &contract.id,
            vec![
                contract.id.clone(),
                contract.kind.clone(),
                contract.claim.clone(),
            ],
            Some(Target::Contract(contract.id.clone())),
            Record::Contract(contract.id.clone()),
            "",
        );
    }

    for handover in &plan.handovers {
        let state = if handover.done { DONE } else { OPEN };
        push(
            Section::Handovers,
            &handover.id,
            vec![
                handover.id.clone(),
                handover.reason.clone(),
                state.to_owned(),
                handover.action.clone(),
            ],
            Some(Target::Handover(handover.id.clone())),
            Record::Handover(handover.id.clone()),
            state,
        );
    }

    for milestone in &snapshot.milestones {
        let goal = snapshot
            .checkpoints
            .iter()
            .find(|c| c.id == milestone.id)
            .map(|c| c.rationale.clone())
            .unwrap_or_default();
        let members = snapshot
            .tasks
            .iter()
            .filter(|t| t.checkpoint == milestone.id);
        let total = members.clone().count();
        let done = members.filter(|t| t.status == TaskStatus::Done).count();
        let rounds = plan
            .gate_rounds
            .iter()
            .filter(|r| r.milestone == milestone.id);
        let last = rounds
            .clone()
            .max_by_key(|r| (r.round, r.seq))
            .map_or("not gated", |r| r.outcome.as_str());
        let fix_rounds: BTreeSet<u32> = rounds.map(|r| r.round).filter(|&r| r > 0).collect();
        push(
            Section::Milestones,
            &milestone.id,
            vec![
                milestone.id.clone(),
                goal,
                format!("{done}/{total}"),
                last.to_owned(),
                fix_rounds.len().to_string(),
            ],
            Some(Target::Milestone(milestone.id.clone())),
            Record::Milestone(milestone.id.clone()),
            "",
        );
    }

    for (pos, run) in plan.runs.iter().enumerate() {
        let outcome = if !run.outcome.is_empty() {
            run.outcome.as_str()
        } else if !run.skip_reason.is_empty() {
            "skipped"
        } else if run.finished.is_empty() {
            OPEN
        } else {
            ""
        };
        let took = match (parse_utc(&run.started), parse_utc(&run.finished)) {
            (Some(start), Some(end)) if end >= start => duration(end - start),
            _ => String::new(),
        };
        push(
            Section::Timeline,
            &pos.to_string(),
            vec![
                run.step.clone(),
                outcome.to_owned(),
                minute(&run.started),
                took,
            ],
            Some(Target::Run(pos)),
            Record::Run(pos),
            outcome,
        );
    }

    let mut notes: Vec<&AuthoredFile> = authored
        .iter()
        .filter_map(|f| note_rank(f.kind).map(|_| f))
        .collect();
    notes.sort_by_key(|f| {
        let task = if f.kind == AuthoredKind::RunNote {
            f.task
        } else {
            0
        };
        (note_rank(f.kind), task)
    });
    for file in notes {
        push(
            Section::Notes,
            &file.path,
            vec![
                note_kind(file.kind).to_owned(),
                file.id.clone(),
                file.headline.clone(),
            ],
            Some(Target::Authored(file.path.clone())),
            Record::Authored(file.path.clone()),
            "",
        );
    }

    if let Some(approved) = plan.approvals.iter().filter_map(|a| parse_utc(&a.at)).max() {
        for (pos, change) in snapshot.changes.iter().enumerate() {
            if parse_utc(&change.at).is_none_or(|at| at < approved) {
                continue;
            }
            let target = snapshot
                .tasks
                .iter()
                .find(|t| !change.task_ref.is_empty() && t.task_ref == change.task_ref)
                .map(|t| Target::Task(t.id));
            push(
                Section::Changes,
                &pos.to_string(),
                vec![
                    minute(&change.at),
                    change.task_ref.clone(),
                    change.field.clone(),
                    change.reason.clone(),
                ],
                target,
                Record::Change(pos),
                "",
            );
        }
    }

    for (a, attempt) in plan.attempts.iter().enumerate() {
        for (d, deviation) in attempt.deviations.iter().enumerate() {
            push(
                Section::Departures,
                &format!("{a}.{d}"),
                vec![attempt.task.to_string(), deviation.what.clone()],
                task_target(attempt.task),
                Record::Departure {
                    attempt: a,
                    deviation: d,
                },
                "",
            );
        }
    }

    for row in cost {
        let (key, name) = match &row.kind {
            RowKind::Milestone(id) => (format!("milestone:{id}"), or(id, "no milestone")),
            RowKind::Executor(name) => (format!("executor:{name}"), or(name, "no executor")),
            RowKind::Other(kind) => (format!("other:{kind}"), format!("other: {kind}")),
        };
        let mut time = duration(row.seconds);
        if row.gate_seconds > 0 {
            time.push_str(&format!(" + gate {}", duration(row.gate_seconds)));
        }
        push(
            Section::Cost,
            &key,
            vec![name, row.agents.len().to_string(), time, tokens(row)],
            None,
            Record::Cost(row.clone()),
            "",
        );
    }

    for health in health(snapshot) {
        let (key, subject, kind) = match &health.site {
            HealthSite::Accept { task, index } => {
                (format!("accept:{task}.{index}"), task.to_string(), "accept")
            }
            HealthSite::Premise { task, index } => (
                format!("premise:{task}.{index}"),
                task.to_string(),
                "premise",
            ),
            HealthSite::GateProbe { milestone, index } => (
                format!("gate:{milestone}.{index}"),
                milestone.clone(),
                "gate probe",
            ),
        };
        let target = match &health.site {
            HealthSite::Accept { task, .. } | HealthSite::Premise { task, .. } => {
                task_target(*task)
            }
            HealthSite::GateProbe { .. } => None,
        };
        let verdict = or(&health.verdict, UNPROBED);
        push(
            Section::Health,
            &key,
            vec![subject, kind.to_owned(), verdict.clone()],
            target,
            Record::Health(health),
            &verdict,
        );
    }

    out
}

/// The probes the Health section lists: an accept entry or premise unprobed while
/// probeable, or recorded with a verdict other than `healthy` or `holds`; a gate probe
/// unrecorded or recorded with a verdict other than `holds`. Tasks come in snapshot
/// order, each task's accept entries before its premises, then the gate probes.
fn health(snapshot: &Snapshot) -> Vec<Health> {
    let due = |probeable: bool, probe: Option<&Probe>| -> Option<String> {
        let verdict = probe.map_or("", |p| p.verdict.as_str());
        let show = if verdict.is_empty() {
            probeable
        } else {
            !matches!(verdict, "healthy" | "holds")
        };
        show.then(|| verdict.to_owned())
    };
    let mut out = Vec::new();
    for task in &snapshot.tasks {
        let Some(spec) = &task.spec else { continue };
        for (index, accept) in spec.accept.iter().enumerate() {
            if let Some(verdict) = due(accept.probeable, accept.probe.as_ref()) {
                out.push(Health {
                    site: HealthSite::Accept {
                        task: task.id,
                        index,
                    },
                    verdict,
                });
            }
        }
        for (index, premise) in spec.premises.iter().enumerate() {
            if let Some(verdict) = due(premise.probeable, premise.probe.as_ref()) {
                out.push(Health {
                    site: HealthSite::Premise {
                        task: task.id,
                        index,
                    },
                    verdict,
                });
            }
        }
    }
    for milestone in &snapshot.milestones {
        for (index, probe) in milestone.gate_probes.iter().enumerate() {
            if probe.verdict != "holds" {
                out.push(Health {
                    site: HealthSite::GateProbe {
                        milestone: milestone.id.clone(),
                        index,
                    },
                    verdict: probe.verdict.clone(),
                });
            }
        }
    }
    out
}

/// The Notes section's order by kind, run notes last; `None` for Brief kinds.
fn note_rank(kind: AuthoredKind) -> Option<u8> {
    match kind {
        AuthoredKind::Brief | AuthoredKind::Design => None,
        AuthoredKind::Note => Some(0),
        AuthoredKind::Challenge => Some(1),
        AuthoredKind::ColdRead => Some(2),
        AuthoredKind::Backlog => Some(3),
        AuthoredKind::Settle => Some(4),
        AuthoredKind::Unknown => Some(5),
        AuthoredKind::RunNote => Some(6),
    }
}

fn note_kind(kind: AuthoredKind) -> &'static str {
    match kind {
        AuthoredKind::Brief => "brief",
        AuthoredKind::Design => "design",
        AuthoredKind::Note => "note",
        AuthoredKind::Challenge => "challenge",
        AuthoredKind::ColdRead => "cold read",
        AuthoredKind::Backlog => "backlog",
        AuthoredKind::Settle => "settle",
        AuthoredKind::Unknown => "other",
        AuthoredKind::RunNote => "run note",
    }
}

fn or(value: &str, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

/// A tomlctl timestamp cut to the minute, `2026-09-27 08:00`; other text unchanged.
fn minute(at: &str) -> String {
    match at.get(..16) {
        Some(head) if at.as_bytes().get(10) == Some(&b'T') => head.replacen('T', " ", 1),
        _ => at.to_owned(),
    }
}

/// The row's token total, marked `partial` when some counted agents have no usage.
fn tokens(row: &rollup::Row) -> String {
    let usage = &row.usage;
    let total = usage.input + usage.cache_write + usage.cache_read + usage.output;
    match row.tokens {
        Tokens::Recorded => count(total),
        Tokens::Partial => format!("{} partial", count(total)),
        Tokens::NotRecorded => "not recorded".to_owned(),
    }
}

fn count(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        let tenths = (n + 50) / 100;
        format!("{}.{}k", tenths / 10, tenths % 10)
    } else {
        let tenths = (n + 50_000) / 100_000;
        format!("{}.{}M", tenths / 10, tenths % 10)
    }
}

fn duration(seconds: u64) -> String {
    let (d, h, m, s) = (
        seconds / 86_400,
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60,
    );
    if d > 0 {
        format!("{d}d{h:02}h")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::fixture;
    use crate::model::plan::{Approval, Change};

    const NOW: &str = "2026-09-28T12:00:00Z";

    fn file(path: &str, kind: AuthoredKind, id: &str, task: u32) -> AuthoredFile {
        AuthoredFile {
            path: path.to_string(),
            kind,
            id: id.to_string(),
            headline: format!("{id} headline"),
            task,
            ..AuthoredFile::default()
        }
    }

    fn listing() -> Vec<AuthoredFile> {
        vec![
            file("notes/run/task-3.md", AuthoredKind::RunNote, "task-3", 3),
            file("design/keys.md", AuthoredKind::Design, "keys", 0),
            file(
                "notes/structure.md",
                AuthoredKind::Challenge,
                "structure",
                0,
            ),
            file("brief.md", AuthoredKind::Brief, "brief", 0),
            file("notes/keys.md", AuthoredKind::Note, "keys", 0),
        ]
    }

    fn built(snap: &Snapshot, authored: &[AuthoredFile]) -> Vec<PlanRow> {
        let cost = rollup::rollup(snap, parse_utc(NOW).unwrap());
        rows(snap, authored, &cost)
    }

    fn ids(rows: &[PlanRow], section: Section) -> Vec<String> {
        rows.iter()
            .filter(|r| r.section == section)
            .map(|r| r.id.clone())
            .collect()
    }

    fn state(snap: &Snapshot, authored: &[AuthoredFile]) -> PlanState {
        let cost = rollup::rollup(snap, parse_utc(NOW).unwrap());
        let mut state = PlanState::default();
        state.rebuild(snap, authored, &cost, true, Instant::now());
        state
    }

    #[test]
    fn plan_state_builds_every_section_of_the_fixture() {
        let snap = fixture();
        let rows = built(&snap, &listing());
        assert_eq!(
            ids(&rows, Section::Brief),
            ["brief:brief.md", "brief:design/keys.md"]
        );
        assert_eq!(ids(&rows, Section::Decisions), ["decisions:D1"]);
        assert_eq!(ids(&rows, Section::Contracts), ["contracts:C1"]);
        assert_eq!(
            ids(&rows, Section::Handovers),
            ["handovers:H1", "handovers:H2"]
        );
        assert_eq!(
            ids(&rows, Section::Milestones),
            ["milestones:A", "milestones:B"]
        );
        assert_eq!(
            ids(&rows, Section::Timeline),
            ["timeline:0", "timeline:1", "timeline:2"]
        );
        assert_eq!(
            ids(&rows, Section::Notes),
            [
                "notes:notes/keys.md",
                "notes:notes/structure.md",
                "notes:notes/run/task-3.md"
            ]
        );
        assert_eq!(ids(&rows, Section::Changes), ["changes:0"]);
        assert_eq!(ids(&rows, Section::Departures), ["departures:0.0"]);
        assert_eq!(
            ids(&rows, Section::Cost),
            [
                "cost:milestone:A",
                "cost:milestone:B",
                "cost:other:implement-deep",
                "cost:other:implement-lite"
            ]
        );
        assert_eq!(
            ids(&rows, Section::Health),
            ["health:accept:6.1", "health:premise:6.0", "health:gate:B.0"]
        );
        let sections: Vec<Section> = rows.iter().map(|r| r.section).collect();
        let mut sorted = sections.clone();
        sorted.sort();
        assert_eq!(sections, sorted, "rows come in section order");
    }

    #[test]
    fn plan_state_rows_show_their_cells_targets_and_states() {
        let snap = fixture();
        let rows = built(&snap, &listing());
        let row = |id: &str| rows.iter().find(|r| r.id == id).expect(id);

        let decision = row("decisions:D1");
        assert_eq!(
            decision.cells,
            [
                "D1",
                "two-way",
                "agent-assumed",
                "Read the key names from the config."
            ]
        );
        assert_eq!(decision.target, Some(Target::Decision("D1".to_string())));

        assert_eq!(row("handovers:H1").state, "open");
        assert_eq!(row("handovers:H2").state, "done");
        assert_eq!(row("handovers:H2").cells[2], "done");

        let a = row("milestones:A");
        assert_eq!(
            a.cells,
            ["A", "the foundation every view reads", "3/3", "pass", "1"]
        );
        assert_eq!(a.target, Some(Target::Milestone("A".to_string())));
        assert_eq!(row("milestones:B").cells[2..], ["0/5", "fail", "0"]);

        let run = row("timeline:2");
        assert_eq!(
            run.cells,
            ["implement:B", "stopped", "2026-09-28 11:00", "58m00s"]
        );
        assert_eq!(run.state, "stopped");
        assert_eq!(run.target, Some(Target::Run(2)));

        assert_eq!(row("brief:brief.md").cells, ["brief headline"]);
        let note = row("notes:notes/run/task-3.md");
        assert_eq!(note.cells, ["run note", "task-3", "task-3 headline"]);
        assert_eq!(
            note.target,
            Some(Target::Authored("notes/run/task-3.md".to_string()))
        );

        let change = row("changes:0");
        assert_eq!(
            change.cells,
            [
                "2026-09-28 09:15",
                "bind-the-keys",
                "detail",
                "the config task settled the names"
            ]
        );
        assert_eq!(change.target, Some(Target::Task(6)));

        let departure = row("departures:0.0");
        assert_eq!(
            departure.cells,
            ["3", "Returned warnings beside the config"]
        );
        assert_eq!(departure.target, Some(Target::Task(3)));

        let cost = row("cost:other:implement-lite");
        assert_eq!(cost.cells[0], "other: implement-lite");
        assert_eq!(cost.cells[1], "1");
        assert_eq!(cost.target, None);
        assert!(matches!(cost.record, Record::Cost(_)));
        assert_eq!(row("cost:other:implement-deep").cells[3], "not recorded");

        let accept = row("health:accept:6.1");
        assert_eq!(accept.cells, ["6", "accept", "vacuous"]);
        assert_eq!(accept.state, "vacuous");
        assert_eq!(accept.target, Some(Target::Task(6)));
        let gate = row("health:gate:B.0");
        assert_eq!(gate.cells, ["B", "gate probe", "unprobed"]);
        assert_eq!(gate.target, None);
    }

    #[test]
    fn plan_state_health_leaves_out_an_unprobed_entry_that_is_not_probeable() {
        let mut snap = fixture();
        let rows = built(&snap, &[]);
        assert!(
            !rows.iter().any(|r| r.id == "health:premise:6.1"),
            "premise 6.1 is unprobed and not probeable"
        );
        assert!(!rows.iter().any(|r| r.id == "health:accept:5.0"));

        let spec = snap.tasks[5].spec.as_mut().unwrap();
        spec.premises[1].probe = Some(Probe {
            verdict: "fails".to_string(),
            ..Probe::default()
        });
        spec.accept[0].probe = Some(Probe {
            verdict: "healthy".to_string(),
            ..Probe::default()
        });
        let rows = built(&snap, &[]);
        assert_eq!(
            ids(&rows, Section::Health),
            [
                "health:accept:6.1",
                "health:premise:6.0",
                "health:premise:6.1",
                "health:gate:B.0"
            ],
            "a recorded bad verdict shows whether or not the entry is probeable"
        );
    }

    #[test]
    fn plan_state_changes_start_at_the_latest_approval() {
        let mut snap = fixture();
        let push_change = |snap: &mut Snapshot, at: &str| {
            snap.changes.push(Change {
                at: at.to_string(),
                task_ref: "assemble-the-app".to_string(),
                field: "detail".to_string(),
                reason: format!("at {at}"),
                ..Change::default()
            });
        };
        push_change(&mut snap, "2026-09-28T09:30:00Z");
        push_change(&mut snap, "2026-09-28T10:30:00Z");
        snap.plan.as_mut().unwrap().approvals.push(Approval {
            at: "2026-09-28T09:30:00Z".to_string(),
            ..Approval::default()
        });
        let rows = built(&snap, &[]);
        assert_eq!(
            ids(&rows, Section::Changes),
            ["changes:2", "changes:3"],
            "the change before the latest approval and the undated one are left out"
        );
        assert_eq!(
            rows.iter().find(|r| r.id == "changes:3").unwrap().target,
            Some(Target::Task(7))
        );

        snap.plan.as_mut().unwrap().approvals.clear();
        assert!(ids(&built(&snap, &[]), Section::Changes).is_empty());
    }

    #[test]
    fn plan_state_draws_an_empty_section_as_its_heading() {
        let mut snap = fixture();
        snap.plan.as_mut().unwrap().contracts.clear();
        let state = state(&snap, &[]);
        let visible = state.visible();
        let headers: Vec<(&str, usize)> = visible
            .iter()
            .filter_map(|row| match row {
                VisibleRow::Header { label, count } => Some((label.as_str(), *count)),
                VisibleRow::Item(_) => None,
            })
            .collect();
        assert_eq!(headers.len(), 11);
        assert_eq!(headers[0], ("brief", 0));
        assert_eq!(headers[2], ("contracts", 0));
        assert_eq!(headers[6], ("notes", 0));
        let contracts = visible
            .iter()
            .position(|r| matches!(r, VisibleRow::Header { label, .. } if label == "contracts"))
            .unwrap();
        assert!(
            matches!(&visible[contracts + 1], VisibleRow::Header { label, .. } if label == "handovers")
        );
    }

    #[test]
    fn plan_state_without_a_plan_store_holds_no_rows_and_says_so() {
        let mut snap = fixture();
        snap.plan = None;
        let state = state(&snap, &listing());
        assert!(state.rows.is_empty());
        assert!(state.visible().is_empty());
        assert_eq!(state.empty_text(), Some(NOT_V2));
        assert_eq!(state.count(), 0);

        let v2 = self::state(&fixture(), &[]);
        assert_eq!(v2.empty_text(), None);
    }

    #[test]
    fn plan_state_counts_health_rows_and_open_handovers() {
        let state = state(&fixture(), &listing());
        assert_eq!(state.count(), 3 + 1);
    }

    #[test]
    fn plan_state_keeps_the_cursor_across_a_rebuild() {
        let snap = fixture();
        let mut state = state(&snap, &listing());
        state.select_row("handovers:H2");
        let mut changed = snap.clone();
        changed.plan.as_mut().unwrap().decisions.clear();
        let cost = rollup::rollup(&changed, parse_utc(NOW).unwrap());
        state.rebuild(&changed, &listing(), &cost, true, Instant::now());
        assert_eq!(state.cursor.as_deref(), Some("handovers:H2"));

        changed.plan.as_mut().unwrap().handovers.pop();
        state.rebuild(&changed, &listing(), &cost, true, Instant::now());
        assert_eq!(
            state.cursor.as_deref(),
            Some("milestones:A"),
            "a vanished row's cursor takes the row at its position"
        );
    }

    #[test]
    fn plan_state_jumps_between_sections_and_stops_at_both_ends() {
        let mut snap = fixture();
        snap.plan.as_mut().unwrap().contracts.clear();
        let mut state = state(&snap, &listing());
        assert_eq!(
            state.cursor.as_deref(),
            Some("brief:brief.md"),
            "the first build puts the cursor on the first row"
        );
        state.prev_section();
        assert_eq!(
            state.cursor.as_deref(),
            Some("brief:brief.md"),
            "the first section has none before it"
        );
        state.next_section();
        assert_eq!(state.cursor.as_deref(), Some("decisions:D1"));
        state.next_section();
        assert_eq!(
            state.cursor.as_deref(),
            Some("handovers:H1"),
            "an empty section is skipped"
        );
        state.move_cursor(1);
        assert_eq!(state.cursor.as_deref(), Some("handovers:H2"));
        state.prev_section();
        assert_eq!(state.cursor.as_deref(), Some("decisions:D1"));

        state.select_row("health:gate:B.0");
        state.next_section();
        assert_eq!(
            state.cursor.as_deref(),
            Some("health:gate:B.0"),
            "the last section has none after it"
        );
        state.prev_section();
        assert_eq!(state.cursor.as_deref(), Some("cost:milestone:A"));
    }

    #[test]
    fn plan_state_moves_through_its_visible_order() {
        let mut state = state(&fixture(), &[]);
        assert_eq!(state.cursor.as_deref(), Some("decisions:D1"));
        state.move_cursor(1);
        assert_eq!(state.cursor.as_deref(), Some("contracts:C1"));
        state.move_cursor(1);
        assert_eq!(state.cursor.as_deref(), Some("handovers:H1"));
        state.move_cursor(-5);
        assert_eq!(state.cursor.as_deref(), Some("decisions:D1"));
        assert_eq!(CursorList::cursor_row_id(&state), Some("decisions:D1"));
    }

    #[test]
    fn plan_state_formats_times_and_tokens_compactly() {
        assert_eq!(minute("2026-09-27T08:00:00Z"), "2026-09-27 08:00");
        assert_eq!(minute(""), "");
        assert_eq!(duration(3_480), "58m00s");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_250), "1.3k");
        assert_eq!(count(2_340_000), "2.3M");
    }
}
