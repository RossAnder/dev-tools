//! `BacklogOp` fan-out to the per-verb leaf modules.
//!
//! Every variant is destructured field-by-field with no `..` rest pattern, so
//! a flag added to a variant fails to compile here rather than being silently
//! dropped on the way to its leaf.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::edit::{EDITABLE_FIELDS, Edit, apply_edits, refusal_for};
use super::schema;
use crate::cli::{BacklogOp, EvidenceOp, WriteIntegrityArgs, write_integrity_opts};
use crate::errors::{ErrorKind, tagged_arg_err};
use crate::io::{
    dry_run_read_opts, mutate_doc_conditional, on_missing_for, read_doc, read_flag_file,
    read_ndjson_source, relativise, repo_or_cwd_root, warn_if_created,
};
use crate::output::{Rows, print_rows_compact};

/// Resolve a triage companion given as `--<flag>` or `--<flag>-file`. The
/// file form is read through `read_flag_file`, which drops a leading BOM and
/// one trailing newline.
fn literal_or_file(
    literal: Option<String>,
    file: Option<PathBuf>,
    flag: &str,
) -> Result<Option<String>> {
    let (Some(path), None) = (file, literal.as_ref()) else {
        return Ok(literal);
    };
    read_flag_file(&path, &format!("{flag}-file")).map(Some)
}

fn row_err(line: usize, msg: impl std::fmt::Display) -> anyhow::Error {
    tagged_arg_err(
        ErrorKind::Validation,
        "ndjson",
        format!("line {line}: {msg}"),
    )
}

