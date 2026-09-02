//! Integration tests for `tomlctl flow render-progress-log`.
//!
//! The renderer is a PURE function of `execution-record.toml` + the flow
//! title; these tests drive it through the binary's `--stdout` preview path
//! (no on-disk PROGRESS-LOG.md write) so the rendered bytes can be compared
//! directly. The golden triple lives in `tests/fixtures/render/` and is pinned
//! LF via `tomlctl/.gitattributes` so a Windows checkout can't flip the
//! expected bytes to CRLF.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_RECORD: &str = include_str!("fixtures/render/execution-record.toml");
const FIXTURE_CONTEXT: &str = include_str!("fixtures/render/context.toml");
const FIXTURE_PLAN: &str = include_str!("fixtures/render/render-fixture-plan.md");
const GOLDEN_PROGRESS_LOG: &str = include_str!("fixtures/render/PROGRESS-LOG.md");

const SLUG: &str = "harness-progressive-disclosure-wave-2";

/// Stage a flow tree under a fresh tempdir:
///   `<root>/.claude/flows/<SLUG>/{context.toml, execution-record.toml}`
///   `<root>/docs/plans/render-fixture-plan.md`   (matches context's plan_path)
/// Returns `(tempdir, root_path)`. The caller passes `root_path` to
/// `TOMLCTL_ROOT` so `flow render-progress-log --slug` resolves to the staged
/// tree and the title-resolution path finds the fixture plan.
fn stage_flow(record_body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let flow_dir = root.join(".claude").join("flows").join(SLUG);
    fs::create_dir_all(&flow_dir).unwrap();
    fs::write(flow_dir.join("context.toml"), FIXTURE_CONTEXT).unwrap();
    fs::write(flow_dir.join("execution-record.toml"), record_body).unwrap();
    let plan_dir = root.join("docs").join("plans");
    fs::create_dir_all(&plan_dir).unwrap();
    fs::write(plan_dir.join("render-fixture-plan.md"), FIXTURE_PLAN).unwrap();
    (dir, root)
}

