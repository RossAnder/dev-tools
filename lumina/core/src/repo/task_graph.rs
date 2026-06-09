//! Task graph + dispatch-tier derivation (migrations 0005/0006, R5 carve):
//! `set_task_kind`, the pure `compute_tier`, Kahn's `compute_task_batches`, the
//! `get_task_dispatch_plan` fold, and `set_task_tier`. The private row decoders
//! (`TaskBatchRow`, `DispatchSpecRow`) and the `task_kind_sort_key` helper are
//! used only by these fns and move with them.
//!
//! `pub use task_graph::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here (incl. the pure `compute_tier`) stays reachable at its existing
//! `crate::repo::*` path. `list_task_dependencies` (now in
//! `task_dependencies.rs`) and `work_item_kind` / `enum_to_str` (in `shared.rs`)
//! are reached via `use super::*`.

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::DbClient;
use crate::domain::{BatchEntry, Lane, TaskKind, Tier};
use crate::error::AppError;
use serde_json::Value;

// ---------------------------------------------------------------------------
// work_items.task_kind setter (migration 0005, vocab narrowed in 0007).
// Task-scoped column; the CHECK constraint accepts NULL OR the three enum
// literals (foundation | main | polish — see CONVENTIONS §j for the cull
// rationale). We validate the enum value here before the write so an unknown
// literal surfaces as `Validation` (→ 422) rather than a `Db` 500.
// ---------------------------------------------------------------------------

/// Set or clear a task's `task_kind` (migration 0005). Task-scoped: a non-`task`
/// kind is rejected with a typed [`AppError::Validation`] (mirrors
/// [`set_effort`] / [`set_complexity`]). `task_kind = None` CLEARS the column to
/// NULL (deliberate divergence from the SET-OR-LEAVE convention — `task_kind`
/// is a discriminator the sprint composer may legitimately want to clear). One
/// event `work_item.task_kind_set`.
pub async fn set_task_kind(
    db: &impl DbClient,
    task_id: &str,
    task_kind: Option<TaskKind>,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "task_kind is settable only on a task, not on '{kind}'"
        )));
    }

    let value: Option<String> = task_kind.map(enum_to_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET task_kind = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![task_id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "task_id": task_id, "task_kind": value });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.task_kind_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// compute_tier (migration 0006). Pure function deriving the dispatch [`Tier`]
// from a task's spec (effort, complexity, files-touched count, cross-repo
// flag). No I/O, no async — consumed by the read-only `get_task_dispatch_plan`
// fold below.
// ---------------------------------------------------------------------------

/// Derive the dispatch [`Tier`] from a task's spec.
///
/// Rule (round-3, documented in CONVENTIONS.md §k of the
/// `lumina-story-blocks` plugin):
///
/// ```text
/// if complexity == "high":          Deep
/// if effort == "l":                 Deep
/// if files_touched_count > 3:       Deep
/// if has_cross_repo:                Deep
/// else:                             Lite
/// ```
///
/// `effort` and `complexity` are passed as `Option<&str>` to match the
/// row-struct idiom (free-text-in-row); unrecognised wire values fall through
/// to the residual `Lite` branch (defensive — the wire surface should reject
/// these at the MCP-param boundary via the typed [`Effort`] / [`Complexity`]
/// enums).
pub fn compute_tier(
    effort: Option<&str>,
    complexity: Option<&str>,
    files_touched_count: usize,
    has_cross_repo: bool,
) -> Tier {
    if complexity == Some("high") {
        return Tier::Deep;
    }
    if effort == Some("l") {
        return Tier::Deep;
    }
    if files_touched_count > 3 {
        return Tier::Deep;
    }
    if has_cross_repo {
        return Tier::Deep;
    }
    Tier::Lite
}

// ---------------------------------------------------------------------------
// compute_task_batches (migration 0005). Kahn's algorithm topological sort on
// the per-story task dependency graph. Read-only; no transaction, no events.
// ---------------------------------------------------------------------------

