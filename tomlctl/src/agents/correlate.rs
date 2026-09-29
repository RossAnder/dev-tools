//! Read-only correlation from a hook payload to the facts a record needs: the
//! agent's transcript, the flow and tasks it was dispatched on, its teammate
//! metadata, and its context size.
//!
//! Observed against Claude Code 2.1.283, 2026-09-28. A subagent's transcript is
//! `<dirname(transcript_path)>/<session_id>/subagents/agent-<agent_id>.jsonl`,
//! live-appended one JSON object per line, with a sibling `.meta.json` that
//! appears within ~124 ms of the transcript's first line. The transcript can
//! also lag the hook that names it, so a lookup here may still see an agent's
//! previous dispatch; callers that close a segment look again.
//!
//! Every failure degrades to `None`, a default, or `0`: a hook has nobody to
//! report an error to.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::Value as JsonValue;

use super::schema::AgentKind;

/// A transcript up to this size is read whole.
const WHOLE_READ_MAX: u64 = 4 * 1024 * 1024;
/// Past `WHOLE_READ_MAX`, only this much of the end is read.
const TAIL_BYTES: u64 = 1024 * 1024;
/// A meta file is a few hundred bytes; anything far larger is not one.
const MAX_META_BYTES: u64 = 16 * 1024;
const RETRY_ATTEMPTS: u32 = 10;
const RETRY_DELAY: Duration = Duration::from_millis(100);

/// The flow and tasks named by an agent's latest dispatch prompt. `task_ids`
/// is sorted and deduplicated; more than one id means a cluster dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dispatch {
    pub(crate) slug: String,
    pub(crate) task_ids: Vec<u32>,
}

/// What the sibling `.meta.json` says about an agent. A missing or unreadable
/// file reads as a subagent with no name or team.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Meta {
    pub(crate) kind: AgentKind,
    pub(crate) name: String,
    pub(crate) team: String,
}

/// Resolve the agent's transcript, preferring the payload's
/// `agent_transcript_path`. The result must canonicalise to a file inside
/// `dirname(transcript_path)`: both paths arrive in the payload, and nothing
/// outside the session's own directory is ever read.
pub(crate) fn subagent_transcript(
    transcript_path: &str,
    session_id: &str,
    agent_id: &str,
    agent_transcript_path: Option<&str>,
) -> Option<PathBuf> {
    subagent_transcript_with(
        transcript_path,
        session_id,
        agent_id,
        agent_transcript_path,
        RETRY_ATTEMPTS,
    )
}

fn subagent_transcript_with(
    transcript_path: &str,
    session_id: &str,
    agent_id: &str,
    agent_transcript_path: Option<&str>,
    attempts: u32,
) -> Option<PathBuf> {
    let root = Path::new(transcript_path).parent()?;
    let candidate = match agent_transcript_path.filter(|p| !p.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => {
            if session_id.is_empty() || agent_id.is_empty() {
                return None;
            }
            root.join(session_id)
                .join("subagents")
                .join(format!("agent-{agent_id}.jsonl"))
        }
    };
    if !wait_for_file(&candidate, attempts) {
        return None;
    }
    let canonical = std::fs::canonicalize(&candidate).ok()?;
    let root = std::fs::canonicalize(root).ok()?;
    canonical.starts_with(&root).then_some(canonical)
}

/// The latest dispatch in `path`: the newest user-authored line carrying at
/// least one `tasks show <id> --slug <slug>` command. `None` when there is no
/// such line, or when that line's slugs disagree.
pub(crate) fn latest_dispatch(path: &Path) -> Option<Dispatch> {
    if !wait_for_file(path, RETRY_ATTEMPTS) {
        return None;
    }
    dispatch_in(&read_tail(path)?)
}

