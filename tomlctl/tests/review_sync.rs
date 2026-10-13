//! `tomlctl review sync-v1` against a sandbox whose v2 store holds exported
//! findings and whose v1 ledgers carry the `v2_ref`s an export writes.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{cli, sandbox};
use serde_json::Value as JsonValue;

const STORE_PATH: &str = ".claude/review-v2/t.toml";
const REVIEW_LEDGER: &str = ".claude/reviews/t.toml";
const OPTIMISE_LEDGER: &str = ".claude/optimise-findings/t.toml";
const OTHER_LEDGER: &str = ".claude/reviews/elsewhere.toml";

const STORE: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-12T10:00:00Z"

[[runs]]
id = "U2"
command = "optimise-v2"
family = "perf"
depth = "standard"
started = "2026-10-12T11:00:00Z"

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 30
summary = "the limit is off by one"
severity = "major"
status = "exported"
exported_as = "R1"
export_ledger = ".claude/reviews/t.toml"

[[findings]]
id = "F2"
run = "U2"
cell = "C2"
family = "perf"
kind = "cleanup"
lens = "algorithm"
route = "checker"
file = "src/scan.rs"
line = 12
summary = "quadratic rescan"
status = "exported"
exported_as = "O1"
export_ledger = ".claude/optimise-findings/t.toml"

[[findings]]
id = "F3"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "hygiene"
route = "checker"
file = "src/lib.rs"
line = 4
summary = "dead helper"
status = "exported"
exported_as = "R2"
export_ledger = ".claude/reviews/t.toml"

[[findings]]
id = "F4"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "hygiene"
route = "checker"
file = "src/lib.rs"
line = 8
summary = "stale comment"
status = "exported"
exported_as = "R3"
export_ledger = ".claude/reviews/t.toml"

[[findings]]
id = "F5"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/io.rs"
line = 50
summary = "the lock is released early"
severity = "major"
status = "exported"
exported_as = "R1"
export_ledger = ".claude/reviews/elsewhere.toml"
"#;

const REVIEW: &str = r#"schema_version = 1
last_updated = 2026-10-12

[[items]]
id = "R1"
file = "src/lib.rs"
line = 30
severity = "warning"
effort = "small"
category = "quality"
summary = "the limit is off by one"
first_flagged = 2026-10-12
rounds = 1
status = "fixed"
resolved = 2026-10-13
resolution = "abc123 bound the limit"
v2_ref = ".claude/review-v2/t.toml#F1"

[[items]]
id = "R2"
file = "src/lib.rs"
line = 4
severity = "suggestion"
effort = "trivial"
category = "quality"
summary = "dead helper"
first_flagged = 2026-10-12
rounds = 1
status = "wontfix"
wontfix_rationale = "kept for the plugin ABI"
v2_ref = ".claude/review-v2/t.toml#F3"

[[items]]
id = "R3"
file = "src/lib.rs"
line = 8
severity = "suggestion"
effort = "trivial"
category = "quality"
summary = "stale comment"
first_flagged = 2026-10-12
rounds = 1
status = "open"
v2_ref = ".claude/review-v2/t.toml#F4"

[[items]]
id = "R4"
file = "src/other.rs"
line = 1
severity = "suggestion"
effort = "trivial"
category = "quality"
summary = "another store's finding"
first_flagged = 2026-10-12
rounds = 1
status = "fixed"
resolved = 2026-10-13
resolution = "def456"
v2_ref = ".claude/review-v2/other.toml#F1"
"#;

const OPTIMISE: &str = r#"schema_version = 1
last_updated = 2026-10-12

[[items]]
id = "O1"
file = "src/scan.rs"
line = 12
severity = "suggestion"
effort = "small"
category = "algorithm"
summary = "quadratic rescan"
first_flagged = 2026-10-12
rounds = 1
status = "applied"
resolved = 2026-10-14
resolution = "fed789 index the rows"
v2_ref = ".claude/review-v2/t.toml#F2"
"#;

const ELSEWHERE: &str = r#"schema_version = 1
last_updated = 2026-10-12

