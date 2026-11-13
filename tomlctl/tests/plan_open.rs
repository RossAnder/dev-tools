//! `tomlctl plan open` against a tempdir-rooted `.claude/` layout.

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::common;
use common::stdout_json;

const SLUG: &str = "draft-flow";

fn root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical root");
    fs::create_dir_all(root.join(".claude")).expect(".claude");
    (dir, root)
}

fn tomlctl(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("tomlctl")
        .expect("the binary builds")
        .env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .args(["--error-format", "json"])
        .args(args)
        .write_stdin("")
        .assert()
}

fn open(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["plan", "open", "--slug", SLUG];
    args.extend_from_slice(extra);
    stdout_json(&tomlctl(root, &args).success())
}

fn error_kind(assert: &assert_cmd::assert::Assert) -> String {
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let json: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("expected an error envelope: {e}; got: {stderr}"));
    json["error"]["kind"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn flow_file(root: &Path, name: &str) -> PathBuf {
    root.join(".claude").join("flows").join(SLUG).join(name)
}

fn authored_file(root: &Path, name: &str) -> PathBuf {
    root.join("docs").join("flows").join(SLUG).join(name)
}

fn read_toml(path: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(path).expect("readable")).expect("valid TOML")
}

fn registered(root: &Path) -> bool {
    let registry = root.join(".claude").join("active-flow.toml");
    registry.exists()
        && read_toml(&registry)
            .get("active")
            .and_then(toml::Value::as_array)
            .is_some_and(|rows| {
                rows.iter()
                    .any(|row| row.get("slug").and_then(toml::Value::as_str) == Some(SLUG))
            })
}

