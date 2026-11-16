Verdict: Put elaboration in plan-v2-engine as a re-entry procedure for a running flow, and make `plan milestone update --elaborated` its last store write before one commit. Nothing owns the step today (`tomlctl/src/plan/stage.rs:169-177` gives a running plan only `next: plan drift`, and `claude/skills/implement-v2-engine/references/run-milestone.md:67-73` stops on a deferred milestone). A pool task modelled on settle would need a new tomlctl link kind, a worker-view section, edits to both worker agents and a new drain section, and in the one timed run it would have overlapped about 2 of 20.5 minutes.

```toml sources
read_at = "5aca3e7be8f169ba2b524edbb4e87b955e5b67da"
dirty = ["docs/flows/milestones-by-dependency/brief.md"]

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 169
claim = "An approved plan reports approved, running or done. Running always gives next = plan drift, with no notion of a deferred milestone waiting to be elaborated."

[[source]]
path = "claude/skills/plan-v2-engine/SKILL.md"
line = 91
claim = "Re-entry follows `next` once no run step is missing, so /plan-v2 <slug> on a running flow has no elaboration step to resume."

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 67
claim = "A deferred milestone stops the engine, and elaborating it is out of this slice."

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 243
claim = "A deferred milestone is cold-read when it is elaborated. No step says who does it or how."

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 98
claim = "A revision changes rows with tasks update --ndjson and adds rows with add-many, with needs only on existing ids. An id is never predicted."

[[source]]
path = "docs/plans/v2-planning.md"
line = 265
claim = "The original design had the engine run elaboration and review as two top-level Workflow runs just before the milestone, with the orchestrator applying proposals between them."

[[source]]
path = "docs/plans/jiggly-exploring-robin.md"
line = 19
claim = "The implement-v2 plan put deferred-milestone elaboration out of scope; the engine stops and says so."

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 440
claim = "plan check relaxes only accept-no-check for a deferred milestone's rows, at line 520. Nothing else is keyed on deferral."

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 477
claim = "Rows added after approval are exempt from plan/probe-absent (lines 545-547), so an elaborated milestone's new rows are never warned for missing probes."

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 160
claim = "launch-args requires premise verdicts at HEAD, but nothing at launch requires accept probes."

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 324
claim = "--elaborated works only on a currently deferred milestone, needs --reason, is audited, and cannot be undone by any flag."

[[source]]
path = "claude/skills/tomlctl/references/tasks-write.md"
line = 143
claim = "add-many takes action and detail as row keys."

[[source]]
path = "claude/skills/tomlctl/references/tasks-write.md"
line = 196
claim = "tasks update --ndjson rows take only ids, id, status, agent, commit, ref, set, spec, file_notes, closes, refs, reason and actor, so action and detail must go under set."

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 181
claim = "The settle drain: proposal files as row files, one ask-once gate, review settle records, and review schedule mints the fix rows into the next checkpoint."

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 47
claim = "Workers learn the settle mode from a Findings section of their view, with the same text in plan-worker-lite.md:47-50."

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 558
claim = "The worker view builds the settles section from finding_links, a tomlctl schema hook that an elaboration row would need its own equivalent of."

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 7
claim = "The run's store digest covers every task-store section except the dispatched rows' status and agent, so writing another milestone's rows during a live run is caught as out of lane."

[[source]]
path = "claude/skills/plan-v2-engine/references/approve.md"
line = 150
claim = "A re-approval (--amend) is needed only for a changed brief, so an elaboration that leaves intent alone needs no new stamp."

[[source]]
path = ".claude/flows/tomlctl-test-and-gate-cost/tasks.toml"
claim = "Second corpus elaboration (milestone C, 2026-11-13T12:11:51Z): rows added and updated, gates changed and --elaborated set in the same second. No cold-read run, no decision and no design file (commit 64c957a5 touched tasks.toml only)."

[[source]]
path = "docs/flows/milestones-by-dependency/brief.md"
claim = "B-4222c464 measurement and the friction it names: action and detail under set, and the ColourField.tsx / colourField.ts case collision."
```

## Answer

**What the procedure must contain.** It runs in an attended `/plan-v2 <slug>` session on a flow whose stage is `approved` or `running`, for one milestone whose `elaboration` is `deferred` and whose rows' cross-milestone `needs` are all `done` and committed. Steps, in order:

1. **Preconditions.** The milestone is deferred. No `implement:<id>` run is open on it. Every need outside it is `done` with a `commit` stamp. Read `defer_reason`, the brief, the notes, and what the upstream milestones actually built.
2. **Surface and decide.** Put questions through the three Surface tests and ask the survivors in one `AskUserQuestion` call. Record each choice with `plan decide`. Write `design/<name>.md` only where a row needs prose that no record holds.
3. **Revise the rows.** Use `tasks add-many` for new rows, with `needs` only on ids that exist and a `reason` on every row (the store is audited). Use `tasks update --ndjson` for existing rows, with `action`, `detail`, `title` and `checkpoint` under `set`, `spec` as a top-level key, and a `reason` on every row. Then add `set.needs` edges between new rows using the printed ids, and add the new ids to `binds` and contract `tasks`. Finish with the renamed-names sweep from 4a.
4. **Prove, scoped to the milestone, while it is still `deferred`.** Run `plan check` and `tasks check`, then premises with `--milestone <id>`. Then the cold read through `plan-v2-prove`, for this milestone only, with its own `cold-read` run row. Then the fixes, and then accept probes with `--emit/--record --milestone <id>` and gate probes. None of these verbs or the Workflow filters on deferral, so all of them run before the flag flips.
5. **Flip.** Run `plan milestone update --id <id> --elaborated --reason '<what landed, which decisions, which rows>'`. Then run `plan check` again: only now does `plan/accept-no-check` apply to these rows. Any fix here re-probes the entries it touched.
6. **Commit.** Make one commit, with each path staged by name: `plan.toml`, `tasks.toml`, both `.sha256` files, the design files and `notes/cold-read-<id>.md`. No re-approval is needed unless the brief changed (approve.md:150). The close summary lists the agent-assumed decisions, because no approval gate will ever show them.

**How the two routes compare.**

| | `/plan-v2` re-entry | Pool task modelled on settle |
|---|---|---|
| Files changed | 3 to 4, mostly prose: plan-v2-engine `SKILL.md` (Arguments, Re-entry, step table), one new reference section, the deferred-stop message at run-milestone.md:73, and optionally a `plan stage` `next` hint. | About 8, plus tests. A new tomlctl link or marker for the elaboration row (schema, add-many keys, check). A worker-view section (view.rs). Both plan-worker agents. A new drain section beside apply-rows.md:181. A placement rule in write-and-prove. Changes to run-milestone's deferred stop, because a row in the deferred milestone itself is never dispatched. |
| tomlctl verbs | None new. Every verb needed already works on a deferred milestone. | Settle has `review schedule` to mint rows from a proposal. Elaboration has no such verb, so the orchestrator still writes every row by hand at the drain. |
| Overlap gained | None within one session. Combined with the dependency-ordered pick (B-0ee4f204), C runs first and nothing waits on B. A second session overlaps fully only if the pre-flight is scoped (sibling lens `concurrent-elaboration`). | Only the worker's reading and drafting. The questions, records, cold read, probes and commit stay serial at the drain. A settle-style separate milestone gains nothing at all, because one Workflow runs at a time (a non-goal). Overlap needs the row placed inside another code milestone. |
| Fit | The plan-v2 role rules already own decisions, records, design files and the cold read (plan-v2-engine `SKILL.md:16-20`). | It puts plan-v2's Design, Write and Prove inside implement-v2's drain, including launching `plan-v2-prove` from the implement engine. |

**Measured, from the `package-design-tools` transcript (session f526a28c, UTC).**

| Time (UTC) | Span | What happened |
|---|---|---|
| 12:57:01 | 2.1 min | `/plan-v2` invoked; agent reading |
| 12:59:06 | 6.0 min | Question asked; answered at 13:05:05 |
| 13:05:05 to 13:11:28 | 6.4 min | Design file, decide, add-many, a refused update, `--elaborated`, premises and probes |
| 13:11:28 to 13:15:43 | 4.3 min | Cold read |
| 13:15:43 to 13:17:32 | 1.8 min | Fixes and commit 08f7671c |

Of the 20.5 minutes, only the 2.1 minutes of reading, plus part of the drafting, is work a pool worker could have taken. This contradicts the brief's line that reading "took most of the time". The largest single spans were the user's answer and the record writing.

**Recommendation.** Take the `/plan-v2` re-entry route. Revisit the pool route if a measured elaboration's agent-only reading and drafting comes to most of its time, or if a flow defers several milestones whose elaborations could share one pool.

## Findings

