//! `tomlctl review verdict` against a sandbox whose store is seeded by hand
//! with a finder cell, a sweeper cell and unverified findings of each route.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{cli, parse_json_error_envelope, sandbox};
use serde_json::{Value as JsonValue, json};

const SCOPE: &str = "t";

const STORE: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "deep"
started = "2026-10-12T10:00:00Z"

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["src/**"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "done"

[[cells]]
id = "C2"
run = "U1"
wave = 1
slice = ["src/**"]
concerns = ["hygiene"]
agent = "review-sweeper"
reason = "proposed"
status = "covered"
sealed = true
note = "done"

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
symbol = "lib::limit"
summary = "the limit is off by one"
severity = "major"
status = "unverified"

[[findings]]
id = "F2"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "correctness"
route = "checker"
file = "docs/guide.md"
line = 3
summary = "stale flag in the guide"
status = "unverified"

[[findings]]
id = "F3"
run = "U1"
cell = "C2"
family = "code"
kind = "cleanup"
lens = "hygiene"
route = "checker"
file = "src/lib.rs"
line = 4
summary = "dead helper"
status = "unverified"

[[findings]]
id = "F4"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "panel"
file = "src/lib.rs"
line = 10
symbol = "lib::open"
summary = "the store is opened unlocked"
severity = "critical"
status = "unverified"

[[findings]]
id = "F5"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 31
symbol = "lib::limit"
summary = "the limit is off by one again"
severity = "major"
same_as = "F1"
status = "unverified"
"#;

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let store = root.join(".claude/review-v2/t.toml");
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(store, STORE).unwrap();
        Self { _dir: dir, root }
    }

    fn verdict(&self, finding: &str, args: &[&str]) -> assert_cmd::assert::Assert {
        cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "verdict", finding])
            .args(["--scope", SCOPE])
            .args(args)
            .assert()
    }

    fn verdicted(&self, finding: &str, args: &[&str]) -> JsonValue {
        let out = self.verdict(finding, args).success();
        serde_json::from_slice(&out.get_output().stdout).expect("verdict prints JSON")
    }

    fn batch(&self, rows: &[JsonValue]) -> assert_cmd::assert::Assert {
        let payload = self.root.join(".claude/verdicts.ndjson");
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        fs::write(&payload, text).unwrap();
        cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "verdict"])
            .args(["--scope", SCOPE, "--ndjson"])
            .arg(payload)
            .assert()
    }

    fn batched(&self, rows: &[JsonValue]) -> JsonValue {
        let out = self.batch(rows).success();
        serde_json::from_slice(&out.get_output().stdout).expect("verdict prints JSON")
    }

    fn raw(&self) -> String {
        fs::read_to_string(self.root.join(".claude/review-v2/t.toml")).unwrap()
    }

    fn store(&self) -> toml::Value {
        toml::from_str(&self.raw()).unwrap()
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

    fn status(&self, id: &str) -> String {
        self.finding(id)["status"].as_str().unwrap().to_string()
    }

    fn verdicts(&self) -> Vec<toml::Value> {
        self.store()
            .get("verdicts")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    }
}

fn row(finding: &str, by: &str, verdict: &str) -> JsonValue {
    json!({
        "finding": finding,
        "by": by,
        "verdict": verdict,
        "citation": "src/lib.rs:30",
        "rationale": "checked the anchor",
    })
}

const CONFIRMED: &[&str] = &[
    "--verdict",
    "confirmed",
    "--by",
    "verifier",
    "--citation",
    "src/lib.rs:30",
    "--rationale",
    "reproduced with nine rows",
];

#[test]
fn confirmed_unverified_becomes_open() {
    let repo = Repo::new();
    let out = repo.verdicted("F1", CONFIRMED);
    assert_eq!(
        out["verdicts"],
        json!([{"finding": "F1", "verdict": "confirmed"}]),
        "the verdict-return object carries no reroute: {out}"
    );
    assert_eq!(repo.status("F1"), "open");
    let verdicts = repo.verdicts();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0]["run"].as_str(), Some("U1"));
    assert_eq!(verdicts[0]["citation"].as_str(), Some("src/lib.rs:30"));
    assert_eq!(
        verdicts[0]["rationale"].as_str(),
        Some("reproduced with nine rows")
    );
}

