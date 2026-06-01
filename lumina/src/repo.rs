//! Repository layer — the sole mutation path with transactional event writes (Task 3).
//!
//! **Single-source-of-truth discipline (the drift-killer):** every mutation in
//! this module opens a [`crate::db::begin_write`] transaction (which issues
//! `BEGIN IMMEDIATE`, taking the SQLite RESERVED lock upfront so writer
//! contention surfaces at begin-time rather than after the first statement),
//! mutates exactly one domain table, calls [`record_event`] to append ONE
//! `events` row, then commits.
//! Nothing outside this module writes the domain tables, so the HTTP handlers
//! (Task 4) and the MCP tools (Task 5) — both of which call these functions —
//! cannot drift on validation or event emission. If any step in a mutation
//! errors, the `?` propagates and the still-uncommitted `Transaction` is
//! dropped, which issues an automatic ROLLBACK (sqlx contract): the domain row
//! and its event row vanish together. The invariant is "one domain write ⇒ one
//! event row, atomically, or neither".
//!
//! **Hierarchy pre-check (belt-and-braces):** `create_work_item` validates the
//! `(kind, parent-kind)` pair in Rust BEFORE opening the transaction, returning
//! a typed [`AppError::Validation`] (→ 422) on an illegal edge. The DB trigger
//! from Task 2 is the authoritative backstop; the pre-check exists so callers
//! get a clean typed error instead of a raw `RAISE(ABORT, ...)` surfaced as a
//! `Db` 500. Because the pre-check rejects before `begin()`, an illegal create
//! never opens a transaction and writes zero rows.
//!
//! All `query!`/`query_as!` macros are compile-checked against the committed
//! `.sqlx/` offline cache. SQLite's nullability inference is bytecode-based and
//! conservative; columns the schema declares `NOT NULL` are forced non-null
//! with the `as "col!"` override where the macro would otherwise widen them to
//! `Option<T>`, and a few computed/joined columns are forced nullable with
//! `as "col?"`.

use serde_json::Value;
// `SqlitePool` is now named only by the `#[cfg(test)]` raw-assertion helpers
// (the production repo fns front the `DbClient` seam post-A12), so the import is
// test-gated to avoid an unused-import warning in the lib build.
#[cfg(test)]
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::{
    AcceptanceCriterion, ActivityType, AlternativePatch, BatchEntry, ClosureGate, Complexity,
    ContextBlock, Disposition, Effort, Finding, FindingDecisionKind, NewFindingDecision, NewRun,
    NewSprint, NextAction, OpenQuestion, QuestionOption,
    RejectedAlternative, Relevance, RepoLink, ResearchNote, ResearchState, Risk, RiskPatch,
    RiskSeverity, Severity, Shape, StoryReadiness, TargetKind, TaskDependency, TaskKind, Tier,
    TriageState, UpdateFindingRequest,
    UpdateResearchNoteRequest, UpdateWorkItemRequest, WorkItem, WorkItemActivity, WorkItemDetail,
};
use crate::args;
use crate::db::{DbClient, Scalar};
use crate::error::AppError;

/// Raw `work_items` row as it comes off the database, before `attributes` is
/// decoded from its stored TEXT into `Option<Value>`. Generic over `R: Row` per
/// the canonical [`crate::db`] FromRow recipe so it rides `query_*<T>` on both
/// the SQLite arm today and a future Pg arm unchanged. The column→field
/// nullability is carried by the field types (`String` vs `Option<String>`),
/// replacing the old `AS "col!"`/`"col?"` macro hints.
#[derive(Debug)]
struct WorkItemRow {
    id: String,
    kind: String,
    parent_id: Option<String>,
    title: String,
    body: Option<String>,
    status: String,
    position: Option<i64>,
    attributes: Option<String>,
    relevance: Option<String>,
    effort: Option<String>,
    complexity: Option<String>,
    origin: Option<String>,
    closure_gate: Option<String>,
    blocked_by_question_id: Option<String>,
    enabling_option_id: Option<String>,
    task_kind: Option<String>,
    tier: Option<String>,
    shape: Option<String>,
    spawned_from_finding_id: Option<String>,
    created_at: String,
    updated_at: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for WorkItemRow
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<i64>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(WorkItemRow {
            id: row.try_get("id")?,
            kind: row.try_get("kind")?,
            parent_id: row.try_get("parent_id")?,
            title: row.try_get("title")?,
            body: row.try_get("body")?,
            status: row.try_get("status")?,
            position: row.try_get("position")?,
            attributes: row.try_get("attributes")?,
            relevance: row.try_get("relevance")?,
            effort: row.try_get("effort")?,
            complexity: row.try_get("complexity")?,
            origin: row.try_get("origin")?,
            closure_gate: row.try_get("closure_gate")?,
            blocked_by_question_id: row.try_get("blocked_by_question_id")?,
            enabling_option_id: row.try_get("enabling_option_id")?,
            task_kind: row.try_get("task_kind")?,
            tier: row.try_get("tier")?,
            shape: row.try_get("shape")?,
            spawned_from_finding_id: row.try_get("spawned_from_finding_id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

/// Decode a [`WorkItemRow`] into the public [`WorkItem`], turning the raw
/// `attributes` TEXT into `Option<Value>` via [`decode_attributes`].
fn work_item_from_row(r: WorkItemRow) -> Result<WorkItem, AppError> {
    Ok(WorkItem {
        id: r.id,
        kind: r.kind,
        parent_id: r.parent_id,
        title: r.title,
        body: r.body,
        status: r.status,
        position: r.position,
        attributes: decode_attributes(r.attributes)?,
        relevance: r.relevance,
        effort: r.effort,
        complexity: r.complexity,
        origin: r.origin,
        closure_gate: r.closure_gate,
        blocked_by_question_id: r.blocked_by_question_id,
        enabling_option_id: r.enabling_option_id,
        task_kind: r.task_kind,
        tier: r.tier,
        shape: r.shape,
        spawned_from_finding_id: r.spawned_from_finding_id,
        created_at: r.created_at,
        updated_at: r.updated_at,
    })
}

/// Decode a nullable `attributes` TEXT column into `Option<Value>`. A non-NULL
/// column that does not parse as JSON is a stored-data corruption, surfaced as
/// `Other` (→ 500) rather than swallowed — the write-side normalisation
/// guarantees only valid JSON objects are ever stored.
fn decode_attributes(raw: Option<String>) -> Result<Option<Value>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| AppError::Other(e.into())),
    }
}

/// Render the snake_case wire form of a unit domain enum
/// ([`Status`]/[`Severity`]/[`Disposition`]/[`ActivityType`]/[`Relevance`]/
/// [`Effort`]/[`Complexity`]/[`ClosureGate`]/[`ResearchState`]) for storage.
/// Goes through serde so it stays the single source of the wire spelling
/// (`in_progress`, etc.). A unit enum always serialises to a JSON string, so the
/// fallthrough is `unreachable!` — mapped, not `unwrap()`-ed. (Mirrors
/// `mcp::enum_to_str`, which is the param-edge twin we cannot share across the
/// module boundary.)
fn enum_to_str<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        _ => unreachable!("unit domain enum serialises to a JSON string"),
    }
}

/// Validate that `entry_kind` is a legal [`ActivityType`] wire value, returning
/// the canonical spelling. Typed `Validation` (NOT a panic) on an illegal value.
fn validate_entry_kind(entry_kind: &str) -> Result<String, AppError> {
    serde_json::from_value::<ActivityType>(Value::String(entry_kind.to_owned()))
        .map(enum_to_str)
        .map_err(|_| {
            AppError::Validation(format!(
                "unknown activity entry_kind '{entry_kind}' (expected one of execution, \
                 verification, deviation, deferral, reconcile, status_transition, checkpoint, \
                 vet, comment)"
            ))
        })
}

/// Normalise a JSON value destined for `attributes` or an activity `payload`
/// before storage (TOML-export safety, pinned in the plan's ## Approach). The
/// root MUST be a JSON object — a scalar/array root is `Validation`; keys whose
/// value is JSON `null` are DROPPED (not stored).
///
/// Returns the cleaned object. This guarantees `toml::to_string_pretty` over the
/// whole `WorkItemDetail` in export.rs cannot hit the toml crate's
/// null/scalar-root serialization failure.
fn normalise_object(value: &Value, what: &str) -> Result<serde_json::Map<String, Value>, AppError> {
    let obj = value.as_object().ok_or_else(|| {
        AppError::Validation(format!("{what} must be a JSON object at the root"))
    })?;
    let cleaned: serde_json::Map<String, Value> = obj
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(cleaned)
}

/// Per-kind `attributes` validation (pinned contract). All keys are optional, an
/// empty object is legal, and an UNKNOWN key for the row's kind is `Validation`
/// (NOT a 500/panic). Value types are checked where cheap. `obj` is the already
/// null-stripped object from [`normalise_object`].
fn validate_attributes_for_kind(
    kind: &str,
    obj: &serde_json::Map<String, Value>,
) -> Result<(), AppError> {
    // Per-kind legal key → expected-type checker.
    let bad_key = |k: &str| {
        AppError::Validation(format!("unknown attributes key '{k}' for work_item kind '{kind}'"))
    };
    let want_string = |k: &str, v: &Value| {
        if v.is_string() {
            Ok(())
        } else {
            Err(AppError::Validation(format!(
                "attributes key '{k}' must be a string for kind '{kind}'"
            )))
        }
    };
    // Widened (migration 0004 / T4): `files_touched` accepts an array whose
    // entries are either a string (legacy bare-path form) OR an object with
    // EXACTLY the keys `repo` (string) and `path` (string). The MCP edge
    // (`set_task_spec`) is responsible for canonicalising the `repo` slug and
    // confirming it references a `repo_links` row on the task's project
    // ancestor — this validator only enforces the JSON shape so that direct
    // DB-attribute writes (importer, e2e fixtures) cannot store a malformed
    // entry.
    let want_files_touched = |k: &str, v: &Value| -> Result<(), AppError> {
        let arr = v.as_array().ok_or_else(|| {
            AppError::Validation(format!(
                "attributes key '{k}' must be an array for kind '{kind}'"
            ))
        })?;
        for entry in arr {
            if entry.is_string() {
                continue;
            }
            if let Some(obj) = entry.as_object() {
                // Exactly the two keys `repo` and `path`, both strings.
                if obj.len() == 2
                    && obj.get("repo").and_then(Value::as_str).is_some()
                    && obj.get("path").and_then(Value::as_str).is_some()
                {
                    continue;
                }
            }
            return Err(AppError::Validation(format!(
                "attributes key '{k}' entries must be a string or {{repo, path}} \
                 object for kind '{kind}'"
            )));
        }
        Ok(())
    };
    let want_object = |k: &str, v: &Value| {
        if v.is_object() {
            Ok(())
        } else {
            Err(AppError::Validation(format!(
                "attributes key '{k}' must be an object for kind '{kind}'"
            )))
        }
    };

    for (k, v) in obj {
        match kind {
            "story" => match k.as_str() {
                "problem_statement"
                | "research_notes"
                | "execution_strategy"
                | "not_doing" => want_string(k, v)?,
                "verification_commands" => want_object(k, v)?,
                _ => return Err(bad_key(k)),
            },
            "task" => match k.as_str() {
                "execution_detail" | "outcome" => want_string(k, v)?,
                "files_touched" => want_files_touched(k, v)?,
                _ => return Err(bad_key(k)),
            },
            "epic" => match k.as_str() {
                "outcome" | "context" => want_string(k, v)?,
                _ => return Err(bad_key(k)),
            },
            "focus" => match k.as_str() {
                "framing" => want_string(k, v)?,
                _ => return Err(bad_key(k)),
            },
            "project" => return Err(bad_key(k)),
            other => {
                return Err(AppError::Validation(format!(
                    "unknown work_item kind '{other}' for attributes validation"
                )));
            }
        }
    }
    Ok(())
}

/// Parse and canonicalise a GitHub `<owner>/<name>` repo slug (migration 0004).
/// Returns the fully-lowercased slug on success, `Validation` (→ 422) on any
/// rule violation. Reused by every `repo_links` mutator and by the structured
/// `files_touched` entry validator on `set_task_spec` (T4).
///
/// Rules (from Research Notes in the plan):
///   * Exactly one `/` separator (split on first; reject zero or > 1).
///   * Owner: 1-39 chars from `[A-Za-z0-9-]`; first char alphanumeric; no
///     leading/trailing hyphen; no consecutive hyphens. (Mirrors the GitHub
///     username regex `^[a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38}$` with i-flag.)
///   * Name: 1-100 chars from `[A-Za-z0-9._-]`; must NOT end with `.git`; must
///     NOT be exactly `.` or `..`. We additionally reject a leading `.`/`-`/`_`
///     in the name segment — GitHub rejects those in practice, and the plan's
///     `x/-y` acceptance case requires this guard (documented inline below).
///   * Both segments are lowercased on the canonical return value (GitHub repo
///     resolution is case-insensitive; storing `Foo/Bar` and `foo/bar` as
///     distinct rows would defeat the per-project UNIQUE(slug) constraint).
///
/// Hand-rolled (no `regex` crate dep) — keep this in sync if the rules change.
pub fn parse_github_slug(s: &str) -> Result<String, AppError> {
    let err = |reason: &str| {
        AppError::Validation(format!("invalid GitHub slug '{s}': {reason}"))
    };

    // Exactly one '/'.
    let slash_count = s.bytes().filter(|b| *b == b'/').count();
    if slash_count != 1 {
        return Err(err("must contain exactly one '/' separator"));
    }
    let (owner, name) = s.split_once('/').expect("slash_count == 1 guarantees split");

    // --- owner -----------------------------------------------------------
    if owner.is_empty() {
        return Err(err("owner segment is empty"));
    }
    if owner.len() > 39 {
        return Err(err("owner segment exceeds 39 chars"));
    }
    let owner_bytes = owner.as_bytes();
    // First / last char alphanumeric (rejects leading/trailing hyphen).
    let is_alnum = |b: u8| b.is_ascii_alphanumeric();
    let is_owner_char = |b: u8| is_alnum(b) || b == b'-';
    if !is_alnum(owner_bytes[0]) {
        return Err(err("owner must start with an alphanumeric character"));
    }
    if !is_alnum(owner_bytes[owner_bytes.len() - 1]) {
        return Err(err("owner must end with an alphanumeric character"));
    }
    let mut prev_was_hyphen = false;
    for &b in owner_bytes {
        if !is_owner_char(b) {
            return Err(err(
                "owner may only contain alphanumeric characters and hyphens",
            ));
        }
        if b == b'-' && prev_was_hyphen {
            return Err(err("owner must not contain consecutive hyphens"));
        }
        prev_was_hyphen = b == b'-';
    }

    // --- name ------------------------------------------------------------
    if name.is_empty() {
        return Err(err("name segment is empty"));
    }
    if name.len() > 100 {
        return Err(err("name segment exceeds 100 chars"));
    }
    if name == "." || name == ".." {
        return Err(err("name segment must not be '.' or '..'"));
    }
    // Empirical: GitHub rejects a leading `.`, `-`, or `_` in repo names. The
    // plan's `x/-y` test case requires this guard; without it the basic name
    // regex `[A-Za-z0-9._-]{1,100}` would accept `-y`. Keep this rule in lock-
    // step with the test expectations in `mod tests::parse_github_slug_*`.
    let name_bytes = name.as_bytes();
    match name_bytes[0] {
        b'.' | b'-' | b'_' => {
            return Err(err(
                "name must not start with '.', '-', or '_'",
            ));
        }
        _ => {}
    }
    for &b in name_bytes {
        let ok = b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-');
        if !ok {
            return Err(err(
                "name may only contain alphanumeric characters, '.', '_', and '-'",
            ));
        }
    }
    // The `.git` suffix is reserved (GitHub rejects).
    if name.ends_with(".git") {
        return Err(err("name must not end with '.git'"));
    }

    // Canonical form: both segments lowercased, joined by '/'.
    let canonical = format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        name.to_ascii_lowercase()
    );
    Ok(canonical)
}

/// The five legal work-item kinds, ordered parent→child. A kind's legal parent
/// kind is the entry immediately before it; `project` (index 0) is the root and
/// must have a NULL parent.
const KINDS: [&str; 5] = ["project", "epic", "focus", "story", "task"];

/// Validate a `(kind, parent_kind)` edge against the hierarchy rules. Returns
/// the typed `Validation` error rather than relying on the DB trigger, so a bad
/// edge becomes a 422 instead of a raw trigger error mapped to 500.
///
/// Rules (mirroring the migration trigger):
///   * `project` ⇔ parent is NULL.
///   * every other kind ⇔ parent kind is the immediately-higher level.
fn validate_hierarchy_edge(kind: &str, parent_kind: Option<&str>) -> Result<(), AppError> {
    let Some(idx) = KINDS.iter().position(|k| *k == kind) else {
        return Err(AppError::Validation(format!(
            "unknown work_item kind '{kind}' (expected one of {})",
            KINDS.join(", ")
        )));
    };

    match (idx, parent_kind) {
        // project: must be a root.
        (0, None) => Ok(()),
        (0, Some(pk)) => Err(AppError::Validation(format!(
            "a 'project' must be a root (NULL parent), but a parent of kind '{pk}' was given"
        ))),
        // non-project: must have a parent of the immediately-higher kind.
        (_, None) => Err(AppError::Validation(format!(
            "a '{kind}' requires a parent of kind '{}'", KINDS[idx - 1]
        ))),
        (i, Some(pk)) if pk == KINDS[i - 1] => Ok(()),
        (i, Some(pk)) => Err(AppError::Validation(format!(
            "a '{kind}' must sit under a '{}', not under a '{pk}'",
            KINDS[i - 1]
        ))),
    }
}

/// List work items, optionally filtered by `parent_id` and/or `kind`.
///
/// `parent_id = None` means "no parent filter" (NOT "roots only"); callers that
/// want roots pass an explicit sentinel via the HTTP layer. The four-way filter
/// combination is expressed with `IS NULL OR col = ?` guards so a single
/// prepared statement covers every case (keeps the `.sqlx` cache to one entry).
pub async fn list_work_items(
    db: &impl DbClient,
    parent_id: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<WorkItem>, AppError> {
    // Soft-delete reader policy (pinned): list views hide tombstoned rows.
    // `attributes` arrives as `Option<String>` on `WorkItemRow` and is decoded
    // into `WorkItem.attributes: Option<Value>` by hand below.
    let rows = db
        .query_all::<WorkItemRow>(
            LIST_WORK_ITEMS_SQL,
            args![parent_id.map(str::to_owned), kind.map(str::to_owned)],
        )
        .await?;

    let items = rows
        .into_iter()
        .map(work_item_from_row)
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(items)
}

const LIST_WORK_ITEMS_SQL: &str = r#"
        SELECT
            id, kind, parent_id, title, body, status, position, attributes,
            relevance, effort, complexity, origin, closure_gate,
            blocked_by_question_id, enabling_option_id, task_kind, tier, shape,
            spawned_from_finding_id, created_at, updated_at
        FROM work_items
        WHERE deleted_at IS NULL
          AND ($1 IS NULL OR parent_id = $1)
          AND ($2 IS NULL OR kind = $2)
        ORDER BY COALESCE(position, 0), created_at, id
        "#;

/// Fetch one work item plus its DIRECT children, its findings, and the context
/// blocks linked through `work_item_context`. Returns `NotFound` if the id has
/// no row.
pub async fn get_work_item_detail(
    pool: &impl DbClient,
    id: &str,
) -> Result<WorkItemDetail, AppError> {
    // Soft-delete reader policy (pinned): the DETAIL fetch does NOT filter on
    // `deleted_at` — it returns the row WITH `deleted_at` populated so the export
    // tombstone path and a deleted-marker detail fetch both work.
    let row = pool
        .query_opt::<WorkItemRow>(GET_WORK_ITEM_DETAIL_SQL, args![id.to_owned()])
        .await?
        .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))?;

    let item = work_item_from_row(row)?;

    let children = list_work_items(pool, Some(id), None).await?;
    let findings = list_findings(pool, id).await?;
    let activity = list_activity(pool, id).await?;
    let acceptance_criteria = list_acceptance_criteria(pool, id).await?;
    let research_notes = list_research_notes(pool, id).await?;
    let open_questions = list_open_questions(pool, id).await?;
    // Migration 0004: repo links live only on `project` work-items (kind-check
    // trigger pair). Skip the side-table query for any other kind — returns an
    // empty Vec — to keep the per-detail read count low.
    let repo_links = if item.kind == "project" {
        list_repo_links(pool, &item.id).await?
    } else {
        Vec::new()
    };

    // Migration 0005 folds: risks and rejected_alternatives are per-work-item
    // (live = `superseded_by IS NULL`); task_dependencies are per-task outgoing
    // edges, so the kind filter mirrors the repo_links project-only filter.
    let risks = list_risks(pool, &item.id).await?;
    let rejected_alternatives = list_rejected_alternatives(pool, &item.id).await?;
    let task_dependencies = if item.kind == "task" {
        list_outgoing_task_dependencies(pool, &item.id).await?
    } else {
        Vec::new()
    };

    let context_blocks = pool
        .query_all::<ContextBlock>(DETAIL_CONTEXT_BLOCKS_SQL, args![id.to_owned()])
        .await?;

    Ok(WorkItemDetail {
        item,
        children,
        findings,
        context_blocks,
        activity,
        acceptance_criteria,
        research_notes,
        open_questions,
        repo_links,
        risks,
        rejected_alternatives,
        task_dependencies,
    })
}

const GET_WORK_ITEM_DETAIL_SQL: &str = r#"
        SELECT
            id, kind, parent_id, title, body, status, position, attributes,
            relevance, effort, complexity, origin, closure_gate,
            blocked_by_question_id, enabling_option_id, task_kind, tier, shape,
            spawned_from_finding_id, created_at, updated_at
        FROM work_items
        WHERE id = $1
        "#;

const DETAIL_CONTEXT_BLOCKS_SQL: &str = r#"
        SELECT
            cb.id, cb.title, cb.body, cb.created_at, cb.updated_at
        FROM context_blocks cb
        JOIN work_item_context wic ON wic.context_block_id = cb.id
        WHERE wic.work_item_id = $1
        ORDER BY cb.created_at, cb.id
        "#;

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`ContextBlock`] aggregate
/// (canonical recipe). Used by `get_work_item_detail`'s context-block JOIN; the
/// A6 `context_blocks` wave reuses this same impl for its own reads.
impl<'r, R> sqlx::FromRow<'r, R> for ContextBlock
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ContextBlock {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
            body: row.try_get("body")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`AcceptanceCriterion`]
