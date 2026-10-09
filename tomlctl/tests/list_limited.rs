//! The `limited` report on the three list verbs: a `--limit` that cuts rows
//! wraps pretty row output as `{"rows":[…],"limited":{…}}` and leads `--lines`
//! row output with a header line, while the `--ndjson`, `--pluck` and `--raw`
//! streams stay bare and say so on stderr. A limit that cuts nothing changes
//! nothing.

use serde_json::{Value, json};
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, sandbox, seed_ledger_in, seed_tasks, store_path};

const LEDGER: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
summary = "first"

[[items]]
id = "R2"
status = "open"
summary = "second"

[[items]]
id = "R3"
status = "open"
summary = "third"
"#;

const BACKLOG: &str = r#"schema_version = 1
last_updated = 2026-09-01

[[backlog]]
id = "B-00000001"
kind = "bug"
summary = "first"
area = "src/a.rs"
tags = []
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "1111111111111111"

[[backlog]]
id = "B-00000002"
kind = "bug"
summary = "second"
area = "src/b.rs"
tags = []
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "2222222222222222"

[[backlog]]
id = "B-00000003"
kind = "bug"
summary = "third"
area = "src/c.rs"
tags = []
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "3333333333333333"
"#;

const TASKS: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["first", "second", "third"]

[policy]
checkpoints = "single"
max_parallel = 2
commit_granularity = "per-task"
note = ""

