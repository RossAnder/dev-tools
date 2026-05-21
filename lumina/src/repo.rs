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

use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::domain::{ContextBlock, Finding, WorkItem, WorkItemDetail};
use crate::error::AppError;

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
    let rows = sqlx::query_as!(
        WorkItem,
        r#"
        SELECT
            id            AS "id!",
            kind          AS "kind!",
            parent_id     AS "parent_id?",
            title         AS "title!",
            body          AS "body?",
            status        AS "status!",
            position      AS "position?",
            created_at    AS "created_at!",
            updated_at    AS "updated_at!"
        FROM work_items
        WHERE (?1 IS NULL OR parent_id = ?1)
          AND (?2 IS NULL OR kind = ?2)
        ORDER BY COALESCE(position, 0), created_at, id
        "#,
        parent_id,
        kind,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Fetch one work item plus its DIRECT children, its findings, and the context
/// blocks linked through `work_item_context`. Returns `NotFound` if the id has
/// no row.
pub async fn get_work_item_detail(
    pool: &SqlitePool,
    id: &str,
) -> Result<WorkItemDetail, AppError> {
    let item = sqlx::query_as!(
        WorkItem,
        r#"
        SELECT
            id            AS "id!",
            kind          AS "kind!",
            parent_id     AS "parent_id?",
            title         AS "title!",
            body          AS "body?",
            status        AS "status!",
            position      AS "position?",
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

    let children = list_work_items(pool, Some(id), None).await?;
    let findings = list_findings(pool, id).await?;

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
    })
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
}
