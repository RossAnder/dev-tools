Verdict: Neither gap is covered: drain.md:384 leaves a gate red after round 2 uncommitted with no ask and no record, and verification.md:90 detaches the gate with no pid kept in a place the orchestrator can find; a `[[gate_accept]]` cloned from `[[visual_accept]]` (run_visual.rs:56) plus an orchestrator-named run dir with `cN.pid` would close both.

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 384
claim = "a gate failing after round 2 mints nothing, leaves the milestone uncommitted and reports needs_human; no user offer, no record"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 427
claim = "the commit runs on a passed gate, or on a [[visual_accept]] row for a UI milestone; no gate counterpart"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 473
claim = "re-entry on an unstamped milestone re-runs the gate from step 7; a waiver would need the same skip visual.md gives its accept"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 363
claim = "only a second timeout ends the gate other than a fail; nothing covers a gate agent stopped by the user"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 225
claim = "the visual accept: AskUserQuestion with accept / leave-uncommitted, backlog item first, plan run visual --accept, then commit"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 38
claim = "visual pass re-entry reads [[visual_accept]] for the round via items list and skips to the commit"

[[source]]
path = "claude/skills/implement-v2-engine/references/report.md"
line = 34
claim = "report line for an accepted visual round, needs_person false (report.md:86)"

[[source]]
path = "claude/skills/implement-v2-engine/references/report.md"
line = 29
claim = "the only gate line for a red last round is 'failed after round n'"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 61
claim = "Commit step skip condition names the gate pass and [[visual_accept]] only"

[[source]]
path = "tomlctl/src/plan/run_visual.rs"
line = 56
claim = "plan run visual --accept builds VisualAccept and upserts it on (milestone, round) under store::mutate"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 539
claim = "VisualAccept fields: milestone, round, at, why, states, looks, defects, backlog"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 39
claim = "a store holding a visual acceptance is stamped schema 6; a gate acceptance would need a 7 and a reader bump"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 490
claim = "GateRound carries milestone, round, seq, at, outcome, commands: the failed commands a waiver names are already on record"

[[source]]
path = "claude/skills/tomlctl/references/plan-run.md"
line = 116
claim = "plan run visual contract: keyed (milestone, round), cannot know which states failed"

[[source]]
path = "claude/agents/verification.md"
line = 90
claim = "Step 1b launches the command with nohup ... & and keeps only cN.log and cN.exit; no pid is written"

