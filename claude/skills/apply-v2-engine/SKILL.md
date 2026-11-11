---
name: apply-v2-engine
description: "Orchestrator contract for applying the findings a v2 review's triage selected — the argument rules, resolving the flow the fixes go to and refusing a v1 flow, the preview, opening and approving a new flow for a flow-less review, the settle milestone, the fix milestone and its gates, `review schedule`, the commits, the hand-over to `implement-v2-engine` and the close summary, with re-entry. Invoked by the /apply-v2 carrier after its pre-flight, before the first `tomlctl` call; not for direct use."
---

# Apply v2 engine

This is the orchestrator's half of applying triaged review findings. The `/apply-v2` carrier checks that the session can run it and then invokes this skill. This skill schedules; the `implement-v2-engine` skill runs. Scheduling turns each selected finding into a fix task in a v2 flow's task store, linked both ways: the task names its findings through `[[finding_links]]`, and the finding moves to `scheduled` with `apply_flow` and `apply_task`. From then on the findings ride the flow's own milestone: the execution engine gates and commits it, and a finding becomes `resolved` only when its task is committed.

`<store>` below is the review store's repo-relative path, and `<store flag>` the flag that names it to every `tomlctl review` verb: `--slug <slug>` for `.claude/flows/<slug>/review-v2.toml`, `--scope <scope>` for `.claude/review-v2/<scope>.toml`. `<flow>` is the flow that receives the tasks, its home, and `<id>` the fix milestone's id. The examples below spell the store flag as `--slug <slug>`; a flow-less store takes `--scope <scope>` in its place.

## Your role

- **Triage decided the selection.** Ask the user nothing about which findings to apply, whether to go on, or whether to approve a new flow. The one exception is the question about unfinished plan work that step 2 asks. Every other condition below that this skill cannot settle is a stop that names the condition, never a question. The execution engine asks its own questions once it runs.
- **You run no build, test or gate.** The fix milestone's gates are commands for the execution engine's `verification` agent; you only choose them.
- **You dispatch no agent.** Workers and the verifier are the execution engine's, launched as its own steps say.
- **Every store write goes through a `tomlctl` verb.** `plan.toml`, `tasks.toml` and the review store are never written with Write or Edit. The one file this skill authors by hand is a new flow's `docs/flows/<flow>/brief.md`.

## Rules

1. **Invoke the `tomlctl` skill before the first `tomlctl` call.** Its guidance canon holds throughout: payloads are files staged with the Write tool in the session's scratchpad directory, never heredocs, never piped, and `tomlctl` output is never piped into a text tool or redirected with `2>/dev/null`: `--select`, `--get`, `--limit` and `--max-chars` trim it at the source, where `head` or `cut` would also hide the notice that says rows were dropped.
2. **Nothing is opened, added or scheduled before the preview passes.** A milestone once added cannot be removed, and a new flow once opened keeps its directories.
3. **A home whose plan is approved takes every plan write with a `--reason`**, since its task store is audited. The reason is `apply <store>: <finding ids>`, the form `review schedule` writes on each row it mints.
4. **Writes under `docs/flows/**` prompt in attended `default` mode and are denied under `dontAsk`.** The allow rule that pre-approves them is in the `plan-v2-engine` skill's [open-and-challenge.md](../plan-v2-engine/references/open-and-challenge.md), section "Writing under `docs/flows/`". Name it when a prompt or a denial interrupts the brief; add it to no settings file yourself.
5. **Commits stage each path by name**, drafted through the `commit-conventions` skill. Never `git add -A`, `git add .`, `git add -f` or `commit -a`, and never `--no-verify`. Never stage an edit this skill did not make.

## Arguments

Read the arguments before anything else:

- `<slug>` or `--scope <scope>`: exactly one. With neither, or both, stop and say `/apply-v2` needs one store.
- `<finding ids>` or `--all`: exactly one. With neither, stop and say triage names the selection, as `/apply-v2 <store> F1,F4` or `/apply-v2 <store> --all`; never choose the findings yourself.

Anything else is not an argument: say so and stop, rather than guess what it was meant to change.

## Steps

| Step | Skipped when |
|---|---|
| 1. Resolve the home | Never |
| 2. Preview | Never |
| 3. Resolve the gates | Re-entry found the fix milestone already added |
| 4. Open the new flow | The home is the reviewed flow, or re-entry found the new flow already opened |
| 5. Add the fix milestone, and the settle milestone before it | Re-entry found it already added |
| 6. Schedule | Nothing selected is still `open` |
| 7. Commit the new flow | The home is the reviewed flow, or the new flow is already approved |
| 8. Approve the new flow | The home is the reviewed flow, or the new flow is already approved |
| 9. Run | Never |
| 10. Close | Never |

