//! Black-box coverage for `plan emit`: which rows of a milestone reach the
//! Workflow's args, the reason each other member is left out, and the digest
//! printed beside the args; which commands the milestone's gate runs; and the
//! reproduce check's launches, built from the finding links and review stores.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{
    TASKS_SLUG, cli, git_available, git_command, hex_digest, parse_json_error_envelope, sandbox,
    seed_tasks, stdout_json,
};

const MILESTONE: &str = "M";
const OTHER: &str = "N";

/// Commits an empty tree and returns the head, or `None` when there is no git.
fn committed(root: &Path) -> Option<String> {
    if !git_available() {
        eprintln!("skipping: no git found");
        return None;
    }
    let commit = git_command(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false"])
        .args(["commit", "--allow-empty", "-q", "-m", "fixture"])
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let head = git_command(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    Some(String::from_utf8_lossy(&head.stdout).trim().to_string())
}

fn row(id: u32, status: &str, checkpoint: &str, needs: &[u32], files: &[&str]) -> Value {
    json!({
        "id": id,
        "ref": format!("row-{id}"),
        "title": format!("Row {id}"),
        "effort": "S",
        "status": status,
        "checkpoint": checkpoint,
        "needs": needs,
        "files": files,
    })
}

fn spec(id: u32, executor: &str, premises: Value) -> Value {
    json!({
        "ref": format!("row-{id}"),
        "outcome": "it holds",
        "premises": premises,
        "executor": executor,
    })
}

fn premise(check: &str, at: &str, verdict: Option<(&str, &str)>) -> Value {
    let mut premise = json!({ "claim": "it is there", "check": check, "at": at });
    if let Some((verdict, head)) = verdict {
        premise["probe"] = json!({ "at": head, "neg_rc": 0, "verdict": verdict });
    }
    premise
}

fn store(items: Vec<Value>, specs: Vec<Value>, extra: Value) -> String {
    let mut doc = json!({
        "schema_version": 2,
        "checkpoints": [
            { "id": MILESTONE, "rationale": "this one" },
            { "id": OTHER, "rationale": "a later one" },
        ],
        "items": items,
        "specs": specs,
    });
    if let Value::Object(extra) = extra {
        doc.as_object_mut().unwrap().extend(extra);
    }
    toml::to_string(&doc).expect("the fixture serialises")
}

fn launch_args(root: &Path, flags: &[&str]) -> assert_cmd::assert::Assert {
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "emit", "launch-args", "--slug", TASKS_SLUG])
        .args(["--milestone", MILESTONE])
        .args(flags)
        .assert()
}

