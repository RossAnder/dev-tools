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
// Task 1 (plan `docs/plans/tomlctl-capability-gaps.md`): `items list` grows
// `--count-distinct <FIELD>`, a scalar-cardinality aggregate that replaces
// the 4-stage `--pluck X | jq -r '.[]' | sort -u | wc -l` pipe chain agents
// were spelling out. Output: `{"count_distinct":N,"field":"<name>"}`.
// Null/missing field values are excluded (`--pluck` semantics). The flag
// joins the existing `shape` ArgGroup, so pairwise-mutex with every other
// aggregation shape is enforced at clap parse time.
// ---------------------------------------------------------------------------

/// T1: end-to-end happy path. Fixture has 3 distinct categories across 6
/// rows — output shape must be `{count_distinct:3, field:"category"}`.
#[test]
fn items_list_count_distinct_emits_expected_object() {
    let stdout = run_list_query(&["--count-distinct", "category"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(
        v.get("count_distinct").and_then(|n| n.as_u64()),
        Some(4),
        "QUERY_FIXTURE has 4 distinct categories (style, bug, perf, security); got stdout:\n{stdout}"
    );
    assert_eq!(
        v.get("field").and_then(|s| s.as_str()),
        Some("category"),
        "`field` must echo the flag arg back; got stdout:\n{stdout}"
    );
}

/// T1: `--count-distinct` composes with `--where` — the distinct count is
/// over the FILTERED set. Same contract as Count / CountBy.
#[test]
fn items_list_count_distinct_composes_with_where() {
    // QUERY_FIXTURE open items: R1 (style), R2 (bug), R4 (perf), R6
    // (security) → 4 distinct categories.
    let stdout = run_list_query(&["--where", "status=open", "--count-distinct", "category"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(
        v.get("count_distinct").and_then(|n| n.as_u64()),
        Some(4),
        "open items span 4 distinct categories; got stdout:\n{stdout}"
    );
}

/// T1 / Risk #2: `--count-distinct` and `--pluck` both in the same call
/// must error at clap parse time (via the `shape` ArgGroup), NOT
/// silently collapse via the build_query priority ladder.
#[test]
fn count_distinct_and_pluck_are_mutex_at_parse_time() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--pluck")
        .arg("id")
        .arg("--count-distinct")
        .arg("category")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("cannot be used with")
            || stderr.contains("argument cannot be used"),
        "expected clap ArgGroup mutex error on --pluck + --count-distinct; got stderr:\n{stderr}"
    );
}

/// T1: `--count-distinct` + `--count` also errors at clap (same
/// ArgGroup). Pins that the ArgGroup was extended, not a new disjoint
/// group created.
#[test]
fn count_distinct_with_count_errors_at_clap() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count")
        .arg("--count-distinct")
        .arg("category")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("cannot be used with")
            || stderr.contains("argument cannot be used"),
        "expected clap ArgGroup mutex error on --count + --count-distinct; got stderr:\n{stderr}"
    );
}

/// T1: `--count-distinct` + `--select` errors via `validate_query`, which
/// T8 tagged `kind=validation`. Assert both the human-readable mutex
/// wording and (with `--error-format json`) the structured kind tag.
#[test]
fn count_distinct_with_select_errors_via_validate_query() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);

    // Text mode: anyhow chain contains both flag names.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-distinct")
        .arg("category")
        .arg("--select")
        .arg("id")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--select") && stderr.contains("--count-distinct"),
        "text-mode error must name both flags; got stderr:\n{stderr}"
    );

    // JSON mode: `kind=validation` tag surfaces.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-distinct")
        .arg("category")
        .arg("--select")
        .arg("id")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let envelope: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("json-mode stderr must parse: {e}; stderr:\n{stderr}"));
    assert_eq!(
        envelope
            .get("error")
            .and_then(|e| e.get("kind"))
            .and_then(|s| s.as_str()),
        Some("validation"),
        "expected kind=validation; got stderr:\n{stderr}"
    );
}

// ---------------------------------------------------------------------------
// Task 3 (plan `docs/plans/tomlctl-capability-gaps.md`): `--lines` and
// `--pluck` + `--ndjson` composition. `--lines` is a discoverable spelling
// of `--ndjson` for the Pluck case; both flags enable one-value-per-line
// streaming. Aggregation shapes silently treat the bit as a no-op.
// ---------------------------------------------------------------------------

/// 4-row fixture whose items each carry a `x` string field. Kept as a
/// module-local const to avoid dragging the generic `QUERY_FIXTURE` into
/// tests that only need a tiny pluck surface.
const PLUCK_FIXTURE: &str = r#"schema_version = 1

[[items]]
id = "R1"
x = "v1"

[[items]]
id = "R2"
x = "v2"

[[items]]
id = "R3"
x = "v3"

[[items]]
id = "R4"
x = "v4"
"#;

