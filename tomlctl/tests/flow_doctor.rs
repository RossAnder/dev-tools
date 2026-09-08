//! Integration tests for `tomlctl flow doctor`.
//!
//! Each test materialises a tempdir-rooted fake repo, synthesises one or
//! more flows under `<tmp>/.claude/flows/<slug>/`, and runs the built
//! `tomlctl` binary via `assert_cmd` with `TOMLCTL_ROOT` pointed at the
//! tempdir. Mirrors the sandbox strategy used in `tests/flow_active.rs`
//! and `tests/flow_ensure_artifact.rs`.

use assert_cmd::Command;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

mod common;

/// Build `<tmp>/.claude/` and return `(tempdir, root)`. Tests drive flow
/// creation off this base.
fn fresh_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join(".claude")).unwrap();
    (dir, root)
}

/// `<file>.sha256`. Local helper — mirrors `integrity::sidecar_path`.
fn sidecar_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

/// Write the artifact and a matching `<file>.sha256` sidecar in the
/// canonical `<hex>  <basename>\n` format.
fn write_artifact_with_sidecar(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    let digest = Sha256::digest(body.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", b);
    }
    let basename = path.file_name().unwrap().to_string_lossy();
    fs::write(sidecar_path(path), format!("{hex}  {basename}\n")).unwrap();
}

/// The four `[artifacts]` keys every flow minted before the per-flow task
/// store carries on disk — the corpus a hard fail on a missing `tasks` key
/// would break repo-wide.
fn legacy_artifacts_lines(slug: &str) -> String {
    format!(
        r#"review_ledger = ".claude/flows/{slug}/review-ledger.toml"
optimise_findings = ".claude/flows/{slug}/optimise-findings.toml"
execution_record = ".claude/flows/{slug}/execution-record.toml"
plan_review_findings = ".claude/flows/{slug}/plan-review-findings.toml"
"#
    )
}

/// The full canonical `[artifacts]` body `flow init` writes today.
fn canonical_artifacts_lines(slug: &str) -> String {
    format!(
        "{}tasks = \".claude/flows/{slug}/tasks.toml\"\n",
        legacy_artifacts_lines(slug)
    )
}

/// The `[tasks]` counter block a seeded flow carries unless a test needs the
/// join skewed or the block absent. `total` and `completed` derive from
/// different artifacts, so only a pair that agrees is a clean starting point.
const ZERO_COUNTERS: &str = "\n[tasks]\ntotal = 0\ncompleted = 0\nin_progress = 0\n\n";

/// [`seed_flow`] with the agreeing counters every other test wants.
fn seed_flow_with_artifacts(root: &Path, slug: &str, artifacts_lines: &str) {
    seed_flow(root, slug, ZERO_COUNTERS, artifacts_lines);
}

/// Seed a flow under `<root>/.claude/flows/<slug>/` whose `[tasks]` block is
/// `counters` and whose `[artifacts]` table body is `artifacts_lines`:
/// `context.toml` + `execution-record.toml` + matching sidecars, and a plan
/// file at `docs/plans/<slug>.md` so `plan-path-resolves` passes.
fn seed_flow(root: &Path, slug: &str, counters: &str, artifacts_lines: &str) {
    let flow_dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&flow_dir).unwrap();

    // Plan file the flow points at.
    let plans_dir = root.join("docs").join("plans");
    fs::create_dir_all(&plans_dir).unwrap();
    let plan_file = plans_dir.join(format!("{slug}.md"));
    fs::write(&plan_file, "# plan\n").unwrap();

    let context_body = format!(
        r#"slug = "{slug}"
plan_path = "docs/plans/{slug}.md"
status = "in-progress"
created = 2026-05-08
updated = 2026-05-08
scope = []
{counters}[artifacts]
{artifacts_lines}"#
    );
    write_artifact_with_sidecar(&flow_dir.join("context.toml"), &context_body);

    // execution-record.toml — minimal 2-line bootstrap shape.
    write_artifact_with_sidecar(&execution_record_path(root, slug), EMPTY_RECORD);
}

/// The two-line bootstrap execution record every seeded flow starts with.
const EMPTY_RECORD: &str = "schema_version = 1\nlast_updated = 2026-05-08\n";

/// `<root>/.claude/flows/<slug>/execution-record.toml`.
fn execution_record_path(root: &Path, slug: &str) -> PathBuf {
    root.join(".claude")
        .join("flows")
        .join(slug)
        .join("execution-record.toml")
}

/// Seed a clean, current flow — canonical five-key `[artifacts]` table, so
/// the doctor reports neither a failure nor an advisory.
fn seed_clean_flow(root: &Path, slug: &str) {
    seed_flow_with_artifacts(root, slug, &canonical_artifacts_lines(slug));
}