fn dropped(out: &Value) -> Vec<(u64, String)> {
    out["dropped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["id"].as_u64().unwrap(),
                entry["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn launch_args_apply_the_dispatch_rules() {
    let (_dir, root) = sandbox();
    let Some(head) = committed(&root) else {
        return;
    };
    let holds = Some(("holds", head.as_str()));
    let fails = Some(("fails", head.as_str()));
    let anchor = "tomlctl review anchor F1 --scope fixture";
    let items = vec![
        row(1, "done", MILESTONE, &[], &["src/one.rs"]),
        row(2, "pending", MILESTONE, &[1], &["src/two.rs"]),
        row(3, "pending", MILESTONE, &[], &["src/three.rs"]),
        row(4, "pending", MILESTONE, &[3], &["src/four.rs"]),
        row(5, "pending", MILESTONE, &[], &["src/five.rs"]),
        row(6, "pending", MILESTONE, &[], &[]),
        row(7, "pending", MILESTONE, &[6], &["src/seven.rs"]),
        row(8, "pending", MILESTONE, &[9], &["src/eight.rs"]),
        row(9, "pending", OTHER, &[], &["src/nine.rs"]),
        row(10, "pending", MILESTONE, &[2], &["src/ten.rs"]),
        row(11, "in-progress", MILESTONE, &[], &["src/eleven.rs"]),
        row(12, "pending", MILESTONE, &[], &["src/twelve.rs"]),
        row(13, "pending", MILESTONE, &[], &["src/thirteen.rs"]),
    ];
    let mut items = items;
    items[11]["agent"] = json!("plan-worker-deep");
    let specs = vec![
        spec(1, "lite", json!([])),
        spec(2, "lite", json!([premise("true", "base", holds)])),
        spec(3, "lite", json!([premise("false", "base", fails)])),
        spec(4, "deep", json!([])),
        spec(5, "tool", json!([])),
        spec(6, "lite", json!([])),
        spec(7, "lite", json!([])),
        spec(8, "lite", json!([])),
        spec(9, "lite", json!([])),
        spec(10, "deep", json!([premise("true", "after:1", holds)])),
        spec(11, "lite", json!([])),
        spec(12, "deep", json!([premise(anchor, "base", fails)])),
        spec(13, "deep", json!([premise(anchor, "base", fails)])),
    ];
    let links = json!({ "finding_links": [
        { "ref": "row-12", "store": ".claude/review-v2/fixture.toml", "fixes": ["F1"], "settles": [] },
        { "ref": "row-13", "store": ".claude/review-v2/fixture.toml", "fixes": ["F1"], "settles": [] },
    ]});
    seed_tasks(&root, &store(items, specs, links));

    let out = stdout_json(&launch_args(&root, &[]).success());

    let tasks = out["args"]["tasks"].as_array().unwrap();
    let ids: Vec<u64> = tasks.iter().map(|t| t["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [2, 10, 12], "{out}");
    assert_eq!(
        tasks[0],
        json!({ "id": 2, "executor": "lite", "needs": [], "after": [], "files": ["src/two.rs"] }),
        "a need that is done leaves the list"
    );
    assert_eq!(tasks[1]["needs"], json!([2]), "a dispatched need stays");
    assert_eq!(tasks[1]["executor"], "deep");
    assert_eq!(
        dropped(&out),
        [
            (1, "not-pending".to_string()),
            (3, "withheld:3:0:fails".to_string()),
            (4, "needs-withheld:3".to_string()),
            (5, "executor:tool".to_string()),
            (6, "no-files".to_string()),
            (7, "need-not-done:6:pending".to_string()),
            (8, "need-not-done:9:pending".to_string()),
            (11, "not-pending".to_string()),
            (13, "withheld:13:0:fails".to_string()),
        ],
        "a failing anchor premise withholds only a row no worker has started: {out}"
    );
    assert_eq!(out["after_checks"], json!([]));
    assert_eq!(out["args"]["max_parallel"], 4);
    assert_eq!(out["args"]["milestone"], MILESTONE);
    assert_eq!(out["args"]["slug"], TASKS_SLUG);
}

#[test]
fn launch_args_keep_an_after_task_whose_target_is_dispatched() {
    let (_dir, root) = sandbox();
    if committed(&root).is_none() {
        return;
    }
    let items = vec![
        row(1, "pending", MILESTONE, &[], &["src/one.rs"]),
        row(2, "pending", MILESTONE, &[], &["src/two.rs"]),
        row(3, "pending", MILESTONE, &[], &["src/three.rs"]),
        row(4, "pending", OTHER, &[], &["src/four.rs"]),
        row(5, "pending", MILESTONE, &[], &["src/five.rs"]),
        row(6, "pending", MILESTONE, &[], &[]),
    ];
    let specs = vec![
        spec(1, "lite", json!([])),
        spec(
            2,
            "deep",
            json!([premise("grep -q landed src/one.rs", "after:1", None)]),
        ),
        spec(3, "lite", json!([premise("true", "after:4", None)])),
        spec(4, "lite", json!([])),
        spec(5, "lite", json!([premise("true", "after:6", None)])),
        spec(6, "lite", json!([])),
    ];
    seed_tasks(&root, &store(items, specs, json!({})));

    let out = stdout_json(&launch_args(&root, &["--max-parallel", "2"]).success());

    let tasks = out["args"]["tasks"].as_array().unwrap();
    let ids: Vec<u64> = tasks.iter().map(|t| t["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [1, 2], "{out}");
    assert_eq!(tasks[1]["after"], json!([1]));
    assert_eq!(tasks[1]["needs"], json!([]));
    assert_eq!(
        out["after_checks"],
        json!([{ "task": 2, "premise": 0, "after": 1, "check": "grep -q landed src/one.rs" }])
    );
    assert_eq!(
        dropped(&out),
        [
            (3, "withheld:3:0:unchecked".to_string()),
            (5, "withheld:5:0:unchecked".to_string()),
            (6, "no-files".to_string()),
        ],
        "an after: task whose task is neither done nor dispatched is withheld: {out}"
    );
    assert_eq!(out["args"]["max_parallel"], 2);
}

#[test]
fn launch_args_refuse_a_stale_premise_verdict() {
    let (_dir, root) = sandbox();
    if committed(&root).is_none() {
        return;
    }
    let stale = "0123456789abcdef0123456789abcdef01234567";
    let items = vec![
        row(1, "done", MILESTONE, &[], &["src/one.rs"]),
        row(2, "pending", MILESTONE, &[], &["src/two.rs"]),
    ];
    let specs = vec![
        spec(1, "lite", json!([])),
        spec(
            2,
            "lite",
            json!([
                premise("true", "base", Some(("holds", stale))),
                premise("true", "after:1", None),
            ]),
        ),
    ];
    seed_tasks(&root, &store(items, specs, json!({})));

    let assert = launch_args(&root, &[]).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("2:0, 2:1"), "{message}");
    assert!(
        message.contains("plan probes --emit --premises --milestone M"),
        "{message}"
    );
}

#[test]
fn launch_args_hash_matches_the_args() {
    let (_dir, root) = sandbox();
    if committed(&root).is_none() {
        return;
    }
    let items = vec![row(1, "pending", MILESTONE, &[], &["src/one.rs"])];
    let specs = vec![spec(1, "lite", json!([]))];
    seed_tasks(&root, &store(items, specs, json!({})));

    let out = stdout_json(&launch_args(&root, &[]).success());

    let args = &out["args"];
    let keys: Vec<&str> = args
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["slug", "milestone", "root", "max_parallel", "tasks"]);
    let compact = serde_json::to_string(args).unwrap();
    assert_eq!(
        out["args_sha256"],
        hex_digest(compact.as_bytes()),
        "{compact}"
    );
}

#[test]
fn launch_args_carry_a_compile_cap_inside_the_hash() {
    let (_dir, root) = sandbox();
    if committed(&root).is_none() {
        return;
    }
    let items = vec![row(1, "pending", MILESTONE, &[], &["src/one.rs"])];
    let specs = vec![spec(1, "lite", json!([]))];
    seed_tasks(&root, &store(items, specs, json!({})));

    let out = stdout_json(&launch_args(&root, &["--max-compile-parallel", "2"]).success());
    let args = &out["args"];
    let keys: Vec<&str> = args
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "slug",
            "milestone",
            "root",
            "max_parallel",
            "max_compile_parallel",
            "tasks"
        ]
    );
    assert_eq!(args["max_compile_parallel"], 2);
    let compact = serde_json::to_string(args).unwrap();
    assert_eq!(out["args_sha256"], hex_digest(compact.as_bytes()));

    for bad in ["0", "9"] {
        let assert = launch_args(&root, &["--max-compile-parallel", bad]).failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        let error = parse_json_error_envelope(&stderr);
        assert_eq!(error["kind"], "validation", "{stderr}");
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("--max-compile-parallel"),
            "{stderr}"
        );
    }
}

fn lane_check(root: &Path) -> assert_cmd::assert::Assert {
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "emit", "lane-check", "--slug", TASKS_SLUG])
        .args(["--milestone", MILESTONE])
        .assert()
}

