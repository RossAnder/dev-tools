//! `tomlctl flow record` — append validated entries to a flow's execution
//! record. The verb resolves the record from the slug, defaults `date` to
//! today (UTC), derives `task_ref` from a task-store row, runs every entry
//! through `record_schema::normalise`, and mints `E<n>` ids under the write
//! lock. A `--ndjson` batch is all-or-nothing: every row is validated before
//! the lock is taken, and the append itself aborts the write on any failure.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use toml::Value as TomlValue;

use super::record_schema::{self, RecordReport};
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs, write_integrity_opts};
use crate::errors::{ErrorKind, tagged_err};
use crate::fields::{self, FieldArgs};
use crate::io::{
    dry_run_read_opts, mutate_doc_conditional, on_missing_for, read_doc, read_json_value_from_arg,
    read_ndjson_source, read_toml, relativise, repo_or_cwd_root, seed_doc_for, stamped_conditional,
    warn_if_created,
};
use crate::items::{items_add_many_with_dedupe, parse_ndjson};
use crate::output::{Rows, print_json_compact, print_rows_compact};

const RECORD_ARRAY: &str = "items";
const RECORD_ID_PREFIX: &str = "E";
const CONTEXT_FILE: &str = "context.toml";

/// One `flow record` invocation, as parsed by clap.
pub(crate) struct RecordRequest {
    pub(crate) slug: String,
    pub(crate) record_type: Option<&'static str>,
    pub(crate) task: Option<u32>,
    pub(crate) json: Option<String>,
    pub(crate) fields: FieldArgs,
    pub(crate) ndjson: Option<String>,
    pub(crate) dry_run: bool,
    pub(crate) integrity: WriteIntegrityArgs,
    pub(crate) stamp: bool,
}

/// The flow's record path and its `context.toml` `scope` globs.
struct FlowTarget {
    root: PathBuf,
    record: PathBuf,
    scope: Vec<String>,
}

