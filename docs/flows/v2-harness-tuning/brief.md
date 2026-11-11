# v2-harness-tuning

## Intent

- B-4bd349ed: review-v2-pipeline single-finding verifier launches fail with API 400 because the verdict-return schema has a top-level allOf
- B-4be1dc74: a replayed review add supersedes only the cell's unverified rows, so an advisory row is written twice
- B-7b9d8057: review schedule takes a fix task's files from the finding's anchors only, so a fix that names another file or a test file leaves the worker unable to make it
- B-9728b95d: an apply fix task's only accept check is reproduce-status, which cannot pass until the orchestrator's reproduce check has run
- B-0bb55cd5: implement-v2-milestone waits for a whole wave before starting the next, so file-disjoint tasks idle behind one long task
- B-8f42ffc6: an after:<n> premise withholds its task for the whole launch, not until task n is done
- B-1bb57b0e: the reproduce check launches one Opus verifier per task even when every finding in the unit is a grep-decidable cleanup or doc row
- B-548dfa4f: plan review dispatches one verifier per finding, and its mechanical ripple cell returns thin on an unapproved plan's untracked files
- B-3a80ce63: each fixed finding adds one reproduce-status accept check to the gate, so a 32-finding milestone sends 32 commands the engine already read from the store
- B-56a8a9ff: the implement-v2 gate reruns narrow accept checks that the milestone's full-suite gate already covered
- B-590ba0b1: the reproduce check's units and the gate's accept-check list are assembled by hand from tasks and finding_links output, with no verb that emits them
- B-fddac023: the implement-v2 orchestrator hand-assembles the Workflow args and the gate prompt from row reads
- B-e2b98a71: review schedule mints apply rows with no needs on the done rows holding the same files, so plan check and tasks check print a dag/unreachable-claim warning per pair
- B-72a04d82: a reproduce-check relaunch reuses the earlier round's run_dir
- B-72f3bbac: the 450-character target did not stop sweeper cover notes being refused at the 600 cap, and finder field caps are still met only at review add
- B-4b11a10c: apply-v2-engine role rule says to ask nothing about whether to go on, but step 2 asks whether to run the plan's unfinished work first
- B-93fd1ef6: a subagent Write to a file named findings.md is refused by the harness's report-file guard
- B-690fbabb: plan review has no disposition for a finding the author fixed by hand, so those fixes are dismissed, skipped by the re-check and counted as rejections
- B-cd288c8e: most plan-review findings arrive advisory-tier with no target, expect and new, so valid findings become manual orchestrator work
- B-33070446: review digest prints duplicate_candidates and ruled_out_hits in full
- B-3b1f4f14: the verification hand-back prints every passing command in full
- B-14586282: a gate that cannot pass in this environment is found only at the first milestone gate
- B-f8331d1a: shared registration files serialise a plan's task graph and push workers to bend designs to stay in lane
- B-804db301: nothing audits what plan workers run, so gate-owned test runs go unseen unless self-reported
- B-6a0aff17: plan drift own_work() decides own work by row status, not by a set agent, so a started row reset to pending shows its own partial edits as uncommitted hits
- B-ef8adde8: implement-v2 drain step 10 and its Edits-follow-their-files paragraph compare default git status --porcelain output with rows' files, which C-quotes paths with spaces or non-ASCII characters
- B-efcdf91b: an unknown field in --template renders blank with exit 0, where --get warns
- B-c866bda9: the Workflow-script gate cannot see the logic of review-v2.js and plan-v2-review.js: agent counts, escalation threading, reverify, baseline key matching
- B-e57e08ff: the pre-commit hook costs two to four minutes per commit touching tomlctl/src, which a per-task commit train pays once per commit

## Non-goals

- The v1 carriers (`/review`, `/optimise`, `/plan-new`, `/implement`, the v1 apply commands) and their skills stay untouched.
- No change to which model or effort any agent runs at; routing a row to an agent that already exists is in scope, retuning a frontmatter is not.
- No new review lenses, plan steps or user-facing commands beyond the verbs and flags these items need.
- `lumina/`, `statusline/` and glimpse's behaviour are out; glimpse takes only the lockfile refresh a tomlctl change forces.
- No replacement of the Workflow runtime or of the store formats; additive fields only.
- Backlog items not listed under Intent, even where the same files are open.

## Scope budget