fn tasks_update(root: &Path, args: &[&str]) {
    cli(root)
        .args(["tasks", "update"])
        .args(args)
        .args(["--slug", TASKS_SLUG])
        .assert()
        .success();
}

#[test]
fn lane_check_reports_paths_and_store_writes_outside_the_run() {
    let (_dir, root) = sandbox();
    let ignore = root.join(".gitignore");
    let mut ignored = fs::read_to_string(&ignore).unwrap();
    ignored.push_str("\n.claude/.locks/\n");
    fs::write(&ignore, ignored).unwrap();
    if git_available() {
        git_command(&root)
            .args(["add", ".gitignore"])
            .output()
            .unwrap();
    }
    if committed(&root).is_none() {
        return;
    }
    let items = vec![
        row(1, "pending", MILESTONE, &[], &["src/one.rs"]),
        row(2, "pending", MILESTONE, &[], &["src/two.rs"]),
    ];
    let specs = vec![spec(1, "lite", json!([])), spec(2, "lite", json!([]))];
    seed_tasks(&root, &store(items, specs, json!({})));
    let flow = root.join(".claude").join("flows").join(TASKS_SLUG);
    fs::write(flow.join("plan.toml"), "schema_version = 2\n").unwrap();

    let assert = lane_check(&root).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert_eq!(parse_json_error_envelope(&stderr)["kind"], "not_found");

    cli(&root)
        .args(["plan", "run", "start", "--slug", TASKS_SLUG])
        .args(["--step", "implement:M", "--tasks", "1", "--run-id", "wf_9"])
        .assert()
        .success();
    let plan = fs::read_to_string(flow.join("plan.toml")).unwrap();
    assert!(plan.contains("store_digest"), "{plan}");

    assert_eq!(
        stdout_json(&lane_check(&root).success()),
        json!({
            "clean": true,
            "run_id": "wf_9",
            "out_of_lane": [],
            "store_writes": [],
            "digest": "matched",
        })
    );

    tasks_update(
        &root,
        &["1", "--status", "done", "--agent", "plan-worker-lite"],
    );
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/one.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join(".claude/backlog.toml"), "").unwrap();
    let out = stdout_json(&lane_check(&root).success());
    assert_eq!(out["clean"], true, "the worker's own row and file: {out}");

    fs::write(root.join("src/two.rs"), "fn two() {}\n").unwrap();
    fs::write(root.join("stray.txt"), "x\n").unwrap();
    tasks_update(&root, &["2", "--status", "done"]);
    let out = stdout_json(&lane_check(&root).success());
    assert_eq!(out["clean"], false);
    assert_eq!(
        out["out_of_lane"],
        json!([
            { "path": "src/two.rs", "rows": [2] },
            { "path": "stray.txt", "rows": [] },
        ])
    );
    assert_eq!(
        out["store_writes"],
        json!([{ "source": "digest", "section": "rows" }])
    );
    assert_eq!(out["digest"], "changed");
}

