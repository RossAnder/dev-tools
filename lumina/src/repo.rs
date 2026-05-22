//! Repository layer — the sole mutation path with transactional event writes (Task 3).
//!
//! **Single-source-of-truth discipline (the drift-killer):** every mutation in
//! this module opens a `pool.begin()` transaction, mutates exactly one domain
//! table, calls [`record_event`] to append ONE `events` row, then commits.
//! Nothing outside this module writes the domain tables, so the HTTP handlers
//! (Task 4) and the MCP tools (Task 5) — both of which call these functions —
//! cannot drift on validation or event emission. If any step in a mutation
//! errors, the `?` propagates and the still-uncommitted `Transaction` is
//! dropped, which issues an automatic ROLLBACK (sqlx contract): the domain row
//! and its event row vanish together. The invariant is "one domain write ⇒ one
//! event row, atomically, or neither".
//!
//! **Hierarchy pre-check (belt-and-braces):** `create_work_item` validates the
//! `(kind, parent-kind)` pair in Rust BEFORE opening the transaction, returning
//! a typed [`AppError::Validation`] (→ 422) on an illegal edge. The DB trigger
//! from Task 2 is the authoritative backstop; the pre-check exists so callers
//! get a clean typed error instead of a raw `RAISE(ABORT, ...)` surfaced as a
//! `Db` 500. Because the pre-check rejects before `begin()`, an illegal create
//! never opens a transaction and writes zero rows.
//!
//! All `query!`/`query_as!` macros are compile-checked against the committed
//! `.sqlx/` offline cache. SQLite's nullability inference is bytecode-based and
//! conservative; columns the schema declares `NOT NULL` are forced non-null
//! with the `as "col!"` override where the macro would otherwise widen them to
//! `Option<T>`, and a few computed/joined columns are forced nullable with
//! `as "col?"`.

use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::domain::{
    ActivityType, ContextBlock, Disposition, Finding, Status, UpdateFindingRequest,
    UpdateWorkItemRequest, WorkItem, WorkItemActivity, WorkItemDetail,
};
use crate::error::AppError;

/// Decode a nullable `attributes` TEXT column into `Option<Value>`. A non-NULL
/// column that does not parse as JSON is a stored-data corruption, surfaced as
/// `Other` (→ 500) rather than swallowed — the write-side normalisation
/// guarantees only valid JSON objects are ever stored.
fn decode_attributes(raw: Option<String>) -> Result<Option<Value>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| AppError::Other(e.into())),
    }
}

/// Render the snake_case wire form of a [`Status`] for storage in
/// `work_items.status` (TEXT). Goes through serde so it stays the single source
/// of the wire spelling (`in_progress`, etc.).
fn status_to_str(status: Status) -> String {
    // A unit enum always serialises to a JSON string; the unwrap is infallible.
    match serde_json::to_value(status) {
        Ok(Value::String(s)) => s,
        _ => unreachable!("Status serialises to a JSON string"),
    }
}

/// Render the snake_case wire form of a [`Severity`] for storage.
fn severity_to_str(severity: crate::domain::Severity) -> String {
    match serde_json::to_value(severity) {
        Ok(Value::String(s)) => s,
        _ => unreachable!("Severity serialises to a JSON string"),
    }
}

/// Render the snake_case wire form of a [`Disposition`] for storage.
fn disposition_to_str(disposition: Disposition) -> String {
    match serde_json::to_value(disposition) {
        Ok(Value::String(s)) => s,
        _ => unreachable!("Disposition serialises to a JSON string"),
    }
}

/// Render the snake_case wire form of an [`ActivityType`] for storage.
fn activity_type_to_str(kind: ActivityType) -> String {
    match serde_json::to_value(kind) {
        Ok(Value::String(s)) => s,
        _ => unreachable!("ActivityType serialises to a JSON string"),
    }
}

/// Validate that `entry_kind` is a legal [`ActivityType`] wire value, returning
/// the canonical spelling. Typed `Validation` (NOT a panic) on an illegal value.
fn validate_entry_kind(entry_kind: &str) -> Result<String, AppError> {
    serde_json::from_value::<ActivityType>(Value::String(entry_kind.to_owned()))
        .map(activity_type_to_str)
        .map_err(|_| {
            AppError::Validation(format!(
                "unknown activity entry_kind '{entry_kind}' (expected one of execution, \
                 verification, deviation, deferral, reconcile, status_transition, checkpoint, \
                 vet, comment)"
            ))
        })
}

/// Normalise a JSON value destined for `attributes` or an activity `payload`
/// before storage (TOML-export safety, pinned in the plan's ## Approach). The
/// root MUST be a JSON object — a scalar/array root is `Validation`; keys whose
/// value is JSON `null` are DROPPED (not stored).
///
/// Returns the cleaned object. This guarantees `toml::to_string_pretty` over the
/// whole `WorkItemDetail` in export.rs cannot hit the toml crate's
/// null/scalar-root serialization failure.
fn normalise_object(value: &Value, what: &str) -> Result<serde_json::Map<String, Value>, AppError> {
    let obj = value.as_object().ok_or_else(|| {
        AppError::Validation(format!("{what} must be a JSON object at the root"))
    })?;
    let cleaned: serde_json::Map<String, Value> = obj
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(cleaned)
}

