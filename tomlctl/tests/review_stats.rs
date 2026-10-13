//! `tomlctl review stats` against a sandbox store with two runs, verifier and
//! audit verdicts, probe cells, and v1 rows linked back through `v2_ref`.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{cli, sandbox};
use serde_json::{Value as JsonValue, json};

const STORE_PATH: &str = ".claude/review-v2/t.toml";
const FLOW_STORE_PATH: &str = ".claude/flows/f1/review-v2.toml";
const REVIEW_LEDGER: &str = ".claude/reviews/t.toml";

const STORE: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-12T10:00:00Z"
waivers = [{ concern = "security", reason = "no input handling in scope" }]

[[runs]]
id = "U2"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-13T10:00:00Z"

[[cells]]
id = "C1"
run = "U1"
slice = ["src/lib.rs"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
bound = [{ concern = "correctness", hash = "h1", path = "lenses/correctness.md" }]
status = "covered"
sealed = true
note = "covered"

[[cells]]
id = "C2"
run = "U1"
slice = ["src"]
concerns = ["hygiene"]
agent = "review-sweeper"
reason = "proposed"
bound = [{ concern = "hygiene", hash = "g1", path = "lenses/hygiene.md" }]
status = "covered"
sealed = true
note = "covered"

[[cells]]
id = "C3"
run = "U1"
slice = ["src/net.rs"]
concerns = ["probe:retry-storm"]
question = "Can a retry storm starve the pool?"
agent = "review-finder"
reason = "nearest correctness: no lens owns retry behaviour"
status = "covered"
sealed = true
note = "covered"

[[cells]]
id = "C4"
run = "U2"
slice = ["src/lib.rs", "src/io.rs"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
bound = [{ concern = "correctness", hash = "h2", path = "lenses/correctness.md" }]
status = "thin"
sealed = true
note = "ran out of budget on io.rs"

[[cells]]
id = "C5"
run = "U2"
slice = ["src/net.rs"]
concerns = ["probe:lock-order"]
question = "Is the lock order consistent?"
agent = "review-finder"
reason = "nearest correctness: lock ordering spans two modules"
status = "covered"
sealed = true
note = "covered"

[[cells]]
id = "C6"
run = "U2"
slice = ["src/odd.rs"]
concerns = ["probe:odd-path"]
question = "Does the odd path ever run?"
agent = "review-finder"
reason = "checks the odd path"
status = "uncovered"
sealed = true
note = "could not reach it"

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 10
summary = "off by one"
status = "open"

[[findings]]
id = "F2"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 20
summary = "unchecked index"
status = "refuted"

[[findings]]
id = "F3"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 30
summary = "missing guard"
status = "refuted"

[[findings]]
id = "F4"
run = "U1"
cell = "C2"
family = "code"
kind = "cleanup"
lens = "hygiene"
file = "src/util.rs"
line = 4
summary = "unused import"
status = "dismissed"
dismiss_reason = "Style only. The user prefers it this way."

[[findings]]
id = "F5"
run = "U2"
cell = "C4"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 40
summary = "lost write on retry"
status = "open"

[[findings]]
id = "F6"
run = "U2"
cell = "C4"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/io.rs"
line = 5
summary = "lock released early"
status = "open"

[[findings]]
id = "F7"
run = "U2"
cell = "C4"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 11
summary = "off by one"
same_as = "F1"
outcome = "repeat"
status = "merged"
merged_into = "F1"

[[findings]]
id = "F8"
run = "U2"
cell = "C5"
family = "code"
kind = "advisory"
lens = "probe:lock-order"
file = "src/net.rs"
summary = "lock order differs across modules"
status = "open"

[[findings]]
id = "F9"
run = "U1"
cell = "C3"
family = "code"
kind = "cleanup"
lens = "probe:retry-storm"
file = "src/net.rs"
line = 7
summary = "missing backoff test"
status = "open"

[[findings]]
id = "F10"
run = "U2"
cell = "C4"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 50
summary = "overflow on add"
status = "refuted"

[[findings]]
id = "F11"
run = "U2"
cell = "C6"
family = "code"
kind = "advisory"
lens = "probe:odd-path"
file = "src/odd.rs"
summary = "odd path is dead"
status = "open"

[[findings]]
id = "F12"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 10
summary = "an earlier submission"
status = "superseded"

[[findings]]
id = "F13"
run = "U1"
cell = "C2"
family = "code"
kind = "cleanup"
lens = "hygiene"
file = "src/util.rs"
line = 9
summary = "dead helper"
status = "exported"
exported_as = "R1"
export_ledger = ".claude/reviews/t.toml"

[[findings]]
id = "F14"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 60
summary = "wrong default"
status = "exported"
exported_as = "R2"
export_ledger = ".claude/reviews/t.toml"

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"
verdict = "confirmed"
citation = "src/lib.rs:10"
rationale = "Reproduced with an empty slice."

[[verdicts]]
finding = "F2"
run = "U1"
by = "verifier"
verdict = "refuted"
citation = "src/lib.rs:18"
rationale = "Verifier sees a bounds check. It runs first."

[[verdicts]]
finding = "F3"
run = "U1"
by = "verifier"
verdict = "confirmed"
citation = "src/lib.rs:30"
rationale = "No guard on the path."

[[verdicts]]
finding = "F3"
run = "U1"
by = "audit"
verdict = "refuted"
citation = "src/caller.rs:12"
rationale = "Audit: the guard holds upstream in the caller. See caller.rs."

[[verdicts]]
finding = "F4"
run = "U1"
by = "checker"
verdict = "confirmed"
citation = "src/util.rs:4"
rationale = "The import has no use."

[[verdicts]]
finding = "F5"
run = "U2"
by = "verifier"
verdict = "confirmed"
citation = "src/lib.rs:40"
rationale = "Reproduced."

[[verdicts]]
finding = "F6"
run = "U2"
by = "verifier"
verdict = "plausible"
citation = "src/io.rs:5"
rationale = "Likely."

[[verdicts]]
finding = "F7"
run = "U2"
by = "verifier"
verdict = "confirmed"
citation = "src/lib.rs:11"
rationale = "Same as F1."

[[verdicts]]
finding = "F9"
run = "U1"
by = "checker"
verdict = "confirmed"
citation = "src/net.rs:7"
rationale = "No test names backoff."

[[verdicts]]
finding = "F10"
run = "U2"
by = "verifier"
verdict = "refuted"
citation = "src/lib.rs:48"
rationale = "Checked arithmetic is used."

[[verdicts]]
finding = "F10"
run = "U2"
by = "audit"
verdict = "refuted"
citation = "src/lib.rs:48"
rationale = "Audit agrees with the verifier. checked_add is used."
"#;

const REVIEW: &str = r#"schema_version = 1
last_updated = 2026-10-12

[[items]]
id = "R1"
file = "src/util.rs"
line = 9
severity = "suggestion"
effort = "trivial"
category = "quality"
summary = "dead helper"
first_flagged = 2026-10-12
rounds = 1
status = "wontfix"
wontfix_rationale = "Kept for the plugin ABI. Revisit in 2027."
v2_ref = ".claude/review-v2/t.toml#F13"

[[items]]
id = "R2"
file = "src/lib.rs"
line = 60
severity = "warning"
effort = "small"
category = "quality"
summary = "wrong default"
first_flagged = 2026-10-12
rounds = 1
status = "fixed"
resolved = 2026-10-14
resolution = "abc123 set the default"
v2_ref = ".claude/review-v2/t.toml#F14"
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
        repo.write(REVIEW_LEDGER, REVIEW);
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

    fn stats(&self, extra: &[&str]) -> JsonValue {
        let mut args = vec!["--error-format", "json", "review", "stats"];
        args.extend_from_slice(extra);
        let out = cli(&self.root)
            .current_dir(&self.root)
            .args(&args)
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("stats prints JSON")
    }
}

fn row<'a>(rows: &'a JsonValue, key: &str) -> &'a JsonValue {
    rows.as_array()
        .expect("rows is an array")
        .iter()
        .find(|r| r["key"] == key)
        .unwrap_or_else(|| panic!("no row {key}: {rows}"))
}

