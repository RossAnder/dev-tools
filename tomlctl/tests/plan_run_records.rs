//! Black-box coverage for the `plan run` leaves that record what a milestone's
//! run left behind: `plan run notes` reading each worker's run note into an
//! `[[attempt]]`, `plan run gate` recording a gate run as a `[[gate_round]]`,
//! and `plan run report` appending what an invocation left for a person.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{TASKS_SLUG, cli, parse_json_error_envelope, sandbox, seed_tasks, stdout_json};

const MILESTONE: &str = "M";
const RUN_ID: &str = "wf_7";

fn row(id: u32, status: &str) -> Value {
    json!({
        "id": id,
        "ref": format!("row-{id}"),
        "title": format!("Row {id}"),
        "effort": "S",
        "status": status,
        "checkpoint": MILESTONE,
        "files": [format!("src/row{id}.rs")],
    })
}

fn tasks_store(statuses: &[&str]) -> String {
    let items: Vec<Value> = statuses
        .iter()
        .enumerate()
        .map(|(i, status)| row(u32::try_from(i).unwrap() + 1, status))
        .collect();
    let doc = json!({
        "schema_version": 2,
        "checkpoints": [{ "id": MILESTONE, "rationale": "this one" }],
        "items": items,
    });
    toml::to_string(&doc).expect("the fixture serialises")
}

fn implement_run(run_id: &str, tasks: &[u32], started: &str) -> String {
    let tasks: Vec<String> = tasks.iter().map(u32::to_string).collect();
    format!(
        "\n[[run]]\nstep = \"implement:{MILESTONE}\"\nrun_id = \"{run_id}\"\n\
         started = {started}\nfinished = {started}\nskip_reason = \"\"\noutcome = \"ok\"\n\
         tasks = [{}]\n",
        tasks.join(", ")
    )
}

fn flow_dir(root: &Path) -> PathBuf {
    root.join(".claude").join("flows").join(TASKS_SLUG)
}

/// A flow whose task store holds one row per status and whose plan records
/// one `implement:M` run dispatching every row.
fn flow(statuses: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let (dir, root) = sandbox();
    seed_tasks(&root, &tasks_store(statuses));
    let ids: Vec<u32> = (1..=u32::try_from(statuses.len()).unwrap()).collect();
    fs::write(
        flow_dir(&root).join("plan.toml"),
        format!(
            "schema_version = 2\n{}",
            implement_run(RUN_ID, &ids, "2026-11-05T11:00:00Z")
        ),
    )
    .unwrap();
    (dir, root)
}

fn write_note(root: &Path, task: u32, text: &str) {
    let dir = root
        .join("docs")
        .join("flows")
        .join(TASKS_SLUG)
        .join("notes")
        .join("run");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("task-{task}.md")), text).unwrap();
}

fn notes(root: &Path) -> assert_cmd::assert::Assert {
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "run", "notes", "--slug", TASKS_SLUG])
        .args(["--milestone", MILESTONE])
        .assert()
}

fn plan_text(root: &Path) -> String {
    fs::read_to_string(flow_dir(root).join("plan.toml")).unwrap()
}