/// Seed an `active-flow.toml` registry pointing at the listed slugs (in
/// insertion order). All bindings are minimal — only `slug` + `last_used`
/// are populated.
fn seed_active_flow_registry(root: &Path, slugs: &[&str]) {
    let claude = root.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let registry = claude.join("active-flow.toml");
    let mut body = String::from("schema_version = 1\n");
    for slug in slugs {
        body.push_str(&format!(
            "\n[[active]]\nslug = \"{slug}\"\nlast_used = \"2026-05-08T12:00:00Z\"\n"
        ));
    }
    write_artifact_with_sidecar(&registry, &body);
}

/// Run `tomlctl flow doctor <args>` against `root` and parse stdout as JSON.
/// Asserts process success.
fn run_doctor(root: &Path, args: &[&str]) -> JsonValue {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("doctor");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON; err={e}; stdout:\n{stdout}"))
}

/// Find the first check entry with the given `name` (and matching `scope` if
/// provided). Panics if not found.
fn find_check<'a>(v: &'a JsonValue, name: &str, scope: Option<&str>) -> &'a JsonValue {
    let arr = v["checks"].as_array().expect("checks must be array");
    arr.iter()
        .find(|c| {
            c["name"].as_str() == Some(name) && scope.is_none_or(|s| c["scope"].as_str() == Some(s))
        })
        .unwrap_or_else(|| {
            panic!(
                "no check with name={name} scope={:?} in {}",
                scope,
                serde_json::to_string_pretty(arr).unwrap()
            )
        })
}

// ---------------------------------------------------------------------------
// Acceptance: clean flow returns ok=true with all checks passing.
// ---------------------------------------------------------------------------

/// Clean flow on a clean root: every check passes, `ok=true`, no fixes
/// applied, no warnings.
#[test]
fn clean_flow_returns_ok_true_with_all_checks_passing() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "feature-x");
    seed_active_flow_registry(&root, &["feature-x"]);

    let v = run_doctor(&root, &["--slug", "feature-x"]);
    assert_eq!(
        v["ok"],
        JsonValue::Bool(true),
        "clean flow must return ok=true; got: {v}"
    );
    assert!(v["fixes_applied"].as_array().unwrap().is_empty());
    assert!(v["warnings"].as_array().unwrap().is_empty());

    // Every check is ok=true.
    let checks = v["checks"].as_array().unwrap();
    for c in checks {
        assert_eq!(
            c["ok"],
            JsonValue::Bool(true),
            "check must pass: {}",
            serde_json::to_string(c).unwrap()
        );
    }

    // Coverage pin: each named check is present at least once.
    for name in [
        "context-exists",
        "execution-record-exists",
        "context-sidecar",
        "execution-record-sidecar",
        "tasks-exists",
        "tasks-sidecar",
        "tasks-counters",
        "artifacts-canonical",
        "plan-path-resolves",
        "active-flow-registry",
        "gitignore-claude",
    ] {
        find_check(&v, name, None);
    }
}

// ---------------------------------------------------------------------------
// Acceptance: legacy four-key [artifacts] table.
// ---------------------------------------------------------------------------

/// `<root>/.claude/flows/<slug>/context.toml`.
fn context_path(root: &Path, slug: &str) -> PathBuf {
    root.join(".claude")
        .join("flows")
        .join(slug)
        .join("context.toml")
}

/// A flow whose `[artifacts]` table predates the task store still passes
/// `artifacts-canonical`; the absent `tasks` key surfaces as a top-level
/// advisory naming the key.
#[test]
fn legacy_four_key_artifacts_passes_with_tasks_warning() {
    let (_g, root) = fresh_root();
    seed_flow_with_artifacts(&root, "legacy", &legacy_artifacts_lines("legacy"));
    seed_active_flow_registry(&root, &["legacy"]);

    let v = run_doctor(&root, &["--slug", "legacy"]);
    let chk = find_check(&v, "artifacts-canonical", Some("legacy"));
    assert_eq!(
        chk["ok"],
        JsonValue::Bool(true),
        "a missing [artifacts].tasks key must NOT fail the check; got: {chk}"
    );
    assert_eq!(
        v["ok"],
        JsonValue::Bool(true),
        "an otherwise-clean legacy flow must still return ok=true; got: {v}"
    );

    let warnings = v["warnings"].as_array().expect("warnings must be array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("tasks")),
        "the absent key must surface as a warning naming `tasks`; got: {warnings:?}"
    );
}