fn dispatch_in(text: &str) -> Option<Dispatch> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    // `[0-9]` rather than `\d`: the crate is built without Unicode classes.
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"tasks show ([0-9]+) --slug ([a-z0-9][a-z0-9-]{0,63})")
            .expect("dispatch pattern compiles")
    });

    for line in text.lines().rev() {
        // Cheap rejections before a JSON parse: a line that cannot match is
        // skipped whatever its shape.
        if !line.contains("tasks show") || !line.contains("\"user\"") {
            continue;
        }
        let Some(prompt) = user_text(line) else {
            continue;
        };
        let mut slug: Option<&str> = None;
        let mut task_ids = Vec::new();
        for caps in pattern.captures_iter(&prompt) {
            let (Some(id), Some(s)) = (caps.get(1), caps.get(2)) else {
                continue;
            };
            let Ok(id) = id.as_str().parse::<u32>() else {
                continue;
            };
            match slug {
                Some(prev) if prev != s.as_str() => return None,
                _ => slug = Some(s.as_str()),
            }
            task_ids.push(id);
        }
        if let Some(slug) = slug {
            task_ids.sort_unstable();
            task_ids.dedup();
            return Some(Dispatch {
                slug: slug.to_string(),
                task_ids,
            });
        }
    }
    None
}

