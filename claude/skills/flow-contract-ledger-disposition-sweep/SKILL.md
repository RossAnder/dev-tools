---
name: flow-contract-ledger-disposition-sweep
description: Ledger disposition sweep procedure for /review and /optimise — read-only orphan surfacing (file orphans, symbol orphans), duplicate-finding detection, and the disposition-sweep workflow that surfaces stale or no-longer-applicable items without auto-transitioning them. Defines how the sweep walks open `[[items]]`, batches Glob/Grep lookups for efficiency, and reports findings to the console for user disposition. Consult during the disposition-sweep phase of /review or /optimise, or any time a ledger needs orphan/duplicate triage.
---

### Orphan surfacing (read-only)

After the ledger loads and before the dispatch section, walk every `[[items]]` entry in the resolved ledger whose `status == "open"` and report orphans to the console without auto-transitioning:

- **File orphan**: the item's `file` path no longer exists. Detect via a single `Glob` call per unique path, or — for small ledgers — a batched `Test-Path` / `[ -e <path> ]` check.
- **Symbol orphan**: the item has a non-empty `symbol` field and a `Grep` for that symbol (name-only, not exact-match) against the current file tree returns no results. Use one `Grep` call with `output_mode: "files_with_matches"` over the repo to avoid per-item lookups.

For each orphan, emit a one-line console note in Step 3's report:

```
orphan R7 — file `src/old-module.rs` no longer present (check for rename; run the active flow command if the work has moved)
orphan R12 — symbol `foo_bar` not found anywhere in the repo (likely renamed; re-run the active flow command at the new location)
```

Orphans surface, they do NOT auto-transition. The ledger ID is preserved — symbol renames and file moves do not invalidate disposition history. Prefer `tomlctl items orphans <ledger>` over a hand-rolled Glob/Grep walk — the subcommand emits a JSON array of `{id, class, file, symbol?, dangling_deps?}` records (classes: `missing-file`, `symbol-missing`, `dangling-dep`) in one call, keeping the orchestrator's Read budget free for Step 2. Render the returned records as console one-liners per the format above.

### Deferred-item reopen sweep

After orphan surfacing and before the dispatch section, walk every `[[items]]` entry with `status = "deferred"` and check whether each item's `defer_trigger` has fired. Known trigger forms (literal substring match on `defer_trigger`):

- `after <path> exists` → test `[ -e <path> ]` (or `Test-Path <path>` on Windows).
- `after <file>:<symbol> landed` → test `<file>` exists AND `grep -qF "<symbol>" <file>` finds a match.
- `when <id> resolves` → look up `<id>` in the same ledger; fires when its `status` is any of `fixed`, `applied`, `verified-clean`, `wontfix`, or `wontapply`.
- `after <branch> merges` → test `git merge-base --is-ancestor <branch> HEAD`.
- `after <YYYY-MM-DD>` → fires when today's ISO date is ≥ the embedded date.
- Any other free-text trigger → surface to the console as a reminder; do not attempt automated detection.

For each fired trigger, prompt the user with the item's `id`, `summary`, and the matched trigger text:

```
deferred <id> — trigger fired: <matched trigger>
  summary: <summary>
Reopen?
  [y] reopen (status → open, reopen_rationale recorded)
  [n] skip (leave deferred)
  [a] abort sweep (do not inspect further candidates)
```

On `[y]`, queue the transition for a single atomic `tomlctl items apply --ops -` at the end of the sweep: set `status = "open"`, preserve `defer_reason` (audit trail), drop `defer_trigger`, set `reopen_rationale = "trigger fired: <matched trigger text>"`. Never auto-transition silently — every reopen passes through the prompt.

Non-interactive invocations surface candidates only (`found N deferred items with fired triggers; re-run interactively to reopen`) and do not mutate the ledger.
