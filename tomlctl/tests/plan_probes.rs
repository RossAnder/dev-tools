//! Black-box coverage for `plan probes`: which rows the script runs, that a
//! stored command reaches Bash unchanged, and the verdict `--record` computes
//! from each capture.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{
    TASKS_SLUG, cli, git_bash, git_command, hex_digest, parse_json_error_envelope, sandbox,
    seed_tasks,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const FILE: &str = "src/declared.rs";
/// What `row` writes in place of a digest, until `record` looks up the real one.
const UNSTAMPED: &str = "sha256=unstamped";

fn digest(command: &str) -> String {
    hex_digest(command.as_bytes())
}

fn store(accepts: Value) -> String {
    let doc = json!({
        "schema_version": 2,
        "items": [{
            "id": 1,
            "ref": "the-row",
            "title": "The row",
            "effort": "S",
            "files": [FILE],
        }],
        "specs": [{ "ref": "the-row", "outcome": "it holds", "accept": accepts }],
    });
    toml::to_string(&doc).expect("the fixture serialises")
}

fn accept(kind: &str, check: &str, positive: &str) -> Value {
    json!({
        "behaviour": "it holds",
        "check": check,
        "kind": kind,
        "positive": positive,
        "writes_build": false,
    })
}

fn emit(root: &Path) -> String {
    let out = cli(root)
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--emit"])
        .assert()
        .success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

/// Gives each `row` the digest `--emit` prints for its control, so a case varies
/// only what it names. A control the script does not run stays unstamped.
fn stamped(root: &Path, capture: &str) -> String {
    if !capture.contains(UNSTAMPED) {
        return capture.to_string();
    }
    let script = emit(root);
    capture
        .split_inclusive('\n')
        .map(|line| {
            let mut columns = line.split('\t');
            let (Some(key), Some(control)) = (columns.next(), columns.next()) else {
                return line.to_string();
            };
            let prefix = format!("p {key} {control} ");
            let emitted = script
                .lines()
                .find_map(|l| l.strip_prefix(&prefix))
                .and_then(|rest| rest.split(' ').next());
            match emitted {
                Some(hex) => line.replacen(UNSTAMPED, &format!("sha256={hex}"), 1),
                None => line.to_string(),
            }
        })
        .collect()
}

fn record(root: &Path, capture: &str) -> assert_cmd::assert::Assert {
    record_with(root, capture, &[])
}

fn record_with(root: &Path, capture: &str, flags: &[&str]) -> assert_cmd::assert::Assert {
    let path = root.join("capture.tsv");
    fs::write(&path, stamped(root, capture)).unwrap();
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--record"])
        .arg(&path)
        .args(flags)
        .assert()
}

fn probes(store_path: &Path) -> Vec<Option<toml::Value>> {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(store_path).unwrap()).unwrap();
    doc["specs"][0]["accept"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.get("probe").cloned())
        .collect()
}

fn verdict(probe: &Option<toml::Value>) -> &str {
    probe.as_ref().expect("a recorded probe")["verdict"]
        .as_str()
        .expect("a string verdict")
}

fn row(key: &str, control: &str, rc: i64, stderr: &str) -> String {
    format!("{key}\t{control}\t{UNSTAMPED}\trc={rc}\t\t{stderr}\n")
}

/// The verdict recorded for one row whose capture holds `rows`.
fn verdict_of(accepts: Value, rows: &[String]) -> String {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &store(accepts));
    record(&root, &format!("head\t{SHA}\n{}", rows.concat())).success();
    verdict(&probes(&path)[0]).to_string()
}

fn assert_refused(root: &Path, store_path: &Path, capture: &str, expected: &str) {
    let before = fs::read_to_string(store_path).unwrap();
    let assert = record(root, capture).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    assert!(
        error["message"].as_str().unwrap().contains(expected),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(store_path).unwrap(),
        before,
        "a refused capture must write nothing"
    );
}

#[test]
fn a_failing_forward_with_no_positive_is_healthy() {
    let verdict = verdict_of(
        json!([accept("forward", "c", "")]),
        &[row("1:0", "neg", 1, "")],
    );
    assert_eq!(verdict, "healthy");
}

#[test]
fn a_forward_that_already_holds_is_vacuous() {
    let verdict = verdict_of(
        json!([accept("forward", "c", "")]),
        &[row("1:0", "neg", 0, "")],
    );
    assert_eq!(verdict, "vacuous");
}

#[test]
fn a_forward_whose_positive_passes_is_healthy() {
    let verdict = verdict_of(
        json!([accept("forward", "c", "p")]),
        &[row("1:0", "neg", 1, ""), row("1:0", "pos", 0, "")],
    );
    assert_eq!(verdict, "healthy");
}

#[test]
fn a_forward_whose_positive_fails_is_unsatisfiable() {
    let verdict = verdict_of(
        json!([accept("forward", "c", "p")]),
        &[row("1:0", "neg", 1, ""), row("1:0", "pos", 1, "")],
    );
    assert_eq!(verdict, "unsatisfiable");
}

#[test]
fn a_falsifier_that_holds_is_healthy() {
    let verdict = verdict_of(
        json!([accept("falsifier", "c", "")]),
        &[row("1:0", "neg", 0, "")],
    );
    assert_eq!(verdict, "healthy");
}

#[test]
fn a_falsifier_that_does_not_hold_is_permanently_green() {
    let verdict = verdict_of(
        json!([accept("falsifier", "c", "")]),
        &[row("1:0", "neg", 1, "")],
    );
    assert_eq!(verdict, "permanently-green");
}

#[test]
fn every_breakage_marker_is_broken_whatever_the_exit_status() {
    for marker in [
        "bash: nope: command not found",
        "fatal: not a git repository",
        "bash: eval: line 1: syntax error near unexpected token",
        "bash: eval: line 1: unexpected EOF while looking for matching",
    ] {
        for rc in [0, 1] {
            let verdict = verdict_of(
                json!([accept("forward", "c", "")]),
                &[row("1:0", "neg", rc, marker)],
            );
            assert_eq!(verdict, "broken", "rc {rc}, stderr {marker}");
        }
    }
}

