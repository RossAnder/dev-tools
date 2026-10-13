//! `tomlctl review list` and `review show` against a sandbox whose store is
//! seeded by hand, with a temp-dir catalogue and claude dir for the paths a
//! finder's assignment carries.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, parse_json_error_envelope, sandbox};
use serde_json::Value as JsonValue;

const SCOPE: &str = "t";

const FINDINGS: &str = r#"
[[findings]]
id = "F1"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/review/lib.rs"
line = 30
symbol = "lib::limit"
summary = "the limit is off by one"
severity = "major"
failure_scenario = "a nine-row input returns eight rows"
fix = "compare with <= instead of <"
fix_shape = "edits"
effort = "small"
counter = "a caller that pads the input"
grade = "high"
evidence = ["src/review/lib.rs:30 compares with <"]
status = "open"

[[findings]]
id = "F2"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "correctness"
route = "checker"
file = "src/review/guide.md"
line = 3
summary = "stale flag in the guide"
edit = "drop the --fast line"
direction = "remove"
old = "--fast"
effort = "trivial"
status = "unverified"

[[findings]]
id = "F3"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/other.rs"
line = 4
summary = "outside the slice"
severity = "minor"
status = "open"

[[findings]]
id = "F4"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "correctness"
route = "checker"
file = "src/review/lib.rs"
line = 8
summary = "a withdrawn row"
status = "superseded"

[[findings]]
id = "F5"
run = "U1"
cell = "C1"
family = "code"
kind = "advisory"
lens = "correctness"
route = "none"
file = "src/review/store.rs"
summary = "the lock could be per-cell"
angle = "risk"
rationale = "contention grows with agents"
adoption_cost = "a store format change"
status = "open"

[[findings]]
id = "F6"
run = "U1"
cell = "C1"
family = "code"
kind = "advisory"
lens = "correctness"
route = "none"
file = "README.md"
summary = "swap the TOML store for SQLite"
angle = "alternative"
rationale = "concurrent writers"
alternative = "lumina's store"
adoption_cost = "a migration"
status = "open"

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"
seat = 1
verdict = "confirmed"
citation = "src/review/lib.rs:30"
rationale = "reproduced with nine rows"

[[verdicts]]
finding = "F1"
run = "U1"
by = "audit"
verdict = "refuted"
citation = "src/review/lib.rs:12-13"
rationale = "callers cap the input"
"#;

struct Repo {
    _dir: tempfile::TempDir,
    _home: tempfile::TempDir,
    root: PathBuf,
    claude_dir: PathBuf,
    builtin: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let home = tempfile::tempdir().unwrap();
        // Not canonicalised: a `\\?\` path takes no `/` separators, and the
        // store records the catalogue root `/`-separated.
        let claude_dir = home.path().join(".claude");
        let builtin = claude_dir.join("skills/review-v2-lenses");
        write_catalogue(&builtin);
        let store = format!("{}{}{FINDINGS}", run_row(&builtin), cell_row(&builtin));
        let path = root.join(".claude/review-v2/t.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, store).unwrap();
        Self {
            _dir: dir,
            _home: home,
            root,
            claude_dir,
            builtin,
        }
    }

    fn review(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        cli(&self.root)
            .current_dir(&self.root)
            .env("CLAUDE_CONFIG_DIR", &self.claude_dir)
            .args(["--error-format", "json", "review"])
            .args(args)
            .args(["--scope", SCOPE])
            .assert()
    }

    fn json(&self, args: &[&str]) -> JsonValue {
        let out = self.review(args).success();
        serde_json::from_slice(&out.get_output().stdout).expect("review prints JSON")
    }