/// Sort-key projection of [`TaskKind`] for intra-phase tie-breaking. Foundation
/// tasks float to the earliest possible phase so the sprint composer's
/// "Phase 1 (foundation)" labelling is structural, not advisory. Polish tasks
/// sink to the latest position within their phase. NULL `task_kind` sorts at
/// the `Main` slot — treating an unlabelled task as "default main work" gives
/// stable foundation-before-everything-else-before-polish ordering without
/// penalising callers that haven't yet stamped a kind.
///
/// Pre-migration-0007 values (`vertical-slice`, `pattern-replacement`) cannot
/// be produced by any current Rust write path; should they survive in a
/// stale read (or arrive from a foreign data source bypassing the typed
/// enum), the catch-all arm puts them in the same `Main` bucket as NULL —
/// preserving the historical "neither foundation nor polish" intent without
/// reanimating the deprecated taxonomy.
fn task_kind_sort_key(s: Option<&str>) -> u8 {
    match s {
        Some("foundation") => 0,
        Some("main") => 1,
        Some("polish") => 2,
        _ => 1, // NULL or any legacy / unknown value → same slot as Main.
    }
}

/// Raw row read by [`compute_task_batches`]: a story's task children with the
/// `task_kind` discriminator carried alongside so the intra-phase sort avoids a
/// second query. Generic over `R: Row` per the canonical [`crate::db`] FromRow
/// recipe; `task_kind` is nullable (`Option<String>`), `id`/`created_at` are
/// NOT-NULL (`String`).
#[derive(Debug)]
struct TaskBatchRow {
    id: String,
    task_kind: Option<String>,
    created_at: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for TaskBatchRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(TaskBatchRow {
            id: row.try_get("id")?,
            task_kind: row.try_get("task_kind")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Compute task batches (phases) for a story via Kahn's topological sort.
/// Returns a `Vec` of phases, each phase a `Vec` of task ids whose dependencies
/// were satisfied by earlier phases. Within a phase, tasks sort by
/// `(task_kind ordering, created_at)` so foundation tasks rise to the earliest
/// phase as a deterministic tie-break.
///
/// Errors:
///   * `NotFound` — `story_id` does not exist.
///   * `Validation` — `story_id` exists but is not `kind='story'`.
///   * `Cycle { edges }` — the dependency graph contains a cycle; the residue
///     (edges that remain after Kahn's drains the zero-in-degree frontier) is
///     returned so the caller can surface the offending edges.
///
/// Read-only: no transaction, no events.
pub async fn compute_task_batches(
    pool: &impl DbClient,
    story_id: &str,
) -> Result<Vec<Vec<String>>, AppError> {
    // Validate the story exists and IS a story (NotFound vs Validation split).
    let kind = work_item_kind(pool, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "compute_task_batches expects a 'story', not a '{kind}'"
        )));
    }

    // Load all task children of the story, ordered by created_at for stable
    // intra-phase tie-breaking when task_kind is NULL on every task. We carry
    // `task_kind` alongside the id so the intra-phase sort can use it without
    // a second query.
    let tasks = pool
        .query_all::<TaskBatchRow>(
            r#"
        SELECT id, task_kind, created_at
        FROM work_items
        WHERE parent_id = $1
          AND kind = 'task'
          AND deleted_at IS NULL
        ORDER BY created_at, id
        "#,
            args![story_id.to_owned()],
        )
        .await?;

    if tasks.is_empty() {
        return Ok(Vec::new());
    }

    // Build the dependency graph: in_degree[v] = number of unsatisfied deps;
    // successors[u] = tasks that depend on u (so we can decrement their
    // in-degree when u is drained).
    use std::collections::HashMap;
    let task_ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    // Point-query-only (`.get(...)`), never iterated in order — HashMap is the
    // access-pattern-correct structure.
    let mut id_to_idx: HashMap<&str, usize> = HashMap::with_capacity(task_ids.len());
    for (i, id) in task_ids.iter().enumerate() {
        id_to_idx.insert(id.as_str(), i);
    }

