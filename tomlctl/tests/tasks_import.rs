//! Integration tests for `tomlctl tasks import-plan`.
//!
//! The fixture plan under `tests/fixtures/tasks/` is a house-format document
//! carrying every grammar variant the importer claims: h3 and h4 numbered
//! headings under phase labels, wrapped `Files` and `Depends on` lines with
//! parentheticals, a `Blocked-by` synonym, an em-dash title, a task with no
//! edge in either direction, a file shared by two tasks that a dependency
//! already orders, three checkpoint marker shapes, and one body carrying a
//! backslash. `house-plan.tasks.toml` is what importing it must produce.
//!
//! `house-plan.rendered.md` is the renderer's output over that same store, and
//! importing it must land on the same bytes again — the leg that closes plan →
//! store → plan → store. Without it each half is byte-pinned on its own while
//! the renderer stays free to emit a document its own importer reads
//! differently.
//!
//! Both fixtures are pinned LF by `tomlctl/.gitattributes`: the store is
//! byte-compared against a writer that always emits `\n`, and the CRLF case is
//! built here from the plan's own bytes, so a CRLF working-tree copy would
//! leave that test comparing a document against itself.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_PLAN: &str = include_str!("fixtures/tasks/house-plan.md");
const GOLDEN_STORE: &str = include_str!("fixtures/tasks/house-plan.tasks.toml");
const RENDERED_PLAN: &str = include_str!("fixtures/tasks/house-plan.rendered.md");

const SLUG: &str = "house-plan-fixture";
const PLAN_REL: &str = "docs/plans/house-plan.md";

/// The date the golden carries; every actual store is normalised onto it
/// before the compare, and `last_updated_is_a_bare_date` is what still holds
/// the writer to stamping one.
const GOLDEN_DATE: &str = "last_updated = 2026-09-08";

const FIXTURE_CONTEXT: &str = r#"schema_version = 1
last_updated = 2026-09-08
slug = "house-plan-fixture"
status = "in-progress"
plan_path = "docs/plans/house-plan.md"
"#;

/// Two completions this plan produces and one it does not. `E1` matches task
/// 1's derived ref exactly; `E2` spells task 4's `max_parallel` with a hyphen,
/// so only the normalised matcher joins them and adoption is observable.
const FIXTURE_RECORD: &str = r#"schema_version = 1
last_updated = 2026-09-08

[[items]]
id = "E1"
type = "task-completion"
date = 2026-09-08
agent = "implement-lite"
task_ref = "scaffold-the-module-tree"
status = "done"
summary = "Scaffolded the module tree."

[[items]]
id = "E2"
type = "task-completion"
date = 2026-09-08
agent = "implement-lite"
task_ref = "parse-the-policy-bullets-and-the-max-parallel-range"
status = "done"
summary = "Parsed the four bullets."

[[items]]
id = "E3"
type = "task-completion"
date = 2026-09-08
agent = "implement-deep"
task_ref = "a-task-from-another-plan"
status = "done"
summary = "Belongs to a different flow."
"#;

/// A record on the sibling filename accounting for a task this plan does not
/// carry, so a resolver that ignored an `[artifacts]` override would land on a
/// different adopted-and-unmatched pair rather than the same one.
const DECOY_RECORD: &str = r#"schema_version = 1
last_updated = 2026-09-08

[[items]]
id = "E1"
type = "task-completion"
date = 2026-09-08
agent = "implement-lite"
task_ref = "a-task-the-override-hides"
status = "done"
summary = "Only the sibling filename carries this."
"#;

/// Stage a flow tree under a fresh tempdir:
///   `<root>/.claude/flows/<SLUG>/{context.toml, execution-record.toml}`
///   `<root>/docs/plans/house-plan.md`   (the path `context.toml` records)
///
/// The record is staged unconditionally, so every import that does NOT pass
/// `--reconcile-record` is also an assertion that it left the record alone.
/// The root is canonicalised because the binary canonicalises `TOMLCTL_ROOT`,
/// and a path assertion against the uncanonicalised form would drift on a
/// platform with a short temp path.
fn stage(plan_body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical root");

    let flow_dir = root.join(".claude").join("flows").join(SLUG);
    fs::create_dir_all(&flow_dir).expect("flow dir");
    fs::write(flow_dir.join("context.toml"), FIXTURE_CONTEXT).expect("context written");
    fs::write(flow_dir.join("execution-record.toml"), FIXTURE_RECORD).expect("record written");

    write_plan(&root, plan_body);
    (dir, root)
}