#[test]
fn refuted_unverified_becomes_refuted() {
    let repo = Repo::new();
    repo.verdicted(
        "F1",
        &[
            "--verdict",
            "refuted",
            "--by",
            "verifier",
            "--citation",
            "src/lib.rs:28",
            "--rationale",
            "callers cap the input at eight",
        ],
    );
    assert_eq!(repo.status("F1"), "refuted");
}

#[test]
fn replayed_verdict_upserts_not_appends() {
    let repo = Repo::new();
    repo.verdicted("F1", CONFIRMED);
    let after_first = repo.raw();

    let replay = repo.verdicted("F1", CONFIRMED);
    assert_eq!(replay["unchanged"], true, "{replay}");
    assert_eq!(
        repo.raw(),
        after_first,
        "an identical replay must not rewrite the store"
    );

    repo.verdicted(
        "F1",
        &[
            "--verdict",
            "refuted",
            "--by",
            "verifier",
            "--citation",
            "src/lib.rs:28",
            "--rationale",
            "on a second look, callers cap the input",
        ],
    );
    let verdicts = repo.verdicts();
    assert_eq!(verdicts.len(), 1, "one verdict per (finding, by, seat)");
    assert_eq!(verdicts[0]["verdict"].as_str(), Some("refuted"));
    assert_eq!(repo.status("F1"), "refuted");

    repo.verdicted(
        "F1",
        &[
            "--verdict",
            "confirmed",
            "--by",
            "audit",
            "--citation",
            "src/lib.rs:30",
            "--rationale",
            "the cap is not enforced",
        ],
    );
    assert_eq!(repo.verdicts().len(), 2, "another `by` is another verdict");
    assert_eq!(repo.status("F1"), "open");
}

#[test]
fn checker_unsettled_reroutes_to_verifier() {
    let repo = Repo::new();
    let out = repo.batched(&[row("F2", "checker", "unsettled")]);
    assert_eq!(
        out["verdicts"],
        json!([{"finding": "F2", "verdict": "unsettled", "reroute": "verifier"}]),
        "{out}"
    );
    let finding = repo.finding("F2");
    assert_eq!(finding["route"].as_str(), Some("verifier"));
    assert_eq!(finding["status"].as_str(), Some("unverified"));

    let refused = repo.batch(&[row("F1", "verifier", "unsettled")]).failure();
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&refused.get_output().stderr));
    assert_eq!(err["kind"], "validation");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("unsettled only from the checker"),
        "{err}"
    );
}

#[test]
fn checker_cannot_refute_a_finder_row() {
    let repo = Repo::new();
    let out = repo.batched(&[
        row("F2", "checker", "refuted"),
        row("F3", "checker", "refuted"),
    ]);
    assert_eq!(
        out["verdicts"],
        json!([{"finding": "F3", "verdict": "refuted"}]),
        "only the sweeper's row is refuted: {out}"
    );
    assert_eq!(out["refused"][0]["finding"], "F2", "{out}");
    assert!(
        out["refused"][0]["reasons"]
            .to_string()
            .contains("only a row a sweeper wrote"),
        "{out}"
    );
    assert_eq!(repo.status("F2"), "unverified");
    assert_eq!(repo.status("F3"), "refuted");
    assert_eq!(repo.verdicts().len(), 1);
}

