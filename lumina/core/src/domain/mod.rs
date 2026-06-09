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
//!
//! The concrete types are carved into cohesive sibling modules (D1 refactor) and
//! re-exported here so every type stays reachable at `crate::domain::X`:
//!   * [`enums`] — the closed-enum domain types.
//!   * [`work_items`] — the work-item row + detail aggregate, PTY rows, context
//!     blocks, and the work-item create/update bodies.
//!   * [`findings`] — the finding row + the planning/decision child-table rows
//!     and their update bodies.
//!   * [`planning`] — the read-model / input aggregates for the planning,
//!     dispatch, sprint, and finding-query pipelines.

mod enums;
mod findings;
mod planning;
mod work_items;

pub use enums::*;
pub use findings::*;
pub use planning::*;
pub use work_items::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
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

    #[test]
    fn planning_enums_round_trip_snake_case() {
        assert_snake(Relevance::Active, "active");
        assert_snake(Relevance::Backlog, "backlog");
        assert_snake(Relevance::Deferred, "deferred");
        assert_snake(Relevance::Rejected, "rejected");
        // Effort wire form is lowercase s|m|l — divergent from the display S/M/L.
        assert_snake(Effort::S, "s");
        assert_snake(Effort::M, "m");
        assert_snake(Effort::L, "l");
        assert_snake(Complexity::Low, "low");
        assert_snake(Complexity::Medium, "medium");
        assert_snake(Complexity::High, "high");
        assert_snake(Origin::Plan, "plan");
        assert_snake(Origin::Implement, "implement");
        assert_snake(Origin::Optimise, "optimise");
        assert_snake(Origin::Tdd, "tdd");
        assert_snake(Origin::None, "none");
        assert_snake(ResearchState::Proposed, "proposed");
        assert_snake(ResearchState::Accepted, "accepted");
        assert_snake(QuestionStatus::Open, "open");
        assert_snake(QuestionStatus::Cancelled, "cancelled");
        assert_snake(ClosureGate::Hard, "hard");
        assert_snake(ClosureGate::Soft, "soft");
    }

    #[test]
    fn migration_0011_enums_round_trip_snake_case() {
        // RunKind — wire forms must equal the runs.kind CHECK vocab.
        assert_snake(RunKind::Review, "review");
        assert_snake(RunKind::Optimise, "optimise");
        // RunStatus — runs.status CHECK vocab.
        assert_snake(RunStatus::Open, "open");
        assert_snake(RunStatus::Triaged, "triaged");
        assert_snake(RunStatus::Closed, "closed");
        // TargetKind — runs.target_kind CHECK vocab.
        assert_snake(TargetKind::Sprint, "sprint");
        assert_snake(TargetKind::Story, "story");
        // FindingDecisionKind — finding_decisions.decision CHECK vocab; the
        // two-word variants are snake_case (NOT kebab-case).
        assert_snake(FindingDecisionKind::SpawnTask, "spawn_task");
        assert_snake(FindingDecisionKind::SpawnStory, "spawn_story");
        assert_snake(FindingDecisionKind::Defer, "defer");
        assert_snake(FindingDecisionKind::Dismiss, "dismiss");
        assert_snake(FindingDecisionKind::Resolve, "resolve");
        // TriageState — findings.triage_state values (default `pending`).
        assert_snake(TriageState::Pending, "pending");
        assert_snake(TriageState::Accepted, "accepted");
        assert_snake(TriageState::Dismissed, "dismissed");
        assert_snake(TriageState::Deferred, "deferred");
        // FindingAxis — the query_findings count-by axis.
        assert_snake(FindingAxis::Severity, "severity");

        // Explicit json! assertions on the load-bearing two-word forms, to
        // pin them against the 0011 CHECK literals byte-for-byte.
        assert_eq!(
            serde_json::to_value(RunKind::Optimise).unwrap(),
            serde_json::json!("optimise")
        );
        assert_eq!(
            serde_json::to_value(FindingDecisionKind::SpawnTask).unwrap(),
            serde_json::json!("spawn_task")
        );
        assert_eq!(
            serde_json::to_value(FindingDecisionKind::SpawnStory).unwrap(),
            serde_json::json!("spawn_story")
        );
    }

    #[test]
    fn lane_round_trips_wire_form() {
        // Wire form must equal the work_items.lane CHECK vocab byte-for-byte.
        assert_snake(Lane::Implement, "implement");
        assert_snake(Lane::Review, "review");
        // Explicit assertions on the load-bearing wire strings.
        assert_eq!(serde_json::to_string(&Lane::Implement).unwrap(), "\"implement\"");
        assert_eq!(
            serde_json::from_str::<Lane>("\"review\"").unwrap(),
            Lane::Review
        );
    }

    #[test]
    fn relevance_schema_lists_all_variants() {
        let schema = schemars::schema_for!(Relevance);
        let value = serde_json::to_value(&schema).expect("schema to value");
        let mut got = Vec::new();
        collect_schema_variants(&value, &mut got);
        got.sort_unstable();
        got.dedup();
        let mut expected = ["active", "backlog", "deferred", "rejected"];
        expected.sort_unstable();
        assert_eq!(got, expected, "Relevance schema advertises all four variants");
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
        let mut expected = ["project", "epic", "focus", "story", "task"];
        expected.sort_unstable();
        assert_eq!(got, expected, "Kind schema advertises all five variants");
    }
}
