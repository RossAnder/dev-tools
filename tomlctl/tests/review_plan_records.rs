//! Plan-family findings that name a record, driven through the binary over a v2
//! flow built with `plan open`, `plan decide`, `plan milestone add` and
//! `tasks add-many`: what `review add` accepts and refuses, the tier stamped at
//! verdict and at advisory mint, the tier staying frozen, the report's target
//! lines, and the requiredness that markdown and code stores keep.

mod common;

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, sandbox};
use serde_json::{Value as JsonValue, json};

const SLUG: &str = "records";
const TASKS_FILE: &str = ".claude/flows/records/tasks.toml";
const PLAN_FILE: &str = ".claude/flows/records/plan.toml";
const BRIEF_FILE: &str = "docs/flows/records/brief.md";
const DESIGN_FILE: &str = "docs/flows/records/design/wiring.md";
const STORE: &str = ".claude/flows/records/review-v2.toml";
const CELLS: usize = 60;

const VOCABULARY: &str = r#"
[routing]
executable = ["**/*.rs"]
tests = ["tests/**"]
"#;

const BRIEF: &str = "# records\n\n## Intent\n\nStop the retry storm.\n\n## Non-goals\n\n\
    No-new-helper.\n\n## Scope budget\n\nFour files.\n\n## Success criteria\n\nA retry waits.\n\n\
    ## Background\n\nThe retry storm hit twice in one week.\n\n## Unknowns\n\nNone yet.\n";

const DESIGN: &str = "# Wiring\n\nThe guard wires into the caller once.\n";

const DECISION: &str = r#"{"decision":"Guard-bound-decision.","why":"Guard-bound-why.","provenance":"agent-assumed","door":"one-way","binds":["1"],"surfaced":true}"#;

const CONTRACT: &str =
    r#"{"kind":"invariant","claim":"Both-agree-claim.","check":"test -f src/a.rs","tasks":[1,2]}"#;

const TASK_RECORDS: &str = r#"[[record]]
title = "Add the guard"
effort = "S"
checkpoint = "A"
action = "Guard-action."
files = ["src/guard.rs", "src/a.rs", "src/gone.rs"]

[record.file_notes]
"src/guard.rs" = "(new)"

[record.spec]
outcome = "Guard-outcome."
executor = "lite"
constraints = ["No-new-deps.", "Keep-it-small."]

[[record.spec.accept]]
behaviour = "the guard exists"
check = "test -f src/guard.rs"
positive = "test -f src/a.rs"
kind = "forward"
writes_build = false

[[record]]
title = "Wire the guard"
effort = "S"
checkpoint = "A"
needs = [1]
action = "Wire-action."
files = ["src/a.rs"]

[record.spec]
outcome = "Wired-outcome."
executor = "deep"

[[record.spec.accept]]
behaviour = "the caller is there"
check = "test -f src/a.rs"
kind = "falsifier"
writes_build = false

[[record]]
title = "Spare work"
effort = "S"
checkpoint = "A"
files = ["src/a.rs"]

[record.spec]
outcome = "Spare-outcome."
"#;

struct Flow {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    scratch: tempfile::TempDir,
    root: PathBuf,
    next_cell: Cell<usize>,
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A catalogue directory holding only the routing globs, as a path a store's
/// run can name.
fn catalogue() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let builtin = dir.path().join("review-v2-lenses");
    fs::create_dir_all(&builtin).unwrap();
    fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
    let path = builtin.to_string_lossy().replace('\\', "/");
    (dir, path)
}

fn run_entry(id: &str, family: &str, builtin: &str) -> String {
    format!(
        "[[runs]]\nid = \"{id}\"\ncommand = \"review-plan-v2\"\nfamily = \"{family}\"\n\
         depth = \"standard\"\nstarted = \"2026-10-12T10:00:00Z\"\n\
         catalogue = {{ builtin = \"{builtin}\" }}\n\n"
    )
}

fn cell_entry(id: &str, run: &str) -> String {
    format!(
        "[[cells]]\nid = \"{id}\"\nrun = \"{run}\"\nwave = 1\nslice = [\"src/**\"]\n\
         concerns = [\"plan-structure\", \"contrarian\"]\nagent = \"review-finder\"\n\
         reason = \"proposed\"\nstatus = \"assigned\"\n\n"
    )
}

