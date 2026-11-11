//! Black-box tests for `tomlctl agents audit`: transcripts are written as
//! fixture JSON lines into a tempdir, which the verb reads as a launch's
//! transcript directory.

mod common;

use std::fs;
use std::path::Path;

use common::{cli, stdout_json};
use serde_json::{Value, json};

/// One transcript line holding a Bash `tool_use` call, nested the way an agent
/// transcript records it.
fn bash_line(command: &str) -> String {
    json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "text", "text": "running"},
                {"type": "tool_use", "id": "t1", "name": "Bash",
                 "input": {"command": command, "description": "d"}}
            ]
        }
    })
    .to_string()
}

fn write_transcript(dir: &Path, name: &str, lines: &[String]) {
    fs::write(dir.join(name), lines.join("\n") + "\n").unwrap();
}

fn audit(dir: &Path, patterns: &[&str]) -> Value {
    let mut cmd = cli(dir);
    cmd.args(["agents", "audit", "--transcripts"]).arg(dir);
    for pattern in patterns {
        cmd.args(["--pattern", pattern]);
    }
    let assert = cmd.assert().success();
    stdout_json(&assert)
}

#[test]
fn audit_reports_gate_owned_commands() {
    let tmp = tempfile::tempdir().unwrap();
    let read_line = json!({
        "type": "assistant",
        "message": {"content": [{"type": "tool_use", "name": "Read",
                                 "input": {"file_path": "cargo test"}}]}
    })
    .to_string();
    write_transcript(
        tmp.path(),
        "agent-a1.jsonl",
        &[
            "not json at all".to_owned(),
            bash_line("cargo test --manifest-path tomlctl/Cargo.toml"),
            read_line,
        ],
    );
    write_transcript(
        tmp.path(),
        "agent-b2.jsonl",
        &[bash_line(
            "cargo nextest run --manifest-path tomlctl/Cargo.toml",
        )],
    );
    // Neither a non-agent file nor a nested directory is read.
    write_transcript(tmp.path(), "session.jsonl", &[bash_line("cargo build")]);
    fs::create_dir(tmp.path().join("nested")).unwrap();
    write_transcript(
        &tmp.path().join("nested"),
        "agent-c3.jsonl",
        &[bash_line("cargo build")],
    );

    assert_eq!(
        audit(tmp.path(), &[]),
        json!([
            {"agent": "agent-a1", "command": "cargo test --manifest-path tomlctl/Cargo.toml"},
            {"agent": "agent-b2", "command": "cargo nextest run --manifest-path tomlctl/Cargo.toml"},
        ])
    );
}

#[test]
fn audit_reports_recursive_grep() {
    let tmp = tempfile::tempdir().unwrap();
    let long = format!("grep -rn {} .", "x".repeat(300));
    write_transcript(
        tmp.path(),
        "agent-a1.jsonl",
        &[bash_line("grep -rn 'fn main' ."), bash_line(&long)],
    );

    let rows = audit(tmp.path(), &[]);
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["command"], "grep -rn 'fn main' .");
    assert_eq!(rows[1]["command"].as_str().unwrap().chars().count(), 200);
}

#[test]
fn audit_is_quiet_on_narrow_checks() {
    let tmp = tempfile::tempdir().unwrap();
    write_transcript(
        tmp.path(),
        "agent-a1.jsonl",
        &[
            bash_line("cargo clippy --manifest-path tomlctl/Cargo.toml"),
            bash_line("rg -n 'enum AgentsOp' tomlctl/src"),
        ],
    );

    assert_eq!(audit(tmp.path(), &[]), json!([]));
}

#[test]
fn audit_takes_custom_patterns() {
    let tmp = tempfile::tempdir().unwrap();
    write_transcript(
        tmp.path(),
        "agent-a1.jsonl",
        &[
            bash_line("cargo clippy --manifest-path tomlctl/Cargo.toml"),
            bash_line("cargo test"),
        ],
    );

    assert_eq!(
        audit(tmp.path(), &[r"cargo\s+clippy"]),
        json!([{"agent": "agent-a1", "command": "cargo clippy --manifest-path tomlctl/Cargo.toml"}])
    );
}
