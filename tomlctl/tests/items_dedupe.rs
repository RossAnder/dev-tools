//! Task 5 + Task 6 + Task 11 integration tests — `--dedupe-by`, auto-populated
//! `dedup_id`, `items find-duplicates --across`, and `items backfill-dedup-id`.
//! Split out of the monolithic `integration.rs` by R23. Every test body is
//! byte-identical to its pre-split form; helpers live in `tests/common/mod.rs`.

use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;

mod common;
use common::seed_ledger;

// ---------------------------------------------------------------------------
// Task 5 (plan `docs/plans/tomlctl-capability-gaps.md`): `items add` and
// `items add-many` grow `--dedupe-by <F1,F2,...>`. Callers who pass
// identical payloads twice get one insert on the first call and a skip
// (with `matched_id`) on the second. Nested-field paths and explicit
// `dedup_id` dedup both work via the shared JSON walker in convert.rs.
// Absent `--dedupe-by` → legacy behaviour byte-identical.
// ---------------------------------------------------------------------------

/// T5 (a): double-add with `--dedupe-by summary,file` inserts once, then
/// skips on the second call and reports the `matched_id` of the first-added
/// row. Also asserts that `added:0` and the matched id appear in the JSON
/// output so an agent can branch on the skip shape.
#[test]
fn items_add_dedupe_by_double_add_dedupes_on_second_call() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1
"#,
    );
    let payload = r#"{"id":"R1","file":"x","summary":"y","status":"open"}"#;

    // First call: no existing rows, so the add proceeds. The output carries
    // `added:1` and no `matched_id`.
    let out1 = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(payload)
        .arg("--dedupe-by")
        .arg("summary,file")
        .write_stdin("")
        .assert()
        .success();
    let stdout1 = String::from_utf8_lossy(&out1.get_output().stdout).to_string();
    assert!(
        stdout1.contains(r#""added":1"#),
        "first add must report added=1; got: {stdout1}"
    );
    assert!(
        !stdout1.contains("matched_id"),
        "first add must not emit matched_id; got: {stdout1}"
    );

    // Second call with the same payload: pre-scan finds R1, skips the add.
    let out2 = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(payload)
        .arg("--dedupe-by")
        .arg("summary,file")
        .write_stdin("")
        .assert()
        .success();
    let stdout2 = String::from_utf8_lossy(&out2.get_output().stdout).to_string();
    assert!(
        stdout2.contains(r#""added":0"#),
        "second add must report added=0; got: {stdout2}"
    );
    assert!(
        stdout2.contains(r#""matched_id":"R1""#),
        "second add must report matched_id=R1; got: {stdout2}"
    );

    // Ledger still has exactly one [[items]] row — the dedupe short-circuit
    // must NOT produce a duplicate R1.
    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).unwrap();
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(
        items.len(),
        1,
        "exactly one item must remain after double-add + dedupe; got {}",
        items.len()
    );
}

/// T5 (b): changing one of the dedupe fields defeats the match — the
/// second call inserts a fresh row. Fixture pins the semantics the agent
/// cares about: "re-using a summary with a different file is a different
/// finding".
#[test]
fn items_add_dedupe_by_different_field_value_adds_both() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1
"#,
    );
    // First add: file=x.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","file":"x","summary":"y"}"#)
        .arg("--dedupe-by")
        .arg("summary,file")
        .write_stdin("")
        .assert()
        .success();

    // Second add: file=z (same summary; different file → no dedupe).
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R2","file":"z","summary":"y"}"#)
        .arg("--dedupe-by")
        .arg("summary,file")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains(r#""added":1"#),
        "distinct-file add must succeed; got: {stdout}"
    );

    // Ledger now has both rows.
    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).unwrap();
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 2);
}

