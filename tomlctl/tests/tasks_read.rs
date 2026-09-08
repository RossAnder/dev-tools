//! Black-box coverage for the `tomlctl tasks` read verbs — `show`, `list`
//! and `edges`.
//!
//! These three are the verbs a carrier *parses*, so the assertions are on the
//! bytes and on the envelope's key order, not on the exit status alone. One
//! staged store carries every shape they are asked about: two settled rows and
//! three pending ones, a row whose `needs` and `coupling` targets differ, a
//! transitive successor, a file claimed by two rows with no path between them,
//! and two titles carrying the characters a DOT label has to escape.
//!
//! `list` gets the heaviest coverage because `/implement` derives its
//! idempotency skip-list from one of its queries: a wrong answer there
//! re-dispatches completed work or drops incomplete work, and neither is
//! visible in an exit code.

use serde_json::{Value, json};
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, parse_json_error_envelope, sandbox, seed_tasks};

/// 1 → 2, 1 → {3, 4}, 3 → 5, plus a `coupling` edge 2 → 3 so `deps` and
/// `dependents` cannot agree by accident. Tasks 3 and 4 both claim `shared.rs`
/// with no path between them, which is the one computed overlap. Exactly two
/// rows are `done`, so the skip-list query returns a plural answer — the
/// cardinality `--raw` refuses and `--lines` exists for.
const READ_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/whimsical-hugging-puppy.md"
last_import_refs = [
    "scaffold-the-module-tree",
    "wire-the-graph-engine",
    "write-the-read-verbs",
    "render-the-plan",
    "publish-the-docs",
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
status = "done"
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
ref = "write-the-read-verbs"
title = 'Write the "read" verbs'
effort = "M"
status = "pending"
checkpoint = "A"
files = ["tomlctl/src/tasks/read.rs", "tomlctl/src/tasks/shared.rs"]
needs = [1]
coupling = [2]
deps_note = "after the engine"
action = "Write show, list and edges."
detail = "Each is a pure read."
acceptance = "The read suite passes."
agent = ""
commit = ""

[[items]]
id = 4
ref = "render-the-plan"
title = 'Render the plan\graph'
effort = "L"
status = "pending"
checkpoint = "B"
files = ["tomlctl/src/tasks/shared.rs"]
needs = [1]
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
needs = [3]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

/// Append the staged store's slug unless the case names its own target.
/// `--slug` and `--file` are mutually exclusive, so a blanket append would
/// turn the one case that drives `--file` into clap usage prose.
fn targeted<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut out = args.to_vec();
    if !args.iter().any(|arg| *arg == "--slug" || *arg == "--file") {
        out.extend(["--slug", TASKS_SLUG]);
    }
    out
}

/// Run `tomlctl tasks <args…>`, require exit 0, and hand back stdout verbatim.
/// The raw bytes are the unit under test wherever a carrier reads them without
/// a JSON parse — `--dot`, `--raw` and the newline-delimited encodings.
fn tasks_stdout(root: &Path, args: &[&str]) -> String {
    let out = cli(root)
        .arg("tasks")
        .args(targeted(args))
        .write_stdin("")
        .assert()
        .success();
    String::from_utf8(out.get_output().stdout.clone()).expect("stdout must be UTF-8")
}

fn tasks(root: &Path, args: &[&str]) -> Value {
    let text = tasks_stdout(root, args);
    serde_json::from_str(text.trim()).unwrap_or_else(|e| {
        panic!(
            "`tasks {}` stdout must be JSON: {e}; got: {text}",
            args.join(" ")
        )
    })
}

/// Run `tomlctl --error-format json tasks <args…>`, require exit 1, and hand
/// back the parsed `error` object. A refusal is asserted through `kind`
/// because that tag is what a carrier branches on.
fn tasks_err(root: &Path, args: &[&str]) -> Value {
    untargeted_err(root, &targeted(args))
}

/// The same refusal path with `args` passed through verbatim, for the one case
/// whose subject is a call naming no store at all — [`targeted`] would
/// otherwise repair it by appending the staged slug.
fn untargeted_err(root: &Path, args: &[&str]) -> Value {
    let out = cli(root)
        .args(["--error-format", "json", "tasks"])
        .args(args)
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr))
}

/// Top-level keys in the order the emitter wrote them. The summary shape is an
/// ordered contract and a parsed map would not carry it: nested keys are
/// indented deeper than the two spaces this matches.
fn top_level_keys(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("  \""))
        .map(|line| {
            line.trim_start()
                .trim_start_matches('"')
                .split('"')
                .next()
                .expect("a quoted key")
        })
        .collect()
}

