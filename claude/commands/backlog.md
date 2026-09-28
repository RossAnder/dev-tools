---
description: Triage the repo-scoped backlog — cluster open captures, take dispositions per item or cluster, reconcile promotions, audit evidence hygiene
argument-hint: [no arguments — the store is repo-scoped, not flow-scoped]
---

# /backlog — sweep the repo-scoped capture log

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.
>
> **Portable to other harnesses.** Under Claude Code the skill names below are
> `Skill()` dispatches; under Codex or any harness without that tool, read each
> named `SKILL.md` at the path given beside it. The one Claude-only affordance,
> `AskUserQuestion`, degrades: put the choices to the user as ordinary prose.
> That changes nothing about what this command writes.

Walks `.claude/backlog.toml` — the repo-scoped store of tangential discoveries — from an open set to a decided one: cluster what has accumulated into candidate work scopes, take a disposition per cluster or per item from the user, settle the promotions flows have claimed, audit the evidence drop-box, and optionally age decided items out. It is a triage pass over captures that other commands minted; it does not review code and never mints an item on its own initiative.

Invoke the `backlog-capture` skill (`claude/skills/backlog-capture/SKILL.md`) for the capture discipline — the mint test, the `backlog check` gate and its verdict ladder, the `kind` and `status` vocabularies, the orchestrator-only writer rule, and the evidence-publication rules. That skill owns all of them and this carrier does not restate any. Consult it whenever the user dictates a new item mid-sweep, and mint through the same `check`-then-`add` gate every other carrier uses.

## Step 0: Pre-flight (binary, then live set)

There is **no flow envelope**. The store is repo-scoped and shared by every flow in the worktree, so this command resolves no flow, dispatches no `flow-bootstrap`, and takes no `--flow` argument.

Two gates, in order:

```bash
tomlctl --version
```

This command needs 0.10 or later: the `backlog` group landed in 0.6, and `backlog reconcile` and `backlog list --live` in 0.10. Below that the verbs and flags used here do not exist and fail with a clap usage error; halt and tell the user to reinstall with `cargo install --path tomlctl`.

```bash
tomlctl backlog list --live --count
```

A missing store reads as zero rather than erroring, so a fresh clone reaches this gate cleanly. On a count of 0, say the backlog holds nothing open or promoted and stop — nothing to triage.

A store whose live rows are all `promoted` still has work in Step 3, so take the open count too. When it is 0, skip Steps 1 and 2 and go straight to Step 3:

```bash
tomlctl backlog list --open --count
```

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

Offer, per cluster where the group holds together and per item otherwise, via `AskUserQuestion` (or plain prose on a harness without it): promote, dismiss, resolve, keep open, or relate to another item. Promote and relate need a follow-up value — the flow slug or repo-relative plan path for `--to`, the second id and the edge kind for a relation. Dismiss and resolve carry the user's own wording; do not draft a reason on their behalf.

Apply each decision as it is taken:

```bash
tomlctl backlog triage B-1a2b3c4d --promote --to <flow-slug> --error-format json
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

### When a promotion target is refused

`--promote` resolves `--to` before it writes, so a refusal leaves the store and its sidecar untouched and the decision can simply be retaken. Under `--error-format json` the refusal is one stderr line, `{"error":{"kind":…,"message":…,"arg":…}}`. Branch on `kind` and `arg`:

- **`not_found` with `arg` `to`** — the target is neither a flow nor a plan. (`not_found` with `arg` `ids` is a stale item id, not a target problem.) Ask the user which of three to do:
  - **Bootstrap a seed flow** — a draft flow that holds the claim until `/plan-new --backlog` plans it. See **Bootstrapping a seed flow** below.
  - **Pick an existing flow** — offer the rows of `tomlctl flow list`'s `flows` array at a status other than `review` or `complete`, and retry with the chosen slug.
  - **Promote as external** — the work is tracked outside this repo; retry with `--external`, which stores `external:<ref>` unresolved.
- **`validation` with `arg` `to`** — the `--to` flow has closed at `review` or `complete`, and unless one of its tasks already closes the item, Step 3 will report the claim `orphaned`. Ask: promote anyway with `--allow-closed`, or bootstrap a seed flow.

Leaving the item `open` stays a valid answer to either.

### Bootstrapping a seed flow

The `backlog-capture` skill's **Seed flows** subsection owns the recipe — the stub layout with its `<!-- backlog-seed -->` marker, and the command sequence. Bind its two inputs here:

- **`<slug>`** — propose the refused `--to` value when it matches `^[a-z0-9][a-z0-9-]{0,63}$`, otherwise ask; confirm it with the user either way. If `.claude/flows/<slug>/` or the stub path already exists, ask for another slug rather than overwrite anything.
- **`<plans_dir>`** — the settings value below. A string is the directory, and an array means its first entry other than `__DONT_ASK__`. The sentinel, an empty array, or an absent key (`kind=not_found`, exit 1) means the default `docs/plans/`.

```bash
tomlctl json get .claude/settings.json plansDirectory
```

Write the stub with the Write tool, titled by the user or the cluster key, listing each item being promoted by id and stored summary. Then run the skill's sequence in order: `flow init` with no `--scope` or `--branch`, `flow active remove`, and the promote to `<slug>`. If `flow init` fails, stop there and report it, naming the stub it left behind; do not promote. A later promotion to the same slug resolves as an ordinary draft flow.

## Step 3: Promotions

A `promoted` row is a live claim on a flow until the tasks that close it are done. This step finds the claims that have been delivered, the ones no task has picked up, and the ones that no longer lead anywhere. It is the same user-engagement gate as Step 2: offer, never infer, and a claim the user does not rule on stays as it is.

```bash
tomlctl backlog reconcile
```

Without `--adopt` or `--apply` it writes nothing. Render one table of bucket, count, and ids with their `target`, leaving out empty buckets; when every bucket is empty, say there are no promotions to reconcile and move on. Branch on the bucket, never on `reason`, which is prose — show it beside each row that needs a decision.

**`unlinked` — offer to adopt.** The flow is live but no task closes the row. `--adopt` links it to every task in its flow whose prose names its id, and reaches link-less `orphaned` rows too. The links land in the flow's `tasks.toml`, but the plan owns them — the next `tasks import-plan` rebuilds them from the plan's `- **Backlog**:` bullets — so an adopted link survives only once the plan is rendered. A render also rewrites the plan's rendered sections, and any drift someone left there would be overwritten. Check each target flow **before** adopting into it, because once adopt has linked something the new bullet is itself drift and `--check` can no longer tell the two apart:

```bash
tomlctl tasks render --slug <slug> --check
```

On exit 1 the flow already had drift: report it and stop for that flow — no adopt, no render; settling the drift is `/plan-update`'s job. On exit 0, adopt into that flow alone and render it when `render_needed` names it:

```bash
tomlctl backlog reconcile --flow <slug> --adopt
tomlctl tasks render --slug <slug>
```

Skip a flow with no `.claude/flows/<slug>/tasks.toml`: adopt never creates one. A row adopt could not link stays `unlinked`; list it, since a claim no task delivers yet is the flow's to plan. After any adoption, re-run the bare `reconcile` so the buckets below reflect it.

**`ready` — offer to resolve.** Every task that closes the row is done:

```bash
tomlctl backlog reconcile --apply
```

`--apply` resolves every ready row the run covers, recording its flow, tasks and commits; add `--flow <slug>` to resolve a single flow's. Report `applied`, and `skipped` with each reason.

**`orphaned`, `stalled`, `dangling` — reopen or re-promote.** An `orphaned` claim sits on a flow that closed without delivering it, a `stalled` one on a closing task that is `deferred` or `failed`, and a `dangling` one on a target that resolves to no flow. Offer, per row or per group sharing a target: reopen, re-promote, or keep. A reopen needs a rationale naming the flow, because it clears the claim — the user's wording, or a factual line such as ``flow `<slug>` closed without it``:

```bash
tomlctl backlog triage <ids> --reopen --rationale "<rationale naming the flow>"
```

A re-promote is Step 2's promote, refusal handling and seed bootstrap included. A reopened row is back in the open set; take its disposition now as in Step 2, or leave it for the next sweep.

**`in-progress` and `external` — list only.** Work is under way, or the claim points outside the repo; neither is this command's to move.

## Step 4: Audit the evidence drop-box

```bash
tomlctl backlog evidence audit
```

Report every finding grouped by class, and name the item each belongs to. Four classes are not defects and must be described as such:

- `tracked` — a file in the git-ignored drop-box that is nonetheless staged or committed, so it is **about to be published**. Surface it for review before the next commit; the `backlog-capture` skill's publication rules decide it.
- `nested` — a subdirectory inside a drop-box. Its contents stay ignored, but nothing sizes or classifies them, so report it as unexamined rather than as clean.
- `empty` — a directory with a marker and no files. The expected state in a fresh clone, because the contents never left the machine that captured them.
- `git-unavailable` — `git check-ignore` did not run, so no file could be classified as published. Report the gap; it says nothing about the tree.

The rest — `unowned`, `no-marker`, `oversize`, `disallowed-extension`, `referenced-missing`, `stray`, `sensitive-published` — are real findings, and `sensitive-published` leads: it is a published file whose format routinely carries an `Authorization` header or a session token. Run without `--strict` here: this step is a report, not a gate.

## Step 5: Compact (offer, do not assume)

Only after Steps 2 and 3, and only if the user agrees. Preview first and show what would move:

```bash
tomlctl backlog compact --dry-run
```

```bash
tomlctl backlog compact --older-than 90d
```

Decided items age into `[[compacted]]` and stay readable there. `open` and `promoted` items are never touched at any age — a promotion is a live claim until Step 3 resolves or reopens it. When the dry run would move nothing, say so and skip the write.

## Step 6: Summary

Close with counts by status before and after the sweep, the ids that moved and where each went, any seed flows bootstrapped, the relations written, the promotions resolved, adopted or reopened with the flows rendered or left drifted, the audit classes seen with a count each, and whether compaction ran. Name the ids that stayed `open` too — part of what a sweep produces is the list nobody was ready to decide.

```bash
tomlctl backlog list --count-by status
```
