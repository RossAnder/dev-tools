//! `tomlctl review apply` over a v2 flow built with `plan open`, `plan decide`,
//! `plan milestone add` and `tasks add-many`: findings minted by `review add`,
//! settled by `review verdict`, then written to the records they name. Covers
//! the tiers, the refusals that write nothing, compare-and-set, repeated edits,
//! the approved and audited flows, prose edits and `--dry-run`.

mod common;

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, git_available, git_command, sandbox};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};

const SLUG: &str = "applying";
const TASKS_FILE: &str = ".claude/flows/applying/tasks.toml";
const PLAN_FILE: &str = ".claude/flows/applying/plan.toml";
const BRIEF_FILE: &str = "docs/flows/applying/brief.md";
const STORE: &str = ".claude/flows/applying/review-v2.toml";
const CELLS: usize = 60;

const VOCABULARY: &str = r#"
[routing]
executable = ["**/*.rs"]
tests = ["tests/**"]
"#;

const BRIEF: &str = "# applying\n\n## Intent\n\nStop the retry storm.\n\n## Non-goals\n\n\
    No-new-helper.\n\n## Scope budget\n\nFour files.\n\n## Success criteria\n\nA retry waits.\n\n\
    ## Background\n\nThe retry storm hit twice in one week.\n\n## Unknowns\n\nNone yet.\n";

const DECISION: &str = r#"{"decision":"Guard-bound-decision.","why":"Guard-bound-why.","provenance":"agent-assumed","door":"one-way","binds":["1"],"surfaced":true}"#;

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
constraints = ["No-new-deps."]

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
constraints = ["Old-constraint.", "Other-constraint."]

[[record.spec.accept]]
behaviour = "the caller is there"
check = "test -f src/a.rs"
kind = "falsifier"
writes_build = false

[[record]]
title = "Spare work"
effort = "S"
checkpoint = "A"
action = "Spare-action."
files = ["src/a.rs"]

[record.spec]
outcome = "Spare-outcome."
constraints = ["A-one.", "A-two."]

[[record.spec.accept]]
behaviour = "the spare holds"
check = "test -f src/a.rs"
kind = "falsifier"
writes_build = false
"#;

