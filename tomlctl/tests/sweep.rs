//! Black-box coverage for `tomlctl sweep` and `tomlctl items sweep`.
//!
//! Every case runs the real binary over a sandbox repo, so the patterns use
//! the `(?-u:…)` forms the shipped build accepts: `cargo test` links Unicode
//! tables through a dev-dependency and would pass a `\b` the binary refuses.
//! Files are written, never committed — the enumeration lists untracked
//! files too. The ledger sits under `.claude/`, which the default excludes
//! cover, so its verbatim `sweep` strings must never surface as sites.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{
    assert_sidecar_matches, cli, git_available, parse_json_error_envelope, sandbox, seed_ledger_in,
};

const PATTERN: &str = r"(?-u:\bneedle\b)";
const LEDGER: &str = "review-ledger.toml";

/// `R1` is anchored on `src/a.rs:needle` twice over — through `file` +
/// `symbol` and through `instances` — and on a line `src/a.rs` no longer
/// reaches. `src/b.rs` is the site nobody recorded.
const LEDGER_FIXTURE: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
symbol = "needle"
instances = ["src/a.rs:needle", "src/a.rs:99"]
sweep = ['(?-u:\bneedle\b)']
enumeration = "complete"
"#;

fn write(root: &Path, rel: &str, bytes: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// `src/a.rs` carries `needle` on line 2, `src/b.rs` on line 3.
fn seed_sources(root: &Path) {
    write(root, "src/a.rs", "fn other() {}\nfn needle() {}\n");
    write(root, "src/b.rs", "fn x() {}\n\nneedle();\n");
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

fn instances_of(ledger: &Path, id: &str) -> Vec<String> {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(ledger).unwrap()).unwrap();
    let item = doc["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("no item {id} in {}", ledger.display()));
    item.get("instances")
        .and_then(toml::Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn sidecar_of(ledger: &Path) -> std::path::PathBuf {
    let mut s = ledger.as_os_str().to_os_string();
    s.push(".sha256");
    s.into()
}

#[test]
fn sweep_reports_each_site_once_with_full_coverage() {
    if !git_available() {
        return;
    }
    let (_dir, root) = sandbox();
    seed_sources(&root);

    let out = run_json(&root, &["sweep", "-e", PATTERN]);

    assert_eq!(
        out["hits"],
        json!([
            { "file": "src/a.rs", "line": 2, "pattern": 0 },
            { "file": "src/b.rs", "line": 3, "pattern": 0 },
        ]),
        "{out}"
    );
    assert_eq!(
        out["skipped"],
        json!({ "binary": 0, "oversize": 0, "unreadable": 0, "unenumerated": 0 })
    );
    assert_eq!(out["truncated"], json!(false));
    assert_eq!(out["coverage_complete"], json!(true));
}

#[test]
fn sweep_outside_a_repo_is_a_tagged_error() {
    if !git_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(&root, "src/a.rs", "needle\n");

    let out = cli(&root)
        .args(["--error-format", "json", "sweep", "-e", PATTERN])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    let err = parse_json_error_envelope(&stderr);

    assert_eq!(err["kind"], json!("other"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(
        message.starts_with("git ls-files failed"),
        "message must name the enumeration step, got: {message}"
    );
}

#[test]
fn items_sweep_partitions_new_and_gone_and_never_sees_its_ledger() {
    if !git_available() {
        return;
    }
    let (_dir, root) = sandbox();
    seed_sources(&root);
    let ledger = seed_ledger_in(&root, LEDGER, LEDGER_FIXTURE);

    let out = run_json(&root, &["items", "sweep", ledger.to_str().unwrap()]);

    let items = out["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{out}");
    let r1 = &items[0];
    assert_eq!(r1["id"], json!("R1"));
    assert_eq!(r1["new"], json!(["src/b.rs:3"]), "{r1}");
    assert_eq!(
        r1["gone"],
        json!([{ "anchor": "src/a.rs:99", "reason": "no-hit" }]),
        "{r1}"
    );
    assert_eq!(r1["kept"], json!(["src/a.rs:needle"]), "{r1}");
    assert_eq!(r1["unverified"], json!([]), "{r1}");
    assert_eq!(r1["recorded"], json!(["src/a.rs"]), "{r1}");
    assert_eq!(r1["found"], json!(["src/a.rs", "src/b.rs"]), "{r1}");
    assert_eq!(r1["coverage_complete"], json!(true), "{r1}");
    assert_eq!(r1["truncated"], json!(false), "{r1}");
    assert_eq!(out["skipped_items"], json!([]));
    assert_eq!(out["coverage_complete"], json!(true));
    assert_eq!(out["truncated"], json!(false));
    assert!(
        !out.to_string().contains(LEDGER),
        "the ledger's own sweep strings surfaced as sites: {out}"
    );
}

#[test]
fn items_sweep_update_dry_run_prints_the_plan_and_writes_nothing() {
    if !git_available() {
        return;
    }
    let (_dir, root) = sandbox();
    seed_sources(&root);
    let ledger = seed_ledger_in(&root, LEDGER, LEDGER_FIXTURE);
    let before = fs::read(&ledger).unwrap();

    let out = run_json(
        &root,
        &[
            "items",
            "sweep",
            ledger.to_str().unwrap(),
            "--update",
            "--dry-run",
        ],
    );

    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!(out["dry_run"], json!(true), "{out}");
    let wc = &out["would_change"];
    assert_eq!(wc["kind"], json!("items"), "{out}");
    assert_eq!(wc["updated"], json!(1), "{out}");
    assert_eq!(wc["ids"], json!(["R1"]), "{out}");
    assert_eq!(
        fs::read(&ledger).unwrap(),
        before,
        "dry-run rewrote the ledger"
    );
    assert!(
        !sidecar_of(&ledger).exists(),
        "dry-run must not mint a sidecar"
    );
}

#[test]
fn items_sweep_update_rewrites_instances_and_is_idempotent() {
    if !git_available() {
        return;
    }
    let (_dir, root) = sandbox();
    seed_sources(&root);
    let ledger = seed_ledger_in(&root, LEDGER, LEDGER_FIXTURE);
    let ledger_arg = ledger.to_str().unwrap();

    let out = run_json(&root, &["items", "sweep", ledger_arg, "--update"]);
    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!(out["updated"], json!(["R1"]), "{out}");
    assert_eq!(out["created"], json!(false), "{out}");
    assert_eq!(
        instances_of(&ledger, "R1"),
        ["src/a.rs:needle", "src/b.rs:3"],
        "instances must be the kept anchors followed by the new sites"
    );
    assert_sidecar_matches(&ledger);

    let ledger_bytes = fs::read(&ledger).unwrap();
    let sidecar_bytes = fs::read(sidecar_of(&ledger)).unwrap();
    let again = run_json(&root, &["items", "sweep", ledger_arg, "--update"]);
    assert_eq!(again["ok"], json!(true), "{again}");
    assert_eq!(again["updated"], json!([]), "{again}");
    assert_eq!(
        instances_of(&ledger, "R1"),
        ["src/a.rs:needle", "src/b.rs:3"],
        "a second run must not grow the list"
    );
    assert_eq!(
        fs::read(&ledger).unwrap(),
        ledger_bytes,
        "no-change run rewrote the ledger"
    );
    assert_eq!(
        fs::read(sidecar_of(&ledger)).unwrap(),
        sidecar_bytes,
        "no-change run rewrote the sidecar"
    );
}
