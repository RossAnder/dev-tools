//! `backlog next` — the rows an agent may pick up, in pickup order.
//!
//! Ranked `open` rows come first in queue order, each carrying its
//! `position` over the whole live queue. With `fill`, unranked `open` rows
//! follow oldest-first by `created`.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;
use toml::value::Datetime;

use super::focus::queue_order;
use super::schema::{self, FIELD_CREATED, FIELD_FOCUS, FIELD_STATUS, FIELD_TIER, STATUS_OPEN};
use crate::cli::ReadIntegrityArgs;
use crate::convert::toml_to_json;
use crate::io::{item_id, items_array};
use crate::output::{Rows, print_report};

pub(crate) fn dispatch_next(
    tier: Option<&str>,
    fill: bool,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let doc = schema::read_store(&integrity)?;
    print_report(JsonValue::Array(select(&doc, tier, fill)), Rows::Top)
}

fn created_of(row: &TomlValue) -> Option<Datetime> {
    match row.get(FIELD_CREATED)? {
        TomlValue::Datetime(dt) => Some(*dt),
        TomlValue::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn is_open(row: &TomlValue) -> bool {
    row.get(FIELD_STATUS).and_then(|v| v.as_str()) == Some(STATUS_OPEN)
}

/// Rows whose `tier` is set to something other than `tier` are dropped;
/// untiered rows pass.
fn tier_allows(row: &TomlValue, tier: Option<&str>) -> bool {
    match (tier, row.get(FIELD_TIER).and_then(|v| v.as_str())) {
        (Some(want), Some(have)) => want == have,
        _ => true,
    }
}

pub(crate) fn select(doc: &TomlValue, tier: Option<&str>, fill: bool) -> Vec<JsonValue> {
    let rows = items_array(doc, schema::ARRAY_BACKLOG);
    let by_id: HashMap<&str, &TomlValue> = rows
        .iter()
        .filter_map(|row| item_id(row).map(|id| (id, row)))
        .collect();

    let mut out: Vec<JsonValue> = Vec::new();
    for (index, id) in queue_order(doc).iter().enumerate() {
        let Some(row) = by_id.get(id.as_str()) else {
            continue;
        };
        if !is_open(row) || !tier_allows(row, tier) {
            continue;
        }
        let mut report = toml_to_json(row);
        if let Some(obj) = report.as_object_mut() {
            obj.insert("position".to_string(), json!(index + 1));
        }
        out.push(report);
    }

    if fill {
        let mut unranked: Vec<&TomlValue> = rows
            .iter()
            .filter(|row| is_open(row) && row.get(FIELD_FOCUS).is_none() && tier_allows(row, tier))
            .collect();
        unranked.sort_by_key(|row| created_of(row));
        out.extend(unranked.into_iter().map(toml_to_json));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(rows: &str) -> TomlValue {
        toml::from_str(rows).unwrap()
    }

    fn ids(rows: &[JsonValue]) -> Vec<&str> {
        rows.iter()
            .map(|r| r.get(schema::FIELD_ID).and_then(|v| v.as_str()).unwrap())
            .collect()
    }

    const STORE: &str = r#"
[[backlog]]
id = "B-a"
status = "open"
created = 2026-10-29T10:00:00Z
[[backlog]]
id = "B-b"
status = "open"
focus = 2
created = 2026-10-01
[[backlog]]
id = "B-c"
status = "promoted"
focus = 1
created = 2026-10-02
[[backlog]]
id = "B-d"
status = "open"
focus = 3
tier = "deep"
created = 2026-10-03
[[backlog]]
id = "B-e"
status = "open"
tier = "human"
created = 2026-10-29
[[backlog]]
id = "B-f"
status = "open"
tier = "lite"
created = 2026-10-29
[[backlog]]
id = "B-g"
status = "open"
created = 2026-10-29
"#;

    #[test]
    fn ranked_rows_come_in_queue_order_with_whole_queue_positions() {
        let rows = select(&doc(STORE), None, false);
        assert_eq!(ids(&rows), vec!["B-b", "B-d"]);
        assert_eq!(rows[0]["position"], json!(2));
        assert_eq!(rows[1]["position"], json!(3));
    }

    #[test]
    fn fill_appends_unranked_oldest_first_with_file_order_on_ties() {
        let rows = select(&doc(STORE), None, true);
        assert_eq!(ids(&rows), vec!["B-b", "B-d", "B-e", "B-f", "B-g", "B-a"]);
        assert!(rows[2].get("position").is_none());
    }

    #[test]
    fn tier_filter_drops_other_tiers_and_keeps_untiered() {
        let rows = select(&doc(STORE), Some("lite"), true);
        assert_eq!(ids(&rows), vec!["B-b", "B-f", "B-g", "B-a"]);
    }

    #[test]
    fn promoted_rows_never_appear() {
        let rows = select(&doc(STORE), None, true);
        assert!(!ids(&rows).contains(&"B-c"));
    }
}
