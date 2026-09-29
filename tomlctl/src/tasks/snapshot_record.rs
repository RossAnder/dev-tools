//! The snapshot's `record` array and the per-checkpoint facts joined from it.
//!
//! Execution-record entries name their task by `task_ref`, which can predate a
//! heading rename, so a ref is resolved exactly first and then through the
//! separator-blind matcher import uses. `checkpoint` entries carry no ref at
//! all: their `commits[]` are mapped to rows by commit prefix, and a row's
//! `checkpoint` is what names the group a commit landed for.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::schema::{Store, TaskRow};
use super::slug::normalise_for_match;
use crate::convert::toml_to_json;
use crate::io::items_array;

/// A checkpoint's commits, in record order, and its latest verification as
/// `{outcome, summary}`.
pub(crate) type CheckpointFacts = (Vec<String>, Option<JsonValue>);

/// `record` is the parsed `execution-record.toml`; `None` (no record yet)
/// yields an empty array. Each entry is emitted verbatim plus `task_id`, and
/// `checkpoint` entries also carry `checkpoint_ids`.
pub(crate) fn record_view(record: Option<&TomlValue>, store: &Store) -> JsonValue {
    let Some(doc) = record else {
        return JsonValue::Array(Vec::new());
    };
    let entries = items_array(doc, "items")
        .iter()
        .map(|entry| {
            let mut value = toml_to_json(entry);
            if let JsonValue::Object(map) = &mut value {
                let task_id = entry
                    .get("task_ref")
                    .and_then(TomlValue::as_str)
                    .and_then(|r| resolve_ref(store, r))
                    .map_or(JsonValue::Null, |row| json!(row.id));
                map.insert("task_id".to_string(), task_id);
                if is_checkpoint(entry) {
                    let ids: BTreeSet<String> = entry_commits(entry)
                        .iter()
                        .filter_map(|commit| checkpoint_of_commit(store, commit))
                        .collect();
                    map.insert("checkpoint_ids".to_string(), json!(ids));
                }
            }
            value
        })
        .collect();
    JsonValue::Array(entries)
}

