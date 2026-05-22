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

use schemars::JsonSchema;
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
    /// Nullable JSON object of kind-specific fields (migration 0002); `None`
    /// means "no kind-specific fields".
    pub attributes: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

/// A row of `work_item_activity` (migration 0002): the append-only per-item
/// activity log, ordered by the per-item monotonic `seq`. Read aggregate only —
/// `Serialize` but not `JsonSchema` (mirrors `WorkItem`/`Finding`).
#[derive(Debug, Clone, Serialize)]
pub struct WorkItemActivity {
    pub id: String,
    pub work_item_id: String,
    pub seq: i64,
    pub entry_kind: String,
    pub author: Option<String>,
    pub summary: String,
    pub payload: Option<serde_json::Value>,
    pub created_at: String,
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
    /// The item's activity-log rows (migration 0002), ordered by `seq`.
    pub activity: Vec<WorkItemActivity>,
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

/// Partial-update body for a work item. Every field is optional with
/// SET-OR-LEAVE semantics: an absent/`None` field leaves the column untouched
/// (the repo's `COALESCE(?, col)` write), it does NOT clear the column to NULL.
/// Deserialised by the HTTP PATCH handler (Task 4) and the MCP update tool
/// (Task 5); `JsonSchema` is derived for the rmcp `Parameters<T>` contract.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateWorkItemRequest {
    /// New title; absent leaves the existing title unchanged.
    #[serde(default)]
    pub title: Option<String>,
    /// New body; absent leaves the existing body unchanged (does NOT clear it).
    #[serde(default)]
    pub body: Option<String>,
    /// New status; absent leaves the existing status unchanged.
    #[serde(default)]
    pub status: Option<Status>,
    /// New sibling-ordering position; absent leaves the existing position unchanged.
    #[serde(default)]
    pub position: Option<i64>,
    /// New kind-specific attributes JSON object; absent leaves the existing
    /// attributes unchanged (does NOT clear them).
    #[serde(default)]
    pub attributes: Option<serde_json::Value>,
}

/// Partial-update body for a finding's mutable fields. Every field is optional
/// with SET-OR-LEAVE semantics (absent ⇒ column untouched). Deserialised by the
/// HTTP (Task 4) / MCP (Task 5) update path; `JsonSchema` for the rmcp contract.
/// The immutable identity/provenance columns (`id`, `work_item_id`, `kind`,
/// `fingerprint`, `dedup_id`, `first_flagged`, `flow`) are intentionally absent;
/// terminal disposition (`resolved_at`/`resolution`/`defer_*`/`wontfix_*`) is
/// driven by the dedicated `resolve_finding(disposition)` path, not this body.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateFindingRequest {
    /// New severity; absent leaves the existing severity unchanged.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// New effort estimate; absent leaves the existing effort unchanged.
    #[serde(default)]
    pub effort: Option<String>,
    /// New category; absent leaves the existing category unchanged.
    #[serde(default)]
    pub category: Option<String>,
    /// New workflow status; absent leaves the existing status unchanged.
    #[serde(default)]
    pub status: Option<String>,
    /// New offending file path; absent leaves the existing file unchanged.
    #[serde(default)]
    pub file: Option<String>,
    /// New line number; absent leaves the existing line unchanged.
    #[serde(default)]
    pub line: Option<i64>,
    /// New symbol name; absent leaves the existing symbol unchanged.
    #[serde(default)]
    pub symbol: Option<String>,
    /// New one-line summary; absent leaves the existing summary unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New long-form description; absent leaves the existing description unchanged.
    #[serde(default)]
    pub description: Option<String>,
}

/// The five legal work-item kinds, ordered parent→child (`project` is the root).
/// Mirrors the `KINDS` constant in `repo.rs` and the hierarchy trigger pair in
/// migration `0001_init.sql`. Serialises snake_case so the wire value matches
/// the TEXT stored in `work_items.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Root of the hierarchy; has a NULL parent.
    Project,
    /// Child of a `project`.
    Epic,
    /// Child of an `epic`.
    Feature,
    /// Child of a `feature`.
    Story,
    /// Leaf; child of a `story`.
    Task,
}

/// The legal work-item workflow statuses. Slice-1 storage is free-text
/// (migration 0001 declares `status` as plain TEXT with no CHECK), but the MCP
/// param surface advertises this typed set so callers send legal values; the
/// repo (Task 3) validates against it. Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Not yet started.
    Todo,
    /// Actively being worked.
    InProgress,
    /// Awaiting review / verification.
    Blocked,
    /// Completed.
    Done,
    /// Abandoned without completion.
    Cancelled,
}