/// Run `flow render-progress-log --slug <SLUG> --stdout <extra…>` against the
/// staged `root` and return raw stdout bytes (so byte-equality holds without
/// any lossy UTF-8 round-trip).
fn render_stdout(root: &Path, extra: &[&str]) -> Vec<u8> {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("render-progress-log")
        .arg("--slug")
        .arg(SLUG)
        .arg("--stdout");
    for a in extra {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    out.get_output().stdout.clone()
}

/// GOLDEN: the fixture record renders byte-for-byte to the committed
/// `PROGRESS-LOG.md`. This is the byte-identity contract — the renderer must
/// reproduce the real flow's progress log exactly (title from the plan's
/// `# Plan:` header, EM DASH H1, the three tables + Session Log).
#[test]
fn render_matches_golden_progress_log() {
    let (_dir, root) = stage_flow(FIXTURE_RECORD);
    let got = render_stdout(&root, &[]);
    let got_str = String::from_utf8_lossy(&got);
    // On mismatch, point at the FIRST differing line (std only — no
    // pretty_assertions dependency) so the failure is diagnosable without
    // eyeballing two large blobs.
    if got_str != GOLDEN_PROGRESS_LOG {
        panic!("{}", first_line_diff(&got_str, GOLDEN_PROGRESS_LOG));
    }
    // Byte-exact (catches any trailing-newline / line-ending drift the lossy
    // string compare above could mask).
    assert_eq!(
        got,
        GOLDEN_PROGRESS_LOG.as_bytes(),
        "rendered bytes must equal the golden bytes exactly"
    );
}

/// IDEMPOTENCY: render twice ⇒ identical bytes (the render is a pure function
/// of its inputs, which don't change between runs).
#[test]
fn render_is_idempotent() {
    let (_dir, root) = stage_flow(FIXTURE_RECORD);
    let first = render_stdout(&root, &[]);
    let second = render_stdout(&root, &[]);
    assert_eq!(first, second, "render-then-render must be byte-identical");
}

/// CROSS-REORDER: swapping two same-date entries in the source record does NOT
/// change the output. Guaranteed by the `(date asc, id asc)` pre-sort, the
/// count-based Session-Log `Changes` cell, and the lexicographic Commits union.
/// We reorder by physically moving the E2 `[[items]]` block to the end of the
/// record (all entries share the same `2026-05-20` date), then assert the
/// rendered bytes are unchanged.
#[test]
fn render_is_stable_under_same_date_reorder() {
    let (_dir, root) = stage_flow(FIXTURE_RECORD);
    let baseline = render_stdout(&root, &[]);

    let reordered = move_first_item_block_to_end(FIXTURE_RECORD);
    // Sanity: the reorder actually changed the source bytes (otherwise the test
    // would pass vacuously).
    assert_ne!(
        reordered, FIXTURE_RECORD,
        "reorder helper must have changed the source ordering"
    );
    let (_dir2, root2) = stage_flow(&reordered);
    let after = render_stdout(&root2, &[]);

    assert_eq!(
        baseline, after,
        "swapping same-date entries in the source must not change the render"
    );
}

/// EMPTY-STATE: a minimal record with NO task-completion entries renders the
/// Completed Items table as a single `(none)` row (first cell `(none)`, the
/// rest blank), per the established empty-state convention.
#[test]
fn render_empty_completed_table_shows_none_row() {
    // Minimal record: schema header + one non-task-completion entry so the
    // Completed Items query yields zero rows.
    let record = r#"schema_version = 1
last_updated = 2026-05-20

[[items]]
id = "E1"
type = "status-transition"
date = 2026-05-20
agent = "implement"
summary = "draft -> in-progress"
from_status = "draft"
to_status = "in-progress"
"#;
    let (_dir, root) = stage_flow(record);
    let got = String::from_utf8(render_stdout(&root, &[])).unwrap();

    // The Completed Items section must carry the `(none)` empty-state row.
    let completed_section = got
        .split("## Deviations")
        .next()
        .expect("output has a Completed Items section before Deviations");
    assert!(
        completed_section.contains("## Completed Items"),
        "output must contain the Completed Items header, got:\n{got}"
    );
    // Pin the EXACT empty-row shape for the 5-column Completed table —
    // `| (none) | | | | |` — in the Completed section specifically, rather than
    // the loose `| (none) |` substring that would match any table's empty row.
    assert!(
        completed_section.contains("| (none) | | | | |\n"),
        "empty Completed Items table must render the exact `| (none) | | | | |` row, got:\n{completed_section}"
    );
    // And the title fell through the plan header (the fixture context's plan is
    // staged) — confirm the H1 EM DASH form is present.
    assert!(
        got.contains("# Harness Progressive-Disclosure Wave 2 \u{2014} Progress Log"),
        "H1 title (EM DASH) must be present, got:\n{got}"
    );
}

/// WRITE-MODE ENVELOPE: without `--stdout` the command writes the sibling
/// `PROGRESS-LOG.md` (NO `.sha256` sidecar — it's a derived artifact) and emits
/// the `{"ok":true,"path":…,"tables":{…}}` envelope with the rendered row
/// counts (12 completed, 2 deviation chain-tips, 2 deferrals, 2 session
/// buckets — the fixture spans 2026-05-20 and 2026-05-21).
#[test]
fn render_write_mode_writes_file_without_sidecar_and_reports_counts() {
    let (_dir, root) = stage_flow(FIXTURE_RECORD);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("render-progress-log")
        .arg("--slug")
        .arg(SLUG)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let env: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("write-mode stdout must be a JSON envelope");
    assert_eq!(env.get("ok").and_then(|v| v.as_bool()), Some(true));
    let tables = env.get("tables").expect("envelope has tables");
    assert_eq!(tables.get("completed").and_then(|v| v.as_u64()), Some(12));
    assert_eq!(tables.get("deviations").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(tables.get("deferrals").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(tables.get("sessions").and_then(|v| v.as_u64()), Some(2));

    // The file landed next to the execution record…
    let progress = root
        .join(".claude")
        .join("flows")
        .join(SLUG)
        .join("PROGRESS-LOG.md");
    assert!(progress.exists(), "PROGRESS-LOG.md must be written");
    // …and matches the golden bytes.
    let on_disk = fs::read(&progress).unwrap();
    assert_eq!(
        on_disk,
        GOLDEN_PROGRESS_LOG.as_bytes(),
        "written PROGRESS-LOG.md must equal the golden bytes"
    );
    // …and NO sidecar was written (derived artifact).
    let sidecar = {
        let mut s = progress.into_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    assert!(
        !sidecar.exists(),
        "no .sha256 sidecar must be written for the derived PROGRESS-LOG.md"
    );
}

/// HELP SNAPSHOT: `flow render-progress-log --help` lists the three flags the
/// later doc tasks reference verbatim — `--slug`, `--stdout`,
/// `--verify-integrity` (and the verb kebab-cases as `render-progress-log`).
#[test]
fn render_help_lists_expected_flags() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("flow")
        .arg("render-progress-log")
        .arg("--help")
        .assert()
        .success();
    let help = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        help.contains("--slug"),
        "help must list --slug, got:\n{help}"
    );
    assert!(
        help.contains("--stdout"),
        "help must list --stdout, got:\n{help}"
    );
    assert!(
        help.contains("--verify-integrity"),
        "help must list --verify-integrity, got:\n{help}"
    );
}

/// Security: a `--slug` carrying path-traversal components is REJECTED by
/// the strict slug validator BEFORE any path is resolved or read/written. The
/// command must fail (non-zero) with a validation error and leave NO stray file
/// behind anywhere under the staged tree.
#[test]
fn render_rejects_traversal_slug() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    // A would-be escape target: `<root>/escape-marker.md`. The traversal slug
    // `../escape-marker` from `.claude/flows/<slug>/PROGRESS-LOG.md` would, if
    // unvalidated, resolve a write outside the flow dir.
    let escape_target = root.join("escape-marker.md");

    let assert = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("render-progress-log")
        .arg("--slug")
        .arg("../escape-marker")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("invalid slug"),
        "traversal slug must fail with a validation error, got stderr:\n{stderr}"
    );
    // No stray file was written outside the (absent) flow dir.
    assert!(
        !escape_target.exists(),
        "a rejected traversal slug must not write any file outside the flow dir"
    );
    // And `escape-marker.toml` / `.md` siblings were not seeded either.
    assert!(
        !root.join("escape-marker").exists(),
        "no stray escape-marker artifact may be created"
    );
}