fn row_string(
    line: usize,
    row: &serde_json::Map<String, JsonValue>,
    key: &str,
) -> Result<Option<String>> {
    match row.get(key) {
        None => Ok(None),
        Some(JsonValue::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(row_err(line, format!("`{key}` must be a string"))),
    }
}

fn row_strings(
    line: usize,
    row: &serde_json::Map<String, JsonValue>,
    key: &str,
) -> Result<Option<Vec<String>>> {
    match row.get(key) {
        None => Ok(None),
        Some(JsonValue::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| row_err(line, format!("`{key}` must be an array of strings")))
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Some(_) => Err(row_err(
            line,
            format!("`{key}` must be an array of strings"),
        )),
    }
}

/// Every row is checked before the store is locked. A key outside the
/// editable set is refused with the core's message, naming the owning verb.
fn parse_edit_rows(text: &str) -> Result<Vec<Edit>> {
    let objects = crate::items::parse_ndjson_objects(text)?;
    if objects.is_empty() {
        return Err(tagged_arg_err(
            ErrorKind::Validation,
            "ndjson",
            "the batch names no rows; each row is {\"id\", <field>: <value>}",
        ));
    }
    let mut edits = Vec::with_capacity(objects.len());
    for (line, row) in objects {
        for key in row.keys().filter(|k| k.as_str() != schema::FIELD_ID) {
            if let Some(msg) = refusal_for(key) {
                return Err(row_err(line, msg));
            }
        }
        let id = match row.get(schema::FIELD_ID) {
            Some(JsonValue::String(s)) if !s.trim().is_empty() => s.clone(),
            _ => return Err(row_err(line, "needs a non-empty string `id`")),
        };
        edits.push(Edit {
            id,
            context: row_string(line, &row, schema::FIELD_CONTEXT)?,
            tags: row_strings(line, &row, schema::FIELD_TAGS)?,
            evidence: row_strings(line, &row, schema::FIELD_EVIDENCE)?,
            origin: row_string(line, &row, schema::FIELD_ORIGIN)?,
            flow: row_string(line, &row, schema::FIELD_FLOW)?,
            base_sha: row_string(line, &row, schema::FIELD_BASE_SHA)?,
            ..Edit::default()
        });
    }
    Ok(edits)
}

/// Apply the edits under one lock and write only when a row changed,
/// stamping `last_updated` with it.
fn run_edits(edits: &[Edit], dry_run: bool, integrity: &WriteIntegrityArgs) -> Result<()> {
    let path = schema::backlog_path()?;
    let today = crate::time::today_toml_date()?;
    let outcomes = if dry_run {
        if path.exists() {
            read_doc(
                &path,
                dry_run_read_opts(integrity.verify_integrity),
                |doc| apply_edits(&mut doc.clone(), edits),
            )?
        } else {
            apply_edits(&mut TomlValue::Table(toml::Table::new()), edits)?
        }
    } else {
        let opts = write_integrity_opts(integrity);
        let on_missing = on_missing_for(&path, integrity.no_create)?;
        let mut outcomes = None;
        let created =
            mutate_doc_conditional(&path, integrity.allow_outside, opts, on_missing, |doc| {
                let result = apply_edits(doc, edits)?;
                let changed = result.iter().any(|o| o.changed);
                if changed && let Some(root) = doc.as_table_mut() {
                    root.insert(
                        schema::FIELD_LAST_UPDATED.to_string(),
                        TomlValue::Datetime(today),
                    );
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
    });
    if dry_run {
        out["dry_run"] = json!(true);
    }
    print_rows_compact(&out, Rows::Field("rows"))
}

pub(crate) fn dispatch(op: BacklogOp) -> Result<()> {
    match op {
        BacklogOp::Add {
            summary,
            kind,
            area,
            tag,
            evidence,
            related,
            context,
            origin,
            flow,
            base_sha,
            auto_base_sha,
            on_duplicate,
            json,
            dry_run,
            integrity,
        } => crate::backlog::add::dispatch(
            summary,
            kind,
            area,
            tag,
            evidence,
            related,
            context,
            origin,
            flow,
            base_sha,
            auto_base_sha,
            on_duplicate,
            json,
            dry_run,
            integrity,
        ),
        BacklogOp::AddMany {
            ndjson,
            auto_base_sha,
            on_duplicate,
            dry_run,
            integrity,
        } => crate::backlog::add_many::dispatch(
            ndjson,
            auto_base_sha,
            on_duplicate,
            dry_run,
            integrity,
        ),
        BacklogOp::Check {
            summary,
            ndjson,
            area,
            kind,
            tag,
            similarity_strong,
            similarity_related,
            integrity,
        } => crate::backlog::check::dispatch(
            summary,
            ndjson,
            area,
            kind,
            tag,
            similarity_strong,
            similarity_related,
            integrity,
        ),
        BacklogOp::List {
            status,
            kind,
            tag,
            open,
            live,
            area_prefix,
            has_evidence,
            count,
            query,
            integrity,
        } => crate::backlog::query::dispatch_list(
            status,
            kind,
            tag,
            open,
            live,
            area_prefix,
            has_evidence,
            count,
            query,
            integrity,
        ),
        BacklogOp::Show { ids, integrity } => {
            crate::backlog::query::dispatch_show(&ids.into_ids(), integrity)
        }
        BacklogOp::Get { ids, integrity } => {
            crate::backlog::query::dispatch_get(&ids.into_ids(), integrity)
        }
        BacklogOp::Edit {
            id: _,
            context: _,
            context_file: _,
            tags: _,
            add_tag: _,
            remove_tag: _,
            evidence: _,
            add_evidence: _,
            origin: _,
            flow: _,
            base_sha: _,
            ndjson: Some(src),
            dry_run,
            integrity,
        } => {
            let edits = parse_edit_rows(&read_ndjson_source(&src)?)?;
            run_edits(&edits, dry_run, &integrity)
        }
        BacklogOp::Edit {
            id,
            context,
            context_file,
            tags,
            add_tag,
            remove_tag,
            evidence,
            add_evidence,
            origin,
            flow,
            base_sha,
            ndjson: None,
            dry_run,
            integrity,
        } => {
            let id = id.ok_or_else(|| {
                tagged_arg_err(ErrorKind::Validation, "id", "`backlog edit` needs an id")
            })?;
            let edit = Edit {
                id: id.clone(),
                context: literal_or_file(context, context_file, "context")?,
                tags,
                add_tags: add_tag,
                remove_tags: remove_tag,
                evidence,
                add_evidence,
                origin,
                flow,
                base_sha,
            };
            if edit
                == (Edit {
                    id,
                    ..Edit::default()
                })
            {
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
        BacklogOp::Relate {
            a,
            to,
            relation,
            integrity,
        } => crate::backlog::relate::dispatch(a, to, relation, integrity),
        // The parser's conflicts leave every flag-form field empty here.
        BacklogOp::Triage {
            ids: _,
            mode: _,
            to: _,
            reason: _,
            reason_file: _,
            resolution: _,
            resolution_file: _,
            rationale: _,
            rationale_file: _,
            external: _,
            allow_closed: _,
            expect_status,
            ndjson: Some(src),
            dry_run,
            integrity,
        } => crate::backlog::triage::dispatch_batch(&src, expect_status, dry_run, integrity),
        BacklogOp::Triage {
            ids,
            mode,
            to,
            reason,
            reason_file,
            resolution,
            resolution_file,
            rationale,
            rationale_file,
            external,
            allow_closed,
            expect_status,
            ndjson: None,
            dry_run: _,
            integrity,
        } => crate::backlog::triage::dispatch(
            ids.into_ids(),
            mode,
            to,
            literal_or_file(reason, reason_file, "reason")?,
            literal_or_file(resolution, resolution_file, "resolution")?,
            literal_or_file(rationale, rationale_file, "rationale")?,
            external,
            allow_closed,
            expect_status,
            integrity,
        ),
        BacklogOp::Reconcile {
            flow,
            apply,
            adopt,
            integrity,
        } => crate::backlog::reconcile::dispatch(flow, apply, adopt, integrity),
        BacklogOp::Cluster {
            by,
            min_size,
            min_shared_tags,
            per_tag,
            all_statuses,
            integrity,
        } => crate::backlog::cluster::dispatch(
            by,
            min_size,
            min_shared_tags,
            per_tag,
            all_statuses,
            integrity,
        ),
        BacklogOp::Compact {
            older_than,
            dry_run,
            integrity,
        } => crate::backlog::compact::dispatch(older_than, dry_run, integrity),
        BacklogOp::Evidence { op } => match op {
            EvidenceOp::Dir {
                id,
                no_create,
                integrity,
            } => crate::backlog::evidence_ops::dispatch_dir(id, no_create, integrity),
            EvidenceOp::Audit {
                strict,
                max_bytes,
                integrity,
            } => crate::backlog::evidence_ops::dispatch_audit(strict, max_bytes, integrity),
        },
    }
}