[[items]]
id = "R1"
file = "src/io.rs"
line = 50
severity = "warning"
effort = "small"
category = "quality"
summary = "the lock is released early"
first_flagged = 2026-10-12
rounds = 1
status = "deferred"
defer_reason = "needs the lock rework"
defer_trigger = "when io.rs moves to fd-lock"
v2_ref = ".claude/review-v2/t.toml#F5"
"#;

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let repo = Self { _dir: dir, root };
        repo.write(STORE_PATH, STORE);
        repo
    }

    fn with_ledgers() -> Self {
        let repo = Self::new();
        repo.write(REVIEW_LEDGER, REVIEW);
        repo.write(OPTIMISE_LEDGER, OPTIMISE);
        repo.write(OTHER_LEDGER, ELSEWHERE);
        repo
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.root.join(rel)).unwrap()
    }

    fn sync(&self) -> JsonValue {
        let out = cli(&self.root)
            .current_dir(&self.root)
            .args([
                "--error-format",
                "json",
                "review",
                "sync-v1",
                "--scope",
                "t",
            ])
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("sync-v1 prints JSON")
    }

    fn triage(&self, id: &str, args: &[&str]) {
        cli(&self.root)
            .current_dir(&self.root)
            .args(["review", "triage", id, "--scope", "t"])
            .args(args)
            .assert()
            .success();
    }

    fn store(&self) -> toml::Value {
        toml::from_str(&self.read(STORE_PATH)).unwrap()
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

    fn field(&self, id: &str, key: &str) -> Option<String> {
        self.finding(id)
            .get(key)
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
    }

    fn sync_verdicts(&self) -> Vec<toml::Value> {
        self.store()
            .get("verdicts")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|v| v["by"].as_str() == Some("sync"))
            .collect()
    }
}

/// `text` with its one occurrence of `from` replaced by `to`.
fn swapped(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from:?} occurs once");
    text.replacen(from, to, 1)
}

const R1_FIXED: &str = "status = \"fixed\"\nresolved = 2026-10-13\n\
                        resolution = \"abc123 bound the limit\"\n\
                        v2_ref = \".claude/review-v2/t.toml#F1\"";
const R1_REOPENED: &str = "status = \"open\"\nreopen_rationale = \"the bound regressed\"\n\
                           v2_ref = \".claude/review-v2/t.toml#F1\"";

