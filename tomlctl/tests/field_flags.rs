//! `--set`, `--set-json` and `--set-file` on `items add` and `items update`:
//! the flags stand in for `--json` or merge over it, and a prose value read
//! from a file survives the TOML round trip byte for byte.

use serde_json::{Value, json};
use std::path::Path;

mod common;
use common::{cli, parse_json_error_envelope, sandbox, seed_ledger_in};

const LEDGER: &str = r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"
"#;

fn get(root: &Path, ledger: &Path, id: &str) -> Value {
    let out = cli(root)
        .args(["items", "get"])
        .arg(ledger)
        .arg(id)
        .assert()
        .success();
    serde_json::from_slice(&out.get_output().stdout).expect("items get prints JSON")
}

#[test]
fn add_with_only_field_flags_mints_an_id() {
    let (_tmp, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);

    let out = cli(&root)
        .args(["items", "add"])
        .arg(&ledger)
        .args([
            "--set",
            "summary=second finding",
            "--set",
            "status=open",
            "--set-json",
            "line=42",
            "--id-prefix",
            "R",
            "--get",
            "id",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(stdout.trim(), "R2");

    let row = get(&root, &ledger, "R2");
    assert_eq!(row["summary"], json!("second finding"));
    assert_eq!(row["status"], json!("open"));
    assert_eq!(row["line"], json!(42));
}

#[test]
fn field_flags_merge_over_json_and_win() {
    let (_tmp, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);

    cli(&root)
        .args(["items", "add"])
        .arg(&ledger)
        .args([
            "--json",
            r#"{"id":"R9","summary":"from json","status":"open"}"#,
            "--set",
            "summary=from flag",
        ])
        .assert()
        .success();

    let row = get(&root, &ledger, "R9");
    assert_eq!(row["summary"], json!("from flag"));
    assert_eq!(row["status"], json!("open"));
}

#[test]
fn set_file_prose_round_trips_byte_identical() {
    let (_tmp, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let prose = "He said \"don't\" -- path C:\\Users\\x\\y.rs\nsecond line with ''' and \"\"\"\n\n\tindented \\n literal";
    let prose_path = root.join("prose.txt");
    std::fs::write(&prose_path, prose).unwrap();

    cli(&root)
        .args(["items", "add"])
        .arg(&ledger)
        .args(["--id-prefix", "R", "--set", "summary=prose"])
        .arg("--set-file")
        .arg(format!("description={}", prose_path.display()))
        .assert()
        .success();

    let row = get(&root, &ledger, "R2");
    assert_eq!(row["description"].as_str(), Some(prose));
}

#[test]
fn update_with_set_changes_only_the_named_field() {
    let (_tmp, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);

    cli(&root)
        .args(["items", "update"])
        .arg(&ledger)
        .args(["R1", "--set", "status=fixed"])
        .assert()
        .success();

    let row = get(&root, &ledger, "R1");
    assert_eq!(row["status"], json!("fixed"));
    assert_eq!(row["summary"], json!("first"));
}

#[test]
fn a_key_given_twice_is_a_validation_error() {
    let (_tmp, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let before = std::fs::read(&ledger).unwrap();

    let out = cli(&root)
        .args(["--error-format", "json", "items", "update"])
        .arg(&ledger)
        .args([
            "R1",
            "--set",
            "status=fixed",
            "--set-json",
            "status=\"open\"",
        ])
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], json!("validation"));
    assert_eq!(std::fs::read(&ledger).unwrap(), before);
}

#[test]
fn add_without_json_or_a_field_flag_is_refused() {
    let (_tmp, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);

    cli(&root)
        .args(["items", "add"])
        .arg(&ledger)
        .assert()
        .failure()
        .code(2);
}
