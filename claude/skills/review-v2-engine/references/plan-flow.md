# Plan review on a v2 flow

A plan-family run is in **flow mode** when its flow has `.claude/flows/<slug>/plan.toml`, and in markdown mode otherwise. This reference is the whole orchestrator procedure for flow mode. `/plan-v2` runs it as its Review step, and `/review-plan-v2` runs it when the resolved flow has `plan.toml`. Markdown mode is unchanged: the engine's stages and the carrier's merge offer, as written.

Flow mode differs from the engine's stages in four ways:

- **Scope.** The scope is `docs/flows/<slug>/brief.md` plus every `docs/flows/<slug>/design/*.md`. No task-store import runs: the store is already canonical, and `tasks import-plan` refuses a v2 flow.
- **Findings.** Findings target records (`task:<ref>.spec.accept[1]`, `decision:D3.why`, `brief#Background`) with an `expect` and a `new`, and tomlctl computes each one's `tier`. The reviewer side is `review-v2-method`'s `references/plan-records.md`; the tool side is the `tomlctl` skill's `references/review.md`, section "Plan records".
- **Coverage.** The saved Workflow `plan-v2-review` runs stages 2 to 6: the survey and baselines, a `plan-review-lead` composing the map, `review-v2-pipeline` waves the lead settles, and the lead's audit. The engine's own stages are the fallback.
- **Close.** Tiered apply through `tomlctl review apply` replaces the merge offer, then a narrower re-check run reviews what the edits changed.

The engine's role rules hold throughout: read digests and projections, never rows; refer to findings by id; stage payloads and user prose with the Write tool under `<run-dir>/payloads/`; never quote user text into a shell argument. `<ids>` in a command is finding ids, comma-separated. `<scratch>` is the session's scratchpad directory, where the models table is staged because no run directory exists before the first run starts.

## Contents

1. Pre-flight
2. Run start
3. Survey signals and schemas
4. Launch
5. Reading the return, and the fallback
6. Mop-up
7. Triage by tier
8. Re-probe
9. Close the first run
10. Re-check
11. Finish
12. An approved flow
13. Re-entry

## 1. Pre-flight

1. The installed binary must carry the apply verb:

   ```bash
   tomlctl capabilities --get features
   ```

   Stop when `review_apply`, `review_run_head`, `review_run_show`, `review_ruled_out`, `review_same_as_link`, `review_blocked_on`, `review_settle` or `review_triage_answer` is absent, naming `cargo install --path tomlctl` in the dev-tools checkout and a restart.
2. Bind `<slug>`. `/plan-v2` has it already. `/review-plan-v2` takes it from the envelope its engine stage 0 built.
3. Skip the engine's stage 0 steps 4 and 6. Step 4's `review sync-v1` reads v1 ledgers, and the plan family never exports to one. Step 6's task import is refused on a v2 flow. Step 5's listing of parked advisory and open blocked findings still runs.
4. Read where the plan stands:

   ```bash
   tomlctl plan stage --slug <slug>
   ```

   On `approved`, `running` or `done`, section 12 governs every edit. Settle any open review run by section 13 before anything else.

## 2. Run start

Read the `model:` line of each of the eight agents with Grep, under `<claude dir>/agents/`: the engine's seven (`review-surveyor`, `review-sweeper`, `review-checker`, `review-finder`, `review-finder-cf`, `review-verifier`, `plan-executor`) and `plan-review-lead`. Record the lead's model under `lead`. Stage the table as JSON with the Write tool, since the run directory does not exist yet: use the session scratchpad, then pass it by `@` path. Stage the scope list there too, as `<scratch>/review-scope.json` holding `["docs/flows/<slug>/brief.md","docs/flows/<slug>/design/*.md"]`.

```bash
tomlctl review run start --slug <slug> --command review-plan-v2 --family plan --depth <depth> --base HEAD --set-json 'scope=@<scratch>/review-scope.json' --set-json models=@<scratch>/review-models.json --set substrate=<workflow|agent-waves>
```

