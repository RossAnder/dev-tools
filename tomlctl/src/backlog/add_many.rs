//! `backlog add-many` — capture a batch of discoveries under one lock, one
//! read and one write.
//!
//! Each NDJSON line is one `add --json` payload, run through the same
//! `add_item` in input order, so two lines that fingerprint-collide behave as
//! two `add` calls in sequence. The batch is all-or-nothing: every line is
//! parsed and validated before the lock is taken, and a row refused inside
//! the lock aborts the batch before anything is written.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map as JsonMap, Value as JsonValue};
use toml::Value as TomlValue;

use super::add::{self, AddOutcome, AddRequest};
use super::schema;
use crate::cli::{OnDuplicate, WriteIntegrityArgs, write_integrity_opts};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{self, advise, on_missing_for, warn_if_created};
use crate::output::{Rows, build_dry_run_plan_envelope, print_rows_compact};

/// One validated line and the 1-based source line it came from, which every
/// error and every per-row envelope names.
struct Row {
    line: usize,
    req: AddRequest,
    advisories: Vec<String>,
}

pub(crate) fn dispatch(
    ndjson: String,
    auto_base_sha: bool,
    on_duplicate: OnDuplicate,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let file = schema::backlog_path()?;
    let base_sha = add::resolve_base_sha(None, auto_base_sha);
    let rows = parse_rows(
        &io::read_ndjson_source(&ndjson)?,
        on_duplicate,
        base_sha,
        &file,
    )?;
    let advisories = batch_advisories(&rows);
    for advisory in &advisories {
        advise!("tomlctl: {advisory}");
    }

    if dry_run {
        let mut doc = add::preview_doc(&file, &integrity)?;
        let outcomes = apply_rows(&mut doc, &rows, &file)?;
        let lines = rows.iter().map(|row| row.line);
        let plan = add::preview_plan(doc, lines.zip(&outcomes));
        let mut envelope = build_dry_run_plan_envelope(&plan);
        if let Some(map) = envelope.as_object_mut() {
            map.insert("rows".to_string(), row_envelopes(&rows, &outcomes));
            insert_advisories(map, advisories);
        }
        return print_rows_compact(&envelope, Rows::Field("rows"));
    }

    let opts = write_integrity_opts(&integrity);
    let on_missing = on_missing_for(&file, integrity.no_create)?;
    let mut outcomes = Vec::new();
    let created =
        io::mutate_doc_conditional(&file, integrity.allow_outside, opts, on_missing, |doc| {
            outcomes = apply_rows(doc, &rows, &file)?;
            // An all-`skip` batch leaves the file and its sidecar untouched,
            // exactly as a single skipped `add` does.
            Ok(outcomes
                .iter()
                .any(|outcome| !matches!(outcome, AddOutcome::Skipped { .. })))
        })?;
    warn_if_created(&file, created);

    let mut envelope = JsonMap::new();
    envelope.insert("ok".to_string(), true.into());
    envelope.insert(
        "path".to_string(),
        serde_json::json!(io::relativise(&io::repo_or_cwd_root()?, &file)),
    );
    envelope.insert("created".to_string(), created.into());
    for action in ["added", "bumped", "skipped"] {
        let ids = ids_with_action(&outcomes, action);
        envelope.insert(action.to_string(), ids.into());
    }
    envelope.insert("rows".to_string(), row_envelopes(&rows, &outcomes));
    insert_advisories(&mut envelope, advisories);
    print_rows_compact(&JsonValue::Object(envelope), Rows::Field("rows"))
}

