//! `tomlctl review plan` (`--propose` and `--ndjson`) and `review cover`
//! against sandbox repos and a temp-dir catalogue. Stores are seeded by hand
//! with the run row a test needs, so no test depends on `review survey`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, git_available, git_command, sandbox};
use serde_json::{Value as JsonValue, json};

const SCOPE: &str = "t";

const VOCABULARY: &str = r#"
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

fn lens(
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

fn check(id: &str) -> String {
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"plan\"]\nexecutor = \"cell\"\n\
         produces = [\"defect\"]\nactivation = \"floor\"\n+++\n## Runs\nA check.\n\n\
         ## Interpreting the result\nRead it.\n"
    )
}

fn facet(id: &str, signal: &str) -> String {
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"code\"]\n\
         apply_when = {{ signals = [\"{signal}\"] }}\nattaches_to = [\"correctness\"]\n+++\n\
         ## General\nStack notes.\n\n### For correctness\n- a pitfall\n"
    )
}

/// The fixture catalogue: two floor lenses per family, signal and candidate
/// lenses of both code modes, one plan executor lens, two cell-run checks,
/// and two root-scoped facets.
fn write_catalogue(builtin: &Path) {
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

struct Repo {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
    builtin: PathBuf,
}

/// A run row as `run start` would leave it, with the signals a test needs.
struct RunSpec<'a> {
    id: &'a str,
    family: &'a str,
    depth: &'a str,
    changed_lines: Option<i64>,
    scope: &'a [&'a str],
    signals: &'a str,
}

