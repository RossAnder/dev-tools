//! Black-box coverage for the `tomlctl backlog` write verbs — `add`,
//! `relate`, `triage`, `compact` and `evidence dir`.
//!
//! Each case runs the built binary against a throwaway `TOMLCTL_ROOT` that is
//! a real git repository carrying the evidence ignore rules, because
//! `evidence audit`'s `tracked` class shells out to `git check-ignore` and a
//! store outside a repo answers differently.
//!
//! Ids are never hardcoded: every one is read back out of the `add` envelope
//! that minted it, so a change to the id derivation surfaces as a failed
//! shape assertion rather than as a silently-passing lookup.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{
    age_terminal_date, assert_sidecar_matches, backlog, cli, parse_json_error_envelope, sandbox,
    store_path,
};

const FLAKE_SUMMARY: &str = "pty_readiness_probe flakes on slow CI";
const FLAKE_AREA: &str = "lumina/server/tests/pty_readiness_probe.rs";
const FLAKE_CONTEXT: &str = "Only reproduces when the readiness gate races the first prompt write.";
const DRIFT_SUMMARY: &str = "sqlite migration checksum drifts after a renormalise";
const DRIFT_AREA: &str = "lumina/server/db/migrate.rs";

fn sidecar_path(root: &Path) -> PathBuf {
    root.join(".claude").join("backlog.toml.sha256")
}

fn evidence_dir(root: &Path, id: &str) -> PathBuf {
    root.join(".claude").join("backlog-evidence").join(id)
}

/// Store and sidecar bytes together — the pair a dry run must leave alone.
fn snapshot(root: &Path) -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(store_path(root)).unwrap(),
        fs::read(sidecar_path(root)).unwrap(),
    )
}

fn read_store(root: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(store_path(root)).unwrap()).unwrap()
}

fn rows<'a>(doc: &'a toml::Value, array: &str) -> &'a [toml::Value] {
    doc.get(array)
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

fn row<'a>(doc: &'a toml::Value, array: &str, id: &str) -> &'a toml::Value {
    rows(doc, array)
        .iter()
        .find(|r| field(r, "id") == Some(id))
        .unwrap_or_else(|| panic!("no `{array}` row with id {id} in:\n{doc}"))
}

fn field<'a>(row: &'a toml::Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(toml::Value::as_str)
}