#[test]
fn a_breakage_on_the_positive_control_is_broken() {
    let verdict = verdict_of(
        json!([accept("forward", "c", "p")]),
        &[
            row("1:0", "neg", 1, ""),
            row("1:0", "pos", 0, "bash: nope: command not found"),
        ],
    );
    assert_eq!(verdict, "broken");
}

#[test]
fn a_missing_undeclared_path_is_broken() {
    let verdict = verdict_of(
        json!([accept("forward", "c", "")]),
        &[row(
            "1:0",
            "neg",
            2,
            "grep: src/typo.rs: No such file or directory",
        )],
    );
    assert_eq!(verdict, "broken");
}

#[test]
fn a_missing_declared_path_is_what_the_task_creates() {
    for stderr in [
        "grep: src/declared.rs: No such file or directory",
        "ls: cannot access './src/declared.rs': No such file or directory",
        "grep: declared.rs: No such file or directory",
        "warning: nothing|grep: src/declared.rs: No such file or directory",
    ] {
        let verdict = verdict_of(
            json!([accept("forward", "c", "")]),
            &[row("1:0", "neg", 2, stderr)],
        );
        assert_eq!(verdict, "healthy", "{stderr}");
    }
}

#[test]
fn a_declared_path_does_not_excuse_an_undeclared_one_beside_it() {
    let verdict = verdict_of(
        json!([accept("forward", "c", "")]),
        &[row(
            "1:0",
            "neg",
            2,
            "grep: src/declared.rs: No such file or directory|grep: src/typo.rs: No such file or directory",
        )],
    );
    assert_eq!(verdict, "broken");
}

#[test]
fn a_guard_with_no_check_beside_it_is_guard_alone() {
    let verdict = verdict_of(
        json!([accept("guard", "c", "")]),
        &[row("1:0", "neg", 0, "")],
    );
    assert_eq!(verdict, "guard-alone");
}

#[test]
fn a_guard_beside_a_healthy_forward_is_healthy() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(
        &root,
        &store(json!([
            accept("guard", "g", ""),
            accept("forward", "c", ""),
        ])),
    );
    record(
        &root,
        &format!(
            "head\t{SHA}\n{}{}",
            row("1:0", "neg", 0, ""),
            row("1:1", "neg", 1, "")
        ),
    )
    .success();
    let probes = probes(&path);
    assert_eq!(verdict(&probes[0]), "healthy");
    assert_eq!(verdict(&probes[1]), "healthy");
}

#[test]
fn a_guard_beside_a_vacuous_forward_stands_alone() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(
        &root,
        &store(json!([
            accept("guard", "g", ""),
            accept("forward", "c", ""),
        ])),
    );
    record(
        &root,
        &format!(
            "head\t{SHA}\n{}{}",
            row("1:0", "neg", 0, ""),
            row("1:1", "neg", 0, "")
        ),
    )
    .success();
    let probes = probes(&path);
    assert_eq!(verdict(&probes[0]), "guard-alone");
    assert_eq!(verdict(&probes[1]), "vacuous");
}

#[test]
fn a_guard_beside_an_unprobed_forward_is_healthy() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(
        &root,
        &store(json!([
            accept("guard", "g", ""),
            { "kind": "forward", "check": "cargo test", "behaviour": "it holds" },
        ])),
    );
    record(&root, &format!("head\t{SHA}\n{}", row("1:0", "neg", 0, ""))).success();
    assert_eq!(verdict(&probes(&path)[0]), "healthy");
}

#[test]
fn a_guard_that_does_not_hold_is_broken() {
    let verdict = verdict_of(
        json!([accept("guard", "g", ""), accept("forward", "c", "")]),
        &[row("1:0", "neg", 1, ""), row("1:1", "neg", 1, "")],
    );
    assert_eq!(verdict, "broken");
}

#[test]
fn record_stores_the_head_and_both_return_codes() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(
        &root,
        &store(json!([
            accept("forward", "c", "p"),
            accept("forward", "d", ""),
        ])),
    );
    let out = record(
        &root,
        &format!(
            "head\t{SHA}\n{}{}{}",
            row("1:0", "neg", 3, ""),
            row("1:0", "pos", 0, ""),
            row("1:1", "neg", 4, "")
        ),
    )
    .success();
    let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(
        report,
        json!({
            "ok": true,
            "recorded": [
                { "task": 1, "accept": 0, "verdict": "healthy", "reason": "" },
                { "task": 1, "accept": 1, "verdict": "healthy", "reason": "" },
            ],
        })
    );

    let probes = probes(&path);
    let first = probes[0].as_ref().unwrap();
    assert_eq!(first["at"].as_str(), Some(SHA));
    assert_eq!(first["neg_rc"].as_integer(), Some(3));
    assert_eq!(first["pos_rc"].as_integer(), Some(0));
    let second = probes[1].as_ref().unwrap();
    assert_eq!(second["neg_rc"].as_integer(), Some(4));
    assert!(
        second.get("pos_rc").is_none(),
        "a control that did not run has no return code: {second}"
    );
}

#[test]
fn record_replaces_an_earlier_probe() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &store(json!([accept("forward", "c", "")])));
    record(&root, &format!("head\t{SHA}\n{}", row("1:0", "neg", 0, ""))).success();
    assert_eq!(verdict(&probes(&path)[0]), "vacuous");
    record(&root, &format!("head\t{SHA}\n{}", row("1:0", "neg", 1, ""))).success();
    assert_eq!(verdict(&probes(&path)[0]), "healthy");
}

#[test]
fn a_crlf_capture_records_the_same_verdict() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &store(json!([accept("forward", "c", "")])));
    let capture = format!("head\t{SHA}\n{}", row("1:0", "neg", 1, "")).replace('\n', "\r\n");
    record(&root, &capture).success();
    let probes = probes(&path);
    assert_eq!(verdict(&probes[0]), "healthy");
    assert_eq!(probes[0].as_ref().unwrap()["at"].as_str(), Some(SHA));
}