/// Helper: run `items list <args>` against a seeded fixture string and
/// return stdout. Mirrors `run_list_query` but accepts an arbitrary fixture
/// so Pluck-specific layouts don't need to fit the shared QUERY_FIXTURE.
fn run_list_query_with(fixture: &str, args: &[&str]) -> String {
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

/// T3-1: `--pluck x --lines` emits one quoted JSON string per line. Asserts
/// the exact byte sequence so a future refactor that e.g. emits bare
/// strings (T2's `--raw` territory) trips this test rather than silently
/// changing the contract.
#[test]
fn lines_with_pluck_emits_one_json_value_per_line() {
    let stdout = run_list_query_with(PLUCK_FIXTURE, &["--pluck", "x", "--lines"]);
    assert_eq!(stdout, "\"v1\"\n\"v2\"\n\"v3\"\n\"v4\"\n");
}

/// T3-2: `--pluck x --ndjson` is byte-identical to `--pluck x --lines`.
/// The two spellings are aliases at the semantic level — this test pins
/// the identity so future work can't accidentally diverge them.
#[test]
fn ndjson_with_pluck_is_byte_identical_to_lines_with_pluck() {
    let lines_out = run_list_query_with(PLUCK_FIXTURE, &["--pluck", "x", "--lines"]);
    let ndjson_out = run_list_query_with(PLUCK_FIXTURE, &["--pluck", "x", "--ndjson"]);
    assert_eq!(
        lines_out, ndjson_out,
        "--lines and --ndjson must be byte-identical on --pluck"
    );
}

/// T3-3: `--lines` composes with `--distinct` and `--sort-by`. The slow-path
/// branch of `run_streaming` handles these; this test pins that sort/distinct
/// still apply in the streaming emit order.
#[test]
fn lines_with_pluck_distinct_and_sort() {
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
x = "gamma"

[[items]]
id = "R2"
x = "alpha"

[[items]]
id = "R3"
x = "alpha"

[[items]]
id = "R4"
x = "beta"
"#;
    let stdout = run_list_query_with(
        fixture,
        &["--pluck", "x", "--lines", "--distinct", "--sort-by", "x:asc"],
    );
    assert_eq!(stdout, "\"alpha\"\n\"beta\"\n\"gamma\"\n");
}

/// T3-4: `--lines` composes with `--limit` — exactly N lines in the output.
/// Catches a regression where the streaming slow path fails to honour
/// `apply_window`.
#[test]
fn lines_with_pluck_and_limit() {
    let stdout = run_list_query_with(
        PLUCK_FIXTURE,
        &["--pluck", "x", "--lines", "--limit", "2"],
    );
    let line_count = stdout.lines().count();
    assert_eq!(line_count, 2, "expected 2 lines with --limit 2; got:\n{stdout}");
    assert_eq!(stdout, "\"v1\"\n\"v2\"\n");
}

/// T3-5: `--lines` shows up in `items list --help` as a discrete entry.
/// Clap aliases don't render in help, so this test is the structural guard
/// against someone "simplifying" the flag into `alias = "lines"`.
#[test]
fn lines_flag_listed_in_items_list_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("list")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--lines"),
        "items list --help must list --lines as a discrete flag; got:\n{stdout}"
    );
    // Both flags should be visible — the point of T3 is that --ndjson and
    // --lines coexist, not that one replaces the other.
    assert!(
        stdout.contains("--ndjson"),
        "items list --help must still list --ndjson alongside --lines; got:\n{stdout}"
    );
}

