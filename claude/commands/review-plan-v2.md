---
description: Review an implementation plan with composed, verified coverage — the v2 plan family, findings in the v2 store, and a merge offer into the plan
argument-hint: [plan path or directory] [--depth quick|standard|deep] [--flow <slug>] [--probe "<question>"] [--lens +id|-id]
---

# /review-plan-v2 — plan review, v2

Reviews an implementation plan against the codebase with coverage composed for this plan: floor plan lenses, a dry run per task, the whole-plan `plan-checks` cell, and whatever lenses and probes the plan's signals call for. Every defect and cleanup finding is verified before it is offered back; verified findings can then be merged into the plan.

Invoke the `review-v2-engine` skill before anything else and run its stages in order. It owns the pipeline: pre-flight, run start, survey, compose, waves, resolve and digest, audit, triage and close, the steering flags and the depth envelopes. This file states only what the plan family adds at each stage; where it says nothing, the engine's text stands.

## Bindings

| Binding | Value |
|---|---|
| family | `plan` |
| `review run start --command` | `review-plan-v2` |
| envelope `--command` | `review-plan` |
| base | `HEAD` |
| scope | the plan documents bound at stage 0 |
| depth | `--depth`, default `standard` |

## Stage 0 — pre-flight

Invoke the `tomlctl` skill and the `flow-contract-flow-context` skill as the engine's stage 0 directs, and build the envelope with `--command review-plan`. Then, in this order, after the envelope is gated: steps 1 and 2 below, the engine's `review sync-v1`, and step 3, which is the engine's plan-family import.

**1. Plans directory.** When `envelope.plans_directory` is null, invoke the `flow-contract-plansdirectory-prompt` skill and run its first-use prompt. Its in-memory bind is the plans directory for step 2.

**2. Plan resolution.** Bind `<plan>` before the task store is touched, because the store's skip rule compares it with the flow's plan:

1. an argument naming a file — that file;
2. an argument naming a directory — a multi-file plan: read every markdown file in it and classify each by role (outline or master, numbered detail documents, progress or status, supporting). The outline is `<plan>`; the scope lists every document;
3. no path argument — the plan produced earlier in this conversation; then the resolved flow's `plan_path` (read it with `tomlctl get <context_path> plan_path`, and flag it when `envelope.resolved.stale` is true); then the most recently modified file in the plans directory, asking when several are equally recent; then ask the user.

Note the plan's age from its header or git history. Triage reports it, flagged past 14 days.

**3. Task store.** Invoke the `flow-contract-task-store` skill and import before anything reads a row status. Re-running is idempotent, and later runs import too, since the execution record can hold completions the store has not seen:

```bash
tomlctl tasks import-plan --slug <slug> --reconcile-record
```

Skip the import in three cases and name the case that fired in the triage presentation:

- **no flow** — the run is flow-less, so nothing owns a store;
- **a `<parent>-tdd-<NNN>` sub-flow** — the parent flow owns the plan and its store;
- **`<plan>` is not the flow's `plan_path`** — a real import would write another document over every row. The surveyor's `STORE CHECK` still dry-runs `<plan>`, but its `tasks check` half then describes the flow's own plan.

An error-class finding is review material here, never a halt. When the real import refuses, on a `dag/cycle`, a `checkpoint/invalid-cut` or a `dag/duplicate-number`, leave the store as it stands and continue. The surveyor's dry run carries those findings into the `STORE CHECK` fence, and the cells judge them.

## Stage 2 — survey

- The surveyor prompt carries the `plan:` line, set to `<plan>`.
- The plan family's surveyor checks are `store-check` (the `STORE CHECK` fence: `tasks import-plan --dry-run` and `tasks check --plan`) and `format-contract` (the `FORMAT CONTRACT` fence: house format or foreign). Read their `checks:` lines in the digest; a `not run` there goes into the triage presentation.
- `break-set-baseline` resolves one command per predicted break set in the plan. The digest lists them as `verification:` lines keyed `break-set-baseline`, `break-set-baseline-2` and so on, in plan order. Dispatch the `verification` agent once per line, one at a time, and record each under its own key, as the engine's survey reference says. Project checks whose `families` include `plan` follow them.
- Pass the plan's own `transient:` keys from its `## Verification Commands` as the dispatches' `transient:` patterns.
- Run no break-set command yourself. The baseline observes today's tree; judging each prediction's preconditions against it is the `plan-checks` cell's work.

## Stage 3 — compose

- **The floor.** `plan-structure`, `plan-grounding`, `plan-dependencies` and `plan-executor` are proposed on every plan run. Keep them; a floor lens is never dropped or waived.
- **The `plan-checks` cell.** The proposal emits one `review-finder` cell whose concerns are the three cell-run checks, `check:two-control-probe`, `check:falsifiability` and `check:acceptance-contradiction`. Write it to the cell file exactly as proposed: same concerns, slice, agent and reason. It takes the place of v1's orchestrator-only checks, so you run no acceptance probe, no falsifiability scan and no cross-task contradiction scan yourself, and its defects are verified like any other.
- **`plan-executor` cells.** Each holds up to four task refs, outside the cell ceiling. Edit only which tasks they cover, within the depth envelope's plan-executor row.

