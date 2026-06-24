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
use super::events::{record_event, record_inert_event};
use crate::args;
use crate::db::DbClient;
use crate::domain::{BatchEntry, GatingTier, Lane, TaskKind, Tier};
use crate::error::AppError;

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
// compute_gating_tier (migration 0026, story-planning-round-5). Pure function
// deriving the HUMAN-GATING [`GatingTier`] from a story's signals. The SINGLE
// SOURCE of the gating rule (mirroring `compute_tier`'s single-source role for
// the dispatch tier); consumed by `get_story_readiness` and, in T4, the
// `get_gating_tier` MCP tool.
// ---------------------------------------------------------------------------

/// Derive the [`GatingTier`] from a story's signals (story-planning-round-5,
/// User Decision 2).
///
/// Rule (evaluated in this EXACT order):
///
/// ```text
/// if spawned_from_finding && complexity != high && unresolved_questions == 0:
///                                                        Autonomous
/// else if complexity == high || unresolved_questions > 0 || scope_files > 6:
///                                                        Full
/// else:                                                  Light
/// ```
///
/// Note (User Decision 2): `scope_files` does NOT guard the `Autonomous`
/// branch — a finding-spawned, non-high-complexity, fully-resolved story is
/// autonomous regardless of how many files it touches. `scope_files > 6` only
/// promotes an OTHERWISE-Light story to `Full`.
///
/// `complexity` is passed as `Option<&str>` to match the row-struct idiom
/// (free-text-in-row, like [`compute_tier`]); an unrecognised / `None` value is
/// treated as NOT high (so it never forces `Full` on the complexity axis nor
/// blocks the `Autonomous` branch — the high check is an exact `Some("high")`).
pub fn compute_gating_tier(
    spawned_from_finding: bool,
    complexity: Option<&str>,
    unresolved_questions: i64,
    scope_files: i64,
) -> GatingTier {
    let is_high = complexity == Some("high");
    if spawned_from_finding && !is_high && unresolved_questions == 0 {
        return GatingTier::Autonomous;
    }
    if is_high || unresolved_questions > 0 || scope_files > 6 {
        return GatingTier::Full;
    }
    GatingTier::Light
}

// ---------------------------------------------------------------------------
// Round-5 rework mutators (migration 0026): the plan-epoch bump + the persisted
// task<->research grounding edge writers. Each is a single-mutation-path write —
// ONE `BEGIN IMMEDIATE` tx recording EXACTLY ONE EXPORT-INERT event on the
// `plan_epoch` aggregate (never `work_item`), so the bump/link/retire never
// re-renders a work_item git-export snapshot. The epoch COLUMN still rides the
// work_item snapshot (it is a `work_items` column); only the bump EVENT is inert.
// ---------------------------------------------------------------------------