- All 29 items listed under Intent, in one plan of several milestones.
- Paths: `claude/workflows/*.js` and the script test under `claude/workflows/tests/`, the v2 engine, method and tomlctl skills under `claude/skills/`, the v2 agents under `claude/agents/` and the shared `claude/agents/verification.md`, the v2 carriers `claude/commands/implement-v2.md` and `claude/commands/apply-v2.md`, `tomlctl/src/**`, `tomlctl/tests/**`, `tomlctl/README.md`, `.githooks/`, `scripts/`, and `CLAUDE.md` where a documented contract changes.
- An item whose fix turns out to need a design larger than its siblings (the Workflow-script test harness, the worker audit, per-verb registration) may be cut to its smallest useful step, with the remainder returned to the backlog and named at approval.

## Success criteria

Failures that stop a run:

- A single-finding or panel verifier launched by `review-v2-pipeline` is accepted by the API and records its verdict (B-4bd349ed).
- Replaying a full `review add` for a cell leaves one copy of every row, advisory rows included (B-4be1dc74).
- A scheduled fix task holds every file its finding's fix names, test files included (B-7b9d8057).
- Every accept check a worker is shown is one the worker can run and pass; a check only the gate can pass is marked as the gate's (B-9728b95d).
- An agent can write a file named for the findings reference without the harness refusing it (B-93fd1ef6).

Wall time and agent cost:

- In a milestone launch, a ready file-disjoint task starts as soon as a slot frees, and a task held by an `after:<n>` premise starts in the same launch once task n is done (B-0bb55cd5, B-8f42ffc6).
- The reproduce check settles units made only of grep-decidable cleanup or doc findings without one Opus verifier each (B-1bb57b0e).
- A plan review verifies a cell's findings in batches, and its mechanical cell either reads the plan's untracked files or is not run (B-548dfa4f).
- A milestone's gate proves every fixed finding gone with one check, not one per finding (B-3a80ce63).
- An accept check the plan marks as covered by a milestone gate is not run again at that gate (B-56a8a9ff).
- A commit train over an already gated tree does not pay the full pre-commit test and clippy cost per commit, and no step tells the committer to pass `--no-verify` (B-e57e08ff).

Orchestrator hand work and context:

- The milestone launch args, the reproduce units and the gate's command list each come from one `tomlctl` call, with no piping or hand assembly (B-590ba0b1, B-fddac023).
- Scheduling fix rows into a flow adds no `dag/unreachable-claim` warning for a file last held by a done, committed row (B-e2b98a71).
- A reproduce-check relaunch cannot read a payload an earlier round wrote (B-72a04d82).
- A sweeper's cover note over its cap is cut and reported rather than refused, and a finder learns which rows `review add` would refuse before the write that would refuse them (B-72f3bbac).
- A plan-review finding the author fixed by hand has its own disposition: it is counted apart from dismissals and its edited targets reach the re-check (B-690fbabb).
- A confirmed plan-review finding that carries an edit can be written by `review apply` (B-cd288c8e).
- `review digest` gives counts for `duplicate_candidates` and `ruled_out_hits` by default (B-33070446).
- The verification hand-back gives one line per passing command (B-3b1f4f14).
- A gate command the plan stored a probe for, and that cannot pass in this environment, is found before the first worker is launched (B-14586282).
- A worker that fails for lack of a file names the files it needs, and the drain can widen the row from that return (B-f8331d1a).
- Gate-owned test runs and root-recursive greps by plan workers are visible to the orchestrator without the worker reporting them (B-804db301).

Correctness of tools and prose:

- `plan drift` treats a row with a set agent and no commit as own work whatever its status (B-6a0aff17).
- The drain's path comparisons hold for paths with spaces or non-ASCII characters (B-ef8adde8).
- A `--template` path that matches no row is warned of on stderr, unless its first segment is an optional field a documented template names, which stays quiet (B-efcdf91b).
- The `apply-v2-engine` role rule and its step 2 no longer contradict (B-4b11a10c).
- The logic of `review-v2.js` and `plan-v2-review.js` is exercised by a test the hook or the suite runs (B-c866bda9).

## Background

**B-4bd349ed.** Run U3 of v2-apply: all five single-finding verifier launches were rejected with a 400 saying input_schema does not support oneOf, allOf or anyOf at the top level, leaving their findings unverified. review-v2.js passes schemas.verdict_return straight to agent() for single and panel verifiers; batches wrap it under verdicts and work. tomlctl review schema verdict-return emits a top-level allOf for the unblocks/fix_sketch pairing. Workaround: pass the schema to the Workflow with allOf removed, or mop up the unverified ids by direct Agent dispatch. Fix either in schema_out.rs or by wrapping the single return in the script.

Evidence: `notes/backlog-B-4bd349ed.md`