/// Parse and validate every line before any lock is taken. Blank lines are
/// skipped but still counted, so `line N` is the line the caller wrote.
fn parse_rows(
    text: &str,
    on_duplicate: OnDuplicate,
    base_sha: Option<String>,
    file: &Path,
) -> Result<Vec<Row>> {
    let today = crate::time::today_toml_date()?;
    let refuse = |line: usize, message: String| {
        tagged_err(
            ErrorKind::Validation,
            Some(file.to_path_buf()),
            format!("line {line}: {message}"),
        )
    };
    let mut rows = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = idx + 1;
        if raw.trim().is_empty() {
            continue;
        }
        let payload: JsonValue = serde_json::from_str(raw).map_err(|e| {
            refuse(
                line,
                format!("expected one JSON object per line, as `backlog add --json` takes: {e}"),
            )
        })?;
        let req = add::payload_request(payload, on_duplicate, today, base_sha.clone(), file)
            .with_context(|| format!("line {line}"))?;
        let advisories = add::advisories(&req);
        rows.push(Row {
            line,
            req,
            advisories,
        });
    }
    if rows.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            Some(file.to_path_buf()),
            "the NDJSON source carries no rows",
        ));
    }
    Ok(rows)
}

fn apply_rows(doc: &mut TomlValue, rows: &[Row], file: &Path) -> Result<Vec<AddOutcome>> {
    rows.iter()
        .map(|row| add::add_item(doc, &row.req, file).with_context(|| format!("line {}", row.line)))
        .collect()
}

/// Ids in first-seen order, each once: a row minted and then bumped by a
/// later line is listed under both `added` and `bumped`.
fn ids_with_action(outcomes: &[AddOutcome], wanted: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for outcome in outcomes {
        let action = match outcome {
            AddOutcome::Added { .. } => "added",
            AddOutcome::Bumped { .. } => "bumped",
            AddOutcome::Skipped { .. } => "skipped",
        };
        if action == wanted && !ids.iter().any(|id| id == outcome.id()) {
            ids.push(outcome.id().to_string());
        }
    }
    ids
}

fn row_envelopes(rows: &[Row], outcomes: &[AddOutcome]) -> JsonValue {
    rows.iter()
        .zip(outcomes)
        .map(|(row, outcome)| {
            let mut entry = JsonMap::new();
            entry.insert("line".to_string(), row.line.into());
            outcome.extend_envelope(&mut entry);
            entry.insert("advisories".to_string(), serde_json::json!(row.advisories));
            JsonValue::Object(entry)
        })
        .collect()
}

fn batch_advisories(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .flat_map(|row| {
            row.advisories
                .iter()
                .map(move |advisory| format!("line {}: {advisory}", row.line))
        })
        .collect()
}