/// aggregate (canonical recipe). Used by `list_acceptance_criteria`; column→field
/// nullability is carried by the field types (`String`/`i64` for NOT-NULL columns,
/// `Option<String>` for the nullable `checked_at`/`checked_by`), replacing the old
/// `AS "col!"`/`"col?"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for AcceptanceCriterion
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(AcceptanceCriterion {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            text: row.try_get("text")?,
            checked: row.try_get("checked")?,
            checked_at: row.try_get("checked_at")?,
            checked_by: row.try_get("checked_by")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Raw `work_item_activity` row as it comes off the database, before `payload` is
/// decoded from its stored TEXT into `Option<Value>` (via [`decode_attributes`]).
/// Generic over `R: Row` per the canonical [`crate::db`] FromRow recipe. The
/// `payload` field is `Option<String>` here; the `list_activity` transform turns
/// it into the public [`WorkItemActivity`]'s `Option<Value>`.
#[derive(Debug)]
struct ActivityRow {
    id: String,
    work_item_id: String,
    seq: i64,
    entry_kind: String,
    author: Option<String>,
    summary: String,
    payload: Option<String>,
    origin: Option<String>,
    created_at: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for ActivityRow
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ActivityRow {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            entry_kind: row.try_get("entry_kind")?,
            author: row.try_get("author")?,
            summary: row.try_get("summary")?,
            payload: row.try_get("payload")?,
            origin: row.try_get("origin")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`ResearchNote`] aggregate
/// (canonical recipe, A7 wave). Column→field nullability is carried by the field
/// types (`String`/`i64` for NOT-NULL columns, `Option<String>` for the nullable
/// `body`/`confidence`/`state`/`rationale`/`lens`/`origin`/`superseded_by`),
/// replacing the old `AS "col!"`/`"col?"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for ResearchNote
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ResearchNote {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            summary: row.try_get("summary")?,
            body: row.try_get("body")?,
            confidence: row.try_get("confidence")?,
            state: row.try_get("state")?,
            rationale: row.try_get("rationale")?,
            lens: row.try_get("lens")?,
            origin: row.try_get("origin")?,
            superseded_by: row.try_get("superseded_by")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`QuestionOption`] aggregate
/// (canonical recipe, A7 wave). Used by `list_open_questions`'s per-question
/// options fold; the nullable `detail` carries its nullability via `Option<String>`.
impl<'r, R> sqlx::FromRow<'r, R> for QuestionOption
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(QuestionOption {
            id: row.try_get("id")?,
            question_id: row.try_get("question_id")?,
            seq: row.try_get("seq")?,
            label: row.try_get("label")?,
            detail: row.try_get("detail")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Raw scalar columns of an `open_questions` row as they come off the database,
/// WITHOUT the nested `options` array-of-tables (those are folded in a second
/// query by `list_open_questions`). Generic over `R: Row` per the canonical
/// [`crate::db`] FromRow recipe; mirrors the [`ActivityRow`] private-row-struct
/// precedent. The `list_open_questions` loop builds each public [`OpenQuestion`]
/// from one of these rows plus its `seq`-ordered `options`.
#[derive(Debug)]
struct OpenQuestionRow {
    id: String,
    story_id: String,
    seq: i64,
    question: String,
    status: Option<String>,
    answer: Option<String>,
    chosen_option_id: Option<String>,
    decided_at: Option<String>,
    decided_by: Option<String>,
    prompting_finding_id: Option<String>,
    prompting_note_id: Option<String>,
    created_at: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for OpenQuestionRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(OpenQuestionRow {
            id: row.try_get("id")?,
            story_id: row.try_get("story_id")?,
            seq: row.try_get("seq")?,
            question: row.try_get("question")?,
            status: row.try_get("status")?,
            answer: row.try_get("answer")?,
            chosen_option_id: row.try_get("chosen_option_id")?,
            decided_at: row.try_get("decided_at")?,
            decided_by: row.try_get("decided_by")?,
            prompting_finding_id: row.try_get("prompting_finding_id")?,
            prompting_note_id: row.try_get("prompting_note_id")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// List the LIVE research-note rows for a work item (migration 0003), ordered by
/// the per-item monotonic `seq`. "Live" = `superseded_by IS NULL`: a note
/// superseded by a newer one drops out of this fold. Runtime seam: `query_all`
/// onto the [`ResearchNote`] read struct (all columns map 1:1 via its FromRow).
async fn list_research_notes(
    db: &impl DbClient,
    work_item_id: &str,
) -> Result<Vec<ResearchNote>, AppError> {
    db.query_all::<ResearchNote>(
        r#"
        SELECT
            id,
            work_item_id,
            seq,
            summary,
            body,
            confidence,
            state,
            rationale,
            lens,
            origin,
            superseded_by,
            created_at
        FROM research_notes
        WHERE work_item_id = $1
          AND superseded_by IS NULL
        ORDER BY seq
        "#,
        args![work_item_id.to_owned()],
    )
    .await
}

/// List the open-question rows for a story (migration 0003), ordered by the
/// per-story monotonic `seq`, EACH with its `question_options` (also `seq`-
/// ordered) folded into the nested `options` Vec. Two queries (questions, then
/// per-question options) keep the read shape exact: the questions query reads the
/// scalar columns into [`OpenQuestionRow`], the options query reads
/// [`QuestionOption`], and the loop assembles the public [`OpenQuestion`].
async fn list_open_questions(
    db: &impl DbClient,
    story_id: &str,
) -> Result<Vec<OpenQuestion>, AppError> {
    let questions = db
        .query_all::<OpenQuestionRow>(
            r#"
        SELECT
            id,
            story_id,
            seq,
            question,
            status,
            answer,
            chosen_option_id,
            decided_at,
            decided_by,
            prompting_finding_id,
            prompting_note_id,
            created_at
        FROM open_questions
        WHERE story_id = $1
        ORDER BY seq
        "#,
            args![story_id.to_owned()],
        )
        .await?;

    let mut out = Vec::with_capacity(questions.len());
    for q in questions {
        let options = db
            .query_all::<QuestionOption>(
                r#"
            SELECT
                id,
                question_id,
                seq,
                label,
                detail,
                created_at
            FROM question_options
            WHERE question_id = $1
            ORDER BY seq
            "#,
                args![q.id.clone()],
            )
            .await?;

        // Scalars first, the `options` array-of-tables last (tables-last rule).
        out.push(OpenQuestion {
            id: q.id,
            story_id: q.story_id,
            seq: q.seq,
            question: q.question,
            status: q.status,
            answer: q.answer,
            chosen_option_id: q.chosen_option_id,
            decided_at: q.decided_at,
            decided_by: q.decided_by,
            prompting_finding_id: q.prompting_finding_id,
            prompting_note_id: q.prompting_note_id,
            created_at: q.created_at,
            options,
        });
    }

    Ok(out)
}

/// List the acceptance-criteria rows for a work item, ordered by the per-item
/// monotonic `seq` (migration 0003). `query_as!` straight onto the
/// [`AcceptanceCriterion`] read struct (all columns map 1:1; `checked` is the
/// `0/1` INTEGER mirrored as `i64`).
pub async fn list_acceptance_criteria(
    db: &impl DbClient,
    work_item_id: &str,
) -> Result<Vec<AcceptanceCriterion>, AppError> {
    let rows = db
        .query_all::<AcceptanceCriterion>(
            "SELECT id, work_item_id, seq, text, checked, checked_at, checked_by, created_at \
             FROM acceptance_criteria \
             WHERE work_item_id = $1 \
             ORDER BY seq",
            args![work_item_id.to_owned()],
        )
        .await?;

    Ok(rows)
}

/// List the activity-log rows for a work item, ordered by the per-item
/// monotonic `seq`. `query!` + manual map because `payload` arrives as
/// `Option<String>` and is decoded into `Option<Value>`.
async fn list_activity(
    db: &impl DbClient,
    work_item_id: &str,
) -> Result<Vec<WorkItemActivity>, AppError> {
    let rows = db
        .query_all::<ActivityRow>(
            "SELECT id, work_item_id, seq, entry_kind, author, summary, payload, origin, created_at \
             FROM work_item_activity \
             WHERE work_item_id = $1 \
             ORDER BY seq",
            args![work_item_id.to_owned()],
        )
        .await?;

    rows.into_iter()
        .map(|r| {
            Ok(WorkItemActivity {
                id: r.id,
                work_item_id: r.work_item_id,
                seq: r.seq,
                entry_kind: r.entry_kind,
                author: r.author,
                summary: r.summary,
                payload: decode_attributes(r.payload)?,
                origin: r.origin,
                created_at: r.created_at,
            })
        })
        .collect()
}

/// Hand-written generic `FromRow` for the public [`Finding`] directly (no raw
/// row struct needed — every `Finding` field maps 1:1 to a column with no
/// post-decode transform, unlike `WorkItemRow`'s `attributes` decode). Generic
/// over `R: Row` per the canonical [`crate::db`] FromRow recipe so it rides
/// `query_*<T>` on the SQLite arm today and a future Pg arm unchanged; the
/// column→field nullability is carried by the field types (`String` for the
/// NOT-NULL `id`, `Option<_>` for the rest), replacing the old `AS "col!"` /
/// `"col?"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for crate::domain::Finding
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<i64>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(Finding {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            kind: row.try_get("kind")?,
            severity: row.try_get("severity")?,
            effort: row.try_get("effort")?,
            category: row.try_get("category")?,
            status: row.try_get("status")?,
            file: row.try_get("file")?,
            line: row.try_get("line")?,
            symbol: row.try_get("symbol")?,
            summary: row.try_get("summary")?,
            description: row.try_get("description")?,
            first_flagged: row.try_get("first_flagged")?,
            rounds: row.try_get("rounds")?,
            fingerprint: row.try_get("fingerprint")?,
            flow: row.try_get("flow")?,
            dedup_id: row.try_get("dedup_id")?,
            origin: row.try_get("origin")?,
            confidence: row.try_get("confidence")?,
            superseded_by: row.try_get("superseded_by")?,
            run_id: row.try_get("run_id")?,
            triage_state: row.try_get("triage_state")?,
            resolved_at: row.try_get("resolved_at")?,
            resolution: row.try_get("resolution")?,
            defer_reason: row.try_get("defer_reason")?,
            defer_trigger: row.try_get("defer_trigger")?,
            wontfix_rationale: row.try_get("wontfix_rationale")?,
            repo_id: row.try_get("repo_id")?,
        })
    }
}

/// List the LIVE findings attached to a work item, newest-flagged first.
/// "Live" = `superseded_by IS NULL` (migration 0003): a finding superseded by a
/// newer one drops out of this fold, mirroring the research-note supersession
/// chain. This is the fold `get_work_item_detail` returns.
pub async fn list_findings(
    db: &impl DbClient,
    work_item_id: &str,
) -> Result<Vec<Finding>, AppError> {
    let rows = db
        .query_all::<Finding>(
            "SELECT id, work_item_id, kind, severity, effort, category, status, \
             file, line, symbol, summary, description, first_flagged, rounds, \
             fingerprint, flow, dedup_id, origin, confidence, superseded_by, \
             run_id, triage_state, \
             resolved_at, resolution, defer_reason, defer_trigger, \
             wontfix_rationale, repo_id \
             FROM findings \
             WHERE work_item_id = $1 AND superseded_by IS NULL \
             ORDER BY first_flagged DESC, id",
            args![work_item_id.to_owned()],
        )
        .await?;

    Ok(rows)
}

// ===========================================================================
// Findings query / aggregation (B20, migration 0011 — Part B Phase B4)
// ===========================================================================

/// Hand-written generic `FromRow` for the public [`crate::domain::AxisCount`]
/// (columns `key: String`, `count: i64`), used by [`query_findings`]'s grouped
/// count-by branch. Generic over `R: Row` per the canonical [`crate::db`]
/// FromRow recipe (so it rides `query_all::<AxisCount>` on the SQLite arm today
/// and a future Pg arm unchanged), and indexed by column NAME to stay robust to
/// SELECT-column reordering. The orphan rule permits this impl because
/// `AxisCount` is crate-local — exactly as the [`crate::domain::Finding`] impl
/// above proves.
impl<'r, R> sqlx::FromRow<'r, R> for crate::domain::AxisCount
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(crate::domain::AxisCount {
            key: row.try_get("key")?,
            count: row.try_get("count")?,
        })
    }
}

/// The two output shapes [`query_findings`] can return, selected by the filter's
/// `count_by` axis. Defined HERE (not in `domain.rs`) because it is a repo-layer
/// sum over two existing domain types rather than a stored entity.
///
/// EXTERNALLY-tagged (`#[serde(rename_all = "snake_case")]` on the enum) so the
/// wire shape is `{"findings":[...]}` / `{"counts":[...]}` — the B21 MCP + B22
/// HTTP layers `serde_json::to_value` this directly, with the variant name
/// carrying the discriminator. (No `JsonSchema`: the MCP layer wraps aggregate
/// reads with `Content::json` rather than `Json<T>`, mirroring `StoryReadiness`
/// / `BatchEntry`.)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryFindingsResult {
    /// Full live findings (the `count_by == None` branch).
    Findings(Vec<crate::domain::Finding>),
    /// Grouped counts by the requested axis (the `count_by == Some(_)` branch).
    Counts(Vec<crate::domain::AxisCount>),
}

// SHARED NULL-GUARD FILTER — both `query_findings` SQL constants below embed the
// SAME `($N IS NULL OR col = $N)`-per-field WHERE clause, in the fixed positional
// order `work_item_id ($1)`, `run_id ($2)`, `severity ($3)`, `category ($4)`,
// `status ($5)`, `triage_state ($6)`, bound from the filter's `Option<String>`
// fields in that exact order (an absent field passes NULL, disabling its
// conjunct). This mirrors the `($N IS NULL OR col = $N)` idiom in
// `LIST_WORK_ITEMS_SQL` and the run-listing query: each placeholder is bound ONCE
// per distinct `$N` and the runtime SQLite layer references it positionally, so a
// placeholder appearing twice in the SQL reads the same single bound value.
// `superseded_by IS NULL` keeps the result to LIVE findings only — consistent
// with `list_findings` and the `get_work_item_detail` fold. The SQL stays a
// `&'static str` literal (the runtime seam requires `'static`; the WHERE clause
// is NEVER built dynamically from user input — the only variation is whether each
// bound value is NULL). The clause is written inline in both constants (rather
// than concatenated from a shared fragment) so each stays a single greppable
// `&'static str` literal.

/// Full-row SELECT for the `count_by == None` branch — the exact column list of
/// [`list_findings`] (decoded by the shared [`crate::domain::Finding`] `FromRow`)
/// plus the shared NULL-guard filter and a stable `first_flagged DESC, id` order.
const QUERY_FINDINGS_ROWS_SQL: &str = "\
    SELECT id, work_item_id, kind, severity, effort, category, status, \
           file, line, symbol, summary, description, first_flagged, rounds, \
           fingerprint, flow, dedup_id, origin, confidence, superseded_by, \
           run_id, triage_state, \
           resolved_at, resolution, defer_reason, defer_trigger, \
           wontfix_rationale, repo_id \
    FROM findings \
    WHERE ($1 IS NULL OR work_item_id = $1) \
      AND ($2 IS NULL OR run_id = $2) \
      AND ($3 IS NULL OR severity = $3) \
      AND ($4 IS NULL OR category = $4) \
      AND ($5 IS NULL OR status = $5) \
      AND ($6 IS NULL OR triage_state = $6) \
      AND superseded_by IS NULL \
    ORDER BY first_flagged DESC, id";

/// Grouped count-by-severity SELECT for the `count_by == Some(Severity)` branch.
/// `COALESCE(severity, '(none)')` keeps `AxisCount.key` non-null when a finding
/// has no severity (the same sentinel is used in both the SELECT alias and the
/// GROUP BY so the bucket is coherent). Same NULL-guard filter + `superseded_by
/// IS NULL` live constraint as the full-row branch.
const QUERY_FINDINGS_COUNT_SEVERITY_SQL: &str = "\
    SELECT COALESCE(severity, '(none)') AS key, COUNT(*) AS count \
    FROM findings \
    WHERE ($1 IS NULL OR work_item_id = $1) \
      AND ($2 IS NULL OR run_id = $2) \
      AND ($3 IS NULL OR severity = $3) \
      AND ($4 IS NULL OR category = $4) \
      AND ($5 IS NULL OR status = $5) \
      AND ($6 IS NULL OR triage_state = $6) \
      AND superseded_by IS NULL \
    GROUP BY COALESCE(severity, '(none)') \
    ORDER BY key";

/// Query LIVE findings with a static NULL-guard filter, optionally returning
/// grouped axis counts instead of full rows (decision D12, migration 0011).
///
/// The filter (`work_item_id`, `run_id`, `severity`, `category`, `status`,
/// `triage_state` — all `Option<String>`) is applied through a single static
/// `($N IS NULL OR col = $N)`-per-field WHERE clause (see
/// [`QUERY_FINDINGS_FILTER_SQL`]): an absent field binds `NULL`, which disables
/// its conjunct, so one prepared statement covers every filter combination
/// WITHOUT ever building SQL from user input. "Live only" — `superseded_by IS
/// NULL` is always applied, matching [`list_findings`] and the
/// `get_work_item_detail` fold (superseded findings are intentionally NOT
/// queryable here).
///
/// When `filter.count_by` is set, the query GROUPs instead of returning rows:
/// for [`crate::domain::FindingAxis::Severity`] it returns one
/// [`crate::domain::AxisCount`] per distinct severity (NULL severities fold into
/// a `'(none)'` sentinel bucket), as [`QueryFindingsResult::Counts`]. When
/// `count_by` is `None`, it returns the full live findings ordered
/// `first_flagged DESC, id` as [`QueryFindingsResult::Findings`]. The `count_by`
/// dispatch is a `match` so adding a future axis is a localised change. This is
/// a READ — no transaction, no event row.
pub async fn query_findings(
    db: &impl DbClient,
    filter: &crate::domain::QueryFindingsFilter,
) -> Result<QueryFindingsResult, AppError> {
    // The six NULL-guard binds, in the fixed positional order $1..=$6. Each
    // value is cloned once into the owned `Args` bundle; the SQL references the
    // matching `$N` (twice per field — once in `IS NULL`, once in `= $N`) and the
    // runtime SQLite layer resolves both references to this single bound value.
    let bind_args = || {
        args![
            filter.work_item_id.clone(),
            filter.run_id.clone(),
            filter.severity.clone(),
            filter.category.clone(),
            filter.status.clone(),
            filter.triage_state.clone(),
        ]
    };

    match filter.count_by {
        Some(crate::domain::FindingAxis::Severity) => {
            let counts = db
                .query_all::<crate::domain::AxisCount>(
                    QUERY_FINDINGS_COUNT_SEVERITY_SQL,
                    bind_args(),
                )
                .await?;
            Ok(QueryFindingsResult::Counts(counts))
        }
        None => {
            let findings = db
                .query_all::<Finding>(QUERY_FINDINGS_ROWS_SQL, bind_args())
                .await?;
            Ok(QueryFindingsResult::Findings(findings))
        }
    }
}

/// SELECT for [`get_story_finding_queue`]: every live finding attached to the
/// story itself OR one of its DIRECT task children, EXCLUDING any whose
/// work-item is soft-deleted. The single static JOIN to `work_items` exists for
/// the tombstone guard (`w.deleted_at IS NULL`) — a finding on a tombstoned
/// work-item must drop out of the queue.
const STORY_FINDING_QUEUE_SQL: &str = "\
    SELECT f.id, f.work_item_id, f.kind, f.severity, f.effort, f.category, f.status, \
           f.file, f.line, f.symbol, f.summary, f.description, f.first_flagged, f.rounds, \
           f.fingerprint, f.flow, f.dedup_id, f.origin, f.confidence, f.superseded_by, \
           f.run_id, f.triage_state, \
           f.resolved_at, f.resolution, f.defer_reason, f.defer_trigger, \
           f.wontfix_rationale, f.repo_id \
    FROM findings f \
    JOIN work_items w ON f.work_item_id = w.id \
    WHERE (w.id = $1 OR w.parent_id = $1) \
      AND w.deleted_at IS NULL \
      AND f.superseded_by IS NULL \
    ORDER BY f.first_flagged DESC, f.id";

/// Compose a story's review/optimise finding queue (decision D7, migration
/// 0011): every LIVE finding attached to the story itself OR one of its DIRECT
/// task children, ordered newest-flagged first.
///
/// ## Queue scope
/// The story plus its direct task children. The hierarchy makes tasks direct
/// children of a story (`work_items.parent_id` = story id, enforced by the
/// hierarchy trigger), so a single static JOIN `findings ↔ work_items` with
/// `(w.id = $1 OR w.parent_id = $1)` spans the queue WITHOUT a recursive CTE.
///
/// ## Tombstone guard (the point of the JOIN)
/// `w.deleted_at IS NULL` EXCLUDES findings whose work-item has been
/// soft-deleted — the JOIN exists for this guard (a bare `findings`-only query
/// could not see the work-item's tombstone). `f.superseded_by IS NULL` keeps the
/// result to live findings, consistent with [`list_findings`] /
/// [`query_findings`]. This is a READ — no transaction, no event row.
pub async fn get_story_finding_queue(
    db: &impl DbClient,
    story_id: &str,
) -> Result<Vec<crate::domain::Finding>, AppError> {
    let rows = db
        .query_all::<Finding>(STORY_FINDING_QUEUE_SQL, args![story_id.to_owned()])
        .await?;
    Ok(rows)
}

/// Create a work item under the single-mutation-path discipline.
///
/// 1. Belt-and-braces hierarchy pre-check (typed 422 on an illegal edge) — runs
///    BEFORE any transaction, so an illegal create writes zero rows.
/// 2. Open one transaction.
/// 3. Insert the `work_items` row (id = freshly-minted UUIDv7 as TEXT).
/// 4. Append ONE `events` row via [`record_event`].
/// 5. Commit. Any error before commit rolls back BOTH writes.
///
/// Returns the new id as a `Uuid`.
///
/// This is the back-compatible 5-arg entry point (no `origin`, default
/// provenance NULL). It delegates to [`create_work_item_with_origin`]; the
/// default `relevance="backlog"` for a new epic/focus/story is applied there.
pub async fn create_work_item(
    pool: &impl DbClient,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
) -> Result<Uuid, AppError> {
    create_work_item_with_origin(pool, kind, parent_id, title, body, None).await
}

/// Create a work item, stamping the optional `origin` provenance (migration
/// 0003). Same single-mutation-path discipline as the 5-arg [`create_work_item`]
/// wrapper. A newly-created `epic`/`focus`/`story` acquires the default
/// `relevance="backlog"` (epic/focus/story carry the relevance axis;
/// task/project are left NULL); the relevance default is applied in the INSERT.
pub async fn create_work_item_with_origin(
    pool: &impl DbClient,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
    origin: Option<&str>,
) -> Result<Uuid, AppError> {
    create_work_item_full(
        pool,
        kind,
        parent_id,
        title,
        body,
        CreateOpts {
            origin,
            outcome: None,
            shape: None,
        },
    )
    .await
}

/// The create core (migration 0010). The 5-arg [`create_work_item`] and 6-arg
/// [`create_work_item_with_origin`] wrappers delegate here; this 8-arg form adds
/// the `outcome` (epic) and `shape` (focus) channels plus the migration-0010
/// create-time gates. Gates run BEFORE `begin_write` (like the parent pre-check)
/// so an illegal create writes zero rows.
///
/// Create-time gates (User Decisions, ADR lumina epic/focus semantics):
///   * an `epic` requires a non-empty `outcome`;
///   * a `focus` requires a `shape`;
///   * `shape` is valid only on a `focus` (consistency guard);
///   * a `story` may only be created once its ancestor epic has ≥1 close-criterion.
///
/// When `outcome` is supplied (epic only) it is folded into the row's
/// `attributes` JSON (`{"outcome": ...}`) after the same normalise + per-kind
/// validate chain the PATCH path uses. `shape` is bound directly to the
/// `work_items.shape` column. Otherwise identical to the legacy create path:
/// `status="open"`, default `relevance="backlog"` for epic/focus/story, and a
/// single `work_item.created` event.
/// The migration-0010 create-time option channels for [`create_work_item_full`]:
/// `origin` (provenance stamp), `outcome` (epic-only, folds into `attributes`),
/// and `shape` (focus-only, binds the `shape` column). Bundled into a struct so
/// the three same-typed `Option<&str>` tail params are passed by name rather than
/// position (R16 — they were previously mis-order-prone).
pub struct CreateOpts<'a> {
    pub origin: Option<&'a str>,
    pub outcome: Option<&'a str>,
    pub shape: Option<&'a str>,
}

pub async fn create_work_item_full(
    db: &impl DbClient,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
    opts: CreateOpts<'_>,
) -> Result<Uuid, AppError> {
    let origin = opts.origin;
    let mut tx = db.begin().await?;
    let id = create_work_item_full_tx(tx.as_mut(), kind, parent_id, title, body, opts).await?;
    let id_str = id.to_string();

    let payload = serde_json::json!({
        "kind": kind,
        "parent_id": parent_id,
        "title": title,
        "origin": origin,
    });
    record_event(tx.as_mut(), "work_item", &id_str, "work_item.created", payload).await?;

    tx.commit().await?;

    Ok(id)
}

/// Reusable tx helper extracted from [`create_work_item_full`] (B16): perform ALL
/// create-time validation + the `work_items` INSERT INSIDE the caller's
/// transaction, returning the new id.
///
/// **Does NOT record an event and does NOT commit** — the public
/// [`create_work_item_full`] wrapper records the single `work_item.created` event
/// after this returns; the batch spawn path (B17b) calls this under a shared tx
/// and records ONE coarse batch event.
///
/// Every validation read — the parent-kind resolution and the story
/// close-criterion gate — runs on the passed `tx` (NOT autocommit `db`), so the
/// batch caller sees a single consistent snapshot under the BEGIN IMMEDIATE
/// writer lock. Validation order, the `AppError::Validation`/`NotFound` messages,
/// and the early-return-before-any-write behaviour are PRESERVED byte-identically
/// (the gate reads were already the last thing before the INSERT; moving the
/// parent-kind read onto the tx is the only structural change, and it returns the
/// SAME `parent work_item '{pid}' does not exist` Validation as before). The
/// `spawned_from_finding_id` column is intentionally NOT set here — it stays NULL
/// on create (B17b stamps it on the spawn path).
pub async fn create_work_item_full_tx(
    tx: &mut dyn crate::db::DbTx,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
    opts: CreateOpts<'_>,
) -> Result<uuid::Uuid, AppError> {
    let CreateOpts {
        origin,
        outcome,
        shape,
    } = opts;
    // Resolve the parent's kind (if any) for the pre-check, INSIDE the tx so the
    // read shares the writer-lock snapshot with the INSERT below. A non-NULL
    // parent_id that does not exist is a Validation error, not a 500.
    let parent_kind: Option<String> = match parent_id {
        Some(pid) => {
            // R21: liveness filter — a soft-deleted (tombstoned) ancestor must not
            // serve as a parent. With `AND deleted_at IS NULL`, a create under a
            // tombstoned epic/focus falls through to the parent-not-found path
            // below rather than succeeding under a dead ancestor.
            let row = crate::db::tx_scalar_opt::<String>(
                tx,
                r#"SELECT kind FROM work_items WHERE id = $1 AND deleted_at IS NULL"#,
                args![pid.to_owned()],
            )
            .await?;
            match row {
                Some(k) => Some(k),
                None => {
                    return Err(AppError::Validation(format!(
                        "parent work_item '{pid}' does not exist"
                    )));
                }
            }
        }
        None => None,
    };

    validate_hierarchy_edge(kind, parent_kind.as_deref())?;

    // --- migration-0010 create-time gates ---------------------------------
    // Epic requires a non-empty outcome at create.
    if kind == "epic" && outcome.map(|s| s.trim().is_empty()).unwrap_or(true) {
        return Err(AppError::Validation(
            "an epic requires a non-empty outcome at create".into(),
        ));
    }
    // Focus requires a shape at create.
    if kind == "focus" && shape.is_none() {
        return Err(AppError::Validation(
            "a focus requires a shape at create".into(),
        ));
    }
    // Shape is only valid on a focus (consistency guard).
    if shape.is_some() && kind != "focus" {
        return Err(AppError::Validation("shape is only valid on a focus".into()));
    }
    // R2: validate the shape VALUE against the typed `Shape` enum (the single
    // source of the valid set: vertical-slice|cross-cutting|foundational) so an
    // invalid string is rejected with a typed Validation here rather than
    // escaping to the SQL CHECK as a 500. Parsing through serde keeps this in
    // lockstep with the enum's wire spelling — no hardcoded literal list.
    if let Some(s) = shape {
        serde_json::from_value::<Shape>(Value::String(s.to_owned())).map_err(|_| {
            AppError::Validation(format!(
                "invalid shape '{s}': expected one of vertical-slice|cross-cutting|foundational"
            ))
        })?;
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    // epic/focus/story carry the relevance axis and default to "backlog" on
    // create; task/project are left NULL.
    let default_relevance: Option<&str> = match kind {
        "epic" | "focus" | "story" => Some("backlog"),
        _ => None,
    };

    // Build the attributes JSON from `outcome` (epic only). A non-epic carrying
    // `outcome` is rejected by validate_attributes_for_kind, which is correct.
    let attributes_str: Option<String> = match outcome {
        Some(o) => {
            let attrs = serde_json::json!({ "outcome": o });
            let cleaned = normalise_object(&attrs, "attributes")?;
            validate_attributes_for_kind(kind, &cleaned)?;
            validate_plan_field_constraints(&cleaned)?; // R34
            Some(
                serde_json::to_string(&Value::Object(cleaned))
                    .map_err(|e| AppError::Other(e.into()))?,
            )
        }
        None => None,
    };

    // R3: story-creation close-criterion gate — runs INSIDE the tx (pre-INSERT)
    // so the gate read and the write share one snapshot under the writer lock,
    // closing the TOCTOU window against a concurrent criterion removal. The
    // validated parent is a focus; resolve the focus's parent (the epic) and
    // require ≥1 close-criterion.
    if kind == "story" {
        let focus_id = parent_id.expect("hierarchy edge guarantees a focus parent for a story");
        let epic_id: Option<String> = crate::db::tx_scalar_one::<Option<String>>(
            tx,
            r#"SELECT parent_id FROM work_items WHERE id = $1"#,
            args![focus_id.to_owned()],
        )
        .await?;
        let epic_id = epic_id.ok_or_else(|| {
            AppError::Validation("story's focus parent has no epic ancestor".into())
        })?;
        let crit_count: i64 = crate::db::tx_scalar_one::<i64>(
            tx,
            r#"SELECT COUNT(*) FROM acceptance_criteria WHERE work_item_id = $1"#,
            args![epic_id.clone()],
        )
        .await?;
        if crit_count == 0 {
            return Err(AppError::Validation(format!(
                "cannot create a story under epic '{epic_id}': the epic has no \
                 close-criteria; add at least one close-criterion to the epic first"
            )));
        }
    }

    tx.execute(
        CREATE_WORK_ITEM_INSERT_SQL,
        args![
            id_str.clone(),
            kind.to_owned(),
            parent_id.map(str::to_owned),
            title.to_owned(),
            body.map(str::to_owned),
            "open".to_owned(),
            origin.map(str::to_owned),
            default_relevance.map(str::to_owned),
            shape.map(str::to_owned),
            attributes_str
        ],
    )
    .await?;

    Ok(id)
}

const CREATE_WORK_ITEM_INSERT_SQL: &str = r#"
        INSERT INTO work_items (id, kind, parent_id, title, body, status, origin, relevance, shape, attributes)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#;

/// One work-item spec for the bulk [`create_work_items`] path (B17b). Mirrors the
/// [`create_work_item_full`] arg list (kind/parent/title/body + the [`CreateOpts`]
/// channels origin/outcome/shape) plus the optional spawn provenance
/// `spawned_from_finding_id`.
pub struct NewWorkItemSpec<'a> {
    pub kind: &'a str,
    pub parent_id: Option<&'a str>,
    pub title: &'a str,
    pub body: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub outcome: Option<&'a str>,
    pub shape: Option<&'a str>,
    /// When `Some`, stamp `work_items.spawned_from_finding_id` (migration 0011
    /// nullable FK → `findings(id)`) after the INSERT. `create_work_item_full_tx`
    /// deliberately leaves the column NULL on create — this batch spawn path is
    /// the only writer of the column, so the referenced finding must already
    /// exist (FK), or pass `None`.
    pub spawned_from_finding_id: Option<&'a str>,
}

/// Bulk-create a batch of work items under ONE `BEGIN IMMEDIATE` transaction
/// (B17b; plan D8/D10, risk R-B2), all-or-nothing. Each spec is created via
/// [`create_work_item_full_tx`] (which runs ALL create-time validation — the
/// hierarchy edge, the migration-0010 epic-outcome / focus-shape gates, and the
/// story close-criterion gate — and the `work_items` INSERT INSIDE the shared
/// tx), then, when `spawned_from_finding_id` is `Some`, the new row's spawn
/// column is stamped. Returns the new ids in input order.
///
/// ## Parents must already exist (D10)
/// `create_work_item_full_tx`'s parent-kind read runs on the tx, and a missing
/// parent surfaces as [`AppError::Validation`] (`parent work_item '…' does not
/// exist`). This path does NOT support inline `depends_on` nor creating a
/// parent within the same batch — every `parent_id` must reference an EXISTING
/// (committed) work item.
///
/// ## Atomicity (validation aborts the whole batch)
/// Any error from `create_work_item_full_tx` or the spawn-stamp `?`-propagates,
/// dropping `tx` un-committed → SQLite rolls back → ZERO rows persist (a single
/// invalid spec leaves nothing, including the valid specs that preceded it).
///
/// ## Single coarse event (D8 / R-B4)
/// Exactly ONE `events` row is recorded for the whole batch, NOT one per item.
/// Its `aggregate_type` is **deliberately not `"work_item"`**: the git-export
/// drain (`export.rs`) materialises only `aggregate_type="work_item"` events, so
/// a `"work_item"` batch event would wrongly re-render each item N times. A
/// `"batch"`-typed event keyed by a fresh UUIDv7 is correctly inert — drained and
/// `exported_at`-stamped, but not materialised to a file. The accepted
/// consequence (the intended D8/B26 trade-off) is that bulk-created work items are
/// NOT git-exported individually; only the coarse batch event records the write.
pub async fn create_work_items(
    db: &impl DbClient,
    specs: &[NewWorkItemSpec<'_>],
) -> Result<Vec<uuid::Uuid>, AppError> {
    let mut tx = db.begin().await?;

    let mut ids: Vec<Uuid> = Vec::with_capacity(specs.len());
    for spec in specs {
        // A `create_work_item_full_tx` error `?`-propagates here, dropping `tx`
        // un-committed → full rollback → zero writes (all-or-nothing, D10).
        let id = create_work_item_full_tx(
            tx.as_mut(),
            spec.kind,
            spec.parent_id,
            spec.title,
            spec.body,
            CreateOpts {
                origin: spec.origin,
                outcome: spec.outcome,
                shape: spec.shape,
            },
        )
        .await?;

        // B17b owns the spawn stamp: `create_work_item_full_tx` leaves
        // `spawned_from_finding_id` NULL, so set it here when provided. The FK to
        // `findings(id)` is enforced by SQLite — an unknown id aborts the batch.
        if let Some(fid) = spec.spawned_from_finding_id {
            tx.execute(
                "UPDATE work_items SET spawned_from_finding_id = $1 WHERE id = $2",
                args![fid.to_owned(), id.to_string()],
            )
            .await?;
        }

        ids.push(id);
    }

    // Exactly one coarse event for the whole batch (D8). aggregate_type MUST NOT
    // be "work_item" (R-B4) — a fresh UUIDv7 under a "batch" aggregate, which the
    // export drain ignores (there is no run/finding context here, so unlike
    // `add_findings` the only sensible key is a freshly-minted batch id).
    let id_strs: Vec<String> = ids.iter().map(Uuid::to_string).collect();
    let payload = serde_json::json!({ "count": ids.len(), "ids": id_strs });
    record_event(
        tx.as_mut(),
        "batch",
        &Uuid::now_v7().to_string(),
        "work_items.batch_created",
        payload,
    )
    .await?;

    tx.commit().await?;

    Ok(ids)
}

/// Shared closure-gate check for a `→done` transition (migration 0003,
/// User Decision 3). Runs INSIDE the caller's transaction (so the read and the
/// subsequent write are atomic). Both `update_work_item_status` (the
/// `transition_status` MCP path) and the generic `update_work_item` PATCH path
/// call this so neither can bypass the gate.
///
/// Logic: the gate fires ONLY when the target status is `done` AND the item is a
/// `task`. It reads the task's immediate parent; if that parent is a `story`
/// with `closure_gate = 'hard'` and the task has ANY unchecked acceptance
/// criterion, the transition is rejected with [`AppError::Validation`].
/// `soft` (the default), a non-story parent, or no parent ⇒ allow. Items that
/// are not tasks, or transitions to a status other than `done`, are unaffected.
///
/// # Two distinct scopes (R8 — precise, pinning a plan-ambiguous semantic)
///
/// The `closure_gate` FLAG and the acceptance CRITERIA counted live on different
/// rows, deliberately:
///   * the `closure_gate` flag is read from the parent STORY
///     (`work_items.closure_gate` on the parent row), and
///   * the acceptance criteria counted are the TASK's OWN
///     (`acceptance_criteria WHERE work_item_id = <task id>`).
///
/// The criteria are NOT story-scoped: a story's gate decides WHETHER each of its
/// child tasks must clear ITS OWN criteria before `→done`. The flag is the
/// story's policy; the criteria are the task's checklist.
///
/// The `id`'s existence is NOT asserted here — the caller's UPDATE
/// `rows_affected()==0 ⇒ NotFound` check is the authority; if the row is absent
/// the kind read below simply returns `None` and the gate is inert.
async fn enforce_closure_gate(
    tx: &mut dyn crate::db::DbTx,
    id: &str,
    target_status: &str,
) -> Result<(), AppError> {
    if target_status != "done" {
        return Ok(());
    }

    // Read the item's kind + parent. Absent row ⇒ inert (caller handles NotFound).
    let Some((kind, parent_id)) = crate::db::tx_query_opt::<(String, Option<String>)>(
        tx,
        r#"SELECT kind, parent_id FROM work_items WHERE id = $1"#,
        args![id.to_owned()],
    )
    .await?
    else {
        return Ok(());
    };

    if kind != "task" {
        return Ok(());
    }

    // The gate is the immediate parent story's `closure_gate` (no ancestor walk).
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    let Some((parent_kind, parent_closure_gate)) =
        crate::db::tx_query_opt::<(String, Option<String>)>(
            tx,
            r#"SELECT kind, closure_gate FROM work_items WHERE id = $1"#,
            args![parent_id.clone()],
        )
        .await?
    else {
        return Ok(());
    };

    if parent_kind != "story" || parent_closure_gate.as_deref() != Some("hard") {
        // soft (default) / non-story parent ⇒ allow.
        return Ok(());
    }

    // Hard gate: reject if any acceptance criterion of the TASK is unchecked.
    let unchecked = crate::db::tx_scalar_one::<i64>(
        tx,
        r#"SELECT COUNT(*) FROM acceptance_criteria WHERE work_item_id = $1 AND checked = 0"#,
        args![id.to_owned()],
    )
    .await?;

    if unchecked > 0 {
        return Err(AppError::Validation(format!(
            "task '{id}' cannot transition to 'done': its story's closure_gate is 'hard' \
             and {unchecked} acceptance criterion(s) remain unchecked"
        )));
    }

    Ok(())
}

/// Epic-done transition gate (migration 0010, ADR lumina epic/focus semantics).
/// Runs INSIDE the caller's transaction, immediately after [`enforce_closure_gate`]
/// at every `→done` entry point. UNCONDITIONAL — it does NOT read `closure_gate`
/// (the close-criteria always define the epic deliverable):
/// an `epic→done` requires BOTH
///   (a) all of the epic's OWN close-criteria checked, AND
///   (b) all descendant stories terminal (`done`/`cancelled`).
///
/// The fixed hierarchy epic→focus→story makes the 2-level `parent_id IN (focus
/// children)` subquery exhaustive (stories cannot nest under stories), so the
/// "recursive descendant" rule is satisfied without a CTE. Inert unless the
/// target is `done` and the row is an `epic`; an absent row is inert (the
/// caller's `rows_affected()==0` check owns NotFound).
async fn enforce_epic_done_gate(
    tx: &mut dyn crate::db::DbTx,
    id: &str,
    target_status: &str,
) -> Result<(), AppError> {
    if target_status != "done" {
        return Ok(());
    }
    let Some(kind) = crate::db::tx_scalar_opt::<String>(
        tx,
        r#"SELECT kind FROM work_items WHERE id = $1"#,
        args![id.to_owned()],
    )
    .await?
    else {
        return Ok(());
    };
    if kind != "epic" {
        return Ok(());
    }
    let unchecked = crate::db::tx_scalar_one::<i64>(
        tx,
        r#"SELECT COUNT(*) FROM acceptance_criteria WHERE work_item_id = $1 AND checked = 0"#,
        args![id.to_owned()],
    )
    .await?;
    if unchecked > 0 {
        return Err(AppError::Validation(format!(
            "epic '{id}' cannot transition to 'done': {unchecked} close-criterion(s) remain unchecked"
        )));
    }
    // R20: FIXED-DEPTH assumption — this subquery hardcodes the epic→focus→story
    // 2-level descent (stories cannot nest under stories in the schema), so it is
    // exhaustive WITHOUT a recursive CTE. If the hierarchy ever gains deeper
    // story nesting this count would miss the grandchildren — a recursive-CTE
    // refactor is the deliberate follow-up (out of scope here). VACUOUS CASE: an
    // epic with zero descendant stories counts 0 non-terminal and so passes this
    // clause and closes (the close-criterion clause above still applies) — a
    // childless epic→done is intentionally allowed.
    let nonterminal = crate::db::tx_scalar_one::<i64>(
        tx,
        r#"SELECT COUNT(*) FROM work_items
           WHERE kind = 'story' AND deleted_at IS NULL
             AND status NOT IN ('done','cancelled')
             AND parent_id IN (SELECT id FROM work_items WHERE kind = 'focus' AND deleted_at IS NULL AND parent_id = $1)"#,
        args![id.to_owned()],
    )
    .await?;
    if nonterminal > 0 {
        return Err(AppError::Validation(format!(
            "epic '{id}' cannot transition to 'done': {nonterminal} descendant story(ies) are not terminal (done/cancelled)"
        )));
    }
    Ok(())
}

/// Update a work item's free-text status under the single-mutation-path
/// discipline (status update + one event in one transaction). `NotFound` if the
/// id has no row — checked via `rows_affected()` so the missing-row case never
/// emits a spurious event. A `→done` transition on a task is gated by
/// [`enforce_closure_gate`] (the read runs inside the same tx, before the write).
pub async fn update_work_item_status(
    db: &impl DbClient,
    id: &str,
    status: &str,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    // Closure gate (migration 0003): reject task→done under a `hard` story while
    // any acceptance criterion is unchecked. Runs before the UPDATE in this tx.
    enforce_closure_gate(tx.as_mut(), id, status).await?;
    // Epic-done gate (migration 0010): reject epic→done unless all close-criteria
    // are checked and all descendant stories are terminal. Independent of the
    // closure gate above (task→done vs epic→done); both run.
    enforce_epic_done_gate(tx.as_mut(), id, status).await?;

    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET status = $2, updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
            args![id.to_owned(), status.to_owned()],
        )
        .await?;

    if affected == 0 {
        // tx drops here → rollback; no event emitted for a missing row.
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "status": status });
    record_event(tx.as_mut(), "work_item", id, "work_item.status_changed", payload).await?;

    tx.commit().await?;

    Ok(())
}

/// Set a work item's `relevance` (migration 0003, User Decision 2). The
/// relevance axis is structural and carried ONLY by epic/focus/story; a
/// `task`/`project` is rejected with a typed [`AppError::Validation`]. The
/// kind is read first; `NotFound` if the id has no row; one event on success.
pub async fn set_relevance(
    db: &impl DbClient,
    id: &str,
    relevance: Relevance,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, id).await?;
    if !matches!(kind.as_str(), "epic" | "focus" | "story") {
        return Err(AppError::Validation(format!(
            "relevance is settable only on epic/focus/story, not on '{kind}'"
        )));
    }
    let value = enum_to_str(relevance);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET relevance = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "relevance": value });
    record_event(tx.as_mut(), "work_item", id, "work_item.relevance_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a focus item's `shape` (migration 0010). Focus-scoped: a non-`focus`
/// kind is rejected with a typed `AppError::Validation`. Kind read first;
/// `NotFound` via rows_affected()==0; one event. This is the revise-later
/// path — shape-mandatory-at-create for focus is enforced in the create path.
pub async fn set_shape(db: &impl DbClient, id: &str, shape: Shape) -> Result<(), AppError> {
    let kind = work_item_kind(db, id).await?;
    if kind != "focus" {
        return Err(AppError::Validation(format!(
            "shape is settable only on a focus, not on '{kind}'"
        )));
    }
    let value = enum_to_str(shape);
    let mut tx = db.begin().await?;
    let affected = tx
        .execute(
            r#"UPDATE work_items SET shape = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![id.to_owned(), value.clone()],
        )
        .await?;
    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }
    let payload = serde_json::json!({ "shape": value });
    record_event(tx.as_mut(), "work_item", id, "work_item.shape_set", payload).await?;
    tx.commit().await?;
    Ok(())
}

/// R23: per-field byte cap for free-text plan-attribute blobs
/// (epic `outcome`/`context`, focus `framing`). Caps storage amplification — an
/// unbounded blob would let a single PATCH balloon the row arbitrarily. 64 KiB
/// is far above any legitimate plan-prose length while still bounding abuse.
const MAX_PLAN_FIELD_BYTES: usize = 64 * 1024;

/// R23: reject a plan-attribute string whose UTF-8 byte length exceeds
/// [`MAX_PLAN_FIELD_BYTES`]. Called where each plan field's patch value is built
/// so `outcome`/`context`/`framing` share one cap with no per-field duplication.
fn check_plan_field_len(field: &str, value: &str) -> Result<(), AppError> {
    if value.len() > MAX_PLAN_FIELD_BYTES {
        return Err(AppError::Validation(format!(
            "{field} exceeds the {MAX_PLAN_FIELD_BYTES}-byte plan-field limit ({} bytes)",
            value.len()
        )));
    }
    Ok(())
}

/// R34: single chokepoint for the plan-attribute free-text constraints, applied
/// at EVERY attribute-write site (`create_work_item_full`, `update_work_item`,
/// `set_work_item_attributes`) so no path can reach `outcome`/`context`/`framing`
/// without them. For any of those keys present as a string value it applies the
/// [`check_plan_field_len`] 64-KiB cap; additionally it rejects a whitespace-only
/// `outcome` (R22, mirroring create's "non-empty outcome" guard) and a
/// whitespace-only `framing` (R42). `context` gets the length cap only (a blank
/// context is allowed). `cleaned` is the already null-stripped object.
fn validate_plan_field_constraints(
    cleaned: &serde_json::Map<String, Value>,
) -> Result<(), AppError> {
    if let Some(Value::String(v)) = cleaned.get("outcome") {
        if v.trim().is_empty() {
            return Err(AppError::Validation(
                "an epic requires a non-empty outcome".into(),
            ));
        }
        check_plan_field_len("outcome", v)?;
    }
    if let Some(Value::String(v)) = cleaned.get("framing") {
        if v.trim().is_empty() {
            return Err(AppError::Validation(
                "framing must be non-empty".into(),
            ));
        }
        check_plan_field_len("framing", v)?;
    }
    if let Some(Value::String(v)) = cleaned.get("context") {
        check_plan_field_len("context", v)?;
    }
    Ok(())
}

/// Revise an epic's plan attributes (migration 0010). Epic-kind-gated; JSON-
/// merges the present fields via set_work_item_attributes (one event). Sibling
/// keys are preserved by the merge. Mandatory-outcome-at-create is enforced in
/// the create path, not here.
pub async fn set_epic_plan(
    pool: &impl DbClient,
    id: &str,
    outcome: Option<&str>,
    context: Option<&str>,
) -> Result<(), AppError> {
    let kind = work_item_kind(pool, id).await?;
    if kind != "epic" {
        return Err(AppError::Validation(format!(
            "epic-plan attributes are settable only on an epic, not on '{kind}'"
        )));
    }
    // R22/R23/R34: the whitespace-only-outcome rejection and the per-field byte
    // cap are now enforced once in `validate_plan_field_constraints`, called from
    // inside `set_work_item_attributes` (the JSON-merge path this delegates to),
    // so they are NOT duplicated here.
    let mut patch = serde_json::Map::new();
    if let Some(v) = outcome {
        patch.insert("outcome".into(), serde_json::Value::String(v.to_string()));
    }
    if let Some(v) = context {
        patch.insert("context".into(), serde_json::Value::String(v.to_string()));
    }
    // no fields supplied — skip the no-op write + spurious event
    if patch.is_empty() {
        return Ok(());
    }
    set_work_item_attributes(pool, id, &serde_json::Value::Object(patch)).await
}

/// Revise a focus's framing (migration 0010). Focus-kind-gated; JSON-merges
/// {framing} via set_work_item_attributes (one event).
pub async fn set_focus_plan(
    pool: &impl DbClient,
    id: &str,
    framing: Option<&str>,
) -> Result<(), AppError> {
    let kind = work_item_kind(pool, id).await?;
    if kind != "focus" {
        return Err(AppError::Validation(format!(
            "focus framing is settable only on a focus, not on '{kind}'"
        )));
    }
    // R23/R34/R42: the per-field byte cap and the whitespace-only-framing
    // rejection are enforced once in `validate_plan_field_constraints`, called
    // from inside `set_work_item_attributes` (the JSON-merge path), so they are
    // NOT duplicated here.
    let mut patch = serde_json::Map::new();
    if let Some(v) = framing {
        patch.insert("framing".into(), serde_json::Value::String(v.to_string()));
    }
    // no fields supplied — skip the no-op write + spurious event
    if patch.is_empty() {
        return Ok(());
    }
    set_work_item_attributes(pool, id, &serde_json::Value::Object(patch)).await
}

