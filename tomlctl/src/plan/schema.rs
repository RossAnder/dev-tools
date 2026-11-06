//! Plan store records and their TOML conversions.
//!
//! ```toml
//! schema_version = 1
//! last_updated = 2026-11-05
//! [binding] / [id_high_water]
//! [[decision]] / [[contract]] / [[approval]] / [[run]] / [[handover]]
//! ```
//!
//! `to_toml` inserts keys in that order and `preserve_order` holds it, so a
//! write is byte-stable with no sort step. Each root key but `schema_version`
//! is omitted while empty. Free text is LF-normalised on both read and write,
//! because a lone `\r` forces `toml`'s escaped string form. Unknown keys are
//! dropped: the store is tool-owned and every write re-emits it from this shape.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use toml::Value as TomlValue;
use toml::map::Map;
use toml::value::Datetime;

use crate::tasks::schema::{
    as_date as as_datetime, id_array, lf, normal_path, str_arr_value, str_array,
};

pub(crate) const SCHEMA_VERSION: i64 = 1;

type Table = Map<String, TomlValue>;

/// A closed set of string values. Bounding the read helpers on it ties a
/// parser to its own list, so the error cannot name another vocabulary's values.
pub(crate) trait Vocabulary: Sized {
    const VOCABULARY: &'static [&'static str];

    fn parse(raw: &str) -> Option<Self>;
}

macro_rules! vocabulary {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum $name {
            $($variant),+
        }

        impl $name {
            pub(crate) const VOCABULARY: &'static [&'static str] = &[$($text),+];

            pub(crate) fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            /// Case-sensitive, as the task store's vocabularies are.
            pub(crate) fn parse(raw: &str) -> Option<Self> {
                match raw {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl Vocabulary for $name {
            const VOCABULARY: &'static [&'static str] = $name::VOCABULARY;

            fn parse(raw: &str) -> Option<Self> {
                $name::parse(raw)
            }
        }
    };
}

vocabulary!(Provenance {
    UserStated => "user-stated",
    UserConfirmed => "user-confirmed",
    AgentAssumed => "agent-assumed",
});

vocabulary!(Door {
    OneWay => "one-way",
    TwoWay => "two-way",
});

vocabulary!(ContractKind {
    Invariant => "invariant",
    Interface => "interface",
});

vocabulary!(
    /// How the approval's plan review was settled.
    ReviewOutcome {
        Ran => "ran",
        SkippedLowRisk => "skipped-low-risk",
        SkippedFlag => "skipped-flag",
    }
);

vocabulary!(
    /// How a run closed. `plan run finish` writes the first three and
    /// `plan run abandon` the last.
    RunOutcome {
        Ok => "ok",
        Errors => "errors",
        Stopped => "stopped",
        Abandoned => "abandoned",
    }
);

/// The record kinds whose ids are minted, each under its own number sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordKind {
    Decision,
    Contract,
    Handover,
}

impl RecordKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Decision => "D",
            Self::Contract => "C",
            Self::Handover => "H",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanStore {
    pub(crate) schema_version: i64,
    /// Bare TOML date, absent until the first mutation stamps it.
    pub(crate) last_updated: Option<Datetime>,
    pub(crate) binding: Binding,
    /// The highest number ever assigned per id prefix. No plan verb removes a
    /// record, so the mark matters after a `--raw-store` repair deletes a row:
    /// that row's id is never minted again.
    pub(crate) id_high_water: BTreeMap<String, u64>,
    pub(crate) decisions: Vec<Decision>,
    pub(crate) contracts: Vec<Contract>,
    pub(crate) approvals: Vec<Approval>,
    pub(crate) runs: Vec<Run>,
    pub(crate) handovers: Vec<Handover>,
}

/// The branch and scope a draft holds here until approval, so that it matches
/// no flow-resolver step before then.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Binding {
    pub(crate) branch: String,
    pub(crate) scope: Vec<String>,
}

