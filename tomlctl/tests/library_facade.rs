//! The library facade against the CLI verbs it stands in for. glimpse calls
//! the facade in-process instead of spawning `tomlctl`, so each wrapper must
//! hand back exactly the JSON the matching verb prints.

use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{
    TASKS_SLUG, assert_sidecar_matches, cli, pin_root, sandbox, seed_ledger_in, seed_tasks,
};

/// Two rows, 1 → 2, with one checkpoint.
const STORE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["scaffold-the-module-tree", "wire-the-graph-engine"]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "the store and its engine"

[[items]]
id = 1
ref = "scaffold-the-module-tree"
title = "Scaffold the module tree"
effort = "S"
status = "done"
checkpoint = "A"
files = ["tomlctl/src/tasks/mod.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = "aaaa111"

[[items]]
id = 2
ref = "wire-the-graph-engine"
title = "Wire the graph engine"
effort = "M"
status = "in-progress"
checkpoint = "A"
files = ["tomlctl/src/tasks/graph.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

const RECORD: &str = r#"schema_version = 1
last_updated = 2026-09-07

[[items]]
id = "E1"
date = 2026-09-07
type = "task-completion"
task_ref = "scaffold-the-module-tree"
summary = "module tree in place"
"#;

const AGENTS: &str = r#"schema_version = 1

[[agents]]
id = "A1"
agent_id = "a1b2c3"
task_id = 2
state = "running"
"#;

/// A second flow whose `context.toml` has no `.sha256` sidecar, so any
/// integrity check on the listing path skips it.
fn stage_unsealed_flow(root: &Path) {
    let flow = root.join(".claude").join("flows").join("unsealed-flow");
    fs::create_dir_all(&flow).unwrap();
    fs::write(
        flow.join("context.toml"),
        "slug = \"unsealed-flow\"\n\
         plan_path = \"docs/plans/unsealed-flow.md\"\n\
         status = \"draft\"\n\
         created = 2026-09-08\n\
         updated = 2026-09-08\n\
         scope = []\n",
    )
    .unwrap();
}

fn cli_json(root: &Path, args: &[&str]) -> Value {
    let out = cli(root).args(args).write_stdin("").assert().success();
    serde_json::from_slice(&out.get_output().stdout).expect("stdout must be JSON")
}

#[test]
fn snapshot_matches_the_cli() {
    let (_dir, root) = sandbox();
    let store = seed_tasks(&root, STORE);
    let flow = store.parent().unwrap();
    fs::write(flow.join("execution-record.toml"), RECORD).unwrap();
    fs::write(flow.join("agents.toml"), AGENTS).unwrap();

    let facade = tomlctl::snapshot(&root, TASKS_SLUG).expect("facade snapshot");
    let verb = cli_json(&root, &["tasks", "snapshot", "--slug", TASKS_SLUG]);
    assert_eq!(facade, verb);
    assert_eq!(
        store.file_name().and_then(|name| name.to_str()),
        Some(tomlctl::SNAPSHOT_INPUTS[0])
    );
}

#[test]
fn snapshot_refuses_a_slug_the_cli_would_refuse() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, STORE);
    for slug in ["../x", "Foo"] {
        let err = tomlctl::snapshot(&root, slug).expect_err("the slug is invalid");
        let message = format!("{err:#}");
        assert!(
            message.contains(&format!("invalid slug: {slug}")),
            "unexpected error for {slug}: {message}"
        );
        assert!(tomlctl::validate_slug(slug).is_err(), "{slug} validates");
    }
    tomlctl::validate_slug("live-flow-demo").expect("a valid slug passes");
}

#[test]
fn flow_list_matches_the_cli() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, STORE);
    stage_unsealed_flow(&root);

    let facade = tomlctl::flow_list(&root).expect("facade flow list");
    let verb = cli_json(&root, &["flow", "list"]);
    assert_eq!(facade, verb);
    assert_eq!(facade["flows"].as_array().map(Vec::len), Some(2));
}

#[test]
fn record_agent_refuses_a_harness_without_an_adapter() {
    let err = tomlctl::record_agent("manual", &json!({})).expect_err("manual has no adapter");
    let message = format!("{err:#}");
    assert!(
        message.contains("manual payload adapter is not implemented"),
        "unexpected error: {message}"
    );
}

