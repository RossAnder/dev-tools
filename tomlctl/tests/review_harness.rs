//! Shared fixtures for the review cell tests: the git sandbox repo, the fixture
//! catalogue and the row helpers.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, git_available, git_command, sandbox};
use serde_json::Value as JsonValue;

pub(crate) const SCOPE: &str = "t";

pub(crate) const VOCABULARY: &str = r#"
[signals."lang:rust"]
kind = "stack"
meaning = "Rust crates"
detect = { files = ["**/Cargo.toml"] }

[signals."framework:vue"]
kind = "stack"
meaning = "Vue apps"
detect = { files = ["**/*.vue"] }

[signals."docs-touched"]
kind = "diff"
meaning = "prose changed"
detect = { files = ["**/*.md"] }

[signals."schema-change"]
kind = "diff"
meaning = "a migration changed"
detect = { files = ["**/migrations/**"] }

[signals."churn-hotspot"]
kind = "health"
meaning = "files changed often"
detect = { files = ["**"], window = 90, threshold = 3 }

[signals."trust-boundary"]
kind = "judged"
meaning = "input crosses a boundary"
hint = "parsers and auth"

[concerns.logic]
meaning = "wrong output"
families = ["code"]

[concerns.ripple]
meaning = "stale callers"
families = ["code"]

[concerns.exploit]
meaning = "an exploitable input"
families = ["code"]

[concerns.pattern]
meaning = "off the converged pattern"
families = ["code"]

[concerns.dead-code]
meaning = "unused code"
families = ["code"]

[concerns.comment-rot]
meaning = "prose against behaviour"
families = ["code"]

[concerns.schema]
meaning = "data model drift"
families = ["code"]

[concerns.plan-shape]
meaning = "plan structure"
families = ["plan"]

[concerns.executability]
meaning = "a task an agent cannot execute"
families = ["plan"]
"#;

pub(crate) fn lens(
    id: &str,
    family: &str,
    mode: &str,
    activation: &str,
    slice: &str,
    tag: &str,
    extra: &str,
) -> String {
    let produces = if mode == "mechanical" {
        r#"["cleanup"]"#
    } else {
        r#"["defect", "cleanup", "advisory"]"#
    };
    let v1 = if family == "code" {
        "v1_category = \"quality\"\n"
    } else {
        ""
    };
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"{family}\"]\nmode = \"{mode}\"\n\
         produces = {produces}\nowns = [\"{tag}\"]\nactivation = \"{activation}\"\nslice = \"{slice}\"\n\
         {v1}{extra}+++\n## Question\nWhat does {id} ask?\n\n## Owns\n`{tag}` — owned here.\n\n\
         ## Probes — {family}\n- one probe\n\n## Does not flag\n- confirmations\n"
    )
}

pub(crate) fn check(id: &str) -> String {
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"plan\"]\nexecutor = \"cell\"\n\
         produces = [\"defect\"]\nactivation = \"floor\"\n+++\n## Runs\nA check.\n\n\
         ## Interpreting the result\nRead it.\n"
    )
}

pub(crate) fn facet(id: &str, signal: &str) -> String {
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"code\"]\n\
         apply_when = {{ signals = [\"{signal}\"] }}\nattaches_to = [\"correctness\"]\n+++\n\
         ## General\nStack notes.\n\n### For correctness\n- a pitfall\n"
    )
}

