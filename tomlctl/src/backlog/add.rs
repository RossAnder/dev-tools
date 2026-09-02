//! `backlog add` — capture a discovery, or bump an already-known one.
//!
//! Identity is content-derived, so "is this already known" is answered by the
//! fingerprint rather than by the caller: the same discovery captured twice
//! folds onto one row. `--on-duplicate` chooses what folding means.
//!
//! `add_item` is the whole decision, and it takes a `&mut TomlValue` rather
//! than a path: the live write runs it inside `mutate_doc_conditional`'s
//! lock and `--dry-run` runs it against a clone, so the preview and the
//! write cannot disagree about what would happen.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde_json::{Map as JsonMap, Value as JsonValue};
use toml::Value as TomlValue;

use super::ids;
use super::schema::{
    self, ARRAY_BACKLOG, BacklogError, FIELD_AREA, FIELD_CONTEXT, FIELD_CREATED, FIELD_DEDUP_ID,
    FIELD_EVIDENCE, FIELD_FLOW, FIELD_ID, FIELD_KIND, FIELD_LAST_SEEN, FIELD_ORIGIN, FIELD_RELATED,
    FIELD_SEEN_COUNT, FIELD_STATUS, FIELD_SUMMARY, FIELD_TAGS, KIND_OTHER, STATUS_OPEN,
    TERMINAL_DATE_FIELDS,
};
use crate::cli::{
    OnDuplicate, WriteIntegrityArgs, on_missing_for, read_json_arg, warn_if_created,
    write_integrity_opts,
};
use crate::convert::{is_date_key, json_to_toml, json_type_name, toml_to_json};
use crate::errors::{ErrorKind, TaggedError, tagged_err};
use crate::integrity::IntegrityOpts;
use crate::io::{self, OnMissing, items_array, items_array_mut};
use crate::items::{MutationPlan, SkippedRow};
use crate::output::{emit_dry_run_plan, print_json_compact};

const FIELD_SCHEMA_VERSION: &str = "schema_version";
const FIELD_LAST_UPDATED: &str = "last_updated";

/// Payload fields the caller never gets to choose: they derive from the
/// content or from the clock, so a `--json` payload replayed out of a `show`
/// carries stale copies of them.
const MINTED_FIELDS: [&str; 5] = [
    FIELD_ID,
    FIELD_DEDUP_ID,
    FIELD_CREATED,
    FIELD_LAST_SEEN,
    FIELD_SEEN_COUNT,
];

/// One capture, resolved from either the field flags or a `--json` payload.
/// `kind` is already coerced and `today` already resolved, so `add_item` is a
/// pure function of this plus the document.
struct AddRequest {
    kind: String,
    summary: String,
    area: String,
    tags: Vec<String>,
    status: String,
    origin: Option<String>,
    flow: Option<String>,
    context: Option<String>,
    evidence: Vec<String>,
    related: Vec<String>,
    /// Fields a `--json` payload carried beyond the flag surface, in payload
    /// order. Disjoint from the rest by construction.
    extra: toml::Table,
    on_duplicate: OnDuplicate,
    today: toml::value::Datetime,
}

enum AddOutcome {
    Added { id: String, dedup_id: String },
    Bumped { id: String, seen_count: i64 },
    Skipped { id: String },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    summary: Option<String>,
    kind: Option<String>,
    area: Option<String>,
    tag: Vec<String>,
    evidence: Vec<String>,
    related: Vec<String>,
    context: Option<String>,
    origin: Option<String>,
    flow: Option<String>,
    on_duplicate: OnDuplicate,
    json: Option<String>,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let file = schema::backlog_path()?;
    let req = build_request(
        &file,
        summary,
        kind,
        area,
        tag,
        evidence,
        related,
        context,
        origin,
        flow,
        on_duplicate,
        json,
    )?;

    if dry_run {
        let mut doc = preview_doc(&file, &integrity)?;
        let outcome = add_item(&mut doc, &req, &file)?;
        return emit_dry_run_plan(&preview_plan(doc, &outcome));
    }

