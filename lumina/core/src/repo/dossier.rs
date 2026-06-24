//! Story dossier composer (migration 0026, story-planning-round-5): the single
//! composed read an orchestrator pulls to drive dispatch. `get_story_dossier`
//! bundles the story's [`WorkItemDetail`] (LIVE planning rows only), the per-task
//! research grounding (the R52 answer), the derived files footprint, the
//! dispatch-plan waves, and the [`StoryReadiness`] verdict.
//!
//! READ-ONLY — it composes existing reads (`get_work_item_detail`,
//! `story_files_footprint`, `get_task_dispatch_plan`, `get_story_readiness`,
//! `list_task_research_links`) plus one lightweight liveness-id read; no
//! `db.begin()`, no events. `pub use dossier::*` in `repo/mod.rs` exposes
//! `get_story_dossier` at `crate::repo::get_story_dossier`.
//!
//! **Liveness contract.** The dossier presents LIVE planning rows ONLY:
//!   * research_notes / findings — already filtered `superseded_by IS NULL` by the
//!     existing `get_work_item_detail` folds.
//!   * open_questions — `get_work_item_detail`'s fold does NOT filter, so the
//!     dossier POST-FILTERS the composed detail's `open_questions` to exclude
//!     RETIRED (`retired_at IS NOT NULL`) AND CANCELLED (`status='cancelled'`)
//!     questions, WITHOUT changing `get_work_item_detail`'s behaviour for other
//!     callers (the regular detail view keeps showing all questions).
//!   * child tasks — `status='cancelled'` tasks are excluded from the dossier's
//!     task set (and from the per-task grounding).
//!
//! **Epoch is METADATA, never a filter (A.5 corrected model).** A surviving LIVE
//! row keeps its older epoch and STILL renders; rows are excluded ONLY because
//! they are superseded / retired / cancelled, never by epoch. The `plan_epoch`
//! values ride the rows as data.

use super::*;
use crate::args;
use crate::db::DbClient;
use crate::domain::{StoryDossier, TaskResearchGrounding};
use crate::error::AppError;

