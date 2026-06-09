//! Findings query / aggregation (B20, migration 0011 — Part B Phase B4).
//!
//! The two public read paths `query_findings` (filter / count-by) and
//! `get_story_finding_queue` (story + direct task children), the
//! `QueryFindingsResult` output enum, and the `&'static str` SELECTs + the
//! `AxisCount` `FromRow` impl they own.
//!
//! `Finding` decodes via its `FromRow` impl (which stays in `repo/mod.rs`,
//! globally visible). The domain types named in the signatures are imported
//! explicitly from `crate::*` (a `use super::*` glob does NOT carry super's
//! private `use` imports); `use super::*` is retained for consistency with the
//! sibling carves.

use crate::args;
use crate::db::DbClient;
use crate::domain::Finding;
use crate::error::AppError;

/// Hand-written generic `FromRow` for the public [`crate::domain::AxisCount`]
/// (columns `key: String`, `count: i64`), used by [`query_findings`]'s grouped
/// count-by branch. Generic over `R: Row` per the canonical [`crate::db`]
/// FromRow recipe (so it rides `query_all::<AxisCount>` on the SQLite arm today
/// and a future Pg arm unchanged), and indexed by column NAME to stay robust to
/// SELECT-column reordering. The orphan rule permits this impl because
/// `AxisCount` is crate-local — exactly as the [`crate::domain::Finding`] impl
/// above proves.
impl<'r, R> sqlx::FromRow<'r, R> for crate::domain::AxisCount
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(crate::domain::AxisCount {
            key: row.try_get("key")?,
            count: row.try_get("count")?,
        })
    }
}

/// The two output shapes [`query_findings`] can return, selected by the filter's
/// `count_by` axis. Defined HERE (not in `domain.rs`) because it is a repo-layer
/// sum over two existing domain types rather than a stored entity.
///
/// EXTERNALLY-tagged (`#[serde(rename_all = "snake_case")]` on the enum) so the
/// wire shape is `{"findings":[...]}` / `{"counts":[...]}` — the B21 MCP + B22
/// HTTP layers `serde_json::to_value` this directly, with the variant name
/// carrying the discriminator. (No `JsonSchema`: the MCP layer wraps aggregate
/// reads with `Content::json` rather than `Json<T>`, mirroring `StoryReadiness`
/// / `BatchEntry`.)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryFindingsResult {
    /// Full live findings (the `count_by == None` branch).
    Findings(Vec<crate::domain::Finding>),
    /// Grouped counts by the requested axis (the `count_by == Some(_)` branch).
    Counts(Vec<crate::domain::AxisCount>),
}

// SHARED NULL-GUARD FILTER — both `query_findings` SQL constants below embed the
// SAME `($N IS NULL OR col = $N)`-per-field WHERE clause, in the fixed positional
// order `work_item_id ($1)`, `run_id ($2)`, `severity ($3)`, `category ($4)`,
// `status ($5)`, `triage_state ($6)`, bound from the filter's `Option<String>`
// fields in that exact order (an absent field passes NULL, disabling its
// conjunct). This mirrors the `($N IS NULL OR col = $N)` idiom in
// `LIST_WORK_ITEMS_SQL` and the run-listing query: each placeholder is bound ONCE
// per distinct `$N` and the runtime SQLite layer references it positionally, so a
// placeholder appearing twice in the SQL reads the same single bound value.
// `superseded_by IS NULL` keeps the result to LIVE findings only — consistent
// with `list_findings` and the `get_work_item_detail` fold. The SQL stays a
// `&'static str` literal (the runtime seam requires `'static`; the WHERE clause
// is NEVER built dynamically from user input — the only variation is whether each
// bound value is NULL). The clause is written inline in both constants (rather
// than concatenated from a shared fragment) so each stays a single greppable
// `&'static str` literal.
//
// R16 (design note): the `($N IS NULL OR col = $N)` NULL-guard pattern is
// NON-SARGABLE — the `$N IS NULL` disjunct makes the predicate non-index-friendly,
// so if a covering index on (e.g.) `severity`/`triage_state` were ever added it
// could not be used while the guard branch is live. This is accepted as immaterial
// at the current `findings`-table scale (a full scan is cheap); the deliberate
// trade-off is one prepared statement covering every filter combination (no
// dynamic SQL). Revisit only if the table grows enough that the scan dominates.