fn ids(rows: &Value) -> Vec<u64> {
    rows.as_array()
        .unwrap_or_else(|| panic!("expected an array, got {rows}"))
        .iter()
        .map(|row| row["id"].as_u64().expect("each row carries an id"))
        .collect()
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

/// No `--with` is the summary shape, whole and in order.
#[test]
fn show_without_a_projection_emits_the_summary_in_its_declared_key_order() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    let stdout = tasks_stdout(&root, &["show", "3"]);
    assert_eq!(
        top_level_keys(&stdout),
        vec![
            "id",
            "ref",
            "title",
            "effort",
            "status",
            "checkpoint",
            "files",
            "needs",
            "coupling"
        ],
        "{stdout}"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).expect("stdout must be JSON"),
        json!({
            "id": 3,
            "ref": "write-the-read-verbs",
            "title": "Write the \"read\" verbs",
            "effort": "M",
            "status": "pending",
            "checkpoint": "A",
            "files": ["tomlctl/src/tasks/read.rs", "tomlctl/src/tasks/shared.rs"],
            "needs": [1],
            "coupling": [2],
        }),
        "{stdout}"
    );
}

/// The fetch-by-id form a dispatching orchestrator hands an implementing agent
/// in place of pasted prose. `id` survives a projection that never asked for
/// the summary — without it the fetched row could not be matched back to the
/// id that was requested — and the rest of the summary stays out.
#[test]
fn the_fetch_by_id_projection_carries_the_body_the_files_and_a_summary_per_dep() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    let stdout = tasks_stdout(&root, &["show", "3", "--with", "body,files,deps"]);
    assert_eq!(
        top_level_keys(&stdout),
        vec!["id", "files", "action", "detail", "acceptance", "deps"],
        "{stdout}"
    );

    let out: Value = serde_json::from_str(&stdout).expect("stdout must be JSON");
    assert_eq!(out["id"], json!(3), "{stdout}");
    assert_eq!(
        out["action"],
        json!("Write show, list and edges."),
        "{stdout}"
    );
    assert_eq!(out["detail"], json!("Each is a pure read."), "{stdout}");
    assert_eq!(
        out["acceptance"],
        json!("The read suite passes."),
        "{stdout}"
    );
    assert_eq!(
        out["files"],
        json!(["tomlctl/src/tasks/read.rs", "tomlctl/src/tasks/shared.rs"]),
        "{stdout}"
    );

    // A dep is the whole summary, not a bare id: the orchestrator reads a
    // predecessor's status and files off this without a second fetch. Task 3's
    // two targets arrive through different fields, so a `deps` built from
    // `needs` alone would still answer with one of them.
    assert_eq!(ids(&out["deps"]), vec![1, 2], "{stdout}");
    assert_eq!(
        out["deps"][1]["ref"],
        json!("wire-the-graph-engine"),
        "{stdout}"
    );
    assert_eq!(out["deps"][1]["status"], json!("done"), "{stdout}");
    assert_eq!(
        out["deps"][0]["files"],
        json!(["tomlctl/src/tasks/mod.rs"]),
        "{stdout}"
    );
}

/// The two edge parts are not symmetric and are not each other's inverse:
/// `deps` is the row's own direct targets, `dependents` the transitive
/// successor set with the row itself excluded.
#[test]
fn dependents_walk_forward_transitively_where_deps_name_only_direct_targets() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    let from_one = tasks(&root, &["show", "1", "--with", "dependents"]);
    assert_eq!(ids(&from_one["dependents"]), vec![2, 3, 4, 5], "{from_one}");
    assert_eq!(from_one["id"], json!(1), "{from_one}");

    let deps_of_one = tasks(&root, &["show", "1", "--with", "deps"]);
    assert_eq!(deps_of_one["deps"], json!([]), "{deps_of_one}");

    let deps_of_five = tasks(&root, &["show", "5", "--with", "deps"]);
    assert_eq!(ids(&deps_of_five["deps"]), vec![3], "{deps_of_five}");
}

/// A fetch for a row the store does not hold is a `not_found` envelope naming
/// the id — an orchestrator distinguishes that from a store it could not read
/// at all by the tag, not by the prose.
#[test]
fn a_fetch_for_an_absent_row_is_a_not_found_envelope() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    let err = tasks_err(&root, &["show", "99"]);
    assert_eq!(err["kind"], json!("not_found"), "{err}");
    assert_eq!(err["message"], json!("no task 99 in the store"), "{err}");
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

/// The skip-list query, byte for byte. `/implement` re-dispatches every task
/// whose `ref` this does not name, so both membership and encoding are
/// load-bearing: a summary object per row, or an id in place of the `ref`,
/// would silently re-run completed work.
#[test]
fn the_skip_list_query_emits_a_flat_array_of_the_settled_refs() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    assert_eq!(
        tasks_stdout(&root, &["list", "--where", "status=done", "--pluck", "ref"]),
        "[\n  \"scaffold-the-module-tree\",\n  \"wire-the-graph-engine\"\n]\n"
    );
}

