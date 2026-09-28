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
    TASKS_SLUG, age_terminal_date, assert_sidecar_matches, backlog, cli, parse_json_error_envelope,
    sandbox, seed_tasks, store_path,
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
    assert_eq!(preview["path"], json!(".claude/backlog.toml"));
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
/// `other` — so the advisory is the only signal that the kind the caller
/// typed was not understood. Nothing else would notice it going away, and a
/// captured stderr never carries it, so the envelope is where it has to be.
#[test]
fn an_unknown_kind_is_coerced_and_the_envelope_says_so() {
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
    let out = out.get_output();
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");

    let envelope: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    let advisory = envelope["advisories"][0]
        .as_str()
        .unwrap_or_else(|| panic!("the coercion must reach the envelope: {envelope}"));
    for fragment in ["unknown backlog kind", "`regression`", "`other`"] {
        assert!(
            advisory.contains(fragment),
            "the unknown-kind advisory must carry {fragment}; got: {advisory:?}"
        );
    }
    assert_eq!(
        field(
            row(
                &read_store(&root),
                "backlog",
                envelope["id"].as_str().unwrap()
            ),
            "kind"
        ),
        Some("other")
    );
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

fn add_drift(root: &Path) -> String {
    let added = backlog(
        root,
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
    added["id"].as_str().unwrap().to_string()
}

/// Run a refused `backlog triage <args…>` and hand back its JSON error.
fn refused_triage(root: &Path, args: &[&str]) -> serde_json::Value {
    let out = cli(root)
        .args(["--error-format", "json", "backlog", "triage"])
        .args(args)
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    parse_json_error_envelope(&stderr)
}

fn promoted_to(root: &Path, id: &str) -> Option<String> {
    field(row(&read_store(root), "backlog", id), "promoted_to").map(str::to_owned)
}

/// A flow bound to `docs/plans/<TASKS_SLUG>.md` whose `context.toml` sits at
/// `status` rather than the fixture's `in-progress`.
fn seed_flow_at(root: &Path, status: &str) {
    seed_tasks(root, "schema_version = 1\n");
    let ctx = root
        .join(".claude")
        .join("flows")
        .join(TASKS_SLUG)
        .join("context.toml");
    let text = fs::read_to_string(&ctx).unwrap();
    let rewritten = text.replace(
        "status = \"in-progress\"",
        &format!("status = \"{status}\""),
    );
    assert_ne!(rewritten, text, "the seeded context must carry a status");
    fs::write(&ctx, rewritten).unwrap();
}

#[test]
fn promote_to_an_unknown_target_is_not_found_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let id = add_drift(&root);
    let before = snapshot(&root);

    let err = refused_triage(&root, &[&id, "--promote", "--to", "no-such-flow"]);
    assert_eq!(err["kind"], json!("not_found"), "{err}");
    assert_eq!(err["arg"], json!("to"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("`no-such-flow`"), "{message}");
    assert!(message.contains("--external"), "{message}");
    assert_eq!(
        snapshot(&root),
        before,
        "a refused promotion must leave the store and its sidecar untouched"
    );
}

/// An unknown id and an unknown `--to` are both `not_found`; `arg` is what
/// tells a caller which of the two it has.
#[test]
fn promote_of_an_unknown_id_names_the_ids_argument() {
    let (_tmp, root) = sandbox();
    add_drift(&root);
    let before = snapshot(&root);

    let err = refused_triage(
        &root,
        &["B-00000000", "--promote", "--to", "external:GH-12"],
    );
    assert_eq!(err["kind"], json!("not_found"), "{err}");
    assert_eq!(err["arg"], json!("ids"), "{err}");
    assert_eq!(snapshot(&root), before);
}

#[test]
fn promote_external_stores_the_prefix_exactly_once() {
    let (_tmp, root) = sandbox();
    let id = add_drift(&root);

    let out = backlog(
        &root,
        &[
            "triage",
            &id,
            "--promote",
            "--to",
            "task-store-polish",
            "--external",
        ],
    );
    assert_eq!(out["transition"], json!("promote"));
    assert_eq!(out["to"], json!("external:task-store-polish"));
    assert_eq!(
        promoted_to(&root, &id).as_deref(),
        Some("external:task-store-polish")
    );

    let out = backlog(
        &root,
        &[
            "triage",
            &id,
            "--promote",
            "--to",
            "external:GH-12",
            "--external",
        ],
    );
    assert_eq!(out["to"], json!("external:GH-12"));
    assert_eq!(promoted_to(&root, &id).as_deref(), Some("external:GH-12"));
    assert_sidecar_matches(&store_path(&root));
}

#[test]
fn promote_into_a_closed_flow_needs_allow_closed() {
    let (_tmp, root) = sandbox();
    seed_flow_at(&root, "review");
    let id = add_drift(&root);
    let before = snapshot(&root);

    let err = refused_triage(&root, &[&id, "--promote", "--to", TASKS_SLUG]);
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert_eq!(err["arg"], json!("to"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("`review`"), "{message}");
    assert!(message.contains("--allow-closed"), "{message}");
    assert_eq!(snapshot(&root), before);

    let out = backlog(
        &root,
        &[
            "triage",
            &id,
            "--promote",
            "--to",
            TASKS_SLUG,
            "--allow-closed",
        ],
    );
    assert_eq!(out["to"], json!(TASKS_SLUG));
    assert_eq!(promoted_to(&root, &id).as_deref(), Some(TASKS_SLUG));
}

#[test]
fn promote_to_a_bound_plan_path_stores_the_flow_slug() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, "schema_version = 1\n");
    let id = add_drift(&root);
    let plan = format!("docs/plans/{TASKS_SLUG}.md");

    let out = backlog(&root, &["triage", &id, "--promote", "--to", &plan]);
    assert_eq!(out["to"], json!(TASKS_SLUG));
    assert_eq!(promoted_to(&root, &id).as_deref(), Some(TASKS_SLUG));
}

/// stderr is a machine channel: under `--error-format json` it carries the
/// error envelope and nothing else, and every other capture of it — an NDJSON
/// stream, a test harness, a logged agent run — has a parser on the far end.
/// Advisories are therefore terminal-only, and the credential scan reaches a
/// piped caller through the success envelope instead of through stderr.
#[test]
fn advisories_stay_off_a_captured_stderr_and_ride_the_envelope() {
    let (_dir, root) = sandbox();
    let leaky = "auth fails once the ghp_deadbeefcafe token expires";
    let capture = [
        "backlog",
        "add",
        "--summary",
        leaky,
        "--kind",
        "bug",
        "--area",
        "tomlctl/src/io.rs",
    ];

    let minted = cli(&root).args(capture).write_stdin("").assert().success();
    let minted = minted.get_output();
    assert_eq!(
        String::from_utf8_lossy(&minted.stderr),
        "",
        "neither the credential note nor the created-file note may reach a captured stderr"
    );

    let envelope: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&minted.stdout).trim()).unwrap();
    assert_eq!(envelope["created"], json!(true), "{envelope}");
    let advisories = envelope["advisories"]
        .as_array()
        .unwrap_or_else(|| panic!("the envelope must carry `advisories`: {envelope}"));
    assert_eq!(advisories.len(), 1, "{envelope}");
    let advisory = advisories[0].as_str().unwrap();
    assert!(advisory.contains("a credential"), "{advisory}");
    assert!(
        !advisory.contains("ghp_"),
        "the advisory names the field, never the value: {advisory}"
    );

    // The same advisory ahead of a failing run: stderr is the envelope alone.
    let refused = cli(&root)
        .args(["--error-format", "json"])
        .args(capture)
        .args(["--on-duplicate", "fail"])
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert_eq!(
        stderr.lines().count(),
        1,
        "stderr must be exactly the JSON error envelope: {stderr}"
    );
    assert_eq!(
        parse_json_error_envelope(&stderr)["kind"],
        json!("validation"),
        "{stderr}"
    );
}