fn attempts(root: &Path) -> Vec<toml::Value> {
    let doc: toml::Value = toml::from_str(&plan_text(root)).expect("the store parses");
    doc.get("attempt")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn attempt_of(attempts: &[toml::Value], task: i64, run_id: &str) -> toml::Value {
    attempts
        .iter()
        .find(|a| a["task"].as_integer() == Some(task) && a["run_id"].as_str() == Some(run_id))
        .unwrap_or_else(|| panic!("no attempt for task {task} in run {run_id}: {attempts:?}"))
        .clone()
}

fn rows(out: &Value) -> Vec<Value> {
    out["rows"].as_array().expect("rows is an array").clone()
}

const BLOCK_NOTE: &str = r#"Status: done — the overlay lists a task's records

```toml run-note
files_touched = ["src/row1.rs"]

[[deviations]]
what = "split the overlay"
why = "the file was too long"
means = "one file"

[[tangential]]
kind = "debt"
area = "src/app.rs"
summary = "a long match"
why = "hard to extend"

[[checks]]
command = "cargo clippy"
result = "pass"

[handover]
reason = "restart"
action = "restart the session"
when = "after landing"
done_when = "the overlay shows"
```

## What changed

prose
"#;

const OLD_NOTE: &str = "Status: done — an old-style note\n\n## What changed\n\nprose\n\n\
    TANGENTIAL: bug | src/target.rs | a stale test | the gate fails\n\
    TANGENTIAL: flaky-test | tests/a.rs | it times out | it blocks the gate\n";

#[test]
fn a_block_note_records_every_field_and_reports_its_counts() {
    let (_dir, root) = flow(&["done"]);
    write_note(&root, 1, BLOCK_NOTE);

    let out = stdout_json(&notes(&root).success());
    assert_eq!(out["ok"], json!(true));
    assert_eq!(out["milestone"], json!(MILESTONE));
    assert_eq!(out["run_id"], json!(RUN_ID));
    assert_eq!(
        rows(&out),
        [json!({
            "task": 1,
            "status": "done",
            "block": "present",
            "deviations": 1,
            "tangential": 1,
            "handover": true,
            "problem": "",
        })]
    );

    let attempt = attempt_of(&attempts(&root), 1, RUN_ID);
    assert_eq!(attempt["milestone"].as_str(), Some(MILESTONE));
    assert_eq!(attempt["status"].as_str(), Some("done"));
    assert_eq!(
        attempt["summary"].as_str(),
        Some("the overlay lists a task's records")
    );
    assert_eq!(attempt["block"].as_str(), Some("present"));
    assert_eq!(
        attempt["files_touched"].as_array().unwrap()[0].as_str(),
        Some("src/row1.rs")
    );
    assert_eq!(attempt["deviations"][0]["means"].as_str(), Some("one file"));
    assert_eq!(attempt["tangential"][0]["kind"].as_str(), Some("debt"));
    assert_eq!(attempt["checks"][0]["result"].as_str(), Some("pass"));
    assert_eq!(attempt["handover"]["reason"].as_str(), Some("restart"));
}

#[test]
fn an_old_style_note_keeps_its_tangential_lines() {
    let (_dir, root) = flow(&["done"]);
    write_note(&root, 1, OLD_NOTE);

    let out = stdout_json(&notes(&root).success());
    let row = &rows(&out)[0];
    assert_eq!(row["block"], json!("absent"));
    assert_eq!(row["tangential"], json!(2));
    assert_eq!(row["problem"], json!(""));

    let attempt = attempt_of(&attempts(&root), 1, RUN_ID);
    assert_eq!(attempt["block"].as_str(), Some("absent"));
    let tangential = attempt["tangential"].as_array().unwrap();
    assert_eq!(tangential.len(), 2);
    assert_eq!(tangential[0]["area"].as_str(), Some("src/target.rs"));
    assert_eq!(tangential[1]["kind"].as_str(), Some("flaky-test"));
}

#[test]
fn a_missing_note_records_an_absent_attempt_with_the_row_status() {
    let (_dir, root) = flow(&["done", "failed"]);
    write_note(&root, 1, BLOCK_NOTE);

    let out = stdout_json(&notes(&root).success());
    let rows = rows(&out);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["task"], json!(2));
    assert_eq!(rows[1]["status"], json!("failed"));
    assert_eq!(rows[1]["block"], json!("absent"));
    let problem = rows[1]["problem"].as_str().unwrap();
    assert!(
        problem.contains("missing") && problem.contains("task-2.md"),
        "{problem}"
    );

    let attempt = attempt_of(&attempts(&root), 2, RUN_ID);
    assert_eq!(attempt["status"].as_str(), Some("failed"));
    assert_eq!(attempt["block"].as_str(), Some("absent"));
}

#[test]
fn a_status_that_disagrees_with_the_row_is_a_problem() {
    let (_dir, root) = flow(&["parked"]);
    write_note(&root, 1, BLOCK_NOTE);

    let out = stdout_json(&notes(&root).success());
    let row = &rows(&out)[0];
    assert_eq!(row["status"], json!("done"));
    let problem = row["problem"].as_str().unwrap();
    assert!(
        problem.contains("`done`") && problem.contains("`parked`"),
        "{problem}"
    );
    assert_eq!(
        attempt_of(&attempts(&root), 1, RUN_ID)["status"].as_str(),
        Some("done")
    );
}

