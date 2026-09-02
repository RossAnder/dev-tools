//! `backlog cluster` — the area, tag and relation grouping views.
//!
//! Three independent views over one item set, never blended into a score: a
//! group in one view claims nothing about the others.
//!
//! Area collapse rule — each item starts at its full `area` path; the
//! undersized group with the longest prefix (ties broken by smallest key)
//! drops one trailing component and merges into whatever already sits there;
//! a group stops collapsing the moment it reaches `--min-size`; a
//! one-component prefix never collapses to the repo root. So a crowded
//! subtree keeps its specific prefix and a lone neighbour is not swept into
//! it. Items carrying no `area` form the `unscoped` group, which is emitted
//! whenever it is non-empty — `--min-size` gates path prefixes, and "no
//! path" is not one.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::cli::{ClusterBy, ReadIntegrityArgs};
use crate::io::items_array;

use super::schema::{
    ARRAY_BACKLOG, FIELD_AREA, FIELD_ID, FIELD_KIND, FIELD_STATUS, FIELD_TAGS, RELATION_FIELDS,
    STATUS_OPEN, coerce_kind, read_store,
};

const VIEW_AREA: &str = "area";
const VIEW_TAGS: &str = "tags";
const VIEW_RELATIONS: &str = "relations";

/// Key of the group holding every item with no `area`.
const UNSCOPED_KEY: &str = "unscoped";

pub(crate) fn dispatch(
    by: ClusterBy,
    min_size: usize,
    min_shared_tags: usize,
    all_statuses: bool,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let items = parse_items(&read_store(&integrity)?, all_statuses);
    crate::output::print_json(&build_views(&items, by, min_size, min_shared_tags))
}

/// One clusterable row. `kind` is absent rather than defaulted when the row
/// carries none, so the emitted `kinds` array never invents a bucket.
struct Item {
    id: String,
    kind: Option<&'static str>,
    area: String,
    tags: BTreeSet<String>,
    edges: BTreeSet<String>,
}

/// Live `[[backlog]]` rows only. `compacted` rows are terminal by
/// construction, so `--all-statuses` widens the status filter over the live
/// array rather than pulling the fold-away array in. A row with no `status`
/// is not `open` and so needs `--all-statuses` too.
fn parse_items(doc: &TomlValue, all_statuses: bool) -> Vec<Item> {
    let mut out = Vec::new();
    for row in items_array(doc, ARRAY_BACKLOG) {
        let id = str_field(row, FIELD_ID);
        if id.is_empty() {
            continue;
        }
        if !all_statuses && str_field(row, FIELD_STATUS) != STATUS_OPEN {
            continue;
        }
        let kind = row
            .get(FIELD_KIND)
            .and_then(TomlValue::as_str)
            .filter(|k| !k.is_empty())
            .map(coerce_kind);
        let mut edges = BTreeSet::new();
        for field in RELATION_FIELDS {
            collect_strings(row.get(*field), &mut edges);
        }
        edges.remove(&id);
        let mut tags = BTreeSet::new();
        collect_strings(row.get(FIELD_TAGS), &mut tags);
        out.push(Item {
            area: str_field(row, FIELD_AREA),
            id,
            kind,
            tags,
            edges,
        });
    }
    out
}

