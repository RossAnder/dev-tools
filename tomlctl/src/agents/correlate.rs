//! Read-only correlation from a hook payload to the facts a record needs: the
//! agent's transcript, the flow and tasks it was dispatched on, its teammate
//! metadata, and its context size.
//!
//! Observed against Claude Code 2.1.283, 2026-09-28. A subagent's transcript is
//! `<dirname(transcript_path)>/<session_id>/subagents/agent-<agent_id>.jsonl`,
//! live-appended one JSON object per line, with a sibling `.meta.json` that
//! appears within ~124 ms of the transcript's first line. The transcript can
//! also lag the hook that names it, so a lookup here may still see an agent's
//! previous dispatch; callers that close a segment look again. The `codex_*`
//! readers follow the rollout format of openai/codex `rust-v0.158.0`.
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

/// The flow, tasks and ledger items named by an agent's latest dispatch
/// prompt. `task_ids` is sorted and deduplicated, and more than one id means a
/// cluster dispatch; `item_ids` is deduplicated in first-seen order. Either
/// may be empty: a ledger dispatch names no task, a task dispatch no item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dispatch {
    pub(crate) slug: String,
    pub(crate) task_ids: Vec<u32>,
    pub(crate) item_ids: Vec<String>,
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
        Some(p) => {
            let p = PathBuf::from(p);
            if !wait_for_file(&p, attempts) {
                return None;
            }
            p
        }
        None => {
            if session_id.is_empty() || agent_id.is_empty() {
                return None;
            }
            let subagents = root.join(session_id).join("subagents");
            wait_for(attempts, || derived_transcript(&subagents, agent_id))?
        }
    };
    let canonical = std::fs::canonicalize(&candidate).ok()?;
    let root = std::fs::canonicalize(root).ok()?;
    canonical.starts_with(&root).then_some(canonical)
}

/// The transcript of `agent_id` under a session's `subagents/` directory: at
/// its top level for an Agent-tool dispatch, or one level under
/// `workflows/<run>/` for an agent a Workflow script dispatched. The
/// `SubagentStart` payload names neither.
fn derived_transcript(subagents: &Path, agent_id: &str) -> Option<PathBuf> {
    let file = format!("agent-{agent_id}.jsonl");
    let direct = subagents.join(&file);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(subagents.join("workflows"))
        .ok()?
        .filter_map(Result::ok)
        .map(|run| run.path().join(&file))
        .find(|path| path.is_file())
}

/// The latest dispatch in `path`: the newest user-authored line carrying at
/// least one `tasks show <id> --slug <slug>` command or `ledger: <path>` line.
/// `None` when there is no such line, or when that line's slugs disagree.
pub(crate) fn latest_dispatch(path: &Path) -> Option<Dispatch> {
    if !wait_for_file(path, RETRY_ATTEMPTS) {
        return None;
    }
    latest_dispatch_with(path, user_text)
}

fn latest_dispatch_with(path: &Path, prompt_of: fn(&str) -> Option<String>) -> Option<Dispatch> {
    dispatch_in(&read_tail(path)?, prompt_of)
        .or_else(|| head_dispatch(path, prompt_of, WHOLE_READ_MAX))
}

/// The newest dispatch among the whole lines of a transcript's first
/// `TAIL_BYTES`, where the spawn prompt sits, for a file longer than `read`,
/// the window its caller has already searched; `None` for a shorter one.
fn head_dispatch(
    path: &Path,
    prompt_of: fn(&str) -> Option<String>,
    read: u64,
) -> Option<Dispatch> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() <= read {
        return None;
    }
    dispatch_in(&read_head(file)?, prompt_of)
}

