//! `plan decide`, `plan contract`, `plan handover` and `plan run`.

use assert_cmd::Command;
use std::fs;
use std::path::Path;

mod common;
use common::{parse_json_error_envelope, stdout_json};

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

fn opened() -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, root) = sandbox();
    plan(&root, &["open"]).success();
    (dir, root)
}

fn start_run(root: &Path, step: &str) {
    plan(
        root,
        &[
            "run",
            "start",
            "--step",
            step,
            "--workflow",
            "w",
            "--args-hash",
            "h",
        ],
    )
    .success();
}

fn open_runs(root: &Path) -> serde_json::Value {
    let out = plan(root, &["view", "--for", "digest"]).success();
    stdout_json(&out)["open_items"]["open_runs"].clone()
}

fn run_row(root: &Path, index: usize) -> toml::Value {
    let doc: toml::Value = toml::from_str(&store_text(root)).expect("the store parses");
    doc["run"][index].clone()
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

fn stderr(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).to_string()
}

const GOOD: &str = r#"{"decision":"Use the helper.","provenance":"agent-assumed","door":"two-way","rejected":[{"option":"new","because":"dup"}]}"#;

/// `plan open` refuses a flow recording a foreign plan, so a record verb that
/// seeded `plan.toml` there would convert a v1 flow behind its back.
#[test]
fn every_record_verb_refuses_a_flow_with_no_plan_toml() {
    let (_dir, root) = sandbox();
    let flow = root.join(".claude").join("flows").join(SLUG);
    fs::create_dir_all(&flow).expect("flow dir");
    fs::write(
        flow.join("context.toml"),
        "plan_path = \"docs/plans/v1.md\"\n",
    )
    .expect("context");
    let rows = root.join("rows.ndjson");
    fs::write(&rows, format!("{GOOD}\n")).expect("rows file");
    let rows = rows.to_str().expect("utf8 path");

    let verbs: [&[&str]; 8] = [
        &["decide", "--ndjson", rows],
        &["contract", "--ndjson", rows],
        &["handover", "add", "--command", "c", "--source-task", "1"],
        &["handover", "done", "H1"],
        &[
            "run",
            "start",
            "--step",
            "s",
            "--workflow",
            "w",
            "--args-hash",
            "h",
        ],
        &["run", "finish", "--step", "s"],
        &["run", "abandon", "--step", "s", "--reason", "dead"],
        &["run", "skip", "--step", "s", "--reason", "low risk"],
    ];
    for verb in verbs {
        let out = plan(&root, verb).failure();
        assert!(
            stderr(&out).contains(&format!("tomlctl plan open --slug {SLUG}")),
            "{verb:?}: {}",
            stderr(&out)
        );
        assert!(!flow.join("plan.toml").exists(), "{verb:?} seeded a store");
    }
}

#[test]
fn every_read_verb_on_an_unopened_flow_names_the_flow_and_plan_open() {
    let (_dir, root) = sandbox();
    let verbs: [&[&str]; 5] = [
        &["stage"],
        &["check"],
        &["gate"],
        &["drift"],
        &["view", "--for", "digest"],
    ];
    for verb in verbs {
        let out = Command::cargo_bin("tomlctl")
            .expect("the binary builds")
            .env("TOMLCTL_ROOT", &root)
            .args(["--error-format", "json", "plan"])
            .args(verb)
            .args(["--slug", SLUG])
            .write_stdin("")
            .assert()
            .failure();
        let error = parse_json_error_envelope(&stderr(&out));
        assert_eq!(error["kind"], "not_found", "{verb:?}: {error}");
        let message = error["message"].as_str().expect("a message");
        assert!(
            message.contains(&format!("flow `{SLUG}`")),
            "{verb:?}: {error}"
        );
        assert!(
            message.contains(&format!("tomlctl plan open --slug {SLUG}")),
            "{verb:?}: {error}"
        );
        assert!(message.contains("mistyped"), "{verb:?}: {error}");
        assert!(message.contains("v1 flow"), "{verb:?}: {error}");
        assert!(
            !root.join(".claude").join("flows").join(SLUG).exists(),
            "{verb:?} created the flow"
        );
    }
}

#[test]
fn a_mistyped_slug_creates_no_flow_directory() {
    let (_dir, root) = sandbox();
    plan(
        &root,
        &["handover", "add", "--command", "c", "--source-task", "1"],
    )
    .failure();
    assert!(!root.join(".claude").join("flows").join(SLUG).exists());
}