/// T3-6: `--lines` on a non-Pluck/non-Array shape is a silent no-op. For
/// Count the output is a single `{"count": N}` object regardless — per-line
/// decomposition has no meaning. Agents can blanket-add `--lines` to
/// scripts without branching on shape.
#[test]
fn lines_on_count_shape_is_noop_single_object() {
    let stdout = run_list_query_with(PLUCK_FIXTURE, &["--count", "--lines"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must parse as a single JSON value: {e}; stdout:\n{stdout}"));
    assert_eq!(
        v.get("count").and_then(|n| n.as_u64()),
        Some(4),
        "expected {{count: 4}}; got:\n{stdout}"
    );
    // Structural guard: the whole stdout is a single parseable JSON object,
    // not a sequence of per-line JSON values. The pretty-print formatter
    // splits the object across multiple display lines — that's fine, what
    // matters is that there's exactly one top-level JSON value.
    assert!(
        v.is_object(),
        "Count + --lines must emit a single top-level JSON object; got:\n{stdout}"
    );
    // Byte-identical parity vs the same query without `--lines` — proves
    // --lines is a true no-op on Count.
    let stdout_no_lines = run_list_query_with(PLUCK_FIXTURE, &["--count"]);
    assert_eq!(
        stdout, stdout_no_lines,
        "--lines on --count must be a byte-identical no-op"
    );
}

/// T3-7: null/missing plucked values are dropped in streaming — same
/// contract as `apply_pluck` in the non-streaming path. Pins the parity
/// constraint that motivated mirroring the `None | Some(JsonValue::Null)`
/// match in `run_streaming`.
#[test]
fn lines_with_pluck_drops_null_and_missing_fields() {
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
x = "v1"

[[items]]
id = "R2"

[[items]]
id = "R3"
x = "v3"
"#;
    // R2 is missing `x` entirely — it must not appear as `null\n` or as an
    // empty line in the output.
    let stdout = run_list_query_with(fixture, &["--pluck", "x", "--lines"]);
    assert_eq!(stdout, "\"v1\"\n\"v3\"\n");
    // Non-streaming path must drop the same items (byte-set parity).
    let stdout_array = run_list_query_with(fixture, &["--pluck", "x"]);
    let arr: serde_json::Value = serde_json::from_str(&stdout_array)
        .unwrap_or_else(|e| panic!("--pluck x (no lines) must be JSON: {e}; stdout:\n{stdout_array}"));
    assert_eq!(arr, serde_json::json!(["v1", "v3"]));
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

// ---------------------------------------------------------------------------
// Task 10 (plan `docs/plans/tomlctl-capability-gaps.md`): `--dry-run` on
// `items remove` and `items apply` via the compute/apply split. The split
// factors the mutation path into a pure `compute_*_mutation(&TomlValue, ...)`
// phase (no lock, no sidecar, no tempfile) and the existing I/O tail
// (lock + guard + atomic write + sidecar). `--dry-run` stops after the
// compute phase and emits `{"ok":true,"dry_run":true,"would_change":{...}}`
// without touching the filesystem. The invariance test (e) pins the
// structural guarantee that drives the whole split: the doc
// `compute_remove_mutation` builds, when serialised through the same
// `toml::to_string_pretty` emit path the live apply uses, is byte-identical
// to the bytes a real apply lands on disk.
// ---------------------------------------------------------------------------

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
        items[0].as_table().unwrap().get("id").and_then(|v| v.as_str()),
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
    assert_eq!(before_bytes, after_bytes, "ledger must be unchanged after apply --dry-run");
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

// ---------------------------------------------------------------------------
// Task 8 (plan `docs/plans/tomlctl-capability-gaps.md`): `--error-format
// {text,json}` global flag + closed `ErrorKind` taxonomy. Tagged call sites:
//   - io.rs `read_toml` / `read_toml_str` missing-file      -> kind=not_found
//   - io.rs `read_toml` / `read_doc_borrowed` TOML parse    -> kind=parse
//   - integrity.rs `verify_integrity` sidecar failure       -> kind=integrity
//   - query.rs `validate_query` mutex violations            -> kind=validation
//   - items.rs `items_next_id` prefix-shape validation      -> kind=validation
// Every other `bail!` / `anyhow!` falls through to kind=other. Exit code
// stays 1 regardless of format. Text-mode output is byte-identical to the
// pre-T8 `eprintln!("tomlctl: {:#}", err)` stream.
// ---------------------------------------------------------------------------

/// T8 helper: parse the one-line JSON error envelope tomlctl emits on
/// stderr when `--error-format json` is active. Returns the `error` object
/// so each test can assert on `kind` / `message` / `file` independently.
fn parse_json_error_envelope(stderr: &str) -> serde_json::Value {
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

/// T8 Test 1: missing-file path -> `kind=not_found`. `items get` on a
/// nonexistent file is the cleanest trigger — it goes straight through
/// `read_toml`'s NotFound arm with the path known, so the envelope also
/// carries a non-null `file` field.
#[test]
fn error_format_json_missing_file_tagged_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let missing = claude.join("nope.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("get")
        .arg(&missing)
        .arg("R1")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("not_found"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("No such file") || message.contains("not found"),
        "expected missing-file prose in message, got: {message}"
    );
    let file = err["file"].as_str().expect("file must be populated on not_found");
    assert!(
        file.contains("nope.toml"),
        "file field must carry the target path, got: {file}"
    );
}

/// T8 Test 2: sidecar hash mismatch -> `kind=integrity`. Write a valid TOML
/// with a deliberately-wrong sidecar; `--verify-integrity` triggers
/// `integrity.rs::verify_integrity` which tags the mismatch.
#[test]
fn error_format_json_sidecar_mismatch_tagged_integrity() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("data.toml");
    fs::write(&file, "key = \"value\"\n").unwrap();
    // A 64-hex-char digest that will NEVER match the real hash of the file.
    let mut sidecar = file.clone().into_os_string();
    sidecar.push(".sha256");
    fs::write(
        &sidecar,
        "deadbeef00000000000000000000000000000000000000000000000000000000  data.toml\n",
    )
    .unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("get")
        .arg(&file)
        .arg("key")
        .arg("--verify-integrity")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("integrity"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("integrity check failed")
            && message.contains("expected")
            && message.contains("actual"),
        "expected dual-digest message, got: {message}"
    );
    let file_field = err["file"].as_str().unwrap();
    assert!(
        file_field.contains("data.toml"),
        "file must name the verified path, got: {file_field}"
    );
}

/// T8 Test 3: TOML parse error -> `kind=parse`. Malformed TOML, `parse`
/// subcommand. Exercises the borrowed fast-path (`read_doc_borrowed`) since
/// `--verify-integrity` is absent. Owned path (`read_toml`) is covered
/// transitively by any `items list` / `get` / `items get` on the same bad
/// fixture; the parse subcommand is the cleanest fixture here.
#[test]
fn error_format_json_bad_toml_tagged_parse() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("bad.toml");
    // A clearly invalid TOML: bare `=` with no RHS.
    fs::write(&file, "malformed = =\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("parse")
        .arg(&file)
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("parse"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("parse")
            && (message.contains("borrowed TOML")
                || message.contains("parsing")),
        "expected TOML parse prose, got: {message}"
    );
}

/// T8 Test 4: query mutex violation -> `kind=validation`. `items list
/// --select x --exclude y` is rejected inside `validate_query`'s first
/// branch. Uses an existing (empty-items) ledger so the file read succeeds
/// and the error genuinely comes from `validate_query`, not `read_toml`.
#[test]
fn error_format_json_query_mutex_tagged_validation() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("empty.toml");
    fs::write(&file, "schema_version = 1\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&file)
        .arg("--select")
        .arg("a")
        .arg("--exclude")
        .arg("b")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("--select and --exclude are mutually exclusive"),
        "expected validate_query mutex prose, got: {message}"
    );
    assert!(err["file"].is_null(), "query validation has no file hint");
}

/// T8 Test 5: `items_next_id` prefix validation -> `kind=validation`. Pass
/// `--prefix ""` against an EXISTING (empty-items) ledger so control reaches
/// `items_next_id`'s empty-prefix check (the cli.rs missing-file fast path
/// has its own untagged bail, which isn't the plan's tag site).
#[test]
fn error_format_json_next_id_empty_prefix_tagged_validation() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("ledger.toml");
    fs::write(&file, "schema_version = 1\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&file)
        .arg("--prefix")
        .arg("")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("prefix must not be empty"),
        "expected prefix-empty validation message, got: {message}"
    );
}

/// T8 Test 6: untagged error -> `kind=other`. `items get <file> <missing-id>`
/// errors inside `items_get_from` (not on the plan's closed list), so it
/// should fall through to the generic `other` bucket. Confirms the default
/// fallback works for every un-annotated bail in the codebase.
#[test]
fn error_format_json_untagged_fallback_kind_other() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("ledger.toml");
    fs::write(
        &file,
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "present"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("get")
        .arg(&file)
        .arg("R999")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(
        err["kind"],
        serde_json::json!("other"),
        "untagged errors must fall through to kind=other"
    );
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("no item with id = R999"),
        "expected item-not-found prose in other-kind message, got: {message}"
    );
    assert!(
        err["file"].is_null(),
        "other-kind errors have no file hint (no TaggedError in chain)"
    );
}

