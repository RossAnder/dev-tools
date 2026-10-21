//! The owned-store guard through the CLI: every generic writer refuses the
//! verb-owned stores unless `--raw-store` is passed, and leaves other stores
//! and all reads alone.

mod common;

use std::fs;
use std::path::Path;

use common::cli;

const SEED: &str =
    "schema_version = 1\n\n[[items]]\nid = \"A1\"\nsummary = \"first\"\nstatus = \"open\"\n";

/// One generic-writer invocation against `file`: the argument list and any
/// stdin the verb reads.
struct Case {
    name: &'static str,
    args: Vec<String>,
    stdin: &'static str,
}

fn cases(file: &Path) -> Vec<Case> {
    let f = file.display().to_string();
    let with = |parts: &[&str]| -> Vec<String> {
        let mut args = Vec::new();
        for part in parts {
            args.push(if *part == "FILE" {
                f.clone()
            } else {
                (*part).to_string()
            });
        }
        args
    };
    vec![
        Case {
            name: "set",
            args: with(&["set", "FILE", "marker", "1"]),
            stdin: "",
        },
        Case {
            name: "set-json",
            args: with(&["set-json", "FILE", "marker", "--json", r#"{"a":1}"#]),
            stdin: "",
        },
        Case {
            name: "array-append",
            args: with(&["array-append", "FILE", "events", "--json", r#"{"k":1}"#]),
            stdin: "",
        },
        Case {
            name: "items add",
            args: with(&[
                "items",
                "add",
                "FILE",
                "--json",
                r#"{"id":"N1","summary":"new","status":"open"}"#,
            ]),
            stdin: "",
        },
        Case {
            name: "items add-many",
            args: with(&["items", "add-many", "FILE", "--ndjson", "-"]),
            stdin: "{\"id\":\"N2\",\"summary\":\"many\",\"status\":\"open\"}\n",
        },
        Case {
            name: "items update",
            args: with(&[
                "items",
                "update",
                "FILE",
                "A1",
                "--json",
                r#"{"summary":"changed"}"#,
            ]),
            stdin: "",
        },
        Case {
            name: "items remove",
            args: with(&["items", "remove", "FILE", "A1"]),
            stdin: "",
        },
        Case {
            name: "items apply",
            args: with(&[
                "items",
                "apply",
                "FILE",
                "--ops",
                r#"[{"op":"update","id":"A1","json":{"summary":"applied"}}]"#,
            ]),
            stdin: "",
        },
        Case {
            name: "items backfill-dedup-id",
            args: with(&["items", "backfill-dedup-id", "FILE"]),
            stdin: "",
        },
    ]
}

fn seed(root: &Path, rel: &str) -> std::path::PathBuf {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, SEED).unwrap();
    path
}

/// Owned stores paired with a verb the refusal must name.
const OWNED: [(&str, &str); 2] = [
    (".claude/backlog.toml", "backlog edit"),
    (".claude/flows/s/tasks.toml", "tasks update"),
];

#[test]
fn generic_writers_refuse_owned_stores() {
    let (_dir, root) = common::sandbox();
    for (rel, verb) in OWNED {
        let path = seed(&root, rel);
        for case in cases(&path) {
            let before = fs::read(&path).unwrap();
            let out = cli(&root)
                .args(&case.args)
                .write_stdin(case.stdin)
                .assert()
                .failure()
                .code(1);
            let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
            assert!(
                stderr.contains(verb) && stderr.contains("--raw-store"),
                "{} on {rel}: stderr should name `{verb}` and --raw-store; got: {stderr}",
                case.name
            );
            assert_eq!(
                fs::read(&path).unwrap(),
                before,
                "{} on {rel} must leave the store untouched",
                case.name
            );
        }
    }
}

#[test]
fn raw_store_lets_generic_writers_through() {
    let (_dir, root) = common::sandbox();
    for (rel, _) in OWNED {
        let path = seed(&root, rel);
        for case in cases(&path) {
            fs::write(&path, SEED).unwrap();
            cli(&root)
                .args(&case.args)
                .arg("--raw-store")
                .write_stdin(case.stdin)
                .assert()
                .success();
        }
    }
}

#[test]
fn generic_writers_allow_an_unowned_store() {
    let (_dir, root) = common::sandbox();
    let path = seed(&root, ".claude/flows/s/review-ledger.toml");
    for case in cases(&path) {
        fs::write(&path, SEED).unwrap();
        cli(&root)
            .args(&case.args)
            .write_stdin(case.stdin)
            .assert()
            .success();
    }
}

#[test]
fn reads_of_owned_stores_are_unguarded() {
    let (_dir, root) = common::sandbox();
    for (rel, _) in OWNED {
        let path = seed(&root, rel);
        cli(&root)
            .args(["items", "get"])
            .arg(&path)
            .arg("A1")
            .assert()
            .success();
        cli(&root)
            .args(["items", "list"])
            .arg(&path)
            .assert()
            .success();
    }
}
