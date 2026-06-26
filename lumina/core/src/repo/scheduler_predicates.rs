//! Scheduler TRIGGER PREDICATES (migration 0028, focus 1C.3): the read-only
//! SELECTs that DECIDE which `scheduled_units` rows the in-process tokio
//! scheduler should CREATE. This is the sibling half of `repo/scheduler.rs` —
//! that module operates on rows that ALREADY exist in `scheduled_units` (the
//! claim/lease lifecycle); THIS module finds the work items whose state matches
//! a build trigger and returns the dispatchable candidates the scheduler loop
//! will turn into `scheduled_units` rows.
//!
//! There are THREE trigger predicates, evaluated in a DETERMINISTIC PRIORITY
//! order so one pipeline STAGE advances per scan — mirroring the kind-priority
//! the sibling `claim_next_scheduled_unit` orders its candidate set by
//! (`build_story` → `build_tasks` → `compose_sprint` → `drive`):
//!
//!   1. **build_story** — STUB backlog stories: a `kind='story'` whose
//!      `relevance='backlog'`, whose `attributes.problem_statement` is set
//!      (non-empty), and which has NO children (a framed-but-not-decomposed
//!      story).
//!   2. **build_tasks** — "APPROVED" stories. There is NO `Approved` Status
//!      variant in this codebase (see `domain/enums.rs`), so the INTERIM
//!      predicate decided during planning is `relevance='active'` AND
//!      `ready_for_decomposition`, where `ready_for_decomposition` is derived
//!      the SAME way [`super::get_story_readiness`] derives it — by REUSING that
//!      function verbatim (no divergent re-implementation).
//!   3. **compose_sprint** — stories with ≥1 READY (open, dependency-satisfied,
//!      unparked, unleased) task not yet bound to any sprint. The per-task
//!      readiness predicate is held BYTE-CONSISTENT with `claim_next_task` /
//!      `get_sprint_quiescence` (sans the lane/tier filters), plus a "not in a
//!      sprint" `NOT EXISTS (sprint_tasks)` clause.
//!
//! **Active-ancestor gating (applies to ALL three predicates).** A candidate is
//! dispatchable ONLY IF every ancestor up the chain to the root is `active` —
//! the scheduler dispatches only UNDER active ancestors. Implemented by
//! [`all_ancestors_active`], which adapts the [`super::find_project_ancestor`]
//! recursive-CTE shape into an "is any ancestor non-active?" probe (a NULL-safe
//! `relevance <> 'active'` — the project root and any unset relevance carry NULL
//! and so never block; only an explicit `backlog`/`deferred`/`rejected` ancestor
//! does).
//!
//! Runtime `sqlx::query*` only — routed through the [`crate::db::DbClient`] read
//! seam like the other reads in `repo/reads.rs`; no `query!`/`query_as!` bang
//! macros (the macro-eradication gate). Read-only throughout: no transaction, no
//! events.

use crate::args;
use crate::db::DbClient;
use crate::domain::ScheduledUnitKind;
use crate::error::AppError;

use super::get_story_readiness;

/// One dispatchable trigger candidate: the trigger [`ScheduledUnitKind`] and the
/// `work_items.id` whose state matched it. The scheduler loop turns each into a
/// `scheduled_units` row (kind + work_item_id); `trigger_kind` is exactly the
/// `scheduled_units.kind` dispatch vocab the loop writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerCandidate {
    /// The matched trigger kind (= the `scheduled_units.kind` to create).
    pub trigger_kind: ScheduledUnitKind,
    /// The story/sprint-scale `work_items.id` the unit will drive.
    pub work_item_id: String,
}