    fn show(&self, ids: &str, role: &str) -> Vec<JsonValue> {
        let out = self.json(&["show", ids, "--for", role]);
        out["items"].as_array().cloned().expect("show lists items")
    }
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn write_catalogue(builtin: &Path) {
    let entries = [
        (
            "lenses/correctness.md",
            "+++\nid = \"correctness\"\nstatus = \"stable\"\n+++\n## Question\nWrong output?\n",
        ),
        (
            "facets/rust.md",
            "+++\nid = \"rust\"\nstatus = \"stable\"\n+++\n## General\nRust notes.\n",
        ),
        (
            "methods/trace.md",
            "+++\nid = \"trace\"\nstatus = \"stable\"\n+++\n## Steps\nTrace it.\n",
        ),
    ];
    for (rel, text) in entries {
        let path = builtin.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::write(builtin.join("vocabulary.toml"), "").unwrap();
}

fn run_row(builtin: &Path) -> String {
    format!(
        "schema_version = 1\n\n[[runs]]\nid = \"U1\"\ncommand = \"review-v2\"\nfamily = \"code\"\n\
         depth = \"standard\"\nstarted = \"2026-10-12T10:00:00Z\"\n\
         catalogue = {{ builtin = \"{}\" }}\n\
         baselines = {{ lint-baseline = \"logs/lint.txt\", profile-baseline = \"none — no profiler\" }}\n\n",
        slash(builtin)
    )
}

fn cell_row(builtin: &Path) -> String {
    format!(
        "[[cells]]\nid = \"C1\"\nrun = \"U1\"\nwave = 1\nslice = [\"src/review/**\"]\n\
         concerns = [\"correctness\"]\nquestion = \"does the limit hold?\"\nagent = \"review-finder\"\n\
         reason = \"proposed\"\n\
         bound = [{{ concern = \"correctness\", hash = \"9c1e\", path = \"{}/lenses/correctness.md\", \
         methods = [\"trace\"], v1_category = \"quality\" }}]\n\
         facets = [\"rust\"]\nstatus = \"assigned\"\n\n",
        slash(builtin)
    )
}

fn ids(rows: &JsonValue) -> Vec<String> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn list_in_glob_filters_prior() {
    let repo = Repo::new();
    let out = repo.json(&["list", "--in", "src/review/**", "--for", "finder"]);
    assert_eq!(
        ids(&out["findings"]),
        ["F1", "F2", "F5"],
        "only the slice's rows, superseded ones left out: {out}"
    );
    let first = &out["findings"][0];
    for withheld in ["lens", "severity", "failure_scenario", "grade", "route"] {
        assert!(
            first.get(withheld).is_none(),
            "PRIOR shows {withheld}: {first}"
        );
    }
    assert_eq!(first["status"], "open");

    let dir = repo.json(&["list", "--in", "src/review", "--kind", "defect"]);
    assert_eq!(ids(&dir["findings"]), ["F1"], "{dir}");
    let checker = repo.json(&[
        "list",
        "--status",
        "unverified",
        "--route",
        "checker",
        "--for",
        "orchestrator",
    ]);
    assert_eq!(ids(&checker["findings"]), ["F2"], "{checker}");
    assert_eq!(checker["findings"][0]["cell"], "C1");
    assert_eq!(checker["findings"][0]["route"], "checker");

    let refused = repo.review(&["list", "--status", "opne"]).failure();
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&refused.get_output().stderr));
    assert_eq!(err["kind"], "validation");
}

#[test]
fn show_for_verifier_omits_grade_and_fix() {
    let repo = Repo::new();
    let out = repo.json(&["show", "F1", "--for", "verifier"]);
    let row = &out["items"][0];
    for withheld in ["grade", "fix", "fix_shape", "effort", "status"] {
        assert!(
            row.get(withheld).is_none(),
            "verifier sees {withheld}: {row}"
        );
    }
    for shown in [
        "failure_scenario",
        "counter",
        "evidence",
        "severity",
        "lens",
    ] {
        assert!(row.get(shown).is_some(), "verifier misses {shown}: {row}");
    }
    let verifying = out["references"]["verifying"].as_str().unwrap();
    assert_eq!(
        verifying,
        format!(
            "{}/skills/review-v2-method/references/verifying.md",
            slash(&repo.claude_dir)
        )
    );

    let implementer = &repo.show("F1", "implementer")[0];
    assert_eq!(implementer["fix"], "compare with <= instead of <");
    assert!(implementer.get("grade").is_none(), "{implementer}");
}

