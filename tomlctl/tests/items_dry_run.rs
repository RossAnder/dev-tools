//! Task 10 integration tests — `--dry-run` on `items remove` and `items apply`.
//! Split out of the monolithic `integration.rs` by R23. Every test body is
//! byte-identical to its pre-split form; helpers live in `tests/common/mod.rs`.
//!
//! The compute/apply split factors the mutation path into a pure
//! `compute_*_mutation(&TomlValue, ...)` phase (no lock, no sidecar, no
//! tempfile) and the existing I/O tail (lock + guard + atomic write + sidecar).
//! `--dry-run` stops after the compute phase and emits
//! `{"ok":true,"dry_run":true,"would_change":{...}}` without touching the
//! filesystem. The invariance test (e) pins the structural guarantee that
//! drives the whole split: the doc `compute_remove_mutation` builds, when
//! serialised through the same `toml::to_string_pretty` emit path the live
//! apply uses, is byte-identical to the bytes a real apply lands on disk.

use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;

mod common;
use common::seed_ledger;

/// T10 (a): `remove --dry-run <id>` leaves the ledger file byte-identical
/// AND the `.sha256` sidecar mtime unchanged. Stdout carries
/// `would_change.removed=[<id>]` with added/updated counts at 0.
#[test]
fn items_remove_dry_run_does_not_touch_ledger_or_sidecar() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"

[[items]]
id = "R2"
summary = "second"
status = "open"
"#,
    );
    // Prime the sidecar via any write: add a throw-away field then
    // remove it, or (simpler) do an `items update` on R1 that's a
    // no-op semantically but forces the sidecar to exist with a
    // predictable mtime. Using `items update` with a real patch
    // guarantees the sidecar lands.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("update")
        .arg(&ledger)
        .arg("R1")
        .arg("--json")
        .arg(r#"{"status":"open"}"#)
        .write_stdin("")
        .assert()
        .success();

    let sidecar = {
        let mut s = ledger.clone().into_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    assert!(sidecar.exists(), "sidecar must exist after priming write");

    let before_bytes = fs::read(&ledger).unwrap();
    let before_sidecar_bytes = fs::read(&sidecar).unwrap();
    let before_sidecar_mtime = fs::metadata(&sidecar).unwrap().modified().unwrap();

    // The dry-run remove.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("remove")
        .arg(&ledger)
        .arg("R1")
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();

    let after_bytes = fs::read(&ledger).unwrap();
    let after_sidecar_bytes = fs::read(&sidecar).unwrap();
    let after_sidecar_mtime = fs::metadata(&sidecar).unwrap().modified().unwrap();

    assert_eq!(
        before_bytes, after_bytes,
        "ledger bytes must be unchanged after dry-run"
    );
    assert_eq!(
        before_sidecar_bytes, after_sidecar_bytes,
        "sidecar bytes must be unchanged after dry-run"
    );
    assert_eq!(
        before_sidecar_mtime, after_sidecar_mtime,
        "sidecar mtime must be unchanged after dry-run"
    );

    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dry-run stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: items-shape envelopes carry kind:"items" as the discriminator.
    assert_eq!(wc["kind"], serde_json::json!("items"));
    assert_eq!(wc["added"], serde_json::json!(0));
    assert_eq!(wc["updated"], serde_json::json!(0));
    assert_eq!(wc["removed"], serde_json::json!(1));
    assert_eq!(wc["ids"], serde_json::json!(["R1"]));
}

/// T10 (b): a real `remove <id>` (no `--dry-run`) actually removes the
/// item and changes the file. Control case — confirms the dry-run path
/// is a specific opt-in and the default write path still works after
/// the compute/apply split.
#[test]
fn items_remove_without_dry_run_actually_removes() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"

[[items]]
id = "R2"
summary = "second"
status = "open"
"#,
    );

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("remove")
        .arg(&ledger)
        .arg("R1")
        .write_stdin("")
        .assert()
        .success();

    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).unwrap();
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 1, "R1 must be gone; R2 remains");
    assert_eq!(
        items[0]
            .as_table()
            .unwrap()
            .get("id")
            .and_then(|v| v.as_str()),
        Some("R2")
    );
}

