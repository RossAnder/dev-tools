//! `tomlctl review add` against a sandbox whose store is seeded by hand with
//! the runs, cells and prior findings each test needs, and a temp-dir
//! catalogue holding only the vocabulary's `[routing]` globs.

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

/// 40 numbered lines; `const LIMIT` sits on line 30.
fn lib_rs() -> String {
    let mut text = String::new();
    for n in 1..=40 {
        if n == 30 {
            text.push_str("const LIMIT: usize = 8;\n");
        } else {
            text.push_str(&format!("// line {n}\n"));
        }
    }
    text
}

const PLAN: &str = "# Plan\n\n## Tasks\n\n### 4. Add the export verb [M]\n\
    - **Acceptance**: `cargo test --test export_cli`\n\
    - **Files**: `src/export.rs`\n\
    Run `cargo test --test export_cli` again.\n\n\
    ### 5. Wire the export verb [S]\n\
    - **Files**: `src/cli.rs`\n";

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

        write(&root, "src/lib.rs", &lib_rs());
        write(&root, "docs/guide.md", "# Guide\n\nRun with `--fast`.\n");
        write(
            &root,
            "docs/crlf.md",
            "# Title\r\n\r\nfirst line\r\nsecond line\r\n",
        );
        write(&root, "docs/plans/p.md", PLAN);

        let run = |id: &str, family: &str, depth: &str| {
            format!(
                "[[runs]]\nid = \"{id}\"\ncommand = \"review-v2\"\nfamily = \"{family}\"\n\
                 depth = \"{depth}\"\nstarted = \"2026-10-12T10:00:00Z\"\n\
                 catalogue = {{ builtin = \"{builtin}\" }}\n\n"
            )
        };
        let cell = |id: &str, run: &str, concerns: &str, sealed: bool| {
            let seal = if sealed {
                "status = \"covered\"\nsealed = true\nnote = \"done\"\n"
            } else {
                "status = \"assigned\"\n"
            };
            format!(
                "[[cells]]\nid = \"{id}\"\nrun = \"{run}\"\nwave = 1\nslice = [\"src/**\"]\n\
                 concerns = [{concerns}]\nagent = \"review-finder\"\nreason = \"proposed\"\n{seal}\n"
            )
        };
        let mut store = String::from("schema_version = 1\n\n");
        store.push_str(&run("U1", "code", "standard"));
        store.push_str(&run("U2", "code", "deep"));
        store.push_str(&run("U3", "plan", "standard"));
        store.push_str(&cell("C1", "U1", "\"correctness\", \"doc-drift\"", false));
        store.push_str(&cell("C2", "U1", "\"correctness\"", true));
        store.push_str(&cell("C3", "U2", "\"correctness\"", false));
        store.push_str(&cell("C4", "U3", "\"plan-structure\"", false));
        store.push_str(&cell("C5", "U1", "\"contrarian\"", false));
        store.push_str(
            "[[findings]]\nid = \"F1\"\nrun = \"U1\"\ncell = \"C2\"\nfamily = \"code\"\n\
             kind = \"advisory\"\nlens = \"correctness\"\nroute = \"none\"\nfile = \"src/lib.rs\"\n\
             symbol = \"lib::run\"\nsummary = \"dismissed idea\"\nstatus = \"dismissed\"\n\
             dismiss_reason = \"not wanted\"\n\n\
             [[findings]]\nid = \"F2\"\nrun = \"U1\"\ncell = \"C2\"\nfamily = \"code\"\n\
             kind = \"defect\"\nlens = \"correctness\"\nroute = \"verifier\"\nfile = \"src/lib.rs\"\n\
             line = 30\nsymbol = \"lib::limit\"\nsummary = \"open defect\"\nseverity = \"major\"\n\
             status = \"open\"\n\n",
        );
        write(&root, ".claude/review-v2/t.toml", &store);
        Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
        }
    }

    fn add(&self, run: &str, cell: &str, rows: &[JsonValue]) -> assert_cmd::assert::Assert {
        let payload = self.root.join(".claude/payload.ndjson");
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        fs::write(&payload, text).unwrap();
        cli(&self.root)
            .current_dir(&self.root)
            .args([
                "review", "add", "--scope", SCOPE, "--run", run, "--cell", cell,
            ])
            .arg("--ndjson")
            .arg(payload)
            .assert()
    }

    fn added(&self, run: &str, cell: &str, rows: &[JsonValue]) -> JsonValue {
        let out = self.add(run, cell, rows).success();
        serde_json::from_slice(&out.get_output().stdout).expect("add prints JSON")
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

    fn finding_count(&self) -> usize {
        self.store()
            .get("findings")
            .and_then(toml::Value::as_array)
            .map_or(0, Vec::len)
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn defect(lens: &str, symbol: &str, severity: &str) -> JsonValue {
    json!({
        "kind": "defect",
        "lens": lens,
        "file": "src/lib.rs",
        "line": 30,
        "symbol": symbol,
        "summary": "the limit is off by one",
        "severity": severity,
        "failure_scenario": "a ninth row is dropped",
        "fix": "raise LIMIT to 9",
        "fix_shape": "edits",
        "effort": "trivial",
        "counter": "callers never pass nine rows",
        "grade": "high",
        "evidence": ["src/lib.rs:30 LIMIT is 8"],
    })
}

fn cleanup(file: &str, line: u32, direction: &str, old: Option<&str>) -> JsonValue {
    let mut row = json!({
        "kind": "cleanup",
        "lens": "doc-drift",
        "file": file,
        "line": line,
        "summary": "stale text",
        "edit": "reword it",
        "direction": direction,
        "effort": "trivial",
    });
    if let Some(old) = old {
        row["old"] = json!(old);
    }
    row
}

fn advisory(same_as: Option<&str>) -> JsonValue {
    let mut row = json!({
        "kind": "advisory",
        "lens": "contrarian",
        "file": "src/lib.rs",
        "symbol": "lib::run",
        "summary": "run could stream",
        "angle": "alternative",
        "rationale": "memory grows with input",
        "adoption_cost": "an API change",
    });
    if let Some(target) = same_as {
        row["same_as"] = json!(target);
    }
    row
}

fn ids(out: &JsonValue) -> Vec<String> {
    out["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap().to_string())
        .collect()
}

fn reasons(out: &JsonValue, index: usize) -> String {
    out["refused"][index]["reasons"].to_string()
}

#[test]
fn defect_without_failure_scenario_is_refused_rest_lands() {
    let repo = Repo::new();
    let mut bare = defect("correctness", "lib::limit", "major");
    bare.as_object_mut().unwrap().remove("failure_scenario");
    let ok = cleanup("docs/guide.md", 3, "change", Some("`--fast`"));
    let out = repo.added("U1", "C1", &[bare, ok]);

    assert_eq!(ids(&out), ["F3"]);
    assert_eq!(out["refused"].as_array().unwrap().len(), 1, "{out}");
    assert_eq!(out["refused"][0]["line"], 1);
    assert!(reasons(&out, 0).contains("failure_scenario"), "{out}");
    let minted = repo.finding("F3");
    assert_eq!(minted["kind"].as_str(), Some("cleanup"));
    assert_eq!(minted["status"].as_str(), Some("unverified"));
    assert_eq!(minted["cell"].as_str(), Some("C1"));
    assert_eq!(minted["seen_in"][0].as_str(), Some("U1"));
    assert!(minted["first_seen"].as_datetime().is_some(), "{minted:?}");
}

#[test]
fn cleanup_old_text_must_exist_near_line() {
    let repo = Repo::new();
    let old = "const LIMIT: usize = 8;";
    let far = cleanup("src/lib.rs", 5, "change", Some(old));
    let near = cleanup("src/lib.rs", 12, "change", Some(old));
    let absent = cleanup("src/lib.rs", 30, "change", Some("const LIMIT: usize = 9;"));
    let out = repo.added("U1", "C1", &[far, near, absent]);

    assert_eq!(ids(&out), ["F3"], "only the row within 20 lines lands");
    assert_eq!(repo.finding("F3")["line"].as_integer(), Some(12));
    let refused: Vec<i64> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["line"].as_i64().unwrap())
        .collect();
    assert_eq!(refused, [1, 3]);
    assert!(
        reasons(&out, 0).contains("within 20 lines of line 5"),
        "{out}"
    );
}

#[test]
fn add_direction_with_new_target_file_lands() {
    let repo = Repo::new();
    let mut row = cleanup("docs/guide.md", 1, "add", None);
    row["target_file"] = json!("docs/new.md");
    let out = repo.added("U1", "C1", &[row.clone()]);
    assert_eq!(ids(&out), ["F3"], "{out}");
    assert_eq!(out["findings"][0]["route"], "checker");

    row["target_file"] = json!("docs/crlf.md");
    let refused = repo.add("U1", "C1", &[row]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(
        stderr.contains("`old` is required"),
        "an existing target needs its anchor text: {stderr}"
    );
}

#[test]
fn distinct_defects_on_one_symbol_both_mint() {
    let repo = Repo::new();
    let mut second = defect("correctness", "lib::limit", "minor");
    second["summary"] = json!("the limit is not configurable");
    let out = repo.added(
        "U1",
        "C1",
        &[defect("correctness", "lib::limit", "major"), second],
    );
    assert_eq!(ids(&out), ["F3", "F4"]);
    for id in ["F3", "F4"] {
        let row = repo.finding(id);
        assert_eq!(row["status"].as_str(), Some("unverified"));
        assert_eq!(row["anchor"].as_str(), Some("code|src/lib.rs|lib::limit"));
        assert_eq!(
            row["related"].as_array().map(|r| r[0].as_str()),
            Some(Some("F2")),
            "the stored finding on the symbol is a candidate: {row:?}"
        );
        assert!(row.get("same_as").is_none());
    }
}

#[test]
fn resubmitted_cell_supersedes_unverified_rows() {
    let repo = Repo::new();
    let first = repo.added(
        "U1",
        "C5",
        &[defect("contrarian", "lib::limit", "major"), advisory(None)],
    );
    assert_eq!(ids(&first), ["F3", "F4"]);
    assert_eq!(repo.finding("F4")["status"].as_str(), Some("open"));

    let second = repo.added("U1", "C5", &[defect("contrarian", "lib::limit", "major")]);
    assert_eq!(ids(&second), ["F5"]);
    assert_eq!(second["superseded"], json!(["F3"]));
    assert_eq!(repo.finding("F3")["status"].as_str(), Some("superseded"));
    assert_eq!(repo.finding("F5")["status"].as_str(), Some("unverified"));
    assert_eq!(
        repo.finding("F4")["status"].as_str(),
        Some("open"),
        "only unverified rows are withdrawn"
    );
    assert_eq!(
        repo.finding("F5").get("related"),
        Some(&toml::Value::Array(vec![toml::Value::String("F2".into())])),
        "a superseded row is no longer a candidate"
    );
    assert_eq!(repo.finding("F2")["status"].as_str(), Some("open"));
    assert_eq!(repo.finding_count(), 5);
}

#[test]
fn sealed_cell_refuses_and_prints_ids() {
    let repo = Repo::new();
    let before = repo.store();
    let assert = repo
        .add("U1", "C2", &[defect("correctness", "lib::x", "major")])
        .failure();
    let out = assert.get_output();
    let printed: JsonValue = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert_eq!(printed["sealed"], true);
    assert_eq!(printed["status"], "covered");
    assert_eq!(ids(&printed), ["F1", "F2"]);
    assert_eq!(printed["findings"][1]["severity"], "major");
    assert_eq!(printed["findings"][1]["route"], "verifier");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cell C2 is sealed"), "{stderr}");
    assert_eq!(repo.store(), before, "a sealed cell's store is untouched");
}

#[test]
fn executable_cleanup_routes_to_verifier() {
    let repo = Repo::new();
    let code = cleanup("src/lib.rs", 30, "change", Some("LIMIT: usize = 8"));
    let doc = cleanup("docs/guide.md", 3, "change", Some("`--fast`"));
    let mut spread = doc.clone();
    spread["instances"] = json!(["docs/crlf.md", "src/lib.rs"]);
    let mut test_add = cleanup("docs/guide.md", 3, "change", Some("`--fast`"));
    test_add["instances"] = json!(["tests/a.rs"]);
    let out = repo.added("U1", "C1", &[code, doc, spread, test_add]);

    let routes: Vec<&str> = out["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["route"].as_str().unwrap())
        .collect();
    assert_eq!(routes, ["verifier", "checker", "verifier", "checker"]);
    assert_eq!(repo.finding("F3")["route"].as_str(), Some("verifier"));
    assert!(out["findings"][0]["severity"].is_null());
}

#[test]
fn critical_defect_at_deep_routes_to_panel() {
    let repo = Repo::new();
    let deep = repo.added(
        "U2",
        "C3",
        &[
            defect("correctness", "lib::a", "critical"),
            defect("correctness", "lib::b", "major"),
        ],
    );
    let routes: Vec<&str> = deep["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["route"].as_str().unwrap())
        .collect();
    assert_eq!(routes, ["panel", "verifier"]);
    assert_eq!(deep["findings"][0]["severity"], "critical");
    assert_eq!(repo.finding("F3")["route"].as_str(), Some("panel"));

    let standard = repo.added("U1", "C1", &[defect("correctness", "lib::a", "critical")]);
    assert_eq!(standard["findings"][0]["route"], "verifier");
}

#[test]
fn advisory_same_as_dismissed_is_merged() {
    let repo = Repo::new();
    let out = repo.added("U1", "C5", &[advisory(Some("F1")), advisory(None)]);
    assert_eq!(ids(&out), ["F3", "F4"]);
    assert_eq!(out["findings"][0]["route"], "none");
    let merged = repo.finding("F3");
    assert_eq!(merged["status"].as_str(), Some("merged"));
    assert_eq!(merged["merged_into"].as_str(), Some("F1"));
    assert_eq!(merged["outcome"].as_str(), Some("suppressed"));
    assert_eq!(merged["same_as"].as_str(), Some("F1"));
    let fresh = repo.finding("F4");
    assert_eq!(fresh["status"].as_str(), Some("open"));
    assert_eq!(fresh["related"][0].as_str(), Some("F1"));

    let mut stray = advisory(Some("F2"));
    stray["symbol"] = json!("lib::run");
    let refused = repo.add("U1", "C5", &[stray]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(
        stderr.contains("not an anchor candidate") && stderr.contains("candidates: F1"),
        "{stderr}"
    );
}

#[test]
fn lens_outside_cell_concerns_is_refused() {
    let repo = Repo::new();
    let before = repo.finding_count();
    let refused = repo
        .add("U1", "C1", &[defect("security", "lib::limit", "major")])
        .failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(stderr.contains("`lens` is"), "{stderr}");
    assert!(stderr.contains("correctness, doc-drift"), "{stderr}");
    assert_eq!(repo.finding_count(), before, "nothing was written");

    let mixed = repo.added(
        "U1",
        "C1",
        &[
            defect("security", "lib::limit", "major"),
            defect("correctness", "lib::limit", "major"),
        ],
    );
    assert_eq!(ids(&mixed), ["F3"]);
    assert!(reasons(&mixed, 0).contains("lens"), "{mixed}");
}

#[test]
fn cleanup_old_text_matches_in_crlf_file() {
    let repo = Repo::new();
    let lf = cleanup("docs/crlf.md", 3, "change", Some("first line\nsecond line"));
    let crlf = cleanup(
        "docs/crlf.md",
        3,
        "remove",
        Some("first line\r\nsecond line"),
    );
    let out = repo.added("U1", "C1", &[lf, crlf]);
    assert_eq!(ids(&out), ["F3", "F4"], "{out}");
    assert!(out["refused"].as_array().unwrap().is_empty());
}

#[test]
fn plan_anchor_old_must_be_unique_in_section() {
    let repo = Repo::new();
    let row = |section: &str, anchor_old: &str| {
        json!({
            "kind": "cleanup",
            "lens": "plan-structure",
            "file": "docs/plans/p.md",
            "line": 5,
            "symbol": "add-the-export-verb",
            "summary": "a Files line is stale",
            "edit": "name the right file",
            "direction": "change",
            "old": "`src/export.rs`",
            "effort": "trivial",
            "plan_section": section,
            "anchor_old": anchor_old,
            "anchor_new": "`src/export/mod.rs`",
        })
    };
    let out = repo.added(
        "U3",
        "C4",
        &[
            row("### 4. Add the export verb [M]", "`src/export.rs`"),
            row(
                "### 4. Add the export verb [M]",
                "`cargo test --test export_cli`",
            ),
            row("### 9. No such task", "`src/export.rs`"),
            row("### 5. Wire the export verb [S]", "`src/export.rs`"),
        ],
    );
    assert_eq!(ids(&out), ["F3"], "{out}");
    assert_eq!(repo.finding("F3")["family"].as_str(), Some("plan"));
    let refused: Vec<i64> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["line"].as_i64().unwrap())
        .collect();
    assert_eq!(refused, [2, 3, 4]);
    assert!(reasons(&out, 0).contains("occurs 2 times"), "{out}");
    assert!(reasons(&out, 1).contains("names no heading"), "{out}");
    assert!(reasons(&out, 2).contains("does not occur under"), "{out}");
}
