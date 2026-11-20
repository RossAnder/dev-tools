Verdict: All four gaps are still open after implement-v2-run-hardening. No step stops a launch for an undone user input (run-milestone.md:96, 116 have no handover read). No step owns or commits a handover done-mark (records.rs:611 writes the mark; drain.md:461 commits only inside a milestone). Elaborate never reads unowned commits against the decisions (elaborate.md:47). No rule stops the orchestrator editing the brief while a launch is open (open-and-challenge.md:99-111). Each has a small design below; B-8aee5fe3 is needs-plan.

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 17
claim = "Pre-flight at stage `done` reports and stops, so after the last milestone no engine step runs that could mark or commit a handover"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 96
claim = "Gate probes run at the pick and stop the engine before launch, with the fix left to the user; they must name an existing gate"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 116
claim = "A premise is a code fact; a `fails` verdict withholds the task and sends it to the replan, not to the user"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 404
claim = "Handovers are written only at the drain, as a step after landing, proposed by a worker"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 461
claim = "The flow-store commit runs only inside a milestone's step 10; nothing commits a store write made after the report"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 30
claim = "Rule 7 says how to write a handover and says nothing about marking one done or committing the mark"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 178
claim = "`plan handover set` refuses a `done` key; the separate verb `plan handover done` sets the mark (no `--done` flag exists)"

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 551
claim = "upsert_handover refuses `done`, pointing at `plan handover done <id>`"

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 611
claim = "handover_done sets `done = true` and writes no evidence, reason or change row"

[[source]]
path = "tomlctl/src/plan/probes.rs"
line = 193
claim = "A premise's `at` is only `base` or `after:<n>`; no premise kind is owned by the user"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 424
claim = "The only handover check classes are incomplete fields and an unknown source_task; nothing ties a handover to the tasks it blocks"

[[source]]
path = "docs/flows/human-only-handovers/design/handover.md"
line = 30
claim = "A handover's `when` is never 'at checkpoint A', since a step inside the run is not a handover, so a pre-task input has no legal home"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 74
claim = "Plan-time handovers take the same post-landing shape; nothing lets one block a task"

[[source]]
path = "claude/skills/plan-v2-engine/references/approve.md"
line = 75
claim = "The approval report lists undone handovers, which is where partnerhub's 'before milestone A' H1/H2 were shown and then not enforced"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 47
claim = "Elaborate reads only the upstream rows' own `commit`s; it never reads commits no row owns, and it never checks the brief or decisions against code changed by hand"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 17
claim = "A changed brief must be settled with `plan approve --amend` before elaborating, which is why a brief fix found mid-elaboration costs a second gate"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 501
claim = "`plan drift` already lists files changed since the newest approval by commits no row owns, but no plan-v2 step calls it"

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 99
claim = "'Running it' has no rule against the orchestrator editing tracked or untracked files while a launch's window is open"

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 111
claim = "On exit 1 the plan stops and the change goes to the user, with no way to tell the orchestrator's own edit apart"

