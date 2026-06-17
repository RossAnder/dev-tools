//! MCP findings tools carved out of the `mcp` module's combined tool router
//! (structural split; behaviour unchanged).
//!
//! The nine findings tools (`add_finding`, `update_finding`, `resolve_finding`,
//! `supersede_finding`, `add_findings`, `batch_update_findings`,
//! `query_findings`, `get_story_finding_queue`, `set_finding_repo`) and their
//! `*Params` structs (plus the batch element structs `BatchFindingInput` /
//! `FindingTriageInput`) live here. They register via the `tool_router_findings`
//! sub-router, summed into the combined field by `LuminaTools::with_state`.

use super::*;

use lumina_core::domain::{Disposition, Origin, Severity};
use lumina_core::repo::NewFinding;

/// Arguments for the `add_finding` write tool → `repo::create_finding`. Carries
/// the work-item id plus the common finding fields; the typed `severity` enum
/// advertises the legal values.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddFindingParams {
    /// The work-item id the finding attaches to.
    pub work_item_id: String,
    /// The finding kind (free-text classification, e.g. `review`/`optimise`).
    #[serde(default)]
    pub kind: Option<String>,
    /// The finding severity; one of `critical`/`major`/`minor`/`suggestion`.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// An effort estimate; optional free-text.
    #[serde(default)]
    pub effort: Option<String>,
    /// A category; optional free-text.
    #[serde(default)]
    pub category: Option<String>,
    /// The offending file path; optional.
    #[serde(default)]
    pub file: Option<String>,
    /// The offending line number; optional.
    #[serde(default)]
    pub line: Option<i64>,
    /// The offending symbol name; optional.
    #[serde(default)]
    pub symbol: Option<String>,
    /// A one-line summary of the finding.
    #[serde(default)]
    pub summary: Option<String>,
    /// A long-form description of the finding.
    #[serde(default)]
    pub description: Option<String>,
    /// The evidence grade / weighting (`high|medium|low`); optional free-text
    /// (migration 0003).
    #[serde(default)]
    pub confidence: Option<String>,
    /// Optional provenance stamp (which command produced this finding); one of
    /// `plan`/`implement`/`review`/`optimise`/`tdd`/`human`/`none` (migration 0003).
    #[serde(default)]
    pub origin: Option<Origin>,
    /// Optional FK to a `repo_links` row (migration 0004); when set, the
    /// finding's `file` lives in the named non-primary linked repo. Omitting
    /// this (the default) means the file lives in the project's primary
    /// linked repo (implicit-primary resolution at read time).
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Arguments for the `update_finding` write tool: a partial set-or-leave update.
/// Carries the target `id` plus the optional mutable fields (mirrors
/// `domain::UpdateFindingRequest`, which lacks `id`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateFindingParams {
    /// The finding id to update.
    pub id: String,
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
    /// New evidence grade (`high|medium|low`); absent leaves the existing
    /// confidence unchanged (migration 0003).
    #[serde(default)]
    pub confidence: Option<String>,
    /// New FK to a `repo_links` row (migration 0004); absent leaves the
    /// existing binding unchanged (SET-OR-LEAVE — clearing back to the primary
    /// uses the dedicated `set_finding_repo` tool).
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Arguments for the `resolve_finding` write tool → `repo::resolve_finding`. The
/// typed `disposition` enum advertises the legal terminal dispositions.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResolveFindingParams {
    /// The finding id to resolve.
    pub id: String,
    /// The terminal disposition; one of
    /// `fixed`/`wontfix`/`verified_clean`/`deferred`/`duplicate`.
    pub disposition: Disposition,
    /// Optional free-text resolution note.
    #[serde(default)]
    pub resolution: Option<String>,
    /// Optional rationale (used for `wontfix`).
    #[serde(default)]
    pub rationale: Option<String>,
}

/// Arguments for the `supersede_finding` write tool → `repo::supersede_finding`
/// (set the old finding's `superseded_by`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SupersedeFindingParams {
    /// The superseded (old) finding id.
    pub old_id: String,
    /// The superseding (new) finding id.
    pub new_id: String,
}