/// Keyed by every checkpoint the store declares or a record entry maps to. A
/// commit that matches a row goes to that row's checkpoint; one matching no
/// row goes to every checkpoint its entry maps to, so a fixup riding a train
/// is not dropped. A verification counts for a checkpoint when its `task_id`
/// is a member, and the last such entry in record order wins.
pub(crate) fn checkpoint_facts(
    view: &JsonValue,
    store: &Store,
) -> BTreeMap<String, CheckpointFacts> {
    let mut facts: BTreeMap<String, CheckpointFacts> = store
        .checkpoints
        .iter()
        .map(|checkpoint| (checkpoint.id.clone(), (Vec::new(), None)))
        .collect();
    let entries = view.as_array().map(Vec::as_slice).unwrap_or_default();

    for entry in entries {
        match entry.get("type").and_then(JsonValue::as_str) {
            Some("checkpoint") => {
                let named: Vec<String> = entry
                    .get("checkpoint_ids")
                    .and_then(JsonValue::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(JsonValue::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let commits = entry
                    .get("commits")
                    .and_then(JsonValue::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                for commit in commits.iter().filter_map(JsonValue::as_str) {
                    let targets = match checkpoint_of_commit(store, commit) {
                        Some(id) => vec![id],
                        None => named.clone(),
                    };
                    for id in targets {
                        let (list, _) = facts.entry(id).or_insert_with(|| (Vec::new(), None));
                        if !list.iter().any(|seen| seen == commit) {
                            list.push(commit.to_string());
                        }
                    }
                }
            }
            Some("verification") => {
                let Some(checkpoint) = entry
                    .get("task_id")
                    .and_then(JsonValue::as_u64)
                    .and_then(|id| u32::try_from(id).ok())
                    .and_then(|id| store.find(id))
                    .map(|row| row.checkpoint.as_str())
                    .filter(|checkpoint| !checkpoint.is_empty())
                else {
                    continue;
                };
                let outcome = entry.get("outcome").cloned().unwrap_or(json!(""));
                let summary = entry.get("summary").cloned().unwrap_or(json!(""));
                let slot = facts
                    .entry(checkpoint.to_string())
                    .or_insert_with(|| (Vec::new(), None));
                slot.1 = Some(json!({ "outcome": outcome, "summary": summary }));
            }
            _ => {}
        }
    }
    facts
}

fn resolve_ref<'a>(store: &'a Store, task_ref: &str) -> Option<&'a TaskRow> {
    store.find_ref(task_ref).or_else(|| {
        let wanted = normalise_for_match(task_ref);
        if wanted.is_empty() {
            return None;
        }
        store
            .items
            .iter()
            .find(|row| normalise_for_match(&row.r#ref) == wanted)
    })
}

fn is_checkpoint(entry: &TomlValue) -> bool {
    entry.get("type").and_then(TomlValue::as_str) == Some("checkpoint")
}

fn entry_commits(entry: &TomlValue) -> Vec<&str> {
    entry
        .get("commits")
        .and_then(TomlValue::as_array)
        .map(|commits| commits.iter().filter_map(TomlValue::as_str).collect())
        .unwrap_or_default()
}

/// Either side may be abbreviated, so the shorter must prefix the longer.
fn checkpoint_of_commit(store: &Store, commit: &str) -> Option<String> {
    let commit = commit.trim();
    if commit.is_empty() {
        return None;
    }
    store
        .items
        .iter()
        .filter(|row| !row.checkpoint.is_empty())
        .find(|row| {
            let own = row.commit.trim();
            !own.is_empty() && (own.starts_with(commit) || commit.starts_with(own))
        })
        .map(|row| row.checkpoint.clone())
}

#[cfg(test)]
mod tests {
    use super::super::schema::{Checkpoint, Effort, Status};
    use super::*;

    fn row(id: u32, r#ref: &str, checkpoint: &str, commit: &str) -> TaskRow {
        TaskRow {
            id,
            r#ref: r#ref.to_string(),
            title: r#ref.to_string(),
            effort: Effort::S,
            status: Status::Done,
            checkpoint: checkpoint.to_string(),
            phase: String::new(),
            phase_depth: 0,
            heading_depth: 3,
            files: Vec::new(),
            needs: Vec::new(),
            coupling: Vec::new(),
            deps_note: String::new(),
            action: String::new(),
            detail: String::new(),
            acceptance: String::new(),
            agent: String::new(),
            commit: commit.to_string(),
        }
    }

    fn store() -> Store {
        Store {
            checkpoints: ["A", "B", "D"]
                .iter()
                .map(|id| Checkpoint {
                    id: id.to_string(),
                    rationale: String::new(),
                })
                .collect(),
            items: vec![
                row(1, "make-promotion-a-live-claim", "A", "29d5d00"),
                row(2, "add-the-count_distinct-filter", "A", "5aa4696"),
                row(3, "add-the-backlog-links-side-table", "D", "f5f16f5"),
                row(4, "import-backlog-links", "D", "e7611f4"),
                row(5, "parse-the-plan-bullet", "B", "a238133"),
            ],
            ..Store::default()
        }
    }

    fn record(body: &str) -> TomlValue {
        toml::from_str(body).expect("fixture record parses")
    }

    fn view_of(body: &str) -> JsonValue {
        record_view(Some(&record(body)), &store())
    }

    #[test]
    fn task_refs_resolve_exactly_then_normalised_then_to_null() {
        let view = view_of(
            r#"
            [[items]]
            id = "E1"
            type = "task-completion"
            task_ref = "import-backlog-links"
            [[items]]
            id = "E2"
            type = "task-completion"
            task_ref = "add-the-count-distinct-filter"
            [[items]]
            id = "E3"
            type = "deviation"
            task_ref = "no-such-task"
            [[items]]
            id = "E4"
            type = "reconcile"
            "#,
        );
        let ids: Vec<&JsonValue> = view
            .as_array()
            .expect("array")
            .iter()
            .map(|entry| &entry["task_id"])
            .collect();
        assert_eq!(
            ids,
            [&json!(4), &json!(2), &JsonValue::Null, &JsonValue::Null]
        );
        assert_eq!(
            view[0]["id"],
            json!("E1"),
            "entry fields pass through verbatim"
        );
    }

    #[test]
    fn an_absent_record_is_an_empty_array() {
        assert_eq!(record_view(None, &store()), json!([]));
    }

    #[test]
    fn a_two_checkpoint_commit_train_maps_to_both_checkpoints() {
        let view = view_of(
            r#"
            [[items]]
            id = "E18"
            type = "checkpoint"
            commits = ["29d5d00", "5aa4696", "f5f16f5abcdef", "e7611f4", "0000000"]
            summary = "checkpoints A+D"
            "#,
        );
        assert_eq!(view[0]["checkpoint_ids"], json!(["A", "D"]));
        assert_eq!(view[0]["task_id"], JsonValue::Null);

        let facts = checkpoint_facts(&view, &store());
        assert_eq!(
            facts["A"].0,
            ["29d5d00", "5aa4696", "0000000"],
            "an unmatched commit rides with every checkpoint its train names"
        );
        assert_eq!(facts["D"].0, ["f5f16f5abcdef", "e7611f4", "0000000"]);
        assert!(facts["B"].0.is_empty());
    }

    #[test]
    fn a_non_checkpoint_entry_carries_no_checkpoint_ids() {
        let view = view_of(
            r#"
            [[items]]
            id = "E1"
            type = "task-completion"
            task_ref = "import-backlog-links"
            commits = ["e7611f4"]
            "#,
        );
        assert!(view[0].get("checkpoint_ids").is_none());
    }

    #[test]
    fn the_latest_member_verification_wins() {
        let view = view_of(
            r#"
            [[items]]
            id = "E7"
            type = "verification"
            task_ref = "import-backlog-links"
            summary = "gate: build"
            outcome = "fail"
            [[items]]
            id = "E8"
            type = "verification"
            task_ref = "parse-the-plan-bullet"
            summary = "gate B"
            outcome = "pass"
            [[items]]
            id = "E9"
            type = "verification"
            task_ref = "add-the-backlog-links-side-table"
            summary = "gate: tests"
            outcome = "pass"
            "#,
        );
        let facts = checkpoint_facts(&view, &store());
        assert_eq!(
            facts["D"].1,
            Some(json!({ "outcome": "pass", "summary": "gate: tests" }))
        );
        assert_eq!(
            facts["B"].1,
            Some(json!({ "outcome": "pass", "summary": "gate B" }))
        );
        assert_eq!(facts["A"].1, None);
    }
}
