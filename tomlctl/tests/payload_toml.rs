//! Black-box coverage for TOML record files on the payload flags: a `.toml`
//! path given to `--ndjson`, `--json '@…'`, `--ops '@…'` or
//! `--defaults-json '@…'` is read as a record file, and prose carrying
//! backticks, quotes, `$`, backslashes and newlines lands verbatim.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{
    TASKS_SLUG, assert_sidecar_matches, backlog, cli, parse_json_error_envelope, sandbox,
    seed_ledger_in, seed_tasks, store_path,
};

/// Prose as it must land in the store.
const PROSE: &str = "Run `cargo test` and say \"done\" -- it's $HOME\\bin, not 'x'.\nSecond line.";

/// The same prose as a `'''` block whose closer sits on its own line.
const PROSE_BLOCK: &str =
    "'''\nRun `cargo test` and say \"done\" -- it's $HOME\\bin, not 'x'.\nSecond line.\n'''";

const LEDGER: &str = "schema_version = 1\n";

const TASKS_FIXTURE: &str = r#"schema_version = 1
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
commit = ""

[[items]]
id = 2
ref = "wire-the-graph-engine"
title = "Wire the graph engine"
effort = "M"
status = "pending"
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

fn stage(root: &Path, name: &str, bytes: &str) -> String {
    let path = root.join(name);
    fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

fn read_toml(path: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn rows_of<'a>(doc: &'a toml::Value, array: &str) -> &'a [toml::Value] {
    doc.get(array)
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

fn row_by<'a>(doc: &'a toml::Value, array: &str, id: &str) -> &'a toml::Value {
    rows_of(doc, array)
        .iter()
        .find(|r| r.get("id").and_then(toml::Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("no `{array}` row {id} in:\n{doc}"))
}

fn str_field<'a>(row: &'a toml::Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(toml::Value::as_str)
}

/// A plain ledger, not an owned store, so the generic `items` writers run.
fn ledger(root: &Path) -> PathBuf {
    seed_ledger_in(root, "review-ledger.toml", LEDGER)
}

fn ok_json(root: &Path, args: &[&str]) -> Value {
    let out = cli(root).args(args).write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; got: {stdout}"))
}

fn err_json(root: &Path, args: &[&str]) -> Value {
    let out = cli(root)
        .args(["--error-format", "json"])
        .args(args)
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr))
}

