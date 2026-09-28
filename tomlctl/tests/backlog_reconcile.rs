//! Black-box coverage for `tomlctl backlog reconcile` — each promoted row is
//! bucketed by the status of the tasks that close it in its target flow, and
//! only `--adopt` and `--apply` write.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{TASKS_SLUG, assert_sidecar_matches, backlog, sandbox, seed_tasks, store_path};

const OTHER_SLUG: &str = "fixture-other-flow";

/// A task row `(ref, status, commit, detail)`.
type Task<'a> = (&'a str, &'a str, &'a str, &'a str);

/// A link `(ref, closes, refs)`.
type Link<'a> = (&'a str, &'a [&'a str], &'a [&'a str]);

fn tasks_toml(slug: &str, tasks: &[Task], links: &[Link]) -> String {
    let mut text = format!("schema_version = 1\nplan_path = \"docs/plans/{slug}.md\"\n");
    for (name, closes, refs) in links {
        text.push_str(&format!(
            "\n[[backlog_links]]\nref = {name:?}\ncloses = {closes:?}\nrefs = {refs:?}\n"
        ));
    }
    for (index, (name, status, commit, detail)) in tasks.iter().enumerate() {
        text.push_str(&format!(
            "\n[[items]]\nid = {}\nref = {name:?}\ntitle = \"Task {name}\"\neffort = \"S\"\n\
             status = {status:?}\ncommit = {commit:?}\ndetail = {detail:?}\n",
            index + 1
        ));
    }
    text
}

fn seed_fixture_flow(root: &Path, tasks: &[Task], links: &[Link]) -> PathBuf {
    seed_tasks(root, &tasks_toml(TASKS_SLUG, tasks, links))
}

/// The fixture flow's `context.toml`, rewritten to sit at `status`.
fn set_fixture_status(root: &Path, status: &str) {
    let ctx = root
        .join(".claude")
        .join("flows")
        .join(TASKS_SLUG)
        .join("context.toml");
    let text = fs::read_to_string(&ctx).unwrap();
    let rewritten = text.replace(
        "status = \"in-progress\"",
        &format!("status = \"{status}\""),
    );
    assert_ne!(rewritten, text, "the seeded context must carry a status");
    fs::write(&ctx, rewritten).unwrap();
}

fn seed_other_flow(root: &Path) {
    let flow = root.join(".claude").join("flows").join(OTHER_SLUG);
    fs::create_dir_all(&flow).unwrap();
    fs::write(
        flow.join("context.toml"),
        format!("status = \"in-progress\"\nplan_path = \"docs/plans/{OTHER_SLUG}.md\"\n"),
    )
    .unwrap();
}

/// Promoted rows `(id, promoted_to)`, plus one open row no bucket takes.
fn seed_backlog(root: &Path, rows: &[(&str, &str)]) -> PathBuf {
    let mut text = String::from(
        "schema_version = 1\nlast_updated = 2026-08-01\n\n[[backlog]]\n\
         id = \"B-0be00000\"\nkind = \"bug\"\nsummary = \"still open\"\narea = \"\"\n\
         status = \"open\"\ncreated = 2026-08-01\nlast_seen = 2026-08-01\nseen_count = 1\n",
    );
    for (id, target) in rows {
        text.push_str(&format!(
            "\n[[backlog]]\nid = {id:?}\nkind = \"bug\"\nsummary = \"item {id}\"\n\
             area = \"\"\nstatus = \"promoted\"\ncreated = 2026-08-01\n\
             last_seen = 2026-08-01\nseen_count = 1\npromoted = 2026-08-02\n\
             promoted_to = {target:?}\n"
        ));
    }
    let path = store_path(root);
    fs::write(&path, text).unwrap();
    path
}

fn reconcile(root: &Path, args: &[&str]) -> Value {
    let mut full = vec!["reconcile"];
    full.extend_from_slice(args);
    let report = backlog(root, &full);
    assert_eq!(report["ok"], json!(true), "{report}");
    report
}

