//! `tomlctl review export-v1` against a sandbox whose v2 store is seeded by
//! hand, with a temp-dir catalogue holding only the `[routing]` globs and
//! v1 ledgers written per test.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{cli, sandbox};
use serde_json::Value as JsonValue;

const STORE_PATH: &str = ".claude/review-v2/t.toml";
const LEDGER: &str = ".claude/reviews/t.toml";

const VOCABULARY: &str = r#"
[routing]
executable = ["**/*.rs"]
tests = ["tests/**"]
"#;

const FINDINGS: &str = r#"
[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["src/**"]
concerns = ["correctness", "type-design"]
agent = "review-finder"
reason = "proposed"
bound = [{ concern = "correctness", hash = "a1", path = "lenses/correctness.md", v1_category = "quality" }, { concern = "type-design", hash = "b2", path = "lenses/type-design.md", v1_category = "architecture" }]
status = "covered"
sealed = true
note = "done"

[[cells]]
id = "C2"
run = "U1"
wave = 1
slice = ["src/**"]
concerns = ["probe:locking"]
question = "does the lock outlive the write?"
agent = "review-finder"
reason = "lock handling changed"
status = "covered"
sealed = true
note = "done"

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
seen_in = ["U1", "U2"]
family = "code"
kind = "defect"
lens = "type-design"
route = "verifier"
file = "src/lib.rs"
line = 30
symbol = "lib::limit"
summary = "the limit is off by one"
severity = "major"
failure_scenario = "a ninth row is dropped"
fix = "raise LIMIT to 9"
fix_shape = "edits"
effort = "small"
counter = "callers never pass nine rows"
grade = "high"
evidence = ["src/lib.rs:30"]
status = "open"
first_seen = 2026-10-12

[[findings]]
id = "F2"
run = "U1"
cell = "C1"
family = "code"
kind = "advisory"
lens = "correctness"
route = "none"
file = "src/lib.rs"
summary = "run could stream"
angle = "alternative"
rationale = "memory grows with input"
adoption_cost = "an API change"
status = "open"
first_seen = 2026-10-12

[[findings]]
id = "F3"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 12
symbol = "lib::store"
summary = "the store has no migration story"
severity = "major"
failure_scenario = "an old store fails to load"
fix = "design a migration"
fix_shape = "needs-plan"
effort = "medium"
status = "open"
first_seen = 2026-10-12

[[findings]]
id = "F4"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "correctness"
route = "verifier"
file = "tests/old_test.rs"
line = 3
summary = "the test only restates the type"
edit = "delete the test"
direction = "remove"
old = "fn restates() {}"
effort = "trivial"
status = "open"
first_seen = 2026-10-12

[[findings]]
id = "F5"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "correctness"
route = "checker"
file = "docs/guide.md"
line = 3
summary = "the guide names a removed flag"
edit = "drop --fast"
direction = "change"
old = "Run with `--fast`."
effort = "trivial"
status = "open"
first_seen = 2026-10-12

[[findings]]
id = "F6"
run = "U1"
cell = "C2"
family = "code"
kind = "cleanup"
lens = "probe:locking"
route = "checker"
file = "src/lib.rs"
line = 4
summary = "dead helper"
edit = "remove the helper"
direction = "remove"
old = "fn helper() {}"
effort = "trivial"
depends_on = ["F1", "F9"]
status = "open"
first_seen = 2026-10-12

[[findings]]
id = "F7"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/io.rs"
line = 50
symbol = "io::lock"
summary = "the lock is released early"
severity = "minor"
failure_scenario = "two writers interleave"
fix = "hold the guard to the rename"
fix_shape = "edits"
effort = "trivial"
status = "open"
first_seen = 2026-10-12

[[findings]]
id = "F8"
run = "U1"
cell = "C1"
family = "perf"
kind = "cleanup"
lens = "correctness"
route = "checker"
file = "src/scan.rs"
line = 9
summary = "quadratic rescan"
edit = "index the rows"
direction = "change"
old = "rows.iter()"
effort = "small"
status = "open"
first_seen = 2026-10-12

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"
seat = 1
verdict = "confirmed"
severity = "critical"
citation = "src/lib.rs:30"
rationale = "reproduced with nine rows"

[[verdicts]]
finding = "F4"
run = "U1"
by = "verifier"
seat = 1
verdict = "confirmed"
citation = "tests/old_test.rs:3"
rationale = "no other guard needed"

[[verdicts]]
finding = "F5"
run = "U1"
by = "checker"
seat = 1
verdict = "unsettled"
citation = "docs/guide.md:3"
rationale = "flag may be an alias"

[[verdicts]]
finding = "F5"
run = "U1"
by = "verifier"
seat = 1
verdict = "plausible"
citation = "docs/guide.md:3"
rationale = "the flag is gone but an alias is undocumented"

[[verdicts]]
finding = "F6"
run = "U1"
by = "checker"
seat = 1
verdict = "confirmed"
citation = "src/lib.rs:4"
rationale = "no reference"

[[verdicts]]
finding = "F7"
run = "U1"
by = "verifier"
seat = 1
verdict = "confirmed"
citation = "src/io.rs:50"
rationale = "the guard drops before rename"
"#;

struct Repo {
    _dir: tempfile::TempDir,
    _catalogue: tempfile::TempDir,
    root: PathBuf,
    store: String,
}

impl Repo {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let catalogue = tempfile::tempdir().unwrap();
        let builtin = catalogue.path().join("review-v2-lenses");
        fs::create_dir_all(&builtin).unwrap();
        fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
        let builtin = builtin.to_string_lossy().replace('\\', "/");
        let run = |id: &str, started: &str| {
            format!(
                "[[runs]]\nid = \"{id}\"\ncommand = \"review-v2\"\nfamily = \"code\"\n\
                 depth = \"standard\"\nstarted = \"{started}\"\n\
                 catalogue = {{ builtin = \"{builtin}\" }}\n\n"
            )
        };
        let store = format!(
            "schema_version = 1\n\n{}{}{FINDINGS}",
            run("U1", "2026-10-12T10:00:00Z"),
            run("U2", "2026-10-20T09:00:00Z"),
        );
        let repo = Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
            store,
        };
        repo.reset_store();
        repo
    }

    /// Rewrites the store as seeded, as if an export's v2 write never landed.
    fn reset_store(&self) {
        self.write(STORE_PATH, &self.store);
    }

    fn write(&self, rel: &str, text: &str) {
        write(&self.root, rel, text);
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.root.join(rel)).unwrap()
    }

    fn export(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        cli(&self.root)
            .current_dir(&self.root)
            .args([
                "--error-format",
                "json",
                "review",
                "export-v1",
                "--scope",
                "t",
            ])
            .args(args)
            .args(["--to", LEDGER])
            .assert()
    }

    fn exported(&self, args: &[&str]) -> JsonValue {
        let out = self.export(args).success();
        serde_json::from_slice(&out.get_output().stdout).expect("export-v1 prints JSON")
    }

    fn ledger(&self) -> toml::Value {
        toml::from_str(&self.read(LEDGER)).unwrap()
    }

    fn rows(&self) -> Vec<toml::Value> {
        self.ledger()
            .get("items")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    fn row(&self, id: &str) -> toml::Value {
        self.rows()
            .into_iter()
            .find(|r| r["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no v1 row {id}"))
    }

    /// The v1 row the export placed `finding` in, by its `v2_ref`.
    fn row_for(&self, finding: &str) -> toml::Value {
        let wanted = format!("{STORE_PATH}#{finding}");
        self.rows()
            .into_iter()
            .find(|r| r.get("v2_ref").and_then(toml::Value::as_str) == Some(wanted.as_str()))
            .unwrap_or_else(|| panic!("no v1 row links {finding}"))
    }

    fn finding(&self, id: &str) -> toml::Value {
        let store: toml::Value = toml::from_str(&self.read(STORE_PATH)).unwrap();
        store["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no finding {id}"))
            .clone()
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn findings_in(out: &JsonValue, key: &str) -> Vec<String> {
    out[key]
        .as_array()
        .unwrap_or_else(|| panic!("no `{key}` in {out}"))
        .iter()
        .map(|e| e["finding"].as_str().unwrap().to_string())
        .collect()
}

fn text(row: &toml::Value, key: &str) -> String {
    row.get(key)
        .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
        .unwrap_or_default()
}

const OPEN_IO_LOCK: &str = r#"schema_version = 1
last_updated = 2026-10-12

[[items]]
id = "R1"
file = "src/io.rs"
line = 48
symbol = "io::lock"
severity = "warning"
effort = "small"
category = "quality"
summary = "lock guard dropped before the rename"
first_flagged = 2026-10-01
rounds = 1
status = "open"
"#;

#[test]
fn export_refuses_advisory_and_needs_plan() {
    let repo = Repo::new();
    let out = repo.exported(&["F2,F3,F6"]);
    let refused = findings_in(&out, "refused");
    assert_eq!(refused, ["F2", "F3"], "{out}");
    assert!(
        out["refused"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("advisory"),
        "{out}"
    );
    assert!(
        out["refused"][1]["reason"]
            .as_str()
            .unwrap()
            .contains("needs-plan"),
        "{out}"
    );
    assert_eq!(findings_in(&out, "exported"), ["F6"], "{out}");
    assert_eq!(repo.rows().len(), 1);
    assert_eq!(text(&repo.finding("F2"), "status"), "open");
    assert_eq!(text(&repo.finding("F3"), "status"), "open");

    let fresh = Repo::new();
    let err = fresh.export(&["F2,F3"]).failure();
    let stderr = String::from_utf8_lossy(&err.get_output().stderr).to_string();
    assert!(stderr.contains("F2") && stderr.contains("F3"), "{stderr}");
    assert!(
        !fresh.root.join(LEDGER).exists(),
        "an export refusing every finding writes no ledger"
    );
}

#[test]
fn export_all_skips_test_removals_and_plausible_cleanup() {
    let repo = Repo::new();
    let out = repo.exported(&["--all"]);
    let mut exported = findings_in(&out, "exported");
    exported.sort();
    assert_eq!(exported, ["F1", "F6", "F7"], "{out}");
    let held = &out["held"];
    assert_eq!(findings_in(&out, "held"), ["F4", "F5"], "{out}");
    assert_eq!(held[0]["reason"], "test removal", "{out}");
    assert!(
        held[1]["reason"].as_str().unwrap().contains("plausible"),
        "{out}"
    );
    assert_eq!(text(&repo.finding("F4"), "status"), "open");
    assert_eq!(text(&repo.finding("F5"), "status"), "open");
    assert_eq!(
        text(&repo.finding("F8"), "status"),
        "open",
        "a perf finding stays out of a review ledger"
    );

    let out = repo.exported(&["F4", "F5"]);
    assert_eq!(
        findings_in(&out, "exported"),
        ["F4", "F5"],
        "an explicit id still exports: {out}"
    );
}

#[test]
fn export_uses_reassessed_severity() {
    let repo = Repo::new();
    repo.exported(&["F1,F7"]);
    assert_eq!(
        text(&repo.row_for("F1"), "severity"),
        "critical",
        "the verifier's reassessment wins over the finder's major"
    );
    assert_eq!(text(&repo.row_for("F7"), "severity"), "suggestion");
}

#[test]
fn export_category_from_cell_binding() {
    let repo = Repo::new();
    repo.exported(&["F1,F6,F7"]);
    assert_eq!(text(&repo.row_for("F1"), "category"), "architecture");
    assert_eq!(text(&repo.row_for("F7"), "category"), "quality");
    assert_eq!(
        text(&repo.row_for("F6"), "category"),
        "quality",
        "a probe cell takes the family default"
    );
    let row = repo.row_for("F1");
    for key in [
        "id",
        "file",
        "line",
        "severity",
        "effort",
        "category",
        "summary",
        "first_flagged",
        "rounds",
        "status",
    ] {
        assert!(row.get(key).is_some(), "v1 row lacks `{key}`: {row:?}");
    }
    assert_eq!(text(&row, "effort"), "small");
    assert_eq!(text(&row, "status"), "open");
    assert!(row["first_flagged"].as_datetime().is_some(), "{row:?}");
    let description = text(&row, "description");
    assert!(
        description.contains("raise LIMIT to 9") && description.contains("a ninth row is dropped"),
        "{description}"
    );
    assert!(
        !description.contains("callers never pass"),
        "the counter is not the implementer's: {description}"
    );
}

#[test]
fn export_reuses_open_v1_id_and_bumps_rounds_once() {
    let repo = Repo::new();
    repo.write(LEDGER, OPEN_IO_LOCK);
    let out = repo.exported(&["F7"]);
    assert_eq!(out["exported"][0]["v1"], "R1", "{out}");
    assert_eq!(out["exported"][0]["action"], "reused", "{out}");
    assert_eq!(repo.rows().len(), 1);
    let row = repo.row("R1");
    assert_eq!(row["rounds"].as_integer(), Some(2));
    assert_eq!(text(&row, "v2_ref"), format!("{STORE_PATH}#F7"));
    assert_eq!(
        text(&row, "summary"),
        "lock guard dropped before the rename",
        "the v1 row keeps its text"
    );

    repo.reset_store();
    let out = repo.exported(&["F7"]);
    assert_eq!(out["exported"][0]["action"], "linked", "{out}");
    assert_eq!(
        repo.row("R1")["rounds"].as_integer(),
        Some(2),
        "a re-export of the same v2_ref does not bump rounds again"
    );
}

#[test]
fn export_is_idempotent_on_v2_ref() {
    let repo = Repo::new();
    let out = repo.exported(&["F1"]);
    assert_eq!(out["exported"][0]["v1"], "R1", "{out}");
    assert_eq!(out["exported"][0]["action"], "minted", "{out}");
    let f1 = repo.finding("F1");
    assert_eq!(text(&f1, "status"), "exported");
    assert_eq!(text(&f1, "exported_as"), "R1");
    assert_eq!(text(&f1, "export_ledger"), LEDGER);
    let ledger_after = repo.read(LEDGER);

    repo.reset_store();
    let out = repo.exported(&["F1"]);
    assert_eq!(out["exported"][0]["v1"], "R1", "{out}");
    assert_eq!(out["exported"][0]["action"], "linked", "{out}");
    assert_eq!(
        repo.read(LEDGER),
        ledger_after,
        "the v1 ledger is not rewritten"
    );
    assert_eq!(text(&repo.finding("F1"), "exported_as"), "R1");

    let out = repo.exported(&["F1,F6"]);
    assert_eq!(
        findings_in(&out, "refused"),
        ["F1"],
        "an exported finding is not open: {out}"
    );
    let r2 = repo.row_for("F6");
    assert_eq!(
        r2["depends_on"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["R1"],
        "a dependency already exported to this ledger maps to its v1 id"
    );
    assert_eq!(out["dropped_deps"][0]["depends_on"], "F9", "{out}");
}

#[test]
fn export_regression_on_fixed_v1_match() {
    let repo = Repo::new();
    repo.write(
        LEDGER,
        &OPEN_IO_LOCK
            .replace("symbol = \"io::lock\"\n", "")
            .replace("lock guard dropped before the rename", "the lock is released early")
            .replace(
                "status = \"open\"\n",
                "status = \"fixed\"\nresolved = 2026-10-05\nresolution = \"abc123 held the guard\"\n",
            ),
    );
    let out = repo.exported(&["F7"]);
    assert_eq!(out["exported"][0]["action"], "regression", "{out}");
    assert_eq!(out["exported"][0]["v1"], "R2", "{out}");
    let row = repo.row("R2");
    assert_eq!(
        row["related"].as_array().unwrap()[0].as_str(),
        Some("R1"),
        "{row:?}"
    );
    assert_eq!(
        text(&repo.row("R1"), "status"),
        "fixed",
        "the fixed row is untouched"
    );
}

fn snapshot(repo: &Repo, rel: &str) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let read = |p: PathBuf| fs::read(p).ok();
    (
        read(repo.root.join(rel)),
        read(repo.root.join(format!("{rel}.sha256"))),
    )
}

#[test]
fn export_dry_run_writes_neither_ledger_nor_store() {
    let repo = Repo::new();
    let store_before = snapshot(&repo, STORE_PATH);
    let out = repo.exported(&["--dry-run", "F1"]);
    assert_eq!(out["dry_run"], true, "{out}");
    assert_eq!(
        findings_in(&out["would_change"], "exported"),
        ["F1"],
        "{out}"
    );
    assert_eq!(
        snapshot(&repo, STORE_PATH),
        store_before,
        "v2 store untouched"
    );
    assert_eq!(
        snapshot(&repo, LEDGER),
        (None, None),
        "no v1 ledger created"
    );

    repo.write(LEDGER, OPEN_IO_LOCK);
    let ledger_before = snapshot(&repo, LEDGER);
    let out = repo.exported(&["--dry-run", "F7"]);
    assert_eq!(
        findings_in(&out["would_change"], "exported"),
        ["F7"],
        "{out}"
    );
    assert_eq!(
        snapshot(&repo, LEDGER),
        ledger_before,
        "v1 ledger untouched"
    );
    assert_eq!(
        snapshot(&repo, STORE_PATH),
        store_before,
        "v2 store untouched"
    );
}

#[test]
fn export_stamps_last_updated_from_run_date() {
    let repo = Repo::new();
    repo.exported(&["F1"]);
    assert_eq!(
        text(&repo.ledger(), "last_updated"),
        "2026-10-20",
        "a new ledger is seeded with the newest seen_in run's date"
    );

    let older = Repo::new();
    older.write(
        LEDGER,
        &OPEN_IO_LOCK.replace("last_updated = 2026-10-12", "last_updated = 2026-10-01"),
    );
    older.exported(&["F6"]);
    assert_eq!(text(&older.ledger(), "last_updated"), "2026-10-12");

    let newer = Repo::new();
    newer.write(
        LEDGER,
        &OPEN_IO_LOCK.replace("last_updated = 2026-10-12", "last_updated = 2026-10-15"),
    );
    newer.exported(&["F6"]);
    assert_eq!(
        text(&newer.ledger(), "last_updated"),
        "2026-10-15",
        "a later ledger date is kept"
    );
}
