//! Black-box coverage for the `tomlctl tasks` graph verbs — `ready`,
//! `batches`, `closure` and `check`.
//!
//! All four are pure reads over one staged store, so a single fixture carries
//! every shape they are asked about: a chain, a fork whose two arms claim one
//! file, two checkpoint groups, and a row the last import no longer names. The
//! second fixture is that graph with one edge reversed, which is what the
//! refusal paths are asserted against.
//!
//! Exit codes are asserted alongside the JSON: `check` is the one verb whose
//! findings are carried by the exit status, and a carrier gates on that status
//! before it reads a byte of the envelope.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{TASKS_SLUG, cli, parse_json_error_envelope, sandbox, seed_tasks};

/// 1 → {2, 3} → 4 → 5. Tasks 2 and 3 both claim `graph.rs` with no path
/// between them (one `dag/unreachable-claim`), task 2 is `in-progress` with 4
/// and 5 stranded behind it (one `dag/stalled-dependency`), and task 5's `ref`
/// is absent from `last_import_refs` while still `pending` (one
/// `plan/orphan-row`). Every row carries a checkpoint, so
/// `checkpoint/orphan-task` stays quiet and the warning count is exactly three.
const GRAPH_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/whimsical-hugging-puppy.md"
last_import_refs = [
    "scaffold-the-module-tree",
    "wire-the-graph-engine",
    "write-the-importer",
    "wire-the-renderer",
]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "the store and its engine"

[[checkpoints]]
id = "B"
rationale = "the renderer and its docs"

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
commit = ""

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

