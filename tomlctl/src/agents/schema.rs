//! Store, record and segment types for a flow's `agents.toml`, and their TOML
//! conversions.
//!
//! ```toml
//! schema_version = 1
//! last_updated = 2026-09-28
//! [[agents]]            # one row per (session_id, agent_id)
//! [[agents.segments]]   # one per assignment: task_ids, started_at, ended_at
//! ```
//!
//! `to_toml` inserts keys in declaration order and `preserve_order` holds it,
//! so a write is byte-stable with no sort step. `segments` is a TOML value
//! like any other; the shared writer lays a non-empty array of tables out as
//! `[[agents.segments]]` blocks, and the reader accepts the inline form too.
//! Unknown keys are dropped — the store is tool-owned and every write re-emits
//! it from this shape.

use std::fmt;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use toml::Value as TomlValue;
use toml::map::Map;
use toml::value::Datetime;

pub(crate) const SCHEMA_VERSION: i64 = 1;

/// Basename of the per-flow store, beside the flow's `tasks.toml`.
pub(crate) const STORE_FILE: &str = "agents.toml";

/// Prefix of every minted record id: `A1`, `A2`, ….
const ID_PREFIX: &str = "A";

type Table = Map<String, TomlValue>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentsStore {
    pub(crate) schema_version: i64,
    /// Bare TOML date, absent until the first mutation stamps it.
    pub(crate) last_updated: Option<Datetime>,
    pub(crate) agents: Vec<AgentRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct AgentRecord {
    pub(crate) id: String,
    pub(crate) harness: Harness,
    pub(crate) session_id: String,
    pub(crate) agent_id: String,
    pub(crate) agent_type: String,
    pub(crate) kind: AgentKind,
    /// Teammate name; `""` for a subagent.
    pub(crate) name: String,
    pub(crate) team: String,
    pub(crate) status: AgentStatus,
    /// ISO-8601 timestamps as the hook payload or transcript carried them;
    /// `ended_at` is `""` while the agent has not stopped.
    pub(crate) started_at: String,
    pub(crate) updated_at: String,
    pub(crate) ended_at: String,
    pub(crate) transcript_path: String,
    pub(crate) summary: String,
    pub(crate) context_tokens: u64,
    pub(crate) segments: Vec<Segment>,
}

/// One assignment: the tasks an agent was dispatched on and when. The open
/// segment is the last one, while its `ended_at` is `""`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Segment {
    pub(crate) task_ids: Vec<u32>,
    pub(crate) started_at: String,
    pub(crate) ended_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Harness {
    #[default]
    ClaudeCode,
    Codex,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AgentKind {
    #[default]
    Subagent,
    Teammate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AgentStatus {
    #[default]
    Running,
    Idle,
    Stopped,
}

impl Default for AgentsStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_updated: None,
            agents: Vec::new(),
        }
    }
}

/// `<root>/.claude/flows/<slug>/agents.toml`, anchored at the repo root rather
/// than the cwd: a hook runs from its payload's `cwd`, which can be any
/// subdirectory. The slug passes the same check as a task store's, so it
/// cannot reach outside `.claude/flows/`.
pub(crate) fn agents_path(slug: &str) -> Result<PathBuf> {
    Ok(crate::tasks::resolve_store_path(Some(slug), None)?.with_file_name(STORE_FILE))
}

impl AgentsStore {
    /// `A<max+1>` over every well-formed id, `A1` on an empty store. A gap is
    /// never refilled, so an id stays unique across the store's history.
    pub(crate) fn next_id(&self) -> String {
        let next = self
            .agents
            .iter()
            .filter_map(|record| record.id.strip_prefix(ID_PREFIX)?.parse::<u64>().ok())
            .max()
            .map_or(1, |max| max.saturating_add(1));
        format!("{ID_PREFIX}{next}")
    }

    pub(crate) fn find_mut(
        &mut self,
        session_id: &str,
        agent_id: &str,
    ) -> Option<&mut AgentRecord> {
        self.agents
            .iter_mut()
            .rev()
            .find(|record| record.session_id == session_id && record.agent_id == agent_id)
    }