#[test]
fn show_for_audit_inlines_cited_lines() {
    let repo = Repo::new();
    let body: String = (1..=40).map(|n| format!("line {n}\r\n")).collect();
    let file = repo.root.join("src/review/lib.rs");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, body).unwrap();

    let row = &repo.show("F1", "audit")[0];
    assert_eq!(row["verdicts"].as_array().unwrap().len(), 2, "{row}");
    let excerpts = row["excerpts"].as_array().unwrap();
    let cited = excerpts
        .iter()
        .find(|e| e["citation"] == "src/review/lib.rs:30")
        .unwrap_or_else(|| panic!("no excerpt for the citation: {row}"));
    let lines: Vec<&str> = cited["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    let expected: Vec<String> = (25..=35).map(|n| format!("{n}: line {n}")).collect();
    assert_eq!(lines, expected);
    let range = excerpts
        .iter()
        .find(|e| e["citation"] == "src/review/lib.rs:12-13")
        .unwrap_or_else(|| panic!("no excerpt for the range: {row}"));
    assert_eq!(range["lines"][0], "7: line 7");
    assert_eq!(range["lines"].as_array().unwrap().len(), 12);
}

#[test]
fn show_for_backlog_emits_backlog_shape() {
    let repo = Repo::new();
    let rows = repo.show("F5,F6", "backlog");
    assert_eq!(rows[0]["id"], "F5");
    assert_eq!(rows[0]["kind"], "question");
    assert_eq!(rows[0]["area"], "src/review");
    assert_eq!(rows[0]["origin"], "review");
    assert_eq!(rows[0]["summary"], "the lock could be per-cell");
    let context = rows[0]["context"].as_str().unwrap();
    assert!(
        context.starts_with("contention grows with agents"),
        "{context}"
    );
    assert!(
        context.contains("Adoption cost: a store format change"),
        "{context}"
    );
    for not_backlog in ["angle", "rationale", "file", "lens", "status"] {
        assert!(
            rows[0].get(not_backlog).is_none(),
            "{not_backlog}: {}",
            rows[0]
        );
    }
    assert_eq!(rows[1]["kind"], "direction");
    assert!(
        rows[1].get("area").is_none(),
        "a root file has no area: {}",
        rows[1]
    );
}

#[test]
fn show_cell_carries_bound_catalogue_paths() {
    let repo = Repo::new();
    let out = repo.json(&["show", "C1", "--for", "finder"]);
    let cell = &out["items"][0];
    let builtin = slash(&repo.builtin);
    assert_eq!(cell["slice"][0], "src/review/**");
    assert_eq!(cell["question"], "does the limit hold?");
    assert_eq!(
        cell["bound"][0]["path"],
        format!("{builtin}/lenses/correctness.md")
    );
    assert_eq!(cell["facet_paths"][0]["id"], "rust");
    assert_eq!(
        cell["facet_paths"][0]["path"],
        format!("{builtin}/facets/rust.md")
    );
    assert_eq!(cell["method_paths"][0]["id"], "trace");
    assert_eq!(
        cell["method_paths"][0]["path"],
        format!("{builtin}/methods/trace.md")
    );
    assert_eq!(cell["pack"], ".claude/review-v2-runs/scope-t/U1/pack.md");
    assert_eq!(cell["baselines"]["lint-baseline"], "logs/lint.txt");
    assert_eq!(cell["baselines"]["profile-baseline"], "none — no profiler");
    assert!(cell.get("agent").is_none(), "{cell}");
    assert_eq!(
        out["references"]["finding-kinds"],
        format!(
            "{}/skills/review-v2-method/references/finding-kinds.md",
            slash(&repo.claude_dir)
        )
    );

    let refused = repo.review(&["show", "C1", "--for", "verifier"]).failure();
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&refused.get_output().stderr));
    assert_eq!(err["kind"], "validation");
}

#[test]
fn show_for_finder_cf_omits_the_pack() {
    let repo = Repo::new();
    let path = repo.root.join(".claude/review-v2/t.toml");
    let text = fs::read_to_string(&path).unwrap();
    let text = text
        .replacen(
            "agent = \"review-finder\"\n",
            "agent = \"review-finder-cf\"\n",
            1,
        )
        .replacen(
            "depth = \"standard\"\n",
            "depth = \"standard\"\nbase = \"e23b918\"\n",
            1,
        );
    fs::write(&path, text).unwrap();

    let cell = &repo.show("C1", "finder")[0];
    assert!(cell.get("pack").is_none(), "the cf cell has a pack: {cell}");
    assert_eq!(cell["base"], "e23b918", "{cell}");
    assert_eq!(cell["baselines"]["lint-baseline"], "logs/lint.txt");
    assert_eq!(cell["method_paths"][0]["id"], "trace");
}