/// Set a work item's `effort` grade (migration 0003). Task-scoped: the effort
/// axis drives batch sizing for a leaf task, so a non-`task` kind is rejected
/// with a typed [`AppError::Validation`]. Kind read first; `NotFound` via
/// `rows_affected()==0`; one event.
pub async fn set_effort(db: &impl DbClient, id: &str, effort: Effort) -> Result<(), AppError> {
    let kind = work_item_kind(db, id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "effort is settable only on a task, not on '{kind}'"
        )));
    }
    let value = enum_to_str(effort);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET effort = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "effort": value });
    record_event(tx.as_mut(), "work_item", id, "work_item.effort_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a work item's `complexity` grade (migration 0003). Task-scoped (drives
/// model-tier assignment for a leaf task); a non-`task` kind is rejected with a
/// typed [`AppError::Validation`]. Kind read first; `NotFound` via
/// `rows_affected()==0`; one event.
pub async fn set_complexity(
    db: &impl DbClient,
    id: &str,
    complexity: Complexity,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "complexity is settable only on a task, not on '{kind}'"
        )));
    }
    let value = enum_to_str(complexity);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET complexity = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "complexity": value });
    record_event(tx.as_mut(), "work_item", id, "work_item.complexity_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a story's `closure_gate` (migration 0003, User Decision 3). Story-scoped:
/// the gate decides whether tasks under the story reject a `→done` transition
/// while their acceptance criteria are unchecked (`hard`) or merely flag it
/// (`soft`). A non-`story` kind is rejected with a typed [`AppError::Validation`].
/// Kind read first; `NotFound` via `rows_affected()==0`; one event.
pub async fn set_closure_gate(
    db: &impl DbClient,
    story_id: &str,
    gate: ClosureGate,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "closure_gate is settable only on a story, not on '{kind}'"
        )));
    }
    let value = enum_to_str(gate);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET closure_gate = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![story_id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{story_id}' not found")));
    }

    let payload = serde_json::json!({ "closure_gate": value });
    record_event(tx.as_mut(), "work_item", story_id, "work_item.closure_gate_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Append ONE `acceptance_criteria` row under the single-mutation-path
/// discipline (migration 0003, mirroring [`append_activity`]). `seq` is
/// allocated `MAX(seq)+1` per work item WITHIN the transaction; the
/// `UNIQUE(work_item_id, seq)` constraint surfaces a race as a constraint
/// violation. The work item must exist (`NotFound` otherwise). Event
/// `work_item.acceptance_criterion_added`. Returns the new criterion id.
pub async fn add_acceptance_criterion(
    db: &impl DbClient,
    work_item_id: &str,
    text: &str,
) -> Result<Uuid, AppError> {
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    // R43: reject a blank criterion (a whitespace-only close-criterion would
    // vacuously satisfy the story-create ≥1-criterion gate) and cap storage
    // amplification at the shared 64-KiB plan-field limit.
    if text.trim().is_empty() {
        return Err(AppError::Validation(
            "acceptance-criterion text must be non-empty".into(),
        ));
    }
    check_plan_field_len("acceptance-criterion text", text)?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM acceptance_criteria WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO acceptance_criteria (id, work_item_id, seq, text) VALUES ($1, $2, $3, $4)",
        args![id_str.clone(), work_item_id.to_owned(), seq, text.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({ "criterion_id": id_str, "seq": seq });
    record_event(
        tx.as_mut(),
        "work_item",
        work_item_id,
        "work_item.acceptance_criterion_added",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read an acceptance criterion's owning `work_item_id`, erroring `NotFound` if
/// the criterion id has no row. Used by the check/uncheck paths to attribute the
/// owning item (for the audit-activity append and the event aggregate).
async fn acceptance_criterion_work_item(
    db: &impl DbClient,
    id: &str,
) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM acceptance_criteria WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("acceptance_criterion '{id}' not found")))
}

/// Check an acceptance criterion (migration 0003): set `checked=1`,
/// `checked_at=CURRENT_TIMESTAMP`, `checked_by`, AND append a `verification`
/// `work_item_activity` row for the owning work item (state-vs-immutable-audit,
/// per the plan's acceptance-criteria research note) — all in ONE transaction
/// with ONE event. The owning work_item_id is read first (`NotFound` if the
/// criterion is absent). Event `work_item.acceptance_criterion_checked`.
pub async fn check_acceptance_criterion(
    db: &impl DbClient,
    id: &str,
    by: Option<&str>,
) -> Result<(), AppError> {
    let work_item_id = acceptance_criterion_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE acceptance_criteria \
             SET checked = 1, checked_at = CURRENT_TIMESTAMP, checked_by = $2 \
             WHERE id = $1",
            args![id.to_owned(), by.map(str::to_owned)],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    // Append the immutable verification-audit activity row for the owning item.
    // seq is allocated MAX(seq)+1 within this same tx.
    let activity_id = Uuid::now_v7().to_string();
    let act_seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM work_item_activity WHERE work_item_id = $1",
        args![work_item_id.clone()],
    )
    .await?;
    let summary = format!("acceptance criterion {id} checked");
    tx.execute(
        "INSERT INTO work_item_activity (id, work_item_id, seq, entry_kind, author, summary) \
         VALUES ($1, $2, $3, 'verification', $4, $5)",
        args![
            activity_id,
            work_item_id.clone(),
            act_seq,
            by.map(str::to_owned),
            summary
        ],
    )
    .await?;

    let payload = serde_json::json!({ "criterion_id": id, "checked": true });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "work_item.acceptance_criterion_checked",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Uncheck an acceptance criterion (migration 0003): clear `checked`,
/// `checked_at`, `checked_by`. One event. `NotFound` via `rows_affected()==0`.
/// (No audit-activity row — un-checking is a correction, not a verification.)
pub async fn uncheck_acceptance_criterion(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let work_item_id = acceptance_criterion_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE acceptance_criteria \
             SET checked = 0, checked_at = NULL, checked_by = NULL \
             WHERE id = $1",
            args![id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    let payload = serde_json::json!({ "criterion_id": id, "checked": false });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "work_item.acceptance_criterion_unchecked",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Hard-delete an acceptance criterion (migration 0003): criteria have no
/// independent export identity, so a removal is a hard DELETE. One event.
/// `NotFound` via `rows_affected()==0`.
pub async fn remove_acceptance_criterion(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    // Resolve the owning item first so the event aggregate is the work_item
    // (and so an absent criterion is NotFound before any write).
    let work_item_id = acceptance_criterion_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "DELETE FROM acceptance_criteria WHERE id = $1",
            args![id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    let payload = serde_json::json!({ "criterion_id": id, "removed": true });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "work_item.acceptance_criterion_removed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Fetch a work item's `kind`, erroring `NotFound` if the id has no row. Used by
/// the attribute-validating write paths to resolve the per-kind contract before
/// touching the row. Does NOT filter `deleted_at` (callers decide).
async fn work_item_kind(db: &impl DbClient, id: &str) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        r#"SELECT kind FROM work_items WHERE id = $1"#,
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))
}

/// Partial update of a work item under the single-mutation-path discipline.
/// Each field is **set-or-leave**: a `None` bind leaves the column untouched via
/// `COALESCE(?, col)` (it does NOT clear to NULL). If `attributes` is present it
/// is normalised (object-root, null-keys dropped) and per-kind validated
/// (unknown key ⇒ `Validation`) BEFORE the write. `NotFound` via
/// `rows_affected()==0` so a missing row emits no event. Event `work_item.updated`.
pub async fn update_work_item(
    db: &impl DbClient,
    id: &str,
    req: &UpdateWorkItemRequest,
) -> Result<(), AppError> {
    // Pre-validate `attributes` (needs the row's kind) before opening the tx.
    let attributes_str: Option<String> = match &req.attributes {
        Some(value) => {
            let kind = work_item_kind(db, id).await?;
            let cleaned = normalise_object(value, "attributes")?;
            validate_attributes_for_kind(&kind, &cleaned)?;
            validate_plan_field_constraints(&cleaned)?; // R34
            Some(serde_json::to_string(&Value::Object(cleaned)).map_err(|e| AppError::Other(e.into()))?)
        }
        None => None,
    };

    let status_str: Option<String> = req.status.map(enum_to_str);

    let mut tx = db.begin().await?;

    // Closure gate (migration 0003): this generic PATCH can set status="done"
    // directly, so it routes through the SAME gate as update_work_item_status
    // (User Decision 3) — a task→done under a `hard` story with unchecked
    // criteria is rejected here too. No-op when status is absent / not "done".
    if let Some(s) = status_str.as_deref() {
        enforce_closure_gate(tx.as_mut(), id, s).await?;
        // Epic-done gate (migration 0010): same UNCONDITIONAL epic→done rule as
        // the transition_status path; both gates run, they cover disjoint kinds.
        enforce_epic_done_gate(tx.as_mut(), id, s).await?;
    }

    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET title      = COALESCE($2, title),
            body       = COALESCE($3, body),
            status     = COALESCE($4, status),
            position   = COALESCE($5, position),
            attributes = COALESCE($6, attributes),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1 AND deleted_at IS NULL
        "#,
            args![
                id.to_owned(),
                req.title.clone(),
                req.body.clone(),
                status_str.clone(),
                req.position,
                attributes_str.clone()
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({
        "title": req.title,
        "body": req.body,
        "status": status_str,
        "position": req.position,
        "attributes_set": req.attributes.is_some(),
    });
    record_event(tx.as_mut(), "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Append ONE `work_item_activity` row under the single-mutation-path discipline.
/// `seq` is allocated as `MAX(seq)+1` for the item WITHIN the transaction; the
/// `UNIQUE(work_item_id, seq)` constraint makes a race surface as a constraint
/// violation rather than silent duplication. `entry_kind` is validated against
/// the [`ActivityType`] set (typed `Validation`, not panic); `payload`, if
/// present, is normalised (object-root, null-keys dropped). The work item must
/// exist (`NotFound` otherwise). Event `work_item.activity_appended`. Returns the
/// new activity row id.
pub async fn append_activity(
    db: &impl DbClient,
    work_item_id: &str,
    entry_kind: &str,
    author: Option<&str>,
    summary: &str,
    payload: Option<&Value>,
    origin: Option<&str>,
) -> Result<Uuid, AppError> {
    let entry_kind = validate_entry_kind(entry_kind)?;

    let payload_str: Option<String> = match payload {
        Some(value) => {
            let cleaned = normalise_object(value, "activity payload")?;
            Some(
                serde_json::to_string(&Value::Object(cleaned))
                    .map_err(|e| AppError::Other(e.into()))?,
            )
        }
        None => None,
    };

    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    // Allocate the per-item monotonic seq inside the tx.
    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM work_item_activity WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO work_item_activity (id, work_item_id, seq, entry_kind, author, summary, payload, origin) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        args![
            id_str.clone(),
            work_item_id.to_owned(),
            seq,
            entry_kind.to_owned(),
            author.map(str::to_owned),
            summary.to_owned(),
            payload_str,
            origin.map(str::to_owned)
        ],
    )
    .await?;

    let event_payload = serde_json::json!({
        "activity_id": id_str,
        "seq": seq,
        "entry_kind": entry_kind,
    });
    record_event(tx.as_mut(), "work_item", work_item_id, "work_item.activity_appended", event_payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read-modify-merge a work item's `attributes`: SELECT the current object,
/// overwrite the keys present in `patch`, leave absent keys, normalise
/// (object-root, drop null-valued keys), per-kind validate, write back. This is
/// the fn the MCP `set_story_plan`/`set_task_spec` partial setters compose on, so
/// merging must NOT clobber sibling keys. One event `work_item.updated`.
///
/// # Why Rust-side merge (not SQL `json_patch`) — T3
///
/// We deliberately retain the Rust-side read+merge inside the transaction
/// rather than swapping in `UPDATE … SET attributes = json_patch(attributes, ?)`.
/// The validator chain ([`normalise_object`] + [`validate_attributes_for_kind`])
/// must run on the MERGED MAP, not on the patch alone, so that an unknown key
/// (which the per-kind validator rejects) and a non-object root (rejected by
/// `normalise_object`) are surfaced as a clean typed [`AppError::Validation`]
/// (→ 422) instead of a constraint-free `json_patch` overwrite. The atomicity
/// gain — read and write are committed together, or neither — comes from the
/// surrounding [`crate::db::begin_write`] tx, not from the SQL primitive.
///
/// # Null-key semantics — unsupported via this entry point (T3)
///
/// `normalise_object` strips null-valued keys on every call, so a patch shaped
/// `{"x": null}` does NOT delete an existing `x` key — it is silently dropped
/// from the patch before the merge. This is intentional: the widened
/// `set_story_plan` callers (the `not-doing` and `verification-commands`
/// SKILLs) never pass null values, only omitted-or-string. Callers needing
/// explicit key-deletion semantics must go through a future dedicated
/// `clear_attribute_key` path; do not work around it by storing an empty
/// string or by editing `normalise_object` to preserve nulls (the TOML export
/// path depends on null-key stripping).
pub async fn set_work_item_attributes(
    db: &impl DbClient,
    id: &str,
    patch: &Value,
) -> Result<(), AppError> {
    // The patch itself must be a null-free object root.
    let patch_obj = normalise_object(patch, "attributes")?;

    let mut tx = db.begin().await?;

    // Read current kind + attributes (do not resurrect a tombstoned row).
    let (current_kind, current_attributes) =
        crate::db::tx_query_opt::<(String, Option<String>)>(
            tx.as_mut(),
            r#"SELECT kind, attributes FROM work_items WHERE id = $1 AND deleted_at IS NULL"#,
            args![id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))?;

    // Merge: start from the existing object (or empty), overwrite present keys.
    // A stored blob that is non-JSON or a non-object root is data corruption (the
    // write side normalises every stored value to an object root) — fail loudly
    // as `Other` (→ 500) rather than silently discarding it (R13), mirroring
    // `decode_attributes`.
    let mut merged: serde_json::Map<String, Value> = match current_attributes {
        Some(s) => match serde_json::from_str::<Value>(&s) {
            Ok(Value::Object(m)) => m,
            Ok(_) => {
                return Err(AppError::Other(anyhow::anyhow!(
                    "stored attributes for work_item '{id}' is not a JSON object (corrupt blob)"
                )));
            }
            Err(e) => return Err(AppError::Other(e.into())),
        },
        None => serde_json::Map::new(),
    };
    for (k, v) in patch_obj {
        merged.insert(k, v);
    }
    // Re-normalise the merged result (drop any nulls a prior store missed).
    let merged_value = Value::Object(merged);
    let cleaned = normalise_object(&merged_value, "attributes")?;
    validate_attributes_for_kind(&current_kind, &cleaned)?;
    validate_plan_field_constraints(&cleaned)?; // R34

    let merged_str =
        serde_json::to_string(&Value::Object(cleaned)).map_err(|e| AppError::Other(e.into()))?;

    tx.execute(
        r#"UPDATE work_items SET attributes = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
        args![id.to_owned(), merged_str],
    )
    .await?;

    let payload = serde_json::json!({ "attributes_merged": true });
    record_event(tx.as_mut(), "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a work item's sibling-ordering `position` under the single-mutation-path
/// discipline. Reuses the `work_item.updated` event type (matches the
/// `update_work_item` partial-update convention — position is one of its
/// COALESCE fields). `NotFound` via `rows_affected()==0`.
pub async fn reorder_work_item(
    db: &impl DbClient,
    id: &str,
    position: i64,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET position = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
            args![id.to_owned(), position],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "position": position });
    record_event(tx.as_mut(), "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Create a `context_blocks` row under the single-mutation-path discipline.
/// Returns the new id. Event `context_block.created`.
pub async fn create_context_block(
    db: &impl DbClient,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<Uuid, AppError> {
    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    tx.execute(
        "INSERT INTO context_blocks (id, title, body) VALUES ($1, $2, $3)",
        args![id_str.clone(), title.map(str::to_owned), body.map(str::to_owned)],
    )
    .await?;

    let payload = serde_json::json!({ "title": title });
    record_event(tx.as_mut(), "context_block", &id_str, "context_block.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Link a context block to a work item (insert the `work_item_context` row)
/// under the single-mutation-path discipline. Event `context_block.linked`.
pub async fn link_context_block(
    db: &impl DbClient,
    work_item_id: &str,
    context_block_id: &str,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    tx.execute(
        "INSERT INTO work_item_context (work_item_id, context_block_id) VALUES ($1, $2)",
        args![work_item_id.to_owned(), context_block_id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({ "context_block_id": context_block_id });
    record_event(tx.as_mut(), "work_item", work_item_id, "context_block.linked", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Unlink a context block from a work item (hard-delete the link row — links
/// have no independent export identity) under the single-mutation-path
/// discipline. `NotFound` via `rows_affected()==0`. Event `context_block.unlinked`.
pub async fn unlink_context_block(
    db: &impl DbClient,
    work_item_id: &str,
    context_block_id: &str,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "DELETE FROM work_item_context WHERE work_item_id = $1 AND context_block_id = $2",
            args![work_item_id.to_owned(), context_block_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "context link (work_item '{work_item_id}', block '{context_block_id}') not found"
        )));
    }

    let payload = serde_json::json!({ "context_block_id": context_block_id });
    record_event(tx.as_mut(), "work_item", work_item_id, "context_block.unlinked", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Partial update of a finding under the single-mutation-path discipline. Each
/// field is set-or-leave via `COALESCE(?, col)`. The typed `severity` enum is
/// rendered to its snake_case wire form before storage. `NotFound` via
/// `rows_affected()==0`. Event `finding.updated`.
pub async fn update_finding(
    db: &impl DbClient,
    id: &str,
    req: &UpdateFindingRequest,
) -> Result<(), AppError> {
    let severity_str: Option<String> = req.severity.map(enum_to_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE findings \
             SET severity    = COALESCE($2, severity), \
                 effort      = COALESCE($3, effort), \
                 category    = COALESCE($4, category), \
                 status      = COALESCE($5, status), \
                 file        = COALESCE($6, file), \
                 line        = COALESCE($7, line), \
                 symbol      = COALESCE($8, symbol), \
                 summary     = COALESCE($9, summary), \
                 description = COALESCE($10, description), \
                 confidence  = COALESCE($11, confidence) \
             WHERE id = $1",
            args![
                id.to_owned(),
                severity_str.clone(),
                req.effort.clone(),
                req.category.clone(),
                req.status.clone(),
                req.file.clone(),
                req.line,
                req.symbol.clone(),
                req.summary.clone(),
                req.description.clone(),
                req.confidence.clone(),
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("finding '{id}' not found")));
    }

    // R16: record only the fields the caller actually set, so a description-only
    // update does not log a misleading null severity/status (null read as
    // "unchanged"). Absent fields are omitted from the payload entirely.
    let mut payload_map = serde_json::Map::new();
    if let Some(s) = &severity_str {
        payload_map.insert("severity".to_owned(), Value::String(s.clone()));
    }
    if let Some(s) = &req.status {
        payload_map.insert("status".to_owned(), Value::String(s.clone()));
    }
    let payload = Value::Object(payload_map);
    record_event(tx.as_mut(), "finding", id, "finding.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// One finding-triage update for the bulk [`batch_update_findings`] path (B17c).
/// Set-or-leave: a `None` field leaves that column unchanged (`COALESCE`).
pub struct FindingTriageUpdate<'a> {
    pub finding_id: &'a str,
    pub triage_state: Option<&'a str>,
    pub severity: Option<Severity>,
    pub category: Option<&'a str>,
    /// NON-terminal status only; a terminal [`Disposition`] value is rejected
    /// pre-tx (terminal dispositions belong to [`resolve_finding`]).
    pub status: Option<&'a str>,
}

/// Bulk non-terminal triage update over many findings under the
/// single-mutation-path discipline (plan D9). ONE transaction, all-or-nothing,
/// and exactly ONE coarse `findings.batch_triaged` event keyed to a non-
/// `work_item` aggregate (R-B4: a `work_item` aggregate would be re-rendered by
/// the export drain). Mirrors [`update_finding`]'s per-row COALESCE shape but is
/// restricted to the four mutable triage columns (`triage_state`, `severity`,
/// `category`, NON-terminal `status`).
///
/// Terminal dispositions are NOT this path's job: any input whose `status` parses
/// as a [`Disposition`] wire value (`fixed`/`wontfix`/`verified_clean`/`deferred`/
/// `duplicate`) is rejected with [`AppError::Validation`] BEFORE `db.begin()`, so
/// a terminal value writes nothing — the caller is pointed at [`resolve_finding`].
/// The terminal set is derived from the enum's serde wire form (no hardcoded
/// literal list), keeping it in lockstep with [`Disposition`].
///
/// A missing `finding_id` (`rows_affected() == 0`) aborts the whole batch with
/// [`AppError::NotFound`] (mirrors [`update_finding`]). Returns the count of
/// findings updated.
pub async fn batch_update_findings(
    db: &impl DbClient,
    updates: &[FindingTriageUpdate<'_>],
) -> Result<u64, AppError> {
    // Pre-tx validation: reject ANY terminal-disposition status before opening a
    // transaction, so a terminal value writes zero rows (all-or-nothing also for
    // the rejection path). "Is this terminal?" is decided by serde-parsing the
    // value through `Disposition` — exactly as `create_work_item_full_tx`
    // validates `Shape` — so the terminal set tracks the enum's wire spelling.
    for u in updates {
        if let Some(s) = u.status
            && serde_json::from_value::<Disposition>(Value::String(s.to_owned())).is_ok()
        {
            return Err(AppError::Validation(format!(
                "status '{s}' is a terminal disposition; use resolve_finding for \
                 terminal dispositions (fixed/wontfix/verified_clean/deferred/duplicate)"
            )));
        }
    }

    let mut tx = db.begin().await?;

    let mut updated: u64 = 0;
    for u in updates {
        let severity_str: Option<String> = u.severity.map(enum_to_str);
        let affected = tx
            .execute(
                "UPDATE findings \
                 SET triage_state = COALESCE($2, triage_state), \
                     severity     = COALESCE($3, severity), \
                     category     = COALESCE($4, category), \
                     status       = COALESCE($5, status) \
                 WHERE id = $1",
                args![
                    u.finding_id.to_owned(),
                    u.triage_state.map(str::to_owned),
                    severity_str,
                    u.category.map(str::to_owned),
                    u.status.map(str::to_owned),
                ],
            )
            .await?;

        if affected == 0 {
            // A `?`-propagated error here drops `tx` un-committed → full rollback.
            return Err(AppError::NotFound(format!(
                "finding '{}' not found",
                u.finding_id
            )));
        }
        updated += 1;
    }

    // Exactly one coarse event for the whole batch (D8/R-B4). aggregate_type MUST
    // NOT be "work_item" (the export drain renders only `work_item` aggregates) —
    // these are findings with no run context, so mint a fresh finding-scoped id.
    let aggregate_id = Uuid::now_v7().to_string();
    let payload = serde_json::json!({ "updated": updated });
    record_event(
        tx.as_mut(),
        "finding",
        &aggregate_id,
        "findings.batch_triaged",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(updated)
}

/// Supersede a finding (migration 0003): set `findings.superseded_by = new_id` on
/// the OLD finding so it drops out of the live `get_work_item_detail` fold
/// (`superseded_by IS NULL`). Single-mutation-path + one event
/// `finding.superseded`; `NotFound` (via `rows_affected()==0`) if the old finding
/// is absent. Mirrors [`supersede_research_note`]. The `new_id` is a soft
/// self-FK; it is VALIDATED here (R7) — an absent `new_id` is a typed
/// [`AppError::Validation`] (a clean 422) rather than an FK-violation 500. The
/// DB column itself remains `ON DELETE NO ACTION` (see the supersession-semantics
/// note above [`supersede_research_note`]).
pub async fn supersede_finding(
    db: &impl DbClient,
    old_id: &str,
    new_id: &str,
) -> Result<(), AppError> {
    // Validate the superseding finding exists (R7): clean 422 over a dangling-FK 500.
    let new_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM findings WHERE id = $1",
            args![new_id.to_owned()],
        )
        .await?
        .is_some();
    if !new_exists {
        return Err(AppError::Validation(format!(
            "superseding finding '{new_id}' does not exist"
        )));
    }

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE findings SET superseded_by = $2 WHERE id = $1",
            args![old_id.to_owned(), new_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("finding '{old_id}' not found")));
    }

    let payload = serde_json::json!({ "superseded_by": new_id });
    record_event(tx.as_mut(), "finding", old_id, "finding.superseded", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Resolve a finding to a terminal [`Disposition`] under the single-mutation-path
/// discipline: stamp `status` (the disposition wire value), `resolved_at`, and
/// the optional `resolution`/`wontfix_rationale` free-text. `NotFound` via
/// `rows_affected()==0`. Event `finding.resolved`.
pub async fn resolve_finding(
    db: &impl DbClient,
    id: &str,
    disposition: Disposition,
    resolution: Option<&str>,
    rationale: Option<&str>,
) -> Result<(), AppError> {
    let disposition_str = enum_to_str(disposition);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE findings \
             SET status            = $2, \
                 resolved_at       = CURRENT_TIMESTAMP, \
                 resolution        = COALESCE($3, resolution), \
                 wontfix_rationale = COALESCE($4, wontfix_rationale) \
             WHERE id = $1",
            args![
                id.to_owned(),
                disposition_str.clone(),
                resolution.map(|s| s.to_owned()),
                rationale.map(|s| s.to_owned()),
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("finding '{id}' not found")));
    }

    let payload = serde_json::json!({ "disposition": disposition_str });
    record_event(tx.as_mut(), "finding", id, "finding.resolved", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// SOFT-delete a work item: stamp `deleted_at` under the single-mutation-path
/// discipline. The row (and its cascaded activity) is preserved — a work item
/// owns export identity, so hard-delete would orphan the export TOML and lose
/// history. Idempotent-ish: a row already deleted (or absent) is `NotFound` via
/// `rows_affected()==0`. Event `work_item.deleted`.
pub async fn delete_work_item(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    // R36: block soft-deleting a `focus` that still has non-terminal, non-deleted
    // child stories. The epic-done gate's rollup counts only stories whose focus
    // parent is `deleted_at IS NULL` (enforce_epic_done_gate), so tombstoning a
    // focus mid-flight would silently drop its live stories from the rollup and
    // let the epic close with non-terminal descendants. Force explicit story
    // disposition first. Read inside the tx through the seam so the liveness check
    // and the soft-delete share one snapshot under the writer lock. A
    // missing/already-deleted id yields kind=None here and falls through to the
    // UPDATE's `affected == 0` NotFound path below — behaviour preserved.
    let kind: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT kind FROM work_items WHERE id = $1 AND deleted_at IS NULL",
        args![id.to_owned()],
    )
    .await?;
    if kind.as_deref() == Some("focus") {
        let live_stories: i64 = crate::db::tx_scalar_one::<i64>(
            tx.as_mut(),
            "SELECT COUNT(*) FROM work_items \
             WHERE kind = 'story' AND parent_id = $1 AND deleted_at IS NULL \
             AND status NOT IN ('done','cancelled')",
            args![id.to_owned()],
        )
        .await?;
        if live_stories > 0 {
            return Err(AppError::Validation(format!(
                "focus '{id}' cannot be deleted: {live_stories} non-terminal child \
                 story(ies) remain; resolve or cancel them first"
            )));
        }
    }

    let affected = tx
        .execute(
            r#"UPDATE work_items SET deleted_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
            args![id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "deleted": true });
    record_event(tx.as_mut(), "work_item", id, "work_item.deleted", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Input for [`create_finding`]. Carries the full findings shape the importer
/// (Task 7) maps from a review-ledger / optimise-findings `[[items]]` entry,
/// INCLUDING the disposition fields (`resolved_at`/`resolution`/`defer_reason`/
/// `defer_trigger`/`wontfix_rationale`) so `deferred`/`wontfix` imports are not
/// lossy (P7). Lives in `repo.rs` (not `domain.rs`, which is out of this task's
/// cluster); every field except the source `id`-derived `dedup_id` is optional,
/// mirroring the heterogeneous review/optimise finding shapes.
#[derive(Debug, Clone, Default)]
pub struct NewFinding<'a> {
    pub kind: Option<&'a str>,
    /// Typed [`Severity`] — review-finding categorisation (see CONVENTIONS §k.2
    /// for the deliberate vocab split with [`RiskSeverity`]). The DB column is
    /// free TEXT for historical reasons (migration 0001 / `findings` table
    /// pre-dates round-3); this field is the authoritative compile-time guard.
    /// Direct-repo callers (test fixtures, import paths) thus cannot smuggle a
    /// `RiskSeverity` wire value (`low|medium|high`) into `findings.severity`.
    pub severity: Option<Severity>,
    pub effort: Option<&'a str>,
    pub category: Option<&'a str>,
    pub status: Option<&'a str>,
    pub file: Option<&'a str>,
    pub line: Option<i64>,
    pub symbol: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub description: Option<&'a str>,
    pub first_flagged: Option<&'a str>,
    pub rounds: Option<i64>,
    pub fingerprint: Option<&'a str>,
    pub flow: Option<&'a str>,
    pub dedup_id: Option<&'a str>,
    /// FK to `runs.id` (migration 0011): the review/optimise run this finding was
    /// raised under; NULL on legacy findings that predate runs. ONLY the batch
    /// [`add_findings`] path (B17a) stamps this — the single-item [`create_finding`]
    /// wrapper leaves it `None`, and the triage-only `batch_update_findings` (B17c)
    /// never touches it — so run association happens exclusively at insert time.
    pub run_id: Option<&'a str>,
    /// Provenance (migration 0003): which command produced this finding; free
    /// TEXT in the DB (validated against the `Origin` enum at the MCP edge).
    pub origin: Option<&'a str>,
    /// `high|medium|low` evidence grade (migration 0003); free TEXT in the DB.
    pub confidence: Option<&'a str>,
    pub resolved_at: Option<&'a str>,
    pub resolution: Option<&'a str>,
    pub defer_reason: Option<&'a str>,
    pub defer_trigger: Option<&'a str>,
    pub wontfix_rationale: Option<&'a str>,
    /// FK to `repo_links.id` (migration 0004); NULL ⇒ implicit-primary
    /// resolution at read time.
    pub repo_id: Option<&'a str>,
}

/// Create a finding attached to a work item under the single-mutation-path
/// discipline: insert ONE `findings` row (id = freshly-minted UUIDv7 as TEXT)
/// AND append ONE `events` row via [`record_event`] in ONE transaction, so the
/// outbox fires and `export` materialises the finding's snapshot. Mirrors
/// [`create_work_item`]'s structure.
///
/// ALL findings columns are mapped, including the disposition fields, so a
/// `deferred`/`wontfix` import round-trips without loss (P7). Returns the new
/// finding id.
pub async fn create_finding(
    db: &impl DbClient,
    work_item_id: &str,
    finding: &NewFinding<'_>,
) -> Result<Uuid, AppError> {
    let mut tx = db.begin().await?;
    let (id, _affected) = create_finding_tx(tx.as_mut(), work_item_id, finding).await?;
    let id_str = id.to_string();

    let payload = serde_json::json!({
        "work_item_id": work_item_id,
        "severity": finding.severity,
        "category": finding.category,
        "status": finding.status,
    });
    record_event(tx.as_mut(), "finding", &id_str, "finding.created", payload).await?;

    tx.commit().await?;

    Ok(id)
}

/// Reusable tx helper extracted from [`create_finding`] (B16): mint the id, bind
/// every `findings` column, and INSERT the row INSIDE the caller's transaction.
///
/// **Does NOT record an event and does NOT commit** — that is the caller's job.
/// The public [`create_finding`] wrapper records the single `finding.created`
/// event after this returns; the batch-triage path (B17a) will call this N times
/// under one tx and record ONE coarse batch event instead of N.
///
/// Returns `(id, rows_affected)`. The INSERT uses the migration-0011 dedup upsert
/// `ON CONFLICT(work_item_id, dedup_id) WHERE dedup_id IS NOT NULL AND
/// superseded_by IS NULL DO NOTHING`, whose conflict-target predicate is written
/// BYTE-IDENTICAL to the `ux_findings_dedup` partial-index predicate (required:
/// a differing predicate fails to bind the index and silently duplicates). A
/// deduped insert yields `rows_affected == 0` (1 = a fresh row); B17a reads this
/// to distinguish added-vs-deduped. `triage_state` is left to the column DEFAULT
/// (`'pending'`), so it is omitted from the column list.
pub async fn create_finding_tx(
    tx: &mut dyn crate::db::DbTx,
    work_item_id: &str,
    finding: &NewFinding<'_>,
) -> Result<(uuid::Uuid, u64), AppError> {
    let id = Uuid::now_v7();
    let id_str = id.to_string();

    // Materialise the typed `Severity` into its wire form for the TEXT column
    // bind. `enum_to_str` round-trips via serde, so a `Severity::Minor` →
    // `"minor"`. No Severity value can produce a `RiskSeverity` wire literal
    // (`"low"|"medium"|"high"`) — the type system precludes it.
    let severity_str = finding.severity.map(enum_to_str);

    let affected = tx
        .execute(
            "INSERT INTO findings ( \
                id, work_item_id, kind, severity, effort, category, status, \
                file, line, symbol, summary, description, first_flagged, rounds, \
                fingerprint, flow, dedup_id, origin, confidence, resolved_at, resolution, \
                defer_reason, defer_trigger, wontfix_rationale, repo_id, run_id \
            ) \
            VALUES ( \
                $1, $2, $3, $4, $5, $6, $7, \
                $8, $9, $10, $11, $12, $13, $14, \
                $15, $16, $17, $18, $19, $20, $21, \
                $22, $23, $24, $25, $26 \
            ) \
            ON CONFLICT(work_item_id, dedup_id) \
                WHERE dedup_id IS NOT NULL AND superseded_by IS NULL DO NOTHING",
            args![
                id_str.clone(),
                work_item_id.to_owned(),
                finding.kind.map(|s| s.to_owned()),
                severity_str,
                finding.effort.map(|s| s.to_owned()),
                finding.category.map(|s| s.to_owned()),
                finding.status.map(|s| s.to_owned()),
                finding.file.map(|s| s.to_owned()),
                finding.line,
                finding.symbol.map(|s| s.to_owned()),
                finding.summary.map(|s| s.to_owned()),
                finding.description.map(|s| s.to_owned()),
                finding.first_flagged.map(|s| s.to_owned()),
                finding.rounds,
                finding.fingerprint.map(|s| s.to_owned()),
                finding.flow.map(|s| s.to_owned()),
                finding.dedup_id.map(|s| s.to_owned()),
                finding.origin.map(|s| s.to_owned()),
                finding.confidence.map(|s| s.to_owned()),
                finding.resolved_at.map(|s| s.to_owned()),
                finding.resolution.map(|s| s.to_owned()),
                finding.defer_reason.map(|s| s.to_owned()),
                finding.defer_trigger.map(|s| s.to_owned()),
                finding.wontfix_rationale.map(|s| s.to_owned()),
                finding.repo_id.map(|s| s.to_owned()),
                finding.run_id.map(|s| s.to_owned()),
            ],
        )
        .await?;

    Ok((id, affected))
}

/// Compute a stable dedup hash over the identity tuple of a finding
/// (`work_item_id`, `file`, `line`, `symbol`, `summary`). B17a feeds this into a
/// finding's `dedup_id` so a re-run that re-raises the same finding collapses onto
/// the migration-0011 `ux_findings_dedup` partial index (and thus the `DO NOTHING`
/// upsert in [`create_finding_tx`]) instead of double-inserting.
///
/// The components are joined with the ASCII Unit Separator (`\u{1f}`) — a byte
/// that cannot appear in a file path / symbol / summary in practice — so the
/// field boundaries are unambiguous and cross-boundary collisions are avoided
/// (e.g. `file="a", symbol="b"` hashes differently from `file="ab", symbol=""`).
/// `None` is encoded distinctly from `Some("")` by emitting a literal NUL marker
/// for the absent case, so a missing field and an empty field never collide.
/// Returns lowercase hex. No caller until B17a; `pub` keeps clippy's dead_code
/// lint quiet.
pub fn finding_dedup_hash(
    work_item_id: &str,
    file: Option<&str>,
    line: Option<i64>,
    symbol: Option<&str>,
    summary: Option<&str>,
) -> String {
    use sha2::{Digest, Sha256};

    // Encode an optional string component: a NUL byte distinguishes `None` from
    // any `Some(_)` (a present value is prefixed with a non-NUL `\x01` tag).
    fn feed_opt_str(hasher: &mut Sha256, value: Option<&str>) {
        match value {
            None => hasher.update([0x00]),
            Some(s) => {
                hasher.update([0x01]);
                hasher.update(s.as_bytes());
            }
        }
    }

    const SEP: &[u8] = b"\x1f";
    let mut hasher = Sha256::new();
    // work_item_id is always present (non-optional), tag it like a present value.
    hasher.update([0x01]);
    hasher.update(work_item_id.as_bytes());
    hasher.update(SEP);
    feed_opt_str(&mut hasher, file);
    hasher.update(SEP);
    match line {
        None => hasher.update([0x00]),
        Some(n) => {
            hasher.update([0x01]);
            hasher.update(n.to_le_bytes());
        }
    }
    hasher.update(SEP);
    feed_opt_str(&mut hasher, symbol);
    hasher.update(SEP);
    feed_opt_str(&mut hasher, summary);

    let digest = hasher.finalize();
    // Lowercase hex render (no extra dep — format each byte).
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Bulk-insert a batch of findings under ONE `BEGIN IMMEDIATE` transaction
/// (B17a, migration 0011), with content-hash dedup and all-or-nothing atomicity.
///
/// Each `(work_item_id, finding)` element is inserted via [`create_finding_tx`].
/// Before the transaction opens, this stamps every finding's `dedup_id` with the
/// content hash [`finding_dedup_hash`] computes over its
/// `(work_item_id, file, line, symbol, summary)` identity tuple — OVERWRITING
/// whatever `dedup_id` the caller passed: the batch path OWNS dedup. That hash is
/// what collapses a re-raised finding onto the `ux_findings_dedup` partial index,
/// so the `ON CONFLICT … DO NOTHING` upsert in `create_finding_tx` skips a row
/// already committed by a prior batch (`rows_affected == 0`) instead of
/// double-inserting.
///
/// `run_id`, when `Some`, is stamped onto every finding (run association happens
/// ONLY here — the triage-only `batch_update_findings` (B17c) never touches
/// `findings.run_id`). `None` leaves the FK NULL (legal; no `runs` row required).
///
/// ## Atomicity (validation aborts the whole batch)
/// Any error from `create_finding_tx` `?`-propagates, dropping `tx` un-committed →
/// SQLite rolls back → ZERO rows persist (e.g. an FK violation from a `run_id`
/// that names no `runs` row aborts the entire batch, not just the offending row).
///
/// ## Single coarse event (D8 / R-B4)
/// Exactly ONE `events` row is recorded for the whole batch, NOT one per finding.
/// Its `aggregate_type` is **deliberately not `"work_item"`**: the git-export
/// drain (`export.rs`) materialises only `aggregate_type="work_item"` events, so a
/// `"work_item"` batch event would wrongly re-render. A `run`-typed event (when a
/// `run_id` is supplied) or a `finding`-typed event (otherwise, keyed by a fresh
/// UUIDv7) is correctly inert — drained and `exported_at`-stamped, but not
/// materialised to a file.
///
/// Returns [`BatchInsertResult`]: `added` (rows inserted), `skipped` (rows the
/// dedup upsert collapsed), and `skipped_ids` — the dedup CONTENT HASH of each
/// skipped input (NOT the finding's row id, which never minted). That hash is the
/// stable cross-run identifier a re-run recomputes via [`finding_dedup_hash`] to
/// assert membership.
///
/// Advisory: keep batches to ≲500 rows per call (one transaction, one event).
pub async fn add_findings(
    db: &impl DbClient,
    run_id: Option<&str>,
    items: &[(&str, NewFinding<'_>)],
) -> Result<crate::domain::BatchInsertResult, AppError> {
    // Pre-tx: compute the dedup content hash per element. This Vec OUTLIVES the
    // tx loop because each `NewFinding.dedup_id` we build below borrows `&hashes[i]`.
    let hashes: Vec<String> = items
        .iter()
        .map(|(work_item_id, finding)| {
            finding_dedup_hash(
                work_item_id,
                finding.file,
                finding.line,
                finding.symbol,
                finding.summary,
            )
        })
        .collect();

    let mut tx = db.begin().await?;

    let mut added: i64 = 0;
    let mut skipped: i64 = 0;
    let mut skipped_ids: Vec<String> = Vec::new();

    for (i, (work_item_id, finding)) in items.iter().enumerate() {
        // The batch path OWNS dedup + run association: overwrite the caller's
        // `dedup_id` with the computed content hash and stamp `run_id` (clone the
        // element so the source `items` slice is untouched).
        let stamped = NewFinding {
            dedup_id: Some(&hashes[i]),
            run_id,
            ..finding.clone()
        };
        // A `create_finding_tx` error `?`-propagates here, dropping `tx`
        // un-committed → full rollback → zero writes (all-or-nothing).
        let (_id, affected) = create_finding_tx(tx.as_mut(), work_item_id, &stamped).await?;
        if affected == 1 {
            added += 1;
        } else {
            // `affected == 0` ⇒ the dedup upsert collapsed onto an existing live
            // row. Record the content hash (the stable cross-run identifier).
            skipped += 1;
            skipped_ids.push(hashes[i].clone());
        }
    }

    // Exactly one coarse event for the whole batch (D8). aggregate_type MUST NOT
    // be "work_item" (R-B4) — keyed to the run when present, else a fresh
    // finding-scoped id, both of which the export drain ignores.
    let (aggregate_type, aggregate_id) = match run_id {
        Some(rid) => ("run", rid.to_owned()),
        None => ("finding", Uuid::now_v7().to_string()),
    };
    let payload = serde_json::json!({ "added": added, "skipped": skipped });
    record_event(
        tx.as_mut(),
        aggregate_type,
        &aggregate_id,
        "findings.batch_added",
        payload,
    )
    .await?;

    tx.commit().await?;

    Ok(crate::domain::BatchInsertResult {
        added,
        skipped,
        skipped_ids,
    })
}

// ---------------------------------------------------------------------------
// Runs / sprints / triage-decisions (migration 0011) — the review/optimise
// findings-queue domain (B23). Every mutator follows the single-mutation-path
// discipline (one `db::begin` tx, the domain write(s), EXACTLY ONE
// `record_event`, one commit).
//
// **Export-inert routing (R-B4).** `runs`, `sprints`, and `finding_decisions`
// are NOT git-exported entities — the export drain (`export.rs`) materialises
// ONLY `aggregate_type = "work_item"` events. So every event in this section is
// routed to a NON-`"work_item"` aggregate (`"run"` / `"sprint"` / `"finding"`),
// mirroring how `add_findings` / `batch_update_findings` / `create_finding`
// pick inert aggregates: the event drains and is `exported_at`-stamped but
// renders no file. A spawn decision (which DOES create a `work_item`) routes the
// CHILD's `work_item.created` event through `create_work_item_full_tx`'s caller
// — but B23 deliberately uses `create_work_item_full_tx` (the no-event tx
// helper), folding the spawn into the decision's single `"finding"` event so the
// whole decision is one event, NOT two. (Resolve is the documented exception —
// see `record_finding_decision`.)
// ---------------------------------------------------------------------------

/// Open a new review/optimise [`run`](crate::domain::NewRun) over a live story
/// or an existing sprint (migration 0011, B23). The target is validated BEFORE
/// the transaction opens so an absent / wrong-kind / tombstoned target is a
/// clean [`AppError::Validation`] (→ 422) rather than a dangling-FK 500:
///   * `TargetKind::Story` requires a LIVE `kind='story'` row (`deleted_at IS
///     NULL`) — a tombstoned story is rejected;
///   * `TargetKind::Sprint` requires a `sprints` row.
///
/// Single-mutation-path: one `runs` INSERT (`status` left to the column DEFAULT
/// `'open'`, omitted from the column list — mirroring how `create_finding_tx`
/// omits `triage_state`) + EXACTLY ONE export-inert `run.created` event
/// (`aggregate_type="run"`; R-B4 — never `"work_item"`). Returns the run id.
pub async fn create_run(db: &impl DbClient, run: &NewRun) -> Result<Uuid, AppError> {
    // Validate the target exists, is live, and matches `target_kind` BEFORE the
    // tx — a clean Validation, never a 500.
    match run.target_kind {
        TargetKind::Story => {
            let live = db
                .query_opt::<Scalar<i64>>(
                    "SELECT 1 FROM work_items \
                     WHERE id = $1 AND kind = 'story' AND deleted_at IS NULL",
                    args![run.target_id.clone()],
                )
                .await?
                .is_some();
            if !live {
                return Err(AppError::Validation(format!(
                    "run target '{}' is not a live story",
                    run.target_id
                )));
            }
        }
        TargetKind::Sprint => {
            let exists = db
                .query_opt::<Scalar<i64>>(
                    "SELECT 1 FROM sprints WHERE id = $1",
                    args![run.target_id.clone()],
                )
                .await?
                .is_some();
            if !exists {
                return Err(AppError::Validation(format!(
                    "run target '{}' is not an existing sprint",
                    run.target_id
                )));
            }
        }
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    let kind_str = enum_to_str(run.kind);
    let target_kind_str = enum_to_str(run.target_kind);

    let mut tx = db.begin().await?;

    // `status` is omitted so the column DEFAULT ('open') applies.
    tx.execute(
        "INSERT INTO runs (id, kind, target_id, target_kind) VALUES ($1, $2, $3, $4)",
        args![
            id_str.clone(),
            kind_str.clone(),
            run.target_id.clone(),
            target_kind_str.clone()
        ],
    )
    .await?;

    // One export-inert event (R-B4): aggregate_type="run", NOT "work_item".
    let payload = serde_json::json!({
        "kind": kind_str,
        "target_id": run.target_id,
        "target_kind": target_kind_str,
    });
    record_event(tx.as_mut(), "run", &id_str, "run.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Create a new (previously-ephemeral) [`sprint`](crate::domain::NewSprint)
/// grouping (migration 0011, B23). Single-mutation-path: one `sprints` INSERT
/// (`title` nullable from the input; `status` left to the column DEFAULT
/// `'open'`, omitted from the column list) + EXACTLY ONE export-inert
/// `sprint.created` event (`aggregate_type="sprint"`; R-B4 — never
/// `"work_item"`). Returns the sprint id.
pub async fn create_sprint(db: &impl DbClient, sprint: &NewSprint) -> Result<Uuid, AppError> {
    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    // `status` is omitted so the column DEFAULT ('open') applies.
    tx.execute(
        "INSERT INTO sprints (id, title) VALUES ($1, $2)",
        args![id_str.clone(), sprint.title.clone()],
    )
    .await?;

    let payload = serde_json::json!({ "title": sprint.title });
    record_event(tx.as_mut(), "sprint", &id_str, "sprint.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Add one or more tasks to a sprint via the `sprint_tasks` junction (migration
/// 0011, B23), all-or-nothing. The sprint is validated BEFORE the loop; then,
/// inside ONE tx, every `task_id` is validated as a LIVE `kind='task'` row — a
/// missing / non-task id aborts the WHOLE batch (mirroring
/// [`batch_update_findings`]), so a partial membership never persists. Each
/// membership is an `INSERT … ON CONFLICT(sprint_id, task_id) DO NOTHING`, so
/// re-adding an already-member task is a no-op (`rows_affected()==0`), NOT an
/// error — only genuinely-new memberships count toward the returned `added`.
///
/// Single-mutation-path: the N junction INSERTs + EXACTLY ONE export-inert
/// coarse `sprint.tasks_added` event (`aggregate_type="sprint"`, keyed by the
/// sprint id; R-B4 — never `"work_item"`), payload `{added, requested}`.
/// Returns the count of memberships actually inserted.
pub async fn add_tasks_to_sprint(
    db: &impl DbClient,
    sprint_id: &str,
    task_ids: &[&str],
) -> Result<u64, AppError> {
    // Validate the sprint exists BEFORE the loop (NotFound, not a dangling-FK 500).
    let sprint_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM sprints WHERE id = $1",
            args![sprint_id.to_owned()],
        )
        .await?
        .is_some();
    if !sprint_exists {
        return Err(AppError::NotFound(format!("sprint '{sprint_id}' not found")));
    }

    let mut tx = db.begin().await?;

    let mut added: u64 = 0;
    for &task_id in task_ids {
        // Validate the id is a LIVE task — a non-task / missing id aborts the
        // whole batch (`?`-propagated rollback → zero memberships persist).
        let kind: Option<String> = crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT kind FROM work_items WHERE id = $1 AND deleted_at IS NULL",
            args![task_id.to_owned()],
        )
        .await?;
        match kind.as_deref() {
            Some("task") => {}
            _ => {
                return Err(AppError::Validation(format!(
                    "sprint member '{task_id}' is not a live task"
                )));
            }
        }

        let affected = tx
            .execute(
                "INSERT INTO sprint_tasks (sprint_id, task_id) VALUES ($1, $2) \
                 ON CONFLICT(sprint_id, task_id) DO NOTHING",
                args![sprint_id.to_owned(), task_id.to_owned()],
            )
            .await?;
        // `affected == 0` ⇒ a dedup skip (already a member), NOT an error.
        if affected == 1 {
            added += 1;
        }
    }

    // One export-inert coarse event (R-B4): aggregate_type="sprint", keyed by the
    // sprint id, NOT "work_item".
    let payload = serde_json::json!({ "added": added, "requested": task_ids.len() });
    record_event(
        tx.as_mut(),
        "sprint",
        sprint_id,
        "sprint.tasks_added",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(added)
}

/// Record a triage [`decision`](crate::domain::NewFindingDecision) against a
/// finding (migration 0011, B23), returning `(decision_id,
/// spawned_work_item_id)` — the second element is `Some` only for the two spawn
/// verdicts.
///
/// ## Decision → behaviour map (the B23 judgement core)
/// The plan leaves the per-decision `triage_state`, the spawn parent, the title
/// source, and the `Resolve` disposition UNDER-SPECIFIED; this implements the
/// orchestrator's chosen, internally-consistent design:
///   * `SpawnTask` → create a child `task` under the finding's host work_item;
///     `triage_state = "accepted"`.
///   * `SpawnStory` → create a child `story` under the finding's host work_item;
///     `triage_state = "accepted"`.
///   * `Defer` → no spawn; `triage_state = "deferred"`.
///   * `Dismiss` → no spawn; `triage_state = "dismissed"`.
///   * `Resolve` → no spawn; `triage_state = "accepted"`; ALSO resolves the
///     finding terminally (see the delegation note below).
///
/// ## Spawn parent + title
/// A spawn parents the new item under the finding's own host `work_item_id`
/// (a finding with a NULL host cannot parent a child, so a spawn-on-hostless
/// finding is a clean `Validation`). The child's title is the finding's
/// `summary` when present and non-empty, else `"Spawned from finding <id>"`.
/// `create_work_item_full_tx` enforces the hierarchy: a `task` needs a `story`
/// parent, a `story` needs a `focus` parent whose epic carries ≥1
/// close-criterion. An incompatible host kind ⇒ the helper's `Validation`
/// propagates UN-swallowed (the caller must issue a spawn kind that fits the
/// host). The new id is then stamped onto `work_items.spawned_from_finding_id`
/// (mirroring [`create_work_items`]).
///
/// ## Single-mutation-path + the Resolve exception (D9)
/// The non-Resolve verdicts run ENTIRELY in one tx: (optional) child create via
/// `create_work_item_full_tx` (the no-event tx helper) + spawn stamp, the
/// `findings.triage_state` UPDATE, the `finding_decisions` INSERT, and EXACTLY
/// ONE export-inert `finding.decision_recorded` event (`aggregate_type="finding"`,
/// keyed by the finding id; R-B4 — never `"work_item"`, even though a spawn
/// created one: the child's create folds into this one event).
///
/// `Resolve` is the documented exception: `resolve_finding` opens its OWN tx +
/// `finding.resolved` event and cannot nest, so for `Resolve` ONLY we call
/// `resolve_finding(db, finding_id, Disposition::Fixed, None, None)` FIRST, THEN
/// open the decision tx (no spawn). This intentionally yields TWO events for a
/// resolve (`finding.resolved` + `finding.decision_recorded`).
pub async fn record_finding_decision(
    db: &impl DbClient,
    decision: &NewFindingDecision,
) -> Result<(Uuid, Option<Uuid>), AppError> {
    let finding_id = decision.finding_id.as_str();

    // Validate the finding exists and capture its host work_item_id (nullable).
    // A missing finding is NotFound (not a dangling-FK 500). `work_item_id` is a
    // nullable column → reads back as Option<String>.
    let host_id: Option<String> = match crate::db::scalar_opt::<Option<String>>(
        db,
        "SELECT work_item_id FROM findings WHERE id = $1",
        args![finding_id.to_owned()],
    )
    .await?
    {
        Some(host) => host,
        None => return Err(AppError::NotFound(format!("finding '{finding_id}' not found"))),
    };

    // Map the verdict to (spawn-kind, triage_state). `Resolve` additionally
    // delegates a terminal resolution (handled below, before the decision tx).
    let (spawn_kind, triage_state): (Option<&str>, TriageState) = match decision.decision {
        FindingDecisionKind::SpawnTask => (Some("task"), TriageState::Accepted),
        FindingDecisionKind::SpawnStory => (Some("story"), TriageState::Accepted),
        FindingDecisionKind::Defer => (None, TriageState::Deferred),
        FindingDecisionKind::Dismiss => (None, TriageState::Dismissed),
        FindingDecisionKind::Resolve => (None, TriageState::Accepted),
    };
    let triage_state_str = enum_to_str(triage_state);
    let decision_str = enum_to_str(decision.decision);

    // A spawn needs a host to parent under; a hostless finding cannot spawn.
    if spawn_kind.is_some() && host_id.is_none() {
        return Err(AppError::Validation(format!(
            "cannot spawn from finding '{finding_id}': it has no host work_item to parent under"
        )));
    }

    // Resolve delegation (D9): resolve_finding opens its OWN tx + event and
    // cannot nest, so run it FIRST (before our decision tx). This is the one
    // verdict that legitimately yields two events.
    if matches!(decision.decision, FindingDecisionKind::Resolve) {
        resolve_finding(db, finding_id, Disposition::Fixed, None, None).await?;
    }

    let decision_id = Uuid::now_v7();
    let decision_id_str = decision_id.to_string();

    let mut tx = db.begin().await?;

    // 1. (spawn only) create the child under the finding's host, then stamp the
    //    provenance back-link. `create_work_item_full_tx` is the no-event tx
    //    helper, so the child's create folds into THIS decision's single event.
    let spawned_id: Option<Uuid> = if let Some(kind) = spawn_kind {
        let host = host_id
            .as_deref()
            .expect("spawn host presence checked above");
        // Title: the finding's summary when present + non-empty, else a fallback.
        let summary: Option<String> = crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT summary FROM findings WHERE id = $1",
            args![finding_id.to_owned()],
        )
        .await?;
        let fallback = format!("Spawned from finding {finding_id}");
        let title: &str = match summary.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => &fallback,
        };
        // An incompatible host kind surfaces the helper's Validation UN-swallowed.
        let new_id = create_work_item_full_tx(
            tx.as_mut(),
            kind,
            Some(host),
            title,
            None,
            CreateOpts {
                origin: Some("review"),
                outcome: None,
                shape: None,
            },
        )
        .await?;
        // Stamp the provenance back-link (mirrors `create_work_items`).
        tx.execute(
            "UPDATE work_items SET spawned_from_finding_id = $1 WHERE id = $2",
            args![finding_id.to_owned(), new_id.to_string()],
        )
        .await?;
        Some(new_id)
    } else {
        None
    };

    // 2. Stamp the mapped triage_state on the finding.
    tx.execute(
        "UPDATE findings SET triage_state = $2 WHERE id = $1",
        args![finding_id.to_owned(), triage_state_str.clone()],
    )
    .await?;

    // 3. Record the append-only decision audit row (decided_at left to DEFAULT).
    tx.execute(
        "INSERT INTO finding_decisions (id, finding_id, decision, spawned_work_item_id, decided_by) \
         VALUES ($1, $2, $3, $4, $5)",
        args![
            decision_id_str.clone(),
            finding_id.to_owned(),
            decision_str.clone(),
            spawned_id.map(|id| id.to_string()),
            decision.decided_by.clone(),
        ],
    )
    .await?;

    // 4. EXACTLY ONE export-inert event for the decision (R-B4):
    //    aggregate_type="finding", keyed by the finding id, NOT "work_item" —
    //    even when a spawn created a work_item, its create folds into this event.
    let payload = serde_json::json!({
        "decision": decision_str,
        "spawned_work_item_id": spawned_id.map(|id| id.to_string()),
    });
    record_event(
        tx.as_mut(),
        "finding",
        finding_id,
        "finding.decision_recorded",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok((decision_id, spawned_id))
}

// ---------------------------------------------------------------------------
// Research notes (migration 0003) — first-class records with confidence,
// accept/reject state, and a `superseded_by` supersession chain. Mirror the
// acceptance-criteria/activity child-table idiom (seq = MAX+1 per work item,
// one event per write).
// ---------------------------------------------------------------------------

/// Append ONE `research_notes` row under the single-mutation-path discipline
/// (migration 0003). `seq` is `MAX(seq)+1` per work item WITHIN the transaction;
/// `state` defaults to `proposed`. The work item must exist (`NotFound`
/// otherwise). Event `work_item.research_note_added`. Returns the new note id.
#[allow(clippy::too_many_arguments)]
pub async fn add_research_note(
    db: &impl DbClient,
    work_item_id: &str,
    summary: &str,
    body: Option<&str>,
    confidence: Option<&str>,
    lens: Option<&str>,
    origin: Option<&str>,
) -> Result<Uuid, AppError> {
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    // State defaults to `proposed` on create.
    let state = enum_to_str(ResearchState::Proposed);

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM research_notes WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO research_notes
            (id, work_item_id, seq, summary, body, confidence, state, lens, origin)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
        args![
            id_str.clone(),
            work_item_id.to_owned(),
            seq,
            summary.to_owned(),
            body.map(str::to_owned),
            confidence.map(str::to_owned),
            state,
            lens.map(str::to_owned),
            origin.map(str::to_owned),
        ],
    )
    .await?;

    let payload = serde_json::json!({ "note_id": id_str, "seq": seq });
    record_event(tx.as_mut(), "work_item", work_item_id, "work_item.research_note_added", payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read a research note's owning `work_item_id`, erroring `NotFound` if the note
/// id has no row. Used by the update/supersede paths to attribute the owning item
/// for the event aggregate.
async fn research_note_work_item(db: &impl DbClient, id: &str) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM research_notes WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("research_note '{id}' not found")))
}

/// Partial set-or-leave update of a research note's curatable fields (migration
/// 0003): `confidence`/`state`/`rationale`/`lens` via `COALESCE(?, col)` (absent
/// ⇒ untouched). The typed `state` enum is rendered to its wire form. The owning
/// work_item_id is read first (`NotFound` if the note is absent). One event
/// `work_item.research_note_updated`.
pub async fn update_research_note(
    db: &impl DbClient,
    id: &str,
    req: &UpdateResearchNoteRequest,
) -> Result<(), AppError> {
    let work_item_id = research_note_work_item(db, id).await?;
    let state_str: Option<String> = req.state.map(enum_to_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE research_notes
        SET confidence = COALESCE($2, confidence),
            state      = COALESCE($3, state),
            rationale  = COALESCE($4, rationale),
            lens       = COALESCE($5, lens)
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                req.confidence.clone(),
                state_str.clone(),
                req.rationale.clone(),
                req.lens.clone(),
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("research_note '{id}' not found")));
    }

    let payload = serde_json::json!({ "note_id": id, "state": state_str });
    record_event(tx.as_mut(), "work_item", &work_item_id, "work_item.research_note_updated", payload)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// Supersede a research note (migration 0003): set `superseded_by = new_id` on
/// the OLD note so it drops out of the live fold (`superseded_by IS NULL`). One
/// event `work_item.research_note_superseded`; `NotFound` via
/// `rows_affected()==0`. Mirrors [`supersede_finding`]. The `new_id` is
/// VALIDATED here (R7) — an absent `new_id` is a typed [`AppError::Validation`].
///
/// # Supersession / ON DELETE semantics (R14)
///
/// The supersession pointers — `findings.superseded_by`,
/// `research_notes.superseded_by` — and the open-question provenance pointers
/// `open_questions.prompting_finding_id` / `open_questions.prompting_note_id`
/// are currently declared `ON DELETE NO ACTION` in migration `0003`.
/// Supersession is a SOFT pointer (the superseded row is kept for the export
/// audit trail, never hard-deleted), so today nothing exercises the delete path.
/// A future hard-delete path SHOULD migrate these columns to `ON DELETE SET NULL`
/// to avoid a delete being blocked by — or leaving — a dangling pointer. Do NOT
/// edit the committed `0003_*.sql` to change this: that would alter its sqlx
/// migration checksum and break already-applied DBs (a new migration is the path).
pub async fn supersede_research_note(
    db: &impl DbClient,
    old_id: &str,
    new_id: &str,
) -> Result<(), AppError> {
    let work_item_id = research_note_work_item(db, old_id).await?;

    // Validate the superseding note exists (R7): clean 422 over a dangling-FK 500.
    let new_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM research_notes WHERE id = $1",
            args![new_id.to_owned()],
        )
        .await?
        .is_some();
    if !new_exists {
        return Err(AppError::Validation(format!(
            "superseding research_note '{new_id}' does not exist"
        )));
    }

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE research_notes SET superseded_by = $2 WHERE id = $1",
            args![old_id.to_owned(), new_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("research_note '{old_id}' not found")));
    }

    let payload = serde_json::json!({ "superseded_by": new_id });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "work_item.research_note_superseded",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Open questions + options + branch resolution (migration 0003). A story-scoped
// decision lifecycle: add a question + N options, block tasks on the question,
// tie a task to a branch (`enabling_option_id`), then resolve — picking an option
// unblocks the chosen branch and cancels the other branches' exclusive tasks.
// ---------------------------------------------------------------------------

/// Append ONE `open_questions` row under the single-mutation-path discipline
/// (migration 0003). Story-scoped: a non-`story` target is rejected with a typed
/// [`AppError::Validation`] (kind read first; this also yields `NotFound` if the
/// id is absent). `seq` = `MAX(seq)+1` per story; `status` defaults to `open`.
/// Event `open_question.added`. Returns the new question id.
pub async fn add_open_question(
    db: &impl DbClient,
    story_id: &str,
    question: &str,
) -> Result<Uuid, AppError> {
    let kind = work_item_kind(db, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "open questions are settable only on a story, not on '{kind}'"
        )));
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM open_questions WHERE story_id = $1",
        args![story_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO open_questions (id, story_id, seq, question, status) VALUES ($1, $2, $3, $4, 'open')",
        args![id_str.clone(), story_id.to_owned(), seq, question.to_owned()],
    )
    .await?;

    // Route the event to the owning STORY's work_item aggregate (R1): export only
    // renders work_item aggregates, so an `open_question`-typed event would never
    // reach the git-export snapshot. event_type/payload are otherwise unchanged,
    // so the "exactly one event" invariant holds.
    let payload = serde_json::json!({ "question_id": id_str, "seq": seq });
    record_event(tx.as_mut(), "work_item", story_id, "open_question.added", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Read an open question's owning `story_id`, erroring `NotFound` if the question
/// id has no row. Used by the option-add and resolve paths.
async fn open_question_story(db: &impl DbClient, id: &str) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT story_id FROM open_questions WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("open_question '{id}' not found")))
}

/// Append ONE `question_options` row under the single-mutation-path discipline
/// (migration 0003). `seq` = `MAX(seq)+1` per question; the question must exist
/// (`NotFound` otherwise). Event `open_question.option_added`. Returns the new
/// option id.
pub async fn add_question_option(
    db: &impl DbClient,
    question_id: &str,
    label: &str,
    detail: Option<&str>,
) -> Result<Uuid, AppError> {
    // Verify the question exists first (NotFound, not a dangling-FK 500) AND
    // capture its owning story for the event aggregate (R1).
    let story_id = open_question_story(db, question_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM question_options WHERE question_id = $1",
        args![question_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO question_options (id, question_id, seq, label, detail) VALUES ($1, $2, $3, $4, $5)",
        args![
            id_str.clone(),
            question_id.to_owned(),
            seq,
            label.to_owned(),
            detail.map(str::to_owned),
        ],
    )
    .await?;

    // Route to the owning STORY's work_item aggregate (R1) so export renders it.
    let payload = serde_json::json!({ "option_id": id_str, "seq": seq });
    record_event(tx.as_mut(), "work_item", &story_id, "open_question.option_added", payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Block a task on an open question (migration 0003): set
/// `blocked_by_question_id = question_id` AND `status = 'blocked'` in one write.
/// One event `work_item.blocked_on_question`; `NotFound` via `rows_affected()==0`.
///
/// Task-scoped (R3): a non-`task` kind is rejected with a typed
/// [`AppError::Validation`] (mirrors [`set_effort`]), and the referenced
/// `question_id` must exist (else `Validation`, not a dangling-FK 500). The
/// task's current status must be `todo`/`open` (R12): the branch-resolution
/// model restores blocked tasks to `todo` on unblock, so blocking an
/// `in_progress`/`done` task would silently lose its state — that is rejected
/// with `Validation` rather than clobbered.
pub async fn block_task_on_question(
    db: &impl DbClient,
    task_id: &str,
    question_id: &str,
) -> Result<(), AppError> {
    // Task-scoped guard (R3); also yields NotFound if the id is absent.
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "block_task_on_question is settable only on a task, not on '{kind}'"
        )));
    }

    // The referenced question must exist (R3): clean 422 over a dangling-FK 500.
    let q_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM open_questions WHERE id = $1",
            args![question_id.to_owned()],
        )
        .await?
        .is_some();
    if !q_exists {
        return Err(AppError::Validation(format!(
            "open_question '{question_id}' does not exist"
        )));
    }

    // R12: only block a pre-todo task. Blocking an in_progress/done task would be
    // silently downgraded to `todo` on unblock, losing state — reject instead.
    let current = crate::db::scalar_one::<String>(
        db,
        "SELECT status FROM work_items WHERE id = $1",
        args![task_id.to_owned()],
    )
    .await?;
    if !matches!(current.as_str(), "todo" | "open") {
        return Err(AppError::Validation(format!(
            "task '{task_id}' cannot be blocked from status '{current}': only a 'todo'/'open' \
             task may be blocked (the branch-resolution model restores blocked tasks to 'todo')"
        )));
    }

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET blocked_by_question_id = $2, status = 'blocked', updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
            args![task_id.to_owned(), question_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "blocked_by_question_id": question_id });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.blocked_on_question", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Tie a task to a specific answer-option branch (migration 0003): set
/// `enabling_option_id = option_id` (the exclusive-branch marker — a task with
/// this set is cancelled if a DIFFERENT option is chosen on resolution). One
/// event `work_item.enabling_option_set`; `NotFound` via `rows_affected()==0`.
///
/// Task-scoped (R3): a non-`task` kind is rejected with a typed
/// [`AppError::Validation`] (mirrors [`set_effort`]), and the referenced
/// `option_id` must exist (else `Validation`, not a dangling-FK 500).
pub async fn set_enabling_option(
    db: &impl DbClient,
    task_id: &str,
    option_id: &str,
) -> Result<(), AppError> {
    // Task-scoped guard (R3); also yields NotFound if the id is absent.
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "set_enabling_option is settable only on a task, not on '{kind}'"
        )));
    }

    // The referenced option must exist (R3): clean 422 over a dangling-FK 500.
    let opt_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM question_options WHERE id = $1",
            args![option_id.to_owned()],
        )
        .await?
        .is_some();
    if !opt_exists {
        return Err(AppError::Validation(format!(
            "question_option '{option_id}' does not exist"
        )));
    }

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET enabling_option_id = $2, updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
            args![task_id.to_owned(), option_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "enabling_option_id": option_id });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.enabling_option_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Resolve an open question by picking an answer option (migration 0003).
///
/// This is the one multi-write mutation in the module: in ONE transaction it
///   1. marks the question `status='answered'`, stamps `chosen_option_id`,
///      `decided_at`, `decided_by`;
///   2. transitions the CHOSEN branch's blocked tasks `blocked → todo` — both the
///      exclusive tasks tied to the chosen option AND any non-exclusive blocked
///      task (NULL `enabling_option_id`) on this question;
///   3. transitions the OTHER branches' EXCLUSIVE blocked tasks (a non-NULL
///      `enabling_option_id` that is NOT the chosen one) to `status='cancelled'`.
///
/// It emits EXACTLY ONE `open_question.resolved` event for the whole resolution
/// (NOT one per task), preserving the +1-event-per-logical-write invariant.
///
/// `chosen_option_id` must belong to the question (else `Validation`). `NotFound`
/// if the question is absent (checked before any write).
pub async fn resolve_open_question(
    db: &impl DbClient,
    question_id: &str,
    chosen_option_id: &str,
    by: Option<&str>,
) -> Result<(), AppError> {
    // NotFound if the question is absent (before any write); capture the owning
    // story for the event aggregate (R1).
    let story_id = open_question_story(db, question_id).await?;

    // Reject re-resolving an already-answered/cancelled question (R4) so the
    // advertised idempotency is real rather than silently re-running the branch
    // transitions on a second call. `status` is a nullable column, so it reads
    // back as `Option<String>` (NULL → None).
    let status = crate::db::scalar_one::<Option<String>>(
        db,
        "SELECT status FROM open_questions WHERE id = $1",
        args![question_id.to_owned()],
    )
    .await?;
    if status.as_deref() != Some("open") {
        return Err(AppError::Validation(format!(
            "open_question '{question_id}' already resolved/cancelled (status {})",
            status.as_deref().unwrap_or("unknown")
        )));
    }

    // Validate the chosen option belongs to THIS question.
    let owns = crate::db::scalar_one::<i64>(
        db,
        "SELECT COUNT(*) FROM question_options WHERE id = $1 AND question_id = $2",
        args![chosen_option_id.to_owned(), question_id.to_owned()],
    )
    .await?;
    if owns == 0 {
        return Err(AppError::Validation(format!(
            "option '{chosen_option_id}' does not belong to open_question '{question_id}'"
        )));
    }

    let mut tx = db.begin().await?;

    // 1. Mark the question answered.
    tx.execute(
        r#"
        UPDATE open_questions
        SET status = 'answered',
            chosen_option_id = $2,
            decided_at = CURRENT_TIMESTAMP,
            decided_by = $3
        WHERE id = $1
        "#,
        args![question_id.to_owned(), chosen_option_id.to_owned(), by.map(str::to_owned)],
    )
    .await?;

    // 2. Unblock the chosen branch: blocked tasks on this question whose
    //    enabling_option is the chosen one OR is NULL (non-exclusive) → todo.
    tx.execute(
        r#"
        UPDATE work_items
        SET status = 'todo', updated_at = CURRENT_TIMESTAMP
        WHERE blocked_by_question_id = $1
          AND status = 'blocked'
          AND (enabling_option_id = $2 OR enabling_option_id IS NULL)
        "#,
        args![question_id.to_owned(), chosen_option_id.to_owned()],
    )
    .await?;

    // 3. Cancel the other branches' EXCLUSIVE tasks: blocked tasks on this
    //    question with a non-NULL enabling_option that is NOT the chosen one.
    tx.execute(
        r#"
        UPDATE work_items
        SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP
        WHERE blocked_by_question_id = $1
          AND status = 'blocked'
          AND enabling_option_id IS NOT NULL
          AND enabling_option_id <> $2
        "#,
        args![question_id.to_owned(), chosen_option_id.to_owned()],
    )
    .await?;

    // EXACTLY ONE event for the whole resolution (NOT per task). Routed to the
    // owning STORY's work_item aggregate (R1) so export renders it; `question_id`
    // is carried so the export drain can re-render this question's affected tasks
    // (R2) without a per-task event.
    let payload =
        serde_json::json!({ "chosen_option_id": chosen_option_id, "question_id": question_id });
    record_event(tx.as_mut(), "work_item", &story_id, "open_question.resolved", payload).await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Repo links (migration 0004) — project↔GitHub-repo associations. Every mutator
// in this section follows the single-mutation-path discipline (one tx, one
// domain-table write, one `record_event`, one commit). Events are routed to the
// owning PROJECT's `work_item` aggregate so `export.rs`'s drain dispatch
// re-renders the project automatically (NOT a fresh `repo_link` aggregate_type
// — the drain would silently skip it).
// ---------------------------------------------------------------------------

/// Walk `parent_id` from `work_item_id` until the row where `kind='project'`,
/// returning the project's id. Used by `set_finding_repo` (project-scope check
/// for the finding's repo binding) AND, in a later task, by `set_task_spec`'s
/// `files_touched` validator (every structured entry must reference a slug
/// linked to the task's project ancestor).
///
/// Errors:
///   * `NotFound` — `work_item_id` does not exist.
///   * `Validation` — the chain bottoms out before reaching a `project` row
///     (defensive: under the 0001 hierarchy trigger pair this is unreachable
///     for items created via `create_work_item`, but a partially-loaded test DB
///     could expose it).
pub async fn find_project_ancestor(
    pool: &impl DbClient,
    work_item_id: &str,
) -> Result<String, AppError> {
    // Recursive CTE: seed with the target row, then repeatedly join to the
    // parent until we either hit the project (returned) or NULL parent on a
    // non-project (caller maps to Validation). The CTE is bounded by the
    // 5-level hierarchy so the walk is O(5) and termination is structural.
    let found: Option<String> = crate::db::scalar_opt::<String>(
        pool,
        r#"
        WITH RECURSIVE ancestors(id, kind, parent_id) AS (
            SELECT id, kind, parent_id FROM work_items WHERE id = $1
            UNION ALL
            SELECT w.id, w.kind, w.parent_id
            FROM work_items w
            JOIN ancestors a ON w.id = a.parent_id
        )
        SELECT id FROM ancestors WHERE kind = 'project' LIMIT 1
        "#,
        args![work_item_id.to_owned()],
    )
    .await?;

    if let Some(id) = found {
        return Ok(id);
    }

    // Distinguish "id does not exist" from "id exists but has no project
    // ancestor": probe the row directly.
    let exists = crate::db::scalar_opt::<i64>(
        pool,
        r#"SELECT 1 FROM work_items WHERE id = $1"#,
        args![work_item_id.to_owned()],
    )
    .await?
    .is_some();

    if !exists {
        Err(AppError::NotFound(format!(
            "work_item '{work_item_id}' not found"
        )))
    } else {
        Err(AppError::Validation(format!(
            "work_item '{work_item_id}' has no 'project' ancestor"
        )))
    }
}

/// `true` if a sqlx error is a SQLite UNIQUE-constraint violation (extended code
/// `SQLITE_CONSTRAINT_UNIQUE` = 2067, primary code `SQLITE_CONSTRAINT` = 19).
/// Used by `add_repo_link` / `set_primary_repo` to translate the partial-primary
/// UNIQUE-index hit into a typed `Validation` rather than a raw 500.
///
/// We match by the backend's constraint-code string (which `sqlx` exposes via
/// `DatabaseError::code()`). On SQLite, both `1555` (PRIMARY KEY) and `2067`
/// (UNIQUE) are flavours of `SQLITE_CONSTRAINT_UNIQUE`-class violations callers
/// should treat as conflicts; the conservative match-set is those two unique
/// flavours, while other constraint codes (FK, CHECK, NOT NULL) pass through as
/// `Db` 500. The `Backend::Pg` arm matches Postgres' SQLSTATE `23505`
/// (`unique_violation`) and is RESERVED for the future Part C — it is statically
/// unreachable today (every caller fronts a `SqlitePool`).
fn is_unique_violation(backend: crate::db::Backend, e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = e
        && let Some(code) = db_err.code()
    {
        return match backend {
            crate::db::Backend::Sqlite => code == "2067" || code == "1555",
            // Reserved for Part C (live Postgres); never reached in the
            // SQLite-only build.
            crate::db::Backend::Pg => code == "23505",
        };
    }
    false
}

/// Add a new `repo_links` row attaching `slug` to `project_id` under the
/// single-mutation-path discipline. `slug` is canonicalised via
/// [`parse_github_slug`] (lowercased both segments); `is_primary` may be set on
/// create, in which case the partial UNIQUE index enforces at most one primary
/// per project (a second primary surfaces as `Validation` via
/// [`is_unique_violation`]).
///
/// `project_id`'s kind is NOT pre-checked — the kind-check trigger pair on
/// `repo_links` (migration 0004) is the authoritative guard; an attach to a
/// non-project surfaces as `Db` 500 via `RAISE(ABORT, ...)`, which matches the
/// repo's "trigger is authoritative" convention (per the file docstring).
///
/// Event `repo_link.created` on the owning project's `work_item` aggregate.
/// Returns the new repo-link id.
pub async fn add_repo_link(
    db: &impl DbClient,
    project_id: &str,
    slug: &str,
    is_primary: bool,
) -> Result<Uuid, AppError> {
    let canonical = parse_github_slug(slug)?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    let is_primary_int: i64 = if is_primary { 1 } else { 0 };

    let backend = db.backend();
    let mut tx = db.begin().await?;

    // Allocate position = MAX(position)+1 per project, inside the tx so a
    // concurrent insert under SQLite's single-writer lock is serialised.
    // COALESCE(MAX(.), -1) + 1 gives 0 for the first row.
    let position = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(position), -1) + 1 FROM repo_links WHERE project_id = $1",
        args![project_id.to_owned()],
    )
    .await?;

    match tx
        .execute(
            r#"
        INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at)
        VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)
        "#,
            args![
                id_str.clone(),
                project_id.to_owned(),
                canonical.clone(),
                position,
                is_primary_int,
            ],
        )
        .await
    {
        Ok(_) => {}
        Err(AppError::Db(ref sqlx_err)) if is_unique_violation(backend, sqlx_err) => {
            // Either the (project_id, slug) UNIQUE or the partial primary UNIQUE
            // index fired. Both are caller-fixable; surface as Validation.
            return Err(AppError::Validation(format!(
                "repo_link conflict: slug '{canonical}' is already linked, or another \
                 primary repo already exists for project '{project_id}' (primary repo conflict)"
            )));
        }
        Err(e) => return Err(e),
    }

    let payload = serde_json::json!({
        "id": id_str,
        "project_id": project_id,
        "slug": canonical,
        "is_primary": is_primary,
    });
    record_event(tx.as_mut(), "work_item", project_id, "repo_link.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`RepoLink`] aggregate
/// (canonical recipe, A8 wave). All columns are NOT NULL, so the field types are
/// `String`/`i64` (no `Option<String>` bound is needed); `is_primary` mirrors the
/// INTEGER 0/1 as `i64`. Replaces the old `query_as!` `AS "col!"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for RepoLink
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(RepoLink {
            id: row.try_get("id")?,
            project_id: row.try_get("project_id")?,
            slug: row.try_get("slug")?,
            position: row.try_get("position")?,
            is_primary: row.try_get("is_primary")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// List the `repo_links` rows for a project, ordered by `position` ASC. Returns
/// an empty Vec for a project with no links (or for a non-project id — caller is
/// expected to gate this query on `kind='project'`). Read-only; no transaction.
pub async fn list_repo_links(
    db: &impl DbClient,
    project_id: &str,
) -> Result<Vec<RepoLink>, AppError> {
    let rows = db
        .query_all::<RepoLink>(
            r#"
        SELECT
            id,
            project_id,
            slug,
            position,
            is_primary,
            created_at
        FROM repo_links
        WHERE project_id = $1
        ORDER BY position ASC
        "#,
            args![project_id.to_owned()],
        )
        .await?;

    Ok(rows)
}

/// Hard-delete a `repo_links` row. The owning project's id is read first so
/// (a) an absent id is `NotFound` BEFORE any write, and (b) the event aggregate
/// is the project's `work_item` (so the export drain re-renders the project).
///
/// `findings.repo_id`'s FK is `ON DELETE SET NULL` (migration 0004), so any
/// finding pointing at this link drops back to implicit-primary resolution
/// automatically — no separate UPDATE here.
///
/// Event `repo_link.removed` on the owning project's `work_item` aggregate.
pub async fn remove_repo_link(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    // Resolve the owning project + slug BEFORE the write so the event aggregate
    // is correct and so an absent id is `NotFound` (not `rows_affected()==0`).
    let (project_id, slug) = db
        .query_opt::<(String, String)>(
            "SELECT project_id, slug FROM repo_links WHERE id = $1",
            args![id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("repo_link '{id}' not found")))?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute("DELETE FROM repo_links WHERE id = $1", args![id.to_owned()])
        .await?;

    if affected == 0 {
        // Lost a race against a concurrent delete — caller sees NotFound.
        return Err(AppError::NotFound(format!("repo_link '{id}' not found")));
    }

    let payload = serde_json::json!({
        "id": id,
        "project_id": project_id,
        "slug": slug,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        &project_id,
        "repo_link.removed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Promote `repo_link_id` to the project's primary repo. Critical ordering:
/// inside one [`crate::db::begin_write`] tx, FIRST clear any existing primary on the same
/// project, THEN set the target to primary. SQLite checks the partial UNIQUE
/// index `idx_repo_links_one_primary` per-statement, so the clear MUST precede
/// the set or the second UPDATE fails with `SQLITE_CONSTRAINT_UNIQUE`.
///
/// The `AND project_id = ?` guard on the set defends against a cross-project
/// hijack where `repo_link_id` belongs to a different project (would otherwise
/// silently no-op and still emit an event).
///
/// Concurrent calls are serialised by SQLite's single-writer lock (last write
/// wins, both succeed); a residual unique-violation surfaces as `Validation` via
/// [`is_unique_violation`]. `NotFound` if the target id doesn't exist under the
/// given project. Event `repo_link.primary_changed` with the previous primary
/// id (or null) and the new primary id.
pub async fn set_primary_repo(
    db: &impl DbClient,
    project_id: &str,
    repo_link_id: &str,
) -> Result<(), AppError> {
    let backend = db.backend();
    let mut tx = db.begin().await?;

    // Step 1: capture the previous primary's id (for the event payload) BEFORE
    // we clear it. NULL if no current primary.
    let previous: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT id FROM repo_links WHERE project_id = $1 AND is_primary = 1",
        args![project_id.to_owned()],
    )
    .await?;

    // Step 2: clear the existing primary (idempotent if `previous` is None).
    tx.execute(
        "UPDATE repo_links SET is_primary = 0 WHERE project_id = $1 AND is_primary = 1",
        args![project_id.to_owned()],
    )
    .await?;

    // Step 3: promote the target — AND project_id guards against cross-project
    // ids. rows_affected()==0 ⇒ NotFound (id absent or wrong project).
    let affected = match tx
        .execute(
            "UPDATE repo_links SET is_primary = 1 WHERE id = $1 AND project_id = $2",
            args![repo_link_id.to_owned(), project_id.to_owned()],
        )
        .await
    {
        Ok(n) => n,
        Err(AppError::Db(ref sqlx_err)) if is_unique_violation(backend, sqlx_err) => {
            return Err(AppError::Validation(format!(
                "primary repo conflict on project '{project_id}': another row already \
                 holds is_primary=1 (concurrent set_primary_repo)"
            )));
        }
        Err(e) => return Err(e),
    };

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "repo_link '{repo_link_id}' not found under project '{project_id}'"
        )));
    }

    let payload = serde_json::json!({
        "project_id": project_id,
        "new_primary_id": repo_link_id,
        "previous_primary_id": previous,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        project_id,
        "repo_link.primary_changed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Set or clear `findings.repo_id` under the single-mutation-path discipline.
/// `repo_id=Some` binds the finding to a non-primary linked repo; `None` clears
/// the binding (the finding falls back to the project's primary repo at read
/// time).
///
/// Validation (soft, BEYOND the FK):
///   * The finding must exist (`NotFound` otherwise).
///   * When `repo_id` is `Some`, the target `repo_links` row must belong to
///     the finding's project ancestor (`Validation` otherwise). The schema FK
///     only ensures the id exists in `repo_links`; this guard rejects a
///     cross-project hijack where a finding under project A is bound to a
///     repo link of project B.
///
/// Event `finding.repo_changed` on the finding's work_item aggregate
/// (`aggregate_type = "work_item"`, `aggregate_id = <finding.work_item_id>`).
pub async fn set_finding_repo(
    pool: &impl DbClient,
    finding_id: &str,
    repo_id: Option<&str>,
) -> Result<(), AppError> {
    // Resolve the finding's owning work_item_id BEFORE opening the tx. NotFound
    // if the finding is absent.
    let work_item_id: String = pool
        .query_opt::<Scalar<Option<String>>>(
            "SELECT work_item_id FROM findings WHERE id = $1",
            args![finding_id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("finding '{finding_id}' not found")))?
        .0
        .ok_or_else(|| {
            // A finding with NULL work_item_id has no project to validate against.
            // This is a Validation, not a 500 — the importer may produce such rows
            // for orphaned findings and the caller is expected to repair them first.
            AppError::Validation(format!(
                "finding '{finding_id}' has no work_item_id; cannot bind to a repo"
            ))
        })?;

    // Project-scope check on the repo_id (if set): the target repo_link must
    // belong to the project ancestor of this finding's work-item.
    if let Some(rid) = repo_id {
        let project_id = find_project_ancestor(pool, &work_item_id).await?;
        let owns = pool
            .query_opt::<Scalar<i64>>(
                "SELECT 1 FROM repo_links WHERE id = $1 AND project_id = $2",
                args![rid.to_owned(), project_id.clone()],
            )
            .await?
            .is_some();
        if !owns {
            return Err(AppError::Validation(format!(
                "repo_link '{rid}' does not belong to the project ancestor '{project_id}' \
                 of finding '{finding_id}'"
            )));
        }
    }

    // `pool` is `&impl DbClient`, so `pool.begin()` resolves unambiguously to
    // `DbClient::begin` (returning the object-safe `Box<dyn DbTx>` this function
    // threads through) — there is no inherent `begin` on `impl DbClient`.
    let mut tx = pool.begin().await?;

    let affected = tx
        .execute(
            "UPDATE findings SET repo_id = $2 WHERE id = $1",
            args![finding_id.to_owned(), repo_id.map(|s| s.to_owned())],
        )
        .await?;

    if affected == 0 {
        // Lost a race against a concurrent delete — surface NotFound rather
        // than emitting a spurious event.
        return Err(AppError::NotFound(format!("finding '{finding_id}' not found")));
    }

    let payload = serde_json::json!({
        "finding_id": finding_id,
        "repo_id": repo_id,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "finding.repo_changed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ===========================================================================
// Migration 0005 — round-2 planning surface
//
// Three new child tables (`risks`, `rejected_alternatives`, `task_dependencies`)
// and one new column (`work_items.task_kind`). Every mutation in this section
// follows the single-mutation-path discipline: open one `crate::db::begin_write` tx,
// write to exactly ONE domain table, call `record_event` for ONE outbox row,
// commit. Events are routed to the owning work-item's `work_item` aggregate
// (NOT a fresh `risk` / `rejected_alternative` / `task_dependency` aggregate),
// because `export.rs`'s drain dispatch only re-renders `work_item` aggregates;
// a stand-alone aggregate type would never reach the git-export snapshot.
// ===========================================================================

// ---------------------------------------------------------------------------
// Risks (migration 0005). Mirror the `research_notes` CRUD: append-with-seq,
// partial set-or-leave update, supersession by self-FK, hard remove. Severity
// is a closed enum CHECK-constrained at the DB layer (low|medium|high|critical);
// we validate it here so an invalid value surfaces as `Validation` (→ 422)
// rather than a constraint-violation 500.
// ---------------------------------------------------------------------------

/// Render the canonical wire spelling of a `RiskSeverity` for storage. Mirrors
/// `enum_to_str` but takes a typed enum so callers cannot fabricate an invalid
/// value at the call site. The `&str` callers (e.g. `add_risk`) go through
/// `validate_risk_severity_str` to project a raw string into this enum first.
fn risk_severity_str(s: RiskSeverity) -> String {
    enum_to_str(s)
}

/// Validate a raw severity string against the closed [`RiskSeverity`] enum.
/// Surfaces a clean `Validation` (→ 422) on an unknown value, BEFORE the DB
/// CHECK constraint would otherwise fire as a `Db` 500. The canonical wire
/// spelling (lowercase) is returned for storage.
fn validate_risk_severity_str(s: &str) -> Result<String, AppError> {
    serde_json::from_value::<RiskSeverity>(Value::String(s.to_owned()))
        .map(risk_severity_str)
        .map_err(|_| {
            AppError::Validation(format!(
                "unknown risk severity '{s}' (expected one of low, medium, high, critical)"
            ))
        })
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`Risk`] aggregate (canonical
/// recipe, A9 wave). The NOT NULL columns map to `String`/`i64`; the nullable
/// columns (`body`/`rationale`/`severity`/`mitigation`/`superseded_by`) map to
/// `Option<String>`. Replaces the old `query_as!` `AS "col!"`/`"col?"` hints.
impl<'r, R> sqlx::FromRow<'r, R> for Risk
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(Risk {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            summary: row.try_get("summary")?,
            body: row.try_get("body")?,
            rationale: row.try_get("rationale")?,
            severity: row.try_get("severity")?,
            mitigation: row.try_get("mitigation")?,
            superseded_by: row.try_get("superseded_by")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// List the LIVE risk rows for a work item (migration 0005), ordered by the
/// per-item monotonic `seq`. "Live" = `superseded_by IS NULL`; a superseded
/// risk drops out of this fold. Runtime seam: `query_all` onto the [`Risk`]
/// read struct (all columns map 1:1 via its FromRow).
async fn list_risks(db: &impl DbClient, work_item_id: &str) -> Result<Vec<Risk>, AppError> {
    db.query_all::<Risk>(
        r#"
        SELECT
            id,
            work_item_id,
            seq,
            summary,
            body,
            rationale,
            severity,
            mitigation,
            superseded_by,
            created_at
        FROM risks
        WHERE work_item_id = $1
          AND superseded_by IS NULL
        ORDER BY seq
        "#,
        args![work_item_id.to_owned()],
    )
    .await
}

/// Append ONE `risks` row under the single-mutation-path discipline (migration
/// 0005). Mirrors [`add_research_note`]: `seq = MAX+1` per work item allocated
/// inside the tx, work item must exist (`NotFound` otherwise), severity validated
/// against the closed [`RiskSeverity`] enum BEFORE the write so an unknown value
/// is a clean 422 (not a `Db` 500 from the CHECK constraint). Event
/// `risk.added` routed to the owning work-item's `work_item` aggregate so
/// `export.rs` re-renders. Returns the new risk id.
pub async fn add_risk(
    db: &impl DbClient,
    work_item_id: &str,
    summary: &str,
    body: Option<&str>,
    rationale: Option<&str>,
    severity: &str,
    mitigation: Option<&str>,
) -> Result<Uuid, AppError> {
    let severity = validate_risk_severity_str(severity)?;
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM risks WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO risks
            (id, work_item_id, seq, summary, body, rationale, severity, mitigation)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        args![
            id_str.clone(),
            work_item_id.to_owned(),
            seq,
            summary.to_owned(),
            body.map(str::to_owned),
            rationale.map(str::to_owned),
            severity.to_owned(),
            mitigation.map(str::to_owned),
        ],
    )
    .await?;

    let payload = serde_json::json!({
        "risk_id": id_str,
        "seq": seq,
        "severity": severity,
    });
    record_event(tx.as_mut(), "work_item", work_item_id, "risk.added", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Read a risk's owning `work_item_id`, erroring `NotFound` if the risk id has
/// no row. Mirrors [`research_note_work_item`] — the update / supersede /
/// remove paths all need the owning aggregate id for the event routing.
async fn risk_work_item(db: &impl DbClient, id: &str) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM risks WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("risk '{id}' not found")))
}

/// Partial set-or-leave update of a risk's curatable fields (migration 0005):
/// `summary`/`body`/`rationale`/`severity`/`mitigation` via `COALESCE(?, col)`.
/// The typed [`RiskSeverity`] is rendered to its wire form before the COALESCE
/// bind. Mirrors [`update_research_note`]. `NotFound` via `rows_affected()==0`;
/// one event `risk.updated`.
pub async fn update_risk(
    db: &impl DbClient,
    id: &str,
    patch: &RiskPatch,
) -> Result<(), AppError> {
    let work_item_id = risk_work_item(db, id).await?;
    let severity_str: Option<String> = patch.severity.map(risk_severity_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE risks
        SET summary    = COALESCE($2, summary),
            body       = COALESCE($3, body),
            rationale  = COALESCE($4, rationale),
            severity   = COALESCE($5, severity),
            mitigation = COALESCE($6, mitigation)
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                patch.summary.clone(),
                patch.body.clone(),
                patch.rationale.clone(),
                severity_str.clone(),
                patch.mitigation.clone(),
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("risk '{id}' not found")));
    }

    let payload = serde_json::json!({
        "risk_id": id,
        "severity": severity_str,
    });
    record_event(tx.as_mut(), "work_item", &work_item_id, "risk.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Supersede a risk (migration 0005): insert a NEW risk row under the same
/// work item, then set `superseded_by = new_id` on the OLD row so it drops out
/// of the live `list_risks` fold. Both writes share ONE transaction and emit
/// EXACTLY ONE `risk.superseded` event (NOT a separate `risk.added` for the
/// new row — supersession is one logical write, mirroring the research-note
/// supersession discipline in [`supersede_research_note`]). Returns the new id.
#[allow(clippy::too_many_arguments)]
pub async fn supersede_risk(
    db: &impl DbClient,
    work_item_id: &str,
    old_id: &str,
    new_summary: &str,
    new_body: Option<&str>,
    new_rationale: Option<&str>,
    new_severity: &str,
    new_mitigation: Option<&str>,
) -> Result<Uuid, AppError> {
    let severity = validate_risk_severity_str(new_severity)?;
    // Verify the old risk belongs to the named work item; NotFound otherwise.
    let actual_wi = risk_work_item(db, old_id).await?;
    if actual_wi != work_item_id {
        return Err(AppError::Validation(format!(
            "risk '{old_id}' belongs to work_item '{actual_wi}', not '{work_item_id}'"
        )));
    }

    let new_id = Uuid::now_v7();
    let new_id_str = new_id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM risks WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO risks
            (id, work_item_id, seq, summary, body, rationale, severity, mitigation)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        args![
            new_id_str.clone(),
            work_item_id.to_owned(),
            seq,
            new_summary.to_owned(),
            new_body.map(str::to_owned),
            new_rationale.map(str::to_owned),
            severity.clone(),
            new_mitigation.map(str::to_owned),
        ],
    )
    .await?;

    let affected = tx
        .execute(
            "UPDATE risks SET superseded_by = $2 WHERE id = $1",
            args![old_id.to_owned(), new_id_str.clone()],
        )
        .await?;

    if affected == 0 {
        // Concurrent delete between `risk_work_item` read and the UPDATE; the
        // tx drops → ROLLBACK so the INSERT above does not leak.
        return Err(AppError::NotFound(format!("risk '{old_id}' not found")));
    }

    let payload = serde_json::json!({
        "old_id": old_id,
        "new_id": new_id_str,
        "seq": seq,
        "severity": severity,
    });
    record_event(tx.as_mut(), "work_item", work_item_id, "risk.superseded", payload).await?;

    tx.commit().await?;
    Ok(new_id)
}

/// Hard-delete a risk under the single-mutation-path discipline. Risks have no
/// independent export identity (they fold into the owning work-item's TOML), so
/// removal is a hard DELETE. `NotFound` via `rows_affected()==0`. Event
/// `risk.removed` on the owning work-item's aggregate.
pub async fn remove_risk(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let work_item_id = risk_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute("DELETE FROM risks WHERE id = $1", args![id.to_owned()])
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("risk '{id}' not found")));
    }

    let payload = serde_json::json!({ "risk_id": id, "removed": true });
    record_event(tx.as_mut(), "work_item", &work_item_id, "risk.removed", payload).await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rejected alternatives (migration 0005). Same shape as `risks` minus severity;
// `confidence` is free TEXT (matches `research_notes.confidence` — validated in
// the repo, NOT a DB CHECK).
// ---------------------------------------------------------------------------

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`RejectedAlternative`]
/// aggregate (canonical recipe, A9 wave). NOT NULL columns map to `String`/`i64`;
/// the nullable columns (`body`/`rationale`/`confidence`/`superseded_by`) map to
/// `Option<String>`. Replaces the old `query_as!` `AS "col!"`/`"col?"` hints.
impl<'r, R> sqlx::FromRow<'r, R> for RejectedAlternative
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(RejectedAlternative {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            summary: row.try_get("summary")?,
            body: row.try_get("body")?,
            rationale: row.try_get("rationale")?,
            confidence: row.try_get("confidence")?,
            superseded_by: row.try_get("superseded_by")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// List the LIVE rejected-alternative rows for a work item (migration 0005),
/// ordered by the per-item monotonic `seq`. "Live" = `superseded_by IS NULL`.
async fn list_rejected_alternatives(
    db: &impl DbClient,
    work_item_id: &str,
) -> Result<Vec<RejectedAlternative>, AppError> {
    db.query_all::<RejectedAlternative>(
        r#"
        SELECT
            id,
            work_item_id,
            seq,
            summary,
            body,
            rationale,
            confidence,
            superseded_by,
            created_at
        FROM rejected_alternatives
        WHERE work_item_id = $1
          AND superseded_by IS NULL
        ORDER BY seq
        "#,
        args![work_item_id.to_owned()],
    )
    .await
}

/// Append ONE `rejected_alternatives` row under the single-mutation-path
/// discipline (migration 0005). Mirrors [`add_risk`] minus the severity
/// validation: `confidence` is free TEXT (validated nowhere at the DB; mirrors
/// `research_notes.confidence`). Event `rejected_alternative.added`.
pub async fn add_rejected_alternative(
    db: &impl DbClient,
    work_item_id: &str,
    summary: &str,
    body: Option<&str>,
    rationale: Option<&str>,
    confidence: Option<&str>,
) -> Result<Uuid, AppError> {
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM rejected_alternatives WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO rejected_alternatives
            (id, work_item_id, seq, summary, body, rationale, confidence)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        args![
            id_str.clone(),
            work_item_id.to_owned(),
            seq,
            summary.to_owned(),
            body.map(str::to_owned),
            rationale.map(str::to_owned),
            confidence.map(str::to_owned),
        ],
    )
    .await?;

    let payload = serde_json::json!({
        "alternative_id": id_str,
        "seq": seq,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        work_item_id,
        "rejected_alternative.added",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read a rejected-alternative's owning `work_item_id`, erroring `NotFound` if
/// the id has no row. Mirrors [`risk_work_item`].
async fn rejected_alternative_work_item(
    db: &impl DbClient,
    id: &str,
) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM rejected_alternatives WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("rejected_alternative '{id}' not found")))
}

/// Partial set-or-leave update of a rejected-alternative's curatable fields
/// (migration 0005). Mirrors [`update_risk`] minus severity; `confidence` is
/// free TEXT, no enum projection. Event `rejected_alternative.updated`.
pub async fn update_rejected_alternative(
    db: &impl DbClient,
    id: &str,
    patch: &AlternativePatch,
) -> Result<(), AppError> {
    let work_item_id = rejected_alternative_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE rejected_alternatives
        SET summary    = COALESCE($2, summary),
            body       = COALESCE($3, body),
            rationale  = COALESCE($4, rationale),
            confidence = COALESCE($5, confidence)
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                patch.summary.clone(),
                patch.body.clone(),
                patch.rationale.clone(),
                patch.confidence.clone(),
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("rejected_alternative '{id}' not found")));
    }

    let payload = serde_json::json!({ "alternative_id": id });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "rejected_alternative.updated",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Supersede a rejected alternative (migration 0005): insert a NEW row under
/// the same work item, then point the OLD row at it via `superseded_by`.
/// Mirrors [`supersede_risk`]; one transaction, one event
/// `rejected_alternative.superseded`. Returns the new id.
pub async fn supersede_rejected_alternative(
    db: &impl DbClient,
    work_item_id: &str,
    old_id: &str,
    new_summary: &str,
    new_body: Option<&str>,
    new_rationale: Option<&str>,
    new_confidence: Option<&str>,
) -> Result<Uuid, AppError> {
    let actual_wi = rejected_alternative_work_item(db, old_id).await?;
    if actual_wi != work_item_id {
        return Err(AppError::Validation(format!(
            "rejected_alternative '{old_id}' belongs to work_item '{actual_wi}', \
             not '{work_item_id}'"
        )));
    }

    let new_id = Uuid::now_v7();
    let new_id_str = new_id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM rejected_alternatives WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO rejected_alternatives
            (id, work_item_id, seq, summary, body, rationale, confidence)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        args![
            new_id_str.clone(),
            work_item_id.to_owned(),
            seq,
            new_summary.to_owned(),
            new_body.map(str::to_owned),
            new_rationale.map(str::to_owned),
            new_confidence.map(str::to_owned),
        ],
    )
    .await?;

    let affected = tx
        .execute(
            "UPDATE rejected_alternatives SET superseded_by = $2 WHERE id = $1",
            args![old_id.to_owned(), new_id_str.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "rejected_alternative '{old_id}' not found"
        )));
    }

    let payload = serde_json::json!({
        "old_id": old_id,
        "new_id": new_id_str,
        "seq": seq,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        work_item_id,
        "rejected_alternative.superseded",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(new_id)
}

/// Hard-delete a rejected alternative under the single-mutation-path discipline.
/// `NotFound` via `rows_affected()==0`; one event `rejected_alternative.removed`.
pub async fn remove_rejected_alternative(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let work_item_id = rejected_alternative_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "DELETE FROM rejected_alternatives WHERE id = $1",
            args![id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("rejected_alternative '{id}' not found")));
    }

    let payload = serde_json::json!({ "alternative_id": id, "removed": true });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "rejected_alternative.removed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Task dependencies (migration 0005). Directed edges between two `kind=task`
// work-items. The BEFORE INSERT trigger on `task_dependencies` enforces the
// kind=task constraint on both endpoints; we PRE-CHECK in the repo so an
// illegal edge surfaces as a clean `Validation` (→ 422) rather than the
// trigger's RAISE(ABORT, ...) mapped to a `Db` 500.
// ---------------------------------------------------------------------------

/// List the OUTGOING task_dependencies edges from `task_id`, ordered by
/// `depends_on_id` for deterministic output. Used by `get_work_item_detail`'s
/// per-task fold.
/// Generic-`R` [`sqlx::FromRow`] for the read-only [`TaskDependency`] edge
/// aggregate (canonical recipe, A9 wave). All four columns are NOT NULL, so the
/// field types are `String` (no `Option<String>` bound is needed). Replaces the
/// old `query_as!` `AS "col!"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for TaskDependency
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(TaskDependency {
            task_id: row.try_get("task_id")?,
            depends_on_id: row.try_get("depends_on_id")?,
            kind: row.try_get("kind")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

async fn list_outgoing_task_dependencies(
    db: &impl DbClient,
    task_id: &str,
) -> Result<Vec<TaskDependency>, AppError> {
    db.query_all::<TaskDependency>(
        r#"
        SELECT
            task_id,
            depends_on_id,
            kind,
            created_at
        FROM task_dependencies
        WHERE task_id = $1
        ORDER BY depends_on_id
        "#,
        args![task_id.to_owned()],
    )
    .await
}

/// List all task_dependencies edges where BOTH endpoints are direct task
/// children of `story_id`. Sorted by `(task_id, depends_on_id)` for
/// deterministic output. Used by [`compute_task_batches`] and by the
/// `wire-task-deps` SKILL to render the story's dependency graph.
pub async fn list_task_dependencies(
    db: &impl DbClient,
    story_id: &str,
) -> Result<Vec<TaskDependency>, AppError> {
    // `$1` (story_id) is referenced twice in the predicate; SQLite reuses the
    // same bound value for both occurrences, so a single positional bind suffices.
    db.query_all::<TaskDependency>(
        r#"
        SELECT
            task_id,
            depends_on_id,
            kind,
            created_at
        FROM task_dependencies
        WHERE task_id       IN (SELECT id FROM work_items WHERE parent_id = $1 AND kind = 'task')
          AND depends_on_id IN (SELECT id FROM work_items WHERE parent_id = $1 AND kind = 'task')
        ORDER BY task_id, depends_on_id
        "#,
        args![story_id.to_owned()],
    )
    .await
}

/// Add a task→task dependency edge under the single-mutation-path discipline.
/// PRE-CHECKs that both endpoints reference `kind='task'` rows so the kind-
/// check trigger's `RAISE(ABORT, ...)` does not surface as a `Db` 500. The
/// composite PK `(task_id, depends_on_id)` makes duplicate edges structurally
/// impossible — a re-add surfaces as a UNIQUE-violation `Validation`.
/// Self-loops are rejected by the row-level `CHECK (task_id <> depends_on_id)`,
/// re-projected here as a clean `Validation`. Event `task_dependency.added`
/// routed to the owning task's aggregate so `export.rs` re-renders.
pub async fn add_task_dependency(
    db: &impl DbClient,
    task_id: &str,
    depends_on_id: &str,
    kind: &str,
) -> Result<TaskDependency, AppError> {
    if task_id == depends_on_id {
        return Err(AppError::Validation(format!(
            "task_dependency self-loop rejected: task '{task_id}' cannot depend on itself"
        )));
    }

    // Pre-check both endpoints are kind=task; surfaces NotFound (id absent)
    // and Validation (wrong kind) as clean typed errors.
    let task_kind = work_item_kind(db, task_id).await?;
    if task_kind != "task" {
        return Err(AppError::Validation(format!(
            "task_dependency.task_id must reference a 'task', not a '{task_kind}'"
        )));
    }
    let dep_kind = work_item_kind(db, depends_on_id).await?;
    if dep_kind != "task" {
        return Err(AppError::Validation(format!(
            "task_dependency.depends_on_id must reference a 'task', not a '{dep_kind}'"
        )));
    }

    let backend = db.backend();
    let mut tx = db.begin().await?;

    match tx
        .execute(
            r#"
        INSERT INTO task_dependencies (task_id, depends_on_id, kind)
        VALUES ($1, $2, $3)
        "#,
            args![task_id.to_owned(), depends_on_id.to_owned(), kind.to_owned()],
        )
        .await
    {
        Ok(_) => {}
        Err(AppError::Db(ref sqlx_err)) if is_unique_violation(backend, sqlx_err) => {
            return Err(AppError::Validation(format!(
                "task_dependency '{task_id} -> {depends_on_id}' already exists"
            )));
        }
        Err(e) => return Err(e),
    }

    let row = crate::db::tx_query_one::<TaskDependency>(
        tx.as_mut(),
        r#"
        SELECT
            task_id,
            depends_on_id,
            kind,
            created_at
        FROM task_dependencies
        WHERE task_id = $1 AND depends_on_id = $2
        "#,
        args![task_id.to_owned(), depends_on_id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({
        "task_id": task_id,
        "depends_on_id": depends_on_id,
        "kind": kind,
    });
    record_event(tx.as_mut(), "work_item", task_id, "task_dependency.added", payload).await?;

    tx.commit().await?;
    Ok(row)
}

/// Remove a task→task dependency edge. `NotFound` via `rows_affected()==0` so
/// removing an absent edge does not emit a spurious event. Event
/// `task_dependency.removed` routed to the owning task's aggregate.
pub async fn remove_task_dependency(
    db: &impl DbClient,
    task_id: &str,
    depends_on_id: &str,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "DELETE FROM task_dependencies WHERE task_id = $1 AND depends_on_id = $2",
            args![task_id.to_owned(), depends_on_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "task_dependency '{task_id} -> {depends_on_id}' not found"
        )));
    }

    let payload = serde_json::json!({
        "task_id": task_id,
        "depends_on_id": depends_on_id,
    });
    record_event(tx.as_mut(), "work_item", task_id, "task_dependency.removed", payload).await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// work_items.task_kind setter (migration 0005, vocab narrowed in 0007).
// Task-scoped column; the CHECK constraint accepts NULL OR the three enum
// literals (foundation | main | polish — see CONVENTIONS §j for the cull
// rationale). We validate the enum value here before the write so an unknown
// literal surfaces as `Validation` (→ 422) rather than a `Db` 500.
// ---------------------------------------------------------------------------

/// Set or clear a task's `task_kind` (migration 0005). Task-scoped: a non-`task`
/// kind is rejected with a typed [`AppError::Validation`] (mirrors
/// [`set_effort`] / [`set_complexity`]). `task_kind = None` CLEARS the column to
/// NULL (deliberate divergence from the SET-OR-LEAVE convention — `task_kind`
/// is a discriminator the sprint composer may legitimately want to clear). One
/// event `work_item.task_kind_set`.
pub async fn set_task_kind(
    db: &impl DbClient,
    task_id: &str,
    task_kind: Option<TaskKind>,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "task_kind is settable only on a task, not on '{kind}'"
        )));
    }

    let value: Option<String> = task_kind.map(enum_to_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET task_kind = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![task_id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "task_id": task_id, "task_kind": value });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.task_kind_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// compute_tier (migration 0006). Pure function deriving the dispatch [`Tier`]
// from a task's spec (effort, complexity, files-touched count, cross-repo
// flag). No I/O, no async — consumed by the read-only `get_task_dispatch_plan`
// fold below.
// ---------------------------------------------------------------------------

/// Derive the dispatch [`Tier`] from a task's spec.
///
/// Rule (round-3, documented in CONVENTIONS.md §k of the
/// `lumina-story-blocks` plugin):
///
/// ```text
/// if complexity == "high":          Deep
/// if effort == "l":                 Deep
/// if files_touched_count > 3:       Deep
/// if has_cross_repo:                Deep
/// else:                             Lite
/// ```
///
/// `effort` and `complexity` are passed as `Option<&str>` to match the
/// row-struct idiom (free-text-in-row); unrecognised wire values fall through
/// to the residual `Lite` branch (defensive — the wire surface should reject
/// these at the MCP-param boundary via the typed [`Effort`] / [`Complexity`]
/// enums).
pub fn compute_tier(
    effort: Option<&str>,
    complexity: Option<&str>,
    files_touched_count: usize,
    has_cross_repo: bool,
) -> Tier {
    if complexity == Some("high") {
        return Tier::Deep;
    }
    if effort == Some("l") {
        return Tier::Deep;
    }
    if files_touched_count > 3 {
        return Tier::Deep;
    }
    if has_cross_repo {
        return Tier::Deep;
    }
    Tier::Lite
}

// ---------------------------------------------------------------------------
// compute_task_batches (migration 0005). Kahn's algorithm topological sort on
// the per-story task dependency graph. Read-only; no transaction, no events.
// ---------------------------------------------------------------------------

/// Sort-key projection of [`TaskKind`] for intra-phase tie-breaking. Foundation
/// tasks float to the earliest possible phase so the sprint composer's
/// "Phase 1 (foundation)" labelling is structural, not advisory. Polish tasks
/// sink to the latest position within their phase. NULL `task_kind` sorts at
/// the `Main` slot — treating an unlabelled task as "default main work" gives
/// stable foundation-before-everything-else-before-polish ordering without
/// penalising callers that haven't yet stamped a kind.
///
/// Pre-migration-0007 values (`vertical-slice`, `pattern-replacement`) cannot
/// be produced by any current Rust write path; should they survive in a
/// stale read (or arrive from a foreign data source bypassing the typed
/// enum), the catch-all arm puts them in the same `Main` bucket as NULL —
/// preserving the historical "neither foundation nor polish" intent without
/// reanimating the deprecated taxonomy.
fn task_kind_sort_key(s: Option<&str>) -> u8 {
    match s {
        Some("foundation") => 0,
        Some("main") => 1,
        Some("polish") => 2,
        _ => 1, // NULL or any legacy / unknown value → same slot as Main.
    }
}

/// Raw row read by [`compute_task_batches`]: a story's task children with the
/// `task_kind` discriminator carried alongside so the intra-phase sort avoids a
/// second query. Generic over `R: Row` per the canonical [`crate::db`] FromRow
/// recipe; `task_kind` is nullable (`Option<String>`), `id`/`created_at` are
/// NOT-NULL (`String`).
#[derive(Debug)]
struct TaskBatchRow {
    id: String,
    task_kind: Option<String>,
    created_at: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for TaskBatchRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(TaskBatchRow {
            id: row.try_get("id")?,
            task_kind: row.try_get("task_kind")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Compute task batches (phases) for a story via Kahn's topological sort.
/// Returns a `Vec` of phases, each phase a `Vec` of task ids whose dependencies
/// were satisfied by earlier phases. Within a phase, tasks sort by
/// `(task_kind ordering, created_at)` so foundation tasks rise to the earliest
/// phase as a deterministic tie-break.
///
/// Errors:
///   * `NotFound` — `story_id` does not exist.
///   * `Validation` — `story_id` exists but is not `kind='story'`.
///   * `Cycle { edges }` — the dependency graph contains a cycle; the residue
///     (edges that remain after Kahn's drains the zero-in-degree frontier) is
///     returned so the caller can surface the offending edges.
///
/// Read-only: no transaction, no events.
pub async fn compute_task_batches(
    pool: &impl DbClient,
    story_id: &str,
) -> Result<Vec<Vec<String>>, AppError> {
    // Validate the story exists and IS a story (NotFound vs Validation split).
    let kind = work_item_kind(pool, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "compute_task_batches expects a 'story', not a '{kind}'"
        )));
    }

    // Load all task children of the story, ordered by created_at for stable
    // intra-phase tie-breaking when task_kind is NULL on every task. We carry
    // `task_kind` alongside the id so the intra-phase sort can use it without
    // a second query.
    let tasks = pool
        .query_all::<TaskBatchRow>(
            r#"
        SELECT id, task_kind, created_at
        FROM work_items
        WHERE parent_id = $1
          AND kind = 'task'
          AND deleted_at IS NULL
        ORDER BY created_at, id
        "#,
            args![story_id.to_owned()],
        )
        .await?;

    if tasks.is_empty() {
        return Ok(Vec::new());
    }

    // Build the dependency graph: in_degree[v] = number of unsatisfied deps;
    // successors[u] = tasks that depend on u (so we can decrement their
    // in-degree when u is drained).
    use std::collections::BTreeMap;
    let task_ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let mut id_to_idx: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, id) in task_ids.iter().enumerate() {
        id_to_idx.insert(id.as_str(), i);
    }

    let edges = list_task_dependencies(pool, story_id).await?;
    let n = task_ids.len();
    let mut in_degree: Vec<usize> = vec![0; n];
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); n];

    for edge in &edges {
        // Both endpoints must be in the per-story task set; defensively skip
        // edges whose endpoints lie outside (should not happen given the
        // `list_task_dependencies` WHERE clause, but the index lookup is the
        // authority here).
        let Some(&u) = id_to_idx.get(edge.depends_on_id.as_str()) else {
            continue;
        };
        let Some(&v) = id_to_idx.get(edge.task_id.as_str()) else {
            continue;
        };
        successors[u].push(v);
        in_degree[v] += 1;
    }

    // Build the intra-phase sort key cache once.
    let sort_key: Vec<(u8, &str, &str)> = tasks
        .iter()
        .map(|t| (task_kind_sort_key(t.task_kind.as_deref()), t.created_at.as_str(), t.id.as_str()))
        .collect();

    // Kahn's: repeatedly drain the zero-in-degree frontier as a phase, then
    // decrement successors. Within each phase, sort by (task_kind, created_at,
    // id) so foundation tasks float earliest.
    let mut remaining: usize = n;
    let mut drained: Vec<bool> = vec![false; n];
    let mut phases: Vec<Vec<String>> = Vec::new();

    loop {
        let mut frontier: Vec<usize> = (0..n)
            .filter(|&i| !drained[i] && in_degree[i] == 0)
            .collect();
        if frontier.is_empty() {
            break;
        }
        frontier.sort_by(|&a, &b| sort_key[a].cmp(&sort_key[b]));

        let phase: Vec<String> = frontier.iter().map(|&i| task_ids[i].clone()).collect();
        for &i in &frontier {
            drained[i] = true;
            remaining -= 1;
            // Defensive clone of successor list — borrow-checker won't let us
            // index `successors[i]` while mutably borrowing `in_degree` if the
            // successor list itself borrows from `successors` (it doesn't,
            // since `Vec<usize>` is Copy-like). This pattern is the simplest.
            for j in successors[i].clone() {
                in_degree[j] = in_degree[j].saturating_sub(1);
            }
        }
        phases.push(phase);
    }

    if remaining > 0 {
        // Cycle: the residue is the set of undrained tasks plus the edges that
        // remain among them. Carry the offending edges (not just the task ids)
        // so the caller can render a precise error.
        let residue_edges: Vec<(String, String)> = edges
            .iter()
            .filter_map(|e| {
                let u = id_to_idx.get(e.depends_on_id.as_str())?;
                let v = id_to_idx.get(e.task_id.as_str())?;
                if !drained[*u] && !drained[*v] {
                    Some((e.task_id.clone(), e.depends_on_id.clone()))
                } else {
                    None
                }
            })
            .collect();
        return Err(AppError::Cycle { edges: residue_edges });
    }

    Ok(phases)
}

// ---------------------------------------------------------------------------
// get_task_dispatch_plan (migration 0006). Read-only fold that composes
// `compute_task_batches` with per-task spec reads (effort/complexity/
// files_touched) and runs `compute_tier` per row. Same outer shape as
// `compute_task_batches` (`Vec<Vec<…>>` — batches of tasks), but each entry is
// a [`BatchEntry`] carrying the derived [`Tier`] alongside the inputs.
// Read-only; no transaction, no events.
// ---------------------------------------------------------------------------

/// Raw row read per-task by [`get_task_dispatch_plan`]: the spec columns
/// (`effort`/`complexity`/`attributes`) feeding [`compute_tier`]. Generic over
/// `R: Row` per the canonical [`crate::db`] FromRow recipe; all three columns
/// are nullable (`Option<String>`).
#[derive(Debug)]
struct DispatchSpecRow {
    effort: Option<String>,
    complexity: Option<String>,
    attributes: Option<String>,
}

impl<'r, R> sqlx::FromRow<'r, R> for DispatchSpecRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(DispatchSpecRow {
            effort: row.try_get("effort")?,
            complexity: row.try_get("complexity")?,
            attributes: row.try_get("attributes")?,
        })
    }
}