fn write_plan(root: &Path, body: &str) {
    let path = root.join("docs").join("plans").join("house-plan.md");
    fs::create_dir_all(path.parent().expect("plan has a parent")).expect("plans dir");
    fs::write(path, body).expect("plan written");
}

fn flow_dir(root: &Path) -> PathBuf {
    root.join(".claude").join("flows").join(SLUG)
}

/// The staged context with `extra` appended, which is where an `[artifacts]`
/// table has to go: a table header ends the top-level key run.
fn write_context(root: &Path, extra: &str) {
    fs::write(
        flow_dir(root).join("context.toml"),
        format!("{FIXTURE_CONTEXT}{extra}"),
    )
    .expect("context written");
}

fn store_path(root: &Path) -> PathBuf {
    root.join(".claude")
        .join("flows")
        .join(SLUG)
        .join("tasks.toml")
}

fn cli(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("tomlctl").expect("the binary builds");
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5");
    cmd
}

/// `tasks import-plan <args…>`, required to succeed, stdout parsed as the
/// import envelope.
fn import(root: &Path, args: &[&str]) -> serde_json::Value {
    let out = cli(root)
        .arg("tasks")
        .arg("import-plan")
        .args(args)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("import stdout must be JSON: {e}; got: {stdout}"))
}

/// `tasks import-plan <args…>`, required to fail, stderr parsed as the error
/// envelope `{"error":{kind,message,file}}`.
fn import_err(root: &Path, args: &[&str]) -> serde_json::Value {
    let out = cli(root)
        // The structured envelope is opt-in; the default stderr format is
        // prose, and asserting on prose would pin a message rather than a kind.
        .args(["--error-format", "json"])
        .arg("tasks")
        .arg("import-plan")
        .args(args)
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let envelope: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr must be a JSON error envelope: {e}; got: {stderr}"));
    envelope
        .get("error")
        .cloned()
        .unwrap_or_else(|| panic!("envelope must carry `error`: {envelope}"))
}

fn counts(envelope: &serde_json::Value) -> (u64, u64, u64) {
    let field = |key: &str| {
        envelope
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| panic!("envelope must carry a numeric `{key}`: {envelope}"))
    };
    (field("added"), field("updated"), field("unchanged"))
}

fn refs(envelope: &serde_json::Value, key: &str) -> Vec<String> {
    envelope
        .get(key)
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("envelope must carry an array `{key}`: {envelope}"))
        .iter()
        .map(|entry| entry.as_str().unwrap_or_default().to_string())
        .collect()
}

fn finding_classes(envelope: &serde_json::Value) -> Vec<String> {
    findings_of(envelope, |_| true)
}

/// Classes of the `error`-severity findings alone. Only these gate the write,
/// so a run that must be judged on whether it was refused is judged on this
/// rather than on the whole list.
fn error_finding_classes(envelope: &serde_json::Value) -> Vec<String> {
    findings_of(envelope, |finding| {
        finding.get("severity").and_then(serde_json::Value::as_str) == Some("error")
    })
}

