Verdict: Real friction, in two of four multi-milestone v2 runs. The store-order pick and the deferred-milestone stop (`claude/skills/implement-v2-engine/references/run-milestone.md:58`, `:67`) held runnable work behind an unelaborated milestone, and elaboration had no owned procedure. But in three flows the milestone order carried intent that no `needs` edge records, so picking by `needs` alone would have broken it.

```toml sources
read_at = "5aca3e7be8f169ba2b524edbb4e87b955e5b67da"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 58
claim = "Pick rule: first checkpoint in store order with a non-done row; --milestone refused while an earlier checkpoint is unfinished"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 67
claim = "A deferred milestone stops the engine; elaborating it is out of this slice"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 17
claim = "Pre-flight: a blockers entry other than an implement open run stops the engine with that entry quoted"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 64
claim = "After a commit the engine picks the next milestone while nothing is parked and --milestone was not given"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 89
claim = "Open-run rule covers only implement:<id> and reproduce:<id> runs, not cold-read"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 57
claim = "When to defer a milestone; nothing on where a deferred milestone sits in order"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 243
claim = "A deferred milestone is cold-read when it is elaborated; no procedure owns that step"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 729
claim = "plan/milestone-width is info-only, skips deferred milestones, and judges rows, never milestone order"

[[source]]
path = ".claude/flows/glimpse-v2-visibility/tasks.toml"
claim = "Milestone C rows 34-37 need only A rows (5, 6, 8, 9, 10, 13) or each other, none of B's 15-33"

[[source]]
path = ".claude/flows/glimpse-v2-visibility/plan.toml"
claim = "Handover H1 'when: After milestone B is committed.' is the only record of the C-after-H1 order"

[[source]]
path = ".claude/flows/tomlctl-test-and-gate-cost/tasks.toml"
claim = "Task 14 sits in B with needs = [] though it could start in A"

[[source]]
path = ".claude/backlog.toml"
claim = "B-0ee4f204, B-c2d75b04, B-4222c464, B-a505bedd and their measured context"
```

## Answer

Ten sessions since 2026-11-05 ran `/plan-v2` or `/implement-v2` in dev-tools or reportdesignkit (rdk), counting the current one, which is out of scope. Of the four multi-milestone flows, two showed the friction the question asks about: rdk `package-design-tools` and dev-tools `tomlctl-test-and-gate-cost`. `glimpse-v2-visibility` (no deferred milestone) showed a cross-milestone ordering need, and `human-only-handovers` (2026-11-09, plan only) showed none. Times are UTC, as the transcripts record them, and the session ids are the transcript UUID prefixes.

Three things matter for the decision:

1. **The store-order hold is real.** It cost about 70 minutes on rdk, and the user saw it in glimpse.
2. **Store order carried intent in all three flows that ran.** Each had a reason, outside `needs`, for its milestone order:
   - rdk review finding F10, a risk-first order;
   - glimpse's C must follow handover H1;
   - dev-tools task 14 is held back to protect the compile cache.

   A picker driven only by `needs` would have broken all three.
3. **Each elaboration was improvised a different way.** One ran a cold read and one did not, and both hit the gaps that B-c2d75b04 and B-4222c464 name.

## Findings

- **Library/API or file**: `claude/skills/implement-v2-engine/references/run-milestone.md:58` (pick rule), `:67` (a deferred milestone stops the engine)
- **Source**: rdk session f526a28c. The user wrote at 2026-11-12T13:54:54Z: "Why does my glimpse dag view have this section of the DAG on hold while a single agent works on task 32?" The orchestrator replied at 13:55:06Z: "The engine runs one milestone per Workflow run, in store order A, B, C, D… It also refuses `--milestone C` while an earlier milestone still has an unfinished row… the hold is purely a consequence of how the plan was cut, not a real dependency." The rdk store confirms that no C row (17–28, 51, 52) needs a B row (29–32, 49, 50).
- **Evidence-grade**: high
- **Finding**: The store-order pick and the stop at a deferred milestone held 12 runnable C rows for about 70 minutes, through B's 24-minute elaboration and then B's mostly single-chain rounds. This is B-0ee4f204's measurement, and the transcript confirms it.
- **Details**: A was committed at 12:53Z (dbbfb4bd). Elaboration of B ran 12:57–13:18Z (commit 08f7671c). B's rounds ran 13:18–14:03Z, then the user paused before B's gate. C launched only at 2026-11-13T07:36Z (session 75c7f6a2), after B's gate. The overnight part of that wait was the user's own pause, not the engine.
- **Counter**: A C row with a `needs` or a file overlap on a B row would make the hold real. The rdk store shows none.
- **Impact on plan / report**: This confirms the problem behind B-0ee4f204. The next finding limits the fix.

