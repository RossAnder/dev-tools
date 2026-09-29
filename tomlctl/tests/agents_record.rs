//! Black-box tests for `tomlctl agents record` and `tomlctl agents list`.
//!
//! Each test stages a flow inside a [`common::sandbox`] and a fake Claude Code
//! projects tree in a second tempdir: `<tmp>/projects/p/<session>.jsonl` beside
//! `<session>/subagents/agent-<id>.jsonl` and its `.meta.json`. Hook payloads
//! are piped on stdin, exactly as an async hook delivers them.

mod common;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use common::{TASKS_SLUG, cli, git_available, sandbox, seed_tasks};
use serde_json::{Value, json};

const SESSION: &str = "s1";

/// The fake `~/.claude/projects/<proj>/` directory. The `TempDir` keeps the
/// tree alive for the whole test.
struct Projects {
    _dir: tempfile::TempDir,
    proj: PathBuf,
}

impl Projects {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("projects").join("p");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join(format!("{SESSION}.jsonl")), "").unwrap();
        Self { _dir: dir, proj }
    }

    /// The parent session transcript the payload's `transcript_path` names.
    fn parent(&self) -> String {
        self.proj
            .join(format!("{SESSION}.jsonl"))
            .to_string_lossy()
            .into_owned()
    }

    fn transcript(&self, agent_id: &str) -> PathBuf {
        self.proj
            .join(SESSION)
            .join("subagents")
            .join(format!("agent-{agent_id}.jsonl"))
    }

    /// Write an agent's transcript holding `lines` and its meta file. The meta
    /// is always written: a missing one costs the binary its full retry wait.
    fn agent(&self, agent_id: &str, lines: &[String], meta: &Value) {
        let path = self.transcript(agent_id);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut body = String::new();
        for line in lines {
            body.push_str(line);
            body.push('\n');
        }
        fs::write(&path, body).unwrap();
        fs::write(path.with_extension("meta.json"), meta.to_string()).unwrap();
    }

    fn append(&self, agent_id: &str, line: &str) {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(self.transcript(agent_id))
            .unwrap();
        writeln!(file, "{line}").unwrap();
    }
}

fn user_line(content: &str) -> String {
    json!({"parentUuid": null, "type": "user",
           "message": {"role": "user", "content": content}})
    .to_string()
}

fn dispatch_line(task_id: u32, slug: &str) -> String {
    user_line(&format!(
        "DISPATCH: implement-deep\nFetch it: `tomlctl tasks show {task_id} --slug {slug} --with body,files,deps`"
    ))
}

fn subagent_meta() -> Value {
    json!({"agentType": "implement-deep", "description": "T2 work"})
}

fn teammate_meta(name: &str) -> Value {
    json!({"agentType": "implement-deep", "name": name, "teamName": "pool",
           "taskKind": "in_process_teammate"})
}

fn payload(event: &str, root: &Path, projects: &Projects, agent_id: &str) -> Value {
    json!({
        "hook_event_name": event,
        "session_id": SESSION,
        "agent_id": agent_id,
        "agent_type": "implement-deep",
        "transcript_path": projects.parent(),
        "cwd": root.to_string_lossy(),
    })
}

fn parse(stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout);
    serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("stdout must be one JSON value: {e}; got: {text}"))
}

fn record_with(mut cmd: Command, payload: &Value) -> Value {
    let out = cmd
        .args(["agents", "record", "--harness", "claude-code"])
        .write_stdin(payload.to_string())
        .assert()
        .success();
    parse(&out.get_output().stdout)
}

fn record(root: &Path, payload: &Value) -> Value {
    record_with(cli(root), payload)
}

fn list(root: &Path, slug: &str) -> Value {
    let out = cli(root)
        .args(["agents", "list", "--slug", slug])
        .write_stdin("")
        .assert()
        .success();
    parse(&out.get_output().stdout)
}

fn flow_dir(root: &Path, slug: &str) -> PathBuf {
    root.join(".claude").join("flows").join(slug)
}

fn store(root: &Path) -> PathBuf {
    flow_dir(root, TASKS_SLUG).join("agents.toml")
}

/// A sandbox holding the fixture flow, with its `context.toml` beside the
/// store as `record` requires.
fn flow() -> (tempfile::TempDir, PathBuf) {
    let (dir, root) = sandbox();
    seed_tasks(&root, "schema_version = 1\n");
    (dir, root)
}

