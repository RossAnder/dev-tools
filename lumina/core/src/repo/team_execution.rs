//! Team-execution work-queue (migration 0013, plan §C/§D, R5 carve): the
//! atomic `claim_next_task` primitive, its lease-lifecycle companions
//! `release_task` / `renew_lease`, and the `complete_task` done→review CASCADE
//! (the documented COMPOSER exception to the per-mutator single-tx rule). The
//! private claim-scan row decoders (`ClaimCandidateRow`, `OverlapScanRow`), the
//! `files_touched_*` helpers, and the `CompleteTaskResult` output struct move
//! with them.
//!
//! `pub use team_execution::*` in `repo/mod.rs` PRESERVES the public surface —
//! every `pub` fn here + `CompleteTaskResult` stays reachable at its existing
//! `crate::repo::*` path. `create_work_item_full_tx` / `CreateOpts` / `enum_to_str`
//! (in `shared.rs`/`work_items.rs`) and `update_work_item_status` (in
//! `work_items.rs`) are reached via `use super::*`.

use super::*;
use super::events::{record_event, record_inert_event};
use crate::args;
use crate::db::DbClient;
use crate::domain::{ClaimedTask, FileOverlapWarning, Lane, Tier};
use crate::error::AppError;
use serde_json::Value;

// ---------------------------------------------------------------------------
// claim_next_task (team-execution migration 0013; sprint-status + checkpoint
// guards tightened in migration 0016, plan §C). The atomic work-queue claim
// primitive: one BEGIN IMMEDIATE txn does lazy-reclaim → sprint-status guard →
// checkpoint-freeze guard → candidate select → lease, then a cheap post-commit
// read computes the advisory file-overlap report. Race-safe under SQLite's
// single writer (the SELECT→UPDATE share one RESERVED-locked txn).
// ---------------------------------------------------------------------------

/// Read a task's CANONICAL `(repo_link_id, path)` overlap keys from the
/// first-class `task_files` EXPECTED set (migration 0020, T7) — the re-keyed
/// replacement for the old raw-slug `attributes.files_touched` parse.
///
/// **Why canonical, why `task_files`.** The stored EXPECTED rows are ALREADY in
/// canonical form: `set_task_expected_files` runs every entry through
/// [`crate::repo::canonical_file_key`] before insert, which collapses an explicit
/// PRIMARY-repo slug to the NULL `repo_link_id` bucket (matching the storage
/// `COALESCE(repo_link_id,'')` UNIQUE index — Ground R2). So a bare path and an
/// explicit-primary `{repo, path}` for the SAME primary repo are stored as the
/// SAME `(None, path)` key and no longer FALSE-DISTINGUISH, while genuinely
/// different repos keep a distinct `Some(repo_link_id)` and never FALSE-overlap.
/// Reading the keys straight from `task_files` means the scan needs NO per-task
/// project-ancestor + repo_links resolution — the canonicalisation already
/// happened at write time, so this stays a cheap post-commit read (a single
/// indexed SELECT per scanned task, `idx_task_files_task(task_id, kind)`), in
/// keeping with the advisory's cheap-read discipline (ADR-0002: the overlap is
/// advisory, never a gate).
///
/// Returns the de-duplicated set of canonical keys (a `TaskFile` row already
/// dedups at storage, but collecting into a `BTreeSet` gives the caller a ready
/// set for intersection). An absent EXPECTED set yields an empty set — no
/// caution, never an error.
async fn task_expected_overlap_keys(
    db: &impl DbClient,
    task_id: &str,
) -> Result<std::collections::BTreeSet<(Option<String>, String)>, AppError> {
    let rows = crate::repo::list_task_files(db, task_id, Some("expected")).await?;
    Ok(rows.into_iter().map(|f| (f.repo_link_id, f.path)).collect())
}

/// Extract the `files_touched` array from a task's stored `attributes` TEXT
/// blob. Absent / NULL attributes, a non-object root, or a missing/non-array
/// `files_touched` key all yield an empty vec (best-effort — a malformed blob
/// produces no overlap caution rather than an error; `decode_attributes` is
/// the authoritative corruption detector elsewhere). Returns the RAW JSON
/// entries (bare strings or `{repo,path}` objects) so they flow into
/// `ClaimedTask.files_touched` verbatim.
fn files_touched_from_attributes(attributes: Option<&str>) -> Vec<Value> {
    match attributes {
        None => Vec::new(),
        Some(raw) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|v| {
                v.get("files_touched")
                    .and_then(Value::as_array)
                    .map(|a| a.to_vec())
            })
            .unwrap_or_default(),
    }
}