[[source]]
path = "claude/agents/verification.md"
line = 38
claim = "the scratch dir D comes from mktemp inside the agent, so the orchestrator cannot find it once the agent is stopped"
```

## Answer

**Waiver.** reportdesignkit flow `package-design-tools` hit the round cap three times and each time the orchestrator improvised past drain.md:384:

- **E** (session 6ec4c43a, 2026-11-16): gate round 2 red at 13:14Z on a load flake; the 13:15Z report stopped uncommitted. At 13:28Z the user said, in effect, fix the single tests and commit. The orchestrator minted rounds 3, 4 and 5, each fix row's `reason` carrying "user-authorized 2026-11-16" as free text, and committed after round 5 passed at 04:45Z the next day (work 9c1e1c52). That authorisation exists only inside reason strings.
- **H** (session 914ec3a0, 2026-11-17): round 2 red at 11:37Z with apps/server timeouts and two browser cases, under another session's benchmark load. An AskUserQuestion at 11:39Z offered re-gate, wait, or commit on the solo runs, describing the last option as "not what the engine allows". The user picked commit. The 11:40Z report holds one `gate` entry with needs_person true. There is no backlog follow-up and nothing in the store says the commit rode a red gate.
- **J** (session fad99714, 2026-11-18): a third case the backlog row does not list. Round 2 was red at 13:44Z. At the user's call the three failing files were run alone, merged into round 2's `gate_round` with "user-approved flake check" in their summaries, and J was committed as 487c6038. This time flaky backlog rows (B-3e0333ca, B-81f703fa) were minted, but ad hoc. The round's stored `outcome` is still `fail`.

**Visual accept as run-hardening built it.** It works in four steps:

1. The orchestrator asks the user with two options (visual.md:225).
2. A backlog item is captured first, of kind debt, taken from `backlog check` or `backlog add` (visual.md:232).
3. `plan run visual --accept` upserts `[[visual_accept]]` keyed on (milestone, round), with why, states, looks, defects and backlog (run_visual.rs:56, schema.rs:539). The store is then stamped schema 6 (schema.rs:39).
4. The commit proceeds on the row (drain.md:427, SKILL.md:61). Re-entry reads the row with `items list --array visual_accept` and skips to the commit (visual.md:38, :264). The report prints `visual round <r> accepted: <states>; backlog <B-id>` with needs_person false (report.md:34, :86).

**The gate has none of these.** The gate side has no ask, no record and no commit path. Its only report line is `failed after round <n>` (report.md:29). Re-entry on an unstamped milestone re-runs the gate (drain.md:473), so even a recorded waiver would be re-gated.

**Stopped gate (F, session 201a3cf0, 2026-11-17).** The sequence:

- 06:14:55Z: F's round-1 gate went out to `verification` (agent a7b35de0f11d58840) with a 1500 s budget, so Step 1b applied. The agent launched `bun run gates` detached under nohup.
- About 06:17Z: the user interrupted the agent. The orchestrator found the orphan tree by command line, gave the user a kill command and paused.
- 07:06Z: on "continue", it found that the orphan's browser connection had dropped after 28 of 148 test files and that coverage was hung. It killed the tree and checked the project's lock file before re-gating at 07:08Z. That round passed at 07:14Z. About 52 minutes were lost from launch to kill.

The project's lock is already stale-checked by pid (its gates-lock module), so the lock cleared itself once the tree died. What cost the time was the live orphan, not the lock file. verification.md:90 records only `cN.log` and `cN.exit`, and `<D>` is a mktemp directory inside the agent (verification.md:38), so a stopped agent leaves nothing the orchestrator can use to find the process. No drain or SKILL row covers a stopped gate agent (drain.md:363 covers timeouts only).

## Proposed design

**Gate waiver.**

- **Shape.** Add a `[[gate_accept]]` record keyed on (milestone, round) with `why`, `commands` and `backlog`. `commands` is a list of `{command, failed_ids}`. Write it with `tomlctl plan run gate --accept --round <n> --json @<path>`, or with a sibling leaf `plan run waive`.
- **What the verb checks.** Unlike the visual verb, this one can check its payload. Every waived command must be a `fail` in the latest `gate_round` of that round, and a payload that leaves out a failing command is refused.
- **Engine changes.**
  - drain.md:384 asks first: accept and commit / leave uncommitted.
  - SKILL.md:61 and drain.md:427 take the row as a pass.
  - drain.md:473 skips the re-gate when the row exists.
  - report.md gains `gate round <n> accepted: <commands>; backlog <B-id>`.
  - Schema 7 is needed, and the pre-flight must refuse a binary without the new verb.
- **Option for E.** A third answer, "one more round", recorded as `kind = "round"` on the same record. This replaces the free-text "user-authorized" in reason strings, and the round counter at drain.md:381 reads it to raise the cap by one. The tradeoff: it is more vocabulary, but E is exactly that case.
- **Rejected.**
  - Mutating `GateRound.outcome` to `waived`: the gate record is a log of runs, and the plan says it counts nothing (drain.md:361).
  - Merging with `[[visual_accept]]` into one `[[waiver]]`: the brief's non-goals say run-hardening's deliverable is built on, not reworked.

**Orphan cleanup.**

- **Run dir.** The orchestrator passes a `run_dir:` input, for example `<scratch>/gate-<id>-<n>/`, and the agent uses it in place of mktemp. A stopped agent's files are then findable.
- **Pid file.** Step 1b writes `cN.pid` right after the launch. On MSYS/Git Bash it must hold the Windows pid read from `/proc/$!/winpid`: `$!` is an MSYS pid (probe: msys 770 mapped to win 33212) and cannot reach native `bun.exe` children. On POSIX, launch under `setsid` and store the process group id.
- **Cleanup rule.** A new drain row, "gate agent stopped". For each `cN.pid` with no `cN.exit`, kill the tree (`taskkill //PID <win> //T //F` on Windows, `kill -TERM -<pgid>` on POSIX) and confirm it is gone. Record one `reconcile` report entry and re-dispatch the same round with `--rerun` semantics.
- **Lock.** The harness does not touch project locks. They are project-owned and generic code cannot name them. It only states that a project lock should be pid-checked, as reportdesignkit's already is.

