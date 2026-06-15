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

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`FootprintFile`] derived
/// shape (T5) — lives beside the read helpers like [`TaskFile`]'s. The footprint
/// SELECT projects only `(repo_link_id, path)`: the NOT NULL `path` → `String`,
/// the nullable `repo_link_id` → `Option<String>`.
use crate::domain::FootprintFile;

impl<'r, R> sqlx::FromRow<'r, R> for FootprintFile
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(FootprintFile {
            repo_link_id: row.try_get("repo_link_id")?,
            path: row.try_get("path")?,
        })
    }
}

/// Run a DERIVED footprint query: the DISTINCT `(repo_link_id, path)` union over
/// the `task_files` rows of the task set named by the caller's `sql` (a static
/// `SELECT DISTINCT repo_link_id, path FROM task_files WHERE task_id IN (<member
/// subquery>)`). This is the shared primitive the story/sprint footprint reads in
/// `reads.rs` compose — they each supply the membership subquery; the dedup +
/// cross-kind collapse semantics live here, in one place.
///
/// **Why a plain `SELECT DISTINCT` dedupes correctly across NULL repo_link_id.**
/// SQL `DISTINCT` treats two NULLs as EQUAL for the purpose of row distinctness
/// (unlike a UNIQUE *index*, where the storage layer needed `COALESCE(repo_link_id,'')`
/// to fold NULL≡'' — see `idx_task_files_unique`). So two primary-bucket rows
/// `(NULL, "src/x.rs")` collapse to one DISTINCT row WITHOUT any COALESCE here,
/// which is exactly the dedup we want for the footprint. (`Some(id)` rows still
/// distinguish by their specific id, as desired.) Because `kind` is NOT in the
/// projection, a path that is BOTH `expected` and `actual` also collapses to one
/// row. Read-only, no transaction; ordered `repo_link_id, path` for determinism.
///
/// `args` binds the membership subquery's parameter(s) (e.g. `$1` = the story id
/// or sprint id); the SQL stays a `&'static str` per the `DbClient` seam.
pub(crate) async fn footprint_over(
    db: &impl DbClient,
    sql: &'static str,
    args: crate::db::Args,
) -> Result<Vec<FootprintFile>, AppError> {
    db.query_all::<FootprintFile>(sql, args).await
}

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

/// Outcome of a close-time reconcile ([`reconcile_task_files_at_close`]) — the
/// divergence counts the caller (and the audit activity) report. `cleared` is the
/// number of untouched-EXPECTED rows DELETEd on THIS run (drives idempotency: a
/// re-run finds them already gone ⇒ `cleared == 0`); `unexpected_actual` is the
/// count of ACTUAL canonical keys that were never in the EXPECTED set (a stable
/// condition that survives re-runs, since ACTUAL is append-only and never pruned).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileOutcome {
    /// EXPECTED rows cleared this run because no ACTUAL row matched (idempotent:
    /// 0 on a re-run once they are gone).
    pub cleared: usize,
    /// ACTUAL canonical keys that were never EXPECTED (over-report; NEVER pruned).
    pub unexpected_actual: usize,
}