    let opts = write_integrity_opts(&integrity);
    let on_missing = on_missing_for(&file, &integrity)?;
    let mut outcome: Option<AddOutcome> = None;
    let created = io::mutate_doc_conditional(
        &file,
        integrity.allow_outside,
        opts,
        on_missing,
        |doc| {
            let result = add_item(doc, &req, &file)?;
            // `skip` must leave the file AND its sidecar untouched, which is
            // the whole reason this goes through the conditional wrapper.
            let persist = !matches!(result, AddOutcome::Skipped { .. });
            outcome = Some(result);
            Ok(persist)
        },
    )?;

    match outcome.ok_or_else(|| anyhow!("backlog add reached the write path without a decision"))? {
        AddOutcome::Added { id, dedup_id } => {
            warn_if_created(&file, created);
            print_json_compact(&serde_json::json!({
                "ok": true,
                "action": "added",
                "id": id,
                "dedup_id": dedup_id,
                "created": created,
                "path": file.display().to_string(),
            }))
        }
        AddOutcome::Bumped { id, seen_count } => print_json_compact(&serde_json::json!({
            "ok": true,
            "action": "bumped",
            "id": id,
            "seen_count": seen_count,
        })),
        AddOutcome::Skipped { id } => print_json_compact(&serde_json::json!({
            "ok": true,
            "action": "skipped",
            "id": id,
        })),
    }
}

/// Apply one capture to `doc`. Mutates nothing on the `skip` and `fail`
/// branches, so a caller that declines to persist leaves a doc identical to
/// the one it read.
fn add_item(doc: &mut TomlValue, req: &AddRequest, file: &Path) -> Result<AddOutcome> {
    let dedup_id = ids::dedup_id_from_parts(&req.kind, &req.area, &req.summary);
    let incumbent = find_by_dedup_id(doc, &dedup_id);

    if let Some(idx) = incumbent {
        let id = stored_id(doc, idx);
        match req.on_duplicate {
            OnDuplicate::Skip => return Ok(AddOutcome::Skipped { id }),
            OnDuplicate::Fail => {
                return Err(tagged_err(
                    ErrorKind::Validation,
                    Some(file.to_path_buf()),
                    format!(
                        "backlog item \"{id}\" already carries dedup_id {dedup_id}; \
                         re-run with --on-duplicate bump, skip or add"
                    ),
                ));
            }
            OnDuplicate::Bump => {
                stamp_root(doc, req)?;
                let seen_count = bump_row(doc, idx, req)?;
                return Ok(AddOutcome::Bumped { id, seen_count });
            }
            OnDuplicate::Add => {}
        }
    }

    let id = ids::derive_id(&dedup_id, &ids::existing_dedup_ids(doc));
    let row = build_row(req, &id, &dedup_id);
    schema::validate(&toml_to_json(&row)).map_err(|e| e.into_tagged(Some(file.to_path_buf())))?;
    stamp_root(doc, req)?;
    items_array_mut(doc, ARRAY_BACKLOG)?.push(row);
    if incumbent.is_none() {
        // A second row under one id is what `--on-duplicate add` was asked
        // for; every other path treats a shared id as the collision it is.
        schema::validate_ids_unique(doc).map_err(|e| e.into_tagged(Some(file.to_path_buf())))?;
    }
    Ok(AddOutcome::Added { id, dedup_id })
}

fn find_by_dedup_id(doc: &TomlValue, dedup_id: &str) -> Option<usize> {
    items_array(doc, ARRAY_BACKLOG)
        .iter()
        .position(|row| row.get(FIELD_DEDUP_ID).and_then(TomlValue::as_str) == Some(dedup_id))
}

fn stored_id(doc: &TomlValue, idx: usize) -> String {
    items_array(doc, ARRAY_BACKLOG)[idx]
        .get(FIELD_ID)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
        .to_string()
}

fn stamp_root(doc: &mut TomlValue, req: &AddRequest) -> Result<()> {
    let root = doc
        .as_table_mut()
        .context("backlog.toml root is not a table")?;
    root.entry(FIELD_SCHEMA_VERSION.to_string())
        .or_insert(TomlValue::Integer(1));
    root.insert(
        FIELD_LAST_UPDATED.to_string(),
        TomlValue::Datetime(req.today),
    );
    Ok(())
}

