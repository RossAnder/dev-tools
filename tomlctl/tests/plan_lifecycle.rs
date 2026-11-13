//! One plan driven through the binary from `plan open` to `plan drift` in a
//! throwaway repository, in the order a carrier would call the verbs, so each
//! verb reads what the verbs before it actually wrote.

use assert_cmd::Command;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

use crate::common;
use common::{
    assert_sidecar_matches, git_bash, git_command, parse_json_error_envelope, sandbox, store_path,
};

const SLUG: &str = "plan-lifecycle";
const BACKLOG_ID: &str = "B-0aaaaaaa";

const BACKLOG: &str = r#"schema_version = 1
last_updated = 2026-08-01

[[backlog]]
id = "B-0aaaaaaa"
kind = "bug"
summary = "The retry storm"
context = "Seen under load."
area = "src"
status = "open"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
"#;

const DECISIONS: &str = concat!(
    r#"{"decision":"Guard-bound-decision.","why":"Guard-bound-why.","provenance":"agent-assumed","door":"one-way","binds":["1"],"surfaced":true}"#,
    "\n",
    r#"{"decision":"Path-bound-decision.","provenance":"agent-assumed","door":"two-way","binds":["src/a.rs"],"revisit_if":"It widens."}"#,
    "\n",
);

const CONTRACT: &str =
    r#"{"kind":"invariant","claim":"Both-agree-claim.","check":"test -f src/a.rs","tasks":[1,2]}"#;

/// Task 4 starts with no spec, so the store passes through the `write` stage.
const TASK_RECORDS: &str = r#"[[record]]
title = "Add the guard"
effort = "S"
checkpoint = "A"
files = ["src/guard.rs", "src/a.rs"]
closes = ["B-0aaaaaaa"]

[record.file_notes]
"src/guard.rs" = "(new)"

[record.spec]
outcome = "Guard-outcome."
executor = "lite"

[[record.spec.accept]]
behaviour = "the guard exists"
check = "test -f src/guard.rs"
positive = "test -f src/a.rs"
kind = "forward"
writes_build = false

[[record.spec.premises]]
claim = "Premise-a-exists."
check = "test -f src/a.rs"
at = "base"

[[record]]
title = "Wire the guard"
effort = "S"
checkpoint = "A"
needs = [1]
files = ["src/b.rs"]

[record.spec]
outcome = "Wired-outcome."
executor = "deep"

[[record.spec.accept]]
behaviour = "the caller is there"
check = "test -f src/b.rs"
kind = "falsifier"
writes_build = false

[[record]]
title = "Extend the guard"
effort = "S"
checkpoint = "B"
files = ["src/c.rs"]

[record.spec]
outcome = "Extended-outcome."

[[record.spec.accept]]
behaviour = "the extension holds"

[[record]]
title = "Spare work"
effort = "S"
checkpoint = "B"
files = ["src/d.rs"]
"#;

struct Flow {
    _dir: tempfile::TempDir,
    root: PathBuf,
    scratch: tempfile::TempDir,
}

impl Flow {
    fn new() -> Self {
        let (dir, root) = sandbox();
        fs::write(store_path(&root), BACKLOG).expect("the backlog");
        fs::create_dir_all(root.join("src")).expect("src");
        for name in ["a", "b", "c", "d"] {
            fs::write(
                root.join("src").join(format!("{name}.rs")),
                "fn base() {}\n",
            )
            .expect("a source file");
        }
        Self {
            _dir: dir,
            root,
            scratch: tempfile::tempdir().expect("scratch"),
        }
    }

    fn tomlctl(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("tomlctl").expect("the binary builds");
        cmd.env("TOMLCTL_ROOT", &self.root)
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args(["--error-format", "json"])
            .args(args)
            .write_stdin("");
        cmd
    }

    /// Stdout of a call that must succeed, naming the call when it does not.
    fn text(&self, args: &[&str]) -> String {
        let out = self.tomlctl(args).output().expect("the binary runs");
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(
            out.status.success(),
            "`{}` failed\nstdout: {stdout}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        stdout
    }

    fn ok(&self, args: &[&str]) -> Value {
        let stdout = self.text(args);
        serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("`{}`: {e}; stdout:\n{stdout}", args.join(" ")))
    }

