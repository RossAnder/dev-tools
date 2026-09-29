//! `tomlctl agents record` — turn one harness hook payload into a change to
//! the owning flow's `agents.toml`.
//!
//! [`apply`] is the whole state transition over a loaded store and touches no
//! file; [`record`] resolves the flow, the transcript and the dispatch around
//! it and writes the result under the store's exclusive lock.
//!
//! Output is one JSON object: `{"recorded":true,"slug","event","id","task_ids"}`
//! with `event` one of `start`, `stop`, `idle`, or
//! `{"recorded":false,"reason"}` when the payload names nothing to record.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::{Value as JsonValue, json};

use super::correlate::{self, Dispatch};
use super::schema::{self, AgentKind, AgentRecord, AgentStatus, AgentsStore, Harness, STORE_FILE};
use crate::cli::{WriteIntegrityArgs, write_integrity_opts};
use crate::io;

/// Longest summary kept, in chars.
const SUMMARY_MAX_CHARS: usize = 600;

/// One lifecycle event, already correlated. `task_ids` on `Stop` and `Idle`
/// is the dispatch read again at close time, `None` when none resolved; the
/// transcript lags the hook, so it can correct what the start recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    Start {
        harness: Harness,
        session_id: String,
        agent_id: String,
        agent_type: String,
        kind: AgentKind,
        name: String,
        team: String,
        transcript_path: String,
        task_ids: Vec<u32>,
    },
    Stop {
        harness: Harness,
        session_id: String,
        agent_id: String,
        agent_type: String,
        kind: AgentKind,
        name: String,
        team: String,
        summary: String,
        context_tokens: u64,
        transcript_path: String,
        task_ids: Option<Vec<u32>>,
    },
    Idle {
        session_id: String,
        name: String,
        task_ids: Option<Vec<u32>>,
    },
}

