//! Tests for phased admission in `review plan --propose`: judgement lenses
//! first, then the sweeper pool, then repeats from the room left, with the
//! context-free cells and the `--reserve` the carrier asks for.

use std::fs;

use crate::review_harness::{
    Repo, RunSpec, STORE, VOCABULARY, ceiling_cells, ceiling_cut, concerns, lens, strings,
    with_concern,
};
use serde_json::Value as JsonValue;

/// Twenty top-level directories of twenty files each.
fn wide_files() -> Vec<String> {
    (0..20)
        .flat_map(|d| (0..20).map(move |f| format!("d{d:02}/f{f:02}.rs")))
        .collect()
}

fn wide_repo() -> Option<Repo> {
    let files = wide_files();
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    Repo::new(&refs)
}

/// Trust-boundary accepted and `d19/f00.rs` the one hotspot.
fn wide_signals() -> String {
    let churn: Vec<String> = (0..10)
        .map(|d| format!("\"d{:02}/f00.rs\"", 19 - d))
        .collect();
    format!(
        r#"[{{ name = "trust-boundary", source = "judged", accepted = true }}, {{ name = "churn-hotspot", source = "detector", roots = ["."], evidence = [{}] }}]"#,
        churn.join(", ")
    )
}

fn seed_wide(repo: &Repo, depth: &str, signals: &str) {
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth,
        changed_lines: Some(4000),
        scope: &["d*/**"],
        signals,
    }]);
}

fn is_sweeper(row: &JsonValue) -> bool {
    row["agent"] == "review-sweeper"
}

fn is_repeat(row: &JsonValue) -> bool {
    row["pass"] == 2
}

fn overwrite_lens(repo: &Repo, id: &str, text: String) {
    fs::write(repo.builtin.join(format!("lenses/{id}.md")), text).unwrap();
}

#[test]
fn a_large_standard_scope_keeps_sweepers_in_the_pool_and_matched_lenses_before_repeats() {
    let Some(repo) = wide_repo() else {
        return;
    };
    seed_wide(&repo, "standard", &wide_signals());
    // Without doc-drift's twenty cells, idioms is the non-floor cell holding
    // the hotspot.
    let (rows, summary) = repo.propose(STORE, "U1", &["--lens", "-doc-drift"]);
    assert_eq!(summary["ceiling"], 16, "{summary}");
    assert_eq!(summary["mechanical_pool"], 4, "ceil(16 / 4): {summary}");
    assert!(ceiling_cells(&rows) <= 16, "{rows:?}");

    let sweepers: Vec<&JsonValue> = rows.iter().filter(|r| is_sweeper(r)).collect();
    assert!(
        !sweepers.is_empty() && sweepers.len() <= 4,
        "the sweepers share a pool of four: {sweepers:?}"
    );
    assert!(
        !sweepers.iter().any(|r| is_repeat(r)),
        "a sweeper cell never repeats: {sweepers:?}"
    );

    let repeats: Vec<&JsonValue> = rows.iter().filter(|r| is_repeat(r)).collect();
    assert!(!repeats.is_empty(), "room is left for repeats: {rows:?}");
    assert!(
        !with_concern(&rows, "security").is_empty() && !ceiling_cut(&summary, "security"),
        "the matched signal lens keeps its cells while a repeat is kept: {summary}"
    );

    // Room is short of a repeat for every floor cell, so the hotspot cells
    // must take theirs first.
    let holds_hotspot = |r: &JsonValue| {
        strings(&r["slice"])
            .iter()
            .any(|e| e == "d19/f00.rs" || e == "d19/**" || e == "d*/**")
    };
    let owed: Vec<&JsonValue> = rows
        .iter()
        .filter(|r| r.get("pass").is_none() && !is_sweeper(r) && holds_hotspot(r))
        .collect();
    assert!(
        owed.len() >= 2,
        "a floor and a non-floor cell hold the hotspot: {rows:?}"
    );
    let due = rows
        .iter()
        .filter(|r| r.get("pass").is_none() && !is_sweeper(r))
        .filter(|r| concerns(r).contains(&"correctness".to_string()) || holds_hotspot(r))
        .count();
    assert!(
        repeats.len() < due,
        "the fixture leaves too little room to repeat every owed cell: {rows:?}"
    );
    for first in owed {
        let mut reversed = strings(&first["slice"]);
        reversed.reverse();
        assert!(
            repeats
                .iter()
                .any(|r| concerns(r) == concerns(first) && strings(&r["slice"]) == reversed),
            "the hotspot cell {first} keeps its repeat: {repeats:?}"
        );
    }
    let first_cold = repeats
        .iter()
        .position(|r| !holds_hotspot(r))
        .unwrap_or(repeats.len());
    assert!(
        repeats[first_cold..].iter().all(|r| !holds_hotspot(r)),
        "hotspot repeats come first: {repeats:?}"
    );
}

#[test]
fn reserve_leaves_its_cells_unused() {
    let Some(repo) = wide_repo() else {
        return;
    };
    seed_wide(&repo, "standard", &wide_signals());
    let (full, _) = repo.propose(STORE, "U1", &[]);
    assert_eq!(
        ceiling_cells(&full),
        16,
        "the scope fills the ceiling: {full:?}"
    );
    let (rows, summary) = repo.propose(STORE, "U1", &["--reserve", "2"]);
    assert_eq!(summary["reserve"], 2, "{summary}");
    assert_eq!(
        ceiling_cells(&rows),
        14,
        "two cells of the ceiling stay free: {rows:?}"
    );
}

