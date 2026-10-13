//! `--set`, `--set-json` and `--set-file` on `items add`, `items update` and
//! `review run update`: the flags stand in for `--json` or merge over it, and
//! a value read from a file survives the TOML round trip byte for byte.

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
fn update_refuses_a_dotted_field_flag_key_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let ledger = seed_ledger_in(
        &root,
        "ledger.toml",
        "schema_version = 1\n\n[[items]]\nid = \"R1\"\nstatus = \"open\"\n\n[items.meta]\nowner = \"a\"\nteam = \"b\"\n",
    );
    let before = std::fs::read(&ledger).unwrap();

    for flag in ["--set", "--set-json"] {
        let value = if flag == "--set" {
            "meta.owner=x"
        } else {
            "meta.owner=\"x\""
        };
        let out = cli(&root)
            .args(["--error-format", "json", "items", "update"])
            .arg(&ledger)
            .args(["R1", flag, value])
            .assert()
            .failure()
            .code(1);
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        let err = parse_json_error_envelope(&stderr);
        assert_eq!(err["kind"], json!("validation"), "{flag}: {err}");
        let message = err["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("meta.owner") && message.contains("--set-json meta="),
            "{flag}: {message}"
        );
        assert_eq!(std::fs::read(&ledger).unwrap(), before, "{flag}");
    }
}

#[test]
fn set_json_reads_an_at_path_file() {
    let (_tmp, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let files_json = root.join("files.json");
    std::fs::write(&files_json, "\u{feff}[\"src/a.rs\", \"src/b.rs\"]\r\n").unwrap();

    cli(&root)
        .args(["items", "add"])
        .arg(&ledger)
        .args(["--id-prefix", "R", "--set", "summary=from file"])
        .arg("--set-json")
        .arg(format!("files=@{}", files_json.display()))
        .assert()
        .success();
    let row = get(&root, &ledger, "R2");
    assert_eq!(row["files"], json!(["src/a.rs", "src/b.rs"]));

    let store = root.join(".claude/review-v2/t.toml");
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    std::fs::write(
        &store,
        "schema_version = 1\n\n[[runs]]\nid = \"U1\"\ncommand = \"review-v2\"\nfamily = \"code\"\n\
         depth = \"quick\"\nstarted = \"2026-10-12T10:00:00Z\"\nbase = \"abc\"\nscope = [\"src/**\"]\n",
    )
    .unwrap();
    let waivers_json = root.join("waivers.json");
    std::fs::write(
        &waivers_json,
        r#"[{"concern":"ripple","reason":"no callers"}]"#,
    )
    .unwrap();
    cli(&root)
        .current_dir(&root)
        .args([
            "review",
            "run",
            "update",
            "U1",
            "--scope",
            "t",
            "--set-json",
        ])
        .arg(format!("waivers=@{}", waivers_json.display()))
        .assert()
        .success();
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&store).unwrap()).unwrap();
    let waiver = &doc["runs"][0]["waivers"][0];
    assert_eq!(waiver["concern"].as_str(), Some("ripple"), "{doc}");
    assert_eq!(waiver["reason"].as_str(), Some("no callers"), "{doc}");

    let before = std::fs::read(&ledger).unwrap();
    let bad = root.join("bad.json");
    std::fs::write(&bad, "[1,").unwrap();
    for (value, expect) in [
        (
            "line=@".to_string(),
            "`--set-json line=…` is not valid JSON:".to_string(),
        ),
        (
            format!("line=@{}", bad.display()),
            format!(
                "`--set-json line=…` is not valid JSON in `{}`",
                bad.display()
            ),
        ),
    ] {
        let out = cli(&root)
            .args(["--error-format", "json", "items", "update"])
            .arg(&ledger)
            .args(["R1", "--set-json", &value])
            .assert()
            .failure()
            .code(1);
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        let err = parse_json_error_envelope(&stderr);
        assert_eq!(err["kind"], json!("validation"), "{value}: {err}");
        let message = err["message"].as_str().unwrap_or_default();
        assert!(message.contains(&expect), "{value}: {message}");
    }
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
