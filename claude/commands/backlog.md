---
description: Triage the repo-scoped backlog — cluster open captures, take dispositions per item or cluster, audit evidence hygiene
argument-hint: [no arguments — the store is repo-scoped, not flow-scoped]
---

# /backlog — sweep the repo-scoped capture log

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Walks `.claude/backlog.toml` — the repo-scoped store of tangential discoveries — from an open set to a decided one: cluster what has accumulated into candidate work scopes, take a disposition per cluster or per item from the user, audit the evidence drop-box, and optionally age decided items out. It is a triage pass over captures that other commands minted; it does not review code and never mints an item on its own initiative.

Invoke the `backlog-capture` skill for the capture discipline — the mint test, the `backlog check` gate and its verdict ladder, the `kind` and `status` vocabularies, the orchestrator-only writer rule, and the evidence-publication rules. That skill owns all of them and this carrier does not restate any. Consult it whenever the user dictates a new item mid-sweep, and mint through the same `check`-then-`add` gate every other carrier uses.

Invoke the `flow-contract-task-visibility` skill for the run-scoped task-surface contract (view-not-store rule, subject prefix with lowercase `<ref>`, `activeForm`, lifecycle, granularity floor, silent degradation). This command is repo-scoped rather than flow-aware, so the slug slot is the literal `no-flow` per that contract's no-flow rule. Mint one task each for Steps 1-4 — `no-flow /backlog · cluster — group the open set`, `· dispositions — apply the user's verdicts`, `· evidence-audit — check the drop-boxes`, `· compact — fold aged terminal rows` — as a set before Step 1 opens; Step 0 and Step 5 sit below the granularity floor. `TaskUpdate` each `→ in_progress` as its step opens and `→ completed` when it closes, naming a skipped step's reason in its `description` rather than leaving the row `pending`. When the task tools are absent, continue the sweep unchanged.

## Step 0: Pre-flight (binary, then open set)

There is **no flow envelope**. The store is repo-scoped and shared by every flow in the worktree, so this command resolves no flow, dispatches no `flow-bootstrap`, and takes no `--flow` argument.

Two gates, in order:

```bash
tomlctl --version
```

The `backlog` group landed in 0.6. Below that the verbs do not exist and every step here fails with an `InvalidSubcommand`; halt and tell the user to reinstall with `cargo install --path tomlctl`.

```bash
tomlctl backlog list --open --count
```

A missing store reads as zero rather than erroring, so a fresh clone reaches this gate cleanly. On a count of 0, say the backlog is empty and stop — nothing to triage, and no tasks are minted.

## Step 1: Cluster the open set

```bash
tomlctl backlog cluster --by all
```

Three views come back — `area`, `tags`, `relations` — each an array of groups carrying `key`, `reason`, `size`, `item_ids`, `kinds` and `areas`. Render each view as a compact table of key, size, ids and kinds. An empty view is one line saying so, not an empty table.

Singleton groups are dropped from every view, so an item can be open and appear in none of them. Derive that remainder yourself — the open ids minus the union of every view's `item_ids` — and list it under an **Unclustered** heading, one row per item, so nothing falls out of the sweep merely by being unrelated to anything else:

```bash
tomlctl backlog list --open --select id,kind,area,summary
```

## Step 2: Take dispositions

**This is a user-engagement gate — the autonomy directive does not apply.** Never infer a disposition and never apply one the user did not give. An item they did not rule on stays `open`, and that is a valid outcome for the whole sweep.

Offer, per cluster where the group holds together and per item otherwise, via `AskUserQuestion`: promote, dismiss, resolve, keep open, or relate to another item. Promote and relate need a follow-up value — the flow slug or repo-relative plan path for `--to`, the second id and the edge kind for a relation. Dismiss and resolve carry the user's own wording; do not draft a reason on their behalf.

Apply each decision as it is taken:

```bash
tomlctl backlog triage B-1a2b3c4d --promote --to tomlctl-backlog-capture
```

```bash
tomlctl backlog triage B-1a2b3c4d --dismiss --reason "the API it reports was removed"
```

```bash
tomlctl backlog triage B-1a2b3c4d --resolve --resolution "fixed in the spawn-path rewrite"
```

`triage` accepts several ids in one call, which is how a whole cluster moves at once. Relations are a separate verb, and `--as` takes `relates-to`, `duplicates` or `supersedes` — the latter two also dismiss an item, so confirm the user meant that transition before writing one:

```bash
tomlctl backlog relate B-1a2b3c4d --to B-5e6f7a8b --as relates-to
```

## Step 3: Audit the evidence drop-box

```bash
tomlctl backlog evidence audit
```

Report every finding grouped by class, and name the item each belongs to. Four classes are not defects and must be described as such:

- `tracked` — a file in the git-ignored drop-box that is nonetheless staged or committed, so it is **about to be published**. Surface it for review before the next commit; the `backlog-capture` skill's publication rules decide it.
- `nested` — a subdirectory inside a drop-box. Its contents stay ignored, but nothing sizes or classifies them, so report it as unexamined rather than as clean.
- `empty` — a directory with a marker and no files. The expected state in a fresh clone, because the contents never left the machine that captured them.
- `git-unavailable` — `git check-ignore` did not run, so no file could be classified as published. Report the gap; it says nothing about the tree.

The rest — `unowned`, `no-marker`, `oversize`, `disallowed-extension`, `referenced-missing`, `stray`, `sensitive-published` — are real findings, and `sensitive-published` leads: it is a published file whose format routinely carries an `Authorization` header or a session token. Run without `--strict` here: this step is a report, not a gate.

## Step 4: Compact (offer, do not assume)

Only after Step 2, and only if the user agrees. Preview first and show what would move:

```bash
tomlctl backlog compact --dry-run
```

```bash
tomlctl backlog compact --older-than 90d
```

Decided items age into `[[compacted]]` and stay readable there; `open` items are never touched at any age. When the dry run would move nothing, say so and skip the write.

## Step 5: Summary

Close with counts by status before and after the sweep, the ids that moved and where each went, the relations written, the audit classes seen with a count each, and whether compaction ran. Name the ids that stayed `open` too — part of what a sweep produces is the list nobody was ready to decide.

```bash
tomlctl backlog list --count-by status
```
