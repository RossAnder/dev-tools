//! T4: integration tests for `tomlctl flow find-plans`.
//!
//! Each test builds a self-contained tempdir laid out as a mock repo root
//! (containing `.claude/` and one or more plan directories), points
//! `TOMLCTL_ROOT` at that tempdir, and then invokes the built `tomlctl`
//! binary via `assert_cmd`. The TOMLCTL_ROOT env var is the canonical test
//! escape-hatch from `repo_or_cwd_root()` — without it, `find-plans` would
//! attempt to detect a repo via `git rev-parse --show-toplevel` and read
//! the CHECKED-IN `.claude/settings.json`, which would invalidate every
//! configuration assertion.

use assert_cmd::Command;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Path, PathBuf};

mod common;

/// Build a mock repo root in a fresh tempdir. Returns (tempdir, root path).
/// Caller is responsible for populating plans + settings + flows.
fn make_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join(".claude")).unwrap();
    (dir, root)
}

/// Run `tomlctl flow find-plans <args...>` against the given tempdir root and
/// parse the stdout as JSON. Asserts success. Returns the parsed array.
fn run_find_plans(root: &Path, args: &[&str]) -> Vec<JsonValue> {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("find-plans");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: JsonValue = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON array; err={e}; stdout:\n{stdout}"));
    v.as_array()
        .unwrap_or_else(|| panic!("stdout must be a JSON array; got: {stdout}"))
        .clone()
}

/// Same as `run_find_plans` but expects failure; returns stderr.
fn run_find_plans_failing(root: &Path, args: &[&str]) -> String {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("find-plans");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().failure();
    String::from_utf8_lossy(&out.get_output().stderr).to_string()
}

/// Seed a minimal plan markdown file under `root`. Path is relative to root.
fn seed_plan(root: &Path, rel: &str, body: &str) {
    let abs = root.join(rel);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(abs, body).unwrap();
}

/// Seed a flow `context.toml` under `<root>/.claude/flows/<slug>/`.
fn seed_context(root: &Path, slug: &str, body: &str) {
    let dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("context.toml"), body).unwrap();
}

// ---------------------------------------------------------------------------
// Acceptance tests for the documented capabilities (per task spec).
// ---------------------------------------------------------------------------

/// Discovery from explicit `--dirs` (Step 1 of the resolution order). The
/// CLI flag wins over any settings.json configuration; here we verify the
/// flag works on a tempdir with no settings file at all.
#[test]
fn discovery_from_explicit_dirs() {
    let (_g, root) = make_root();
    seed_plan(&root, "my-plans/feature-x.md", "# x\n");
    seed_plan(&root, "my-plans/feature-y.md", "# y\n");

    let arr = run_find_plans(&root, &["--dirs", "my-plans"]);
    let mut slugs: Vec<String> = arr
        .iter()
        .map(|r| r.get("slug").and_then(|s| s.as_str()).unwrap().to_string())
        .collect();
    slugs.sort();
    assert_eq!(slugs, vec!["feature-x", "feature-y"]);

    // No flow context.toml exists for either, so has_flow=false on both.
    for r in &arr {
        assert_eq!(
            r.get("has_flow").and_then(|v| v.as_bool()),
            Some(false),
            "record without context.toml must report has_flow=false: {r}"
        );
    }
}

/// `plansDirectory` (string form) read from `.claude/settings.json` (Step 3
/// of the resolution order, taken when neither `--dirs` nor the namespaced
/// array is set).
#[test]
fn reads_plans_directory_string_from_settings() {
    let (_g, root) = make_root();
    seed_plan(&root, "alt-plans/alpha.md", "# a\n");
    fs::write(
        root.join(".claude").join("settings.json"),
        r#"{"plansDirectory": "alt-plans"}"#,
    )
    .unwrap();

    let arr = run_find_plans(&root, &[]);
    let slugs: Vec<&str> = arr
        .iter()
        .map(|r| r.get("slug").and_then(|s| s.as_str()).unwrap())
        .collect();
    assert_eq!(slugs, vec!["alpha"]);
}

/// `tomlctl.plansDirectories` (array form) — multi-path opt-in. Verifies
/// that ALL listed directories are walked (Step 2 of the resolution order).
#[test]
fn reads_tomlctl_plans_directories_array_multi_path() {
    let (_g, root) = make_root();
    seed_plan(&root, "primary/p1.md", "# p1\n");
    seed_plan(&root, "secondary/s1.md", "# s1\n");
    seed_plan(&root, "secondary/s2.md", "# s2\n");
    fs::write(
        root.join(".claude").join("settings.json"),
        r#"{"tomlctl": {"plansDirectories": ["primary", "secondary"]}}"#,
    )
    .unwrap();

    let arr = run_find_plans(&root, &[]);
    let mut slugs: Vec<String> = arr
        .iter()
        .map(|r| r.get("slug").and_then(|s| s.as_str()).unwrap().to_string())
        .collect();
    slugs.sort();
    assert_eq!(slugs, vec!["p1", "s1", "s2"]);
}