/// Arguments for the `set_finding_repo` write tool → `repo::set_finding_repo`.
/// Omitting `repo_id` clears the binding (the finding falls back to the
/// project's primary linked repo at read time).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetFindingRepoParams {
    /// The finding id whose repo binding to set or clear.
    pub finding_id: String,
    /// The repo-link id to bind to; omit to clear back to implicit-primary
    /// resolution. The target row must belong to the finding's project
    /// ancestor (repo-level project-scope check).
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// One finding in the `add_findings` batch. Mirrors the common subset of
/// [`AddFindingParams`] (the heterogeneous review/optimise finding shape) minus
/// the batch-owned channels: `dedup_id` (the repo STAMPS each finding's content
/// hash itself — callers do NOT supply it) and `run_id` (a top-level field on
/// [`AddFindingsParams`], applied to every element). The typed `severity` enum
/// advertises the legal `critical|major|minor|suggestion` values; a bogus value
/// fails deserialisation → `invalid_params` before the handler runs.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchFindingInput {
    /// The work-item id this finding attaches to.
    pub work_item_id: String,
    /// The finding kind (free-text classification, e.g. `review`/`optimise`).
    #[serde(default)]
    pub kind: Option<String>,
    /// The finding severity; one of `critical`/`major`/`minor`/`suggestion`.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// An effort estimate; optional free-text.
    #[serde(default)]
    pub effort: Option<String>,
    /// A category; optional free-text.
    #[serde(default)]
    pub category: Option<String>,
    /// The offending file path; optional.
    #[serde(default)]
    pub file: Option<String>,
    /// The offending line number; optional.
    #[serde(default)]
    pub line: Option<i64>,
    /// The offending symbol name; optional.
    #[serde(default)]
    pub symbol: Option<String>,
    /// A one-line summary of the finding.
    #[serde(default)]
    pub summary: Option<String>,
    /// A long-form description of the finding.
    #[serde(default)]
    pub description: Option<String>,
    /// The evidence grade / weighting (`high|medium|low`); optional free-text.
    #[serde(default)]
    pub confidence: Option<String>,
    /// Optional provenance stamp (which command produced this finding); one of
    /// `plan`/`implement`/`review`/`optimise`/`tdd`/`human`/`none`.
    #[serde(default)]
    pub origin: Option<Origin>,
    /// Optional FK to a `repo_links` row (migration 0004); omitting it (NULL)
    /// means the file lives in the project's primary linked repo.
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Arguments for the `add_findings` batch write tool → `repo::add_findings`
/// (B17a). A top-level `run_id` (optional) is applied to EVERY element; the
/// repo stamps each element's dedup content hash itself, so a dedup-collapse is
/// counted as `skipped` (NOT an error). A validation error aborts the whole
/// batch.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddFindingsParams {
    /// Optional FK to a `runs.id` row; when present, every finding in the batch
    /// is associated with this review/optimise run.
    #[serde(default)]
    pub run_id: Option<String>,
    /// The findings to insert (each attaches to its own `work_item_id`).
    pub items: Vec<BatchFindingInput>,
}

/// One finding-triage update in the `batch_update_findings` batch. Set-or-leave:
/// a `None` field leaves that column unchanged (`COALESCE`). The `status` field
/// accepts NON-terminal values only — a terminal [`Disposition`] value is
/// rejected (terminal dispositions belong to `resolve_finding`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindingTriageInput {
    /// The finding id to update.
    pub finding_id: String,
    /// New triage state; absent leaves the existing value unchanged.
    #[serde(default)]
    pub triage_state: Option<String>,
    /// New severity; one of `critical`/`major`/`minor`/`suggestion`; absent
    /// leaves the existing severity unchanged.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// New category; absent leaves the existing category unchanged.
    #[serde(default)]
    pub category: Option<String>,
    /// New NON-terminal workflow status; absent leaves it unchanged. A terminal
    /// disposition (`fixed`/`wontfix`/`verified_clean`/`deferred`/`duplicate`)
    /// is rejected — use `resolve_finding` for terminal dispositions.
    #[serde(default)]
    pub status: Option<String>,
}

/// Arguments for the `batch_update_findings` batch write tool →
/// `repo::batch_update_findings` (B17c). All-or-nothing: a missing finding id
/// (`NotFound`) or a terminal-disposition `status` (`Validation`) aborts the
/// whole batch. Returns the count of findings updated.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchUpdateFindingsParams {
    /// The per-finding triage updates to apply.
    pub updates: Vec<FindingTriageInput>,
}

/// Arguments for the `get_story_finding_queue` read tool →
/// `repo::get_story_finding_queue` (migration 0011). The queue spans the story
/// itself plus its DIRECT task children (excluding any on tombstoned items).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetStoryFindingQueueParams {
    /// The story work-item id whose live finding-queue to compose.
    pub story_id: String,
}

