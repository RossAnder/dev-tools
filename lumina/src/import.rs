//! Minimal flow importer — ingest one `.claude/flows/<slug>/` into the DB (Task 7).
//!
//! Reads a flow's `context.toml`, resolves its `[artifacts]` paths relative to
//! the flow directory, scaffolds a default `project → epic → feature` chain (if
//! absent), creates a `story` under the feature for the flow, then maps the
//! flow's execution-record and findings into the DB. ALL writes go through the
//! `repo` layer so the events outbox fires and `export` materialises snapshots.
//!
//! ## execution-record `[[items]]` type filter
//!
//! Only `type = "task-completion"` items become `task` work-items. The slice
//! INTENTIONALLY DROPS the following item types (mirroring the plan's
//! Out-of-scope — full flow/finding-type fidelity is a later phase):
//!
//!   * `deviation`
//!   * `verification`
//!   * `status-transition`
//!   * `reconcile`
//!   * `deferral`
//!   * `checkpoint`
//!
//! Any unrecognised `type` is also dropped (forward-compatible: a future record
//! schema can add types without breaking the importer; they simply won't import
//! until the slice is widened).
//!
//! ## Findings (optional, skip-if-absent — P7)
//!
//! `review_ledger` and `optimise_findings` are OPTIONAL inputs: if the resolved
//! path is absent on disk it is skipped silently (not every flow has a review or
//! optimise pass). Each present ledger's `[[items]]` map to `findings` rows
//! attached to the STORY work-item, carrying severity/effort/category/status/
//! summary AND the disposition fields (`defer_reason`/`defer_trigger`/
//! `wontfix_rationale`/`resolution`/`resolved_at`) so deferred/wontfix imports
//! are not lossy.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::domain::Severity;
use crate::repo::{self, NewFinding};

/// Counts returned by [`import_flow`] for the CLI summary line and the test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Work-items created by the default `project → epic → feature → story`
    /// scaffold (0 for each level already present, so a second import reuses
    /// the chain). Always counts the per-flow `story`.
    pub scaffold_created: u32,
    /// `task` work-items created from `task-completion` execution-record items.
    pub tasks_created: u32,
    /// `findings` rows created from the review-ledger / optimise-findings.
    pub findings_created: u32,
    /// execution-record items DROPPED by the type filter (deviation,
    /// verification, status-transition, reconcile, deferral, checkpoint, …).
    pub items_dropped: u32,
}

// --- TOML input shapes (permissive: heterogeneous-per-type union) ----------

/// `context.toml` envelope — only the fields the importer needs.
#[derive(Debug, Deserialize)]
struct ContextDoc {
    slug: String,
    #[serde(default)]
    plan_path: Option<String>,
    #[serde(default)]
    scope: Vec<String>,
    #[serde(default)]
    artifacts: Artifacts,
}

/// `[artifacts]` block. Every path is optional — a flow may declare only some.
#[derive(Debug, Default, Deserialize)]
struct Artifacts {
    #[serde(default)]
    execution_record: Option<String>,
    #[serde(default)]
    review_ledger: Option<String>,
    #[serde(default)]
    optimise_findings: Option<String>,
}

/// `execution-record.toml` file envelope.
#[derive(Debug, Deserialize)]
struct ExecutionRecord {
    #[serde(default)]
    items: Vec<ExecItem>,
}

/// A single execution-record `[[items]]` entry. The vocabulary is heterogeneous
/// per `type`, so every per-type field is `Option`/`default`; we read only the
/// `task-completion` shape (`task_ref`/`status`/`files`) plus the always-present
/// `type`. Unknown extra fields are tolerated by serde (no `deny_unknown_fields`).
#[derive(Debug, Deserialize)]
struct ExecItem {
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    task_ref: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    summary: Option<String>,
}

/// A ledger file envelope (`review-ledger.toml` / `optimise-findings.toml`).
#[derive(Debug, Deserialize)]
struct Ledger {
    #[serde(default)]
    items: Vec<LedgerItem>,
}

