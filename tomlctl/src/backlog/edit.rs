//! `backlog edit` — in-place edits of the free-form fields of a capture.
//!
//! Only `context`, `tags`, `evidence`, `origin`, `flow` and `base_sha` are
//! editable. Every other field is owned by another verb, or minted, and an
//! attempt to edit it is refused with the verb that does own it
//! (`refusal_for`). `apply_edits` works on a clone and swaps it in only when
//! every edited row and the whole document validate, so a failure leaves the
//! caller's document untouched.

use std::collections::BTreeSet;

use anyhow::Result;
use toml::Value as TomlValue;

use super::schema::{
    self, ARRAY_BACKLOG, FIELD_AREA, FIELD_BASE_SHA, FIELD_COMPACTED_ON, FIELD_CONTEXT,
    FIELD_CREATED, FIELD_DEDUP_ID, FIELD_DISMISS_REASON, FIELD_DISMISSED, FIELD_DUPLICATE_OF,
    FIELD_EVIDENCE, FIELD_FLOW, FIELD_ID, FIELD_KIND, FIELD_LAST_SEEN, FIELD_ORIGIN,
    FIELD_PROMOTED, FIELD_PROMOTED_TO, FIELD_RELATED, FIELD_REOPEN_RATIONALE, FIELD_RESOLUTION,
    FIELD_RESOLVED, FIELD_RESOLVED_COMMITS, FIELD_RESOLVED_FLOW, FIELD_RESOLVED_TASKS,
    FIELD_SEEN_COUNT, FIELD_STATUS, FIELD_SUMMARY, FIELD_SUPERSEDES, FIELD_TAGS,
    FIELD_TERMINAL_DATE, FIELD_TERMINAL_REASON,
};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::items_array;

/// The fields `backlog edit` may change.
pub(crate) const EDITABLE_FIELDS: &[&str] = &[
    FIELD_CONTEXT,
    FIELD_TAGS,
    FIELD_EVIDENCE,
    FIELD_ORIGIN,
    FIELD_FLOW,
    FIELD_BASE_SHA,
];

/// One row's edits. `None` / empty leaves the field alone. For `tags` and
/// `evidence` a replacement is applied first, then additions, then (tags only)
/// removals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Edit {
    pub(crate) id: String,
    pub(crate) context: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) add_tags: Vec<String>,
    pub(crate) remove_tags: Vec<String>,
    pub(crate) evidence: Option<Vec<String>>,
    pub(crate) add_evidence: Vec<String>,
    pub(crate) origin: Option<String>,
    pub(crate) flow: Option<String>,
    pub(crate) base_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditOutcome {
    pub(crate) id: String,
    pub(crate) changed: bool,
}

/// The refusal for a field `backlog edit` does not own, naming the verb that
/// does. `None` for an editable field. A field the store does not know is
/// refused too, listing what is editable.
pub(crate) fn refusal_for(field: &str) -> Option<String> {
    if EDITABLE_FIELDS.contains(&field) {
        return None;
    }
    let msg = match field {
        FIELD_STATUS
        | FIELD_PROMOTED
        | FIELD_PROMOTED_TO
        | FIELD_DISMISSED
        | FIELD_DISMISS_REASON
        | FIELD_RESOLVED
        | FIELD_RESOLUTION
        | FIELD_REOPEN_RATIONALE
        | FIELD_RESOLVED_FLOW
        | FIELD_RESOLVED_TASKS
        | FIELD_RESOLVED_COMMITS => format!(
            "`{field}` is managed by `backlog triage` (resolve, dismiss, promote, reopen); \
             `backlog edit` cannot change it"
        ),
        FIELD_SUMMARY | FIELD_KIND | FIELD_AREA => format!(
            "`{field}` feeds the item's `id` and `dedup_id`, so it cannot be edited; capture a \
             successor with `backlog add` and `supersedes`"
        ),
        FIELD_CREATED
        | FIELD_LAST_SEEN
        | FIELD_SEEN_COUNT
        | FIELD_ID
        | FIELD_DEDUP_ID
        | FIELD_TERMINAL_DATE
        | FIELD_TERMINAL_REASON
        | FIELD_COMPACTED_ON => {
            format!("`{field}` is minted by tomlctl and cannot be edited")
        }
        FIELD_RELATED | FIELD_DUPLICATE_OF | FIELD_SUPERSEDES => {
            format!("`{field}` is written by `backlog relate`; `backlog edit` cannot change it")
        }
        _ => format!(
            "`{field}` is not an editable backlog field; `backlog edit` changes {}",
            EDITABLE_FIELDS.join(", ")
        ),
    };
    Some(msg)
}