/// A code lens with the frontmatter `review plan` binds against.
fn bindable_lens(id: &str) -> String {
    format!(
        "+++\nid = \"{id}\"\nstatus = \"stable\"\nfamilies = [\"code\"]\nmode = \"judgement\"\n\
         produces = [\"defect\"]\nowns = [\"{id}-tag\"]\nactivation = \"floor\"\n\
         slice = \"per-subsystem\"\nv1_category = \"quality\"\n+++\n## Question\nWhat does {id} ask?\n\n\
         ## Owns\n`{id}-tag` — owned here.\n\n## Probes — code\n- one probe\n\n\
         ## Does not flag\n- confirmations\n"
    )
}

#[test]
fn cf_agent_refused_outside_the_variant() {
    if !common::git_available() {
        eprintln!("skipping: git is not available");
        return;
    }
    let repo = Repo::new();
    for id in ["correctness", "naming"] {
        fs::write(
            repo.builtin.join(format!("lenses/{id}.md")),
            bindable_lens(id),
        )
        .unwrap();
    }
    let source = repo.root.join("src/a.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "fn f() {}\n").unwrap();
    let added = common::git_command(&repo.root)
        .args(["add", "-A"])
        .output()
        .unwrap();
    assert!(added.status.success(), "git add failed");
    fs::write(
        repo.root.join(".claude/review-v2/t.toml"),
        format!(
            "schema_version = 1\n\n[[runs]]\nid = \"U1\"\ncommand = \"review-v2\"\nfamily = \"code\"\n\
             depth = \"standard\"\nstarted = \"2026-10-12T10:00:00Z\"\nbase = \"abc\"\n\
             scope = [\"src/**\"]\nsignals = []\ncatalogue = {{ builtin = \"{}\" }}\n",
            slash(&repo.builtin)
        ),
    )
    .unwrap();
    let row = |concerns: &[&str]| {
        serde_json::json!({
            "slice": ["src/a.rs"],
            "concerns": concerns,
            "agent": "review-finder-cf",
            "reason": "proposed",
        })
        .to_string()
    };
    let payload = repo.root.join(".claude/payload.ndjson");
    fs::write(
        &payload,
        format!(
            "{}\n{}\n{}\n",
            row(&["naming"]),
            row(&["correctness", "naming"]),
            row(&["correctness"]),
        ),
    )
    .unwrap();

    let out = repo.json(&["plan", "--run", "U1", "--ndjson", &slash(&payload)]);
    let refused: Vec<u64> = out["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["line"].as_u64().unwrap())
        .collect();
    assert_eq!(refused, [1, 2], "{out}");
    for entry in out["refused"].as_array().unwrap() {
        let reasons = entry["reasons"].to_string();
        assert!(reasons.contains("review-finder-cf"), "{reasons}");
    }
    let cells = out["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 1, "{out}");
    assert_eq!(cells[0]["agent"], "review-finder-cf");
    assert_eq!(cells[0]["concerns"], serde_json::json!(["correctness"]));
}

#[test]
fn missing_store_reports_one_error_for_reads_and_writes() {
    let repo = Repo::new();
    fs::remove_file(repo.root.join(".claude/review-v2/t.toml")).unwrap();
    for args in [
        &["list"][..],
        &["list", "--strict-read"],
        &["show", "F1", "--for", "verifier"],
        &["digest", "--run", "U1"],
        &["resolve", "--run", "U1"],
    ] {
        let refused = repo.review(args).failure();
        let err = parse_json_error_envelope(&String::from_utf8_lossy(&refused.get_output().stderr));
        assert_eq!(err["kind"], "not_found", "{args:?}: {err}");
        let message = err["message"].as_str().unwrap();
        assert!(
            message.starts_with("review store not found at ") && message.ends_with("t.toml"),
            "{args:?}: {message}"
        );
    }
    assert!(
        !repo.root.join(".claude/review-v2/t.toml").exists(),
        "a refused write creates no store"
    );
}
