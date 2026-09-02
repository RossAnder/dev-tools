---
description: Drive RED/GREEN/REFACTOR cycles for a feature; composes /implement per cycle and enforces test-first via SHA256 fingerprint diff
argument-hint: [feature description or "resume"]
---

# /tdd — RED/GREEN/REFACTOR cycle driver

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Drives strict RED → GREEN → REFACTOR cycles for one feature inside an existing `/plan-new` flow. Each cycle writes one failing test, dispatches `/implement` to pass it, then optionally refactors with coverage gating. Anti-cheat is structural — no GREEN without a recorded RED `outcome=fail` verification, plus a SHA256 test-file fingerprint that must match between RED and GREEN.

## Step 0: Pre-flight (flow resolution + doctor)

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` / `envelope.doctor.*` binding, no-flow fallback, doctor-fail handling, staleness reconciliation, and the mandatory bootstrap-summary console line).

Build the input envelope and dispatch `flow-bootstrap`:

```bash
tomlctl flow envelope build \
  --command tdd \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)" \
  --require-artifact execution_record \
  --staleness-threshold 7d
```

The block above is complete and copy-pasteable as-is — do NOT look up `--help`. The `--require-artifact execution_record` flag is what pins `require_artifacts = ["execution_record"]` in the emitted envelope (`/tdd` reads the record before writing it); `--staleness-threshold 7d` is the default, passed explicitly for clarity. On detached HEAD, omit `--branch` so the envelope records `branch:null`. Dispatch `flow-bootstrap` via the Task tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt. Gate on `envelope.ok`; bind `slug`, `context_path`, `artifacts.*`, `doctor.ok`. For `/tdd`, no-flow resolution prompts the user (it does not fall back to a scope file). Emit the bootstrap-summary line before any other action.

# TDD Cycles

## Overview

Each cycle: (1) writes one failing test via the `test-author` skill, commits `red:`; (2) writes a one-task mini-plan and dispatches `/implement` to make it pass, commits `green:`; (3) optionally refactors with coverage gating. **Prerequisite**: the parent plan's `## Verification Commands` block MUST carry a working `test:` line (run `/test-bootstrap` first) — `/tdd` halts with `"No test framework detected. Run /test-bootstrap first."` otherwise and does NOT auto-bootstrap. Composition is via per-cycle mini-plans that `/implement` consumes unmodified.

## Cycle FSM

Invoke the `flow-contract-execution-record-schema` skill before the first execution-record write to load the canonical schema (field set, type vocabulary, the two-call heredoc append idiom, append-only + supersession, `--verify-integrity` read contract, field-length caps, and deterministic PROGRESS-LOG.md regeneration via `tomlctl flow render-progress-log`). `/tdd` writes `verification` and `task-completion` entries into cycle sub-flows and copies entries up to the parent record; all writes follow this contract.

Finite state machine RED → GREEN → REFACTOR → cycle decision (loop or stop), gated by recorded entries in the cycle sub-flow's `execution-record.toml`; states cannot be skipped or reordered.

Invoke the `flow-contract-task-visibility` skill for the run-scoped task-surface contract (view-not-store rule, subject prefix with lowercase `<ref>`, `activeForm`, lifecycle, granularity floor, silent degradation, and the resume rule). Mint one task per FSM state at the start of each cycle, subject-prefixed `<parent-slug> /tdd · c<NNN>-<red|green|refactor>` using the same zero-padded cycle counter the sub-flow slug carries, and `TaskUpdate` on each state transition — the cycle sub-flow's execution record remains the gate, and a task call must never influence the FSM. **On `/tdd resume` re-entering a halted cycle, mint nothing**: the contract forbids reading entries back, so the carrier cannot detect its own prior set and a second mint would duplicate every subject. Complete the stranded rows with the halt reason named instead.

**RED**: author ONE failing test (`test-author` skill), run the parent's `test:` command, require FAIL, append a `verification` entry with `outcome=fail`. Commit `red: <cycle-slug>`. Capture `red_test_fingerprint` **POST-COMMIT** from the `red:` commit's tree (NOT the working tree), excluding snapshot artifacts (`**/__snapshots__/**`, `*.snap*`, `**/snapshots/**`, `*.snapshot`, `.snap.new`). Fingerprint pipeline (single source of truth):

