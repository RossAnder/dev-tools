Verdict: a worker could carry out milestone B as written; nothing stops it. There is one false premise (task 9 places the After Merge examples in /plan-new "Phase 5"; they are in Phase 6), and task 10's guard check fails on the present tree for an environmental reason.

# Cold read, milestone B (tasks 8, 9, 10, 11, 12)

## Unactionable points

None. Each task names its file, the passage to change and the shape the change takes. Every reference to `design/handover.md#...` resolves to `docs/flows/human-only-handovers/design/handover.md`, and the sections cited (`#shape`, `#reasons`, `#where-agent-performable-steps-go-instead`) all exist there. The worker view does not give that path in full, but a single glob finds it.

Minor points a worker can settle alone (not counted):
- Task 10 says "Add a check" to plan-risk.md. The lens has no checks section, so the natural home is a bullet under `## Probes — plan` (plan-risk.md:28), with the calibration under `## Severity calibration`, which already has a `major` tier (plan-risk.md:71).
- Task 11 cites `tomlctl plan handover set --slug <slug> --ndjson ...`. The installed CLI has only `plan handover add|done` (`tomlctl plan handover --help`; `PlanHandoverOp` at tomlctl/src/cli/types/plan.rs:74). Task 11 needs tasks 1 and 3 (milestone A), and contract C2 makes task 1 provide the verb, so this is an ordering dependency, not a false premise. A worker should confirm the verb's flags against the reinstalled binary before citing it, because of the constraint that every backticked tomlctl command must match the CLI.

## False premises

1. Task 9 Detail says "plan-new.md Phase 5 step 5: replace the parenthetical list of After Merge examples". Decision D1 repeats "/plan-new Phase 5". In claude/commands/plan-new.md, Phase 5 is "Directed Research" (line 94). The parenthetical "(install a binary, resync a cache, restart sessions, update memory, run a **predicted, unverified** criterion by hand)" is in step 5 "Set the execution policy" of `## Phase 6: Design` (plan-new.md:100, sentence at :108). The acceptance grep pins the text, so a worker will still find it. The phase number is wrong all the same.

All other premises held. The checks that confirmed them:
- Format contract template, SKILL.md:206-211: `## After Merge` at :206, and the examples (install, resync a plugin cache, restart, update a memory note, run a predicted criterion by hand) at :207-209. The `**Format rules:**` block starts at :228. The file has 254 lines, well under the 500 cap.
- Verification Commands optional-key bullets have the form ``- `key: <command>` — ...`` at column 0 (SKILL.md:105-127), so a new ``- `install: <command>` `` bullet will match task 8's anchored grep.
- implement.md Phase 1 extracts `## Verification Commands` (implement.md:73). Phase 3 orders build → tests → e2e → success → lint + audit and passes one ordered `commands:` list to the verification agent (implement.md:256). Phase 4 has the backlog harvest (:295). "steps the user must take by hand" is at :330 and the `### After Merge` report heading at :324.
- plan-risk.md: "an After Merge or Risks line" at :57, After Merge probe at :36, minor tier at :72. plan-structure.md: "schedules to be run by hand" at :34, `risk` advisory at :65, `critical` at :70.
- approve.md section 3, item 7: "`stops`: each milestone gate command and undone handover." (approve.md:60). The empty-array-as-`none` rule is at :62.
- write-and-prove.md section 1 Design has the **Milestones** paragraph at :47, followed by the design-files paragraph.
- docs/ideas/v2-pipeline.md:143 has the `**After Merge:**` bullet under `### Smaller fixes` (:140), containing "an agent can be told to run".

## Check commands (run on the present tree, from the repo root, under Git Bash)

Task 8:
- Premise `grep -q 'update a memory note' claude/skills/flow-contract-plan-output-format/SKILL.md`: exit 0 (holds).
- `! grep -q 'update a memory note' ...SKILL.md`: exit 1 (red now, discriminates).
- ``grep -q '^- `install: <command>`' ...SKILL.md``: exit 1 (red now).
- `awk '/^\*\*Format rules:\*\*/{f=1} f' ...SKILL.md | grep -q 'Done when'`: exit 1 (red now).
- Contract C4 (``grep -q '`install: <command>`' SKILL.md && grep -q 'install:' claude/commands/implement.md``): exit 1 (red now; turns green only after both tasks 8 and 9).

Task 9:
- Premise `grep -q 'steps the user must take by hand' claude/commands/implement.md`: exit 0.
- `! grep -q 'install a binary, resync a cache' claude/commands/plan-new.md`: exit 1 (red now).
- `grep -q 'install:' claude/commands/implement.md`: exit 1 (red now).
- `! grep -q 'steps the user must take by hand' claude/commands/implement.md`: exit 1 (red now).

Task 10:
- Premise `grep -q 'an After Merge or Risks line' .../plan-risk.md`: exit 0. The forward check is its negation, so it is red now.
- `! grep -q 'schedules to be run by hand' .../plan-structure.md`: exit 1 (red now).
- Guard `tomlctl review catalogue --check`: exit 1, printed `tomlctl: lens catalogue not found at <claude config dir>/skills/review-v2-lenses`. The guard fails on the present tree for an environmental reason: the review-v2-lenses skill is not linked into the Claude config dir on this machine, and `--builtin` defaults there (`tomlctl review catalogue --help`). Where the link does exist, the default would validate the deployed copy rather than the worker's edit unless that copy is a symlink. `tomlctl review catalogue --builtin claude/skills/review-v2-lenses --check` exits 0 on the present tree and validates the repo copy, so that is the form the guard should use. As written, a worker who runs it sees a failure unrelated to their edit.

Task 11:
- Premise `grep -q 'gate command and undone handover' .../approve.md`: exit 0. The forward check is its negation, so it is red now.
- `grep -q 'plan handover set' .../write-and-prove.md`: exit 1 (red now).

Task 12:
- Premise `grep -q 'an agent can be told to run' docs/ideas/v2-pipeline.md`: exit 0. The forward check is its negation, so it is red now.

## Coverage

Read: the worker views and file lists for tasks 8-12; docs/flows/human-only-handovers/design/handover.md (cited by tasks 8, 10, 11 and 12); claude/skills/flow-contract-plan-output-format/SKILL.md (sections Verification Commands, After Merge, Format rules); claude/commands/implement.md and claude/commands/plan-new.md (headings and After Merge, Phase 1/3/4 sites); claude/skills/review-v2-lenses/lenses/plan-risk.md and plan-structure.md (frontmatter, After Merge sites, severity); claude/skills/plan-v2-engine/references/approve.md section 3 and write-and-prove.md section 1; docs/ideas/v2-pipeline.md:140-143; `tomlctl plan --help`, `plan handover --help`, `review catalogue --help`.

Not read, as the cold-read procedure requires: notes/after-merge-corpus.md and notes/structure-challenge.md. Could not check: the shape of `plan handover set` and the gate's `handovers` key order, since tasks 1 and 3 (milestone A) produce both and neither exists yet.