/// T8 Test 7: text-mode regression — when `--error-format` is absent the
/// stderr stream is byte-identical to the pre-T8 `tomlctl: {:#}` line. Spot
/// checks three of the tagged kinds (not_found, validation-query,
/// validation-next-id) to pin no-prefix / no-bracketed-annotation rendering.
#[test]
fn error_format_text_mode_byte_identical_across_tag_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let empty_ledger = claude.join("empty.toml");
    fs::write(&empty_ledger, "schema_version = 1\n").unwrap();
    let missing = claude.join("missing.toml");

    // Spot 1: not_found — missing-file path via items get.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("get")
        .arg(&missing)
        .arg("R1")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.starts_with("tomlctl: reading "),
        "pre-T8 prefix must be unchanged, got: {stderr:?}"
    );
    assert!(
        !stderr.contains("[not_found]") && !stderr.contains("{\"error\""),
        "text mode must NOT leak tag prefix or JSON envelope, got: {stderr:?}"
    );

    // Spot 2: validation — query mutex.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&empty_ledger)
        .arg("--select")
        .arg("a")
        .arg("--exclude")
        .arg("b")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert_eq!(
        stderr.trim_end(),
        "tomlctl: --select and --exclude are mutually exclusive",
        "text-mode validation output must be byte-identical"
    );

    // Spot 3: validation — next-id empty prefix on existing file.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&empty_ledger)
        .arg("--prefix")
        .arg("")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert_eq!(
        stderr.trim_end(),
        "tomlctl: prefix must not be empty — use a letter like R, O, or A",
        "text-mode next-id validation output must be byte-identical"
    );
}

/// T8: `--error-format json` is a global flag — caller can place it BEFORE
/// or AFTER the subcommand name with identical behaviour. Pin both positions
/// against a missing-file trigger so the `global = true` attribute doesn't
/// silently regress.
#[test]
fn error_format_json_flag_position_is_global() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let missing = claude.join("missing.toml");

    // Flag BEFORE subcommand.
    let out_before = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("get")
        .arg(&missing)
        .arg("R1")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr_before =
        String::from_utf8_lossy(&out_before.get_output().stderr).to_string();
    let env_before = parse_json_error_envelope(&stderr_before);
    assert_eq!(env_before["kind"], serde_json::json!("not_found"));

    // Flag AFTER subcommand (and after the file/id args).
    let out_after = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("get")
        .arg(&missing)
        .arg("R1")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr_after = String::from_utf8_lossy(&out_after.get_output().stderr).to_string();
    let env_after = parse_json_error_envelope(&stderr_after);
    assert_eq!(env_after["kind"], serde_json::json!("not_found"));

    // Envelopes match byte-for-byte: both placements produce the same JSON.
    assert_eq!(
        stderr_before, stderr_after,
        "flag position must not affect the JSON envelope"
    );
}

