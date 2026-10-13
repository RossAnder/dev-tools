//! `tomlctl review triage` against a sandbox whose store is seeded by hand
//! with one open run, advisory and defect findings, and a temp-dir catalogue
//! holding only the vocabulary's `[routing]` globs.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, sandbox};
use serde_json::{Value as JsonValue, json};

const SCOPE: &str = "t";

const VOCABULARY: &str = r#"
[routing]
executable = ["**/*.rs"]
tests = ["tests/**"]
"#;

const FINDINGS: &str = r#"
[[findings]]
id = "F1"
run = "U1"
cell = "C1"
family = "code"
kind = "advisory"
lens = "contrarian"
route = "none"
file = "docs/guide.md"
line = 3
summary = "the guide could teach the slow path"
angle = "alternative"
rationale = "new users reach for --fast first"
adoption_cost = "one paragraph"
status = "open"

[[findings]]
id = "F2"
run = "U1"
cell = "C1"
family = "code"
kind = "advisory"
lens = "contrarian"
route = "none"
file = "src/lib.rs"
line = 2
symbol = "lib::run"
summary = "run could stream"
angle = "alternative"
rationale = "rows arrive one at a time"
adoption_cost = "an iterator API"
status = "open"

[[findings]]
id = "F3"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 2
symbol = "lib::limit"
summary = "the limit is off by one"
severity = "major"
status = "open"

[[findings]]
id = "F4"
run = "U1"
cell = "C1"
family = "code"
kind = "advisory"
lens = "contrarian"
route = "none"
file = "src/lib.rs"
symbol = "lib::open"
summary = "open could take a handle"
angle = "direction"
rationale = "callers already hold one"
adoption_cost = "a signature change"
status = "open"
"#;

struct Repo {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let catalogue = tempfile::tempdir().unwrap();
        let builtin = catalogue.path().join("review-v2-lenses");
        fs::create_dir_all(&builtin).unwrap();
        fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
        let builtin = builtin.to_string_lossy().replace('\\', "/");