#[test]
fn stats_refute_and_audit_overturn_rates_per_lens() {
    let repo = Repo::new();
    let out = repo.stats(&["--scope", "t", "--by", "lens"]);
    assert_eq!(out["by"], "lens", "{out}");
    let rows = &out["rows"];

    let correctness = row(rows, "correctness");
    assert_eq!(
        correctness["hashes"],
        json!(["h1", "h2"]),
        "both bound versions are one row with the hashes as a column: {correctness}"
    );
    assert_eq!(correctness["runs"], 2, "{correctness}");
    assert_eq!(correctness["cells"], 2, "{correctness}");
    assert_eq!(
        correctness["findings"],
        json!({"defect": 8, "cleanup": 0, "advisory": 0}),
        "the superseded row is not counted: {correctness}"
    );
    assert_eq!(correctness["verified"], 7, "{correctness}");
    assert_eq!(correctness["refuted"], 2, "{correctness}");
    assert_eq!(correctness["refute_rate"], 0.286, "{correctness}");
    assert_eq!(correctness["audited"], 2, "{correctness}");
    assert_eq!(correctness["overturned"], 1, "{correctness}");
    assert_eq!(correctness["audit_overturn_rate"], 0.5, "{correctness}");
    assert_eq!(correctness["thin"], 1, "{correctness}");

    let hygiene = row(rows, "hygiene");
    assert_eq!(hygiene["refute_rate"], 0.0, "{hygiene}");
    assert_eq!(hygiene["audit_overturn_rate"], JsonValue::Null, "{hygiene}");
    assert_eq!(hygiene["dismissed"], 1, "{hygiene}");
    assert_eq!(hygiene["dismissal_rate"], 0.5, "{hygiene}");

    let security = row(rows, "security");
    assert_eq!(security["waived"], 1, "{security}");
    assert_eq!(security["waiver_rate"], 1.0, "{security}");

    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| !r["key"].as_str().unwrap().starts_with("probe:")),
        "probes group under --by probe, not as lenses: {rows}"
    );
    assert_eq!(repo.read(STORE_PATH), STORE, "stats writes nothing");
}