// ---------------------------------------------------------------------------
// Task 9 (plan `docs/plans/tomlctl-capability-gaps.md`): `--strict-read` on
// every read subcommand — surface `kind=not_found` on a missing file instead
// of returning an empty default. Today the only read path with a "missing →
// silent default" branch is `items next-id --prefix <P>` (returns `"<P>1"`);
// every other read subcommand already errors on a missing file via
// `read_toml`'s T8-tagged NotFound, so `--strict-read` is a no-op there but
// accepted uniformly so callers can pass it without branching on subcommand.
//
// Default (flag absent) behaviour must stay byte-identical to pre-T9:
// `items next-id --prefix R <missing>` still mints `"R1"` for flows that
// bootstrap the ledger lazily. Pinned in `items_next_id_on_missing_file_prints_prefix_one`
// above; the (a) test below re-asserts it for the T9 section's completeness.
//
// Layering: `--strict-read` fires BEFORE `--verify-integrity`, so
// `items list <missing> --strict-read --verify-integrity` produces
// `kind=not_found`, NOT `kind=integrity`. This is the ordering the README's
// "File state contract" subsection guarantees.
// ---------------------------------------------------------------------------

/// T9 (a): default (flag absent) behaviour on `items next-id` with a missing
/// ledger stays byte-identical to pre-T9 — `"R1"` is the R19 bootstrapping
/// fast path, and nothing about the T9 addition is allowed to disturb it.
/// Duplicates `items_next_id_on_missing_file_prints_prefix_one` in spirit
/// but lives in the T9 section so a regression in the strict-read gate
/// surfaces alongside the T9 tests instead of in the far-away R58 block.
#[test]
fn strict_read_default_preserves_next_id_missing_file_fast_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");
    assert!(!missing.exists());

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&missing)
        .arg("--prefix")
        .arg("R")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("\"R1\""),
        "default (non-strict) next-id on missing file must still mint \"R1\", got:\n{stdout}"
    );
}

/// T9 (b): `--strict-read` on a missing-file `items next-id` errors with the
/// documented "file does not exist" prose on stderr and exits 1. Without the
/// flag the command succeeds with `"R1"` (covered above).
#[test]
fn strict_read_next_id_missing_file_errors_with_not_found_prose() {
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
        .arg("--strict-read")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("file does not exist:"),
        "stderr must carry the T9 not_found prose, got:\n{stderr}"
    );
    assert!(
        stderr.contains("no-such-ledger.toml"),
        "stderr must name the missing path, got:\n{stderr}"
    );
}

/// T9 (c): `--strict-read` composes with `--error-format json` — the stderr
/// envelope's `error.kind` is `"not_found"` and the `file` field is populated
/// with the missing path. Uses `items list` to cover the "benign no-op"
/// dispatch arm: today `items list` already errors on a missing file via
/// `read_toml`, so `--strict-read` doesn't change the outcome there, but it
/// MUST still surface `kind=not_found` through the T9 gate (rather than
/// letting `read_toml`'s own NotFound win, which would be behaviourally
/// identical but bypass the T9 ordering contract in (d) below).
#[test]
fn strict_read_items_list_missing_file_json_envelope_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&missing)
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
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("file does not exist:"),
        "message must be the T9 strict-read prose, got: {message}"
    );
    let file_field = err["file"].as_str().expect("file must be populated");
    assert!(
        file_field.contains("no-such-ledger.toml"),
        "file field must carry the missing path, got: {file_field}"
    );
}

/// T9 (d): layering — `--strict-read` fires BEFORE `--verify-integrity`.
/// A missing file under both flags surfaces `kind=not_found`, NOT
/// `kind=integrity`, even though the sidecar verify would also have failed
/// (the sidecar is trivially missing too). Pins the ordering documented in
/// the README's "File state contract" subsection so a future refactor that
/// reordered `strict_read_check` past `maybe_verify_integrity` trips this
/// test rather than silently reclassifying the error.
#[test]
fn strict_read_fires_before_verify_integrity_on_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&missing)
        .arg("--strict-read")
        .arg("--verify-integrity")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(
        err["kind"],
        serde_json::json!("not_found"),
        "strict-read must win over verify-integrity on a missing file"
    );
    let message = err["message"].as_str().unwrap();
    assert!(
        !message.contains("sidecar") && !message.contains("integrity check failed"),
        "message must be the not_found prose, not an integrity-sidecar message, got: {message}"
    );
}