```
git ls-tree -r <red-commit> -- <test-glob> | sha256sum | awk '{print $1}'
```

Per-language test-globs: rust `tests/**/*.rs` + `src/**/*.rs:#[cfg(test)]`; python `tests/**/*.py` + `**/test_*.py`; ts `**/*.test.{ts,tsx}` + `__tests__/**`; go `**/*_test.go`. Persist `red_test_fingerprint` + `test_globs` into the cycle `context.toml`. **Anti-cheat rule 1** is structural: RED → GREEN requires the recorded `outcome=fail` verification entry; GREEN refuses to start without it.

**GREEN**: derive slugs — `<NNN>` = zero-padded 3-digit cycle counter (monotonic under the lockfile); `<short-name>` = first 4 words of the failing test, lowercased/hyphenated, ≤30 chars; collisions append `-2`, `-3`; cycle slug = `<parent-slug>-tdd-<NNN>` (flat, satisfies plan-new's `^[a-z0-9][a-z0-9-]{0,63}$`); sub-flow at `.claude/flows/<parent-slug>-tdd-<NNN>/`. Write a one-task mini-plan at `docs/plans/<parent-slug>/tdd/cycle-<NNN>-<short-name>.md` whose task `task_ref` is `tdd-cycle-<NNN>-<short-name>`, acceptance "`<test name>` passes", carrying the parent's `test:` line. Bootstrap the sub-flow execution-record (see `## Cycle sub-flow layout`). Dispatch `/implement --flow <parent-slug>-tdd-<NNN>`. On return, recompute the fingerprint POST-COMMIT from the GREEN HEAD and require **strict equality** with RED's value.

**Mismatch handling (do all three before halting)**: (1) revert the GREEN commit — `git revert --no-edit <green-sha>` by default; `git reset --hard HEAD~1` only on single-developer linear history with no push since GREEN; skip if `/implement` exited pre-commit. (2) Append a NEW superseding `task-completion` entry marking the cycle failed (do NOT mutate the original `status=done` in place):

```
cat <<'EOF' | tomlctl items add <cycle-record> --json -
{"id":"<E{n+1}>","type":"task-completion","date":"<today>","agent":"tdd","task_ref":"tdd-cycle-<NNN>-<short-name>","summary":"GREEN fingerprint mismatch — reverted","files":[],"status":"failed","failure_reason":"fingerprint-mismatch","red_fingerprint":"<sha256-from-RED>","green_fingerprint":"<sha256-recomputed-at-GREEN>","supersedes_entry":"<E-id-of-original-done-entry>"}
EOF
tomlctl set <cycle-record> last_updated <today>
```

`failure_reason` is an optional discriminator (the schema's required set does not proscribe extra keys); the resume FSM pivots on `status=failed` AND `failure_reason="fingerprint-mismatch"` together, and both fingerprint hashes are retained for audit. (3) Halt without REFACTOR; do NOT advance the cycle counter; do NOT copy up to the parent (REFACTOR does that). Surface a diagnostic naming offending files (diff `git ls-tree -r <green-sha> -- <test-glob>` vs the RED tree). Resume contract: a `status=failed` + `failure_reason="fingerprint-mismatch"` entry is a **re-RED trigger for the SAME cycle-NNN** (not a fresh NNN+1); counter does not advance until the re-RED yields a clean GREEN. Commit `green: <cycle-slug>` only once the test passes AND the fingerprint matches. **Anti-cheat rule 2** (no test mutation) is the fingerprint diff.

**REFACTOR**: run the coverage tool; if changed-line coverage <90%, append a follow-up parent task and re-enter GREEN as the next cycle (a follow-up outside the feature's plan scope goes to the backlog instead — see **Deferred follow-ups**). Otherwise optionally do a production-only refactor (same fingerprint check; revert on regression). Append a `task-completion` to the **parent** record with `task_ref` prefixed `tdd-cycle-<NNN>-<original-slug>` and a re-minted `id` (`tomlctl items next-id <parent-record> --prefix E`). Copy up the cycle's `verification` entries with the same prefixing + re-mint.

**Cycle decision**: loop to RED for `<NNN+1>` if uncovered behaviour remains OR coverage gating appended a follow-up; stop when all feature behaviour is covered AND changed-line coverage ≥90% AND all tests pass. On stop, summarise cycles run, total commits, final coverage, and deferred follow-ups.

**Deferred follow-ups.** A follow-up inside the feature's plan scope keeps its existing path — it appends a parent task, unchanged. One that falls outside that scope goes to the repo backlog: invoke the `backlog-capture` skill, then run its check-then-add gate on each such follow-up, minting with `--origin tdd --flow <parent-slug>`. The stop summary lists the minted or bumped ids with their verdicts beside the deferred follow-ups (`(none)` when empty).

## Cycle sub-flow layout

Each cycle gets a transient flow at `.claude/flows/<parent-slug>-tdd-<NNN>/` with its own `context.toml` (cycle slug, parent reference, persisted `red_test_fingerprint` + `test_globs`) and one-task `execution-record.toml`. **Bootstrap protocol** (idempotent): no explicit pre-create is needed — the first mutating `tomlctl items add` against the cycle `execution-record.toml` auto-creates and seeds it with the byte-identical `schema_version = 1` + `last_updated = <today>` skeleton and materialises its `.sha256` sidecar in the same transaction (the write success envelope carries `"created": true`). At completion, copy `task-completion` + `verification` entries up to the parent record (prefix + ID re-mint per REFACTOR), keeping the parent as audit source-of-truth and cycle noise isolated, then regenerate the parent's progress log:

```bash
tomlctl flow render-progress-log --slug <parent-slug>
```

## Anti-cheat enforcement

Two structural rules, both FSM-enforced (not agent honesty), neither bypassable by a flag. **Rule 1 (no implementation before failing test)**: RED → GREEN requires a recorded `verification` entry with `outcome=fail` in the cycle execution-record; GREEN refuses to start without it. **Rule 2 (no test mutation between RED and GREEN)**: SHA256 fingerprint captured POST-COMMIT from the `red:` tree, recomputed POST-COMMIT from the GREEN HEAD; strict equality required; mismatch → revert GREEN + halt with a diagnostic naming changed test files. Test refactors are a separate cycle outside the RED/GREEN/REFACTOR loop — not in `/tdd`'s scope.

## Bootstrap-missing fallback

At startup, before RED, detect a usable test framework: (1) Re-resolve the parent via `tomlctl flow resolve --flow <parent-slug> --json` (`<parent-slug>` = Step 0's `envelope.resolved.slug`). The `--flow` flag forces the explicit-flag resolver path (deterministic single-flow lookup, not the 6-step algorithm) and deliberately bypasses `flow-bootstrap`'s `tomlctl --version` and `tomlctl flow doctor` gates: Step 0 already cleared both for this parent at entry, and this is a re-read of an already-validated projection (`plan_path`), not a fresh resolution. Trade-off: a between-Step-0-and-now tomlctl upgrade or invariant break won't be caught here, but the window is small and fails loudly on the next schema-mismatched read. Parse stdout as `parent_envelope`; extract `plan_path = parent_envelope.resolved.plan_path`; if `parent_envelope.resolved == false`, halt with `parent flow context.toml missing — re-run the parent command first`. (2) Re-parse the plan markdown's `## Verification Commands` block (`context.toml` does NOT carry verification commands; `/implement` extracts them transiently, so `/tdd` must re-parse). (3) Extract the `test:` line; if absent/empty, halt with the literal `No test framework detected. Run /test-bootstrap first.` (4) Do NOT auto-bootstrap (single-responsibility) — the user runs `/test-bootstrap`, then re-runs `/tdd`.

## Concurrency: per-parent-flow lockfile

`/tdd` MUST acquire `.claude/flows/<parent-slug>/.tdd.lock` before incrementing the cycle counter (mirrors the tomlctl + `/implement` lockfile convention). It prevents two concurrent `/tdd` sessions racing on cycle-NNN allocation (both picking `002` and clobbering mini-plan paths) and interleaving RED/GREEN entries during the parent copy-up (scrambling task_ref prefixes). On contention, halt with the literal `another /tdd session active in this flow`. Released on exit (clean or abort); a stale lockfile (crashed process) can be removed manually with `rm .claude/flows/<parent-slug>/.tdd.lock` after confirming no other session runs.

## Edge-case handling

**Cycle >5 min**: warn, do NOT auto-split (too-large behaviour-step; user re-scopes). **`/implement` retry-budget exhausted**: surface three choices — revise (edit mini-plan + re-dispatch), abort (revert + halt session), retry (re-dispatch same mini-plan). **User abort mid-cycle**: sub-flow stays on disk; recover via `/tdd resume`. **Idempotency-on-resume**: the deterministic `task_ref` `tdd-cycle-<NNN>-<short-name>` lets `/implement`'s Phase 2 skip-list no-op a completed cycle (latest post-supersession `task-completion` is `status=done`); a superseded `status=failed` does NOT satisfy the skip-list, so a re-RED → GREEN runs end-to-end against the same `task_ref`. **Coverage tool absent**: if no `coverage:` line and `test:` rejects `--coverage`, REFACTOR's gate downgrades to a warning, not a halt. **Verification stdout privacy**: `verification` entries are stored verbatim (no auto-redaction) and test runners can leak env secrets — redact via a setup hook before tests, or use `--no-stdout-capture` to record only outcome + exit code; cycle sub-flow dirs carry the parent's retention/scrubbing sensitivity.

## Resume protocol (`/tdd resume`)

Resumes the most recent uncompleted cycle in the resolved parent flow: (1) bind the parent `slug` from Step 0's `envelope.resolved.slug`; (2) list `.claude/flows/<parent-slug>-tdd-*/` sorted by `<NNN>` descending; (3) for each, read the cycle execution-record and inspect the **latest** (highest-id, post-supersession) `task-completion` for the deterministic `task_ref`; (4) the first directory whose latest `task-completion` is `status` ∈ {absent, `failed`} — or has none — is the resume target (`status=done` dirs are complete and skipped).

5. Dispatch into the correct FSM branch by recorded state: **no `verification` yet** → RED; **`verification` `outcome=fail` but no `task-completion`** → GREEN (re-dispatch `/implement` against the existing mini-plan; deterministic `task_ref` makes it idempotent); **latest `task-completion` `status=failed` AND `failure_reason="fingerprint-mismatch"`** → re-RED for the SAME cycle-NNN — discard the failed GREEN attempt (supersession records it), capture a fresh fingerprint against a freshly-authored RED test, continue through GREEN, counter does NOT advance until a clean GREEN (a superseded `status=failed` does NOT satisfy `/implement`'s skip-list, so the re-dispatch runs the work); **latest `task-completion` `status=failed` with any other/absent `failure_reason`** → surface the three choices (revise / abort / retry) per `## Edge-case handling`; **latest `task-completion` `status=done` but no REFACTOR copy-up to parent** → resume from REFACTOR.
6. If every cycle is complete, halt with `"no uncompleted /tdd cycle to resume"` and prompt to start a new cycle.

`/tdd resume` MUST acquire the same per-parent-flow lockfile before any state read.

## Acceptance smoke-check

`/tdd`'s GREEN dispatch relies on `/implement <plan-path> --flow <slug>` resolving correctly. Any change to `/implement`'s argument parser MUST preserve `--flow <slug>` recognition, or GREEN fails to land in the correct cycle sub-flow. Verify manually: create a throwaway flow at `.claude/flows/tdd-smoke/`, write a one-task plan at `/tmp/tdd-smoke-plan.md`, invoke `/implement /tmp/tdd-smoke-plan.md --flow tdd-smoke`, and confirm `/implement` writes its `task-completion` into `.claude/flows/tdd-smoke/execution-record.toml` (not auto-resolved via scope glob or branch).
