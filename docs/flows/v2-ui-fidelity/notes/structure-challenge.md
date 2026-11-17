Verdict: no restructure. The harness already has the shape this work needs: a per-milestone conditional slice that only milestones carrying the property read (apply rows), optional milestone facts written only when non-empty (gate_probes), and a pluggable review catalogue. Each part of the UI work lands as an extension in a seam that already exists, and a plan with no UI work reads none of it.

```toml sources
read_at = "52fe01b9a17c76c7068e241fa891e6cacd6fcbc6"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 70
claim = "apply-rows.md is a conditional slice: a milestone with no apply row reads none of it"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 27
claim = "rule 4: the verification agent is the engine's one Agent dispatch"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 17
claim = "the engine never runs a build, test suite or gate inline"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 89
claim = "the open-run rule names the engine's step prefixes implement: and reproduce:"

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 61
claim = "the reproduce check is a judged pass before commit, with its own run step, launch rules and report path"

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 171
claim = "fix rounds are counted from a reason prefix in the audited changes, two rounds at most per pass"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 291
claim = "the chain from the drain to the gate is written out once, as reproduce, then settle, then gate"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 367
claim = "a passing gate goes straight to the commit"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 408
claim = "step 9 already rejects a handover for a step an agent could perform"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 474
claim = "stamping on re-entry lists the pre-gate passes to rerun"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 542
claim = "the needs_human report's section-to-kind table"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 40
claim = "the pre-flight stale-row reset names implement: and reproduce: runs"

[[source]]
path = "claude/agents/verification.md"
line = 127
claim = "verification must not interpret output; it is haiku with Bash, Read and Grep only"

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 4
claim = "plan workers hold no Playwright tools"

[[source]]
path = "claude/agents/review-finder.md"
line = 4
claim = "review-finder already holds the Playwright observation tools"

[[source]]
path = "claude/skills/research-methods/references/browser-observation.md"
line = 17
claim = "agents attach only to a named dev server and never start or stop one"

[[source]]
path = "claude/skills/review-v2-lenses/checks/dev-server-probe.md"
line = 1
claim = "the review surveyor only detects a running dev server and never starts one"

[[source]]
path = "claude/skills/review-v2-lenses/vocabulary.toml"
line = 138
claim = "a frontend diff signal already exists for UI files"

[[source]]
path = "claude/skills/review-v2-lenses/SKILL.md"
line = 12
claim = "the catalogue is a set of drop-in entry files: lenses, facets, methods and checks"

[[source]]
path = "claude/skills/review-v2-engine/SKILL.md"
line = 103
claim = "only plan-family runs take the brief as scope; a code-family run on a flow does not see it"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 369
claim = "MilestoneSpec holds a milestone's execution facts: gates, elaboration, gate_probes and after"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 1347
claim = "gate_probes is written only when non-empty, so a store without it is unchanged"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 276
claim = "an Accept entry has no key saying whether its behaviour is user-visible"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 170
claim = "ReportKind is a closed vocabulary of needs_human report kinds"

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 82
claim = "run step names are free strings; only elaborate: and cold-read: are special-cased"

[[source]]
path = "tomlctl/src/plan/milestone.rs"
line = 143
claim = "plan milestone add, update and the audited patch are where a new milestone field is threaded"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 506
claim = "the worker view injects decisions whose binds name the task"

[[source]]
path = "tomlctl/src/plan/approve.rs"
line = 226
claim = "approval already refuses unanswered surfaced decisions; one-way doors are only listed at the gate"

[[source]]
path = "claude/skills/plan-v2-engine/SKILL.md"
line = 16
claim = "role rule: ask only when the answer changes the plan; zero questions is the normal case"

[[source]]
path = "claude/skills/plan-v2-engine/SKILL.md"
line = 26
claim = "rules 3 and 4: each fan-out is one saved Workflow, and the challenger is the one Agent dispatch"

[[source]]
path = "claude/skills/plan-v2-engine/SKILL.md"
line = 93
claim = "re-entry resumes from the run rows of investigate, challenge and surface before Design"

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 144
claim = "the Surface filter: three tests, test 1 requiring a different plan, at most three questions"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 82
claim = "Surface again restates the three tests, and its test 3 already differs from the original"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 51
claim = "elaboration's surface step cites the open-and-challenge tests by reference"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 116
claim = "the worked spec example accepts behaviour with test -f and grep checks"

[[source]]
path = "claude/skills/plan-v2-engine/scripts/fanout-check.sh"
line = 34
claim = "the fan-out check takes --exclude paths beside the note paths"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 163
claim = "the observe handover reason covers judgements that need time or several real sessions"

[[source]]
path = "claude/commands/implement-v2.md"
line = 29
claim = "the implement-v2 pre-flight lists the tomlctl features the engine depends on"
```