const SUITE: &str = "cargo nextest run --manifest-path x/Cargo.toml";
const LINT: &str = "cargo clippy --manifest-path x/Cargo.toml";

fn accept(check: &str, writes_build: bool, owner: Option<(&str, &str)>) -> Value {
    let mut entry = json!({
        "behaviour": "it holds",
        "check": check,
        "kind": "forward",
        "writes_build": writes_build,
    });
    if let Some((run_by, covered_by)) = owner {
        entry["run_by"] = json!(run_by);
        if !covered_by.is_empty() {
            entry["covered_by"] = json!(covered_by);
        }
    }
    entry
}

fn accept_spec(id: u32, accept: Vec<Value>) -> Value {
    json!({ "ref": format!("row-{id}"), "outcome": "it holds", "accept": accept, "executor": "lite" })
}

fn gate_store(items: Vec<Value>, specs: Vec<Value>) -> String {
    store(
        items,
        specs,
        json!({ "milestones": [
            { "id": MILESTONE, "gates": [SUITE, LINT] },
            { "id": OTHER, "gates": ["cargo build --manifest-path y/Cargo.toml"] },
        ]}),
    )
}

fn gate_list(root: &Path) -> Value {
    stdout_json(
        &cli(root)
            .args(["--error-format", "json"])
            .args(["plan", "emit", "gate-list", "--slug", TASKS_SLUG])
            .args(["--milestone", MILESTONE])
            .assert()
            .success(),
    )
}

#[test]
fn gate_list_orders_gates_then_gate_owned_checks() {
    let (_dir, root) = sandbox();
    let items = vec![
        row(3, "pending", MILESTONE, &[], &["src/three.rs"]),
        row(1, "done", MILESTONE, &[], &["src/one.rs"]),
        row(2, "pending", OTHER, &[], &["src/two.rs"]),
    ];
    let specs = vec![
        accept_spec(
            3,
            vec![
                accept("cargo test --test three", true, None),
                accept("grep -q three src/three.rs", false, None),
                accept("tomlctl review reproduce-status", false, Some(("gate", ""))),
            ],
        ),
        accept_spec(
            1,
            vec![
                accept("grep -q one src/one.rs", false, Some(("worker", ""))),
                accept("cargo test --test one", true, None),
            ],
        ),
        accept_spec(2, vec![accept("cargo test --test two", true, None)]),
    ];
    seed_tasks(&root, &gate_store(items, specs));

    let out = gate_list(&root);

    assert_eq!(
        out["commands"],
        json!([
            { "command": SUITE, "source": "gate" },
            { "command": LINT, "source": "gate" },
            { "command": "cargo test --test one", "source": "accept:1:1" },
            { "command": "cargo test --test three", "source": "accept:3:0" },
            { "command": "tomlctl review reproduce-status", "source": "accept:3:2" },
        ]),
        "gates first, then gate-owned checks by row id and entry, worker checks and other \
         milestones left out: {out}"
    );
    assert_eq!(out["skipped"], json!([]));
    let keys: Vec<&str> = out
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["commands", "skipped"],
        "no option or hold is printed"
    );
}