#[test]
fn context_free_cells_are_proposed_within_a_sixth_of_the_ceiling() {
    let Some(repo) = wide_repo() else {
        return;
    };
    seed_wide(&repo, "standard", &wide_signals());
    let (rows, summary) = repo.propose(STORE, "U1", &["--context-free"]);
    let cf: Vec<&JsonValue> = rows
        .iter()
        .filter(|r| r["agent"] == "review-finder-cf")
        .collect();
    assert!(
        !cf.is_empty() && cf.len() <= 3,
        "at most ceil(16 / 6) context-free cells: {cf:?}"
    );
    assert_eq!(summary["context_free"], cf.len(), "{summary}");
    for cell in &cf {
        assert_eq!(concerns(cell), vec!["correctness"], "{cell}");
        assert_eq!(
            cell["question"],
            "context-free variant: diff only, no pack, no prior findings"
        );
        assert_eq!(
            cell["reason"],
            "context-free correctness pass over the diff, independent of the pack"
        );
        assert!(
            cell.get("pass").is_none(),
            "a context-free cell never repeats"
        );
    }
    let mut covered: Vec<String> = cf.iter().flat_map(|c| strings(&c["slice"])).collect();
    covered.sort();
    let mut correctness: Vec<String> = rows
        .iter()
        .filter(|r| {
            r["agent"] == "review-finder"
                && r.get("pass").is_none()
                && concerns(r).contains(&"correctness".to_string())
        })
        .flat_map(|c| strings(&c["slice"]))
        .collect();
    correctness.sort();
    assert_eq!(
        covered, correctness,
        "the context-free cells cover correctness's slices"
    );
    assert!(ceiling_cells(&rows) <= 16, "{rows:?}");
    repo.plan("U1", &rows);
}

#[test]
fn context_free_is_refused_at_quick() {
    let Some(repo) = wide_repo() else {
        return;
    };
    seed_wide(&repo, "quick", "[]");
    let out = repo
        .tomlctl()
        .args([
            "review",
            "plan",
            "--run",
            "U1",
            "--propose",
            "--context-free",
        ])
        .args(STORE)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("--context-free"), "{stderr}");
}

#[test]
fn a_signal_lens_merges_down_to_the_room_left() {
    let Some(repo) = Repo::new(&["a/x.rs", "b/y.rs", "c/z.rs", "d/w.rs"]) else {
        return;
    };
    overwrite_lens(
        &repo,
        "security",
        lens(
            "security",
            "code",
            "judgement",
            "signal",
            "per-file",
            "exploit",
            "apply_when = { signals = [\"trust-boundary\"] }\n",
        ),
    );
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "standard",
        changed_lines: Some(400),
        scope: &["a/**", "b/**", "c/**", "d/**"],
        signals: r#"[{ name = "trust-boundary", source = "judged", accepted = true }]"#,
    }]);
    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert_eq!(summary["ceiling"], 4, "{summary}");
    let security = with_concern(&rows, "security");
    assert!(
        !security.is_empty() && security.len() < 4,
        "security's four per-file slices merge to fit: {security:?}"
    );
    assert!(!ceiling_cut(&summary, "security"), "{summary}");
    assert!(ceiling_cells(&rows) <= 4, "{rows:?}");
}

#[test]
fn a_signal_lens_that_cannot_fit_one_cell_is_cut_with_no_repeat() {
    let Some(repo) = Repo::new(&["a/x.rs", "a/y.md", "b/z.rs", "b/w.md"]) else {
        return;
    };
    let routing = "\n[routing]\nexecutable = [\"**/*.rs\"]\n";
    fs::write(
        repo.builtin.join("vocabulary.toml"),
        format!("{VOCABULARY}{routing}"),
    )
    .unwrap();
    overwrite_lens(
        &repo,
        "correctness",
        lens(
            "correctness",
            "code",
            "judgement",
            "floor",
            "whole-scope",
            "logic",
            "",
        ),
    );
    // Code and prose slices never merge, so security needs two cells.
    overwrite_lens(
        &repo,
        "security",
        lens(
            "security",
            "code",
            "judgement",
            "signal",
            "per-subsystem",
            "exploit",
            "apply_when = { signals = [\"trust-boundary\"] }\nkinds = [\"code\", \"other\"]\n",
        ),
    );
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "standard",
        changed_lines: Some(300),
        scope: &["a/**", "b/**"],
        signals: r#"[{ name = "trust-boundary", source = "judged", accepted = true }]"#,
    }]);
    let (rows, summary) = repo.propose(STORE, "U1", &[]);
    assert_eq!(summary["ceiling"], 3, "{summary}");
    assert!(with_concern(&rows, "security").is_empty(), "{rows:?}");
    assert!(ceiling_cut(&summary, "security"), "{summary}");
    assert!(
        ceiling_cells(&rows) < 3,
        "a cell of room is left for a repeat: {rows:?}"
    );
    assert!(
        !rows.iter().any(is_repeat),
        "nothing repeats while a matched lens is cut: {rows:?}"
    );

    assert_eq!(summary["mechanical_pool"], 1, "{summary}");
    for sweeper in ["ripple", "hygiene"] {
        assert_eq!(
            with_concern(&rows, sweeper).len(),
            1,
            "{sweeper} holds one cell once a judgement lens is cut: {rows:?}"
        );
    }
}
