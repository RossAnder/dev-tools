---
name: review-v2-engine
description: "Orchestrator contract for /review-v2, /optimise-v2 and /review-plan-v2 — the run stages, steering flags, depth envelopes, the Workflow launch with its Agent-wave fallback, triage and close. Invoke from a v2 carrier before the first `tomlctl review` call."
---

# Review v2 engine

This is the orchestrator's half of a v2 review run. The three carriers (`/review-v2` for the `code` family, `/optimise-v2` for `perf`, `/review-plan-v2` for `plan`) invoke it and add only their family deltas. The agents' half — finding kinds, the store-write idiom, return objects — is the `review-v2-method` skill, which every v2 agent preloads; you never restate it in a prompt.

## Your role

Work goes to the cheapest executor that can do it reliably: deterministic `tomlctl` first, then Haiku (sweeps and checks grep can settle, judged-signal proposals), then Opus (judgement findings, refute-first verification, plan dry runs), and you last. What is left for you is judgement:

- **compose** the coverage map: accept judged signals, edit the proposal, author probes, write waivers;
- **adjudicate**: decide follow-up and mop-up waves, audit a sample of verdicts;
- **triage** with the user, and close.

Four rules keep your context for those decisions:

1. **Read digests and projections, never rows.** Your reads are `review plan --propose` output, `review digest`, the `--for orchestrator` and `--for audit` projections, the surveyor's digest and `profile.md`. Plan-family triage may read findings with `--for implementer`, since the orchestrator projection omits the proposed edit's `expect` and `new`. Never open the store file, never read lens, facet or method bodies, and never read `pack.md`.
2. **Never re-type a finding.** Agents write the store themselves. Refer to findings by id.
3. **Never paste constraints, rows or catalogue text into a prompt.** A prompt is the assignment template in `references/agent-waves.md`; agents fetch the rest from the store and the pack.
4. **Never load the `review-v2-lenses` skill.** `tomlctl` resolves the catalogue path, binds each concern's content into the cell, and hands agents the absolute paths. Load the `backlog-capture` skill only at stage 8, and only when the user picked an item for the backlog.

## The store and the dispatch preamble

Every `tomlctl review` verb except `catalogue` and `schema` takes exactly one store flag:

- a flow is bound → `--slug <slug>`, store `.claude/flows/<slug>/review-v2.toml`;
- no flow → `--scope <scope>`, store `.claude/review-v2/<scope>.toml`, with `<scope>` derived as v1 derives it (path argument, feature name, branch, or `recent`).

The examples in this skill and its references use `--slug <slug>`; pass `--scope <scope>` in its place for a flow-less store. The store is auto-created by the first write.

**The `ledger:` line.** Every dispatch of a flow-bound run — surveyor, `verification`, cell, verifier, checker — opens its prompt with this line at column 0, with nothing before it:

```text
ledger: .claude/flows/<slug>/review-v2.toml
```

Agent correlation reads that line to attribute the agent to the run's store. A flow-less run omits it. The rest of the preamble and each prompt template are in `references/agent-waves.md`.

