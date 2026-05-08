//! T7 integration tests for `tomlctl flow init` against a tempdir-rooted
//! `.claude/flows/<slug>/` layout.
//!
//! Sandbox strategy mirrors `tests/flow_active.rs`: every test creates a
//! `tempfile::tempdir()` and points `TOMLCTL_ROOT` at it via
//! `assert_cmd::Command::env`. The plan path passed to `--plan` is a real
//! file on the tempdir so callers that resolve it later don't trip on a
//! missing source — but `flow init` itself does NOT read the plan file
//! (it only stamps the path string into `context.toml`).

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::assert_sidecar_matches;

/// Bootstrap an empty `.claude/` under a fresh tempdir and create a
/// throwaway plan file at `docs/plans/<slug>.md` so tests have a real
/// path to feed to `--plan` (the field is opaque to `flow init`, but
/// integration tests prefer a real on-disk anchor for clarity).
fn fresh_root(slug: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let plans = dir.path().join("docs").join("plans");
    fs::create_dir_all(&plans).unwrap();
    let plan_file = plans.join(format!("{slug}.md"));
    fs::write(&plan_file, "# Plan\n").unwrap();
    let context = claude.join("flows").join(slug).join("context.toml");
    (dir, plan_file, context)
}

/// Run `tomlctl flow init <args>` against `dir` and return the assertion
/// handle. Standard env wiring (`TOMLCTL_ROOT`, short lock timeout).
fn run_init(dir: &tempfile::TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("init");
    for a in args {
        cmd.arg(a);
    }
    cmd.write_stdin("").assert()
}

/// Run `tomlctl flow init <args>` with `--error-format json` so test
/// callers asserting on a failure can parse the structured envelope.
fn run_init_with_error_json(
    dir: &tempfile::TempDir,
    args: &[&str],
) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("flow")
        .arg("init");
    for a in args {
        cmd.arg(a);
    }
    cmd.write_stdin("").assert()
}

/// Parse stdout as a single JSON line.
fn json_stdout(out: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"))
}

/// Local sidecar-path helper.
fn sidecar_for(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

// ---------------------------------------------------------------------------
// fresh init
// ---------------------------------------------------------------------------

/// A fresh `flow init` materialises:
/// - `.claude/flows/<slug>/context.toml` + `.sha256`
/// - `.claude/flows/<slug>/execution-record.toml` + `.sha256`
/// - one `[[active]]` entry in `.claude/active-flow.toml` + `.sha256`
#[test]
fn fresh_init_creates_context_record_sidecars_and_active_entry() {
    let (dir, plan, context) = fresh_root("feature-x");
    let plan_str = plan.to_string_lossy().to_string();

    let out = run_init(
        &dir,
        &["--slug", "feature-x", "--plan", &plan_str],
    )
    .success();

    let v = json_stdout(&out);
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["slug"], serde_json::json!("feature-x"));
    assert_eq!(v["action"], serde_json::json!("init"));
    assert_eq!(
        v["artifacts"]["execution_record"],
        serde_json::json!(".claude/flows/feature-x/execution-record.toml")
    );
    assert_eq!(
        v["artifacts"]["review_ledger"],
        serde_json::json!(".claude/flows/feature-x/review-ledger.toml")
    );
    assert_eq!(
        v["artifacts"]["optimise_findings"],
        serde_json::json!(".claude/flows/feature-x/optimise-findings.toml")
    );
    assert_eq!(
        v["artifacts"]["plan_review_findings"],
        serde_json::json!(".claude/flows/feature-x/plan-review-findings.toml")
    );

    // Files on disk.
    assert!(context.exists(), "context.toml must exist");
    assert_sidecar_matches(&context);

    let exec_record = dir
        .path()
        .join(".claude")
        .join("flows")
        .join("feature-x")
        .join("execution-record.toml");
    assert!(exec_record.exists(), "execution-record.toml must exist");
    assert_sidecar_matches(&exec_record);

    let active = dir.path().join(".claude").join("active-flow.toml");
    assert!(active.exists(), "active-flow.toml must exist");
    assert_sidecar_matches(&active);

    // active-flow.toml has the new entry.
    let raw = fs::read_to_string(&active).unwrap();
    let parsed: toml::Value = toml::from_str(&raw).unwrap();
    let arr = parsed
        .get("active")
        .and_then(|v| v.as_array())
        .expect("active array must exist");
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0].get("slug").and_then(|v| v.as_str()),
        Some("feature-x")
    );

    // execution-record.toml content is the canonical 2-line bootstrap.
    let er_text = fs::read_to_string(&exec_record).unwrap();
    assert!(
        er_text.starts_with("schema_version = 1\n"),
        "execution-record.toml must start with schema_version = 1; got:\n{er_text}"
    );
    assert!(
        er_text.contains("last_updated ="),
        "execution-record.toml must contain a last_updated field; got:\n{er_text}"
    );

    // context.toml carries the seed shape: status="draft", tasks counts
    // zero, artifacts populated.
    let ctx_text = fs::read_to_string(&context).unwrap();
    let ctx: toml::Value = toml::from_str(&ctx_text).unwrap();
    assert_eq!(
        ctx.get("status").and_then(|v| v.as_str()),
        Some("draft")
    );
    assert_eq!(
        ctx.get("tasks")
            .and_then(|t| t.get("total"))
            .and_then(|v| v.as_integer()),
        Some(0)
    );
    assert!(
        ctx.get("created").is_some(),
        "created must be present"
    );
    assert!(
        ctx.get("updated").is_some(),
        "updated must be present"
    );
}

