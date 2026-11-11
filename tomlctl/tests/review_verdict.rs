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

[[findings]]
id = "F6"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "ripple"
route = "checker"
file = "docs/guide.md"
line = 3
summary = "the guide still names the removed flag"
status = "unverified"
"#;

const VOCABULARY: &str = "[routing]\nexecutable = [\"**/*.rs\"]\n";

/// The one lens the catalogue marks mechanical.
const RIPPLE_LENS: &str = "+++\nid = \"ripple\"\nstatus = \"stable\"\n\
    families = [\"code\", \"plan\"]\nmode = \"mechanical\"\nproduces = [\"cleanup\"]\n\
    owns = [\"stale-references\"]\nactivation = \"floor\"\nslice = \"whole-scope\"\n+++\n\n\
    ## Question\n\nWhat still names what the diff removed?\n";

struct Repo {
    _dir: tempfile::TempDir,
    _catalogue: Option<tempfile::TempDir>,
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        Self::seeded(STORE.to_string(), None)
    }

    /// The same store, its run recording a catalogue whose one lens,
    /// `ripple`, is mechanical.
    fn with_catalogue() -> Self {
        let catalogue = tempfile::tempdir().unwrap();
        let builtin = catalogue.path().join("review-v2-lenses");
        fs::create_dir_all(builtin.join("lenses")).unwrap();
        fs::write(builtin.join("vocabulary.toml"), VOCABULARY).unwrap();
        fs::write(builtin.join("lenses").join("ripple.md"), RIPPLE_LENS).unwrap();
        let builtin = builtin.to_string_lossy().replace('\\', "/");
        let started = "started = \"2026-10-12T10:00:00Z\"\n";
        let store = STORE.replacen(
            started,
            &format!("{started}catalogue = {{ builtin = \"{builtin}\" }}\n"),
            1,
        );
        Self::seeded(store, Some(catalogue))
    }

    fn seeded(store_text: String, catalogue: Option<tempfile::TempDir>) -> Self {
        let (dir, root) = sandbox();
        let store = root.join(".claude/review-v2/t.toml");
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(store, store_text).unwrap();
        Self {
            _dir: dir,
            _catalogue: catalogue,
            root,
        }
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
            .contains("records no catalogue"),
        "a run with no catalogue cannot say the lens is mechanical: {out}"
    );
    assert_eq!(repo.status("F2"), "unverified");
    assert_eq!(repo.status("F3"), "refuted");
    assert_eq!(repo.verdicts().len(), 1);
}

#[test]
fn checker_may_refute_a_mechanical_cleanup_from_a_finder_cell() {
    let repo = Repo::with_catalogue();
    let out = repo.batched(&[row("F6", "checker", "refuted")]);
    assert_eq!(
        out["verdicts"],
        json!([{"finding": "F6", "verdict": "refuted"}]),
        "{out}"
    );
    assert_eq!(out["refused"], json!([]), "{out}");
    assert_eq!(repo.status("F6"), "refuted");
}

#[test]
fn checker_refute_of_a_non_mechanical_finder_row_is_refused() {
    let repo = Repo::with_catalogue();
    let out = repo.batched(&[
        row("F2", "checker", "refuted"),
        row("F6", "checker", "refuted"),
    ]);
    assert_eq!(
        out["verdicts"],
        json!([{"finding": "F6", "verdict": "refuted"}]),
        "{out}"
    );
    assert_eq!(out["refused"][0]["finding"], "F2", "{out}");
    assert_eq!(
        out["refused"][0]["reasons"],
        json!([
            "a checker may refute only a row a sweeper wrote or a cleanup under a mechanical lens; \
             F2 is a cleanup under lens \"correctness\" from cell C1, which is not a review-sweeper cell"
        ]),
        "{out}"
    );
    assert_eq!(repo.status("F2"), "unverified");
}

#[test]
fn checker_refute_names_only_the_catalogue_failure_when_it_cannot_be_read() {
    let repo = Repo::with_catalogue();
    let drifted = repo.raw().replacen(
        "catalogue = { builtin = ",
        "catalogue = { tree_sha = \"0\", builtin = ",
        1,
    );
    fs::write(repo.root.join(".claude/review-v2/t.toml"), drifted).unwrap();
    let refused = repo.batch(&[row("F2", "checker", "refuted")]).failure();
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&refused.get_output().stderr));
    let message = err["message"].as_str().unwrap();
    let (_, line) = message.split_once('\n').expect("the refused row follows");
    let out: JsonValue = serde_json::from_str(line).expect("the refused row is JSON");
    assert_eq!(out["finding"], "F2", "{out}");
    assert_eq!(
        out["reasons"],
        json!([
            "run U1's catalogue, which says whether lens \"correctness\" is mechanical, could not be read: \
             run U1: the lens catalogue changed since the run started (tree_sha no longer matches); \
             start a new run with `review run start`"
        ]),
        "{out}"
    );
    assert_eq!(repo.status("F2"), "unverified");
}