fn bucket_ids(report: &Value, bucket: &str) -> Vec<String> {
    report["buckets"][bucket]
        .as_array()
        .unwrap_or_else(|| panic!("bucket `{bucket}` missing: {report}"))
        .iter()
        .map(|entry| entry["id"].as_str().unwrap().to_string())
        .collect()
}

fn entry<'a>(report: &'a Value, bucket: &str, id: &str) -> &'a Value {
    report["buckets"][bucket]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("`{id}` is not in `{bucket}`: {report}"))
}

fn reason<'a>(report: &'a Value, bucket: &str, id: &str) -> &'a str {
    entry(report, bucket, id)["reason"].as_str().unwrap()
}

fn backlog_row(root: &Path, id: &str) -> toml::Value {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(store_path(root)).unwrap()).unwrap();
    doc.get("backlog")
        .and_then(toml::Value::as_array)
        .unwrap()
        .iter()
        .find(|row| row.get("id").and_then(toml::Value::as_str) == Some(id))
        .cloned()
        .unwrap_or_else(|| panic!("no backlog row {id}"))
}

fn str_of<'a>(row: &'a toml::Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(toml::Value::as_str)
}

fn strings_of(row: &toml::Value, key: &str) -> Vec<String> {
    row.get(key)
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("`{key}` is not an array in {row}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn sidecar(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".sha256");
    PathBuf::from(s)
}

#[test]
fn apply_resolves_a_ready_row_with_its_link_and_keeps_the_claim() {
    let (_tmp, root) = sandbox();
    seed_fixture_flow(
        &root,
        &[
            ("one", "done", "abc1234", ""),
            ("two", "done", "def5678", ""),
            ("three", "pending", "", ""),
            ("four", "done", "0ff1ce9", ""),
        ],
        &[
            ("one", &["B-aaaa0001"], &[]),
            ("two", &["B-aaaa0001"], &[]),
            ("three", &["B-aaaa0002"], &[]),
            ("four", &[], &["B-aaaa0001"]),
        ],
    );
    seed_backlog(
        &root,
        &[("B-aaaa0001", TASKS_SLUG), ("B-aaaa0002", TASKS_SLUG)],
    );

    let report = reconcile(&root, &["--apply"]);
    assert_eq!(bucket_ids(&report, "ready"), ["B-aaaa0001"], "{report}");
    assert_eq!(bucket_ids(&report, "in-progress"), ["B-aaaa0002"]);
    assert_eq!(report["applied"], json!(["B-aaaa0001"]));
    assert_eq!(report["skipped"], json!([]));

    let row = backlog_row(&root, "B-aaaa0001");
    assert_eq!(str_of(&row, "status"), Some("resolved"), "{row}");
    assert_eq!(str_of(&row, "resolved_flow"), Some(TASKS_SLUG));
    assert_eq!(strings_of(&row, "resolved_tasks"), ["one", "two", "four"]);
    assert_eq!(
        strings_of(&row, "resolved_commits"),
        ["abc1234", "def5678"],
        "a refs task's commit did not deliver the item"
    );
    assert_eq!(str_of(&row, "promoted_to"), Some(TASKS_SLUG));
    assert_eq!(
        str_of(&backlog_row(&root, "B-aaaa0002"), "status"),
        Some("promoted"),
        "a row with a pending closing task is left alone"
    );
    assert_sidecar_matches(&store_path(&root));
}

#[test]
fn adopt_links_an_unlinked_row_and_names_the_flow_to_render() {
    let (_tmp, root) = sandbox();
    let tasks = seed_fixture_flow(
        &root,
        &[
            ("one", "pending", "", "Fixes backlog item B-aaaa0001."),
            ("two", "pending", "", ""),
        ],
        &[],
    );
    seed_backlog(&root, &[("B-aaaa0001", TASKS_SLUG)]);

    let before = reconcile(&root, &[]);
    assert_eq!(bucket_ids(&before, "unlinked"), ["B-aaaa0001"], "{before}");
    assert_eq!(before["adopted"], json!([]));
    assert_eq!(before["render_needed"], json!([]));

    let report = reconcile(&root, &["--adopt"]);
    assert_eq!(
        report["adopted"],
        json!([{"id": "B-aaaa0001", "flow": TASKS_SLUG, "task_refs": ["one"]}]),
        "{report}"
    );
    assert_eq!(report["render_needed"], json!([TASKS_SLUG]));
    assert_eq!(bucket_ids(&report, "in-progress"), ["B-aaaa0001"]);
    assert_eq!(bucket_ids(&report, "unlinked"), Vec::<String>::new());

    let store: toml::Value = toml::from_str(&fs::read_to_string(&tasks).unwrap()).unwrap();
    let links = store
        .get("backlog_links")
        .and_then(toml::Value::as_array)
        .expect("adopt writes a link");
    assert_eq!(links.len(), 1, "{store}");
    assert_eq!(str_of(&links[0], "ref"), Some("one"));
    assert_eq!(strings_of(&links[0], "closes"), ["B-aaaa0001"]);
    assert_sidecar_matches(&tasks);
}

#[test]
fn a_closed_flow_orphans_a_row_with_a_pending_closing_task() {
    let (_tmp, root) = sandbox();
    seed_fixture_flow(
        &root,
        &[("one", "pending", "", "")],
        &[("one", &["B-aaaa0001"], &[])],
    );
    set_fixture_status(&root, "review");
    seed_backlog(&root, &[("B-aaaa0001", TASKS_SLUG)]);

    let report = reconcile(&root, &[]);
    assert_eq!(bucket_ids(&report, "orphaned"), ["B-aaaa0001"], "{report}");
    let orphan = entry(&report, "orphaned", "B-aaaa0001");
    assert_eq!(orphan["flow_status"], "review");
    assert_eq!(orphan["closes"], json!(["one"]));
    let why = reason(&report, "orphaned", "B-aaaa0001");
    assert!(
        why.contains(&format!("flow `{TASKS_SLUG}` is at `review`")),
        "{why}"
    );
    assert!(why.contains("one (pending)"), "{why}");
}

#[test]
fn a_deferred_closing_task_stalls_the_row() {
    let (_tmp, root) = sandbox();
    seed_fixture_flow(
        &root,
        &[("one", "done", "", ""), ("two", "deferred", "", "")],
        &[("one", &["B-aaaa0001"], &[]), ("two", &["B-aaaa0001"], &[])],
    );
    seed_backlog(&root, &[("B-aaaa0001", TASKS_SLUG)]);

    let report = reconcile(&root, &["--apply"]);
    assert_eq!(bucket_ids(&report, "stalled"), ["B-aaaa0001"], "{report}");
    let why = reason(&report, "stalled", "B-aaaa0001");
    assert!(why.contains("two (deferred)"), "{why}");
    assert_eq!(report["applied"], json!([]), "a stalled row is not applied");
}

#[test]
fn a_target_naming_nothing_dangles() {
    let (_tmp, root) = sandbox();
    seed_backlog(&root, &[("B-aaaa0001", "no-such-flow")]);

    let report = reconcile(&root, &[]);
    assert_eq!(bucket_ids(&report, "dangling"), ["B-aaaa0001"], "{report}");
    let dangling = entry(&report, "dangling", "B-aaaa0001");
    assert_eq!(dangling["target"], "no-such-flow");
    assert_eq!(dangling["flow_status"], Value::Null);
    let why = reason(&report, "dangling", "B-aaaa0001");
    assert!(
        why.contains("`no-such-flow` names no flow and no plan"),
        "{why}"
    );
}

#[test]
fn an_external_target_is_external() {
    let (_tmp, root) = sandbox();
    seed_backlog(&root, &[("B-aaaa0001", "external:GH-42")]);

    let report = reconcile(&root, &[]);
    assert_eq!(bucket_ids(&report, "external"), ["B-aaaa0001"], "{report}");
    assert_eq!(bucket_ids(&report, "dangling"), Vec::<String>::new());
    let external = entry(&report, "external", "B-aaaa0001");
    assert_eq!(external["target"], "external:GH-42");
    assert_eq!(
        external["reason"], "promoted to an external reference",
        "{external}"
    );
}

#[test]
fn a_refs_only_link_stays_unlinked() {
    let (_tmp, root) = sandbox();
    seed_fixture_flow(
        &root,
        &[("one", "done", "", "")],
        &[("one", &[], &["B-aaaa0001"])],
    );
    seed_backlog(&root, &[("B-aaaa0001", TASKS_SLUG)]);

    let report = reconcile(&root, &["--apply"]);
    assert_eq!(bucket_ids(&report, "unlinked"), ["B-aaaa0001"], "{report}");
    let unlinked = entry(&report, "unlinked", "B-aaaa0001");
    assert_eq!(unlinked["closes"], json!([]));
    assert_eq!(unlinked["refs"], json!(["one"]));
    let why = reason(&report, "unlinked", "B-aaaa0001");
    assert!(why.contains("referenced only by: one"), "{why}");
    assert_eq!(report["applied"], json!([]));
    assert_eq!(
        str_of(&backlog_row(&root, "B-aaaa0001"), "status"),
        Some("promoted")
    );
}

#[test]
fn the_flow_filter_keeps_only_rows_targeting_that_flow() {
    let (_tmp, root) = sandbox();
    seed_fixture_flow(&root, &[("one", "done", "", "")], &[]);
    seed_other_flow(&root);
    seed_backlog(
        &root,
        &[
            ("B-aaaa0001", TASKS_SLUG),
            ("B-aaaa0002", OTHER_SLUG),
            ("B-aaaa0003", "no-such-flow"),
            ("B-aaaa0004", "external:GH-42"),
        ],
    );

    let all = reconcile(&root, &[]);
    assert_eq!(
        bucket_ids(&all, "unlinked"),
        ["B-aaaa0001", "B-aaaa0002"],
        "{all}"
    );

    let report = reconcile(&root, &["--flow", OTHER_SLUG]);
    assert_eq!(report["flow"], OTHER_SLUG);
    assert_eq!(bucket_ids(&report, "unlinked"), ["B-aaaa0002"], "{report}");
    assert_eq!(bucket_ids(&report, "dangling"), Vec::<String>::new());
    assert_eq!(bucket_ids(&report, "external"), Vec::<String>::new());
}

#[test]
fn a_run_without_flags_leaves_the_store_and_sidecar_byte_identical() {
    let (_tmp, root) = sandbox();
    let tasks = seed_tasks(&root, "schema_version = 1\n");
    let added = backlog(
        &root,
        &[
            "add",
            "--summary",
            "reconcile fixture",
            "--kind",
            "bug",
            "--area",
            "tomlctl/src/backlog/reconcile.rs",
        ],
    );
    let id = added["id"].as_str().unwrap().to_string();
    backlog(&root, &["triage", &id, "--promote", "--to", TASKS_SLUG]);
    fs::write(
        &tasks,
        tasks_toml(
            TASKS_SLUG,
            &[("one", "done", "abc1234", &format!("Closes {id}."))],
            &[("one", &[id.as_str()], &[])],
        ),
    )
    .unwrap();

    let path = store_path(&root);
    let before = (fs::read(&path).unwrap(), fs::read(sidecar(&path)).unwrap());
    let tasks_before = fs::read(&tasks).unwrap();

    let report = reconcile(&root, &[]);
    assert_eq!(
        bucket_ids(&report, "ready"),
        std::slice::from_ref(&id),
        "{report}"
    );
    assert_eq!(report["applied"], json!([]));
    assert_eq!(report["adopted"], json!([]));
    assert_eq!(
        (fs::read(&path).unwrap(), fs::read(sidecar(&path)).unwrap()),
        before,
        "a run without flags must not write the backlog or its sidecar"
    );
    assert_eq!(fs::read(&tasks).unwrap(), tasks_before);
}