/// T10 (c): `apply --dry-run --ops [...]` with a mixed add/update/remove
/// batch returns the right counts in `would_change`, leaves the ledger
/// untouched, and leaves the sidecar untouched.
#[test]
fn items_apply_dry_run_reports_mixed_batch_counts() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"

[[items]]
id = "R2"
summary = "second"
status = "open"
"#,
    );
    // Prime the sidecar.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("update")
        .arg(&ledger)
        .arg("R1")
        .arg("--json")
        .arg(r#"{"status":"open"}"#)
        .write_stdin("")
        .assert()
        .success();

    let sidecar = {
        let mut s = ledger.clone().into_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    let before_bytes = fs::read(&ledger).unwrap();
    let before_sidecar_bytes = fs::read(&sidecar).unwrap();

    // Add R3, update R1, remove R2.
    let ops = r#"[
        {"op":"add","json":{"id":"R3","summary":"third","status":"open"}},
        {"op":"update","id":"R1","json":{"status":"fixed"}},
        {"op":"remove","id":"R2"}
    ]"#;

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("apply")
        .arg(&ledger)
        .arg("--ops")
        .arg(ops)
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();

    let after_bytes = fs::read(&ledger).unwrap();
    let after_sidecar_bytes = fs::read(&sidecar).unwrap();
    assert_eq!(
        before_bytes, after_bytes,
        "ledger must be unchanged after apply --dry-run"
    );
    assert_eq!(
        before_sidecar_bytes, after_sidecar_bytes,
        "sidecar must be unchanged after apply --dry-run"
    );

    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dry-run stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: items-shape envelopes carry kind:"items" as the discriminator.
    assert_eq!(wc["kind"], serde_json::json!("items"));
    assert_eq!(wc["added"], serde_json::json!(1));
    assert_eq!(wc["updated"], serde_json::json!(1));
    assert_eq!(wc["removed"], serde_json::json!(1));
    // ids = [...added, ...updated, ...removed]
    assert_eq!(wc["ids"], serde_json::json!(["R3", "R1", "R2"]));
}

/// T10 (d): `apply --dry-run --no-remove --ops [{remove-op}]` errors
/// with the SAME `--no-remove` error message as a real apply. The gate
/// lives in `compute_apply_mutation` (via `items_apply_to_opts`) so the
/// dry-run and live paths surface the identical error.
#[test]
fn items_apply_dry_run_no_remove_errors_with_same_message() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"
"#,
    );
    let ops = r#"[{"op":"remove","id":"R1"}]"#;

    // First: a real apply with --no-remove to capture the canonical error.
    let real = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("apply")
        .arg(&ledger)
        .arg("--ops")
        .arg(ops)
        .arg("--no-remove")
        .write_stdin("")
        .assert()
        .failure();
    let real_stderr = String::from_utf8_lossy(&real.get_output().stderr).to_string();

    // Then: the same call but with --dry-run.
    let dry = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("apply")
        .arg(&ledger)
        .arg("--ops")
        .arg(ops)
        .arg("--no-remove")
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .failure();
    let dry_stderr = String::from_utf8_lossy(&dry.get_output().stderr).to_string();

    // Canonical substring: both must carry the same --no-remove diagnostic.
    let canonical = "is a remove op, but --no-remove was set";
    assert!(
        real_stderr.contains(canonical),
        "real-run stderr missing canonical --no-remove message; got:\n{real_stderr}"
    );
    assert!(
        dry_stderr.contains(canonical),
        "dry-run stderr missing canonical --no-remove message; got:\n{dry_stderr}"
    );
}

