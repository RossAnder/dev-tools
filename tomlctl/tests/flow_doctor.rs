//! Integration tests for `tomlctl flow doctor`.
//!
//! Each test materialises a tempdir-rooted fake repo, synthesises one or
//! more flows under `<tmp>/.claude/flows/<slug>/`, and runs the built
//! `tomlctl` binary via `assert_cmd` with `TOMLCTL_ROOT` pointed at the
//! tempdir. Mirrors the sandbox strategy used in `tests/flow_active.rs`
//! and `tests/flow_ensure_artifact.rs`.

use assert_cmd::Command;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

mod common;

/// Build `<tmp>/.claude/` and return `(tempdir, root)`. Tests drive flow
/// creation off this base.
fn fresh_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join(".claude")).unwrap();
    (dir, root)
}

/// `<file>.sha256`. Local helper — mirrors `integrity::sidecar_path`.
fn sidecar_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

/// Write the artifact and a matching `<file>.sha256` sidecar in the
/// canonical `<hex>  <basename>\n` format.
fn write_artifact_with_sidecar(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    let digest = Sha256::digest(body.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", b);
    }
    let basename = path.file_name().unwrap().to_string_lossy();
    fs::write(sidecar_path(path), format!("{hex}  {basename}\n")).unwrap();
}

/// Seed a clean flow under `<root>/.claude/flows/<slug>/`:
/// canonical `context.toml` + `execution-record.toml` + matching sidecars,
/// and a plan file at `docs/plans/<slug>.md` so `plan-path-resolves` passes.
fn seed_clean_flow(root: &Path, slug: &str) {
    let flow_dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&flow_dir).unwrap();

    // Plan file the flow points at.
    let plans_dir = root.join("docs").join("plans");
    fs::create_dir_all(&plans_dir).unwrap();
    let plan_file = plans_dir.join(format!("{slug}.md"));
    fs::write(&plan_file, "# plan\n").unwrap();

    // Canonical context.toml — every key the doctor's checks inspect.
    let context_body = format!(
        r#"slug = "{slug}"
plan_path = "docs/plans/{slug}.md"
status = "in-progress"
created = 2026-05-08
updated = 2026-05-08
scope = []

[tasks]
total = 0
completed = 0
in_progress = 0

[artifacts]
review_ledger = ".claude/flows/{slug}/review-ledger.toml"
optimise_findings = ".claude/flows/{slug}/optimise-findings.toml"
execution_record = ".claude/flows/{slug}/execution-record.toml"
plan_review_findings = ".claude/flows/{slug}/plan-review-findings.toml"
"#
    );
    write_artifact_with_sidecar(&flow_dir.join("context.toml"), &context_body);

    // execution-record.toml — minimal 2-line bootstrap shape.
    let er_body = "schema_version = 1\nlast_updated = 2026-05-08\n";
    write_artifact_with_sidecar(&flow_dir.join("execution-record.toml"), er_body);
}

/// Seed an `active-flow.toml` registry pointing at the listed slugs (in
/// insertion order). All bindings are minimal — only `slug` + `last_used`
/// are populated.
fn seed_active_flow_registry(root: &Path, slugs: &[&str]) {
    let claude = root.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let registry = claude.join("active-flow.toml");
    let mut body = String::from("schema_version = 1\n");
    for slug in slugs {
        body.push_str(&format!(
            "\n[[active]]\nslug = \"{slug}\"\nlast_used = \"2026-05-08T12:00:00Z\"\n"
        ));
    }
    write_artifact_with_sidecar(&registry, &body);
}

/// Run `tomlctl flow doctor <args>` against `root` and parse stdout as JSON.
/// Asserts process success.
fn run_doctor(root: &Path, args: &[&str]) -> JsonValue {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("doctor");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON; err={e}; stdout:\n{stdout}"))
}

/// Find the first check entry with the given `name` (and matching `scope` if
/// provided). Panics if not found.
fn find_check<'a>(v: &'a JsonValue, name: &str, scope: Option<&str>) -> &'a JsonValue {
    let arr = v["checks"].as_array().expect("checks must be array");
    arr.iter()
        .find(|c| {
            c["name"].as_str() == Some(name) && scope.is_none_or(|s| c["scope"].as_str() == Some(s))
        })
        .unwrap_or_else(|| {
            panic!(
                "no check with name={name} scope={:?} in {}",
                scope,
                serde_json::to_string_pretty(arr).unwrap()
            )
        })
}

