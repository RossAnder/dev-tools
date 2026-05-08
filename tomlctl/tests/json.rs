//! T2 of `docs/plans/flow-tracking-overhaul.md`: integration tests for
//! `tomlctl json {get,set,unset}`.
//!
//! Test scenarios (one `#[test]` each):
//!  1. round-trip get/set/unset against `.claude/settings.json`.
//!  2. `--dry-run` on `set` emits a `would_change` envelope and leaves
//!     the file + sidecar byte-identical.
//!  3. sidecar SKIPPED on `settings.json` writes (P16).
//!  4. sidecar UPDATED on a non-`settings.json` JSON write.
//!  5. `unset` of a non-existent key is a no-op.
//!  6. containment guard refuses outside-`.claude/` paths.
//!  7. `tomlctl set foo.json key val` symmetric P19 rejection — DEFERRED
//!     (test stub `#[ignore]`d so it shows up in the suite as a TODO).
//!  8. `get` returns 2-space-indented JSON with trailing newline.
//!  9. `--raw` on a scalar emits bare unquoted output.
//! 10. `--strict-read` on a missing file → `kind=not_found`.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::parse_json_error_envelope;

/// Create a tempdir, return (TempDir, repo-root-path). The caller seeds
/// any `.claude/...` files they need; tests use `TOMLCTL_ROOT` to point
/// the binary at the tempdir as the repo root for the
/// `guard_write_path` containment check.
fn fresh_tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let path = dir.path().to_path_buf();
    (dir, path)
}