#[test]
fn panel_finding_is_verified_only_by_its_quorum() {
    let repo = Repo::new();
    let vote = |by: &str, seat: &str, verdict: &str| {
        format!(
            "\n[[verdicts]]\nfinding = \"F15\"\nrun = \"U2\"\nby = \"{by}\"\n{seat}verdict = \"{verdict}\"\ncitation = \"src/lib.rs:70\"\nrationale = \"Read the anchor.\"\n"
        )
    };
    let mut text = STORE.to_string();
    text.push_str(
        "\n[[findings]]\nid = \"F15\"\nrun = \"U2\"\ncell = \"C4\"\nfamily = \"code\"\nkind = \"defect\"\nlens = \"correctness\"\nroute = \"panel\"\nfile = \"src/lib.rs\"\nline = 70\nsummary = \"unlocked open\"\nseverity = \"critical\"\nstatus = \"unverified\"\n",
    );
    text.push_str(&vote("verifier", "", "refuted"));
    repo.write(STORE_PATH, &text);

    let verified = |repo: &Repo| {
        let out = repo.stats(&["--scope", "t", "--by", "lens"]);
        row(&out["rows"], "correctness")["verified"].clone()
    };
    assert_eq!(
        verified(&repo),
        7,
        "a verifier verdict on a panel finding is not its call"
    );

    text.push_str(&vote("panel", "seat = 1\n", "confirmed"));
    repo.write(STORE_PATH, &text);
    assert_eq!(verified(&repo), 7, "one seat decides nothing");

    text.push_str(&vote("panel", "seat = 2\n", "plausible"));
    repo.write(STORE_PATH, &text);
    assert_eq!(verified(&repo), 8, "two standing seats settle it");
}