**B-4be1dc74.** Run U3 of v2-apply, cell C27: the first add landed F109 to F116 and refused one row on a 162-character summary; the method skill says to fix and resubmit the full set once, and the replay superseded F109 to F115 but left advisory F116 beside its copy F125. The supersede filter in add.rs keeps only rows whose status is Unverified, and an advisory row is born open. The orchestrator then links the pair by hand and review resolve keeps reporting it pending with verdicts incomplete, since advisory rows get no verdict. Until fixed, after any replayed add check the cell for a doubled advisory and park one.

Evidence: `notes/backlog-B-4be1dc74.md`

**B-7b9d8057.** Seen twice in the v2-apply apply-1 run. F131's fix also named implement-v2-engine/SKILL.md, outside its task's files, which cost an extra minted task and launch. F87, F92 and F97 named integration test files under tomlctl/tests, so the worker wrote unit tests inside the modules instead. Until fixed, read each would-mint row's files against its finding's fix text at the apply preview and widen files through an audited update before launch.

Evidence: `notes/backlog-B-7b9d8057.md`

**B-9728b95d.** v2-apply apply-1: the worker view printed tomlctl review reproduce-status <id> --expect gone as each fix task's Accept, with Writes build yes. The verdict it reads is recorded only by the reproduce check that runs after every worker has returned, so the worker has no acceptance it can run. Task 52's worker ran it, got exit 1 with a null verdict, and marked the task done anyway; the other 14 skipped it. A stricter worker would return failed. Options: have the worker view mark the check as run by the gate, or have review schedule add a worker-runnable check beside it. Related to B-3a80ce63, which covers the same checks' cost at the gate.

Evidence: `notes/backlog-B-9728b95d.md`

**B-0bb55cd5.** v2-apply apply-1, run wf_62701b38-ed4, max_parallel 4: wave 1 held deep task 41 (12 findings, about 7 minutes) beside three lite tasks that finished in about 20 seconds, and the other 11 file-disjoint tasks waited for it. The run took about 9.5 minutes where a rolling pool, starting the next ready task as a slot frees, would have taken about 7. The script awaits parallel() over the wave. Same family as B-8f42ffc6. Until fixed, nothing to do at the call site; a milestone with one large task and many small ones pays the wait.

Evidence: `notes/backlog-B-0bb55cd5.md`

**B-8f42ffc6.** Milestones A and C of v2-apply each needed three launches before the gate, an estimated 8 minutes. Evaluating after: premises between waves inside the Workflow would remove the relaunch.

Evidence: `notes/backlog-B-8f42ffc6.md`

**B-1bb57b0e.** v2-apply apply-1, run wf_d54910b8-e81: 15 review-verifier launches, each starting at about 45 thousand tokens of context before any work, and 12 of them finished in 11 to 27 seconds on doc or cleanup rows whose removed text a grep settles. The review wave already routes such rows to the pooled Haiku review-checker; reproduce mode has no such route. Options: route units whose findings are all cleanup or doc rows to the checker, or pool several small units into one verifier. Same shape as B-548dfa4f on the plan side.

Evidence: `notes/backlog-B-1bb57b0e.md`

**B-548dfa4f.** Two v2-apply plan review runs used 83 agents and about 6.5M tokens, 60 percent of planning wall time; 56 were verifier dispatches at about 60k context each. Batch verifiers per cell, and in flow mode skip the ripple cell or point it at the untracked files.

Evidence: `notes/backlog-B-548dfa4f.md`

**B-3a80ce63.** The v2-apply apply-1 gate ran 32 reproduce-status commands through the verification agent right after the reproduce check read the same verdicts. One aggregate check per milestone, expecting every fixed finding gone, would keep the guarantee that a milestone cannot commit with a finding outstanding.

Evidence: `notes/backlog-B-3a80ce63.md`

**B-56a8a9ff.** Seen 2026-11-10: milestone A's gates open with the full nextest suite, and drain.md step 7 then appends every writes_build accept check, leaving out only byte-identical commands. Six narrow cargo test and nextest subsets ran after the suite that contains them. Cheap here at a few seconds, costly on a slow crate. A subset cannot be detected mechanically in general. Decided by the user 2026-11-10: fix it at plan time with a marker, an accept-entry field naming the gate that covers the check, written at Write and honoured mechanically at the drain; a drain-time judgement rule was declined. That needs tomlctl schema, plan check and worker-view work, so it goes through a plan, not a direct fix.

Evidence: `notes/backlog-B-56a8a9ff.md`