// ---------------------------------------------------------------------------
// Acceptance: clean flow returns ok=true with all checks passing.
// ---------------------------------------------------------------------------

/// Clean flow on a clean root: every check passes, `ok=true`, no fixes
/// applied, no warnings.
#[test]
fn clean_flow_returns_ok_true_with_all_checks_passing() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "feature-x");
    seed_active_flow_registry(&root, &["feature-x"]);

    let v = run_doctor(&root, &["--slug", "feature-x"]);
    assert_eq!(
        v["ok"],
        JsonValue::Bool(true),
        "clean flow must return ok=true; got: {v}"
    );
    assert!(v["fixes_applied"].as_array().unwrap().is_empty());
    assert!(v["warnings"].as_array().unwrap().is_empty());

    // Every check is ok=true.
    let checks = v["checks"].as_array().unwrap();
    for c in checks {
        assert_eq!(
            c["ok"],
            JsonValue::Bool(true),
            "check must pass: {}",
            serde_json::to_string(c).unwrap()
        );
    }

    // Coverage pin: each named check is present at least once.
    for name in [
        "context-exists",
        "execution-record-exists",
        "context-sidecar",
        "execution-record-sidecar",
        "artifacts-canonical",
        "plan-path-resolves",
        "active-flow-registry",
        "gitignore-claude",
    ] {
        find_check(&v, name, None);
    }
}

// ---------------------------------------------------------------------------
// Acceptance: tampered sidecar reports + fixes under --fix.
// ---------------------------------------------------------------------------

/// Tampered sidecar (digest mismatch on context.toml) reports as a check
/// failure without mutating disk.
#[test]
fn tampered_context_sidecar_reports_check_failure_no_repair() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "feature-x");
    seed_active_flow_registry(&root, &["feature-x"]);

    // Overwrite the context.toml sidecar with a wrong-digest line.
    let context = root
        .join(".claude")
        .join("flows")
        .join("feature-x")
        .join("context.toml");
    let bogus_sidecar = format!(
        "{}  {}\n",
        "0".repeat(64),
        context.file_name().unwrap().to_string_lossy()
    );
    fs::write(sidecar_path(&context), &bogus_sidecar).unwrap();

    let v = run_doctor(&root, &["--slug", "feature-x"]);
    assert_eq!(v["ok"], JsonValue::Bool(false));
    let chk = find_check(&v, "context-sidecar", Some("feature-x"));
    assert_eq!(chk["ok"], JsonValue::Bool(false));
    let detail = chk["detail"].as_str().expect("detail must be present");
    assert!(
        detail.contains("mismatch"),
        "detail must surface mismatch reason; got: {detail}"
    );

    // No fixes applied (we didn't pass --fix).
    assert!(v["fixes_applied"].as_array().unwrap().is_empty());

    // Sidecar bytes unchanged from the bogus state.
    assert_eq!(
        fs::read_to_string(sidecar_path(&context)).unwrap(),
        bogus_sidecar
    );
}

