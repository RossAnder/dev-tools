<!-- Generated from execution-record.toml. Do not edit by hand. -->

# /plan-v2 carrier, agents and Workflows (v2 planning, Slice 2) — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | add-the-dependency-card-recipe | 2026-11-06 | | 1 file |
| E9 | add-the-investigator-and-challenger-agents | 2026-11-06 | | 3 files |
| E12 | gate-workflow-scripts-on-their-agents | 2026-11-06 | | 2 files |
| E13 | add-the-cold-reader-agent | 2026-11-06 | | 2 files |
| E16 | add-the-investigate-workflow | 2026-11-06 | | 1 file |
| E21 | write-the-probe-runbook | 2026-11-06 | | 1 file |
| E31 | apply-the-probe-results | 2026-11-09 | | 1 file |
| E32 | add-the-prove-workflow | 2026-11-09 | | 1 file |
| E37 | write-the-approve-reference | 2026-11-09 | | 1 file |
| E40 | write-the-open-and-challenge-reference | 2026-11-09 | | 1 file |
| E42 | write-the-write-and-prove-reference | 2026-11-09 | | 1 file |
| E44 | write-the-engine-skill | 2026-11-09 | | 1 file |
| E45 | add-the-carrier-and-its-gate-row | 2026-11-09 | | 2 files |
| E46 | point-the-project-docs-at-the-carrier | 2026-11-09 | | 2 files |
| E48 | record-the-departures | 2026-11-09 | | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E3 | No `git` command that changes anything, in claude/agents/plan-investigator.md and claude/agents/plan-challenger.md | 2026-11-06 | | research-methods step 5 tells the investigator to run `git log -S` and blame to disconfirm a finding, and research-read-only already bans the mutating verbs. | — |
| E4 | Also excluded everything else under docs/flows/ and .claude/flows/, in claude/agents/plan-challenger.md | 2026-11-06 | | brief.md and design/*.md hold the planner's thinking too, and "Structure challenge" allows only intent, future state and code. The limit stays prompt-only, as the plan states. | — |
| E5 | Return shape {verdict, note}, in claude/agents/plan-challenger.md | 2026-11-06 | | The agent must return a small object; this is the investigator's shape without a lens id. | — |
| E6 | If a file is still at the note path, the agent neither reads nor overwrites it and says so in its return, in the plan-v2-note-writer block | 2026-11-06 | | Write refuses an unread file, and a cold reader must not read its predecessor. | — |
| E7 | Private paths join the public-text ban, and an injected instruction in fetched content must be reported in the note, in the plan-v2-note-writer block | 2026-11-06 | | Notes are committed to a public repo, and an injection attempt is worth a reviewer seeing at commit. | — |
| E8 | Added color purple to plan-investigator and orange to plan-challenger | 2026-11-06 | | All 13 existing agents set color. | — |
| E10 | `...` counts only outside strings and comments; `${` counts in any string but not in comments, in tomlctl/src/cli/dispatch/tests/lint.rs | 2026-11-06 | | An ellipsis in a description is not a spread. Assumes the runtime reads the literal's structure rather than its raw text, inferred from the workflow-authoring wording and not run. | — |
| E11 | Non-key positions skipped: ternaries and shorthand pass, quoted keys are read as keys, in tomlctl/src/cli/dispatch/tests/lint.rs | 2026-11-06 | | review-v2.js's own `cell.agentType : '(none)'` would otherwise read as an agent, `r.agentType === 'x'` has no colon, and a shorthand `{ id, agentType }` has no value to read. | — |
| E14 | Also rejects an empty lenses list, duplicate ids and a blank or multi-line question, in claude/workflows/plan-v2-investigate.js | 2026-11-06 | | Duplicate ids would share one note, an empty run would record as a clean ok, and a later line in a question could pose as a note: line. | — |
| E15 | Suffix match after folding backslashes, returning the canonical relative path, in claude/workflows/plan-v2-investigate.js | 2026-11-06 | | An agent may report the absolute path it resolved, and an exact match would call that a mismatch. | — |
| E17 | `git archive -o` to a file, then System32 tar.exe with --exclude=.github, in docs/ideas/plan-v2-probes.md | 2026-11-06 | | A PowerShell 5.1 pipe corrupts binary output, Git's tar would read C: as a host, and the 18 .github symlinks at 5b58a7e failed to extract when tried. | — |
| E18 | No restore step; the flow directory's deletion covers it, in docs/ideas/plan-v2-probes.md | 2026-11-06 | | T2's target is docs/flows/plan-v2-probe/t2-target.md, created by the runbook, so deleting the flow directory restores the tree and no tracked file is at risk. | — |
| E19 | The links step runs in an elevated window, in docs/ideas/plan-v2-probes.md | 2026-11-06 | | Without elevation it fails here with "Administrator privilege required", as Developer Mode is off. The links step could not be run elevated, so it is untested; its target form matches the existing flow-bootstrap.md link. | — |
| E20 | Added Nested memory:, Auto-memory:, Tally: and a ratings table, in docs/ideas/plan-v2-probes.md | 2026-11-06 | | Surprises: other than none stops task 6, so expected facts need their own lines. | — |
| E33 | Skip reason reads "plan review is not built yet" in claude/skills/plan-v2-engine/references/approve.md | 2026-11-09 | | .claude/rules/documentation.md bars plan phases from shipped skill prose | — |
| E34 | Added tomlctl flow render-progress-log after flow record in claude/skills/plan-v2-engine/references/approve.md | 2026-11-09 | | flow-contract-reconciler rule 5 requires a PROGRESS-LOG.md re-render after every execution-record append; the rendered file lands under .claude/flows/<slug>/ so the second commit picks it up | — |
| E35 | Added a git status --short check of the shared stores before the first commit, asking the user before going on, in claude/skills/plan-v2-engine/references/approve.md | 2026-11-09 | | The second commit stages .claude/active-flow.toml and .claude/backlog.toml whole, and unrelated edits in them would be swept in | — |
| E36 | Added a short Amend paragraph linked from the open-run refusal row in claude/skills/plan-v2-engine/references/approve.md | 2026-11-09 | | plan stage gives a plan approve --amend hint that the procedure must explain | — |
| E38 | Launch args written to <scratch>/investigate-args.json before launch, in claude/skills/plan-v2-engine/references/open-and-challenge.md | 2026-11-09 | | A scratch file lets the open-run rule check that it still holds the exact args rather than trusting memory after compaction | — |
| E39 | Fan-out check uses status, a git diff HEAD --binary capture and sha256 digests of untracked files, excluding notes/ and .claude/flows/<slug>, in claude/skills/plan-v2-engine/references/open-and-challenge.md | 2026-11-09 | | Status alone misses an edit to an already-dirty file or to an untracked file such as brief.md; the flow store directory is written by plan run start/finish during the run | — |
| E41 | Added step 4b running each base premise's check, in claude/skills/plan-v2-engine/references/write-and-prove.md | 2026-11-09 | | plan probes covers only accept entries, so nothing would run a base premise's check before approval | — |
| E43 | A plan needing a dependency at --depth quick continues at standard and the user is told, in claude/skills/plan-v2-engine/SKILL.md | 2026-11-09 | | The dependency card can only be built by a plan-v2-investigate launch; continuing keeps dependency facts tool-settled as Surfacing requires | — |
| E47 | Rewrote the whole row-2 clause (agents hold Write, no plan note verb, carrier documents Edit(docs/flows/**), gate row instead of VALID_COMMANDS) in docs/plans/v2-planning.md | 2026-11-09 | | Leaving the old plan-investigator clause and the VALID_COMMANDS item would contradict the departures table | — |
| E63 | Both plan-v2 Workflows wrap each agent call so a thrown error becomes an entry with status `error` and an `error` message (also listed in `errors`); every entry now carries `error`, null unless it failed. SCRIPT_VERSION is 2. Applies to plan-v2-investigate.js and plan-v2-prove.js (task 7). Uncommitted. | 2026-11-09 | | /review found every agent failure read as no-return, which the engine attributes to an unregistered agent, so a schema or runtime fault was misdiagnosed. The plan's description of the return shapes is now out of date. | — |
| E64 | open-and-challenge.md: the fan-out check snapshots git config (compared first, before any git status/diff) and HEAD, and excludes only the note paths each launch asks for. plan run start passes --args-hash (sha256 of the args file); the digest's stage_inputs.runs now projects args_hash (tomlctl/src/plan/view.rs, plan_records test, tomlctl references/plan.md), and a relaunch requires the hash to match. Surface records a run step (skip when nothing survives, start+finish when items do). Each lens is written into the brief's Unknowns as `- <id>: <question>`; pending is recomputed from those ids minus finished notes. Investigate gained Prove's status table, dependency lens ids are normalised, and the no-agent-launched case is named. Uncommitted. | 2026-11-09 | | /review found the check let stray or overwritten notes and planted git config through, and that a resumed session could not recover the lens list, the launch HEAD or the args. args_hash already existed on the [[run]] row (v2-planning.md asked for it at launch) but had no writer or reader; the user chose to wire it through rather than record it as dropped. | — |
| E65 | SKILL.md Re-entry reads stage_inputs.runs after the open-run rule and resumes at the earliest step whose run step (investigate, challenge, surface, cold-read) has neither a finished nor a skipped row, then follows plan stage's next. Rule 5 no longer says plan stage alone is enough; --depth and --no-review are stated as session-only bindings. Uncommitted. | 2026-11-09 | | plan stage reads brief, milestones, tasks, specs and findings but no [[run]] rows, so a session that ended after Open or after the probes resumed past Investigate, the structure challenge or the cold read. | — |
| E66 | plan-challenger and plan-cold-reader now carry a new agent-neutral plan-read-only shared block instead of research-read-only (review-apply, uncommitted). | 2026-11-09 | | research-read-only speaks in research-methods terms (graded findings, the Counter line, lenses, sibling lenses) that neither agent loads or holds Skill to load. Rewording the shared block would weaken it for the research agents, so the two plan agents got their own block in scripts/shared-blocks.toml. | — |
| E67 | The fan-out snapshot and check moved from a hand-copied shell block in open-and-challenge.md to claude/skills/plan-v2-engine/scripts/fanout-check.sh (snapshot|check modes, per-step snapshot dirs, config compared first); every launch takes a fresh snapshot (review-apply, uncommitted). | 2026-11-09 | | The first real /plan-v2 run rebuilt the block three times as one long Bash line and reused the investigate snapshot before the challenger. A script removes the retyping and drift; per-step directories keep a later launch from overwriting the head-before.txt an open run's relaunch compares against. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-11-06 | 30 entries: status-transition × 1, task-completion × 7, deviation × 14, verification × 7, checkpoint × 1 | 03faed8, 851daf5, b1984dc, bd11bc1, cfa6f39 |
| 2026-11-09 | 37 entries: task-completion × 9, deviation × 14, verification × 12, checkpoint × 1, status-transition × 1 | 0f638f3, 179054e, 1dc2b9e, 7904566, 7ab0593, 85a9e51, 9b1fcbe, d6ccebe, f391ce5 |
