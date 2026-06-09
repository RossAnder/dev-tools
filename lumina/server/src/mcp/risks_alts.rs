//! MCP risk-register + rejected-alternative tools (migration 0005, T4 /
//! lumina-story-planning-round-2), carved out of the `mcp` module's combined
//! tool router (structural split; behaviour unchanged).
//!
//! The eight tools (`add_risk`, `update_risk`, `supersede_risk`, `remove_risk`,
//! `add_rejected_alternative`, `update_rejected_alternative`,
//! `supersede_rejected_alternative`, `remove_rejected_alternative`) and their
//! `*Params` structs live here. They register via the `tool_router_risks_alts`
//! sub-router, summed into the combined field by `LuminaTools::with_state`.

use super::*;

use crate::domain::{AlternativePatch, RiskPatch, RiskSeverity};

// ---- Risk-register params (migration 0005, T4) ---------------------------

/// Arguments for the `add_risk` write tool → `repo::add_risk` (migration 0005).
/// `severity` is the closed [`RiskSeverity`] enum (wire form
/// `low|medium|high|critical`); a bogus value fails deserialisation, surfacing
/// as `invalid_params` before the handler runs.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddRiskParams {
    /// The work-item id the risk attaches to.
    pub work_item_id: String,
    /// A one-line summary of the risk.
    pub summary: String,
    /// Optional long-form body.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale ("why this is a risk").
    #[serde(default)]
    pub rationale: Option<String>,
    /// The risk severity; one of `low`/`medium`/`high`/`critical`.
    pub severity: RiskSeverity,
    /// Optional mitigation strategy.
    #[serde(default)]
    pub mitigation: Option<String>,
}

/// Arguments for the `update_risk` write tool → `repo::update_risk`. Carries
/// the target `id` plus the optional mutable fields (mirrors [`RiskPatch`],
/// which lacks `id`). The MCP layer reshapes to a `RiskPatch` before the call.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateRiskParams {
    /// The risk id to update.
    pub id: String,
    /// New summary; absent leaves the existing summary unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New body; absent leaves the existing body unchanged.
    #[serde(default)]
    pub body: Option<String>,
    /// New rationale; absent leaves the existing rationale unchanged.
    #[serde(default)]
    pub rationale: Option<String>,
    /// New severity; absent leaves the existing severity unchanged.
    #[serde(default)]
    pub severity: Option<RiskSeverity>,
    /// New mitigation strategy; absent leaves the existing mitigation unchanged.
    #[serde(default)]
    pub mitigation: Option<String>,
}

/// Arguments for the `supersede_risk` write tool → `repo::supersede_risk`. The
/// old risk's `superseded_by` is set to the new risk's id, and the new risk
/// is appended under the same work item — both in ONE transaction, ONE event
/// `risk.superseded`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SupersedeRiskParams {
    /// The work-item id the risk attaches to (must match the old risk's owner).
    pub work_item_id: String,
    /// The superseded (old) risk id.
    pub old_id: String,
    /// A one-line summary of the new risk.
    pub summary: String,
    /// Optional long-form body for the new risk.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale for the new risk.
    #[serde(default)]
    pub rationale: Option<String>,
    /// The new risk's severity; one of `low`/`medium`/`high`/`critical`.
    pub severity: RiskSeverity,
    /// Optional mitigation strategy for the new risk.
    #[serde(default)]
    pub mitigation: Option<String>,
}

/// Arguments for the (DESTRUCTIVE) `remove_risk` write tool →
/// `repo::remove_risk` (a hard delete — risks have no independent export
/// identity; they fold into the owning work-item's TOML).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveRiskParams {
    /// The risk id to hard-delete.
    pub id: String,
}

// ---- Rejected-alternative params (migration 0005, T4) --------------------