`substrate` follows the engine's rule: `workflow` when the Workflow tool is available in this session, otherwise `agent-waves`. `--command` is `review-plan-v2` from either carrier. `<depth>` is the carrier's `--depth`, default `standard`. Bind `<run>` and `<run-dir>` from what it prints. A catalogue validation error halts the review, as the engine's stage 1 says; so does a refusal over `.claude/review-lenses`, which takes `--project` only on the user's say-so.

## 3. Survey signals and schemas

The survey records the deterministic signals, and `tomlctl review run show <run> --slug <slug>` returns them whenever they are needed; the surveyor reads them that way, so the wrapper is passed none:

```bash
tomlctl review survey --run <run> --slug <slug>
```

For each `unknown` signal it prints that would have activated a lens, add `--lens +<id>` to the steering, or leave it and name it in the close summary.

Fetch the two return schemas the waves need, and the ids of the catalogue's mechanical lenses that are not disabled, which the wrapper passes to the pipeline as `mechanical_lenses`:

```bash
tomlctl review schema cell-return
tomlctl review schema verdict-return
tomlctl review catalogue --family plan --rows entries --where type=lens --where mode=mechanical --where disabled=false --select id --lines
```

## 4. Launch

Write the args object with the Write tool to `<run-dir>/payloads/wrapper-args.json`, so a later session can see what was launched, then launch:

```text
Workflow({
  name: "plan-v2-review",
  args: {
    slug: "<slug>",
    run: "<run>",
    mode: "full",
    depth: "<depth>",
    run_dir: "<run-dir>",
    ledger_line: "ledger: .claude/flows/<slug>/review-v2.toml",
    signals: {
      steering: ["--lens +contrarian", "--probe \"<question>\""]
    },
    mechanical_lenses: ["<lens id>"],
    schemas: { cell_return: <cell-return schema>, verdict_return: <verdict-return schema> }
  }
})
```

- `signals.survey` is no longer read or checked: the surveyor reads its signals from the store.
- `mechanical_lenses` is section 3's catalogue output, one id per entry; omit it when the query printed none.
- `signals.steering` holds one flag per entry: `/review-plan-v2`'s `--lens` and `--probe` flags, plus any `--lens +<id>` section 3 added. Omit it when there are none. `/plan-v2` has no steering flags.
- `targets` is absent in `full` mode.

`/plan-v2` records the step in the same turn, once the launch reports `async_launched`, with the run id the launch printed. `/review-plan-v2` writes no `plan.toml` run rows, because those record `/plan-v2`'s steps.

```bash
tomlctl plan run start --slug <slug> --step review --workflow plan-v2-review --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
```

End the turn and resume on the task notification. The wrapper writes nothing back but its index; every agent it runs writes the store itself.

## 5. Reading the return, and the fallback

The return is `{slug, run, mode, waves: [{wave, cells, verified, agents, settled}], unverified, audit, agents, errors, script_version}`. `agents` counts every dispatch the wrapper made, its own and each wave's. Each wave's pipeline index also carries a top-level `escalations` array of `{cell, text}`; the wrapper hands it to the lead's `settle` phase, whose return may carry `reverify`, ids of findings the wrapper re-verifies once. The index does not return the escalations: the lead writes each one that gets no follow-up cell to `<run-dir>/payloads/escalations-w<wave>.md`, which the close summary in section 11 reads. With `errors` empty, every wave settled and `audit` set, go to section 6.

Otherwise, fall back to the engine at the first stage the store shows incomplete, and stay on the engine for the rest of the run. Never relaunch the wrapper over a run that already holds a wave: its compose phase would mint a second wave over the first. Read the store state with the digest and the run directory:

```bash
tomlctl review digest --run <run> --slug <slug>
```

| What failed | `errors` reads | Resume at |
|---|---|---|
| The launch was refused, errored, the name was unknown, or the notification carried no result | — | the first row below the store matches |
| The args were refused | ends `no agent launched` | fix the args from `errors` and launch once more; nothing ran |
| The surveyor failed or stopped `missing:` | `surveyor …; no cell composed` | engine stage 2, in flow mode, per `survey.md` |
| Compose failed or wrote nothing | ends `fall back to engine stage 3` | engine stage 2's surveyor and baselines once more, then stage 3 |
| A wave threw, ran no cell, or its settle failed | ends `no audit ran` | engine stage 4 for the unsealed cells when the digest lists any, else stage 5 |
| Only the audit failed | `lead audit …` | engine stage 6 |
| A follow-up was not launched | names its cells | stage 5's follow-up decision: launch those cells as the next wave, within the envelope, or waive them |