/// A caller that reviews a capture with `--dry-run` before committing it must
/// not see less than one that skips the review. The stderr line is
/// terminal-gated, so on a pipe the preview envelope is the only place the
/// credential scan can land.
#[test]
fn a_dry_run_surfaces_the_credential_advisory_on_the_preview_envelope() {
    let (_dir, root) = sandbox();
    let leaky = "auth fails once the ghp_deadbeefcafe token expires";

    let preview = cli(&root)
        .args([
            "backlog",
            "add",
            "--summary",
            leaky,
            "--kind",
            "bug",
            "--area",
            "tomlctl/src/io.rs",
            "--dry-run",
        ])
        .write_stdin("")
        .assert()
        .success();
    let preview = preview.get_output();
    assert_eq!(
        String::from_utf8_lossy(&preview.stderr),
        "",
        "a captured stderr carries no advisory, which is why the envelope must"
    );

    let envelope: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&preview.stdout).trim()).unwrap();
    assert_eq!(envelope["dry_run"], json!(true), "{envelope}");
    assert_eq!(envelope["would_change"]["added"], json!(1), "{envelope}");
    let advisories = envelope["advisories"]
        .as_array()
        .unwrap_or_else(|| panic!("the preview envelope must carry `advisories`: {envelope}"));
    assert_eq!(advisories.len(), 1, "{envelope}");
    let advisory = advisories[0].as_str().unwrap();
    assert!(advisory.contains("a credential"), "{advisory}");
    assert!(
        !advisory.contains("ghp_"),
        "the advisory names the field, never the value: {advisory}"
    );
}