/// `--raw` and `--lines` are not spellings of one another, and confusing them
/// is the live failure mode: `--raw` is the single-value form and refuses any
/// other cardinality, `--lines` is the newline-delimited one and accepts every
/// cardinality including none. Both wordings are pinned because the refusal
/// message is what tells an agent which flag it wanted.
#[test]
fn raw_is_the_single_value_form_and_lines_the_newline_delimited_one() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    let plural: &[&str] = &["list", "--where", "status=done", "--pluck", "ref"];

    let mut raw = plural.to_vec();
    raw.push("--raw");
    assert_eq!(
        tasks_err(&root, &raw)["message"],
        json!(
            "--raw requires single-value output (got 2 items); use --lines for newline-delimited"
        )
    );

    let mut lines = plural.to_vec();
    lines.push("--lines");
    assert_eq!(
        tasks_stdout(&root, &lines),
        "\"scaffold-the-module-tree\"\n\"wire-the-graph-engine\"\n"
    );

    // Composed, they are the streaming bare-scalar form — one unquoted value
    // per line, which is what a shell `while read` loop consumes.
    let mut raw_lines = lines.clone();
    raw_lines.push("--raw");
    assert_eq!(
        tasks_stdout(&root, &raw_lines),
        "scaffold-the-module-tree\nwire-the-graph-engine\n"
    );

    // One match is the cardinality `--raw` exists for, and the value arrives
    // bare — a quoted `"5"` would reach a carrier's arithmetic as a string.
    assert_eq!(
        tasks_stdout(
            &root,
            &[
                "list",
                "--where",
                "ref=publish-the-docs",
                "--pluck",
                "id",
                "--raw"
            ]
        ),
        "5\n"
    );

    // An empty match is refused rather than emitted as nothing, and the
    // message names the flag that answers a possibly-empty pluck.
    let empty = tasks_err(
        &root,
        &[
            "list",
            "--where",
            "status=failed",
            "--pluck",
            "ref",
            "--raw",
        ],
    );
    assert_eq!(
        empty["message"],
        json!(
            "--raw requires single-value output (got 0 items); for a possibly-empty pluck use `--pluck <f> --lines` to stream zero-or-more JSON values one per line"
        )
    );
    assert_eq!(
        tasks_stdout(
            &root,
            &[
                "list",
                "--where",
                "status=failed",
                "--pluck",
                "ref",
                "--lines"
            ]
        ),
        ""
    );
}

/// `--raw` over the row array has no scalar to render and says so, rather than
/// stripping the quotes off a whole array and handing back something that
/// parses as neither JSON nor a value.
#[test]
fn raw_over_the_unprojected_row_array_is_refused() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    assert_eq!(
        tasks_err(&root, &["list", "--raw"])["message"],
        json!(
            "--raw requires a scalar target (string|number|bool); got array — use `--lines` (or omit --raw) to emit JSON"
        )
    );
}

/// The row total a flow's `context.toml` carries, and the per-status breakdown
/// beside it. `--count --raw` is the bare-integer form.
#[test]
fn the_count_shapes_answer_over_the_whole_store() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    assert_eq!(tasks(&root, &["list", "--count"]), json!({ "count": 5 }));
    assert_eq!(tasks_stdout(&root, &["list", "--count", "--raw"]), "5\n");
    assert_eq!(
        tasks(&root, &["list", "--count-by", "status"]),
        json!({ "done": 2, "pending": 3 })
    );
}

/// The streaming encoding emits one compact row per line with no enclosing
/// array — and selects the same rows, in the same order, as the batched array
/// it replaces. Without that parity an agent could not blanket-add the flag.
#[test]
fn the_streaming_encoding_matches_the_batched_array_row_for_row() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    let query: &[&str] = &[
        "list",
        "--where",
        "status=pending",
        "--select",
        "id,ref,files",
    ];

    let mut streamed = query.to_vec();
    streamed.push("--ndjson");
    assert_eq!(
        tasks_stdout(&root, &streamed),
        "{\"id\":3,\"ref\":\"write-the-read-verbs\",\"files\":[\"tomlctl/src/tasks/read.rs\",\"tomlctl/src/tasks/shared.rs\"]}\n\
         {\"id\":4,\"ref\":\"render-the-plan\",\"files\":[\"tomlctl/src/tasks/shared.rs\"]}\n\
         {\"id\":5,\"ref\":\"publish-the-docs\",\"files\":[\"tomlctl/README.md\"]}\n"
    );

    let batched = tasks(&root, query);
    let per_line: Vec<Value> = tasks_stdout(&root, &streamed)
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line must be JSON"))
        .collect();
    assert_eq!(batched, Value::Array(per_line));
}

