# Review and Approve

The last two steps. Review runs plan review or records why it was skipped, which `plan stage` needs before it reads `ready`. Approve shows the gate, takes the user's answer, commits the plan, stamps the approval and commits what the stamp wrote.

Stage prose for a `--set-file` flag with the Write tool in the session's scratchpad directory, never under `.claude/`, where a Write is a protected-path write, and never under `docs/flows/<slug>/`, which is committed.

## Review

Settle review when `plan stage` reads `review`, the stage that applies exactly while review is unsettled. At `ready` it is already settled, and a second skip would show twice under `risks`.

### Skip

Review runs unless a row of this table applies. `--review` rules out the second row:

| When | Reason | `plan approve --review` |
|---|---|---|
| `--no-review` was given | `the user passed --no-review` | `skipped-flag` |
| You judge the plan low risk | your judgement in one line, naming what makes the plan low risk: few tasks, no one-way door, every `accept` entry probed `healthy`, nothing touching auth, data or a shared contract | `skipped-low-risk` |

Low risk is a judgement on this plan's evidence, not a threshold; when in doubt, run the review. Record the skip:

```bash
tomlctl plan run skip --slug <slug> --step review --reason '<reason from the Skip table>'
```

Keep the reason and the row's `--review` value: Approve passes both to `plan approve`, and the gate shows the reason under `risks` as `kind = "skip-reason"`.

### Run

Invoke the `review-v2-engine` skill and follow its `references/plan-flow.md`, sections 1 to 11, as `/plan-v2`: the review depth is this command's `--depth`, and there are no steering flags. Launch the `plan-v2-review` Workflow first, by its section 4, then record the step in the same turn with the run id the launch printed:

```bash
tomlctl plan run start --slug <slug> --step review --workflow plan-v2-review --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
```

End the turn and resume on the task notification. Unlike Investigate and the cold read, a failed launch does not stop the step: fall back to the review engine's stages by `plan-flow.md` section 5. When the launch never reached `async_launched`, record the step with `plan run start --slug <slug> --step review --session "$CLAUDE_CODE_SESSION_ID"`, passing no `--workflow` or `--run-id`, before falling back, so `plan-flow.md` section 11 has a row to close.

The review ends with `plan run finish --step review`, by section 11. An open `review` row on re-entry is resumed from the store by section 13, never relaunched.

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
3. `risks`: each `skip-reason` row's `detail`, then each `review-finding` row as `id`, `severity`, `target` and `summary`. A `review-finding` row is an open critical or major defect, or an open re-check finding, which the user approves past or acts on.
4. `task_mix`: tasks by file-count bucket and `executor`.
5. `review_edits`: each edit the review applied, as `id`, `target`, `tier`, `by` and `summary`, in the order given, which puts decisions first, then user, cited and mechanical.
6. `deferred_milestones`: each with its `reason`.
7. `stops`: each milestone gate command.
8. `handovers`: each undone handover, with its `reason`, `action`, `when` and `done_when`.

Then the secret-scan hits, each as `file:line` with the matched text, or `secret scan: clean`. Totals come last: the counts of tasks, milestones, assumptions and one-way doors. An empty array is shown as `none` under its heading, not dropped, so the user can tell an empty section from a missing one.

After the totals, show what approving does to the v1 carriers. Print this block verbatim as a quoted block, with only `<slug>` filled in, never paraphrased or shortened:

> Bare v1 carriers will now pick this flow, `/review` and `/optimise` included, and none can run it: `tasks import-plan --slug` refuses it, `/implement` has no named halt for that, and `/review-plan` falls back to plan mode and can overwrite `brief.md`. Review it with `/review-plan-v2`, whose flow mode edits the plan's records. `/implement-v2` is the command that runs it. Pass `--flow` to v1 carriers, or clear it with `tomlctl flow active remove --slug <slug>`.

Ask with `AskUserQuestion`: approve, or say what to change. When review was skipped, `risks` holds a `skip-reason` row; offer "review now" as a third option. Only "approve" goes on. "Review now" runs the Review step's Run section, then this step starts again from step 1, approving with `--review ran`; the skip row stays in `plan.toml` and still shows under `risks`. Any other answer, a secret-scan hit to remove included, routes back into the plan: the step that owns the change (Surface, Design, Write or Prove) makes it, Prove re-runs as far as the change unprobes, and this step starts again from step 1.

### 4. Commit the plan

`plan approve` refuses while `git status` reports anything for `tasks.toml`, `plan.toml`, `brief.md` or `design/`, because the stamp names `HEAD` and that commit must hold the plan. Commit first.

Before staging, check the shared files the second commit will carry. `.claude/active-flow.toml` is always one. `.claude/backlog.toml` is one only when some task has `closes`; this prints nothing when none does:

```bash
tomlctl tasks list --slug <slug> --where-has closes --select id --lines
```

Then check each file that applies:

```bash
git status --short -- .claude/active-flow.toml [.claude/backlog.toml]
```

A checked file already modified holds edits this plan did not make, and the second commit would sweep them in. Name them to the user and offer two options before committing anything: the user commits or stashes those edits themselves and this step runs again, or stop. Never stage edits this engine did not make into either commit, and never offer to include them.

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

When the review ran:

```bash
tomlctl plan approve --slug <slug> --review ran
```

When it was skipped, pass the Skip table row's `--review` value and its reason verbatim:

```bash
tomlctl plan approve --slug <slug> --review skipped-low-risk --reason '<reason from the Skip table>'
```

A review run after a skip, through "review now", approves with `--review ran`.

Each precondition is a `kind=validation` refusal, and nothing is written until all pass:

| Refusal | What clears it |
|---|---|
| A run is open; the message names the open steps | The open-run rule in `SKILL.md`: relaunch the oldest open step, or `plan run abandon --step <s> --reason <why>`, then step 1 again |
| A surfaced decision is still `agent-assumed` | Ask it through `AskUserQuestion` and record the answer with `plan decide`. A surfaced decision is never answered on the user's behalf |
| `plan check` reports an error | The verb `plan stage` names for that class, then Prove from the change on. Step 1 should have caught this; a refusal here means something changed after it |
| `` `--review ran` refused: `` with no finished plan-family run, or an unfinished one | Close the run by the review engine's `plan-flow.md` section 9, or resume an interrupted one by its section 13 |
| `` `--review ran` refused: `` with an open user-tier or question-tier finding | Ask it by `plan-flow.md` section 7: a user-tier row is accepted and applied with `--user-accepted`, or vetoed and dismissed; a question-tier row is revised through the `plan` verbs or kept, then resolved with `review triage --resolve` |
| `` `--review ran` refused: `` with an applied `spec.accept` edit whose entry has no probe | Re-probe by `plan-flow.md` section 8; an entry the emit skips needs its `check` fixed through Prove first |
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
- `.claude/backlog.toml` and `.claude/backlog.toml.sha256`, when step 4 found a task with `closes`.

Invoke the `commit-conventions` skill for the message and commit.

### 8. Warn

Print the v1-carrier warning from step 3 again, verbatim as a quoted block with the slug filled in, never paraphrased or shortened. Then end with the approved commit, the second commit, the stage `plan stage` now reports, and any `promoted` or `skipped_stale` backlog ids.
