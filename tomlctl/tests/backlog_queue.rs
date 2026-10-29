//! Black-box coverage for the backlog focus queue, `backlog next`, tiers and
//! datetime captures.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{backlog, cli, sandbox, store_path};

fn mint(root: &Path, summary: &str) -> String {
    let added = backlog(root, &["add", "--summary", summary, "--kind", "debt"]);
    added["id"].as_str().unwrap().to_string()
}

fn row(root: &Path, id: &str) -> toml::Value {
    let text = fs::read_to_string(store_path(root)).unwrap();
    let doc: toml::Value = toml::from_str(&text).unwrap();
    doc["backlog"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_str() == Some(id))
        .unwrap()
        .clone()
}

fn queue(out: &Value) -> Vec<String> {
    out["queue"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn next_ids(root: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["next"];
    args.extend_from_slice(extra);
    backlog(root, &args)
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn add_then_at_one_puts_the_new_row_first() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha");
    let b = mint(&root, "beta");
    let c = mint(&root, "gamma");

    let first = backlog(&root, &["focus", &a, &b]);
    assert_eq!(queue(&first), [a.clone(), b.clone()]);
    let second = backlog(&root, &["focus", &c, "--at", "1"]);
    assert_eq!(queue(&second), [c.clone(), a.clone(), b.clone()]);
    assert_eq!(
        second["changed"][0],
        json!({"id": c, "before": null, "after": 1})
    );
    assert_eq!(next_ids(&root, &[]), [c, a, b]);
}

#[test]
fn up_and_down_swap_with_the_neighbour() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha");
    let b = mint(&root, "beta");
    let c = mint(&root, "gamma");
    backlog(&root, &["focus", &a, &b, &c]);

    let up = backlog(&root, &["focus", &c, "--up"]);
    assert_eq!(queue(&up), [a.clone(), c.clone(), b.clone()]);
    let down = backlog(&root, &["focus", &a, "--down"]);
    assert_eq!(queue(&down), [c.clone(), a.clone(), b.clone()]);

    let top = backlog(&root, &["focus", &c, "--up"]);
    assert_eq!(top["changed"], json!([]));
    assert_eq!(queue(&top), [c, a, b]);
}

#[test]
fn remove_drops_the_rank_and_leaves_positions_dense() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha");
    let b = mint(&root, "beta");
    let c = mint(&root, "gamma");
    backlog(&root, &["focus", &a, &b, &c]);

    let out = backlog(&root, &["focus", &b, "--remove"]);
    assert_eq!(queue(&out), [a.clone(), c.clone()]);
    assert!(row(&root, &b).get("focus").is_none());

    let next = backlog(&root, &["next"]);
    assert_eq!(next[0]["id"], json!(a));
    assert_eq!(next[0]["position"], json!(1));
    assert_eq!(next[1]["id"], json!(c));
    assert_eq!(next[1]["position"], json!(2));
    assert_eq!(next.as_array().unwrap().len(), 2);
}

#[test]
fn next_fill_appends_unranked_rows_and_tier_filters() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha");
    let b = mint(&root, "beta");
    let c = mint(&root, "gamma");
    let d = mint(&root, "delta");
    backlog(&root, &["edit", &a, "--tier", "deep"]);
    backlog(&root, &["edit", &b, "--tier", "lite"]);
    backlog(&root, &["edit", &c, "--tier", "human"]);
    backlog(&root, &["focus", &c, &a, &b]);

    assert_eq!(
        next_ids(&root, &["--tier", "lite"]),
        std::slice::from_ref(&b)
    );
    let all = backlog(&root, &["next", "--tier", "lite", "--fill"]);
    let ids: Vec<&str> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [b.as_str(), d.as_str()]);
    assert_eq!(all[0]["position"], json!(3));
    assert!(all[1].get("position").is_none());
}

#[test]
fn fill_orders_unranked_rows_oldest_first() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha");
    let b = mint(&root, "beta");
    let c = mint(&root, "gamma");
    backlog(&root, &["focus", &c]);

    let ids = next_ids(&root, &["--fill"]);
    assert_eq!(ids[0], c);
    let mut rest = ids[1..].to_vec();
    rest.sort();
    let mut want = vec![a, b];
    want.sort();
    assert_eq!(rest, want);
}

#[test]
fn edit_tier_sets_and_clears() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha");

    let set = backlog(&root, &["edit", &a, "--tier", "deep"]);
    assert_eq!(set["rows"][0]["changed"], json!(true));
    assert_eq!(row(&root, &a)["tier"].as_str(), Some("deep"));

    let clear = backlog(&root, &["edit", &a, "--clear-tier"]);
    assert_eq!(clear["rows"][0]["changed"], json!(true));
    assert!(row(&root, &a).get("tier").is_none());

    cli(&root)
        .args(["backlog", "edit", &a, "--tier", "bogus"])
        .write_stdin("")
        .assert()
        .failure();
}

#[test]
fn fresh_captures_carry_a_z_datetime_the_date_filter_finds() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha");

    let r = row(&root, &a);
    let created = r["created"].as_datetime().expect("created is a datetime");
    let rendered = created.to_string();
    assert!(rendered.ends_with('Z'), "created: {rendered}");
    let last_seen = r["last_seen"]
        .as_datetime()
        .expect("last_seen is a datetime");
    assert!(last_seen.to_string().ends_with('Z'));

    let day = &rendered[..10];
    let hits = backlog(&root, &["list", "--where", &format!("created={day}")]);
    assert_eq!(hits[0]["id"], json!(a));
    let none = backlog(&root, &["list", "--where-lt", &format!("created={day}")]);
    assert_eq!(none, json!([]));
}

#[test]
fn resolve_drops_the_rank_and_promote_keeps_it() {
    let (_tmp, root) = sandbox();
    let a = mint(&root, "alpha");
    let b = mint(&root, "beta");
    backlog(&root, &["focus", &a, &b]);

    backlog(&root, &["triage", &a, "--resolve", "--resolution", "done"]);
    assert!(row(&root, &a).get("focus").is_none());

    backlog(
        &root,
        &["triage", &b, "--promote", "--external", "--to", "elsewhere"],
    );
    assert_eq!(row(&root, &b)["status"].as_str(), Some("promoted"));
    assert_eq!(row(&root, &b)["focus"].as_integer(), Some(2));
}

#[test]
fn capabilities_lists_the_tier_values_for_next() {
    let (_tmp, root) = sandbox();
    let out = cli(&root)
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let caps: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let values = &caps["commands"]["backlog"]["subcommands"]["next"]["flags"]["--tier"]["values"];
    assert_eq!(values, &json!(["lite", "deep", "human"]));
}
