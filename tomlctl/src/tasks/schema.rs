//! Store, policy, checkpoint and task-row types, and their TOML conversions.
//!
//! ```toml
//! schema_version = 1
//! last_updated = 2026-09-07
//! plan_path = "docs/plans/<slug>.md"
//! last_import_refs = ["…"]
//! [policy] / [[checkpoints]] / [[items]]
//! ```
//!
//! `to_toml` inserts keys in that order and `preserve_order` holds it, so a
//! write is byte-stable with no sort step. Bodies are LF-normalised on both
//! read and write: `toml`'s writer picks its string form by scanning content,
//! and a lone `\r` is an ASCII control character that forces the escaped
//! `"""` form. Unknown keys are dropped — the store is tool-owned and every
//! write re-emits it from this shape.

use std::fmt;

use anyhow::{Result, anyhow};
use toml::Value as TomlValue;
use toml::map::Map;
use toml::value::Datetime;

pub(crate) const SCHEMA_VERSION: i64 = 1;

type Table = Map<String, TomlValue>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Store {
    pub(crate) schema_version: i64,
    /// Bare TOML date, absent until the first mutation stamps it.
    pub(crate) last_updated: Option<Datetime>,
    pub(crate) plan_path: String,
    /// Ref set of the last import; a row outside it has been renamed or
    /// deleted in the plan.
    pub(crate) last_import_refs: Vec<String>,
    pub(crate) policy: Policy,
    pub(crate) checkpoints: Vec<Checkpoint>,
    pub(crate) items: Vec<TaskRow>,
}

/// `## Execution Policy` as stored. The two vocabulary fields stay `String`
/// so `tasks check` — not the reader — decides what is out of vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Policy {
    pub(crate) checkpoints: String,
    pub(crate) max_parallel: u32,
    pub(crate) commit_granularity: String,
    pub(crate) note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    pub(crate) id: String,
    pub(crate) rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskRow {
    pub(crate) id: u32,
    pub(crate) r#ref: String,
    pub(crate) title: String,
    pub(crate) effort: Effort,
    pub(crate) status: Status,
    /// `""` when the task belongs to no checkpoint group.
    pub(crate) checkpoint: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Status {
    #[default]
    Pending,
    InProgress,
    Done,
    Failed,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effort {
    S,
    M,
    L,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_updated: None,
            plan_path: String::new(),
            last_import_refs: Vec::new(),
            policy: Policy::default(),
            checkpoints: Vec::new(),
            items: Vec::new(),
        }
    }
}

/// Fallback for an absent or partial `[policy]` table. Every field is a
/// value from the documented vocabulary, so a store written before its first
/// import still serialises to a document `tasks check` can accept.
impl Default for Policy {
    fn default() -> Self {
        Self {
            checkpoints: "milestones".to_string(),
            max_parallel: 6,
            commit_granularity: "per-task".to_string(),
            note: String::new(),
        }
    }
}

impl Store {
    pub(crate) fn next_id(&self) -> u32 {
        self.items
            .iter()
            .map(|row| row.id)
            .max()
            .map_or(1, |max| max.saturating_add(1))
    }

    pub(crate) fn find(&self, id: u32) -> Option<&TaskRow> {
        self.items.iter().find(|row| row.id == id)
    }

    pub(crate) fn find_ref(&self, r#ref: &str) -> Option<&TaskRow> {
        self.items.iter().find(|row| row.r#ref == r#ref)
    }
}

impl Status {
    pub(crate) const VOCABULARY: &'static [&'static str] =
        &["pending", "in-progress", "done", "failed", "deferred"];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in-progress",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Deferred => "deferred",
        }
    }

    /// Case-sensitive: `Done` is not `done`.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "in-progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            "deferred" => Some(Self::Deferred),
            _ => None,
        }
    }

    fn parse_in_row(raw: &str, id: u32) -> Result<Self> {
        Self::parse(raw).ok_or_else(|| {
            anyhow!(
                "task {id}: unknown status `{raw}` — expected one of {}",
                Self::VOCABULARY.join(", ")
            )
        })
    }
}

