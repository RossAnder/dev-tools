//! Black-box coverage for the `tomlctl inputs` verbs over a sandboxed
//! `TOMLCTL_ROOT`. Ids are read back out of the envelope that minted them.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{cli, parse_json_error_envelope, sandbox};

/// Run `tomlctl inputs <args…>` with `stdin`, require success, and parse
/// stdout as JSON.
fn inputs_with_stdin(root: &Path, args: &[&str], stdin: &str) -> Value {
    let out = cli(root)
        .arg("inputs")
        .args(args)
        .write_stdin(stdin)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "`inputs {}` stdout must be JSON: {e}; got: {stdout}",
            args.join(" ")
        )
    })
}

fn inputs(root: &Path, args: &[&str]) -> Value {
    inputs_with_stdin(root, args, "")
}

fn minted(envelope: &Value) -> String {
    envelope["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no `id` in {envelope}"))
        .to_string()
}

fn listed(root: &Path, args: &[&str]) -> Vec<Value> {
    let mut argv = vec!["list"];
    argv.extend_from_slice(args);
    inputs(root, &argv)["inputs"].as_array().unwrap().clone()
}

fn record(root: &Path, id: &str) -> Value {
    listed(root, &[])
        .into_iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no input record {id}"))
}

#[test]
fn inputs_add_then_list_pending() {
    let (_dir, root) = sandbox();
    let capture = minted(&inputs_with_stdin(
        &root,
        &["add", "--json", "-"],
        r#"{"kind": "capture", "text": "flaky probe", "capture_kind": "flaky-test"}"#,
    ));
    let payload = root.join("note.json");
    fs::write(
        &payload,
        r#"{"kind": "note", "text": "look at R3", "ledger": "review", "items": ["R3"]}"#,
    )
    .unwrap();
    let at_path = format!("@{}", payload.display());
    let note = minted(&inputs(&root, &["add", "--json", &at_path]));
    let done = minted(&inputs(
        &root,
        &[
            "add",
            "--json",
            r#"{"kind": "request", "text": "re-scope"}"#,
        ],
    ));
    assert_eq!(
        (capture.as_str(), note.as_str(), done.as_str()),
        ("I1", "I2", "I3")
    );

    let handled = inputs(
        &root,
        &["handle", &done, "--by", "review", "--note", "re-scoped"],
    );
    assert_eq!(handled["applied"], json!([done]));

    let full = inputs(&root, &["list"]);
    assert_eq!(full["path"], ".claude/inputs.toml");
    assert!(full["revision"].as_str().is_some_and(|r| r.len() == 64));
    let pending: Vec<Value> = listed(&root, &["--pending"])
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(pending, json!([capture, note]).as_array().unwrap().clone());
    let review: Vec<Value> = listed(&root, &["--pending", "--ledger", "review"])
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(review, vec![json!(note)]);
    let stored = record(&root, &capture);
    assert_eq!(stored["author"], "user");
    assert_eq!(stored["status"], "new");
    assert!(root.join(".claude").join("inputs.toml.sha256").is_file());
}

#[test]
fn inputs_answer_marks_the_question_handled() {
    let (_dir, root) = sandbox();
    let question = json!({
        "kind": "question",
        "author": "review",
        "ledger": "review",
        "items": ["R3"],
        "prompt": "Defer R3?",
        "choice": "single",
        "options": ["yes", "no"],
    })
    .to_string();
    let asked = minted(&inputs(&root, &["add", "--json", &question]));

    let out = inputs(
        &root,
        &[
            "answer",
            &asked,
            "--pick",
            "yes",
            "--text",
            "after the release",
        ],
    );
    let reply = minted(&out);
    assert_eq!(out["question"], asked);

    let closed = record(&root, &asked);
    assert_eq!(closed["status"], "handled");
    assert_eq!(closed["handled_by"], "user");
    assert_eq!(closed["handled_note"], format!("answered by {reply}"));
    assert_eq!(closed["answered_by"], reply);
    let answer = record(&root, &reply);
    assert_eq!(answer["kind"], "answer");
    assert_eq!(answer["answers"], asked);
    assert_eq!(answer["picked"], json!(["yes"]));
    assert_eq!(answer["text"], "after the release");
    assert_eq!(answer["ledger"], "review", "the answer inherits the target");

    let answers: Vec<Value> = listed(&root, &["--pending", "--kind", "answer"])
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(answers, vec![json!(reply)]);
}

