//! `tomlctl review run start|update` and `review schema` against a sandbox
//! repo. Every start passes `--builtin` so no test reads the developer's
//! claude dir.

use crate::common;

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
    fn tomlctl(&self) -> common::Command {
        let mut cmd = cli(&self.root);
        cmd.env_remove("GIT_DIR")
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

    /// The project lens layer as an absolute `--project` value.
    fn project(&self) -> String {
        self.root
            .join(".claude/review-lenses")
            .display()
            .to_string()
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
            .join(".review-v2-runs")
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
    assert_eq!(one["dir"], ".review-v2-runs/scope-t/U1");
    assert!(
        !repo.root.join(".claude/review-v2-runs").exists(),
        "nothing lands under .claude/"
    );
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
            "--project",
            &repo.project(),
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

    let project = repo.project();
    let accept = ["--project", project.as_str()];
    let unchanged = repo.start(
        "HEAD",
        &["--set-json", r#"scope=["src/**"]"#, accept[0], accept[1]],
    );
    assert_eq!(
        unchanged["changed_lines"], 4,
        "no diff falls back to the scope's tracked line count"
    );
    let whole = repo.start(&repo.first, &accept);
    assert_eq!(whole["changed_lines"], 8, "no scope is the whole repo");
}

#[test]
fn run_start_ignores_the_runs_root() {
    let Some(repo) = repo() else { return };
    let ignore = repo.root.join(".review-v2-runs/.gitignore");
    repo.start(&repo.first, &[]);
    assert_eq!(fs::read_to_string(&ignore).unwrap(), "*\n");

    fs::write(&ignore, "custom\n").unwrap();
    repo.start(&repo.first, &[]);
    assert_eq!(
        fs::read_to_string(&ignore).unwrap(),
        "custom\n",
        "an existing ignore file is kept"
    );
}

/// Commits a project lens layer on top of `repo`, so `repo.first..HEAD`
/// changes it.
fn commit_project_layer(repo: &Repo) {
    fs::create_dir_all(repo.root.join(".claude/review-lenses")).unwrap();
    fs::write(
        repo.root.join(".claude/review-lenses/config.toml"),
        "enable = []\n",
    )
    .unwrap();
    git(&repo.root, &["add", ".claude/review-lenses/config.toml"]);
    git(&repo.root, &["commit", "-q", "-m", "project layer"]);
}

#[test]
fn run_start_refuses_a_project_layer_the_range_changes() {
    let Some(repo) = repo() else { return };
    commit_project_layer(&repo);
    let stderr = repo.start_fails(&repo.first, &[]);
    assert!(
        stderr.contains(".claude/review-lenses/config.toml") && stderr.contains("--project"),
        "names the changed path and the way to accept it: {stderr}"
    );
    assert!(!repo.store().exists(), "no run is minted");

    repo.start(&repo.first, &["--project", &repo.project()]);
    let catalogue = &repo.run(0)["catalogue"];
    assert_eq!(
        catalogue["project_layer_changed"],
        toml::Value::Array(vec![".claude/review-lenses/config.toml".into()]),
        "an accepted layer is recorded: {catalogue}"
    );

    let unchanged = repo.start("HEAD", &[]);
    assert_eq!(
        unchanged["run"], "U2",
        "a range that leaves the layer alone starts"
    );
    assert!(
        repo.run(1)["catalogue"]
            .get("project_layer_changed")
            .is_none()
    );
}

#[test]
fn run_start_refuses_a_project_layer_changed_after_head() {
    let Some(repo) = repo() else { return };
    let head = git(&repo.root, &["rev-parse", "HEAD"]);
    commit_project_layer(&repo);
    let stderr = repo.start_fails(&repo.first, &["--head", &head]);
    assert!(
        stderr.contains(".claude/review-lenses/config.toml") && stderr.contains("--project"),
        "a layer committed past --head is still the one loaded: {stderr}"
    );
    assert!(!repo.store().exists(), "no run is minted");

    repo.start(
        &repo.first,
        &["--head", &head, "--project", &repo.project()],
    );
    assert_eq!(
        repo.run(0)["catalogue"]["project_layer_changed"],
        toml::Value::Array(vec![".claude/review-lenses/config.toml".into()]),
    );
}

#[test]
fn run_start_refuses_an_uncommitted_project_layer() {
    let Some(repo) = repo() else { return };
    commit_project_layer(&repo);
    let layer = repo.root.join(".claude/review-lenses");
    fs::write(layer.join("config.toml"), "enable = []\n# edited\n").unwrap();
    let stderr = repo.start_fails("HEAD", &[]);
    assert!(
        stderr.contains(".claude/review-lenses/config.toml") && stderr.contains("--project"),
        "an unstaged edit is refused: {stderr}"
    );

    git(
        &repo.root,
        &["checkout", "--", ".claude/review-lenses/config.toml"],
    );
    fs::create_dir_all(layer.join("lenses")).unwrap();
    fs::write(layer.join("lenses/extra.md"), "untracked\n").unwrap();
    let stderr = repo.start_fails("HEAD", &[]);
    assert!(
        stderr.contains(".claude/review-lenses/lenses/extra.md"),
        "an untracked file is refused: {stderr}"
    );
    assert!(!repo.store().exists(), "no run is minted");

    fs::remove_file(layer.join("lenses/extra.md")).unwrap();
    fs::write(layer.join("config.toml"), "enable = []\n# edited\n").unwrap();
    repo.start("HEAD", &["--project", &repo.project()]);
    assert_eq!(
        repo.run(0)["catalogue"]["project_layer_changed"],
        toml::Value::Array(vec![".claude/review-lenses/config.toml".into()]),
        "an explicit --project accepts the edited layer"
    );
}

#[test]
fn run_catalogue_refuses_drift_since_start() {
    let Some(repo) = repo() else { return };
    repo.start(&repo.first, &[]);
    let survey = || {
        repo.tomlctl()
            .args(["review", "survey", "--scope", SCOPE, "--run", "U1"])
            .assert()
    };
    survey().success();

    fs::write(
        repo.builtin.join("lenses/correctness.md"),
        CORRECTNESS.replace("off-by-one", "off-by-two"),
    )
    .unwrap();
    let out = survey().failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("tree_sha") && stderr.contains("start a new run"),
        "{stderr}"
    );
}

#[test]
fn run_start_leaves_a_default_builtin_root_unrecorded() {
    let Some(repo) = repo() else { return };
    let claude = tempfile::tempdir().unwrap();
    let builtin = claude.path().join("skills/review-v2-lenses");
    fs::create_dir_all(builtin.join("lenses")).unwrap();
    fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
    fs::write(builtin.join("lenses/correctness.md"), CORRECTNESS).unwrap();
    let with_claude_dir = || {
        let mut cmd = repo.tomlctl();
        cmd.env("CLAUDE_CONFIG_DIR", claude.path());
        cmd
    };
    with_claude_dir()
        .args([
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
        ])
        .arg(format!("--base={}", repo.first))
        .assert()
        .success();
    let catalogue = &repo.run(0)["catalogue"];
    assert!(
        catalogue.get("builtin").is_none(),
        "no machine-specific path in the store: {catalogue}"
    );
    with_claude_dir()
        .args(["review", "survey", "--scope", SCOPE, "--run", "U1"])
        .assert()
        .success();
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
    let stderr = repo.start_fails(&repo.first, &["--project", &repo.project()]);
    assert!(
        stderr.contains("config.toml") && stderr.contains("no-such-lens"),
        "names the offending file and id: {stderr}"
    );
    assert!(!repo.store().exists(), "no run is minted");
    assert!(!repo.runs_dir().exists(), "no run directory is made");
}

#[cfg(unix)]
#[test]
fn run_start_refuses_a_runs_root_linked_outside_the_repo() {
    let Some(repo) = repo() else { return };
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), repo.root.join(".review-v2-runs")).unwrap();
    let stderr = repo.start_fails(&repo.first, &[]);
    assert!(stderr.contains(".review-v2-runs/"), "{stderr}");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(!repo.store().exists(), "no run is minted");
}

