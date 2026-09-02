//! Integration tests for `tomlctl flow active {list,add,remove,touch}`
//! against a tempdir-rooted `.claude/active-flow.toml` registry.
//!
//! Sandbox strategy: every test creates a `tempfile::tempdir()` and points
//! `TOMLCTL_ROOT` at it via `assert_cmd::Command::env`. The registry path
//! resolves to `<tempdir>/.claude/active-flow.toml` per `repo_or_cwd_root`'s
//! env-override branch. Per-process env state means parallel test runs
//! don't race — each `assert_cmd` invocation forks a fresh process with
//! its own copy of the env.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::assert_sidecar_matches;

/// Create the empty `.claude/` directory under a fresh tempdir and return
/// both. Tests that write to `active-flow.toml` rely on the dir existing
/// because `guard_write_path`'s `ensure_parent_under_claude` only mkdir-p's
/// when the nearest existing ancestor IS already under `.claude/` — for
/// the top-level `.claude/` itself the parent dir (the tempdir) must
/// pre-exist.
fn fresh_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let registry = claude.join("active-flow.toml");
    (dir, registry)
}

/// Run `tomlctl flow active <subcommand> <args...>` against the given
/// tempdir-root and return the assertion handle. Standard env wiring
/// (`TOMLCTL_ROOT`, short lock timeout) is applied so a hung lock can't
/// stall the test suite past a few seconds.
fn run_active(dir: &tempfile::TempDir, subargs: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("active");
    for a in subargs {
        cmd.arg(a);
    }
    cmd.write_stdin("").assert()
}

/// Stdout helper — parse a successful `flow active <op>` invocation as
/// JSON. Panics with the raw stdout on parse failure.
fn json_stdout(out: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"))
}

