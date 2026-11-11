# Triage and close — stages 6 to 8

After the last wave has resolved and its digest shows no unverified id, the orchestrator audits a sample of verdicts, presents the run to the user, applies the user's dispositions, and closes the run. Pass `--scope <scope>` in place of `--slug <slug>` for a flow-less store in every command below.

Stage the user's prose and your own rationales with the Write tool, in the run directory's `payloads/` subdirectory, and pass them through the `--<flag>-file` twins (`--dismiss-file`, `--defer-file`, `--trigger-file`, `--reopen-file`, `--park-file`, `--resolve-file`, `--addressed-file`, `--answer-file`, `--fix-file`, `--rationale-file`, `--incomplete-file`). Never quote user text into a shell argument.

## Stage 6 — Audit

The audit is the outside check on the verifiers and checkers; its overturns are what calibration learns from, so it is never skipped and never widened into a re-review.

**The sample.** Read it from the final digest:

```bash
tomlctl review digest --run <run> --slug <slug>
```

- every REFUTED critical defect;
- every contested finding;
- the digest's `audit_sample`, chosen by position in store order: two standing defects (latest non-audit verdict CONFIRMED or PLAUSIBLE), two REFUTED non-critical defects, and two checker-decided cleanup rows, one of them from a sweeper cell when there is one. Each REFUTED slot the run cannot fill takes one more standing defect by the same rule, so a run with no refutations still samples four standing defects.

Take the indexed rows exactly as the digest lists them. Choosing your own replacements turns a sample into a selection.

**The projection.** Read the whole sample in one call:

```bash
tomlctl review show <ids, comma-separated> --slug <slug> --for audit
```

The audit projection is the verifier projection plus every verdict on each row and the five lines either side of each cited `file:line`, so you adjudicate without opening files. Open a file only when the excerpt cannot settle the row, and then only at the cited range.

**Judging a row.** Ask one question per row: does the recorded verdict follow from its citation?

- A CONFIRMED defect is overturned when its citation does not show the `failure_scenario` happening, or shows documented intent the verifier missed.
- A REFUTED defect is overturned when its citation does not defeat the `failure_scenario` — the refutation rests on an uncited claim, the wrong line, or a path the trigger never takes.
- A checked cleanup row is overturned when its `old` text is not at the anchor, the edit leaves `file`, `target_file` and `instances`, or a symbol called dead is referenced.
- A contested finding is settled for whichever verdict its citations support.

**Recording.** Every sampled row gets an audit verdict, upheld or overturned, so `review stats` can measure the verifiers against a denominator. The verdict is your call on the finding itself: `confirmed` when it stands, `refuted` when it does not. Upholding a CONFIRMED or PLAUSIBLE defect, or a checker's confirm, is `confirmed`; upholding a refutation is `refuted`; an overturn is the opposite call. Use `confirmed` or `refuted` only. An audit `plausible` reads as a confirm in the overturn count while saying the audit could not decide, so the count would be measured against a call nobody made; when the excerpt and the cited range leave a row undecided, it is upheld only if its recorded verdict follows from its citation, and overturned otherwise.

Write the whole sample as one NDJSON batch with the Write tool, one row per sampled id, each with its own citation and a one-line rationale:

```json
{"finding": "F7", "verdict": "confirmed", "by": "audit", "citation": "src/merge.rs:88", "rationale": "Upheld: merge.rs:88 truncates the batch at CHUNK, as the failure_scenario says."}
{"finding": "F11", "verdict": "confirmed", "by": "audit", "citation": "src/render.rs:66", "rationale": "Overturned: the refutation cites render.rs:40, but the empty section is emitted at render.rs:66, which the trigger reaches."}
```

```bash
tomlctl review verdict --slug <slug> --ndjson <run-dir>/payloads/audit.ndjson
```

Read the output's `refused` list and fix each refused row before going on. Then record the sample on the run: `sampled` is the batch's row count and `overturned` the rows whose call differs from the recorded one:

```bash
tomlctl review run update <run> --slug <slug> --set-json 'audit={"sampled":<n>,"overturned":<m>}'
```

Run `review resolve --run <run>` again when any overturned row carries a `same_as`, then re-read the digest before stage 7.

## Stage 7 — Triage

**The report.** Write it to the run directory:

```bash
tomlctl review report --run <run> --slug <slug> --out <run-dir>/report.md
```

It prints the path and a counts line. Do not read the report back; it is for the user.

**What to present.** In one message, in this order:

1. the counts line;
2. the blocked findings, grouped by `blocked_on` in the order of the handling below, each by id, summary and what it lacks (its `blocked_*` detail), with the handling its group gets;
3. the deferred proposals, each with its age, the overdue ones marked as needing a disposition before the run can close;
4. contested findings, with your audit outcome for each;
5. the advisory list from the digest, each item with your recommendation and a one-line reason;
6. the report path;
7. the disposition grammar below, and what happens to anything the user leaves untriaged.

Read the blocked findings from the store rather than the run, since a block an earlier run left open is still the user's to answer:

```bash
tomlctl review list --slug <slug> --status open --blocked-on any
```

Read the deferred proposals from the final digest's `proposals`: each row carries the `proposal` note's path, `proposal_at`, its `age` (runs of the family started since the deferral) and `overdue`, set at an age of 3.

**Blocked findings.** A blocked finding cannot be fixed as written; what happens to it depends on what it lacks. Every route below is the user's to take: you never answer a decision, pick the `plan` route, park a blocked finding or send it to the backlog on your own recommendation.

- **`decision`** — ask every decision finding together, in one `AskUserQuestion` round after the presentation: one question per finding, up to four questions per call, the calls back to back, each naming the finding's summary and what it lacks and offering its `blocked_options` as the choices (with more than four options, name them all in the question and take the answer as text). Stage each answer — the chosen option's text and anything the user added, at most 800 characters — in `<run-dir>/payloads/answer-<id>.md`, and record it:

  ```bash
  tomlctl review triage F2 --slug <slug> --answer-file <run-dir>/payloads/answer-F2.md --expect-status open
  ```

  The answer clears the block and leaves the finding `open` and fixable, with the answer bound to its fix task when it is scheduled, so it can be picked under `apply` in the same triage. A question the user leaves unanswered records nothing, and the finding stays blocked for the next run.
- **`evidence`** — offer it for apply. `/apply-v2` schedules it as a settle task, whose worker investigates and returns a proposal or a refutation in a note; it is not fixed in that round. A finding that already carries a `proposal` has had its settle round and is listed with the deferred proposals instead.
- **`measurement`** and **`alternatives`** — list them as waiting for trials. Nothing runs them yet, and `/apply-v2` refuses them; they stay `open` and blocked.
- **`design`** — offer the `plan` route: it names `/plan-v2` with the finding as the work to plan, so the plan starts from the store's record of it rather than a copy. Only the user picks it.

**Deferred proposals.** A proposal deferred at an earlier settle stays `open` and blocked while it ages. List each one with its age and the path of its note. An overdue one must be given a disposition before `review run finish`, which refuses while any is overdue, `--incomplete-file` included: `accept` or `refute` through `review settle`, or any `review triage` disposition that moves it off `open`. Recommend one, as for advisory; the user decides.

**Advisory recommendations.** Each recommendation is advice; the user decides. Recommend one of:

- **park** — the default. The item stays in the store, shows in later runs' `PRIOR` and in `review list --kind advisory`, and costs nothing now.
- **backlog** — only when the item is a concrete, argued direction that `backlog-capture`'s minting bar would accept: not a hunch, not an unargued direction, not something another store already owns.
- **convert** — when adopting it needs no decision after all, and you can write the concrete `edit` or `fix` now.
- **dismiss** — when the item restates a settled decision with no new evidence, or its `adoption_cost` plainly outweighs its `rationale`.

A `challenge` item names the decision it challenges; recommend parking it unless the rationale carries evidence the decision did not have.

**The gate.** Dispositions are the user's. Present them and wait for the answer: an instruction to proceed autonomously, or not to ask questions, does not cover this step, because a disposition decides what is applied, sent to the backlog or dropped. Ask with `AskUserQuestion` or end the turn on the presentation. An empty answer (a headless session) takes no disposition: stage 8 parks the untriaged advisory, records no decision answer, leaves every blocked finding as it is, hands nothing to apply, and the close summary says triage is pending. When a deferred proposal is overdue, the run cannot finish without a disposition: leave it open, skip stage 8's step 5, and name the overdue ids in the close summary as what the next session must answer.

### The grammar

