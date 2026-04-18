//! R41 + R58: black-box integration harness for tomlctl. Exercises the built
//! binary end-to-end via `assert_cmd`, covering behaviours that unit tests
//! can't easily reach (stdin sentinel, concurrent lock contention, CLI
//! argument parsing, etc.).

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// R58 coverage: `tomlctl items next-id` on a missing ledger must return
/// `<prefix>1` without parsing anything. This exercises the early-return path
/// added in R19 when `file.exists()` is false.
#[test]
fn items_next_id_on_missing_file_prints_prefix_one() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");
    assert!(!missing.exists());

    // `items next-id` doesn't consume stdin, but assert_cmd inherits the
    // parent's stdin by default — pipe an empty string in so nothing blocks
    // if the parent's stdin happens to be a TTY when tests run interactively.
    // R60: `--allow-outside` is now flattened onto each subcommand variant
    // rather than global, so it must appear after the subcommand name.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&missing)
        .arg("--prefix")
        .arg("R")
        .arg("--allow-outside")
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::contains("R1"));
}

/// R41 part 2 — stdin sentinel: piping a JSON payload into `items apply --ops -`
/// on a seeded ledger must apply the add op and leave the expected item on
/// disk.
#[test]
fn items_apply_reads_ops_from_stdin_dash() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
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

    let payload = r#"[{"op":"add","json":{"id":"R42","summary":"added via stdin","status":"open"}}]"#;

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("apply")
        .arg(&ledger)
        .arg("--ops")
        .arg("-")
        .write_stdin(payload)
        .assert()
        .success();

    let out = fs::read_to_string(&ledger).unwrap();
    assert!(
        out.contains(r#"id = "R42""#),
        "expected R42 added to ledger, got:\n{out}"
    );
    assert!(
        out.contains("added via stdin"),
        "expected stdin-sourced summary to land on disk, got:\n{out}"
    );
}

/// R41 part 3 — lock contention smoke test: spawn two `items add` processes
/// on the same file with a short timeout. At least one must succeed; the
/// other either succeeds (lock acquired after the first finishes) or errors
/// cleanly with the documented "could not acquire" / "lock held" message —
/// never a silent corruption or a panic.
#[test]
fn items_add_lock_contention_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("ledger.toml");
    fs::write(
        &ledger,
        r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
summary = "seed"
"#,
    )
    .unwrap();

    let bin = assert_cmd::cargo::cargo_bin("tomlctl");

    fn spawn(
        bin: &std::path::Path,
        root: &std::path::Path,
        ledger: &std::path::Path,
        id: &str,
    ) -> std::process::Child {
        let patch = format!(
            r#"{{"id":"{}","status":"open","summary":"concurrent"}}"#,
            id
        );
        std::process::Command::new(bin)
            .env("TOMLCTL_ROOT", root)
            .env("TOMLCTL_LOCK_TIMEOUT", "2")
            .arg("items")
            .arg("add")
            .arg(ledger)
            .arg("--json")
            .arg(patch)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn tomlctl")
    }

    let c1 = spawn(&bin, dir.path(), &ledger, "RA");
    let c2 = spawn(&bin, dir.path(), &ledger, "RB");
    let o1 = c1.wait_with_output().unwrap();
    let o2 = c2.wait_with_output().unwrap();

    let succeeded = [&o1, &o2].iter().filter(|o| o.status.success()).count();
    assert!(
        succeeded >= 1,
        "at least one concurrent add must succeed; got statuses {:?} / {:?}",
        o1.status,
        o2.status
    );

    // Any failed child must fail CLEANLY (non-zero exit, no panic / broken
    // pipe). The 2-second timeout makes panic unlikely but we still want
    // the expected error text somewhere in stderr when a failure occurs.
    for out in [&o1, &o2] {
        if out.status.success() {
            continue;
        }
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("lock held")
                || err.contains("acquire")
                || err.contains("could not"),
            "failing child must report a lock error, got stderr:\n{err}"
        );
    }

    // The surviving state must still parse as valid TOML and contain at
    // least the seed `R1` plus one of the two racers.
    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).expect("post-race ledger must be valid TOML");
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    assert!(
        items.len() >= 2,
        "expected seed + at least one winner, got {} items",
        items.len()
    );
}

