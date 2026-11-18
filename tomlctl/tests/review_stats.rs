//! `tomlctl review stats` against a sandbox store with two runs, verifier and
//! audit verdicts, probe cells, and v1 rows linked back through `v2_ref`.

use crate::common;

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
        let out = cli(&self.root).args(&args).assert().success();
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
fn stats_by_run_reports_waves_from_cells() {
    let repo = Repo::new();
    let text = STORE
        .replacen(
            "id = \"C1\"\nrun = \"U1\"\n",
            "id = \"C1\"\nrun = \"U1\"\nwave = 1\n",
            1,
        )
        .replacen(
            "id = \"C2\"\nrun = \"U1\"\n",
            "id = \"C2\"\nrun = \"U1\"\nwave = 2\n",
            1,
        );
    assert_ne!(text, STORE, "the fixture moved");
    repo.write(STORE_PATH, &text);
    let out = repo.stats(&["--scope", "t", "--by", "run"]);
    assert_eq!(row(&out["rows"], "U1")["waves"], 2, "{out}");
    assert_eq!(row(&out["rows"], "U2")["waves"], 0, "{out}");
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

const PLAN_STORE: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-plan-v2"
family = "plan"
depth = "standard"
started = "2026-11-09T10:00:00Z"
finished = "2026-11-09T10:30:00Z"
models = { verifier = "opus", checker = "haiku" }
agents = 10

[[runs]]
id = "U2"
command = "review-plan-v2"
family = "plan"
depth = "quick"
started = "2026-11-09T11:00:00Z"
models = { verifier = "sonnet" }
rechecks = "U1"

[[findings]]
id = "F1"
run = "U1"
family = "plan"
kind = "defect"
lens = "feasibility"
route = "verifier"
tier = "mechanical"
status = "resolved"
applied = { by = "tool", old = '["a.rs"]', new = '["b.rs", "c.rs"]' }

[[findings]]
id = "F2"
run = "U1"
family = "plan"
kind = "cleanup"
lens = "feasibility"
route = "checker"
tier = "cited"
status = "resolved"
applied = { by = "tool", old = '"ab"', new = '"abcd"' }

[[findings]]
id = "F3"
run = "U1"
family = "plan"
kind = "defect"
lens = "intent"
route = "verifier"
tier = "user"
status = "resolved"
applied = { by = "user", old = "null", new = '"x"' }

[[findings]]
id = "F4"
run = "U1"
family = "plan"
kind = "defect"
lens = "intent"
route = "verifier"
tier = "user"
status = "dismissed"
dismiss_reason = "keep it"

[[findings]]
id = "F5"
run = "U1"
family = "plan"
kind = "advisory"
lens = "intent"
route = "none"
tier = "advisory"
status = "parked"
park_reason = "later"

[[findings]]
id = "F6"
run = "U2"
family = "plan"
kind = "defect"
lens = "feasibility"
route = "verifier"
status = "open"

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"
verdict = "confirmed"

[[verdicts]]
finding = "F1"
run = "U2"
by = "audit"
verdict = "refuted"

[[verdicts]]
finding = "F2"
run = "U1"
by = "checker"
verdict = "confirmed"

[[verdicts]]
finding = "F2"
run = "U2"
by = "audit"
verdict = "confirmed"

[[verdicts]]
finding = "F6"
run = "U2"
by = "verifier"
verdict = "refuted"
"#;

#[test]
fn plan_outcomes_count_tiers_and_dispositions() {
    let repo = Repo::new();
    repo.write(FLOW_STORE_PATH, PLAN_STORE);
    let out = repo.stats(&["--slug", "f1", "--plan-outcomes"]);
    let runs = out["plan_outcomes"]["runs"]
        .as_array()
        .expect("runs is an array");
    assert_eq!(runs.len(), 2, "{out}");

    let first = &runs[0];
    assert_eq!(first["run"], "U1", "{first}");
    assert_eq!(
        first["tiers"],
        json!({
            "advisory": {"parked": 1},
            "cited": {"applied-disclosed": 1},
            "mechanical": {"applied": 1},
            "user": {"user-accepted": 1, "user-vetoed": 1},
        }),
        "{first}"
    );
    assert_eq!(first["veto_rate"], 0.5, "{first}");
    assert_eq!(
        first["growth"],
        json!({"edits": 3, "chars_added": 7, "entries_added": 3, "entries_removed": 1}),
        "{first}"
    );
    assert_eq!(first["recheck_yield"], JsonValue::Null, "{first}");
    assert_eq!(
        first["cost"],
        json!({"agents": 10, "minutes": 30.0}),
        "{first}"
    );

    let second = &runs[1];
    assert_eq!(second["rechecks"], "U1", "{second}");
    assert_eq!(second["recheck_yield"], 1, "{second}");
    assert_eq!(second["tiers"], json!({"none": {"open": 1}}), "{second}");

    let total = &out["plan_outcomes"]["total"];
    assert_eq!(total["runs"], 2, "{total}");
    assert_eq!(total["growth"]["edits"], 3, "{total}");
}

const APPLY_STORE_PATH: &str = ".claude/review-v2/applied.toml";

const APPLY_STORE: &str = r#"schema_version = 2

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-11-01T10:00:00Z"

[[findings]]
id = "F1"
run = "U1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 10
summary = "off by one"
status = "scheduled"
apply_flow = "fix-apply"
apply_task = "fix-off-by-one"

[[findings]]
id = "F2"
run = "U1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 20
summary = "unchecked index"
status = "resolved"
resolution = "abc123 checked the index"
resolved = 2026-11-03
apply_flow = "fix-apply"
apply_task = "check-the-index"

[[findings]]
id = "F3"
run = "U1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 30
summary = "missing guard"
status = "resolved"
resolution = "fixed by hand"
resolved = 2026-11-03

[[findings]]
id = "F4"
run = "U1"
family = "code"
kind = "cleanup"
lens = "hygiene"
file = "src/io.rs"
line = 5
summary = "dead branch"
status = "open"
reopen_reason = "apply: task deferred"

[[findings]]
id = "F5"
run = "U1"
family = "code"
kind = "cleanup"
lens = "hygiene"
file = "src/io.rs"
line = 9
summary = "stale comment"
status = "open"
reopen_reason = "the user changed their mind"

[[findings]]
id = "F6"
run = "U1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 40
summary = "replaced by a resubmission"
status = "superseded"

[[reproductions]]
finding = "F2"
flow = "fix-apply"
milestone = "A"
round = 1
task = "check-the-index"
verdict = "reproduces"
by = "verifier"
citation = "src/lib.rs:20"

[[reproductions]]
finding = "F2"
flow = "fix-apply"
milestone = "A"
round = 2
task = "check-the-index"
verdict = "gone"
by = "verifier"
citation = "src/lib.rs:21"

[[reproductions]]
finding = "F4"
flow = "fix-apply"
milestone = "A"
round = 1
task = "drop-the-branch"
verdict = "unsettled"
by = "orchestrator"
citation = "src/io.rs:5"

[[reproductions]]
finding = "F6"
flow = "fix-apply"
milestone = "A"
round = 1
task = "replaced"
verdict = "reproduces"
by = "verifier"
citation = "src/lib.rs:40"
"#;

#[test]
fn stats_apply_counts_outcomes_and_reproductions_from_the_store() {
    let repo = Repo::new();
    repo.write(APPLY_STORE_PATH, APPLY_STORE);
    let out = repo.stats(&["--scope", "applied", "--apply"]);
    assert!(
        out.get("rows").is_none(),
        "a section alone prints no rows: {out}"
    );
    assert!(out.get("outcomes").is_none(), "{out}");
    let apply = &out["apply"];
    assert_eq!(
        apply["outcomes"],
        json!([
            {
                "lens": "correctness",
                "kind": "defect",
                "scheduled": 1,
                "resolved": 1,
                "reopened": 0,
                "reproductions": {"gone": 1, "reproduces": 0, "unsettled": 0},
            },
            {
                "lens": "hygiene",
                "kind": "cleanup",
                "scheduled": 0,
                "resolved": 0,
                "reopened": 1,
                "reproductions": {"gone": 0, "reproduces": 0, "unsettled": 1},
            },
        ]),
        "a resolve without an apply link, a reopen not made by apply and a superseded row are not counted: {apply}"
    );
    assert_eq!(
        apply["reproductions"],
        json!({
            "rows": {"gone": 1, "reproduces": 1, "unsettled": 1},
            "findings": {"gone": 1, "reproduces": 0, "unsettled": 1},
        }),
        "every row counts once, and each finding by its last row: {apply}"
    );
    assert_eq!(
        repo.read(APPLY_STORE_PATH),
        APPLY_STORE,
        "the store is only read"
    );
}

const BLOCKED_STORE_PATH: &str = ".claude/review-v2/blocked.toml";

const BLOCKED_STORE: &str = r#"schema_version = 2

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-11-01T10:00:00Z"

[[cells]]
id = "C1"
run = "U1"
slice = ["src/lib.rs"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
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
status = "covered"
sealed = true
note = "covered"

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 10
summary = "which owner frees the buffer"
status = "open"
blocked_on = "decision"
blocked_missing = "who owns the buffer"
blocked_options = ["the reader", "the pool"]

[[findings]]
id = "F2"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 20
summary = "the retry may never stop"
status = "open"
fix_sketch = "cap the retries at three"
unblocked_by = "verifier"

[[findings]]
id = "F3"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 30
summary = "the cache may be stale"
status = "open"
fix_sketch = "invalidate on write"
unblocked_by = "settle"
settle_tier = "mechanical"

[[findings]]
id = "F4"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 40
summary = "the lock may be held across await"
status = "refuted"
blocked_on = "evidence"
blocked_missing = "whether the guard outlives the await"

[[findings]]
id = "F5"
run = "U1"
cell = "C2"
family = "code"
kind = "cleanup"
lens = "hygiene"
file = "src/io.rs"
line = 5
summary = "the helper may be dead"
status = "open"
blocked_on = "evidence"
blocked_missing = "whether any caller remains"
proposal = "docs/flows/f/notes/proposal-f5.md"
proposal_at = 2026-11-02T10:00:00Z

[[findings]]
id = "F6"
run = "U1"
cell = "C2"
family = "code"
kind = "cleanup"
lens = "hygiene"
file = "src/io.rs"
line = 9
summary = "stale comment"
status = "open"

[[findings]]
id = "F7"
run = "U1"
cell = "C2"
family = "code"
kind = "cleanup"
lens = "hygiene"
file = "src/io.rs"
line = 12
summary = "which name the flag takes"
status = "open"
decision_answer = "the short one"
blocked_options = ["the short one", "the long one"]
unblocked_by = "user"

[[verdicts]]
finding = "F4"
run = "U1"
by = "orchestrator"
verdict = "refuted"
citation = "src/lib.rs:41"
rationale = "its settle proposal refuted the finding"
"#;

#[test]
fn stats_apply_reports_blocked_rates_and_settle_outcomes() {
    let repo = Repo::new();
    repo.write(BLOCKED_STORE_PATH, BLOCKED_STORE);
    let out = repo.stats(&["--scope", "blocked", "--apply"]);
    let apply = &out["apply"];
    let blocked_on = |decision: usize, evidence: usize, unrecorded: usize| {
        json!({
            "decision": decision,
            "evidence": evidence,
            "measurement": 0,
            "alternatives": 0,
            "design": 0,
            "unrecorded": unrecorded,
        })
    };
    let settle = |mechanical: usize, refuted: usize, deferred: usize| {
        json!({
            "mechanical": mechanical,
            "cited": 0,
            "user": 0,
            "refuted": refuted,
            "deferred": deferred,
        })
    };

    let correctness = row(&apply["blocked"]["lens"], "correctness");
    assert_eq!(correctness["findings"], 4, "{correctness}");
    assert_eq!(
        correctness["blocked"], 4,
        "an answered block still counts as blocked: {correctness}"
    );
    assert_eq!(
        correctness["blocked_on"],
        blocked_on(1, 2, 1),
        "a settled block reads as evidence, a verifier-cleared one as unrecorded: {correctness}"
    );
    assert_eq!(correctness["verifier_unblocked"], 1, "{correctness}");
    assert_eq!(correctness["verifier_unblock_rate"], 0.25, "{correctness}");
    assert_eq!(correctness["settle"], settle(1, 1, 0), "{correctness}");

    let hygiene = row(&apply["blocked"]["lens"], "hygiene");
    assert_eq!(hygiene["findings"], 3, "{hygiene}");
    assert_eq!(hygiene["blocked"], 2, "{hygiene}");
    assert_eq!(
        hygiene["blocked_on"],
        blocked_on(1, 1, 0),
        "a user answer reads as a decision block: {hygiene}"
    );
    assert_eq!(hygiene["blocked_rate"], 0.667, "{hygiene}");
    assert_eq!(hygiene["verifier_unblock_rate"], 0.0, "{hygiene}");
    assert_eq!(hygiene["settle"], settle(0, 0, 1), "{hygiene}");

    let finder = row(&apply["blocked"]["agent"], "review-finder");
    assert_eq!(finder["blocked"], correctness["blocked"], "{finder}");
    assert_eq!(finder["verifier_unblocked"], 1, "{finder}");
    let sweeper = row(&apply["blocked"]["agent"], "review-sweeper");
    assert_eq!(sweeper["blocked_on"], hygiene["blocked_on"], "{sweeper}");

    assert_eq!(apply["settle"], settle(1, 1, 1), "{apply}");
    assert_eq!(apply["outcomes"], json!([]), "{apply}");
    assert_eq!(
        repo.read(BLOCKED_STORE_PATH),
        BLOCKED_STORE,
        "the store is only read"
    );
}

#[test]
fn stats_apply_reads_no_v1_ledger_and_leaves_outcomes_alone() {
    let repo = Repo::new();
    let before = repo.stats(&["--scope", "t", "--outcomes"]);
    fs::remove_file(repo.root.join(REVIEW_LEDGER)).unwrap();
    let out = repo.stats(&["--scope", "t", "--apply"]);
    assert_eq!(
        out["apply"]["outcomes"],
        json!([]),
        "an exported finding is not a native apply outcome: {out}"
    );
    repo.write(REVIEW_LEDGER, REVIEW);
    let both = repo.stats(&["--scope", "t", "--outcomes", "--apply"]);
    assert_eq!(both["outcomes"], before["outcomes"], "{both}");
}

#[test]
fn a_dismissal_is_a_veto_only_on_a_user_row_or_after_approval() {
    let repo = Repo::new();
    let dismissed_mechanical = |id: &str, run: &str| {
        format!(
            "\n[[findings]]\nid = \"{id}\"\nrun = \"{run}\"\nfamily = \"plan\"\nkind = \"defect\"\n\
             lens = \"feasibility\"\nroute = \"verifier\"\ntier = \"mechanical\"\n\
             status = \"dismissed\"\ndismiss_reason = \"no\"\n"
        )
    };
    let store = format!(
        "{PLAN_STORE}{}{}\n[[findings]]\nid = \"F9\"\nrun = \"U1\"\nfamily = \"plan\"\n\
         kind = \"advisory\"\nlens = \"intent\"\nroute = \"none\"\ntier = \"question\"\n\
         status = \"resolved\"\nresolution = \"kept by user\"\n",
        dismissed_mechanical("F7", "U1"),
        dismissed_mechanical("F8", "U2"),
    );
    repo.write(FLOW_STORE_PATH, &store);
    repo.write(
        ".claude/flows/f1/plan.toml",
        "schema_version = 1\n\n[[approval]]\ncommit = \"abc1234\"\n\
         at = 2026-11-09T10:45:00Z\nreview = \"ran\"\n",
    );
    let out = repo.stats(&["--slug", "f1", "--plan-outcomes"]);
    let runs = &out["plan_outcomes"]["runs"];
    let first = &runs[0];
    assert_eq!(first["tiers"]["mechanical"]["dismissed"], 1, "{first}");
    assert_eq!(first["tiers"]["mechanical"]["user-vetoed"], JsonValue::Null);
    assert_eq!(
        first["tiers"]["question"],
        json!({"user-answered": 1}),
        "{first}"
    );
    assert_eq!(first["veto_rate"], 0.5, "{first}");
    let second = &runs[1];
    assert_eq!(
        second["tiers"]["mechanical"],
        json!({"user-vetoed": 1}),
        "{second}"
    );

    repo.write(".claude/flows/f1/plan.toml", "schema_version = 1\n");
    let draft = repo.stats(&["--slug", "f1", "--plan-outcomes"]);
    assert_eq!(
        draft["plan_outcomes"]["runs"][1]["tiers"]["mechanical"],
        json!({"dismissed": 1}),
        "{draft}"
    );
}

#[test]
fn plan_outcomes_count_author_addressed() {
    let repo = Repo::new();
    let addressed = |id: &str, tier: &str| {
        format!(
            "\n[[findings]]\nid = \"{id}\"\nrun = \"U1\"\nfamily = \"plan\"\nkind = \"defect\"\n\
             lens = \"handfix\"\nroute = \"verifier\"\ntier = \"{tier}\"\nstatus = \"resolved\"\n\
             resolution = \"fixed by hand\"\naddressed = [\"task:fix-limit.spec.outcome\"]\n"
        )
    };
    let store = format!(
        "{PLAN_STORE}{}{}\n[[findings]]\nid = \"F9\"\nrun = \"U1\"\nfamily = \"plan\"\n\
         kind = \"defect\"\nlens = \"handfix\"\nroute = \"verifier\"\ntier = \"mechanical\"\n\
         status = \"dismissed\"\ndismiss_reason = \"no\"\n",
        addressed("F7", "mechanical"),
        addressed("F8", "question"),
    );
    repo.write(FLOW_STORE_PATH, &store);

    let out = repo.stats(&["--slug", "f1", "--plan-outcomes"]);
    let first = &out["plan_outcomes"]["runs"][0];
    assert_eq!(
        first["tiers"]["mechanical"],
        json!({"applied": 1, "author-addressed": 1, "dismissed": 1}),
        "{first}"
    );
    assert_eq!(
        first["tiers"]["question"],
        json!({"author-addressed": 1}),
        "{first}"
    );
    assert_eq!(
        first["veto_rate"], 0.5,
        "an addressed row is no veto: {first}"
    );
    assert_eq!(
        first["growth"]["edits"], 3,
        "a hand edit is not a tool edit: {first}"
    );

    let lenses = repo.stats(&["--slug", "f1", "--by", "lens"]);
    let handfix = row(&lenses["rows"], "handfix");
    assert_eq!(
        handfix["dismissed"], 1,
        "only the dismissed row counts as a dismissal: {handfix}"
    );
}

#[test]
fn overturns_group_by_model() {
    let repo = Repo::new();
    repo.write(FLOW_STORE_PATH, PLAN_STORE);
    let out = repo.stats(&["--slug", "f1", "--by", "model"]);
    assert_eq!(out["by"], "model", "{out}");
    let rows = &out["rows"];
    let keys: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["haiku", "opus", "sonnet"], "{rows}");

    let opus = row(rows, "opus");
    assert_eq!(opus["roles"], json!(["verifier"]), "{opus}");
    assert_eq!(opus["audited"], 1, "{opus}");
    assert_eq!(
        opus["overturned"], 1,
        "the audit ran under sonnet, yet the overturn lands on the verifying opus: {opus}"
    );
    let haiku = row(rows, "haiku");
    assert_eq!(haiku["audited"], 1, "{haiku}");
    assert_eq!(haiku["overturned"], 0, "{haiku}");
    let sonnet = row(rows, "sonnet");
    assert_eq!(sonnet["audited"], 0, "{sonnet}");
    assert_eq!(sonnet["overturned"], 0, "{sonnet}");
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

const CLASSES_STORE_PATH: &str = ".claude/review-v2/classes.toml";

/// One audited row of every class, overturns in three of them, an unaudited
/// row, and a run whose user let two waves past the ceiling.
const CLASSES_STORE: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-12T10:00:00Z"
models = { verifier = "opus", checker = "haiku" }
ceiling_overrides = [
  { wave = 2, ceiling = 10, planned = 8, added = 4, reason = "the user kept every lens" },
  { wave = 3, ceiling = 10, planned = 12, added = 1, reason = "one follow-up" },
]

[[runs]]
id = "U2"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-13T10:00:00Z"
models = { verifier = "sonnet" }

[[cells]]
id = "C1"
run = "U1"
slice = ["src/lib.rs"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "covered"

[[findings]]
id = "D1"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 1
summary = "confirmed and upheld"
status = "open"

[[findings]]
id = "D2"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 2
summary = "confirmed and overturned"
status = "open"

[[findings]]
id = "D3"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 3
summary = "refuted and overturned"
status = "refuted"

[[findings]]
id = "D4"
run = "U1"
cell = "C1"
family = "code"
kind = "defect"
lens = "correctness"
file = "src/lib.rs"
line = 4
summary = "confirmed, never audited"
status = "open"

[[findings]]
id = "K1"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "correctness"
file = "src/lib.rs"
line = 5
summary = "checked and overturned"
status = "open"

[[findings]]
id = "K2"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "correctness"
file = "src/lib.rs"
line = 6
summary = "checked and upheld"
status = "open"

[[findings]]
id = "K3"
run = "U1"
cell = "C1"
family = "code"
kind = "cleanup"
lens = "correctness"
file = "src/lib.rs"
line = 7
summary = "a cleanup a verifier decided"
status = "open"

[[verdicts]]
finding = "D1"
run = "U1"
by = "verifier"
verdict = "confirmed"

[[verdicts]]
finding = "D1"
run = "U2"
by = "audit"
verdict = "confirmed"

[[verdicts]]
finding = "D2"
run = "U1"
by = "verifier"
verdict = "confirmed"

[[verdicts]]
finding = "D2"
run = "U2"
by = "audit"
verdict = "refuted"

[[verdicts]]
finding = "D3"
run = "U1"
by = "verifier"
verdict = "refuted"

[[verdicts]]
finding = "D3"
run = "U2"
by = "audit"
verdict = "confirmed"

[[verdicts]]
finding = "D4"
run = "U1"
by = "verifier"
verdict = "confirmed"

[[verdicts]]
finding = "K1"
run = "U1"
by = "checker"
verdict = "confirmed"

[[verdicts]]
finding = "K1"
run = "U2"
by = "audit"
verdict = "refuted"

[[verdicts]]
finding = "K2"
run = "U1"
by = "checker"
verdict = "confirmed"

[[verdicts]]
finding = "K2"
run = "U2"
by = "audit"
verdict = "confirmed"

[[verdicts]]
finding = "K3"
run = "U1"
by = "verifier"
verdict = "confirmed"

[[verdicts]]
finding = "K3"
run = "U2"
by = "audit"
verdict = "confirmed"
"#;

/// Asserts a row's `audit_classes` entry for `class` is `(audited, overturned)`.
fn assert_class(row: &JsonValue, class: &str, audited: u64, overturned: u64) {
    let entry = &row["audit_classes"][class];
    assert_eq!(entry["audited"], audited, "{class}: {row}");
    assert_eq!(entry["overturned"], overturned, "{class}: {row}");
}

fn class_sum(row: &JsonValue, key: &str) -> u64 {
    row["audit_classes"]
        .as_object()
        .expect("audit_classes is an object")
        .values()
        .map(|c| c[key].as_u64().unwrap())
        .sum()
}

#[test]
fn audit_counts_split_by_class_per_lens_and_per_model() {
    let repo = Repo::new();
    repo.write(CLASSES_STORE_PATH, CLASSES_STORE);

    let lens = repo.stats(&["--scope", "classes", "--by", "lens"]);
    let correctness = row(&lens["rows"], "correctness");
    assert_eq!(correctness["audited"], 6, "{correctness}");
    assert_eq!(correctness["overturned"], 3, "{correctness}");
    assert_eq!(correctness["audit_overturn_rate"], 0.5, "{correctness}");
    assert_class(correctness, "confirmed_defect", 2, 1);
    assert_class(correctness, "refuted_defect", 1, 1);
    assert_class(correctness, "checked_cleanup", 2, 1);
    assert_class(correctness, "other", 1, 0);
    assert_eq!(
        correctness["audit_classes"]["refuted_defect"]["rate"], 1.0,
        "{correctness}"
    );
    assert_eq!(class_sum(correctness, "audited"), 6, "{correctness}");
    assert_eq!(class_sum(correctness, "overturned"), 3, "{correctness}");

    let model = repo.stats(&["--scope", "classes", "--by", "model"]);
    let opus = row(&model["rows"], "opus");
    assert_eq!(opus["audited"], 4, "{opus}");
    assert_eq!(opus["overturned"], 2, "{opus}");
    assert_class(opus, "confirmed_defect", 2, 1);
    assert_class(opus, "refuted_defect", 1, 1);
    assert_class(opus, "checked_cleanup", 0, 0);
    assert_class(opus, "other", 1, 0);
    assert_eq!(
        opus["audit_classes"]["checked_cleanup"]["rate"],
        JsonValue::Null,
        "{opus}"
    );
    assert_eq!(class_sum(opus, "audited"), 4, "{opus}");

    let haiku = row(&model["rows"], "haiku");
    assert_eq!(haiku["audited"], 2, "{haiku}");
    assert_eq!(haiku["overturned"], 1, "{haiku}");
    assert_class(haiku, "confirmed_defect", 0, 0);
    assert_class(haiku, "refuted_defect", 0, 0);
    assert_class(haiku, "checked_cleanup", 2, 1);
    assert_class(haiku, "other", 0, 0);
    assert_eq!(class_sum(haiku, "audited"), 2, "{haiku}");
}

#[test]
fn stats_by_run_counts_ceiling_overrides() {
    let repo = Repo::new();
    repo.write(CLASSES_STORE_PATH, CLASSES_STORE);
    let out = repo.stats(&["--scope", "classes", "--by", "run"]);
    let u1 = row(&out["rows"], "U1");
    assert_eq!(
        u1["ceiling_overrides"],
        json!({"count": 2, "over": 5}),
        "2 over in wave 2 and 3 over in wave 3: {u1}"
    );
    assert_class(u1, "checked_cleanup", 2, 1);
    let u2 = row(&out["rows"], "U2");
    assert_eq!(
        u2["ceiling_overrides"],
        json!({"count": 0, "over": 0}),
        "{u2}"
    );
}

#[test]
fn stats_apply_reads_a_verifier_unblock_from_its_recorded_block() {
    let repo = Repo::new();
    let store = BLOCKED_STORE.replace(
        "unblocked_by = \"verifier\"",
        "unblocked_by = \"verifier\"\nunblocked_from = \"design\"",
    );
    repo.write(BLOCKED_STORE_PATH, &store);
    let out = repo.stats(&["--scope", "blocked", "--apply"]);
    let correctness = row(&out["apply"]["blocked"]["lens"], "correctness");
    assert_eq!(correctness["blocked_on"]["design"], 1, "{correctness}");
    assert_eq!(correctness["blocked_on"]["unrecorded"], 0, "{correctness}");
    assert_eq!(correctness["verifier_unblocked"], 1, "{correctness}");
}

const YIELD_STORE_PATH: &str = ".claude/review-v2/yield.toml";

fn yield_cell(id: &str, run: &str, agent: &str, pass: Option<u32>) -> String {
    let pass = pass.map(|p| format!("pass = {p}\n")).unwrap_or_default();
    format!(
        "[[cells]]\nid = \"{id}\"\nrun = \"{run}\"\nwave = 1\n{pass}slice = [\"src/lib.rs\"]\n\
         concerns = [\"correctness\"]\nagent = \"{agent}\"\nreason = \"proposed\"\n\
         status = \"covered\"\nsealed = true\nnote = \"covered\"\n\n"
    )
}

/// A defect finding in `cell` with a verifier verdict of `verdict`, plus the
/// `extra` lines, which must set `status`.
fn yield_finding(id: &str, run: &str, cell: &str, verdict: &str, extra: &str) -> String {
    format!(
        "[[findings]]\nid = \"{id}\"\nrun = \"{run}\"\ncell = \"{cell}\"\nfamily = \"code\"\n\
         kind = \"defect\"\nlens = \"correctness\"\nroute = \"verifier\"\nfile = \"src/lib.rs\"\n\
         line = 1\nsummary = \"{id}\"\n{extra}\n\n\
         [[verdicts]]\nfinding = \"{id}\"\nrun = \"{run}\"\nby = \"verifier\"\n\
         verdict = \"{verdict}\"\ncitation = \"src/lib.rs:1\"\nrationale = \"read it\"\n\n"
    )
}

fn yield_store() -> String {
    let open = "status = \"open\"";
    let mut s = String::from("schema_version = 2\n\n");
    for (run, depth, day) in [
        ("U1", "standard", 1),
        ("U2", "standard", 2),
        ("U3", "deep", 3),
    ] {
        s += &format!(
            "[[runs]]\nid = \"{run}\"\ncommand = \"review-v2\"\nfamily = \"code\"\n\
             depth = \"{depth}\"\nstarted = \"2026-11-0{day}T10:00:00Z\"\n\n"
        );
    }
    s += &yield_cell("C1", "U1", "review-finder", None);
    s += &yield_cell("C2", "U1", "review-finder", Some(2));
    s += &yield_cell("C3", "U1", "review-finder-cf", None);
    s += &yield_cell("C4", "U2", "review-finder", None);
    s += &yield_cell("C5", "U2", "review-finder-cf", None);
    s += &yield_cell("C6", "U2", "review-finder", Some(2));
    s += &yield_cell("C7", "U3", "review-finder", None);
    // A repeat row the same as a first-pass row.
    s += &yield_finding("F1", "U1", "C1", "confirmed", open);
    s += &yield_finding(
        "F2",
        "U1",
        "C2",
        "confirmed",
        "same_as = \"F1\"\nstatus = \"open\"",
    );
    // A first-pass row merged into a repeat row.
    s += &yield_finding(
        "F3",
        "U1",
        "C1",
        "confirmed",
        "status = \"merged\"\nmerged_into = \"F4\"",
    );
    s += &yield_finding("F4", "U1", "C2", "confirmed", open);
    // A context-free row apply resolved.
    s += &yield_finding(
        "F5",
        "U1",
        "C3",
        "confirmed",
        "status = \"resolved\"\nresolution = \"abc123 fixed it\"\nresolved = 2026-11-04\n\
         apply_flow = \"fix-apply\"\napply_task = \"fix-f5\"",
    );
    // A refuted repeat row.
    s += &yield_finding("F7", "U1", "C2", "refuted", open);
    // A context-free row apply only scheduled.
    s += &yield_finding(
        "F8",
        "U2",
        "C5",
        "confirmed",
        "status = \"scheduled\"\napply_flow = \"fix-apply\"\napply_task = \"fix-f8\"",
    );
    // A repeat row the same as an earlier run's row.
    s += &yield_finding(
        "F9",
        "U2",
        "C6",
        "confirmed",
        "same_as = \"F1\"\nstatus = \"open\"",
    );
    s += &yield_finding("F10", "U3", "C7", "confirmed", open);
    s
}

fn arm(
    cells: u64,
    findings: u64,
    verified: u64,
    unique: u64,
    applied: u64,
    scheduled: u64,
) -> JsonValue {
    json!({
        "cells": cells,
        "findings": findings,
        "verified": verified,
        "unique": unique,
        "applied": applied,
        "scheduled": scheduled,
    })
}

#[test]
fn stats_yield_credits_only_clusters_without_a_first_pass_member() {
    let repo = Repo::new();
    let store = yield_store();
    repo.write(YIELD_STORE_PATH, &store);
    let out = repo.stats(&["--scope", "yield", "--yield"]);
    assert!(
        out.get("rows").is_none(),
        "a section alone prints no rows: {out}"
    );
    let runs = out["yield"]["runs"].as_array().expect("runs is an array");
    assert_eq!(runs.len(), 3, "{out}");

    let u1 = &runs[0];
    assert_eq!(
        (u1["run"].as_str(), u1["depth"].as_str()),
        (Some("U1"), Some("standard"))
    );
    assert_eq!(
        u1["first"],
        arm(1, 2, 2, 2, 0, 0),
        "the same_as pair and the merged pair each credit first: {u1}"
    );
    assert_eq!(
        u1["repeat"],
        arm(1, 3, 2, 0, 0, 0),
        "a repeat row joined to a first-pass row adds nothing unique, and a refuted one counts in findings only: {u1}"
    );
    assert_eq!(u1["cf"], arm(1, 1, 1, 1, 1, 0), "{u1}");
    assert_eq!(
        u1["cross_run"],
        json!({"first": 0, "repeat": 0, "cf": 0}),
        "{u1}"
    );

    let u2 = &runs[1];
    assert_eq!(u2["first"], arm(1, 0, 0, 0, 0, 0), "{u2}");
    assert_eq!(
        u2["repeat"],
        arm(1, 0, 0, 0, 0, 0),
        "a row the same as an earlier run's counts in cross_run only: {u2}"
    );
    assert_eq!(u2["cf"], arm(1, 1, 1, 1, 0, 1), "{u2}");
    assert_eq!(
        u2["cross_run"],
        json!({"first": 0, "repeat": 1, "cf": 0}),
        "{u2}"
    );

    let pooled = &out["yield"]["pooled"];
    assert_eq!(
        pooled["standard"]["first"],
        arm(2, 2, 2, 2, 0, 0),
        "{pooled}"
    );
    assert_eq!(
        pooled["standard"]["repeat"],
        arm(2, 3, 2, 0, 0, 0),
        "{pooled}"
    );
    assert_eq!(pooled["standard"]["cf"], arm(2, 2, 2, 2, 1, 1), "{pooled}");
    assert_eq!(
        pooled["standard"]["cross_run"],
        json!({"first": 0, "repeat": 1, "cf": 0}),
        "{pooled}"
    );
    assert_eq!(pooled["deep"]["first"], arm(1, 1, 1, 1, 0, 0), "{pooled}");
    assert_eq!(repo.read(YIELD_STORE_PATH), store, "the store is only read");
}