    let edges = list_task_dependencies(pool, story_id).await?;
    let n = task_ids.len();
    let mut in_degree: Vec<usize> = vec![0; n];
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); n];

    for edge in &edges {
        // Both endpoints must be in the per-story task set; defensively skip
        // edges whose endpoints lie outside (should not happen given the
        // `list_task_dependencies` WHERE clause, but the index lookup is the
        // authority here).
        let Some(&u) = id_to_idx.get(edge.depends_on_id.as_str()) else {
            continue;
        };
        let Some(&v) = id_to_idx.get(edge.task_id.as_str()) else {
            continue;
        };
        successors[u].push(v);
        in_degree[v] += 1;
    }

    // Build the intra-phase sort key cache once.
    let sort_key: Vec<(u8, &str, &str)> = tasks
        .iter()
        .map(|t| (task_kind_sort_key(t.task_kind.as_deref()), t.created_at.as_str(), t.id.as_str()))
        .collect();

    // Kahn's: repeatedly drain the zero-in-degree frontier as a phase, then
    // decrement successors. Within each phase, sort by (task_kind, created_at,
    // id) so foundation tasks float earliest.
    let mut remaining: usize = n;
    let mut drained: Vec<bool> = vec![false; n];
    let mut phases: Vec<Vec<String>> = Vec::new();

    loop {
        let mut frontier: Vec<usize> = (0..n)
            .filter(|&i| !drained[i] && in_degree[i] == 0)
            .collect();
        if frontier.is_empty() {
            break;
        }
        frontier.sort_by(|&a, &b| sort_key[a].cmp(&sort_key[b]));

        let phase: Vec<String> = frontier.iter().map(|&i| task_ids[i].clone()).collect();
        for &i in &frontier {
            drained[i] = true;
            remaining -= 1;
            // `mem::take` consumes node `i`'s successor list once (it drains
            // exactly once when `i` is drained), sidestepping the borrow against
            // `in_degree` with zero allocation and no behaviour change.
            for j in std::mem::take(&mut successors[i]) {
                in_degree[j] = in_degree[j].saturating_sub(1);
            }
        }
        phases.push(phase);
    }

    if remaining > 0 {
        // Cycle: the residue is the set of undrained tasks plus the edges that
        // remain among them. Carry the offending edges (not just the task ids)
        // so the caller can render a precise error.
        let residue_edges: Vec<(String, String)> = edges
            .iter()
            .filter_map(|e| {
                let u = id_to_idx.get(e.depends_on_id.as_str())?;
                let v = id_to_idx.get(e.task_id.as_str())?;
                if !drained[*u] && !drained[*v] {
                    Some((e.task_id.clone(), e.depends_on_id.clone()))
                } else {
                    None
                }
            })
            .collect();
        return Err(AppError::Cycle { edges: residue_edges });
    }

    Ok(phases)
}

// ---------------------------------------------------------------------------
// get_task_dispatch_plan (migration 0006). Read-only fold that composes
// `compute_task_batches` with per-task spec reads (effort/complexity/
// files_touched) and runs `compute_tier` per row. Same outer shape as
// `compute_task_batches` (`Vec<Vec<…>>` — batches of tasks), but each entry is
// a [`BatchEntry`] carrying the derived [`Tier`] alongside the inputs.
// Read-only; no transaction, no events.
// ---------------------------------------------------------------------------

/// Raw row read in bulk by [`get_task_dispatch_plan`]: the `id` plus the spec
/// columns (`effort`/`complexity`/`attributes`) feeding [`compute_tier`].
/// Generic over `R: Row` per the canonical [`crate::db`] FromRow recipe; the
/// three spec columns are nullable (`Option<String>`), `id` is NOT NULL.
#[derive(Debug)]
struct DispatchSpecRow {
    id: String,
    effort: Option<String>,
    complexity: Option<String>,
    attributes: Option<String>,
}

impl<'r, R> sqlx::FromRow<'r, R> for DispatchSpecRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(DispatchSpecRow {
            id: row.try_get("id")?,
            effort: row.try_get("effort")?,
            complexity: row.try_get("complexity")?,
            attributes: row.try_get("attributes")?,
        })
    }
}