    /// The latest row for a named teammate in `session_id`. Matches on name
    /// alone: the idle event's team name is deprecated upstream, and a
    /// subagent's empty name never matches.
    pub(crate) fn find_teammate_mut(
        &mut self,
        session_id: &str,
        name: &str,
    ) -> Option<&mut AgentRecord> {
        if name.is_empty() {
            return None;
        }
        self.agents
            .iter_mut()
            .rev()
            .find(|record| record.session_id == session_id && record.name == name)
    }
}

impl AgentRecord {
    pub(crate) fn open_segment_ref(&self) -> Option<&Segment> {
        self.segments
            .last()
            .filter(|segment| segment.ended_at.is_empty())
    }

    /// Start an assignment on `task_ids` at `at`, closing any open one first.
    /// Returns `false` and changes nothing when the open segment already
    /// covers the same set, which is what makes a repeated start event
    /// idempotent. Ids are stored sorted and deduplicated.
    pub(crate) fn open_segment(&mut self, task_ids: &[u32], at: &str) -> bool {
        let task_ids = id_set(task_ids);
        if self
            .open_segment_ref()
            .is_some_and(|open| open.task_ids == task_ids)
        {
            return false;
        }
        self.close_segment(at);
        self.segments.push(Segment {
            task_ids,
            started_at: at.to_string(),
            ended_at: String::new(),
        });
        true
    }

    /// End the open segment at `at`. Returns `false` when none is open.
    pub(crate) fn close_segment(&mut self, at: &str) -> bool {
        match self.segments.last_mut() {
            Some(open) if open.ended_at.is_empty() => {
                open.ended_at = at.to_string();
                true
            }
            _ => false,
        }
    }
}

fn id_set(task_ids: &[u32]) -> Vec<u32> {
    let mut ids = task_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    ids
}

impl Harness {
    pub(crate) const VOCABULARY: &'static [&'static str] = &["claude-code", "codex", "manual"];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Manual => "manual",
        }
    }

    /// Case-sensitive: `Codex` is not `codex`.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "claude-code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

impl AgentKind {
    pub(crate) const VOCABULARY: &'static [&'static str] = &["subagent", "teammate"];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Subagent => "subagent",
            Self::Teammate => "teammate",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "subagent" => Some(Self::Subagent),
            "teammate" => Some(Self::Teammate),
            _ => None,
        }
    }
}

impl AgentStatus {
    pub(crate) const VOCABULARY: &'static [&'static str] = &["running", "idle", "stopped"];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Idle => "idle",
            Self::Stopped => "stopped",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "running" => Some(Self::Running),
            "idle" => Some(Self::Idle),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

impl fmt::Display for Harness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub(crate) fn from_toml(doc: &TomlValue) -> Result<AgentsStore> {
    let root = doc
        .as_table()
        .ok_or_else(|| anyhow!("agents store root is not a table"))?;

    let schema_version = root
        .get("schema_version")
        .and_then(TomlValue::as_integer)
        .unwrap_or(SCHEMA_VERSION);
    // A newer store holds keys this shape drops, and the next write would
    // re-emit it still stamped with its own version.
    if schema_version > SCHEMA_VERSION {
        return Err(anyhow!(
            "agents store schema_version {schema_version} is newer than the supported \
             {SCHEMA_VERSION} — upgrade tomlctl to read this store"
        ));
    }

