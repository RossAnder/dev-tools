//! MCP first-class touched-file tools (migration 0020, files-touched-first-class
//! pass / T6) — the read/write surface over the `task_files` child table that
//! promotes the former `attributes.files_touched` JSON array to an indexable,
//! de-duplicated set (the repo layer lives in `lumina_core::repo::task_files` +
//! `repo::reads`).
//!
//! Four tools register via the `tool_router_files` sub-router, summed into the
//! combined `tool_router` field by `LuminaTools::with_state`:
//!   * `record_task_actual_files` — APPEND to a task's EXECUTION-time set
//!     (`kind='actual'`) → `repo::add_task_actual_files`;
//!   * `reconcile_task_files` — close-time reconcile of the EXPECTED set against
//!     the ACTUAL set → `repo::reconcile_task_files_at_close`;
//!   * `get_story_files_footprint` — the DISTINCT `(repo_link_id, path)` union
//!     over a story's task children → `repo::story_files_footprint`;
//!   * `get_sprint_files_footprint` — the same union over a sprint's member
//!     tasks → `repo::sprint_files_footprint`.
//!
//! Each WRITE delegates 1:1 to its `repo::*` mutator via `.map_err(app_error_to_mcp)`
//! and returns a `structured_result`; each READ returns a `json_result` with
//! `read_only_hint = true`. The Option-A seam (the repo fn owns its OWN
//! `BEGIN IMMEDIATE` tx + the single coarse export-INERT `task_files` event;
//! `aggregate_type = "task_files"`, never `"work_item"`, so the git-export drain
//! ignores it) means these tools do NOT open a tx or record an event — they just
//! wrap the repo fn, exactly as `mcp/worktrees.rs` wraps `record_task_commits`.
//!
//! The actual-files write reuses the SAME `files_touched` entry shape
//! `set_task_spec` accepts: the `crate::mcp::FileRef` untagged enum (a bare path
//! string `"src/foo.rs"`, or a `{repo: "owner/name", path: "src/foo.rs"}` object
//! whose slug must reference a `repo_links` row on the task's project ancestor).
//! The repo fn (`add_task_actual_files`) re-resolves + re-validates the slugs
//! internally (the same path `set_task_spec` and `set_task_expected_files` use),
//! so this layer just converts each `FileRef` to its on-the-wire JSON form and
//! hands the slice through — an unknown slug surfaces as the repo's typed
//! `Validation` → `invalid_params`.

use super::*;

/// Arguments for the `record_task_actual_files` write tool →
/// `repo::add_task_actual_files`. APPEND-ONLY (the execution-time set
/// accumulates across re-runs; a re-recorded `(repo, path)` collapses
/// idempotently). At least one entry is required — an empty batch is a clean
/// `Validation` (no event for a zero-row append), mirroring `record_task_commits`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecordTaskActualFilesParams {
    /// The task whose ACTUAL touched-file set to append to (must reference an
    /// existing `kind='task'` row — the repo fn resolves its project ancestor
    /// for any `{repo, path}` slug).
    pub task_id: String,
    /// The files actually touched. Each entry is either a bare path string
    /// (resolves to the project's PRIMARY linked repo — the NULL/primary
    /// bucket) or a `{repo, path}` object naming a non-primary linked repo (the
    /// `repo` slug must reference a `repo_links` row on the task's project
    /// ancestor — the same shape `set_task_spec.files_touched` accepts).
    pub files_touched: Vec<FileRef>,
}

/// Arguments for the `reconcile_task_files` write tool →
/// `repo::reconcile_task_files_at_close`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReconcileTaskFilesParams {
    /// The task whose EXPECTED set is reconciled against its ACTUAL set.
    pub task_id: String,
}

/// Arguments for the `get_story_files_footprint` read tool →
/// `repo::story_files_footprint`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetStoryFilesFootprintParams {
    /// The story whose direct task children's touched-file footprint to read.
    pub story_id: String,
}

/// Arguments for the `get_sprint_files_footprint` read tool →
/// `repo::sprint_files_footprint`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetSprintFilesFootprintParams {
    /// The sprint whose member tasks' touched-file footprint to read.
    pub sprint_id: String,
}

/// Convert a slice of `FileRef` param entries to their on-the-wire JSON form
/// (`Path → string`, `Qualified → {repo, path}`) so the repo fn can re-resolve
/// + re-validate the slugs internally against the task's project ancestor — the
/// SAME conversion `set_task_spec` performs before handing entries to
/// `set_task_expected_files`. No repo-link lookup happens here: the repo writer
/// owns the canonicalisation + the typed reject on an unknown/malformed slug.
fn files_to_json(entries: Vec<FileRef>) -> Vec<serde_json::Value> {
    entries
        .into_iter()
        .map(|entry| match entry {
            FileRef::Path(path) => serde_json::Value::String(path),
            FileRef::Qualified { repo: slug, path } => {
                serde_json::json!({ "repo": slug, "path": path })
            }
        })
        .collect()
}

