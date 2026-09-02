//! Integration tests for `tomlctl flow envelope build`.
//!
//! Pure read-only subcommand — no filesystem state to stage. Each test
//! invokes the CLI through `assert_cmd` and asserts on the emitted JSON
//! envelope's shape, fields, and (for the error paths) the
//! `kind=validation` envelope on stderr.

use assert_cmd::Command;

mod common;
use common::parse_json_error_envelope;

/// Acceptance: minimal-args invocation emits a JSON envelope with the
/// command echoed, all optional fields as JSON `null`, every repeatable
/// field as an empty array, and `staleness_threshold` defaulted to "7d".
#[test]
fn flow_envelope_build_minimal_args_emits_default_envelope() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("flow")
        .arg("envelope")
        .arg("build")
        .arg("--command")
        .arg("review")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be parseable JSON");
    assert_eq!(v["command"], serde_json::json!("review"));
    assert!(v["flow_override"].is_null());
    assert_eq!(v["path_args"], serde_json::json!([]));
    assert!(v["branch"].is_null());
    assert!(v["worktree"].is_null());
    assert!(v["cwd"].is_null());
    assert_eq!(v["require_artifacts"], serde_json::json!([]));
    assert_eq!(v["staleness_threshold"], serde_json::json!("7d"));

    // Top-level key set pins the envelope shape — adding or dropping a
    // documented field is a breaking change for downstream consumers, so
    // this assertion guards against an accidental schema drift.
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "branch",
            "command",
            "cwd",
            "flow_override",
            "path_args",
            "require_artifacts",
            "staleness_threshold",
            "worktree",
        ]
    );

    // The key-set assertion above pins only the SET of keys. envelope.rs
    // documents that field ORDER matches the schema doc byte-for-byte (serde_json
    // `preserve_order` enabled), so assert the actual emission order on the raw
    // stdout string — a HashMap round-trip would lose insertion order and not
    // catch a reordering regression. Each key must appear, and at a strictly
    // increasing byte offset, in the documented order.
    let documented_order = [
        "command",
        "flow_override",
        "path_args",
        "branch",
        "worktree",
        "cwd",
        "require_artifacts",
        "staleness_threshold",
    ];
    let mut last_offset: Option<usize> = None;
    for key in documented_order {
        let needle = format!("\"{key}\"");
        let offset = stdout.find(&needle).unwrap_or_else(|| {
            panic!("emitted JSON must contain key {needle}; got stdout:\n{stdout}")
        });
        if let Some(prev) = last_offset {
            assert!(
                offset > prev,
                "field `{key}` must appear after the preceding documented field \
                 (preserve_order byte-for-byte schema order); got stdout:\n{stdout}"
            );
        }
        last_offset = Some(offset);
    }
}

/// Passing the same `--require-artifact` value twice pins the current
/// contract — the impl does NOT de-duplicate, so both occurrences land in
/// `require_artifacts` as a duplicate array `["execution_record",
/// "execution_record"]`. This documents (not changes) the behaviour: if a
/// future refactor adds dedup, this test trips so the contract change is
/// deliberate rather than silent.
#[test]
fn flow_envelope_build_duplicate_require_artifact_preserved_verbatim() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("flow")
        .arg("envelope")
        .arg("build")
        .arg("--command")
        .arg("implement")
        .arg("--require-artifact")
        .arg("execution_record")
        .arg("--require-artifact")
        .arg("execution_record")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be parseable JSON");
    assert_eq!(
        v["require_artifacts"],
        serde_json::json!(["execution_record", "execution_record"]),
        "duplicate --require-artifact values are preserved verbatim (no dedup); got:\n{stdout}"
    );
}

/// Acceptance: every flag wired together produces a faithful echo. Two
/// `--path-arg` values land in `path_args` in order; one
/// `--require-artifact` lands in `require_artifacts`; the override and
/// `cwd`/`branch`/`worktree` round-trip as strings.
#[test]
fn flow_envelope_build_all_fields_set_round_trip() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("flow")
        .arg("envelope")
        .arg("build")
        .arg("--command")
        .arg("implement")
        .arg("--flow-override")
        .arg("feature-x")
        .arg("--path-arg")
        .arg("src/foo.rs")
        .arg("--path-arg")
        .arg("src/bar.rs")
        .arg("--branch")
        .arg("feat/x")
        .arg("--worktree")
        .arg("/abs/work/tree")
        .arg("--cwd")
        .arg("/abs/cwd")
        .arg("--require-artifact")
        .arg("execution_record")
        .arg("--staleness-threshold")
        .arg("3d")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be parseable JSON");
    assert_eq!(v["command"], serde_json::json!("implement"));
    assert_eq!(v["flow_override"], serde_json::json!("feature-x"));
    assert_eq!(
        v["path_args"],
        serde_json::json!(["src/foo.rs", "src/bar.rs"])
    );
    assert_eq!(v["branch"], serde_json::json!("feat/x"));
    assert_eq!(v["worktree"], serde_json::json!("/abs/work/tree"));
    assert_eq!(v["cwd"], serde_json::json!("/abs/cwd"));
    assert_eq!(
        v["require_artifacts"],
        serde_json::json!(["execution_record"])
    );
    assert_eq!(v["staleness_threshold"], serde_json::json!("3d"));
}

/// Acceptance: an unknown `--command` value is rejected with
/// `kind=validation`; stderr surfaces the load-bearing whitelist prose so
/// callers know which strings are accepted without having to read the
/// source.
#[test]
fn flow_envelope_build_rejects_unknown_command() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("--error-format")
        .arg("json")
        .arg("flow")
        .arg("envelope")
        .arg("build")
        .arg("--command")
        .arg("bogus")
        .write_stdin("")
        .assert()
        .failure()
        // Exit 1 is intentional. The impl tags this via
        // `tagged_err(ErrorKind::Validation, ...)`, which routes through the
        // binary's anyhow-error convention (exit 1) consistently across every
        // validation site — do not "fix" this to `.code(2)`; the source exit
        // code is the contract being pinned here.
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("validation"));
    let msg = err["message"].as_str().expect("message populated");
    assert!(
        msg.contains("invalid --command value 'bogus'"),
        "error.message must echo the bad command; got: {msg}"
    );
    // Sanity-check that the whitelist is surfaced — at least one canonical
    // command name appears in the message, so callers can recover.
    assert!(
        msg.contains("review") && msg.contains("implement"),
        "error.message must list the carrier whitelist; got: {msg}"
    );
}

/// Acceptance: an unknown `--require-artifact` value is rejected with
/// `kind=validation`. Mirrors the `--command` validation path so an
/// agent passing a typo'd artifact name fails fast.
#[test]
fn flow_envelope_build_rejects_unknown_require_artifact() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("--error-format")
        .arg("json")
        .arg("flow")
        .arg("envelope")
        .arg("build")
        .arg("--command")
        .arg("implement")
        .arg("--require-artifact")
        .arg("garbage")
        .write_stdin("")
        .assert()
        .failure()
        // Exit 1 is intentional here too — same ErrorKind::Validation /
        // anyhow convention as the `--command` path above. The asserted
        // `.code(1)` pins the binary's real contract.
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("validation"));
    let msg = err["message"].as_str().expect("message populated");
    assert!(
        msg.contains("invalid --require-artifact value 'garbage'"),
        "error.message must echo the bad artifact; got: {msg}"
    );
    assert!(
        msg.contains("execution_record") && msg.contains("review_ledger"),
        "error.message must list the artifact whitelist; got: {msg}"
    );
}