Two reasons the compose row restarts at stage 2. The surveyor's digest went to the Workflow, not to you, so you hold neither its candidates nor its `verification:` lines. And stage 4 waits on a recorded baseline for every one of those lines; the Workflow records them from one `verification` dispatch with `keep_going: true`, and `tomlctl review run show <run> --slug <slug>` lists the keys recorded. A second surveyor on the same run rewrites `profile.md` and `pack.md` in place.

A store with no `profile.md` in `<run-dir>` is at stage 2; one with no cells in the digest is at stage 3, handled as the compose row; one with unsealed cells is at stage 4. Engine stage 4 for unsealed cells launches `review-v2-pipeline` with only those cells, as `agent-waves.md` ("What recovering a Workflow run costs") describes, or runs them as Agent waves. A fallback at stage 3 or later composes, settles and audits as the engine's stages say, in flow mode: the surveyor prompt carries `flow: <slug>` in place of `plan:`, and each cell assignment names the flow.

The wrapper records no `substrate` change. Name a fallback in the close summary as `substrate: plan-v2-review → engine at stage <n>`.

## 6. Mop-up

The wrapper cannot dispatch outside its script, so the `unverified` ids it returns, and any cell it left unsealed, are yours. Settle the store and read the whole run:

```bash
tomlctl review resolve --run <run> --slug <slug>
tomlctl review digest --run <run> --slug <slug>
```

- Each unverified id gets a mop-up dispatch by its route, per `agent-waves.md`, section "Mop-up". Repeat until none is left or an id fails twice.
- Each unsealed cell gets one re-dispatch with the identical cell prompt, per `agent-waves.md`, section "The wave barrier". Never re-compose it through `review plan`, which would mint a second cell beside it.
- An id that two mop-ups failed to verify stays `unverified`, and section 9 finishes the run with `--incomplete-file` naming it.

The audit sampled the digest before the mop-up, so findings verified here are not in its sample. Do not re-audit them.

The run's agent total is the engine's stage 4 definition, with the wrapper's index standing as a wave index: its `agents` already counts the wrapper's own dispatches, so add only what you dispatch outside it. Section 9 records it before the finish.

At `deep`, the lead wrote the map without asking the user. Say in the close summary that the map was not confirmed.

## 7. Triage by tier

Read every open finding with its `tier`, `target`, `expect` and `new`:

```bash
tomlctl review list --slug <slug> --run <run> --status open --for implementer
```

Group the rows by `tier`. Tiers are computed when a finding opens and frozen after, so read them; never re-derive one. On an approved flow, section 12 replaces this section's first two steps.

### Mechanical and cited

Apply every open mechanical and cited defect or cleanup row without asking, in one call, previewed first:

```bash
tomlctl review apply <ids> --slug <slug> --dry-run
tomlctl review apply <ids> --slug <slug>
```

- **Refusal.** The verb refuses the whole call, writing nothing, and lists each refused id with its reasons. Drop the refused ids, report each with its reason, and apply the rest. A refused id stays `open`, and shows at the gate under `risks` when it is a critical or major defect.
- **Stale.** `skipped_stale` rows lost a race with an earlier edit to the same record. Report them under `changed during the review` and never retry them; the next review reads them through `PRIOR`.
- **Already.** `already` ids were made before, by a session that died before stamping them; the verb stamps them now.
- **Mid-batch failure.** `applying <id> failed; stamped before it: […]` leaves at most that one edit unstamped. Re-run the call with only the ids not listed as stamped: the stamped ids are no longer `open`, so the verb would refuse a call naming them, and the unstamped edit that did land is recognised as `already`.

Keep the `target` of every `applied` and `already` row. Section 8 re-probes from them, and section 10 re-checks them.

Cited edits are applied without asking but are disclosed: the approval gate lists every applied edit under `review_edits`, decisions first, then user, cited and mechanical.

### User tier

