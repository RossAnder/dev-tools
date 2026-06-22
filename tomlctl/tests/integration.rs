//! R41 + R58: black-box integration harness for tomlctl. Exercises the built
//! binary end-to-end via `assert_cmd`, covering behaviours that unit tests
//! can't easily reach (stdin sentinel, concurrent lock contention, CLI
//! argument parsing, etc.).
//!
//! R23 split this originally-monolithic 4700-line file by topic. Tests
//! specifically about `--dry-run`, `--dedupe-by` / `dedup_id` / backfill,
//! `blocks verify`, and the T7 capabilities surface (including
//! `--count-distinct`, `--raw`, `--error-format`, `--strict-read`,
//! `--lines`, and the `--help` snapshot suite) now live in their own test
//! binaries: `tomlctl/tests/items_dry_run.rs`,
//! `tomlctl/tests/items_dedupe.rs`, `tomlctl/tests/blocks.rs`, and
//! `tomlctl/tests/capabilities.rs`.
//!
//! What remains here is the cross-cutting residue — `items next-id`
//! coverage, `items apply` non-dry-run paths, lock contention, `items
//! add-many` happy paths, `array-append`, the query suite (`--where*`,
//! `--sort-by`, `--group-by`, `--count-by`, `--pluck`, `--distinct`,
//! `--select`, `--exclude`, `--offset`, `--limit`, `--ndjson`, typed-RHS,
//! R80 sidecar coverage on the two new write paths), and the
//! `.claude/settings.json` permissions-shape test.
//!
//! Shared helpers (tempdir + ledger bootstrap, JSON error-envelope parsing,
//! list-query runners, sidecar assertion) live in `tests/common/mod.rs` and
//! are pulled in via `mod common;` below.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

mod common;
use common::{
    assert_sidecar_matches, ids_from, parse_json_error_envelope, run_list_query, seed_ledger,
};

/// R9: parse a write-success envelope (`{"ok":true,"created":<bool>,"path":<file>,…}`)
/// from `stdout` and return it as a typed `serde_json::Value`. Replaces the
/// loose `stdout.contains(r#""created":true"#)` / `contains(r#""path":"#)`
/// substring asserts on the T2 created/path tests — those pass even when
/// `path` is empty or the shape has drifted. Mirrors the parsed-JSON pattern
/// in `tomlctl/tests/render_progress_log.rs` (`serde_json::from_str` +
/// `as_bool`/`as_u64`).
fn parse_write_envelope(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("write-success stdout must be a JSON envelope: {e}; got: {stdout}"))
}

/// R9: assert a write-success envelope reports the expected `created` bool and
/// carries a non-empty `path` string ending in `expected_filename`. The
/// filename-suffix check is what the old `contains(r#""path":"#)` substring
/// missed — it accepted an empty or malformed path.
fn assert_write_envelope(stdout: &str, expected_created: bool, expected_filename: &str) {
    let v = parse_write_envelope(stdout);
    assert_eq!(
        v["ok"].as_bool(),
        Some(true),
        "envelope must report ok:true; got: {stdout}"
    );
    assert_eq!(
        v["created"].as_bool(),
        Some(expected_created),
        "envelope must report created:{expected_created}; got: {stdout}"
    );
    let path = v["path"]
        .as_str()
        .unwrap_or_else(|| panic!("envelope `path` must be a string; got: {stdout}"));
    assert!(
        !path.is_empty(),
        "envelope `path` must be non-empty; got: {stdout}"
    );
    assert!(
        path.ends_with(expected_filename),
        "envelope `path` must end with `{expected_filename}`; got path {path:?} in: {stdout}"
    );
}

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
    // R74: `items next-id` is a read-only path (either `<prefix>1` on a
    // missing ledger or read-only scan of existing ids), so it carries
    // `ReadIntegrityArgs` — no `--allow-outside` needed (and no longer
    // accepted on this subcommand). The test covers the missing-file fast
    // path which never touches the filesystem past `exists()`.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&missing)
        .arg("--prefix")
        .arg("R")
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::contains("R1"));
}

/// R40: `items next-id` no longer defaults `--prefix` to `R`. With four
/// ledger schemas in use (R review, O optimise, E execution-record, plus
/// any future), a silent R-default would mis-mint three of four callers.
/// Omitting `--prefix` now fails at the clap layer (exit 2, "required
/// arguments were not provided"), and the `--help` usage line pins the
/// flag as required.
#[test]
fn items_next_id_requires_prefix_flag() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    // 1. Omitting `--prefix` fails at parse time. clap's default terse
    //    message is "error: one or more required arguments were not
    //    provided" — we assert on `required` alone because the exact
    //    wording can shift between clap minor versions, but "required"
    //    is stable across them.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&missing)
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));

    // 2. `--help` usage line shows `--prefix <PREFIX>` without surrounding
    //    brackets (clap's notation for required flags is unadorned;
    //    optional flags appear as `[--flag <VAL>]`). This pins the
    //    schema-level requirement even if clap's error wording drifts.
    let help = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("next-id")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&help.get_output().stdout).to_string();
    assert!(
        stdout.contains("--prefix <PREFIX>"),
        "expected --help to show `--prefix <PREFIX>` as required, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("[--prefix"),
        "expected --prefix to NOT be shown as optional `[--prefix ...]`, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Task 4 (plan `docs/plans/tomlctl-capability-gaps.md`): `items next-id
// --infer-from-file` — scan the ledger's existing ids, infer the prefix, and
// mint the next monotonic one. Structurally mutually exclusive with
// `--prefix` via a required clap ArgGroup (one of the two must be passed;
// never both). R40 is preserved: zero-prefix invocations still fail.
// ---------------------------------------------------------------------------

/// T4 acceptance (a): ledger contains only E-prefixed ids. `--infer-from-file`
/// picks `E` (the single prefix in use) and emits `E{max_n+1}`. The fixture
/// uses `E1, E2, E5` to pin that the helper picks `max+1 = 6`, not
/// `len+1 = 4` — i.e. it walks the numeric suffixes, not the row count.
#[test]
fn items_next_id_infer_from_file_picks_sole_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("execution-record.toml");
    fs::write(
        &ledger,
        r#"schema_version = 1

[[items]]
id = "E1"
type = "status-transition"
summary = "seed"

[[items]]
id = "E2"
type = "status-transition"
summary = "seed"

[[items]]
id = "E5"
type = "task-completion"
summary = "seed"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&ledger)
        .arg("--infer-from-file")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    // Output shape matches the non-inferred path: one JSON-encoded string
    // literal per line (the existing `items next-id` contract).
    assert!(
        stdout.contains("\"E6\""),
        "expected `\"E6\"` in stdout, got:\n{stdout}"
    );
}

/// T4 acceptance (b): ledger contains multiple distinct prefixes. Inference
/// can't pick one without guessing, so the helper errors out with the exact
/// plan-specified message, prefixes sorted alphabetically for determinism
/// (`E, F, R` regardless of on-disk row order).
#[test]
fn items_next_id_infer_from_file_rejects_multiple_prefixes() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("mixed.toml");
    fs::write(
        &ledger,
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "review finding"

[[items]]
id = "E2"
summary = "execution record"

[[items]]
id = "F3"
summary = "future schema"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&ledger)
        .arg("--infer-from-file")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains(
            "--infer-from-file found multiple prefixes (E, F, R); pass --prefix explicitly"
        ),
        "expected multi-prefix error with alpha-sorted list, got stderr:\n{stderr}"
    );
}