#[test]
fn a_malformed_block_or_first_line_records_a_malformed_attempt() {
    let (_dir, root) = flow(&["done", "done"]);
    write_note(
        &root,
        1,
        "Status: done — x\n\n```toml run-note\nfiles_touched = [\n```\n",
    );
    write_note(&root, 2, "# Task 2\n\nStatus: done — x\n");

    let out = stdout_json(&notes(&root).success());
    let rows = rows(&out);
    for row in &rows {
        assert_eq!(row["block"], json!("malformed"), "{row}");
        let problem = row["problem"].as_str().unwrap();
        assert!(
            problem.contains("malformed") && !problem.contains('\n'),
            "{problem}"
        );
    }
    let recorded = attempts(&root);
    assert_eq!(recorded.len(), 2);
    assert!(
        recorded
            .iter()
            .all(|a| a["block"].as_str() == Some("malformed"))
    );
}

#[test]
fn a_second_call_writes_the_same_store() {
    let (_dir, root) = flow(&["done", "done", "failed"]);
    write_note(&root, 1, BLOCK_NOTE);
    write_note(&root, 2, OLD_NOTE);

    let first = stdout_json(&notes(&root).success());
    let after_first = plan_text(&root);
    let second = stdout_json(&notes(&root).success());
    assert_eq!(first, second);
    assert_eq!(plan_text(&root), after_first);
    assert_eq!(attempts(&root).len(), 3);
}

#[test]
fn a_later_run_records_attempts_beside_the_earlier_ones() {
    let (_dir, root) = flow(&["done"]);
    write_note(&root, 1, OLD_NOTE);
    notes(&root).success();

    let mut doc: toml::Value = toml::from_str(&plan_text(&root)).unwrap();
    let later: toml::Value =
        toml::from_str(&implement_run("wf_8", &[1], "2026-11-06T11:00:00Z")).unwrap();
    doc["run"]
        .as_array_mut()
        .unwrap()
        .push(later["run"][0].clone());
    fs::write(
        flow_dir(&root).join("plan.toml"),
        toml::to_string(&doc).unwrap(),
    )
    .unwrap();
    write_note(&root, 1, BLOCK_NOTE);

    let out = stdout_json(&notes(&root).success());
    assert_eq!(out["run_id"], json!("wf_8"));
    let recorded = attempts(&root);
    assert_eq!(recorded.len(), 2);
    assert_eq!(
        attempt_of(&recorded, 1, RUN_ID)["block"].as_str(),
        Some("absent")
    );
    assert_eq!(
        attempt_of(&recorded, 1, "wf_8")["block"].as_str(),
        Some("present")
    );
}

#[test]
fn only_the_attempt_array_is_written() {
    let (_dir, root) = flow(&["done"]);
    write_note(&root, 1, BLOCK_NOTE);
    let before: toml::Value = toml::from_str(&plan_text(&root)).unwrap();

    notes(&root).success();
    let after: toml::Value = toml::from_str(&plan_text(&root)).unwrap();
    let keys = |doc: &toml::Value| -> Vec<String> {
        doc.as_table()
            .unwrap()
            .keys()
            .filter(|k| !["attempt", "last_updated", "schema_version"].contains(&k.as_str()))
            .cloned()
            .collect()
    };
    assert_eq!(keys(&before), keys(&after));
    assert_eq!(before["run"], after["run"]);
}

#[test]
fn a_milestone_with_no_run_is_refused_naming_it() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, &tasks_store(&["done"]));
    fs::write(flow_dir(&root).join("plan.toml"), "schema_version = 2\n").unwrap();

    let assert = notes(&root).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], json!("not_found"));
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains(&format!("implement:{MILESTONE}")),
        "{err}"
    );
    assert_eq!(plan_text(&root), "schema_version = 2\n");
}

fn gate(root: &Path, round: u32, rerun: bool, rows: &[Value]) -> assert_cmd::assert::Assert {
    let stdin: String = rows.iter().map(|row| format!("{row}\n")).collect();
    let round = round.to_string();
    let mut cmd = cli(root);
    cmd.args(["--error-format", "json"])
        .args(["plan", "run", "gate", "--slug", TASKS_SLUG])
        .args(["--milestone", MILESTONE, "--round", &round, "--ndjson", "-"]);
    if rerun {
        cmd.arg("--rerun");
    }
    cmd.write_stdin(stdin).assert()
}

