//! Canonical flow-artifact path map — single source of truth for the four
//! well-known artifacts that a flow's `context.toml` references.
//!
//! `to_pairs` yields the iterable form `flow::doctor` needs for its per-key
//! check; the named field accessors serve `flow::init` and `flow::resolve`.

/// Four canonical artifact paths for a flow, repo-relative. The string
/// values are exactly what `flow init` writes into `context.toml`'s
/// `[artifacts]` table.
#[derive(Debug, Clone)]
pub(crate) struct CanonicalArtifacts {
    pub(crate) review_ledger: String,
    pub(crate) optimise_findings: String,
    pub(crate) execution_record: String,
    pub(crate) plan_review_findings: String,
}

impl CanonicalArtifacts {
    /// Compute the canonical map from `slug`. Format:
    /// `.claude/flows/<slug>/<key-with-dashes>.toml`.
    pub(crate) fn for_slug(slug: &str) -> Self {
        Self {
            review_ledger: format!(".claude/flows/{slug}/review-ledger.toml"),
            optimise_findings: format!(".claude/flows/{slug}/optimise-findings.toml"),
            execution_record: format!(".claude/flows/{slug}/execution-record.toml"),
            plan_review_findings: format!(".claude/flows/{slug}/plan-review-findings.toml"),
        }
    }

    /// JSON projection: a 4-key object preserving the canonical order
    /// (`review_ledger`, `optimise_findings`, `execution_record`,
    /// `plan_review_findings`). `serde_json::Map` carries
    /// `preserve_order` (set in Cargo.toml) so the field order in the
    /// emitted JSON matches insertion order.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::with_capacity(4);
        m.insert(
            "review_ledger".to_string(),
            serde_json::Value::String(self.review_ledger.clone()),
        );
        m.insert(
            "optimise_findings".to_string(),
            serde_json::Value::String(self.optimise_findings.clone()),
        );
        m.insert(
            "execution_record".to_string(),
            serde_json::Value::String(self.execution_record.clone()),
        );
        m.insert(
            "plan_review_findings".to_string(),
            serde_json::Value::String(self.plan_review_findings.clone()),
        );
        serde_json::Value::Object(m)
    }

    /// Iterable view used by `flow::doctor` to walk per-key during the
    /// `artifacts-canonical` invariant check. The element order matches
    /// the historical `doctor::canonical_artifacts` output so any future
    /// "first divergence found" check stays deterministic.
    pub(crate) fn to_pairs(&self) -> [(&'static str, &str); 4] {
        [
            ("review_ledger", self.review_ledger.as_str()),
            ("optimise_findings", self.optimise_findings.as_str()),
            ("execution_record", self.execution_record.as_str()),
            ("plan_review_findings", self.plan_review_findings.as_str()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_slug_yields_four_canonical_paths() {
        let a = CanonicalArtifacts::for_slug("feature-x");
        assert_eq!(
            a.review_ledger,
            ".claude/flows/feature-x/review-ledger.toml"
        );
        assert_eq!(
            a.optimise_findings,
            ".claude/flows/feature-x/optimise-findings.toml"
        );
        assert_eq!(
            a.execution_record,
            ".claude/flows/feature-x/execution-record.toml"
        );
        assert_eq!(
            a.plan_review_findings,
            ".claude/flows/feature-x/plan-review-findings.toml"
        );
    }

    #[test]
    fn to_pairs_preserves_canonical_order() {
        let a = CanonicalArtifacts::for_slug("x");
        let pairs = a.to_pairs();
        let keys: Vec<&'static str> = pairs.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            vec![
                "review_ledger",
                "optimise_findings",
                "execution_record",
                "plan_review_findings"
            ]
        );
    }
}