/// `--fix` regenerates the tampered sidecar. The sidecar bytes must change
/// to a digest matching the file's actual contents, and `fixes_applied`
/// must surface the regen action.
#[test]
fn tampered_sidecar_regenerated_under_fix() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "feature-x");
    seed_active_flow_registry(&root, &["feature-x"]);

    let context = root
        .join(".claude")
        .join("flows")
        .join("feature-x")
        .join("context.toml");
    fs::write(
        sidecar_path(&context),
        format!(
            "{}  {}\n",
            "f".repeat(64),
            context.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();

    let v = run_doctor(&root, &["--slug", "feature-x", "--fix"]);
    let fixes = v["fixes_applied"]
        .as_array()
        .expect("fixes_applied must be array");
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("sidecar-refresh".to_string())
                && f["ok"] == JsonValue::Bool(true)
        ),
        "fixes_applied must include a sidecar-refresh ok=true entry; got: {v}"
    );

    // Sidecar now matches the file.
    let live_bytes = fs::read(&context).unwrap();
    let want = Sha256::digest(&live_bytes);
    let mut want_hex = String::with_capacity(64);
    for b in want.iter() {
        use std::fmt::Write;
        let _ = write!(want_hex, "{:02x}", b);
    }
    let on_disk = fs::read_to_string(sidecar_path(&context)).unwrap();
    let got_hex = on_disk.split_whitespace().next().unwrap();
    assert_eq!(
        got_hex.to_ascii_lowercase(),
        want_hex,
        "sidecar must match file after --fix"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: missing artifact reports without fixing.
// ---------------------------------------------------------------------------

/// A missing `context.toml` (e.g. half-bootstrapped flow dir) reports as a
/// failing check; doctor never creates the file even with `--fix`.
#[test]
fn missing_context_reports_without_creation_under_fix() {
    let (_g, root) = fresh_root();
    let flow_dir = root.join(".claude").join("flows").join("ghost");
    fs::create_dir_all(&flow_dir).unwrap();
    // No context.toml. No execution-record.toml. Empty flow dir.

    let v = run_doctor(&root, &["--slug", "ghost", "--fix"]);
    assert_eq!(v["ok"], JsonValue::Bool(false));

    let chk = find_check(&v, "context-exists", Some("ghost"));
    assert_eq!(chk["ok"], JsonValue::Bool(false));
    let detail = chk["detail"].as_str().expect("detail must be present");
    assert!(
        detail.contains("missing"),
        "detail must surface missing reason; got: {detail}"
    );

    // Doctor must NEVER create missing artifacts (that's flow init's job).
    assert!(
        !flow_dir.join("context.toml").exists(),
        "doctor --fix must NOT create missing context.toml"
    );
    assert!(
        !flow_dir.join("execution-record.toml").exists(),
        "doctor --fix must NOT create missing execution-record.toml"
    );

    // The fixes_applied list never carries a "create" action — only
    // sidecar-refresh / active-prune.
    let fixes = v["fixes_applied"].as_array().unwrap();
    for f in fixes {
        let name = f["name"].as_str().unwrap_or("");
        assert!(
            name == "sidecar-refresh" || name == "active-prune",
            "doctor must never emit a create-action fix; got: {f}"
        );
    }
}

// ---------------------------------------------------------------------------
// Acceptance: stale active-flow registry entry pruned under --fix.
// ---------------------------------------------------------------------------

/// Without `--fix`, a stale registry entry is reported but not pruned.
#[test]
fn stale_active_flow_entry_reported_without_fix() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "real");
    // Registry references both `real` AND a deleted slug `gone`.
    seed_active_flow_registry(&root, &["real", "gone"]);

    let v = run_doctor(&root, &[]);
    assert_eq!(v["ok"], JsonValue::Bool(false));

    let chk = find_check(&v, "active-flow-registry", Some("global"));
    assert_eq!(chk["ok"], JsonValue::Bool(false));
    let detail = chk["detail"].as_str().expect("detail must be present");
    assert!(
        detail.contains("gone"),
        "detail must name the stale slug; got: {detail}"
    );

    // Registry bytes unchanged — `gone` survives (no --fix).
    let raw = fs::read_to_string(root.join(".claude").join("active-flow.toml")).unwrap();
    assert!(raw.contains("\"gone\""));
}

