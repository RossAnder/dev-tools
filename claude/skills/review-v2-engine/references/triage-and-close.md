# Triage and close — stages 6 to 8

After the last wave has resolved and its digest shows no unverified id, the orchestrator audits a sample of verdicts, presents the run to the user, applies the user's dispositions, and closes the run. Pass `--scope <scope>` in place of `--slug <slug>` for a flow-less store in every command below.

Stage the user's prose and your own rationales with the Write tool, in the run directory's `payloads/` subdirectory, and pass them through the `--<flag>-file` twins (`--dismiss-file`, `--defer-file`, `--trigger-file`, `--reopen-file`, `--park-file`, `--resolve-file`, `--rationale-file`, `--incomplete-file`). Never quote user text into a shell argument.

## Stage 6 — Audit

The audit is the outside check on the verifiers and checkers; its overturns are what calibration learns from, so it is never skipped and never widened into a re-review.

**The sample.** Read it from the final digest:

```bash
tomlctl review digest --run <run> --slug <slug>
```

- every REFUTED critical defect;
- every contested finding;
- the digest's audit sample, chosen by index: two CONFIRMED defects, two REFUTED non-critical defects, and two checked cleanup rows, at least one of them from a sweeper cell.

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

**Recording.** Record each overturn as an audit verdict, with the rationale staged as a file:

```bash
tomlctl review verdict <finding> --slug <slug> --verdict confirmed --by audit --citation <file:line> --rationale-file <run-dir>/payloads/audit-<finding>.md
```

A row you agree with gets no verdict; it counts as sampled. Then record the sample size and the overturns on the run:

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
2. the Needs-a-plan group — defects with `fix_shape = needs-plan`, by id and summary;
3. contested findings, with your audit outcome for each;
4. the advisory list from the digest, each item with your recommendation and a one-line reason;
5. the report path;
6. the disposition grammar below, and what happens to anything the user leaves untriaged.

**Advisory recommendations.** Each recommendation is advice; the user decides. Recommend one of:

- **park** — the default. The item stays in the store, shows in later runs' `PRIOR` and in `review list --kind advisory`, and costs nothing now.
- **backlog** — only when the item is a concrete, argued direction that `backlog-capture`'s minting bar would accept: not a hunch, not an unargued direction, not something another store already owns.
- **convert** — when adopting it needs no decision after all, and you can write the concrete `edit` or `fix` now.
- **dismiss** — when the item restates a settled decision with no new evidence, or its `adoption_cost` plainly outweighs its `rationale`.

A `challenge` item names the decision it challenges; recommend parking it unless the rationale carries evidence the decision did not have.

**The gate.** Dispositions are the user's. Present them and wait for the answer: an instruction to proceed autonomously, or not to ask questions, does not cover this step, because a disposition decides what is exported, sent to the backlog or dropped. Ask with `AskUserQuestion` or end the turn on the presentation. An empty answer (a headless session) takes no disposition: stage 8 parks the untriaged advisory, exports nothing, and the close summary says triage is pending.

### The grammar