#[tool_router(router = tool_router_files, vis = "pub(crate)")]
impl LuminaTools {
    /// APPEND to a task's ACTUAL (execution-time) touched-file set (single repo
    /// call → `repo::add_task_actual_files`). APPEND-ONLY + idempotent: a
    /// re-recorded `(repo, path)` collapses on the `task_files` UNIQUE index and
    /// is NOT counted in `inserted`. An empty `files_touched` is invalid_params
    /// (no zero-row append). Records exactly ONE coarse export-INERT `task_files`
    /// event (`aggregate_type='task_files'`, NOT `work_item` — never
    /// git-exported); the write does NOT route through `update_work_item`, so the
    /// task's `updated_at` is untouched. Returns `{ inserted }` — the count of
    /// genuinely-new rows.
    #[tool(
        description = "Append to a task's ACTUAL (execution-time) touched-file set. APPEND-ONLY and idempotent: each entry is either a bare path string (resolves to the project's PRIMARY linked repo) or a {repo, path} object naming a non-primary linked repo (the slug must reference a repo_links row on the task's project ancestor, else invalid_params); a re-recorded (repo, path) collapses on the UNIQUE index and is not counted. An empty files_touched is invalid_params (no zero-row append). Records exactly one coarse export-INERT task_files event (aggregate_type='task_files', never git-exported) and does NOT route through update_work_item (the task's updated_at is untouched). Returns { inserted } — the count of NEWLY recorded rows.",
        annotations(open_world_hint = false)
    )]
    async fn record_task_actual_files(
        &self,
        Parameters(RecordTaskActualFilesParams { task_id, files_touched }): Parameters<
            RecordTaskActualFilesParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "record_task_actual_files", "mcp tool invoked");
        let entries = files_to_json(files_touched);
        let inserted = repo::add_task_actual_files(&self.pool, &task_id, &entries)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "inserted": inserted }))
    }

    /// Reconcile a task's EXPECTED file set against its ACTUAL set AT CLOSE
    /// (single repo call → `repo::reconcile_task_files_at_close`). CLEARS every
    /// `kind='expected'` row never matched by an `kind='actual'` row (a
    /// planned-but-untouched file); NEVER prunes the ACTUAL set; EXPECTED and
    /// ACTUAL stay distinct kinds. Idempotent — a re-run after a prior reconcile
    /// clears zero and re-audits nothing. A MATERIAL divergence (≥1 expected
    /// cleared) appends exactly ONE `reconcile` audit activity. The close routes
    /// auto-trigger this; this tool lets an operator / the e2e trigger it
    /// explicitly. Returns `{ cleared, unexpected_actual }`.
    #[tool(
        description = "Reconcile a task's EXPECTED file set against its ACTUAL set at close. CLEARS every kind='expected' row never matched by a kind='actual' row (a planned-but-untouched file); NEVER prunes the ACTUAL set; EXPECTED and ACTUAL stay distinct kinds. Idempotent: a re-run after a prior reconcile clears zero and re-audits nothing. A MATERIAL divergence (>=1 expected cleared) appends exactly one 'reconcile' audit activity. The transition->done close routes auto-trigger this; this tool exposes an explicit trigger for an operator / e2e. Returns { cleared, unexpected_actual }.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn reconcile_task_files(
        &self,
        Parameters(ReconcileTaskFilesParams { task_id }): Parameters<ReconcileTaskFilesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "reconcile_task_files", "mcp tool invoked");
        let outcome = repo::reconcile_task_files_at_close(&self.pool, &task_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({
            "cleared": outcome.cleared,
            "unexpected_actual": outcome.unexpected_actual,
        }))
    }

    /// Read a story's DERIVED files footprint (single repo call →
    /// `repo::story_files_footprint`): the DISTINCT `(repo_link_id, path)` union
    /// over the `task_files` rows of the story's DIRECT task children, deduped
    /// ACROSS kind (a path present as both expected and actual appears once).
    /// Pure derived read — there is NO independent story footprint store. An
    /// unknown/childless story yields an empty list. Read-only.
    #[tool(
        description = "Read a story's DERIVED files footprint: the DISTINCT (repo_link_id, path) union over the task_files rows of the story's DIRECT task children, deduped across kind (a path present as both expected and actual appears once). Pure derived read — there is no independent footprint store; an unknown/childless story yields an empty list. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_story_files_footprint(
        &self,
        Parameters(GetStoryFilesFootprintParams { story_id }): Parameters<
            GetStoryFilesFootprintParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_story_files_footprint", "mcp tool invoked");
        let footprint = repo::story_files_footprint(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&footprint)
    }

    /// Read a sprint's DERIVED files footprint (single repo call →
    /// `repo::sprint_files_footprint`): the DISTINCT `(repo_link_id, path)` union
    /// over the `task_files` rows of the sprint's MEMBER tasks (the
    /// `sprint_tasks` junction), deduped ACROSS kind exactly as the story
    /// footprint. Pure derived read. An unknown/empty sprint yields an empty
    /// list. Read-only.
    #[tool(
        description = "Read a sprint's DERIVED files footprint: the DISTINCT (repo_link_id, path) union over the task_files rows of the sprint's MEMBER tasks (the sprint_tasks junction), deduped across kind exactly as the story footprint. Pure derived read; an unknown/empty sprint yields an empty list. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_sprint_files_footprint(
        &self,
        Parameters(GetSprintFilesFootprintParams { sprint_id }): Parameters<
            GetSprintFilesFootprintParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_sprint_files_footprint", "mcp tool invoked");
        let footprint = repo::sprint_files_footprint(&self.pool, &sprint_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&footprint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::test_support::{create_item, seed_chain_to_story};
    use lumina_core::db::connect_in_memory;

    /// Build a tool handler over a fresh in-memory pool, returning it plus the
    /// concrete pool clone (for RAW sqlx assertions over the SAME pool).
    async fn tools() -> (LuminaTools, std::sync::Arc<AnyPool>) {
        let pool = std::sync::Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        (LuminaTools::new(pool.clone()), pool)
    }

    /// Count `task_files` rows for a task narrowed to one `kind`.
    async fn count_actual(pool: &AnyPool, task_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM task_files WHERE task_id = $1 AND kind = 'actual'",
        )
        .bind(task_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count actual task_files")
    }

    /// Read the `inserted` count out of a `record_task_actual_files` result.
    fn inserted_of(res: &CallToolResult) -> i64 {
        res.structured_content
            .as_ref()
            .expect("structured payload")["inserted"]
            .as_i64()
            .expect("inserted i64")
    }

    /// `record_task_actual_files` appends the EXECUTION-time set and records
    /// exactly ONE coarse export-INERT `task_files` event on the
    /// `aggregate_type='task_files'` aggregate (NOT `work_item`) — the AC's
    /// inert-event guarantee at the MCP layer. (The full export-drain assertion
    /// — drain → 0 work_item rows for this write — is the T8 e2e thread's
    /// concern; here we assert the aggregate_type the drain keys off.)
    #[tokio::test]
    async fn record_actual_appends_and_emits_inert_task_files_event() {
        let (tools, pool) = tools().await;
        let story = seed_chain_to_story(&tools).await;
        let task = create_item(&tools, "task", Some(&story)).await;

        // Baseline: the task's updated_at + the count of work_item-aggregate
        // events on it BEFORE the actual write (the create stamped both).
        let updated_before: String =
            sqlx::query_scalar("SELECT updated_at FROM work_items WHERE id = $1")
                .bind(&task)
                .fetch_one(pool.sqlite())
                .await
                .expect("task updated_at before");
        let work_item_events_before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'work_item' AND aggregate_id = $1",
        )
        .bind(&task)
        .fetch_one(pool.sqlite())
        .await
        .expect("work_item events before");

        let res = tools
            .record_task_actual_files(Parameters(RecordTaskActualFilesParams {
                task_id: task.clone(),
                files_touched: vec![
                    FileRef::Path("src/a.rs".to_owned()),
                    FileRef::Path("src/b.rs".to_owned()),
                ],
            }))
            .await
            .expect("record_task_actual_files succeeds");
        assert_eq!(inserted_of(&res), 2, "both new actual rows inserted");
        assert_eq!(count_actual(&pool, &task).await, 2, "two actual rows stored");

        // Exactly one inert task_files event on the task_files aggregate
        // (NOT work_item — the drain only renders work_item aggregates).
        let inert: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events \
             WHERE aggregate_type = 'task_files' AND aggregate_id = $1 \
               AND event_type = 'task_files.actual_appended'",
        )
        .bind(&task)
        .fetch_one(pool.sqlite())
        .await
        .expect("count inert task_files event");
        assert_eq!(inert, 1, "the append records exactly one inert task_files event");

        // NO new work_item event + updated_at unchanged (does not route through
        // update_work_item — so the export drain never re-renders this task for
        // the file write; the T8 e2e proves the drain itself).
        let work_item_events_after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'work_item' AND aggregate_id = $1",
        )
        .bind(&task)
        .fetch_one(pool.sqlite())
        .await
        .expect("work_item events after");
        assert_eq!(
            work_item_events_after, work_item_events_before,
            "the file write records NO new work_item event"
        );
        let updated_after: String =
            sqlx::query_scalar("SELECT updated_at FROM work_items WHERE id = $1")
                .bind(&task)
                .fetch_one(pool.sqlite())
                .await
                .expect("task updated_at after");
        assert_eq!(updated_before, updated_after, "task updated_at unchanged");

        // Append-only idempotency: re-appending src/a.rs inserts 0 new rows.
        let again = tools
            .record_task_actual_files(Parameters(RecordTaskActualFilesParams {
                task_id: task.clone(),
                files_touched: vec![FileRef::Path("src/a.rs".to_owned())],
            }))
            .await
            .expect("re-append succeeds");
        assert_eq!(inserted_of(&again), 0, "re-appending the same key is a no-op");
        assert_eq!(count_actual(&pool, &task).await, 2, "still two rows after the idempotent re-append");
    }

    /// An EMPTY `files_touched` is rejected before any write (invalid_params) —
    /// no zero-row append, no event (mirrors the repo's empty-batch reject).
    #[tokio::test]
    async fn record_actual_empty_is_invalid_params() {
        let (tools, _pool) = tools().await;
        let story = seed_chain_to_story(&tools).await;
        let task = create_item(&tools, "task", Some(&story)).await;

        let err = tools
            .record_task_actual_files(Parameters(RecordTaskActualFilesParams {
                task_id: task,
                files_touched: vec![],
            }))
            .await
            .expect_err("an empty actual batch is rejected");
        // `app_error_to_mcp` maps the repo's `Validation` to `invalid_params`;
        // the message is preserved. Assert on the message (matching the
        // prevailing `err.to_string().contains(...)` idiom across the mcp tests),
        // which is robust against the exact ErrorCode constant path.
        assert!(
            err.message.contains("at least one file entry"),
            "an empty files_touched is a clean Validation reject: {err}"
        );
    }

    /// `get_story_files_footprint` returns the DISTINCT `(repo_link_id, path)`
    /// union over the story's task children, deduped across tasks AND kind: two
    /// tasks both expecting the same path collapse it to ONE entry; a path that
    /// is both expected and actual on a task appears once.
    #[tokio::test]
    async fn story_footprint_dedups_across_tasks_and_kinds() {
        let (tools, _pool) = tools().await;
        let story = seed_chain_to_story(&tools).await;
        let t1 = create_item(&tools, "task", Some(&story)).await;
        let t2 = create_item(&tools, "task", Some(&story)).await;

        // t1: expected shared.rs + both.rs; actual both.rs (cross-kind dup on
        // t1). t2: expected shared.rs. The EXPECTED set is seeded via the core
        // repo fn that `set_task_spec` wraps (the private cross-module MCP
        // method is not reachable here); the ACTUAL set uses THIS module's tool.
        repo::set_task_expected_files(
            tools.pool(),
            &t1,
            &[
                serde_json::json!("src/shared.rs"),
                serde_json::json!("src/both.rs"),
            ],
        )
        .await
        .expect("t1 expected");
        repo::set_task_expected_files(
            tools.pool(),
            &t2,
            &[serde_json::json!("src/shared.rs")],
        )
        .await
        .expect("t2 expected");
        tools
            .record_task_actual_files(Parameters(RecordTaskActualFilesParams {
                task_id: t1.clone(),
                files_touched: vec![FileRef::Path("src/both.rs".to_owned())],
            }))
            .await
            .expect("t1 actual both.rs");

        let res = tools
            .get_story_files_footprint(Parameters(GetStoryFilesFootprintParams {
                story_id: story.clone(),
            }))
            .await
            .expect("story footprint");
        // The read tool returns a JSON-array content; `into_typed::<Value>`
        // deserialises the first text content (the `json_result` mirror).
        // `FootprintFile` is Serialize-only (no Deserialize), so we parse to a
        // generic Value and pull the `path` fields out.
        let paths = footprint_paths(res);
        assert_eq!(
            paths,
            vec!["src/both.rs".to_owned(), "src/shared.rs".to_owned()],
            "DISTINCT union ordered by path: shared.rs once (two tasks), both.rs once (expected+actual)"
        );
    }

    /// `get_sprint_files_footprint` unions over the sprint's MEMBER tasks (the
    /// `sprint_tasks` junction), deduped identically to the story footprint.
    #[tokio::test]
    async fn sprint_footprint_unions_member_tasks() {
        let (tools, _pool) = tools().await;
        let story = seed_chain_to_story(&tools).await;
        let t1 = create_item(&tools, "task", Some(&story)).await;
        let t2 = create_item(&tools, "task", Some(&story)).await;

        // Open a sprint and bind both tasks to it via the core repo fns the
        // `create_sprint` / `add_tasks_to_sprint` MCP tools wrap (those private
        // cross-module methods are not reachable here).
        let sprint = repo::create_sprint(
            tools.pool(),
            &lumina_core::domain::NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
        .await
        .expect("create sprint")
        .to_string();
        repo::add_tasks_to_sprint(tools.pool(), &sprint, &[t1.as_str(), t2.as_str()])
            .await
            .expect("bind tasks to sprint");

        tools
            .record_task_actual_files(Parameters(RecordTaskActualFilesParams {
                task_id: t1.clone(),
                files_touched: vec![FileRef::Path("src/a.rs".to_owned())],
            }))
            .await
            .expect("t1 actual a.rs");
        tools
            .record_task_actual_files(Parameters(RecordTaskActualFilesParams {
                task_id: t2.clone(),
                files_touched: vec![FileRef::Path("src/b.rs".to_owned())],
            }))
            .await
            .expect("t2 actual b.rs");

        let res = tools
            .get_sprint_files_footprint(Parameters(GetSprintFilesFootprintParams {
                sprint_id: sprint.clone(),
            }))
            .await
            .expect("sprint footprint");
        let paths = footprint_paths(res);
        assert_eq!(
            paths,
            vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()],
            "sprint footprint is the DISTINCT union over member tasks"
        );
    }

    /// `reconcile_task_files` clears the untouched EXPECTED row, keeps the
    /// touched one + the over-report ACTUAL, and returns the divergence counts;
    /// a re-run is an idempotent no-op (clears 0).
    #[tokio::test]
    async fn reconcile_clears_untouched_expected_and_is_idempotent() {
        let (tools, _pool) = tools().await;
        let story = seed_chain_to_story(&tools).await;
        let task = create_item(&tools, "task", Some(&story)).await;

        // EXPECTED a.rs (touched) + b.rs (NOT touched); ACTUAL a.rs + c.rs. The
        // EXPECTED set is seeded via the core repo fn `set_task_spec` wraps; the
        // ACTUAL set + the reconcile under test use THIS module's own tools.
        repo::set_task_expected_files(
            tools.pool(),
            &task,
            &[
                serde_json::json!("src/a.rs"),
                serde_json::json!("src/b.rs"),
            ],
        )
        .await
        .expect("expected set");
        tools
            .record_task_actual_files(Parameters(RecordTaskActualFilesParams {
                task_id: task.clone(),
                files_touched: vec![
                    FileRef::Path("src/a.rs".to_owned()),
                    FileRef::Path("src/c.rs".to_owned()),
                ],
            }))
            .await
            .expect("actual set");

        let res = tools
            .reconcile_task_files(Parameters(ReconcileTaskFilesParams { task_id: task.clone() }))
            .await
            .expect("reconcile runs");
        let payload = res.structured_content.expect("reconcile payload");
        assert_eq!(payload["cleared"].as_i64(), Some(1), "the untouched expected (b.rs) is cleared");
        assert_eq!(
            payload["unexpected_actual"].as_i64(),
            Some(1),
            "one actual (c.rs) was never expected (over-report)"
        );

        // Re-run is idempotent — nothing left to clear.
        let again = tools
            .reconcile_task_files(Parameters(ReconcileTaskFilesParams { task_id: task.clone() }))
            .await
            .expect("re-reconcile runs");
        assert_eq!(
            again.structured_content.expect("payload")["cleared"].as_i64(),
            Some(0),
            "the re-run clears nothing (idempotent)"
        );
    }

    /// Extract the ordered `path` strings from a footprint read-tool result.
    /// The read tools mirror their `json_result` value into the first text
    /// content, so `into_typed::<serde_json::Value>` recovers the JSON array
    /// (`FootprintFile` is Serialize-only, so we read the generic Value rather
    /// than deserialise into the read struct).
    fn footprint_paths(res: CallToolResult) -> Vec<String> {
        let value: serde_json::Value = res.into_typed().expect("footprint json value");
        value
            .as_array()
            .expect("footprint is a JSON array")
            .iter()
            .map(|f| {
                f.get("path")
                    .and_then(|p| p.as_str())
                    .expect("each footprint entry carries a path string")
                    .to_owned()
            })
            .collect()
    }
}