#[test]
fn gate_list_skips_a_covered_check() {
    let (_dir, root) = sandbox();
    let items = vec![row(1, "pending", MILESTONE, &[], &["src/one.rs"])];
    let specs = vec![accept_spec(
        1,
        vec![
            accept("cargo test --test one", true, Some(("covered", SUITE))),
            accept("cargo test --test two", true, None),
        ],
    )];
    seed_tasks(&root, &gate_store(items, specs));

    let out = gate_list(&root);

    let commands: Vec<&str> = out["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["command"].as_str().unwrap())
        .collect();
    assert_eq!(commands, [SUITE, LINT, "cargo test --test two"], "{out}");
    assert_eq!(
        out["skipped"],
        json!([{ "task": 1, "accept": 0, "reason": format!("covered_by {SUITE}") }])
    );
}

#[test]
fn gate_list_skips_a_duplicate_command() {
    let (_dir, root) = sandbox();
    let reproduce = "tomlctl review reproduce-status --flow f --milestone M --expect gone";
    let items = vec![
        row(1, "pending", MILESTONE, &[], &["src/one.rs"]),
        row(2, "pending", MILESTONE, &[], &["src/two.rs"]),
    ];
    let specs = vec![
        accept_spec(
            1,
            vec![
                accept(reproduce, false, Some(("gate", ""))),
                accept(LINT, true, None),
            ],
        ),
        accept_spec(2, vec![accept(reproduce, false, Some(("gate", "")))]),
    ];
    seed_tasks(&root, &gate_store(items, specs));

    let out = gate_list(&root);

    assert_eq!(
        out["commands"],
        json!([
            { "command": SUITE, "source": "gate" },
            { "command": LINT, "source": "gate" },
            { "command": reproduce, "source": "accept:1:0" },
        ]),
        "{out}"
    );
    assert_eq!(
        out["skipped"],
        json!([
            { "task": 1, "accept": 1, "reason": "duplicate" },
            { "task": 2, "accept": 0, "reason": "duplicate" },
        ])
    );
}

const SCOPE: &str = "fixture";
const SCOPE_STORE: &str = ".claude/review-v2/fixture.toml";

fn flow_store() -> String {
    format!(".claude/flows/{TASKS_SLUG}/review-v2.toml")
}

fn link(row: u32, store: &str, fixes: &[&str]) -> Value {
    json!({ "ref": format!("row-{row}"), "store": store, "fixes": fixes, "settles": [] })
}

/// A finding routed to `route`, with `extra` keys merged in.
fn finding(id: &str, route: &str, extra: Value) -> Value {
    let mut finding = json!({
        "id": id, "family": "code", "kind": "cleanup", "status": "scheduled", "route": route,
    });
    if let Value::Object(extra) = extra {
        finding.as_object_mut().unwrap().extend(extra);
    }
    finding
}

fn reproduction(finding: &str, milestone: &str, round: u32, verdict: &str) -> Value {
    json!({
        "finding": finding, "flow": TASKS_SLUG, "milestone": milestone, "round": round,
        "task": "row-1", "verdict": verdict, "by": "verifier",
    })
}

fn seed_review(root: &Path, rel: &str, findings: Vec<Value>, reproductions: Vec<Value>) {
    let mut doc = json!({ "schema_version": 2, "findings": findings });
    if !reproductions.is_empty() {
        doc["reproductions"] = json!(reproductions);
    }
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, toml::to_string(&doc).unwrap()).unwrap();
}

fn plain_spec(id: u32) -> Value {
    spec(id, "lite", json!([]))
}

/// Seeds the milestone's rows, each with a spec, and the given links.
fn seed_apply_rows(root: &Path, rows: Vec<Value>, links: Vec<Value>) {
    let specs = rows
        .iter()
        .map(|row| plain_spec(u32::try_from(row["id"].as_u64().unwrap()).unwrap()))
        .collect();
    seed_tasks(root, &store(rows, specs, json!({ "finding_links": links })));
}

fn reproduce_units(root: &Path, flags: &[&str]) -> assert_cmd::assert::Assert {
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "emit", "reproduce-units", "--slug", TASKS_SLUG])
        .args(["--milestone", MILESTONE])
        .args(flags)
        .assert()
}

fn launches(root: &Path, flags: &[&str]) -> Vec<Value> {
    let out = stdout_json(&reproduce_units(root, flags).success());
    out["launches"].as_array().unwrap().clone()
}

/// Each unit of a launch as `(task, findings)`.
fn unit_findings(launch: &Value) -> Vec<(String, Vec<String>)> {
    launch["units"]
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| {
            let findings = unit["findings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap().to_string())
                .collect();
            (unit["task"].as_str().unwrap().to_string(), findings)
        })
        .collect()
}

fn refusal(root: &Path, flags: &[&str]) -> String {
    let assert = reproduce_units(root, flags).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    error["message"].as_str().unwrap().to_string()
}

