//! A worker's mid-run request in `plan.toml`, as `[[request]]` rows:
//!
//! ```toml
//! [[request]]
//! id = "Q1"
//! run_id = "wf_…" / milestone = "A" / task = 3 / at = …
//! status = "open"          # or "ruled", then ruled_at = …
//! applied = false
//! [[request.paths]]   path, why, ruling, by, reason
//! [[request.changes]] field, what, evidence, ruling, by, reason
//! ```
//!
//! Each path or change item carries its own ruling, so one request can be
//! granted in part. A store holding a request is stamped at least
//! `REQUEST_SCHEMA_VERSION`, so a binary predating the array refuses it rather
//! than dropping it.

use anyhow::{Result, anyhow};
use toml::Value as TomlValue;
use toml::value::Datetime;

use super::schema::{
    PlanStore, Table, bool_or_false, nested, required_datetime, required_str, required_u32, str_or,
    vocab, vocabulary,
};
use crate::tasks::schema::{as_date as as_datetime, lf};

const ID_PREFIX: &str = "Q";

vocabulary!(
    /// Whether a request still waits on any ruling.
    RequestStatus {
        Open => "open",
        Ruled => "ruled",
    }
);

vocabulary!(
    /// The answer to one path or change item.
    Ruling {
        Open => "open",
        Granted => "granted",
        Denied => "denied",
        NeedsUser => "needs-user",
    }
);

vocabulary!(
    /// Who ruled an item; `""` while it is open.
    RuledBy {
        Unset => "",
        Tomlctl => "tomlctl",
        Orchestrator => "orchestrator",
    }
);

vocabulary!(
    /// The task spec field a change item would amend.
    ChangeField {
        Accept => "accept",
        Means => "means",
        Constraints => "constraints",
        Detail => "detail",
        Files => "files",
    }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) id: String,
    pub(crate) run_id: String,
    pub(crate) milestone: String,
    pub(crate) task: u32,
    pub(crate) at: Datetime,
    pub(crate) status: RequestStatus,
    /// Omitted from the TOML while `None`.
    pub(crate) ruled_at: Option<Datetime>,
    /// Set once the drain has written the granted items to the task store.
    pub(crate) applied: bool,
    pub(crate) paths: Vec<PathItem>,
    pub(crate) changes: Vec<ChangeItem>,
}

/// A file the requesting task needs and does not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathItem {
    pub(crate) path: String,
    pub(crate) why: String,
    pub(crate) ruling: Ruling,
    pub(crate) by: RuledBy,
    pub(crate) reason: String,
}

/// A correction to one field of the requesting task's own spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangeItem {
    pub(crate) field: ChangeField,
    pub(crate) what: String,
    pub(crate) evidence: String,
    pub(crate) ruling: Ruling,
    pub(crate) by: RuledBy,
    pub(crate) reason: String,
}

