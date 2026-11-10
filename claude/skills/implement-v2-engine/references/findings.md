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

A store path maps to the store flag every `tomlctl review` verb takes: `.claude/flows/<s>/review-v2.toml` is `--slug <s>`, and `.claude/review-v2/<scope>.toml` is `--scope <scope>`. `<store flag>` below is that flag.

## What the worker sees

The worker's view, `tomlctl plan view --slug <slug> --for worker --task <n>`, prints a `## Findings` section for an apply row: each finding its links name, read from its review store at the moment the view runs through the implementer projection, headed with the finding id, whether the row `fixes` or `settles` it, and the store. A store that cannot be read, or a finding no longer in it, is one line in that section, never a failed view. Workers therefore need no review verb of their own, and the args file carries nothing extra for an apply row.

## Anchor premises on a started row

`review schedule` gives each apply row one `base` premise per finding, whose check is `tomlctl review anchor <finding> <store flag>` (with `--gone` for a `remove` cleanup): it holds while the code the finding cites still reads as the finding says. Premises rerun before every launch ([run-milestone.md](run-milestone.md), step 3), and a row whose first worker already made the edit no longer matches its anchor.

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

List the milestone's rows and their links, as under **Apply rows**:

```bash
tomlctl tasks list --slug <slug> --where checkpoint=<id> --select id,ref,files --lines
tomlctl items list .claude/flows/<slug>/tasks.toml --array finding_links --where-in ref=<ref1,ref2> --select ref,store,fixes --lines
```

Each row with a non-empty `fixes` is one unit: `task` its `ref`, `files` its `files`, `findings` its `fixes`. A finding that more than one row fixes, as a reproduce fix task's finding does (below), goes only in the unit of the highest-id row among them. A `settles` link is never checked here: its row brings back a proposal, not a fix.

On the first check of the milestone in this invocation every unit is checked. On a later pass, after a reproduce or gate fix round, a finding is checked again only when its current reproduction is not `gone` (`review reproduce-status <F> <store flag> --expect gone` exits 1), or when its unit's `files` share a path with a row the latest `implement:<id>` launch dispatched. A unit left with no finding drops out; when no unit is left, go to the gate.

### Round and launch

The units of one launch share one review store. When the milestone links more than one, run the check below once per store, one after another, each with its own `reproduce:<id>` run started and finished before the next launch.

1. **The round** is one more than the highest `round` among this store's reproductions for the flow and milestone, or 1 when there are none:

   ```bash
   tomlctl items list <store path> --array reproductions --where flow=<slug> --where milestone=<id> --pluck round --raw --lines
   ```

2. **The args.** Write them with the Write tool to `<scratch>/reproduce-<id>-<round>-args.json`:

   ```json
   {"mode": "reproduce", "run_dir": ".review-v2-runs/apply-<slug>-<id>-<round>", "store": {"slug": "<s>"}, "flow": "<slug>", "milestone": "<id>", "round": 1, "units": [{"task": "<task ref>", "files": ["<path>"], "findings": ["F3"]}]}
   ```

   `store` holds `slug` for `.claude/flows/<s>/review-v2.toml` or `scope` for `.claude/review-v2/<scope>.toml`, never both. When a second store is checked in the same round, its `run_dir` ends `-<s>` or `-<scope>` so the two runs keep apart. The mode carries no pack and no schemas: the Workflow holds the unit's return schema itself. It refuses the whole launch, starting no agent, when `run_dir` is not one line, `store` holds neither or both keys, `flow`, `milestone` or a `task` is malformed, `round` is not a positive integer, `units` is empty, a `task` repeats, or a unit's `findings` is empty or holds an id not shaped `F<n>`.

3. **Launch and record.** The reproduce check is the engine's second Workflow launch, under the same rules as the worker launch ([run-milestone.md](run-milestone.md), step 4): never `resumeFromRunId`, and no Agent-tool fallback.

   ```
   Workflow({name: "review-v2-pipeline", args: <the args file's object>})
   ```

   A launch result carrying `error` stops the engine: report it in `needs_human` and record nothing. Otherwise, in the same turn, record the run and end the turn:

   ```bash
   hash=$(sha256sum <scratch>/reproduce-<id>-<round>-args.json | cut -d' ' -f1)
   tomlctl plan run start --slug <slug> --step reproduce:<id> --workflow review-v2-pipeline --args-hash "$hash" --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
   ```