struct Flow {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    scratch: tempfile::TempDir,
    root: PathBuf,
    builtin: String,
    next_cell: Cell<usize>,
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn digest(command: &str) -> String {
    Sha256::digest(command.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Flow {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let catalogue = tempfile::tempdir().unwrap();
        let lenses = catalogue.path().join("review-v2-lenses");
        fs::create_dir_all(&lenses).unwrap();
        fs::write(lenses.join("vocabulary.toml"), VOCABULARY).unwrap();
        let builtin = lenses.to_string_lossy().replace('\\', "/");
        write(&root, "src/a.rs", "fn base() {}\n");
        write(&root, "src/b.rs", "fn other() {}\n");
        let flow = Self {
            _dir: dir,
            _catalogue: catalogue,
            scratch: tempfile::tempdir().unwrap(),
            root,
            builtin,
            next_cell: Cell::new(1),
        };

        flow.ok(&["plan", "open", "--slug", SLUG]);
        write(&flow.root, BRIEF_FILE, BRIEF);
        let decision = flow.payload("decision.ndjson", &format!("{DECISION}\n"));
        flow.ok(&["plan", "decide", "--slug", SLUG, "--ndjson", &decision]);
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

        let mut store = String::from("schema_version = 1\n\n");
        store.push_str(&format!(
            "[[runs]]\nid = \"U1\"\ncommand = \"review-plan-v2\"\nfamily = \"plan\"\n\
             depth = \"standard\"\nstarted = \"2026-10-12T10:00:00Z\"\n\
             catalogue = {{ builtin = \"{}\" }}\n\n",
            flow.builtin
        ));
        for n in 1..=CELLS {
            store.push_str(&format!(
                "[[cells]]\nid = \"C{n}\"\nrun = \"U1\"\nwave = 1\nslice = [\"src/**\"]\n\
                 concerns = [\"plan-structure\", \"contrarian\"]\nagent = \"review-finder\"\n\
                 reason = \"proposed\"\nstatus = \"assigned\"\n\n"
            ));
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

    /// Every file a call could write, as bytes.
    fn snapshot(&self) -> Vec<Option<Vec<u8>>> {
        [TASKS_FILE, PLAN_FILE, BRIEF_FILE, STORE]
            .iter()
            .map(|rel| fs::read(self.root.join(rel)).ok())
            .collect()
    }

    /// A call that must fail, as its stderr, having written nothing.
    fn refused(&self, args: &[&str]) -> String {
        let before = self.snapshot();
        let out = self.tomlctl(args).output().expect("the binary runs");
        assert!(
            !out.status.success(),
            "`{}` succeeded:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout)
        );
        assert_eq!(self.snapshot(), before, "a refused call wrote");
        String::from_utf8_lossy(&out.stderr).to_string()
    }

    fn cell(&self) -> String {
        let n = self.next_cell.get();
        assert!(n <= CELLS, "the fixture ran out of cells");
        self.next_cell.set(n + 1);
        format!("C{n}")
    }

    /// The id of the one finding `row` mints, in a cell of its own.
    fn minted(&self, row: JsonValue) -> String {
        let payload = self.payload("rows.ndjson", &format!("{row}\n"));
        let out = self.ok(&[
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
        ]);
        assert!(out["refused"].as_array().unwrap().is_empty(), "{out}");
        out["findings"][0]["id"].as_str().unwrap().to_string()
    }

    fn verdict(&self, id: &str, verdict: &str, citation: &str) {
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
        ]);
    }

    /// Mints `row` and settles it open under a verdict citing `citation`.
    fn opened(&self, row: JsonValue, citation: &str) -> String {
        let id = self.minted(row);
        self.verdict(&id, "confirmed", citation);
        id
    }

    fn apply(&self, ids: &[&str], flags: &[&str]) -> JsonValue {
        let mut args = vec!["review", "apply"];
        args.extend_from_slice(ids);
        args.extend_from_slice(&["--slug", SLUG]);
        args.extend_from_slice(flags);
        self.ok(&args)
    }

    fn apply_refused(&self, ids: &[&str], flags: &[&str]) -> String {
        let mut args = vec!["review", "apply"];
        args.extend_from_slice(ids);
        args.extend_from_slice(&["--slug", SLUG]);
        args.extend_from_slice(flags);
        self.refused(&args)
    }

    fn doc(&self, rel: &str) -> toml::Value {
        toml::from_str(&fs::read_to_string(self.root.join(rel)).unwrap()).unwrap()
    }

    fn finding(&self, id: &str) -> toml::Value {
        self.doc(STORE)["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no finding {id}"))
            .clone()
    }

    fn status(&self, id: &str) -> String {
        self.finding(id)["status"].as_str().unwrap().to_string()
    }

    fn tier(&self, id: &str) -> String {
        self.finding(id)["tier"].as_str().unwrap().to_string()
    }

    fn spec(&self, task_ref: &str) -> JsonValue {
        let doc = self.doc(TASKS_FILE);
        let spec = doc["specs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["ref"].as_str() == Some(task_ref))
            .expect("the spec");
        serde_json::to_value(spec).unwrap()
    }

    fn row(&self, task_ref: &str) -> JsonValue {
        let doc = self.doc(TASKS_FILE);
        let row = doc["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["ref"].as_str() == Some(task_ref))
            .expect("the row");
        serde_json::to_value(row).unwrap()
    }

    /// The first accept entry of the task without its tool-written probe.
    fn accept_entry(&self, task_ref: &str) -> JsonValue {
        let mut entry = self.spec(task_ref)["accept"][0].clone();
        entry.as_object_mut().unwrap().remove("probe");
        entry
    }

    fn update(&self, row: &JsonValue) {
        let body = self.payload("update.ndjson", &format!("{row}\n"));
        self.ok(&["tasks", "update", "--slug", SLUG, "--ndjson", &body]);
    }

    /// Records a vacuous probe verdict on task 1's accept entry, and healthy
    /// ones on the falsifiers beside it.
    fn probe_task_one(&self) {
        let falsifier = digest("test -f src/a.rs");
        let capture = format!(
            "head\t{}\n1:0\tneg\tsha256={}\trc=0\t\t\n1:0\tpos\tsha256={falsifier}\trc=0\t\t\n\
             2:0\tneg\tsha256={falsifier}\trc=0\t\t\n3:0\tneg\tsha256={falsifier}\trc=0\t\t\n",
            "0".repeat(40),
            digest("test -f src/guard.rs"),
        );
        let path = self.payload("capture.tsv", &capture);
        self.ok(&["plan", "probes", "--slug", SLUG, "--record", &path]);
    }

    /// Adds an approval to the plan, as `plan approve` would leave one.
    fn approve(&self) {
        let path = self.root.join(PLAN_FILE);
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str(
            "\n[[approval]]\ncommit = \"abc1234\"\nat = 2026-11-05T12:00:00Z\n\
             brief_sha256 = \"feed\"\nby = \"user\"\nreview = \"skipped-low-risk\"\n\
             reason = \"One-file fix.\"\n",
        );
        fs::write(path, text).unwrap();
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

fn edit(
    target: &str,
    file: &str,
    symbol: &str,
    expect: Option<JsonValue>,
    new: JsonValue,
) -> JsonValue {
    let mut row = base("defect");
    row["target"] = json!(target);
    row["file"] = json!(file);
    row["symbol"] = json!(symbol);
    if let Some(expect) = expect {
        row["expect"] = expect;
    }
    row["new"] = new;
    row
}

fn task_edit(task_ref: &str, field: &str, expect: Option<JsonValue>, new: JsonValue) -> JsonValue {
    edit(
        &format!("task:{task_ref}.{field}"),
        TASKS_FILE,
        task_ref,
        expect,
        new,
    )
}

fn cite_tasks() -> String {
    format!("{TASKS_FILE}:1")
}

#[test]
fn a_run_started_with_rechecks_records_the_field() {
    if !git_available() {
        eprintln!("skipping: no git to resolve --base");
        return;
    }
    let flow = Flow::new();
    let git = |args: &[&str]| {
        let out = git_command(&flow.root).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["add", "-f", "."]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "base",
    ]);
    let started = flow.ok(&[
        "review",
        "run",
        "start",
        "--slug",
        SLUG,
        "--command",
        "review-plan-v2",
        "--family",
        "plan",
        "--depth",
        "quick",
        "--base",
        "HEAD",
        "--builtin",
        &flow.builtin,
        "--set",
        "rechecks=U1",
    ]);
    let id = started["id"]
        .as_str()
        .or_else(|| started["run"].as_str())
        .unwrap_or_else(|| panic!("no run id in {started}"))
        .to_string();
    let run = flow.doc(STORE)["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_str() == Some(id.as_str()))
        .cloned()
        .expect("the new run");
    assert_eq!(run["rechecks"].as_str(), Some("U1"), "{run:?}");
}

#[test]
fn mechanical_and_cited_edits_are_written_and_stamped() {
    let flow = Flow::new();
    flow.probe_task_one();
    let entry = flow.accept_entry("add-the-guard");
    assert!(
        flow.spec("add-the-guard")["accept"][0]
            .get("probe")
            .is_some(),
        "the fixture records a probe"
    );
    let mut fixed = entry.clone();
    fixed["check"] = json!("test -f src/guard2.rs");
    let mut probe_row = task_edit(
        "add-the-guard",
        "spec.accept[0]",
        Some(entry),
        fixed.clone(),
    );
    probe_row["proof"] = json!({"kind": "probe"});
    let mechanical = flow.opened(probe_row, "src/a.rs:1");
    assert_eq!(flow.tier(&mechanical), "mechanical");

    let files = flow.opened(
        task_edit(
            "wire-the-guard",
            "files",
            Some(json!(["src/a.rs"])),
            json!(["src/a.rs", "src/b.rs"]),
        ),
        &cite_tasks(),
    );
    assert_eq!(flow.tier(&files), "cited");
    let append = flow.opened(
        task_edit(
            "add-the-guard",
            "spec.constraints[+]",
            None,
            json!("Keep-it-small."),
        ),
        &cite_tasks(),
    );
    let removal = flow.opened(
        task_edit(
            "wire-the-guard",
            "spec.constraints[0]",
            Some(json!("Old-constraint.")),
            JsonValue::Null,
        ),
        &cite_tasks(),
    );

    let out = flow.apply(&[&mechanical, &files, &append, &removal], &[]);
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["already"], json!([]), "{out}");
    assert_eq!(out["skipped_stale"], json!([]), "{out}");
    assert_eq!(
        out["applied"],
        json!([
            {"id": mechanical, "target": "task:add-the-guard.spec.accept[0]", "tier": "mechanical"},
            {"id": files, "target": "task:wire-the-guard.files", "tier": "cited"},
            {"id": append, "target": "task:add-the-guard.spec.constraints[+]", "tier": "cited"},
            {"id": removal, "target": "task:wire-the-guard.spec.constraints[0]", "tier": "cited"},
        ]),
        "{out}"
    );

    let accept = &flow.spec("add-the-guard")["accept"][0];
    assert_eq!(accept["check"], "test -f src/guard2.rs", "{accept}");
    assert!(
        accept.get("probe").is_none(),
        "the edit drops the probe: {accept}"
    );
    assert_eq!(
        flow.row("wire-the-guard")["files"],
        json!(["src/a.rs", "src/b.rs"])
    );
    assert_eq!(
        flow.spec("add-the-guard")["constraints"],
        json!(["No-new-deps.", "Keep-it-small."])
    );
    assert_eq!(
        flow.spec("wire-the-guard")["constraints"],
        json!(["Other-constraint."])
    );
    assert!(
        flow.doc(TASKS_FILE).get("changes").is_none()
            || flow.doc(TASKS_FILE)["changes"]
                .as_array()
                .unwrap()
                .is_empty(),
        "an unaudited store keeps no change rows"
    );

    let stamped = flow.finding(&mechanical);
    assert_eq!(stamped["status"].as_str(), Some("resolved"));
    assert_eq!(
        stamped["resolution"].as_str(),
        Some("applied to task:add-the-guard.spec.accept[0]")
    );
    let applied = &stamped["applied"];
    assert_eq!(applied["tier"].as_str(), Some("mechanical"));
    assert_eq!(applied["by"].as_str(), Some("tool"));
    assert_eq!(applied["recovered"].as_bool(), Some(false));
    assert_eq!(
        serde_json::from_str::<JsonValue>(applied["new"].as_str().unwrap()).unwrap(),
        fixed
    );
    let appended = flow.finding(&append);
    assert_eq!(appended["applied"]["old"].as_str(), Some("null"));

    let again = flow.apply_refused(&[&files], &[]);
    assert!(again.contains("is resolved"), "{again}");
}

#[test]
fn advisory_and_user_tier_findings_are_refused_until_accepted() {
    let flow = Flow::new();
    let advisory = flow.opened(
        task_edit(
            "wire-the-guard",
            "action",
            Some(json!("Wire-action.")),
            json!("Wire-action-revised."),
        ),
        "src/a.rs:1",
    );
    assert_eq!(flow.tier(&advisory), "advisory");
    let message = flow.apply_refused(&[&advisory], &["--user-accepted"]);
    assert!(message.contains("advisory tier"), "{message}");

    let decision = flow.opened(
        edit(
            "decision:D1.why",
            PLAN_FILE,
            "D1",
            Some(json!("Guard-bound-why.")),
            json!("Guard-bound-why-revised."),
        ),
        &format!("{PLAN_FILE}:1"),
    );
    assert_eq!(flow.tier(&decision), "user");
    let message = flow.apply_refused(&[&decision], &[]);
    assert!(message.contains("--user-accepted"), "{message}");
    assert_eq!(flow.status(&decision), "open");

    let out = flow.apply(&[&decision], &["--user-accepted"]);
    assert_eq!(
        out["applied"],
        json!([{"id": decision, "target": "decision:D1.why", "tier": "user"}])
    );
    let applied = flow.finding(&decision)["applied"].clone();
    assert_eq!(applied["by"].as_str(), Some("user"), "{applied}");
    assert!(
        fs::read_to_string(flow.root.join(PLAN_FILE))
            .unwrap()
            .contains("Guard-bound-why-revised.")
    );
}

#[test]
fn a_drifted_expect_is_skipped_and_the_rest_proceed() {
    let flow = Flow::new();
    let stale = flow.opened(
        task_edit(
            "add-the-guard",
            "action",
            Some(json!("Guard-action.")),
            json!("Guard-action-revised."),
        ),
        &cite_tasks(),
    );
    let fresh = flow.opened(
        task_edit(
            "wire-the-guard",
            "action",
            Some(json!("Wire-action.")),
            json!("Wire-action-revised."),
        ),
        &cite_tasks(),
    );
    flow.update(&json!({"id": 1, "set": {"action": "Drifted-action."}}));

    let out = flow.apply(&[&stale, &fresh], &[]);
    assert_eq!(
        out["skipped_stale"],
        json!([{
            "id": stale,
            "target": "task:add-the-guard.action",
            "expected": "Guard-action.",
            "found": "Drifted-action.",
        }]),
        "{out}"
    );
    assert_eq!(out["applied"][0]["id"], fresh.as_str(), "{out}");
    assert_eq!(flow.status(&stale), "open", "a stale finding stays open");
    assert!(flow.finding(&stale).get("applied").is_none());
    assert_eq!(flow.row("add-the-guard")["action"], "Drifted-action.");
    assert_eq!(flow.row("wire-the-guard")["action"], "Wire-action-revised.");
}

#[test]
fn values_already_in_place_are_stamped_without_a_write() {
    let flow = Flow::new();
    let set = flow.opened(
        task_edit(
            "spare-work",
            "action",
            Some(json!("Spare-action.")),
            json!("Spare-action-revised."),
        ),
        &cite_tasks(),
    );
    let append = flow.opened(
        task_edit("spare-work", "spec.constraints[+]", None, json!("A-three.")),
        &cite_tasks(),
    );
    let removal = flow.opened(
        task_edit(
            "spare-work",
            "spec.constraints[0]",
            Some(json!("A-one.")),
            JsonValue::Null,
        ),
        &cite_tasks(),
    );
    flow.update(&json!({"id": 3, "set": {"action": "Spare-action-revised."}}));
    flow.update(&json!({"id": 3, "spec": {"constraints": ["A-two.", "A-three."]}}));
    let before = flow.spec("spare-work")["constraints"].clone();
    assert_eq!(before, json!(["A-two.", "A-three."]));

    let out = flow.apply(&[&set, &append, &removal], &[]);
    assert_eq!(out["applied"], json!([]), "{out}");
    assert_eq!(out["already"], json!([set, append, removal]), "{out}");
    assert_eq!(out["skipped_stale"], json!([]), "{out}");
    assert_eq!(
        flow.spec("spare-work")["constraints"],
        before,
        "an append or removal already made is not repeated"
    );
    for id in [&set, &append, &removal] {
        assert_eq!(flow.status(id), "resolved");
        let applied = flow.finding(id)["applied"].clone();
        assert_eq!(applied["recovered"].as_bool(), Some(true), "{applied}");
        assert_eq!(applied["by"].as_str(), Some("tool"), "{applied}");
    }
    let applied = flow.finding(&set)["applied"].clone();
    assert_eq!(applied["old"].as_str(), Some("\"Spare-action.\""));
    assert_eq!(applied["new"].as_str(), Some("\"Spare-action-revised.\""));
}

#[test]
fn a_new_the_target_rejects_refuses_the_whole_call_before_any_write() {
    let flow = Flow::new();
    let fine = flow.opened(
        task_edit(
            "wire-the-guard",
            "action",
            Some(json!("Wire-action.")),
            json!("Wire-action-revised."),
        ),
        &cite_tasks(),
    );
    // Task 2 already needs task 1, so task 1 needing task 2 closes a cycle.
    let cycle = flow.opened(
        task_edit("add-the-guard", "needs", Some(json!([])), json!([2])),
        &cite_tasks(),
    );
    let message = flow.apply_refused(&[&fine, &cycle], &[]);
    assert!(message.contains("1 of 2 ids refused"), "{message}");
    assert!(message.contains(&cycle), "{message}");
    assert_eq!(flow.status(&fine), "open");
    assert_eq!(flow.row("wire-the-guard")["action"], "Wire-action.");
}

#[test]
fn an_approved_flow_applies_only_what_the_user_accepts() {
    let flow = Flow::new();
    flow.probe_task_one();
    let entry = flow.accept_entry("add-the-guard");
    let mut fixed = entry.clone();
    fixed["check"] = json!("test -f src/guard2.rs");
    let mut row = task_edit("add-the-guard", "spec.accept[0]", Some(entry), fixed);
    row["proof"] = json!({"kind": "probe"});
    let mechanical = flow.opened(row, "src/a.rs:1");
    assert_eq!(flow.tier(&mechanical), "mechanical");
    flow.approve();

    let message = flow.apply_refused(&[&mechanical], &[]);
    assert!(message.contains("the plan is approved"), "{message}");
    assert!(message.contains("--user-accepted"), "{message}");
    assert_eq!(flow.status(&mechanical), "open");
}

#[test]
fn an_audited_store_gets_a_change_row_from_the_review_actor() {
    let flow = Flow::new();
    let path = flow.root.join(TASKS_FILE);
    let text = fs::read_to_string(&path).unwrap();
    let audited = if text.contains("audit = false") {
        text.replace("audit = false", "audit = true")
    } else {
        format!("audit = true\n{text}")
    };
    fs::write(&path, audited).unwrap();

    let id = flow.opened(
        task_edit(
            "wire-the-guard",
            "action",
            Some(json!("Wire-action.")),
            json!("Wire-action-revised."),
        ),
        &cite_tasks(),
    );
    flow.apply(&[&id], &[]);
    let doc = flow.doc(TASKS_FILE);
    let changes = doc["changes"].as_array().expect("a change row");
    assert_eq!(changes.len(), 1, "{changes:?}");
    let change = &changes[0];
    assert_eq!(change["ref"].as_str(), Some("wire-the-guard"));
    assert_eq!(change["field"].as_str(), Some("action"));
    assert_eq!(change["new"].as_str(), Some("Wire-action-revised."));
    assert_eq!(change["actor"].as_str(), Some("review"), "{change:?}");
    assert_eq!(
        change["reason"].as_str(),
        Some(format!("review U1 {id}").as_str()),
        "{change:?}"
    );
}

#[test]
fn a_brief_edit_replaces_text_once_and_an_ambiguous_one_is_refused() {
    let flow = Flow::new();
    let brief_cite = format!("{BRIEF_FILE}:1");
    let row = |expect: &str, new: &str| {
        edit(
            "brief#Background",
            BRIEF_FILE,
            "Background",
            Some(json!(expect)),
            json!(new),
        )
    };
    let edited = flow.opened(row("hit twice", "hit three times"), &brief_cite);
    let ambiguous = flow.opened(row("in one week", "within a week"), &brief_cite);

    let out = flow.apply(&[&edited], &[]);
    assert_eq!(
        out["applied"],
        json!([{"id": edited, "target": "brief#Background", "tier": "cited"}]),
        "{out}"
    );
    let brief = fs::read_to_string(flow.root.join(BRIEF_FILE)).unwrap();
    assert_eq!(brief, BRIEF.replace("hit twice", "hit three times"));

    // The phrase now occurs twice in the section.
    fs::write(
        flow.root.join(BRIEF_FILE),
        brief.replace(
            "hit three times in one week.",
            "hit three times in one week, and in one week more.",
        ),
    )
    .unwrap();
    let message = flow.apply_refused(&[&ambiguous], &[]);
    assert!(message.contains("occurs 2 times"), "{message}");
    assert_eq!(flow.status(&ambiguous), "open");
}

#[test]
fn a_dry_run_writes_nothing() {
    let flow = Flow::new();
    let id = flow.opened(
        task_edit(
            "wire-the-guard",
            "action",
            Some(json!("Wire-action.")),
            json!("Wire-action-revised."),
        ),
        &cite_tasks(),
    );
    let stale = flow.opened(
        task_edit(
            "add-the-guard",
            "action",
            Some(json!("Guard-action.")),
            json!("Guard-action-revised."),
        ),
        &cite_tasks(),
    );
    flow.update(&json!({"id": 1, "set": {"action": "Drifted-action."}}));
    let before = flow.snapshot();
    let sidecars: Vec<_> = [TASKS_FILE, PLAN_FILE, STORE]
        .iter()
        .map(|rel| fs::read(flow.root.join(format!("{rel}.sha256"))).ok())
        .collect();

    let out = flow.apply(&[&id, &stale], &["--dry-run"]);
    assert_eq!(out["dry_run"], true, "{out}");
    assert_eq!(out["applied"][0]["id"], id.as_str(), "{out}");
    assert_eq!(out["skipped_stale"][0]["id"], stale.as_str(), "{out}");
    assert_eq!(flow.snapshot(), before, "a dry run wrote");
    let after: Vec<_> = [TASKS_FILE, PLAN_FILE, STORE]
        .iter()
        .map(|rel| fs::read(flow.root.join(format!("{rel}.sha256"))).ok())
        .collect();
    assert_eq!(after, sidecars, "a dry run rewrote a sidecar");
    assert_eq!(flow.status(&id), "open");
}
