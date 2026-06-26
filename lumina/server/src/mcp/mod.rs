//! MCP server (rmcp 1.7, Streamable-HTTP transport) — Task 5 [resolves P10].
//!
//! Exposes the work-item repository as a domain-shaped MCP tool surface over the
//! Streamable-HTTP transport, co-hosted in the same axum router / tokio runtime
//! / sqlx pool as the JSON API (`app.rs` mounts the returned service with
//! `.nest_service("/mcp", mcp::service(pool.clone()))`).
//!
//! ## Shape
//!
//! * [`LuminaTools`] is the tool-handler struct. Its `#[tool_router]` impl
//!   declares the domain tools, each taking `Parameters<T>` where `T`
//!   derives `serde::Deserialize + schemars::JsonSchema` (the rmcp tool-argument
//!   contract). Each `#[tool]` declares its OWN `Parameters<T>` wrapper struct
//!   carrying the target `id` + fields — the `domain::*Request` structs lack an
//!   `id` and are NOT reused directly (the create tool, which has no id, reuses
//!   `domain::CreateWorkItemRequest`). Where a field is a domain enum
//!   ([`Kind`]/[`Status`]/[`Severity`]/[`Disposition`]/[`ActivityType`]) the
//!   schema advertises the legal snake_case values, and an out-of-set value is
//!   rejected at deserialization → surfaces as `invalid_params`.
//! * Every tool maps to EXACTLY ONE `repo::*` mutation (preserving the repo's
//!   +1 work_items / +1 events single-mutation-path invariant) or COMPOSES
//!   existing reads — no new SQL is issued in this module. The two composers
//!   that are *not* a single repo call are `create_context_block` with an
//!   optional `link_to` (create-then-link: two independent txns, each its own
//!   mutation path) and `set_story_plan`/`set_task_spec` (which build ONE
//!   sub-object then make ONE `set_work_item_attributes` call — itself one txn).
//!   The execute→record category (ADR-0006 Step 1b + the detached-integration
//!   wave-2 follow-up) extends the same rule across the execution plane:
//!   `execute_worktree_merge` / `execute_worktree_create` each compose ONE
//!   existing record mutation (`record_worktree_merge` / `create_worktree`)
//!   with a companion-executed git intent — still no new SQL WRITES, with NO
//!   DB transaction held across the companion round-trip (the create flow's
//!   pre-flight issues three read-only scalar SELECTs through the
//!   `lumina_core::db` seam, documented in `mcp/worktrees.rs`).
//! * Every tool maps the returned `AppError` into rmcp's tool-error type via
//!   [`app_error_to_mcp`].
//! * [`service`] builds a [`StreamableHttpService`] from a per-request
//!   `service_factory` closure that CAPTURES the `Arc<SqlitePool>` and clones
//!   the `Arc` per request (clone-per-request is cheap) — the pool is never
//!   moved into a single shared instance, per P10.
//!
//! ## Tool annotations
//!
//! Read tools carry `read_only_hint = true`; `delete_work_item` carries
//! `destructive_hint = true`; the setters and `transition_status` carry
//! `idempotent_hint = true`; ALL tools carry `open_world_hint = false` (this
//! server touches only the local SQLite store — plus, for the execute tools
//! (`execute_worktree_merge` / `execute_worktree_create`), the loopback-only
//! local git companion — never an open-world resource).
//!
//! ## Tool-output / tool-error mapping (the riskiest novelty in the slice)
//!
//! The `domain` read structs (`WorkItem`, `WorkItemDetail`, …) derive only
//! `Serialize`, NOT `schemars::JsonSchema` (domain.rs is frozen for this task),
//! so the rmcp `Json<T>` output wrapper — which requires `T: Serialize +
//! JsonSchema` — cannot wrap them. Every tool therefore returns
//! `Result<CallToolResult, rmcp::ErrorData>` and builds the success result by
//! hand with `CallToolResult::success(vec![Content::json(value)?])` (structured
//! JSON content). `AppError` maps to `rmcp::ErrorData` (re-exported as
//! `rmcp::model::ErrorData`): `NotFound → resource_not_found`,
//! `Validation → invalid_params`, `Db`/`Other → internal_error` (DB internals
//! never leak — only the generic client message crosses the boundary).
//!
//! ## Security
//!
//! `allowed_hosts` is left at the rmcp 1.7 loopback default
//! (`["localhost", "127.0.0.1", "::1"]`), which is safe per
//! GHSA-89vp-x53w-74fx (DNS-rebinding, fixed ≥ 1.4.0). It is NOT widened here.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, ErrorData, ServerCapabilities, ServerInfo,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::app::AppState;
use lumina_core::db::AnyPool;
use lumina_core::domain::CreateWorkItemRequest;
use lumina_core::error::AppError;
use lumina_core::repo;