        write(&root, "src/lib.rs", "// line 1\nconst LIMIT: usize = 8;\n");
        write(&root, "docs/guide.md", "# Guide\n\nRun with `--fast`.\n");
        let store = format!(
            "schema_version = 1\n\n[[runs]]\nid = \"U1\"\ncommand = \"review-v2\"\n\
             family = \"code\"\ndepth = \"standard\"\nstarted = \"2026-10-12T10:00:00Z\"\n\
             catalogue = {{ builtin = \"{builtin}\" }}\n\n\
             [[cells]]\nid = \"C1\"\nrun = \"U1\"\nwave = 1\nslice = [\"src/**\", \"docs/**\"]\n\
             concerns = [\"correctness\", \"contrarian\"]\nagent = \"review-finder\"\n\
             reason = \"proposed\"\nstatus = \"covered\"\nsealed = true\nnote = \"done\"\n\
             {FINDINGS}"
        );
        write(&root, ".claude/review-v2/t.toml", &store);
        Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
        }
    }

    fn triage(&self, ids: &str, args: &[&str]) -> assert_cmd::assert::Assert {
        cli(&self.root)
            .current_dir(&self.root)
            .args(["review", "triage", ids, "--scope", SCOPE])
            .args(args)
            .assert()
    }

    fn triaged(&self, ids: &str, args: &[&str]) -> JsonValue {
        let out = self.triage(ids, args).success();
        serde_json::from_slice(&out.get_output().stdout).expect("triage prints JSON")
    }

    /// Writes `text` under `.claude/` and returns its path as an argument.
    fn staged(&self, name: &str, text: &str) -> String {
        let path = self.root.join(".claude").join(name);
        fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn store(&self) -> toml::Value {
        toml::from_str(&fs::read_to_string(self.root.join(".claude/review-v2/t.toml")).unwrap())
            .unwrap()
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

    fn status(&self, id: &str) -> String {
        self.finding(id)["status"].as_str().unwrap().to_string()
    }

    /// Rewrites the store through `edit` and returns its new bytes.
    fn rewrite(&self, edit: impl FnOnce(&mut toml::Value)) -> String {
        let path = self.root.join(".claude/review-v2/t.toml");
        let mut doc = self.store();
        edit(&mut doc);
        fs::write(&path, toml::to_string(&doc).unwrap()).unwrap();
        fs::read_to_string(path).unwrap()
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn text<'a>(row: &'a toml::Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(toml::Value::as_str)
}

#[test]
fn triage_dismiss_requires_reason() {
    let repo = Repo::new();
    let before = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();

    repo.triage("F3", &["--dismiss", "  "]).failure();
    let blank = repo.staged("blank.md", "\r\n  \r\n");
    repo.triage("F3", &["--dismiss-file", &blank]).failure();
    repo.triage("F3", &["--dismiss"]).failure();

    let after = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();
    assert_eq!(before, after, "a refused dismissal writes nothing");
    assert_eq!(repo.status("F3"), "open");
}

#[test]
fn triage_honours_expect_status() {
    let repo = Repo::new();
    let out = repo.triaged("F3", &["--dismiss", "not a bug", "--expect-status", "open"]);
    assert_eq!(
        out["applied"],
        json!([{"id": "F3", "from": "open", "status": "dismissed"}])
    );
    assert_eq!(repo.status("F3"), "dismissed");

    let out = repo.triaged(
        "F3,F1",
        &[
            "--defer",
            "later",
            "--trigger",
            "after v2",
            "--expect-status",
            "open",
        ],
    );
    assert_eq!(
        out["skipped_stale"],
        json!([{"id": "F3", "field": "status", "expected": "open", "found": "dismissed"}])
    );
    assert_eq!(out["applied"][0]["id"], "F1");
    assert_eq!(repo.status("F3"), "dismissed", "a stale row is not moved");
    assert_eq!(
        text(&repo.finding("F3"), "dismiss_reason"),
        Some("not a bug")
    );
    let f1 = repo.finding("F1");
    assert_eq!(text(&f1, "status"), Some("deferred"));
    assert_eq!(text(&f1, "defer_reason"), Some("later"));
    assert_eq!(text(&f1, "defer_trigger"), Some("after v2"));

    // A replay of the move already made is unchanged, not stale.
    let out = repo.triaged("F3", &["--dismiss", "not a bug", "--expect-status", "open"]);
    assert_eq!(out["unchanged"], json!(["F3"]));
    assert_eq!(out["skipped_stale"], json!([]));

    // Reopening clears the companions of the status it leaves.
    let out = repo.triaged(
        "F3",
        &["--reopen", "it is a bug", "--expect-status", "dismissed"],
    );
    assert_eq!(out["applied"][0]["status"], "open");
    let f3 = repo.finding("F3");
    assert_eq!(text(&f3, "status"), Some("open"));
    assert_eq!(text(&f3, "dismiss_reason"), None);
    assert_eq!(text(&f3, "reopen_reason"), Some("it is a bug"));
}

#[test]
fn triage_reads_reason_from_file() {
    let repo = Repo::new();
    let reason = repo.staged("reason.md", "\u{feff}two lines\r\nof reason\r\n");
    let trigger = repo.staged("trigger.md", "when the API settles\n");
    repo.triaged(
        "F1,F4",
        &["--defer-file", &reason, "--trigger-file", &trigger],
    );
    for id in ["F1", "F4"] {
        let row = repo.finding(id);
        assert_eq!(text(&row, "status"), Some("deferred"));
        assert_eq!(text(&row, "defer_reason"), Some("two lines\nof reason"));
        assert_eq!(text(&row, "defer_trigger"), Some("when the API settles"));
    }

    let resolution = repo.staged("resolution.md", "fixed in the limit rewrite\n");
    repo.triaged("F3", &["--resolve-file", &resolution]);
    let f3 = repo.finding("F3");
    assert_eq!(text(&f3, "resolution"), Some("fixed in the limit rewrite"));
    assert!(
        f3.get("resolved")
            .and_then(toml::Value::as_datetime)
            .is_some(),
        "resolved is stamped as a TOML date: {f3:?}"
    );
}

#[test]
fn park_marks_advisory_parked() {
    let repo = Repo::new();
    let out = repo.triaged("F4", &["--park", "untriaged at the close of U1"]);
    assert_eq!(out["applied"][0]["status"], "parked");
    let f4 = repo.finding("F4");
    assert_eq!(text(&f4, "status"), Some("parked"));
    assert_eq!(
        text(&f4, "park_reason"),
        Some("untriaged at the close of U1")
    );

    repo.triage("F3", &["--park", "a defect is not parked"])
        .failure();
    assert_eq!(repo.status("F3"), "open");

    // One refused id among several lands the rest and reports the refusal.
    let out = repo.triaged("F1,F3", &["--park", "later", "--expect-status", "open"]);
    assert_eq!(out["applied"][0]["id"], "F1");
    assert_eq!(out["refused"][0]["id"], "F3");
    assert_eq!(repo.status("F1"), "parked");
    assert_eq!(repo.status("F3"), "open");
}

#[test]
fn convert_advisory_mints_routed_cleanup() {
    let repo = Repo::new();
    let payload = repo.staged(
        "convert-F1.ndjson",
        &format!(
            "{}\n",
            json!({
                "edit": "mention the default path before --fast",
                "direction": "change",
                "old": "Run with `--fast`.",
                "effort": "trivial",
            })
        ),
    );
    let args = [
        "--convert",
        "cleanup",
        "--ndjson",
        &payload,
        "--expect-status",
        "open",
    ];
    let out = repo.triaged("F1", &args);
    assert_eq!(
        out["applied"],
        json!([{"id": "F1", "from": "open", "status": "converted", "converted_to": "F5"}])
    );
    assert_eq!(
        out["minted"],
        json!([{"id": "F5", "kind": "cleanup", "severity": null, "route": "checker", "status": "unverified"}])
    );

    let f1 = repo.finding("F1");
    assert_eq!(text(&f1, "status"), Some("converted"));
    assert_eq!(text(&f1, "converted_to"), Some("F5"));
    let f5 = repo.finding("F5");
    assert_eq!(text(&f5, "kind"), Some("cleanup"));
    assert_eq!(text(&f5, "route"), Some("checker"));
    assert_eq!(text(&f5, "status"), Some("unverified"));
    assert_eq!(text(&f5, "lens"), Some("contrarian"));
    assert_eq!(text(&f5, "file"), Some("docs/guide.md"));
    assert_eq!(text(&f5, "run"), Some("U1"));
    assert_eq!(text(&f5, "cell"), Some("C1"));
    assert_eq!(f5["related"][0].as_str(), Some("F1"));
    let verdicts = repo.store()["verdicts"].as_array().unwrap().clone();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(text(&verdicts[0], "finding"), Some("F5"));
    assert_eq!(text(&verdicts[0], "by"), Some("orchestrator"));
    assert_eq!(text(&verdicts[0], "citation"), Some("docs/guide.md:3"));

    // A replay reports the replacement already minted and mints nothing.
    let out = repo.triaged("F1", &args);
    assert_eq!(out["unchanged"], json!(["F1"]));
    assert_eq!(out["minted"][0]["id"], "F5");
    assert_eq!(repo.store()["findings"].as_array().unwrap().len(), 5);

    // An edit to executable code routes to the verifier.
    let payload = repo.staged(
        "convert-F2.ndjson",
        &format!(
            "{}\n",
            json!({
                "edit": "return an iterator",
                "direction": "change",
                "old": "const LIMIT",
                "effort": "small",
            })
        ),
    );
    let out = repo.triaged("F2", &["--convert", "cleanup", "--ndjson", &payload]);
    assert_eq!(out["minted"][0]["route"], "verifier");

    // A replacement the field catalogue refuses writes nothing.
    let payload = repo.staged("convert-F4.ndjson", "{\"direction\": \"change\"}\n");
    repo.triage("F4", &["--convert", "cleanup", "--ndjson", &payload])
        .failure();
    assert_eq!(repo.status("F4"), "open");
    assert_eq!(repo.store()["findings"].as_array().unwrap().len(), 6);
}

#[test]
fn convert_refuses_old_absent_at_anchor() {
    let repo = Repo::new();
    let before = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();
    let payload = repo.staged(
        "convert-F1.ndjson",
        &format!(
            "{}\n",
            json!({
                "edit": "mention the default path before --fast",
                "direction": "change",
                "old": "Run with `--slow`.",
                "effort": "trivial",
            })
        ),
    );
    let out = repo
        .triage("F1", &["--convert", "cleanup", "--ndjson", &payload])
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("`old` does not occur in docs/guide.md"),
        "refusal names the missing `old`: {stderr}"
    );
    let after = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();
    assert_eq!(before, after, "a refused conversion writes nothing");
}

/// Finishes U1 and adds run `U2` of `family`, `finished` when given.
fn second_run(doc: &mut toml::Value, family: &str, finished: Option<&str>) {
    let runs = doc["runs"].as_array_mut().unwrap();
    runs[0]
        .as_table_mut()
        .unwrap()
        .insert("finished".into(), "2026-10-12T11:00:00Z".into());
    let mut u2 = runs[0].as_table().unwrap().clone();
    u2.remove("finished");
    u2.insert("id".into(), "U2".into());
    u2.insert("family".into(), family.into());
    u2.insert("started".into(), "2026-10-13T09:00:00Z".into());
    if let Some(at) = finished {
        u2.insert("finished".into(), at.into());
    }
    runs.push(toml::Value::Table(u2));
}

fn convert_payload(repo: &Repo) -> String {
    repo.staged(
        "convert-F1.ndjson",
        &format!(
            "{}\n",
            json!({
                "edit": "mention the default path before --fast",
                "direction": "change",
                "old": "Run with `--fast`.",
                "effort": "trivial",
            })
        ),
    )
}

fn refused_with(repo: &Repo, args: &[&str], needle: &str) {
    let before = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();
    let out = repo.triage("F1", args).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains(needle), "{args:?}: {stderr}");
    let after = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();
    assert_eq!(
        before, after,
        "{args:?}: a refused conversion writes nothing"
    );
}