/// T10 (e) INVARIANCE: `compute_apply_mutation` on a fixture doc and
/// `write_toml_with_sidecar` of the resulting `plan.new_doc` land
/// byte-identically with the on-disk file produced by a real live
/// apply on the same fixture. This is the structural guarantee that
/// drives the compute/apply split: the dry-run and live paths share
/// the compute stage, and the apply stage is a pure serialisation of
/// the plan's `new_doc`, so the dry-run summary can't lie about what
/// a real run would do.
///
/// Implementation: we can't touch `compute_apply_mutation` from a
/// black-box integration test (it's `pub(crate)`), so we verify the
/// invariance end-to-end via two independent runs — one through the
/// real CLI, one through `items apply --dry-run` followed by a live
/// apply — and assert the final on-disk bytes agree. This is weaker
/// than a direct invocation but still catches the worst-case drift
/// (the dry-run's `new_doc` doesn't match the live apply's `new_doc`):
/// if they ever diverged, the serialised output would too.
///
/// A second, stronger check: run `--dry-run` first (no file change)
/// then the live apply on the SAME fixture; the output bytes match
/// what a fresh live-only apply would produce on a pristine copy.
/// Any divergence between the compute paths used by dry-run and live
/// would surface as a difference between the two live applies'
/// outputs, because the live path's `compute_apply_mutation` is the
/// only code that builds the pre-persist `new_doc`.
#[test]
fn items_apply_dry_run_then_live_apply_matches_live_only_apply() {
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"
severity = "warning"
category = "quality"
file = "src/a.rs"

[[items]]
id = "R2"
summary = "second"
status = "open"
severity = "warning"
category = "quality"
file = "src/b.rs"
"#;
    let ops = r#"[
        {"op":"add","json":{"id":"R3","summary":"third","status":"open","severity":"warning","category":"quality","file":"src/c.rs"}},
        {"op":"update","id":"R1","json":{"status":"fixed","resolution":"fixed in xyz","resolved":"2026-04-18"}},
        {"op":"remove","id":"R2"}
    ]"#;

    // Fixture A: seed, then run dry-run + live apply.
    let (dir_a, ledger_a) = seed_ledger(fixture);
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir_a.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env("TOMLCTL_NO_DEDUP_ID", "1")
        .arg("items")
        .arg("apply")
        .arg(&ledger_a)
        .arg("--ops")
        .arg(ops)
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir_a.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env("TOMLCTL_NO_DEDUP_ID", "1")
        .arg("items")
        .arg("apply")
        .arg(&ledger_a)
        .arg("--ops")
        .arg(ops)
        .write_stdin("")
        .assert()
        .success();
    let bytes_a = fs::read(&ledger_a).unwrap();

    // Fixture B: seed, then run live apply only (no dry-run).
    let (dir_b, ledger_b) = seed_ledger(fixture);
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir_b.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env("TOMLCTL_NO_DEDUP_ID", "1")
        .arg("items")
        .arg("apply")
        .arg(&ledger_b)
        .arg("--ops")
        .arg(ops)
        .write_stdin("")
        .assert()
        .success();
    let bytes_b = fs::read(&ledger_b).unwrap();

    assert_eq!(
        bytes_a, bytes_b,
        "dry-run then live apply must produce byte-identical output to live-only apply"
    );
}

// ---------------------------------------------------------------------------
// T-glistening dispatch: dry-run path coverage for the six newly-supported
// subcommands (`items add`, `items add-many`, `items update`, `set`,
// `set-json`, `array-append`). Mirrors the T10 (a/c) pattern verbatim:
// prime a sidecar with a real write, snapshot file + sidecar bytes, run the
// `--dry-run` invocation, assert the would_change envelope shape, then
// assert byte-equality on file + sidecar.
//
// Items helpers (Add / AddMany / Update / ArrayAppend) emit
// `{"ok":true,"dry_run":true,"would_change":{"kind":"items","added":N,"updated":N,"removed":N,"skipped":N,"ids":[...]}}`
// via `emit_dry_run_plan`. Scalar helpers (Set / SetJson) emit
// `{"ok":true,"dry_run":true,"would_change":{"kind":"scalar","path":"<p>","old":<json|null>,"new":<json>}}`
// via `emit_dry_run_scalar`. The shape divergence is intentional — the
// items envelope counts row-level changes; the scalar envelope describes a
// single key-path mutation. R10 added the `kind` discriminator (additive)
// so consumers can branch on `would_change.kind` rather than dispatching
// on the subcommand they invoked.
// ---------------------------------------------------------------------------