impl Event {
    fn label(&self) -> &'static str {
        match self {
            Self::Start { .. } => "start",
            Self::Stop { .. } => "stop",
            Self::Idle { .. } => "idle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// `changed` is false only for a repeated start on the same task set,
    /// which leaves the store byte-identical and skips the write.
    Recorded {
        event: &'static str,
        id: String,
        task_ids: Vec<u32>,
        changed: bool,
    },
    NotRecorded(&'static str),
}

/// Apply `event` to `store` at timestamp `now`.
pub(crate) fn apply(store: &mut AgentsStore, event: Event, now: &str) -> Outcome {
    let label = event.label();
    match event {
        Event::Start {
            harness,
            session_id,
            agent_id,
            agent_type,
            kind,
            name,
            team,
            transcript_path,
            task_ids,
        } => {
            if store.find_mut(&session_id, &agent_id).is_none() {
                let id = store.next_id();
                store.agents.push(AgentRecord {
                    id,
                    harness,
                    session_id: session_id.clone(),
                    agent_id: agent_id.clone(),
                    started_at: now.to_string(),
                    ..AgentRecord::default()
                });
            }
            let row = store
                .find_mut(&session_id, &agent_id)
                .expect("row exists: found or just pushed");
            let opened = row.open_segment(&task_ids, now);
            let changed = opened || row.status != AgentStatus::Running;
            if changed {
                fill(&mut row.agent_type, agent_type);
                fill(&mut row.transcript_path, transcript_path);
                fill(&mut row.name, name);
                fill(&mut row.team, team);
                if kind == AgentKind::Teammate {
                    row.kind = kind;
                }
                row.status = AgentStatus::Running;
                row.ended_at.clear();
                row.updated_at = now.to_string();
            }
            recorded(label, row, changed)
        }
        Event::Stop {
            harness,
            session_id,
            agent_id,
            agent_type,
            kind,
            name,
            team,
            summary,
            context_tokens,
            transcript_path,
            task_ids,
        } => {
            if store.find_mut(&session_id, &agent_id).is_none() {
                // A stop that outran its own start: only its dispatch placed
                // it in this flow, so without one there is nothing to own it.
                let Some(ids) = task_ids.as_deref() else {
                    return Outcome::NotRecorded("no-flow");
                };
                let id = store.next_id();
                let mut row = AgentRecord {
                    id,
                    harness,
                    session_id: session_id.clone(),
                    agent_id: agent_id.clone(),
                    agent_type,
                    kind,
                    name,
                    team,
                    started_at: now.to_string(),
                    ..AgentRecord::default()
                };
                row.open_segment(ids, now);
                store.agents.push(row);
            }
            let row = store
                .find_mut(&session_id, &agent_id)
                .expect("row exists: found or just pushed");
            close(row, task_ids.as_deref(), now);
            row.status = match row.kind {
                AgentKind::Teammate => AgentStatus::Idle,
                AgentKind::Subagent => AgentStatus::Stopped,
            };
            if row.status == AgentStatus::Stopped {
                row.ended_at = now.to_string();
            }
            let summary = trim_summary(&summary);
            if !summary.is_empty() {
                row.summary = summary;
            }
            if context_tokens > 0 {
                row.context_tokens = context_tokens;
            }
            fill(&mut row.transcript_path, transcript_path);
            row.updated_at = now.to_string();
            recorded(label, row, true)
        }
        Event::Idle {
            session_id,
            name,
            task_ids,
        } => {
            let Some(row) = store.find_teammate_mut(&session_id, &name) else {
                return Outcome::NotRecorded("unknown-agent");
            };
            close(row, task_ids.as_deref(), now);
            row.status = AgentStatus::Idle;
            row.updated_at = now.to_string();
            recorded(label, row, true)
        }
    }
}

/// Close the open segment and, when a dispatch was read at close time, make
/// the closed segment carry its task set.
fn close(row: &mut AgentRecord, task_ids: Option<&[u32]>, now: &str) {
    if !row.close_segment(now) {
        return;
    }
    if let (Some(ids), Some(last)) = (task_ids, row.segments.last_mut()) {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        if last.task_ids != ids {
            last.task_ids = ids;
        }
    }
}

fn recorded(event: &'static str, row: &AgentRecord, changed: bool) -> Outcome {
    Outcome::Recorded {
        event,
        id: row.id.clone(),
        task_ids: row
            .segments
            .last()
            .map(|segment| segment.task_ids.clone())
            .unwrap_or_default(),
        changed,
    }
}

/// Overwrite `slot` only with a non-empty value: a later event that lacks a
/// field never blanks what an earlier one recorded.
fn fill(slot: &mut String, value: String) {
    if !value.is_empty() {
        *slot = value;
    }
}

fn trim_summary(summary: &str) -> String {
    summary.trim().chars().take(SUMMARY_MAX_CHARS).collect()
}

/// Which lifecycle event a payload carries, for flow selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Start,
    Stop,
    Idle,
}

/// The flow an event belongs to, given every flow's loaded store.
///
/// - Start: the dispatch's flow, else the flow already holding this agent,
///   else session affinity — the flow holding this session's most recently
///   updated row.
/// - Stop: the flow holding this agent, else the dispatch's flow.
/// - Idle: the flow holding this teammate.
///
/// Among several holders the most recently updated row wins.
fn choose_flow(
    kind: Kind,
    flows: &[(String, AgentsStore)],
    session_id: &str,
    agent_id: &str,
    name: &str,
    dispatch: Option<&Dispatch>,
) -> Option<String> {
    let latest = |matches: &dyn Fn(&AgentRecord) -> bool| {
        flows
            .iter()
            .flat_map(|(slug, store)| store.agents.iter().map(move |row| (slug, row)))
            .filter(|(_, row)| row.session_id == session_id && matches(row))
            .max_by(|a, b| a.1.updated_at.cmp(&b.1.updated_at))
            .map(|(slug, _)| slug.clone())
    };
    let dispatched = || dispatch.map(|d| d.slug.clone());
    match kind {
        Kind::Start => dispatched()
            .or_else(|| latest(&|row| row.agent_id == agent_id))
            .or_else(|| latest(&|_| true)),
        Kind::Stop => latest(&|row| row.agent_id == agent_id).or_else(dispatched),
        Kind::Idle if name.is_empty() => None,
        Kind::Idle => latest(&|row| row.name == name),
    }
}

/// Every flow's `agents.toml` under `<root>/.claude/flows/`. A store that
/// cannot be read is skipped: a hook has nobody to report the error to, and
/// the chosen store is read again under its lock before any write.
fn load_flows(root: &Path) -> Vec<(String, AgentsStore)> {
    let Ok(entries) = std::fs::read_dir(root.join(".claude").join("flows")) else {
        return Vec::new();
    };
    let mut flows: Vec<(String, AgentsStore)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let slug = entry.file_name().into_string().ok()?;
            crate::flow::validate_slug(&slug).ok()?;
            let path = entry.path().join(STORE_FILE);
            if !path.is_file() {
                return None;
            }
            let store = schema::from_toml(&io::read_toml(&path).ok()?).ok()?;
            Some((slug, store))
        })
        .collect();
    flows.sort_by(|a, b| a.0.cmp(&b.0));
    flows
}