/// `--where-in` over a ref set with a projection — the shape a plan review
/// uses to ask what became of the rows a rewritten plan stopped naming.
#[test]
fn a_ref_set_predicate_composes_with_a_projection() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    assert_eq!(
        tasks(
            &root,
            &[
                "list",
                "--where-in",
                "ref=render-the-plan,publish-the-docs",
                "--select",
                "id,ref,status"
            ]
        ),
        json!([
            { "id": 4, "ref": "render-the-plan", "status": "pending" },
            { "id": 5, "ref": "publish-the-docs", "status": "pending" },
        ])
    );
}

/// `--file` is the store target here, where the same flag on the generic
/// `items list` is a row predicate. A store reached this way answers over
/// every row it holds; read as a predicate it would match none, since no row
/// claims the store as one of its files.
#[test]
fn the_file_flag_names_the_store_rather_than_filtering_rows() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, READ_FIXTURE);
    let store = store.to_str().expect("the staged path is UTF-8");

    assert_eq!(
        tasks(&root, &["list", "--file", store, "--pluck", "id"]),
        json!([1, 2, 3, 4, 5])
    );
}

// ---------------------------------------------------------------------------
// edges
// ---------------------------------------------------------------------------

/// Both stored kinds then the computed one, each ascending, every edge running
/// from the prerequisite to the row that waits on it.
#[test]
fn the_edge_list_groups_the_stored_kinds_ahead_of_the_computed_overlap() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    assert_eq!(
        tasks(&root, &["edges"]),
        json!([
            { "kind": "needs", "from": 1, "to": 2 },
            { "kind": "needs", "from": 1, "to": 3 },
            { "kind": "needs", "from": 1, "to": 4 },
            { "kind": "needs", "from": 3, "to": 5 },
            { "kind": "coupling", "from": 2, "to": 3 },
            { "kind": "overlap", "from": 3, "to": 4 },
        ])
    );

    // Tasks 1 and 3 share no file and 3 and 4 share `shared.rs` with no path
    // between them, so the one overlap is the pair a file-claim scheduler has
    // to keep apart.
    assert_eq!(
        tasks(&root, &["edges", "--kind", "overlap"]),
        json!([{ "kind": "overlap", "from": 3, "to": 4 }])
    );
    assert_eq!(
        tasks(&root, &["edges", "--kind", "coupling"]),
        json!([{ "kind": "coupling", "from": 2, "to": 3 }])
    );
}

/// DOT source is written to stdout unwrapped, so its bytes are the contract:
/// a node per task, the three kinds styled apart, and a title's quote or
/// backslash escaped rather than ending the label early.
#[test]
fn dot_source_is_emitted_as_bare_graphviz_bytes() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    assert_eq!(
        tasks_stdout(&root, &["edges", "--dot"]),
        concat!(
            "digraph tasks {\n",
            "  1 [label=\"1: Scaffold the module tree\"];\n",
            "  2 [label=\"2: Wire the graph engine\"];\n",
            "  3 [label=\"3: Write the \\\"read\\\" verbs\"];\n",
            "  4 [label=\"4: Render the plan\\\\graph\"];\n",
            "  5 [label=\"5: Publish the docs\"];\n",
            "  1 -> 2;\n",
            "  1 -> 3;\n",
            "  1 -> 4;\n",
            "  3 -> 5;\n",
            "  2 -> 3 [style=dashed];\n",
            "  3 -> 4 [style=dotted, dir=none];\n",
            "}\n",
        )
    );

    // A kind filter drops the other two edge sets and keeps every node, so a
    // filtered graph still draws the whole plan.
    let needs_only = tasks_stdout(&root, &["edges", "--dot", "--kind", "needs"]);
    assert!(
        needs_only.contains("  5 [label=\"5: Publish the docs\"];\n"),
        "{needs_only}"
    );
    assert!(needs_only.contains("  1 -> 2;\n"), "{needs_only}");
    assert!(!needs_only.contains("style="), "{needs_only}");
}

// ---------------------------------------------------------------------------
// target resolution
// ---------------------------------------------------------------------------

/// Every read verb refuses an empty `--slug | --file` target as a
/// `kind=validation` envelope, not as clap usage prose on exit 2.
#[test]
fn every_read_verb_refuses_an_empty_target_as_validation() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, READ_FIXTURE);

    for args in [vec!["show", "1"], vec!["list"], vec!["edges"]] {
        let err = untargeted_err(&root, &args);
        assert_eq!(err["kind"], json!("validation"), "{args:?}");
        let message = err["message"].as_str().expect("a message string");
        assert!(message.contains("--slug"), "{args:?}: {message}");
        assert!(message.contains("--file"), "{args:?}: {message}");
    }
}
