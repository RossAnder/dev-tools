//! Black-box coverage for `tomlctl tasks train` — the commit groups a
//! checkpoint stages, in the order it commits them.

use serde_json::{Value, json};
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, parse_json_error_envelope, sandbox, seed_tasks};

struct Row {
    id: u32,
    status: &'static str,
    checkpoint: &'static str,
    files: &'static [&'static str],
    needs: &'static [u32],
    commit: &'static str,
}

const fn done(
    id: u32,
    checkpoint: &'static str,
    files: &'static [&'static str],
    needs: &'static [u32],
) -> Row {
    Row {
        id,
        status: "done",
        checkpoint,
        files,
        needs,
        commit: "",
    }
}

fn store(granularity: &str, rows: &[Row]) -> String {
    let mut out = format!(
        "schema_version = 1\n\
         plan_path = \"docs/plans/{TASKS_SLUG}.md\"\n\n\
         [policy]\n\
         checkpoints = \"milestones\"\n\
         max_parallel = 6\n\
         commit_granularity = \"{granularity}\"\n\n\
         [[checkpoints]]\n\
         id = \"A\"\n\
         rationale = \"first\"\n\n\
         [[checkpoints]]\n\
         id = \"B\"\n\
         rationale = \"second\"\n"
    );
    for row in rows {
        let files: Vec<String> = row.files.iter().map(|f| format!("\"{f}\"")).collect();
        let needs: Vec<String> = row.needs.iter().map(u32::to_string).collect();
        out.push_str(&format!(
            "\n[[items]]\n\
             id = {id}\n\
             ref = \"task-{id}\"\n\
             title = \"Task {id}\"\n\
             effort = \"S\"\n\
             status = \"{status}\"\n\
             checkpoint = \"{checkpoint}\"\n\
             files = [{files}]\n\
             needs = [{needs}]\n\
             commit = \"{commit}\"\n",
            id = row.id,
            status = row.status,
            checkpoint = row.checkpoint,
            files = files.join(", "),
            needs = needs.join(", "),
            commit = row.commit,
        ));
    }
    out
}

fn train(root: &Path, args: &[&str]) -> Value {
    let out = cli(root)
        .args(["tasks", "train", "--slug", TASKS_SLUG])
        .args(args)
        .write_stdin("")
        .assert()
        .success();
    let text = String::from_utf8(out.get_output().stdout.clone()).expect("stdout must be UTF-8");
    serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("`tasks train` stdout must be JSON: {e}; got: {text}"))
}

fn staged(granularity: &str, rows: &[Row]) -> (tempfile::TempDir, std::path::PathBuf) {
    let (tmp, root) = sandbox();
    seed_tasks(&root, &store(granularity, rows));
    (tmp, root)
}

fn group_ids(report: &Value) -> Vec<Vec<u64>> {
    report["groups"]
        .as_array()
        .unwrap_or_else(|| panic!("expected `groups`, got {report}"))
        .iter()
        .map(|group| {
            group["ids"]
                .as_array()
                .expect("each group carries ids")
                .iter()
                .map(|id| id.as_u64().expect("an integer id"))
                .collect()
        })
        .collect()
}

#[test]
fn per_task_groups_merge_two_tasks_that_share_a_file() {
    let (_tmp, root) = staged(
        "per-task",
        &[
            done(1, "A", &["a.rs", "shared.rs"], &[]),
            done(2, "A", &["b.rs"], &[]),
            done(3, "A", &["shared.rs", "c.rs"], &[]),
        ],
    );

    let report = train(&root, &[]);
    assert_eq!(report["granularity"], "per-task", "{report}");
    assert_eq!(
        report["groups"][0],
        json!({
            "ids": [1, 3],
            "refs": ["task-1", "task-3"],
            "files": ["a.rs", "c.rs", "shared.rs"],
            "checkpoints": ["A"],
            "shared_with_pending": [],
        }),
        "{report}"
    );
    assert_eq!(group_ids(&report), vec![vec![1, 3], vec![2]], "{report}");
}

#[test]
fn per_checkpoint_groups_one_commit_per_checkpoint() {
    let (_tmp, root) = staged(
        "per-checkpoint",
        &[
            done(1, "A", &["a.rs"], &[]),
            done(2, "B", &["b.rs"], &[1]),
            done(3, "A", &["c.rs"], &[]),
            done(4, "B", &["d.rs"], &[]),
        ],
    );

    let report = train(&root, &[]);
    assert_eq!(group_ids(&report), vec![vec![1, 3], vec![2, 4]], "{report}");
    assert_eq!(report["groups"][1]["checkpoints"], json!(["B"]), "{report}");
}