#[test]
fn stats_outcomes_join_v1_status() {
    let repo = Repo::new();
    let out = repo.stats(&["--scope", "t", "--outcomes"]);
    assert!(
        out.get("rows").is_none(),
        "a section alone prints no rows: {out}"
    );
    let outcomes = out["outcomes"].as_array().expect("outcomes is an array");
    let group = |lens: &str, kind: &str| {
        outcomes
            .iter()
            .find(|g| g["lens"] == lens && g["kind"] == kind)
            .unwrap_or_else(|| panic!("no outcome group {lens}/{kind}: {out}"))
    };

    let fixed = group("correctness", "defect");
    assert_eq!(fixed["exported"], 1, "{fixed}");
    assert_eq!(fixed["v1"], json!({"fixed": 1}), "{fixed}");
    assert_eq!(fixed["v2"], json!({"resolved": 1}), "{fixed}");

    let wontfix = group("hygiene", "cleanup");
    assert_eq!(wontfix["v1"], json!({"wontfix": 1}), "{wontfix}");
    assert_eq!(wontfix["v2"], json!({"dismissed": 1}), "{wontfix}");
    assert_eq!(
        wontfix["wontfix_reasons"][0]["reason"], "Kept for the plugin ABI. Revisit in 2027.",
        "{wontfix}"
    );
    assert_eq!(
        wontfix["wontfix_reasons"][0]["v1"], ".claude/reviews/t.toml#R1",
        "{wontfix}"
    );

    assert_eq!(repo.read(STORE_PATH), STORE, "the store is only read");
    assert_eq!(
        repo.read(REVIEW_LEDGER),
        REVIEW,
        "the v1 ledger is only read"
    );
    assert!(
        !repo.root.join(format!("{STORE_PATH}.sha256")).exists(),
        "no sidecar is written"
    );
}

#[test]
fn outcomes_skip_a_retired_link() {
    let repo = Repo::new();
    let reexported = STORE.replace(
        "summary = \"wrong default\"\nstatus = \"exported\"\nexported_as = \"R2\"\n",
        "summary = \"wrong default\"\nstatus = \"exported\"\nexported_as = \"R3\"\nretired_links = [\".claude/reviews/t.toml#R2\"]\n",
    );
    assert_ne!(reexported, STORE, "the F14 fixture moved");
    repo.write(STORE_PATH, &reexported);
    let mut ledger = REVIEW.to_string();
    ledger.push_str(
        "\n[[items]]\nid = \"R3\"\nfile = \"src/lib.rs\"\nline = 60\nseverity = \"warning\"\neffort = \"small\"\ncategory = \"quality\"\nsummary = \"wrong default\"\nfirst_flagged = 2026-10-15\nrounds = 1\nstatus = \"open\"\nrelated = [\"R2\"]\nv2_ref = \".claude/review-v2/t.toml#F14\"\n",
    );
    repo.write(REVIEW_LEDGER, &ledger);

    let out = repo.stats(&["--scope", "t", "--outcomes"]);
    let group = out["outcomes"]
        .as_array()
        .expect("outcomes is an array")
        .iter()
        .find(|g| g["lens"] == "correctness" && g["kind"] == "defect")
        .unwrap_or_else(|| panic!("no correctness/defect group: {out}"))
        .clone();
    assert_eq!(group["exported"], 1, "{group}");
    assert_eq!(
        group["linked"], 1,
        "the retired R2 still names F14 but is not its link: {group}"
    );
    assert_eq!(group["v1"], json!({"open": 1}), "{group}");
    assert_eq!(group["v2"], json!({"open": 1}), "{group}");
}

