//! `items add --id-prefix` and `items add-many --id-prefix`: the id is minted
//! inside the locked write and reported as `id` / `ids` in the envelope.

use std::fs;
use std::path::Path;

mod common;
use common::{cli, seed_ledger};

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    cli(root).args(args).write_stdin("").output().unwrap()
}

fn ok_json(root: &Path, args: &[&str]) -> serde_json::Value {
    let out = run(root, args);
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn ledger_ids(ledger: &Path) -> Vec<String> {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(ledger).unwrap()).unwrap();
    doc.get("items")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .map(|r| r["id"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn sequential_adds_mint_e1_then_e2() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let l = ledger.to_str().unwrap();
    let first = ok_json(
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
    );
    assert_eq!(first["id"], "E1");
    assert_eq!(first["added"], 1);
    let second = ok_json(
        dir.path(),
        &[
            "items",
            "add",
            l,
            "--json",
            r#"{"summary":"b"}"#,
            "--id-prefix",
            "E",
        ],
    );
    assert_eq!(second["id"], "E2");
    assert_eq!(ledger_ids(&ledger), ["E1", "E2"]);
}

#[test]
fn concurrent_id_prefix_adds_mint_distinct_ids() {
    use std::thread;

    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let root = dir.path().to_path_buf();
    let l = ledger.to_str().unwrap().to_string();

    let handles: Vec<_> = ["a", "b", "c", "d"]
        .iter()
        .map(|summary| {
            let root = root.clone();
            let l = l.clone();
            let payload = format!(r#"{{"summary":"{summary}"}}"#);
            thread::spawn(move || {
                cli(&root)
                    .env("TOMLCTL_LOCK_TIMEOUT", "30")
                    .args(["items", "add", &l, "--json", &payload, "--id-prefix", "E"])
                    .write_stdin("")
                    .assert()
                    .success();
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("every writer must succeed");
    }

    let mut ids = ledger_ids(&ledger);
    ids.sort_unstable();
    assert_eq!(ids, ["E1", "E2", "E3", "E4"]);
}

#[test]
fn add_mints_past_the_existing_high_water_mark() {
    let (dir, ledger) =
        seed_ledger("schema_version = 1\n\n[[items]]\nid = \"E7\"\n\n[[items]]\nid = \"R40\"\n");
    let l = ledger.to_str().unwrap();
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
    );
    assert_eq!(env["id"], "E8");
}

#[test]
fn add_many_mints_a_contiguous_run_in_input_order() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n\n[[items]]\nid = \"E3\"\n");
    let l = ledger.to_str().unwrap();
    let out = cli(dir.path())
        .args(["items", "add-many", l, "--ndjson", "-", "--id-prefix", "E"])
        .write_stdin("{\"summary\":\"a\"}\n{\"summary\":\"b\"}\n{\"summary\":\"c\"}\n")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(env["added"], 3);
    assert_eq!(env["ids"], serde_json::json!(["E4", "E5", "E6"]));
    assert_eq!(ledger_ids(&ledger), ["E3", "E4", "E5", "E6"]);
}

#[test]
fn add_many_dedupe_skip_consumes_no_id() {
    let (dir, ledger) =
        seed_ledger("schema_version = 1\n\n[[items]]\nid = \"E1\"\nsummary = \"dup\"\n");
    let l = ledger.to_str().unwrap();
    let out = cli(dir.path())
        .args([
            "items",
            "add-many",
            l,
            "--ndjson",
            "-",
            "--id-prefix",
            "E",
            "--dedupe-by",
            "summary",
        ])
        .write_stdin("{\"summary\":\"new1\"}\n{\"summary\":\"dup\"}\n{\"summary\":\"new2\"}\n")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(env["ids"], serde_json::json!(["E2", "E3"]));
    assert_eq!(env["skipped"], 1);
    assert_eq!(env["skipped_rows"][0]["matched_id"], "E1");
}

#[test]
fn explicit_id_with_id_prefix_is_refused() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let l = ledger.to_str().unwrap();
    let before = fs::read_to_string(&ledger).unwrap();
    let out = run(
        dir.path(),
        &[
            "--error-format",
            "json",
            "items",
            "add",
            l,
            "--json",
            r#"{"id":"E9","summary":"a"}"#,
            "--id-prefix",
            "E",
        ],
    );
    assert!(!out.status.success());
    let err = common::parse_json_error_envelope(&String::from_utf8_lossy(&out.stderr));
    assert_eq!(err["kind"], "validation");
    assert_eq!(fs::read_to_string(&ledger).unwrap(), before);

    let many = cli(dir.path())
        .args(["items", "add-many", l, "--ndjson", "-", "--id-prefix", "E"])
        .write_stdin("{\"summary\":\"a\"}\n{\"id\":\"E9\",\"summary\":\"b\"}\n")
        .output()
        .unwrap();
    assert!(!many.status.success());
    assert!(String::from_utf8_lossy(&many.stderr).contains("row 2"));
    assert_eq!(fs::read_to_string(&ledger).unwrap(), before);
}

#[test]
fn get_id_prints_the_bare_minted_id() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let l = ledger.to_str().unwrap();
    let out = run(
        dir.path(),
        &[
            "items",
            "add",
            l,
            "--json",
            r#"{"summary":"a"}"#,
            "--id-prefix",
            "E",
            "--get",
            "id",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "E1\n");
}

#[test]
fn dedupe_skip_mints_nothing_and_get_id_prints_the_matched_id() {
    let (dir, ledger) =
        seed_ledger("schema_version = 1\n\n[[items]]\nid = \"E1\"\nsummary = \"dup\"\n");
    let l = ledger.to_str().unwrap();
    let before = fs::read_to_string(&ledger).unwrap();
    let out = run(
        dir.path(),
        &[
            "items",
            "add",
            l,
            "--json",
            r#"{"summary":"dup"}"#,
            "--id-prefix",
            "E",
            "--dedupe-by",
            "summary",
            "--get",
            "id",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "E1\n");
    assert_eq!(fs::read_to_string(&ledger).unwrap(), before);

    let next = ok_json(
        dir.path(),
        &[
            "items",
            "add",
            l,
            "--json",
            r#"{"summary":"fresh"}"#,
            "--id-prefix",
            "E",
        ],
    );
    assert_eq!(next["id"], "E2");
}

#[test]
fn dry_run_reports_the_minted_ids_without_writing() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n\n[[items]]\nid = \"E1\"\n");
    let l = ledger.to_str().unwrap();
    let before = fs::read_to_string(&ledger).unwrap();
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
            "--dry-run",
        ],
    );
    assert_eq!(env["would_change"]["ids"], serde_json::json!(["E2"]));
    let out = cli(dir.path())
        .args([
            "items",
            "add-many",
            l,
            "--ndjson",
            "-",
            "--id-prefix",
            "E",
            "--dry-run",
        ])
        .write_stdin("{\"summary\":\"a\"}\n{\"summary\":\"b\"}\n")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(env["would_change"]["ids"], serde_json::json!(["E2", "E3"]));
    assert_eq!(fs::read_to_string(&ledger).unwrap(), before);
}