/// R60: `blocks verify` must NOT accept any of the four integrity/containment
/// flags (`--verify-integrity`, `--no-write-integrity`, `--strict-integrity`,
/// `--allow-outside`). They were previously `global = true` on `Cli` and
/// silently ignored here (`blocks verify` scans markdown, not the TOML +
/// sidecar pair). The R60 refactor moves them onto each TOML-touching
/// subcommand via a flattened `IntegrityArgs`, which structurally keeps them
/// off `blocks verify`. Passing one now errors at the clap layer — this test
/// locks in that contract so a future refactor can't silently re-introduce
/// the flag on a subcommand where it has no semantic hook.
#[test]
fn blocks_verify_rejects_integrity_flags() {
    for flag in [
        "--verify-integrity",
        "--no-write-integrity",
        "--strict-integrity",
        "--allow-outside",
    ] {
        let assert = Command::cargo_bin("tomlctl")
            .unwrap()
            .arg("blocks")
            .arg("verify")
            .arg(flag)
            .arg("some-file.md")
            .write_stdin("")
            .assert()
            .failure();
        let err = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        assert!(
            err.contains("unexpected argument")
                || err.contains("argument '--")
                || err.contains("found argument"),
            "`blocks verify {flag}` must be rejected by clap as an unknown argument; got stderr:\n{err}"
        );
    }
}

// ---------------------------------------------------------------------------
// Task 6 additions — end-to-end coverage for the agent-native-tomlctl plan:
// `items add-many`, `array-append`, the expanded `items list` query surface,
// and the checked-in `.claude/settings.json` permissions shape.
// ---------------------------------------------------------------------------

