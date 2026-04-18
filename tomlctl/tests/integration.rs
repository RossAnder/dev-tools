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

// ---------------- R79: extended query-surface coverage ----------------
//
// Each test below exercises one flag of the `items list` query surface that
// was previously uncovered by the integration harness. They all share the
// 6-row `QUERY_FIXTURE` plus the `run_list_query` helper defined above, and
// stay under 25 lines so a CLI-surface break points at a single culprit.

/// Helper: parse `run_list_query` JSON output into a sorted Vec<String> of
/// `id` fields. Keeps the per-flag tests one-liner-ish without swallowing the
/// panic-on-bad-JSON contract already baked into `run_list_query`.
fn ids_from(stdout: &str) -> Vec<String> {
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

/// Read the `<file>.sha256` sidecar and assert it has the canonical
/// `sha256sum` format with a digest matching the current file bytes.
fn assert_sidecar_matches(ledger: &Path) {
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

/// R74: read-only subcommands (`parse`, `get`, `validate`, `items list`,
/// `items get`, `items find-duplicates`, `items orphans`, `items next-id`)
/// must NOT expose the write-side integrity flags (`--allow-outside`,
/// `--no-write-integrity`, `--strict-integrity`). They still accept
/// `--verify-integrity` because that's the only read-side integrity
/// concept. A test-per-flag per-subcommand would be noisy — inspect the
/// rendered `--help` text and assert the write-side flags don't appear.
#[test]
fn read_only_subcommands_hide_write_integrity_flags_in_help() {
    let read_subs: &[&[&str]] = &[
        &["parse", "--help"],
        &["get", "--help"],
        &["validate", "--help"],
        &["items", "list", "--help"],
        &["items", "get", "--help"],
        &["items", "find-duplicates", "--help"],
        &["items", "orphans", "--help"],
        &["items", "next-id", "--help"],
    ];
    for path in read_subs {
        let mut cmd = Command::cargo_bin("tomlctl").unwrap();
        for a in *path {
            cmd.arg(a);
        }
        let assert = cmd.write_stdin("").assert().success();
        let stdout =
            String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        // --verify-integrity is allowed on read paths; present is fine.
        for banned in ["--allow-outside", "--no-write-integrity", "--strict-integrity"] {
            assert!(
                !stdout.contains(banned),
                "read-only sub `{}` must NOT list `{}` in --help; got:\n{}",
                path.join(" "),
                banned,
                stdout
            );
        }
    }
}

/// R74 (complement): write subcommands MUST continue to list every integrity
/// flag in `--help`. Pins the structural guarantee that the split didn't
/// accidentally strip a flag from a writer.
#[test]
fn write_subcommands_expose_all_integrity_flags_in_help() {
    let write_subs: &[&[&str]] = &[
        &["set", "--help"],
        &["set-json", "--help"],
        &["array-append", "--help"],
        &["items", "add", "--help"],
        &["items", "update", "--help"],
        &["items", "remove", "--help"],
        &["items", "apply", "--help"],
        &["items", "add-many", "--help"],
    ];
    for path in write_subs {
        let mut cmd = Command::cargo_bin("tomlctl").unwrap();
        for a in *path {
            cmd.arg(a);
        }
        let assert = cmd.write_stdin("").assert().success();
        let stdout =
            String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        for required in [
            "--allow-outside",
            "--no-write-integrity",
            "--verify-integrity",
            "--strict-integrity",
        ] {
            assert!(
                stdout.contains(required),
                "write sub `{}` must list `{}` in --help; got:\n{}",
                path.join(" "),
                required,
                stdout
            );
        }
    }
}

/// R76: `--count`, `--count-by`, `--group-by`, `--pluck` are declared as a
/// mutually exclusive clap ArgGroup on `items list`. Two of them on the
/// same command must fail at parse time with clap's "cannot be used with"
/// error — not silently collapse to one shape via the `build_query`
/// priority ladder. `--ndjson` is orthogonal (a separate output encoding,
/// not a shape) and is NOT in the group.
#[test]
fn items_list_shape_flags_are_mutually_exclusive_at_parse_time() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count")
        .arg("--count-by")
        .arg("status")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("cannot be used with")
            || stderr.contains("argument cannot be used"),
        "expected clap mutex error, got stderr:\n{stderr}"
    );
    // --ndjson + --count-by must still be parse-accepted (they're orthogonal;
    // the runtime may still reject it via validate_query, but it MUST NOT be
    // rejected by the ArgGroup). Only assert that stderr does NOT carry the
    // ArgGroup mutex phrase for this pair.
    let out2 = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-by")
        .arg("status")
        .arg("--ndjson")
        .write_stdin("")
        .assert();
    // Accept either success OR a validate-layer runtime error — just not
    // the clap ArgGroup "cannot be used" phrase, which would mean --ndjson
    // leaked into the shape group by mistake.
    let stderr2 = String::from_utf8_lossy(&out2.get_output().stderr).to_string();
    assert!(
        !stderr2.contains("cannot be used with"),
        "--ndjson must stay OUTSIDE the shape ArgGroup (R82 + R76); got stderr:\n{stderr2}"
    );
}

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