    let agents = table_array(root, "agents", "")?
        .iter()
        .enumerate()
        .map(|(index, value)| record_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;

    Ok(AgentsStore {
        schema_version,
        last_updated: root.get("last_updated").and_then(as_date),
        agents,
    })
}

pub(crate) fn to_toml(store: &AgentsStore) -> TomlValue {
    let AgentsStore {
        schema_version,
        last_updated,
        agents,
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
        "agents".to_string(),
        TomlValue::Array(agents.iter().map(record_to_toml).collect()),
    );
    TomlValue::Table(root)
}

fn record_from_toml(value: &TomlValue, index: usize) -> Result<AgentRecord> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("agents[{index}] is not a table"))?;
    let id = str_or(table, "id", "");
    if id.is_empty() {
        return Err(anyhow!("agents[{index}] has no `id`"));
    }
    let context = format!("agent {id}");

    let segments = table_array(table, "segments", &context)?
        .iter()
        .enumerate()
        .map(|(position, value)| segment_from_toml(value, position, &context))
        .collect::<Result<Vec<_>>>()?;

    Ok(AgentRecord {
        harness: vocab(
            table,
            "harness",
            &context,
            Harness::parse,
            Harness::VOCABULARY,
        )?,
        session_id: str_or(table, "session_id", ""),
        agent_id: str_or(table, "agent_id", ""),
        agent_type: str_or(table, "agent_type", ""),
        kind: vocab(
            table,
            "kind",
            &context,
            AgentKind::parse,
            AgentKind::VOCABULARY,
        )?,
        name: str_or(table, "name", ""),
        team: str_or(table, "team", ""),
        status: vocab(
            table,
            "status",
            &context,
            AgentStatus::parse,
            AgentStatus::VOCABULARY,
        )?,
        started_at: str_or(table, "started_at", ""),
        updated_at: str_or(table, "updated_at", ""),
        ended_at: str_or(table, "ended_at", ""),
        transcript_path: str_or(table, "transcript_path", ""),
        summary: lf(&str_or(table, "summary", "")),
        // Telemetry: a malformed count reads as zero rather than refusing the
        // whole store.
        context_tokens: table
            .get("context_tokens")
            .and_then(TomlValue::as_integer)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0),
        segments,
        id,
    })
}

fn record_to_toml(record: &AgentRecord) -> TomlValue {
    let AgentRecord {
        id,
        harness,
        session_id,
        agent_id,
        agent_type,
        kind,
        name,
        team,
        status,
        started_at,
        updated_at,
        ended_at,
        transcript_path,
        summary,
        context_tokens,
        segments,
    } = record;

    let mut table = Table::new();
    let mut put = |key: &str, value: &str| {
        table.insert(key.to_string(), TomlValue::String(value.to_string()));
    };
    put("id", id);
    put("harness", harness.as_str());
    put("session_id", session_id);
    put("agent_id", agent_id);
    put("agent_type", agent_type);
    put("kind", kind.as_str());
    put("name", name);
    put("team", team);
    put("status", status.as_str());
    put("started_at", started_at);
    put("updated_at", updated_at);
    put("ended_at", ended_at);
    put("transcript_path", transcript_path);
    put("summary", &lf(summary));
    table.insert(
        "context_tokens".to_string(),
        TomlValue::Integer(i64::try_from(*context_tokens).unwrap_or(i64::MAX)),
    );
    table.insert(
        "segments".to_string(),
        TomlValue::Array(segments.iter().map(segment_to_toml).collect()),
    );
    TomlValue::Table(table)
}

fn segment_from_toml(value: &TomlValue, position: usize, context: &str) -> Result<Segment> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("{context}: segments[{position}] is not a table"))?;
    let task_ids = match table.get("task_ids") {
        None => Vec::new(),
        Some(raw) => raw
            .as_array()
            .ok_or_else(|| anyhow!("{context}: segments[{position}].task_ids is not an array"))?
            .iter()
            .map(|entry| {
                entry
                    .as_integer()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| {
                        anyhow!(
                            "{context}: segments[{position}].task_ids holds a non-id entry `{entry}`"
                        )
                    })
            })
            .collect::<Result<Vec<_>>>()?,
    };
    Ok(Segment {
        task_ids,
        started_at: str_or(table, "started_at", ""),
        ended_at: str_or(table, "ended_at", ""),
    })
}

fn segment_to_toml(segment: &Segment) -> TomlValue {
    let Segment {
        task_ids,
        started_at,
        ended_at,
    } = segment;
    let mut table = Table::new();
    table.insert(
        "task_ids".to_string(),
        TomlValue::Array(
            task_ids
                .iter()
                .map(|id| TomlValue::Integer(i64::from(*id)))
                .collect(),
        ),
    );
    table.insert(
        "started_at".to_string(),
        TomlValue::String(started_at.clone()),
    );
    table.insert("ended_at".to_string(), TomlValue::String(ended_at.clone()));
    TomlValue::Table(table)
}

