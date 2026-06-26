//! Scheduled-unit domain types (migration 0028, focus 1C.3): the foundation for
//! an in-process tokio scheduler that drives STORY/SPRINT-scale planning,
//! sprint-composition, and merge work out of a durable `scheduled_units` queue.
//!
//! This module defines only the typed wire/read shapes — the
//! [`ScheduledUnitKind`] dispatch enum and the [`ScheduledUnit`] row aggregate.
//! The repo reads/writes (the dispatch-lease primitive) and the scheduler loop
//! are the NEXT tasks and build on these types; nothing here touches the DB.
//!
//! Carved into its own module (registered in `domain/mod.rs`); re-exported via
//! `pub use scheduler::*` so the types stay reachable at `crate::domain::X`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The dispatch kind of a [`ScheduledUnit`] (migration 0028) — CHECK-enforced at
/// the DB layer on the `scheduled_units.kind` column
/// (`build_story|build_tasks|compose_sprint|drive`). It is a CLOSED vocabulary
/// the scheduler switches on, so a stray value fails loudly at the DB CHECK. The
/// wire form matches the SQL CHECK literals byte-for-byte (snake_case). Used at
/// the MCP-param / scheduler-dispatch layer; the [`ScheduledUnit`] row struct
/// carries `kind` as a non-`Option` `String` per the row-struct idiom (mirrors
/// [`crate::domain::PtySession::source`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledUnitKind {
    /// Build (plan) a story.
    BuildStory,
    /// Decompose a story into tasks.
    BuildTasks,
    /// Compose a sprint over a planned story.
    ComposeSprint,
    /// Drive a composed sprint through execution to merge.
    Drive,
}

/// A row of `scheduled_units` (migration 0028): one durable scheduler claim/lease
/// over a (kind, work_item) driver job, at STORY/SPRINT scale. DELIBERATELY a
/// dedicated table — NOT an overload of the team-execution `work_items.assignee`
/// / `lease_expires_at` columns (migration 0013), which carry TASK-claim
/// semantics for the per-task agent work-queue.
///
/// `kind` is the typed [`ScheduledUnitKind`] dispatch vocab, carried as a
/// non-`Option` `String` per the row-struct idiom (see
/// [`crate::domain::PtySession::source`]). `status` is FREE TEXT (repo-validated,
/// like `work_items.status` / `sprints.status`), defaulting to `pending`.
/// `assignee` / `lease_expires_at` are the claim owner + ISO-8601 lease deadline
/// (NULL = unclaimed; a past deadline is lazily reclaimable, mirroring the
/// migration-0013 claim sweep). `plan_epoch` captures the work-item's plan epoch
/// at dispatch time. Read aggregate only — `Serialize`, no `JsonSchema` (mirrors
/// the sibling row structs `WorkItem`/`Finding`/`PtySession`).
#[derive(Debug, Clone, Serialize)]
pub struct ScheduledUnit {
    pub id: String,
    /// The dispatch kind; the typed [`ScheduledUnitKind`] is the wire / MCP form.
    pub kind: String,
    /// The `work_items.id` of the story/sprint work-item this unit drives.
    pub work_item_id: String,
    /// Free-text lifecycle status (repo-validated; the column default is `pending`).
    pub status: String,
    /// The scheduler worker id holding the current lease; `None` when unclaimed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    /// ISO-8601 lease deadline; `None` when unclaimed. A past value is reclaimable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<String>,
    /// The work-item's `plan_epoch` captured at dispatch time (`NOT NULL DEFAULT 0`).
    pub plan_epoch: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduled_unit_kind_round_trips_wire_form() {
        // migration 0028: wire forms must equal the scheduled_units.kind CHECK
        // vocab byte-for-byte (snake_case).
        for (value, expected) in [
            (ScheduledUnitKind::BuildStory, "build_story"),
            (ScheduledUnitKind::BuildTasks, "build_tasks"),
            (ScheduledUnitKind::ComposeSprint, "compose_sprint"),
            (ScheduledUnitKind::Drive, "drive"),
        ] {
            let json = serde_json::to_value(value).expect("serialise");
            assert_eq!(json, serde_json::Value::String(expected.to_owned()), "wire form");
            let back: ScheduledUnitKind =
                serde_json::from_value(json).expect("deserialise");
            assert_eq!(back, value, "round-trip");
        }
    }
}