## What the change does to the code as it stands

The future state touches three engines, but in each one the UI work is a property of one milestone or one surface. It does not change how every milestone runs. The present code already has a pattern for exactly that. `implement-v2-engine/SKILL.md:70` routes apply rows to `references/apply-rows.md` with "A milestone with no apply row reads none of it". `tasks/schema.rs:1347` writes `gate_probes` only when it holds an entry. The review catalogue (`review-v2-lenses/SKILL.md:12`) is a set of drop-in files. Following the change through edit by edit finds no module whose shape blocks it. What it does find is a list of specific edits the present code forces, given below so the planner can size them.

## Claims: what the present structure forces on this change

**1. The visual judgement cannot go into the gate's agent. It forces a new agent and a change to implement-v2's rule 4.** `verification.md:127` forbids interpreting output, and the agent is haiku with only Bash, Read and Grep (`verification.md:4-5`). The workers hold no Playwright tools (`plan-worker-lite.md:4`, and the same in `plan-worker-deep.md:4`). `review-verifier` holds none either. `review-finder` holds them (`review-finder.md:4`) but is bound to review-store cells. So "an agent looks at screenshots" means a new judged agent. Dispatching it breaks `implement-v2-engine/SKILL.md:27` ("the verification agent is the one Agent dispatch") unless it goes through a Workflow, as the reproduce check does (`apply-rows.md:93`). Refuted if an existing v2 agent with image-reading and judgement licence can be pointed at a set of screenshots without changing its contract.

**2. The visual pass lands as a new conditional reference beside `apply-rows.md`, not as a rework of the drain.** The reproduce check is the precedent for a judged pass before commit. It has its own run step (`reproduce:<id>`), its own launch rules, its own fix-round prefix counted from the audited `changes` (`apply-rows.md:171`), and its own `needs_human` path. Run step names are free strings (`plan/stage.rs:82` special-cases only `elaborate:` and `cold-read:`), so a `visual:<id>` step needs no tomlctl change. The edits it forces are each one clause, in places that list the passes:
- `drain.md:291`, which writes the chain from the drain to the gate.
- `drain.md:367`, where "passes the gate: go to the commit" must first run the visual pass. It runs after the gate, because a broken build renders nothing.
- `drain.md:474`, stamping on re-entry.
- The open-run rule at `SKILL.md:89`/`105`.
- `run-milestone.md:17` and `:40`.
- The plan-v2 open-run lists at `plan-v2-engine/SKILL.md:106` and `open-and-challenge.md:228`.

That is about ten one-token additions, and it does not justify a pass registry: the prose is read in order, and three passes are still easy to follow. Refuted if the planner finds the visual pass must change control flow for milestones without UI states, for example a gate whose order changes for every milestone.

**3. Something must serve the app, and no actor now may start a server.** Starting an app at gate time is forbidden to three actors as the code stands. The orchestrator never runs a build or gate inline (`implement-v2-engine/SKILL.md:17`). Agents attach only to a named server and "never start, restart or kill one" (`browser-observation.md:17`). The review surveyor only detects one (`dev-server-probe.md`). "Without the user opening the app" therefore forces one of two things. Either the capture (start the app, screenshot each declared state, stop it) becomes a deterministic command that the gate's `verification` agent runs, since it already runs long commands detached under a timeout. Or a new rule licenses one actor to own a server's lifecycle. Taking the first, the judged agent only reads PNGs. That leaves rule 4 and the no-server rule intact and keeps the capture reproducible. Refuted if a capture of the declared states can be produced with no running process, such as a static render.