fn findings_of(
    envelope: &serde_json::Value,
    keep: impl Fn(&serde_json::Value) -> bool,
) -> Vec<String> {
    envelope
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("envelope must carry `findings`: {envelope}"))
        .iter()
        .filter(|finding| keep(finding))
        .map(|finding| {
            finding
                .get("class")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

/// The store as written, with its `last_updated` stamp rewritten to the
/// golden's. Every mutation restamps it from the clock, so it is the one line
/// a committed golden cannot carry.
fn normalised_store(root: &Path) -> String {
    let text = fs::read_to_string(store_path(root)).expect("the store is on disk");
    let mut hit = false;
    let out: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with("last_updated = ") {
                hit = true;
                GOLDEN_DATE.to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    assert!(
        hit,
        "the written store carries no `last_updated` line:\n{text}"
    );
    format!("{}\n", out.join("\n"))
}

/// The first differing line, so a golden mismatch is diagnosable without
/// eyeballing two 200-line blobs. Std only — the crate carries no
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

fn assert_matches_golden(root: &Path) {
    let got = normalised_store(root);
    if got != GOLDEN_STORE {
        panic!("{}", first_line_diff(&got, GOLDEN_STORE));
    }
}

/// GOLDEN: the fixture plan imports to the committed store, byte for byte.
/// Every grammar variant the fixture carries is asserted through this one
/// comparison — a dropped parenthetical, a mis-sliced em-dash title, a
/// membership that moved group, or an escaped backslash all land here.
#[test]
fn a_fresh_import_matches_the_golden_store() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    let envelope = import(&root, &["--slug", SLUG]);

    assert_eq!(counts(&envelope), (10, 0, 0), "{envelope}");
    assert_eq!(envelope.get("ok"), Some(&serde_json::Value::Bool(true)));
    assert!(finding_classes(&envelope).is_empty(), "{envelope}");
    assert_eq!(refs(&envelope, "added_refs").len(), 10, "{envelope}");
    assert!(refs(&envelope, "removed_refs").is_empty(), "{envelope}");
    // The record is staged but the flag was not passed, so nothing adopted.
    assert!(refs(&envelope, "adopted_refs").is_empty(), "{envelope}");

    assert_matches_golden(&root);
}

/// GOLDEN, closing leg: importing the renderer's own output reproduces the
/// store it was rendered from, byte for byte. A task heading flattened back to
/// one depth, a phase label the render stopped emitting, a `Files` line the
/// parser now splits elsewhere — each leaves both byte-goldens internally
/// consistent and only this comparison sees them stop describing one document.
#[test]
fn importing_the_rendered_plan_reproduces_the_golden_store() {
    // Were the two documents identical this would restate the fresh-import
    // test under another name and pin nothing about the renderer.
    assert_ne!(
        RENDERED_PLAN, FIXTURE_PLAN,
        "the rendered golden is a copy of the source plan"
    );

    let (_dir, root) = stage(RENDERED_PLAN);
    let envelope = import(&root, &["--slug", SLUG]);

    assert_eq!(counts(&envelope), (10, 0, 0), "{envelope}");
    assert!(finding_classes(&envelope).is_empty(), "{envelope}");

    assert_matches_golden(&root);
}

/// The one line the golden cannot pin, pinned separately: a bare TOML date,
/// unquoted, which is what `--verify-integrity` readers and `flow doctor`
/// both expect of a seeded flow artefact.
#[test]
fn the_written_store_stamps_a_bare_date() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);

    let text = fs::read_to_string(store_path(&root)).expect("the store is on disk");
    let stamp = text
        .lines()
        .find(|line| line.starts_with("last_updated = "))
        .expect("the store carries a `last_updated` line");
    let value = stamp.trim_start_matches("last_updated = ");
    assert_eq!(
        value.len(),
        10,
        "expected a bare `YYYY-MM-DD`, got {stamp:?}"
    );
    assert!(
        value
            .chars()
            .enumerate()
            .all(|(at, ch)| if at == 4 || at == 7 {
                ch == '-'
            } else {
                ch.is_ascii_digit()
            }),
        "expected a bare `YYYY-MM-DD`, got {stamp:?}"
    );
}

/// A re-import is a no-op on content and keeps every field an execution
/// wrote — the plan document can express none of `status`, `agent`, `commit`.
/// Setting one row in flight is what the graph reads as a stall, so the clean
/// bill this asserts is at error severity: a warning is a diagnostic about the
/// run in progress, and gating a re-import on one would refuse every store
/// with work under way.
#[test]
fn a_second_import_changes_no_content_and_keeps_execution_state() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);

    cli(&root)
        .args(["tasks", "update", "5", "--slug", SLUG])
        .args(["--status", "in-progress", "--agent", "implement-deep"])
        .args(["--commit", "0d1bf49"])
        .write_stdin("")
        .assert()
        .success();
    let after_update = normalised_store(&root);

    let envelope = import(&root, &["--slug", SLUG]);
    assert_eq!(counts(&envelope), (0, 0, 10), "{envelope}");
    assert!(refs(&envelope, "added_refs").is_empty(), "{envelope}");
    assert!(refs(&envelope, "removed_refs").is_empty(), "{envelope}");
    assert!(error_finding_classes(&envelope).is_empty(), "{envelope}");

    assert_eq!(
        normalised_store(&root),
        after_update,
        "a second import must leave the store's content untouched"
    );
    assert!(
        after_update.contains("status = \"in-progress\"")
            && after_update.contains("agent = \"implement-deep\"")
            && after_update.contains("commit = \"0d1bf49\""),
        "the execution fields must survive the re-import:\n{after_update}"
    );
}