mod files;
mod findings;
mod mode;
mod planning;
// `pub(crate)` so the HTTP `get_work_item` mirror (T2) can reuse the
// projection vocabulary (`reads::Section` / `reads::project_work_item_detail`).
pub(crate) mod reads;
mod repo_links;
mod risks_alts;
mod runs_sprints;
mod scheduler;
mod sessions;
mod task_graph;
mod team_execution;
mod work_items;
mod worktrees;

#[cfg(test)]
pub(crate) mod test_support;

// Re-export every public item (the `*Params` structs, `VerificationCommands`,
// `FileRef`, `TaskActivityType`, …) from the carved tool-family modules so the
// pre-refactor public surface at `crate::mcp::*` is preserved unchanged — the
// integration tests (`tests/e2e.rs`) and `http::structured_patches` import these
// types by that path. Tool methods and the `tool_router_*` fns are impl items,
// so these globs re-export only the free types.
pub use files::*;
pub use findings::*;
pub use mode::*;
pub use planning::*;
pub use reads::*;
pub use repo_links::*;
pub use risks_alts::*;
pub use runs_sprints::*;
pub use scheduler::*;
pub use sessions::*;
pub use task_graph::*;
pub use team_execution::*;
pub use work_items::*;
pub use worktrees::*;

/// Map the crate-wide [`AppError`] into rmcp's tool-error currency.
///
/// `Db`/`Other` collapse to a generic `internal_error` so SQLite internals and
/// arbitrary `anyhow` chains never reach an MCP client — mirroring the
/// `IntoResponse` impl in `error.rs` (single error-mapping discipline across the
/// HTTP and MCP entry points).
fn app_error_to_mcp(err: AppError) -> ErrorData {
    match err {
        AppError::NotFound(m) => ErrorData::resource_not_found(m, None),
        AppError::Validation(m) => ErrorData::invalid_params(m, None),
        // Cycle is caller-fixable input (a task-dependency graph cycle); map to
        // `invalid_params` like `Validation`. The offending-edge list is included
        // in the surfaced message so the wire-task-deps SKILL / composer can act
        // on it without a separate query.
        AppError::Cycle { ref edges } => {
            let edge_str = edges
                .iter()
                .map(|(a, b)| format!("{a} -> {b}"))
                .collect::<Vec<_>>()
                .join(", ");
            ErrorData::invalid_params(
                format!("task-dependency cycle detected: [{edge_str}]"),
                None,
            )
        }
        AppError::Db(_) => ErrorData::internal_error("a database error occurred", None),
        AppError::Other(_) => ErrorData::internal_error("an internal error occurred", None),
    }
}

/// Serialise any `Serialize` repo result into a tool result carrying the value
/// as JSON text content. Used by the read tools, whose outputs (a `Vec` of
/// items / a detail aggregate) are returned as unstructured JSON content.
/// `Content::json` only fails if serialisation fails, which for these
/// owned-`String`/`Option` domain structs is effectively unreachable; it is
/// still mapped to `internal_error` rather than unwrapped.
fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    let content = Content::json(value)?;
    Ok(CallToolResult::success(vec![content]))
}

/// Build a STRUCTURED tool result from a JSON object. Used by the mutation
/// tools (`create`/`update`), whose small object payloads (`{ "id": … }`) are
/// surfaced both as `structured_content` and a JSON-text content mirror.
fn structured_result(value: serde_json::Value) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::structured(value))
}

/// Render a unit domain enum to its snake_case wire string. The enum always
/// serialises to a JSON string (unit variants), so the fallthrough is
/// unreachable — but it is mapped, not `unwrap()`-ed. Used for the repo fns that
/// take `&str` (`create_work_item`, `update_work_item_status`,
/// `append_activity`) while the param surface carries a typed enum so the schema
/// advertises the legal values.
fn enum_to_str<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => unreachable!("unit domain enum serialises to a JSON string"),
    }
}

