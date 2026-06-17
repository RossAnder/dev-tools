//! `sprint-quiescence:<sprint_id>` — the first concrete [`TopicResolver`]
//! (Wave 1, T6). Streams a sprint's [`lumina_core::domain::SprintQuiescence`]
//! aggregate (claimable / in_progress / blocked_on_question / in_review /
//! terminal counts plus the derived `done`/`stalled` verdict) as full snapshots.
//! The `in_review` bucket (1B-F9 M3, an unclaimed `status='review'` task) flows
//! through `serde_json::to_value(q)` automatically — the resolver serialises the
//! whole struct, so a new count field needs no resolver change.

use async_trait::async_trait;

use lumina_core::db::AnyPool;
use lumina_core::error::AppError;
use lumina_core::notify::ChangeNotification;

use crate::stream::TopicResolver;

/// Resolver for `sprint-quiescence:<sprint_id>` topics. The param is the
/// sprint id; the snapshot is `repo::get_sprint_quiescence`'s aggregate
/// serialised to JSON (snake_case fields, per the domain derive).
pub struct SprintQuiescenceTopic;

#[async_trait]
impl TopicResolver for SprintQuiescenceTopic {
    fn prefix(&self) -> &'static str {
        "sprint-quiescence"
    }

    /// Deliberately OVER-APPROXIMATES (the param is ignored): any committed
    /// `work_item` / `sprint` / `batch` / `worktree` change marks every
    /// quiescence subscription dirty. Quiescence derives from `work_items`
    /// rows (status / assignee / question-park / deps / checkpoint), the
    /// `sprint_tasks` junction (whose batch writes record `sprint`- or
    /// `batch`-aggregate events), the sprint's own status, and the
    /// merge/reject lifecycle (`worktree` events flip the owning sprint's
    /// status). Per the seam contract, `interested` MUST NOT
    /// under-approximate; false positives only cost one cheap recompute per
    /// 150 ms coalesce window, deduped-on-equal.
    fn interested(&self, _param: &str, change: &ChangeNotification) -> bool {
        matches!(
            change.aggregate_type.as_str(),
            "work_item" | "sprint" | "batch" | "worktree"
        )
    }

    /// Recompute the full snapshot. `get_sprint_quiescence` treats an
    /// unknown sprint id as an empty (trivially-done) sprint rather than
    /// `NotFound`, so a subscribe to a not-yet-created sprint yields a
    /// zeros `init` rather than an error frame.
    async fn resolve(&self, pool: &AnyPool, param: &str) -> Result<serde_json::Value, AppError> {
        let q = lumina_core::repo::get_sprint_quiescence(pool, param).await?;
        // `SprintQuiescence` is a closed struct of i64/bool fields, so this
        // serialise cannot fail in practice; the mapping mirrors the crate's
        // internal-failure idiom (`AppError::Other` → 500-class).
        serde_json::to_value(q).map_err(|e| AppError::Other(e.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use lumina_core::db::connect_in_memory;
    use lumina_core::domain::NewSprint;
    use lumina_core::repo;

    /// `interested` over-approximates across the four quiescence-feeding
    /// aggregate families and ignores the param; unrelated aggregates
    /// (pty/session/finding/run) do NOT dirty the topic.
    #[test]
    fn stream_quiescence_interested_matches_relevant_aggregates() {
        let topic = SprintQuiescenceTopic;
        for relevant in ["work_item", "sprint", "batch", "worktree"] {
            assert!(
                topic.interested("any-param", &ChangeNotification::new(relevant, "x", "created")),
                "{relevant} must dirty the quiescence topic"
            );
        }
        for irrelevant in ["session", "finding", "run"] {
            assert!(
                !topic.interested("any-param", &ChangeNotification::new(irrelevant, "x", "created")),
                "{irrelevant} must not dirty the quiescence topic"
            );
        }
    }

    /// `resolve` round-trips `get_sprint_quiescence` to JSON: a fresh
    /// (draft, taskless) sprint snapshots as all-zero counts with
    /// `done=true`; an unknown sprint id resolves the same way (no error).
    #[tokio::test]
    async fn stream_quiescence_resolve_snapshots_a_fresh_sprint() {
        let pool = connect_in_memory().await.expect("in-memory pool");
        let any: AnyPool = pool.clone().into();
        let sprint = repo::create_sprint(
            &pool,
            &NewSprint { title: None, worktree_id: None, predecessor_sprint_id: None },
        )
        .await
        .expect("sprint")
        .to_string();

        let topic = SprintQuiescenceTopic;
        let snapshot = topic.resolve(&any, &sprint).await.expect("resolve");
        assert_eq!(snapshot["claimable"], 0);
        assert_eq!(snapshot["in_progress"], 0);
        assert_eq!(snapshot["blocked_on_question"], 0);
        assert_eq!(snapshot["in_review"], 0, "1B-F9 M3: the new review bucket is surfaced in the stream payload");
        assert_eq!(snapshot["terminal"], 0);
        assert_eq!(snapshot["done"], true, "a taskless sprint is trivially done");
        assert_eq!(snapshot["stalled"], false);

        // Unknown sprint: the read treats it as empty, not NotFound.
        let unknown = topic.resolve(&any, "no-such-sprint").await.expect("resolve unknown");
        assert_eq!(unknown["done"], true);
    }
}
