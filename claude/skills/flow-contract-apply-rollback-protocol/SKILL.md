---
name: flow-contract-apply-rollback-protocol
description: "Canonical apply-rollback-protocol contract for the apply-flow carriers (/optimise-apply, /review-apply) — defines the Step 5.5 rollback protocol that fires when Step 5 verification fails: the trigger conditions (build failure on a touched file, an out-of-scope test regression that reproduces on a narrow rerun — never a `flaky` or `timeout` outcome — and applied-claim-without-diff), the seven-step revert sequence (collect touched paths, classify them — scope-clamping declared untracked files — before stashing, stash with `-u` and record the stash commit SHA, restore tracked files, reverse ledger transitions back to `open` with `rollback_rationale`, append a `[[rollback_events]]` entry, surface a `### Rollback` callout), the interactive/non-interactive confirmation prompts, and the safety constraints (only this-run transitions, re-derive paths from git diff, never bypass the stash, never auto-retry). Consult before reverting any apply-flow batch or appending a rollback event to a review/optimise ledger."
---

## Step 5.5: Rollback protocol

### Triggers

Rollback fires when Step 5 verification returns `outcome: fail` AND any of:

1. **Build failure on a file this run touched** — compile error, type error, linker error on a path in the union of `git diff --name-only HEAD`, `--cached`, and `git ls-files --others --exclude-standard`.
2. **Test regression outside the finding-ledger scope** — a test file that isn't in any selected item's `file` field now fails, and the failure reproduces: the verification agent's `rerun:` line still exits non-zero, or, where no rerun ran, a re-dispatch narrowed to the block's `failed_ids` fails again. A `flaky` outcome is never a trigger — it is load or timing, not this run's edit. Neither is a `timeout` on its own; Step 5 re-dispatches it with a larger budget or split.
3. **Applied claim without matching diff** — an agent emitted an `applied <id>` tag but the diff-reconciliation in Step 5 found no matching entry; the agent forged the tag.

Only transitions from THIS run are eligible for rollback. Items resolved in previous runs are never touched.

### Sequence

1. **Collect touched paths**: union of `git diff --name-only HEAD`, `git diff --name-only --cached`, `git ls-files --others --exclude-standard`. Call this set `PATHS`.
2. **Classify paths before stashing**: `stash push -u` stashes AND deletes every untracked path it names, so the removal clamp is applied here, before anything is stashed. Run every `git` call in steps 2-4 except `check-ignore` as `git --literal-pathspecs …`, so a `*`, `?` or `[` in a path cannot match a file outside the run's edits (`check-ignore` rejects that flag; call it without).
   - **Directory entries** — a path ending in `/`, or one where `test -d <path>` succeeds — are left out of every pathspec below, since a directory pathspec captures files outside the run's own edits. Name each in the callout.
   - **Missing or ignored paths** — a path that does not exist on disk, or one git ignores (`git check-ignore -q -- <path>` exits 0) — are left out. Either kind makes `stash push` exit 1 with the stash already written but the tree left unreverted.
   - **Tracked paths** are the output of `git --literal-pathspecs ls-files -- <PATHS without directory entries>`, never a list read from agent text. Call this set `TRACKED`.
   - **Untracked paths** (in PATHS, absent from `TRACKED`) are eligible for the stash only when all of these hold: the cluster agent's output declared the path as a new file; it is a regular file (`test -f <path>` succeeds and `test -L <path>` fails); and it falls under at least one of the resolved flow's `context.toml.scope` glob patterns (or the flow-less run's selector-file list). The declaration test guards against subverted agent output; the **scope-glob clamp** bounds the rollback's blast radius by scope, not by agent-declared filenames. An untracked path failing any test stays on disk and is named in the callout with the test it failed.

   The stash pathspec `STASHED` is the `TRACKED` paths that remain after the exclusions plus the eligible untracked paths.
3. **Stash working-tree state**: when `STASHED` is non-empty, run `git --literal-pathspecs stash push -u -m "<apply-command>-rollback-<ISO timestamp>" -- <STASHED>`. Never run a bare `git stash push` or `git clean`. Immediately after a successful push, record the stash's commit SHA as `stash_ref` (`git rev-parse --verify -q stash@{0}`) — a positional `stash@{N}` shifts under any later stash, the SHA does not. `No local changes to save` means there is nothing to recover and no `stash_ref`. **Any non-zero exit halts the rollback**: surface `git stash list` and `git status --porcelain -- <PATHS>` to the user and run none of the later steps.
4. **Restore tracked files**: the stash has already returned the paths it named to `HEAD`; restore the rest of the tracked set with `git --literal-pathspecs restore -- <TRACKED>` when `TRACKED` is non-empty. There is no separate clean step: the eligible untracked paths left the tree with the stash, and each stays recoverable from it.
5. **Reverse ledger transitions**: construct a single `tomlctl items apply <ledger> --ops - --on-stale skip --no-stamp` payload that transitions each affected item back to `status = "open"` with `rollback_rationale = "<concise cause>"`. Each op carries `"expect": {"status": "<the status this run transitioned it to>"}`, so an item a human re-dispositioned (through glimpse) after this run's write keeps that disposition; always pass the flag with `expect`, since a tomlctl predating the precondition ignores `expect` alone. List each `skipped_stale` id in the callout as changed during the run and never retry it. Do NOT clear `resolved` or `resolution` — leave the prior transition evidence so the audit trail remains intact across reopens. Pass `--no-stamp` on this write and on step 6's append, so neither marks the ledger fresh before the run's final write.
6. **Append rollback event**: add one `[[rollback_events]]` entry at the ledger root per the Rollback event log sub-section in the `flow-contract-ledger-schema` skill. Include `timestamp` (ISO 8601 date-time), `command = "<apply-command>"`, `cause`, `items` (array of reverted IDs, without step 5's `skipped_stale` ids), and the `stash_ref` recorded in step 3 (omitted when that step saved nothing). Use `tomlctl array-append` to append without op-type JSON framing:

   ```bash
   tomlctl array-append <ledger> rollback_events --json - --no-stamp <<'EOF'
   {"timestamp":"2026-04-18T14:32:00Z","command":"<apply-command>","cause":"build failure on <file>:<line>","items":["<id1>","<id2>"],"stash_ref":"<stash commit SHA>"}
   EOF
   ```

   `array-append` is a `mutate_doc*`-routed verb, so this idiom works against a fresh (missing) ledger too — the first `rollback_events` append auto-creates the file with the `schema_version = 1` skeleton, no pre-initialisation needed. Stdin-heredoc is the primary form because `cause` is constructed from live verification output and will routinely contain shell metacharacters (backticks, `$`, embedded quotes, newlines from multi-line error text) that break argv-quoting. The argv form `tomlctl array-append <ledger> rollback_events --json '{...}'` is acceptable only when `cause` is a literal fixed string with no shell metacharacters. The `items apply --array <name> --ops -` form remains the power-tool for batched or mixed-op writes to non-default arrays.
7. **Surface a prominent `### Rollback` callout** in the final summary: list the reopened items, the items left alone as changed during the run, the cause, the untracked paths the stash removed, each untracked path left on disk with the test it failed, each directory entry left out, and the `stash_ref` SHA so the user can invoke `git stash show <sha>` or `git stash apply <sha>` to recover.

### Confirmation prompts

**Interactive mode**: after diagnosing the trigger, prompt:

```
Rollback protocol armed — <N> transitions reopen, <M> files revert.
  cause: <build fail | test regression | applied-without-diff>
  stash: will save <M> files; the callout names the stash by commit SHA
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