/// Claim the next ready task in a sprint by `(lane, tier)` under a lease — the
/// core team-execution queue primitive (plan §C). The whole claim runs in ONE
/// `BEGIN IMMEDIATE` transaction so the SELECT→UPDATE is race-free under
/// SQLite's single writer (the property the agent-teams shared list cannot
/// give); the advisory file-overlap report is computed as a cheap read AFTER
/// the commit, so no `files_touched` JSON parse runs under the writer lock.
///
/// Steps (all but the last inside the txn):
///   1. **Lazy reclaim** — expired leases (`status='in_progress'` AND
///      `lease_expires_at < now`) on this sprint's tasks are reset to `todo`
///      / `assignee=NULL`; if any rows were reclaimed, ONE coarse,
///      export-INERT `leases.reclaimed` event is recorded (mirrors the
///      migration-0011 Part-B coarse-event idiom — `aggregate_type="sprint"`,
///      never `"work_item"`). Zero reclaimed ⇒ no event.
///   2. **Sprint-status guard** — `Ok(None)` unless the sprint's status is
///      exactly `'active'` (migration-0016 layer-2 rule: a sprint's tasks are
///      claimable ⟺ the sprint is `active`; `draft`/`ready`/`review`/terminal
///      states are all non-runnable). A missing sprint is likewise `Ok(None)`.
///      2b. **Checkpoint-freeze guard** — `Ok(None)` while ANY checkpoint task
///      (`work_items.checkpoint = 1`, migration 0016) in the sprint is
///      `in_progress`: a checkpoint freezes its whole sprint (a sprint-wide
///      barrier) until that checkpoint task leaves `in_progress`.
///   3. **Candidate select** — the first ready task (status=`todo`, unleased,
///      matching lane + optional tier, not blocked on a question, live, with
///      every task-dependency `done`), ordered by the `compute_task_batches`
///      tie-break (`task_kind` sort, `created_at`, `id`). NO file-overlap
///      filtering (overlap is advisory). No candidate ⇒ `Ok(None)`.
///   4. **Lease** — stamp `status='in_progress'`, `assignee`, and
///      `lease_expires_at = now + lease_ttl_secs`; record ONE export-eligible
///      `work_item.claimed` event. Commit.
///   5. **Advisory overlap (post-commit)** — for every OTHER `in_progress`
///      task in the sprint sharing ≥1 `files_touched` key with the claimed
///      task, a [`FileOverlapWarning`] is attached. The claim is NEVER
///      rejected on overlap (ADR-0002).
///
/// `lease_ttl_secs` is seconds added to `now` for the new lease deadline;
/// both `now` and `now + ttl` are computed by SQLite's `datetime(...)` so the
/// stored `lease_expires_at` shares the `CURRENT_TIMESTAMP` format
/// (`YYYY-MM-DD HH:MM:SS`, UTC) and the `<`/`>` comparisons are lexical.
pub async fn claim_next_task(
    db: &impl DbClient,
    sprint_id: &str,
    lane: Lane,
    tier: Option<Tier>,
    agent_id: &str,
    lease_ttl_secs: i64,
) -> Result<Option<ClaimedTask>, AppError> {
    let lane_str = enum_to_str(lane);
    let tier_str: Option<String> = tier.map(enum_to_str);

    let mut tx = db.begin().await?;

    // --- Step 1: lazy reclaim expired leases scoped to this sprint. ---------
    // A past `lease_expires_at` on an `in_progress` task whose id is bound to
    // this sprint via `sprint_tasks` is reset to a reclaimable `todo`. Using
    // `datetime('now')` keeps the comparison in the CURRENT_TIMESTAMP format.
    let reclaimed = tx
        .execute(
            r#"
        UPDATE work_items
        SET status = 'todo', assignee = NULL, lease_expires_at = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE status = 'in_progress'
          AND lease_expires_at IS NOT NULL
          AND lease_expires_at < datetime('now')
          AND id IN (SELECT task_id FROM sprint_tasks WHERE sprint_id = $1)
        "#,
            args![sprint_id.to_owned()],
        )
        .await?;

    if reclaimed > 0 {
        // ONE coarse, export-INERT event for the whole reclaim batch (the
        // precedented exception to the per-row +1-event rule, mirroring the
        // migration-0011 Part-B coarse events). `aggregate_type="sprint"`, so
        // the git-export drain (which materialises only `"work_item"` events)
        // ignores it — reclaimed rows are not re-exported individually here.
        let payload = serde_json::json!({ "reclaimed": reclaimed, "sprint_id": sprint_id });
        record_inert_event(tx.as_mut(), "sprint", sprint_id, "leases.reclaimed", payload).await?;
    }

    // --- Step 2: sprint-status guard. --------------------------------------
    // A missing sprint OR a non-`active` status ⇒ Ok(None) (migration-0016
    // layer-2 rule: a sprint's tasks are claimable ⟺ the sprint is `active`;
    // `draft`/`ready`/`review`/`done`/`cancelled` are all non-runnable). The
    // lazy-reclaim above still committed if it fired (a sprint may legitimately
    // be reclaimed and then found non-runnable); commit the reclaim and return
    // None. NB this is the SPRINT-status guard — wholly separate from the
    // TASK-readiness predicate (`status IN ('todo','open')`) at Step 3/4.
    let sprint_status: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT status FROM sprints WHERE id = $1",
        args![sprint_id.to_owned()],
    )
    .await?;
    let runnable = sprint_status.as_deref() == Some("active");
    if !runnable {
        tx.commit().await?;
        return Ok(None);
    }

    // --- Step 2b: checkpoint-freeze guard (sprint-wide barrier). -----------
    // A checkpoint task (`work_items.checkpoint = 1`, migration 0016) freezes
    // its WHOLE sprint while it runs: as long as ANY live checkpoint task bound
    // to this sprint is `in_progress`, the claim yields Ok(None) so nothing else
    // is dispatched until the barrier clears. Like the status guard, this is a
    // pre-candidate-select gate that returns None (the reclaim, if any, still
    // commits). The freeze lifts the moment that checkpoint task leaves
    // `in_progress` (e.g. transitions to `done`).
    let frozen: Option<i64> = crate::db::tx_scalar_opt::<i64>(
        tx.as_mut(),
        r#"
        SELECT 1
        FROM sprint_tasks st
        JOIN work_items c ON c.id = st.task_id
        WHERE st.sprint_id = $1
          AND c.checkpoint = 1
          AND c.status = 'in_progress'
          AND c.deleted_at IS NULL
        LIMIT 1
        "#,
        args![sprint_id.to_owned()],
    )
    .await?;
    if frozen.is_some() {
        tx.commit().await?;
        return Ok(None);
    }

    // --- Step 3: candidate select (first ready wins, LIMIT 16). ------------
    // Ready ≡ not-started + unleased + matching lane + (tier unconstrained when
    // the caller passes None) + not blocked on a question + live + every
    // task-dependency `done`. The "not-started" set is `status IN ('todo',
    // 'open')`: `create_work_item` stamps the create-default `status='open'`
    // (and the `work_items.status` column DEFAULT is 'open'), so EVERY
    // freshly-created task — most importantly the review task spawned by
    // `complete_task` (T6) and the rework task spawned by
    // `record_finding_decision` (T8), both created via the create path — starts
    // at 'open'. A 'todo'-only predicate would render those spawned tasks
    // invisible and SILENTLY break the entire review→rework cascade.
    // `block_task_on_question` (repo.rs:4299) sets the same precedent, treating
    // `"todo" | "open"` as the equivalent "ready, not started" precondition (its
    // branch-resolution restores blocked tasks to 'todo', which is in this set).
    // `lane IS NOT NULL` is implied by `lane = $2`
    // (a legacy `lane IS NULL` task can never match a non-null bound value),
    // so back-compat (lane=NULL tasks invisible) falls out for free. The
    // ORDER BY mirrors `compute_task_batches`' intra-phase tie-break: the
    // `task_kind` sort weight (foundation<main/NULL<polish), then created_at,
    // then id. The `:tier IS NULL OR tier = :tier` shape uses a NULL sentinel
    // bind so one prepared statement covers both the any-tier and exact-tier
    // cases.
    let candidate = crate::db::tx_query_opt::<ClaimCandidateRow>(
        tx.as_mut(),
        r#"
        SELECT t.id, t.tier
        FROM work_items t
        JOIN sprint_tasks st ON st.task_id = t.id AND st.sprint_id = $1
        WHERE t.status IN ('todo', 'open')
          AND t.assignee IS NULL
          AND t.lane = $2
          AND ($3 IS NULL OR t.tier = $3)
          AND t.blocked_by_question_id IS NULL
          AND t.deleted_at IS NULL
          AND NOT EXISTS (
              SELECT 1 FROM task_dependencies d
              JOIN work_items dep ON dep.id = d.depends_on_id
              WHERE d.task_id = t.id AND dep.status <> 'done'
          )
        ORDER BY
          CASE t.task_kind
            WHEN 'foundation' THEN 0
            WHEN 'polish' THEN 2
            ELSE 1
          END,
          t.created_at,
          t.id
        LIMIT 16
        "#,
        args![sprint_id.to_owned(), lane_str.clone(), tier_str.clone()],
    )
    .await?;

    let Some(row) = candidate else {
        // No ready candidate — commit (the reclaim, if any, must persist) and
        // signal "nothing to claim" with Ok(None). No claim event.
        tx.commit().await?;
        return Ok(None);
    };
    let task_id = row.id;
    let claimed_tier_str = row.tier;

    // --- Step 4: lease the winning candidate + one claim event. ------------
    // The new lease deadline is `now + lease_ttl_secs`, computed by SQLite so
    // it shares the stored-timestamp format. The WHERE re-asserts the
    // not-started/unleased predicate (defence-in-depth; the SELECT and UPDATE
    // already share one writer-locked txn so no concurrent claimer can
    // interleave). The status guard MUST mirror the step-3 readiness set
    // (`IN ('todo','open')`) — otherwise an 'open'-status candidate (the create
    // default for every spawned review/rework task) would be selected but match
    // 0 rows here and the claim would spuriously bail.
    let ttl_modifier = format!("+{lease_ttl_secs} seconds");
    let leased = tx
        .execute(
            r#"
        UPDATE work_items
        SET status = 'in_progress',
            assignee = $2,
            lease_expires_at = datetime('now', $3),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1 AND status IN ('todo', 'open') AND assignee IS NULL
        "#,
            args![task_id.clone(), agent_id.to_owned(), ttl_modifier],
        )
        .await?;
    if leased == 0 {
        // Should be unreachable inside the single writer txn; treat as
        // "lost the race" → no claim, roll back via drop, surface None.
        return Ok(None);
    }

    // Read back the just-stamped lease deadline so the result carries the exact
    // stored value (rather than recomputing `now` in Rust and risking a
    // sub-second skew with the DB clock).
    let lease_expires_at: String = crate::db::tx_scalar_one::<String>(
        tx.as_mut(),
        "SELECT lease_expires_at FROM work_items WHERE id = $1",
        args![task_id.clone()],
    )
    .await?;

    let claim_payload = serde_json::json!({
        "assignee": agent_id,
        "lane": lane_str,
        "lease_expires_at": lease_expires_at,
        "sprint_id": sprint_id,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        &task_id,
        "work_item.claimed",
        claim_payload,
    )
    .await?;

    tx.commit().await?;

    // --- Step 5: advisory file-overlap report (POST-commit; cheap read). ---
    // Per ADR-0002 the claim NEVER skips on overlap. CRUCIAL: this all runs
    // OUTSIDE the write txn so it never holds the writer lock.
    //
    // `ClaimedTask.files_touched` keeps its existing contract: the RAW
    // best-effort `attributes.files_touched` entries (bare strings / {repo,path}
    // objects) verbatim, so downstream consumers see the same shape as before.
    let claimed_attrs: Option<String> = crate::db::scalar_opt::<String>(
        db,
        "SELECT attributes FROM work_items WHERE id = $1",
        args![task_id.clone()],
    )
    .await?;
    let files_touched = files_touched_from_attributes(claimed_attrs.as_deref());

    // The OVERLAP scan (T7) is re-keyed onto the CANONICAL `(repo_link_id, path)`
    // form: read the claimed task's EXPECTED `task_files` keys, then scan the
    // OTHER in_progress tasks in this sprint and report any that share ≥1
    // canonical key. Because the stored keys are already canonical (NULL ≡
    // primary), a bare path and an explicit-primary `{repo, path}` for the same
    // primary repo correctly OVERLAP (no false-negative), and genuinely
    // different repos stay distinct (no false-positive). Each scanned task costs
    // one cheap indexed `list_task_files` read.
    let claimed_keys = task_expected_overlap_keys(db, &task_id).await?;

    let mut file_overlap_warnings: Vec<FileOverlapWarning> = Vec::new();
    if !claimed_keys.is_empty() {
        // Other in_progress tasks in the same sprint, excluding the just-claimed
        // one. We only need their ids; the canonical keys are read per task from
        // `task_files` below.
        let others = db
            .query_all::<OverlapScanRow>(
                r#"
                SELECT t.id
                FROM work_items t
                JOIN sprint_tasks st ON st.task_id = t.id AND st.sprint_id = $1
                WHERE t.status = 'in_progress'
                  AND t.id <> $2
                  AND t.deleted_at IS NULL
                ORDER BY t.created_at, t.id
                "#,
                args![sprint_id.to_owned(), task_id.clone()],
            )
            .await?;

        for other in others {
            let other_keys = task_expected_overlap_keys(db, &other.id).await?;
            // The advisory `shared` list reports the PATH segment of each shared
            // canonical key (the human-meaningful piece).
            let mut shared: Vec<String> = other_keys
                .intersection(&claimed_keys)
                .map(|(_, path)| path.clone())
                .collect();
            if !shared.is_empty() {
                shared.sort();
                shared.dedup();
                file_overlap_warnings.push(FileOverlapWarning {
                    task_id: other.id,
                    shared,
                });
            }
        }
    }

    // Re-type the claimed tier string back into the typed enum for the result.
    let claimed_tier: Option<Tier> = match claimed_tier_str {
        Some(s) => Some(
            serde_json::from_value::<Tier>(Value::String(s))
                .map_err(|e| AppError::Other(e.into()))?,
        ),
        None => None,
    };

    Ok(Some(ClaimedTask {
        task_id,
        lane,
        tier: claimed_tier,
        assignee: agent_id.to_owned(),
        lease_expires_at,
        files_touched,
        file_overlap_warnings,
    }))
}

/// Raw row read by the candidate SELECT in [`claim_next_task`]: the winning
/// task's id + its `tier` column (re-typed to [`Tier`] for the result).
/// `tier` is nullable. Generic over `R: Row` per the canonical [`crate::db`]
/// FromRow recipe.
#[derive(Debug)]
struct ClaimCandidateRow {
    id: String,
    tier: Option<String>,
}

impl<'r, R> sqlx::FromRow<'r, R> for ClaimCandidateRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ClaimCandidateRow {
            id: row.try_get("id")?,
            tier: row.try_get("tier")?,
        })
    }
}

/// Raw row read by the post-commit file-overlap scan in [`claim_next_task`]:
/// an in-progress sprint task's id. T7 re-keyed the scan onto the first-class
/// `task_files` EXPECTED set, so the per-task canonical `(repo_link_id, path)`
/// keys are read via [`task_expected_overlap_keys`] (not parsed from the
/// `attributes` blob) — this row therefore carries only the `id`.
/// Generic over `R: Row` per the canonical [`crate::db`] FromRow recipe.
#[derive(Debug)]
struct OverlapScanRow {
    id: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for OverlapScanRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(OverlapScanRow {
            id: row.try_get("id")?,
        })
    }
}

// ---------------------------------------------------------------------------
// get_checkpoint_suggestions (1B-F8): cross-task EXPECTED files-overlap →
// checkpoint candidates. A read-only sibling of the advisory claim-overlap scan
// above — same canonical `(repo_link_id, path)` keys via
// `task_expected_overlap_keys`, but computed pairwise across a story's (or
// sprint's) WHOLE task set so the compose-sprint operator can stamp checkpoints
// BEFORE the sprint runs. Read-only: no tx, no event.
// ---------------------------------------------------------------------------

/// A task's canonical EXPECTED overlap keys (`(repo_link_id, path)`; NULL ≡
/// primary repo) — the unit of the cross-task intersection. Aliased so the
/// per-task scan map stays under `clippy::type_complexity`.
type ExpectedKeys = std::collections::BTreeSet<(Option<String>, String)>;

/// One OTHER task a checkpoint candidate shares EXPECTED files with, plus the
/// shared paths. Mirrors [`crate::domain::FileOverlapWarning`]'s shape — the
/// PATH segment of each shared canonical key is the human-meaningful piece the
/// operator reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CheckpointOverlap {
    /// The other task whose EXPECTED set intersects this candidate's.
    pub task_id: String,
    /// The shared PATHs (sorted + deduped), one per shared `(repo_link_id, path)`
    /// canonical key.
    pub shared_paths: Vec<String>,
}

/// A CHECKPOINT CANDIDATE: a task whose first-class EXPECTED `task_files` set
/// intersects ≥1 OTHER task's EXPECTED set in the same story/sprint scope. Two
/// tasks planning to touch the same file want a consolidated commit (a
/// checkpoint) rather than racing on the shared sprint worktree — this is the
/// server-side signal `compose-sprint` surfaces, lets the operator override, and
/// stamps via `set_task_checkpoint`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CheckpointSuggestion {
    /// The candidate task id.
    pub task_id: String,
    /// The overlapping peers + their shared paths (ordered by peer task id).
    pub overlaps: Vec<CheckpointOverlap>,
}