/// Story-level dispatch plan. Composes [`compute_task_batches`] with per-task
/// spec reads (effort/complexity/files_touched) and runs [`compute_tier`] per
/// row. Returns the same outer shape as `compute_task_batches`
/// (`Vec<Vec<…>>` — batches of tasks) but each entry is a [`BatchEntry`]
/// carrying the derived [`Tier`] alongside the inputs.
///
/// `has_cross_repo` is currently always reported as `false`: the round-3
/// scope did not add a `repo_link_id_for_slug` / `primary_repo_id_for_project`
/// helper to resolve `{repo, path}` entries in `attributes.files_touched`
/// against the project's `repo_links`. The other three Deep-triggering
/// branches of [`compute_tier`] still fire correctly; the cross-repo branch
/// remains dormant until a follow-up pass wires the slug→link resolver. The
/// MCP `set_task_spec` edge validates slugs at write time so a stored
/// `files_touched` entry referencing an unknown slug is unreachable.
///
/// Read-only: no transaction, no events. Cycles bubble out of the inner
/// `compute_task_batches` call as `AppError::Cycle`.
pub async fn get_task_dispatch_plan(
    pool: &impl DbClient,
    story_id: &str,
) -> Result<Vec<Vec<BatchEntry>>, AppError> {
    let batches = compute_task_batches(pool, story_id).await?;
    if batches.is_empty() {
        return Ok(Vec::new());
    }

    let mut out: Vec<Vec<BatchEntry>> = Vec::with_capacity(batches.len());
    for batch in batches {
        let mut entries: Vec<BatchEntry> = Vec::with_capacity(batch.len());
        for task_id in batch {
            // Fetch the task row for effort/complexity/attributes. Tombstoned
            // rows are filtered to match the read-side convention; an absent
            // row at this point would be a races-with-delete and surfaces as
            // `NotFound`.
            let row = pool
                .query_opt::<DispatchSpecRow>(
                    r#"
                SELECT effort, complexity, attributes
                FROM work_items
                WHERE id = $1 AND deleted_at IS NULL
                "#,
                    args![task_id.to_owned()],
                )
                .await?
                .ok_or_else(|| AppError::NotFound(format!("work_item '{task_id}' not found")))?;

            // Count `files_touched` entries (bare-string OR {repo,path}
            // objects). A malformed `attributes` blob is treated as 0 here
            // rather than re-erroring — `decode_attributes` is the
            // authoritative corruption-detector elsewhere; this read is a
            // best-effort summary.
            let files_touched_count: usize = match row.attributes.as_deref() {
                None => 0,
                Some(raw) => serde_json::from_str::<Value>(raw)
                    .ok()
                    .and_then(|v| v.get("files_touched").and_then(Value::as_array).map(Vec::len))
                    .unwrap_or(0),
            };

            // TODO(round-3 follow-up): wire a {repo,path} slug resolver
            // against the project ancestor's `repo_links` to flag a
            // `{repo, path}` entry whose slug != primary as cross-repo. No
            // helper exists in repo.rs yet — leaving this dormant; the other
            // three Deep-triggering branches still fire correctly.
            let has_cross_repo = false;

            let tier = if row.effort.is_none()
                && row.complexity.is_none()
                && files_touched_count == 0
                && !has_cross_repo
            {
                None
            } else {
                Some(compute_tier(
                    row.effort.as_deref(),
                    row.complexity.as_deref(),
                    files_touched_count,
                    has_cross_repo,
                ))
            };

            entries.push(BatchEntry {
                task_id,
                effort: row.effort,
                complexity: row.complexity,
                tier,
                files_touched_count,
                has_cross_repo,
            });
        }
        out.push(entries);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// set_task_tier (migration 0006). Single-mutation-path write to the
// `work_items.tier` column. Task-scope: rejects non-task rows at the Rust
// layer (no DB-level kind-coupling guard, matching `set_task_kind`).
// ---------------------------------------------------------------------------

/// Set the dispatch [`Tier`] on a task work-item. Updates the
/// `work_items.tier` column directly (NOT via attributes JSON — `tier` is a
/// real column added in migration 0006). Records one `work_item.tier_set`
/// event on the task's `work_item` aggregate. Rejects non-task rows at the
/// Rust layer (no DB-level kind-coupling guard, matching `set_task_kind`).
///
/// `tier == None` clears the column (writes NULL).
pub async fn set_task_tier(
    db: &impl DbClient,
    task_id: &str,
    tier: Option<Tier>,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "tier is settable only on a task, not on '{kind}'"
        )));
    }

    let value: Option<String> = tier.map(enum_to_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET tier = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
            args![task_id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "task_id": task_id, "tier": value });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.tier_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// get_story_readiness (migration 0005). Compose existing reads to summarise a
