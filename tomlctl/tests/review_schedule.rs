//! `review schedule` against a sandbox seeded by hand: a v2 flow (a task
//! store beside a `plan.toml`) and a flow-less review store. Selected
//! findings become fix task rows linked through `[[finding_links]]`, and the
//! findings move to `scheduled`.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{TASKS_SLUG, cli, sandbox, stage_tasks_flow, stdout_json, write_tasks_store};
use serde_json::Value as JsonValue;
use toml::Value;

const SCOPE: &str = "t";
const REVIEW_STORE: &str = ".claude/review-v2/t.toml";

const HEADER: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-01T10:00:00Z"

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["src/**"]
concerns = ["correctness", "dry"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "done"
"#;

fn tasks_fixture(audit: bool) -> String {
    let (version, audit) = if audit {
        (2, "audit = true\n")
    } else {
        (1, "")
    };
    format!(
        r#"schema_version = {version}
last_updated = 2026-09-07
{audit}
[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "the fixes"

[[items]]
id = 1
ref = "seed-the-store"
title = "Seed the store"
effort = "S"
status = "done"
checkpoint = "A"
files = ["src/seed.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#
    )
}

struct Defect<'a> {
    id: &'a str,
    file: &'a str,
    symbol: &'a str,
    effort: &'a str,
    /// TOML lines naming what blocks the fix; empty for a fixable defect.
    blocked: &'a str,
    depends_on: &'a [&'a str],
}

fn defect(d: Defect<'_>) -> String {
    let deps: Vec<String> = d.depends_on.iter().map(|id| format!("{id:?}")).collect();
    format!(
        r#"
[[findings]]
id = "{id}"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "{file}"
line = 1
symbol = "{symbol}"
summary = "{symbol} is off by one"
severity = "major"
failure_scenario = "a two-row input returns one row"
fix = "count from zero"
{blocked}effort = "{effort}"
depends_on = [{deps}]
status = "open"
"#,
        id = d.id,
        file = d.file,
        symbol = d.symbol,
        effort = d.effort,
        blocked = d.blocked,
        deps = deps.join(", "),
    )
}

fn bug(id: &str, file: &str, symbol: &str) -> String {
    defect(Defect {
        id,
        file,
        symbol,
        effort: "small",
        blocked: "",
        depends_on: &[],
    })
}

fn cleanup(id: &str, file: &str, direction: &str, old: &str) -> String {
    format!(
        r#"
[[findings]]
id = "{id}"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "cleanup"
lens = "dry"
route = "checker"
file = "{file}"
line = 2
summary = "tidy the fast flag"
edit = "drop the fast flag"
direction = "{direction}"
old = {old:?}
effort = "trivial"
status = "open"
"#
    )
}

const ADVISORY: &str = r#"
[[findings]]
id = "F9"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "advisory"
lens = "correctness"
route = "none"
file = "src/a.rs"
summary = "consider a newtype"
status = "open"
"#;

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
    tasks: PathBuf,
}

impl Repo {
    fn new(findings: &[String]) -> Self {
        Self::with(findings, false, true)
    }

    fn with(findings: &[String], audit: bool, plan_store: bool) -> Self {
        let (dir, root) = sandbox();
        let tasks = stage_tasks_flow(
            &root,
            TASKS_SLUG,
            &format!("docs/plans/{TASKS_SLUG}.md"),
            Some(&tasks_fixture(audit)),
        );
        if plan_store {
            fs::write(tasks.parent().unwrap().join("plan.toml"), "").unwrap();
        }
        let store = root.join(REVIEW_STORE);
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(store, format!("{HEADER}{}", findings.concat())).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        for (file, symbol) in [
            ("src/a.rs", "alpha"),
            ("src/b.rs", "beta"),
            ("src/c.rs", "gamma"),
        ] {
            fs::write(
                root.join(file),
                format!("pub fn {symbol}() {{}}\nlet fast = \"--fast\";\n"),
            )
            .unwrap();
        }
        Self {
            _dir: dir,
            root,
            tasks,
        }
    }