#[test]
fn reproduce_units_give_a_shared_finding_to_the_highest_row() {
    let (_dir, root) = sandbox();
    let rows = vec![
        row(3, "done", MILESTONE, &[], &["src/three.rs"]),
        row(1, "done", MILESTONE, &[], &["src/one.rs"]),
        row(2, "done", MILESTONE, &[], &["src/two.rs"]),
        row(4, "done", OTHER, &[], &["src/four.rs"]),
        row(5, "done", MILESTONE, &[], &["src/five.rs"]),
    ];
    let mut settle = link(5, SCOPE_STORE, &[]);
    settle["settles"] = json!(["F5"]);
    let links = vec![
        link(1, SCOPE_STORE, &["F1", "F2"]),
        link(3, SCOPE_STORE, &["F2"]),
        link(2, SCOPE_STORE, &["F3"]),
        link(4, SCOPE_STORE, &["F2", "F4"]),
        settle,
    ];
    seed_apply_rows(&root, rows, links);
    let findings = ["F1", "F2", "F3", "F4", "F5"]
        .iter()
        .map(|id| finding(id, "verifier", json!({})))
        .collect();
    seed_review(&root, SCOPE_STORE, findings, Vec::new());
    let before = fs::read(root.join(SCOPE_STORE)).unwrap();

    let launches = launches(&root, &[]);

    assert_eq!(launches.len(), 1, "{launches:?}");
    assert_eq!(
        unit_findings(&launches[0]),
        [
            ("row-1".to_string(), vec!["F1".to_string()]),
            ("row-2".to_string(), vec!["F3".to_string()]),
            ("row-3".to_string(), vec!["F2".to_string()]),
        ],
        "F2 goes to row 3 alone, another milestone's row is ignored and a settles link \
         makes no unit: {launches:?}"
    );
    assert_eq!(launches[0]["units"][2]["files"], json!(["src/three.rs"]));
    assert_eq!(
        fs::read(root.join(SCOPE_STORE)).unwrap(),
        before,
        "the verb writes nothing"
    );
}

#[test]
fn reproduce_units_one_launch_per_store() {
    let (_dir, root) = sandbox();
    let flow_store = flow_store();
    let rows = vec![
        row(1, "done", MILESTONE, &[], &["src/one.rs"]),
        row(2, "done", MILESTONE, &[], &["src/two.rs"]),
    ];
    let links = vec![
        link(1, SCOPE_STORE, &["F1"]),
        link(1, &flow_store, &["F7"]),
        link(2, &flow_store, &["F8"]),
    ];
    seed_apply_rows(&root, rows, links);
    seed_review(
        &root,
        SCOPE_STORE,
        vec![finding("F1", "verifier", json!({}))],
        Vec::new(),
    );
    seed_review(
        &root,
        &flow_store,
        vec![
            finding("F7", "verifier", json!({})),
            finding("F8", "verifier", json!({})),
        ],
        vec![reproduction("F7", MILESTONE, 1, "reproduces")],
    );

    let launches = launches(&root, &[]);

    let stores: Vec<&Value> = launches.iter().map(|l| &l["store"]).collect();
    assert_eq!(
        stores,
        [&json!({ "slug": TASKS_SLUG }), &json!({ "scope": SCOPE })],
        "{launches:?}"
    );
    assert_eq!(
        launches[0]["run_dir"],
        format!(".review-v2-runs/apply-{TASKS_SLUG}-{MILESTONE}-2-{TASKS_SLUG}")
    );
    assert_eq!(
        launches[1]["run_dir"],
        format!(".review-v2-runs/apply-{TASKS_SLUG}-{MILESTONE}-1-{SCOPE}"),
        "a second store gets its own directory and its own round"
    );
    assert_eq!(
        unit_findings(&launches[0]),
        [
            ("row-1".to_string(), vec!["F7".to_string()]),
            ("row-2".to_string(), vec!["F8".to_string()]),
        ]
    );
    assert_eq!(
        unit_findings(&launches[1]),
        [("row-1".to_string(), vec!["F1".to_string()])]
    );
}

#[test]
fn reproduce_units_round_follows_the_store() {
    let (_dir, root) = sandbox();
    seed_apply_rows(
        &root,
        vec![row(1, "done", MILESTONE, &[], &["src/one.rs"])],
        vec![link(1, SCOPE_STORE, &["F1"])],
    );
    let mut elsewhere = reproduction("F1", MILESTONE, 9, "gone");
    elsewhere["flow"] = json!("another-flow");
    seed_review(
        &root,
        SCOPE_STORE,
        vec![finding("F1", "verifier", json!({}))],
        vec![
            reproduction("F1", MILESTONE, 2, "reproduces"),
            reproduction("F1", MILESTONE, 1, "reproduces"),
            reproduction("F1", OTHER, 7, "gone"),
            elsewhere,
        ],
    );

    let launches = launches(&root, &[]);

    assert_eq!(
        launches[0]["round"], 3,
        "one past this flow and milestone's highest round"
    );
    assert_eq!(
        launches[0]["run_dir"],
        format!(".review-v2-runs/apply-{TASKS_SLUG}-{MILESTONE}-3"),
        "a single store adds no suffix"
    );
}

