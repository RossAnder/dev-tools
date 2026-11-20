Verdict: mostly actionable, but 8 unactionable points and 1 false premise would stop or mislead workers on tasks 16, 18, 19, 22, 23, 28, 30 and 31. Tasks 17, 20, 21, 24, 25, 26, 27 and 29 can be carried out as written.

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = []

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 89
claim = "plan run gate merges a block into the recorded run by exact command string (c.command == command.command)"

[[source]]
path = "claude/agents/verification.md"
line = 146
claim = "a block's command: field is the command as supplied, so a cd-prefixed narrowed command is recorded under that text"

[[source]]
path = "claude/agents/verification.md"
line = 111
claim = "rerun: replaces {ids} with every failed id, each single-quoted and space-separated"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 57
claim = "step 4 only checks that the capture is whole and copies nothing"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 144
claim = "the evidence copy (fail and unsure states only) is in step 7, Keep the evidence"

[[source]]
path = "claude/skills/review-v2-engine/references/plan-flow.md"
line = 320
claim = "a re-check run start needs rechecks=<run> and copies the first run's pack.md and profile.md; targets come only from review recheck-targets"

[[source]]
path = "scripts/shared-blocks.toml"
line = 173
claim = "review-finder-body holds the whole body of review-finder.md byte-identical with review-finder-cf.md"

[[source]]
path = "claude/agents/review-finder.md"
line = 13
claim = "the review-finder-body block opens at line 13 and closes at line 67, so it covers the agent's whole body"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 178
claim = "the dropped-reason table gives each reason a route, and it has no handover:<id> row"

[[source]]
path = "claude/skills/implement-v2-engine/references/report.md"
line = 86
claim = "each report line needs a needs_person decision, and a section's lines map to one kind"

[[source]]
path = "CLAUDE.md"
line = 45
claim = "CLAUDE.md has four Deploy paragraphs (lines 45, 54, 65 and 74)"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 378
claim = "Two rounds at most, step 7 fix rounds, step 10 and Stamping on re-entry (line 471) exist as the tasks cite"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 264
claim = "the pool's compile cap and canStart, which task 20 edits, are here"