fn gate_rounds(root: &Path) -> Vec<toml::Value> {
    let doc: toml::Value = toml::from_str(&plan_text(root)).expect("the store parses");
    doc.get("gate_round")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// A log in the sandbox, which sits under the OS temp directory.
fn write_log(root: &Path, name: &str, text: &str) -> String {
    let dir = root.join("verify-logs");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

fn command_row(command: &str, outcome: &str, log: &str) -> Value {
    json!({
        "command": command,
        "outcome": outcome,
        "exit": if outcome == "pass" { 0 } else { 101 },
        "duration_s": 12,
        "summary": format!("{command} {outcome}"),
        "failed_ids": if outcome == "fail" { json!(["a::b"]) } else { json!([]) },
        "log": log,
    })
}

#[test]
fn a_failing_gate_round_keeps_the_cut_tail_of_its_log() {
    let (_dir, root) = flow(&["done"]);
    let long_line = "x".repeat(1000);
    let text: String = (1..=300).map(|n| format!("{n} {long_line}\n")).collect();
    let log = write_log(&root, "test.log", &text);

    let out = stdout_json(
        &gate(
            &root,
            0,
            false,
            &[
                command_row("cargo clippy", "pass", ""),
                command_row("cargo test", "fail", &log),
            ],
        )
        .success(),
    );
    let kept_path = format!(".claude/flows/{TASKS_SLUG}/gates/{MILESTONE}-r0-s1-c2.log");
    assert_eq!(
        out,
        json!({
            "ok": true,
            "milestone": MILESTONE,
            "round": 0,
            "outcome": "fail",
            "commands": [
                {"command": "cargo clippy", "outcome": "pass", "log": ""},
                {"command": "cargo test", "outcome": "fail", "log": kept_path},
            ],
            "unread": [],
        })
    );

    let kept = fs::read(root.join(&kept_path)).unwrap();
    assert!(kept.len() <= 64 * 1024, "{}", kept.len());
    assert!(
        String::from_utf8(kept)
            .unwrap()
            .ends_with(&format!("300 {long_line}\n"))
    );

    let rounds = gate_rounds(&root);
    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0]["seq"].as_integer(), Some(1));
    assert_eq!(rounds[0]["outcome"].as_str(), Some("fail"));
    let test = &rounds[0]["commands"][1];
    assert_eq!(test["exit"].as_integer(), Some(101));
    assert_eq!(test["duration_s"].as_integer(), Some(12));
    assert_eq!(test["failed_ids"][0].as_str(), Some("a::b"));
    assert_eq!(test["log"].as_str(), Some(kept_path.as_str()));
}

#[test]
fn the_tail_keeps_the_last_two_hundred_lines() {
    let (_dir, root) = flow(&["done"]);
    let text: String = (1..=300).map(|n| format!("line {n}\n")).collect();
    let log = write_log(&root, "short.log", &text);

    gate(&root, 0, false, &[command_row("cargo test", "flaky", &log)]).success();
    let kept = fs::read_to_string(
        flow_dir(&root)
            .join("gates")
            .join(format!("{MILESTONE}-r0-s1-c1.log")),
    )
    .unwrap();
    assert!(kept.starts_with("line 101\n"), "{kept}");
    assert_eq!(kept.lines().count(), 200);
}

#[test]
fn a_pass_round_keeps_no_log() {
    let (_dir, root) = flow(&["done"]);
    let log = write_log(&root, "pass.log", "all good\n");

    let out = stdout_json(
        &gate(
            &root,
            0,
            false,
            &[
                command_row("cargo test", "pass", &log),
                json!({"command": "cargo clippy", "outcome": "pass"}),
            ],
        )
        .success(),
    );
    assert_eq!(out["outcome"], json!("pass"));
    assert_eq!(out["unread"], json!([]));
    let round = &gate_rounds(&root)[0];
    let clippy = &round["commands"][1];
    assert!(clippy.get("exit").is_none());
    assert_eq!(clippy["duration_s"].as_integer(), Some(0));
    assert_eq!(clippy["summary"].as_str(), Some(""));
    assert!(round["commands"][0]["log"].as_str() == Some(""));
    assert!(!flow_dir(&root).join("gates").exists());
}

