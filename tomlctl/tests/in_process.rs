//! `tomlctl::run_with` against the built binary: each command runs both ways
//! and must give the same exit code, stdout and stderr, and the same files.

use assert_cmd::Command;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::common;
use common::{TASKS_SLUG, sandbox, seed_ledger_in, seed_tasks};

type Env = [(OsString, Option<OsString>)];

const LEDGER: &str = r#"schema_version = 1

[[items]]
id = "R1"
summary = "seed"
status = "open"

[[items]]
id = "R2"
summary = "second"
status = "fixed"
"#;

/// Three rows needing each other in a loop, so `tasks check` reports a
/// `dag/cycle` error and exits 1.
const CYCLIC_TASKS: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["first", "second", "third"]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "one group"

[[items]]
id = 1
ref = "first"
title = "First"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["a.rs"]
needs = [3]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 2
ref = "second"
title = "Second"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["b.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 3
ref = "third"
title = "Third"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["c.rs"]
needs = [2]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn set(name: &str, value: &str) -> (OsString, Option<OsString>) {
    (name.into(), Some(value.into()))
}

/// The variables every run here carries, ahead of the test's own.
fn base_env(root: &Path, env: &Env) -> Vec<(OsString, Option<OsString>)> {
    let mut all = vec![
        (
            OsString::from("TOMLCTL_ROOT"),
            Some(root.as_os_str().to_owned()),
        ),
        set("TOMLCTL_LOCK_TIMEOUT", "5"),
    ];
    all.extend(env.iter().cloned());
    all
}

fn binary(root: &Path, args: &[OsString], env: &Env, stdin: &[u8]) -> tomlctl::Outcome {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    for (name, value) in base_env(root, env) {
        match value {
            Some(value) => cmd.env(name, value),
            None => cmd.env_remove(name),
        };
    }
    let output = cmd.args(args).write_stdin(stdin.to_vec()).output().unwrap();
    tomlctl::Outcome {
        code: output.status.code().expect("the binary exits with a code"),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn in_process(root: &Path, args: &[OsString], env: &Env, stdin: &[u8]) -> tomlctl::Outcome {
    let argv =
        std::iter::once(OsString::from(env!("CARGO_BIN_EXE_tomlctl"))).chain(args.iter().cloned());
    tomlctl::run_with(argv, &base_env(root, env), stdin)
}

fn snapshot(files: &[PathBuf]) -> Vec<Option<Vec<u8>>> {
    files.iter().map(|f| fs::read(f).ok()).collect()
}

fn restore(files: &[PathBuf], saved: &[Option<Vec<u8>>]) {
    for (file, bytes) in files.iter().zip(saved) {
        match bytes {
            Some(bytes) => fs::write(file, bytes).unwrap(),
            None => {
                let _ = fs::remove_file(file);
            }
        }
    }
}

fn assert_same(what: &str, binary: &tomlctl::Outcome, in_process: &tomlctl::Outcome) {
    let show = |o: &tomlctl::Outcome| {
        format!(
            "code {}\n--- stdout\n{}\n--- stderr\n{}",
            o.code,
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    };
    assert!(
        binary == in_process,
        "{what}: the in-process run differs from the binary\n=== binary\n{}\n=== in-process\n{}",
        show(binary),
        show(in_process)
    );
}

/// Run `args` through the binary, put `watched` back as it was, run it in
/// process, and require the same outcome and the same files after both.
fn twin(
    root: &Path,
    watched: &[PathBuf],
    args: &[&str],
    env: &Env,
    stdin: &[u8],
) -> tomlctl::Outcome {
    let args = os(args);
    let before = snapshot(watched);
    let by_binary = binary(root, &args, env, stdin);
    let after_binary = snapshot(watched);
    restore(watched, &before);
    let by_run_with = in_process(root, &args, env, stdin);
    assert_same(&format!("{args:?}"), &by_binary, &by_run_with);
    assert_eq!(
        snapshot(watched),
        after_binary,
        "{args:?}: the in-process run left different files"
    );
    by_run_with
}

fn ledger(root: &Path) -> (PathBuf, Vec<PathBuf>) {
    let path = seed_ledger_in(root, "ledger.toml", LEDGER);
    let sidecar = PathBuf::from(format!("{}.sha256", path.display()));
    (path.clone(), vec![path, sidecar])
}

#[test]
fn a_read_verb_matches_the_binary() {
    let (_tmp, root) = sandbox();
    let (path, watched) = ledger(&root);
    let path = path.to_str().unwrap();
    let out = twin(&root, &watched, &["items", "list", path], &[], b"");
    assert_eq!(out.code, 0);
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"R2\""));
}

#[test]
fn a_failing_read_matches_the_binary() {
    let (_tmp, root) = sandbox();
    let missing = root.join(".claude").join("missing.toml");
    let out = twin(&root, &[], &["parse", missing.to_str().unwrap()], &[], b"");
    assert_eq!(out.code, 1);
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("tomlctl: "));
}

#[test]
fn help_goes_to_stdout_with_code_zero() {
    let (_tmp, root) = sandbox();
    let out = twin(&root, &[], &["--help"], &[], b"");
    assert_eq!(out.code, 0);
    assert!(!out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn an_unknown_flag_goes_to_stderr_with_code_two() {
    let (_tmp, root) = sandbox();
    let out = twin(&root, &[], &["--no-such-flag"], &[], b"");
    assert_eq!(out.code, 2);
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn a_verb_exit_code_comes_through() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, CYCLIC_TASKS);
    let out = twin(
        &root,
        &[store],
        &["tasks", "check", "--slug", TASKS_SLUG],
        &[],
        b"",
    );
    assert_eq!(out.code, 1);
    assert!(String::from_utf8_lossy(&out.stdout).contains("dag/cycle"));
}

/// Each run gets its own stdin bytes and its own claim on them.
#[test]
fn stdin_is_read_through_dash_once_per_run() {
    let (_tmp, root) = sandbox();
    let (path, watched) = ledger(&root);
    let path = path.to_str().unwrap();
    for id in ["R3", "R4"] {
        let row = format!("{{\"id\":\"{id}\",\"summary\":\"from stdin\"}}\n");
        let out = twin(
            &root,
            &watched,
            &["items", "add-many", path, "--ndjson", "-"],
            &[],
            row.as_bytes(),
        );
        assert_eq!(out.code, 0);
    }
    let written = fs::read_to_string(&watched[0]).unwrap();
    assert!(written.contains("id = \"R3\"") && written.contains("id = \"R4\""));
}

/// One run's output options must not carry into the next run in the process.
#[test]
fn output_options_do_not_leak_between_runs() {
    let (_tmp, root) = sandbox();
    let (path, watched) = ledger(&root);
    let path = path.to_str().unwrap();
    let limited = twin(
        &root,
        &watched,
        &["items", "list", path, "--limit", "1"],
        &[],
        b"",
    );
    assert!(!String::from_utf8_lossy(&limited.stdout).contains("\"R2\""));
    let full = twin(
        &root,
        &watched,
        &["items", "list", path, "--select", "id"],
        &[],
        b"",
    );
    let full = String::from_utf8_lossy(&full.stdout);
    assert!(
        full.contains("\"R2\"") && !full.contains("limited"),
        "{full}"
    );
}

/// An overlay variable holds for its own run only.
#[test]
fn an_environment_overlay_lasts_one_run() {
    let (_tmp, root) = sandbox();
    let (path, watched) = ledger(&root);
    let path = path.to_str().unwrap();
    let add = |id: &str, env: &Env| {
        let row = format!(
            r#"{{"id":"{id}","file":"src/a.rs","summary":"{id}","severity":"warning","category":"quality"}}"#
        );
        let out = twin(
            &root,
            &watched,
            &["items", "add", path, "--json", &row],
            env,
            b"",
        );
        assert_eq!(out.code, 0);
    };
    add("R3", &[set("TOMLCTL_NO_DEDUP_ID", "1")]);
    add("R4", &[]);
    let written = fs::read_to_string(&watched[0]).unwrap();
    let r3 = &written[written.find("id = \"R3\"").unwrap()..written.find("id = \"R4\"").unwrap()];
    let r4 = &written[written.find("id = \"R4\"").unwrap()..];
    assert!(!r3.contains("dedup_id"), "{written}");
    assert!(r4.contains("dedup_id"), "{written}");
}