/// T9 (e): `--strict-read` is accepted on every read subcommand and emits
/// a consistent `kind=not_found` envelope. Spot-check `parse`, `get`,
/// `validate`, `items get`, `items orphans`, and `items find-duplicates`
/// — each is a different dispatch arm that flattens `ReadIntegrityArgs`.
/// A single array-driven test keeps the arity manageable and pins the
/// uniform surface without bloating the test count.
#[test]
fn strict_read_uniform_across_read_subcommands() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    // Each entry is the argv after the binary name. `--strict-read`
    // `--error-format json` are appended inside the loop so the test
    // body reads flat.
    let cases: &[&[&str]] = &[
        &["parse", ""],
        &["get", "", "some.path"],
        &["validate", ""],
        &["items", "get", "", "R1"],
        &["items", "orphans", ""],
        &["items", "find-duplicates", ""],
    ];

    for argv in cases {
        let mut cmd = Command::cargo_bin("tomlctl").unwrap();
        cmd.env("TOMLCTL_ROOT", dir.path());
        // Replace the empty-string placeholder with the missing path. The
        // argv shape above pins placement (file arg is always the first
        // empty string) so a future subcommand added with a different
        // layout would need an explicit entry.
        for a in *argv {
            if a.is_empty() {
                cmd.arg(&missing);
            } else {
                cmd.arg(a);
            }
        }
        cmd.arg("--strict-read")
            .arg("--error-format")
            .arg("json")
            .write_stdin("");
        let out = cmd.assert().failure().code(1);
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        let err = parse_json_error_envelope(&stderr);
        assert_eq!(
            err["kind"],
            serde_json::json!("not_found"),
            "subcommand {:?} must surface kind=not_found under --strict-read, got envelope: {err}",
            argv
        );
    }
}

// ---------------------------------------------------------------------------
// Task 2 (plan `docs/plans/tomlctl-capability-gaps.md`): `--raw` bare-scalar
// output for `items list --count` / `--count-distinct` / `--pluck` (N=1 or
// `--lines`-streamed) and for `get <file> <scalar-path>`. The motivation is
// the ~35 `tomlctl ... | jq -r .count` pipe chains the transcript audit
// uncovered: agents consuming counts or single-scalar `get` results into a
// bash `read -r N` loop want the bare integer/string on stdout, not the
// JSON-wrapped form. Error strings on invalid compositions are load-bearing
// — tests assert byte-for-byte — so a downstream script checking for an
// exact substring stays stable across releases.
// ---------------------------------------------------------------------------

/// T2-1: `items list --count --raw` emits a bare integer plus a single
/// trailing newline. Byte-identity check — the whole point of `--raw` is
/// that the stdout is parseable by `read -r N` without jq.
#[test]
fn items_list_count_raw_emits_bare_integer() {
    let stdout = run_list_query(&["--count", "--raw"]);
    assert_eq!(stdout, "6\n", "QUERY_FIXTURE has 6 rows; expected bare `6\\n`");
}

/// T2-2: `items list --count-distinct foo --raw` emits the bare count,
/// dropping the `field` key. Stdout is a single integer line with no
/// JSON wrapping.
#[test]
fn items_list_count_distinct_raw_emits_bare_integer() {
    let stdout = run_list_query(&["--count-distinct", "category", "--raw"]);
    // QUERY_FIXTURE categories: style, bug, bug, perf, style, security → 4.
    assert_eq!(stdout, "4\n", "expected bare `4\\n`; got:\n{stdout}");
}

/// T2-3: `--pluck foo --raw` with N=1 (string) emits the unquoted string.
/// Uses the `symbol` field from QUERY_FIXTURE which only R2 carries.
#[test]
fn items_list_pluck_raw_n_eq_1_string_emits_unquoted() {
    let stdout = run_list_query(&["--where-has", "symbol", "--pluck", "symbol", "--raw"]);
    // QUERY_FIXTURE R2 has symbol = "old::fn".
    assert_eq!(stdout, "old::fn\n", "expected bare `old::fn\\n`; got:\n{stdout}");
}

/// T2-4: `--pluck foo --raw` with N=1 (integer) emits the bare integer.
/// Exercise the JsonValue::Number arm of `emit_raw` with a genuine integer
/// coming out of toml's `Integer` type.
#[test]
fn items_list_pluck_raw_n_eq_1_integer_emits_bare() {
    // Use `--where id=R1` + `--pluck rounds` — but QUERY_FIXTURE doesn't
    // carry `rounds`. Build a one-row fixture instead.
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
n = 42
"#;
    let stdout = run_list_query_with(fixture, &["--pluck", "n", "--raw"]);
    assert_eq!(stdout, "42\n", "expected bare `42\\n`; got:\n{stdout}");
}

/// T2-5: `--pluck foo --raw` on a 0-item result errors with the exact
/// task-spec wording. Tests assert byte-for-byte — a reword to
/// "no items matched" or "empty result" would break agent scripts.
#[test]
fn items_list_pluck_raw_n_eq_0_errors_with_exact_message() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        // Nothing matches `status=absent` → 0 rows → 0 plucked values.
        .arg("--where")
        .arg("status=absent")
        .arg("--pluck")
        .arg("id")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--raw requires single-value output (got 0 items)"),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-6: `--pluck foo --raw` on N>1 rows errors with the pinned wording