/// `coupling` has no plan syntax — the renderer folds it into `Depends on` —
/// so the importer subtracts an existing row's coupling from the parsed
/// `needs` rather than taking `needs` whole. Without that subtraction the
/// edge would move back into `needs` on every import and the store would
/// never settle.
#[test]
fn an_existing_coupling_edge_survives_a_re_import() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);

    // The shape `tasks add --coupling` leaves behind: task 3's edge to 1 is a
    // coupling edge, so `needs` holds only the remainder.
    let text = fs::read_to_string(store_path(&root)).expect("the store is on disk");
    let coupled = text.replace(
        "needs = [\n    1,\n    2,\n]\ncoupling = []",
        "needs = [2]\ncoupling = [1]",
    );
    assert_ne!(coupled, text, "the golden's task-3 edge shape moved");
    fs::write(store_path(&root), &coupled).expect("the store is rewritten");

    let envelope = import(&root, &["--slug", SLUG]);
    assert_eq!(counts(&envelope), (0, 0, 10), "{envelope}");

    let after = fs::read_to_string(store_path(&root)).expect("the store is on disk");
    assert!(
        after.contains("needs = [2]\ncoupling = [1]"),
        "the coupling edge must not migrate back into `needs`:\n{after}"
    );
}

/// `--dry-run` against a real store target previews the same envelope and
/// leaves both the file and its sidecar untouched.
#[test]
fn a_dry_run_against_a_store_writes_no_bytes() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);

    let sidecar = sidecar_of(&store_path(&root));
    let before = fs::read(store_path(&root)).expect("the store is on disk");
    let before_sidecar = fs::read(&sidecar).expect("the sidecar is on disk");

    let envelope = import(&root, &["--slug", SLUG, "--dry-run"]);
    assert_eq!(counts(&envelope), (0, 0, 10), "{envelope}");

    assert_eq!(
        fs::read(store_path(&root)).expect("the store is still on disk"),
        before,
        "a dry run must not rewrite the store"
    );
    assert_eq!(
        fs::read(&sidecar).expect("the sidecar is still on disk"),
        before_sidecar,
        "a dry run must not rewrite the sidecar"
    );
}

/// Plan mode: `--plan` alone, no store target. This is `/plan-new` Phase 7,
/// where the plan document exists and no flow does. A relative `--plan` that
/// does not resolve against the working directory resolves against the root,
/// so the verb works from a subdirectory.
#[test]
fn plan_mode_validates_without_a_store_target() {
    let (_dir, root) = stage(FIXTURE_PLAN);

    let envelope = import(&root, &["--plan", PLAN_REL, "--dry-run"]);
    assert_eq!(counts(&envelope), (10, 0, 0), "{envelope}");
    assert!(finding_classes(&envelope).is_empty(), "{envelope}");
    assert!(
        !store_path(&root).exists(),
        "plan mode must create no store"
    );

    // Without a target the write path has nowhere to go, and clap cannot
    // express the conditional requirement, so it is a validation error.
    let error = import_err(&root, &["--plan", PLAN_REL]);
    assert_eq!(
        error.get("kind").and_then(serde_json::Value::as_str),
        Some("validation"),
        "{error}"
    );
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(message.contains("--dry-run"), "{message}");
    assert!(!store_path(&root).exists(), "{message}");
}

