//! Tests for kind-aware slicing: `review plan --propose` against a catalogue
//! whose `[routing]` table classifies files as code, test, lockfile or other.

use std::fs;

use crate::review_harness::{
    Repo, RunSpec, STORE, VOCABULARY, concerns, lens, strings, with_concern,
};
use serde_json::{Value as JsonValue, json};

const ROUTING: &str = r#"
[concerns.missing-tests]
meaning = "untested change"
families = ["code"]

[concerns.dead-tests]
meaning = "tests that test nothing"
families = ["code"]

[routing]
executable = ["**/*.rs"]
tests = ["**/tests/**"]
lockfiles = ["**/Cargo.lock"]
"#;

/// The harness catalogue with `[routing]`, correctness reading code and
/// prose, security sliced by its signals' evidence, test-gaps by changed code
/// with its tests, and a floor lens reading only tests.
fn kinded_repo(files: &[&str]) -> Option<Repo> {
    let repo = Repo::new(files)?;
    let lenses = [
        (
            "correctness",
            lens(
                "correctness",
                "code",
                "judgement",
                "floor",
                "per-subsystem",
                "logic",
                "kinds = [\"code\", \"other\"]\n",
            ),
        ),
        (
            "security",
            lens(
                "security",
                "code",
                "judgement",
                "signal",
                "signal-evidence",
                "exploit",
                "apply_when = { signals = [\"trust-boundary\"] }\n",
            ),
        ),
        (
            "test-gaps",
            lens(
                "test-gaps",
                "code",
                "judgement",
                "candidate",
                "changed-with-tests",
                "missing-tests",
                "",
            ),
        ),
        (
            "test-pruning",
            lens(
                "test-pruning",
                "code",
                "judgement",
                "floor",
                "per-subsystem",
                "dead-tests",
                "kinds = [\"test\"]\n",
            ),
        ),
    ];
    for (id, text) in lenses {
        fs::write(repo.builtin.join(format!("lenses/{id}.md")), text).unwrap();
    }
    fs::write(
        repo.builtin.join("vocabulary.toml"),
        format!("{VOCABULARY}{ROUTING}"),
    )
    .unwrap();
    Some(repo)
}

const MIXED: &[&str] = &[
    "crate/Cargo.toml",
    "crate/Cargo.lock",
    "crate/README.md",
    "crate/src/a.rs",
    "crate/src/b.rs",
    "crate/tests/a_test.rs",
];

const RUST_ROOT: &str = r#"{ name = "lang:rust", source = "detector", roots = ["crate"] }"#;

fn seed(repo: &Repo, scope: &[&str], signals: &str) {
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "standard",
        changed_lines: None,
        scope,
        signals,
    }]);
}

fn first_pass(rows: &[JsonValue]) -> Vec<JsonValue> {
    rows.iter()
        .filter(|r| r.get("pass").is_none())
        .cloned()
        .collect()
}

fn slices(rows: &[JsonValue], concern: &str) -> Vec<Vec<String>> {
    with_concern(rows, concern)
        .into_iter()
        .map(|r| strings(&r["slice"]))
        .collect()
}

#[test]
fn correctness_keeps_code_and_prose_apart_and_never_reads_a_lockfile() {
    let Some(repo) = kinded_repo(MIXED) else {
        return;
    };
    seed(&repo, &["crate/**"], &format!("[{RUST_ROOT}]"));
    let (rows, _) = repo.propose(STORE, "U1", &[]);
    let rows = first_pass(&rows);
    let mut got = slices(&rows, "correctness");
    got.sort();
    assert_eq!(
        got,
        vec![
            vec![
                "crate/Cargo.toml".to_string(),
                "crate/README.md".to_string()
            ],
            vec!["crate/src/**".to_string()],
        ],
        "one code slice and one prose slice, the lockfile in neither: {rows:?}"
    );
    for row in with_concern(&rows, "correctness") {
        let slice = strings(&row["slice"]);
        assert!(
            !slice
                .iter()
                .any(|e| e == "crate/Cargo.lock" || e == "crate/**"),
            "no correctness slice holds or covers the lockfile: {row}"
        );
    }
}

#[test]
fn security_slices_to_its_signal_evidence_and_falls_back_per_subsystem() {
    let Some(repo) = kinded_repo(MIXED) else {
        return;
    };
    seed(
        &repo,
        &["crate/**"],
        &format!(
            r#"[{RUST_ROOT}, {{ name = "trust-boundary", source = "judged", accepted = true, evidence = ["crate/src/a.rs"] }}]"#
        ),
    );
    let (rows, _) = repo.propose(STORE, "U1", &[]);
    assert_eq!(
        slices(&first_pass(&rows), "security"),
        vec![vec!["crate/src/a.rs".to_string()]],
        "security reads the evidence file only: {rows:?}"
    );

    seed(
        &repo,
        &["crate/**"],
        &format!(
            r#"[{RUST_ROOT}, {{ name = "trust-boundary", source = "judged", accepted = true, evidence = [] }}]"#
        ),
    );
    let (rows, _) = repo.propose(STORE, "U1", &[]);
    assert_eq!(
        slices(&first_pass(&rows), "security"),
        vec![vec!["crate/src/**".to_string()]],
        "with no evidence, security slices per subsystem over code: {rows:?}"
    );
}

