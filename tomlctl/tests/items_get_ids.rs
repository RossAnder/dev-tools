//! `items get` takes one or several ids: one id prints the item, several print
//! an array in the order given, and an unknown id fails the whole call.

use serde_json::Value;
use std::path::Path;

mod common;
use common::{cli, sandbox, seed_ledger_in};

const LEDGER: &str = r#"schema_version = 1
last_updated = 2026-01-01

[[items]]
id = "R1"
summary = "first"

[[items]]
id = "R2"
summary = "second"

[[items]]
id = "R3"
summary = "third"

[[rollback_events]]
id = "E1"
note = "event"
"#;

fn get(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    cli(root).args(["items", "get"]).args(args).assert()
}

fn json(root: &Path, args: &[&str]) -> Value {
    let out = get(root, args).success().get_output().stdout.clone();
    serde_json::from_slice(&out).expect("stdout is JSON")
}

fn ids(v: &Value) -> Vec<&str> {
    v.as_array()
        .expect("an array")
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect()
}

#[test]
fn one_id_prints_a_single_object() {
    let (_dir, root) = sandbox();
    let file = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let out = json(&root, &[file.to_str().unwrap(), "R2"]);
    assert_eq!(out["id"], "R2");
    assert_eq!(out["summary"], "second");
}

#[test]
fn comma_and_space_lists_print_an_array_in_the_order_given() {
    let (_dir, root) = sandbox();
    let file = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let f = file.to_str().unwrap();
    assert_eq!(ids(&json(&root, &[f, "R3,R1"])), ["R3", "R1"]);
    assert_eq!(ids(&json(&root, &[f, "R3", "R1"])), ["R3", "R1"]);
}

#[test]
fn a_repeated_id_prints_once() {
    let (_dir, root) = sandbox();
    let file = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let f = file.to_str().unwrap();
    assert_eq!(ids(&json(&root, &[f, "R1,R2,R1"])), ["R1", "R2"]);
}

#[test]
fn an_unknown_id_fails_the_whole_call_naming_it() {
    let (_dir, root) = sandbox();
    let file = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let f = file.to_str().unwrap();
    get(&root, &[f, "R1,R9"])
        .failure()
        .stdout("")
        .stderr(predicates::str::contains("no item with id = R9"));
}

#[test]
fn array_is_still_honoured() {
    let (_dir, root) = sandbox();
    let file = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let f = file.to_str().unwrap();
    let out = json(&root, &[f, "E1", "--array", "rollback_events"]);
    assert_eq!(out["note"], "event");
}