#[test]
fn stats_escapes_count_refinds_in_covered_cells() {
    let repo = Repo::new();
    let out = repo.stats(&["--scope", "t", "--escapes"]);
    let escapes = &out["escapes"];
    assert_eq!(
        escapes["count"], 1,
        "only F5 lands in a file and concern U1 sealed covered: {escapes}"
    );
    let found = &escapes["findings"][0];
    assert_eq!(found["finding"], "F5", "{escapes}");
    assert_eq!(found["concern"], "correctness", "{escapes}");
    assert_eq!(found["covered_by"], json!({"run": "U1", "cell": "C1"}));
    assert_eq!(
        escapes["by_concern"],
        json!([{"concern": "correctness", "escapes": 1}])
    );
}

#[test]
fn calibration_ignores_unaudited_refutations() {
    let repo = Repo::new();
    let out = repo.stats(&["--scope", "t", "--calibration", "--exemplars", "3"]);
    let areas = out["calibration"]
        .as_array()
        .expect("calibration is an array");
    let texts = |area: &str| -> Vec<String> {
        areas
            .iter()
            .find(|a| a["area"] == area)
            .map(|a| {
                a["exemplars"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e["text"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };

    let correctness = texts("correctness");
    assert_eq!(
        correctness,
        vec!["Audit: the guard holds upstream in the caller.".to_string()],
        "only the audit overturn teaches; a verifier refutation and an audit that agrees do not: {out}"
    );
    let all: Vec<String> = areas
        .iter()
        .flat_map(|a| a["exemplars"].as_array().unwrap().clone())
        .map(|e| e["text"].as_str().unwrap().to_string())
        .collect();
    assert!(
        all.iter()
            .all(|t| !t.starts_with("Verifier") && !t.starts_with("Checked")),
        "raw verifier rationales never reach calibration: {all:?}"
    );

    let hygiene = texts("hygiene");
    assert!(
        hygiene.contains(&"Style only.".to_string()),
        "a user dismissal is an exemplar, first sentence only: {out}"
    );
    assert!(
        hygiene.contains(&"Kept for the plugin ABI.".to_string()),
        "a v1 wontfix reason is an exemplar: {out}"
    );
}

#[test]
fn stats_probe_groups_by_concern_tag() {
    let repo = Repo::new();
    let out = repo.stats(&["--scope", "t", "--by", "probe"]);
    let rows = &out["rows"];
    assert_eq!(rows.as_array().unwrap().len(), 2, "{rows}");

    let tagged = row(rows, "correctness");
    assert_eq!(tagged["runs"], 2, "the tag recurs across runs: {tagged}");
    assert_eq!(tagged["cells"], 2, "{tagged}");
    assert_eq!(
        tagged["probes"],
        json!(["probe:lock-order", "probe:retry-storm"])
    );
    assert_eq!(
        tagged["findings"],
        json!({"defect": 0, "cleanup": 1, "advisory": 1})
    );

    let untagged = row(rows, "untagged");
    assert_eq!(untagged["probes"], json!(["probe:odd-path"]), "{untagged}");
    assert_eq!(untagged["uncovered"], 1, "{untagged}");
}

#[test]
fn stats_all_aggregates_across_stores() {
    let repo = Repo::new();
    repo.write(FLOW_STORE_PATH, STORE);
    repo.write(".claude/review-v2/notes.txt", "not a store");
    let out = repo.stats(&["--all"]);
    assert_eq!(
        out["stores"],
        json!([FLOW_STORE_PATH, STORE_PATH]),
        "flow stores then flow-less ones: {out}"
    );
    let correctness = row(&out["rows"], "correctness");
    assert_eq!(
        correctness["runs"], 4,
        "runs are keyed per store: {correctness}"
    );
    assert_eq!(correctness["findings"]["defect"], 16, "{correctness}");

    let by_run = repo.stats(&["--all", "--by", "run"]);
    let runs = by_run["rows"].as_array().unwrap();
    assert_eq!(runs.len(), 4, "{by_run}");
    assert!(
        runs.iter()
            .any(|r| r["store"] == FLOW_STORE_PATH && r["key"] == "U2"),
        "{by_run}"
    );

    let perf = repo.stats(&["--all", "--family", "perf"]);
    assert_eq!(perf["rows"], json!([]), "no perf runs: {perf}");
}