fn validation(msg: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

pub(crate) fn dispatch(req: RecordRequest) -> Result<()> {
    let target = resolve_target(&req.slug)?;
    let task_ref = match req.task {
        Some(id) => Some(task_ref_for(&req.slug, id, req.integrity.verify_integrity)?),
        None => None,
    };
    let today = crate::time::today_utc_iso()?;
    let mut entries = build_entries(&req, task_ref.as_deref(), &today)?;
    let batch = req.ndjson.is_some();

    let mut reports = Vec::with_capacity(entries.len());
    for (i, entry) in entries.iter_mut().enumerate() {
        let report =
            record_schema::normalise(entry, &target.scope).with_context(|| row_label(batch, i))?;
        reports.push(report);
    }
    let rows: Vec<JsonValue> = entries.into_iter().map(JsonValue::Object).collect();

    let ids = if req.dry_run {
        preview_ids(&target.record, &rows, req.integrity.verify_integrity)?
    } else {
        append(&target.record, &rows, &req)?
    };

    let path = relativise(&target.root, &target.record);
    let results: Vec<JsonValue> = rows
        .iter()
        .zip(&ids)
        .zip(&reports)
        .map(|((row, id), report)| row_result(id, row, report))
        .collect();

    if batch {
        let mut envelope = JsonMap::new();
        envelope.insert("ok".into(), JsonValue::Bool(true));
        if req.dry_run {
            envelope.insert("dry_run".into(), JsonValue::Bool(true));
        }
        envelope.insert("ids".into(), json!(ids));
        envelope.insert("path".into(), JsonValue::String(path));
        envelope.insert("rows".into(), JsonValue::Array(results));
        return print_rows_compact(&JsonValue::Object(envelope), Rows::Field("rows"));
    }

    let mut envelope = JsonMap::new();
    envelope.insert("ok".into(), JsonValue::Bool(true));
    if req.dry_run {
        envelope.insert("dry_run".into(), JsonValue::Bool(true));
    }
    if let Some(JsonValue::Object(result)) = results.into_iter().next() {
        envelope.extend(result);
    }
    envelope.insert("path".into(), JsonValue::String(path));
    print_json_compact(&JsonValue::Object(envelope))
}

fn row_label(batch: bool, index: usize) -> String {
    if batch {
        format!("--ndjson row {}", index + 1)
    } else {
        "flow record".to_string()
    }
}

fn row_result(id: &str, row: &JsonValue, report: &RecordReport) -> JsonValue {
    json!({
        "id": id,
        "type": row.get("type").cloned().unwrap_or(JsonValue::Null),
        "task_ref": row.get("task_ref").cloned().unwrap_or(JsonValue::Null),
        "truncated": report.truncated,
        "dropped_files": report.dropped_files,
        "scope_warnings": report.scope_warnings,
    })
}

/// The record a flow's `[artifacts].execution_record` names, else the
/// `execution-record.toml` beside its `context.toml`. A slug with no
/// `context.toml` is refused, so a mistyped slug never seeds a stray flow
/// directory.
fn resolve_target(slug: &str) -> Result<FlowTarget> {
    super::validate_slug(slug)?;
    let root = repo_or_cwd_root()?;
    let dir = root.join(".claude").join("flows").join(slug);
    let context_path = dir.join(CONTEXT_FILE);
    if !context_path.is_file() {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(context_path.clone()),
            format!(
                "no flow `{slug}`: `{}` does not exist (run `tomlctl flow init`)",
                context_path.display()
            ),
        ));
    }
    let context = read_toml(&context_path)
        .with_context(|| format!("reading `{}`", context_path.display()))?;
    let scope = context
        .get("scope")
        .and_then(TomlValue::as_array)
        .map(|globs| {
            globs
                .iter()
                .filter_map(TomlValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let record = super::execution_record_path(&root, &dir, &context_path, Some(&context))?;
    Ok(FlowTarget {
        root,
        record,
        scope,
    })
}

/// The `ref` of task `id` in the flow's task store, read through the same
/// loader `tasks show` uses.
fn task_ref_for(slug: &str, id: u32, verify_integrity: bool) -> Result<String> {
    let path = crate::tasks::resolve_store_path(Some(slug), None)?;
    let integrity = ReadIntegrityArgs {
        verify_integrity,
        strict_read: false,
    };
    let store = crate::tasks::load_store(&path, &integrity)?;
    let row = store.find(id).ok_or_else(|| {
        tagged_err(
            ErrorKind::NotFound,
            None,
            format!("--task {id}: no task {id} in `{}`", path.display()),
        )
    })?;
    Ok(row.r#ref.clone())
}

/// Every entry the call writes, before schema validation: the `--json` base
/// with the field flags over it, each `--ndjson` row laid over that, then
/// `type`, `task_ref` and `date` filled in from the flags.
fn build_entries(
    req: &RecordRequest,
    task_ref: Option<&str>,
    today: &str,
) -> Result<Vec<JsonMap<String, JsonValue>>> {
    let json_base = req
        .json
        .as_deref()
        .map(|arg| read_json_value_from_arg(arg).context("parsing --json"))
        .transpose()?;
    let base = match fields::build(&req.fields, json_base)? {
        None => JsonMap::new(),
        Some(JsonValue::Object(map)) => map,
        Some(other) => {
            return Err(validation(format!(
                "--json must be a JSON object, got JSON {}",
                crate::convert::json_type_name(&other)
            )));
        }
    };

    let mut entries = match &req.ndjson {
        None => vec![base],
        Some(src) => {
            let text = read_ndjson_source(src)?;
            let rows = parse_ndjson(&text)?;
            if rows.is_empty() {
                return Err(validation(
                    "--ndjson holds no rows; give one JSON object per line".to_string(),
                ));
            }
            rows.into_iter()
                .enumerate()
                .map(|(i, row)| match row {
                    JsonValue::Object(row) => {
                        let mut entry = base.clone();
                        entry.extend(row);
                        Ok(entry)
                    }
                    other => Err(validation(format!(
                        "--ndjson row {}: must be a JSON object, got JSON {}",
                        i + 1,
                        crate::convert::json_type_name(&other)
                    ))),
                })
                .collect::<Result<Vec<_>>>()?
        }
    };

    let batch = req.ndjson.is_some();
    for (i, entry) in entries.iter_mut().enumerate() {
        fill_defaults(entry, req.record_type, task_ref, today)
            .with_context(|| row_label(batch, i))?;
    }
    Ok(entries)
}

fn is_unset(value: Option<&JsonValue>) -> bool {
    match value {
        None | Some(JsonValue::Null) => true,
        Some(JsonValue::String(s)) => s.is_empty(),
        Some(_) => false,
    }
}

/// Fill `type`, `task_ref` and `date` from the flags. A payload value that
/// disagrees with its flag is refused rather than silently overwritten.
fn fill_defaults(
    entry: &mut JsonMap<String, JsonValue>,
    record_type: Option<&str>,
    task_ref: Option<&str>,
    today: &str,
) -> Result<()> {
    for (field, flag, value) in [
        ("type", "--type", record_type),
        ("task_ref", "--task", task_ref),
    ] {
        let Some(value) = value else { continue };
        match entry.get(field) {
            Some(current) if !is_unset(Some(current)) && current.as_str() != Some(value) => {
                return Err(validation(format!(
                    "payload `{field}` is {current} but {flag} gives `{value}`; drop one of them"
                )));
            }
            _ => {
                entry.insert(field.to_string(), JsonValue::String(value.to_string()));
            }
        }
    }
    if is_unset(entry.get("date")) {
        entry.insert("date".to_string(), JsonValue::String(today.to_string()));
    }
    if is_unset(entry.get("agent")) {
        return Err(validation(
            "execution-record entry is missing required field `agent`; pass `--set agent=<name>`"
                .to_string(),
        ));
    }
    Ok(())
}

/// The ids a live append would mint, computed on a copy of the record (or of
/// the skeleton a missing record would be seeded with). Nothing is written.
fn preview_ids(record: &Path, rows: &[JsonValue], verify_integrity: bool) -> Result<Vec<String>> {
    let mint = |doc: &TomlValue| -> Result<Vec<String>> {
        let mut copy = doc.clone();
        let outcome = items_add_many_with_dedupe(
            &mut copy,
            RECORD_ARRAY,
            rows,
            None,
            &[],
            Some(RECORD_ID_PREFIX),
        )?;
        Ok(outcome.ids)
    };
    if record.exists() {
        read_doc(record, dry_run_read_opts(verify_integrity), mint)
    } else {
        mint(&seed_doc_for(record)?)
    }
}

/// Append every row in one locked write, minting each id inside the lock so
/// two concurrent writers never take the same one.
fn append(record: &Path, rows: &[JsonValue], req: &RecordRequest) -> Result<Vec<String>> {
    let integrity = &req.integrity;
    let on_missing = on_missing_for(record, integrity.no_create)?;
    let mut ids = Vec::new();
    let created = mutate_doc_conditional(
        record,
        integrity.allow_outside,
        write_integrity_opts(integrity),
        on_missing,
        stamped_conditional(req.stamp, |doc| {
            let outcome = items_add_many_with_dedupe(
                doc,
                RECORD_ARRAY,
                rows,
                None,
                &[],
                Some(RECORD_ID_PREFIX),
            )?;
            ids = outcome.ids;
            Ok(outcome.added > 0)
        }),
    )?;
    warn_if_created(record, created);
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(v: JsonValue) -> JsonMap<String, JsonValue> {
        match v {
            JsonValue::Object(m) => m,
            _ => panic!("fixture must be an object"),
        }
    }

    fn message(err: anyhow::Error) -> String {
        format!("{err:#}")
    }

    #[test]
    fn fills_type_task_ref_and_date() {
        let mut e = entry(json!({"agent": "implement"}));
        fill_defaults(&mut e, Some("deviation"), Some("add-x"), "2026-10-09").unwrap();
        assert_eq!(e["type"], json!("deviation"));
        assert_eq!(e["task_ref"], json!("add-x"));
        assert_eq!(e["date"], json!("2026-10-09"));
    }

    #[test]
    fn keeps_a_payload_date_and_an_agreeing_type() {
        let mut e = entry(json!({"agent": "a", "date": "2026-01-02", "type": "checkpoint"}));
        fill_defaults(&mut e, Some("checkpoint"), None, "2026-10-09").unwrap();
        assert_eq!(e["date"], json!("2026-01-02"));
    }

    #[test]
    fn disagreeing_task_ref_or_type_is_refused() {
        let mut e = entry(json!({"agent": "a", "task_ref": "other"}));
        let msg = message(fill_defaults(&mut e, None, Some("add-x"), "d").unwrap_err());
        assert!(msg.contains("--task") && msg.contains("other"), "{msg}");

        let mut e = entry(json!({"agent": "a", "type": "deferral"}));
        let msg = message(fill_defaults(&mut e, Some("deviation"), None, "d").unwrap_err());
        assert!(msg.contains("--type"), "{msg}");
    }

    #[test]
    fn missing_agent_names_the_flag() {
        for payload in [json!({}), json!({"agent": ""}), json!({"agent": null})] {
            let mut e = entry(payload);
            let err = fill_defaults(&mut e, Some("checkpoint"), None, "d").unwrap_err();
            let tag = err
                .downcast_ref::<crate::errors::TaggedError>()
                .expect("tagged");
            assert_eq!(tag.kind.as_str(), "validation");
            assert!(
                tag.message.contains("--set agent=<name>"),
                "{}",
                tag.message
            );
        }
    }
}
