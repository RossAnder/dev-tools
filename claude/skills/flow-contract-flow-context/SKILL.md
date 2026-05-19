---
name: flow-contract-flow-context
description: Flow resolution + doctor contract for flow-bootstrap envelopes — defines how a carrier's Step-0 builds the input envelope, gates on `envelope.ok`, and binds `envelope.resolved.{slug, context_path, artifacts.*, status, plan_path, scope, stale}` plus `envelope.doctor.ok` for downstream phases. Covers the no-flow fallback, doctor-fail handling, staleness reconciliation, status vocabulary, slug derivation, canonical artifact paths, and the mandatory bootstrap-summary console line format. Consult when any flow-carrying command (review, optimise, plan-new, plan-update, implement, tdd, review-plan, review-apply, optimise-apply) dispatches flow-bootstrap and needs to interpret the returned envelope correctly.
---

## Flow Context

Flow resolution + doctor checks are delegated to the `flow-bootstrap` sub-agent
(`claude/agents/flow-bootstrap.md`). Each carrier's Step-0 builds a JSON input envelope,
dispatches the agent, gates on `envelope.ok`, and binds `envelope.resolved.{slug,
context_path, artifacts.*, status, plan_path, scope, stale}` plus `envelope.doctor.ok` for
downstream phases. Canonical input/output envelope shapes: see `flow-bootstrap.md` Contract
section (mirrored at `scripts/templates/flow-context.md` Section 3).

All `.claude/...` paths resolve to the project-local `.claude/` at the git top-level. No
fallback to `~/.claude/`. **Status vocabulary**: `status ∈ {draft, in-progress, review,
complete}`; auto-transitions to `complete` from non-`plan-update-complete` ops are
forbidden (route through `review`); unknown values fail-soft to `in-progress` on read.
**Slug derivation**: filename minus `.md` (multi-file plan: parent directory name); no
further slugification. **Canonical artifacts**:
`.claude/flows/<slug>/{review-ledger,optimise-findings,execution-record,plan-review-findings}.toml`
— read from `envelope.resolved.artifacts.*`, never recompute inline; persist back to
`context.toml` on next write when absent. **Completed-flow handling**: `status = "complete"`
flows are filtered out of scope-glob + branch-match resolution but remain targetable via
explicit `--flow <slug>`. **Bootstrap-summary line**: after `flow-bootstrap` returns the
envelope, the carrier MUST emit one console line before any other action —
`flow resolved: <slug> (status=<s>, stale=<b>); doctor: <pass | fail: <N> issues | not-run: <reason>>`.
Substitute `no flow resolved (<source>);` for the flow clause when
`envelope.resolved.resolved == false`. Use the `not-run: <reason>` form when
`envelope.doctor == null` (tomlctl invocation failure, skipped on no-flow, etc.) — the
carrier proceeds without the doctor gate but the user sees the omission explicitly rather
than silently. **Legacy `.claude/active-flow` ignore**: the pre-overhaul
single-line slug file is no longer consulted; the registry lives at
`.claude/active-flow.toml` (multi-entry, gitignored per-clone state).