/// Finding severities. Confirmed against the importer fixtures (e.g.
/// `severity = "suggestion"` in `import.rs` tests). Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Must-fix; blocks acceptance.
    Critical,
    /// Should-fix; significant but not blocking.
    Major,
    /// Nice-to-fix.
    Minor,
    /// Advisory only.
    Suggestion,
}

/// The legal `work_item_activity.entry_kind` set (migration 0002 stores it as
/// free TEXT; this enum is the canonical legal set the repo validates against,
/// per the Task-2 spec). Serialises snake_case — note `status_transition` etc.
///
/// NOTE (flagged deviation): the importer's `DROPPED_ITEM_TYPES` in `import.rs`
/// uses the HYPHENATED `"status-transition"` for the source-flow item type,
/// whereas this enum's snake_case wire value is `"status_transition"`
/// (underscore). These name two different things — the importer drops legacy
/// flow items by their source string and never writes them as `entry_kind`,
/// while this enum governs new activity writes — but the near-collision is
/// surfaced here rather than silently reconciled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    /// Task execution record.
    Execution,
    /// Verification / acceptance-check record.
    Verification,
    /// A deviation from the plan.
    Deviation,
    /// A deferral of work.
    Deferral,
    /// A reconciliation pass.
    Reconcile,
    /// A status transition (serialises as `status_transition`).
    StatusTransition,
    /// A checkpoint marker.
    Checkpoint,
    /// A vet / gate decision.
    Vet,
    /// A free-form human comment.
    Comment,
}

/// Terminal resolution dispositions for a finding, driving the dedicated
/// `resolve_finding(disposition)` repo path (Task 3) which stamps
/// `resolved_at`/`resolution`/`wontfix_rationale`. Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// The finding was fixed.
    Fixed,
    /// Acknowledged but intentionally not fixed (carries a rationale).
    Wontfix,
    /// Re-checked and found to be a non-issue / no longer present.
    VerifiedClean,
    /// Deferred to a later flow (carries a defer reason/trigger).
    Deferred,
    /// A duplicate of another finding.
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip an enum value through serde JSON and assert the wire form is
    /// exactly the expected snake_case string.
    fn assert_snake<T>(value: T, expected: &str)
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug + Copy,
    {
        let json = serde_json::to_value(value).expect("serialise");
        assert_eq!(json, serde_json::Value::String(expected.to_owned()), "wire form");
        let back: T = serde_json::from_value(json).expect("deserialise");
        assert_eq!(back, value, "round-trip");
    }

    #[test]
    fn enums_round_trip_snake_case() {
        assert_snake(Kind::Project, "project");
        assert_snake(Kind::Task, "task");
        assert_snake(Status::InProgress, "in_progress");
        assert_snake(Status::Done, "done");
        assert_snake(Severity::Suggestion, "suggestion");
        assert_snake(Severity::Critical, "critical");
        assert_snake(ActivityType::StatusTransition, "status_transition");
        assert_snake(ActivityType::Execution, "execution");
        assert_snake(ActivityType::Vet, "vet");
        assert_snake(Disposition::VerifiedClean, "verified_clean");
        assert_snake(Disposition::Wontfix, "wontfix");
    }

    /// Recursively collect every advertised string variant from a JSON schema
    /// value: strings inside any `enum` array, plus any scalar `const` value.
    /// schemars 1 emits a flat top-level `enum` for bare unit enums but switches
    /// to a `oneOf` of `const`-tagged subschemas once variants carry doc comments,
    /// so the test must accept both shapes.
    fn collect_schema_variants(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(arr) = map.get("enum").and_then(|e| e.as_array()) {
                    out.extend(arr.iter().filter_map(|v| v.as_str()).map(str::to_owned));
                }
                if let Some(c) = map.get("const").and_then(|c| c.as_str()) {
                    out.push(c.to_owned());
                }
                for v in map.values() {
                    collect_schema_variants(v, out);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    collect_schema_variants(v, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn kind_schema_lists_all_variants() {
        let schema = schemars::schema_for!(Kind);
        let value = serde_json::to_value(&schema).expect("schema to value");
        let mut got = Vec::new();
        collect_schema_variants(&value, &mut got);
        got.sort_unstable();
        got.dedup();
        let mut expected = ["project", "epic", "feature", "story", "task"];
        expected.sort_unstable();
        assert_eq!(got, expected, "Kind schema advertises all five variants");
    }
}
