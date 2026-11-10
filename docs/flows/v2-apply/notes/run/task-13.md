Status: done — the review engine's triage now hands the user's picks to /apply-v2; export-v1 and the v1 apply carriers are gone from the code and perf paths, and sync-v1 stays as a legacy read-back at stage 0

## What changed

- `claude/skills/review-v2-engine/references/triage-and-close.md`
  - Grammar row `export F1,F4` / `export all` is now `apply F1,F4` / `apply all`, which maps to stage 8's apply hand-off and makes no `review triage` call.
  - The gate paragraph and the empty-answer case say "applied" / "hands nothing to apply" in place of "exported".
  - The convert paragraph verifies a replacement "before handing it to apply".
  - Stage 8 step 3 is now "Apply picks". It writes nothing. It keeps the user's ids as named, says ahead of time which picks the schedule rule will refuse, and explains what `apply all` takes and holds back, listing the open defects and cleanup with `review list --status open --kind defect|cleanup` first. The glimpse paragraph now says a pick shows in glimpse once it is scheduled, as a fix task. The plan-family paragraph says that family "never hands off to apply"; the merge offer and the flow-mode `review apply` text are unchanged.
  - Stage 8 step 4 names `/apply-v2 <slug> F1,F4`, `/apply-v2 --scope <scope> F1,F4`, or `--all` for `apply all`. It names only the ids the user picked and leaves the run to the user. It says what `/apply-v2` does and that a store bound to a v1 flow is refused there, by `/apply-v2`'s own first step.
  - Steps 6 and 7: the refreshed report no longer mentions exports, and the close summary reports what was handed to apply.
  - The `plan F5` row and the Needs-a-plan group are untouched, as the detail asks.
- `claude/skills/review-v2-engine/SKILL.md`
  - Stage 0 capability check: added `review_schedule`, `review_reconcile`, `review_anchor`, `review_reproduce` and `review_stats_apply`. The installed binary lists all five.
  - Stage 0 step 4 (`review sync-v1`) is reworded as a legacy read-back for v1 rows whose `v2_ref` names the store. The command is kept, per D3.
  - The stages 6–8 summary names `/apply-v2` with the picked ids in place of the export and the v1 hand-off.
  - Family deltas: code and perf now name the `/apply-v2` hand-off, and the plan family says "in place of the apply hand-off".
- `claude/skills/review-v2-engine/references/compose.md`: the duplicates paragraph is reworded for scheduling. A linked duplicate becomes `merged`, so only its target is scheduled; an unlinked true duplicate is scheduled as a second fix. "Both rows export separately" is now "are applied separately".
- `claude/commands/review-v2.md`: rewrote the description frontmatter, the intro paragraph and the "writes no v1 ledger" line. The bindings swap "v1 export target" and the `/review-apply` row for one `/apply-v2` hand-off row. Stage 8 is now "backlog and hand-off", and its export bullet is removed.
- `claude/commands/optimise-v2.md`: rewrote the intro sentence. The bindings swap the export-target and `/optimise-apply` rows for one `/apply-v2` row. Stage 8 is now a hand-off section with no export commands.
- `CLAUDE.md`: the Review v2 Store bullet's last sentence now describes the `/apply-v2` hand-off through `review schedule` and `/implement-v2`'s engine, and `sync-v1` as a read-back only. The Deploy bullet's list of linked v2 entries now includes `/apply-v2`; both link scripts already carry it.

## Deviations

None from a `suggested` means. Where the detail left the edit open:
- Triage-time refusals: `/apply-v2` has no dry run of its own, so the orchestrator now names likely refusals when it presents the hand-off. The alternative was to say nothing and leave it to `/apply-v2`'s preview; I chose the up-front warning because the old export step reported refusals before the hand-off.
- A store bound to a v1 flow: the hand-off says `/apply-v2` refuses it at its first step and names the route itself. The engine therefore names no v1 carrier, which the outcome requires, and the user is still pointed somewhere.

## Handover

None.

## Checks

- Compile check: not applicable (markdown only).
- Accept checks (all `Writes build: no`): all five pass (triage-and-close names /apply-v2; no `review export-v1` in triage-and-close; SKILL.md names no export-v1 or v1 apply carrier; neither carrier names a v1 apply carrier; CLAUDE.md names /apply-v2).
- Each new `tomlctl` invocation, `review list --slug <slug> --status open --kind defect`, was run against a live store and exits 0. The form also appears in `apply-v2-engine`.
- I did not run the `cli::dispatch::tests` carrier gate, because it builds the crate. The gate runs it.

TANGENTIAL: debt | claude/skills/review-v2-engine/references/plan-flow.md | Says the plan grammar is the engine's "without `export`"; the engine row is now `apply`, so it should read "without `apply`" | A stale word in the plan-family flow-mode reference misnames the grammar row it excludes
TANGENTIAL: debt | claude/skills/review-v2-engine/references/measurement.md | Apply outcomes are described only through `stats --outcomes` over exported findings; with triage no longer exporting, new runs feed only `stats --apply`, which the table row does not name | The measurement table will read empty outcome baselines for new runs unless it points at `--apply`