A user-owned target is a decision, a `spec.outcome`, or the brief's Intent, Non-goals, Scope budget or Success criteria. A row on one is asked whatever its evidence. It is user tier when it is a defect or cleanup with a `new`, an edit `review apply` can write; any other row on such a target is question tier, below. Read the batch through the implementer projection, which carries the fix and the edit:

```bash
tomlctl review show <ids> --slug <slug> --for implementer
```

Ask in `AskUserQuestion` batches of up to four, one question per finding. Each question names the target, the current value and the proposed value in one or two lines, with the finding's summary, and offers **Accept** and **Veto**.

- **Accept.** Apply the accepted ids together:

  ```bash
  tomlctl review apply <ids> --slug <slug> --user-accepted
  ```

  The verb records `by = "user"`. Its refusals, stale rows and failures read as above.
- **Veto.** Stage the user's reason with the Write tool, or `vetoed at the plan review` when they gave none, and dismiss. `review stats --plan-outcomes` counts a dismissed user-tier row as a veto.

  ```bash
  tomlctl review triage <ids> --slug <slug> --dismiss-file <run-dir>/payloads/veto.md --expect-status open
  ```

- **No answer.** A headless session leaves the rows `open`. `plan approve --review ran` refuses while any user-tier or question-tier finding is open, and `plan stage` counts them as a blocker. Say `user tier pending` in the close summary.

### Question tier

A question-tier row is an advisory, or a defect or cleanup with no `new`, on a user-owned target. It carries no edit, so `review apply` refuses it, and `review triage --park` refuses it too: only the user answers it. Ask it in the same `AskUserQuestion` batches, naming the target, its current value and the finding's summary, and offer **Revise** and **Keep**, never Accept or Veto.

- **Revise.** The user's revision goes through the `plan` verbs that own the record, as an attended edit. Then resolve the row with `revised by user` staged:

  ```bash
  tomlctl review triage <ids> --slug <slug> --resolve-file <run-dir>/payloads/revised.md --expect-status open
  ```

- **Keep.** Resolve the row the same way with `kept by user` staged. `review stats --plan-outcomes` counts a resolved question row as `user-answered`, outside the veto rate.
- **No answer.** As for the user tier: the row stays `open` and blocks `plan approve --review ran` and `plan stage`.

A question-tier advisory can also become a proposed edit. When the user wants a change but leaves its wording to the review, or the review can propose one worth asking, convert the advisory with `review triage <id> --slug <slug> --convert defect|cleanup --ndjson <path>`, staging a replacement that carries the `new`. `--convert` accepts a question-tier row, unlike `--park`. The replacement keeps the user-owned target, so once verified it is tiered `user` and asked with **Accept** and **Veto** as above; a replacement without a `new` is a question again. It still applies only with `review apply --user-accepted`, so conversion does not let anyone but the user approve the edit.

A row stamped `user` before the question tier existed keeps `user`: tiers are frozen. One that carries no `new` is refused by `review apply`; close it as a question.

### Advisory, and what the tiers leave

Present, in one message, with a recommendation and a one-line reason for each item as the engine's stage 7 says:

- the advisory rows, recommending park, backlog, convert or dismiss;
- defect and cleanup rows whose tier is `advisory`: no `new`, a proof that did not hold, or no citation in the target's file. They are never applied. The user may dismiss or defer them; one left `open` shows at the gate under `risks` when it is critical or major;
- the ids the applies above refused.

Dispositions are the user's, as at the engine's stage 7. The grammar is the engine's, without `apply`, which the plan family never does. A `convert` in flow mode writes the replacement with `target`, `expect` and `new` per `plan-records.md`; it is minted `unverified` and verified in this run before section 9. Backlog picks go through the `backlog-capture` skill, as the engine's stage 8 says.

## 8. Re-probe

An applied edit to a `spec.accept` entry drops that entry's probe, so the plan's stage returns to `prove`. `plan approve --review ran` also refuses while an entry an applied finding changed has no probe. Re-probe whenever any `applied` or `already` target from section 7 is a `task:<ref>.spec.accept` target, with or without an index:

```bash
tomlctl plan probes --slug <slug> --emit > <run-dir>/payloads/probes.sh
```