fn with_stale_stamp(plan: &str) -> String {
    plan.lines()
        .map(|line| {
            if line.starts_with("last_updated") {
                "last_updated = 2020-01-02"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn stage_backlog(root: &Path) {
    let store = r#"schema_version = 1

[[backlog]]
id = "B-aaaa1111"
kind = "bug"
summary = "Retries   storm\n the upstream"
context = "Seen under load.\n# Not a heading"
evidence = ["src/retry.rs:40", "the 429 log"]

[[backlog]]
id = "B-bbbb2222"
kind = "debt"
summary = "No context here"

[[compacted]]
id = "B-cccc3333"
summary = "Folded away"
"#;
    fs::write(root.join(".claude").join("backlog.toml"), store).expect("backlog store");
}

#[test]
fn a_fresh_open_writes_the_brief_and_the_stores() {
    let (_dir, root) = root();
    let envelope = open(&root, &[]);

    let brief_rel = format!("docs/flows/{SLUG}/brief.md");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["slug"], SLUG);
    assert_eq!(envelope["brief"], brief_rel.as_str());
    let created: Vec<&str> = envelope["created"]
        .as_array()
        .expect("created is an array")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for name in [
        "context.toml",
        "execution-record.toml",
        "tasks.toml",
        "plan.toml",
    ] {
        let rel = format!(".claude/flows/{SLUG}/{name}");
        assert!(created.contains(&rel.as_str()), "{rel} in {created:?}");
        assert!(flow_file(&root, name).exists(), "{name} on disk");
    }
    assert!(
        created.contains(&brief_rel.as_str()),
        "{brief_rel} in {created:?}"
    );
    assert!(authored_file(&root, "brief.md").is_file());
    assert!(
        !flow_file(&root, "brief.md").exists(),
        "no authored file may sit under .claude/"
    );

    let brief = fs::read_to_string(authored_file(&root, "brief.md")).expect("brief");
    let positions: Vec<usize> = [
        "## Intent",
        "## Non-goals",
        "## Scope budget",
        "## Success criteria",
        "## Background",
        "## Unknowns",
    ]
    .iter()
    .map(|heading| {
        brief
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} in {brief}"))
    })
    .collect();
    assert!(positions.is_sorted(), "headings in order: {brief}");
    assert!(!brief.contains('\r'));

    let context = read_toml(&flow_file(&root, "context.toml"));
    assert_eq!(context["plan_path"].as_str(), Some(brief_rel.as_str()));
    assert_eq!(context["status"].as_str(), Some("draft"));
    assert!(flow_file(&root, "plan.toml.sha256").exists());
}

#[test]
fn a_draft_is_not_auto_picked_and_stays_resumable() {
    let (_dir, root) = root();
    open(&root, &["--branch", "main", "--scope", "src/**"]);

    let by_path =
        stdout_json(&tomlctl(&root, &["flow", "resolve", "--path", "src/a.rs"]).success());
    assert_eq!(by_path["resolved"], false, "{by_path}");
    let by_branch =
        stdout_json(&tomlctl(&root, &["flow", "resolve", "--branch", "main"]).success());
    assert_eq!(by_branch["resolved"], false, "{by_branch}");

    assert!(!registered(&root), "a draft must have no registry entry");
    let context = read_toml(&flow_file(&root, "context.toml"));
    assert!(context.get("branch").is_none());
    assert_eq!(context["scope"].as_array().map(Vec::len), Some(0));

    let listed = stdout_json(&tomlctl(&root, &["flow", "list", "--status", "draft"]).success());
    let slugs: Vec<&str> = listed["flows"]
        .as_array()
        .expect("flows")
        .iter()
        .filter_map(|flow| flow["slug"].as_str())
        .collect();
    assert_eq!(slugs, [SLUG]);
    let explicit = stdout_json(&tomlctl(&root, &["flow", "resolve", "--flow", SLUG]).success());
    assert_eq!(explicit["resolved"], true, "{explicit}");
}

#[test]
fn the_binding_is_held_in_plan_toml() {
    let (_dir, root) = root();
    open(
        &root,
        &[
            "--branch", "main", "--scope", "src/**", "--scope", "docs/**",
        ],
    );
    let plan = read_toml(&flow_file(&root, "plan.toml"));
    assert_eq!(plan["binding"]["branch"].as_str(), Some("main"));
    let scope: Vec<&str> = plan["binding"]["scope"]
        .as_array()
        .expect("scope")
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(scope, ["src/**", "docs/**"]);
}

#[test]
fn a_rerun_keeps_the_brief_the_created_date_and_the_binding() {
    let (_dir, root) = root();
    open(&root, &["--branch", "main", "--scope", "src/**"]);

    let brief_path = authored_file(&root, "brief.md");
    fs::write(&brief_path, "# edited\n").expect("edit brief");
    let context_path = flow_file(&root, "context.toml");
    let context = fs::read_to_string(&context_path).expect("context");
    let created_line = context
        .lines()
        .find(|line| line.starts_with("created"))
        .expect("a created line");
    fs::write(
        &context_path,
        context.replace(created_line, "created = 2020-01-02"),
    )
    .expect("backdate");
    // A stale stamp makes any rewrite visible in the bytes, whatever today is.
    let plan_path = flow_file(&root, "plan.toml");
    let stale = with_stale_stamp(&fs::read_to_string(&plan_path).expect("plan.toml"));
    fs::write(&plan_path, &stale).expect("backdate the stamp");
    let plan_before = fs::read(&plan_path).expect("plan.toml");

    let envelope = open(&root, &[]);

    assert_eq!(envelope["created"], serde_json::json!([]));
    assert_eq!(
        fs::read_to_string(&brief_path).expect("brief"),
        "# edited\n"
    );
    assert_eq!(
        read_toml(&context_path)["created"].to_string(),
        "2020-01-02"
    );
    assert_eq!(
        fs::read(flow_file(&root, "plan.toml")).expect("plan.toml"),
        plan_before,
        "a bare re-run must leave plan.toml's bytes alone"
    );
    assert!(!registered(&root));

    open(&root, &["--branch", "dev"]);
    let plan = read_toml(&flow_file(&root, "plan.toml"));
    assert_eq!(plan["binding"]["branch"].as_str(), Some("dev"));
    assert_eq!(
        plan["binding"]["scope"].as_array().map(Vec::len),
        Some(1),
        "a flag not given keeps its held value"
    );
}

#[test]
fn a_flow_recording_another_plan_is_refused_untouched() {
    let (_dir, root) = root();
    let plans = root.join("docs").join("plans");
    fs::create_dir_all(&plans).expect("plans dir");
    let plan = plans.join(format!("{SLUG}.md"));
    fs::write(&plan, "# Plan\n").expect("plan");
    tomlctl(
        &root,
        &[
            "flow",
            "init",
            "--slug",
            SLUG,
            "--plan",
            plan.to_str().expect("utf-8"),
        ],
    )
    .success();

    let refused = tomlctl(&root, &["plan", "open", "--slug", SLUG]).failure();

    assert_eq!(error_kind(&refused), "validation");
    assert!(!flow_file(&root, "plan.toml").exists());
    assert!(!authored_file(&root, "brief.md").exists());
    assert!(registered(&root), "a refused open must leave the v1 entry");
}

#[test]
fn an_approved_plan_keeps_its_registry_entry_and_writes_nothing() {
    let (_dir, root) = root();
    open(&root, &[]);

    let plan_path = flow_file(&root, "plan.toml");
    let mut plan = fs::read_to_string(&plan_path).expect("plan.toml");
    plan.push_str(
        "\n[[approval]]\ncommit = \"abc\"\nat = 2026-11-05T12:00:00Z\nbrief_sha256 = \"d\"\nby = \"user\"\nreview = \"ran\"\nreason = \"\"\n",
    );
    fs::write(&plan_path, &plan).expect("stamp an approval");
    tomlctl(
        &root,
        &[
            "flow", "active", "add", "--slug", SLUG, "--branch", "main", "--scope", "src/**",
        ],
    )
    .success();
    let registry = root.join(".claude").join("active-flow.toml");
    let registry_before = fs::read(&registry).expect("registry");
    fs::remove_file(authored_file(&root, "brief.md")).expect("remove brief");

    let envelope = open(&root, &["--branch", "other"]);

    assert_eq!(envelope["created"], serde_json::json!([]));
    assert_eq!(fs::read(&registry).expect("registry"), registry_before);
    assert_eq!(fs::read_to_string(&plan_path).expect("plan.toml"), plan);
    assert!(
        !authored_file(&root, "brief.md").exists(),
        "an approved plan's brief is not re-seeded"
    );
}

#[test]
fn from_fills_intent_background_and_a_pointer_note() {
    let (_dir, root) = root();
    stage_backlog(&root);
    let envelope = open(&root, &["--from", "B-aaaa1111,B-bbbb2222,B-aaaa1111"]);

    let brief = fs::read_to_string(authored_file(&root, "brief.md")).expect("brief");
    let section = |from: &str, to: &str| {
        let start = brief.find(from).expect("start heading");
        let end = brief.find(to).expect("end heading");
        brief[start..end].to_string()
    };
    let intent = section("## Intent", "## Non-goals");
    assert!(
        intent.contains("- B-aaaa1111: Retries storm the upstream"),
        "{intent}"
    );
    assert!(intent.contains("- B-bbbb2222: No context here"), "{intent}");
    assert_eq!(intent.matches("B-aaaa1111").count(), 1, "{intent}");
    let background = section("## Background", "## Unknowns");
    assert!(background.contains("Seen under load."), "{background}");
    assert!(background.contains("\\# Not a heading"), "{background}");
    assert!(
        background.contains("notes/backlog-B-aaaa1111.md"),
        "{background}"
    );

    let note =
        fs::read_to_string(authored_file(&root, "notes/backlog-B-aaaa1111.md")).expect("note");
    assert!(note.contains("`src/retry.rs:40`"), "{note}");
    assert!(note.contains("`the 429 log`"), "{note}");
    assert!(
        note.contains(".claude/backlog-evidence/B-aaaa1111/"),
        "{note}"
    );
    let bare =
        fs::read_to_string(authored_file(&root, "notes/backlog-B-bbbb2222.md")).expect("note");
    assert!(
        bare.contains(".claude/backlog-evidence/B-bbbb2222/"),
        "{bare}"
    );
    let rel = format!("docs/flows/{SLUG}/notes/backlog-B-aaaa1111.md");
    assert!(
        envelope["created"]
            .as_array()
            .expect("created")
            .iter()
            .any(|path| path.as_str() == Some(rel.as_str())),
        "{envelope}"
    );
    assert!(
        !flow_file(&root, "notes").exists(),
        "no authored file may sit under .claude/"
    );
}

#[cfg(unix)]
#[test]
fn an_authored_directory_linked_out_of_the_repo_is_refused() {
    let (_dir, root) = root();
    let outside = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(root.join("docs")).expect("docs dir");
    std::os::unix::fs::symlink(outside.path(), root.join("docs").join("flows")).expect("link");

    let refused = tomlctl(&root, &["plan", "open", "--slug", SLUG]).failure();

    assert_eq!(error_kind(&refused), "validation");
    assert_eq!(
        fs::read_dir(outside.path()).expect("readable").count(),
        0,
        "nothing may be written through the link"
    );
}

#[cfg(unix)]
#[test]
fn a_notes_directory_linked_out_of_the_repo_is_refused() {
    let (_dir, root) = root();
    stage_backlog(&root);
    open(&root, &[]);
    let outside = tempfile::tempdir().expect("tempdir");
    std::os::unix::fs::symlink(outside.path(), authored_file(&root, "notes")).expect("link");

    let refused = tomlctl(
        &root,
        &["plan", "open", "--slug", SLUG, "--from", "B-aaaa1111"],
    )
    .failure();

    assert_eq!(error_kind(&refused), "validation");
    assert_eq!(
        fs::read_dir(outside.path()).expect("readable").count(),
        0,
        "nothing may be written through the link"
    );
}

#[test]
fn from_reads_a_compacted_row() {
    let (_dir, root) = root();
    stage_backlog(&root);
    open(&root, &["--from", "B-cccc3333"]);
    let brief = fs::read_to_string(authored_file(&root, "brief.md")).expect("brief");
    assert!(brief.contains("- B-cccc3333: Folded away"), "{brief}");
}

#[test]
fn an_unknown_backlog_id_writes_nothing() {
    let (_dir, root) = root();
    stage_backlog(&root);
    let refused = tomlctl(
        &root,
        &[
            "plan",
            "open",
            "--slug",
            SLUG,
            "--from",
            "B-aaaa1111,B-nope",
        ],
    )
    .failure();
    assert_eq!(error_kind(&refused), "not_found");
    assert!(
        !root.join(".claude").join("flows").join(SLUG).exists(),
        "a refused open must create no flow"
    );
}

/// The id names a file under `notes/`, so a hand-edited row must not reach outside it.
#[test]
fn a_backlog_id_naming_a_path_outside_notes_writes_nothing() {
    for id in ["../evil", "..", "sub/evil", "sub\\\\evil"] {
        let (_dir, root) = root();
        fs::write(
            root.join(".claude").join("backlog.toml"),
            format!("schema_version = 1\n\n[[backlog]]\nid = \"{id}\"\nsummary = \"s\"\n"),
        )
        .expect("backlog store");
        let id = id.replace("\\\\", "\\");
        let refused = tomlctl(&root, &["plan", "open", "--slug", SLUG, "--from", &id]).failure();
        assert_eq!(error_kind(&refused), "validation", "{id}");
        let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
        assert!(stderr.contains("single path component"), "{id}: {stderr}");
        assert!(
            !root.join("docs").exists(),
            "{id}: a refused open must write nothing"
        );
    }
}

#[test]
fn a_slug_with_a_separator_is_refused() {
    let (_dir, root) = root();
    let refused = tomlctl(&root, &["plan", "open", "--slug", "../escape"]).failure();
    assert_eq!(error_kind(&refused), "validation");
}
