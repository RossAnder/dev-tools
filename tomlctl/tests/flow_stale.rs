//! Integration tests for `tomlctl flow stale` (T5 of
//! `docs/plans/flow-tracking-overhaul.md`).
//!
//! Each test materialises a temp `<root>/.claude/flows/<slug>/context.toml`
//! (or omits it, for the missing-file paths) and runs the CLI through
//! `assert_cmd`, asserting on the JSON staleness envelope.

use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;

mod common;
use common::parse_json_error_envelope;

/// Build `<root>/.claude/flows/<slug>/` and write `context.toml` with the
/// given body. Returns `(tempdir, root_path, context_path)`. The caller
/// passes `root_path` to `TOMLCTL_ROOT` so `tomlctl flow stale --slug`
/// resolves to the staged file.
fn seed_flow(slug: &str, body: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let flow_dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&flow_dir).unwrap();
    let context = flow_dir.join("context.toml");
    fs::write(&context, body).unwrap();
    (dir, root, context)
}

/// Same as [`seed_flow`] but does NOT create `context.toml`. Used by the
/// missing-file paths.
fn seed_empty_flow(slug: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let flow_dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&flow_dir).unwrap();
    let context = flow_dir.join("context.toml");
    (dir, root, context)
}

/// Today's UTC date as `YYYY-MM-DD`. Tests that assert on age in seconds
/// derive their expected values from this so they're stable across runs
/// regardless of when CI fires.
fn today_utc_iso() -> String {
    use jiff::Timestamp;
    Timestamp::now()
        .in_tz("UTC")
        .unwrap()
        .strftime("%Y-%m-%d")
        .to_string()
}

/// `YYYY-MM-DD` for `today - n_days`. Used to seed "old" flows whose age
/// the staleness verdict must report.
fn iso_days_ago(n: i64) -> String {
    use jiff::Timestamp;
    use jiff::ToSpan;
    let today = Timestamp::now().in_tz("UTC").unwrap().date();
    let past = today.checked_sub(n.days()).unwrap();
    past.strftime("%Y-%m-%d").to_string()
}

/// Acceptance: a fresh flow whose `updated` is today reports stale=false
/// with reason "updated within threshold" under the default 7d threshold.
#[test]
fn flow_stale_fresh_flow_reports_not_stale() {
    let body = format!(
        r#"schema_version = 1
slug = "fresh"
created = {today}
updated = {today}
"#,
        today = today_utc_iso()
    );
    let (_dir, root, _ctx) = seed_flow("fresh", &body);

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("fresh")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(v["stale"], serde_json::json!(false));
    assert_eq!(v["reason"], serde_json::json!("updated within threshold"));
    assert_eq!(v["age_seconds"], serde_json::json!(0));
    let last = v["last_activity"].as_str().expect("last_activity present");
    assert!(
        last.starts_with(&today_utc_iso()) && last.ends_with("T00:00:00Z"),
        "last_activity must be `<today>T00:00:00Z`, got: {last}"
    );
}

/// Acceptance: an 8-day-old flow under the default `7d` threshold reads as
/// stale with reason "updated > 7d ago".
#[test]
fn flow_stale_old_flow_default_threshold_reports_stale() {
    let body = format!(
        r#"schema_version = 1
slug = "old"
updated = {past}
"#,
        past = iso_days_ago(8)
    );
    let (_dir, root, _ctx) = seed_flow("old", &body);

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("old")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(v["stale"], serde_json::json!(true));
    assert_eq!(v["reason"], serde_json::json!("updated > 7d ago"));
    assert_eq!(v["age_seconds"], serde_json::json!(8 * 86_400));
}

/// Acceptance: missing `context.toml` (no `--strict-read`) is a meaningful
/// "stale=true" answer, NOT an error. Reason is `"context.toml missing"`,
/// `last_activity` and `age_seconds` are JSON null.
#[test]
fn flow_stale_missing_context_default_returns_stale_true_no_error() {
    let (_dir, root, _ctx) = seed_empty_flow("ghost");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("ghost")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(v["stale"], serde_json::json!(true));
    assert_eq!(v["reason"], serde_json::json!("context.toml missing"));
    assert!(
        v["last_activity"].is_null(),
        "last_activity must be null on missing-file"
    );
    assert!(
        v["age_seconds"].is_null(),
        "age_seconds must be null on missing-file"
    );
}

/// Acceptance: `--strict-read` on a missing `context.toml` flips the
/// behaviour to a hard `kind=not_found` error.
#[test]
fn flow_stale_missing_context_strict_read_errors_with_not_found() {
    let (_dir, root, ctx) = seed_empty_flow("ghost");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("ghost")
        .arg("--strict-read")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("not_found"));
    let file_field = err["file"].as_str().expect("file populated");
    assert!(
        file_field.contains("context.toml"),
        "error.file must point at the missing context.toml; got: {file_field}"
    );
    // The error message echoes the absolute path; sanity-check the leaf.
    let msg = err["message"].as_str().expect("message populated");
    assert!(
        msg.contains("file does not exist"),
        "error.message must carry the strict-read prose; got: {msg}"
    );
    // And confirm the file really wasn't created (no test-side surprise).
    assert!(!ctx.exists());
}