**4. Declared states belong on the milestone record in tomlctl. The record's present shape forces specific edits.** `MilestoneSpec` (`tasks/schema.rs:369`) holds a milestone's execution facts, and `gate_probes` is the template for an optional list that is absent from a store without UI (`schema.rs:1347`). The edits this forces:
- Parse and write in `schema.rs:1317-1387`.
- `plan milestone add`/`update` and the audited patch in `plan/milestone.rs:143-335`.
- A `FEATURES` entry, its README and capabilities test (the registration rule at `write-and-prove.md:169`).
- The feature list in the `implement-v2.md:29` pre-flight.

A design file alone would leave `plan check` unable to refuse a UI milestone that declares no states. Refuted if the planner keeps the states only in `design/`, accepting that no verb can check them.

**5. The report vocabulary is closed, so a visual defect reaching the user forces a tomlctl edit.** `ReportKind` (`plan/schema.rs:170-183`) has no kind for a visual defect. Either a new kind goes in, together with its row in the table at `drain.md:542`, or such defects are filed under `gate`. Refuted if `gate` lines are judged enough to carry a defect with its screenshot path.

**6. The picked layout reaches workers through existing mechanisms, with no new channel.** A decision whose `binds` name task ids is injected into the worker view (`plan/view.rs:506`; `write-and-prove.md:196`). A `design/<name>.md#anchor` citation is checked by `plan/design-anchor-dead` (`write-and-prove.md:61`). The workers can Read an image file. Recording the pick as a `user-stated` decision, bound to the UI rows and citing a design file that names the image, therefore needs no tomlctl change. "Before tasks are written" has two sites, Design (`write-and-prove.md` section 1) and elaboration (`elaborate.md` section 2). A single `references/ui-layout.md` cited from both follows the way `elaborate.md:51` already cites `open-and-challenge.md` section 5. Refuted if the worker view does not print bound decisions for a task (`plan view --for worker --task <n>` on a bound row settles it).

**7. Rendering layout options forces a change to plan-v2's dispatch rules and a fence for the image paths.** `plan-v2-engine/SKILL.md:26-27` allows saved Workflows plus exactly one Agent dispatch, the challenger. A drafter that renders mockups therefore needs a Workflow, an amended rule 4, or the orchestrator doing the rendering itself. Its output lies outside `notes/`, so the fan-out check must be given its directory with `--exclude`, which the script already accepts (`fanout-check.sh:34-39`). A pre-Design step that must survive sessions, the "what you will see and do" account, needs a run step. It also needs a place in the re-entry list at `SKILL.md:93` and a row in the step table at `SKILL.md:49-60`. Refuted if the orchestrator can render and show the options inline without a subagent.

**8. Changing the question rule forces edits in four places, and one of them is a duplicate that has already drifted. Edit it once.** The future state reverses three statements:
- `plan-v2-engine/SKILL.md:16`: "Ask only when the answer changes the plan… zero questions… is the normal case".
- Surface test 1, "It must be a different plan" (`open-and-challenge.md:146`).
- The cap of three (`:150`).