fn related(row: &toml::Value) -> Vec<String> {
    row.get("related")
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// A fresh mint is `B-` plus eight lowercase hex.
fn assert_minted_id(id: &str) {
    let hex = id
        .strip_prefix("B-")
        .unwrap_or_else(|| panic!("id must carry the `B-` prefix; got {id:?}"));
    assert_eq!(hex.len(), 8, "a fresh mint is eight hex wide; got {id:?}");
    assert!(
        hex.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "id must be lowercase hex; got {id:?}"
    );
}

/// The core write walk: mint → bump → second capture → relate → triage →
/// compact. Written as one ordered case because each step's assertion is
/// about the state the previous step left behind.
#[test]
fn mint_bump_relate_triage_and_compact_walk() {
    let (_tmp, root) = sandbox();
    let store = store_path(&root);

    let added = backlog(
        &root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
            "--tag",
            "ci",
            "--tag",
            "windows",
            "--context",
            FLAKE_CONTEXT,
        ],
    );
    assert_eq!(added["ok"], json!(true));
    assert_eq!(added["action"], json!("added"));
    assert_eq!(added["created"], json!(true));
    let id_a = added["id"].as_str().unwrap().to_string();
    assert_minted_id(&id_a);
    let dedup_a = added["dedup_id"].as_str().unwrap().to_string();
    assert_eq!(dedup_a.len(), 16, "dedup_id is 16 hex; got {dedup_a:?}");
    assert_sidecar_matches(&store);

    // The same discovery folds onto the row it already minted.
    let bumped = backlog(
        &root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ],
    );
    assert_eq!(bumped["action"], json!("bumped"));
    assert_eq!(bumped["id"].as_str(), Some(id_a.as_str()));
    assert_eq!(bumped["seen_count"], json!(2));

    let second = backlog(
        &root,
        &[
            "add",
            "--summary",
            DRIFT_SUMMARY,
            "--kind",
            "bug",
            "--area",
            DRIFT_AREA,
        ],
    );
    assert_eq!(second["action"], json!("added"));
    assert_eq!(second["created"], json!(false));
    let id_b = second["id"].as_str().unwrap().to_string();
    assert_minted_id(&id_b);
    assert_ne!(id_b, id_a);

    let edge = backlog(
        &root,
        &["relate", &id_a, "--to", &id_b, "--as", "relates-to"],
    );
    assert_eq!(edge["relation"], json!("relates-to"));
    assert_eq!(edge["changed"], json!(true));
    let doc = read_store(&root);
    assert_eq!(related(row(&doc, "backlog", &id_a)), vec![id_b.clone()]);
    assert_eq!(related(row(&doc, "backlog", &id_b)), vec![id_a.clone()]);

    let resolution = "fixed by resolving the binary absolutely";
    let triaged = backlog(
        &root,
        &["triage", &id_a, "--resolve", "--resolution", resolution],
    );
    assert_eq!(triaged["transition"], json!("resolve"));
    assert_eq!(triaged["ids"], json!([id_a]));
    let doc = read_store(&root);
    let a = row(&doc, "backlog", &id_a);
    assert_eq!(field(a, "status"), Some("resolved"));
    assert_eq!(field(a, "resolution"), Some(resolution));
    assert!(
        a.get("resolved").is_some(),
        "a resolved row carries its terminal date; got {a}"
    );
    assert_eq!(
        field(row(&doc, "backlog", &id_b), "status"),
        Some("open"),
        "triage touches only the ids it was handed"
    );
    assert_sidecar_matches(&store);

    // Every write above refreshed the sidecar, so a verifying read passes.
    cli(&root)
        .args(["backlog", "list", "--verify-integrity"])
        .write_stdin("")
        .assert()
        .success();

    age_terminal_date(&store, "resolved");
    let before = snapshot(&root);
    let preview = backlog(&root, &["compact", "--older-than", "90d", "--dry-run"]);
    assert_eq!(preview["dry_run"], json!(true));
    assert_eq!(preview["would_change"]["compacted"], json!(1));
    assert_eq!(preview["would_change"]["remaining"], json!(1));
    assert_eq!(preview["would_change"]["ids"], json!([id_a]));
    assert_eq!(
        snapshot(&root),
        before,
        "`compact --dry-run` must leave the store and its sidecar byte-identical"
    );

    let folded = backlog(&root, &["compact", "--older-than", "90d"]);
    assert_eq!(folded["ok"], json!(true));
    assert_eq!(folded["compacted"], json!(1));
    assert_eq!(folded["remaining"], json!(1));
    let doc = read_store(&root);
    assert!(
        rows(&doc, "backlog")
            .iter()
            .all(|r| field(r, "id") != Some(id_a.as_str())),
        "the folded row leaves the live array; got:\n{doc}"
    );
    let folded_row = row(&doc, "compacted", &id_a);
    assert_eq!(field(folded_row, "dedup_id"), Some(dedup_a.as_str()));
    assert_eq!(field(folded_row, "context"), Some(FLAKE_CONTEXT));
    assert_eq!(field(folded_row, "status"), Some("resolved"));
    assert_sidecar_matches(&store);
}

#[test]
fn add_dry_run_leaves_the_store_and_sidecar_byte_identical() {
    let (_tmp, root) = sandbox();
    backlog(
        &root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ],
    );
    let before = snapshot(&root);

    let preview = backlog(
        &root,
        &[
            "add",
            "--summary",
            "statusline row layout drifts under a narrow terminal",
            "--kind",
            "annoyance",
            "--dry-run",
        ],
    );
    assert_eq!(preview["ok"], json!(true));
    assert_eq!(preview["dry_run"], json!(true));
    assert_eq!(preview["would_change"]["added"], json!(1));
    assert_eq!(preview["would_change"]["updated"], json!(0));
    let previewed = preview["would_change"]["ids"][0].as_str().unwrap();
    assert_minted_id(previewed);

    assert_eq!(
        snapshot(&root),
        before,
        "`add --dry-run` must leave the store and its sidecar byte-identical"
    );
}

/// The coercion is fail-soft — the capture succeeds and the row stores
/// `other` — so the warning on stderr is the only signal that the kind the
/// caller typed was not understood. Nothing else would notice it going away.
#[test]
fn an_unknown_kind_warns_on_stderr() {
    let (_tmp, root) = sandbox();

    let out = cli(&root)
        .args([
            "backlog",
            "add",
            "--summary",
            DRIFT_SUMMARY,
            "--kind",
            "regression",
            "--area",
            DRIFT_AREA,
        ])
        .write_stdin("")
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    for fragment in ["unknown backlog kind", "`regression`", "`other`"] {
        assert!(
            stderr.contains(fragment),
            "the unknown-kind warning must carry {fragment}; got: {stderr:?}"
        );
    }
}

