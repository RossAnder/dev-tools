//! `set` applies the positional pair and every `--set PATH=VALUE` in one
//! write, and `array-append` builds its row from the field flags.

use std::fs;
use std::path::Path;

mod common;
use common::{assert_sidecar_matches, cli, seed_ledger};

const CONTEXT: &str = "status = \"draft\"\n\n[tasks]\ntotal = 0\ncompleted = 0\n";

fn run(root: &Path, args: &[&str]) -> String {
    let out = cli(root).args(args).write_stdin("").output().unwrap();
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn run_err(root: &Path, args: &[&str]) -> String {
    let out = cli(root).args(args).write_stdin("").output().unwrap();
    assert!(!out.status.success(), "{args:?} unexpectedly succeeded");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn doc(file: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

#[test]
fn set_applies_positional_and_flag_pairs_in_one_write() {
    let (dir, ctx) = seed_ledger(CONTEXT);
    let f = ctx.to_str().unwrap();
    run(
        dir.path(),
        &[
            "set",
            f,
            "status",
            "in-progress",
            "--set",
            "tasks.total=17",
            "--set",
            "tasks.in_progress=5",
        ],
    );
    let d = doc(&ctx);
    assert_eq!(d["status"].as_str(), Some("in-progress"));
    assert_eq!(d["tasks"]["total"].as_integer(), Some(17));
    assert_eq!(d["tasks"]["in_progress"].as_integer(), Some(5));
    assert_eq!(d["tasks"]["completed"].as_integer(), Some(0));
    assert_sidecar_matches(&ctx);
}

#[test]
fn set_accepts_flag_pairs_without_positionals() {
    let (dir, ctx) = seed_ledger(CONTEXT);
    let f = ctx.to_str().unwrap();
    run(
        dir.path(),
        &[
            "set",
            f,
            "--set",
            "status=review",
            "--set",
            "tasks.completed=4",
        ],
    );
    let d = doc(&ctx);
    assert_eq!(d["status"].as_str(), Some("review"));
    assert_eq!(d["tasks"]["completed"].as_integer(), Some(4));
}

#[test]
fn type_binds_the_positional_pair_only() {
    let (dir, ctx) = seed_ledger(CONTEXT);
    let f = ctx.to_str().unwrap();
    run(
        dir.path(),
        &[
            "set",
            f,
            "tasks.total",
            "17",
            "--type",
            "str",
            "--set",
            "tasks.completed=3",
        ],
    );
    let d = doc(&ctx);
    assert_eq!(d["tasks"]["total"].as_str(), Some("17"));
    assert_eq!(d["tasks"]["completed"].as_integer(), Some(3));
}

#[test]
fn set_refuses_a_path_given_twice_and_writes_nothing() {
    let (dir, ctx) = seed_ledger(CONTEXT);
    let f = ctx.to_str().unwrap();
    let stderr = run_err(dir.path(), &["set", f, "status", "a", "--set", "status=b"]);
    assert!(stderr.contains("more than once"), "stderr: {stderr}");
    assert_eq!(fs::read_to_string(&ctx).unwrap(), CONTEXT);
}

#[test]
fn set_refuses_a_malformed_pair_and_a_missing_source() {
    let (dir, ctx) = seed_ledger(CONTEXT);
    let f = ctx.to_str().unwrap();
    let stderr = run_err(dir.path(), &["set", f, "--set", "status"]);
    assert!(stderr.contains("PATH=VALUE"), "stderr: {stderr}");
    run_err(dir.path(), &["set", f]);
    run_err(dir.path(), &["set", f, "status"]);
    assert_eq!(fs::read_to_string(&ctx).unwrap(), CONTEXT);
}

#[test]
fn dry_run_reports_every_pair_and_keeps_the_single_pair_shape() {
    let (dir, ctx) = seed_ledger(CONTEXT);
    let f = ctx.to_str().unwrap();
    let many: serde_json::Value = serde_json::from_str(&run(
        dir.path(),
        &[
            "set",
            f,
            "status",
            "in-progress",
            "--set",
            "tasks.total=17",
            "--dry-run",
        ],
    ))
    .unwrap();
    assert_eq!(
        many,
        serde_json::json!({
            "ok": true,
            "dry_run": true,
            "pairs": [
                {"kind": "scalar", "path": "status", "old": "draft", "new": "in-progress"},
                {"kind": "scalar", "path": "tasks.total", "old": 0, "new": 17},
            ],
        })
    );
    let one: serde_json::Value =
        serde_json::from_str(&run(dir.path(), &["set", f, "status", "x", "--dry-run"])).unwrap();
    assert_eq!(
        one,
        serde_json::json!({
            "ok": true,
            "dry_run": true,
            "would_change": {"kind": "scalar", "path": "status", "old": "draft", "new": "x"},
        })
    );
    assert_eq!(fs::read_to_string(&ctx).unwrap(), CONTEXT);
}

#[test]
fn array_append_builds_the_row_from_field_flags() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let l = ledger.to_str().unwrap();
    run(
        dir.path(),
        &[
            "array-append",
            l,
            "vet_events",
            "--set",
            "lens=x",
            "--set-json",
            "sampled_count=3",
        ],
    );
    let d = doc(&ledger);
    let rows = d["vet_events"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["lens"].as_str(), Some("x"));
    assert_eq!(rows[0]["sampled_count"].as_integer(), Some(3));
    assert_sidecar_matches(&ledger);
}

#[test]
fn array_append_field_flags_merge_over_json() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let l = ledger.to_str().unwrap();
    run(
        dir.path(),
        &[
            "array-append",
            l,
            "vet_events",
            "--json",
            r#"{"lens":"base","keep":true}"#,
            "--set",
            "lens=over",
        ],
    );
    let d = doc(&ledger);
    let row = &d["vet_events"].as_array().unwrap()[0];
    assert_eq!(row["lens"].as_str(), Some("over"));
    assert_eq!(row["keep"].as_bool(), Some(true));
}

#[test]
fn array_append_refuses_field_flags_with_ndjson_and_no_source() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let l = ledger.to_str().unwrap();
    run_err(
        dir.path(),
        &[
            "array-append",
            l,
            "vet_events",
            "--ndjson",
            "-",
            "--set",
            "a=b",
        ],
    );
    let stderr = run_err(dir.path(), &["array-append", l, "vet_events"]);
    assert!(stderr.contains("--set"), "stderr: {stderr}");
    assert_eq!(fs::read_to_string(&ledger).unwrap(), "schema_version = 1\n");
}
