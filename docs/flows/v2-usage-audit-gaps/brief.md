# v2-usage-audit-gaps

## Intent

The causes behind nineteen v2 harness failures seen in the 2026-11-14..20 usage audit (dev-tools, partnerhub_pages, strapi-server, reportdesignkit) are closed, so a consumer milestone run stops spending fix rounds, user questions and wrong turns on them. The incident detail behind each one, which today lives only in this machine's session transcripts and consumer repos, is captured in this flow's committed notes, paraphrased, so the work can be triaged and built on any machine.

The items fall into five themes:

- **Fix rounds and gates.** B-d7f07e99: a gate failure that already reproduces at the milestone's base commit costs a fix round. B-5826b324: fix and diagnose workers may not run their own reproducing check. B-95a60315: a whitespace-only fix reruns the whole gate. B-686f291e: a gate still failing after the last round has no recorded user-accept path. B-37932bb6: a gate launched with nohup outlives a stopped verification agent and holds the gate lock.
- **Plan and decision integrity.** B-ec551154: a replan fix task can contradict a recorded decision silently. B-2053e101: Prove never runs a comparison gate on the unchanged tree. B-bd50caa8: plan edits made after the review's re-check are never re-reviewed. B-959255fa: elaborate does not reconcile fixes made by hand between milestones. B-8aee5fe3: an input the user must supply before a task starts can only be a handover, which runs after the task. B-caa21ce6: handover done-marks are never committed. B-ba011350: editing the brief while the challenger runs trips the fan-out check.
- **Research sourcing.** B-98d14bca: orchestrator-written elaboration specs, handover steps and applied advisories assert platform facts with no fetched source. B-6d3fb3e3: review cells fetch the same outside page independently in one run. B-8ec0b5b5: a source a worker verifies stays in its run note, so a later task researches it again. B-34d901f9: investigators skip Context7 and guess doc URLs.
- **Visual.** B-0e2db574: a later visual round can fail a state an earlier round passed on unchanged code. B-80fd8af7: a project with UI milestones and no capture config is never asked for one.
- **Self-edit.** B-d05d1f05: a milestone that edits the engine running it has no rule, and the rule rests on unprobed claims about when edited skills and agents load.

## Non-goals