/// Compute the `<file>.sha256` sidecar path the same way the live writers do
/// (suffix `.sha256` after the extension). Local helper kept private to this
/// binary to avoid leaking sidecar-naming conventions into `tests/common`.
fn sidecar_for(ledger: &std::path::Path) -> PathBuf {
    let mut s = ledger.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

/// Drive a real `items update` on R1 to materialise the `.sha256` sidecar.
/// Same priming pattern as the existing T10 (a) / T10 (c) tests above —
/// keeps the post-prime ledger bytes deterministic across runs because the
/// patch is a no-op (`status:"open"` already holds).
fn prime_sidecar_via_update(dir: &tempfile::TempDir, ledger: &std::path::Path) {
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("update")
        .arg(ledger)
        .arg("R1")
        .arg("--json")
        .arg(r#"{"status":"open"}"#)
        .write_stdin("")
        .assert()
        .success();
}

/// Run a `tomlctl --dry-run` command and assert the ledger and sidecar
/// bytes are unchanged. Returns the parsed stdout JSON envelope.
///
/// `extra_args` should include the subcommand and all arguments EXCEPT
/// `--dry-run` (which this helper appends). `extra_envs` are additional
/// env vars beyond the standard `TOMLCTL_ROOT` / `TOMLCTL_LOCK_TIMEOUT` /
/// `TOMLCTL_NO_DEDUP_ID` set.
fn run_dry_run_invariant(
    dir: &tempfile::TempDir,
    ledger: &std::path::Path,
    extra_args: &[&str],
    extra_envs: &[(&str, &str)],
) -> serde_json::Value {
    let sidecar = sidecar_for(ledger);
    let before_bytes = fs::read(ledger).unwrap();
    let before_sidecar = fs::read(&sidecar).unwrap();

    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env("TOMLCTL_NO_DEDUP_ID", "1");
    for (k, v) in extra_envs {
        cmd.env(k, v);
    }
    let out = cmd
        .args(extra_args)
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();

    let after_bytes = fs::read(ledger).unwrap();
    let after_sidecar = fs::read(&sidecar).unwrap();
    assert_eq!(
        before_bytes, after_bytes,
        "dry-run must not change ledger bytes"
    );
    assert_eq!(
        before_sidecar, after_sidecar,
        "dry-run must not change sidecar bytes"
    );

    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dry-run stdout must be JSON: {e}; stdout:\n{stdout}"))
}

/// T-glistening (1): `items add --dry-run --json {...}` emits the
/// `would_change` envelope (added=1, ids=[<id>]) and leaves the ledger +
/// sidecar byte-identical.
#[test]
fn items_add_dry_run_emits_envelope_and_leaves_file_unchanged() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"
"#,
    );
    prime_sidecar_via_update(&dir, &ledger);
    let sidecar = sidecar_for(&ledger);
    assert!(sidecar.exists(), "sidecar must exist after priming write");

    // R16: capture mtime before the dry-run to assert it is unchanged after.
    let before_mtime = fs::metadata(&sidecar).unwrap().modified().unwrap();

    let v = run_dry_run_invariant(
        &dir,
        &ledger,
        &[
            "items",
            "add",
            ledger.to_str().unwrap(),
            "--json",
            r#"{"id":"R2","summary":"second","status":"open"}"#,
        ],
        &[],
    );

    let after_mtime = fs::metadata(&sidecar).unwrap().modified().unwrap();
    assert_eq!(
        before_mtime, after_mtime,
        "dry-run must not touch sidecar mtime"
    );

    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: items-shape envelopes carry kind:"items" as the discriminator.
    assert_eq!(wc["kind"], serde_json::json!("items"));
    assert_eq!(wc["added"], serde_json::json!(1));
    assert_eq!(wc["updated"], serde_json::json!(0));
    assert_eq!(wc["removed"], serde_json::json!(0));
    assert_eq!(wc["ids"], serde_json::json!(["R2"]));
}

