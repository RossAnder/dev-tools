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

/// Set or clear the per-machine clone directory (`local_path`) on a `repo_links`
/// row under the single-mutation-path discipline. `Some(raw)` SETS the column;
/// `None` CLEARS it back to NULL.
///
/// The owning project's id is resolved FIRST (so an absent `repo_link_id` is
/// `NotFound` BEFORE any write, and the event aggregate is the project's
/// `work_item`, letting the export drain re-render the project).
///
/// A `Some(raw)` value is first trimmed of surrounding whitespace (review R13)
/// then run through [`normalise_path_structural`] (separator-folding +
/// verbatim-prefix stripping + internal-separator collapse, but NO case fold) and
/// the STRUCTURAL form is what gets STORED and validated — storing the structural
/// (casing-preserved) form keeps the operator's casing in the detail/export
/// snapshot (review R7), while matching stays correct because
/// [`normalise_path_for_compare`] case-folds BOTH sides at compare time. We
/// validate the structural string is absolute via [`is_absolute_normalised`]
/// (`/`-rooted OR drive-anchored `^[A-Za-z]:/`, case-insensitive on the drive
/// letter) rather than gating on raw `Path::is_absolute`, which rejects `\dev\foo`
/// / `C:foo` and varies by host OS (validating the normalised form keeps a Linux
/// CI executor and a Windows operator consistent — review P5). A non-absolute
/// value is `Validation`.
///
/// Event `repo_link.local_path_changed` on the owning project's `work_item`
/// aggregate.
pub async fn set_repo_local_path(
    db: &impl DbClient,
    repo_link_id: &str,
    local_path: Option<&str>,
) -> Result<(), AppError> {
    // Resolve the owning project FIRST — absent id ⇒ NotFound BEFORE any write,
    // and the event aggregate is correct. `project_id` is NOT NULL.
    let project_id: String = db
        .query_opt::<Scalar<String>>(
            "SELECT project_id FROM repo_links WHERE id = $1",
            args![repo_link_id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("repo_link '{repo_link_id}' not found")))?
        .0;

    // Compute the value to STORE: trim, then normalise to the STRUCTURAL form,
    // THEN validate that structural form is absolute. Store the structural string
    // (casing preserved — review R7). `None` clears the column.
    let to_store: Option<String> = match local_path {
        Some(raw) => {
            // Trim surrounding whitespace before normalise/validate/store so a
            // value like " /dev/foo" is accepted (review R13).
            let trimmed = raw.trim();
            let normalised = normalise_path_structural(trimmed);
            if !is_absolute_normalised(&normalised) {
                return Err(AppError::Validation(format!(
                    "local_path must be absolute (a `/`-rooted or drive-anchored path); \
                     got '{raw}' (normalised '{normalised}')"
                )));
            }
            Some(normalised)
        }
        None => None,
    };

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE repo_links SET local_path = $2 WHERE id = $1",
            args![repo_link_id.to_owned(), to_store.clone()],
        )
        .await?;

    if affected == 0 {
        // Lost a race against a concurrent delete — surface NotFound rather than
        // emitting a spurious event.
        return Err(AppError::NotFound(format!(
            "repo_link '{repo_link_id}' not found"
        )));
    }

    let payload = serde_json::json!({
        "id": repo_link_id,
        "project_id": project_id,
        "local_path": to_store,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        &project_id,
        "repo_link.local_path_changed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// True iff the already-normalised (structural or compare) string `s` is
/// absolute: a `/`-rooted unix path OR a drive-anchored Windows path
/// (`^[A-Za-z]:/` — letter, colon, slash). Operates on the NORMALISED form
/// (separators already folded to `/`) so it is host-OS-independent — see
/// `set_repo_local_path`. The drive-letter check is case-insensitive
/// (`is_ascii_alphabetic`), so it holds on the casing-preserved structural form.
///
/// NOTE: a UNC path `//server/share` passes the leading-`/` branch and is
/// accepted INTENTIONALLY — UNC is a valid Windows clone directory, so this
/// predicate must not reject it (review R8). This is purely an *absoluteness*
/// gate, NOT a filesystem-sink safety gate: the future filesystem consumer of a
/// stored `local_path` (see the layer-1-staging NOTE above the resolution
/// section, review R11) MUST add its own host-class check (e.g. reject or
/// specially handle UNC / network shares) BEFORE using a stored path as a real
/// FS sink — accepting UNC here does not vouch for its safety as a write target.
fn is_absolute_normalised(s: &str) -> bool {
    if s.starts_with('/') {
        return true;
    }
    let b = s.as_bytes();
    // `C:/...` — letter, colon, slash.
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}

// ---------------------------------------------------------------------------
// Clone-path resolution (T2b) — pure/DB path normalisation + cwd→project bind.
// ---------------------------------------------------------------------------

/// Normalise a filesystem path string into a canonical *structural* form:
/// verbatim-prefix-stripped, `\`→`/`-folded, internal-separator-collapsed, and
/// trailing-slash-stripped — but with CASING PRESERVED. This is the STORAGE form
/// (`set_repo_local_path` stores it, keeping the operator's casing in the
/// detail/export snapshot) and the shared base of [`normalise_path_for_compare`].
/// PRIVATE: callers use the `resolve_*` / `select_*` wrappers and the
/// `is_absolute_normalised` predicate below.
///
/// Pinned ordered algorithm (the order is load-bearing — do not reorder):
///   1. Strip a leading Windows verbatim prefix, longest-match FIRST:
///      - verbatim-UNC `\\?\UNC\` → strip the prefix, then prepend `\\` so the
///        remainder keeps a leading DOUBLE separator
///        (`\\?\UNC\server\share` → `\\server\share`, NOT `UNC\server\share`).
///      - plain verbatim `\\?\` → strip it (`\\?\C:\dev` → `C:\dev`).
///        The UNC check MUST precede the plain check (it is the longer prefix).
///   2. Replace every `\` with `/` (so the UNC case `\\server\share` becomes
///      `//server/share` — the double leading separator survives as `//`).
///   3. Collapse runs of `/` into a single `/`, BUT preserve a leading DOUBLE
///      slash for UNC: a string starting `//` keeps its `//`, and only the
///      remainder is collapsed (`/dev//foo` → `/dev/foo`;
///      `//server//share` → `//server/share`).
///   4. Strip a SINGLE trailing `/`, but never reduce a root to empty: a bare
///      `/` (unix root) and a `C:/` drive root (`^[A-Za-z]:/$`) are left intact.
///
/// No `std::fs::canonicalize`: the path may name a directory that does not yet
/// exist (the clone has not happened), so this is purely lexical.
fn normalise_path_structural(p: &str) -> String {
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
    let folded: String = stripped.replace('\\', "/");

    // Step 3: collapse runs of `/` to a single `/`, preserving a leading `//`
    // (UNC). When the string begins with `//`, emit the `//` verbatim and
    // collapse only the remainder (the host/share separators); otherwise collapse
    // every run including the leading one.
    let leading_unc = folded.starts_with("//");
    let mut s = String::with_capacity(folded.len());
    // `body` is the portion subject to run-collapsing. For UNC we emit a literal
    // `//` first, then collapse the rest (with any further leading slashes after
    // the `//` trimmed, so `///x` → `//x`).
    let body = if leading_unc {
        s.push_str("//");
        folded.trim_start_matches('/')
    } else {
        folded.as_str()
    };
    let mut prev_slash = false;
    for ch in body.chars() {
        if ch == '/' {
            if !prev_slash {
                s.push('/');
            }
            prev_slash = true;
        } else {
            s.push(ch);
            prev_slash = false;
        }
    }

    // Step 4: strip a single trailing `/`, but never collapse a root to empty.
    if s.ends_with('/') && !is_root(&s) {
        s.pop();
    }

    s
}

/// Normalise a filesystem path string into a canonical *comparison* form for
/// component-boundary prefix matching: the [`normalise_path_structural`] form
/// PLUS a host-keyed case fold on `cfg(windows)`. Used by
/// [`select_longest_prefix_project`] and [`resolve_repo_path`] (which compares /
/// joins against a stored path); the storage path stores the structural form and
/// matching stays correct because the compare form folds case on BOTH sides.
///
/// PUBLIC (review R13): the server's companion split-brain guard needs a direct
/// IDENTITY comparison (`normalise(a) == normalise(b)`) rather than the
/// containment semantics of [`select_longest_prefix_project`] — a nested
/// vendored/scratch repo under the clone dir must NOT pass the guard. This is
/// the one sanctioned use beyond the `resolve_*` / `select_*` wrappers.
///
/// NOTE the case fold is HOST-keyed, not path-keyed: the same path string
/// normalises differently on Windows vs Unix. This is correct for the
/// single-machine-now use case (the cwd and the stored `local_path` were both
/// produced on the same host) but is the documented limitation in the plan's
/// Risks note — a future cross-host store would need a path-keyed casing policy.
pub fn normalise_path_for_compare(p: &str) -> String {
    #[allow(unused_mut)]
    let mut s = normalise_path_structural(p);

    // Host-keyed case fold (windows only).
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

// ---------------------------------------------------------------------------
// NOTE (review R11): the resolution functions below — `resolve_repo_path`,
// `select_longest_prefix_project`, and the `resolve_cwd_to_project` DB wrapper —
// are LAYER-1 substrate, staged AHEAD of their layer-2 consumer (the
// harness-session-corpus cwd→project correlation; see ADR-0004). They have NO
// production caller yet (confirmed via a repo-wide grep). In particular,
// `resolve_repo_path`'s security-critical clamp invariant (`..`-escaping and
// absolute `rel` can never escape the base) is NOT yet exercised by a live flow
// — it is covered only by the unit tests in this module until the consumer
// lands. Treat the clamp guarantee as load-bearing the moment a real FS sink
// starts consuming the joined path.
// ---------------------------------------------------------------------------

/// Join a repo-relative path `rel` against a clone directory `local_path`,
/// returning a path that can NEVER ascend above the clone dir.
///
/// The `-> PathBuf` signature has no error channel, so misuse CLAMPS rather than
/// erroring or escaping:
///   - `local_path` is normalised via [`normalise_path_for_compare`] and used as
///     the base.
///   - `rel` is normalised lexically: its `\` separators are folded to `/` first
///     (so a Windows-style `rel` splits on BOTH Windows and Unix), then the
///     `Component::Normal` parts are pushed onto the base and each `ParentDir`
///     (`..`) LEXICALLY CANCELS the most-recently-pushed `Normal` component —
///     CLAMPED at the base, so a `..` can never pop INTO (above) the base. Every
///     `CurDir` (`.`) is skipped, and any absolute anchor (`RootDir` / a
///     drive-or-UNC `Prefix`) is IGNORED so an absolute `rel` is treated
///     relative-to-the-base instead of REPLACING it (the naïve `PathBuf::join`
///     bug this guards against). `a/../b` → `base/b`; `../../etc` → `base`
///     (clamped).
///
/// Security invariant: a `..`-escaping or absolute `rel` is clamped to within
/// `local_path` and can never escape. No DB; no canonicalize.
pub fn resolve_repo_path(local_path: &str, rel: &str) -> PathBuf {
    let mut out = PathBuf::from(normalise_path_for_compare(local_path));

    // Track how many `Normal` components we have pushed BEYOND the base so a
    // `..` only ever cancels a component we ourselves pushed — never one of the
    // base's own components (the clamp invariant). `depth==0` ⇒ a `..` is a
    // no-op (already at the base floor).
    let mut depth: usize = 0;

    // Fold `\` → `/` so a Windows-style `rel` splits into components on Unix too
    // (on Unix, `Path::components` treats `\` as an ordinary `Normal` byte).
    let rel_norm = rel.replace('\\', "/");
    for comp in Path::new(&rel_norm).components() {
        match comp {
            Component::Normal(part) => {
                out.push(part);
                depth += 1;
            }
            // `..` lexically cancels the last pushed component, clamped at the
            // base: pop only if we have pushed something beyond the base.
            Component::ParentDir => {
                if depth > 0 {
                    out.pop();
                    depth -= 1;
                }
                // depth == 0 ⇒ already at the base floor; drop the `..` (clamp).
            }
            // Skip `.`, ignore any absolute anchor — an absolute `rel` must NOT
            // replace the base.
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
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

        // Component-boundary match: equal, or cwd extends base past a `/`. A
        // filesystem-ROOT base (`/` or `C:/`) already ENDS in `/`, so the
        // stripped tail does NOT begin with a fresh `/` — accept a root base when
        // cwd starts with it (the boundary is the trailing `/` the root carries)
        // (review R1). A deeper non-root base still wins by longest-prefix below.
        let matches = cwd_norm == base
            || cwd_norm
                .strip_prefix(&base)
                .is_some_and(|tail| tail.starts_with('/') || base.ends_with('/'));
        if !matches {
            continue;
        }

        let len = base.len();
        if len > best_len {
            best_len = len;
            best_projects.clear();
            best_projects.push(project_id.clone());
        } else if len == best_len && !best_projects.contains(project_id) {
            best_projects.push(project_id.clone());
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
    // NOTE (review R12): this is an unindexed full-scan of `repo_links` filtered
    // on `local_path IS NOT NULL`. Acceptable at the current handful-of-repos
    // cardinality (the scan is over a tiny table). If cwd→project resolution
    // becomes hot, the upgrade is a partial index
    // (`CREATE INDEX … ON repo_links(local_path) WHERE local_path IS NOT NULL`)
    // in a future migration — NOT added here (no migration in this scope).
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
    use crate::db::connect_in_memory;
    use crate::repo::test_support::count_events_for;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    // =====================================================================
    // Pure-fn unit tests (T5) — `normalise_path_for_compare` is now PUB (R13:
    // the companion split-brain guard's identity comparison); the structural
    // normaliser stays private but is reachable here because this `mod tests`
    // is co-located in the same module (`super::*`). No DB.
    // =====================================================================

    // ---------------------------------------------------------------------
    // normalise_path_for_compare
    // ---------------------------------------------------------------------

    /// Plain verbatim `\\?\` prefix is stripped; `\` fold to `/`; on Windows the
    /// result is additionally case-folded. The expected value is built with the
    /// SAME host-keyed casing so the assertion holds on both Unix and Windows
    /// (we round the input through a known-good lower form on Windows).
    #[test]
    fn normalise_strips_plain_verbatim_prefix() {
        let got = normalise_path_for_compare(r"\\?\C:\dev\foo");
        // On Unix: "C:/dev/foo"; on Windows: "c:/dev/foo" (step-4 case fold).
        #[cfg(windows)]
        let expected = "c:/dev/foo";
        #[cfg(not(windows))]
        let expected = "C:/dev/foo";
        assert_eq!(got, expected);
    }

    /// Verbatim-UNC `\\?\UNC\Server\Share` strips the prefix and re-prepends `\\`
    /// so the leading DOUBLE separator survives the `\`→`/` fold as `//`. Uses an
    /// UPPERCASE input so the assertion also exercises the host-keyed case fold
    /// (review R20): on Windows the result is lowered to `//server/share`; on Unix
    /// the casing is preserved as `//Server/Share`.
    #[test]
    fn normalise_verbatim_unc_keeps_double_leading_separator() {
        let got = normalise_path_for_compare(r"\\?\UNC\Server\Share");
        #[cfg(windows)]
        let expected = "//server/share";
        #[cfg(not(windows))]
        let expected = "//Server/Share";
        assert_eq!(
            got, expected,
            "verbatim-UNC must keep the double leading separator as `//`"
        );
    }

    /// Internal repeated separators collapse to a single `/`, BUT a leading `//`
    /// (UNC) is preserved (review R4). Asserted on the STRUCTURAL form so the
    /// collapse is exercised independently of the host-keyed case fold.
    #[rstest]
    #[case("/dev//foo", "/dev/foo")]
    #[case("/dev///foo//bar", "/dev/foo/bar")]
    #[case("//server//share", "//server/share")]
    #[case("//server///share//x", "//server/share/x")]
    #[case(r"C:\\dev\\foo", "C:/dev/foo")]
    fn normalise_collapses_internal_separators(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(
            normalise_path_structural(input),
            expected,
            "internal `/` runs collapse; leading `//` (UNC) is preserved; input {input:?}"
        );
    }

    /// The compare form collapses internal separators too (it composes the
    /// structural form), with the host-keyed case fold applied on top (review R4).
    #[test]
    fn normalise_compare_collapses_and_case_folds() {
        let got = normalise_path_for_compare("/Dev//Foo");
        #[cfg(windows)]
        let expected = "/dev/foo";
        #[cfg(not(windows))]
        let expected = "/Dev/Foo";
        assert_eq!(got, expected);
    }

    /// `\` separators fold to `/`. Asserted as a HARD LITERAL (review R16):
    /// lowercase input so the value is case-fold-invariant — `a/b/c` is the
    /// expected on BOTH hosts (lowercasing it is a no-op), so the literal is not
    /// tautological with the SUT's own fold.
    #[test]
    fn normalise_folds_backslashes() {
        assert_eq!(normalise_path_for_compare(r"a\b\c"), "a/b/c");
    }

    /// A single trailing `/` is stripped, but a root is NEVER reduced to empty:
    /// `/` stays `/`. Asserted as HARD LITERALS (review R16); these cases are
    /// case-free so the literal holds on both hosts without re-running the SUT's
    /// case fold. (The drive-root `C:/` case, where the case fold DOES differ, is
    /// asserted separately below with a cfg-gated literal.)
    #[rstest]
    #[case("/dev/foo/", "/dev/foo")]
    #[case("/", "/")]
    fn normalise_trailing_slash_and_root(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(normalise_path_for_compare(input), expected, "input {input:?}");
    }

    /// The drive root `C:/` is NEVER reduced to empty by the trailing-slash strip.
    /// The expected is a cfg-gated HARD LITERAL because the drive letter DOES
    /// case-fold on Windows (`C:/` → `c:/`) but not on Unix (review R16).
    #[test]
    fn normalise_drive_root_not_emptied() {
        let got = normalise_path_for_compare("C:/");
        #[cfg(windows)]
        let expected = "c:/";
        #[cfg(not(windows))]
        let expected = "C:/";
        assert_eq!(got, expected);
    }

    /// On Windows ONLY, step 4 case-folds. This assertion is gated to `cfg(windows)`
    /// because the fold is HOST-keyed (documented limitation): on Unix the same
    /// input keeps its case, so a single cross-platform assertion is impossible.
    #[cfg(windows)]
    #[test]
    fn normalise_case_folds_on_windows() {
        assert_eq!(normalise_path_for_compare(r"C:\Dev\FOO"), "c:/dev/foo");
    }

    /// On Unix ONLY, casing is PRESERVED (the negative of the Windows case-fold).
    #[cfg(not(windows))]
    #[test]
    fn normalise_preserves_case_on_unix() {
        assert_eq!(normalise_path_for_compare("/Dev/FOO"), "/Dev/FOO");
    }

    /// THE KEY CROSS-MATCH the feature relies on: a Windows verbatim form
    /// `\\?\C:\dev\foo` and the bare stored form `C:/dev/foo` MUST normalise to
    /// the SAME string — this is what lets a cwd captured in verbatim form bind
    /// to a stored bare `local_path`. Asserted as an equality between the two
    /// normalised outputs (host-keyed casing cancels out — both go through step 4).
    #[test]
    fn normalise_verbatim_and_bare_cross_match() {
        let verbatim = normalise_path_for_compare(r"\\?\C:\dev\foo");
        let bare = normalise_path_for_compare("C:/dev/foo");
        assert_eq!(
            verbatim, bare,
            "verbatim `\\\\?\\C:\\dev\\foo` and bare `C:/dev/foo` must normalise identically"
        );
    }

    // ---------------------------------------------------------------------
    // is_absolute_normalised — focused accept/reject branches
    // ---------------------------------------------------------------------

    /// `is_absolute_normalised` accepts `/`-rooted and drive-anchored
    /// (`^[A-Za-z]:/`) NORMALISED strings and rejects relative / bare-drive /
    /// empty forms (review R19). Inputs are already-normalised structural forms
    /// (the predicate's contract), so no separator folding is needed here.
    #[rstest]
    #[case("/x", true)] // unix-rooted
    #[case("C:/x", true)] // drive-anchored (letter, colon, slash)
    #[case("C:foo", false)] // drive-relative — no slash after the colon
    #[case("dev/foo", false)] // plain relative
    #[case("c:", false)] // bare drive letter, no slash
    #[case("", false)] // empty
    fn is_absolute_normalised_accepts_and_rejects(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(
            is_absolute_normalised(input),
            expected,
            "is_absolute_normalised({input:?})"
        );
    }

    // ---------------------------------------------------------------------
    // resolve_repo_path
    // ---------------------------------------------------------------------

    /// A normal `a/b` rel round-trips: the components are pushed onto the base.
    /// Asserted as a HARD LITERAL (review R16) — `/repo/base` is case-free, so it
    /// normalises identically on both hosts.
    #[test]
    fn resolve_repo_path_normal_join() {
        let got = resolve_repo_path("/repo/base", "a/b");
        assert_eq!(got, PathBuf::from("/repo/base/a/b"));
    }

    /// Security invariant: a leading-`..`-escaping `rel` is clamped INSIDE the
    /// base — every leading `..` is a no-op at the base floor (depth 0), so only
    /// the surviving `Normal` components join, never an ancestor of the base.
    #[test]
    fn resolve_repo_path_clamps_parent_escape() {
        let got = resolve_repo_path("/repo/base", "../../etc/passwd");
        // The leading `..` are clamped (no-ops at depth 0); only `etc/passwd`
        // survives, joined onto the base. HARD LITERAL (review R16).
        assert_eq!(
            got,
            PathBuf::from("/repo/base/etc/passwd"),
            "`..` must be clamped within the base"
        );
        // And the result is strictly within the base.
        assert!(
            got.starts_with("/repo/base"),
            "clamped path must stay under the base: {got:?}"
        );
    }

    /// A `..` LEXICALLY CANCELS the preceding pushed component (review R5):
    /// `a/../b` resolves to `base/b`, NOT `base/a/b`.
    #[test]
    fn resolve_repo_path_cancels_parent_dir() {
        let got = resolve_repo_path("/repo/base", "a/../b");
        assert_eq!(
            got,
            PathBuf::from("/repo/base/b"),
            "`..` must cancel the preceding `a`, yielding base/b"
        );
    }

    /// `..` cancellation is CLAMPED at the base: a `rel` that cancels past the
    /// pushed depth (`../../etc` after a single `a`) never pops INTO the base —
    /// the surplus `..` are no-ops at depth 0 (review R5). `a/../../etc` →
    /// `base/etc` (push a, cancel a → depth 0, surplus `..` no-op, push etc).
    #[test]
    fn resolve_repo_path_parent_cancellation_clamped_at_base() {
        let got = resolve_repo_path("/repo/base", "a/../../etc");
        assert_eq!(
            got,
            PathBuf::from("/repo/base/etc"),
            "cancellation must clamp at the base floor: {got:?}"
        );
        assert!(
            got.starts_with("/repo/base"),
            "clamped path must stay under the base: {got:?}"
        );
    }

    /// An ABSOLUTE `rel` does NOT replace the base — its root anchor is ignored
    /// and only the `Normal` parts join onto the base (the naïve `PathBuf::join`
    /// replace-bug this guards against). Covers a unix-absolute `/etc/passwd`.
    /// HARD LITERAL (review R16) — `/repo/base` is case-free.
    #[test]
    fn resolve_repo_path_unix_absolute_rel_is_ignored() {
        let got = resolve_repo_path("/repo/base", "/etc/passwd");
        assert_eq!(
            got,
            PathBuf::from("/repo/base/etc/passwd"),
            "an absolute `rel` must NOT replace the base — only Normal parts join"
        );
        assert!(
            got.starts_with("/repo/base"),
            "absolute-rel result must stay under the base: {got:?}"
        );
    }

    /// A drive-anchored / UNC absolute `rel` likewise does not replace the base:
    /// the `Prefix`/`RootDir` anchors are dropped and only the `Normal`
    /// components survive. (On Unix, `Path::components` of `C:/x` yields a single
    /// `Normal("C:")` part — still a clamp, never an escape — so we assert the
    /// result stays under the base rather than pinning exact components.)
    #[test]
    fn resolve_repo_path_drive_absolute_rel_is_clamped_under_base() {
        let base = normalise_path_for_compare("/repo/base");
        let got = resolve_repo_path("/repo/base", r"D:\windows\system32");
        assert!(
            got.starts_with(&base),
            "a drive-absolute `rel` must stay clamped under the base: {got:?}"
        );
        assert_ne!(
            got,
            PathBuf::from("D:/windows/system32"),
            "the drive-absolute `rel` must NOT have replaced the base"
        );
    }

    // ---------------------------------------------------------------------
    // select_longest_prefix_project
    // ---------------------------------------------------------------------

    /// Nested clone dirs: the DEEPER (longer) prefix wins.
    #[test]
    fn select_longest_prefix_project_deeper_wins() {
        let candidates = [
            ("outer".to_string(), "/dev/mono".to_string()),
            ("inner".to_string(), "/dev/mono/pkg/sub".to_string()),
        ];
        assert_eq!(
            select_longest_prefix_project("/dev/mono/pkg/sub/src/lib.rs", &candidates),
            Some("inner".to_string()),
            "the deeper clone-dir prefix is the most specific match"
        );
        // A cwd inside the outer-but-not-inner subtree resolves to the outer.
        assert_eq!(
            select_longest_prefix_project("/dev/mono/other/x", &candidates),
            Some("outer".to_string()),
        );
    }

    /// A filesystem-ROOT base (`/`) matches a cwd that starts with it — the root
    /// already carries its trailing `/`, so the boundary test must accept it
    /// (review R1). And a DEEPER non-root base still beats the root by
    /// longest-prefix.
    #[test]
    fn select_longest_prefix_project_root_base_matches() {
        // A bare root base `/` matches cwd `/foo`.
        let root_only = [("root".to_string(), "/".to_string())];
        assert_eq!(
            select_longest_prefix_project("/foo", &root_only),
            Some("root".to_string()),
            "a root base `/` must match a cwd that starts with it"
        );

        // A deeper `/dev/foo` still beats the root base `/` for cwd `/dev/foo/x`.
        let root_and_deep = [
            ("root".to_string(), "/".to_string()),
            ("deep".to_string(), "/dev/foo".to_string()),
        ];
        assert_eq!(
            select_longest_prefix_project("/dev/foo/x", &root_and_deep),
            Some("deep".to_string()),
            "a deeper non-root base must win over the root base (longest-prefix)"
        );
    }

    /// An EXACT match (cwd == local_path) resolves to that project.
    #[test]
    fn select_longest_prefix_project_exact_match() {
        let candidates = [("p".to_string(), "/dev/foo".to_string())];
        assert_eq!(
            select_longest_prefix_project("/dev/foo", &candidates),
            Some("p".to_string()),
        );
    }

    /// Component-boundary discipline: a cwd `/dev/foobar` does NOT match a
    /// `local_path` `/dev/foo` (the match must land on a `/` boundary).
    #[test]
    fn select_longest_prefix_project_component_boundary_non_match() {
        let candidates = [("p".to_string(), "/dev/foo".to_string())];
        assert_eq!(
            select_longest_prefix_project("/dev/foobar/x", &candidates),
            None,
            "a non-component-boundary prefix must NOT match"
        );
    }

    /// No candidate matches ⇒ `None`.
    #[test]
    fn select_longest_prefix_project_no_match_is_none() {
        let candidates = [("p".to_string(), "/dev/foo".to_string())];
        assert_eq!(
            select_longest_prefix_project("/elsewhere/x", &candidates),
            None,
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

    /// The SAME project_id appearing twice at the longest length is NOT a tie —
    /// it resolves to that one project (the dedup branch in the longest set).
    #[test]
    fn select_longest_prefix_project_same_project_twice_resolves() {
        let candidates = [
            ("p".to_string(), "/dev/foo".to_string()),
            ("p".to_string(), "/dev/foo".to_string()),
        ];
        assert_eq!(
            select_longest_prefix_project("/dev/foo/x", &candidates),
            Some("p".to_string()),
            "the same project at the longest length is not a tie"
        );
    }

    // =====================================================================
    // DB-backed tests (T5) — open `connect_in_memory`, seed a project +
    // repo_link via the established `repo::*` helpers (reached via `super::*`).
    // =====================================================================

    /// Seed a bare `project` work-item and a single `repo_link` under it, returning
    /// `(pool, project_id, repo_link_id)`. Uses the public `create_work_item` /
    /// `add_repo_link` mutators (the same path the e2e `repo_links_flow` thread drives).
    async fn seed_project_with_repo_link(
    ) -> (sqlx::SqlitePool, String, String) {
        let pool = connect_in_memory().await.expect("migrated in-memory pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let repo_link = add_repo_link(&pool, &project, "octocat/hello-world", true)
            .await
            .expect("repo link")
            .to_string();
        (pool, project, repo_link)
    }

    /// `set_repo_local_path(Some(..))` sets the column to the STRUCTURAL
    /// normalised value (separators folded, casing PRESERVED — review R7) and
    /// reads back; the e2e relies on this storing the normalised (not raw) form.
    #[tokio::test]
    async fn set_repo_local_path_sets_normalised_value() {
        let (pool, _project, repo_link) = seed_project_with_repo_link().await;

        // A raw backslash-form, drive-anchored path. The STORED value is the
        // STRUCTURAL form (sep-folded, casing PRESERVED), which is exactly what
        // `normalise_path_structural` produces — NOT the compare form (no
        // host-keyed case fold at store time, so the operator's casing survives).
        let raw = r"C:\dev\hello-world";
        set_repo_local_path(&pool, &repo_link, Some(raw))
            .await
            .expect("set local_path");

        let stored: Option<String> =
            sqlx::query_scalar("SELECT local_path FROM repo_links WHERE id = ?1")
                .bind(&repo_link)
                .fetch_one(&pool)
                .await
                .expect("read back local_path");
        assert_eq!(
            stored.as_deref(),
            Some(normalise_path_structural(raw).as_str()),
            "the STORED value is the STRUCTURAL (casing-preserved) form of the raw input"
        );
    }

    /// Surrounding whitespace is TRIMMED before normalise/validate/store, so a
    /// value like ` /dev/foo ` is ACCEPTED and stored without the padding (review
    /// R13).
    #[tokio::test]
    async fn set_repo_local_path_trims_whitespace() {
        let (pool, _project, repo_link) = seed_project_with_repo_link().await;

        set_repo_local_path(&pool, &repo_link, Some("  /dev/foo  "))
            .await
            .expect("padded-but-absolute path must be accepted after trim");

        let stored: Option<String> =
            sqlx::query_scalar("SELECT local_path FROM repo_links WHERE id = ?1")
                .bind(&repo_link)
                .fetch_one(&pool)
                .await
                .expect("read back local_path");
        assert_eq!(
            stored.as_deref(),
            Some("/dev/foo"),
            "the stored value is trimmed of surrounding whitespace"
        );
    }

    /// `set_repo_local_path(None)` clears the column back to NULL.
    #[tokio::test]
    async fn set_repo_local_path_none_clears_to_null() {
        let (pool, _project, repo_link) = seed_project_with_repo_link().await;

        // Set then clear.
        set_repo_local_path(&pool, &repo_link, Some("/dev/x"))
            .await
            .expect("set");
        set_repo_local_path(&pool, &repo_link, None)
            .await
            .expect("clear");

        let stored: Option<String> =
            sqlx::query_scalar("SELECT local_path FROM repo_links WHERE id = ?1")
                .bind(&repo_link)
                .fetch_one(&pool)
                .await
                .expect("read back local_path");
        assert_eq!(stored, None, "None clears local_path back to NULL");
    }

    /// An absent `repo_link_id` is `NotFound` BEFORE any write (the owning-project
    /// resolve runs first) — and emits ZERO events (review R18).
    #[tokio::test]
    async fn set_repo_local_path_absent_id_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let absent = "00000000-0000-0000-0000-000000000000";
        let err = set_repo_local_path(&pool, absent, Some("/x"))
            .await
            .expect_err("absent id must be NotFound");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");

        // No event was recorded on any aggregate — the resolve fails before the
        // tx opens, so nothing is written. The absent id is also (vacuously) not
        // an event aggregate, so a count over it is 0 (review R18).
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE event_type = 'repo_link.local_path_changed'",
        )
        .fetch_one(&pool)
        .await
        .expect("count events");
        assert_eq!(
            total, 0,
            "a NotFound set must record no local_path_changed event"
        );
    }

    /// A RELATIVE path (normalises to a non-absolute form) is rejected with
    /// `Validation` and writes nothing.
    #[tokio::test]
    async fn set_repo_local_path_relative_is_validation() {
        let (pool, _project, repo_link) = seed_project_with_repo_link().await;

        // `dev/foo` (no leading `/` and no drive anchor) normalises to `dev/foo`
        // which is NOT absolute ⇒ Validation.
        let err = set_repo_local_path(&pool, &repo_link, Some(r"dev\foo"))
            .await
            .expect_err("relative path must be rejected");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Nothing was stored.
        let stored: Option<String> =
            sqlx::query_scalar("SELECT local_path FROM repo_links WHERE id = ?1")
                .bind(&repo_link)
                .fetch_one(&pool)
                .await
                .expect("read back local_path");
        assert_eq!(stored, None, "the rejected set wrote no local_path");
    }

    /// EXACTLY ONE `repo_link.local_path_changed` event fires on the owning
    /// PROJECT aggregate per successful set — AND its payload carries the
    /// expected `id` / `project_id` / `local_path` (the normalised STORED value)
    /// (review R18).
    #[tokio::test]
    async fn set_repo_local_path_emits_exactly_one_event() {
        let (pool, project, repo_link) = seed_project_with_repo_link().await;

        let raw = "/dev/hello-world";
        set_repo_local_path(&pool, &repo_link, Some(raw))
            .await
            .expect("set local_path");

        let n = count_events_for(&pool, &project, "repo_link.local_path_changed").await;
        assert_eq!(
            n, 1,
            "exactly one local_path_changed event on the project aggregate"
        );

        // Read the event row and assert its JSON payload (review R18). The
        // `payload` column is the JSON-serialised object record_event wrote; the
        // `local_path` field MUST be the normalised STORED form (structural).
        let payload_str: String = sqlx::query_scalar(
            "SELECT payload FROM events \
             WHERE aggregate_id = ?1 AND event_type = 'repo_link.local_path_changed'",
        )
        .bind(&project)
        .fetch_one(&pool)
        .await
        .expect("read event payload");
        let payload: serde_json::Value =
            serde_json::from_str(&payload_str).expect("payload is valid JSON");

        assert_eq!(
            payload["id"].as_str(),
            Some(repo_link.as_str()),
            "payload.id is the repo_link id"
        );
        assert_eq!(
            payload["project_id"].as_str(),
            Some(project.as_str()),
            "payload.project_id is the owning project"
        );
        assert_eq!(
            payload["local_path"].as_str(),
            Some(normalise_path_structural(raw).as_str()),
            "payload.local_path is the normalised STORED value"
        );
    }

    /// `resolve_cwd_to_project` end-to-end against a small seeded DB:
    ///   * a NULL-`local_path` row is EXCLUDED (the SQL filters `IS NOT NULL`);
    ///   * the longest-prefix project resolves correctly.
    #[tokio::test]
    async fn resolve_cwd_to_project_excludes_null_and_resolves_longest() {
        let pool = connect_in_memory().await.expect("pool");

        // Project A: a clone dir at /dev/mono.
        let proj_a = create_work_item(&pool, "project", None, "A", None)
            .await
            .expect("project A")
            .to_string();
        let link_a = add_repo_link(&pool, &proj_a, "a/outer", true)
            .await
            .expect("link A")
            .to_string();
        set_repo_local_path(&pool, &link_a, Some("/dev/mono"))
            .await
            .expect("set A local_path");

        // Project B: a DEEPER clone dir at /dev/mono/pkg.
        let proj_b = create_work_item(&pool, "project", None, "B", None)
            .await
            .expect("project B")
            .to_string();
        let link_b = add_repo_link(&pool, &proj_b, "b/inner", true)
            .await
            .expect("link B")
            .to_string();
        set_repo_local_path(&pool, &link_b, Some("/dev/mono/pkg"))
            .await
            .expect("set B local_path");

        // Project C: a repo_link with NO local_path (NULL) — must be excluded.
        let proj_c = create_work_item(&pool, "project", None, "C", None)
            .await
            .expect("project C")
            .to_string();
        add_repo_link(&pool, &proj_c, "c/null", true)
            .await
            .expect("link C (no local_path)");

        // A cwd under the deeper dir resolves to B (longest prefix).
        let got = resolve_cwd_to_project(&pool, "/dev/mono/pkg/src/lib.rs")
            .await
            .expect("resolve");
        assert_eq!(got, Some(proj_b.clone()), "deeper clone dir wins");

        // A cwd under the outer-only subtree resolves to A.
        let got = resolve_cwd_to_project(&pool, "/dev/mono/other/x")
            .await
            .expect("resolve");
        assert_eq!(got, Some(proj_a.clone()), "outer clone dir matches");

        // A cwd matching nothing resolves to None (and the NULL-local_path
        // project C is never a candidate).
        let got = resolve_cwd_to_project(&pool, "/elsewhere")
            .await
            .expect("resolve");
        assert_eq!(got, None, "no match (NULL-local_path project excluded)");
    }

    /// A SOFT-DELETED (tombstoned) project's clone dir is EXCLUDED from
    /// `resolve_cwd_to_project` (the `w.deleted_at IS NULL` JOIN guard): a cwd
    /// under it no longer resolves once the project is soft-deleted.
    #[tokio::test]
    async fn resolve_cwd_to_project_excludes_soft_deleted_project() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let link = add_repo_link(&pool, &project, "p/repo", true)
            .await
            .expect("link")
            .to_string();
        set_repo_local_path(&pool, &link, Some("/dev/tomb"))
            .await
            .expect("set local_path");

        // Before delete: the cwd resolves to the project.
        let got = resolve_cwd_to_project(&pool, "/dev/tomb/src")
            .await
            .expect("resolve before delete");
        assert_eq!(got, Some(project.clone()), "resolves before soft-delete");

        // Soft-delete the project (stamps deleted_at via the single-mutation path).
        delete_work_item(&pool, &project)
            .await
            .expect("soft-delete project");

        // After delete: the same cwd no longer resolves (tombstone excluded).
        let got = resolve_cwd_to_project(&pool, "/dev/tomb/src")
            .await
            .expect("resolve after delete");
        assert_eq!(
            got, None,
            "a soft-deleted project's clone dir is excluded from cwd resolution"
        );
    }
}
