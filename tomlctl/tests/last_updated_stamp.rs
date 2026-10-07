//! CLI writes refresh an existing root `last_updated` to today (UTC) when they
//! change the document; `--no-stamp` opts out, a no-op write stays a no-op,
//! and a file without the key never gains one.

use std::fs;
use std::path::Path;

mod common;
use common::{cli, seed_ledger};

const OLD: &str = "2026-01-01";

fn seeded() -> (tempfile::TempDir, std::path::PathBuf) {
    seed_ledger(&format!(
        "schema_version = 1\nlast_updated = {OLD}\n\n[[items]]\nid = \"R1\"\nstatus = \"open\"\n"
    ))
}

fn run(root: &Path, args: &[&str]) {
    let out = cli(root).args(args).write_stdin("").output().unwrap();
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn last_updated(file: &Path) -> Option<String> {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    doc.get("last_updated").map(|v| match v {
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

#[test]
fn items_add_stamps_today() {
    let (dir, ledger) = seeded();
    let l = ledger.to_str().unwrap();
    run(dir.path(), &["items", "add", l, "--json", r#"{"id":"R2"}"#]);
    assert_eq!(last_updated(&ledger).as_deref(), Some(today_iso().as_str()));
}

#[test]
fn items_add_id_prefix_stamps_today() {
    let (dir, ledger) = seeded();
    let l = ledger.to_str().unwrap();
    run(
        dir.path(),
        &[
            "items",
            "add",
            l,
            "--json",
            r#"{"s":"a"}"#,
            "--id-prefix",
            "R",
        ],
    );
    assert_eq!(last_updated(&ledger).as_deref(), Some(today_iso().as_str()));
}

#[test]
fn no_stamp_keeps_the_old_date() {
    let (dir, ledger) = seeded();
    let l = ledger.to_str().unwrap();
    run(
        dir.path(),
        &["items", "add", l, "--json", r#"{"id":"R2"}"#, "--no-stamp"],
    );
    assert_eq!(last_updated(&ledger).as_deref(), Some(OLD));
}

#[test]
fn plan_path_writes_stamp_today() {
    let (dir, ledger) = seeded();
    let l = ledger.to_str().unwrap();
    run(dir.path(), &["items", "remove", l, "R1"]);
    assert_eq!(last_updated(&ledger).as_deref(), Some(today_iso().as_str()));
}

#[test]
fn set_of_another_key_stamps_today() {
    let (dir, ledger) = seeded();
    let l = ledger.to_str().unwrap();
    run(dir.path(), &["set", l, "scope", "x"]);
    assert_eq!(last_updated(&ledger).as_deref(), Some(today_iso().as_str()));
}

#[test]
fn noop_update_leaves_bytes_and_sidecar_unchanged() {
    let (dir, ledger) = seeded();
    let l = ledger.to_str().unwrap();
    run(dir.path(), &["integrity", "refresh", l]);
    let sidecar = ledger.with_extension("toml.sha256");
    let bytes = fs::read(&ledger).unwrap();
    let digest = fs::read(&sidecar).unwrap();
    run(
        dir.path(),
        &["items", "update", l, "R1", "--json", r#"{"status":"open"}"#],
    );
    assert_eq!(fs::read(&ledger).unwrap(), bytes);
    assert_eq!(fs::read(&sidecar).unwrap(), digest);
}

#[test]
fn dedupe_skip_does_not_stamp() {
    let (dir, ledger) = seeded();
    let l = ledger.to_str().unwrap();
    run(
        dir.path(),
        &[
            "items",
            "add",
            l,
            "--json",
            r#"{"id":"R9","status":"open"}"#,
            "--dedupe-by",
            "status",
        ],
    );
    assert_eq!(last_updated(&ledger).as_deref(), Some(OLD));
}

#[test]
fn file_without_the_key_gains_none() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    let l = ledger.to_str().unwrap();
    run(dir.path(), &["items", "add", l, "--json", r#"{"id":"R1"}"#]);
    assert_eq!(last_updated(&ledger), None);
}

#[test]
fn explicit_set_of_last_updated_wins() {
    let (dir, ledger) = seeded();
    let l = ledger.to_str().unwrap();
    run(
        dir.path(),
        &["set", l, "last_updated", "2026-03-03", "--type", "date"],
    );
    assert_eq!(last_updated(&ledger).as_deref(), Some("2026-03-03"));
}

#[test]
fn first_add_to_a_missing_flowless_review_ledger_writes_last_updated() {
    let dir = tempfile::tempdir().unwrap();
    let reviews = dir.path().join(".claude").join("reviews");
    fs::create_dir_all(&reviews).unwrap();
    let ledger = reviews.join("x.toml");
    run(
        dir.path(),
        &[
            "items",
            "add",
            ledger.to_str().unwrap(),
            "--json",
            r#"{"id":"R1"}"#,
        ],
    );
    let doc: toml::Value = toml::from_str(&fs::read_to_string(&ledger).unwrap()).unwrap();
    assert_eq!(doc["schema_version"].as_integer(), Some(1));
    assert_eq!(last_updated(&ledger).as_deref(), Some(today_iso().as_str()));
}