    fn command(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "schedule"])
            .args(args)
            .args(["--scope", SCOPE, "--flow", TASKS_SLUG, "--checkpoint", "A"])
            .assert()
    }

    fn schedule(&self, args: &[&str]) -> JsonValue {
        stdout_json(&self.command(args).success())
    }

    fn tasks_doc(&self) -> Value {
        toml::from_str(&fs::read_to_string(&self.tasks).unwrap()).unwrap()
    }

    fn review_doc(&self) -> Value {
        toml::from_str(&fs::read_to_string(self.root.join(REVIEW_STORE)).unwrap()).unwrap()
    }

    fn review_bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(REVIEW_STORE)).unwrap()
    }

    /// Reopen a finding by hand, dropping its apply link, as a crash between
    /// the two writes or a later reopen would leave it.
    fn reopen(&self, id: &str) {
        let mut doc = self.review_doc();
        for f in doc["findings"].as_array_mut().unwrap() {
            if f["id"].as_str() == Some(id) {
                let table = f.as_table_mut().unwrap();
                table.insert("status".into(), Value::String("open".into()));
                table.remove("apply_flow");
                table.remove("apply_task");
            }
        }
        fs::write(self.root.join(REVIEW_STORE), toml::to_string(&doc).unwrap()).unwrap();
    }

    fn commit_task(&self, r#ref: &str) {
        let mut doc = self.tasks_doc();
        for row in doc["items"].as_array_mut().unwrap() {
            if row["ref"].as_str() == Some(r#ref) {
                let table = row.as_table_mut().unwrap();
                table.insert("status".into(), Value::String("done".into()));
                table.insert("commit".into(), Value::String("abc1234".into()));
            }
        }
        write_tasks_store(&self.tasks, &toml::to_string(&doc).unwrap());
    }
}

fn items(doc: &Value) -> Vec<Value> {
    doc["items"].as_array().cloned().unwrap_or_default()
}

fn row<'a>(doc: &'a Value, r#ref: &str) -> &'a Value {
    doc["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["ref"].as_str() == Some(r#ref))
        .unwrap_or_else(|| panic!("no row {ref}"))
}

fn spec<'a>(doc: &'a Value, r#ref: &str) -> &'a Value {
    doc["specs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["ref"].as_str() == Some(r#ref))
        .unwrap_or_else(|| panic!("no spec {ref}"))
}

fn link<'a>(doc: &'a Value, r#ref: &str) -> &'a Value {
    doc["finding_links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["ref"].as_str() == Some(r#ref))
        .unwrap_or_else(|| panic!("no link {ref}"))
}

fn finding<'a>(doc: &'a Value, id: &str) -> &'a Value {
    doc["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"].as_str() == Some(id))
        .unwrap()
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect()
}

