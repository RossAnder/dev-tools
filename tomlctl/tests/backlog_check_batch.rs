//! Black-box coverage for `tomlctl backlog check --ndjson`: many probes
//! graded against one read of the store, one result row per probe line.
//!
//! The store is seeded through `add`, because a `duplicate` verdict is a claim
//! about the fingerprint `add` lands on.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{backlog, backlog_stdout, cli, parse_json_error_envelope, sandbox};

const FLAKE_SUMMARY: &str = "pty_readiness_probe flakes on slow CI";
const FLAKE_AREA: &str = "lumina/server/tests/pty_readiness_probe.rs";
const FLAKE_CONTEXT: &str = "Only reproduces when the readiness gate races the first prompt write.";
const NOVEL_SUMMARY: &str = "sqlite migration checksum drifts after a renormalise";

fn seed_flake(root: &Path) -> Value {
    backlog(
        root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
            "--context",
            FLAKE_CONTEXT,
        ],
    )
}

/// Write `lines` to a staged NDJSON file beside the store, one per line.
fn stage(root: &Path, lines: &[String]) -> PathBuf {
    let path = root.join("probes.ndjson");
    fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

fn flake_line() -> String {
    json!({"summary": FLAKE_SUMMARY, "kind": "flaky-test", "area": FLAKE_AREA}).to_string()
}

fn novel_line() -> String {
    json!({"summary": NOVEL_SUMMARY, "kind": "bug", "area": "tomlctl/src/io.rs"}).to_string()
}

#[test]
fn a_duplicate_and_a_novel_probe_are_graded_in_one_batch() {
    let (_tmp, root) = sandbox();
    let added = seed_flake(&root);
    let src = stage(&root, &[flake_line(), novel_line()]);

    let v = backlog(&root, &["check", "--ndjson", src.to_str().unwrap()]);
    let results = v["results"].as_array().expect("results is an array");
    assert_eq!(results.len(), 2, "{v}");

    assert_eq!(results[0]["line"], json!(1));
    assert_eq!(results[0]["summary"], json!(FLAKE_SUMMARY));
    assert_eq!(results[0]["verdict"], json!("duplicate"));
    assert_eq!(results[0]["dedup_id"], added["dedup_id"]);
    assert_eq!(results[0]["candidates"][0]["id"], added["id"]);
    assert_eq!(results[0]["candidates"][0]["context"], json!(FLAKE_CONTEXT));

    assert_eq!(results[1]["line"], json!(2));
    assert_eq!(results[1]["verdict"], json!("novel"));
    assert_eq!(results[1]["candidates"], json!([]));

    assert_eq!(v["thresholds"]["strong"].as_f64(), Some(0.75));
}

/// A batch row answers exactly as the single-probe form does for the same
/// probe, so a caller can switch forms without re-learning the verdicts.
#[test]
fn a_batch_row_matches_the_single_probe_verdict() {
    let (_tmp, root) = sandbox();
    seed_flake(&root);
    let src = stage(&root, &[flake_line()]);

    let batch = backlog(&root, &["check", "--ndjson", src.to_str().unwrap()]);
    let single = backlog(
        &root,
        &[
            "check",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ],
    );
    let row = &batch["results"][0];
    for key in ["verdict", "dedup_id", "candidates"] {
        assert_eq!(row[key], single[key], "{key}");
    }
}

#[test]
fn get_verdict_prints_one_verdict_per_probe() {
    let (_tmp, root) = sandbox();
    seed_flake(&root);
    let src = stage(&root, &[flake_line(), novel_line()]);

    let out = backlog_stdout(
        &root,
        &[
            "check",
            "--ndjson",
            src.to_str().unwrap(),
            "--get",
            "verdict",
        ],
    );
    assert_eq!(out.lines().collect::<Vec<_>>(), ["duplicate", "novel"]);
}

#[test]
fn the_batch_reads_its_probes_from_stdin() {
    let (_tmp, root) = sandbox();
    seed_flake(&root);

    let out = cli(&root)
        .args(["backlog", "check", "--ndjson", "-", "--get", "verdict"])
        .write_stdin(format!("{}\n{}\n", novel_line(), flake_line()))
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["novel", "duplicate"]);
}

#[test]
fn a_malformed_line_is_refused_by_its_source_line() {
    let (_tmp, root) = sandbox();
    // The blank line still counts, so the broken payload is line 3.
    let src = stage(
        &root,
        &[flake_line(), String::new(), "{\"summary\":".to_string()],
    );

    let out = cli(&root)
        .args([
            "--error-format",
            "json",
            "backlog",
            "check",
            "--ndjson",
            src.to_str().unwrap(),
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    assert!(out.get_output().stdout.is_empty(), "no partial answer");
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(message.starts_with("line 3: "), "{message}");
}

/// `@<file>` is not a file read on `--summary`; probing the literal string
/// would answer `novel` for a summary nobody wrote, so it is refused instead.
#[test]
fn an_at_file_summary_is_refused_with_the_stdin_form() {
    let (_tmp, root) = sandbox();
    fs::write(root.join("notes.txt"), format!("{FLAKE_SUMMARY}\n")).unwrap();

    let out = cli(&root)
        .current_dir(&root)
        .args([
            "--error-format",
            "json",
            "backlog",
            "check",
            "--summary",
            "@notes.txt",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    assert!(out.get_output().stdout.is_empty(), "no verdict");
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("--summary - < notes.txt"), "{message}");
}

#[test]
fn ndjson_conflicts_with_summary() {
    let (_tmp, root) = sandbox();
    let src = stage(&root, &[flake_line()]);

    cli(&root)
        .args([
            "backlog",
            "check",
            "--summary",
            FLAKE_SUMMARY,
            "--ndjson",
            src.to_str().unwrap(),
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(2);
}