#[test]
fn convert_refuses_a_finished_run() {
    let repo = Repo::new();
    repo.rewrite(|doc| second_run(doc, "code", Some("2026-10-13T10:00:00Z")));
    let payload = convert_payload(&repo);
    let convert = ["--convert", "cleanup", "--ndjson", payload.as_str()];

    refused_with(&repo, &convert, "U1, which is finished");
    let named = |run: &'static str| [&convert[..], &["--run", run]].concat();
    refused_with(&repo, &named("U2"), "--run U2 is finished");
    refused_with(&repo, &named("U9"), "--run U9 is not in this store");
}

#[test]
fn convert_into_open_run() {
    let repo = Repo::new();
    repo.rewrite(|doc| second_run(doc, "code", None));
    let payload = convert_payload(&repo);
    let out = repo.triaged(
        "F1",
        &["--convert", "cleanup", "--ndjson", &payload, "--run", "U2"],
    );
    assert_eq!(out["minted"][0]["id"], "F5");
    assert_eq!(out["minted"][0]["status"], "unverified");

    let f5 = repo.finding("F5");
    assert_eq!(text(&f5, "run"), Some("U2"));
    assert_eq!(f5["seen_in"], toml::Value::Array(vec!["U2".into()]));
    assert_eq!(text(&f5, "cell"), Some("C1"), "the advisory's cell is kept");
    assert_eq!(text(&f5, "status"), Some("unverified"));
    assert_eq!(text(&repo.finding("F1"), "status"), Some("converted"));
    let verdicts = repo.store()["verdicts"].as_array().unwrap().clone();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(text(&verdicts[0], "finding"), Some("F5"));
    assert_eq!(text(&verdicts[0], "run"), Some("U2"));

    let finish = |root: &Path| {
        cli(root)
            .current_dir(root)
            .args(["review", "run", "finish", "U2", "--scope", SCOPE])
            .assert()
    };
    let out = finish(&repo.root).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("unverified findings: F5"), "{stderr}");

    cli(&repo.root)
        .current_dir(&repo.root)
        .args(["review", "verdict", "F5", "--scope", SCOPE])
        .args(["--verdict", "confirmed", "--by", "checker"])
        .args([
            "--citation",
            "docs/guide.md:3",
            "--rationale",
            "the edit applies",
        ])
        .assert()
        .success();
    assert_ne!(repo.status("F5"), "unverified");
    finish(&repo.root).success();
}