/// The MCP tool-handler. Holds an [`AppState`] clone (whose `pool` mirrors
/// the legacy `pool` field on this struct for back-compat with the 60+
/// existing `&self.pool` call sites) plus the generated `ToolRouter` (the
/// `#[tool_router]` macro emits `Self::tool_router()`; we store its result in
/// the `tool_router` field so `#[tool_handler]` can route through it).
///
/// The state carries the PTY plumbing (`pty_registry`, `pty_transport`,
/// `pty_register_tx`) used by the HTTP PTY routes. The MCP layer no longer
/// exposes PTY tools (removed in the lumina-interactive-prompts plan,
/// 2026-05-28): the PTY service is driven exclusively via the HTTP API +
/// the SPA — no tool in this module reaches into `self.state.pty_*`. The
/// state's `companion` registry, however, IS read by the execute tools
/// (`execute_worktree_merge`, ADR-0006 Step 1b, and
/// `execute_worktree_create`, the wave-2 create sibling), which dispatch
/// coarse git intents through it.
#[derive(Clone)]
pub struct LuminaTools {
    pool: Arc<AnyPool>,
    // Threaded through both transports by the composition root
    // (`service_with_state`). Read by `execute_worktree_merge` /
    // `execute_worktree_create` (mcp/worktrees.rs) for the companion seam
    // (`state.companion`); the PTY fields stay unread here (PTY is HTTP-only).
    state: AppState,
    tool_router: ToolRouter<Self>,
}

impl LuminaTools {
    /// Construct a tool-handler over the given pool. Convenience constructor
    /// that wraps the pool in a default-PTY [`AppState`] (`pty_register_tx`
    /// stays `None`). Used by the existing test suite and by the legacy
    /// [`service(pool)`] entry point. The MCP tool surface no longer touches
    /// the PTY plumbing — the field is carried for shape parity with the
    /// composition root.
    pub fn new(pool: Arc<AnyPool>) -> Self {
        Self::with_state(AppState::new(pool))
    }

    /// Borrow the underlying concrete `SqlitePool`. Exposed so in-process tests
    /// (which drive the tool handlers directly) can assert DB state and seed
    /// prerequisite rows over the SAME pool the tools mutate — several of those
    /// assertions run RAW `sqlx::query_scalar(…).fetch_one(tools.pool())`, which
    /// needs a concrete `&SqlitePool` (a `&AnyPool` is not an sqlx Executor), so
    /// this reaches through the erased pool via [`AnyPool::sqlite`].
    pub fn pool(&self) -> &SqlitePool {
        self.pool.sqlite()
    }

    /// Construct a tool-handler over a fully-populated [`AppState`]. The MCP
    /// tools no longer reach into the state's PTY plumbing (the PTY service
    /// is driven via the HTTP layer only); the `AppState`-shaped constructor
    /// is retained so the composition root can wire one type through both
    /// the HTTP and MCP services.
    pub fn with_state(state: AppState) -> Self {
        Self {
            pool: state.pool.clone(),
            state,
            tool_router: Self::tool_router_reads()
                + Self::tool_router_work_items()
                + Self::tool_router_planning()
                + Self::tool_router_findings()
                + Self::tool_router_runs_sprints()
                + Self::tool_router_repo_links()
                + Self::tool_router_risks_alts()
                + Self::tool_router_task_graph()
                + Self::tool_router_team_execution()
                + Self::tool_router_sessions()
                + Self::tool_router_mode()
                + Self::tool_router_files()
                + Self::tool_router_worktrees()
                + Self::tool_router_scheduler(),
        }
    }
}