/// Bump a story's rework plan epoch (migration 0026). One `BEGIN IMMEDIATE` txn:
/// `UPDATE work_items SET plan_epoch = plan_epoch + 1 WHERE id = :story_id`
/// (`rows_affected()==0 ⇒ NotFound`), then ONE export-inert `plan_epoch`-aggregate
/// event (`plan_epoch.bumped`, payload `{from, to}`), commit. Returns the NEW
/// epoch.
///
/// EXPORT-INERT by deliberate trade-off: the `plan_epoch` column rides the
/// work_item snapshot (it is a real `work_items` column the export renders), but
/// the bump itself emits no `work_item` event — a rework bump is bookkeeping, not
/// a planning-content change, so it does not warrant re-rendering the whole item.
///
/// Story-kind-gated (a non-`story` kind is rejected with a typed
/// [`AppError::Validation`]): the rework pass bumps a STORY, the MCP param is
/// `story_id`, and only the story-gated readers (`StoryReadiness` / the dossier)
/// consume `plan_epoch` — so bumping a non-story would write dead state no reader
/// observes. Gating here keeps the round-5 surfaces consistent (every other one —
/// `get_story_dossier` / `get_gating_tier` / `get_story_readiness` — is
/// story-gated) and matches the sibling setters in this file
/// ([`set_task_tier`] / [`set_task_lane`] / [`set_task_kind`]). `NotFound` on a
/// missing/zero-row id; `Validation` on a present non-story row.
pub async fn bump_plan_epoch(db: &impl DbClient, story_id: &str) -> Result<i64, AppError> {
    // Story-scoped (also NotFound if the id is absent — the kind read fails
    // first). `work_item_kind` does NOT filter `deleted_at`, so the explicit
    // tombstone guard on the SELECT/UPDATE below is still required for parity
    // with the sibling setters.
    let kind = work_item_kind(db, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "plan_epoch is bumpable only on a story, not on '{kind}'"
        )));
    }

    let mut tx = db.begin().await?;

    // Read the current epoch on the tx (same writer snapshot as the UPDATE) so
    // the event payload's `from`/`to` are consistent and a missing row is caught
    // before the write. The `deleted_at IS NULL` guard matches the sibling
    // setters ([`set_task_tier`] / [`set_task_lane`]) so a tombstoned story's
    // epoch is never advanced. `plan_epoch` is `NOT NULL DEFAULT 0`, so a present
    // live row always decodes a non-null `i64`.
    let from: Option<i64> = crate::db::tx_scalar_opt::<i64>(
        tx.as_mut(),
        "SELECT plan_epoch FROM work_items WHERE id = $1 AND deleted_at IS NULL",
        args![story_id.to_owned()],
    )
    .await?;
    let Some(from) = from else {
        return Err(AppError::NotFound(format!("work_item '{story_id}' not found")));
    };

    let affected = tx
        .execute(
            r#"UPDATE work_items SET plan_epoch = plan_epoch + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
            args![story_id.to_owned()],
        )
        .await?;
    if affected == 0 {
        // Concurrent-delete race between the read and the UPDATE.
        return Err(AppError::NotFound(format!("work_item '{story_id}' not found")));
    }

    let to = from + 1;
    let payload = serde_json::json!({ "from": from, "to": to });
    record_inert_event(tx.as_mut(), "plan_epoch", story_id, "plan_epoch.bumped", payload).await?;

    tx.commit().await?;
    Ok(to)
}

/// Resolve a research note's owning STORY id — the note's `work_item_id` resolved
/// to its story ancestor. Used by the grounding writers to (a) cross-story-check
/// the link and (b) route the inert event to the story aggregate so it groups
/// with the story. `NotFound` if the note id has no row.
async fn research_note_story(db: &impl DbClient, note_id: &str) -> Result<String, AppError> {
    let owner = crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM research_notes WHERE id = $1",
        args![note_id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("research_note '{note_id}' not found")))?;

    // A research note lives on a STORY today (the planning skills attach notes to
    // a story); resolve to the story ancestor so cross-story comparison is
    // apples-to-apples even if a note ever hangs off a sub-row. If the owner IS
    // the story this returns it unchanged.
    story_ancestor(db, &owner).await
}

/// Resolve a work item's STORY ancestor id (self if it IS a story), via the
/// `parent_id` chain. `NotFound` if the start id is absent; `Validation` if the
/// chain bottoms out before any `story` row (defensive — unreachable for items
/// created via `create_work_item`).
async fn story_ancestor(db: &impl DbClient, work_item_id: &str) -> Result<String, AppError> {
    let found: Option<String> = crate::db::scalar_opt::<String>(
        db,
        r#"
        WITH RECURSIVE ancestors(id, kind, parent_id) AS (
            SELECT id, kind, parent_id FROM work_items WHERE id = $1
            UNION ALL
            SELECT w.id, w.kind, w.parent_id
            FROM work_items w
            JOIN ancestors a ON w.id = a.parent_id
        )
        SELECT id FROM ancestors WHERE kind = 'story' LIMIT 1
        "#,
        args![work_item_id.to_owned()],
    )
    .await?;
    if let Some(id) = found {
        return Ok(id);
    }
    let exists = crate::db::scalar_opt::<i64>(
        db,
        r#"SELECT 1 FROM work_items WHERE id = $1"#,
        args![work_item_id.to_owned()],
    )
    .await?
    .is_some();
    if !exists {
        Err(AppError::NotFound(format!("work_item '{work_item_id}' not found")))
    } else {
        Err(AppError::Validation(format!(
            "work_item '{work_item_id}' has no 'story' ancestor"
        )))
    }
}

