//! Black-box coverage for the `tomlctl backlog` read verbs — `check`,
//! `list`, `show`, `cluster` and `evidence audit`.
//!
//! The `list` / `show` / `cluster` cases seed the store by hand, because what
//! they assert is a projection over a known row set rather than anything
//! `add` derives. The `check` cases go through `add` instead: a verdict is a
//! claim about the fingerprint `add` would land on, so a hand-written
//! `dedup_id` would prove nothing.

use assert_cmd::Command;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{ids_from, parse_json_error_envelope};

const FLAKE_SUMMARY: &str = "pty_readiness_probe flakes on slow CI";
const FLAKE_AREA: &str = "lumina/server/tests/pty_readiness_probe.rs";
const FLAKE_CONTEXT: &str =
    "Only reproduces when the readiness gate races the first prompt write.";

const TOTAL_AREA: &str = "lumina/web/src/checkout/Total.vue";
const TOTAL_SUMMARY: &str = "checkout total overlaps the confirm button below 1400px";
const TOTAL_PARAPHRASE: &str = "checkout total overlaps the confirm button below 1440px";

const GITIGNORE: &str =
    "/.claude/backlog-evidence/*/*\n!/.claude/backlog-evidence/*/.evidence\n";

/// Three rows spanning status ∈ {open, dismissed}, kind ∈ {flaky-test, bug,
/// debt}, and two sibling area prefixes that differ only past a component
/// boundary (`lumina/server` vs `lumina/server-extras`). The `related` edge is
/// one-sided on purpose, so `show` has an inbound-only neighbour to report.
const FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-01

[[backlog]]
id = "B-00000001"
kind = "flaky-test"
summary = "readiness probe flakes on slow ci"
area = "lumina/server/pty/probe.rs"
tags = ["ci", "windows"]
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "1111111111111111"
related = ["B-00000002"]

[[backlog]]
id = "B-00000002"
kind = "bug"
summary = "report totals drift after a renormalise"
area = "lumina/server-extras/report.rs"
tags = ["ci"]
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "2222222222222222"

[[backlog]]
id = "B-00000003"
kind = "debt"
summary = "route table is hand maintained"
area = "lumina/server/http/routes.rs"
tags = ["windows"]
status = "dismissed"
dismissed = 2026-08-01
dismiss_reason = "not worth the churn"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "3333333333333333"
"#;

/// Three open rows whose areas share `lumina/server/pty` but differ in the
/// leaf, which is what the area view has to collapse upward to group.
const CLUSTER_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-01

[[backlog]]
id = "B-000000a1"
kind = "bug"
summary = "spawn drops the first byte"
area = "lumina/server/pty/spawn.rs"
tags = ["pty"]
status = "open"
dedup_id = "a1a1a1a1a1a1a1a1"

[[backlog]]
id = "B-000000a2"
kind = "bug"
summary = "readiness gate races the prompt write"
area = "lumina/server/pty/gate.rs"
tags = ["pty"]
status = "open"
dedup_id = "a2a2a2a2a2a2a2a2"

[[backlog]]
id = "B-000000a3"
kind = "debt"
summary = "conpty shim duplicates the resize path"
area = "lumina/server/pty/conpty.rs"
tags = ["pty"]
status = "open"
dedup_id = "a3a3a3a3a3a3a3a3"
"#;

fn sandbox() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    // `TOMLCTL_ROOT` is canonicalised by the binary, so canonicalise here too
    // or every emitted path fails to relativise against it.
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::write(root.join(".gitignore"), GITIGNORE).unwrap();
    let _ = std::process::Command::new("git")
        .args(["init", "-q", "."])
        .current_dir(&root)
        .output();
    (dir, root)
}

fn store_path(root: &Path) -> PathBuf {
    root.join(".claude").join("backlog.toml")
}

fn evidence_root(root: &Path) -> PathBuf {
    root.join(".claude").join("backlog-evidence")
}

fn seed(root: &Path, body: &str) {
    fs::write(store_path(root), body).unwrap();
}

fn cli(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5");
    cmd
}