/// Acceptance: `context.toml` exists but has no `updated` field — reason is
/// `"updated field missing"`, stale=true, last_activity/age_seconds null.
#[test]
fn flow_stale_updated_field_missing_returns_stale_with_reason() {
    let body = r#"schema_version = 1
slug = "no-updated"
"#;
    let (_dir, root, _ctx) = seed_flow("no-updated", body);

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("no-updated")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(v["stale"], serde_json::json!(true));
    assert_eq!(v["reason"], serde_json::json!("updated field missing"));
    assert!(v["last_activity"].is_null());
    assert!(v["age_seconds"].is_null());
}

/// Acceptance: `--threshold 1d` against a 2-day-old flow reads as stale.
/// Pins the custom-threshold path through the parser → comparator chain.
#[test]
fn flow_stale_custom_threshold_honoured() {
    let body = format!(
        r#"schema_version = 1
slug = "two-day-old"
updated = {past}
"#,
        past = iso_days_ago(2)
    );
    let (_dir, root, _ctx) = seed_flow("two-day-old", &body);

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("two-day-old")
        .arg("--threshold")
        .arg("1d")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(v["stale"], serde_json::json!(true));
    assert_eq!(v["reason"], serde_json::json!("updated > 1d ago"));
    assert_eq!(v["age_seconds"], serde_json::json!(2 * 86_400));
}

/// Acceptance: the threshold parser accepts every documented suffix
/// (`s`, `m`, `h`, `d`, `w`). End-to-end probe: each threshold passes
/// without a parse error against an arbitrary fresh flow. Bare numbers
/// and unknown suffixes are rejected with `kind=validation`.
#[test]
fn flow_stale_threshold_parser_accepts_all_documented_forms() {
    let body = format!(
        r#"schema_version = 1
updated = {today}
"#,
        today = today_utc_iso()
    );
    let (_dir, root, _ctx) = seed_flow("ts", &body);

    for good in ["7d", "48h", "1w", "60m", "300s"] {
        Command::cargo_bin("tomlctl")
            .unwrap()
            .env("TOMLCTL_ROOT", &root)
            .arg("flow")
            .arg("stale")
            .arg("--slug")
            .arg("ts")
            .arg("--threshold")
            .arg(good)
            .write_stdin("")
            .assert()
            .success();
    }
}

/// Acceptance: an invalid threshold (`5x`) errors at validation time with
/// `kind=validation`. Stderr surfaces the load-bearing prose
/// `"invalid threshold: 5x"`.
#[test]
fn flow_stale_threshold_parser_rejects_unknown_suffix() {
    let body = format!(
        r#"schema_version = 1
updated = {today}
"#,
        today = today_utc_iso()
    );
    let (_dir, root, _ctx) = seed_flow("ts", &body);

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("ts")
        .arg("--threshold")
        .arg("5x")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("validation"));
    let msg = err["message"].as_str().expect("message populated");
    assert!(
        msg.contains("invalid threshold: 5x"),
        "error.message must echo the bad input; got: {msg}"
    );
}

/// Acceptance: JSON envelope shape is stable. Top-level keys are exactly
/// `stale`, `last_activity`, `age_seconds`, `reason` — no extras, no missing.
/// Pinned across both the in-threshold and out-of-threshold branches so a
/// future refactor can't drop a key on one path while keeping it on the other.
#[test]
fn flow_stale_json_envelope_keys_are_stable() {
    // Branch A: fresh flow → in-threshold.
    let body_fresh = format!(
        r#"schema_version = 1
updated = {today}
"#,
        today = today_utc_iso()
    );
    let (_d, root, _c) = seed_flow("fresh", &body_fresh);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("fresh")
        .write_stdin("")
        .assert()
        .success();
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.get_output().stdout)).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["age_seconds", "last_activity", "reason", "stale"]
    );

    // Branch B: 9-day-old flow → out-of-threshold.
    let body_old = format!(
        r#"schema_version = 1
updated = {past}
"#,
        past = iso_days_ago(9)
    );
    let (_d2, root2, _c2) = seed_flow("old", &body_old);
    let out2 = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root2)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("old")
        .write_stdin("")
        .assert()
        .success();
    let v2: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out2.get_output().stdout)).unwrap();
    let mut keys2: Vec<&str> = v2.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys2.sort();
    assert_eq!(
        keys2,
        vec!["age_seconds", "last_activity", "reason", "stale"]
    );

    // Branch C: missing context.toml (still emits the full key set, with
    // last_activity/age_seconds as JSON null).
    let (_d3, root3, _c3) = seed_empty_flow("ghost");
    let out3 = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root3)
        .arg("flow")
        .arg("stale")
        .arg("--slug")
        .arg("ghost")
        .write_stdin("")
        .assert()
        .success();
    let v3: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out3.get_output().stdout)).unwrap();
    let mut keys3: Vec<&str> = v3.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys3.sort();
    assert_eq!(
        keys3,
        vec!["age_seconds", "last_activity", "reason", "stale"]
    );
}
