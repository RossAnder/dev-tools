---
description: Implement optimisation findings from /optimise — research-informed, verified changes
argument-hint: [item IDs to apply (preferred "O1,O3,O5"), or legacy numeric "1,3,5", or "all" / "critical"]
---

# Apply Optimisation Findings

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Implements the findings `/optimise` produced. Runs the shared apply pipeline (Step 0 → Step 6) under the vocabulary bound below, plus the optimise-specific deltas in `## Domain deltas`.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Contracts

Invoke the `flow-contract-apply-pipeline` skill to load the shared apply-pipeline contract: pre-flight gating, ledger location, selector semantics, the freshness gate, pre-analysis, file clustering, agent dispatch, the interim checkpoint, verification + ledger mutation, the final-summary skeleton, and deviation follow-up. Everything in this file either binds that contract's vocabulary or states an optimise-specific delta.

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (envelope shapes, `envelope.resolved.*` / `envelope.doctor.*` binding rules, project-local `.claude/` resolution, status vocabulary + completed-flow handling, slug derivation, canonical artifact paths, the mandatory bootstrap-summary console line, and the legacy `.claude/active-flow` ignore rule).

Invoke the `flow-contract-ledger-schema` skill to load the canonical ledger contract — one schema covers both the flow-local `optimise-findings.toml` and the flow-less fallback: the `[[items]]` field set, the severity / effort / category / disposition vocabularies, unknown-value fail-soft rules, the append-only `[[rollback_events]]` and `[[vet_events]]` logs, the parse-rewrite read/write contract with the `tomlctl items` query surface, the key-order convention, and the ID-assignment + dedup/regression rules.

Pipeline hooks — invoke each at its step:

- **Step 3** — invoke the `flow-contract-apply-dependency-sort` skill for Kahn's algorithm over the in-selection `depends_on` subset, the cycle-detection abort, and how the topological order feeds clustering into sequential batches.
- **Step 4** — invoke the `flow-contract-task-visibility` skill for the run-scoped task surface; `flow-contract-apply-pipeline` Step 4 carries the apply-flow binding (per-cluster subjects, the `verify` task, and the per-batch minting exception).
- **Step 4.5** — invoke the `flow-contract-apply-vet-implement-lite` skill for the post-cluster, pre-checkpoint vet of `implement-lite` apply tags (mandatory `[vet-recommended]` reads, per-cluster spot-sampling, expand-and-re-dispatch-to-deep on sample failure, the `vet:` console line).
- **Step 5.5** — invoke the `flow-contract-apply-rollback-protocol` skill for the rollback triggers, the stash-first touched-path revert sequence, the ledger reversal to `status = "open"` with `rollback_rationale`, the `[[rollback_events]]` append, and the confirmation prompts. Here the successful prior status is `applied` and the event log's `command` is `"optimise-apply"`.
- **Constraints** — invoke the `flow-contract-apply-constraints` skill for the rules bounding every cluster agent's edits: orchestrator front-loading, suggestion / dependency / public-API guardrails, behaviour preservation, one-concern-per-edit, minimum-change discipline, the 3-file-per-item hard cap and its `--file-budget` / `--allow-cross-file` overrides, the cap-exceeded skip-tag forms, and the no-auto-commit rule.

## Carrier vocabulary

| Token | Binding |
|---|---|
| `<CMD>` | `/optimise-apply` |
| `<PRODUCER>` | `/optimise` |
| `<ID>` | `O{n}` |
| `<LEDGER>` | `artifacts.optimise_findings` (typically `.claude/flows/<slug>/optimise-findings.toml`); flow-less fallback `.claude/optimise-findings/<scope>.toml` |
| `<APPLIED>` | `applied` |
| `<REJECTED>` | `wontapply` / `wontapply_rationale` |
| `<NO-CHANGE>` | `wontapply` / `wontapply_rationale` — the optimise schema has no `verified-clean` disposition, so moot findings (already in place, deleted source file) carry a rationale naming the reason rather than a distinct status |
| `<TERMINAL>` | `applied`, `wontapply` |
| `<CRITICAL-CATEGORIES>` | `memory`, `query`, `concurrency` |

## Step 0 envelope

```bash
tomlctl flow envelope build \
  --command optimise-apply \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)" \
  --require-artifact optimise_findings \
  --staleness-threshold 7d
```

Complete and copy-pasteable as-is — do NOT look up `--help`. `--require-artifact optimise_findings` pins the findings ledger as required (this command reads it before applying); `--staleness-threshold 7d` is the default, passed explicitly for clarity. On detached HEAD omit `--branch` so the envelope records `branch:null`. Add `--flow-override <slug>` when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. Dispatch via the `Task` tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt.

## Domain deltas

**Narration (Step 2)** — for `category = concurrency`, the pre-analysis notes must briefly state the invariant being restored (e.g. "lock ordering: outer lock A acquired before inner lock B to prevent deadlock", "async boundary: must not await while holding a non-async-aware lock", "channel capacity: bounded N prevents unbounded producer growth"), so downstream agents apply the optimisation rather than re-litigating the correctness argument. Forward this requirement to the Explore agent when pre-analysis is delegated.

**Clustering (Step 3)** — concurrency changes need extra sequencing care. A finding that flips a type from sync to async must be applied before any finding that modifies that type's callers; findings touching a shared primitive's consumers (e.g. Mutex → channel) belong in the same cluster as the primitive change, or in a strictly later batch.

**Agent result tags (Step 4)** — exactly one of two forms per finding: `applied O{n}: <summary>` (bytes written); `skipped O{n}: <reason>` (would break behaviour, unclear semantics, already in place with no byte written, requires deliberate refactor, or needs user confirmation on a public-API or schema change). **Optimise agents never emit `verified-clean`** — an optimisation is bytes-written by definition, so an already-optimal call site is either correctly in place (`skipped O{n}: already in place, no byte written`, transitioned to `wontapply` with that reason as rationale) or a regression of a prior fix, minted as a new O-item by the Step 5 cross-check.

**Step 5b: concurrency-semantic check (orchestrator-driven)** — for findings that modified concurrency primitives, synchronization, or task-spawning patterns, the orchestrator reads the changed code directly and confirms: synchronization primitives suit the access pattern and runtime (async-aware vs blocking, read-write vs exclusive); spawned tasks are bounded or tracked; and channel/queue capacity choices are intentional and documented with rationale. No sub-agent dispatch — the analysis benefits from the Step-2 pre-analysis context the orchestrator already carries, and the judgement does not fit the `verification` agent's run-and-report contract.

**Final summary** — title `## Applied Optimisations`; the `### Verified Clean` sub-section does NOT apply to this carrier (there is no "code already matches" audit state). `### Verification` reports build, tests, and any concurrency/memory checks.

**Constraints delta** — public-API or schema changes flagged by `concurrency` or `memory` findings need explicit user confirmation: agents emit `skipped O{n}: requires user confirmation on public API / schema change` and let the orchestrator surface the decision rather than applying unilaterally.