fn scheduled_ref(out: &JsonValue, id: &str) -> String {
    out["scheduled"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["finding"] == id)
        .unwrap_or_else(|| panic!("{id} not scheduled: {out}"))["ref"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_cleanup_and_a_defect_become_designed_rows() {
    let repo = Repo::new(&[
        cleanup("F1", "src/a.rs", "remove", "let fast = \"--fast\";"),
        bug("F2", "src/b.rs", "beta"),
    ]);
    let out = repo.schedule(&["F1,F2"]);
    let cleanup_ref = scheduled_ref(&out, "F1");
    let defect_ref = scheduled_ref(&out, "F2");
    assert_ne!(cleanup_ref, defect_ref);

    let doc = repo.tasks_doc();
    assert_eq!(items(&doc).len(), 3);
    let c = row(&doc, &cleanup_ref);
    assert_eq!(strings(&c["files"]), ["src/a.rs"]);
    assert_eq!(c["checkpoint"].as_str(), Some("A"));
    assert_eq!(c["effort"].as_str(), Some("S"));
    assert_eq!(c["status"].as_str(), Some("pending"));
    let cs = spec(&doc, &cleanup_ref);
    assert_eq!(cs["executor"].as_str(), Some("lite"));
    assert_eq!(
        cs["premises"][0]["check"].as_str(),
        Some("tomlctl review anchor F1 --scope t")
    );
    assert_eq!(cs["premises"][0]["at"].as_str(), Some("base"));
    assert_eq!(
        cs["accept"][0]["check"].as_str(),
        Some("tomlctl review anchor F1 --scope t --gone")
    );
    assert_eq!(cs["accept"][0]["kind"].as_str(), Some("forward"));
    assert_eq!(cs["accept"][0]["writes_build"].as_bool(), Some(false));
    assert_eq!(
        strings(&cs["constraints"]),
        [
            "Touch no file outside this task's files.",
            "Change no behaviour the finding does not name."
        ]
    );

    let ds = spec(&doc, &defect_ref);
    assert_eq!(ds["executor"].as_str(), Some("lite"));
    assert_eq!(
        ds["accept"][0]["check"].as_str(),
        Some("tomlctl review reproduce-status F2 --scope t --expect gone")
    );
    assert_eq!(ds["accept"][0]["writes_build"].as_bool(), Some(true));
    assert!(
        ds["outcome"]
            .as_str()
            .unwrap()
            .contains("a two-row input returns one row")
    );
    assert!(
        row(&doc, &defect_ref)["detail"]
            .as_str()
            .unwrap()
            .contains("Fix: count from zero")
    );

    let l = link(&doc, &defect_ref);
    assert_eq!(l["store"].as_str(), Some(REVIEW_STORE));
    assert_eq!(strings(&l["fixes"]), ["F2"]);

    let review = repo.review_doc();
    for (id, r#ref) in [("F1", &cleanup_ref), ("F2", &defect_ref)] {
        let f = finding(&review, id);
        assert_eq!(f["status"].as_str(), Some("scheduled"));
        assert_eq!(f["apply_flow"].as_str(), Some(TASKS_SLUG));
        assert_eq!(f["apply_task"].as_str(), Some(r#ref.as_str()));
    }
}

#[test]
fn findings_sharing_a_file_fold_into_one_task() {
    let repo = Repo::new(&[
        bug("F1", "src/a.rs", "alpha"),
        cleanup("F2", "src/a.rs", "change", "let fast = \"--fast\";"),
        bug("F3", "src/c.rs", "gamma"),
    ]);
    let out = repo.schedule(&["F1", "F2", "F3"]);
    let folded = scheduled_ref(&out, "F1");
    assert_eq!(scheduled_ref(&out, "F2"), folded);
    assert_ne!(scheduled_ref(&out, "F3"), folded);

    let doc = repo.tasks_doc();
    assert_eq!(items(&doc).len(), 3);
    assert_eq!(strings(&link(&doc, &folded)["fixes"]), ["F1", "F2"]);
    let s = spec(&doc, &folded);
    assert_eq!(s["premises"].as_array().unwrap().len(), 2);
    assert_eq!(s["accept"].as_array().unwrap().len(), 2);
    assert!(
        row(&doc, &folded)["title"]
            .as_str()
            .unwrap()
            .ends_with("(and 1 more)")
    );
}

#[test]
fn a_medium_defect_or_a_third_file_runs_deep() {
    let repo = Repo::new(&[defect(Defect {
        id: "F1",
        file: "src/a.rs",
        symbol: "alpha",
        effort: "medium",
        blocked: "",
        depends_on: &[],
    })]);
    let out = repo.schedule(&["F1"]);
    let doc = repo.tasks_doc();
    let r#ref = scheduled_ref(&out, "F1");
    assert_eq!(spec(&doc, &r#ref)["executor"].as_str(), Some("deep"));
    assert_eq!(row(&doc, &r#ref)["effort"].as_str(), Some("M"));
}

#[test]
fn what_selection_refuses_is_refused_and_nothing_is_written() {
    let large = defect(Defect {
        id: "F2",
        file: "src/b.rs",
        symbol: "beta",
        effort: "large",
        blocked: "",
        depends_on: &[],
    });
    let blocked = defect(Defect {
        id: "F3",
        file: "src/c.rs",
        symbol: "gamma",
        effort: "small",
        blocked: "fix_shape = \"needs-plan\"\n",
        depends_on: &[],
    });
    let repo = Repo::new(&[ADVISORY.to_string(), large, blocked]);
    let tasks_before = fs::read(&repo.tasks).unwrap();
    let review_before = repo.review_bytes();
    let out = repo.command(&["F9,F2,F3,F404"]).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    for needle in [
        "advisory",
        "effort is large",
        "needs-plan",
        "no such finding",
    ] {
        assert!(stderr.contains(needle), "{needle}: {stderr}");
    }
    assert_eq!(fs::read(&repo.tasks).unwrap(), tasks_before);
    assert_eq!(repo.review_bytes(), review_before);

    let out = repo.schedule(&["--all"]);
    assert_eq!(out["scheduled"].as_array().unwrap().len(), 0);
    let refused: Vec<&str> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["finding"].as_str().unwrap())
        .collect();
    assert!(refused.contains(&"F2"), "{out}");
    assert!(refused.contains(&"F3"), "{out}");
    assert_eq!(items(&repo.tasks_doc()).len(), 1);
}

#[test]
fn emitted_checks_parse() {
    let repo = Repo::new(&[
        cleanup("F1", "src/a.rs", "remove", "let fast = \"--fast\";"),
        cleanup("F2", "src/b.rs", "change", "let fast = \"--fast\";"),
        bug("F3", "src/c.rs", "gamma"),
    ]);
    repo.schedule(&["F1,F2,F3"]);
    let doc = repo.tasks_doc();
    let mut checks = Vec::new();
    for s in doc["specs"].as_array().unwrap() {
        for key in ["premises", "accept"] {
            for entry in s[key].as_array().unwrap() {
                checks.push(entry["check"].as_str().unwrap().to_string());
            }
        }
    }
    assert_eq!(checks.len(), 6);
    for check in checks {
        let words: Vec<&str> = check.split_whitespace().collect();
        assert_eq!(words[0], "tomlctl", "{check}");
        let out = cli(&repo.root)
            .current_dir(&repo.root)
            .args(&words[1..])
            .output()
            .unwrap();
        let code = out.status.code();
        assert!(matches!(code, Some(0 | 1)), "{check} exited {code:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let json: JsonValue = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("{check} printed no JSON ({e}): {stdout}"));
        assert!(json["ok"].is_boolean(), "{check}: {json}");
    }
}

#[test]
fn a_v1_flow_with_no_plan_store_is_refused() {
    let repo = Repo::with(&[bug("F1", "src/a.rs", "alpha")], false, false);
    let tasks_before = fs::read(&repo.tasks).unwrap();
    let out = repo.command(&["F1"]).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("plan store"), "{stderr}");
    assert_eq!(fs::read(&repo.tasks).unwrap(), tasks_before);
    assert_eq!(
        finding(&repo.review_doc(), "F1")["status"].as_str(),
        Some("open")
    );
}

#[test]
fn an_approved_reason_is_stamped_on_every_row() {
    let repo = Repo::with(
        &[
            bug("F1", "src/a.rs", "alpha"),
            bug("F2", "src/b.rs", "beta"),
        ],
        true,
        true,
    );
    let out = repo.schedule(&["F1,F2"]);
    let doc = repo.tasks_doc();
    let changes = doc["changes"].as_array().unwrap();
    for id in ["F1", "F2"] {
        let r#ref = scheduled_ref(&out, id);
        let change = changes
            .iter()
            .find(|c| c["ref"].as_str() == Some(r#ref.as_str()))
            .unwrap_or_else(|| panic!("no change for {ref}"));
        assert_eq!(
            change["reason"].as_str(),
            Some(format!("apply {REVIEW_STORE}: {id}").as_str())
        );
    }
}

#[test]
fn depends_on_becomes_needs() {
    let repo = Repo::new(&[
        bug("F1", "src/a.rs", "alpha"),
        defect(Defect {
            id: "F2",
            file: "src/b.rs",
            symbol: "beta",
            effort: "small",
            blocked: "",
            depends_on: &["F1"],
        }),
    ]);
    let out = repo.schedule(&["F2,F1"]);
    let doc = repo.tasks_doc();
    let first = row(&doc, &scheduled_ref(&out, "F1"))["id"]
        .as_integer()
        .unwrap();
    let needs = row(&doc, &scheduled_ref(&out, "F2"))["needs"].clone();
    assert_eq!(needs.as_array().unwrap(), &[Value::Integer(first)]);
}

#[test]
fn a_rerun_after_the_task_landed_marks_without_minting() {
    let repo = Repo::new(&[bug("F1", "src/a.rs", "alpha")]);
    let first = repo.schedule(&["F1"]);
    let r#ref = scheduled_ref(&first, "F1");
    repo.reopen("F1");

    let again = repo.schedule(&["F1"]);
    assert_eq!(scheduled_ref(&again, "F1"), r#ref);
    assert_eq!(again["scheduled"][0]["action"], "linked");
    assert_eq!(items(&repo.tasks_doc()).len(), 2);
    let f = finding(&repo.review_doc(), "F1").clone();
    assert_eq!(f["status"].as_str(), Some("scheduled"));
    assert_eq!(f["apply_task"].as_str(), Some(r#ref.as_str()));
}

const EVIDENCE: &str =
    "blocked_on = \"evidence\"\nblocked_missing = \"whether any caller passes two rows\"\n";

fn evidence_bug(id: &str, blocked: &str) -> String {
    defect(Defect {
        id,
        file: "src/a.rs",
        symbol: "alpha",
        effort: "small",
        blocked,
        depends_on: &[],
    })
}

fn settle_ref(out: &JsonValue, id: &str) -> String {
    out["settle"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["finding"] == id)
        .unwrap_or_else(|| panic!("{id} not settled: {out}"))["ref"]
        .as_str()
        .unwrap()
        .to_string()
}

fn settle_note(id: &str) -> String {
    format!("docs/flows/{TASKS_SLUG}/notes/settle/{id}.md")
}

#[test]
fn an_evidence_blocked_finding_becomes_one_settle_task() {
    let repo = Repo::new(&[evidence_bug("F1", EVIDENCE), bug("F2", "src/b.rs", "beta")]);
    let out = repo.schedule(&["F1,F2", "--settle-checkpoint", "A"]);
    let r#ref = settle_ref(&out, "F1");
    assert_eq!(out["settle"].as_array().unwrap().len(), 1, "{out}");
    assert_eq!(out["scheduled"].as_array().unwrap().len(), 1, "{out}");
    assert_ne!(scheduled_ref(&out, "F2"), r#ref);

    let doc = repo.tasks_doc();
    assert_eq!(items(&doc).len(), 3);
    let note = settle_note("F1");
    let r = row(&doc, &r#ref);
    assert_eq!(strings(&r["files"]), [note.as_str()]);
    assert_eq!(r["checkpoint"].as_str(), Some("A"));
    let file_note = doc["file_notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["ref"].as_str() == Some(r#ref.as_str()))
        .unwrap_or_else(|| panic!("no file note for {ref}"));
    assert_eq!(file_note["file"].as_str(), Some(note.as_str()));
    assert_eq!(file_note["note"].as_str(), Some("(new)"));

    let s = spec(&doc, &r#ref);
    assert_eq!(s["executor"].as_str(), Some("deep"));
    let outcome = s["outcome"].as_str().unwrap();
    assert!(outcome.contains("Proposal:"), "{outcome}");
    assert!(outcome.contains("Refuted:"), "{outcome}");
    assert!(
        outcome.contains("whether any caller passes two rows"),
        "{outcome}"
    );
    assert_eq!(
        s["accept"][0]["check"].as_str(),
        Some(format!("head -n 1 {note} | grep -Eq '^(Proposal|Refuted):'").as_str())
    );
    assert_eq!(s["accept"][0]["kind"].as_str(), Some("forward"));
    assert_eq!(s["accept"][0]["writes_build"].as_bool(), Some(false));

    let l = link(&doc, &r#ref);
    assert_eq!(l["store"].as_str(), Some(REVIEW_STORE));
    assert_eq!(strings(&l["settles"]), ["F1"]);

    let f = finding(&repo.review_doc(), "F1").clone();
    assert_eq!(f["status"].as_str(), Some("scheduled"));
    assert_eq!(f["apply_task"].as_str(), Some(r#ref.as_str()));
}

#[test]
fn an_evidence_blocked_finding_is_refused_without_a_settle_checkpoint() {
    let repo = Repo::new(&[evidence_bug("F1", EVIDENCE)]);
    let tasks_before = fs::read(&repo.tasks).unwrap();
    let out = repo.command(&["F1"]).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("--settle-checkpoint"), "{stderr}");
    assert_eq!(fs::read(&repo.tasks).unwrap(), tasks_before);

    let out = repo.schedule(&["--all"]);
    assert_eq!(out["settle"].as_array().unwrap().len(), 0, "{out}");
    assert_eq!(out["refused"][0]["finding"], "F1", "{out}");
    assert_eq!(items(&repo.tasks_doc()).len(), 1);
}

#[test]
fn a_finding_with_a_proposal_gets_no_second_settle_task() {
    let blocked = format!(
        "{EVIDENCE}proposal = \"docs/flows/{TASKS_SLUG}/notes/settle/F1.md\"\n\
         proposal_at = 2026-10-02T10:00:00Z\n"
    );
    let repo = Repo::new(&[evidence_bug("F1", &blocked)]);
    let tasks_before = fs::read(&repo.tasks).unwrap();
    let out = repo.command(&["F1", "--settle-checkpoint", "A"]).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("one settle round"), "{stderr}");
    assert_eq!(fs::read(&repo.tasks).unwrap(), tasks_before);
}

#[test]
fn a_settle_rerun_links_the_live_settle_task() {
    let repo = Repo::new(&[evidence_bug("F1", EVIDENCE)]);
    let first = repo.schedule(&["F1", "--settle-checkpoint", "A"]);
    let r#ref = settle_ref(&first, "F1");
    repo.reopen("F1");

    let again = repo.schedule(&["--all", "--settle-checkpoint", "A"]);
    assert_eq!(settle_ref(&again, "F1"), r#ref);
    assert_eq!(again["settle"][0]["action"], "linked");
    assert_eq!(items(&repo.tasks_doc()).len(), 2);
    assert_eq!(
        finding(&repo.review_doc(), "F1")["status"].as_str(),
        Some("scheduled")
    );
}

#[test]
fn a_settle_flag_leaves_a_measurement_block_waiting_for_trials() {
    let blocked = "blocked_on = \"measurement\"\nblocked_missing = \"the hot path's cost\"\n\
                   blocked_metric = \"ns per row\"\nblocked_command = \"cargo bench\"\n";
    let repo = Repo::new(&[evidence_bug("F1", blocked)]);
    let out = repo.command(&["F1", "--settle-checkpoint", "A"]).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("waits for trials"), "{stderr}");
    assert_eq!(items(&repo.tasks_doc()).len(), 1);
}

#[test]
fn a_settled_decision_answer_binds_the_fix_task() {
    let answered = "decision_answer = \"keep the cache per process\"\n\
                    fix_sketch = \"key the cache by pid\"\n";
    let repo = Repo::new(&[evidence_bug("F1", answered)]);
    let out = repo.schedule(&["F1"]);
    let r#ref = scheduled_ref(&out, "F1");
    let doc = repo.tasks_doc();
    let constraints = strings(&spec(&doc, &r#ref)["constraints"])
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert!(
        constraints.contains(&"Decision: keep the cache per process".to_string()),
        "{constraints:?}"
    );
    assert!(
        row(&doc, &r#ref)["detail"]
            .as_str()
            .unwrap()
            .contains("Fix sketch: key the cache by pid")
    );
}

#[test]
fn a_rerun_over_a_committed_task_mints_a_new_one() {
    let repo = Repo::new(&[bug("F1", "src/a.rs", "alpha")]);
    let first = repo.schedule(&["F1"]);
    let old = scheduled_ref(&first, "F1");
    repo.commit_task(&old);
    repo.reopen("F1");

    let again = repo.schedule(&["F1"]);
    let new = scheduled_ref(&again, "F1");
    assert_ne!(new, old);
    assert_eq!(again["scheduled"][0]["action"], "minted");
    assert_eq!(items(&repo.tasks_doc()).len(), 3);
    let f = finding(&repo.review_doc(), "F1").clone();
    assert_eq!(f["apply_task"].as_str(), Some(new.as_str()));
    assert_eq!(f["apply_task_id"].as_integer(), Some(3));
}

#[test]
fn a_dry_run_reports_the_rows_and_writes_nothing() {
    let large = defect(Defect {
        id: "F3",
        file: "src/c.rs",
        symbol: "gamma",
        effort: "large",
        blocked: "",
        depends_on: &[],
    });
    let repo = Repo::new(&[
        evidence_bug("F1", EVIDENCE),
        bug("F2", "src/b.rs", "beta"),
        large,
    ]);
    let tasks_before = fs::read(&repo.tasks).unwrap();
    let review_before = repo.review_bytes();

    let out = repo.schedule(&["F1,F2,F3", "--dry-run"]);
    assert_eq!(out["dry_run"].as_bool(), Some(true), "{out}");
    let row = &out["scheduled"][0];
    assert_eq!(row["finding"], "F2", "{out}");
    assert_eq!(row["action"], "would-mint", "{out}");
    assert_eq!(row["task"], 2, "{out}");
    assert_eq!(row["executor"], "lite", "{out}");
    assert_eq!(row["files"], serde_json::json!(["src/b.rs"]), "{out}");
    assert_eq!(out["settle"][0]["finding"], "F1", "{out}");
    assert_eq!(out["settle"][0]["task"], 3, "{out}");
    assert_eq!(out["refused"][0]["finding"], "F3", "{out}");
    assert_eq!(fs::read(&repo.tasks).unwrap(), tasks_before);
    assert_eq!(repo.review_bytes(), review_before);

    let out = repo.schedule(&["F3", "--dry-run"]);
    assert_eq!(out["scheduled"].as_array().unwrap().len(), 0, "{out}");
    assert_eq!(out["refused"][0]["finding"], "F3", "{out}");
}

#[test]
fn a_dry_run_needs_no_flow() {
    let repo = Repo::new(&[bug("F1", "src/a.rs", "alpha")]);
    let review_before = repo.review_bytes();
    let out = cli(&repo.root)
        .current_dir(&repo.root)
        .args(["review", "schedule", "--all", "--scope", SCOPE, "--dry-run"])
        .assert()
        .success();
    let out = stdout_json(&out);
    assert_eq!(out["scheduled"][0]["finding"], "F1", "{out}");
    assert_eq!(out["scheduled"][0]["task"], 1, "{out}");
    assert_eq!(repo.review_bytes(), review_before);

    cli(&repo.root)
        .current_dir(&repo.root)
        .args(["review", "schedule", "--all", "--scope", SCOPE])
        .assert()
        .failure();
}
