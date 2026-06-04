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

use std::path::{Component, Path, PathBuf};

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
/// (canonical recipe, A8 wave). Most columns are NOT NULL (`String`/`i64`), but
/// `local_path` (migration 0014) is nullable, so the impl now carries the
/// `Option<String>` decode/type bound alongside the `String` bound (the generic
/// recipe does not infer the Option bound from the String bound). `is_primary`
/// mirrors the INTEGER 0/1 as `i64`. Replaces the old `query_as!` `AS "col!"`
/// macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for RepoLink
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(RepoLink {
            id: row.try_get("id")?,
            project_id: row.try_get("project_id")?,
            slug: row.try_get("slug")?,
            position: row.try_get("position")?,
            is_primary: row.try_get("is_primary")?,
            local_path: row.try_get("local_path")?,
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
            local_path,
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

// ---------------------------------------------------------------------------
// Clone-path resolution (T2b) — pure/DB path normalisation + cwd→project bind.
// ---------------------------------------------------------------------------

/// Normalise a filesystem path string into a canonical *comparison* form for
/// component-boundary prefix matching. PRIVATE: callers use the `resolve_*` /
/// `select_*` wrappers below.
///
/// Pinned ordered algorithm (the order is load-bearing — do not reorder):
///   1. Strip a leading Windows verbatim prefix, longest-match FIRST:
///      - verbatim-UNC `\\?\UNC\` → strip the prefix, then prepend `\\` so the
///        remainder keeps a leading DOUBLE separator
///        (`\\?\UNC\server\share` → `\\server\share`, NOT `UNC\server\share`).
///      - plain verbatim `\\?\` → strip it (`\\?\C:\dev` → `C:\dev`).
///      The UNC check MUST precede the plain check (it is the longer prefix).
///   2. Replace every `\` with `/` (so the UNC case `\\server\share` becomes
///      `//server/share` — the double leading separator survives as `//`).
///   3. Strip a SINGLE trailing `/`, but never reduce a root to empty: a bare
///      `/` (unix root) and a `C:/` drive root (`^[A-Za-z]:/$`) are left intact.
///   4. On `cfg(windows)` ONLY, lowercase via `to_ascii_lowercase()`.
///
/// NOTE step 4 is HOST-keyed, not path-keyed: the same path string normalises
/// differently on Windows vs Unix (case-fold). This is correct for the
/// single-machine-now use case (the cwd and the stored `local_path` were both
/// produced on the same host) but is the documented limitation in the plan's
/// Risks note — a future cross-host store would need a path-keyed casing policy.
///
/// No `std::fs::canonicalize`: the path may name a directory that does not yet
/// exist (the clone has not happened), so this is purely lexical.
fn normalise_path_for_compare(p: &str) -> String {
    // Step 1: strip the Windows verbatim prefix (UNC form first — it is longer).
    let stripped: String = if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        // Re-prepend `\\` so the UNC host keeps its double leading separator.
        format!(r"\\{rest}")
    } else if let Some(rest) = p.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        p.to_owned()
    };

    // Step 2: normalise separators.
    let mut s: String = stripped.replace('\\', "/");

    // Step 3: strip a single trailing `/`, but never collapse a root to empty.
    if s.ends_with('/') && !is_root(&s) {
        s.pop();
    }

    // Step 4 (windows only): host-keyed case fold.
    #[cfg(windows)]
    {
        s = s.to_ascii_lowercase();
    }

    s
}

/// True iff `s` is a path root that must NOT be trailing-slash-stripped to
/// empty: the unix root `/`, or a drive root `^[A-Za-z]:/$` (e.g. `C:/`).
fn is_root(s: &str) -> bool {
    if s == "/" {
        return true;
    }
    let b = s.as_bytes();
    // `C:/` — exactly three bytes: ASCII letter, colon, slash.
    b.len() == 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}

/// Join a repo-relative path `rel` against a clone directory `local_path`,
/// returning a path that can NEVER ascend above the clone dir.
///
/// The `-> PathBuf` signature has no error channel, so misuse CLAMPS rather than
/// erroring or escaping:
///   - `local_path` is normalised via [`normalise_path_for_compare`] and used as
///     the base.
///   - `rel` is normalised lexically: its `\` separators are folded to `/` first
///     (so a Windows-style `rel` splits on BOTH Windows and Unix), then only the
///     `Component::Normal` parts are kept and pushed onto the base. Every
///     `ParentDir` (`..`) is DROPPED (clamp — no escape), every `CurDir` (`.`)
///     skipped, and any absolute anchor (`RootDir` / a drive-or-UNC `Prefix`) is
///     IGNORED so an absolute `rel` is treated relative-to-the-base instead of
///     REPLACING it (the naïve `PathBuf::join` bug this guards against).
///
/// Security invariant: a `..`-escaping or absolute `rel` is clamped to within
/// `local_path` and can never escape. No DB; no canonicalize.
pub fn resolve_repo_path(local_path: &str, rel: &str) -> PathBuf {
    let mut out = PathBuf::from(normalise_path_for_compare(local_path));

    // Fold `\` → `/` so a Windows-style `rel` splits into components on Unix too
    // (on Unix, `Path::components` treats `\` as an ordinary `Normal` byte).
    let rel_norm = rel.replace('\\', "/");
    for comp in Path::new(&rel_norm).components() {
        match comp {
            Component::Normal(part) => out.push(part),
            // Drop `..` (clamp), skip `.`, ignore any absolute anchor — an
            // absolute `rel` must NOT replace the base.
            Component::ParentDir
            | Component::CurDir
            | Component::RootDir
            | Component::Prefix(_) => {}
        }
    }

    out
}

