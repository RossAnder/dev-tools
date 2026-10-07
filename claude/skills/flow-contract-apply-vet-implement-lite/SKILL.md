---
name: flow-contract-apply-vet-implement-lite
description: "Canonical apply-vet-implement-lite contract for the apply-flow carriers (/optimise-apply, /review-apply) — defines the Step 4.5 orchestrator vet pass that fires on every `implement-lite` cluster return: which `applied`/`[vet-recommended]` tags must be inspected (every `[vet-recommended]`-flagged tag is a mandatory read; bare `applied` tags are spot-sampled), the per-cluster spot-sample minimum, the mandatory `file:line` check of every lite already-applied skip, the sample-failure expand-and-fix escalation to `implement-deep`, the deep-cluster skip rule, and the mandatory per-cluster vet console line and `[[vet_events]]` entry. Consult before running the post-cluster, pre-checkpoint vet pass in an apply-flow command."
---

## Step 4.5: Vet `implement-lite` apply tags (orchestrator)

After cluster agents return but BEFORE the Step 4.6 escalation routing and the interim checkpoint, the orchestrator MUST vet `applied` tags from `implement-lite` clusters. The Step 5a build/test verification catches bytes-don't-compile bugs and existing-test regressions, but it does NOT catch:

- Subtle correctness issues that compile and pass existing tests (e.g. an off-by-one that the tests don't exercise).
- Anti-pattern introductions (e.g. the agent picked an idiom that compiles but fights the surrounding code's style).
- Inconsistent style with surroundings (the lite agent's spec was met but the result reads jarringly).
- The lite agent flagged an apply with `[vet-recommended]` (per `implement-lite`'s output contract) — the agent itself surfaced residual uncertainty.

**Vetting procedure (per cluster):**

1. **Inspect every `applied <id> [vet-recommended]: ... — uncertain: <reason>` tag.** This is the agent's explicit ask; the orchestrator MUST read the touched files at the named line ranges and confirm the change is sound. The `uncertain:` reason says where to look first — the idiom it pattern-matched, the call site or test it left unread, the judgement call it made. If wrong, re-dispatch the failed item to `implement-deep` for a corrected fix.
2. **Spot-sample bare `applied` tags.** Sample at least **2 applies per cluster** (all of them when the cluster has 2 or fewer). Sample first any non-trivial edit whose report says `note: check not run` — nothing compiled it. For each sampled apply:
   - Read the touched lines and confirm the change matches the finding's recommended action.
   - Confirm the surrounding code's style (naming, error handling, idioms) is preserved or improved, not regressed.
   - Confirm the change makes structural sense and addresses the finding's root cause — not just satisfying the spec by adding a duplicating helper or silencing the symptom without fixing the underlying issue.
3. **Check every lite already-applied skip.** A `skipped <id>: already-applied` tag (or the prompt's own already-in-place words) becomes `<NO-CHANGE>` at the interim checkpoint and closes the finding with no diff for Step 5 to reconcile, so a false one is a silent loss. Read every such skip's cited `file:line` — not a sample — and confirm the finding's recommended post-state is there. A skip that cites no `file:line`, or whose cited lines do not carry the fix, fails the vet: re-dispatch it to `implement-deep` as an ordinary apply. These checks count toward N and M below.
4. **Sample-failure → expand-and-fix.** If a sampled apply fails vetting, **expand the sample to 100% of that cluster's applies** — the failure pattern likely affects others. For each failed apply, mark it for re-dispatch to `implement-deep` (do NOT revert silently — let `implement-deep` produce the correct fix and then verify).
5. **Skip sampling for `implement-deep` cluster output.** Deep clusters carry their own escalation discipline; the orchestrator's review focus is `implement-lite` output specifically. A spot-check of deep output remains advisable but is not gated.
6. **Record the vet outcome** per lite cluster, as a console line — `vet: cluster <id> — N applies sampled, M failed, K re-dispatched to deep` — and as one `[[vet_events]]` entry on the ledger, in the shape the ledger-schema contract defines. `command` is the bare carrier name (`review-apply`, `optimise-apply`), `lens` is `"implement-lite"`, `agent_index` is the cluster's ordinal (`c3` → `3`), `sampled_count` / `dropped_count` / `downgraded_count` are N / M / K, and `dropped_ids` lists the failed applies. The log is what lets lite's accuracy be judged across runs.

   ```bash
   cat <<'EOF' | tomlctl array-append <ledger> vet_events --json - --no-stamp
   {"timestamp":"<ISO 8601>","command":"review-apply","agent_index":3,"lens":"implement-lite","sampled_count":2,"dropped_count":1,"downgraded_count":1,"dropped_ids":["R12"],"rationale":"R12 guarded the wrong branch"}
   EOF
   ```

   Pass `--no-stamp`: this write lands mid-run and must not mark the ledger fresh before the run's final write.

The vet pass is what separates "the change succeeded" from "the right thing happened." Skipping it means a regression — bytes that compile and pass existing tests but break correctness, style, or root-cause coverage — can ship unnoticed.
