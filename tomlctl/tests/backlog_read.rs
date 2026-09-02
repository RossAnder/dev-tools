//! Black-box coverage for the `tomlctl backlog` read verbs — `check`,
//! `list`, `show`, `cluster` and `evidence audit`.
//!
//! The `list` / `show` / `cluster` cases seed the store by hand, because what
//! they assert is a projection over a known row set rather than anything
//! `add` derives. The `check` cases go through `add` instead: a verdict is a
//! claim about the fingerprint `add` would land on, so a hand-written
//! `dedup_id` would prove nothing.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{
    age_terminal_date, backlog, backlog_stdout, cli, git_available, ids_from,
    parse_json_error_envelope, sandbox, store_path,
};

const FLAKE_SUMMARY: &str = "pty_readiness_probe flakes on slow CI";
const FLAKE_AREA: &str = "lumina/server/tests/pty_readiness_probe.rs";
const FLAKE_CONTEXT: &str =
    "Only reproduces when the readiness gate races the first prompt write.";

const TOTAL_AREA: &str = "lumina/web/src/checkout/Total.vue";
const TOTAL_SUMMARY: &str = "checkout total overlaps the confirm button below 1400px";
const TOTAL_PARAPHRASE: &str = "checkout total overlaps the confirm button below 1440px";

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

/// Two rows sharing an id but not a fingerprint — what a text merge of two
/// worktrees leaves behind. They are the whole store, so no weaker rung can
/// contribute a candidate and the verdict is the collision alone.
const COLLIDING_ID_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-01

[[backlog]]
id = "B-00000001"
kind = "flaky-test"
summary = "readiness probe flakes on slow ci"
area = "lumina/server/pty/probe.rs"
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "1111111111111111"

[[backlog]]
id = "B-00000001"
kind = "bug"
summary = "guard_write_path accepts a symlinked leaf"
area = "tomlctl/src/io.rs"
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "4444444444444444"
"#;

fn evidence_root(root: &Path) -> PathBuf {
    root.join(".claude").join("backlog-evidence")
}

fn seed(root: &Path, body: &str) {
    fs::write(store_path(root), body).unwrap();
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

/// A summary is text some other agent wrote. The `-` sentinel is what lets it
/// reach the gate as data instead of as a shell token, so the verdict it earns
/// has to match the one the same text earns as a literal.
#[test]
fn check_reads_the_summary_from_stdin_under_the_dash_sentinel() {
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

    let out = cli(&root)
        .arg("backlog")
        .args([
            "check",
            "--summary",
            "-",
            "--kind",
            "flaky-test",
            "--area",
            FLAKE_AREA,
        ])
        // The trailing newline a heredoc supplies is not part of the summary:
        // keep it and the fingerprint stops matching the flag-minted row.
        .write_stdin(format!("{FLAKE_SUMMARY}\n"))
        .assert()
        .success();

    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["verdict"], json!("duplicate"));
    assert_eq!(v["dedup_id"], added["dedup_id"]);
    assert_eq!(v["candidates"][0]["id"], added["id"]);
    assert_eq!(v["candidates"][0]["context"], json!(FLAKE_CONTEXT));
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

#[test]
fn check_reports_duplicate_id_for_rows_sharing_an_id() {
    let (_tmp, root) = sandbox();
    seed(&root, COLLIDING_ID_FIXTURE);

    // The probe matches neither row on text nor on structure: a collision
    // makes every later id lookup ambiguous, so it is reported whatever was
    // asked.
    let v = backlog(
        &root,
        &[
            "check",
            "--summary",
            "sidecar rename races the antivirus scanner",
            "--kind",
            "bug",
            "--area",
            "statusline/src/render.rs",
        ],
    );
    assert_eq!(v["verdict"], json!("duplicate-id"));
    let candidates = v["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2, "{v}");
    for hit in candidates {
        assert_eq!(hit["id"], json!("B-00000001"));
        assert_eq!(hit["reason"], json!("duplicate-id"));
        assert_eq!(hit["status"], json!("open"));
    }
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
fn a_lowered_max_bytes_flags_a_file_the_default_threshold_clears() {
    let (_tmp, root) = sandbox();
    seed(&root, FIXTURE);
    let dir = evidence_root(&root).join("B-00000001");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".evidence"), "B-00000001  seeded\n").unwrap();
    fs::write(dir.join("shot.png"), b"xx").unwrap();

    let default = backlog(&root, &["evidence", "audit"]);
    assert_eq!(default["counts"]["oversize"], json!(0), "{default}");

    let lowered = backlog(&root, &["evidence", "audit", "--max-bytes", "1"]);
    assert_eq!(lowered["counts"]["oversize"], json!(1), "{lowered}");
    assert!(
        lowered["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| { f["class"] == json!("oversize") && f["file"] == json!("shot.png") }),
        "{lowered}"
    );
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
    let add = std::process::Command::new("git")
        .args(["add", "-f", ".claude/backlog-evidence/B-00000001/shot.png"])
        .current_dir(&root)
        .output()
        .unwrap();
    // An add that ran and failed would otherwise surface below as a bare
    // tracked-count mismatch, with nothing naming git as the cause.
    assert!(
        add.status.success(),
        "`git add -f` failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&add.stderr)
    );

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