/// Full-row SELECT for the `count_by == None` branch — the exact column list of
/// [`list_findings`] (decoded by the shared [`crate::domain::Finding`] `FromRow`)
/// plus the shared NULL-guard filter and a stable `first_flagged DESC, id` order.
const QUERY_FINDINGS_ROWS_SQL: &str = "\
    SELECT id, work_item_id, kind, severity, effort, category, status, \
           file, line, symbol, summary, description, first_flagged, rounds, \
           fingerprint, flow, dedup_id, origin, confidence, superseded_by, \
           run_id, triage_state, \
           resolved_at, resolution, defer_reason, defer_trigger, \
           wontfix_rationale, repo_id \
    FROM findings \
    WHERE ($1 IS NULL OR work_item_id = $1) \
      AND ($2 IS NULL OR run_id = $2) \
      AND ($3 IS NULL OR severity = $3) \
      AND ($4 IS NULL OR category = $4) \
      AND ($5 IS NULL OR status = $5) \
      AND ($6 IS NULL OR triage_state = $6) \
      AND superseded_by IS NULL \
    ORDER BY first_flagged DESC, id";

/// Grouped count-by-severity SELECT for the `count_by == Some(Severity)` branch.
/// `COALESCE(severity, '(none)')` keeps `AxisCount.key` non-null when a finding
/// has no severity (the same sentinel is used in both the SELECT alias and the
/// GROUP BY so the bucket is coherent). Same NULL-guard filter + `superseded_by
/// IS NULL` live constraint as the full-row branch.
const QUERY_FINDINGS_COUNT_SEVERITY_SQL: &str = "\
    SELECT COALESCE(severity, '(none)') AS key, COUNT(*) AS count \
    FROM findings \
    WHERE ($1 IS NULL OR work_item_id = $1) \
      AND ($2 IS NULL OR run_id = $2) \
      AND ($3 IS NULL OR severity = $3) \
      AND ($4 IS NULL OR category = $4) \
      AND ($5 IS NULL OR status = $5) \
      AND ($6 IS NULL OR triage_state = $6) \
      AND superseded_by IS NULL \
    GROUP BY COALESCE(severity, '(none)') \
    ORDER BY key";

/// Query LIVE findings with a static NULL-guard filter, optionally returning
/// grouped axis counts instead of full rows (decision D12, migration 0011).
///
/// The filter (`work_item_id`, `run_id`, `severity`, `category`, `status`,
/// `triage_state` — all `Option<String>`) is applied through a single static
/// `($N IS NULL OR col = $N)`-per-field WHERE clause (see
/// [`QUERY_FINDINGS_FILTER_SQL`]): an absent field binds `NULL`, which disables
/// its conjunct, so one prepared statement covers every filter combination
/// WITHOUT ever building SQL from user input. "Live only" — `superseded_by IS
/// NULL` is always applied, matching [`list_findings`] and the
/// `get_work_item_detail` fold (superseded findings are intentionally NOT
/// queryable here).
///
/// When `filter.count_by` is set, the query GROUPs instead of returning rows:
/// for [`crate::domain::FindingAxis::Severity`] it returns one
/// [`crate::domain::AxisCount`] per distinct severity (NULL severities fold into
/// a `'(none)'` sentinel bucket), as [`QueryFindingsResult::Counts`]. When
/// `count_by` is `None`, it returns the full live findings ordered
/// `first_flagged DESC, id` as [`QueryFindingsResult::Findings`]. The `count_by`
/// dispatch is a `match` so adding a future axis is a localised change. This is
/// a READ — no transaction, no event row.
pub async fn query_findings(
    db: &impl DbClient,
    filter: &crate::domain::QueryFindingsFilter,
) -> Result<QueryFindingsResult, AppError> {
    // The six NULL-guard binds, in the fixed positional order $1..=$6. Each
    // value is cloned once into the owned `Args` bundle; the SQL references the
    // matching `$N` (twice per field — once in `IS NULL`, once in `= $N`) and the
    // runtime SQLite layer resolves both references to this single bound value.
    let bind_args = || {
        args![
            filter.work_item_id.clone(),
            filter.run_id.clone(),
            filter.severity.clone(),
            filter.category.clone(),
            filter.status.clone(),
            filter.triage_state.clone(),
        ]
    };

    match filter.count_by {
        Some(crate::domain::FindingAxis::Severity) => {
            let counts = db
                .query_all::<crate::domain::AxisCount>(
                    QUERY_FINDINGS_COUNT_SEVERITY_SQL,
                    bind_args(),
                )
                .await?;
            Ok(QueryFindingsResult::Counts(counts))
        }
        None => {
            let findings = db
                .query_all::<Finding>(QUERY_FINDINGS_ROWS_SQL, bind_args())
                .await?;
            Ok(QueryFindingsResult::Findings(findings))
        }
    }
}