/// A `tasks` key that is PRESENT but points somewhere else still fails the
/// check — the tolerance above is for absence only.
#[test]
fn divergent_tasks_artifact_value_fails_the_check() {
    let (_g, root) = fresh_root();
    let artifacts = format!(
        "{}tasks = \".claude/flows/other-flow/tasks.toml\"\n",
        legacy_artifacts_lines("wrong-tasks")
    );
    seed_flow_with_artifacts(&root, "wrong-tasks", &artifacts);
    seed_active_flow_registry(&root, &["wrong-tasks"]);

    let v = run_doctor(&root, &["--slug", "wrong-tasks"]);
    assert_eq!(v["ok"], JsonValue::Bool(false));
    let chk = find_check(&v, "artifacts-canonical", Some("wrong-tasks"));
    assert_eq!(
        chk["ok"],
        JsonValue::Bool(false),
        "a divergent [artifacts].tasks value must fail; got: {chk}"
    );
    let detail = chk["detail"].as_str().expect("detail must be present");
    assert!(
        detail.contains("tasks"),
        "detail must name the divergent key; got: {detail}"
    );
}

/// A legacy four-key table whose plan declares NO `## Tasks` section: `--fix`
/// reports the advisory and leaves the file's bytes exactly as they were. That
/// gate is the whole scope of the guarantee — the backfill test below covers
/// the other side of it.
#[test]
fn fix_leaves_a_task_less_flows_context_bytes_untouched() {
    let (_g, root) = fresh_root();
    seed_flow_with_artifacts(&root, "legacy", &legacy_artifacts_lines("legacy"));
    seed_active_flow_registry(&root, &["legacy"]);

    let context = context_path(&root, "legacy");
    let before = fs::read(&context).unwrap();
    assert!(
        !fs::read_to_string(plan_path(&root, "legacy"))
            .unwrap()
            .contains("## Tasks"),
        "precondition: the gate this test scopes to is the plan declaring no task section"
    );

    let v = run_doctor(&root, &["--slug", "legacy", "--fix"]);
    let warnings = v["warnings"].as_array().expect("warnings must be array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("tasks")),
        "precondition: --fix run must still reach the tasks advisory; got: {warnings:?}"
    );

    assert_eq!(
        fs::read(&context).unwrap(),
        before,
        "doctor --fix must not write context.toml bytes"
    );
    assert!(
        !String::from_utf8_lossy(&fs::read(&context).unwrap()).contains("tasks ="),
        "doctor --fix must not backfill the [artifacts].tasks key for a task-less plan"
    );
}

/// `<root>/docs/plans/<slug>.md` — the plan `seed_flow_with_artifacts` writes
/// and `context.toml` points at.
fn plan_path(root: &Path, slug: &str) -> PathBuf {
    root.join("docs").join("plans").join(format!("{slug}.md"))
}

/// Overwrite that plan with one carrying a `## Tasks` section — the single
/// input the task-store checks and the `[artifacts].tasks` backfill are all
/// gated on.
fn declare_a_task_section(root: &Path, slug: &str) {
    fs::write(
        plan_path(root, slug),
        "# plan\n\n## Tasks\n\n### 1. Do the thing [S]\n- **Files**: `src/lib.rs`\n",
    )
    .unwrap();
}

/// `<root>/.claude/flows/<slug>/tasks.toml`.
fn tasks_store_path(root: &Path, slug: &str) -> PathBuf {
    root.join(".claude")
        .join("flows")
        .join(slug)
        .join("tasks.toml")
}

/// A plan that declares no task section legitimately carries no store, so both
/// task-store checks report structurally — keeping the per-flow check count
/// stable — and each carries the reason it did not look. An `ok=true` with no
/// detail would be indistinguishable from a store that was actually there.
#[test]
fn task_store_checks_skip_a_flow_whose_plan_declares_no_task_section() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "prose-only");
    seed_active_flow_registry(&root, &["prose-only"]);

    let v = run_doctor(&root, &["--slug", "prose-only"]);
    assert_eq!(v["ok"], JsonValue::Bool(true), "got: {v}");

    let exists = find_check(&v, "tasks-exists", Some("prose-only"));
    assert_eq!(exists["ok"], JsonValue::Bool(true), "got: {exists}");
    assert!(
        exists["detail"].as_str().unwrap_or("").contains("## Tasks"),
        "tasks-exists must say why it did not look; got: {exists}"
    );

    let sidecar = find_check(&v, "tasks-sidecar", Some("prose-only"));
    assert_eq!(sidecar["ok"], JsonValue::Bool(true), "got: {sidecar}");
    assert!(
        sidecar["detail"]
            .as_str()
            .unwrap_or("")
            .contains("tasks.toml"),
        "tasks-sidecar must name the artifact it skipped; got: {sidecar}"
    );

    assert!(v["warnings"].as_array().unwrap().is_empty(), "got: {v}");
}