**B-590ba0b1.** In the v2-apply apply-1 run the orchestrator piped tomlctl output through a script to build both, which the tomlctl guidance canon forbids. The by-hand rules are findings.md's highest-id row owns a finding several rows fix, and drain.md step 7's writes_build filter with de-duplication. A verb emitting the reproduce args object for a flow and milestone, and one emitting the gate's command list, would remove both.

Evidence: `notes/backlog-B-590ba0b1.md`

**B-fddac023.** It read spec.accept for twelve tasks (12k characters) and wrote the launch args twice per launch. tasks ready has no flag that emits the args object or the gate command list.

Evidence: `notes/backlog-B-fddac023.md`

**B-e2b98a71.** Scheduling 15 rows into v2-apply made plan check and tasks check each print about 50 warnings, which buries any real one. drain.md's rule for minted rows, needs follows the files, is not applied by the verb. Either schedule adds the latest holder of each file to needs, or the check skips a pair whose earlier row is done with a commit. The warnings are harmless meanwhile: exit status stays 0.

Evidence: `notes/backlog-B-e2b98a71.md`

**B-72a04d82.** Dropped during the v2-apply run as harmless today. Check whether a later round can read a stale payload from the earlier one.

Evidence: `notes/backlog-B-72a04d82.md`

**B-72f3bbac.** Run U3 of v2-apply, after B-00ae0571's fix: verifier rationales were no longer refused, but all three Haiku sweepers were (cover notes of 680, 737 and 897 characters, the last refused again at 628), and two finders were refused at review add (failure_scenario at 617 against 600, summary at 162 against 160). The summary refusal forced the full-set replay that doubled an advisory row. B-00ae0571's other options were never taken: a truncate flag that cuts at the cap and says so, or a larger cap. A dry-run length check before the write would also do.

Evidence: `notes/backlog-B-72f3bbac.md`

**B-4b11a10c.** The Triage decided the selection rule says every condition the skill cannot settle is a stop, never a question; the unfinished-milestone check added afterwards asks with AskUserQuestion. Reword the rule to name that one exception, or the two lines contradict. Escalated by review run U3 but outside its bounds, since the question was added after the run's head.

Evidence: `notes/backlog-B-4b11a10c.md`

**B-93fd1ef6.** The worker creating the reference worked around it with touch plus Edit. Rename the reference or note the collision.

Evidence: `notes/backlog-B-93fd1ef6.md`

**B-690fbabb.** Seen in the v2-apply planning session: 37 advisory-tier and re-check findings were hand-fixed then dismissed, against plan-flow.md sections 7 and 10. The re-check's targets held only review apply rows, and review stats --plan-outcomes shows them as dismissed. An 'addressed by author edit' disposition in review triage would record the edited targets, count separately and feed the re-check.

Evidence: `notes/backlog-B-690fbabb.md`

**B-cd288c8e.** 22 of 39 open findings in the first v2-apply plan review and 21 of 23 in the re-check. Options: have finders and plan-executor supply the edit fields, or have the lead convert at settle.

Evidence: `notes/backlog-B-cd288c8e.md`

**B-33070446.** One plan-review digest was 16k characters, mostly those two arrays. Collapsing them to counts by default would cut it.

Evidence: `notes/backlog-B-33070446.md`

**B-3b1f4f14.** A 31-command gate returned 13.5k characters, mostly pass blocks. One line per passing command would do.

Evidence: `notes/backlog-B-3b1f4f14.md`

**B-14586282.** scripts/link-v2-review.sh exits 1 where the config dir holds copies, which caused both user questions and all three handovers of v2-apply. Probe each gate command at Prove or at the implement-v2 pre-flight, with --dry-run where the command has one.

Evidence: `notes/backlog-B-14586282.md`