impl Effort {
    pub(crate) const VOCABULARY: &'static [&'static str] = &["S", "M", "L"];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
        }
    }

    /// Case-sensitive: `s` is not `S`.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "S" => Some(Self::S),
            "M" => Some(Self::M),
            "L" => Some(Self::L),
            _ => None,
        }
    }

    fn parse_in_row(raw: &str, id: u32) -> Result<Self> {
        Self::parse(raw).ok_or_else(|| {
            anyhow!(
                "task {id}: unknown effort `{raw}` — expected one of {}",
                Self::VOCABULARY.join(", ")
            )
        })
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub(crate) fn from_toml(doc: &TomlValue) -> Result<Store> {
    let root = doc
        .as_table()
        .ok_or_else(|| anyhow!("tasks store root is not a table"))?;

    let checkpoints = table_array(root, "checkpoints")?
        .iter()
        .enumerate()
        .map(|(index, value)| checkpoint_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    let items = table_array(root, "items")?
        .iter()
        .enumerate()
        .map(|(index, value)| row_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;

    Ok(Store {
        schema_version: root
            .get("schema_version")
            .and_then(TomlValue::as_integer)
            .unwrap_or(SCHEMA_VERSION),
        last_updated: root.get("last_updated").and_then(as_date),
        plan_path: str_or(root, "plan_path", ""),
        last_import_refs: str_array(root, "last_import_refs"),
        policy: match root.get("policy") {
            Some(value) => policy_from_toml(value)?,
            None => Policy::default(),
        },
        checkpoints,
        items,
    })
}

pub(crate) fn to_toml(store: &Store) -> TomlValue {
    let Store {
        schema_version,
        last_updated,
        plan_path,
        last_import_refs,
        policy,
        checkpoints,
        items,
    } = store;

    let mut root = Table::new();
    root.insert(
        "schema_version".to_string(),
        TomlValue::Integer(*schema_version),
    );
    if let Some(date) = last_updated {
        root.insert("last_updated".to_string(), TomlValue::Datetime(*date));
    }
    root.insert(
        "plan_path".to_string(),
        TomlValue::String(plan_path.clone()),
    );
    root.insert(
        "last_import_refs".to_string(),
        str_arr_value(last_import_refs),
    );
    root.insert("policy".to_string(), policy_to_toml(policy));
    root.insert(
        "checkpoints".to_string(),
        TomlValue::Array(checkpoints.iter().map(checkpoint_to_toml).collect()),
    );
    root.insert(
        "items".to_string(),
        TomlValue::Array(items.iter().map(row_to_toml).collect()),
    );
    TomlValue::Table(root)
}

fn policy_from_toml(value: &TomlValue) -> Result<Policy> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("`policy` is not a table"))?;
    let fallback = Policy::default();
    let max_parallel = match table.get("max_parallel") {
        Some(raw) => raw
            .as_integer()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| anyhow!("`policy.max_parallel` is not a non-negative integer"))?,
        None => fallback.max_parallel,
    };
    Ok(Policy {
        checkpoints: str_or(table, "checkpoints", &fallback.checkpoints),
        max_parallel,
        commit_granularity: str_or(table, "commit_granularity", &fallback.commit_granularity),
        note: lf(&str_or(table, "note", "")),
    })
}

fn policy_to_toml(policy: &Policy) -> TomlValue {
    let Policy {
        checkpoints,
        max_parallel,
        commit_granularity,
        note,
    } = policy;
    let mut table = Table::new();
    table.insert(
        "checkpoints".to_string(),
        TomlValue::String(checkpoints.clone()),
    );
    table.insert(
        "max_parallel".to_string(),
        TomlValue::Integer(i64::from(*max_parallel)),
    );
    table.insert(
        "commit_granularity".to_string(),
        TomlValue::String(commit_granularity.clone()),
    );
    table.insert("note".to_string(), TomlValue::String(lf(note)));
    TomlValue::Table(table)
}

fn checkpoint_from_toml(value: &TomlValue, index: usize) -> Result<Checkpoint> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("checkpoints[{index}] is not a table"))?;
    let id = str_or(table, "id", "");
    if id.is_empty() {
        return Err(anyhow!("checkpoints[{index}] has no `id`"));
    }
    Ok(Checkpoint {
        id,
        rationale: lf(&str_or(table, "rationale", "")),
    })
}

fn checkpoint_to_toml(checkpoint: &Checkpoint) -> TomlValue {
    let Checkpoint { id, rationale } = checkpoint;
    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::String(id.clone()));
    table.insert("rationale".to_string(), TomlValue::String(lf(rationale)));
    TomlValue::Table(table)
}

