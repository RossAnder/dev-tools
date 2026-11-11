Verdict: a worker could carry out milestone E mostly as written, but 4 points would stop one: 3 unactionable (task 33 twice, task 34 once) and 1 false premise (task 36). Tasks 35, 37, 38 and 39 are actionable as written.

# Cold read, milestone E (tasks 33 to 39)

## Unactionable

### U1. Task 33, Detail (1): the gate probe is placed in pre-flight, before the milestone is known

The Detail says: "Pre-flight, step 1: after the present checks, emit the milestone's gate probes with `tomlctl plan probes --slug <slug> --emit --gates --milestone <id>`". In `claude/skills/implement-v2-engine/references/run-milestone.md`, section 1 (Pre-flight, lines 7-54) runs before section 2 (Pick the milestone, lines 56-75). Line 58 is where `<id>` gets chosen: "The milestone is the first checkpoint, in store order, holding a row that is not `done`." Without `--milestone`, nothing in section 1 knows `<id>`. The worker has to pick one of three fixes, and the plan settles none of them:
- move the probe to after the pick;
- emit every milestone's gates without `--milestone`, which `design/shapes.md#gate-probes` allows (`[--milestone <id>]`);
- pick the milestone early.

### U2. Task 33, Detail (3): how to withhold a task after `plan emit launch-args` has built the args

The Detail says: "read every `after_checks` command before launch ... and withhold a task whose after: check does anything but read, or builds", and then "Write `args` ... pass it to the Workflow, and record the launch with `plan run start` using `args_sha256` as `--args-hash`". `design/shapes.md#launch-args` (lines 48-64) gives the verb no flag to exclude a task. A task withheld at this point has to be cut from `args` by hand, along with every member that needs it or names it in `after`. That breaks the "one call" outcome and leaves `args_sha256` describing args that were never launched. The worker would have to decide alone:
- edit args by hand and re-hash;
- stop and replan the premise and re-emit, which is what section 3 does today for a premise line that writes (run-milestone.md:89 "stop and replan that premise rather than run it");
- or something else.

### U3. Task 34, Detail (1): where "the transcript directory the launch reported" comes from

The Detail says: "run `tomlctl agents audit --transcripts <dir>` with the transcript directory the launch reported". Nothing in the engine or the Workflow reports such a directory:
- The index shape is `{slug, milestone, tasks: [{id, status, note, error, deviations, handover}], errors, script_version}` (run-milestone.md:214, drain.md:27).
- The `implement-v2-milestone.js` changes in task 26 (per its worker view) add only `needs_files` to the index.
- `rg -i transcript` over `claude/workflows/*.js` returns nothing.

On this host the directory is `<config dir>/projects/<project>/<session>/subagents/workflows/<runId>/`, which holds `agent-*.jsonl`. The plan never states that path, how to derive it from `runId` and the session, or that the Workflow notification carries it. The worker would have to guess what to write into `drain.md`.

A related point, not counted: `claude/skills/implement-v2-engine/SKILL.md:16` says "You never read a worker's transcript". Task 36 adds the audit to SKILL.md without saying how it squares with that rule. A worker can likely reason that the tomlctl verb reads the transcripts and the orchestrator does not, but neither task says so.

## False premises

### F1. Task 36, Detail (3): apply-v2-engine/SKILL.md has no by-hand widening instruction and no fix-row accept-check description

The Detail says: "at the preview, the instruction to read each would-mint row's files against its finding's fix text and widen by hand is replaced by the finding's `fix_files`, with the by-hand widening kept ...; where it describes a fix row's accept check, say it is the one milestone-wide, gate-owned `reproduce-status` check."

What the file holds:
- The preview in `claude/skills/apply-v2-engine/SKILL.md` (lines 68-109) has a dry run, a findings read, then the Blocked, Already-scheduled, Nothing-to-schedule, Dirty-files and Unfinished-plan bullets. No line reads files against fix text, and none widens.
- `rg -n -i 'widen|fix text|accept|reproduce'` over the file finds only lines 173 and 241, and both mean the user accepting settle proposals. Neither is a fix row's accept check.
- The only other task in the plan that edits this file is task 6, milestone A: "Name the one question in the apply-v2 role rule". It does not add that text.

So there is nothing to replace. The worker would have to invent where `fix_files` goes, for example in the Dirty-files bullet, which collects `would-mint` files. The accept `grep -q 'fix_files'` passes wherever the word lands.

## Check commands

Each was run on the present tree from the repo root. Every forward check fails now, as it should before the change. The one guard check passes.