impl Flow {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let (catalogue_dir, builtin) = catalogue();
        write(&root, "src/a.rs", "fn base() {}\n");
        let flow = Self {
            _dir: dir,
            _catalogue: catalogue_dir,
            scratch: tempfile::tempdir().unwrap(),
            root,
            next_cell: Cell::new(1),
        };

        flow.ok(&["plan", "open", "--slug", SLUG]);
        write(&flow.root, BRIEF_FILE, BRIEF);
        write(&flow.root, DESIGN_FILE, DESIGN);
        let decision = flow.payload("decision.ndjson", &format!("{DECISION}\n"));
        flow.ok(&["plan", "decide", "--slug", SLUG, "--ndjson", &decision]);
        let contract = flow.payload("contract.ndjson", &format!("{CONTRACT}\n"));
        flow.ok(&["plan", "contract", "--slug", SLUG, "--ndjson", &contract]);
        flow.ok(&[
            "plan",
            "handover",
            "add",
            "--slug",
            SLUG,
            "--command",
            "cargo install --path tomlctl",
            "--source-task",
            "1",
        ]);
        flow.ok(&[
            "plan",
            "milestone",
            "add",
            "--slug",
            SLUG,
            "--id",
            "A",
            "--goal",
            "The guard",
            "--gate",
            "cargo test",
        ]);
        let records = flow.payload("tasks.toml", TASK_RECORDS);
        flow.ok(&["tasks", "add-many", "--slug", SLUG, "--ndjson", &records]);

        // The task-update path refuses an empty outcome, so the store is edited
        // to hold the defect a `check-class` proof names.
        let tasks = flow.root.join(TASKS_FILE);
        let text = fs::read_to_string(&tasks).unwrap();
        let emptied = text.replace("outcome = \"Spare-outcome.\"", "outcome = \"\"");
        assert_ne!(emptied, text, "the spare task's outcome was not found");
        fs::write(&tasks, emptied).unwrap();