/// `--verify-integrity` against a record whose `.sha256` sidecar
/// MISMATCHES the file bytes fails fast (before rendering); WITHOUT the flag the
/// same record renders normally (the integrity check is opt-in).
#[test]
fn render_verify_integrity_mismatch_fails_but_renders_without_flag() {
    let (_dir, root) = stage_flow(FIXTURE_RECORD);
    // Plant a well-formed-but-wrong sidecar (64 hex zeros) next to the record.
    let record = root
        .join(".claude")
        .join("flows")
        .join(SLUG)
        .join("execution-record.toml");
    let sidecar = {
        let mut s = record.into_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    fs::write(
        &sidecar,
        format!("{}  execution-record.toml\n", "0".repeat(64)),
    )
    .unwrap();

    // WITH --verify-integrity → hard fail on the digest mismatch.
    let assert = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", &root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("render-progress-log")
        .arg("--slug")
        .arg(SLUG)
        .arg("--stdout")
        .arg("--verify-integrity")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("integrity"),
        "mismatched sidecar must fail the integrity check, got stderr:\n{stderr}"
    );

    // WITHOUT the flag → renders fine despite the bogus sidecar.
    let got = render_stdout(&root, &[]);
    assert_eq!(
        got,
        GOLDEN_PROGRESS_LOG.as_bytes(),
        "without --verify-integrity the record renders the golden bytes"
    );
}

/// The title-cased-slug FALLBACK. When the flow's plan file is absent
/// (so no `# Plan:` header can be read), the H1 falls through to the
/// title-cased slug. We stage a flow WITHOUT the plan file and assert the H1 is
/// the title-cased SLUG, not a plan-derived title.
#[test]
fn render_falls_back_to_titlecased_slug_when_plan_absent() {
    // Stage the flow tree but DELETE the plan file the fixture context points
    // at, forcing the slug fallback.
    let (_dir, root) = stage_flow(FIXTURE_RECORD);
    let plan = root
        .join("docs")
        .join("plans")
        .join("render-fixture-plan.md");
    fs::remove_file(&plan).unwrap();

    let got = String::from_utf8(render_stdout(&root, &[])).unwrap();
    // `harness-progressive-disclosure-wave-2` → `Harness Progressive-Disclosure
    // Wave 2` (split on `-`, title-case each token; the embedded hyphen in
    // `progressive-disclosure` splits into two tokens). EM DASH H1 form.
    assert!(
        got.contains("# Harness Progressive Disclosure Wave 2 \u{2014} Progress Log\n"),
        "absent plan must fall back to the title-cased slug H1, got:\n{got}"
    );
}

