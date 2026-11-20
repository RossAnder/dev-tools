Verdict: no restructure — each of the nineteen causes lands on a seam the engine already has (the gate-round record, the visual-accept pattern, the accept-owner vocabulary, the premise `at` vocabulary, decision binds, the shared worker procedure block, prove-by-reference from elaborate); the two candidate restructures save no edit this change makes.

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 363
claim = "The gate passes only when every block is pass or flaky; anything else blocks the commit"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 378
claim = "Fix rounds are counted from `gate <id> round ` reasons on minted fix tasks, so a failure that mints nothing spends no round"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 427
claim = "The commit condition names only the gate pass and the visual acceptance"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 473
claim = "Stamping on re-entry reruns the gate for an unstamped milestone; only the visual path reads an acceptance first"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 318
claim = "The gate command list is never filtered by hand; narrowing must come from the verb"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 108
claim = "The transcript audit's allowed list, which a worker reproducing check must fit"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 469
claim = "The flow-store commit runs only after a milestone commit"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 35
claim = "The visual pass reads its acceptance before capturing, the shape a gate waiver must copy at re-entry"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 55
claim = "A visual capture is recorded only by its run row and notes; no per-state verdict or capture digest is stored"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 223
claim = "The existing user-accept procedure: AskUserQuestion, backlog item, record, commit, report line"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 118
claim = "Re-entry for unstamped milestones reruns the gate and stops again after round 2; it reads only the visual acceptance"

[[source]]
path = "tomlctl/src/plan/run_visual.rs"
line = 41
claim = "The visual acceptance leaf: payload parse, milestone checks, upsert keyed on (milestone, round)"

[[source]]
path = "tomlctl/src/plan/run_visual.rs"
line = 231
claim = "Private payload helpers check_keys, entries and text, copied identically in request.rs"

[[source]]
path = "tomlctl/src/plan/request.rs"
line = 285
claim = "Second copy of check_keys, entries and text"

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 153
claim = "Gate runs are keyed (milestone, round, seq); a new run or a --rerun merge, with no notion of a base-commit run"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 590
claim = "write_version is a strict ladder: each new record kind needs a new schema constant and rung"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 662
claim = "Existing stores are read under the record name visual_accept"

[[source]]
path = "tomlctl/src/plan/drift.rs"
line = 48
claim = "The only stored base commit is the approval commit; there is no per-milestone base"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 515
claim = "Decisions binding a row by task id or path are already found for the worker view"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 377
claim = "run_by = worker is refused with writes_build = true: a worker never runs a building check"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 416
claim = "Premise `at` vocabulary is base or after:<id>, the existing not-yet seam"

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 79
claim = "The build discipline sits inside the shared plan-worker-procedure block, so one edit covers both tiers"

[[source]]
path = "claude/agents/verification.md"
line = 84
claim = "Step 1b launches long gates with nohup, detached from the agent"

[[source]]
path = "claude/agents/verification.md"
line = 125
claim = "The verification agent may not change directories beyond what a command does"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 191
claim = "Elaboration probes defer to write-and-prove 4d by reference"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 408
claim = "Gate probes are emitted and recorded at prove time, the seam for a base-tree comparison gate run"

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 95
claim = "The fan-out check digests untracked files including brief.md, which is why a brief edit trips it"

[[source]]
path = "claude/skills/plan-v2-engine/scripts/fanout-check.sh"
line = 34
claim = "The only carve-outs are --exclude paths, note paths and the flow store"

[[source]]
path = "claude/skills/implement-v2-engine/references/report.md"
line = 30
claim = "The report's gate: section already carries per-milestone gate and visual lines a waiver line joins"

