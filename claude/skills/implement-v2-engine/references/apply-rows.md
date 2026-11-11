# Findings: the rules for apply rows

What changes when a milestone holds rows that fix or settle findings from a v2 review store. Every section here is skipped when the milestone holds no apply row, so a plan milestone runs exactly as [run-milestone.md](run-milestone.md) and [drain.md](drain.md) say, and reads nothing from this file.

`<slug>`, `<id>` and `<scratch>` mean what they mean in those references: the flow, the milestone and the session scratchpad.

## Apply rows

A row is an **apply row** when a `[[finding_links]]` entry in the flow's `tasks.toml` names its `ref`. Each entry carries:

| Field | Means |
|---|---|
| `ref` | The task row the link belongs to |
| `store` | The review store's repo-relative path: `.claude/flows/<s>/review-v2.toml` for a flow-bound review, `.claude/review-v2/<scope>.toml` for a flow-less one |
| `fixes` | Findings the row's work resolves |
| `settles` | Findings the row investigates, coming back with a proposal rather than a fix |

`review schedule` writes these links when it mints the rows, and marks each finding `scheduled` with its task. List the milestone's apply rows and the stores they name:

```bash
tomlctl tasks list --slug <slug> --where checkpoint=<id> --pluck ref --raw --lines
tomlctl items list .claude/flows/<slug>/tasks.toml --array finding_links --where-in ref=<ref1,ref2> --select ref,store,fixes,settles --lines
```

When the second command prints nothing, the milestone holds no apply row and nothing below applies to it.

A store path maps to the store flag every `tomlctl review` verb takes: `.claude/flows/<s>/review-v2.toml` is `--slug <s>`, and `.claude/review-v2/<scope>.toml` is `--scope <scope>`. `<store-flag>` below is that flag.

## What the worker sees

The worker's view, `tomlctl plan view --slug <slug> --for worker --task <n>`, prints a `## Findings` section for an apply row: each finding its links name, read from its review store at the moment the view runs through the implementer projection, headed with the finding id, whether the row `fixes` or `settles` it, and the store. A store that cannot be read, or a finding no longer in it, is one line in that section, never a failed view. Workers therefore need no review verb of their own, and the args file carries nothing extra for an apply row.

## Anchor premises on a started row

`review schedule` gives each apply row one `base` premise per finding, whose check is `tomlctl review anchor <finding> <store-flag>` (with `--gone` for a `remove` cleanup): it holds while the code the finding cites still reads as the finding says. Premises rerun before every launch ([run-milestone.md](run-milestone.md), step 3), and a row whose first worker already made the edit no longer matches its anchor.

So when acting on the recorded verdicts, read a `fails` verdict on an anchor premise, one whose `check` opens `tomlctl review anchor `, by whether its row is started, as the pre-flight's listing defines it (an `agent` of `plan-worker-lite` or `plan-worker-deep`):

| Row | An anchor premise that `fails` |
|---|---|
| Started | Is the row's own work. It does not withhold the row, and the relaunch goes ahead |
| Not started | Withholds the row, as any false premise does, and goes to the drain replan |

A `broken` anchor premise withholds the row either way, since it is the check that is wrong. Every other premise of an apply row is read as run-milestone step 3 reads it.

## Linked review stores in the clean-tree and lane checks

`review reconcile`, and the steps that record reproductions, settle proposals and schedule accepted findings, write a review store while the milestone is open. A store under `.claude/flows/<slug>/` is never listed by either check. Any other store a `[[finding_links]]` entry of the flow names, with its `.sha256` sidecar, is exempt in the same way as `.claude/backlog.toml`:

- from the pre-flight's clean-tree check ([run-milestone.md](run-milestone.md), step 1), so a dirty linked store does not stop the engine;
- from the drain's out-of-lane rule ([drain.md](drain.md), step 3), so a dirty linked store is not an edit outside every worker's lane.

