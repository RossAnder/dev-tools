Verdict: All three gaps are real. Nothing reruns a failure at the base (drain.md:367-376 mints a fix row for every `fail`). The worker contract forbids running a fix row's own reproducer while CLAUDE.md:80 allows it (plan-worker-deep.md:79, drain.md:266, schema.rs:377). A fix round always reruns the whole gate, and `--rerun` covers only held commands and timeouts (drain.md:355, :376). Smallest design: a base rerun in a detached HEAD worktree, with base failures recorded by `plan run gate` and minted outside the round count; a building `run_by = "worker"` reproducer that the pool runs one at a time; and a gate that, after a whitespace-only round, reruns only the failed commands.

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 367
claim = "every gate `fail` is minted as fix tasks; the accept is the narrowest reproducing command (370); the reason prefix `gate <id> round <n>:` is what the round count matches (372, 378-384)"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 266
claim = "a minted row's accept without `writes_build = false` is not run by the worker; the gate runs it"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 318
claim = "a failing accept check is a gate failure: it mints a fix task and costs a round"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 355
claim = "`--rerun` within a drain is only for the held commands or after a timeout; every other gate call records a new full run"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 376
claim = "after a fix round the engine relaunches and gates again in full when every row is done"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 110
claim = "the transcript audit already allows a narrower form of any row's accept check, or a check a row's detail names"

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 79
claim = "a worker runs only `Run by: worker` checks and never `Run by: gate`, with or without a covered tail (plan-worker-lite.md:79 is identical)"

[[source]]
path = "CLAUDE.md"
line = 80
claim = "the build discipline licenses a sub-agent's own narrow test, naming `cargo nextest run -E 'test(/^<file>::/)'`, the form the worker contract forbids"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 186
claim = "`covered` marks a narrow test the suite gate runs; 'a worker's own narrow run of such a test is named in `detail`', a licence the worker contract does not honour"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 377
claim = "`run_by = \"worker\"` with `writes_build = true` is refused at write; owner() defaults a building check to the gate (319-329)"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 228
claim = "gate-list skips `covered` checks and appends gate-owned accept checks to commands"

[[source]]
path = "tomlctl/src/plan/probes.rs"
line = 177
claim = "probes skip any check whose writes_build is not false, so a building worker check would still never be probed"

[[source]]
path = "claude/workflows/implement-v2-milestone.js"
line = 264
claim = "the pool already caps tasks holding crate sources with max_compile_parallel (264-271), the precedent for a build slot"

[[source]]
path = "claude/agents/verification.md"
line = 111
claim = "the narrow rerun runs in the same tree, never at another commit; the agent changes no directory beyond what a command does (125)"

[[source]]
path = "claude/skills/tomlctl/references/plan-run.md"
line = 82
claim = "`plan run gate --rerun` merges into the round's latest run, replacing only the commands it names"

[[source]]
path = ".claude/flows/implement-v2-run-hardening/plan.toml"
line = 1227
claim = "milestone A gate rounds 0/1/2: round 0 failed fmt and 3 nextest ids (538 s suite), round 1 failed only fmt with 3511/3511 passing, round 2 reran clippy, nextest (418 s) and install (232 s)"

[[source]]
path = ".claude/flows/implement-v2-run-hardening/tasks.toml"
claim = "fix tasks 17-21 and their accept entries: 18-20 carry `run_by = covered` reproducers; the `gate A round` reasons"

[[source]]
path = "docs/flows/implement-v2-run-hardening/notes/run/task-18.md"
claim = "task 18 recorded its reproducer as not-run and ran no rustfmt check"

[[source]]
path = "docs/flows/implement-v2-run-hardening/notes/run/task-4.md"
claim = "task 4 recorded its plan_emit nextest check as not-run; task-11.md likewise for plan_run_records"