/// Each backlog read verb threads `--verify-integrity` through its own call
/// site, so a passing verified read cannot tell a plumbed verb from one that
/// silently drops the flag. Only a sidecar that no longer covers the store
/// separates them.
#[test]
fn list_verify_integrity_rejects_a_tampered_sidecar() {
    let (_tmp, root) = sandbox();
    backlog(
        &root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ],
    );

    let sidecar = sidecar_path(&root);
    let mut digest = fs::read(&sidecar).unwrap();
    digest[0] = if digest[0] == b'0' { b'1' } else { b'0' };
    fs::write(&sidecar, digest).unwrap();

    let out = cli(&root)
        .args([
            "--error-format",
            "json",
            "backlog",
            "list",
            "--verify-integrity",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], json!("integrity"));
}

#[test]
fn evidence_dir_writes_only_the_marker_and_is_idempotent() {
    let (_tmp, root) = sandbox();
    let added = backlog(
        &root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ],
    );
    let id = added["id"].as_str().unwrap().to_string();
    let before = snapshot(&root);

    let first = backlog(&root, &["evidence", "dir", &id]);
    assert_eq!(first["ok"], json!(true));
    assert_eq!(first["id"].as_str(), Some(id.as_str()));
    assert_eq!(first["created"], json!(true));
    assert_eq!(first["files"], json!(0));
    assert_eq!(
        first["dir"],
        json!(format!(".claude/backlog-evidence/{id}"))
    );

    let dir = evidence_dir(&root, &id);
    assert!(dir.is_dir(), "the drop-box must exist at {}", dir.display());
    let marker = fs::read_to_string(dir.join(".evidence")).unwrap();
    assert!(
        marker.lines().next().unwrap().starts_with(&id),
        "the marker's caption line opens with the item id; got {marker:?}"
    );
    assert_eq!(
        snapshot(&root),
        before,
        "`evidence dir` writes the marker and nothing else"
    );

    let second = backlog(&root, &["evidence", "dir", &id]);
    assert_eq!(second["created"], json!(false));
    assert_eq!(second["files"], json!(0));
}

#[test]
fn on_duplicate_fail_reports_a_validation_envelope_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    backlog(
        &root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ],
    );
    let before = snapshot(&root);

    let out = cli(&root)
        .args([
            "--error-format",
            "json",
            "backlog",
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
            "--on-duplicate",
            "fail",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], json!("validation"));

    assert_eq!(
        snapshot(&root),
        before,
        "a refused duplicate must leave the store and its sidecar untouched"
    );
}

/// One store id maps to one row, so there is no mode that appends a second
/// row under an id the store already holds. The parser is where that is
/// refused — `add` never sees the value.
#[test]
fn on_duplicate_add_is_not_a_parser_value() {
    let (_tmp, root) = sandbox();
    cli(&root)
        .args([
            "backlog",
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
            "--on-duplicate",
            "add",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(2);
    assert!(
        !store_path(&root).exists(),
        "a rejected parse must not create the store"
    );
}

#[test]
fn dismiss_stores_its_terminal_date_and_reason() {
    let (_tmp, root) = sandbox();
    let added = backlog(
        &root,
        &[
            "add",
            "--summary",
            DRIFT_SUMMARY,
            "--kind",
            "bug",
            "--area",
            DRIFT_AREA,
        ],
    );
    let id = added["id"].as_str().unwrap().to_string();

    let reason = "the renormalise guard removed the drift";
    let triaged = backlog(&root, &["triage", &id, "--dismiss", "--reason", reason]);
    assert_eq!(triaged["transition"], json!("dismiss"));
    assert_eq!(triaged["ids"], json!([id]));

    let doc = read_store(&root);
    let dismissed = row(&doc, "backlog", &id);
    assert_eq!(field(dismissed, "status"), Some("dismissed"));
    assert_eq!(field(dismissed, "dismiss_reason"), Some(reason));
    assert!(
        dismissed.get("dismissed").is_some(),
        "a dismissed row carries its terminal date; got {dismissed}"
    );
    assert_sidecar_matches(&store_path(&root));
}

#[test]
fn triage_with_two_mode_flags_is_a_parser_error() {
    let (_tmp, root) = sandbox();
    let out = cli(&root)
        .args([
            "backlog",
            "triage",
            "B-a1b2c3d4",
            "--promote",
            "--to",
            "docs/plans/x.md",
            "--dismiss",
            "--reason",
            "not worth it",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(2);
    // Exit 2 is clap's blanket usage code — a misspelt flag earns it too, and
    // would leave the mutual exclusion itself untested. The conflict wording
    // is what distinguishes it from an unrecognised argument.
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("cannot be used with"),
        "the refusal must be a conflict, not an unrecognised flag; got: {stderr:?}"
    );
}