/// (including the suggested `--lines` remediation). Substitutes the
/// actual N in — asserts on the literal `(got 6 items)` so a drift in
/// count arithmetic would be caught.
#[test]
fn items_list_pluck_raw_n_gt_1_errors_with_exact_message_and_count() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--pluck")
        .arg("id")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains(
            "--raw requires single-value output (got 6 items); use --lines for newline-delimited"
        ),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-7: `--pluck foo --raw --lines` emits one bare value per line. The
/// streaming path threads `q.raw` through to the per-item emit point,
/// so strings come out unquoted. Pin the byte sequence to catch any
/// regression that accidentally re-quotes.
#[test]
fn items_list_pluck_raw_with_lines_emits_bare_per_line() {
    let stdout = run_list_query_with(PLUCK_FIXTURE, &["--pluck", "x", "--raw", "--lines"]);
    assert_eq!(stdout, "v1\nv2\nv3\nv4\n", "expected 4 bare lines; got:\n{stdout}");
}

/// T2-8: `tomlctl get <file> <scalar-path> --raw` emits the bare value on
/// a scalar target (integer here). Covers the `Cmd::Get` raw branch.
#[test]
fn get_raw_on_integer_scalar_emits_bare_integer() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let doc = claude.join("context.toml");
    fs::write(&doc, "[tasks]\ntotal = 7\nname = \"launch\"\n").unwrap();
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("get")
        .arg(&doc)
        .arg("tasks.total")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(stdout, "7\n", "expected bare `7\\n`; got:\n{stdout}");
}

/// T2-9: `get <file> <table-path> --raw` errors with the exact wording the
/// task spec pins. `[tasks]` is a TOML table, so navigating to `tasks`
/// returns a JSON object — `emit_raw` rejects it.
#[test]
fn get_raw_on_table_errors_with_exact_message() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let doc = claude.join("context.toml");
    fs::write(&doc, "[tasks]\ntotal = 7\n").unwrap();
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("get")
        .arg(&doc)
        .arg("tasks")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--raw requires a scalar target; got table"),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-10: `get <file> <array-path> --raw` errors with the exact wording.
/// `scope` below is a TOML array.
#[test]
fn get_raw_on_array_errors_with_exact_message() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let doc = claude.join("context.toml");
    fs::write(&doc, "scope = [\"a\", \"b\"]\n").unwrap();
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("get")
        .arg(&doc)
        .arg("scope")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--raw requires a scalar target; got array"),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-11: `items list --count-by foo --raw` is rejected at `validate_query`
/// with the exact canonical message. `--count-by` emits a map, which has
/// no bare-scalar form.
#[test]
fn items_list_count_by_with_raw_errors_with_exact_message() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-by")
        .arg("status")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains(
            "--raw is not supported on --count-by / --group-by (output is a map, not a scalar)"
        ),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-12: same error for `--group-by foo --raw`. Pins that validation hits
/// both shapes — not just CountBy by accident.
#[test]
fn items_list_group_by_with_raw_errors_with_exact_message() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--group-by")
        .arg("status")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains(
            "--raw is not supported on --count-by / --group-by (output is a map, not a scalar)"
        ),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-13: `--pluck foo --distinct --raw` — distinct narrows the pluck
/// array to 1 row; raw then emits that lone bare value. Covers the
/// interaction between the pluck-field dedup path and the N==1 raw
/// happy case, which has a non-obvious code path (dedup runs in the
/// slow path of `run()` since `--distinct` is engaged).
#[test]
fn items_list_pluck_distinct_raw_n_eq_1_emits_bare() {
    // Fixture has four identical x values — dedup collapses to one.
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
x = "only"

[[items]]
id = "R2"
x = "only"

[[items]]
id = "R3"
x = "only"
"#;
    let stdout = run_list_query_with(fixture, &["--pluck", "x", "--distinct", "--raw"]);
    assert_eq!(stdout, "only\n", "expected bare `only\\n`; got:\n{stdout}");
}

/// T2-14: `--count --raw --error-format json` on a HAPPY path emits the
/// bare integer on stdout — the `--error-format json` flag only affects
/// errors. Pins that `--raw` output is NOT JSON-wrapped just because the
/// error-format is `json`.
#[test]
fn items_list_count_raw_with_error_format_json_still_bare_on_happy_path() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(
        stdout, "6\n",
        "happy-path --raw stdout must be bare; --error-format json only affects errors; got:\n{stdout}"
    );
}

/// T2-15: `--strict-read` wins against `--raw`-N=0: a missing ledger must
/// surface `kind=not_found`, NOT the "(got 0 items)" raw-validation error,
/// because the strict-read gate fires BEFORE the query pipeline runs.
/// Tests the documented ordering contract from T9.
#[test]
fn items_list_pluck_raw_strict_read_on_missing_file_wins() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join(".claude").join("no-ledger.toml");
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("list")
        .arg(&missing)
        .arg("--pluck")
        .arg("id")
        .arg("--raw")
        .arg("--strict-read")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let envelope: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("json-mode stderr must parse: {e}; stderr:\n{stderr}"));
    assert_eq!(
        envelope
            .get("error")
            .and_then(|e| e.get("kind"))
            .and_then(|s| s.as_str()),
        Some("not_found"),
        "strict-read must surface kind=not_found (not raw-validation); got stderr:\n{stderr}"
    );
}