/// Present only when some row raised one; each row's own list is always on
/// its `rows` entry.
fn insert_advisories(envelope: &mut JsonMap<String, JsonValue>, advisories: Vec<String>) {
    if !advisories.is_empty() {
        envelope.insert("advisories".to_string(), advisories.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::schema::{
        ARRAY_BACKLOG, FIELD_BASE_SHA, FIELD_ID, FIELD_RELATED, FIELD_SEEN_COUNT,
    };
    use crate::errors::TaggedError;
    use crate::io::items_array;
    use crate::test_support::with_root;
    use std::fs;
    use std::path::PathBuf;

    fn wargs() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    /// Stage `ndjson` beside the store and run the batch against it.
    fn run_with(root: &Path, ndjson: &str, on_duplicate: OnDuplicate) -> Result<()> {
        let src = root.join("batch.ndjson");
        fs::write(&src, ndjson).unwrap();
        dispatch(
            src.display().to_string(),
            false,
            on_duplicate,
            false,
            wargs(),
        )
    }

    fn run(root: &Path, ndjson: &str) -> Result<()> {
        run_with(root, ndjson, OnDuplicate::Bump)
    }

    fn store_path(root: &Path) -> PathBuf {
        root.join(".claude").join("backlog.toml")
    }

    fn rows(root: &Path) -> Vec<TomlValue> {
        let doc: TomlValue =
            toml::from_str(&fs::read_to_string(store_path(root)).unwrap()).unwrap();
        items_array(&doc, ARRAY_BACKLOG).to_vec()
    }

    fn message(err: &anyhow::Error) -> String {
        format!("{err:#}")
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    const FIRST: &str = r#"{"summary":"sidecar rename loses to the indexer","kind":"bug","area":"tomlctl/src/io.rs"}"#;
    const SECOND: &str =
        r#"{"summary":"ledger lock times out under load","kind":"bug","area":"tomlctl/src/io.rs"}"#;

    #[test]
    fn a_colliding_line_bumps_the_row_an_earlier_line_minted() {
        with_root(|root| {
            let repeat = r#"{"summary":"Sidecar rename LOSES to the indexer!","kind":"bug","area":"tomlctl/src/io.rs","tags":["windows"]}"#;
            run(root, &format!("{FIRST}\n{SECOND}\n{repeat}\n")).unwrap();
            let rows = rows(root);
            assert_eq!(rows.len(), 2, "{rows:?}");
            assert_eq!(rows[0][FIELD_SEEN_COUNT].as_integer(), Some(2));
            assert_eq!(rows[0]["tags"][0].as_str(), Some("windows"));
            assert_eq!(rows[1][FIELD_SEEN_COUNT].as_integer(), Some(1));
        });
    }

    #[test]
    fn a_line_may_relate_to_an_id_an_earlier_line_minted() {
        with_root(|root| {
            run(root, FIRST).unwrap();
            let first = rows(root)[0][FIELD_ID].as_str().unwrap().to_string();
            fs::remove_file(store_path(root)).unwrap();
            fs::remove_file(crate::integrity::sidecar_path(&store_path(root))).unwrap();

            let pointing = format!(
                r#"{{"summary":"lock timeout masks the rename failure","related":["{first}"]}}"#
            );
            run(root, &format!("{FIRST}\n{pointing}\n")).unwrap();
            let rows = rows(root);
            assert_eq!(rows[1][FIELD_RELATED][0].as_str(), Some(first.as_str()));
        });
    }

    #[test]
    fn a_malformed_line_is_named_by_its_source_line_and_nothing_lands() {
        with_root(|root| {
            // The blank line still counts, so the broken payload is line 3.
            let err = run(root, &format!("{FIRST}\n\n{{\"summary\":\n")).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(message(&err).starts_with("line 3: "), "{}", message(&err));
            assert!(!store_path(root).exists());
        });
    }

    #[test]
    fn an_unknown_key_is_refused_by_line_and_key() {
        with_root(|root| {
            let typo = r#"{"summary":"a typo'd field","sumary":"x"}"#;
            let err = run(root, &format!("{FIRST}\n{typo}\n")).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            let rendered = message(&err);
            assert!(rendered.starts_with("line 2: "), "{rendered}");
            assert!(rendered.contains("`sumary`"), "{rendered}");
            assert!(!store_path(root).exists());
        });
    }

    #[test]
    fn every_stored_row_field_is_a_known_key() {
        with_root(|root| {
            let resolved = r#"{"summary":"already fixed elsewhere","status":"resolved","resolved":"2026-09-01","resolution":"fixed in abc123","base_sha":"abc123","id":"B-00000000","seen_count":4}"#;
            run(root, resolved).unwrap();
            let rows = rows(root);
            assert_eq!(rows[0][FIELD_BASE_SHA].as_str(), Some("abc123"));
            assert!(rows[0]["resolved"].as_datetime().is_some());
        });
    }

    #[test]
    fn a_row_refused_inside_the_lock_aborts_the_whole_batch() {
        with_root(|root| {
            run(root, FIRST).unwrap();
            let file = store_path(root);
            let sidecar = crate::integrity::sidecar_path(&file);
            let before = (fs::read(&file).unwrap(), fs::read(&sidecar).unwrap());

            let err =
                run_with(root, &format!("{SECOND}\n{FIRST}\n"), OnDuplicate::Fail).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(message(&err).starts_with("line 2: "), "{}", message(&err));
            assert_eq!(
                (fs::read(&file).unwrap(), fs::read(&sidecar).unwrap()),
                before,
                "the valid first line must not land without the second"
            );
        });
    }

    #[test]
    fn a_source_with_no_rows_is_refused() {
        with_root(|root| {
            let err = run(root, "\n  \n").unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(!store_path(root).exists());
        });
    }
}