/// A plan that DOES declare one, with no store on disk: doctor creates no
/// artifacts, so a failing check would have no route out. The absence stays
/// advisory and the run stays green — but the advisory has to name the flow
/// and the verb that clears it, or it is a report nobody can act on.
#[test]
fn a_declared_task_section_with_no_store_warns_without_failing() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "planned");
    declare_a_task_section(&root, "planned");
    seed_active_flow_registry(&root, &["planned"]);

    let v = run_doctor(&root, &["--slug", "planned", "--fix"]);
    assert_eq!(v["ok"], JsonValue::Bool(true), "got: {v}");

    let chk = find_check(&v, "tasks-exists", Some("planned"));
    assert_eq!(chk["ok"], JsonValue::Bool(true), "got: {chk}");
    assert!(
        chk["detail"].as_str().unwrap_or("").contains("missing"),
        "the check must report the absence, not the skip; got: {chk}"
    );

    let warnings = v["warnings"].as_array().expect("warnings must be array");
    assert!(
        warnings.iter().any(|w| {
            let text = w.as_str().unwrap_or("");
            text.contains("planned") && text.contains("import-plan")
        }),
        "got: {warnings:?}"
    );

    assert!(
        !tasks_store_path(&root, "planned").exists(),
        "doctor --fix must NOT create a missing task store"
    );
}

/// A task store on disk carries a digest whatever the plan section says, so
/// this check is gated on the file rather than on the plan. A mismatch fails
/// the run, repairs nothing on a report-only pass, and is regenerated under
/// `--fix` like any other artifact sidecar.
#[test]
fn a_tampered_task_store_sidecar_fails_and_is_regenerated_under_fix() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "stored");
    declare_a_task_section(&root, "stored");
    seed_active_flow_registry(&root, &["stored"]);

    let store = tasks_store_path(&root, "stored");
    write_artifact_with_sidecar(&store, "schema_version = 1\nlast_updated = 2026-05-08\n");
    let bogus = format!(
        "{}  {}\n",
        "0".repeat(64),
        store.file_name().unwrap().to_string_lossy()
    );
    fs::write(sidecar_path(&store), &bogus).unwrap();

    let v = run_doctor(&root, &["--slug", "stored"]);
    assert_eq!(v["ok"], JsonValue::Bool(false), "got: {v}");
    let chk = find_check(&v, "tasks-sidecar", Some("stored"));
    assert_eq!(chk["ok"], JsonValue::Bool(false), "got: {chk}");
    assert!(
        chk["detail"].as_str().unwrap_or("").contains("mismatch"),
        "detail must surface the mismatch reason; got: {chk}"
    );
    // The store is present; only its digest disagrees.
    assert_eq!(
        find_check(&v, "tasks-exists", Some("stored"))["ok"],
        JsonValue::Bool(true),
        "got: {v}"
    );
    assert_eq!(
        fs::read_to_string(sidecar_path(&store)).unwrap(),
        bogus,
        "a report-only run must repair nothing"
    );

    let fixed = run_doctor(&root, &["--slug", "stored", "--fix"]);
    let fixes = fixed["fixes_applied"]
        .as_array()
        .expect("fixes_applied must be array");
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("sidecar-refresh".to_string())
                && f["scope"] == JsonValue::String("stored".to_string())
                && f["ok"] == JsonValue::Bool(true)
        ),
        "fixes_applied must include a sidecar-refresh for the task store; got: {fixed}"
    );
    assert_ne!(fs::read_to_string(sidecar_path(&store)).unwrap(), bogus);
    assert_eq!(
        run_doctor(&root, &["--slug", "stored"])["ok"],
        JsonValue::Bool(true),
        "the regenerated digest must clear the check"
    );
}