Each user instruction maps to `review triage` calls, or `review settle` for a deferred proposal. `--expect-status` is the status the row had when you presented it (`open` for an untriaged row; the row's current status for a `reopen`); a row that moved in the meantime lands under `skipped_stale` and is reported back rather than forced.

| User says | Calls |
|---|---|
| `dismiss F3 — <reason>` | stage `<reason>`; `review triage F3 --dismiss-file <path> --expect-status open` |
| `defer F3 — <reason> — <trigger>` | stage both; `review triage F3 --defer-file <reason path> --trigger-file <trigger path> --expect-status open` |
| `reopen F3 — <reason>` | stage `<reason>`; `review triage F3 --reopen-file <path> --expect-status <its current status>` |
| `park F7 — <reason>` | stage `<reason>`; `review triage F7 --park-file <path> --expect-status open` |
| `backlog F7,F9` | stage 8's backlog hand-off, then one `review triage <id> --backlog-ref <B-id> --expect-status open` per id |
| `convert F8` | write the replacement row (below); `review triage F8 --convert cleanup\|defect --ndjson <path> --expect-status open` |
| `addressed F4 — <what changed>` | plan family only, for a finding the author fixed by hand: stage `<what changed>`; `review triage F4 --addressed-file <path> --expect-status open`, adding `--targets <targets>` when the hand edit changed records other than the finding's own `target`. The row becomes `resolved` with its `addressed` targets, which the re-check covers; `--addressed <text>` is the inline twin, never used for user prose |
| `answer F2 — <answer>` | stage `<answer>`; `review triage F2 --answer-file <path> --expect-status open`, for a finding blocked on `decision` |
| `accept F6 — <fix>` | stage `<fix>`; `review settle F6 --accept --tier user --fix-file <path>`, for a deferred proposal |
| `refute F6 — <citation>` | `review settle F6 --refute --citation '<file:line>'`, for a deferred proposal |
| `plan F5` | stage 8's plan hand-off names `/plan-v2` for the finding blocked on `design`; no `review triage` call |
| `apply F1,F4` / `apply all` | stage 8's apply hand-off; no `review triage` call |

Every call takes the store flag. Several ids sharing one disposition and one reason go in one call (`review triage F3,F4 --dismiss-file …`); a `backlog-ref` is per id, because each pick gets its own backlog item.

For example, a dismissal whose reason you staged:

```bash
tomlctl review triage F3 --slug <slug> --dismiss-file <run-dir>/payloads/dismiss-F3.md --expect-status open
```

**Convert.** Write the replacement as one JSON object with the Write tool to `<run-dir>/payloads/convert-<id>.ndjson`: the concrete `edit` (with `direction`, `old`, `effort` and any `target_file`) for `cleanup`, or the `fix`, `failure_scenario`, `severity`, `effort`, `counter`, `grade` and `evidence` for `defect` (and `line` when the advisory has none), anchored at the advisory's `file`, `line` and `symbol`. The verb mints the new finding with an `orchestrator` verdict, routed by its edit target, and marks the advisory `converted`. Read what it prints: when the new finding is minted `unverified` with a `verifier` or `checker` route, verify it with a one-row dispatch per `agent-waves.md` before handing it to apply.

**Convert after the run finished.** An advisory parked in an earlier run and revisited later belongs to a finished run, which cannot verify a replacement, so a plain `--convert` on it is refused. Convert it into the run you have open: add `--run <open run>`, which must be of the advisory's family. The replacement is minted `unverified` in that run, routed by its catalogue, and that run cannot finish until the replacement is verified, so verify it in the same run's waves. With no open run, leave the advisory parked and convert it at the next run.

## Stage 8 — Close

In this order.

**1. Park the untriaged advisory.** Every advisory the digest still lists as untriaged is parked with one staged reason, `untriaged at the close of <run>`:

```bash
tomlctl review triage <ids, comma-separated> --slug <slug> --park-file <run-dir>/payloads/park-untriaged.md --expect-status open
```

**2. Backlog and plan picks.** Only for ids the user named under `backlog` or `plan`; the orchestrator never sends an item to the backlog or to planning on its own recommendation.

A `plan` pick writes nothing. Name `/plan-v2` with the finding as the work to plan: `/plan-v2 <finding summary> — planned from tomlctl review show <id> --slug <slug> --for implementer` (or `--scope <scope>`), so the plan's brief reads the finding and its `blocked_contract` from the store. Do not start it; the user runs it. The finding stays `open` and blocked on `design`, and shows in the next run's opening listing until the planned work resolves it.

For `backlog` picks, invoke the `backlog-capture` skill now — not earlier in the run — and follow it as the only writer. Read the rows it captures from:

```bash
tomlctl review show <ids, comma-separated> --slug <slug> --for backlog
```

Run the skill's check-then-add gate per row. Record each minted or matched backlog id with `review triage <id> --backlog-ref <B-id> --expect-status open`. A row the gate reports as a duplicate of an existing item takes that item's id; a row the gate refuses to mint is reported back to the user and stays parked.

**3. Apply picks.** Nothing is written at this step; `/apply-v2` reads the selection from the store when the user runs it. Keep the ids the user named under `apply`, exactly as named, for step 4. Which findings schedule, settle, hold back or are refused is the schedule rule's, owned by `tomlctl review schedule` in the `tomlctl` skill's `references/review-apply.md`; do not restate it. Before naming the picks, preview them with the verb itself, which writes nothing and needs no flow or checkpoint:

```bash
tomlctl review schedule <picked-ids> --slug <slug> --dry-run
tomlctl review schedule --all --slug <slug> --dry-run
```

Use `--scope <scope>` in place of `--slug <slug>` for a flow-less store, and the second form for `apply all`. Report its `refused` and `held` rows with their reasons, and its `settle` rows as settle tasks rather than fixes, so the user is not surprised when `/apply-v2` runs. A decision answered at this triage is no longer blocked, so run the preview after recording the answers.

`apply all` is not scoped to this run or this family: it takes the store's open code and perf findings alike, earlier runs' included, so a `/review-v2` and an `/optimise-v2` run on the same store share one selection. List the open defects and cleanup before naming it, show the user that whole list, and when the user meant only this run's findings, offer those ids explicitly instead:

```bash
tomlctl review list --slug <slug> --status open --kind defect
tomlctl review list --slug <slug> --status open --kind cleanup
```

Glimpse does not read the v2 store. A picked finding shows there once `/apply-v2` has scheduled it, as its fix task in the flow's task graph; a finding left `open`, parked or deferred does not appear in glimpse at all, and `review list` and the report are where to see it.

The plan family never hands off to apply: its eligible findings go through the carrier's merge offer instead, which takes this step's place. In flow mode, on a v2 flow, `plan-flow.md`'s tiered apply through `review apply` takes it instead, with its re-probes and re-check run; `plan-flow.md` then owns the rest of the close. Every status write the merge makes — resolving the merged findings, dismissing the ones a Discard drops — lands before step 5. `review run finish` refuses while an id is unverified, so finish after those writes, never before.

**4. Apply hand-off.** Name `/apply-v2` with the store and the user's picks: `/apply-v2 <slug> F1,F4` for a flow-bound store, `/apply-v2 --scope <scope> F1,F4` for a flow-less one, and `--all` in place of the ids for `apply all`. Name only ids the user picked, never ones you recommended. Do not start it; the user runs it. It schedules the picks as fix tasks in a v2 flow — the reviewed flow when its plan is approved, a new flow for a flow-less store — and a finding is `resolved` only once its fix task passes the milestone's gates and is committed. A store bound to a v1 flow has no path through `/apply-v2`, which refuses it at its first step and names the route that applies it; say so when the bound flow has no `.claude/flows/<slug>/plan.toml`.

**5. Finish the run.** Record the run's agent total first, as the engine's stage 4 defines it, so it includes any verifier dispatched after a convert:

```bash
tomlctl review run update <run> --slug <slug> --set-json agents=<total>
tomlctl review run finish <run> --slug <slug>
```

It stamps `finished` and computes `counts`. It refuses while any finding is `unverified`, any `same_as` is unresolved, an unselected matched concern or unsliced scope file lacks a waiver, or any cell of the run is unsealed, and while a deferred proposal of the run's family is overdue; the refusal names each. Fix the cause — a mop-up dispatch, `review resolve`, a waiver, the user's disposition of the overdue proposal — and finish again. Finish with `--incomplete-file <path>` only when a gap cannot be closed in this run, with the staged reason naming each open id or gap and why; it does not cover an overdue proposal.

**6. Refresh the report.** Re-run `review report --out` to the same path so the file carries the dispositions and counts.

**7. Close summary.** Print the run id, the report path, the counts line, what was handed to apply (the `/apply-v2` command named, with its ids or `--all`, and any pick you said the schedule rule will refuse), the backlog ids, any `skipped_stale` rows, the decision answers recorded, any `plan` picks with the `/plan-v2` command named, the deferred proposals given a disposition and any still overdue, a substrate fallback if one happened, and `triage pending` when the user's answer was empty.

Then list each unhandled escalation: every entry of a wave index's `escalations` that you did not turn into a follow-up cell — out of budget, or anchored outside the scope and not a `ripple` row — by cell and text, each offered as a backlog capture. Capture only the ones the user accepts, through `backlog-capture`'s check-then-add gate as in step 2; the rest are dropped with the run.
