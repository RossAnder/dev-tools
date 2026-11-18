//! `tomlctl review plan` (`--propose` and `--ndjson`) and `review cover`
//! against sandbox repos and a temp-dir catalogue. Stores are seeded by hand
//! with the run row a test needs, so no test depends on `review survey`.

use crate::common;

use std::fs;

use crate::review_harness::{
    Repo, RunSpec, SCOPE, STORE, ceiling_cells, ceiling_cut, concerns, flat_files, strings,
    with_concern, write_catalogue,
};
use common::{cli, git_available, git_command, sandbox};
use serde_json::{Value as JsonValue, json};

#[cfg(unix)]
#[test]
fn propose_out_lands_in_the_runs_root_and_refuses_one_linked_outside() {
    const OUT: &str = ".review-v2-runs/scope-t/U1/proposal.ndjson";
    let Some(repo) = Repo::new(&["src/a.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(10),
        scope: &["src/**"],
        signals: "[]",
    }]);
    let propose = || {
        let mut cmd = repo.tomlctl();
        cmd.args(["review", "plan", "--run", "U1", "--propose", "--out", OUT])
            .args(STORE);
        cmd
    };

    propose().assert().success();
    assert!(repo.root.join(OUT).is_file());

    fs::remove_dir_all(repo.root.join(".review-v2-runs")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), repo.root.join(".review-v2-runs")).unwrap();
    propose().assert().failure();
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);

    fs::create_dir_all(outside.path().join("scope-t/U1")).unwrap();
    propose().arg("--allow-outside").assert().success();
    assert!(outside.path().join("scope-t/U1/proposal.ndjson").is_file());
}

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
    let drift = with_concern(&rows, "doc-drift");
    assert_eq!(
        drift.len(),
        1,
        "doc-drift's four per-file slices merge into the one cell left: {rows:?}"
    );
    assert!(!ceiling_cut(&summary, "doc-drift"), "{summary}");

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
fn propose_after_a_planned_wave_lists_only_uncovered_concerns_as_ceiling_cuts() {
    let Some(repo) = Repo::new(&["src/a/x.rs", "src/b/y.rs", "src/c/z.rs", "src/d/w.rs"]) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(250),
        scope: &["src/**"],
        signals: r#"[{ name = "docs-touched", source = "detector", roots = ["src"] }]"#,
    }]);
    let (rows, first) = repo.propose(STORE, "U1", &[]);
    assert!(!with_concern(&rows, "doc-drift").is_empty(), "{first}");
    // The wave leaves doc-drift out, so the next proposal finds no room for it.
    let kept: Vec<JsonValue> = rows
        .iter()
        .filter(|r| !concerns(r).contains(&"doc-drift".to_string()))
        .cloned()
        .collect();
    let planned: Vec<String> = kept.iter().flat_map(concerns).collect();
    assert!(!planned.is_empty());
    let pad = (kept.len()..3).map(|i| {
        json!({"slice": ["src/**"], "concerns": [format!("probe:pad-{i}")], "agent": "review-finder",
               "reason": "fills the ceiling", "question": "Is the ceiling spent?"})
    });
    let wave: Vec<JsonValue> = kept.into_iter().chain(pad).collect();
    repo.plan("U1", &wave);

    let (rows, second) = repo.propose(STORE, "U1", &[]);
    assert!(rows.is_empty(), "the ceiling is spent: {rows:?}");
    for concern in &planned {
        assert!(
            !ceiling_cut(&second, concern),
            "{concern} already has a cell: {second}"
        );
    }
    assert!(ceiling_cut(&second, "doc-drift"), "{second}");
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
        cmd.env_remove("GIT_DIR")
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

    let sweeper = |r: &JsonValue| r["agent"] == "review-sweeper";
    let is_floor = |r: &JsonValue| concerns(r).contains(&"correctness".to_string());
    let holds_hotspot = |r: &JsonValue| {
        let slice = strings(&r["slice"]);
        slice.contains(&"src/a.rs".to_string()) || slice.contains(&"src/**".to_string())
    };
    let due: Vec<&&JsonValue> = firsts
        .iter()
        .filter(|r| !sweeper(r) && (is_floor(r) || holds_hotspot(r)))
        .collect();
    assert!(
        firsts.iter().any(|r| !is_floor(r) && holds_hotspot(r)),
        "a non-floor cell holding the hotspot is in the fixture: {rows:?}"
    );
    assert!(
        firsts.iter().any(|r| !is_floor(r) && !holds_hotspot(r)),
        "a cell owed no repeat is in the fixture: {rows:?}"
    );
    assert!(
        firsts.iter().any(|r| sweeper(r)),
        "a sweeper cell is in the fixture: {rows:?}"
    );
    assert_eq!(
        repeats.len(),
        due.len(),
        "one repeat per floor or hotspot judgement cell: {rows:?}"
    );
    assert!(
        !repeats.iter().any(|r| sweeper(r)),
        "a sweeper cell never repeats: {rows:?}"
    );
    for first in &due {
        let mut reversed = strings(&first["slice"]);
        reversed.reverse();
        assert!(
            repeats
                .iter()
                .any(|r| concerns(r) == concerns(first) && strings(&r["slice"]) == reversed),
            "no reverse-order repeat of {first}"
        );
    }
    let hot_first = repeats
        .iter()
        .position(|r| !holds_hotspot(r))
        .unwrap_or(repeats.len());
    assert!(
        repeats[hot_first..].iter().all(|r| !holds_hotspot(r)),
        "hotspot repeats come before the rest: {repeats:?}"
    );
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
fn propose_never_exceeds_ceiling() {
    let mut files: Vec<String> = (0..24)
        .flat_map(|d| (0..3).map(move |f| format!("d{d:02}/f{f}.rs")))
        .collect();
    files.extend(["d24/x.rs".to_string(), "d24/y.rs".to_string()]);
    assert_eq!(files.len(), 74);
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let Some(repo) = Repo::new(&refs) else {
        return;
    };
    let hot: Vec<String> = files.iter().take(30).map(|f| format!("{f:?}")).collect();
    let signals = format!(
        r#"[{{ name = "trust-boundary", source = "judged", accepted = true }}, {{ name = "churn-hotspot", source = "detector", roots = ["."], evidence = [{}] }}]"#,
        hot.join(", ")
    );
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "standard",
        changed_lines: Some(2000),
        scope: &["d*/**"],
        signals: &signals,
    }]);
    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert_eq!(summary["ceiling"], 16, "{summary}");
    assert!(
        ceiling_cells(&rows) <= 16,
        "{} cells over a ceiling of 16: {rows:?}",
        ceiling_cells(&rows)
    );
    assert!(summary["cells"].as_u64().unwrap() <= 16, "{summary}");
    for floor in ["correctness", "ripple"] {
        assert!(
            !with_concern(&rows, floor).is_empty(),
            "{floor} keeps a cell: {rows:?}"
        );
    }
    assert_eq!(
        summary["unsliced"],
        json!([]),
        "merging keeps every file sliced"
    );
}