impl Repo {
    /// A git sandbox holding `files`, all tracked, and the fixture catalogue.
    fn new(files: &[&str]) -> Option<Self> {
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

    fn run_row(&self, run: &RunSpec) -> String {
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
    fn seed(&self, runs: &[RunSpec]) -> PathBuf {
        let path = self
            .root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"));
        self.seed_at(&path, runs);
        path
    }

    fn seed_at(&self, path: &Path, runs: &[RunSpec]) {
        let mut text = String::from("schema_version = 1\n\n");
        for run in runs {
            text.push_str(&self.run_row(run));
        }
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

    /// `review plan --propose`: the cell rows and the trailing summary.
    fn propose(&self, store: &[&str], run: &str, extra: &[&str]) -> (Vec<JsonValue>, JsonValue) {
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

    fn payload(&self, rows: &[JsonValue]) -> PathBuf {
        let path = self.root.join(".claude/payload.ndjson");
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        fs::write(&path, text).unwrap();
        path
    }

    fn plan_cmd(&self, run: &str, rows: &[JsonValue]) -> assert_cmd::assert::Assert {
        let payload = self.payload(rows);
        self.tomlctl()
            .args(["review", "plan", "--scope", SCOPE, "--run", run, "--ndjson"])
            .arg(payload)
            .assert()
    }

    fn plan(&self, run: &str, rows: &[JsonValue]) -> JsonValue {
        let out = self.plan_cmd(run, rows).success();
        serde_json::from_slice(&out.get_output().stdout).expect("plan prints JSON")
    }

    fn cover(&self, cell: &str, status: &str, note: &str) -> assert_cmd::assert::Assert {
        self.tomlctl()
            .args([
                "review", "cover", cell, "--scope", SCOPE, "--status", status, "--note", note,
            ])
            .assert()
    }

    fn store(&self) -> toml::Value {
        let path = self
            .root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"));
        toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Stamps the store's first run finished and returns the store's bytes.
    fn finish_first_run(&self, path: &Path) -> Vec<u8> {
        let mut doc = self.store();
        doc["runs"][0]
            .as_table_mut()
            .unwrap()
            .insert("finished".into(), "2026-10-12T11:00:00Z".into());
        fs::write(path, toml::to_string(&doc).unwrap()).unwrap();
        fs::read(path).unwrap()
    }
}

fn concerns(row: &JsonValue) -> Vec<String> {
    row["concerns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect()
}

fn strings(value: &JsonValue) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect()
}

const STORE: &[&str] = &["--scope", SCOPE];

#[test]
fn propose_includes_floor_and_matched_signal_lenses() {
    let Some(repo) = Repo::new(&["src/a.rs", "src/b.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(600),
        scope: &["src/**"],
        signals: r#"[{ name = "lang:rust", source = "detector", roots = ["src"] }, { name = "trust-boundary", source = "judged", evidence = ["src/a.rs"] }]"#,
    }]);
    let (rows, summary) = repo.propose(STORE, "U1", &["--accept-signals", "trust-boundary"]);
    let all: Vec<String> = rows.iter().flat_map(concerns).collect();
    for expected in ["correctness", "ripple", "security"] {
        assert!(
            all.contains(&expected.to_string()),
            "{expected} missing from {all:?}"
        );
    }
    assert!(
        !all.contains(&"domain-model".to_string()),
        "an unmatched signal lens is not proposed: {all:?}"
    );
    for row in &rows {
        assert_eq!(row["reason"], "proposed", "{row}");
    }
    let finder = rows
        .iter()
        .find(|r| concerns(r).contains(&"correctness".to_string()))
        .unwrap();
    assert_eq!(finder["agent"], "review-finder");
    assert_eq!(strings(&finder["slice"]), vec!["src/**"]);
    let sweeper = rows
        .iter()
        .find(|r| concerns(r).contains(&"ripple".to_string()))
        .unwrap();
    assert_eq!(sweeper["agent"], "review-sweeper");
    let n = rows.len() as u64;
    assert_eq!(summary["agents"], json!(2 * n + 1 + (5 * n).div_ceil(20)));

    let (rows, _) = repo.propose(STORE, "U1", &[]);
    let all: Vec<String> = rows.iter().flat_map(concerns).collect();
    assert!(
        !all.contains(&"security".to_string()),
        "an undecided judged candidate activates nothing: {all:?}"
    );
}

#[test]
fn propose_respects_scaled_ceiling() {
    let Some(repo) = Repo::new(&["src/a/x.rs", "src/b/y.rs", "src/c/z.rs", "src/d/w.rs"]) else {
        return;
    };
    let signals = r#"[{ name = "docs-touched", source = "detector", roots = ["src"] }]"#;
    repo.seed(&[
        RunSpec {
            id: "U1",
            family: "code",
            depth: "quick",
            changed_lines: Some(250),
            scope: &["src/**"],
            signals,
        },
        RunSpec {
            id: "U2",
            family: "code",
            depth: "quick",
            changed_lines: Some(900),
            scope: &["src/**"],
            signals,
        },
    ]);

    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert_eq!(
        summary["ceiling"], 3,
        "max(3, ceil(250 / 100)) = 3: {summary}"
    );
    assert!(rows.len() <= 3, "{} cells over a ceiling of 3", rows.len());
    assert!(
        !rows
            .iter()
            .any(|r| concerns(r).contains(&"doc-drift".to_string())),
        "doc-drift's four per-file cells cannot fit"
    );
    let unselected = &summary["unselected"];
    assert!(
        unselected
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["concern"] == "doc-drift" && u["reason"] == "ceiling"),
        "a matched candidate past the ceiling is listed: {unselected}"
    );

    let (rows, summary) = repo.propose(STORE, "U2", &[]);
    assert_eq!(
        summary["ceiling"], 6,
        "min(6, ceil(900 / 100)) = 6: {summary}"
    );
    let drift: Vec<Vec<String>> = rows
        .iter()
        .filter(|r| concerns(r).contains(&"doc-drift".to_string()))
        .map(|r| strings(&r["slice"]))
        .collect();
    assert_eq!(
        drift,
        vec![
            vec!["src/a/*"],
            vec!["src/b/*"],
            vec!["src/c/*"],
            vec!["src/d/*"]
        ],
        "per-file slices group by directory"
    );
    assert!(rows.len() <= 6);
}

#[test]
fn propose_scope_without_diff_uses_line_count() {
    if !git_available() {
        eprintln!("skipping: git is not available");
        return;
    }
    let (_dir, root) = sandbox();
    let catalogue = tempfile::tempdir().unwrap();
    let builtin = catalogue.path().join("review-v2-lenses");
    write_catalogue(&builtin);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/big.rs"), "// line\n".repeat(450)).unwrap();
    let git = |args: &[&str]| {
        let out = git_command(&root)
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
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "seed"]);
    let tomlctl = || {
        let mut cmd = cli(&root);
        cmd.current_dir(&root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        cmd
    };
    tomlctl()
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
            "--base=HEAD",
            "--set-json",
            r#"scope=["src/**"]"#,
            "--builtin",
        ])
        .arg(&builtin)
        .assert()
        .success();
    let out = tomlctl()
        .args([
            "review",
            "plan",
            "--scope",
            SCOPE,
            "--run",
            "U1",
            "--propose",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let summary: JsonValue = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert_eq!(summary["proposal"]["changed_lines"], 450, "{summary}");
    assert_eq!(
        summary["proposal"]["ceiling"], 5,
        "an unchanged scope sizes the ceiling by its 450 tracked lines"
    );
}

#[test]
fn propose_bundles_same_mode_concerns_only() {
    let Some(repo) = Repo::new(&["src/a.rs", "src/b.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(600),
        scope: &["src/**"],
        signals: "[]",
    }]);
    let (rows, _) = repo.propose(STORE, "U1", &[]);
    let with = |id: &str| {
        rows.iter()
            .find(|r| concerns(r).contains(&id.to_string()))
            .unwrap_or_else(|| panic!("{id} not proposed: {rows:?}"))
    };
    assert_eq!(concerns(with("correctness")), vec!["correctness", "idioms"]);
    assert_eq!(with("correctness")["agent"], "review-finder");
    assert_eq!(concerns(with("ripple")), vec!["ripple", "hygiene"]);
    assert_eq!(with("ripple")["agent"], "review-sweeper");
    assert_eq!(
        concerns(with("doc-drift")),
        vec!["doc-drift"],
        "a lens sliced differently gets its own cell"
    );
    let mechanical = ["ripple", "hygiene"];
    for row in &rows {
        let ids = concerns(row);
        let mixed = ids.iter().any(|c| mechanical.contains(&c.as_str()))
            && ids.iter().any(|c| !mechanical.contains(&c.as_str()));
        assert!(!mixed, "a cell mixes modes: {ids:?}");
    }
}

#[test]
fn propose_adds_reverse_order_repeat_cells_at_standard() {
    let Some(repo) = Repo::new(&["src/a.rs", "src/b.rs", "lib/c.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "standard",
        changed_lines: Some(2000),
        scope: &["src/**", "lib/**"],
        signals: r#"[{ name = "churn-hotspot", source = "detector", roots = ["src"], evidence = ["src/a.rs"] }]"#,
    }]);
    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert_eq!(strings(&summary["hotspots"]), vec!["src/a.rs"]);
    let firsts: Vec<&JsonValue> = rows.iter().filter(|r| r.get("pass").is_none()).collect();
    let repeats: Vec<&JsonValue> = rows.iter().filter(|r| r["pass"] == 2).collect();

    let correctness_slices: Vec<Vec<String>> = firsts
        .iter()
        .filter(|r| concerns(r).contains(&"correctness".to_string()))
        .map(|r| strings(&r["slice"]))
        .collect();
    assert!(
        correctness_slices.contains(&vec!["src/a.rs".to_string()]),
        "the hotspot is sliced on its own: {correctness_slices:?}"
    );

    let is_floor = |r: &JsonValue| {
        let ids = concerns(r);
        ids.contains(&"correctness".to_string()) || ids.contains(&"ripple".to_string())
    };
    let holds_hotspot = |r: &JsonValue| {
        let slice = strings(&r["slice"]);
        slice.contains(&"src/a.rs".to_string()) || slice.contains(&"src/**".to_string())
    };
    let due: Vec<&&JsonValue> = firsts
        .iter()
        .filter(|r| is_floor(r) || holds_hotspot(r))
        .collect();
    assert!(
        firsts.iter().any(|r| !is_floor(r) && holds_hotspot(r)),
        "a non-floor cell holding the hotspot is in the fixture: {rows:?}"
    );
    assert!(
        firsts.iter().any(|r| !is_floor(r) && !holds_hotspot(r)),
        "a cell owed no repeat is in the fixture: {rows:?}"
    );
    assert_eq!(
        repeats.len(),
        due.len(),
        "one repeat per floor or hotspot cell: {rows:?}"
    );
    for first in due {
        let mut reversed = strings(&first["slice"]);
        reversed.reverse();
        assert!(
            repeats
                .iter()
                .any(|r| concerns(r) == concerns(first) && strings(&r["slice"]) == reversed),
            "no reverse-order repeat of {first}"
        );
    }
    let ripple_repeat = repeats
        .iter()
        .find(|r| concerns(r).contains(&"ripple".to_string()))
        .unwrap();
    assert_eq!(strings(&ripple_repeat["slice"]), vec!["lib/**", "src/**"]);
}

#[test]
fn plan_executor_cells_batch_tasks_outside_ceiling() {
    let Some(repo) = Repo::new(&["docs/plan.md"]) else {
        return;
    };
    let path = repo.root.join(".claude/flows/pf/review-v2.toml");
    repo.seed_at(
        &path,
        &[RunSpec {
            id: "U1",
            family: "plan",
            depth: "standard",
            changed_lines: None,
            scope: &["docs/plan.md"],
            signals: "[]",
        }],
    );
    let task = |id: u32, effort: &str, checkpoint: &str, needs: &str| {
        format!(
            "[[items]]\nid = {id}\nref = \"task-{id}\"\ntitle = \"Task {id}\"\neffort = \"{effort}\"\n\
             status = \"pending\"\ncheckpoint = \"{checkpoint}\"\nneeds = [{needs}]\n\n"
        )
    };
    let tasks = [
        task(1, "S", "A", ""),
        task(2, "S", "A", "1"),
        task(3, "M", "", ""),
        task(4, "L", "", ""),
        task(5, "S", "", ""),
        task(6, "S", "", ""),
    ]
    .concat();
    fs::write(
        repo.root.join(".claude/flows/pf/tasks.toml"),
        format!("schema_version = 1\n\n{tasks}"),
    )
    .unwrap();

    let (rows, summary) = repo.propose(&["--slug", "pf"], "U1", &[]);
    let executor: Vec<Vec<String>> = rows
        .iter()
        .filter(|r| r["agent"] == "plan-executor")
        .map(|r| strings(&r["slice"]))
        .collect();
    assert_eq!(
        executor,
        vec![
            vec!["task-4", "task-3", "task-2", "task-1"],
            vec!["task-5", "task-6"],
        ],
        "L, then M, then checkpoint-maximal, then by id; at most 4 per cell"
    );
    assert_eq!(summary["executor_cells"], 2);
    let counted = rows
        .iter()
        .filter(|r| r["agent"] != "plan-executor")
        .count();
    assert_eq!(
        summary["cells"],
        json!(counted),
        "executor cells sit outside the ceiling count"
    );
    let checks = rows
        .iter()
        .find(|r| concerns(r).iter().any(|c| c.starts_with("check:")))
        .expect("the cell-run checks are proposed");
    assert_eq!(
        concerns(checks),
        vec!["check:falsifiability", "check:two-control-probe"]
    );
    assert_eq!(checks["agent"], "review-finder");
}

#[test]
fn plan_binds_hashes_and_root_scoped_facets() {
    let Some(repo) = Repo::new(&["src/a.rs", "web/app.vue"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**", "web/**"],
        signals: r#"[{ name = "lang:rust", source = "detector", roots = ["src"] }, { name = "framework:vue", source = "detector", roots = ["web"] }]"#,
    }]);
    let out = repo.plan(
        "U1",
        &[
            json!({"slice": ["src/a.rs"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"}),
            json!({"slice": ["web/app.vue"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"}),
        ],
    );
    assert_eq!(out["wave"], 1);
    assert_eq!(out["cells"][0]["id"], "C1");
    assert_eq!(strings(&out["cells"][0]["facets"]), vec!["rust"]);
    assert_eq!(strings(&out["cells"][1]["facets"]), vec!["vue"]);

    let store = repo.store();
    let cell = &store["cells"][0];
    assert_eq!(cell["status"].as_str(), Some("assigned"));
    assert_eq!(cell["wave"].as_integer(), Some(1));
    let bound = &cell["bound"][0];
    assert_eq!(bound["concern"].as_str(), Some("correctness"));
    let hash = bound["hash"].as_str().unwrap();
    assert_eq!(hash.len(), 64, "a sha256 content hash: {hash}");
    assert!(
        bound["path"]
            .as_str()
            .unwrap()
            .ends_with("lenses/correctness.md"),
        "{bound:?}"
    );
    assert_eq!(bound["v1_category"].as_str(), Some("quality"));
}

#[test]
fn plan_refuses_mixed_mode_bundle() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    let mixed = json!({"slice": ["src/a.rs"], "concerns": ["correctness", "ripple"], "agent": "review-finder", "reason": "bundled"});
    let out = repo.plan_cmd("U1", std::slice::from_ref(&mixed)).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("one mode"), "{stderr}");
    assert!(repo.store().get("cells").is_none(), "nothing is written");

    let good = json!({"slice": ["src/a.rs"], "concerns": ["ripple"], "agent": "review-sweeper", "reason": "proposed"});
    let out = repo.plan("U1", &[mixed, good]);
    assert_eq!(out["cells"].as_array().unwrap().len(), 1, "{out}");
    assert_eq!(out["refused"][0]["line"], 1, "{out}");
}

#[test]
fn plan_refuses_a_finished_run() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    let path = repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    let before = repo.finish_first_run(&path);
    let row = json!({"slice": ["src/a.rs"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"});
    let out = repo.plan_cmd("U1", &[row]).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("run U1 is finished"), "{stderr}");
    assert_eq!(fs::read(&path).unwrap(), before, "nothing is written");
}

#[test]
fn probe_cell_requires_question() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    let probe = json!({"slice": ["src/a.rs"], "concerns": ["probe:retry-cap"], "agent": "review-finder", "reason": "nearest logic: retries are unbounded"});
    let out = repo.plan_cmd("U1", std::slice::from_ref(&probe)).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("`question` is required"), "{stderr}");

    let mut asked = probe;
    asked["question"] = json!("Can the retry loop spin forever when the lock is never released?");
    let out = repo.plan("U1", &[asked]);
    assert_eq!(out["cells"][0]["concerns"][0], "probe:retry-cap");
    let store = repo.store();
    assert!(
        store["cells"][0].get("bound").is_none(),
        "a probe binds no lens"
    );
    assert!(
        store["cells"][0]["question"]
            .as_str()
            .unwrap()
            .starts_with("Can the retry")
    );
}

#[test]
fn accounting_lists_unselected_and_unsliced_until_waived() {
    let Some(repo) = Repo::new(&["src/a.rs", "src/b.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: r#"[{ name = "trust-boundary", source = "judged", accepted = true, evidence = ["src/a.rs"] }, { name = "docs-touched", source = "detector", roots = ["src"] }]"#,
    }]);
    let out = repo.plan(
        "U1",
        &[
            json!({"slice": ["src/a.rs"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"}),
            json!({"slice": ["src/a.rs"], "concerns": ["ripple"], "agent": "review-sweeper", "reason": "proposed"}),
        ],
    );
    let accounting = &out["accounting"];
    assert_eq!(
        strings(&accounting["unselected"]),
        vec!["security", "doc-drift"],
        "the matched signal lens and the matched candidate need a cell or a waiver; \
         unmatched candidates do not: {accounting}"
    );
    assert_eq!(
        strings(&accounting["unsliced"]),
        vec!["src/b.rs"],
        "{accounting}"
    );

    repo.tomlctl()
        .args(["review", "run", "update", "U1", "--scope", SCOPE, "--set-json"])
        .arg(
            r#"waivers=[{"concern":"security","reason":"no input parsing in scope"},{"concern":"doc-drift","reason":"no prose"},{"file":"src/b.rs","reason":"generated"}]"#,
        )
        .assert()
        .success();
    let out = repo.plan(
        "U1",
        &[json!({"slice": ["src/a.rs"], "concerns": ["probe:lock-order"], "question": "Can two writers deadlock?", "agent": "review-finder", "reason": "nearest logic: no lens owns lock order"})],
    );
    assert_eq!(out["wave"], 2, "a later call appends the next wave");
    assert_eq!(
        strings(&out["accounting"]["unselected"]),
        Vec::<String>::new()
    );
    assert_eq!(
        strings(&out["accounting"]["unsliced"]),
        Vec::<String>::new()
    );
}

#[test]
fn plan_ndjson_replay_is_unchanged() {
    let Some(repo) = Repo::new(&["src/a.rs", "src/b.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    let rows = [
        json!({"slice": ["src/a.rs"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"}),
        json!({"slice": ["src/b.rs"], "concerns": ["ripple"], "agent": "review-sweeper", "reason": "proposed"}),
    ];
    let ids = |out: &JsonValue| -> Vec<String> {
        out["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect()
    };
    let first = repo.plan("U1", &rows);
    assert_eq!(first["wave"], 1, "{first}");
    assert_eq!(first["unchanged"], false, "{first}");

    let replay = repo.plan("U1", &rows);
    assert_eq!(replay["wave"], 1, "a replay mints no second wave: {replay}");
    assert_eq!(replay["unchanged"], true, "{replay}");
    assert_eq!(ids(&replay), ids(&first));
    let store = repo.store();
    assert_eq!(store["cells"].as_array().unwrap().len(), 2, "{store:?}");

    let partial = repo.plan("U1", &rows[..1]);
    assert_eq!(
        partial["wave"], 2,
        "a partial overlap is a new wave: {partial}"
    );
    assert_eq!(partial["unchanged"], false, "{partial}");

    let cell = ids(&partial)[0].clone();
    repo.cover(&cell, "covered", "read in full").success();
    let after_seal = repo.plan("U1", &rows[..1]);
    assert_eq!(
        after_seal["wave"], 3,
        "a sealed cell in the newest wave is not replayed: {after_seal}"
    );
}

#[test]
fn cover_seals_the_cell() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    repo.plan(
        "U1",
        &[json!({"slice": ["src/a.rs"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"})],
    );
    let out = repo
        .cover("C1", "thin", "read a.rs; the error paths were out of reach")
        .success();
    let printed: JsonValue = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(printed["unchanged"], false);
    let store = repo.store();
    let cell = &store["cells"][0];
    assert_eq!(cell["status"].as_str(), Some("thin"));
    assert_eq!(cell["sealed"].as_bool(), Some(true));
    assert_eq!(
        cell["note"].as_str(),
        Some("read a.rs; the error paths were out of reach")
    );

    repo.cover("C1", "partial", "x").failure();
    repo.cover("C9", "covered", "x").failure();
}

#[test]
fn cover_on_sealed_cell_with_same_note_is_a_no_op() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    let path = repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    repo.plan(
        "U1",
        &[json!({"slice": ["src/a.rs"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"})],
    );
    repo.cover("C1", "covered", "read every branch").success();
    let before = fs::read(&path).unwrap();

    let out = repo.cover("C1", "covered", "read every branch").success();
    let printed: JsonValue = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(printed["unchanged"], true, "{printed}");
    assert_eq!(fs::read(&path).unwrap(), before, "a no-op writes nothing");

    let out = repo.cover("C1", "covered", "a different note").failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("already sealed"), "{stderr}");
    assert_eq!(fs::read(&path).unwrap(), before);
}

/// A note file staged with a BOM stores the note as typed, so a retry passing
/// the same note literally still matches the sealed cell.
#[test]
fn cover_note_file_drops_a_leading_bom() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    repo.plan(
        "U1",
        &[json!({"slice": ["src/a.rs"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"})],
    );
    let note_file = repo.root.join(".claude/note.txt");
    fs::write(&note_file, "\u{feff}read every branch\r\n").unwrap();
    repo.tomlctl()
        .args([
            "review",
            "cover",
            "C1",
            "--scope",
            SCOPE,
            "--status",
            "covered",
            "--note-file",
        ])
        .arg(&note_file)
        .assert()
        .success();
    let store = repo.store();
    assert_eq!(
        store["cells"][0]["note"].as_str(),
        Some("read every branch")
    );

    let out = repo.cover("C1", "covered", "read every branch").success();
    let printed: JsonValue = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(printed["unchanged"], true, "{printed}");
}

#[test]
fn cover_refuses_a_cell_of_a_finished_run() {
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    let path = repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    repo.plan(
        "U1",
        &[json!({"slice": ["src/a.rs"], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"})],
    );
    let before = repo.finish_first_run(&path);

    let out = repo.cover("C1", "covered", "read every branch").failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("run U1 is finished"), "{stderr}");
    assert_eq!(fs::read(&path).unwrap(), before, "nothing is written");
}