// ---------------------------------------------------------------------------
// idempotent re-init
// ---------------------------------------------------------------------------

/// Re-running `flow init` on an existing slug is a no-op for `context.toml`:
/// `created` is unchanged, the response carries `action="noop"`, and the
/// file's bytes (specifically the `created` field) are preserved.
#[test]
fn reinit_existing_slug_is_idempotent_and_preserves_created() {
    let (dir, plan, context) = fresh_root("feature-x");
    let plan_str = plan.to_string_lossy().to_string();

    run_init(&dir, &["--slug", "feature-x", "--plan", &plan_str]).success();
    let first_text = fs::read_to_string(&context).unwrap();
    let first_ctx: toml::Value = toml::from_str(&first_text).unwrap();
    let first_created = first_ctx.get("created").cloned();

    // Sleep a tick so any timestamp-derived field would diverge if the
    // re-init were destructive.
    std::thread::sleep(std::time::Duration::from_millis(2));

    let out = run_init(&dir, &["--slug", "feature-x", "--plan", &plan_str]).success();
    let v = json_stdout(&out);
    assert_eq!(v["action"], serde_json::json!("noop"));
    assert_eq!(v["slug"], serde_json::json!("feature-x"));

    // `created` byte-identical post re-init.
    let second_text = fs::read_to_string(&context).unwrap();
    let second_ctx: toml::Value = toml::from_str(&second_text).unwrap();
    let second_created = second_ctx.get("created").cloned();
    assert_eq!(
        first_created, second_created,
        "created must be preserved verbatim across re-inits"
    );
    // Whole-file byte identity is the strongest pin.
    assert_eq!(
        first_text, second_text,
        "context.toml bytes must be unchanged across re-inits"
    );
}

// ---------------------------------------------------------------------------
// dry-run
// ---------------------------------------------------------------------------