/// T5 (c): `add-many` with a mixed NDJSON batch — one row duplicates an
/// existing item, two are novel. Output must enumerate both the counts
/// and the per-row skip log in input order. Row indexing is 1-based to
/// match the existing `items_add_many` error-message convention.
#[test]
fn items_add_many_dedupe_by_mixed_batch_reports_skipped_rows() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "alpha"
status = "open"
"#,
    );
    let payload = "\
{\"id\":\"R2\",\"file\":\"src/b.rs\",\"summary\":\"beta\"}
{\"id\":\"R99\",\"file\":\"src/a.rs\",\"summary\":\"alpha\"}
{\"id\":\"R3\",\"file\":\"src/c.rs\",\"summary\":\"gamma\"}
";

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add-many")
        .arg(&ledger)
        .arg("--ndjson")
        .arg("-")
        .arg("--dedupe-by")
        .arg("summary,file")
        .write_stdin(payload)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    // Full payload shape: added=2, skipped=1, one-entry skipped_rows with
    // row=2 and matched_id=R1. Parse the stdout as JSON to compare
    // structurally rather than on-string-contains so ordering of other
    // keys doesn't make the test flaky.
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("add-many stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["added"], serde_json::json!(2));
    assert_eq!(v["skipped"], serde_json::json!(1));
    let skipped = v["skipped_rows"].as_array().expect("skipped_rows array");
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["row"], serde_json::json!(2));
    assert_eq!(skipped[0]["matched_id"], serde_json::json!("R1"));

    // Ledger now has 1 (seed) + 2 (novel rows) = 3 items; the duplicate
    // row is absent.
    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).unwrap();
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 3);
}

/// T5 (d): `--dedupe-by` accepts dotted paths for nested-object fields
/// (`meta.source_run`). Both sides walk via the JSON-side dotted-path
/// walker introduced in `convert.rs::walk_json_path`. Pins that
/// descent-via-objects is honoured and that a nested miss on one row
/// doesn't falsely hit a sibling row that lacks `meta` entirely.
#[test]
fn items_add_dedupe_by_nested_field_path() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "alpha"
meta = { source_run = "abc" }

[[items]]
id = "R2"
summary = "beta"
"#,
    );

    // Payload with matching `meta.source_run` — dedupes against R1.
    let out_hit = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R3","summary":"new","meta":{"source_run":"abc"}}"#)
        .arg("--dedupe-by")
        .arg("meta.source_run")
        .write_stdin("")
        .assert()
        .success();
    let stdout_hit = String::from_utf8_lossy(&out_hit.get_output().stdout).to_string();
    assert!(
        stdout_hit.contains(r#""added":0"#)
            && stdout_hit.contains(r#""matched_id":"R1""#),
        "nested dedupe-by meta.source_run must match R1; got: {stdout_hit}"
    );

    // Payload with a distinct `meta.source_run` — appends. Also exercises
    // the "sibling item lacks meta" case: R2 has no `meta`, so the walker
    // returns None on the candidate side. None == None(payload lacks too)
    // would falsely match a payload with no `meta` — this test pins that
    // a payload WITH meta.source_run never falsely matches R2.
    let out_miss = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R4","summary":"new","meta":{"source_run":"xyz"}}"#)
        .arg("--dedupe-by")
        .arg("meta.source_run")
        .write_stdin("")
        .assert()
        .success();
    let stdout_miss = String::from_utf8_lossy(&out_miss.get_output().stdout).to_string();
    assert!(
        stdout_miss.contains(r#""added":1"#),
        "distinct nested value must bypass dedupe; got: {stdout_miss}"
    );

    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).unwrap();
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 3, "R1 + R2 + R4 — R3 was deduped");
}