/// T4 acceptance (c): empty ledger (no `[[items]]` entries) + no explicit
/// `--prefix`. Inference has nothing to work from; surface the "non-empty
/// ledger or explicit --prefix" guidance so the caller knows the remediation.
/// We pass an existing-but-item-less file so the empty-inference branch is
/// exercised (the missing-file branch raises the same message via the cli.rs
/// early return — both paths share the error text).
#[test]
fn items_next_id_infer_from_file_rejects_empty_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("empty.toml");
    fs::write(&ledger, "schema_version = 1\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&ledger)
        .arg("--infer-from-file")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains(
            "--infer-from-file requires a non-empty ledger or explicit --prefix"
        ),
        "expected empty-ledger error, got stderr:\n{stderr}"
    );
}

/// T4 acceptance (d): passing BOTH `--prefix` AND `--infer-from-file` must
/// fail at clap parse time (exit 2, not exit 1) because the flags live in a
/// `multiple(false)` ArgGroup. The error comes from clap directly, so we
/// assert on the group-conflict phrase rather than a tomlctl-authored string.
#[test]
fn items_next_id_prefix_and_infer_from_file_are_mutually_exclusive() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&missing)
        .arg("--prefix")
        .arg("R")
        .arg("--infer-from-file")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("argument cannot be used"),
        "expected clap group-conflict error, got stderr:\n{stderr}"
    );
}

/// R44: `items apply` rejects an ops array larger than `MAX_OPS_PER_APPLY`
/// with a message that names the count, the cap, and directs the user to
/// split the batch. The cap is a pre-write check, so the on-disk ledger is
/// untouched when it fires.
#[test]
fn items_apply_rejects_over_cap_ops_count() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("ledger.toml");
    let seed = r#"schema_version = 1

