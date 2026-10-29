//! `backlog edit` — in-place edits of the free-form fields of a capture.
//!
//! Only `context`, `tags`, `evidence`, `origin`, `flow`, `base_sha` and `tier`
//! are editable. Every other field is owned by another verb, or minted, and an
//! attempt to edit it is refused with the verb that does own it
//! (`refusal_for`). `apply_edits` works on a clone and swaps it in only when
//! every edited row and the whole document validate, so a failure leaves the
//! caller's document untouched.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::add::{field_advisory, preview_doc};
use super::schema::{
    self, ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_AREA, FIELD_BASE_SHA, FIELD_COMPACTED_ON,
    FIELD_CONTEXT, FIELD_EVIDENCE, FIELD_FLOW, FIELD_FOCUS, FIELD_ID, FIELD_KIND,
    FIELD_LAST_UPDATED, FIELD_ORIGIN, FIELD_STATUS, FIELD_SUMMARY, FIELD_TAGS, FIELD_TERMINAL_DATE,
    FIELD_TERMINAL_REASON, FIELD_TIER, MANAGED_FIELDS, MINTED_FIELDS, RELATION_FIELDS,
};
use crate::cli::{WriteIntegrityArgs, write_integrity_opts};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_arg_err, tagged_err};
use crate::io::{
    advise, items_array, mutate_doc_conditional, on_missing_for, read_ndjson_source, relativise,
    repo_or_cwd_root, warn_if_created,
};
use crate::items::{parse_ndjson_batch, row_err, row_string, row_strings};
use crate::output::{Rows, print_rows_compact};

/// The fields `backlog edit` may change.
pub(crate) const EDITABLE_FIELDS: &[&str] = &[
    FIELD_CONTEXT,
    FIELD_TAGS,
    FIELD_EVIDENCE,
    FIELD_ORIGIN,
    FIELD_FLOW,
    FIELD_BASE_SHA,
    FIELD_TIER,
];

const ROW_ADD_TAGS: &str = "add_tags";
const ROW_REMOVE_TAGS: &str = "remove_tags";
const ROW_ADD_EVIDENCE: &str = "add_evidence";

/// `--ndjson` row keys that amend a stored array rather than replace it, the
/// batch spelling of `--add-tag`, `--remove-tag` and `--add-evidence`. They
/// name no stored field, so they are let through before `refusal_for` runs.
const ADDITIVE_ROW_KEYS: &[&str] = &[ROW_ADD_TAGS, ROW_REMOVE_TAGS, ROW_ADD_EVIDENCE];

/// Stored fields only a compacted row carries. Written by `compact`, so a
/// live-row edit naming one is refused as minted.
const COMPACTED_ONLY_FIELDS: &[&str] = &[
    FIELD_TERMINAL_DATE,
    FIELD_TERMINAL_REASON,
    FIELD_COMPACTED_ON,
];

/// One row's edits. `None` / empty leaves the field alone. For `tags` and
/// `evidence` a replacement is applied first, then additions, then (tags only)
/// removals. `tier` is `Some(None)` to remove the field.
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
    pub(crate) tier: Option<Option<String>>,
}