#[test]
fn propose_merges_small_floor_slices_before_cutting_signal_lenses() {
    let files: Vec<String> = (0..12).map(|d| format!("d{d:02}/a.rs")).collect();
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let Some(repo) = Repo::new(&refs) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "standard",
        changed_lines: Some(2000),
        scope: &["d*/**"],
        signals: r#"[{ name = "trust-boundary", source = "judged", accepted = true }]"#,
    }]);
    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    let firsts: Vec<JsonValue> = rows
        .iter()
        .filter(|r| r.get("pass").is_none())
        .cloned()
        .collect();
    let correctness = with_concern(&firsts, "correctness");
    assert!(
        (1..12).contains(&correctness.len()),
        "the twelve one-directory slices merge: {correctness:?}"
    );
    assert!(
        correctness.len() <= 7,
        "the floor lens's first pass fits half of the 15 cells left beside ripple's: {correctness:?}"
    );
    assert!(
        !with_concern(&rows, "security").is_empty(),
        "the signal lens keeps a cell: {rows:?}"
    );
    assert!(!ceiling_cut(&summary, "security"), "{summary}");
    assert!(ceiling_cells(&rows) <= 16, "{rows:?}");
    assert_eq!(summary["unsliced"], json!([]), "{summary}");
}

#[test]
fn propose_counts_check_cells_against_ceiling() {
    let docs: Vec<String> = (1..=5).map(|i| format!("docs/p{i}.md")).collect();
    let mut files: Vec<&str> = docs.iter().map(String::as_str).collect();
    files.push("docs/plan.md");
    let Some(repo) = Repo::new(&files) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "plan",
        depth: "quick",
        changed_lines: None,
        scope: &["docs/plan.md"],
        signals: "[]",
    }]);
    let planned: Vec<JsonValue> = docs
        .iter()
        .map(|d| {
            json!({"slice": [d], "concerns": ["plan-structure"], "agent": "review-finder", "reason": "proposed"})
        })
        .collect();
    repo.plan("U1", &planned);

    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert_eq!(summary["ceiling"], 6, "{summary}");
    assert!(
        ceiling_cells(&rows) <= 1,
        "five cells planned leave room for one: {rows:?}"
    );
    assert!(summary["cells"].as_u64().unwrap() <= 1, "{summary}");
    assert!(
        ceiling_cut(&summary, "check:falsifiability"),
        "a check cell past the ceiling is listed: {summary}"
    );
}

#[test]
fn propose_caps_sweeper_slice_by_file_count() {
    let files = flat_files(90);
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let Some(repo) = Repo::new(&refs) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "standard",
        changed_lines: Some(2000),
        scope: &["src/**"],
        signals: "[]",
    }]);
    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert_eq!(summary["mechanical_pool"], 4, "ceil(16 / 4): {summary}");
    let ripple = with_concern(&rows, "ripple");
    assert_eq!(ripple.len(), 3, "90 files at 40 a cell: {ripple:?}");
    let mut swept: Vec<String> = Vec::new();
    for cell in &ripple {
        assert_eq!(cell["agent"], "review-sweeper");
        let slice = strings(&cell["slice"]);
        assert!(
            slice.len() <= 40,
            "{} files in one sweeper cell",
            slice.len()
        );
        swept.extend(slice);
    }
    swept.sort();
    assert_eq!(swept, files, "the chunks cover the scope in path order");
    assert!(ceiling_cells(&rows) <= 16, "{rows:?}");
    assert_eq!(summary["over_cap"], json!([]), "{summary}");
}

#[test]
fn propose_reports_over_cap_cells_at_a_small_ceiling() {
    let files = flat_files(90);
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let Some(repo) = Repo::new(&refs) else {
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
    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert_eq!(summary["ceiling"], 3, "{summary}");
    assert!(ceiling_cells(&rows) <= 3, "{rows:?}");
    let ripple = with_concern(&rows, "ripple");
    assert!(
        !ripple.is_empty() && ripple.len() < 3,
        "the sweeper chunks merge past the cap to fit: {ripple:?}"
    );
    let over = summary["over_cap"].as_array().expect("over_cap is listed");
    assert_eq!(over.len(), 1, "{summary}");
    assert!(
        concerns(&over[0]).contains(&"ripple".to_string()),
        "{summary}"
    );
    assert!(over[0]["files"].as_u64().unwrap() > 40, "{summary}");
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
    // A ceiling of 4: the test appends four cells over three waves.
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(400),
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
