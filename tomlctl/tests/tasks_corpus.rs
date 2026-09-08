//! Corpus smoke test: every house-format plan under `docs/plans/` imports.
//!
//! The importer's unit tests feed it hand-written fixtures; this one feeds it
//! the real corpus, which is CRLF on disk. Nothing here normalises line
//! endings — a `sections()` regression that stopped stripping the trailing
//! `\r` surfaces as a corpus-wide parse failure rather than as a fixture
//! nobody thought to write.
//!
//! The file list is derived at runtime; a transcribed one goes stale the day
//! someone adds a plan. `EXPECTED_UNPARSEABLE` is the only hardcoded
//! membership, and each member is asserted to STILL fail, so a plan that gets
//! fixed cannot leave a stale exemption behind.

use assert_cmd::Command;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Plans the importer is expected to reject, by file name, with the reason.
/// Each is asserted to STILL fail, where failing is a non-zero exit or an
/// error-class finding in the envelope — the two ways a plan does not import.
const EXPECTED_UNPARSEABLE: &[(&str, &str)] = &[
    (
        "lumina-story-planning-round-2.md",
        "task id `13a` is not an integer (a hand-inserted task between 13 and 14)",
    ),
    (
        "specialised-flow-agents.md",
        "its Wave 1/2 tasks 11-17 are `#####` headings, below the `#{3,4}` task \
         grammar, so they parse as phase labels and task 18's `Depends on` dangles",
    ),
    (
        "tomlctl-capability-gaps.md",
        "effort tag `[M-leaning-L]` is outside the S|M|L vocabulary",
    ),
];

/// Derived companions of a plan, not plans themselves: pre-merge snapshots,
/// research dumps, superseded revisions, and the census / follow-up reports.
/// None is a `/implement` input, and several carry deliberately partial task
/// sections.
const SKIP_SUFFIXES: &[&str] = &[
    ".premerge.md",
    ".research.md",
    ".revised.md",
    ".preflight.md",
    ".stub.md",
    "-CENSUS.md",
    "-FOLLOWUP.md",
];

