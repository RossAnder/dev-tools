//! The document writers (`set`, `set-json`, `array-append`) refresh an existing
//! root `updated` to today (UTC) when the target is named `context.toml` and the
//! write changed something else; `--no-stamp` opts out, a write that sets
//! `updated` itself keeps the caller's value, and other files are untouched.

use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::cli;

const OLD: &str = "2020-01-01";

fn seed(name: &str, body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let flow = dir.path().join(".claude").join("flows").join("x");
    fs::create_dir_all(&flow).unwrap();
    let file = flow.join(name);
    fs::write(&file, body).unwrap();
    (dir, file)
}

fn seed_context() -> (tempfile::TempDir, PathBuf) {
    seed(
        "context.toml",
        &format!("slug = \"x\"\nstatus = \"in-progress\"\nupdated = {OLD}\n"),
    )
}

fn run(root: &Path, args: &[&str]) {
    let out = cli(root).args(args).write_stdin("").output().unwrap();
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn read(file: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

fn updated(file: &Path) -> Option<String> {
    read(file).get("updated").map(|v| match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

fn today_iso() -> String {
    jiff::Timestamp::now()
        .in_tz("UTC")
        .unwrap()
        .strftime("%Y-%m-%d")
        .to_string()
}

/// Accepts the day read before or after the run, so a UTC midnight crossing
/// mid-test cannot fail it.
fn assert_stamped_today(file: &Path, before: &str) {
    let after = today_iso();
    let got = updated(file);
    assert!(
        got.as_deref() == Some(before) || got.as_deref() == Some(after.as_str()),
        "updated {got:?} is neither {before} nor {after}"
    );
}

#[test]
fn set_on_a_context_refreshes_updated_as_a_date() {
    let (dir, ctx) = seed_context();
    let before = today_iso();
    run(
        dir.path(),
        &["set", ctx.to_str().unwrap(), "status", "review"],
    );
    assert_stamped_today(&ctx, &before);
    assert!(matches!(
        read(&ctx).get("updated"),
        Some(toml::Value::Datetime(_))
    ));
    assert_eq!(read(&ctx)["status"].as_str(), Some("review"));
}

#[test]
fn set_json_on_a_context_refreshes_updated() {
    let (dir, ctx) = seed_context();
    let before = today_iso();
    run(
        dir.path(),
        &[
            "set-json",
            ctx.to_str().unwrap(),
            "scope",
            "--json",
            r#"["a/**"]"#,
        ],
    );
    assert_stamped_today(&ctx, &before);
}

#[test]
fn array_append_on_a_context_refreshes_updated() {
    let (dir, ctx) = seed_context();
    let before = today_iso();
    run(
        dir.path(),
        &[
            "array-append",
            ctx.to_str().unwrap(),
            "notes",
            "--set",
            "k=v",
        ],
    );
    assert_stamped_today(&ctx, &before);
}

#[test]
fn no_stamp_keeps_the_old_updated() {
    let (dir, ctx) = seed_context();
    run(
        dir.path(),
        &[
            "set",
            ctx.to_str().unwrap(),
            "status",
            "review",
            "--no-stamp",
        ],
    );
    assert_eq!(updated(&ctx).as_deref(), Some(OLD));
}

#[test]
fn a_write_that_sets_updated_keeps_the_callers_value() {
    let (dir, ctx) = seed_context();
    run(
        dir.path(),
        &[
            "set",
            ctx.to_str().unwrap(),
            "updated",
            "2021-02-02",
            "--type",
            "date",
            "--set",
            "status=review",
        ],
    );
    assert_eq!(updated(&ctx).as_deref(), Some("2021-02-02"));
    assert_eq!(read(&ctx)["status"].as_str(), Some("review"));
}

#[test]
fn noop_set_leaves_bytes_unchanged() {
    let (dir, ctx) = seed_context();
    let bytes = fs::read(&ctx).unwrap();
    run(
        dir.path(),
        &["set", ctx.to_str().unwrap(), "status", "in-progress"],
    );
    assert_eq!(fs::read(&ctx).unwrap(), bytes);
}

#[test]
fn another_file_keeps_its_updated() {
    let (dir, file) = seed(
        "other.toml",
        &format!("status = \"in-progress\"\nupdated = {OLD}\n"),
    );
    run(
        dir.path(),
        &["set", file.to_str().unwrap(), "status", "review"],
    );
    assert_eq!(updated(&file).as_deref(), Some(OLD));
}

#[test]
fn a_context_without_updated_gains_none() {
    let (dir, ctx) = seed("context.toml", "slug = \"x\"\nstatus = \"in-progress\"\n");
    run(
        dir.path(),
        &["set", ctx.to_str().unwrap(), "status", "review"],
    );
    assert_eq!(updated(&ctx), None);
}