**Dispatches** are one-shot: `Agent({subagent_type, description, prompt, run_in_background: true})`, never with `name:` (a named spawn has no return channel) and never with `model:` (model and effort live in each agent's frontmatter). At most 16 agents run at once, counting the surveyor, every `verification` agent and any agent of an earlier wave still running.

## Steering flags

The same on all three carriers:

- `--depth quick|standard|deep` — the depth envelope below; default `standard`.
- `--flow <slug>` — bind this flow; passed to the envelope build as `--flow-override <slug>`.
- `--probe "<question>"` — repeatable; each becomes a `probe:<slug>` cell at stage 3.
- `--lens +id` / `--lens -id` — repeatable; forces a lens into or out of the proposal, passed straight through to `review plan --propose`.

Anything else in the arguments is scope: paths, globs, or a feature name.

## Stages

| # | Stage | Executor | Reference |
|---|---|---|---|
| 0 | Pre-flight | orchestrator + tomlctl | below |
| 1 | Run start | tomlctl | below |
| 2 | Survey | tomlctl, review-surveyor (Haiku), `verification` agents | `references/survey.md` |
| 3 | Compose | orchestrator | `references/compose.md` |
| 4 | Wave | Workflow `review-v2-pipeline`, or Agent waves | below; `references/agent-waves.md` |
| 5 | Resolve and digest | tomlctl → orchestrator | below; `references/compose.md` |
| 6 | Audit | orchestrator | `references/triage-and-close.md` |
| 7 | Triage | orchestrator + user | `references/triage-and-close.md` |
| 8 | Close | tomlctl, plus the orchestrator's backlog writes | `references/triage-and-close.md` |

### Stage 0 — Pre-flight

1. Invoke the `tomlctl` skill before the first `tomlctl` call. Its guidance canon holds here: tomlctl output is never piped into a text tool (`--select`, `--get`, `--limit` and `--max-chars` trim it at the source), never redirected with `2>/dev/null`, and payloads are staged as files rather than fed through heredocs.
2. Check the installed binary carries the review group:

   ```bash
   tomlctl capabilities
   ```

   Halt unless `.features` lists `review_add`, `review_survey`, `review_catalogue`, `review_run_head`, `review_run_show`, `review_ruled_out`, `review_same_as_link`, `review_blocked_on`, `review_settle`, `review_triage_answer`, `review_schedule`, `review_reconcile`, `review_anchor`, `review_reproduce`, `review_stats_apply` and `review_digest_with`, telling the user to run `cargo install --path tomlctl` in the dev-tools checkout and restart.
3. Invoke the `flow-contract-flow-context` skill and build the envelope as it says, with the **v1 command name**: `--command review` for `/review-v2`, `--command optimise` for `/optimise-v2`, `--command review-plan` for `/review-plan-v2`. The envelope builder knows only the v1 names. Gate on `envelope.ok`, emit the bootstrap-summary line, and bind the store flag from `envelope.resolved.slug`, or derive `<scope>` when no flow resolved. A v2 run skips the user-inputs sweep.
4. Read back the v1 outcomes of findings an earlier run exported to a v1 ledger, for stores that still hold `exported` findings, so a re-found fixed issue resolves as a regression rather than a repeat. It is a legacy read-back for v1 rows whose `v2_ref` names this store, since triage hands findings to `/apply-v2` rather than to a v1 ledger:

   ```bash
   tomlctl review sync-v1 --slug <slug>
   ```

5. List what earlier runs left without a landing place — parked advisory, and open findings still blocked, grouped by what they are blocked on — and report both in the run's opening lines, so the user sees them before new findings arrive:

   ```bash
   tomlctl review list --slug <slug> --status parked
   tomlctl review list --slug <slug> --status open --blocked-on any
   ```

6. **Plan family only.** Invoke the `flow-contract-task-store` skill and run its auto-import at Step 0 with the ref-set diff gate (its sections 9 and 10), exactly as an orchestrator does there. The import is yours; agents only ever run `tasks import-plan --dry-run`. Skip it on a v2 flow, one with `.claude/flows/<slug>/plan.toml`: its store is already canonical and `tasks import-plan` refuses it. That run is in flow mode, and `references/plan-flow.md` takes over from here.

### Stage 1 — Run start

Resolve the run's inputs:

- **Bounds and scope** — a run is measured from `base` to `head`, and its scope is a list of repo-relative, `/`-separated paths or globs. Refuse `..`, absolute paths outside the worktree, and any glob matching more than 200 files. Choose by the first case that holds:
  - *A flow-bound run whose task rows carry `commit` stamps* (`tomlctl tasks list --slug <slug> --select id,commit --lines`): the commit list is the distinct stamps. `head` is the stamp that descends from every other; `base` is the first parent (`<stamp>^`) of the stamp every other descends from. Order them with `git merge-base --is-ancestor`, never by committer date, which ties on rebased work, and ask the user when the stamps are not on one ancestry line. The scope is the union of the files those commits touch (`git show --name-only --format= <stamp>` per stamp), not the files changed across `base..head`: an interleaved commit from another flow contributes no file.
  - *A flow-less run, or a flow with no stamps*: choose `--base`, `--head` and the scope from the user's argument — a range, a commit list, paths — and state the choice in the run's opening line. With no argument on a feature branch, `base` is the merge-base with the default branch, `head` is omitted (the run measures to `HEAD`), and the scope is the files that diff changes; on the default branch with no argument, ask the user where the change starts.
  - *A path review of unchanged code*: `base` is `HEAD` and `head` is omitted; `changed_lines` then falls back to the scope's tracked line count.
  - *Plan family*: `base` is `HEAD`; the scope is the plan path in markdown mode and, in flow mode, `docs/flows/<slug>/brief.md` plus `docs/flows/<slug>/design/*.md`.

  Write the scope list with the Write tool to a JSON file in a scratch directory outside the repository and pass it as `--set-json 'scope=@<path>'`. Agents read the working tree whatever the bounds are, so when `head` is behind `HEAD`, name in the run's opening line every scope file a later commit changed (`git diff --name-only <head> HEAD -- <scope entries>`).

  **Ripple reaches past the scope.** The scope bounds what cells read as changed, not where a finding may be anchored: a `ripple` cell greps the whole tree for names the diff removed or renamed, and a stale reference in an untouched file is a finding on that file.
- **Models** — read the `model:` line of each of the seven v2 agents (`review-surveyor`, `review-sweeper`, `review-checker`, `review-finder`, `review-finder-cf`, `review-verifier`, `plan-executor`) under `<claude dir>/agents/` with Grep. Never assume them.
- **Substrate** — `workflow` when the Workflow tool is available in this session, otherwise `agent-waves`.

```bash
tomlctl review run start --slug <slug> --command review-v2 --family code --depth standard --base <base-sha> --head <head-sha> --set-json 'scope=@<scratch>/scope.json' --set-json 'models={"surveyor":"haiku","sweeper":"haiku","checker":"haiku","finder":"opus","finder-cf":"opus","verifier":"opus","executor":"opus"}' --set substrate=workflow
```

Omit `--head` when the run measures to `HEAD`. It mints the run (`U<n>`), resolves `--base` and `--head` to shas, computes `changed_lines` over `base..head`, records the catalogue roots and shas, and validates the merged catalogue. A validation error halts the run: report the offending file and stop, because a project-layer error is the project's to fix. It also refuses when `.claude/review-lenses` in the working tree differs from `--base` — committed after `base`, `--head` included, or uncommitted — since the layer it loads is the working tree's and can carry check commands, so the change would configure its own review: show the user the listed paths and pass `--project .claude/review-lenses` only on their say-so. A run started that way records the paths as `catalogue.project_layer_changed`; name them to the user in the close summary. It prints `{run, dir, base, head, changed_lines}`, `head` being `null` without `--head`. Bind `<run>` and `<run-dir>` (the printed `dir`); `pack.md` and `profile.md` are written there, and every payload you stage goes in `<run-dir>/payloads/`.

### Stage 2 — Survey

`review survey` records the deterministic signals; one review-surveyor proposes judged signals, runs the surveyor checks, resolves the baseline commands and writes `profile.md` and `pack.md`; then one `verification` dispatch carries every resolved baseline command with `keep_going: true`, each with its own `timeout`, and every baseline key is recorded from that one return. Read the run's signals and recorded baseline keys back with `tomlctl review run show <run> --slug <slug>` rather than copying them into a prompt. Follow `references/survey.md`. Stage 3 may start once the surveyor's digest is in; **stage 4 waits until every resolved baseline has a recorded entry**.

### Stage 3 — Compose

Accept or reject the judged candidates, propose the default map, edit it (re-slice, re-bundle, probes, facet edits, a reason for every edited row), write it, and record a waiver for every unselected matched concern and unsliced scope file the digest lists. At `deep`, confirm the map with the user before writing it. State the expected agent count, and say why when it exceeds 25. Follow `references/compose.md`.

```bash
tomlctl review plan --run <run> --slug <slug> --propose --accept-signals trust-boundary --out <run-dir>/proposal.ndjson
tomlctl review plan --run <run> --slug <slug> --ndjson <run-dir>/payloads/cells-w1.ndjson
```

The first writes the proposal into the run directory for you to read and edit. The second appends the edited cells as the wave, binding each concern's catalogue content; each later wave appends through the same call with its own cell file.

### Stage 4 — Wave

Fetch the two return schemas once per run:

```bash
tomlctl review schema cell-return
tomlctl review schema verdict-return
```

Launch the wave on the saved Workflow:

```text
Workflow({
  name: "review-v2-pipeline",
  args: {
    run: "<run>",
    store: { slug: "<slug>" },
    ledger_line: "ledger: .claude/flows/<slug>/review-v2.toml",
    wave: <wave>,
    depth: "<depth>",
    pack_path: "<run-dir>/pack.md",
    mechanical_lenses: ["hygiene", "ripple"],
    cells: [ { id: "C1", agentType: "review-finder" } ],
    schemas: { cell_return: <cell-return schema>, verdict_return: <verdict-return schema> }
  }
})
```

- `store` is `{ scope: "<scope>" }` for a flow-less store, and `ledger_line` is then omitted.
- `cells` lists this wave's cells only, each with the `agent` the composed map gave it.
- `mechanical_lenses` is the ids of the catalogue's `mode = "mechanical"` lenses that are not disabled, read once per run with `tomlctl review catalogue --family <family> --rows entries --where type=lens --where mode=mechanical --where disabled=false --select id --lines`; a checker may refute a cleanup row a sweeper did not write only when its lens is one of them.
- After the launch reports `async_launched`, end your turn. Resume on the task notification.

The Workflow runs one agent per cell, then routes each returned finding by the `route` `review add` computed: one `review-verifier` per finding (three seats for `panel`); one verifier batch per cell for minor defects at `quick` and `standard`, and one per cell for verifier-routed cleanup rows at every depth; and pooled `review-checker` batches of up to 20 cleanup rows, with a checker's `unsettled` rows re-routed to a verifier. It returns only the index `{run, wave, cells: [{id, status, findings, escalated}], verified, agents, errors, script_version, escalations}`; the findings are already in the store. `agents` counts the Workflow's own agent launches; `escalations` is `[{cell, text}]`, each text cut to 300 characters, and sits last so a cut result loses escalation text before any cell's status. The conversation sees at most 8,000 characters of a Workflow result: when the result says it was truncated, read the notification's output file for the whole index.

**Fallback.** Run the wave as Agent waves per `references/agent-waves.md` when the Workflow tool is disabled or absent, the name is unknown, the launch reports `error`, or the run ends with no result or with an index showing no cell launched. A Workflow that completed with some cells `no-return` or `error` is not a fallback case: go to stage 5 and let the mop-up cover them. Once the fallback fires, the rest of the run stays on it; `review run update` does not take `substrate`, so record the switch in the close summary as `agent-waves.md` describes.

**The agent total.** The run's `agents` is the sum of every wave index's `agents`, whichever substrate ran the wave, plus every agent you dispatch for the run outside a wave index: `flow-bootstrap`, the surveyor, the baseline `verification` dispatch, each mop-up dispatch, each rule-out re-verification, and any verifier stage 7 dispatches after a convert. An Agent-wave fallback keeps its own wave index, so its agents arrive through that index. Never add a wave twice, and never count an agent an index already counts. Keep the total as you go, and record it once, immediately before `review run finish` (stages 6 to 8).

### Stage 5 — Resolve and digest

```bash
tomlctl review resolve --run <run> --slug <slug> --wave <wave>
tomlctl review digest --run <run> --slug <slug> --since-wave <wave>
```

`resolve` settles every finding whose `same_as` has complete verdicts (repeat, contested, reopened, regression, suppressed). The digest then gives counts by kind and lens, cells by status with notes for `thin` and `uncovered`, unverified ids with their routes, unselected concerns and unsliced files still needing a waiver, contested findings, refuted criticals, the counts of `duplicate_candidates` and `ruled_out_hits`, and the advisory list.

Read three more lists before deciding. When either digest count is above zero, read both arrays in full with one more digest call:

```bash
tomlctl review digest --run <run> --slug <slug> --since-wave <wave> --with duplicates,ruled-out
```

- **`escalations`**, from the wave index: each is a follow-up cell candidate. One you do not turn into a cell — out of budget, or anchored outside the scope and not a ripple row — is named in the close summary and offered as a backlog capture.
- **`duplicate_candidates`**, from the digest: a pair of places that match across cells is a question, never a fold. Link each pair you judge one defect, every pair in one call, and leave a pair you judge distinct alone:

  ```bash
  tomlctl review resolve --run <run> --slug <slug> --same-as F12=F7 --same-as F15=F9
  ```

- **`ruled_out_hits`**, from the digest: a finding at a place another cell ruled deliberate. Check that the verifier's rationale answers the rule-out, and re-dispatch one verifier where it does not, as `references/compose.md` sets out.

Then decide, in this order:

1. **Mop-up** — any unverified id gets a mop-up dispatch from the store, per `references/agent-waves.md` (Mop-up). Repeat until no unverified id is left or an id fails twice.
2. **Follow-up** — new cells for `uncovered` and `thin` cells, escalated sweeper candidates, or a gap the findings revealed, within the depth envelope's follow-up allowance and cell ceiling. The criteria are in `references/compose.md` (Follow-up and mop-up). A follow-up goes back through `review plan` as the next wave, then stage 4.
3. Otherwise, go to stage 6.

### Stages 6 to 8 — Audit, triage, close

Audit the digest's sample through the `audit` projection; write the report and present the counts, the blocked findings grouped by `blocked_on`, the deferred proposals with their age, contested items and the advisory list with a recommendation per item; ask every decision in one round and record each answer; take the user's dispositions, an overdue proposal's included; park untriaged advisory, hand user-picked items to the backlog through the `backlog-capture` skill, name `/apply-v2` with the store and the ids the user picked under `apply`, and finish the run. Follow `references/triage-and-close.md`. Triage is a user gate: an instruction to proceed autonomously does not cover it.

Record the agent total, as stage 4 defines it, then finish:

```bash
tomlctl review run update <run> --slug <slug> --set-json agents=23
tomlctl review run finish <run> --slug <slug>
```

`run finish` refuses while any finding is unverified, any `same_as` is unresolved, any unselected concern or unsliced file lacks a waiver, or any cell is unsealed.

## Depth envelopes

`--depth` sets the run's budget; the default is `standard`.

| | quick | standard | deep |
|---|---|---|---|
| cell ceiling, all waves (`--propose` scales it to about 1 cell per 100 changed lines, floor 3) | 6 | 16 | 32 |
| concerns per cell | ≤3 | ≤3 | ≤2 |
| follow-up waves | 0 | 1 | 2 |
| defect verification | one verifier per critical or major defect (the plan family batches them, see below); one batch verifier per cell for minor | as quick | one verifier per non-critical defect; a critical goes to a 3-vote panel instead, and survives on ≥2 non-refuted votes |
| cleanup check | pooled Haiku checker batches; code-touching and test-removal rows go to one Opus verifier batch per cell, unsettled checker rows to an Opus verifier each | as quick | as quick |
| repeat passes | none | floor and hotspot cells once more, files in reverse order | every high-risk cell once more |
| plan-executor cells | ≤4 tasks | ≤12 tasks, L and M first, checkpoint-marker tasks first | every task |
| confirm the map before launch | no | no | yes |

**Plan family batching.** Under the `plan-v2-review` Workflow the pipeline runs with `batch_defects`: a cell's verifier-routed defects that would each get a single verifier go to one batch when there are two or more, and a row carrying `same_as` stays single so `--strike-same-as` still works on its verdict. Reverify follows the same split: the ids the lead lists under `reverify_single`, those carrying a `same_as`, get a verifier each, and the rest of its `reverify` list go in one batch. The code and perf families keep the per-finding rule in the table.

The ceiling counts every wave's cells, repeat cells included; plan-executor cells sit outside it. At `quick`, escalated sweeper candidates still get one Opus cell (see `references/compose.md`).

## Family deltas

The carriers own these; the engine leaves room for them at the stage named.

- `/review-v2` (code): the `LINT BASELINE`, the context-free correctness cell at stage 3, and the `/apply-v2` hand-off.
- `/optimise-v2` (perf): a Focal Points Brief only for what the bound facets do not carry, the `PROFILE BASELINE`, and the `/apply-v2` hand-off.
- `/review-plan-v2` (plan): plan resolution, the stage 0 task-store import, `STORE CHECK`, `FORMAT CONTRACT` and the `BREAK-SET BASELINE`, the `plan-checks` cell and `plan-executor` cells, and the merge offer in place of the apply hand-off. That is markdown mode. On a v2 flow (flow mode), `/review-plan-v2` and `/plan-v2`'s Review step follow `references/plan-flow.md` instead: the `plan-v2-review` Workflow in place of stages 2 to 6, then tiered apply, re-probes and a re-check run in place of the merge offer.

## References

- `references/survey.md` — stage 2: detectors, the surveyor dispatch, judged candidates, checks by executor, pack fences, the baseline dispatches and the stage 3→4 barrier.
- `references/compose.md` — stage 3 and the stage 5 decisions: candidates, the proposal, re-slicing, bundling, probes, facet edits, steering flags, waivers, the deep confirm step, the agent-count statement, follow-up and mop-up criteria.
- `references/agent-waves.md` — the stage 4 fallback, the surveyor prompt, every prompt template, routing, the wave index and the mop-up dispatch.
- `references/triage-and-close.md` — stages 6 to 8.
- `references/plan-flow.md` — the plan family on a v2 flow: run start, the `plan-v2-review` launch and its fallback, mop-up, triage by tier through `review apply`, re-probes, the re-check run, finish, the approved-flow rule and re-entry.
- `references/measurement.md` — the outcome signals, the v1 baselines and the thresholds that govern retuning.
