//! First-class task touched-file set (migration 0020): the read/write functions
//! over the `task_files` child table that promotes the former
//! `attributes.files_touched` JSON array (migration 0004) to an indexable,
//! de-duplicated set. One row per `(task × kind × repo × path)`; `kind`
//! discriminates the PLAN-time set (`'expected'`) from the EXECUTION-time set
//! (`'actual'`).
//!
//! **Canonical `(repo_link_id, path)` key (Ground R2).** A bare path (`None`
//! repo_link_id) and an explicit `{repo, path}` slug that resolves to the
//! project's PRIMARY linked repo MUST map to the SAME canonical key — otherwise
//! the same primary-repo file would false-distinguish across the two spellings
//! (and a later dedup/overlap scan would miss the overlap). The storage UNIQUE
//! index keys on `COALESCE(repo_link_id,'')`, i.e. NULL ≡ primary; this module's
//! [`canonical_file_key`] mirrors that by collapsing an explicit primary slug to
//! `None` (the primary repo's id is the same `''` bucket as NULL). A non-primary
//! slug keeps its specific `repo_link_id`. Slug → repo-link resolution reuses the
//! same path `set_task_spec` uses today ([`find_project_ancestor`] +
//! [`list_repo_links`]).
//!
//! **Single-mutation-path discipline / the T2↔T6 seam (Option A).** Like every
//! other repo mutator (and `record_task_commits` specifically), each writer here
//! opens its OWN `db.begin()` (`BEGIN IMMEDIATE`) transaction and records EXACTLY
//! ONE coarse, export-INERT `task_files` event itself (`aggregate_type =
//! "task_files"`; R-B4 — never `"work_item"`, so the git-export drain ignores it).
//! The T6 MCP tools / HTTP mirrors just wrap these; they do NOT own the tx or the
//! event. The inert-aggregate vocabulary in `repo/events.rs` is widened to admit
//! `task_files` for this purpose.
//!
//! `pub use task_files::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here is reachable at its existing `crate::repo::*` path. The shared
//! substrate (`find_project_ancestor`, `list_repo_links`, `parse_github_slug`,
//! `record_inert_event`) is reached via `use super::*` / `use super::events`.

use uuid::Uuid;

use super::*;
use super::events::record_inert_event;
use crate::args;
use crate::db::DbClient;
use crate::domain::TaskFile;
use crate::error::AppError;
use serde_json::Value;

/// Per-field byte cap on a stored `path` (mirrors `worktrees::MAX_FREE_TEXT_BYTES`
/// — record-only, so this is an unbounded-growth guard, not traversal defence).
/// An over-cap path is a clean [`AppError::Validation`], never a silently-stored
/// blob.
const MAX_PATH_BYTES: usize = 4096;