/// A `#[global_allocator]` in the library compiles cleanly and silently
/// becomes the allocator of every crate that links it, so only the binary's
/// `main.rs` may name one.
#[test]
fn only_the_binary_declares_a_global_allocator() {
    fn walk(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let main = src.join("main.rs");
    let mut files = Vec::new();
    walk(&src, &mut files);
    assert!(files.contains(&main), "the walk must reach src/main.rs");

    let offenders: Vec<_> = files
        .iter()
        .filter(|path| **path != main)
        .filter(|path| {
            fs::read_to_string(path)
                .unwrap()
                .contains("global_allocator")
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "only src/main.rs may declare a global allocator; also found in {offenders:?}"
    );
}

#[test]
fn record_agent_refuses_an_unknown_harness() {
    let err = tomlctl::record_agent("Codex", &json!({})).expect_err("harness is case-sensitive");
    let message = format!("{err:#}");
    assert!(
        message.contains("unknown harness `Codex`"),
        "unexpected error: {message}"
    );
}

const REVIEW_LEDGER: &str = r#"schema_version = 1
last_updated = 2026-09-30

[[items]]
id = "R1"
status = "open"
severity = "major"
first_flagged = 2026-09-29
summary = "nil deref"

[[items]]
id = "R2"
status = "fixed"
severity = "minor"
first_flagged = 2026-09-30
summary = "typo"
"#;

const BACKLOG: &str = r#"schema_version = 1
last_updated = 2026-09-30

[[backlog]]
id = "B-0001"
kind = "bug"
status = "open"
summary = "a captured bug"
"#;

fn ids(read: &Value) -> Vec<&str> {
    read["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|row| row["id"].as_str().expect("id is a string"))
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn ledger_read_returns_rows_and_revision() {
    let (_dir, root) = sandbox();
    let flow = root.join(".claude").join("flows").join("ledger-flow");
    fs::create_dir_all(&flow).unwrap();
    fs::write(flow.join("review-ledger.toml"), REVIEW_LEDGER).unwrap();
    let review = tomlctl::LedgerRef::Flow {
        slug: "ledger-flow".into(),
        kind: tomlctl::LedgerKind::Review,
    };

    let first = tomlctl::ledger_read(&root, &review).expect("read the review ledger");
    assert_eq!(ids(&first), ["R1", "R2"]);
    assert_eq!(first["kind"], "review");
    assert_eq!(
        first["path"],
        ".claude/flows/ledger-flow/review-ledger.toml"
    );
    assert_eq!(first["items"][0]["first_flagged"], "2026-09-29");
    assert_eq!(first["revision"], sha256_hex(REVIEW_LEDGER.as_bytes()));
    let again = tomlctl::ledger_read(&root, &review).expect("re-read the review ledger");
    assert_eq!(again["revision"], first["revision"]);

    let backlog = seed_ledger_in(&root, "backlog.toml", BACKLOG);
    let read = tomlctl::ledger_read(&root, &tomlctl::LedgerRef::Backlog).expect("read the backlog");
    assert_eq!(ids(&read), ["B-0001"]);
    assert_eq!(read["kind"], "backlog");
    let by_path =
        tomlctl::ledger_read(&root, &tomlctl::LedgerRef::File(backlog)).expect("read by path");
    assert_eq!(by_path["kind"], "backlog");
    assert_eq!(by_path["revision"], read["revision"]);

    let bad = tomlctl::LedgerRef::Flow {
        slug: "../x".into(),
        kind: tomlctl::LedgerKind::Review,
    };
    assert!(
        tomlctl::ledger_read(&root, &bad).is_err(),
        "a traversal slug is refused"
    );
}

#[test]
fn ledger_read_of_a_missing_file_is_empty() {
    let (_dir, root) = sandbox();
    let scope = tomlctl::LedgerRef::Scope {
        kind: tomlctl::LedgerKind::Optimise,
        scope: "absent".into(),
    };
    let read = tomlctl::ledger_read(&root, &scope).expect("a missing ledger reads");
    assert_eq!(read["items"], json!([]));
    assert_eq!(read["revision"], Value::Null);
    assert_eq!(read["kind"], "optimise");
    assert_eq!(read["path"], ".claude/optimise-findings/absent.toml");
}

#[test]
fn ledger_scopes_lists_ledger_only_flows_and_flowless_scopes() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, STORE);
    let claude = root.join(".claude");
    let ledger_only = claude.join("flows").join("ledger-only");
    fs::create_dir_all(&ledger_only).unwrap();
    fs::write(
        ledger_only.join("optimise-findings.toml"),
        "schema_version = 1\n",
    )
    .unwrap();
    fs::write(
        ledger_only.join("plan-review-findings.toml"),
        "schema_version = 1\n",
    )
    .unwrap();
    let context_only = claude.join("flows").join("context-only");
    fs::create_dir_all(&context_only).unwrap();
    fs::write(
        context_only.join("context.toml"),
        "slug = \"context-only\"\n",
    )
    .unwrap();
    let reviews = claude.join("reviews");
    fs::create_dir_all(&reviews).unwrap();
    fs::write(reviews.join("tomlctl.toml"), REVIEW_LEDGER).unwrap();
    fs::write(reviews.join("tomlctl.toml.sha256"), "0".repeat(64)).unwrap();
    fs::write(reviews.join("Not A Scope.toml"), REVIEW_LEDGER).unwrap();

    let scopes = tomlctl::ledger_scopes(&root).expect("list scopes");
    assert_eq!(
        scopes["flows"],
        json!([
            {"slug": TASKS_SLUG, "has_tasks": true, "ledgers": []},
            {"slug": "ledger-only", "has_tasks": false, "ledgers": ["optimise", "plan-review"]},
        ])
    );
    assert_eq!(
        scopes["scopes"],
        json!([{"kind": "review", "scope": "tomlctl"}])
    );
}

const WRITE_FLOW: &str = "write-flow";

const REVIEW_WRITE: &str = r#"schema_version = 1
last_updated = 2026-09-30

[[items]]
id = "R1"
file = "src/a.rs"
severity = "warning"
effort = "small"
category = "quality"
summary = "nil deref"
dedup_id = "0000000000000000"
status = "open"

[[items]]
id = "R2"
file = "src/b.rs"
severity = "suggestion"
effort = "trivial"
category = "style"
summary = "typo"
status = "open"

[[items]]
id = "R3"
file = "src/c.rs"
severity = "warning"
effort = "medium"
category = "perf"
summary = "quadratic scan"
status = "deferred"
defer_reason = "waits on the port"
defer_trigger = "when the port lands"
"#;

const OPTIMISE_WRITE: &str = r#"schema_version = 1
last_updated = 2026-09-30

[[items]]
id = "O1"
file = "src/a.rs"
severity = "warning"
effort = "small"
category = "alloc"
summary = "clone in a loop"
status = "open"
"#;

const PLAN_REVIEW_WRITE: &str = r#"schema_version = 1
last_updated = 2026-09-30

[[items]]
id = "P1"
severity = "warning"
summary = "task 4 has no acceptance"
status = "open"
"#;

fn flow_ledger(
    root: &Path,
    kind: tomlctl::LedgerKind,
    toml: &str,
) -> (tomlctl::LedgerRef, PathBuf) {
    let ledger = tomlctl::LedgerRef::Flow {
        slug: WRITE_FLOW.into(),
        kind,
    };
    let name = match kind {
        tomlctl::LedgerKind::Review => "review-ledger.toml",
        tomlctl::LedgerKind::Optimise => "optimise-findings.toml",
        tomlctl::LedgerKind::PlanReview => "plan-review-findings.toml",
    };
    let dir = root.join(".claude").join("flows").join(WRITE_FLOW);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, toml).unwrap();
    (ledger, path)
}

fn obj(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

fn row(root: &Path, ledger: &tomlctl::LedgerRef, id: &str) -> Value {
    let read = tomlctl::ledger_read(root, ledger).expect("read the ledger");
    read["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("no row {id}"))
}

fn last_updated(path: &Path) -> String {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    doc["last_updated"].to_string()
}

#[test]
fn ledger_transition_defers_an_open_finding() {
    let (_dir, root) = sandbox();
    let (ledger, path) = flow_ledger(&root, tomlctl::LedgerKind::Review, REVIEW_WRITE);
    let _env = pin_root(&root);

    let out = tomlctl::ledger_transition(
        &root,
        &ledger,
        &strings(&["R1", "R2"]),
        "deferred",
        obj(json!({"defer_reason": "out of scope", "defer_trigger": "next round"})),
        "open",
    )
    .expect("defer two open findings");
    assert_eq!(out, json!({"applied": ["R1", "R2"], "skipped_stale": []}));
    for id in ["R1", "R2"] {
        let r = row(&root, &ledger, id);
        assert_eq!(r["status"], "deferred");
        assert_eq!(r["defer_reason"], "out of scope");
        assert_eq!(r["defer_trigger"], "next round");
    }
    assert_eq!(last_updated(&path), "2026-09-30");
    assert_sidecar_matches(&path);
}

#[test]
fn ledger_transition_skips_a_changed_status() {
    let (_dir, root) = sandbox();
    let (ledger, path) = flow_ledger(&root, tomlctl::LedgerKind::Review, REVIEW_WRITE);
    let _env = pin_root(&root);
    let fields = || obj(json!({"wontfix_rationale": "intended"}));

    let out = tomlctl::ledger_transition(
        &root,
        &ledger,
        &strings(&["R1", "R3", "R9"]),
        "wontfix",
        fields(),
        "open",
    )
    .expect("the fresh row lands");
    assert_eq!(out["applied"], json!(["R1"]));
    assert_eq!(
        out["skipped_stale"],
        json!([
            {"id": "R9", "field": "id", "expected": "R9", "found": null},
            {"id": "R3", "field": "status", "expected": "open", "found": "deferred"},
        ])
    );
    assert_eq!(row(&root, &ledger, "R3")["status"], "deferred");

    let before = fs::read(&path).unwrap();
    let out = tomlctl::ledger_transition(
        &root,
        &ledger,
        &strings(&["R3"]),
        "wontfix",
        fields(),
        "open",
    )
    .expect("an all-stale call succeeds");
    assert_eq!(out["applied"], json!([]));
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "an all-stale call writes nothing"
    );
}

#[test]
fn ledger_transition_refuses_fixed() {
    let (_dir, root) = sandbox();
    let (ledger, path) = flow_ledger(&root, tomlctl::LedgerKind::Review, REVIEW_WRITE);
    let _env = pin_root(&root);
    let before = fs::read(&path).unwrap();
    let ids = strings(&["R1"]);

    let err = tomlctl::ledger_transition(
        &root,
        &ledger,
        &ids,
        "fixed",
        obj(json!({"resolution": "done"})),
        "open",
    )
    .expect_err("fixed belongs to the apply flows");
    assert!(
        format!("{err:#}").contains("open → fixed transition is not offered"),
        "unexpected error: {err:#}"
    );
    let err = tomlctl::ledger_transition(
        &root,
        &ledger,
        &ids,
        "wontapply",
        obj(json!({"wontapply_rationale": "x"})),
        "open",
    )
    .expect_err("wontapply is an optimise status");
    assert!(format!("{err:#}").contains("not offered on a review ledger"));
    let err = tomlctl::ledger_transition(
        &root,
        &ledger,
        &ids,
        "wontfix",
        obj(json!({"wontfix_rationale": "x", "summary": "rewritten"})),
        "open",
    )
    .expect_err("content is not a form field");
    assert!(format!("{err:#}").contains("`summary` is not a field"));
    let err = tomlctl::ledger_transition(
        &root,
        &tomlctl::LedgerRef::File(path.clone()),
        &ids,
        "wontfix",
        obj(json!({"wontfix_rationale": "x"})),
        "open",
    )
    .expect_err("a path-named ledger is read-only");
    assert!(format!("{err:#}").contains("read-only"));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn ledger_restore_undoes_a_transition() {
    let (_dir, root) = sandbox();
    let (ledger, path) = flow_ledger(&root, tomlctl::LedgerKind::Review, REVIEW_WRITE);
    let _env = pin_root(&root);
    let original = row(&root, &ledger, "R3");

    tomlctl::ledger_transition(
        &root,
        &ledger,
        &strings(&["R3"]),
        "open",
        obj(json!({"reopen_rationale": "trigger fired"})),
        "deferred",
    )
    .expect("reopen");
    let reopened = row(&root, &ledger, "R3");
    assert_eq!(reopened["status"], "open");
    assert_eq!(reopened["defer_reason"], "waits on the port");
    assert_eq!(reopened.get("defer_trigger"), None);

    let undo = || {
        tomlctl::ledger_restore(
            &root,
            &ledger,
            "R3",
            obj(json!({"status": "deferred", "defer_trigger": "when the port lands"})),
            strings(&["reopen_rationale"]),
            obj(json!({"status": "open", "reopen_rationale": "trigger fired", "defer_trigger": null})),
        )
        .expect("restore")
    };
    assert_eq!(undo(), json!({"applied": ["R3"], "skipped_stale": []}));
    assert_eq!(row(&root, &ledger, "R3"), original);
    assert_eq!(undo()["applied"], json!([]), "a second undo is stale");
    assert_eq!(last_updated(&path), "2026-09-30");

    let err = tomlctl::ledger_restore(
        &root,
        &ledger,
        "R3",
        obj(json!({"status": "fixed"})),
        Vec::new(),
        obj(json!({"status": "deferred"})),
    )
    .expect_err("restore never sets an apply-flow status");
    assert!(format!("{err:#}").contains("cannot set status"));
}

#[test]
fn ledger_writes_refuse_a_mismatched_root() {
    let (_pinned_dir, pinned) = sandbox();
    let (_dir, root) = sandbox();
    let (ledger, path) = flow_ledger(&root, tomlctl::LedgerKind::Review, REVIEW_WRITE);
    let before = fs::read(&path).unwrap();
    let _env = pin_root(&pinned);
    let ids = strings(&["R1"]);

    let errors = [
        tomlctl::ledger_transition(
            &root,
            &ledger,
            &ids,
            "wontfix",
            obj(json!({"wontfix_rationale": "x"})),
            "open",
        ),
        tomlctl::ledger_classify(
            &root,
            &ledger,
            &ids,
            obj(json!({"severity": "critical"})),
            obj(json!({"severity": "warning"})),
        ),
        tomlctl::ledger_restore(
            &root,
            &ledger,
            "R1",
            obj(json!({"severity": "critical"})),
            Vec::new(),
            obj(json!({"severity": "warning"})),
        ),
    ];
    for result in errors {
        let err = result.expect_err("the roots differ");
        assert!(
            format!("{err:#}").contains("root mismatch"),
            "unexpected error: {err:#}"
        );
    }
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn ledger_classify_recomputes_dedup_id() {
    let (_cli_dir, cli_root) = sandbox();
    let (_, cli_path) = flow_ledger(&cli_root, tomlctl::LedgerKind::Review, REVIEW_WRITE);
    cli(&cli_root)
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .args(["items", "update"])
        .arg(&cli_path)
        .args([
            "R1",
            "--json",
            r#"{"severity":"critical","category":"bug"}"#,
        ])
        .write_stdin("")
        .assert()
        .success();
    let cli_doc: toml::Value = toml::from_str(&fs::read_to_string(&cli_path).unwrap()).unwrap();
    let by_cli = cli_doc["items"][0]["dedup_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (_dir, root) = sandbox();
    let (ledger, _) = flow_ledger(&root, tomlctl::LedgerKind::Review, REVIEW_WRITE);
    let _env = pin_root(&root);
    let fields = || obj(json!({"severity": "critical", "category": "bug"}));
    let stale = tomlctl::ledger_classify(
        &root,
        &ledger,
        &strings(&["R1"]),
        fields(),
        obj(json!({"severity": "suggestion"})),
    )
    .expect("a stale classify succeeds");
    assert_eq!(stale["applied"], json!([]));
    assert_eq!(stale["skipped_stale"][0]["found"], "warning");

    let out = tomlctl::ledger_classify(
        &root,
        &ledger,
        &strings(&["R1"]),
        fields(),
        obj(json!({"severity": "warning", "category": "quality"})),
    )
    .expect("classify");
    assert_eq!(out["applied"], json!(["R1"]));
    let r = row(&root, &ledger, "R1");
    assert_eq!(r["severity"], "critical");
    assert_eq!(r["category"], "bug");
    assert_ne!(r["dedup_id"], "0000000000000000");
    assert_eq!(r["dedup_id"], by_cli.as_str());

    let err = tomlctl::ledger_classify(
        &root,
        &ledger,
        &strings(&["R1"]),
        obj(json!({"summary": "rewritten"})),
        obj(json!({"severity": "critical"})),
    )
    .expect_err("classify writes only the three control fields");
    assert!(format!("{err:#}").contains("not `summary`"));
}

#[test]
fn ledger_classify_refuses_plan_review_and_backlog() {
    let (_dir, root) = sandbox();
    let (plan_review, _) = flow_ledger(&root, tomlctl::LedgerKind::PlanReview, PLAN_REVIEW_WRITE);
    seed_ledger_in(&root, "backlog.toml", BACKLOG);
    let _env = pin_root(&root);
    for (ledger, id) in [(plan_review, "P1"), (tomlctl::LedgerRef::Backlog, "B-0001")] {
        let err = tomlctl::ledger_classify(
            &root,
            &ledger,
            &strings(&[id]),
            obj(json!({"severity": "critical"})),
            obj(json!({"severity": "warning"})),
        )
        .expect_err("only review and optimise are classified");
        let message = format!("{err:#}");
        assert!(
            message.contains("plan-review") || message.contains("backlog"),
            "unexpected error: {message}"
        );
    }
}

#[test]
fn ledger_classify_refuses_off_vocabulary_severity_and_effort() {
    let (_dir, root) = sandbox();
    let (ledger, path) = flow_ledger(&root, tomlctl::LedgerKind::Review, REVIEW_WRITE);
    let _env = pin_root(&root);
    let before = fs::read(&path).unwrap();
    for (fields, expected) in [
        (
            json!({"severity": "major"}),
            "`severity` must be one of critical, warning, suggestion",
        ),
        (
            json!({"effort": "large"}),
            "`effort` must be one of trivial, small, medium",
        ),
    ] {
        let err = tomlctl::ledger_classify(
            &root,
            &ledger,
            &strings(&["R1"]),
            obj(fields),
            obj(json!({"severity": "warning"})),
        )
        .expect_err("an off-vocabulary value is refused");
        let message = format!("{err:#}");
        assert!(message.contains(expected), "unexpected error: {message}");
    }
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn ledger_transition_discards_a_plan_review_finding_with_its_reason() {
    let (_dir, root) = sandbox();
    let (ledger, path) = flow_ledger(&root, tomlctl::LedgerKind::PlanReview, PLAN_REVIEW_WRITE);
    let _env = pin_root(&root);

    let out = tomlctl::ledger_transition(
        &root,
        &ledger,
        &strings(&["P1"]),
        "discarded",
        obj(json!({"discard_reason": "task 4 was dropped"})),
        "open",
    )
    .expect("discard");
    assert_eq!(out["applied"], json!(["P1"]));
    let r = row(&root, &ledger, "P1");
    assert_eq!(r["status"], "discarded");
    assert_eq!(r["discard_reason"], "task 4 was dropped");
    assert_eq!(last_updated(&path), "2026-09-30");
}

#[test]
fn ledger_transition_wontapply_requires_wontapply_rationale() {
    let (_dir, root) = sandbox();
    let (ledger, path) = flow_ledger(&root, tomlctl::LedgerKind::Optimise, OPTIMISE_WRITE);
    let _env = pin_root(&root);
    let before = fs::read(&path).unwrap();
    let ids = strings(&["O1"]);

    let err = tomlctl::ledger_transition(&root, &ledger, &ids, "wontapply", Map::new(), "open")
        .expect_err("wontapply needs its rationale");
    assert!(
        format!("{err:#}").contains("missing required field `wontapply_rationale`"),
        "unexpected error: {err:#}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);

    let out = tomlctl::ledger_transition(
        &root,
        &ledger,
        &ids,
        "wontapply",
        obj(json!({"wontapply_rationale": "the clone is cheap"})),
        "open",
    )
    .expect("wontapply with its rationale");
    assert_eq!(out["applied"], json!(["O1"]));
    assert_eq!(
        row(&root, &ledger, "O1")["wontapply_rationale"],
        "the clone is cheap"
    );
}

const BACKLOG_WRITE: &str = r#"schema_version = 1
last_updated = 2026-09-30

[[backlog]]
id = "B-aaaaaaaa"
kind = "bug"
summary = "first"
area = "src/a.rs"
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "aaaaaaaaaaaaaaaa"

[[backlog]]
id = "B-bbbbbbbb"
kind = "debt"
summary = "second"
area = ""
status = "dismissed"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "bbbbbbbbbbbbbbbb"
dismissed = 2026-09-02
dismiss_reason = "not ours"
"#;

#[test]
fn backlog_triage_dismisses_with_a_reason() {
    let (_dir, root) = sandbox();
    let path = seed_ledger_in(&root, "backlog.toml", BACKLOG_WRITE);
    let _env = pin_root(&root);

    let out = tomlctl::backlog_triage(
        &root,
        &strings(&["B-aaaaaaaa"]),
        tomlctl::BacklogTriage::Dismiss {
            reason: "stale capture".into(),
        },
        "open",
    )
    .expect("dismiss an open capture");
    assert_eq!(out, json!({"applied": ["B-aaaaaaaa"], "skipped_stale": []}));
    let r = row(&root, &tomlctl::LedgerRef::Backlog, "B-aaaaaaaa");
    assert_eq!(r["status"], "dismissed");
    assert_eq!(r["dismiss_reason"], "stale capture");
    assert!(r["dismissed"].is_string(), "the dismissal is dated: {r}");
    assert_sidecar_matches(&path);
}

#[test]
fn backlog_triage_skips_a_changed_status() {
    let (_dir, root) = sandbox();
    let path = seed_ledger_in(&root, "backlog.toml", BACKLOG_WRITE);
    let _env = pin_root(&root);
    let resolve = || tomlctl::BacklogTriage::Resolve {
        resolution: "fixed".into(),
    };

    let out = tomlctl::backlog_triage(
        &root,
        &strings(&["B-aaaaaaaa", "B-bbbbbbbb", "B-zzzzzzzz"]),
        resolve(),
        "open",
    )
    .expect("the fresh row lands");
    assert_eq!(out["applied"], json!(["B-aaaaaaaa"]));
    assert_eq!(
        out["skipped_stale"],
        json!([
            {"id": "B-bbbbbbbb", "field": "status", "expected": "open", "found": "dismissed"},
            {"id": "B-zzzzzzzz", "field": "id", "expected": "B-zzzzzzzz", "found": null},
        ])
    );
    let backlog = tomlctl::LedgerRef::Backlog;
    assert_eq!(row(&root, &backlog, "B-aaaaaaaa")["status"], "resolved");
    assert_eq!(row(&root, &backlog, "B-bbbbbbbb")["status"], "dismissed");

    let before = fs::read(&path).unwrap();
    let out = tomlctl::backlog_triage(&root, &strings(&["B-bbbbbbbb"]), resolve(), "open")
        .expect("an all-stale call succeeds");
    assert_eq!(out["applied"], json!([]));
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "an all-stale call writes nothing"
    );
}

fn inputs_path(root: &Path) -> PathBuf {
    root.join(".claude").join("inputs.toml")
}

/// The records of an `inputs list` envelope without the stamps that differ
/// between two runs of the same writes.
fn undated(read: &Value) -> Vec<Value> {
    read["inputs"]
        .as_array()
        .expect("inputs is an array")
        .iter()
        .map(|row| {
            let mut row = row.clone();
            let fields = row.as_object_mut().expect("a record is an object");
            for stamp in ["created", "acknowledged", "handled"] {
                fields.remove(stamp);
            }
            row
        })
        .collect()
}

fn question_record() -> Value {
    json!({
        "kind": "question",
        "author": "review",
        "ledger": "review",
        "flow": "write-flow",
        "items": ["R3"],
        "prompt": "Defer R3?",
        "choice": "multi",
        "options": ["yes", "later"],
    })
}

#[test]
fn inputs_read_matches_the_cli() {
    let (_dir, root) = sandbox();
    let missing = tomlctl::inputs_read(&root).expect("a missing store reads");
    assert_eq!(missing, cli_json(&root, &["inputs", "list"]));
    assert_eq!(missing["revision"], Value::Null);

    let store = r#"schema_version = 1
last_updated = 2026-10-02

[[inputs]]
id = "I1"
kind = "note"
author = "user"
status = "new"
created = 2026-10-02T08:00:00Z
ledger = "backlog"
text = "look at B-0001"
"#;
    seed_ledger_in(&root, "inputs.toml", store);
    let read = tomlctl::inputs_read(&root).expect("read the store");
    assert_eq!(read, cli_json(&root, &["inputs", "list"]));
    assert_eq!(read["revision"], sha256_hex(store.as_bytes()));
    assert_eq!(read["inputs"][0]["created"], "2026-10-02T08:00:00Z");
}

#[test]
fn inputs_add_matches_the_cli() {
    let record = json!({
        "kind": "capture",
        "ledger": "backlog",
        "text": "the watcher leaks a handle",
        "capture_kind": "bug",
        "area": "glimpse/src/watch.rs",
    });
    let (_cli_dir, cli_root) = sandbox();
    let by_cli = cli_json(&cli_root, &["inputs", "add", "--json", &record.to_string()]);

    let (_dir, root) = sandbox();
    let _env = pin_root(&root);
    let by_facade = tomlctl::inputs_add(&root, &record).expect("add a capture");
    assert_eq!(by_facade, by_cli);
    assert_eq!(by_facade, json!({"id": "I1"}));

    let read = tomlctl::inputs_read(&root).expect("read back");
    assert_eq!(read, cli_json(&root, &["inputs", "list"]));
    assert_eq!(
        undated(&read),
        undated(&cli_json(&cli_root, &["inputs", "list"]))
    );
    assert_eq!(read["inputs"][0]["status"], "new");
    assert_eq!(read["inputs"][0]["author"], "user");
    assert_sidecar_matches(&inputs_path(&root));
}

#[test]
fn inputs_answer_and_withdraw_match_the_cli() {
    let (_cli_dir, cli_root) = sandbox();
    let asked = question_record().to_string();
    cli_json(&cli_root, &["inputs", "add", "--json", &asked]);
    let answered_by_cli = cli_json(
        &cli_root,
        &[
            "inputs", "answer", "I1", "--pick", "yes", "--pick", "later", "--text", "both",
        ],
    );
    let withdrawn_by_cli = cli_json(&cli_root, &["inputs", "withdraw", "I2"]);

    let (_dir, root) = sandbox();
    let _env = pin_root(&root);
    tomlctl::inputs_add(&root, &question_record()).expect("post a question");
    let answered = tomlctl::inputs_answer(&root, "I1", &strings(&["yes", "later"]), Some("both"))
        .expect("answer it");
    assert_eq!(answered, answered_by_cli);
    assert_eq!(answered, json!({"id": "I2", "question": "I1"}));
    let read = tomlctl::inputs_read(&root).expect("read the answer");
    assert_eq!(read["inputs"][0]["status"], "handled");
    assert_eq!(read["inputs"][1]["picked"], json!(["yes", "later"]));

    let withdrawn = tomlctl::inputs_withdraw(&root, &strings(&["I2"])).expect("withdraw it");
    assert_eq!(withdrawn, withdrawn_by_cli);
    assert_eq!(withdrawn, json!({"applied": ["I2"], "reopened": ["I1"]}));
    assert_eq!(
        undated(&tomlctl::inputs_read(&root).expect("read after withdraw")),
        undated(&cli_json(&cli_root, &["inputs", "list"]))
    );
    assert_sidecar_matches(&inputs_path(&root));
}

#[test]
fn inputs_writes_refuse_a_mismatched_root() {
    let (_pinned_dir, pinned) = sandbox();
    let (_dir, root) = sandbox();
    let _env = pin_root(&pinned);

    let errors = [
        tomlctl::inputs_add(&root, &question_record()),
        tomlctl::inputs_answer(&root, "I1", &strings(&["yes"]), None),
        tomlctl::inputs_withdraw(&root, &strings(&["I1"])),
    ];
    for result in errors {
        let err = result.expect_err("the roots differ");
        assert!(
            format!("{err:#}").contains("root mismatch"),
            "unexpected error: {err:#}"
        );
    }
    assert!(
        !inputs_path(&root).exists(),
        "a refused write creates nothing"
    );
    assert!(!inputs_path(&pinned).exists());
}