#[test]
fn a_capture_that_does_not_match_the_store_is_refused_whole() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(
        &root,
        &store(json!([
            accept("forward", "c", "p"),
            accept("forward", "d", ""),
        ])),
    );
    let neg0 = row("1:0", "neg", 1, "");
    let pos0 = row("1:0", "pos", 0, "");
    let neg1 = row("1:1", "neg", 1, "");
    let head = format!("head\t{SHA}\n");

    assert_refused(&root, &path, &format!("{neg0}{pos0}{neg1}"), "head");
    assert_refused(&root, &path, &format!("head\t\n{neg0}{pos0}{neg1}"), "head");
    assert_refused(&root, &path, &format!("{head}{neg0}{neg1}"), "pos control");
    assert_refused(&root, &path, &format!("{head}{pos0}{neg1}"), "neg control");
    assert_refused(&root, &path, &format!("{head}{neg0}{pos0}"), "1 of the 2");
    assert_refused(
        &root,
        &path,
        &format!("{head}{neg0}{pos0}{neg1}{}", row("1:1", "pos", 0, "")),
        "pos control",
    );
    assert_refused(
        &root,
        &path,
        &format!("{head}{neg0}{pos0}{neg1}{}", row("9:0", "neg", 0, "")),
        "9:0",
    );
    assert_refused(
        &root,
        &path,
        &format!("{head}{neg0}{neg0}{pos0}{neg1}"),
        "repeats",
    );
    for malformed in [
        "1:0\tneg\tsha256=a\trc=1\n",
        "1:0\tneg\tsha256=a\trc=x\t\t\n",
        "1\tneg\tsha256=a\trc=1\t\t\n",
        "1:0\tmid\tsha256=a\trc=1\t\t\n",
    ] {
        assert_refused(
            &root,
            &path,
            &format!("{head}{malformed}"),
            "not a probe row",
        );
    }
}

#[test]
fn a_bad_capture_line_shows_the_expected_shape_and_a_cut_copy_of_the_line() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &store(json!([accept("forward", "c", "")])));
    let long = format!("1:0\tmid\t{}\n", "x".repeat(200));
    let assert = record(&root, &format!("head\t{SHA}\n{long}")).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    let message = error["message"].as_str().expect("a message");
    assert!(
        message.contains("capture line 2 is not a probe row"),
        "{message}"
    );
    assert!(
        message.contains("<task>:<index><TAB>neg|pos|premise"),
        "{message}"
    );
    assert!(message.contains("1:0\\tmid\\t"), "{message}");
    assert!(message.ends_with("..."), "{message}");
    assert!(!message.contains(&"x".repeat(100)), "{message}");
    assert!(probes(&path)[0].is_none());
}

#[test]
fn a_capture_with_no_head_line_shows_what_it_opens_with() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &store(json!([accept("forward", "c", "")])));
    assert_refused(&root, &path, "not a head\n", "got \"not a head\"");
    assert_refused(&root, &path, "", "got nothing");
}

#[test]
fn a_missing_capture_path_is_not_found_and_names_the_file() {
    let (_dir, root) = sandbox();
    let store_path = seed_tasks(&root, &store(json!([accept("forward", "c", "")])));
    let before = fs::read(&store_path).unwrap();
    let capture = root.join("absent.tsv");

    let assert = cli(&root)
        .args(["--error-format", "json"])
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--record"])
        .arg(&capture)
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "not_found", "{stderr}");
    assert_eq!(error["file"], capture.display().to_string(), "{stderr}");
    assert_eq!(fs::read(&store_path).unwrap(), before);
}

#[test]
fn emit_on_a_flow_with_no_stores_names_plan_open() {
    let (_dir, root) = sandbox();
    let assert = cli(&root)
        .args(["--error-format", "json"])
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--emit"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "not_found", "{stderr}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|m| m.contains(&format!("tomlctl plan open --slug {TASKS_SLUG}"))),
        "{stderr}"
    );
}

/// The `reason` of the one row `rows` records.
fn reason_of(accepts: Value, rows: &[String]) -> (String, String) {
    let (_dir, root) = sandbox();
    seed_tasks(&root, &store(accepts));
    let out = record(&root, &format!("head\t{SHA}\n{}", rows.concat())).success();
    let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let row = &report["recorded"][0];
    (
        row["verdict"].as_str().expect("a verdict").to_string(),
        row["reason"].as_str().expect("a reason").to_string(),
    )
}

#[test]
fn each_unhealthy_verdict_names_the_control_and_the_rule() {
    let forward = || json!([accept("forward", "c", "p")]);
    let cases = [
        (
            forward(),
            vec![
                row("1:0", "neg", 0, ""),
                row("1:0", "pos", 0, "bash: nope: command not found"),
            ],
            "broken",
            "pos: stderr holds the marker `command not found`",
        ),
        (
            json!([accept("forward", "c", "")]),
            vec![row(
                "1:0",
                "neg",
                2,
                "warn|grep: src/typo.rs: No such file or directory",
            )],
            "broken",
            "neg: stderr names a missing path no row declares: \"grep: src/typo.rs: No such file \
             or directory\"",
        ),
        (
            json!([accept("guard", "c", "")]),
            vec![row("1:0", "neg", 3, "")],
            "broken",
            "neg: a guard must pass today, and this exited 3",
        ),
        (
            json!([accept("other", "c", "")]),
            vec![row("1:0", "neg", 0, "")],
            "broken",
            "kind: `other` is not an accept kind",
        ),
        (
            forward(),
            vec![row("1:0", "neg", 0, ""), row("1:0", "pos", 0, "")],
            "vacuous",
            "neg: a forward check must fail before the work, and this exited 0",
        ),
        (
            forward(),
            vec![row("1:0", "neg", 1, ""), row("1:0", "pos", 4, "")],
            "unsatisfiable",
            "pos: the positive control must pass, and this exited 4",
        ),
        (
            json!([accept("falsifier", "c", "")]),
            vec![row("1:0", "neg", 5, "")],
            "permanently-green",
            "neg: a falsifier check must pass today, and this exited 5",
        ),
        (
            json!([accept("guard", "c", "")]),
            vec![row("1:0", "neg", 0, "")],
            "guard-alone",
            "neg: this guard passes, and no forward or falsifier row beside it discriminates",
        ),
    ];
    for (accepts, rows, verdict, reason) in cases {
        let got = reason_of(accepts, &rows);
        assert_eq!(got, (verdict.to_string(), reason.to_string()));
    }
}

