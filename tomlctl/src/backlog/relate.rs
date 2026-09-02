//! `backlog relate` — typed edges between items.
//!
//! Three edges over one write path. `relates-to` is symmetric and carries no
//! status change; `duplicates` and `supersedes` are directed and dismiss one
//! side, so each clears the cluster it does not write, stamps the dismissal
//! date alongside the reason, and re-checks the row through
//! `schema::validate` before the document is persisted.
//!
//! Both ids are resolved inside the `mutate_doc` closure, against the
//! post-lock document, and a miss returns `Err` so nothing is written.
//! `compacted` rows do not count — an edge onto an aged-out row would be
//! rewritten by the next `compact`.

use anyhow::Result;
use serde_json::json;
use toml::Value as TomlValue;
use toml::value::Datetime;

use super::schema::{
    self, ARRAY_BACKLOG, FIELD_DISMISS_REASON, FIELD_DISMISSED, FIELD_DUPLICATE_OF, FIELD_ID,
    FIELD_RELATED, FIELD_STATUS, FIELD_SUPERSEDES, MANAGED_FIELDS, STATUS_DISMISSED,
};
use crate::cli::{
    RelationKind, WriteIntegrityArgs, on_missing_for, warn_if_created, write_integrity_opts,
};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{items_array, items_array_mut, mutate_doc};
use crate::output::print_json_compact;

const FIELD_LAST_UPDATED: &str = "last_updated";

pub(crate) fn dispatch(
    a: String,
    to: String,
    relation: RelationKind,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let path = schema::backlog_path()?;
    let today = crate::flow::today_toml_date()?;
    let opts = write_integrity_opts(&integrity);
    let on_missing = on_missing_for(&path, &integrity)?;

    let mut changed = false;
    let created = mutate_doc(&path, integrity.allow_outside, opts, on_missing, |doc| {
        changed = apply_relation(doc, &a, &to, relation, today)?;
        Ok(())
    })?;
    warn_if_created(&path, created);

    print_json_compact(&json!({
        "ok": true,
        "relation": relation_name(relation),
        "a": a,
        "b": to,
        "changed": changed,
        "path": path.display().to_string(),
    }))
}

fn relation_name(relation: RelationKind) -> &'static str {
    match relation {
        RelationKind::RelatesTo => "relates-to",
        RelationKind::Duplicates => "duplicates",
        RelationKind::Supersedes => "supersedes",
    }
}

fn validation_err(msg: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, schema::backlog_path().ok(), msg)
}

fn not_found(id: &str) -> anyhow::Error {
    tagged_err(
        ErrorKind::NotFound,
        schema::backlog_path().ok(),
        format!("no open backlog item with id \"{id}\""),
    )
}

fn index_of(items: &[TomlValue], id: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| item.get(FIELD_ID).and_then(TomlValue::as_str) == Some(id))
}

/// Write one edge, reporting whether the document changed. Every rejection
/// happens before the first mutation, so a caller that propagates the `Err`
/// leaves the document untouched.
fn apply_relation(
    doc: &mut TomlValue,
    a: &str,
    b: &str,
    relation: RelationKind,
    today: Datetime,
) -> Result<bool> {
    if a == b {
        return Err(validation_err(format!(
            "backlog id \"{a}\" cannot be related to itself"
        )));
    }
    let items = items_array(doc, ARRAY_BACKLOG);
    let ia = index_of(items, a).ok_or_else(|| not_found(a))?;
    let ib = index_of(items, b).ok_or_else(|| not_found(b))?;

    let array = items_array_mut(doc, ARRAY_BACKLOG)?;
    let changed = match relation {
        RelationKind::RelatesTo => {
            let forward = append_related(&mut array[ia], b)?;
            let back = append_related(&mut array[ib], a)?;
            forward || back
        }
        RelationKind::Duplicates => {
            let edge = set_edge(&mut array[ia], FIELD_DUPLICATE_OF, b)?;
            let dismissed = dismiss(&mut array[ia], today, format!("duplicate of {b}"))?;
            edge || dismissed
        }
        RelationKind::Supersedes => {
            let edge = set_edge(&mut array[ia], FIELD_SUPERSEDES, b)?;
            let dismissed = dismiss(&mut array[ib], today, format!("superseded by {a}"))?;
            edge || dismissed
        }
    };

    for idx in [ia, ib] {
        schema::validate(&toml_to_json(&array[idx]))
            .map_err(|e| e.into_tagged(schema::backlog_path().ok()))?;
    }

    if changed && let Some(root) = doc.as_table_mut() {
        root.insert(FIELD_LAST_UPDATED.to_string(), TomlValue::Datetime(today));
    }
    Ok(changed)
}