/// Run `tomlctl backlog <args…>`, require success, and hand back stdout.
fn backlog_stdout(root: &Path, args: &[&str]) -> String {
    let out = cli(root)
        .arg("backlog")
        .args(args)
        .write_stdin("")
        .assert()
        .success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

fn backlog(root: &Path, args: &[&str]) -> Value {
    let stdout = backlog_stdout(root, args);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "`backlog {}` stdout must be JSON: {e}; got: {stdout}",
            args.join(" ")
        )
    })
}

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// See the sibling helper in `backlog_write.rs`: the clock cannot be moved, so
/// the fixture's terminal date is aged instead.
fn age_terminal_date(store: &Path, field: &str) {
    let text = fs::read_to_string(store).unwrap();
    let needle = format!("{field} = ");
    let mut hit = false;
    let aged: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with(&needle) {
                hit = true;
                format!("{needle}2020-01-01")
            } else {
                line.to_string()
            }
        })
        .collect();
    assert!(hit, "no `{field} = …` line to age in:\n{text}");
    fs::write(store, format!("{}\n", aged.join("\n"))).unwrap();
}

// ---------------------------------------------------------------- check

#[test]
fn check_on_an_absent_store_is_novel() {
    let (_tmp, root) = sandbox();
    assert!(!store_path(&root).exists());

    let v = backlog(&root, &["check", "--summary", FLAKE_SUMMARY]);
    assert_eq!(v["verdict"], json!("novel"));
    assert_eq!(v["candidates"], json!([]));
    assert_eq!(v["dedup_id"].as_str().unwrap().len(), 16);
    assert_eq!(v["thresholds"]["strong"].as_f64(), Some(0.75));
    assert_eq!(v["thresholds"]["related"].as_f64(), Some(0.35));
}

#[test]
fn check_reports_duplicate_on_the_fingerprint_add_minted() {
    let (_tmp, root) = sandbox();
    let added = backlog(
        &root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
            "--context",
            FLAKE_CONTEXT,
        ],
    );

    // The fingerprint spans kind and area, so the probe has to carry both.
    let v = backlog(
        &root,
        &[
            "check",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ],
    );
    assert_eq!(v["verdict"], json!("duplicate"));
    assert_eq!(v["dedup_id"], added["dedup_id"]);
    let hit = &v["candidates"][0];
    assert_eq!(hit["id"], added["id"]);
    assert_eq!(hit["reason"], json!("dedup_id"));
    assert_eq!(hit["status"], json!("open"));
    assert_eq!(hit["context"], json!(FLAKE_CONTEXT));
    assert_eq!(hit["evidence_files"], json!(0));
}

#[test]
fn check_reports_likely_duplicate_for_a_near_paraphrase() {
    let (_tmp, root) = sandbox();
    let added = backlog(
        &root,
        &[
            "add",
            "--summary",
            TOTAL_SUMMARY,
            "--kind",
            "bug",
            "--area",
            TOTAL_AREA,
        ],
    );

    let v = backlog(
        &root,
        &[
            "check",
            "--summary",
            TOTAL_PARAPHRASE,
            "--kind",
            "bug",
            "--area",
            TOTAL_AREA,
        ],
    );
    assert_eq!(v["verdict"], json!("likely-duplicate"));
    assert_ne!(
        v["dedup_id"], added["dedup_id"],
        "a paraphrase that survives normalisation is a distinct fingerprint"
    );
    let hit = &v["candidates"][0];
    assert_eq!(hit["id"], added["id"]);
    assert_eq!(hit["reason"], json!("trigram"));
    let score = hit["score"].as_f64().unwrap();
    assert!((0.75..1.0).contains(&score), "score out of band: {score}");
}

#[test]
fn check_reports_related_by_area_for_an_unrelated_summary() {
    let (_tmp, root) = sandbox();
    let added = backlog(
        &root,
        &[
            "add",
            "--summary",
            "sqlite migration checksum drifts after a renormalise",
            "--kind",
            "bug",
            "--area",
            "tomlctl/src/backlog/add.rs",
        ],
    );

    let v = backlog(
        &root,
        &[
            "check",
            "--summary",
            "dry run preview must leave the sidecar byte identical",
            "--kind",
            "bug",
            "--area",
            "tomlctl/src/backlog/check.rs",
        ],
    );
    assert_eq!(v["verdict"], json!("related"));
    let hit = &v["candidates"][0];
    assert_eq!(hit["id"], added["id"]);
    assert_eq!(hit["reason"], json!("area"));
}

