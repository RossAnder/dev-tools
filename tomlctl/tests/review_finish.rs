//! `tomlctl review run finish` against a sandbox repo whose store is seeded
//! by hand with the run, cells and findings each test needs.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, git_available, git_command, sandbox};
use serde_json::Value as JsonValue;

const SCOPE: &str = "t";

const VOCABULARY: &str = r#"
[concerns.logic]
meaning = "wrong output for a nameable input"
families = ["code"]

[concerns.ripple]
meaning = "stale callers"
families = ["code"]
"#;

fn lens(id: &str, mode: &str, tag: &str) -> String {
    let produces = if mode == "mechanical" {
        r#"["cleanup"]"#
    } else {
        r#"["defect", "cleanup", "advisory"]"#
    };
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"code\"]\nmode = \"{mode}\"\n\
         produces = {produces}\nowns = [\"{tag}\"]\nactivation = \"floor\"\nslice = \"whole-scope\"\n\
         v1_category = \"quality\"\n+++\n## Question\nWhat does {id} ask?\n\n## Owns\n`{tag}` — owned here.\n\n\
         ## Probes — code\n- one probe\n\n## Does not flag\n- confirmations\n"
    )
}

struct Repo {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
    builtin: PathBuf,
}

impl Repo {
    /// A git sandbox tracking `src/a.rs` and `src/b.rs`, and a catalogue of
    /// two floor lenses, `correctness` and `ripple`.
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
        fs::write(
            builtin.join("lenses/correctness.md"),
            lens("correctness", "judgement", "logic"),
        )
        .unwrap();
        fs::write(
            builtin.join("lenses/ripple.md"),
            lens("ripple", "mechanical", "ripple"),
        )
        .unwrap();
        for rel in ["src/a.rs", "src/b.rs"] {
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

    fn path(&self) -> PathBuf {
        self.root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"))
    }

    /// Seeds the store with run `U1` (scope `src/**`) followed by `rows`.
    fn seed(&self, rows: &str) {
        let text = format!(
            "schema_version = 1\n\n[[runs]]\nid = \"U1\"\ncommand = \"review-v2\"\nfamily = \"code\"\n\
             depth = \"quick\"\nstarted = \"2026-10-12T10:00:00Z\"\nbase = \"abc\"\nscope = [\"src/**\"]\n\
             changed_lines = 10\ncatalogue = {{ builtin = \"{}\" }}\n\n{rows}",
            self.builtin.to_string_lossy().replace('\\', "/"),
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

    fn finish(&self, extra: &[&str]) -> assert_cmd::assert::Assert {
        self.tomlctl()
            .args(["review", "run", "finish", "U1", "--scope", SCOPE])
            .args(extra)
            .assert()
    }

    fn run(&self) -> toml::Value {
        let store: toml::Value = toml::from_str(&fs::read_to_string(self.path()).unwrap()).unwrap();
        store["runs"][0].clone()
    }
}

/// A cell over `slice` with `concerns`, sealed with `status`.
fn cell(id: &str, slice: &str, concerns: &str, status: &str) -> String {
    format!(
        "[[cells]]\nid = \"{id}\"\nrun = \"U1\"\nwave = 1\nslice = [{slice}]\nconcerns = [{concerns}]\n\
         agent = \"review-finder\"\nreason = \"proposed\"\nstatus = \"{status}\"\nsealed = true\nnote = \"n\"\n\n"
    )
}

/// The cell that leaves nothing unselected or unsliced.
fn full_cell() -> String {
    cell("C1", r#""src/**""#, r#""correctness", "ripple""#, "covered")
}

fn finding(id: &str, run: &str, seen_in: &str, kind: &str, status: &str, extra: &str) -> String {
    format!(
        "[[findings]]\nid = \"{id}\"\nrun = \"{run}\"\ncell = \"C1\"\nseen_in = [{seen_in}]\nfamily = \"code\"\n\
         kind = \"{kind}\"\nlens = \"correctness\"\nfile = \"src/a.rs\"\nline = 1\nsummary = \"s\"\n\
         status = \"{status}\"\n{extra}\n"
    )
}

fn stderr(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).to_string()
}

fn stdout_json(assert: &assert_cmd::assert::Assert) -> JsonValue {
    serde_json::from_slice(&assert.get_output().stdout).expect("finish prints JSON")
}

fn assert_unfinished(path: &Path) {
    let store: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let run = &store["runs"][0];
    assert!(
        run.get("finished").is_none(),
        "a refusal stamps nothing: {run}"
    );
    assert!(run.get("counts").is_none(), "{run}");
}

#[test]
fn finish_refuses_unverified_findings() {
    let Some(repo) = Repo::new() else {
        return;
    };
    let rows = format!(
        "{}{}{}",
        full_cell(),
        finding("F1", "U1", r#""U1""#, "defect", "unverified", ""),
        finding(
            "F2",
            "U1",
            r#""U1""#,
            "defect",
            "unverified",
            "same_as = \"F9\"\n"
        ),
    );
    repo.seed(&rows);
    let refused = repo.finish(&[]).failure();
    let err = stderr(&refused);
    assert!(err.contains("unverified findings: F1"), "{err}");
    assert!(err.contains("unresolved same_as: F2"), "{err}");
    assert_unfinished(&repo.path());

    repo.seed(&format!(
        "{}{}",
        full_cell(),
        finding("F1", "U1", r#""U1""#, "defect", "open", ""),
    ));
    repo.finish(&[]).success();
    assert!(repo.run().get("finished").is_some());
}

#[test]
fn finish_refuses_unwaived_coverage() {
    let Some(repo) = Repo::new() else {
        return;
    };
    repo.seed(&cell("C1", r#""src/a.rs""#, r#""correctness""#, "covered"));
    let refused = repo.finish(&[]).failure();
    let err = stderr(&refused);
    assert!(
        err.contains("unselected concerns without a waiver: ripple"),
        "{err}"
    );
    assert!(
        err.contains("unsliced files without a waiver: src/b.rs"),
        "{err}"
    );
    assert_unfinished(&repo.path());

    repo.tomlctl()
        .args(["review", "run", "update", "U1", "--scope", SCOPE, "--set-json"])
        .arg(
            r#"waivers=[{"concern":"ripple","reason":"no callers"},{"file":"src/b.rs","reason":"generated"}]"#,
        )
        .assert()
        .success();
    repo.finish(&[]).success();
    assert!(repo.run().get("finished").is_some());
}

#[test]
fn finish_refuses_unsealed_cells() {
    let Some(repo) = Repo::new() else {
        return;
    };
    let unsealed = full_cell().replace("sealed = true\n", "");
    repo.seed(&unsealed);
    let refused = repo.finish(&[]).failure();
    let err = stderr(&refused);
    assert!(err.contains("unsealed cells: C1"), "{err}");
    assert_unfinished(&repo.path());

    let out = repo.finish(&["--incomplete", "finder died"]).success();
    let json = stdout_json(&out);
    assert_eq!(json["gaps"]["unsealed"][0], "C1", "{json}");
}

#[test]
fn finish_incomplete_records_reason() {
    let Some(repo) = Repo::new() else {
        return;
    };
    repo.seed(&format!(
        "{}{}",
        cell("C1", r#""src/a.rs""#, r#""correctness""#, "covered"),
        finding("F1", "U1", r#""U1""#, "defect", "unverified", ""),
    ));
    let out = repo.finish(&["--incomplete", "budget spent"]).success();
    let json = stdout_json(&out);
    assert_eq!(json["incomplete"], "budget spent", "{json}");
    assert_eq!(json["gaps"]["unverified"][0], "F1", "{json}");
    assert_eq!(json["gaps"]["unsliced"][0], "src/b.rs", "{json}");
    let run = repo.run();
    assert_eq!(run["incomplete"].as_str(), Some("budget spent"), "{run}");
    assert!(run.get("finished").is_some(), "{run}");

    let reason = repo.root.join(".claude/reason.txt");
    fs::write(&reason, "verifier timed out\r\n").unwrap();
    let reason = reason.to_string_lossy().into_owned();
    repo.finish(&["--incomplete-file", &reason]).success();
    assert_eq!(
        repo.run()["incomplete"].as_str(),
        Some("verifier timed out")
    );

    repo.finish(&["--incomplete", "  "]).failure();
}

#[test]
fn finish_incomplete_is_a_catalogue_field() {
    let Some(repo) = Repo::new() else {
        return;
    };
    repo.seed(&full_cell());
    repo.finish(&["--incomplete", "budget spent"]).success();
    assert_eq!(repo.run()["incomplete"].as_str(), Some("budget spent"));

    let out = repo
        .tomlctl()
        .args(["review", "run", "start", "--scope", SCOPE])
        .args(["--command", "review-v2", "--family", "code"])
        .args(["--depth", "quick", "--base", "HEAD"])
        .args(["--set", "incomplete=early"])
        .assert()
        .failure();
    let err = stderr(&out);
    assert!(
        err.contains("`incomplete` is set by tomlctl and cannot be supplied"),
        "{err}"
    );
}

#[test]
fn finish_computes_counts() {
    let Some(repo) = Repo::new() else {
        return;
    };
    let rows = [
        full_cell(),
        cell("C2", r#""src/b.rs""#, r#""correctness""#, "uncovered"),
        finding("F1", "U1", r#""U1""#, "defect", "open", ""),
        finding("F2", "U1", r#""U1""#, "defect", "refuted", ""),
        finding("F3", "U1", r#""U1""#, "cleanup", "open", ""),
        finding("F4", "U1", r#""U1""#, "cleanup", "superseded", ""),
        finding("F5", "U1", r#""U1""#, "advisory", "open", ""),
        finding(
            "F6",
            "U1",
            r#""U1""#,
            "advisory",
            "merged",
            "same_as = \"F10\"\noutcome = \"suppressed\"\nmerged_into = \"F10\"\n",
        ),
        finding(
            "F7",
            "U1",
            r#""U1""#,
            "defect",
            "merged",
            "same_as = \"F8\"\noutcome = \"repeat\"\nmerged_into = \"F8\"\n",
        ),
        // A repeat target from an earlier run, re-found here.
        finding("F8", "U0", r#""U0", "U1""#, "defect", "open", ""),
        // An earlier run's finding this run never saw.
        finding("F9", "U0", r#""U0""#, "defect", "open", ""),
        finding("F10", "U0", r#""U0""#, "advisory", "dismissed", ""),
        // Seen again here and refuted: counted as the report counts it.
        finding("F11", "U0", r#""U0", "U1""#, "cleanup", "refuted", ""),
    ]
    .concat();
    repo.seed(&rows);
    let out = repo.finish(&[]).success();
    let json = stdout_json(&out);
    let expected = serde_json::json!({
        "cells": 2, "uncovered": 1, "defect": 2, "cleanup": 1,
        "advisory": 1, "refuted": 2, "suppressed": 1,
    });
    assert_eq!(json["counts"], expected, "{json}");
    assert!(json.get("incomplete").is_none(), "{json}");
    let run = repo.run();
    assert_eq!(run["counts"]["defect"].as_integer(), Some(2), "{run}");
    assert_eq!(run["counts"]["suppressed"].as_integer(), Some(1), "{run}");
}
