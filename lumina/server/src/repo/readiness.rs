//! Readiness + quiescence read composers (migrations 0005/0013, R5 carve):
//! `get_sprint_quiescence`, `list_open_questions_for_sprint`, and
//! `get_story_readiness`. All three are READ-ONLY — plain auto-commit SELECTs
//! through the `DbClient` read seam, no `db.begin()`, no events.
//!
//! `pub use readiness::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here stays reachable at its existing `crate::repo::*` path.
//! `work_item_kind` / `get_work_item_detail` / `list_task_dependencies` are
//! reached via `use super::*`.

use super::*;
use crate::args;
use crate::db::DbClient;
use crate::domain::{
    NextAction, OpenQuestionSummary, SprintQuiescence, StoryReadiness, WorkItem,
};
use crate::error::AppError;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Quiescence + arbiter read (team-execution migration 0013, plan §F). Two
// READ-ONLY composers a sprint lead / arbiter agent polls:
//   * `get_sprint_quiescence` — the four sprint-wide lane-agnostic counts plus
//     the derived `done`/`stalled` verdict, used to decide whether to terminate
//     the run or escalate a stall to an arbiter.
//   * `list_open_questions_for_sprint` — the unresolved questions across the
//     sprint's stories, for the arbiter to resolve / escalate.
// Both issue plain auto-commit SELECTs through the `DbClient` read seam (no
// `db.begin()`, no events) — mirroring `get_story_readiness` / `query_findings`.
// ---------------------------------------------------------------------------