/// Persist a task↔research grounding edge (migration 0026): INSERT one
/// `task_research_links(task_id, research_note_id)` row so a task's research
/// provenance survives as a QUERYABLE edge, not just prose ("T4 implements
/// R-note X"). One `BEGIN IMMEDIATE` txn + ONE export-inert `plan_epoch`-aggregate
/// event (`task_research.linked`), routed to the task's STORY id so it groups
/// with the story.
///
/// **Validation lives HERE (NOT at the MCP layer)** so the T5 HTTP mirror inherits
/// it (single source):
///   * `task_id` must be `kind='task'` (else `Validation`; also `NotFound` if the
///     id is absent — the kind read fails first);
///   * `research_note_id` must EXIST, be LIVE (`superseded_by IS NULL`), AND belong
///     to the SAME story as the task (the task's parent story vs the note's
///     story ancestor must match; a cross-story link is `Validation`).
///
/// IDEMPOTENT: the composite PK `(task_id, research_note_id)` makes a re-link a
/// no-op success (`ON CONFLICT DO NOTHING` — the event still records the intent,
/// matching the "one logical write ⇒ one event" envelope without a spurious second
/// row).
pub async fn link_task_research(
    db: &impl DbClient,
    task_id: &str,
    research_note_id: &str,
) -> Result<(), AppError> {
    // Task-scoped (also NotFound if the id is absent).
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "link_task_research links a research note only to a task, not to a '{kind}'"
        )));
    }

    // The note must EXIST and be LIVE (a superseded note is not a valid current
    // grounding). One read covers both: an absent OR superseded note yields no row.
    let live = db
        .query_opt::<crate::db::Scalar<i64>>(
            "SELECT 1 FROM research_notes WHERE id = $1 AND superseded_by IS NULL",
            args![research_note_id.to_owned()],
        )
        .await?
        .is_some();
    if !live {
        // Distinguish absent from superseded for a precise message.
        let exists = db
            .query_opt::<crate::db::Scalar<i64>>(
                "SELECT 1 FROM research_notes WHERE id = $1",
                args![research_note_id.to_owned()],
            )
            .await?
            .is_some();
        return Err(AppError::Validation(if exists {
            format!("research_note '{research_note_id}' is superseded and cannot ground a task")
        } else {
            format!("research_note '{research_note_id}' does not exist")
        }));
    }

    // Same-story check: the task's parent story vs the note's story ancestor.
    let task_story = story_ancestor(db, task_id).await?;
    let note_story = research_note_story(db, research_note_id).await?;
    if task_story != note_story {
        return Err(AppError::Validation(format!(
            "research_note '{research_note_id}' belongs to story '{note_story}', not the task's \
             story '{task_story}' — a grounding edge must stay within one story"
        )));
    }

    let mut tx = db.begin().await?;

    tx.execute(
        r#"
        INSERT INTO task_research_links (task_id, research_note_id)
        VALUES ($1, $2)
        ON CONFLICT (task_id, research_note_id) DO NOTHING
        "#,
        args![task_id.to_owned(), research_note_id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({
        "task_id": task_id,
        "research_note_id": research_note_id,
    });
    record_inert_event(tx.as_mut(), "plan_epoch", &task_story, "task_research.linked", payload)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// Remove a task↔research grounding edge (migration 0026). `pub(crate)` —
/// repo-internal, NOT MCP-surfaced; the rework/cancel path uses it to drop a
/// grounding when a note is superseded or a task is re-planned. One
/// `BEGIN IMMEDIATE` txn + ONE export-inert `plan_epoch`-aggregate event
/// (`task_research.unlinked`), routed to the task's story.
///
/// Idempotent: a missing edge is a no-op success (`rows_affected()==0` is NOT an
/// error — unlinking an absent edge already achieves the intent), but it still
/// records the event so the audit trail captures the rework intent uniformly with
/// [`link_task_research`]. The task must still be a `task` (kind-gated, mirroring
/// the link writer) so the event-aggregate resolution is well-defined.
//
// `allow(dead_code)`: this is the repo-internal unlink primitive for the
// rework/cancel path, which is NOT wired in this pass (T3 lands the writers; the
// consumer is a later task). It is exercised by the dossier rework test only — a
// `#[cfg(test)]`-only use does not clear the non-test dead-code lint — so the
// allow stays until the production consumer lands.
#[allow(dead_code)]
pub(crate) async fn unlink_task_research(
    db: &impl DbClient,
    task_id: &str,
    research_note_id: &str,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "unlink_task_research unlinks a research note only from a task, not from a '{kind}'"
        )));
    }
    // Resolve the story aggregate before the write (the edge may already be gone,
    // but the task still exists per the kind read above).
    let task_story = story_ancestor(db, task_id).await?;

    let mut tx = db.begin().await?;

    tx.execute(
        r#"DELETE FROM task_research_links WHERE task_id = $1 AND research_note_id = $2"#,
        args![task_id.to_owned(), research_note_id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({
        "task_id": task_id,
        "research_note_id": research_note_id,
    });
    record_inert_event(tx.as_mut(), "plan_epoch", &task_story, "task_research.unlinked", payload)
        .await?;

    tx.commit().await?;
    Ok(())
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
/// columns (`effort`/`complexity`) feeding [`compute_tier`]. The
/// `files_touched_count` no longer comes from the attributes JSON — it is the
/// de-duplicated EXPECTED `task_files` count (migration 0020, T7), read
/// separately below — so `attributes` is no longer projected here.
/// Generic over `R: Row` per the canonical [`crate::db`] FromRow recipe; the
/// two spec columns are nullable (`Option<String>`), `id` is NOT NULL.
#[derive(Debug)]
struct DispatchSpecRow {
    id: String,
    effort: Option<String>,
    complexity: Option<String>,
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
        })
    }
}