#[test]
fn the_reason_stays_out_of_the_store() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &store(json!([accept("guard", "c", "")])));
    record(&root, &format!("head\t{SHA}\n{}", row("1:0", "neg", 3, ""))).success();
    let probe = probes(&path)[0].clone().expect("a recorded probe");
    assert_eq!(probe["verdict"].as_str(), Some("broken"));
    assert!(probe.get("reason").is_none(), "{probe}");
}

#[test]
fn a_capture_taken_before_a_command_was_edited_is_refused() {
    let (_dir, root) = sandbox();
    let before = store(json!([accept("forward", "false", "true")]));
    let path = seed_tasks(&root, &before);
    let capture = stamped(
        &root,
        &format!(
            "head\t{SHA}\n{}{}",
            row("1:0", "neg", 1, ""),
            row("1:0", "pos", 0, "")
        ),
    );

    seed_tasks(&root, &store(json!([accept("forward", "true", "true")])));
    assert_refused(
        &root,
        &path,
        &capture,
        "neg control for task 1's `accept[0]`",
    );
    assert_refused(&root, &path, &capture, "plan probes --emit");

    seed_tasks(&root, &store(json!([accept("forward", "false", "false")])));
    assert_refused(
        &root,
        &path,
        &capture,
        "pos control for task 1's `accept[0]`",
    );

    seed_tasks(&root, &before);
    record(&root, &capture).success();
    assert_eq!(verdict(&probes(&path)[0]), "healthy");
}

#[test]
fn a_capture_with_no_digest_is_refused() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &store(json!([accept("forward", "c", "")])));
    let head = format!("head\t{SHA}\n");
    for line in [
        "1:0\tneg\trc=1\t\t\n",
        "1:0\tneg\t\trc=1\t\t\n",
        "1:0\tneg\tsha256=\trc=1\t\t\n",
        "1:0\tneg\tc0ffee\trc=1\t\t\n",
    ] {
        assert_refused(&root, &path, &format!("{head}{line}"), "1:0");
        assert_refused(&root, &path, &format!("{head}{line}"), "no command digest");
    }
}

/// Task 1 holds a healthy probe, and task 2 is pending at a later milestone.
fn two_tasks(first_status: &str) -> String {
    let probed = json!({
        "behaviour": "it holds",
        "check": "already-true",
        "kind": "forward",
        "positive": "",
        "writes_build": false,
        "probe": { "at": SHA, "neg_rc": 1, "verdict": "healthy" },
    });
    let doc = json!({
        "schema_version": 2,
        "checkpoints": [
            { "id": "A", "rationale": "first" },
            { "id": "B", "rationale": "second" },
        ],
        "milestones": [
            { "id": "A", "gates": ["true"], "elaboration": "upfront", "defer_reason": "" },
            { "id": "B", "gates": ["true"], "elaboration": "upfront", "defer_reason": "" },
        ],
        "items": [
            {
                "id": 1,
                "ref": "first",
                "title": "First",
                "effort": "S",
                "status": first_status,
                "checkpoint": "A",
                "files": [FILE],
            },
            {
                "id": 2,
                "ref": "second",
                "title": "Second",
                "effort": "S",
                "checkpoint": "B",
                "files": [FILE],
                "needs": [1],
            },
        ],
        "specs": [
            { "ref": "first", "outcome": "it holds", "accept": [probed] },
            {
                "ref": "second",
                "outcome": "it holds",
                "accept": [accept("forward", "later-check", "")],
            },
        ],
    });
    toml::to_string(&doc).expect("the fixture serialises")
}

fn second_probe(store_path: &Path) -> Option<toml::Value> {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(store_path).unwrap()).unwrap();
    doc["specs"][1]["accept"][0].get("probe").cloned()
}

#[test]
fn re_probing_leaves_a_finished_rows_verdict_in_place_and_plan_check_clean() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &two_tasks("done"));
    let flow = path.parent().unwrap();
    fs::write(flow.join("plan.toml"), "schema_version = 1\n").unwrap();
    let authored = root.join("docs/flows").join(TASKS_SLUG);
    fs::create_dir_all(&authored).unwrap();
    fs::write(
        authored.join("brief.md"),
        "## Intent\nA.\n\n## Non-goals\nB.\n\n## Scope budget\nC.\n\n## Success criteria\nD.\n",
    )
    .unwrap();

    let script = emit(&root);
    assert!(
        script.contains("\n# skipped task 1: status is not pending\n"),
        "{script}"
    );
    assert!(!script.contains("already-true"), "{script}");
    assert!(script.contains("later-check"), "{script}");

    record(&root, &format!("head\t{SHA}\n{}", row("2:0", "neg", 1, ""))).success();
    let kept = probes(&path);
    let kept = kept[0].as_ref().expect("the stored probe stays");
    assert_eq!(kept["verdict"].as_str(), Some("healthy"));
    assert_eq!(kept["at"].as_str(), Some(SHA));
    assert_eq!(verdict(&second_probe(&path)), "healthy");

    let out = cli(&root)
        .args(["plan", "check", "--slug", TASKS_SLUG])
        .assert()
        .success();
    let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(report["findings"], json!([]), "{report}");
}

#[test]
fn a_capture_naming_a_row_no_longer_pending_is_refused() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &two_tasks("in-progress"));
    let capture = format!(
        "head\t{SHA}\n1:0\tneg\tsha256={}\trc=0\t\t\n{}",
        digest("already-true"),
        row("2:0", "neg", 1, "")
    );
    assert_refused(&root, &path, &capture, "1:0");
}