/// The advisory an absent `[artifacts].tasks` key raises is clearable: for a
/// flow whose plan declares a task section, `--fix` fills the key in place.
/// Only an ABSENT key is ever filled — a present one, canonical or not, is
/// `artifacts-canonical`'s to judge.
#[test]
fn fix_backfills_the_tasks_key_for_a_flow_whose_plan_declares_a_task_section() {
    let (_g, root) = fresh_root();
    seed_flow_with_artifacts(
        &root,
        "planned-legacy",
        &legacy_artifacts_lines("planned-legacy"),
    );
    declare_a_task_section(&root, "planned-legacy");
    seed_active_flow_registry(&root, &["planned-legacy"]);

    let context = context_path(&root, "planned-legacy");
    assert!(
        !fs::read_to_string(&context).unwrap().contains("tasks ="),
        "precondition: the key must start absent"
    );

    let v = run_doctor(&root, &["--slug", "planned-legacy", "--fix"]);
    let fixes = v["fixes_applied"]
        .as_array()
        .expect("fixes_applied must be array");
    assert!(
        fixes.iter().any(|f| f["name"]
            == JsonValue::String("artifacts-tasks-backfill".to_string())
            && f["scope"] == JsonValue::String("planned-legacy".to_string())
            && f["ok"] == JsonValue::Bool(true)),
        "got: {v}"
    );

    let after = fs::read_to_string(&context).unwrap();
    assert!(
        after.contains(r#"tasks = ".claude/flows/planned-legacy/tasks.toml""#),
        "the backfilled value must be the canonical one; got:\n{after}"
    );

    // Clearing the advisory is the point, and the rewrite has to leave a
    // digest that still covers the bytes it produced.
    let again = run_doctor(&root, &["--slug", "planned-legacy"]);
    assert!(
        !again["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("[artifacts].tasks")),
        "the backfill must clear the advisory it answers; got: {again}"
    );
    assert_eq!(
        find_check(&again, "context-sidecar", Some("planned-legacy"))["ok"],
        JsonValue::Bool(true),
        "got: {again}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: the [tasks] counter join is advisory in both tiers.
// ---------------------------------------------------------------------------

/// A task store carrying two rows. The counter join reads the `ref` set and
/// nothing else, so the rows carry what identifies them and stop there.
const POPULATED_STORE: &str = r#"schema_version = 1
last_updated = 2026-05-08
plan_path = "docs/plans/joined.md"
last_import_refs = ["do-the-thing", "check-the-thing"]

[[items]]
id = 1
ref = "do-the-thing"
title = "Do the thing"
status = "done"

[[items]]
id = 2
ref = "check-the-thing"
title = "Check the thing"
status = "pending"
"#;

/// An execution record whose `done` task-completions name `refs`, in order —
/// the set `[tasks].completed` counts.
fn record_completing(refs: &[&str]) -> String {
    let mut body = String::from(EMPTY_RECORD);
    for (index, task_ref) in refs.iter().enumerate() {
        body.push_str(&format!(
            "\n[[items]]\nid = \"E{id}\"\ntype = \"task-completion\"\ndate = 2026-05-08\n\
             agent = \"implement-lite\"\ntask_ref = \"{task_ref}\"\nstatus = \"done\"\n\
             summary = \"Completed.\"\n",
            id = index + 1
        ));
    }
    body
}

/// `[tasks].completed` is derived from the execution record and `total` counts
/// store rows, so a `completed` past `total` is the join between them having
/// gone stale. It stays advisory: doctor creates no store and cannot re-derive
/// the record-side number, so there is no repair `--fix` could apply and the
/// run stays green.
#[test]
fn counters_past_the_row_count_warn_without_failing_the_run() {
    let (_g, root) = fresh_root();
    seed_flow(
        &root,
        "skewed",
        "\n[tasks]\ntotal = 2\ncompleted = 5\nin_progress = 0\n\n",
        &canonical_artifacts_lines("skewed"),
    );
    seed_active_flow_registry(&root, &["skewed"]);

    let v = run_doctor(&root, &["--slug", "skewed"]);
    assert_eq!(v["ok"], JsonValue::Bool(true), "got: {v}");
    assert_eq!(
        find_check(&v, "tasks-counters", Some("skewed"))["ok"],
        JsonValue::Bool(true),
        "the advisory must not flip the envelope; got: {v}"
    );

    let warnings = v["warnings"].as_array().expect("warnings must be array");
    assert!(
        warnings.iter().any(|w| {
            let text = w.as_str().unwrap_or("");
            text.contains("skewed") && text.contains("completed = 5") && text.contains("total = 2")
        }),
        "the advisory must name the flow and both counters; got: {warnings:?}"
    );

    // Control: the same shape with counters that agree raises nothing, so what
    // fired above is the skew and not the presence of a `[tasks]` block.
    seed_clean_flow(&root, "agreed");
    let agreed = run_doctor(&root, &["--slug", "agreed"]);
    assert!(
        agreed["warnings"].as_array().unwrap().is_empty(),
        "got: {agreed}"
    );
}

/// The second tier: `completed` counts a ref no store row carries, so the two
/// counters are counting different sets. The join needs rows to compare
/// against and is skipped without them — otherwise a flow whose record
/// predates its store would report every completion it holds.
#[test]
fn a_record_completion_naming_no_store_row_warns_without_failing_the_run() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "joined");
    declare_a_task_section(&root, "joined");
    seed_active_flow_registry(&root, &["joined"]);
    write_artifact_with_sidecar(
        &execution_record_path(&root, "joined"),
        &record_completing(&["do-the-thing", "a-ref-no-row-carries"]),
    );

    let names_the_orphan = |v: &JsonValue| {
        v["warnings"]
            .as_array()
            .expect("warnings must be array")
            .iter()
            .filter_map(JsonValue::as_str)
            .find(|w| w.contains("a-ref-no-row-carries"))
            .map(str::to_string)
    };

    // No store on disk: the join has nothing to compare against and must stay
    // quiet, or the warning below would be firing on the record alone.
    let unimported = run_doctor(&root, &["--slug", "joined"]);
    assert_eq!(names_the_orphan(&unimported), None, "got: {unimported}");

    write_artifact_with_sidecar(&tasks_store_path(&root, "joined"), POPULATED_STORE);
    let v = run_doctor(&root, &["--slug", "joined"]);
    assert_eq!(v["ok"], JsonValue::Bool(true), "got: {v}");
    assert_eq!(
        find_check(&v, "tasks-counters", Some("joined"))["ok"],
        JsonValue::Bool(true),
        "the advisory must not flip the envelope; got: {v}"
    );

    let warning = names_the_orphan(&v).unwrap_or_else(|| panic!("got: {v}"));
    assert!(warning.contains("joined"), "{warning}");
    assert!(
        !warning.contains("do-the-thing"),
        "the completion the store DOES carry must not be named — the warning is \
         the join, not a count of the record's entries: {warning}"
    );
}

/// A flow offering neither counters nor a populated store has nothing to join,
/// so the check reports structurally — keeping the per-flow check count stable
/// the way the task-store checks do — and carries the reason it did not look.
#[test]
fn the_counter_join_skips_a_flow_offering_neither_counters_nor_a_store() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "bare", "\n", &canonical_artifacts_lines("bare"));
    seed_active_flow_registry(&root, &["bare"]);

    let v = run_doctor(&root, &["--slug", "bare"]);
    assert_eq!(v["ok"], JsonValue::Bool(true), "got: {v}");

    let chk = find_check(&v, "tasks-counters", Some("bare"));
    assert_eq!(chk["ok"], JsonValue::Bool(true), "got: {chk}");
    assert!(
        chk["detail"].as_str().unwrap_or("").starts_with("skipped:"),
        "the check must say why it did not look; got: {chk}"
    );
    assert!(v["warnings"].as_array().unwrap().is_empty(), "got: {v}");

    // Control: counters alone are enough to reach the join, so the skip above
    // is the absent block rather than the absent store.
    seed_clean_flow(&root, "counted");
    let counted = run_doctor(&root, &["--slug", "counted"]);
    assert_eq!(
        find_check(&counted, "tasks-counters", Some("counted"))["detail"],
        JsonValue::Null,
        "a flow carrying counters must not report as skipped; got: {counted}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: tampered sidecar reports + fixes under --fix.
// ---------------------------------------------------------------------------

/// Tampered sidecar (digest mismatch on context.toml) reports as a check
/// failure without mutating disk.
#[test]
fn tampered_context_sidecar_reports_check_failure_no_repair() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "feature-x");
    seed_active_flow_registry(&root, &["feature-x"]);

    // Overwrite the context.toml sidecar with a wrong-digest line.
    let context = root
        .join(".claude")
        .join("flows")
        .join("feature-x")
        .join("context.toml");
    let bogus_sidecar = format!(
        "{}  {}\n",
        "0".repeat(64),
        context.file_name().unwrap().to_string_lossy()
    );
    fs::write(sidecar_path(&context), &bogus_sidecar).unwrap();

    let v = run_doctor(&root, &["--slug", "feature-x"]);
    assert_eq!(v["ok"], JsonValue::Bool(false));
    let chk = find_check(&v, "context-sidecar", Some("feature-x"));
    assert_eq!(chk["ok"], JsonValue::Bool(false));
    let detail = chk["detail"].as_str().expect("detail must be present");
    assert!(
        detail.contains("mismatch"),
        "detail must surface mismatch reason; got: {detail}"
    );

    // No fixes applied (we didn't pass --fix).
    assert!(v["fixes_applied"].as_array().unwrap().is_empty());

    // Sidecar bytes unchanged from the bogus state.
    assert_eq!(
        fs::read_to_string(sidecar_path(&context)).unwrap(),
        bogus_sidecar
    );
}

