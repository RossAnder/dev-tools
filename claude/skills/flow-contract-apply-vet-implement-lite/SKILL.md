---
name: flow-contract-apply-vet-implement-lite
description: Canonical apply-vet-implement-lite contract for the apply-flow carriers (/optimise-apply, /review-apply) — defines the Step 4.5 orchestrator vet pass that fires on every `implement-lite` cluster return: which `applied`/`[vet-recommended]` tags must be inspected (every `[vet-recommended]`-flagged tag is a mandatory read; bare `applied` tags are spot-sampled), the per-cluster spot-sample minimum, the sample-failure expand-and-fix escalation to `implement-deep`, the deep-cluster skip rule, and the mandatory per-cluster vet console line. Consult before running the post-cluster, pre-checkpoint vet pass in an apply-flow command.
---

## Step 4.5: Vet `implement-lite` apply tags (orchestrator)

After cluster agents return but BEFORE the interim checkpoint, the orchestrator (Opus) MUST vet `applied` tags from `implement-lite` clusters. The Step 5a build/test verification catches bytes-don't-compile bugs and existing-test regressions, but it does NOT catch:

- Subtle correctness issues that compile and pass existing tests (e.g. an off-by-one that the tests don't exercise).
- Anti-pattern introductions (e.g. the agent picked an idiom that compiles but fights the surrounding code's style).
- Inconsistent style with surroundings (the lite agent's spec was met but the result reads jarringly).
- The lite agent flagged an apply with `[vet-recommended]` (per `implement-lite`'s output contract) — the agent itself surfaced residual uncertainty.

**Vetting procedure (per cluster):**

1. **Inspect every `applied <id> [vet-recommended]: ...` tag.** This is the agent's explicit ask; the orchestrator MUST read the touched files at the named line ranges and confirm the change is sound. If wrong, re-dispatch the failed item to `implement-deep` for a corrected fix.
2. **Spot-sample bare `applied` tags.** Sample at least **1 applied per cluster** (or all if the cluster has fewer than 3 applies). For each sampled apply:
   - Read the touched lines and confirm the change matches the finding's recommended action.
   - Confirm the surrounding code's style (naming, error handling, idioms) is preserved or improved, not regressed.
   - Confirm the change makes structural sense and addresses the finding's root cause — not just satisfying the spec by adding a duplicating helper or silencing the symptom without fixing the underlying issue.
3. **Sample-failure → expand-and-fix.** If a sampled apply fails vetting, **expand the sample to 100% of that cluster's applies** — the failure pattern likely affects others. For each failed apply, mark it for re-dispatch to `implement-deep` (do NOT revert silently — let `implement-deep` produce the correct fix and then verify).
4. **Skip sampling for `implement-deep` cluster output.** Deep clusters carry their own escalation discipline; the orchestrator's review focus is `implement-lite` output specifically. A spot-check of deep output remains advisable but is not gated.
5. **Record the vet outcome** as a console line per cluster: `vet: cluster <id> — N applies sampled, M failed, K re-dispatched to deep`.

The vet pass is what separates "the change succeeded" from "the right thing happened." Skipping it means a regression — bytes that compile and pass existing tests but break correctness, style, or root-cause coverage — can ship unnoticed.
