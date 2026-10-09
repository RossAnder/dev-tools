//! The global output options across every leaf command `tomlctl capabilities`
//! lists. Each command has at least one case here, and the leaf set and the
//! case set must agree, so a new command cannot ship without one. Every case
//! runs plain, under `-q` (nothing on stdout, same exit status) and under
//! `--lines` (one JSON value per line, in the case's declared shape); each JSON
//! case also runs `--select` and `--get` on a key its fixture guarantees. A
//! text case must refuse `--lines` instead.
//!
//! Reads share one fixture tree; each write invocation gets its own copy.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;
use common::{
    TASKS_SLUG, cli, git_available, git_command, sandbox, seed_ledger_in, write_tasks_store,
};

const LEDGER: &str = ".claude/review-ledger.toml";
const JSON_DOC: &str = ".claude/data.json";

/// `R1` and `R2` are tier-A duplicates sharing `src/a.rs`; `R3` depends on
/// `R1` and sweeps for `gamma`; `R4` names a file that does not exist, so
/// `items orphans` has a row.
const LEDGER_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-01-01

[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
symbol = "alpha"
summary = "alpha leaks"
severity = "warning"
category = "quality"

[[items]]
id = "R2"
status = "open"
file = "src/a.rs"
symbol = "alpha"
summary = "alpha leaks"
severity = "warning"
category = "quality"

[[items]]
id = "R3"
status = "open"
file = "src/b.rs"
depends_on = ["R1"]
sweep = ["fn gamma"]

[[items]]
id = "R4"
status = "open"
file = "src/gone.rs"
"#;

const TASKS_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["first", "second"]

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
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

const BLOCK_DOC: &str =
    "# doc\n\n<!-- SHARED-BLOCK:demo START -->\nshared\n<!-- SHARED-BLOCK:demo END -->\n";

/// Where a command's rows sit, mirroring the emitter it goes through.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// One value: `--lines` prints it as one compact line.
    One,
    /// Rows are this field of the report object.
    Field(&'static str),
    /// The report is the row array.
    Top,
    /// Not JSON: refuses `--lines`.
    Text,
}

struct Case {
    argv: &'static [&'static str],
    stdin: &'static str,
    shape: Shape,
    /// A field every row (or the single value) carries in the fixture.
    key: &'static str,
    write: bool,
    /// Runs on its own copy of the fixture; set for every write.
    isolated: bool,
    git: bool,
    /// The stderr text of the command's refusal of `--lines`, for a command
    /// that refuses it.
    refuses_lines: Option<&'static str>,
}

const fn read(argv: &'static [&'static str], shape: Shape, key: &'static str) -> Case {
    Case {
        argv,
        stdin: "",
        shape,
        key,
        write: false,
        isolated: false,
        git: false,
        refuses_lines: None,
    }
}

const fn write(argv: &'static [&'static str], key: &'static str) -> Case {
    Case {
        argv,
        stdin: "",
        shape: Shape::One,
        key,
        write: true,
        isolated: true,
        git: false,
        refuses_lines: None,
    }
}

const TEXT_REFUSAL: &str = "does not apply to this command's text output";

const fn text(argv: &'static [&'static str]) -> Case {
    read(argv, Shape::Text, "").refuses_lines(TEXT_REFUSAL)
}

impl Case {
    const fn stdin(mut self, stdin: &'static str) -> Self {
        self.stdin = stdin;
        self
    }

    const fn refuses_lines(mut self, message: &'static str) -> Self {
        self.refuses_lines = Some(message);
        self
    }

    /// A write whose envelope carries per-row results in `field`.
    const fn rows(mut self, field: &'static str) -> Self {
        self.shape = Shape::Field(field);
        self
    }

    const fn git(mut self) -> Self {
        self.git = true;
        self
    }

    /// A read whose fixture the command may change, so it runs on a copy.
    const fn isolated(mut self) -> Self {
        self.isolated = true;
        self
    }
}

const S: &str = TASKS_SLUG;