/// A FORKED supersession chain — two deviation entries that re-point at the
/// SAME predecessor — collapses to a SINGLE rendered head (the latest by
/// `(date, id)`), not both. Drives `--stdout` against a hand-built record so the
/// fork case is exercised independently of the golden fixture's linear chain.
#[test]
fn render_deviation_fork_collapses_to_latest_tip() {
    // E1 is the common predecessor; E2 and E3 both supersede it (a fork). The
    // latest by (date asc, id asc) is E3 (later date) — only E3 must render.
    let record = r#"schema_version = 1
last_updated = 2026-05-22

[[items]]
id = "E1"
type = "deviation"
date = 2026-05-20
agent = "implement"
summary = "Original deviation at the fork root"
original_intent = "intent-0"
rationale = "rationale-0"
commits = ["aaa0000"]

[[items]]
id = "E2"
type = "deviation"
date = 2026-05-21
agent = "implement"
summary = "Fork arm A superseding the root"
original_intent = "intent-A"
rationale = "rationale-A"
commits = ["bbb1111"]
supersedes_entry = "E1"

[[items]]
id = "E3"
type = "deviation"
date = 2026-05-22
agent = "implement"
summary = "Fork arm B superseding the root (latest)"
original_intent = "intent-B"
rationale = "rationale-B"
commits = ["ccc2222"]
supersedes_entry = "E1"
"#;
    let (_dir, root) = stage_flow(record);
    let got = String::from_utf8(render_stdout(&root, &[])).unwrap();
    let deviations_section = got
        .split("## Deferrals")
        .next()
        .and_then(|s| s.split("## Deviations").nth(1))
        .expect("output has a Deviations section");
    // Only the latest fork tip (E3) renders as a DATA ROW (its `#` cell opens
    // the row); the root E1 and the losing arm E2 are pruned. We anchor on the
    // first-column (`| <id> | <summary>`) row-start form — `| E1 |` ALSO appears
    // in E3's `Supersedes` cell (correct provenance: E3's immediate predecessor
    // is E1), so a bare `| E1 |` substring would false-match.
    assert!(
        deviations_section.contains("| E3 | Fork arm B superseding the root (latest) |"),
        "the latest fork tip E3 must render as a data row, got:\n{deviations_section}"
    );
    assert!(
        !deviations_section.contains("| E1 | Original deviation at the fork root |"),
        "the superseded fork root E1 must not render as a data row, got:\n{deviations_section}"
    );
    assert!(
        !deviations_section.contains("| E2 | Fork arm A superseding the root |"),
        "the losing fork arm E2 must collapse away, got:\n{deviations_section}"
    );
    // E3's `Supersedes` cell correctly cites its immediate predecessor E1 even
    // though E1's row was pruned (dangling-after-prune provenance).
    assert!(
        deviations_section.contains("| rationale-B | E1 |"),
        "the rendered tip must cite its immediate predecessor in Supersedes, got:\n{deviations_section}"
    );
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Build a human-readable diagnostic naming the FIRST line index where
/// `got` and `want` differ (1-based), with both lines quoted, plus a
/// total-line-count note. Pure std — deliberately avoids a pretty_assertions
/// dependency. Used by the golden test so a render drift reports *where* it
/// diverged rather than dumping two opaque blobs.
fn first_line_diff(got: &str, want: &str) -> String {
    let got_lines: Vec<&str> = got.lines().collect();
    let want_lines: Vec<&str> = want.lines().collect();
    let max = got_lines.len().max(want_lines.len());
    for i in 0..max {
        let g = got_lines.get(i);
        let w = want_lines.get(i);
        if g != w {
            return format!(
                "rendered markdown differs from the golden PROGRESS-LOG.md at line {} \
                 (got {} lines, want {} lines):\n  got:  {:?}\n  want: {:?}",
                i + 1,
                got_lines.len(),
                want_lines.len(),
                g.unwrap_or(&"<EOF>"),
                w.unwrap_or(&"<EOF>"),
            );
        }
    }
    // Lines all match but the strings differ → a trailing-newline / whitespace
    // difference the line walk can't see.
    format!(
        "rendered markdown differs from the golden PROGRESS-LOG.md only in trailing \
         whitespace / final newline (got {} bytes, want {} bytes)",
        got.len(),
        want.len(),
    )
}

/// Move the FIRST `[[items]]` block of a TOML execution record to the END of
/// the file, preserving the leading `schema_version`/`last_updated` preamble.
/// Used by the cross-reorder test to physically reshuffle two same-date entries
/// in the source. Operates on whole-block boundaries (`[[items]]` … up to the
/// next `[[items]]` or EOF) so the result stays valid TOML.
fn move_first_item_block_to_end(record: &str) -> String {
    let marker = "[[items]]";
    let first = record
        .find(marker)
        .expect("record has at least one [[items]] block");
    let preamble = &record[..first];
    let rest = &record[first..];
    // Split `rest` into its [[items]] blocks. The first split element is empty
    // (rest starts with the marker), so skip it; each block is `[[items]]` +
    // its body up to the next marker.
    let mut blocks: Vec<String> = rest
        .split(marker)
        .filter(|s| !s.is_empty())
        .map(|body| format!("{marker}{body}"))
        .collect();
    assert!(
        blocks.len() >= 2,
        "need at least two item blocks to reorder"
    );
    let head = blocks.remove(0);
    blocks.push(head);
    format!("{preamble}{}", blocks.join(""))
}