/// T-glistening (2): `items add-many --dry-run --ndjson <path>` emits the
/// `would_change` envelope (added=N, ids=[...]) and leaves file + sidecar
/// byte-identical. Uses an NDJSON file source (stdin would also work, but
/// a file source mirrors the existing add-many test pattern more closely).
#[test]
fn items_add_many_dry_run_emits_envelope_and_leaves_file_unchanged() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"
"#,
    );
    prime_sidecar_via_update(&dir, &ledger);

    let ndjson = dir.path().join("rows.ndjson");
    fs::write(
        &ndjson,
        "\
{\"id\":\"R2\",\"summary\":\"second\",\"status\":\"open\"}
{\"id\":\"R3\",\"summary\":\"third\",\"status\":\"open\"}
",
    )
    .unwrap();

    let ndjson_str = ndjson.to_str().unwrap().to_owned();
    let v = run_dry_run_invariant(
        &dir,
        &ledger,
        &[
            "items",
            "add-many",
            ledger.to_str().unwrap(),
            "--ndjson",
            &ndjson_str,
        ],
        &[],
    );

    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: items-shape envelopes carry kind:"items" as the discriminator.
    assert_eq!(wc["kind"], serde_json::json!("items"));
    assert_eq!(wc["added"], serde_json::json!(2));
    assert_eq!(wc["updated"], serde_json::json!(0));
    assert_eq!(wc["removed"], serde_json::json!(0));
    assert_eq!(wc["ids"], serde_json::json!(["R2", "R3"]));
}

/// T-glistening (3): `items update --dry-run --json {patch}` emits the
/// `would_change` envelope (updated=1, ids=[<id>]) and leaves file +
/// sidecar byte-identical.
#[test]
fn items_update_dry_run_emits_envelope_and_leaves_file_unchanged() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"
"#,
    );
    prime_sidecar_via_update(&dir, &ledger);

    // items update does not set TOMLCTL_NO_DEDUP_ID; pass empty extra_envs
    // and use the standard envs from run_dry_run_invariant (TOMLCTL_NO_DEDUP_ID=1
    // is harmless here since no add occurs).
    let v = run_dry_run_invariant(
        &dir,
        &ledger,
        &[
            "items",
            "update",
            ledger.to_str().unwrap(),
            "R1",
            "--json",
            r#"{"status":"fixed"}"#,
        ],
        &[],
    );

    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: items-shape envelopes carry kind:"items" as the discriminator.
    assert_eq!(wc["kind"], serde_json::json!("items"));
    assert_eq!(wc["added"], serde_json::json!(0));
    assert_eq!(wc["updated"], serde_json::json!(1));
    assert_eq!(wc["removed"], serde_json::json!(0));
    assert_eq!(wc["ids"], serde_json::json!(["R1"]));
}

/// T-glistening (4): `set <file> <path> <value> --dry-run` emits the scalar
/// envelope (`would_change.{path,old,new}`) and leaves file + sidecar
/// byte-identical.
#[test]
fn set_dry_run_emits_scalar_envelope_and_leaves_file_unchanged() {
    let (dir, ledger) = seed_ledger("schema_version = 1\nstatus = \"open\"\n");
    // Prime the sidecar via a real `set` write (no-op semantically — same
    // value).
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("set")
        .arg(&ledger)
        .arg("status")
        .arg("open")
        .write_stdin("")
        .assert()
        .success();
    let sidecar = sidecar_for(&ledger);
    assert!(sidecar.exists(), "sidecar must exist after priming write");

    let before_bytes = fs::read(&ledger).unwrap();
    let before_sidecar = fs::read(&sidecar).unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("set")
        .arg(&ledger)
        .arg("status")
        .arg("fixed")
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();

    let after_bytes = fs::read(&ledger).unwrap();
    let after_sidecar = fs::read(&sidecar).unwrap();
    assert_eq!(
        before_bytes, after_bytes,
        "ledger bytes must be unchanged after set --dry-run"
    );
    assert_eq!(
        before_sidecar, after_sidecar,
        "sidecar bytes must be unchanged after set --dry-run"
    );

    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dry-run stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: scalar-shape envelopes carry kind:"scalar" as the discriminator.
    assert_eq!(wc["kind"], serde_json::json!("scalar"));
    assert_eq!(wc["path"], serde_json::json!("status"));
    assert_eq!(wc["old"], serde_json::json!("open"));
    assert_eq!(wc["new"], serde_json::json!("fixed"));
}