/// Arguments for the `query_research_notes` read tool →
/// `repo::query_research_notes` (the F7 anchor pass). Every field is optional
/// (the static NULL-guard filter); an absent field does not constrain its
/// predicate. The owned-`String` shape deserialises off the wire and converts
/// to the borrowing `repo::QueryResearchNotesFilter` at the handler.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryResearchNotesParams {
    /// Constrain to research notes on this work-item; absent ⇒ no constraint.
    #[serde(default)]
    pub work_item_id: Option<String>,
    /// Constrain to notes citing this FILE — match a note whose anchors hold an
    /// anchor EQUAL to this path OR of the `<path>:<line>` form for it; absent
    /// ⇒ no constraint.
    #[serde(default)]
    pub file: Option<String>,
    /// Constrain to notes carrying this EXACT anchor string (a specific
    /// `path:line` cite or http(s) URL); absent ⇒ no constraint.
    #[serde(default)]
    pub anchor: Option<String>,
}

#[tool_router(router = tool_router_findings, vis = "pub(crate)")]
impl LuminaTools {
    /// Create a finding attached to a work item (single repo call → `create_finding`).
    #[tool(
        description = "Add a finding to a work item (kind/severity/effort/category/file/line/symbol/summary/description). The optional `repo_id` is an FK to a `repo_links` row (migration 0004); omitting it (NULL) means the file lives in the project's primary linked repo. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn add_finding(
        &self,
        Parameters(p): Parameters<AddFindingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_finding", "mcp tool invoked");
        let origin_str = p.origin.map(enum_to_str);
        let finding = NewFinding {
            kind: p.kind.as_deref(),
            severity: p.severity,
            effort: p.effort.as_deref(),
            category: p.category.as_deref(),
            status: None,
            file: p.file.as_deref(),
            line: p.line,
            symbol: p.symbol.as_deref(),
            summary: p.summary.as_deref(),
            description: p.description.as_deref(),
            origin: origin_str.as_deref(),
            confidence: p.confidence.as_deref(),
            repo_id: p.repo_id.as_deref(),
            ..NewFinding::default()
        };
        let id = repo::create_finding(&self.pool, &p.work_item_id, &finding)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a finding (single repo call → `update_finding`).
    #[tool(
        description = "Partially update a finding by id (severity/effort/category/status/file/line/symbol/summary/description/confidence/repo_id; absent fields unchanged). The optional `repo_id` is an FK to a `repo_links` row (migration 0004); omitting it leaves the existing binding unchanged (use `set_finding_repo` to clear it back to the primary). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_finding(
        &self,
        Parameters(p): Parameters<UpdateFindingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "update_finding", "mcp tool invoked");
        let req = lumina_core::domain::UpdateFindingRequest {
            severity: p.severity,
            effort: p.effort,
            category: p.category,
            status: p.status,
            file: p.file,
            line: p.line,
            symbol: p.symbol,
            summary: p.summary,
            description: p.description,
            confidence: p.confidence,
            repo_id: p.repo_id,
        };
        repo::update_finding(&self.pool, &p.id, &req)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Resolve a finding to a terminal disposition (single repo call →
    /// `resolve_finding`).
    #[tool(
        description = "Resolve a finding to a terminal disposition (fixed/wontfix/verified_clean/deferred/duplicate). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn resolve_finding(
        &self,
        Parameters(ResolveFindingParams { id, disposition, resolution, rationale }): Parameters<
            ResolveFindingParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "resolve_finding", "mcp tool invoked");
        repo::resolve_finding(
            &self.pool,
            &id,
            disposition,
            resolution.as_deref(),
            rationale.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Supersede one finding with another (single repo call →
    /// `supersede_finding`; sets the old finding's `superseded_by`).
    #[tool(
        description = "Supersede an old finding with a new one (sets the old finding's superseded_by). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn supersede_finding(
        &self,
        Parameters(SupersedeFindingParams { old_id, new_id }): Parameters<SupersedeFindingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "supersede_finding", "mcp tool invoked");
        repo::supersede_finding(&self.pool, &old_id, &new_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "old_id": old_id, "new_id": new_id }))
    }

    /// Set or clear a finding's repo binding (single repo call →
    /// `repo::set_finding_repo`). Omitting `repo_id` clears the binding back
    /// to implicit-primary resolution.
    #[tool(
        description = "Set a finding's `repo_id` to a non-primary linked repo, or omit `repo_id` to clear the binding (the finding then falls back to the project's primary linked repo at read time). The target row must belong to the finding's project ancestor (cross-project ids surface as invalid_params). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_finding_repo(
        &self,
        Parameters(SetFindingRepoParams { finding_id, repo_id }): Parameters<SetFindingRepoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_finding_repo", "mcp tool invoked");
        repo::set_finding_repo(&self.pool, &finding_id, repo_id.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "finding_id": finding_id }))
    }

    /// Bulk-insert a batch of findings under ONE transaction (single repo call →
    /// `repo::add_findings`). The repo STAMPS each finding's dedup content hash
    /// itself, so a dedup-collapse onto an existing live row is counted as
    /// `skipped` (NOT an error); a validation error aborts the whole batch.
    /// Returns `{ added, skipped, skipped_ids }`.
    #[tool(
        description = "Bulk-add findings to work items in ONE transaction. Optional top-level `run_id` associates every finding with a review/optimise run. Dedup is automatic (a collapse onto an existing live row counts as `skipped`, not an error). Returns { added, skipped, skipped_ids }. Records one coarse event. Advisory: keep batches to <=~500 rows per call.",
        annotations(open_world_hint = false)
    )]
    async fn add_findings(
        &self,
        Parameters(AddFindingsParams { run_id, items }): Parameters<AddFindingsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_findings", "mcp tool invoked");
        // The repo takes BORROWING input structs (`&str`), so pre-compute the
        // owned `Origin`→wire-string conversions into a Vec that OUTLIVES the
        // borrowing `Vec<(&str, NewFinding)>` built below (each element's
        // `origin: Option<&str>` borrows `&origin_strs[i]`).
        let origin_strs: Vec<Option<String>> =
            items.iter().map(|i| i.origin.map(enum_to_str)).collect();
        let borrowed: Vec<(&str, NewFinding)> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                (
                    item.work_item_id.as_str(),
                    NewFinding {
                        kind: item.kind.as_deref(),
                        severity: item.severity,
                        effort: item.effort.as_deref(),
                        category: item.category.as_deref(),
                        file: item.file.as_deref(),
                        line: item.line,
                        symbol: item.symbol.as_deref(),
                        summary: item.summary.as_deref(),
                        description: item.description.as_deref(),
                        origin: origin_strs[i].as_deref(),
                        confidence: item.confidence.as_deref(),
                        repo_id: item.repo_id.as_deref(),
                        ..NewFinding::default()
                    },
                )
            })
            .collect();
        let result = repo::add_findings(&self.pool, run_id.as_deref(), &borrowed)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::to_value(result).unwrap_or_default())
    }

    /// Bulk non-terminal triage update over many findings under ONE transaction
    /// (single repo call → `repo::batch_update_findings`). All-or-nothing: a
    /// missing finding id (`NotFound`) or a terminal-disposition `status`
    /// (`Validation`) aborts the whole batch. Returns `{ updated }`.
    #[tool(
        description = "Bulk-update finding triage (triage_state/severity/category/non-terminal status) in ONE transaction (all-or-nothing). A terminal disposition (fixed/wontfix/verified_clean/deferred/duplicate) is rejected — use resolve_finding for those. A missing finding id aborts the batch. Returns { updated }. Records one coarse event. Advisory: keep batches to <=~500 rows per call.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn batch_update_findings(
        &self,
        Parameters(BatchUpdateFindingsParams { updates }): Parameters<BatchUpdateFindingsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "batch_update_findings", "mcp tool invoked");
        // The repo takes BORROWING `FindingTriageUpdate<&str>` structs, so build
        // the borrowing Vec off the owned `updates` (which outlives the call).
        let borrowed: Vec<repo::FindingTriageUpdate> = updates
            .iter()
            .map(|u| repo::FindingTriageUpdate {
                finding_id: u.finding_id.as_str(),
                triage_state: u.triage_state.as_deref(),
                severity: u.severity,
                category: u.category.as_deref(),
                status: u.status.as_deref(),
            })
            .collect();
        let count = repo::batch_update_findings(&self.pool, &borrowed)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "updated": count }))
    }

    /// Query LIVE findings with a static NULL-guard filter, optionally returning
    /// grouped axis counts instead of full rows (single repo call →
    /// `repo::query_findings`). Reuses `lumina_core::domain::QueryFindingsFilter`
    /// directly as the param type (it derives `Deserialize + JsonSchema`).
    /// Read-only.
    #[tool(
        description = "Query LIVE findings with a static NULL-guard filter. Each optional field (work_item_id/run_id/severity/category/status/triage_state) constrains its column; an ABSENT field is unconstrained, so one prepared statement covers every filter combination. Only live (non-superseded) findings are returned. With `count_by = \"severity\"` the result switches to grouped mode, returning {\"counts\":[{key,count}]} (one bucket per severity; NULL severities fold into a `(none)` bucket) instead of {\"findings\":[...]}. Read-only. Advisory: an unfiltered query can return a large set — prefer narrowing the filter (e.g. by work_item_id or run_id), or use count_by to aggregate.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn query_findings(
        &self,
        Parameters(filter): Parameters<lumina_core::domain::QueryFindingsFilter>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "query_findings", "mcp tool invoked");
        let result = repo::query_findings(&self.pool, &filter)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::to_value(result).unwrap_or_default())
    }

    /// Query LIVE research notes across work items with a static NULL-guard
    /// filter over `work_item_id` + the two anchor predicates (single repo call
    /// → `repo::query_research_notes`; mirrors `query_findings`). Read-only.
    #[tool(
        description = "Query LIVE (non-superseded) research notes across work items with a static NULL-guard filter. Each optional field constrains a predicate; an ABSENT field is unconstrained, so one prepared statement covers every combination. `work_item_id` scopes to one item's notes. `file` matches every note whose `anchors` cite that file (the anchor equals the path, or is the `path:line` form for that file) — the \"what did we research about this file\" lookup. `anchor` is an exact full-anchor-string match (a specific `path:line` cite or a specific http(s) URL). Returns the matching notes newest-first as a JSON array. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn query_research_notes(
        &self,
        Parameters(QueryResearchNotesParams { work_item_id, file, anchor }): Parameters<
            QueryResearchNotesParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "query_research_notes", "mcp tool invoked");
        let filter = repo::QueryResearchNotesFilter {
            work_item_id: work_item_id.as_deref(),
            file: file.as_deref(),
            anchor: anchor.as_deref(),
        };
        let rows = repo::query_research_notes(&self.pool, &filter)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::to_value(rows).unwrap_or_default())
    }

    /// Compose a story's review/optimise finding queue (single repo call →
    /// `repo::get_story_finding_queue`). Read-only.
    #[tool(
        description = "Compose a story's finding queue: every LIVE (non-superseded) finding attached to the story itself OR one of its DIRECT task children, ordered newest-flagged first. Findings on tombstoned (soft-deleted) work-items are excluded. Returns the findings as a JSON array. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_story_finding_queue(
        &self,
        Parameters(GetStoryFindingQueueParams { story_id }): Parameters<GetStoryFindingQueueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_story_finding_queue", "mcp tool invoked");
        let rows = repo::get_story_finding_queue(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::to_value(rows).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::db::connect_in_memory;
    use crate::mcp::test_support::*;

    /// A valid `add_findings` payload deserialises into the params struct; an
    /// out-of-set `severity` on a batch ELEMENT fails to deserialise (the plan's
    /// "invalid enum → invalid_params" acceptance at the deserialise boundary).
    #[tokio::test]
    async fn add_findings_params_deserialise_and_reject_bad_enum() {
        // A legal payload (optional run_id + a single item) deserialises.
        let ok = serde_json::from_value::<AddFindingsParams>(serde_json::json!({
            "run_id": "run-1",
            "items": [{ "work_item_id": "w1", "severity": "major", "summary": "x" }]
        }));
        assert!(ok.is_ok(), "a legal add_findings payload deserialises");

        // A bogus `severity` on the element fails (rmcp → invalid_params).
        let err = serde_json::from_value::<AddFindingsParams>(serde_json::json!({
            "items": [{ "work_item_id": "w1", "severity": "bogus" }]
        }))
        .expect_err("an invalid element severity must fail to deserialize");
        assert!(
            err.to_string().contains("severity") || err.to_string().contains("variant"),
            "deserialization error should concern the severity enum: {err}"
        );
    }

    /// A valid `batch_update_findings` payload deserialises; an out-of-set
    /// `severity` on a batch ELEMENT fails to deserialise.
    #[tokio::test]
    async fn batch_update_findings_params_deserialise_and_reject_bad_enum() {
        let ok = serde_json::from_value::<BatchUpdateFindingsParams>(serde_json::json!({
            "updates": [{ "finding_id": "f1", "severity": "minor", "status": "triaged" }]
        }));
        assert!(ok.is_ok(), "a legal batch_update_findings payload deserialises");

        let err = serde_json::from_value::<BatchUpdateFindingsParams>(serde_json::json!({
            "updates": [{ "finding_id": "f1", "severity": "bogus" }]
        }))
        .expect_err("an invalid element severity must fail to deserialize");
        assert!(
            err.to_string().contains("severity") || err.to_string().contains("variant"),
            "deserialization error should concern the severity enum: {err}"
        );
    }

    /// A legal `query_findings` payload (a couple of filter fields +
    /// `count_by: "severity"`) deserialises into the reused
    /// `lumina_core::domain::QueryFindingsFilter` param type; a bogus `count_by`
    /// value is REJECTED at the deserialise boundary (rmcp → invalid_params).
    #[tokio::test]
    async fn query_findings_params_deserialise_and_reject_bad_enum() {
        // A legal payload: two filter fields + the grouped count axis.
        let ok = serde_json::from_value::<lumina_core::domain::QueryFindingsFilter>(serde_json::json!({
            "work_item_id": "w1",
            "severity": "major",
            "count_by": "severity"
        }));
        assert!(ok.is_ok(), "a legal query_findings payload deserialises");

        // An empty payload is also legal — every field is optional.
        let empty = serde_json::from_value::<lumina_core::domain::QueryFindingsFilter>(serde_json::json!({}));
        assert!(empty.is_ok(), "an empty query_findings payload deserialises");

        // A bogus `count_by` axis fails (the FindingAxis enum has only `severity`).
        let err = serde_json::from_value::<lumina_core::domain::QueryFindingsFilter>(serde_json::json!({
            "count_by": "bogus_axis"
        }))
        .expect_err("an invalid count_by axis must fail to deserialize");
        assert!(
            err.to_string().contains("count_by")
                || err.to_string().contains("variant")
                || err.to_string().contains("severity"),
            "deserialization error should concern the count_by axis enum: {err}"
        );
    }

    /// A `get_story_finding_queue` payload with a `story_id` deserialises.
    #[tokio::test]
    async fn get_story_finding_queue_params_deserialise() {
        let ok = serde_json::from_value::<GetStoryFindingQueueParams>(serde_json::json!({
            "story_id": "s1"
        }));
        assert!(ok.is_ok(), "a legal get_story_finding_queue payload deserialises");
    }

    /// Driving the `add_findings` tool handler against an in-memory pool inserts
    /// N findings under one transaction and returns `{ added: N, skipped: 0 }`.
    #[tokio::test]
    async fn add_findings_tool_inserts_batch_and_reports_added() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;

        // Two distinct findings (different file/symbol so dedup does not collapse
        // them) attached to the story.
        let result = tools
            .add_findings(Parameters(AddFindingsParams {
                run_id: None,
                items: vec![
                    BatchFindingInput {
                        work_item_id: story.clone(),
                        kind: Some("review".to_owned()),
                        severity: Some(Severity::Major),
                        effort: None,
                        category: None,
                        file: Some("src/a.rs".to_owned()),
                        line: Some(1),
                        symbol: Some("foo".to_owned()),
                        summary: Some("finding one".to_owned()),
                        description: None,
                        confidence: None,
                        origin: Some(Origin::Review),
                        repo_id: None,
                    },
                    BatchFindingInput {
                        work_item_id: story.clone(),
                        kind: Some("review".to_owned()),
                        severity: Some(Severity::Minor),
                        effort: None,
                        category: None,
                        file: Some("src/b.rs".to_owned()),
                        line: Some(2),
                        symbol: Some("bar".to_owned()),
                        summary: Some("finding two".to_owned()),
                        description: None,
                        confidence: None,
                        origin: None,
                        repo_id: None,
                    },
                ],
            }))
            .await
            .expect("add_findings tool succeeds");
        assert_eq!(result.is_error, Some(false), "tool result is not an error");

        let payload = result.structured_content.expect("structured payload");
        assert_eq!(payload["added"].as_i64(), Some(2), "two findings added");
        assert_eq!(payload["skipped"].as_i64(), Some(0), "none skipped");

        // The rows actually landed on the story.
        let findings_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM findings WHERE work_item_id = ?")
                .bind(&story)
                .fetch_one(pool.sqlite())
                .await
                .expect("count findings");
        assert_eq!(findings_count, 2, "both findings persisted");
    }
}