/// Story-level dispatch plan. Composes [`compute_task_batches`] with per-task
/// spec reads (effort/complexity/files_touched) and runs [`compute_tier`] per
/// row. Returns the same outer shape as `compute_task_batches`
/// (`Vec<Vec<…>>` — batches of tasks) but each entry is a [`BatchEntry`]
/// carrying the derived [`Tier`] alongside the inputs.
///
/// `has_cross_repo` is currently always reported as `false`: the round-3
/// scope did not add a `repo_link_id_for_slug` / `primary_repo_id_for_project`
/// helper to resolve `{repo, path}` entries in `attributes.files_touched`
/// against the project's `repo_links`. The other three Deep-triggering
/// branches of [`compute_tier`] still fire correctly; the cross-repo branch
/// remains dormant until a follow-up pass wires the slug→link resolver. The
/// MCP `set_task_spec` edge validates slugs at write time so a stored
/// `files_touched` entry referencing an unknown slug is unreachable.
///
/// Read-only: no transaction, no events. Cycles bubble out of the inner
/// `compute_task_batches` call as `AppError::Cycle`.
pub async fn get_task_dispatch_plan(
    pool: &impl DbClient,
    story_id: &str,
) -> Result<Vec<Vec<BatchEntry>>, AppError> {
    let batches = compute_task_batches(pool, story_id).await?;
    if batches.is_empty() {
        return Ok(Vec::new());
    }

    // ONE bulk read of every live task spec on this story (instead of a query
    // per task in the nested loop below). compute_task_batches loaded only
    // id/task_kind/created_at, so these spec columns were genuinely not
    // available before — keyed by id for the per-task lookup.
    let spec_rows = pool
        .query_all::<DispatchSpecRow>(
            r#"
        SELECT id, effort, complexity, attributes
        FROM work_items
        WHERE parent_id = $1 AND kind = 'task' AND deleted_at IS NULL
        "#,
            args![story_id.to_owned()],
        )
        .await?;
    let mut specs_by_id: std::collections::HashMap<String, DispatchSpecRow> =
        std::collections::HashMap::with_capacity(spec_rows.len());
    for row in spec_rows {
        specs_by_id.insert(row.id.clone(), row);
    }

    let mut out: Vec<Vec<BatchEntry>> = Vec::with_capacity(batches.len());
    for batch in batches {
        let mut entries: Vec<BatchEntry> = Vec::with_capacity(batch.len());
        for task_id in batch {
            // Look up the task spec for effort/complexity/attributes from the
            // bulk read. An id present in the batches but absent here is a
            // races-with-delete (the bulk read filters tombstoned rows) and
            // surfaces as `NotFound`, matching the prior per-task semantics.
            let row = specs_by_id
                .remove(&task_id)
                .ok_or_else(|| AppError::NotFound(format!("work_item '{task_id}' not found")))?;

            // Count `files_touched` entries (bare-string OR {repo,path}
            // objects). A malformed `attributes` blob is treated as 0 here
            // rather than re-erroring — `decode_attributes` is the
            // authoritative corruption-detector elsewhere; this read is a
            // best-effort summary.
            let files_touched_count: usize = match row.attributes.as_deref() {
                None => 0,
                Some(raw) => serde_json::from_str::<Value>(raw)
                    .ok()
                    .and_then(|v| v.get("files_touched").and_then(Value::as_array).map(Vec::len))
                    .unwrap_or(0),
            };

            // TODO(round-3 follow-up): wire a {repo,path} slug resolver
            // against the project ancestor's `repo_links` to flag a
            // `{repo, path}` entry whose slug != primary as cross-repo. No
            // helper exists in repo.rs yet — leaving this dormant; the other
            // three Deep-triggering branches still fire correctly.
            let has_cross_repo = false;

            let tier = if row.effort.is_none()
                && row.complexity.is_none()
                && files_touched_count == 0
                && !has_cross_repo
            {
                None
            } else {
                Some(compute_tier(
                    row.effort.as_deref(),
                    row.complexity.as_deref(),
                    files_touched_count,
                    has_cross_repo,
                ))
            };

            entries.push(BatchEntry {
                task_id,
                effort: row.effort,
                complexity: row.complexity,
                tier,
                files_touched_count,
                has_cross_repo,
            });
        }
        out.push(entries);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// set_task_tier (migration 0006). Single-mutation-path write to the
// `work_items.tier` column. Task-scope: rejects non-task rows at the Rust
// layer (no DB-level kind-coupling guard, matching `set_task_kind`).
// ---------------------------------------------------------------------------

/// Set the dispatch [`Tier`] on a task work-item. Updates the
/// `work_items.tier` column directly (NOT via attributes JSON — `tier` is a
/// real column added in migration 0006). Records one `work_item.tier_set`
/// event on the task's `work_item` aggregate. Rejects non-task rows at the
/// Rust layer (no DB-level kind-coupling guard, matching `set_task_kind`).
///
/// `tier == None` clears the column (writes NULL).
pub async fn set_task_tier(
    db: &impl DbClient,
    task_id: &str,
    tier: Option<Tier>,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "tier is settable only on a task, not on '{kind}'"
        )));
    }

    let value: Option<String> = tier.map(enum_to_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET tier = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
            args![task_id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "task_id": task_id, "tier": value });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.tier_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// set_task_lane (team-execution). Single-mutation-path write to the