fn table_of<'a>(item: &'a mut TomlValue, field: &str) -> Result<&'a mut toml::Table> {
    item.as_table_mut()
        .ok_or_else(|| validation_err(format!("backlog row is not a table; cannot set `{field}`")))
}

fn append_related(item: &mut TomlValue, id: &str) -> Result<bool> {
    let table = table_of(item, FIELD_RELATED)?;
    let entry = table
        .entry(FIELD_RELATED.to_string())
        .or_insert_with(|| TomlValue::Array(Vec::new()));
    let array = entry.as_array_mut().ok_or_else(|| {
        validation_err(format!("backlog field `{FIELD_RELATED}` is not an array"))
    })?;
    if array
        .iter()
        .any(|v| v.as_str().is_some_and(|existing| existing == id))
    {
        return Ok(false);
    }
    array.push(TomlValue::String(id.to_string()));
    Ok(true)
}

fn set_edge(item: &mut TomlValue, field: &'static str, target: &str) -> Result<bool> {
    let table = table_of(item, field)?;
    if table.get(field).and_then(TomlValue::as_str) == Some(target) {
        return Ok(false);
    }
    table.insert(field.to_string(), TomlValue::String(target.to_string()));
    Ok(true)
}

/// An already-dismissed row keeps the date and reason it was dismissed with:
/// the first dismissal is the one that carries the context.
///
/// Clearing the rest of the managed cluster is what lets an edge dismiss a
/// `promoted` or `resolved` row: `schema::validate` refuses a row still
/// carrying another status's date or companion.
fn dismiss(item: &mut TomlValue, today: Datetime, reason: String) -> Result<bool> {
    let table = table_of(item, FIELD_STATUS)?;
    if table.get(FIELD_STATUS).and_then(TomlValue::as_str) == Some(STATUS_DISMISSED) {
        return Ok(false);
    }
    for field in MANAGED_FIELDS {
        if *field == FIELD_DISMISSED || *field == FIELD_DISMISS_REASON {
            continue;
        }
        table.remove(*field);
    }
    table.insert(
        FIELD_STATUS.to_string(),
        TomlValue::String(STATUS_DISMISSED.to_string()),
    );
    table.insert(FIELD_DISMISSED.to_string(), TomlValue::Datetime(today));
    table.insert(FIELD_DISMISS_REASON.to_string(), TomlValue::String(reason));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    const STORE: &str = r#"schema_version = 1
last_updated = 2026-08-01

[[backlog]]
id = "B-a1b2c3d4"
summary = "readiness probe flakes"
status = "open"

[[backlog]]
id = "B-7f0e2d91"
summary = "readiness gate races the prompt write"
status = "open"

[[compacted]]
id = "B-0000dead"
summary = "aged out"
status = "resolved"
"#;

    fn today() -> Datetime {
        "2026-09-02".parse().unwrap()
    }

    fn doc() -> TomlValue {
        toml::from_str(STORE).unwrap()
    }

    fn row<'a>(doc: &'a TomlValue, id: &str) -> &'a TomlValue {
        items_array(doc, ARRAY_BACKLOG)
            .iter()
            .find(|item| item.get(FIELD_ID).and_then(TomlValue::as_str) == Some(id))
            .unwrap()
    }

    fn related(doc: &TomlValue, id: &str) -> Vec<String> {
        row(doc, id)
            .get(FIELD_RELATED)
            .and_then(TomlValue::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn status(doc: &TomlValue, id: &str) -> String {
        row(doc, id)
            .get(FIELD_STATUS)
            .and_then(TomlValue::as_str)
            .unwrap_or_default()
            .to_string()
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    #[test]
    fn relates_to_writes_both_directions() {
        let mut d = doc();
        assert!(
            apply_relation(
                &mut d,
                "B-a1b2c3d4",
                "B-7f0e2d91",
                RelationKind::RelatesTo,
                today()
            )
            .unwrap()
        );
        assert_eq!(related(&d, "B-a1b2c3d4"), vec!["B-7f0e2d91".to_string()]);
        assert_eq!(related(&d, "B-7f0e2d91"), vec!["B-a1b2c3d4".to_string()]);
        assert_eq!(status(&d, "B-a1b2c3d4"), "open");
        assert_eq!(
            d.get(FIELD_LAST_UPDATED).unwrap().as_datetime().unwrap(),
            &today()
        );
    }

    #[test]
    fn relates_to_is_idempotent() {
        let mut d = doc();
        apply_relation(
            &mut d,
            "B-a1b2c3d4",
            "B-7f0e2d91",
            RelationKind::RelatesTo,
            today(),
        )
        .unwrap();
        let before = toml::to_string(&d).unwrap();
        assert!(
            !apply_relation(
                &mut d,
                "B-a1b2c3d4",
                "B-7f0e2d91",
                RelationKind::RelatesTo,
                today()
            )
            .unwrap()
        );
        assert_eq!(toml::to_string(&d).unwrap(), before);
        assert_eq!(related(&d, "B-a1b2c3d4").len(), 1);
    }

    #[test]
    fn duplicates_dismisses_the_subject_and_validates() {
        let mut d = doc();
        assert!(
            apply_relation(
                &mut d,
                "B-a1b2c3d4",
                "B-7f0e2d91",
                RelationKind::Duplicates,
                today()
            )
            .unwrap()
        );
        let subject = row(&d, "B-a1b2c3d4");
        assert_eq!(
            subject.get(FIELD_DUPLICATE_OF).unwrap().as_str(),
            Some("B-7f0e2d91")
        );
        assert_eq!(status(&d, "B-a1b2c3d4"), STATUS_DISMISSED);
        assert_eq!(
            subject.get(FIELD_DISMISS_REASON).unwrap().as_str(),
            Some("duplicate of B-7f0e2d91")
        );
        assert_eq!(
            subject.get(FIELD_DISMISSED).unwrap().as_datetime(),
            Some(&today())
        );
        assert_eq!(schema::validate(&toml_to_json(subject)), Ok(()));
        assert_eq!(status(&d, "B-7f0e2d91"), "open");
    }

    #[test]
    fn duplicates_keeps_an_earlier_dismissal() {
        let mut d = doc();
        apply_relation(
            &mut d,
            "B-a1b2c3d4",
            "B-7f0e2d91",
            RelationKind::Duplicates,
            today(),
        )
        .unwrap();
        let later: Datetime = "2026-10-05".parse().unwrap();
        assert!(
            !apply_relation(
                &mut d,
                "B-a1b2c3d4",
                "B-7f0e2d91",
                RelationKind::Duplicates,
                later
            )
            .unwrap()
        );
        assert_eq!(
            row(&d, "B-a1b2c3d4").get(FIELD_DISMISSED).unwrap().as_datetime(),
            Some(&today())
        );
    }

    #[test]
    fn duplicates_clears_the_terminal_cluster_it_replaces() {
        let mut d: TomlValue = toml::from_str(
            r#"schema_version = 1

[[backlog]]
id = "B-a1b2c3d4"
summary = "fixed once, then found to be a duplicate"
status = "resolved"
resolved = 2026-08-01
resolution = "fixed in abc123"

[[backlog]]
id = "B-7f0e2d91"
summary = "the original"
status = "open"
"#,
        )
        .unwrap();
        assert!(
            apply_relation(
                &mut d,
                "B-a1b2c3d4",
                "B-7f0e2d91",
                RelationKind::Duplicates,
                today()
            )
            .unwrap()
        );
        let subject = row(&d, "B-a1b2c3d4");
        assert_eq!(subject.get(schema::FIELD_RESOLVED), None);
        assert_eq!(subject.get(schema::FIELD_RESOLUTION), None);
        assert_eq!(status(&d, "B-a1b2c3d4"), STATUS_DISMISSED);
        assert_eq!(schema::validate(&toml_to_json(subject)), Ok(()));
    }

    #[test]
    fn supersedes_dismisses_the_object_not_the_subject() {
        let mut d = doc();
        assert!(
            apply_relation(
                &mut d,
                "B-a1b2c3d4",
                "B-7f0e2d91",
                RelationKind::Supersedes,
                today()
            )
            .unwrap()
        );
        assert_eq!(status(&d, "B-a1b2c3d4"), "open");
        assert_eq!(status(&d, "B-7f0e2d91"), STATUS_DISMISSED);
        let object = row(&d, "B-7f0e2d91");
        assert_eq!(
            object.get(FIELD_DISMISS_REASON).unwrap().as_str(),
            Some("superseded by B-a1b2c3d4")
        );
        assert_eq!(schema::validate(&toml_to_json(object)), Ok(()));
        assert_eq!(
            row(&d, "B-a1b2c3d4").get(FIELD_SUPERSEDES).unwrap().as_str(),
            Some("B-7f0e2d91")
        );
    }

    #[test]
    fn a_self_edge_is_a_validation_error() {
        let mut d = doc();
        let err = apply_relation(
            &mut d,
            "B-a1b2c3d4",
            "B-a1b2c3d4",
            RelationKind::RelatesTo,
            today(),
        )
        .unwrap_err();
        assert_eq!(kind_of(&err), "validation");
        assert_eq!(toml::to_string(&d).unwrap(), toml::to_string(&doc()).unwrap());
    }

    #[test]
    fn an_unknown_or_compacted_id_is_not_found() {
        for (a, b) in [
            ("B-deadbeef", "B-7f0e2d91"),
            ("B-a1b2c3d4", "B-deadbeef"),
            // A compacted row is not a write target.
            ("B-a1b2c3d4", "B-0000dead"),
        ] {
            let mut d = doc();
            let err = apply_relation(&mut d, a, b, RelationKind::RelatesTo, today()).unwrap_err();
            assert_eq!(kind_of(&err), "not_found", "{a} -> {b}");
            assert_eq!(
                toml::to_string(&d).unwrap(),
                toml::to_string(&doc()).unwrap(),
                "{a} -> {b}"
            );
        }
    }

    /// Resolve under a throwaway root, dropping the override before any
    /// assertion runs so a panic cannot leak `TOMLCTL_ROOT` into later tests.
    fn under_root<T>(f: impl FnOnce(&Path) -> T) -> (PathBuf, T) {
        let _guard = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // SAFETY: set_var is unsafe in edition 2024; acceptable inside tests
        // where we hold the env lock.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", root.as_os_str());
        }
        fs::create_dir_all(root.join(".claude")).unwrap();
        fs::write(root.join(".claude").join("backlog.toml"), STORE).unwrap();
        let out = f(&root);
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
        (root, out)
    }

    fn args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    #[test]
    fn dispatch_is_byte_stable_on_a_re_run_and_leaves_evidence_alone() {
        let (_root, (first, second, evidence, err_bytes, before_err)) = under_root(|root| {
            let store = root.join(".claude").join("backlog.toml");
            let evidence_dir = root
                .join(".claude")
                .join("backlog-evidence")
                .join("B-a1b2c3d4");
            fs::create_dir_all(&evidence_dir).unwrap();
            fs::write(evidence_dir.join(".evidence"), "B-a1b2c3d4  probe\n").unwrap();
            fs::write(evidence_dir.join("run.log"), b"trace").unwrap();

            dispatch(
                "B-a1b2c3d4".to_string(),
                "B-7f0e2d91".to_string(),
                RelationKind::Duplicates,
                args(),
            )
            .unwrap();
            let first = fs::read(&store).unwrap();
            dispatch(
                "B-a1b2c3d4".to_string(),
                "B-7f0e2d91".to_string(),
                RelationKind::Duplicates,
                args(),
            )
            .unwrap();
            let second = fs::read(&store).unwrap();

            let evidence = super::super::evidence::list_dir(&evidence_dir).unwrap();

            let before_err = fs::read(&store).unwrap();
            let err = dispatch(
                "B-a1b2c3d4".to_string(),
                "B-deadbeef".to_string(),
                RelationKind::RelatesTo,
                args(),
            )
            .unwrap_err();
            (
                first,
                second,
                evidence,
                fs::read(&store).unwrap(),
                (before_err, kind_of(&err)),
            )
        });

        assert_eq!(first, second, "a re-run must rewrite identical bytes");
        assert!(String::from_utf8_lossy(&first).contains("duplicate of B-7f0e2d91"));
        assert_eq!(evidence, Some(vec![("run.log".to_string(), 5)]));
        let (before, kind) = before_err;
        assert_eq!(kind, "not_found");
        assert_eq!(before, err_bytes, "a rejected relate writes nothing");
    }
}