fn validation_err(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, schema::backlog_path().ok(), msg)
}

fn dedup_keep_order(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|v| seen.insert(v.clone()))
        .collect()
}

fn string_array(table: &toml::Table, field: &str) -> Result<Vec<String>> {
    match table.get(field) {
        None => Ok(Vec::new()),
        Some(TomlValue::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str().map(str::to_string).ok_or_else(|| {
                    validation_err(format!("backlog field `{field}` holds a non-string entry"))
                })
            })
            .collect(),
        Some(_) => Err(validation_err(format!(
            "backlog field `{field}` is not an array"
        ))),
    }
}

fn set_string_array(table: &mut toml::Table, field: &str, values: Vec<String>) {
    if values.is_empty() {
        table.remove(field);
    } else {
        table.insert(
            field.to_string(),
            TomlValue::Array(values.into_iter().map(TomlValue::String).collect()),
        );
    }
}

fn set_string(table: &mut toml::Table, field: &str, value: &str) {
    table.insert(field.to_string(), TomlValue::String(value.to_string()));
}

fn apply_to_row(row: &mut TomlValue, edit: &Edit) -> Result<()> {
    let table = row
        .as_table_mut()
        .ok_or_else(|| validation_err(format!("backlog item \"{}\" is not a table", edit.id)))?;
    if let Some(context) = &edit.context {
        set_string(table, FIELD_CONTEXT, context);
    }
    if edit.tags.is_some() || !edit.add_tags.is_empty() || !edit.remove_tags.is_empty() {
        let base = match &edit.tags {
            Some(replacement) => replacement.clone(),
            None => string_array(table, FIELD_TAGS)?,
        };
        let removed: BTreeSet<&String> = edit.remove_tags.iter().collect();
        let tags: Vec<String> = dedup_keep_order(base.into_iter().chain(edit.add_tags.clone()))
            .into_iter()
            .filter(|t| !removed.contains(t))
            .collect();
        set_string_array(table, FIELD_TAGS, tags);
    }
    if edit.evidence.is_some() || !edit.add_evidence.is_empty() {
        let base = match &edit.evidence {
            Some(replacement) => replacement.clone(),
            None => string_array(table, FIELD_EVIDENCE)?,
        };
        let evidence = dedup_keep_order(base.into_iter().chain(edit.add_evidence.clone()));
        set_string_array(table, FIELD_EVIDENCE, evidence);
    }
    if let Some(origin) = &edit.origin {
        set_string(table, FIELD_ORIGIN, origin);
    }
    if let Some(flow) = &edit.flow {
        set_string(table, FIELD_FLOW, flow);
    }
    if let Some(base_sha) = &edit.base_sha {
        set_string(table, FIELD_BASE_SHA, base_sha);
    }
    Ok(())
}