impl Binding {
    fn is_empty(&self) -> bool {
        self.branch.is_empty() && self.scope.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Decision {
    pub(crate) id: String,
    pub(crate) decision: String,
    pub(crate) why: String,
    pub(crate) provenance: Provenance,
    pub(crate) door: Door,
    /// Read through [`Bind::parse`].
    pub(crate) binds: Vec<String>,
    pub(crate) rejected: Vec<Rejected>,
    pub(crate) evidence: String,
    pub(crate) if_false: String,
    pub(crate) revisit_if: String,
    pub(crate) check: String,
    pub(crate) surfaced: bool,
    pub(crate) vetoed: bool,
}

impl Decision {
    /// A surfaced decision stays unanswered while it is still `agent-assumed`.
    pub(crate) fn is_unanswered(&self) -> bool {
        self.surfaced && self.provenance == Provenance::AgentAssumed
    }
}

/// What one `Decision::binds` entry names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Bind {
    Task(u32),
    /// Normalised through `normal_path`, the form a row's `files` compare in.
    Path(String),
}

impl Bind {
    /// An all-digit entry names a task and nothing else, so a file named `3`
    /// cannot satisfy it. `None` for an entry naming nothing: an empty one, or
    /// digits too large for a task id.
    pub(crate) fn parse(entry: &str) -> Option<Self> {
        if entry.is_empty() {
            return None;
        }
        if entry.bytes().all(|byte| byte.is_ascii_digit()) {
            return entry.parse().ok().map(Self::Task);
        }
        Some(Self::Path(normal_path(entry)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rejected {
    pub(crate) option: String,
    pub(crate) because: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Contract {
    pub(crate) id: String,
    pub(crate) kind: ContractKind,
    pub(crate) claim: String,
    pub(crate) check: String,
    pub(crate) tasks: Vec<u32>,
    pub(crate) provides: String,
    pub(crate) consumes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Approval {
    pub(crate) commit: String,
    pub(crate) at: Datetime,
    pub(crate) brief_sha256: String,
    pub(crate) by: String,
    pub(crate) review: ReviewOutcome,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) step: String,
    /// Empty on a step with no Workflow, and omitted from the TOML while empty.
    pub(crate) workflow: String,
    /// Omitted from the TOML while empty.
    pub(crate) args_hash: String,
    pub(crate) run_id: String,
    /// The launching session. Omitted from the TOML while empty.
    pub(crate) session: String,
    pub(crate) started: Datetime,
    /// `None` writes `finished = ""`: an open run keeps the key it will fill.
    pub(crate) finished: Option<Datetime>,
    /// Set when the step was not run.
    pub(crate) skip_reason: String,
    /// `None` on an open or skipped row, and on one closed before the key
    /// existed. Omitted from the TOML while `None`.
    pub(crate) outcome: Option<RunOutcome>,
    /// Why the run was abandoned. Omitted from the TOML while empty.
    pub(crate) reason: String,
}

/// Where a `[[run]]` row stands, derived from its fields and never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunState {
    Skipped,
    Abandoned,
    Open,
    Finished,
}

impl RunState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Abandoned => "abandoned",
            Self::Open => "open",
            Self::Finished => "finished",
        }
    }
}

impl Run {
    /// A skip reason, a finish time or an outcome each closes a row, so a
    /// hand-edited row carrying an outcome and no `finished` reads as closed
    /// everywhere. A whitespace-only skip reason skips nothing.
    pub(crate) fn state(&self) -> RunState {
        if !self.skip_reason.trim().is_empty() {
            RunState::Skipped
        } else if self.outcome == Some(RunOutcome::Abandoned) {
            RunState::Abandoned
        } else if self.finished.is_none() && self.outcome.is_none() {
            RunState::Open
        } else {
            RunState::Finished
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.state() == RunState::Open
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Handover {
    pub(crate) id: String,
    pub(crate) command: String,
    pub(crate) source_task: u32,
    pub(crate) done: bool,
}

impl Default for PlanStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_updated: None,
            binding: Binding::default(),
            id_high_water: BTreeMap::new(),
            decisions: Vec::new(),
            contracts: Vec::new(),
            approvals: Vec::new(),
            runs: Vec::new(),
            handovers: Vec::new(),
        }
    }
}

impl PlanStore {
    /// One past the highest number `kind`'s rows carry or its prefix has ever
    /// been assigned, raising the mark so that the id outlives its row.
    pub(crate) fn mint_id(&mut self, kind: RecordKind) -> String {
        let prefix = kind.prefix();
        let ids: Vec<&str> = match kind {
            RecordKind::Decision => self.decisions.iter().map(|r| r.id.as_str()).collect(),
            RecordKind::Contract => self.contracts.iter().map(|r| r.id.as_str()).collect(),
            RecordKind::Handover => self.handovers.iter().map(|r| r.id.as_str()).collect(),
        };
        let top = ids
            .iter()
            .filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        let mark = self.id_high_water.get(prefix).copied().unwrap_or(0);
        let next = top.max(mark).saturating_add(1);
        self.id_high_water.insert(prefix.to_string(), next);
        format!("{prefix}{next}")
    }
}

pub(crate) fn from_toml(doc: &TomlValue) -> Result<PlanStore> {
    let root = doc
        .as_table()
        .ok_or_else(|| anyhow!("plan store root is not a table"))?;

    let schema_version = root
        .get("schema_version")
        .and_then(TomlValue::as_integer)
        .unwrap_or(SCHEMA_VERSION);
    // A newer store holds keys this shape drops, and the next write would
    // re-emit it still stamped with its own version, so the loss would be
    // invisible to the binary that wrote it.
    if schema_version > SCHEMA_VERSION {
        return Err(anyhow!(
            "plan store schema_version {schema_version} is newer than the supported \
             {SCHEMA_VERSION} — upgrade tomlctl to read this store"
        ));
    }

    Ok(PlanStore {
        schema_version,
        last_updated: root.get("last_updated").and_then(as_datetime),
        binding: match root.get("binding") {
            Some(value) => binding_from_toml(value)?,
            None => Binding::default(),
        },
        id_high_water: id_high_water_from_toml(root),
        decisions: records(root, "decision", decision_from_toml)?,
        contracts: records(root, "contract", contract_from_toml)?,
        approvals: records(root, "approval", approval_from_toml)?,
        runs: records(root, "run", run_from_toml)?,
        handovers: records(root, "handover", handover_from_toml)?,
    })
}

pub(crate) fn to_toml(store: &PlanStore) -> TomlValue {
    let PlanStore {
        schema_version,
        last_updated,
        binding,
        id_high_water,
        decisions,
        contracts,
        approvals,
        runs,
        handovers,
    } = store;

    let mut root = Table::new();
    root.insert(
        "schema_version".to_string(),
        TomlValue::Integer(*schema_version),
    );
    if let Some(date) = last_updated {
        root.insert("last_updated".to_string(), TomlValue::Datetime(*date));
    }
    if !binding.is_empty() {
        root.insert("binding".to_string(), binding_to_toml(binding));
    }
    if !id_high_water.is_empty() {
        root.insert(
            "id_high_water".to_string(),
            id_high_water_to_toml(id_high_water),
        );
    }
    insert_records(&mut root, "decision", decisions, decision_to_toml);
    insert_records(&mut root, "contract", contracts, contract_to_toml);
    insert_records(&mut root, "approval", approvals, approval_to_toml);
    insert_records(&mut root, "run", runs, run_to_toml);
    insert_records(&mut root, "handover", handovers, handover_to_toml);
    TomlValue::Table(root)
}

fn records<T>(
    root: &Table,
    key: &str,
    parse: impl Fn(&Table, &str) -> Result<T>,
) -> Result<Vec<T>> {
    let Some(raw) = root.get(key) else {
        return Ok(Vec::new());
    };
    let rows = raw
        .as_array()
        .ok_or_else(|| anyhow!("`{key}` is not an array of tables"))?;
    rows.iter()
        .enumerate()
        .map(|(index, value)| {
            let context = format!("{key}[{index}]");
            let table = value
                .as_table()
                .ok_or_else(|| anyhow!("{context} is not a table"))?;
            parse(table, &context)
        })
        .collect()
}

fn insert_records<T>(root: &mut Table, key: &str, rows: &[T], render: impl Fn(&T) -> TomlValue) {
    if !rows.is_empty() {
        root.insert(
            key.to_string(),
            TomlValue::Array(rows.iter().map(render).collect()),
        );
    }
}

fn binding_from_toml(value: &TomlValue) -> Result<Binding> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("`binding` is not a table"))?;
    Ok(Binding {
        branch: str_or(table, "branch"),
        scope: str_array(table, "scope"),
    })
}