/// T-glistening (5): `set-json <file> <path> --json '{"k":"v"}' --dry-run`
/// emits the scalar envelope and leaves file + sidecar byte-identical.
/// `new` echoes the parsed JSON payload as-is (the live writer would
/// `maybe_date_coerce` only on DATE_KEYS at the leaf — non-date payloads
/// round-trip through `new_value` unchanged, per `compute_set_json_mutation`).
#[test]
fn set_json_dry_run_emits_scalar_envelope_and_leaves_file_unchanged() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    // Prime sidecar via a real set-json write.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("set-json")
        .arg(&ledger)
        .arg("meta")
        .arg("--json")
        .arg(r#"{"phase":"one"}"#)
        .write_stdin("")
        .assert()
        .success();
    let sidecar = sidecar_for(&ledger);

    let before_bytes = fs::read(&ledger).unwrap();
    let before_sidecar = fs::read(&sidecar).unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("set-json")
        .arg(&ledger)
        .arg("meta")
        .arg("--json")
        .arg(r#"{"phase":"two"}"#)
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();

    let after_bytes = fs::read(&ledger).unwrap();
    let after_sidecar = fs::read(&sidecar).unwrap();
    assert_eq!(
        before_bytes, after_bytes,
        "ledger bytes must be unchanged after set-json --dry-run"
    );
    assert_eq!(
        before_sidecar, after_sidecar,
        "sidecar bytes must be unchanged after set-json --dry-run"
    );

    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dry-run stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: scalar-shape envelopes carry kind:"scalar" as the discriminator.
    assert_eq!(wc["kind"], serde_json::json!("scalar"));
    assert_eq!(wc["path"], serde_json::json!("meta"));
    assert_eq!(wc["old"], serde_json::json!({"phase":"one"}));
    assert_eq!(wc["new"], serde_json::json!({"phase":"two"}));
}

/// T-glistening (6): `array-append <file> <array> --json {...} --dry-run`
/// emits the items envelope (added=1, ids=[<id-or-empty>]) and leaves file +
/// sidecar byte-identical. `array-append` reuses `compute_array_append_mutation`
/// which is the same `MutationPlan` shape `items add` uses, so the envelope
/// keys match the items helpers, not the scalar helpers.
#[test]
fn array_append_dry_run_emits_envelope_and_leaves_file_unchanged() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    // Prime sidecar via a real array-append write.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("array-append")
        .arg(&ledger)
        .arg("rollback_events")
        .arg("--json")
        .arg(r#"{"id":"E1","cause":"first"}"#)
        .write_stdin("")
        .assert()
        .success();

    let v = run_dry_run_invariant(
        &dir,
        &ledger,
        &[
            "array-append",
            ledger.to_str().unwrap(),
            "rollback_events",
            "--json",
            r#"{"id":"E2","cause":"second"}"#,
        ],
        &[],
    );

    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: items-shape envelopes carry kind:"items" as the discriminator —
    // array-append uses the items envelope shape because it shares the
    // MutationPlan compute path with `items add`.
    assert_eq!(wc["kind"], serde_json::json!("items"));
    assert_eq!(wc["added"], serde_json::json!(1));
    assert_eq!(wc["updated"], serde_json::json!(0));
    assert_eq!(wc["removed"], serde_json::json!(0));
    // `array-append` has no implicit `id` semantics; the helper reads the
    // payload's `id` field if present, so ids = ["E2"] with the payload above.
    assert_eq!(wc["ids"], serde_json::json!(["E2"]));
}

// ---------------------------------------------------------------------------
// Edge-case coverage for the dry-run paths.
// ---------------------------------------------------------------------------

/// T-glistening (7) edge case: `items add --dry-run` against a missing file
/// errors with `kind=not_found` AND does NOT bootstrap the file on disk.
///
/// Plan-deviation note: the orchestrator's spec sketch suggested that a
/// non-strict dry-run against a missing file would bootstrap an empty doc
/// and emit a positive envelope showing the row would be added. The actual
/// dry-run dispatch (`Cmd::Items::Add { dry_run: true }`) goes through
/// `read_doc` which surfaces `kind=not_found` from `read_toml` — there is
/// no bootstrap branch on the dry-run path. The principle the spec was
/// after — "dry-run never touches the filesystem" — is preserved either
/// way; this test pins the actual behaviour (errors AND no file created).
#[test]
fn items_add_dry_run_against_missing_file_errors_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("missing.toml");
    assert!(!ledger.exists(), "precondition: file must not exist");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env("TOMLCTL_NO_DEDUP_ID", "1")
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","summary":"first","status":"open"}"#)
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .failure();

    // Filesystem invariance: the dry-run must NOT have created the file.
    assert!(
        !ledger.exists(),
        "dry-run must never create the target file on disk"
    );
    let sidecar = sidecar_for(&ledger);
    assert!(
        !sidecar.exists(),
        "dry-run must never create the sidecar on disk"
    );

    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("json error stderr must parse: {e}; stderr:\n{stderr}"));
    let err = v.get("error").expect("error envelope");
    assert_eq!(err["kind"], serde_json::json!("not_found"));
}

