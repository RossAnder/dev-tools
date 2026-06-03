//! Repo links (migration 0004, R4 carve) — project↔GitHub-repo associations.
//! Every mutator in this file follows the single-mutation-path discipline (one
//! tx, one domain-table write, one `record_event`, one commit). Events are
//! routed to the owning PROJECT's `work_item` aggregate so `export.rs`'s drain
//! dispatch re-renders the project automatically (NOT a fresh `repo_link`
//! aggregate_type — the drain would silently skip it).
//!
//! The shared substrate these compose on — `parse_github_slug`,
//! `is_unique_violation`, `find_project_ancestor` — lives in `repo/shared.rs`
//! and is reached via `use super::*`; the event-outbox writer comes from
//! `super::events`.
//!
//! `pub use repo_links::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here stays reachable at its existing `crate::repo::*` path (the HTTP
//! handlers / MCP tools / importer call them by path and are unchanged). The
//! domain types named in the signatures are imported explicitly from `crate::*`
//! (a `use super::*` glob does NOT carry super's private `use` imports).

use uuid::Uuid;

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::{DbClient, Scalar};
use crate::domain::RepoLink;
use crate::error::AppError;

/// Add a new `repo_links` row attaching `slug` to `project_id` under the
/// single-mutation-path discipline. `slug` is canonicalised via
/// [`parse_github_slug`] (lowercased both segments); `is_primary` may be set on
/// create, in which case the partial UNIQUE index enforces at most one primary
/// per project (a second primary surfaces as `Validation` via
/// [`is_unique_violation`]).
///
/// `project_id`'s kind is NOT pre-checked — the kind-check trigger pair on
/// `repo_links` (migration 0004) is the authoritative guard; an attach to a
/// non-project surfaces as `Db` 500 via `RAISE(ABORT, ...)`, which matches the
/// repo's "trigger is authoritative" convention (per the file docstring).
///
/// Event `repo_link.created` on the owning project's `work_item` aggregate.
/// Returns the new repo-link id.
pub async fn add_repo_link(
    db: &impl DbClient,
    project_id: &str,
    slug: &str,
    is_primary: bool,
) -> Result<Uuid, AppError> {
    let canonical = parse_github_slug(slug)?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    let is_primary_int: i64 = if is_primary { 1 } else { 0 };

    let backend = db.backend();
    let mut tx = db.begin().await?;

    // Allocate position = MAX(position)+1 per project, inside the tx so a
    // concurrent insert under SQLite's single-writer lock is serialised.
    // COALESCE(MAX(.), -1) + 1 gives 0 for the first row.
    let position = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(position), -1) + 1 FROM repo_links WHERE project_id = $1",
        args![project_id.to_owned()],
    )
    .await?;

    match tx
        .execute(
            r#"
        INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at)
        VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)
        "#,
            args![
                id_str.clone(),
                project_id.to_owned(),
                canonical.clone(),
                position,
                is_primary_int,
            ],
        )
        .await
    {
        Ok(_) => {}
        Err(AppError::Db(ref sqlx_err)) if is_unique_violation(backend, sqlx_err) => {
            // Either the (project_id, slug) UNIQUE or the partial primary UNIQUE
            // index fired. Both are caller-fixable; surface as Validation.
            return Err(AppError::Validation(format!(
                "repo_link conflict: slug '{canonical}' is already linked, or another \
                 primary repo already exists for project '{project_id}' (primary repo conflict)"
            )));
        }
        Err(e) => return Err(e),
    }

    let payload = serde_json::json!({
        "id": id_str,
        "project_id": project_id,
        "slug": canonical,
        "is_primary": is_primary,
    });
    record_event(tx.as_mut(), "work_item", project_id, "repo_link.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`RepoLink`] aggregate
/// (canonical recipe, A8 wave). All columns are NOT NULL, so the field types are
/// `String`/`i64` (no `Option<String>` bound is needed); `is_primary` mirrors the
/// INTEGER 0/1 as `i64`. Replaces the old `query_as!` `AS "col!"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for RepoLink
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(RepoLink {
            id: row.try_get("id")?,
            project_id: row.try_get("project_id")?,
            slug: row.try_get("slug")?,
            position: row.try_get("position")?,
            is_primary: row.try_get("is_primary")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// List the `repo_links` rows for a project, ordered by `position` ASC. Returns
/// an empty Vec for a project with no links (or for a non-project id — caller is
/// expected to gate this query on `kind='project'`). Read-only; no transaction.
pub async fn list_repo_links(
    db: &impl DbClient,
    project_id: &str,
) -> Result<Vec<RepoLink>, AppError> {
    let rows = db
        .query_all::<RepoLink>(
            r#"
        SELECT
            id,
            project_id,
            slug,
            position,
            is_primary,
            created_at
        FROM repo_links
        WHERE project_id = $1
        ORDER BY position ASC
        "#,
            args![project_id.to_owned()],
        )
        .await?;

    Ok(rows)
}

/// Hard-delete a `repo_links` row. The owning project's id is read first so
/// (a) an absent id is `NotFound` BEFORE any write, and (b) the event aggregate
/// is the project's `work_item` (so the export drain re-renders the project).
///
/// `findings.repo_id`'s FK is `ON DELETE SET NULL` (migration 0004), so any
/// finding pointing at this link drops back to implicit-primary resolution
/// automatically — no separate UPDATE here.
///
/// Event `repo_link.removed` on the owning project's `work_item` aggregate.
pub async fn remove_repo_link(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    // Resolve the owning project + slug BEFORE the write so the event aggregate
    // is correct and so an absent id is `NotFound` (not `rows_affected()==0`).
    let (project_id, slug) = db
        .query_opt::<(String, String)>(
            "SELECT project_id, slug FROM repo_links WHERE id = $1",
            args![id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("repo_link '{id}' not found")))?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute("DELETE FROM repo_links WHERE id = $1", args![id.to_owned()])
        .await?;

    if affected == 0 {
        // Lost a race against a concurrent delete — caller sees NotFound.
        return Err(AppError::NotFound(format!("repo_link '{id}' not found")));
    }

    let payload = serde_json::json!({
        "id": id,
        "project_id": project_id,
        "slug": slug,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        &project_id,
        "repo_link.removed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Promote `repo_link_id` to the project's primary repo. Critical ordering:
/// inside one [`crate::db::begin_write`] tx, FIRST clear any existing primary on the same
/// project, THEN set the target to primary. SQLite checks the partial UNIQUE
/// index `idx_repo_links_one_primary` per-statement, so the clear MUST precede
/// the set or the second UPDATE fails with `SQLITE_CONSTRAINT_UNIQUE`.
///
/// The `AND project_id = ?` guard on the set defends against a cross-project
/// hijack where `repo_link_id` belongs to a different project (would otherwise
/// silently no-op and still emit an event).
///
/// Concurrent calls are serialised by SQLite's single-writer lock (last write
/// wins, both succeed); a residual unique-violation surfaces as `Validation` via
/// [`is_unique_violation`]. `NotFound` if the target id doesn't exist under the
/// given project. Event `repo_link.primary_changed` with the previous primary
/// id (or null) and the new primary id.
pub async fn set_primary_repo(
    db: &impl DbClient,
    project_id: &str,
    repo_link_id: &str,
) -> Result<(), AppError> {
    let backend = db.backend();
    let mut tx = db.begin().await?;

    // Step 1: capture the previous primary's id (for the event payload) BEFORE
    // we clear it. NULL if no current primary.
    let previous: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT id FROM repo_links WHERE project_id = $1 AND is_primary = 1",
        args![project_id.to_owned()],
    )
    .await?;

    // Step 2: clear the existing primary (idempotent if `previous` is None).
    tx.execute(
        "UPDATE repo_links SET is_primary = 0 WHERE project_id = $1 AND is_primary = 1",
        args![project_id.to_owned()],
    )
    .await?;

    // Step 3: promote the target — AND project_id guards against cross-project
    // ids. rows_affected()==0 ⇒ NotFound (id absent or wrong project).
    let affected = match tx
        .execute(
            "UPDATE repo_links SET is_primary = 1 WHERE id = $1 AND project_id = $2",
            args![repo_link_id.to_owned(), project_id.to_owned()],
        )
        .await
    {
        Ok(n) => n,
        Err(AppError::Db(ref sqlx_err)) if is_unique_violation(backend, sqlx_err) => {
            return Err(AppError::Validation(format!(
                "primary repo conflict on project '{project_id}': another row already \
                 holds is_primary=1 (concurrent set_primary_repo)"
            )));
        }
        Err(e) => return Err(e),
    };

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "repo_link '{repo_link_id}' not found under project '{project_id}'"
        )));
    }

    let payload = serde_json::json!({
        "project_id": project_id,
        "new_primary_id": repo_link_id,
        "previous_primary_id": previous,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        project_id,
        "repo_link.primary_changed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Set or clear `findings.repo_id` under the single-mutation-path discipline.
/// `repo_id=Some` binds the finding to a non-primary linked repo; `None` clears
/// the binding (the finding falls back to the project's primary repo at read
/// time).
///
/// Validation (soft, BEYOND the FK):
///   * The finding must exist (`NotFound` otherwise).
///   * When `repo_id` is `Some`, the target `repo_links` row must belong to
///     the finding's project ancestor (`Validation` otherwise). The schema FK
///     only ensures the id exists in `repo_links`; this guard rejects a
///     cross-project hijack where a finding under project A is bound to a
///     repo link of project B.
///
/// Event `finding.repo_changed` on the finding's work_item aggregate
/// (`aggregate_type = "work_item"`, `aggregate_id = <finding.work_item_id>`).
pub async fn set_finding_repo(
    pool: &impl DbClient,
    finding_id: &str,
    repo_id: Option<&str>,
) -> Result<(), AppError> {
    // Resolve the finding's owning work_item_id BEFORE opening the tx. NotFound
    // if the finding is absent.
    let work_item_id: String = pool
        .query_opt::<Scalar<Option<String>>>(
            "SELECT work_item_id FROM findings WHERE id = $1",
            args![finding_id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("finding '{finding_id}' not found")))?
        .0
        .ok_or_else(|| {
            // A finding with NULL work_item_id has no project to validate against.
            // This is a Validation, not a 500 — the importer may produce such rows
            // for orphaned findings and the caller is expected to repair them first.
            AppError::Validation(format!(
                "finding '{finding_id}' has no work_item_id; cannot bind to a repo"
            ))
        })?;

    // Project-scope check on the repo_id (if set): the target repo_link must
    // belong to the project ancestor of this finding's work-item.
    if let Some(rid) = repo_id {
        let project_id = find_project_ancestor(pool, &work_item_id).await?;
        let owns = pool
            .query_opt::<Scalar<i64>>(
                "SELECT 1 FROM repo_links WHERE id = $1 AND project_id = $2",
                args![rid.to_owned(), project_id.clone()],
            )
            .await?
            .is_some();
        if !owns {
            return Err(AppError::Validation(format!(
                "repo_link '{rid}' does not belong to the project ancestor '{project_id}' \
                 of finding '{finding_id}'"
            )));
        }
    }

    // `pool` is `&impl DbClient`, so `pool.begin()` resolves unambiguously to
    // `DbClient::begin` (returning the object-safe `Box<dyn DbTx>` this function
    // threads through) — there is no inherent `begin` on `impl DbClient`.
    let mut tx = pool.begin().await?;

    let affected = tx
        .execute(
            "UPDATE findings SET repo_id = $2 WHERE id = $1",
            args![finding_id.to_owned(), repo_id.map(|s| s.to_owned())],
        )
        .await?;

    if affected == 0 {
        // Lost a race against a concurrent delete — surface NotFound rather
        // than emitting a spurious event.
        return Err(AppError::NotFound(format!("finding '{finding_id}' not found")));
    }

    let payload = serde_json::json!({
        "finding_id": finding_id,
        "repo_id": repo_id,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "finding.repo_changed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}
