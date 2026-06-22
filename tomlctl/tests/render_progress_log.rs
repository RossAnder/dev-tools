//! T3: integration tests for `tomlctl flow render-progress-log`
//! (`docs/plans/flow-tracking-overhaul.md`-adjacent file-auto-creation plan,
//! Phase 1).
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
    assert_eq!(
        String::from_utf8_lossy(&got),
        GOLDEN_PROGRESS_LOG,
        "rendered markdown must equal the committed PROGRESS-LOG.md byte-for-byte"
    );
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
    assert!(
        completed_section.contains("| (none) |"),
        "empty Completed Items table must render a `(none)` row, got:\n{completed_section}"
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
/// counts (12 completed, 0 deviations, 0 deferrals, 1 session bucket).
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
    assert_eq!(tables.get("deviations").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(tables.get("deferrals").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(tables.get("sessions").and_then(|v| v.as_u64()), Some(1));

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
    assert!(help.contains("--slug"), "help must list --slug, got:\n{help}");
    assert!(
        help.contains("--stdout"),
        "help must list --stdout, got:\n{help}"
    );
    assert!(
        help.contains("--verify-integrity"),
        "help must list --verify-integrity, got:\n{help}"
    );
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

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