/// Fold a repeat sighting into an existing row: counters and the arrays move,
/// `summary` and `status` do not — a bump is evidence that the item is still
/// live, not a re-statement of what it is.
fn bump_row(doc: &mut TomlValue, idx: usize, req: &AddRequest) -> Result<i64> {
    let row = items_array_mut(doc, ARRAY_BACKLOG)?
        .get_mut(idx)
        .and_then(TomlValue::as_table_mut)
        .ok_or_else(|| anyhow!("backlog row {idx} is not a table"))?;
    let seen_count = row
        .get(FIELD_SEEN_COUNT)
        .and_then(TomlValue::as_integer)
        .unwrap_or(1)
        .saturating_add(1);
    row.insert(FIELD_SEEN_COUNT.to_string(), TomlValue::Integer(seen_count));
    row.insert(
        FIELD_LAST_SEEN.to_string(),
        TomlValue::Datetime(req.today),
    );
    union_into(row, FIELD_TAGS, &req.tags);
    union_into(row, FIELD_EVIDENCE, &req.evidence);
    union_into(row, FIELD_RELATED, &req.related);
    Ok(seen_count)
}

fn union_into(row: &mut toml::Table, field: &str, incoming: &[String]) {
    if incoming.is_empty() {
        return;
    }
    let mut merged: Vec<TomlValue> = row
        .get(field)
        .and_then(TomlValue::as_array)
        .cloned()
        .unwrap_or_default();
    for value in incoming {
        if !merged
            .iter()
            .any(|held| held.as_str() == Some(value.as_str()))
        {
            merged.push(TomlValue::String(value.clone()));
        }
    }
    row.insert(field.to_string(), TomlValue::Array(merged));
}

fn build_row(req: &AddRequest, id: &str, dedup_id: &str) -> TomlValue {
    let mut row = toml::Table::new();
    row.insert(FIELD_ID.to_string(), TomlValue::String(id.to_string()));
    row.insert(FIELD_KIND.to_string(), TomlValue::String(req.kind.clone()));
    row.insert(
        FIELD_SUMMARY.to_string(),
        TomlValue::String(req.summary.clone()),
    );
    row.insert(FIELD_AREA.to_string(), TomlValue::String(req.area.clone()));
    row.insert(FIELD_TAGS.to_string(), string_array(&req.tags));
    row.insert(
        FIELD_STATUS.to_string(),
        TomlValue::String(req.status.clone()),
    );
    row.insert(FIELD_CREATED.to_string(), TomlValue::Datetime(req.today));
    row.insert(FIELD_LAST_SEEN.to_string(), TomlValue::Datetime(req.today));
    row.insert(FIELD_SEEN_COUNT.to_string(), TomlValue::Integer(1));
    row.insert(
        FIELD_DEDUP_ID.to_string(),
        TomlValue::String(dedup_id.to_string()),
    );
    for (field, value) in [
        (FIELD_ORIGIN, &req.origin),
        (FIELD_FLOW, &req.flow),
        (FIELD_CONTEXT, &req.context),
    ] {
        if let Some(value) = value {
            row.insert(field.to_string(), TomlValue::String(value.clone()));
        }
    }
    for (field, values) in [(FIELD_EVIDENCE, &req.evidence), (FIELD_RELATED, &req.related)] {
        if !values.is_empty() {
            row.insert(field.to_string(), string_array(values));
        }
    }
    for (field, value) in &req.extra {
        row.insert(field.clone(), value.clone());
    }
    TomlValue::Table(row)
}

fn string_array(values: &[String]) -> TomlValue {
    TomlValue::Array(values.iter().cloned().map(TomlValue::String).collect())
}