/// Arguments for the `add_rejected_alternative` write tool →
/// `repo::add_rejected_alternative`. Mirrors [`AddRiskParams`] minus severity;
/// `confidence` is free TEXT (validated nowhere at the DB, matching
/// `research_notes.confidence`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddRejectedAlternativeParams {
    /// The work-item id the rejected alternative attaches to.
    pub work_item_id: String,
    /// A one-line summary of the rejected alternative.
    pub summary: String,
    /// Optional long-form body.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale ("why this was rejected").
    #[serde(default)]
    pub rationale: Option<String>,
    /// Optional evidence grade (`high|medium|low`).
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Arguments for the `update_rejected_alternative` write tool →
/// `repo::update_rejected_alternative` (mirrors [`AlternativePatch`] + `id`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateRejectedAlternativeParams {
    /// The rejected-alternative id to update.
    pub id: String,
    /// New summary; absent leaves the existing summary unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New body; absent leaves the existing body unchanged.
    #[serde(default)]
    pub body: Option<String>,
    /// New rationale; absent leaves the existing rationale unchanged.
    #[serde(default)]
    pub rationale: Option<String>,
    /// New evidence grade (`high|medium|low`); absent leaves it unchanged.
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Arguments for the `supersede_rejected_alternative` write tool →
/// `repo::supersede_rejected_alternative`. Mirrors [`SupersedeRiskParams`]
/// minus severity.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SupersedeRejectedAlternativeParams {
    /// The work-item id the alternative attaches to (must match the old row's owner).
    pub work_item_id: String,
    /// The superseded (old) rejected-alternative id.
    pub old_id: String,
    /// A one-line summary of the new rejected alternative.
    pub summary: String,
    /// Optional long-form body for the new row.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale for the new row.
    #[serde(default)]
    pub rationale: Option<String>,
    /// Optional evidence grade (`high|medium|low`) for the new row.
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Arguments for the (DESTRUCTIVE) `remove_rejected_alternative` write tool →
/// `repo::remove_rejected_alternative` (a hard delete).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveRejectedAlternativeParams {
    /// The rejected-alternative id to hard-delete.
    pub id: String,
}

#[tool_router(router = tool_router_risks_alts, vis = "pub(crate)")]
impl LuminaTools {
    // ---- Risk-register tools (migration 0005, T4) -----------------------

