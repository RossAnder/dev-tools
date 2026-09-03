//! The agent-facing I/O contract: the `@<path>` payload form on every
//! JSON-accepting flag, the single-line pipe idiom for `--ndjson -`, the
//! `--select` + `--ndjson` output composition, and case-insensitive
//! `--tier`. Each is documented in `claude/skills/tomlctl/references/`;
//! these pin the behaviour those references promise.

mod common;

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

fn seeded_ledger(dir: &Path) -> PathBuf {
    let claude = dir.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("ledger.toml");
    fs::write(
        &ledger,
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "seed"
status = "open"
"#,
    )
    .unwrap();
    ledger
}

fn tomlctl(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5");
    cmd
}

#[test]
fn items_apply_reads_ops_from_at_file() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = seeded_ledger(dir.path());
    let ops = dir.path().join("ops.json");
    fs::write(
        &ops,
        r#"[{"op":"add","json":{"id":"R42","summary":"added via @file","status":"open"}}]"#,
    )
    .unwrap();

    tomlctl(dir.path())
        .args(["items", "apply"])
        .arg(&ledger)
        .arg("--ops")
        .arg(format!("@{}", ops.display()))
        .assert()
        .success();

    let out = fs::read_to_string(&ledger).unwrap();
    assert!(out.contains("added via @file"), "got:\n{out}");
}

#[test]
fn items_add_many_takes_rows_on_stdin_and_defaults_from_at_file() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = seeded_ledger(dir.path());
    let defaults = dir.path().join("defaults.json");
    fs::write(&defaults, r#"{"status":"open","rounds":1}"#).unwrap();

    // Two sources on one invocation: rows on stdin, defaults from a file.
    tomlctl(dir.path())
        .args(["items", "add-many"])
        .arg(&ledger)
        .args(["--ndjson", "-", "--defaults-json"])
        .arg(format!("@{}", defaults.display()))
        .write_stdin(
            "{\"id\":\"R2\",\"summary\":\"a|b\"}\n{\"id\":\"R3\",\"summary\":\"tab\\there\"}\n",
        )
        .assert()
        .success()
        .stdout(predicates::str::contains(r#""added":2"#));

    let out = fs::read_to_string(&ledger).unwrap();
    assert!(out.contains(r#"id = "R3""#), "got:\n{out}");
    assert_eq!(
        out.matches("rounds = 1").count(),
        2,
        "defaults stamped on both rows:\n{out}"
    );
}

#[test]
fn items_add_many_accepts_at_prefixed_ndjson_path() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = seeded_ledger(dir.path());
    let rows = dir.path().join("rows.ndjson");
    fs::write(
        &rows,
        "{\"id\":\"R9\",\"summary\":\"from @path\",\"status\":\"open\"}\n",
    )
    .unwrap();

    tomlctl(dir.path())
        .args(["items", "add-many"])
        .arg(&ledger)
        .arg("--ndjson")
        .arg(format!("@{}", rows.display()))
        .assert()
        .success();

    assert!(fs::read_to_string(&ledger).unwrap().contains("from @path"));
}

#[test]
fn at_file_missing_names_the_path_and_leaves_the_ledger_alone() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = seeded_ledger(dir.path());
    let before = fs::read_to_string(&ledger).unwrap();

    tomlctl(dir.path())
        .args(["items", "apply"])
        .arg(&ledger)
        .args(["--ops", "@no-such-ops.json"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("@no-such-ops.json"));

    assert_eq!(fs::read_to_string(&ledger).unwrap(), before);
}

#[test]
fn second_stdin_sentinel_points_at_the_at_form() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = seeded_ledger(dir.path());

    tomlctl(dir.path())
        .args(["items", "add-many"])
        .arg(&ledger)
        .args(["--ndjson", "-", "--defaults-json", "-"])
        .write_stdin("{\"id\":\"R2\"}\n")
        .assert()
        .failure()
        .stderr(predicates::str::contains("already consumed"))
        .stderr(predicates::str::contains("@<path>"));
}

#[test]
fn items_list_select_composes_with_ndjson() {
    let stdout = common::run_list_query(&["--status", "open", "--select", "id,status", "--ndjson"]);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        4,
        "one projected object per open item:\n{stdout}"
    );
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["id", "status"],
            "projection must apply per line:\n{line}"
        );
    }
}

#[test]
fn items_list_exclude_composes_with_ndjson() {
    let stdout = common::run_list_query(&["--status", "open", "--exclude", "status", "--ndjson"]);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("status").is_none(), "excluded field leaked:\n{line}");
        assert!(v.get("id").is_some());
    }
}

#[test]
fn find_duplicates_tier_is_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = seeded_ledger(dir.path());
    for tier in ["B", "b", "A", "c"] {
        tomlctl(dir.path())
            .args(["items", "find-duplicates"])
            .arg(&ledger)
            .args(["--tier", tier])
            .assert()
            .success();
    }
}

#[test]
fn find_duplicates_tier_possible_values_are_uppercase() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = seeded_ledger(dir.path());
    tomlctl(dir.path())
        .args(["items", "find-duplicates"])
        .arg(&ledger)
        .args(["--tier", "D"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("[possible values: A, B, C]"));
}