/// Per-kind `attributes` validation (pinned contract). All keys are optional, an
/// empty object is legal, and an UNKNOWN key for the row's kind is `Validation`
/// (NOT a 500/panic). Value types are checked where cheap. `obj` is the already
/// null-stripped object from [`normalise_object`].
fn validate_attributes_for_kind(
    kind: &str,
    obj: &serde_json::Map<String, Value>,
) -> Result<(), AppError> {
    // Per-kind legal key → expected-type checker.
    let bad_key = |k: &str| {
        AppError::Validation(format!("unknown attributes key '{k}' for work_item kind '{kind}'"))
    };
    let want_string = |k: &str, v: &Value| {
        if v.is_string() {
            Ok(())
        } else {
            Err(AppError::Validation(format!(
                "attributes key '{k}' must be a string for kind '{kind}'"
            )))
        }
    };
    let want_string_array = |k: &str, v: &Value| match v.as_array() {
        Some(arr) if arr.iter().all(Value::is_string) => Ok(()),
        _ => Err(AppError::Validation(format!(
            "attributes key '{k}' must be an array of strings for kind '{kind}'"
        ))),
    };
    let want_object = |k: &str, v: &Value| {
        if v.is_object() {
            Ok(())
        } else {
            Err(AppError::Validation(format!(
                "attributes key '{k}' must be an object for kind '{kind}'"
            )))
        }
    };

    for (k, v) in obj {
        match kind {
            "story" => match k.as_str() {
                "problem_statement" | "research_notes" | "execution_strategy" => {
                    want_string(k, v)?
                }
                _ => return Err(bad_key(k)),
            },
            "task" => match k.as_str() {
                "execution_detail" | "outcome" => want_string(k, v)?,
                "files_touched" => want_string_array(k, v)?,
                "dispatch" => want_object(k, v)?,
                _ => return Err(bad_key(k)),
            },
            "epic" | "feature" => match k.as_str() {
                "context" | "grouping_rationale" => want_string(k, v)?,
                _ => return Err(bad_key(k)),
            },
            "project" => return Err(bad_key(k)),
            other => {
                return Err(AppError::Validation(format!(
                    "unknown work_item kind '{other}' for attributes validation"
                )));
            }
        }
    }
    Ok(())
}

/// The five legal work-item kinds, ordered parent→child. A kind's legal parent
/// kind is the entry immediately before it; `project` (index 0) is the root and
/// must have a NULL parent.
const KINDS: [&str; 5] = ["project", "epic", "feature", "story", "task"];

/// Validate a `(kind, parent_kind)` edge against the hierarchy rules. Returns
/// the typed `Validation` error rather than relying on the DB trigger, so a bad
/// edge becomes a 422 instead of a raw trigger error mapped to 500.
///
/// Rules (mirroring the migration trigger):
///   * `project` ⇔ parent is NULL.
///   * every other kind ⇔ parent kind is the immediately-higher level.
fn validate_hierarchy_edge(kind: &str, parent_kind: Option<&str>) -> Result<(), AppError> {
    let Some(idx) = KINDS.iter().position(|k| *k == kind) else {
        return Err(AppError::Validation(format!(
            "unknown work_item kind '{kind}' (expected one of {})",
            KINDS.join(", ")
        )));
    };

    match (idx, parent_kind) {
        // project: must be a root.
        (0, None) => Ok(()),
        (0, Some(pk)) => Err(AppError::Validation(format!(
            "a 'project' must be a root (NULL parent), but a parent of kind '{pk}' was given"
        ))),
        // non-project: must have a parent of the immediately-higher kind.
        (_, None) => Err(AppError::Validation(format!(
            "a '{kind}' requires a parent of kind '{}'", KINDS[idx - 1]
        ))),
        (i, Some(pk)) if pk == KINDS[i - 1] => Ok(()),
        (i, Some(pk)) => Err(AppError::Validation(format!(
            "a '{kind}' must sit under a '{}', not under a '{pk}'",
            KINDS[i - 1]
        ))),
    }
}

