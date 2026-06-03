//! TOML input shapes for the flow importer (permissive: heterogeneous-per-type
//! union). These are private deserialise DTOs the importer reads into; they are
//! `pub(crate)` (with `pub(crate)` fields) only so the parent `import` module's
//! `import_flow`/`ensure_scaffold` can read their fields.

use serde::Deserialize;

/// `context.toml` envelope — only the fields the importer needs.
#[derive(Debug, Deserialize)]
pub(crate) struct ContextDoc {
    pub(crate) slug: String,
    #[serde(default)]
    pub(crate) plan_path: Option<String>,
    #[serde(default)]
    pub(crate) scope: Vec<String>,
    #[serde(default)]
    pub(crate) artifacts: Artifacts,
}

/// `[artifacts]` block. Every path is optional — a flow may declare only some.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct Artifacts {
    #[serde(default)]
    pub(crate) execution_record: Option<String>,
    #[serde(default)]
    pub(crate) review_ledger: Option<String>,
    #[serde(default)]
    pub(crate) optimise_findings: Option<String>,
}

/// `execution-record.toml` file envelope.
#[derive(Debug, Deserialize)]
pub(crate) struct ExecutionRecord {
    #[serde(default)]
    pub(crate) items: Vec<ExecItem>,
}

/// A single execution-record `[[items]]` entry. The vocabulary is heterogeneous
/// per `type`, so every per-type field is `Option`/`default`; we read only the
/// `task-completion` shape (`task_ref`/`status`/`files`) plus the always-present
/// `type`. Unknown extra fields are tolerated by serde (no `deny_unknown_fields`).
#[derive(Debug, Deserialize)]
pub(crate) struct ExecItem {
    #[serde(rename = "type")]
    pub(crate) item_type: String,
    #[serde(default)]
    pub(crate) task_ref: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) files: Vec<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
}

/// A ledger file envelope (`review-ledger.toml` / `optimise-findings.toml`).
#[derive(Debug, Deserialize)]
pub(crate) struct Ledger {
    #[serde(default)]
    pub(crate) items: Vec<LedgerItem>,
}

/// A single ledger `[[items]]` finding. All fields optional (heterogeneous
/// disposition shapes). Date-typed columns (`first_flagged`, `resolved_at`) are
/// read as `toml::value::Datetime` so TOML local-date literals parse, then
/// stringified for the TEXT columns.
#[derive(Debug, Deserialize)]
pub(crate) struct LedgerItem {
    #[serde(default)]
    pub(crate) severity: Option<String>,
    #[serde(default)]
    pub(crate) effort: Option<String>,
    #[serde(default)]
    pub(crate) category: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) file: Option<String>,
    #[serde(default)]
    pub(crate) line: Option<i64>,
    #[serde(default)]
    pub(crate) symbol: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) first_flagged: Option<toml::value::Datetime>,
    #[serde(default)]
    pub(crate) rounds: Option<i64>,
    #[serde(default)]
    pub(crate) fingerprint: Option<String>,
    #[serde(default)]
    pub(crate) flow: Option<String>,
    #[serde(default)]
    pub(crate) dedup_id: Option<String>,
    // Disposition fields (P7) — carried so deferred/wontfix imports aren't lossy.
    // The ledger uses `resolved` (a date) in some schemas and `resolved_at` in
    // others; accept both, preferring `resolved_at`.
    #[serde(default)]
    pub(crate) resolved_at: Option<toml::value::Datetime>,
    #[serde(default)]
    pub(crate) resolved: Option<toml::value::Datetime>,
    #[serde(default)]
    pub(crate) resolution: Option<String>,
    #[serde(default)]
    pub(crate) defer_reason: Option<String>,
    #[serde(default)]
    pub(crate) defer_trigger: Option<String>,
    #[serde(default)]
    pub(crate) wontfix_rationale: Option<String>,
}