fn mint(root: &Path, summary: &str) -> String {
    backlog(root, &["add", "--summary", summary, "--kind", "debt"])["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn items_add_many_lands_prose_verbatim() {
    let (_tmp, root) = sandbox();
    let l = ledger(&root);
    let src = stage(
        &root,
        "rows.toml",
        &format!(
            "[[record]]\nid = \"R1\"\nsummary = {PROSE_BLOCK}\n\n\
             [[record]]\nid = \"R2\"\nsummary = \"\"\"\nbasic '''quoted''' text\n\"\"\"\n"
        ),
    );

    let out = ok_json(
        &root,
        &["items", "add-many", l.to_str().unwrap(), "--ndjson", &src],
    );
    assert_eq!(out["added"], json!(2), "{out}");

    let doc = read_toml(&l);
    assert_eq!(
        str_field(row_by(&doc, "items", "R1"), "summary"),
        Some(PROSE)
    );
    assert_eq!(
        str_field(row_by(&doc, "items", "R2"), "summary"),
        Some("basic '''quoted''' text")
    );
    assert_sidecar_matches(&l);
}

#[test]
fn a_crlf_encoded_record_file_reads_as_lf() {
    let (_tmp, root) = sandbox();
    let l = ledger(&root);
    let src = stage(
        &root,
        "crlf.toml",
        "[[record]]\r\nid = \"R1\"\r\nsummary = '''\r\nline one `a`\r\nline two $b\r\n'''\r\n",
    );

    ok_json(
        &root,
        &["items", "add-many", l.to_str().unwrap(), "--ndjson", &src],
    );
    let doc = read_toml(&l);
    assert_eq!(
        str_field(row_by(&doc, "items", "R1"), "summary"),
        Some("line one `a`\nline two $b")
    );
}

#[test]
fn items_apply_reads_ops_from_a_toml_file() {
    let (_tmp, root) = sandbox();
    let l = ledger(&root);
    let src = stage(
        &root,
        "ops.toml",
        &format!("[[record]]\nop = \"add\"\n[record.json]\nid = \"R1\"\nsummary = {PROSE_BLOCK}\n"),
    );

    ok_json(
        &root,
        &[
            "items",
            "apply",
            l.to_str().unwrap(),
            "--ops",
            &format!("@{src}"),
        ],
    );
    let doc = read_toml(&l);
    assert_eq!(
        str_field(row_by(&doc, "items", "R1"), "summary"),
        Some(PROSE)
    );
}

#[test]
fn items_add_json_reads_one_object_from_a_toml_file() {
    let (_tmp, root) = sandbox();
    let l = ledger(&root);
    let src = stage(
        &root,
        "one.toml",
        &format!("id = \"R1\"\nsummary = {PROSE_BLOCK}\n"),
    );

    ok_json(
        &root,
        &[
            "items",
            "add",
            l.to_str().unwrap(),
            "--json",
            &format!("@{src}"),
        ],
    );
    let doc = read_toml(&l);
    assert_eq!(
        str_field(row_by(&doc, "items", "R1"), "summary"),
        Some(PROSE)
    );
}

#[test]
fn items_add_many_defaults_json_reads_a_toml_file() {
    let (_tmp, root) = sandbox();
    let l = ledger(&root);
    let defaults = stage(
        &root,
        "defaults.toml",
        &format!("status = \"open\"\nnote = {PROSE_BLOCK}\n"),
    );
    let rows = stage(&root, "rows.toml", "[[record]]\nid = \"R1\"\n");

    ok_json(
        &root,
        &[
            "items",
            "add-many",
            l.to_str().unwrap(),
            "--ndjson",
            &rows,
            "--defaults-json",
            &format!("@{defaults}"),
        ],
    );
    let doc = read_toml(&l);
    let row = row_by(&doc, "items", "R1");
    assert_eq!(str_field(row, "status"), Some("open"));
    assert_eq!(str_field(row, "note"), Some(PROSE));
}

#[test]
fn backlog_add_many_lands_context_verbatim() {
    let (_tmp, root) = sandbox();
    let src = stage(
        &root,
        "adds.toml",
        &format!(
            "[[record]]\nsummary = \"toml payload flake\"\nkind = \"flaky-test\"\n\
             area = \"tomlctl/src/payload.rs\"\ncontext = {PROSE_BLOCK}\n"
        ),
    );

    let out = backlog(&root, &["add-many", "--ndjson", &src]);
    let id = out["added"][0].as_str().unwrap().to_string();
    let doc = read_toml(&store_path(&root));
    assert_eq!(
        str_field(row_by(&doc, "backlog", &id), "context"),
        Some(PROSE)
    );
    assert_sidecar_matches(&store_path(&root));
}

#[test]
fn backlog_check_grades_toml_probes_and_echoes_prose() {
    let (_tmp, root) = sandbox();
    let summary = "probe with `ticks`, \"quotes\" and $VARS";
    let src = stage(
        &root,
        "probes.toml",
        &format!(
            "[[record]]\nsummary = '''{summary}'''\nkind = \"bug\"\n\
             area = \"tomlctl/src/payload.rs\"\n"
        ),
    );

    let out = backlog(&root, &["check", "--ndjson", &src]);
    let results = out["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{out}");
    assert_eq!(results[0]["summary"], json!(summary), "{out}");
    assert_eq!(results[0]["verdict"], json!("novel"), "{out}");
}

#[test]
fn flow_record_lands_prose_and_resolves_the_row_task() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, TASKS_FIXTURE);
    let record = store.parent().unwrap().join("execution-record.toml");
    let src = stage(
        &root,
        "deviations.toml",
        &format!(
            "[[record]]\ntask = 2\nsummary = '''kept `sync` for $CACHE'''\n\
             original_intent = '''go \"async\"'''\nrationale = {PROSE_BLOCK}\n"
        ),
    );

    let out = ok_json(
        &root,
        &[
            "flow",
            "record",
            "--slug",
            TASKS_SLUG,
            "--type",
            "deviation",
            "--set",
            "agent=implement",
            "--ndjson",
            &src,
        ],
    );
    assert_eq!(out["ids"], json!(["E1"]), "{out}");
    let doc = read_toml(&record);
    let row = &rows_of(&doc, "items")[0];
    assert_eq!(str_field(row, "rationale"), Some(PROSE));
    assert_eq!(str_field(row, "summary"), Some("kept `sync` for $CACHE"));
    assert_eq!(str_field(row, "task_ref"), Some("wire-the-graph-engine"));
}