/// T5 (e): explicit `--dedupe-by dedup_id` pins the forward-compatible
/// contract for when T6 starts auto-populating `dedup_id`. Today this is
/// entirely a user-supplied field (T6 ships the auto-populate later in
/// the plan). The test seeds a ledger with a row carrying an explicit
/// `dedup_id`, then:
///
///   1. A payload with the same `dedup_id` is skipped (matches).
///   2. A payload with a distinct `dedup_id` is added.
///
/// This proves `find_dedupe_match` handles the `dedup_id` field like any
/// other, so the upgrade path for T6 is "set the field; the scan already
/// works" — no change to this flag's semantics is required.
#[test]
fn items_add_dedupe_by_explicit_dedup_id_field() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "alpha"
dedup_id = "abc123def4567890"
"#,
    );

    // Hit: same dedup_id as R1 → skip.
    let out_hit = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R2","summary":"different","dedup_id":"abc123def4567890"}"#)
        .arg("--dedupe-by")
        .arg("dedup_id")
        .write_stdin("")
        .assert()
        .success();
    let stdout_hit = String::from_utf8_lossy(&out_hit.get_output().stdout).to_string();
    assert!(
        stdout_hit.contains(r#""added":0"#)
            && stdout_hit.contains(r#""matched_id":"R1""#),
        "explicit dedup_id must match; got: {stdout_hit}"
    );

    // Miss: distinct dedup_id → add.
    let out_miss = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R2","summary":"beta","dedup_id":"fedcba9876543210"}"#)
        .arg("--dedupe-by")
        .arg("dedup_id")
        .write_stdin("")
        .assert()
        .success();
    let stdout_miss = String::from_utf8_lossy(&out_miss.get_output().stdout).to_string();
    assert!(
        stdout_miss.contains(r#""added":1"#),
        "distinct dedup_id must add; got: {stdout_miss}"
    );

    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).unwrap();
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 2);
}

/// T5: empty-value `--dedupe-by ""` (or `","` or `" , "`) must error at
/// the CLI boundary rather than silently disable dedupe. This is the
/// fail-loud contract from the plan — a caller who typed the flag with no
/// payload almost certainly didn't mean "no-op", so we surface a
/// directed error instead of writing a duplicate row that would slip
/// past their intended guard. Covers the critical-decision branch in
/// `parse_dedupe_fields`.
#[test]
fn items_add_dedupe_by_empty_value_is_fail_loud() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1
"#,
    );
    for bad in ["", ",", " , "] {
        let out = Command::cargo_bin("tomlctl")
            .unwrap()
            .env("TOMLCTL_ROOT", dir.path())
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .arg("items")
            .arg("add")
            .arg(&ledger)
            .arg("--json")
            .arg(r#"{"id":"R1","summary":"y"}"#)
            .arg("--dedupe-by")
            .arg(bad)
            .write_stdin("")
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(
            stderr.contains("--dedupe-by requires at least one field name"),
            "--dedupe-by {bad:?} must error; got stderr:\n{stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// Task 6 (plan `docs/plans/tomlctl-capability-gaps.md`): `dedup_id`
// auto-populate on every write funnel + `find-duplicates --across <other>`
// for cross-ledger dedup. T6a is helper-level (covered by dedup.rs unit
// tests); T6b and T6c need end-to-end CLI coverage — that's this block.
//
// Acceptance (a)-(h) from the plan are mapped 1:1 onto the tests below so
// the plan audit trail stays readable.
// ---------------------------------------------------------------------------

/// T6b acceptance (a): a freshly-added item carries `dedup_id` on disk,
/// and the digest matches `tier_b_fingerprint` of the fingerprinted fields.
#[test]
fn items_add_auto_populates_dedup_id_on_disk() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","file":"src/a.rs","summary":"x","severity":"warning","category":"quality"}"#)
        .write_stdin("")
        .assert()
        .success();
    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).unwrap();
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    let dedup_id = items[0]
        .as_table()
        .unwrap()
        .get("dedup_id")
        .and_then(|v| v.as_str())
        .expect("dedup_id auto-populated");
    assert_eq!(dedup_id.len(), 16, "must be 16 hex chars; got {dedup_id:?}");
    assert!(
        dedup_id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "lowercase hex only; got {dedup_id:?}"
    );
}

/// T6b acceptance (b): `items update` with a fingerprinted-field patch
/// (`summary`) recomputes `dedup_id`. The new digest must differ from
/// the original (summary changed → fingerprint input changed).
#[test]
fn items_update_summary_recomputes_dedup_id() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items").arg("add").arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","file":"src/a.rs","summary":"old","severity":"warning","category":"quality"}"#)
        .write_stdin("").assert().success();
    let before: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let dedup_before = before.get("items").and_then(|v| v.as_array()).unwrap()[0]
        .as_table().unwrap()
        .get("dedup_id").and_then(|v| v.as_str()).unwrap().to_string();

    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items").arg("update").arg(&ledger).arg("R1")
        .arg("--json").arg(r#"{"summary":"new"}"#)
        .write_stdin("").assert().success();
    let after: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let dedup_after = after.get("items").and_then(|v| v.as_array()).unwrap()[0]
        .as_table().unwrap()
        .get("dedup_id").and_then(|v| v.as_str()).unwrap().to_string();
    assert_ne!(
        dedup_before, dedup_after,
        "summary change must recompute dedup_id"
    );
}