fn row_from_toml(value: &TomlValue, index: usize) -> Result<TaskRow> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("items[{index}] is not a table"))?;
    let id = table
        .get("id")
        .and_then(TomlValue::as_integer)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| anyhow!("items[{index}] has no non-negative integer `id`"))?;

    let r#ref = str_or(table, "ref", "");
    if r#ref.is_empty() {
        return Err(anyhow!("task {id}: `ref` is missing or empty"));
    }
    let title = str_or(table, "title", "");
    if title.is_empty() {
        return Err(anyhow!("task {id}: `title` is missing or empty"));
    }
    let effort = match table.get("effort") {
        Some(raw) => Effort::parse_in_row(
            raw.as_str()
                .ok_or_else(|| anyhow!("task {id}: `effort` is not a string"))?,
            id,
        )?,
        None => return Err(anyhow!("task {id}: `effort` is missing")),
    };
    let status = match table.get("status") {
        Some(raw) => Status::parse_in_row(
            raw.as_str()
                .ok_or_else(|| anyhow!("task {id}: `status` is not a string"))?,
            id,
        )?,
        None => Status::default(),
    };

    Ok(TaskRow {
        id,
        r#ref,
        title,
        effort,
        status,
        checkpoint: str_or(table, "checkpoint", ""),
        files: str_array(table, "files"),
        needs: id_array(table, "needs", id)?,
        coupling: id_array(table, "coupling", id)?,
        deps_note: lf(&str_or(table, "deps_note", "")),
        action: lf(&str_or(table, "action", "")),
        detail: lf(&str_or(table, "detail", "")),
        acceptance: lf(&str_or(table, "acceptance", "")),
        agent: str_or(table, "agent", ""),
        commit: str_or(table, "commit", ""),
    })
}

fn row_to_toml(row: &TaskRow) -> TomlValue {
    let TaskRow {
        id,
        r#ref,
        title,
        effort,
        status,
        checkpoint,
        files,
        needs,
        coupling,
        deps_note,
        action,
        detail,
        acceptance,
        agent,
        commit,
    } = row;

    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::Integer(i64::from(*id)));
    table.insert("ref".to_string(), TomlValue::String(r#ref.clone()));
    table.insert("title".to_string(), TomlValue::String(title.clone()));
    table.insert(
        "effort".to_string(),
        TomlValue::String(effort.as_str().to_string()),
    );
    table.insert(
        "status".to_string(),
        TomlValue::String(status.as_str().to_string()),
    );
    table.insert(
        "checkpoint".to_string(),
        TomlValue::String(checkpoint.clone()),
    );
    table.insert("files".to_string(), str_arr_value(files));
    table.insert("needs".to_string(), id_arr_value(needs));
    table.insert("coupling".to_string(), id_arr_value(coupling));
    table.insert("deps_note".to_string(), TomlValue::String(lf(deps_note)));
    table.insert("action".to_string(), TomlValue::String(lf(action)));
    table.insert("detail".to_string(), TomlValue::String(lf(detail)));
    table.insert("acceptance".to_string(), TomlValue::String(lf(acceptance)));
    table.insert("agent".to_string(), TomlValue::String(agent.clone()));
    table.insert("commit".to_string(), TomlValue::String(commit.clone()));
    TomlValue::Table(table)
}

/// Collapse CRLF and lone CR to LF.
fn lf(body: &str) -> String {
    if body.contains('\r') {
        body.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        body.to_string()
    }
}

/// A bare-date `last_updated`, also accepting the quoted form a hand edit
/// leaves behind. Anything else reads as absent and the next write restamps.
fn as_date(value: &TomlValue) -> Option<Datetime> {
    match value {
        TomlValue::Datetime(date) => Some(*date),
        TomlValue::String(raw) => raw.parse().ok(),
        _ => None,
    }
}

fn str_or(table: &Table, key: &str, fallback: &str) -> String {
    table
        .get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or(fallback)
        .to_string()
}

fn str_array(table: &Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(TomlValue::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(TomlValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn id_array(table: &Table, key: &str, id: u32) -> Result<Vec<u32>> {
    let Some(raw) = table.get(key) else {
        return Ok(Vec::new());
    };
    let arr = raw
        .as_array()
        .ok_or_else(|| anyhow!("task {id}: `{key}` is not an array"))?;
    arr.iter()
        .map(|entry| {
            entry
                .as_integer()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| anyhow!("task {id}: `{key}` holds a non-id entry `{entry}`"))
        })
        .collect()
}

/// The array under `key`, or an empty slice when absent. A present
/// non-array is an error rather than an empty read — silently dropping a
/// hand-edited `items` table would look like an empty store.
fn table_array<'a>(table: &'a Table, key: &str) -> Result<&'a [TomlValue]> {
    match table.get(key) {
        None => Ok(&[]),
        Some(raw) => raw
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| anyhow!("`{key}` is not an array of tables")),
    }
}