fn sync_verdict(repo: &Repo, id: &str) -> toml::Value {
    repo.sync_verdicts()
        .into_iter()
        .find(|v| v["finding"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("{id} has a sync verdict"))
}

#[test]
fn reopened_finding_survives_sync() {
    let repo = Repo::with_ledgers();
    repo.sync();
    assert_eq!(repo.field("F1", "status").as_deref(), Some("resolved"));

    repo.triage("F1", &["--reopen", "the bound is still wrong"]);
    let out = repo.sync();
    assert_eq!(
        repo.field("F1", "status").as_deref(),
        Some("open"),
        "the fixed v1 row no longer settles a reopened finding: {out}"
    );
    assert_eq!(
        repo.field("F1", "reopen_reason").as_deref(),
        Some("the bound is still wrong")
    );
    assert!(
        out["skipped"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!({
                "v1": ".claude/reviews/t.toml#R1",
                "finding": "F1",
                "reason": "link retired by reopen",
            })),
        "{out}"
    );
}

#[test]
fn v1_reopen_syncs_back_to_open() {
    let repo = Repo::with_ledgers();
    repo.sync();
    repo.write(REVIEW_LEDGER, &swapped(REVIEW, R1_FIXED, R1_REOPENED));

    let out = repo.sync();
    assert_eq!(repo.field("F1", "status").as_deref(), Some("open"), "{out}");
    assert_eq!(
        repo.field("F1", "reopen_reason").as_deref(),
        Some("v1: the bound regressed")
    );
    for gone in ["resolution", "resolved"] {
        assert_eq!(repo.field("F1", gone), None, "{gone} is cleared");
    }
    assert_eq!(
        repo.field("F1", "exported_as").as_deref(),
        Some("R1"),
        "the link stays, so a later v1 outcome syncs again"
    );
    assert_eq!(
        sync_verdict(&repo, "F1")["rationale"].as_str(),
        Some("open: the bound regressed")
    );
    assert_eq!(
        repo.field("F4", "status").as_deref(),
        Some("exported"),
        "a v1 row open without a reopen rationale is still skipped"
    );
    assert_eq!(repo.sync()["unchanged"], true, "a replay writes nothing");
}

#[test]
fn v2_disposition_after_v1_reopen_survives_sync() {
    let repo = Repo::with_ledgers();
    repo.sync();
    repo.write(REVIEW_LEDGER, &swapped(REVIEW, R1_FIXED, R1_REOPENED));
    repo.sync();
    assert_eq!(repo.field("F1", "status").as_deref(), Some("open"));

    repo.triage("F1", &["--dismiss", "not worth the churn"]);
    let out = repo.sync();
    assert_eq!(
        repo.field("F1", "status").as_deref(),
        Some("dismissed"),
        "a reopen already taken is not taken again: {out}"
    );
    assert_eq!(
        repo.field("F1", "dismiss_reason").as_deref(),
        Some("not worth the churn")
    );
    assert_eq!(out["unchanged"], true, "{out}");
}

#[test]
fn sync_clears_a_stale_reopen_reason() {
    let repo = Repo::with_ledgers();
    repo.sync();
    repo.write(REVIEW_LEDGER, &swapped(REVIEW, R1_FIXED, R1_REOPENED));
    repo.sync();
    assert!(repo.field("F1", "reopen_reason").is_some());

    repo.write(REVIEW_LEDGER, REVIEW);
    repo.sync();
    assert_eq!(repo.field("F1", "status").as_deref(), Some("resolved"));
    assert_eq!(
        repo.field("F1", "reopen_reason"),
        None,
        "the status it leaves takes its companion with it"
    );
    assert_eq!(repo.field("F1", "resolved").as_deref(), Some("2026-10-13"));
}

#[test]
fn array_v2_ref_links_each_named_finding() {
    let repo = Repo::new();
    let review = swapped(
        REVIEW,
        "v2_ref = \".claude/review-v2/t.toml#F3\"",
        "v2_ref = [\".claude/review-v2/t.toml#F3\", \".claude/review-v2/t.toml#F4\"]",
    );
    repo.write(REVIEW_LEDGER, &review);
    repo.sync();
    for id in ["F3", "F4"] {
        assert_eq!(
            repo.field(id, "dismiss_reason").as_deref(),
            Some("v1: wontfix: kept for the plugin ABI"),
            "{id}"
        );
    }
}

#[test]
fn fixed_v1_row_resolves_v2_finding() {
    let repo = Repo::with_ledgers();
    repo.sync();
    for (id, resolution, date) in [
        ("F1", "v1: fixed: abc123 bound the limit", "2026-10-13"),
        ("F2", "v1: applied: fed789 index the rows", "2026-10-14"),
    ] {
        assert_eq!(
            repo.field(id, "status").as_deref(),
            Some("resolved"),
            "{id}"
        );
        assert_eq!(repo.field(id, "resolution").as_deref(), Some(resolution));
        assert_eq!(repo.field(id, "resolved").as_deref(), Some(date));
        assert!(
            repo.field(id, "exported_as").is_some(),
            "{id} keeps its export link for `stats --outcomes`"
        );
    }
    assert_eq!(
        repo.field("F4", "status").as_deref(),
        Some("exported"),
        "a v1 row still open leaves its finding exported"
    );
}

#[test]
fn wontfix_v1_row_dismisses_with_reason() {
    let repo = Repo::with_ledgers();
    repo.sync();
    assert_eq!(repo.field("F3", "status").as_deref(), Some("dismissed"));
    assert_eq!(
        repo.field("F3", "dismiss_reason").as_deref(),
        Some("v1: wontfix: kept for the plugin ABI")
    );
}

#[test]
fn deferred_v1_row_defers_with_trigger() {
    let repo = Repo::with_ledgers();
    repo.sync();
    assert_eq!(repo.field("F5", "status").as_deref(), Some("deferred"));
    assert_eq!(
        repo.field("F5", "defer_reason").as_deref(),
        Some("needs the lock rework")
    );
    assert_eq!(
        repo.field("F5", "defer_trigger").as_deref(),
        Some("when io.rs moves to fd-lock")
    );
}

#[test]
fn sync_reads_recorded_export_ledger() {
    let repo = Repo::new();
    repo.write(OTHER_LEDGER, ELSEWHERE);
    let out = repo.sync();
    assert_eq!(
        repo.field("F5", "status").as_deref(),
        Some("deferred"),
        "a ledger named only by `export_ledger` is read: {out}"
    );
    let paths: Vec<&str> = out["ledgers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["path"].as_str())
        .collect();
    assert!(paths.contains(&OTHER_LEDGER), "{out}");
    assert!(
        paths.contains(&REVIEW_LEDGER) && paths.contains(&OPTIMISE_LEDGER),
        "each held family's default ledger is read too: {out}"
    );
}

#[test]
fn sync_verdict_carries_ledger_citation() {
    let repo = Repo::with_ledgers();
    repo.sync();
    let verdicts = repo.sync_verdicts();
    let f1 = verdicts
        .iter()
        .find(|v| v["finding"].as_str() == Some("F1"))
        .expect("F1 gains a sync verdict");
    assert_eq!(f1["verdict"].as_str(), Some("confirmed"));
    assert_eq!(f1["run"].as_str(), Some("U1"));
    assert_eq!(f1["citation"].as_str(), Some(".claude/reviews/t.toml#R1"));
    assert_eq!(
        f1["rationale"].as_str(),
        Some("fixed: abc123 bound the limit")
    );
    assert!(f1.get("seat").is_none(), "{f1:?}");

    let f5 = verdicts
        .iter()
        .find(|v| v["finding"].as_str() == Some("F5"))
        .expect("F5 gains a sync verdict");
    assert_eq!(
        f5["citation"].as_str(),
        Some(".claude/reviews/elsewhere.toml#R1")
    );
    assert!(
        verdicts.iter().all(|v| v["finding"].as_str() != Some("F4")),
        "an open v1 row writes no verdict"
    );
    assert_eq!(verdicts.len(), 4, "{verdicts:?}");
}

#[test]
fn sync_is_idempotent() {
    let repo = Repo::with_ledgers();
    let first = repo.sync();
    assert_eq!(first["unchanged"], false, "{first}");
    let after_first = repo.read(STORE_PATH);
    let second = repo.sync();
    assert_eq!(second["unchanged"], true, "{second}");
    assert_eq!(
        repo.read(STORE_PATH),
        after_first,
        "a replay writes nothing"
    );
    assert_eq!(repo.sync_verdicts().len(), 4);
    assert_eq!(repo.read(REVIEW_LEDGER), REVIEW, "v1 ledgers are only read");
    assert_eq!(repo.read(OPTIMISE_LEDGER), OPTIMISE);
}

#[test]
fn missing_v1_ledger_is_a_no_op() {
    let repo = Repo::new();
    let out = repo.sync();
    assert_eq!(out["unchanged"], true, "{out}");
    assert_eq!(out["synced"], serde_json::json!([]), "{out}");
    assert!(
        out["ledgers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["missing"] == true),
        "{out}"
    );
    assert_eq!(repo.read(STORE_PATH), STORE, "the store is not rewritten");
    assert!(
        !repo.root.join(REVIEW_LEDGER).exists(),
        "no v1 ledger is created"
    );

    let (_dir, bare) = sandbox();
    let out = cli(&bare)
        .current_dir(&bare)
        .args(["review", "sync-v1", "--scope", "t"])
        .assert()
        .success();
    let out: JsonValue = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(out["unchanged"], true, "{out}");
    assert!(
        !bare.join(STORE_PATH).exists(),
        "an absent store is not created"
    );
}