/// Raw row read in bulk by [`get_task_dispatch_plan`]: a task id paired with
/// its de-duplicated EXPECTED `task_files` count (migration 0020, T7). The
/// count is `COUNT(DISTINCT COALESCE(repo_link_id,'') || '\u{1}' || path)` over
/// the task's `kind='expected'` rows — DISTINCT on the CANONICAL
/// `(repo_link_id, path)` key so a bare path and an explicit-primary
/// `{repo, path}` for the SAME primary repo (stored as NULL vs a primary
/// repo_link_id) collapse to ONE file (Ground R2/R3). `id` is NOT NULL; `n`
/// is a NOT-NULL aggregate.
#[derive(Debug)]
struct ExpectedCountRow {
    id: String,
    n: i64,
}

impl<'r, R> sqlx::FromRow<'r, R> for ExpectedCountRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ExpectedCountRow {
            id: row.try_get("id")?,
            n: row.try_get("n")?,
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
        SELECT id, effort, complexity
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

    // ONE bulk read of the de-duplicated EXPECTED `task_files` count per task on
    // this story (migration 0020, T7) — the new `files_touched_count` source.
    // The count is DISTINCT on the CANONICAL `(repo_link_id, path)` key
    // (`COALESCE(repo_link_id,'') || sep || path`) so a bare path and an
    // explicit-primary `{repo, path}` for the SAME primary repo (stored as NULL
    // vs a primary repo_link_id) collapse to ONE file — keeping `compute_tier`
    // STABLE (a borderline task is NOT spuriously over-promoted to `deep` by
    // `files>3`; Ground R2/R3). We dedup in SQL (a single grouped read) rather
    // than folding rows through `canonical_file_key` in Rust: the canonical key
    // for a STORED row is exactly `COALESCE(repo_link_id,'')` (the storage
    // UNIQUE-index bucket — see `task_files.rs`), so the SQL form is the simpler
    // correct one and needs no per-task repo_links lookup. `char(1)` is an
    // unprintable separator that cannot appear in a repo_link_id (a uuid) or a
    // path, so the concatenated key is unambiguous. Tasks with no expected rows
    // are absent here and default to 0 below.
    let count_rows = pool
        .query_all::<ExpectedCountRow>(
            r#"
        SELECT tf.task_id AS id,
               COUNT(DISTINCT COALESCE(tf.repo_link_id, '') || char(1) || tf.path) AS n
        FROM task_files tf
        JOIN work_items w ON w.id = tf.task_id
        WHERE w.parent_id = $1
          AND w.kind = 'task'
          AND w.deleted_at IS NULL
          AND tf.kind = 'expected'
        GROUP BY tf.task_id
        "#,
            args![story_id.to_owned()],
        )
        .await?;
    let mut expected_count_by_id: std::collections::HashMap<String, usize> =
        std::collections::HashMap::with_capacity(count_rows.len());
    for row in count_rows {
        expected_count_by_id.insert(row.id, row.n.max(0) as usize);
    }

