Status: done — the drain records attempts, gate runs and the report through the three plan run leaves, and both carriers' pre-flights require plan_run_records

## What changed

- `claude/skills/implement-v2-engine/references/drain.md`
  - Step 2, "Which notes you read": dropped the Grep of every note for `TANGENTIAL:` and `Status:` lines. The `problem` column of `plan run notes` now reports a Status disagreement or a missing or malformed note, and tangential entries, deviations and handover proposals are read from the recorded attempts once step 3 has written them. The rule for which notes are read in full is unchanged, and so is the rule that breakage the milestone caused in a file no row holds goes to the replan.
  - Step 3: a new "The attempts" subsection after the lane check (the clean-lane pointer now says "go on to the attempts below"). It covers the `tomlctl plan run notes --slug <slug> --milestone <id>` call, a table of its row keys, the `kind=validation` refusal for a run that recorded no tasks, a `problem` being reported under `reconcile:`, and the attempt read, `tomlctl items list .claude/flows/<slug>/plan.toml --array attempt --where run_id=<run_id> --select ...`.
  - Step 7: a "Record the gate run" paragraph after the outcome table. It gives the staged row shape and how each verification-block field maps to a row key, the `plan run gate` call, `<n>` = 0 for the first gate run and the fix round's number after that, `--rerun` only for a second dispatch within the same drain (held commands or a timeout re-dispatch, same round), no flag otherwise, and says the round count still comes from the `gate <id> round` reasons.
  - Step 8: candidates come from the attempts' `tangential` entries plus the verification blocks' `TANGENTIAL:` lines (e.g. `flaky-test`), which are not notes. The replan-input rule is restated there. The `backlog check` and `backlog add` procedure is unchanged.
  - Step 10: the flow-store commit names the gate logs, and when the Next rule ends the invocation at this milestone the report is recorded first, so it lands in that commit.
  - Step 11: the intro points to a new "Record the report" subsection. That subsection has the staged row shape, the table mapping sections to `kind` (all ten), the `task` and `path` rules, `needs_person = false` for allowed audit rows, a run that was not audited and a covered check, and the `plan run report` call. With nothing to report it is still called, without `--ndjson`. The same rule appears in step 10. The `reconcile:` template line now carries the `plan run notes` problem.
- `claude/commands/implement-v2.md`: `plan_run_records` added to pre-flight step 2's feature list, and the sentence after it extended with "or record a run's attempts, gate rounds and report".
- `claude/commands/apply-v2.md`: the same feature added to its pre-flight step 2, with the sentence extended the same way.
- `CLAUDE.md`: the Implement v2 Deploy bullet names `plan run notes`, `plan run gate` and `plan run report` among what the pre-flight refuses a binary without. In Backlog capture, a plan worker now surfaces candidates as `[[tangential]]` entries in its run note's block, and other sub-agents on a `TANGENTIAL:` line.

## Deviations

- Attempt read: the detail offered `plan view --for digest` or `items list ... --array attempt`. The digest has no attempt content (checked: `plan view --for digest` prints counts and open items only), so the drain uses `items list` filtered by `--where run_id=<run_id>`. The filter keeps attempts from earlier relaunches out, since attempts are upserted per (task, run).
- An empty report is recorded by calling `plan run report` without `--ndjson`. plan-run.md documents that as "the report has no entries", which avoids staging an empty file.

## Checks

- Accept: the drain calls the three leaves: pass.
- Accept: `plan_run_records` appears in implement-v2.md: pass.
- Accept: `plan run notes` appears in CLAUDE.md: pass.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 55 passed, 0 failed.
- The installed binary advertises `plan_run_records` (`tomlctl capabilities --get features`), and `items list ... --array attempt --where run_id=...` runs against this flow's store.

TANGENTIAL: none