/// `true` iff NO ancestor of `work_item_id` (EXCLUDING the item itself) on the
/// path to the root carries a relevance other than `'active'` — i.e. the
/// candidate sits entirely under active ancestors and is therefore dispatchable.
///
/// Adapts [`super::find_project_ancestor`]'s recursive-CTE shape: seed with the
/// candidate row, walk `parent_id` to the root, then look for ANY ancestor
/// (`id <> $1` excludes the candidate) whose `relevance` is explicitly set and
/// NOT `'active'`. The `relevance IS NOT NULL AND relevance <> 'active'` form is
/// NULL-safe: `relevance` is only ever set on epic/focus/story, so the project
/// root (and any task / unset row) carries NULL and never blocks — only an
/// explicit `backlog`/`deferred`/`rejected` ancestor does. A found blocker ⇒
/// `false`; none ⇒ `true`. A missing `work_item_id` yields an empty CTE ⇒ no
/// blocker ⇒ `true` (a non-existent candidate never reaches here from the
/// predicates, which select live rows). Read-only.
pub async fn all_ancestors_active(
    db: &impl DbClient,
    work_item_id: &str,
) -> Result<bool, AppError> {
    let blocker: Option<i64> = crate::db::scalar_opt::<i64>(
        db,
        r#"
        WITH RECURSIVE ancestors(id, parent_id, relevance) AS (
            SELECT id, parent_id, relevance FROM work_items WHERE id = $1
            UNION ALL
            SELECT w.id, w.parent_id, w.relevance
            FROM work_items w
            JOIN ancestors a ON w.id = a.parent_id
        )
        SELECT 1 FROM ancestors
        WHERE id <> $1
          AND relevance IS NOT NULL
          AND relevance <> 'active'
        LIMIT 1
        "#,
        args![work_item_id.to_owned()],
    )
    .await?;
    Ok(blocker.is_none())
}

/// Retain only those `ids` whose every ancestor is `active` (the shared gate the
/// three predicates apply). One [`all_ancestors_active`] probe per id — O(n)
/// reads for a small candidate set, mirroring the per-task readiness loop in
/// `get_story_readiness`. Order is preserved (each predicate's SQL already
/// orders by `created_at, id`).
async fn gate_active_ancestors(
    db: &impl DbClient,
    ids: Vec<String>,
) -> Result<Vec<String>, AppError> {
    let mut kept = Vec::with_capacity(ids.len());
    for id in ids {
        if all_ancestors_active(db, &id).await? {
            kept.push(id);
        }
    }
    Ok(kept)
}

/// **build_story** candidates — STUB backlog stories under active ancestors.
///
/// A stub = a `kind='story'`, live (`deleted_at IS NULL`), with
/// `relevance='backlog'`, a non-empty `attributes.problem_statement`, and NO
/// live children (no tasks decomposed yet). `json_extract` returns NULL for a
/// NULL `attributes` blob or an absent key (so the row is filtered, never an
/// error); `TRIM(...) <> ''` rejects a whitespace-only problem statement.
/// Ordered `created_at, id` for determinism, then active-ancestor-gated.
/// Read-only.
pub async fn build_story_candidates(db: &impl DbClient) -> Result<Vec<String>, AppError> {
    let ids = crate::db::scalar_all::<String>(
        db,
        r#"
        SELECT w.id
        FROM work_items w
        WHERE w.kind = 'story'
          AND w.deleted_at IS NULL
          AND w.relevance = 'backlog'
          AND json_extract(w.attributes, '$.problem_statement') IS NOT NULL
          AND TRIM(json_extract(w.attributes, '$.problem_statement')) <> ''
          AND NOT EXISTS (
              SELECT 1 FROM work_items c
              WHERE c.parent_id = w.id AND c.deleted_at IS NULL
          )
        ORDER BY w.created_at, w.id
        "#,
        args![],
    )
    .await?;
    gate_active_ancestors(db, ids).await
}

/// **build_tasks** candidates — "APPROVED" stories ready to decompose, under
/// active ancestors.
///
/// There is no `Approved` Status in this codebase, so the INTERIM predicate is
/// `relevance='active'` AND `ready_for_decomposition`. The active stories are
/// SQL-selected first (cheap), then `ready_for_decomposition` is derived by
/// REUSING [`super::get_story_readiness`] verbatim — its
/// `ready_for_decomposition` is `problem_statement_set` AND
/// `accepted_research_count >= 1` AND `unresolved_questions == 0` AND
/// `has_approach`, and reusing the function guarantees this module never
/// diverges from that definition. One readiness read per active story
/// (O(active stories)); order preserved from the SQL, then
/// active-ancestor-gated. Read-only.
pub async fn build_tasks_candidates(db: &impl DbClient) -> Result<Vec<String>, AppError> {
    let active_stories = crate::db::scalar_all::<String>(
        db,
        r#"
        SELECT w.id
        FROM work_items w
        WHERE w.kind = 'story'
          AND w.deleted_at IS NULL
          AND w.relevance = 'active'
        ORDER BY w.created_at, w.id
        "#,
        args![],
    )
    .await?;

    let mut ready = Vec::new();
    for id in active_stories {
        // Reuse the canonical readiness derivation — do NOT re-implement it.
        if get_story_readiness(db, &id).await?.ready_for_decomposition {
            ready.push(id);
        }
    }
    gate_active_ancestors(db, ready).await
}