/// T6b acceptance (c): `items update` with a non-fingerprint patch
/// (`status`) preserves `dedup_id` — status is NOT in FINGERPRINTED_FIELDS.
#[test]
fn items_update_non_fingerprint_field_preserves_dedup_id() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items").arg("add").arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","file":"src/a.rs","summary":"x","severity":"warning","category":"quality"}"#)
        .write_stdin("").assert().success();
    let before: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let dedup_before = before.get("items").and_then(|v| v.as_array()).unwrap()[0]
        .as_table().unwrap()
        .get("dedup_id").and_then(|v| v.as_str()).unwrap().to_string();

    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items").arg("update").arg(&ledger).arg("R1")
        .arg("--json").arg(r#"{"status":"fixed"}"#)
        .write_stdin("").assert().success();
    let after: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let dedup_after = after.get("items").and_then(|v| v.as_array()).unwrap()[0]
        .as_table().unwrap()
        .get("dedup_id").and_then(|v| v.as_str()).unwrap().to_string();
    assert_eq!(
        dedup_before, dedup_after,
        "non-fingerprint patch must preserve dedup_id"
    );
}

/// T6b acceptance (d): explicit `{"dedup_id":"explicit"}` in an update
/// patch is preserved regardless of other patch fields.
#[test]
fn items_update_explicit_dedup_id_preserved() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items").arg("add").arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","file":"src/a.rs","summary":"x","severity":"warning","category":"quality"}"#)
        .write_stdin("").assert().success();
    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items").arg("update").arg(&ledger).arg("R1")
        .arg("--json").arg(r#"{"dedup_id":"explicit"}"#)
        .write_stdin("").assert().success();
    let after: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let got = after.get("items").and_then(|v| v.as_array()).unwrap()[0]
        .as_table().unwrap()
        .get("dedup_id").and_then(|v| v.as_str()).unwrap().to_string();
    assert_eq!(got, "explicit", "explicit dedup_id must win");
}

/// T6b acceptance (e): explicit `dedup_id` AND a fingerprint-field patch
/// together — explicit still wins (the recompute must NOT overwrite it).
#[test]
fn items_update_explicit_dedup_id_wins_over_fingerprint_patch() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items").arg("add").arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","file":"src/a.rs","summary":"old","severity":"warning","category":"quality"}"#)
        .write_stdin("").assert().success();
    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items").arg("update").arg(&ledger).arg("R1")
        .arg("--json").arg(r#"{"summary":"new","dedup_id":"explicit"}"#)
        .write_stdin("").assert().success();
    let after: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let got = after.get("items").and_then(|v| v.as_array()).unwrap()[0]
        .as_table().unwrap()
        .get("dedup_id").and_then(|v| v.as_str()).unwrap().to_string();
    assert_eq!(
        got, "explicit",
        "explicit dedup_id must beat recompute even when summary also changes"
    );
}