/// Precedence: `tomlctl.plansDirectories` wins over `plansDirectory` when
/// both are present. We seed plans only under the array's path so the
/// outcome unambiguously tells us which key was honoured.
#[test]
fn tomlctl_array_wins_over_plans_directory() {
    let (_g, root) = make_root();
    // The `plansDirectory` (string-form) target exists and has plans, but
    // we expect it to be IGNORED because the namespaced array is present.
    seed_plan(&root, "string-form/should-not-appear.md", "# nope\n");
    seed_plan(&root, "array-form/winner.md", "# yes\n");
    fs::write(
        root.join(".claude").join("settings.json"),
        r#"{"plansDirectory": "string-form", "tomlctl": {"plansDirectories": ["array-form"]}}"#,
    )
    .unwrap();

    let arr = run_find_plans(&root, &[]);
    let slugs: Vec<&str> = arr
        .iter()
        .map(|r| r.get("slug").and_then(|s| s.as_str()).unwrap())
        .collect();
    assert_eq!(
        slugs,
        vec!["winner"],
        "tomlctl.plansDirectories must win over plansDirectory (string)"
    );
}

/// Default fallback (Step 4). With no `--dirs`, no settings.json present,
/// the resolver falls back to `docs/plans/`. Seeding a plan there proves
/// the default works; the missing-default case is covered separately.
#[test]
fn default_fallback_when_neither_configured() {
    let (_g, root) = make_root();
    seed_plan(&root, "docs/plans/default-plan.md", "# default\n");
    // No settings.json.

    let arr = run_find_plans(&root, &[]);
    let slugs: Vec<&str> = arr
        .iter()
        .map(|r| r.get("slug").and_then(|s| s.as_str()).unwrap())
        .collect();
    assert_eq!(slugs, vec!["default-plan"]);
}

/// P4 sentinel: `plansDirectory == "__DONT_ASK__"` is treated as
/// "explicitly unset" and resolution falls through to the default.
#[test]
fn plans_directory_sentinel_falls_through_to_default() {
    let (_g, root) = make_root();
    seed_plan(&root, "docs/plans/d.md", "# d\n");
    fs::write(
        root.join(".claude").join("settings.json"),
        r#"{"plansDirectory": "__DONT_ASK__"}"#,
    )
    .unwrap();

    let arr = run_find_plans(&root, &[]);
    let slugs: Vec<&str> = arr
        .iter()
        .map(|r| r.get("slug").and_then(|s| s.as_str()).unwrap())
        .collect();
    assert_eq!(slugs, vec!["d"]);
}

/// P4 sentinel on the namespaced array. When every entry is the sentinel,
/// the whole key is treated as unset and we fall through to (potentially)
/// `plansDirectory` and then the default.
#[test]
fn tomlctl_plans_directories_all_sentinel_falls_through() {
    let (_g, root) = make_root();
    seed_plan(&root, "docs/plans/d.md", "# d\n");
    fs::write(
        root.join(".claude").join("settings.json"),
        r#"{"tomlctl": {"plansDirectories": ["__DONT_ASK__"]}}"#,
    )
    .unwrap();

    let arr = run_find_plans(&root, &[]);
    let slugs: Vec<&str> = arr
        .iter()
        .map(|r| r.get("slug").and_then(|s| s.as_str()).unwrap())
        .collect();
    assert_eq!(slugs, vec!["d"]);
}

/// Multi-file plan: a subdirectory `feature/` containing `00-outline.md`
/// produces a single record whose `slug` is the directory name and whose
/// `path` points at the outline file.
#[test]
fn multi_file_plan_uses_outline_md() {
    let (_g, root) = make_root();
    seed_plan(&root, "docs/plans/big-feature/00-outline.md", "# outline\n");
    seed_plan(&root, "docs/plans/big-feature/01-research.md", "# research\n");

    let arr = run_find_plans(&root, &[]);
    assert_eq!(arr.len(), 1, "multi-file plan must produce exactly one record");
    let r = &arr[0];
    assert_eq!(r.get("slug").and_then(|s| s.as_str()), Some("big-feature"));
    let path = r.get("path").and_then(|s| s.as_str()).unwrap();
    assert!(
        path.ends_with("00-outline.md"),
        "outline file should be 00-outline.md, got: {path}"
    );
}

/// Multi-file plan outline-priority order: when `00-outline.md` is absent
/// but `index.md` is present, the outline picker chooses `index.md`.
#[test]
fn multi_file_plan_falls_back_to_index_md() {
    let (_g, root) = make_root();
    seed_plan(&root, "docs/plans/feat/index.md", "# idx\n");
    seed_plan(&root, "docs/plans/feat/notes.md", "# notes\n");

    let arr = run_find_plans(&root, &[]);
    assert_eq!(arr.len(), 1);
    let path = arr[0].get("path").and_then(|s| s.as_str()).unwrap();
    assert!(
        path.ends_with("index.md"),
        "outline should be index.md, got: {path}"
    );
}

