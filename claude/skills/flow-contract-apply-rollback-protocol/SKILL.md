---
name: flow-contract-apply-rollback-protocol
description: "Canonical apply-rollback-protocol contract for the apply-flow carriers (/optimise-apply, /review-apply) — defines the Step 5.5 rollback protocol that fires when Step 5 verification fails: the trigger conditions (build failure on a touched file, out-of-scope test regression, applied-claim-without-diff), the seven-step revert sequence (collect touched paths, stash with `-u`, restore tracked files, scope-clamped `git clean` of declared untracked files, reverse ledger transitions back to `open` with `rollback_rationale`, append a `[[rollback_events]]` entry, surface a `### Rollback` callout), the interactive/non-interactive confirmation prompts, and the safety constraints (only this-run transitions, re-derive paths from git diff, never bypass the stash, never auto-retry). Consult before reverting any apply-flow batch or appending a rollback event to a review/optimise ledger."
---

## Step 5.5: Rollback protocol

### Triggers

Rollback fires when Step 5 verification fails AND any of:

1. **Build failure on a file this run touched** — compile error, type error, linker error on a path in the union of `git diff --name-only HEAD`, `--cached`, and `git ls-files --others --exclude-standard`.
2. **Test regression outside the finding-ledger scope** — a test file that isn't in any selected item's `file` field now fails (tests that weren't supposed to change but were).
3. **Applied claim without matching diff** — an agent emitted an `applied <id>` tag but the diff-reconciliation in Step 5 found no matching entry; the agent forged the tag.

Only transitions from THIS run are eligible for rollback. Items resolved in previous runs are never touched.

### Sequence

1. **Collect touched paths**: union of `git diff --name-only HEAD`, `git diff --name-only --cached`, `git ls-files --others --exclude-standard`. Call this set `PATHS`.
2. **Stash working-tree state**: `git stash push -u -m "<apply-command>-rollback-<ISO timestamp>" -- <PATHS>`. Note the stash ref for the `[[rollback_events]]` entry.
3. **Restore tracked files**: `git checkout -- <PATHS-that-were-already-tracked>`.
4. **Remove untracked agent-created files**: for each path in PATHS that is untracked AND was declared in its cluster agent's output as a new file, run `git clean -fd -- <path>` scoped to that single path. NEVER run bare `git clean`. Reject any path not declared by the cluster agent to guard against subverted agent output. **Scope-glob clamp** (additional defense): before invoking `git clean -fd -- <path>`, verify each declared path falls under at least one of the resolved flow's `context.toml.scope` glob patterns (or the flow-less run's selector-file list). Reject any path that falls outside scope — the rollback's blast radius is bounded by scope, not by agent-declared filenames.
5. **Reverse ledger transitions**: construct a single `tomlctl items apply --ops -` payload that transitions each affected item back to `status = "open"` with `rollback_rationale = "<concise cause>"`. Do NOT clear `resolved` or `resolution` — leave the prior transition evidence so the audit trail remains intact across reopens.
6. **Append rollback event**: add one `[[rollback_events]]` entry at the ledger root per the Rollback event log sub-section in the `flow-contract-ledger-schema` skill. Include `timestamp` (ISO 8601 date-time), `command = "<apply-command>"`, `cause`, `items` (array of reverted IDs), and the `stash_ref`. Use `tomlctl array-append` to append without op-type JSON framing:

   ```bash
   tomlctl array-append <ledger> rollback_events --json - <<'EOF'
   {"timestamp":"2026-04-18T14:32:00Z","command":"<apply-command>","cause":"build failure on <file>:<line>","items":["<id1>","<id2>"],"stash_ref":"stash@{0}"}
   EOF
   ```

   Stdin-heredoc is the primary form because `cause` is constructed from live verification output and will routinely contain shell metacharacters (backticks, `$`, embedded quotes, newlines from multi-line error text) that break argv-quoting. The argv form `tomlctl array-append <ledger> rollback_events --json '{...}'` is acceptable only when `cause` is a literal fixed string with no shell metacharacters. The `items apply --array <name> --ops -` form remains the power-tool for batched or mixed-op writes to non-default arrays.
7. **Surface a prominent `### Rollback` callout** in the final summary: list the reopened items, the cause, and the stash ref so the user can invoke `git stash show stash@{N}` or `git stash pop` to recover.

### Confirmation prompts

**Interactive mode**: after diagnosing the trigger, prompt:

```
Rollback protocol armed — <N> transitions reopen, <M> files revert.
  cause: <build fail | test regression | applied-without-diff>
  stash: will save <M> files to stash@{0}
Proceed?
  [p] proceed with rollback
  [s] skip (leave state as-is; failure surfaces to user)
  [a] abort this /<apply-command> run
```

**Non-interactive**: default to `[s] skip` and surface the failure without rolling back. The user reviews the failure and can invoke rollback manually.

### Safety constraints

- Never roll back items that reached their successful status (`fixed` for /review-apply, `applied` for /optimise-apply) in prior runs — only items this run transitioned.
- Never accept a path list from agent output directly; always re-derive from git diff evidence.
- Never bypass the stash — unstashed rollbacks lose user-in-progress work.
- Never follow a rollback with automatic retry — the user decides what to do next after reopening.