/// The canonical `(repo_link_id, path)` key for a touched file (Ground R2). The
/// `repo_link_id` is `None` for the project's PRIMARY linked repo (the storage
/// `COALESCE(repo_link_id,'')='')` bucket — so a bare path and an explicit
/// primary-repo slug both canonicalise to `None` and never false-distinguish;
/// `Some(id)` qualifies the file to a specific NON-primary linked repo.
pub type FileKey = (Option<String>, String);

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`TaskFile`] aggregate
/// (canonical recipe, mirroring `RepoLink`). The NOT NULL columns map to
/// `String`; the nullable `repo_link_id` maps to `Option<String>`.
impl<'r, R> sqlx::FromRow<'r, R> for TaskFile
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(TaskFile {
            id: row.try_get("id")?,
            task_id: row.try_get("task_id")?,
            repo_link_id: row.try_get("repo_link_id")?,
            path: row.try_get("path")?,
            kind: row.try_get("kind")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Resolve a single best-effort `files_touched` JSON entry to its CANONICAL
/// `(repo_link_id, path)` key, against the task's project ancestor's linked
/// repos (Ground R2). The `links` slice is the task project ancestor's
/// `repo_links` (pre-fetched once by the caller via [`list_repo_links`] so the
/// per-entry resolution is in-memory, not N queries).
///
/// Entry shapes (the same two `set_task_spec` accepts):
///   * bare string `"src/foo.rs"` → `(None, "src/foo.rs")` (the primary-repo
///     bucket — NULL `repo_link_id`).
///   * object `{"repo": "<owner>/<name>", "path": "src/foo.rs"}` → the slug is
///     canonicalised ([`parse_github_slug`]) and matched against `links`:
///       - matches the PRIMARY link ⇒ `(None, path)` — collapsed to the NULL
///         bucket so it shares one canonical key with the bare form (R2);
///       - matches a NON-primary link ⇒ `(Some(link_id), path)`;
///       - matches no linked slug ⇒ [`AppError::Validation`] (the same reject
///         `set_task_spec` raises for an unknown slug).
///
/// A malformed entry (neither a string nor a `{repo, path}` object) is a
/// [`AppError::Validation`] — the writers want a clean typed reject, not a
/// silent drop, since these feed a durable set.
pub fn canonical_file_key(
    entry: &Value,
    links: &[crate::domain::RepoLink],
) -> Result<FileKey, AppError> {
    if let Some(path) = entry.as_str() {
        return Ok((None, path.to_owned()));
    }
    if let Some(obj) = entry.as_object() {
        let repo = obj.get("repo").and_then(Value::as_str);
        let path = obj.get("path").and_then(Value::as_str);
        if let (Some(repo), Some(path)) = (repo, path) {
            // Canonicalise the slug (lowercases both segments) so a mixed-case
            // caller form still matches the stored canonical slug.
            let canonical = parse_github_slug(repo)?;
            let link = links.iter().find(|l| l.slug == canonical).ok_or_else(|| {
                AppError::Validation(format!(
                    "files_touched entry references repo slug '{canonical}' which is not a \
                     linked repo on the task's project ancestor (linked slugs: [{}])",
                    links.iter().map(|l| l.slug.as_str()).collect::<Vec<_>>().join(", ")
                ))
            })?;
            // R2: collapse an explicit PRIMARY-repo slug to the NULL bucket so a
            // bare path and an explicit-primary `{repo, path}` share one canonical
            // key (matching the storage `COALESCE(repo_link_id,'')` semantics).
            let repo_link_id = if link.is_primary == 1 {
                None
            } else {
                Some(link.id.clone())
            };
            return Ok((repo_link_id, path.to_owned()));
        }
    }
    Err(AppError::Validation(
        "files_touched entry must be a path string or a {repo, path} object".to_owned(),
    ))
}

/// Resolve a slice of best-effort `files_touched` JSON entries to their
/// canonical `(repo_link_id, path)` keys, DE-DUPLICATED (preserving
/// first-seen order). Fetches the task's project ancestor's `repo_links` ONCE
/// (only when any entry is the `{repo, path}` form — a slice of all bare paths
/// needs no repo-link lookup, preserving the zero-query legacy fast path), then
/// canonicalises each entry via [`canonical_file_key`]. Any unknown/malformed
/// entry propagates as [`AppError::Validation`]. Shared by both writers (so they
/// dedup identically) and reusable by `team_execution`'s advisory scan.
async fn resolve_file_keys(
    db: &impl DbClient,
    task_id: &str,
    entries: &[Value],
) -> Result<Vec<FileKey>, AppError> {
    // Fast path: when every entry is a bare path, no repo-link lookup is needed
    // (mirrors `set_task_spec`'s zero-query legacy path).
    let has_qualified = entries.iter().any(|e| e.is_object());
    let links: Vec<crate::domain::RepoLink> = if has_qualified {
        let project_id = find_project_ancestor(db, task_id).await?;
        list_repo_links(db, &project_id).await?
    } else {
        Vec::new()
    };

    let mut out: Vec<FileKey> = Vec::with_capacity(entries.len());
    for entry in entries {
        let key = canonical_file_key(entry, &links)?;
        // Validate the path byte-bound + dedup on the canonical key.
        check_path(&key.1)?;
        if !out.contains(&key) {
            out.push(key);
        }
    }
    Ok(out)
}

/// Reject a `path` whose UTF-8 byte length exceeds [`MAX_PATH_BYTES`] (R20-style
/// unbounded-growth guard). The byte length (not char count) is the bound,
/// matching how SQLite measures TEXT storage.
fn check_path(path: &str) -> Result<(), AppError> {
    if path.len() > MAX_PATH_BYTES {
        return Err(AppError::Validation(format!(
            "task_files path exceeds the {MAX_PATH_BYTES}-byte limit ({} bytes)",
            path.len()
        )));
    }
    Ok(())
}

/// REPLACE a task's EXPECTED touched-file set (migration 0020, plan-time set).
/// `set_task_spec` re-means its `files_touched` to the EXPECTED set and wants to
/// set the WHOLE set for a task, so this is a REPLACE (not append): inside ONE
/// `BEGIN IMMEDIATE` tx it DELETEs every existing `kind='expected'` row for the
/// task, then INSERTs the canonicalised, de-duplicated new set, and records
/// EXACTLY ONE coarse export-inert `task_files.expected_set` event
/// (`aggregate_type="task_files"`; R-B4). An EMPTY `entries` is legal — it
/// clears the task's expected set (the DELETE runs, zero rows are re-inserted),
/// and the single event still records (the set was meaningfully reset to empty).
///
/// `entries` are the best-effort `files_touched` JSON entries (bare path strings
/// or `{repo, path}` objects); each is canonicalised to its `(repo_link_id,
/// path)` key via [`canonical_file_key`] (an explicit primary-repo slug collapses
/// to the NULL bucket — R2). An unknown slug or a malformed entry is a clean
/// [`AppError::Validation`] BEFORE any write (the canonicalisation runs before the
/// tx opens). Returns the count of rows actually inserted.
pub async fn set_task_expected_files(
    db: &impl DbClient,
    task_id: &str,
    entries: &[Value],
) -> Result<usize, AppError> {
    // Canonicalise + dedup BEFORE the tx so an unknown slug / malformed entry is
    // a typed reject that writes nothing (mirrors `set_task_spec`'s pre-write
    // validation order). Resolution reads on autocommit `db`; the writes below
    // re-open the tx.
    let keys = resolve_file_keys(db, task_id, entries).await?;

    let mut tx = db.begin().await?;

    // REPLACE semantics: drop the prior expected set first (idempotent re-set —
    // setting the same set twice converges to the same rows).
    tx.execute(
        "DELETE FROM task_files WHERE task_id = $1 AND kind = 'expected'",
        args![task_id.to_owned()],
    )
    .await?;

    let mut inserted = 0usize;
    for (repo_link_id, path) in &keys {
        let row_id = Uuid::now_v7().to_string();
        // The whole set was just cleared, so a plain INSERT cannot conflict on
        // the idx_task_files_unique index (keys are deduped above). No
        // ON CONFLICT clause needed for the replace path.
        tx.execute(
            r#"
            INSERT INTO task_files (id, task_id, repo_link_id, path, kind)
            VALUES ($1, $2, $3, $4, 'expected')
            "#,
            args![
                row_id,
                task_id.to_owned(),
                repo_link_id.clone(),
                path.clone()
            ],
        )
        .await?;
        inserted += 1;
    }

    let payload = serde_json::json!({
        "task_id": task_id,
        "kind": "expected",
        "count": inserted,
    });
    record_inert_event(
        tx.as_mut(),
        "task_files",
        task_id,
        "task_files.expected_set",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(inserted)
}

/// APPEND to a task's ACTUAL touched-file set (migration 0020, execution-time
/// set) — APPEND-ONLY by design. The execution-time set accumulates across
/// re-runs: a file touched-then-reverted within a task is tolerated as a benign
/// over-report (the actual set is a superset hint, not an exact diff), and a
/// re-run with overlapping files is idempotent. Inside ONE `BEGIN IMMEDIATE` tx
/// it INSERTs each canonicalised, de-duplicated entry with
/// `ON CONFLICT(idx_task_files_unique) DO NOTHING` so an already-recorded
/// `(task, 'actual', repo, path)` collapses rather than erroring, and records
/// EXACTLY ONE coarse export-inert `task_files.actual_appended` event. An EMPTY
/// `entries` is a clean [`AppError::Validation`] BEFORE the tx (no event for a
/// zero-row append — mirroring `record_task_commits`' empty-batch reject), so the
/// append is never a silent no-op-with-event.
///
/// `entries` are canonicalised exactly as [`set_task_expected_files`] does (an
/// explicit primary-repo slug collapses to the NULL bucket — R2; an unknown slug
/// / malformed entry is a clean [`AppError::Validation`] before any write).
/// Returns the count of rows ACTUALLY inserted (an already-present pair does NOT
/// count — the append is idempotent).
pub async fn add_task_actual_files(
    db: &impl DbClient,
    task_id: &str,
    entries: &[Value],
) -> Result<usize, AppError> {
    // An empty append is a no-op — reject it BEFORE opening the tx so no
    // export-inert event is recorded for a zero-row batch (the
    // `record_task_commits` precedent).
    if entries.is_empty() {
        return Err(AppError::Validation(
            "add_task_actual_files requires at least one file entry".to_owned(),
        ));
    }

    // Canonicalise + dedup BEFORE the tx (typed reject on an unknown slug /
    // malformed entry writes nothing).
    let keys = resolve_file_keys(db, task_id, entries).await?;

    let mut tx = db.begin().await?;

    let mut inserted = 0usize;
    for (repo_link_id, path) in &keys {
        let row_id = Uuid::now_v7().to_string();
        // Append-only: ON CONFLICT DO NOTHING collapses a re-recorded pair on the
        // idx_task_files_unique expression index. `affected == 0` ⇒ a dedup skip
        // (the pair was already recorded), NOT an error — only genuinely-new
        // rows count.
        let affected = tx
            .execute(
                r#"
                INSERT INTO task_files (id, task_id, repo_link_id, path, kind)
                VALUES ($1, $2, $3, $4, 'actual')
                ON CONFLICT (task_id, kind, COALESCE(repo_link_id, ''), path) DO NOTHING
                "#,
                args![
                    row_id,
                    task_id.to_owned(),
                    repo_link_id.clone(),
                    path.clone()
                ],
            )
            .await?;
        if affected == 1 {
            inserted += 1;
        }
    }

    let payload = serde_json::json!({
        "task_id": task_id,
        "kind": "actual",
        "inserted": inserted,
        "requested": keys.len(),
    });
    record_inert_event(
        tx.as_mut(),
        "task_files",
        task_id,
        "task_files.actual_appended",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(inserted)
}

/// List a task's [`TaskFile`] rows (migration 0020), optionally narrowed to one
/// `kind`. `kind=None` returns both the expected and actual sets; `kind=Some`
/// filters to that set. Ordered by `kind` then `path` for deterministic output
/// (the `idx_task_files_task(task_id, kind)` index supports the scan).
/// Read-only, no transaction.
pub async fn list_task_files(
    db: &impl DbClient,
    task_id: &str,
    kind: Option<&str>,
) -> Result<Vec<TaskFile>, AppError> {
    match kind {
        Some(k) => {
            db.query_all::<TaskFile>(
                r#"
                SELECT id, task_id, repo_link_id, path, kind, created_at
                FROM task_files
                WHERE task_id = $1 AND kind = $2
                ORDER BY path
                "#,
                args![task_id.to_owned(), k.to_owned()],
            )
            .await
        }
        None => {
            db.query_all::<TaskFile>(
                r#"
                SELECT id, task_id, repo_link_id, path, kind, created_at
                FROM task_files
                WHERE task_id = $1
                ORDER BY kind, path
                "#,
                args![task_id.to_owned()],
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::repo::test_support::seed_chain_to_story;
    use pretty_assertions::assert_eq;
    use sqlx::SqlitePool;

    /// The primary repo slug used across these tests. `parse_github_slug`
    /// lowercases both segments, so this canonical form round-trips unchanged.
    const PRIMARY_SLUG: &str = "octocat/hello-world";
    /// A SECOND, NON-primary linked repo slug (for the distinct-key assertion).
    const SECONDARY_SLUG: &str = "octocat/other-repo";

    /// Seed the legal project→epic→focus→story chain, attach a PRIMARY repo link
    /// (and a second NON-primary link) to the project, and create one task under
    /// the story. Returns `(pool, task_id)`. The task's project ancestor carries
    /// both linked repos, so `canonical_file_key`'s slug resolution (and the
    /// non-primary distinctness) is exercisable. `SqlitePool` implements
    /// `DbClient`, so it doubles as the `&impl DbClient` the writers take AND the
    /// raw handle for direct sqlx assertions (the established repo-test idiom).
    async fn seed_project_link_task() -> (SqlitePool, String) {
        let pool = connect_in_memory().await.expect("migrated in-memory pool");

        // The seed chain returns the story id; walk up to the project to attach
        // the repo links (repo_links hang off the PROJECT row).
        let story = seed_chain_to_story(&pool).await;
        let project = find_project_ancestor(&pool, &story)
            .await
            .expect("project ancestor of the seeded story");

        add_repo_link(&pool, &project, PRIMARY_SLUG, true)
            .await
            .expect("primary repo link");
        add_repo_link(&pool, &project, SECONDARY_SLUG, false)
            .await
            .expect("secondary (non-primary) repo link");

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task under the seeded story")
            .to_string();

        (pool, task)
    }

    /// Count `task_files` rows for a task, optionally narrowed to one `kind`.
    async fn count_task_files(pool: &SqlitePool, task_id: &str, kind: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM task_files WHERE task_id = $1 AND kind = $2",
        )
        .bind(task_id)
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("count task_files")
    }

    // -----------------------------------------------------------------------
    // canonical_file_key — bare vs {repo: primary, path} fold to the SAME key.
    // -----------------------------------------------------------------------

    /// THE KEY R2 INVARIANT: a bare `"src/x.rs"` and an explicit
    /// `{repo: "<primary-slug>", path: "src/x.rs"}` canonicalise to the SAME
    /// `(None, "src/x.rs")` key — both the NULL/primary bucket — so the two
    /// spellings never false-distinguish. A NON-primary slug stays distinct
    /// (`Some(link_id)`), proving the fold is primary-specific, not blanket.
    #[tokio::test]
    async fn canonical_key_folds_bare_and_explicit_primary() {
        let (pool, task) = seed_project_link_task().await;

        // Resolve the linked repos off the task's own project ancestor (the same
        // resolution path the writers use), then drive the pure fn directly.
        let project = find_project_ancestor(&pool, &task)
            .await
            .expect("project ancestor of the task");
        let links = list_repo_links(&pool, &project)
            .await
            .expect("links on the task's project");

        let bare = canonical_file_key(&serde_json::json!("src/x.rs"), &links)
            .expect("bare key");
        let explicit_primary = canonical_file_key(
            &serde_json::json!({ "repo": PRIMARY_SLUG, "path": "src/x.rs" }),
            &links,
        )
        .expect("explicit-primary key");

        assert_eq!(
            bare, explicit_primary,
            "a bare path and an explicit-primary {{repo, path}} must fold to the SAME canonical key"
        );
        assert_eq!(bare, (None, "src/x.rs".to_owned()), "the folded key is the NULL/primary bucket");

        // A NON-primary slug keeps its specific repo_link_id ⇒ a DISTINCT key.
        let secondary_id = links
            .iter()
            .find(|l| l.slug == SECONDARY_SLUG)
            .map(|l| l.id.clone())
            .expect("secondary link present");
        let explicit_secondary = canonical_file_key(
            &serde_json::json!({ "repo": SECONDARY_SLUG, "path": "src/x.rs" }),
            &links,
        )
        .expect("explicit-secondary key");
        assert_eq!(
            explicit_secondary,
            (Some(secondary_id), "src/x.rs".to_owned()),
            "a non-primary slug keeps its specific repo_link_id (distinct key)"
        );
        assert_ne!(
            bare, explicit_secondary,
            "the non-primary key must NOT collapse into the primary bucket"
        );
    }

    /// End-to-end fold: appending BOTH the bare and the explicit-primary spelling
    /// of the same primary-repo file yields exactly ONE stored `task_files` row
    /// (the canonical key dedups them), not two.
    #[tokio::test]
    async fn append_bare_and_explicit_primary_store_one_row() {
        let (pool, task) = seed_project_link_task().await;

        let inserted = add_task_actual_files(
            &pool,
            &task,
            &[
                serde_json::json!("src/x.rs"),
                serde_json::json!({ "repo": PRIMARY_SLUG, "path": "src/x.rs" }),
            ],
        )
        .await
        .expect("append both spellings");

        assert_eq!(
            inserted, 1,
            "the bare + explicit-primary spellings fold to one canonical key ⇒ one row"
        );
        assert_eq!(
            count_task_files(&pool, &task, "actual").await,
            1,
            "exactly one actual row stored for the same primary-repo file"
        );

        // The single stored row is in the NULL/primary bucket (repo_link_id NULL).
        let repo_link_id: Option<String> =
            sqlx::query_scalar("SELECT repo_link_id FROM task_files WHERE task_id = $1")
                .bind(&task)
                .fetch_one(&pool)
                .await
                .expect("read the stored row's repo_link_id");
        assert_eq!(repo_link_id, None, "the folded primary-repo file stores NULL repo_link_id");
    }

    // -----------------------------------------------------------------------
    // add_task_actual_files — append-only idempotency via ON CONFLICT DO NOTHING.
    // -----------------------------------------------------------------------

    /// Re-appending the same `(repo_link_id, path)` is a no-op: the first append
    /// inserts one row and returns 1; the SECOND append returns 0 (collapsed on
    /// `idx_task_files_unique` via `ON CONFLICT DO NOTHING`) and the stored row
    /// count stays 1.
    #[tokio::test]
    async fn append_actual_is_idempotent() {
        let (pool, task) = seed_project_link_task().await;

        let first = add_task_actual_files(&pool, &task, &[serde_json::json!("src/x.rs")])
            .await
            .expect("first append");
        assert_eq!(first, 1, "first append inserts exactly one row");

        let second = add_task_actual_files(&pool, &task, &[serde_json::json!("src/x.rs")])
            .await
            .expect("second (idempotent) append");
        assert_eq!(second, 0, "re-appending the same key is a no-op (ON CONFLICT DO NOTHING)");

        assert_eq!(
            count_task_files(&pool, &task, "actual").await,
            1,
            "the table still holds exactly one row after the idempotent re-append"
        );
    }

    /// NO file write routes through `update_work_item` (AC): the actual-files
    /// writer records its provenance on the export-INERT `task_files` aggregate,
    /// never the task's `work_item` aggregate — so `update_work_item` (which would
    /// emit a `work_item.*` event and bump `work_items.updated_at`) is NOT on the
    /// write path. Asserted two ways: (a) the append records exactly one
    /// `task_files.actual_appended` event on `aggregate_type='task_files'`, and
    /// (b) it records ZERO new `aggregate_type='work_item'` events AND leaves the
    /// task's `work_items.updated_at` byte-identical (an `update_work_item` would
    /// have stamped CURRENT_TIMESTAMP).
    #[tokio::test]
    async fn actual_write_does_not_route_through_update_work_item() {
        let (pool, task) = seed_project_link_task().await;

        // Baseline: the task's current updated_at + the count of work_item events
        // on it BEFORE the file write (the seed created it, so this is non-zero).
        let updated_before: String =
            sqlx::query_scalar("SELECT updated_at FROM work_items WHERE id = $1")
                .bind(&task)
                .fetch_one(&pool)
                .await
                .expect("task updated_at before");
        let work_item_events_before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'work_item' AND aggregate_id = $1",
        )
        .bind(&task)
        .fetch_one(&pool)
        .await
        .expect("work_item events before");

        add_task_actual_files(&pool, &task, &[serde_json::json!("src/x.rs")])
            .await
            .expect("append actual file");

        // (a) Exactly one inert task_files event on the task_files aggregate.
        let inert: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events \
             WHERE aggregate_type = 'task_files' AND aggregate_id = $1 \
               AND event_type = 'task_files.actual_appended'",
        )
        .bind(&task)
        .fetch_one(&pool)
        .await
        .expect("count task_files event");
        assert_eq!(inert, 1, "the append records exactly one inert task_files event");

        // (b) NO new work_item-aggregate event on the task, and updated_at is
        // unchanged — proving the write never went through update_work_item.
        let work_item_events_after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'work_item' AND aggregate_id = $1",
        )
        .bind(&task)
        .fetch_one(&pool)
        .await
        .expect("work_item events after");
        assert_eq!(
            work_item_events_after, work_item_events_before,
            "the file write records NO new work_item event (does not route through update_work_item)"
        );

        let updated_after: String =
            sqlx::query_scalar("SELECT updated_at FROM work_items WHERE id = $1")
                .bind(&task)
                .fetch_one(&pool)
                .await
                .expect("task updated_at after");
        assert_eq!(
            updated_before, updated_after,
            "the task's updated_at is unchanged (no update_work_item bump)"
        );
    }
}