/// `--dry-run` writes nothing on a fresh root: no context, no
/// execution-record, no active-flow registry, and no sidecars. The
/// envelope on stdout carries the `would_change` shape.
#[test]
fn dry_run_does_not_mutate_anything() {
    let (dir, plan, context) = fresh_root("feature-x");
    let plan_str = plan.to_string_lossy().to_string();
    let exec_record = dir
        .path()
        .join(".claude")
        .join("flows")
        .join("feature-x")
        .join("execution-record.toml");
    let active = dir.path().join(".claude").join("active-flow.toml");

    let out = run_init(
        &dir,
        &[
            "--slug",
            "feature-x",
            "--plan",
            &plan_str,
            "--branch",
            "feat/x",
            "--dry-run",
        ],
    )
    .success();

    let v = json_stdout(&out);
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    let wc = &v["would_change"];
    assert_eq!(wc["action"], serde_json::json!("init"));
    assert_eq!(wc["slug"], serde_json::json!("feature-x"));
    assert_eq!(
        wc["execution_record_bootstrap"],
        serde_json::json!(true),
        "fresh root → execution-record bootstrap is true in dry-run"
    );
    let active_reg = &wc["active_registration"];
    assert_eq!(active_reg["slug"], serde_json::json!("feature-x"));
    assert_eq!(active_reg["binding"]["branch"], serde_json::json!("feat/x"));

    // No filesystem mutation.
    assert!(!context.exists(), "dry-run must not create context.toml");
    assert!(!sidecar_for(&context).exists(), "no context sidecar");
    assert!(
        !exec_record.exists(),
        "dry-run must not bootstrap execution-record.toml"
    );
    assert!(!sidecar_for(&exec_record).exists(), "no exec sidecar");
    assert!(
        !active.exists(),
        "dry-run must not register in active-flow.toml"
    );
    assert!(!sidecar_for(&active).exists(), "no active sidecar");
}

// ---------------------------------------------------------------------------
// slug sanitiser
// ---------------------------------------------------------------------------

/// The slug regex `^[a-z0-9][a-z0-9-]{0,63}$` rejects:
/// - uppercase chars (`UPPER`)
/// - whitespace (`with space`)
/// - underscore prefix (`_under`)
/// - empty string
/// - 65+ chars (`a` × 65)
///
/// Each rejection MUST surface as a `kind=validation` error in the JSON
/// envelope so downstream agents can branch on the kind.
#[test]
fn slug_sanitiser_rejects_uppercase() {
    let (dir, plan, _ctx) = fresh_root("placeholder");
    let plan_str = plan.to_string_lossy().to_string();
    let out = run_init_with_error_json(
        &dir,
        &["--slug", "UPPER", "--plan", &plan_str],
    )
    .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr must be JSON: {e}; stderr:\n{stderr}"));
    assert_eq!(v["error"]["kind"], serde_json::json!("validation"));
}

#[test]
fn slug_sanitiser_rejects_whitespace() {
    let (dir, plan, _ctx) = fresh_root("placeholder");
    let plan_str = plan.to_string_lossy().to_string();
    let out = run_init_with_error_json(
        &dir,
        &["--slug", "with space", "--plan", &plan_str],
    )
    .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(v["error"]["kind"], serde_json::json!("validation"));
}

#[test]
fn slug_sanitiser_rejects_underscore_prefix() {
    let (dir, plan, _ctx) = fresh_root("placeholder");
    let plan_str = plan.to_string_lossy().to_string();
    let out = run_init_with_error_json(
        &dir,
        &["--slug", "_under", "--plan", &plan_str],
    )
    .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(v["error"]["kind"], serde_json::json!("validation"));
}

#[test]
fn slug_sanitiser_rejects_empty_slug() {
    let (dir, plan, _ctx) = fresh_root("placeholder");
    let plan_str = plan.to_string_lossy().to_string();
    let out = run_init_with_error_json(
        &dir,
        &["--slug", "", "--plan", &plan_str],
    )
    .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(v["error"]["kind"], serde_json::json!("validation"));
}

#[test]
fn slug_sanitiser_rejects_overlong_slug() {
    let (dir, plan, _ctx) = fresh_root("placeholder");
    let plan_str = plan.to_string_lossy().to_string();
    // 65 chars: 1 leading char + 64 trailing chars (the regex caps total
    // length at 64 via `[a-z0-9][a-z0-9-]{0,63}`).
    let too_long: String = "a".repeat(65);
    let out = run_init_with_error_json(
        &dir,
        &["--slug", &too_long, "--plan", &plan_str],
    )
    .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(v["error"]["kind"], serde_json::json!("validation"));
}

// ---------------------------------------------------------------------------
// optional-arg propagation
// ---------------------------------------------------------------------------

