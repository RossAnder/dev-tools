//! `tasks import-plan`, `tasks render` and `tasks check --plan` on a flow whose
//! directory holds a `plan.toml`.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

use crate::common;
use common::{parse_json_error_envelope, stage_tasks_flow};

const SLUG: &str = "v2-flow";
const PLAN_REL: &str = "docs/plans/v2-flow.md";
const STORE: &str = "schema_version = 1\nlast_updated = 2026-09-07\n";

fn stage(with_plan_toml: bool) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical root");
    let store = stage_tasks_flow(&root, SLUG, PLAN_REL, Some(STORE));
    if with_plan_toml {
        fs::write(store.parent().expect("flow dir").join("plan.toml"), "").expect("plan.toml");
    }
    (dir, root)
}

fn run(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("tomlctl")
        .expect("the binary builds")
        .env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .args(["--error-format", "json"])
        .args(["tasks"])
        .args(args)
        .args(["--slug", SLUG])
        .write_stdin("")
        .assert()
}

fn assert_refused(root: &Path, args: &[&str]) {
    let assert = run(root, args).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|m| m.contains("plan view")),
        "{stderr}"
    );
}

#[test]
fn render_is_refused_on_a_flow_with_a_plan_toml() {
    let (_dir, root) = stage(true);
    assert_refused(&root, &["render"]);
}

#[test]
fn import_plan_is_refused_on_a_flow_with_a_plan_toml() {
    let (_dir, root) = stage(true);
    assert_refused(&root, &["import-plan"]);
}

#[test]
fn check_plan_skips_render_drift_on_a_flow_with_a_plan_toml() {
    let (_dir, root) = stage(true);
    // The recorded plan document does not exist, so reading it would fail.
    run(&root, &["check", "--plan"]).success();
}

#[test]
fn render_is_not_refused_without_a_plan_toml() {
    let (_dir, root) = stage(false);
    let assert = run(&root, &["render"]).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        !stderr.contains("plan view"),
        "a v1 flow must not get the v2 refusal: {stderr}"
    );
}
