//! Integration tests for `tomlctl tasks render`.
//!
//! `house-plan.rendered.md` is what rendering `house-plan.tasks.toml` into
//! `house-plan.md` must produce, byte for byte. It legitimately differs from
//! the source plan inside the three owned sections — task 4's `Blocked-by`
//! renders as `Depends on`, the three checkpoint marker shapes collapse to
//! one, `Depends on` carries `needs ∪ coupling` with the note
//! re-parenthesised, the phase-label subheadings and the `(new)` file suffixes
//! are not store-backed — and must not differ by one byte outside them.
//!
//! Both fixtures are pinned LF by `tomlctl/.gitattributes`: the golden is
//! byte-compared against a stdout preview and an on-disk write, and the CRLF
//! case is built here from the golden's own bytes, so a CRLF working-tree copy
//! would leave that test comparing a document against itself.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_PLAN: &str = include_str!("fixtures/tasks/house-plan.md");
const FIXTURE_STORE: &str = include_str!("fixtures/tasks/house-plan.tasks.toml");
const GOLDEN_PLAN: &str = include_str!("fixtures/tasks/house-plan.rendered.md");

const SLUG: &str = "house-plan-fixture";
const PLAN_REL: &str = "docs/plans/house-plan.md";

/// The first heading of the owned run and the first heading past it. Every
/// byte before the one and from the other on is the renderer's to preserve.
const FIRST_OWNED: &str = "## Execution Policy";
const AFTER_OWNED: &str = "## Risks";

fn context(plan_path: &str) -> String {
    format!(
        "schema_version = 1\nlast_updated = 2026-09-08\nslug = \"{SLUG}\"\n\
         status = \"in-progress\"\nplan_path = \"{plan_path}\"\n"
    )
}

/// Stage a flow tree under a fresh tempdir:
///   `<root>/.claude/flows/<SLUG>/{context.toml, tasks.toml}`
///   `<root>/docs/plans/house-plan.md`   (the path `context.toml` records)
///
/// The store is the committed golden, so the render under test reads exactly
/// the bytes `tasks_import` proves an import produces. The root is
/// canonicalised because the binary canonicalises `TOMLCTL_ROOT`, and the
/// containment check compares canonical paths.
fn stage(plan_body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical root");

    let flow_dir = root.join(".claude").join("flows").join(SLUG);
    fs::create_dir_all(&flow_dir).expect("flow dir");
    fs::write(flow_dir.join("context.toml"), context(PLAN_REL)).expect("context written");
    fs::write(flow_dir.join("tasks.toml"), FIXTURE_STORE).expect("store written");

    write_plan(&root, plan_body);
    (dir, root)
}

fn write_plan(root: &Path, body: &str) {
    let path = plan_path(root);
    fs::create_dir_all(path.parent().expect("plan has a parent")).expect("plans dir");
    fs::write(path, body).expect("plan written");
}

fn plan_path(root: &Path) -> PathBuf {
    root.join("docs").join("plans").join("house-plan.md")
}

fn context_path(root: &Path) -> PathBuf {
    root.join(".claude")
        .join("flows")
        .join(SLUG)
        .join("context.toml")
}

fn plan_bytes(root: &Path) -> Vec<u8> {
    fs::read(plan_path(root)).expect("the plan is on disk")
}

fn cli(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("tomlctl").expect("the binary builds");
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5");
    cmd
}

/// `tasks render <args…>` under `--slug`, with the caller's expected exit
/// code, returning `(stdout, stderr)`.
fn render(root: &Path, args: &[&str], code: i32) -> (String, String) {
    let assert = cli(root)
        // The structured envelope is opt-in; the default stderr format is
        // prose, and asserting on prose would pin a message rather than a kind.
        .args(["--error-format", "json"])
        .args(["tasks", "render", "--slug", SLUG])
        .args(args)
        .write_stdin("")
        .assert()
        .code(code);
    let out = assert.get_output();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn json_of(text: &str) -> serde_json::Value {
    serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("expected one JSON line: {e}; got: {text}"))
}

/// The first differing line, so a golden mismatch is diagnosable without
/// eyeballing two 100-line blobs. Std only — the crate carries no
/// `pretty_assertions` dev-dependency.
fn first_line_diff(got: &str, want: &str) -> String {
    for (index, (a, b)) in got.lines().zip(want.lines()).enumerate() {
        if a != b {
            return format!("line {}:\n  got:  {a:?}\n  want: {b:?}", index + 1);
        }
    }
    format!(
        "line counts differ: got {} lines, want {}",
        got.lines().count(),
        want.lines().count()
    )
}

fn assert_matches_golden(got: &str) {
    if got != GOLDEN_PLAN {
        panic!("{}", first_line_diff(got, GOLDEN_PLAN));
    }
}

fn head_of(doc: &str) -> &str {
    let at = doc.find(FIRST_OWNED).expect("the owned run opens the plan");
    &doc[..at]
}

fn tail_of(doc: &str) -> &str {
    let at = doc.find(AFTER_OWNED).expect("a section closes the plan");
    &doc[at..]
}