fn task_ids(segment: &Value) -> Vec<u64> {
    segment["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_u64().unwrap())
        .collect()
}

#[test]
fn a_start_records_a_running_row_on_the_dispatched_task() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());

    let out = record(&root, &payload("SubagentStart", &root, &projects, "a1"));
    assert_eq!(
        out,
        json!({"recorded": true, "slug": TASKS_SLUG, "event": "start",
               "id": "A1", "task_ids": [2]})
    );

    let rows = list(&root, TASKS_SLUG);
    let row = &rows[0];
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(row["status"], "running");
    assert_eq!(row["kind"], "subagent");
    assert_eq!(row["agent_type"], "implement-deep");
    assert_eq!(row["session_id"], SESSION);
    assert_eq!(row["ended_at"], "");
    assert!(
        row["transcript_path"]
            .as_str()
            .unwrap()
            .ends_with("agent-a1.jsonl"),
        "{row}"
    );
    let segments = row["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(task_ids(&segments[0]), [2]);
    assert_eq!(segments[0]["ended_at"], "");
}

#[test]
fn a_repeated_start_on_the_same_task_leaves_the_store_untouched() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());
    let start = payload("SubagentStart", &root, &projects, "a1");

    record(&root, &start);
    let before = fs::read(store(&root)).unwrap();
    let again = record(&root, &start);
    assert_eq!(again["recorded"], true);
    assert_eq!(again["id"], "A1");
    assert_eq!(fs::read(store(&root)).unwrap(), before);
}

#[test]
fn a_stop_marks_the_subagent_stopped_with_its_summary() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());
    record(&root, &payload("SubagentStart", &root, &projects, "a1"));
    projects.append(
        "a1",
        &json!({"type": "assistant", "message": {"usage": {
            "input_tokens": 10, "cache_read_input_tokens": 200,
            "cache_creation_input_tokens": 30, "output_tokens": 5}}})
        .to_string(),
    );

    let mut stop = payload("SubagentStop", &root, &projects, "a1");
    stop["last_assistant_message"] = json!("  applied 2: done  ");
    stop["stop_hook_active"] = json!(false);
    let out = record(&root, &stop);
    assert_eq!(
        out,
        json!({"recorded": true, "slug": TASKS_SLUG, "event": "stop",
               "id": "A1", "task_ids": [2]})
    );

    let row = &list(&root, TASKS_SLUG)[0];
    assert_eq!(row["status"], "stopped");
    assert_eq!(row["summary"], "applied 2: done");
    assert_eq!(row["context_tokens"], 245);
    assert_ne!(row["ended_at"], "");
    let segments = row["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 1);
    assert_ne!(segments[0]["ended_at"], "");
}

#[test]
fn a_teammate_retask_closes_the_first_segment_and_opens_a_second() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent(
        "t1",
        &[dispatch_line(2, TASKS_SLUG)],
        &teammate_meta("worker-1"),
    );
    let start = payload("SubagentStart", &root, &projects, "t1");
    record(&root, &start);

    projects.append(
        "t1",
        &json!({"type": "assistant", "message": {"content": "applied 2"}}).to_string(),
    );
    projects.append(
        "t1",
        &json!({"type": "user", "message": {"content": [{"type": "text", "text":
            format!("<teammate-message>next: tomlctl tasks show 3 --slug {TASKS_SLUG} --with body,files,deps</teammate-message>")}]}})
        .to_string(),
    );
    let out = record(&root, &start);
    assert_eq!(out["id"], "A1");
    assert_eq!(out["task_ids"], json!([3]));

    let rows = list(&root, TASKS_SLUG);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    let row = &rows[0];
    assert_eq!(row["kind"], "teammate");
    assert_eq!(row["name"], "worker-1");
    assert_eq!(row["team"], "pool");
    assert_eq!(row["status"], "running");
    let segments = row["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(task_ids(&segments[0]), [2]);
    assert_ne!(segments[0]["ended_at"], "");
    assert_eq!(task_ids(&segments[1]), [3]);
    assert_eq!(segments[1]["ended_at"], "");
}

#[test]
fn a_teammate_idle_event_marks_the_teammate_idle() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent(
        "t1",
        &[dispatch_line(2, TASKS_SLUG)],
        &teammate_meta("worker-1"),
    );
    record(&root, &payload("SubagentStart", &root, &projects, "t1"));

    let idle = json!({
        "hook_event_name": "TeammateIdle",
        "session_id": SESSION,
        "teammate_name": "worker-1",
        "team_name": "pool",
        "transcript_path": projects.parent(),
        "cwd": root.to_string_lossy(),
    });
    let out = record(&root, &idle);
    assert_eq!(
        out,
        json!({"recorded": true, "slug": TASKS_SLUG, "event": "idle",
               "id": "A1", "task_ids": [2]})
    );

    let row = &list(&root, TASKS_SLUG)[0];
    assert_eq!(row["status"], "idle");
    assert_eq!(row["ended_at"], "", "an idle teammate has not ended");
    assert_ne!(row["segments"][0]["ended_at"], "");
}

