//! Black-box coverage for `tomlctl items clusters` and the `instance-missing`
//! class of `tomlctl items orphans`.
//!
//! The source files an item names are written under the sandbox root, so the
//! file keys the binary emits are the canonical relative spellings rather
//! than whatever the ledger wrote; `src/missing.rs` is deliberately absent so
//! its verbatim spelling survives and `items orphans` has one anchor to fail.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{cli, parse_json_error_envelope, sandbox, seed_ledger_in};

const LEDGER: &str = "review-ledger.toml";

/// `R1` and `R2` share `src/b.rs` (through `R1`'s instances and `R2`'s
/// oddly-spelled `file`) and so cluster; `R3` depends on `R1` and shares
/// `src/c.rs` with `R4`, which is in the first layer because its only
/// dependency, `R9`, is outside the selection.
const LEDGER_FIXTURE: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
instances = ["src/a.rs:alpha", "src/b.rs:beta"]

[[items]]
id = "R2"
status = "open"
file = "src/../src/b.rs"

[[items]]
id = "R3"
status = "open"
file = "src/c.rs"
depends_on = ["R1"]

[[items]]
id = "R4"
status = "open"
file = "src/c.rs"
instances = ["src/missing.rs:7"]
depends_on = ["R9"]
"#;

const CYCLIC_FIXTURE: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
depends_on = ["R2"]

[[items]]
id = "R2"
status = "open"
file = "src/b.rs"
depends_on = ["R1"]
"#;

fn write(root: &Path, rel: &str, bytes: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn seed_sources(root: &Path) {
    write(root, "src/a.rs", "fn alpha() {}\n");
    write(root, "src/b.rs", "fn beta() {}\n");
    write(root, "src/c.rs", "fn gamma() {}\n");
}

fn run_json(root: &Path, args: &[&str]) -> Value {
    let out = cli(root).args(args).write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "`{}` stdout must be JSON: {e}; got: {stdout}",
            args.join(" ")
        )
    })
}

fn run_error(root: &Path, args: &[&str]) -> Value {
    let out = cli(root)
        .args(["--error-format", "json"])
        .args(args)
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    parse_json_error_envelope(&stderr)
}

fn cluster_field<'a>(out: &'a Value, field: &str) -> Vec<&'a Value> {
    out["clusters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| &c[field])
        .collect()
}

#[test]
fn clusters_join_shared_files_within_a_layer_and_batch_dependents_later() {
    let (_dir, root) = sandbox();
    seed_sources(&root);
    let ledger = seed_ledger_in(&root, LEDGER, LEDGER_FIXTURE);

    let out = run_json(&root, &["items", "clusters", ledger.to_str().unwrap()]);

    assert_eq!(
        cluster_field(&out, "id"),
        [&json!("c1"), &json!("c2"), &json!("c3")],
        "{out}"
    );
    assert_eq!(
        cluster_field(&out, "item_ids"),
        [&json!(["R1", "R2"]), &json!(["R3"]), &json!(["R4"])],
        "{out}"
    );
    assert_eq!(
        cluster_field(&out, "files"),
        [
            &json!(["src/a.rs", "src/b.rs"]),
            &json!(["src/c.rs"]),
            &json!(["src/c.rs", "src/missing.rs"]),
        ],
        "{out}"
    );
    assert_eq!(
        cluster_field(&out, "depends_on"),
        [&json!([]), &json!(["c1"]), &json!([])],
        "{out}"
    );
    assert_eq!(
        cluster_field(&out, "lite_file_scope"),
        [&json!(true), &json!(true), &json!(true)],
        "{out}"
    );
    assert_eq!(out["batches"], json!([["c1", "c3"], ["c2"]]), "{out}");
    assert_eq!(
        out["dropped_deps"],
        json!([{ "id": "R4", "missing": ["R9"] }]),
        "{out}"
    );
}

#[test]
fn ids_narrow_the_selection_and_its_clustering() {
    let (_dir, root) = sandbox();
    seed_sources(&root);
    let ledger = seed_ledger_in(&root, LEDGER, LEDGER_FIXTURE);

    let out = run_json(
        &root,
        &[
            "items",
            "clusters",
            ledger.to_str().unwrap(),
            "--ids",
            "R3,R1",
        ],
    );

    assert_eq!(
        cluster_field(&out, "item_ids"),
        [&json!(["R1"]), &json!(["R3"])],
        "{out}"
    );
    assert_eq!(
        cluster_field(&out, "depends_on"),
        [&json!([]), &json!(["c1"])],
        "{out}"
    );
    assert_eq!(out["batches"], json!([["c1"], ["c2"]]), "{out}");
    assert_eq!(out["dropped_deps"], json!([]), "{out}");
}

#[test]
fn an_unknown_id_is_not_found() {
    let (_dir, root) = sandbox();
    seed_sources(&root);
    let ledger = seed_ledger_in(&root, LEDGER, LEDGER_FIXTURE);

    let err = run_error(
        &root,
        &[
            "items",
            "clusters",
            ledger.to_str().unwrap(),
            "--ids",
            "R1,R7",
        ],
    );

    assert_eq!(err["kind"], json!("not_found"), "{err}");
    assert!(
        err["message"].as_str().unwrap().contains("R7"),
        "message must name the unknown id, got: {err}"
    );
}

#[test]
fn a_cycle_is_a_validation_error() {
    let (_dir, root) = sandbox();
    seed_sources(&root);
    let ledger = seed_ledger_in(&root, LEDGER, CYCLIC_FIXTURE);

    let err = run_error(&root, &["items", "clusters", ledger.to_str().unwrap()]);

    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert_eq!(
        err["message"],
        json!(
            "refusing items clusters: the dependency graph contains a cycle through items R1, R2"
        ),
        "{err}"
    );
}

#[test]
fn orphans_report_the_one_unresolvable_instance() {
    let (_dir, root) = sandbox();
    seed_sources(&root);
    let ledger = seed_ledger_in(&root, LEDGER, LEDGER_FIXTURE);

    let out = run_json(&root, &["items", "orphans", ledger.to_str().unwrap()]);

    let rows = out.as_array().unwrap();
    assert!(
        rows.iter().all(|row| row["id"] == json!("R4")),
        "R1, R2 and R3 resolve in full, so only R4 may surface: {out}"
    );
    let instance_missing: Vec<&Value> = rows
        .iter()
        .filter(|row| row["class"] == json!("instance-missing"))
        .collect();
    assert_eq!(
        instance_missing,
        [&json!({
            "id": "R4",
            "class": "instance-missing",
            "instance": "src/missing.rs:7",
            "reason": "missing-file",
        })],
        "{out}"
    );
}