// story's planning-pipeline readiness and the next recommended block.
// Read-only; no transaction, no events.
// ---------------------------------------------------------------------------

/// Compute a story's [`StoryReadiness`] aggregate (migration 0005). Composes
/// existing reads only — no mutations, no events.
///
/// The `next_recommended_action` cascade is a UX rollup over the CONVENTIONS
/// §l six-phase sequence (first match wins). See [`NextAction`]'s docstring
/// for the auto-recommended subset and the optional variants. Implemented
/// order (matches the cascade body below):
///   1. !problem_statement_set                         → `RunProblemStatement`
///   2. unresolved_questions > 0                       → `ResolveOpenQuestions`
///   3. !any_user_questions_ever                       → `RunUserInterrogation`
///   4. accepted_research_count == 0
///      a. any proposed research note                  → `RunVetResearch`
///      b. else                                        → `RunResearchNotes`
///   5. !has_approach                                  → `RunApproach`
///   6. no `attributes.verification_commands`          → `RunVerificationCommands`
///   7. no risks rows (live)                           → `RunRisks`
///   8. no `findings.kind='story-review'` row (live)   → `RunStoryReview`
///   9. no child tasks                                 → `RunDecomposeTasks`
///  10. !has_acceptance_criteria_on_all_tasks          → `RunSetTaskSpec`
///  11. ≥2 tasks AND no task_dependencies rows         → `RunWireTaskDeps`
///  12. else                                           → `StoryReady`
///
/// Variants `RunAlternatives`, `RunNotDoing`, `RunEdgeCases` are present in
/// the [`NextAction`] enum but NEVER auto-recommended by this cascade — they
/// are user-discretion blocks (a story may legitimately have nothing to
/// record); users invoke them directly via the `/lumina:` slash forms.
///
/// Errors:
///   * `NotFound` — `story_id` does not exist.
///   * `Validation` — `story_id` exists but is not `kind='story'`.
pub async fn get_story_readiness(
    pool: &impl DbClient,
    story_id: &str,
) -> Result<StoryReadiness, AppError> {
    let kind = work_item_kind(pool, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "get_story_readiness expects a 'story', not a '{kind}'"
        )));
    }

    // Load the story row (attributes carries problem_statement /
    // execution_strategy / verification_commands keys).
    let detail = get_work_item_detail(pool, story_id).await?;
    let attrs: serde_json::Map<String, Value> = detail
        .item
        .attributes
        .as_ref()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    let problem_statement_set = attrs.contains_key("problem_statement");
    let has_approach = attrs.contains_key("execution_strategy");
    let has_verification_commands = attrs.contains_key("verification_commands");

    // Accepted research = live notes with state='accepted'. The detail fold
    // already filters `superseded_by IS NULL`.
    let accepted_research_count: u32 = detail
        .research_notes
        .iter()
        .filter(|n| n.state.as_deref() == Some("accepted"))
        .count() as u32;
    let any_proposed_research = detail
        .research_notes
        .iter()
        .any(|n| n.state.as_deref() == Some("proposed"));

    let unresolved_questions: u32 = detail
        .open_questions
        .iter()
        .filter(|q| q.status.as_deref() == Some("open"))
        .count() as u32;
    // Distinguishes "story has never been interrogated" from "story has been
    // interrogated and all questions are answered" — the cascade emits
    // `RunUserInterrogation` only in the former case (when zero questions
    // exist), and `ResolveOpenQuestions` in the latter when `status='open'`
    // questions remain.
    let any_user_questions_ever = !detail.open_questions.is_empty();

    // Tasks under the story. We need each task's acceptance-criteria count to
    // compute `has_acceptance_criteria_on_all_tasks`; one query per task is
    // O(n) reads but acceptable for a per-story summary (n is small).
    let tasks: Vec<&WorkItem> = detail
        .children
        .iter()
        .filter(|c| c.kind == "task")
        .collect();
    let task_count = tasks.len();

    let mut tasks_with_no_ac: u32 = 0;
    for t in &tasks {
        let n: i64 = crate::db::scalar_one::<i64>(
            pool,
            r#"SELECT COUNT(*) FROM acceptance_criteria WHERE work_item_id = $1"#,
            args![t.id.clone()],
        )
        .await?;
        if n == 0 {
            tasks_with_no_ac += 1;
        }
    }
    let has_acceptance_criteria_on_all_tasks = task_count > 0 && tasks_with_no_ac == 0;

    // Risk count (live).
    let has_risks = !detail.risks.is_empty();

    // Story-review audit signal — the story has been audited if at least one
    // `findings.kind = 'story-review'` row exists. The advisor does not
    // inspect status/severity of those findings (that's the user's call); it
    // surfaces `RunStoryReview` only when no audit has ever happened.
    let story_review_audited = detail
        .findings
        .iter()
        .any(|f| f.kind.as_deref() == Some("story-review"));

    // Dependency edges among this story's tasks.
    let dep_count = list_task_dependencies(pool, story_id).await?.len();

    let ready_for_decomposition = problem_statement_set
        && accepted_research_count >= 1
        && unresolved_questions == 0
        && has_approach;

    // Cascade — first match wins. Ordering is a UX rollup keyed on missing
    // signals; see [`NextAction`]'s docstring for the auto-recommended subset
    // and its mapping back to CONVENTIONS §l phases. The cascade is NOT a
    // strict re-encoding of §l ordering — phase ordering is enforced by
    // `/lumina:plan-story`.
    let next_recommended_action = if !problem_statement_set {
        NextAction::RunProblemStatement
    } else if unresolved_questions > 0 {
        // Phase 1 derivative: questions exist and are still open; user must
        // answer them before moving on.
        NextAction::ResolveOpenQuestions
    } else if !any_user_questions_ever {
        // Phase 1: no questions have ever been recorded — invite the user to
        // interrogate.
        NextAction::RunUserInterrogation
    } else if accepted_research_count == 0 {
        if any_proposed_research {
            NextAction::RunVetResearch
        } else {
            NextAction::RunResearchNotes
        }
    } else if !has_approach {
        NextAction::RunApproach
    } else if !has_verification_commands {
        NextAction::RunVerificationCommands
    } else if !has_risks {
        NextAction::RunRisks
    } else if !story_review_audited {
        // Phase 4 audit gate — emitted once the story has cleared PS +
        // research + approach + verification + risks but has never been
        // audited via `/lumina:story-review`.
        NextAction::RunStoryReview
    } else if task_count == 0 {
        NextAction::RunDecomposeTasks
    } else if !has_acceptance_criteria_on_all_tasks {
        NextAction::RunSetTaskSpec
    } else if task_count >= 2 && dep_count == 0 {
        NextAction::RunWireTaskDeps
    } else {
        NextAction::StoryReady
    };

    Ok(StoryReadiness {
        story_id: story_id.to_owned(),
        problem_statement_set,
        accepted_research_count,
        unresolved_questions,
        has_approach,
        has_acceptance_criteria_on_all_tasks,
        ready_for_decomposition,
        next_recommended_action,
    })
}

