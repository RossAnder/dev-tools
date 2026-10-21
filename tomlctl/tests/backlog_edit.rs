//! Black-box coverage for `tomlctl backlog edit`.

use serde_json::json;
use std::fs;
use std::path::Path;

mod common;
use common::{assert_sidecar_matches, backlog, cli, sandbox, store_path};

fn mint(root: &Path, summary: &str) -> String {
    let added = backlog(
        root,
        &[
            "add",
            "--summary",
            summary,
            "--kind",
            "debt",
            "--tag",
            "keep",
        ],
    );
    added["id"].as_str().unwrap().to_string()
}

fn read_store(root: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(store_path(root)).unwrap()).unwrap()
}

fn row(root: &Path, id: &str) -> toml::Value {
    read_store(root)["backlog"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_str() == Some(id))
        .unwrap()
        .clone()
}

fn strs(row: &toml::Value, key: &str) -> Vec<String> {
    row.get(key)
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

fn failure_stderr(root: &Path, args: &[&str]) -> String {
    let out = cli(root)
        .arg("backlog")
        .args(args)
        .write_stdin("")
        .assert()
        .failure();
    String::from_utf8_lossy(&out.get_output().stderr).to_string()
}

#[test]
fn context_is_replaced_from_a_file_with_backticks_and_quotes() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "first discovery");
    let text = "run `cargo test` and say \"done\" -- it's 'fine'";
    let file = root.join("ctx.md");
    fs::write(&file, format!("{text}\n")).unwrap();

    let out = backlog(
        &root,
        &["edit", &id, "--context-file", file.to_str().unwrap()],
    );
    assert_eq!(out["ok"], json!(true));
    assert_eq!(out["rows"], json!([{"id": id, "changed": true}]));
    assert_eq!(row(&root, &id)["context"].as_str(), Some(text));
    assert_sidecar_matches(&store_path(&root));
}

#[test]
fn tags_are_added_and_removed() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "second discovery");

    let out = backlog(
        &root,
        &[
            "edit",
            &id,
            "--add-tag",
            "x",
            "--add-tag",
            "y",
            "--remove-tag",
            "keep",
        ],
    );
    assert_eq!(out["rows"][0]["changed"], json!(true));
    assert_eq!(strs(&row(&root, &id), "tags"), ["x", "y"]);

    let again = backlog(&root, &["edit", &id, "--add-tag", "x"]);
    assert_eq!(again["rows"][0]["changed"], json!(false));
}

#[test]
fn ndjson_edits_two_rows_without_a_positional_id() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha discovery");
    let b = mint(&root, "beta discovery");
    let ndjson = root.join("edits.ndjson");
    fs::write(
        &ndjson,
        format!(
            "{}\n{}\n",
            json!({"id": a, "origin": "review", "tags": ["t1"]}),
            json!({"id": b, "context": "why", "evidence": ["x.log"]}),
        ),
    )
    .unwrap();

    let out = backlog(&root, &["edit", "--ndjson", ndjson.to_str().unwrap()]);
    assert_eq!(
        out["rows"],
        json!([{"id": a, "changed": true}, {"id": b, "changed": true}])
    );
    let ra = row(&root, &a);
    assert_eq!(ra["origin"].as_str(), Some("review"));
    assert_eq!(strs(&ra, "tags"), ["t1"]);
    let rb = row(&root, &b);
    assert_eq!(rb["context"].as_str(), Some("why"));
    assert_eq!(strs(&rb, "evidence"), ["x.log"]);
}

#[test]
fn a_toml_record_file_is_read() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "record discovery");
    let record = root.join("edits.toml");
    fs::write(
        &record,
        format!("[[record]]\nid = \"{id}\"\ncontext = '''\nuse `a` and \"b\"\n'''\n"),
    )
    .unwrap();

    backlog(&root, &["edit", "--ndjson", record.to_str().unwrap()]);
    assert_eq!(
        row(&root, &id)["context"].as_str(),
        Some("use `a` and \"b\"")
    );
}

#[test]
fn summary_is_refused_naming_backlog_add() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "gamma discovery");
    let before = fs::read(store_path(&root)).unwrap();
    let ndjson = root.join("bad.ndjson");
    fs::write(
        &ndjson,
        format!("{}\n", json!({"id": id, "summary": "renamed"})),
    )
    .unwrap();

    let stderr = failure_stderr(&root, &["edit", "--ndjson", ndjson.to_str().unwrap()]);
    assert!(stderr.contains("backlog add"), "{stderr}");
    assert_eq!(fs::read(store_path(&root)).unwrap(), before);
}

#[test]
fn status_is_refused_naming_backlog_triage() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "delta discovery");
    let ndjson = root.join("bad.ndjson");
    fs::write(
        &ndjson,
        format!("{}\n", json!({"id": id, "status": "resolved"})),
    )
    .unwrap();

    let stderr = failure_stderr(&root, &["edit", "--ndjson", ndjson.to_str().unwrap()]);
    assert!(stderr.contains("backlog triage"), "{stderr}");
}

#[test]
fn an_unknown_id_fails_the_whole_batch() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "epsilon discovery");
    let before = fs::read(store_path(&root)).unwrap();
    let ndjson = root.join("mixed.ndjson");
    fs::write(
        &ndjson,
        format!(
            "{}\n{}\n",
            json!({"id": id, "context": "changed"}),
            json!({"id": "B-deadbeef", "context": "changed"}),
        ),
    )
    .unwrap();

    let stderr = failure_stderr(&root, &["edit", "--ndjson", ndjson.to_str().unwrap()]);
    assert!(stderr.contains("B-deadbeef"), "{stderr}");
    assert_eq!(fs::read(store_path(&root)).unwrap(), before);
}

#[test]
fn dry_run_writes_nothing() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "zeta discovery");
    let store = store_path(&root);
    let sidecar = root.join(".claude").join("backlog.toml.sha256");
    let before = (fs::read(&store).unwrap(), fs::read(&sidecar).unwrap());

    let out = backlog(&root, &["edit", &id, "--context", "preview", "--dry-run"]);
    assert_eq!(out["dry_run"], json!(true));
    assert_eq!(out["rows"][0]["changed"], json!(true));
    assert_eq!(
        (fs::read(&store).unwrap(), fs::read(&sidecar).unwrap()),
        before
    );
}

#[test]
fn an_edit_with_no_fields_is_refused() {
    let (_tmp, root) = sandbox();
    let id = mint(&root, "eta discovery");
    let stderr = failure_stderr(&root, &["edit", &id]);
    assert!(stderr.contains("nothing to edit"), "{stderr}");
}

#[test]
fn an_edit_that_leaves_a_row_invalid_is_refused_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = store_path(&root);
    fs::write(
        &store,
        "schema_version = 1\n\n[[backlog]]\nid = \"B-00000001\"\nsummary = \"broken\"\nstatus = \"bogus\"\n",
    )
    .unwrap();
    let before = fs::read(&store).unwrap();

    let stderr = failure_stderr(&root, &["edit", "B-00000001", "--context", "x"]);
    assert!(stderr.contains("bogus"), "{stderr}");
    assert_eq!(fs::read(&store).unwrap(), before);
}