**B-f8331d1a.** In v2-apply review/dispatch.rs and cli/types/review/* chained the graph to 1.26 effective parallelism; one task failed for lacking dispatch.rs, and three passed loose fields instead of an args struct because the re-export sat in a file they did not hold. Done 2026-11-11: write-and-prove.md tells the planner to claim a registration file on every task that registers there. Still open: letting a failed worker return the files it needs so the drain widens the row mechanically, and any restructuring that removes the shared file from the lane, such as per-verb registration.

Evidence: `notes/backlog-B-f8331d1a.md`

**B-804db301.** 5 of 42 v2-apply workers ran test binaries the gate owns and one ran a recursive grep from the root; the orchestrator saw only the one that reported itself.

Evidence: `notes/backlog-B-804db301.md`

**B-6a0aff17.** drain.md step 3 explains the case in prose. Matching the pre-flight's started-row rule (no commit, agent set) in the tool would remove the special case.

Evidence: `notes/backlog-B-6a0aff17.md`

**B-ef8adde8.** The pre-flight was moved to `tomlctl plan drift --uncommitted --get uncommitted_files`, which prints unquoted paths. The drain's `git status --porcelain -- <paths>` reads still quote such paths, so one would not match a row's `files` spelling or pass to `git add`. Arrived cheap-in-file from the review-apply run and was deferred: the replacement read for a pathspec-scoped status needs choosing, and no verification step exercises that prose.

Evidence: `notes/backlog-B-ef8adde8.md`

**B-efcdf91b.** Reproduce with tasks list --template '{id} [{message}]'. A plan check call used {message} for the detail field and printed blank lines, needing a second call. Looked at 2026-11-11: validate_row_paths in convert.rs passes when any one path matches, by design, because rows are sparse and documented idioms name optional fields (backlog add --template '{action} {id} {advisories}'). A warning for every path absent from all rows would fire on those. A right fix needs each verb's field vocabulary, so a path outside it is refused and an optional one stays quiet.

Evidence: `notes/backlog-B-efcdf91b.md`

**B-c866bda9.** command_lint in tomlctl/src/cli/dispatch/tests/lint.rs only checks that tomlctl backtick spans parse under clap and that launches name saved scripts. The scripts' own logic is pure functions of agent returns, so a small JS harness with stubbed agent/parallel would cover it, but the runtime (node --test, bun) and the stub shape need a decision first. Until then drift between the scripts and agent-waves.md is caught by reading only. Moved from review ledger item R24 of flow review-v2-first-run-repairs at the user's instruction.

Evidence: `notes/backlog-B-c866bda9.md`

**B-e57e08ff.** Each such commit reruns the cli::dispatch::tests subset (one test alone takes over a minute) and glimpse clippy. An 11-commit train took about 40 minutes of hook time on an already gated tree. Workaround used: gate once with the hook's own checks added (the two parity verifiers, fmt, the full suite, glimpse clippy --locked), then commit the train with --no-verify.

Evidence: `notes/backlog-B-e57e08ff.md`

## Unknowns

- wave-scheduling: How does claude/workflows/implement-v2-milestone.js form and await its waves and withhold after:<n> tasks, and what would a rolling pool that starts each ready file-disjoint task as a slot frees and releases an after:<n> task once task n is done need from the Workflow script API and from the engine's launch and drain steps (B-0bb55cd5, B-8f42ffc6)?
- verifier-cost: In claude/workflows/review-v2.js and plan-v2-review.js, how are reproduce units, plan-review verifier dispatches and the ripple cell built, why does the single verdict-return schema reach agent() with a top-level allOf, can a later reproduce round read an earlier round's run_dir payload, and what is the smallest change that routes grep-decidable units to review-checker and batches plan-review verifiers per cell (B-4bd349ed, B-1bb57b0e, B-548dfa4f, B-72a04d82)?
- schedule-rows: In tomlctl review schedule, where do a minted fix row's files, needs and accept checks come from, what in a finding's stored fields names the other files and test files its fix touches, and what would it take to add those files, a needs edge on the latest done holder of each file, and one aggregate reproduce check per milestone that the worker view marks as the gate's (B-7b9d8057, B-9728b95d, B-3a80ce63, B-e2b98a71)?
- gate-assembly: Which values does the implement-v2 orchestrator assemble by hand for the milestone launch args, the reproduce units and the gate command list per implement-v2-engine's run-milestone.md and drain.md and apply-v2-engine, which store fields hold each, and what verb or flag shapes on tomlctl tasks, plan and review would emit them whole, including an accept-entry field naming the covering gate and a pre-launch probe of each gate command (B-590ba0b1, B-fddac023, B-56a8a9ff, B-14586282)?
- plan-review-outcomes: In a flow-mode plan review, where do findings lose target, expect and new between finder, plan-executor, lead settle and review apply, what dispositions does review triage record and how does the re-check choose its targets, and what would an addressed-by-author disposition and edit fields supplied at the source each need in the store, the verbs and the agents' instructions (B-690fbabb, B-cd288c8e)?
- script-harness: What do review-v2.js and plan-v2-review.js need stubbed (agent, parallel, pipeline, phase, args) for their logic to run outside the Workflow runtime, which of node --test and bun test is installed here and on the Windows machine the hook also runs on, and how would the pre-commit hook or the tomlctl suite invoke such a test (B-c866bda9)?