#[test]
fn check_reports_previously_resolved_against_a_compacted_row() {
    let (_tmp, root) = sandbox();
    let added = backlog(
        &root,
        &[
            "add",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
            "--context",
            FLAKE_CONTEXT,
        ],
    );
    let id = added["id"].as_str().unwrap().to_string();
    backlog(
        &root,
        &["triage", &id, "--resolve", "--resolution", "gate now waits"],
    );
    age_terminal_date(&store_path(&root), "resolved");
    let folded = backlog(&root, &["compact", "--older-than", "90d"]);
    assert_eq!(folded["compacted"], json!(1));

    let v = backlog(
        &root,
        &[
            "check",
            "--summary",
            FLAKE_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ],
    );
    assert_eq!(v["verdict"], json!("previously-resolved"));
    let hit = &v["candidates"][0];
    assert_eq!(hit["id"].as_str(), Some(id.as_str()));
    assert_eq!(hit["reason"], json!("compacted"));
    // The stored workaround is the reason an aged-out row is surfaced at all.
    assert_eq!(hit["context"], json!(FLAKE_CONTEXT));
}

// ----------------------------------------------------------------- list

#[test]
fn list_open_selects_only_the_untriaged_rows() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    assert_eq!(
        ids_from(&backlog_stdout(&root, &["list", "--open"])),
        vec!["B-00000001", "B-00000002"]
    );
}

#[test]
fn list_kind_selects_on_the_kind_field() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    assert_eq!(
        ids_from(&backlog_stdout(&root, &["list", "--kind", "flaky-test"])),
        vec!["B-00000001"]
    );
}

#[test]
fn repeated_tag_filters_are_anded() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    assert_eq!(
        ids_from(&backlog_stdout(&root, &["list", "--tag", "ci"])),
        vec!["B-00000001", "B-00000002"]
    );
    assert_eq!(
        ids_from(&backlog_stdout(
            &root,
            &["list", "--tag", "ci", "--tag", "windows"]
        )),
        vec!["B-00000001"]
    );
}

#[test]
fn area_prefix_matches_on_path_component_boundaries() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    // `lumina/server-extras/report.rs` shares the string prefix but not the
    // component, so it must not survive the filter.
    assert_eq!(
        ids_from(&backlog_stdout(
            &root,
            &["list", "--area-prefix", "lumina/server"]
        )),
        vec!["B-00000001", "B-00000003"]
    );
}

#[test]
fn has_evidence_reads_the_drop_box_rather_than_the_store() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    // Marker-only is not evidence; a file is.
    let bare = evidence_root(&root).join("B-00000001");
    fs::create_dir_all(&bare).unwrap();
    fs::write(bare.join(".evidence"), "B-00000001  seeded\n").unwrap();
    let populated = evidence_root(&root).join("B-00000002");
    fs::create_dir_all(&populated).unwrap();
    fs::write(populated.join("trace.log"), b"boom").unwrap();

    assert_eq!(
        ids_from(&backlog_stdout(&root, &["list", "--has-evidence"])),
        vec!["B-00000002"]
    );
}

#[test]
fn count_emits_the_aggregation_shape() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    assert_eq!(
        backlog(&root, &["list", "--count"]),
        json!({ "count": 3 })
    );
}

#[test]
fn the_generic_query_surface_is_threaded_through_list() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);

    let projected = backlog(&root, &["list", "--select", "id,status", "--open"]);
    let rows = projected.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let keys: Vec<&str> = row.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["id", "status"], "unexpected projection: {row}");
    }

    let lines = backlog_stdout(&root, &["list", "--pluck", "id", "--lines"]);
    let plucked: Vec<String> = lines
        .lines()
        .map(|line| serde_json::from_str::<String>(line).unwrap())
        .collect();
    assert_eq!(
        plucked,
        vec!["B-00000001", "B-00000002", "B-00000003"],
        "--pluck --lines emits one quoted id per line"
    );
}

// ----------------------------------------------------------------- show

#[test]
fn show_reports_an_inbound_relation_neighbour() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);

    let v = backlog(&root, &["show", "B-00000002"]);
    assert_eq!(v["item"]["id"], json!("B-00000002"));
    let neighbours = v["neighbours"].as_array().unwrap();
    assert_eq!(neighbours.len(), 1, "{v}");
    assert_eq!(neighbours[0]["id"], json!("B-00000001"));
    assert_eq!(neighbours[0]["relation"], json!("related"));
    assert_eq!(neighbours[0]["direction"], json!("in"));
    assert_eq!(neighbours[0]["status"], json!("open"));
    assert_eq!(
        neighbours[0]["summary"],
        json!("readiness probe flakes on slow ci")
    );
}