### 1. Resolve the home

For `--scope <scope>`, the home is a new flow; step 4 names it. For `<slug>`:

```bash
tomlctl plan stage --slug <slug>
```

| `plan stage` | Home |
|---|---|
| `approved`, `running` or `done` | `<slug>` itself. A `done` plan moves back to `running` once the fix milestone lands |
| any other stage | none: stop, saying the plan of `<slug>` must be approved through `/plan-v2 <slug>` before findings can be applied to it |
| refused, naming no `plan.toml` | Run `tomlctl flow list --select slug --lines`. When `<slug>` is listed, it is a v1 flow: stop, saying a review bound to a v1 flow has no apply path in `/apply-v2`, and that `/review-apply` or `/optimise-apply` applies it after `review export-v1`. When it is not listed, stop and say no flow `<slug>` exists |

`blockers` other than an open run stop the command with the entry quoted, as the execution engine's pre-flight would. An open run is the execution engine's to settle at step 9, so it does not stop this step.

### 2. Preview

Ask `review schedule` itself what it would do, writing nothing. On the reviewed flow, name it; for a flow-less review the home does not exist yet, so leave `--flow` out:

```bash
tomlctl review schedule <ids | --all> --slug <slug> --flow <slug> --dry-run
tomlctl review schedule <ids | --all> --scope <scope> --dry-run
```

It prints `{scheduled, settle, held, refused, dropped_deps}`. A `scheduled` or `settle` row with `action: "would-mint"` carries the task's `title`, `executor` and `files`; one with `action: "linked"` is already on a live task of the flow. Each `held` and `refused` row carries its reason. The rule it applies is the `tomlctl` skill's `references/review-apply.md`, section `tomlctl review schedule`; never restate or second-guess it here. An explicit id that is unknown is a `refused` row: stop and name it.

Then read the findings that pass, for the report and the gates:

```bash
tomlctl review show <passing ids> --slug <slug> --for implementer
```

- **Blocked on evidence.** Note each finding under `settle`: those make step 5 add a settle milestone. Such a finding edits nothing yet, so it takes no part in the dirty-files check below or in step 3's gates.

- **Already scheduled.** A finding refused with `status is scheduled, not open` already has `apply_flow` naming a flow and is past this skill: Re-entry, below, says what follows. With `--all` the verb skips non-open findings without a refused row, so find them from the home's links instead: for `<slug>` the home is `<slug>`, and for `--scope` it is the flow Re-entry's "A new flow already opened" finds by its brief (none when no such flow exists).

```bash
tomlctl items list .claude/flows/<flow>/tasks.toml --array finding_links --select ref,store,fixes,settles --lines
```

  Keep the rows whose `store` is `<store>`; their findings are the already-scheduled set, and `--all` reports no refused row for them.
- **Nothing to schedule.** When no selected finding passes the rule and the already-scheduled set is empty too, report each one with the reason it is held or refused and stop. No flow is opened and no milestone added.
- **Dirty files.** Collect the `files` of every `would-mint` row under `scheduled`. Then:

```bash
git status --porcelain -- <every edited path>
```

  Any path it prints holds edits a fix worker would build on and the milestone commit would sweep in. Stop and name each; the user commits, moves or discards them, never this skill.

- **Unfinished plan work.** On the reviewed flow, list the rows the plan has not finished:

```bash
tomlctl tasks list --slug <slug> --where-not status=done --select id,status,checkpoint --lines
```

  When it prints any row, the execution engine would run those milestones before the fix milestone, since it takes the first with a row that is not `done` and refuses `--milestone` past it. That is plan work nobody asked this command for. Ask with `AskUserQuestion`, naming the milestones and their row counts: run the plan's unfinished work first and then the fixes, or stop. On stop, nothing has been written; say that `/implement-v2 <slug>` finishes the plan and that this command can be rerun after it. A new flow has no earlier rows and skips this.

Report what will be scheduled, held and refused, each with its reason, and go on.

### 3. Resolve the gates

The fix milestone is gated, since a fix that passes no gate proves nothing. The edited paths from step 2 place the fix in crates or packages: the nearest enclosing directory with a manifest (`Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`) and the commands the `CLAUDE.md` files name for it, read from the repository root's `CLAUDE.md` and that directory's own `CLAUDE.md` when it has one. A path no manifest encloses takes the commands the root `CLAUDE.md` names for that path's tree, such as a test it says runs when such paths change.

- **On the reviewed flow**, start from the gates of the flow's latest milestone that has any:

```bash
tomlctl plan gate --slug <flow> --rows stops --lines
```

  The first line is the rest of the object; each later line is one `stops` row, `{kind, milestone, command}`, in store order. Take the commands of the last milestone those rows name. Add the build, test and lint commands of each crate or package the edited paths sit in that those commands do not already cover.