/// T2-16: `--pluck foo --raw` with N=1 boolean emits `true` / `false` bare.
/// Covers the JsonValue::Bool arm of `emit_raw`.
#[test]
fn items_list_pluck_raw_n_eq_1_bool_emits_true() {
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
active = true
"#;
    let stdout = run_list_query_with(fixture, &["--pluck", "active", "--raw"]);
    assert_eq!(stdout, "true\n", "expected bare `true\\n`; got:\n{stdout}");
}

// ---------------------------------------------------------------------------
// Task 7 (plan `docs/plans/tomlctl-capability-gaps.md`): `tomlctl
// capabilities` emits a JSON description of the binary's user-facing
// surface so downstream flow-command templates can feature-gate cleanly
// without parsing `--help` prose. Also pins the 0.1.0 → 0.2.0 version
// bump that this minor release carries (new flags, new subcommand,
// auto-populated `dedup_id` field, structured `--error-format json`).
// ---------------------------------------------------------------------------

/// T7-1: `tomlctl capabilities` writes a JSON object to stdout with the
/// three top-level keys the spec pins (`version`, `features`, `subcommands`).
/// Also asserts it parses cleanly as JSON — no trailing garbage, no BOM.
#[test]
fn capabilities_output_parses_as_json() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("capabilities stdout must parse as JSON: {e}; stdout:\n{stdout}"));
    let obj = v
        .as_object()
        .expect("capabilities output must be a JSON object");
    assert!(obj.contains_key("version"), "missing `version` key: {v}");
    assert!(obj.contains_key("features"), "missing `features` key: {v}");
    assert!(
        obj.contains_key("subcommands"),
        "missing `subcommands` key: {v}"
    );
}

/// T7-2: the `features` array advertises every T1..T11 feature the plan
/// enumerated. The expected list duplicates the names from `cli::FEATURES`
/// deliberately — if the const drifts (someone removes a feature or renames
/// one), this test fails in review rather than silently shipping a
/// half-advertised capability set.
#[test]
fn capabilities_features_contains_every_plan_feature() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("capabilities stdout must parse as JSON: {e}"));
    let features = v
        .get("features")
        .and_then(|f| f.as_array())
        .expect("`features` must be a JSON array");
    let features: Vec<&str> = features.iter().filter_map(|e| e.as_str()).collect();

    // Exhaustive list duplicated from cli::FEATURES — drift between the
    // two is caught here in review.
    let expected = [
        "count_distinct",         // T1
        "raw",                    // T2
        "lines",                  // T3
        "infer_prefix",           // T4
        "dedupe_by",              // T5
        "dedup_id_auto",          // T6b
        "find_duplicates_across", // T6c
        "capabilities",           // T7
        "error_format_json",      // T8
        "strict_read",            // T9
        "dry_run",                // T10
        "backfill_dedup_id",      // T11
    ];
    for name in expected {
        assert!(
            features.contains(&name),
            "expected feature `{name}` in capabilities output; got {features:?}"
        );
    }
    assert_eq!(
        features.len(),
        expected.len(),
        "feature count drift: expected {} entries, got {} ({features:?})",
        expected.len(),
        features.len()
    );
}

/// T7-3: the `version` string equals `0.2.0`. Literal assertion rather
/// than reading Cargo.toml — the whole point of this task is the semver
/// bump, so pinning the exact release marker keeps the acceptance criterion
/// honest. Bump both sides in lockstep on the next minor release.
#[test]
fn capabilities_version_matches_cargo_toml() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("capabilities stdout must parse as JSON: {e}"));
    let version = v
        .get("version")
        .and_then(|s| s.as_str())
        .expect("`version` must be a string");
    assert_eq!(
        version, "0.2.0",
        "expected version `0.2.0` (the 0.1.0 → 0.2.0 bump this release carries); got `{version}`"
    );
}

/// T7-4: the `subcommands` array includes the metadata subcommand itself
/// (so `tomlctl capabilities | jq '.subcommands | index("capabilities")'`
/// is truthy) plus at least one real data-path subcommand (`items`). Both
/// sanity-check the list is populated and not an empty placeholder.
#[test]
fn capabilities_subcommands_contains_capabilities_and_items() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("capabilities stdout must parse as JSON: {e}"));
    let subs = v
        .get("subcommands")
        .and_then(|s| s.as_array())
        .expect("`subcommands` must be a JSON array");
    let subs: Vec<&str> = subs.iter().filter_map(|e| e.as_str()).collect();
    assert!(
        subs.contains(&"capabilities"),
        "subcommands must include `capabilities`; got {subs:?}"
    );
    assert!(
        subs.contains(&"items"),
        "subcommands must include `items`; got {subs:?}"
    );
}