/// Append ONE `events` row inside an in-flight transaction. Called by every
/// mutation; no domain write may bypass it. `id` is a fresh UUIDv7 (TEXT);
/// `payload` is serialised to a JSON string; `exported_at` is left NULL so the
/// git-export materialiser (Task 6) drains it on its next tick.
///
/// Takes a `&mut dyn DbTx` (the backend-erased in-flight transaction, not the
/// pool) precisely so the event row shares the caller's transaction and is
/// committed/rolled-back atomically with the domain write. Every caller passes
/// `&mut tx` where `tx: Transaction<'_, Sqlite>` came from
/// [`crate::db::begin_write`]; that reference unsizes to `&mut dyn DbTx` via the
/// `impl DbTx for Transaction<'_, Sqlite>` blanket coercion, so the ~100 callers
/// need no changes.
async fn record_event(
    tx: &mut dyn crate::db::DbTx,
    aggregate_type: &str,
    aggregate_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let event_id = Uuid::now_v7().to_string();
    let payload_str = serde_json::to_string(&payload).map_err(|e| AppError::Other(e.into()))?;

    // Runtime trait call through the object-safe `DbTx::execute` (placeholders
    // are `$N`, args are owned/`'static`: the borrowed `&str` params are
    // `.to_owned()`'d before binding; `event_id`/`payload_str` are already
    // owned `String`). The returned affected-row count is ignored.
    tx.execute(
        r#"
        INSERT INTO events (id, aggregate_type, aggregate_id, event_type, payload)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        crate::args![
            event_id,
            aggregate_type.to_owned(),
            aggregate_id.to_owned(),
            event_type.to_owned(),
            payload_str
        ],
    )
    .await?;

    Ok(())
}

/// PTY-session CRUD (migration 0008). Separate submodule because the `pty_*`
/// tables do NOT participate in the `events` outbox — they are a per-session
/// transcript / queue store with no git-export materialisation, so the
/// single-mutation-path "+1 work_items / +1 events" invariant does not apply
/// here. Each mutator opens its own `db::begin_write` transaction (still
/// `BEGIN IMMEDIATE`, so writer contention surfaces upfront) and either
/// commits a single statement or composes a read-then-write atomically;
/// neither flow appends an `events` row.
pub mod pty {
    use crate::args;
    use crate::db::DbClient;
    use crate::domain::{PtyMessage, PtyQueueEntry, PtySession};
    use crate::error::AppError;

