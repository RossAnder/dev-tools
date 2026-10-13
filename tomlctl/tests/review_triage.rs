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