#[test]
fn a_milestone_restricts_both_modes_to_its_checkpoint() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &two_tasks("pending"));

    let out = cli(&root)
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--emit"])
        .args(["--milestone", "B"])
        .assert()
        .success();
    let script = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(script.contains("\np 2:0 neg "), "{script}");
    assert!(
        script.contains("\n# skipped task 1: not in the requested milestone\n"),
        "{script}"
    );
    assert!(!script.contains("already-true"), "{script}");

    let second = format!("head\t{SHA}\n{}", row("2:0", "neg", 1, ""));
    let before = fs::read_to_string(&path).unwrap();
    record(&root, &second).failure();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        before,
        "with no flag the capture is short of task 1"
    );
    record_with(&root, &second, &["--milestone", "B"]).success();
    assert_eq!(verdict(&second_probe(&path)), "healthy");

    let first = format!(
        "head\t{SHA}\n1:0\tneg\tsha256={}\trc=0\t\t\n",
        digest("already-true")
    );
    record_with(&root, &first, &["--milestone", "B"]).failure();
}

#[test]
fn an_unknown_milestone_is_refused_in_both_modes() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &two_tasks("pending"));
    let before = fs::read_to_string(&path).unwrap();

    let emit = cli(&root)
        .args(["--error-format", "json"])
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--emit"])
        .args(["--milestone", "Z"])
        .assert()
        .failure();
    let record = record_with(&root, &format!("head\t{SHA}\n"), &["--milestone", "Z"]).failure();
    for assert in [emit, record] {
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        let error = parse_json_error_envelope(&stderr);
        assert_eq!(error["kind"], "validation", "{stderr}");
        assert!(error["message"].as_str().unwrap().contains('Z'), "{stderr}");
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn a_dot_slash_declared_path_still_excuses_its_missing_file() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(
        &root,
        &store(json!([accept("forward", "c", "")])).replace(FILE, "./src\\\\declared.rs"),
    );
    record(
        &root,
        &format!(
            "head\t{SHA}\n{}",
            row(
                "1:0",
                "neg",
                2,
                "grep: src/declared.rs: No such file or directory"
            )
        ),
    )
    .success();
    assert_eq!(verdict(&probes(&path)[0]), "healthy");
}

#[test]
fn record_on_a_missing_store_creates_nothing() {
    let (_dir, root) = sandbox();
    let path = root
        .join(".claude")
        .join("flows")
        .join(TASKS_SLUG)
        .join("tasks.toml");
    record(&root, &format!("head\t{SHA}\n")).failure();
    assert!(!path.exists(), "a capture must not create a store");
}

#[test]
fn emit_runs_only_rows_explicitly_marked_and_names_the_rest_by_key() {
    let (_dir, root) = sandbox();
    seed_tasks(
        &root,
        &store(json!([
            accept("forward", "echo marked", "echo positive"),
            { "kind": "forward", "check": "echo unlabelled\ntouch pwned" },
            {
                "kind": "forward",
                "check": "echo labelled-true",
                "positive": "echo labelled-true-positive",
                "writes_build": true,
            },
            accept("forward", "", "echo empty-check-positive"),
            accept("forward", "   ", ""),
        ])),
    );
    let script = emit(&root);
    let marked = format!("\np 1:0 neg {} 'echo marked'\n", digest("echo marked"));
    assert!(script.contains(&marked), "{script}");
    let positive = format!("\np 1:0 pos {} 'echo positive'\n", digest("echo positive"));
    assert!(script.contains(&positive), "{script}");
    assert!(
        script.contains("\n# skipped 1:1: writes_build is not false\n"),
        "{script}"
    );
    assert!(
        script.contains("\n# skipped 1:2: writes_build is not false\n"),
        "{script}"
    );
    assert!(
        script.contains("\n# skipped 1:3: empty check\n"),
        "{script}"
    );
    assert!(
        script.contains("\n# skipped 1:4: empty check\n"),
        "{script}"
    );
    for absent in [
        "unlabelled",
        "pwned",
        "labelled-true",
        "empty-check-positive",
    ] {
        assert!(
            !script.contains(absent),
            "`{absent}` must not reach:\n{script}"
        );
    }
}

#[test]
fn emit_refuses_a_command_a_script_cannot_carry() {
    let (_dir, root) = sandbox();
    seed_tasks(
        &root,
        &store(json!([accept("forward", "echo a\u{0}b", "")])),
    );
    let assert = cli(&root)
        .args(["--error-format", "json"])
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--emit"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
}

/// A pending task carrying a `base` premise and an `after:` one, beside a task
/// of `first_status` whose `base` premise must stay out of the script.
fn premise_store_after(first_status: &str) -> String {
    let doc = json!({
        "schema_version": 2,
        "checkpoints": [
            { "id": "A", "rationale": "first" },
            { "id": "B", "rationale": "second" },
        ],
        "items": [
            {
                "id": 1,
                "ref": "done-row",
                "title": "Done",
                "effort": "S",
                "status": first_status,
                "checkpoint": "A",
            },
            {
                "id": 2,
                "ref": "the-row",
                "title": "The row",
                "effort": "S",
                "checkpoint": "B",
                "files": [FILE],
            },
        ],
        "specs": [
            {
                "ref": "done-row",
                "outcome": "it held",
                "accept": [accept("forward", "c", "")],
                "premises": [{ "claim": "old", "check": "echo finished", "at": "base" }],
            },
            {
                "ref": "the-row",
                "outcome": "it holds",
                "accept": [accept("forward", "echo accept-check", "")],
                "premises": [
                    { "claim": "it's there", "check": "test -f 'a b'", "at": "base" },
                    { "claim": "it lands", "check": "echo after-check", "at": "after:1" },
                    { "claim": "blank", "check": "  ", "at": "base" },
                ],
            },
        ],
    });
    toml::to_string(&doc).expect("the fixture serialises")
}

fn premise_store() -> String {
    premise_store_after("done")
}

