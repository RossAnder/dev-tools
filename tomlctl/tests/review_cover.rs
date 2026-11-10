//! `tomlctl review cover --ruled-out` against hand-seeded stores and a
//! temp-dir catalogue holding one judgement lens and one mechanical lens.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, sandbox};
use serde_json::{Value as JsonValue, json};

const SCOPE: &str = "t";

const VOCABULARY: &str = r#"
[concerns.logic]
meaning = "wrong output"
families = ["code"]

[concerns.ripple]
meaning = "stale callers"
families = ["code"]
"#;

fn lens(id: &str, mode: &str, slice: &str, tag: &str) -> String {
    let produces = if mode == "mechanical" {
        r#"["cleanup"]"#
    } else {
        r#"["defect", "cleanup", "advisory"]"#
    };
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"code\"]\nmode = \"{mode}\"\n\
         produces = {produces}\nowns = [\"{tag}\"]\nactivation = \"floor\"\nslice = \"{slice}\"\n\
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
    fn new() -> Self {
        let (dir, root) = sandbox();
        let catalogue = tempfile::tempdir().unwrap();
        let builtin = catalogue.path().join("review-v2-lenses");
        for (rel, text) in [
            (
                "lenses/correctness.md",
                lens("correctness", "judgement", "per-subsystem", "logic"),
            ),
            (
                "lenses/ripple.md",
                lens("ripple", "mechanical", "whole-scope", "ripple"),
            ),
        ] {
            let path = builtin.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
        Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
            builtin,
        }
    }

    fn store_path(&self) -> PathBuf {
        self.root
            .join(".claude/review-v2")
            .join(format!("{SCOPE}.toml"))
    }

    /// Seeds run U1 and cell C1 over `src/a.rs`, binding `concern` and
    /// carrying `cell_extra` verbatim.
    fn seed(&self, concern: &str, agent: &str, cell_extra: &str) {
        let text = format!(
            "schema_version = 1\n\n[[runs]]\nid = \"U1\"\ncommand = \"review-v2\"\nfamily = \"code\"\n\
             depth = \"standard\"\nstarted = \"2026-10-12T10:00:00Z\"\nbase = \"abc\"\n\
             scope = [\"src/**\"]\nsignals = []\ncatalogue = {{ builtin = \"{}\" }}\n\n\
             [[cells]]\nid = \"C1\"\nrun = \"U1\"\nslice = [\"src/a.rs\"]\nconcerns = [\"{concern}\"]\n\
             agent = \"{agent}\"\nreason = \"proposed\"\n{cell_extra}",
            self.builtin.to_string_lossy().replace('\\', "/"),
        );
        let path = self.store_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn payload(&self, rows: &[JsonValue]) -> PathBuf {
        let path = self.root.join(".claude/ruled-out.ndjson");
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        fs::write(&path, text).unwrap();
        path
    }

    fn cover(&self, ruled_out: Option<&Path>) -> assert_cmd::assert::Assert {
        let mut cmd = cli(&self.root);
        cmd.current_dir(&self.root).args([
            "review", "cover", "C1", "--scope", SCOPE, "--status", "covered", "--note", "swept",
        ]);
        if let Some(path) = ruled_out {
            cmd.arg("--ruled-out").arg(path);
        }
        cmd.assert()
    }

    fn cell(&self) -> toml::Value {
        let store: toml::Value =
            toml::from_str(&fs::read_to_string(self.store_path()).unwrap()).unwrap();
        store["cells"][0].clone()
    }
}

fn stdout_json(assert: &assert_cmd::assert::Assert) -> JsonValue {
    serde_json::from_slice(&assert.get_output().stdout).expect("cover prints JSON")
}

fn ruled_out(cell: &toml::Value) -> Vec<toml::Value> {
    cell.get("ruled_out")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

#[test]
fn cover_stores_ruled_out_entries() {
    let repo = Repo::new();
    repo.seed("correctness", "review-finder", "status = \"assigned\"\n");
    let payload = repo.payload(&[
        json!({"file": "src/a.rs", "symbol": "a::pad", "reason": "the pad is deliberate"}),
        json!({"file": "src/a.rs", "line": 12, "reason": "a sentinel, not an off-by-one"}),
    ]);
    let out = repo.cover(Some(&payload)).success();
    assert_eq!(stdout_json(&out)["ruled_out"], json!(2));

    let cell = repo.cell();
    assert_eq!(cell["sealed"].as_bool(), Some(true));
    let entries = ruled_out(&cell);
    assert_eq!(entries.len(), 2, "both entries are stored: {cell}");
    assert_eq!(entries[0]["file"].as_str(), Some("src/a.rs"));
    assert_eq!(entries[0]["symbol"].as_str(), Some("a::pad"));
    assert_eq!(entries[0]["reason"].as_str(), Some("the pad is deliberate"));
    assert_eq!(entries[1]["line"].as_integer(), Some(12));
    assert!(entries[1].get("symbol").is_none());

    // The same seal with another set replaces the first.
    let payload =
        repo.payload(&[json!({"file": "src/a.rs", "line": 4, "reason": "kept on purpose"})]);
    let out = repo.cover(Some(&payload)).success();
    assert_eq!(stdout_json(&out)["unchanged"], json!(false));
    let entries = ruled_out(&repo.cell());
    assert_eq!(entries.len(), 1, "re-covering replaces the rule-outs");
    assert_eq!(entries[0]["reason"].as_str(), Some("kept on purpose"));
}

#[test]
fn cover_refuses_ruled_out_entry_without_reason() {
    let repo = Repo::new();
    repo.seed("correctness", "review-finder", "status = \"assigned\"\n");
    let before = fs::read(repo.store_path()).unwrap();
    let payload = repo.payload(&[
        json!({"file": "src/a.rs", "line": 3, "reason": "fine"}),
        json!({"file": "src/a.rs", "symbol": "a::pad"}),
    ]);
    let out = repo.cover(Some(&payload)).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("line 2") && stderr.contains("`reason`"),
        "the refusal names the entry and the missing key: {stderr}"
    );
    assert_eq!(
        fs::read(repo.store_path()).unwrap(),
        before,
        "a refused entry seals nothing"
    );
}

#[test]
fn cover_refuses_ruled_out_entry_with_neither_symbol_nor_line() {
    let repo = Repo::new();
    repo.seed("correctness", "review-finder", "status = \"assigned\"\n");
    let before = fs::read(repo.store_path()).unwrap();
    let payload = repo.payload(&[
        json!({"file": "src/a.rs", "line": 3, "reason": "fine"}),
        json!({"file": "src/a.rs", "symbol": "", "reason": "fine"}),
    ]);
    let out = repo.cover(Some(&payload)).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("line 2") && stderr.contains("`symbol` or `line` is required"),
        "{stderr}"
    );
    assert!(!stderr.contains("line 1"), "{stderr}");
    assert_eq!(fs::read(repo.store_path()).unwrap(), before);
}