        let mut store = String::from("schema_version = 1\n\n");
        store.push_str(&run_entry("U1", "plan", &builtin));
        for n in 1..=CELLS {
            store.push_str(&cell_entry(&format!("C{n}"), "U1"));
        }
        write(&flow.root, STORE, &store);
        flow
    }

    fn tomlctl(&self, args: &[&str]) -> assert_cmd::Command {
        let mut cmd = cli(&self.root);
        cmd.current_dir(&self.root).args(args).write_stdin("");
        cmd
    }

    fn payload(&self, name: &str, body: &str) -> String {
        let path = self.scratch.path().join(name);
        fs::write(&path, body).unwrap();
        path.to_str().unwrap().to_string()
    }

    /// A call that must succeed, as the JSON it printed.
    fn ok(&self, args: &[&str]) -> JsonValue {
        let out = self.tomlctl(args).output().expect("the binary runs");
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(
            out.status.success(),
            "`{}` failed\nstdout: {stdout}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("`{}`: {e}; stdout:\n{stdout}", args.join(" ")))
    }

    /// A call that must fail, as its stderr, having left the review store as
    /// it was.
    fn refused(&self, args: &[&str]) -> String {
        let before = self.store_bytes();
        let out = self.tomlctl(args).output().expect("the binary runs");
        assert!(
            !out.status.success(),
            "`{}` succeeded:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout)
        );
        assert_eq!(self.store_bytes(), before, "a refused call wrote");
        String::from_utf8_lossy(&out.stderr).to_string()
    }

    fn store_bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(STORE)).unwrap()
    }

    fn cell(&self) -> String {
        let n = self.next_cell.get();
        assert!(n <= CELLS, "the fixture ran out of cells");
        self.next_cell.set(n + 1);
        format!("C{n}")
    }

    /// Adds `rows` to a fresh cell. A cell's set replaces its earlier
    /// unverified rows, so every call takes its own.
    fn add_args(&self, rows: &[JsonValue]) -> Vec<String> {
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        let payload = self.payload("rows.ndjson", &text);
        [
            "review",
            "add",
            "--slug",
            SLUG,
            "--run",
            "U1",
            "--cell",
            &self.cell(),
            "--ndjson",
            &payload,
        ]
        .map(String::from)
        .to_vec()
    }

    /// The id of the one finding `row` mints.
    fn minted(&self, row: JsonValue) -> String {
        let args = self.add_args(&[row]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = self.ok(&args);
        assert!(out["refused"].as_array().unwrap().is_empty(), "{out}");
        out["findings"][0]["id"].as_str().unwrap().to_string()
    }

    /// The stderr of an add whose only row is refused.
    fn add_refused(&self, row: JsonValue) -> String {
        let args = self.add_args(&[row]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.refused(&args)
    }

    fn verdict(&self, id: &str, verdict: &str, citation: &str) -> JsonValue {
        self.ok(&[
            "review",
            "verdict",
            id,
            "--slug",
            SLUG,
            "--verdict",
            verdict,
            "--by",
            "verifier",
            "--citation",
            citation,
            "--rationale",
            "checked against the record",
        ])
    }

    fn store(&self) -> toml::Value {
        toml::from_str(&fs::read_to_string(self.root.join(STORE)).unwrap()).unwrap()
    }

    fn finding(&self, id: &str) -> toml::Value {
        self.store()["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no finding {id}"))
            .clone()
    }

    fn tier(&self, id: &str) -> Option<String> {
        self.finding(id)
            .get("tier")
            .and_then(toml::Value::as_str)
            .map(String::from)
    }

    fn status(&self, id: &str) -> String {
        self.finding(id)["status"].as_str().unwrap().to_string()
    }

    /// The first accept entry of the task, as the store holds it.
    fn accept_entry(&self, task_ref: &str) -> JsonValue {
        let doc: toml::Value =
            toml::from_str(&fs::read_to_string(self.root.join(TASKS_FILE)).unwrap()).unwrap();
        let spec = doc["specs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["ref"].as_str() == Some(task_ref))
            .expect("the spec");
        serde_json::to_value(&spec["accept"][0]).unwrap()
    }
}

fn base(kind: &str) -> JsonValue {
    json!({
        "kind": kind,
        "lens": "plan-structure",
        "summary": "the record is stale",
        "severity": "major",
        "failure_scenario": "a worker reads the stale value",
        "fix": "write the current value",
        "fix_shape": "edits",
        "effort": "trivial",
        "counter": "nobody reads it",
        "grade": "high",
        "evidence": ["the record shows it"],
    })
}

/// A flow-mode defect editing one record.
fn edit(
    target: &str,
    file: &str,
    symbol: &str,
    expect: Option<JsonValue>,
    new: Option<JsonValue>,
) -> JsonValue {
    let mut row = base("defect");
    row["target"] = json!(target);
    row["file"] = json!(file);
    row["symbol"] = json!(symbol);
    if let Some(expect) = expect {
        row["expect"] = expect;
    }
    if let Some(new) = new {
        row["new"] = new;
    }
    row
}

fn task_edit(
    task_ref: &str,
    field: &str,
    expect: Option<JsonValue>,
    new: Option<JsonValue>,
) -> JsonValue {
    edit(
        &format!("task:{task_ref}.{field}"),
        TASKS_FILE,
        task_ref,
        expect,
        new,
    )
}

fn advisory(file: &str, symbol: &str) -> JsonValue {
    json!({
        "kind": "advisory",
        "lens": "contrarian",
        "file": file,
        "symbol": symbol,
        "summary": "the guard could be a hook",
        "angle": "alternative",
        "rationale": "a hook needs no new row",
        "adoption_cost": "a second mechanism",
    })
}

fn stored_text(finding: &toml::Value, key: &str) -> Option<String> {
    finding
        .get(key)
        .and_then(toml::Value::as_str)
        .map(String::from)
}

#[test]
fn every_target_kind_is_accepted_and_stored_as_json_text() {
    let flow = Flow::new();
    let rows = vec![
        task_edit(
            "add-the-guard",
            "action",
            Some(json!("Guard-action.")),
            Some(json!("Guard-action-revised.")),
        ),
        task_edit(
            "add-the-guard",
            "spec.executor",
            Some(json!("lite")),
            Some(json!("deep")),
        ),
        task_edit(
            "add-the-guard",
            "spec.constraints[0]",
            Some(json!("No-new-deps.")),
            Some(json!("No-new-deps-at-all.")),
        ),
        task_edit(
            "add-the-guard",
            "spec.constraints[+]",
            None,
            Some(json!("Document-it.")),
        ),
        task_edit(
            "add-the-guard",
            "spec.constraints[1]",
            Some(json!("Keep-it-small.")),
            Some(JsonValue::Null),
        ),
        edit(
            "decision:D1.why",
            PLAN_FILE,
            "D1",
            Some(json!("Guard-bound-why.")),
            Some(json!("Guard-bound-why-revised.")),
        ),
        edit(
            "contract:C1.claim",
            PLAN_FILE,
            "C1",
            Some(json!("Both-agree-claim.")),
            Some(json!("Both-agree-claim-revised.")),
        ),
        edit(
            "milestone:A.gates",
            TASKS_FILE,
            "milestone:A",
            Some(json!(["cargo test"])),
            Some(json!(["cargo test --lib"])),
        ),
        edit(
            "milestone:A.defer_reason",
            TASKS_FILE,
            "milestone:A",
            Some(json!("")),
            Some(json!("Waits on B.")),
        ),
        edit(
            "handover:H1.done",
            PLAN_FILE,
            "H1",
            Some(json!(false)),
            Some(json!(true)),
        ),
        edit(
            "brief#Background",
            BRIEF_FILE,
            "Background",
            Some(json!("retry storm")),
            Some(json!("retry storms")),
        ),
        edit(
            "design/wiring.md#wiring",
            DESIGN_FILE,
            "wiring",
            Some(json!("wires into the caller")),
            Some(json!("wires into one caller")),
        ),
    ];
    for row in rows {
        let target = row["target"].as_str().unwrap().to_string();
        let id = flow.minted(row.clone());
        let finding = flow.finding(&id);
        assert_eq!(stored_text(&finding, "target").as_deref(), Some(&*target));
        for key in ["expect", "new"] {
            assert_eq!(
                stored_text(&finding, key),
                row.get(key).map(JsonValue::to_string),
                "{target}: `{key}` is stored as JSON text"
            );
        }
        assert_eq!(flow.status(&id), "unverified", "{target}");
    }
}

#[test]
fn an_empty_expect_and_a_removal_survive_the_store_round_trip() {
    let flow = Flow::new();
    let empty = flow.minted(edit(
        "milestone:A.defer_reason",
        TASKS_FILE,
        "milestone:A",
        Some(json!("")),
        Some(json!("Waits on B.")),
    ));
    assert_eq!(
        stored_text(&flow.finding(&empty), "expect").as_deref(),
        Some("\"\"")
    );
    let removal = flow.minted(task_edit(
        "add-the-guard",
        "spec.constraints[1]",
        Some(json!("Keep-it-small.")),
        Some(JsonValue::Null),
    ));
    assert_eq!(
        stored_text(&flow.finding(&removal), "new").as_deref(),
        Some("null")
    );
}

#[test]
fn a_stale_or_misplaced_row_is_refused_and_nothing_is_written() {
    let flow = Flow::new();
    let stale = flow.add_refused(task_edit(
        "add-the-guard",
        "action",
        Some(json!("Some-other-action.")),
        Some(json!("Guard-action-revised.")),
    ));
    assert!(stale.contains("not the current value"), "{stale}");

    let unknown = flow.add_refused(task_edit(
        "gone",
        "action",
        Some(json!("x")),
        Some(json!("y")),
    ));
    assert!(unknown.contains("no task with ref `gone`"), "{unknown}");

    let no_expect = flow.add_refused(task_edit(
        "add-the-guard",
        "action",
        None,
        Some(json!("Guard-action-revised.")),
    ));
    assert!(no_expect.contains("`new` needs `expect`"), "{no_expect}");

    let mut elsewhere = task_edit(
        "add-the-guard",
        "action",
        Some(json!("Guard-action.")),
        Some(json!("Guard-action-revised.")),
    );
    elsewhere["file"] = json!("src/a.rs");
    let misplaced = flow.add_refused(elsewhere);
    assert!(misplaced.contains("`file` is"), "{misplaced}");

    let mut markdown = task_edit("add-the-guard", "action", None, None);
    markdown["plan_section"] = json!("### 1. Add the guard");
    let section = flow.add_refused(markdown);
    assert!(section.contains("does not apply on a v2 flow"), "{section}");

    let mut bare = task_edit("add-the-guard", "action", None, None);
    bare.as_object_mut().unwrap().remove("target");
    let untargeted = flow.add_refused(bare);
    assert!(untargeted.contains("`target` is required"), "{untargeted}");

    let mut absent = edit(
        "brief#Background",
        BRIEF_FILE,
        "Background",
        Some(json!("not in the brief")),
        Some(json!("x")),
    );
    absent["symbol"] = json!("Background");
    let prose = flow.add_refused(absent);
    assert!(prose.contains("does not occur"), "{prose}");

    let mut advisory_edit = advisory(TASKS_FILE, "add-the-guard");
    advisory_edit["target"] = json!("task:add-the-guard.action");
    advisory_edit["new"] = json!("Guard-action-revised.");
    let carried = flow.add_refused(advisory_edit);
    assert!(carried.contains("does not apply"), "{carried}");
}

#[test]
fn a_proof_that_does_not_hold_is_refused() {
    let flow = Flow::new();
    let entry = flow.accept_entry("add-the-guard");
    let mut fixed = entry.clone();
    fixed["check"] = json!("test -f src/a.rs");
    let mut probe = task_edit("add-the-guard", "spec.accept[0]", Some(entry), Some(fixed));
    probe["proof"] = json!({"kind": "probe"});
    let reasons = flow.add_refused(probe);
    assert!(reasons.contains("`proof` does not hold"), "{reasons}");
    assert!(reasons.contains("no probe verdict"), "{reasons}");

    let mut exists = task_edit(
        "add-the-guard",
        "files",
        Some(json!(["src/guard.rs", "src/a.rs", "src/gone.rs"])),
        Some(json!(["src/guard.rs", "src/gone.rs"])),
    );
    exists["proof"] = json!({"kind": "path-missing", "path": "src/a.rs"});
    let reasons = flow.add_refused(exists);
    assert!(reasons.contains("`src/a.rs` exists"), "{reasons}");

    let mut wide = task_edit(
        "add-the-guard",
        "files",
        Some(json!(["src/guard.rs", "src/a.rs", "src/gone.rs"])),
        Some(json!(["src/other.rs"])),
    );
    wide["proof"] = json!({"kind": "path-missing", "path": "src/gone.rs"});
    let reasons = flow.add_refused(wide);
    assert!(reasons.contains("`proof` does not hold"), "{reasons}");
}

#[test]
fn the_tier_is_stamped_when_a_verdict_opens_the_finding() {
    let flow = Flow::new();
    let cite_store = format!("{TASKS_FILE}:1");

    let mut dropped = task_edit(
        "add-the-guard",
        "files",
        Some(json!(["src/guard.rs", "src/a.rs", "src/gone.rs"])),
        Some(json!(["src/guard.rs", "src/a.rs"])),
    );
    dropped["proof"] = json!({"kind": "path-missing", "path": "src/gone.rs"});
    let mechanical = flow.minted(dropped);
    assert_eq!(flow.tier(&mechanical), None, "no tier before a verdict");
    flow.verdict(&mechanical, "confirmed", "src/a.rs:1");
    assert_eq!(flow.status(&mechanical), "open");
    assert_eq!(flow.tier(&mechanical).as_deref(), Some("mechanical"));

    let cited = flow.minted(task_edit(
        "add-the-guard",
        "action",
        Some(json!("Guard-action.")),
        Some(json!("Guard-action-revised.")),
    ));
    flow.verdict(&cited, "confirmed", &cite_store);
    assert_eq!(flow.tier(&cited).as_deref(), Some("cited"));

    let uncited = flow.minted(task_edit(
        "wire-the-guard",
        "action",
        Some(json!("Wire-action.")),
        Some(json!("Wire-action-revised.")),
    ));
    flow.verdict(&uncited, "confirmed", "src/a.rs:1");
    assert_eq!(
        flow.tier(&uncited).as_deref(),
        Some("advisory"),
        "agreement alone stays advisory"
    );

    let mut outcome = task_edit(
        "spare-work",
        "spec.outcome",
        Some(json!("")),
        Some(json!("Spare things work.")),
    );
    outcome["proof"] = json!({"kind": "check-class", "class": "plan/spec-no-outcome"});
    let user = flow.minted(outcome);
    flow.verdict(&user, "confirmed", &cite_store);
    assert_eq!(
        flow.tier(&user).as_deref(),
        Some("user"),
        "a valid proof does not lift an outcome out of the user tier"
    );

    let decision = flow.minted(edit(
        "decision:D1.why",
        PLAN_FILE,
        "D1",
        Some(json!("Guard-bound-why.")),
        Some(json!("Guard-bound-why-revised.")),
    ));
    flow.verdict(&decision, "confirmed", &format!("{PLAN_FILE}:1"));
    assert_eq!(flow.tier(&decision).as_deref(), Some("user"));

    let refuted = flow.minted(task_edit(
        "wire-the-guard",
        "effort",
        Some(json!("S")),
        Some(json!("L")),
    ));
    flow.verdict(&refuted, "refuted", &cite_store);
    assert_eq!(flow.status(&refuted), "refuted");
    assert_eq!(
        flow.tier(&refuted),
        None,
        "a refuted finding is not stamped"
    );
}

#[test]
fn a_stamped_tier_is_frozen() {
    let flow = Flow::new();
    let id = flow.minted(task_edit(
        "add-the-guard",
        "action",
        Some(json!("Guard-action.")),
        Some(json!("Guard-action-revised.")),
    ));
    flow.verdict(&id, "confirmed", "src/a.rs:1");
    assert_eq!(flow.tier(&id).as_deref(), Some("advisory"));

    // The same verdict now carries a citation that would earn `cited`.
    flow.verdict(&id, "confirmed", &format!("{TASKS_FILE}:1"));
    assert_eq!(flow.status(&id), "open");
    assert_eq!(flow.tier(&id).as_deref(), Some("advisory"));
}

#[test]
fn a_reopened_finding_is_stamped() {
    let flow = Flow::new();
    let id = flow.minted(task_edit(
        "add-the-guard",
        "action",
        Some(json!("Guard-action.")),
        Some(json!("Guard-action-revised.")),
    ));
    flow.verdict(&id, "refuted", &format!("{TASKS_FILE}:1"));
    assert_eq!(flow.tier(&id), None);
    flow.ok(&[
        "review",
        "triage",
        &id,
        "--slug",
        SLUG,
        "--reopen",
        "the refutation was wrong",
    ]);
    assert_eq!(flow.status(&id), "open");
    assert_eq!(flow.tier(&id).as_deref(), Some("advisory"));
}

#[test]
fn advisory_rows_are_stamped_at_mint() {
    let flow = Flow::new();
    let plain = flow.minted(advisory(BRIEF_FILE, "Background"));
    assert_eq!(flow.status(&plain), "open");
    assert_eq!(flow.tier(&plain).as_deref(), Some("advisory"));

    let mut about_a_decision = advisory(PLAN_FILE, "D1");
    about_a_decision["target"] = json!("decision:D1.why");
    let question = flow.minted(about_a_decision);
    assert_eq!(flow.status(&question), "open");
    assert_eq!(flow.tier(&question).as_deref(), Some("question"));

    let err = flow.refused(&[
        "review", "triage", &question, "--slug", SLUG, "--park", "later",
    ]);
    assert!(err.contains("question tier"), "{err}");
    let err = flow.refused(&[
        "review",
        "apply",
        &question,
        "--slug",
        SLUG,
        "--user-accepted",
    ]);
    assert!(err.contains("review triage --resolve"), "{err}");
    flow.ok(&[
        "review",
        "triage",
        &question,
        "--slug",
        SLUG,
        "--resolve",
        "kept by user",
    ]);
    assert_eq!(flow.status(&question), "resolved");
}

#[test]
fn a_conversion_to_a_bad_target_is_refused() {
    let flow = Flow::new();
    let id = flow.minted(advisory(BRIEF_FILE, "Background"));
    let mut replacement = task_edit("gone", "action", Some(json!("x")), Some(json!("y")));
    replacement["lens"] = json!("plan-structure");
    let payload = flow.payload("convert.ndjson", &format!("{replacement}\n"));
    let stderr = flow.refused(&[
        "review",
        "triage",
        &id,
        "--slug",
        SLUG,
        "--convert",
        "defect",
        "--ndjson",
        &payload,
    ]);
    assert!(stderr.contains("no task with ref `gone`"), "{stderr}");
    assert_eq!(flow.status(&id), "open");
}

#[test]
fn the_report_shows_the_target_and_its_edit() {
    let flow = Flow::new();
    let id = flow.minted(task_edit(
        "add-the-guard",
        "action",
        Some(json!("Guard-action.")),
        Some(json!("Guard-action-revised.")),
    ));
    flow.verdict(&id, "confirmed", &format!("{TASKS_FILE}:1"));
    let out = flow.root.join(".claude").join("report.md");
    flow.ok(&[
        "review",
        "report",
        "--slug",
        SLUG,
        "--run",
        "U1",
        "--out",
        out.to_str().unwrap(),
    ]);
    let report = fs::read_to_string(&out).unwrap();
    assert!(report.contains("`task:add-the-guard.action`"), "{report}");
    assert!(
        report.contains("  - Target: task:add-the-guard.action"),
        "{report}"
    );
    assert!(report.contains("  - Expect: \"Guard-action.\""), "{report}");
    assert!(
        report.contains("  - New: \"Guard-action-revised.\""),
        "{report}"
    );
    assert!(report.contains("  - Tier: cited"), "{report}");
}

/// A `--scope` store with a plan run and a code run, the shape every
/// non-flow store has.
struct Scoped {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
}

impl Scoped {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let (catalogue_dir, builtin) = catalogue();
        write(&root, "src/lib.rs", "const LIMIT: usize = 8;\n");
        write(
            &root,
            "docs/plans/p.md",
            "# Plan\n\n## Tasks\n\n### 1. Add it [S]\n",
        );
        let mut store = String::from("schema_version = 1\n\n");
        store.push_str(&run_entry("P1", "plan", &builtin));
        store.push_str(&run_entry("K1", "code", &builtin));
        store.push_str(&cell_entry("C1", "P1"));
        store.push_str(&cell_entry("C2", "P1"));
        store.push_str(&cell_entry("C3", "K1"));
        store.push_str(&cell_entry("C4", "K1"));
        write(&root, ".claude/review-v2/t.toml", &store);
        Self {
            _dir: dir,
            _catalogue: catalogue_dir,
            root,
        }
    }

    fn add(&self, run: &str, cell: &str, row: &JsonValue) -> assert_cmd::assert::Assert {
        let payload = self.root.join(".claude/payload.ndjson");
        fs::write(&payload, format!("{row}\n")).unwrap();
        cli(&self.root)
            .current_dir(&self.root)
            .args([
                "review", "add", "--scope", "t", "--run", run, "--cell", cell,
            ])
            .arg("--ndjson")
            .arg(payload)
            .write_stdin("")
            .assert()
    }
}

fn stderr_of(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).to_string()
}

#[test]
fn markdown_and_code_stores_keep_their_requiredness() {
    let scoped = Scoped::new();

    let mut plan = base("defect");
    plan["file"] = json!("docs/plans/p.md");
    plan["symbol"] = json!("add-it");
    plan["line"] = json!(5);
    let refused = scoped.add("P1", "C1", &plan).failure();
    assert!(
        stderr_of(&refused).contains("`plan_section` is required"),
        "{}",
        stderr_of(&refused)
    );
    plan["plan_section"] = json!("### 1. Add it [S]");
    scoped.add("P1", "C2", &plan).success();

    let mut code = base("defect");
    code["file"] = json!("src/lib.rs");
    code["symbol"] = json!("lib::limit");
    let refused = scoped.add("K1", "C3", &code).failure();
    assert!(
        stderr_of(&refused).contains("`line` is required"),
        "{}",
        stderr_of(&refused)
    );
    code["line"] = json!(1);
    scoped.add("K1", "C4", &code).success();
}
