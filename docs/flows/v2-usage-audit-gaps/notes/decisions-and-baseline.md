Verdict: All three gaps stand after run-hardening. A gate fix row is minted with no check against the decisions (drain.md:367), and a decision bound by task id never reaches a fix row on the same files (view.rs:644). Prove's gate probes test the environment and never run the gate itself on the base tree (write-and-prove.md:408, shapes.md:101). The re-check covers only the targets findings named, and no later step compares records against it (plan-flow.md:308, approve.md:83).

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 151
claim = "The replan rule: a change to a decision goes to the user as a parked row, never through the replan"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 367
claim = "Fix rounds mint fix tasks from failing blocks (files, accept, executor, reason) with no step that checks them against decisions"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 374
claim = "Carry-the-cause-forward adds constraints to pending rows; nothing amends or re-binds decisions"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 512
claim = "The worker view prints only decisions whose binds name the task id or a path in the row's files"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 644
claim = "bind_names: a task-id bind matches that id alone, so a fix row on the bound task's files does not inherit the decision"

[[source]]
path = "claude/skills/implement-v2-engine/references/questions.md"
line = 63
claim = "The existing user-stated decision record shape (plan decide, binds a task id) that a decision amendment at the drain can reuse"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 64
claim = "A gate gets a probe only when it can fail for environment reasons; the probe is a cheap command such as a dry run"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 378
claim = "Accept probes cover only writes_build = false entries, so a build-writing comparison check is never probed"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 408
claim = "Gate probes run the stored --gate-probe commands only, under a 300 s limit"

[[source]]
path = "docs/flows/v2-harness-tuning/design/shapes.md"
line = 101
claim = "Gate-probe design: a probe fails where the gate cannot pass in this environment; the gate itself is never a probe"

[[source]]
path = "claude/skills/review-v2-engine/references/plan-flow.md"
line = 308
claim = "A re-check runs only over recheck-targets: applied targets and addressed entries of the first run"

[[source]]
path = "claude/skills/review-v2-engine/references/plan-flow.md"
line = 358
claim = "Re-check findings are asked or left open, and an accepted one is applied with no further check"

[[source]]
path = "claude/skills/review-v2-engine/references/plan-flow.md"
line = 249
claim = "A hand fix is recorded as addressed with the targets the author names; untargeted edits are not covered"

[[source]]
path = "claude/skills/plan-v2-engine/references/approve.md"
line = 70
claim = "Gate risks hold only skip-reason and review-finding rows, with no slot for records changed after the re-check"

[[source]]
path = "claude/skills/plan-v2-engine/references/approve.md"
line = 83
claim = "A change asked for at the gate routes back to its owning step and Prove re-runs, but the review does not"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 191
claim = "Precedent: store_digest hashes task-store sections for an implement run; review runs carry no such digest"

[[source]]
path = "claude/skills/review-v2-method/references/plan-records.md"
line = 44
claim = "Plan target grammar (task fields, spec fields, decision keys, milestone gates) that a changed-record delta can be expressed in"