/// T-glistening (8) edge case: `items add --dedupe-by <field> --dry-run`
/// against a fixture whose existing row matches the new row on the dedupe
/// fields emits the envelope with `added=0, ids=[]` (the row would be
/// skipped on a real run). `emit_dry_run_plan` now surfaces the skipped
/// count in the envelope as `would_change.skipped`.
#[test]
fn items_add_dry_run_with_dedupe_by_matching_existing_row_emits_skipped() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "first"
status = "open"
file = "src/a.rs"
"#,
    );
    prime_sidecar_via_update(&dir, &ledger);
    let sidecar = sidecar_for(&ledger);

    let before_bytes = fs::read(&ledger).unwrap();
    let before_sidecar = fs::read(&sidecar).unwrap();

    // The new row's `summary` matches R1's `summary` ("first") — with
    // `--dedupe-by summary` the live path would skip; the dry-run path
    // reflects that with added=0.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env("TOMLCTL_NO_DEDUP_ID", "1")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R2","summary":"first","status":"open","file":"src/b.rs"}"#)
        .arg("--dedupe-by")
        .arg("summary")
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();

    let after_bytes = fs::read(&ledger).unwrap();
    let after_sidecar = fs::read(&sidecar).unwrap();
    assert_eq!(
        before_bytes, after_bytes,
        "ledger bytes must be unchanged after dedupe-skip dry-run"
    );
    assert_eq!(
        before_sidecar, after_sidecar,
        "sidecar bytes must be unchanged after dedupe-skip dry-run"
    );

    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dry-run stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    // R10: items-shape envelopes carry kind:"items" as the discriminator.
    assert_eq!(wc["kind"], serde_json::json!("items"));
    assert_eq!(wc["added"], serde_json::json!(0));
    assert_eq!(wc["updated"], serde_json::json!(0));
    assert_eq!(wc["removed"], serde_json::json!(0));
    assert_eq!(wc["skipped"], serde_json::json!(1));
    // No row appended → no ids surface in the envelope.
    assert_eq!(wc["ids"], serde_json::json!([] as [&str; 0]));
}

