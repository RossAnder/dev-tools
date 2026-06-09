//! Findings CRUD + batch-write + content-hash dedup (R3 carve).
//!
//! The public findings entry points — `update_finding`, `batch_update_findings`,
//! `supersede_finding`, `resolve_finding`, `create_finding`, `create_finding_tx`,
//! `finding_dedup_hash`, `add_findings` — plus the `NewFinding` /
//! `FindingTriageUpdate` input structs and the dedup INSERT/predicate single
//! sources. The shared substrate these compose on (`enum_to_str`) lives in
//! `repo/shared.rs` and is reached via `use super::*`; the event-outbox writers
//! come from `super::events`; `MAX_BATCH_ITEMS` is the ancestor-private batch cap
//! in `mod.rs`, named directly via `use super::MAX_BATCH_ITEMS`.
//!
//! `pub use findings::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here stays reachable at its existing `crate::repo::*` path (the HTTP
//! handlers / MCP tools / importer call them by path and are unchanged). The
//! domain types named in the signatures are imported explicitly from `crate::*`
//! (a `use super::*` glob does NOT carry super's private `use` imports).

use serde_json::Value;
use uuid::Uuid;

use super::*;
// Ancestor-private batch cap defined in `mod.rs` — a child module may name its
// ancestor's private items directly (mirrors `work_items.rs`).
use super::MAX_BATCH_ITEMS;
use super::events::{record_event, record_inert_event};
use crate::args;
use crate::db::{DbClient, Scalar};
use crate::domain::{Disposition, Severity, UpdateFindingRequest};
use crate::error::AppError;

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
    /// NON-terminal workflow `status` only; a terminal [`Disposition`] value
    /// (`fixed`/`wontfix`/`verified_clean`/`deferred`/`duplicate`) is rejected
    /// pre-tx (terminal dispositions belong to [`resolve_finding`]). R13: note
    /// the `deferred`/`duplicate` workflow-`status` values are rejected here —
    /// they are NOT the same axis as the triage-state `Deferred`/`Dismissed`
    /// dispositions, which are set via `record_finding_decision(Defer/Dismiss)`
    /// and ride the separate `triage_state` field above.
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
    // R14: an empty batch opens no tx and writes no coarse event — zero updated.
    if updates.is_empty() {
        return Ok(0);
    }
    // R3: reject an over-cap batch BEFORE any allocation / tx.
    if updates.len() > MAX_BATCH_ITEMS {
        return Err(AppError::Validation(format!(
            "batch of {} finding updates exceeds the maximum of {MAX_BATCH_ITEMS} per call",
            updates.len()
        )));
    }

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
    record_inert_event(
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

/// The dedup partial-index predicate (R10/R-B3). This MUST stay BYTE-IDENTICAL
/// to the `WHERE` clause of the `ux_findings_dedup` partial UNIQUE index in
/// `migrations/0011_runs_sprints_findings_queue.sql` (lines 70-71): SQLite binds
/// an `ON CONFLICT` upsert to a partial index ONLY when the conflict target's
/// predicate matches the index's predicate exactly — a one-byte drift silently
/// fails to bind the index and lets duplicate findings insert. Single-sourced as
/// a macro (not a `const`) because [`CREATE_FINDING_INSERT_SQL`] is built with
/// `concat!`, which accepts only literals — so the production INSERT and the
/// `findings_dedup_conflict_predicate_matches_migration` parity test expand the
/// SAME literal, and the test asserts the migration file embeds it verbatim.
macro_rules! findings_dedup_predicate {
    () => {
        "dedup_id IS NOT NULL AND superseded_by IS NULL"
    };
}

/// The dedup-aware `findings` INSERT used by [`create_finding_tx`]. Built by
/// concatenating the column/values clause with the shared `findings_dedup_predicate!`
/// macro so the `ON CONFLICT … WHERE <predicate> DO NOTHING` conflict-target
/// predicate is the SAME string the migration-0011 index uses (R10) and the
/// parity test checks.
const CREATE_FINDING_INSERT_SQL: &str = concat!(
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
        WHERE ",
    findings_dedup_predicate!(),
    " DO NOTHING"
);

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
            CREATE_FINDING_INSERT_SQL,
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
/// R3: a HARD cap of [`MAX_BATCH_ITEMS`] (500) rows per call is enforced at the
/// top — an over-cap batch is a clean [`AppError::Validation`] that writes
/// nothing (one transaction, one event for a legal batch). An empty batch is the
/// zero result with no tx (R14).
pub async fn add_findings(
    db: &impl DbClient,
    run_id: Option<&str>,
    items: &[(&str, NewFinding<'_>)],
) -> Result<crate::domain::BatchInsertResult, AppError> {
    // R14: an empty batch opens no tx and writes no coarse count:0 event — return
    // the zero result before any allocation.
    if items.is_empty() {
        return Ok(crate::domain::BatchInsertResult {
            added: 0,
            skipped: 0,
            skipped_ids: Vec::new(),
        });
    }
    // R3: reject an over-cap batch BEFORE the per-element hash allocation / tx —
    // an unbounded payload would force a huge pre-tx `Vec<String>` of hashes and
    // hold the writer lock across N inserts, starving other writers.
    if items.len() > MAX_BATCH_ITEMS {
        return Err(AppError::Validation(format!(
            "batch of {} findings exceeds the maximum of {MAX_BATCH_ITEMS} per call",
            items.len()
        )));
    }

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
    record_inert_event(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::repo::test_support::*;
    use sqlx::SqlitePool;

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

    /// R9: `add_findings` OWNS dedup — it stamps every element's `dedup_id` with
    /// the content hash over `(work_item_id, file, line, symbol, summary)`, and
    /// `work_item_id` is ALWAYS present, so the computed hash is never NULL. Two
    /// content-empty findings on the SAME work_item therefore hash IDENTICALLY and
    /// the second COLLAPSES onto the `ux_findings_dedup` partial index via
    /// `ON CONFLICT DO NOTHING` (added==1, skipped==1). This is the batch path's
    /// index-active behaviour, in deliberate contrast to the single `create_finding`
    /// path where a caller-supplied NULL `dedup_id` is index-EXEMPT and both rows
    /// persist (proven at the SQL layer in `tests/migration_0011.rs`). The original
    /// finding hypothesised added==2; the batch path's owned-hash dedup makes the
    /// real outcome a collapse — pinned here so a future change to the hash inputs
    /// (e.g. leaving content-empty findings NULL) is caught.
    #[tokio::test]
    async fn add_findings_content_empty_findings_collapse_via_owned_hash() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let a = NewFinding::default();
        let b = NewFinding::default();

        let r = add_findings(&pool, None, &[(story.as_str(), a), (story.as_str(), b)])
            .await
            .expect("batch add of two content-empty findings");
        assert_eq!(r.added, 1, "the first content-empty finding inserts");
        assert_eq!(
            r.skipped, 1,
            "the hash-identical second collapses — the batch path owns dedup"
        );
        assert_eq!(
            count_findings(&pool).await,
            1,
            "one row persisted after the dedup-collapse"
        );
    }

    /// R3: an over-cap `add_findings` batch is rejected with `Validation` and
    /// writes nothing (the cap is checked before any allocation / tx).
    #[tokio::test]
    async fn add_findings_rejects_over_cap_batch() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // MAX_BATCH_ITEMS + 1 distinct findings (distinct summaries → distinct
        // dedup hashes, so the over-cap rejection — not dedup — is what fires).
        let summaries: Vec<String> = (0..=MAX_BATCH_ITEMS).map(|i| format!("f{i}")).collect();
        let items: Vec<(&str, NewFinding)> = summaries
            .iter()
            .map(|s| {
                (
                    story.as_str(),
                    NewFinding { summary: Some(s.as_str()), ..NewFinding::default() },
                )
            })
            .collect();

        let res = add_findings(&pool, None, &items).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "an over-cap batch is a Validation, got {res:?}"
        );
        assert_eq!(
            count_findings(&pool).await,
            0,
            "over-cap rejection writes zero findings"
        );
    }

    /// R3: an empty `add_findings` batch is the zero result with no tx / no event
    /// (R14 early-return paired with the cap check).
    #[tokio::test]
    async fn add_findings_empty_batch_is_zero_result() {
        let pool = connect_in_memory().await.expect("pool");
        let events_before = count_events(&pool).await;
        let r = add_findings(&pool, None, &[]).await.expect("empty batch");
        assert_eq!(r.added, 0);
        assert_eq!(r.skipped, 0);
        assert!(r.skipped_ids.is_empty());
        assert_eq!(
            count_events(&pool).await,
            events_before,
            "an empty batch records no coarse event"
        );
    }

    /// R10 / R-B3: the dedup conflict-target predicate baked into
    /// `CREATE_FINDING_INSERT_SQL` MUST stay byte-identical to the
    /// `ux_findings_dedup` partial-index predicate in migration 0011 — a one-byte
    /// drift fails to bind the index and silently lets duplicates insert. Both are
    /// pinned to the single-source `findings_dedup_predicate!` macro here.
    #[test]
    fn findings_dedup_conflict_predicate_matches_migration() {
        // The production INSERT embeds the predicate verbatim.
        assert!(
            CREATE_FINDING_INSERT_SQL.contains(findings_dedup_predicate!()),
            "the create_finding INSERT must embed the shared dedup predicate"
        );
        assert!(
            CREATE_FINDING_INSERT_SQL
                .contains(concat!("WHERE ", findings_dedup_predicate!(), " DO NOTHING")),
            "the ON CONFLICT target uses the shared predicate as its WHERE clause"
        );
        // The migration's partial-index predicate is the SAME string.
        const MIGRATION_0011: &str =
            include_str!("../../migrations/0011_runs_sprints_findings_queue.sql");
        assert!(
            MIGRATION_0011.contains(findings_dedup_predicate!()),
            "the ux_findings_dedup index predicate in migration 0011 must match \
             the findings_dedup_predicate! macro byte-for-byte"
        );
    }
}
