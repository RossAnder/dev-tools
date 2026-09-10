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
///
/// Membership says the document breaks an authoring rule and is left as it
/// stands — not that a defect is outstanding. The rules are the task-store
/// contract's (`claude/skills/flow-contract-task-store/SKILL.md`), and each
/// reason below names the one that fired. A rejection no rule accounts for
/// belongs in the importer or in the plan, never here.
const EXPECTED_UNPARSEABLE: &[(&str, &str)] = &[
    (
        "lumina-story-planning-round-2.md",
        "task ids `13a` / `13b` are not integers — inserted between 13 and 14 \
         instead of renumbering what followed, and the plan has since landed",
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

/// Every `docs/plans/*.md` that survives `SKIP_SUFFIXES` and carries a
/// numbered task heading INSIDE its `## Tasks` section. That is what separates
/// a house-format plan from the older `#### T1:` ad-hoc shape and from a
/// design brief whose Tasks section is prose. The section scoping is
/// load-bearing: a brief numbering headings elsewhere imports to no task at
/// all, which is an error-class finding over a document nobody meant to
/// include.
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
        if has_task_in_tasks_section(&source) {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Whether `source` carries a numbered task heading under its own `## Tasks`
/// heading. A `## ` line other than that one closes the section, so a
/// numbered heading anywhere else in the document counts for nothing.
fn has_task_in_tasks_section(source: &str) -> bool {
    let mut in_tasks = false;
    let mut has_task = false;
    for line in source.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match line.strip_prefix("## ") {
            Some(title) => in_tasks = title.trim().eq_ignore_ascii_case("Tasks"),
            None => has_task |= in_tasks && is_numbered_task_heading(line),
        }
    }
    has_task
}

/// `^#{3,6} [0-9]+\. `, spelled out. Hand-rolled rather than compiled: the
/// binary resolves `regex` without its unicode features and a test build
/// unifies them back on, so a pattern here would be checked under settings the
/// shipped binary does not use.
///
/// The depth run mirrors the importer's own heading grammar. A narrower run
/// here would drop a plan out of scope that the importer accepts, and a plan
/// out of scope is one this test never dry-runs.
fn is_numbered_task_heading(line: &str) -> bool {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if !(3..=6).contains(&hashes) {
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
         ({SKIP_SUFFIXES:?} + a numbered task heading inside `## Tasks`) \
         matches nothing, so this test would pass vacuously"
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

/// The filter decides which documents the pass above dry-runs, and a design
/// brief numbering headings outside a `## Tasks` section imports to no task at
/// all — an error-class finding over a document nobody meant to include. The
/// cases are staged rather than drawn from the corpus, so the distinction
/// stays asserted whether or not a live plan happens to draw it.
#[test]
fn the_scope_filter_reads_only_the_tasks_section() {
    const IN_SCOPE: &str = "## Tasks\n\n### 1. Do the thing [S]\n- **Action**: Do it.\n";
    let staged = tempfile::tempdir().expect("tempdir");

    for (name, body) in [
        ("in-scope.md", IN_SCOPE),
        // Numbered headings on both sides of the section and none inside it.
        (
            "brief.md",
            "## Approach\n\n### 1. Ahead of the section [S]\n\n## Tasks\n\nProse.\n\n\
             ## Risks\n\n### 2. Past the section [S]\n",
        ),
        ("sectionless.md", "## Approach\n\n### 1. Do the thing [S]\n"),
        ("companion.premerge.md", IN_SCOPE),
        ("notes.txt", IN_SCOPE),
    ] {
        fs::write(staged.path().join(name), body).expect("plan written");
    }

    assert_eq!(
        in_scope_plans(staged.path())
            .iter()
            .map(|plan| file_name(plan))
            .collect::<Vec<_>>(),
        vec!["in-scope.md".to_string()]
    );
}
