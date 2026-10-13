//! Black-box coverage for `tomlctl review catalogue` over temp-dir catalogues.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

const VOCABULARY: &str = r#"
[signals."lang:rust"]
kind = "stack"
meaning = "Rust sources"
[signals."lang:rust".detect]
files = ["**/Cargo.toml"]

[concerns.logic]
meaning = "wrong output for a nameable input"
families = ["code"]

[routing]
executable = ["**/*.rs"]
tests = ["**/tests/**"]
"#;

const CORRECTNESS: &str = r#"+++
id = "correctness"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect"]
owns = ["logic"]
activation = "floor"
slice = "per-subsystem"
+++
## Question
Does it compute the right thing?

## Owns
`logic` — wrong output.

## Probes — code
- off-by-one in ranges

## Does not flag
- confirmations
"#;

const RUST: &str = r#"+++
id = "rust"
status = "stable"
families = ["code"]
apply_when = { signals = ["lang:rust"] }
attaches_to = ["*"]
+++
## General
Ownership pitfalls.
"#;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn catalogue() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let builtin = tmp.path().join("builtin");
    write(&builtin, "vocabulary.toml", VOCABULARY);
    write(&builtin, "lenses/correctness.md", CORRECTNESS);
    write(&builtin, "facets/rust.md", RUST);
    tmp
}

fn tomlctl(tmp: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", tmp.path())
        .args(["review", "catalogue", "--builtin"])
        .arg(tmp.path().join("builtin"));
    cmd
}

fn run_json(tmp: &tempfile::TempDir, extra: &[&str]) -> Value {
    let out = tomlctl(tmp).args(extra).assert().success();
    serde_json::from_slice(&out.get_output().stdout).expect("stdout is JSON")
}

#[test]
fn catalogue_prints_merged_index_with_hashes() {
    let tmp = catalogue();
    let json = run_json(&tmp, &[]);
    let entries = json["entries"].as_array().unwrap();
    let ids: Vec<&str> = entries.iter().filter_map(|e| e["id"].as_str()).collect();
    assert_eq!(ids, ["correctness", "rust"]);
    let lens = &entries[0];
    assert_eq!(lens["hash"].as_str().unwrap().len(), 64);
    assert_eq!(lens["layer"], "builtin");
    assert_eq!(json["tree_sha"].as_str().unwrap().len(), 64);
}

#[test]
fn catalogue_signals_marks_matched() {
    let tmp = catalogue();
    let json = run_json(&tmp, &["--signals", "lang:rust"]);
    let entries = json["entries"].as_array().unwrap();
    let rust = entries.iter().find(|e| e["id"] == "rust").unwrap();
    assert_eq!(rust["matched"], serde_json::json!(["lang:rust"]));
    let lens = entries.iter().find(|e| e["id"] == "correctness").unwrap();
    assert_eq!(lens["matched"], serde_json::json!([]));

    let json = run_json(&tmp, &[]);
    let rust = json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "rust")
        .unwrap();
    assert_eq!(rust["matched"], serde_json::json!([]));
}

#[test]
fn catalogue_owners_prints_generated_map() {
    let tmp = catalogue();
    let json = run_json(&tmp, &["--owners"]);
    assert_eq!(
        json["owners"],
        serde_json::json!([{"family": "code", "tag": "logic", "owners": ["correctness"]}])
    );
}

#[test]
fn catalogue_check_exits_nonzero_on_invalid() {
    let tmp = catalogue();
    tomlctl(&tmp).arg("--check").assert().success();

    fs::remove_file(tmp.path().join("builtin/lenses/correctness.md")).unwrap();
    let out = tomlctl(&tmp).arg("--check").assert().failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(stderr.contains("logic"), "stderr: {stderr}");
}