/// `--reconcile-record` marks an exact `task_ref` done under the derived ref,
/// adopts the record's spelling when only separators differ, and reports the
/// completions this plan does not account for.
#[test]
fn reconcile_record_adopts_a_separator_only_ref() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    let envelope = import(&root, &["--slug", SLUG, "--reconcile-record"]);

    assert_eq!(
        refs(&envelope, "adopted_refs"),
        vec!["parse-the-policy-bullets-and-the-max-parallel-range".to_string()],
        "{envelope}"
    );
    assert_eq!(
        refs(&envelope, "unmatched_refs"),
        vec!["a-task-from-another-plan".to_string()],
        "{envelope}"
    );

    let text = fs::read_to_string(store_path(&root)).expect("the store is on disk");
    assert!(
        text.contains("ref = \"parse-the-policy-bullets-and-the-max-parallel-range\""),
        "the record's spelling must win over the title's:\n{text}"
    );
    assert_eq!(
        text.matches("status = \"done\"").count(),
        2,
        "exactly tasks 1 and 4 are complete in the record:\n{text}"
    );

    // The adopted ref is the one the next import must join on, so it has to
    // be the ref the store recorded as imported.
    let refs_block = text
        .split("last_import_refs = [")
        .nth(1)
        .and_then(|tail| tail.split(']').next())
        .expect("the store carries `last_import_refs`");
    assert!(
        refs_block.contains("parse-the-policy-bullets-and-the-max-parallel-range"),
        "{refs_block}"
    );
}

/// The ref-set gate `/review-plan` and `/plan-update` run after a markdown
/// rewrite. It is a DRY RUN by contract: the renamed heading re-claims the
/// task number its old row still holds, so a real import raises
/// `dag/duplicate-number` at error severity and refuses before writing.
#[test]
fn a_renamed_heading_reports_both_sides_of_the_ref_diff() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);
    let before = fs::read(store_path(&root)).expect("the store is on disk");

    write_plan(
        &root,
        &FIXTURE_PLAN.replace(
            "9. Document the store contract",
            "9. Document the task-store contract",
        ),
    );
    let envelope = import(&root, &["--slug", SLUG, "--dry-run"]);

    assert_eq!(
        refs(&envelope, "removed_refs"),
        vec!["document-the-store-contract".to_string()],
        "{envelope}"
    );
    assert_eq!(
        refs(&envelope, "added_refs"),
        vec!["document-the-task-store-contract".to_string()],
        "{envelope}"
    );
    assert!(
        finding_classes(&envelope).contains(&"dag/duplicate-number".to_string()),
        "the kept old row still claims task 9: {envelope}"
    );
    assert_eq!(
        fs::read(store_path(&root)).expect("the store is still on disk"),
        before,
        "a dry run must write nothing"
    );
}

/// An `error`-class finding refuses the whole import — no partial store, no
/// partial sidecar — while the same plan under `--dry-run` still reports it.
#[test]
fn an_error_class_finding_aborts_the_write() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);
    let before = fs::read(store_path(&root)).expect("the store is on disk");

    write_plan(
        &root,
        &FIXTURE_PLAN.replace("Max parallel agents**: 6", "Max parallel agents**: 12"),
    );

    let preview = import(&root, &["--slug", SLUG, "--dry-run"]);
    assert_eq!(
        finding_classes(&preview),
        vec!["policy/max-parallel-range".to_string()],
        "{preview}"
    );
    assert_eq!(preview.get("ok"), Some(&serde_json::Value::Bool(false)));

    let error = import_err(&root, &["--slug", SLUG]);
    assert_eq!(
        error.get("kind").and_then(serde_json::Value::as_str),
        Some("validation"),
        "{error}"
    );
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(message.contains("policy/max-parallel-range"), "{message}");
    assert_eq!(
        fs::read(store_path(&root)).expect("the store is still on disk"),
        before,
        "a refused import must persist nothing"
    );
}