/// The fixture catalogue: two floor lenses per family, signal and candidate
/// lenses of both code modes, one plan executor lens, two cell-run checks,
/// and two root-scoped facets.
pub(crate) fn write_catalogue(builtin: &Path) {
    let entries = [
        (
            "lenses/correctness.md",
            lens(
                "correctness",
                "code",
                "judgement",
                "floor",
                "per-subsystem",
                "logic",
                "",
            ),
        ),
        (
            "lenses/ripple.md",
            lens(
                "ripple",
                "code",
                "mechanical",
                "floor",
                "whole-scope",
                "ripple",
                "",
            ),
        ),
        (
            "lenses/security.md",
            lens(
                "security",
                "code",
                "judgement",
                "signal",
                "per-subsystem",
                "exploit",
                "apply_when = { signals = [\"trust-boundary\"] }\n",
            ),
        ),
        (
            "lenses/domain-model.md",
            lens(
                "domain-model",
                "code",
                "judgement",
                "signal",
                "per-subsystem",
                "schema",
                "apply_when = { signals = [\"schema-change\"] }\n",
            ),
        ),
        (
            "lenses/idioms.md",
            lens(
                "idioms",
                "code",
                "judgement",
                "candidate",
                "whole-scope",
                "pattern",
                "",
            ),
        ),
        (
            "lenses/hygiene.md",
            lens(
                "hygiene",
                "code",
                "mechanical",
                "candidate",
                "whole-scope",
                "dead-code",
                "",
            ),
        ),
        (
            "lenses/doc-drift.md",
            lens(
                "doc-drift",
                "code",
                "judgement",
                "candidate",
                "per-file",
                "comment-rot",
                "apply_when = { signals = [\"docs-touched\"] }\n",
            ),
        ),
        (
            "lenses/plan-structure.md",
            lens(
                "plan-structure",
                "plan",
                "judgement",
                "floor",
                "whole-scope",
                "plan-shape",
                "",
            ),
        ),
        (
            "lenses/plan-executor.md",
            lens(
                "plan-executor",
                "plan",
                "executor",
                "floor",
                "per-task",
                "executability",
                "",
            ),
        ),
        ("checks/falsifiability.md", check("falsifiability")),
        ("checks/two-control-probe.md", check("two-control-probe")),
        ("facets/rust.md", facet("rust", "lang:rust")),
        ("facets/vue.md", facet("vue", "framework:vue")),
    ];
    for (rel, text) in entries {
        let path = builtin.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
}

pub(crate) struct Repo {
    pub(crate) _dir: tempfile::TempDir,
    pub(crate) _catalogue: tempfile::TempDir,
    pub(crate) root: PathBuf,
    pub(crate) builtin: PathBuf,
}

/// A run row as `run start` would leave it, with the signals a test needs.
pub(crate) struct RunSpec<'a> {
    pub(crate) id: &'a str,
    pub(crate) family: &'a str,
    pub(crate) depth: &'a str,
    pub(crate) changed_lines: Option<i64>,
    pub(crate) scope: &'a [&'a str],
    pub(crate) signals: &'a str,
}

