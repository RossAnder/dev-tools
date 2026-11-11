Status: done — plan-family batch_defects rule stated in the engine skill and agent-waves reference

- claude/skills/review-v2-engine/SKILL.md: defect-verification row of the depth envelope points to a new "Plan family batching" paragraph (batch_defects, same_as rows stay single, reverify_single get a verifier each, the rest one batch; code and perf unchanged).
- claude/skills/review-v2-engine/references/agent-waves.md: routing-table row for plan-family critical/major defects under batch_defects, and a "Plan reverify" paragraph for the Agent-wave fallback.

Deviations: none.
Checks: accept grep passes; cargo test --lib cli::dispatch::tests 54 passed.
TANGENTIAL: none