- The v1 carriers and their agents are not changed.
- Nothing is replayed against a consumer repo, and no consumer repo is edited.
- What implement-v2-run-hardening delivered (commit paths, held external commands, gate blocks, the visual accept, the indirect-script audit, the fix worker's own capture) is built on, not reworked.
- The other open backlog rows from the same audit, which carry enough detail on their own, are not in scope.
- Notes quote no transcript verbatim: no logs, machine-local paths, tokens or consumer content; they paraphrase, citing session ids, timestamps, task, decision and finding ids and repo names.

## Scope budget

The `implement-v2-engine` skill and its references (drain, visual, run-milestone, questions, report); the `plan-v2-engine` references (open-and-challenge, write-and-prove, elaborate, approve, ui-layout); the `review-v2-engine` plan-flow reference; the `research-methods` skill; the agents `plan-worker-lite`, `plan-worker-deep`, `plan-investigator`, `review-finder`, `review-verifier`, `verification` and `ui-inspector`; the saved Workflows `implement-v2-milestone` and `implement-v2-visual`; the `tomlctl plan` group (run records, handovers, check classes, emit) with its tests and skill references; CLAUDE.md where it binds workers. Two milestones (D9) of small, file-disjoint tasks. Beyond the surfaces above, the tasks also edit the `tomlctl review` store and run start, the `tomlctl tasks` schema and milestone fields, the capabilities list and version, the `review-finder-cf` agent, the `review-v2-method` skill and the `/implement-v2` and `/plan-v2` carriers.

## Success criteria

- A gate failure that also fails at the milestone's base commit is reported apart and costs no fix round.
- A fix or diagnose worker can run its own reproducing check before it returns done, without breaking the build discipline for parallel workers.
- A fix round whose change is whitespace only, in files where whitespace carries no meaning, reruns only the commands that failed.
- After the last gate round, the user can accept a failing gate in one command; the waiver is recorded, the commit proceeds on it, the report prints it and a backlog item follows it up.
- Stopping a verification agent leaves no gate process running; a lock the project owns is left to the project's own stale-lock check.
- A fix task that would contradict a recorded decision reaches the user before it runs, and the decision is amended or the fix changed.
- Each comparison or snapshot gate is run on the base tree before its milestone gate: Prove runs it when its tool exists, and when a task of the plan creates the tool, that task's accept runs it on the base against itself, so a gate that fails before any work is found while planning or at that task, never first at the milestone gate.
- Plan records changed after the review's re-check are rechecked before a first approval, or the approval records the user's waiver.
- Elaborating a milestone reconciles the changes made since the last milestone commit with the brief and the decisions.
- A task that needs an input only the user can supply waits for it instead of launching and parking.
- Handover done-marks reach a commit of their own flow.
- Editing the brief while a launch runs is either prevented or not mistaken for an agent's stray write.
- A platform fact in an orchestrator-written spec, handover step or applied advisory carries a fetched source.
- A review cell reads an outside page an earlier cell of the same run fetched instead of fetching it again; a source a worker verifies is kept where later tasks read it.
- Investigators reach for Context7 or a search before guessing a documentation URL, as the decision on B-34d901f9 settles.
- A visual round does not fail a state an earlier round passed unless that state's capture changed.
- Planning a UI milestone in a project with no capture config asks the user for one.
- A milestone that edits the engine running it follows a rule backed by a probe.
- Every item's incident detail is in a committed note of this flow.
- The tomlctl suite and the dispatch gates pass, the binary is reinstalled, glimpse is reinstalled against it, and the `/implement-v2` and `/plan-v2` pre-flights refuse a binary without the new verbs.

## Background

**B-d7f07e99.** 2026-11-20: dev-tools implement-v2-run-hardening milestone A spent a round on a test already red at HEAD (commit 2b9163ff made outside the gate ran only unit tests); partnerhub_pages preview-cpu-trim made a deep fix task of a live-Strapi test that failed on changed data. Rerun failed ids at the base commit and route pre-existing failures to the backlog or the user without spending a round.

Evidence: `notes/backlog-B-d7f07e99.md`

**B-5826b324.** drain.md:266 and the plan-worker contract forbid running an accept check that builds, while drain.md:370 makes the narrowest reproducing command the fix row's accept and CLAUDE.md allows a worker's own narrow test. dev-tools tasks 4 and 11 shipped broken tests and fix tasks 18/19 never ran theirs; partnerhub task 12 reads node_modules to find a 500 it may not reproduce (C's task 43, which did reproduce, found four causes). License a serial own-reproducer run.

Evidence: `notes/backlog-B-5826b324.md`

**B-95a60315.** dev-tools milestone A round 2 reran clippy and the full nextest suite (about 8 min) for a rustfmt-only fix. If git diff -w over the fix rows' files is empty, rerun only the failed commands.

Evidence: `notes/backlog-B-95a60315.md`

**B-686f291e.** Seen on reportdesignkit: milestone E ran gate rounds 3-5 on the user's word, and H committed on solo reruns after a red round 2; drain.md says to leave the milestone uncommitted. B-22eca7ef adds a waiver for a failed visual pass only; the same record shape (states/criteria waived -> gate commands waived) beside plan run gate would cover this.

Evidence: `notes/backlog-B-686f291e.md`

**B-37932bb6.** reportdesignkit 2026-11-17: the orphaned gate's browser died, 52 minutes were lost and the stale .rdk/gates.lock was checked by hand. Step 1b could write cN.pid and the drain could name the cleanup for a stopped agent.

Evidence: `notes/backlog-B-37932bb6.md`

**B-ec551154.** partnerhub_pages preview-cpu-trim fix task 11 masks island uids, against D8 ('normalise exactly those and nothing else'). The replan should route a conflicting fix through plan decide and a user question (questions.md), never silently.

Evidence: `notes/backlog-B-ec551154.md`

**B-2053e101.** partnerhub_pages preview-cpu-trim: the static-identity gate (718 s) reported 401 differing files from build-noise island uids already known in commit 7be8b96, after D8 was written to forbid masking them. A negative-control run of each comparison gate on the base tree during Prove would have exposed it.

Evidence: `notes/backlog-B-2053e101.md`

**B-bd50caa8.** strapi-server: advisory F78 (unverified by design) pinned oven/bun:1.4.5, hand-applied after the re-check; the tag does not exist and the gate failed (fix task 31). dev-tools implement-v2-run-hardening: the U2 re-check covered only targeted records, not new task 16 or rows moved in a milestone re-cut. A post-triage delta check over changed records would close both.

Evidence: `notes/backlog-B-bd50caa8.md`

**B-959255fa.** strapi-server milestone C elaboration (2026-11-19) found most of C already done during provisioning, while the brief and D3/D10 still named @napi-rs/image; the user had to ask to fix the loose ends (an extra amend cycle). Elaborate could run git log <last milestone commit>..HEAD over the milestone's files and check decisions against it.

Evidence: `notes/backlog-B-959255fa.md`

**B-8aee5fe3.** partnerhub_pages task 6 launched without handover H2 (token and zone id), got a 403 and parked; the user set H2 up mid-run. Distinct from B-0f9fc8b5 (gate-produced effect). Options: a handover field that blocks a task, or a premise such as test -n on the env var.

Evidence: `notes/backlog-B-8aee5fe3.md`

**B-caa21ce6.** partnerhub_pages 2026-11-20: H9's done-mark rode into the user's unrelated commit 20d7159; H5's is still uncommitted and must be kept out of another flow's milestone commit. Add a flow-store commit after marking handovers done (report.md / handover step).

Evidence: `notes/backlog-B-caa21ce6.md`

**B-ba011350.** strapi-server 2026-11-16: brief edited mid-launch, the check raised a safety question to the user. Add 'no edits to the brief while a launch runs' to the Running it section.

Evidence: `notes/backlog-B-ba011350.md`

**B-98d14bca.** Audit 2026-11-20: investigator notes were well sourced, but wrong claims leaked through orchestrator text: partnerhub task 34 was told to measure with wrangler tail (cannot see preview invocations) and failed round 1; console labels in handovers were wrong; strapi F78 pinned a nonexistent image tag. Apply the research-methods source hierarchy to elaboration and handover writing, or route such claims to an investigator.

Evidence: `notes/backlog-B-98d14bca.md`

**B-6d3fb3e3.** partnerhub_pages 2026-11-19: three finders and two verifiers each fetched the Cloudflare Pages/Workers header pages for one question (findings F65, F82). review-finder.md also has no line on when to look things up. A run-level sources note the cells read and append to would cut the repeats.

Evidence: `notes/backlog-B-6d3fb3e3.md`

**B-8ec0b5b5.** partnerhub_pages: the Cloudflare GraphQL Analytics CPU query found by task 34 was never written to a durable note, so preview-cpu-trim task 9 re-researched it (3 Context7, 2 searches, 2 fetches).

Evidence: `notes/backlog-B-8ec0b5b5.md`

**B-34d901f9.** Census 2026-11-14..20: investigators made 0 Context7 calls against 101 WebFetch and about 92 curl lookups; 15 of 126 calls missed or 404'd on guessed Cloudflare and Strapi URLs. Plan workers do follow Context7-first. Decide whether investigators should try Context7 (or a search) before guessing URLs.

Evidence: `notes/backlog-B-34d901f9.md`

**B-0e2db574.** partnerhub_pages milestone C round 2 failed a 180 px brand-selector defect that round 1 passed on the same code. Options: re-judge only states the fix tasks touch, or carry a passed state's verdict unless its capture changed.

Evidence: `notes/backlog-B-0e2db574.md`

**B-80fd8af7.** reportdesignkit (UI-heavy) has no capture config, so no milestone carried ui_states; K's elaboration resized a page without the layout step and the user found defects by eye. Planning (or a plan check warning) should ask for the capture command when a milestone touches UI.

Evidence: `notes/backlog-B-80fd8af7.md`

**B-d05d1f05.** dev-tools: mid-run-requests F36 cites glimpse-v2-visibility D12 and v2-harness-tuning D16, and implement-v2-run-hardening D13 repeats it. The underlying claims (edited skills/agents load from the next session; AskUserQuestion blocks a SendMessage) are asserted from memory, never probed; one probe would settle the rule for both engine skills.

Evidence: `notes/backlog-B-d05d1f05.md`

## Unknowns

Every lens mines the session transcripts under `~/.claude/projects/` (the project folders for dev-tools, partnerhub-pages, strapi-server and reportdesignkit, each session's `subagents/` and Workflow journals) and the consumer repos' flow stores and notes under `~/dev/<repo>/`, for 2026-11-14..20, read-only; then reads the harness text and code as it stands after implement-v2-run-hardening. Each note paraphrases (session id, time, ids, repo names; no verbatim logs, machine paths or secrets), states the current text's gap with file:line, and proposes a design with its options and tradeoffs.

- fix-rounds: For B-d7f07e99, B-5826b324 and B-95a60315, what exactly happened in each cited run, how do the drain's fix rounds, accept ownership (`run_by`, `covered_by`) and reruns work now, and what is the smallest design that reruns failures at the base commit, licenses a worker's own reproducer without parallel-build thrash, and narrows a rerun after a whitespace-only fix?
- gate-waiver-and-orphans: For B-686f291e and B-37932bb6, what happened on reportdesignkit milestones E and H and in the stopped-gate incident, how does the visual accept record run-hardening added work (store, commit step, report, backlog follow-up), and what would a gate waiver on the same pattern and a verification-agent process cleanup (pid file, lock) need?
- decisions-and-baseline: For B-ec551154, B-2053e101 and B-bd50caa8, what happened (partnerhub preview-cpu-trim D8 and fix task 11, its static-identity gate, strapi F78, dev-tools U2), how do the replan, Prove's gate probes and the plan review's re-check and triage work now, and what design catches a fix that contradicts a decision, a gate failing on the base tree, and records changed after the re-check?
- handovers-and-elaborate: For B-8aee5fe3, B-caa21ce6, B-959255fa and B-ba011350, what happened (partnerhub H2/H5/H9, strapi milestone C elaboration, strapi brief edit mid-launch), how do handovers, premises, `plan handover done`, elaborate and the fan-out check work now, and what design blocks a task on a user-supplied input, commits done-marks, reconciles hand fixes at elaboration and keeps brief edits from tripping the check?
- research-sourcing: For B-98d14bca, B-6d3fb3e3, B-8ec0b5b5 and B-34d901f9, what happened (partnerhub task 34's wrangler tail spec, H2/H5 console labels, strapi F78, the repeated header-page fetches, the GraphQL query re-researched), what do research-methods, the investigator, finder, verifier and worker files say about sources today, and what design makes orchestrator-written platform claims sourced, shares fetched sources within a review run and across tasks, and steers investigators to Context7 or search before guessed URLs?
- visual-and-self-edit: For B-0e2db574, B-80fd8af7 and B-d05d1f05, what happened (partnerhub milestone C visual rounds, reportdesignkit's missing capture config and K's elaboration, the dev-tools self-edit decisions F36, D12, D16, D13), how do visual rounds, ui-layout and the engines treat these now, and what does Claude Code documentation or a probe settle about when edited skills and agent files take effect in a running session and whether a pending AskUserQuestion blocks a SendMessage?