impl Repo {
    /// A git sandbox holding `files`, all tracked, and the fixture catalogue.
    pub(crate) fn new(files: &[&str]) -> Option<Self> {
        if !git_available() {
            eprintln!("skipping: git is not available");
            return None;
        }
        let (dir, root) = sandbox();
        let catalogue = tempfile::tempdir().unwrap();
        let builtin = catalogue.path().join("review-v2-lenses");
        write_catalogue(&builtin);
        for rel in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "fn f() {}\n").unwrap();
        }
        let out = git_command(&root).args(["add", "-A"]).output().unwrap();
        assert!(
            out.status.success(),
            "git add: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
            builtin,
        })
    }

    pub(crate) fn run_row(&self, run: &RunSpec) -> String {
        let scope: Vec<String> = run.scope.iter().map(|s| format!("{s:?}")).collect();
        let changed = run
            .changed_lines
            .map(|n| format!("changed_lines = {n}\n"))
            .unwrap_or_default();
        format!(
            "[[runs]]\nid = \"{}\"\ncommand = \"review-v2\"\nfamily = \"{}\"\ndepth = \"{}\"\n\
             started = \"2026-10-12T10:00:00Z\"\nbase = \"abc\"\nscope = [{}]\n{changed}\
             signals = {}\ncatalogue = {{ builtin = \"{}\" }}\n\n",
            run.id,
            run.family,
            run.depth,
            scope.join(", "),
            run.signals,
            self.builtin.to_string_lossy().replace('\\', "/"),
        )
    }

    /// Seeds the `--scope t` store with `runs`.
    pub(crate) fn seed(&self, runs: &[RunSpec]) -> PathBuf {
        let path = self
            .root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"));
        self.seed_at(&path, runs);
        path
    }

    pub(crate) fn seed_at(&self, path: &Path, runs: &[RunSpec]) {
        let mut text = String::from("schema_version = 1\n\n");
        for run in runs {
            text.push_str(&self.run_row(run));
        }
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub(crate) fn tomlctl(&self) -> common::Command {
        let mut cmd = cli(&self.root);
        cmd.env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        cmd
    }

    /// `review plan --propose`: the cell rows and the trailing summary.
    pub(crate) fn propose(
        &self,
        store: &[&str],
        run: &str,
        extra: &[&str],
    ) -> (Vec<JsonValue>, JsonValue) {
        let out = self
            .tomlctl()
            .args(["review", "plan", "--run", run, "--propose"])
            .args(store)
            .args(extra)
            .assert()
            .success();
        let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
        let mut rows: Vec<JsonValue> = stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("every line is JSON"))
            .collect();
        let summary = rows.pop().expect("a summary line");
        assert!(
            summary.get("proposal").is_some(),
            "the last line is the summary: {summary}"
        );
        (rows, summary["proposal"].clone())
    }

    pub(crate) fn payload(&self, rows: &[JsonValue]) -> PathBuf {
        let path = self.root.join(".claude/payload.ndjson");
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        fs::write(&path, text).unwrap();
        path
    }

    pub(crate) fn plan_cmd(&self, run: &str, rows: &[JsonValue]) -> assert_cmd::assert::Assert {
        let payload = self.payload(rows);
        self.tomlctl()
            .args(["review", "plan", "--scope", SCOPE, "--run", run, "--ndjson"])
            .arg(payload)
            .assert()
    }

    pub(crate) fn plan(&self, run: &str, rows: &[JsonValue]) -> JsonValue {
        let out = self.plan_cmd(run, rows).success();
        serde_json::from_slice(&out.get_output().stdout).expect("plan prints JSON")
    }

    pub(crate) fn cover(&self, cell: &str, status: &str, note: &str) -> assert_cmd::assert::Assert {
        self.tomlctl()
            .args([
                "review", "cover", cell, "--scope", SCOPE, "--status", status, "--note", note,
            ])
            .assert()
    }

    pub(crate) fn store(&self) -> toml::Value {
        let path = self
            .root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"));
        toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Stamps the store's first run finished and returns the store's bytes.
    pub(crate) fn finish_first_run(&self, path: &Path) -> Vec<u8> {
        let mut doc = self.store();
        doc["runs"][0]
            .as_table_mut()
            .unwrap()
            .insert("finished".into(), "2026-10-12T11:00:00Z".into());
        fs::write(path, toml::to_string(&doc).unwrap()).unwrap();
        fs::read(path).unwrap()
    }
}

pub(crate) fn concerns(row: &JsonValue) -> Vec<String> {
    row["concerns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect()
}

pub(crate) fn strings(value: &JsonValue) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect()
}

pub(crate) const STORE: &[&str] = &["--scope", SCOPE];

pub(crate) fn ceiling_cells(rows: &[JsonValue]) -> usize {
    rows.iter()
        .filter(|r| r["agent"] != "plan-executor")
        .count()
}

pub(crate) fn with_concern<'a>(rows: &'a [JsonValue], id: &str) -> Vec<&'a JsonValue> {
    rows.iter()
        .filter(|r| concerns(r).contains(&id.to_string()))
        .collect()
}

pub(crate) fn ceiling_cut(summary: &JsonValue, concern: &str) -> bool {
    summary["unselected"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["concern"] == concern && u["reason"] == "ceiling")
}

/// `count` files directly under `src/`, named in path order.
pub(crate) fn flat_files(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("src/f{i:03}.rs")).collect()
}