/// A single ledger `[[items]]` finding. All fields optional (heterogeneous
/// disposition shapes). Date-typed columns (`first_flagged`, `resolved_at`) are
/// read as `toml::value::Datetime` so TOML local-date literals parse, then
/// stringified for the TEXT columns.
#[derive(Debug, Deserialize)]
struct LedgerItem {
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<i64>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    first_flagged: Option<toml::value::Datetime>,
    #[serde(default)]
    rounds: Option<i64>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    flow: Option<String>,
    #[serde(default)]
    dedup_id: Option<String>,
    // Disposition fields (P7) — carried so deferred/wontfix imports aren't lossy.
    // The ledger uses `resolved` (a date) in some schemas and `resolved_at` in
    // others; accept both, preferring `resolved_at`.
    #[serde(default)]
    resolved_at: Option<toml::value::Datetime>,
    #[serde(default)]
    resolved: Option<toml::value::Datetime>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    defer_reason: Option<String>,
    #[serde(default)]
    defer_trigger: Option<String>,
    #[serde(default)]
    wontfix_rationale: Option<String>,
}

/// The execution-record item types the slice intentionally DROPS (see the
/// module doc). Used only for the in-comment enumeration / clarity; the actual
/// filter keeps `task-completion` and drops everything else.
const DROPPED_ITEM_TYPES: [&str; 6] = [
    "deviation",
    "verification",
    "status-transition",
    "reconcile",
    "deferral",
    "checkpoint",
];

/// Resolve an artifact path declared in `context.toml [artifacts]` against the
/// flow directory. Absolute paths are used verbatim. For relative paths we
/// prefer `flow_dir/<path>` (the fixture's convention); if that does not exist
/// we fall back to the path as-given relative to the CWD (the live-flow
/// convention, where `[artifacts]` paths are repo-root-relative). Returns the
/// first candidate that exists, or the `flow_dir`-joined candidate if neither
/// exists (so the caller's `exists()` skip-if-absent check still works).
fn resolve_artifact(flow_dir: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let under_flow = flow_dir.join(p);
    if under_flow.exists() {
        return under_flow;
    }
    // Live-flow convention: paths are repo-root-relative. Only prefer this if it
    // actually exists; otherwise return the flow-dir candidate so a genuinely
    // absent artifact reports as absent.
    if p.exists() {
        return p.to_path_buf();
    }
    under_flow
}

/// Ensure a single-instance scaffold work-item of `kind` under `parent` exists,
/// returning its id. If a work-item of `kind` already exists under `parent` we
/// reuse it (idempotent re-import); otherwise we create one and bump
/// `scaffold_created`. The default scaffold uses fixed titles so re-imports of
/// different flows share ONE project/epic/feature chain.
async fn ensure_scaffold(
    pool: &SqlitePool,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    summary: &mut ImportSummary,
) -> anyhow::Result<String> {
    let existing = repo::list_work_items(pool, parent_id, Some(kind))
        .await
        .with_context(|| format!("listing existing {kind} work-items"))?;
    if let Some(item) = existing.into_iter().next() {
        return Ok(item.id);
    }
    let id = repo::create_work_item(pool, kind, parent_id, title, None)
        .await
        .with_context(|| format!("creating scaffold {kind} '{title}'"))?;
    summary.scaffold_created += 1;
    Ok(id.to_string())
}

