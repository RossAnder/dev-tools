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
//! server touches only the local SQLite store, never an open-world resource).
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
use crate::db::AnyPool;
use crate::domain::CreateWorkItemRequest;
use crate::error::AppError;
use crate::repo;

mod findings;
mod planning;
mod reads;
mod repo_links;
mod risks_alts;
mod runs_sprints;
mod task_graph;
mod team_execution;
mod work_items;

#[cfg(test)]
pub(crate) mod test_support;

// Re-export every public item (the `*Params` structs, `VerificationCommands`,
// `FileRef`, `TaskActivityType`, …) from the carved tool-family modules so the
// pre-refactor public surface at `crate::mcp::*` is preserved unchanged — the
// integration tests (`tests/e2e.rs`) and `http::structured_patches` import these
// types by that path. Tool methods and the `tool_router_*` fns are impl items,
// so these globs re-export only the free types.
pub use findings::*;
pub use planning::*;
pub use reads::*;
pub use repo_links::*;
pub use risks_alts::*;
pub use runs_sprints::*;
pub use task_graph::*;
pub use team_execution::*;
pub use work_items::*;

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
/// the SPA. `AppState` is still cloned onto `LuminaTools` so the field
/// shape matches the composition root, but no tool in this module reaches
/// into `self.state.pty_*`.
#[derive(Clone)]
pub struct LuminaTools {
    pool: Arc<AnyPool>,
    // Held for shape parity with the composition root (`service_with_state`):
    // the field is currently unread because the MCP layer no longer exposes
    // PTY tools, but `AppState` is still threaded through both transports so
    // a future tool that needs cross-cutting state can reach for it without
    // a constructor churn.
    #[allow(dead_code)]
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
                + Self::tool_router_team_execution(),
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
        item: crate::domain::WorkItem,
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
    use crate::db::connect_in_memory;
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
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "advertised tools {names:?} must contain {expected}"
            );
        }

        // Exact total: catches a stray (or silently-dropped) tool that the
        // membership loop above would not.
        // 73 = 39 baseline (Round-1) + 14 Round-2 migration-0005 tools (T4)
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
        // The six lumina-pty-service T10 PTY tools were removed in the
        // lumina-interactive-prompts plan (2026-05-28).
        assert_eq!(
            names.len(),
            73,
            "advertised tool count must be exactly 73, got {}: {names:?}",
            names.len()
        );

        // Name-uniqueness guard (router-split risk mitigation): the nine
        // per-family `tool_router_*` sub-routers are summed with
        // `ToolRouter::merge`, which is NAME-KEYED — a duplicate tool name
        // across two families would be silently absorbed while KEEPING the
        // count at 73 (the second registration overwrites the first). Collect
        // the names into a set and assert it ALSO has 73 entries, so a
        // collision is caught rather than masked by the bare count check above.
        let unique: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(
            unique.len(),
            73,
            "advertised tool names must be UNIQUE (73 distinct), got {} distinct of {}: {names:?}",
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
            // migration 0011 Part-B batch-write tool (B18): the triage update is
            // COALESCE-shaped, so re-applying the same updates is idempotent.
            "batch_update_findings",
        ] {
            assert_eq!(
                annotations_of(&tools, idem).idempotent_hint,
                Some(true),
                "{idem} must be idempotent_hint=true"
            );
        }
    }
}