fn emit_premises(root: &Path, flags: &[&str]) -> String {
    let out = cli(root)
        .args(["plan", "probes", "--slug", TASKS_SLUG])
        .args(["--emit", "--premises"])
        .args(flags)
        .assert()
        .success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

#[test]
fn emit_premises_prints_a_line_per_checkable_premise_of_a_pending_task() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, &premise_store());
    let script = emit_premises(&root, &[]);
    assert!(script.starts_with("#!/usr/bin/env bash\n"), "{script}");
    assert!(
        script.contains("cd \"$(git rev-parse --show-toplevel)\""),
        "{script}"
    );
    let check = "test -f 'a b'";
    let line = format!(
        "\np 2:0 premise {} 'test -f '\\''a b'\\'''\n",
        digest(check)
    );
    assert!(script.contains(&line), "{script}");
    let after = format!(
        "\np 2:1 premise {} 'echo after-check'\n",
        digest("echo after-check")
    );
    assert!(script.contains(&after), "{script}");
    assert!(
        script.contains("\n# skipped 2:2: empty check\n"),
        "{script}"
    );
    assert!(
        script.contains("\n# skipped task 1: status is not pending\n"),
        "{script}"
    );
    for absent in ["finished", "accept-check", " neg "] {
        assert!(
            !script.contains(absent),
            "`{absent}` must not reach:\n{script}"
        );
    }
}

#[test]
fn an_after_premise_waits_for_its_task_to_be_done() {
    for status in ["pending", "in-progress", "failed"] {
        let (_dir, root) = sandbox();
        seed_tasks(&root, &premise_store_after(status));
        let script = emit_premises(&root, &[]);
        assert!(
            script.contains("\n# skipped 2:1: task 1 is not done\n"),
            "{status}: {script}"
        );
        assert!(!script.contains("after-check"), "{status}: {script}");
        assert!(script.contains("\np 2:0 premise "), "{status}: {script}");
    }
}

#[test]
fn an_at_that_is_neither_base_nor_after_a_number_is_skipped_without_its_text() {
    let (_dir, root) = sandbox();
    let fixture = premise_store().replace("after:1", "after:+1-secret");
    assert!(fixture.contains("after:+1-secret"));
    seed_tasks(&root, &fixture);
    let script = emit_premises(&root, &[]);
    assert!(
        script.contains("\n# skipped 2:1: `at` is neither `base` nor `after:<number>`\n"),
        "{script}"
    );
    for absent in ["after-check", "secret"] {
        assert!(
            !script.contains(absent),
            "`{absent}` must not reach:\n{script}"
        );
    }
}

#[test]
fn a_task_list_restricts_both_modes_to_its_rows() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &two_tasks("pending"));

    let out = cli(&root)
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--emit"])
        .args(["--task", "2"])
        .assert()
        .success();
    let script = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(script.contains("\np 2:0 neg "), "{script}");
    assert!(
        script.contains("\n# skipped task 1: not a requested task\n"),
        "{script}"
    );
    assert!(!script.contains("already-true"), "{script}");

    let both = emit_premises(&root, &["--task", "1,2"]);
    assert!(!both.contains("not a requested task"), "{both}");

    let second = format!("head\t{SHA}\n{}", row("2:0", "neg", 1, ""));
    record_with(&root, &second, &["--task", "2"]).success();
    assert_eq!(verdict(&second_probe(&path)), "healthy");

    let first = format!(
        "head\t{SHA}\n1:0\tneg\tsha256={}\trc=0\t\t\n",
        digest("already-true")
    );
    record_with(&root, &first, &["--task", "2"]).failure();
}

#[test]
fn an_unknown_task_is_refused_in_both_modes() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &two_tasks("pending"));
    let before = fs::read_to_string(&path).unwrap();

    let emit = cli(&root)
        .args(["--error-format", "json"])
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--emit"])
        .args(["--task", "2,9"])
        .assert()
        .failure();
    let record = record_with(&root, &format!("head\t{SHA}\n"), &["--task", "9"]).failure();
    for assert in [emit, record] {
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        let error = parse_json_error_envelope(&stderr);
        assert_eq!(error["kind"], "validation", "{stderr}");
        assert!(
            error["message"].as_str().unwrap().contains("--task 9"),
            "{stderr}"
        );
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn premises_is_refused_with_neither_emit_nor_record() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &premise_store());
    let before = fs::read_to_string(&path).unwrap();
    let bare = cli(&root)
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--premises"])
        .assert()
        .code(2);
    assert!(
        bare.get_output().stdout.is_empty(),
        "{:?}",
        bare.get_output()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

const BASE_CHECK: &str = "test -f 'a b'";
const AFTER_CHECK: &str = "echo after-check";

/// A capture row for a premise, carrying the digest of `check`.
fn premise_row(key: &str, check: &str, rc: i64, stderr: &str) -> String {
    format!(
        "{key}\tpremise\tsha256={}\trc={rc}\t\t{stderr}\n",
        digest(check)
    )
}

/// The capture `premise_store`'s script yields, with the `base` premise's
/// result given and the `after:` one holding.
fn premise_capture(rc: i64, stderr: &str) -> String {
    format!(
        "head\t{SHA}\n{}{}",
        premise_row("2:0", BASE_CHECK, rc, stderr),
        premise_row("2:1", AFTER_CHECK, 0, "")
    )
}

fn record_premises(root: &Path, capture: &str, flags: &[&str]) -> assert_cmd::assert::Assert {
    let mut all = vec!["--premises"];
    all.extend_from_slice(flags);
    record_with(root, capture, &all)
}

fn premise_probes(store_path: &Path) -> Vec<Option<toml::Value>> {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(store_path).unwrap()).unwrap();
    doc["specs"][1]["premises"]
        .as_array()
        .unwrap()
        .iter()
        .map(|premise| premise.get("probe").cloned())
        .collect()
}

#[track_caller]
fn assert_premises_refused(root: &Path, store_path: &Path, capture: &str, expected: &str) {
    let before = fs::read_to_string(store_path).unwrap();
    let assert = record_premises(root, capture, &[]).failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    assert!(
        error["message"].as_str().unwrap().contains(expected),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(store_path).unwrap(),
        before,
        "a refused capture must write nothing"
    );
}