#[test]
fn panel_needs_two_non_refuted() {
    let repo = Repo::new();
    let seat = |n: u32, verdict: &str| {
        let mut vote = row("F4", "panel", verdict);
        vote["seat"] = json!(n);
        vote
    };
    repo.batched(&[seat(1, "confirmed")]);
    assert_eq!(repo.status("F4"), "unverified", "one vote decides nothing");
    repo.batched(&[seat(2, "refuted")]);
    assert_eq!(repo.status("F4"), "unverified", "one vote each way");
    repo.batched(&[seat(3, "plausible")]);
    assert_eq!(repo.status("F4"), "open", "two non-refuted of three");
    assert_eq!(repo.verdicts().len(), 3);

    repo.batched(&[seat(3, "refuted")]);
    assert_eq!(
        repo.status("F4"),
        "refuted",
        "a seat re-voting replaces its vote"
    );
    assert_eq!(repo.verdicts().len(), 3);

    let mut seatless = row("F4", "panel", "confirmed");
    seatless.as_object_mut().unwrap().remove("seat");
    let refused = repo.batch(&[seatless]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(stderr.contains("`seat` is required"), "{stderr}");
}

#[test]
fn panel_counts_each_seat_once_by_its_latest_verdict() {
    let repo = Repo::new();
    let seeded = |finding: &str, seat: u32, verdict: &str| {
        format!(
            "\n[[verdicts]]\nfinding = \"{finding}\"\nrun = \"U1\"\nby = \"panel\"\nseat = {seat}\nverdict = \"{verdict}\"\ncitation = \"src/lib.rs:10\"\nrationale = \"copied\"\n"
        )
    };
    let seat2 = |finding: &str, verdict: &str| {
        let mut vote = row(finding, "panel", verdict);
        vote["seat"] = json!(2);
        vote
    };

    // Seat 1 voted twice (refuted, then confirmed): it is one standing seat.
    let mut text = repo.raw();
    text.push_str(&seeded("F4", 1, "refuted"));
    text.push_str(&seeded("F4", 1, "confirmed"));
    // Seat 1 refuted twice on F1: two copies are one refuting seat.
    text.push_str(&seeded("F1", 1, "refuted"));
    text.push_str(&seeded("F1", 1, "refuted"));
    fs::write(repo.root.join(".claude/review-v2/t.toml"), text).unwrap();

    repo.batched(&[seat2("F4", "confirmed")]);
    assert_eq!(
        repo.status("F4"),
        "open",
        "seat 1 standing + seat 2 standing"
    );

    repo.batched(&[seat2("F1", "confirmed")]);
    assert_eq!(
        repo.status("F1"),
        "unverified",
        "one refuting seat copied twice is not two refuting seats"
    );
}

#[test]
fn panel_latest_seat_vote_overrides_an_earlier_one() {
    let repo = Repo::new();
    let mut text = repo.raw();
    for verdict in ["confirmed", "refuted"] {
        text.push_str(&format!(
            "\n[[verdicts]]\nfinding = \"F4\"\nrun = \"U1\"\nby = \"panel\"\nseat = 1\nverdict = \"{verdict}\"\ncitation = \"src/lib.rs:10\"\nrationale = \"copied\"\n"
        ));
    }
    fs::write(repo.root.join(".claude/review-v2/t.toml"), text).unwrap();
    let mut vote = row("F4", "panel", "confirmed");
    vote["seat"] = json!(2);
    repo.batched(&[vote]);
    assert_eq!(
        repo.status("F4"),
        "unverified",
        "seat 1's latest vote is refuted, so only seat 2 stands"
    );
}

#[test]
fn severity_reassessment_is_recorded() {
    let repo = Repo::new();
    let mut args = CONFIRMED.to_vec();
    args.extend(["--severity", "minor"]);
    repo.verdicted("F1", &args);
    assert_eq!(repo.verdicts()[0]["severity"].as_str(), Some("minor"));
    assert_eq!(
        repo.finding("F1")["severity"].as_str(),
        Some("major"),
        "the finder's severity is kept beside the reassessment"
    );

    let mut on_cleanup = row("F2", "checker", "confirmed");
    on_cleanup["severity"] = json!("minor");
    let refused = repo.batch(&[on_cleanup]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(stderr.contains("reassesses a defect"), "{stderr}");
}

#[test]
fn verdict_without_citation_is_refused() {
    let repo = Repo::new();
    let out = repo
        .verdict(
            "F1",
            &[
                "--verdict",
                "confirmed",
                "--by",
                "verifier",
                "--rationale",
                "trust me",
            ],
        )
        .failure();
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr));
    assert_eq!(err["kind"], "validation");
    assert!(
        err["message"].as_str().unwrap().contains("`citation`"),
        "{err}"
    );
    assert_eq!(repo.status("F1"), "unverified");
    assert!(repo.verdicts().is_empty());
}

#[test]
fn same_as_finding_stays_unverified_until_struck() {
    let repo = Repo::new();
    repo.verdicted("F5", CONFIRMED);
    assert_eq!(
        repo.status("F5"),
        "unverified",
        "an identity claim waits for `review resolve`"
    );

    let mut struck = CONFIRMED.to_vec();
    struck.push("--strike-same-as");
    repo.verdicted("F5", &struck);
    let finding = repo.finding("F5");
    assert!(finding.get("same_as").is_none(), "{finding:?}");
    assert_eq!(finding["status"].as_str(), Some("open"));
    assert_eq!(repo.verdicts().len(), 1);
}