fn binding_to_toml(binding: &Binding) -> TomlValue {
    let Binding { branch, scope } = binding;
    let mut table = Table::new();
    table.insert("branch".to_string(), TomlValue::String(branch.clone()));
    table.insert("scope".to_string(), str_arr_value(scope));
    TomlValue::Table(table)
}

/// A mark that is not a non-negative integer reads as absent, matching how the
/// `items` verbs read this table.
fn id_high_water_from_toml(root: &Table) -> BTreeMap<String, u64> {
    root.get("id_high_water")
        .and_then(TomlValue::as_table)
        .map(|table| {
            table
                .iter()
                .filter_map(|(prefix, mark)| {
                    let mark = u64::try_from(mark.as_integer()?).ok()?;
                    Some((prefix.clone(), mark))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn id_high_water_to_toml(marks: &BTreeMap<String, u64>) -> TomlValue {
    let mut table = Table::new();
    for (prefix, mark) in marks {
        table.insert(
            prefix.clone(),
            TomlValue::Integer(i64::try_from(*mark).unwrap_or(i64::MAX)),
        );
    }
    TomlValue::Table(table)
}

fn decision_from_toml(table: &Table, context: &str) -> Result<Decision> {
    let id = required_str(table, "id", context)?;
    let context = format!("decision {id}");
    Ok(Decision {
        decision: lf(&str_or(table, "decision")),
        why: lf(&str_or(table, "why")),
        provenance: vocab(table, "provenance", &context)?,
        door: vocab(table, "door", &context)?,
        binds: str_array(table, "binds"),
        rejected: rejected_from_toml(table, &context)?,
        evidence: lf(&str_or(table, "evidence")),
        if_false: lf(&str_or(table, "if_false")),
        revisit_if: lf(&str_or(table, "revisit_if")),
        check: str_or(table, "check"),
        surfaced: bool_or_false(table, "surfaced", &context)?,
        vetoed: bool_or_false(table, "vetoed", &context)?,
        id,
    })
}

fn decision_to_toml(decision: &Decision) -> TomlValue {
    let Decision {
        id,
        decision,
        why,
        provenance,
        door,
        binds,
        rejected,
        evidence,
        if_false,
        revisit_if,
        check,
        surfaced,
        vetoed,
    } = decision;
    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::String(id.clone()));
    table.insert("decision".to_string(), TomlValue::String(lf(decision)));
    table.insert("why".to_string(), TomlValue::String(lf(why)));
    table.insert(
        "provenance".to_string(),
        TomlValue::String(provenance.as_str().to_string()),
    );
    table.insert(
        "door".to_string(),
        TomlValue::String(door.as_str().to_string()),
    );
    table.insert("binds".to_string(), str_arr_value(binds));
    table.insert(
        "rejected".to_string(),
        TomlValue::Array(rejected.iter().map(rejected_to_toml).collect()),
    );
    table.insert("evidence".to_string(), TomlValue::String(lf(evidence)));
    table.insert("if_false".to_string(), TomlValue::String(lf(if_false)));
    table.insert("revisit_if".to_string(), TomlValue::String(lf(revisit_if)));
    table.insert("check".to_string(), TomlValue::String(check.clone()));
    table.insert("surfaced".to_string(), TomlValue::Boolean(*surfaced));
    table.insert("vetoed".to_string(), TomlValue::Boolean(*vetoed));
    TomlValue::Table(table)
}

fn rejected_from_toml(table: &Table, context: &str) -> Result<Vec<Rejected>> {
    let Some(raw) = table.get("rejected") else {
        return Ok(Vec::new());
    };
    let entries = raw
        .as_array()
        .ok_or_else(|| anyhow!("{context}: `rejected` is not an array"))?;
    entries
        .iter()
        .map(|entry| {
            let entry = entry.as_table().ok_or_else(|| {
                anyhow!("{context}: `rejected` holds a non-table entry `{entry}`")
            })?;
            Ok(Rejected {
                option: lf(&str_or(entry, "option")),
                because: lf(&str_or(entry, "because")),
            })
        })
        .collect()
}

fn rejected_to_toml(entry: &Rejected) -> TomlValue {
    let Rejected { option, because } = entry;
    let mut table = Table::new();
    table.insert("option".to_string(), TomlValue::String(lf(option)));
    table.insert("because".to_string(), TomlValue::String(lf(because)));
    TomlValue::Table(table)
}

fn contract_from_toml(table: &Table, context: &str) -> Result<Contract> {
    let id = required_str(table, "id", context)?;
    let context = format!("contract {id}");
    Ok(Contract {
        kind: vocab(table, "kind", &context)?,
        claim: lf(&str_or(table, "claim")),
        check: str_or(table, "check"),
        tasks: id_array(table, "tasks", &context)?,
        provides: lf(&str_or(table, "provides")),
        consumes: lf(&str_or(table, "consumes")),
        id,
    })
}

fn contract_to_toml(contract: &Contract) -> TomlValue {
    let Contract {
        id,
        kind,
        claim,
        check,
        tasks,
        provides,
        consumes,
    } = contract;
    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::String(id.clone()));
    table.insert(
        "kind".to_string(),
        TomlValue::String(kind.as_str().to_string()),
    );
    table.insert("claim".to_string(), TomlValue::String(lf(claim)));
    table.insert("check".to_string(), TomlValue::String(check.clone()));
    table.insert(
        "tasks".to_string(),
        TomlValue::Array(
            tasks
                .iter()
                .map(|id| TomlValue::Integer(i64::from(*id)))
                .collect(),
        ),
    );
    table.insert("provides".to_string(), TomlValue::String(lf(provides)));
    table.insert("consumes".to_string(), TomlValue::String(lf(consumes)));
    TomlValue::Table(table)
}

fn approval_from_toml(table: &Table, context: &str) -> Result<Approval> {
    let commit = required_str(table, "commit", context)?;
    Ok(Approval {
        at: required_datetime(table, "at", context)?,
        brief_sha256: str_or(table, "brief_sha256"),
        by: str_or(table, "by"),
        review: vocab(table, "review", context)?,
        reason: lf(&str_or(table, "reason")),
        commit,
    })
}

fn approval_to_toml(approval: &Approval) -> TomlValue {
    let Approval {
        commit,
        at,
        brief_sha256,
        by,
        review,
        reason,
    } = approval;
    let mut table = Table::new();
    table.insert("commit".to_string(), TomlValue::String(commit.clone()));
    table.insert("at".to_string(), TomlValue::Datetime(*at));
    table.insert(
        "brief_sha256".to_string(),
        TomlValue::String(brief_sha256.clone()),
    );
    table.insert("by".to_string(), TomlValue::String(by.clone()));
    table.insert(
        "review".to_string(),
        TomlValue::String(review.as_str().to_string()),
    );
    table.insert("reason".to_string(), TomlValue::String(lf(reason)));
    TomlValue::Table(table)
}

fn run_from_toml(table: &Table, context: &str) -> Result<Run> {
    Ok(Run {
        step: required_str(table, "step", context)?,
        workflow: str_or(table, "workflow"),
        args_hash: str_or(table, "args_hash"),
        run_id: str_or(table, "run_id"),
        session: str_or(table, "session"),
        started: required_datetime(table, "started", context)?,
        finished: match table.get("finished") {
            None => None,
            Some(TomlValue::String(raw)) if raw.is_empty() => None,
            Some(raw) => Some(
                as_datetime(raw)
                    .ok_or_else(|| anyhow!("{context}: `finished` is not a datetime: `{raw}`"))?,
            ),
        },
        skip_reason: lf(&str_or(table, "skip_reason")),
        outcome: match table.get("outcome") {
            None => None,
            Some(_) => Some(vocab(table, "outcome", context)?),
        },
        reason: lf(&str_or(table, "reason")),
    })
}

fn run_to_toml(run: &Run) -> TomlValue {
    let Run {
        step,
        workflow,
        args_hash,
        run_id,
        session,
        started,
        finished,
        skip_reason,
        outcome,
        reason,
    } = run;
    let mut table = Table::new();
    table.insert("step".to_string(), TomlValue::String(step.clone()));
    if !workflow.is_empty() {
        table.insert("workflow".to_string(), TomlValue::String(workflow.clone()));
    }
    if !args_hash.is_empty() {
        table.insert(
            "args_hash".to_string(),
            TomlValue::String(args_hash.clone()),
        );
    }
    table.insert("run_id".to_string(), TomlValue::String(run_id.clone()));
    if !session.is_empty() {
        table.insert("session".to_string(), TomlValue::String(session.clone()));
    }
    table.insert("started".to_string(), TomlValue::Datetime(*started));
    table.insert(
        "finished".to_string(),
        match finished {
            Some(at) => TomlValue::Datetime(*at),
            None => TomlValue::String(String::new()),
        },
    );
    table.insert(
        "skip_reason".to_string(),
        TomlValue::String(lf(skip_reason)),
    );
    if let Some(outcome) = outcome {
        table.insert(
            "outcome".to_string(),
            TomlValue::String(outcome.as_str().to_string()),
        );
    }
    if !reason.is_empty() {
        table.insert("reason".to_string(), TomlValue::String(lf(reason)));
    }
    TomlValue::Table(table)
}

fn handover_from_toml(table: &Table, context: &str) -> Result<Handover> {
    let id = required_str(table, "id", context)?;
    let context = format!("handover {id}");
    let source_task = table
        .get("source_task")
        .and_then(TomlValue::as_integer)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| anyhow!("{context} has no non-negative integer `source_task`"))?;
    Ok(Handover {
        command: str_or(table, "command"),
        source_task,
        done: bool_or_false(table, "done", &context)?,
        id,
    })
}

fn handover_to_toml(handover: &Handover) -> TomlValue {
    let Handover {
        id,
        command,
        source_task,
        done,
    } = handover;
    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::String(id.clone()));
    table.insert("command".to_string(), TomlValue::String(command.clone()));
    table.insert(
        "source_task".to_string(),
        TomlValue::Integer(i64::from(*source_task)),
    );
    table.insert("done".to_string(), TomlValue::Boolean(*done));
    TomlValue::Table(table)
}

fn str_or(table: &Table, key: &str) -> String {
    table
        .get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
        .to_string()
}

fn required_str(table: &Table, key: &str, context: &str) -> Result<String> {
    let value = str_or(table, key);
    if value.is_empty() {
        return Err(anyhow!("{context}: `{key}` is missing or empty"));
    }
    Ok(value)
}

fn required_datetime(table: &Table, key: &str, context: &str) -> Result<Datetime> {
    table
        .get(key)
        .and_then(as_datetime)
        .ok_or_else(|| anyhow!("{context}: `{key}` is missing or not a datetime"))
}

/// An absent vocabulary field is refused rather than defaulted: a store cannot
/// claim who stated a decision, or how reversible it is, without having said.
fn vocab<T: Vocabulary>(table: &Table, key: &str, context: &str) -> Result<T> {
    let raw = table
        .get(key)
        .ok_or_else(|| anyhow!("{context}: `{key}` is missing"))?
        .as_str()
        .ok_or_else(|| anyhow!("{context}: `{key}` is not a string"))?;
    T::parse(raw).ok_or_else(|| {
        anyhow!(
            "{context}: unknown {key} `{raw}` — expected one of {}",
            T::VOCABULARY.join(", ")
        )
    })
}

/// A present non-boolean is refused: `surfaced = "true"` read as `false` would
/// slip a decision past approval's answered check.
fn bool_or_false(table: &Table, key: &str, context: &str) -> Result<bool> {
    match table.get(key) {
        None => Ok(false),
        Some(raw) => raw
            .as_bool()
            .ok_or_else(|| anyhow!("{context}: `{key}` is not a boolean")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn at(raw: &str) -> Datetime {
        raw.parse().expect("the datetime parses")
    }

    pub(crate) fn fixture() -> PlanStore {
        PlanStore {
            schema_version: SCHEMA_VERSION,
            last_updated: Some(at("2020-01-02")),
            binding: Binding {
                branch: "main".to_string(),
                scope: vec!["src/**".to_string()],
            },
            id_high_water: BTreeMap::from([("D".to_string(), 7)]),
            decisions: vec![Decision {
                id: "D1".to_string(),
                decision: "Use the backoff helper.".to_string(),
                why: "It already exists.".to_string(),
                provenance: Provenance::AgentAssumed,
                door: Door::OneWay,
                binds: vec!["3".to_string(), "src/a.rs".to_string()],
                rejected: vec![Rejected {
                    option: "A new helper".to_string(),
                    because: "It duplicates the first.".to_string(),
                }],
                evidence: "Read src/a.rs.".to_string(),
                if_false: "Task 3 grows.".to_string(),
                revisit_if: "The helper goes.".to_string(),
                check: "grep -q backoff src/a.rs".to_string(),
                surfaced: true,
                vetoed: true,
            }],
            contracts: vec![Contract {
                id: "C1".to_string(),
                kind: ContractKind::Interface,
                claim: "Both sides agree on the header.".to_string(),
                check: "cargo test header".to_string(),
                tasks: vec![3, 5],
                provides: "parse".to_string(),
                consumes: "retry".to_string(),
            }],
            approvals: vec![Approval {
                commit: "abc1234".to_string(),
                at: at("2026-11-05T12:00:00Z"),
                brief_sha256: "feed".to_string(),
                by: "user".to_string(),
                review: ReviewOutcome::SkippedLowRisk,
                reason: "One-file fix.".to_string(),
            }],
            runs: vec![
                Run {
                    step: "investigate".to_string(),
                    workflow: "plan-v2-investigate".to_string(),
                    args_hash: "beef".to_string(),
                    run_id: "wf_1".to_string(),
                    session: "sess_1".to_string(),
                    started: at("2026-11-05T12:00:00Z"),
                    finished: Some(at("2026-11-05T12:30:00Z")),
                    skip_reason: String::new(),
                    outcome: None,
                    reason: String::new(),
                },
                Run {
                    step: "review".to_string(),
                    workflow: String::new(),
                    args_hash: String::new(),
                    run_id: String::new(),
                    session: String::new(),
                    started: at("2026-11-05T13:00:00Z"),
                    finished: None,
                    skip_reason: "Low risk.".to_string(),
                    outcome: None,
                    reason: String::new(),
                },
            ],
            handovers: vec![Handover {
                id: "H1".to_string(),
                command: "cargo install --path tomlctl".to_string(),
                source_task: 3,
                done: true,
            }],
        }
    }

    fn record_mut<'a>(doc: &'a mut TomlValue, key: &str) -> &'a mut Table {
        doc.as_table_mut()
            .expect("root table")
            .get_mut(key)
            .expect("the array is present")
            .as_array_mut()
            .expect("an array")[0]
            .as_table_mut()
            .expect("a row table")
    }

    #[test]
    fn store_round_trips_through_toml() {
        let store = fixture();
        assert_eq!(from_toml(&to_toml(&store)).expect("parses"), store);
    }

    /// The in-memory round trip never meets the serialiser, which is what a
    /// table array nested between a decision's scalars has to survive.
    #[test]
    fn store_round_trips_through_rendered_text() {
        let store = fixture();
        let text = toml::to_string_pretty(&to_toml(&store)).expect("renders");
        let doc: TomlValue = toml::from_str(&text).expect("the rendered text parses");
        assert_eq!(from_toml(&doc).expect("parses"), store);
    }

    #[test]
    fn an_empty_store_writes_only_its_schema_version() {
        let doc = to_toml(&PlanStore::default());
        let keys: Vec<&str> = doc
            .as_table()
            .expect("root table")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["schema_version"]);
        assert_eq!(from_toml(&doc).expect("parses"), PlanStore::default());
    }

    #[test]
    fn an_absent_schema_version_reads_as_current() {
        let doc = TomlValue::Table(Table::new());
        assert_eq!(
            from_toml(&doc).expect("parses").schema_version,
            SCHEMA_VERSION
        );
    }

    #[test]
    fn newer_schema_version_is_refused() {
        let mut raw = to_toml(&fixture());
        raw.as_table_mut().expect("root table").insert(
            "schema_version".to_string(),
            TomlValue::Integer(SCHEMA_VERSION + 1),
        );
        let message = from_toml(&raw)
            .expect_err("newer schema rejected")
            .to_string();
        assert!(
            message.contains(&(SCHEMA_VERSION + 1).to_string()),
            "{message}"
        );
        assert!(message.contains("upgrade tomlctl"), "{message}");
    }

    #[test]
    fn unknown_keys_are_dropped() {
        let mut raw = to_toml(&fixture());
        raw.as_table_mut()
            .expect("root table")
            .insert("stray".to_string(), TomlValue::Boolean(true));
        record_mut(&mut raw, "decision").insert("stray".to_string(), TomlValue::Boolean(true));
        let written = to_toml(&from_toml(&raw).expect("parses"));
        assert_eq!(written, to_toml(&fixture()));
    }

    #[test]
    fn an_unknown_vocabulary_value_names_the_record_and_the_choices() {
        let cases = [
            ("decision", "provenance", "guessed", "agent-assumed"),
            ("decision", "door", "revolving", "one-way"),
            ("contract", "kind", "promise", "invariant"),
            ("approval", "review", "waved", "skipped-flag"),
        ];
        for (array, key, bad, listed) in cases {
            let mut raw = to_toml(&fixture());
            record_mut(&mut raw, array).insert(key.to_string(), TomlValue::String(bad.to_string()));
            let message = from_toml(&raw).expect_err(bad).to_string();
            assert!(message.contains(array), "{message}");
            assert!(message.contains(bad), "{message}");
            assert!(message.contains(listed), "{message}");
        }
    }

    #[test]
    fn a_missing_required_key_is_refused() {
        let cases = [
            ("decision", "id"),
            ("decision", "provenance"),
            ("decision", "door"),
            ("contract", "id"),
            ("contract", "kind"),
            ("approval", "commit"),
            ("approval", "at"),
            ("approval", "review"),
            ("run", "step"),
            ("run", "started"),
            ("handover", "id"),
            ("handover", "source_task"),
        ];
        for (array, key) in cases {
            let mut raw = to_toml(&fixture());
            record_mut(&mut raw, array).remove(key);
            let message = from_toml(&raw)
                .expect_err(&format!("{array}.{key}"))
                .to_string();
            assert!(message.contains(key), "{array}.{key}: {message}");
        }
    }

    #[test]
    fn a_quoted_flag_is_refused_rather_than_read_as_false() {
        for (array, key) in [
            ("decision", "surfaced"),
            ("decision", "vetoed"),
            ("handover", "done"),
        ] {
            let mut raw = to_toml(&fixture());
            record_mut(&mut raw, array)
                .insert(key.to_string(), TomlValue::String("true".to_string()));
            let message = from_toml(&raw)
                .expect_err(&format!("{array}.{key}"))
                .to_string();
            assert!(message.contains(key), "{message}");
        }
    }

    #[test]
    fn an_open_run_writes_an_empty_finished_and_reads_it_as_none() {
        let store = fixture();
        let mut doc = to_toml(&store);
        let open = &doc["run"][1];
        assert_eq!(open.get("finished").and_then(TomlValue::as_str), Some(""));

        let table = doc
            .as_table_mut()
            .expect("root table")
            .get_mut("run")
            .expect("runs")
            .as_array_mut()
            .expect("an array")[1]
            .as_table_mut()
            .expect("a row table");
        table.remove("finished");
        assert_eq!(from_toml(&doc).expect("parses").runs[1].finished, None);
    }

    #[test]
    fn a_run_outcome_and_reason_round_trip_and_stay_absent_while_unset() {
        let mut store = fixture();
        let doc = to_toml(&store);
        assert!(doc["run"][0].get("outcome").is_none(), "{doc}");
        assert!(doc["run"][0].get("reason").is_none(), "{doc}");
        assert_eq!(from_toml(&doc).expect("parses").runs[0].outcome, None);

        store.runs[0].outcome = Some(RunOutcome::Abandoned);
        store.runs[0].reason = "The session died.".to_string();
        let read = from_toml(&to_toml(&store)).expect("parses");
        assert_eq!(read.runs[0].outcome, Some(RunOutcome::Abandoned));
        assert_eq!(read.runs[0].reason, "The session died.");
    }

    #[test]
    fn a_run_omits_workflow_args_hash_and_session_while_empty() {
        let doc = to_toml(&fixture());
        for key in ["workflow", "args_hash", "session"] {
            assert!(doc["run"][0].get(key).is_some(), "{key}: {doc}");
            assert!(doc["run"][1].get(key).is_none(), "{key}: {doc}");
        }
    }

    #[test]
    fn empty_workflow_keys_in_a_stored_run_read_as_unset() {
        let mut raw = to_toml(&fixture());
        let row = record_mut(&mut raw, "run");
        row.insert("workflow".to_string(), TomlValue::String(String::new()));
        row.insert("args_hash".to_string(), TomlValue::String(String::new()));
        let read = from_toml(&raw).expect("parses");
        assert_eq!(read.runs[0].workflow, "");
        assert_eq!(read.runs[0].args_hash, "");
        assert!(to_toml(&read)["run"][0].get("workflow").is_none());
    }

    #[test]
    fn a_run_is_open_only_with_no_finished_time_skip_reason_or_outcome() {
        let open = Run {
            skip_reason: String::new(),
            ..fixture().runs[1].clone()
        };
        assert!(open.is_open());
        assert!(
            !Run {
                finished: Some(at("2026-11-05T13:30:00Z")),
                ..open.clone()
            }
            .is_open()
        );
        assert!(
            !Run {
                skip_reason: "Low risk.".to_string(),
                ..open.clone()
            }
            .is_open()
        );
        assert!(
            !Run {
                outcome: Some(RunOutcome::Abandoned),
                ..open
            }
            .is_open()
        );
    }

    #[test]
    fn an_unknown_run_outcome_is_refused() {
        let mut raw = to_toml(&fixture());
        record_mut(&mut raw, "run").insert(
            "outcome".to_string(),
            TomlValue::String("exploded".to_string()),
        );
        let message = from_toml(&raw).expect_err("refused").to_string();
        assert!(message.contains("exploded"), "{message}");
    }

    #[test]
    fn a_finished_that_is_no_datetime_is_refused() {
        let mut raw = to_toml(&fixture());
        record_mut(&mut raw, "run").insert(
            "finished".to_string(),
            TomlValue::String("soon".to_string()),
        );
        let message = from_toml(&raw).expect_err("refused").to_string();
        assert!(message.contains("soon"), "{message}");
    }

    #[test]
    fn crlf_text_normalises_to_lf_on_both_sides() {
        let mut store = fixture();
        store.decisions[0].why = "One.\r\nTwo.".to_string();
        store.decisions[0].rejected[0].because = "Three.\r\nFour.".to_string();
        let doc = to_toml(&store);
        assert_eq!(doc["decision"][0]["why"].as_str(), Some("One.\nTwo."));
        assert_eq!(
            doc["decision"][0]["rejected"][0]["because"].as_str(),
            Some("Three.\nFour.")
        );

        let mut raw = to_toml(&fixture());
        record_mut(&mut raw, "decision").insert(
            "why".to_string(),
            TomlValue::String("One.\r\nTwo.".to_string()),
        );
        assert_eq!(
            from_toml(&raw).expect("parses").decisions[0].why,
            "One.\nTwo."
        );
    }

    #[test]
    fn minting_runs_per_prefix_from_the_highest_row() {
        let mut store = PlanStore::default();
        assert_eq!(store.mint_id(RecordKind::Decision), "D1");
        assert_eq!(store.mint_id(RecordKind::Contract), "C1");
        assert_eq!(store.mint_id(RecordKind::Handover), "H1");

        let mut store = fixture();
        store.id_high_water.clear();
        assert_eq!(store.mint_id(RecordKind::Decision), "D2");
        assert_eq!(store.mint_id(RecordKind::Contract), "C2");
    }

    #[test]
    fn a_removed_top_id_is_never_minted_again() {
        let mut store = PlanStore::default();
        let first = store.mint_id(RecordKind::Handover);
        store.handovers.push(Handover {
            id: first,
            command: String::new(),
            source_task: 1,
            done: false,
        });
        store.handovers.clear();

        let mut reread = from_toml(&to_toml(&store)).expect("parses");
        assert_eq!(reread.mint_id(RecordKind::Handover), "H2");
    }

    /// `items remove --raw-store` writes this table, so a plan write dropping
    /// it would lose the mark that repair left.
    #[test]
    fn a_mark_left_by_the_items_verbs_survives_a_write() {
        let mut raw = TomlValue::Table(Table::new());
        let mut marks = Table::new();
        marks.insert("D".to_string(), TomlValue::Integer(9));
        raw.as_table_mut()
            .expect("root table")
            .insert("id_high_water".to_string(), TomlValue::Table(marks));

        let mut store = from_toml(&to_toml(&from_toml(&raw).expect("parses"))).expect("parses");
        assert_eq!(store.mint_id(RecordKind::Decision), "D10");
    }
}
