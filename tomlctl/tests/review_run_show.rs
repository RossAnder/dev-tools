//! `review run show`: one run read back from the store, with signal roots
//! limited to the run's scope and waves derived from its cells.

mod common;

use std::fs;
use std::path::Path;

use common::{cli, sandbox};
use serde_json::{Value as JsonValue, json};

const SCOPE: &str = "t";

const STORE: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-11-10T08:00:00Z"
base = "aaaaaaa"
head = "bbbbbbb"
scope = ["tomlctl/src/review/**", "docs/x.md"]
agents = 7

[runs.baselines]
lint-baseline = "logs/lint.txt"
profile-baseline = "none — no profiler"

[runs.audit]
sampled = 4

[[runs.signals]]
name = "lang:rust"
source = "detector"
roots = ["tomlctl", "tomlctl/src/review/cells", "lumina", "glimpse/src"]

[[runs.signals]]
name = "trust-boundary"
source = "judged"
accepted = true
roots = ["."]
evidence = ["tomlctl/src/review/add.rs"]

[[runs]]
id = "U2"
command = "review-v2"
family = "code"
depth = "quick"
started = "2026-11-10T09:00:00Z"
base = "ccccccc"

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["tomlctl/src/review/**"]
status = "assigned"

[[cells]]
id = "C2"
run = "U1"
wave = 3
slice = ["docs/x.md"]
status = "assigned"

[[cells]]
id = "C3"
run = "U2"
wave = 5
slice = ["docs/x.md"]
status = "assigned"
"#;

fn store(root: &Path) {
    let path = root.join(format!(".claude/review-v2/{SCOPE}.toml"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, STORE).unwrap();
}

fn show(root: &Path, run: &str) -> JsonValue {
    let out = cli(root)
        .args(["review", "run", "show", run, "--scope", SCOPE])
        .assert()
        .success();
    serde_json::from_slice(&out.get_output().stdout).expect("run show prints JSON")
}

#[test]
fn run_show_prints_signals_with_roots_limited_to_scope() {
    let (_dir, root) = sandbox();
    store(&root);
    let report = show(&root, "U1");
    assert_eq!(report["run"], "U1");
    assert_eq!(report["base"], "aaaaaaa");
    assert_eq!(report["head"], "bbbbbbb");
    assert_eq!(report["depth"], "standard");
    assert_eq!(
        report["scope"],
        json!(["tomlctl/src/review/**", "docs/x.md"])
    );

    let rust = &report["signals"][0];
    assert_eq!(rust["name"], "lang:rust");
    assert_eq!(
        rust["roots"],
        json!(["tomlctl", "tomlctl/src/review/cells"]),
        "a root above or under a scope entry stays; the rest are counted"
    );
    assert_eq!(rust["roots_outside_scope"], 2);

    let judged = &report["signals"][1];
    assert_eq!(judged["roots"], json!(["."]));
    assert_eq!(judged["roots_outside_scope"], 0);
    assert_eq!(judged["accepted"], true);
    assert_eq!(judged["evidence"], json!(["tomlctl/src/review/add.rs"]));
}

#[test]
fn run_show_prints_recorded_baselines() {
    let (_dir, root) = sandbox();
    store(&root);
    let report = show(&root, "U1");
    assert_eq!(
        report["baselines"],
        json!({
            "lint-baseline": "logs/lint.txt",
            "profile-baseline": "none — no profiler",
        })
    );
    assert_eq!(report["audit"], json!({"sampled": 4}));
    assert_eq!(report["agents"], 7);

    let bare = show(&root, "U2");
    assert_eq!(bare["baselines"], json!({}), "no baselines reads empty");
    assert!(bare["audit"].is_null(), "{bare}");
    assert!(bare["head"].is_null(), "{bare}");
    assert_eq!(bare["signals"], json!([]));
}

#[test]
fn run_show_derives_waves_from_cells() {
    let (_dir, root) = sandbox();
    store(&root);
    assert_eq!(show(&root, "U1")["waves"], 3);
    assert_eq!(show(&root, "U2")["waves"], 5);
}

#[test]
fn run_show_refuses_an_unknown_run() {
    let (_dir, root) = sandbox();
    store(&root);
    let out = cli(&root)
        .args(["--error-format", "json"])
        .args(["review", "run", "show", "U9", "--scope", SCOPE])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("not_found"), "{stderr}");
    assert!(stderr.contains("U9"), "{stderr}");
}