/// Compute checkpoint candidates over an explicit task-id set (the scope-shared
/// core of [`story_checkpoint_suggestions`] / [`sprint_checkpoint_suggestions`]).
///
/// **Why EXPECTED, why canonical.** The overlap runs on the first-class
/// `task_files` EXPECTED set — read once per task via [`task_expected_overlap_keys`]
/// (canonical `(repo_link_id, path)` keys; NULL ≡ primary, so a bare path and an
/// explicit-primary `{repo, path}` for the same repo correctly overlap and
/// genuinely-different repos stay distinct). EXPECTED is the ONLY set populated
/// at compose time — the ACTUAL set accrues during execution — so a pre-run
/// checkpoint suggestion can only see EXPECTED.
///
/// O(n²) pairwise over the scope's tasks (sprint-sized), each pair a cheap
/// `BTreeSet` intersection over keys already read once per task. A task with an
/// EMPTY expected set, or one overlapping nothing, is OMITTED. Candidates come
/// back ordered by the input task order (the SELECTs order by `created_at, id`),
/// each carrying its overlapping peers ordered by peer task id.
async fn checkpoint_suggestions_over(
    db: &impl DbClient,
    task_ids: &[String],
) -> Result<Vec<CheckpointSuggestion>, AppError> {
    // Read each task's canonical EXPECTED keys exactly once.
    let mut per_task: Vec<(String, ExpectedKeys)> = Vec::with_capacity(task_ids.len());
    for id in task_ids {
        per_task.push((id.clone(), task_expected_overlap_keys(db, id).await?));
    }

    let mut suggestions: Vec<CheckpointSuggestion> = Vec::new();
    for (i, (id_i, keys_i)) in per_task.iter().enumerate() {
        if keys_i.is_empty() {
            continue;
        }
        let mut overlaps: Vec<CheckpointOverlap> = Vec::new();
        for (j, (id_j, keys_j)) in per_task.iter().enumerate() {
            if i == j {
                continue;
            }
            let mut shared: Vec<String> = keys_i
                .intersection(keys_j)
                .map(|(_, path)| path.clone())
                .collect();
            if !shared.is_empty() {
                shared.sort();
                shared.dedup();
                overlaps.push(CheckpointOverlap {
                    task_id: id_j.clone(),
                    shared_paths: shared,
                });
            }
        }
        if !overlaps.is_empty() {
            // `overlaps` is already in `per_task` (task) order — a stable,
            // id-ordered peer list since the SELECTs order by `created_at, id`.
            suggestions.push(CheckpointSuggestion {
                task_id: id_i.clone(),
                overlaps,
            });
        }
    }
    Ok(suggestions)
}

/// Checkpoint candidates over a STORY's DIRECT task children — the cross-task
/// EXPECTED files-overlap suggestion, story-scoped. Mirrors the
/// `story_files_footprint` membership (`parent_id = $1 AND kind = 'task'`,
/// tombstones excluded). Read-only; an unknown/childless story yields `[]`.
pub async fn story_checkpoint_suggestions(
    db: &impl DbClient,
    story_id: &str,
) -> Result<Vec<CheckpointSuggestion>, AppError> {
    let rows = db
        .query_all::<OverlapScanRow>(
            r#"
            SELECT id
            FROM work_items
            WHERE parent_id = $1 AND kind = 'task' AND deleted_at IS NULL
            ORDER BY created_at, id
            "#,
            args![story_id.to_owned()],
        )
        .await?;
    let ids: Vec<String> = rows.into_iter().map(|r| r.id).collect();
    checkpoint_suggestions_over(db, &ids).await
}

/// Checkpoint candidates over a SPRINT's MEMBER tasks (the `sprint_tasks`
/// junction — the membership the sprint footprint reads) — the same cross-task
/// EXPECTED files-overlap suggestion, sprint-scoped. Read-only; an unknown/empty
/// sprint yields `[]`.
pub async fn sprint_checkpoint_suggestions(
    db: &impl DbClient,
    sprint_id: &str,
) -> Result<Vec<CheckpointSuggestion>, AppError> {
    let rows = db
        .query_all::<OverlapScanRow>(
            r#"
            SELECT t.id
            FROM work_items t
            JOIN sprint_tasks st ON st.task_id = t.id AND st.sprint_id = $1
            WHERE t.kind = 'task' AND t.deleted_at IS NULL
            ORDER BY t.created_at, t.id
            "#,
            args![sprint_id.to_owned()],
        )
        .await?;
    let ids: Vec<String> = rows.into_iter().map(|r| r.id).collect();
    checkpoint_suggestions_over(db, &ids).await
}

// ---------------------------------------------------------------------------
// release_task + renew_lease (team-execution migration 0013, plan §C). The
// lease-lifecycle companions to `claim_next_task`: `release_task` is the
// park-and-pull / voluntary-yield path; `renew_lease` is the heartbeat. Both
// are owner-guarded (`WHERE assignee = :agent_id`) so a non-owner — or a task
// whose lease was already reclaimed out from under the caller — is a clean
// no-op that mutates nothing and records no event. Each opens ONE
// `BEGIN IMMEDIATE` txn and writes +1 work_items / +1 events when (and only
// when) it actually mutates, mirroring `claim_next_task`.
// ---------------------------------------------------------------------------

/// Release a task the calling agent holds — clear its lease and (only if the
/// task is mid-execution) hand it back to the ready queue. Owner-guarded: the
/// `WHERE assignee = :agent_id` clause means a non-owner, a missing task, or a
/// task whose lease was already reclaimed mutates NOTHING and records no event,
/// returning `Ok(false)`.
///
/// Status semantics (plan §C): a single `CASE` makes `assignee`/
/// `lease_expires_at` clearing unconditional while flipping `status` to `todo`
/// ONLY when it is currently `in_progress`. A `blocked` task is deliberately
/// LEFT `blocked` — park-after-question requires that a task parked on an open
/// question stays invisible to the claim until the question resolves; resetting
/// it to `todo` here would make it spuriously claimable while its question is
/// still open. (Any other status — `done`/`cancelled` — is likewise left as-is;
/// only `in_progress` returns to the queue.)
///
/// Returns `Ok(true)` if the row was the caller's and was updated, `Ok(false)`
/// for the owner-guarded no-op. One `work_item.released` event on a true
/// mutation; none on the no-op.
pub async fn release_task(
    db: &impl DbClient,
    task_id: &str,
    agent_id: &str,
) -> Result<bool, AppError> {
    let mut tx = db.begin().await?;

    // Owner-guarded clear. `assignee`/`lease_expires_at` always cleared; status
    // flips to `todo` ONLY from `in_progress` (a `blocked` task stays blocked so
    // park-after-question holds). A non-owner / missing row matches 0 rows.
    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET assignee = NULL,
            lease_expires_at = NULL,
            status = CASE WHEN status = 'in_progress' THEN 'todo' ELSE status END,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1 AND assignee = $2
        "#,
            args![task_id.to_owned(), agent_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        // Not owned by `agent_id` (or absent) — no-op, no event. Roll back via
        // drop. Consistent with the owner-guarded no-op contract.
        return Ok(false);
    }

    let payload = serde_json::json!({ "released_by": agent_id });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.released", payload).await?;

    tx.commit().await?;
    Ok(true)
}