impl PlanStore {
    /// One past the highest `Q` number a request carries or the prefix has
    /// ever been assigned, raising the mark so that the id outlives its row.
    pub(crate) fn mint_request_id(&mut self) -> String {
        let top = self
            .requests
            .iter()
            .filter_map(|r| r.id.strip_prefix(ID_PREFIX)?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        let mark = self.id_high_water.get(ID_PREFIX).copied().unwrap_or(0);
        let next = top.max(mark).saturating_add(1);
        self.id_high_water.insert(ID_PREFIX.to_string(), next);
        format!("{ID_PREFIX}{next}")
    }
}

/// An absent `by` reads as unset; a present unknown one is refused.
fn ruled_by(table: &Table, context: &str) -> Result<RuledBy> {
    match table.get("by") {
        None => Ok(RuledBy::Unset),
        Some(_) => vocab(table, "by", context),
    }
}

pub(super) fn request_from_toml(table: &Table, context: &str) -> Result<Request> {
    let id = required_str(table, "id", context)?;
    let context = format!("request {id}");
    Ok(Request {
        run_id: str_or(table, "run_id"),
        milestone: str_or(table, "milestone"),
        task: required_u32(table, "task", &context)?,
        at: required_datetime(table, "at", &context)?,
        status: vocab(table, "status", &context)?,
        ruled_at: match table.get("ruled_at") {
            None => None,
            Some(raw) => Some(
                as_datetime(raw)
                    .ok_or_else(|| anyhow!("{context}: `ruled_at` is not a datetime: `{raw}`"))?,
            ),
        },
        applied: bool_or_false(table, "applied", &context)?,
        paths: nested(table, "paths", &context, |entry, context| {
            Ok(PathItem {
                path: str_or(entry, "path"),
                why: lf(&str_or(entry, "why")),
                ruling: vocab(entry, "ruling", context)?,
                by: ruled_by(entry, context)?,
                reason: lf(&str_or(entry, "reason")),
            })
        })?,
        changes: nested(table, "changes", &context, |entry, context| {
            Ok(ChangeItem {
                field: vocab(entry, "field", context)?,
                what: lf(&str_or(entry, "what")),
                evidence: lf(&str_or(entry, "evidence")),
                ruling: vocab(entry, "ruling", context)?,
                by: ruled_by(entry, context)?,
                reason: lf(&str_or(entry, "reason")),
            })
        })?,
        id,
    })
}

pub(super) fn request_to_toml(request: &Request) -> TomlValue {
    let Request {
        id,
        run_id,
        milestone,
        task,
        at,
        status,
        ruled_at,
        applied,
        paths,
        changes,
    } = request;
    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::String(id.clone()));
    table.insert("run_id".to_string(), TomlValue::String(run_id.clone()));
    table.insert(
        "milestone".to_string(),
        TomlValue::String(milestone.clone()),
    );
    table.insert("task".to_string(), TomlValue::Integer(i64::from(*task)));
    table.insert("at".to_string(), TomlValue::Datetime(*at));
    table.insert(
        "status".to_string(),
        TomlValue::String(status.as_str().to_string()),
    );
    if let Some(ruled_at) = ruled_at {
        table.insert("ruled_at".to_string(), TomlValue::Datetime(*ruled_at));
    }
    table.insert("applied".to_string(), TomlValue::Boolean(*applied));
    if !paths.is_empty() {
        table.insert(
            "paths".to_string(),
            TomlValue::Array(paths.iter().map(path_item_to_toml).collect()),
        );
    }
    if !changes.is_empty() {
        table.insert(
            "changes".to_string(),
            TomlValue::Array(changes.iter().map(change_item_to_toml).collect()),
        );
    }
    TomlValue::Table(table)
}

fn path_item_to_toml(item: &PathItem) -> TomlValue {
    let PathItem {
        path,
        why,
        ruling,
        by,
        reason,
    } = item;
    let mut table = Table::new();
    table.insert("path".to_string(), TomlValue::String(path.clone()));
    table.insert("why".to_string(), TomlValue::String(lf(why)));
    insert_ruling(&mut table, *ruling, *by, reason);
    TomlValue::Table(table)
}

fn change_item_to_toml(item: &ChangeItem) -> TomlValue {
    let ChangeItem {
        field,
        what,
        evidence,
        ruling,
        by,
        reason,
    } = item;
    let mut table = Table::new();
    table.insert(
        "field".to_string(),
        TomlValue::String(field.as_str().to_string()),
    );
    table.insert("what".to_string(), TomlValue::String(lf(what)));
    table.insert("evidence".to_string(), TomlValue::String(lf(evidence)));
    insert_ruling(&mut table, *ruling, *by, reason);
    TomlValue::Table(table)
}