- **For a new flow**, the gates are the build, test and lint commands the `CLAUDE.md` files name for the crates and packages the edited paths sit in.

When no test command resolves, stop before step 4 and say so, naming the edited paths and the `CLAUDE.md` files read. Never write a gate the `CLAUDE.md` files do not name.

### 4. Open the new flow

Name it `<scope>-apply`, or `<scope>-apply-<n>` with the smallest `n` from 2 up when that is taken. A name is taken when this lists it:

```bash
tomlctl flow list --select slug --lines
```

Re-entry, below, says when an existing `<scope>-apply` flow is this skill's own unfinished one rather than a taken name. Open it:

```bash
tomlctl plan open --slug <flow>
```

`plan open` writes `docs/flows/<flow>/brief.md` with six empty sections. Fill the four `plan check` requires with the Edit tool, from the review run; read each run the selected findings came from with `tomlctl review run show <run> --scope <scope>`:

- **Intent**: apply the selected findings of `<store>`, one line per finding, `<id>: <summary>`.
- **Non-goals**: apply only; no change the selected findings do not name, and no finding outside the selection.
- **Scope budget**: one fix milestone over the edited paths from step 2, and the run's bounds (its base and head, its scope).
- **Success criteria**: each selected finding is `resolved` by the commit of the task that fixes it, after the milestone's gates pass.

Leave Background and Unknowns empty, or put the store path and run ids under Background. Write no `design/` file: the tasks carry their own specs.

### 5. Add the fix milestone

The milestone's id is `apply-<n>`, with the smallest `n` from 1 up for which no checkpoint of the home is `apply-<n>` or `settle-<n>`:

```bash
tomlctl items list .claude/flows/<flow>/tasks.toml --array checkpoints --select id --lines
```

Add it with one `--gate` per command from step 3, in the order step 3 resolved them. On a new flow, whose plan is a draft:

```bash
tomlctl plan milestone add --slug <flow> --id <id> --goal 'Fix <finding ids> from <store>' --gate '<command>'
```

On the reviewed flow, whose plan is approved, add `--reason 'apply <store>: <finding ids>'`.

**The settle milestone.** When step 2 noted a finding blocked on evidence, add a settle milestone first, so it runs before the fix milestone and the fix milestone is the checkpoint right after it. Its id is `settle-<n>`, with the same `n` as the fix milestone's `apply-<n>`, and it has no `--gate`: its rows write proposal notes, which no build or test proves.

```bash
tomlctl plan milestone add --slug <flow> --id settle-<n> --goal 'Settle <finding ids> from <store>'
tomlctl plan milestone add --slug <flow> --id apply-<n> --goal 'Fix <finding ids> from <store>' --gate '<command>'
```

The execution engine's settle drain judges each proposal when the settle milestone's rows are `done`, asks the user once about those only the user can accept, and schedules the accepted findings into `apply-<n>`, the checkpoint after `settle-<n>`. When every selected finding is blocked on evidence, `apply-<n>` starts with no rows and takes its tasks from that drain. Step 3 still resolves its gates, from the edited paths of the findings it fixes now, or when there are none, from the paths of the blocked findings' `file`, where their fixes are expected to land.

### 6. Schedule

```bash
tomlctl review schedule <ids> --slug <slug> --flow <flow> --checkpoint <id>
```

With a settle milestone, add `--settle-checkpoint settle-<n>`; without it the verb refuses every finding blocked on evidence:

```bash
tomlctl review schedule <ids> --slug <slug> --flow <flow> --checkpoint apply-<n> --settle-checkpoint settle-<n>
```

Pass `--all` in place of the ids when the arguments did. The verb mints the task rows with their specs and links under the task store's lock, then marks each finding `scheduled`. Report its `scheduled` and `settle` rows as finding, task and `action` (`minted` or `linked`), and its `held`, `refused`, `dropped_deps` and `skipped_stale` rows each with its reason. When it fails, report its error and stop; a rerun resumes by Re-entry.

When `scheduled` and `settle` are both empty, the milestones stay with no rows: report that, say a rerun with another selection reuses it, and stop. The execution engine passes over a milestone with no rows.

### 7. Commit the new flow

`plan approve` refuses while `git status` reports the flow's `tasks.toml`, `plan.toml` or `brief.md`, because its stamp names `HEAD`. The review store, which sits outside the flow, holds the `scheduled` marks those rows answer, so the two land in one commit. Commit both first.

Steps 7 and 8 follow the `plan-v2-engine` skill's [approve.md](../plan-v2-engine/references/approve.md), sections 4 to 7 (commit the plan, stamp the approval, record the transition, commit what the stamp wrote), with `<flow>` as the slug and these deltas only:

