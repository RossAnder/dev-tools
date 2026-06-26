//! Shared repo substrate — row decoders, enum/JSON helpers, hierarchy &
//! attribute validators, the GitHub-slug parser, the create-in-tx primitive,
//! the closure/epic-done gates, the project-ancestor walk, and the nested
//! detail readers (`list_*`).
//!
//! These are the cross-cluster helpers consumed by the domain mutator clusters
//! that remain in `repo/mod.rs`. Every item is `pub(crate)` (so the sibling
//! clusters keep resolving the calls), except [`parse_github_slug`], which is
//! `pub` to preserve the external `crate::repo::parse_github_slug` path.
//!
//! `use super::*` reaches the private items still defined in `mod.rs` that these
//! helpers depend on — `KINDS`, `MAX_PLAN_FIELD_BYTES`, `CreateOpts`,
//! `CREATE_WORK_ITEM_INSERT_SQL`, and the `OpenQuestionRow`/`ActivityRow`
//! raw-row structs the `list_*` readers consume.

use serde_json::Value;
use uuid::Uuid;

use crate::args;
use crate::db::DbClient;
use crate::domain::{
    AcceptanceCriterion, ActivityType, Finding, OpenQuestion, QuestionOption, ResearchNote, Shape,
    TaskResearchLink, WorkItem, WorkItemActivity,
};
use crate::error::AppError;

// Private items that REMAIN in `mod.rs` but are consumed by the helpers moved
// here. A child module may name its ancestor's private items directly.
use super::{
    ActivityRow, CreateOpts, KINDS, MAX_PLAN_FIELD_BYTES, OpenQuestionRow,
    CREATE_WORK_ITEM_INSERT_SQL,
};

/// Raw `work_items` row as it comes off the database, before `attributes` is
/// decoded from its stored TEXT into `Option<Value>`. Generic over `R: Row` per
/// the canonical [`crate::db`] FromRow recipe so it rides `query_*<T>` on both
/// the SQLite arm today and a future Pg arm unchanged. The column→field
/// nullability is carried by the field types (`String` vs `Option<String>`),
/// replacing the old `AS "col!"`/`"col?"` macro hints.
#[derive(Debug)]
pub(crate) struct WorkItemRow {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) parent_id: Option<String>,
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) status: String,
    pub(crate) position: Option<i64>,
    pub(crate) attributes: Option<String>,
    pub(crate) relevance: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) complexity: Option<String>,
    pub(crate) origin: Option<String>,
    pub(crate) closure_gate: Option<String>,
    pub(crate) blocked_by_question_id: Option<String>,
    pub(crate) enabling_option_id: Option<String>,
    pub(crate) task_kind: Option<String>,
    pub(crate) tier: Option<String>,
    pub(crate) shape: Option<String>,
    pub(crate) spawned_from_finding_id: Option<String>,
    pub(crate) assignee: Option<String>,
    pub(crate) lease_expires_at: Option<String>,
    pub(crate) lane: Option<String>,
    pub(crate) reviews_work_item_id: Option<String>,
    /// Checkpoint-barrier flag (migration 0016): the nullable `INTEGER` 0/1
    /// column, decoded as `Option<i64>` per the row-struct idiom (SQLite stores
    /// the bool as INTEGER) and mapped to `Option<bool>` on the public
    /// [`WorkItem`] in [`work_item_from_row`].
    pub(crate) checkpoint: Option<i64>,
    /// Rework plan epoch (migration 0026): the `NOT NULL DEFAULT 0` INTEGER
    /// column, decoded as a non-null `i64` (every row carries at least epoch 0)
    /// and mapped straight onto [`WorkItem::plan_epoch`].
    pub(crate) plan_epoch: i64,
    /// Autonomous-drive depth (migration 0028): the nullable
    /// `plan-only|compose-sprint|drive-to-merge` CHECK column, decoded as
    /// `Option<String>` per the row-struct idiom (see `lane`/`tier`/`shape`) and
    /// mapped straight onto [`WorkItem::drive_depth`].
    pub(crate) drive_depth: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    /// Soft-delete tombstone instant (NULL = live). Selected by both
    /// `GET_WORK_ITEM_DETAIL_SQL` and `LIST_WORK_ITEMS_SQL` so the export
    /// tombstone fold reads it off the detail row instead of issuing a separate
    /// query (O17). Maps to the `#[serde(skip_serializing)]` `WorkItem.deleted_at`.
    pub(crate) deleted_at: Option<String>,
}