/// Seed a fresh tempdir with a minimal ledger and return
/// `(tempdir, ledger_path)`. The caller owns `tempdir`; dropping it cleans up.
fn seed_ledger(initial: &str) -> (tempfile::TempDir, PathBuf) {
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
const QUERY_FIXTURE: &str = r#"schema_version = 1

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

// ---------------- add-many suite ----------------

#[test]
fn items_add_many_happy_path_with_defaults() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1
"#,
    );
    let payload = "\
{\"id\":\"R1\",\"summary\":\"one\"}
{\"id\":\"R2\",\"summary\":\"two\"}
{\"id\":\"R3\",\"summary\":\"three\",\"status\":\"wontfix\"}
{\"id\":\"R4\",\"summary\":\"four\"}
{\"id\":\"R5\",\"summary\":\"five\"}
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
        .arg("--defaults-json")
        .arg(r#"{"first_flagged":"2026-04-18","rounds":1,"status":"open"}"#)
        .write_stdin(payload)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains(r#"{"ok":true,"added":5}"#),
        "expected ok/added=5 on stdout, got: {stdout}"
    );

    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).expect("post-write TOML must parse");
    let items = parsed
        .get("items")
        .and_then(|v| v.as_array())
        .expect("[[items]] array");
    assert_eq!(items.len(), 5, "expected 5 items, got {}", items.len());

    // Every row carried the date default through as a TOML Datetime (not a
    // string). DATE_KEYS owns this contract; pin it here end-to-end.
    for it in items {
        let tbl = it.as_table().unwrap();
        let ff = tbl.get("first_flagged").expect("first_flagged present");
        assert!(
            ff.as_datetime().is_some(),
            "first_flagged must be a TOML datetime, got {:?}",
            ff
        );
        assert_eq!(ff.as_datetime().unwrap().to_string(), "2026-04-18");
        assert_eq!(tbl.get("rounds").and_then(|v| v.as_integer()), Some(1));
    }

    // Row 3 overrode `status`; rows 1/2/4/5 must have the default.
    let by_id = |id: &str| -> &toml::Table {
        items
            .iter()
            .find(|it| it.as_table().and_then(|t| t.get("id")).and_then(|v| v.as_str()) == Some(id))
            .and_then(|v| v.as_table())
            .unwrap_or_else(|| panic!("missing {id}"))
    };
    assert_eq!(by_id("R1").get("status").and_then(|v| v.as_str()), Some("open"));
    assert_eq!(by_id("R3").get("status").and_then(|v| v.as_str()), Some("wontfix"));
    assert_eq!(by_id("R5").get("status").and_then(|v| v.as_str()), Some("open"));
}

#[test]
fn items_add_many_rejects_malformed_line_without_mutating() {
    let seed = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
summary = "seed"
"#;
    let (dir, ledger) = seed_ledger(seed);
    let before = fs::read(&ledger).unwrap();

    let payload = "\
{\"id\":\"R2\",\"summary\":\"ok\"}
{\"id\":\"R3\",\"summary\":\"ok\"}
not-json
{\"id\":\"R4\",\"summary\":\"ok\"}
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
        .write_stdin(payload)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("line 3"),
        "error must name line 3; got stderr:\n{stderr}"
    );

    // Ledger must be byte-identical to the seed — no partial mutation.
    let after = fs::read(&ledger).unwrap();
    assert_eq!(
        before, after,
        "malformed batch must leave the ledger untouched"
    );
}

// ---------------- array-append suite ----------------

#[test]
fn array_append_single_json_creates_rollback_events() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
summary = "seed"
"#,
    );

    let payload = r#"{"timestamp":"2026-04-18T14:32:00Z","command":"review-apply","cause":"build failure","items":["R3","R7"],"stash_ref":"stash@{0}"}"#;

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("array-append")
        .arg(&ledger)
        .arg("rollback_events")
        .arg("--json")
        .arg(payload)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains(r#"{"ok":true,"appended":1}"#),
        "expected ok/appended=1 on stdout, got: {stdout}"
    );

    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).expect("post-write TOML must parse");
    let events = parsed
        .get("rollback_events")
        .and_then(|v| v.as_array())
        .expect("[[rollback_events]] array present");
    assert_eq!(events.len(), 1);
    let ev = events[0].as_table().unwrap();
    // `timestamp` is NOT in `DATE_KEYS` (see convert.rs::DATE_KEYS), so it
    // must stay a plain TOML string rather than promoting to a datetime.
    let ts = ev.get("timestamp").expect("timestamp present");
    assert!(
        ts.as_str().is_some(),
        "timestamp must remain a string (not in DATE_KEYS), got {:?}",
        ts
    );
    assert_eq!(ts.as_str(), Some("2026-04-18T14:32:00Z"));
    // Source-level assertion: the serialised TOML carries `timestamp =
    // "...":"` with quotes, confirming no datetime promotion happened.
    assert!(
        contents.contains("timestamp = \"2026-04-18T14:32:00Z\""),
        "serialised form must quote the string; got:\n{contents}"
    );
    assert_eq!(ev.get("command").and_then(|v| v.as_str()), Some("review-apply"));

    // Seed [[items]] row must remain untouched.
    let items = parsed.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 1);
}