- **Library/API or file**: `claude/skills/plan-v2-engine/references/write-and-prove.md:57`; `tomlctl/src/plan/check.rs:729` (`milestone_width_findings`)
- **Source**: rdk session 7664d9d9. At 2026-11-12T09:42:52Z the orchestrator asked: "Review finding F10 (milestone order): as planned, the Themes editor… is only reached after the whole font and Copy storage pipeline… Reorder?" The user picked "Themes editor earlier (Recommended)". At 09:50:29Z C's reason was recorded as "Re-cut at the plan review (finding F10…): the storage and font tasks… now follow the Themes editor." dev-tools session c372393f, 2026-11-16T04:51:00Z: "in that flow the store order was the user's deliberate choice."
- **Evidence-grade**: high
- **Finding**: The deferred B was put ahead of the independent, already elaborated C on purpose, to test the D2 panel-fit premise first. B-a505bedd says otherwise: "C… too carried a defer reason before it was elaborated", but C, then labelled B, was elaborated upfront. Neither "place deferred milestones last" nor "pick by needs alone" respects that choice.
- **Details**: Before the re-cut, B was the storage and fonts milestone (12 rows, upfront) and C the deferred Themes milestone. F10 swapped their labels and order. `plan/milestone-width` (info) reported only "milestone `C`: 17 could join `A`". It skips deferred milestones and never compares milestone order, so nothing flagged the order.
- **Counter**: The re-cut's ask would refute this, if it sought only an earlier panel-fit answer and not a hold on font work until then. The question's wording, "answered before any font work", reads as an ordering intent.
- **Impact on plan / report**: Ordering intent needs a field the picker honours, such as the `after = [ids]` that c372393f proposed and the user accepted at 04:59:38Z. Without one, B-a505bedd's reorder rule would undo the user's F10 decision.

- **Library/API or file**: `.claude/flows/glimpse-v2-visibility/tasks.toml` (C rows 34–37) and `plan.toml` handover H1; `claude/skills/implement-v2-engine/references/run-milestone.md:58`
- **Source**: dev-tools session 9c1cf859, 2026-11-12T10:24:11Z: "Milestone C now runs after handover H1, because its first drain raises `plan.toml`'s schema stamp and a glimpse built before milestone B would refuse the flow." The store records C's needs as only `[5]`, `[6]`, `[8,9,10,13]` and `[36]`, all A rows or C rows. Session 6c71a3dd launched C at 2026-11-13T06:54:30Z without checking H1, and its closing report at 07:05:27Z still lists H1 as open.
- **Evidence-grade**: high for the store fact, medium for "H1 not done" (the user may have done it unrecorded)
- **Finding**: C's order behind B and behind H1 is held only by store order and a handover's free-text `when`. A `needs`-based picker would run C right after A, and today's engine never checks a handover before it launches a later milestone.
- **Details**: D12 in that plan was "tomlctl first, glimpse second, contract edits last". The rejected alternative was "milestones merge; drain edits need their own capability check". The order also encodes the installed binary (the A gate runs `cargo install --path tomlctl`), which no task edge expresses.
- **Counter**: A store field already gating C on H1 would refute this. `plan.toml` has only H1's `when`, and run-milestone.md has no handover step.
- **Impact on plan / report**: The ordering field has to cover a handover or install that must finish before a milestone starts, not only another milestone's commit.

- **Library/API or file**: `.claude/flows/tomlctl-test-and-gate-cost/tasks.toml` (task 14, `needs = []`, checkpoint B)
- **Source**: dev-tools session ec0edfee, 2026-11-13T11:10:03Z: "The workspace task (14) sits in milestone B although it could start in A; I kept it there so the target directory does not move while A's workers are compiling." `plan/milestone-width` reported "milestone `B`: 14 could join `A`" four times.
- **Evidence-grade**: high
- **Finding**: This is a third order chosen for a reason with no edge: protecting the shared build environment. Any rule that pulls rows forward or runs milestones side by side has to preserve it.
- **Details**: Task 14 moved the target directory into a root workspace. Running it beside A's compiling workers would have invalidated their builds.
- **Counter**: Showing that task 14's move was harmless under concurrency would refute this, and the session asserted the opposite.
- **Impact on plan / report**: This argues against filling idle pool slots with a later milestone's rows, which matches c372393f's recommendation, and for keeping the `milestone-width` info as advice, not a rule.