impl<'r, R> sqlx::FromRow<'r, R> for WorkItemRow
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
            assignee: row.try_get("assignee")?,
            lease_expires_at: row.try_get("lease_expires_at")?,
            lane: row.try_get("lane")?,
            reviews_work_item_id: row.try_get("reviews_work_item_id")?,
            checkpoint: row.try_get("checkpoint")?,
            plan_epoch: row.try_get("plan_epoch")?,
            drive_depth: row.try_get("drive_depth")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            deleted_at: row.try_get("deleted_at")?,
        })
    }
}

/// Decode a [`WorkItemRow`] into the public [`WorkItem`], turning the raw
/// `attributes` TEXT into `Option<Value>` via [`decode_attributes`].
pub(crate) fn work_item_from_row(r: WorkItemRow) -> Result<WorkItem, AppError> {
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
        assignee: r.assignee,
        lease_expires_at: r.lease_expires_at,
        lane: r.lane,
        reviews_work_item_id: r.reviews_work_item_id,
        checkpoint: r.checkpoint.map(|v| v != 0),
        plan_epoch: r.plan_epoch,
        drive_depth: r.drive_depth,
        created_at: r.created_at,
        updated_at: r.updated_at,
        deleted_at: r.deleted_at,
    })
}

/// Decode a nullable `attributes` TEXT column into `Option<Value>`. A non-NULL
/// column that does not parse as JSON is a stored-data corruption, surfaced as
/// `Other` (→ 500) rather than swallowed — the write-side normalisation
/// guarantees only valid JSON objects are ever stored.
pub(crate) fn decode_attributes(raw: Option<String>) -> Result<Option<Value>, AppError> {
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
pub(crate) fn enum_to_str<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        _ => unreachable!("unit domain enum serialises to a JSON string"),
    }
}