#[test]
fn recording_premises_stores_holds_fails_or_broken_from_the_capture() {
    let cases = [
        (0, "", "holds"),
        (1, "", "fails"),
        (2, "grep: missing.rs: No such file or directory", "fails"),
        (0, "bash: nope: command not found", "broken"),
        (1, "fatal: not a git repository", "broken"),
    ];
    for (rc, stderr, expected) in cases {
        let (_dir, root) = sandbox();
        let path = seed_tasks(&root, &premise_store());
        let out = record_premises(&root, &premise_capture(rc, stderr), &[]).success();
        let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
        assert_eq!(
            report,
            json!({
                "ok": true,
                "recorded": [
                    { "task": 2, "premise": 0, "verdict": expected },
                    { "task": 2, "premise": 1, "verdict": "holds" },
                ],
            }),
            "rc {rc}, stderr {stderr}"
        );
        let probes = premise_probes(&path);
        let first = probes[0].as_ref().expect("a recorded premise probe");
        assert_eq!(first["verdict"].as_str(), Some(expected));
        assert_eq!(first["at"].as_str(), Some(SHA));
        assert_eq!(first["neg_rc"].as_integer(), Some(rc));
        assert!(first.get("pos_rc").is_none(), "{first}");
        assert_eq!(verdict(&probes[1]), "holds");
        assert_eq!(probes[2], None, "an unchecked premise gets no probe");
    }
}

#[test]
fn a_premise_capture_taken_before_its_check_was_edited_is_refused() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &premise_store());
    let capture = format!(
        "head\t{SHA}\n{}{}",
        premise_row("2:0", "test -f 'old'", 0, ""),
        premise_row("2:1", AFTER_CHECK, 0, "")
    );
    assert_premises_refused(&root, &path, &capture, "`premises[0]`");
    assert_premises_refused(&root, &path, &capture, "different command");
}

#[test]
fn an_accept_capture_passed_with_premises_is_refused() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &premise_store());
    let capture = format!(
        "head\t{SHA}\n2:0\tneg\tsha256={}\trc=1\t\t\n",
        digest("echo accept-check")
    );
    assert_premises_refused(&root, &path, &capture, "is an `accept` row");
}

#[test]
fn a_premise_capture_passed_without_premises_is_refused() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &premise_store());
    assert_refused(&root, &path, &premise_capture(0, ""), "is a premise row");
}

#[test]
fn a_short_or_overlong_premise_capture_is_refused_whole() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &premise_store());
    let head = format!("head\t{SHA}\n");
    let base = premise_row("2:0", BASE_CHECK, 0, "");
    let after = premise_row("2:1", AFTER_CHECK, 0, "");
    assert_premises_refused(&root, &path, &format!("{head}{base}"), "1 of the 2");
    assert_premises_refused(
        &root,
        &path,
        &format!("{head}{base}{after}{}", premise_row("2:2", "  ", 0, "")),
        "2:2",
    );
    assert_premises_refused(
        &root,
        &path,
        &format!("{head}{base}{after}{base}"),
        "repeats",
    );
}

#[test]
fn recording_premises_honours_the_milestone_and_task_scope() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &premise_store());
    let capture = premise_capture(0, "");
    for flags in [["--milestone", "A"], ["--task", "1"]] {
        let before = fs::read_to_string(&path).unwrap();
        record_premises(&root, &capture, &flags).failure();
        assert_eq!(fs::read_to_string(&path).unwrap(), before, "{flags:?}");
    }
    record_premises(&root, &capture, &["--milestone", "B"]).success();
    record_premises(&root, &premise_capture(1, ""), &["--task", "2"]).success();
    assert_eq!(verdict(&premise_probes(&path)[0]), "fails");
}

#[test]
fn a_re_issued_premise_keeps_its_verdict_and_an_edited_one_drops_it() {
    let (_dir, root) = sandbox();
    let path = seed_tasks(&root, &premise_store());
    record_premises(&root, &premise_capture(0, ""), &[]).success();
    let update = |premises: Value| {
        let row = json!({ "id": 2, "spec": { "premises": premises } });
        cli(&root)
            .args(["tasks", "update", "--slug", TASKS_SLUG, "--ndjson", "-"])
            .write_stdin(format!("{row}\n"))
            .assert()
            .success();
    };
    let base = json!({ "claim": "it's there", "check": BASE_CHECK, "at": "base" });
    let after = json!({ "claim": "it lands", "check": AFTER_CHECK, "at": "after:1" });
    let blank = json!({ "claim": "blank", "check": "  ", "at": "base" });

    let added = json!({
        "title": "Third",
        "effort": "S",
        "spec": {
            "outcome": "it holds",
            "premises": [{ "claim": "c", "check": "true", "at": "after:2" }],
        },
    });
    cli(&root)
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(format!("{added}\n"))
        .assert()
        .success();
    assert_eq!(verdict(&premise_probes(&path)[0]), "holds");

    update(json!([base, after, blank]));
    let probes = premise_probes(&path);
    assert_eq!(verdict(&probes[0]), "holds");
    assert_eq!(verdict(&probes[1]), "holds");

    let edited = json!({ "claim": "it lands later", "check": AFTER_CHECK, "at": "after:1" });
    update(json!([base, edited, blank]));
    let probes = premise_probes(&path);
    assert_eq!(verdict(&probes[0]), "holds");
    assert_eq!(probes[1], None, "an edited premise keeps no verdict");
}

#[test]
fn a_supplied_premise_probe_is_refused() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, &premise_store());
    let row = json!({
        "id": 2,
        "spec": { "premises": [{
            "claim": "c",
            "check": "true",
            "at": "base",
            "probe": { "at": SHA, "neg_rc": 0, "verdict": "holds" },
        }] },
    });
    let assert = cli(&root)
        .args(["--error-format", "json"])
        .args(["tasks", "update", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(format!("{row}\n"))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("only `plan probes --record --premises` writes it"),
        "{stderr}"
    );
}