/// The whole lines among a file's first `TAIL_BYTES`, or the whole file when
/// it is no longer.
fn read_head(file: std::fs::File) -> Option<String> {
    let mut bytes = Vec::new();
    file.take(TAIL_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > TAIL_BYTES {
        let end = memchr::memrchr(b'\n', &bytes[..TAIL_BYTES as usize]).unwrap_or(0);
        bytes.truncate(end);
    }
    Some(into_text(bytes))
}

/// The dispatch at the head of a transcript longer than the stop's tail
/// window, which [`dispatch_and_tokens`] cannot see: the spawn prompt of a
/// long run. A teammate retasked since may have moved on from it, so it only
/// fills what the tail read left unknown.
pub(crate) fn spawn_dispatch(path: &Path) -> Option<Dispatch> {
    head_dispatch(path, user_text, TAIL_BYTES)
}

/// [`spawn_dispatch`] over a Codex rollout.
pub(crate) fn codex_spawn_dispatch(path: &Path) -> Option<Dispatch> {
    head_dispatch(path, codex_user_text, TAIL_BYTES)
}

/// The `timestamp` of the first transcript line carrying one, as written:
/// when the agent began, for a record whose start event was lost. Claude Code
/// and Codex lines both carry the field at the top level.
pub(crate) fn first_timestamp(path: &Path) -> Option<String> {
    let head = read_head(std::fs::File::open(path).ok()?)?;
    head.lines().find_map(|line| {
        let value: JsonValue = serde_json::from_str(line).ok()?;
        let stamp = value.get("timestamp")?.as_str()?;
        stamp
            .parse::<jiff::Timestamp>()
            .is_ok()
            .then(|| stamp.to_string())
    })
}

/// The dispatch and the context size at the last assistant turn, from one read
/// of the last `TAIL_BYTES` of `path`, for a stop that needs both. A dispatch
/// outside that window, such as the spawn prompt of a long run, gives `None`;
/// [`spawn_dispatch`] reads that one.
pub(crate) fn dispatch_and_tokens(path: &Path) -> (Option<Dispatch>, u64) {
    dispatch_and_tokens_with(path, user_text, tokens_in)
}

/// [`dispatch_and_tokens`] over a Codex rollout.
pub(crate) fn codex_dispatch_and_tokens(path: &Path) -> (Option<Dispatch>, u64) {
    dispatch_and_tokens_with(path, codex_user_text, codex_tokens_in)
}

fn dispatch_and_tokens_with(
    path: &Path,
    prompt_of: fn(&str) -> Option<String>,
    tokens_of: fn(&str) -> u64,
) -> (Option<Dispatch>, u64) {
    let present = wait_for_file(path, RETRY_ATTEMPTS);
    let Some(text) = read_last(path, TAIL_BYTES) else {
        return (None, 0);
    };
    let dispatch = if present {
        dispatch_in(&text, prompt_of)
    } else {
        None
    };
    (dispatch, tokens_of(&text))
}

/// Resolve a Codex agent's rollout from the payload path naming it. Codex
/// keeps rollouts under `<CODEX_HOME>/sessions/YYYY/MM/DD/`, so a child
/// spawned past midnight sits outside its parent's directory; containment is
/// instead the file name, `rollout-<timestamp>-<thread id>.jsonl`, which must
/// carry `agent_id`.
pub(crate) fn codex_rollout(path: &str, agent_id: &str) -> Option<PathBuf> {
    codex_rollout_with(path, agent_id, RETRY_ATTEMPTS)
}

fn codex_rollout_with(path: &str, agent_id: &str, attempts: u32) -> Option<PathBuf> {
    if path.is_empty() || agent_id.is_empty() {
        return None;
    }
    let candidate = Path::new(path);
    if !wait_for_file(candidate, attempts) {
        return None;
    }
    let canonical = std::fs::canonicalize(candidate).ok()?;
    let name = canonical.file_name()?.to_str()?;
    (name.starts_with("rollout-") && name.ends_with(".jsonl") && name.contains(agent_id))
        .then_some(canonical)
}

/// [`latest_dispatch`] over a Codex rollout.
pub(crate) fn codex_latest_dispatch(path: &Path) -> Option<Dispatch> {
    if !wait_for_file(path, RETRY_ATTEMPTS) {
        return None;
    }
    latest_dispatch_with(path, codex_user_text)
}

fn dispatch_in(text: &str, prompt_of: fn(&str) -> Option<String>) -> Option<Dispatch> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    // `[0-9]` rather than `\d`: the ids are ASCII digits, which `\d` would widen to all Unicode digits.
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"tasks show ([0-9]+(?:,[0-9]+)*) --slug ([a-z0-9][a-z0-9-]{0,63})")
            .expect("dispatch pattern compiles")
    });
    static PLAN_VIEW: OnceLock<Regex> = OnceLock::new();
    let plan_view = PLAN_VIEW.get_or_init(|| {
        Regex::new(r"plan view --slug ([a-z0-9][a-z0-9-]{0,63}) --for worker --task ([0-9]+)")
            .expect("plan view dispatch pattern compiles")
    });
    static LEDGER: OnceLock<Regex> = OnceLock::new();
    // Multi-line: the line sits below a prompt's `DISPATCH:` header.
    let ledger = LEDGER.get_or_init(|| {
        Regex::new(
            r"(?m)^ledger: \.claude/flows/([a-z0-9][a-z0-9-]{0,63})/(?:review-ledger|optimise-findings|plan-review-findings|review-v2)\.toml(?: items: ([ROP][0-9]+(?:,[ROP][0-9]+)*))?",
        )
        .expect("ledger dispatch pattern compiles")
    });

    for line in text.lines().rev() {
        // A cheap rejection before a JSON parse: a line that cannot match is
        // skipped whatever its shape.
        if !line.contains("tasks show")
            && !line.contains("plan view")
            && !line.contains("ledger: .claude/flows/")
        {
            continue;
        }
        let Some(prompt) = prompt_of(line) else {
            continue;
        };
        let mut slug: Option<&str> = None;
        let mut task_ids = Vec::new();
        for caps in pattern.captures_iter(&prompt) {
            let (Some(id), Some(s)) = (caps.get(1), caps.get(2)) else {
                continue;
            };
            if !same_slug(&mut slug, s.as_str()) {
                return None;
            }
            task_ids.extend(id.as_str().split(',').filter_map(|n| n.parse::<u32>().ok()));
        }
        for caps in plan_view.captures_iter(&prompt) {
            let (Some(s), Some(id)) = (caps.get(1), caps.get(2)) else {
                continue;
            };
            if !same_slug(&mut slug, s.as_str()) {
                return None;
            }
            task_ids.extend(id.as_str().parse::<u32>().ok());
        }
        let mut item_ids: Vec<String> = Vec::new();
        for caps in ledger.captures_iter(&prompt) {
            let Some(s) = caps.get(1) else {
                continue;
            };
            if !same_slug(&mut slug, s.as_str()) {
                return None;
            }
            for id in caps.get(2).map_or("", |m| m.as_str()).split(',') {
                if !id.is_empty() && !item_ids.iter().any(|seen| seen == id) {
                    item_ids.push(id.to_string());
                }
            }
        }
        if let Some(slug) = slug {
            task_ids.sort_unstable();
            task_ids.dedup();
            return Some(Dispatch {
                slug: slug.to_string(),
                task_ids,
                item_ids,
            });
        }
    }
    None
}