/// Validate that `entry_kind` is a legal [`ActivityType`] wire value, returning
/// the canonical spelling. Typed `Validation` (NOT a panic) on an illegal value.
pub(crate) fn validate_entry_kind(entry_kind: &str) -> Result<String, AppError> {
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
pub(crate) fn normalise_object(value: &Value, what: &str) -> Result<serde_json::Map<String, Value>, AppError> {
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
pub(crate) fn validate_attributes_for_kind(
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

/// Validate a `(kind, parent_kind)` edge against the hierarchy rules. Returns
/// the typed `Validation` error rather than relying on the DB trigger, so a bad
/// edge becomes a 422 instead of a raw trigger error mapped to 500.
///
/// Rules (mirroring the migration trigger):
///   * `project` ⇔ parent is NULL.
///   * every other kind ⇔ parent kind is the immediately-higher level.
pub(crate) fn validate_hierarchy_edge(kind: &str, parent_kind: Option<&str>) -> Result<(), AppError> {
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

/// List the LIVE research-note rows for a work item (migration 0003), ordered by
/// the per-item monotonic `seq`. "Live" = `superseded_by IS NULL`: a note
/// superseded by a newer one drops out of this fold. Runtime seam: `query_all`
/// onto the [`ResearchNote`] read struct (all columns map 1:1 via its FromRow).
pub(crate) async fn list_research_notes(
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
            anchors,
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

/// List the LIVE task↔research grounding edges for a task (migration 0026),
/// each JOINed to its `research_notes` endpoint for the note `summary`. "Live" =
/// `research_notes.superseded_by IS NULL`: an edge whose note was superseded
/// drops out of this fold (mirroring [`list_research_notes`]). Ordered by the
/// edge's `created_at` for a stable fold. Two columns, so the read decodes
/// through the tuple `FromRow` seam (`(research_note_id, summary)`) and maps into
/// the public [`TaskResearchLink`].
pub(crate) async fn list_task_research_links(
    db: &impl DbClient,
    task_id: &str,
) -> Result<Vec<TaskResearchLink>, AppError> {
    let rows = db
        .query_all::<(String, String)>(
            r#"
        SELECT trl.research_note_id, rn.summary
        FROM task_research_links trl
        JOIN research_notes rn ON rn.id = trl.research_note_id
        WHERE trl.task_id = $1
          AND rn.superseded_by IS NULL
        ORDER BY trl.created_at, trl.research_note_id
        "#,
            args![task_id.to_owned()],
        )
        .await?;

    Ok(rows
        .into_iter()
        .map(|(research_note_id, summary)| TaskResearchLink {
            research_note_id,
            summary,
        })
        .collect())
}

/// List the open-question rows for a story (migration 0003), ordered by the
/// per-story monotonic `seq`, EACH with its `question_options` (also `seq`-
/// ordered) folded into the nested `options` Vec. Two queries regardless of
/// question count: the questions query reads the scalar columns into
/// [`OpenQuestionRow`], then ONE options query reads every option for the
/// story's questions into [`QuestionOption`] (`ORDER BY question_id, seq` so each
/// per-question group is already in `seq` order), and the loop assembles the
/// public [`OpenQuestion`], taking each question's options out of the grouped map.
pub(crate) async fn list_open_questions(
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

    // One bulk options read for every question on this story (idx_question_options
    // _question(question_id, seq) supports the scan). The `ORDER BY question_id,
    // seq` guarantees per-group seq order, so the grouped map preserves it.
    let option_rows = db
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
        WHERE question_id IN (SELECT id FROM open_questions WHERE story_id = $1)
        ORDER BY question_id, seq
        "#,
            args![story_id.to_owned()],
        )
        .await?;

    let mut options_by_question: std::collections::HashMap<String, Vec<QuestionOption>> =
        std::collections::HashMap::new();
    for opt in option_rows {
        options_by_question
            .entry(opt.question_id.clone())
            .or_default()
            .push(opt);
    }

    let mut out = Vec::with_capacity(questions.len());
    for q in questions {
        let options = options_by_question.remove(&q.id).unwrap_or_default();

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
pub(crate) async fn list_activity(
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
        lane,
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

    // Lane is a TASK-ONLY field (team-execution). The single source of the
    // create-time default lives HERE so every create path (the simple
    // `create_work_item` test helper, `create_work_item_with_origin`, the MCP
    // `create_work_item` tool, and the bulk `create_work_items`) inherits it:
    //   * `kind == "task"`: use the caller-provided `lane` if present, ELSE
    //     default to `'implement'` — so every freshly-planned task is claimable
    //     by `claim_next_task` (whose candidate select is `lane = 'implement'`)
    //     without a separate setter call.
    //   * any non-task kind: `lane` stays NULL (a caller-supplied `lane` on a
    //     non-task is silently ignored — lane has no meaning off a task, and the
    //     `lane` setter / claim path only ever read it for tasks).
    let lane_value: Option<String> = if kind == "task" {
        Some(lane.map(enum_to_str).unwrap_or_else(|| "implement".to_owned()))
    } else {
        None
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
            attributes_str,
            lane_value
        ],
    )
    .await?;

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
pub(crate) async fn enforce_closure_gate(
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
pub(crate) async fn enforce_epic_done_gate(
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

/// Fetch a work item's `kind`, erroring `NotFound` if the id has no row. Used by
/// the attribute-validating write paths to resolve the per-kind contract before
/// touching the row. Does NOT filter `deleted_at` (callers decide).
pub(crate) async fn work_item_kind(db: &impl DbClient, id: &str) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        r#"SELECT kind FROM work_items WHERE id = $1"#,
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))
}

/// R23: reject a plan-attribute string whose UTF-8 byte length exceeds
/// [`MAX_PLAN_FIELD_BYTES`]. Called where each plan field's patch value is built
/// so `outcome`/`context`/`framing` share one cap with no per-field duplication.
pub(crate) fn check_plan_field_len(field: &str, value: &str) -> Result<(), AppError> {
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
pub(crate) fn validate_plan_field_constraints(
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
pub(crate) fn is_unique_violation(backend: crate::db::Backend, e: &sqlx::Error) -> bool {
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
