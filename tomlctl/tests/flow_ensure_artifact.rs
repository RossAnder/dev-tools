//! T8: integration tests for `tomlctl flow ensure-artifact` (per
//! `docs/plans/flow-tracking-overhaul.md` Task 8).
//!
//! Each test materialises a sandbox tempdir at `<tmp>/.claude/flows/<slug>/`,
//! optionally seeds the artifact + sidecar, and runs the built `tomlctl`
//! binary via `assert_cmd` with `TOMLCTL_ROOT` pointed at the tempdir.

use assert_cmd::Command;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::parse_json_error_envelope;

/// Build `<tmp>/.claude/flows/<slug>/` and return `(tempdir, root, flow_dir)`.
fn seed_flow_dir(slug: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let flow_dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&flow_dir).unwrap();
    (dir, root, flow_dir)
}

/// Write `body` as the artifact and a matching `<file>.sha256` sidecar in
/// the standard `<hex>  <basename>\n` format.
fn write_artifact_with_sidecar(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let digest = Sha256::digest(body.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", b);
    }
    let basename = path.file_name().unwrap().to_string_lossy();
    let sidecar = sidecar_path(path);
    fs::write(sidecar, format!("{hex}  {basename}\n")).unwrap();
}

/// `<file>.sha256`. Mirrors `integrity::sidecar_path` so tests don't have
/// to import a private helper.
fn sidecar_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

/// Run `tomlctl flow ensure-artifact <args>` against `root` and parse the
/// stdout as JSON. Asserts process success.
fn run_ensure(root: &Path, args: &[&str]) -> JsonValue {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("ensure-artifact");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON; err={e}; stdout:\n{stdout}"))
}

/// Same as `run_ensure` but expects a non-zero exit. Returns stderr.
fn run_ensure_failing(root: &Path, args: &[&str]) -> String {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("ensure-artifact");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().failure();
    String::from_utf8_lossy(&out.get_output().stderr).to_string()
}

// ---------------------------------------------------------------------------
// Acceptance: required tests per task spec.
// ---------------------------------------------------------------------------

/// present-and-valid: an artifact whose sidecar matches its content
/// reports `exists:true, sidecar_valid:true` and never mutates disk.
#[test]
fn ensure_artifact_present_and_valid_reports_true() {
    let (_g, root, flow_dir) = seed_flow_dir("present");
    let artifact = flow_dir.join("execution-record.toml");
    write_artifact_with_sidecar(&artifact, "schema_version = 1\nlast_updated = 2026-05-08\n");
    // Pre-flight pin: both files exist before the call.
    assert!(artifact.exists());
    assert!(sidecar_path(&artifact).exists());

    let v = run_ensure(
        &root,
        &["--slug", "present", "--kind", "execution-record"],
    );
    assert_eq!(v["exists"], JsonValue::Bool(true));
    assert_eq!(v["sidecar_valid"], JsonValue::Bool(true));
    let path = v["path"].as_str().expect("path key");
    assert!(
        path.ends_with("execution-record.toml") && path.contains("present"),
        "path must point at the slug's execution-record: {path}"
    );
    let sp = v["sidecar_path"].as_str().expect("sidecar_path key");
    assert!(sp.ends_with("execution-record.toml.sha256"), "got: {sp}");
}

/// missing artifact (no `--bootstrap`): returns `exists:false,
/// sidecar_valid:null`. No file created.
#[test]
fn ensure_artifact_missing_reports_false_with_null_sidecar() {
    let (_g, root, flow_dir) = seed_flow_dir("ghost");
    let artifact = flow_dir.join("execution-record.toml");
    assert!(!artifact.exists(), "pre-flight: artifact must be absent");

    let v = run_ensure(&root, &["--slug", "ghost", "--kind", "execution-record"]);
    assert_eq!(v["exists"], JsonValue::Bool(false));
    assert!(
        v["sidecar_valid"].is_null(),
        "sidecar_valid must be null when the artifact is missing; got: {}",
        v["sidecar_valid"]
    );

    // No write side-effect.
    assert!(!artifact.exists(), "report path must not create the artifact");
    assert!(!sidecar_path(&artifact).exists(), "report path must not write the sidecar");
}