/// Record `s` as the dispatch's slug; `false` when a different one is already
/// recorded.
fn same_slug<'a>(slug: &mut Option<&'a str>, s: &'a str) -> bool {
    match slug {
        Some(prev) if *prev != s => false,
        _ => {
            *slug = Some(s);
            true
        }
    }
}

/// The prose of a user-role transcript line: a string `message.content`, or
/// the concatenated text blocks of an array one. A line carrying any
/// `tool_result` block is a tool's output echoed back, not a prompt, and
/// yields `None` even when it quotes a dispatch command.
fn user_text(line: &str) -> Option<String> {
    if !line.contains("\"user\"") || line.contains("\"tool_result\"") {
        return None;
    }
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

/// The prompt prose of a Codex rollout line: the `input_text` blocks of a
/// user-role `response_item` message, or the `content` of an
/// `inter_agent_communication`, the form a spawn prompt takes under Codex's
/// v2 multi-agent protocol. Tool output is a `function_call_output` item.
fn codex_user_text(line: &str) -> Option<String> {
    let value: JsonValue = serde_json::from_str(line).ok()?;
    let payload = value.get("payload")?;
    match value.get("type")?.as_str()? {
        "response_item" => {
            if payload.get("type")?.as_str()? != "message"
                || payload.get("role")?.as_str()? != "user"
            {
                return None;
            }
            let mut out = String::new();
            for block in payload.get("content")?.as_array()? {
                if block.get("type").and_then(JsonValue::as_str) == Some("input_text")
                    && let Some(text) = block.get("text").and_then(JsonValue::as_str)
                {
                    out.push_str(text);
                    out.push('\n');
                }
            }
            Some(out)
        }
        "inter_agent_communication" => Some(payload.get("content")?.as_str()?.to_string()),
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

#[cfg(test)]
fn context_tokens(path: &Path) -> u64 {
    read_tail(path).map_or(0, |text| tokens_in(&text))
}

/// The context size at the agent's last assistant turn: the sum of that
/// line's input, cache-read, cache-creation and output token counts.
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

/// The context size at a Codex agent's last response: that response's
/// `total_tokens`, whose input count already includes the cached tokens.
fn codex_tokens_in(text: &str) -> u64 {
    for line in text.lines().rev() {
        if !line.contains("token_") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<JsonValue>(line) else {
            continue;
        };
        let Some(payload) = value.get("payload") else {
            continue;
        };
        let usage = match value.get("type").and_then(JsonValue::as_str) {
            Some("event_msg")
                if payload.get("type").and_then(JsonValue::as_str) == Some("token_count") =>
            {
                payload
                    .get("info")
                    .and_then(|info| info.get("last_token_usage"))
            }
            Some("token_usage_record") => payload.get("usage"),
            _ => None,
        };
        let Some(usage) = usage else {
            continue;
        };
        let count = |key: &str| usage.get(key).and_then(JsonValue::as_u64);
        return count("total_tokens").unwrap_or_else(|| {
            count("input_tokens")
                .unwrap_or(0)
                .saturating_add(count("output_tokens").unwrap_or(0))
        });
    }
    0
}

/// The whole file up to `WHOLE_READ_MAX`, else its last `TAIL_BYTES` from the
/// first line boundary on, so no partial line is ever parsed.
fn read_tail(path: &Path) -> Option<String> {
    let len = std::fs::metadata(path).ok()?.len();
    let cap = if len <= WHOLE_READ_MAX {
        WHOLE_READ_MAX
    } else {
        TAIL_BYTES
    };
    read_last(path, cap)
}

/// The file's last `cap` bytes from the first line boundary on, or the whole
/// file when it is no larger than `cap`.
fn read_last(path: &Path, cap: u64) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut bytes = Vec::new();
    if len <= cap {
        file.take(cap + 1).read_to_end(&mut bytes).ok()?;
        return Some(into_text(bytes));
    }
    // One byte early, so a window that opens on a line start keeps that line.
    file.seek(SeekFrom::Start(len - cap - 1)).ok()?;
    file.take(cap + 1).read_to_end(&mut bytes).ok()?;
    let start = memchr::memchr(b'\n', &bytes).map_or(bytes.len(), |i| i + 1);
    bytes.drain(..start);
    Some(into_text(bytes))
}

/// Take the buffer as text without copying it when it is valid UTF-8.
fn into_text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
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
    wait_for(attempts, || path.is_file().then_some(())).is_some()
}

/// Poll `probe` until it answers, at most `attempts` retries apart.
fn wait_for<T>(attempts: u32, probe: impl Fn() -> Option<T>) -> Option<T> {
    for _ in 0..attempts {
        if let Some(found) = probe() {
            return Some(found);
        }
        std::thread::sleep(RETRY_DELAY);
    }
    probe()
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
                item_ids: Vec::new(),
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
            dispatch_in(&text, user_text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: vec![5, 12],
                item_ids: Vec::new(),
            })
        );
    }

    #[test]
    fn a_comma_separated_id_list_yields_every_id() {
        let text = user_line("tomlctl tasks show 3,7 --slug some-slug --with body");
        assert_eq!(
            dispatch_in(&text, user_text),
            Some(Dispatch {
                slug: "some-slug".into(),
                task_ids: vec![3, 7],
                item_ids: Vec::new(),
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
        assert_eq!(dispatch_in(&text, user_text), None);
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
            dispatch_in(&text, user_text),
            Some(Dispatch {
                slug: "real-flow".into(),
                task_ids: vec![4],
                item_ids: Vec::new(),
            })
        );
    }

    fn codex_line(kind: &str, payload: serde_json::Value) -> String {
        serde_json::json!({"timestamp": "2026-09-29T10:00:01.000Z", "type": kind, "payload": payload})
            .to_string()
    }

    #[test]
    fn a_codex_rollout_names_its_dispatch_in_a_user_message_or_agent_message() {
        let prompt = codex_line(
            "response_item",
            serde_json::json!({"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "DISPATCH: implement-deep\n`tomlctl tasks show 16 --slug my-flow --with body`"}
            ]}),
        );
        let echoed = [
            codex_line(
                "response_item",
                serde_json::json!({"type": "function_call_output", "call_id": "c1",
                    "output": "tasks show 9 --slug echoed-flow"}),
            ),
            codex_line(
                "response_item",
                serde_json::json!({"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "running tasks show 8 --slug assistant-flow"}
                ]}),
            ),
            codex_line(
                "event_msg",
                serde_json::json!({"type": "agent_message", "message": "tasks show 7 --slug event-flow"}),
            ),
        ];
        let text = std::iter::once(prompt.clone())
            .chain(echoed.iter().cloned())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            dispatch_in(&text, codex_user_text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: vec![16],
                item_ids: Vec::new(),
            })
        );
        // Claude's reader finds nothing in a Codex rollout.
        assert_eq!(dispatch_in(&text, user_text), None);

        let retask = codex_line(
            "inter_agent_communication",
            serde_json::json!({"author": "/root", "recipient": "/root/worker",
                "other_recipients": [], "content": "next: tomlctl tasks show 3 --slug my-flow",
                "trigger_turn": true}),
        );
        assert_eq!(
            dispatch_in(&[prompt, retask].join("\n"), codex_user_text).map(|d| d.task_ids),
            Some(vec![3])
        );
    }

    #[test]
    fn codex_context_tokens_reads_the_last_response_usage() {
        let usage = |input: u64, cached: u64, output: u64| {
            serde_json::json!({"input_tokens": input, "cached_input_tokens": cached,
                "output_tokens": output, "reasoning_output_tokens": 0,
                "total_tokens": input + output})
        };
        let text = [
            codex_line(
                "event_msg",
                serde_json::json!({"type": "token_count", "info": {
                    "total_token_usage": usage(9000, 8000, 900),
                    "last_token_usage": usage(4000, 3500, 200),
                    "model_context_window": 272000}, "rate_limits": null}),
            ),
            codex_line(
                "event_msg",
                serde_json::json!({"type": "token_count", "info": null, "rate_limits": null}),
            ),
            codex_line(
                "response_item",
                serde_json::json!({"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "token_count"}]}),
            ),
        ]
        .join("\n");
        assert_eq!(codex_tokens_in(&text), 4200);

        let record = codex_line(
            "token_usage_record",
            serde_json::json!({"thread_id": "t", "turn_id": "1", "session_id": "s",
                "root_turn_id": "1", "response_id": "r",
                "usage": {"input_tokens": 50, "cached_input_tokens": 0, "output_tokens": 5},
                "turn_token_usage": usage(1, 0, 1), "thread_token_usage": usage(1, 0, 1)}),
        );
        assert_eq!(codex_tokens_in(&[text, record].join("\n")), 55);
        assert_eq!(codex_tokens_in(""), 0);
    }

    #[test]
    fn a_codex_rollout_must_be_named_for_its_agent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let day = tmp
            .path()
            .join("sessions")
            .join("2026")
            .join("09")
            .join("29");
        std::fs::create_dir_all(&day).expect("day dir");
        let thread = "0199a3c1-7f2e-7d40-9b1a-5c7e2f0a1b2c";
        let rollout = day.join(format!("rollout-2026-09-29T10-00-00-{thread}.jsonl"));
        std::fs::write(&rollout, "").expect("rollout");
        let stray = day.join("notes.jsonl");
        std::fs::write(&stray, "").expect("stray");
        let path = rollout.to_string_lossy().into_owned();

        assert!(codex_rollout_with(&path, thread, 0).is_some());
        assert_eq!(codex_rollout_with(&path, "another-thread", 0), None);
        assert_eq!(codex_rollout_with(&path, "", 0), None);
        assert_eq!(
            codex_rollout_with(&stray.to_string_lossy(), "notes", 0),
            None
        );
        assert_eq!(codex_rollout_with("", thread, 0), None);
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
        // The old dispatch sits before the tail window, but on the first line.
        assert_eq!(
            latest_dispatch(&agent).map(|d| d.slug),
            Some("old-flow".to_string())
        );
    }

    #[test]
    fn a_dispatch_in_the_tail_wins_over_the_first_line() {
        let (_tmp, _, agent) = session_tree(&[]);
        let filler = user_line(&"x".repeat(1000));
        let mut body = user_line("tasks show 1 --slug old-flow");
        body.push('\n');
        while (body.len() as u64) <= WHOLE_READ_MAX {
            body.push_str(&filler);
            body.push('\n');
        }
        body.push_str(&user_line("tasks show 2 --slug new-flow"));
        body.push('\n');
        std::fs::write(&agent, &body).expect("large transcript");
        let (dispatch, _) = dispatch_and_tokens(&agent);
        assert_eq!(dispatch.map(|d| d.slug), Some("new-flow".to_string()));
    }

    #[test]
    fn a_large_transcript_without_a_head_dispatch_gives_none() {
        let (_tmp, _, agent) = session_tree(&[]);
        let filler = user_line(&"x".repeat(1000));
        let mut body = String::new();
        while (body.len() as u64) <= WHOLE_READ_MAX {
            body.push_str(&filler);
            body.push('\n');
        }
        std::fs::write(&agent, &body).expect("large transcript");
        assert_eq!(latest_dispatch(&agent), None);
    }

    #[test]
    fn a_stop_reads_only_the_tail_of_a_large_transcript() {
        let (_tmp, _, agent) = session_tree(&[]);
        let filler = user_line(&"x".repeat(1000));
        let mut body = user_line("tasks show 1 --slug old-flow");
        body.push('\n');
        while (body.len() as u64) <= WHOLE_READ_MAX {
            body.push_str(&filler);
            body.push('\n');
        }
        std::fs::write(&agent, &body).expect("large transcript");
        assert_eq!(dispatch_and_tokens(&agent).0, None);
        // The start path still finds the spawn prompt on the first line.
        assert_eq!(
            latest_dispatch(&agent).map(|d| d.slug),
            Some("old-flow".to_string())
        );
    }

    /// A worker's transcript as a Workflow writes it: the relayed request,
    /// then the computed task naming the dispatch, then turns that quote the
    /// slug only in assistant and request-blob lines, past the stop's window.
    fn long_worker_transcript() -> String {
        let mut body = [
            user_line("[Workflow harness — user request] /implement-v2 my-flow"),
            user_line("slug: my-flow\n- `tomlctl plan view --slug my-flow --for worker --task 11`"),
        ]
        .join("\n");
        body.push('\n');
        let assistant = serde_json::json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "input": {"command": "tomlctl tasks show 8 --slug my-flow"}},
            {"type": "text", "text": "x".repeat(1000)}]}})
        .to_string();
        let blob = serde_json::json!({"type": "api-request-blob", "message": {"content": [
            {"type": "text", "text": "tomlctl tasks show 8 --slug my-flow"}]}})
        .to_string();
        while (body.len() as u64) <= TAIL_BYTES + 64 * 1024 {
            body.push_str(&assistant);
            body.push('\n');
            body.push_str(&blob);
            body.push('\n');
        }
        body
    }

    #[test]
    fn a_stop_past_the_tail_window_finds_the_spawn_dispatch_at_the_head() {
        let (_tmp, _, agent) = session_tree(&[]);
        std::fs::write(&agent, long_worker_transcript()).expect("transcript");
        assert!(std::fs::metadata(&agent).expect("stat").len() <= WHOLE_READ_MAX);
        // The tail holds no user line naming the dispatch.
        assert_eq!(dispatch_and_tokens(&agent).0, None);
        let task_11 = Some(Dispatch {
            slug: "my-flow".into(),
            task_ids: vec![11],
            item_ids: Vec::new(),
        });
        assert_eq!(spawn_dispatch(&agent), task_11);
        assert_eq!(latest_dispatch(&agent), task_11);
    }

    #[test]
    fn a_short_transcript_has_no_separate_spawn_dispatch() {
        let (_tmp, _, agent) = session_tree(&[user_line("tasks show 1 --slug my-flow")]);
        assert_eq!(spawn_dispatch(&agent), None);
    }

    #[test]
    fn a_workflow_agent_transcript_is_found_under_its_run() {
        let (tmp, parent, _) = session_tree(&[]);
        let run = tmp
            .path()
            .join("proj")
            .join("s1")
            .join("subagents")
            .join("workflows")
            .join("wf_1");
        std::fs::create_dir_all(&run).expect("run dir");
        std::fs::write(run.join("agent-w1.jsonl"), "").expect("transcript");
        let found = subagent_transcript_with(&parent, "s1", "w1", None, 0).expect("found");
        assert!(
            found.ends_with("workflows/wf_1/agent-w1.jsonl"),
            "{found:?}"
        );
        assert_eq!(subagent_transcript_with(&parent, "s1", "w2", None, 0), None);
    }

    #[test]
    fn first_timestamp_reads_the_earliest_stamped_line() {
        let lines = [
            serde_json::json!({"type": "queue-operation"}).to_string(),
            serde_json::json!({"type": "user", "timestamp": "not a time"}).to_string(),
            serde_json::json!({"type": "user", "timestamp": "2026-11-11T10:20:13.405Z"})
                .to_string(),
            serde_json::json!({"type": "user", "timestamp": "2026-11-11T10:21:00Z"}).to_string(),
        ];
        let (_tmp, _, agent) = session_tree(&lines);
        assert_eq!(
            first_timestamp(&agent).as_deref(),
            Some("2026-11-11T10:20:13.405Z")
        );
        std::fs::write(&agent, "").expect("empty");
        assert_eq!(first_timestamp(&agent), None);
    }

    #[test]
    fn a_plan_view_worker_line_links_one_task() {
        let text = user_line("run `tomlctl plan view --slug my-flow --for worker --task 6`");
        assert_eq!(
            dispatch_in(&text, user_text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: vec![6],
                item_ids: Vec::new(),
            })
        );
    }

    #[test]
    fn both_dispatch_forms_for_one_task_link_it_once() {
        let text = user_line(
            "tomlctl plan view --slug my-flow --for worker --task 6\n\
             tomlctl tasks show 6 --slug my-flow",
        );
        assert_eq!(
            dispatch_in(&text, user_text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: vec![6],
                item_ids: Vec::new(),
            })
        );
    }

    #[test]
    fn a_plan_view_line_naming_another_flow_gives_none() {
        let text = user_line(
            "tomlctl plan view --slug my-flow --for worker --task 6\n\
             tomlctl tasks show 6 --slug other-flow",
        );
        assert_eq!(dispatch_in(&text, user_text), None);
    }

    #[test]
    fn a_ledger_line_names_its_flow() {
        let text = user_line("ledger: .claude/flows/my-flow/review-ledger.toml");
        assert_eq!(
            dispatch_in(&text, user_text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: Vec::new(),
                item_ids: Vec::new(),
            })
        );
    }

    #[test]
    fn a_ledger_line_with_items_collects_the_item_ids() {
        let text = user_line(
            "ledger: .claude/flows/my-flow/optimise-findings.toml items: O5,O2,O5\n\
             ledger: .claude/flows/my-flow/optimise-findings.toml items: O9,O2",
        );
        assert_eq!(
            dispatch_in(&text, user_text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: Vec::new(),
                item_ids: vec!["O5".into(), "O2".into(), "O9".into()],
            })
        );
    }

    #[test]
    fn a_ledger_line_and_a_task_dispatch_must_name_the_same_flow() {
        let agreeing = user_line(
            "tomlctl tasks show 4 --slug my-flow\n\
             ledger: .claude/flows/my-flow/plan-review-findings.toml items: P1",
        );
        assert_eq!(
            dispatch_in(&agreeing, user_text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: vec![4],
                item_ids: vec!["P1".into()],
            })
        );
        let disagreeing = user_line(
            "tomlctl tasks show 4 --slug my-flow\n\
             ledger: .claude/flows/other-flow/review-ledger.toml",
        );
        assert_eq!(dispatch_in(&disagreeing, user_text), None);
    }

    #[test]
    fn ledger_line_attributes_review_v2_store() {
        let text = user_line("ledger: .claude/flows/my-flow/review-v2.toml");
        assert_eq!(
            dispatch_in(&text, user_text),
            Some(Dispatch {
                slug: "my-flow".into(),
                task_ids: Vec::new(),
                item_ids: Vec::new(),
            })
        );
    }

    #[test]
    fn a_flowless_ledger_line_is_not_a_dispatch() {
        let text = [
            user_line("ledger: .claude/reviews/my-scope.toml items: R1"),
            user_line("ledger: .claude/flows/my-flow/tasks.toml"),
        ]
        .join("\n");
        assert_eq!(dispatch_in(&text, user_text), None);
    }

    #[test]
    fn a_ledger_line_below_other_prompt_text_is_still_a_dispatch() {
        let text = user_line(
            "DISPATCH: implement-deep\n\
             ledger: .claude/flows/my-flow/review-ledger.toml items: R3,R7",
        );
        assert_eq!(
            dispatch_in(&text, user_text).map(|d| d.item_ids),
            Some(vec!["R3".to_string(), "R7".to_string()])
        );
    }
}