fn flake_line() -> String {
    json!({"summary": FLAKE_SUMMARY, "kind": "flaky-test", "area": FLAKE_AREA}).to_string()
}

fn drift_line() -> String {
    json!({"summary": DRIFT_SUMMARY, "kind": "bug", "area": DRIFT_AREA}).to_string()
}

/// Run a refused `backlog add-many --ndjson -` over `ndjson` and hand back
/// its JSON error.
fn refused_batch(root: &Path, ndjson: String) -> serde_json::Value {
    let out = cli(root)
        .args([
            "--error-format",
            "json",
            "backlog",
            "add-many",
            "--ndjson",
            "-",
        ])
        .write_stdin(ndjson)
        .assert()
        .failure()
        .code(1);
    parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr))
}

#[test]
fn add_many_mints_a_batch_and_folds_a_repeat_onto_its_earlier_line() {
    let (_tmp, root) = sandbox();
    let batch = root.join("batch.ndjson");
    let repeat =
        json!({"summary": FLAKE_SUMMARY, "kind": "flaky-test", "area": FLAKE_AREA, "tags": ["ci"]});
    fs::write(
        &batch,
        format!("{}\n{}\n{repeat}\n", flake_line(), drift_line()),
    )
    .unwrap();

    let out = backlog(
        &root,
        &["add-many", "--ndjson", &format!("@{}", batch.display())],
    );
    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!(out["created"], json!(true), "{out}");
    assert_eq!(out["path"], json!(".claude/backlog.toml"), "{out}");
    let rows_out = out["rows"].as_array().unwrap();
    assert_eq!(rows_out.len(), 3, "{out}");
    let id_a = rows_out[0]["id"].as_str().unwrap().to_string();
    let id_b = rows_out[1]["id"].as_str().unwrap().to_string();
    assert_minted_id(&id_a);
    assert_minted_id(&id_b);
    assert_eq!(out["added"], json!([id_a, id_b]), "{out}");
    assert_eq!(out["bumped"], json!([id_a]), "{out}");
    assert_eq!(rows_out[0]["line"], json!(1));
    assert_eq!(rows_out[0]["action"], json!("added"));
    assert_eq!(rows_out[2]["line"], json!(3));
    assert_eq!(rows_out[2]["action"], json!("bumped"));
    assert_eq!(rows_out[2]["id"].as_str(), Some(id_a.as_str()));
    assert_eq!(rows_out[2]["seen_count"], json!(2));

    let doc = read_store(&root);
    assert_eq!(rows(&doc, "backlog").len(), 2, "{doc}");
    let a = row(&doc, "backlog", &id_a);
    assert_eq!(a["seen_count"].as_integer(), Some(2));
    assert_eq!(a["tags"][0].as_str(), Some("ci"));
    assert_sidecar_matches(&store_path(&root));
}

#[test]
fn add_many_with_a_malformed_line_writes_nothing() {
    let (_tmp, root) = sandbox();
    add_drift(&root);
    let before = snapshot(&root);

    let err = refused_batch(&root, format!("{}\n{{\"summary\": \n", flake_line()));
    assert_eq!(err["kind"], json!("validation"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.starts_with("line 2: "), "{message}");
    assert_eq!(
        snapshot(&root),
        before,
        "a batch with one bad line must leave the store and its sidecar byte-identical"
    );
}

#[test]
fn add_many_names_the_line_carrying_an_unknown_key() {
    let (_tmp, root) = sandbox();
    let typo = json!({"summary": "a row with a typo'd field", "contxt": "x"});

    let err = refused_batch(
        &root,
        format!("{}\n{}\n{typo}\n", flake_line(), drift_line()),
    );
    assert_eq!(err["kind"], json!("validation"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.starts_with("line 3: "), "{message}");
    assert!(message.contains("`contxt`"), "{message}");
    assert!(
        !store_path(&root).exists(),
        "a refused batch must not create the store"
    );
}