/// Reconcile a task's EXPECTED file set against its ACTUAL set AT CLOSE, then
/// audit any material divergence (T4).
///
/// **What it does.** CLEARS every `kind='expected'` row whose canonical
/// `(COALESCE(repo_link_id,''), path)` key has NO matching `kind='actual'` row for
/// the SAME task — i.e. files that were planned-to-touch but never actually
/// touched. EXPECTED and ACTUAL stay DISTINCT kinds throughout: this only DELETEs
/// the untouched EXPECTED rows; it NEVER converts/merges an EXPECTED row into an
/// ACTUAL one, and it NEVER touches the ACTUAL set (ACTUAL is append-only — a
/// touched-then-reverted file is a tolerated over-report, not pruned here).
///
/// **Idempotency (Grounds R4 "clear destroys intent", R7 "reclaim partial
/// actuals").** Safe to run repeatedly: a second pass after a prior reconcile
/// finds the untouched-EXPECTED rows already gone, so it DELETEs zero, appends NO
/// audit, and returns `cleared == 0`. This makes it safe under a `complete_task`
/// re-run (crash-recovery / double-call) AND under a lease-reclaim re-open then
/// re-close — ACTUAL recording is INDEPENDENT of EXPECTED presence, so a partial
/// ACTUAL set recorded before a reclaim is preserved across the re-close. The
/// audit append is gated on `cleared > 0` (a deletion genuinely happened on THIS
/// run) precisely so a re-close never re-appends the same audit row.
///
/// **Audit-on-divergence (not silent).** When the reconcile finds MATERIAL
/// divergence — defined here as `cleared > 0` (at least one EXPECTED row was
/// cleared because it was never touched) — it appends EXACTLY ONE `reconcile`-kind
/// activity (origin `implement`) summarising the divergence counts (cleared +
/// any unexpected-ACTUAL), via the shared [`append_activity`] writer (its own tx +
/// `work_item.activity_appended` event). The expected set is NOT cleared silently.
/// The unexpected-ACTUAL count is reported in that same audit entry, but does NOT
/// by itself trigger an audit (it is a stable, re-run-surviving condition, so
/// firing on it would break the re-close idempotency the close routes rely on);
/// any nonzero `cleared` is the materiality threshold.
///
/// Each underlying write owns its own `BEGIN IMMEDIATE` tx (the reconcile DELETE +
/// its inert `task_files` event in one tx; the audit activity in `append_activity`'s
/// own tx), matching the Option-A seam the other writers in this module use — the
/// close routes COMPOSE this step, they do not wrap it in an outer tx.
pub async fn reconcile_task_files_at_close(
    db: &impl DbClient,
    task_id: &str,
) -> Result<ReconcileOutcome, AppError> {
    // Read both sets up front (read-only, autocommit) so we can compute the
    // unexpected-ACTUAL count for the audit summary. The DELETE below is the
    // authoritative mutation; these reads only inform the audit text.
    let actual = list_task_files(db, task_id, Some("actual")).await?;
    let expected = list_task_files(db, task_id, Some("expected")).await?;

    // Canonical key per stored row: COALESCE(repo_link_id,'') ⇒ the NULL/primary
    // bucket maps to the empty string, mirroring the storage UNIQUE index (R2).
    let key = |f: &TaskFile| (f.repo_link_id.clone().unwrap_or_default(), f.path.clone());
    let unexpected_actual = actual
        .iter()
        .filter(|f| !expected.iter().any(|e| key(e) == key(f)))
        .count();

    // CLEAR the untouched-EXPECTED rows in ONE tx + one inert event. The DELETE is
    // expressed entirely in SQL (a correlated NOT EXISTS over the ACTUAL rows for
    // the same task + canonical key) so it is atomic and idempotent: a re-run
    // matches zero rows once the untouched-EXPECTED are gone. `rows_affected` is
    // the authoritative "did this run clear anything" signal — NOT the in-memory
    // read above (which could race a concurrent ACTUAL append between the read and
    // the writer lock; the DELETE re-evaluates under the RESERVED lock).
    let mut tx = db.begin().await?;
    let cleared = tx
        .execute(
            // Delete by id of the doomed EXPECTED rows. The id-subquery (rather
            // than a `DELETE … AS e` alias on the target table) keeps this portable
            // across SQLite versions; the correlated NOT EXISTS over the ACTUAL
            // rows is keyed on the canonical `(COALESCE(repo_link_id,''), path)`
            // bucket, matching the storage UNIQUE index (R2).
            r#"
            DELETE FROM task_files
            WHERE id IN (
                SELECT e.id FROM task_files AS e
                WHERE e.task_id = $1
                  AND e.kind = 'expected'
                  AND NOT EXISTS (
                      SELECT 1 FROM task_files AS a
                      WHERE a.task_id = e.task_id
                        AND a.kind = 'actual'
                        AND COALESCE(a.repo_link_id, '') = COALESCE(e.repo_link_id, '')
                        AND a.path = e.path
                  )
            )
            "#,
            args![task_id.to_owned()],
        )
        .await? as usize;

    // Only record the inert reconcile event when the DELETE actually removed rows
    // (a zero-row reconcile is a true no-op — mirroring the empty-append reject in
    // `add_task_actual_files`, which never records an event for a zero-row batch).
    if cleared > 0 {
        let payload = serde_json::json!({
            "task_id": task_id,
            "cleared_expected": cleared,
            "unexpected_actual": unexpected_actual,
        });
        record_inert_event(
            tx.as_mut(),
            "task_files",
            task_id,
            "task_files.reconciled",
            payload,
        )
        .await?;
        tx.commit().await?;
    } else {
        // Nothing cleared ⇒ roll back (no event) — the idempotent re-run path (no
        // EXPECTED row was untouched, or a prior reconcile already cleared them).
        // An explicit `drop` documents the rollback intent; `commit` consumed `tx`
        // on the other branch, so this is the only place `tx` is disposed here.
        drop(tx);
    }

    // Audit-on-divergence: a MATERIAL divergence (≥1 EXPECTED row cleared this
    // run) appends EXACTLY ONE `reconcile` activity describing the counts. Gated on
    // `cleared > 0` so a re-close never re-appends (idempotency). The summary also
    // names the unexpected-ACTUAL over-report for completeness.
    if cleared > 0 {
        let summary = format!(
            "files_touched reconcile at close: cleared {cleared} expected file(s) that were \
             never actually touched; {unexpected_actual} actual file(s) were not in the \
             expected set (over-report, kept)"
        );
        let audit_payload = serde_json::json!({
            "cleared_expected": cleared,
            "unexpected_actual": unexpected_actual,
        });
        append_activity(
            db,
            task_id,
            "reconcile",
            None,
            &summary,
            Some(&audit_payload),
            Some("implement"),
        )
        .await?;
    }

    Ok(ReconcileOutcome {
        cleared,
        unexpected_actual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::repo::test_support::{seed_chain_to_story, seed_sprint};
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

    // -----------------------------------------------------------------------
    // reconcile_task_files_at_close (T4) — clear untouched-EXPECTED, audit on
    // divergence, idempotent under re-run, and wired into transition_status→done.
    // -----------------------------------------------------------------------

    /// Count `reconcile`-kind activity rows for a task (the audit-on-divergence
    /// signal). `work_item_activity.entry_kind` carries the wire-form `'reconcile'`.
    async fn count_reconcile_activity(pool: &SqlitePool, task_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM work_item_activity \
             WHERE work_item_id = $1 AND entry_kind = 'reconcile'",
        )
        .bind(task_id)
        .fetch_one(pool)
        .await
        .expect("count reconcile activity")
    }

    /// (a) The reconcile CLEARS EXPECTED rows with no matching ACTUAL, KEEPS the
    /// EXPECTED row that WAS actually touched, and NEVER prunes any ACTUAL row
    /// (including an over-report ACTUAL that was never EXPECTED). EXPECTED and
    /// ACTUAL stay DISTINCT kinds.
    #[tokio::test]
    async fn reconcile_clears_untouched_expected_keeps_the_rest() {
        let (pool, task) = seed_project_link_task().await;

        // EXPECTED: planned to touch a.rs (touched) + b.rs (NOT touched).
        set_task_expected_files(
            &pool,
            &task,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/b.rs")],
        )
        .await
        .expect("set expected");
        // ACTUAL: actually touched a.rs (matches an expected) + c.rs (over-report,
        // never expected). b.rs was expected but never touched.
        add_task_actual_files(
            &pool,
            &task,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/c.rs")],
        )
        .await
        .expect("append actual");

        let outcome = reconcile_task_files_at_close(&pool, &task)
            .await
            .expect("reconcile runs");
        assert_eq!(outcome.cleared, 1, "exactly the untouched EXPECTED (b.rs) is cleared");
        assert_eq!(
            outcome.unexpected_actual, 1,
            "exactly one ACTUAL (c.rs) was never EXPECTED (an over-report)"
        );

        // EXPECTED now holds ONLY a.rs (the touched one); b.rs is gone.
        let expected = list_task_files(&pool, &task, Some("expected"))
            .await
            .expect("list expected after reconcile");
        let expected_paths: Vec<&str> = expected.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            expected_paths,
            vec!["src/a.rs"],
            "the untouched EXPECTED (b.rs) is cleared; the touched one (a.rs) stays"
        );

        // ACTUAL is UNTOUCHED — both a.rs and c.rs (the over-report) remain. The
        // reconcile never prunes ACTUAL, and EXPECTED/ACTUAL stay distinct kinds.
        let actual = list_task_files(&pool, &task, Some("actual"))
            .await
            .expect("list actual after reconcile");
        let actual_paths: Vec<&str> = actual.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            actual_paths,
            vec!["src/a.rs", "src/c.rs"],
            "ACTUAL is preserved verbatim (the over-report c.rs is NOT pruned)"
        );
        assert_eq!(
            count_task_files(&pool, &task, "actual").await,
            2,
            "both actual rows survive the reconcile"
        );
    }

    /// (c) A material divergence (≥1 EXPECTED cleared) appends EXACTLY ONE
    /// `reconcile` audit activity (not silent). The audit fires once per material
    /// reconcile run.
    #[tokio::test]
    async fn reconcile_appends_exactly_one_audit_activity_on_divergence() {
        let (pool, task) = seed_project_link_task().await;

        set_task_expected_files(
            &pool,
            &task,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/b.rs")],
        )
        .await
        .expect("set expected");
        // a.rs touched; b.rs NOT → b.rs will be cleared → material divergence.
        add_task_actual_files(&pool, &task, &[serde_json::json!("src/a.rs")])
            .await
            .expect("append actual");

        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            0,
            "no audit before the reconcile"
        );

        reconcile_task_files_at_close(&pool, &task)
            .await
            .expect("reconcile runs");

        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            1,
            "a material divergence appends exactly one reconcile audit activity (not silent)"
        );
    }

    /// (b) Running the reconcile TWICE is a no-op: the second pass clears zero,
    /// appends NO second audit, and leaves the row counts unchanged. This is the
    /// idempotency guarantee the close routes lean on (re-run / reclaim re-close).
    #[tokio::test]
    async fn reconcile_is_idempotent_on_rerun() {
        let (pool, task) = seed_project_link_task().await;

        set_task_expected_files(
            &pool,
            &task,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/b.rs")],
        )
        .await
        .expect("set expected");
        add_task_actual_files(&pool, &task, &[serde_json::json!("src/a.rs")])
            .await
            .expect("append actual");

        let first = reconcile_task_files_at_close(&pool, &task)
            .await
            .expect("first reconcile");
        assert_eq!(first.cleared, 1, "first pass clears the untouched EXPECTED (b.rs)");
        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            1,
            "first pass appends one audit"
        );
        let expected_after_first = count_task_files(&pool, &task, "expected").await;
        let actual_after_first = count_task_files(&pool, &task, "actual").await;

        // Second pass: nothing left to clear ⇒ a true no-op.
        let second = reconcile_task_files_at_close(&pool, &task)
            .await
            .expect("second (idempotent) reconcile");
        assert_eq!(second.cleared, 0, "the re-run clears nothing (b.rs already gone)");

        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            1,
            "the re-run appends NO second audit (idempotent)"
        );
        assert_eq!(
            count_task_files(&pool, &task, "expected").await,
            expected_after_first,
            "expected row count unchanged by the re-run"
        );
        assert_eq!(
            count_task_files(&pool, &task, "actual").await,
            actual_after_first,
            "actual row count unchanged by the re-run"
        );

        // No-divergence case is ALSO a no-op: a task whose expected set exactly
        // matches its actual set clears nothing and audits nothing.
        let (pool2, task2) = seed_project_link_task().await;
        set_task_expected_files(&pool2, &task2, &[serde_json::json!("src/only.rs")])
            .await
            .expect("set expected");
        add_task_actual_files(&pool2, &task2, &[serde_json::json!("src/only.rs")])
            .await
            .expect("append actual");
        let outcome = reconcile_task_files_at_close(&pool2, &task2)
            .await
            .expect("reconcile runs");
        assert_eq!(outcome.cleared, 0, "a fully-matching set clears nothing");
        assert_eq!(outcome.unexpected_actual, 0, "no unexpected actual");
        assert_eq!(
            count_reconcile_activity(&pool2, &task2).await,
            0,
            "no audit when there is no divergence (not silent ≠ noisy)"
        );
    }

    /// (d, non-team route) The plain `transition_status` → `done` path
    /// (`update_work_item_status`) TRIGGERS the reconcile for a `kind='task'`
    /// transition: untouched-EXPECTED is cleared and an audit activity is appended.
    /// A re-transition to `done` is idempotent (no second clear, no second audit).
    #[tokio::test]
    async fn transition_status_done_triggers_reconcile_and_is_idempotent() {
        let (pool, task) = seed_project_link_task().await;

        set_task_expected_files(
            &pool,
            &task,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/b.rs")],
        )
        .await
        .expect("set expected");
        add_task_actual_files(&pool, &task, &[serde_json::json!("src/a.rs")])
            .await
            .expect("append actual");

        // Move the task to `done` via the non-team close route.
        update_work_item_status(&pool, &task, "done")
            .await
            .expect("transition to done");

        // The reconcile fired: b.rs (untouched EXPECTED) cleared, audit appended.
        let expected_paths: Vec<String> = list_task_files(&pool, &task, Some("expected"))
            .await
            .expect("expected after done")
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(
            expected_paths,
            vec!["src/a.rs".to_string()],
            "transition_status→done cleared the untouched EXPECTED (b.rs)"
        );
        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            1,
            "transition_status→done appended exactly one reconcile audit"
        );

        // A NON-done transition does NOT reconcile (guard: only kind=task→done).
        // Re-transition to `done` is idempotent: no second clear, no second audit.
        update_work_item_status(&pool, &task, "done")
            .await
            .expect("re-transition to done");
        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            1,
            "a re-transition to done does not re-audit (idempotent)"
        );
        assert_eq!(
            count_task_files(&pool, &task, "expected").await,
            1,
            "a re-transition to done does not re-clear"
        );
    }

    /// The reconcile fires ONLY for `kind='task'` → `done` transitions: a NON-done
    /// status change on a task never reconciles, and (defensively) a non-task item
    /// never has its (absent) file set touched.
    #[tokio::test]
    async fn transition_status_non_done_does_not_reconcile() {
        let (pool, task) = seed_project_link_task().await;

        set_task_expected_files(
            &pool,
            &task,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/b.rs")],
        )
        .await
        .expect("set expected");
        add_task_actual_files(&pool, &task, &[serde_json::json!("src/a.rs")])
            .await
            .expect("append actual");

        // Transition to a NON-done status: the reconcile must NOT fire.
        update_work_item_status(&pool, &task, "in_progress")
            .await
            .expect("transition to in_progress");

        assert_eq!(
            count_task_files(&pool, &task, "expected").await,
            2,
            "a non-done transition leaves the EXPECTED set intact"
        );
        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            0,
            "a non-done transition appends no reconcile audit"
        );
    }

    // -----------------------------------------------------------------------
    // Derived story/sprint footprint (T5) — DISTINCT (repo_link_id, path) union
    // over member tasks, deduped across kind; WorkItemDetail story-files fold is
    // story-kind-gated.
    // -----------------------------------------------------------------------

    /// Seed the legal chain and return `(pool, story_id)`. Unlike
    /// [`seed_project_link_task`] this returns the STORY so the footprint reads
    /// (which union over the story's task children) are exercisable; repo links
    /// are NOT needed (the footprint tests use bare-path primary-bucket files).
    async fn seed_story() -> (SqlitePool, String) {
        let pool = connect_in_memory().await.expect("migrated in-memory pool");
        let story = seed_chain_to_story(&pool).await;
        (pool, story)
    }

    /// Create a `task` under `story` and return its id.
    async fn task_under(pool: &SqlitePool, story: &str, title: &str) -> String {
        create_work_item(pool, "task", Some(story), title, None)
            .await
            .expect("task under story")
            .to_string()
    }

    /// (a) Two tasks sharing a path → that path appears EXACTLY ONCE in the
    /// story footprint (DISTINCT union across tasks). (b) A path that is BOTH
    /// `expected` AND `actual` on one task → appears EXACTLY ONCE (deduped across
    /// kind — the footprint SELECT omits `kind`).
    #[tokio::test]
    async fn story_footprint_dedups_across_tasks_and_kinds() {
        let (pool, story) = seed_story().await;
        let t1 = task_under(&pool, &story, "T1").await;
        let t2 = task_under(&pool, &story, "T2").await;

        // (a) shared path across two tasks: both expect src/shared.rs.
        set_task_expected_files(&pool, &t1, &[serde_json::json!("src/shared.rs")])
            .await
            .expect("t1 expected");
        set_task_expected_files(&pool, &t2, &[serde_json::json!("src/shared.rs")])
            .await
            .expect("t2 expected");

        // (b) cross-kind dup on t1: src/both.rs is BOTH expected and actual.
        set_task_expected_files(
            &pool,
            &t1,
            &[serde_json::json!("src/shared.rs"), serde_json::json!("src/both.rs")],
        )
        .await
        .expect("t1 expected (with both.rs)");
        add_task_actual_files(&pool, &t1, &[serde_json::json!("src/both.rs")])
            .await
            .expect("t1 actual both.rs");

        // A task-unique actual to prove the union spans actual rows too.
        add_task_actual_files(&pool, &t2, &[serde_json::json!("src/only2.rs")])
            .await
            .expect("t2 actual only2.rs");

        let footprint = story_files_footprint(&pool, &story)
            .await
            .expect("story footprint");
        let paths: Vec<&str> = footprint.iter().map(|f| f.path.as_str()).collect();

        // DISTINCT union, ordered by (repo_link_id, path) — all in the NULL
        // primary bucket here, so ordering is by path. Each path appears ONCE.
        assert_eq!(
            paths,
            vec!["src/both.rs", "src/only2.rs", "src/shared.rs"],
            "the footprint is the DISTINCT (repo_link_id, path) union: shared.rs once \
             (two tasks), both.rs once (expected+actual on one task)"
        );
        // Every entry is the NULL/primary bucket (bare paths).
        assert!(
            footprint.iter().all(|f| f.repo_link_id.is_none()),
            "bare-path footprint entries all fold to the NULL/primary bucket"
        );
    }

    /// The SPRINT footprint is the DISTINCT union over the sprint's MEMBER tasks
    /// (the `sprint_tasks` junction), deduped identically to the story footprint.
    #[tokio::test]
    async fn sprint_footprint_unions_member_tasks() {
        let (pool, story) = seed_story().await;
        let sprint = seed_sprint(&pool).await;
        let t1 = task_under(&pool, &story, "T1").await;
        let t2 = task_under(&pool, &story, "T2").await;
        add_tasks_to_sprint(&pool, &sprint, &[t1.as_str(), t2.as_str()])
            .await
            .expect("bind tasks to sprint");

        set_task_expected_files(&pool, &t1, &[serde_json::json!("src/a.rs")])
            .await
            .expect("t1 expected");
        add_task_actual_files(&pool, &t1, &[serde_json::json!("src/a.rs")])
            .await
            .expect("t1 actual a.rs (cross-kind dup)");
        add_task_actual_files(&pool, &t2, &[serde_json::json!("src/b.rs")])
            .await
            .expect("t2 actual b.rs");

        let footprint = sprint_files_footprint(&pool, &sprint)
            .await
            .expect("sprint footprint");
        let paths: Vec<&str> = footprint.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["src/a.rs", "src/b.rs"],
            "sprint footprint is the DISTINCT union over member tasks (a.rs once \
             despite expected+actual)"
        );
    }

    /// (c) `WorkItemDetail.story_files_footprint` is POPULATED for a `kind='story'`
    /// item and is EMPTY for a non-story item (here a `task`) — EXACTLY mirroring
    /// the project-only `repo_links` fold.
    #[tokio::test]
    async fn work_item_detail_story_files_is_story_kind_gated() {
        let (pool, story) = seed_story().await;
        let task = task_under(&pool, &story, "T1").await;
        set_task_expected_files(&pool, &task, &[serde_json::json!("src/x.rs")])
            .await
            .expect("task expected");

        // Story detail: footprint populated (one path from its task child).
        let story_detail = get_work_item_detail(&pool, &story)
            .await
            .expect("story detail");
        let story_paths: Vec<&str> = story_detail
            .story_files_footprint
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(
            story_paths,
            vec!["src/x.rs"],
            "a kind='story' detail folds in the derived files footprint"
        );

        // Task detail (non-story): footprint EMPTY (kind-gated off).
        let task_detail = get_work_item_detail(&pool, &task)
            .await
            .expect("task detail");
        assert!(
            task_detail.story_files_footprint.is_empty(),
            "a non-story (task) detail has an empty story_files_footprint (kind-gated)"
        );
    }
}