[[items]]
id = 1
ref = "first"
title = "First"
effort = "S"
status = "pending"
files = ["src/a.rs"]
needs = []
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
files = ["src/b.rs"]
needs = []
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
files = ["src/c.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

/// Rows in every fixture.
const TOTAL: usize = 3;

/// One list verb over its three-row fixture: the argv that lists it, and a
/// string field to pluck.
struct Verb {
    root: tempfile::TempDir,
    argv: Vec<String>,
    pluck: &'static str,
}

fn verbs() -> Vec<Verb> {
    let (items_dir, items_root) = sandbox();
    let ledger = seed_ledger_in(&items_root, "ledger.toml", LEDGER);
    let (tasks_dir, tasks_root) = sandbox();
    seed_tasks(&tasks_root, TASKS);
    let (backlog_dir, backlog_root) = sandbox();
    std::fs::write(store_path(&backlog_root), BACKLOG).unwrap();
    let owned = |args: &[&str]| args.iter().map(|s| s.to_string()).collect();
    vec![
        Verb {
            root: items_dir,
            argv: owned(&["items", "list", ledger.to_str().unwrap()]),
            pluck: "id",
        },
        Verb {
            root: tasks_dir,
            argv: owned(&["tasks", "list", "--slug", TASKS_SLUG]),
            pluck: "ref",
        },
        Verb {
            root: backlog_dir,
            argv: owned(&["backlog", "list"]),
            pluck: "id",
        },
    ]
}

struct Run {
    stdout: String,
    stderr: String,
}

impl Verb {
    fn root(&self) -> &Path {
        self.root.path()
    }

    fn label(&self) -> String {
        self.argv[..2].join(" ")
    }

    fn run(&self, extra: &[&str]) -> Run {
        let out = cli(&self.root().canonicalize().unwrap())
            .args(&self.argv)
            .args(extra)
            .write_stdin("")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(
            out.status.success(),
            "`{} {}` failed: {stderr}",
            self.label(),
            extra.join(" ")
        );
        Run { stdout, stderr }
    }
}

fn json_lines(s: &str) -> Vec<Value> {
    s.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

fn header(shown: usize) -> Value {
    json!({ "shown": shown, "total": TOTAL })
}

fn notice(shown: usize) -> String {
    format!("tomlctl: showing {shown} of {TOTAL} rows")
}

#[test]
fn pretty_rows_cut_by_limit_are_wrapped_with_the_header() {
    for verb in verbs() {
        let r = verb.run(&["--limit", "2"]);
        let v: Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(v["limited"], header(2), "{}: {}", verb.label(), r.stdout);
        assert_eq!(
            v["rows"].as_array().map(Vec::len),
            Some(2),
            "{}",
            verb.label()
        );
        assert!(r.stderr.is_empty(), "{}: {}", verb.label(), r.stderr);
    }
}

#[test]
fn a_limit_that_cuts_nothing_keeps_the_bare_array() {
    for verb in verbs() {
        for limit in ["3", "5"] {
            let r = verb.run(&["--limit", limit]);
            let v: Value = serde_json::from_str(&r.stdout).unwrap();
            assert_eq!(
                v.as_array().map(Vec::len),
                Some(TOTAL),
                "{} --limit {limit}: {}",
                verb.label(),
                r.stdout
            );
            let lines = json_lines(&verb.run(&["--limit", limit, "--lines"]).stdout);
            assert_eq!(
                lines.len(),
                TOTAL,
                "{} --limit {limit} --lines",
                verb.label()
            );
            assert!(
                lines.iter().all(|l| l.get("limited").is_none()),
                "{lines:?}"
            );
        }
    }
}

#[test]
fn lines_lead_with_the_header_line() {
    for verb in verbs() {
        // Streamed, and buffered because `--max-chars` needs the whole set.
        for extra in [&[][..], &["--max-chars", "100"][..]] {
            let args = [&["--limit", "1", "--lines"][..], extra].concat();
            let r = verb.run(&args);
            let lines = json_lines(&r.stdout);
            assert_eq!(lines.len(), 2, "{} {args:?}: {}", verb.label(), r.stdout);
            assert_eq!(
                lines[0],
                json!({ "limited": header(1) }),
                "{}",
                verb.label()
            );
            assert!(lines[1].get("limited").is_none(), "{}", verb.label());
            assert!(r.stderr.is_empty(), "{}: {}", verb.label(), r.stderr);
        }
    }
}

#[test]
fn ndjson_stays_header_free_and_reports_on_stderr() {
    for verb in verbs() {
        for extra in [&[][..], &["--max-chars", "100"][..], &["--lines"][..]] {
            let args = [&["--limit", "1", "--ndjson"][..], extra].concat();
            let r = verb.run(&args);
            let lines = json_lines(&r.stdout);
            assert_eq!(lines.len(), 1, "{} {args:?}: {}", verb.label(), r.stdout);
            assert!(lines[0].get("limited").is_none(), "{}", r.stdout);
            assert!(
                r.stderr.contains(&notice(1)),
                "{} {args:?}: {}",
                verb.label(),
                r.stderr
            );
        }
    }
}

#[test]
fn plucked_raw_lines_stay_header_free() {
    for verb in verbs() {
        let r = verb.run(&["--pluck", verb.pluck, "--raw", "--lines", "--limit", "2"]);
        let lines: Vec<&str> = r.stdout.lines().collect();
        assert_eq!(lines.len(), 2, "{}: {}", verb.label(), r.stdout);
        assert!(lines.iter().all(|l| !l.contains("limited")), "{}", r.stdout);
        assert!(
            r.stderr.contains(&notice(2)),
            "{}: {}",
            verb.label(),
            r.stderr
        );
    }
}

#[test]
fn count_under_limit_is_header_free() {
    for verb in verbs() {
        let r = verb.run(&["--count", "--limit", "1"]);
        let v: Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(v, json!({ "count": 1 }), "{}: {}", verb.label(), r.stdout);
    }
}

#[test]
fn json_errors_withhold_the_stderr_notice() {
    for verb in verbs() {
        let r = verb.run(&["--error-format", "json", "--limit", "1", "--ndjson"]);
        assert_eq!(json_lines(&r.stdout).len(), 1, "{}", verb.label());
        assert!(r.stderr.is_empty(), "{}: {}", verb.label(), r.stderr);
    }
}