#[test]
fn an_internal_agent_with_an_empty_type_is_not_recorded() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());

    for event in ["SubagentStart", "SubagentStop"] {
        let mut internal = payload(event, &root, &projects, "a1");
        internal["agent_type"] = json!("");
        assert_eq!(
            record(&root, &internal),
            json!({"recorded": false, "reason": "internal-agent"})
        );
    }
    assert!(!store(&root).exists());
}

#[test]
fn an_agent_with_no_dispatch_and_no_session_affinity_is_not_recorded() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent(
        "v1",
        &[user_line("commands: cargo test")],
        &json!({"agentType": "verification"}),
    );
    let mut start = payload("SubagentStart", &root, &projects, "v1");
    start["agent_type"] = json!("verification");

    assert_eq!(
        record(&root, &start),
        json!({"recorded": false, "reason": "no-flow"})
    );
    assert!(!store(&root).exists());
}

#[test]
fn a_second_agent_without_a_dispatch_is_attached_by_session_affinity() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());
    record(&root, &payload("SubagentStart", &root, &projects, "a1"));

    projects.agent(
        "v1",
        &[user_line("commands: cargo test")],
        &json!({"agentType": "verification"}),
    );
    let mut start = payload("SubagentStart", &root, &projects, "v1");
    start["agent_type"] = json!("verification");
    assert_eq!(
        record(&root, &start),
        json!({"recorded": true, "slug": TASKS_SLUG, "event": "start",
               "id": "A2", "task_ids": []})
    );

    let rows = list(&root, TASKS_SLUG);
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(rows[1]["agent_id"], "v1");
    assert_eq!(rows[1]["agent_type"], "verification");
    assert_eq!(rows[1]["status"], "running");
}

#[test]
fn a_dispatch_naming_a_flow_without_a_context_is_refused_and_creates_nothing() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(4, "ghost-flow")], &subagent_meta());

    assert_eq!(
        record(&root, &payload("SubagentStart", &root, &projects, "a1")),
        json!({"recorded": false, "reason": "unknown-flow"})
    );
    assert!(!flow_dir(&root, "ghost-flow").exists());
    assert!(!store(&root).exists());
}

#[test]
fn an_unsupported_event_is_not_recorded() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());

    assert_eq!(
        record(&root, &payload("PostToolUse", &root, &projects, "a1")),
        json!({"recorded": false, "reason": "unsupported-event"})
    );
    assert!(!store(&root).exists());
}

#[test]
fn a_harness_without_an_adapter_or_outside_the_vocabulary_fails() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());
    let start = payload("SubagentStart", &root, &projects, "a1").to_string();

    for harness in ["manual", "Claude-Code"] {
        cli(&root)
            .args(["agents", "record", "--harness", harness])
            .write_stdin(start.clone())
            .assert()
            .failure();
    }
    assert!(!store(&root).exists());
}

#[test]
fn a_payload_cwd_in_a_subdirectory_records_under_the_repo_root() {
    if !git_available() {
        eprintln!("skipping: git is not available to resolve the repo root");
        return;
    }
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());
    let nested = root.join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();

    // No `TOMLCTL_ROOT` and a launch directory outside the sandbox: only the
    // payload's `cwd` can lead the binary to the sandbox's repo root.
    let elsewhere = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env_remove("TOMLCTL_ROOT")
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .current_dir(elsewhere.path());
    let out = record_with(cmd, &payload("SubagentStart", &nested, &projects, "a1"));

    assert_eq!(out["recorded"], true, "{out}");
    assert_eq!(out["slug"], TASKS_SLUG);
    assert!(store(&root).is_file());
    assert!(!nested.join(".claude").exists());
    assert_eq!(list(&root, TASKS_SLUG)[0]["agent_id"], "a1");
}

#[test]
fn list_prints_every_row_with_the_store_field_order() {
    let (_dir, root) = flow();
    let projects = Projects::new();
    projects.agent("a1", &[dispatch_line(2, TASKS_SLUG)], &subagent_meta());
    projects.agent("a2", &[dispatch_line(3, TASKS_SLUG)], &subagent_meta());
    record(&root, &payload("SubagentStart", &root, &projects, "a1"));
    record(&root, &payload("SubagentStart", &root, &projects, "a2"));

    let rows = list(&root, TASKS_SLUG);
    let rows = rows.as_array().expect("list prints a JSON array");
    let ids: Vec<&str> = rows.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["A1", "A2"]);
    let keys: Vec<&str> = rows[0]
        .as_object()
        .unwrap()
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
    assert_eq!(rows[0]["harness"], "claude-code");
    let segment_keys: Vec<&str> = rows[1]["segments"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(segment_keys, ["task_ids", "started_at", "ended_at"]);
    assert_eq!(task_ids(&rows[1]["segments"][0]), [3]);
}

#[test]
fn list_of_a_flow_no_hook_has_touched_is_empty() {
    let (_dir, root) = flow();
    assert_eq!(list(&root, TASKS_SLUG), json!([]));
}