    /// Add a risk to a work item (single repo call → `repo::add_risk`). The
    /// `severity` is the closed [`RiskSeverity`] enum, rendered to the wire
    /// form (`low|medium|high|critical`) before the call.
    #[tool(
        description = "Add a risk (summary/body/rationale/severity/mitigation) to a work item. Severity is one of low/medium/high/critical. Records one event (risk.added).",
        annotations(open_world_hint = false)
    )]
    async fn add_risk(
        &self,
        Parameters(p): Parameters<AddRiskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_risk", "mcp tool invoked");
        let severity_str = enum_to_str(p.severity);
        let id = repo::add_risk(
            &self.pool,
            &p.work_item_id,
            &p.summary,
            p.body.as_deref(),
            p.rationale.as_deref(),
            &severity_str,
            p.mitigation.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a risk (single repo call →
    /// `repo::update_risk`).
    #[tool(
        description = "Partially update a risk by id (summary/body/rationale/severity/mitigation; absent fields unchanged). Records one event (risk.updated).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_risk(
        &self,
        Parameters(p): Parameters<UpdateRiskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "update_risk", "mcp tool invoked");
        let patch = RiskPatch {
            summary: p.summary,
            body: p.body,
            rationale: p.rationale,
            severity: p.severity,
            mitigation: p.mitigation,
        };
        repo::update_risk(&self.pool, &p.id, &patch)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Supersede a risk with a new one (single repo call →
    /// `repo::supersede_risk`). The old row's `superseded_by` is set to the
    /// new row's id; both writes ride ONE transaction and ONE event
    /// (`risk.superseded`).
    #[tool(
        description = "Supersede an old risk with a new one under the same work item (sets the old row's superseded_by; appends the new row). One transaction, one event (risk.superseded).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn supersede_risk(
        &self,
        Parameters(p): Parameters<SupersedeRiskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "supersede_risk", "mcp tool invoked");
        let severity_str = enum_to_str(p.severity);
        let new_id = repo::supersede_risk(
            &self.pool,
            &p.work_item_id,
            &p.old_id,
            &p.summary,
            p.body.as_deref(),
            p.rationale.as_deref(),
            &severity_str,
            p.mitigation.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(
            serde_json::json!({ "old_id": p.old_id, "new_id": new_id.to_string() }),
        )
    }

    /// HARD-delete a risk (single repo call → `repo::remove_risk`). Risks
    /// have no independent export identity; the export fold drops them from
    /// the owning work-item's TOML.
    #[tool(
        description = "Remove (hard-delete) a risk by id. Records one event (risk.removed).",
        annotations(destructive_hint = true, open_world_hint = false)
    )]
    async fn remove_risk(
        &self,
        Parameters(RemoveRiskParams { id }): Parameters<RemoveRiskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "remove_risk", "mcp tool invoked");
        repo::remove_risk(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "removed": true }))
    }

    // ---- Rejected-alternative tools (migration 0005, T4) ----------------

    /// Add a rejected planning alternative to a work item (single repo call →
    /// `repo::add_rejected_alternative`).
    #[tool(
        description = "Add a rejected planning alternative (summary/body/rationale/confidence) to a work item. Records one event (rejected_alternative.added).",
        annotations(open_world_hint = false)
    )]
    async fn add_rejected_alternative(
        &self,
        Parameters(p): Parameters<AddRejectedAlternativeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_rejected_alternative", "mcp tool invoked");
        let id = repo::add_rejected_alternative(
            &self.pool,
            &p.work_item_id,
            &p.summary,
            p.body.as_deref(),
            p.rationale.as_deref(),
            p.confidence.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a rejected alternative (single repo call
    /// → `repo::update_rejected_alternative`).
    #[tool(
        description = "Partially update a rejected planning alternative by id (summary/body/rationale/confidence; absent fields unchanged). Records one event (rejected_alternative.updated).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_rejected_alternative(
        &self,
        Parameters(p): Parameters<UpdateRejectedAlternativeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "update_rejected_alternative", "mcp tool invoked");
        let patch = AlternativePatch {
            summary: p.summary,
            body: p.body,
            rationale: p.rationale,
            confidence: p.confidence,
        };
        repo::update_rejected_alternative(&self.pool, &p.id, &patch)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Supersede a rejected alternative with a new one (single repo call →
    /// `repo::supersede_rejected_alternative`).
    #[tool(
        description = "Supersede an old rejected planning alternative with a new one (sets the old row's superseded_by; appends the new row). One transaction, one event (rejected_alternative.superseded).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn supersede_rejected_alternative(
        &self,
        Parameters(p): Parameters<SupersedeRejectedAlternativeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "supersede_rejected_alternative", "mcp tool invoked");
        let new_id = repo::supersede_rejected_alternative(
            &self.pool,
            &p.work_item_id,
            &p.old_id,
            &p.summary,
            p.body.as_deref(),
            p.rationale.as_deref(),
            p.confidence.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(
            serde_json::json!({ "old_id": p.old_id, "new_id": new_id.to_string() }),
        )
    }

    /// HARD-delete a rejected alternative (single repo call →
    /// `repo::remove_rejected_alternative`).
    #[tool(
        description = "Remove (hard-delete) a rejected planning alternative by id. Records one event (rejected_alternative.removed).",
        annotations(destructive_hint = true, open_world_hint = false)
    )]
    async fn remove_rejected_alternative(
        &self,
        Parameters(RemoveRejectedAlternativeParams { id }): Parameters<
            RemoveRejectedAlternativeParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "remove_rejected_alternative", "mcp tool invoked");
        repo::remove_rejected_alternative(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "removed": true }))
    }
}