[[source]]
path = "claude/skills/plan-v2-engine/scripts/fanout-check.sh"
line = 93
claim = "The tolerated set is the excludes, the note paths, the flow store and run notes; brief.md is never tolerated"
```

## What happened

- **B-8aee5fe3 (partnerhub_pages, flow site-worker-new-strapi).** Session 724defa2 approved the plan at 2026-11-19 12:43Z. Its report said handovers H1 and H2 (a Cloudflare token, account id and zone id in `.env`) were due "before milestone A", and H2's `when` said the same, with `source_task = 6`. In session 36cbd3f6, the milestone A run launched 20 minutes later without asking. Task 6 wrote its script and tests, got a 403 from the old token, and parked with reason `credentials` (attempt run wf_983982d2-9ca). The user then set H2 up mid-run. The H2 text was rewritten twice during the drain: once for a missing permission, and once at 13:47Z because its permission names did not match the dashboard's labels (that is the research-sourcing lens's B-98d14bca). H2 was marked done and task 6 unparked at 14:06Z.
- **B-caa21ce6 (same flow).** Done-marks were made in three different ways. In session 4bf2fd51 on 2026-11-20 (05:37 to 05:56Z), the orchestrator marked H3, H1 and H4 done and committed only H4, by hand, as 39a0954. In session b2a1b292, H9 was marked at 10:54Z, after milestone C's flow-store commit 16ee1f6 at 10:31Z, so the `done = true` rode into the user's unrelated commit 20d7159 "deps and doc updates" at 12:19Z. H5 was marked at 12:24Z and is still an uncommitted diff of `plan.toml` and its sidecar. Every one of these marks was made outside a drain, because milestone C was the flow's last.
- **B-959255fa (strapi-server, flow cloud-deploy-parallel-run).** Between the record commit for milestone B (954a90e, 11-16) and the elaboration of C, six hand commits landed during provisioning. One of them, bb751f8, replaced `@napi-rs/image` with sharp. In session 465843d5 on 2026-11-19 (09:41 to 10:23Z), the elaboration read the code at HEAD and correctly found most of C already done. Its closing report then listed as untouched loose ends that the brief and D3/D10 still named `@napi-rs/image`, and that three backlog rows looked resolved. The user replied "fix the loose ends", and that cost a second amend cycle (98d8e07, then the approval stamp 0f8743e). bb751f8 touched `package.json` and `src/api/article/services/og-image.ts`, which are exactly the files bound by D10 and D3. A plain intersection of unowned commits with decision `binds` would have flagged both decisions.
- **B-ba011350 (strapi-server, same flow).** In session ac680855 on 2026-11-16, the challenger's snapshot was taken at 08:16:55Z. The orchestrator then corrected two lines of the brief's Background at 08:18:47Z while the challenger ran. The check at 08:24Z exited 1 on `brief.md`'s digest. The orchestrator raised a "Fan-out" safety question, and the user answered at 08:28Z, about 3 minutes of stall. The orchestrator did announce the edit in advance, but the procedure gave it no way to avoid the question.

## Findings

- **Library/API or file**: claude/skills/implement-v2-engine/references/run-milestone.md:96 and :116 (Gate probes, Premises); docs/flows/human-only-handovers/design/handover.md:30
- **Source**: file reads; partnerhub plan.toml H2 row (`when` "Before /implement-v2 starts milestone A", `source_task = 6`) and the task 6 parked attempt; session 36cbd3f6 13:32Z
- **Evidence-grade**: high
- **Finding**: Nothing blocks a task on an input only the user can supply. The handover design forbids an in-run `when`, a premise is a code fact whose `fails` verdict goes to the replan, and neither the schedule nor `launch-args` reads handovers. needs-plan.
- **Details**: A `rg -i handover` over schedule.rs, emit.rs, probes.rs and stage.rs finds nothing, and check.rs:424 checks only fields and `source_task`. Three options follow. (A) Add a blocking handover: a new `blocks = [<task ids>]` key (records.rs `HANDOVER_SET_KEYS`, schema.rs, a check class for an unknown or done blocked id). `launch-args` drops blocked tasks as `handover` while the handover is undone. At the pick (run-milestone.md step 2, beside gate probes), the engine asks the user once through `AskUserQuestion` to do it now, then marks it done. handover.md:30 is amended to allow a `before` handover. (B) Add a user-owned premise: a premise flag such as `owner = "user"`, whose `fails` verdict goes to the user instead of the replan (probes.rs:193, run-milestone.md:158 table). It reruns at every launch, so a later expiry is also caught, but the action text then lives in a premise rather than in a handover. (C) A prose-only rule at the pick, reading undone handovers whose `when` names this milestone. This costs no code but matches free text. A plus B is the strongest: the handover holds the action, and a read-only `ready_check` (for example a presence test on the `.env` key, never a network call, per run-milestone.md:126's read rule) decides whether it holds.
- **Counter**: A gate probe attached to milestone A, checking the `.env` key, would already have stopped the launch. It would, but `--gate-probe` must name an existing gate (milestone.rs:95) and reports a gate that cannot pass, so it would be a misuse, not a design.
- **Impact on plan / report**: one milestone. Schema, records and check in tomlctl; emit launch-args; run-milestone step 2; write-and-prove:74; handover.md. Keep it distinct from B-0f9fc8b5 (an effect the gate produces, now resolved).

- **Library/API or file**: tomlctl/src/plan/records.rs:611 `handover_done`; claude/skills/implement-v2-engine/references/run-milestone.md:17; drain.md:461
- **Source**: partnerhub git: 20d7159 carries only H9's `done = false -> true` hunk in the flow store, and `git diff` shows H5's mark uncommitted; sessions b2a1b292 10:54Z/12:24Z and 4bf2fd51 05:56Z
- **Evidence-grade**: high
- **Finding**: No step owns marking a handover done, and no step commits the mark. A mark made after the final milestone, or between invocations, lands in whatever commit the user makes next.
- **Details**: The engine text never runs `plan handover done`: the only mention is the tomlctl reference, plan.md:191-197. Pre-flight at stage `done` stops (run-milestone.md:17), and the flow-store commit runs only inside drain step 10, through `plan emit commit-paths --milestone` (a required flag). The backlog item's "`plan handover set --done`" does not exist: `set` refuses `done` (records.rs:551). Proposed rule, added to SKILL.md rule 7 and report.md: whenever the user reports a handover performed, mark it with `plan handover done`, then commit `.claude/flows/<slug>/plan.toml` and its `.sha256` in the same turn, staged by path with `git commit -- <paths>` and the message from commit-conventions, so nothing else in the index rides along. Pre-flight's `done` branch also offers the undone handovers and runs that commit. Option: `plan handover done` could take `--note '<what was observed>'`, since H4 was marked "not verified from this machine" with no record of that in the store.
- **Counter**: If marks are meant to wait for the next milestone's flow-store commit, the current behaviour is fine mid-flow. But a flow whose last milestone is committed has no next commit, and that is where all three marks in this incident were made.
- **Impact on plan / report**: one small prose task (SKILL.md, run-milestone.md:17, report.md), plus an optional `--note` on the verb.

- **Library/API or file**: claude/skills/plan-v2-engine/references/elaborate.md:47 (section 1, "Read what the milestone stands on")
- **Source**: strapi-server `git show --stat bb751f8` (touches package.json and og-image.ts); `tomlctl items list ... --array decision` shows D3 binds og-image.ts and D10 binds package.json; session 465843d5 10:23:43Z report
- **Evidence-grade**: high
- **Finding**: Elaborate reads only the rows' own commits. Hand commits since then are never put against the brief, the decisions or the backlog, so a choice the user dropped survives in them until the user asks.
- **Details**: `tomlctl plan drift --slug <slug>` (plan.md:501) already lists `changed_files` from commits no row owns, but no plan-v2 step calls it. Its baseline is the newest approval, which an amend moves. Proposed change for section 1: run `plan drift`, intersect `changed_files` with each decision's file `binds`, read `git show` for each unowned commit that hits one, and put each decision whose text no longer holds into section 2's Surface round. A brief line naming a dropped choice joins the same round, and on a yes, the brief edit, `plan decide` and `plan approve --amend` happen inside this elaboration before the flip, in one gate instead of two. `backlog reconcile` candidates go into the same report. An optional tomlctl addition: an `affected_decisions` array in `plan drift`'s output, which would make the intersection mechanical.
- **Counter**: A decision whose `binds` names only task ids, or names a file the hand change did not touch, is missed by the intersection, so this is a reliable trigger, not a complete one. The brief still has to be read by judgement.
- **Impact on plan / report**: elaborate.md section 1 and 2 prose. The optional drift field is a small tomlctl task with a test.

- **Library/API or file**: claude/skills/plan-v2-engine/references/open-and-challenge.md:99-111 ("Running it"); scripts/fanout-check.sh:93
- **Source**: session ac680855: snapshot 08:16:55Z, Edit to brief.md 08:18:47Z, check exit 1 on the brief digest 08:24:15Z, AskUserQuestion answered 08:28Z
- **Evidence-grade**: high
- **Finding**: The fan-out check cannot tell the orchestrator's own edit from an agent's, and nothing tells the orchestrator not to edit while a launch is open, so a brief correction stops the plan.
- **Details**: Option 1 (recommended): in "Running it", forbid any Write or Edit in the repo between `snapshot` and `check`, the brief and notes included. A correction is held in `<scratch>` and applied after the check passes. This costs nothing and keeps the check's meaning intact. Option 2: add a `fanout-check.sh own <dir> <path>` that re-digests one path after the orchestrator's edit. This weakens the check, since an agent write to that file before the orchestrator's edit would be absorbed. Option 3: tolerate `brief.md`. This is rejected, because the brief is an injection target. Option 1 also covers investigate, the cold read and elaboration's cold read (elaborate.md:162-186), and it keeps the brief an agent was launched with consistent with the brief on disk.
- **Counter**: A Bash write by the orchestrator would trip the check the same way, so a prose rule naming only Write and Edit is incomplete. It must say "no repo writes of any kind".
- **Impact on plan / report**: one sentence in open-and-challenge.md section 3, cited from elaborate.md section 4c.

Coverage: I read in full drain.md steps 9-10, run-milestone.md sections 1-3, implement-v2-engine SKILL.md (rules and steps), elaborate.md sections 1-3, open-and-challenge.md sections 3-4, design/handover.md, records.rs:520-625, plan.md (`handover set`/`done`, `drift`), and write-and-prove.md:55-91. Checks run: an `rg` for handover across claude/ and the schedule, emit, probes and stage sources; `plan drift` on strapi-server (read-only); git log and show in both consumer repos; transcript greps over partnerhub sessions 724defa2, 36cbd3f6, 4bf2fd51, b2a1b292 and 44c4e644, and strapi sessions ac680855 and 465843d5. Ruled out: gate probes as the input-block mechanism, since a probe must name a gate (milestone.rs:95). Also ruled out: a cross-flow sweep of H5 into another flow's milestone commit, since commit-paths stages by path and pre-flight exempts other flows' records (run-milestone.md:41). The wrong console labels in H2 are B-98d14bca's, under the research-sourcing lens.