/// The prose of a user-role transcript line: a string `message.content`, or
/// the concatenated text blocks of an array one. A line carrying any
/// `tool_result` block is a tool's output echoed back, not a prompt, and
/// yields `None` even when it quotes a dispatch command.
fn user_text(line: &str) -> Option<String> {
    let value: JsonValue = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "user" {
        return None;
    }
    match value.get("message")?.get("content")? {
        JsonValue::String(s) => Some(s.clone()),
        JsonValue::Array(blocks) => {
            let mut out = String::new();
            for block in blocks {
                match block.get("type").and_then(JsonValue::as_str) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(JsonValue::as_str) {
                            out.push_str(t);
                            out.push('\n');
                        }
                    }
                    Some("tool_result") => return None,
                    _ => {}
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Read the `.meta.json` beside the transcript at `path`.
pub(crate) fn meta(path: &Path) -> Meta {
    meta_with(path, RETRY_ATTEMPTS)
}

fn meta_with(path: &Path, attempts: u32) -> Meta {
    let meta_path = path.with_extension("meta.json");
    if !wait_for_file(&meta_path, attempts) {
        return Meta::default();
    }
    let Some(text) = read_capped(&meta_path, MAX_META_BYTES) else {
        return Meta::default();
    };
    let Ok(value) = serde_json::from_str::<JsonValue>(&text) else {
        return Meta::default();
    };
    let field = |key: &str| {
        value
            .get(key)
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let kind = if field("taskKind") == "in_process_teammate" {
        AgentKind::Teammate
    } else {
        AgentKind::Subagent
    };
    Meta {
        kind,
        name: field("name"),
        team: field("teamName"),
    }
}

/// The context size at the agent's last assistant turn: the sum of that
/// line's input, cache-read, cache-creation and output token counts.
pub(crate) fn context_tokens(path: &Path) -> u64 {
    read_tail(path).map_or(0, |text| tokens_in(&text))
}

fn tokens_in(text: &str) -> u64 {
    for line in text.lines().rev() {
        if !line.contains("\"assistant\"") || !line.contains("\"usage\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<JsonValue>(line) else {
            continue;
        };
        if value.get("type").and_then(JsonValue::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = value.get("message").and_then(|m| m.get("usage")) else {
            continue;
        };
        return [
            "input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
            "output_tokens",
        ]
        .iter()
        .filter_map(|k| usage.get(*k).and_then(JsonValue::as_u64))
        .fold(0u64, u64::saturating_add);
    }
    0
}

/// The whole file up to `WHOLE_READ_MAX`, else its last `TAIL_BYTES` from the
/// first line boundary on, so no partial line is ever parsed.
fn read_tail(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut bytes = Vec::new();
    if len <= WHOLE_READ_MAX {
        file.take(WHOLE_READ_MAX + 1).read_to_end(&mut bytes).ok()?;
        return Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    file.seek(SeekFrom::Start(len - TAIL_BYTES)).ok()?;
    file.take(TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let start = memchr::memchr(b'\n', &bytes).map_or(bytes.len(), |i| i + 1);
    Some(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

/// Read a file expected to be small, or nothing. The cap binds the bytes
/// actually read, not a prior stat, which a symlink can misreport.
fn read_capped(path: &Path, cap: u64) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(cap + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= cap).then_some(text)
}

/// Wait for a file the harness may not have written yet.
fn wait_for_file(path: &Path, attempts: u32) -> bool {
    for _ in 0..attempts {
        if path.is_file() {
            return true;
        }
        std::thread::sleep(RETRY_DELAY);
    }
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_line(content: &str) -> String {
        serde_json::json!({"type": "user", "message": {"role": "user", "content": content}})
            .to_string()
    }

    /// A session tree: `<tmp>/proj/<session>.jsonl` and the agent transcript
    /// under `<tmp>/proj/<session>/subagents/`, holding `lines`. `<tmp>` itself
    /// sits outside the tree.
    fn session_tree(lines: &[String]) -> (tempfile::TempDir, String, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(&proj).expect("project dir");
        let parent = proj.join("s1.jsonl");
        std::fs::write(&parent, "").expect("parent transcript");
        let sub = proj.join("s1").join("subagents");
        std::fs::create_dir_all(&sub).expect("subagents dir");
        let agent = sub.join("agent-a1.jsonl");
        std::fs::write(&agent, lines.join("\n") + "\n").expect("agent transcript");
        (tmp, parent.to_string_lossy().into_owned(), agent)
    }

    #[test]
    fn a_spawn_prompt_names_its_flow_and_task() {
        let prompt = user_line(
            "DISPATCH: implement-deep\nFetch it: `tomlctl tasks show 16 --slug my-flow --with body`",
        );
        let (_tmp, parent, _) = session_tree(&[prompt]);
        let path = subagent_transcript(&parent, "s1", "a1", None).expect("derived path resolves");
        assert_eq!(
            latest_dispatch(&path),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: vec![16],
            })
        );
    }

    #[test]
    fn a_teammate_retask_supersedes_the_earlier_dispatch() {
        let lines = [
            user_line("tomlctl tasks show 3 --slug my-flow"),
            serde_json::json!({"type": "assistant", "message": {"content": "done"}}).to_string(),
            serde_json::json!({"type": "user", "message": {"content": [
                {"type": "text", "text": "<teammate-message>next: tomlctl tasks show 7 --slug my-flow</teammate-message>"}
            ]}})
            .to_string(),
        ];
        let (_tmp, _, agent) = session_tree(&lines);
        assert_eq!(latest_dispatch(&agent).map(|d| d.task_ids), Some(vec![7]));
    }

    #[test]
    fn a_cluster_dispatch_collects_every_id_sorted_and_deduplicated() {
        let text = user_line(
            "tasks show 12 --slug my-flow\ntasks show 5 --slug my-flow\ntasks show 12 --slug my-flow",
        );
        assert_eq!(
            dispatch_in(&text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: vec![5, 12],
            })
        );
    }

    #[test]
    fn slugs_that_disagree_within_the_newest_dispatch_give_none() {
        let text = [
            user_line("tasks show 1 --slug agreed"),
            user_line("tasks show 2 --slug one-flow\ntasks show 3 --slug other-flow"),
        ]
        .join("\n");
        assert_eq!(dispatch_in(&text), None);
    }

    #[test]
    fn a_tool_result_quoting_the_pattern_is_not_a_dispatch() {
        let text = [
            user_line("tasks show 4 --slug real-flow"),
            serde_json::json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t", "content": "tasks show 9 --slug echoed-flow"},
                {"type": "text", "text": "tasks show 9 --slug echoed-flow"}
            ]}})
            .to_string(),
            serde_json::json!({"type": "assistant", "message": {"content": [
                {"type": "text", "text": "running tasks show 8 --slug assistant-flow"}
            ]}})
            .to_string(),
        ]
        .join("\n");
        assert_eq!(
            dispatch_in(&text),
            Some(Dispatch {
                slug: "real-flow".into(),
                task_ids: vec![4],
            })
        );
    }

    #[test]
    fn a_transcript_path_outside_the_session_tree_is_rejected() {
        let (tmp, parent, agent) = session_tree(&[user_line("x")]);
        // The file a `..` session id climbs to exists, so only the
        // containment check can refuse it.
        let climbed = tmp.path().join("subagents");
        std::fs::create_dir_all(&climbed).expect("climbed dir");
        std::fs::write(climbed.join("agent-a1.jsonl"), "").expect("climbed file");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let outside = elsewhere.path().join("agent-a1.jsonl");
        std::fs::write(&outside, "").expect("outside file");

        let supplied = outside.to_string_lossy().into_owned();
        assert_eq!(
            subagent_transcript_with(&parent, "s1", "a1", Some(&supplied), 0),
            None
        );
        // A traversal out of the tree is caught after canonicalisation.
        assert_eq!(subagent_transcript_with(&parent, "..", "a1", None, 0), None);
        let inside = agent.to_string_lossy().into_owned();
        assert!(subagent_transcript_with(&parent, "", "", Some(&inside), 0).is_some());
    }

    #[test]
    fn a_missing_transcript_resolves_to_none() {
        let (_tmp, parent, _) = session_tree(&[]);
        assert_eq!(
            subagent_transcript_with(&parent, "s1", "nope", None, 0),
            None
        );
    }

    #[test]
    fn meta_marks_an_in_process_teammate_and_carries_its_name_and_team() {
        let (_tmp, _, agent) = session_tree(&[]);
        std::fs::write(
            agent.with_extension("meta.json"),
            r#"{"agentType":"implement-deep","name":"worker-1","teamName":"session-9",
                "taskKind":"in_process_teammate"}"#,
        )
        .expect("meta");
        assert_eq!(
            meta_with(&agent, 0),
            Meta {
                kind: AgentKind::Teammate,
                name: "worker-1".into(),
                team: "session-9".into(),
            }
        );
    }

    #[test]
    fn meta_without_a_teammate_task_kind_is_a_subagent() {
        let (_tmp, _, agent) = session_tree(&[]);
        std::fs::write(
            agent.with_extension("meta.json"),
            r#"{"agentType":"implement-deep","requestShape":"background"}"#,
        )
        .expect("meta");
        assert_eq!(meta_with(&agent, 0), Meta::default());
        std::fs::remove_file(agent.with_extension("meta.json")).expect("remove");
        assert_eq!(meta_with(&agent, 0), Meta::default());
    }

    #[test]
    fn context_tokens_sums_the_last_assistant_usage() {
        let lines = [
            serde_json::json!({"type": "assistant", "message": {"usage": {
                "input_tokens": 1, "output_tokens": 1}}})
            .to_string(),
            serde_json::json!({"type": "assistant", "message": {"usage": {
                "input_tokens": 2, "cache_creation_input_tokens": 180,
                "cache_read_input_tokens": 73196, "output_tokens": 1470,
                "output_tokens_details": {"thinking_tokens": 0}}}})
            .to_string(),
            user_line("usage \"assistant\""),
        ];
        let (_tmp, _, agent) = session_tree(&lines);
        assert_eq!(context_tokens(&agent), 2 + 180 + 73196 + 1470);
        assert_eq!(context_tokens(Path::new("definitely/not/here.jsonl")), 0);
    }

    #[test]
    fn a_large_transcript_is_read_from_a_line_boundary_in_its_tail() {
        let (_tmp, _, agent) = session_tree(&[]);
        let old = user_line("tasks show 1 --slug old-flow");
        let filler = user_line(&"x".repeat(1000));
        let mut body = String::new();
        body.push_str(&old);
        body.push('\n');
        while (body.len() as u64) <= WHOLE_READ_MAX {
            body.push_str(&filler);
            body.push('\n');
        }
        std::fs::write(&agent, &body).expect("large transcript");
        let tail = read_tail(&agent).expect("tail");
        assert!(tail.len() as u64 <= TAIL_BYTES);
        assert!(tail.starts_with("{\"type\""), "tail starts on a whole line");
        // The old dispatch sits before the tail window.
        assert_eq!(latest_dispatch(&agent), None);
    }
}
