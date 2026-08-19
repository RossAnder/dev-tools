---
description: Apply review findings from /review — transition open review-ledger items to fixed / wontfix / verified-clean with resolution evidence
argument-hint: [R1,R3 | all | critical | critical,warnings | empty for default]
---

# Apply Review Findings

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Implements the findings `/review` produced. Runs the shared apply pipeline (Step 0 → Step 6) under the vocabulary bound below, plus the review-specific deltas in `## Domain deltas`.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Contracts

Invoke the `flow-contract-apply-pipeline` skill to load the shared apply-pipeline contract: pre-flight gating, ledger location, selector semantics, the freshness gate, pre-analysis, file clustering, agent dispatch, the interim checkpoint, verification + ledger mutation, the final-summary skeleton, and deviation follow-up. Everything in this file either binds that contract's vocabulary or states a review-specific delta.

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (envelope shapes, `envelope.resolved.*` / `envelope.doctor.*` binding rules, project-local `.claude/` resolution, status vocabulary + completed-flow handling, slug derivation, canonical artifact paths, the mandatory bootstrap-summary console line, and the legacy `.claude/active-flow` ignore rule).

Invoke the `flow-contract-ledger-schema` skill to load the canonical ledger contract — one schema covers both the flow-local `review-ledger.toml` and the flow-less fallback: the `[[items]]` field set, the severity / effort / category / disposition vocabularies (including `verified-clean`), unknown-value fail-soft rules, the append-only `[[rollback_events]]` and `[[vet_events]]` logs, the parse-rewrite read/write contract with the `tomlctl items` query surface, the key-order convention, and the ID-assignment + dedup/regression rules.

Pipeline hooks — invoke each at its step:

- **Step 3** — invoke the `flow-contract-apply-dependency-sort` skill for Kahn's algorithm over the in-selection `depends_on` subset, the cycle-detection abort, and how the topological order feeds clustering into sequential batches.
- **Step 4** — invoke the `flow-contract-task-visibility` skill for the run-scoped task surface; `flow-contract-apply-pipeline` Step 4 carries the apply-flow binding (per-cluster subjects, the `verify` task, and the per-batch minting exception).
- **Step 4.5** — invoke the `flow-contract-apply-vet-implement-lite` skill for the post-cluster, pre-checkpoint vet of `implement-lite` apply tags (mandatory `[vet-recommended]` reads, per-cluster spot-sampling, expand-and-re-dispatch-to-deep on sample failure, the `vet:` console line).
- **Step 5.5** — invoke the `flow-contract-apply-rollback-protocol` skill for the rollback triggers, the stash-first touched-path revert sequence, the ledger reversal to `status = "open"` with `rollback_rationale`, the `[[rollback_events]]` append, and the confirmation prompts. Here the successful prior status is `fixed` and the event log's `command` is `"review-apply"`.
- **Constraints** — invoke the `flow-contract-apply-constraints` skill for the rules bounding every cluster agent's edits: orchestrator front-loading, suggestion / dependency / public-API guardrails, behaviour preservation, one-concern-per-edit, minimum-change discipline, the 3-file-per-item hard cap and its `--file-budget` / `--allow-cross-file` overrides, the cap-exceeded skip-tag forms, and the no-auto-commit rule.

## Carrier vocabulary

| Token | Binding |
|---|---|
| `<CMD>` | `/review-apply` |
| `<PRODUCER>` | `/review` |
| `<ID>` | `R{n}` |
| `<LEDGER>` | `artifacts.review_ledger` (typically `.claude/flows/<slug>/review-ledger.toml`); flow-less fallback `.claude/reviews/<scope>.toml` |
| `<APPLIED>` | `fixed` |
| `<REJECTED>` | `wontfix` / `wontfix_rationale` |
| `<NO-CHANGE>` | `verified-clean` / `verified_note` |
| `<TERMINAL>` | `fixed`, `wontfix`, `verified-clean` |
| `<CRITICAL-CATEGORIES>` | `security`, `db` |

## Step 0 envelope

```bash
tomlctl flow envelope build \
  --command review-apply \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)" \
  --require-artifact review_ledger \
  --staleness-threshold 7d
```

Complete and copy-pasteable as-is — do NOT look up `--help`. `--require-artifact review_ledger` pins the ledger as required (this command reads it before applying); `--staleness-threshold 7d` is the default, passed explicitly for clarity. On detached HEAD omit `--branch` so the envelope records `branch:null`. Add `--flow-override <slug>` when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. Dispatch via the `Task` tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt.

## Domain deltas

**Narration (Step 2)** — for `category ∈ {security, architecture}`, the pre-analysis notes must briefly state the threat model or invariant being restored (e.g. "SQLi: untrusted input flows into raw query", "layering: domain module reaching into infrastructure"), so downstream agents apply the fix rather than re-litigating intent. Forward this requirement to the Explore agent when pre-analysis is delegated.

**Agent result tags (Step 4)** — exactly one of three forms per finding: `applied R{n}: <summary>` (bytes written); `verified-clean R{n}: <audit note>` (code already matches, no bytes written — preserve the item's original `category` in the note); `skipped R{n}: <reason>` (cannot be safely applied — would break behaviour, unclear semantics, requires deliberate refactor, or needs user confirmation on a public-API or schema change).

**`verified-clean` category vs disposition (Step 5)** — the `verified-clean` *category* is reserved for items `/review` itself first flagged as already-clean. A `/review-apply` audit transition sets the `verified-clean` *status* and never reassigns `category`.

**Category-specific verification (Step 5a)** — add to the `commands:` list where they fit the verification agent's run-and-report contract:

- `security` — a vulnerability scanner if one is on PATH (absent → skip silently and note it); `npm audit` is advisory only, never a hard gate (known false-positive rate on dev-only transitives); grep the staged diff for `AKIA`, `-----BEGIN`, `password\s*=`; confirm input-validation findings gained test coverage (post-apply count ≥ pre-apply). Pre-existing audit findings unrelated to files touched in this run are informational, not blocking.
- `db` — migration dry-run when migrations were touched (use CLAUDE.md's documented command; absent → warn and proceed); reject unreviewed destructive `DROP` / `ALTER` without a down-path.
- `architecture` — the project's module / layer linter if configured (absent → skip silently). `dependency-check` is a security scanner, not an architecture linter.
- `quality` / `completeness` — build + relevant tests. For `completeness`, report pre-apply vs post-apply test counts in the summary's `### Verification` block.

**Final summary** — title `## Applied Review Fixes`; the `### Verified Clean` sub-section is live for this carrier.

**Constraints delta** — `architecture` and `quality` findings frequently tempt refactors: stay inside the finding's scope and emit `skipped R{n}: requires deliberate refactor, not a point-fix` rather than widening. Public-API or schema changes flagged by `architecture` or `db` findings need explicit user confirmation — `skipped R{n}: requires user confirmation on public API / schema change`.