/// tampered sidecar (artifact present, sidecar digest mismatch):
/// `sidecar_valid:false`. No auto-repair.
#[test]
fn ensure_artifact_tampered_sidecar_reports_false_no_repair() {
    let (_g, root, flow_dir) = seed_flow_dir("tampered");
    let artifact = flow_dir.join("execution-record.toml");
    fs::write(&artifact, "schema_version = 1\nlast_updated = 2026-05-08\n").unwrap();
    // Hand-write a sidecar with an obviously wrong digest.
    fs::write(
        sidecar_path(&artifact),
        "0000000000000000000000000000000000000000000000000000000000000000  execution-record.toml\n",
    )
    .unwrap();

    let pre_sidecar = fs::read_to_string(sidecar_path(&artifact)).unwrap();
    let v = run_ensure(
        &root,
        &["--slug", "tampered", "--kind", "execution-record"],
    );
    assert_eq!(v["exists"], JsonValue::Bool(true));
    assert_eq!(v["sidecar_valid"], JsonValue::Bool(false));

    // No auto-repair: sidecar bytes are unchanged after the report call.
    let post_sidecar = fs::read_to_string(sidecar_path(&artifact)).unwrap();
    assert_eq!(
        pre_sidecar, post_sidecar,
        "report path must not auto-refresh a mismatched sidecar"
    );
}

/// `--bootstrap` for `kind=execution-record` materialises both file +
/// sidecar atomically, with the documented 2-line body.
#[test]
fn bootstrap_execution_record_materialises_file_and_sidecar() {
    let (_g, root, flow_dir) = seed_flow_dir("boot");
    let artifact = flow_dir.join("execution-record.toml");
    let sidecar = sidecar_path(&artifact);
    assert!(!artifact.exists());
    assert!(!sidecar.exists());

    let v = run_ensure(
        &root,
        &[
            "--slug",
            "boot",
            "--kind",
            "execution-record",
            "--bootstrap",
        ],
    );
    assert_eq!(v["exists"], JsonValue::Bool(true));
    assert_eq!(v["sidecar_valid"], JsonValue::Bool(true));
    assert_eq!(
        v["bootstrapped"],
        JsonValue::Bool(true),
        "bootstrapped marker must surface on the success path"
    );

    // File body matches the documented 2-line bootstrap shape (date allowed
    // to be any current ISO date — we pin schema_version + last_updated key
    // and a YYYY-MM-DD value).
    let body = fs::read_to_string(&artifact).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2, "bootstrap body must be exactly 2 lines: {body:?}");
    assert_eq!(lines[0], "schema_version = 1");
    assert!(
        lines[1].starts_with("last_updated = ") && lines[1].len() == "last_updated = 2026-05-08".len(),
        "second line must be `last_updated = YYYY-MM-DD`, got: {:?}",
        lines[1]
    );

    // Sidecar exists with a 64-hex digest matching the file bytes.
    let sc = fs::read_to_string(&sidecar).unwrap();
    let basename = artifact.file_name().unwrap().to_string_lossy();
    assert!(
        sc.ends_with(&format!("  {basename}\n")),
        "sidecar must end with `  <basename>\\n`: {sc:?}"
    );
    let hex = sc.split_whitespace().next().unwrap();
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    let expected = Sha256::digest(body.as_bytes());
    let mut want = String::with_capacity(64);
    for b in expected.iter() {
        use std::fmt::Write;
        let _ = write!(want, "{:02x}", b);
    }
    assert_eq!(hex.to_ascii_lowercase(), want, "sidecar digest must match body");
}

/// `--bootstrap --dry-run` for execution-record emits a `would_change`
/// plan and does NOT write anything.
#[test]
fn bootstrap_execution_record_dry_run_emits_plan_no_write() {
    let (_g, root, flow_dir) = seed_flow_dir("dry");
    let artifact = flow_dir.join("execution-record.toml");
    let sidecar = sidecar_path(&artifact);
    assert!(!artifact.exists());

    let v = run_ensure(
        &root,
        &[
            "--slug",
            "dry",
            "--kind",
            "execution-record",
            "--bootstrap",
            "--dry-run",
        ],
    );
    assert_eq!(v["ok"], JsonValue::Bool(true));
    assert_eq!(v["dry_run"], JsonValue::Bool(true));
    let wc = &v["would_change"];
    assert_eq!(wc["kind"], JsonValue::String("ensure-artifact".to_string()));
    assert_eq!(wc["action"], JsonValue::String("bootstrap".to_string()));
    assert_eq!(
        wc["artifact_kind"],
        JsonValue::String("execution-record".to_string())
    );

    // No FS side-effects.
    assert!(!artifact.exists(), "dry-run must not write the artifact");
    assert!(!sidecar.exists(), "dry-run must not write the sidecar");
}