/// **compose_sprint** candidates — stories with ≥1 READY task not yet in a
/// sprint, under active ancestors.
///
/// A task is READY by the SAME predicate `claim_next_task` /
/// `get_sprint_quiescence` use (sans the lane/tier filters): live, status IN
/// (`'todo'`,`'open'`), `assignee IS NULL`, `blocked_by_question_id IS NULL`, and
/// no unsatisfied task→task dependency (`NOT EXISTS` a dep whose target is not
/// `done`). The extra `NOT EXISTS (sprint_tasks)` clause requires the task to be
/// unbound to any sprint. `DISTINCT` collapses a story with multiple ready
/// tasks to one candidate; ordered `created_at, id`, then active-ancestor-gated.
/// Read-only.
pub async fn compose_sprint_candidates(db: &impl DbClient) -> Result<Vec<String>, AppError> {
    let ids = crate::db::scalar_all::<String>(
        db,
        r#"
        SELECT s.id
        FROM work_items s
        JOIN work_items t ON t.parent_id = s.id AND t.kind = 'task'
        WHERE s.kind = 'story'
          AND s.deleted_at IS NULL
          AND t.deleted_at IS NULL
          AND t.status IN ('todo', 'open')
          AND t.assignee IS NULL
          AND t.blocked_by_question_id IS NULL
          AND NOT EXISTS (
              SELECT 1 FROM task_dependencies d
              JOIN work_items dep ON dep.id = d.depends_on_id
              WHERE d.task_id = t.id AND dep.status <> 'done'
          )
          AND NOT EXISTS (
              SELECT 1 FROM sprint_tasks st WHERE st.task_id = t.id
          )
        GROUP BY s.id
        ORDER BY MIN(s.created_at), s.id
        "#,
        args![],
    )
    .await?;
    gate_active_ancestors(db, ids).await
}

