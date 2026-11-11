//! `tomlctl review settle` and the ageing of deferred proposals, against a
//! sandbox repo whose store is seeded by hand.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{cli, git_available, git_command, sandbox, stdout_json};

const SCOPE: &str = "t";

const VOCABULARY: &str = r#"
[concerns.logic]
meaning = "wrong output for a nameable input"
families = ["code"]
"#;

const LENS: &str = "+++\nid = \"correctness\"\nstatus = \"stable\"\nfamilies = [\"code\"]\n\
    mode = \"judgement\"\nproduces = [\"defect\", \"cleanup\", \"advisory\"]\nowns = [\"logic\"]\n\
    activation = \"floor\"\nslice = \"whole-scope\"\nv1_category = \"quality\"\n+++\n\
    ## Question\nIs it correct?\n\n## Owns\n`logic` — owned here.\n\n\
    ## Probes — code\n- one probe\n\n## Does not flag\n- confirmations\n";

const CELL: &str = "[[cells]]\nid = \"C1\"\nrun = \"U1\"\nwave = 1\nslice = [\"src/**\"]\n\
    concerns = [\"correctness\"]\nagent = \"review-finder\"\nreason = \"proposed\"\n\
    status = \"covered\"\nsealed = true\nnote = \"n\"\n\n";

const EVIDENCE: &str =
    "blocked_on = \"evidence\"\nblocked_missing = \"whether the cache is shared\"\n";
const LINK: &str = "apply_flow = \"auth-apply\"\napply_task = \"settle-the-cache\"\n";

struct Repo {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
    builtin: String,
}