/// `--fix` regenerates the tampered sidecar. The sidecar bytes must change
/// to a digest matching the file's actual contents, and `fixes_applied`
/// must surface the regen action.
#[test]
fn tampered_sidecar_regenerated_under_fix() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "feature-x");
    seed_active_flow_registry(&root, &["feature-x"]);

    let context = root
        .join(".claude")
        .join("flows")
        .join("feature-x")
        .join("context.toml");
    fs::write(
        sidecar_path(&context),
        format!(
            "{}  {}\n",
            "f".repeat(64),
            context.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();

    let v = run_doctor(&root, &["--slug", "feature-x", "--fix"]);
    let fixes = v["fixes_applied"]
        .as_array()
        .expect("fixes_applied must be array");
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("sidecar-refresh".to_string())
                && f["ok"] == JsonValue::Bool(true)
        ),
        "fixes_applied must include a sidecar-refresh ok=true entry; got: {v}"
    );

    // Sidecar now matches the file.
    let live_bytes = fs::read(&context).unwrap();
    let want = Sha256::digest(&live_bytes);
    let mut want_hex = String::with_capacity(64);
    for b in want.iter() {
        use std::fmt::Write;
        let _ = write!(want_hex, "{:02x}", b);
    }
    let on_disk = fs::read_to_string(sidecar_path(&context)).unwrap();
    let got_hex = on_disk.split_whitespace().next().unwrap();
    assert_eq!(
        got_hex.to_ascii_lowercase(),
        want_hex,
        "sidecar must match file after --fix"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: missing artifact reports without fixing.
// ---------------------------------------------------------------------------

/// A missing `context.toml` (e.g. half-bootstrapped flow dir) reports as a
/// failing check; doctor never creates the file even with `--fix`.
#[test]
fn missing_context_reports_without_creation_under_fix() {
    let (_g, root) = fresh_root();
    let flow_dir = root.join(".claude").join("flows").join("ghost");
    fs::create_dir_all(&flow_dir).unwrap();
    // No context.toml. No execution-record.toml. Empty flow dir.

    let v = run_doctor(&root, &["--slug", "ghost", "--fix"]);
    assert_eq!(v["ok"], JsonValue::Bool(false));

    let chk = find_check(&v, "context-exists", Some("ghost"));
    assert_eq!(chk["ok"], JsonValue::Bool(false));
    let detail = chk["detail"].as_str().expect("detail must be present");
    assert!(
        detail.contains("missing"),
        "detail must surface missing reason; got: {detail}"
    );

    // Doctor must NEVER create missing artifacts (that's flow init's job).
    assert!(
        !flow_dir.join("context.toml").exists(),
        "doctor --fix must NOT create missing context.toml"
    );
    assert!(
        !flow_dir.join("execution-record.toml").exists(),
        "doctor --fix must NOT create missing execution-record.toml"
    );

    // The fixes_applied list never carries a "create" action. The allowlist is
    // the whole fix vocabulary, so a name added later fails here.
    let fixes = v["fixes_applied"].as_array().unwrap();
    for f in fixes {
        let name = f["name"].as_str().unwrap_or("");
        assert!(
            matches!(
                name,
                "sidecar-refresh" | "active-prune" | "artifacts-tasks-backfill"
            ),
            "doctor must never emit a create-action fix; got: {f}"
        );
    }
}

// ---------------------------------------------------------------------------
// Acceptance: stale active-flow registry entry pruned under --fix.
// ---------------------------------------------------------------------------

/// Without `--fix`, a stale registry entry is reported but not pruned.
#[test]
fn stale_active_flow_entry_reported_without_fix() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "real");
    // Registry references both `real` AND a deleted slug `gone`.
    seed_active_flow_registry(&root, &["real", "gone"]);

    let v = run_doctor(&root, &[]);
    assert_eq!(v["ok"], JsonValue::Bool(false));

    let chk = find_check(&v, "active-flow-registry", Some("global"));
    assert_eq!(chk["ok"], JsonValue::Bool(false));
    let detail = chk["detail"].as_str().expect("detail must be present");
    assert!(
        detail.contains("gone"),
        "detail must name the stale slug; got: {detail}"
    );

    // Registry bytes unchanged — `gone` survives (no --fix).
    let raw = fs::read_to_string(root.join(".claude").join("active-flow.toml")).unwrap();
    assert!(raw.contains("\"gone\""));
}