#[test]
fn emit_on_a_missing_store_is_not_found() {
    let (_dir, root) = sandbox();
    let assert = cli(&root)
        .args(["--error-format", "json"])
        .args(["plan", "probes", "--slug", TASKS_SLUG, "--emit"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert_eq!(
        parse_json_error_envelope(&stderr)["kind"],
        "not_found",
        "{stderr}"
    );
}

/// A committed sandbox, its Git Bash, and the head commit; `None` when this
/// machine has no bash to run a script with.
fn repo(root: &Path) -> Option<(PathBuf, String)> {
    let Some(bash) = git_bash() else {
        eprintln!("skipping: no bash found");
        return None;
    };
    let commit = git_command(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false"])
        .args(["commit", "--allow-empty", "-q", "-m", "fixture"])
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let head = git_command(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    Some((bash, head))
}

/// Run the emitted script from `cwd` and return its stdout, the capture.
fn run_script(bash: &Path, root: &Path, cwd: &Path) -> String {
    fs::write(root.join("probes.sh"), emit(root)).unwrap();
    let script = root.join("probes.sh").to_string_lossy().replace('\\', "/");
    let script = script.trim_start_matches("//?/").to_string();
    let out = std::process::Command::new(bash)
        .arg(&script)
        .current_dir(cwd)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "the script must not stop on a failing control: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn stored_commands_reach_bash_exactly_as_stored() {
    let (_dir, root) = sandbox();
    let Some((bash, head)) = repo(&root) else {
        return;
    };
    let path = seed_tasks(
        &root,
        &store(json!([
            // Each exits 0 only if Bash saw the text below unchanged. The calling
            // script expanding anything, or a quote closing early, changes what
            // it compares.
            accept("falsifier", r#"test "it's" = 'it'\''s'"#, ""),
            accept("falsifier", r#"test '$HOME' = "\$HOME""#, ""),
            accept(
                "falsifier",
                r#"test "`echo a`" = a && test '`echo a`' != a"#,
                ""
            ),
            accept("falsifier", r#"test 'a\b' = "a\\b""#, ""),
            accept("falsifier", "false\ntrue", ""),
            // Run by the helper's `eval`, this is one word and no command of that
            // name exists. A quote that closed would run the `touch`.
            accept("forward", "true' ; touch escaped ; '", ""),
            accept("forward", "false", "true' ; touch escaped-positive ; '"),
            accept("forward", "echo 'unterminated", ""),
        ])),
    );
    let capture = run_script(&bash, &root, &root);
    assert_eq!(
        capture.lines().next(),
        Some(&*format!("head\t{head}")),
        "{capture}"
    );
    record(&root, &capture).success();

    assert!(!root.join("escaped").exists(), "a quote broke out in check");
    assert!(
        !root.join("escaped-positive").exists(),
        "a quote broke out in positive"
    );
    let probes = probes(&path);
    let verdicts: Vec<&str> = probes.iter().map(verdict).collect();
    assert_eq!(
        verdicts,
        [
            "healthy", "healthy", "healthy", "healthy", "healthy", "broken", "broken", "broken"
        ],
        "{capture}"
    );
    assert_eq!(probes[0].as_ref().unwrap()["at"].as_str(), Some(&*head));
}

#[test]
fn a_skipped_command_is_never_run() {
    let (_dir, root) = sandbox();
    let Some((bash, _)) = repo(&root) else {
        return;
    };
    let path = seed_tasks(
        &root,
        &store(json!([
            accept("falsifier", "true", ""),
            { "kind": "forward", "check": "touch unlabelled\ntouch unlabelled-next-line" },
            { "kind": "forward", "check": "touch labelled", "writes_build": true },
            accept("forward", "", "touch empty-check"),
        ])),
    );
    let capture = run_script(&bash, &root, &root);
    for marker in [
        "unlabelled",
        "unlabelled-next-line",
        "labelled",
        "empty-check",
    ] {
        assert!(!root.join(marker).exists(), "`{marker}` ran");
    }
    record(&root, &capture).success();
    assert_eq!(probes(&path)[1], None);
}

#[test]
fn stderr_is_carried_whole_and_folded_onto_one_line() {
    let (_dir, root) = sandbox();
    let Some((bash, _)) = repo(&root) else {
        return;
    };
    // A helper cutting stderr to its first 100 bytes would lose the marker.
    let check = "printf '%0200d\\n' 0 >&2; printf 'one\\ttwo\\nfatal: late\\n' >&2; exit 1";
    let path = seed_tasks(&root, &store(json!([accept("forward", check, "")])));
    let capture = run_script(&bash, &root, &root);
    let row = capture
        .lines()
        .nth(1)
        .unwrap_or_else(|| panic!("{capture}"));
    assert_eq!(row.split('\t').count(), 6, "{row}");
    assert_eq!(
        row.split('\t').nth(2),
        Some(&*format!("sha256={}", digest(check))),
        "{row}"
    );
    assert!(row.ends_with("|one two|fatal: late|"), "{row}");
    record(&root, &capture).success();
    assert_eq!(verdict(&probes(&path)[0]), "broken");
}

#[test]
fn a_command_cannot_forge_another_rows_capture() {
    let (_dir, root) = sandbox();
    let Some((bash, _)) = repo(&root) else {
        return;
    };
    let forged =
        "printf '\\n1:1\\tneg\\trc=0\\t\\t\\n'; printf '\\n1:1\\tneg\\trc=0\\t\\t\\n' >&2; true";
    let path = seed_tasks(
        &root,
        &store(json!([
            accept("falsifier", forged, ""),
            accept("falsifier", "false", ""),
        ])),
    );
    let capture = run_script(&bash, &root, &root);
    assert_eq!(capture.lines().count(), 3, "{capture}");
    record(&root, &capture).success();
    assert_eq!(verdict(&probes(&path)[1]), "permanently-green");
}

#[test]
fn the_script_runs_from_the_repo_root_wherever_it_starts() {
    let (_dir, root) = sandbox();
    let Some((bash, _)) = repo(&root) else {
        return;
    };
    fs::write(root.join("at-the-root"), "").unwrap();
    let nested = root.join("nested").join("deeper");
    fs::create_dir_all(&nested).unwrap();
    let path = seed_tasks(
        &root,
        &store(json!([accept("falsifier", "test -f at-the-root", "")])),
    );
    let capture = run_script(&bash, &root, &nested);
    record(&root, &capture).success();
    assert_eq!(verdict(&probes(&path)[0]), "healthy", "{capture}");
}