- **Library/API or file**: `tomlctl/src/plan/stage.rs:169-177` `assess`, together with `claude/skills/implement-v2-engine/references/run-milestone.md:67-73`
- **Source**: both read at HEAD 5aca3e7b; `docs/plans/v2-planning.md:265`; `docs/plans/jiggly-exploring-robin.md:19`; the corpus stores
- **Evidence-grade**: high
- **Finding**: No skill or verb owns elaborating a deferred milestone after approval, and the two elaborations in the corpus did different steps. Add an owned Elaborate procedure. `needs-plan`
- **Details**: After approval, `plan stage` returns `running` with `plan drift` and says nothing about a deferred milestone. plan-v2 Re-entry (`SKILL.md:91`) therefore has no step to resume, and implement-v2 stops on the milestone without saying who elaborates it. The original design (v2-planning.md:265) gave elaboration to the engine as an Elaborate Workflow run followed by a Review run; the implement-v2 plan cut it. In the corpus, package-design-tools B had a design file, five decisions, premises, probes and a cold read. tomlctl-test-and-gate-cost C (12:11:51Z, commit 64c957a5, `tasks.toml` only) had a gate change, three added rows and one updated row, and no cold read, decision or design file.
- **Counter**: The finding fails if a skill or command that this sweep missed (`grep -i elaborat` over `claude/`) already routes a running flow's deferred milestone to a procedure.
- **Impact on plan / report**: This is the plan's core task. Add a reference section in plan-v2-engine and wire it from Re-entry and from run-milestone's deferred stop.

- **Library/API or file**: `tomlctl/src/plan/check.rs:440-451,520` `spec_findings` / `row_spec_findings`; `claude/skills/tomlctl/references/plan.md:324`
- **Source**: code read; `grep -n deferred` over `tomlctl/src/plan/probes.rs`, `emit.rs` and `claude/workflows/plan-v2-prove.js` returned no hits; the package-design-tools trail shows `milestone:B` elaboration at 13:11:01Z, the cold-read run from 13:11:31Z to 13:15:47Z, and the fixes at 13:17:10Z
- **Evidence-grade**: high
- **Finding**: Deferral only relaxes `plan/accept-no-check`. Premises, probes, the cold read and launch-args all work on a deferred milestone, so the procedure can and should set `--elaborated` last, after Prove, and commit it with its rows.
- **Details**: In the measured run, B read `elaborated` 30 s before its cold read launched and 6 minutes before the fixes it needed. Any flow-store commit in that window would have committed unproved rows as runnable (B-c2d75b04). Keeping the flag as the final write means the strict `plan check` after the flip is the only extra step. The flip cannot be undone (`plan.md:323-324`: no flag goes back to `deferred`), which is one more reason to flip only when the work is proved.
- **Counter**: The finding fails if `plan probes --emit` or `plan-v2-prove` turned out to skip a deferred milestone's rows. The greps above found no such filter, but a run against a fixture would settle it.
- **Impact on plan / report**: It fixes the order of the procedure's steps 4 to 6, and it answers the "keep it deferred until the cold read passes" half of B-c2d75b04 with no tomlctl change.

- **Library/API or file**: `tomlctl/src/plan/check.rs:477-486,545-547` `added_after_approval` / `probeable`; `run-milestone.md:160`
- **Source**: code read
- **Evidence-grade**: high
- **Finding**: Nothing requires probes on an elaborated milestone's new rows. `plan/probe-absent` skips rows added after approval and is only a warning anyway, and launch-args checks premise verdicts, not accept probes. The procedure must run the milestone's accept and gate probes itself.
- **Details**: Elaboration adds rows after approval by definition (rows 49 and 50 on package-design-tools B; three rows on tomlctl-test-and-gate-cost C). Both sessions happened to probe, but no check would have caught it if they had not. A tomlctl alternative is to treat a row added after approval in a milestone flipped to `elaborated` as probeable. That is a judgement call for the plan, not a defect today.
- **Counter**: The finding fails if a gate on the implement-v2 side refuses an unprobed `writes_build = false` accept entry before launch. run-milestone.md steps 1 to 4 show none.
- **Impact on plan / report**: The procedure gets an explicit probe step. Optionally, a task widens `probeable` (check.rs only).

- **Library/API or file**: `claude/skills/tomlctl/references/tasks-write.md:143-145` against `:196-198`
- **Source**: reference read; transcript f526a28c at 13:10:18Z: `unknown key \`action\` — a row takes ids, id, status, agent, commit, ref, set, spec, ...`
- **Evidence-grade**: high
- **Finding**: `add-many` takes `action` and `detail` as row keys, but `tasks update --ndjson` refuses them outside `set`. The procedure should show the update record shape with `[record.set]` holding `action` and `detail`, beside `reason` and `spec`.
- **Details**: write-and-prove.md:98 sends a revision to `tasks update --ndjson`, but its only update examples (lines 172-178 and 314-328) show `set.title` and `spec`. The refusal cost one round of edits (13:10:18 to 13:10:49) and wrote nothing. That is cheap, but an owned procedure should state the shape.
- **Counter**: The finding fails if the shape is already stated in a section an elaborating session would read. It is stated in tasks-write.md:196, which the session did not read until after the refusal.
- **Impact on plan / report**: One record example in the new procedure. No verb change.