#[allow(clippy::too_many_arguments)]
fn build_request(
    file: &Path,
    summary: Option<String>,
    kind: Option<String>,
    area: Option<String>,
    tag: Vec<String>,
    evidence: Vec<String>,
    related: Vec<String>,
    context: Option<String>,
    origin: Option<String>,
    flow: Option<String>,
    on_duplicate: OnDuplicate,
    json: Option<String>,
) -> Result<AddRequest> {
    let today = crate::flow::today_toml_date()?;
    let req = match json {
        Some(raw) => {
            let conflicting: Vec<&str> = [
                ("--summary", summary.is_some()),
                ("--kind", kind.is_some()),
                ("--area", area.is_some()),
                ("--tag", !tag.is_empty()),
                ("--evidence", !evidence.is_empty()),
                ("--related", !related.is_empty()),
                ("--context", context.is_some()),
                ("--origin", origin.is_some()),
                ("--flow", flow.is_some()),
            ]
            .into_iter()
            .filter_map(|(name, given)| given.then_some(name))
            .collect();
            if !conflicting.is_empty() {
                return Err(tagged_err(
                    ErrorKind::Validation,
                    Some(file.to_path_buf()),
                    format!(
                        "--json carries the whole item; drop {} or drop --json",
                        conflicting.join(", ")
                    ),
                ));
            }
            let payload: JsonValue =
                serde_json::from_str(&read_json_arg(&raw)?).context("parsing --json")?;
            request_from_payload(payload, on_duplicate, today, file)?
        }
        None => AddRequest {
            kind: schema::coerce_kind(kind.as_deref().unwrap_or(KIND_OTHER)).to_string(),
            summary: summary.unwrap_or_default(),
            area: area.unwrap_or_default(),
            tags: tag,
            status: STATUS_OPEN.to_string(),
            origin,
            flow,
            context,
            evidence,
            related,
            extra: toml::Table::new(),
            on_duplicate,
            today,
        },
    };
    if req.summary.trim().is_empty() {
        return Err(BacklogError::MissingField {
            field: FIELD_SUMMARY,
        }
        .into_tagged(Some(file.to_path_buf())));
    }
    Ok(req)
}

/// Read a whole-item payload. Content fields are the caller's; the five
/// `MINTED_FIELDS` are dropped and recomputed, so replaying a `show` output
/// re-derives identity rather than resurrecting a stale copy of it.
fn request_from_payload(
    payload: JsonValue,
    on_duplicate: OnDuplicate,
    today: toml::value::Datetime,
    file: &Path,
) -> Result<AddRequest> {
    let JsonValue::Object(map) = payload else {
        return Err(BacklogError::NotAnObject {
            got: json_type_name(&payload),
        }
        .into_tagged(Some(file.to_path_buf())));
    };
    let consumed = [
        FIELD_KIND,
        FIELD_SUMMARY,
        FIELD_AREA,
        FIELD_TAGS,
        FIELD_STATUS,
        FIELD_ORIGIN,
        FIELD_FLOW,
        FIELD_CONTEXT,
        FIELD_EVIDENCE,
        FIELD_RELATED,
    ];
    let mut extra = toml::Table::new();
    for (field, value) in &map {
        if consumed.contains(&field.as_str()) || MINTED_FIELDS.contains(&field.as_str()) {
            continue;
        }
        extra.insert(field.clone(), payload_value(field, value)?);
    }
    Ok(AddRequest {
        kind: schema::coerce_kind(string_of(&map, FIELD_KIND).unwrap_or(KIND_OTHER)).to_string(),
        summary: string_of(&map, FIELD_SUMMARY).unwrap_or_default().to_string(),
        area: string_of(&map, FIELD_AREA).unwrap_or_default().to_string(),
        tags: strings_of(&map, FIELD_TAGS),
        status: string_of(&map, FIELD_STATUS)
            .unwrap_or(STATUS_OPEN)
            .to_string(),
        origin: string_of(&map, FIELD_ORIGIN).map(str::to_string),
        flow: string_of(&map, FIELD_FLOW).map(str::to_string),
        context: string_of(&map, FIELD_CONTEXT).map(str::to_string),
        evidence: strings_of(&map, FIELD_EVIDENCE),
        related: strings_of(&map, FIELD_RELATED),
        extra,
        on_duplicate,
        today,
    })
}