#[test]
fn array_append_ndjson_appends_many() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1
"#,
    );
    let payload = "\
{\"timestamp\":\"2026-04-18T10:00:00Z\",\"command\":\"review-apply\",\"cause\":\"first\",\"items\":[\"R1\"],\"stash_ref\":\"stash@{0}\"}
{\"timestamp\":\"2026-04-18T11:00:00Z\",\"command\":\"optimise-apply\",\"cause\":\"second\",\"items\":[\"R2\"],\"stash_ref\":\"stash@{1}\"}
{\"timestamp\":\"2026-04-18T12:00:00Z\",\"command\":\"review-apply\",\"cause\":\"third\",\"items\":[\"R3\"],\"stash_ref\":\"stash@{2}\"}
";

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("array-append")
        .arg(&ledger)
        .arg("rollback_events")
        .arg("--ndjson")
        .arg("-")
        .write_stdin(payload)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains(r#"{"ok":true,"appended":3}"#),
        "expected ok/appended=3 on stdout, got: {stdout}"
    );

    let contents = fs::read_to_string(&ledger).unwrap();
    let parsed: toml::Value = toml::from_str(&contents).unwrap();
    let events = parsed
        .get("rollback_events")
        .and_then(|v| v.as_array())
        .expect("[[rollback_events]] present");
    assert_eq!(events.len(), 3);
    // Insertion order must be preserved.
    let causes: Vec<&str> = events
        .iter()
        .map(|e| e.as_table().unwrap().get("cause").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(causes, vec!["first", "second", "third"]);
}

#[test]
fn array_append_requires_json_or_ndjson() {
    let (dir, ledger) = seed_ledger(
        r#"schema_version = 1
"#,
    );
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("array-append")
        .arg(&ledger)
        .arg("rollback_events")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--json") && stderr.contains("--ndjson"),
        "error must name both --json and --ndjson as the required alternatives; got stderr:\n{stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("requires") || stderr.to_lowercase().contains("required"),
        "error must explain that one is required; got stderr:\n{stderr}"
    );
}

// ---------------- query suite ----------------

/// Run `tomlctl items list …` against the 6-item fixture and return
/// `(stdout, exit-assert)`. Panics if the command fails.
fn run_list_query(args: &[&str]) -> String {
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

#[test]
fn items_list_group_by_file_with_select_shape() {
    let stdout = run_list_query(&["--group-by", "file", "--select", "id,severity"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let obj = v.as_object().expect("group-by output is an object");
    // Two files in the fixture.
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![&"src/a.rs".to_string(), &"src/b.rs".to_string()],
        "expected exactly the two fixture files as group keys"
    );

    // Every element of every group must project down to exactly {id,
    // severity} — nothing else (projection-after-grouping contract).
    for (k, arr) in obj.iter() {
        let arr = arr
            .as_array()
            .unwrap_or_else(|| panic!("group {k} is not an array"));
        assert!(!arr.is_empty(), "group {k} unexpectedly empty");
        for el in arr {
            let m = el
                .as_object()
                .unwrap_or_else(|| panic!("element in {k} is not an object: {el}"));
            let mut el_keys: Vec<&String> = m.keys().collect();
            el_keys.sort();
            assert_eq!(
                el_keys,
                vec![&"id".to_string(), &"severity".to_string()],
                "element in group {k} must project to exactly [id, severity]; got {:?}",
                m.keys().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn items_list_count_by_status_shape() {
    let stdout = run_list_query(&["--count-by", "status"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let obj = v.as_object().expect("count-by is an object");
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![&"fixed".to_string(), &"open".to_string()],
        "expected buckets for open+fixed"
    );
    let mut total = 0i64;
    for (_k, v) in obj.iter() {
        let n = v
            .as_i64()
            .unwrap_or_else(|| panic!("count-by values must be integers, got {v}"));
        total += n;
    }
    assert_eq!(total, 6, "count-by sums must equal fixture size");
}

#[test]
fn items_list_where_composition() {
    // Fixture items with status=open AND first_flagged >= 2026-04-01:
    //   R2 (open, 2026-04-02), R4 (open, 2026-04-10), R6 (open, 2026-04-15).
    let stdout = run_list_query(&[
        "--where",
        "status=open",
        "--where-gte",
        "first_flagged=@date:2026-04-01",
    ]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("list output is an array");
    let mut ids: Vec<&str> = arr
        .iter()
        .map(|el| el.get("id").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["R2", "R4", "R6"], "composed filter mismatch");
}

#[test]
fn items_list_pluck_emits_scalar_array() {
    // Only R2 has `symbol` in the fixture.
    let stdout = run_list_query(&["--where-has", "symbol", "--pluck", "id"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("pluck output must be an array");
    assert_eq!(arr.len(), 1);
    for el in arr {
        assert!(
            el.is_string(),
            "each plucked element must be a string scalar, got {el}"
        );
    }
    assert_eq!(arr[0].as_str(), Some("R2"));
}

#[test]
fn items_list_distinct_on_projected_shape() {
    let stdout = run_list_query(&["--select", "category", "--distinct"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("distinct output is an array");
    // Fixture categories: style, bug, bug, perf, style, security → distinct
    // {style, bug, perf, security}.
    let mut cats: Vec<&str> = arr
        .iter()
        .map(|el| el.get("category").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    cats.sort();
    assert_eq!(cats, vec!["bug", "perf", "security", "style"]);
    for el in arr {
        let m = el.as_object().expect("each element is an object");
        let keys: Vec<&String> = m.keys().collect();
        assert_eq!(keys, vec![&"category".to_string()],
            "each element must project to {{category}} only, got {:?}", m.keys().collect::<Vec<_>>());
    }
}

#[test]
fn items_list_sort_by_asc_then_limit() {
    let stdout = run_list_query(&["--sort-by", "first_flagged", "--limit", "2"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("list output is an array");
    assert_eq!(arr.len(), 2);
    // Fixture earliest → latest:
    //   R1 (2026-03-10), R3 (2026-03-25), R2 (2026-04-02), R5 (2026-04-05),
    //   R4 (2026-04-10), R6 (2026-04-15).
    let ids: Vec<&str> = arr
        .iter()
        .map(|el| el.get("id").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(ids, vec!["R1", "R3"], "ascending sort + limit 2 mismatch");
}

#[test]
fn items_list_ndjson_shape() {
    // --status open matches R1, R2, R4, R6.
    let stdout = run_list_query(&["--status", "open", "--ndjson"]);
    let lines: Vec<&str> = stdout
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 4, "expected 4 open items as 4 NDJSON lines; got:\n{stdout}");
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("each NDJSON line must parse: {e}; line:\n{line}"));
        assert_eq!(
            v.get("status").and_then(|s| s.as_str()),
            Some("open"),
            "every row must have status=open; line:\n{line}"
        );
    }
}

#[test]
fn items_list_preserves_legacy_filter_flags() {
    // Back-compat: the legacy `--status` + `--count` pair must still emit
    // the `{"count": N}` shape unchanged by the Task-5 query additions.
    let stdout = run_list_query(&["--status", "open", "--count"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let obj = v.as_object().expect("count output must be an object");
    let count = obj
        .get("count")
        .and_then(|n| n.as_i64())
        .unwrap_or_else(|| panic!("missing integer `count` key in {obj:?}"));
    assert_eq!(count, 4, "expected 4 open items in the fixture");
}

// ---------------- settings-shape suite ----------------

#[test]
fn settings_json_contains_tomlctl_allow_with_outside_deny() {
    // The manifest dir is `tomlctl/`; the repo root is its parent and owns
    // `.claude/settings.json`. Using env!() keeps the test `cd`-free.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let settings_path = manifest.join("..").join(".claude").join("settings.json");
    let raw = fs::read_to_string(&settings_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {}: {e}",
            settings_path.display()
        )
    });
    let v: serde_json::Value = serde_json::from_str(&raw)
        .expect(".claude/settings.json must be valid JSON");
    let allow = v
        .pointer("/permissions/allow")
        .and_then(|x| x.as_array())
        .expect("permissions.allow must exist and be an array");
    let deny = v
        .pointer("/permissions/deny")
        .and_then(|x| x.as_array())
        .expect("permissions.deny must exist and be an array");

    assert!(
        allow.iter().any(|s| s.as_str() == Some("Bash(tomlctl *)")),
        "permissions.allow must contain `Bash(tomlctl *)`; got {:?}",
        allow
    );
    assert!(
        deny.iter().any(|s| s.as_str() == Some("Bash(tomlctl --allow-outside *)")),
        "permissions.deny must contain `Bash(tomlctl --allow-outside *)`; got {:?}",
        deny
    );
}