fn field<'a>(payload: &'a JsonValue, key: &str) -> &'a str {
    payload
        .get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
}

fn not_recorded(reason: &str) -> JsonValue {
    json!({"recorded": false, "reason": reason})
}

/// Record one hook payload. Adapters exist for `claude-code` and `codex`; any
/// other harness is an error rather than a silent no-op.
pub(crate) fn record(
    harness: Harness,
    payload: &JsonValue,
    write_opts: &WriteIntegrityArgs,
) -> Result<JsonValue> {
    if !matches!(harness, Harness::ClaudeCode | Harness::Codex) {
        bail!(
            "agents record: the {harness} payload adapter is not implemented — only claude-code \
             and codex are"
        );
    }
    let codex = harness == Harness::Codex;

    // The repo root is resolved once per process and cached, so the hook's
    // own working directory has to be in place before anything asks for it.
    let cwd = field(payload, "cwd");
    if !cwd.is_empty() && Path::new(cwd).is_dir() {
        std::env::set_current_dir(cwd)?;
    }

    let kind = match field(payload, "hook_event_name") {
        "SubagentStart" => Kind::Start,
        "SubagentStop" => Kind::Stop,
        "TeammateIdle" if !codex => Kind::Idle,
        _ => return Ok(not_recorded("unsupported-event")),
    };
    let agent_type = field(payload, "agent_type");
    if kind != Kind::Idle && agent_type.is_empty() {
        return Ok(not_recorded("internal-agent"));
    }

    let session_id = field(payload, "session_id");
    let agent_id = field(payload, "agent_id");
    let name = field(payload, "teammate_name");
    let root = io::repo_or_cwd_root()?;
    let flows = load_flows(&root);

    let transcript: Option<PathBuf> = match kind {
        Kind::Idle => flows
            .iter()
            .flat_map(|(_, store)| store.agents.iter())
            .filter(|row| !name.is_empty() && row.session_id == session_id && row.name == name)
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
            .map(|row| PathBuf::from(&row.transcript_path))
            .filter(|path| path.is_file()),
        // Codex's start payload names the child's own rollout in
        // `transcript_path`; its stop payload names the parent's there and the
        // child's in `agent_transcript_path`.
        Kind::Start | Kind::Stop if codex => {
            let own = match field(payload, "agent_transcript_path") {
                "" if kind == Kind::Start => field(payload, "transcript_path"),
                path => path,
            };
            correlate::codex_rollout(own, agent_id)
        }
        Kind::Start | Kind::Stop => {
            let agent_transcript = payload
                .get("agent_transcript_path")
                .and_then(JsonValue::as_str);
            correlate::subagent_transcript(
                field(payload, "transcript_path"),
                session_id,
                agent_id,
                agent_transcript,
            )
        }
    };
    let dispatch = transcript.as_deref().and_then(if codex {
        correlate::codex_latest_dispatch
    } else {
        correlate::latest_dispatch
    });

    let Some(slug) = choose_flow(kind, &flows, session_id, agent_id, name, dispatch.as_ref())
    else {
        return Ok(not_recorded("no-flow"));
    };
    crate::flow::validate_slug(&slug)?;
    if !root
        .join(".claude")
        .join("flows")
        .join(&slug)
        .join("context.toml")
        .is_file()
    {
        return Ok(not_recorded("unknown-flow"));
    }

    // A dispatch for another flow says nothing about this flow's tasks.
    let task_ids = dispatch.filter(|d| d.slug == slug).map(|d| d.task_ids);
    let transcript_path = transcript
        .as_deref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Codex writes no `.meta.json` and has no teammates: its agents read as
    // unnamed subagents.
    let meta = match (kind, transcript.as_deref()) {
        (Kind::Idle, _) | (_, None) => correlate::Meta::default(),
        (_, Some(_)) if codex => correlate::Meta::default(),
        (_, Some(path)) => correlate::meta(path),
    };
    let context_tokens = |path: &Path| {
        if codex {
            correlate::codex_context_tokens(path)
        } else {
            correlate::context_tokens(path)
        }
    };
    let event = match kind {
        Kind::Start => Event::Start {
            harness,
            session_id: session_id.to_string(),
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            kind: meta.kind,
            name: meta.name,
            team: meta.team,
            transcript_path,
            task_ids: task_ids.unwrap_or_default(),
        },
        Kind::Stop => Event::Stop {
            harness,
            session_id: session_id.to_string(),
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            kind: meta.kind,
            name: meta.name,
            team: meta.team,
            summary: field(payload, "last_assistant_message").to_string(),
            context_tokens: transcript.as_deref().map_or(0, context_tokens),
            transcript_path,
            task_ids,
        },
        Kind::Idle => Event::Idle {
            session_id: session_id.to_string(),
            name: name.to_string(),
            task_ids,
        },
    };

    let file = schema::agents_path(&slug)?;
    let now = crate::time::now_rfc3339();
    let today = crate::time::today_toml_date()?;
    let on_missing = io::on_missing_for(&file, write_opts.no_create)?;
    let mut outcome: Option<Outcome> = None;
    io::mutate_doc_conditional(
        &file,
        write_opts.allow_outside,
        write_integrity_opts(write_opts),
        on_missing,
        |doc| {
            // Ids are minted here, under the lock, so two hooks racing on the
            // same store can never mint the same one.
            let mut store = schema::from_toml(doc)?;
            let result = apply(&mut store, event, &now);
            let persist = matches!(result, Outcome::Recorded { changed: true, .. });
            if persist {
                store.last_updated = Some(today);
                *doc = schema::to_toml(&store);
            }
            outcome = Some(result);
            Ok(persist)
        },
    )?;

    Ok(match outcome {
        Some(Outcome::Recorded {
            event,
            id,
            task_ids,
            ..
        }) => json!({
            "recorded": true,
            "slug": slug,
            "event": event,
            "id": id,
            "task_ids": task_ids,
        }),
        Some(Outcome::NotRecorded(reason)) => not_recorded(reason),
        None => bail!("agents record reached the write path without an outcome"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::schema::Segment;

    fn start(agent_id: &str, task_ids: &[u32]) -> Event {
        Event::Start {
            harness: Harness::ClaudeCode,
            session_id: "s1".into(),
            agent_id: agent_id.into(),
            agent_type: "implement-deep".into(),
            kind: AgentKind::Subagent,
            name: String::new(),
            team: String::new(),
            transcript_path: format!("/p/agent-{agent_id}.jsonl"),
            task_ids: task_ids.to_vec(),
        }
    }

    fn teammate_start(agent_id: &str, name: &str, task_ids: &[u32]) -> Event {
        Event::Start {
            harness: Harness::ClaudeCode,
            session_id: "s1".into(),
            agent_id: agent_id.into(),
            agent_type: "implement-deep".into(),
            kind: AgentKind::Teammate,
            name: name.into(),
            team: "pool".into(),
            transcript_path: String::new(),
            task_ids: task_ids.to_vec(),
        }
    }

    fn stop(agent_id: &str, task_ids: Option<&[u32]>) -> Event {
        Event::Stop {
            harness: Harness::ClaudeCode,
            session_id: "s1".into(),
            agent_id: agent_id.into(),
            agent_type: "implement-deep".into(),
            kind: AgentKind::Subagent,
            name: String::new(),
            team: String::new(),
            summary: "  Applied.  ".into(),
            context_tokens: 1234,
            transcript_path: String::new(),
            task_ids: task_ids.map(<[u32]>::to_vec),
        }
    }

    fn recorded_id(outcome: &Outcome) -> &str {
        match outcome {
            Outcome::Recorded { id, .. } => id,
            Outcome::NotRecorded(reason) => panic!("not recorded: {reason}"),
        }
    }

    #[test]
    fn a_new_start_mints_a_running_row_with_one_open_segment() {
        let mut store = AgentsStore::default();
        let outcome = apply(&mut store, start("a1", &[16]), "t0");
        assert_eq!(
            outcome,
            Outcome::Recorded {
                event: "start",
                id: "A1".into(),
                task_ids: vec![16],
                changed: true,
            }
        );
        let row = &store.agents[0];
        assert_eq!(row.status, AgentStatus::Running);
        assert_eq!(row.agent_type, "implement-deep");
        assert_eq!(row.started_at, "t0");
        assert_eq!(row.updated_at, "t0");
        assert_eq!(
            row.segments,
            vec![Segment {
                task_ids: vec![16],
                started_at: "t0".into(),
                ended_at: String::new(),
            }]
        );
    }

    #[test]
    fn a_repeated_start_on_the_same_tasks_changes_nothing() {
        let mut store = AgentsStore::default();
        apply(&mut store, start("a1", &[16, 17]), "t0");
        let before = store.clone();
        let outcome = apply(&mut store, start("a1", &[17, 16]), "t1");
        assert!(matches!(outcome, Outcome::Recorded { changed: false, .. }));
        assert_eq!(store, before);
    }

    #[test]
    fn a_start_on_new_tasks_closes_the_previous_segment() {
        let mut store = AgentsStore::default();
        apply(&mut store, teammate_start("a1", "worker-1", &[3]), "t0");
        let outcome = apply(&mut store, teammate_start("a1", "worker-1", &[7]), "t1");
        assert_eq!(recorded_id(&outcome), "A1");
        assert_eq!(store.agents.len(), 1);
        let segments = &store.agents[0].segments;
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].task_ids, vec![3]);
        assert_eq!(segments[0].ended_at, "t1");
        assert_eq!(segments[1].task_ids, vec![7]);
        assert_eq!(segments[1].ended_at, "");
    }

    #[test]
    fn a_subagent_stop_marks_the_row_stopped() {
        let mut store = AgentsStore::default();
        apply(&mut store, start("a1", &[16]), "t0");
        let outcome = apply(&mut store, stop("a1", Some(&[16])), "t1");
        assert!(matches!(
            outcome,
            Outcome::Recorded {
                event: "stop",
                changed: true,
                ..
            }
        ));
        let row = &store.agents[0];
        assert_eq!(row.status, AgentStatus::Stopped);
        assert_eq!(row.ended_at, "t1");
        assert_eq!(row.summary, "Applied.");
        assert_eq!(row.context_tokens, 1234);
        assert_eq!(row.segments[0].ended_at, "t1");
    }

    #[test]
    fn a_teammate_stop_leaves_the_row_idle_and_not_ended() {
        let mut store = AgentsStore::default();
        apply(&mut store, teammate_start("a1", "worker-1", &[3]), "t0");
        apply(&mut store, stop("a1", None), "t1");
        let row = &store.agents[0];
        assert_eq!(row.status, AgentStatus::Idle);
        assert_eq!(row.ended_at, "");
        assert_eq!(row.segments[0].ended_at, "t1");
        assert_eq!(row.segments[0].task_ids, vec![3]);
    }

    #[test]
    fn an_idle_event_matches_the_teammate_by_session_and_name() {
        let mut store = AgentsStore::default();
        apply(&mut store, teammate_start("a1", "worker-1", &[3]), "t0");
        apply(&mut store, teammate_start("a2", "worker-2", &[4]), "t0");
        let outcome = apply(
            &mut store,
            Event::Idle {
                session_id: "s1".into(),
                name: "worker-2".into(),
                task_ids: None,
            },
            "t1",
        );
        assert_eq!(recorded_id(&outcome), "A2");
        assert_eq!(store.agents[0].status, AgentStatus::Running);
        assert_eq!(store.agents[1].status, AgentStatus::Idle);
        assert_eq!(store.agents[1].segments[0].ended_at, "t1");

        let other_session = apply(
            &mut store,
            Event::Idle {
                session_id: "s2".into(),
                name: "worker-1".into(),
                task_ids: None,
            },
            "t2",
        );
        assert_eq!(other_session, Outcome::NotRecorded("unknown-agent"));
    }

    #[test]
    fn a_stop_carrying_a_different_dispatch_rewrites_the_closing_segment() {
        let mut store = AgentsStore::default();
        apply(&mut store, start("a1", &[3]), "t0");
        let outcome = apply(&mut store, stop("a1", Some(&[9, 8])), "t1");
        assert!(matches!(
            &outcome,
            Outcome::Recorded { task_ids, .. } if task_ids == &vec![8, 9]
        ));
        assert_eq!(store.agents[0].segments.len(), 1);
        assert_eq!(store.agents[0].segments[0].task_ids, vec![8, 9]);
    }

    #[test]
    fn a_stop_for_an_unknown_agent_with_a_flow_writes_a_stopped_row() {
        let mut store = AgentsStore::default();
        apply(&mut store, start("a0", &[1]), "t0");
        let outcome = apply(&mut store, stop("a1", Some(&[5])), "t1");
        assert_eq!(recorded_id(&outcome), "A2");
        let row = &store.agents[1];
        assert_eq!(row.agent_id, "a1");
        assert_eq!(row.agent_type, "implement-deep");
        assert_eq!(row.status, AgentStatus::Stopped);
        assert_eq!(
            row.segments,
            vec![Segment {
                task_ids: vec![5],
                started_at: "t1".into(),
                ended_at: "t1".into(),
            }]
        );
    }

    #[test]
    fn a_stop_for_an_unknown_agent_without_a_flow_is_not_recorded() {
        let mut store = AgentsStore::default();
        let outcome = apply(&mut store, stop("a1", None), "t1");
        assert_eq!(outcome, Outcome::NotRecorded("no-flow"));
        assert!(store.agents.is_empty());

        // Nor does any flow claim it: no row holds it and no dispatch names one.
        let flows = vec![("held".to_string(), {
            let mut held = AgentsStore::default();
            apply(&mut held, start("other", &[1]), "t0");
            held
        })];
        assert_eq!(choose_flow(Kind::Stop, &flows, "s1", "a1", "", None), None);
    }

    #[test]
    fn flow_choice_prefers_dispatch_then_owner_then_session_affinity() {
        let mut older = AgentsStore::default();
        apply(&mut older, start("a1", &[1]), "2026-09-28T01:00:00Z");
        let mut newer = AgentsStore::default();
        apply(&mut newer, start("a2", &[2]), "2026-09-28T02:00:00Z");
        let flows = vec![("older".to_string(), older), ("newer".to_string(), newer)];
        let dispatch = Dispatch {
            slug: "named".into(),
            task_ids: vec![4],
        };

        let pick =
            |kind, agent_id, dispatch| choose_flow(kind, &flows, "s1", agent_id, "", dispatch);
        assert_eq!(
            pick(Kind::Start, "a1", Some(&dispatch)).as_deref(),
            Some("named")
        );
        assert_eq!(pick(Kind::Start, "a1", None).as_deref(), Some("older"));
        assert_eq!(pick(Kind::Start, "fresh", None).as_deref(), Some("newer"));
        assert_eq!(
            pick(Kind::Stop, "a1", Some(&dispatch)).as_deref(),
            Some("older")
        );
        assert_eq!(
            pick(Kind::Stop, "fresh", Some(&dispatch)).as_deref(),
            Some("named")
        );
        assert_eq!(
            choose_flow(Kind::Start, &flows, "other-session", "x", "", None),
            None
        );
    }

    #[test]
    fn the_summary_is_trimmed_to_its_cap_on_a_char_boundary() {
        let long = "é".repeat(SUMMARY_MAX_CHARS + 5);
        let trimmed = trim_summary(&long);
        assert_eq!(trimmed.chars().count(), SUMMARY_MAX_CHARS);
    }

    #[test]
    fn record_writes_the_store_of_an_existing_flow_only() {
        crate::test_support::with_root(|root| {
            let proj = root.join("proj");
            let sub = proj.join("s1").join("subagents");
            std::fs::create_dir_all(&sub).expect("subagents dir");
            std::fs::write(proj.join("s1.jsonl"), "").expect("parent transcript");
            let prompt = json!({"type": "user", "message": {"content":
                "tomlctl tasks show 6 --slug live-flow --with body"}});
            std::fs::write(sub.join("agent-a1.jsonl"), format!("{prompt}\n"))
                .expect("agent transcript");
            std::fs::write(
                sub.join("agent-a1.meta.json"),
                r#"{"agentType":"implement-deep"}"#,
            )
            .expect("meta");

            let payload = json!({
                "hook_event_name": "SubagentStart",
                "session_id": "s1",
                "agent_id": "a1",
                "agent_type": "implement-deep",
                "transcript_path": proj.join("s1.jsonl").to_string_lossy(),
            });
            let args = WriteIntegrityArgs {
                allow_outside: false,
                no_write_integrity: false,
                verify_integrity: false,
                strict_integrity: false,
                no_create: false,
            };

            let flow_dir = root.join(".claude").join("flows").join("live-flow");
            assert_eq!(
                record(Harness::ClaudeCode, &payload, &args).expect("records"),
                not_recorded("unknown-flow")
            );
            assert!(!flow_dir.join(STORE_FILE).exists());

            std::fs::create_dir_all(&flow_dir).expect("flow dir");
            std::fs::write(flow_dir.join("context.toml"), "slug = \"live-flow\"\n")
                .expect("context");
            assert_eq!(
                record(Harness::ClaudeCode, &payload, &args).expect("records"),
                json!({"recorded": true, "slug": "live-flow", "event": "start",
                       "id": "A1", "task_ids": [6]})
            );
            let written = schema::from_toml(
                &io::read_toml(&flow_dir.join(STORE_FILE)).expect("store written"),
            )
            .expect("store reads");
            assert_eq!(written.schema_version, 1);
            assert!(written.last_updated.is_some());
            assert_eq!(written.agents[0].segments[0].task_ids, vec![6]);

            let internal = json!({"hook_event_name": "SubagentStop", "agent_type": ""});
            assert_eq!(
                record(Harness::ClaudeCode, &internal, &args).expect("records"),
                not_recorded("internal-agent")
            );
            assert!(record(Harness::Manual, &payload, &args).is_err());
        });
    }

    fn codex_line(kind: &str, payload: JsonValue) -> String {
        json!({"timestamp": "2026-09-29T10:00:01.000Z", "type": kind, "payload": payload})
            .to_string()
    }

    #[test]
    fn record_maps_codex_start_and_stop_payloads_onto_one_row() {
        crate::test_support::with_root(|root| {
            let day = root
                .join("codex-home")
                .join("sessions")
                .join("2026")
                .join("09")
                .join("29");
            std::fs::create_dir_all(&day).expect("rollout dir");
            let session = "0199a3b0-11aa-7c3e-8f00-3d2c1b0a9f8e";
            let child = "0199a3c1-7f2e-7d40-9b1a-5c7e2f0a1b2c";
            let parent_rollout = day.join(format!("rollout-2026-09-29T09-58-12-{session}.jsonl"));
            let child_rollout = day.join(format!("rollout-2026-09-29T10-00-00-{child}.jsonl"));
            std::fs::write(&parent_rollout, "").expect("parent rollout");
            let usage = json!({"input_tokens": 41000, "cached_input_tokens": 38400,
                "output_tokens": 1200, "reasoning_output_tokens": 640, "total_tokens": 42200});
            let lines = [
                codex_line(
                    "session_meta",
                    json!({"id": child, "timestamp": "2026-09-29T10:00:00.000Z",
                        "cwd": root.to_string_lossy(), "originator": "codex_cli_rs",
                        "cli_version": "0.158.0"}),
                ),
                codex_line(
                    "response_item",
                    json!({"type": "message", "role": "user", "content": [{"type": "input_text",
                        "text": "DISPATCH: implement-deep\nFetch it: `tomlctl tasks show 6 --slug codex-flow --with body,files,deps`"}]}),
                ),
                codex_line(
                    "response_item",
                    json!({"type": "message", "role": "assistant", "content": [
                        {"type": "output_text", "text": "Applied task 6."}], "phase": "final_answer"}),
                ),
                codex_line(
                    "event_msg",
                    json!({"type": "token_count", "info": {"total_token_usage": usage,
                        "last_token_usage": usage, "model_context_window": 272000},
                        "rate_limits": null}),
                ),
            ];
            std::fs::write(&child_rollout, lines.join("\n") + "\n").expect("child rollout");
            let flow_dir = root.join(".claude").join("flows").join("codex-flow");
            std::fs::create_dir_all(&flow_dir).expect("flow dir");
            std::fs::write(flow_dir.join("context.toml"), "slug = \"codex-flow\"\n")
                .expect("context");
            let args = WriteIntegrityArgs {
                allow_outside: false,
                no_write_integrity: false,
                verify_integrity: false,
                strict_integrity: false,
                no_create: false,
            };

            // `cwd` is left out: record would chdir the whole test process.
            let start = json!({
                "hook_event_name": "SubagentStart",
                "session_id": session,
                "turn_id": "019a0001-0000-7000-8000-000000000001",
                "agent_id": child,
                "agent_type": "worker",
                "model": "gpt-5-codex",
                "permission_mode": "bypassPermissions",
                "transcript_path": child_rollout.to_string_lossy(),
            });
            assert_eq!(
                record(Harness::Codex, &start, &args).expect("records"),
                json!({"recorded": true, "slug": "codex-flow", "event": "start",
                       "id": "A1", "task_ids": [6]})
            );

            let stop = json!({
                "hook_event_name": "SubagentStop",
                "session_id": session,
                "turn_id": "019a0001-0000-7000-8000-000000000001",
                "agent_id": child,
                "agent_type": "worker",
                "model": "gpt-5-codex",
                "permission_mode": "bypassPermissions",
                "transcript_path": parent_rollout.to_string_lossy(),
                "agent_transcript_path": child_rollout.to_string_lossy(),
                "last_assistant_message": "Applied task 6.",
                "stop_hook_active": false,
            });
            assert_eq!(
                record(Harness::Codex, &stop, &args).expect("records"),
                json!({"recorded": true, "slug": "codex-flow", "event": "stop",
                       "id": "A1", "task_ids": [6]})
            );

            let store = schema::from_toml(
                &io::read_toml(&flow_dir.join(STORE_FILE)).expect("store written"),
            )
            .expect("store reads");
            assert_eq!(store.agents.len(), 1);
            let row = &store.agents[0];
            assert_eq!(row.harness, Harness::Codex);
            assert_eq!(row.session_id, session);
            assert_eq!(row.agent_id, child);
            assert_eq!(row.agent_type, "worker");
            assert_eq!(row.kind, AgentKind::Subagent);
            assert_eq!(row.name, "");
            assert_eq!(row.status, AgentStatus::Stopped);
            assert_eq!(row.summary, "Applied task 6.");
            assert_eq!(row.context_tokens, 42200);
            assert!(
                row.transcript_path.ends_with(&format!("{child}.jsonl")),
                "{}",
                row.transcript_path
            );
            assert_eq!(row.segments.len(), 1);
            assert_eq!(row.segments[0].task_ids, vec![6]);
            assert!(!row.segments[0].ended_at.is_empty());
        });
    }

    #[test]
    fn a_codex_payload_without_its_rollout_degrades_like_a_missing_transcript() {
        crate::test_support::with_root(|root| {
            let flow_dir = root.join(".claude").join("flows").join("codex-flow");
            std::fs::create_dir_all(&flow_dir).expect("flow dir");
            std::fs::write(flow_dir.join("context.toml"), "slug = \"codex-flow\"\n")
                .expect("context");
            let args = WriteIntegrityArgs {
                allow_outside: false,
                no_write_integrity: false,
                verify_integrity: false,
                strict_integrity: false,
                no_create: false,
            };

            // Both transcript paths are nullable in Codex's stop schema.
            let stop = json!({
                "hook_event_name": "SubagentStop",
                "session_id": "s-codex",
                "turn_id": "t1",
                "agent_id": "0199a3c1-0000-7000-8000-00000000000f",
                "agent_type": "default",
                "model": "gpt-5-codex",
                "permission_mode": "default",
                "transcript_path": null,
                "agent_transcript_path": null,
                "last_assistant_message": null,
                "stop_hook_active": false,
            });
            assert_eq!(
                record(Harness::Codex, &stop, &args).expect("records"),
                not_recorded("no-flow")
            );
            assert!(!flow_dir.join(STORE_FILE).exists());

            let idle = json!({"hook_event_name": "TeammateIdle", "session_id": "s-codex",
                              "teammate_name": "worker-1"});
            assert_eq!(
                record(Harness::Codex, &idle, &args).expect("records"),
                not_recorded("unsupported-event")
            );
        });
    }
}
