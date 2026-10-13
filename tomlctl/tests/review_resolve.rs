//! `tomlctl review resolve` against a sandbox whose store is seeded by hand
//! with a prior run `U1` holding each target and a current run `U2` holding
//! the rows that name them in `same_as`. Verdicts go through `review verdict`.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{cli, sandbox};
use serde_json::Value as JsonValue;

const SCOPE: &str = "t";

const HEADER: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "deep"
started = "2026-10-01T10:00:00Z"

[[runs]]
id = "U2"
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
run = "U2"
wave = 1
slice = ["src/**"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "done"

[[cells]]
id = "C3"
run = "U2"
wave = 2
slice = ["src/**"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "done"
"#;

/// A defect row. `same_as` empty omits the key; `extra` is appended verbatim.
fn defect(id: &str, run: &str, cell: &str, status: &str, same_as: &str, extra: &str) -> String {
    let same_as = if same_as.is_empty() {
        String::new()
    } else {
        format!("same_as = \"{same_as}\"\n")
    };
    format!(
        r#"
[[findings]]
id = "{id}"
run = "{run}"
cell = "{cell}"
seen_in = ["{run}"]
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 30
symbol = "lib::limit"
summary = "the limit is off by one"
severity = "major"
{same_as}status = "{status}"
{extra}"#
    )
}

/// A prior-run target, `U1`/`C1`, with no `same_as`.
fn target(id: &str, status: &str, extra: &str) -> String {
    defect(id, "U1", "C1", status, "", extra)
}

/// A current-run row in wave 1 naming `same_as`.
fn row(id: &str, same_as: &str, extra: &str) -> String {
    defect(id, "U2", "C2", "unverified", same_as, extra)
}

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new(findings: &[String]) -> Self {
        let (dir, root) = sandbox();
        let store = root.join(".claude/review-v2/t.toml");
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(store, format!("{HEADER}{}", findings.concat())).unwrap();
        Self { _dir: dir, root }
    }

    fn verdict(&self, finding: &str, verdict: &str) {
        self.vote(finding, verdict, &["--by", "verifier"]);
    }

    fn vote(&self, finding: &str, verdict: &str, by: &[&str]) {
        cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "verdict", finding])
            .args(["--scope", SCOPE, "--verdict", verdict])
            .args(by)
            .args(["--citation", "src/lib.rs:30", "--rationale"])
            .arg(format!("{verdict} on reading the anchor"))
            .assert()
            .success();
    }

    fn resolve(&self, extra: &[&str]) -> JsonValue {
        let out = cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "resolve"])
            .args(["--scope", SCOPE, "--run", "U2"])
            .args(extra)
            .assert()
            .success();
        serde_json::from_slice(&out.get_output().stdout).expect("resolve prints JSON")
    }

    fn raw(&self) -> String {
        fs::read_to_string(self.root.join(".claude/review-v2/t.toml")).unwrap()
    }

    fn store(&self) -> toml::Value {
        toml::from_str(&self.raw()).unwrap()
    }

    fn findings(&self) -> Vec<toml::Value> {
        self.store()["findings"].as_array().unwrap().clone()
    }

    fn finding(&self, id: &str) -> toml::Value {
        self.findings()
            .into_iter()
            .find(|f| f["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no finding {id}"))
    }

    fn field(&self, id: &str, key: &str) -> Option<String> {
        self.finding(id)
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_string)
    }

    fn strings(&self, id: &str, key: &str) -> Vec<String> {
        self.finding(id)
            .get(key)
            .and_then(toml::Value::as_array)
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
            .unwrap_or_default()
    }

    /// `(by, verdict)` of every verdict recorded against `id`, in store order.
    fn verdicts_on(&self, id: &str) -> Vec<(String, String)> {
        self.store()
            .get("verdicts")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|v| v["finding"].as_str() == Some(id))
            .map(|v| {
                (
                    v["by"].as_str().unwrap().to_string(),
                    v["verdict"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    /// Asserts the row merged into `into` with `outcome`.
    fn assert_merged(&self, id: &str, into: &str, outcome: &str) {
        assert_eq!(self.field(id, "status").as_deref(), Some("merged"), "{id}");
        assert_eq!(self.field(id, "merged_into").as_deref(), Some(into), "{id}");
        assert_eq!(self.field(id, "outcome").as_deref(), Some(outcome), "{id}");
    }
}

fn s(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn same_as_repeat_merges_and_appends_seen_in_once() {
    let fresh = |id: &str| {
        row(
            id,
            "F1",
            "evidence = [\"src/lib.rs:34 still loops to len\"]\n",
        )
        .replace("line = 30", "line = 34")
    };
    let repo = Repo::new(&[
        target(
            "F1",
            "open",
            "evidence = [\"src/lib.rs:30 loops to len\"]\n",
        ),
        fresh("F2"),
        fresh("F3"),
    ]);
    repo.verdict("F2", "confirmed");
    repo.verdict("F3", "plausible");

    let out = repo.resolve(&[]);
    assert_eq!(out["resolved"].as_array().unwrap().len(), 2, "{out}");
    repo.assert_merged("F2", "F1", "repeat");
    repo.assert_merged("F3", "F1", "repeat");
    assert_eq!(repo.field("F1", "status").as_deref(), Some("open"));
    assert_eq!(
        repo.strings("F1", "seen_in"),
        s(&["U1", "U2"]),
        "two repeats from one run append it once"
    );
    assert_eq!(repo.finding("F1")["line"].as_integer(), Some(34));
    assert_eq!(
        repo.strings("F1", "evidence"),
        s(&["src/lib.rs:34 still loops to len"])
    );
}

#[test]
fn contested_row_merges_and_records_outcome() {
    let repo = Repo::new(&[
        target("F1", "exported", "exported_as = \"R7\"\n"),
        row("F2", "F1", "").replace("route = \"verifier\"", "route = \"panel\""),
    ]);
    repo.vote("F2", "refuted", &["--by", "panel", "--seat", "1"]);
    let waiting = repo.resolve(&[]);
    assert_eq!(
        waiting["pending"][0]["reason"], "verdicts incomplete",
        "one refuting seat of three decides nothing: {waiting}"
    );
    repo.vote("F2", "confirmed", &["--by", "panel", "--seat", "2"]);
    repo.vote("F2", "refuted", &["--by", "panel", "--seat", "3"]);

    let out = repo.resolve(&[]);
    assert_eq!(out["resolved"][0]["outcome"], "contested", "{out}");
    repo.assert_merged("F2", "F1", "contested");
    assert_eq!(repo.field("F1", "status").as_deref(), Some("exported"));
    assert_eq!(repo.strings("F1", "seen_in"), s(&["U1"]));
}

#[test]
fn refuted_same_as_on_open_is_contested() {
    let repo = Repo::new(&[target("F1", "unverified", ""), row("F2", "F1", "")]);
    repo.verdict("F1", "confirmed");
    repo.verdict("F2", "refuted");

    repo.resolve(&[]);
    repo.assert_merged("F2", "F1", "contested");
    assert_eq!(
        repo.field("F1", "status").as_deref(),
        Some("open"),
        "a contested target keeps its status"
    );
    assert_eq!(
        repo.verdicts_on("F1"),
        vec![
            ("verifier".to_string(), "confirmed".to_string()),
            ("verifier".to_string(), "refuted".to_string()),
        ],
        "the target keeps its own verdict and gains the refuting one"
    );
}

#[test]
fn confirmed_same_as_on_refuted_reopens() {
    let repo = Repo::new(&[target("F1", "unverified", ""), row("F2", "F1", "")]);
    repo.verdict("F1", "refuted");
    repo.verdict("F2", "confirmed");

    repo.resolve(&[]);
    repo.assert_merged("F2", "F1", "reopened");
    assert_eq!(repo.field("F1", "status").as_deref(), Some("open"));
    assert_eq!(
        repo.verdicts_on("F1").last(),
        Some(&("verifier".to_string(), "confirmed".to_string())),
        "the reopened target gains the confirming verdict"
    );
}

#[test]
fn plausible_same_as_on_refuted_is_contested() {
    let repo = Repo::new(&[target("F1", "unverified", ""), row("F2", "F1", "")]);
    repo.verdict("F1", "refuted");
    repo.verdict("F2", "plausible");

    repo.resolve(&[]);
    repo.assert_merged("F2", "F1", "contested");
    assert_eq!(repo.field("F1", "status").as_deref(), Some("refuted"));
}

#[test]
fn confirmed_same_as_on_resolved_is_regression() {
    let repo = Repo::new(&[
        target(
            "F1",
            "resolved",
            "resolution = \"fixed\"\nresolved = 2026-10-02\n",
        ),
        row("F2", "F1", ""),
    ]);
    repo.verdict("F2", "confirmed");

    repo.resolve(&[]);
    assert_eq!(repo.field("F2", "status").as_deref(), Some("open"));
    assert_eq!(repo.field("F2", "outcome").as_deref(), Some("regression"));
    assert_eq!(repo.strings("F2", "related"), s(&["F1"]));
    assert_eq!(repo.field("F2", "merged_into"), None);
    assert_eq!(repo.field("F1", "status").as_deref(), Some("resolved"));
}

#[test]
fn same_as_on_parked_is_suppressed() {
    let repo = Repo::new(&[
        target("F1", "parked", "park_reason = \"waits on the v3 store\"\n"),
        row("F2", "F1", ""),
    ]);
    repo.verdict("F2", "confirmed");

    repo.resolve(&[]);
    repo.assert_merged("F2", "F1", "suppressed");
    assert_eq!(repo.field("F1", "status").as_deref(), Some("parked"));
}

/// `F2` names `F3`, a row of the same run that itself names `F1`, so `F2`
/// can resolve only after `F3` has, whichever verdict landed first.
#[test]
fn same_run_target_resolves_first_regardless_of_verdict_order() {
    let findings = [
        target("F1", "open", ""),
        row("F2", "F3", ""),
        row("F3", "F1", ""),
    ];

    let early = Repo::new(&findings);
    early.verdict("F2", "confirmed");
    let first = early.resolve(&[]);
    assert_eq!(
        first["pending"][0]["reason"], "target F3 is unverified",
        "{first}"
    );
    early.verdict("F3", "refuted");
    early.resolve(&[]);

    let late = Repo::new(&findings);
    late.verdict("F3", "refuted");
    late.verdict("F2", "confirmed");
    let out = late.resolve(&[]);
    assert_eq!(out["pending"], serde_json::json!([]), "{out}");

    for repo in [&early, &late] {
        repo.assert_merged("F3", "F1", "contested");
        repo.assert_merged("F2", "F1", "repeat");
        assert_eq!(repo.strings("F1", "seen_in"), s(&["U1", "U2"]));
    }
    assert_eq!(
        early.findings(),
        late.findings(),
        "verdict order must not change the resolved findings"
    );
}

#[test]
fn wave_limits_resolution_to_its_cells() {
    let repo = Repo::new(&[
        target("F1", "open", ""),
        row("F2", "F1", ""),
        defect("F3", "U2", "C3", "unverified", "F1", ""),
    ]);
    repo.verdict("F2", "confirmed");
    repo.verdict("F3", "confirmed");

    repo.resolve(&["--wave", "2"]);
    assert_eq!(repo.field("F2", "status").as_deref(), Some("unverified"));
    repo.assert_merged("F3", "F1", "repeat");
}

#[test]
fn resolve_is_idempotent() {
    let repo = Repo::new(&[
        target("F1", "unverified", ""),
        row("F2", "F1", ""),
        row("F3", "F1", ""),
    ]);
    repo.verdict("F1", "confirmed");
    repo.verdict("F2", "refuted");
    repo.verdict("F3", "confirmed");

    let first = repo.resolve(&[]);
    assert_eq!(first["unchanged"], false, "{first}");
    let after_first = repo.raw();

    let second = repo.resolve(&[]);
    assert_eq!(second["unchanged"], true, "{second}");
    assert_eq!(second["resolved"], serde_json::json!([]), "{second}");
    assert_eq!(
        repo.raw(),
        after_first,
        "a second resolve must not rewrite the store"
    );
    assert_eq!(repo.verdicts_on("F1").len(), 2);
}