/// Scan all three trigger predicates and return the dispatchable candidates in
/// DETERMINISTIC PRIORITY order — `build_story`, then `build_tasks`, then
/// `compose_sprint` — so the scheduler loop advances one stage per scan (it can
/// take the first candidate, or the first of the highest-priority kind). Within
/// each kind the per-predicate SQL already orders by `created_at, id`. Every
/// candidate is active-ancestor-gated by its predicate. This MIRRORS the
/// kind-priority `claim_next_scheduled_unit` orders its candidate set by, so the
/// two agree on which stage runs next.
///
/// The `drive` kind has no trigger predicate here — a `drive` unit is created
/// after a `compose_sprint` stage produces a sprint, not by scanning work-item
/// state. Read-only: no transaction, no events.
pub async fn scan_trigger_candidates(
    db: &impl DbClient,
) -> Result<Vec<TriggerCandidate>, AppError> {
    let mut out = Vec::new();
    for id in build_story_candidates(db).await? {
        out.push(TriggerCandidate {
            trigger_kind: ScheduledUnitKind::BuildStory,
            work_item_id: id,
        });
    }
    for id in build_tasks_candidates(db).await? {
        out.push(TriggerCandidate {
            trigger_kind: ScheduledUnitKind::BuildTasks,
            work_item_id: id,
        });
    }
    for id in compose_sprint_candidates(db).await? {
        out.push(TriggerCandidate {
            trigger_kind: ScheduledUnitKind::ComposeSprint,
            work_item_id: id,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{connect_in_memory, AnyPool};
    use crate::domain::Relevance;
    use crate::repo::test_support::*;
    use crate::repo::{
        add_acceptance_criterion, add_research_note, create_work_item, create_work_item_full,
        set_relevance, CreateOpts,
    };
    use sqlx::SqlitePool;

    /// Read a work item's `parent_id` (panics if it has none — used to walk the
    /// seeded project→epic→focus→story chain to its ancestor ids).
    async fn parent_of(pool: &SqlitePool, id: &str) -> String {
        sqlx::query_scalar::<_, Option<String>>("SELECT parent_id FROM work_items WHERE id = ?1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
            .expect("work item has a parent")
    }

    /// Build a project→epic→focus chain with the epic+focus marked `active`, and
    /// return `(epic, focus)`. The seeded epic/focus default to `relevance =
    /// 'backlog'`, so the active-ancestor gate would block every story beneath
    /// them until they are promoted — this helper does that promotion.
    async fn active_focus(pool: &SqlitePool, db: &AnyPool) -> (String, String) {
        let project = create_work_item(pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            pool,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        add_acceptance_criterion(pool, &epic, "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = create_work_item_full(
            pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
        )
        .await
        .expect("focus")
        .to_string();
        set_relevance(db, &epic, Relevance::Active).await.expect("epic active");
        set_relevance(db, &focus, Relevance::Active).await.expect("focus active");
        (epic, focus)
    }

    /// (a) A backlog stub story (problem_statement set, no children) IS surfaced
    /// by `build_story` when every ancestor is `active`, and is NOT surfaced when
    /// an ancestor is `backlog`/`deferred`/`rejected` (the active-ancestor-gate
    /// criterion). Also covers the stub's "no children" clause — a decomposed
    /// story drops out.
    #[tokio::test]
    async fn build_story_surfaces_backlog_stub_only_under_active_ancestors() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let focus = parent_of(&pool, &story).await;
        let epic = parent_of(&pool, &focus).await;

        set_relevance(&db, &epic, Relevance::Active).await.expect("epic active");
        set_relevance(&db, &focus, Relevance::Active).await.expect("focus active");

        // Stub: relevance backlog (the create default, set explicitly) + a
        // non-empty problem_statement + no children.
        sqlx::query(
            "UPDATE work_items SET relevance = 'backlog', \
             attributes = json_object('problem_statement', 'need X') WHERE id = ?1",
        )
        .bind(&story)
        .execute(&pool)
        .await
        .expect("frame the stub story");

        let cands = build_story_candidates(&db).await.expect("build_story scan");
        assert!(
            cands.contains(&story),
            "a backlog stub under active ancestors IS surfaced by build_story"
        );

        // Flip the focus ancestor to deferred → the active-ancestor gate excludes it.
        set_relevance(&db, &focus, Relevance::Deferred).await.expect("focus deferred");
        let gated = build_story_candidates(&db).await.expect("build_story gated scan");
        assert!(
            !gated.contains(&story),
            "a story under a non-active (deferred) ancestor is NOT dispatchable"
        );

        // Restore active ancestors → surfaced again; then decompose a task → it is
        // no longer a stub (the no-children clause).
        set_relevance(&db, &focus, Relevance::Active).await.expect("focus active again");
        assert!(
            build_story_candidates(&db).await.unwrap().contains(&story),
            "the stub is surfaced once ancestors are active again"
        );
        create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("decompose a task");
        assert!(
            !build_story_candidates(&db).await.unwrap().contains(&story),
            "a story with a decomposed child task is no longer a build_story stub"
        );
    }

    /// (b) The interim `build_tasks` predicate (`relevance='active'` AND
    /// `ready_for_decomposition`) surfaces a story once it is active AND
    /// ready-for-decomposition — and the readiness derivation matches
    /// `get_story_readiness` exactly (asserted via the reused function). The gate
    /// still applies: a rejected ancestor excludes a ready story.
    #[tokio::test]
    async fn build_tasks_surfaces_active_ready_for_decomposition_story() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let focus = parent_of(&pool, &story).await;
        let epic = parent_of(&pool, &focus).await;
        set_relevance(&db, &epic, Relevance::Active).await.expect("epic active");
        set_relevance(&db, &focus, Relevance::Active).await.expect("focus active");
        set_relevance(&db, &story, Relevance::Active).await.expect("story active");

        // Active but NOT ready (no approach, no accepted research) → not surfaced.
        sqlx::query(
            "UPDATE work_items SET attributes = json_object('problem_statement', 'need X') WHERE id = ?1",
        )
        .bind(&story)
        .execute(&pool)
        .await
        .expect("set problem_statement only");
        assert!(
            !build_tasks_candidates(&db).await.unwrap().contains(&story),
            "an active story missing approach + accepted research is NOT ready_for_decomposition"
        );

        // Make it ready: problem_statement + execution_strategy (approach) + ≥1
        // accepted research note + no open questions.
        sqlx::query(
            "UPDATE work_items SET attributes = \
             json_object('problem_statement', 'need X', 'execution_strategy', 'do Y') WHERE id = ?1",
        )
        .bind(&story)
        .execute(&pool)
        .await
        .expect("set problem_statement + approach");
        let note = add_research_note(&db, &story, "researched", None, None, None, None, None)
            .await
            .expect("research note")
            .to_string();
        sqlx::query("UPDATE research_notes SET state = 'accepted' WHERE id = ?1")
            .bind(&note)
            .execute(&pool)
            .await
            .expect("accept the research note");

        // The reused readiness function agrees — proving no divergent definition.
        assert!(
            get_story_readiness(&db, &story).await.unwrap().ready_for_decomposition,
            "get_story_readiness reports ready_for_decomposition for the same story"
        );
        assert!(
            build_tasks_candidates(&db).await.unwrap().contains(&story),
            "an active, ready_for_decomposition story IS surfaced by build_tasks"
        );

        // Gate: a rejected ancestor excludes the otherwise-ready story.
        set_relevance(&db, &focus, Relevance::Rejected).await.expect("focus rejected");
        assert!(
            !build_tasks_candidates(&db).await.unwrap().contains(&story),
            "the active-ancestor gate excludes a ready story under a rejected ancestor"
        );
    }

    /// (c) `scan_trigger_candidates` returns candidates in priority order —
    /// build_story before build_tasks before compose_sprint — when one story
    /// matches each predicate (all under a shared active focus).
    #[tokio::test]
    async fn scan_orders_build_story_before_build_tasks_before_compose_sprint() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let (_epic, focus) = active_focus(&pool, &db).await;

        // story_bs — build_story stub: backlog, problem_statement, no children.
        let story_bs = create_work_item(&pool, "story", Some(&focus), "BS", None)
            .await
            .expect("story_bs")
            .to_string();
        sqlx::query(
            "UPDATE work_items SET relevance = 'backlog', \
             attributes = json_object('problem_statement', 'frame me') WHERE id = ?1",
        )
        .bind(&story_bs)
        .execute(&pool)
        .await
        .expect("frame story_bs");

        // story_bt — build_tasks: active + ready_for_decomposition, no tasks.
        let story_bt = create_work_item(&pool, "story", Some(&focus), "BT", None)
            .await
            .expect("story_bt")
            .to_string();
        set_relevance(&db, &story_bt, Relevance::Active).await.expect("story_bt active");
        sqlx::query(
            "UPDATE work_items SET attributes = \
             json_object('problem_statement', 'p', 'execution_strategy', 'a') WHERE id = ?1",
        )
        .bind(&story_bt)
        .execute(&pool)
        .await
        .expect("frame story_bt");
        let note = add_research_note(&db, &story_bt, "r", None, None, None, None, None)
            .await
            .expect("note")
            .to_string();
        sqlx::query("UPDATE research_notes SET state = 'accepted' WHERE id = ?1")
            .bind(&note)
            .execute(&pool)
            .await
            .expect("accept note");

        // story_cs — compose_sprint: has a ready task (create-default 'open',
        // unleased, unblocked) not bound to any sprint.
        let story_cs = create_work_item(&pool, "story", Some(&focus), "CS", None)
            .await
            .expect("story_cs")
            .to_string();
        set_relevance(&db, &story_cs, Relevance::Active).await.expect("story_cs active");
        create_work_item(&pool, "task", Some(&story_cs), "T", None)
            .await
            .expect("ready task under story_cs");

        let cands = scan_trigger_candidates(&db).await.expect("scan");

        let pos_bs = cands
            .iter()
            .position(|c| c.work_item_id == story_bs)
            .expect("story_bs present as a build_story candidate");
        let pos_bt = cands
            .iter()
            .position(|c| c.work_item_id == story_bt)
            .expect("story_bt present as a build_tasks candidate");
        let pos_cs = cands
            .iter()
            .position(|c| c.work_item_id == story_cs)
            .expect("story_cs present as a compose_sprint candidate");

        assert_eq!(cands[pos_bs].trigger_kind, ScheduledUnitKind::BuildStory);
        assert_eq!(cands[pos_bt].trigger_kind, ScheduledUnitKind::BuildTasks);
        assert_eq!(cands[pos_cs].trigger_kind, ScheduledUnitKind::ComposeSprint);
        assert!(
            pos_bs < pos_bt && pos_bt < pos_cs,
            "priority order: build_story < build_tasks < compose_sprint (got bs={pos_bs}, bt={pos_bt}, cs={pos_cs})"
        );
    }
}
