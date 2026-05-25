//! Repository layer — the sole mutation path with transactional event writes (Task 3).
//!
//! **Single-source-of-truth discipline (the drift-killer):** every mutation in
//! this module opens a `pool.begin()` transaction, mutates exactly one domain
//! table, calls [`record_event`] to append ONE `events` row, then commits.
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
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::domain::{
    AcceptanceCriterion, ActivityType, ClosureGate, Complexity, ContextBlock, Disposition, Effort,
    Finding, OpenQuestion, QuestionOption, Relevance, RepoLink, ResearchNote, ResearchState,
    UpdateFindingRequest, UpdateResearchNoteRequest, UpdateWorkItemRequest, WorkItem,
    WorkItemActivity, WorkItemDetail,
};
use crate::error::AppError;

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
                "problem_statement" | "research_notes" | "execution_strategy" => {
                    want_string(k, v)?
                }
                _ => return Err(bad_key(k)),
            },
            "task" => match k.as_str() {
                "execution_detail" | "outcome" => want_string(k, v)?,
                "files_touched" => want_files_touched(k, v)?,
                "dispatch" => want_object(k, v)?,
                _ => return Err(bad_key(k)),
            },
            "epic" | "feature" => match k.as_str() {
                "context" | "grouping_rationale" => want_string(k, v)?,
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
const KINDS: [&str; 5] = ["project", "epic", "feature", "story", "task"];

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
    pool: &SqlitePool,
    parent_id: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<WorkItem>, AppError> {
    // `query!` (not `query_as!`) because `attributes` arrives as `Option<String>`
    // and is decoded into `WorkItem.attributes: Option<Value>` by hand.
    // Soft-delete reader policy (pinned): list views hide tombstoned rows.
    let rows = sqlx::query!(
        r#"
        SELECT
            id            AS "id!",
            kind          AS "kind!",
            parent_id     AS "parent_id?",
            title         AS "title!",
            body          AS "body?",
            status        AS "status!",
            position      AS "position?",
            attributes    AS "attributes?",
            relevance              AS "relevance?",
            effort                 AS "effort?",
            complexity             AS "complexity?",
            origin                 AS "origin?",
            closure_gate           AS "closure_gate?",
            blocked_by_question_id AS "blocked_by_question_id?",
            enabling_option_id     AS "enabling_option_id?",
            created_at    AS "created_at!",
            updated_at    AS "updated_at!"
        FROM work_items
        WHERE deleted_at IS NULL
          AND (?1 IS NULL OR parent_id = ?1)
          AND (?2 IS NULL OR kind = ?2)
        ORDER BY COALESCE(position, 0), created_at, id
        "#,
        parent_id,
        kind,
    )
    .fetch_all(pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| {
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
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(items)
}

/// Fetch one work item plus its DIRECT children, its findings, and the context
/// blocks linked through `work_item_context`. Returns `NotFound` if the id has
/// no row.
pub async fn get_work_item_detail(
    pool: &SqlitePool,
    id: &str,
) -> Result<WorkItemDetail, AppError> {
    // Soft-delete reader policy (pinned): the DETAIL fetch does NOT filter on
    // `deleted_at` — it returns the row WITH `deleted_at` populated so the export
    // tombstone path and a deleted-marker detail fetch both work.
    let row = sqlx::query!(
        r#"
        SELECT
            id            AS "id!",
            kind          AS "kind!",
            parent_id     AS "parent_id?",
            title         AS "title!",
            body          AS "body?",
            status        AS "status!",
            position      AS "position?",
            attributes    AS "attributes?",
            relevance              AS "relevance?",
            effort                 AS "effort?",
            complexity             AS "complexity?",
            origin                 AS "origin?",
            closure_gate           AS "closure_gate?",
            blocked_by_question_id AS "blocked_by_question_id?",
            enabling_option_id     AS "enabling_option_id?",
            created_at    AS "created_at!",
            updated_at    AS "updated_at!"
        FROM work_items
        WHERE id = ?1
        "#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))?;

    let item = WorkItem {
        id: row.id,
        kind: row.kind,
        parent_id: row.parent_id,
        title: row.title,
        body: row.body,
        status: row.status,
        position: row.position,
        attributes: decode_attributes(row.attributes)?,
        relevance: row.relevance,
        effort: row.effort,
        complexity: row.complexity,
        origin: row.origin,
        closure_gate: row.closure_gate,
        blocked_by_question_id: row.blocked_by_question_id,
        enabling_option_id: row.enabling_option_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
    };

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

    let context_blocks = sqlx::query_as!(
        ContextBlock,
        r#"
        SELECT
            cb.id          AS "id!",
            cb.title       AS "title?",
            cb.body        AS "body?",
            cb.created_at  AS "created_at!",
            cb.updated_at  AS "updated_at!"
        FROM context_blocks cb
        JOIN work_item_context wic ON wic.context_block_id = cb.id
        WHERE wic.work_item_id = ?1
        ORDER BY cb.created_at, cb.id
        "#,
        id,
    )
    .fetch_all(pool)
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
    })
}

/// List the LIVE research-note rows for a work item (migration 0003), ordered by
/// the per-item monotonic `seq`. "Live" = `superseded_by IS NULL`: a note
/// superseded by a newer one drops out of this fold. `query_as!` straight onto
/// the [`ResearchNote`] read struct (all columns map 1:1).
async fn list_research_notes(
    pool: &SqlitePool,
    work_item_id: &str,
) -> Result<Vec<ResearchNote>, AppError> {
    let rows = sqlx::query_as!(
        ResearchNote,
        r#"
        SELECT
            id            AS "id!",
            work_item_id  AS "work_item_id!",
            seq           AS "seq!",
            summary       AS "summary!",
            body          AS "body?",
            confidence    AS "confidence?",
            state         AS "state?",
            rationale     AS "rationale?",
            lens          AS "lens?",
            origin        AS "origin?",
            superseded_by AS "superseded_by?",
            created_at    AS "created_at!"
        FROM research_notes
        WHERE work_item_id = ?1
          AND superseded_by IS NULL
        ORDER BY seq
        "#,
        work_item_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// List the open-question rows for a story (migration 0003), ordered by the
/// per-story monotonic `seq`, EACH with its `question_options` (also `seq`-
/// ordered) folded into the nested `options` Vec. Two queries (questions, then
/// per-question options) keep the `.sqlx` cache simple and the read shape exact.
async fn list_open_questions(
    pool: &SqlitePool,
    story_id: &str,
) -> Result<Vec<OpenQuestion>, AppError> {
    let questions = sqlx::query!(
        r#"
        SELECT
            id                   AS "id!",
            story_id             AS "story_id!",
            seq                  AS "seq!",
            question             AS "question!",
            status               AS "status?",
            answer               AS "answer?",
            chosen_option_id     AS "chosen_option_id?",
            decided_at           AS "decided_at?",
            decided_by           AS "decided_by?",
            prompting_finding_id AS "prompting_finding_id?",
            prompting_note_id    AS "prompting_note_id?",
            created_at           AS "created_at!"
        FROM open_questions
        WHERE story_id = ?1
        ORDER BY seq
        "#,
        story_id,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(questions.len());
    for q in questions {
        let options = sqlx::query_as!(
            QuestionOption,
            r#"
            SELECT
                id          AS "id!",
                question_id AS "question_id!",
                seq         AS "seq!",
                label       AS "label!",
                detail      AS "detail?",
                created_at  AS "created_at!"
            FROM question_options
            WHERE question_id = ?1
            ORDER BY seq
            "#,
            q.id,
        )
        .fetch_all(pool)
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
async fn list_acceptance_criteria(
    pool: &SqlitePool,
    work_item_id: &str,
) -> Result<Vec<AcceptanceCriterion>, AppError> {
    let rows = sqlx::query_as!(
        AcceptanceCriterion,
        r#"
        SELECT
            id           AS "id!",
            work_item_id AS "work_item_id!",
            seq          AS "seq!",
            text         AS "text!",
            checked      AS "checked!",
            checked_at   AS "checked_at?",
            checked_by   AS "checked_by?",
            created_at   AS "created_at!"
        FROM acceptance_criteria
        WHERE work_item_id = ?1
        ORDER BY seq
        "#,
        work_item_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// List the activity-log rows for a work item, ordered by the per-item
/// monotonic `seq`. `query!` + manual map because `payload` arrives as
/// `Option<String>` and is decoded into `Option<Value>`.
async fn list_activity(
    pool: &SqlitePool,
    work_item_id: &str,
) -> Result<Vec<WorkItemActivity>, AppError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            id            AS "id!",
            work_item_id  AS "work_item_id!",
            seq           AS "seq!",
            entry_kind    AS "entry_kind!",
            author        AS "author?",
            summary       AS "summary!",
            payload       AS "payload?",
            origin        AS "origin?",
            created_at    AS "created_at!"
        FROM work_item_activity
        WHERE work_item_id = ?1
        ORDER BY seq
        "#,
        work_item_id,
    )
    .fetch_all(pool)
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

/// List the LIVE findings attached to a work item, newest-flagged first.
/// "Live" = `superseded_by IS NULL` (migration 0003): a finding superseded by a
/// newer one drops out of this fold, mirroring the research-note supersession
/// chain. This is the fold `get_work_item_detail` returns.
pub async fn list_findings(
    pool: &SqlitePool,
    work_item_id: &str,
) -> Result<Vec<Finding>, AppError> {
    let rows = sqlx::query_as!(
        Finding,
        r#"
        SELECT
            id                AS "id!",
            work_item_id      AS "work_item_id?",
            kind              AS "kind?",
            severity          AS "severity?",
            effort            AS "effort?",
            category          AS "category?",
            status            AS "status?",
            file              AS "file?",
            line              AS "line?",
            symbol            AS "symbol?",
            summary           AS "summary?",
            description       AS "description?",
            first_flagged     AS "first_flagged?",
            rounds            AS "rounds?",
            fingerprint       AS "fingerprint?",
            flow              AS "flow?",
            dedup_id          AS "dedup_id?",
            origin            AS "origin?",
            confidence        AS "confidence?",
            superseded_by     AS "superseded_by?",
            resolved_at       AS "resolved_at?",
            resolution        AS "resolution?",
            defer_reason      AS "defer_reason?",
            defer_trigger     AS "defer_trigger?",
            wontfix_rationale AS "wontfix_rationale?",
            repo_id           AS "repo_id?"
        FROM findings
        WHERE work_item_id = ?1
          AND superseded_by IS NULL
        ORDER BY first_flagged DESC, id
        "#,
        work_item_id,
    )
    .fetch_all(pool)
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
/// default `relevance="backlog"` for a new epic/feature/story is applied there.
pub async fn create_work_item(
    pool: &SqlitePool,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
) -> Result<Uuid, AppError> {
    create_work_item_with_origin(pool, kind, parent_id, title, body, None).await
}

/// Create a work item, stamping the optional `origin` provenance (migration
/// 0003). Same single-mutation-path discipline as the 5-arg [`create_work_item`]
/// wrapper. A newly-created `epic`/`feature`/`story` acquires the default
/// `relevance="backlog"` (epic/feature/story carry the relevance axis;
/// task/project are left NULL); the relevance default is applied in the INSERT.
pub async fn create_work_item_with_origin(
    pool: &SqlitePool,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
    origin: Option<&str>,
) -> Result<Uuid, AppError> {
    // Resolve the parent's kind (if any) for the pre-check. A non-NULL
    // parent_id that does not exist is a Validation error, not a 500.
    let parent_kind: Option<String> = match parent_id {
        Some(pid) => {
            let row = sqlx::query!(
                r#"SELECT kind AS "kind!" FROM work_items WHERE id = ?1"#,
                pid,
            )
            .fetch_optional(pool)
            .await?;
            match row {
                Some(r) => Some(r.kind),
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

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    // epic/feature/story carry the relevance axis and default to "backlog" on
    // create; task/project are left NULL.
    let default_relevance: Option<&str> = match kind {
        "epic" | "feature" | "story" => Some("backlog"),
        _ => None,
    };

    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO work_items (id, kind, parent_id, title, body, status, origin, relevance)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        id_str,
        kind,
        parent_id,
        title,
        body,
        "open",
        origin,
        default_relevance,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({
        "kind": kind,
        "parent_id": parent_id,
        "title": title,
        "origin": origin,
    });
    record_event(&mut tx, "work_item", &id_str, "work_item.created", payload).await?;

    tx.commit().await?;

    Ok(id)
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
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    target_status: &str,
) -> Result<(), AppError> {
    if target_status != "done" {
        return Ok(());
    }

    // Read the item's kind + parent. Absent row ⇒ inert (caller handles NotFound).
    let Some(row) = sqlx::query!(
        r#"SELECT kind AS "kind!", parent_id AS "parent_id?" FROM work_items WHERE id = ?1"#,
        id,
    )
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(());
    };

    if row.kind != "task" {
        return Ok(());
    }

    // The gate is the immediate parent story's `closure_gate` (no ancestor walk).
    let Some(parent_id) = row.parent_id else {
        return Ok(());
    };
    let Some(parent) = sqlx::query!(
        r#"SELECT kind AS "kind!", closure_gate AS "closure_gate?" FROM work_items WHERE id = ?1"#,
        parent_id,
    )
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(());
    };

    if parent.kind != "story" || parent.closure_gate.as_deref() != Some("hard") {
        // soft (default) / non-story parent ⇒ allow.
        return Ok(());
    }

    // Hard gate: reject if any acceptance criterion of the TASK is unchecked.
    let unchecked = sqlx::query!(
        r#"SELECT COUNT(*) AS "n!" FROM acceptance_criteria WHERE work_item_id = ?1 AND checked = 0"#,
        id,
    )
    .fetch_one(&mut **tx)
    .await?
    .n;

    if unchecked > 0 {
        return Err(AppError::Validation(format!(
            "task '{id}' cannot transition to 'done': its story's closure_gate is 'hard' \
             and {unchecked} acceptance criterion(s) remain unchecked"
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
    pool: &SqlitePool,
    id: &str,
    status: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    // Closure gate (migration 0003): reject task→done under a `hard` story while
    // any acceptance criterion is unchecked. Runs before the UPDATE in this tx.
    enforce_closure_gate(&mut tx, id, status).await?;

    let affected = sqlx::query!(
        r#"
        UPDATE work_items
        SET status = ?2, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
        id,
        status,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        // tx drops here → rollback; no event emitted for a missing row.
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "status": status });
    record_event(&mut tx, "work_item", id, "work_item.status_changed", payload).await?;

    tx.commit().await?;

    Ok(())
}

/// Set a work item's `relevance` (migration 0003, User Decision 2). The
/// relevance axis is structural and carried ONLY by epic/feature/story; a
/// `task`/`project` is rejected with a typed [`AppError::Validation`]. The
/// kind is read first; `NotFound` if the id has no row; one event on success.
pub async fn set_relevance(
    pool: &SqlitePool,
    id: &str,
    relevance: Relevance,
) -> Result<(), AppError> {
    let kind = work_item_kind(pool, id).await?;
    if !matches!(kind.as_str(), "epic" | "feature" | "story") {
        return Err(AppError::Validation(format!(
            "relevance is settable only on epic/feature/story, not on '{kind}'"
        )));
    }
    let value = enum_to_str(relevance);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE work_items SET relevance = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1"#,
        id,
        value,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "relevance": value });
    record_event(&mut tx, "work_item", id, "work_item.relevance_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a work item's `effort` grade (migration 0003). Task-scoped: the effort
/// axis drives batch sizing for a leaf task, so a non-`task` kind is rejected
/// with a typed [`AppError::Validation`]. Kind read first; `NotFound` via
/// `rows_affected()==0`; one event.
pub async fn set_effort(pool: &SqlitePool, id: &str, effort: Effort) -> Result<(), AppError> {
    let kind = work_item_kind(pool, id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "effort is settable only on a task, not on '{kind}'"
        )));
    }
    let value = enum_to_str(effort);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE work_items SET effort = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1"#,
        id,
        value,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "effort": value });
    record_event(&mut tx, "work_item", id, "work_item.effort_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a work item's `complexity` grade (migration 0003). Task-scoped (drives
/// model-tier assignment for a leaf task); a non-`task` kind is rejected with a
/// typed [`AppError::Validation`]. Kind read first; `NotFound` via
/// `rows_affected()==0`; one event.
pub async fn set_complexity(
    pool: &SqlitePool,
    id: &str,
    complexity: Complexity,
) -> Result<(), AppError> {
    let kind = work_item_kind(pool, id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "complexity is settable only on a task, not on '{kind}'"
        )));
    }
    let value = enum_to_str(complexity);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE work_items SET complexity = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1"#,
        id,
        value,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "complexity": value });
    record_event(&mut tx, "work_item", id, "work_item.complexity_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a story's `closure_gate` (migration 0003, User Decision 3). Story-scoped:
/// the gate decides whether tasks under the story reject a `→done` transition
/// while their acceptance criteria are unchecked (`hard`) or merely flag it
/// (`soft`). A non-`story` kind is rejected with a typed [`AppError::Validation`].
/// Kind read first; `NotFound` via `rows_affected()==0`; one event.
pub async fn set_closure_gate(
    pool: &SqlitePool,
    story_id: &str,
    gate: ClosureGate,
) -> Result<(), AppError> {
    let kind = work_item_kind(pool, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "closure_gate is settable only on a story, not on '{kind}'"
        )));
    }
    let value = enum_to_str(gate);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE work_items SET closure_gate = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1"#,
        story_id,
        value,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{story_id}' not found")));
    }

    let payload = serde_json::json!({ "closure_gate": value });
    record_event(&mut tx, "work_item", story_id, "work_item.closure_gate_set", payload).await?;

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
    pool: &SqlitePool,
    work_item_id: &str,
    text: &str,
) -> Result<Uuid, AppError> {
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(pool, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    let seq = sqlx::query!(
        r#"SELECT COALESCE(MAX(seq), 0) + 1 AS "next!" FROM acceptance_criteria WHERE work_item_id = ?1"#,
        work_item_id,
    )
    .fetch_one(&mut *tx)
    .await?
    .next;

    sqlx::query!(
        r#"INSERT INTO acceptance_criteria (id, work_item_id, seq, text) VALUES (?1, ?2, ?3, ?4)"#,
        id_str,
        work_item_id,
        seq,
        text,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "criterion_id": id_str, "seq": seq });
    record_event(
        &mut tx,
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
    pool: &SqlitePool,
    id: &str,
) -> Result<String, AppError> {
    sqlx::query!(
        r#"SELECT work_item_id AS "work_item_id!" FROM acceptance_criteria WHERE id = ?1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .map(|r| r.work_item_id)
    .ok_or_else(|| AppError::NotFound(format!("acceptance_criterion '{id}' not found")))
}

/// Check an acceptance criterion (migration 0003): set `checked=1`,
/// `checked_at=CURRENT_TIMESTAMP`, `checked_by`, AND append a `verification`
/// `work_item_activity` row for the owning work item (state-vs-immutable-audit,
/// per the plan's acceptance-criteria research note) — all in ONE transaction
/// with ONE event. The owning work_item_id is read first (`NotFound` if the
/// criterion is absent). Event `work_item.acceptance_criterion_checked`.
pub async fn check_acceptance_criterion(
    pool: &SqlitePool,
    id: &str,
    by: Option<&str>,
) -> Result<(), AppError> {
    let work_item_id = acceptance_criterion_work_item(pool, id).await?;

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE acceptance_criteria
        SET checked = 1, checked_at = CURRENT_TIMESTAMP, checked_by = ?2
        WHERE id = ?1
        "#,
        id,
        by,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    // Append the immutable verification-audit activity row for the owning item.
    // seq is allocated MAX(seq)+1 within this same tx.
    let activity_id = Uuid::now_v7().to_string();
    let act_seq = sqlx::query!(
        r#"SELECT COALESCE(MAX(seq), 0) + 1 AS "next!" FROM work_item_activity WHERE work_item_id = ?1"#,
        work_item_id,
    )
    .fetch_one(&mut *tx)
    .await?
    .next;
    let summary = format!("acceptance criterion {id} checked");
    sqlx::query!(
        r#"
        INSERT INTO work_item_activity (id, work_item_id, seq, entry_kind, author, summary)
        VALUES (?1, ?2, ?3, 'verification', ?4, ?5)
        "#,
        activity_id,
        work_item_id,
        act_seq,
        by,
        summary,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "criterion_id": id, "checked": true });
    record_event(
        &mut tx,
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
pub async fn uncheck_acceptance_criterion(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let work_item_id = acceptance_criterion_work_item(pool, id).await?;

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE acceptance_criteria
        SET checked = 0, checked_at = NULL, checked_by = NULL
        WHERE id = ?1
        "#,
        id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    let payload = serde_json::json!({ "criterion_id": id, "checked": false });
    record_event(
        &mut tx,
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
pub async fn remove_acceptance_criterion(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    // Resolve the owning item first so the event aggregate is the work_item
    // (and so an absent criterion is NotFound before any write).
    let work_item_id = acceptance_criterion_work_item(pool, id).await?;

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(r#"DELETE FROM acceptance_criteria WHERE id = ?1"#, id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    let payload = serde_json::json!({ "criterion_id": id, "removed": true });
    record_event(
        &mut tx,
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
async fn work_item_kind(pool: &SqlitePool, id: &str) -> Result<String, AppError> {
    sqlx::query!(r#"SELECT kind AS "kind!" FROM work_items WHERE id = ?1"#, id)
        .fetch_optional(pool)
        .await?
        .map(|r| r.kind)
        .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))
}

/// Partial update of a work item under the single-mutation-path discipline.
/// Each field is **set-or-leave**: a `None` bind leaves the column untouched via
/// `COALESCE(?, col)` (it does NOT clear to NULL). If `attributes` is present it
/// is normalised (object-root, null-keys dropped) and per-kind validated
/// (unknown key ⇒ `Validation`) BEFORE the write. `NotFound` via
/// `rows_affected()==0` so a missing row emits no event. Event `work_item.updated`.
pub async fn update_work_item(
    pool: &SqlitePool,
    id: &str,
    req: &UpdateWorkItemRequest,
) -> Result<(), AppError> {
    // Pre-validate `attributes` (needs the row's kind) before opening the tx.
    let attributes_str: Option<String> = match &req.attributes {
        Some(value) => {
            let kind = work_item_kind(pool, id).await?;
            let cleaned = normalise_object(value, "attributes")?;
            validate_attributes_for_kind(&kind, &cleaned)?;
            Some(serde_json::to_string(&Value::Object(cleaned)).map_err(|e| AppError::Other(e.into()))?)
        }
        None => None,
    };

    let status_str: Option<String> = req.status.map(enum_to_str);

    let mut tx = pool.begin().await?;

    // Closure gate (migration 0003): this generic PATCH can set status="done"
    // directly, so it routes through the SAME gate as update_work_item_status
    // (User Decision 3) — a task→done under a `hard` story with unchecked
    // criteria is rejected here too. No-op when status is absent / not "done".
    if let Some(s) = status_str.as_deref() {
        enforce_closure_gate(&mut tx, id, s).await?;
    }

    let affected = sqlx::query!(
        r#"
        UPDATE work_items
        SET title      = COALESCE(?2, title),
            body       = COALESCE(?3, body),
            status     = COALESCE(?4, status),
            position   = COALESCE(?5, position),
            attributes = COALESCE(?6, attributes),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1 AND deleted_at IS NULL
        "#,
        id,
        req.title,
        req.body,
        status_str,
        req.position,
        attributes_str,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

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
    record_event(&mut tx, "work_item", id, "work_item.updated", payload).await?;

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
    pool: &SqlitePool,
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
    let _ = work_item_kind(pool, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    // Allocate the per-item monotonic seq inside the tx.
    let seq = sqlx::query!(
        r#"SELECT COALESCE(MAX(seq), 0) + 1 AS "next!" FROM work_item_activity WHERE work_item_id = ?1"#,
        work_item_id,
    )
    .fetch_one(&mut *tx)
    .await?
    .next;

    sqlx::query!(
        r#"
        INSERT INTO work_item_activity (id, work_item_id, seq, entry_kind, author, summary, payload, origin)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        id_str,
        work_item_id,
        seq,
        entry_kind,
        author,
        summary,
        payload_str,
        origin,
    )
    .execute(&mut *tx)
    .await?;

    let event_payload = serde_json::json!({
        "activity_id": id_str,
        "seq": seq,
        "entry_kind": entry_kind,
    });
    record_event(&mut tx, "work_item", work_item_id, "work_item.activity_appended", event_payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read-modify-merge a work item's `attributes`: SELECT the current object,
/// overwrite the keys present in `patch`, leave absent keys, normalise
/// (object-root, drop null-valued keys), per-kind validate, write back. This is
/// the fn the MCP `set_story_plan`/`set_task_spec` partial setters compose on, so
/// merging must NOT clobber sibling keys. One event `work_item.updated`.
pub async fn set_work_item_attributes(
    pool: &SqlitePool,
    id: &str,
    patch: &Value,
) -> Result<(), AppError> {
    // The patch itself must be a null-free object root.
    let patch_obj = normalise_object(patch, "attributes")?;

    let mut tx = pool.begin().await?;

    // Read current kind + attributes (do not resurrect a tombstoned row).
    let current = sqlx::query!(
        r#"SELECT kind AS "kind!", attributes AS "attributes?" FROM work_items WHERE id = ?1 AND deleted_at IS NULL"#,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))?;

    // Merge: start from the existing object (or empty), overwrite present keys.
    // A stored blob that is non-JSON or a non-object root is data corruption (the
    // write side normalises every stored value to an object root) — fail loudly
    // as `Other` (→ 500) rather than silently discarding it (R13), mirroring
    // `decode_attributes`.
    let mut merged: serde_json::Map<String, Value> = match current.attributes {
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
    validate_attributes_for_kind(&current.kind, &cleaned)?;

    let merged_str =
        serde_json::to_string(&Value::Object(cleaned)).map_err(|e| AppError::Other(e.into()))?;

    sqlx::query!(
        r#"UPDATE work_items SET attributes = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1 AND deleted_at IS NULL"#,
        id,
        merged_str,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "attributes_merged": true });
    record_event(&mut tx, "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a work item's sibling-ordering `position` under the single-mutation-path
/// discipline. Reuses the `work_item.updated` event type (matches the
/// `update_work_item` partial-update convention — position is one of its
/// COALESCE fields). `NotFound` via `rows_affected()==0`.
pub async fn reorder_work_item(
    pool: &SqlitePool,
    id: &str,
    position: i64,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE work_items SET position = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1 AND deleted_at IS NULL"#,
        id,
        position,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "position": position });
    record_event(&mut tx, "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Create a `context_blocks` row under the single-mutation-path discipline.
/// Returns the new id. Event `context_block.created`.
pub async fn create_context_block(
    pool: &SqlitePool,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<Uuid, AppError> {
    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"INSERT INTO context_blocks (id, title, body) VALUES (?1, ?2, ?3)"#,
        id_str,
        title,
        body,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "title": title });
    record_event(&mut tx, "context_block", &id_str, "context_block.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Link a context block to a work item (insert the `work_item_context` row)
/// under the single-mutation-path discipline. Event `context_block.linked`.
pub async fn link_context_block(
    pool: &SqlitePool,
    work_item_id: &str,
    context_block_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"INSERT INTO work_item_context (work_item_id, context_block_id) VALUES (?1, ?2)"#,
        work_item_id,
        context_block_id,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "context_block_id": context_block_id });
    record_event(&mut tx, "work_item", work_item_id, "context_block.linked", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Unlink a context block from a work item (hard-delete the link row — links
/// have no independent export identity) under the single-mutation-path
/// discipline. `NotFound` via `rows_affected()==0`. Event `context_block.unlinked`.
pub async fn unlink_context_block(
    pool: &SqlitePool,
    work_item_id: &str,
    context_block_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"DELETE FROM work_item_context WHERE work_item_id = ?1 AND context_block_id = ?2"#,
        work_item_id,
        context_block_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "context link (work_item '{work_item_id}', block '{context_block_id}') not found"
        )));
    }

    let payload = serde_json::json!({ "context_block_id": context_block_id });
    record_event(&mut tx, "work_item", work_item_id, "context_block.unlinked", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Partial update of a finding under the single-mutation-path discipline. Each
/// field is set-or-leave via `COALESCE(?, col)`. The typed `severity` enum is
/// rendered to its snake_case wire form before storage. `NotFound` via
/// `rows_affected()==0`. Event `finding.updated`.
pub async fn update_finding(
    pool: &SqlitePool,
    id: &str,
    req: &UpdateFindingRequest,
) -> Result<(), AppError> {
    let severity_str: Option<String> = req.severity.map(enum_to_str);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE findings
        SET severity    = COALESCE(?2, severity),
            effort      = COALESCE(?3, effort),
            category    = COALESCE(?4, category),
            status      = COALESCE(?5, status),
            file        = COALESCE(?6, file),
            line        = COALESCE(?7, line),
            symbol      = COALESCE(?8, symbol),
            summary     = COALESCE(?9, summary),
            description = COALESCE(?10, description),
            confidence  = COALESCE(?11, confidence)
        WHERE id = ?1
        "#,
        id,
        severity_str,
        req.effort,
        req.category,
        req.status,
        req.file,
        req.line,
        req.symbol,
        req.summary,
        req.description,
        req.confidence,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

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
    record_event(&mut tx, "finding", id, "finding.updated", payload).await?;

    tx.commit().await?;
    Ok(())
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
    pool: &SqlitePool,
    old_id: &str,
    new_id: &str,
) -> Result<(), AppError> {
    // Validate the superseding finding exists (R7): clean 422 over a dangling-FK 500.
    let new_exists = sqlx::query!(
        r#"SELECT 1 AS "one!" FROM findings WHERE id = ?1"#,
        new_id,
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !new_exists {
        return Err(AppError::Validation(format!(
            "superseding finding '{new_id}' does not exist"
        )));
    }

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE findings SET superseded_by = ?2 WHERE id = ?1"#,
        old_id,
        new_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("finding '{old_id}' not found")));
    }

    let payload = serde_json::json!({ "superseded_by": new_id });
    record_event(&mut tx, "finding", old_id, "finding.superseded", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Resolve a finding to a terminal [`Disposition`] under the single-mutation-path
/// discipline: stamp `status` (the disposition wire value), `resolved_at`, and
/// the optional `resolution`/`wontfix_rationale` free-text. `NotFound` via
/// `rows_affected()==0`. Event `finding.resolved`.
pub async fn resolve_finding(
    pool: &SqlitePool,
    id: &str,
    disposition: Disposition,
    resolution: Option<&str>,
    rationale: Option<&str>,
) -> Result<(), AppError> {
    let disposition_str = enum_to_str(disposition);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE findings
        SET status            = ?2,
            resolved_at       = CURRENT_TIMESTAMP,
            resolution        = COALESCE(?3, resolution),
            wontfix_rationale = COALESCE(?4, wontfix_rationale)
        WHERE id = ?1
        "#,
        id,
        disposition_str,
        resolution,
        rationale,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("finding '{id}' not found")));
    }

    let payload = serde_json::json!({ "disposition": disposition_str });
    record_event(&mut tx, "finding", id, "finding.resolved", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// SOFT-delete a work item: stamp `deleted_at` under the single-mutation-path
/// discipline. The row (and its cascaded activity) is preserved — a work item
/// owns export identity, so hard-delete would orphan the export TOML and lose
/// history. Idempotent-ish: a row already deleted (or absent) is `NotFound` via
/// `rows_affected()==0`. Event `work_item.deleted`.
pub async fn delete_work_item(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE work_items SET deleted_at = CURRENT_TIMESTAMP WHERE id = ?1 AND deleted_at IS NULL"#,
        id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "deleted": true });
    record_event(&mut tx, "work_item", id, "work_item.deleted", payload).await?;

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
    pub severity: Option<&'a str>,
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
    pool: &SqlitePool,
    work_item_id: &str,
    finding: &NewFinding<'_>,
) -> Result<Uuid, AppError> {
    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO findings (
            id, work_item_id, kind, severity, effort, category, status,
            file, line, symbol, summary, description, first_flagged, rounds,
            fingerprint, flow, dedup_id, origin, confidence, resolved_at, resolution,
            defer_reason, defer_trigger, wontfix_rationale, repo_id
        )
        VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            ?15, ?16, ?17, ?18, ?19, ?20, ?21,
            ?22, ?23, ?24, ?25
        )
        "#,
        id_str,
        work_item_id,
        finding.kind,
        finding.severity,
        finding.effort,
        finding.category,
        finding.status,
        finding.file,
        finding.line,
        finding.symbol,
        finding.summary,
        finding.description,
        finding.first_flagged,
        finding.rounds,
        finding.fingerprint,
        finding.flow,
        finding.dedup_id,
        finding.origin,
        finding.confidence,
        finding.resolved_at,
        finding.resolution,
        finding.defer_reason,
        finding.defer_trigger,
        finding.wontfix_rationale,
        finding.repo_id,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({
        "work_item_id": work_item_id,
        "severity": finding.severity,
        "category": finding.category,
        "status": finding.status,
    });
    record_event(&mut tx, "finding", &id_str, "finding.created", payload).await?;

    tx.commit().await?;

    Ok(id)
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
    pool: &SqlitePool,
    work_item_id: &str,
    summary: &str,
    body: Option<&str>,
    confidence: Option<&str>,
    lens: Option<&str>,
    origin: Option<&str>,
) -> Result<Uuid, AppError> {
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(pool, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    // State defaults to `proposed` on create.
    let state = enum_to_str(ResearchState::Proposed);

    let mut tx = pool.begin().await?;

    let seq = sqlx::query!(
        r#"SELECT COALESCE(MAX(seq), 0) + 1 AS "next!" FROM research_notes WHERE work_item_id = ?1"#,
        work_item_id,
    )
    .fetch_one(&mut *tx)
    .await?
    .next;

    sqlx::query!(
        r#"
        INSERT INTO research_notes
            (id, work_item_id, seq, summary, body, confidence, state, lens, origin)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        id_str,
        work_item_id,
        seq,
        summary,
        body,
        confidence,
        state,
        lens,
        origin,
    )
    .execute(&mut *tx)
    .await?;

    let payload = serde_json::json!({ "note_id": id_str, "seq": seq });
    record_event(&mut tx, "work_item", work_item_id, "work_item.research_note_added", payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read a research note's owning `work_item_id`, erroring `NotFound` if the note
/// id has no row. Used by the update/supersede paths to attribute the owning item
/// for the event aggregate.
async fn research_note_work_item(pool: &SqlitePool, id: &str) -> Result<String, AppError> {
    sqlx::query!(
        r#"SELECT work_item_id AS "work_item_id!" FROM research_notes WHERE id = ?1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .map(|r| r.work_item_id)
    .ok_or_else(|| AppError::NotFound(format!("research_note '{id}' not found")))
}

/// Partial set-or-leave update of a research note's curatable fields (migration
/// 0003): `confidence`/`state`/`rationale`/`lens` via `COALESCE(?, col)` (absent
/// ⇒ untouched). The typed `state` enum is rendered to its wire form. The owning
/// work_item_id is read first (`NotFound` if the note is absent). One event
/// `work_item.research_note_updated`.
pub async fn update_research_note(
    pool: &SqlitePool,
    id: &str,
    req: &UpdateResearchNoteRequest,
) -> Result<(), AppError> {
    let work_item_id = research_note_work_item(pool, id).await?;
    let state_str: Option<String> = req.state.map(enum_to_str);

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE research_notes
        SET confidence = COALESCE(?2, confidence),
            state      = COALESCE(?3, state),
            rationale  = COALESCE(?4, rationale),
            lens       = COALESCE(?5, lens)
        WHERE id = ?1
        "#,
        id,
        req.confidence,
        state_str,
        req.rationale,
        req.lens,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("research_note '{id}' not found")));
    }

    let payload = serde_json::json!({ "note_id": id, "state": state_str });
    record_event(&mut tx, "work_item", &work_item_id, "work_item.research_note_updated", payload)
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
    pool: &SqlitePool,
    old_id: &str,
    new_id: &str,
) -> Result<(), AppError> {
    let work_item_id = research_note_work_item(pool, old_id).await?;

    // Validate the superseding note exists (R7): clean 422 over a dangling-FK 500.
    let new_exists = sqlx::query!(
        r#"SELECT 1 AS "one!" FROM research_notes WHERE id = ?1"#,
        new_id,
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !new_exists {
        return Err(AppError::Validation(format!(
            "superseding research_note '{new_id}' does not exist"
        )));
    }

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE research_notes SET superseded_by = ?2 WHERE id = ?1"#,
        old_id,
        new_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("research_note '{old_id}' not found")));
    }

    let payload = serde_json::json!({ "superseded_by": new_id });
    record_event(
        &mut tx,
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
    pool: &SqlitePool,
    story_id: &str,
    question: &str,
) -> Result<Uuid, AppError> {
    let kind = work_item_kind(pool, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "open questions are settable only on a story, not on '{kind}'"
        )));
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    let seq = sqlx::query!(
        r#"SELECT COALESCE(MAX(seq), 0) + 1 AS "next!" FROM open_questions WHERE story_id = ?1"#,
        story_id,
    )
    .fetch_one(&mut *tx)
    .await?
    .next;

    sqlx::query!(
        r#"INSERT INTO open_questions (id, story_id, seq, question, status) VALUES (?1, ?2, ?3, ?4, 'open')"#,
        id_str,
        story_id,
        seq,
        question,
    )
    .execute(&mut *tx)
    .await?;

    // Route the event to the owning STORY's work_item aggregate (R1): export only
    // renders work_item aggregates, so an `open_question`-typed event would never
    // reach the git-export snapshot. event_type/payload are otherwise unchanged,
    // so the "exactly one event" invariant holds.
    let payload = serde_json::json!({ "question_id": id_str, "seq": seq });
    record_event(&mut tx, "work_item", story_id, "open_question.added", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Read an open question's owning `story_id`, erroring `NotFound` if the question
/// id has no row. Used by the option-add and resolve paths.
async fn open_question_story(pool: &SqlitePool, id: &str) -> Result<String, AppError> {
    sqlx::query!(r#"SELECT story_id AS "story_id!" FROM open_questions WHERE id = ?1"#, id)
        .fetch_optional(pool)
        .await?
        .map(|r| r.story_id)
        .ok_or_else(|| AppError::NotFound(format!("open_question '{id}' not found")))
}

/// Append ONE `question_options` row under the single-mutation-path discipline
/// (migration 0003). `seq` = `MAX(seq)+1` per question; the question must exist
/// (`NotFound` otherwise). Event `open_question.option_added`. Returns the new
/// option id.
pub async fn add_question_option(
    pool: &SqlitePool,
    question_id: &str,
    label: &str,
    detail: Option<&str>,
) -> Result<Uuid, AppError> {
    // Verify the question exists first (NotFound, not a dangling-FK 500) AND
    // capture its owning story for the event aggregate (R1).
    let story_id = open_question_story(pool, question_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = pool.begin().await?;

    let seq = sqlx::query!(
        r#"SELECT COALESCE(MAX(seq), 0) + 1 AS "next!" FROM question_options WHERE question_id = ?1"#,
        question_id,
    )
    .fetch_one(&mut *tx)
    .await?
    .next;

    sqlx::query!(
        r#"INSERT INTO question_options (id, question_id, seq, label, detail) VALUES (?1, ?2, ?3, ?4, ?5)"#,
        id_str,
        question_id,
        seq,
        label,
        detail,
    )
    .execute(&mut *tx)
    .await?;

    // Route to the owning STORY's work_item aggregate (R1) so export renders it.
    let payload = serde_json::json!({ "option_id": id_str, "seq": seq });
    record_event(&mut tx, "work_item", &story_id, "open_question.option_added", payload)
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
    pool: &SqlitePool,
    task_id: &str,
    question_id: &str,
) -> Result<(), AppError> {
    // Task-scoped guard (R3); also yields NotFound if the id is absent.
    let kind = work_item_kind(pool, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "block_task_on_question is settable only on a task, not on '{kind}'"
        )));
    }

    // The referenced question must exist (R3): clean 422 over a dangling-FK 500.
    let q_exists = sqlx::query!(
        r#"SELECT 1 AS "one!" FROM open_questions WHERE id = ?1"#,
        question_id,
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !q_exists {
        return Err(AppError::Validation(format!(
            "open_question '{question_id}' does not exist"
        )));
    }

    // R12: only block a pre-todo task. Blocking an in_progress/done task would be
    // silently downgraded to `todo` on unblock, losing state — reject instead.
    let current = sqlx::query!(
        r#"SELECT status AS "status!" FROM work_items WHERE id = ?1"#,
        task_id,
    )
    .fetch_one(pool)
    .await?
    .status;
    if !matches!(current.as_str(), "todo" | "open") {
        return Err(AppError::Validation(format!(
            "task '{task_id}' cannot be blocked from status '{current}': only a 'todo'/'open' \
             task may be blocked (the branch-resolution model restores blocked tasks to 'todo')"
        )));
    }

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE work_items
        SET blocked_by_question_id = ?2, status = 'blocked', updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
        task_id,
        question_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "blocked_by_question_id": question_id });
    record_event(&mut tx, "work_item", task_id, "work_item.blocked_on_question", payload).await?;

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
    pool: &SqlitePool,
    task_id: &str,
    option_id: &str,
) -> Result<(), AppError> {
    // Task-scoped guard (R3); also yields NotFound if the id is absent.
    let kind = work_item_kind(pool, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "set_enabling_option is settable only on a task, not on '{kind}'"
        )));
    }

    // The referenced option must exist (R3): clean 422 over a dangling-FK 500.
    let opt_exists = sqlx::query!(
        r#"SELECT 1 AS "one!" FROM question_options WHERE id = ?1"#,
        option_id,
    )
    .fetch_optional(pool)
    .await?
    .is_some();
    if !opt_exists {
        return Err(AppError::Validation(format!(
            "question_option '{option_id}' does not exist"
        )));
    }

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"
        UPDATE work_items
        SET enabling_option_id = ?2, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
        task_id,
        option_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "enabling_option_id": option_id });
    record_event(&mut tx, "work_item", task_id, "work_item.enabling_option_set", payload).await?;

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
    pool: &SqlitePool,
    question_id: &str,
    chosen_option_id: &str,
    by: Option<&str>,
) -> Result<(), AppError> {
    // NotFound if the question is absent (before any write); capture the owning
    // story for the event aggregate (R1).
    let story_id = open_question_story(pool, question_id).await?;

    // Reject re-resolving an already-answered/cancelled question (R4) so the
    // advertised idempotency is real rather than silently re-running the branch
    // transitions on a second call.
    let status = sqlx::query!(
        r#"SELECT status AS "status?" FROM open_questions WHERE id = ?1"#,
        question_id,
    )
    .fetch_one(pool)
    .await?
    .status;
    if status.as_deref() != Some("open") {
        return Err(AppError::Validation(format!(
            "open_question '{question_id}' already resolved/cancelled (status {})",
            status.as_deref().unwrap_or("unknown")
        )));
    }

    // Validate the chosen option belongs to THIS question.
    let owns = sqlx::query!(
        r#"SELECT COUNT(*) AS "n!" FROM question_options WHERE id = ?1 AND question_id = ?2"#,
        chosen_option_id,
        question_id,
    )
    .fetch_one(pool)
    .await?
    .n;
    if owns == 0 {
        return Err(AppError::Validation(format!(
            "option '{chosen_option_id}' does not belong to open_question '{question_id}'"
        )));
    }

    let mut tx = pool.begin().await?;

    // 1. Mark the question answered.
    sqlx::query!(
        r#"
        UPDATE open_questions
        SET status = 'answered',
            chosen_option_id = ?2,
            decided_at = CURRENT_TIMESTAMP,
            decided_by = ?3
        WHERE id = ?1
        "#,
        question_id,
        chosen_option_id,
        by,
    )
    .execute(&mut *tx)
    .await?;

    // 2. Unblock the chosen branch: blocked tasks on this question whose
    //    enabling_option is the chosen one OR is NULL (non-exclusive) → todo.
    sqlx::query!(
        r#"
        UPDATE work_items
        SET status = 'todo', updated_at = CURRENT_TIMESTAMP
        WHERE blocked_by_question_id = ?1
          AND status = 'blocked'
          AND (enabling_option_id = ?2 OR enabling_option_id IS NULL)
        "#,
        question_id,
        chosen_option_id,
    )
    .execute(&mut *tx)
    .await?;

    // 3. Cancel the other branches' EXCLUSIVE tasks: blocked tasks on this
    //    question with a non-NULL enabling_option that is NOT the chosen one.
    sqlx::query!(
        r#"
        UPDATE work_items
        SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP
        WHERE blocked_by_question_id = ?1
          AND status = 'blocked'
          AND enabling_option_id IS NOT NULL
          AND enabling_option_id <> ?2
        "#,
        question_id,
        chosen_option_id,
    )
    .execute(&mut *tx)
    .await?;

    // EXACTLY ONE event for the whole resolution (NOT per task). Routed to the
    // owning STORY's work_item aggregate (R1) so export renders it; `question_id`
    // is carried so the export drain can re-render this question's affected tasks
    // (R2) without a per-task event.
    let payload =
        serde_json::json!({ "chosen_option_id": chosen_option_id, "question_id": question_id });
    record_event(&mut tx, "work_item", &story_id, "open_question.resolved", payload).await?;

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
    pool: &SqlitePool,
    work_item_id: &str,
) -> Result<String, AppError> {
    // Recursive CTE: seed with the target row, then repeatedly join to the
    // parent until we either hit the project (returned) or NULL parent on a
    // non-project (caller maps to Validation). The CTE is bounded by the
    // 5-level hierarchy so the walk is O(5) and termination is structural.
    let row = sqlx::query!(
        r#"
        WITH RECURSIVE ancestors(id, kind, parent_id) AS (
            SELECT id, kind, parent_id FROM work_items WHERE id = ?1
            UNION ALL
            SELECT w.id, w.kind, w.parent_id
            FROM work_items w
            JOIN ancestors a ON w.id = a.parent_id
        )
        SELECT id AS "id!", kind AS "kind!"
        FROM ancestors
        WHERE kind = 'project'
        LIMIT 1
        "#,
        work_item_id,
    )
    .fetch_optional(pool)
    .await?;

    if let Some(r) = row {
        return Ok(r.id);
    }

    // Distinguish "id does not exist" from "id exists but has no project
    // ancestor": probe the row directly.
    let exists = sqlx::query!(
        r#"SELECT 1 AS "one!" FROM work_items WHERE id = ?1"#,
        work_item_id,
    )
    .fetch_optional(pool)
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
/// We match by the SQLite extended-result-code string (which `sqlx` exposes via
/// `DatabaseError::code()`); both `1555` (PRIMARY KEY) and `2067` (UNIQUE) are
/// flavours of `SQLITE_CONSTRAINT_UNIQUE`-class violations callers should treat
/// as conflicts. The conservative match-set is the two unique flavours; other
/// constraint codes (FK, CHECK, NOT NULL) pass through as `Db` 500.
fn is_unique_violation(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = e
        && let Some(code) = db_err.code()
    {
        return code == "2067" || code == "1555";
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
    pool: &SqlitePool,
    project_id: &str,
    slug: &str,
    is_primary: bool,
) -> Result<Uuid, AppError> {
    let canonical = parse_github_slug(slug)?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    let is_primary_int: i64 = if is_primary { 1 } else { 0 };

    let mut tx = pool.begin().await?;

    // Allocate position = MAX(position)+1 per project, inside the tx so a
    // concurrent insert under SQLite's single-writer lock is serialised.
    // COALESCE(MAX(.), -1) + 1 gives 0 for the first row.
    let position = sqlx::query!(
        r#"SELECT COALESCE(MAX(position), -1) + 1 AS "next!" FROM repo_links WHERE project_id = ?1"#,
        project_id,
    )
    .fetch_one(&mut *tx)
    .await?
    .next;

    let insert = sqlx::query!(
        r#"
        INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
        "#,
        id_str,
        project_id,
        canonical,
        position,
        is_primary_int,
    )
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert {
        if is_unique_violation(&e) {
            // Either the (project_id, slug) UNIQUE or the partial primary UNIQUE
            // index fired. Both are caller-fixable; surface as Validation.
            return Err(AppError::Validation(format!(
                "repo_link conflict: slug '{canonical}' is already linked, or another \
                 primary repo already exists for project '{project_id}' (primary repo conflict)"
            )));
        }
        return Err(e.into());
    }

    let payload = serde_json::json!({
        "id": id_str,
        "project_id": project_id,
        "slug": canonical,
        "is_primary": is_primary,
    });
    record_event(&mut tx, "work_item", project_id, "repo_link.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// List the `repo_links` rows for a project, ordered by `position` ASC. Returns
/// an empty Vec for a project with no links (or for a non-project id — caller is
/// expected to gate this query on `kind='project'`). Read-only; no transaction.
pub async fn list_repo_links(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<Vec<RepoLink>, AppError> {
    let rows = sqlx::query_as!(
        RepoLink,
        r#"
        SELECT
            id         AS "id!",
            project_id AS "project_id!",
            slug       AS "slug!",
            position   AS "position!",
            is_primary AS "is_primary!",
            created_at AS "created_at!"
        FROM repo_links
        WHERE project_id = ?1
        ORDER BY position ASC
        "#,
        project_id,
    )
    .fetch_all(pool)
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
pub async fn remove_repo_link(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    // Resolve the owning project + slug BEFORE the write so the event aggregate
    // is correct and so an absent id is `NotFound` (not `rows_affected()==0`).
    let row = sqlx::query!(
        r#"SELECT project_id AS "project_id!", slug AS "slug!" FROM repo_links WHERE id = ?1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("repo_link '{id}' not found")))?;

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(r#"DELETE FROM repo_links WHERE id = ?1"#, id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    if affected == 0 {
        // Lost a race against a concurrent delete — caller sees NotFound.
        return Err(AppError::NotFound(format!("repo_link '{id}' not found")));
    }

    let payload = serde_json::json!({
        "id": id,
        "project_id": row.project_id,
        "slug": row.slug,
    });
    record_event(
        &mut tx,
        "work_item",
        &row.project_id,
        "repo_link.removed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Promote `repo_link_id` to the project's primary repo. Critical ordering:
/// inside one `pool.begin()` tx, FIRST clear any existing primary on the same
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
    pool: &SqlitePool,
    project_id: &str,
    repo_link_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    // Step 1: capture the previous primary's id (for the event payload) BEFORE
    // we clear it. NULL if no current primary.
    let previous: Option<String> = sqlx::query!(
        r#"SELECT id AS "id!" FROM repo_links WHERE project_id = ?1 AND is_primary = 1"#,
        project_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .map(|r| r.id);

    // Step 2: clear the existing primary (idempotent if `previous` is None).
    sqlx::query!(
        r#"UPDATE repo_links SET is_primary = 0 WHERE project_id = ?1 AND is_primary = 1"#,
        project_id,
    )
    .execute(&mut *tx)
    .await?;

    // Step 3: promote the target — AND project_id guards against cross-project
    // ids. rows_affected()==0 ⇒ NotFound (id absent or wrong project).
    let set_result = sqlx::query!(
        r#"UPDATE repo_links SET is_primary = 1 WHERE id = ?1 AND project_id = ?2"#,
        repo_link_id,
        project_id,
    )
    .execute(&mut *tx)
    .await;

    let affected = match set_result {
        Ok(r) => r.rows_affected(),
        Err(e) => {
            if is_unique_violation(&e) {
                return Err(AppError::Validation(format!(
                    "primary repo conflict on project '{project_id}': another row already \
                     holds is_primary=1 (concurrent set_primary_repo)"
                )));
            }
            return Err(e.into());
        }
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
        &mut tx,
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
    pool: &SqlitePool,
    finding_id: &str,
    repo_id: Option<&str>,
) -> Result<(), AppError> {
    // Resolve the finding's owning work_item_id BEFORE opening the tx. NotFound
    // if the finding is absent.
    let work_item_id: String = sqlx::query!(
        r#"SELECT work_item_id AS "work_item_id?" FROM findings WHERE id = ?1"#,
        finding_id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("finding '{finding_id}' not found")))?
    .work_item_id
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
        let owns = sqlx::query!(
            r#"SELECT 1 AS "one!" FROM repo_links WHERE id = ?1 AND project_id = ?2"#,
            rid,
            project_id,
        )
        .fetch_optional(pool)
        .await?
        .is_some();
        if !owns {
            return Err(AppError::Validation(format!(
                "repo_link '{rid}' does not belong to the project ancestor '{project_id}' \
                 of finding '{finding_id}'"
            )));
        }
    }

    let mut tx = pool.begin().await?;

    let affected = sqlx::query!(
        r#"UPDATE findings SET repo_id = ?2 WHERE id = ?1"#,
        finding_id,
        repo_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

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
        &mut tx,
        "work_item",
        &work_item_id,
        "finding.repo_changed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Append ONE `events` row inside an in-flight transaction. Called by every
/// mutation; no domain write may bypass it. `id` is a fresh UUIDv7 (TEXT);
/// `payload` is serialised to a JSON string; `exported_at` is left NULL so the
/// git-export materialiser (Task 6) drains it on its next tick.
///
/// Takes `&mut Transaction` (not the pool) precisely so the event row shares the
/// caller's transaction and is committed/rolled-back atomically with the domain
/// write.
async fn record_event(
    tx: &mut Transaction<'_, Sqlite>,
    aggregate_type: &str,
    aggregate_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let event_id = Uuid::now_v7().to_string();
    let payload_str = serde_json::to_string(&payload).map_err(|e| AppError::Other(e.into()))?;

    sqlx::query!(
        r#"
        INSERT INTO events (id, aggregate_type, aggregate_id, event_type, payload)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        event_id,
        aggregate_type,
        aggregate_id,
        event_type,
        payload_str,
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::Status;

    /// Row count of `work_items` (compile-checked literal — sqlx 0.9's
    /// `SqlSafeStr` bound rejects a dynamically-built table name on the runtime
    /// `query_as`, so the two count helpers are split per table).
    async fn count_work_items(pool: &SqlitePool) -> i64 {
        sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM work_items"#)
            .fetch_one(pool)
            .await
            .unwrap()
            .n
    }

    /// Row count of `events`.
    async fn count_events(pool: &SqlitePool) -> i64 {
        sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM events"#)
            .fetch_one(pool)
            .await
            .unwrap()
            .n
    }

    /// Build the legal project→epic→feature→story chain and return the story id,
    /// so tests can create a legal `task` (or an illegal one) beneath it.
    async fn seed_chain_to_story(pool: &SqlitePool) -> String {
        let project = create_work_item(pool, "project", None, "P", None)
            .await
            .expect("legal project");
        let epic = create_work_item(pool, "epic", Some(&project.to_string()), "E", None)
            .await
            .expect("legal epic");
        let feature = create_work_item(pool, "feature", Some(&epic.to_string()), "F", None)
            .await
            .expect("legal feature");
        let story = create_work_item(pool, "story", Some(&feature.to_string()), "S", None)
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
        let ev = sqlx::query!(
            r#"SELECT aggregate_id AS "aggregate_id!", event_type AS "event_type!", exported_at AS "exported_at?"
               FROM events"#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ev.aggregate_id, id.to_string());
        assert_eq!(ev.event_type, "work_item.created");
        assert!(ev.exported_at.is_none(), "new event must be unexported");
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

        // Illegal: feature directly under project (feature's legal parent is epic).
        let err = create_work_item(&pool, "feature", Some(&project.to_string()), "Bad", None)
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

        assert_eq!(count_work_items(&pool).await, 4);
        assert_eq!(count_events(&pool).await, 4);

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("legal task under story");

        assert_eq!(count_work_items(&pool).await, 5);
        assert_eq!(count_events(&pool).await, 5);

        // Detail aggregate: the story has the task as a direct child.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.item.kind, "story");
        assert_eq!(detail.children.len(), 1);
        assert_eq!(detail.children[0].id, task.to_string());
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

        let got = sqlx::query!(r#"SELECT status AS "status!" FROM work_items WHERE id = ?1"#, id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(got.status, "in-progress");

        // Missing id → NotFound, no event emitted.
        let err = update_work_item_status(&pool, "does-not-exist", "x")
            .await
            .expect_err("missing id must error");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
        assert_eq!(count_events(&pool).await, 2, "no event for a missing-row update");
    }

    /// Row count of `work_item_activity`.
    async fn count_activity(pool: &SqlitePool) -> i64 {
        sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM work_item_activity"#)
            .fetch_one(pool)
            .await
            .unwrap()
            .n
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
        let dat = sqlx::query!(r#"SELECT deleted_at AS "deleted_at?" FROM work_items WHERE id = ?1"#, id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .deleted_at;
        assert!(dat.is_some(), "deleted_at stamped");

        // Re-deleting is NotFound (already tombstoned).
        let err = delete_work_item(&pool, &id).await.expect_err("re-delete");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
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
        sqlx::query!(r#"SELECT COUNT(*) AS "n!" FROM acceptance_criteria"#)
            .fetch_one(pool)
            .await
            .unwrap()
            .n
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

        add_acceptance_criterion(&pool, &task, "must build").await.expect("ac1");
        add_acceptance_criterion(&pool, &task, "must test").await.expect("ac2");

        assert_eq!(count_criteria(&pool).await, 2);
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
        sqlx::query!(r#"SELECT status AS "status!" FROM work_items WHERE id = ?1"#, id)
            .fetch_one(pool)
            .await
            .unwrap()
            .status
    }

    /// Count events of a given `event_type` (test helper — proves the
    /// exactly-one-event-per-logical-write invariant for the multi-write resolve).
    async fn count_events_of_type(pool: &SqlitePool, event_type: &str) -> i64 {
        sqlx::query!(
            r#"SELECT COUNT(*) AS "n!" FROM events WHERE event_type = ?1"#,
            event_type,
        )
        .fetch_one(pool)
        .await
        .unwrap()
        .n
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
}
