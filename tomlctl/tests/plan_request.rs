//! Black-box coverage for `plan request`: a running worker's one request of
//! its run for files it does not hold and corrections to its own spec, ruled
//! at once where a rule settles it and left open for the orchestrator where
//! none does.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

use crate::common;
use common::{cli, stdout_json, write_tasks_store};

const SLUG: &str = "request-flow";
const MILESTONE: &str = "A";
const RUN_ID: &str = "wf_1";

struct Row {
    id: u32,
    needs: Vec<u32>,
    files: Vec<&'static str>,
}

fn row(id: u32, needs: &[u32], files: &[&'static str]) -> Row {
    Row {
        id,
        needs: needs.to_vec(),
        files: files.to_vec(),
    }
}

fn tasks_toml(rows: &[Row]) -> String {
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "ref": format!("row-{}", r.id),
                "title": format!("Row {}", r.id),
                "effort": "S",
                "status": "pending",
                "checkpoint": MILESTONE,
                "needs": r.needs,
                "files": r.files,
            })
        })
        .collect();
    let doc = json!({
        "schema_version": 2,
        "checkpoints": [{ "id": MILESTONE, "rationale": "this one" }],
        "items": items,
    });
    toml::to_string(&doc).expect("the fixture serialises")
}

fn flow_dir(root: &Path) -> PathBuf {
    root.join(".claude").join("flows").join(SLUG)
}

fn plan_text(root: &Path) -> String {
    fs::read_to_string(flow_dir(root).join("plan.toml")).unwrap()
}

/// A plan flow whose task store holds `rows` and whose plan carries an open
/// `implement:A` run dispatching `dispatched`, opened and started through the
/// verbs.
fn flow(rows: &[Row], dispatched: &[u32]) -> (tempfile::TempDir, PathBuf) {
    let (dir, root) = common::sandbox();
    cli(&root)
        .args(["plan", "open", "--slug", SLUG])
        .assert()
        .success();
    write_tasks_store(&flow_dir(&root).join("tasks.toml"), &tasks_toml(rows));
    let tasks: Vec<String> = dispatched.iter().map(u32::to_string).collect();
    cli(&root)
        .args(["plan", "run", "start", "--slug", SLUG])
        .args(["--step", &format!("implement:{MILESTONE}")])
        .args(["--run-id", RUN_ID])
        .args(["--tasks", &tasks.join(",")])
        .assert()
        .success();
    (dir, root)
}

fn generators(root: &Path) {
    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::write(
        root.join(".claude").join("generated.toml"),
        r#"[[generators]]
id = "canvas"
inputs = ["packages/x/tokens.css"]
outputs = ["packages/x/canvas.generated.css"]
regenerate = "bun run tokens"
check = "bun run tokens --check"
"#,
    )
    .unwrap();
}

fn add(root: &Path, task: u32, payload: &Value) -> assert_cmd::assert::Assert {
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "request", "add", "--slug", SLUG])
        .args(["--task", &task.to_string()])
        .args(["--json", "-"])
        .write_stdin(payload.to_string())
        .assert()
}

fn add_paths(root: &Path, task: u32, paths: &[&str]) -> assert_cmd::assert::Assert {
    let items: Vec<Value> = paths
        .iter()
        .map(|p| json!({ "path": p, "why": "the task needs it" }))
        .collect();
    add(root, task, &json!({ "paths": items }))
}

fn rule(root: &Path, id: &str, rows: &[Value]) -> assert_cmd::assert::Assert {
    let ndjson: String = rows.iter().map(|r| format!("{r}\n")).collect();
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "request", "rule", id, "--slug", SLUG])
        .args(["--ndjson", "-"])
        .write_stdin(ndjson)
        .assert()
}

fn list(root: &Path) -> Vec<Value> {
    let out = cli(root)
        .args(["plan", "request", "list", "--slug", SLUG])
        .assert()
        .success();
    stdout_json(&out).as_array().expect("rows").clone()
}

fn stderr(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).to_string()
}

fn item<'a>(out: &'a Value, name: &str) -> &'a Value {
    out["items"]
        .as_array()
        .and_then(|items| items.iter().find(|i| i["item"] == name))
        .unwrap_or_else(|| panic!("no item {name} in {out}"))
}

fn grant(item: &str, reason: &str) -> Value {
    json!({ "item": item, "ruling": "granted", "reason": reason })
}

#[test]
fn a_generated_output_of_a_held_input_is_granted_by_tomlctl_and_ruled_at_once() {
    let (_dir, root) = flow(
        &[
            row(1, &[], &["packages/x/tokens.css"]),
            row(2, &[1], &["packages/x/canvas.generated.css"]),
        ],
        &[1, 2],
    );
    generators(&root);

    let out = stdout_json(&add_paths(&root, 1, &["packages/x/canvas.generated.css"]).success());

    assert_eq!(out["status"], "ruled", "{out}");
    let p1 = item(&out, "p1");
    assert_eq!(p1["ruling"], "granted", "{out}");
    assert_eq!(p1["by"], "tomlctl", "{out}");
}