fn str_arr_value(values: &[String]) -> TomlValue {
    TomlValue::Array(
        values
            .iter()
            .map(|s| TomlValue::String(s.clone()))
            .collect(),
    )
}

fn id_arr_value(values: &[u32]) -> TomlValue {
    TomlValue::Array(
        values
            .iter()
            .map(|id| TomlValue::Integer(i64::from(*id)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Store {
        Store {
            schema_version: SCHEMA_VERSION,
            last_updated: Some("2026-09-07".parse().expect("bare date parses")),
            plan_path: "docs/plans/whimsical-hugging-puppy.md".to_string(),
            last_import_refs: vec![
                "seed-the-store".to_string(),
                "wire-the-renderer".to_string(),
            ],
            policy: Policy {
                checkpoints: "milestones".to_string(),
                max_parallel: 6,
                commit_granularity: "per-task".to_string(),
                note: "tasks 6 and 7 land in one commit".to_string(),
            },
            checkpoints: vec![Checkpoint {
                id: "A".to_string(),
                rationale: "shared kit and the document panel — buildable alone".to_string(),
            }],
            items: vec![
                TaskRow {
                    id: 1,
                    r#ref: "seed-the-store".to_string(),
                    title: "Seed the store".to_string(),
                    effort: Effort::S,
                    status: Status::Done,
                    checkpoint: "A".to_string(),
                    files: vec!["tomlctl/src/tasks/store.rs".to_string()],
                    needs: Vec::new(),
                    coupling: Vec::new(),
                    deps_note: String::new(),
                    action: "Seed it.".to_string(),
                    detail: String::new(),
                    acceptance: "The seed exists.".to_string(),
                    agent: "implement-lite".to_string(),
                    commit: "0d1bf49".to_string(),
                },
                TaskRow {
                    id: 12,
                    r#ref: "wire-the-renderer".to_string(),
                    title: "Wire the renderer".to_string(),
                    effort: Effort::L,
                    status: Status::InProgress,
                    checkpoint: String::new(),
                    files: vec![
                        "tomlctl/src/tasks/render.rs".to_string(),
                        "tomlctl/src/tasks/markdown.rs".to_string(),
                    ],
                    needs: vec![1],
                    coupling: vec![1],
                    deps_note: "1 is what this task needs".to_string(),
                    action: "First line.\n\nSecond line.".to_string(),
                    detail: "Rewrite the three sections in place.".to_string(),
                    acceptance: "Round-trip holds.".to_string(),
                    agent: String::new(),
                    commit: String::new(),
                },
            ],
        }
    }

    fn row_mut(doc: &mut TomlValue, index: usize) -> &mut Table {
        doc.as_table_mut()
            .expect("root table")
            .get_mut("items")
            .expect("items array")
            .as_array_mut()
            .expect("items is an array")[index]
            .as_table_mut()
            .expect("row table")
    }

    #[test]
    fn store_round_trips_through_toml() {
        let store = fixture();
        assert_eq!(from_toml(&to_toml(&store)).expect("parses"), store);
    }

    #[test]
    fn crlf_bodies_normalise_to_lf() {
        let mut store = fixture();
        store.items[1].action = "First line.\r\nSecond line.".to_string();
        let mut doc = to_toml(&store);
        assert_eq!(
            row_mut(&mut doc, 1)
                .get("action")
                .and_then(TomlValue::as_str),
            Some("First line.\nSecond line.")
        );

        let mut raw = to_toml(&fixture());
        row_mut(&mut raw, 1).insert(
            "action".to_string(),
            TomlValue::String("First line.\r\nSecond line.".to_string()),
        );
        assert_eq!(
            from_toml(&raw).expect("parses").items[1].action,
            "First line.\nSecond line."
        );
    }

    #[test]
    fn unknown_status_names_the_row_id() {
        let mut raw = to_toml(&fixture());
        row_mut(&mut raw, 1).insert("status".to_string(), TomlValue::String("ready".to_string()));
        let message = from_toml(&raw)
            .expect_err("unknown status rejected")
            .to_string();
        assert!(message.contains("12"), "{message}");
        assert!(message.contains("ready"), "{message}");
    }

    #[test]
    fn lookups_and_next_id_span_the_rows() {
        let store = fixture();
        assert_eq!(store.next_id(), 13);
        assert_eq!(
            store.find(12).map(|row| row.r#ref.as_str()),
            Some("wire-the-renderer")
        );
        assert_eq!(store.find(99), None);
        assert_eq!(store.find_ref("seed-the-store").map(|row| row.id), Some(1));
        assert_eq!(Store::default().next_id(), 1);
    }
}