/// Compose a story's full planning [`StoryDossier`] (migration 0026).
/// Story-kind-gated (a non-story is `Validation`; an absent id is `NotFound` via
/// the kind read). See the module docs for the liveness contract and the
/// epoch-is-metadata model.
///
/// Composition:
///   * `story` — the story's [`WorkItemDetail`] (via `get_work_item_detail`), with
///     its `open_questions` POST-FILTERED to LIVE only (not retired, not
///     cancelled) and its `children` filtered to NON-cancelled.
///   * `task_research_links` — per NON-cancelled task: its id + title + the LIVE
///     research notes grounding it (via `list_task_research_links`, which already
///     drops superseded notes). The keyed shape preserves the task↔note
///     association (the detail's children are shallow `WorkItem`s).
///   * `story_files_footprint` — via `story_files_footprint`.
///   * `dispatch_plan` — via `get_task_dispatch_plan` (cycles propagate as
///     `AppError::Cycle`, matching every other dispatch-plan consumer).
///   * `readiness` — via `get_story_readiness`.
pub async fn get_story_dossier(
    db: &impl DbClient,
    story_id: &str,
) -> Result<StoryDossier, AppError> {
    // Story-kind gate (NotFound if absent; Validation if not a story) — mirrors
    // get_story_readiness / compute_task_batches.
    let kind = work_item_kind(db, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "get_story_dossier expects a 'story', not a '{kind}'"
        )));
    }

    // The full detail aggregate. research_notes/findings folds are already LIVE
    // (superseded_by IS NULL); open_questions is NOT filtered, so we trim it below.
    let mut detail = get_work_item_detail(db, story_id).await?;

    // --- open_questions: keep LIVE only (not retired, not cancelled) ----------
    // `get_work_item_detail` does NOT select `retired_at`, and the OpenQuestion
    // domain shape carries no `retired_at`, so we cannot filter retired off the
    // composed value alone. One lightweight id read returns the LIVE question ids
    // for this story; we retain only those — leaving `get_work_item_detail`
    // unchanged for every other caller. The CANCELLED exclusion is folded into
    // the SAME read for one round-trip (it could also be read off the composed
    // `status`, but doing both here keeps the liveness predicate single-source).
    let live_question_ids: std::collections::HashSet<String> = db
        .query_all::<crate::db::Scalar<String>>(
            r#"
            SELECT id FROM open_questions
            WHERE story_id = $1
              AND retired_at IS NULL
              AND status IS NOT 'cancelled'
            "#,
            args![story_id.to_owned()],
        )
        .await?
        .into_iter()
        .map(|s| s.0)
        .collect();
    detail
        .open_questions
        .retain(|q| live_question_ids.contains(&q.id));

    // --- children: drop cancelled tasks (and resolve the live task set) -------
    // The dossier's task set excludes cancelled tasks. We keep the filtered tasks
    // both ON the detail (so the rendered story matches the grounding) and as the
    // grounding key set below.
    detail
        .children
        .retain(|c| !(c.kind == "task" && c.status == "cancelled"));

    // --- per-task research grounding (the R52 answer) -------------------------
    // For each NON-cancelled task child, fold its LIVE grounding edges. A keyed
    // shape is REQUIRED: `detail.children` is a shallow `Vec<WorkItem>` (no
    // per-task task_research_links fold), so a flat link list would lose the
    // task↔note association. `list_task_research_links` already filters to LIVE
    // notes (`research_notes.superseded_by IS NULL`), so a grounding whose note
    // was superseded by the rework drops out here without an unlink.
    let mut task_research_links: Vec<TaskResearchGrounding> = Vec::new();
    for child in &detail.children {
        if child.kind != "task" {
            continue;
        }
        let links = list_task_research_links(db, &child.id).await?;
        task_research_links.push(TaskResearchGrounding {
            task_id: child.id.clone(),
            task_title: child.title.clone(),
            links,
        });
    }

    // --- the remaining composed reads ----------------------------------------
    let story_files_footprint = story_files_footprint(db, story_id).await?;
    // Cycles propagate as AppError::Cycle (the same shape every other dispatch-plan
    // consumer surfaces — the MCP/HTTP layers map it to invalid_params/422).
    let dispatch_plan = get_task_dispatch_plan(db, story_id).await?;
    let readiness = get_story_readiness(db, story_id).await?;

    Ok(StoryDossier {
        story: detail,
        task_research_links,
        story_files_footprint,
        dispatch_plan,
        readiness,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AnyPool;
    use crate::db::connect_in_memory;
    use crate::domain::ResearchState;
    use crate::repo::test_support::*;

    /// Rework-sim end-to-end (T3 AC): a story with research note A (survives),
    /// research note B (superseded), an open question (retired), and a task
    /// grounded on note B. After the rework — `supersede_research_note(B → B2)`,
    /// `retire_open_question(question)`, `bump_plan_epoch(story)` — the dossier
    ///   * EXCLUDES superseded note B (it is not in the live research_notes fold);
    ///   * EXCLUDES the retired question;
    ///   * INCLUDES surviving note A (older epoch 0, still LIVE — epoch is
    ///     metadata, not a filter);
    ///   * the per-task grounding no longer cites dead note B (the link's note is
    ///     superseded so `list_task_research_links` drops it);
    ///   * `readiness.plan_epoch` reflects the bump.
    #[tokio::test]
    async fn dossier_rework_excludes_dead_rows_keeps_survivors() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;

        // Research note A — the survivor (authored at epoch 0).
        let note_a = add_research_note(&db, &story, "note A survives", None, Some("high"), None, None, None)
            .await
            .expect("note A")
            .to_string();
        // Accept A so it counts toward readiness (not strictly required for the
        // dossier liveness asserts, but keeps the readiness verdict realistic).
        update_research_note(
            &db,
            &note_a,
            &crate::domain::UpdateResearchNoteRequest {
                confidence: None,
                state: Some(ResearchState::Accepted),
                rationale: None,
                lens: None,
                anchors: None,
            },
        )
        .await
        .expect("accept A");

        // Research note B — will be superseded by the rework.
        let note_b = add_research_note(&db, &story, "note B (will die)", None, Some("low"), None, None, None)
            .await
            .expect("note B")
            .to_string();

        // An open question — will be retired by the rework.
        let question = add_open_question(&db, &story, "rework: which path?")
            .await
            .expect("question")
            .to_string();

        // A task grounded on note B.
        let task = create_work_item(&pool, "task", Some(&story), "T-grounded-on-B", None)
            .await
            .expect("task")
            .to_string();
        link_task_research(&db, &task, &note_b)
            .await
            .expect("link task → note B");

        // --- pre-rework sanity: B is live, the question is live, grounding cites B.
        let pre = get_story_dossier(&db, &story).await.expect("pre dossier");
        assert!(
            pre.story.research_notes.iter().any(|n| n.id == note_b),
            "note B is live before the rework"
        );
        assert!(
            pre.story.open_questions.iter().any(|q| q.id == question),
            "the question is live before the rework"
        );
        let pre_grounding = pre
            .task_research_links
            .iter()
            .find(|g| g.task_id == task)
            .expect("the grounded task is in the dossier");
        assert!(
            pre_grounding.links.iter().any(|l| l.research_note_id == note_b),
            "pre-rework grounding cites note B"
        );

        // --- the rework: supersede B → B2, retire the question, bump the epoch.
        let note_b2 = add_research_note(&db, &story, "note B2 (replacement)", None, Some("high"), None, None, None)
            .await
            .expect("note B2")
            .to_string();
        supersede_research_note(&db, &note_b, &note_b2)
            .await
            .expect("supersede B → B2");
        retire_open_question(&db, &question)
            .await
            .expect("retire the question");
        let new_epoch = bump_plan_epoch(&db, &story).await.expect("bump epoch");
        assert_eq!(new_epoch, 1, "the first bump moves epoch 0 → 1");

        // --- post-rework dossier asserts.
        let post = get_story_dossier(&db, &story).await.expect("post dossier");

        // (1) Superseded note B is EXCLUDED; survivor A and replacement B2 remain.
        assert!(
            !post.story.research_notes.iter().any(|n| n.id == note_b),
            "superseded note B is excluded from the dossier"
        );
        assert!(
            post.story.research_notes.iter().any(|n| n.id == note_a),
            "surviving note A (older epoch, still live) still renders"
        );
        assert!(
            post.story.research_notes.iter().any(|n| n.id == note_b2),
            "the replacement note B2 renders"
        );

        // (2) The retired question is EXCLUDED.
        assert!(
            !post.story.open_questions.iter().any(|q| q.id == question),
            "the retired question is excluded from the dossier"
        );

        // (3) The grounding no longer cites the dead note B (its note is
        //     superseded, so list_task_research_links drops the edge).
        let post_grounding = post
            .task_research_links
            .iter()
            .find(|g| g.task_id == task)
            .expect("the grounded task is still in the dossier");
        assert!(
            !post_grounding.links.iter().any(|l| l.research_note_id == note_b),
            "post-rework grounding does NOT cite the dead note B"
        );

        // (4) Epoch is metadata, surfaced via readiness — the bump is reflected.
        assert_eq!(post.readiness.plan_epoch, 1, "readiness carries the bumped epoch");
    }

    /// A CANCELLED task is excluded from the dossier's task set and its grounding;
    /// a CANCELLED question is excluded from the dossier's open_questions (the
    /// liveness filter covers both retired AND cancelled).
    #[tokio::test]
    async fn dossier_excludes_cancelled_tasks_and_questions() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;

        let live_task = create_work_item(&pool, "task", Some(&story), "LIVE", None)
            .await
            .expect("live task")
            .to_string();
        let cancelled_task = create_work_item(&pool, "task", Some(&story), "CANX", None)
            .await
            .expect("cancelled task")
            .to_string();
        sqlx::query("UPDATE work_items SET status = 'cancelled' WHERE id = $1")
            .bind(&cancelled_task)
            .execute(&pool)
            .await
            .expect("cancel task");

        // A cancelled question (status flipped directly — the cancel-branch path).
        let q = add_open_question(&db, &story, "cancelled branch?")
            .await
            .expect("question")
            .to_string();
        sqlx::query("UPDATE open_questions SET status = 'cancelled' WHERE id = $1")
            .bind(&q)
            .execute(&pool)
            .await
            .expect("cancel question");

        let dossier = get_story_dossier(&db, &story).await.expect("dossier");

        assert!(
            dossier.story.children.iter().any(|c| c.id == live_task),
            "the live task is in the dossier"
        );
        assert!(
            !dossier.story.children.iter().any(|c| c.id == cancelled_task),
            "the cancelled task is excluded from the dossier task set"
        );
        assert!(
            !dossier.task_research_links.iter().any(|g| g.task_id == cancelled_task),
            "the cancelled task is excluded from the grounding"
        );
        assert!(
            !dossier.story.open_questions.iter().any(|oq| oq.id == q),
            "the cancelled question is excluded from the dossier"
        );
    }

    /// `get_story_dossier` is story-kind-gated: a task target is `Validation`, a
    /// missing id is `NotFound`.
    #[tokio::test]
    async fn dossier_kind_gated() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        let err = get_story_dossier(&db, &task)
            .await
            .expect_err("a task target must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        let missing = get_story_dossier(&db, "no-such-id")
            .await
            .expect_err("a missing id must reject");
        assert!(matches!(missing, AppError::NotFound(_)), "got {missing:?}");
    }

    /// Cross-story rejection (T3 AC): `link_task_research` with a note from a
    /// DIFFERENT story is `Validation`. Also covers the live-note / kind guards.
    #[tokio::test]
    async fn link_task_research_rejects_cross_story() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();

        let story_a = seed_chain_to_story(&pool).await;
        let story_b = seed_chain_to_story(&pool).await;

        let task_a = create_work_item(&pool, "task", Some(&story_a), "TA", None)
            .await
            .expect("task A")
            .to_string();
        // A note on story B — the WRONG story for task A.
        let note_b = add_research_note(&db, &story_b, "note on story B", None, None, None, None, None)
            .await
            .expect("note B")
            .to_string();

        let err = link_task_research(&db, &task_a, &note_b)
            .await
            .expect_err("a cross-story link must reject");
        assert!(
            matches!(err, AppError::Validation(_)),
            "cross-story link is a Validation error, got {err:?}"
        );

        // A SAME-story link succeeds (and is idempotent on a re-link).
        let note_a = add_research_note(&db, &story_a, "note on story A", None, None, None, None, None)
            .await
            .expect("note A")
            .to_string();
        link_task_research(&db, &task_a, &note_a)
            .await
            .expect("same-story link ok");
        link_task_research(&db, &task_a, &note_a)
            .await
            .expect("re-link is an idempotent no-op success");
        let links = list_task_research_links(&db, &task_a).await.expect("links");
        assert_eq!(links.len(), 1, "the idempotent re-link did not duplicate the edge");

        // Linking a non-task is Validation.
        let err = link_task_research(&db, &story_a, &note_a)
            .await
            .expect_err("a non-task link target must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Linking a SUPERSEDED note is Validation.
        let note_a2 = add_research_note(&db, &story_a, "note A2", None, None, None, None, None)
            .await
            .expect("note A2")
            .to_string();
        supersede_research_note(&db, &note_a, &note_a2)
            .await
            .expect("supersede A → A2");
        let task_a2 = create_work_item(&pool, "task", Some(&story_a), "TA2", None)
            .await
            .expect("task A2")
            .to_string();
        let err = link_task_research(&db, &task_a2, &note_a)
            .await
            .expect_err("a superseded note cannot ground a task");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    /// `unlink_task_research` (the repo-internal rework/cancel primitive) drops a
    /// grounding edge, is idempotent on an absent edge, and is kind-gated.
    #[tokio::test]
    async fn unlink_task_research_drops_edge_idempotently() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let note = add_research_note(&db, &story, "n", None, None, None, None, None)
            .await
            .expect("note")
            .to_string();

        link_task_research(&db, &task, &note).await.expect("link");
        assert_eq!(
            list_task_research_links(&db, &task).await.expect("links").len(),
            1,
            "one edge after link"
        );

        unlink_task_research(&db, &task, &note).await.expect("unlink");
        assert!(
            list_task_research_links(&db, &task).await.expect("links").is_empty(),
            "the edge is gone after unlink"
        );

        // Idempotent: unlinking an already-absent edge is a no-op success.
        unlink_task_research(&db, &task, &note)
            .await
            .expect("unlinking an absent edge is a no-op success");

        // Kind-gated.
        let err = unlink_task_research(&db, &story, &note)
            .await
            .expect_err("a non-task unlink target must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    /// `bump_plan_epoch` increments monotonically and returns the new epoch; an
    /// absent id is `NotFound`. `retire_open_question` on an absent id is
    /// `NotFound`.
    #[tokio::test]
    async fn bump_epoch_and_retire_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;

        assert_eq!(bump_plan_epoch(&db, &story).await.expect("bump 1"), 1);
        assert_eq!(bump_plan_epoch(&db, &story).await.expect("bump 2"), 2);

        let err = bump_plan_epoch(&db, "no-such-id")
            .await
            .expect_err("bump missing");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");

        let err = retire_open_question(&db, "no-such-id")
            .await
            .expect_err("retire missing");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }
}