fn finding_classes(envelope: &serde_json::Value) -> Vec<String> {
    envelope
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("envelope must carry `findings`: {envelope}"))
        .iter()
        .map(|finding| {
            finding
                .get("class")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

fn sidecar_of(file: &Path) -> PathBuf {
    let mut raw = file.as_os_str().to_os_string();
    raw.push(".sha256");
    PathBuf::from(raw)
}

/// GOLDEN: the fixture store renders into the fixture plan to the committed
/// bytes, and re-rendering its own output changes nothing. A marker whose
/// closure moved, a dropped parenthetical, a collapsed `Blocked-by` or a
/// separator that grew a space all land on this one comparison.
#[test]
fn a_render_matches_the_golden_rendered_plan() {
    let (_dir, root) = stage(FIXTURE_PLAN);

    let (stdout, _) = render(&root, &[], 0);
    let envelope = json_of(&stdout);
    assert_eq!(envelope.get("ok"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        envelope.get("path").and_then(serde_json::Value::as_str),
        Some(PLAN_REL),
        "{envelope}"
    );
    assert_eq!(
        envelope
            .get("sections")
            .and_then(serde_json::Value::as_array)
            .map(|s| s.len()),
        Some(3),
        "{envelope}"
    );

    let written = fs::read_to_string(plan_path(&root)).expect("the plan is on disk");
    assert_matches_golden(&written);
    assert!(
        !sidecar_of(&plan_path(&root)).exists(),
        "a derived write must leave no integrity sidecar"
    );

    // The render is the fixed point of itself, so the golden cannot drift a
    // byte per pass the way an accumulating marker would.
    render(&root, &[], 0);
    assert_matches_golden(&fs::read_to_string(plan_path(&root)).expect("the plan is on disk"));
}

/// `--stdout` is a preview: the same bytes, and nothing on disk.
#[test]
fn stdout_previews_the_render_and_writes_no_bytes() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    let before = plan_bytes(&root);

    let (stdout, _) = render(&root, &["--stdout"], 0);
    assert_matches_golden(&stdout);
    assert_eq!(
        plan_bytes(&root),
        before,
        "`--stdout` must not rewrite the plan"
    );
    assert!(!sidecar_of(&plan_path(&root)).exists());
}

/// `--check` is a report, not a repair: exit 1 with the drift finding, and
/// the drifted plan left exactly as it was.
#[test]
fn check_reports_drift_and_repairs_nothing() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    let before = plan_bytes(&root);

    let (stdout, _) = render(&root, &["--check"], 1);
    let envelope = json_of(&stdout);
    assert_eq!(envelope.get("ok"), Some(&serde_json::Value::Bool(false)));
    assert_eq!(
        finding_classes(&envelope),
        vec!["render/drift".to_string()],
        "{envelope}"
    );
    let detail = envelope["findings"][0]["detail"]
        .as_str()
        .unwrap_or_default();
    for section in ["Execution Policy", "Tasks", "Dependency Graph"] {
        assert!(detail.contains(section), "{detail}");
    }

    assert_eq!(
        plan_bytes(&root),
        before,
        "`--check` must not repair the plan it reports on"
    );
}

/// Line endings are not drift. The comparison folds CRLF before matching, so
/// a Windows checkout of an up-to-date plan reports clean — and keeps its
/// endings, since `--check` writes nothing at all.
#[test]
fn check_passes_on_a_crlf_copy_of_the_render() {
    let crlf = GOLDEN_PLAN.replace('\n', "\r\n");
    assert!(
        crlf.len() > GOLDEN_PLAN.len(),
        "the committed golden is already CRLF — the `text eol=lf` pin is not holding"
    );

    let (_dir, root) = stage(&crlf);
    let (stdout, _) = render(&root, &["--check"], 0);
    let envelope = json_of(&stdout);
    assert_eq!(envelope.get("ok"), Some(&serde_json::Value::Bool(true)));
    assert!(finding_classes(&envelope).is_empty(), "{envelope}");

    assert_eq!(
        plan_bytes(&root),
        crlf.as_bytes(),
        "`--check` must leave the endings it read"
    );
}

/// The renderer owns three sections and nothing else, so the hand-authored
/// header, `## Context` and `## Risks` survive the write byte for byte.
#[test]
fn bytes_outside_the_owned_sections_survive_the_write() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    render(&root, &[], 0);
    let written = fs::read_to_string(plan_path(&root)).expect("the plan is on disk");

    assert_eq!(head_of(&written), head_of(FIXTURE_PLAN));
    assert_eq!(tail_of(&written), tail_of(FIXTURE_PLAN));
    assert!(
        head_of(FIXTURE_PLAN).contains("## Context") && tail_of(FIXTURE_PLAN).len() > 1,
        "the preserved run is empty — the assertion above proves nothing"
    );

    // Without this the pair above would also pass on a renderer that wrote
    // the source back unchanged.
    assert_ne!(written, FIXTURE_PLAN, "the owned sections did not change");
}

/// `plan_path` is file-controlled input and the write runs no guard of its
/// own, so an escaping value is refused before the plan is read — leaving both
/// the plan and the path it pointed at untouched.
#[test]
fn an_escaping_plan_path_is_refused_with_no_write() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    let before = plan_bytes(&root);
    let escape = root
        .parent()
        .expect("the tempdir has a parent")
        .join("escape.md");

    // `/etc/passwd` is the absolute case: `Path::is_absolute` is false for a
    // rootless path on Windows, so containment rather than the lexical scan
    // is what refuses it there.
    for recorded in ["../escape.md", "docs/../../escape.md", "/etc/passwd"] {
        fs::write(context_path(&root), context(recorded)).expect("context rewritten");

        let (_, stderr) = render(&root, &[], 1);
        let error = json_of(&stderr)
            .get("error")
            .cloned()
            .unwrap_or_else(|| panic!("stderr must be an error envelope: {stderr}"));
        assert_eq!(
            error.get("kind").and_then(serde_json::Value::as_str),
            Some("validation"),
            "{recorded}: {error}"
        );

        assert_eq!(plan_bytes(&root), before, "{recorded}");
        assert!(!escape.exists(), "{recorded}");
    }
}