## Stage 7 — triage

Open the presentation with the plan, its age, and an overall assessment (`Ready to execute`, `Needs revision` or `Major gaps`), then the engine's items. Add a line for any skipped task store or surveyor check that did not run.

The grammar's `export` does not apply to this family. Verified findings go back into the plan through the merge offer at stage 8, and the presentation says so.

## Stage 8 — close

The engine's close runs with two changes: the merge offer below takes the place of export (step 3), and there is no apply hand-off (step 4). Parking the untriaged advisory and the backlog picks run first, as the engine says; when the user picked an item for the backlog, invoke the `backlog-capture` skill there. Advisory is never merged into the plan.

### The merge offer

**1. Eligible findings.** Defect and cleanup rows at status `open` whose `file` is a plan document of this run. A row is `open` only after a verifier, checker or audit returned CONFIRMED or PLAUSIBLE, so nothing unverified is offered. Read them through the implementer projection, which carries `plan_section`, `anchor_old`, `anchor_new` and the fix or edit:

```bash
tomlctl review list --slug <slug> --kind defect --status open --in <plan> --for implementer
tomlctl review list --slug <slug> --kind cleanup --status open --in <plan> --for implementer
```

With none, print `No verified findings to merge.` and go on to finish the run.

**2. Ask both questions in one `AskUserQuestion` call.**

- **Q1 "Findings"** — multi-select over `Critical`, `Major`, `Minor` and `Cleanup`, default `Critical` and `Major`.
- **Q2 "Merge mode"** — single-select:
  - **Manual merge + accept** *(recommended)* — a judgement merge of every selected finding, anchored or not, written over the plan.
  - **Mechanical merge → review** — anchor-based and conflict-safe, written to a `.revised.md` sibling for a later Accept, Keep both or Discard.

An empty answer to either question, or no finding selected, merges nothing: the findings stay `open` in the store, and the report and the close summary say `merge skipped — re-run interactively to merge`. Manual mode overwrites the plan, so it runs only on an explicit interactive selection. v2 has no user-inputs store, so no question record is posted.

**3. Merge.** Read Step 4 of the v1 `/review-plan` command, the `review-plan.md` beside this file in the commands directory, and follow Step 4A or 4B with the substitutions below. Before writing any merged text, invoke the `flow-contract-plan-output-format` skill, whose Files-line closure rule every merge must keep.

- **Manual (4A).** The judgement merge over every selected finding, using `anchor_new` where present and the `fix` or `edit` as intent where not; the `<plan>.premerge.md` backup with its one-generation retention; then A3.5 in full against the written file — the ref-set gate dry run first, then the real import and `tasks check`, then Files-line closure for every task body the merge touched. With the store skipped at stage 0, derive (a) and (b) by hand and say so.
- **Mechanical (4B).** Only findings carrying both `anchor_old` and `anchor_new`. B2 skips every `plan_section` holding more than one of them; B3 replaces an `anchor_old` only when it occurs exactly once under its section; B4 writes `<plan>.revised.md` and dry-runs the re-derivation against it. At B7, Q3 defaults to **Keep both**, and an empty answer is Keep both.
- **Findings in place of P-ids.** Read v1's `P{n}` as this run's `F<n>`, its `description` as the row's `fix` or `edit`, and its severity selection as Q1's.
- **Status writes.** Each v1 `tomlctl items apply` on `plan-review-findings.toml` becomes a `review triage` call with the text staged by the Write tool in `<run-dir>/payloads/`:
  - a merged finding (A4, or B8 Accept) is resolved with the staged text `merged into <plan>`, the plan path written out in full;
  - a Discard dismisses the B3 findings with the staged reason `merge discarded: <plan>.revised.md`;
  - Keep both writes nothing.

  ```bash
  tomlctl review triage F3,F7 --slug <slug> --resolve-file <run-dir>/payloads/merged.md --expect-status open
  tomlctl review triage F3,F7 --slug <slug> --dismiss-file <run-dir>/payloads/merge-discarded.md --expect-status open
  ```

  Report `skipped_stale` ids under a `changed during the run` line, and never retry them.
- **Not carried over.** v1's Step 0.7 held answers, its question records, its round counter and its re-run dedup. The next run reads merged findings through `PRIOR`: the `merged into <plan>` resolution marks each section a merge rewrote.

**4. Summary.** Print the 4A or 4B console summary in the close summary. When the merge changed a **Files** line, a **Depends on** line or the task count, end it with `merged text is unreviewed — re-run /review-plan-v2 for round 2`.

Then finish the run as the engine says. `review run finish` refuses while an id is unverified, so finish after the merge's status writes, never before.