/// A vocabulary field: absent reads as the type's default, and a present value
/// outside the vocabulary refuses the store, naming the row.
fn vocab<T: Default>(
    table: &Table,
    key: &str,
    context: &str,
    parse: fn(&str) -> Option<T>,
    vocabulary: &[&str],
) -> Result<T> {
    let Some(raw) = table.get(key) else {
        return Ok(T::default());
    };
    let raw = raw
        .as_str()
        .ok_or_else(|| anyhow!("{context}: `{key}` is not a string"))?;
    parse(raw).ok_or_else(|| {
        anyhow!(
            "{context}: unknown {key} `{raw}` — expected one of {}",
            vocabulary.join(", ")
        )
    })
}

/// Collapse CRLF and lone CR to LF, so the writer never falls back to the
/// escaped string form a lone `\r` forces.
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

/// The array under `key`, or an empty slice when absent. A present non-array
/// is an error rather than an empty read, which would look like no records.
fn table_array<'a>(table: &'a Table, key: &str, context: &str) -> Result<&'a [TomlValue]> {
    match table.get(key) {
        None => Ok(&[]),
        Some(raw) => raw
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| match context {
                "" => anyhow!("`{key}` is not an array of tables"),
                _ => anyhow!("{context}: `{key}` is not an array of tables"),
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, agent_id: &str) -> AgentRecord {
        AgentRecord {
            id: id.to_string(),
            session_id: "c3349435".to_string(),
            agent_id: agent_id.to_string(),
            agent_type: "implement-deep".to_string(),
            ..AgentRecord::default()
        }
    }

    fn fixture() -> AgentsStore {
        AgentsStore {
            schema_version: SCHEMA_VERSION,
            last_updated: Some("2026-09-28".parse().expect("bare date parses")),
            agents: vec![
                AgentRecord {
                    status: AgentStatus::Stopped,
                    started_at: "2026-09-28T04:44:22Z".to_string(),
                    updated_at: "2026-09-28T04:52:08Z".to_string(),
                    ended_at: "2026-09-28T04:52:08Z".to_string(),
                    transcript_path: "C:/Users/x/subagents/agent-a007.jsonl".to_string(),
                    summary: "Applied.\nTwo lines.".to_string(),
                    context_tokens: 48_213,
                    segments: vec![Segment {
                        task_ids: vec![16],
                        started_at: "2026-09-28T04:44:22Z".to_string(),
                        ended_at: "2026-09-28T04:52:08Z".to_string(),
                    }],
                    ..record("A1", "a007")
                },
                AgentRecord {
                    harness: Harness::Manual,
                    kind: AgentKind::Teammate,
                    name: "worker-1".to_string(),
                    team: "pool".to_string(),
                    status: AgentStatus::Idle,
                    segments: vec![
                        Segment {
                            task_ids: vec![3, 4],
                            started_at: "t1".to_string(),
                            ended_at: "t2".to_string(),
                        },
                        Segment {
                            task_ids: vec![7],
                            started_at: "t3".to_string(),
                            ended_at: String::new(),
                        },
                    ],
                    ..record("A3", "a0b1")
                },
                record("A4", "a0c2"),
            ],
        }
    }

    fn render(store: &AgentsStore) -> String {
        toml::to_string_pretty(&to_toml(store)).expect("serialises")
    }

    #[test]
    fn store_round_trips_byte_stable() {
        let store = fixture();
        let first = render(&store);
        let reread = from_toml(&toml::from_str(&first).expect("reparses")).expect("reads");
        assert_eq!(reread, store);
        assert_eq!(render(&reread), first);
        assert!(
            first
                .find("schema_version")
                .expect("schema_version written")
                < first.find("last_updated").expect("last_updated written")
        );
        let doc = to_toml(&store);
        let keys: Vec<&str> = doc["agents"][0]
            .as_table()
            .expect("record table")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "harness",
                "session_id",
                "agent_id",
                "agent_type",
                "kind",
                "name",
                "team",
                "status",
                "started_at",
                "updated_at",
                "ended_at",
                "transcript_path",
                "summary",
                "context_tokens",
                "segments",
            ]
        );
    }

    #[test]
    fn inline_segments_and_unknown_keys_read_back() {
        let raw = r#"
            schema_version = 1
            stray = true
            [[agents]]
            id = "A2"
            session_id = "s"
            agent_id = "a"
            extra = "dropped"
            segments = [ { task_ids = [16, 17], started_at = "t0", ended_at = "" } ]
        "#;
        let store = from_toml(&toml::from_str(raw).expect("parses")).expect("reads");
        let row = &store.agents[0];
        assert_eq!(row.harness, Harness::ClaudeCode);
        assert_eq!(row.kind, AgentKind::Subagent);
        assert_eq!(row.status, AgentStatus::Running);
        assert_eq!(row.segments[0].task_ids, vec![16, 17]);
        let written = render(&store);
        assert!(!written.contains("stray"), "{written}");
        assert!(!written.contains("extra"), "{written}");
    }

    #[test]
    fn vocabularies_round_trip_and_reject_unknown_values() {
        for raw in Harness::VOCABULARY {
            assert_eq!(Harness::parse(raw).map(Harness::as_str), Some(*raw));
        }
        for raw in AgentKind::VOCABULARY {
            assert_eq!(AgentKind::parse(raw).map(AgentKind::as_str), Some(*raw));
        }
        for raw in AgentStatus::VOCABULARY {
            assert_eq!(AgentStatus::parse(raw).map(AgentStatus::as_str), Some(*raw));
        }
        assert_eq!(Harness::parse("Codex"), None);
        assert_eq!(Harness::parse("claude"), None);
        assert_eq!(AgentKind::parse("in_process_teammate"), None);
        assert_eq!(AgentStatus::parse("done"), None);

        let mut raw = to_toml(&fixture());
        raw["agents"].as_array_mut().expect("agents array")[1]
            .as_table_mut()
            .expect("record table")
            .insert("status".to_string(), TomlValue::String("done".to_string()));
        let message = from_toml(&raw)
            .expect_err("unknown status rejected")
            .to_string();
        assert!(message.contains("A3"), "{message}");
        assert!(message.contains("done"), "{message}");
    }

    #[test]
    fn next_id_starts_at_one_and_skips_gaps() {
        assert_eq!(AgentsStore::default().next_id(), "A1");
        assert_eq!(fixture().next_id(), "A5");

        let mut store = fixture();
        store.agents.push(record("hand-edited", "zz"));
        store.agents.remove(2);
        assert_eq!(store.next_id(), "A4");
    }

    #[test]
    fn open_segment_is_idempotent_on_the_same_set() {
        let mut row = record("A1", "a");
        assert!(row.open_segment(&[5, 2], "t0"));
        assert!(!row.open_segment(&[2, 5, 5], "t1"));
        assert_eq!(row.segments.len(), 1);
        assert_eq!(row.segments[0].task_ids, vec![2, 5]);
        assert_eq!(row.segments[0].started_at, "t0");

        assert!(row.open_segment(&[9], "t2"));
        assert_eq!(row.segments.len(), 2);
        assert_eq!(row.segments[0].ended_at, "t2");
        assert_eq!(
            row.open_segment_ref().map(|s| s.task_ids.clone()),
            Some(vec![9])
        );

        assert!(row.close_segment("t3"));
        assert!(!row.close_segment("t4"));
        assert_eq!(row.segments[1].ended_at, "t3");
        assert!(row.open_segment_ref().is_none());

        assert!(row.open_segment(&[9], "t5"));
        assert_eq!(row.segments.len(), 3);
    }

    #[test]
    fn lookups_match_session_and_agent_or_teammate_name() {
        let mut store = fixture();
        assert_eq!(
            store.find_mut("c3349435", "a0b1").map(|row| row.id.clone()),
            Some("A3".to_string())
        );
        assert!(store.find_mut("other-session", "a0b1").is_none());
        assert_eq!(
            store
                .find_teammate_mut("c3349435", "worker-1")
                .map(|row| row.id.clone()),
            Some("A3".to_string())
        );
        assert!(store.find_teammate_mut("c3349435", "").is_none());
        assert!(store.find_teammate_mut("c3349435", "worker-2").is_none());
    }

    #[test]
    fn newer_schema_version_is_refused() {
        let mut raw = to_toml(&fixture());
        raw.as_table_mut().expect("root table").insert(
            "schema_version".to_string(),
            TomlValue::Integer(SCHEMA_VERSION + 1),
        );
        assert!(from_toml(&raw).is_err());
    }
}