#[test]
fn a_rerun_replaces_only_the_commands_it_names() {
    let (_dir, root) = flow(&["done"]);
    let log = write_log(&root, "first.log", "boom\n");
    gate(
        &root,
        1,
        false,
        &[
            command_row("cargo clippy", "pass", ""),
            command_row("cargo test", "timeout", &log),
        ],
    )
    .success();
    assert_eq!(gate_rounds(&root)[0]["outcome"].as_str(), Some("timeout"));

    let out =
        stdout_json(&gate(&root, 1, true, &[command_row("cargo test", "pass", "")]).success());
    assert_eq!(out["outcome"], json!("pass"));
    let rounds = gate_rounds(&root);
    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0]["seq"].as_integer(), Some(1));
    let commands = rounds[0]["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0]["summary"].as_str(), Some("cargo clippy pass"));
    assert_eq!(commands[1]["outcome"].as_str(), Some("pass"));
    assert_eq!(commands[1]["log"].as_str(), Some(""));
}

#[test]
fn a_call_without_rerun_records_a_new_run_of_the_round() {
    let (_dir, root) = flow(&["done"]);
    let rows = [command_row("cargo test", "fail", "")];
    gate(&root, 0, false, &rows).success();
    gate(&root, 0, false, &rows).success();

    let rounds = gate_rounds(&root);
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0]["seq"].as_integer(), Some(1));
    assert_eq!(rounds[1]["seq"].as_integer(), Some(2));
}

#[test]
fn a_rerun_of_an_unrecorded_round_is_refused() {
    let (_dir, root) = flow(&["done"]);
    let before = plan_text(&root);
    let assert = gate(&root, 0, true, &[command_row("cargo test", "pass", "")]).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], json!("validation"));
    assert!(
        err["message"].as_str().unwrap().contains("--rerun"),
        "{err}"
    );
    assert_eq!(plan_text(&root), before);
}

#[test]
fn a_log_outside_the_temp_directory_or_unreadable_is_not_read() {
    let (_dir, root) = flow(&["done"]);
    let outside = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let missing = root.join("verify-logs").join("gone.log");

    let out = stdout_json(
        &gate(
            &root,
            0,
            false,
            &[
                command_row("cargo build", "fail", &outside.to_string_lossy()),
                command_row("cargo test", "fail", &missing.to_string_lossy()),
            ],
        )
        .success(),
    );
    assert_eq!(out["unread"], json!(["cargo build", "cargo test"]));
    let round = &gate_rounds(&root)[0];
    assert_eq!(round["commands"][0]["log"].as_str(), Some(""));
    assert_eq!(round["commands"][1]["log"].as_str(), Some(""));
    assert!(!flow_dir(&root).join("gates").exists());
}

#[test]
fn a_refused_row_writes_nothing_and_names_its_line() {
    let (_dir, root) = flow(&["done"]);
    let before = plan_text(&root);
    for bad in [
        json!({"command": "cargo test", "outcome": "pass", "status": "x"}),
        json!({"command": "cargo test", "outcome": "meh"}),
    ] {
        let assert = gate(
            &root,
            0,
            false,
            &[command_row("cargo clippy", "pass", ""), bad],
        )
        .failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        let err = parse_json_error_envelope(&stderr);
        assert_eq!(err["kind"], json!("validation"));
        assert!(err["message"].as_str().unwrap().contains("line 2"), "{err}");
        assert_eq!(plan_text(&root), before);
    }
}

#[test]
fn the_gate_leaf_writes_only_the_gate_round_array() {
    let (_dir, root) = flow(&["done"]);
    let before: toml::Value = toml::from_str(&plan_text(&root)).unwrap();
    gate(&root, 0, false, &[command_row("cargo test", "pass", "")]).success();
    let after: toml::Value = toml::from_str(&plan_text(&root)).unwrap();
    let keys = |doc: &toml::Value| -> Vec<String> {
        doc.as_table()
            .unwrap()
            .keys()
            .filter(|k| !["gate_round", "last_updated", "schema_version"].contains(&k.as_str()))
            .cloned()
            .collect()
    };
    assert_eq!(keys(&before), keys(&after));
    assert_eq!(before["run"], after["run"]);
    assert!(after.get("gate_round").is_some());
}

fn report(root: &Path, why: &str, rows: Option<&[Value]>) -> assert_cmd::assert::Assert {
    let mut cmd = cli(root);
    cmd.args(["--error-format", "json"])
        .args(["plan", "run", "report", "--slug", TASKS_SLUG])
        .args(["--milestone", MILESTONE, "--why", why]);
    if let Some(rows) = rows {
        let stdin: String = rows.iter().map(|row| format!("{row}\n")).collect();
        cmd.args(["--ndjson", "-"]).write_stdin(stdin);
    }
    cmd.assert()
}