impl LuminaTools {
    /// Recursively build a `{ item, children: [...] }` node for `get_tree`,
    /// COMPOSED purely from `list_work_items(parent=item)` reads (no new SQL).
    /// `max_depth` (root = depth 0) bounds the descent; `None` ⇒ unbounded. The
    /// recursion is `Box::pin`-ed because an `async fn` cannot recurse by value.
    fn build_subtree<'a>(
        &'a self,
        item: lumina_core::domain::WorkItem,
        max_depth: Option<u32>,
        depth: u32,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<serde_json::Value, ErrorData>> + Send + 'a>,
    > {
        Box::pin(async move {
            // Stop descending once we hit the depth bound (children omitted).
            let at_limit = max_depth.is_some_and(|m| depth >= m);
            let children_json = if at_limit {
                Vec::new()
            } else {
                let children = repo::list_work_items(&self.pool, Some(&item.id), None)
                    .await
                    .map_err(app_error_to_mcp)?;
                let mut out = Vec::with_capacity(children.len());
                for child in children {
                    out.push(self.build_subtree(child, max_depth, depth + 1).await?);
                }
                out
            };
            Ok(serde_json::json!({ "item": item, "children": children_json }))
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LuminaTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Lumina work-item hierarchy: define/execute work items, findings, context blocks, \
                 and activity over a SQLite-canonical store. Reads (list/get/tree/sprint) are \
                 read-only; writes record one event each in the same transaction.",
            )
    }
}

/// Build the MCP service mounted at `/mcp` [resolves P10].
///
/// `StreamableHttpService::new` takes a per-request `service_factory` closure,
/// the session manager, and a config. The closure captures `pool` and clones
/// the `Arc` per request, building a FRESH [`LuminaTools`] each time — the pool
/// is never moved into a single shared instance.
///
/// The returned `StreamableHttpService` `impl tower::Service`, so `app.rs` can
/// `.nest_service("/mcp", mcp::service(pool.clone()))` it. `allowed_hosts` is
/// left at the rmcp 1.7 loopback default (safe per GHSA-89vp-x53w-74fx).
///
/// **PTY note**: this constructor synthesises a default-PTY [`AppState`] per
/// request (`pty_register_tx == None`). The MCP layer no longer exposes any
/// PTY tools (the PTY service is driven via the HTTP API only), so the
/// synthesised `AppState`'s PTY fields are unused on this path. The
/// [`service_with_state`] variant remains so the composition root can wire
/// one `AppState` shape through both the HTTP and MCP services.
pub fn service(
    pool: Arc<AnyPool>,
) -> StreamableHttpService<LuminaTools, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(LuminaTools::new(pool.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}

/// Build the MCP service from a fully-populated [`AppState`]. The
/// composition root (`app::build_router`) calls this variant so a single
/// `AppState` shape threads through both the HTTP and MCP services. The
/// MCP tool surface itself no longer reaches into the state's PTY plumbing
/// (the PTY MCP tools were removed in the lumina-interactive-prompts plan,
/// 2026-05-28 — see the `## Security: claude PTY auto-approve scope` block
/// in `lumina/CLAUDE.md`).
///
/// The factory closure clones the entire `AppState` per request; `AppState`
/// is `Clone` and its fields are `Arc`-wrapped, so the clone is cheap.
pub fn service_with_state(
    state: AppState,
) -> StreamableHttpService<LuminaTools, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(LuminaTools::with_state(state.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::db::connect_in_memory;
    use crate::mcp::test_support::*;

    /// Driving the `create_work_item` tool handler DIRECTLY writes one
    /// work_items row + one events row (the repo's single-mutation-path
    /// invariant), and the advertised tool list contains every domain tool name.
    #[tokio::test]
    async fn create_tool_writes_rows_and_lists_domain_tools() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());

        // (a) the advertised tool list contains EVERY domain tool name.
        let names: Vec<String> = tools
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        for expected in [
            // definition tools
            "create_work_item",
            "update_work_item",
            "move_work_item",
            "delete_work_item",
            "set_story_plan",
            "set_task_spec",
            "create_context_block",
            "link_context_block",
            // execution tools
            "record_task_activity",
            "transition_status",
            "add_finding",
            "update_finding",
            "resolve_finding",
            // planning / decision tools (migration 0003)
            "set_relevance",
            "set_effort",
            "set_complexity",
            "set_closure_gate",
            "add_acceptance_criterion",
            "check_acceptance_criterion",
            "uncheck_acceptance_criterion",
            "remove_acceptance_criterion",
            "add_research_note",
            "update_research_note",
            "supersede_research_note",
            "supersede_finding",
            "add_open_question",
            "add_question_option",
            "block_task_on_question",
            "set_enabling_option",
            "resolve_open_question",
            // read tools
            "list_work_items",
            "get_work_item",
            "get_tree",
            "get_sprint_view",
            // repo-link tools (migration 0004, T4)
            "add_repo_link",
            "remove_repo_link",
            "set_primary_repo",
            "list_repo_links",
            "set_finding_repo",
            // risk-register tools (migration 0005, T4 / lumina-story-planning-round-2)
            "add_risk",
            "update_risk",
            "supersede_risk",
            "remove_risk",
            // rejected-alternative tools (migration 0005, T4)
            "add_rejected_alternative",
            "update_rejected_alternative",
            "supersede_rejected_alternative",
            "remove_rejected_alternative",
            // task-dependency tools (migration 0005, T4)
            "block_task_on_task",
            "unblock_task_from_task",
            "list_task_dependencies",
            "compute_task_batches",
            // story readiness + task_kind (migration 0005, T4)
            "get_story_readiness",
            "set_task_kind",
            // task dispatch-plan + tier (migration 0006, round-3 T4)
            "get_task_dispatch_plan",
            "set_task_tier",
            // task work-queue lane setter (team-execution: lane as a
            // first-class task field, default 'implement' at create)
            "set_task_lane",
            // epic/focus shape + plan setters (migration 0010, T6)
            "set_shape",
            "set_epic_plan",
            "set_focus_plan",
            // batch-write tools (migration 0011, Part B / B18)
            "add_findings",
            "create_work_items",
            "batch_update_findings",
            // migration 0011 Part-B query tools (B21)
            "query_findings",
            "get_story_finding_queue",
            // research-note anchor query (F7 anchor pass; defined in
            // mcp/findings.rs — cross-work-item query over the migration-0024
            // research_notes.anchors JSON array via json_each, NULL-guard
            // filter over work_item_id + file/anchor predicates, read-only)
            "query_research_notes",
            // migration 0011 Part-B run/sprint/triage domain tools (B24)
            "create_run",
            "create_sprint",
            "add_tasks_to_sprint",
            "record_finding_decision",
            // team-execution work-queue tools (team-execution migration, §G / T9)
            "claim_next_task",
            "release_task",
            "renew_lease",
            "complete_task",
            "get_sprint_quiescence",
            "list_open_questions_for_sprint",
            // session-context read tool (harness-session-corpus, ADR-0004 / T8)
            "get_session_context",
            // execution-mode read tool (focus 1C.1, AC1 consumer; defined in
            // mcp/mode.rs — corroborates the caller's LUMINA_AUTONOMOUS token
            // server-side via crate::pty::mode, no DB)
            "get_execution_mode",
            // worktree / checkpoint / commit-provenance tools (migration 0016,
            // sprint-lifecycle & worktree substrate, ADR-0002 layer 2)
            "create_worktree",
            "record_worktree_merge",
            "record_worktree_rejection",
            "set_task_checkpoint",
            "record_task_commits",
            "get_worktree",
            "list_worktrees",
            "list_task_commits",
            // sprint-status transition tool (migration 0016; defined in
            // mcp/runs_sprints.rs — counted/listed/annotated here)
            "set_sprint_status",
            // git-execution companion triggers (ADR-0006 Step 1b + the
            // detached-integration ref-CAS wave 2; defined in mcp/worktrees.rs
            // — the execute→record pair: each composes ONE existing record
            // mutation (record_worktree_merge / create_worktree) with a
            // companion-executed intent (MergeWorktree / CreateWorktree))
            "execute_worktree_merge",
            "execute_worktree_create",
            // task-commit removal repair tool (review R32; defined in
            // mcp/worktrees.rs — the pair-exact delete for a bad historical
            // task_commits row)
            "remove_task_commit",
            // first-class touched-file tools (migration 0020,
            // files-touched-first-class pass / T6; defined in mcp/files.rs —
            // each wraps ONE existing repo::task_files / repo::reads fn that
            // owns its own tx + a coarse export-INERT task_files event)
            "record_task_actual_files",
            "reconcile_task_files",
            "get_story_files_footprint",
            "get_sprint_files_footprint",
            // checkpoint-suggestion read (1B-F8; defined in mcp/files.rs —
            // cross-task EXPECTED files-overlap → candidate checkpoint tasks,
            // story- or sprint-scoped; composes repo::*_checkpoint_suggestions
            // over the first-class task_files EXPECTED set, read-only no tx)
            "get_checkpoint_suggestions",
            // planning-orchestrator round-5 tools (migration 0026): three
            // writes (mcp/planning.rs) + two composed reads (mcp/reads.rs).
            "bump_plan_epoch",
            "link_task_research",
            "retire_open_question",
            "get_story_dossier",
            "get_gating_tier",
            // manual scheduler-dispatch tool (focus 1C.3, the P1 proving slice;
            // defined in mcp/scheduler.rs — lease a scheduled unit then spawn one
            // forked-autonomous claude via pty::spawn::spawn_pty_session_internal)
            "dispatch_scheduled_unit",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "advertised tools {names:?} must contain {expected}"
            );
        }

        // Exact total: catches a stray (or silently-dropped) tool that the
        // membership loop above would not.
        // 88 = 39 baseline (Round-1) + 14 Round-2 migration-0005 tools (T4)
        //    + 2 Round-3 migration-0006 tools (T4: get_task_dispatch_plan, set_task_tier)
        //    + 3 migration-0010 epic/focus tools (T6: set_shape, set_epic_plan, set_focus_plan)
        //    + 3 migration-0011 Part-B batch-write tools (B18: add_findings,
        //      create_work_items, batch_update_findings)
        //    + 2 migration-0011 Part-B query tools (B21: query_findings,
        //      get_story_finding_queue)
        //    + 4 migration-0011 Part-B domain tools (B24: create_run, create_sprint,
        //      add_tasks_to_sprint, record_finding_decision)
        //    + 6 team-execution work-queue tools (claim_next_task, release_task,
        //      renew_lease, complete_task, get_sprint_quiescence,
        //      list_open_questions_for_sprint).
        //    + 1 get_session_context (harness-session-corpus, ADR-0004 / T8:
        //      read-only ancestry/sprint-context resolver).
        //    + 9 migration-0016 sprint-lifecycle & worktree tools (ADR-0002
        //      layer 2): the 8-tool worktree/checkpoint/commit family
        //      (create_worktree, record_worktree_merge, record_worktree_rejection,
        //      set_task_checkpoint, record_task_commits, get_worktree,
        //      list_worktrees, list_task_commits) + set_sprint_status (defined in
        //      mcp/runs_sprints.rs, counted here).
        //    + 1 set_task_lane (team-execution: lane as a first-class task field,
        //      default 'implement' at create; defined in mcp/task_graph.rs).
        //    + 1 execute_worktree_merge (ADR-0006 Step 1b git-execution
        //      companion trigger; defined in mcp/worktrees.rs — composes the
        //      ONE existing record_worktree_merge mutation with a
        //      companion-executed MergeWorktree intent; no new SQL writes).
        //    + 1 execute_worktree_create (detached-integration ref-CAS plan,
        //      wave 2; defined in mcp/worktrees.rs — the create-side
        //      execute→record sibling: composes the ONE existing
        //      create_worktree mutation with a companion-executed
        //      CreateWorktree intent, the companion resolving the committish
        //      base_ref and reporting the ground-truth path/head back).
        //    + 1 remove_task_commit (review R32 follow-up; defined in
        //      mcp/worktrees.rs — pair-exact removal of a bad historical
        //      task_commits provenance edge; the R4 sha shape-validation only
        //      guards NEW record_task_commits writes).
        //    + 1 get_execution_mode (focus 1C.1, AC1 consumer; defined in
        //      mcp/mode.rs — corroborates the caller's LUMINA_AUTONOMOUS token
        //      against this process's secret via crate::pty::mode and resolves
        //      to "autonomous"/"interactive"; read-only, no DB, no write).
        //    + 4 first-class touched-file tools (migration 0020,
        //      files-touched-first-class pass / T6; defined in mcp/files.rs):
        //      record_task_actual_files (append the execution-time set →
        //      repo::add_task_actual_files), reconcile_task_files (close-time
        //      reconcile → repo::reconcile_task_files_at_close),
        //      get_story_files_footprint + get_sprint_files_footprint (the
        //      DERIVED DISTINCT (repo_link_id, path) footprint reads →
        //      repo::story_files_footprint / repo::sprint_files_footprint). Each
        //      wraps ONE existing repo fn that owns its own tx + a coarse
        //      export-INERT task_files event (the reads take none).
        //    + 1 get_checkpoint_suggestions (1B-F8; defined in mcp/files.rs —
        //      a read-only sibling of the footprint reads: cross-task EXPECTED
        //      files-overlap → candidate checkpoint task ids, story- or
        //      sprint-scoped; composes repo::story_checkpoint_suggestions /
        //      repo::sprint_checkpoint_suggestions over the first-class
        //      task_files EXPECTED set, no tx + no event).
        //    + 1 query_research_notes (F7 research-note anchor pass; defined in
        //      mcp/findings.rs beside query_findings — a read-only cross-work-item
        //      query over the migration-0024 research_notes.anchors JSON array
        //      via json_each: a static NULL-guard filter over work_item_id + the
        //      file/anchor anchor predicates → repo::query_research_notes; no tx,
        //      no event).
        //    + 5 planning-orchestrator round-5 tools (migration 0026, T4): three
        //      writes (mcp/planning.rs) — bump_plan_epoch (→ repo::bump_plan_epoch,
        //      monotonic story plan-epoch increment), link_task_research (→
        //      repo::link_task_research, the task↔research grounding edge; the repo
        //      validates task-is-task + note-live + same-story), retire_open_question
        //      (→ repo::retire_open_question) — and two composed reads (mcp/reads.rs)
        //      — get_story_dossier (→ repo::get_story_dossier, the full planning
        //      dossier) + get_gating_tier (REUSES repo::get_story_readiness's
        //      already-populated gating_tier, plus contributing signals; no new
        //      repo fn). Takes the surface 94 → 99.
        //    + 1 dispatch_scheduled_unit (focus 1C.3 manual scheduler dispatch,
        //      the P1 proving slice; defined in mcp/scheduler.rs — maps the unit
        //      kind to a build-out skill prompt, resolves+confines the project
        //      clone-dir cwd, ensures+claims a scheduled-unit lease
        //      (repo::ensure_scheduled_unit + repo::claim_next_scheduled_unit,
        //      with a targeted-claim guard that releases a mis-claimed
        //      higher-priority unit via repo::release_scheduled_unit), then spawns
        //      ONE forked-autonomous claude via
        //      pty::spawn::spawn_pty_session_internal. Takes the surface 99 → 100.
        // The six lumina-pty-service T10 PTY tools were removed in the
        // lumina-interactive-prompts plan (2026-05-28).
        assert_eq!(
            names.len(),
            100,
            "advertised tool count must be exactly 100, got {}: {names:?}",
            names.len()
        );

        // Name-uniqueness guard (router-split risk mitigation): the fourteen
        // per-family `tool_router_*` sub-routers are summed with
        // `ToolRouter::merge`, which is NAME-KEYED — a duplicate tool name
        // across two families would be silently absorbed while KEEPING the
        // bare count unchanged (the second registration overwrites the first).
        // Collect the names into a set and assert it has the SAME number of
        // entries as the list, so a collision is caught rather than masked by
        // the bare count check above.
        let unique: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(
            unique.len(),
            100,
            "advertised tool names must be UNIQUE (100 distinct), got {} distinct of {}: {names:?}",
            unique.len(),
            names.len()
        );

        // The renamed tool replaces the old name (no stale `update_work_item_status`).
        assert!(
            !names.iter().any(|n| n == "update_work_item_status"),
            "the renamed transition_status must replace update_work_item_status"
        );

        // Seed a legal chain so a `task` create is legal; then create the task
        // by driving the tool handler method directly.
        let story = seed_chain_to_story(&tools).await;

        let work_items_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_items")
                .fetch_one(pool.sqlite())
                .await
                .expect("count work_items");
        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
            .await
            .expect("count events");

        let result = tools
            .create_work_item(Parameters(CreateWorkItemRequest {
                kind: "task".to_owned(),
                parent_id: Some(story),
                title: "MCP-created task".to_owned(),
                body: Some("body".to_owned()),
                origin: None,
                outcome: None,
                shape: None,
                lane: None,
            }))
            .await
            .expect("create_work_item tool succeeds");
        assert_eq!(result.is_error, Some(false), "tool result is not an error");

        let work_items_after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_items")
                .fetch_one(pool.sqlite())
                .await
                .expect("count work_items");
        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
            .await
            .expect("count events");

        assert_eq!(
            work_items_after - work_items_before,
            1,
            "the create tool inserts exactly one work_items row"
        );
        assert_eq!(
            events_after - events_before,
            1,
            "the create tool inserts exactly one events row (outbox)"
        );
    }