/// Q3: select the single project whose linked clone dir is the longest
/// component-boundary prefix of `cwd`. Pure (DB-free) for direct unit testing;
/// the DB wrapper is [`resolve_cwd_to_project`].
///
/// Algorithm:
///   - Normalise `cwd` and each candidate `local_path` via
///     [`normalise_path_for_compare`].
///   - A candidate matches on a COMPONENT BOUNDARY (not a raw string prefix):
///     `cwd == local_path` OR `cwd` starts with `local_path + "/"`. So a cwd
///     `/dev/foobar` does NOT match a `local_path` `/dev/foo`.
///   - Among the matches keep the LONGEST normalised `local_path` (most
///     specific — resolves nesting like `/dev/mono` vs `/dev/mono/pkg` to the
///     deeper repo).
///   - Collect the DISTINCT `project_id`s in that longest set. Return
///     `Some(project_id)` iff EXACTLY ONE distinct project_id; otherwise `None`.
///     (Two distinct projects sharing the longest clone dir is a genuine tie ⇒
///     `None`; the same project_id appearing twice at the longest length is NOT
///     a tie ⇒ resolves to that one project.)
pub fn select_longest_prefix_project(
    cwd: &str,
    candidates: &[(String, String)],
) -> Option<String> {
    let cwd_norm = normalise_path_for_compare(cwd);

    let mut best_len: usize = 0;
    // Distinct project_ids among the current longest-match set.
    let mut best_projects: Vec<String> = Vec::new();

    for (project_id, local_path) in candidates {
        let base = normalise_path_for_compare(local_path);

        // Component-boundary match: equal, or cwd extends base past a `/`.
        let matches = cwd_norm == base
            || cwd_norm
                .strip_prefix(&base)
                .is_some_and(|tail| tail.starts_with('/'));
        if !matches {
            continue;
        }

        let len = base.len();
        if len > best_len {
            best_len = len;
            best_projects.clear();
            best_projects.push(project_id.clone());
        } else if len == best_len {
            if !best_projects.contains(project_id) {
                best_projects.push(project_id.clone());
            }
        }
    }

    match best_projects.as_slice() {
        [only] => Some(only.clone()),
        [] => None,
        _ => {
            tracing::debug!(
                cwd = %cwd,
                tied_projects = ?best_projects,
                "cwd→project resolution: multiple distinct projects share the \
                 longest clone-dir prefix; returning None (genuine tie)"
            );
            None
        }
    }
}

/// DB wrapper over [`select_longest_prefix_project`]: load every linked clone
/// dir (project_id, local_path) for a NON-soft-deleted project and resolve
/// `cwd` to a single project, or `None` (no match, or a tie).
///
/// The `w.deleted_at IS NULL` guard excludes soft-deleted (tombstoned) projects
/// so a cwd never binds to a tombstoned project (precedent: the
/// `STORY_FINDING_QUEUE_SQL` tombstone JOIN in `findings_query.rs`).
pub async fn resolve_cwd_to_project(
    db: &impl DbClient,
    cwd: &str,
) -> Result<Option<String>, AppError> {
    let candidates = db
        .query_all::<(String, String)>(
            "SELECT rl.project_id, rl.local_path \
             FROM repo_links rl \
             JOIN work_items w ON w.id = rl.project_id \
             WHERE rl.local_path IS NOT NULL AND w.deleted_at IS NULL",
            args![],
        )
        .await?;

    Ok(select_longest_prefix_project(cwd, &candidates))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Security invariant: a `..`-escaping `rel` is clamped INSIDE the base —
    /// the result is the base joined with the surviving `Normal` components,
    /// never an ancestor of the base.
    #[test]
    fn resolve_repo_path_clamps_parent_escape() {
        let got = resolve_repo_path("/repo/base", "../../etc/passwd");
        // The `..` are dropped; only `etc/passwd` survives, joined onto the base.
        let expected = PathBuf::from("/repo/base").join("etc").join("passwd");
        assert_eq!(got, expected, "`..` must be clamped within the base");
        // And the result is strictly within the normalised base.
        assert!(
            got.starts_with(normalise_path_for_compare("/repo/base")),
            "clamped path must stay under the base: {got:?}"
        );
    }

    /// Two DISTINCT projects sharing the same (longest) clone dir is a genuine
    /// tie ⇒ `None`.
    #[test]
    fn select_longest_prefix_project_distinct_tie_is_none() {
        let candidates = [
            ("projA".to_string(), "/dev/foo".to_string()),
            ("projB".to_string(), "/dev/foo".to_string()),
        ];
        assert_eq!(
            select_longest_prefix_project("/dev/foo/x", &candidates),
            None,
            "distinct-project tie on the same clone dir must resolve to None"
        );
    }
}