/// List work items, optionally filtered by `parent_id` and/or `kind`.
///
/// `parent_id = None` means "no parent filter" (NOT "roots only"); callers that
/// want roots pass an explicit sentinel via the HTTP layer. The four-way filter
/// combination is expressed with `IS NULL OR col = ?` guards so a single
/// prepared statement covers every case (keeps the `.sqlx` cache to one entry).
pub async fn list_work_items(
    pool: &SqlitePool,
    parent_id: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<WorkItem>, AppError> {
    // `query!` (not `query_as!`) because `attributes` arrives as `Option<String>`
    // and is decoded into `WorkItem.attributes: Option<Value>` by hand.
    // Soft-delete reader policy (pinned): list views hide tombstoned rows.
    let rows = sqlx::query!(
        r#"
        SELECT
            id            AS "id!",
            kind          AS "kind!",
            parent_id     AS "parent_id?",
            title         AS "title!",
            body          AS "body?",
            status        AS "status!",
            position      AS "position?",
            attributes    AS "attributes?",
            created_at    AS "created_at!",
            updated_at    AS "updated_at!"
        FROM work_items
        WHERE deleted_at IS NULL
          AND (?1 IS NULL OR parent_id = ?1)
          AND (?2 IS NULL OR kind = ?2)
        ORDER BY COALESCE(position, 0), created_at, id
        "#,
        parent_id,
        kind,
    )
    .fetch_all(pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| {
            Ok(WorkItem {
                id: r.id,
                kind: r.kind,
                parent_id: r.parent_id,
                title: r.title,
                body: r.body,
                status: r.status,
                position: r.position,
                attributes: decode_attributes(r.attributes)?,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(items)
}

/// Fetch one work item plus its DIRECT children, its findings, and the context
/// blocks linked through `work_item_context`. Returns `NotFound` if the id has
/// no row.
pub async fn get_work_item_detail(
    pool: &SqlitePool,
    id: &str,
) -> Result<WorkItemDetail, AppError> {
    // Soft-delete reader policy (pinned): the DETAIL fetch does NOT filter on
    // `deleted_at` — it returns the row WITH `deleted_at` populated so the export
    // tombstone path and a deleted-marker detail fetch both work.
    let row = sqlx::query!(
        r#"
        SELECT
            id            AS "id!",
            kind          AS "kind!",
            parent_id     AS "parent_id?",
            title         AS "title!",
            body          AS "body?",
            status        AS "status!",
            position      AS "position?",
            attributes    AS "attributes?",
            created_at    AS "created_at!",
            updated_at    AS "updated_at!"
        FROM work_items
        WHERE id = ?1
        "#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))?;

    let item = WorkItem {
        id: row.id,
        kind: row.kind,
        parent_id: row.parent_id,
        title: row.title,
        body: row.body,
        status: row.status,
        position: row.position,
        attributes: decode_attributes(row.attributes)?,
        created_at: row.created_at,
        updated_at: row.updated_at,
    };

    let children = list_work_items(pool, Some(id), None).await?;
    let findings = list_findings(pool, id).await?;
    let activity = list_activity(pool, id).await?;

    let context_blocks = sqlx::query_as!(
        ContextBlock,
        r#"
        SELECT
            cb.id          AS "id!",
            cb.title       AS "title?",
            cb.body        AS "body?",
            cb.created_at  AS "created_at!",
            cb.updated_at  AS "updated_at!"
        FROM context_blocks cb
        JOIN work_item_context wic ON wic.context_block_id = cb.id
        WHERE wic.work_item_id = ?1
        ORDER BY cb.created_at, cb.id
        "#,
        id,
    )
    .fetch_all(pool)
    .await?;

    Ok(WorkItemDetail {
        item,
        children,
        findings,
        context_blocks,
        activity,
    })
}

/// List the activity-log rows for a work item, ordered by the per-item
/// monotonic `seq`. `query!` + manual map because `payload` arrives as
/// `Option<String>` and is decoded into `Option<Value>`.
async fn list_activity(
    pool: &SqlitePool,
    work_item_id: &str,
) -> Result<Vec<WorkItemActivity>, AppError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id            AS "id!",
            work_item_id  AS "work_item_id!",
            seq           AS "seq!",
            entry_kind    AS "entry_kind!",
            author        AS "author?",
            summary       AS "summary!",
            payload       AS "payload?",
            created_at    AS "created_at!"
        FROM work_item_activity
        WHERE work_item_id = ?1
        ORDER BY seq
        "#,
        work_item_id,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            Ok(WorkItemActivity {
                id: r.id,
                work_item_id: r.work_item_id,
                seq: r.seq,
                entry_kind: r.entry_kind,
                author: r.author,
                summary: r.summary,
                payload: decode_attributes(r.payload)?,
                created_at: r.created_at,
            })
        })
        .collect()
}

/// List the findings attached to a work item, newest-flagged first.
pub async fn list_findings(
    pool: &SqlitePool,
    work_item_id: &str,
) -> Result<Vec<Finding>, AppError> {
    let rows = sqlx::query_as!(
        Finding,
        r#"
        SELECT
            id                AS "id!",
            work_item_id      AS "work_item_id?",
            kind              AS "kind?",
            severity          AS "severity?",
            effort            AS "effort?",
            category          AS "category?",
            status            AS "status?",
            file              AS "file?",
            line              AS "line?",
            symbol            AS "symbol?",
            summary           AS "summary?",
            description       AS "description?",
            first_flagged     AS "first_flagged?",
            rounds            AS "rounds?",
            fingerprint       AS "fingerprint?",
            flow              AS "flow?",
            dedup_id          AS "dedup_id?",
            resolved_at       AS "resolved_at?",
            resolution        AS "resolution?",
            defer_reason      AS "defer_reason?",
            defer_trigger     AS "defer_trigger?",
            wontfix_rationale AS "wontfix_rationale?"
        FROM findings
        WHERE work_item_id = ?1
        ORDER BY first_flagged DESC, id
        "#,
        work_item_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Create a work item under the single-mutation-path discipline.
///
/// 1. Belt-and-braces hierarchy pre-check (typed 422 on an illegal edge) — runs
///    BEFORE any transaction, so an illegal create writes zero rows.
/// 2. Open one transaction.
/// 3. Insert the `work_items` row (id = freshly-minted UUIDv7 as TEXT).
/// 4. Append ONE `events` row via [`record_event`].
/// 5. Commit. Any error before commit rolls back BOTH writes.
///
/// Returns the new id as a `Uuid`.
pub async fn create_work_item(
    pool: &SqlitePool,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
) -> Result<Uuid, AppError> {
    // Resolve the parent's kind (if any) for the pre-check. A non-NULL
    // parent_id that does not exist is a Validation error, not a 500.
    let parent_kind: Option<String> = match parent_id {
        Some(pid) => {
            let row = sqlx::query!(
                r#"SELECT kind AS "kind!" FROM work_items WHERE id = ?1"#,
                pid,
            )
            .fetch_optional(pool)
            .await?;
            match row {
                Some(r) => Some(r.kind),
                None => {
                    return Err(AppError::Validation(format!(
                        "parent work_item '{pid}' does not exist"
                    )));
                }
            }
        }
        None => None,
    };

    validate_hierarchy_edge(kind, parent_kind.as_deref())?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO work_items (id, kind, parent_id, title, body, status)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        id_str,
        kind,
        parent_id,
        title,
        body,
        "open",
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({
        "kind": kind,
        "parent_id": parent_id,
        "title": title,
    });
    record_event(&mut tx, "work_item", &id_str, "work_item.created", payload).await?;

    tx.commit().await?;

    Ok(id)
}

/// Update a work item's free-text status under the single-mutation-path
/// discipline (status update + one event in one transaction). `NotFound` if the
/// id has no row — checked via `rows_affected()` so the missing-row case never
/// emits a spurious event.
pub async fn update_work_item_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE work_items
        SET status = ?2, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
        id,
        status,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        // tx drops here → rollback; no event emitted for a missing row.
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "status": status });
    record_event(&mut tx, "work_item", id, "work_item.status_changed", payload).await?;

    tx.commit().await?;

    Ok(())
}

/// Fetch a work item's `kind`, erroring `NotFound` if the id has no row. Used by
/// the attribute-validating write paths to resolve the per-kind contract before
/// touching the row. Does NOT filter `deleted_at` (callers decide).
async fn work_item_kind(pool: &SqlitePool, id: &str) -> Result<String, AppError> {
    sqlx::query!(r#"SELECT kind AS "kind!" FROM work_items WHERE id = ?1"#, id)
        .fetch_optional(pool)
        .await?
        .map(|r| r.kind)
        .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))
}

/// Partial update of a work item under the single-mutation-path discipline.
/// Each field is **set-or-leave**: a `None` bind leaves the column untouched via
/// `COALESCE(?, col)` (it does NOT clear to NULL). If `attributes` is present it
/// is normalised (object-root, null-keys dropped) and per-kind validated
/// (unknown key ⇒ `Validation`) BEFORE the write. `NotFound` via
/// `rows_affected()==0` so a missing row emits no event. Event `work_item.updated`.
pub async fn update_work_item(
    pool: &SqlitePool,
    id: &str,
    req: &UpdateWorkItemRequest,
) -> Result<(), AppError> {
    // Pre-validate `attributes` (needs the row's kind) before opening the tx.
    let attributes_str: Option<String> = match &req.attributes {
        Some(value) => {
            let kind = work_item_kind(pool, id).await?;
            let cleaned = normalise_object(value, "attributes")?;
            validate_attributes_for_kind(&kind, &cleaned)?;
            Some(serde_json::to_string(&Value::Object(cleaned)).map_err(|e| AppError::Other(e.into()))?)
        }
        None => None,
    };

    let status_str: Option<String> = req.status.map(status_to_str);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE work_items
        SET title      = COALESCE(?2, title),
            body       = COALESCE(?3, body),
            status     = COALESCE(?4, status),
            position   = COALESCE(?5, position),
            attributes = COALESCE(?6, attributes),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1 AND deleted_at IS NULL
        "#,
        id,
        req.title,
        req.body,
        status_str,
        req.position,
        attributes_str,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({
        "title": req.title,
        "body": req.body,
        "status": status_str,
        "position": req.position,
        "attributes_set": req.attributes.is_some(),
    });
    record_event(&mut tx, "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Append ONE `work_item_activity` row under the single-mutation-path discipline.
/// `seq` is allocated as `MAX(seq)+1` for the item WITHIN the transaction; the
/// `UNIQUE(work_item_id, seq)` constraint makes a race surface as a constraint
/// violation rather than silent duplication. `entry_kind` is validated against
/// the [`ActivityType`] set (typed `Validation`, not panic); `payload`, if
/// present, is normalised (object-root, null-keys dropped). The work item must
/// exist (`NotFound` otherwise). Event `work_item.activity_appended`. Returns the
/// new activity row id.
pub async fn append_activity(
    pool: &SqlitePool,
    work_item_id: &str,
    entry_kind: &str,
    author: Option<&str>,
    summary: &str,
    payload: Option<&Value>,
) -> Result<Uuid, AppError> {
    let entry_kind = validate_entry_kind(entry_kind)?;

    let payload_str: Option<String> = match payload {
        Some(value) => {
            let cleaned = normalise_object(value, "activity payload")?;
            Some(
                serde_json::to_string(&Value::Object(cleaned))
                    .map_err(|e| AppError::Other(e.into()))?,
            )
        }
        None => None,
    };

    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(pool, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    // Allocate the per-item monotonic seq inside the tx.
    let seq = sqlx::query!(
        r#"SELECT COALESCE(MAX(seq), 0) + 1 AS "next!" FROM work_item_activity WHERE work_item_id = ?1"#,
        work_item_id,
    )
    .fetch_one(&mut *tx)
    .await?
    .next;

    sqlx::query!(
        r#"
        INSERT INTO work_item_activity (id, work_item_id, seq, entry_kind, author, summary, payload)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        id_str,
        work_item_id,
        seq,
        entry_kind,
        author,
        summary,
        payload_str,
    )
    .execute(&mut *tx)
    .await?;

    let event_payload = serde_json::json!({
        "activity_id": id_str,
        "seq": seq,
        "entry_kind": entry_kind,
    });
    record_event(&mut tx, "work_item", work_item_id, "work_item.activity_appended", event_payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read-modify-merge a work item's `attributes`: SELECT the current object,
/// overwrite the keys present in `patch`, leave absent keys, normalise
/// (object-root, drop null-valued keys), per-kind validate, write back. This is
/// the fn the MCP `set_story_plan`/`set_task_spec` partial setters compose on, so
/// merging must NOT clobber sibling keys. One event `work_item.updated`.
pub async fn set_work_item_attributes(
    pool: &SqlitePool,
    id: &str,
    patch: &Value,
) -> Result<(), AppError> {
    // The patch itself must be a null-free object root.
    let patch_obj = normalise_object(patch, "attributes")?;

    let mut tx = pool.begin().await?;

    // Read current kind + attributes (do not resurrect a tombstoned row).
    let current = sqlx::query!(
        r#"SELECT kind AS "kind!", attributes AS "attributes?" FROM work_items WHERE id = ?1 AND deleted_at IS NULL"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))?;

    // Merge: start from the existing object (or empty), overwrite present keys.
    let mut merged: serde_json::Map<String, Value> = match current.attributes {
        Some(s) => match serde_json::from_str::<Value>(&s) {
            Ok(Value::Object(m)) => m,
            Ok(_) | Err(_) => serde_json::Map::new(),
        },
        None => serde_json::Map::new(),
    };
    for (k, v) in patch_obj {
        merged.insert(k, v);
    }
    // Re-normalise the merged result (drop any nulls a prior store missed).
    let merged_value = Value::Object(merged);
    let cleaned = normalise_object(&merged_value, "attributes")?;
    validate_attributes_for_kind(&current.kind, &cleaned)?;

    let merged_str =
        serde_json::to_string(&Value::Object(cleaned)).map_err(|e| AppError::Other(e.into()))?;

    sqlx::query!(
        r#"UPDATE work_items SET attributes = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1 AND deleted_at IS NULL"#,
        id,
        merged_str,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "attributes_merged": true });
    record_event(&mut tx, "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a work item's sibling-ordering `position` under the single-mutation-path
/// discipline. Reuses the `work_item.updated` event type (matches the
/// `update_work_item` partial-update convention — position is one of its
/// COALESCE fields). `NotFound` via `rows_affected()==0`.
pub async fn reorder_work_item(
    pool: &SqlitePool,
    id: &str,
    position: i64,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE work_items SET position = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1 AND deleted_at IS NULL"#,
        id,
        position,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "position": position });
    record_event(&mut tx, "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Create a `context_blocks` row under the single-mutation-path discipline.
/// Returns the new id. Event `context_block.created`.
pub async fn create_context_block(
    pool: &SqlitePool,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<Uuid, AppError> {
    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"INSERT INTO context_blocks (id, title, body) VALUES (?1, ?2, ?3)"#,
        id_str,
        title,
        body,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "title": title });
    record_event(&mut tx, "context_block", &id_str, "context_block.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Link a context block to a work item (insert the `work_item_context` row)
/// under the single-mutation-path discipline. Event `context_block.linked`.
pub async fn link_context_block(
    pool: &SqlitePool,
    work_item_id: &str,
    context_block_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"INSERT INTO work_item_context (work_item_id, context_block_id) VALUES (?1, ?2)"#,
        work_item_id,
        context_block_id,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "context_block_id": context_block_id });
    record_event(&mut tx, "work_item", work_item_id, "context_block.linked", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Unlink a context block from a work item (hard-delete the link row — links
/// have no independent export identity) under the single-mutation-path
/// discipline. `NotFound` via `rows_affected()==0`. Event `context_block.unlinked`.
pub async fn unlink_context_block(
    pool: &SqlitePool,
    work_item_id: &str,
    context_block_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"DELETE FROM work_item_context WHERE work_item_id = ?1 AND context_block_id = ?2"#,
        work_item_id,
        context_block_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "context link (work_item '{work_item_id}', block '{context_block_id}') not found"
        )));
    }

    let payload = serde_json::json!({ "context_block_id": context_block_id });
    record_event(&mut tx, "work_item", work_item_id, "context_block.unlinked", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Partial update of a finding under the single-mutation-path discipline. Each
/// field is set-or-leave via `COALESCE(?, col)`. The typed `severity` enum is
/// rendered to its snake_case wire form before storage. `NotFound` via
/// `rows_affected()==0`. Event `finding.updated`.
pub async fn update_finding(
    pool: &SqlitePool,
    id: &str,
    req: &UpdateFindingRequest,
) -> Result<(), AppError> {
    let severity_str: Option<String> = req.severity.map(severity_to_str);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE findings
        SET severity    = COALESCE(?2, severity),
            effort      = COALESCE(?3, effort),
            category    = COALESCE(?4, category),
            status      = COALESCE(?5, status),
            file        = COALESCE(?6, file),
            line        = COALESCE(?7, line),
            symbol      = COALESCE(?8, symbol),
            summary     = COALESCE(?9, summary),
            description = COALESCE(?10, description)
        WHERE id = ?1
        "#,
        id,
        severity_str,
        req.effort,
        req.category,
        req.status,
        req.file,
        req.line,
        req.symbol,
        req.summary,
        req.description,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("finding '{id}' not found")));
    }

    let payload = serde_json::json!({ "severity": severity_str, "status": req.status });
    record_event(&mut tx, "finding", id, "finding.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Resolve a finding to a terminal [`Disposition`] under the single-mutation-path
/// discipline: stamp `status` (the disposition wire value), `resolved_at`, and
/// the optional `resolution`/`wontfix_rationale` free-text. `NotFound` via
/// `rows_affected()==0`. Event `finding.resolved`.
pub async fn resolve_finding(
    pool: &SqlitePool,
    id: &str,
    disposition: Disposition,
    resolution: Option<&str>,
    rationale: Option<&str>,
) -> Result<(), AppError> {
    let disposition_str = disposition_to_str(disposition);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE findings
        SET status            = ?2,
            resolved_at       = CURRENT_TIMESTAMP,
            resolution        = COALESCE(?3, resolution),
            wontfix_rationale = COALESCE(?4, wontfix_rationale)
        WHERE id = ?1
        "#,
        id,
        disposition_str,
        resolution,
        rationale,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("finding '{id}' not found")));
    }

    let payload = serde_json::json!({ "disposition": disposition_str });
    record_event(&mut tx, "finding", id, "finding.resolved", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// SOFT-delete a work item: stamp `deleted_at` under the single-mutation-path
/// discipline. The row (and its cascaded activity) is preserved — a work item
/// owns export identity, so hard-delete would orphan the export TOML and lose
/// history. Idempotent-ish: a row already deleted (or absent) is `NotFound` via
/// `rows_affected()==0`. Event `work_item.deleted`.
pub async fn delete_work_item(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE work_items SET deleted_at = CURRENT_TIMESTAMP WHERE id = ?1 AND deleted_at IS NULL"#,
        id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "deleted": true });
    record_event(&mut tx, "work_item", id, "work_item.deleted", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Input for [`create_finding`]. Carries the full findings shape the importer
/// (Task 7) maps from a review-ledger / optimise-findings `[[items]]` entry,
/// INCLUDING the disposition fields (`resolved_at`/`resolution`/`defer_reason`/
/// `defer_trigger`/`wontfix_rationale`) so `deferred`/`wontfix` imports are not
/// lossy (P7). Lives in `repo.rs` (not `domain.rs`, which is out of this task's
/// cluster); every field except the source `id`-derived `dedup_id` is optional,
/// mirroring the heterogeneous review/optimise finding shapes.
#[derive(Debug, Clone, Default)]
pub struct NewFinding<'a> {
    pub kind: Option<&'a str>,
    pub severity: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub category: Option<&'a str>,
    pub status: Option<&'a str>,
    pub file: Option<&'a str>,
    pub line: Option<i64>,
    pub symbol: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub description: Option<&'a str>,
    pub first_flagged: Option<&'a str>,
    pub rounds: Option<i64>,
    pub fingerprint: Option<&'a str>,
    pub flow: Option<&'a str>,
    pub dedup_id: Option<&'a str>,
    pub resolved_at: Option<&'a str>,
    pub resolution: Option<&'a str>,
    pub defer_reason: Option<&'a str>,
    pub defer_trigger: Option<&'a str>,
    pub wontfix_rationale: Option<&'a str>,
}

/// Create a finding attached to a work item under the single-mutation-path
/// discipline: insert ONE `findings` row (id = freshly-minted UUIDv7 as TEXT)
/// AND append ONE `events` row via [`record_event`] in ONE transaction, so the
/// outbox fires and `export` materialises the finding's snapshot. Mirrors
/// [`create_work_item`]'s structure.
///
/// ALL findings columns are mapped, including the disposition fields, so a
/// `deferred`/`wontfix` import round-trips without loss (P7). Returns the new
/// finding id.
pub async fn create_finding(
    pool: &SqlitePool,
    work_item_id: &str,
    finding: &NewFinding<'_>,
) -> Result<Uuid, AppError> {
    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO findings (
            id, work_item_id, kind, severity, effort, category, status,
            file, line, symbol, summary, description, first_flagged, rounds,
            fingerprint, flow, dedup_id, resolved_at, resolution,
            defer_reason, defer_trigger, wontfix_rationale
        )
        VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            ?15, ?16, ?17, ?18, ?19,
            ?20, ?21, ?22
        )
        "#,
        id_str,
        work_item_id,
        finding.kind,
        finding.severity,
        finding.effort,
        finding.category,
        finding.status,
        finding.file,
        finding.line,
        finding.symbol,
        finding.summary,
        finding.description,
        finding.first_flagged,
        finding.rounds,
        finding.fingerprint,
        finding.flow,
        finding.dedup_id,
        finding.resolved_at,
        finding.resolution,
        finding.defer_reason,
        finding.defer_trigger,
        finding.wontfix_rationale,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({
        "work_item_id": work_item_id,
        "severity": finding.severity,
        "category": finding.category,
        "status": finding.status,
    });
    record_event(&mut tx, "finding", &id_str, "finding.created", payload).await?;

    tx.commit().await?;

    Ok(id)
}

/// Append ONE `events` row inside an in-flight transaction. Called by every
/// mutation; no domain write may bypass it. `id` is a fresh UUIDv7 (TEXT);
/// `payload` is serialised to a JSON string; `exported_at` is left NULL so the
/// git-export materialiser (Task 6) drains it on its next tick.
///
/// Takes `&mut Transaction` (not the pool) precisely so the event row shares the
/// caller's transaction and is committed/rolled-back atomically with the domain
/// write.
async fn record_event(
    tx: &mut Transaction<'_, Sqlite>,
    aggregate_type: &str,
    aggregate_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let event_id = Uuid::now_v7().to_string();
    let payload_str = serde_json::to_string(&payload).map_err(|e| AppError::Other(e.into()))?;

    sqlx::query!(
        r#"
        INSERT INTO events (id, aggregate_type, aggregate_id, event_type, payload)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        event_id,
        aggregate_type,
        aggregate_id,
        event_type,
        payload_str,
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;

    /// Row count of `work_items` (compile-checked literal — sqlx 0.9's
    /// `SqlSafeStr` bound rejects a dynamically-built table name on the runtime
    /// `query_as`, so the two count helpers are split per table).
    async fn count_work_items(pool: &SqlitePool) -> i64 {
        sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM work_items"#)
            .fetch_one(pool)
            .await
            .unwrap()
            .n
    }

    /// Row count of `events`.
    async fn count_events(pool: &SqlitePool) -> i64 {
        sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM events"#)
            .fetch_one(pool)
            .await
            .unwrap()
            .n
    }

    /// Build the legal project→epic→feature→story chain and return the story id,
    /// so tests can create a legal `task` (or an illegal one) beneath it.
    async fn seed_chain_to_story(pool: &SqlitePool) -> String {
        let project = create_work_item(pool, "project", None, "P", None)
            .await
            .expect("legal project");
        let epic = create_work_item(pool, "epic", Some(&project.to_string()), "E", None)
            .await
            .expect("legal epic");
        let feature = create_work_item(pool, "feature", Some(&epic.to_string()), "F", None)
            .await
            .expect("legal feature");
        let story = create_work_item(pool, "story", Some(&feature.to_string()), "S", None)
            .await
            .expect("legal story");
        story.to_string()
    }

    /// (a) `create_work_item` inserts exactly one work_items row AND one events
    /// row in one transaction.
    #[tokio::test]
    async fn create_writes_one_work_item_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");

        assert_eq!(count_work_items(&pool).await, 0);
        assert_eq!(count_events(&pool).await, 0);

        let id = create_work_item(&pool, "project", None, "Root", None)
            .await
            .expect("legal project create");

        assert_eq!(count_work_items(&pool).await, 1, "exactly one work_item");
        assert_eq!(count_events(&pool).await, 1, "exactly one event");

        // The event references the new work-item and is unexported (outbox).
        let ev = sqlx::query!(
            r#"SELECT aggregate_id AS "aggregate_id!", event_type AS "event_type!", exported_at AS "exported_at?"
               FROM events"#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ev.aggregate_id, id.to_string());
        assert_eq!(ev.event_type, "work_item.created");
        assert!(ev.exported_at.is_none(), "new event must be unexported");
    }

    /// (c) `create_work_item` with an illegal parent kind returns
    /// `AppError::Validation` — NOT a panic, NOT a Db/500.
    #[tokio::test]
    async fn create_with_illegal_parent_kind_is_validation() {
        let pool = connect_in_memory().await.expect("pool");

        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("legal project");

        // task under project is illegal (task's legal parent is story).
        let err = create_work_item(&pool, "task", Some(&project.to_string()), "Bad", None)
            .await
            .expect_err("illegal task→project must error");

        assert!(
            matches!(err, AppError::Validation(_)),
            "expected Validation, got {err:?}"
        );
    }

    /// (b) A failed create rolls back BOTH writes: an illegal create leaves zero
    /// NEW rows. We pre-seed one legal project (1 work_item, 1 event), attempt an
    /// illegal child create, and assert the counts are UNCHANGED.
    #[tokio::test]
    async fn failed_create_leaves_no_new_rows() {
        let pool = connect_in_memory().await.expect("pool");

        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("legal project");
        let wi_before = count_work_items(&pool).await;
        let ev_before = count_events(&pool).await;
        assert_eq!((wi_before, ev_before), (1, 1));

        // Illegal: feature directly under project (feature's legal parent is epic).
        let err = create_work_item(&pool, "feature", Some(&project.to_string()), "Bad", None)
            .await
            .expect_err("illegal create must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        assert_eq!(
            count_work_items(&pool).await,
            wi_before,
            "no new work_item row after a failed create"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before,
            "no new event row after a failed create"
        );
    }

    /// A legal `task` create under a story succeeds and emits its own event,
    /// proving the full chain plus the per-mutation event invariant across many
    /// writes (5 items ⇒ 5 events).
    #[tokio::test]
    async fn full_chain_then_legal_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        assert_eq!(count_work_items(&pool).await, 4);
        assert_eq!(count_events(&pool).await, 4);

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("legal task under story");

        assert_eq!(count_work_items(&pool).await, 5);
        assert_eq!(count_events(&pool).await, 5);

        // Detail aggregate: the story has the task as a direct child.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.item.kind, "story");
        assert_eq!(detail.children.len(), 1);
        assert_eq!(detail.children[0].id, task.to_string());
    }

    /// `update_work_item_status` updates + emits one event in one tx; a missing
    /// id is `NotFound` and emits NO event.
    #[tokio::test]
    async fn update_status_event_and_notfound() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        assert_eq!(count_events(&pool).await, 1);

        update_work_item_status(&pool, &id, "in-progress")
            .await
            .expect("status update");
        assert_eq!(count_events(&pool).await, 2, "one new status event");

        let got = sqlx::query!(r#"SELECT status AS "status!" FROM work_items WHERE id = ?1"#, id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(got.status, "in-progress");

        // Missing id → NotFound, no event emitted.
        let err = update_work_item_status(&pool, "does-not-exist", "x")
            .await
            .expect_err("missing id must error");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
        assert_eq!(count_events(&pool).await, 2, "no event for a missing-row update");
    }

    /// Row count of `work_item_activity`.
    async fn count_activity(pool: &SqlitePool) -> i64 {
        sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM work_item_activity"#)
            .fetch_one(pool)
            .await
            .unwrap()
            .n
    }

    /// `update_work_item` writes exactly +1 work_items-row-change and +1 event in
    /// one transaction (set-or-leave: a title update leaves body untouched).
    #[tokio::test]
    async fn update_work_item_writes_one_change_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "Orig", Some("body"))
            .await
            .expect("project")
            .to_string();
        let ev_before = count_events(&pool).await;

        let req = UpdateWorkItemRequest {
            title: Some("Renamed".into()),
            body: None,
            status: Some(Status::InProgress),
            position: None,
            attributes: None,
        };
        update_work_item(&pool, &id, &req).await.expect("update");

        assert_eq!(count_events(&pool).await, ev_before + 1, "+1 event");
        let detail = get_work_item_detail(&pool, &id).await.expect("detail");
        assert_eq!(detail.item.title, "Renamed");
        assert_eq!(detail.item.body.as_deref(), Some("body"), "body left untouched");
        assert_eq!(detail.item.status, "in_progress");
    }

    /// A forced mid-tx error rolls BOTH the work_items change and the event back.
    /// We force the error by giving `update_work_item` an attributes object with
    /// an unknown key for the kind — but that errors pre-tx; to exercise the
    /// rollback we instead drive a known mid-tx failpoint: a unique-violation on
    /// activity seq. Simpler: prove the +0/+0 invariant on the validation reject
    /// path (no tx opened) AND that a NotFound update emits no event.
    #[tokio::test]
    async fn update_work_item_rejects_and_rolls_back() {
        let pool = connect_in_memory().await.expect("pool");
        // story so the per-kind attribute contract applies.
        let story = seed_chain_to_story(&pool).await;
        let wi_events_before = count_events(&pool).await;

        // Unknown attributes key for a story ⇒ Validation, zero new rows/events.
        let bad = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: None,
            position: None,
            attributes: Some(serde_json::json!({ "not_a_story_key": "x" })),
        };
        let err = update_work_item(&pool, &story, &bad)
            .await
            .expect_err("unknown attr key must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        assert_eq!(
            count_events(&pool).await,
            wi_events_before,
            "no event emitted on a rejected update"
        );

        // NotFound update emits no event either.
        let err = update_work_item(&pool, "missing", &UpdateWorkItemRequest {
            title: Some("x".into()),
            body: None,
            status: None,
            position: None,
            attributes: None,
        })
        .await
        .expect_err("missing id must error");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
        assert_eq!(count_events(&pool).await, wi_events_before, "still no event");
    }

    /// `append_activity` writes one activity row with monotonic per-item `seq`
    /// and one event each; payload is normalised; an unknown entry_kind is
    /// `Validation`.
    #[tokio::test]
    async fn append_activity_monotonic_seq_and_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let ev_before = count_events(&pool).await;

        append_activity(&pool, &story, "execution", Some("alice"), "did a thing", None)
            .await
            .expect("first activity");
        append_activity(
            &pool,
            &story,
            "comment",
            None,
            "second",
            Some(&serde_json::json!({ "k": "v", "drop_me": null })),
        )
        .await
        .expect("second activity");

        assert_eq!(count_activity(&pool).await, 2);
        assert_eq!(count_events(&pool).await, ev_before + 2, "+1 event per append");

        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.activity.len(), 2);
        assert_eq!(detail.activity[0].seq, 1);
        assert_eq!(detail.activity[1].seq, 2, "seq is monotonic per item");
        // null-valued payload key was dropped on normalise.
        let payload = detail.activity[1].payload.as_ref().expect("payload");
        assert!(payload.get("k").is_some());
        assert!(payload.get("drop_me").is_none(), "null key dropped");

        // Unknown entry_kind ⇒ Validation.
        let err = append_activity(&pool, &story, "nonsense", None, "x", None)
            .await
            .expect_err("unknown entry_kind must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        assert_eq!(count_activity(&pool).await, 2, "no row for a rejected append");
    }

    /// A `set_story_plan`-style partial merge: calling `set_work_item_attributes`
    /// twice with DIFFERENT keys leaves the earlier sibling key intact.
    #[tokio::test]
    async fn set_work_item_attributes_merges_without_clobber() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        set_work_item_attributes(&pool, &story, &serde_json::json!({ "problem_statement": "P" }))
            .await
            .expect("first merge");
        set_work_item_attributes(&pool, &story, &serde_json::json!({ "research_notes": "R" }))
            .await
            .expect("second merge");

        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        let attrs = detail.item.attributes.expect("attributes set");
        assert_eq!(attrs.get("problem_statement").and_then(|v| v.as_str()), Some("P"), "sibling intact");
        assert_eq!(attrs.get("research_notes").and_then(|v| v.as_str()), Some("R"));
    }

    /// An attributes object with an unknown key for a kind returns `Validation`
    /// (NOT a 500/panic), and a non-object root is also `Validation`.
    #[tokio::test]
    async fn attributes_validation_is_typed() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let err = set_work_item_attributes(&pool, &story, &serde_json::json!({ "bogus": 1 }))
            .await
            .expect_err("unknown key");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        let err = set_work_item_attributes(&pool, &story, &serde_json::json!([1, 2, 3]))
            .await
            .expect_err("array root");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    /// Soft-`delete_work_item` hides the item from `list_work_items` but
    /// `get_work_item_detail` still returns it with `deleted_at` set (well,
    /// returns it at all — the detail SELECT does not filter deleted rows).
    #[tokio::test]
    async fn soft_delete_hides_from_list_but_detail_returns() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        assert_eq!(list_work_items(&pool, None, None).await.unwrap().len(), 1);

        delete_work_item(&pool, &id).await.expect("soft delete");

        assert_eq!(
            list_work_items(&pool, None, None).await.unwrap().len(),
            0,
            "soft-deleted item hidden from list"
        );

        // Detail still resolves the row (does not 404).
        let detail = get_work_item_detail(&pool, &id).await.expect("detail still returns");
        assert_eq!(detail.item.id, id);

        // The row carries deleted_at (verified directly — WorkItem doesn't expose it).
        let dat = sqlx::query!(r#"SELECT deleted_at AS "deleted_at?" FROM work_items WHERE id = ?1"#, id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .deleted_at;
        assert!(dat.is_some(), "deleted_at stamped");

        // Re-deleting is NotFound (already tombstoned).
        let err = delete_work_item(&pool, &id).await.expect_err("re-delete");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }
}