/// SELECT for [`get_story_finding_queue`]: every live finding attached to the
/// story itself OR one of its DIRECT task children, EXCLUDING any whose
/// work-item is soft-deleted. The single static JOIN to `work_items` exists for
/// the tombstone guard (`w.deleted_at IS NULL`) — a finding on a tombstoned
/// work-item must drop out of the queue.
const STORY_FINDING_QUEUE_SQL: &str = "\
    SELECT f.id, f.work_item_id, f.kind, f.severity, f.effort, f.category, f.status, \
           f.file, f.line, f.symbol, f.summary, f.description, f.first_flagged, f.rounds, \
           f.fingerprint, f.flow, f.dedup_id, f.origin, f.confidence, f.superseded_by, \
           f.run_id, f.triage_state, \
           f.resolved_at, f.resolution, f.defer_reason, f.defer_trigger, \
           f.wontfix_rationale, f.repo_id \
    FROM findings f \
    JOIN work_items w ON f.work_item_id = w.id \
    WHERE (w.id = $1 OR (w.parent_id = $1 AND w.kind = 'task')) \
      AND w.deleted_at IS NULL \
      AND f.superseded_by IS NULL \
    ORDER BY f.first_flagged DESC, f.id";

/// Compose a story's review/optimise finding queue (decision D7, migration
/// 0011): every LIVE finding attached to the story itself OR one of its DIRECT
/// task children, ordered newest-flagged first.
///
/// ## Queue scope
/// The story plus its direct task children. The hierarchy makes tasks direct
/// children of a story (`work_items.parent_id` = story id, enforced by the
/// hierarchy trigger), so a single static JOIN `findings ↔ work_items` with
/// `(w.id = $1 OR (w.parent_id = $1 AND w.kind = 'task'))` spans the queue
/// WITHOUT a recursive CTE. The child branch's `kind = 'task'` guard (R20) makes
/// the queue self-contained rather than relying on the external hierarchy
/// invariant that a story's only children are tasks.
///
/// ## Tombstone guard (the point of the JOIN)
/// `w.deleted_at IS NULL` EXCLUDES findings whose work-item has been
/// soft-deleted — the JOIN exists for this guard (a bare `findings`-only query
/// could not see the work-item's tombstone). `f.superseded_by IS NULL` keeps the
/// result to live findings, consistent with [`list_findings`] /
/// [`query_findings`]. This is a READ — no transaction, no event row.
pub async fn get_story_finding_queue(
    db: &impl DbClient,
    story_id: &str,
) -> Result<Vec<crate::domain::Finding>, AppError> {
    let rows = db
        .query_all::<Finding>(STORY_FINDING_QUEUE_SQL, args![story_id.to_owned()])
        .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::QueryFindingsFilter;
    use crate::domain::Severity;
    use crate::repo::{NewFinding, add_repo_link, create_finding, create_work_item, delete_work_item};
    use crate::repo::test_support::*;
    use sqlx::SqlitePool;

    async fn set_finding_triage_state(pool: &SqlitePool, id: &str, state: &str) {
        sqlx::query("UPDATE findings SET triage_state = $1 WHERE id = $2")
            .bind(state)
            .bind(id)
            .execute(pool)
            .await
            .expect("update triage_state");
    }

    /// `query_findings` with NO filter and `count_by = None` returns ALL live
    /// findings; per-field filters (`work_item_id`, `severity`, `triage_state`)
    /// narrow the set; superseded findings never appear.
    #[tokio::test]
    async fn query_findings_filters_live_findings() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("legal task under story")
            .to_string();

        // Two findings on the story (one critical, one minor), one on the task.
        let f_crit = create_finding(
            &pool,
            &story,
            &NewFinding {
                severity: Some(Severity::Critical),
                summary: Some("crit on story"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("crit finding")
        .to_string();
        create_finding(
            &pool,
            &story,
            &NewFinding {
                severity: Some(Severity::Minor),
                summary: Some("minor on story"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("minor finding");
        create_finding(
            &pool,
            &task,
            &NewFinding {
                severity: Some(Severity::Critical),
                summary: Some("crit on task"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("task finding");

        // Mark the story's critical finding as accepted for the triage filter.
        set_finding_triage_state(&pool, &f_crit, "accepted").await;

        let all_count = |r: &QueryFindingsResult| match r {
            QueryFindingsResult::Findings(v) => v.len(),
            QueryFindingsResult::Counts(_) => panic!("expected Findings variant"),
        };

        // No filter → all three live findings.
        let all = query_findings(&pool, &QueryFindingsFilter::default_empty())
            .await
            .expect("query all");
        assert_eq!(all_count(&all), 3, "no filter returns all live findings");

        // Filter by work_item_id = story → the two story findings.
        let by_story = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_work_item_id(&story),
        )
        .await
        .expect("query by story");
        assert_eq!(all_count(&by_story), 2, "story has two findings");

        // Filter by severity = critical → the two critical findings.
        let by_sev = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_severity("critical"),
        )
        .await
        .expect("query by severity");
        assert_eq!(all_count(&by_sev), 2, "two critical findings");

        // Filter by triage_state = accepted → just the one we marked.
        let by_triage = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_triage_state("accepted"),
        )
        .await
        .expect("query by triage_state");
        match &by_triage {
            QueryFindingsResult::Findings(v) => {
                assert_eq!(v.len(), 1, "one accepted finding");
                assert_eq!(v[0].id, f_crit, "the accepted finding is f_crit");
            }
            QueryFindingsResult::Counts(_) => panic!("expected Findings variant"),
        }

        // Combined filter (work_item_id = story AND severity = minor) → one row.
        let combined = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty()
                .with_work_item_id(&story)
                .with_severity("minor"),
        )
        .await
        .expect("combined query");
        assert_eq!(all_count(&combined), 1, "one minor finding on the story");

        // Supersede the minor finding → it drops out of the live result.
        let minor_id = match &by_story {
            QueryFindingsResult::Findings(v) => v
                .iter()
                .find(|f| f.summary.as_deref() == Some("minor on story"))
                .map(|f| f.id.clone())
                .expect("minor finding present"),
            QueryFindingsResult::Counts(_) => unreachable!(),
        };
        sqlx::query("UPDATE findings SET superseded_by = $1 WHERE id = $2")
            .bind(&f_crit)
            .bind(&minor_id)
            .execute(&pool)
            .await
            .expect("supersede minor");
        let after_supersede = query_findings(&pool, &QueryFindingsFilter::default_empty())
            .await
            .expect("query after supersede");
        assert_eq!(
            all_count(&after_supersede),
            2,
            "superseded finding drops out of the live result"
        );
    }

    /// `query_findings` with `count_by = Some(Severity)` returns grouped
    /// `AxisCount`s whose counts sum to the total live findings, including the
    /// `'(none)'` sentinel bucket for a NULL-severity finding.
    #[tokio::test]
    async fn query_findings_count_by_severity_groups_and_sums() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // Two critical, one minor, one with NO severity (→ '(none)' bucket).
        for summary in ["c1", "c2"] {
            create_finding(
                &pool,
                &story,
                &NewFinding {
                    severity: Some(Severity::Critical),
                    summary: Some(summary),
                    ..NewFinding::default()
                },
            )
            .await
            .expect("crit");
        }
        create_finding(
            &pool,
            &story,
            &NewFinding {
                severity: Some(Severity::Minor),
                summary: Some("m1"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("minor");
        create_finding(
            &pool,
            &story,
            &NewFinding {
                severity: None,
                summary: Some("no-sev"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("no-sev");

        let res = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_count_by(crate::domain::FindingAxis::Severity),
        )
        .await
        .expect("count-by query");

        let counts = match res {
            QueryFindingsResult::Counts(c) => c,
            QueryFindingsResult::Findings(_) => panic!("expected Counts variant"),
        };

        // Buckets: '(none)' (1), 'critical' (2), 'minor' (1) — ordered by key.
        let by_key: std::collections::HashMap<&str, i64> =
            counts.iter().map(|c| (c.key.as_str(), c.count)).collect();
        assert_eq!(by_key.get("critical"), Some(&2), "two criticals");
        assert_eq!(by_key.get("minor"), Some(&1), "one minor");
        assert_eq!(
            by_key.get("(none)"),
            Some(&1),
            "one NULL-severity finding in the sentinel bucket"
        );
        let total: i64 = counts.iter().map(|c| c.count).sum();
        assert_eq!(total, 4, "grouped counts sum to all four live findings");
    }

    /// `get_story_finding_queue` spans the story PLUS its direct task children,
    /// and a finding on a SOFT-DELETED work-item drops out (tombstone guard).
    #[tokio::test]
    async fn story_finding_queue_excludes_tombstoned_work_items() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("legal task under story")
            .to_string();

        // One finding on the story, one on the child task.
        create_finding(
            &pool,
            &story,
            &NewFinding {
                summary: Some("on story"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("story finding");
        create_finding(
            &pool,
            &task,
            &NewFinding {
                summary: Some("on task"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("task finding");

        // Before deletion: both findings appear in the queue.
        let before = get_story_finding_queue(&pool, &story)
            .await
            .expect("queue before");
        let summaries_before: std::collections::HashSet<&str> = before
            .iter()
            .filter_map(|f| f.summary.as_deref())
            .collect();
        assert_eq!(before.len(), 2, "story + task findings span the queue");
        assert!(summaries_before.contains("on story"));
        assert!(summaries_before.contains("on task"));

        // Soft-delete the task work-item.
        delete_work_item(&pool, &task)
            .await
            .expect("soft-delete task");

        // After deletion: the task's finding drops out; the story's remains.
        let after = get_story_finding_queue(&pool, &story)
            .await
            .expect("queue after");
        assert_eq!(after.len(), 1, "tombstoned task's finding excluded");
        assert_eq!(
            after[0].summary.as_deref(),
            Some("on story"),
            "only the story's finding survives the tombstone guard"
        );
    }

    /// Tiny test-only builder helpers for [`QueryFindingsFilter`] (the struct's
    /// fields are public but it has no constructor; tests want a fluent empty
    /// base + per-field setters).
    impl QueryFindingsFilter {
        fn default_empty() -> Self {
            QueryFindingsFilter {
                work_item_id: None,
                run_id: None,
                severity: None,
                category: None,
                status: None,
                triage_state: None,
                count_by: None,
            }
        }
        fn with_work_item_id(mut self, id: &str) -> Self {
            self.work_item_id = Some(id.to_owned());
            self
        }
        fn with_severity(mut self, s: &str) -> Self {
            self.severity = Some(s.to_owned());
            self
        }
        fn with_triage_state(mut self, s: &str) -> Self {
            self.triage_state = Some(s.to_owned());
            self
        }
        fn with_count_by(mut self, axis: crate::domain::FindingAxis) -> Self {
            self.count_by = Some(axis);
            self
        }
    }

    /// R8 (the dominant R-A1 safety-net gap): seed a finding with the nullable
    /// disposition columns POPULATED (non-NULL `resolution`/`defer_reason`/
    /// `wontfix_rationale`/`repo_id`), read it back through `query_findings`, and
    /// assert the decoded `Option<String>` values round-trip. Most tests seed via
    /// `NewFinding::default()` leaving these NULL, so this is the only test that
    /// would catch a `String`-vs-`Option<String>` decode mismatch on these columns.
    #[tokio::test]
    async fn query_findings_decodes_populated_disposition_columns() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // A repo_link on the project ancestor so `repo_id` can be a real FK value.
        let project: String = sqlx::query_scalar::<_, String>(
            "SELECT id FROM work_items WHERE kind = 'project' LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("project id");
        let repo_id = add_repo_link(&pool, &project, "octocat/hello-world", true)
            .await
            .expect("add repo link")
            .to_string();

        let finding = create_finding(
            &pool,
            &story,
            &NewFinding {
                summary: Some("populated dispositions"),
                resolution: Some("fixed in PR #42"),
                defer_reason: Some("blocked on upstream"),
                wontfix_rationale: Some("by design"),
                repo_id: Some(&repo_id),
                ..NewFinding::default()
            },
        )
        .await
        .expect("finding with populated dispositions")
        .to_string();

        let res = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_work_item_id(&story),
        )
        .await
        .expect("query findings");
        let rows = match res {
            QueryFindingsResult::Findings(v) => v,
            QueryFindingsResult::Counts(_) => panic!("expected Findings variant"),
        };
        let f = rows
            .iter()
            .find(|f| f.id == finding)
            .expect("the populated finding is in the result");
        assert_eq!(
            f.resolution.as_deref(),
            Some("fixed in PR #42"),
            "resolution decodes to its non-NULL value"
        );
        assert_eq!(
            f.defer_reason.as_deref(),
            Some("blocked on upstream"),
            "defer_reason decodes to its non-NULL value"
        );
        assert_eq!(
            f.wontfix_rationale.as_deref(),
            Some("by design"),
            "wontfix_rationale decodes to its non-NULL value"
        );
        assert_eq!(
            f.repo_id.as_deref(),
            Some(repo_id.as_str()),
            "repo_id decodes to its non-NULL FK value"
        );
    }
}