List every store the flow links, for both checks:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array finding_links --pluck store --distinct --raw --lines
```

The exemption covers those paths and their sidecars only. Any other path under `.claude/review-v2/` or another flow's directory is still someone else's edit.

## Reproduce check

Runs from [drain.md](drain.md), step 6, when every row of the milestone is `done` and at least one row's link carries `fixes`, before the gate (step 7). A milestone where no row fixes a finding skips it and goes straight to the gate. The gate proves the build; this step asks of each fixed finding whether the failure or cleanup it describes can still occur in the working tree, so a passing gate is not the only evidence a finding is gone.

You never run a build, a test or a gate in this conversation to settle a verdict. The verifiers judge, and where you settle one yourself you read the cited code.

### The units

One call prints the launches, units and rounds; nothing is read from the task or finding-link listings and nothing is assembled by hand:

```bash
tomlctl plan emit reproduce-units --slug <slug> --milestone <id>
```

It prints `{"launches": [...]}`, one entry per review store the milestone's `[[finding_links]]` name. Each entry is the whole `review-v2-pipeline` args object: `mode`, `store`, `flow`, `milestone`, `round`, `run_dir` and `units`, each unit holding `task` (the row's `ref`), `files`, `findings` (the row's `fixes`) and `route`. The verb settles what the check needs and you do not recompute it: a finding that more than one row fixes, as a reproduce fix task's finding does (below), belongs only to the highest-id row's unit; a `settles` link is never a unit, since its row brings back a proposal, not a fix; `round` is one more than the highest `round` among that store's reproductions for the flow and milestone; and `run_dir` names the round and, when the milestone links more than one store, the store.

On the first check of the milestone in this invocation, call it as above, so every unit is checked. On a later pass, after a reproduce or gate fix round, add `--since-launch`: a unit is then kept only when one of its findings' latest reproduction is not `gone`, or its `files` share a path with a row the latest `implement:<id>` run dispatched.

```bash
tomlctl plan emit reproduce-units --slug <slug> --milestone <id> --since-launch
```

An empty `launches` means no unit is left: go to the gate.

**Routes.** A unit whose `route` is `checker` holds only grep-decidable findings, and the pipeline settles it with pooled `review-checker` agents rather than one `review-verifier` per unit; a checker records `gone` only when the finding's cited text is absent at its anchor, with `by = "checker"`. A finding a checker leaves `unsettled` goes on to a `review-verifier` in the same launch, so a `checker` unit needs no second launch of yours. A unit whose `route` is `verifier` goes to a `review-verifier` directly.

### Round and launch

Launch `review-v2-pipeline` once per `launches` entry, with that entry as its args, one after another, each with its own `reproduce:<id>` run started and finished before the next launch.

1. **The args.** Write the entry, exactly as the verb printed it, with the Write tool to `<scratch>/<run>-args.json`, where `<run>` is the last path segment of the entry's `run_dir`, which no other launch shares; this file is what the run's hash is taken over. The mode carries no pack and no schemas: the Workflow holds the return schemas itself. It refuses the whole launch, starting no agent, when `run_dir` is not one line, `store` holds neither or both keys, `flow`, `milestone` or a `task` is malformed, `round` is not a positive integer, `units` is empty, a `task` repeats, or a unit's `findings` is empty or holds an id not shaped `F<n>`; an entry the verb printed passes that validation unchanged.

2. **Launch and record.** The reproduce check is the engine's second Workflow launch, under the same rules as the worker launch ([run-milestone.md](run-milestone.md), step 4): never `resumeFromRunId`, and no Agent-tool fallback.

   ```
   Workflow({name: "review-v2-pipeline", args: <the args file's object>})
   ```

   A launch result carrying `error` stops the engine: report it in `needs_human` and record nothing. Otherwise, in the same turn, record the run and end the turn:

   ```bash
   hash=$(sha256sum <scratch>/<run>-args.json | cut -d' ' -f1)
   tomlctl plan run start --slug <slug> --step reproduce:<id> --workflow review-v2-pipeline --args-hash "$hash" --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
   ```

3. **What comes back.** Each checker and verifier reads its findings through the implementer projection, judges against the working tree whether each still reproduces, records its rows with `review reproduce --ndjson`, and changes no other file. The notification's result is `{mode, units: [{task, status, reproductions, error}], agents, errors, script_version}`, where a unit's `status` is `ok`, `no-return` or `error`, and `agents` counts the agents the launch started. An index whose `units` is empty and whose `errors` ends `no agent launched` is a refusal: close the run as `errors`, fix the args and launch again; never relaunch the same args. A notification with no index, the run stopped or failed before returning, closes the run with `plan run abandon --slug <slug> --step reproduce:<id> --reason '<why>'` and stops the engine, as a failed worker launch does. Otherwise close the run before the next launch:

   ```bash
   tomlctl plan run finish --slug <slug> --step reproduce:<id> --outcome ok
   ```

   with `--outcome errors` when the index's `errors` is not empty.

4. **Relaunch once.** The units whose `status` is not `ok` are relaunched once, together and in the same round, after the first run is finished, under a new `reproduce:<id>` run. Their args come from the verb, never from editing the earlier entry: `--relaunch 1`, `--round` the earlier entry's `round`, and `--task` the refs of the units that did not settle.

   ```bash
   tomlctl plan emit reproduce-units --slug <slug> --milestone <id> --relaunch <n> --round <r> --task <ref1,ref2>
   ```

   `<n>` counts the relaunches of that round from 1, and is 1 here, since a round is relaunched once. Without all three flags the verb would print a new round holding every unit. The printed entry keeps the round being relaunched and holds only the named units, and its `run_dir` ends `-r<n>`, so the relaunch writes and reads its payloads in a directory no earlier launch used. Take that entry through steps 1 to 3 as before. A unit that fails twice is not relaunched again; its findings are read below like any other, and with no reproduction recorded they fall to the `unsettled, or none` row.

### Read the store and act

Read the store, never the return: the return says what the checker or verifier meant to record, the store what it did. Once the launches of every store are done, read every verdict of the milestone with one call per store:

```bash
tomlctl review reproduce-status --flow <slug> --milestone <id> <store-flag> --expect gone
```

It prints `{ok, expect, findings: [{id, matches, verdict, flow, milestone, round, by}]}`, one entry per finding the milestone's rows fix in that store, with `verdict` the current reproduction: `gone`, `reproduces`, `unsettled`, or `null` with none. It exits 1 whenever any finding is not `gone`; that is an answer, not a failure, and every finding is still listed. A scope store takes `--scope <scope>` in place of `--slug <slug>` in the commands below. Act on each finding by its `verdict`:

| Current reproduction | The engine does |
|---|---|
| `gone` | Nothing |
| `reproduces` | Mints a fix task in this milestone, below |
| `unsettled`, or none | Reads the cited code itself, never running a build, test or gate, and records `gone` or `reproduces` with `by = "orchestrator"` in a `review reproduce --ndjson` row staged in `<scratch>`, in this round; then acts on that verdict by this table. When it cannot tell, it sets the row that fixes the finding to `parked` and writes that row's note, by [drain.md](drain.md), step 6, "Parking a row yourself" |

The orchestrator's row carries the same keys a verifier's does:

```json
{"finding": "F6", "flow": "<slug>", "milestone": "<id>", "round": 1, "task": "<task ref>", "verdict": "gone", "by": "orchestrator", "citation": "src/retry.rs:40: the bound is checked before the division"}
```

```bash
tomlctl review reproduce --slug <slug> --ndjson <scratch>/reproduce-<id>-<round>-orchestrator.ndjson
```

### Reproduce fix tasks

A finding that still reproduces gets one fix task in this milestone, minted with `tasks add-many` in the shape of [drain.md](drain.md), step 6, "New tasks":

- `reason` opens `reproduce <id> round <n>: `, then the finding and the verifier's citation, where `<n>` is one more than the highest `<n>` the count under **Two rounds at most** prints, or 1 when it prints none: the fix rounds spent, never the check's `round`, which also rises on a relaunch and after a gate fix round. That prefix is how a later invocation counts the rounds already spent.
- `files` are those of the row that fixed the finding, and `needs` follows the files as that section says.
- `review_store` is the store's repo-relative path and `fixes` names the finding, so the row is an apply row and its finding is checked again under its unit.
- It carries no `review anchor` premise: the code no longer reads as the finding first said, and the anchor would withhold the row.
- `spec.outcome` states the finding gone; `spec.executor` is `lite` when the citation names the fix and `deep` when finding why the fix fell short is part of the work.
- `spec.accept` holds one entry, the milestone-wide one `review schedule` gives every fix row of the milestone, byte for byte, so the gate list keeps it once:

  ```toml
  behaviour = "every finding this milestone fixes no longer reproduces"
  check = "tomlctl review reproduce-status --flow <slug> --milestone <id> <store-flag> --expect gone"
  kind = "forward"
  run_by = "gate"
  writes_build = false
  ```

  The gate runs it, never the worker, and it names no finding of its own.

Then relaunch through [run-milestone.md](run-milestone.md), step 3, and come back here when every row is `done`.

**Two rounds at most**, apart from the gate's own two. Count the rounds already minted for this milestone:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array changes --where-prefix "reason=reproduce <id> round " --select ref,reason --lines
```

The highest `<n>` printed is the rounds spent. A check that still finds a finding reproducing after round 2 mints nothing: go on from [drain.md](drain.md), step 8, without the gate or the commit, leave the milestone uncommitted, and report it in `needs_human` with each finding, its store, its latest citation and the round count. The fix rows stay `done` and their edits stay in place for the user.

A milestone cannot reach its commit with a finding outstanding: the gate runs only once this check leaves no finding `reproduces` or unsettled, and the one milestone-wide accept check every fix row carries, `review reproduce-status --flow <slug> --milestone <id> <store-flag> --expect gone`, covers every finding the milestone's rows fix in that store and fails the gate if any is not `gone`. One such check runs per store the milestone links.

## Settle drain

Runs from [drain.md](drain.md), step 6, when every row of the milestone is `done` and at least one row's link carries `settles`, after the reproduce check and before the gate (step 7). A settle milestone, which `apply-v2-engine` adds before its fix milestone when a selected finding is blocked on evidence, has no gates, so its commit follows this step directly. This is the one point where the user is asked about the proposals, so every proposal of the milestone is judged before any is recorded.

A settle row's `files` is one proposal file, `docs/flows/<slug>/notes/settle/<F>.md`, and its worker writes that file beside its run note. List the rows, their links and their findings:

```bash
tomlctl tasks list --slug <slug> --where checkpoint=<id> --select id,ref,files --lines
tomlctl items list .claude/flows/<slug>/tasks.toml --array finding_links --where-in ref=<ref1,ref2> --select ref,store,settles --lines
tomlctl review show <F1,F2> --slug <slug> --for orchestrator
```

A finding the last command shows no longer `scheduled` was recorded by an earlier pass of this step, since every `review settle` outcome takes it off `scheduled`: skip steps 1 to 4 for it, and take it to step 5 when it is `open` and no longer blocked.

1. **Read** each proposal file with the Read tool, and the code it cites. A file whose first line opens neither `Proposal:` nor `Refuted:`, or that asks for more investigation instead of proposing a fix, or that does not answer what the finding's `blocked_missing` says is missing, fails its row: set it `failed` with `tomlctl tasks update <n> --slug <slug> --status failed` and handle it by **Failure on an apply row**, below. Record nothing for any finding of the milestone while a row is not `done`; relaunch through [run-milestone.md](run-milestone.md), step 3, and come back here when every row is `done` again.
2. **Tier** each `Proposal:` by what shows its fix is right:

   | Tier | When |
   |---|---|
   | `mechanical` | A read-only command the proposal names (a search, a `tomlctl` read, a `git show`) prints what the proposal says it prints when you run it. Never a build, a test or a gate |
   | `cited` | Reading the lines the proposal cites shows what it says they show |
   | `user` | Anything else |

   A `Refuted:` whose citation, read, shows the finding does not hold is recorded with `--refute` at step 4. One whose citation does not show that is put to the user with the `user`-tier proposals.
3. **Ask once.** Put every `user`-tier proposal and every unconfirmed refutation to the user through `AskUserQuestion`, one question per finding with the proposal's fix and its evidence, and options to accept it or defer it (and, for a refutation, to refute the finding). Four questions fit one call; send the calls one after another at this step, never spread across the milestone. A `mechanical` or `cited` proposal is accepted without asking.
4. **Record** each finding with one `review settle` call, in its store:

   ```bash
   tomlctl review settle <F> --slug <slug> --accept --tier <mechanical|cited|user> --fix-file <scratch>/settle-<id>-<F>-fix.md
   tomlctl review settle <F> --slug <slug> --refute --citation '<what shows the finding false>'
   tomlctl review settle <F> --slug <slug> --defer --proposal docs/flows/<slug>/notes/settle/<F>.md
   ```

   `--accept` takes the fix the proposal settled on, staged with the Write tool from the proposal file's edit-level fix; the user tier is accepted only on the user's yes. A rejected or unanswered proposal is deferred: the finding keeps its proposal, gets no second settle round, since `review schedule` refuses an evidence block whose proposal is already made, and ages toward a forced disposition at its family's later review runs.
5. **Schedule** the accepted findings into the fix milestone, one call per store. The fix milestone is the checkpoint that follows this one in store order, which `apply-v2-engine` adds right after it:

   ```bash
   tomlctl items list .claude/flows/<slug>/tasks.toml --array checkpoints --select id --lines
   tomlctl review schedule <F1,F2> --slug <slug> --flow <slug> --checkpoint <fix id>
   ```

   The verb writes each minted row's `reason` itself. Report its `held` and `refused` rows in `needs_human`: such a finding stays `open` with its accepted fix, for a later `/apply-v2`. When no checkpoint follows this one, schedule nothing and report the accepted findings there the same way.

The milestone commit then holds the proposal files, which are its rows' `files`; the review store, the task store with the minted fix rows, and the run notes go in the flow-store commit, as **Reconcile** stages them. Reconcile finds no settle finding still `scheduled` into this flow, since step 4 took each off; one it does find, because this step never recorded it, it reopens as committed with no recorded proposal.

## Failure on an apply row

These rules replace [drain.md](drain.md), step 6's for a `failed` or `parked` apply row only. Plan rows keep that step's rules, its **Never up a tier** among them.

- **A fix worker cannot reclassify.** A return on an apply row that says the task needs a decision or more evidence is `failed`, whatever status the worker wrote: the review already judged the finding fixable, and a settle task is where evidence is found.
- **A `parked` return on an apply row is read as `failed`** unless the note's reason is one a handover covers: `restart`, `permissions`, `attended`, `observe`, `credentials` or `external` ([drain.md](drain.md), step 9). Set such a row `failed` with `tomlctl tasks update <n> --slug <slug> --status failed` before the replan.
- **Tier.** Count the row's earlier failures, from the replans that answered them:

  ```bash
  tomlctl items list .claude/flows/<slug>/tasks.toml --array changes --where ref=<ref> --where-prefix "reason=apply failure " --select field,reason --lines
  ```

  The highest `<n>` in a reason opening `apply failure <n>: ` is the failures already spent, or none.

  | Row | First failure | Second failure |
  |---|---|---|
  | `executor = "lite"` | Move it to `deep` with its spec otherwise unchanged, through an audited update whose `reason` opens `apply failure 1: ` and says the row moved up a tier on the worker's return | Park it for the user |
  | `executor = "deep"` | Replan it by [drain.md](drain.md), step 6's re-spec rule, with the `reason` opening `apply failure 1: ` | Park it for the user |

  The `lite` move is the one place an `executor` moves with nothing else changed, and it applies to apply rows alone:

  ```toml
  [[record]]
  id = 14
  reason = "apply failure 1: the worker's note says the fix needs a decision on the error kind; moved up a tier with its spec unchanged"
  [record.spec]
  executor = "deep"
  ```

  A second failure parks the row with `tomlctl tasks update <n> --slug <slug> --status parked`, and you write its note by [drain.md](drain.md), step 6, "Parking a row yourself", carrying the worker's note forward as the evidence. A parked apply row is re-asked at the next invocation like any other.

## Reconcile

Runs in [drain.md](drain.md), step 10, after the milestone commit is stamped on its rows and before the flow-store commit, when the milestone holds an apply row.

For each distinct store the milestone's links name, from the listing under **Apply rows**, with its store flag:

```bash
tomlctl review reconcile --slug <s> --flow <slug>
tomlctl review reconcile --scope <scope> --flow <slug>
```

It moves each finding scheduled into `<slug>` by its task's state, by the table in the `tomlctl` skill's references/review-apply.md, section `tomlctl review reconcile`. A finding listed under `in_flight` after its milestone is stamped belongs to a row of another milestone; it is not an error.

A refusal stops the engine before the flow-store commit, as a failed commit does: make no flow-store commit, leave the milestone commit and its stamps in place, and report the store and the refusal in `needs_human`, so the user fixes the store and the reconcile and the flow-store commit are made by hand or by a later invocation.

**Staging.** A reconciled store under `.claude/flows/<slug>/` is already in the flow-store commit's paths. A store outside it joins that commit with its sidecar: add `<store>` and `<store>.sha256` to the `git status --porcelain --` and `git add --` lists of the flow-store commit, and to its `git commit --` pathspec, so the pre-commit hook compares the pair. It never goes in the milestone commit, which stays the rows' `files` alone: a milestone is still exactly two commits.