[[items]]
id = "R1"
summary = "seed"
status = "open"
"#;
    fs::write(&ledger, seed).unwrap();

    // 10_001 trivial no-op updates (target a non-existent id so the
    // per-op validation would eventually error if the cap didn't trip
    // first, but the cap check runs before mutation starts).
    let mut payload = String::from("[");
    for i in 0..10_001 {
        if i > 0 {
            payload.push(',');
        }
        payload.push_str(r#"{"op":"update","id":"R1","json":{"status":"open"}}"#);
    }
    payload.push(']');

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
        .failure()
        .stderr(
            predicate::str::contains("10001")
                .and(predicate::str::contains("10000"))
                .and(predicate::str::contains("split the batch")),
        );

    // Ledger byte-identical to the seed — cap is pre-write.
    let after = fs::read_to_string(&ledger).unwrap();
    assert_eq!(after, seed, "ledger must be unmodified after cap rejection");
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

// ---------------------------------------------------------------------------
// Task 6 additions — end-to-end coverage for the agent-native-tomlctl plan:
// `items add-many`, `array-append`, the expanded `items list` query surface,
// and the checked-in `.claude/settings.json` permissions shape.
// ---------------------------------------------------------------------------

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
        stdout.contains(r#""ok":true"#) && stdout.contains(r#""added":5"#),
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
        stdout.contains(r#""ok":true"#) && stdout.contains(r#""appended":1"#),
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
        stdout.contains(r#""ok":true"#) && stdout.contains(r#""appended":3"#),
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

// ---------------- R79: extended query-surface coverage ----------------
//
// Each test below exercises one flag of the `items list` query surface that
// was previously uncovered by the integration harness. They all share the
// 6-row `QUERY_FIXTURE` plus the `run_list_query` helper defined in
// `tests/common/mod.rs`, and stay under 25 lines so a CLI-surface break
// points at a single culprit.

#[test]
fn items_list_where_not_excludes_matches() {
    // status != open leaves only the two fixed rows (R3, R5).
    let stdout = run_list_query(&["--where-not", "status=open"]);
    assert_eq!(ids_from(&stdout), vec!["R3", "R5"]);
}

#[test]
fn items_list_where_in_matches_either() {
    // severity ∈ {major, critical} → R2 (major), R3 (critical), R4 (major),
    // R6 (critical). R1/R5 are `minor` and filtered out.
    let stdout = run_list_query(&["--where-in", "severity=major,critical"]);
    assert_eq!(ids_from(&stdout), vec!["R2", "R3", "R4", "R6"]);
}

#[test]
fn items_list_where_missing_excludes_present() {
    // Only R2 carries `symbol`; every other row is missing it.
    let stdout = run_list_query(&["--where-missing", "symbol"]);
    assert_eq!(ids_from(&stdout), vec!["R1", "R3", "R4", "R5", "R6"]);
}

#[test]
fn items_list_where_lt_strict() {
    // first_flagged < 2026-03-25 → only R1 (2026-03-10). Boundary is
    // strict-less, so R3 (exactly 2026-03-25) is excluded.
    let stdout = run_list_query(&["--where-lt", "first_flagged=@date:2026-03-25"]);
    assert_eq!(ids_from(&stdout), vec!["R1"]);
}

#[test]
fn items_list_where_lte_inclusive() {
    // first_flagged <= 2026-03-25 → R1 (2026-03-10) and R3 (2026-03-25).
    let stdout = run_list_query(&["--where-lte", "first_flagged=@date:2026-03-25"]);
    assert_eq!(ids_from(&stdout), vec!["R1", "R3"]);
}

#[test]
fn items_list_where_contains_substring() {
    // R4's summary is "n^2 loop" — the only row with "loop" in its summary.
    let stdout = run_list_query(&["--where-contains", "summary=loop"]);
    assert_eq!(ids_from(&stdout), vec!["R4"]);
}

#[test]
fn items_list_where_prefix_starts_with() {
    // R5's summary ("unused import") is the only one starting with "unused".
    let stdout = run_list_query(&["--where-prefix", "summary=unused"]);
    assert_eq!(ids_from(&stdout), vec!["R5"]);
}

#[test]
fn items_list_where_suffix_ends_with() {
    // All six rows have `file` ending in ".rs".
    let stdout = run_list_query(&["--where-suffix", "file=.rs"]);
    assert_eq!(ids_from(&stdout), vec!["R1", "R2", "R3", "R4", "R5", "R6"]);
}

#[test]
fn items_list_where_regex_matches() {
    // Only R2 carries `symbol = "old::fn"`. Match any `old::\w+`.
    let stdout = run_list_query(&["--where-regex", r"symbol=^old::\w+$"]);
    assert_eq!(ids_from(&stdout), vec!["R2"]);
}

#[test]
fn items_list_exclude_drops_fields() {
    // --exclude summary,symbol must strip those keys from every element while
    // keeping the rest of the projection intact.
    let stdout = run_list_query(&["--exclude", "summary,symbol", "--where", "id=R2"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("list output is a JSON array");
    assert_eq!(arr.len(), 1);
    let obj = arr[0].as_object().expect("element is an object");
    assert!(!obj.contains_key("summary"), "summary must be excluded, got {obj:?}");
    assert!(!obj.contains_key("symbol"), "symbol must be excluded, got {obj:?}");
    assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("R2"));
    assert_eq!(obj.get("severity").and_then(|v| v.as_str()), Some("major"));
}

#[test]
fn items_list_offset_skips_window() {
    // Sort ascending by first_flagged and skip the first two. Expected order:
    //   R1, R3, R2, R5, R4, R6  →  after offset=2: R2, R5, R4, R6.
    let stdout = run_list_query(&["--sort-by", "first_flagged", "--offset", "2"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("list output is a JSON array");
    let ids: Vec<&str> = arr
        .iter()
        .map(|el| el.get("id").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(ids, vec!["R2", "R5", "R4", "R6"]);
}

#[test]
fn items_list_sort_by_desc_reverses() {
    // --sort-by first_flagged:desc — newest first, limit 2 → R6 (2026-04-15),
    // R4 (2026-04-10). This pins that the `:desc` suffix is honoured end-to-end.
    let stdout = run_list_query(&["--sort-by", "first_flagged:desc", "--limit", "2"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("list output is a JSON array");
    let ids: Vec<&str> = arr
        .iter()
        .map(|el| el.get("id").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(ids, vec!["R6", "R4"]);
}

/// Typed-RHS coverage for `@int`, `@float`, `@bool`, `@string`. Uses a
/// dedicated fixture with non-string scalar fields (the main `QUERY_FIXTURE`
/// is string-and-date only) so each prefix exercises the apples-to-apples
/// compare path in `eq_typed` / `json_matches_toml`.
#[test]
fn items_list_where_typed_rhs_prefixes_match() {
    let fixture = r#"schema_version = 1

[[items]]
id = "A"
rounds = 3
weight = 1.5
active = true
[[items]]
id = "B"
rounds = 42
weight = 3.14
active = false
[[items]]
id = "C"
rounds = 7
weight = 2.0
active = true
"#;
    let (dir, ledger) = seed_ledger(fixture);
    let run = |flag: &str, val: &str| -> Vec<String> {
        let out = Command::cargo_bin("tomlctl")
            .unwrap()
            .env("TOMLCTL_ROOT", dir.path())
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .arg("items")
            .arg("list")
            .arg(&ledger)
            .arg(flag)
            .arg(val)
            .write_stdin("")
            .assert()
            .success();
        let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
        ids_from(&stdout)
    };
    assert_eq!(run("--where", "rounds=@int:42"), vec!["B"]);
    assert_eq!(run("--where", "weight=@float:3.14"), vec!["B"]);
    assert_eq!(run("--where", "active=@bool:true"), vec!["A", "C"]);
    // `@string:"A"` against a string `id` field — the quotes are part of the
    // RHS value (no JSON-shellquote stripping happens inside tomlctl).
    assert_eq!(run("--where", r#"id=@string:A"#), vec!["A"]);
}

/// R73: a malformed typed-RHS must surface as a non-zero exit + a clear
/// error that names both the bad RHS and the key under predicate. The
/// old behaviour silently dropped every row, so the user saw an empty
/// list and had no signal their filter was broken.
#[test]
fn items_list_typed_rhs_parse_error_bails_with_clear_message() {
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
first_flagged = 2026-04-18
"#;
    let (dir, ledger) = seed_ledger(fixture);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--where-gt")
        .arg("first_flagged=@date:not-a-date")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("first_flagged"),
        "error must name the predicate key; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("not-a-date"),
        "error must echo the bad RHS; stderr:\n{stderr}"
    );
}

// ---------------- R80: sidecar coverage on new write paths ----------------

#[test]
fn items_add_many_writes_sidecar() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let payload = "{\"id\":\"R1\",\"summary\":\"one\"}\n{\"id\":\"R2\",\"summary\":\"two\"}\n";
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add-many")
        .arg(&ledger)
        .arg("--ndjson")
        .arg("-")
        .arg("--defaults-json")
        .arg(r#"{"status":"open"}"#)
        .write_stdin(payload)
        .assert()
        .success();
    assert_sidecar_matches(&ledger);
}

#[test]
fn items_add_many_verify_integrity_success() {
    // After a write, `--verify-integrity items list` on the same ledger must
    // succeed — the sidecar's digest matches the just-written bytes.
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let payload = "{\"id\":\"R1\",\"summary\":\"one\"}\n";
    Command::cargo_bin("tomlctl")
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
        .success();

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--verify-integrity")
        .write_stdin("")
        .assert()
        .success();
}

#[test]
fn items_add_many_verify_integrity_detects_tampering() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let payload = "{\"id\":\"R1\",\"summary\":\"one\"}\n";
    Command::cargo_bin("tomlctl")
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
        .success();

    // Flip one hex nibble in the sidecar. The first char is `0-9a-f`; rotate
    // it by one so the file stays a valid 64-hex-char digest (otherwise the
    // "does not contain a 64-hex-char digest" branch fires instead of the
    // mismatch branch we want to exercise).
    let sidecar: PathBuf = {
        let mut s = ledger.as_os_str().to_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    let mut raw = fs::read_to_string(&sidecar).unwrap();
    let first = raw.as_bytes()[0] as char;
    let swapped = match first {
        '0'..='8' => ((first as u8) + 1) as char,
        '9' => 'a',
        'a'..='e' => ((first as u8) + 1) as char,
        'f' => '0',
        _ => panic!("sidecar does not start with a hex digit: {raw:?}"),
    };
    raw.replace_range(0..1, &swapped.to_string());
    fs::write(&sidecar, &raw).unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--verify-integrity")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("expected") && stderr.contains("actual"),
        "mismatch error must name both digests; got stderr:\n{stderr}"
    );
}

#[test]
fn array_append_writes_sidecar() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let payload = r#"{"timestamp":"2026-04-18T10:00:00Z","command":"review-apply","cause":"first","items":["R1"],"stash_ref":"stash@{0}"}"#;
    Command::cargo_bin("tomlctl")
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
    assert_sidecar_matches(&ledger);
}

#[test]
fn array_append_verify_integrity_detects_tampering() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let payload = r#"{"timestamp":"2026-04-18T10:00:00Z","command":"review-apply","cause":"x","items":["R1"],"stash_ref":"stash@{0}"}"#;
    Command::cargo_bin("tomlctl")
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

    let sidecar: PathBuf = {
        let mut s = ledger.as_os_str().to_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    let mut raw = fs::read_to_string(&sidecar).unwrap();
    let first = raw.as_bytes()[0] as char;
    let swapped = match first {
        '0'..='8' => ((first as u8) + 1) as char,
        '9' => 'a',
        'a'..='e' => ((first as u8) + 1) as char,
        'f' => '0',
        _ => panic!("sidecar does not start with a hex digit: {raw:?}"),
    };
    raw.replace_range(0..1, &swapped.to_string());
    fs::write(&sidecar, &raw).unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--verify-integrity")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("expected") && stderr.contains("actual"),
        "mismatch error must name both digests; got stderr:\n{stderr}"
    );
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

/// `integrity refresh` materialises the `.sha256` sidecar for an existing
/// file whose bytes were written outside tomlctl (the `/plan-new` bootstrap
/// uses the `Write` tool for the 2-line `execution-record.toml` skeleton,
/// bypassing the tomlctl write pipeline). The primary regression this
/// pins: before the subcommand existed, the first downstream
/// `tomlctl items list ... --verify-integrity` call against the
/// freshly-bootstrapped file failed with "sidecar ... is missing" because
/// no write had produced the sidecar yet.
#[test]
fn integrity_refresh_materialises_sidecar_for_bootstrapped_file() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude").join("flows").join("test");
    fs::create_dir_all(&claude_dir).unwrap();
    let target = claude_dir.join("execution-record.toml");
    // Simulate `/plan-new`'s Write: 2-line TOML skeleton, NO sidecar.
    fs::write(&target, "schema_version = 1\nlast_updated = 2026-04-21\n").unwrap();
    let sidecar = target.with_extension("toml.sha256");
    assert!(!sidecar.exists(), "precondition: sidecar must not exist before refresh");

    // A verify-integrity read must fail in this state — this is the
    // error the user hit that prompted this fix.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&target)
        .arg("--verify-integrity")
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("sidecar"))
        .stderr(predicate::str::contains("missing"));

    // `integrity refresh` creates the sidecar without modifying the TOML.
    let toml_before = fs::read(&target).unwrap();
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("integrity")
        .arg("refresh")
        .arg(&target)
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"));

    assert!(sidecar.exists(), "sidecar must exist after refresh");
    let toml_after = fs::read(&target).unwrap();
    assert_eq!(
        toml_before, toml_after,
        "refresh must not modify the TOML file"
    );
    // Sidecar must be in the canonical `<hex>  <basename>\n` format and
    // the digest must match the actual file contents.
    let sidecar_text = fs::read_to_string(&sidecar).unwrap();
    assert!(
        sidecar_text.ends_with("  execution-record.toml\n"),
        "sidecar must end with `  <basename>\\n`; got {sidecar_text:?}"
    );
    let hex = sidecar_text.split_whitespace().next().unwrap();
    assert_eq!(hex.len(), 64, "digest must be 64 hex chars");
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));

    // The downstream verify-integrity read now succeeds — this is the
    // behaviour `/plan-new`'s bootstrap relies on.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&target)
        .arg("--verify-integrity")
        .write_stdin("")
        .assert()
        .success();
}

/// `integrity refresh` on a non-existent file errors cleanly with
/// `kind=not_found` under `--error-format json` — mirroring the existing
/// missing-file taxonomy on read paths so agents can branch on the tag
/// rather than regexing prose.
#[test]
fn integrity_refresh_missing_file_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let missing = claude_dir.join("no-such-file.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("integrity")
        .arg("refresh")
        .arg(&missing)
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    // `guard_write_path` canonicalises against the parent when the file
    // is absent; since `.claude/` exists, canonicalisation succeeds and
    // the guard accepts — but `refresh_sidecar` then hits NotFound on
    // open. That's the path we want tagged. Assert on the JSON envelope
    // kind directly — the previous `|| stderr.contains("does not exist")`
    // fallback masked tag regressions because the prose is always
    // present in the inner error regardless of whether the NotFound tag
    // survives to the envelope.
    assert!(
        stderr.contains("\"kind\":\"not_found\""),
        "expected kind=not_found in JSON envelope; got stderr: {stderr}"
    );
}

/// `guard_write_path` auto-creates missing intermediate directories when
/// the target lands under `.claude/`, matching the Write tool's `mkdir -p`
/// semantics. This removes the confusing "parent directory ... not found
/// (os error 2/3)" error class that agents hit when calling
/// `tomlctl items add` against a not-yet-bootstrapped flow directory
/// (e.g. `.claude/flows/<new-slug>/execution-record.toml`).
///
/// T2 (doc-comment refresh): POST-T1 the write itself now SUCCEEDS rather
/// than failing at `read_toml` — auto-create seeds the missing target with
/// the schema-aware skeleton (`execution-record.toml` is a recognised flow
/// file, so it gets `schema_version = 1`), then applies the `set`. The
/// directory-creation side-effect this test originally pinned is still
/// exercised (the auto-`mkdir -p` runs before the seed), but the call no
/// longer exits non-zero. We therefore tighten the assertions to the
/// post-T1 contract: the command succeeds, the envelope reports
/// `"created":true`, the flow dir was created, the seeded file exists on
/// disk carrying `schema_version = 1`, and the pre-fix "parent directory"
/// confusion is absent from stderr.
#[test]
fn write_under_claude_auto_creates_missing_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let flow_dir = claude_dir.join("flows").join("hashed-kindling-engelbart");
    let target = flow_dir.join("execution-record.toml");
    assert!(!flow_dir.exists(), "precondition: flow dir must not exist yet");

    // POST-T1: the call now SUCCEEDS — the auto-`mkdir -p` creates the flow
    // directory, the missing target is seeded with the schema skeleton, and
    // the `set` lands on the seeded doc. Assert the success exit, the
    // `created:true` envelope, and the absence of the pre-fix confusion.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("set")
        .arg(&target)
        .arg("schema_version")
        .arg("1")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    // R9: parsed-JSON typed assertion on the envelope (created bool + a
    // non-empty path ending in the target filename) rather than a loose
    // substring match that would accept an empty/malformed path.
    assert_write_envelope(&stdout, true, "execution-record.toml");
    assert!(
        !stderr.contains("parent directory") && !stderr.contains("canonicalising write target"),
        "pre-fix error must not re-appear; got stderr: {stderr}"
    );
    assert!(
        flow_dir.exists(),
        "auto-mkdir must have created the flow directory"
    );
    // The seeded-then-set file carries the recognised-flow-file skeleton
    // (`schema_version = 1`), which the `set schema_version 1` left intact.
    let on_disk = fs::read_to_string(&target).expect("seeded file must exist on disk");
    assert!(
        on_disk.contains("schema_version = 1"),
        "seeded execution-record must carry schema_version = 1; got: {on_disk}"
    );
}

/// The auto-mkdir helper must NOT create directories outside `.claude/`.
/// When a write target sits outside the containment root and the parent
/// doesn't exist, the usual "parent directory ... not found" bail still
/// fires — we don't silently mkdir arbitrary paths, even with
/// `--allow-outside`. This pins the containment property of the helper:
/// creation is bounded by the same anchor the guard enforces.
#[test]
fn write_outside_claude_does_not_auto_create_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    // `.claude/` must exist so `repo_or_cwd_root()` has an anchor, but
    // the target deliberately sits outside it.
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    let outside_parent = dir.path().join("outside-dir");
    let outside_target = outside_parent.join("stray.toml");
    assert!(!outside_parent.exists(), "precondition: parent must not exist");

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("set")
        .arg(&outside_target)
        .arg("schema_version")
        .arg("1")
        // R24: `--allow-outside` is a per-subcommand `WriteIntegrityArgs`
        // flag, so it MUST follow the `set` positionals. Placed before the
        // subcommand it is rejected by clap (exit 2, "unexpected argument")
        // and the `.failure()` below would pass for the wrong reason —
        // never exercising the write-path containment it documents.
        .arg("--allow-outside")
        .write_stdin("")
        .assert()
        .failure();
    assert!(
        !outside_parent.exists(),
        "auto-mkdir must not fire outside .claude/, even under --allow-outside"
    );
}

/// `integrity refresh` refuses to operate on a file outside `.claude/`
/// unless `--allow-outside` is passed — mirrors the existing write-side
/// containment guard used by `set` / `items *` so a malicious artifacts
/// path can't trick us into dropping a sidecar next to an arbitrary
/// target.
#[test]
fn integrity_refresh_refuses_outside_claude_by_default() {
    let dir = tempfile::tempdir().unwrap();
    // Create `.claude/` so `repo_or_cwd_root()` + containment check have
    // an anchor, then put the target OUTSIDE it.
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    let outside = dir.path().join("outside.toml");
    fs::write(&outside, "x = 1\n").unwrap();

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("integrity")
        .arg("refresh")
        .arg(&outside)
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing to write outside .claude/"));
}

/// `integrity refresh` is idempotent on an unchanged file: running it
/// twice in a row produces byte-identical sidecar content. This pins
/// the "calling refresh is a no-op-ish" claim in SKILL.md — the TOML
/// bytes haven't changed, so the SHA-256 digest hasn't changed, so the
/// sidecar text (which is `<hex>  <basename>\n`, no embedded timestamp)
/// must be bit-for-bit the same. The on-disk mtime may differ because
/// `atomic_write` rewrites via a tempfile + rename each call; that's
/// fine — we assert on contents, not metadata.
#[test]
fn integrity_refresh_is_idempotent_on_unchanged_file() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude").join("flows").join("test");
    fs::create_dir_all(&claude_dir).unwrap();
    let target = claude_dir.join("execution-record.toml");
    fs::write(&target, "schema_version = 1\nlast_updated = 2026-04-21\n").unwrap();
    let sidecar = target.with_extension("toml.sha256");

    // First refresh — materialises the sidecar.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("integrity")
        .arg("refresh")
        .arg(&target)
        .write_stdin("")
        .assert()
        .success();
    assert!(sidecar.exists(), "sidecar must exist after first refresh");
    let sidecar_bytes_first = fs::read(&sidecar).unwrap();

    // Second refresh — bytes must be identical.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("integrity")
        .arg("refresh")
        .arg(&target)
        .write_stdin("")
        .assert()
        .success();
    let sidecar_bytes_second = fs::read(&sidecar).unwrap();
    assert_eq!(
        sidecar_bytes_first, sidecar_bytes_second,
        "sidecar contents must be byte-identical across two refreshes of an unchanged file"
    );
}

/// `integrity refresh --allow-outside` is the escape hatch for
/// operating on files outside `.claude/`. Sibling to
/// `integrity_refresh_refuses_outside_claude_by_default` — that test
/// pins the refusal branch; this pins the success branch so regressions
/// to the flag wiring can't silently reintroduce the containment guard
/// when the user explicitly opted out.
#[test]
fn integrity_refresh_allow_outside_writes_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    // `.claude/` exists so `repo_or_cwd_root()` resolves; target sits
    // OUTSIDE it. With `--allow-outside`, containment is bypassed and
    // the sidecar lands next to the target.
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    let outside = dir.path().join("outside.toml");
    fs::write(&outside, "x = 1\n").unwrap();
    let sidecar = outside.with_extension("toml.sha256");
    assert!(!sidecar.exists(), "precondition: sidecar must not exist before refresh");

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("integrity")
        .arg("refresh")
        .arg(&outside)
        .arg("--allow-outside")
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"));

    assert!(sidecar.exists(), "sidecar must exist after refresh --allow-outside");
    let sidecar_text = fs::read_to_string(&sidecar).unwrap();
    assert!(
        sidecar_text.ends_with("  outside.toml\n"),
        "sidecar must end with `  <basename>\\n`; got {sidecar_text:?}"
    );
    let hex = sidecar_text.split_whitespace().next().unwrap();
    assert_eq!(hex.len(), 64, "digest must be 64 hex chars");
    assert!(
        hex.chars().all(|c| c.is_ascii_hexdigit()),
        "digest must be all ASCII hex; got {hex:?}"
    );
}

/// `integrity refresh` on a zero-byte file hashes it correctly via the
/// streaming read loop's `n == 0` break — the digest must be the
/// well-known SHA-256 of the empty input
/// (`e3b0c442...b7852b855`). Pins the edge case where the read loop
/// could theoretically spin or mis-handle EOF-on-first-read.
#[test]
fn integrity_refresh_handles_zero_byte_file() {
    const EMPTY_SHA256: &str =
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude").join("flows").join("test");
    fs::create_dir_all(&claude_dir).unwrap();
    let target = claude_dir.join("execution-record.toml");
    fs::write(&target, b"").unwrap();
    assert_eq!(
        fs::metadata(&target).unwrap().len(),
        0,
        "precondition: target must be zero bytes"
    );
    let sidecar = target.with_extension("toml.sha256");

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("integrity")
        .arg("refresh")
        .arg(&target)
        .write_stdin("")
        .assert()
        .success();

    assert!(sidecar.exists(), "sidecar must exist after refresh");
    let sidecar_text = fs::read_to_string(&sidecar).unwrap();
    let hex = sidecar_text.split_whitespace().next().unwrap();
    assert_eq!(
        hex, EMPTY_SHA256,
        "zero-byte file must hash to the well-known SHA-256 of empty input; got {hex}"
    );
    assert!(
        sidecar_text.ends_with("  execution-record.toml\n"),
        "sidecar must end with `  <basename>\\n`; got {sidecar_text:?}"
    );
}

// ===== T2: `created` envelope/stderr surfacing =====================
//
// T1 auto-creates a missing write target (default) and computes a `created`
// bool that it discarded. T2 surfaces that bool: every write success envelope
// now carries `"created":<bool>` + `"path":<file>`, and a one-line stderr
// guidance fires only when a file was newly seeded (with a
// `(schema_version=1)` suffix for the four recognised flow files). These
// black-box tests drive the built binary so they exercise the real dispatch +
// stderr channel, mirroring the existing `assert_cmd` harness.

/// T2 (a): `items add` to a MISSING recognised flow file (`review-ledger.toml`
/// under `.claude/`) reports `"created":true` and lands the seeded file with
/// `schema_version = 1` on disk.
#[test]
fn items_add_to_missing_ledger_reports_created_true() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("review-ledger.toml");
    assert!(!ledger.exists(), "precondition: ledger must not exist yet");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","status":"open"}"#)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    // R9: parsed-JSON typed assertion — created:true AND a non-empty `path`
    // ending in the ledger filename. The old `contains(r#""path":"#)` accepted
    // an empty or malformed path; this pins the real shape.
    assert_write_envelope(&stdout, true, "review-ledger.toml");

    // The seeded file exists and carries the recognised-flow-file skeleton.
    let on_disk = fs::read_to_string(&ledger).expect("seeded ledger must exist on disk");
    assert!(
        on_disk.contains("schema_version = 1"),
        "seeded review-ledger must carry schema_version = 1; got: {on_disk}"
    );
    // The added row landed too.
    assert!(
        on_disk.contains(r#"id = "R1""#),
        "the added item must be persisted; got: {on_disk}"
    );
    assert_sidecar_matches(&ledger);
}

/// T2 (b): a SECOND `items add` to the now-existing ledger reports
/// `"created":false` (the file already exists, so nothing was seeded).
#[test]
fn items_add_to_existing_ledger_reports_created_false() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("review-ledger.toml");

    let run_add = |json: &str| {
        Command::cargo_bin("tomlctl")
            .unwrap()
            .env("TOMLCTL_ROOT", dir.path())
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .arg("items")
            .arg("add")
            .arg(&ledger)
            .arg("--json")
            .arg(json)
            .write_stdin("")
            .assert()
            .success()
    };

    // First add seeds the file.
    let first = run_add(r#"{"id":"R1","status":"open"}"#);
    let first_stdout = String::from_utf8_lossy(&first.get_output().stdout).to_string();
    // R9: parsed-JSON typed assertion (created bool + path-suffix) instead of
    // a loose substring match.
    assert_write_envelope(&first_stdout, true, "review-ledger.toml");

    // Second add to the now-existing file must report created:false.
    let second = run_add(r#"{"id":"R2","status":"open"}"#);
    let second_stdout = String::from_utf8_lossy(&second.get_output().stdout).to_string();
    assert_write_envelope(&second_stdout, false, "review-ledger.toml");
}

/// T2 (c) REGRESSION: a write to a PRE-EXISTING ledger reports
/// `"created":false` and leaves the file + its `.sha256` sidecar
/// byte-identical to the pre-write baseline when the write is a no-op
/// (a `--dedupe-by` add that matches an existing row). This pins the
/// invariant that surfacing `created` did not perturb the no-write path.
#[test]
fn write_to_existing_ledger_created_false_and_byte_identical_on_noop() {
    let (dir, ledger) = seed_ledger(
        "schema_version = 1\n\n[[items]]\nid = \"R1\"\nsource = \"a\"\ntarget = \"b\"\n",
    );
    // Refresh the sidecar so we have a baseline pair to compare against.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("integrity")
        .arg("refresh")
        .arg(&ledger)
        .write_stdin("")
        .assert()
        .success();

    let sidecar: PathBuf = {
        let mut s = ledger.as_os_str().to_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    let ledger_before = fs::read(&ledger).unwrap();
    let sidecar_before = fs::read(&sidecar).unwrap();

    // A dedupe add whose key matches the existing row is a no-op: no write,
    // no sidecar bump. It must report created:false.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--dedupe-by")
        .arg("source,target")
        .arg("--json")
        .arg(r#"{"id":"R2","source":"a","target":"b"}"#)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    // R9: parsed-JSON typed assertion on the created/path envelope. The
    // `added` field is dedupe-add-specific (not part of the created/path
    // shape), so it stays a typed field assertion alongside.
    let env = parse_write_envelope(&stdout);
    assert_eq!(
        env["ok"].as_bool(),
        Some(true),
        "no-op dedupe add must report ok:true; got: {stdout}"
    );
    assert_eq!(
        env["created"].as_bool(),
        Some(false),
        "no-op dedupe add to existing ledger must report created:false; got: {stdout}"
    );
    assert_eq!(
        env["added"].as_u64(),
        Some(0),
        "dedupe hit must report added:0; got: {stdout}"
    );

    let ledger_after = fs::read(&ledger).unwrap();
    let sidecar_after = fs::read(&sidecar).unwrap();
    assert_eq!(
        ledger_before, ledger_after,
        "no-op write must leave the ledger bytes byte-identical"
    );
    assert_eq!(
        sidecar_before, sidecar_after,
        "no-op write must leave the sidecar bytes byte-identical"
    );
}

/// T2 (d): an `items update` against a FRESHLY-MISSING ledger ERRORS (no
/// matching id on the seeded skeleton) and creates NO stray file — the
/// transactional "write-only-on-closure-success" property holds even when the
/// seed fired in memory. `items remove` behaves the same way.
#[test]
fn update_and_remove_against_missing_ledger_error_and_leave_no_file() {
    for op in ["update", "remove"] {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join(".claude");
        fs::create_dir_all(&claude).unwrap();
        let ledger = claude.join("review-ledger.toml");
        assert!(!ledger.exists(), "precondition: ledger must not exist yet");

        // `items update` / `items remove` take the id as a POSITIONAL arg
        // (`tomlctl items update <file> <id> …`), not a `--id` flag.
        let mut cmd = Command::cargo_bin("tomlctl").unwrap();
        cmd.env("TOMLCTL_ROOT", dir.path())
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .arg("items")
            .arg(op)
            .arg(&ledger)
            .arg("R99");
        if op == "update" {
            cmd.arg("--json").arg(r#"{"status":"fixed"}"#);
        }
        cmd.write_stdin("").assert().failure();

        assert!(
            !ledger.exists(),
            "`items {op}` against a missing ledger must NOT leave a stray seeded file"
        );
    }
}

/// T2 (e): the stderr guidance line appears WHEN AND ONLY WHEN a file was
/// created, carrying the `(schema_version=1)` suffix for a recognised flow
/// file and omitting it for an arbitrary `.toml`. A second write to the
/// now-existing file emits NO guidance line.
#[test]
fn created_stderr_guidance_fires_only_on_creation() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();

    // (i) Recognised flow file → guidance carries the schema suffix.
    let ledger = claude.join("review-ledger.toml");
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1"}"#)
        .write_stdin("")
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("created new file") && stderr.contains("(schema_version=1)"),
        "recognised flow file creation must emit the schema-suffixed guidance; got: {stderr}"
    );

    // (ii) Second add to the existing ledger → NO guidance line.
    let out2 = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R2"}"#)
        .write_stdin("")
        .assert()
        .success();
    let stderr2 = String::from_utf8_lossy(&out2.get_output().stderr).to_string();
    assert!(
        !stderr2.contains("created new file"),
        "no guidance line may fire when the file already exists; got: {stderr2}"
    );

    // (iii) Arbitrary `.toml` (not a recognised flow file) → guidance WITHOUT
    // the schema suffix. `set` auto-creates an empty-table-seeded file.
    let arbitrary = claude.join("scratch.toml");
    let out3 = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("set")
        .arg(&arbitrary)
        .arg("key")
        .arg("val")
        .write_stdin("")
        .assert()
        .success();
    let stderr3 = String::from_utf8_lossy(&out3.get_output().stderr).to_string();
    assert!(
        stderr3.contains("created new file") && !stderr3.contains("(schema_version=1)"),
        "arbitrary .toml creation must emit guidance WITHOUT the schema suffix; got: {stderr3}"
    );
}

// ===== R10: `--no-create` end-to-end ===============================
//
// T1 maps `--no-create` to `OnMissing::Error` (dispatch::on_missing_for). The
// clap-acceptance layer and the io.rs `read_or_seed`/`mutate_doc` primitive
// are unit-tested in isolation; this drives the BUILT binary so the wiring
// `--no-create → OnMissing::Error → kind=not_found, no file persisted` is
// covered end-to-end through the real dispatch path.

/// R10: `items add <missing-ledger> --json … --no-create` must FAIL with the
/// NotFound error and leave NO stray file on disk. We assert the typed error
/// kind (`kind=not_found`, the closed taxonomy `io::read_toml` tags a missing
/// file with) via `--error-format json`, which is the most shape-stable
/// surface; the message also names the missing path, which the JSON envelope's
/// `file` field carries.
#[test]
fn items_add_no_create_against_missing_ledger_errors_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("review-ledger.toml");
    assert!(!ledger.exists(), "precondition: ledger must not exist yet");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("add")
        .arg(&ledger)
        .arg("--json")
        .arg(r#"{"id":"R1","status":"open"}"#)
        .arg("--no-create")
        .write_stdin("")
        .assert()
        .failure();

    // (a) The error is tagged `not_found` and names the missing ledger.
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(
        err["kind"].as_str(),
        Some("not_found"),
        "--no-create against a missing ledger must surface kind=not_found; got: {stderr}"
    );
    let file = err["file"]
        .as_str()
        .expect("not_found envelope must carry the missing path in `file`");
    assert!(
        file.ends_with("review-ledger.toml"),
        "error `file` must name the missing ledger; got {file:?} in: {stderr}"
    );

    // (b) No stray file was seeded — `OnMissing::Error` short-circuits before
    // any persist.
    assert!(
        !ledger.exists(),
        "--no-create must NOT leave a stray seeded file on disk"
    );
}

// ===== R11: `mutate_doc_plan` created-reporting ====================
//
// The `items apply` / `items remove` write verbs route through
// `io::mutate_doc_plan` (a distinct entrypoint from `items add`'s `mutate_doc`).
// T2's `created` reporting must surface through THAT path too: an `apply` batch
// carrying an `add` op against a MISSING recognised flow file seeds the file
// (`created:true`) and lands `schema_version = 1` on disk.

/// R11: `items apply --ops '[{"op":"add",…}]'` against a MISSING `review-ledger.toml`
/// reports `created:true` through `mutate_doc_plan` and seeds the file with
/// `schema_version = 1` plus the added row. Parsed-JSON envelope assertions
/// (per R9).
#[test]
fn items_apply_add_op_to_missing_ledger_reports_created_true_via_mutate_doc_plan() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("review-ledger.toml");
    assert!(!ledger.exists(), "precondition: ledger must not exist yet");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("apply")
        .arg(&ledger)
        .arg("--ops")
        .arg(r#"[{"op":"add","json":{"id":"R1","status":"open"}}]"#)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    // R9-style parsed-JSON envelope: created:true + a path ending in the ledger
    // filename, proving the `created` signal surfaces through mutate_doc_plan.
    assert_write_envelope(&stdout, true, "review-ledger.toml");

    // The seeded file carries the recognised-flow-file skeleton and the add.
    let on_disk = fs::read_to_string(&ledger).expect("seeded ledger must exist on disk");
    assert!(
        on_disk.contains("schema_version = 1"),
        "apply-seeded review-ledger must carry schema_version = 1; got: {on_disk}"
    );
    assert!(
        on_disk.contains(r#"id = "R1""#),
        "the applied add op must be persisted; got: {on_disk}"
    );
    assert_sidecar_matches(&ledger);
}

/// R11 (sibling): `items remove` against a MISSING ledger errors (no matching
/// id on the freshly-seeded skeleton) and creates NO stray file — confirming
/// the `mutate_doc_plan` remove arm's documented behaviour (the
/// `compute_remove_mutation` `?` fires BEFORE the persist). This pins the
/// negative half of the mutate_doc_plan `created` story that
/// `update_and_remove_against_missing_ledger_error_and_leave_no_file` covers
/// for the generic case — here scoped explicitly to the remove arm's
/// no-stray-file property after the in-memory seed fired.
#[test]
fn items_remove_against_missing_ledger_errors_via_mutate_doc_plan_and_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("review-ledger.toml");
    assert!(!ledger.exists(), "precondition: ledger must not exist yet");

    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("remove")
        .arg(&ledger)
        .arg("R99")
        .write_stdin("")
        .assert()
        .failure();

    assert!(
        !ledger.exists(),
        "`items remove` against a missing ledger must NOT leave a stray seeded file"
    );
}

// ===== R20: P8 `--allow-outside` + auto-create stray file ==========
//
// INVARIANT: the `.claude/` containment guard is the safety boundary;
// `--allow-outside` is a deliberate double-opt-out the plan (P8) documents as
// permissive. `guard_write_path` SKIPS the auto-mkdir helper under
// `--allow-outside`, so a target whose parent does NOT exist still bails (that
// is `write_outside_claude_does_not_auto_create_parent_dirs`). This test
// pins the COMPLEMENT: when the outside parent ALREADY EXISTS, the
// containment check only WARNS (it does not block) and the auto-create write
// lands a stray file. The "outside" path stays INSIDE the tempdir root so the
// test never writes to a real filesystem location.

/// R20: under `--allow-outside`, an auto-create whose target's parent dir
/// already exists but is OUTSIDE `.claude/` DOES create the stray file — the
/// guard warns rather than blocks. Complements
/// `write_outside_claude_does_not_auto_create_parent_dirs` (mkdir refusal).
#[test]
fn allow_outside_auto_create_with_existing_parent_writes_stray_file() {
    let dir = tempfile::tempdir().unwrap();
    // `.claude/` must exist so `repo_or_cwd_root()` has an anchor.
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    // The "outside" parent is a SIBLING of `.claude/` under the tempdir root,
    // PRE-CREATED so the auto-mkdir refusal (skipped under --allow-outside
    // anyway) is irrelevant — `canonicalize_for_write` resolves the existing
    // parent and the containment check then only warns.
    let outside_parent = dir.path().join("outside-dir");
    fs::create_dir_all(&outside_parent).unwrap();
    let outside_target = outside_parent.join("stray.toml");
    assert!(
        !outside_target.exists(),
        "precondition: stray target must not exist yet"
    );

    // `--allow-outside` lives in the per-subcommand `WriteIntegrityArgs`
    // bundle (not a global flag), so it follows the `set` positionals.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("set")
        .arg(&outside_target)
        .arg("key")
        .arg("val")
        .arg("--allow-outside")
        .write_stdin("")
        .assert()
        .success();

    // The documented P8 behaviour: the stray file IS created (guard warns,
    // doesn't block) and carries the written content.
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("writing outside .claude/") && stderr.contains("--allow-outside"),
        "the guard must WARN (not block) when --allow-outside is set; got: {stderr}"
    );
    assert!(
        outside_target.exists(),
        "--allow-outside must let the auto-create land a stray file when the parent exists"
    );
    let on_disk = fs::read_to_string(&outside_target).expect("stray file must exist on disk");
    assert!(
        on_disk.contains(r#"key = "val""#),
        "stray file must carry the written content; got: {on_disk}"
    );
}