[[source]]
path = "claude/skills/plan-v2-engine/references/ui-layout.md"
line = 77
claim = "section 5 holds the config row (a project with no .claude/ui-capture.toml gets a row declaring one)"
```

## Unactionable

1. **Task 16, base rerun recording (detail (1)).** The task says to dispatch the base rerun as `cd <worktree> && <the command narrowed to the failed ids>` and record the result with `plan run gate ... --base --blocks <file>`. The verification agent writes `command:` as the command it was given (verification.md:146). The recorder matches a block to the recorded run by the exact command string (run_gate.rs:89), and milestone A's task 3 says `--base` "refuses a command that run did not record as fail". So every base block names a command the round never recorded, and the recorder refuses it. The task does not say how to map a base block back to its round command, for example by rewriting `command:` in the blocks file or by staging `--ndjson` rows under the original command. A worker would have to invent that step.

2. **Task 16, partial base failure.** The detail covers only "a command whose failed ids all failed at base". It does not say what happens when only some of the ids also fail at base. The worker cannot tell whether the fix row should cover all the failed ids, which spends a round on pre-existing failures and works against the stated Outcome, or only the ids that pass at base, or whether the base-failing subset should also go to the user.

3. **Tasks 16 (detail (5)) and 19, the bun and vitest rerun templates.** The task gives `bun test -t '<name>'` and `--project <p> -t '<name>'`. The verification agent fills `{ids}` with every failed id, each single-quoted and space-separated (verification.md:111). `-t` takes one pattern, so with more than one id the extra ids become positional filters. Neither task says how the ids should be joined into one pattern, or whether these templates may only be used for a single failed id. Both workers would guess, and task 19 must keep "the agent still interprets nothing".

4. **Task 22, a blocking handover the user does not perform at the pick (detail (1)).** The task asks the user to perform the handover now, and launch-args holds the task back until then. It does not say what happens when the user declines or cannot do it in this session. run-milestone.md's `dropped` table (line 178 on) gives every drop reason a route and has no `handover:<id>` row. No task in this milestone adds one, and the drain has no route for it either. The worker has to decide both the table row and the outcome: the milestone ends uncommitted, or the task goes to `needs_human`.

5. **Task 28, the approve-time re-check.** The task says to "run the plan-v2-review recheck over exactly those targets" from approve.md, before the gate. Section 10 of plan-flow.md is written for a re-check inside an open review step. Its run start needs `--set rechecks=<run>`, it copies the first run's `pack.md` and `profile.md` into the re-check directory, it says targets come from `review recheck-targets` "never assemble[d] ... by hand", and it says `/plan-v2`'s step row "stays the one section 4 opened". At Approve the review step has already finished. The task does not settle which run `rechecks=` names, which run's pack is copied, or whether a `plan run start --step review` row is reopened. The worker would have to design that bookkeeping.

6. **Task 30, a parity-locked file missing from its files.** The task edits claude/agents/review-finder.md and has the constraint "Touch no file outside this task's files". The `review-finder-body` shared block (scripts/shared-blocks.toml:173) keeps the whole body of review-finder.md, lines 13 to 67, byte-identical with claude/agents/review-finder-cf.md. Adding the pointer line therefore breaks parity unless review-finder-cf.md is edited too, and that file is not in the task's files. The worker can either break the constraint or fail `scripts/verify-shared-blocks.sh` and the pre-commit hook. Fix: add claude/agents/review-finder-cf.md to the files and require a byte-identical edit, as task 21 does.

7. **Task 31, "the Deploy paragraph" of CLAUDE.md.** CLAUDE.md has four Deploy paragraphs, under Review v2 (line 45), Planning v2 (54), Implement v2 (65) and Apply v2 (74). The task does not say which one should carry the reload note. It also does not say whether the Implement v2 Deploy paragraph's list of refused binaries, which names each pre-flight feature, should gain the new feature names.

8. **Task 18, the new report lines.** Only the `gate round <n> accepted:` line is given `needs_person false`. The task does not say whether `gate round <n>: one more round granted`, `base failure: <command> <ids>` and the visual disagreement line need a person. It also does not say which section they go under: they presumably belong in `gate:` with kind `gate`, but report.md:86 asks for a decision on every line. A worker would guess, and the recorded `needs_person` decides what the next invocation shows.

## False premises

1. **Task 23: "Step 4 copies every state's capture and geometry.json beside the notes, not only fails."** In visual.md, step 4 ("Check the capture", line 57) only checks that the capture is whole and copies nothing. The copy of `fail` and `unsure` states is step 7's "Keep the evidence" paragraph (visual.md:144). A worker can find the right place, but the step reference is wrong. The copy has to widen where it already is, in step 7, and the edit must still run before step 7 compares round r against round r-1.

## Check commands

The worker-owned forward checks were each run on the present tree. All of them exit non-zero now, so each one can tell whether the change has landed:

- 16: `grep -q -- '--base --blocks' ...drain.md && grep -q -- '--after-round' ...drain.md`: fails now. `grep -q 'run_by = "worker"' ...drain.md`: fails now.
- 17: the `--accept --json` / `cN.pid` / `docs/references/` greps on drain.md: fail now.
- 18: `gate acceptance` in SKILL.md and `accepted: <commands>` in report.md: fail now.
- 19: `run_dir:` / `cN.pid` / `winpid` in verification.md: fail now.
- 20: `grep -q 'builds' ...implement-v2-milestone.js`: fails now (0 matches). It is weak, because a comment would satisfy it.
- 21: `[[sources]]` in both workers and `docs/references/` in the deep worker: fail now.
- 22: `ready_check` and `snapshot` in run-milestone.md: fail now. `snapshot` is weak, because any prose use satisfies it.
- 23: `disagreement` in visual.md: fails now.
- 24: `unverified` in questions.md: fails now.
- 25: `--baseline-gate` / `ready_check` / `manifest inspect` in write-and-prove.md: fail now.
- 26: `ui-capture.toml` and `No repo writes while a launch is open` in open-and-challenge.md: fail now.
- 27: `affected_decisions` in elaborate.md: fails now.
- 28: `--waive-review-delta` in approve.md and `review_delta` in plan-flow.md: fail now.
- 29: `llms.txt` in research-methods and `Context7 first` in plan-investigator.md: fail now.
- 30: `sources/` in finder, verifier and method: fail now.
- 31: `gate_accept` in implement-v2.md and `review_plan_digest` in plan-v2.md: fail now.
- Task 21's gate guard `bash scripts/verify-shared-blocks.sh` printed `shared-block parity: OK` (exit 0) on the present tree. It would catch finding 6 for task 30, but task 30 has no accept entry that runs it.
- Not run: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`, the gate-owned guard of tasks 16, 17, 18, 20, 22, 25, 27, 28 and 31. It is a crate build, left to the orchestrator's gate.

Other probes: `/proc/$!/winpid` printed a Windows pid under Git Bash here, so task 19's winpid premise holds. `taskkill` is on PATH, and `setsid` is absent here, which is expected for a POSIX-only branch. The flags B relies on (`--base`, `--accept` with `--json`, `--after-round`, `--baseline-gate`, `--baseline-proof`, `--waive-review-delta`) and the feature names in task 31 match milestone A's task 2 and task 15 specs. A has not landed: every A row is `pending`. `plan handover set`/`done`, `plan drift` and `plan decide --ndjson` exist today (tomlctl/src/cli/types/plan.rs). Every task cites `design/direction.md#milestone-b`, and none is a UI task.

Coverage: I read the worker views and files lists of tasks 16 to 31; milestone A's task views for tasks 2, 3, 6, 7, 8, 9, 11, 12 and 15, for flag and feature names only; and drain.md, run-milestone.md, SKILL.md, report.md, visual.md and questions.md of implement-v2-engine. I also read verification.md, implement-v2-milestone.js, the plan-worker-lite and plan-worker-deep agents, write-and-prove.md, open-and-challenge.md, ui-layout.md, elaborate.md, approve.md, plan-flow.md, research-methods/SKILL.md, plan-investigator.md, review-finder.md, review-verifier.md, review-v2-method/SKILL.md, implement-v2.md, plan-v2.md, CLAUDE.md, scripts/shared-blocks.toml and run_gate.rs, each at the sections the tasks cite. The flow's notes/ were only listed, never read. I could not check how milestone A's built verbs behave, because A is unbuilt.