/// A terminal-status date arrives from JSON as a string; stored as one it
/// would sort as text and never answer `compact --older-than`.
fn payload_value(field: &str, value: &JsonValue) -> Result<TomlValue> {
    if (TERMINAL_DATE_FIELDS.contains(&field) || is_date_key(field))
        && let JsonValue::String(text) = value
        && let Ok(date) = text.parse::<toml::value::Datetime>()
    {
        return Ok(TomlValue::Datetime(date));
    }
    json_to_toml(value)
}

fn string_of<'a>(map: &'a JsonMap<String, JsonValue>, field: &str) -> Option<&'a str> {
    map.get(field).and_then(JsonValue::as_str)
}

fn strings_of(map: &JsonMap<String, JsonValue>, field: &str) -> Vec<String> {
    map.get(field)
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The document `--dry-run` reasons over: the stored one, or the seed a live
/// run would have started from. No lock and no sidecar write, so the preview
/// leaves an absent store absent.
fn preview_doc(file: &Path, integrity: &WriteIntegrityArgs) -> Result<TomlValue> {
    let opts = IntegrityOpts {
        write_sidecar: false,
        verify_on_read: integrity.verify_integrity,
        strict: false,
    };
    match io::read_doc(file, opts, |doc| Ok(doc.clone())) {
        Ok(doc) => Ok(doc),
        Err(e) if is_not_found(&e) => match on_missing_for(file, integrity)? {
            OnMissing::Create(seed) => Ok(seed),
            OnMissing::Error => Err(e),
        },
        Err(e) => Err(e),
    }
}

fn is_not_found(err: &anyhow::Error) -> bool {
    err.downcast_ref::<TaggedError>()
        .is_some_and(|tagged| matches!(tagged.kind, ErrorKind::NotFound))
}

fn preview_plan(new_doc: TomlValue, outcome: &AddOutcome) -> MutationPlan {
    let mut plan = MutationPlan {
        new_doc,
        added: Vec::new(),
        updated: Vec::new(),
        removed: Vec::new(),
        skipped: Vec::new(),
    };
    match outcome {
        AddOutcome::Added { id, .. } => plan.added.push(id.clone()),
        AddOutcome::Bumped { id, .. } => plan.updated.push(id.clone()),
        AddOutcome::Skipped { id } => plan.skipped.push(SkippedRow {
            row: 0,
            matched_id: id.clone(),
        }),
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::panic::AssertUnwindSafe;
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

    /// Resolve the store under a throwaway root. The override is dropped even
    /// when an assertion panics, so a failure cannot leak `TOMLCTL_ROOT` into
    /// every later test in the process.
    fn in_sandbox<T>(f: impl FnOnce(&Path) -> T) -> T {
        let _guard = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // SAFETY: set_var is unsafe in edition 2024; acceptable inside tests
        // where we hold the env lock.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", root.as_os_str());
        }
        fs::create_dir_all(root.join(".claude")).unwrap();
        let out = std::panic::catch_unwind(AssertUnwindSafe(|| f(&root)));
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
        match out {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    struct Capture<'a> {
        summary: Option<&'a str>,
        kind: Option<&'a str>,
        area: Option<&'a str>,
        tags: &'a [&'a str],
        evidence: &'a [&'a str],
        on_duplicate: OnDuplicate,
        json: Option<&'a str>,
        dry_run: bool,
    }

    impl Default for Capture<'_> {
        fn default() -> Self {
            Self {
                summary: None,
                kind: None,
                area: None,
                tags: &[],
                evidence: &[],
                on_duplicate: OnDuplicate::Bump,
                json: None,
                dry_run: false,
            }
        }
    }

    fn run(c: Capture<'_>) -> Result<()> {
        let owned = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        dispatch(
            c.summary.map(str::to_string),
            c.kind.map(str::to_string),
            c.area.map(str::to_string),
            owned(c.tags),
            owned(c.evidence),
            Vec::new(),
            None,
            None,
            None,
            c.on_duplicate,
            c.json.map(str::to_string),
            c.dry_run,
            wargs(),
        )
    }

    fn capture(summary: &str) -> Capture<'_> {
        Capture {
            summary: Some(summary),
            kind: Some("flaky-test"),
            area: Some("lumina/server/tests/pty_readiness_probe.rs"),
            ..Capture::default()
        }
    }

    fn store_path(root: &Path) -> PathBuf {
        root.join(".claude").join("backlog.toml")
    }

    fn store(root: &Path) -> TomlValue {
        toml::from_str(&fs::read_to_string(store_path(root)).unwrap()).unwrap()
    }

    fn rows(root: &Path) -> Vec<TomlValue> {
        items_array(&store(root), ARRAY_BACKLOG).to_vec()
    }

    fn modified(file: &Path) -> std::time::SystemTime {
        fs::metadata(file).unwrap().modified().unwrap()
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    #[test]
    fn a_fresh_mint_writes_every_required_field() {
        in_sandbox(|root| {
            run(capture("PTY readiness probe flakes on slow CI")).unwrap();
            let doc = store(root);
            assert_eq!(doc.get(FIELD_SCHEMA_VERSION), Some(&TomlValue::Integer(1)));
            assert!(doc.get(FIELD_LAST_UPDATED).unwrap().as_datetime().is_some());

            let rows = items_array(&doc, ARRAY_BACKLOG);
            assert_eq!(rows.len(), 1);
            let row = &rows[0];
            let id = row[FIELD_ID].as_str().unwrap();
            let dedup_id = row[FIELD_DEDUP_ID].as_str().unwrap();
            assert_eq!(id, format!("B-{}", &dedup_id[..8]));
            assert_eq!(dedup_id.len(), 16);
            assert!(id.len() == 10 && id[2..].chars().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(row[FIELD_STATUS].as_str(), Some(STATUS_OPEN));
            assert_eq!(row[FIELD_KIND].as_str(), Some("flaky-test"));
            assert_eq!(row[FIELD_SEEN_COUNT].as_integer(), Some(1));
            assert!(row[FIELD_CREATED].as_datetime().is_some());
            assert!(row[FIELD_LAST_SEEN].as_datetime().is_some());
            assert_eq!(row[FIELD_TAGS].as_array().map(Vec::len), Some(0));
            // `validate` is what stands between a half-built row and the
            // store, so the written row must satisfy it.
            assert_eq!(schema::validate(&toml_to_json(&rows[0])), Ok(()));
        });
    }

    #[test]
    fn a_punctuation_variant_bumps_the_incumbent() {
        in_sandbox(|root| {
            run(Capture {
                tags: &["ci"],
                ..capture("PTY readiness probe flakes on slow CI")
            })
            .unwrap();
            run(Capture {
                tags: &["windows", "ci"],
                ..capture("  the PTY readiness-probe FLAKES, on slow CI!! ")
            })
            .unwrap();

            let rows = rows(root);
            assert_eq!(rows.len(), 1, "a punctuation variant must not mint");
            let row = rows[0].as_table().unwrap();
            assert_eq!(row[FIELD_SEEN_COUNT].as_integer(), Some(2));
            assert_eq!(
                row[FIELD_SUMMARY].as_str(),
                Some("PTY readiness probe flakes on slow CI"),
                "a bump must not restate the summary"
            );
            let tags: Vec<&str> = row[FIELD_TAGS]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(TomlValue::as_str)
                .collect();
            assert_eq!(tags, ["ci", "windows"]);
        });
    }

    #[test]
    fn a_bump_unions_evidence_without_duplicating_it() {
        in_sandbox(|root| {
            let summary = "guard_write_path refuses a symlinked leaf";
            run(Capture {
                evidence: &["tomlctl/src/io.rs:88"],
                ..capture(summary)
            })
            .unwrap();
            run(Capture {
                evidence: &["tomlctl/src/io.rs:88", "repro.log"],
                ..capture(summary)
            })
            .unwrap();
            let rows = rows(root);
            let evidence: Vec<&str> = rows[0][FIELD_EVIDENCE]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(TomlValue::as_str)
                .collect();
            assert_eq!(evidence, ["tomlctl/src/io.rs:88", "repro.log"]);
        });
    }

    #[test]
    fn on_duplicate_fail_names_the_incumbent() {
        in_sandbox(|root| {
            let summary = "sidecar rename loses to the indexer";
            run(capture(summary)).unwrap();
            let id = rows(root)[0][FIELD_ID].as_str().unwrap().to_string();
            let err = run(Capture {
                on_duplicate: OnDuplicate::Fail,
                ..capture(summary)
            })
            .unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(format!("{err:#}").contains(&id), "{err:#}");
            assert_eq!(rows(root).len(), 1);
        });
    }

    #[test]
    fn on_duplicate_skip_leaves_the_store_and_sidecar_untouched() {
        in_sandbox(|root| {
            let summary = "compact drops the terminal reason";
            run(capture(summary)).unwrap();
            let file = store_path(root);
            let sidecar = crate::integrity::sidecar_path(&file);
            let before = fs::read(&file).unwrap();
            let before_sidecar = fs::read(&sidecar).unwrap();
            let before_mtime = modified(&file);

            run(Capture {
                on_duplicate: OnDuplicate::Skip,
                tags: &["ignored"],
                ..capture(summary)
            })
            .unwrap();

            assert_eq!(fs::read(&file).unwrap(), before);
            assert_eq!(fs::read(&sidecar).unwrap(), before_sidecar);
            // Re-serialising an unchanged doc yields the same bytes, so only
            // the mtime separates "wrote nothing" from "wrote the same thing".
            assert_eq!(modified(&file), before_mtime);
        });
    }

    #[test]
    fn on_duplicate_add_appends_a_second_row_under_one_id() {
        in_sandbox(|root| {
            let summary = "two captures, one fingerprint";
            run(capture(summary)).unwrap();
            run(Capture {
                on_duplicate: OnDuplicate::Add,
                ..capture(summary)
            })
            .unwrap();
            let rows = rows(root);
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0][FIELD_ID], rows[1][FIELD_ID]);
        });
    }

    #[test]
    fn dry_run_leaves_an_absent_store_absent() {
        in_sandbox(|root| {
            run(Capture {
                dry_run: true,
                ..capture("nothing should land")
            })
            .unwrap();
            let file = store_path(root);
            assert!(!file.exists(), "{}", file.display());
            assert!(!crate::integrity::sidecar_path(&file).exists());
        });
    }

    #[test]
    fn dry_run_leaves_an_existing_store_byte_identical() {
        in_sandbox(|root| {
            run(capture("first capture")).unwrap();
            let file = store_path(root);
            let sidecar = crate::integrity::sidecar_path(&file);
            let before = fs::read(&file).unwrap();
            let before_sidecar = fs::read(&sidecar).unwrap();

            run(Capture {
                dry_run: true,
                ..capture("second capture")
            })
            .unwrap();
            run(Capture {
                dry_run: true,
                ..capture("first capture")
            })
            .unwrap();

            assert_eq!(fs::read(&file).unwrap(), before);
            assert_eq!(fs::read(&sidecar).unwrap(), before_sidecar);
        });
    }

    #[test]
    fn a_json_payload_mints_and_re_derives_identity() {
        in_sandbox(|root| {
            run(Capture {
                json: Some(
                    r#"{"id":"B-deadbeef","dedup_id":"0000000000000000","seen_count":97,
                        "kind":"bug","summary":"json payload mints","area":"tomlctl/src",
                        "tags":["cli"],"context":"pass --json - to stream it",
                        "duplicate_of":"B-7f0e2d91"}"#,
                ),
                ..Capture::default()
            })
            .unwrap();
            let rows = rows(root);
            let row = &rows[0];
            assert_ne!(row[FIELD_ID].as_str(), Some("B-deadbeef"));
            assert_ne!(row[FIELD_DEDUP_ID].as_str(), Some("0000000000000000"));
            assert_eq!(row[FIELD_SEEN_COUNT].as_integer(), Some(1));
            assert_eq!(row[FIELD_KIND].as_str(), Some("bug"));
            assert_eq!(row[FIELD_STATUS].as_str(), Some(STATUS_OPEN));
            assert_eq!(row[FIELD_CONTEXT].as_str(), Some("pass --json - to stream it"));
            assert_eq!(row["duplicate_of"].as_str(), Some("B-7f0e2d91"));
            assert_eq!(schema::validate(&toml_to_json(&rows[0])), Ok(()));
        });
    }

    #[test]
    fn a_json_payload_carrying_a_terminal_status_keeps_its_date_typed() {
        in_sandbox(|root| {
            run(Capture {
                json: Some(
                    r#"{"summary":"already resolved elsewhere","kind":"bug",
                        "status":"resolved","resolved":"2026-09-01",
                        "resolution":"fixed in abc123"}"#,
                ),
                ..Capture::default()
            })
            .unwrap();
            let rows = rows(root);
            assert!(rows[0]["resolved"].as_datetime().is_some());
            assert_eq!(schema::validate(&toml_to_json(&rows[0])), Ok(()));
        });
    }

    #[test]
    fn json_and_the_field_flags_are_mutually_exclusive() {
        in_sandbox(|_| {
            let err = run(Capture {
                summary: Some("both at once"),
                json: Some(r#"{"summary":"payload"}"#),
                ..Capture::default()
            })
            .unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(format!("{err:#}").contains("--summary"), "{err:#}");
        });
    }

    #[test]
    fn an_empty_or_blank_summary_is_rejected() {
        in_sandbox(|root| {
            // Whitespace-only is the case `schema::validate` alone lets
            // through: it reads `"   "` as present, so the guard has to run
            // before the row is built.
            for payload in [
                r#"{"kind":"bug","area":"tomlctl/src"}"#,
                r#"{"kind":"bug","summary":"   "}"#,
            ] {
                let err = run(Capture {
                    json: Some(payload),
                    ..Capture::default()
                })
                .unwrap_err();
                assert_eq!(kind_of(&err), "validation", "{err:#}");
            }
            assert!(!store_path(root).exists());
        });
    }

    #[test]
    fn an_unknown_kind_coerces_to_other() {
        in_sandbox(|root| {
            run(Capture {
                kind: Some("regression"),
                ..capture("kind falls back rather than failing")
            })
            .unwrap();
            assert_eq!(rows(root)[0][FIELD_KIND].as_str(), Some(KIND_OTHER));
        });
    }

    #[test]
    fn a_missing_kind_is_other_and_a_missing_area_is_empty() {
        in_sandbox(|root| {
            run(Capture {
                summary: Some("bare capture"),
                ..Capture::default()
            })
            .unwrap();
            let rows = rows(root);
            assert_eq!(rows[0][FIELD_KIND].as_str(), Some(KIND_OTHER));
            assert_eq!(rows[0][FIELD_AREA].as_str(), Some(""));
        });
    }

    #[test]
    fn a_hand_written_store_without_root_scalars_still_round_trips() {
        in_sandbox(|root| {
            let file = store_path(root);
            fs::write(
                &file,
                "[[backlog]]\nid = \"B-00000000\"\ndedup_id = \"0000000000000000\"\n\
                 summary = \"hand written\"\nstatus = \"open\"\n",
            )
            .unwrap();
            run(capture("appended beside a hand-written row")).unwrap();
            let doc = store(root);
            assert_eq!(doc.get(FIELD_SCHEMA_VERSION), Some(&TomlValue::Integer(1)));
            assert!(doc.get(FIELD_LAST_UPDATED).unwrap().as_datetime().is_some());
            assert_eq!(items_array(&doc, ARRAY_BACKLOG).len(), 2);
        });
    }

    #[test]
    fn two_distinct_captures_get_distinct_ids() {
        in_sandbox(|root| {
            run(capture("first distinct discovery")).unwrap();
            run(capture("second distinct discovery")).unwrap();
            let rows = rows(root);
            assert_eq!(rows.len(), 2);
            assert_ne!(rows[0][FIELD_ID], rows[1][FIELD_ID]);
            assert_eq!(schema::validate_ids_unique(&store(root)), Ok(()));
        });
    }
}