/// The checkpoint table and the policy are assigned whole on every import, so
/// a plan the grammar reads as taskless would replace both with what an empty
/// parse yields while reporting an import of nothing. What the refusal
/// protects is the state already on disk, so that is what is asserted: the
/// plan states a `max_parallel` the store does not hold and names no
/// checkpoint at all, and neither reaches the file.
#[test]
fn a_taskless_plan_is_refused_and_leaves_the_checkpoints_and_policy_alone() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);
    let before = fs::read(store_path(&root)).expect("the store is on disk");

    // Seven hashes is past the deepest heading the grammar reads as a task,
    // so every numbered heading reads as a phase label instead. The `####`
    // run goes first: `\n### ` requires the space the deeper heading spends
    // on a fourth hash, so neither substitution can catch the other's run.
    let graph_at = FIXTURE_PLAN
        .find("## Dependency Graph")
        .expect("the fixture carries a graph section");
    let risks_at = FIXTURE_PLAN
        .find("## Risks")
        .expect("a section closes the fixture");
    let taskless = format!("{}{}", &FIXTURE_PLAN[..graph_at], &FIXTURE_PLAN[risks_at..])
        .replace("\n#### ", "\n####### ")
        .replace("\n### ", "\n####### ")
        .replace("Max parallel agents**: 6", "Max parallel agents**: 4");
    assert!(
        taskless.contains("Max parallel agents**: 4") && !taskless.contains("CHECKPOINT"),
        "the taskless plan restates the store's own policy and checkpoints, so \
         leaving them in place would prove nothing"
    );
    write_plan(&root, &taskless);

    let preview = import(&root, &["--slug", SLUG, "--dry-run"]);
    assert_eq!(
        error_finding_classes(&preview),
        vec!["plan/no-tasks".to_string()],
        "{preview}"
    );
    assert_eq!(counts(&preview), (0, 0, 0), "{preview}");

    let error = import_err(&root, &["--slug", SLUG]);
    assert_eq!(
        error.get("kind").and_then(serde_json::Value::as_str),
        Some("validation"),
        "{error}"
    );
    assert!(
        error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("plan/no-tasks"),
        "{error}"
    );

    assert_eq!(
        fs::read(store_path(&root)).expect("the store is still on disk"),
        before,
        "a refused import must leave the store byte-identical"
    );
    let after = fs::read_to_string(store_path(&root)).expect("the store is still on disk");
    assert!(after.contains("max_parallel = 6"), "{after}");
    for checkpoint in ["A", "B", "C"] {
        assert!(
            after.contains(&format!("id = \"{checkpoint}\"")),
            "checkpoint {checkpoint} did not survive:\n{after}"
        );
    }
}

/// `--plan` takes an absolute or subdirectory-relative argument, but the value
/// the store records is one the read side has to accept back, so a plan whose
/// recorded form that side would refuse is refused at the import rather than
/// at every render afterwards. The absolute in-root case still imports: the
/// argument is canonicalised and relativised, so the root's `\\?\` prefix —
/// which a typed path never carries — does not decide containment.
#[test]
fn an_unrecordable_plan_argument_is_refused_and_an_absolute_one_records_a_relative_path() {
    let (dir, root) = stage(FIXTURE_PLAN);

    let outside = tempfile::tempdir().expect("a directory outside the root");
    let elsewhere = outside.path().join("house-plan.md");
    fs::write(&elsewhere, FIXTURE_PLAN).expect("plan written");
    let not_markdown = root.join("docs").join("plans").join("house-plan.txt");
    fs::write(&not_markdown, FIXTURE_PLAN).expect("plan written");

    for refused in [elsewhere.as_path(), not_markdown.as_path()] {
        let named = refused.to_string_lossy().to_string();
        let error = import_err(&root, &["--slug", SLUG, "--plan", &named]);
        assert_eq!(
            error.get("kind").and_then(serde_json::Value::as_str),
            Some("validation"),
            "{named}: {error}"
        );
        assert!(
            !store_path(&root).exists(),
            "{named}: a refused import must persist nothing"
        );
    }

    // The tempdir's own path rather than the canonical root: on Windows the
    // two differ by exactly the prefix the resolver has to strip.
    let typed = dir.path().join("docs").join("plans").join("house-plan.md");
    assert!(typed.is_absolute(), "{}", typed.display());
    let envelope = import(&root, &["--slug", SLUG, "--plan", &typed.to_string_lossy()]);
    assert_eq!(counts(&envelope), (10, 0, 0), "{envelope}");

    let text = fs::read_to_string(store_path(&root)).expect("the store is on disk");
    assert!(
        text.contains(&format!("plan_path = \"{PLAN_REL}\"")),
        "the store must record the repo-relative form:\n{text}"
    );
}