[[items]]
id = 3
ref = "write-the-importer"
title = "Write the importer"
effort = "M"
status = "pending"
checkpoint = "A"
files = ["tomlctl/src/tasks/graph.rs", "tomlctl/src/tasks/import_plan.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 4
ref = "wire-the-renderer"
title = "Wire the renderer"
effort = "L"
status = "pending"
checkpoint = "B"
files = ["tomlctl/src/tasks/render.rs"]
needs = [2, 3]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 5
ref = "publish-the-docs"
title = "Publish the docs"
effort = "S"
status = "pending"
checkpoint = "B"
files = ["tomlctl/README.md"]
needs = [4]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

/// A three-row loop that is otherwise clean: every ref is imported, every row
/// has a checkpoint and `max_parallel` is in range, so `dag/cycle` is the only
/// finding `check` can raise here.
const CYCLIC_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/whimsical-hugging-puppy.md"
last_import_refs = ["first", "second", "third"]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "one group"

[[items]]
id = 1
ref = "first"
title = "First"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["a.rs"]
needs = [3]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 2
ref = "second"
title = "Second"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["b.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 3
ref = "third"
title = "Third"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["c.rs"]
needs = [2]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

/// The engine's node cap, mirrored — it is private to the graph module, so
/// widening it there has to move this line with it.
const NODE_CAP: u32 = 256;

/// `count` independent rows: every `ref` imported, every row inside the one
/// declared group, and a file per row so no pair can overlap. Nothing but the
/// row count varies, which is what makes the pair of stores below a boundary
/// rather than two unrelated samples.
fn wide_fixture(count: u32) -> String {
    let mut store = String::from(
        "schema_version = 1\n\
         last_updated = 2026-09-07\n\
         plan_path = \"docs/plans/whimsical-hugging-puppy.md\"\n\
         last_import_refs = [\n",
    );
    for id in 1..=count {
        store.push_str(&format!("    \"task-{id}\",\n"));
    }
    store.push_str(
        "]\n\n\
         [policy]\n\
         checkpoints = \"milestones\"\n\
         max_parallel = 6\n\
         commit_granularity = \"per-task\"\n\
         note = \"\"\n\n\
         [[checkpoints]]\n\
         id = \"A\"\n\
         rationale = \"one group\"\n",
    );
    for id in 1..=count {
        store.push_str(&format!(
            "\n[[items]]\n\
             id = {id}\n\
             ref = \"task-{id}\"\n\
             title = \"Task {id}\"\n\
             effort = \"S\"\n\
             status = \"pending\"\n\
             checkpoint = \"A\"\n\
             files = [\"src/file-{id}.rs\"]\n\
             needs = []\n\
             coupling = []\n\
             deps_note = \"\"\n\
             action = \"\"\n\
             detail = \"\"\n\
             acceptance = \"\"\n\
             agent = \"\"\n\
             commit = \"\"\n"
        ));
    }
    store
}

/// Run `tomlctl tasks <args…> --slug <TASKS_SLUG>`, require exit 0, and parse
/// stdout as JSON.
fn tasks(root: &Path, args: &[&str]) -> Value {
    let out = cli(root)
        .arg("tasks")
        .args(args)
        .args(["--slug", TASKS_SLUG])
        .write_stdin("")
        .assert()
        .success();
    parse_stdout(&out.get_output().stdout, args)
}

/// `check`'s findings ride on the exit status, so its stdout has to be read on
/// the failing path too — `.success()` would discard exactly the case the exit
/// code exists for.
fn check_run(root: &Path, expected_code: i32, extra: &[&str]) -> Value {
    let assertion = cli(root)
        .args(["tasks", "check", "--slug", TASKS_SLUG])
        .args(extra)
        .write_stdin("")
        .assert()
        .code(expected_code);
    parse_stdout(&assertion.get_output().stdout, &["check"])
}

fn parse_stdout(stdout: &[u8], args: &[&str]) -> Value {
    let text = String::from_utf8_lossy(stdout).to_string();
    serde_json::from_str(text.trim()).unwrap_or_else(|e| {
        panic!(
            "`tasks {}` stdout must be JSON: {e}; got: {text}",
            args.join(" ")
        )
    })
}

/// Run `tomlctl --error-format json tasks <args…>`, require exit 1, and hand
/// back the parsed `error` object.
fn tasks_err(root: &Path, args: &[&str]) -> Value {
    let out = cli(root)
        .args(["--error-format", "json", "tasks"])
        .args(args)
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    parse_json_error_envelope(&stderr)
}

fn classes(findings: &Value) -> Vec<&str> {
    findings
        .as_array()
        .expect("findings must be an array")
        .iter()
        .map(|finding| finding["class"].as_str().expect("a class string"))
        .collect()
}

// ---------------------------------------------------------------------------
// ready / batches / closure
// ---------------------------------------------------------------------------

/// With 2 in flight, 3 is unblocked by dependency but claims 2's file, so it
/// reports as held rather than ready — and 4 is the wave behind both.
#[test]
fn ready_separates_the_dispatchable_frontier_from_a_held_file_claim() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, GRAPH_FIXTURE);

    assert_eq!(
        tasks(&root, &["ready", "--in-flight", "2"]),
        json!({
            "ready": [],
            "held": [{
                "id": 3,
                "blocked_on_file": "tomlctl/src/tasks/graph.rs",
                "holder": 2,
            }],
            "next": [4],
            "blocked": [],
        })
    );

    // Nothing in flight: no claim can hold 3, and 4's in-progress predecessor
    // is undeclared, so no wave this answer describes reaches it — it lands in
    // `blocked`, naming the row and the status that stall it. 5 is stranded by
    // the same row one hop further back, and `blocked` is reached through the
    // intervening pending 4, so it names 2 — the row a human has to move —
    // rather than the waiting row nobody can act on.
    assert_eq!(
        tasks(&root, &["ready"]),
        json!({
            "ready": [3],
            "held": [],
            "next": [],
            "blocked": [{"id": 4, "blocker": 2, "blocker_status": "in-progress"},
                        {"id": 5, "blocker": 2, "blocker_status": "in-progress"}],
        })
    );
}

/// Kahn layers over the whole graph, each ascending.
#[test]
fn batches_layers_the_graph_in_dependency_order() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, GRAPH_FIXTURE);

    assert_eq!(
        tasks(&root, &["batches"]),
        json!({"batches": [[1], [2, 3], [4], [5]]})
    );
}

/// A group reports its members, its antichain (the ids a `CHECKPOINT … after`
/// marker names), the upward walk that marker prints as its `dependency
/// closure`, and whether the prefix up to it is downward-closed. Group B is
/// the pair that keeps `members` and `dependency_closure` apart: the marker
/// names five ids where the group holds two.
#[test]
fn closure_reports_group_membership_and_walks_one_task_both_ways() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, GRAPH_FIXTURE);

    assert_eq!(
        tasks(&root, &["closure", "--checkpoint", "A"]),
        json!({
            "checkpoint": "A",
            "members": [1, 2, 3],
            "maximal": [2, 3],
            "dependency_closure": [1, 2, 3],
            "valid_cut": true,
        })
    );
    assert_eq!(
        tasks(&root, &["closure", "--checkpoint", "B"]),
        json!({
            "checkpoint": "B",
            "members": [4, 5],
            "maximal": [5],
            "dependency_closure": [1, 2, 3, 4, 5],
            "valid_cut": true,
        })
    );

    assert_eq!(
        tasks(&root, &["closure", "--task", "4", "--up"]),
        json!({"task": 4, "direction": "up", "ids": [1, 2, 3, 4]})
    );
    assert_eq!(
        tasks(&root, &["closure", "--task", "2", "--down"]),
        json!({"task": 2, "direction": "down", "ids": [2, 4, 5]})
    );
}