#[test]
fn checker_refute_is_refused_when_an_unchecked_catalogue_root_is_gone() {
    let repo = Repo::with_catalogue();
    let builtin = repo
        ._catalogue
        .as_ref()
        .unwrap()
        .path()
        .join("review-v2-lenses");
    fs::remove_dir_all(builtin).unwrap();
    let refused = repo.batch(&[row("F2", "checker", "refuted")]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(
        stderr.contains("recorded built-in catalogue root") && stderr.contains("is gone"),
        "{stderr}"
    );
    assert_eq!(repo.status("F2"), "unverified");
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
    fs::write(repo.root.join(".claude/review-v2/t.toml"), text).unwrap();

    repo.batched(&[seat2("F4", "confirmed")]);
    assert_eq!(
        repo.status("F4"),
        "open",
        "seat 1 standing + seat 2 standing"
    );

    // Seat 1 refuted twice: two copies are one refuting seat.
    let copied = Repo::new();
    let mut text = copied.raw();
    text.push_str(&seeded("F4", 1, "refuted"));
    text.push_str(&seeded("F4", 1, "refuted"));
    fs::write(copied.root.join(".claude/review-v2/t.toml"), text).unwrap();

    copied.batched(&[seat2("F4", "refuted")]);
    assert_eq!(
        copied.status("F4"),
        "refuted",
        "seat 1 refuting + seat 2 refuting"
    );
    copied.batched(&[seat2("F4", "confirmed")]);
    assert_eq!(
        copied.status("F4"),
        "unverified",
        "one refuting seat copied twice is not two refuting seats"
    );
}

#[test]
fn panel_finding_takes_only_panel_and_audit_verdicts() {
    let repo = Repo::new();
    let mut seat1 = row("F4", "panel", "confirmed");
    seat1["seat"] = json!(1);
    repo.batched(&[seat1]);

    let refused = repo.batch(&[row("F4", "verifier", "confirmed")]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(stderr.contains("routed to the panel"), "{stderr}");
    assert_eq!(
        repo.status("F4"),
        "unverified",
        "a verifier cannot settle a panel finding past its quorum"
    );
    assert_eq!(repo.verdicts().len(), 1);

    repo.batched(&[row("F4", "audit", "refuted")]);
    assert_eq!(
        repo.status("F4"),
        "refuted",
        "the audit decides a panel finding at once"
    );

    let mut misrouted = row("F1", "panel", "confirmed");
    misrouted["seat"] = json!(1);
    let refused = repo.batch(&[misrouted]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(stderr.contains("not routed to the panel"), "{stderr}");
    assert_eq!(repo.status("F1"), "unverified");
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

/// Two blocked defects: one on evidence, one in the legacy needs-plan form.
const BLOCKED: &str = r#"
[[findings]]
id = "F7"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 40
symbol = "lib::cache"
summary = "the cache may serve stale rows"
severity = "major"
blocked_on = "evidence"
blocked_missing = "whether any caller reads after a write"
status = "unverified"

[[findings]]
id = "F8"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 50
symbol = "lib::flush"
summary = "flush drops the tail"
severity = "minor"
fix_shape = "needs-plan"
status = "unverified"
"#;

const SKETCH: &str = "invalidate the entry in lib::write before returning";

fn blocked_repo() -> Repo {
    Repo::seeded(format!("{STORE}{BLOCKED}"), None)
}

fn unblocking(finding: &str) -> JsonValue {
    let mut row = row(finding, "verifier", "confirmed");
    row["unblocks"] = json!(true);
    row["fix_sketch"] = json!(SKETCH);
    row
}

#[test]
fn unblocking_verdict_clears_the_block_and_keeps_the_sketch() {
    let repo = blocked_repo();
    let mut args = CONFIRMED.to_vec();
    args.extend(["--unblocks", "--fix-sketch", SKETCH]);
    let out = repo.verdicted("F7", &args);
    assert_eq!(
        out["verdicts"],
        json!([{"finding": "F7", "verdict": "confirmed", "unblocks": true, "fix_sketch": SKETCH}]),
        "{out}"
    );
    let finding = repo.finding("F7");
    assert!(finding.get("blocked_on").is_none(), "{finding:?}");
    assert!(finding.get("blocked_missing").is_none(), "{finding:?}");
    assert_eq!(finding["fix_sketch"].as_str(), Some(SKETCH));
    assert_eq!(finding["unblocked_by"].as_str(), Some("verifier"));
    assert_eq!(finding["unblocked_from"].as_str(), Some("evidence"));
    assert_eq!(finding["status"].as_str(), Some("open"));
    let verdicts = repo.verdicts();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0]["unblocks"].as_bool(), Some(true));
    assert_eq!(verdicts[0]["fix_sketch"].as_str(), Some(SKETCH));

    let after = repo.raw();
    let replay = repo.verdicted("F7", &args);
    assert_eq!(replay["unchanged"], true, "{replay}");
    assert_eq!(repo.raw(), after, "a replayed unblock writes nothing");
}

#[test]
fn unblocking_clears_a_legacy_needs_plan_block() {
    let repo = blocked_repo();
    repo.batched(&[unblocking("F8")]);
    let finding = repo.finding("F8");
    assert!(finding.get("fix_shape").is_none(), "{finding:?}");
    assert_eq!(finding["fix_sketch"].as_str(), Some(SKETCH));
    assert_eq!(finding["unblocked_by"].as_str(), Some("verifier"));
    assert_eq!(finding["unblocked_from"].as_str(), Some("design"));
}

#[test]
fn unblock_without_fix_sketch_is_refused() {
    let repo = blocked_repo();
    let mut bare = unblocking("F7");
    bare.as_object_mut().unwrap().remove("fix_sketch");
    let refused = repo.batch(&[bare]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(stderr.contains("fix_sketch"), "{stderr}");
    assert_eq!(repo.finding("F7")["blocked_on"].as_str(), Some("evidence"));
    assert!(repo.verdicts().is_empty());
}

#[test]
fn unblock_of_a_finding_that_is_not_blocked_is_refused() {
    let repo = blocked_repo();
    let refused = repo.batch(&[unblocking("F1")]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(stderr.contains("F1 is not blocked"), "{stderr}");
    assert!(repo.finding("F1").get("fix_sketch").is_none());
    assert!(repo.verdicts().is_empty());
}

#[test]
fn unblock_from_a_refuting_or_non_verifier_verdict_is_refused() {
    let repo = blocked_repo();
    let mut refuting = unblocking("F7");
    refuting["verdict"] = json!("refuted");
    let mut audit = unblocking("F7");
    audit["by"] = json!("audit");
    for row in [refuting, audit] {
        repo.batch(&[row]).failure();
    }
    assert_eq!(repo.finding("F7")["blocked_on"].as_str(), Some("evidence"));
    assert!(repo.verdicts().is_empty());
}

#[test]
fn verdict_without_unblocks_leaves_the_block() {
    let repo = blocked_repo();
    let mut sketch_only = row("F7", "verifier", "confirmed");
    sketch_only["fix_sketch"] = json!(SKETCH);
    let refused = repo.batch(&[sketch_only]).failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(stderr.contains("only with unblocks"), "{stderr}");

    repo.verdicted("F7", CONFIRMED);
    let finding = repo.finding("F7");
    assert_eq!(finding["blocked_on"].as_str(), Some("evidence"));
    assert!(finding.get("fix_sketch").is_none(), "{finding:?}");
    assert!(finding.get("unblocked_by").is_none(), "{finding:?}");
    assert_eq!(finding["status"].as_str(), Some("open"));
}

#[test]
fn rationale_and_sketch_both_from_stdin_are_refused() {
    let repo = blocked_repo();
    let out = repo
        .verdict(
            "F7",
            &[
                "--verdict",
                "confirmed",
                "--by",
                "verifier",
                "--citation",
                "src/a.rs:1",
                "--rationale",
                "-",
                "--unblocks",
                "--fix-sketch",
                "-",
            ],
        )
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(stderr.contains("both read stdin"), "{stderr}");
    assert!(repo.verdicts().is_empty());
}

const PANEL_BLOCKED: &str = r#"
[[findings]]
id = "F9"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "panel"
file = "src/lib.rs"
line = 60
symbol = "lib::sync"
summary = "sync may reorder writes"
severity = "major"
blocked_on = "evidence"
blocked_missing = "whether two writers can interleave"
status = "unverified"

[[findings]]
id = "F10"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
route = "panel"
file = "src/lib.rs"
line = 70
symbol = "lib::drain"
summary = "drain skips the last row"
severity = "major"
status = "unverified"
"#;

#[test]
fn orchestrator_verdict_on_a_panel_finding_is_refused_unless_it_refutes_a_blocked_one() {
    let repo = Repo::seeded(format!("{STORE}{PANEL_BLOCKED}"), None);
    for (finding, verdict) in [("F9", "confirmed"), ("F10", "refuted")] {
        let refused = repo
            .batch(&[row(finding, "orchestrator", verdict)])
            .failure();
        let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
        assert!(stderr.contains("is routed to the panel"), "{stderr}");
        assert_eq!(repo.status(finding), "unverified");
    }
    assert!(repo.verdicts().is_empty());
}