[[source]]
path = ".claude/backlog.toml"
claim = "B-ec551154, B-2053e101 and B-bd50caa8 as captured on 2026-11-20"
```

## What happened

**partnerhub_pages, preview-cpu-trim (B-ec551154 and B-2053e101).** In plan session b2a1b292, between 13:02Z and 13:15Z on 2026-11-20, the orchestrator added an island-uid mask, among other build-artefact masks, to the parity normaliser (commit 7be8b96). About an hour later it wrote D8, which was agent-assumed, a two-way door, and bound to normalise.ts and task 8. D8 says the static-identity gate masks only deploy ids, `/_astro/` hashes and build stamps, and never uses `normalise()`. It was rewritten once more at 14:13Z, after the review finished at 14:13:17Z and before approval at 14:16Z. Task 8's detail repeats the rule but allows one narrow named mask if a run shows other noise. The task 8 worker (implement session cd978dc2, run wf_d6446ecc-e7a) left a tangential note: the two-build run was never exercised, and island-uid stability between two builds was unverified. At 14:32Z the orchestrator chose not to capture it, because the gate would answer it.

Gate A round 0 ran the static-identity gate for 718 s and reported 401 differing files. 399 of them differed first at an island uid, including pages no task touched, and two were line-ending differences. At 14:53Z the drain minted fix task 11 (deep). Its outcome and constraints allow a named mask for location-derived island uids and CRLF-to-LF folding. The worker found the uid is a hash over the component's absolute source path, so it differs whenever the base is built in another directory, and it masked it. The round 1 gate passed. The user was asked at 14:51Z about two unrelated failing tests and the deploy, but never about D8. D8 was not amended, still lists only the three masks, and is committed that way in 33f512d. Task 11's worker view carries no "Decisions binding this task" section. On B-2053e101: the static-identity gate had no gate probe. Its milestone's only probes were two wrangler version and whoami checks, recorded at 13:41Z. The script did not exist at Prove, because task 8 creates it.

**strapi-server, cloud-deploy-parallel-run (B-bd50caa8, first half).** In plan session ac680855 on 2026-11-16, review U1 ran from 08:58Z to 09:36Z and its re-check U2 from 09:36Z to 10:06Z. F78 is an advisory with route none, so it never reached a verifier. It was raised by the re-check U2 itself and said the image's Bun pin did not match the Bun that maintains the lockfile. At 10:05Z the orchestrator fixed it by hand. It took the version from the local `bun --version`, a canary that has no published image, and edited the design section and task 16's detail. It recorded F78 as addressed with both targets, and the 26 re-check edits accepted at 10:04Z were applied at the same point. U2 was then finished at 10:06Z and nothing re-checked those edits. Gate A round 1 failed because that image tag does not exist, and fix task 31 re-pinned it.

**dev-tools, implement-v2-run-hardening (B-bd50caa8, second half).** In plan session c622969d on 2026-11-20, the U1 triage at 13:42Z to 13:46Z re-cut the milestones (binary first, prose second). It moved rows between A and B, added task 16 (the pre-flight feature list), and revised D2, D5 and D13. The re-check U2 ran over the 15 `recheck-targets`, which name neither task 16, nor the moved rows' `checkpoint`, nor the milestone records. At 13:47Z the orchestrator told the user that limit and said it would list it at the gate. The 13:57Z gate showed "Risks: none", because the gate's `risks` array has no row kind for it. Five more re-check edits (F22 and F24 to F27) were applied at 13:57Z, after which nothing ran a check.

## How it works now

- **Replan.** drain.md:151 already routes "a change … to a decision" to the user, but only as a parked row and only through the step-6 inputs table. Fix rounds (drain.md:367-372) mint rows from the failing block's tail, with files, accept, executor and reason, and no step reads the decisions. A decision reaches a worker only through `binds` (view.rs:512). bind_names (view.rs:644-658) matches a task-id bind to that exact id, so D8 (bound to `8` and normalise.ts) never reached fix task 11 on static-identity.ts. The fix spec was the orchestrator's own text, so the contradiction was authored in the drain, not by the worker.
- **Prove.** Accept probes skip every entry with `writes_build = true` (write-and-prove.md:378). Gate probes run only the stored `--gate-probe` commands, defined as cheap environment checks (write-and-prove.md:64, 408-420). The design rules out the gate itself as a probe (shapes.md:101). Nothing runs a comparison or snapshot gate on an unchanged tree, before or during the milestone.
- **Plan review.** The re-check covers the `recheck-targets` of the first run only (plan-flow.md:308-314). Its own findings are applied or left open, with no further pass (plan-flow.md:358-361). A hand fix covers only the targets the author names (plan-flow.md:249-255). After the step closes, a change at the gate goes back to its owning step and Prove re-runs, but the review never does (approve.md:83). The gate's `risks` hold only skip reasons and open review findings (approve.md:70). Review runs store no record digest; lane.rs:191 shows the pattern, used only for implement runs.

## Findings

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:367 (Fix rounds) and tomlctl/src/plan/view.rs:644 `bind_names`
- **Source**: drain.md:151, 367-374; view.rs:512-516, 644-658; `tomlctl plan view --for worker --task 11` in the consumer flow printed no decisions section, while task 8's printed D8; implement session cd978dc2, 14:49Z to 14:54Z
- **Evidence-grade**: high
- **Finding**: needs-plan. A gate fix row can contradict a recorded decision with nobody asked. Fix this in two parts: the tool shows which decisions a new row touches, and the drain prose reconciles each one with the user before the row is launched.
- **Details**: Two holes. (1) Mechanical: bind_names treats a `binds` entry of digits as that task alone. Options: (a) have a task-id bind also match any row holding a file of the bound task, so the decision follows its files; this is the cheapest, and every fix row on the same file inherits it. (b) Have `tasks add-many` or a `plan emit` leaf print, for each new row, the decisions bound to its files or to tasks holding them. (c) Add a `plan check` warning for a gate-round row that shares files with a decision-bound task while the decision does not bind the row. (2) Judgement: drain.md step 7 gains a step. For each fix row, read the decisions the tool lists, and when the fix's outcome or constraints depart from one, put it to the user before launch through `AskUserQuestion`. The options are amend the decision (`plan decide` on the same id, provenance user-confirmed, binding the fix row) and mint, or reshape the fix to keep it. Asking at the drain costs one question. drain.md:151's parked-row route would cost a relaunch, so the step-7 text should say asking replaces parking for fix rows. Tradeoff: (a) widens every worker view that shares a file with a bound task, but a decision on a file is usually meant for every row on it.
- **Counter**: If the task 8 detail's "narrow named mask" clause is taken as part of D8, task 11 did not contradict it and only the record's wording drifted. Even then the decision text, the design's expectations list and the fix diverged, and no one was asked.
- **Impact on plan / report**: one tomlctl change (view.rs bind_names or an emit leaf, with tests) plus drain.md step-7 text, with a questions.md cross-reference for the record shape (questions.md:63-82).

- **Library/API or file**: claude/skills/plan-v2-engine/references/write-and-prove.md:408 (gate probes) and docs/flows/v2-harness-tuning/design/shapes.md:101
- **Source**: write-and-prove.md:64, 378, 408-422; shapes.md:101; consumer gate record round 0 (static-identity, 718 s, 401 differing) and round 1 (pass, 199 s); plan session b2a1b292, 13:41Z, wrangler-only probes
- **Evidence-grade**: high
- **Finding**: needs-plan. Prove never runs a comparison or snapshot gate on the unchanged tree, so a gate that fails on the base is found only at the milestone gate, at the cost of a round.
- **Details**: Options. (a) Add a gate kind, e.g. `--gate-baseline '<gate>'`, for a gate that must pass on the base tree. When its command exists at Prove, the `verification` agent runs it once on HEAD with a timeout and the verdict is recorded beside the gate probes. The orchestrator must not run it inline, since it can take 12 minutes and Prove's limit is 300 s. (b) When a milestone task creates the gate's tool, as task 8 did here, Prove cannot run it. That task's accept then carries a null control: the comparison of the base against itself reports no difference (`writes_build = true`, run by the gate and listed before the gates that use it), so a failure points at the tool, not the work. A write-and-prove authoring rule plus a `plan check` warning for a baseline gate with neither a probe nor a null-control accept would enforce this. (c) Rerun a failing gate at the base commit in the drain. This belongs to the fix-rounds lens (B-d7f07e99) and only catches the failure after the round is spent. Tradeoff: (a) adds minutes of build to planning, while (b) costs a gate slot and nothing at planning. Here the tool did not exist yet, so only (b) would have caught it, as the task 8 worker's tangential question suggested.
- **Counter**: A Prove-time base run would show nothing for a gate whose noise appears only with the milestone's changes. Here, though, the noise came from the build location and appeared on pages no task touched, so a base-against-base run reproduces it.
- **Impact on plan / report**: tomlctl milestone field and check class with tests, `plan probes` or the gate-list emit, and write-and-prove.md section 4d text; cross-reference B-d7f07e99.

- **Library/API or file**: claude/skills/review-v2-engine/references/plan-flow.md:308 (Re-check) and claude/skills/plan-v2-engine/references/approve.md:83
- **Source**: plan-flow.md:249-255, 308-314, 358-361; approve.md:70, 83; lane.rs:191; strapi review store F78 (seen_in U2, addressed, resolved 2026-11-16) and session ac680855, 10:04Z to 10:06Z; dev-tools `tomlctl review recheck-targets --run U1 --slug implement-v2-run-hardening` (15 targets, none on task 16 or the checkpoints); session c622969d, 13:47Z and 13:57Z
- **Evidence-grade**: high
- **Finding**: needs-plan. Plan records changed after the re-check's targets were fixed are never reviewed. That covers re-check triage edits, hand fixes of re-check findings, rows added or moved in triage, and the orchestrator's own edits up to approval. Close it with a record-level digest and a delta check before approval.
- **Details**: Options. (a) Stamp a record digest when the review run starts (one hash per target in plan-records.md:44 grammar: each task field and spec field, each decision key, each milestone's gates and rows, each brief and design anchor), as lane.rs does for implement runs. `plan stage` and `plan approve --review ran` then compare it with the stores and list the changed targets not covered by a finished run. A non-empty list sends the plan to a quick recheck over exactly those targets. The `plan-v2-review` recheck mode already takes `targets` (plan-flow.md:354). Repeat until the delta is empty or the user waives it, and the waiver shows under `risks` as a new `kind`. (b) Prose only: section 10 loops while the re-check's triage applied or addressed anything, and the author must name `--targets` for rows added or moved. This is cheaper, but it misses edits made outside triage, like D8's at 14:13Z, and relies on the author remembering. (c) Show the delta at the gate only, as a `risks` row, with no recheck. This is cheapest and leaves the decision to the user. Tradeoff: (a) costs one more quick run, of about 15 to 24 agents in these runs, whenever triage edits anything. Waiving under a small-delta threshold, or for edits that are `checkpoint`-only, keeps it bounded. An unverified advisory such as F78 that the author applies by hand would be re-checked under (a), but only a sourced fact closes the root cause, which belongs to the research-sourcing lens (B-98d14bca).
- **Counter**: If a quick recheck of a hand-applied pin would not have caught a nonexistent image tag, (a) adds cost without catching the strapi case. The fix then belongs to B-98d14bca, and this item closes on the dev-tools half alone.
- **Impact on plan / report**: tomlctl review-run digest field, an approve/stage check with tests, and new text in plan-flow.md sections 10 and 11 and approve.md (risks kind, refusal row).

Coverage: read in full or in the cited part: the three backlog rows and the brief; drain.md 145-240 and 300-391; questions.md 55-92; write-and-prove.md 20-70 and 370-429; plan-flow.md (grep plus 300-380); approve.md 1-151; shapes.md 90-125; view.rs 500-540 and 644-670; the partnerhub plan.toml D8 record, attempts and gate rounds, the task 8 and 11 worker views, the task 11 run note, and git show 7be8b96. Transcripts: partnerhub b2a1b292 and cd978dc2, strapi ac680855, dev-tools c622969d, all scanned by keyword and time window. Ruled out: the decision record's `check` field as a hook, since no code reads it for decisions (check.rs reads only contract checks), and the cold-read and investigate steps as the fix point, since both were skipped or did not see the two-build run. Out of lens: rerunning gates at the base commit (B-d7f07e99) and source-less orchestrator facts (B-98d14bca).
