# Review and Approve

The last two steps. Review settles how plan review was handled, which `plan stage` needs before it reads `ready`. Approve shows the gate, takes the user's answer, commits the plan, stamps the approval and commits what the stamp wrote.

Stage prose for a `--set-file` flag with the Write tool in the session's scratchpad directory, never under `.claude/`, where a Write is a protected-path write, and never under `docs/flows/<slug>/`, which is committed.

## Review

Plan review is not built into `/plan-v2` yet, so there is no route to `plan approve --review ran`.

**`--review`.** Refuse it when the arguments are read, before Open, so no work is spent on a plan that cannot be reviewed. Print that plan review is not built into `/plan-v2` yet, that the plan can be approved with the review skipped, and that `--review` should be dropped to do that. Stop.

**Otherwise.** Record the skip, with one fixed reason chosen by the flag:

| Flag | Reason |
|---|---|
| none | `plan review is not built yet` |
| `--no-review` | `the user passed --no-review` |

```bash
tomlctl plan run skip --slug <slug> --step review --reason 'plan review is not built yet'
```

Keep the reason: Approve passes the same text to `plan approve --reason`, and the gate shows it under `risks` as `kind = "skip-reason"`. Record it when `plan stage` reads `review`, the stage that applies exactly while review is unsettled. At `ready` the skip is already recorded, and a second one would show twice under `risks`.

## Approve

### 1. Confirm the stage

```bash
tomlctl plan stage --slug <slug>
```

Go on only when `stage` reads `ready`. Otherwise follow its `next` and come back; a blocker here is cheaper than a refusal after the plan is committed. An open run reads as a `relaunch step <s> or plan run abandon --step <s> --reason <why>` hint at any stage, and is settled by the open-run rule in `SKILL.md`.

### 2. Scan for secrets

Notes, the brief and the design files are committed to a public repo. Before the gate, scan the plan's half of the flow:

```bash
grep -rnE 'BEGIN [A-Z ]*PRIVATE KEY|ghp_[A-Za-z0-9]{20}|github_pat_|sk-ant-|AKIA[0-9A-Z]{16}|C:[\\/]Users[\\/]' docs/flows/<slug>/
```

`grep` exits 1 and prints nothing when the scan is clean. Keep every hit for the gate. Do not redact on your own: the user decides at the gate whether a hit is a secret, a home path worth removing, or harmless.

### 3. Show the gate

```bash
tomlctl plan gate --slug <slug>
```

Show one screen, the arrays in the object's key order, because that order puts the things that need thought before the things that reassure:

1. `assumptions`: each `agent-assumed` decision with `why`, `binds`, `door`, `if_false` and `revisit_if`, in the order given, which ranks them by how much they bind.
2. `one_way_doors`: each decision with its `provenance` and `revisit_if`.
3. `risks`: the review skip reason.
4. `task_mix`: tasks by file-count bucket and `executor`.
5. `review_edits`: always empty while plan review is not built; say so in one line.
6. `deferred_milestones`: each with its `reason`.
7. `stops`: each milestone gate command and undone handover.

Then the secret-scan hits, each as `file:line` with the matched text, or `secret scan: clean`. Totals come last: the counts of tasks, milestones, assumptions and one-way doors. An empty array is shown as `none` under its heading, not dropped, so the user can tell an empty section from a missing one.

Ask with `AskUserQuestion`: approve, or say what to change. Only "approve" goes on. Any other answer, a secret-scan hit to remove included, routes back into the plan: the step that owns the change (Surface, Design, Write or Prove) makes it, Prove re-runs as far as the change unprobes, and this step starts again from step 1.

### 4. Commit the plan

`plan approve` refuses while `git status` reports anything for `tasks.toml`, `plan.toml`, `brief.md` or `design/`, because the stamp names `HEAD` and that commit must hold the plan. Commit first.

Before staging, check the files the second commit will carry:

```bash
git status --short -- .claude/active-flow.toml .claude/backlog.toml
```

Either one already modified means the second commit would sweep in edits this plan did not make. Name them to the user and ask whether to go on before committing anything.

List what the first commit holds:

```bash
git status --porcelain --untracked-files=all -- docs/flows/<slug>/ .claude/flows/<slug>/
```

Then check that no path you are about to stage is ignored:

```bash
git check-ignore <every path from the list>
```