#[test]
fn reproduce_units_route_checker_only_for_grep_decidable_units() {
    let (_dir, root) = sandbox();
    let rows = (1..=7)
        .map(|id| row(id, "done", MILESTONE, &[], &["src/lib.rs"]))
        .collect();
    let links = vec![
        link(1, SCOPE_STORE, &["F1", "F2"]),
        link(2, SCOPE_STORE, &["F3", "F4"]),
        link(3, SCOPE_STORE, &["F5"]),
        link(4, SCOPE_STORE, &["F6"]),
        link(5, SCOPE_STORE, &["F7"]),
        link(6, SCOPE_STORE, &["F8"]),
        link(7, SCOPE_STORE, &["F9"]),
    ];
    seed_apply_rows(&root, rows, links);
    seed_review(
        &root,
        SCOPE_STORE,
        vec![
            finding("F1", "checker", json!({ "direction": "change" })),
            finding("F2", "checker", json!({ "direction": "remove" })),
            finding("F3", "checker", json!({})),
            finding("F4", "verifier", json!({})),
            finding("F5", "checker", json!({ "fix_files": ["tests/lib.rs"] })),
            finding("F6", "checker", json!({ "direction": "add" })),
            finding("F7", "checker", json!({ "instances": ["src/b.rs"] })),
            finding("F8", "checker", json!({ "target_file": "src/c.rs" })),
            finding("F9", "checker", json!({ "fix_files": [], "instances": [] })),
        ],
        Vec::new(),
    );

    let launches = launches(&root, &[]);

    let routes: Vec<(&str, &str)> = launches[0]["units"]
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| {
            (
                unit["task"].as_str().unwrap(),
                unit["route"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        routes,
        [
            ("row-1", "checker"),
            ("row-2", "verifier"),
            ("row-3", "verifier"),
            ("row-4", "verifier"),
            ("row-5", "verifier"),
            ("row-6", "verifier"),
            ("row-7", "checker"),
        ],
        "one verifier finding, fix_files, an add, instances or a target_file sends the \
         unit to a verifier; empty lists do not: {launches:?}"
    );
}

#[test]
fn reproduce_units_relaunch_gets_its_own_run_dir() {
    let (_dir, root) = sandbox();
    seed_apply_rows(
        &root,
        vec![row(1, "done", MILESTONE, &[], &["src/one.rs"])],
        vec![link(1, SCOPE_STORE, &["F1"])],
    );
    seed_review(
        &root,
        SCOPE_STORE,
        vec![finding("F1", "verifier", json!({}))],
        Vec::new(),
    );

    let first = launches(&root, &[]);
    let relaunch = launches(
        &root,
        &["--relaunch", "1", "--round", "1", "--task", "row-1"],
    );
    let second = launches(
        &root,
        &["--relaunch", "2", "--round", "1", "--task", "row-1"],
    );

    let dirs: Vec<&str> = [&first, &relaunch, &second]
        .iter()
        .map(|l| l[0]["run_dir"].as_str().unwrap())
        .collect();
    let base = format!(".review-v2-runs/apply-{TASKS_SLUG}-{MILESTONE}-1");
    assert_eq!(
        dirs,
        [base.clone(), format!("{base}-r1"), format!("{base}-r2")],
        "no relaunch shares a directory with the launch before it"
    );
}

#[test]
fn reproduce_units_relaunch_keeps_its_round_and_named_units() {
    let (_dir, root) = sandbox();
    let rows = (1..=3)
        .map(|id| row(id, "done", MILESTONE, &[], &["src/lib.rs"]))
        .collect();
    let links = vec![
        link(1, SCOPE_STORE, &["F1"]),
        link(2, SCOPE_STORE, &["F2"]),
        link(3, SCOPE_STORE, &["F3"]),
    ];
    seed_apply_rows(&root, rows, links);
    seed_review(
        &root,
        SCOPE_STORE,
        vec![
            finding("F1", "verifier", json!({})),
            finding("F2", "verifier", json!({})),
            finding("F3", "verifier", json!({})),
        ],
        vec![
            reproduction("F1", MILESTONE, 2, "gone"),
            reproduction("F2", MILESTONE, 3, "reproduces"),
        ],
    );

    let launches = launches(
        &root,
        &["--relaunch", "1", "--round", "2", "--task", "row-1,row-3"],
    );

    assert_eq!(
        launches[0]["round"], 2,
        "the relaunched round, not a new one"
    );
    assert_eq!(
        unit_findings(&launches[0]),
        [
            ("row-1".to_string(), vec!["F1".to_string()]),
            ("row-3".to_string(), vec!["F3".to_string()]),
        ]
    );

    let message = refusal(&root, &["--round", "2"]);
    assert!(message.contains("only with `--relaunch`"), "{message}");
    let message = refusal(&root, &["--task", "row-1"]);
    assert!(message.contains("only with `--relaunch`"), "{message}");
    let message = refusal(&root, &["--relaunch", "1", "--task", "row-1"]);
    assert!(message.contains("--round"), "{message}");
    let message = refusal(&root, &["--relaunch", "1", "--round", "2"]);
    assert!(message.contains("--task"), "{message}");
    let message = refusal(
        &root,
        &["--relaunch", "1", "--round", "2", "--task", "row-1,row-9"],
    );
    assert!(message.contains("row-9"), "{message}");
}

#[test]
fn reproduce_units_since_launch_filters() {
    let (_dir, root) = sandbox();
    let rows = vec![
        row(1, "done", MILESTONE, &[], &["src/one.rs"]),
        row(2, "done", MILESTONE, &[], &["src/two.rs"]),
        row(3, "done", MILESTONE, &[], &["src/three.rs"]),
        row(4, "done", MILESTONE, &[], &["src/four.rs"]),
        row(5, "done", MILESTONE, &[], &["src/four.rs", "tests/four.rs"]),
    ];
    let links = vec![
        link(1, SCOPE_STORE, &["F1"]),
        link(2, SCOPE_STORE, &["F2"]),
        link(3, SCOPE_STORE, &["F3"]),
        link(4, SCOPE_STORE, &["F4"]),
    ];
    seed_apply_rows(&root, rows, links);
    seed_review(
        &root,
        SCOPE_STORE,
        vec![
            finding("F1", "verifier", json!({})),
            finding("F2", "verifier", json!({})),
            finding("F3", "verifier", json!({})),
            finding("F4", "verifier", json!({})),
        ],
        vec![
            reproduction("F1", MILESTONE, 1, "reproduces"),
            reproduction("F1", MILESTONE, 2, "gone"),
            reproduction("F2", MILESTONE, 1, "gone"),
            reproduction("F2", MILESTONE, 2, "reproduces"),
            reproduction("F4", MILESTONE, 1, "gone"),
        ],
    );
    let plan = root
        .join(".claude")
        .join("flows")
        .join(TASKS_SLUG)
        .join("plan.toml");
    let run = |tasks: &str| {
        format!(
            "[[run]]\nstep = \"implement:{MILESTONE}\"\nrun_id = \"\"\n\
             started = 2026-11-10T00:00:00Z\nfinished = 2026-11-10T01:00:00Z\n\
             skip_reason = \"\"\n{tasks}\n"
        )
    };
    let skipped = format!(
        "[[run]]\nstep = \"implement:{MILESTONE}\"\nrun_id = \"\"\n\
         started = 2026-11-10T02:00:00Z\nfinished = \"\"\nskip_reason = \"nothing to run\"\n"
    );
    fs::write(
        &plan,
        format!(
            "schema_version = 2\n\n{}\n{}\n{skipped}",
            run("tasks = [1, 2, 3]"),
            run("tasks = [5]")
        ),
    )
    .unwrap();

    let tasks: Vec<String> = launches(&root, &["--since-launch"])
        .iter()
        .flat_map(unit_findings)
        .map(|(task, _)| task)
        .collect();
    assert_eq!(
        tasks,
        ["row-2", "row-3", "row-4"],
        "row 1 is gone and untouched; row 2 still reproduces, row 3 has no verdict, and row 4 \
         is gone but shares a file with a row the latest launch dispatched"
    );

    fs::write(
        &plan,
        format!("schema_version = 2\n\n{}", run("tasks = [1]")),
    )
    .unwrap();
    let tasks: Vec<String> = launches(&root, &["--since-launch"])
        .iter()
        .flat_map(unit_findings)
        .map(|(task, _)| task)
        .collect();
    assert_eq!(tasks, ["row-1", "row-2", "row-3"], "{tasks:?}");

    fs::write(&plan, format!("schema_version = 2\n\n{}", run(""))).unwrap();
    let tasks = launches(&root, &["--since-launch"])[0]["units"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(tasks, 4, "a run that stored no task ids rules no unit out");
}

#[test]
fn reproduce_units_entry_matches_the_pipeline_args() {
    let (_dir, root) = sandbox();
    seed_apply_rows(
        &root,
        vec![row(1, "done", MILESTONE, &[], &["src/one.rs"])],
        vec![link(1, SCOPE_STORE, &["F1"])],
    );
    seed_review(
        &root,
        SCOPE_STORE,
        vec![finding("F1", "checker", json!({}))],
        Vec::new(),
    );

    let launches = launches(&root, &[]);

    assert_eq!(
        launches,
        [json!({
            "mode": "reproduce",
            "store": { "scope": SCOPE },
            "flow": TASKS_SLUG,
            "milestone": MILESTONE,
            "round": 1,
            "run_dir": format!(".review-v2-runs/apply-{TASKS_SLUG}-{MILESTONE}-1"),
            "units": [{
                "task": "row-1",
                "files": ["src/one.rs"],
                "findings": ["F1"],
                "route": "checker",
            }],
        })],
        "the store is an object and the task the row's ref, as the reproduce mode \
         validates them"
    );
}