/// Compute a sprint's [`SprintQuiescence`] verdict (plan §F): the four
/// lane-agnostic counts across every task bound to the sprint via
/// `sprint_tasks`, plus the two derived booleans the lead polls.
///
/// The counts come from ONE pass with conditional `SUM(CASE …)` aggregates
/// (the crate's count idiom — one round-trip, all four columns consistent on
/// the same snapshot):
///   * `claimable` — the §C claim-readiness predicate MINUS the lease, byte-for-byte
///     identical to [`claim_next_task`]'s candidate WHERE (status IN
///     ('todo','open') AND assignee IS NULL AND blocked_by_question_id IS NULL
///     AND deleted_at IS NULL AND NOT EXISTS(unsatisfied dep)) but WITHOUT any
///     lane/tier filter — quiescence counts across ALL lanes. Keeping the two in
///     lockstep means the lead's "nothing to claim" verdict can never disagree
///     with what a claimer would actually find.
///   * `in_progress` — leased / being-worked (`status='in_progress'`, live).
///   * `blocked_on_question` — parked on a question (`blocked_by_question_id IS
///     NOT NULL`, live).
///   * `terminal` — `status IN ('done','cancelled')`, live.
///
/// **Sprint-level claim gating (migration 0016, plan §C).** `claim_next_task`
/// returns `Ok(None)` for a sprint that is NOT `active` OR is FROZEN (any live
/// checkpoint task is `in_progress`), regardless of how many tasks satisfy the
/// per-task readiness predicate. To keep `claimable` BYTE-CONSISTENT with that
/// guard, after the per-task `claimable` count is computed it is FORCED to 0
/// whenever the sprint is non-`active` or frozen. The two extra reads — the
/// sprint's `status` and a checkpoint-in_progress EXISTS probe — mirror
/// `claim_next_task`'s step-2 / step-2b guards exactly.
///
/// Verdict (computed in Rust from the counts):
///   * `done` ⇔ EVERY sprint task is terminal (`terminal == total`, total =
///     terminal + in_progress + blocked_on_question + the raw-claimable count
///     before any freeze/non-active zeroing). Deliberately NOT derived from the
///     gated `claimable`: a freeze / non-`active` status zeroes `claimable`
///     while real non-terminal work remains, so a `claimable == 0`-based `done`
///     would falsely report a merely-frozen sprint as complete. Basing it on
///     actual task completion keeps `done` honest under a freeze. An empty / all-
///     terminal sprint reads `done=true` as before.
///   * `stalled` ⇔ `blocked_on_question > 0 && claimable == 0 && in_progress ==
///     0 && !done` — the only non-terminal work is parked on a question (needs an
///     arbiter). The `!done` clause and the requirement of at least one
///     question-blocked task mean a frozen / non-`active` sprint with pending
///     (but un-parked) work is NEITHER `done` NOR `stalled` — its work is simply
///     gated, not stalled, and resumes when the sprint activates / unfreezes.
///
/// A missing / unknown `sprint_id` is NOT an error: the join yields zero rows,
/// every count is 0, the sprint-status read is `None` (non-`active`), so
/// `claimable` is 0 and — with no tasks — `done=true, stalled=false` (an empty
/// sprint is trivially quiescent). Read-only — no transaction, no events.
pub async fn get_sprint_quiescence(
    db: &impl DbClient,
    sprint_id: &str,
) -> Result<SprintQuiescence, AppError> {
    #[derive(Debug)]
    struct QuiescenceCountsRow {
        claimable: i64,
        in_progress: i64,
        blocked_on_question: i64,
        terminal: i64,
    }
    impl<'r, R> sqlx::FromRow<'r, R> for QuiescenceCountsRow
    where
        R: sqlx::Row,
        &'r str: sqlx::ColumnIndex<R>,
        i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(QuiescenceCountsRow {
                claimable: row.try_get("claimable")?,
                in_progress: row.try_get("in_progress")?,
                blocked_on_question: row.try_get("blocked_on_question")?,
                terminal: row.try_get("terminal")?,
            })
        }
    }

    // The `claimable` CASE predicate is held byte-consistent with the
    // `claim_next_task` candidate WHERE (sans the `lane = $2` / `:tier` filters,
    // which quiescence omits to count across all lanes). SUM over a boolean CASE
    // yields the count; COALESCE guards the all-NULL (zero-row) sprint so the
    // scalar reads back 0 rather than NULL.
    let counts: QuiescenceCountsRow = db
        .query_one::<QuiescenceCountsRow>(
            r#"
        SELECT
          COALESCE(SUM(CASE WHEN
              t.status IN ('todo', 'open')
              AND t.assignee IS NULL
              AND t.blocked_by_question_id IS NULL
              AND t.deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1 FROM task_dependencies d
                  JOIN work_items dep ON dep.id = d.depends_on_id
                  WHERE d.task_id = t.id AND dep.status <> 'done'
              )
            THEN 1 ELSE 0 END), 0) AS claimable,
          COALESCE(SUM(CASE WHEN
              t.status = 'in_progress' AND t.deleted_at IS NULL
            THEN 1 ELSE 0 END), 0) AS in_progress,
          COALESCE(SUM(CASE WHEN
              t.blocked_by_question_id IS NOT NULL AND t.deleted_at IS NULL
            THEN 1 ELSE 0 END), 0) AS blocked_on_question,
          COALESCE(SUM(CASE WHEN
              t.status IN ('done', 'cancelled') AND t.deleted_at IS NULL
            THEN 1 ELSE 0 END), 0) AS terminal
        FROM sprint_tasks st
        JOIN work_items t ON t.id = st.task_id
        WHERE st.sprint_id = $1
        "#,
            args![sprint_id.to_owned()],
        )
        .await?;

    // --- Sprint-level claim gating (mirror of `claim_next_task` step 2/2b). ---
    // `claim_next_task` returns Ok(None) for a non-`active` sprint OR a frozen
    // one (any live checkpoint task is `in_progress`), regardless of per-task
    // readiness. We read the same two signals so the `claimable` count below can
    // be forced to 0 under those conditions — keeping it byte-consistent with
    // what a claimer would actually find.
    //
    // 1. Sprint status — a missing sprint reads back `None` (non-`active`),
    //    matching the claim's "missing sprint ⇒ Ok(None)" edge.
    let sprint_status: Option<String> = crate::db::scalar_opt::<String>(
        db,
        "SELECT status FROM sprints WHERE id = $1",
        args![sprint_id.to_owned()],
    )
    .await?;
    let sprint_active = sprint_status.as_deref() == Some("active");

    // 2. Checkpoint-freeze probe — identical in shape to the claim's step-2b
    //    guard: a live checkpoint task `in_progress` freezes the whole sprint.
    let frozen: Option<i64> = crate::db::scalar_opt::<i64>(
        db,
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
    let frozen = frozen.is_some();

    // `done` reflects ACTUAL task completion (every sprint task terminal), read
    // off the RAW counts BEFORE any freeze/non-active zeroing — otherwise a
    // freeze that forces `claimable` to 0 could masquerade as completion. Total
    // live tasks = the four mutually-exclusive count buckets summed; `done` ⇔
    // there are tasks (or none) and all of them are terminal.
    let total = counts.claimable + counts.in_progress + counts.blocked_on_question + counts.terminal;
    let done = counts.terminal == total;

    // Gate `claimable` to 0 when the sprint is non-`active` OR frozen — the
    // claim yields nothing under either condition. Applied AFTER `done` is
    // derived so the artificial zero cannot fake completion.
    let claimable = if sprint_active && !frozen {
        counts.claimable
    } else {
        0
    };

    // `stalled` ⇔ the only non-terminal work is parked on a question. The added
    // `!done` clause + the `blocked_on_question > 0` requirement mean a frozen /
    // non-`active` sprint with pending (un-parked) work is NEITHER done NOR
    // stalled — its work is gated, not stalled, and resumes on activate/unfreeze.
    let stalled =
        counts.blocked_on_question > 0 && claimable == 0 && counts.in_progress == 0 && !done;

    Ok(SprintQuiescence {
        claimable,
        in_progress: counts.in_progress,
        blocked_on_question: counts.blocked_on_question,
        terminal: counts.terminal,
        done,
        stalled,
    })
}