fn reports(root: &Path) -> Vec<toml::Value> {
    let doc: toml::Value = toml::from_str(&plan_text(root)).expect("the store parses");
    doc.get("report")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

#[test]
fn a_report_keeps_its_entries_with_their_defaults() {
    let (_dir, root) = flow(&["parked"]);
    let out = stdout_json(
        &report(
            &root,
            "a task is parked",
            Some(&[
                json!({"kind": "parked", "task": 1, "text": "needs a restart", "path": "docs/x.md"}),
                json!({"kind": "audit", "text": "the run was not audited", "needs_person": false}),
            ]),
        )
        .success(),
    );
    assert_eq!(
        out,
        json!({"ok": true, "milestone": MILESTONE, "entries": 2})
    );

    let recorded = reports(&root);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0]["milestone"].as_str(), Some(MILESTONE));
    assert_eq!(recorded[0]["why"].as_str(), Some("a task is parked"));
    assert!(recorded[0]["at"].as_datetime().is_some());
    let entries = recorded[0]["entries"].as_array().unwrap();
    assert_eq!(entries[0]["kind"].as_str(), Some("parked"));
    assert_eq!(entries[0]["task"].as_integer(), Some(1));
    assert_eq!(entries[0]["path"].as_str(), Some("docs/x.md"));
    assert_eq!(entries[0]["needs_person"].as_bool(), Some(true));
    assert_eq!(entries[1]["kind"].as_str(), Some("audit"));
    assert_eq!(entries[1]["task"].as_integer(), Some(0));
    assert_eq!(entries[1]["path"].as_str(), Some(""));
    assert_eq!(entries[1]["needs_person"].as_bool(), Some(false));
}

#[test]
fn a_report_without_ndjson_has_no_entries() {
    let (_dir, root) = flow(&["done"]);
    let out = stdout_json(&report(&root, "nothing to report", None).success());
    assert_eq!(out["entries"], json!(0));
    let recorded = reports(&root);
    assert_eq!(recorded.len(), 1);
    assert!(
        recorded[0]
            .get("entries")
            .and_then(toml::Value::as_array)
            .is_none_or(Vec::is_empty)
    );
}

#[test]
fn two_reports_are_kept_in_order() {
    let (_dir, root) = flow(&["done"]);
    report(
        &root,
        "first",
        Some(&[json!({"kind": "gate", "text": "cargo test failed"})]),
    )
    .success();
    report(&root, "second", None).success();

    let recorded = reports(&root);
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0]["why"].as_str(), Some("first"));
    assert_eq!(recorded[0]["entries"][0]["kind"].as_str(), Some("gate"));
    assert_eq!(recorded[1]["why"].as_str(), Some("second"));
}

#[test]
fn a_refused_report_row_writes_nothing_and_names_its_line() {
    let (_dir, root) = flow(&["done"]);
    let before = plan_text(&root);
    for bad in [
        json!({"kind": "panic", "text": "x"}),
        json!({"kind": "gate", "text": "x", "status": "y"}),
        json!({"kind": "gate", "text": "x", "path": "../outside"}),
    ] {
        let assert = report(
            &root,
            "refused",
            Some(&[json!({"kind": "handover", "text": "ok"}), bad]),
        )
        .failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        let err = parse_json_error_envelope(&stderr);
        assert_eq!(err["kind"], json!("validation"));
        assert!(err["message"].as_str().unwrap().contains("line 2"), "{err}");
        assert_eq!(plan_text(&root), before);
    }
}

#[test]
fn the_report_leaf_writes_only_the_report_array() {
    let (_dir, root) = flow(&["done"]);
    let before: toml::Value = toml::from_str(&plan_text(&root)).unwrap();
    report(&root, "done", None).success();
    let after: toml::Value = toml::from_str(&plan_text(&root)).unwrap();
    let keys = |doc: &toml::Value| -> Vec<String> {
        doc.as_table()
            .unwrap()
            .keys()
            .filter(|k| !["report", "last_updated", "schema_version"].contains(&k.as_str()))
            .cloned()
            .collect()
    };
    assert_eq!(keys(&before), keys(&after));
    assert_eq!(before["run"], after["run"]);
    assert!(after.get("report").is_some());
}
