//! Scheduler OBSERVABILITY read tool (focus 1C.3, story AC #6 — the
//! observability half): `get_scheduler_state`, the operator-facing READ surface
//! over the in-process scheduler.
//!
//! It composes the DB-derived snapshot ([`repo::scheduler_state`] — the bucketed
//! `scheduled_units` rows + the ungrilled stub-triage queue) with the live
//! `control` master-switch/scope read off [`AppState::scheduler_control`] (a
//! server-process handle, NOT DB state — the SAME `Arc<SchedulerControl>` the loop
//! reads and the `POST /api/scheduler/control` route mutates). Both the `control`
//! fields and the buckets are available to the MCP tool because `LuminaTools`
//! holds an [`AppState`] (`self.state`), so — unlike the worry in the task spec —
//! there is NO HTTP-only gap: the MCP tool returns the full snapshot.
//!
//! The JSON-shaping is single-sourced in [`render_scheduler_state`], called by BOTH
//! this tool and the HTTP mirror (`http/scheduler_state.rs`) — precedent: the
//! HTTP scheduler routes import `crate::mcp::dispatch_scheduled_unit_flow`.
//!
//! Read-only: no tx, no event (the repo fn takes none; the control read is an
//! atomic load + a lock snapshot). Registers via the `tool_router_scheduler_state`
//! sub-router, summed into the combined `tool_router` field by
//! [`LuminaTools::with_state`].

use super::*;

use crate::scheduler::SchedulerControl;
use lumina_core::repo::SchedulerState;

/// Arguments for the `get_scheduler_state` read tool — none. The snapshot is a
/// process-global view, so there is nothing to scope; an empty params struct
/// keeps the rmcp `Parameters<T>` contract.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetSchedulerStateParams {}

/// Shape the operator snapshot JSON from the DB-derived [`SchedulerState`] + the
/// live [`SchedulerControl`]. SINGLE SOURCE for both the MCP tool and the HTTP
/// mirror so the two never drift. The `control.scope` is rendered as a SORTED
/// array (or `null` when unrestricted) for deterministic output; each unit bucket
/// carries both a `count` and the row list.
pub(crate) fn render_scheduler_state(
    snapshot: SchedulerState,
    control: &SchedulerControl,
) -> serde_json::Value {
    let scope = control.scope_snapshot().map(|set| {
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    });
    let bucket = |units: &[lumina_core::domain::ScheduledUnit]| {
        serde_json::json!({ "count": units.len(), "units": units })
    };
    serde_json::json!({
        "control": {
            "enabled": control.is_enabled(),
            // `null` when no scope restriction is set; a sorted array otherwise.
            "scope": scope,
        },
        "units": {
            "dispatched": bucket(&snapshot.units.dispatched),
            "ready": bucket(&snapshot.units.ready),
            "stuck": bucket(&snapshot.units.stuck),
            "cancelled": bucket(&snapshot.units.cancelled),
            "parked": bucket(&snapshot.units.parked),
        },
        "stub_triage_queue": snapshot.stub_triage_queue,
    })
}

#[tool_router(router = tool_router_scheduler_state, vis = "pub(crate)")]
impl LuminaTools {
    // ---- Scheduler observability read (read_only_hint = true) --------------

    /// Read the in-process scheduler's state — what it is doing + the human entry
    /// point for stub stories. Composes [`repo::scheduler_state`] (the bucketed
    /// `scheduled_units` rows — `dispatched` (pending+leased) / `ready`
    /// (pending+unleased) / `stuck` (stale) / `cancelled` / `parked`
    /// (pending+driver-blocked-on-a-question) — plus the `stub_triage_queue`, the
    /// ungrilled backlog stories needing operator framing) with the live `control`
    /// master-switch + dispatch scope off `AppState`. Read-only: no DB write, no
    /// event. Returns `{ control{enabled, scope}, units{...}, stub_triage_queue }`.
    #[tool(
        description = "Read the in-process scheduler's observability snapshot: control { enabled, scope }, the scheduled_units bucketed by state (dispatched = pending+leased, ready = pending+unleased, stuck = stale, cancelled, and parked = pending whose driving work_item is blocked on an open question — parked overlaps dispatched/ready), and stub_triage_queue (the UNGRILLED backlog stories — no problem_statement, no children, active ancestors — the human entry point that is NEVER auto-dispatched, the clean complement of the auto-build candidates). Read-only; no DB write, no event.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_scheduler_state(
        &self,
        Parameters(GetSchedulerStateParams {}): Parameters<GetSchedulerStateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_scheduler_state", "mcp tool invoked");
        let snapshot = repo::scheduler_state(&self.pool)
            .await
            .map_err(app_error_to_mcp)?;
        let value = render_scheduler_state(snapshot, &self.state.scheduler_control);
        json_result(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::db::connect_in_memory;
    use lumina_core::repo::create_work_item;

    /// Build a tool handler over a fresh in-memory pool.
    async fn tools() -> LuminaTools {
        let pool = std::sync::Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        LuminaTools::new(pool)
    }

    /// `get_scheduler_state` returns the control snapshot (default-disabled,
    /// unrestricted scope) and empty buckets over a fresh store — proving the MCP
    /// tool reaches `AppState.scheduler_control` (NO HTTP-only gap) and composes
    /// the DB read.
    #[tokio::test]
    async fn get_scheduler_state_composes_control_and_buckets() {
        let tools = tools().await;
        // Seed one ready (pending+unleased) build_story unit over a project so the
        // `ready` bucket is non-empty and proves the DB read landed.
        let project = create_work_item(tools.pool(), "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        repo::ensure_scheduled_unit(
            tools.pool(),
            lumina_core::domain::ScheduledUnitKind::BuildStory,
            &project,
        )
        .await
        .expect("ensure ready unit");

        let res = tools
            .get_scheduler_state(Parameters(GetSchedulerStateParams {}))
            .await
            .expect("get_scheduler_state succeeds");
        assert_eq!(res.is_error, Some(false), "read tool is not an error");
        let value: serde_json::Value = res.into_typed().expect("snapshot json value");

        // Control: default AppState is disabled, unrestricted scope.
        assert_eq!(value["control"]["enabled"], serde_json::json!(false), "default disabled");
        assert_eq!(value["control"]["scope"], serde_json::Value::Null, "no scope restriction");

        // The seeded ready unit shows up exactly once, in the ready bucket.
        assert_eq!(value["units"]["ready"]["count"], serde_json::json!(1), "one ready unit");
        assert_eq!(value["units"]["dispatched"]["count"], serde_json::json!(0));
        assert_eq!(value["units"]["stuck"]["count"], serde_json::json!(0));
        assert_eq!(value["units"]["cancelled"]["count"], serde_json::json!(0));
        assert_eq!(
            value["units"]["ready"]["units"][0]["work_item_id"],
            serde_json::json!(project),
            "the ready unit drives the seeded project"
        );

        // The stub_triage_queue is present (empty here — no backlog stub seeded).
        assert!(value["stub_triage_queue"].is_array(), "stub_triage_queue is an array");
        assert_eq!(value["stub_triage_queue"].as_array().unwrap().len(), 0, "no stub seeded");
    }
}