/// List the UNRESOLVED open questions across the stories owning a sprint's
/// tasks (plan §F) — the arbiter agent's worklist. The owning stories are the
/// DISTINCT `parent_id`s of the sprint's task rows (a task's parent is always
/// its story per the hierarchy trigger). For each unresolved question
/// (`status = 'open'`, the create-default that [`add_open_question`] stamps and
/// that [`resolve_open_question`] flips away to `'answered'`) it returns the
/// question id, the owning story, the question text, the option labels (ordered
/// by `seq`), and the question's age in seconds (`now − created_at`, computed in
/// SQLite via `strftime('%s', …)` so it shares the stored-timestamp format).
///
/// Sprint-scoped + unresolved only: a resolved/answered question, or a question
/// on a story NOT owning any of this sprint's tasks, is excluded. An empty /
/// unknown sprint yields an empty Vec. Read-only — no transaction, no events.
pub async fn list_open_questions_for_sprint(
    db: &impl DbClient,
    sprint_id: &str,
) -> Result<Vec<OpenQuestionSummary>, AppError> {
    #[derive(Debug)]
    struct OpenQuestionRow {
        question_id: String,
        story_id: String,
        text: String,
        age_secs: i64,
    }
    impl<'r, R> sqlx::FromRow<'r, R> for OpenQuestionRow
    where
        R: sqlx::Row,
        &'r str: sqlx::ColumnIndex<R>,
        String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(OpenQuestionRow {
                question_id: row.try_get("question_id")?,
                story_id: row.try_get("story_id")?,
                text: row.try_get("text")?,
                age_secs: row.try_get("age_secs")?,
            })
        }
    }

    // Unresolved questions on the DISTINCT stories owning this sprint's tasks.
    // The story set is the IN-subquery (`parent_id` of the sprint's task rows);
    // `status = 'open'` is the unresolved predicate. `age_secs` is the
    // now−created_at delta in whole seconds via the strftime epoch idiom (both
    // operands in the same TEXT timestamp format). Ordered story, then question
    // seq for a stable arbiter worklist.
    let rows = db
        .query_all::<OpenQuestionRow>(
            r#"
        SELECT
          q.id                                                       AS question_id,
          q.story_id                                                 AS story_id,
          q.question                                                 AS text,
          CAST(strftime('%s', 'now') - strftime('%s', q.created_at) AS INTEGER) AS age_secs
        FROM open_questions q
        WHERE q.status = 'open'
          AND q.story_id IN (
              SELECT DISTINCT t.parent_id
              FROM sprint_tasks st
              JOIN work_items t ON t.id = st.task_id
              WHERE st.sprint_id = $1 AND t.parent_id IS NOT NULL
          )
        ORDER BY q.story_id, q.seq
        "#,
            args![sprint_id.to_owned()],
        )
        .await?;

    // Fetch each question's option labels (ordered by seq). One read per
    // question — O(n) for a per-sprint arbiter worklist (n is small), mirroring
    // the per-task acceptance-criteria reads in `get_story_readiness`.
    let mut summaries: Vec<OpenQuestionSummary> = Vec::with_capacity(rows.len());
    for row in rows {
        let options = crate::db::scalar_all::<String>(
            db,
            "SELECT label FROM question_options WHERE question_id = $1 ORDER BY seq",
            args![row.question_id.clone()],
        )
        .await?;
        summaries.push(OpenQuestionSummary {
            question_id: row.question_id,
            story_id: row.story_id,
            text: row.text,
            options,
            age_secs: row.age_secs,
        });
    }

    Ok(summaries)
}