// `work_items.lane` column. Task-scope: rejects non-task rows at the Rust
// layer (mirroring `set_task_tier` / `set_task_kind`). `lane` already has a
// create-time default of `'implement'` for tasks (see
// `create_work_item_full_tx`); this setter is the explicit re-stamp / clear
// path (the HTTP `PATCH /work-items/{id}/lane` + the MCP `set_task_lane` tool).
// The CHECK constraint accepts `NULL OR ('implement' | 'review')` — the typed
// [`Lane`] enum is the single source of the legal non-NULL values; we do NOT
// widen the vocab here (other lanes are a FUTURE migration).
// ---------------------------------------------------------------------------

/// Set or clear a task's work-queue [`Lane`] (team-execution). Task-scoped: a
/// non-`task` kind is rejected with a typed [`AppError::Validation`] (mirrors
/// [`set_task_tier`] / [`set_task_kind`]). `lane == None` CLEARS the column to
/// NULL (a laneless task is invisible to `claim_next_task` and never cascades a
/// review — the same composer-friendly nullable convention as `tier`/`task_kind`).
/// One event `work_item.lane_set`.
pub async fn set_task_lane(
    db: &impl DbClient,
    task_id: &str,
    lane: Option<Lane>,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "lane is settable only on a task, not on '{kind}'"
        )));
    }

    let value: Option<String> = lane.map(enum_to_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET lane = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
            args![task_id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "task_id": task_id, "lane": value });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.lane_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // compute_tier (migration 0006) — one test per branch of the §k rule.
    // -----------------------------------------------------------------------

    #[test]
    fn compute_tier_high_complexity_is_deep() {
        // complexity=high dominates even with otherwise-Lite inputs.
        assert_eq!(
            compute_tier(Some("s"), Some("high"), 0, false),
            Tier::Deep,
        );
    }

    #[test]
    fn compute_tier_l_effort_is_deep() {
        // effort=l dominates over a low/medium complexity.
        assert_eq!(
            compute_tier(Some("l"), Some("low"), 0, false),
            Tier::Deep,
        );
    }

    #[test]
    fn compute_tier_files_above_three_is_deep() {
        // files_touched_count > 3 — boundary just above the threshold.
        assert_eq!(
            compute_tier(Some("s"), Some("low"), 4, false),
            Tier::Deep,
        );
    }

    #[test]
    fn compute_tier_files_at_three_is_lite() {
        // Boundary: files_touched_count == 3 is NOT > 3, so Lite.
        assert_eq!(
            compute_tier(Some("s"), Some("low"), 3, false),
            Tier::Lite,
        );
    }

    #[test]
    fn compute_tier_cross_repo_is_deep() {
        // has_cross_repo flips an otherwise-Lite row to Deep.
        assert_eq!(
            compute_tier(Some("s"), Some("low"), 1, true),
            Tier::Deep,
        );
    }

    #[test]
    fn compute_tier_residual_is_lite() {
        // All Deep triggers absent → Lite.
        assert_eq!(
            compute_tier(Some("s"), Some("low"), 1, false),
            Tier::Lite,
        );
    }

    #[test]
    fn compute_tier_all_none_is_lite() {
        // Defensive: every input at its null-equivalent still falls through
        // to the residual Lite branch (the wire surface gates None upstream).
        assert_eq!(compute_tier(None, None, 0, false), Tier::Lite);
    }

    // -----------------------------------------------------------------------
    // lane as a first-class task field (team-execution): the create-time
    // default + the `set_task_lane` setter. These need a DB, so they pull in
    // the in-memory pool + the seed-chain helper (mirroring team_execution.rs).
    // -----------------------------------------------------------------------

    use crate::db::AnyPool;
    use crate::db::connect_in_memory;
    use crate::repo::test_support::seed_chain_to_story;

    /// Read a single work item's `lane` column for assertions.
    async fn lane_of(pool: &sqlx::SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT lane FROM work_items WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("lane select")
    }

    /// (a) A freshly-created TASK defaults to `lane='implement'` (the default
    /// lives in the shared INSERT). An explicit `lane='review'` override is
    /// honoured. A NON-task kind (a story here) keeps `lane=NULL` (lane is
    /// task-only). The simple `create_work_item` helper (no opts) inherits the
    /// default WITHOUT any signature change.
    #[tokio::test]
    async fn create_default_lane_is_implement_for_tasks_only() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // A task created via the simple no-opts helper defaults to 'implement'.
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        assert_eq!(
            lane_of(&pool, &task).await.as_deref(),
            Some("implement"),
            "a fresh task defaults to lane='implement'"
        );

        // An explicit lane override (via the full opts path) is honoured.
        let review_task = create_work_item_full(
            &pool,
            "task",
            Some(&story),
            "R",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: None,
                lane: Some(Lane::Review),
            },
        )
        .await
        .expect("review-lane task")
        .to_string();
        assert_eq!(
            lane_of(&pool, &review_task).await.as_deref(),
            Some("review"),
            "an explicit lane override is honoured at create"
        );

        // A non-task kind (the story itself) keeps lane=NULL.
        assert_eq!(
            lane_of(&pool, &story).await,
            None,
            "a non-task kind keeps lane=NULL (lane is task-only)"
        );
    }

    /// (b) `set_task_lane` sets a task's lane, clears it to NULL on `None`, and
    /// rejects a non-task target with `Validation`.
    #[tokio::test]
    async fn set_task_lane_sets_clears_and_kind_gates() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        // Re-stamp to 'review'.
        set_task_lane(&db, &task, Some(Lane::Review))
            .await
            .expect("set lane");
        assert_eq!(lane_of(&pool, &task).await.as_deref(), Some("review"));

        // Clear to NULL.
        set_task_lane(&db, &task, None).await.expect("clear lane");
        assert_eq!(lane_of(&pool, &task).await, None, "None clears the lane");

        // Kind gate: a non-task (the story) is rejected with Validation.
        let err = set_task_lane(&db, &story, Some(Lane::Implement))
            .await
            .expect_err("non-task lane set must error");
        assert!(
            matches!(err, AppError::Validation(_)),
            "a non-task lane set is a Validation error, got {err:?}"
        );

        // A missing id is NotFound (the task_id must reference a row). The kind
        // read fails first with NotFound for an absent id.
        let missing = set_task_lane(&db, "no-such-id", Some(Lane::Implement))
            .await
            .expect_err("missing id must error");
        assert!(
            matches!(missing, AppError::NotFound(_)),
            "a missing id is NotFound, got {missing:?}"
        );
    }
}
