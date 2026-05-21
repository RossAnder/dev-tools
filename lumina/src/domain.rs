//! Typed domain structs for the work-item hierarchy and findings (Task 3).
//!
//! These map the SQLite rows (see `migrations/0001_init.sql`) onto serde types.
//! Conventions:
//!   * `id` / timestamp columns are `String` (TEXT in SQLite; ids are UUIDv7
//!     rendered to text, timestamps are `CURRENT_TIMESTAMP` strings).
//!   * nullable columns are `Option<T>`.
//!   * INTEGER columns are `i64`.
//!
//! All read structs derive `Serialize` for the HTTP/MCP layers. Create-bodies
//! that the HTTP (Task 4) / MCP (Task 5) layers deserialise are separate
//! `*Request` structs deriving `Deserialize` (and `JsonSchema` for rmcp), so the
//! row structs stay write-agnostic.

use serde::{Deserialize, Serialize};

/// A row of `work_items`. The 5-level hierarchy (`project > epic > feature >
/// story > task`) is an adjacency list via `parent_id`.
#[derive(Debug, Clone, Serialize)]
pub struct WorkItem {
    pub id: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub status: String,
    pub position: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// A row of `findings`. Almost every column is nullable in the schema (only
/// `id` is NOT NULL), reflecting the heterogeneous review/optimise finding
/// shapes; disposition fields (`resolved_at`/`resolution`/`defer_*`/
/// `wontfix_rationale`) are carried so deferred/wontfix imports are not lossy.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    pub work_item_id: Option<String>,
    pub kind: Option<String>,
    pub severity: Option<String>,
    pub effort: Option<String>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub symbol: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub first_flagged: Option<String>,
    pub rounds: Option<i64>,
    pub fingerprint: Option<String>,
    pub flow: Option<String>,
    pub dedup_id: Option<String>,
    pub resolved_at: Option<String>,
    pub resolution: Option<String>,
    pub defer_reason: Option<String>,
    pub defer_trigger: Option<String>,
    pub wontfix_rationale: Option<String>,
}

/// A row of `context_blocks` — the drift-killer. Shared context is one row
/// referenced by many work-items through `work_item_context`.
#[derive(Debug, Clone, Serialize)]
pub struct ContextBlock {
    pub id: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Read-aggregate for the detail endpoint: an item plus its DIRECT children,
/// its findings, and its linked context blocks. The full tree is assembled by
/// the HTTP layer / frontend from repeated `list_work_items` calls — direct
/// children are sufficient for the slice.
#[derive(Debug, Clone, Serialize)]
pub struct WorkItemDetail {
    pub item: WorkItem,
    pub children: Vec<WorkItem>,
    pub findings: Vec<Finding>,
    pub context_blocks: Vec<ContextBlock>,
}

/// Create-body for a new work item. Deserialised by the HTTP POST handler
/// (Task 4) and the MCP `create_work_item` tool (Task 5). `JsonSchema` is
/// derived for the rmcp `Parameters<T>` tool-argument contract.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CreateWorkItemRequest {
    /// One of `project`/`epic`/`feature`/`story`/`task`.
    pub kind: String,
    /// Parent work-item id; `None`/absent only for a `project`.
    #[serde(default)]
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
}

/// Update-body for a status transition. Deserialised by the HTTP PATCH handler
/// (Task 4) and the MCP `update_work_item_status` tool (Task 5).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateStatusRequest {
    pub status: String,
}