#[cfg(unix)]
#[test]
fn run_start_refuses_a_dangling_runs_root_link() {
    let Some(repo) = repo() else { return };
    let outside = tempfile::tempdir().unwrap();
    let absent = outside.path().join("absent");
    std::os::unix::fs::symlink(&absent, repo.root.join(".review-v2-runs")).unwrap();
    repo.start_fails(&repo.first, &[]);
    assert!(!absent.exists());
    assert!(!repo.store().exists(), "no run is minted");
}

#[cfg(unix)]
#[test]
fn run_start_refuses_a_store_key_directory_linked_outside_the_repo() {
    let Some(repo) = repo() else { return };
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.runs_dir().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(outside.path(), repo.runs_dir()).unwrap();
    repo.start_fails(&repo.first, &[]);
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn run_start_never_writes_through_a_dangling_ignore_link() {
    let Some(repo) = repo() else { return };
    let outside = tempfile::tempdir().unwrap();
    let absent = outside.path().join("absent");
    fs::create_dir_all(repo.root.join(".review-v2-runs")).unwrap();
    std::os::unix::fs::symlink(&absent, repo.root.join(".review-v2-runs/.gitignore")).unwrap();
    repo.start(&repo.first, &[]);
    assert!(!absent.exists());
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
fn run_start_records_head_and_measures_to_it() {
    let Some(repo) = repo() else { return };
    let second = git(&repo.root, &["rev-parse", "HEAD"]);
    fs::write(
        repo.root.join("src/a.rs"),
        "one\nTWO\nthree\nfour\nfive\nsix\n",
    )
    .unwrap();
    git(&repo.root, &["add", "src/a.rs"]);
    git(&repo.root, &["commit", "-q", "-m", "third"]);
    let scope = ["--set-json", r#"scope=["src/**"]"#];

    let bounded = repo.start(
        &repo.first,
        &[&scope[..], &["--head", "HEAD~1"][..]].concat(),
    );
    assert_eq!(bounded["head"], second.as_str(), "printed as the sha");
    assert_eq!(
        bounded["changed_lines"], 3,
        "the commit after head is not measured"
    );
    assert_eq!(repo.run(0)["head"].as_str(), Some(second.as_str()));

    let open = repo.start(&repo.first, &scope);
    assert!(open["head"].is_null(), "no --head prints null: {open}");
    assert_eq!(
        open["changed_lines"], 5,
        "without head the run measures to HEAD"
    );
    assert!(
        repo.run(1).get("head").is_none(),
        "no --head stores no head"
    );
}

#[test]
fn run_start_refuses_head_not_descended_from_base() {
    let Some(repo) = repo() else { return };
    let stderr = repo.start_fails("HEAD", &["--head", &repo.first]);
    assert!(stderr.contains("does not descend"), "{stderr}");
    assert!(!repo.store().exists(), "no run is minted");

    let stderr = repo.start_fails(&repo.first, &["--head=--all"]);
    assert!(
        stderr.contains("--head") && stderr.contains("does not name a commit"),
        "{stderr}"
    );
}

#[test]
fn run_ids_never_reuse_a_removed_id() {
    let Some(repo) = repo() else { return };
    repo.start(&repo.first, &[]);
    repo.start(&repo.first, &[]);
    repo.tomlctl()
        .args(["items", "remove", "--raw-store"])
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
fn run_list_orders_by_id_and_filters_open_and_finished() {
    let Some(repo) = repo() else { return };
    for _ in 0..10 {
        repo.start(&repo.first, &[]);
    }
    repo.update("U2", "agents=4").success();
    repo.tomlctl()
        .args([
            "review",
            "run",
            "finish",
            "U2",
            "--scope",
            SCOPE,
            "--incomplete",
            "fixture",
        ])
        .assert()
        .success();
    let list = |extra: &[&str]| -> JsonValue {
        let out = repo
            .tomlctl()
            .args(["review", "run", "list", "--scope", SCOPE])
            .args(extra)
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("run list prints JSON")
    };
    let ids = |report: &JsonValue| -> Vec<String> {
        report["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect()
    };

    let all = list(&[]);
    assert_eq!(all["ok"], true);
    let expected: Vec<String> = (1..=10).map(|n| format!("U{n}")).collect();
    assert_eq!(ids(&all), expected, "ordered by id number, not as text");
    let u2 = &all["runs"][1];
    assert_eq!(u2["command"], "review-v2");
    assert_eq!(u2["family"], "code");
    assert_eq!(u2["depth"], "quick");
    assert_eq!(u2["base"].as_str(), Some(repo.first.as_str()));
    assert_eq!(u2["agents"], 4);
    assert_eq!(u2["incomplete"], "fixture");
    assert!(u2["finished"].is_string(), "{u2}");
    assert!(
        all["runs"][0]["finished"].is_null(),
        "an open run reads null"
    );

    let finished = list(&["--finished"]);
    assert_eq!(ids(&finished), ["U2"]);
    let open = list(&["--open"]);
    assert_eq!(ids(&open).len(), 9);
    assert!(!ids(&open).contains(&"U2".to_string()));

    repo.tomlctl()
        .args([
            "review",
            "run",
            "list",
            "--scope",
            SCOPE,
            "--open",
            "--finished",
        ])
        .assert()
        .failure();
}

#[test]
fn run_list_reports_waves_from_cells() {
    let Some(repo) = repo() else { return };
    repo.start(&repo.first, &[]);
    repo.start(&repo.first, &[]);
    let mut store = repo.read_store();
    let cell = |id: &str, run: &str, wave: i64| -> toml::Value {
        toml::from_str(&format!(
            "id = \"{id}\"\nrun = \"{run}\"\nwave = {wave}\nslice = [\"src/a.rs\"]\nstatus = \"assigned\"\n"
        ))
        .unwrap()
    };
    let table = store.as_table_mut().unwrap();
    table.insert(
        "cells".into(),
        toml::Value::Array(vec![
            cell("C1", "U1", 1),
            cell("C2", "U1", 3),
            cell("C3", "U2", 2),
        ]),
    );
    table["runs"][0]
        .as_table_mut()
        .unwrap()
        .insert("waves".into(), toml::Value::Integer(9));
    fs::write(repo.store(), toml::to_string(&store).unwrap()).unwrap();

    let out = repo
        .tomlctl()
        .args(["review", "run", "list", "--scope", SCOPE])
        .assert()
        .success();
    let report: JsonValue = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let waves: Vec<&JsonValue> = report["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["waves"])
        .collect();
    assert_eq!(
        waves,
        [&json!(3), &json!(2)],
        "the highest wave among each run's cells"
    );
    assert!(
        report["runs"][0]["head"].is_null(),
        "every row carries head"
    );
}

#[test]
fn run_list_refuses_a_missing_store() {
    let Some(repo) = repo() else { return };
    let out = repo
        .tomlctl()
        .args(["--error-format", "json"])
        .args(["review", "run", "list", "--scope", SCOPE])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("not_found"), "{stderr}");
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