#[test]
fn a_generated_output_held_by_an_unrelated_pending_row_is_denied_naming_it() {
    let (_dir, root) = flow(
        &[
            row(1, &[], &["packages/x/tokens.css"]),
            row(2, &[1], &["packages/x/canvas.generated.css"]),
            row(3, &[], &["packages/x/canvas.generated.css"]),
        ],
        &[1, 2, 3],
    );
    generators(&root);

    let out = stdout_json(&add_paths(&root, 1, &["packages/x/canvas.generated.css"]).success());

    let p1 = item(&out, "p1");
    assert_eq!(p1["ruling"], "denied", "{out}");
    assert_eq!(p1["by"], "tomlctl", "{out}");
    assert!(p1["reason"].as_str().unwrap().contains("task 3"), "{out}");
}

#[test]
fn a_letter_case_variant_of_another_rows_path_is_denied_naming_that_row() {
    let (_dir, root) = flow(
        &[row(1, &[], &["src/one.ts"]), row(2, &[], &["src/Foo.ts"])],
        &[1, 2],
    );

    let out = stdout_json(&add_paths(&root, 1, &["src/foo.ts"]).success());

    let p1 = item(&out, "p1");
    assert_eq!(p1["ruling"], "denied", "{out}");
    assert!(p1["reason"].as_str().unwrap().contains("task 2"), "{out}");
}

#[test]
fn a_path_under_the_claude_directory_is_denied_as_forbidden() {
    let (_dir, root) = flow(&[row(1, &[], &["src/one.ts"])], &[1]);

    let out = stdout_json(&add_paths(&root, 1, &[".claude/settings.json"]).success());

    let p1 = item(&out, "p1");
    assert_eq!(p1["ruling"], "denied", "{out}");
    assert_eq!(p1["by"], "tomlctl", "{out}");
    assert!(p1["reason"].as_str().unwrap().contains(".claude/"), "{out}");
}

#[test]
fn a_path_the_task_already_holds_in_another_case_is_denied_as_held() {
    let (_dir, root) = flow(&[row(1, &[], &["src/Bar.ts"])], &[1]);

    let out = stdout_json(&add_paths(&root, 1, &["src/bar.ts"]).success());

    let p1 = item(&out, "p1");
    assert_eq!(p1["ruling"], "denied", "{out}");
    assert_eq!(p1["reason"], "already held by this task", "{out}");
}

#[test]
fn an_ordinary_path_stays_open_and_a_later_request_for_it_names_the_grant() {
    let (_dir, root) = flow(
        &[row(1, &[], &["src/one.ts"]), row(2, &[], &["src/two.ts"])],
        &[1, 2],
    );

    let out = stdout_json(&add_paths(&root, 1, &["src/new.ts"]).success());
    assert_eq!(out["status"], "open", "{out}");
    assert_eq!(item(&out, "p1")["ruling"], "open", "{out}");
    let id = out["id"].as_str().unwrap().to_string();

    let ruled = stdout_json(&rule(&root, &id, &[grant("p1", "no other row edits it")]).success());
    assert_eq!(ruled["status"], "ruled", "{ruled}");

    let rows = list(&root);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["paths"][0]["ruling"], "granted", "{rows:?}");
    assert_eq!(rows[0]["paths"][0]["by"], "orchestrator", "{rows:?}");

    let later = stdout_json(&add_paths(&root, 2, &["src/new.ts"]).success());
    let p1 = item(&later, "p1");
    assert_eq!(p1["ruling"], "denied", "{later}");
    assert!(p1["reason"].as_str().unwrap().contains(&id), "{later}");
}

#[test]
fn rule_refuses_a_grant_once_another_row_holds_the_path() {
    let (_dir, root) = flow(
        &[row(1, &[], &["src/one.ts"]), row(3, &[], &["src/three.ts"])],
        &[1, 3],
    );
    let out = stdout_json(&add_paths(&root, 1, &["src/late.ts"]).success());
    let id = out["id"].as_str().unwrap().to_string();

    write_tasks_store(
        &flow_dir(&root).join("tasks.toml"),
        &tasks_toml(&[
            row(1, &[], &["src/one.ts"]),
            row(3, &[], &["src/three.ts", "src/late.ts"]),
        ]),
    );
    let refused = rule(&root, &id, &[grant("p1", "looked free")]).failure();

    let text = stderr(&refused);
    assert!(text.contains("cannot grant"), "{text}");
    assert!(text.contains("task 3"), "{text}");
    assert_eq!(list(&root)[0]["status"], "open");
}