| Task | Command | Result now |
|---|---|---|
| 33 | `grep -q 'plan emit launch-args' claude/skills/implement-v2-engine/references/run-milestone.md` | rc=1 |
| 33 | `grep -q -- '--emit --gates' .../run-milestone.md` | rc=1 |
| 33 | `grep -q -- '--tasks' .../run-milestone.md` | rc=1 (weak: any `--tasks` anywhere satisfies it) |
| 34 | `grep -q 'plan emit gate-list' .../drain.md` | rc=1 |
| 34 | `grep -q 'agents audit --transcripts' .../drain.md` | rc=1 |
| 34 | `grep -q 'needs_files' .../drain.md` | rc=1 |
| 34 | `! grep -Eq 'git status --porcelain -- <paths?>' .../drain.md` | rc=1 (matches drain.md:207, 339, 342, 380; the flow-store read at :371 does not match, as intended) |
| 34 premise | `grep -Eq 'git status --porcelain -- <paths?>' .../drain.md` | rc=0, holds |
| 35 | `grep -q 'plan emit reproduce-units' .../findings.md` | rc=1 |
| 35 | `grep -q -- '--relaunch' .../findings.md` | rc=1 |
| 35 | `grep -q 'reproduce-status --flow' .../findings.md` | rc=1 |
| 36 | `grep -q 'plan_emit' claude/commands/implement-v2.md && grep -q 'accept_run_by' claude/commands/implement-v2.md` | rc=1 |
| 36 | `grep -q 'reproduce_status_milestone' claude/commands/apply-v2.md && grep -q 'review_fix_files' claude/commands/apply-v2.md` | rc=1 |
| 36 | `grep -q 'fix_files' claude/skills/apply-v2-engine/SKILL.md` | rc=1 |
| 36 | `grep -q 'rolling pool' CLAUDE.md` | rc=1 |
| 37 | `grep -q 'covered_by' .../write-and-prove.md && grep -q 'run_by' .../write-and-prove.md` | rc=1 |
| 37 | `grep -q -- '--emit --gates' .../write-and-prove.md` | rc=1 |
| 38 | `test -f scripts/gate-stamp.sh && bash -n scripts/gate-stamp.sh` | rc=1 |
| 38 | `grep -q 'gate-stamp.sh' .githooks/pre-commit` | rc=1 |
| 38 | `test -f scripts/gate-stamp.sh && test ! -f "$(git rev-parse --git-dir)/gate-stamp" && ! bash scripts/gate-stamp.sh check` | rc=1 (the script is absent; no `.git/gate-stamp` exists. Once a stamp is written this check fails whatever the code does, so it discriminates only on a stamp-free tree) |
| 38 | `grep -q 'gate-stamp.sh' CLAUDE.md` | rc=1 |
| 38 guard | `bash -n .githooks/pre-commit` | rc=0 |
| 39 | `test -f .../apply-rows.md && test ! -e .../findings.md` | rc=1 |
| 39 | `! grep -rq 'findings\.md' claude/skills/implement-v2-engine` | rc=1 (17 mentions, all in SKILL.md, drain.md and run-milestone.md) |
| 39 premise | `! grep -rl 'findings\.md' claude CLAUDE.md scripts tomlctl/src tomlctl/README.md \| grep -qv '^claude/skills/implement-v2-engine/'` | rc=0, holds. The wider sweep over `tomlctl/tests`, `.claude/skills`, `.githooks` and `glimpse/src` also finds nothing outside the engine |

Not run: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`, the check that tasks 33-37 and 39 name. It builds the crate and is left to the orchestrator's gate.

## Premises checked and found true

- run-milestone.md has the section-3 skip table (lines 93-100) and section 4, "Build the args": `tasks closure` and `tasks show` reads plus six numbered rules (lines 136-152). `plan run start` is at line 205.
- drain.md has the sentence task 34 (2) deletes (line 61: "`plan drift` counts only rows past `pending` as their own work ..."). It also has step 6's `failed` row (line 115), step 7's milestone and accept reads with by-hand de-duplication (lines 225-247), and the held-command, `timeout:`, `rerun:` and `transient:` rules (lines 262-264). drain.md never says "waves", so that part of task 34's Detail has nothing to change.
- findings.md "Reproduce check" has every element task 35 replaces: the highest-id-owner rule (:76), the per-store split (:82), the round computation (:84-88), the later-pass per-finding loop (:78), the per-finding `reproduce-status` reads (:117-125), the per-finding fix-row accept (:160) and the guarantee sentence (:172). The milestone-wide call's output carries each finding's `verdict` (`claude/skills/tomlctl/references/review-apply.md:139`), so one call can replace the `--expect reproduces` second read.
- implement-v2-engine/SKILL.md has the step-table rows Premises, Launch, Reproduce check and Gate (:52-57), the Workflow description "per wave" (:79) and the "Vetting and harvest" bullet (:78, which sits under "Out of scope").
- Both carriers' pre-flight feature lists exist (implement-v2.md:29, apply-v2.md:31). CLAUDE.md's Implement v2 section has the Workflow, Gate and commit, and Deploy bullets, and Developer setup holds the Supply chain gotcha.
- write-and-prove.md has the Milestones paragraph in section 1 (:47), the `writes_build` rule in section 3 (:158) and section 4d Probes (:347).
- `.githooks/pre-commit` has a `gate()` function (:42) and exactly the five cargo steps task 38 lists, in that order (:53, :59, :64, :69, :79). The verifiers, the doc gate and the sidecar comparison all run before them, and `sha256sum` is already used (:29).
- `design/shapes.md` carries every anchor the tasks cite: launch-args, reproduce-units, gate-list, gate-probes, worker-return, worker-audit, reproduce-checker, reproduce-status-by-milestone, accept-owner, gate-stamp.
- The feature tokens task 36 adds (`plan_emit`, `accept_run_by`, `gate_probes`, `agents_audit`, `reproduce_status_milestone`, `review_fix_files`) are minted by tasks in earlier milestones, per the store.

## Coverage

- Read all seven worker views and `--select id,files` for tasks 33-39.
- Read in full: `run-milestone.md`, `drain.md`, `design/shapes.md` and `.githooks/pre-commit`.
- Read in part: `findings.md` (lines 1-175), `implement-v2-engine/SKILL.md` (step table, out of scope), `apply-v2-engine/SKILL.md` (preview, lines 68-109), `write-and-prove.md` (headings and the rules cited) and the carriers' pre-flight lines.
- Looked only at the Detail and Files of tasks 24 and 26, and the files of tasks 6, 8, 18, 20 and 21, to settle U3 and F1.
- Not checked: the `cli::dispatch::tests` run, which needs a crate build, and what the Workflow tool's notification actually exposes about a transcript directory.
