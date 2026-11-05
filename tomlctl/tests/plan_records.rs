//! `plan decide`, `plan contract`, `plan handover` and `plan run`.

use assert_cmd::Command;
use std::fs;
use std::path::Path;

const SLUG: &str = "rec-flow";

fn plan(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("tomlctl")
        .expect("the binary builds")
        .env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("plan")
        .args(args)
        .args(["--slug", SLUG])
        .write_stdin("")
        .assert()
}

fn ndjson(root: &Path, verb: &str, rows: &str) -> assert_cmd::assert::Assert {
    let file = root.join("rows.ndjson");
    fs::write(&file, rows).expect("rows file");
    plan(root, &[verb, "--ndjson", file.to_str().expect("utf8 path")])
}

fn sandbox() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical root");
    (dir, root)
}

fn store_text(root: &Path) -> String {
    fs::read_to_string(
        root.join(".claude")
            .join("flows")
            .join(SLUG)
            .join("plan.toml"),
    )
    .unwrap_or_default()
}

fn stdout_json(assert: &assert_cmd::assert::Assert) -> serde_json::Value {
    serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON")
}

fn stderr(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).to_string()
}

const GOOD: &str = r#"{"decision":"Use the helper.","provenance":"agent-assumed","door":"two-way","rejected":[{"option":"new","because":"dup"}]}"#;

#[test]
fn decisions_mint_ids_and_updates_keep_untouched_keys() {
    let (_dir, root) = sandbox();
    let out = ndjson(&root, "decide", &format!("{GOOD}\n{GOOD}\n")).success();
    assert_eq!(stdout_json(&out)["ids"], serde_json::json!(["D1", "D2"]));

    let out = ndjson(&root, "decide", "{\"id\":\"D1\",\"surfaced\":true}\n").success();
    assert_eq!(stdout_json(&out)["ids"], serde_json::json!(["D1"]));
    let text = store_text(&root);
    assert!(text.contains("surfaced = true"), "{text}");
    assert!(text.contains("Use the helper."), "{text}");
    assert!(text.contains("because = \"dup\""), "{text}");
}

#[test]
fn bad_vocabulary_unknown_keys_and_bad_rejected_are_refused() {
    let (_dir, root) = sandbox();
    let cases = [
        (
            r#"{"decision":"x","provenance":"guessed","door":"two-way"}"#,
            "agent-assumed",
        ),
        (
            r#"{"decision":"x","provenance":"user-stated","door":"revolving"}"#,
            "one-way",
        ),
        (
            r#"{"decision":"x","provenance":"user-stated","door":"one-way","stray":1}"#,
            "stray",
        ),
        (
            r#"{"decision":"x","provenance":"user-stated","door":"one-way","rejected":[{"option":"a"}]}"#,
            "option, because",
        ),
    ];
    for (row, needle) in cases {
        let out = ndjson(&root, "decide", &format!("{row}\n")).failure();
        assert!(stderr(&out).contains(needle), "{row}: {}", stderr(&out));
    }
    assert_eq!(store_text(&root), "", "a refused batch persists nothing");
}

#[test]
fn a_bad_row_after_a_good_one_persists_neither() {
    let (_dir, root) = sandbox();
    ndjson(&root, "decide", &format!("{GOOD}\n{{\"id\":\"D9\"}}\n")).failure();
    assert_eq!(store_text(&root), "");
}

#[test]
fn contracts_mint_validate_and_update() {
    let (_dir, root) = sandbox();
    let out = ndjson(
        &root,
        "contract",
        "{\"kind\":\"invariant\",\"claim\":\"both agree\",\"tasks\":[3,5]}\n",
    )
    .success();
    assert_eq!(stdout_json(&out)["ids"], serde_json::json!(["C1"]));

    let bad = ndjson(
        &root,
        "contract",
        "{\"kind\":\"promise\",\"claim\":\"x\"}\n",
    )
    .failure();
    assert!(stderr(&bad).contains("invariant"), "{}", stderr(&bad));

    ndjson(&root, "contract", "{\"id\":\"C1\",\"check\":\"true\"}\n").success();
    let text = store_text(&root);
    assert!(text.contains("check = \"true\""), "{text}");
    assert!(text.contains("both agree"), "{text}");
}

#[test]
fn handovers_mint_and_complete() {
    let (_dir, root) = sandbox();
    let out = plan(
        &root,
        &[
            "handover",
            "add",
            "--command",
            "cargo install",
            "--source-task",
            "3",
        ],
    )
    .success();
    assert_eq!(stdout_json(&out)["id"], "H1");
    plan(&root, &["handover", "done", "H1"]).success();
    assert!(store_text(&root).contains("done = true"));
    plan(&root, &["handover", "done", "H7"]).failure();
}

#[test]
fn a_removed_top_handover_id_is_not_reused() {
    let (_dir, root) = sandbox();
    plan(
        &root,
        &["handover", "add", "--command", "a", "--source-task", "1"],
    )
    .success();
    let path = root
        .join(".claude")
        .join("flows")
        .join(SLUG)
        .join("plan.toml");
    let text = fs::read_to_string(&path).expect("store");
    let cut = text.find("[[handover]]").expect("a handover row");
    fs::write(&path, &text[..cut]).expect("rewrite without the row");
    let out = plan(
        &root,
        &["handover", "add", "--command", "b", "--source-task", "1"],
    )
    .success();
    assert_eq!(stdout_json(&out)["id"], "H2");
}

#[test]
fn runs_start_finish_and_skip() {
    let (_dir, root) = sandbox();
    plan(&root, &["run", "finish", "--step", "investigate"]).failure();
    plan(
        &root,
        &[
            "run",
            "start",
            "--step",
            "investigate",
            "--workflow",
            "w",
            "--args-hash",
            "h",
        ],
    )
    .success();
    plan(
        &root,
        &["run", "finish", "--step", "investigate", "--run-id", "wf_9"],
    )
    .success();
    let text = store_text(&root);
    assert!(text.contains("run_id = \"wf_9\""), "{text}");
    assert!(!text.contains("finished = \"\""), "{text}");
    plan(&root, &["run", "finish", "--step", "investigate"]).failure();

    plan(
        &root,
        &["run", "skip", "--step", "review", "--reason", "low risk"],
    )
    .success();
    let text = store_text(&root);
    assert!(text.contains("skip_reason = \"low risk\""), "{text}");
    // The skipped row must still read back: a `[[run]]` needs `started`.
    plan(
        &root,
        &["handover", "add", "--command", "c", "--source-task", "1"],
    )
    .success();
}