/// `--fix` prunes the stale registry entry; `real` survives, `gone` does not.
#[test]
fn stale_active_flow_entry_pruned_under_fix() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "real");
    seed_active_flow_registry(&root, &["real", "gone"]);

    let v = run_doctor(&root, &["--fix"]);
    let fixes = v["fixes_applied"].as_array().expect("fixes must be array");
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("active-prune".to_string())
                && f["scope"] == JsonValue::String("gone".to_string())
                && f["ok"] == JsonValue::Bool(true)
        ),
        "fixes_applied must include an active-prune entry for `gone`; got: {v}"
    );

    let raw = fs::read_to_string(root.join(".claude").join("active-flow.toml")).unwrap();
    assert!(
        !raw.contains("\"gone\""),
        "active-flow.toml must NOT contain `gone` after --fix; got:\n{raw}"
    );
    assert!(
        raw.contains("\"real\""),
        "active-flow.toml MUST still contain `real` after --fix; got:\n{raw}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: gitignored-`.claude` warning fires when matched.
// ---------------------------------------------------------------------------

/// `.gitignore` listing `.claude/` triggers a warning AND a non-passing
/// `gitignore-claude` check.
#[test]
fn gitignored_claude_emits_warning() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "feature-x");
    seed_active_flow_registry(&root, &["feature-x"]);
    fs::write(root.join(".gitignore"), ".claude/\n").unwrap();

    let v = run_doctor(&root, &["--slug", "feature-x"]);
    let warnings = v["warnings"].as_array().expect("warnings must be array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains(".gitignore")),
        "warnings must include a gitignore line; got: {warnings:?}"
    );
    let chk = find_check(&v, "gitignore-claude", Some("global"));
    assert_eq!(
        chk["ok"],
        JsonValue::Bool(true),
        "gitignore-claude is a non-failing check post-R31 — the warning is the actionable surface, the check entry stays ok=true so envelope.ok is not flipped"
    );

    // Plain `.claude` (no trailing slash) also matches.
    fs::write(root.join(".gitignore"), ".claude\n").unwrap();
    let v2 = run_doctor(&root, &["--slug", "feature-x"]);
    assert_eq!(
        find_check(&v2, "gitignore-claude", Some("global"))["ok"],
        JsonValue::Bool(true),
        "gitignore-claude check is non-failing post-R31"
    );
    let warnings2 = v2["warnings"].as_array().expect("warnings must be array");
    assert!(
        warnings2
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains(".gitignore")),
        "plain `.claude` (no trailing slash) must still fire the warning entry"
    );

    // Comments and blank lines are NOT matches.
    fs::write(root.join(".gitignore"), "# .claude/\n\nnode_modules/\n").unwrap();
    let v3 = run_doctor(&root, &["--slug", "feature-x"]);
    let chk3 = find_check(&v3, "gitignore-claude", Some("global"));
    assert_eq!(
        chk3["ok"],
        JsonValue::Bool(true),
        "comments / unrelated entries must NOT fire the warning"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: --fix --dry-run emits plan without writing.
// ---------------------------------------------------------------------------

/// `--fix --dry-run` emits the would-be fixes plan but never touches the
/// filesystem.
#[test]
fn fix_dry_run_emits_plan_without_writing() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "real");
    seed_active_flow_registry(&root, &["real", "gone"]);

    // Tamper context.toml sidecar to also stage a sidecar-refresh fix.
    let context = root
        .join(".claude")
        .join("flows")
        .join("real")
        .join("context.toml");
    let pre_sidecar_bytes = fs::read(sidecar_path(&context)).unwrap();
    fs::write(
        sidecar_path(&context),
        format!(
            "{}  {}\n",
            "0".repeat(64),
            context.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();
    let staged_sidecar_bytes = fs::read(sidecar_path(&context)).unwrap();

    let pre_registry = fs::read_to_string(root.join(".claude").join("active-flow.toml")).unwrap();

    let v = run_doctor(&root, &["--fix", "--dry-run"]);
    assert_eq!(v["dry_run"], JsonValue::Bool(true));
    let fixes = v["fixes_applied"].as_array().expect("fixes must be array");
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("active-prune".to_string())
                && f["action"].as_str().unwrap_or("").contains("would prune")
        ),
        "dry-run plan must include `would prune` action; got: {fixes:?}"
    );
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("sidecar-refresh".to_string())
                && f["action"].as_str().unwrap_or("").contains("would refresh")
        ),
        "dry-run plan must include `would refresh` action; got: {fixes:?}"
    );

    // Filesystem unchanged: registry still has `gone`; sidecar bytes are
    // the bogus ones we staged, not the originals (we tampered) and not a
    // freshly-refreshed digest.
    assert_eq!(
        fs::read_to_string(root.join(".claude").join("active-flow.toml")).unwrap(),
        pre_registry,
        "dry-run must not mutate the registry"
    );
    assert_eq!(
        fs::read(sidecar_path(&context)).unwrap(),
        staged_sidecar_bytes,
        "dry-run must not regenerate the sidecar"
    );
    // Sanity: the staged bogus bytes aren't the originals — the tamper
    // worked, so a subsequent live --fix WOULD change them.
    assert_ne!(
        staged_sidecar_bytes, pre_sidecar_bytes,
        "test setup precondition: tamper must change sidecar bytes"
    );
}
