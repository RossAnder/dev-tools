//! `tomlctl review run start|update` and `review schema` against a sandbox
//! repo. Every start passes `--builtin` so no test reads the developer's
//! claude dir.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, git_available, git_command, sandbox};
use serde_json::{Value as JsonValue, json};

const SCOPE: &str = "t";

const VOCABULARY: &str = r#"
[signals."lang:rust"]
kind = "stack"
meaning = "Rust sources"
[signals."lang:rust".detect]
files = ["**/Cargo.toml"]

[concerns.logic]
meaning = "wrong output for a nameable input"
families = ["code"]

[routing]
executable = ["**/*.rs"]
tests = ["**/tests/**"]
"#;

const CORRECTNESS: &str = r#"+++
id = "correctness"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["logic"]
activation = "floor"
slice = "per-subsystem"
v1_category = "quality"
+++
## Question
Does it compute the right thing?

## Owns
`logic` — wrong output.

## Probes — code
- off-by-one in ranges

## Does not flag
- confirmations
"#;

struct Repo {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
    builtin: PathBuf,
    /// The first commit: `src/a.rs` with three lines.
    first: String,
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = git_command(root)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repo with two commits: the second changes `src/a.rs` (+2 −1) and adds a
/// five-line `docs/x.md`.
fn repo() -> Option<Repo> {
    if !git_available() {
        eprintln!("skipping: git is not available");
        return None;
    }
    let (dir, root) = sandbox();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/a.rs"), "one\ntwo\nthree\n").unwrap();
    git(&root, &["add", "src/a.rs"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let first = git(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("src/a.rs"), "one\nTWO\nthree\nfour\n").unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("docs/x.md"), "1\n2\n3\n4\n5\n").unwrap();
    git(&root, &["add", "src/a.rs", "docs/x.md"]);
    git(&root, &["commit", "-q", "-m", "second"]);

    let catalogue = tempfile::tempdir().unwrap();
    let builtin = catalogue.path().join("review-v2-lenses");
    fs::create_dir_all(builtin.join("lenses")).unwrap();
    fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
    fs::write(builtin.join("lenses/correctness.md"), CORRECTNESS).unwrap();
    Some(Repo {
        _dir: dir,
        _catalogue: catalogue,
        root,
        builtin,
        first,
    })
}

impl Repo {
    fn tomlctl(&self) -> assert_cmd::Command {
        let mut cmd = cli(&self.root);
        cmd.current_dir(&self.root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        cmd
    }

    /// `--base=<rev>`, so an option-shaped rev reaches tomlctl as the value.
    fn start_args(&self, base: &str) -> Vec<String> {
        [
            "review",
            "run",
            "start",
            "--scope",
            SCOPE,
            "--command",
            "review-v2",
            "--family",
            "code",
            "--depth",
            "quick",
            &format!("--base={base}"),
            "--builtin",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain(std::iter::once(self.builtin.display().to_string()))
        .collect()
    }

    fn start(&self, base: &str, extra: &[&str]) -> JsonValue {
        let out = self
            .tomlctl()
            .args(self.start_args(base))
            .args(extra)
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("start prints JSON")
    }

    fn start_fails(&self, base: &str, extra: &[&str]) -> String {
        let out = self
            .tomlctl()
            .args(self.start_args(base))
            .args(extra)
            .assert()
            .failure();
        String::from_utf8_lossy(&out.get_output().stderr).to_string()
    }

    fn update(&self, run: &str, set_json: &str) -> assert_cmd::assert::Assert {
        self.tomlctl()
            .args([
                "review",
                "run",
                "update",
                run,
                "--scope",
                SCOPE,
                "--set-json",
            ])
            .arg(set_json)
            .assert()
    }

    fn store(&self) -> PathBuf {
        self.root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"))
    }

    fn runs_dir(&self) -> PathBuf {
        self.root
            .join(".claude/review-v2-runs")
            .join(format!("scope-{SCOPE}"))
    }

    fn read_store(&self) -> toml::Value {
        toml::from_str(&fs::read_to_string(self.store()).unwrap()).expect("the store parses")
    }

    fn run(&self, index: usize) -> toml::Value {
        self.read_store()["runs"][index].clone()
    }
}

#[test]
fn run_start_mints_sequential_ids_and_creates_run_dir() {
    let Some(repo) = repo() else { return };
    let one = repo.start(&repo.first, &[]);
    let two = repo.start(&repo.first, &[]);
    assert_eq!(one["run"], "U1");
    assert_eq!(two["run"], "U2");
    assert_eq!(one["dir"], ".claude/review-v2-runs/scope-t/U1");
    for run in ["U1", "U2"] {
        assert!(
            repo.runs_dir().join(run).join("payloads").is_dir(),
            "{run} has its run directory"
        );
    }
    let store = repo.read_store();
    let runs = store["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    let started = runs[0]["started"].as_str().unwrap();
    assert!(
        started.ends_with('Z') && started.len() == 20,
        "started is a UTC second stamp: {started}"
    );
    assert_eq!(runs[0]["command"].as_str(), Some("review-v2"));
    assert_eq!(runs[0]["depth"].as_str(), Some("quick"));
}

#[test]
fn run_start_records_catalogue_and_changed_lines() {
    let Some(repo) = repo() else { return };
    fs::create_dir_all(repo.root.join(".claude/review-lenses")).unwrap();
    fs::write(
        repo.root.join(".claude/review-lenses/config.toml"),
        "enable = []\n",
    )
    .unwrap();

    let printed = repo.start(
        &repo.first,
        &[
            "--set-json",
            r#"scope=["src/**"]"#,
            "--set",
            "substrate=workflow",
        ],
    );
    assert_eq!(
        printed["changed_lines"], 3,
        "src/a.rs +2 -1; docs/ is out of scope"
    );
    assert_eq!(printed["base"], repo.first.as_str());

    let run = repo.run(0);
    assert_eq!(
        run["base"].as_str(),
        Some(repo.first.as_str()),
        "stored as the sha"
    );
    assert_eq!(run["changed_lines"].as_integer(), Some(3));
    assert_eq!(run["substrate"].as_str(), Some("workflow"));
    let catalogue = &run["catalogue"];
    let builtin = catalogue["builtin"].as_str().unwrap();
    assert!(
        Path::new(builtin).is_absolute() && builtin.ends_with("review-v2-lenses"),
        "builtin is recorded absolute: {builtin}"
    );
    assert_eq!(catalogue["project"].as_str(), Some(".claude/review-lenses"));
    for sha in ["tree_sha", "config_sha"] {
        let value = catalogue[sha].as_str().unwrap_or_default();
        assert_eq!(value.len(), 64, "{sha} is a sha256: {value:?}");
    }

    let unchanged = repo.start("HEAD", &["--set-json", r#"scope=["src/**"]"#]);
    assert_eq!(
        unchanged["changed_lines"], 4,
        "no diff falls back to the scope's tracked line count"
    );
    let whole = repo.start(&repo.first, &[]);
    assert_eq!(whole["changed_lines"], 8, "no scope is the whole repo");
}

#[test]
fn run_start_halts_on_invalid_project_catalogue() {
    let Some(repo) = repo() else { return };
    fs::create_dir_all(repo.root.join(".claude/review-lenses")).unwrap();
    fs::write(
        repo.root.join(".claude/review-lenses/config.toml"),
        "disable = [\"no-such-lens\"]\n",
    )
    .unwrap();
    let stderr = repo.start_fails(&repo.first, &[]);
    assert!(
        stderr.contains("config.toml") && stderr.contains("no-such-lens"),
        "names the offending file and id: {stderr}"
    );
    assert!(!repo.store().exists(), "no run is minted");
    assert!(!repo.runs_dir().exists(), "no run directory is made");
}

#[test]
fn run_start_refuses_option_shaped_base() {
    let Some(repo) = repo() else { return };
    for base in ["--output=leak.txt", "-h", "--all"] {
        let stderr = repo.start_fails(base, &[]);
        assert!(
            stderr.contains("does not name a commit"),
            "{base}: {stderr}"
        );
    }
    assert!(
        !repo.root.join("leak.txt").exists(),
        "git never saw an option"
    );
    assert!(!repo.store().exists(), "no run is minted");
}

#[test]
fn run_ids_never_reuse_a_removed_id() {
    let Some(repo) = repo() else { return };
    repo.start(&repo.first, &[]);
    repo.start(&repo.first, &[]);
    repo.tomlctl()
        .args(["items", "remove"])
        .arg(repo.store())
        .args(["U2", "--array", "runs"])
        .assert()
        .success();
    assert_eq!(repo.start(&repo.first, &[])["run"], "U3");

    fs::remove_file(repo.store()).unwrap();
    assert_eq!(
        repo.start(&repo.first, &[])["run"],
        "U4",
        "a run directory left on disk retires its id"
    );
}

/// Starts U1 and records a detector signal on it, as `review survey` would.
fn run_with_detector_signal(repo: &Repo) {
    repo.start(&repo.first, &[]);
    let mut store = repo.read_store();
    let run = store["runs"][0].as_table_mut().unwrap();
    let signal: toml::Value =
        toml::from_str("name = \"lang:rust\"\nsource = \"detector\"\nroots = [\"src\"]\n").unwrap();
    run.insert("signals".into(), toml::Value::Array(vec![signal]));
    fs::write(repo.store(), toml::to_string(&store).unwrap()).unwrap();
}

#[test]
fn run_update_refuses_detector_signal_edits() {
    let Some(repo) = repo() else { return };
    run_with_detector_signal(&repo);
    let before = fs::read(repo.store()).unwrap();
    for payload in [
        r#"signals=[{"name":"lang:rust","accepted":false}]"#,
        r#"signals=[{"name":"lang:rust","source":"detector","roots":["lib"]}]"#,
        r#"signals=[{"name":"lang:ts","source":"detector","roots":["web"]}]"#,
    ] {
        let out = repo.update("U1", payload).failure();
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(stderr.contains("detector"), "{payload}: {stderr}");
    }
    assert_eq!(
        fs::read(repo.store()).unwrap(),
        before,
        "a refused update writes nothing"
    );
}

#[test]
fn run_update_upserts_signals_by_name() {
    let Some(repo) = repo() else { return };
    run_with_detector_signal(&repo);
    let candidate =
        r#"signals=[{"name":"trust-boundary","source":"judged","evidence":["src/a.rs"]}]"#;
    repo.update("U1", candidate).success();
    repo.update("U1", candidate).success();
    repo.update(
        "U1",
        r#"signals=[{"name":"trust-boundary","accepted":true}]"#,
    )
    .success();

    let signals = repo.run(0)["signals"].as_array().unwrap().clone();
    assert_eq!(
        signals.len(),
        2,
        "a retried candidate is one entry: {signals:?}"
    );
    assert_eq!(signals[0]["source"].as_str(), Some("detector"));
    let judged = &signals[1];
    assert_eq!(judged["name"].as_str(), Some("trust-boundary"));
    assert_eq!(judged["accepted"].as_bool(), Some(true));
    assert_eq!(
        judged["evidence"][0].as_str(),
        Some("src/a.rs"),
        "evidence kept"
    );

    repo.update(
        "U1",
        r#"signals=[{"name":"trust-boundary","evidence":["src/b.rs"]}]"#,
    )
    .failure();
    repo.update("U1", r#"models={"finder":"opus"}"#).failure();
}

#[test]
fn run_update_merges_baselines_and_waivers() {
    let Some(repo) = repo() else { return };
    repo.start(&repo.first, &[]);
    repo.update("U1", r#"baselines={"lint-baseline":"logs/lint.txt"}"#)
        .success();
    repo.update(
        "U1",
        r#"baselines={"profile-baseline":"none — no profiler"}"#,
    )
    .success();
    repo.update(
        "U1",
        r#"waivers=[{"concern":"security","reason":"no input handling"}]"#,
    )
    .success();
    repo.update(
        "U1",
        r#"waivers=[{"concern":"security","reason":"read-only change"},{"file":"docs/x.md","reason":"prose"}]"#,
    )
    .success();
    repo.update("U1", r#"waivers=[{"reason":"names nothing"}]"#)
        .failure();

    let run = repo.run(0);
    let baselines = run["baselines"].as_table().unwrap();
    assert_eq!(
        baselines.len(),
        2,
        "each update adds its check: {baselines:?}"
    );
    let waivers = run["waivers"].as_array().unwrap();
    assert_eq!(waivers.len(), 2, "{waivers:?}");
    assert_eq!(waivers[0]["reason"].as_str(), Some("read-only change"));
}

#[test]
fn schema_defect_requires_failure_scenario() {
    let (_dir, root) = sandbox();
    let schema = |extra: &[&str]| -> JsonValue {
        let out = cli(&root)
            .args(["review", "schema", "finding"])
            .args(extra)
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("schema prints JSON")
    };
    let required = |value: &JsonValue| -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };

    let defect = schema(&["--kind", "defect"]);
    assert!(required(&defect["required"]).contains(&"failure_scenario".to_string()));
    assert_eq!(defect["additionalProperties"], json!(false));
    let advisory = schema(&["--kind", "advisory"]);
    assert!(!required(&advisory["required"]).contains(&"failure_scenario".to_string()));
    assert!(advisory["properties"].get("failure_scenario").is_none());

    let any = schema(&[]);
    let branch = any["allOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["if"]["properties"]["kind"]["const"] == "defect")
        .expect("a defect branch");
    assert!(required(&branch["then"]["required"]).contains(&"failure_scenario".to_string()));

    cli(&root)
        .args(["review", "schema", "cell-return", "--kind", "defect"])
        .assert()
        .failure();
}