// ---------------------------------------------------------------------------
// get_story_readiness (migration 0005). Compose existing reads to summarise a
// story's planning-pipeline readiness and the next recommended block.
// Read-only; no transaction, no events.
// ---------------------------------------------------------------------------

/// Compute a story's [`StoryReadiness`] aggregate (migration 0005). Composes
/// existing reads only — no mutations, no events.
///
/// The `next_recommended_action` cascade is a UX rollup over the CONVENTIONS
/// §l six-phase sequence (first match wins). See [`NextAction`]'s docstring
/// for the auto-recommended subset and the optional variants. Implemented
/// order (matches the cascade body below):
///   1. !problem_statement_set                         → `RunProblemStatement`
///   2. unresolved_questions > 0                       → `ResolveOpenQuestions`
///   3. !any_user_questions_ever                       → `RunUserInterrogation`
///   4. accepted_research_count == 0
///      a. any proposed research note                  → `RunVetResearch`
///      b. else                                        → `RunResearchNotes`
///   5. !has_approach                                  → `RunApproach`
///   6. no `attributes.verification_commands`          → `RunVerificationCommands`
///   7. no risks rows (live)                           → `RunRisks`
///   8. no `findings.kind='story-review'` row (live)   → `RunStoryReview`
///   9. no child tasks                                 → `RunDecomposeTasks`
///  10. !has_acceptance_criteria_on_all_tasks          → `RunSetTaskSpec`
///  11. ≥2 tasks AND no task_dependencies rows         → `RunWireTaskDeps`
///  12. else                                           → `StoryReady`
///
/// Variants `RunAlternatives`, `RunNotDoing`, `RunEdgeCases` are present in
/// the [`NextAction`] enum but NEVER auto-recommended by this cascade — they
/// are user-discretion blocks (a story may legitimately have nothing to
/// record); users invoke them directly via the `/lumina:` slash forms.
///
/// Errors:
///   * `NotFound` — `story_id` does not exist.
///   * `Validation` — `story_id` exists but is not `kind='story'`.
pub async fn get_story_readiness(
    pool: &impl DbClient,
    story_id: &str,
) -> Result<StoryReadiness, AppError> {
    let kind = work_item_kind(pool, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "get_story_readiness expects a 'story', not a '{kind}'"
        )));
    }

    // Load the story row (attributes carries problem_statement /
    // execution_strategy / verification_commands keys).
    let detail = get_work_item_detail(pool, story_id).await?;
    let attrs: serde_json::Map<String, Value> = detail
        .item
        .attributes
        .as_ref()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    let problem_statement_set = attrs.contains_key("problem_statement");
    let has_approach = attrs.contains_key("execution_strategy");
    let has_verification_commands = attrs.contains_key("verification_commands");

    // Accepted research = live notes with state='accepted'. The detail fold
    // already filters `superseded_by IS NULL`.
    let accepted_research_count: u32 = detail
        .research_notes
        .iter()
        .filter(|n| n.state.as_deref() == Some("accepted"))
        .count() as u32;
    let any_proposed_research = detail
        .research_notes
        .iter()
        .any(|n| n.state.as_deref() == Some("proposed"));

    let unresolved_questions: u32 = detail
        .open_questions
        .iter()
        .filter(|q| q.status.as_deref() == Some("open"))
        .count() as u32;
    // Distinguishes "story has never been interrogated" from "story has been
    // interrogated and all questions are answered" — the cascade emits
    // `RunUserInterrogation` only in the former case (when zero questions
    // exist), and `ResolveOpenQuestions` in the latter when `status='open'`
    // questions remain.
    let any_user_questions_ever = !detail.open_questions.is_empty();

    // Tasks under the story. We need each task's acceptance-criteria count to
    // compute `has_acceptance_criteria_on_all_tasks`; one query per task is
    // O(n) reads but acceptable for a per-story summary (n is small).
    let tasks: Vec<&WorkItem> = detail
        .children
        .iter()
        .filter(|c| c.kind == "task")
        .collect();
    let task_count = tasks.len();

    let mut tasks_with_no_ac: u32 = 0;
    for t in &tasks {
        let n: i64 = crate::db::scalar_one::<i64>(
            pool,
            r#"SELECT COUNT(*) FROM acceptance_criteria WHERE work_item_id = $1"#,
            args![t.id.clone()],
        )
        .await?;
        if n == 0 {
            tasks_with_no_ac += 1;
        }
    }
    let has_acceptance_criteria_on_all_tasks = task_count > 0 && tasks_with_no_ac == 0;

    // Risk count (live).
    let has_risks = !detail.risks.is_empty();

    // Story-review audit signal — the story has been audited if at least one
    // `findings.kind = 'story-review'` row exists. The advisor does not
    // inspect status/severity of those findings (that's the user's call); it
    // surfaces `RunStoryReview` only when no audit has ever happened.
    let story_review_audited = detail
        .findings
        .iter()
        .any(|f| f.kind.as_deref() == Some("story-review"));

    // Dependency edges among this story's tasks.
    let dep_count = list_task_dependencies(pool, story_id).await?.len();

    let ready_for_decomposition = problem_statement_set
        && accepted_research_count >= 1
        && unresolved_questions == 0
        && has_approach;

    // Cascade — first match wins. Ordering is a UX rollup keyed on missing
    // signals; see [`NextAction`]'s docstring for the auto-recommended subset
    // and its mapping back to CONVENTIONS §l phases. The cascade is NOT a
    // strict re-encoding of §l ordering — phase ordering is enforced by
    // `/lumina:plan-story`.
    let next_recommended_action = if !problem_statement_set {
        NextAction::RunProblemStatement
    } else if unresolved_questions > 0 {
        // Phase 1 derivative: questions exist and are still open; user must
        // answer them before moving on.
        NextAction::ResolveOpenQuestions
    } else if !any_user_questions_ever {
        // Phase 1: no questions have ever been recorded — invite the user to
        // interrogate.
        NextAction::RunUserInterrogation
    } else if accepted_research_count == 0 {
        if any_proposed_research {
            NextAction::RunVetResearch
        } else {
            NextAction::RunResearchNotes
        }
    } else if !has_approach {
        NextAction::RunApproach
    } else if !has_verification_commands {
        NextAction::RunVerificationCommands
    } else if !has_risks {
        NextAction::RunRisks
    } else if !story_review_audited {
        // Phase 4 audit gate — emitted once the story has cleared PS +
        // research + approach + verification + risks but has never been
        // audited via `/lumina:story-review`.
        NextAction::RunStoryReview
    } else if task_count == 0 {
        NextAction::RunDecomposeTasks
    } else if !has_acceptance_criteria_on_all_tasks {
        NextAction::RunSetTaskSpec
    } else if task_count >= 2 && dep_count == 0 {
        NextAction::RunWireTaskDeps
    } else {
        NextAction::StoryReady
    };

    Ok(StoryReadiness {
        story_id: story_id.to_owned(),
        problem_statement_set,
        accepted_research_count,
        unresolved_questions,
        has_approach,
        has_acceptance_criteria_on_all_tasks,
        ready_for_decomposition,
        next_recommended_action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AnyPool;
    use crate::db::connect_in_memory;
    use crate::domain::Lane;
    use crate::repo::test_support::*;
    use sqlx::SqlitePool;

    /// Activate a sprint directly (migration-0016: `seed_sprint` mints a
    /// `'draft'` sprint, but the quiescence claim-gating — like the claim
    /// itself — only counts tasks claimable for an `'active'`, unfrozen sprint).
    /// A direct status set is cleaner than walking draft→ready→active for tests
    /// that exercise the count, not the sprint lifecycle. Mirrors the
    /// `team_execution.rs` test helper of the same name.
    async fn activate_sprint(pool: &SqlitePool, sprint_id: &str) {
        sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
            .bind(sprint_id)
            .execute(pool)
            .await
            .expect("activate sprint");
    }

    // =======================================================================
    // get_sprint_quiescence + list_open_questions_for_sprint (T7, plan §F).
    // Read-only composers; reuse the queue seed helpers and the open-question
    // primitives. The blocked-on-question state is staged by `seed_queue_task`
    // (creates the task at 'todo') + `block_task_on_question` (flips it to
    // 'blocked' with `blocked_by_question_id` set, the same path production uses).
    // =======================================================================

    /// An all-terminal sprint (every task done/cancelled) is quiescent:
    /// `done=true`, `stalled=false`, and the `claimable`/`in_progress`/`blocked`
    /// counts are all zero. Also covers the empty-sprint trivial-done case.
    #[tokio::test]
    async fn quiescence_all_terminal_sprint_is_done() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;

        // Empty sprint: trivially quiescent (no tasks at all).
        let empty_sprint = seed_sprint(&pool).await;
        let empty = get_sprint_quiescence(&db, &empty_sprint)
            .await
            .expect("quiescence on empty sprint");
        assert_eq!(empty.claimable, 0);
        assert_eq!(empty.in_progress, 0);
        assert_eq!(empty.blocked_on_question, 0);
        assert_eq!(empty.terminal, 0);
        assert!(empty.done, "an empty sprint is trivially done");
        assert!(!empty.stalled, "an empty sprint is not stalled");

        // A missing/unknown sprint id is not an error — zero counts, done.
        let unknown = get_sprint_quiescence(&db, "no-such-sprint")
            .await
            .expect("quiescence on unknown sprint");
        assert!(unknown.done && !unknown.stalled, "unknown sprint reads as done");

        // A sprint whose only tasks are terminal: one done, one cancelled.
        let sprint = seed_sprint(&pool).await;
        let done_task =
            seed_queue_task(&pool, &story, &sprint, "DONE", Some("implement"), Some("deep")).await;
        let cancelled_task =
            seed_queue_task(&pool, &story, &sprint, "CANX", Some("implement"), Some("deep")).await;
        sqlx::query("UPDATE work_items SET status = 'done' WHERE id = $1")
            .bind(&done_task)
            .execute(&pool)
            .await
            .expect("mark done");
        sqlx::query("UPDATE work_items SET status = 'cancelled' WHERE id = $1")
            .bind(&cancelled_task)
            .execute(&pool)
            .await
            .expect("mark cancelled");

        let q = get_sprint_quiescence(&db, &sprint)
            .await
            .expect("quiescence on all-terminal sprint");
        assert_eq!(q.claimable, 0, "no claimable tasks");
        assert_eq!(q.in_progress, 0, "no in-progress tasks");
        assert_eq!(q.blocked_on_question, 0, "no blocked tasks");
        assert_eq!(q.terminal, 2, "both tasks are terminal");
        assert!(q.done, "all-terminal sprint is done");
        assert!(!q.stalled, "all-terminal sprint is not stalled");
    }

    /// A sprint with at least one claimable task is NOT done (and not stalled).
    /// The `claimable` count uses the SAME readiness predicate as
    /// `claim_next_task`: a dep-blocked task is NOT counted claimable until its
    /// dependency is done.
    #[tokio::test]
    async fn quiescence_claimable_task_is_not_done() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        // Active + unfrozen: claimable reverts to the real per-task count (the
        // sprint-status gate would otherwise zero it on the 'draft' default).
        activate_sprint(&pool, &sprint).await;

        // One ready (claimable) task + an in-progress task to exercise both counts.
        let _ready =
            seed_queue_task(&pool, &story, &sprint, "READY", Some("implement"), Some("deep")).await;
        let ip =
            seed_queue_task(&pool, &story, &sprint, "WORK", Some("implement"), Some("deep")).await;
        sqlx::query(
            "UPDATE work_items SET status = 'in_progress', assignee = 'agent-x', \
             lease_expires_at = datetime('now', '+1800 seconds') WHERE id = $1",
        )
        .bind(&ip)
        .execute(&pool)
        .await
        .expect("mark in_progress");

        let q = get_sprint_quiescence(&db, &sprint)
            .await
            .expect("quiescence");
        assert_eq!(q.claimable, 1, "the ready task is claimable");
        assert_eq!(q.in_progress, 1, "the leased task is in_progress");
        assert!(!q.done, "a sprint with claimable/in-progress work is not done");
        assert!(!q.stalled, "claimable+in_progress work present ⇒ not stalled");

        // The claimable count tracks claim_next_task's predicate: a dep-blocked
        // task is not counted until its dependency is done.
        let dep =
            seed_queue_task(&pool, &story, &sprint, "DEP", Some("implement"), Some("deep")).await;
        let dependent =
            seed_queue_task(&pool, &story, &sprint, "DEPENDENT", Some("implement"), Some("deep"))
                .await;
        add_task_dependency(&pool, &dependent, &dep, "sequence")
            .await
            .expect("dep edge");
        let q2 = get_sprint_quiescence(&db, &sprint)
            .await
            .expect("quiescence after deps");
        // ready (1) + dep (1) are claimable; dependent is dep-blocked (not counted).
        assert_eq!(
            q2.claimable, 2,
            "the dep-blocked task is excluded from claimable, matching claim_next_task"
        );

        // Cross-check: claim_next_task surfaces exactly the same readiness set —
        // claiming twice drains the two claimable tasks, a third claim is None.
        let c1 = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
            .await
            .expect("claim 1")
            .expect("first claimable");
        let c2 = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-b", 1800)
            .await
            .expect("claim 2")
            .expect("second claimable");
        assert_ne!(c1.task_id, c2.task_id, "two distinct claimable tasks");
        let c3 = claim_next_task(&db, &sprint, Lane::Implement, None, "agent-c", 1800)
            .await
            .expect("claim 3");
        assert!(
            c3.is_none(),
            "exactly two were claimable (the dep-blocked task stays invisible), \
             matching the quiescence claimable count of 2"
        );
    }

    /// T8 (migration 0016, plan §C): `get_sprint_quiescence.claimable` is
    /// byte-consistent with `claim_next_task`'s sprint-level gating — it reads 0
    /// for a NON-`active` sprint AND while the sprint is FROZEN (a checkpoint task
    /// is `in_progress`), and a frozen-but-incomplete sprint does NOT report
    /// `done`. The real per-task claimable count returns once the sprint is both
    /// `active` AND unfrozen.
    #[tokio::test]
    async fn quiescence_claimable_is_gated_by_status_and_freeze() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;

        // One genuinely ready task + a checkpoint task (also ready). Two tasks,
        // both per-task claimable, so the RAW claimable count is 2.
        let _ready =
            seed_queue_task(&pool, &story, &sprint, "READY", Some("implement"), Some("deep")).await;
        let checkpoint =
            seed_queue_task(&pool, &story, &sprint, "CKPT", Some("implement"), Some("deep")).await;
        set_task_checkpoint(&db, &checkpoint, true)
            .await
            .expect("mark checkpoint");

        // (1) Sprint is still 'draft' (non-active) → claimable gated to 0, and
        // NOT done (two non-terminal tasks remain). Mirrors claim returning None.
        let draft = get_sprint_quiescence(&db, &sprint)
            .await
            .expect("quiescence on draft sprint");
        assert_eq!(draft.claimable, 0, "a non-'active' sprint exposes no claimable work");
        assert!(!draft.done, "a non-active sprint with pending work is not done");
        assert!(!draft.stalled, "gated-by-status is not 'stalled' (no question-park)");
        // claim agrees: nothing claimable against a draft sprint.
        assert!(
            claim_next_task(&db, &sprint, Lane::Implement, None, "agent-a", 1800)
                .await
                .expect("claim runs")
                .is_none(),
            "claim_next_task returns None for a draft sprint — matches claimable=0"
        );

        // (2) Activate the sprint → the real claimable count (2) returns.
        activate_sprint(&pool, &sprint).await;
        let active = get_sprint_quiescence(&db, &sprint)
            .await
            .expect("quiescence on active sprint");
        assert_eq!(active.claimable, 2, "active+unfrozen exposes the real claimable count");
        assert!(!active.done, "two ready tasks ⇒ not done");

        // (3) Freeze the sprint: put the checkpoint task in_progress. claimable
        // gates back to 0, and the sprint is NOT falsely 'done' despite the
        // forced-zero claimable (a 'ready' task still awaits the barrier).
        sqlx::query("UPDATE work_items SET status = 'in_progress' WHERE id = $1")
            .bind(&checkpoint)
            .execute(&pool)
            .await
            .expect("checkpoint in_progress");
        let frozen = get_sprint_quiescence(&db, &sprint)
            .await
            .expect("quiescence on frozen sprint");
        assert_eq!(frozen.claimable, 0, "a frozen sprint exposes no claimable work");
        assert_eq!(frozen.in_progress, 1, "the checkpoint task is the one in_progress row");
        assert!(
            !frozen.done,
            "a frozen-but-incomplete sprint must NOT report done despite claimable=0"
        );
        assert!(!frozen.stalled, "a freeze is not a 'stalled' (no question-park)");
        // claim agrees: the freeze blocks every claim.
        assert!(
            claim_next_task(&db, &sprint, Lane::Implement, None, "agent-b", 1800)
                .await
                .expect("claim runs")
                .is_none(),
            "claim_next_task returns None during a freeze — matches claimable=0"
        );

        // (4) Lift the freeze (checkpoint task → done): claimable reverts to the
        // remaining ready task (1).
        sqlx::query("UPDATE work_items SET status = 'done' WHERE id = $1")
            .bind(&checkpoint)
            .execute(&pool)
            .await
            .expect("checkpoint done");
        let thawed = get_sprint_quiescence(&db, &sprint)
            .await
            .expect("quiescence after thaw");
        assert_eq!(thawed.claimable, 1, "the remaining ready task is claimable once unfrozen");
        assert_eq!(thawed.terminal, 1, "the checkpoint task is now terminal");
        assert!(!thawed.done, "one ready task remains ⇒ not done");
    }

    /// A sprint whose only non-terminal task is parked on an open question is
    /// STALLED: `blocked_on_question>0 && claimable==0 && in_progress==0`. Such a
    /// sprint is neither done nor progress-able without an arbiter.
    #[tokio::test]
    async fn quiescence_blocked_only_sprint_is_stalled() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        // Active + unfrozen: isolate the STALL on the question-park, not on the
        // sprint-status gate (a 'draft' sprint also zeroes claimable, which would
        // conflate the two reasons a task is unclaimable).
        activate_sprint(&pool, &sprint).await;

        // One task, parked on an open question (todo → blocked via the prod path).
        let task =
            seed_queue_task(&pool, &story, &sprint, "PARKED", Some("implement"), Some("deep")).await;
        let question = add_open_question(&db, &story, "Which approach?")
            .await
            .expect("open question")
            .to_string();
        block_task_on_question(&db, &task, &question)
            .await
            .expect("block task on question");

        let q = get_sprint_quiescence(&db, &sprint)
            .await
            .expect("quiescence on blocked-only sprint");
        assert_eq!(q.claimable, 0, "the parked task is not claimable");
        assert_eq!(q.in_progress, 0, "nothing in progress");
        assert_eq!(q.blocked_on_question, 1, "one task parked on a question");
        assert_eq!(q.terminal, 0, "nothing terminal");
        assert!(!q.done, "a blocked task means not done");
        assert!(q.stalled, "blocked-only with nothing else ⇒ stalled, needs arbiter");
    }

    /// `list_open_questions_for_sprint` returns only UNRESOLVED questions scoped
    /// to the stories owning the sprint's tasks: a resolved question is excluded,
    /// and a question on an unrelated story (not in this sprint) does not appear.
    /// The returned summary carries the question text, option labels (seq order),
    /// and a non-negative age.
    #[tokio::test]
    async fn open_questions_for_sprint_unresolved_and_scoped_only() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;

        // Bind a task on `story` to the sprint so `story` is in scope.
        let _task =
            seed_queue_task(&pool, &story, &sprint, "T", Some("implement"), Some("deep")).await;

        // An UNRESOLVED question on the sprint's story, with two options.
        let live_q = add_open_question(&db, &story, "Pick a serialization format?")
            .await
            .expect("live question")
            .to_string();
        let opt_a = add_question_option(&db, &live_q, "JSON", None)
            .await
            .expect("option A")
            .to_string();
        let _opt_b = add_question_option(&db, &live_q, "TOML", Some("matches the export format"))
            .await
            .expect("option B");

        // A RESOLVED question on the same story — must be EXCLUDED.
        let resolved_q = add_open_question(&db, &story, "Already decided?")
            .await
            .expect("resolved question")
            .to_string();
        let resolved_opt = add_question_option(&db, &resolved_q, "Yes", None)
            .await
            .expect("resolved option")
            .to_string();
        resolve_open_question(&db, &resolved_q, &resolved_opt, Some("lead"))
            .await
            .expect("resolve the question");

        // A question on an UNRELATED story (a separate chain, not in this sprint)
        // — must NOT appear.
        let other_story = seed_chain_to_story(&pool).await;
        let _other_q = add_open_question(&db, &other_story, "Unrelated question?")
            .await
            .expect("unrelated question");

        let questions = list_open_questions_for_sprint(&db, &sprint)
            .await
            .expect("list open questions");

        assert_eq!(
            questions.len(),
            1,
            "only the single unresolved, sprint-scoped question is returned"
        );
        let summary = &questions[0];
        assert_eq!(summary.question_id, live_q, "the live question id");
        assert_eq!(summary.story_id, story, "scoped to the sprint's story");
        assert_eq!(summary.text, "Pick a serialization format?", "question text mapped");
        assert_eq!(
            summary.options,
            vec!["JSON".to_string(), "TOML".to_string()],
            "option labels in seq order"
        );
        assert!(summary.age_secs >= 0, "age is a non-negative second delta");
        // Sanity: the option id was minted (referenced so it isn't dead).
        assert!(!opt_a.is_empty());

        // An unknown/empty sprint yields an empty list.
        let none = list_open_questions_for_sprint(&db, "no-such-sprint")
            .await
            .expect("list on unknown sprint");
        assert!(none.is_empty(), "unknown sprint has no questions");
    }
}