/// `--branch` and `--scope` propagate into `context.toml` AND into the
/// active-flow registration's `[active.binding]` table. Multiple
/// `--scope` flags accumulate into a single TOML array.
#[test]
fn branch_and_scope_propagate_into_context_and_active() {
    let (dir, plan, context) = fresh_root("feature-x");
    let plan_str = plan.to_string_lossy().to_string();
    run_init(
        &dir,
        &[
            "--slug",
            "feature-x",
            "--plan",
            &plan_str,
            "--branch",
            "feat/x",
            "--scope",
            "src/foo/**",
            "--scope",
            "src/bar/**",
        ],
    )
    .success();

    // context.toml carries the branch and scope.
    let ctx: toml::Value = toml::from_str(&fs::read_to_string(&context).unwrap()).unwrap();
    assert_eq!(
        ctx.get("branch").and_then(|v| v.as_str()),
        Some("feat/x"),
        "context.toml.branch must echo --branch"
    );
    let scope_arr = ctx
        .get("scope")
        .and_then(|v| v.as_array())
        .expect("scope must be a TOML array");
    let scope_strs: Vec<&str> = scope_arr.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(scope_strs, vec!["src/foo/**", "src/bar/**"]);

    // active-flow.toml [active.binding] carries the same.
    let active_path = dir.path().join(".claude").join("active-flow.toml");
    let active: toml::Value = toml::from_str(&fs::read_to_string(&active_path).unwrap()).unwrap();
    let entry = active
        .get("active")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .expect("active entry must exist");
    let binding = entry
        .get("binding")
        .and_then(|v| v.as_table())
        .expect("binding sub-table must exist");
    assert_eq!(
        binding.get("branch").and_then(|v| v.as_str()),
        Some("feat/x")
    );
    let active_scope: Vec<&str> = binding
        .get("scope")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    assert_eq!(active_scope, vec!["src/foo/**", "src/bar/**"]);
}

/// Repeated `--scope` flags are accumulative — pin the multi-value clap
/// surface separately from the branch+scope round-trip test so a future
/// regression that drops to single-value parsing surfaces clearly.
#[test]
fn repeated_scope_args_accumulate() {
    let (dir, plan, context) = fresh_root("multi-scope");
    let plan_str = plan.to_string_lossy().to_string();
    run_init(
        &dir,
        &[
            "--slug",
            "multi-scope",
            "--plan",
            &plan_str,
            "--scope",
            "a/**",
            "--scope",
            "b/**",
            "--scope",
            "tests/",
        ],
    )
    .success();

    let ctx: toml::Value = toml::from_str(&fs::read_to_string(&context).unwrap()).unwrap();
    let scope: Vec<&str> = ctx
        .get("scope")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    assert_eq!(scope, vec!["a/**", "b/**", "tests/"]);
}

/// Omitting `--branch` MUST result in the `branch` key being absent from
/// `context.toml` (not written as an empty string). Plan-locked: "branch
/// — optional. ... otherwise the field is omitted entirely (not written
/// as empty string)".
#[test]
fn omitted_branch_is_absent_not_empty() {
    let (dir, plan, context) = fresh_root("no-branch");
    let plan_str = plan.to_string_lossy().to_string();
    run_init(
        &dir,
        &["--slug", "no-branch", "--plan", &plan_str],
    )
    .success();

    let ctx_text = fs::read_to_string(&context).unwrap();
    let ctx: toml::Value = toml::from_str(&ctx_text).unwrap();
    assert!(
        ctx.get("branch").is_none(),
        "branch key MUST be absent when --branch was not passed; ctx text:\n{ctx_text}"
    );
    // Belt-and-braces: also assert the raw bytes don't carry a literal
    // `branch =` line, so a regression that wrote `branch = ""` would
    // fail this test even if `toml::from_str` somehow round-tripped it
    // back to absent.
    assert!(
        !ctx_text.contains("branch ="),
        "raw context.toml must not contain `branch =` line; got:\n{ctx_text}"
    );
}