/// Cross-reference: when `.claude/flows/<slug>/context.toml` exists, the
/// record carries `has_flow=true` plus `status`/`updated`/`branch`.
#[test]
fn cross_reference_with_context_populates_flow_metadata() {
    let (_g, root) = make_root();
    seed_plan(&root, "docs/plans/wired.md", "# wired\n");
    seed_context(
        &root,
        "wired",
        r#"slug = "wired"
plan_path = "docs/plans/wired.md"
status = "in-progress"
created = 2026-04-01
updated = 2026-05-08
branch = "main"
"#,
    );

    let arr = run_find_plans(&root, &[]);
    assert_eq!(arr.len(), 1);
    let r = &arr[0];
    assert_eq!(r.get("slug").and_then(|s| s.as_str()), Some("wired"));
    assert_eq!(r.get("has_flow").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        r.get("status").and_then(|s| s.as_str()),
        Some("in-progress")
    );
    assert_eq!(
        r.get("updated").and_then(|s| s.as_str()),
        Some("2026-05-08"),
        "TOML date should render as ISO-8601 string"
    );
    assert_eq!(r.get("branch").and_then(|s| s.as_str()), Some("main"));
}

/// Missing context.toml (the common case for fresh / orphaned plans): the
/// record reports `has_flow=false` and the optional flow fields are absent.
#[test]
fn missing_context_yields_has_flow_false() {
    let (_g, root) = make_root();
    seed_plan(&root, "docs/plans/orphan.md", "# orphan\n");

    let arr = run_find_plans(&root, &[]);
    assert_eq!(arr.len(), 1);
    let r = &arr[0];
    assert_eq!(r.get("has_flow").and_then(|v| v.as_bool()), Some(false));
    assert!(r.get("status").is_none(), "no status when has_flow=false");
    assert!(r.get("updated").is_none(), "no updated when has_flow=false");
    assert!(r.get("branch").is_none(), "no branch when has_flow=false");
}

/// `--strict-read` errors when a *configured* plan directory is missing.
/// "Configured" means it came from `--dirs`, the namespaced array, or
/// `plansDirectory`. The default `docs/plans/` is NOT considered configured.
#[test]
fn strict_read_errors_on_missing_configured_dir() {
    let (_g, root) = make_root();
    // No "alt" directory exists.
    let stderr = run_find_plans_failing(&root, &["--dirs", "alt", "--strict-read"]);
    assert!(
        stderr.contains("alt"),
        "stderr should mention the missing dir name; got: {stderr}"
    );
}

/// Default-dir-missing must NOT error under `--strict-read` — a fresh clone
/// with no `docs/plans/` directory yet is normal, not an error.
#[test]
fn strict_read_does_not_error_on_missing_default_dir() {
    let (_g, root) = make_root();
    // `docs/plans/` does NOT exist. No settings.json. With --strict-read
    // we still expect success and an empty array.
    let arr = run_find_plans(&root, &["--strict-read"]);
    assert_eq!(arr.len(), 0, "missing default dir → empty array, no error");
}

/// JSON output shape: array of objects with stable field set. The `--json`
/// flag is accepted (for CLI uniformity) but the output is JSON either way.
#[test]
fn json_output_shape_stable() {
    let (_g, root) = make_root();
    seed_plan(&root, "docs/plans/p1.md", "# p1\n");
    seed_plan(&root, "docs/plans/p2.md", "# p2\n");
    seed_context(
        &root,
        "p1",
        r#"slug = "p1"
plan_path = "docs/plans/p1.md"
status = "in-progress"
created = 2026-05-01
updated = 2026-05-08
branch = "main"
"#,
    );

    // R7: `--json` flag dropped — `flow find-plans` always emits JSON.
    let arr = run_find_plans(&root, &[]);
    assert_eq!(arr.len(), 2);

    // Every record must carry `path`, `slug`, `has_flow`. Wired records
    // additionally carry `status`/`updated`/`branch` (per the spec contract).
    for r in &arr {
        assert!(r.get("path").and_then(|v| v.as_str()).is_some(), "path: {r}");
        assert!(r.get("slug").and_then(|v| v.as_str()).is_some(), "slug: {r}");
        assert!(
            r.get("has_flow").and_then(|v| v.as_bool()).is_some(),
            "has_flow: {r}"
        );
    }

    // Find p1 and assert its full shape; p2 (no context.toml) carries the
    // minimal three-field shape.
    let p1 = arr
        .iter()
        .find(|r| r.get("slug").and_then(|s| s.as_str()) == Some("p1"))
        .expect("p1 must be present");
    assert_eq!(p1.get("has_flow").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(p1.get("status").and_then(|v| v.as_str()), Some("in-progress"));
    assert_eq!(p1.get("updated").and_then(|v| v.as_str()), Some("2026-05-08"));
    assert_eq!(p1.get("branch").and_then(|v| v.as_str()), Some("main"));

    let p2 = arr
        .iter()
        .find(|r| r.get("slug").and_then(|s| s.as_str()) == Some("p2"))
        .expect("p2 must be present");
    assert_eq!(p2.get("has_flow").and_then(|v| v.as_bool()), Some(false));
    assert!(p2.get("status").is_none());
    assert!(p2.get("updated").is_none());
    assert!(p2.get("branch").is_none());
}