impl Repo {
    fn new() -> Option<Self> {
        if !git_available() {
            eprintln!("skipping: git is not available");
            return None;
        }
        let (dir, root) = sandbox();
        let catalogue = tempfile::tempdir().unwrap();
        let builtin = catalogue.path().join("review-v2-lenses");
        fs::create_dir_all(builtin.join("lenses")).unwrap();
        fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
        fs::write(builtin.join("lenses/correctness.md"), LENS).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.rs"), "fn f() {}\n").unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/proposal.md"), "# Proposal\n").unwrap();
        fs::write(root.join("docs/other.md"), "# Other\n").unwrap();
        let out = git_command(&root).args(["add", "-A"]).output().unwrap();
        assert!(out.status.success());
        Some(Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
            builtin: builtin.to_string_lossy().replace('\\', "/"),
        })
    }

    fn path(&self) -> PathBuf {
        self.root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"))
    }

    fn run_row(&self, id: &str, family: &str, started: &str) -> String {
        format!(
            "[[runs]]\nid = \"{id}\"\ncommand = \"review-v2\"\nfamily = \"{family}\"\n\
             depth = \"quick\"\nstarted = \"{started}\"\nbase = \"abc\"\nscope = [\"src/**\"]\n\
             changed_lines = 10\ncatalogue = {{ builtin = \"{}\" }}\n\n",
            self.builtin
        )
    }

    /// Seeds run U1, the cell over `src/**`, then `rows`.
    fn seed(&self, rows: &str) {
        let text = format!(
            "schema_version = 2\n\n{}{CELL}{rows}",
            self.run_row("U1", "code", "2026-10-12T10:00:00Z")
        );
        let path = self.path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn tomlctl(&self) -> assert_cmd::Command {
        let mut cmd = cli(&self.root);
        cmd.current_dir(&self.root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        cmd
    }

    fn settle(&self, id: &str, args: &[&str]) -> assert_cmd::assert::Assert {
        self.tomlctl()
            .args(["review", "settle", id, "--scope", SCOPE])
            .args(args)
            .assert()
    }

    fn staged(&self, name: &str, text: &str) -> String {
        let path = self.root.join(".claude").join(name);
        fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn bytes(&self) -> String {
        fs::read_to_string(self.path()).unwrap()
    }

    fn store(&self) -> toml::Value {
        toml::from_str(&self.bytes()).unwrap()
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

    fn digest(&self) -> serde_json::Value {
        let out = self
            .tomlctl()
            .args(["review", "digest", "--run", "U1", "--scope", SCOPE])
            .assert()
            .success();
        stdout_json(&out)
    }
}

fn finding(id: &str, status: &str, extra: &str) -> String {
    format!(
        "[[findings]]\nid = \"{id}\"\nrun = \"U1\"\ncell = \"C1\"\nseen_in = [\"U1\"]\nfamily = \"code\"\n\
         kind = \"defect\"\nlens = \"correctness\"\nroute = \"verifier\"\nfile = \"src/a.rs\"\nline = 1\n\
         summary = \"s {id}\"\nseverity = \"major\"\nstatus = \"{status}\"\n{extra}\n"
    )
}

fn text<'a>(row: &'a toml::Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(toml::Value::as_str)
}

fn stderr(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).to_string()
}

fn assert_unlinked(f: &toml::Value) {
    assert!(f.get("apply_flow").is_none(), "{f}");
    assert!(f.get("apply_task").is_none(), "{f}");
}

#[test]
fn settle_accepts_a_scheduled_finding_and_unblocks_it() {
    let Some(repo) = Repo::new() else {
        return;
    };
    repo.seed(&finding("F1", "scheduled", &format!("{EVIDENCE}{LINK}")));
    let fix = repo.staged("fix.md", "share the cache behind a lock\r\n");
    let args = ["--accept", "--tier", "cited", "--fix-file", &fix];
    let out = stdout_json(&repo.settle("F1", &args).success());
    assert_eq!(out["outcome"], "accepted", "{out}");
    assert_eq!(out["from"], "scheduled", "{out}");
    assert_eq!(out["status"], "open", "{out}");

    let f = repo.finding("F1");
    assert_eq!(text(&f, "status"), Some("open"));
    assert_unlinked(&f);
    assert!(f.get("blocked_on").is_none(), "{f}");
    assert!(f.get("blocked_missing").is_none(), "{f}");
    assert_eq!(
        text(&f, "fix_sketch"),
        Some("share the cache behind a lock")
    );
    assert_eq!(text(&f, "settle_tier"), Some("cited"));
    assert_eq!(text(&f, "unblocked_by"), Some("settle"));

    let before = repo.bytes();
    let replay = stdout_json(&repo.settle("F1", &args).success());
    assert_eq!(replay["unchanged"], true, "{replay}");
    assert_eq!(repo.bytes(), before, "a replay writes nothing");
}

#[test]
fn settle_refutes_through_an_orchestrator_verdict() {
    let Some(repo) = Repo::new() else {
        return;
    };
    repo.seed(&finding("F1", "scheduled", &format!("{EVIDENCE}{LINK}")));
    let args = ["--refute", "--citation", "src/a.rs:1"];
    let out = stdout_json(&repo.settle("F1", &args).success());
    assert_eq!(out["outcome"], "refuted", "{out}");
    assert_eq!(out["status"], "refuted", "{out}");

    let f = repo.finding("F1");
    assert_eq!(text(&f, "status"), Some("refuted"));
    assert_unlinked(&f);
    let store = repo.store();
    let verdicts = store["verdicts"].as_array().expect("a verdict was written");
    assert_eq!(verdicts.len(), 1, "{store}");
    assert_eq!(text(&verdicts[0], "by"), Some("orchestrator"));
    assert_eq!(text(&verdicts[0], "verdict"), Some("refuted"));
    assert_eq!(text(&verdicts[0], "citation"), Some("src/a.rs:1"));

    let before = repo.bytes();
    assert_eq!(
        stdout_json(&repo.settle("F1", &args).success())["unchanged"],
        true
    );
    assert_eq!(repo.bytes(), before);
}

#[test]
fn settle_refutes_a_panel_routed_finding() {
    let Some(repo) = Repo::new() else {
        return;
    };
    let panel = finding("F1", "scheduled", &format!("{EVIDENCE}{LINK}"))
        .replace("route = \"verifier\"", "route = \"panel\"");
    repo.seed(&panel);
    let out = stdout_json(
        &repo
            .settle("F1", &["--refute", "--citation", "src/a.rs:1"])
            .success(),
    );
    assert_eq!(out["status"], "refuted", "{out}");
    assert_eq!(text(&repo.finding("F1"), "status"), Some("refuted"));
}

#[test]
fn settle_defers_once_and_refuses_a_second_deferral() {
    let Some(repo) = Repo::new() else {
        return;
    };
    repo.seed(&finding("F1", "scheduled", &format!("{EVIDENCE}{LINK}")));
    let out = stdout_json(
        &repo
            .settle("F1", &["--defer", "--proposal", "docs/proposal.md"])
            .success(),
    );
    assert_eq!(out["outcome"], "deferred", "{out}");
    let f = repo.finding("F1");
    assert_eq!(text(&f, "status"), Some("open"));
    assert_unlinked(&f);
    assert_eq!(text(&f, "blocked_on"), Some("evidence"), "still blocked");
    assert_eq!(text(&f, "proposal"), Some("docs/proposal.md"));
    let at = f["proposal_at"].as_datetime().expect("a TOML datetime");
    assert!(at.time.is_some() && at.offset.is_some(), "{at}");

    let before = repo.bytes();
    repo.settle("F1", &["--defer", "--proposal", "docs/proposal.md"])
        .success();
    assert_eq!(repo.bytes(), before, "the same deferral replays unchanged");

    let refused = repo
        .settle("F1", &["--defer", "--proposal", "docs/other.md"])
        .failure();
    assert!(
        stderr(&refused).contains("already carries proposal"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(repo.bytes(), before);
}

#[test]
fn settle_refuses_a_finding_not_blocked_on_evidence() {
    let Some(repo) = Repo::new() else {
        return;
    };
    let decision = "blocked_on = \"decision\"\nblocked_missing = \"which policy\"\n\
                    blocked_options = [\"a — faster\", \"b — safer\"]\n";
    repo.seed(&format!(
        "{}{}",
        finding("F1", "open", decision),
        finding("F2", "open", ""),
    ));
    let before = repo.bytes();
    let refused = repo
        .settle("F1", &["--refute", "--citation", "src/a.rs:1"])
        .failure();
    assert!(
        stderr(&refused).contains("blocked on decision"),
        "{}",
        stderr(&refused)
    );
    let refused = repo
        .settle("F2", &["--defer", "--proposal", "docs/proposal.md"])
        .failure();
    assert!(
        stderr(&refused).contains("is not blocked"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(repo.bytes(), before, "a refusal writes nothing");

    repo.settle("F1", &["--accept", "--tier", "cited"])
        .failure();
    repo.settle("F1", &["--refute", "--defer", "--citation", "x"])
        .failure();
}

#[test]
fn settle_digest_age_counts_later_runs_of_the_family() {
    let Some(repo) = Repo::new() else {
        return;
    };
    let proposal = "proposal = \"docs/proposal.md\"\nproposal_at = 2026-10-12T11:00:00Z\n";
    let rows = |later: &[(&str, &str, &str)]| {
        let mut rows = finding("F1", "open", &format!("{EVIDENCE}{proposal}"));
        for (id, family, started) in later {
            rows.push_str(&repo.run_row(id, family, started));
        }
        rows
    };
    // U1 started before the deferral and a perf run is another family: age 1.
    repo.seed(&rows(&[
        ("U2", "code", "2026-10-12T15:00:00Z"),
        ("U3", "perf", "2026-10-13T10:00:00Z"),
    ]));
    let digest = repo.digest();
    let listed = &digest["proposals"];
    assert_eq!(listed.as_array().map(Vec::len), Some(1), "{digest}");
    assert_eq!(listed[0]["id"], "F1");
    assert_eq!(listed[0]["proposal"], "docs/proposal.md");
    assert_eq!(listed[0]["age"], 1, "a run later the same day counts");
    assert_eq!(listed[0]["overdue"], false);

    repo.seed(&rows(&[
        ("U2", "code", "2026-10-12T15:00:00Z"),
        ("U3", "code", "2026-10-13T10:00:00Z"),
        ("U4", "code", "2026-10-14T10:00:00Z"),
    ]));
    let digest = repo.digest();
    assert_eq!(digest["proposals"][0]["age"], 3, "{digest}");
    assert_eq!(digest["proposals"][0]["overdue"], true, "{digest}");

    let fix = repo.staged("fix.md", "take the lock");
    repo.settle(
        "F1",
        &["--accept", "--tier", "mechanical", "--fix-file", &fix],
    )
    .success();
    let digest = repo.digest();
    assert_eq!(digest["proposals"], serde_json::json!([]), "{digest}");
}