[[source]]
path = "claude/commands/implement-v2.md"
line = 29
claim = "The pre-flight refuses a binary by a features list, the place the new verbs' feature names go"
```

## Why the present structure holds

The change is about twenty independent edits across the engine's prose, the agents and tomlctl. I traced each to the edits it would force. Each one either extends a vocabulary or record the code already has, or follows a pattern the code already uses once. No module forces the same edit to be made in several places that a different layout would collapse.

### Theme 1: gates and fix rounds

- **Gate failure already present at base.** Fix rounds are counted from `gate <id> round ` reasons on minted fix tasks (`drain.md:378`). A failure that mints nothing therefore already costs no round. Two edits are forced, and both are extensions:
  - The pass rule at `drain.md:363` requires every block to pass, so a base-failing command still blocks the commit until that rule learns to set it apart.
  - `run_gate.rs:153` keys a run on `(milestone, round, seq)` and cannot mark a run as taken at base. The base result needs a field or flag on `GateCommand` or `GateRound` (`schema.rs`).
  - The planner must also define the base commit. Only `approved_commit` is stored (`drift.rs:48`), and `verification.md:125` keeps the agent from changing directories, so a base tree means a command that carries its own worktree or checkout.
  - Refuted if the gate record already holds a base or pre-existing marker. It holds none at `run_gate.rs:28` `ROW_KEYS`.
- **Fix or diagnose worker running its own reproducing check.** `tasks/schema.rs:377` refuses `run_by = "worker"` with `writes_build = true`. The worker procedure at `plan-worker-deep.md:79` never runs a gate-owned check, and the audit allowlist at `drain.md:108-112` would mark one a breach. The change extends the owner vocabulary (`RUN_BY_OWNERS`, `tasks/schema.rs:62`) or adds an isolation rule such as a private target dir. That extends `owner()`, the view's `Run by:` line, the procedure block and the allowlist.
  - The procedure is a SHARED-BLOCK in both tiers (`plan-worker-lite.md:12-144`), so the worker edit is made once, not twice.
- **Whitespace-only fix reruns only the failed commands.** `drain.md:318` forbids narrowing the list by hand, so the narrowing has to come from `plan emit gate-list`, given the last `[[gate_round]]`'s failed commands. This is a flag on an existing verb.
- **User accepts a failing gate after the last round.** This repeats `visual.md:223-254` and the `run_visual.rs:41-80` leaf: a new record keyed `(milestone, round)`, its schema rung, and these prose sites:
  - the two-rounds stop (`drain.md:384`);
  - the commit condition (`drain.md:427`);
  - Stamping on re-entry (`drain.md:473`);
  - Next (`drain.md:477`);
  - the SKILL.md steps table and the Unstamped paragraph (`SKILL.md:61`, `SKILL.md:118`);
  - the report's `gate:` block (`report.md:30`).

  The site most easily missed is re-entry. `SKILL.md:118` says "a milestone stopped after round 2 stops again". It must read the waiver before rerunning the gate, as `visual.md:35-41` reads `[[visual_accept]]` before capturing. Otherwise an accepted gate is asked again, or stops again, at the next invocation.
- **Gate process outliving its agent.** The `nohup ... &` launch is at `verification.md:84-98` and nowhere else. This is a one-file edit.

### Theme 2: plan and decision integrity

- **Fix task contradicting a decision.** `view.rs:515` already finds the decisions that bind a row by task id or path. The fix-round minting at `drain.md:367-372` can list those decisions for each minted row before the relaunch. This is an extension of the minting step, plus at most a query surface for `bind_names`.
- **Proving comparison and snapshot gates on the base tree.** Gate probes are emitted and recorded at `write-and-prove.md:408-419`. `elaborate.md:191` reaches the same procedure only by reference, so editing write-and-prove 4d covers elaboration.
  - One caveat: the elaborate copy repeats the command lines (`elaborate.md:193-200`). If a new flag is introduced, that block takes it too.
- **A task waiting for a user input.** The premise `at` vocabulary (`tasks/schema.rs:416`, `base` or `after:<id>`) and the withholding at launch (`run-milestone.md:134-137`) are the existing "not yet" seam. Either a new premise kind or a row field fits there.
- **Handover done-marks committed.** No engine reference calls `plan handover done`. It appears only in the tomlctl reference. The flow-store commit runs only after a milestone commit (`drain.md:469`), so a done-mark made between milestones never lands. The fix is a commit point in the flow, not a new structure.
- **Brief edits during a launch.** `open-and-challenge.md:95` digests every untracked file, `brief.md` included, and `fanout-check.sh:34` carves out only `--exclude` paths, note paths and the flow store. The fix is either a rule against editing during a launch or a separately reported orchestrator-owned path. Both are local to the script and that section.
- **Review re-check drift and elaboration reconciliation.** Both land in `approve.md` and `elaborate.md` section 1, which I did not read in full (see Coverage).

### Themes 3 to 5

- **Research sourcing.** The `toml sources` block is already parsed by `plan/note_sources.rs`. Worker notes are already recorded as attempts. Carrying sources forward to later tasks' views, and adding a sourcing rule to the agent prompts, extend what exists.
- **Visual regressions on unchanged captures.** This is the one theme that needs state the design deliberately left out. `visual.md:55` records a capture only by its run row and notes, so no per-state verdict or capture digest survives a round. "Does not fail a state an earlier round passed unless its capture changed" forces a stored per-state `{state, verdict, digest}` per round: a new plan record with its own schema rung, written by a `plan run visual` leaf. This adds a record. It does not reshape the code.
- **Missing capture config, self-edit rule and pre-flight.** These are prose in `ui-layout.md` and `run-milestone.md`, plus the feature list at `implement-v2.md:29` and its plan-v2 counterpart.

### Sequencing the planner should know (not a restructure)

`schema.rs:590-605` is a strict ladder. Each new plan record kind adds a constant and a rung, and changes the stamp documented in `plan-store.md`. This change likely adds two or three of them: a gate waiver, a visual round digest record, and possibly a store-held source. Each rung also adds a store that older binaries refuse.

If two milestones each add a rung, they edit the same lines and must agree on numbering, so assign the version numbers in the plan or land the new record kinds in one milestone.

The payload helpers `check_keys`, `entries` and `text` are byte-identical in `request.rs:285-318` and `run_visual.rs:231-264`. A gate waiver leaf would be a third copy. The task that adds the leaf can lift them if it holds both files. That is an extension-time tidy, not a separate restructure.

## Alternatives ruled out

- **Extract drain step 7 into a `gate.md` sibling of `visual.md`.** The gate-waiver edits also hit step 10, Stamping on re-entry, Next, `SKILL.md` and `report.md`, which an extraction does not spare. About five cross-references to "drain.md, step 7" would move for no saved edit. This is a preference, not a forced edit.
- **One generic acceptance record for gate and visual.** `schema.rs:662` reads `visual_accept` in stores already stamped 6 in consumer repos, so renaming forces a read-both migration. The two payloads differ: commands and failed ids against states, looks and defects. The rounds are counted apart (`visual.md:31`), so the commit and re-entry prose still branches per pass. Refuted if the commit and re-entry sites could read one record without branching, and at `drain.md:427` and `drain.md:473` they cannot.
- **Merging the `plan-worker-lite` and `plan-worker-deep` prompts.** They already share the procedure, forbidden-ops, file-edits and tomlctl blocks through SHARED-BLOCK markers. The remaining difference (Judgement against When to return failed) is the point of having two tiers.
- **Table-driving the schema version ladder.** A rung is a constant plus two lines. A table spares none of the per-kind struct, from/to and test code each record needs anyway.

## Coverage

- **Read in full:** `claude/skills/implement-v2-engine/references/drain.md`, `claude/skills/implement-v2-engine/references/visual.md`, `claude/skills/implement-v2-engine/SKILL.md`, `claude/agents/verification.md`, `tomlctl/src/plan/run_visual.rs`, `tomlctl/src/plan/run_gate.rs`.
- **Read in part:**
  - `tomlctl/src/plan/schema.rs` (header, PlanStore, records, write_version, from/to TOML)
  - `tomlctl/src/tasks/schema.rs` (header, run_by owners, premise_target, MilestoneSpec)
  - `write-and-prove.md` 4d, `elaborate.md` 4, and `open-and-challenge.md` sections 3 and 4
  - `fanout-check.sh` (arguments and pathspecs) and `report.md` (report shape)
  - `request.rs` (helpers) and the `probes.rs`, `inputs.rs`, `ready.rs` and `note_sources.rs` headers
  - the `plan-worker-lite` and `plan-worker-deep` diff
  - `implement-v2.md` (pre-flight)
- **Not read:** `approve.md`, `elaborate.md` section 1, `ui-layout.md`, `run-milestone.md` beyond its premise and launch lines, `questions.md`, `emit.rs`, `claude/workflows/review-v2.js`, the review lens and investigator prompts. The review-recheck, elaboration-reconcile, review fetch-sharing and Context7 claims above therefore rest on where these change sites sit, not on a reading of their bodies.