- **Dirty registry.** When `git status --short -- .claude/active-flow.toml .claude/active-flow.toml.sha256` prints a path, stop and name it; the user commits or discards those edits and reruns. Offer no options.
- **First commit's listing.** `<store>` and `<store>.sha256` join the paths section 4 lists, and go through the same `git check-ignore` check.
- **No gate screen.** Skip section 3. Run `tomlctl plan check --slug <flow>` and, on an error-class finding, report it and stop: the fix is to the verb or the brief, never to the store by hand. Then stamp with `tomlctl plan approve --slug <flow> --review skipped-flag --reason 'triage of <store> selected these findings'`. The triage is the approval.
- **The record** carries `agent=apply-v2`, and its summary reads `apply flow approved at <commit>`.

### 8. Approve the new flow

Covered by step 7's pointer: the plan check, the stamp, the record and the second commit. Section 8 of approve.md, the v1-carrier warning, is printed in step 10.

### 9. Run

Invoke the `implement-v2-engine` skill and follow it from its Arguments section with `<flow>` as the slug, and no `--milestone`. It picks the first milestone with a row that is not `done`: on the reviewed flow, a milestone of the plan still unfinished runs before the fix milestone, and the engine continues to the fix milestone while nothing is parked. The user agreed to that order at step 2 when the plan had unfinished rows.

The engine's report is the run's outcome; this skill adds nothing to it but the close.

### 10. Close

After the execution engine's report:

```bash
tomlctl review stats --slug <slug> --apply
```

Print it as the close summary: the findings scheduled, resolved and reopened per lens and kind, and the reproductions by verdict. A finding still `scheduled` is in flight, its task not yet committed; `/implement-v2 <flow>` carries it on.

When step 8 approved a new flow, print the v1-carrier warning that approve.md prints at its section 8: the block from its section 3, verbatim as a quoted block with `<flow>` filled in as the slug, never paraphrased or shortened.

## Re-entry

Every invocation reads where the apply stands from the stores, never from memory, so an interrupted run resumes by running `/apply-v2` again with the same arguments.

- **A new flow already opened.** At step 4, a `<scope>-apply` or `<scope>-apply-<n>` flow is this store's own when its `brief.md` Intent names `<store>` and its plan is not yet approved, or when a selected finding's `apply_flow` names it. Take it as the home instead of a taken name, and skip `plan open`. A flow of that name whose brief names another store is taken.
- **A fix milestone already added.** At step 5, an `apply-<n>` checkpoint of the home that holds no row is an earlier invocation's milestone that never got its tasks:

```bash
tomlctl tasks list --slug <flow> --where checkpoint=<id> --select id --lines
```

  Do not reuse an empty `apply-<n>` when a selected finding is blocked on evidence with no proposal and no `settle-<n>` sits beside it: step 5 adds `settle-<m>` and `apply-<m>` with a fresh `n`, so the settle milestone precedes its fix milestone. Otherwise reuse the last such checkpoint, skipping steps 3 and 5, unless a `settle-<n>` checkpoint of the same `n` holds a row: that `apply-<n>` is waiting for its settle drain, so it is not reused, and step 5 adds new milestones. A `settle-<n>` with no row beside an empty `apply-<n>` is reused with it, and passed to step 6 as `--settle-checkpoint` when a selected finding is blocked on evidence.
- **Findings already scheduled.** A selected finding that is `scheduled` into the home is not scheduled again; `review schedule` itself links a finding whose live task already holds it rather than minting a second. When every selected finding is already scheduled into the home, go to step 7 on a new flow that is not yet approved, and to step 9 otherwise. A finding `scheduled` into a different flow is reported with that flow and left alone.
- **An approval already stamped.** When the new flow's `plan stage` reads `approved`, `running` or `done`, steps 7 and 8 are done, unless `git status` still lists the approval's writes, which step 8's last commit then takes.

## Out of scope

- **Judging proposals.** This skill adds the settle milestone and schedules its settle tasks; the execution engine's settle drain reads their proposals, asks the user and schedules the accepted findings. A finding still blocked on a decision, a design, a measurement or alternatives is refused by `review schedule` and reported, never scheduled; a decision triage answered is no longer a block, and its answer reaches the fix task as a constraint.
- **Plan-family findings.** `tomlctl review apply` writes them to plan records; `review schedule` refuses them.
- **A v1 flow, its ledgers and `export-v1`.** A review bound to a v1 flow is refused at step 1.
- **Running, gating, committing and reconciling the fix milestone.** These are `implement-v2-engine`'s.
- **Closing the flow.** When every milestone is committed, the execution engine's report says the flow's status is still to be moved.
