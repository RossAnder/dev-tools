//! `items apply --id-prefix` mints ids for its id-less add ops, and the
//! `[id_high_water]` mark keeps a removed top id from being minted again.

use std::fs;
use std::path::Path;

mod common;
use common::{cli, seed_ledger};

fn run(root: &Path, args: &[&str], stdin: &str) -> std::process::Output {
    cli(root).args(args).write_stdin(stdin).output().unwrap()
}

fn ok_stdout(root: &Path, args: &[&str], stdin: &str) -> String {
    let out = run(root, args, stdin);
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn ok_json(root: &Path, args: &[&str], stdin: &str) -> serde_json::Value {
    serde_json::from_str(&ok_stdout(root, args, stdin)).unwrap()
}

fn ledger(path: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn ledger_ids(path: &Path) -> Vec<String> {
    ledger(path)
        .get("items")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .map(|r| r["id"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

const MIXED_OPS: &str = r#"[
    {"op":"add","json":{"summary":"first"}},
    {"op":"update","id":"R1","json":{"status":"fixed"}},
    {"op":"add","json":{"summary":"second"}}
]"#;

#[test]
fn mixed_apply_mints_a_contiguous_run_for_its_id_less_adds() {
    let (dir, path) = seed_ledger(
        "schema_version = 1\n\n[[items]]\nid = \"R1\"\nstatus = \"open\"\n\n[[items]]\nid = \"E3\"\n",
    );
    let l = path.to_str().unwrap();
    let env = ok_json(
        dir.path(),
        &["items", "apply", l, "--ops", "-", "--id-prefix", "E"],
        MIXED_OPS,
    );
    assert_eq!(env["ids"], serde_json::json!(["E4", "E5"]));
    assert_eq!(ledger_ids(&path), ["R1", "E3", "E4", "E5"]);
    let doc = ledger(&path);
    let rows = doc["items"].as_array().unwrap();
    assert_eq!(rows[0]["status"].as_str(), Some("fixed"));
    assert_eq!(rows[2]["summary"].as_str(), Some("first"));
    assert_eq!(rows[3]["summary"].as_str(), Some("second"));
}

#[test]
fn apply_dry_run_reports_the_minted_ids_and_writes_nothing() {
    let (dir, path) = seed_ledger(
        "schema_version = 1\n\n[[items]]\nid = \"R1\"\nstatus = \"open\"\n\n[[items]]\nid = \"E3\"\n",
    );
    let l = path.to_str().unwrap();
    let before = fs::read_to_string(&path).unwrap();
    let env = ok_json(
        dir.path(),
        &[
            "items",
            "apply",
            l,
            "--ops",
            "-",
            "--id-prefix",
            "E",
            "--dry-run",
        ],
        MIXED_OPS,
    );
    assert_eq!(env["ids"], serde_json::json!(["E4", "E5"]));
    assert_eq!(
        env["would_change"]["ids"],
        serde_json::json!(["E4", "E5", "R1"])
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn apply_add_op_carrying_an_id_is_refused_under_id_prefix() {
    let (dir, path) = seed_ledger("schema_version = 1\n\n[[items]]\nid = \"E1\"\n");
    let l = path.to_str().unwrap();
    let before = fs::read_to_string(&path).unwrap();
    let out = run(
        dir.path(),
        &[
            "--error-format",
            "json",
            "items",
            "apply",
            l,
            "--ops",
            "-",
            "--id-prefix",
            "E",
        ],
        r#"[{"op":"add","json":{"summary":"a"}},{"op":"add","json":{"id":"E9","summary":"b"}}]"#,
    );
    assert!(!out.status.success());
    let err = common::parse_json_error_envelope(&String::from_utf8_lossy(&out.stderr));
    assert_eq!(err["kind"], "validation");
    assert!(
        err["message"].as_str().unwrap_or_default().contains("op 2"),
        "error should name the offending op: {err}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn apply_without_id_prefix_omits_ids_from_the_envelope() {
    let (dir, path) = seed_ledger("schema_version = 1\n");
    let l = path.to_str().unwrap();
    let env = ok_json(
        dir.path(),
        &["items", "apply", l, "--ops", "-"],
        r#"[{"op":"add","json":{"id":"R1","summary":"a"}}]"#,
    );
    assert!(env.get("ids").is_none(), "unexpected ids: {env}");
    assert_eq!(ledger_ids(&path), ["R1"]);
}

#[test]
fn removing_the_top_id_is_never_minted_again() {
    let (dir, path) = seed_ledger(
        "schema_version = 1\n\n[[items]]\nid = \"E1\"\n\n[[items]]\nid = \"E2\"\n\n[[items]]\nid = \"E3\"\n",
    );
    let l = path.to_str().unwrap();
    ok_stdout(dir.path(), &["items", "remove", l, "E3"], "");
    assert_eq!(
        ledger(&path)["id_high_water"]["E"].as_integer(),
        Some(3),
        "remove must record the retired number"
    );
    let next = ok_stdout(dir.path(), &["items", "next-id", l, "--prefix", "E"], "");
    assert_eq!(next.trim(), "E4");
    let env = ok_json(
        dir.path(),
        &[
            "items",
            "add",
            l,
            "--json",
            r#"{"summary":"a"}"#,
            "--id-prefix",
            "E",
        ],
        "",
    );
    assert_eq!(env["id"], "E4");
    assert_eq!(ledger_ids(&path), ["E1", "E2", "E4"]);
}

#[test]
fn an_apply_remove_op_raises_the_high_water_mark() {
    let (dir, path) =
        seed_ledger("schema_version = 1\n\n[[items]]\nid = \"E1\"\n\n[[items]]\nid = \"E2\"\n");
    let l = path.to_str().unwrap();
    ok_stdout(
        dir.path(),
        &["items", "apply", l, "--ops", "-"],
        r#"[{"op":"remove","id":"E2"}]"#,
    );
    let env = ok_json(
        dir.path(),
        &["items", "apply", l, "--ops", "-", "--id-prefix", "E"],
        r#"[{"op":"add","json":{"summary":"a"}}]"#,
    );
    assert_eq!(env["ids"], serde_json::json!(["E3"]));
    assert_eq!(ledger_ids(&path), ["E1", "E3"]);
}

#[test]
fn removing_a_lower_id_leaves_a_higher_mark_in_place() {
    let (dir, path) = seed_ledger(
        "schema_version = 1\n\n[id_high_water]\nE = 9\n\n[[items]]\nid = \"E1\"\n\n[[items]]\nid = \"E2\"\n",
    );
    let l = path.to_str().unwrap();
    ok_stdout(dir.path(), &["items", "remove", l, "E2"], "");
    assert_eq!(ledger(&path)["id_high_water"]["E"].as_integer(), Some(9));
    let next = ok_stdout(dir.path(), &["items", "next-id", l, "--prefix", "E"], "");
    assert_eq!(next.trim(), "E10");
}