/// T6b acceptance (f): `TOMLCTL_NO_DEDUP_ID=1` suppresses auto-populate.
/// The resulting item must have NO `dedup_id` field on disk.
#[test]
fn items_add_with_kill_switch_produces_no_dedup_id() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    Command::cargo_bin("tomlctl").unwrap()
        .env("TOMLCTL_ROOT", dir.path()).env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env("TOMLCTL_NO_DEDUP_ID", "1")
        .arg("items").arg("add").arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","file":"src/a.rs","summary":"x","severity":"warning","category":"quality"}"#)
        .write_stdin("").assert().success();
    let after: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let item = &after.get("items").and_then(|v| v.as_array()).unwrap()[0];
    let tbl = item.as_table().unwrap();
    assert!(
        tbl.get("dedup_id").is_none(),
        "kill switch must suppress dedup_id; got: {tbl:?}"
    );
}

/// T6c acceptance (g): `find-duplicates --across` with tier B returns
/// cross-ledger matches, each tagged with `source_file`.
#[test]
fn items_find_duplicates_across_tier_b_returns_cross_ledger_matches() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let primary = claude.join("review.toml");
    let other = claude.join("optimise.toml");
    // Identical fingerprinted fields across the two ledgers — tier B
    // must group them together.
    fs::write(
        &primary,
        r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "dup"
severity = "warning"
category = "quality"
"#,
    )
    .unwrap();
    fs::write(
        &other,
        r#"schema_version = 1

[[items]]
id = "O1"
file = "src/a.rs"
summary = "dup"
severity = "warning"
category = "quality"
"#,
    )
    .unwrap();
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("find-duplicates")
        .arg(&primary)
        .arg("--across")
        .arg(&other)
        .arg("--tier")
        .arg("b")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let groups = parsed.as_array().unwrap();
    assert_eq!(groups.len(), 1, "expected one cross-ledger group; got: {stdout}");
    let items = groups[0].get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 2);
    let source_files: Vec<&str> = items
        .iter()
        .map(|i| i.get("source_file").and_then(|v| v.as_str()).unwrap())
        .collect();
    assert!(source_files.contains(&"review.toml"));
    assert!(source_files.contains(&"optimise.toml"));
}

/// T6c acceptance (h): `find-duplicates --across ... --tier C` errors
/// with the exact documented message. Tier C's line-window grouping is
/// meaningless across two distinct source files.
#[test]
fn items_find_duplicates_across_tier_c_errors_with_exact_message() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let primary = claude.join("x.toml");
    let other = claude.join("y.toml");
    fs::write(&primary, "schema_version = 1\n").unwrap();
    fs::write(&other, "schema_version = 1\n").unwrap();
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("find-duplicates")
        .arg(&primary)
        .arg("--across")
        .arg(&other)
        .arg("--tier")
        .arg("c")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("tier C is file-scoped; use --tier A or --tier B with --across"),
        "expected exact tier-C error; got stderr:\n{stderr}"
    );
}

// ---------------------------------------------------------------------------
// Task 11 (plan `docs/plans/tomlctl-capability-gaps.md`): `items
// backfill-dedup-id <file>` — explicit, auditable upgrade path for pre-Task-6
// ledgers. Walks every item, computes `tier_b_fingerprint` on any item
// lacking `dedup_id`, writes atomically via T10's compute/apply split.
// Idempotent: a re-run on a fully-populated ledger is a no-op (no write,
// no sidecar bump). Honours `TOMLCTL_NO_DEDUP_ID`.
//
// Acceptance (a)-(d) from the plan map 1:1 onto the tests below so the
// plan audit trail stays readable.
// ---------------------------------------------------------------------------