Read the script before running it: each line runs a stored command with your rights, and running it through `bash` puts none of them in front of a permission prompt.

```bash
timeout 540 bash <run-dir>/payloads/probes.sh > <run-dir>/payloads/probes.out
tomlctl plan probes --slug <slug> --record <run-dir>/payloads/probes.out
```

`--record` needs a capture covering every probeable control of the pending rows, so it re-probes the whole plan, not only the edited entries. On exit 124 record nothing, and narrow the run with `--milestone <id>` on both `--emit` and `--record`.

- **Unhealthy.** A recorded verdict other than `healthy` is a `plan check` error that blocks approval. Fix it as the cited edit would have: a new finding for the next review, or, under `/plan-v2`, its Prove step's spec fix and probes again.
- **No probe line.** The emit skips an entry whose `check` is empty or whose `writes_build` is not `false`, so the edit leaves that entry unprobed and approval refuses. Name such entries in the close summary as blockers for the user.

## 9. Close the first run

In this order:

1. **Park.** Park every advisory the digest still lists as untriaged, except user-tier and question-tier rows, with one staged reason, `untriaged at the close of <run>`. Those stay `open` for the user, and `review triage --park` refuses them:

   ```bash
   tomlctl review triage <ids> --slug <slug> --park-file <run-dir>/payloads/park-untriaged.md --expect-status open
   ```

2. **Report.**

   ```bash
   tomlctl review report --run <run> --slug <slug> --out <run-dir>/report.md
   ```

3. **Finish.** Record the run's agent total first, as section 6 says:

   ```bash
   tomlctl review run update <run> --slug <slug> --set-json agents=<total>
   tomlctl review run finish <run> --slug <slug>
   ```

   It refuses while an id is `unverified`, a `same_as` is unresolved, an unselected concern or unsliced file lacks a waiver, or a cell is unsealed. Fix the cause and finish again. Finish with `--incomplete-file <path>` only for a gap this run cannot close, the staged reason naming each open id or gap.

The first run finishes before the re-check starts. A finished run is a fixed `PRIOR` for the re-check's finders, and when the re-check cannot launch, the first run has already settled what it can.

## 10. Re-check

Run a re-check when section 7 recorded at least one `applied` or `already` row. With none, skip to section 11.

**Run start.** A fresh run at `quick`, recording the run it re-checks; `--propose` scales to the run's recorded depth:

```bash
tomlctl review run start --slug <slug> --command review-plan-v2 --family plan --depth quick --base HEAD --set-json 'scope=@<scratch>/review-scope.json' --set rechecks=<run> --set-json models=@<scratch>/review-models.json --set substrate=<workflow|agent-waves>
```

`substrate` follows the engine's rule, as at section 2.

Bind its `<recheck-run>` and `<recheck-dir>`.

**The pack.** A re-check runs no survey, so its directory holds no `pack.md` for the finders, and the launch below requires both copies to be in place first. The wrapper cannot carry them over: it has no shell, and passing the first run's pack path would move the pipeline's run directory, which it takes from that path's parent, into the first run. Copy them without reading them:

```bash
cp <run-dir>/pack.md <recheck-dir>/pack.md
cp <run-dir>/profile.md <recheck-dir>/profile.md
```

Its fences describe the plan before the edits, which the re-check's finders read against the records as they now stand.

**Launch.** Fresh, never a `resumeFromRunId` relaunch: the first launch's journal would replay agents over edited records.

```text
Workflow({
  name: "plan-v2-review",
  args: {
    slug: "<slug>",
    run: "<recheck-run>",
    mode: "recheck",
    depth: "quick",
    run_dir: "<recheck-dir>",
    ledger_line: "ledger: .claude/flows/<slug>/review-v2.toml",
    signals: {},
    targets: ["task:<ref>.spec.accept[1]", "brief#Background"],
    schemas: { cell_return: <cell-return schema>, verdict_return: <verdict-return schema> }
  }
})
```

`targets` lists each distinct `target` of section 7's `applied` and `already` rows, one per entry, as stored. A recheck takes depth `quick` and a non-empty `targets`; the wrapper refuses anything else and launches nothing. `/plan-v2`'s step row stays the one section 4 opened.