#[test]
fn granularity_flag_overrides_the_policy_and_single_commit_is_one_group() {
    let (_tmp, root) = staged(
        "per-task",
        &[done(1, "A", &["a.rs"], &[]), done(2, "B", &["b.rs"], &[1])],
    );

    let report = train(&root, &["--granularity", "single-commit"]);
    assert_eq!(report["granularity"], "single-commit", "{report}");
    assert_eq!(group_ids(&report), vec![vec![1, 2]], "{report}");
    assert_eq!(
        report["groups"][0]["checkpoints"],
        json!(["A", "B"]),
        "{report}"
    );
}

/// 1 needs 2, so 2 commits first even though its id is higher; 3 is free and
/// shares 2's layer, where the lower id goes first.
#[test]
fn groups_commit_in_dependency_order_with_lowest_id_first_within_a_layer() {
    let (_tmp, root) = staged(
        "per-task",
        &[
            done(1, "A", &["a.rs"], &[2]),
            done(2, "A", &["b.rs"], &[]),
            done(3, "A", &["c.rs"], &[]),
        ],
    );

    let report = train(&root, &[]);
    assert_eq!(
        group_ids(&report),
        vec![vec![2], vec![3], vec![1]],
        "{report}"
    );
}

/// 1 and 3 share a file, 3 needs 2 and 2 needs 1: the merged {1, 3} and {2}
/// each depend on the other, so the three collapse into one commit.
#[test]
fn a_dependency_cycle_between_merged_groups_collapses_into_one_group() {
    let (_tmp, root) = staged(
        "per-task",
        &[
            done(1, "A", &["shared.rs"], &[]),
            done(2, "A", &["b.rs"], &[1]),
            done(3, "A", &["shared.rs"], &[2]),
            done(4, "A", &["d.rs"], &[3]),
        ],
    );

    let report = train(&root, &[]);
    assert_eq!(group_ids(&report), vec![vec![1, 2, 3], vec![4]], "{report}");
}

#[test]
fn a_file_a_pending_row_also_claims_is_reported() {
    let (_tmp, root) = staged(
        "per-task",
        &[
            done(1, "A", &["a.rs", "shared.rs"], &[]),
            Row {
                id: 2,
                status: "pending",
                checkpoint: "B",
                files: &["shared.rs", "z.rs"],
                needs: &[],
                commit: "",
            },
            Row {
                id: 3,
                status: "in-progress",
                checkpoint: "B",
                files: &["a.rs"],
                needs: &[],
                commit: "",
            },
        ],
    );

    let report = train(&root, &[]);
    assert_eq!(group_ids(&report), vec![vec![1]], "{report}");
    assert_eq!(
        report["groups"][0]["shared_with_pending"],
        json!(["a.rs", "shared.rs"]),
        "{report}"
    );
}

/// 2 is committed, so it is no candidate, but its edges still order the two
/// groups either side of it: 1 waits on 3 through 2.
#[test]
fn committed_rows_are_excluded_but_still_order_the_rest() {
    let (_tmp, root) = staged(
        "per-task",
        &[
            done(1, "A", &["a.rs"], &[2]),
            Row {
                commit: "abc1234",
                ..done(2, "A", &["b.rs"], &[3])
            },
            done(3, "A", &["c.rs"], &[]),
        ],
    );

    let report = train(&root, &[]);
    assert_eq!(group_ids(&report), vec![vec![3], vec![1]], "{report}");
}

#[test]
fn the_window_narrows_to_named_checkpoints_or_ids() {
    let (_tmp, root) = staged(
        "per-task",
        &[
            done(1, "A", &["a.rs"], &[]),
            done(2, "B", &["b.rs"], &[]),
            done(3, "B", &["c.rs"], &[]),
        ],
    );

    let by_checkpoint = train(&root, &["--checkpoint", "B"]);
    assert_eq!(
        group_ids(&by_checkpoint),
        vec![vec![2], vec![3]],
        "{by_checkpoint}"
    );
    let by_ids = train(&root, &["--ids", "1,3"]);
    assert_eq!(group_ids(&by_ids), vec![vec![1], vec![3]], "{by_ids}");
}

#[test]
fn an_unknown_checkpoint_is_a_validation_error() {
    let (_tmp, root) = staged("per-task", &[done(1, "A", &["a.rs"], &[])]);

    let out = cli(&root)
        .args([
            "--error-format",
            "json",
            "tasks",
            "train",
            "--slug",
            TASKS_SLUG,
            "--checkpoint",
            "Z",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr));
    assert_eq!(err["kind"], "validation", "{err}");
}