## Findings

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:384 (step 7, "Two rounds at most")
- **Source**: drain.md:384, :427, :473; SKILL.md:61; reportdesignkit package-design-tools `gate_round` rows E r0-r5, H r0-r2 and J r2, and `report` rows at 2026-11-16T13:15Z, 2026-11-17T11:40Z and 2026-11-18T13:46Z, read via `tomlctl get ... gate_round` / `report`
- **Evidence-grade**: high
- **Finding**: a gate red after round 2 has no user-accept path or record, so three milestones (E, H, J) were committed by improvisation; add `[[gate_accept]]` on the `[[visual_accept]]` pattern (needs-plan)
- **Details**: E's extra rounds are authorised only by reason text. H's commit is recorded only as a report `gate` entry. J's solo reruns are merged into a round whose outcome stays `fail`. Changes needed: schema.rs (new struct, version 7), a new verb beside run_visual.rs plus the cli, plan-run.md, drain.md steps 7, 10 and re-entry, SKILL.md:61, report.md:29-34 and :86, and the implement-v2 pre-flight.
- **Counter**: a rule elsewhere in the engine already offers the user a commit on a red last round; I searched the implement-v2-engine skill for waive, accept and "commit anyway" and found only visual.md's section.
- **Impact on plan / report**: one milestone of about 4 file-disjoint tasks: the tomlctl record and verb with tests; drain plus SKILL; report; pre-flight and plan-run reference.

- **Library/API or file**: claude/agents/verification.md:90 (Step 1b nohup launch)
- **Source**: verification.md:38, :90-97; session 201a3cf0 transcript 2026-11-17 06:14Z-07:14Z (agent a7b35de0f11d58840); F report `reconcile` entry at 07:15Z; the command `bash -c 'sleep 30 & cat /proc/$!/winpid'` printed a different Windows pid
- **Evidence-grade**: high
- **Finding**: a stopped verification agent leaves its detached gate running, with no pid in a known place and no drain rule to clean it up; add `run_dir:` input, `cN.pid` holding the Windows pid on MSYS, and a stopped-gate drain row (needs-plan)
- **Details**: `<D>` comes from mktemp inside the agent, so after an interrupt the orchestrator had to find the processes by command line through PowerShell. On Windows a kill must use `taskkill /T` on the winpid; MSYS `kill` on `$!` does not reach native children. Changes needed: verification.md (input list, Step 0, Step 1b), drain.md step 7 and its stop rows, and SKILL.md's re-entry bullet at :102.
- **Counter**: Claude Code kills a stopped subagent's nohup'd descendants on its own. The F incident shows it did not under 2.1.326, but a newer harness could; settle this by interrupting a verification run with a 600 s sleep and listing processes.
- **Impact on plan / report**: one task on verification.md plus one drain/SKILL task. The project lock is left to the project.

Outside this lens, one line: the F and H gates were dispatched as async agents even though drain.md:314 shows a plain one-shot `Agent` call.

Coverage: read drain.md steps 7 and 10 and re-entry (:300-:477), visual.md:211-264, report.md:25-40, SKILL.md rows, verification.md:30-100, run_visual.rs:1-90 and schema.rs VisualAccept/GateRound. I read reportdesignkit package-design-tools `gate_round`, `report` and the tasks `changes` gate reasons via tomlctl. I searched transcripts 201a3cf0, 6ec4c43a and 914ec3a0 for human prompts, AskUserQuestion and nohup, and the gates-lock module for pid handling. Ruled out: harness-side lock deletion, since the lock is project-owned and already pid-stale-checked; a single merged waiver table, which the brief's non-goals forbid; and rerunning failed ids under `rerun:`, since the project's aggregate gate script takes no id list. That last one belongs to a sibling lens if any.