/// Apply every edit to the live `backlog` rows and validate the result. An
/// unknown id, a duplicate id across edits, or a row or document that fails
/// validation returns `Err` with `doc` unchanged.
pub(crate) fn apply_edits(doc: &mut TomlValue, edits: &[Edit]) -> Result<Vec<EditOutcome>> {
    let mut seen = BTreeSet::new();
    for edit in edits {
        if !seen.insert(edit.id.as_str()) {
            return Err(validation_err(format!(
                "backlog id \"{}\" appears in more than one edit",
                edit.id
            )));
        }
    }

    let mut work = doc.clone();
    let mut outcomes = Vec::with_capacity(edits.len());
    for edit in edits {
        let index = items_array(&work, ARRAY_BACKLOG)
            .iter()
            .position(|row| row.get(FIELD_ID).and_then(TomlValue::as_str) == Some(edit.id.as_str()))
            .ok_or_else(|| {
                tagged_err(
                    ErrorKind::NotFound,
                    schema::backlog_path().ok(),
                    format!("no backlog item with id \"{}\"", edit.id),
                )
            })?;
        let row = &mut work
            .get_mut(ARRAY_BACKLOG)
            .and_then(TomlValue::as_array_mut)
            .ok_or_else(|| validation_err("`backlog` is not an array"))?[index];
        let before = row.clone();
        apply_to_row(row, edit)?;
        schema::validate(&toml_to_json(row))
            .map_err(|e| e.into_tagged(schema::backlog_path().ok()))?;
        outcomes.push(EditOutcome {
            id: edit.id.clone(),
            changed: *row != before,
        });
    }
    schema::validate_ids_unique(&work).map_err(|e| e.into_tagged(schema::backlog_path().ok()))?;

    *doc = work;
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> TomlValue {
        toml::from_str(
            r#"
[[backlog]]
id = "B-00000001"
summary = "first"
status = "open"
tags = ["a", "b"]
evidence = ["x.log"]

[[backlog]]
id = "B-00000002"
summary = "second"
status = "open"
"#,
        )
        .unwrap()
    }

    fn edit(id: &str) -> Edit {
        Edit {
            id: id.into(),
            ..Edit::default()
        }
    }

    fn row<'a>(doc: &'a TomlValue, id: &str) -> &'a TomlValue {
        items_array(doc, ARRAY_BACKLOG)
            .iter()
            .find(|r| r.get(FIELD_ID).and_then(TomlValue::as_str) == Some(id))
            .unwrap()
    }

    fn strs(row: &TomlValue, field: &str) -> Vec<String> {
        row.get(field)
            .and_then(TomlValue::as_array)
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn context_origin_flow_base_sha_are_set() {
        let mut d = doc();
        let out = apply_edits(
            &mut d,
            &[Edit {
                context: Some("why".into()),
                origin: Some("review".into()),
                flow: Some("slug".into()),
                base_sha: Some("abc123".into()),
                ..edit("B-00000002")
            }],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![EditOutcome {
                id: "B-00000002".into(),
                changed: true
            }]
        );
        let r = row(&d, "B-00000002");
        assert_eq!(r.get(FIELD_CONTEXT).unwrap().as_str(), Some("why"));
        assert_eq!(r.get(FIELD_ORIGIN).unwrap().as_str(), Some("review"));
        assert_eq!(r.get(FIELD_FLOW).unwrap().as_str(), Some("slug"));
        assert_eq!(r.get(FIELD_BASE_SHA).unwrap().as_str(), Some("abc123"));
    }

    #[test]
    fn tags_replace_add_remove_keep_order_and_drop_duplicates() {
        let mut d = doc();
        apply_edits(
            &mut d,
            &[Edit {
                add_tags: vec!["c".into(), "a".into(), "d".into()],
                remove_tags: vec!["b".into()],
                ..edit("B-00000001")
            }],
        )
        .unwrap();
        assert_eq!(strs(row(&d, "B-00000001"), FIELD_TAGS), ["a", "c", "d"]);

        apply_edits(
            &mut d,
            &[Edit {
                tags: Some(vec!["z".into(), "y".into(), "z".into()]),
                add_tags: vec!["w".into()],
                ..edit("B-00000001")
            }],
        )
        .unwrap();
        assert_eq!(strs(row(&d, "B-00000001"), FIELD_TAGS), ["z", "y", "w"]);
    }

    #[test]
    fn removing_every_tag_drops_the_field() {
        let mut d = doc();
        apply_edits(
            &mut d,
            &[Edit {
                remove_tags: vec!["a".into(), "b".into()],
                ..edit("B-00000001")
            }],
        )
        .unwrap();
        assert!(row(&d, "B-00000001").get(FIELD_TAGS).is_none());
    }

    #[test]
    fn evidence_replace_and_add() {
        let mut d = doc();
        apply_edits(
            &mut d,
            &[Edit {
                add_evidence: vec!["y.log".into(), "x.log".into()],
                ..edit("B-00000001")
            }],
        )
        .unwrap();
        assert_eq!(
            strs(row(&d, "B-00000001"), FIELD_EVIDENCE),
            ["x.log", "y.log"]
        );

        apply_edits(
            &mut d,
            &[Edit {
                evidence: Some(vec!["only.log".into()]),
                ..edit("B-00000001")
            }],
        )
        .unwrap();
        assert_eq!(strs(row(&d, "B-00000001"), FIELD_EVIDENCE), ["only.log"]);
    }

    #[test]
    fn unchanged_edit_reports_changed_false() {
        let mut d = doc();
        let out = apply_edits(
            &mut d,
            &[Edit {
                add_tags: vec!["a".into()],
                ..edit("B-00000001")
            }],
        )
        .unwrap();
        assert!(!out[0].changed);
    }

    #[test]
    fn unknown_id_is_an_error_naming_it() {
        let mut d = doc();
        let err = apply_edits(&mut d, &[edit("B-deadbeef")]).unwrap_err();
        assert!(err.to_string().contains("B-deadbeef"), "{err}");
    }

    #[test]
    fn duplicate_id_across_edits_is_refused() {
        let mut d = doc();
        let err = apply_edits(&mut d, &[edit("B-00000001"), edit("B-00000001")]).unwrap_err();
        assert!(err.to_string().contains("more than one edit"), "{err}");
    }

    #[test]
    fn failure_leaves_the_document_untouched() {
        let mut d: TomlValue = toml::from_str(
            r#"
[[backlog]]
id = "B-00000001"
summary = "first"
status = "open"

[[backlog]]
id = "B-00000002"
summary = "second"
status = "bogus"
"#,
        )
        .unwrap();
        let before = d.clone();
        let err = apply_edits(
            &mut d,
            &[
                Edit {
                    context: Some("changed".into()),
                    ..edit("B-00000001")
                },
                Edit {
                    context: Some("changed".into()),
                    ..edit("B-00000002")
                },
            ],
        )
        .unwrap_err();
        assert!(err.to_string().contains("bogus"), "{err}");
        assert_eq!(d, before);
    }

    #[test]
    fn editable_fields_are_not_refused() {
        for field in EDITABLE_FIELDS {
            assert_eq!(refusal_for(field), None, "{field}");
        }
    }

    #[test]
    fn triage_managed_fields_name_backlog_triage() {
        for field in [FIELD_STATUS, FIELD_RESOLUTION, FIELD_PROMOTED_TO] {
            let msg = refusal_for(field).unwrap();
            assert!(msg.contains("`backlog triage`"), "{msg}");
        }
    }

    #[test]
    fn identity_fields_point_at_a_successor() {
        for field in [FIELD_SUMMARY, FIELD_KIND, FIELD_AREA] {
            let msg = refusal_for(field).unwrap();
            assert!(
                msg.contains("`backlog add`") && msg.contains("`supersedes`"),
                "{msg}"
            );
        }
    }

    #[test]
    fn minted_fields_are_refused_as_minted() {
        for field in [
            FIELD_ID,
            FIELD_DEDUP_ID,
            FIELD_CREATED,
            FIELD_LAST_SEEN,
            FIELD_SEEN_COUNT,
        ] {
            let msg = refusal_for(field).unwrap();
            assert!(msg.contains("minted"), "{msg}");
        }
    }

    #[test]
    fn relation_fields_name_backlog_relate() {
        for field in [FIELD_RELATED, FIELD_DUPLICATE_OF, FIELD_SUPERSEDES] {
            assert!(refusal_for(field).unwrap().contains("`backlog relate`"));
        }
    }

    #[test]
    fn unknown_field_lists_the_editable_ones() {
        let msg = refusal_for("colour").unwrap();
        assert!(msg.contains("context, tags, evidence"), "{msg}");
    }
}