#[test]
fn decisions_mint_ids_and_updates_keep_untouched_keys() {
    let (_dir, root) = opened();
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
    let (_dir, root) = opened();
    let before = store_text(&root);
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
    assert_eq!(
        store_text(&root),
        before,
        "a refused batch persists nothing"
    );
}

#[test]
fn a_blank_decision_or_claim_is_refused_on_create_and_on_update() {
    let (_dir, root) = opened();
    ndjson(&root, "decide", &format!("{GOOD}\n")).success();
    ndjson(
        &root,
        "contract",
        "{\"kind\":\"invariant\",\"claim\":\"both agree\"}\n",
    )
    .success();
    let before = store_text(&root);
    let cases = [
        (
            "decide",
            r#"{"decision":"","provenance":"agent-assumed","door":"two-way"}"#,
            "`decision` is empty",
        ),
        (
            "decide",
            r#"{"decision":"   ","provenance":"agent-assumed","door":"two-way"}"#,
            "`decision` is empty",
        ),
        (
            "decide",
            r#"{"id":"D1","decision":""}"#,
            "`decision` is empty",
        ),
        (
            "decide",
            r#"{"id":"D1","decision":"  "}"#,
            "`decision` is empty",
        ),
        (
            "contract",
            r#"{"kind":"invariant","claim":""}"#,
            "`claim` is empty",
        ),
        (
            "contract",
            r#"{"kind":"invariant","claim":"  "}"#,
            "`claim` is empty",
        ),
        ("contract", r#"{"id":"C1","claim":""}"#, "`claim` is empty"),
        (
            "contract",
            r#"{"id":"C1","claim":"  "}"#,
            "`claim` is empty",
        ),
    ];
    for (verb, row, needle) in cases {
        let out = ndjson(&root, verb, &format!("{row}\n")).failure();
        assert!(stderr(&out).contains(needle), "{row}: {}", stderr(&out));
    }
    assert_eq!(store_text(&root), before, "a refused row persists nothing");
}

#[test]
fn a_bad_row_after_a_good_one_persists_neither() {
    let (_dir, root) = opened();
    let before = store_text(&root);
    ndjson(&root, "decide", &format!("{GOOD}\n{{\"id\":\"D9\"}}\n")).failure();
    assert_eq!(store_text(&root), before);
}

#[test]
fn contracts_mint_validate_and_update() {
    let (_dir, root) = opened();
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
    let (_dir, root) = opened();
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
    let (_dir, root) = opened();
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
    let (_dir, root) = opened();
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

#[test]
fn a_one_agent_step_starts_with_only_its_step_and_closes() {
    let (_dir, root) = opened();
    plan(&root, &["run", "start", "--step", "solo"]).success();

    let row = run_row(&root, 0);
    assert!(row.get("workflow").is_none(), "{row}");
    assert!(row.get("args_hash").is_none(), "{row}");
    assert!(row.get("session").is_none(), "{row}");
    assert_eq!(row["run_id"].as_str(), Some(""), "{row}");
    assert_eq!(open_runs(&root), serde_json::json!(["solo"]));

    plan(&root, &["run", "finish", "--step", "solo"]).success();
    assert_eq!(open_runs(&root), serde_json::json!([]));
}

#[test]
fn an_open_run_keeps_its_run_id_and_session_for_a_resume() {
    let (_dir, root) = opened();
    plan(
        &root,
        &[
            "run",
            "start",
            "--step",
            "s",
            "--workflow",
            "w",
            "--args-hash",
            "h",
            "--run-id",
            "wf_1",
            "--session",
            "sess_1",
        ],
    )
    .success();

    // Another verb's write re-emits the store, so the row must survive it.
    plan(
        &root,
        &["run", "skip", "--step", "t", "--reason", "low risk"],
    )
    .success();

    let row = run_row(&root, 0);
    assert_eq!(row["workflow"].as_str(), Some("w"), "{row}");
    assert_eq!(row["args_hash"].as_str(), Some("h"), "{row}");
    assert_eq!(row["run_id"].as_str(), Some("wf_1"), "{row}");
    assert_eq!(row["session"].as_str(), Some("sess_1"), "{row}");
}

#[test]
fn an_empty_session_is_not_stored() {
    let (_dir, root) = opened();
    plan(&root, &["run", "start", "--step", "s", "--session", ""]).success();
    assert!(run_row(&root, 0).get("session").is_none());
}

#[test]
fn finish_records_its_outcome_defaulting_to_ok() {
    let (_dir, root) = opened();
    start_run(&root, "a");
    plan(&root, &["run", "finish", "--step", "a"]).success();
    assert_eq!(run_row(&root, 0)["outcome"].as_str(), Some("ok"));

    for outcome in ["errors", "stopped"] {
        start_run(&root, outcome);
        plan(
            &root,
            &["run", "finish", "--step", outcome, "--outcome", outcome],
        )
        .success();
    }
    assert_eq!(run_row(&root, 1)["outcome"].as_str(), Some("errors"));
    assert_eq!(run_row(&root, 2)["outcome"].as_str(), Some("stopped"));
}

#[test]
fn finish_refuses_an_outcome_outside_its_vocabulary_and_leaves_the_run_open() {
    let (_dir, root) = opened();
    start_run(&root, "a");
    let before = store_text(&root);
    for outcome in ["abandoned", "fine"] {
        let out = plan(
            &root,
            &["run", "finish", "--step", "a", "--outcome", outcome],
        )
        .failure();
        assert!(
            stderr(&out).contains("ok, errors, stopped"),
            "{}",
            stderr(&out)
        );
    }
    assert_eq!(store_text(&root), before);
}

#[test]
fn abandon_closes_every_open_row_of_the_step_and_only_those() {
    let (_dir, root) = opened();
    start_run(&root, "review");
    plan(&root, &["run", "finish", "--step", "review"]).success();
    start_run(&root, "review");
    start_run(&root, "review");
    start_run(&root, "investigate");
    assert_eq!(
        open_runs(&root),
        serde_json::json!(["review", "review", "investigate"])
    );

    let out = plan(
        &root,
        &[
            "run",
            "abandon",
            "--step",
            "review",
            "--reason",
            "session died",
        ],
    )
    .success();
    assert_eq!(stdout_json(&out)["abandoned"], 2);

    assert_eq!(open_runs(&root), serde_json::json!(["investigate"]));
    assert_eq!(run_row(&root, 0)["outcome"].as_str(), Some("ok"));
    for index in [1, 2] {
        let row = run_row(&root, index);
        assert_eq!(row["outcome"].as_str(), Some("abandoned"), "{row}");
        assert_eq!(row["reason"].as_str(), Some("session died"), "{row}");
        assert_eq!(row["skip_reason"].as_str(), Some(""), "{row}");
        assert!(row["finished"].as_datetime().is_some(), "{row}");
    }
    assert!(run_row(&root, 3).get("outcome").is_none());
}

#[test]
fn abandon_shows_as_its_own_state_in_the_views() {
    let (_dir, root) = opened();
    start_run(&root, "review");
    plan(
        &root,
        &[
            "run",
            "abandon",
            "--step",
            "review",
            "--reason",
            "session died",
        ],
    )
    .success();

    let out = plan(&root, &["view", "--for", "digest"]).success();
    let run = stdout_json(&out)["stage_inputs"]["runs"][0].clone();
    assert_eq!(run["state"], "abandoned", "{run}");
    assert_eq!(run["outcome"], "abandoned", "{run}");
    assert_eq!(run["reason"], "session died", "{run}");

    let out = plan(&root, &["view", "--for", "human"]).success();
    let text = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        text.contains("- `review`: abandoned (session died), started"),
        "{text}"
    );
}

#[test]
fn abandon_refuses_a_blank_reason_and_a_step_with_nothing_open() {
    let (_dir, root) = opened();
    start_run(&root, "review");
    let before = store_text(&root);

    plan(
        &root,
        &["run", "abandon", "--step", "review", "--reason", "  "],
    )
    .failure();
    let out = plan(
        &root,
        &["run", "abandon", "--step", "other", "--reason", "dead"],
    )
    .failure();
    assert!(
        stderr(&out).contains("no unfinished run"),
        "{}",
        stderr(&out)
    );
    assert_eq!(store_text(&root), before);
}

#[test]
fn a_run_row_written_before_outcome_existed_still_reads() {
    let (_dir, root) = opened();
    let path = root
        .join(".claude")
        .join("flows")
        .join(SLUG)
        .join("plan.toml");
    let mut text = fs::read_to_string(&path).expect("store");
    text.push_str(
        "\n[[run]]\nstep = \"old\"\nworkflow = \"w\"\nargs_hash = \"h\"\nrun_id = \"\"\n\
         started = 2026-11-05T12:00:00Z\nfinished = 2026-11-05T12:30:00Z\nskip_reason = \"\"\n",
    );
    fs::write(&path, text).expect("store");

    let out = plan(&root, &["view", "--for", "digest"]).success();
    let run = stdout_json(&out)["stage_inputs"]["runs"][0].clone();
    assert_eq!(run["state"], "finished", "{run}");
    assert_eq!(run["outcome"], serde_json::Value::Null, "{run}");
    assert_eq!(run["args_hash"], "h", "{run}");
}