`write-and-prove.md:84-86` restates the three tests rather than citing them, and its test 3 already lists fewer stances than the original (`open-and-challenge.md:148` adds the brief and the user's earlier decisions in the session). `elaborate.md:51` cites by reference. Replacing the restatement in `write-and-prove.md` section 2 with a citation of `open-and-challenge.md` section 5 is the one move this note recommends. It is a dedupe inside an edit the change forces anyway, not a restructure. "A one-way door is never assumed silently" has a natural enforcement point: `approve.rs:226` already refuses unanswered surfaced decisions, and a one-way door that is `agent-assumed` and not surfaced could be refused at the same place. Refuted if the two copies of the tests are meant to differ.

**9. "No user-visible behaviour accepted on a grep or file-existence check alone" has no field to key on.** `Accept` (`tasks/schema.rs:276-290`) carries `behaviour`, `check`, `kind`, `writes_build` and `run_by`, but no marker for user-visible behaviour. So neither `plan check` nor the cold reader can tell which entries the rule binds without one. The spec guidance's own worked example accepts behaviour with `test -f` and `grep` (`write-and-prove.md:116-118,142-144`), and needs rewording. Refuted if the rule is enforced only by judgement, by the cold reader and plan review, and never mechanically.

**10. The review side is catalogue work plus one survey input.** A UI lens is a new `lenses/<id>.md`. The `frontend` signal (`vocabulary.toml:138`), the `dev-server-probe` check and `review-finder`'s Playwright tools (`review-finder.md:4`) already exist. Checking built work against the brief and the user's decisions is different: it forces an input that a code-family run lacks, because only plan-family runs take the brief as scope (`review-v2-engine/SKILL.md:103`). The natural carrier is a surveyor check that writes a fence for flow-bound runs holding the success criteria and the `user-stated`/`user-confirmed` decisions. Refuted if a code-family flow-bound run already puts the brief into its pack.

**11. Handovers.** `drain.md:408` already turns an agent-performable step into a task. The new rule is a sentence there, plus a clause on `observe` (`tomlctl/references/plan.md:163`), saying that a look-and-feel review settled by screenshots is not a handover. No structural change is needed.

## Alternatives ruled out

- **A cross-engine `flow-contract-ui` skill holding the state format, the defect taxonomy and the capture procedure.** Each piece already has a single home that both sides read. The states live on the tomlctl milestone record, which plan writes and implement reads through verbs. The defect taxonomy fits a review-catalogue lens file, which the implement-side inspector can be pointed at by path, as the reproduce check already reuses review-v2 machinery. A contract skill would be a third home.
- **Folding the visual check into the gate as an accept-check kind.** The gate's runner must not interpret output (claim 1).
- **A generic registry of passes between the drain and the commit.** Adding the visual pass grows about ten enumeration sites by one token each (claim 2). A registry would put indirection into procedure prose that is read in order.
- **A UI mode inside the `implement-v2-milestone` Workflow.** That Workflow is a pool of task workers writing their own rows. A visual pass judges the whole milestone after the gate and writes no task rows, which is the shape of the reproduce check's separate launch, not the pool's.

## Coverage

Read in full:
- `claude/skills/plan-v2-engine/SKILL.md`
- `claude/skills/implement-v2-engine/SKILL.md`
- `claude/skills/implement-v2-engine/references/run-milestone.md`
- `claude/agents/verification.md`, procedure and rules
- `claude/skills/review-v2-lenses/checks/dev-server-probe.md`
- `claude/skills/research-methods/references/browser-observation.md`

Read in part:
- `drain.md`, steps 7 to 11
- `apply-rows.md`, the reproduce check
- `open-and-challenge.md`, sections 1 to 5
- `write-and-prove.md`, sections 1 to 3
- `elaborate.md`, sections 1 to 3
- `approve.md`, Review and Approve steps 1 to 3
- `review-v2-lenses/SKILL.md` through the activation tiers
- `review-v2-engine/SKILL.md` stage 1
- `fanout-check.sh` argument handling
- `implement-v2-milestone.js` head
- The tomlctl `MilestoneSpec`, `GateProbe`, `Accept`, `ReportKind`, `stage.rs` open-run handling, the `milestone.rs` outline and the `approve.rs` outline
- Every agent's frontmatter

Not read: the cold-reader, investigator and review-lead procedures, `plan-v2-review.js`, `review-v2.js` beyond its reproduce mode header, and `tomlctl/src/plan/check.rs` rule bodies. No build or test was run.