/// Heartbeat: extend the lease on a task the calling agent is actively running.
/// Owner-guarded AND status-guarded (`WHERE assignee = :agent_id AND
/// status = 'in_progress'`): the lease deadline is bumped to `now +
/// lease_ttl_secs` ONLY for a row the caller owns and is mid-execution. A
/// non-owner, a missing task, or a task no longer `in_progress` (e.g. already
/// reclaimed or released) mutates NOTHING and records no event, returning
/// `Ok(false)` — keeping the heartbeat minimal and idempotent.
///
/// The new deadline is computed by SQLite (`datetime('now', '+N seconds')`),
/// matching the `claim_next_task` lease idiom so the stored `lease_expires_at`
/// shares the `CURRENT_TIMESTAMP` format and the `<`/`>` reclaim comparisons stay
/// lexical. `lease_ttl_secs` is the raw seconds-to-add; the default TTL tuning
/// (e.g. 30 min) lives at the caller, not here.
///
/// Returns `Ok(true)` on a renewed lease, `Ok(false)` for the guarded no-op.
/// One `work_item.lease_renewed` event on a true mutation; none on the no-op.
pub async fn renew_lease(
    db: &impl DbClient,
    task_id: &str,
    agent_id: &str,
    lease_ttl_secs: i64,
) -> Result<bool, AppError> {
    let mut tx = db.begin().await?;

    // `now + ttl` via the same SQLite `datetime(...)` modifier `claim_next_task`
    // uses for the initial lease, so the stored value's format is identical.
    let ttl_modifier = format!("+{lease_ttl_secs} seconds");
    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET lease_expires_at = datetime('now', $3),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1 AND assignee = $2 AND status = 'in_progress'
        "#,
            args![task_id.to_owned(), agent_id.to_owned(), ttl_modifier],
        )
        .await?;

    if affected == 0 {
        // Not owned + in_progress (or absent) — no-op, no event.
        return Ok(false);
    }

    // Read back the freshly-stamped deadline so the event payload carries the
    // exact stored value (no Rust-side `now` recompute / sub-second skew).
    let lease_expires_at: String = crate::db::tx_scalar_one::<String>(
        tx.as_mut(),
        "SELECT lease_expires_at FROM work_items WHERE id = $1",
        args![task_id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({
        "renewed_by": agent_id,
        "lease_expires_at": lease_expires_at,
    });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.lease_renewed", payload).await?;

    tx.commit().await?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// complete_task (team-execution migration 0013, plan §D). The done→review
// CASCADE — the documented COMPOSER exception to the per-mutator single-tx rule
// ("compose, don't trigger"). It does NOT open a single tx writing one domain
// row + one event; instead it COMPOSES several already-single-mutation steps,
// each carrying its OWN event, in the same disciplined shape as
// `record_finding_decision` / `resolve_open_question`:
//
//   1. read the impl task's lane/status/parent_id (drives the branch);
//   2. transition the task to `done` via `update_work_item_status` (its own tx +
//      `work_item.status_changed` event; the closure-gate read runs inside it) —
//      skipped when the task is already `done` (idempotent re-run);
//   3. a SEPARATE owner-guarded lease-clear (its own tx + `work_item.released`
//      event when it mutates) — completion cleanup, mirroring `release_task`;
//   4. for an `implement`-lane task only, spawn EXACTLY ONE review task under the
//      story (Txn-2: one create + post-create stamp + dep edge + sprint bind, all
//      folded into a single `work_item.created` event), guarded by an idempotency
//      probe so a crash-recovery re-run never double-spawns.
//
// A `review`-lane (or `lane IS NULL` / any non-implement) task completes to
// `done` only — NO review spawn — which is what prevents an infinite
// review→review cascade.
// ---------------------------------------------------------------------------

/// Result of [`complete_task`] (plan §D): the completed task's id and the id of
/// the review task spawned for it (`Some` only for an `implement`-lane
/// completion; `None` for a `review`-lane / non-implement completion, or when a
/// review child already existed and was reused on an idempotent re-run — in the
/// reuse case the EXISTING child id is returned, never `None`). A repo.rs-local
/// struct (NOT in `domain.rs`) to honour the task's file-ownership constraint;
/// the MCP/HTTP surface (T9/T10) wraps it with `Content::json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CompleteTaskResult {
    pub task_id: String,
    /// `Some(review_task_id)` for an implement-lane completion (freshly spawned
    /// OR reused on idempotent re-run); `None` for a review-lane completion.
    pub review_task_id: Option<String>,
}

/// Complete a task and cascade its review (plan §D) — the COMPOSER exception to
/// the single-mutation rule. Composes the `done` transition (closure-gate
/// preserved via [`update_work_item_status`]), an owner-guarded lease clear, and
/// — for an `implement`-lane task — the spawn of exactly one review task under
/// the story, bound back via `reviews_work_item_id`, depending on the impl task,
/// and bound into every sprint the impl task belongs to.
///
/// **Idempotency / crash recovery.** Re-running on an already-`done` task skips
/// the transition; the review-spawn step first probes for an existing review
/// child (`reviews_work_item_id = task_id`) and, if present, returns that id with
/// NO new spawn. So a crash between the `done` transition and the spawn — or a
/// flaky double-call — converges to exactly one review task.
///
/// **Lane awareness.** Only `lane = 'implement'` spawns a review; a `review`-lane
/// (or `lane IS NULL` / any other) task completes to `done` only, returning
/// `review_task_id = None` — this is what prevents a review→review→… cascade.
///
/// **Hierarchy.** The review task's `parent_id` is the impl task's OWN
/// `parent_id` (the story), NOT the impl task — a task cannot parent a task
/// (hierarchy trigger, `0001_init.sql:74/94`).
pub async fn complete_task(
    db: &impl DbClient,
    task_id: &str,
    agent_id: &str,
) -> Result<CompleteTaskResult, AppError> {
    // --- Step 1: read the impl task's lane / status / parent_id. -----------
    // A liveness filter (`deleted_at IS NULL`) keeps a tombstoned task from being
    // completed. `lane` drives the branch; `status` gates the idempotent skip of
    // the `done` transition; `parent_id` is the review task's parent (the story).
    #[derive(Debug)]
    struct CompleteTaskRow {
        lane: Option<String>,
        status: String,
        parent_id: Option<String>,
    }
    impl<'r, R> sqlx::FromRow<'r, R> for CompleteTaskRow
    where
        R: sqlx::Row,
        &'r str: sqlx::ColumnIndex<R>,
        String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(CompleteTaskRow {
                lane: row.try_get("lane")?,
                status: row.try_get("status")?,
                parent_id: row.try_get("parent_id")?,
            })
        }
    }
    let task_row: CompleteTaskRow = db
        .query_opt::<CompleteTaskRow>(
            "SELECT lane, status, parent_id FROM work_items WHERE id = $1 AND deleted_at IS NULL",
            args![task_id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("work_item '{task_id}' not found")))?;

    // --- Step 2: done transition (idempotent). -----------------------------
    // `update_work_item_status` opens its OWN tx, runs the closure-gate read
    // before the write, and emits one `work_item.status_changed` event. Skip it
    // when the task is already `done` so a crash-recovery re-run does not re-emit
    // the event (and does not re-run the gate against an already-terminal row).
    if task_row.status != "done" {
        update_work_item_status(db, task_id, "done").await?;
    }

    // --- Step 2b: reconcile the task's files_touched sets (T4). -------------
    // CLEAR every EXPECTED file row that was never ACTUALLY touched (and audit the
    // divergence) as part of the close. `reconcile_task_files_at_close` owns its
    // OWN tx(s) — the same composer discipline as the surrounding steps (each
    // logical sub-mutation keeps its own tx + event), so it does not break the
    // single-mutation-path invariant. It is IDEMPOTENT: a re-run on an
    // already-`done` task (crash-recovery / double-call) or after a lease-reclaim
    // re-open→re-close finds the untouched-EXPECTED rows already gone, clears zero,
    // and appends no duplicate audit — so it composes cleanly with the two-txn
    // idempotent structure above (the reconcile is its own idempotent step, never
    // leaving a `done`-but-un-reconciled state a re-close wouldn't heal).
    //
    // NB the non-team `transition_status`→done path (`update_work_item_status`) now
    // ALSO reconciles on a task→done transition, so on the FRESH-completion path
    // (Step 2 actually ran the transition) the reconcile has effectively already
    // happened; this explicit call is then a no-op (idempotent). It is retained
    // unconditionally because Step 2 is SKIPPED for an already-`done` task (the
    // crash-recovery re-close), and this is what guarantees the reconcile still
    // fires on that path. The double-invocation on the fresh path is harmless by
    // construction (idempotent).
    reconcile_task_files_at_close(db, task_id).await?;

    // --- Step 3: owner-guarded lease clear (completion cleanup). -----------
    // A SEPARATE single-mutation tx (mirroring `release_task`): clear
    // `assignee`/`lease_expires_at` ONLY for the row the caller owns. Tied to
    // completion, so it carries its OWN `work_item.released` event when it
    // actually mutates a row — consistent with the composer precedent
    // (`record_finding_decision` keeps each logical sub-mutation's event). A
    // re-run after the lease is already cleared (or a non-owner) matches 0 rows
    // → no event, idempotent.
    {
        let mut tx = db.begin().await?;
        let cleared = tx
            .execute(
                r#"
            UPDATE work_items
            SET assignee = NULL,
                lease_expires_at = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1 AND assignee = $2
            "#,
                args![task_id.to_owned(), agent_id.to_owned()],
            )
            .await?;
        if cleared > 0 {
            let payload = serde_json::json!({ "released_by": agent_id });
            record_event(tx.as_mut(), "work_item", task_id, "work_item.released", payload).await?;
            tx.commit().await?;
        }
        // No mutation ⇒ drop (rollback) with no event.
    }

    // --- Step 4: lane branch. ----------------------------------------------
    // Only an `implement`-lane completion cascades a review. A `review`-lane (or
    // `lane IS NULL` / any other) completion stops here — completed to `done`,
    // no spawn — which is what prevents an infinite review→review cascade.
    if task_row.lane.as_deref() != Some("implement") {
        return Ok(CompleteTaskResult {
            task_id: task_id.to_owned(),
            review_task_id: None,
        });
    }

    // Idempotency probe (OUTSIDE the spawn txn): a live review child already
    // bound back to this impl task ⇒ reuse it, no new spawn. This is the
    // crash-recovery guard — a re-run after a prior spawn converges to the SAME
    // review task id.
    let existing_review: Option<String> = crate::db::scalar_opt::<String>(
        db,
        "SELECT id FROM work_items WHERE reviews_work_item_id = $1 AND deleted_at IS NULL",
        args![task_id.to_owned()],
    )
    .await?;
    if let Some(review_id) = existing_review {
        return Ok(CompleteTaskResult {
            task_id: task_id.to_owned(),
            review_task_id: Some(review_id),
        });
    }

    // The review task parents under the STORY = the impl task's own parent_id
    // (a task cannot parent a task; hierarchy trigger 0001_init.sql:74/94). A
    // task with no parent is a data-integrity violation (the hierarchy gate
    // requires a `story` parent at create) — surface it as `Validation` rather
    // than silently skipping the cascade.
    let story_id = task_row.parent_id.as_deref().ok_or_else(|| {
        AppError::Validation(format!(
            "cannot spawn a review task for '{task_id}': it has no parent story"
        ))
    })?;

    // Copy the impl task's `files_touched` onto the review task so the reviewer
    // inherits the file scope (and the §C advisory-overlap scan sees it). Read
    // the raw entries from the impl task's attributes via the same best-effort
    // path the claim uses; an empty/absent set ⇒ no files_touched stamp.
    let impl_attrs: Option<String> = crate::db::scalar_opt::<String>(
        db,
        "SELECT attributes FROM work_items WHERE id = $1",
        args![task_id.to_owned()],
    )
    .await?;
    let impl_files_touched = files_touched_from_attributes(impl_attrs.as_deref());

    // Sprints the impl task belongs to — the review task must join EACH so the
    // §C claim JOIN (which keys on `sprint_tasks`) can ever see it. Read OUTSIDE
    // the spawn txn (a cheap read; the bind INSERTs happen inside).
    let impl_sprints: Vec<String> = {
        #[derive(Debug)]
        struct SprintIdRow {
            sprint_id: String,
        }
        impl<'r, R> sqlx::FromRow<'r, R> for SprintIdRow
        where
            R: sqlx::Row,
            &'r str: sqlx::ColumnIndex<R>,
            String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        {
            fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
                Ok(SprintIdRow {
                    sprint_id: row.try_get("sprint_id")?,
                })
            }
        }
        db.query_all::<SprintIdRow>(
            "SELECT sprint_id FROM sprint_tasks WHERE task_id = $1",
            args![task_id.to_owned()],
        )
        .await?
        .into_iter()
        .map(|r| r.sprint_id)
        .collect()
    };

    // --- Txn-2: spawn the review task (one create + stamps + dep + sprint ---
    // binds, all folded into ONE `work_item.created` event — the composer's
    // single-event-per-logical-sub-mutation discipline). ---------------------
    let mut tx = db.begin().await?;

    // Create the review child under the story via the no-event tx helper (mirrors
    // the `record_finding_decision` spawn path). `CreateOpts` carries no
    // lane/tier/reviews link, so those are stamped by the post-create UPDATE.
    let review_title = format!("Review: {task_id}");
    let review_id = create_work_item_full_tx(
        tx.as_mut(),
        "task",
        Some(story_id),
        &review_title,
        None,
        CreateOpts {
            origin: Some("review"),
            outcome: None,
            shape: None,
            lane: None,
        },
    )
    .await?;
    let review_id_str = review_id.to_string();

    // Post-create stamp: lane='review', the back-link, and tier=NULL (a review is
    // a LANE, never a tier — explicitly NULLed so a CreateOpts-default never
    // leaks a tier onto the review task). Mirrors the `spawned_from_finding_id`
    // post-create stamp idiom.
    tx.execute(
        r#"
        UPDATE work_items
        SET lane = 'review',
            reviews_work_item_id = $2,
            tier = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
        args![review_id_str.clone(), task_id.to_owned()],
    )
    .await?;

    // Copy the impl task's files_touched onto the review task's attributes (only
    // when non-empty). Written as a minimal `{"files_touched": [...]}` object —
    // a valid task attribute shape — directly on the tx (the review task was just
    // created with NULL attributes, so a plain SET is sufficient; no read-merge).
    if !impl_files_touched.is_empty() {
        let attrs = serde_json::json!({ "files_touched": impl_files_touched });
        let attrs_str = serde_json::to_string(&attrs).map_err(|e| AppError::Other(e.into()))?;
        tx.execute(
            "UPDATE work_items SET attributes = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
            args![review_id_str.clone(), attrs_str],
        )
        .await?;
    }

    // Dependency edge: the review task depends_on the impl task, so it never
    // becomes claimable until the impl task is `done` (which it now is). Inserted
    // directly on the tx (NOT via `add_task_dependency`, which opens its own tx +
    // event) so it folds into this one composer event.
    tx.execute(
        r#"
        INSERT INTO task_dependencies (task_id, depends_on_id, kind)
        VALUES ($1, $2, 'sequence')
        "#,
        args![review_id_str.clone(), task_id.to_owned()],
    )
    .await?;

    // Bind the review task into EACH sprint the impl task belongs to — without
    // this the §C claim JOIN (keyed on `sprint_tasks`) never surfaces it.
    // Idempotent at the junction (mirrors `add_tasks_to_sprint`).
    for sprint_id in &impl_sprints {
        tx.execute(
            r#"
            INSERT INTO sprint_tasks (sprint_id, task_id)
            VALUES ($1, $2)
            ON CONFLICT(sprint_id, task_id) DO NOTHING
            "#,
            args![sprint_id.to_owned(), review_id_str.clone()],
        )
        .await?;
    }

    // ONE export-eligible create event for the whole spawn (the child's create +
    // all the stamps/binds fold into it — the composer's single-event discipline).
    let payload = serde_json::json!({
        "kind": "task",
        "parent_id": story_id,
        "title": review_title,
        "lane": "review",
        "reviews_work_item_id": task_id,
        "origin": "review",
    });
    record_event(
        tx.as_mut(),
        "work_item",
        &review_id_str,
        "work_item.created",
        payload,
    )
    .await?;

    tx.commit().await?;

    Ok(CompleteTaskResult {
        task_id: task_id.to_owned(),
        review_task_id: Some(review_id_str),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AnyPool;
    use crate::db::connect_in_memory;
    use crate::domain::{FindingDecisionKind, NewFindingDecision};
    use crate::repo::test_support::*;
    use sqlx::SqlitePool;

    /// Activate a sprint directly (migration-0016: `seed_sprint` now mints a
    /// `'draft'` sprint, but the claim only runs against an `'active'` one).
    /// These in-module tests exercise the CLAIM, not the sprint lifecycle, so a
    /// direct status set is cleaner than walking draft→ready→active.
    async fn activate_sprint(pool: &SqlitePool, sprint_id: &str) {
        sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
            .bind(sprint_id)
            .execute(pool)
            .await
            .expect("activate sprint");
    }

    /// Team-execution plan §E review→rework loop: a `spawn_task` on a
    /// STORY-hosted finding, where the story already has a sprint-bound task,
    /// yields a rework task that is `lane='implement'`, `tier=NULL`, bound into
    /// that SAME sprint (via the host-story fallback resolution path, since the
    /// review run targets the story not a sprint), and is consequently CLAIMABLE
    /// on the Implement lane. The host finding's `rounds` counter increments by 1.
    /// All of this folds into the ONE `finding.decision_recorded` event (no new
    /// event — the rework spawn is part of the decision, R-B4).
    #[tokio::test]
    async fn record_finding_decision_spawn_task_rework_is_claimable() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;

        // An existing sprint-bound impl task under the story — its sprint
        // membership is what the rework spawn's host-story FALLBACK inherits.
        let _impl_task =
            seed_queue_task(&pool, &story, &sprint, "IMPL", Some("implement"), Some("deep")).await;

        // A review finding hosted ON THE STORY (the legal host for a rework
        // spawn). Default `rounds` is NULL on insert.
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("rework: fix the bug"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        // Exactly one finding.decision_recorded event for the whole spawn (no
        // extra event from the lane/sprint/rounds stamps).
        let (_decision_id, spawned) = record_finding_decision(
            &db,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::SpawnTask,
                decided_by: Some("reviewer".into()),
            },
        )
        .await
        .expect("spawn_task decision");
        let rework_id = spawned.expect("spawn_task yields a work_item id").to_string();

        // The decision recorded exactly ONE finding.decision_recorded event (the
        // rework spawn folded in — no separate work_item.created for it).
        assert_eq!(
            count_events_for(&pool, &finding, "finding.decision_recorded").await,
            1,
            "exactly one decision event — the rework spawn folds into it"
        );

        // lane='implement', tier=NULL on the rework task.
        let (lane, tier): (Option<String>, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query("SELECT lane, tier FROM work_items WHERE id = $1")
                .bind(&rework_id)
                .fetch_one(&pool)
                .await
                .unwrap();
            (r.try_get("lane").unwrap(), r.try_get("tier").unwrap())
        };
        assert_eq!(lane.as_deref(), Some("implement"), "rework task is on the implement lane");
        assert_eq!(tier, None, "rework tier left NULL (§E — not a deep default)");

        // Bound into the story's sprint via the fallback path.
        let bound: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = $1 AND task_id = $2",
        )
        .bind(&sprint)
        .bind(&rework_id)
        .fetch_one(&pool)
        .await
        .expect("count sprint membership");
        assert_eq!(bound, 1, "rework task bound into the host story's sprint");

        // The host finding's rounds incremented NULL→1.
        let rounds: Option<i64> =
            sqlx::query_scalar::<_, Option<i64>>("SELECT rounds FROM findings WHERE id = $1")
                .bind(&finding)
                .fetch_one(&pool)
                .await
                .expect("select rounds");
        assert_eq!(rounds, Some(1), "host finding rounds incremented by 1 (NULL→1)");

        // And it is now CLAIMABLE on the Implement lane (tier unconstrained).
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-rework", 1800)
            .await
            .expect("claim runs")
            .expect("the rework task is claimable");
        // The first ready impl candidate is claimed; the rework task must be a
        // legitimate claim target. (The pre-existing IMPL task is also claimable;
        // assert the rework task is reachable by claiming until we get it.)
        let mut claimed_ids = vec![claimed.task_id.clone()];
        if claimed.task_id != rework_id {
            let second = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-rework-2", 1800)
                .await
                .expect("second claim runs")
                .expect("a second implement task is claimable");
            claimed_ids.push(second.task_id);
        }
        assert!(
            claimed_ids.contains(&rework_id),
            "the rework task is claimable on the Implement lane, claimed: {claimed_ids:?}"
        );
    }

    // =======================================================================
    // claim_next_task (T4) — the core concurrency primitive. These cover the
    // plan's five acceptance bullets (a)-(e). lane / lease_expires_at have no
    // dedicated repo mutator yet (those land in T5/T6), so the seed helpers
    // stamp them via raw sqlx UPDATE — the same raw-assertion idiom the rest of
    // this module uses for direct row inspection.
    // =======================================================================

    /// (a) Dependencies are respected (a task with an un-done dependency is NOT
    /// claimed until the dep is done), AND the claimed task carries an advisory
    /// `file_overlap_warnings` entry naming an in-progress file-sharing task —
    /// and is claimed anyway (overlap never blocks, ADR-0002).
    #[tokio::test]
    async fn claim_respects_deps_and_reports_advisory_overlap() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;

        // dep_task is an in-progress task sharing src/shared.rs with the claimable
        // task. It is already leased by another agent.
        let dep_task =
            seed_queue_task(&pool, &story, &sprint, "DEP", Some("implement"), Some("deep")).await;
        // ready_task depends on dep_task. While dep_task is not done, ready_task
        // is NOT claimable.
        let ready_task =
            seed_queue_task(&pool, &story, &sprint, "READY", Some("implement"), Some("deep")).await;
        add_task_dependency(&pool, &ready_task, &dep_task, "sequence")
            .await
            .expect("dep edge");

        // Give both tasks files_touched so the overlap scan has data. T7 re-keyed
        // the advisory onto the first-class `task_files` EXPECTED set, so seed
        // there (the `set_work_item_attributes` calls still populate the
        // `ClaimedTask.files_touched` field, but the OVERLAP scan reads
        // `task_files`).
        set_work_item_attributes(
            &db,
            &dep_task,
            &serde_json::json!({ "files_touched": ["src/shared.rs", "src/only_dep.rs"] }),
        )
        .await
        .expect("dep files_touched");
        set_task_expected_files(
            &db,
            &dep_task,
            &[serde_json::json!("src/shared.rs"), serde_json::json!("src/only_dep.rs")],
        )
        .await
        .expect("dep expected files");
        set_work_item_attributes(
            &db,
            &ready_task,
            &serde_json::json!({ "files_touched": ["src/shared.rs", "src/only_ready.rs"] }),
        )
        .await
        .expect("ready files_touched");
        set_task_expected_files(
            &db,
            &ready_task,
            &[serde_json::json!("src/shared.rs"), serde_json::json!("src/only_ready.rs")],
        )
        .await
        .expect("ready expected files");

        // Put dep_task in_progress (so it is an overlap target) but NOT done.
        sqlx::query(
            "UPDATE work_items SET status = 'in_progress', assignee = 'agent-x', \
             lease_expires_at = datetime('now', '+1800 seconds') WHERE id = $1",
        )
        .bind(&dep_task)
        .execute(&pool)
        .await
        .expect("dep in_progress");

        // With dep_task not done, ready_task is blocked → nothing claimable.
        let none = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs");
        assert!(none.is_none(), "ready_task is dep-blocked while dep is not done");

        // Mark dep_task done → ready_task becomes claimable. (It is also no longer
        // in_progress, so it should NOT appear as an overlap target.) Add a THIRD
        // in_progress task that shares a file, to exercise the advisory report.
        let other_ip =
            seed_queue_task(&pool, &story, &sprint, "OTHER", Some("implement"), Some("deep")).await;
        set_work_item_attributes(
            &db,
            &other_ip,
            &serde_json::json!({ "files_touched": ["src/shared.rs", "src/other.rs"] }),
        )
        .await
        .expect("other files_touched");
        set_task_expected_files(
            &db,
            &other_ip,
            &[serde_json::json!("src/shared.rs"), serde_json::json!("src/other.rs")],
        )
        .await
        .expect("other expected files");
        sqlx::query(
            "UPDATE work_items SET status = 'in_progress', assignee = 'agent-y', \
             lease_expires_at = datetime('now', '+1800 seconds') WHERE id = $1",
        )
        .bind(&other_ip)
        .execute(&pool)
        .await
        .expect("other in_progress");
        sqlx::query("UPDATE work_items SET status = 'done' WHERE id = $1")
            .bind(&dep_task)
            .execute(&pool)
            .await
            .expect("dep done");

        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("ready_task is now claimable");

        assert_eq!(claimed.task_id, ready_task, "the dep-satisfied task is claimed");
        assert_eq!(claimed.assignee, "agent-a");
        assert!(!claimed.lease_expires_at.is_empty(), "lease deadline stamped");

        // Advisory overlap: other_ip (in_progress, shares src/shared.rs) IS named;
        // dep_task (now done, not in_progress) is NOT. The claim succeeded despite
        // the overlap.
        let names: Vec<&str> = claimed
            .file_overlap_warnings
            .iter()
            .map(|w| w.task_id.as_str())
            .collect();
        assert!(
            names.contains(&other_ip.as_str()),
            "the in-progress file-sharing task is reported, got {names:?}"
        );
        assert!(
            !names.contains(&dep_task.as_str()),
            "a done (not in-progress) task is not an overlap target"
        );
        let other_warning = claimed
            .file_overlap_warnings
            .iter()
            .find(|w| w.task_id == other_ip)
            .expect("other_ip warning present");
        assert_eq!(
            other_warning.shared,
            vec!["src/shared.rs".to_string()],
            "the shared path is the one common file"
        );

        // And the claim actually leased the row.
        let (status, assignee, _) = task_lease_state(&pool, &ready_task).await;
        assert_eq!(status, "in_progress");
        assert_eq!(assignee.as_deref(), Some("agent-a"));
    }

    /// (b) An empty / ineligible lane returns `Ok(None)`.
    #[tokio::test]
    async fn claim_empty_lane_returns_none() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        // One implement-lane task exists, but we claim the REVIEW lane → none.
        // (The sprint is active so the None is genuinely the empty-lane path,
        // not the sprint-status guard.)
        seed_queue_task(&pool, &story, &sprint, "T", Some("implement"), Some("deep")).await;

        let claimed = claim_next_task(&db, &sprint, Lane::Review, None, "agent-r", 1800)
            .await
            .expect("claim runs");
        assert!(claimed.is_none(), "no review-lane task ⇒ Ok(None)");

        // Also: a tier that matches nothing returns none.
        let claimed_tier = claim_next_task(&db, &sprint, Lane::Implement, Some(Tier::Lite), "agent-l", 1800)
            .await
            .expect("claim runs");
        assert!(claimed_tier.is_none(), "no lite-tier implement task ⇒ Ok(None)");
    }

    /// (c) A task whose `lease_expires_at` is seeded in the PAST is lazily
    /// reclaimed to status='todo'/assignee=NULL, and the call records EXACTLY
    /// ONE coarse, export-inert `leases.reclaimed` event.
    #[tokio::test]
    async fn claim_lazily_reclaims_expired_lease_with_one_inert_event() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        let task =
            seed_queue_task(&pool, &story, &sprint, "STALE", Some("implement"), Some("deep")).await;

        // Seed an EXPIRED lease in the past (no sleep): in_progress + a past
        // lease_expires_at owned by a now-dead agent.
        sqlx::query(
            "UPDATE work_items SET status = 'in_progress', assignee = 'dead-agent', \
             lease_expires_at = '2000-01-01 00:00:00' WHERE id = $1",
        )
        .bind(&task)
        .execute(&pool)
        .await
        .expect("seed expired lease");

        let events_before = count_events_of_type(&pool, "leases.reclaimed").await;

        // Claiming reclaims the stale lease first, then re-claims the now-todo task
        // for agent-a (same call). The task ends up leased to agent-a.
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("the reclaimed task is then claimable");
        assert_eq!(claimed.task_id, task);
        assert_eq!(claimed.assignee, "agent-a", "re-leased to the new claimer");

        // Exactly ONE coarse leases.reclaimed event was recorded.
        assert_eq!(
            count_events_of_type(&pool, "leases.reclaimed").await,
            events_before + 1,
            "exactly one coarse reclaim event"
        );
        // And it is export-INERT: aggregate_type='sprint', NOT 'work_item'.
        use sqlx::Row as _;
        let row = sqlx::query(
            "SELECT aggregate_type, aggregate_id FROM events WHERE event_type = 'leases.reclaimed'",
        )
        .fetch_one(&pool)
        .await
        .expect("reclaim event row");
        let agg_type: String = row.try_get("aggregate_type").unwrap();
        let agg_id: String = row.try_get("aggregate_id").unwrap();
        assert_eq!(agg_type, "sprint", "reclaim event is export-inert (not work_item)");
        assert_eq!(agg_id, sprint, "reclaim event keyed by the sprint id");

        // A second claim against a fresh sprint with no expired lease records NO
        // reclaim event (the zero-rows path emits nothing).
        let events_after_first = count_events_of_type(&pool, "leases.reclaimed").await;
        let _ = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-b", 1800)
            .await
            .expect("second claim runs");
        assert_eq!(
            count_events_of_type(&pool, "leases.reclaimed").await,
            events_after_first,
            "no further reclaim event when nothing is expired"
        );
    }

    /// (d) A legacy `lane IS NULL` task is NEVER returned by the claim
    /// (back-compat — null-lane tasks are invisible to team execution).
    #[tokio::test]
    async fn claim_never_returns_null_lane_task() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        // A task with lane = NULL, bound to the sprint, todo + unleased. The
        // sprint is active so the None is the null-lane path, not the guard.
        seed_queue_task(&pool, &story, &sprint, "LEGACY", None, None).await;

        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs");
        assert!(claimed.is_none(), "a lane=NULL task is invisible to the claim");
    }

    /// (e) The sprint-status guard returns `Ok(None)` unless the sprint is
    /// EXACTLY `'active'` (migration-0016 layer-2 rule) — even when a ready task
    /// exists. `seed_sprint` now mints a `'draft'` sprint, so the claim is
    /// blocked until we activate it; the non-runnable vocab (`draft`/`ready`/
    /// `review`/`cancelled`) is all guarded.
    #[tokio::test]
    async fn claim_honours_sprint_status_guard() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await; // starts 'draft'
        seed_queue_task(&pool, &story, &sprint, "T", Some("implement"), Some("deep")).await;

        // A 'draft' sprint (the create-default) is NOT runnable under the
        // migration-0016 rule ⇒ Ok(None) despite a ready task.
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs");
        assert!(claimed.is_none(), "a draft sprint is not runnable ⇒ Ok(None)");

        // Every other non-`active` status is likewise non-runnable.
        for status in ["ready", "review", "cancelled"] {
            sqlx::query("UPDATE sprints SET status = $2 WHERE id = $1")
                .bind(&sprint)
                .bind(status)
                .execute(&pool)
                .await
                .expect("set sprint status");
            let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
                .await
                .expect("claim runs");
            assert!(
                claimed.is_none(),
                "a '{status}' sprint is not runnable ⇒ Ok(None)"
            );
        }

        // Activate the sprint and the same task IS claimable, proving the guard
        // (not a missing task) caused the None above.
        sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
            .bind(&sprint)
            .execute(&pool)
            .await
            .expect("activate sprint");
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs");
        assert!(claimed.is_some(), "the task is claimable once the sprint is active");
    }

    /// (g) A checkpoint task (`work_items.checkpoint = 1`, migration 0016)
    /// freezes its whole sprint: while it is `in_progress` the claim yields
    /// `Ok(None)` (a sprint-wide barrier) even for an unrelated ready task, and
    /// the claim resumes the moment that checkpoint task leaves `in_progress`.
    #[tokio::test]
    async fn claim_honours_checkpoint_freeze() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        // Activate the sprint so only the checkpoint freeze (not the status
        // guard) gates the claim.
        sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
            .bind(&sprint)
            .execute(&pool)
            .await
            .expect("activate sprint");

        // A normal ready implement task and a SEPARATE checkpoint task, both
        // bound to the sprint.
        let ready =
            seed_queue_task(&pool, &story, &sprint, "READY", Some("implement"), Some("deep")).await;
        let checkpoint =
            seed_queue_task(&pool, &story, &sprint, "CKPT", Some("implement"), Some("deep")).await;

        // Mark the checkpoint task as a checkpoint AND put it in_progress (the
        // freeze condition). A direct UPDATE keeps the test self-contained.
        sqlx::query(
            "UPDATE work_items SET checkpoint = 1, status = 'in_progress', \
             assignee = 'agent-ckpt', lease_expires_at = datetime('now', '+1800 seconds') \
             WHERE id = $1",
        )
        .bind(&checkpoint)
        .execute(&pool)
        .await
        .expect("seed in-progress checkpoint");

        // The sprint is frozen → no claim, even though `ready` is dispatchable.
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs");
        assert!(
            claimed.is_none(),
            "an in_progress checkpoint task freezes the whole sprint ⇒ Ok(None)"
        );

        // Flip the checkpoint task out of in_progress (→ done): the freeze lifts.
        sqlx::query("UPDATE work_items SET status = 'done' WHERE id = $1")
            .bind(&checkpoint)
            .execute(&pool)
            .await
            .expect("complete checkpoint");

        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("the ready task is claimable once the checkpoint clears");
        assert_eq!(
            claimed.task_id, ready,
            "the ready task is dispatched once the sprint-wide freeze lifts"
        );
    }

    /// (f, real-world path) A task left at the `create_work_item` DEFAULT
    /// `status='open'` (NOT pre-staged to 'todo') IS claimable — guarding the
    /// review→rework cascade, since `complete_task` (T6) and
    /// `record_finding_decision` (T8) both spawn their tasks via the create path
    /// and those tasks default to 'open'. A 'todo'-only predicate would render
    /// them invisible and silently never run the cascade.
    #[tokio::test]
    async fn claim_returns_open_status_task() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        // Activate the sprint so the claim isn't blocked by the sprint-status
        // guard — this test isolates the TASK-status 'open' predicate (E5).
        activate_sprint(&pool, &sprint).await;
        // Created exactly the way create_work_item leaves it: status='open'.
        let task =
            seed_queue_task_open(&pool, &story, &sprint, "OPEN", Some("implement"), Some("deep"))
                .await;

        // Sanity: the task really is at the 'open' create-default, not 'todo'.
        let (status_before, _, _) = task_lease_state(&pool, &task).await;
        assert_eq!(
            status_before, "open",
            "the seed preserves the create-default 'open' status"
        );

        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("an 'open'-status task is claimable (the spawned-task path)");
        assert_eq!(claimed.task_id, task, "the 'open' task is the one claimed");
        assert_eq!(claimed.assignee, "agent-a");

        // And it was actually leased: status flips to in_progress, assignee set.
        let (status_after, assignee, lease) = task_lease_state(&pool, &task).await;
        assert_eq!(status_after, "in_progress");
        assert_eq!(assignee.as_deref(), Some("agent-a"));
        assert!(lease.is_some(), "lease deadline stamped on the claimed open task");
    }

    // =======================================================================
    // release_task + renew_lease (T5) — the lease-lifecycle companions to
    // claim_next_task. Reuse the claim seed helpers (seed_chain_to_story +
    // seed_sprint + seed_queue_task) for the project→…→story→task chain and a
    // claimed/leased task; cover the four plan T5 acceptance bullets.
    // =======================================================================

    /// release frees a lease: an owned `in_progress` task returns to `todo` with
    /// `assignee`/`lease_expires_at` cleared, and exactly one `work_item.released`
    /// event is recorded.
    #[tokio::test]
    async fn release_frees_in_progress_lease() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        let task =
            seed_queue_task(&pool, &story, &sprint, "T", Some("implement"), Some("deep")).await;

        // Claim it so it is genuinely in_progress + leased to agent-a.
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("claimable");
        assert_eq!(claimed.task_id, task);

        let events_before = count_events_of_type(&pool, "work_item.released").await;

        let released = release_task(&db, &task, "agent-a")
            .await
            .expect("release runs");
        assert!(released, "the owner releases its own in_progress lease");

        let (status, assignee, lease) = task_lease_state(&pool, &task).await;
        assert_eq!(status, "todo", "in_progress → todo on release");
        assert_eq!(assignee, None, "assignee cleared");
        assert_eq!(lease, None, "lease_expires_at cleared");

        assert_eq!(
            count_events_of_type(&pool, "work_item.released").await,
            events_before + 1,
            "exactly one release event on a true mutation"
        );
    }

    /// releasing a `blocked` task clears the lease but KEEPS status='blocked'
    /// (park-after-question: a task parked on an open question must stay
    /// invisible to the claim until the question resolves).
    #[tokio::test]
    async fn release_keeps_blocked_task_blocked() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        let task =
            seed_queue_task(&pool, &story, &sprint, "T", Some("implement"), Some("deep")).await;

        // Seed a leased BLOCKED task owned by agent-a (the park-after-question
        // shape: assignee + lease set, status='blocked').
        sqlx::query(
            "UPDATE work_items SET status = 'blocked', assignee = 'agent-a', \
             lease_expires_at = datetime('now', '+1800 seconds') WHERE id = $1",
        )
        .bind(&task)
        .execute(&pool)
        .await
        .expect("seed blocked+leased");

        let released = release_task(&db, &task, "agent-a")
            .await
            .expect("release runs");
        assert!(released, "the owner-guarded clear still mutates (lease cleared)");

        let (status, assignee, lease) = task_lease_state(&pool, &task).await;
        assert_eq!(status, "blocked", "a blocked task STAYS blocked on release");
        assert_eq!(assignee, None, "assignee still cleared");
        assert_eq!(lease, None, "lease still cleared");
    }

    /// renew extends `lease_expires_at` for an owned `in_progress` task, and
    /// records exactly one `work_item.lease_renewed` event.
    #[tokio::test]
    async fn renew_extends_owned_in_progress_lease() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        let task =
            seed_queue_task(&pool, &story, &sprint, "T", Some("implement"), Some("deep")).await;

        // Seed an owned in_progress task with a SHORT lease, so a renew to a
        // longer TTL produces a strictly-later deadline (lexical compare on the
        // CURRENT_TIMESTAMP format).
        sqlx::query(
            "UPDATE work_items SET status = 'in_progress', assignee = 'agent-a', \
             lease_expires_at = datetime('now', '+1 seconds') WHERE id = $1",
        )
        .bind(&task)
        .execute(&pool)
        .await
        .expect("seed short lease");
        let (_, _, before) = task_lease_state(&pool, &task).await;
        let before = before.expect("seeded lease present");

        let events_before = count_events_of_type(&pool, "work_item.lease_renewed").await;

        let renewed = renew_lease(&db, &task, "agent-a", 3600)
            .await
            .expect("renew runs");
        assert!(renewed, "the owner renews its own in_progress lease");

        let (status, assignee, after) = task_lease_state(&pool, &task).await;
        assert_eq!(status, "in_progress", "status unchanged by renew");
        assert_eq!(assignee.as_deref(), Some("agent-a"), "assignee unchanged");
        let after = after.expect("lease still present");
        assert!(
            after > before,
            "renew pushes the deadline later: {after} > {before}"
        );

        assert_eq!(
            count_events_of_type(&pool, "work_item.lease_renewed").await,
            events_before + 1,
            "exactly one renew event on a true mutation"
        );
    }

    /// A non-owner release/renew is a no-op (`Ok(false)`) that mutates nothing
    /// and records no event. Also covers renew of a non-`in_progress` owned task
    /// (status-guard no-op).
    #[tokio::test]
    async fn release_and_renew_non_owner_is_noop() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        let task =
            seed_queue_task(&pool, &story, &sprint, "T", Some("implement"), Some("deep")).await;

        // Owned by agent-a, in_progress + leased.
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("claimable");
        assert_eq!(claimed.task_id, task);
        let (_, _, lease_before) = task_lease_state(&pool, &task).await;

        let rel_events_before = count_events_of_type(&pool, "work_item.released").await;
        let renew_events_before = count_events_of_type(&pool, "work_item.lease_renewed").await;

        // A DIFFERENT agent cannot release or renew agent-a's lease.
        let released = release_task(&db, &task, "agent-b")
            .await
            .expect("release runs");
        assert!(!released, "non-owner release is a no-op");
        let renewed = renew_lease(&db, &task, "agent-b", 3600)
            .await
            .expect("renew runs");
        assert!(!renewed, "non-owner renew is a no-op");

        // Nothing mutated: still owned by agent-a, in_progress, same lease.
        let (status, assignee, lease_after) = task_lease_state(&pool, &task).await;
        assert_eq!(status, "in_progress", "status untouched by non-owner");
        assert_eq!(assignee.as_deref(), Some("agent-a"), "assignee untouched");
        assert_eq!(lease_after, lease_before, "lease deadline untouched");

        // No events on either no-op.
        assert_eq!(
            count_events_of_type(&pool, "work_item.released").await,
            rel_events_before,
            "no release event on the non-owner no-op"
        );
        assert_eq!(
            count_events_of_type(&pool, "work_item.lease_renewed").await,
            renew_events_before,
            "no renew event on the non-owner no-op"
        );

        // Owner renew of a NON-in_progress task is also a status-guard no-op:
        // release agent-a's task (→ todo), then an owner renew finds no
        // in_progress row to bump.
        release_task(&db, &task, "agent-a")
            .await
            .expect("owner release runs");
        let renew_events_mid = count_events_of_type(&pool, "work_item.lease_renewed").await;
        let renewed_todo = renew_lease(&db, &task, "agent-a", 3600)
            .await
            .expect("renew runs");
        assert!(!renewed_todo, "renew of a non-in_progress task is a no-op");
        assert_eq!(
            count_events_of_type(&pool, "work_item.lease_renewed").await,
            renew_events_mid,
            "no renew event when the status guard fails"
        );
    }

    // =======================================================================
    // complete_task (T6) — the done→review CASCADE. Reuse the claim/release seed
    // helpers (seed_chain_to_story + seed_sprint + seed_queue_task) and cover the
    // three plan T6 acceptance bullets: an implement-lane completion spawns
    // exactly one back-linked review task under the story (sprint-bound, with a
    // dep edge, files_touched copied); a review-lane completion spawns nothing;
    // a re-run is idempotent (no duplicate, same id).
    // =======================================================================

    /// Read a review task's (parent_id, reviews_work_item_id, lane, tier, status)
    /// for the back-link / hierarchy assertions.
    async fn review_task_shape(
        pool: &SqlitePool,
        review_id: &str,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) {
        use sqlx::Row as _;
        let r = sqlx::query(
            "SELECT parent_id, reviews_work_item_id, lane, tier, status \
             FROM work_items WHERE id = $1",
        )
        .bind(review_id)
        .fetch_one(pool)
        .await
        .expect("review task row");
        (
            r.try_get("parent_id").unwrap(),
            r.try_get("reviews_work_item_id").unwrap(),
            r.try_get("lane").unwrap(),
            r.try_get("tier").unwrap(),
            r.try_get("status").unwrap(),
        )
    }

    /// (1) Completing an `implement`-lane task transitions it to done, clears its
    /// lease, and spawns EXACTLY ONE review task: parent = the story (NOT the impl
    /// task), back-linked via `reviews_work_item_id`, `lane='review'`,
    /// `tier=NULL`, bound into the impl task's sprint, with a dependency edge on
    /// the impl task, and the impl task's `files_touched` copied across.
    #[tokio::test]
    async fn complete_implement_task_spawns_one_backlinked_review() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        let task =
            seed_queue_task(&pool, &story, &sprint, "IMPL", Some("implement"), Some("deep")).await;

        // Give the impl task a files_touched spec so the cascade copies it.
        set_work_item_attributes(
            &db,
            &task,
            &serde_json::json!({ "files_touched": ["src/a.rs", { "repo": "o/n", "path": "src/b.rs" }] }),
        )
        .await
        .expect("impl files_touched");

        // Claim it so it is genuinely in_progress + leased to agent-a.
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("claimable");
        assert_eq!(claimed.task_id, task);

        let result = complete_task(&db, &task, "agent-a")
            .await
            .expect("complete runs");
        assert_eq!(result.task_id, task);
        let review_id = result
            .review_task_id
            .clone()
            .expect("an implement-lane completion spawns a review task");

        // The impl task is done + lease cleared.
        let (status, assignee, lease) = task_lease_state(&pool, &task).await;
        assert_eq!(status, "done", "impl task transitioned to done");
        assert_eq!(assignee, None, "lease assignee cleared on completion");
        assert_eq!(lease, None, "lease deadline cleared on completion");

        // EXACTLY ONE review task bound back to the impl task.
        let review_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_items WHERE reviews_work_item_id = $1")
                .bind(&task)
                .fetch_one(&pool)
                .await
                .expect("count reviews");
        assert_eq!(review_count, 1, "exactly one review task spawned");

        // Hierarchy + back-link + lane/tier shape.
        let (parent_id, reviews, lane, tier, rstatus) = review_task_shape(&pool, &review_id).await;
        assert_eq!(
            parent_id.as_deref(),
            Some(story.as_str()),
            "review task parents under the STORY, not the impl task"
        );
        assert_eq!(
            reviews.as_deref(),
            Some(task.as_str()),
            "review task back-links to the impl task it covers"
        );
        assert_eq!(lane.as_deref(), Some("review"), "spawned with lane='review'");
        assert_eq!(tier, None, "review is a lane, not a tier → tier NULL");
        assert_eq!(rstatus, "open", "review task starts at the create-default status");

        // Bound into the impl task's sprint.
        let bound = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = $1 AND task_id = $2",
        )
        .bind(&sprint)
        .bind(&review_id)
        .fetch_one(&pool)
        .await
        .expect("count sprint binding");
        assert_eq!(bound, 1, "review task bound into the impl task's sprint");

        // Dependency edge: review depends_on impl.
        let dep = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM task_dependencies WHERE task_id = $1 AND depends_on_id = $2",
        )
        .bind(&review_id)
        .bind(&task)
        .fetch_one(&pool)
        .await
        .expect("count dep edge");
        assert_eq!(dep, 1, "review task depends on the impl task");

        // files_touched copied verbatim (bare string + {repo,path} object).
        let attrs: String =
            sqlx::query_scalar::<_, Option<String>>("SELECT attributes FROM work_items WHERE id = $1")
                .bind(&review_id)
                .fetch_one(&pool)
                .await
                .expect("review attributes")
                .expect("review attributes present (files_touched copied)");
        let parsed: serde_json::Value = serde_json::from_str(&attrs).expect("attrs json");
        assert_eq!(
            parsed.get("files_touched"),
            Some(&serde_json::json!(["src/a.rs", { "repo": "o/n", "path": "src/b.rs" }])),
            "the impl task's files_touched is copied onto the review task"
        );

        // The review task IS claimable in the review lane now the impl task is done
        // (proves the sprint bind + dep-satisfied wiring is correct end-to-end).
        let review_claim = claim_next_task(&db, &sprint, Lane::Review, None, "agent-r", 1800)
            .await
            .expect("review claim runs")
            .expect("review task is claimable");
        assert_eq!(review_claim.task_id, review_id);
    }

    /// (2) Completing a `review`-lane task transitions it to done and spawns NO
    /// task (prevents an infinite review→review cascade).
    #[tokio::test]
    async fn complete_review_task_spawns_nothing() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        let review =
            seed_queue_task(&pool, &story, &sprint, "REVIEW", Some("review"), None).await;

        // Claim it in the review lane so it is in_progress + leased.
        let claimed = claim_next_task(&db, &sprint, Lane::Review, None, "agent-r", 1800)
            .await
            .expect("claim runs")
            .expect("claimable");
        assert_eq!(claimed.task_id, review);

        let tasks_before =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_items WHERE kind = 'task'")
                .fetch_one(&pool)
                .await
                .expect("count tasks before");

        let result = complete_task(&db, &review, "agent-r")
            .await
            .expect("complete runs");
        assert_eq!(result.task_id, review);
        assert_eq!(
            result.review_task_id, None,
            "a review-lane completion spawns no further task"
        );

        let (status, assignee, lease) = task_lease_state(&pool, &review).await;
        assert_eq!(status, "done", "review task transitioned to done");
        assert_eq!(assignee, None, "lease cleared");
        assert_eq!(lease, None, "lease deadline cleared");

        let tasks_after =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_items WHERE kind = 'task'")
                .fetch_one(&pool)
                .await
                .expect("count tasks after");
        assert_eq!(
            tasks_after, tasks_before,
            "no new task row created by a review-lane completion"
        );
    }

    /// (3) Re-running `complete_task` on an already-completed implement task is
    /// idempotent: no duplicate review task, and the SAME review_task_id is
    /// returned (crash-recovery convergence).
    #[tokio::test]
    async fn complete_implement_task_is_idempotent() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        let task =
            seed_queue_task(&pool, &story, &sprint, "IMPL", Some("implement"), Some("deep")).await;

        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("claimable");
        assert_eq!(claimed.task_id, task);

        let first = complete_task(&db, &task, "agent-a")
            .await
            .expect("first complete");
        let review_id = first
            .review_task_id
            .clone()
            .expect("first run spawns a review task");

        // Re-run (the crash-recovery / double-call case). The task is already
        // done; the spawn probe finds the existing review child and returns it.
        let second = complete_task(&db, &task, "agent-a")
            .await
            .expect("second complete (idempotent)");
        assert_eq!(
            second.review_task_id.as_deref(),
            Some(review_id.as_str()),
            "the re-run returns the SAME review task id, not a new one"
        );

        // Still EXACTLY ONE review task — no duplicate spawn.
        let review_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_items WHERE reviews_work_item_id = $1")
                .bind(&task)
                .fetch_one(&pool)
                .await
                .expect("count reviews");
        assert_eq!(review_count, 1, "the re-run does not double-spawn the review task");
    }

    // =======================================================================
    // complete_task × files_touched reconcile (T4) — the team-lane close route
    // fires the close-time reconcile and is idempotent under re-run.
    // =======================================================================

    /// Count `reconcile`-kind activity rows for a task (the audit-on-divergence
    /// signal from `reconcile_task_files_at_close`).
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

    /// (d, team-lane route) `complete_task` TRIGGERS the close-time reconcile: a
    /// material divergence (an EXPECTED file never actually touched) is cleared and
    /// an audit activity is appended, while the touched-EXPECTED and ALL ACTUAL
    /// rows survive (EXPECTED/ACTUAL stay distinct kinds). Re-running `complete_task`
    /// (the crash-recovery / double-call path) is idempotent — no second clear, no
    /// second audit — proving the reconcile composes cleanly with the existing
    /// two-txn idempotent close.
    #[tokio::test]
    async fn complete_task_reconciles_files_and_is_idempotent() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        let task =
            seed_queue_task(&pool, &story, &sprint, "IMPL", Some("implement"), Some("deep")).await;

        // EXPECTED: a.rs (will be touched) + b.rs (will NOT be touched → cleared).
        set_task_expected_files(
            &db,
            &task,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/b.rs")],
        )
        .await
        .expect("set expected");
        // ACTUAL: a.rs (matches an expected) + c.rs (over-report, never expected).
        add_task_actual_files(
            &db,
            &task,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/c.rs")],
        )
        .await
        .expect("append actual");

        // Claim then complete via the team-lane route.
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim runs")
            .expect("claimable");
        assert_eq!(claimed.task_id, task);

        complete_task(&db, &task, "agent-a")
            .await
            .expect("first complete");

        // The reconcile fired on close: b.rs (untouched EXPECTED) cleared; a.rs
        // (touched EXPECTED) kept; BOTH actual rows preserved (distinct kinds).
        let expected_paths: Vec<String> = list_task_files(&db, &task, Some("expected"))
            .await
            .expect("expected after complete")
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(
            expected_paths,
            vec!["src/a.rs".to_string()],
            "complete_task cleared the untouched EXPECTED (b.rs), kept the touched one (a.rs)"
        );
        let actual_paths: Vec<String> = list_task_files(&db, &task, Some("actual"))
            .await
            .expect("actual after complete")
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(
            actual_paths,
            vec!["src/a.rs".to_string(), "src/c.rs".to_string()],
            "ALL actual rows survive the close-time reconcile (over-report c.rs not pruned)"
        );
        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            1,
            "complete_task appended exactly one reconcile audit on the divergence"
        );

        // Re-run complete_task (crash-recovery / double-call): idempotent — no
        // second clear, no second audit.
        complete_task(&db, &task, "agent-a")
            .await
            .expect("second complete (idempotent)");
        assert_eq!(
            count_reconcile_activity(&pool, &task).await,
            1,
            "a re-run of complete_task does not re-audit (idempotent reconcile)"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM task_files WHERE task_id = $1 AND kind = 'expected'"
            )
            .bind(&task)
            .fetch_one(&pool)
            .await
            .expect("count expected"),
            1,
            "a re-run of complete_task does not re-clear (b.rs stays gone, a.rs stays)"
        );
    }

    // =======================================================================
    // claim_next_task advisory overlap re-keyed onto the CANONICAL
    // (repo_link_id, path) form via the first-class `task_files` EXPECTED set
    // (migration 0020, T7). AC (b): the advisory neither false-positives nor
    // false-negatives — a bare path and an explicit-primary {repo, path} for
    // the SAME primary repo OVERLAP (no false-negative); genuinely different
    // files / repos do NOT (no false-positive).
    // =======================================================================

    /// (b) The claim-time advisory keys on the canonical `(repo_link_id, path)`
    /// form. Two in_progress tasks that share a canonical key via DIFFERENT
    /// spellings (a bare `"src/shared.rs"` vs an explicit-primary
    /// `{repo: <primary>, path: "src/shared.rs"}`) are reported as overlapping
    /// (NO false-negative). A task whose EXPECTED set is a genuinely different
    /// file — and one in a genuinely different (non-primary) repo at the SAME
    /// path — are NOT reported (NO false-positive).
    #[tokio::test]
    async fn claim_advisory_overlap_keys_on_canonical_form() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;

        // PRIMARY + a NON-primary linked repo on the project (the linked slugs
        // an explicit `{repo, path}` EXPECTED entry can reference).
        let project = find_project_ancestor(&pool, &story)
            .await
            .expect("project ancestor");
        add_repo_link(&pool, &project, "octocat/hello-world", true)
            .await
            .expect("primary repo link");
        add_repo_link(&pool, &project, "octocat/other-repo", false)
            .await
            .expect("secondary repo link");

        // The task we will CLAIM. Its EXPECTED set names the primary file via the
        // BARE spelling.
        let claimed_task =
            seed_queue_task(&pool, &story, &sprint, "CLAIM", Some("implement"), Some("deep")).await;
        set_task_expected_files(
            &db,
            &claimed_task,
            &[serde_json::json!("src/shared.rs"), serde_json::json!("src/only_claim.rs")],
        )
        .await
        .expect("claimed expected files");

        // SHARES the primary file via the EXPLICIT-PRIMARY spelling — must
        // OVERLAP the claimed task's bare spelling (no false-negative).
        let sharer =
            seed_queue_task(&pool, &story, &sprint, "SHARER", Some("implement"), Some("deep")).await;
        set_task_expected_files(
            &db,
            &sharer,
            &[
                serde_json::json!({ "repo": "octocat/hello-world", "path": "src/shared.rs" }),
                serde_json::json!("src/only_sharer.rs"),
            ],
        )
        .await
        .expect("sharer expected files");

        // A genuinely DIFFERENT file — must NOT overlap (no false-positive).
        let disjoint =
            seed_queue_task(&pool, &story, &sprint, "DISJOINT", Some("implement"), Some("deep"))
                .await;
        set_task_expected_files(
            &db,
            &disjoint,
            &[serde_json::json!("src/elsewhere.rs")],
        )
        .await
        .expect("disjoint expected files");

        // SAME PATH but a DIFFERENT (non-primary) repo — a DISTINCT canonical key,
        // so it must NOT overlap the claimed task's primary src/shared.rs (no
        // false-positive across repos).
        let other_repo =
            seed_queue_task(&pool, &story, &sprint, "OTHERREPO", Some("implement"), Some("deep"))
                .await;
        set_task_expected_files(
            &db,
            &other_repo,
            &[serde_json::json!({ "repo": "octocat/other-repo", "path": "src/shared.rs" })],
        )
        .await
        .expect("other-repo expected files");

        // Put the three OTHER tasks in_progress (overlap targets are scanned only
        // among in_progress sprint tasks). Leave `claimed_task` at todo so the
        // claim picks it.
        for t in [&sharer, &disjoint, &other_repo] {
            sqlx::query(
                "UPDATE work_items SET status = 'in_progress', assignee = 'agent-x', \
                 lease_expires_at = datetime('now', '+1800 seconds') WHERE id = $1",
            )
            .bind(t)
            .execute(&pool)
            .await
            .expect("set other in_progress");
        }

        // Claim the bare-spelling task; the advisory scan runs post-commit.
        let claimed = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-claim", 1800)
            .await
            .expect("claim runs")
            .expect("the claimed_task is claimable");
        assert_eq!(claimed.task_id, claimed_task, "the ready task is claimed");

        let warned: Vec<&str> = claimed
            .file_overlap_warnings
            .iter()
            .map(|w| w.task_id.as_str())
            .collect();

        // No false-NEGATIVE: the explicit-primary sharer overlaps the bare claim.
        assert!(
            warned.contains(&sharer.as_str()),
            "the explicit-primary {{repo, path}} sharer overlaps the bare spelling \
             (canonical key fold — no false-negative), got {warned:?}"
        );
        let sharer_warning = claimed
            .file_overlap_warnings
            .iter()
            .find(|w| w.task_id == sharer)
            .expect("sharer warning present");
        assert_eq!(
            sharer_warning.shared,
            vec!["src/shared.rs".to_string()],
            "the reported shared path is the canonical primary file"
        );

        // No false-POSITIVE: a genuinely different file, and the same path in a
        // DIFFERENT repo, are NOT reported.
        assert!(
            !warned.contains(&disjoint.as_str()),
            "a disjoint file is not an overlap (no false-positive), got {warned:?}"
        );
        assert!(
            !warned.contains(&other_repo.as_str()),
            "the same path in a NON-primary repo is a distinct canonical key — not an overlap \
             (no false-positive across repos), got {warned:?}"
        );
    }
}