#[test]
fn cover_without_ruled_out_leaves_the_cell_unchanged() {
    let repo = Repo::new();
    repo.seed(
        "correctness",
        "review-finder",
        "status = \"covered\"\nsealed = true\nnote = \"swept\"\n\
         ruled_out = [{ file = \"src/a.rs\", symbol = \"a::pad\", reason = \"the pad is deliberate\" }]\n",
    );
    let before = fs::read(repo.store_path()).unwrap();
    let out = repo.cover(None).success();
    let printed = stdout_json(&out);
    assert_eq!(printed["unchanged"], json!(true));
    assert!(printed.get("ruled_out").is_none());
    assert_eq!(fs::read(repo.store_path()).unwrap(), before);
    assert_eq!(ruled_out(&repo.cell()).len(), 1);
}

#[test]
fn cover_refuses_ruled_out_outside_a_judgement_cells_slice() {
    let repo = Repo::new();
    repo.seed("correctness", "review-finder", "status = \"assigned\"\n");
    let before = fs::read(repo.store_path()).unwrap();
    let payload = repo.payload(&[json!({"file": "src/b.rs", "line": 2, "reason": "deliberate"})]);
    let out = repo.cover(Some(&payload)).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("src/b.rs") && stderr.contains("slice"),
        "the refusal names the out-of-slice file: {stderr}"
    );
    assert_eq!(fs::read(repo.store_path()).unwrap(), before);
}