Notes are tracked by default, so the commit holds every path under `docs/flows/<slug>/` that `git status` lists, `notes/` included, and every path under `.claude/flows/<slug>/`, except any path `git check-ignore` prints. Stage each by name, never `git add -A` or `git add .`, and never `git add -f`. Invoke the `commit-conventions` skill for the message and commit.

### 5. Stamp the approval

```bash
tomlctl plan approve --slug <slug> --review skipped-flag --reason 'plan review is not built yet'
```

Pass the review skip's reason verbatim. `--review skipped-flag` covers both rows of the Review table: with no route to `ran`, a run with no flag is skipped as surely as one with `--no-review`. `skipped-low-risk` claims a judgement this engine does not make.

Each precondition is a `kind=validation` refusal, and nothing is written until all pass:

| Refusal | What clears it |
|---|---|
| A run is open; the message names the open steps | The open-run rule in `SKILL.md`: relaunch the oldest open step, or `plan run abandon --step <s> --reason <why>`, then step 1 again |
| A surfaced decision is still `agent-assumed` | Ask it through `AskUserQuestion` and record the answer with `plan decide`. A surfaced decision is never answered on the user's behalf |
| `plan check` reports an error | The verb `plan stage` names for that class, then Prove from the change on. Step 1 should have caught this; a refusal here means something changed after it |
| `--review ran` has no finished plan-family run in `review-v2.toml` | Does not apply here: this engine never passes `ran` |
| The flow's `status` is neither `draft` nor `in-progress` | An approved plan is refused without `--amend`, which is for a user-agreed change to intent (below). Any other status is not this engine's to change; stop and name it |
| `git status` reports `tasks.toml`, `plan.toml`, `brief.md` or a `design/` file, ignored and untracked files included | Commit it as in step 4. An ignored design file cannot be staged without `-f`: ask the user whether it belongs in the plan, and move it out of `design/` when it does not |

A flow with no `plan.toml` is `kind=not_found`, naming `tomlctl plan open --slug <slug>`: check the slug's spelling with `flow list`.

On success it prints `{"ok":true,"commit":"<sha>","transition":{"from":"draft","to":"in-progress"},"promoted":[...],"skipped_stale":[...]}`. Show `promoted` and `skipped_stale` to the user; a stale id is a backlog item that moved on before the plan closed it.

**Amend.** When a plan already approved has a changed brief, `plan stage` names `plan approve --amend --review <outcome>`. Amend only after the user agreed to the change to intent, by the same gate and commit as above, with `--amend` added and the same `--review` and `--reason`. It leaves `status`, the binding and the registry alone and reports no status transition, so step 6 records nothing.

### 6. Record the transition

`plan approve` reports the transition and writes no execution-record entry. When `transition.from` differs from `transition.to`, stage a one-line summary, such as `plan approved at <commit>`, with the Write tool and record it:

```bash
tomlctl flow record --slug <slug> --type status-transition --set agent=plan-v2 --set from_status=draft --set to_status=in-progress --set-file summary=<staged-summary>
```

Take `from_status` and `to_status` from `transition`, not from this example. When they are equal, record nothing. Then regenerate the rendered log, which is derived from the record:

```bash
tomlctl flow render-progress-log --slug <slug>
```

### 7. Commit what the stamp wrote

The stamp's writes land after the commit it names, so they need a second commit: the approval row in `plan.toml`, `audit = true` in `tasks.toml`, `context.toml`'s new status, binding and `updated`, the registry entry, any promoted backlog items, and the record from step 6. Stage, by name:

- every path `git status --porcelain --untracked-files=all -- .claude/flows/<slug>/` lists;
- `.claude/active-flow.toml` and `.claude/active-flow.toml.sha256`;
- `.claude/backlog.toml` and `.claude/backlog.toml.sha256`, when any task row had `closes`.

Invoke the `commit-conventions` skill for the message and commit.

### 8. Warn

Print this, with the slug filled in:

> Bare v1 carriers will now pick this flow, `/review` and `/optimise` included, and none can run it: `tasks import-plan --slug` refuses it, `/implement` has no named halt for that, and `/review-plan` falls back to plan mode and can overwrite `brief.md`. No engine can run it yet. Pass `--flow` to v1 carriers, or clear it with `tomlctl flow active remove --slug <slug>`.

Then end with the approved commit, the second commit, the stage `plan stage` now reports, and any `promoted` or `skipped_stale` backlog ids.