/// `--fix` prunes the stale registry entry; `real` survives, `gone` does not.
#[test]
fn stale_active_flow_entry_pruned_under_fix() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "real");
    seed_active_flow_registry(&root, &["real", "gone"]);

    let v = run_doctor(&root, &["--fix"]);
    let fixes = v["fixes_applied"].as_array().expect("fixes must be array");
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("active-prune".to_string())
                && f["scope"] == JsonValue::String("gone".to_string())
                && f["ok"] == JsonValue::Bool(true)
        ),
        "fixes_applied must include an active-prune entry for `gone`; got: {v}"
    );

    let raw = fs::read_to_string(root.join(".claude").join("active-flow.toml")).unwrap();
    assert!(
        !raw.contains("\"gone\""),
        "active-flow.toml must NOT contain `gone` after --fix; got:\n{raw}"
    );
    assert!(
        raw.contains("\"real\""),
        "active-flow.toml MUST still contain `real` after --fix; got:\n{raw}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: gitignored-`.claude` warning fires when matched.
// ---------------------------------------------------------------------------

/// `.gitignore` listing `.claude/` triggers a warning AND a non-passing
/// `gitignore-claude` check.
#[test]
fn gitignored_claude_emits_warning() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "feature-x");
    seed_active_flow_registry(&root, &["feature-x"]);
    fs::write(root.join(".gitignore"), ".claude/\n").unwrap();

    let v = run_doctor(&root, &["--slug", "feature-x"]);
    let warnings = v["warnings"].as_array().expect("warnings must be array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains(".gitignore")),
        "warnings must include a gitignore line; got: {warnings:?}"
    );
    let chk = find_check(&v, "gitignore-claude", Some("global"));
    assert_eq!(
        chk["ok"],
        JsonValue::Bool(true),
        "gitignore-claude is a non-failing check post-R31 — the warning is the actionable surface, the check entry stays ok=true so envelope.ok is not flipped"
    );

    // Plain `.claude` (no trailing slash) also matches.
    fs::write(root.join(".gitignore"), ".claude\n").unwrap();
    let v2 = run_doctor(&root, &["--slug", "feature-x"]);
    assert_eq!(
        find_check(&v2, "gitignore-claude", Some("global"))["ok"],
        JsonValue::Bool(true),
        "gitignore-claude check is non-failing post-R31"
    );
    let warnings2 = v2["warnings"].as_array().expect("warnings must be array");
    assert!(
        warnings2
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains(".gitignore")),
        "plain `.claude` (no trailing slash) must still fire the warning entry"
    );

    // Comments and blank lines are NOT matches.
    fs::write(root.join(".gitignore"), "# .claude/\n\nnode_modules/\n").unwrap();
    let v3 = run_doctor(&root, &["--slug", "feature-x"]);
    let chk3 = find_check(&v3, "gitignore-claude", Some("global"));
    assert_eq!(
        chk3["ok"],
        JsonValue::Bool(true),
        "comments / unrelated entries must NOT fire the warning"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: --fix --dry-run emits plan without writing.
// ---------------------------------------------------------------------------

/// `--fix --dry-run` emits the would-be fixes plan but never touches the
/// filesystem.
#[test]
fn fix_dry_run_emits_plan_without_writing() {
    let (_g, root) = fresh_root();
    seed_clean_flow(&root, "real");
    seed_active_flow_registry(&root, &["real", "gone"]);

    // Tamper context.toml sidecar to also stage a sidecar-refresh fix.
    let context = root
        .join(".claude")
        .join("flows")
        .join("real")
        .join("context.toml");
    let pre_sidecar_bytes = fs::read(sidecar_path(&context)).unwrap();
    fs::write(
        sidecar_path(&context),
        format!(
            "{}  {}\n",
            "0".repeat(64),
            context.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();
    let staged_sidecar_bytes = fs::read(sidecar_path(&context)).unwrap();

    let pre_registry = fs::read_to_string(root.join(".claude").join("active-flow.toml")).unwrap();

    let v = run_doctor(&root, &["--fix", "--dry-run"]);
    assert_eq!(v["dry_run"], JsonValue::Bool(true));
    let fixes = v["fixes_applied"].as_array().expect("fixes must be array");
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("active-prune".to_string())
                && f["action"].as_str().unwrap_or("").contains("would prune")
        ),
        "dry-run plan must include `would prune` action; got: {fixes:?}"
    );
    assert!(
        fixes.iter().any(
            |f| f["name"] == JsonValue::String("sidecar-refresh".to_string())
                && f["action"].as_str().unwrap_or("").contains("would refresh")
        ),
        "dry-run plan must include `would refresh` action; got: {fixes:?}"
    );

    // Filesystem unchanged: registry still has `gone`; sidecar bytes are
    // the bogus ones we staged, not the originals (we tampered) and not a
    // freshly-refreshed digest.
    assert_eq!(
        fs::read_to_string(root.join(".claude").join("active-flow.toml")).unwrap(),
        pre_registry,
        "dry-run must not mutate the registry"
    );
    assert_eq!(
        fs::read(sidecar_path(&context)).unwrap(),
        staged_sidecar_bytes,
        "dry-run must not regenerate the sidecar"
    );
    // Sanity: the staged bogus bytes aren't the originals — the tamper
    // worked, so a subsequent live --fix WOULD change them.
    assert_ne!(
        staged_sidecar_bytes, pre_sidecar_bytes,
        "test setup precondition: tamper must change sidecar bytes"
    );
}