    /// The refusal's kind and message, having checked that it wrote nothing.
    fn refused(&self, args: &[&str]) -> (String, String) {
        let before = self.stores();
        let assert = self.tomlctl(args).assert().failure();
        assert_eq!(self.stores(), before, "`{}` wrote", args.join(" "));
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        let error = parse_json_error_envelope(&stderr);
        (
            error["kind"].as_str().unwrap_or_default().to_string(),
            error["message"].as_str().unwrap_or_default().to_string(),
        )
    }

    fn stores(&self) -> Vec<Option<Vec<u8>>> {
        [
            self.flow_path("plan.toml"),
            self.flow_path("tasks.toml"),
            self.flow_path("context.toml"),
            self.registry(),
            store_path(&self.root),
        ]
        .iter()
        .map(|path| fs::read(path).ok())
        .collect()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = git_command(&self.root)
            .args(["-c", "core.autocrlf=false"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-f", "."]);
        self.git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            message,
        ]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn authored_path(&self, name: &str) -> PathBuf {
        self.root.join("docs").join("flows").join(SLUG).join(name)
    }

    fn flow_path(&self, name: &str) -> PathBuf {
        self.root
            .join(".claude")
            .join("flows")
            .join(SLUG)
            .join(name)
    }

    fn registry(&self) -> PathBuf {
        self.root.join(".claude").join("active-flow.toml")
    }

    fn doc(&self, path: &Path) -> toml::Value {
        toml::from_str(&fs::read_to_string(path).expect("readable")).expect("valid TOML")
    }

    /// A payload file outside the root, as the path a `--ndjson` flag takes.
    fn payload(&self, name: &str, body: &str) -> String {
        let path = self.scratch.path().join(name);
        fs::write(&path, body).expect("a payload file");
        path.to_str().expect("a UTF-8 path").to_string()
    }

    fn plan(&self, verb: &[&str]) -> Value {
        let mut args = vec!["plan"];
        args.extend_from_slice(verb);
        args.extend_from_slice(&["--slug", SLUG]);
        self.ok(&args)
    }

    fn view(&self, audience: &[&str]) -> String {
        let mut args = vec!["plan", "view", "--slug", SLUG, "--for"];
        args.extend_from_slice(audience);
        self.text(&args)
    }

    fn update(&self, row: &Value) -> Value {
        let body = self.payload("update.ndjson", &format!("{row}\n"));
        self.ok(&["tasks", "update", "--slug", SLUG, "--ndjson", &body])
    }

    fn resolve_path(&self) -> Value {
        self.ok(&["flow", "resolve", "--path", "src/a.rs"])
    }

    #[track_caller]
    fn assert_stage(&self, stage: &str, next: &str) -> Value {
        let report = self.plan(&["stage"]);
        assert_eq!(report["stage"], stage, "{report}");
        assert_eq!(report["next"], next, "{report}");
        report
    }

    /// The capture of the emitted script, or the one it would print when this
    /// machine has no bash to run it with.
    fn run_probes(&self, script: &str, head: &str) -> String {
        let Some(bash) = git_bash() else {
            eprintln!("skipping the probe script: no bash found");
            return format!(
                "head\t{head}\n\
                 1:0\tneg\tsha256=ef603698972a5dc824b5864d95804c1ad92413cca0f6cf8be393d7155faf896f\trc=1\t\t\n\
                 1:0\tpos\tsha256=252dd783619f56e288730573bac5c85965cb66c66303d0b5d7debe2f5151a3d5\trc=0\t\t\n\
                 2:0\tneg\tsha256=bc4424d58aeb860208c996eaec8def374f78362a5ae901676c85e34c79261682\trc=0\t\t\n"
            );
        };
        let path = self.payload("probes.sh", script).replace('\\', "/");
        let out = std::process::Command::new(bash)
            .arg(path.trim_start_matches("//?/"))
            .current_dir(&self.root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("bash runs");
        assert!(
            out.status.success(),
            "the probe script failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }
}

fn blockers(report: &Value) -> String {
    report["blockers"].to_string()
}

fn finding_classes(report: &Value) -> Vec<&str> {
    report["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|finding| finding["class"].as_str().expect("a class"))
        .collect()
}

fn array<'a>(doc: &'a toml::Value, key: &str) -> &'a [toml::Value] {
    doc.get(key)
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

fn affected_ids(report: &Value) -> Vec<u64> {
    report["affected"]
        .as_array()
        .expect("affected")
        .iter()
        .map(|row| row["id"].as_u64().expect("an id"))
        .collect()
}

#[test]
fn a_plan_runs_from_open_to_drift() {
    let flow = Flow::new();
    let base = flow.commit("base");

    // Open: a draft holds its binding back from every automatic resolver.
    let opened = flow.plan(&[
        "open",
        "--from",
        BACKLOG_ID,
        "--branch",
        "feat/retry",
        "--scope",
        "src/**",
    ]);
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(flow.resolve_path()["resolved"], false);
    assert!(
        flow.ok(&["flow", "list", "--status", "draft"])["flows"][0]["slug"] == SLUG,
        "a draft stays listed"
    );
    let brief = fs::read_to_string(flow.authored_path("brief.md")).expect("the brief");
    assert!(brief.contains("The retry storm"), "{brief}");
    assert!(brief.contains("Seen under load."), "{brief}");
    assert!(
        flow.authored_path(&format!("notes/backlog-{BACKLOG_ID}.md"))
            .is_file()
    );
    let report = flow.assert_stage("open", "edit docs/flows/plan-lifecycle/brief.md");
    assert!(blockers(&report).contains("Non-goals"), "{report}");

    let filled = brief
        .replace("## Non-goals\n\n", "## Non-goals\n\nNo-new-helper.\n\n")
        .replace("## Scope budget\n\n", "## Scope budget\n\nFour files.\n\n")
        .replace(
            "## Success criteria\n\n",
            "## Success criteria\n\nA retry waits.\n\n",
        );
    fs::write(flow.authored_path("brief.md"), filled).expect("the brief");
    let report = flow.assert_stage("design", "plan milestone add");
    assert!(blockers(&report).contains("no milestones"), "{report}");

    // Records.
    let decisions = flow.payload("decisions.ndjson", DECISIONS);
    assert_eq!(
        flow.plan(&["decide", "--ndjson", &decisions])["ids"],
        json!(["D1", "D2"])
    );
    let contract = flow.payload("contract.ndjson", &format!("{CONTRACT}\n"));
    assert_eq!(
        flow.plan(&["contract", "--ndjson", &contract])["ids"],
        json!(["C1"])
    );
    let handover_rows = flow.payload(
        "handover.ndjson",
        "{\"reason\":\"restart\",\"action\":\"Restart open sessions.\",\"when\":\"After the run.\",\"done_when\":\"The new command is listed.\",\"source_task\":1}\n",
    );
    let handover = flow.plan(&["handover", "set", "--ndjson", &handover_rows]);
    assert_eq!(handover["ids"], json!(["H1"]));
    flow.plan(&[
        "run",
        "start",
        "--step",
        "investigate",
        "--workflow",
        "plan-v2-investigate",
        "--args-hash",
        "h",
    ]);
    flow.plan(&["run", "finish", "--step", "investigate", "--run-id", "wf_1"]);
    let plan = flow.doc(&flow.flow_path("plan.toml"));
    let run = &array(&plan, "run")[0];
    assert_eq!(run["run_id"].as_str(), Some("wf_1"), "{run:?}");
    assert!(
        run.get("finished")
            .is_some_and(|at| at.as_str() != Some("")),
        "{run:?}"
    );

    // Milestones and tasks.
    flow.plan(&[
        "milestone",
        "add",
        "--id",
        "A",
        "--goal",
        "The guard",
        "--gate",
        "cargo test",
    ]);
    flow.assert_stage("design", "tasks add-many");
    flow.plan(&[
        "milestone",
        "add",
        "--id",
        "B",
        "--goal",
        "The extension",
        "--defer",
        "--reason",
        "Depends on A.",
    ]);
    let records = flow.payload("tasks.toml", TASK_RECORDS);
    flow.ok(&["tasks", "add-many", "--slug", SLUG, "--ndjson", &records]);
    let store = flow.doc(&flow.flow_path("tasks.toml"));
    let notes = array(&store, "file_notes");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0]["file"].as_str(), Some("src/guard.rs"));
    assert_eq!(notes[0]["note"].as_str(), Some("(new)"));
    assert_eq!(
        array(&store, "backlog_links")[0]["closes"][0].as_str(),
        Some(BACKLOG_ID)
    );
    let report = flow.assert_stage("write", "tasks update --ndjson");
    assert!(blockers(&report).contains("plan/task-no-spec"), "{report}");

    flow.update(&json!({
        "id": 4,
        "spec": { "outcome": "Spare-outcome.", "accept": [{ "behaviour": "the spare holds" }] },
    }));
    let report = flow.assert_stage("prove", "plan decide");
    assert!(blockers(&report).contains("plan/probe-absent"), "{report}");
    assert!(blockers(&report).contains("unanswered: D1"), "{report}");

    // Approve is refused until the surfaced decision is answered, which is what the
    // stage asks for ahead of the probes.
    let approve = [
        "plan",
        "approve",
        "--slug",
        SLUG,
        "--review",
        "skipped-low-risk",
        "--reason",
        "Low risk.",
    ];
    let (kind, message) = flow.refused(&approve);
    assert_eq!(kind, "validation", "{message}");
    assert!(message.contains("D1"), "{message}");
    let answer = flow.payload(
        "answer.ndjson",
        "{\"id\":\"D1\",\"provenance\":\"user-confirmed\"}\n",
    );
    flow.plan(&["decide", "--ndjson", &answer]);
    flow.assert_stage("prove", "plan probes --emit");

    // Prove.
    let script = flow.text(&["plan", "probes", "--slug", SLUG, "--emit"]);
    assert!(script.contains("# skipped 3:0: empty check"), "{script}");
    let capture = flow.run_probes(&script, &base);
    assert_eq!(capture.lines().next(), Some(&*format!("head\t{base}")));
    let capture = flow.payload("capture.tsv", &capture);
    assert_eq!(
        flow.plan(&["probes", "--record", &capture])["recorded"],
        json!([
            { "task": 1, "accept": 0, "verdict": "healthy", "reason": "" },
            { "task": 2, "accept": 0, "verdict": "healthy", "reason": "" },
        ])
    );
    let store = flow.doc(&flow.flow_path("tasks.toml"));
    assert_eq!(
        array(&store, "specs")[0]["accept"][0]["probe"]["at"].as_str(),
        Some(base.as_str())
    );
    let report = flow.assert_stage(
        "review",
        "the /plan-v2 Review step, or plan run skip --step review --reason <why>",
    );
    assert!(blockers(&report).contains("plan review"), "{report}");

    flow.plan(&["run", "skip", "--step", "review", "--reason", "Low risk."]);
    flow.assert_stage("ready", "plan approve");

    let check = flow.plan(&["check"]);
    assert_eq!(check["ok"], true, "{check}");
    assert_eq!(
        finding_classes(&check),
        ["backlog/unpromoted", "plan/milestone-no-gate"],
        "{check}"
    );

    // Views.
    let human = flow.view(&["human"]);
    for needle in [
        "Add the guard",
        "Guard-bound-decision.",
        "Both-agree-claim.",
        "Depends on A.",
        "Restart open sessions.",
    ] {
        assert!(human.contains(needle), "{needle} missing:\n{human}");
    }
    let digest: Value = serde_json::from_str(&flow.view(&["digest"])).expect("a JSON digest");
    assert_eq!(digest["counts"]["tasks"]["total"], 4, "{digest}");
    assert_eq!(digest["counts"]["specs"], 4, "{digest}");
    assert_eq!(digest["counts"]["decisions"]["surfaced"], 1, "{digest}");
    assert_eq!(digest["counts"]["contracts"], 1, "{digest}");
    assert_eq!(digest["stage_inputs"]["check"]["errors"], 0, "{digest}");
    let worker = flow.view(&["worker", "--task", "1"]);
    for needle in [
        "The retry storm",
        "No-new-helper.",
        "Guard-bound-why.",
        "Path-bound-decision.",
        "Both-agree-claim.",
        "Guard-outcome.",
        "test -f src/guard.rs",
    ] {
        assert!(worker.contains(needle), "{needle} missing:\n{worker}");
    }
    assert!(!worker.contains("Wired-outcome."), "{worker}");
    assert!(!worker.contains("A retry waits."), "{worker}");
    let other = flow.view(&["worker", "--task", "3"]);
    assert!(other.contains("Extended-outcome."), "{other}");
    assert!(!other.contains("Guard-bound-why."), "{other}");
    assert!(!other.contains("Both-agree-claim."), "{other}");

    // Gate.
    let gate = flow.plan(&["gate"]);
    assert_eq!(gate["assumptions"][0]["revisit_if"], "It widens.", "{gate}");
    assert_eq!(gate["one_way_doors"][0]["id"], "D1", "{gate}");
    assert_eq!(
        gate["risks"],
        json!([{ "kind": "skip-reason", "detail": "Low risk." }])
    );
    assert_eq!(
        gate["deferred_milestones"],
        json!([{ "id": "B", "reason": "Depends on A." }])
    );
    assert_eq!(
        gate["stops"],
        json!([{ "kind": "gate", "milestone": "A", "command": "cargo test" }])
    );
    assert_eq!(
        gate["handovers"],
        json!([{
            "id": "H1",
            "reason": "restart",
            "action": "Restart open sessions.",
            "when": "After the run.",
            "done_when": "The new command is listed.",
            "source_task": 1,
        }])
    );
    assert_eq!(
        gate["task_mix"],
        json!([
            { "files": "1", "executor": "", "tasks": 2 },
            { "files": "1", "executor": "deep", "tasks": 1 },
            { "files": "2-3", "executor": "lite", "tasks": 1 },
        ])
    );

    // Approve: refused until the flow is committed.
    let (kind, message) = flow.refused(&approve);
    assert_eq!(kind, "validation", "{message}");
    assert!(message.contains("plan.toml"), "{message}");

    let approved = flow.commit("the plan");
    let stamped = flow.ok(&approve);
    assert_eq!(stamped["commit"], approved.as_str(), "{stamped}");
    assert_eq!(
        stamped["transition"],
        json!({ "from": "draft", "to": "in-progress" })
    );
    assert_eq!(stamped["promoted"], json!([BACKLOG_ID]));
    assert_eq!(stamped["skipped_stale"], json!([]));

    let backlog = flow.doc(&store_path(&flow.root));
    let row = &array(&backlog, "backlog")[0];
    assert_eq!(row["status"].as_str(), Some("promoted"));
    assert_eq!(row["promoted_to"].as_str(), Some(SLUG));

    let registry = flow.doc(&flow.registry());
    let entry = array(&registry, "active")
        .iter()
        .find(|row| row["slug"].as_str() == Some(SLUG))
        .expect("the flow is registered");
    assert_eq!(entry["binding"]["branch"].as_str(), Some("feat/retry"));
    assert_eq!(entry["binding"]["scope"][0].as_str(), Some("src/**"));

    let context = flow.doc(&flow.flow_path("context.toml"));
    assert_eq!(context["status"].as_str(), Some("in-progress"));
    assert_eq!(context["branch"].as_str(), Some("feat/retry"));
    assert_eq!(context["scope"][0].as_str(), Some("src/**"));

    let resolved = flow.resolve_path();
    assert_eq!(resolved["resolved"], true, "{resolved}");
    assert_eq!(resolved["slug"], SLUG, "{resolved}");
    flow.assert_stage("approved", "none");

    // After approval a plan field moves only with a reason, and leaves a row.
    let tasks_path = flow.flow_path("tasks.toml");
    let without = flow.payload(
        "without.ndjson",
        "{\"id\":2,\"set\":{\"action\":\"Rewire it.\"}}\n",
    );
    let (kind, message) = flow.refused(&["tasks", "update", "--slug", SLUG, "--ndjson", &without]);
    assert_eq!(kind, "validation", "{message}");
    assert!(message.contains("reason"), "{message}");
    assert_eq!(array(&flow.doc(&tasks_path), "changes").len(), 0);

    flow.update(&json!({
        "id": 2,
        "set": { "action": "Rewire it." },
        "reason": "The caller moved.",
    }));
    let store = flow.doc(&tasks_path);
    let changes = array(&store, "changes");
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0]["ref"].as_str(), Some("wire-the-guard"));
    assert_eq!(changes[0]["field"].as_str(), Some("action"));
    assert_eq!(changes[0]["new"].as_str(), Some("Rewire it."));
    assert_eq!(changes[0]["reason"].as_str(), Some("The caller moved."));

    let (_, message) = flow.refused(&["tasks", "remove", "4", "--slug", SLUG]);
    assert!(message.contains("--reason"), "{message}");
    flow.ok(&[
        "tasks",
        "remove",
        "4",
        "--slug",
        SLUG,
        "--reason",
        "Out of scope.",
    ]);
    let store = flow.doc(&tasks_path);
    assert_eq!(array(&store, "items").len(), 3);
    assert_eq!(array(&store, "specs").len(), 3);
    let tombstone = &array(&store, "changes")[1];
    assert_eq!(tombstone["ref"].as_str(), Some("spare-work"));
    assert_eq!(tombstone["field"].as_str(), Some("*"));
    assert_eq!(tombstone["old"].as_str(), Some("Spare work"));
    assert_eq!(tombstone["reason"].as_str(), Some("Out of scope."));

    let (kind, message) = flow.refused(&["tasks", "import-plan", "--slug", SLUG]);
    assert_eq!(kind, "validation", "{message}");
    assert!(message.contains("plan view"), "{message}");

    let check = flow.plan(&["check"]);
    assert_eq!(check["ok"], true, "{check}");
    assert_eq!(
        finding_classes(&check),
        ["plan/milestone-no-gate"],
        "{check}"
    );

    // Drift.
    flow.update(&json!({ "id": 1, "status": "in-progress" }));
    flow.assert_stage("running", "plan drift");
    fs::write(flow.root.join("src").join("a.rs"), "fn changed() {}\n").expect("a source file");
    let outside = flow.commit("outside work");

    let drift = flow.plan(&["drift"]);
    assert_eq!(drift["approved_commit"], approved.as_str(), "{drift}");
    assert_eq!(affected_ids(&drift), [1], "{drift}");
    assert_eq!(
        drift["affected"][0]["files"],
        json!(["src/a.rs"]),
        "{drift}"
    );
    assert_eq!(
        drift["affected"][0]["premises"][0]["claim"], "Premise-a-exists.",
        "{drift}"
    );

    flow.update(&json!({ "id": 1, "commit": outside }));
    let drift = flow.plan(&["drift"]);
    assert_eq!(
        affected_ids(&drift),
        Vec::<u64>::new(),
        "a commit the row records is the plan's own: {drift}"
    );
    assert_eq!(array(&flow.doc(&tasks_path), "changes").len(), 2);

    // Finish.
    flow.plan(&["handover", "done", "H1"]);
    let gate = flow.plan(&["gate"]);
    assert_eq!(
        gate["stops"],
        json!([{ "kind": "gate", "milestone": "A", "command": "cargo test" }])
    );
    assert_eq!(gate["handovers"], json!([]));
    for id in 1..=3 {
        flow.update(&json!({ "id": id, "status": "done" }));
    }
    flow.assert_stage("done", "none");

    for store in [
        flow.flow_path("plan.toml"),
        tasks_path,
        flow.flow_path("context.toml"),
        flow.registry(),
        store_path(&flow.root),
    ] {
        assert_sidecar_matches(&store);
    }
}