Each user instruction maps to `review triage` calls. `--expect-status` is the status the row had when you presented it (`open` for an untriaged row; the row's current status for a `reopen`); a row that moved in the meantime lands under `skipped_stale` and is reported back rather than forced.

| User says | Calls |
|---|---|
| `dismiss F3 — <reason>` | stage `<reason>`; `review triage F3 --dismiss-file <path> --expect-status open` |
| `defer F3 — <reason> — <trigger>` | stage both; `review triage F3 --defer-file <reason path> --trigger-file <trigger path> --expect-status open` |
| `reopen F3 — <reason>` | stage `<reason>`; `review triage F3 --reopen-file <path> --expect-status <its current status>` |
| `park F7 — <reason>` | stage `<reason>`; `review triage F7 --park-file <path> --expect-status open` |
| `backlog F7,F9` | stage 8's backlog hand-off, then one `review triage <id> --backlog-ref <B-id> --expect-status open` per id |
| `convert F8` | write the replacement row (below); `review triage F8 --convert cleanup\|defect --ndjson <path> --expect-status open` |
| `plan F5` | no store write; the close summary names `/plan-new` with `review show F5 --for implementer` as its input |
| `export F1,F4` / `export all` | stage 8's `export-v1` |

Every call takes the store flag. Several ids sharing one disposition and one reason go in one call (`review triage F3,F4 --dismiss-file …`); a `backlog-ref` is per id, because each pick gets its own backlog item.

For example, a dismissal whose reason you staged:

```bash
tomlctl review triage F3 --slug <slug> --dismiss-file <run-dir>/payloads/dismiss-F3.md --expect-status open
```

**Convert.** Write the replacement as one JSON object with the Write tool to `<run-dir>/payloads/convert-<id>.ndjson`: the concrete `edit` (with `direction`, `old`, `effort` and any `target_file`) for `cleanup`, or the `fix`, `failure_scenario`, `severity`, `fix_shape`, `effort`, `counter`, `grade` and `evidence` for `defect` (and `line` when the advisory has none), anchored at the advisory's `file`, `line` and `symbol`. The verb mints the new finding with an `orchestrator` verdict, routed by its edit target, and marks the advisory `converted`. Read what it prints: when the new finding is minted `unverified` with a `verifier` or `checker` route, verify it with a one-row dispatch per `agent-waves.md` before exporting it.

## Stage 8 — Close

In this order.

**1. Park the untriaged advisory.** Every advisory the digest still lists as untriaged is parked with one staged reason, `untriaged at the close of <run>`:

```bash
tomlctl review triage <ids, comma-separated> --slug <slug> --park-file <run-dir>/payloads/park-untriaged.md --expect-status open
```

**2. Backlog picks.** Only for ids the user named under `backlog`; the orchestrator never sends an item to the backlog on its own recommendation. Invoke the `backlog-capture` skill now — not earlier in the run — and follow it as the only writer. Read the rows it captures from:

```bash
tomlctl review show <ids, comma-separated> --slug <slug> --for backlog
```

Run the skill's check-then-add gate per row. Record each minted or matched backlog id with `review triage <id> --backlog-ref <B-id> --expect-status open`. A row the gate reports as a duplicate of an existing item takes that item's id; a row the gate refuses to mint is reported back to the user and stays parked.

**3. Export.** For `export F1,F4` or `export all`, export to the family's v1 ledger. The flow-bound targets are `.claude/flows/<slug>/review-ledger.toml` (code) and `.claude/flows/<slug>/optimise-findings.toml` (perf); the flow-less targets are `.claude/reviews/<scope>.toml` and `.claude/optimise-findings/<scope>.toml`. Preview first:

```bash
tomlctl review export-v1 F1,F4 --slug <slug> --to .claude/flows/<slug>/review-ledger.toml --dry-run
tomlctl review export-v1 --all --slug <slug> --to .claude/flows/<slug>/review-ledger.toml --dry-run
```

Report every refusal with its reason (advisory, `needs-plan`, `effort = "large"`, a status other than `open`, the plan family), then run the same command without `--dry-run`. `--all` leaves out `needs-plan` defects, test removals and cleanup whose latest verdict is PLAUSIBLE; those export only by explicit id. Each exported row carries `v2_ref`, and the next run's `review sync-v1` reads its v1 outcome back.

The plan family never exports: its eligible findings go through the carrier's merge offer instead.

**4. Apply hand-off.** Name the apply command and the v1 ids `export-v1` printed: `/review-apply <ids>` for code, `/optimise-apply <ids>` for perf. Do not start it; the user runs it.

**5. Finish the run.**

```bash
tomlctl review run finish <run> --slug <slug>
```

It stamps `finished` and computes `counts`. It refuses while any finding is `unverified`, any `same_as` is unresolved, or an unselected matched concern or unsliced scope file lacks a waiver; the refusal names each. Fix the cause — a mop-up dispatch, `review resolve`, a waiver — and finish again. Finish with `--incomplete-file <path>` only when a gap cannot be closed in this run, with the staged reason naming each open id or gap and why.

**6. Refresh the report.** Re-run `review report --out` to the same path so the file carries the dispositions, exports and counts.

**7. Close summary.** Print the run id, the report path, the counts line, what was exported (v1 ids and ledger), the backlog ids, any `skipped_stale` rows, any `plan` picks with their `/plan-new` hand-off, a substrate fallback if one happened, and `triage pending` when the user's answer was empty.