[[source]]
path = "tomlctl/tests/agents_audit.rs"
line = 105
claim = "the 200-char assertion that commit 2b9163ff's `...` cut marker broke; unchanged from 2b9163ff to the milestone base, and fixed only by task 18 in b5d33317"
```

## What happened

**dev-tools, implement-v2-run-hardening milestone A** (session 7291449b, 2026-11-20 about 14:45Z to 15:09Z; base is approval commit d840a59b).
- Gate round 0 failed `cargo fmt --check` on `plan/check.rs`, and the nextest suite (538 s) failed three ids.
- One of those three, `agents_audit::audit_reports_a_bun_suite_and_cuts_long_commands`, was red before the milestone. Commit 2b9163ff, made outside any gate, added a `...` cut marker in `agents/audit.rs` without touching `tests/agents_audit.rs`, so the test expected 200 characters and got 203. Running the test at the base would settle this; I could not do that read-only.
- The orchestrator wrote that the failure was "likely there before this milestone", but it had no way to check. It minted lite fix task 18 under the `gate A round 1` prefix, alongside fix tasks 17, 19 and 20 for the milestone's own failures. The base failure therefore did not open round 1 by itself.
- Task 18's worker did not run its reproducer (its accept is `run_by = covered`, recorded `not-run`). It also did not run the rustfmt check that CLAUDE.md and the worker contract require.
- Round 1's gate passed all 3511 tests but failed fmt on task 18's new assert. That cost round 2: fix task 21, a rustfmt-only change. Round 2 then reran fmt, clippy, the full nextest suite (418 s), shared-blocks and the held install (232 s), about 7 minutes of suite for a whitespace change.
- Tasks 4 and 11, the work rows whose own tests failed (fixed by 19 and 20), each recorded their narrow nextest accept as `not-run`, because it was `covered` by the suite gate.

**partnerhub_pages, preview-cpu-trim milestone A** (session cd978dc2, gate round 0 at 14:49Z).
- `bun run test` failed two tests in files no row held: a 3000-run property test that timed out under load, and the live-Strapi query-optimisation regression test.
- The orchestrator called both "unrelated" without a base check and asked the user, who picked "fix both in A". It minted lite task 13 and deep task 14 under the `gate A round 1` reason prefix, so both counted against the round budget.
- Task 14's note shows the failure did not depend on the milestone. The test compared against its own hand-written copy of the query, whose empty `on` fragments `qs` drops. This only showed once the published article sample reached an article with those block kinds. A base rerun would have failed identically and settled it in seconds (`bun test`, no build).
- The timeout test, run alone, would most likely have passed (medium; not run). The drain gave no `rerun:` template for `bun run test`, so the verification agent's own narrow rerun could not mark it `flaky`.
- The same round's task 12 (deep: preview article pages answered 500) owned `bun run build:preview && bun run preview:smoke` as a gate-run accept. It diagnosed from wrangler's session log outside the tree and from live read-only GETs the user allowed, and recorded the smoke as `not-run`.
- By contrast, site-worker-new-strapi milestone C's task 43 (2026-11-20 morning) carried a `required` means "reproduce with build:preview then preview:smoke before editing". It ran the build and smoke before and after its fix, outside the contract's licence, and found four causes.
- Side note, out of my lens: that milestone's static-identity gate failed on noise the base already produced (fix task 11). That is the decisions-and-baseline lens's B-2053e101.

## How it works now

- **Fix rounds** (drain.md:367-384). Every `fail` block becomes fix rows: `accept` is the narrowest reproducer (:370) and `reason` opens `gate <id> round <n>:` (:372). The round count is a prefix match over the `[[changes]]` reasons (:381), so any row minted with that prefix spends a round, whether the failure is the milestone's or pre-existing. There is no step that asks whether a failure predates the milestone. The verification agent's narrow rerun (verification.md:111) runs in the same tree, so it separates flaky from real but not ours from pre-existing.
- **Accept ownership.** `owner()` gives `run_by`, else `worker` when `writes_build = false`, else `gate` (schema.rs:319-329). `run_by = "worker"` on a building check is refused (schema.rs:377). `covered` is skipped by gate-list (emit.rs:228-231); `gate` is appended to the gate's commands.
  - A fix row's reproducer is a test run, so it is always `gate` or `covered`, and the worker is told never to run either (plan-worker-deep.md:79, drain.md:266).
  - Three texts disagree. CLAUDE.md:80 licenses the task's own narrow test, naming exactly the `nextest -E` form. write-and-prove.md:186 says a worker's own narrow run of a covered test "is named in `detail`". The drain's audit already allows a narrower form of any accept check (drain.md:110). The worker contract is the only text that forbids it, and it is the one workers obey.
- **Reruns.** After every fix round the full gate runs again (drain.md:376). `--rerun` merges into the same round, only for the held commands or after a timeout (drain.md:355; plan-run.md:82). The record verb could already store a partial run; nothing in the drain permits one.

## Proposed design

**1. Base rerun (B-d7f07e99).** At gate time the milestone's work is uncommitted, so `HEAD` is the milestone's base: no new base record is needed.
- **Trigger.** A `fail` block with `failed_ids` whose command has a `rerun:` template, or any failing test command in a file no row holds.
- **Run.** The orchestrator makes a detached worktree of `HEAD` in the scratchpad (`git worktree add --detach`). This touches no working-tree file, and partnerhub's static-identity gate already uses the same pattern. It then dispatches one more `verification` call whose commands are `cd <wt> && <rerun with the ids>`.
- **Record.** `plan run gate --base` stores `base_failed_ids` per command, and a command whose failed ids all failed at the base is a base failure. The drain mints no `gate <id> round` row for it. It asks the user, as partnerhub's orchestrator did but on evidence: fix it in this milestone under a reason prefix the round count does not match (e.g. `base <id>:`), send it to the backlog, or waive the command. The waiver is the gate-waiver lens's B-686f291e record.
- **Options and tradeoffs.**
  - (a) Worktree rerun: exact, including live-data cases, but a cold build in Rust (separate target dir, deps from sccache) and a `node_modules` link for bun.
  - (b) Static heuristic, "no row touched the failing test file": free, but wrong on exactly the dev-tools case, since task 7 edited `agents/audit.rs`.
  - (c) A baseline gate run at Prove or at milestone start (sibling B-2053e101): it gives the base failure set up front, but costs a full gate every milestone.
  - Recommend (a), triggered only on failure, with (c) as the planning-time complement.
- **Narrow reruns.** Also put `rerun:` templates under bun and other test commands, so a timeout under load is reported `flaky` rather than minted.

**2. Own reproducer (B-5826b324).**
- **Schema.** Allow `run_by = "worker"` with `writes_build = true` by dropping the refusal at schema.rs:377. Probes already skip it (probes.rs:177). Gate-list skips worker checks, and the failing gate command still covers the behaviour.
- **Fix rows.** The drain mints a fix row's reproducer as that worker-owned building check (drain.md:249-252 example, :266, :370). The worker contract runs it before the edit for a diagnose row and after it for every fix row.
- **Pool.** `plan emit launch-args` marks a task `builds: true` when it holds such a check. The Workflow admits a `builds` task only when no other compiling or building task is in flight: an exclusive build slot beside `COMPILE_CAP` (implement-v2-milestone.js:264-271). The shared target is then never contended, and no sibling's half-edit is compiled under it. Cost: a fix round of building rows runs serially. Fix rows are small, and one serial round is far cheaper than a lost round of the full gate.
- **Alternatives.**
  - (b) A `required` means naming the reproducer, as task 43 had: needs no code, but leaves the contract contradiction in place and gives no serialisation.
  - (c) Run the fix rows' reproducers in an orchestrator-side verification call before the full gate: cheap and serial, but the worker still cannot iterate or diagnose.
- **Text sync.** Align plan-worker-{lite,deep}.md:79 with CLAUDE.md:80 and write-and-prove.md:186.

**3. Narrow rerun after a whitespace-only round (B-95a60315).**
- **Snapshot.** `plan run gate` stores a whitespace-normalised digest of each changed file in the milestone's rows at each failing round. Alternatively, record a `git stash create` / temp-index `write-tree` id; both write objects only, but stash-named commands trip the forbidden-ops rules.
- **Narrowing.** On the next gate, `plan emit gate-list --after-round <n>` compares the digests. When every file changed since round n differs only in whitespace, and none is a whitespace-significant kind (py, yaml, Makefile, md), it prints only round n's failed commands, plus the held ones still owed. Their results are recorded as a new round run, and the report names the commands carried as passing from round n.
- **Fallback.** Import reordering or any token change falls back to the full gate, the safe direction.
- **Alternative.** (b) An engine-judgement rule, "every fix row of the round was minted for a formatter failure": no code, but unverified, and rustfmt's import reordering is not whitespace-only.
- **Independent.** Task 18 also skipped the rustfmt check CLAUDE.md requires. A worker's note recording no format check for an edited `.rs` file is a gap the drain could flag; that is one line here, not this item's fix.

## Findings

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:367 (Fix rounds), :381 (round count)
- **Source**: drain.md:367-384; dev-tools plan.toml gate_round A r0-r2; partnerhub preview-cpu-trim tasks 13/14 reasons and session cd978dc2's question at gate round 0
- **Evidence-grade**: high
- **Finding**: needs-plan. Nothing tests a gate failure at the base commit, and a pre-existing failure is minted under the `gate <id> round` prefix, so it counts against the two-round budget. Add a worktree base rerun and a `base_failed_ids` record, and mint outside the prefix.
- **Details**: The round count matches `reason=gate <id> round ` (:381). The verification narrow rerun runs in the same tree (verification.md:111). dev-tools fix task 18 (pre-existing since 2b9163ff) and partnerhub tasks 13/14 were all minted under round 1.
- **Counter**: If the agents_audit test passed at d840a59b, the dev-tools case is not pre-existing. Running it at that commit settles this; I did not, to avoid a build.
- **Impact on plan / report**: one milestone: tomlctl `plan run gate --base` field, drain.md fix-round text, verification call shape; touches drain.md, run_gate.rs, plan-run.md, report.md.

- **Library/API or file**: claude/agents/plan-worker-deep.md:79 and plan-worker-lite.md:79; tomlctl/src/tasks/schema.rs:377
- **Source**: worker contract text; CLAUDE.md:80; write-and-prove.md:186; run notes task-4, task-11, task-18 (`not-run`); partnerhub task-12 (`not-run`) vs site-worker-new-strapi task-43 (ran build and smoke)
- **Evidence-grade**: high
- **Finding**: needs-plan. Fix and diagnose workers cannot run their own reproducer, contrary to CLAUDE.md:80. License a building `run_by = "worker"` check and give the pool an exclusive build slot.
- **Details**: The schema refuses worker plus writes_build (schema.rs:377), and drain.md:266 restates the refusal. The Workflow's `max_compile_parallel` (implement-v2-milestone.js:264-271) is the precedent for a slot keyed on a per-task `builds` flag from `plan emit launch-args` (emit.rs).
- **Counter**: If the parallel fix workers running narrow `nextest -E` in one tree routinely fail on siblings' half-edits even when serialised, the licence buys nothing. A milestone run with the slot would show this.
- **Impact on plan / report**: touches schema.rs, emit.rs (launch-args), implement-v2-milestone.js, plan-worker-{lite,deep}.md, drain.md, write-and-prove.md, tasks-store.md.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:355 and :376
- **Source**: dev-tools plan.toml gate_round A round 2 (nextest 418 s, install 232 s, after round 1's only failure was fmt); task 21's change is a rustfmt line split only (git diff d840a59b b5d33317 -- tomlctl/tests/agents_audit.rs)
- **Evidence-grade**: high
- **Finding**: A whitespace-only fix round reruns the full gate. Store whitespace-normalised digests at each failing round, and let gate-list narrow to the failed commands when nothing but whitespace changed.
- **Details**: `--rerun` is licensed only for held commands and timeouts (:355), and :376 gates again in full. The record verb already supports partial runs (plan-run.md:82).
- **Counter**: If a whitespace-only change can break a non-failed command in this toolchain (an `include_str!` byte test, a grep-based dispatch test over reflowed markdown), narrowing is unsafe. The file-kind exclusion is what answers that.
- **Impact on plan / report**: touches run_gate.rs, emit.rs (gate-list), drain.md, plan-run.md, plan-engine.md.

Coverage: read in full drain.md, verification.md, implement-v2-milestone.js (pool section), schema.rs Accept, emit.rs gate_list. Read the worker contract's build rules, CLAUDE.md build discipline, write-and-prove accept rules, the dev-tools run-hardening tasks/plan stores and run notes 4/11/18-21, partnerhub preview-cpu-trim tasks/plan stores and notes 12/14, site-worker-new-strapi task-43, and session transcripts 7291449b (dev-tools) and cd978dc2 (partnerhub) by targeted search.
- Not run: the agents_audit test at d840a59b, which needs a build.
- Ruled out: reportdesignkit and strapi-server, which carry none of the three cited incidents.
- Ruled out: blaming the partnerhub round count on task 13 alone. Its failure was likely load, but it shared round 1 with real failures.
- Consumer-repo evidence is cited in prose by repo, flow and task id, since it is outside this tree.