/// `--reconcile-record` reads the record the flow's `[artifacts]` names, so a
/// flow that points its record off the sibling filename reconciles against the
/// file it actually writes. The override is file-controlled input like every
/// other recorded path, so one leaving the root is refused rather than read.
#[test]
fn reconcile_record_honours_the_artifacts_override_and_holds_it_under_the_root() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    fs::rename(
        flow_dir(&root).join("execution-record.toml"),
        flow_dir(&root).join("record-2.toml"),
    )
    .expect("the record moves off the sibling name");
    fs::write(flow_dir(&root).join("execution-record.toml"), DECOY_RECORD).expect("decoy written");
    write_context(
        &root,
        &format!("\n[artifacts]\nexecution_record = \".claude/flows/{SLUG}/record-2.toml\"\n"),
    );

    let envelope = import(&root, &["--slug", SLUG, "--reconcile-record"]);
    assert_eq!(
        refs(&envelope, "adopted_refs"),
        vec!["parse-the-policy-bullets-and-the-max-parallel-range".to_string()],
        "{envelope}"
    );
    assert_eq!(
        refs(&envelope, "unmatched_refs"),
        vec!["a-task-from-another-plan".to_string()],
        "the decoy on the sibling filename was read instead: {envelope}"
    );
    let text = fs::read_to_string(store_path(&root)).expect("the store is on disk");
    assert_eq!(
        text.matches("status = \"done\"").count(),
        2,
        "exactly tasks 1 and 4 are complete in the override's record:\n{text}"
    );

    let before = fs::read(store_path(&root)).expect("the store is on disk");
    write_context(
        &root,
        "\n[artifacts]\nexecution_record = \"../escape.toml\"\n",
    );
    let error = import_err(&root, &["--slug", SLUG, "--reconcile-record"]);
    assert_eq!(
        error.get("kind").and_then(serde_json::Value::as_str),
        Some("validation"),
        "{error}"
    );
    assert!(
        error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("execution_record"),
        "{error}"
    );
    assert_eq!(
        fs::read(store_path(&root)).expect("the store is still on disk"),
        before,
        "a refused import must persist nothing"
    );
}

/// The plan corpus is CRLF on disk. The scanner strips one trailing `\r` from
/// every line before matching and the schema collapses CRLF in every body, so
/// the two encodings must land on identical bytes — not merely equivalent
/// ones, since the store is itself byte-compared downstream.
#[test]
fn a_crlf_plan_imports_byte_identically() {
    let crlf = FIXTURE_PLAN.replace('\n', "\r\n");
    assert!(
        crlf.len() > FIXTURE_PLAN.len(),
        "the committed fixture is already CRLF — the `text eol=lf` pin is not holding"
    );

    let (_lf_dir, lf_root) = stage(FIXTURE_PLAN);
    let (_crlf_dir, crlf_root) = stage(&crlf);
    import(&lf_root, &["--slug", SLUG]);
    import(&crlf_root, &["--slug", SLUG]);

    let lf_store = normalised_store(&lf_root);
    let crlf_store = normalised_store(&crlf_root);
    if lf_store != crlf_store {
        panic!("{}", first_line_diff(&crlf_store, &lf_store));
    }
    assert_matches_golden(&crlf_root);
}

/// The leg neither byte-golden can carry: plan → store → plan, judged on the
/// PLAN's own bytes. A `Files` annotation the store cannot hold is dropped by
/// the import AND by the render, so both goldens stay internally consistent —
/// import(plan) is stable, render(store) is stable, and each round trip closes
/// over a document the author's annotation has already left. Only a comparison
/// against the source plan sees it go.
///
/// The entries are compared rather than the bytes: the render legitimately
/// re-flows a wrapped `Files` line onto one line.
#[test]
fn a_files_annotation_survives_the_plan_to_plan_round_trip() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);

    let rendered = cli(&root)
        .args(["tasks", "render", "--slug", SLUG, "--stdout"])
        .write_stdin("")
        .assert()
        .success();
    let rendered = String::from_utf8_lossy(&rendered.get_output().stdout).to_string();

    let source = files_entries(FIXTURE_PLAN);
    assert!(
        source.iter().any(|entry| entry.contains("(new)")),
        "the fixture plan carries no annotated `Files` entry, so this asserts nothing: {source:?}"
    );
    assert_eq!(source, files_entries(&rendered));
}