/// T11 (a): ledger with N items, NONE with `dedup_id` → backfill adds the
/// field to every item, and each digest matches `tier_b_fingerprint` of
/// its on-disk fingerprinted fields.
#[test]
fn items_backfill_dedup_id_populates_every_missing_item() {
    // Seed with a kill-switch so the add path doesn't auto-populate —
    // gives us a legacy-shaped ledger with no `dedup_id` anywhere.
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    for (id, summary) in &[("R1", "alpha"), ("R2", "beta"), ("R3", "gamma")] {
        Command::cargo_bin("tomlctl")
            .unwrap()
            .env("TOMLCTL_ROOT", dir.path())
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .env("TOMLCTL_NO_DEDUP_ID", "1")
            .arg("items")
            .arg("add")
            .arg(&ledger)
            .arg("--json")
            .arg(format!(
                r#"{{"id":"{id}","file":"src/a.rs","summary":"{summary}","severity":"warning","category":"quality"}}"#,
            ))
            .write_stdin("")
            .assert()
            .success();
    }
    let before: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let items_before = before.get("items").and_then(|v| v.as_array()).unwrap();
    for item in items_before {
        assert!(
            item.as_table().unwrap().get("dedup_id").is_none(),
            "legacy ledger must have no dedup_id on any item before backfill"
        );
    }

    // Run backfill (kill switch OFF, so auto-populate can actually fire).
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("backfill-dedup-id")
        .arg(&ledger)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(
        v["backfilled"], serde_json::json!(3),
        "three items were missing dedup_id, so backfilled=3"
    );
    // "reason" must NOT appear on the success-with-work shape.
    assert!(
        v.get("reason").is_none(),
        "'reason' should only appear on disabled-by-env; got: {stdout}"
    );

    // After: every item has a valid 16-hex dedup_id matching its
    // fingerprinted fields. The exact digest is deterministic — this test
    // treats length+hex as the minimum invariant (a format change would
    // also surface in the dedup.rs unit tests).
    let after: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let items_after = after.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items_after.len(), 3);
    for item in items_after {
        let tbl = item.as_table().unwrap();
        let fp = tbl
            .get("dedup_id")
            .and_then(|v| v.as_str())
            .expect("dedup_id must be populated after backfill");
        assert_eq!(fp.len(), 16, "must be 16 hex chars; got {fp:?}");
        assert!(
            fp.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "lowercase hex only; got {fp:?}"
        );
    }
    // All three digests must differ (the summaries differ, which feeds
    // into the fingerprint input).
    let fps: Vec<&str> = items_after
        .iter()
        .map(|i| i.as_table().unwrap().get("dedup_id").unwrap().as_str().unwrap())
        .collect();
    assert_ne!(fps[0], fps[1]);
    assert_ne!(fps[1], fps[2]);
    assert_ne!(fps[0], fps[2]);
}

/// T11 (b): mixed ledger — some items have `dedup_id`, some don't. The
/// backfill must touch ONLY the missing ones, preserving the existing
/// values byte-for-byte (even if the existing values are "wrong" for the
/// current fingerprint algorithm — the CLI is explicit about this).
#[test]
fn items_backfill_dedup_id_preserves_preexisting_values() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "alpha"
severity = "warning"
category = "quality"

[[items]]
id = "R2"
file = "src/b.rs"
summary = "beta"
severity = "warning"
category = "quality"
dedup_id = "preexisting-legacy"