#[test]
fn show_distinguishes_the_three_evidence_shapes() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);

    assert_eq!(
        backlog(&root, &["show", "B-00000001"])["evidence"],
        Value::Null,
        "an absent drop-box is null, not an empty listing"
    );

    backlog(&root, &["evidence", "dir", "B-00000001"]);
    let marker_only = backlog(&root, &["show", "B-00000001"]);
    assert_eq!(marker_only["evidence"]["files"], json!([]));
    assert_eq!(
        marker_only["evidence"]["dir"],
        json!(".claude/backlog-evidence/B-00000001")
    );

    let dir = evidence_root(&root).join("B-00000001");
    fs::write(dir.join("a.log"), b"aa").unwrap();
    fs::write(dir.join("b.log"), b"bbb").unwrap();
    let populated = backlog(&root, &["show", "B-00000001"]);
    let files = populated["evidence"]["files"].as_array().unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["a.log", "b.log"],
        "the marker is excluded from the listing"
    );
    assert_eq!(files[0]["bytes"], json!(2));
    assert_eq!(files[1]["bytes"], json!(3));
}

#[test]
fn show_on_an_unknown_id_reports_a_not_found_envelope() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);

    let out = cli(&root)
        .args(["--error-format", "json", "backlog", "show", "B-99999999"])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], json!("not_found"));
}

// -------------------------------------------------------------- cluster

#[test]
fn cluster_by_all_emits_every_view_and_by_area_emits_one() {
    let (_tmp, root) = sandbox();
    seed(&root, CLUSTER_FIXTURE);

    let all = backlog(&root, &["cluster", "--by", "all"]);
    let mut keys: Vec<&str> = all.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["area", "relations", "tags"]);

    let area_only = backlog(&root, &["cluster", "--by", "area"]);
    assert_eq!(
        area_only
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["area"]
    );
}

#[test]
fn cluster_by_area_collapses_siblings_onto_their_shared_prefix() {
    let (_tmp, root) = sandbox();
    seed(&root, CLUSTER_FIXTURE);

    let view = backlog(&root, &["cluster", "--by", "area"]);
    let groups = view["area"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{view}");
    assert_eq!(groups[0]["key"], json!("lumina/server/pty"));
    assert_eq!(groups[0]["size"], json!(3));
    assert_eq!(
        groups[0]["item_ids"],
        json!(["B-000000a1", "B-000000a2", "B-000000a3"])
    );
}

// ------------------------------------------------------- evidence audit

#[test]
fn audit_strict_fails_on_an_unowned_drop_box_and_passes_once_it_is_gone() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    let orphan = evidence_root(&root).join("B-deadbeef");
    fs::create_dir_all(&orphan).unwrap();

    let out = cli(&root)
        .args(["backlog", "evidence", "audit", "--strict"])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let report: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report["root"], json!(".claude/backlog-evidence"));
    assert_eq!(report["counts"]["unowned"], json!(1));
    assert!(
        report["findings"].as_array().unwrap().iter().any(|f| {
            f["class"] == json!("unowned")
                && f["dir"] == json!(".claude/backlog-evidence/B-deadbeef")
        }),
        "{report}"
    );

    fs::remove_dir_all(&orphan).unwrap();
    cli(&root)
        .args(["backlog", "evidence", "audit", "--strict"])
        .write_stdin("")
        .assert()
        .success();
}

#[test]
fn a_force_added_evidence_file_is_tracked_and_strict_still_passes() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    let dir = evidence_root(&root).join("B-00000001");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".evidence"), "B-00000001  seeded\n").unwrap();
    fs::write(dir.join("shot.png"), b"x").unwrap();
    std::process::Command::new("git")
        .args(["add", "-f", ".claude/backlog-evidence/B-00000001/shot.png"])
        .current_dir(&root)
        .output()
        .unwrap();

    let report = backlog(&root, &["evidence", "audit", "--strict"]);
    assert_eq!(report["counts"]["tracked"], json!(1));
    assert_eq!(report["counts"]["git-unavailable"], json!(0));
    assert_eq!(report["counts"]["unowned"], json!(0));
    assert!(
        report["findings"].as_array().unwrap().iter().any(|f| {
            f["class"] == json!("tracked") && f["file"] == json!("shot.png")
        }),
        "{report}"
    );
}