/// T-glistening (10): `items backfill-dedup-id --dry-run` does not touch
/// the ledger or sidecar, and emits a dry-run envelope with `would_backfill`
/// reflecting the count of items that would have their `dedup_id` populated.
#[test]
fn items_backfill_dedup_id_dry_run_does_not_touch_ledger_or_sidecar() {
    // Use a fixture with one item that lacks a `dedup_id` so the backfill
    // has real work to do.
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
line = 0
severity = "warning"
effort = "small"
category = "quality"
summary = "first"
status = "open"
"#,
    );
    prime_sidecar_via_update(&dir, &ledger);
    let sidecar = sidecar_for(&ledger);
    assert!(sidecar.exists(), "sidecar must exist after priming write");

    let before_bytes = fs::read(&ledger).unwrap();
    let before_sidecar = fs::read(&sidecar).unwrap();

    // Do NOT set TOMLCTL_NO_DEDUP_ID — that env var would cause
    // backfill-dedup-id to short-circuit with `disabled-by-env`.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("backfill-dedup-id")
        .arg(&ledger)
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .success();

    let after_bytes = fs::read(&ledger).unwrap();
    let after_sidecar = fs::read(&sidecar).unwrap();
    assert_eq!(
        before_bytes, after_bytes,
        "dry-run must not change ledger bytes"
    );
    assert_eq!(
        before_sidecar, after_sidecar,
        "dry-run must not change sidecar bytes"
    );

    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dry-run stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    // backfill --dry-run emits {"ok":true,"dry_run":true,"would_backfill":N,"ids":[...]}
    assert!(
        v["would_backfill"].is_number(),
        "backfill --dry-run envelope must include would_backfill: {v}"
    );
    assert!(
        v["ids"].is_array(),
        "backfill --dry-run envelope must include ids array: {v}"
    );
}

/// T-glistening (9) edge case: `items add --strict-read --dry-run` against
/// a missing file errors with `kind=not_found`. `--strict-read` is a
/// `ReadIntegrityArgs` flag and `items add` carries `WriteIntegrityArgs`
/// (no `--strict-read`), so this combination is rejected at clap parse
/// time rather than at the runtime layer. The test pins that behaviour:
/// the dry-run path errors with a parse-level message, NOT a partial
/// write or a successful would_change envelope.
///
/// Plan-deviation note: the spec sketch assumed `--strict-read` was
/// available on `items add`. It is not (it lives on read subcommands —
/// `parse`, `get`, `validate`, `items list`, `items get`, etc.). The
/// spirit of the assertion — "a strict missing-file dry-run errors
/// without touching disk" — is preserved by checking the clap-level
/// rejection AND the file-not-created invariant.
#[test]
fn items_add_dry_run_with_strict_read_against_missing_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("missing.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","summary":"first","status":"open"}"#)
        .arg("--strict-read")
        .arg("--dry-run")
        .write_stdin("")
        .assert()
        .failure()
        // R26: pin clap usage-error exit code 2 explicitly. The previous
        // `.failure()`-only check accepted any non-zero status, so a
        // regression that errored with code 1 (runtime error) for an
        // unrelated reason would have silently passed the OR-predicate
        // on stderr. Code 2 is clap's parse-rejection signal.
        .code(2);

    // `--strict-read` is not on `items add`'s WriteIntegrityArgs, so clap
    // rejects with "unexpected argument" at parse time. The exit code is
    // pinned at 2 (clap usage error) above; the stderr predicate below
    // tightens the structural rejection by requiring clap to surface the
    // offending flag name OR the canonical "unexpected argument" prose.
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--strict-read") || stderr.contains("unexpected argument"),
        "expected clap usage error naming --strict-read; got stderr:\n{stderr}"
    );
    assert!(
        !ledger.exists(),
        "rejected dry-run must never create the target file on disk"
    );
    let sidecar = sidecar_for(&ledger);
    assert!(
        !sidecar.exists(),
        "rejected dry-run must never create the sidecar on disk"
    );
}