#[test]
fn convert_refuses_a_foreign_family_run() {
    let repo = Repo::new();
    repo.rewrite(|doc| second_run(doc, "perf", None));
    let payload = convert_payload(&repo);
    refused_with(
        &repo,
        &["--convert", "cleanup", "--ndjson", &payload, "--run", "U2"],
        "--run U2 is a perf run; F1 is a code finding",
    );

    // An advisory whose run is open converts there and nowhere else.
    let repo = Repo::new();
    repo.rewrite(|doc| {
        second_run(doc, "code", None);
        doc["runs"][0].as_table_mut().unwrap().remove("finished");
    });
    let payload = convert_payload(&repo);
    refused_with(
        &repo,
        &["--convert", "cleanup", "--ndjson", &payload, "--run", "U2"],
        "U1, which is open",
    );
    let out = repo.triaged(
        "F1",
        &["--convert", "cleanup", "--ndjson", &payload, "--run", "U1"],
    );
    assert_eq!(text(&repo.finding("F5"), "run"), Some("U1"));
    assert_eq!(out["minted"][0]["id"], "F5");
}

#[test]
fn triage_refuses_a_settled_finding() {
    let repo = Repo::new();
    let before = repo.rewrite(|doc| {
        let f3 = doc["findings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["id"].as_str() == Some("F3"))
            .unwrap();
        f3.as_table_mut()
            .unwrap()
            .insert("status".into(), "exported".into());
    });

    for args in [["--dismiss", "not a bug"], ["--reopen", "it is a bug"]] {
        let out = repo.triage("F3", &args).failure();
        let stderr = String::from_utf8_lossy(&out.get_output().stderr);
        assert!(
            stderr.contains("F3 is exported; triage does not move a exported finding"),
            "{args:?}: {stderr}"
        );
        let after = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();
        assert_eq!(before, after, "{args:?}: a settled row is not moved");
    }
}

#[test]
fn reopen_retires_the_v1_link() {
    let repo = Repo::new();
    repo.rewrite(|doc| {
        let f3 = doc["findings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["id"].as_str() == Some("F3"))
            .unwrap()
            .as_table_mut()
            .unwrap();
        f3.insert("status".into(), "resolved".into());
        f3.insert("resolution".into(), "fixed in v1".into());
        f3.insert("exported_as".into(), "R7".into());
        f3.insert("export_ledger".into(), ".claude/reviews/t.toml".into());
    });

    let out = repo.triaged("F3", &["--reopen", "the fix regressed"]);
    assert_eq!(
        out["applied"],
        json!([{"id": "F3", "from": "resolved", "status": "open"}])
    );
    let f3 = repo.finding("F3");
    assert_eq!(text(&f3, "status"), Some("open"));
    assert_eq!(text(&f3, "reopen_reason"), Some("the fix regressed"));
    assert_eq!(text(&f3, "resolution"), None);
    assert_eq!(f3.get("exported_as"), None);
    assert_eq!(f3.get("export_ledger"), None);
    assert_eq!(
        f3.get("retired_links"),
        Some(&toml::Value::Array(vec![
            ".claude/reviews/t.toml#R7".into()
        ])),
    );
}

#[test]
fn backlog_ref_marks_backlogged() {
    let repo = Repo::new();
    let out = repo.triaged(
        "F2",
        &["--backlog-ref", "B-1a2b3c4d", "--expect-status", "open"],
    );
    assert_eq!(out["applied"][0]["status"], "backlogged");
    let f2 = repo.finding("F2");
    assert_eq!(text(&f2, "status"), Some("backlogged"));
    assert_eq!(text(&f2, "backlog_ref"), Some("B-1a2b3c4d"));

    repo.triage("F4", &["--backlog-ref", "1a2b3c4d"]).failure();
    assert_eq!(repo.status("F4"), "open");
}