impl Edit {
    fn changes_nothing(&self) -> bool {
        *self
            == (Edit {
                id: self.id.clone(),
                ..Edit::default()
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditOutcome {
    pub(crate) id: String,
    pub(crate) changed: bool,
}

/// The refusal for a field `backlog edit` does not own, naming the verb that
/// does. `None` for an editable field. A field the store does not know is
/// refused too, listing every key an `--ndjson` row accepts, the additive
/// ones included, so a mistyped `add_tag` is told the right spelling.
pub(crate) fn refusal_for(field: &str) -> Option<String> {
    if EDITABLE_FIELDS.contains(&field) {
        return None;
    }
    let msg = match field {
        f if f == FIELD_STATUS || MANAGED_FIELDS.contains(&f) => format!(
            "`{field}` is managed by `backlog triage` (resolve, dismiss, promote, reopen); \
             `backlog edit` cannot change it"
        ),
        FIELD_FOCUS => {
            format!("`{field}` is owned by `backlog focus`; `backlog edit` cannot change it")
        }
        FIELD_SUMMARY | FIELD_KIND | FIELD_AREA => format!(
            "`{field}` feeds the item's `id` and `dedup_id`, so it cannot be edited; capture a \
             successor with `backlog add` and `supersedes`"
        ),
        f if MINTED_FIELDS.contains(&f) || COMPACTED_ONLY_FIELDS.contains(&f) => {
            format!("`{field}` is minted by tomlctl and cannot be edited")
        }
        f if RELATION_FIELDS.contains(&f) => {
            format!("`{field}` is written by `backlog relate`; `backlog edit` cannot change it")
        }
        _ => format!(
            "`{field}` is not an editable backlog field; a `backlog edit` row carries {}",
            row_keys().join(", ")
        ),
    };
    Some(msg)
}

fn validation_err(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, schema::backlog_path().ok(), msg)
}

/// An id absent from the live rows, worded to say so when it survives only in
/// `[[compacted]]`.
fn not_found(doc: &TomlValue, id: &str) -> anyhow::Error {
    let compacted = items_array(doc, ARRAY_COMPACTED)
        .iter()
        .any(|row| row.get(FIELD_ID).and_then(TomlValue::as_str) == Some(id));
    let msg = if compacted {
        format!(
            "backlog id \"{id}\" has been compacted; edit only changes live `[[{ARRAY_BACKLOG}]]` rows"
        )
    } else {
        format!("no backlog item with id \"{id}\"")
    };
    tagged_err(ErrorKind::NotFound, schema::backlog_path().ok(), msg)
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
    match &edit.tier {
        Some(Some(tier)) => set_string(table, FIELD_TIER, tier),
        Some(None) => {
            table.remove(FIELD_TIER);
        }
        None => {}
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
            .ok_or_else(|| not_found(&work, &edit.id))?;
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

/// The keys an `--ndjson` row may carry beside `id`.
fn row_keys() -> Vec<&'static str> {
    EDITABLE_FIELDS
        .iter()
        .chain(ADDITIVE_ROW_KEYS)
        .copied()
        .collect()
}

/// Every row is checked before the store is locked. A key outside the
/// editable set is refused with the core's message, naming the owning verb;
/// a row naming nothing to edit, or an id an earlier row already named, is
/// refused by line.
fn parse_edit_rows(text: &str) -> Result<Vec<Edit>> {
    let objects = parse_ndjson_batch(text)?;
    let mut first_line: BTreeMap<String, usize> = BTreeMap::new();
    let mut edits = Vec::with_capacity(objects.len());
    for (line, row) in objects {
        for key in row
            .keys()
            .filter(|k| k.as_str() != FIELD_ID && !ADDITIVE_ROW_KEYS.contains(&k.as_str()))
        {
            if let Some(msg) = refusal_for(key) {
                return Err(row_err(line, msg));
            }
        }
        let id = match row.get(FIELD_ID) {
            Some(JsonValue::String(s)) if !s.trim().is_empty() => s.clone(),
            _ => return Err(row_err(line, "needs a non-empty string `id`")),
        };
        if let Some(first) = first_line.get(&id) {
            return Err(row_err(
                line,
                format!("{id} is already named on line {first}; a batch names each id once"),
            ));
        }
        first_line.insert(id.clone(), line);
        let string = |key: &str| row_string(line, key, row.get(key));
        let strings = |key: &str| row_strings(line, key, row.get(key));
        let edit = Edit {
            id,
            context: string(FIELD_CONTEXT)?,
            tags: strings(FIELD_TAGS)?,
            add_tags: strings(ROW_ADD_TAGS)?.unwrap_or_default(),
            remove_tags: strings(ROW_REMOVE_TAGS)?.unwrap_or_default(),
            evidence: strings(FIELD_EVIDENCE)?,
            add_evidence: strings(ROW_ADD_EVIDENCE)?.unwrap_or_default(),
            origin: string(FIELD_ORIGIN)?,
            flow: string(FIELD_FLOW)?,
            base_sha: string(FIELD_BASE_SHA)?,
            tier: match row.get(FIELD_TIER) {
                Some(JsonValue::Null) => Some(None),
                _ => string(FIELD_TIER)?.map(Some),
            },
        };
        if edit.changes_nothing() {
            return Err(row_err(
                line,
                format!(
                    "nothing to edit; a row carries at least one of {}",
                    row_keys().join(", ")
                ),
            ));
        }
        edits.push(edit);
    }
    Ok(edits)
}

/// The publication advisories for every value an edit writes, each led by its
/// row's id. A removed tag publishes nothing, so it is not scanned.
fn edit_advisories(edits: &[Edit]) -> Vec<String> {
    let mut out = Vec::new();
    for edit in edits {
        let values = edit
            .context
            .iter()
            .map(|v| (FIELD_CONTEXT, v))
            .chain(
                edit.tags
                    .iter()
                    .flatten()
                    .chain(&edit.add_tags)
                    .map(|v| (FIELD_TAGS, v)),
            )
            .chain(
                edit.evidence
                    .iter()
                    .flatten()
                    .chain(&edit.add_evidence)
                    .map(|v| (FIELD_EVIDENCE, v)),
            );
        out.extend(
            values
                .filter_map(|(field, value)| field_advisory(field, value))
                .map(|line| format!("{}: {line}", edit.id)),
        );
    }
    out
}

/// Apply the edits under one lock and write only when a row changed,
/// stamping `last_updated` with it. Advisories are carried in the envelope
/// as well as on stderr, as `backlog add` carries them.
fn run_edits(edits: &[Edit], dry_run: bool, integrity: &WriteIntegrityArgs) -> Result<()> {
    let path = schema::backlog_path()?;
    let today = crate::time::today_toml_date()?;
    let advisories = edit_advisories(edits);
    for advisory in &advisories {
        advise!("tomlctl: {advisory}");
    }
    let outcomes = if dry_run {
        apply_edits(&mut preview_doc(&path, integrity)?, edits)?
    } else {
        let opts = write_integrity_opts(integrity);
        let on_missing = on_missing_for(&path, integrity.no_create)?;
        let mut outcomes = None;
        let created =
            mutate_doc_conditional(&path, integrity.allow_outside, opts, on_missing, |doc| {
                let result = apply_edits(doc, edits)?;
                let changed = result.iter().any(|o| o.changed);
                if changed && let Some(root) = doc.as_table_mut() {
                    root.insert(FIELD_LAST_UPDATED.to_string(), TomlValue::Datetime(today));
                }
                outcomes = Some(result);
                Ok(changed)
            })?;
        warn_if_created(&path, created);
        outcomes.expect("the closure runs on every success path")
    };

    let rows: Vec<JsonValue> = outcomes
        .iter()
        .map(|o| json!({"id": o.id, "changed": o.changed}))
        .collect();
    let mut out = json!({
        "ok": true,
        "path": relativise(&repo_or_cwd_root()?, &path),
        "rows": rows,
        "advisories": advisories,
    });
    if dry_run {
        out["dry_run"] = json!(true);
    }
    print_rows_compact(&out, Rows::Field("rows"))
}

/// The flag form: one id and the fields its flags name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    id: Option<String>,
    context: Option<String>,
    tags: Option<Vec<String>>,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
    evidence: Option<Vec<String>>,
    add_evidence: Vec<String>,
    origin: Option<String>,
    flow: Option<String>,
    base_sha: Option<String>,
    tier: Option<Option<String>>,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let id = id
        .ok_or_else(|| tagged_arg_err(ErrorKind::Validation, "id", "`backlog edit` needs an id"))?;
    let edit = Edit {
        id,
        context,
        tags,
        add_tags,
        remove_tags,
        evidence,
        add_evidence,
        origin,
        flow,
        base_sha,
        tier,
    };
    if edit.changes_nothing() {
        return Err(tagged_arg_err(
            ErrorKind::Validation,
            "id",
            format!(
                "nothing to edit; pass at least one of {}",
                EDITABLE_FIELDS.join(", ")
            ),
        ));
    }
    run_edits(&[edit], dry_run, &integrity)
}

/// `backlog edit --ndjson`: every row is parsed and checked before the store
/// is locked.
pub(crate) fn dispatch_batch(
    src: &str,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let edits = parse_edit_rows(&read_ndjson_source(src)?)?;
    run_edits(&edits, dry_run, &integrity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::schema::{
        FIELD_CREATED, FIELD_DEDUP_ID, FIELD_DUPLICATE_OF, FIELD_LAST_SEEN, FIELD_PROMOTED_TO,
        FIELD_RELATED, FIELD_RESOLUTION, FIELD_SEEN_COUNT, FIELD_SUPERSEDES,
    };

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

    fn tier_of(d: &TomlValue, id: &str) -> Option<String> {
        row(d, id)
            .get(FIELD_TIER)
            .map(|v| v.as_str().unwrap().to_string())
    }

    #[test]
    fn tier_is_set_replaced_and_cleared() {
        let mut d = doc();
        let set = |d: &mut TomlValue, tier: Option<&str>| {
            apply_edits(
                d,
                &[Edit {
                    tier: Some(tier.map(str::to_string)),
                    ..edit("B-00000001")
                }],
            )
            .unwrap()
        };
        assert!(set(&mut d, Some("lite"))[0].changed);
        assert_eq!(tier_of(&d, "B-00000001").as_deref(), Some("lite"));
        assert!(set(&mut d, Some("deep"))[0].changed);
        assert_eq!(tier_of(&d, "B-00000001").as_deref(), Some("deep"));
        assert!(set(&mut d, None)[0].changed);
        assert_eq!(tier_of(&d, "B-00000001"), None);
    }

    #[test]
    fn a_bad_tier_is_refused_and_leaves_the_document_untouched() {
        let mut d = doc();
        let before = d.clone();
        let err = apply_edits(
            &mut d,
            &[Edit {
                tier: Some(Some("nope".into())),
                ..edit("B-00000001")
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("tier"), "{err}");
        assert_eq!(d, before);
    }

    #[test]
    fn a_batch_row_sets_and_nulls_tier() {
        let edits = parse_edit_rows(
            "{\"id\":\"B-00000001\",\"tier\":\"human\"}\n{\"id\":\"B-00000002\",\"tier\":null}\n",
        )
        .unwrap();
        assert_eq!(edits[0].tier, Some(Some("human".to_string())));
        assert_eq!(edits[1].tier, Some(None));
    }

    #[test]
    fn focus_is_refused_naming_backlog_focus() {
        let msg = refusal_for(FIELD_FOCUS).unwrap();
        assert!(msg.contains("`backlog focus`"), "{msg}");
        let err = parse_edit_rows("{\"id\":\"B-00000001\",\"focus\":2}\n").unwrap_err();
        assert!(err.to_string().contains("`backlog focus`"), "{err}");
    }

    #[test]
    fn unknown_field_lists_the_editable_ones() {
        let msg = refusal_for("colour").unwrap();
        assert!(msg.contains("context, tags, evidence"), "{msg}");
    }

    #[test]
    fn an_unknown_batch_row_key_lists_the_additive_keys() {
        let err = parse_edit_rows("{\"id\":\"B-00000001\",\"add_tag\":[\"x\"]}\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("line 1: `add_tag`"), "{msg}");
        assert!(msg.contains("add_tags, remove_tags, add_evidence"), "{msg}");
    }
}