#[test]
fn rule_refuses_an_item_that_is_already_ruled() {
    let (_dir, root) = flow(&[row(1, &[], &["src/one.ts"])], &[1]);
    let out =
        stdout_json(&add_paths(&root, 1, &[".claude/forbidden.json", "src/new.ts"]).success());
    assert_eq!(out["status"], "open", "{out}");
    let id = out["id"].as_str().unwrap().to_string();

    let refused = rule(&root, &id, &[grant("p1", "try anyway")]).failure();

    let text = stderr(&refused);
    assert!(text.contains("already ruled"), "{text}");
}

#[test]
fn rule_refuses_a_request_that_is_no_longer_open() {
    let (_dir, root) = flow(&[row(1, &[], &["src/one.ts"])], &[1]);
    let out = stdout_json(&add_paths(&root, 1, &["src/new.ts"]).success());
    let id = out["id"].as_str().unwrap().to_string();
    rule(&root, &id, &[grant("p1", "free")]).success();

    let refused = rule(&root, &id, &[grant("p1", "again")]).failure();

    assert!(
        stderr(&refused).contains("only an open request"),
        "{refused:?}"
    );
}

#[test]
fn applied_refuses_an_open_request_and_marks_a_ruled_one() {
    let (_dir, root) = flow(&[row(1, &[], &["src/one.ts"])], &[1]);
    let out = stdout_json(&add_paths(&root, 1, &["src/new.ts"]).success());
    let id = out["id"].as_str().unwrap().to_string();
    let applied = |root: &Path| {
        cli(root)
            .args(["--error-format", "json"])
            .args(["plan", "request", "applied", &id, "--slug", SLUG])
            .assert()
    };

    let refused = applied(&root).failure();
    assert!(stderr(&refused).contains("still open"), "{refused:?}");
    assert_eq!(list(&root)[0]["applied"], false);

    rule(&root, &id, &[grant("p1", "free")]).success();
    let done = stdout_json(&applied(&root).success());

    assert_eq!(done["ids"], json!([id]), "{done}");
    assert_eq!(list(&root)[0]["applied"], true);
}

#[test]
fn a_second_request_from_the_same_task_in_a_run_is_refused() {
    let (_dir, root) = flow(&[row(1, &[], &["src/one.ts"])], &[1]);
    add_paths(&root, 1, &["src/a.ts"]).success();

    let refused = add_paths(&root, 1, &["src/b.ts"]).failure();

    let text = stderr(&refused);
    assert!(text.contains("already posted"), "{text}");
    assert_eq!(list(&root).len(), 1);
}

#[test]
fn a_request_from_a_task_no_open_run_lists_is_refused() {
    let (_dir, root) = flow(
        &[row(1, &[], &["src/one.ts"]), row(2, &[], &["src/two.ts"])],
        &[1],
    );

    let refused = add_paths(&root, 2, &["src/new.ts"]).failure();

    let text = stderr(&refused);
    assert!(text.contains("no open"), "{text}");
    assert!(list(&root).is_empty());
}

#[test]
fn an_unknown_key_in_the_payload_a_path_item_or_a_change_item_is_refused_naming_it() {
    let (_dir, root) = flow(&[row(1, &[], &["src/one.ts"])], &[1]);
    let path = json!({ "path": "src/a.ts", "why": "w" });
    let change = json!({ "field": "means", "what": "w", "evidence": "e" });

    let cases = [
        (json!({ "paths": [path.clone()], "extra": 1 }), "extra"),
        (
            json!({ "paths": [{ "path": "src/a.ts", "why": "w", "stray": 1 }] }),
            "stray",
        ),
        (
            json!({ "changes": [{ "field": "means", "what": "w", "evidence": "e", "odd": 1 }] }),
            "odd",
        ),
    ];
    for (payload, key) in cases {
        let refused = add(&root, 1, &payload).failure();
        let text = stderr(&refused);
        assert!(text.contains(key), "{key}: {text}");
    }
    // The same items without the stray key are accepted.
    add(&root, 1, &json!({ "paths": [path], "changes": [change] })).success();
}

#[test]
fn the_store_reads_schema_version_5_after_the_first_request() {
    let (_dir, root) = flow(&[row(1, &[], &["src/one.ts"])], &[1]);
    assert!(
        !plan_text(&root).contains("schema_version = 5"),
        "{}",
        plan_text(&root)
    );

    add_paths(&root, 1, &["src/new.ts"]).success();

    assert!(
        plan_text(&root).contains("schema_version = 5"),
        "{}",
        plan_text(&root)
    );
}