fn insert_ruling(table: &mut Table, ruling: Ruling, by: RuledBy, reason: &str) {
    table.insert(
        "ruling".to_string(),
        TomlValue::String(ruling.as_str().to_string()),
    );
    table.insert("by".to_string(), TomlValue::String(by.as_str().to_string()));
    table.insert("reason".to_string(), TomlValue::String(lf(reason)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::schema::tests::fixture;
    use crate::plan::schema::{
        BASE_SCHEMA_VERSION, REQUEST_SCHEMA_VERSION, RUN_RECORDS_SCHEMA_VERSION, from_toml, to_toml,
    };

    fn at(raw: &str) -> Datetime {
        raw.parse().expect("the datetime parses")
    }

    fn request() -> Request {
        Request {
            id: "Q1".to_string(),
            run_id: "wf_4e7f630b-d4a".to_string(),
            milestone: "A".to_string(),
            task: 3,
            at: at("2026-11-18T10:00:00Z"),
            status: RequestStatus::Ruled,
            ruled_at: Some(at("2026-11-18T10:05:00Z")),
            applied: false,
            paths: vec![
                PathItem {
                    path: "tomlctl/README.md".to_string(),
                    why: "The new verb needs its line.".to_string(),
                    ruling: Ruling::Granted,
                    by: RuledBy::Tomlctl,
                    reason: "No other row holds it.".to_string(),
                },
                PathItem {
                    path: "src/held.rs".to_string(),
                    why: "A caller changes.".to_string(),
                    ruling: Ruling::Open,
                    by: RuledBy::Unset,
                    reason: String::new(),
                },
            ],
            changes: vec![ChangeItem {
                field: ChangeField::Accept,
                what: "The check names a moved test.".to_string(),
                evidence: "The test lives in tests/a.rs.".to_string(),
                ruling: Ruling::NeedsUser,
                by: RuledBy::Orchestrator,
                reason: "It changes a success criterion.".to_string(),
            }],
        }
    }

    fn with_request() -> PlanStore {
        PlanStore {
            schema_version: REQUEST_SCHEMA_VERSION,
            requests: vec![request()],
            ..fixture()
        }
    }

    fn rendered_round_trip(store: &PlanStore) -> PlanStore {
        let text = toml::to_string_pretty(&to_toml(store)).expect("renders");
        let doc: TomlValue = toml::from_str(&text).expect("the rendered text parses");
        from_toml(&doc).expect("parses")
    }

    fn stored_request(doc: &mut TomlValue) -> &mut Table {
        doc.as_table_mut()
            .expect("root table")
            .get_mut("request")
            .expect("the array is present")
            .as_array_mut()
            .expect("an array")[0]
            .as_table_mut()
            .expect("a row table")
    }

    fn stored_item<'a>(doc: &'a mut TomlValue, key: &str) -> &'a mut Table {
        stored_request(doc)
            .get_mut(key)
            .expect("the items are present")
            .as_array_mut()
            .expect("an array")[0]
            .as_table_mut()
            .expect("an item table")
    }

    #[test]
    fn a_request_with_both_item_kinds_round_trips_through_rendered_text() {
        let store = with_request();
        assert_eq!(rendered_round_trip(&store), store);
        let text = toml::to_string_pretty(&to_toml(&store)).expect("renders");
        let again =
            toml::to_string_pretty(&to_toml(&rendered_round_trip(&store))).expect("renders");
        assert_eq!(text, again, "a second write is byte-stable");
    }

    #[test]
    fn a_request_is_written_last_with_its_keys_in_record_order() {
        let doc = to_toml(&with_request());
        let root: Vec<&str> = doc
            .as_table()
            .expect("root table")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(&root[root.len() - 2..], ["report", "request"]);
        let keys = |value: &TomlValue| -> Vec<String> {
            value.as_table().expect("a table").keys().cloned().collect()
        };
        assert_eq!(
            keys(&doc["request"][0]),
            [
                "id",
                "run_id",
                "milestone",
                "task",
                "at",
                "status",
                "ruled_at",
                "applied",
                "paths",
                "changes"
            ]
        );
        assert_eq!(
            keys(&doc["request"][0]["paths"][0]),
            ["path", "why", "ruling", "by", "reason"]
        );
        assert_eq!(
            keys(&doc["request"][0]["changes"][0]),
            ["field", "what", "evidence", "ruling", "by", "reason"]
        );
    }

    #[test]
    fn a_store_holding_a_request_stamps_the_request_version() {
        let mut base = PlanStore {
            schema_version: BASE_SCHEMA_VERSION,
            attempts: Vec::new(),
            gate_rounds: Vec::new(),
            reports: Vec::new(),
            ..fixture()
        };
        base.requests.push(request());
        for store in [with_request(), base] {
            assert_eq!(store.write_version(), REQUEST_SCHEMA_VERSION);
            assert_eq!(
                to_toml(&store)["schema_version"].as_integer(),
                Some(REQUEST_SCHEMA_VERSION)
            );
        }
    }

    #[test]
    fn a_store_without_a_request_keeps_its_tier_and_writes_no_request_key() {
        let store = fixture();
        let doc = to_toml(&store);
        assert_eq!(
            doc["schema_version"].as_integer(),
            Some(RUN_RECORDS_SCHEMA_VERSION)
        );
        assert!(doc.get("request").is_none(), "{doc}");

        let mut emptied = with_request();
        emptied.requests.clear();
        emptied.schema_version = RUN_RECORDS_SCHEMA_VERSION;
        assert_eq!(to_toml(&emptied), doc);
    }

    #[test]
    fn an_open_request_omits_ruled_at_and_empty_item_arrays() {
        let mut store = with_request();
        store.requests[0].status = RequestStatus::Open;
        store.requests[0].ruled_at = None;
        store.requests[0].changes.clear();
        let doc = to_toml(&store);
        assert!(doc["request"][0].get("ruled_at").is_none(), "{doc}");
        assert!(doc["request"][0].get("changes").is_none(), "{doc}");
        assert_eq!(rendered_round_trip(&store), store);
    }

    #[test]
    fn request_ids_mint_as_q1_then_q2() {
        let mut store = PlanStore::default();
        let first = store.mint_request_id();
        assert_eq!(first, "Q1");
        store.requests.push(Request {
            id: first,
            ..request()
        });
        assert_eq!(store.mint_request_id(), "Q2");
        assert_eq!(store.id_high_water.get("Q"), Some(&2));
    }

    #[test]
    fn a_removed_top_request_id_is_never_minted_again() {
        let mut store = PlanStore::default();
        let first = store.mint_request_id();
        store.requests.push(Request {
            id: first,
            ..request()
        });
        store.requests.clear();
        let mut reread = from_toml(&to_toml(&store)).expect("parses");
        assert_eq!(reread.mint_request_id(), "Q2");
    }

    #[test]
    fn unknown_keys_in_a_request_and_its_items_are_dropped() {
        let mut raw = to_toml(&with_request());
        stored_request(&mut raw).insert("stray".to_string(), TomlValue::Boolean(true));
        stored_item(&mut raw, "paths").insert("stray".to_string(), TomlValue::Boolean(true));
        stored_item(&mut raw, "changes").insert("stray".to_string(), TomlValue::Boolean(true));
        let written = to_toml(&from_toml(&raw).expect("parses"));
        assert_eq!(written, to_toml(&with_request()));
    }

    #[test]
    fn an_unknown_request_vocabulary_value_is_refused_naming_the_choices() {
        let cases: [(&str, &str, &str, &[&str]); 5] = [
            ("", "status", "pending", RequestStatus::VOCABULARY),
            ("paths", "ruling", "maybe", Ruling::VOCABULARY),
            ("paths", "by", "user", RuledBy::VOCABULARY),
            ("changes", "ruling", "maybe", Ruling::VOCABULARY),
            ("changes", "field", "outcome", ChangeField::VOCABULARY),
        ];
        for (items, key, bad, vocabulary) in cases {
            let mut raw = to_toml(&with_request());
            let table = if items.is_empty() {
                stored_request(&mut raw)
            } else {
                stored_item(&mut raw, items)
            };
            table.insert(key.to_string(), TomlValue::String(bad.to_string()));
            let message = from_toml(&raw).expect_err(bad).to_string();
            assert!(message.contains("request Q1"), "{message}");
            assert!(message.contains(bad), "{message}");
            assert!(message.contains(&vocabulary.join(", ")), "{message}");
        }
    }

    #[test]
    fn a_request_item_without_by_reads_as_unset() {
        let mut raw = to_toml(&with_request());
        stored_item(&mut raw, "paths").remove("by");
        let read = from_toml(&raw).expect("parses");
        assert_eq!(read.requests[0].paths[0].by, RuledBy::Unset);
    }

    #[test]
    fn a_request_missing_a_required_key_is_refused() {
        for key in ["id", "task", "at", "status"] {
            let mut raw = to_toml(&with_request());
            stored_request(&mut raw).remove(key);
            let message = from_toml(&raw).expect_err(key).to_string();
            assert!(message.contains(key), "{key}: {message}");
        }
    }
}