#[test]
fn inputs_ack_skips_and_withdraw_reopens() {
    let (_dir, root) = sandbox();
    let id = minted(&inputs(
        &root,
        &["add", "--json", r#"{"kind": "note", "text": "x"}"#],
    ));
    let first = inputs(&root, &["ack", &id, "--by", "review"]);
    assert_eq!(first["applied"], json!([id]));
    let again = inputs(&root, &["ack", &id, "--by", "review"]);
    assert_eq!(
        again["skipped"],
        json!([{"id": id, "kind": "note", "status": "acknowledged"}])
    );
    assert_eq!(record(&root, &id)["acknowledged_by"], "review");

    let question =
        r#"{"kind": "question", "author": "review", "prompt": "Why?", "choice": "text"}"#;
    let asked = minted(&inputs(&root, &["add", "--json", question]));
    let held = inputs(&root, &["ack", &asked, "--by", "review"]);
    assert_eq!(
        held["skipped"],
        json!([{"id": asked, "kind": "question", "status": "new"}]),
        "a question waits on the user"
    );
    let reply = minted(&inputs(&root, &["answer", &asked, "--text", "because"]));
    let out = inputs(&root, &["withdraw", &reply]);
    assert_eq!(out["applied"], json!([reply]));
    assert_eq!(out["reopened"], json!([asked]));
    let reopened = record(&root, &asked);
    assert_eq!(reopened["status"], "new");
    assert!(reopened.get("answered_by").is_none(), "{reopened}");
}

#[test]
fn inputs_refusals_exit_nonzero_with_a_tagged_error() {
    let (_dir, root) = sandbox();
    let out = cli(&root)
        .args(["--error-format", "json", "inputs", "add", "--json"])
        .arg(r#"{"kind": "answer", "answers": "I1", "text": "x"}"#)
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    assert!(
        !root.join(".claude").join("inputs.toml").exists(),
        "a refused add writes nothing"
    );

    let out = cli(&root)
        .args(["--error-format", "json", "inputs", "add", "--json"])
        .arg(r#"{"kind": "question", "author": "user", "prompt": "Why?", "choice": "text"}"#)
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    assert!(
        error.to_string().contains("author"),
        "the refusal explains the invalid author: {stderr}"
    );
    assert!(
        !root.join(".claude").join("inputs.toml").exists(),
        "a refused question writes nothing"
    );

    let out = cli(&root)
        .args([
            "--error-format",
            "json",
            "inputs",
            "ack",
            "I9",
            "--by",
            "review",
        ])
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    assert!(
        error.to_string().contains("I9"),
        "the refusal names the unknown id: {stderr}"
    );
}

#[test]
fn inputs_handle_ndjson_notes_each_record() {
    let (_dir, root) = sandbox();
    let first = minted(&inputs(
        &root,
        &["add", "--json", r#"{"kind": "capture", "text": "a"}"#],
    ));
    let second = minted(&inputs(
        &root,
        &["add", "--json", r#"{"kind": "capture", "text": "b"}"#],
    ));
    let batch = format!(
        "{{\"id\":\"{first}\",\"note\":\"minted B-1\"}}\n\n{{\"id\":\"{second}\",\"note\":\"minted B-2\"}}\n"
    );
    let out = inputs_with_stdin(
        &root,
        &["handle", "--by", "backlog", "--ndjson", "-"],
        &batch,
    );
    assert_eq!(out["applied"], json!([first, second]));
    assert_eq!(record(&root, &first)["handled_note"], "minted B-1");
    assert_eq!(record(&root, &second)["handled_note"], "minted B-2");

    for argv in [
        vec!["handle", &first, "--by", "backlog", "--ndjson", "-"],
        vec!["handle", "--by", "backlog", "--note", "x", "--ndjson", "-"],
        vec!["handle", "--by", "backlog"],
    ] {
        cli(&root)
            .arg("inputs")
            .args(&argv)
            .write_stdin(batch.as_str())
            .assert()
            .failure();
    }
}

#[test]
fn inputs_list_on_a_missing_store() {
    let (_dir, root) = sandbox();
    let out = inputs(&root, &["list", "--verify-integrity"]);
    assert_eq!(out["inputs"], json!([]));
    assert!(out["revision"].is_null());
    cli(&root)
        .args(["inputs", "list", "--strict-read"])
        .write_stdin("")
        .assert()
        .failure();
}