#[test]
fn test_gaps_holds_changed_code_with_its_subsystem_tests_and_no_prose() {
    let Some(repo) = kinded_repo(MIXED) else {
        return;
    };
    seed(
        &repo,
        &["crate/src/**", "crate/README.md"],
        &format!("[{RUST_ROOT}]"),
    );
    let (rows, _) = repo.propose(STORE, "U1", &["--lens", "+test-gaps"]);
    assert_eq!(
        slices(&first_pass(&rows), "test-gaps"),
        vec![vec![
            "crate/src/a.rs".to_string(),
            "crate/src/b.rs".to_string(),
            "crate/tests/a_test.rs".to_string(),
        ]],
        "the changed code plus the tracked tests under the same root: {rows:?}"
    );
}

#[test]
fn a_test_only_lens_is_unmatched_on_a_scope_without_tests() {
    let Some(repo) = kinded_repo(MIXED) else {
        return;
    };
    seed(&repo, &["crate/src/**"], &format!("[{RUST_ROOT}]"));
    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert!(
        with_concern(&rows, "test-pruning").is_empty(),
        "no test in scope, no test-pruning cell: {rows:?}"
    );
    assert!(
        !summary["unselected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["concern"] == "test-pruning"),
        "{summary}"
    );
    let out = repo.plan(
        "U1",
        &[json!({"slice": ["crate/src/**"], "concerns": ["ripple"], "agent": "review-sweeper", "reason": "proposed"})],
    );
    assert!(
        !strings(&out["accounting"]["unselected"]).contains(&"test-pruning".to_string()),
        "accounting agrees the lens is unmatched: {out}"
    );

    seed(&repo, &["crate/**"], &format!("[{RUST_ROOT}]"));
    let (rows, _) = repo.propose(STORE, "U1", &[]);
    assert_eq!(
        slices(&first_pass(&rows), "test-pruning"),
        vec![vec!["crate/tests/**".to_string()]],
        "with a test in scope the lens reads it alone: {rows:?}"
    );
}

#[test]
fn a_catalogue_without_routing_slices_every_kind_together() {
    let Some(repo) = Repo::new(&["src/a.rs", "src/Cargo.lock", "src/README.md"]) else {
        return;
    };
    seed(&repo, &["src/**"], "[]");
    let (rows, _) = repo.propose(STORE, "U1", &[]);
    assert_eq!(
        slices(&first_pass(&rows), "correctness"),
        vec![vec!["src/**".to_string()]],
        "kind matching is off without [routing]: {rows:?}"
    );
}

#[test]
fn a_plan_run_keeps_security_and_test_gaps_on_its_documents() {
    let Some(repo) = Repo::new(&["docs/plans/a.md", "docs/plans/b.md"]) else {
        return;
    };
    let plan_lenses = [
        (
            "plan-security",
            lens(
                "plan-security",
                "plan",
                "judgement",
                "signal",
                "signal-evidence",
                "plan-exploit",
                "apply_when = { signals = [\"trust-boundary\"] }\n",
            ),
        ),
        (
            "plan-test-gaps",
            lens(
                "plan-test-gaps",
                "plan",
                "judgement",
                "floor",
                "changed-with-tests",
                "plan-tests",
                "",
            ),
        ),
    ];
    for (id, text) in plan_lenses {
        fs::write(repo.builtin.join(format!("lenses/{id}.md")), text).unwrap();
    }
    fs::write(
        repo.builtin.join("vocabulary.toml"),
        format!(
            "{VOCABULARY}\n[concerns.plan-exploit]\nmeaning = \"an unsafe plan step\"\n\
             families = [\"plan\"]\n\n[concerns.plan-tests]\nmeaning = \"an untested plan step\"\n\
             families = [\"plan\"]\n{}",
            ROUTING
                .split_once("[routing]")
                .map(|(_, r)| format!("[routing]{r}"))
                .unwrap()
        ),
    )
    .unwrap();
    repo.seed(&[RunSpec {
        id: "P1",
        family: "plan",
        depth: "standard",
        changed_lines: None,
        scope: &["docs/plans/**"],
        signals: r#"[{ name = "trust-boundary", source = "judged", accepted = true, evidence = [] }]"#,
    }]);
    let (rows, _) = repo.propose(STORE, "P1", &[]);
    let all: Vec<String> = rows.iter().flat_map(concerns).collect();
    for lens in ["plan-security", "plan-test-gaps"] {
        assert!(
            !slices(&first_pass(&rows), lens).is_empty(),
            "{lens} still slices the plan documents: {all:?}"
        );
    }
}