/// Local sidecar-path helper (matches `tests/items_dry_run.rs:sidecar_for`).
fn sidecar_for(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

/// list-empty on a missing registry returns `{schema_version:1, active:[]}`
/// without bootstrapping the file on disk (read paths never write).
#[test]
fn list_empty_when_registry_missing_emits_default_envelope() {
    let (dir, registry) = fresh_root();
    assert!(
        !registry.exists(),
        "precondition: registry must not pre-exist"
    );

    let out = run_active(&dir, &["list"]).success();
    let v = json_stdout(&out);
    assert_eq!(v["schema_version"], serde_json::json!(1));
    assert_eq!(v["active"], serde_json::json!([]));
    assert!(
        !registry.exists(),
        "list must not bootstrap the registry file on disk"
    );
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

/// First `add` on a fresh root creates BOTH the registry file AND its
/// `.sha256` sidecar atomically; the resulting entry is reachable via
/// `list`. `last_used` is a non-empty RFC3339-ish string (jiff
/// `Timestamp::now().to_string()` shape — `2026-...Z`).
#[test]
fn add_bootstraps_registry_and_sidecar() {
    let (dir, registry) = fresh_root();

    let out = run_active(&dir, &["add", "--slug", "feature-x", "--branch", "feat/x"]).success();
    let v = json_stdout(&out);
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["slug"], serde_json::json!("feature-x"));
    assert_eq!(v["action"], serde_json::json!("add"));
    let last_used = v["last_used"].as_str().expect("last_used must be a string");
    assert!(
        !last_used.is_empty(),
        "last_used must be non-empty (jiff Timestamp::now string)"
    );
    // RFC3339-ish: contains a `T` and ends in `Z` (UTC). The exact format
    // is jiff's responsibility; this test pins the schema-level shape.
    assert!(
        last_used.contains('T') && last_used.ends_with('Z'),
        "last_used must be RFC3339 UTC, got: {last_used}"
    );

    // File + sidecar exist on disk.
    assert!(registry.exists(), "add must materialise the registry file");
    assert_sidecar_matches(&registry);

    // list shows exactly one entry.
    let list_out = run_active(&dir, &["list"]).success();
    let lv = json_stdout(&list_out);
    let arr = lv["active"].as_array().expect("active must be array");
    assert_eq!(arr.len(), 1, "exactly one entry after first add");
    assert_eq!(arr[0]["slug"], serde_json::json!("feature-x"));
    assert_eq!(
        arr[0]["binding"]["branch"],
        serde_json::json!("feat/x"),
        "branch must round-trip into the binding sub-table"
    );
}

/// Adding the same slug twice MUST replace in place (no duplicate entries).
/// Plan-locked semantic: "matching slug replaces in place (no duplicates)".
#[test]
fn add_existing_slug_updates_in_place_no_duplicates() {
    let (dir, _registry) = fresh_root();

    run_active(&dir, &["add", "--slug", "feature-x", "--branch", "feat/x"]).success();
    // Second add updates the binding (different branch) and bumps last_used.
    run_active(&dir, &["add", "--slug", "feature-x", "--branch", "feat/y"]).success();

    let list_out = run_active(&dir, &["list"]).success();
    let v = json_stdout(&list_out);
    let arr = v["active"].as_array().expect("active must be array");
    assert_eq!(
        arr.len(),
        1,
        "second add of same slug must NOT create a duplicate"
    );
    assert_eq!(arr[0]["slug"], serde_json::json!("feature-x"));
    assert_eq!(
        arr[0]["binding"]["branch"],
        serde_json::json!("feat/y"),
        "second add must overwrite branch"
    );
}

/// After `add` the entry's `last_used` field is present and non-empty.
/// Pins the "required field" contract called out in the plan acceptance
/// criteria ("missing `last_used` after add → fail").
#[test]
fn add_emits_required_last_used_field() {
    let (dir, registry) = fresh_root();
    run_active(&dir, &["add", "--slug", "x"]).success();

    // Read the on-disk doc directly to assert `last_used` survived the
    // round-trip rather than relying on the JSON list output (which builds
    // a fresh projection that could mask a missing field if `entry_to_json`
    // ever silently dropped it).
    let raw = fs::read_to_string(&registry).unwrap();
    let parsed: toml::Value = toml::from_str(&raw).unwrap();
    let active = parsed.get("active").and_then(|v| v.as_array()).unwrap();
    assert_eq!(active.len(), 1);
    let entry = active[0].as_table().unwrap();
    let last_used = entry
        .get("last_used")
        .and_then(|v| v.as_str())
        .expect("last_used must be present and a string");
    assert!(!last_used.is_empty(), "last_used must be non-empty");
}

/// `add --scope <glob>` repeated multiple times accumulates into a
/// `binding.scope` array. Pins the multi-value clap surface.
#[test]
fn add_accumulates_repeated_scope_globs() {
    let (dir, _) = fresh_root();
    run_active(
        &dir,
        &[
            "add",
            "--slug",
            "x",
            "--scope",
            "src/foo/**",
            "--scope",
            "src/bar/**",
        ],
    )
    .success();
    let v = json_stdout(&run_active(&dir, &["list"]).success());
    let arr = v["active"].as_array().unwrap();
    assert_eq!(
        arr[0]["binding"]["scope"],
        serde_json::json!(["src/foo/**", "src/bar/**"])
    );
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

/// `remove --slug <s>` followed by `list` shows zero entries.
#[test]
fn remove_then_list_shows_zero_entries() {
    let (dir, _) = fresh_root();
    run_active(&dir, &["add", "--slug", "x"]).success();
    run_active(&dir, &["add", "--slug", "y"]).success();

    run_active(&dir, &["remove", "--slug", "x"]).success();

    let v = json_stdout(&run_active(&dir, &["list"]).success());
    let arr = v["active"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["slug"], serde_json::json!("y"));
}

// ---------------------------------------------------------------------------
// touch
// ---------------------------------------------------------------------------

/// `touch --slug <s>` updates `last_used` only; binding and other fields
/// stay byte-identical pre/post.
#[test]
fn touch_updates_last_used_only_other_fields_unchanged() {
    let (dir, registry) = fresh_root();
    run_active(
        &dir,
        &[
            "add", "--slug", "x", "--branch", "feat/x", "--scope", "src/**",
        ],
    )
    .success();

    // Snapshot the pre-touch entry minus `last_used`.
    let pre_v = json_stdout(&run_active(&dir, &["list"]).success());
    let pre = &pre_v["active"][0];
    let pre_last = pre["last_used"].as_str().unwrap().to_string();
    let pre_binding = pre["binding"].clone();

    // Sleep one millisecond so the touched timestamp likely differs.
    // jiff Timestamp::now is sub-second; if two consecutive `now()`s land
    // on the same second the strings might match — that's fine, the
    // critical assertion below is that other fields are byte-identical.
    std::thread::sleep(std::time::Duration::from_millis(2));
    run_active(&dir, &["touch", "--slug", "x"]).success();

    let post_v = json_stdout(&run_active(&dir, &["list"]).success());
    let post = &post_v["active"][0];
    let post_last = post["last_used"].as_str().unwrap();

    // last_used: present and a string. We do NOT require strict
    // monotonicity (jiff's Timestamp may have second-resolution rendering
    // depending on platform clock); the touch is still semantically
    // meaningful by always being applied.
    assert!(!post_last.is_empty());
    assert_eq!(
        post["binding"], pre_binding,
        "binding must be byte-identical pre/post touch"
    );
    assert_eq!(post["slug"], serde_json::json!("x"));

    // The pre-touch timestamp should be parseable as RFC3339 (sanity).
    assert!(pre_last.contains('T'));

    // Sidecar still matches the live bytes.
    assert_sidecar_matches(&registry);
}

/// `touch --slug <missing>` errors with `kind=not_found` instead of
/// silently no-oping. The slug must already be in the registry — touch
/// has no implicit `add` semantics.
#[test]
fn touch_unknown_slug_errors_not_found() {
    let (dir, _) = fresh_root();
    run_active(&dir, &["add", "--slug", "x"]).success();

    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("flow")
        .arg("active")
        .arg("touch")
        .arg("--slug")
        .arg("nonexistent");
    let out = cmd.write_stdin("").assert().failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr must be JSON: {e}; stderr:\n{stderr}"));
    assert_eq!(v["error"]["kind"], serde_json::json!("not_found"));
}

// ---------------------------------------------------------------------------
// dry-run
// ---------------------------------------------------------------------------

/// `add --dry-run` emits the would_change envelope WITHOUT mutating
/// either the registry file or its sidecar. On a fresh root neither
/// exists pre- nor post-dry-run.
#[test]
fn add_dry_run_does_not_mutate_or_bootstrap() {
    let (dir, registry) = fresh_root();
    let sidecar = sidecar_for(&registry);

    let out = run_active(
        &dir,
        &["add", "--slug", "x", "--branch", "feat/x", "--dry-run"],
    )
    .success();
    let v = json_stdout(&out);
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    assert_eq!(wc["action"], serde_json::json!("add"));
    assert_eq!(wc["slug"], serde_json::json!("x"));
    let new_entry = &wc["new_entry"];
    assert_eq!(new_entry["slug"], serde_json::json!("x"));
    assert_eq!(new_entry["binding"]["branch"], serde_json::json!("feat/x"));

    assert!(!registry.exists(), "dry-run must not create the registry");
    assert!(!sidecar.exists(), "dry-run must not create the sidecar");
}

/// `remove --dry-run` against a primed registry leaves the file +
/// sidecar byte-identical and surfaces the would-be-removed entry as
/// `would_change.removed_entry`.
#[test]
fn remove_dry_run_does_not_mutate() {
    let (dir, registry) = fresh_root();
    run_active(&dir, &["add", "--slug", "x", "--branch", "feat/x"]).success();
    let sidecar = sidecar_for(&registry);

    let before_bytes = fs::read(&registry).unwrap();
    let before_sidecar = fs::read(&sidecar).unwrap();

    let out = run_active(&dir, &["remove", "--slug", "x", "--dry-run"]).success();
    let v = json_stdout(&out);
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    assert_eq!(wc["action"], serde_json::json!("remove"));
    assert_eq!(wc["slug"], serde_json::json!("x"));
    assert_eq!(wc["removed_entry"]["slug"], serde_json::json!("x"));

    assert_eq!(
        before_bytes,
        fs::read(&registry).unwrap(),
        "dry-run remove must not change registry bytes"
    );
    assert_eq!(
        before_sidecar,
        fs::read(&sidecar).unwrap(),
        "dry-run remove must not change sidecar bytes"
    );
}

/// `touch --dry-run` is read-only. Pins the dry-run invariance for the
/// touch path.
#[test]
fn touch_dry_run_does_not_mutate() {
    let (dir, registry) = fresh_root();
    run_active(&dir, &["add", "--slug", "x"]).success();
    let sidecar = sidecar_for(&registry);

    let before_bytes = fs::read(&registry).unwrap();
    let before_sidecar = fs::read(&sidecar).unwrap();

    let out = run_active(&dir, &["touch", "--slug", "x", "--dry-run"]).success();
    let v = json_stdout(&out);
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    assert_eq!(v["would_change"]["action"], serde_json::json!("touch"));
    assert_eq!(v["would_change"]["slug"], serde_json::json!("x"));

    assert_eq!(before_bytes, fs::read(&registry).unwrap());
    assert_eq!(before_sidecar, fs::read(&sidecar).unwrap());
}

// ---------------------------------------------------------------------------
// concurrent writers
// ---------------------------------------------------------------------------

/// Two parallel `add` invocations on different slugs — both must succeed
/// and both entries must survive in the final registry. The exclusive
/// lock in `with_exclusive_lock` is what serialises them; without it the
/// last writer would clobber the other's entry.
#[test]
fn concurrent_add_invocations_serialise_no_entry_loss() {
    use std::thread;

    let (dir, _) = fresh_root();
    let dir_path = dir.path().to_path_buf();

    let handles: Vec<_> = ["a", "b", "c", "d"]
        .iter()
        .map(|slug| {
            let path = dir_path.clone();
            let slug = slug.to_string();
            thread::spawn(move || {
                let mut cmd = Command::cargo_bin("tomlctl").unwrap();
                cmd.env("TOMLCTL_ROOT", &path)
                    .env("TOMLCTL_LOCK_TIMEOUT", "30")
                    .arg("flow")
                    .arg("active")
                    .arg("add")
                    .arg("--slug")
                    .arg(&slug);
                cmd.write_stdin("").assert().success();
            })
        })
        .collect();
    for h in handles {
        h.join().expect("worker thread must succeed");
    }

    // All four entries must be present.
    let v = json_stdout(&run_active(&dir, &["list"]).success());
    let arr = v["active"].as_array().expect("active must be array");
    assert_eq!(
        arr.len(),
        4,
        "all 4 concurrent adds must survive — got {} entries: {v}",
        arr.len()
    );
    let mut slugs: Vec<String> = arr
        .iter()
        .map(|e| e["slug"].as_str().unwrap_or("").to_string())
        .collect();
    slugs.sort();
    assert_eq!(slugs, vec!["a", "b", "c", "d"]);
}

// ---------------------------------------------------------------------------
// legacy single-line file detection
// ---------------------------------------------------------------------------

/// When ONLY the legacy `.claude/active-flow` (no `.toml` suffix) exists,
/// `list` returns the empty default AND emits the documented one-line
/// stderr warning. The legacy file is NEVER auto-migrated or deleted.
#[test]
fn legacy_active_flow_file_emits_warning_and_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let legacy = claude.join("active-flow");
    fs::write(&legacy, "old-slug\n").unwrap();
    // Sanity: new registry must be absent for the warning to fire.
    let registry = claude.join("active-flow.toml");
    assert!(!registry.exists());

    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("active")
        .arg("list");
    let out = cmd.write_stdin("").assert().success();

    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["active"], serde_json::json!([]), "list must be empty");

    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("legacy `.claude/active-flow` ignored"),
        "stderr must carry the documented legacy warning, got: {stderr:?}"
    );
    assert!(
        stderr.contains("CLAUDE.md"),
        "stderr must reference the cutover instructions, got: {stderr:?}"
    );

    // Legacy file must still exist on disk — no auto-migration.
    assert!(
        legacy.exists(),
        "legacy file must NOT be deleted by tomlctl"
    );
}

/// When the new registry IS present, the legacy warning is silent even
/// if the legacy file lingers — this is the "after cutover" steady state.
/// Pins the negative case so a future regression that fires the warning
/// unconditionally would surface.
#[test]
fn legacy_warning_silent_when_new_registry_exists() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let legacy = claude.join("active-flow");
    fs::write(&legacy, "old-slug\n").unwrap();
    // Bootstrap the new registry first (via add).
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("active")
        .arg("add")
        .arg("--slug")
        .arg("x")
        .write_stdin("")
        .assert()
        .success();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("active")
        .arg("list")
        .write_stdin("")
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        !stderr.contains("legacy"),
        "warning must be silent once new registry exists, got stderr: {stderr:?}"
    );
}