4. **What comes back.** Each unit's `review-verifier` reads its findings through the implementer projection, judges against the working tree whether each still reproduces, records its rows with `review reproduce --ndjson`, and changes no other file. The notification's result is `{mode, units: [{task, status, reproductions, error}], errors, script_version}`, where a unit's `status` is `ok`, `no-return` or `error`. An index whose `units` is empty and whose `errors` ends `no agent launched` is a refusal: close the run as `errors`, fix the args and launch again; never relaunch the same args. A notification with no index, the run stopped or failed before returning, closes the run with `plan run abandon --slug <slug> --step reproduce:<id> --reason '<why>'` and stops the engine, as a failed worker launch does.

5. **Relaunch once.** A unit whose `status` is not `ok` is relaunched once, alone, in the same round: new args holding only that unit, a new `reproduce:<id>` run, after the first run is finished. A unit that fails twice is not relaunched again; its findings are read in step 6 like any other, and with no reproduction recorded they fall to the `unsettled, or none` row.

### Read the store and act

Read the store, never the return: the return says what the verifier meant to record, the store what it did. For each finding of the launch, one call:

```bash
tomlctl review reproduce-status <F> --slug <slug> --expect gone -q
```

A scope store takes `--scope <scope>` in place of `--slug <slug>` in this and the commands below.

and, for a finding where it exits 1, a second with `--expect reproduces`. A finding that fails both is `unsettled` or has no reproduction.

Once every finding of the launch is read, close the run:

```bash
tomlctl plan run finish --slug <slug> --step reproduce:<id> --outcome ok
```

with `--outcome errors` when the index's `errors` is not empty.

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

- `reason` opens `reproduce <id> round <n>: `, then the finding and the verifier's citation. That prefix is how a later invocation counts the rounds already spent.
- `files` are those of the row that fixed the finding, and `needs` follows the files as that section says.
- `review_store` is the store's repo-relative path and `fixes` names the finding, so the row is an apply row and its finding is checked again under its unit.
- It carries no `review anchor` premise: the code no longer reads as the finding first said, and the anchor would withhold the row.
- `spec.outcome` states the finding gone; `spec.executor` is `lite` when the citation names the fix and `deep` when finding why the fix fell short is part of the work.
- `spec.accept` holds one entry: `check = "tomlctl review reproduce-status <F> <store flag> --expect gone"`, `kind = "forward"`, with no `writes_build = false`, so the gate runs it rather than the worker.

Then relaunch through [run-milestone.md](run-milestone.md), step 3, and come back here when every row is `done`.

**Two rounds at most**, apart from the gate's own two. Count the rounds already minted for this milestone:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array changes --where-prefix "reason=reproduce <id> round " --select ref,reason --lines
```

The highest `<n>` printed is the rounds spent. A check that still finds a finding reproducing after round 2 mints nothing: go on from [drain.md](drain.md), step 8, without the gate or the commit, leave the milestone uncommitted, and report it in `needs_human` with each finding, its store, its latest citation and the round count. The fix rows stay `done` and their edits stay in place for the user.

A milestone cannot reach its commit with a finding outstanding: the gate runs only once this check leaves no finding `reproduces` or unsettled, and each fixing row's accept check, `review reproduce-status <F> --expect gone`, fails the gate if one is.

## Reconcile

Runs in [drain.md](drain.md), step 10, after the milestone commit is stamped on its rows and before the flow-store commit, when the milestone holds an apply row.

For each distinct store the milestone's links name, from the listing under **Apply rows**, with its store flag:

```bash
tomlctl review reconcile --slug <s> --flow <slug>
tomlctl review reconcile --scope <scope> --flow <slug>
```

It moves each finding scheduled into `<slug>` by the state of its task: a `fixes` finding whose task is `done` with a commit is `resolved`; a `settles` finding whose task is committed goes back to `open`; a finding whose task is deferred or gone goes back to `open` with its apply link cleared; anything else stays `scheduled`. It prints `{ok, flow, review_store, resolved, reopened, in_flight, retitled}` and writes the store only when something moved. A finding listed under `in_flight` after its milestone is stamped belongs to a row of another milestone; it is not an error.

A refusal stops the engine before the flow-store commit, as a failed commit does: make no flow-store commit, leave the milestone commit and its stamps in place, and report the store and the refusal in `needs_human`, so the user fixes the store and the reconcile and the flow-store commit are made by hand or by a later invocation.

**Staging.** A reconciled store under `.claude/flows/<slug>/` is already in the flow-store commit's paths. A store outside it joins that commit with its sidecar: add `<store>` and `<store>.sha256` to the `git status --porcelain --` and `git add --` lists of the flow-store commit, and to its `git commit --` pathspec, so the pre-commit hook compares the pair. It never goes in the milestone commit, which stays the rows' `files` alone: a milestone is still exactly two commits.