fn str_field(row: &TomlValue, field: &str) -> String {
    row.get(field)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Accepts both the scalar and the array spelling: `related` is an array,
/// while `duplicate_of` and `supersedes` are written as single ids.
fn collect_strings(value: Option<&TomlValue>, out: &mut BTreeSet<String>) {
    match value {
        Some(TomlValue::String(s)) if !s.is_empty() => {
            out.insert(s.clone());
        }
        Some(TomlValue::Array(a)) => {
            for entry in a {
                if let Some(s) = entry.as_str().filter(|s| !s.is_empty()) {
                    out.insert(s.to_string());
                }
            }
        }
        _ => {}
    }
}

fn build_views(
    items: &[Item],
    by: ClusterBy,
    min_size: usize,
    min_shared_tags: usize,
) -> JsonValue {
    let mut out = serde_json::Map::new();
    if matches!(by, ClusterBy::Area | ClusterBy::All) {
        out.insert(VIEW_AREA.into(), cluster_area(items, min_size).into());
    }
    if matches!(by, ClusterBy::Tags | ClusterBy::All) {
        out.insert(VIEW_TAGS.into(), cluster_tags(items, min_shared_tags).into());
    }
    if matches!(by, ClusterBy::Relations | ClusterBy::All) {
        out.insert(VIEW_RELATIONS.into(), cluster_relations(items).into());
    }
    JsonValue::Object(out)
}

fn path_components(area: &str) -> Vec<String> {
    area.split('/')
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect()
}

fn cluster_area(items: &[Item], min_size: usize) -> Vec<JsonValue> {
    let mut groups: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
    let mut unscoped: Vec<usize> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let components = path_components(&item.area);
        if components.is_empty() {
            unscoped.push(index);
        } else {
            groups.entry(components).or_default().push(index);
        }
    }

    loop {
        let candidate = groups
            .iter()
            .filter(|(prefix, members)| members.len() < min_size && prefix.len() > 1)
            // Longest prefix first; among equal lengths the smallest key, so
            // the walk is independent of insertion order.
            .max_by(|(a, _), (b, _)| a.len().cmp(&b.len()).then_with(|| b.cmp(a)))
            .map(|(prefix, _)| prefix.clone());
        let Some(prefix) = candidate else { break };
        let members = groups.remove(&prefix).unwrap_or_default();
        let mut shorter = prefix;
        shorter.pop();
        groups.entry(shorter).or_default().extend(members);
    }

    let mut emitted: Vec<(String, String, Vec<usize>)> = groups
        .into_iter()
        .filter(|(_, members)| members.len() >= min_size)
        .map(|(prefix, members)| {
            let key = prefix.join("/");
            let reason = format!("shared path prefix {key}");
            (key, reason, members)
        })
        .collect();
    if !unscoped.is_empty() {
        emitted.push((
            UNSCOPED_KEY.to_string(),
            "no area recorded".to_string(),
            unscoped,
        ));
    }
    finish(items, emitted)
}

fn cluster_tags(items: &[Item], min_shared: usize) -> Vec<JsonValue> {
    // A shared-tag group needs at least one shared tag: at zero every pair
    // qualifies vacuously and the view collapses to one keyless group.
    let min_shared = min_shared.max(1);
    let mut parent: Vec<usize> = (0..items.len()).collect();
    let mut edges: Vec<(usize, usize, BTreeSet<&str>)> = Vec::new();
    for (i, a) in items.iter().enumerate() {
        for (j, b) in items.iter().enumerate().skip(i + 1) {
            let shared: BTreeSet<&str> = a
                .tags
                .intersection(&b.tags)
                .map(String::as_str)
                .collect();
            if shared.len() >= min_shared {
                union(&mut parent, i, j);
                edges.push((i, j, shared));
            }
        }
    }

    // A transitive merge can leave a group whose members share no single
    // tag, so the key is every tag some qualifying pair inside the group
    // shares — equal to the plain intersection whenever the group is one
    // clique, which is the common case.
    let mut keys: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
    for (i, _, shared) in &edges {
        keys.entry(root(&mut parent, *i))
            .or_default()
            .extend(shared.iter().copied());
    }

    let emitted = components(&mut parent, items.len())
        .into_iter()
        .map(|(component_root, members)| {
            let key = keys
                .get(&component_root)
                .map(|tags| tags.iter().copied().collect::<Vec<_>>().join("+"))
                .unwrap_or_default();
            let reason = format!("share tags {key}");
            (key, reason, members)
        })
        .collect();
    finish(items, emitted)
}

fn cluster_relations(items: &[Item]) -> Vec<JsonValue> {
    let index_of: BTreeMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.id.as_str(), index))
        .collect();
    let mut parent: Vec<usize> = (0..items.len()).collect();
    for (index, item) in items.iter().enumerate() {
        for target in &item.edges {
            // A dangling edge — target outside the considered set — links
            // nothing, so it is dropped rather than pulling in a phantom.
            if let Some(&other) = index_of.get(target.as_str()) {
                union(&mut parent, index, other);
            }
        }
    }

    let emitted = components(&mut parent, items.len())
        .into_values()
        .map(|members| {
            let key = members
                .iter()
                .map(|&index| items[index].id.as_str())
                .min()
                .unwrap_or_default()
                .to_string();
            (
                key,
                "linked by relates-to/duplicates/supersedes edges".to_string(),
                members,
            )
        })
        .collect();
    finish(items, emitted)
}

fn root(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (root(parent, a), root(parent, b));
    if ra == rb {
        return;
    }
    // Lowest index wins, so a component's representative does not depend on
    // the order the edges were discovered in.
    if ra < rb { parent[rb] = ra } else { parent[ra] = rb }
}

/// Components of two or more members, keyed by representative. Singletons
/// are dropped: one item linked to nothing is not a cluster.
fn components(parent: &mut [usize], len: usize) -> BTreeMap<usize, Vec<usize>> {
    let mut out: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..len {
        out.entry(root(parent, index)).or_default().push(index);
    }
    out.retain(|_, members| members.len() > 1);
    out
}