/// Every `Files` entry of every task, as `` `path` `` plus whatever trailed
/// it, with wrapped lines folded first.
fn files_entries(plan: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut open: Option<String> = None;
    for line in plan.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match line.strip_prefix("- **Files**:") {
            Some(rest) => {
                out.extend(open.take());
                open = Some(rest.trim().to_string());
            }
            None => match line.strip_prefix("  ").filter(|_| open.is_some()) {
                Some(more) => {
                    let open = open.as_mut().expect("the filter proved it open");
                    open.push(' ');
                    open.push_str(more.trim());
                }
                None => out.extend(open.take()),
            },
        }
    }
    out.extend(open);
    out.iter()
        .flat_map(|line| line.split(", ").map(str::to_string))
        .collect()
}

/// A plan documenting the task grammar quotes it, and a quoted field line is
/// not one. The example is spliced at the end of the `## Tasks` section, so
/// every line it carries would land on task 10 and on the store's duplicate-id
/// check — the golden compare is what says none of it did.
const FENCED_EXAMPLE: &str = "```md
### 9. A task no plan declares [L]
- **Files**: `src/nowhere.rs`
- **Depends on**: 4
- **Action**: Nothing at all.
- **Acceptance**: Nothing at all.
```

";

/// Offset of the `## Dependency Graph` heading, which is where the Tasks
/// section ends.
fn end_of_tasks(plan: &str) -> usize {
    plan.find("## Dependency Graph")
        .expect("the fixture carries a graph section")
}

/// 1-based line of the spliced fence opener, which is what the refusal names.
fn fence_line(plan: &str) -> usize {
    plan.lines()
        .position(|line| line == "```md")
        .expect("the spliced example opens a fence")
        + 1
}

#[test]
fn a_fenced_example_inside_the_tasks_section_changes_nothing() {
    let at = end_of_tasks(FIXTURE_PLAN);
    let quoted = format!(
        "{}{FENCED_EXAMPLE}{}",
        &FIXTURE_PLAN[..at],
        &FIXTURE_PLAN[at..]
    );
    let (_dir, root) = stage(&quoted);

    let preview = import(&root, &["--slug", SLUG, "--dry-run"]);
    assert!(finding_classes(&preview).is_empty(), "{preview}");
    import(&root, &["--slug", SLUG]);
    assert_matches_golden(&root);
}

/// An unclosed fence feeds `## Dependency Graph` and everything after it into
/// the Tasks section, which parses as a plan that simply has fewer tasks. The
/// refusal is what stops that from landing in a store.
#[test]
fn an_unclosed_fence_inside_the_tasks_section_is_refused() {
    let (_dir, root) = stage(FIXTURE_PLAN);
    import(&root, &["--slug", SLUG]);
    let before = fs::read(store_path(&root)).expect("the store is on disk");

    let at = end_of_tasks(FIXTURE_PLAN);
    let unclosed = format!(
        "{}{}{}",
        &FIXTURE_PLAN[..at],
        FENCED_EXAMPLE.replacen("```\n", "", 1),
        &FIXTURE_PLAN[at..]
    );
    assert_eq!(
        unclosed.matches("```").count(),
        1,
        "the staged plan must carry exactly the one unclosed marker:\n{unclosed}"
    );
    write_plan(&root, &unclosed);

    let error = import_err(&root, &["--slug", SLUG, "--dry-run"]);
    assert_eq!(
        error.get("kind").and_then(serde_json::Value::as_str),
        Some("validation"),
        "{error}"
    );
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        message.contains(&format!("line {}", fence_line(&unclosed))),
        "{message}"
    );
    assert!(message.contains("never closed"), "{message}");

    import_err(&root, &["--slug", SLUG]);
    assert_eq!(
        fs::read(store_path(&root)).expect("the store is still on disk"),
        before,
        "a refused import must persist nothing"
    );
}

fn sidecar_of(file: &Path) -> PathBuf {
    let mut raw = file.as_os_str().to_os_string();
    raw.push(".sha256");
    PathBuf::from(raw)
}