/// A direction alongside `--checkpoint` is refused rather than dropped: a
/// silently-ignored `--up` would answer a question nobody asked.
#[test]
fn closure_refuses_a_checkpoint_paired_with_a_direction() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, GRAPH_FIXTURE);

    let err = tasks_err(
        &root,
        &["closure", "--slug", TASKS_SLUG, "--checkpoint", "A", "--up"],
    );
    assert_eq!(err["kind"], json!("validation"));
    assert!(
        err["message"].as_str().unwrap().contains("--up/--down"),
        "{}",
        err["message"]
    );

    // No mode at all, and a task with no direction, are the two shapes clap's
    // pairwise `conflicts_with` cannot express either.
    for args in [
        vec!["closure", "--slug", TASKS_SLUG],
        vec!["closure", "--slug", TASKS_SLUG, "--task", "4"],
    ] {
        assert_eq!(tasks_err(&root, &args)["kind"], json!("validation"));
    }
}

// ---------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------

/// Three warnings and exit 0: a warning is a report, not a gate. The orphan row
/// only fires because `last_import_refs` is populated — an unimported store
/// would otherwise flag every row it holds. The stall names the row a human has
/// to move, so its `ids` carry the blocker rather than the rows behind it.
#[test]
fn check_reports_the_stall_the_overlap_pair_and_the_orphan_row_as_warnings_at_exit_zero() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, GRAPH_FIXTURE);

    let envelope = check_run(&root, 0, &[]);
    assert_eq!(envelope["ok"], json!(true), "{envelope}");

    let findings = &envelope["findings"];
    assert_eq!(
        classes(findings),
        vec![
            "dag/stalled-dependency",
            "dag/unreachable-claim",
            "plan/orphan-row"
        ],
        "{envelope}"
    );
    for finding in findings.as_array().unwrap() {
        assert_eq!(finding["severity"], json!("warning"), "{envelope}");
    }
    assert_eq!(findings[0]["ids"], json!([2]));
    assert_eq!(findings[1]["ids"], json!([2, 3]));
    assert!(
        findings[1]["detail"]
            .as_str()
            .unwrap()
            .contains("tomlctl/src/tasks/graph.rs"),
        "{envelope}"
    );
    assert_eq!(findings[2]["ids"], json!([5]));
}

/// The stall above is the ordinary mid-run shape of a healthy dispatch, so the
/// caller that knows which rows it sent out declares them and reads the rest of
/// the envelope unchanged. `ready` takes the same set under the same spelling.
#[test]
fn a_row_declared_in_flight_is_not_reported_as_a_stalled_dependency() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, GRAPH_FIXTURE);

    let declared = check_run(&root, 0, &["--in-flight", "2"]);
    assert_eq!(
        classes(&declared["findings"]),
        vec!["dag/unreachable-claim", "plan/orphan-row"],
        "{declared}"
    );

    // An id no row carries is dropped rather than refused — a typo emptying
    // the class would read as a clean bill.
    let typo = check_run(&root, 0, &["--in-flight", "99"]);
    assert_eq!(
        classes(&typo["findings"]),
        vec![
            "dag/stalled-dependency",
            "dag/unreachable-claim",
            "plan/orphan-row"
        ],
        "{typo}"
    );
}

/// One error-class finding and exit 1. The graph verbs refuse the same store
/// outright: Kahn strands a cycle's members, and a stranded task reads in a
/// frontier exactly like one that is merely waiting.
#[test]
fn a_cyclic_store_fails_check_and_refuses_the_graph_verbs() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, CYCLIC_FIXTURE);

    let envelope = check_run(&root, 1, &[]);
    assert_eq!(envelope["ok"], json!(false), "{envelope}");
    assert_eq!(
        classes(&envelope["findings"]),
        vec!["dag/cycle"],
        "{envelope}"
    );
    assert_eq!(envelope["findings"][0]["severity"], json!("error"));
    assert_eq!(envelope["findings"][0]["ids"], json!([1, 2, 3]));

    for verb in ["ready", "batches"] {
        let err = tasks_err(&root, &[verb, "--slug", TASKS_SLUG]);
        assert_eq!(err["kind"], json!("validation"), "{verb}");
        assert!(
            err["message"]
                .as_str()
                .unwrap()
                .contains("cycle through tasks 1, 2, 3"),
            "{verb}: {}",
            err["message"]
        );
    }
}