/// `--bootstrap` for `kind=review-ledger` is a no-op (no file created),
/// returns `bootstrap_noop` marker.
#[test]
fn bootstrap_review_ledger_is_noop_with_marker() {
    let (_g, root, flow_dir) = seed_flow_dir("noop");
    let artifact = flow_dir.join("review-ledger.toml");
    assert!(!artifact.exists());

    let v = run_ensure(
        &root,
        &["--slug", "noop", "--kind", "review-ledger", "--bootstrap"],
    );
    // Read-only report fields still present.
    assert_eq!(v["exists"], JsonValue::Bool(false));
    assert!(v["sidecar_valid"].is_null());
    // Bootstrap-noop marker carries the expected prose.
    let marker = v["bootstrap_noop"]
        .as_str()
        .expect("bootstrap_noop must be a string");
    assert!(
        marker.contains("review-ledger") && marker.contains("owning command"),
        "marker prose should identify the kind + responsibility, got: {marker}"
    );

    // Hard guarantee: no file created.
    assert!(!artifact.exists(), "review-ledger bootstrap must not create the file");
    assert!(
        !sidecar_path(&artifact).exists(),
        "review-ledger bootstrap must not create a sidecar"
    );
}

/// containment guard: targeting a slug that resolves outside `.claude/`
/// errors `kind=validation`.
#[test]
fn bad_slug_with_traversal_errors_validation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join(".claude")).unwrap();

    let stderr = run_ensure_failing(
        &root,
        &[
            "--slug",
            "../escape",
            "--kind",
            "execution-record",
            "--error-format",
            "json",
        ],
    );
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], JsonValue::String("validation".to_string()));
    let msg = err["message"].as_str().expect("message populated");
    assert!(
        msg.contains("invalid slug") || msg.contains("..") || msg.contains("path separator"),
        "validation prose must identify the bad slug; got: {msg}"
    );
}

/// Bootstrap on an already-existing execution-record is idempotent — no
/// rewrite, no sidecar bump, just the read-only report.
#[test]
fn bootstrap_execution_record_idempotent_when_present() {
    let (_g, root, flow_dir) = seed_flow_dir("idem");
    let artifact = flow_dir.join("execution-record.toml");
    write_artifact_with_sidecar(&artifact, "schema_version = 1\nlast_updated = 2026-05-08\n");
    let pre_body = fs::read(&artifact).unwrap();
    let pre_sidecar = fs::read(sidecar_path(&artifact)).unwrap();

    let v = run_ensure(
        &root,
        &[
            "--slug",
            "idem",
            "--kind",
            "execution-record",
            "--bootstrap",
        ],
    );
    assert_eq!(v["exists"], JsonValue::Bool(true));
    assert_eq!(v["sidecar_valid"], JsonValue::Bool(true));
    // Idempotent path does NOT carry the bootstrapped flag — that's
    // reserved for the case where this call materialised the file.
    assert!(
        v.get("bootstrapped").is_none() || v["bootstrapped"] == JsonValue::Null,
        "idempotent re-bootstrap must not advertise `bootstrapped:true`"
    );

    // Bytes preserved.
    assert_eq!(fs::read(&artifact).unwrap(), pre_body);
    assert_eq!(fs::read(sidecar_path(&artifact)).unwrap(), pre_sidecar);
}

/// JSON shape of the read-only report: exactly the four documented keys,
/// no extras, no missing — pinned across the present/missing branches so a
/// future refactor can't drop a key on one path while keeping it on the
/// other.
#[test]
fn report_envelope_keys_are_stable() {
    // Branch A: missing.
    let (_g1, root1, _) = seed_flow_dir("a");
    let v1 = run_ensure(&root1, &["--slug", "a", "--kind", "context"]);
    let mut keys1: Vec<&str> = v1.as_object().unwrap().keys().map(String::as_str).collect();
    keys1.sort();
    assert_eq!(
        keys1,
        vec!["exists", "path", "sidecar_path", "sidecar_valid"],
        "missing-branch envelope must carry exactly the four documented keys"
    );

    // Branch B: present + valid sidecar.
    let (_g2, root2, flow_dir2) = seed_flow_dir("b");
    let artifact = flow_dir2.join("context.toml");
    write_artifact_with_sidecar(&artifact, "schema_version = 1\n");
    let v2 = run_ensure(&root2, &["--slug", "b", "--kind", "context"]);
    let mut keys2: Vec<&str> = v2.as_object().unwrap().keys().map(String::as_str).collect();
    keys2.sort();
    assert_eq!(
        keys2,
        vec!["exists", "path", "sidecar_path", "sidecar_valid"],
        "present-branch envelope must carry the same key set"
    );
}
