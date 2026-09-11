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
`.claude/flows/<slug>/{review-ledger,optimise-findings,execution-record,plan-review-findings,tasks}.toml`
— read from `envelope.resolved.artifacts.*`, never recompute inline; persist back to
`context.toml` on next write when absent. `artifacts.tasks` is the fifth key and carries the
task-DAG store: it may be computed from the slug when a legacy `context.toml` lacks the key,
and `flow doctor` reports the missing key on its top-level `warnings` array rather than as a
failed check, so `envelope.doctor.ok` stays `true`; `flow doctor --fix` backfills the key.
Doctor's per-flow `tasks-exists` and `tasks-counters` checks are advisory the same way —
`tasks-exists` is gated on the plan declaring a `## Tasks` section and reports an absent store
as a warning, never a check failure, and `tasks-counters` backstops the `[tasks]` counter join
entirely on `warnings`, always reporting `ok = true` (with a `skipped:` detail for a flow
offering neither readable counters nor a populated store), so `envelope.doctor.ok` never flips
on a stale ratio; that check's own semantics are the execution-record schema's.
`tasks-sidecar` is `ok` with a `skipped:` detail when no store is on disk and fails only on a
digest mismatch against one that is. A carrier names `"tasks"` in
`require_artifacts` only when it needs the store to already exist — the bootstrap agent's
existence gate tests the file on disk, and flows minted before the store existed carry no
`tasks.toml`. **Completed-flow handling**: `status = "complete"`
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