/// A store the engine will not build a graph from is an error-class finding
/// and a non-zero exit, not a clean bill. `check` runs its row scans without
/// the graph, so nothing else in the envelope would have named the refusal —
/// and a carrier gates on the exit status, so silence here reads as a store
/// safe to import and dispatch from.
#[test]
fn a_store_past_the_node_cap_fails_check_with_an_error_class_finding() {
    let (_tmp, root) = sandbox();

    // At the cap the identical shape reports clean, so the finding below is
    // the refusal and not the row count, the file-per-row claim or the group.
    seed_tasks(&root, &wide_fixture(NODE_CAP));
    let under = check_run(&root, 0, &[]);
    assert_eq!(under["ok"], json!(true), "{under}");
    assert!(classes(&under["findings"]).is_empty(), "{under}");

    seed_tasks(&root, &wide_fixture(NODE_CAP + 1));
    let over = check_run(&root, 1, &[]);
    assert_eq!(over["ok"], json!(false), "{over}");
    assert_eq!(
        classes(&over["findings"]),
        vec!["dag/unbuildable"],
        "{over}"
    );
    assert_eq!(over["findings"][0]["severity"], json!("error"), "{over}");

    // The graph verbs refuse the same store outright, which is what the
    // finding exists to announce.
    for verb in ["ready", "batches"] {
        assert_eq!(
            tasks_err(&root, &[verb, "--slug", TASKS_SLUG])["kind"],
            json!("validation"),
            "{verb}"
        );
    }
}

// ---------------------------------------------------------------------------
// integrity
// ---------------------------------------------------------------------------

/// Every read verb threads `--verify-integrity` through its own load, so a
/// passing verified read cannot tell a plumbed verb from one that drops the
/// flag on the floor. Only a sidecar that no longer covers the store separates
/// them — and the contract is that such a read errors, never repairs.
#[test]
fn every_read_verb_refuses_a_tampered_sidecar_under_verify_integrity() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, GRAPH_FIXTURE);
    let sidecar = sidecar_of(&store);

    // One flipped hex digit: the store's own bytes stay valid TOML, so nothing
    // but the digest comparison can be what refuses the reads below.
    let mut tampered = fs::read(&sidecar).expect("the seed wrote a sidecar");
    tampered[0] = if tampered[0] == b'0' { b'1' } else { b'0' };
    fs::write(&sidecar, &tampered).expect("the sidecar is rewritten");

    let cases: [&[&str]; 7] = [
        &["show", "1"],
        &["list"],
        &["edges"],
        &["ready"],
        &["batches"],
        &["closure", "--checkpoint", "A"],
        &["check"],
    ];

    for args in cases {
        // Control: unflagged, the same call answers normally over the same
        // tampered sidecar. Without it a verb that failed for its own reasons
        // would read as a verb honouring the flag.
        tasks(&root, args);

        let mut verified: Vec<&str> = args.to_vec();
        verified.extend(["--slug", TASKS_SLUG, "--verify-integrity"]);
        assert_eq!(
            tasks_err(&root, &verified)["kind"],
            json!("integrity"),
            "{args:?}"
        );

        assert_eq!(
            fs::read(&sidecar).expect("the sidecar survives"),
            tampered,
            "{args:?}: a verifying read must not repair the digest it rejected"
        );
    }
}

fn sidecar_of(file: &Path) -> PathBuf {
    let mut raw = file.as_os_str().to_os_string();
    raw.push(".sha256");
    PathBuf::from(raw)
}

// ---------------------------------------------------------------------------
// target resolution
// ---------------------------------------------------------------------------

/// Every read verb refuses an empty `--slug | --file` target as a
/// `kind=validation` envelope, not as clap usage prose on exit 2.
#[test]
fn every_graph_verb_refuses_an_empty_target_as_validation() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, GRAPH_FIXTURE);

    let cases: [&[&str]; 4] = [
        &["ready"],
        &["batches"],
        &["closure", "--checkpoint", "A"],
        &["check"],
    ];

    for args in cases {
        let err = tasks_err(&root, args);
        assert_eq!(err["kind"], json!("validation"), "{args:?}");
        let message = err["message"].as_str().unwrap();
        assert!(message.contains("--slug"), "{args:?}: {message}");
        assert!(message.contains("--file"), "{args:?}: {message}");
    }
}
