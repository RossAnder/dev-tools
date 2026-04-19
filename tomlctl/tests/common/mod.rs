//! Shared helpers for the integration-test binaries under `tomlctl/tests/`.
//!
//! Split out of `integration.rs` (R23) so each topic-specific test binary
//! (`items_dry_run.rs`, `items_dedupe.rs`, `capabilities.rs`, `blocks.rs`,
//! and the leftover `integration.rs`) can share the tempdir/ledger
//! bootstrap, the list-query fixture, JSON error-envelope parsing, and the
//! sidecar-digest assertion without duplicating their bodies.
//!
//! Each test binary declares `mod common;` to pull this module in. Cargo
//! does NOT treat `tomlctl/tests/common/mod.rs` as its own test binary
//! (only top-level files under `tests/` are test binaries), so this file
//! is linked once per consumer without fanning out `#[test]` runs.
//!
//! Helpers are `#[allow(dead_code)]` because any given consumer may use
//! only a subset; without the attribute, unused-item warnings would fire
//! on every test binary that skips one of the helpers.

#![allow(dead_code)]

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

/// Create a tempdir, seed `<tempdir>/.claude/ledger.toml` with `initial`,
/// and return both the tempdir (RAII cleanup) and the ledger path.
pub fn seed_ledger(initial: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("ledger.toml");
    fs::write(&ledger, initial).unwrap();
    (dir, ledger)
}

/// Six-item fixture spanning status ∈ {open, fixed}, severity ∈ {minor,
/// major, critical}, file ∈ {src/a.rs, src/b.rs}, first_flagged in
/// 2026-03 and 2026-04, plus one row carrying `symbol = "old::fn"` for the
/// regex/presence tests. `first_flagged` is a TOML date literal so the
/// query engine's date comparison path is exercised end-to-end.
pub const QUERY_FIXTURE: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
severity = "minor"
file = "src/a.rs"
category = "style"
first_flagged = 2026-03-10
summary = "trailing whitespace"

[[items]]
id = "R2"
status = "open"
severity = "major"
file = "src/a.rs"
category = "bug"
first_flagged = 2026-04-02
summary = "nil deref"
symbol = "old::fn"

[[items]]
id = "R3"
status = "fixed"
severity = "critical"
file = "src/b.rs"
category = "bug"
first_flagged = 2026-03-25
summary = "panic on empty input"

[[items]]
id = "R4"
status = "open"
severity = "major"
file = "src/b.rs"
category = "perf"
first_flagged = 2026-04-10
summary = "n^2 loop"

[[items]]
id = "R5"
status = "fixed"
severity = "minor"
file = "src/a.rs"
category = "style"
first_flagged = 2026-04-05
summary = "unused import"

[[items]]
id = "R6"
status = "open"
severity = "critical"
file = "src/b.rs"
category = "security"
first_flagged = 2026-04-15
summary = "unsafe block"
"#;

/// Seed the 6-row `QUERY_FIXTURE` and run `tomlctl items list <ledger> <args…>`.
/// Returns stdout as a String. Used by the query-shape suite.
pub fn run_list_query(args: &[&str]) -> String {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

/// Same as [`run_list_query`] but the caller supplies the fixture bytes.
/// Used where the 6-row default would bias the result (e.g. single-row
/// pluck tests that assert scalar cardinality).
pub fn run_list_query_with(fixture: &str, args: &[&str]) -> String {
    let (dir, ledger) = seed_ledger(fixture);
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

/// Extract sorted `id` values from a JSON-array list output. Used by the
/// `where`-predicate suite where the canonical check is "which rows
/// survived the predicate".
pub fn ids_from(stdout: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("list output is a JSON array");
    let mut ids: Vec<String> = arr
        .iter()
        .map(|el| el.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string())
        .collect();
    ids.sort();
    ids
}

/// Verify the `<ledger>.sha256` sidecar exists, ends with `  <basename>\n`,
/// carries a 64-hex-char digest, and that digest matches a fresh recompute
/// of the live ledger bytes.
pub fn assert_sidecar_matches(ledger: &Path) {
    let sidecar: PathBuf = {
        let mut s = ledger.as_os_str().to_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    assert!(
        sidecar.exists(),
        "sidecar must exist at {}",
        sidecar.display()
    );
    let raw = fs::read_to_string(&sidecar).unwrap();
    let basename = ledger.file_name().unwrap().to_string_lossy();
    assert!(
        raw.ends_with(&format!("  {basename}\n")),
        "sidecar must end with `  <basename>\\n`, got: {raw:?}"
    );
    let hex = raw.split_whitespace().next().unwrap();
    assert_eq!(hex.len(), 64, "digest must be 64 hex chars, got {hex:?}");
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    // Recompute against the live bytes and compare.
    let bytes = fs::read(ledger).unwrap();
    use sha2::{Digest, Sha256};
    let actual = Sha256::digest(&bytes);
    let mut actual_hex = String::with_capacity(64);
    for b in actual.iter() {
        use std::fmt::Write;
        let _ = write!(actual_hex, "{:02x}", b);
    }
    assert_eq!(
        hex.to_ascii_lowercase(),
        actual_hex,
        "sidecar digest must match recomputed file hash"
    );
}

/// Parse a single-line JSON error envelope from `stderr`. Asserts the
/// envelope has a top-level `error` object carrying `kind`, `message`,
/// and `file` keys (the latter nullable). Returns the inner `error`
/// object so callers can assert on specific fields.
pub fn parse_json_error_envelope(stderr: &str) -> serde_json::Value {
    let line = stderr.trim();
    assert!(
        !line.is_empty(),
        "stderr is empty — expected a JSON error envelope"
    );
    let v: serde_json::Value =
        serde_json::from_str(line).expect("stderr must be a single JSON line");
    let err = v
        .get("error")
        .cloned()
        .expect("envelope must have top-level `error` key");
    assert!(
        err.get("kind").and_then(|k| k.as_str()).is_some(),
        "error.kind must be a string, got: {err}"
    );
    assert!(
        err.get("message").and_then(|m| m.as_str()).is_some(),
        "error.message must be a string, got: {err}"
    );
    assert!(
        err.get("file").is_some(),
        "error.file key must always be present (null when unknown), got: {err}"
    );
    err
}