/// Import one flow directory into the DB. See the module doc for the type filter
/// and the skip-if-absent findings contract.
pub async fn import_flow(pool: &SqlitePool, flow_dir: &Path) -> anyhow::Result<ImportSummary> {
    let mut summary = ImportSummary::default();

    // --- 1. Parse context.toml ---------------------------------------------
    let ctx_path = flow_dir.join("context.toml");
    let ctx_raw = std::fs::read_to_string(&ctx_path)
        .with_context(|| format!("reading {}", ctx_path.display()))?;
    let ctx: ContextDoc = toml::from_str(&ctx_raw)
        .with_context(|| format!("parsing {}", ctx_path.display()))?;

    // --- 2. Default scaffold: project → epic → feature ---------------------
    // Fixed titles so multiple flows share one chain; idempotent on re-import.
    let project_id = ensure_scaffold(pool, "project", None, "lumina", &mut summary).await?;
    let epic_id =
        ensure_scaffold(pool, "epic", Some(&project_id), "Imported flows", &mut summary).await?;
    let feature_id =
        ensure_scaffold(pool, "feature", Some(&epic_id), "Flow imports", &mut summary).await?;

    // --- 3. The story for THIS flow ----------------------------------------
    // Title = slug; body = a serialised summary (plan_path + scope) so the
    // detail panel / export can read back the flow envelope.
    let story_body = serde_json::json!({
        "slug": ctx.slug,
        "plan_path": ctx.plan_path,
        "scope": ctx.scope,
    })
    .to_string();
    let story_id = repo::create_work_item(
        pool,
        "story",
        Some(&feature_id),
        &ctx.slug,
        Some(&story_body),
    )
    .await
    .with_context(|| format!("creating story for flow '{}'", ctx.slug))?
    .to_string();
    summary.scaffold_created += 1;

    // --- 4. execution-record [[items]]: keep task-completion, drop the rest -
    if let Some(rel) = ctx.artifacts.execution_record.as_deref() {
        let er_path = resolve_artifact(flow_dir, rel);
        if er_path.exists() {
            let er_raw = std::fs::read_to_string(&er_path)
                .with_context(|| format!("reading {}", er_path.display()))?;
            let record: ExecutionRecord = toml::from_str(&er_raw)
                .with_context(|| format!("parsing {}", er_path.display()))?;
            for item in record.items {
                if item.item_type == "task-completion" {
                    // task title = task_ref (fall back to summary, then a
                    // placeholder); status mapped verbatim; body carries the
                    // `files` array so the field-level acceptance reads it back.
                    let title = item
                        .task_ref
                        .clone()
                        .or_else(|| item.summary.clone())
                        .unwrap_or_else(|| "task".to_string());
                    let body = serde_json::json!({
                        "task_ref": item.task_ref,
                        "files": item.files,
                        "summary": item.summary,
                    })
                    .to_string();
                    let task_id =
                        repo::create_work_item(pool, "task", Some(&story_id), &title, Some(&body))
                            .await
                            .with_context(|| format!("creating task '{title}'"))?;
                    // Map the source status through verbatim (free-text status,
                    // P14). create_work_item seeds "open"; override if present.
                    if let Some(status) = item.status.as_deref() {
                        repo::update_work_item_status(pool, &task_id.to_string(), status)
                            .await
                            .with_context(|| format!("setting status for task '{title}'"))?;
                    }
                    summary.tasks_created += 1;
                } else {
                    // Dropped: deviation / verification / status-transition /
                    // reconcile / deferral / checkpoint / any unknown type.
                    let _ = &DROPPED_ITEM_TYPES; // see module doc enumeration
                    summary.items_dropped += 1;
                }
            }
        }
    }

    // --- 5. Findings (optional, skip-if-absent — P7) -----------------------
    for rel in [
        ctx.artifacts.review_ledger.as_deref(),
        ctx.artifacts.optimise_findings.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let ledger_path = resolve_artifact(flow_dir, rel);
        if !ledger_path.exists() {
            continue; // skip silently — not every flow has this ledger
        }
        let raw = std::fs::read_to_string(&ledger_path)
            .with_context(|| format!("reading {}", ledger_path.display()))?;
        let ledger: Ledger = toml::from_str(&raw)
            .with_context(|| format!("parsing {}", ledger_path.display()))?;
        for fi in ledger.items {
            // Stringify the date-typed columns for the TEXT schema. Prefer the
            // explicit `resolved_at`, fall back to the legacy `resolved`.
            let first_flagged = fi.first_flagged.map(|d| d.to_string());
            let resolved_at = fi
                .resolved_at
                .or(fi.resolved)
                .map(|d| d.to_string());
            // Parse the imported severity (free TEXT in the on-disk ledger)
            // into the typed `Severity` enum. The tomlctl-flow ledger format
            // uses its own severity vocab (`critical|warning|suggestion` —
            // see the round-3 plan's "Canonical lumina vocabulary"
            // cross-system note); lumina's `Severity` is
            // `critical|major|minor|suggestion`. We bridge the two at this
            // single import boundary: tomlctl-flow's `warning` maps to
            // lumina's `Major` (the closest equivalent in lumina's four-tier
            // ladder). Other tomlctl-flow names round-trip identically.
            // A genuinely unknown value (not in either vocab) errors out so
            // stale ledgers surface rather than silently degrade.
            // CONVENTIONS §k.2 documents the deliberate severity split.
            let severity_typed: Option<Severity> = match fi.severity.as_deref() {
                None => None,
                Some(s) => {
                    let canonical = match s {
                        "warning" => "major", // tomlctl-flow → lumina mapping
                        other => other,
                    };
                    Some(
                        serde_json::from_value::<Severity>(serde_json::Value::String(
                            canonical.to_owned(),
                        ))
                        .with_context(|| {
                            format!(
                                "invalid finding severity '{s}' (expected one of \
                                 critical|major|minor|suggestion — or the \
                                 tomlctl-flow alias `warning` — see \
                                 CONVENTIONS §k.2)"
                            )
                        })?,
                    )
                }
            };
            let new_finding = NewFinding {
                kind: None,
                severity: severity_typed,
                effort: fi.effort.as_deref(),
                category: fi.category.as_deref(),
                status: fi.status.as_deref(),
                file: fi.file.as_deref(),
                line: fi.line,
                symbol: fi.symbol.as_deref(),
                summary: fi.summary.as_deref(),
                description: fi.description.as_deref(),
                first_flagged: first_flagged.as_deref(),
                rounds: fi.rounds,
                fingerprint: fi.fingerprint.as_deref(),
                flow: fi.flow.as_deref(),
                dedup_id: fi.dedup_id.as_deref(),
                origin: None,
                confidence: None,
                resolved_at: resolved_at.as_deref(),
                resolution: fi.resolution.as_deref(),
                defer_reason: fi.defer_reason.as_deref(),
                defer_trigger: fi.defer_trigger.as_deref(),
                wontfix_rationale: fi.wontfix_rationale.as_deref(),
                repo_id: None,
            };
            repo::create_finding(pool, &story_id, &new_finding)
                .await
                .context("creating finding")?;
            summary.findings_created += 1;
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;

    /// Absolute path to the committed fixture flow dir (P11).
    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("flow-sample")
    }

    /// Walk parent→child by `kind` from the (single) project root, returning the
    /// id at each level. Asserts exactly one item exists at each scaffold level.
    async fn resolve_chain(pool: &SqlitePool) -> (String, String, String, String) {
        let projects = repo::list_work_items(pool, None, Some("project")).await.unwrap();
        assert_eq!(projects.len(), 1, "exactly one project");
        let project = projects[0].id.clone();
        let epics = repo::list_work_items(pool, Some(&project), Some("epic")).await.unwrap();
        assert_eq!(epics.len(), 1, "exactly one epic");
        let epic = epics[0].id.clone();
        let features = repo::list_work_items(pool, Some(&epic), Some("feature")).await.unwrap();
        assert_eq!(features.len(), 1, "exactly one feature");
        let feature = features[0].id.clone();
        let stories = repo::list_work_items(pool, Some(&feature), Some("story")).await.unwrap();
        assert_eq!(stories.len(), 1, "exactly one story");
        let story = stories[0].id.clone();
        (project, epic, feature, story)
    }

    /// Full acceptance: import the committed fixture and assert (a) the 5-level
    /// chain, (b) dropped types produced NO tasks, (c) findings count, and the
    /// (d) field-level round-trips for a finding and a task.
    #[tokio::test]
    async fn import_fixture_populates_chain_and_findings() {
        let pool = connect_in_memory().await.expect("pool");

        let summary = import_flow(&pool, &fixture_dir())
            .await
            .expect("import succeeds");

        // (a) 5-level chain project→epic→feature→story exists and is unique.
        let (_p, _e, _f, story) = resolve_chain(&pool).await;

        // (b) The fixture has 2 task-completion items + 1 deviation + 1
        // verification + 1 status-transition = 5 items, but only the 2
        // task-completion items become tasks; the other 3 are DROPPED.
        assert_eq!(summary.tasks_created, 2, "two tasks from two task-completion items");
        assert_eq!(summary.items_dropped, 3, "three non-task-completion items dropped");
        let tasks = repo::list_work_items(&pool, Some(&story), Some("task")).await.unwrap();
        assert_eq!(tasks.len(), 2, "exactly two task work-items under the story");
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert!(titles.contains(&"scaffold-the-sample-crate"), "task titles: {titles:?}");
        assert!(titles.contains(&"add-the-storage-layer"), "task titles: {titles:?}");

        // (c) findings count matches the fixture review-ledger (2). The
        // optimise-findings path is absent → skipped silently (P7).
        assert_eq!(summary.findings_created, 2, "two findings from the review-ledger");
        let findings = repo::list_findings(&pool, &story).await.unwrap();
        assert_eq!(findings.len(), 2, "two findings attached to the story");

        // (d) field-level: the deferred finding round-trips its disposition.
        let deferred = findings
            .iter()
            .find(|f| f.status.as_deref() == Some("deferred"))
            .expect("a deferred finding exists");
        assert_eq!(deferred.severity.as_deref(), Some("suggestion"));
        assert_eq!(deferred.category.as_deref(), Some("architecture"));
        assert_eq!(
            deferred.defer_reason.as_deref(),
            Some("The per-request pool is cheap at current scale and refactoring touches every handler")
        );
        assert_eq!(
            deferred.defer_trigger.as_deref(),
            Some("Revisit when request volume exceeds the single-pool comfort threshold")
        );
        assert!(deferred.wontfix_rationale.is_none(), "deferred carries no wontfix rationale");

        // (d) field-level: a task's status + files round-trip. The body carries
        // the `files` array as JSON; assert it parses back to the fixture set.
        let scaffold_task = tasks
            .iter()
            .find(|t| t.title == "scaffold-the-sample-crate")
            .expect("scaffold task exists");
        assert_eq!(scaffold_task.status, "done", "status mapped verbatim");
        let body: serde_json::Value =
            serde_json::from_str(scaffold_task.body.as_deref().unwrap()).unwrap();
        let files: Vec<String> = serde_json::from_value(body["files"].clone()).unwrap();
        assert_eq!(files, vec!["sample/Cargo.toml", "sample/src/main.rs"]);
    }

    /// Re-importing the same flow reuses the project/epic/feature scaffold (one
    /// chain), proving `ensure_scaffold` idempotence. A second import adds a
    /// second story (flows are distinct stories) but does NOT duplicate the
    /// upper three scaffold levels.
    #[tokio::test]
    async fn reimport_reuses_scaffold_chain() {
        let pool = connect_in_memory().await.expect("pool");

        let first = import_flow(&pool, &fixture_dir()).await.expect("first import");
        // First import creates project+epic+feature+story = 4 scaffold items.
        assert_eq!(first.scaffold_created, 4);

        let second = import_flow(&pool, &fixture_dir()).await.expect("second import");
        // Second import reuses project/epic/feature, creates only a new story.
        assert_eq!(second.scaffold_created, 1, "only a new story on re-import");

        let projects = repo::list_work_items(&pool, None, Some("project")).await.unwrap();
        assert_eq!(projects.len(), 1, "still exactly one project after re-import");
    }
}