fn finish(items: &[Item], mut groups: Vec<(String, String, Vec<usize>)>) -> Vec<JsonValue> {
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    groups
        .into_iter()
        .map(|(key, reason, members)| group_json(items, &key, &reason, &members))
        .collect()
}

fn group_json(items: &[Item], key: &str, reason: &str, members: &[usize]) -> JsonValue {
    let item_ids: BTreeSet<&str> = members.iter().map(|&i| items[i].id.as_str()).collect();
    let kinds: BTreeSet<&str> = members.iter().filter_map(|&i| items[i].kind).collect();
    let areas: BTreeSet<&str> = members
        .iter()
        .map(|&i| items[i].area.as_str())
        .filter(|area| !area.is_empty())
        .collect();
    json!({
        "key": key,
        "reason": reason,
        "size": members.len(),
        "item_ids": item_ids,
        "kinds": kinds,
        "areas": areas,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, area: &str, tags: &[&str], edges: &[&str]) -> Item {
        Item {
            id: id.to_string(),
            kind: Some(crate::backlog::schema::KIND_BUG),
            area: area.to_string(),
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
            edges: edges.iter().map(|e| (*e).to_string()).collect(),
        }
    }

    fn scoped(id: &str, area: &str) -> Item {
        item(id, area, &[], &[])
    }

    fn tagged(id: &str, tags: &[&str]) -> Item {
        item(id, "", tags, &[])
    }

    fn linked(id: &str, edges: &[&str]) -> Item {
        item(id, "", &[], edges)
    }

    fn keys(groups: &[JsonValue]) -> Vec<&str> {
        groups.iter().map(|g| g["key"].as_str().unwrap()).collect()
    }

    fn ids(group: &JsonValue) -> Vec<&str> {
        group["item_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect()
    }

    #[test]
    fn area_collapses_to_the_crowded_prefix_not_the_root() {
        let items = [
            scoped("B-01", "lumina/server/pty/manager.rs"),
            scoped("B-02", "lumina/server/pty/spawn.rs"),
            scoped("B-03", "lumina/server/pty/mod.rs"),
            scoped("B-04", "lumina/web/src/App.vue"),
        ];
        let groups = cluster_area(&items, 2);
        assert_eq!(keys(&groups), ["lumina/server/pty"]);
        assert_eq!(ids(&groups[0]), ["B-01", "B-02", "B-03"]);
        assert_eq!(groups[0]["size"], 3);
        assert_eq!(
            groups[0]["reason"],
            "shared path prefix lumina/server/pty"
        );
        // The lone neighbour is never swept up into the pty group, and with
        // nowhere else to land it drops out of the view entirely.
        assert!(
            !groups
                .iter()
                .any(|g| ids(g).contains(&"B-04")),
            "{groups:?}"
        );
    }

    #[test]
    fn area_collapses_a_directory_prefix_up_one_component() {
        let items = [
            scoped("B-01", "lumina/server/pty/manager.rs"),
            scoped("B-02", "lumina/server/session/"),
            scoped("B-03", "lumina/server/wire.rs"),
        ];
        let groups = cluster_area(&items, 2);
        assert_eq!(keys(&groups), ["lumina/server"]);
        assert_eq!(ids(&groups[0]), ["B-01", "B-02", "B-03"]);
        assert_eq!(
            groups[0]["areas"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "lumina/server/pty/manager.rs",
                "lumina/server/session/",
                "lumina/server/wire.rs"
            ]
        );
    }

    #[test]
    fn area_emits_unscoped_below_min_size_and_omits_it_when_empty() {
        let one = [scoped("B-01", "")];
        let groups = cluster_area(&one, 2);
        assert_eq!(keys(&groups), [UNSCOPED_KEY]);
        assert_eq!(groups[0]["areas"].as_array().unwrap().len(), 0);
        assert_eq!(groups[0]["reason"], "no area recorded");

        let none = [scoped("B-01", "lumina/web/src/App.vue")];
        assert!(cluster_area(&none, 2).is_empty());
    }

    #[test]
    fn tags_group_only_at_the_shared_threshold() {
        let items = [tagged("B-01", &["ci", "windows"]), tagged("B-02", &["ci"])];
        assert!(cluster_tags(&items, 2).is_empty());

        let groups = cluster_tags(&items, 1);
        assert_eq!(keys(&groups), ["ci"]);
        assert_eq!(ids(&groups[0]), ["B-01", "B-02"]);
        assert_eq!(groups[0]["reason"], "share tags ci");
    }

    #[test]
    fn tags_zero_threshold_is_clamped_to_one() {
        let items = [tagged("B-01", &[]), tagged("B-02", &[])];
        assert!(cluster_tags(&items, 0).is_empty());

        let mixed = [tagged("B-01", &["ci"]), tagged("B-02", &[])];
        assert!(cluster_tags(&mixed, 0).is_empty());
    }

    #[test]
    fn tags_form_one_group_when_three_items_share_two() {
        let items = [
            tagged("B-01", &["ci", "windows", "pty"]),
            tagged("B-02", &["ci", "windows"]),
            tagged("B-03", &["windows", "ci", "slow"]),
        ];
        let groups = cluster_tags(&items, 2);
        assert_eq!(keys(&groups), ["ci+windows"]);
        assert_eq!(ids(&groups[0]), ["B-01", "B-02", "B-03"]);
    }

    #[test]
    fn tags_merge_transitively_through_a_shared_middle() {
        // A~B on {ci,windows}; B~C on {flaky,windows}; A and C share only
        // `windows`, so nothing but the transitive merge joins them.
        let items = [
            tagged("B-01", &["ci", "windows"]),
            tagged("B-02", &["ci", "windows", "flaky"]),
            tagged("B-03", &["flaky", "windows"]),
        ];
        let groups = cluster_tags(&items, 2);
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(ids(&groups[0]), ["B-01", "B-02", "B-03"]);
        assert_eq!(keys(&groups), ["ci+flaky+windows"]);
    }

    #[test]
    fn relations_walk_a_chain_and_ignore_outsiders() {
        let items = [
            linked("B-01", &["B-02"]),
            linked("B-02", &["B-03"]),
            linked("B-03", &[]),
            linked("B-09", &["B-77"]),
        ];
        let groups = cluster_relations(&items);
        assert_eq!(keys(&groups), ["B-01"]);
        assert_eq!(ids(&groups[0]), ["B-01", "B-02", "B-03"]);
        assert_eq!(
            groups[0]["reason"],
            "linked by relates-to/duplicates/supersedes edges"
        );
        // An edge whose target is not in the considered set links nothing,
        // so the singleton is dropped rather than emitted.
        assert!(!groups.iter().any(|g| ids(g).contains(&"B-09")));
    }

    fn doc(s: &str) -> TomlValue {
        toml::from_str(s).unwrap()
    }

    const MIXED_STATUS: &str = r#"
[[backlog]]
id = "B-01"
summary = "one"
status = "open"
area = "lumina/server/pty/manager.rs"
tags = ["ci", "windows"]

[[backlog]]
id = "B-02"
summary = "two"
status = "resolved"
resolved = 2026-09-01
resolution = "fixed"
area = "lumina/server/pty/spawn.rs"
tags = ["ci", "windows"]
duplicate_of = "B-01"
"#;

    #[test]
    fn resolved_rows_need_all_statuses() {
        let d = doc(MIXED_STATUS);
        let open_only = parse_items(&d, false);
        assert_eq!(
            open_only.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["B-01"]
        );
        assert!(cluster_area(&open_only, 2).is_empty());

        let everything = parse_items(&d, true);
        assert_eq!(
            everything.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["B-01", "B-02"]
        );
        assert_eq!(keys(&cluster_area(&everything, 2)), ["lumina/server/pty"]);
        // `duplicate_of` is a scalar id and still counts as a relation edge.
        assert_eq!(keys(&cluster_relations(&everything)), ["B-01"]);
    }

    #[test]
    fn by_area_emits_only_the_area_key() {
        let items = parse_items(&doc(MIXED_STATUS), true);
        let view = build_views(&items, ClusterBy::Area, 2, 2);
        assert!(view.get(VIEW_AREA).is_some());
        assert!(view.get(VIEW_TAGS).is_none(), "{view}");
        assert!(view.get(VIEW_RELATIONS).is_none(), "{view}");

        let all = build_views(&items, ClusterBy::All, 2, 2);
        for view_key in [VIEW_AREA, VIEW_TAGS, VIEW_RELATIONS] {
            assert!(all.get(view_key).is_some(), "{all}");
        }
        assert_eq!(
            all.as_object().unwrap().keys().collect::<Vec<_>>(),
            [VIEW_AREA, VIEW_TAGS, VIEW_RELATIONS]
        );
    }

    #[test]
    fn a_missing_store_yields_empty_views() {
        let exists = crate::test_support::with_root(|_| {
            crate::backlog::schema::backlog_path().unwrap().exists()
        });
        assert!(!exists);
        let view = build_views(&[], ClusterBy::All, 2, 2);
        for view_key in [VIEW_AREA, VIEW_TAGS, VIEW_RELATIONS] {
            assert_eq!(view[view_key].as_array().unwrap().len(), 0);
        }
    }
}