/// `{B1}` and `{B2}` stand for the two backlog ids the fixture mints.
fn cases() -> Vec<Case> {
    use Shape::*;
    vec![
        read(&["parse", LEDGER], One, "schema_version"),
        read(&["get", LEDGER, "items.0"], One, "id"),
        text(&["get", LEDGER, "items.0.id", "--raw"]),
        write(&["set", LEDGER, "last_updated", "2026-02-02"], "ok"),
        write(&["set-json", LEDGER, "meta", "--json", r#"{"a":1}"#], "ok"),
        read(&["validate", LEDGER], One, "ok"),
        read(&["items", "list", LEDGER], Top, "id"),
        read(&["items", "get", LEDGER, "R1"], One, "id"),
        write(
            &[
                "items",
                "add",
                LEDGER,
                "--id-prefix",
                "R",
                "--json",
                r#"{"status":"open","file":"src/c.rs","summary":"new"}"#,
            ],
            "id",
        ),
        write(
            &[
                "items",
                "add-many",
                LEDGER,
                "--id-prefix",
                "R",
                "--ndjson",
                "-",
            ],
            "ids",
        )
        .stdin(r#"{"status":"open","file":"src/c.rs","summary":"x"}"#),
        write(
            &[
                "items",
                "update",
                LEDGER,
                "R1",
                "--json",
                r#"{"status":"fixed"}"#,
            ],
            "ok",
        ),
        write(&["items", "remove", LEDGER, "R4"], "ok"),
        text(&["items", "next-id", LEDGER, "--prefix", "R"]),
        write(&["items", "apply", LEDGER, "--ops", "-"], "ok")
            .stdin(r#"[{"op":"remove","id":"R4"}]"#),
        read(&["items", "find-duplicates", LEDGER], Top, "tier"),
        read(&["items", "fingerprint", LEDGER, "R1"], One, "dedup_id"),
        read(&["items", "orphans", LEDGER], Top, "class"),
        read(
            &["items", "sweep", LEDGER, "--ids", "R3"],
            Field("items"),
            "id",
        )
        .git(),
        write(
            &["items", "sweep", LEDGER, "--ids", "R3", "--update"],
            "updated",
        )
        .git(),
        read(&["items", "clusters", LEDGER], Field("clusters"), "id"),
        write(&["items", "backfill-dedup-id", LEDGER], "backfilled"),
        read(
            &["blocks", "verify", "doc.md", "--block", "demo"],
            Field("blocks"),
            "name",
        ),
        write(
            &[
                "array-append",
                LEDGER,
                "items",
                "--json",
                r#"{"id":"R9","status":"open"}"#,
            ],
            "appended",
        ),
        read(&["capabilities"], One, "version"),
        write(&["integrity", "refresh", LEDGER], "ok"),
        read(&["flow", "active", "list"], One, "active"),
        write(&["flow", "active", "add", "--slug", "other"], "slug"),
        write(&["flow", "active", "remove", "--slug", S], "removed"),
        write(&["flow", "active", "touch", "--slug", S], "slug"),
        read(&["flow", "find-plans", "--dirs", "docs/plans"], Top, "slug"),
        read(&["flow", "stale", "--slug", S], One, "stale"),
        write(
            &[
                "flow",
                "init",
                "--slug",
                "fresh-flow",
                "--plan",
                "docs/plans/fresh.md",
            ],
            "slug",
        ),
        read(
            &["flow", "envelope", "build", "--command", "review"],
            One,
            "command",
        ),
        read(
            &[
                "flow",
                "ensure-artifact",
                "--slug",
                S,
                "--kind",
                "review-ledger",
            ],
            One,
            "exists",
        )
        .isolated(),
        read(&["flow", "resolve"], One, "slug"),
        read(&["flow", "doctor"], One, "ok").isolated(),
        read(&["flow", "list"], Field("flows"), "slug"),
        text(&["flow", "render-progress-log", "--slug", S, "--stdout"]),
        read(&["json", "get", JSON_DOC, "a"], One, "b"),
        text(&["json", "get", JSON_DOC, "a.b", "--raw"]),
        write(&["json", "set", JSON_DOC, "a.b", "--json", "2"], "path"),
        write(&["json", "unset", JSON_DOC, "a.b"], "path"),
        write(
            &[
                "backlog",
                "add",
                "--summary",
                "a brand new capture",
                "--kind",
                "debt",
            ],
            "id",
        ),
        write(&["backlog", "add-many", "--ndjson", "-"], "id")
            .rows("rows")
            .stdin(concat!(
                r#"{"summary":"third thing","kind":"debt"}"#,
                "\n",
                r#"{"summary":"fourth thing","kind":"bug"}"#
            )),
        read(
            &["backlog", "check", "--summary", "probe flakes on slow CI"],
            Field("candidates"),
            "id",
        ),
        read(&["backlog", "list"], Top, "id"),
        read(&["backlog", "show", "{B1}"], One, "item"),
        read(&["backlog", "show", "{B1},{B2}"], Top, "item"),
        write(
            &[
                "backlog",
                "relate",
                "{B1}",
                "--to",
                "{B2}",
                "--as",
                "relates-to",
            ],
            "relation",
        ),
        write(
            &[
                "backlog",
                "triage",
                "{B1}",
                "--dismiss",
                "--reason",
                "not real",
            ],
            "transition",
        ),
        read(&["backlog", "reconcile"], One, "ok"),
        read(&["backlog", "cluster"], One, "area"),
        write(&["backlog", "compact"], "remaining"),
        write(&["backlog", "evidence", "dir", "{B1}"], "dir"),
        read(
            &["backlog", "evidence", "audit"],
            Field("findings"),
            "class",
        ),
        write(&["tasks", "import-plan", "--slug", S], "unchanged"),
        write(
            &[
                "tasks", "add", "--slug", S, "--title", "Third", "--effort", "S", "--files",
                "src/c.rs",
            ],
            "id",
        ),
        write(
            &["tasks", "add-many", "--slug", S, "--ndjson", "-"],
            "added",
        )
        .stdin(r#"{"title":"Third","effort":"S","files":["src/c.rs"]}"#),
        write(
            &[
                "tasks",
                "update",
                "1",
                "--slug",
                S,
                "--status",
                "in-progress",
            ],
            "changed",
        ),
        write(
            &[
                "tasks",
                "update",
                "1,2",
                "--slug",
                S,
                "--status",
                "in-progress",
            ],
            "id",
        )
        .rows("results"),
        write(&["tasks", "remove", "2", "--slug", S], "ref"),
        read(&["tasks", "show", "1", "--slug", S], One, "ref"),
        read(&["tasks", "show", "1,2", "--slug", S], Top, "ref"),
        read(&["tasks", "list", "--slug", S], Top, "ref"),
        read(&["tasks", "edges", "--slug", S], Top, "kind"),
        text(&["tasks", "edges", "--slug", S, "--dot"]),
        read(&["tasks", "ready", "--slug", S], One, "ready"),
        read(&["tasks", "batches", "--slug", S], Field("batches"), "0"),
        read(
            &["tasks", "closure", "--task", "2", "--up", "--slug", S],
            One,
            "ids",
        ),
        read(&["tasks", "check", "--slug", S], Field("findings"), "class"),
        text(&["tasks", "render", "--slug", S, "--stdout"]),
        read(&["tasks", "snapshot", "--slug", S], Field("tasks"), "ref"),
        write(
            &["agents", "record", "--harness", "claude-code"],
            "recorded",
        )
        .stdin(r#"{"hook_event_name":"Notification","session_id":"s","agent_id":"a"}"#),
        read(&["agents", "list", "--slug", S], Top, "agent_id"),
        read(&["inputs", "list"], Field("inputs"), "id"),
        write(&["inputs", "add", "--json", "-"], "id")
            .stdin(r#"{"kind": "note", "text": "later"}"#),
        write(&["inputs", "ack", "I1", "--by", "review"], "applied"),
        write(
            &["inputs", "handle", "I1", "--by", "review", "--note", "done"],
            "applied",
        ),
        write(&["inputs", "withdraw", "I1"], "applied"),
        write(&["inputs", "answer", "I2", "--text", "because"], "id"),
        read(&["sweep", "--pattern", "fn "], Field("hits"), "file").git(),
    ]
}

/// The fixture every case runs against: a ledger, a JSON document, a flow
/// whose plan carries the rendered task sections, two inputs (`I1` a capture,
/// `I2` a question), two backlog items, one recorded agent, and an evidence
/// directory no item owns.
struct Fixture {
    _dir: tempfile::TempDir,
    _projects: tempfile::TempDir,
    root: PathBuf,
    backlog: [String; 2],
}

fn put(root: &Path, rel: &str, bytes: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn setup(root: &Path, argv: &[&str], stdin: &str) -> Value {
    let out = cli(root)
        .current_dir(root)
        .args(argv)
        .write_stdin(stdin.to_string())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "fixture step `{}` failed: {}",
        argv.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// Record one subagent dispatched on task 1, through a fake projects tree.
fn record_agent(root: &Path, projects: &Path) {
    let proj = projects.join("projects").join("p");
    let transcript = proj.join("s1").join("subagents").join("agent-a1.jsonl");
    fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    fs::write(proj.join("s1.jsonl"), "").unwrap();
    let dispatch = serde_json::json!({
        "parentUuid": null, "type": "user",
        "message": {"role": "user", "content": format!(
            "DISPATCH: implement-deep\nFetch it: `tomlctl tasks show 1 --slug {S} --with body`"
        )},
    });
    fs::write(&transcript, format!("{dispatch}\n")).unwrap();
    fs::write(
        transcript.with_extension("meta.json"),
        r#"{"agentType": "implement-deep", "description": "T1 work"}"#,
    )
    .unwrap();
    let payload = serde_json::json!({
        "hook_event_name": "SubagentStart",
        "session_id": "s1",
        "agent_id": "a1",
        "agent_type": "implement-deep",
        "transcript_path": proj.join("s1.jsonl").to_string_lossy(),
        "cwd": root.to_string_lossy(),
    });
    let out = setup(
        root,
        &["agents", "record", "--harness", "claude-code"],
        &payload.to_string(),
    );
    assert_eq!(out["recorded"], true, "fixture agent not recorded: {out}");
}

fn fixture() -> Fixture {
    let (dir, root) = sandbox();
    let projects = tempfile::tempdir().unwrap();
    seed_ledger_in(&root, "review-ledger.toml", LEDGER_FIXTURE);
    put(&root, "src/a.rs", "fn alpha() {}\n");
    put(&root, "src/b.rs", "fn gamma() {}\n");
    put(&root, "doc.md", BLOCK_DOC);
    put(&root, JSON_DOC, r#"{"a":{"b":1},"list":[1,2]}"#);
    put(&root, &format!("docs/plans/{S}.md"), "# Fixture plan\n");
    put(&root, ".claude/backlog-evidence/B-deadbeef/note.txt", "x\n");
    if git_available() {
        let added = git_command(&root).args(["add", "src"]).output().unwrap();
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
    }
    setup(
        &root,
        &[
            "flow",
            "init",
            "--slug",
            S,
            "--plan",
            &format!("docs/plans/{S}.md"),
        ],
        "",
    );
    let store = root.join(".claude/flows").join(S).join("tasks.toml");
    write_tasks_store(&store, TASKS_FIXTURE);
    setup(&root, &["tasks", "render", "--slug", S], "");
    setup(
        &root,
        &["inputs", "add", "--json", "-"],
        r#"{"kind": "capture", "text": "flaky probe", "capture_kind": "flaky-test"}"#,
    );
    setup(
        &root,
        &["inputs", "add", "--json", "-"],
        r#"{"kind": "question", "author": "review", "prompt": "Why?", "choice": "text"}"#,
    );
    let mint = |summary: &str, kind: &str| {
        let out = setup(
            &root,
            &["backlog", "add", "--summary", summary, "--kind", kind],
            "",
        );
        out["id"].as_str().unwrap().to_string()
    };
    let backlog = [
        mint("probe flakes on slow CI", "flaky-test"),
        mint("slow startup", "debt"),
    ];
    record_agent(&root, projects.path());
    Fixture {
        _dir: dir,
        _projects: projects,
        root,
        backlog,
    }
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    /// The directory the command ran in, kept alive so a write can be re-read.
    root: PathBuf,
    _copy: Option<tempfile::TempDir>,
}

/// Every file under `root`, by relative path. The git directory, the lock
/// files (named by a hash of the absolute path) and the flow registry (which
/// stamps the time of day) are left out because they differ between runs of
/// the same command.
/// Records stamp wall-clock datetimes, so two runs of one write differ there
/// and in the sidecar digest that follows; both are masked out.
fn datetime() -> &'static regex::bytes::Regex {
    static RE: std::sync::OnceLock<regex::bytes::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::bytes::Regex::new(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})?")
            .unwrap()
    })
}

fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if [
                ".git",
                ".locks",
                "active-flow.toml",
                "active-flow.toml.sha256",
            ]
            .iter()
            .any(|skip| name == *skip)
            {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                walk(root, &entry.path(), out);
            } else if !name.to_string_lossy().ends_with(".sha256") {
                let rel = entry.path().strip_prefix(root).unwrap().to_path_buf();
                let bytes = fs::read(entry.path()).unwrap();
                out.insert(
                    rel,
                    datetime()
                        .replace_all(&bytes, &b"<datetime>"[..])
                        .into_owned(),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Run `case` with `before` ahead of the subcommand and `after` behind it —
/// globals are accepted in either position. A write runs on its own copy of
/// the fixture.
fn run(fx: &Fixture, case: &Case, before: &[&str], after: &[&str]) -> Run {
    let mut copy = None;
    let root = if case.isolated {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        copy_tree(&fx.root, &root);
        copy = Some(dir);
        root
    } else {
        fx.root.clone()
    };
    let argv: Vec<String> = case
        .argv
        .iter()
        .map(|a| {
            a.replace("{B1}", &fx.backlog[0])
                .replace("{B2}", &fx.backlog[1])
        })
        .collect();
    let out = cli(&root)
        .current_dir(&root)
        .args(before)
        .args(&argv)
        .args(after)
        .write_stdin(case.stdin.to_string())
        .output()
        .unwrap();
    Run {
        code: out.status.code(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
        root,
        _copy: copy,
    }
}

fn nav<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => map.get(key),
        Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
        _ => None,
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn rows_of(report: &Value, shape: Shape) -> Option<&Vec<Value>> {
    match shape {
        Shape::Top => report.as_array(),
        Shape::Field(key) => report.get(key)?.as_array(),
        Shape::One | Shape::Text => None,
    }
}

/// The `--lines` encoding derived from the plain report, independently of
/// the binary's own split.
fn expected_lines(report: &Value, shape: Shape) -> Vec<Value> {
    match shape {
        Shape::Top => report.as_array().unwrap().clone(),
        Shape::Field(key) => {
            let mut header = report.as_object().unwrap().clone();
            let rows = header.shift_remove(key).unwrap();
            let mut lines = Vec::new();
            if !header.is_empty() {
                lines.push(Value::Object(header));
            }
            lines.extend(rows.as_array().unwrap().iter().cloned());
            lines
        }
        Shape::One | Shape::Text => vec![report.clone()],
    }
}

fn expected_select(report: &Value, shape: Shape, key: &str) -> Value {
    let project = |row: &Value| {
        let mut out = serde_json::Map::new();
        if let Some(v) = nav(row, key) {
            out.insert(key.to_string(), v.clone());
        }
        Value::Object(out)
    };
    match shape {
        Shape::Top => Value::Array(report.as_array().unwrap().iter().map(project).collect()),
        Shape::Field(field) => {
            let mut out = report.clone();
            let rows = out[field].as_array().unwrap().iter().map(project).collect();
            out[field] = Value::Array(rows);
            out
        }
        Shape::One | Shape::Text => project(report),
    }
}

fn expected_get(report: &Value, shape: Shape, key: &str) -> String {
    let lines: Vec<String> = match rows_of(report, shape) {
        Some(rows) => rows
            .iter()
            .map(|r| nav(r, key).map(value_text).unwrap_or_default())
            .collect(),
        None => match nav(report, key) {
            Some(Value::Array(items)) => items.iter().map(value_text).collect(),
            Some(v) => vec![value_text(v)],
            None => Vec::new(),
        },
    };
    lines.iter().map(|l| format!("{l}\n")).collect()
}

fn json_lines(stdout: &str) -> Result<Vec<Value>, String> {
    stdout
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|e| format!("{e}: {line}")))
        .collect()
}

fn keys(v: &Value) -> Vec<&String> {
    v.as_object()
        .map(|o| o.keys().collect())
        .unwrap_or_default()
}

/// Every failure of `case`, as messages; empty when it passes.
fn check(fx: &Fixture, case: &Case) -> Vec<String> {
    let what = case.argv.join(" ");
    let mut fails = Vec::new();
    let mut fail = |msg: String| fails.push(format!("`{what}`: {msg}"));

    let plain = run(fx, case, &[], &[]);
    if plain.code != Some(0) || plain.stdout.is_empty() {
        fail(format!(
            "plain run exited {:?} with stdout {:?}; stderr: {}",
            plain.code, plain.stdout, plain.stderr
        ));
        return fails;
    }

    let quiet = run(fx, case, &["-q"], &[]);
    if !quiet.stdout.is_empty() || quiet.code != plain.code {
        fail(format!(
            "-q exited {:?} (plain {:?}) and printed {:?}",
            quiet.code, plain.code, quiet.stdout
        ));
    }

    let lines = run(fx, case, &[], &["--lines"]);
    if let Some(message) = case.refuses_lines
        && (lines.code == Some(0) || !lines.stdout.is_empty() || !lines.stderr.contains(message))
    {
        fail(format!(
            "--lines must be refused with {message:?}; exited {:?}, stdout {:?}, stderr {:?}",
            lines.code, lines.stdout, lines.stderr
        ));
    }
    if let Shape::Text = case.shape {
        return fails;
    }

    let report: Value = match serde_json::from_str(&plain.stdout) {
        Ok(v) => v,
        Err(e) => {
            fail(format!("plain stdout is not one JSON value: {e}"));
            return fails;
        }
    };
    if !matches!(case.shape, Shape::One) && rows_of(&report, case.shape).is_none_or(Vec::is_empty) {
        fail(format!(
            "the fixture must give the report rows where {:?} puts them: {report}",
            case.shape
        ));
        return fails;
    }

    match json_lines(&lines.stdout) {
        _ if case.refuses_lines.is_some() => {}
        Err(e) => fail(format!("--lines line is not one JSON value: {e}")),
        Ok(got) if case.write && matches!(case.shape, Shape::One) => {
            if got.len() != 1 || keys(&got[0]) != keys(&report) {
                fail(format!(
                    "--lines must print the envelope as one line: {got:?}"
                ));
            }
        }
        Ok(got) => {
            let want = expected_lines(&report, case.shape);
            if got != want {
                fail(format!("--lines printed {got:?}, expected {want:?}"));
            }
        }
    }

    let select = run(fx, case, &[], &["--select", case.key]);
    let want = expected_select(&report, case.shape, case.key);
    match serde_json::from_str::<Value>(&select.stdout) {
        Ok(got) if got == want => {}
        _ => fail(format!(
            "--select {} printed {:?} (stderr {:?}), expected {want}",
            case.key, select.stdout, select.stderr
        )),
    }

    let get = run(fx, case, &[], &["--get", case.key]);
    let want = expected_get(&report, case.shape, case.key);
    if get.stdout != want || get.code != Some(0) {
        fail(format!(
            "--get {} printed {:?} (stderr {:?}), expected {want:?}",
            case.key, get.stdout, get.stderr
        ));
    }

    if let Some(rows) = rows_of(&report, case.shape) {
        let template = run(fx, case, &[], &["--template", &format!("{{{}}}", case.key)]);
        if template.code != Some(0) || template.stdout.lines().count() != rows.len() {
            fail(format!(
                "--template {{{}}} printed {:?} (stderr {:?}), expected {} lines",
                case.key,
                template.stdout,
                template.stderr,
                rows.len()
            ));
        }

        let engine_listed = matches!(case.argv, ["items" | "backlog" | "tasks", "list", ..]);
        if rows.len() > 1 && !engine_listed {
            let limited = run(fx, case, &[], &["--limit", "1", "--lines"]);
            match json_lines(&limited.stdout) {
                Ok(got)
                    if limited.code == Some(0)
                        && got.len() == 2
                        && got[0]["limited"]
                            == serde_json::json!({ "shown": 1, "total": rows.len() }) => {}
                _ => fail(format!(
                    "--limit 1 --lines printed {:?} (stderr {:?}), expected a limited header and 1 of {} rows",
                    limited.stdout,
                    limited.stderr,
                    rows.len()
                )),
            }
        }
    }

    if case.write {
        let absent = ["--get", "definitely_absent_field"];
        let shaped = run(fx, case, &[], &absent);
        if shaped.code != Some(0) || !shaped.stderr.contains("tomlctl: warning:") {
            fail(format!(
                "a failed --get on a write exited {:?} with stderr {:?}; expected exit 0 and a warning",
                shaped.code, shaped.stderr
            ));
        }
        if snapshot(&shaped.root) != snapshot(&plain.root) {
            fail("a failed --get on a write left different files than the plain write".into());
        }
        if case.argv[0] == "set" {
            let json = run(fx, case, &["--error-format", "json"], &absent);
            let warned = json.stderr.lines().any(|line| {
                serde_json::from_str::<Value>(line).is_ok_and(|v| v["warning"].is_object())
            });
            if json.code != Some(0) || !warned {
                fail(format!(
                    "--error-format json must put the warning on stderr as a JSON line: exited {:?}, stderr {:?}",
                    json.code, json.stderr
                ));
            }
        }
    }
    fails
}

/// A read verb that prints compact JSON fails a shaping error, unlike a
/// write: an unresolved flow must not hand `$(tomlctl flow resolve --get
/// slug)` the whole envelope with exit 0.
#[test]
fn a_compact_read_fails_a_bad_get() {
    let (_dir, root) = sandbox();
    put(&root, "doc.toml", "a = 1\n");
    let reads: &[&[&str]] = &[
        &["validate", "doc.toml"],
        &["flow", "resolve"],
        &["flow", "stale", "--slug", "nope", "--json"],
        &["flow", "stale", "--slug", "nope"],
        &["flow", "doctor"],
        &["flow", "envelope", "build", "--command", "review"],
        &["flow", "active", "list"],
    ];
    for argv in reads {
        let out = cli(&root)
            .current_dir(&root)
            .args(["--error-format", "json"])
            .args(*argv)
            .args(["--get", "nope"])
            .output()
            .unwrap();
        let what = argv.join(" ");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "`{what}`: stderr {stderr}");
        assert!(out.stdout.is_empty(), "`{what}` printed to stdout");
        let err: Value = serde_json::from_str(stderr.trim()).unwrap();
        assert_eq!(err["error"]["kind"], "validation", "`{what}`: {err}");
    }
}

/// The leaf command a case's argv names, walked down the capabilities tree.
fn leaf_of(commands: &Value, argv: &[&str]) -> String {
    let mut node = &commands[argv[0]];
    let mut path = vec![argv[0]];
    for word in &argv[1..] {
        match node.get("subcommands").and_then(|s| s.get(*word)) {
            Some(child) => {
                node = child;
                path.push(word);
            }
            None => break,
        }
    }
    assert!(
        node.get("subcommands").is_none(),
        "`{}` stops short of a leaf command",
        argv.join(" ")
    );
    path.join(" ")
}

fn leaves(prefix: &str, node: &Value, out: &mut BTreeSet<String>) {
    match node.get("subcommands").and_then(Value::as_object) {
        Some(subs) => {
            for (name, child) in subs {
                leaves(&format!("{prefix} {name}"), child, out);
            }
        }
        None => {
            out.insert(prefix.to_string());
        }
    }
}

fn capabilities_commands() -> Value {
    let (_dir, root) = sandbox();
    let out = cli(&root).arg("capabilities").output().unwrap();
    assert!(out.status.success());
    let caps: Value = serde_json::from_slice(&out.stdout).unwrap();
    caps["commands"].clone()
}

#[test]
fn every_leaf_command_has_a_case() {
    let commands = capabilities_commands();
    let mut listed = BTreeSet::new();
    for (name, node) in commands.as_object().unwrap() {
        leaves(name, node, &mut listed);
    }
    let covered: BTreeSet<String> = cases().iter().map(|c| leaf_of(&commands, c.argv)).collect();
    let missing: Vec<_> = listed.difference(&covered).collect();
    let unknown: Vec<_> = covered.difference(&listed).collect();
    assert!(
        missing.is_empty() && unknown.is_empty(),
        "commands without a case: {missing:?}; cases naming no command: {unknown:?}"
    );
}

#[test]
fn every_case_honours_the_output_options() {
    let fx = fixture();
    let git = git_available();
    let cases: Vec<Case> = cases().into_iter().filter(|c| git || !c.git).collect();
    let next = AtomicUsize::new(0);
    let fails = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let found = check(&fx, case);
                    fails.lock().unwrap().extend(found);
                }
            });
        }
    });
    let fails = fails.into_inner().unwrap();
    assert!(fails.is_empty(), "{}", fails.join("\n\n"));
}