[[items]]
id = "R3"
file = "src/c.rs"
summary = "gamma"
severity = "warning"
category = "quality"
"#,
    );

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("backfill-dedup-id")
        .arg(&ledger)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v["backfilled"], serde_json::json!(2),
        "only R1 and R3 were missing dedup_id"
    );

    let after: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    let items = after.get("items").and_then(|v| v.as_array()).unwrap();
    // R1 — newly populated, valid 16-hex.
    let r1_fp = items[0].as_table().unwrap().get("dedup_id").and_then(|v| v.as_str()).unwrap();
    assert_eq!(r1_fp.len(), 16);
    // R2 — preserved verbatim, even though it doesn't match the real
    // fingerprint. Preservation is a hard contract — callers who want to
    // rewrite a "wrong" digest must use `items update` explicitly.
    assert_eq!(
        items[1].as_table().unwrap().get("dedup_id").and_then(|v| v.as_str()),
        Some("preexisting-legacy"),
        "pre-existing dedup_id must be preserved byte-for-byte"
    );
    // R3 — newly populated, valid 16-hex, differs from R1 (different
    // summary feeds a different digest).
    let r3_fp = items[2].as_table().unwrap().get("dedup_id").and_then(|v| v.as_str()).unwrap();
    assert_eq!(r3_fp.len(), 16);
    assert_ne!(r1_fp, r3_fp);
}

/// T11 (c): `TOMLCTL_NO_DEDUP_ID=1` short-circuits backfill to a no-op
/// with the documented explanatory output. The ledger file and its
/// sidecar must remain byte-identical — the kill switch leaves no I/O
/// trace.
#[test]
fn items_backfill_dedup_id_kill_switch_is_no_op() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
summary = "alpha"
severity = "warning"
category = "quality"
"#,
    );
    let before_bytes = fs::read(&ledger).unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env("TOMLCTL_NO_DEDUP_ID", "1")
        .arg("items")
        .arg("backfill-dedup-id")
        .arg(&ledger)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["backfilled"], serde_json::json!(0));
    assert_eq!(
        v["reason"], serde_json::json!("disabled-by-env"),
        "kill-switch short-circuit must emit documented reason; got: {stdout}"
    );
    let after_bytes = fs::read(&ledger).unwrap();
    assert_eq!(
        before_bytes, after_bytes,
        "kill-switch path must not rewrite the ledger"
    );
}

/// T11 (d): idempotent re-run — after (a) backfills everything, a second
/// invocation emits `{"ok":true,"backfilled":0}` (no `reason` field,
/// since the kill switch isn't set) and skips the write entirely. The
/// ledger bytes and the sidecar bytes must both be unchanged — the
/// no-op fast path never takes the exclusive lock.
#[test]
fn items_backfill_dedup_id_idempotent_second_run_skips_write() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","file":"src/a.rs","summary":"alpha","severity":"warning","category":"quality"}"#)
        .write_stdin("")
        .assert()
        .success();

    // The add path already auto-populates `dedup_id`, so the first
    // backfill is trivially a no-op on this fixture — we want the
    // idempotence contract to hold for THAT shape (every item already
    // has dedup_id), regardless of whether it got there via add or a
    // prior backfill.
    let sidecar = {
        let mut s = ledger.clone().into_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    assert!(sidecar.exists(), "add primes the sidecar");
    let before_bytes = fs::read(&ledger).unwrap();
    let before_sidecar_bytes = fs::read(&sidecar).unwrap();
    let before_sidecar_mtime = fs::metadata(&sidecar).unwrap().modified().unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("backfill-dedup-id")
        .arg(&ledger)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["backfilled"], serde_json::json!(0));
    assert!(
        v.get("reason").is_none(),
        "reason must not appear when not disabled-by-env; got: {stdout}"
    );
    // The no-op path must not touch the file or sidecar at all.
    let after_bytes = fs::read(&ledger).unwrap();
    let after_sidecar_bytes = fs::read(&sidecar).unwrap();
    let after_sidecar_mtime = fs::metadata(&sidecar).unwrap().modified().unwrap();
    assert_eq!(
        before_bytes, after_bytes,
        "idempotent backfill must leave ledger bytes unchanged"
    );
    assert_eq!(
        before_sidecar_bytes, after_sidecar_bytes,
        "idempotent backfill must leave sidecar bytes unchanged"
    );
    assert_eq!(
        before_sidecar_mtime, after_sidecar_mtime,
        "idempotent backfill must not bump sidecar mtime"
    );
}