**On the return.** Read it by section 5, with the same fallback table, then mop up by section 6 against `<recheck-run>`. The re-check runs no follow-up wave except the one `review-finder` probe cell that collects its sweepers' escalations, as `compose.md` allows at `quick`.

**Its findings are never applied automatically**, whatever their tier:

- a user-tier finding is asked before approval, as in section 7, and an accepted one applied with `--user-accepted`. Re-probe by section 8 when it edits a `spec.accept` entry. A question-tier finding is asked and resolved as in section 7;
- every other open finding is left `open`. The gate lists each open re-check finding under `risks`, as a leftover the user approves past or acts on.

Then close `<recheck-run>` by section 9.

## 11. Finish

`/plan-v2` closes its step row:

```bash
tomlctl plan run finish --slug <slug> --step review --outcome <outcome>
```

| The review | `--outcome` |
|---|---|
| Both runs finished clean, with no fallback | `ok` |
| A fallback fired, or a run finished `--incomplete` | `errors` |
| The user stopped the review | `stopped` |

Then `/plan-v2` goes on to its gate, which shows `review_edits` and `risks`, and approves with `--review ran`. `plan approve` refuses while a plan-family run is unfinished, a user-tier or question-tier finding is open, or an edited `spec.accept` entry has no probe; `plan stage` applies the same rule, so `next` names what is left.

**Close summary.** Print:

- each run id with its report path and counts line;
- the edits by tier: mechanical and cited applied, user accepted and vetoed, questions revised and kept;
- the refused, stale and unprobed ids;
- `user tier pending` when questions went unanswered;
- the re-check's yield, its open findings and any user-tier findings it asked;
- the backlog ids;
- each unhandled escalation the lead wrote to `<run-dir>/payloads/escalations-w*.md`, by cell and text, each offered as a backlog capture; capture only the ones the user accepts, through `backlog-capture`'s check-then-add gate, and the rest are dropped with the run;
- a fallback, if one fired;
- `map not confirmed` at `deep`.

Name the outcome read for this plan: `tomlctl review stats --slug <slug> --plan-outcomes`.

## 12. An approved flow

When `plan.toml` holds an `[[approval]]` (stage `approved`, `running` or `done`), intent is frozen. Every edit is the user's, whatever its tier, and `review apply` refuses any call without `--user-accepted`.

- Section 7's mechanical and cited rows are asked like user-tier rows, in the same `AskUserQuestion` batches. The question names the tier, so the user sees which edits the tool would have made alone. Accepted ids are applied with `--user-accepted`; vetoed ones are dismissed. `review stats --plan-outcomes` counts every dismissed tiered row other than `advisory` as a veto once its run started after the first approval; before it, only a dismissed user-tier row is one.
- Task rows, specs and milestones are written through the audited path on an approved plan, so each edit also leaves a `[[changes]]` row.
- An applied edit to `brief.md` changes the brief the approval recorded. `plan stage` then reports `plan/brief-changed`, and its `next` names `plan approve --amend --review ran`. Tell the user the amend is theirs to run.
- An edit to a `spec.accept` entry is re-probed by section 8 as before.

`/plan-v2` does not reach this section, since its Review step runs before approval. `/review-plan-v2` reaches it on any flow it reviews after approval.

## 13. Re-entry

A `review` row that `plan stage` lists under `open_runs` is settled by the store, not by relaunching the wrapper:

- **Live in this session.** Its notification has not arrived. Wait for it.
- **From a dead session.** Read the flow's plan-family review runs:

  ```bash
  tomlctl review run list --slug <slug> --open --where family=plan --lines
  ```

  Take the newest run it lists, the last row, bind it as `<run>` and `.review-v2-runs/<slug>/<run>/` at the repo root as `<run-dir>`, and resume it at the first incomplete stage by section 5's table. A run already at triage resumes at section 7. Then continue, and close the step row by section 11.

Abandon the row only when the user drops the review: `tomlctl plan run abandon --slug <slug> --step review --reason '<why>'`. An abandoned review run does not settle the review stage, so the plan then needs a fresh review or `plan run skip --step review` with a reason.