/// Every `docs/plans/*.md` that survives `SKIP_SUFFIXES` and carries both a
/// `## Tasks` heading and, anywhere in the file, a numbered task heading. The
/// second condition is what separates a house-format plan from the older
/// `#### T1:` ad-hoc shape and from a design brief whose Tasks section is
/// prose.
fn in_scope_plans(plans_dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(plans_dir)
        .expect("docs/plans is readable")
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = file_name(&path);
        if SKIP_SUFFIXES.iter().any(|s| name.ends_with(s)) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let mut has_section = false;
        let mut has_task = false;
        for line in source.lines() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            has_section |= line
                .strip_prefix("## ")
                .is_some_and(|title| title.trim().eq_ignore_ascii_case("Tasks"));
            has_task |= is_numbered_task_heading(line);
        }
        if has_section && has_task {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// `^#{3,4} [0-9]+\. `, spelled out. Hand-rolled rather than compiled: the
/// binary resolves `regex` without its unicode features and a test build
/// unifies them back on, so a pattern here would be checked under settings the
/// shipped binary does not use.
fn is_numbered_task_heading(line: &str) -> bool {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if !(3..=4).contains(&hashes) {
        return false;
    }
    let Some(rest) = line[hashes..].strip_prefix(' ') else {
        return false;
    };
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    digits > 0 && rest[digits..].starts_with(". ")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

struct ImportRun {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn import_dry_run(repo_root: &Path, plan: &Path) -> ImportRun {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .current_dir(repo_root)
        .env("TOMLCTL_ROOT", repo_root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("tasks")
        .arg("import-plan")
        .arg("--plan")
        .arg(plan)
        .arg("--dry-run")
        .write_stdin("")
        .output()
        .expect("tomlctl runs");
    ImportRun {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    }
}

/// `class: detail` for each `error`-severity finding in a dry-run envelope.
fn error_findings(stdout: &str) -> Vec<String> {
    let envelope: serde_json::Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(e) => return vec![format!("envelope is not JSON ({e}): {}", stdout.trim())],
    };
    envelope
        .get("findings")
        .and_then(|f| f.as_array())
        .map(|rows| {
            rows.iter()
                .filter(|row| row.get("severity").and_then(|s| s.as_str()) == Some("error"))
                .map(|row| {
                    format!(
                        "{}: {}",
                        row.get("class").and_then(|c| c.as_str()).unwrap_or("?"),
                        row.get("detail").and_then(|d| d.as_str()).unwrap_or("?"),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

/// Every in-scope plan in the live corpus imports without an error-class
/// finding, and every `EXPECTED_UNPARSEABLE` member still fails.
///
/// Self-skips when `docs/plans/` is absent — a packaged checkout ships the
/// crate without the repo's plan corpus — the same graceful skip
/// `command_lint` takes when `claude/` is missing.
#[test]
fn corpus_plans_import_cleanly() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let plans_dir = repo_root.join("docs").join("plans");
    if !plans_dir.is_dir() {
        eprintln!("tasks_corpus: docs/plans/ not found, skipping");
        return;
    }

    let plans = in_scope_plans(&plans_dir);
    assert!(
        !plans.is_empty(),
        "docs/plans/ exists but no in-scope plan was found — the scope filter \
         ({SKIP_SUFFIXES:?} + `## Tasks` + a numbered task heading) matches \
         nothing, so this test would pass vacuously"
    );

    let allowlist: BTreeSet<&str> = EXPECTED_UNPARSEABLE.iter().map(|(name, _)| *name).collect();
    let mut in_corpus: BTreeSet<&str> = BTreeSet::new();
    // (file, first stderr line)
    let mut refused: Vec<(String, String)> = Vec::new();
    // (file, "class: detail")
    let mut errors: Vec<(String, String)> = Vec::new();
    // allowlisted files that imported cleanly
    let mut no_longer_failing: Vec<&str> = Vec::new();

    for plan in &plans {
        let name = file_name(plan);
        let run = import_dry_run(&repo_root, plan);

        if let Some(known) = allowlist.get(name.as_str()) {
            in_corpus.insert(known);
            if run.ok && error_findings(&run.stdout).is_empty() {
                no_longer_failing.push(known);
            }
            continue;
        }
        if !run.ok {
            refused.push((name, first_line(&run.stderr)));
            continue;
        }
        for finding in error_findings(&run.stdout) {
            errors.push((name.clone(), finding));
        }
    }

    // One message for every class of failure, so a corpus-wide regression
    // names every offending file in a single run rather than the first.
    let mut msg = String::new();
    if !refused.is_empty() {
        msg.push_str(&format!(
            "{} plan(s) failed `tasks import-plan --dry-run` (non-zero exit):\n",
            refused.len()
        ));
        for (name, err) in &refused {
            msg.push_str(&format!("  docs/plans/{name}\n    {err}\n"));
        }
    }
    if !errors.is_empty() {
        msg.push_str(&format!(
            "{} error-class finding(s) over the plan corpus:\n",
            errors.len()
        ));
        for (name, finding) in &errors {
            msg.push_str(&format!("  docs/plans/{name}\n    {finding}\n"));
        }
    }
    if !no_longer_failing.is_empty() {
        msg.push_str(
            "EXPECTED_UNPARSEABLE member(s) now import cleanly (exit 0, no error-class \
             finding) — drop them from the allowlist:\n",
        );
        for name in &no_longer_failing {
            let reason = EXPECTED_UNPARSEABLE
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, r)| *r)
                .unwrap_or("");
            msg.push_str(&format!("  docs/plans/{name}\n    was: {reason}\n"));
        }
    }
    for (name, reason) in EXPECTED_UNPARSEABLE {
        if !in_corpus.contains(name) {
            msg.push_str(&format!(
                "EXPECTED_UNPARSEABLE names `docs/plans/{name}`, which is not an \
                 in-scope plan (renamed, deleted, or now out of scope)\n    was: \
                 {reason}\n"
            ));
        }
    }
    assert!(msg.is_empty(), "{msg}");
}