    let mut out: Vec<Vec<BatchEntry>> = Vec::with_capacity(batches.len());
    for batch in batches {
        let mut entries: Vec<BatchEntry> = Vec::with_capacity(batch.len());
        for task_id in batch {
            // Look up the task spec for effort/complexity from the bulk read.
            // An id present in the batches but absent here is a
            // races-with-delete (the bulk read filters tombstoned rows) and
            // surfaces as `NotFound`, matching the prior per-task semantics.
            let row = specs_by_id
                .remove(&task_id)
                .ok_or_else(|| AppError::NotFound(format!("work_item '{task_id}' not found")))?;

            // `files_touched_count` is the DE-DUPLICATED EXPECTED `task_files`
            // count (migration 0020, T7) — distinct canonical `(repo_link_id,
            // path)` keys among the task's `kind='expected'` rows (computed in
            // the grouped SQL read above). A bare path and an explicit-primary
            // `{repo, path}` for the same primary repo count as ONE file, so a
            // borderline task is not spuriously over-promoted to `deep`
            // (Ground R2/R3). A task with no expected rows defaults to 0.
            let files_touched_count: usize =
                expected_count_by_id.get(&task_id).copied().unwrap_or(0);

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

    // Done-is-terminal guard (1B-F9 M4): reject flagging a `done` task INTO the
    // review lane. `done` is terminal — a completed task (lite→done, or a
    // reviewer's review→done close) can never be flagged back for re-review;
    // that requires a brand-NEW task (edge-case 019ed5fc-3df0 / not_doing #4).
    // Mirrors the `done → review` STATUS guard in `update_work_item_status`: the
    // status guard stops the dangerous re-claim (the claim needs status='review'
    // AND lane='review'), this lane guard rejects the flag at its own surface so
    // the intent never even half-applies. Read on the tx (same writer snapshot);
    // a missing row falls through to the `affected == 0` NotFound below.
    if matches!(lane, Some(Lane::Review)) {
        let current: Option<String> = crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT status FROM work_items WHERE id = $1 AND deleted_at IS NULL",
            args![task_id.to_owned()],
        )
        .await?;
        if current.as_deref() == Some("done") {
            return Err(AppError::Validation(format!(
                "work_item '{task_id}' is done; done is terminal and cannot be flagged into \
                 the review lane — create a new task to re-review completed work"
            )));
        }
    }

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
    // compute_gating_tier (migration 0026) — one test per branch + boundary
    // of the round-5 gating rule (User Decision 2).
    // -----------------------------------------------------------------------

    #[test]
    fn gating_tier_finding_low_complexity_no_questions_is_autonomous() {
        // The autonomous branch: finding-spawned, NOT high complexity, zero
        // unresolved questions. scope_files is irrelevant here (User Decision 2),
        // so a large file count must NOT demote it from Autonomous.
        assert_eq!(
            compute_gating_tier(true, Some("low"), 0, 99),
            GatingTier::Autonomous,
        );
    }

    #[test]
    fn gating_tier_high_complexity_is_full() {
        // complexity=high forces Full even when finding-spawned (the autonomous
        // branch requires complexity != high, so a high-complexity finding-spawned
        // story falls through to the Full check).
        assert_eq!(
            compute_gating_tier(true, Some("high"), 0, 0),
            GatingTier::Full,
        );
    }

    #[test]
    fn gating_tier_unresolved_questions_is_full() {
        // unresolved_questions > 0 forces Full and also blocks the autonomous
        // branch (a finding-spawned story with an open question is NOT autonomous).
        assert_eq!(
            compute_gating_tier(true, Some("low"), 1, 0),
            GatingTier::Full,
        );
    }

    #[test]
    fn gating_tier_scope_files_above_six_is_full() {
        // scope_files > 6 promotes an otherwise-Light story to Full. NOT
        // finding-spawned, so the autonomous branch never applies.
        assert_eq!(
            compute_gating_tier(false, Some("low"), 0, 7),
            GatingTier::Full,
        );
    }

    #[test]
    fn gating_tier_scope_files_at_six_is_light() {
        // Boundary: scope_files == 6 is NOT > 6, so the residual Light branch
        // (not finding-spawned, low complexity, no questions).
        assert_eq!(
            compute_gating_tier(false, Some("low"), 0, 6),
            GatingTier::Light,
        );
    }

    #[test]
    fn gating_tier_residual_is_light() {
        // Every Full trigger absent AND not finding-spawned → Light.
        assert_eq!(
            compute_gating_tier(false, Some("medium"), 0, 3),
            GatingTier::Light,
        );
    }

    #[test]
    fn gating_tier_dossier_serde_roundtrip() {
        // GatingTier serialises to the snake_case wire form and round-trips back
        // (T4's get_gating_tier returns it via Json<T>; the wire spelling is
        // load-bearing). Named `dossier` so the round-5 narrow filter picks it up.
        for (variant, wire) in [
            (GatingTier::Full, "\"full\""),
            (GatingTier::Light, "\"light\""),
            (GatingTier::Autonomous, "\"autonomous\""),
        ] {
            let json = serde_json::to_string(&variant).expect("serialize GatingTier");
            assert_eq!(json, wire, "{variant:?} serialises to {wire}");
            let back: GatingTier = serde_json::from_str(&json).expect("deserialize GatingTier");
            assert_eq!(back, variant, "{wire} round-trips to {variant:?}");
        }
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

    // -----------------------------------------------------------------------
    // get_task_dispatch_plan files_touched_count re-sourced from the DEDUPED
    // EXPECTED task_files count (migration 0020, T7). AC (a): a task whose
    // EXPECTED set names the SAME primary file as both a bare path and an
    // explicit-primary {repo, path} counts as ONE file — so a borderline task
    // is NOT spuriously over-promoted to `deep` by `files>3`.
    // -----------------------------------------------------------------------

    /// (a) The dispatch-plan tier reads the de-duplicated EXPECTED count. A task
    /// whose EXPECTED set carries the same primary-repo file as `"x"` AND
    /// `{repo: <primary>, path: "x"}` yields `files_touched_count == 1` (the two
    /// spellings fold to one canonical key — Ground R2/R3), so a task that would
    /// be Deep at a raw count of 5 stays `Lite` once deduped to ≤3 files.
    #[tokio::test]
    async fn dispatch_plan_files_touched_count_is_deduped_so_tier_stays_lite() {
        use crate::domain::{Complexity, Effort};

        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;

        // Attach a PRIMARY repo link to the project so an explicit-primary
        // {repo, path} entry resolves (and folds to the NULL bucket).
        let project = find_project_ancestor(&pool, &story)
            .await
            .expect("project ancestor");
        add_repo_link(&pool, &project, "octocat/hello-world", true)
            .await
            .expect("primary repo link");

        // A task with otherwise-Lite spec (effort=s, complexity=low). Without
        // dedup, the EXPECTED set below would count as 5 entries (>3 ⇒ Deep);
        // with canonical dedup the bare+explicit-primary "x" fold to one, so the
        // distinct count is 3 (x, y, z) ⇒ NOT > 3 ⇒ stays Lite.
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        set_effort(&db, &task, Effort::S).await.expect("effort");
        set_complexity(&db, &task, Complexity::Low).await.expect("complexity");

        set_task_expected_files(
            &db,
            &task,
            &[
                serde_json::json!("src/x.rs"),
                serde_json::json!({ "repo": "octocat/hello-world", "path": "src/x.rs" }),
                serde_json::json!("src/y.rs"),
                serde_json::json!("src/z.rs"),
                // A second explicit-primary spelling of x — also folds.
                serde_json::json!({ "repo": "octocat/hello-world", "path": "src/x.rs" }),
            ],
        )
        .await
        .expect("set expected files");

        let plan = get_task_dispatch_plan(&db, &story)
            .await
            .expect("dispatch plan");
        let entry = plan
            .iter()
            .flatten()
            .find(|e| e.task_id == task)
            .expect("the task is in the dispatch plan");

        assert_eq!(
            entry.files_touched_count, 3,
            "the bare + explicit-primary spellings of src/x.rs fold to one canonical key, \
             so the deduped EXPECTED count is 3 (x, y, z), not 5"
        );
        assert_eq!(
            entry.tier,
            Some(Tier::Lite),
            "a deduped count of 3 is NOT > 3, so the borderline task stays Lite (not over-promoted to Deep)"
        );
    }

    /// A NON-primary explicit `{repo, path}` for the same path is a DISTINCT
    /// canonical key, so it counts SEPARATELY — the dedup is primary-specific,
    /// not a blanket path collapse. With `"x"` (primary) + `{repo: secondary,
    /// path: "x"}` the EXPECTED count is 2.
    #[tokio::test]
    async fn dispatch_plan_count_keeps_nonprimary_repo_distinct() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;

        let project = find_project_ancestor(&pool, &story)
            .await
            .expect("project ancestor");
        add_repo_link(&pool, &project, "octocat/hello-world", true)
            .await
            .expect("primary repo link");
        add_repo_link(&pool, &project, "octocat/other-repo", false)
            .await
            .expect("secondary repo link");

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        set_task_expected_files(
            &db,
            &task,
            &[
                serde_json::json!("src/x.rs"),
                serde_json::json!({ "repo": "octocat/other-repo", "path": "src/x.rs" }),
            ],
        )
        .await
        .expect("set expected files");

        let plan = get_task_dispatch_plan(&db, &story)
            .await
            .expect("dispatch plan");
        let entry = plan
            .iter()
            .flatten()
            .find(|e| e.task_id == task)
            .expect("the task is in the dispatch plan");

        assert_eq!(
            entry.files_touched_count, 2,
            "a non-primary {{repo, path}} keeps a distinct canonical key, so x@primary and \
             x@secondary count as two files (the dedup is primary-specific, not blanket)"
        );
    }
}