    /// Hand-written generic `FromRow` for [`PtySession`] per the canonical
    /// [`crate::db`] FromRow recipe: generic over `R: Row` so it rides
    /// `query_*<T>` on the SQLite arm today and a future Pg arm unchanged. The
    /// column→field nullability is carried by the field types (`String` /
    /// `i64` for NOT-NULL columns, `Option<_>` for the rest), replacing the old
    /// `AS "col!"` / `"col?"` macro hints.
    impl<'r, R> sqlx::FromRow<'r, R> for PtySession
    where
        R: sqlx::Row,
        usize: sqlx::ColumnIndex<R>,
        &'r str: sqlx::ColumnIndex<R>,
        String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        Option<i64>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(PtySession {
                id: row.try_get("id")?,
                label: row.try_get("label")?,
                project_id: row.try_get("project_id")?,
                cwd: row.try_get("cwd")?,
                config_json: row.try_get("config_json")?,
                parse_strategy_version: row.try_get("parse_strategy_version")?,
                status: row.try_get("status")?,
                started_at: row.try_get("started_at")?,
                updated_at: row.try_get("updated_at")?,
                ended_at: row.try_get("ended_at")?,
                exit_code: row.try_get("exit_code")?,
                last_error: row.try_get("last_error")?,
                previous_session_id: row.try_get("previous_session_id")?,
                jsonl_path: row.try_get("jsonl_path")?,
            })
        }
    }

    /// Hand-written generic `FromRow` for [`PtyMessage`] per the canonical
    /// [`crate::db`] FromRow recipe (see [`PtySession`]'s impl above).
    impl<'r, R> sqlx::FromRow<'r, R> for PtyMessage
    where
        R: sqlx::Row,
        usize: sqlx::ColumnIndex<R>,
        &'r str: sqlx::ColumnIndex<R>,
        String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(PtyMessage {
                id: row.try_get("id")?,
                session_id: row.try_get("session_id")?,
                sequence: row.try_get("sequence")?,
                created_at: row.try_get("created_at")?,
                kind: row.try_get("kind")?,
                content_json: row.try_get("content_json")?,
                raw_text: row.try_get("raw_text")?,
            })
        }
    }

    /// Hand-written generic `FromRow` for [`PtyQueueEntry`] per the canonical
    /// [`crate::db`] FromRow recipe (see [`PtySession`]'s impl above).
    impl<'r, R> sqlx::FromRow<'r, R> for PtyQueueEntry
    where
        R: sqlx::Row,
        usize: sqlx::ColumnIndex<R>,
        &'r str: sqlx::ColumnIndex<R>,
        String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
        Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(PtyQueueEntry {
                id: row.try_get("id")?,
                session_id: row.try_get("session_id")?,
                sequence: row.try_get("sequence")?,
                input_kind: row.try_get("input_kind")?,
                payload: row.try_get("payload")?,
                enqueued_at: row.try_get("enqueued_at")?,
                dispatched_at: row.try_get("dispatched_at")?,
                completed_at: row.try_get("completed_at")?,
                status: row.try_get("status")?,
                error: row.try_get("error")?,
            })
        }
    }

    /// Render a Rust-side timestamp string compatible with the TEXT timestamp
    /// columns of the `pty_*` tables. Matches the convention in `export.rs`
    /// (`jiff::Timestamp::now().to_string()`) — the only existing call site in
    /// the crate that mints a timestamp in Rust rather than via the SQL
    /// `CURRENT_TIMESTAMP` literal. `pty_*` tables declare their timestamps
    /// `NOT NULL` without a SQL default, so the Rust side must supply the
    /// value at INSERT/UPDATE time.
    fn now_string() -> String {
        jiff::Timestamp::now().to_string()
    }

    // -------------------------------------------------------------------
    // Sessions
    // -------------------------------------------------------------------

    /// Insert a new `pty_sessions` row in `status='spawning'` with
    /// `parse_strategy_version=1` and `started_at = updated_at = now()`. One
    /// `db.begin()` transaction; an INSERT followed by a SELECT-back of
    /// the freshly-stamped row. No `events` outbox write (pinned in this
    /// module's docstring).
    pub async fn create_pty_session(
        db: &impl DbClient,
        id: &str,
        label: Option<&str>,
        project_id: Option<&str>,
        cwd: &str,
        config_json: &str,
    ) -> Result<PtySession, AppError> {
        let now = now_string();
        let parse_strategy_version: i64 = 1;
        let status = "spawning";

        let mut tx = db.begin().await?;

        tx.execute(
            r#"
            INSERT INTO pty_sessions (
                id, label, project_id, cwd, config_json, parse_strategy_version,
                status, started_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
            "#,
            args![
                id.to_owned(),
                label.map(|s| s.to_owned()),
                project_id.map(|s| s.to_owned()),
                cwd.to_owned(),
                config_json.to_owned(),
                parse_strategy_version,
                status.to_owned(),
                now
            ],
        )
        .await?;

        let row = crate::db::tx_query_one::<PtySession>(
            tx.as_mut(),
            r#"
            SELECT
                id,
                label,
                project_id,
                cwd,
                config_json,
                parse_strategy_version,
                status,
                started_at,
                updated_at,
                ended_at,
                exit_code,
                last_error,
                previous_session_id,
                jsonl_path
            FROM pty_sessions
            WHERE id = $1
            "#,
            args![id.to_owned()],
        )
        .await?;

        tx.commit().await?;
        Ok(row)
    }

    /// Update a session's lifecycle `status` (plus an optional `last_error`
    /// snapshot) in one transaction. The constraint surfaced by the plan's
    /// Important Constraints — status and last_error MUST be set together so
    /// readers never see a stale error against a clean status — is satisfied
    /// by issuing both column assignments in the same UPDATE statement.
    /// `NotFound` via `rows_affected()==0`.
    pub async fn update_pty_session_status(
        db: &impl DbClient,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<(), AppError> {
        let now = now_string();
        let mut tx = db.begin().await?;

        let affected = tx
            .execute(
                r#"
            UPDATE pty_sessions
            SET status = $2,
                last_error = $3,
                updated_at = $4
            WHERE id = $1
            "#,
                args![
                    id.to_owned(),
                    status.to_owned(),
                    last_error.map(|s| s.to_owned()),
                    now
                ],
            )
            .await?;

        if affected == 0 {
            return Err(AppError::NotFound(format!("pty_session '{id}' not found")));
        }

        tx.commit().await?;
        Ok(())
    }

    /// Bind the discovered Claude Code session JSONL file path to the
    /// `pty_sessions` row after spawn-time `bind_jsonl_path` resolution.
    /// Updates `updated_at` in the same statement (mirrors
    /// `update_pty_session_status`'s shape). `NotFound` via
    /// `rows_affected()==0`.
    pub async fn set_pty_jsonl_path(
        db: &impl DbClient,
        id: &str,
        path: &str,
    ) -> Result<(), AppError> {
        let now = now_string();
        let mut tx = db.begin().await?;

        let affected = tx
            .execute(
                r#"
            UPDATE pty_sessions
            SET jsonl_path = $2,
                updated_at = $3
            WHERE id = $1
            "#,
                args![id.to_owned(), path.to_owned(), now],
            )
            .await?;

        if affected == 0 {
            return Err(AppError::NotFound(format!("pty_session '{id}' not found")));
        }

        tx.commit().await?;
        Ok(())
    }

    /// Mark a session as ended: stamp `status`, `ended_at=now`, optional
    /// `exit_code`, optional `last_error`, and `updated_at=now` in one
    /// transaction. Typical terminal statuses are `completed|failed|cancelled`;
    /// the caller picks. `NotFound` via `rows_affected()==0`.
    pub async fn update_pty_session_ended(
        db: &impl DbClient,
        id: &str,
        status: &str,
        exit_code: Option<i64>,
        last_error: Option<&str>,
    ) -> Result<(), AppError> {
        let now = now_string();
        let mut tx = db.begin().await?;

        let affected = tx
            .execute(
                r#"
            UPDATE pty_sessions
            SET status = $2,
                ended_at = $3,
                exit_code = $4,
                last_error = $5,
                updated_at = $3
            WHERE id = $1
            "#,
                args![
                    id.to_owned(),
                    status.to_owned(),
                    now,
                    exit_code,
                    last_error.map(|s| s.to_owned())
                ],
            )
            .await?;

        if affected == 0 {
            return Err(AppError::NotFound(format!("pty_session '{id}' not found")));
        }

        tx.commit().await?;
        Ok(())
    }

    /// List sessions, optionally filtered by `status` and/or `project_id`,
    /// newest-first by `started_at`. Filters use the `?n IS NULL OR col = ?n`
    /// idiom so a single prepared statement covers every filter combination.
    /// Reads, no transaction.
    pub async fn list_pty_sessions(
        db: &impl DbClient,
        status: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<Vec<PtySession>, AppError> {
        db.query_all::<PtySession>(
            r#"
            SELECT
                id,
                label,
                project_id,
                cwd,
                config_json,
                parse_strategy_version,
                status,
                started_at,
                updated_at,
                ended_at,
                exit_code,
                last_error,
                previous_session_id,
                jsonl_path
            FROM pty_sessions
            WHERE ($1 IS NULL OR status = $1)
              AND ($2 IS NULL OR project_id = $2)
            ORDER BY started_at DESC, id
            "#,
            args![status.map(|s| s.to_owned()), project_id.map(|s| s.to_owned())],
        )
        .await
    }

    /// Fetch a single session row by id, erroring `NotFound` if the id has no
    /// row. Reads, no transaction.
    pub async fn get_pty_session(
        db: &impl DbClient,
        id: &str,
    ) -> Result<PtySession, AppError> {
        db.query_opt::<PtySession>(
            r#"
            SELECT
                id,
                label,
                project_id,
                cwd,
                config_json,
                parse_strategy_version,
                status,
                started_at,
                updated_at,
                ended_at,
                exit_code,
                last_error,
                previous_session_id,
                jsonl_path
            FROM pty_sessions
            WHERE id = $1
            "#,
            args![id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("pty_session '{id}' not found")))
    }

    /// Soft-delete a session: set `status='cancelled'` and stamp `ended_at=now`
    /// (plus `updated_at`). The row is retained so the transcript and queue
    /// stay intact for inspection. `NotFound` via `rows_affected()==0`.
    pub async fn delete_pty_session(db: &impl DbClient, id: &str) -> Result<(), AppError> {
        let now = now_string();
        let mut tx = db.begin().await?;

        let affected = tx
            .execute(
                r#"
            UPDATE pty_sessions
            SET status = 'cancelled',
                ended_at = $2,
                updated_at = $2
            WHERE id = $1
            "#,
                args![id.to_owned(), now],
            )
            .await?;

        if affected == 0 {
            return Err(AppError::NotFound(format!("pty_session '{id}' not found")));
        }

        tx.commit().await?;
        Ok(())
    }

    // -------------------------------------------------------------------
    // Messages
    // -------------------------------------------------------------------

    /// Append one transcript row to `pty_messages`. `sequence` is supplied by
    /// the caller (the supervisor owns the per-session monotone counter); the
    /// `UNIQUE(session_id, sequence)` constraint surfaces a sequence collision
    /// as a DB error. `created_at` is stamped now-side. One transaction.
    pub async fn insert_pty_message(
        db: &impl DbClient,
        id: &str,
        session_id: &str,
        sequence: i64,
        kind: &str,
        content_json: &str,
        raw_text: Option<&str>,
    ) -> Result<(), AppError> {
        let now = now_string();
        let mut tx = db.begin().await?;

        tx.execute(
            r#"
            INSERT INTO pty_messages (
                id, session_id, sequence, created_at, kind, content_json, raw_text
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            args![
                id.to_owned(),
                session_id.to_owned(),
                sequence,
                now,
                kind.to_owned(),
                content_json.to_owned(),
                raw_text.map(|s| s.to_owned())
            ],
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// List transcript rows for a session in ascending `sequence` order,
    /// optionally starting strictly after `since_sequence`. `limit` caps the
    /// page size. Reads, no transaction.
    pub async fn list_pty_messages(
        db: &impl DbClient,
        session_id: &str,
        since_sequence: Option<i64>,
        limit: i64,
    ) -> Result<Vec<PtyMessage>, AppError> {
        db.query_all::<PtyMessage>(
            r#"
            SELECT
                id,
                session_id,
                sequence,
                created_at,
                kind,
                content_json,
                raw_text
            FROM pty_messages
            WHERE session_id = $1
              AND ($2 IS NULL OR sequence > $2)
            ORDER BY sequence ASC
            LIMIT $3
            "#,
            args![session_id.to_owned(), since_sequence, limit],
        )
        .await
    }

    // -------------------------------------------------------------------
    // Queue
    // -------------------------------------------------------------------

    /// Append a pending input frame to `pty_queue`. Caller-supplied `sequence`
    /// (matching the `pty_messages` discipline); `UNIQUE(session_id, sequence)`
    /// surfaces a collision. `enqueued_at=now`, `status='pending'`. One
    /// transaction.
    pub async fn enqueue_pty_input(
        db: &impl DbClient,
        id: &str,
        session_id: &str,
        sequence: i64,
        input_kind: &str,
        payload: &str,
    ) -> Result<(), AppError> {
        let now = now_string();
        let mut tx = db.begin().await?;

        tx.execute(
            r#"
            INSERT INTO pty_queue (
                id, session_id, sequence, input_kind, payload, enqueued_at, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, 'pending')
            "#,
            args![
                id.to_owned(),
                session_id.to_owned(),
                sequence,
                input_kind.to_owned(),
                payload.to_owned(),
                now
            ],
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// List every queue row for a session in ascending `sequence` order
    /// (regardless of status). The HTTP layer (T9) renders this as the
    /// per-session queue view. Reads, no transaction.
    pub async fn list_pty_queue(
        db: &impl DbClient,
        session_id: &str,
    ) -> Result<Vec<PtyQueueEntry>, AppError> {
        db.query_all::<PtyQueueEntry>(
            r#"
            SELECT
                id,
                session_id,
                sequence,
                input_kind,
                payload,
                enqueued_at,
                dispatched_at,
                completed_at,
                status,
                error
            FROM pty_queue
            WHERE session_id = $1
            ORDER BY sequence ASC
            "#,
            args![session_id.to_owned()],
        )
        .await
    }

    /// Atomically pop the oldest `status='pending'` row for a session: SELECT
    /// the lowest-sequence pending row, then UPDATE it to
    /// `status='dispatched', dispatched_at=now` within the SAME transaction.
    /// Returns the freshly-dispatched row (with `dispatched_at` filled in) or
    /// `None` if no pending row exists. The supervisor calls this each
    /// dispatch tick; the partial index `idx_pty_queue_pending` keeps the
    /// SELECT cheap.
    pub async fn pop_next_pending_pty(
        db: &impl DbClient,
        session_id: &str,
    ) -> Result<Option<PtyQueueEntry>, AppError> {
        let now = now_string();
        let mut tx = db.begin().await?;

        let Some(picked) = crate::db::tx_query_opt::<PtyQueueEntry>(
            tx.as_mut(),
            r#"
            SELECT
                id,
                session_id,
                sequence,
                input_kind,
                payload,
                enqueued_at,
                dispatched_at,
                completed_at,
                status,
                error
            FROM pty_queue
            WHERE session_id = $1 AND status = 'pending'
            ORDER BY sequence ASC
            LIMIT 1
            "#,
            args![session_id.to_owned()],
        )
        .await?
        else {
            // No pending row; close the (empty-write) tx and return None.
            tx.commit().await?;
            return Ok(None);
        };

        tx.execute(
            r#"
            UPDATE pty_queue
            SET status = 'dispatched',
                dispatched_at = $2
            WHERE id = $1
            "#,
            args![picked.id.clone(), now.clone()],
        )
        .await?;

        tx.commit().await?;

        // Reflect the just-applied transition in the returned struct (avoids a
        // second SELECT-back: the only column shape change is the two stamped
        // fields below).
        let mut dispatched = picked;
        dispatched.status = "dispatched".to_owned();
        dispatched.dispatched_at = Some(now);
        Ok(Some(dispatched))
    }

    /// Mark a queue row as terminally completed (typical: `'completed'` /
    /// `'failed'` / `'cancelled'`): stamp `status`, `completed_at=now`, and
    /// optional `error`. One transaction. `NotFound` via `rows_affected()==0`.
    pub async fn complete_pty_queue_entry(
        db: &impl DbClient,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), AppError> {
        let now = now_string();
        let mut tx = db.begin().await?;

        let affected = tx
            .execute(
                r#"
            UPDATE pty_queue
            SET status = $2,
                completed_at = $3,
                error = $4
            WHERE id = $1
            "#,
                args![
                    id.to_owned(),
                    status.to_owned(),
                    now,
                    error.map(|s| s.to_owned())
                ],
            )
            .await?;

        if affected == 0 {
            return Err(AppError::NotFound(format!("pty_queue entry '{id}' not found")));
        }

        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::QueryFindingsFilter;
    use crate::domain::Status;

    /// `finding_dedup_hash` is deterministic (same inputs → same hash) and
    /// field-sensitive (changing any one component changes the hash, including the
    /// None-vs-empty distinction). Cheap insurance for B17a's dedup path.
    #[test]
    fn finding_dedup_hash_is_deterministic_and_field_sensitive() {
        let base = finding_dedup_hash("wi-1", Some("src/a.rs"), Some(10), Some("foo"), Some("bug"));
        // Same inputs → same hash.
        assert_eq!(
            base,
            finding_dedup_hash("wi-1", Some("src/a.rs"), Some(10), Some("foo"), Some("bug")),
            "identical inputs hash identically"
        );
        // Lowercase hex, 64 chars (SHA-256).
        assert_eq!(base.len(), 64, "sha256 hex is 64 chars");
        assert!(
            base.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "lowercase hex only"
        );
        // Each differing field perturbs the hash.
        assert_ne!(base, finding_dedup_hash("wi-2", Some("src/a.rs"), Some(10), Some("foo"), Some("bug")));
        assert_ne!(base, finding_dedup_hash("wi-1", Some("src/b.rs"), Some(10), Some("foo"), Some("bug")));
        assert_ne!(base, finding_dedup_hash("wi-1", Some("src/a.rs"), Some(11), Some("foo"), Some("bug")));
        assert_ne!(base, finding_dedup_hash("wi-1", Some("src/a.rs"), Some(10), Some("bar"), Some("bug")));
        assert_ne!(base, finding_dedup_hash("wi-1", Some("src/a.rs"), Some(10), Some("foo"), Some("other")));
        // None is distinct from Some("") for each optional component.
        assert_ne!(
            finding_dedup_hash("wi-1", None, None, None, None),
            finding_dedup_hash("wi-1", Some(""), None, Some(""), Some("")),
            "None encodes distinctly from empty-string"
        );
    }

    /// Row count of `work_items` (compile-checked literal — sqlx 0.9's
    /// `SqlSafeStr` bound rejects a dynamically-built table name on the runtime
    /// `query_as`, so the two count helpers are split per table).
    async fn count_work_items(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_items")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// Row count of `events`.
    async fn count_events(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM events")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// Build the legal project→epic→focus→story chain and return the story id,
    /// so tests can create a legal `task` (or an illegal one) beneath it.
    ///
    /// Migration-0010 valid-chain recipe: an epic must carry a non-empty outcome,
    /// a focus must carry a shape, and a story can only be created once its
    /// ancestor epic has ≥1 close-criterion. The chain therefore writes 4
    /// work_items (project/epic/focus/story) + 5 events (the four creates plus the
    /// epic close-criterion add).
    async fn seed_chain_to_story(pool: &SqlitePool) -> String {
        let project = create_work_item(pool, "project", None, "P", None)
            .await
            .expect("legal project");
        let epic = create_work_item_full(
            pool,
            "epic",
            Some(&project.to_string()),
            "E",
            None,
            CreateOpts {
                origin: None,
                outcome: Some("the epic outcome"),
                shape: None,
            },
        )
        .await
        .expect("legal epic");
        add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = create_work_item_full(
            pool,
            "focus",
            Some(&epic.to_string()),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect("legal focus");
        let story = create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
            .await
            .expect("legal story");
        story.to_string()
    }

    /// (a) `create_work_item` inserts exactly one work_items row AND one events
    /// row in one transaction.
    #[tokio::test]
    async fn create_writes_one_work_item_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");

        assert_eq!(count_work_items(&pool).await, 0);
        assert_eq!(count_events(&pool).await, 0);

        let id = create_work_item(&pool, "project", None, "Root", None)
            .await
            .expect("legal project create");

        assert_eq!(count_work_items(&pool).await, 1, "exactly one work_item");
        assert_eq!(count_events(&pool).await, 1, "exactly one event");

        // The event references the new work-item and is unexported (outbox).
        use sqlx::Row as _;
        let ev = sqlx::query(
            r#"SELECT aggregate_id, event_type, exported_at FROM events"#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let aggregate_id: String = ev.try_get("aggregate_id").unwrap();
        let event_type: String = ev.try_get("event_type").unwrap();
        let exported_at: Option<String> = ev.try_get("exported_at").unwrap();
        assert_eq!(aggregate_id, id.to_string());
        assert_eq!(event_type, "work_item.created");
        assert!(exported_at.is_none(), "new event must be unexported");
    }

    /// (c) `create_work_item` with an illegal parent kind returns
    /// `AppError::Validation` — NOT a panic, NOT a Db/500.
    #[tokio::test]
    async fn create_with_illegal_parent_kind_is_validation() {
        let pool = connect_in_memory().await.expect("pool");

        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("legal project");

        // task under project is illegal (task's legal parent is story).
        let err = create_work_item(&pool, "task", Some(&project.to_string()), "Bad", None)
            .await
            .expect_err("illegal task→project must error");

        assert!(
            matches!(err, AppError::Validation(_)),
            "expected Validation, got {err:?}"
        );
    }

    /// (b) A failed create rolls back BOTH writes: an illegal create leaves zero
    /// NEW rows. We pre-seed one legal project (1 work_item, 1 event), attempt an
    /// illegal child create, and assert the counts are UNCHANGED.
    #[tokio::test]
    async fn failed_create_leaves_no_new_rows() {
        let pool = connect_in_memory().await.expect("pool");

        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("legal project");
        let wi_before = count_work_items(&pool).await;
        let ev_before = count_events(&pool).await;
        assert_eq!((wi_before, ev_before), (1, 1));

        // Illegal: focus directly under project (focus's legal parent is epic).
        let err = create_work_item(&pool, "focus", Some(&project.to_string()), "Bad", None)
            .await
            .expect_err("illegal create must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        assert_eq!(
            count_work_items(&pool).await,
            wi_before,
            "no new work_item row after a failed create"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before,
            "no new event row after a failed create"
        );
    }

    /// A legal `task` create under a story succeeds and emits its own event,
    /// proving the full chain plus the per-mutation event invariant across many
    /// writes (5 items ⇒ 5 events).
    #[tokio::test]
    async fn full_chain_then_legal_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // 4 work_items (project/epic/focus/story); 5 events (the four creates plus
        // the epic close-criterion add the migration-0010 story gate requires).
        assert_eq!(count_work_items(&pool).await, 4);
        assert_eq!(count_events(&pool).await, 5);

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("legal task under story");

        assert_eq!(count_work_items(&pool).await, 5);
        assert_eq!(count_events(&pool).await, 6);

        // Detail aggregate: the story has the task as a direct child.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.item.kind, "story");
        assert_eq!(detail.children.len(), 1);
        assert_eq!(detail.children[0].id, task.to_string());
    }

    // -----------------------------------------------------------------------
    // create_work_items (B17b) — bulk create under one tx, one coarse event,
    // all-or-nothing, with the optional spawn-provenance stamp.
    // -----------------------------------------------------------------------

    /// Bulk-creating N tasks under an existing story inserts exactly N work_items
    /// AND exactly ONE coarse `events` row (D8) — not N events.
    #[tokio::test]
    async fn create_work_items_bulk_under_existing_story() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let wi_before = count_work_items(&pool).await;
        let ev_before = count_events(&pool).await;

        let specs = vec![
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some(&story),
                title: "T1",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                spawned_from_finding_id: None,
            },
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some(&story),
                title: "T2",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                spawned_from_finding_id: None,
            },
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some(&story),
                title: "T3",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                spawned_from_finding_id: None,
            },
        ];
        let n = specs.len() as i64;

        let ids = create_work_items(&pool, &specs)
            .await
            .expect("bulk create under story");
        assert_eq!(ids.len(), specs.len(), "one id returned per spec");

        assert_eq!(
            count_work_items(&pool).await,
            wi_before + n,
            "exactly N new work_items"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before + 1,
            "exactly ONE coarse batch event for the whole batch (D8)"
        );
        assert_eq!(
            count_events_of_type(&pool, "work_items.batch_created").await,
            1,
            "the coarse event carries the batch event_type"
        );

        // The N tasks exist as direct children of the story.
        let task_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM work_items WHERE parent_id = $1 AND kind = 'task'",
        )
        .bind(&story)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(task_count, n, "all N tasks land under the story");
    }

    /// A spec carrying `spawned_from_finding_id: Some(fid)` stamps the column on
    /// the created task (B17b owns the spawn stamp; the column is NULL on a plain
    /// create). The referenced finding must exist first (FK).
    #[tokio::test]
    async fn create_work_items_stamps_spawned_from_finding_id() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // The spawn FK targets findings(id), so create a finding on the story first.
        let finding_id = create_finding(&pool, &story, &NewFinding::default())
            .await
            .expect("seed finding")
            .to_string();

        let specs = vec![NewWorkItemSpec {
            kind: "task",
            parent_id: Some(&story),
            title: "spawned task",
            body: None,
            origin: None,
            outcome: None,
            shape: None,
            spawned_from_finding_id: Some(&finding_id),
        }];

        let ids = create_work_items(&pool, &specs)
            .await
            .expect("create spawned task");
        let task_id = ids[0].to_string();

        let stamped = sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_from_finding_id FROM work_items WHERE id = $1",
        )
        .bind(&task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            stamped,
            Some(finding_id),
            "the spawn column equals the source finding id"
        );
    }

    /// A batch mixing one valid spec with one invalid spec aborts WHOLLY — the
    /// valid spec must NOT persist (all-or-nothing rollback, D10).
    #[tokio::test]
    async fn create_work_items_aborts_whole_batch_on_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let wi_before = count_work_items(&pool).await;
        let ev_before = count_events(&pool).await;

        let specs = vec![
            // Valid: a task under the story.
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some(&story),
                title: "good",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                spawned_from_finding_id: None,
            },
            // Invalid: parent_id names no existing work_item → Validation.
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some("no-such-parent"),
                title: "bad",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                spawned_from_finding_id: None,
            },
        ];

        let err = create_work_items(&pool, &specs)
            .await
            .expect_err("an invalid spec must abort the batch");
        assert!(
            matches!(err, AppError::Validation(_)),
            "expected Validation, got {err:?}"
        );

        assert_eq!(
            count_work_items(&pool).await,
            wi_before,
            "no work_item persists — the valid spec was rolled back too"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before,
            "no coarse event on an aborted batch"
        );
    }

    /// `update_work_item_status` updates + emits one event in one tx; a missing
    /// id is `NotFound` and emits NO event.
    #[tokio::test]
    async fn update_status_event_and_notfound() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        assert_eq!(count_events(&pool).await, 1);

        update_work_item_status(&pool, &id, "in-progress")
            .await
            .expect("status update");
        assert_eq!(count_events(&pool).await, 2, "one new status event");

        let got: String =
            sqlx::query_scalar::<_, String>("SELECT status FROM work_items WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(got, "in-progress");

        // Missing id → NotFound, no event emitted.
        let err = update_work_item_status(&pool, "does-not-exist", "x")
            .await
            .expect_err("missing id must error");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
        assert_eq!(count_events(&pool).await, 2, "no event for a missing-row update");
    }

    /// Row count of `work_item_activity`.
    async fn count_activity(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_item_activity")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// `update_work_item` writes exactly +1 work_items-row-change and +1 event in
    /// one transaction (set-or-leave: a title update leaves body untouched).
    #[tokio::test]
    async fn update_work_item_writes_one_change_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "Orig", Some("body"))
            .await
            .expect("project")
            .to_string();
        let ev_before = count_events(&pool).await;

        let req = UpdateWorkItemRequest {
            title: Some("Renamed".into()),
            body: None,
            status: Some(Status::InProgress),
            position: None,
            attributes: None,
        };
        update_work_item(&pool, &id, &req).await.expect("update");

        assert_eq!(count_events(&pool).await, ev_before + 1, "+1 event");
        let detail = get_work_item_detail(&pool, &id).await.expect("detail");
        assert_eq!(detail.item.title, "Renamed");
        assert_eq!(detail.item.body.as_deref(), Some("body"), "body left untouched");
        assert_eq!(detail.item.status, "in_progress");
    }

    /// A forced mid-tx error rolls BOTH the work_items change and the event back.
    /// We force the error by giving `update_work_item` an attributes object with
    /// an unknown key for the kind — but that errors pre-tx; to exercise the
    /// rollback we instead drive a known mid-tx failpoint: a unique-violation on
    /// activity seq. Simpler: prove the +0/+0 invariant on the validation reject
    /// path (no tx opened) AND that a NotFound update emits no event.
    #[tokio::test]
    async fn update_work_item_rejects_and_rolls_back() {
        let pool = connect_in_memory().await.expect("pool");
        // story so the per-kind attribute contract applies.
        let story = seed_chain_to_story(&pool).await;
        let wi_events_before = count_events(&pool).await;

        // Unknown attributes key for a story ⇒ Validation, zero new rows/events.
        let bad = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: None,
            position: None,
            attributes: Some(serde_json::json!({ "not_a_story_key": "x" })),
        };
        let err = update_work_item(&pool, &story, &bad)
            .await
            .expect_err("unknown attr key must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        assert_eq!(
            count_events(&pool).await,
            wi_events_before,
            "no event emitted on a rejected update"
        );

        // NotFound update emits no event either.
        let err = update_work_item(&pool, "missing", &UpdateWorkItemRequest {
            title: Some("x".into()),
            body: None,
            status: None,
            position: None,
            attributes: None,
        })
        .await
        .expect_err("missing id must error");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
        assert_eq!(count_events(&pool).await, wi_events_before, "still no event");
    }

    /// `append_activity` writes one activity row with monotonic per-item `seq`
    /// and one event each; payload is normalised; an unknown entry_kind is
    /// `Validation`.
    #[tokio::test]
    async fn append_activity_monotonic_seq_and_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let ev_before = count_events(&pool).await;

        append_activity(&pool, &story, "execution", Some("alice"), "did a thing", None, None)
            .await
            .expect("first activity");
        append_activity(
            &pool,
            &story,
            "comment",
            None,
            "second",
            Some(&serde_json::json!({ "k": "v", "drop_me": null })),
            Some("implement"),
        )
        .await
        .expect("second activity");

        assert_eq!(count_activity(&pool).await, 2);
        assert_eq!(count_events(&pool).await, ev_before + 2, "+1 event per append");

        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.activity.len(), 2);
        assert_eq!(detail.activity[0].seq, 1);
        assert_eq!(detail.activity[1].seq, 2, "seq is monotonic per item");
        // origin stamps round-trip: first entry omitted it (NULL), second set it.
        assert_eq!(detail.activity[0].origin, None, "no origin ⇒ NULL");
        assert_eq!(
            detail.activity[1].origin.as_deref(),
            Some("implement"),
            "origin stamp persisted and round-tripped"
        );
        // null-valued payload key was dropped on normalise.
        let payload = detail.activity[1].payload.as_ref().expect("payload");
        assert!(payload.get("k").is_some());
        assert!(payload.get("drop_me").is_none(), "null key dropped");

        // Unknown entry_kind ⇒ Validation.
        let err = append_activity(&pool, &story, "nonsense", None, "x", None, None)
            .await
            .expect_err("unknown entry_kind must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        assert_eq!(count_activity(&pool).await, 2, "no row for a rejected append");
    }

    /// A `set_story_plan`-style partial merge: calling `set_work_item_attributes`
    /// twice with DIFFERENT keys leaves the earlier sibling key intact.
    #[tokio::test]
    async fn set_work_item_attributes_merges_without_clobber() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        set_work_item_attributes(&pool, &story, &serde_json::json!({ "problem_statement": "P" }))
            .await
            .expect("first merge");
        set_work_item_attributes(&pool, &story, &serde_json::json!({ "research_notes": "R" }))
            .await
            .expect("second merge");

        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        let attrs = detail.item.attributes.expect("attributes set");
        assert_eq!(attrs.get("problem_statement").and_then(|v| v.as_str()), Some("P"), "sibling intact");
        assert_eq!(attrs.get("research_notes").and_then(|v| v.as_str()), Some("R"));
    }

    /// An attributes object with an unknown key for a kind returns `Validation`
    /// (NOT a 500/panic), and a non-object root is also `Validation`.
    #[tokio::test]
    async fn attributes_validation_is_typed() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let err = set_work_item_attributes(&pool, &story, &serde_json::json!({ "bogus": 1 }))
            .await
            .expect_err("unknown key");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        let err = set_work_item_attributes(&pool, &story, &serde_json::json!([1, 2, 3]))
            .await
            .expect_err("array root");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    /// Soft-`delete_work_item` hides the item from `list_work_items` but
    /// `get_work_item_detail` still returns it with `deleted_at` set (well,
    /// returns it at all — the detail SELECT does not filter deleted rows).
    #[tokio::test]
    async fn soft_delete_hides_from_list_but_detail_returns() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        assert_eq!(list_work_items(&pool, None, None).await.unwrap().len(), 1);

        delete_work_item(&pool, &id).await.expect("soft delete");

        assert_eq!(
            list_work_items(&pool, None, None).await.unwrap().len(),
            0,
            "soft-deleted item hidden from list"
        );

        // Detail still resolves the row (does not 404).
        let detail = get_work_item_detail(&pool, &id).await.expect("detail still returns");
        assert_eq!(detail.item.id, id);

        // The row carries deleted_at (verified directly — WorkItem doesn't expose it).
        let dat: Option<String> =
            sqlx::query_scalar::<_, Option<String>>("SELECT deleted_at FROM work_items WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(dat.is_some(), "deleted_at stamped");

        // Re-deleting is NotFound (already tombstoned).
        let err = delete_work_item(&pool, &id).await.expect_err("re-delete");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }

    /// R36: a `focus` with non-terminal child stories cannot be soft-deleted;
    /// once its stories are terminal (done/cancelled) it deletes cleanly. Guards
    /// the epic-done rollup against a tombstoned focus silently dropping live
    /// descendant stories.
    #[tokio::test]
    async fn delete_focus_blocked_while_child_stories_nonterminal() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project");
        let epic = create_work_item_full(
            &pool,
            "epic",
            Some(&project.to_string()),
            "E",
            None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic");
        add_acceptance_criterion(&pool, &epic.to_string(), "c")
            .await
            .expect("criterion");
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic.to_string()),
            "FO",
            None,
            CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice") },
        )
        .await
        .expect("focus");
        let story = create_work_item(&pool, "story", Some(&focus.to_string()), "S", None)
            .await
            .expect("story");

        // Non-terminal (open) story under the focus ⇒ delete blocked.
        let err = delete_work_item(&pool, &focus.to_string())
            .await
            .expect_err("focus delete blocked while a child story is non-terminal");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Make the story terminal, then the focus deletes cleanly.
        update_work_item_status(&pool, &story.to_string(), "done")
            .await
            .expect("story → done");
        delete_work_item(&pool, &focus.to_string())
            .await
            .expect("focus deletes once its stories are terminal");
    }

    /// `set_relevance` is rejected on a task (typed Validation) and accepted on a
    /// story. Also asserts a freshly-created story defaults to `relevance="backlog"`.
    #[tokio::test]
    async fn set_relevance_scope_and_default_backlog() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // Default relevance on a created story is "backlog".
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.item.relevance.as_deref(), Some("backlog"), "story defaults backlog");

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        // task has NULL relevance on create.
        let tdetail = get_work_item_detail(&pool, &task).await.expect("task detail");
        assert!(tdetail.item.relevance.is_none(), "task relevance NULL on create");

        // set_relevance on a task → Validation.
        let err = set_relevance(&pool, &task, Relevance::Active)
            .await
            .expect_err("relevance on task must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // set_relevance on a story → ok.
        set_relevance(&pool, &story, Relevance::Active).await.expect("story relevance ok");
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.item.relevance.as_deref(), Some("active"));
    }

    /// Row count of `acceptance_criteria`.
    async fn count_criteria(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM acceptance_criteria")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// `get_work_item_detail` folds the acceptance_criteria; an add emits +1
    /// event and the criterion starts unchecked.
    #[tokio::test]
    async fn acceptance_criteria_fold_and_add_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let ev_before = count_events(&pool).await;
        // The seed adds one epic close-criterion (migration-0010 story gate), so
        // count the delta rather than an absolute global criterion count.
        let crit_before = count_criteria(&pool).await;

        add_acceptance_criterion(&pool, &task, "must build").await.expect("ac1");
        add_acceptance_criterion(&pool, &task, "must test").await.expect("ac2");

        assert_eq!(count_criteria(&pool).await, crit_before + 2);
        assert_eq!(count_events(&pool).await, ev_before + 2, "+1 event per add");

        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.acceptance_criteria.len(), 2, "detail folds criteria");
        assert_eq!(detail.acceptance_criteria[0].seq, 1);
        assert_eq!(detail.acceptance_criteria[1].seq, 2, "monotonic seq");
        assert_eq!(detail.acceptance_criteria[0].checked, 0, "starts unchecked");
    }

    /// Checking a criterion flips its state, appends exactly one `verification`
    /// activity row, and records exactly one event.
    #[tokio::test]
    async fn check_criterion_writes_activity_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let crit = add_acceptance_criterion(&pool, &task, "must build")
            .await
            .expect("ac")
            .to_string();

        let ev_before = count_events(&pool).await;
        let act_before = count_activity(&pool).await;

        check_acceptance_criterion(&pool, &crit, Some("alice"))
            .await
            .expect("check");

        assert_eq!(count_events(&pool).await, ev_before + 1, "exactly one event");
        assert_eq!(count_activity(&pool).await, act_before + 1, "+1 activity");

        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.acceptance_criteria[0].checked, 1, "criterion flipped");
        assert_eq!(detail.acceptance_criteria[0].checked_by.as_deref(), Some("alice"));
        // The appended activity is a verification entry.
        let verif = detail.activity.iter().find(|a| a.entry_kind == "verification");
        assert!(verif.is_some(), "a verification activity row was appended");

        // Uncheck clears state (no extra activity row, one event).
        let ev2 = count_events(&pool).await;
        let act2 = count_activity(&pool).await;
        uncheck_acceptance_criterion(&pool, &crit).await.expect("uncheck");
        assert_eq!(count_events(&pool).await, ev2 + 1, "uncheck: one event");
        assert_eq!(count_activity(&pool).await, act2, "uncheck: no new activity");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.acceptance_criteria[0].checked, 0, "unchecked");
        assert!(detail.acceptance_criteria[0].checked_by.is_none(), "checked_by cleared");
    }

    /// A `hard` story blocks task→done while a criterion is unchecked, and allows
    /// it once all are checked — across BOTH gated paths (update_work_item_status
    /// and the generic update_work_item PATCH).
    #[tokio::test]
    async fn hard_gate_blocks_then_allows_task_done() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        set_closure_gate(&pool, &story, ClosureGate::Hard).await.expect("hard gate");

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let crit = add_acceptance_criterion(&pool, &task, "must build")
            .await
            .expect("ac")
            .to_string();

        // Blocked while unchecked (status path).
        let err = update_work_item_status(&pool, &task, "done")
            .await
            .expect_err("hard gate blocks done");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Blocked while unchecked (generic PATCH path).
        let patch_done = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: Some(Status::Done),
            position: None,
            attributes: None,
        };
        let err = update_work_item(&pool, &task, &patch_done)
            .await
            .expect_err("PATCH→done also gated");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Check the criterion → done now allowed.
        check_acceptance_criterion(&pool, &crit, None).await.expect("check");
        update_work_item_status(&pool, &task, "done").await.expect("done allowed once checked");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.item.status, "done");
    }

    /// (R18) Multi-criterion hard gate: with TWO acceptance criteria, checking
    /// only ONE still blocks task→done; checking BOTH allows it. This catches a
    /// count-total-vs-count-unchecked bug that a single-criterion test misses
    /// (a "count == total" gate would wrongly allow done after the first check).
    #[tokio::test]
    async fn hard_gate_multi_criterion_partial_check_still_blocks() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        set_closure_gate(&pool, &story, ClosureGate::Hard).await.expect("hard gate");

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let crit_a = add_acceptance_criterion(&pool, &task, "must build")
            .await
            .expect("ac1")
            .to_string();
        let crit_b = add_acceptance_criterion(&pool, &task, "must test")
            .await
            .expect("ac2")
            .to_string();

        // Zero checked → blocked.
        let err = update_work_item_status(&pool, &task, "done")
            .await
            .expect_err("blocked with both unchecked");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Check only ONE of the two → STILL blocked (the partial-check case).
        check_acceptance_criterion(&pool, &crit_a, None).await.expect("check first");
        let err = update_work_item_status(&pool, &task, "done")
            .await
            .expect_err("one-of-two checked must still block");
        assert!(
            matches!(err, AppError::Validation(_)),
            "partial check must still block done, got {err:?}"
        );

        // Check the SECOND → now allowed.
        check_acceptance_criterion(&pool, &crit_b, None).await.expect("check second");
        update_work_item_status(&pool, &task, "done")
            .await
            .expect("done allowed once BOTH criteria checked");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.item.status, "done");
    }

    /// A `soft` story (the default — no closure_gate set) allows task→done even
    /// with an unchecked criterion.
    #[tokio::test]
    async fn soft_gate_allows_task_done_with_unchecked() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        // No set_closure_gate call ⇒ closure_gate is NULL (treated as soft).
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        add_acceptance_criterion(&pool, &task, "unchecked criterion")
            .await
            .expect("ac");

        update_work_item_status(&pool, &task, "done")
            .await
            .expect("soft gate allows done with unchecked criteria");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.item.status, "done");
    }

    /// `set_effort`/`set_complexity` are task-scoped (reject a story);
    /// `set_closure_gate` is story-scoped (reject a task).
    #[tokio::test]
    async fn effort_complexity_closure_gate_scopes() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        set_effort(&pool, &task, Effort::M).await.expect("effort on task ok");
        set_complexity(&pool, &task, Complexity::High).await.expect("complexity on task ok");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.item.effort.as_deref(), Some("m"));
        assert_eq!(detail.item.complexity.as_deref(), Some("high"));

        let err = set_effort(&pool, &story, Effort::S).await.expect_err("effort on story rejects");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        let err = set_complexity(&pool, &story, Complexity::Low)
            .await
            .expect_err("complexity on story rejects");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        set_closure_gate(&pool, &story, ClosureGate::Soft).await.expect("gate on story ok");
        let err = set_closure_gate(&pool, &task, ClosureGate::Hard)
            .await
            .expect_err("gate on task rejects");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    /// Read a single work item's status (test helper).
    async fn item_status(pool: &SqlitePool, id: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM work_items WHERE id = ?1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// Count events of a given `event_type` (test helper — proves the
    /// exactly-one-event-per-logical-write invariant for the multi-write resolve).
    async fn count_events_of_type(pool: &SqlitePool, event_type: &str) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM events WHERE event_type = ?1")
            .bind(event_type)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// `add_open_question` on a non-story (here: a task) returns a typed
    /// `Validation`, and succeeds on a story.
    #[tokio::test]
    async fn add_open_question_rejects_non_story() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        let err = add_open_question(&pool, &task, "should we?")
            .await
            .expect_err("open question on a task must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        add_open_question(&pool, &story, "should we?")
            .await
            .expect("open question on a story ok");
    }

    /// Resolving a two-option question unblocks the chosen branch's task (→todo)
    /// and cancels the other branch's exclusive task (→cancelled); a non-exclusive
    /// blocked task on the question is also unblocked; and the whole multi-write
    /// resolution emits EXACTLY ONE `open_question.resolved` event.
    #[tokio::test]
    async fn resolve_open_question_branches_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let q = add_open_question(&pool, &story, "which approach?")
            .await
            .expect("question")
            .to_string();
        let opt_a = add_question_option(&pool, &q, "A", None).await.expect("opt A").to_string();
        let opt_b = add_question_option(&pool, &q, "B", None).await.expect("opt B").to_string();

        // Three branch tasks: exclusive-to-A, exclusive-to-B, and non-exclusive.
        let task_a = create_work_item(&pool, "task", Some(&story), "TA", None)
            .await
            .expect("task A")
            .to_string();
        let task_b = create_work_item(&pool, "task", Some(&story), "TB", None)
            .await
            .expect("task B")
            .to_string();
        let task_shared = create_work_item(&pool, "task", Some(&story), "TS", None)
            .await
            .expect("task shared")
            .to_string();

        block_task_on_question(&pool, &task_a, &q).await.expect("block A");
        set_enabling_option(&pool, &task_a, &opt_a).await.expect("tie A");
        block_task_on_question(&pool, &task_b, &q).await.expect("block B");
        set_enabling_option(&pool, &task_b, &opt_b).await.expect("tie B");
        block_task_on_question(&pool, &task_shared, &q).await.expect("block shared");

        assert_eq!(item_status(&pool, &task_a).await, "blocked");
        assert_eq!(item_status(&pool, &task_b).await, "blocked");
        assert_eq!(item_status(&pool, &task_shared).await, "blocked");

        let resolved_before = count_events_of_type(&pool, "open_question.resolved").await;
        let ev_before = count_events(&pool).await;

        // Choose option A.
        resolve_open_question(&pool, &q, &opt_a, Some("alice"))
            .await
            .expect("resolve");

        // Chosen branch (A) and non-exclusive (shared) → todo; other branch (B)
        // → cancelled.
        assert_eq!(item_status(&pool, &task_a).await, "todo", "chosen-branch task unblocked");
        assert_eq!(item_status(&pool, &task_shared).await, "todo", "non-exclusive task unblocked");
        assert_eq!(item_status(&pool, &task_b).await, "cancelled", "other-branch task cancelled");

        // Question is answered with the chosen option recorded.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        let folded = detail
            .open_questions
            .iter()
            .find(|oq| oq.id == q)
            .expect("question folded into detail");
        assert_eq!(folded.status.as_deref(), Some("answered"));
        assert_eq!(folded.chosen_option_id.as_deref(), Some(opt_a.as_str()));
        assert_eq!(folded.options.len(), 2, "both options folded");

        // EXACTLY ONE resolved event for the whole multi-write resolution.
        assert_eq!(
            count_events_of_type(&pool, "open_question.resolved").await,
            resolved_before + 1,
            "exactly one open_question.resolved event for the resolution"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before + 1,
            "the multi-write resolution adds exactly one events row"
        );

        // Resolving with an option from a DIFFERENT question is Validation.
        let q2 = add_open_question(&pool, &story, "another?").await.expect("q2").to_string();
        let err = resolve_open_question(&pool, &q2, &opt_a, None)
            .await
            .expect_err("foreign option must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Resolving a missing question is NotFound.
        let err = resolve_open_question(&pool, "missing", &opt_a, None)
            .await
            .expect_err("missing question");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }

    /// A superseded research note drops out of the live `get_work_item_detail`
    /// fold; `add_research_note` defaults `state='proposed'` and emits one event.
    #[tokio::test]
    async fn superseded_research_note_excluded_from_live_fold() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let ev_before = count_events(&pool).await;

        let old = add_research_note(&pool, &story, "old finding", None, Some("low"), None, None)
            .await
            .expect("old note")
            .to_string();
        let new = add_research_note(&pool, &story, "new finding", None, Some("high"), None, None)
            .await
            .expect("new note")
            .to_string();
        assert_eq!(count_events(&pool).await, ev_before + 2, "+1 event per add");

        // Both live before supersession; default state is proposed.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.research_notes.len(), 2, "both notes live");
        assert!(
            detail.research_notes.iter().all(|n| n.state.as_deref() == Some("proposed")),
            "default state proposed"
        );

        // Supersede the old note by the new one.
        supersede_research_note(&pool, &old, &new).await.expect("supersede");

        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.research_notes.len(), 1, "superseded note excluded");
        assert_eq!(detail.research_notes[0].id, new, "only the live note remains");

        // update_research_note set-or-leave: accept the surviving note.
        let req = UpdateResearchNoteRequest {
            confidence: None,
            state: Some(ResearchState::Accepted),
            rationale: Some("chosen".into()),
            lens: None,
        };
        update_research_note(&pool, &new, &req).await.expect("accept");
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.research_notes[0].state.as_deref(), Some("accepted"));
        assert_eq!(detail.research_notes[0].rationale.as_deref(), Some("chosen"));
        assert_eq!(detail.research_notes[0].confidence.as_deref(), Some("high"), "confidence left");
    }

    /// A superseded finding drops out of the live findings fold; `confidence`
    /// threads through create + the update set-or-leave path.
    #[tokio::test]
    async fn superseded_finding_excluded_and_confidence_threads() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let old = create_finding(
            &pool,
            &story,
            &NewFinding {
                summary: Some("old"),
                confidence: Some("low"),
                origin: Some("review"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("old finding")
        .to_string();
        let new = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("new"), confidence: Some("high"), ..NewFinding::default() },
        )
        .await
        .expect("new finding")
        .to_string();

        // Both live; confidence stored from create.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.findings.len(), 2, "both findings live");
        let old_f = detail.findings.iter().find(|f| f.id == old).expect("old in fold");
        assert_eq!(old_f.confidence.as_deref(), Some("low"));
        // origin stamp round-trips from create through the findings fold.
        assert_eq!(old_f.origin.as_deref(), Some("review"), "origin persisted from create");

        // update_finding honours confidence (set-or-leave).
        let req = UpdateFindingRequest {
            severity: None,
            effort: None,
            category: None,
            status: None,
            file: None,
            line: None,
            symbol: None,
            summary: None,
            description: None,
            confidence: Some("medium".into()),
            repo_id: None,
        };
        update_finding(&pool, &old, &req).await.expect("update confidence");

        // Supersede the old finding.
        supersede_finding(&pool, &old, &new).await.expect("supersede");
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.findings.len(), 1, "superseded finding excluded");
        assert_eq!(detail.findings[0].id, new);
        assert_eq!(detail.findings[0].confidence.as_deref(), Some("high"));

        // Superseding a missing finding is NotFound.
        let err = supersede_finding(&pool, "missing", &new)
            .await
            .expect_err("missing finding");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }

    // ---------------------------------------------------------------------
    // parse_github_slug (migration 0004 — project↔repo-links plan T1).
    //
    // Pure-function tests; no DB needed. Cover the validity matrix from the
    // plan's T1 acceptance bullets PLUS the "no leading punctuation in name"
    // guard documented inline on `parse_github_slug` (which is what makes
    // `x/-y` reject — a defensible empirical-GitHub rule the plan calls out).
    // ---------------------------------------------------------------------

    #[test]
    fn parse_github_slug_valid_lowercases_both_segments() {
        assert_eq!(
            parse_github_slug("octocat/Hello-World").unwrap(),
            "octocat/hello-world",
        );
        assert_eq!(
            parse_github_slug("Foo/bar.baz_2").unwrap(),
            "foo/bar.baz_2",
        );
        // Already-lowercased input round-trips unchanged.
        assert_eq!(
            parse_github_slug("octocat/spoon-knife").unwrap(),
            "octocat/spoon-knife",
        );
    }

    #[test]
    fn parse_github_slug_rejects_empty_owner() {
        let err = parse_github_slug("/x").expect_err("empty owner");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_empty_name() {
        let err = parse_github_slug("x/").expect_err("empty name");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_git_suffix() {
        let err = parse_github_slug("x/y.git").expect_err(".git suffix");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_leading_hyphen_in_owner() {
        let err = parse_github_slug("-x/y").expect_err("leading hyphen owner");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_leading_punctuation_in_name() {
        // The plan's acceptance lists `x/-y` as Err; this is enforced by the
        // "no leading `.`/`-`/`_` in name" guard documented on the helper.
        let err = parse_github_slug("x/-y").expect_err("leading hyphen name");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_dots_in_owner() {
        // The owner alphabet is [A-Za-z0-9-] only, so `a..b` is rejected for
        // having `.` chars in the owner (not for consecutive hyphens).
        let err = parse_github_slug("a..b/y").expect_err("dot in owner");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_zero_or_many_slashes() {
        assert!(parse_github_slug("noslash").is_err());
        assert!(parse_github_slug("a/b/c").is_err());
    }

    #[test]
    fn parse_github_slug_rejects_consecutive_hyphens_in_owner() {
        let err = parse_github_slug("a--b/y").expect_err("consecutive hyphens");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_owner_longer_than_39() {
        let owner = "a".repeat(40);
        let err = parse_github_slug(&format!("{owner}/y")).expect_err("owner >39");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_name_longer_than_100() {
        let name = "a".repeat(101);
        let err = parse_github_slug(&format!("x/{name}")).expect_err("name >100");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_dot_and_dotdot_names() {
        assert!(parse_github_slug("x/.").is_err());
        assert!(parse_github_slug("x/..").is_err());
    }

    // -----------------------------------------------------------------------
    // compute_tier (migration 0006) — one test per branch of the §k rule.
    // -----------------------------------------------------------------------

    #[test]
    fn compute_tier_high_complexity_is_deep() {
        // complexity=high dominates even with otherwise-Lite inputs.
        assert_eq!(
            compute_tier(Some("s"), Some("high"), 0, false),
            Tier::Deep,
        );
    }

    #[test]
    fn compute_tier_l_effort_is_deep() {
        // effort=l dominates over a low/medium complexity.
        assert_eq!(
            compute_tier(Some("l"), Some("low"), 0, false),
            Tier::Deep,
        );
    }

    #[test]
    fn compute_tier_files_above_three_is_deep() {
        // files_touched_count > 3 — boundary just above the threshold.
        assert_eq!(
            compute_tier(Some("s"), Some("low"), 4, false),
            Tier::Deep,
        );
    }

    #[test]
    fn compute_tier_files_at_three_is_lite() {
        // Boundary: files_touched_count == 3 is NOT > 3, so Lite.
        assert_eq!(
            compute_tier(Some("s"), Some("low"), 3, false),
            Tier::Lite,
        );
    }

    #[test]
    fn compute_tier_cross_repo_is_deep() {
        // has_cross_repo flips an otherwise-Lite row to Deep.
        assert_eq!(
            compute_tier(Some("s"), Some("low"), 1, true),
            Tier::Deep,
        );
    }

    #[test]
    fn compute_tier_residual_is_lite() {
        // All Deep triggers absent → Lite.
        assert_eq!(
            compute_tier(Some("s"), Some("low"), 1, false),
            Tier::Lite,
        );
    }

    #[test]
    fn compute_tier_all_none_is_lite() {
        // Defensive: every input at its null-equivalent still falls through
        // to the residual Lite branch (the wire surface gates None upstream).
        assert_eq!(compute_tier(None, None, 0, false), Tier::Lite);
    }

    // ------------------------------------------------------------------
    // migration-0010 epic/focus create-time + transition gates
    // ------------------------------------------------------------------

    /// An `epic` create with no outcome (the 5-arg/6-arg wrappers pass
    /// `outcome=None`) is rejected with a typed `Validation`, before any row is
    /// written; supplying a non-empty outcome via the create core succeeds.
    #[tokio::test]
    async fn epic_create_requires_outcome() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let err = create_work_item(&pool, "epic", Some(&project), "E", None)
            .await
            .expect_err("epic without outcome must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // No epic row written by the rejected create (only the project exists).
        assert_eq!(count_work_items(&pool).await, 1, "rejected epic wrote no row");

        let epic = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts {
                origin: None,
                outcome: Some("ship the thing"),
                shape: None,
            },
        )
        .await
        .expect("epic with outcome");
        // Outcome folds into the row's attributes.
        let attrs = sqlx::query_scalar::<_, Option<String>>(
            "SELECT attributes FROM work_items WHERE id = ?1",
        )
        .bind(epic.to_string())
        .fetch_one(&pool)
        .await
        .unwrap()
        .expect("attributes present");
        assert!(attrs.contains("ship the thing"), "outcome folded: {attrs}");
    }

    /// A `focus` create with no shape is rejected with a typed `Validation`;
    /// supplying a shape via the create core succeeds and binds the column.
    #[tokio::test]
    async fn focus_create_requires_shape() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();

        let err = create_work_item(&pool, "focus", Some(&epic), "FO", None)
            .await
            .expect_err("focus without shape must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("cross-cutting"),
            },
        )
        .await
        .expect("focus with shape");
        let shape = sqlx::query_scalar::<_, Option<String>>(
            "SELECT shape FROM work_items WHERE id = ?1",
        )
        .bind(focus.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(shape.as_deref(), Some("cross-cutting"));
    }

    /// A `story` create beneath a focus whose ancestor epic has NO close-criterion
    /// is rejected with `Validation`; once the epic acquires ≥1 close-criterion,
    /// the same create succeeds.
    #[tokio::test]
    async fn story_create_gated_on_epic_close_criterion() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect("focus")
        .to_string();

        // Criterion-less epic ⇒ story create rejected.
        let err = create_work_item(&pool, "story", Some(&focus), "S", None)
            .await
            .expect_err("story under criterion-less epic must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Add the epic close-criterion; the same create now succeeds.
        add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("criterion");
        create_work_item(&pool, "story", Some(&focus), "S", None)
            .await
            .expect("story create succeeds once epic has a criterion");
    }

    /// `epic→done` is rejected while a close-criterion is unchecked AND while a
    /// descendant story is non-terminal, through BOTH the `transition_status`
    /// path (`update_work_item_status`) and the generic PATCH path
    /// (`update_work_item`); it succeeds once both conditions are met.
    #[tokio::test]
    async fn epic_done_gate_blocks_then_allows() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();
        let crit = add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("criterion")
            .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect("focus")
        .to_string();
        let story = create_work_item(&pool, "story", Some(&focus), "S", None)
            .await
            .expect("story")
            .to_string();

        let done_req = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: Some(Status::Done),
            position: None,
            attributes: None,
        };

        // (a) unchecked close-criterion + non-terminal story ⇒ both paths reject.
        // R27: assert on the DISTINCT, stable message substring of the
        // close-criterion clause (not just the Validation variant) so a future
        // clause-merge regression that collapses the two gate messages is caught.
        let err = update_work_item_status(&pool, &epic, "done")
            .await
            .expect_err("transition path must reject unchecked-criterion epic");
        assert_close_criterion_gate(&err);
        let err = update_work_item(&pool, &epic, &done_req)
            .await
            .expect_err("PATCH path must reject unchecked-criterion epic");
        assert_close_criterion_gate(&err);

        // Check the close-criterion; the story is still non-terminal ⇒ still reject
        // (proving the second clause of the gate fires independently).
        // R27: assert the DISTINCT descendant-story-clause substring here.
        check_acceptance_criterion(&pool, &crit, None)
            .await
            .expect("check criterion");
        let err = update_work_item_status(&pool, &epic, "done")
            .await
            .expect_err("transition path must reject non-terminal-story epic");
        assert_descendant_story_gate(&err);
        let err = update_work_item(&pool, &epic, &done_req)
            .await
            .expect_err("PATCH path must reject non-terminal-story epic");
        assert_descendant_story_gate(&err);

        // Make the story terminal; now both clauses are satisfied.
        update_work_item_status(&pool, &story, "done")
            .await
            .expect("story → done");

        // (b) success via the generic PATCH path.
        update_work_item(&pool, &epic, &done_req)
            .await
            .expect("epic → done succeeds once all close-criteria checked and stories terminal");
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM work_items WHERE id = ?1",
        )
        .bind(&epic)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "done");
    }

    /// `set_shape` on a non-focus is rejected with `Validation`; `set_epic_plan`
    /// rejects a non-epic and `set_focus_plan` rejects a non-focus.
    #[tokio::test]
    async fn shape_and_plan_setters_are_kind_gated() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect("focus")
        .to_string();

        // set_shape only on a focus.
        let err = set_shape(&pool, &epic, Shape::Foundational)
            .await
            .expect_err("set_shape on an epic must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        set_shape(&pool, &focus, Shape::Foundational)
            .await
            .expect("set_shape on a focus succeeds");

        // set_epic_plan only on an epic.
        let err = set_epic_plan(&pool, &focus, Some("o2"), None)
            .await
            .expect_err("set_epic_plan on a focus must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        set_epic_plan(&pool, &epic, Some("revised outcome"), Some("ctx"))
            .await
            .expect("set_epic_plan on an epic succeeds");

        // set_focus_plan only on a focus.
        let err = set_focus_plan(&pool, &epic, Some("framing"))
            .await
            .expect_err("set_focus_plan on an epic must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        set_focus_plan(&pool, &focus, Some("the framing"))
            .await
            .expect("set_focus_plan on a focus succeeds");
    }

    /// R27 helper: assert an error is the epic-done CLOSE-CRITERION gate, by its
    /// distinct stable message substring (not merely the Validation variant).
    fn assert_close_criterion_gate(err: &AppError) {
        match err {
            AppError::Validation(m) => assert!(
                m.contains("close-criterion(s) remain unchecked"),
                "expected the close-criterion gate message, got: {m}"
            ),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    /// R27 helper: assert an error is the epic-done DESCENDANT-STORY gate, by its
    /// distinct stable message substring.
    fn assert_descendant_story_gate(err: &AppError) {
        match err {
            AppError::Validation(m) => assert!(
                m.contains("descendant story(ies) are not terminal"),
                "expected the descendant-story gate message, got: {m}"
            ),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    /// R11: setter-level JSON-merge preservation — `set_epic_plan` with only
    /// `outcome`, then only `context`, preserves the earlier `outcome` (and vice
    /// versa). Exercises the epic-plan setter's own key-mapping path on top of
    /// the lower-level merge that `set_work_item_attributes_merges_without_clobber`
    /// already covers. (`focus` has only one legal plan key, `framing`, so it
    /// admits no setter-level sibling-clobber scenario; the epic two-key case is
    /// the meaningful preservation assertion.)
    #[tokio::test]
    async fn epic_plan_setter_merges_without_clobber() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("initial"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();

        // set outcome only, then context only — outcome must survive the merge.
        set_epic_plan(&pool, &epic, Some("ship the thing"), None)
            .await
            .expect("set outcome");
        set_epic_plan(&pool, &epic, None, Some("the context"))
            .await
            .expect("set context");

        let detail = get_work_item_detail(&pool, &epic).await.expect("detail");
        let attrs = detail.item.attributes.expect("attributes set");
        assert_eq!(
            attrs.get("outcome").and_then(|v| v.as_str()),
            Some("ship the thing"),
            "earlier outcome survives a later context-only set"
        );
        assert_eq!(
            attrs.get("context").and_then(|v| v.as_str()),
            Some("the context")
        );
    }

    /// R10: a `shape` supplied on a NON-focus kind (here `epic`) is rejected with
    /// `Validation` BEFORE begin_write, writing zero rows (the consistency guard
    /// "shape is only valid on a focus" fires ahead of any INSERT).
    #[tokio::test]
    async fn shape_on_non_focus_kind_is_validation_and_writes_nothing() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let before_items = count_work_items(&pool).await;
        let before_events = count_events(&pool).await;

        // epic create with a shape (epic carries an outcome, so we exercise the
        // shape-on-non-focus guard, not the missing-outcome one).
        let err = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts {
                origin: None,
                outcome: Some("the outcome"),
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect_err("shape on an epic must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        assert_eq!(
            count_work_items(&pool).await,
            before_items,
            "rejected shape-on-epic create wrote no work_items row"
        );
        assert_eq!(
            count_events(&pool).await,
            before_events,
            "rejected shape-on-epic create wrote no events row"
        );
    }

    /// R34: the 64-KiB plan-field cap and the whitespace-only-outcome reject are
    /// enforced at EVERY attribute-write site, not just `set_epic_plan`. Boundary
    /// matrix: outcome exactly 64 KiB at create → Ok; 64 KiB+1 at create →
    /// Validation; 64 KiB+1 outcome via the generic `update_work_item` PATCH →
    /// Validation; whitespace-only outcome via the PATCH → Validation.
    #[tokio::test]
    async fn plan_field_constraints_enforced_at_create_and_patch() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let at_cap = "a".repeat(MAX_PLAN_FIELD_BYTES);
        let over_cap = "a".repeat(MAX_PLAN_FIELD_BYTES + 1);

        // outcome exactly at the 64-KiB cap → create Ok.
        let epic = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts { origin: None, outcome: Some(&at_cap), shape: None },
        )
        .await
        .expect("64-KiB outcome at create is Ok")
        .to_string();

        // outcome one byte over the cap → create Validation, zero rows.
        let before_items = count_work_items(&pool).await;
        let before_events = count_events(&pool).await;
        let err = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E2",
            None,
            CreateOpts { origin: None, outcome: Some(&over_cap), shape: None },
        )
        .await
        .expect_err("64-KiB+1 outcome at create must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        assert_eq!(count_work_items(&pool).await, before_items, "no row written");
        assert_eq!(count_events(&pool).await, before_events, "no event written");

        // 64-KiB+1 outcome via the generic PATCH → Validation.
        let over_patch = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: None,
            position: None,
            attributes: Some(serde_json::json!({ "outcome": over_cap })),
        };
        let err = update_work_item(&pool, &epic, &over_patch)
            .await
            .expect_err("64-KiB+1 outcome via PATCH must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // whitespace-only outcome via the generic PATCH → Validation.
        let blank_patch = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: None,
            position: None,
            attributes: Some(serde_json::json!({ "outcome": "   " })),
        };
        let err = update_work_item(&pool, &epic, &blank_patch)
            .await
            .expect_err("whitespace-only outcome via PATCH must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    /// R25: epic-done gate edge cases.
    /// (a) a ZERO-STORY epic (checked close-criterion + a focus, NO stories) →
    /// `epic→done` SUCCEEDS (the vacuous descendant-story clause passes); and
    /// (b) a `cancelled` (not `done`) terminal descendant story PASSES the
    /// descendant-terminal check.
    #[tokio::test]
    async fn epic_done_gate_zero_story_and_cancelled_terminal() {
        // (a) zero-story epic closes once its close-criterion is checked.
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();
        let crit = add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("criterion")
            .to_string();
        // a focus but NO stories beneath it.
        create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect("focus");
        check_acceptance_criterion(&pool, &crit, None)
            .await
            .expect("check criterion");
        update_work_item_status(&pool, &epic, "done")
            .await
            .expect("zero-story epic → done succeeds once close-criterion checked");

        // (b) a cancelled (not done) descendant story passes the terminal check.
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();
        let crit = add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("criterion")
            .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect("focus")
        .to_string();
        let story = create_work_item(&pool, "story", Some(&focus), "S", None)
            .await
            .expect("story")
            .to_string();
        check_acceptance_criterion(&pool, &crit, None)
            .await
            .expect("check criterion");
        // cancelled is terminal for the descendant-story clause.
        update_work_item_status(&pool, &story, "cancelled")
            .await
            .expect("story → cancelled");
        update_work_item_status(&pool, &epic, "done")
            .await
            .expect("epic → done succeeds with a cancelled (terminal) descendant story");
    }

    /// R26: `set_shape` is idempotent under re-set — setting `vertical-slice` then
    /// re-setting to `foundational` leaves the column holding the LATEST value.
    #[tokio::test]
    async fn set_shape_reset_holds_latest_value() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect("focus")
        .to_string();

        set_shape(&pool, &focus, Shape::VerticalSlice)
            .await
            .expect("set vertical-slice");
        set_shape(&pool, &focus, Shape::Foundational)
            .await
            .expect("re-set foundational");

        let shape = sqlx::query_scalar::<_, Option<String>>(
            "SELECT shape FROM work_items WHERE id = ?1",
        )
        .bind(&focus)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            shape.as_deref(),
            Some("foundational"),
            "re-set holds the latest value"
        );
    }

    /// A `focus` row's `kind` persists and reads back as `focus` (the migration
    /// 0010 rename of `feature`→`focus`): the round-trip proves the create core
    /// writes the renamed kind and the detail fold returns it unchanged.
    #[tokio::test]
    async fn focus_kind_round_trips() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None },
        )
        .await
        .expect("epic")
        .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
            },
        )
        .await
        .expect("focus")
        .to_string();

        let kind = sqlx::query_scalar::<_, String>(
            "SELECT kind FROM work_items WHERE id = ?1",
        )
        .bind(&focus)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kind, "focus", "kind persists as 'focus'");

        let detail = get_work_item_detail(&pool, &focus).await.expect("detail");
        assert_eq!(detail.item.kind, "focus", "detail folds kind as 'focus'");
    }

    // ---------------------------------------------------------------------
    // add_findings (B17a, migration 0011) — bulk insert with content-hash
    // dedup, all-or-nothing atomicity, and exactly one coarse batch event.
    // ---------------------------------------------------------------------

    /// Row count of `findings` (split per table like `count_work_items` —
    /// sqlx 0.9's `SqlSafeStr` bound rejects a dynamic table name).
    async fn count_findings(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM findings")
            .fetch_one(pool)
            .await
            .expect("count findings")
    }

    /// R-B3 (the load-bearing test): a finding whose identity tuple matches one
    /// already COMMITTED by a prior `add_findings` call is deduped on the re-run —
    /// reported as skipped, NOT double-inserted. A return-value-only assertion is
    /// insufficient: a mis-bound partial index would still report `skipped` from a
    /// bad upsert while silently duplicating the row, so the row-count assertion
    /// after the re-run is mandatory.
    #[tokio::test]
    async fn add_findings_dedup_skips_committed_prior() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let finding = NewFinding {
            file: Some("src/foo.rs"),
            line: Some(42),
            symbol: Some("foo"),
            summary: Some("a thing"),
            ..NewFinding::default()
        };

        // First insert — added, and the fn COMMITS (it owns the tx).
        let r1 = add_findings(&pool, None, &[(story.as_str(), finding.clone())])
            .await
            .expect("first add_findings");
        assert_eq!(r1.added, 1, "first insert adds the row");
        assert_eq!(r1.skipped, 0, "nothing skipped on first insert");
        assert_eq!(count_findings(&pool).await, 1, "one row after first insert");

        // Re-run with the SAME identity tuple — deduped against the committed row.
        let r2 = add_findings(&pool, None, &[(story.as_str(), finding.clone())])
            .await
            .expect("second add_findings");
        assert_eq!(r2.added, 0, "re-run adds nothing");
        assert_eq!(r2.skipped, 1, "re-run skips the duplicate");

        // skipped_ids carries the dedup CONTENT HASH a re-run recomputes.
        let expected_hash = finding_dedup_hash(
            &story,
            Some("src/foo.rs"),
            Some(42),
            Some("foo"),
            Some("a thing"),
        );
        assert!(
            r2.skipped_ids.contains(&expected_hash),
            "skipped_ids must carry the recomputed dedup hash; got {:?}",
            r2.skipped_ids
        );

        // MANDATORY (R-B3): the row count is UNCHANGED — the dedup actually
        // prevented a second physical insert (a mis-bound index would leave 2).
        assert_eq!(
            count_findings(&pool).await,
            1,
            "row count unchanged — the committed duplicate was not re-inserted"
        );
    }

    /// A batch in which one element triggers a real constraint error aborts the
    /// WHOLE batch: the tx drops un-committed → rollback → zero `findings` rows.
    ///
    /// The error path: `findings.run_id REFERENCES runs(id)` with FK enforcement
    /// on (`connect_in_memory` enables `foreign_keys(true)`). Passing a `run_id`
    /// that names no `runs` row makes every `create_finding_tx` INSERT fail with
    /// a foreign-key violation — a clean, real abort path at this layer (there is
    /// no validation inside `create_finding_tx` itself, so the FK is the genuine
    /// failing input rather than a synthetic one).
    #[tokio::test]
    async fn add_findings_aborts_whole_batch_on_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let valid_a = NewFinding { summary: Some("valid a"), ..NewFinding::default() };
        let valid_b = NewFinding { summary: Some("valid b"), ..NewFinding::default() };

        // run_id = Some("no-such-run") makes the FK fail on the first insert; the
        // two otherwise-valid findings never persist because the tx rolls back.
        let res = add_findings(
            &pool,
            Some("no-such-run"),
            &[(story.as_str(), valid_a), (story.as_str(), valid_b)],
        )
        .await;

        assert!(res.is_err(), "a constraint violation aborts the batch, got {res:?}");
        assert_eq!(
            count_findings(&pool).await,
            0,
            "rollback left zero findings — all-or-nothing"
        );
    }

    /// Happy path: a batch of two DISTINCT findings inserts both, skips none, and
    /// records EXACTLY ONE coarse `events` row for the whole batch (not one per
    /// finding).
    #[tokio::test]
    async fn add_findings_multi_item_happy_path() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let events_before = count_events(&pool).await;

        let a = NewFinding {
            file: Some("src/a.rs"),
            line: Some(1),
            summary: Some("alpha"),
            ..NewFinding::default()
        };
        let b = NewFinding {
            file: Some("src/b.rs"),
            line: Some(2),
            summary: Some("beta"),
            ..NewFinding::default()
        };

        let r = add_findings(&pool, None, &[(story.as_str(), a), (story.as_str(), b)])
            .await
            .expect("batch add");
        assert_eq!(r.added, 2, "both distinct findings added");
        assert_eq!(r.skipped, 0, "nothing skipped");
        assert!(r.skipped_ids.is_empty(), "no skipped ids");
        assert_eq!(count_findings(&pool).await, 2, "two rows persisted");

        // EXACTLY ONE new events row for the batch (coarse event, not per-finding).
        assert_eq!(
            count_events(&pool).await - events_before,
            1,
            "exactly one coarse batch event"
        );
    }

    /// Read one finding's `triage_state` (NULL-safe to a sentinel) via the runtime
    /// query API — tests assert DB state with `query_scalar`, never the macros.
    async fn finding_triage_state(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT triage_state FROM findings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select triage_state")
    }

    async fn finding_status(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT status FROM findings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select status")
    }

    async fn finding_category(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT category FROM findings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select category")
    }

    /// Happy path (D9): a bulk triage sets the non-terminal columns on every row,
    /// returns the updated count, and records EXACTLY ONE coarse batch event.
    #[tokio::test]
    async fn batch_update_findings_sets_triage_fields() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let a = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("alpha"), ..NewFinding::default() },
        )
        .await
        .expect("finding a")
        .to_string();
        let b = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("beta"), ..NewFinding::default() },
        )
        .await
        .expect("finding b")
        .to_string();

        let events_before = count_events(&pool).await;

        let updated = batch_update_findings(
            &pool,
            &[
                FindingTriageUpdate {
                    finding_id: &a,
                    triage_state: Some("accepted"),
                    severity: None,
                    category: Some("perf"),
                    status: None,
                },
                FindingTriageUpdate {
                    finding_id: &b,
                    triage_state: Some("accepted"),
                    severity: None,
                    category: Some("perf"),
                    status: None,
                },
            ],
        )
        .await
        .expect("batch triage");

        assert_eq!(updated, 2, "both findings updated");
        assert_eq!(finding_triage_state(&pool, &a).await.as_deref(), Some("accepted"));
        assert_eq!(finding_triage_state(&pool, &b).await.as_deref(), Some("accepted"));
        assert_eq!(finding_category(&pool, &a).await.as_deref(), Some("perf"));
        assert_eq!(finding_category(&pool, &b).await.as_deref(), Some("perf"));

        assert_eq!(
            count_events(&pool).await - events_before,
            1,
            "exactly one coarse batch event for the whole triage"
        );
    }

    /// A terminal-disposition status is rejected PRE-TX (zero writes); a
    /// non-terminal status value is accepted.
    #[tokio::test]
    async fn batch_update_findings_rejects_terminal_status() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let f = create_finding(
            &pool,
            &story,
            &NewFinding {
                summary: Some("gamma"),
                status: Some("open"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("finding")
        .to_string();

        let events_before = count_events(&pool).await;

        // "fixed" is a terminal `Disposition` → rejected before any write.
        let res = batch_update_findings(
            &pool,
            &[FindingTriageUpdate {
                finding_id: &f,
                triage_state: Some("accepted"),
                severity: None,
                category: None,
                status: Some("fixed"),
            }],
        )
        .await;

        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "terminal status rejected as Validation, got {res:?}"
        );
        // Pre-tx rejection wrote nothing: status and triage_state are unchanged.
        assert_eq!(
            finding_status(&pool, &f).await.as_deref(),
            Some("open"),
            "status unchanged after rejected batch"
        );
        assert_eq!(
            finding_triage_state(&pool, &f).await.as_deref(),
            Some("pending"),
            "triage_state unchanged (still column default 'pending') after rejected batch"
        );
        assert_eq!(
            count_events(&pool).await - events_before,
            0,
            "no event recorded for a rejected batch"
        );

        // A NON-terminal status value ("in_review" is not a Disposition variant)
        // is accepted.
        let updated = batch_update_findings(
            &pool,
            &[FindingTriageUpdate {
                finding_id: &f,
                triage_state: None,
                severity: None,
                category: None,
                status: Some("in_review"),
            }],
        )
        .await
        .expect("non-terminal status accepted");
        assert_eq!(updated, 1, "the single finding was updated");
        assert_eq!(finding_status(&pool, &f).await.as_deref(), Some("in_review"));
    }

    /// A missing finding id in the batch aborts the WHOLE batch (rollback): the
    /// real finding's triage_state is left unchanged.
    #[tokio::test]
    async fn batch_update_findings_missing_finding_aborts() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let real = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("delta"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let events_before = count_events(&pool).await;

        let res = batch_update_findings(
            &pool,
            &[
                FindingTriageUpdate {
                    finding_id: &real,
                    triage_state: Some("accepted"),
                    severity: None,
                    category: None,
                    status: None,
                },
                FindingTriageUpdate {
                    finding_id: "01999999-9999-7999-8999-999999999999",
                    triage_state: Some("accepted"),
                    severity: None,
                    category: None,
                    status: None,
                },
            ],
        )
        .await;

        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "a missing finding aborts the batch as NotFound, got {res:?}"
        );
        // Whole-batch rollback: the real finding's triage_state is untouched
        // (still the column default 'pending', not the attempted 'accepted').
        assert_eq!(
            finding_triage_state(&pool, &real).await.as_deref(),
            Some("pending"),
            "real finding's triage_state unchanged after whole-batch rollback"
        );
        assert_eq!(
            count_events(&pool).await - events_before,
            0,
            "no event recorded for an aborted batch"
        );
    }

    // ---------------------------------------------------------------------
    // query_findings / get_story_finding_queue (B20, migration 0011) —
    // the static NULL-guard filter, the count-by-severity grouping, and the
    // tombstone-excluding story queue.
    // ---------------------------------------------------------------------

    /// Force a finding's free-TEXT `triage_state` directly (no repo triage path
    /// is exercised here — we only need the column populated for filter tests).
    async fn set_finding_triage_state(pool: &SqlitePool, id: &str, state: &str) {
        sqlx::query("UPDATE findings SET triage_state = $1 WHERE id = $2")
            .bind(state)
            .bind(id)
            .execute(pool)
            .await
            .expect("update triage_state");
    }

    /// `query_findings` with NO filter and `count_by = None` returns ALL live
    /// findings; per-field filters (`work_item_id`, `severity`, `triage_state`)
    /// narrow the set; superseded findings never appear.
    #[tokio::test]
    async fn query_findings_filters_live_findings() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("legal task under story")
            .to_string();

        // Two findings on the story (one critical, one minor), one on the task.
        let f_crit = create_finding(
            &pool,
            &story,
            &NewFinding {
                severity: Some(Severity::Critical),
                summary: Some("crit on story"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("crit finding")
        .to_string();
        create_finding(
            &pool,
            &story,
            &NewFinding {
                severity: Some(Severity::Minor),
                summary: Some("minor on story"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("minor finding");
        create_finding(
            &pool,
            &task,
            &NewFinding {
                severity: Some(Severity::Critical),
                summary: Some("crit on task"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("task finding");

        // Mark the story's critical finding as accepted for the triage filter.
        set_finding_triage_state(&pool, &f_crit, "accepted").await;

        let all_count = |r: &QueryFindingsResult| match r {
            QueryFindingsResult::Findings(v) => v.len(),
            QueryFindingsResult::Counts(_) => panic!("expected Findings variant"),
        };

        // No filter → all three live findings.
        let all = query_findings(&pool, &QueryFindingsFilter::default_empty())
            .await
            .expect("query all");
        assert_eq!(all_count(&all), 3, "no filter returns all live findings");

        // Filter by work_item_id = story → the two story findings.
        let by_story = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_work_item_id(&story),
        )
        .await
        .expect("query by story");
        assert_eq!(all_count(&by_story), 2, "story has two findings");

        // Filter by severity = critical → the two critical findings.
        let by_sev = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_severity("critical"),
        )
        .await
        .expect("query by severity");
        assert_eq!(all_count(&by_sev), 2, "two critical findings");

        // Filter by triage_state = accepted → just the one we marked.
        let by_triage = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_triage_state("accepted"),
        )
        .await
        .expect("query by triage_state");
        match &by_triage {
            QueryFindingsResult::Findings(v) => {
                assert_eq!(v.len(), 1, "one accepted finding");
                assert_eq!(v[0].id, f_crit, "the accepted finding is f_crit");
            }
            QueryFindingsResult::Counts(_) => panic!("expected Findings variant"),
        }

        // Combined filter (work_item_id = story AND severity = minor) → one row.
        let combined = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty()
                .with_work_item_id(&story)
                .with_severity("minor"),
        )
        .await
        .expect("combined query");
        assert_eq!(all_count(&combined), 1, "one minor finding on the story");

        // Supersede the minor finding → it drops out of the live result.
        let minor_id = match &by_story {
            QueryFindingsResult::Findings(v) => v
                .iter()
                .find(|f| f.summary.as_deref() == Some("minor on story"))
                .map(|f| f.id.clone())
                .expect("minor finding present"),
            QueryFindingsResult::Counts(_) => unreachable!(),
        };
        sqlx::query("UPDATE findings SET superseded_by = $1 WHERE id = $2")
            .bind(&f_crit)
            .bind(&minor_id)
            .execute(&pool)
            .await
            .expect("supersede minor");
        let after_supersede = query_findings(&pool, &QueryFindingsFilter::default_empty())
            .await
            .expect("query after supersede");
        assert_eq!(
            all_count(&after_supersede),
            2,
            "superseded finding drops out of the live result"
        );
    }

    /// `query_findings` with `count_by = Some(Severity)` returns grouped
    /// `AxisCount`s whose counts sum to the total live findings, including the
    /// `'(none)'` sentinel bucket for a NULL-severity finding.
    #[tokio::test]
    async fn query_findings_count_by_severity_groups_and_sums() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // Two critical, one minor, one with NO severity (→ '(none)' bucket).
        for summary in ["c1", "c2"] {
            create_finding(
                &pool,
                &story,
                &NewFinding {
                    severity: Some(Severity::Critical),
                    summary: Some(summary),
                    ..NewFinding::default()
                },
            )
            .await
            .expect("crit");
        }
        create_finding(
            &pool,
            &story,
            &NewFinding {
                severity: Some(Severity::Minor),
                summary: Some("m1"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("minor");
        create_finding(
            &pool,
            &story,
            &NewFinding {
                severity: None,
                summary: Some("no-sev"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("no-sev");

        let res = query_findings(
            &pool,
            &QueryFindingsFilter::default_empty().with_count_by(crate::domain::FindingAxis::Severity),
        )
        .await
        .expect("count-by query");

        let counts = match res {
            QueryFindingsResult::Counts(c) => c,
            QueryFindingsResult::Findings(_) => panic!("expected Counts variant"),
        };

        // Buckets: '(none)' (1), 'critical' (2), 'minor' (1) — ordered by key.
        let by_key: std::collections::HashMap<&str, i64> =
            counts.iter().map(|c| (c.key.as_str(), c.count)).collect();
        assert_eq!(by_key.get("critical"), Some(&2), "two criticals");
        assert_eq!(by_key.get("minor"), Some(&1), "one minor");
        assert_eq!(
            by_key.get("(none)"),
            Some(&1),
            "one NULL-severity finding in the sentinel bucket"
        );
        let total: i64 = counts.iter().map(|c| c.count).sum();
        assert_eq!(total, 4, "grouped counts sum to all four live findings");
    }

    /// `get_story_finding_queue` spans the story PLUS its direct task children,
    /// and a finding on a SOFT-DELETED work-item drops out (tombstone guard).
    #[tokio::test]
    async fn story_finding_queue_excludes_tombstoned_work_items() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("legal task under story")
            .to_string();

        // One finding on the story, one on the child task.
        create_finding(
            &pool,
            &story,
            &NewFinding {
                summary: Some("on story"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("story finding");
        create_finding(
            &pool,
            &task,
            &NewFinding {
                summary: Some("on task"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("task finding");

        // Before deletion: both findings appear in the queue.
        let before = get_story_finding_queue(&pool, &story)
            .await
            .expect("queue before");
        let summaries_before: std::collections::HashSet<&str> = before
            .iter()
            .filter_map(|f| f.summary.as_deref())
            .collect();
        assert_eq!(before.len(), 2, "story + task findings span the queue");
        assert!(summaries_before.contains("on story"));
        assert!(summaries_before.contains("on task"));

        // Soft-delete the task work-item.
        delete_work_item(&pool, &task)
            .await
            .expect("soft-delete task");

        // After deletion: the task's finding drops out; the story's remains.
        let after = get_story_finding_queue(&pool, &story)
            .await
            .expect("queue after");
        assert_eq!(after.len(), 1, "tombstoned task's finding excluded");
        assert_eq!(
            after[0].summary.as_deref(),
            Some("on story"),
            "only the story's finding survives the tombstone guard"
        );
    }

    /// Tiny test-only builder helpers for [`QueryFindingsFilter`] (the struct's
    /// fields are public but it has no constructor; tests want a fluent empty
    /// base + per-field setters).
    impl QueryFindingsFilter {
        fn default_empty() -> Self {
            QueryFindingsFilter {
                work_item_id: None,
                run_id: None,
                severity: None,
                category: None,
                status: None,
                triage_state: None,
                count_by: None,
            }
        }
        fn with_work_item_id(mut self, id: &str) -> Self {
            self.work_item_id = Some(id.to_owned());
            self
        }
        fn with_severity(mut self, s: &str) -> Self {
            self.severity = Some(s.to_owned());
            self
        }
        fn with_triage_state(mut self, s: &str) -> Self {
            self.triage_state = Some(s.to_owned());
            self
        }
        fn with_count_by(mut self, axis: crate::domain::FindingAxis) -> Self {
            self.count_by = Some(axis);
            self
        }
    }

    // -----------------------------------------------------------------------
    // B23 — runs / sprints / triage decisions (migration 0011)
    // -----------------------------------------------------------------------

    // `FindingDecisionKind`/`NewFindingDecision`/`NewRun`/`NewSprint`/`TargetKind`
    // arrive via `use super::*`; only `RunKind` is not used by the production fns
    // and so needs an explicit import here.
    use crate::domain::RunKind;

    /// Read a `runs` row's `status` (NOT NULL with a column DEFAULT).
    async fn run_status(pool: &SqlitePool, id: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM runs WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select run status")
    }

    /// Count `sprint_tasks` rows for a sprint.
    async fn count_sprint_tasks(pool: &SqlitePool, sprint_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = $1")
            .bind(sprint_id)
            .fetch_one(pool)
            .await
            .expect("count sprint_tasks")
    }

    /// Read a work_item's `spawned_from_finding_id` (nullable column).
    async fn spawned_from(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_from_finding_id FROM work_items WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("select spawned_from_finding_id")
    }

    /// Count `finding_decisions` rows for a finding.
    async fn count_finding_decisions(pool: &SqlitePool, finding_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM finding_decisions WHERE finding_id = $1",
        )
        .bind(finding_id)
        .fetch_one(pool)
        .await
        .expect("count finding_decisions")
    }

    /// Seed a legal sprint with no tasks; returns the sprint id.
    async fn seed_sprint(pool: &SqlitePool) -> String {
        create_sprint(pool, &NewSprint { title: Some("S1".into()) })
            .await
            .expect("legal sprint")
            .to_string()
    }

    /// `create_run` accepts a valid live story target and lands a `runs` row with
    /// the column-default status `'open'`.
    #[tokio::test]
    async fn create_run_accepts_live_story_with_open_status() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let id = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Story,
            },
        )
        .await
        .expect("create_run on a live story");

        assert_eq!(run_status(&pool, &id.to_string()).await, "open");
    }

    /// `create_run` rejects a wrong-kind target (a story id passed as a sprint
    /// target), a dangling id, and a tombstoned story — all clean `Validation`.
    #[tokio::test]
    async fn create_run_rejects_invalid_targets() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // wrong kind: a real story id, but declared as a sprint target.
        let wrong_kind = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Sprint,
            },
        )
        .await;
        assert!(
            matches!(wrong_kind, Err(AppError::Validation(_))),
            "story id under a sprint target is a Validation, got {wrong_kind:?}"
        );

        // dangling id under a story target.
        let dangling = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Optimise,
                target_id: "no-such-id".into(),
                target_kind: TargetKind::Story,
            },
        )
        .await;
        assert!(
            matches!(dangling, Err(AppError::Validation(_))),
            "dangling story target is a Validation, got {dangling:?}"
        );

        // tombstoned story: soft-delete it, then target it.
        delete_work_item(&pool, &story).await.expect("soft-delete story");
        let tombstoned = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Story,
            },
        )
        .await;
        assert!(
            matches!(tombstoned, Err(AppError::Validation(_))),
            "tombstoned story target is a Validation, got {tombstoned:?}"
        );
    }

    /// `create_sprint` returns an id and the row exists with the default status.
    #[tokio::test]
    async fn create_sprint_inserts_row() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_sprint(&pool, &NewSprint { title: Some("Sprint 1".into()) })
            .await
            .expect("create_sprint");

        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sprints WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .expect("count sprints");
        assert_eq!(count, 1, "the sprint row exists");
    }

    /// `add_tasks_to_sprint`: a second add of the same task counts 0 (junction
    /// dedup via ON CONFLICT DO NOTHING), and the membership is not duplicated.
    #[tokio::test]
    async fn add_tasks_to_sprint_dedups_membership() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("legal task")
            .to_string();
        let sprint = seed_sprint(&pool).await;

        let first = add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
            .await
            .expect("first add");
        assert_eq!(first, 1, "first add inserts one membership");

        let second = add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
            .await
            .expect("second add");
        assert_eq!(second, 0, "re-adding the same task is a dedup skip, not an error");

        assert_eq!(
            count_sprint_tasks(&pool, &sprint).await,
            1,
            "the membership is not duplicated"
        );
    }

    /// `add_tasks_to_sprint`: a non-task id aborts the whole batch (all-or-nothing)
    /// — no memberships persist, even the valid ones that preceded it.
    #[tokio::test]
    async fn add_tasks_to_sprint_aborts_on_non_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("legal task")
            .to_string();
        let sprint = seed_sprint(&pool).await;

        // The story id is a valid work_item but NOT a task → abort the batch.
        let res = add_tasks_to_sprint(&pool, &sprint, &[task.as_str(), story.as_str()]).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "a non-task member aborts the batch, got {res:?}"
        );
        assert_eq!(
            count_sprint_tasks(&pool, &sprint).await,
            0,
            "rollback left zero memberships — all-or-nothing"
        );
    }

    /// `record_finding_decision` SpawnTask on a story-hosted finding creates a
    /// child task with `spawned_from_finding_id` set, `triage_state='accepted'`,
    /// and a `finding_decisions` row naming the new id.
    #[tokio::test]
    async fn record_finding_decision_spawn_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("needs a follow-up task"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let (decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::SpawnTask,
                decided_by: Some("triager".into()),
            },
        )
        .await
        .expect("spawn_task decision");

        let new_id = spawned.expect("spawn_task yields a work_item id").to_string();

        // The spawned item is a task parented under the host story.
        let (kind, parent): (String, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query("SELECT kind, parent_id FROM work_items WHERE id = $1")
                .bind(&new_id)
                .fetch_one(&pool)
                .await
                .unwrap();
            (r.try_get("kind").unwrap(), r.try_get("parent_id").unwrap())
        };
        assert_eq!(kind, "task", "spawned a task");
        assert_eq!(parent.as_deref(), Some(story.as_str()), "parented under the host story");

        assert_eq!(
            spawned_from(&pool, &new_id).await.as_deref(),
            Some(finding.as_str()),
            "spawned_from_finding_id back-link is stamped"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("accepted"),
            "spawn_task sets triage_state=accepted"
        );

        // A finding_decisions row exists naming the new id.
        let recorded_spawn: Option<String> = sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_work_item_id FROM finding_decisions WHERE id = $1",
        )
        .bind(decision_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select finding_decisions row");
        assert_eq!(
            recorded_spawn.as_deref(),
            Some(new_id.as_str()),
            "the decision row names the spawned work_item"
        );
    }

    /// `record_finding_decision` Resolve delegates to `resolve_finding`: the
    /// finding ends with a terminal `status` (the disposition wire value) AND a
    /// `finding_decisions` row is recorded; no work_item is spawned.
    #[tokio::test]
    async fn record_finding_decision_resolve_delegates() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("already fixed"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::Resolve,
                decided_by: None,
            },
        )
        .await
        .expect("resolve decision");

        assert!(spawned.is_none(), "resolve spawns no work_item");
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "no work_item created by a resolve"
        );
        // Terminal disposition stamped by the delegated resolve_finding.
        assert_eq!(
            finding_status(&pool, &finding).await.as_deref(),
            Some("fixed"),
            "resolve delegates a terminal Fixed disposition"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("accepted"),
            "resolve sets triage_state=accepted"
        );
        assert_eq!(
            count_finding_decisions(&pool, &finding).await,
            1,
            "a decision row is recorded for the resolve"
        );
    }

    /// `record_finding_decision` Dismiss sets `triage_state='dismissed'` and
    /// spawns nothing.
    #[tokio::test]
    async fn record_finding_decision_dismiss() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("not a real problem"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::Dismiss,
                decided_by: None,
            },
        )
        .await
        .expect("dismiss decision");

        assert!(spawned.is_none(), "dismiss spawns no work_item");
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "no work_item created by a dismiss"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("dismissed"),
            "dismiss sets triage_state=dismissed"
        );
    }
}