- **Library/API or file**: `claude/skills/plan-v2-engine/references/write-and-prove.md:243`; `claude/skills/implement-v2-engine/references/run-milestone.md:67`
- **Source**: rdk f526a28c. At 12:57:06Z the orchestrator grepped plan-v2-engine for "deferred\|elaborat" to find a procedure, and at 12:58:44Z the tomlctl references. At 13:10:19Z `tasks update` refused with "unknown key `action`". At 13:11:01Z `plan milestone update --id B --elaborated` ran, and the cold read launched only at 13:11:28Z. It returned 7 blocking points, and 13:26:14Z reports "Task 29… returned `failed` because of a naming error in my plan" (the `colourField.ts`/`ColourField.tsx` case collision). dev-tools ec0edfee: at 11:54:40Z the run ended "stopped at milestone C: milestone C is deferred". At 12:04:54Z the user wrote "elaborate milestone C now and continue". At 12:05Z the orchestrator ran a sampled full `cargo nextest` itself, and at 12:11:50Z it marked C `--elaborated` and launched C at 12:14:27Z with no `plan-v2-prove` cold read.
- **Evidence-grade**: high
- **Finding**: Both elaborations were improvised. rdk invoked `/plan-v2` inline and ran a cold read after marking B elaborated. dev-tools edited the rows by hand, skipped the cold read entirely, and needed a full-suite measurement of B's tree before it could write C.
- **Details**: The two runs disagree on whether a cold read is required, the outcome B-4222c464 predicts for an unowned step. The dev-tools case also shows that an elaboration can need a build or measurement of the previous milestone's tree, which bears on whether elaboration fits as a lightweight pool task.
- **Counter**: A step in implement-v2-engine or plan-v2-engine that both sessions skipped would make this a compliance miss, not a gap. The greps at 12:57Z and 12:58Z found none.
- **Impact on plan / report**: This supports an owned `--elaborate` procedure that sets `elaborated` only after the cold read passes, as B-c2d75b04 and c372393f propose. Measurement needs are part of that procedure's scope.

- **Library/API or file**: `claude/skills/implement-v2-engine/references/run-milestone.md:17`; `claude/skills/implement-v2-engine/SKILL.md:89`
- **Source**: dev-tools session 8af59587, a `plan stage` result at 2026-11-12T13:14:58Z, during rdk's B cold read: `"blockers":["open run: step cold-read, started 2026-11-12T13:11:31Z"],"open_runs":["cold-read"]`. At 13:15:51Z the orchestrator said "For the flow as it stands today, the only route is to finish or abandon the `cold-read` run and then run B before C."
- **Evidence-grade**: medium. The blocker was observed, but no `/implement-v2` was actually launched against it.
- **Finding**: An open planning run is a flow-wide blocker. Under run-milestone.md:17 it stops `/implement-v2`, and the open-run rule settles only `implement:` and `reproduce:` runs.
- **Details**: The same session's analysis at 2026-11-16T04:51Z (c372393f) adds three hazards, none observed failing:
  - the lane-check digest covers the whole store;
  - the flow-store commit stages all of `tasks.toml` and `plan.toml`;
  - `cold-read-B.md` from plan time described the old B's tasks 17–28. rdk f526a28c read it at 12:57:19Z and deleted it at 13:11:25Z, because cold-read notes are keyed by label and labels were swapped.
- **Counter**: An engine rule that treats a `cold-read` open run on another milestone as non-blocking would refute this. SKILL.md:89 names none.
- **Impact on plan / report**: This confirms B-c2d75b04's scope. Pre-flight, the lane digest and open-run blockers have to be scoped to the picked milestone, and cold-read notes should be keyed by milestone id.

Coverage: I read the main-thread dialogue of all nine in-scope transcripts (dev-tools 4b607872, 644152d8, 6c71a3dd, 9c1cf859, c372393f, ec0edfee; rdk 75c7f6a2, 7664d9d9, f526a28c), plus 8af59587, the analysis session behind the four backlog rows; I did not read subagent sidechains. I checked the store edges for the C milestones of rdk `package-design-tools`, `glimpse-v2-visibility` and `tomlctl-test-and-gate-cost`, and read run-milestone.md sections 1–2, SKILL.md:64 and :89, write-and-prove.md:53–57 and :243, and check.rs:729–800.

Ruled out:
- The user's milestone-boundary pause requests (4b607872 13:59:20Z "pause for the day after this milestone"; f526a28c 13:49:19Z "pause at the next milestone before the gate"). They are steering, not order friction, and the SKILL.md:64 continue rule served them.
- glimpse B's serial file chains (4b607872 12:52:03Z). That is within one milestone.
- 644152d8 `human-only-handovers`. It had two upfront milestones and no friction.
- Gate flakes and agent-name stamping. They are off-lens.