#[test]
fn tasks_update_lands_prose_through_a_set_table() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, TASKS_FIXTURE);
    let src = stage(
        &root,
        "update.toml",
        &format!("[[record]]\nid = 2\nstatus = \"in-progress\"\nset.detail = {PROSE_BLOCK}\n"),
    );

    let out = ok_json(
        &root,
        &["tasks", "update", "--slug", TASKS_SLUG, "--ndjson", &src],
    );
    assert_eq!(out["ok"], json!(true), "{out}");
    let doc = read_toml(&store);
    let row = &rows_of(&doc, "items")[1];
    assert_eq!(str_field(row, "detail"), Some(PROSE));
    assert_eq!(str_field(row, "status"), Some("in-progress"));
    assert_sidecar_matches(&store);
}

#[test]
fn backlog_triage_lands_resolution_and_dismissal_prose() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "first triage target");
    let b = mint(&root, "second triage target");
    let src = stage(
        &root,
        "triage.toml",
        &format!(
            "[[record]]\nid = \"{a}\"\nresolve = {PROSE_BLOCK}\n\n\
             [[record]]\nid = \"{b}\"\ndismiss = '''Not ours: C:\\tmp\\x'''\n"
        ),
    );

    backlog(&root, &["triage", "--ndjson", &src]);
    let doc = read_toml(&store_path(&root));
    assert_eq!(
        str_field(row_by(&doc, "backlog", &a), "resolution"),
        Some(PROSE)
    );
    assert_eq!(
        str_field(row_by(&doc, "backlog", &b), "dismiss_reason"),
        Some("Not ours: C:\\tmp\\x")
    );
}

#[test]
fn backlog_edit_lands_context_prose() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "edit target");
    let src = stage(
        &root,
        "edit.toml",
        &format!("[[record]]\nid = \"{id}\"\ncontext = {PROSE_BLOCK}\ntags = [\"x\"]\n"),
    );

    backlog(&root, &["edit", "--ndjson", &src]);
    let doc = read_toml(&store_path(&root));
    assert_eq!(
        str_field(row_by(&doc, "backlog", &id), "context"),
        Some(PROSE)
    );
}

#[test]
fn a_syntax_error_names_the_line_and_column() {
    let (_tmp, root) = sandbox();
    let l = ledger(&root);
    let src = stage(&root, "bad.toml", "[[record]]\nid = \"R1\"\nsummary = \n");

    let err = err_json(
        &root,
        &["items", "add-many", l.to_str().unwrap(), "--ndjson", &src],
    );
    let message = err["message"].as_str().unwrap();
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(message.contains("bad.toml"), "{message}");
    assert!(message.contains("line"), "{message}");
    assert!(message.contains("column"), "{message}");
    assert_eq!(fs::read_to_string(&l).unwrap(), LEDGER);
}

#[test]
fn a_top_level_key_beside_record_rows_is_refused() {
    let (_tmp, root) = sandbox();
    let l = ledger(&root);
    let src = stage(
        &root,
        "shared.toml",
        "shared = \"x\"\n\n[[record]]\nid = \"R1\"\n",
    );

    let err = err_json(
        &root,
        &["items", "add-many", l.to_str().unwrap(), "--ndjson", &src],
    );
    let message = err["message"].as_str().unwrap();
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(message.contains("`shared`"), "{message}");
    assert_eq!(fs::read_to_string(&l).unwrap(), LEDGER);
}