    /// Fetch one advertised tool's annotation block by name.
    fn annotations_of(
        tools: &LuminaTools,
        name: &str,
    ) -> rmcp::model::ToolAnnotations {
        tools
            .tool_router
            .list_all()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("tool {name} advertised"))
            .annotations
            .unwrap_or_else(|| panic!("tool {name} carries annotations"))
    }

    /// Every tool carries `open_world_hint = false`; read tools carry
    /// `read_only_hint = true`; `delete_work_item` carries `destructive_hint`;
    /// setters + `transition_status` carry `idempotent_hint`.
    #[tokio::test]
    async fn tool_annotations_match_the_spec() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());

        // open_world_hint = false on ALL tools.
        for t in tools.tool_router.list_all() {
            let ann = t.annotations.clone().unwrap_or_else(|| {
                panic!("tool {} must carry annotations", t.name)
            });
            assert_eq!(
                ann.open_world_hint,
                Some(false),
                "tool {} must set open_world_hint=false",
                t.name
            );
        }

        // read_only_hint = true on every read tool.
        for read in [
            "list_work_items",
            "get_work_item",
            "get_tree",
            "get_sprint_view",
            "list_repo_links",
            // migration 0005 / T4 read tools.
            "list_task_dependencies",
            "compute_task_batches",
            "get_story_readiness",
            // migration 0006 / round-3 T4 read tool.
            "get_task_dispatch_plan",
            // migration 0011 / Part-B query read tools (B21).
            "query_findings",
            "get_story_finding_queue",
            // research-note anchor query (F7 anchor pass; mcp/findings.rs).
            "query_research_notes",
            // session-context read tool (harness-session-corpus, ADR-0004 / T8).
            "get_session_context",
            // execution-mode read tool (focus 1C.1, AC1 consumer; mcp/mode.rs).
            "get_execution_mode",
            // worktree / commit-provenance read tools (migration 0016, ADR-0002
            // layer 2).
            "get_worktree",
            "list_worktrees",
            "list_task_commits",
            // planning-orchestrator round-5 composed reads (migration 0026).
            "get_story_dossier",
            "get_gating_tier",
        ] {
            assert_eq!(
                annotations_of(&tools, read).read_only_hint,
                Some(true),
                "{read} must be read_only_hint=true"
            );
        }

        // destructive_hint = true on hard-delete tools.
        for destructive in [
            "delete_work_item",
            "remove_repo_link",
            // migration 0005 / T4 hard-delete tools (rows fold into the owning
            // work-item's TOML export — no independent identity).
            "remove_risk",
            "remove_rejected_alternative",
            // R32: pair-exact hard delete of a task_commits provenance edge.
            "remove_task_commit",
        ] {
            assert_eq!(
                annotations_of(&tools, destructive).destructive_hint,
                Some(true),
                "{destructive} must be destructive_hint=true"
            );
        }

        // idempotent_hint = true on the setters + transition_status.
        for idem in [
            "transition_status",
            "update_work_item",
            "move_work_item",
            "set_story_plan",
            "set_task_spec",
            "update_finding",
            "resolve_finding",
            // planning / decision setters (migration 0003)
            "set_relevance",
            "set_effort",
            "set_complexity",
            "set_closure_gate",
            "check_acceptance_criterion",
            "uncheck_acceptance_criterion",
            "update_research_note",
            "supersede_research_note",
            "supersede_finding",
            "block_task_on_question",
            "set_enabling_option",
            "resolve_open_question",
            // repo-link setters (migration 0004, T4)
            "set_primary_repo",
            "set_finding_repo",
            // migration 0005 / T4 setters + supersession + edge removal.
            "update_risk",
            "supersede_risk",
            "update_rejected_alternative",
            "supersede_rejected_alternative",
            "unblock_task_from_task",
            "set_task_kind",
            // migration 0006 / round-3 T4 tier setter.
            "set_task_tier",
            // team-execution lane setter (re-stamp / clear is a no-op-on-repeat).
            "set_task_lane",
            // migration 0011 Part-B batch-write tool (B18): the triage update is
            // COALESCE-shaped, so re-applying the same updates is idempotent.
            "batch_update_findings",
            // migration 0016 (ADR-0002 layer 2): setting the same checkpoint flag
            // twice is a no-op. (set_sprint_status is NEITHER idempotent — a
            // repeated transition is illegal — NOR read-only, so it appears in
            // neither hint list.)
            "set_task_checkpoint",
            // planning-orchestrator round-5 idempotent writes (migration 0026):
            // re-linking the same task↔research edge / re-retiring a question is a
            // no-op. (bump_plan_epoch is NOT idempotent — each call increments —
            // so it carries no idempotent_hint.)
            "link_task_research",
            "retire_open_question",
        ] {
            assert_eq!(
                annotations_of(&tools, idem).idempotent_hint,
                Some(true),
                "{idem} must be idempotent_hint=true"
            );
        }
    }
}