- **Library/API or file**: package-design-tools `notes/cold-read-B.md` (reportdesignkit commit 08f7671c), against the premise run at 13:11:01Z
- **Source**: transcript f526a28c. All 15 checkable premises recorded `holds` before the cold read, and the cold read then returned 4 false premises (for example, `KIT_TOKEN_DECLS` is not on core's public barrel).
- **Evidence-grade**: high
- **Finding**: The cold read is the step that caught the elaborated milestone's false premises, because the premise checks (greps) passed while their claims were false. The procedure keeps the cold read mandatory for an elaborated milestone, skipped only under write-and-prove's few-deep-tasks rule.
- **Details**: The case collision `ColourField.tsx` / `colourField.ts` (row 29's `files`) passed plan check, the premises, the probes and the cold read alike. Only the worker's `bun run boundaries` caught it, which cost one failed round, and row 29's `files` changed at 13:25:36Z. That gap is not specific to elaboration: a case-insensitive duplicate-path class in `plan check` would cover every Write. It is noted here for the pick-and-preflight or a backlog item, outside this lens.
- **Counter**: The finding fails if the tomlctl-test-and-gate-cost C elaboration, which ran no cold read, met no false premise in its two implement runs. That flow's drain notes would settle it, and this lens did not read them.
- **Impact on plan / report**: The cold read stays in step 4. The collision goes to the tangential list.

- **Library/API or file**: `claude/skills/implement-v2-engine/references/apply-rows.md:181-224` (settle drain); `claude/agents/plan-worker-deep.md:47-50` and `plan-worker-lite.md:47-50`; `tomlctl/src/plan/view.rs:558`; `tomlctl/src/plan/lane.rs:7-13`
- **Source**: code and skill read; timings from transcript f526a28c
- **Evidence-grade**: medium (the per-file cost is an estimate from the settle path's footprint)
- **Finding**: An elaboration row in the implement-v2 pool would cost as much as the settle path (a tomlctl link kind, a worker-view section, both agents, a drain section and a placement rule) and would still serialise everything but the reading. Reject it for now and record `revisit_if`.
- **Details**: Settle works because `settle-<n>` is a normal upfront milestone, `review schedule` mints the fix rows, and the next checkpoint receives them. An elaboration row cannot sit in the deferred milestone, which the engine never launches (run-milestone.md:67-73). As its own milestone it runs alone, since one Workflow runs at a time. So it overlaps other work only if the planner places it inside another code milestone, and that milestone's drain then has to host plan-v2's Surface, Design, Write and cold read. The cheaper idea of elaborating in the same session while another milestone's run is live is out of lane: the run's store digest covers every task-store section except the dispatched rows' status and agent (lane.rs:7-13).
- **Counter**: The finding fails if a measured elaboration showed agent-only reading and drafting as most of its time. The one timed case shows 2.1 of 20.5 minutes; the brief's line saying otherwise is not borne out by the transcript timestamps.
- **Impact on plan / report**: The decision row prefers the `/plan-v2` re-entry, rejects the pool route with this evidence, and sets revisit_if to "elaboration reading or drafting measured above half its time, or a flow with several deferred milestones".

Coverage: Read in full: plan-v2-engine `SKILL.md`, `write-and-prove.md`, `run-milestone.md`, apply-v2-engine `SKILL.md`, and `apply-rows.md` lines 180-273. Read in part: tasks-write.md (`add-many`/`update`), plan.md (`milestone add/update`), `stage.rs` (lines 50-190), `check.rs` (lines 430-567), the `lane.rs` header and the v2-planning.md "After approval" section. Checks run: tomlctl reads of both corpus elaborations (trail, milestones, runs, commits); a transcript timeline of reportdesignkit session f526a28c from 12:50 to 13:20Z; greps for deferred filters in probes.rs, emit.rs, plan-v2-prove.js and plan-cold-reader.md (none). Ruled out: a tomlctl verb gap for elaboration (every needed verb works post-approval); re-approval after elaboration (amend only on a brief change); concurrency and pre-flight scoping (sibling lenses `concurrent-elaboration` and `pick-and-preflight-sites`). Not read: the plan-worker procedure beyond the settle section, and the drain.md replan step 6, since the pool route was costed from the settle footprint.