#[test]
fn cover_accepts_ruled_out_outside_a_mechanical_cells_slice() {
    let repo = Repo::new();
    repo.seed("ripple", "review-sweeper", "status = \"assigned\"\n");
    let payload = repo.payload(&[
        json!({"file": "src/b.rs", "symbol": "b::alias", "reason": "an alias kept for callers"}),
    ]);
    repo.cover(Some(&payload)).success();
    let entries = ruled_out(&repo.cell());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["file"].as_str(), Some("src/b.rs"));
}

const SLUG: &str = "s";

/// Seeds a plan-family run and cell C1, sliced by task ref, in flow `SLUG`'s
/// store, and returns the store path.
fn seed_flow(repo: &Repo, v2: bool) -> PathBuf {
    let flow = repo.root.join(".claude/flows").join(SLUG);
    fs::create_dir_all(&flow).unwrap();
    if v2 {
        fs::write(flow.join("plan.toml"), "schema_version = 1\n").unwrap();
    }
    let path = flow.join("review-v2.toml");
    fs::write(
        &path,
        format!(
            "schema_version = 1\n\n[[runs]]\nid = \"U1\"\ncommand = \"review-plan-v2\"\nfamily = \"plan\"\n\
             depth = \"standard\"\nstarted = \"2026-10-12T10:00:00Z\"\nbase = \"abc\"\n\
             scope = []\nsignals = []\ncatalogue = {{ builtin = \"{}\" }}\n\n\
             [[cells]]\nid = \"C1\"\nrun = \"U1\"\nslice = [\"add-the-verb\"]\nconcerns = [\"correctness\"]\n\
             agent = \"plan-executor\"\nreason = \"proposed\"\nstatus = \"assigned\"\n",
            repo.builtin.to_string_lossy().replace('\\', "/"),
        ),
    )
    .unwrap();
    path
}

fn cover_flow(repo: &Repo, ruled_out: &Path) -> assert_cmd::assert::Assert {
    let mut cmd = cli(&repo.root);
    cmd.current_dir(&repo.root)
        .args([
            "review", "cover", "C1", "--slug", SLUG, "--status", "covered", "--note", "swept",
        ])
        .arg("--ruled-out")
        .arg(ruled_out);
    cmd.assert()
}

fn record_rule_outs(repo: &Repo) -> PathBuf {
    repo.payload(&[
        json!({"file": format!(".claude/flows/{SLUG}/tasks.toml"), "symbol": "add-the-verb", "reason": "decided"}),
        json!({"file": format!(".claude/flows/{SLUG}/plan.toml"), "symbol": "D3", "reason": "decided"}),
    ])
}

#[test]
fn cover_accepts_ruled_out_on_a_v2_flows_record_files() {
    let repo = Repo::new();
    let store = seed_flow(&repo, true);
    cover_flow(&repo, &record_rule_outs(&repo)).success();
    let parsed: toml::Value = toml::from_str(&fs::read_to_string(store).unwrap()).unwrap();
    assert_eq!(ruled_out(&parsed["cells"][0]).len(), 2);
}

#[test]
fn cover_refuses_ruled_out_on_record_files_without_a_v2_plan() {
    let repo = Repo::new();
    let store = seed_flow(&repo, false);
    let before = fs::read(&store).unwrap();
    let out = cover_flow(&repo, &record_rule_outs(&repo)).failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("tasks.toml") && stderr.contains("slice"),
        "the refusal names the record file: {stderr}"
    );
    assert_eq!(fs::read(&store).unwrap(), before);
}