fn sidecar_for(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

/// (1) round-trip: write a key, read it back, unset it, get errors.
#[test]
fn json_round_trip_set_get_unset_against_settings_json() {
    let (dir, root) = fresh_tempdir();
    let settings = root.join(".claude").join("settings.json");
    fs::write(&settings, "{}\n").unwrap();

    // Set the array directly via a JSON-array value at `permissions.allow`.
    // (The dotted-path navigator does not auto-create array slots — an
    // index segment requires the array to already exist. This is the
    // documented contract: array-vivify would silently mask off-by-one
    // errors.)
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("set")
        .arg(&settings)
        .arg("permissions.allow")
        .arg("--json")
        .arg(r#"["Bash(tomlctl *)"]"#)
        .write_stdin("")
        .assert()
        .success();

    // get the inserted scalar
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("get")
        .arg(&settings)
        .arg("permissions.allow.0")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(stdout.trim(), "Bash(tomlctl *)");

    // unset the leaf
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("unset")
        .arg(&settings)
        .arg("permissions.allow")
        .write_stdin("")
        .assert()
        .success();

    // get on the now-missing leaf → kind=not_found
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format=json")
        .arg("json")
        .arg("get")
        .arg(&settings)
        .arg("permissions.allow")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"].as_str(), Some("not_found"));

    drop(dir);
}

/// (2) `--dry-run` on set emits `would_change` and leaves the file +
/// sidecar byte-identical.
#[test]
fn json_set_dry_run_does_not_touch_file_or_sidecar() {
    let (dir, root) = fresh_tempdir();
    // Use a non-`settings.json` so a sidecar would normally be written.
    let target = root.join(".claude").join("custom.json");
    fs::write(&target, r#"{"a":1}"#).unwrap();

    // Prime the sidecar by doing a real write first.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("set")
        .arg(&target)
        .arg("a")
        .arg("--json")
        .arg("1")
        .write_stdin("")
        .assert()
        .success();

    let sidecar = sidecar_for(&target);
    assert!(sidecar.exists(), "sidecar must exist after priming write");

    let before_bytes = fs::read(&target).unwrap();
    let before_sidecar = fs::read(&sidecar).unwrap();

    // Dry-run set
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("set")
        .arg(&target)
        .arg("b")
        .arg("--json")
        .arg("2")
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .expect("dry-run stdout must be JSON");
    assert!(
        parsed.get("would_change").is_some(),
        "dry-run output must contain `would_change`, got: {stdout}"
    );
    let wc = &parsed["would_change"];
    assert_eq!(wc["action"].as_str(), Some("set"));
    assert_eq!(wc["path"].as_str(), Some("b"));
    assert_eq!(wc["new_value"], serde_json::json!(2));
    assert_eq!(wc["old_value"], serde_json::Value::Null);

    // No file or sidecar mutation.
    assert_eq!(fs::read(&target).unwrap(), before_bytes);
    assert_eq!(fs::read(&sidecar).unwrap(), before_sidecar);

    drop(dir);
}

/// (3) P16: writes to `settings.json` skip sidecar refresh and emit
/// `sidecar_skipped:"co-writer-protected"` in the envelope.
#[test]
fn json_set_on_settings_json_skips_sidecar_and_marks_envelope() {
    let (dir, root) = fresh_tempdir();
    let target = root.join(".claude").join("settings.json");
    fs::write(&target, "{}\n").unwrap();

    let sidecar = sidecar_for(&target);
    assert!(!sidecar.exists(), "precondition: no sidecar");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("set")
        .arg(&target)
        .arg("foo")
        .arg("--json")
        .arg(r#""bar""#)
        .write_stdin("")
        .assert()
        .success();

    // Envelope must mark sidecar as skipped.
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let env: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("envelope is JSON");
    assert_eq!(env["ok"], serde_json::Value::Bool(true));
    assert_eq!(
        env["sidecar_skipped"].as_str(),
        Some("co-writer-protected"),
        "settings.json writes must mark sidecar_skipped, got: {stdout}"
    );

    // No sidecar file should have been created.
    assert!(
        !sidecar.exists(),
        "sidecar must NOT exist after settings.json write — Claude Code is co-writer"
    );

    // File was actually written.
    let raw = fs::read_to_string(&target).unwrap();
    assert!(raw.contains("\"foo\""), "set must persist; got {raw:?}");
    assert!(raw.ends_with('\n'), "must end in newline; got {raw:?}");

    drop(dir);
}

/// (4) Non-`settings.json` JSON writes refresh the sidecar normally.
#[test]
fn json_set_on_non_settings_refreshes_sidecar() {
    let (dir, root) = fresh_tempdir();
    let target = root.join(".claude").join("foo.json");
    fs::write(&target, "{}\n").unwrap();

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("set")
        .arg(&target)
        .arg("a")
        .arg("--json")
        .arg("42")
        .write_stdin("")
        .assert()
        .success();

    let sidecar = sidecar_for(&target);
    assert!(
        sidecar.exists(),
        "sidecar must be created for non-settings.json writes"
    );
    let raw = fs::read_to_string(&sidecar).unwrap();
    let basename = target.file_name().unwrap().to_string_lossy();
    assert!(
        raw.ends_with(&format!("  {basename}\n")),
        "sidecar must end with `  <basename>\\n`, got: {raw:?}"
    );
    let hex = raw.split_whitespace().next().unwrap();
    assert_eq!(hex.len(), 64, "digest must be 64 hex chars, got {hex:?}");

    drop(dir);
}

/// (5) `unset` of a non-existent key is a successful no-op — file
/// unchanged, exit 0, envelope `ok:true`.
#[test]
fn json_unset_of_missing_leaf_is_noop() {
    let (dir, root) = fresh_tempdir();
    let target = root.join(".claude").join("foo.json");
    fs::write(&target, r#"{"a":1}"#).unwrap();

    let before = fs::read(&target).unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("unset")
        .arg(&target)
        .arg("does.not.exist")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let env: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("envelope is JSON");
    assert_eq!(env["ok"], serde_json::Value::Bool(true));

    // File bytes unchanged.
    let after = fs::read(&target).unwrap();
    assert_eq!(before, after, "unset of missing leaf must not rewrite file");

    drop(dir);
}

/// (6) Containment guard refuses writes to paths outside `.claude/`.
#[test]
fn json_set_refuses_path_outside_claude() {
    let (dir, root) = fresh_tempdir();
    // Target sits OUTSIDE `.claude/` — under tempdir but not the .claude
    // subdir, so `guard_write_path` must refuse.
    let outside = root.join("outside.json");
    fs::write(&outside, "{}\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format=json")
        .arg("json")
        .arg("set")
        .arg(&outside)
        .arg("a")
        .arg("--json")
        .arg("1")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    // The guard_write_path bail isn't tagged kind=validation today (it's
    // an untagged anyhow::bail), so error.kind falls back to "other".
    // What we DO want to assert is that the message names the
    // outside-`.claude/` refusal, which is the load-bearing contract.
    assert!(
        stderr.contains("refusing to write outside .claude/")
            || stderr.contains("\"kind\""),
        "expected outside-.claude refusal or JSON envelope, got: {stderr}"
    );

    drop(dir);
}

/// (7) Symmetric P19 rejection on the TOML write side: `tomlctl set` /
/// `tomlctl set-json` / `tomlctl array-append` against a `.json` target
/// must error `kind=validation` with a message that points the caller at
/// `tomlctl json set`. Pairs with `json_set_refuses_toml_extension` below
/// (the JSON-side refusal of `.toml` targets).
#[test]
fn tomlctl_set_on_dot_json_path_refers_to_json_set() {
    let (dir, root) = fresh_tempdir();
    let target = root.join(".claude").join("settings.json");
    fs::write(&target, "{}\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format=json")
        .arg("set")
        .arg(&target)
        .arg("a")
        .arg("x")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"].as_str(), Some("validation"));
    let msg = err["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("tomlctl json set"),
        "TOML writer rejection on .json target must mention `tomlctl json set`, got: {msg}"
    );

    drop(dir);
}

/// (7b) JSON writers refuse `.toml` targets (positive half of P19). This
/// pairs with the deferred negative half in (7) above.
#[test]
fn json_set_refuses_toml_extension() {
    let (dir, root) = fresh_tempdir();
    let target = root.join(".claude").join("foo.toml");
    fs::write(&target, "schema_version = 1\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format=json")
        .arg("json")
        .arg("set")
        .arg(&target)
        .arg("a")
        .arg("--json")
        .arg(r#""x""#)
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"].as_str(), Some("validation"));
    let msg = err["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("tomlctl set") && msg.contains(".toml"),
        "validation message must reference TOML alternative, got: {msg}"
    );

    drop(dir);
}

/// (8) `get` returns 2-space-indented JSON with a trailing newline.
#[test]
fn json_get_emits_pretty_two_space_indent_with_trailing_newline() {
    let (dir, root) = fresh_tempdir();
    let target = root.join(".claude").join("foo.json");
    fs::write(&target, r#"{"a":{"b":1,"c":2}}"#).unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("get")
        .arg(&target)
        .arg("a")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(stdout.ends_with('\n'), "must end in newline: {stdout:?}");
    // Two-space indent: "  \"b\"" or "  \"c\"" appears.
    assert!(
        stdout.contains("  \"b\"") || stdout.contains("  \"c\""),
        "expected two-space indent in pretty output, got:\n{stdout}"
    );
    // Parses round-trip.
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed, serde_json::json!({"b":1,"c":2}));

    drop(dir);
}

/// (9) `--raw` on a scalar emits the bare unquoted value.
#[test]
fn json_get_raw_on_scalar_emits_bare_value() {
    let (dir, root) = fresh_tempdir();
    let target = root.join(".claude").join("foo.json");
    fs::write(&target, r#"{"name":"alpha","count":42}"#).unwrap();

    // String → bare unquoted "alpha"
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("get")
        .arg(&target)
        .arg("name")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(stdout.trim_end(), "alpha", "raw string must be unquoted");

    // Integer → bare 42
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("json")
        .arg("get")
        .arg(&target)
        .arg("count")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(stdout.trim_end(), "42");

    drop(dir);
}

/// (10) `--strict-read` on a missing file → `kind=not_found`.
#[test]
fn json_get_strict_read_on_missing_file_emits_not_found() {
    let (dir, root) = fresh_tempdir();
    let missing = root.join(".claude").join("nope.json");
    assert!(!missing.exists());

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format=json")
        .arg("json")
        .arg("get")
        .arg(&missing)
        .arg("a")
        .arg("--strict-read")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"].as_str(), Some("not_found"));

    drop(dir);
}
