Verdict: Make `look` a fourth `run_by` owner rather than a fourth kind. The store and probe side is small, about six Rust sites, because `plan probes` already skips any owner that is not `worker` (tomlctl/src/plan/probes.rs:179). The real cost is in the engine. `plan emit gate-list` silently drops an unknown owner (tomlctl/src/plan/emit.rs:213). The `verification` agent cannot judge (claude/agents/verification.md:3-5). The gate record has no judged outcome (tomlctl/src/plan/schema.rs:162). Drain step 7 therefore needs a new judge dispatch, needs-plan.

```toml sources
read_at = "52fe01b9a17c76c7068e241fa891e6cacd6fcbc6"
dirty = []

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 44
claim = "ACCEPT_KINDS is forward/falsifier/guard; RUN_BY_OWNERS (line 49) is worker/gate/covered"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 276
claim = "Accept struct; owner() at 297 and owner_problem() at 309; accept_from_toml 1176 / accept_to_toml 1200 write run_by only when set"

[[source]]
path = "tomlctl/src/tasks/add.rs"
line = 99
claim = "ACCEPT_FIELDS (7 keys) refuses unknown accept keys; kind validated at 593, run_by at 624 against the const arrays"

[[source]]
path = "tomlctl/src/plan/probes.rs"
line = 174
claim = "skip_reason skips any check whose owner is not worker, so a new owner is never probed"

[[source]]
path = "tomlctl/src/plan/probes.rs"
line = 874
claim = "discriminates counts any unprobed forward/falsifier as binding; verdict() at 887 maps an unknown kind to broken"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 201
claim = "gate_list matches owner: covered -> skipped, gate -> commands, `_ => continue` at 213 drops any other owner from both lists"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 617
claim = "plan check raises plan/accept-owner-invalid from owner_problem and plan/probe-absent only for probeable rows"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 733
claim = "worker view prints owner string generically; covered gets a special label"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 162
claim = "CommandOutcome vocabulary is pass/flaky/fail/timeout; GateCommand (462) carries exit, summary, failed_ids, log only"

[[source]]
path = "claude/agents/verification.md"
line = 3
claim = "verification is Haiku, Bash/Read/Grep, runs commands and reports exit-code outcomes with no interpretation"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 298
claim = "step 7 builds the gate from gate-list commands only and dispatches verification once; a failing accept check mints a fix task"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 374
claim = "a minted fix task's accept holds the narrowest command reproducing the failure, which a judged look has no command for"

[[source]]
path = "tomlctl/src/cli/types/mod.rs"
line = 208
claim = "capabilities feature list, held to an exact count by tomlctl/tests/capabilities.rs (expected list at 1856, count assert after)"

[[source]]
path = "claude/commands/implement-v2.md"
line = 29
claim = "carrier pre-flight names every required capabilities feature, so a new feature joins this list"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
line = 16
claim = "any new plan/* check class must appear in claude/skills/tomlctl/references/plan.md's table"

[[source]]
path = "glimpse/src/view/task_spec.rs"
line = 176
claim = "glimpse prints the owner string it is given and does not branch on it, so it needs no change"
```

## Answer

Two ways in, with different costs:

- **A `look` owner (`run_by = "look"`) is the cheaper route.** `kind` keeps its forward/falsifier/guard meaning: a forward look fails before the work. Probes already skip it (probes.rs:179). `owner()` and the views print it as given (view.rs:733, glimpse). The new parts are a `rubric` field (and a capture-output field if the check's stdout is not the capture path) plus the engine's judge step.
- **A `look` kind costs more and breaks existing logic.** It needs `ACCEPT_KINDS` and `verdict()` changes, because an unknown kind reads `broken` (probes.rs:932-935). `discriminates` (probes.rs:874) would need a decision on whether a look entry beside a guard clears `guard-alone`. It also loses the polarity the probes depend on. One caveat applies to either route: a forward look entry left unprobed already counts as binding under `discriminates`, so a guard next to a look can pass `guard-alone` with no exit-code discriminator. Under the owner route that rule needs a line.

Cost per surface, owner route:

1. **schema.rs (small).** Add `RUN_BY_LOOK` to `RUN_BY_OWNERS` (line 49). Add a `rubric: String` to `Accept` (276), round-tripped in `accept_from_toml` and `accept_to_toml` (1176, 1200) and written only when set, as `run_by` is. Add `owner_problem` rules: look needs a rubric, a rubric needs look, and look implies the check writes the build. Add the schema unit tests. `add.rs` needs `"rubric"` in `ACCEPT_FIELDS` (99), and the field in tasks-write.md and tasks-store.md.
2. **plan probes (about zero).** `skip_reason` already returns "run_by is not worker". `plan check`'s `probe-absent` warning keys on `skip_reason`, so it stays quiet. A milestone-level `gate_probes` entry is how a plan proves the capture environment (browser binary, dev server) exists. It needs no code.
3. **plan emit gate-list (small code, contract change).** Today a look entry falls through `_ => continue` (emit.rs:213) and appears in neither `commands` nor `skipped`. It would be silently never judged. Emit a separate `looks: [{task, accept, capture, rubric}]` array rather than mixing them into `commands`, because the verification prompt is `commands` byte for byte (drain.md:311). Add emit tests (tests/plan_emit.rs) and update the drain.md:301 prose.
4. **implement-v2-engine gate and drain (largest, needs-plan).** The capture command can run in the `verification` batch as an ordinary command, so a failed capture is a normal `fail`. Judging has to be a new dispatch after a passing batch. That needs an Opus agent with `Read` on PNGs; `review-verifier` or a new agent fits. Then decide how a judged verdict is recorded. `CommandOutcome` is pass/flaky/fail/timeout and `GateCommand` has no verdict or reason field, so this needs either a new `[[look]]` record under `plan run gate` or an outcome-plus-summary convention. Decide how a look failure mints a fix task, because drain.md:374 expects a reproducing command and a look has none: the fix row's accept would itself be a look entry. Decide whether a judged fail costs a round like an exit-code fail. Decide how the `agents audit` allow rule (drain.md:108-110) treats a worker running the capture command. The narrower-than-an-accept-check clause covers it only if the capture sits in `check`.
5. **verification agent (no change, if it stays unchanged).** It is Haiku with an explicit "no interpretation" contract (verification.md:3). Giving it judgement would break the `lite`/`deep` tuning rule and its contract. It keeps running the capture as a command.
6. **cli::dispatch::tests gates (small, mandatory).**
   - A new capability feature (e.g. `accept_look`) goes into `cli/types/mod.rs` and the exact-count list in tests/capabilities.rs:1856, and into the pre-flight lists in commands/implement-v2.md:29 and apply-v2.md:31.
   - Any new `plan/accept-look-*` check class goes into the plan.md finding-class table, or `finding_classes` fails.
   - Every edited `claude/**/*.md` re-runs the carrier and skill lint subset.
   - Only a new leaf verb (e.g. `plan run look`) needs an `output_options.rs` case.

## Findings

- **Library/API or file**: tomlctl/src/plan/emit.rs:213 `gate_list`
- **Source**: tomlctl/src/plan/emit.rs:201-214, read at 52fe01b9
- **Evidence-grade**: high
- **Finding**: A new accept owner disappears from the gate list with no trace. The judge step needs its own `looks` output from `gate_list`. Do not rely on the `_` arm.
- **Details**: The `match accept.owner()` handles `RUN_BY_COVERED` (skipped) and `RUN_BY_GATE` (commands), and `_ => continue` covers both `worker` and any future owner. Drain step 7 runs exactly `commands` and the closing report names only `skipped` entries, so a look check would be neither judged nor reported.
- **Counter**: The finding fails if another verb lists unrun gate-owned accept checks to the drain. None was found: `owner()` is used only by emit.rs, probes.rs, view.rs and snapshot.rs.
- **Impact on plan / report**: The gate-list change and the drain judge step must land together.

- **Library/API or file**: claude/agents/verification.md:3 and tomlctl/src/plan/schema.rs:162 `CommandOutcome`
- **Source**: frontmatter `model: haiku`, `tools: Bash, Read, Grep`, description "no interpretation"; the `CommandOutcome` vocabulary is pass/flaky/fail/timeout
- **Evidence-grade**: high
- **Finding**: The judgement needs a second agent and a record shape, not a verification option. needs-plan.
- **Details**: The gate record (`plan run gate`, GateCommand at schema.rs:462) stores an exit code, a summary, failed_ids and a log, and `round_outcome` (run_gate.rs:177) derives pass or fail from those outcomes alone. A judged result with a rubric reason has no field to go in. Without a new record, a look fail can only be squeezed into `fail` plus `summary`.
- **Counter**: The finding fails if the plan accepts a look verdict recorded as `fail` with the judge's reason in `summary` and the capture path in `log`. That needs no schema change, but loses which rubric line failed.
- **Impact on plan / report**: The plan has to choose between a new `plan run gate` record and the existing fields before the drain prose is written.

- **Library/API or file**: tomlctl/src/plan/probes.rs:174 `skip_reason` / :874 `discriminates`
- **Source**: probes.rs:179 `accept.owner() != RUN_BY_WORKER` gives "run_by is not worker"; :875-880 treat an unprobed forward/falsifier as binding
- **Evidence-grade**: high
- **Finding**: The owner route costs probes nothing. A look entry that is forward or falsifier would, however, count as discriminating for `guard-alone`. Decide whether it should.
- **Details**: A kind route would instead hit `verdict()`'s `_ =>` broken arm (probes.rs:932) for any probed row, and would need `ACCEPT_KINDS` and add.rs:593 changes.
- **Counter**: The `guard-alone` concern is void if look entries are never placed beside guards. Nothing in the schema enforces that.
- **Impact on plan / report**: This favours `run_by = "look"` over a new kind.

- **Library/API or file**: tomlctl/src/cli/types/mod.rs:208 and tomlctl/tests/capabilities.rs:1856
- **Source**: the capabilities test asserts every expected name and an exact `features.len()`
- **Evidence-grade**: high
- **Finding**: Gating a new feature means one edit each in types/mod.rs, capabilities.rs, implement-v2.md:29 and apply-v2.md:31. A new check class also goes into the references/plan.md table (finding_classes test).
- **Details**: `accept_run_by` is the precedent: it was added to the same four places when the owners landed.
- **Counter**: The finding fails if the plan reuses `accept_run_by` and gates no new feature. Then an old binary would refuse `run_by = "look"` at `tasks add-many` (add.rs:624) instead of at pre-flight.
- **Impact on plan / report**: These are small, mechanical edits. Put them in the same task as the schema change.

Coverage: I read tomlctl/src/tasks/schema.rs (Accept, owner, the TOML round-trip), tasks/add.rs `accept_entry`, plan/probes.rs (skip_reason, discriminates, verdict), plan/emit.rs `gate_list`, plan/check.rs (the accept checks), plan/view.rs `run_by`, plan/schema.rs (CommandOutcome, GateCommand), run_gate.rs (record merge), drain.md step 7 and its fix-task rules, verification.md (input and procedure head), cli/types features, capabilities.rs, finding_classes.rs (header), and glimpse's owner display. I swept `run_by`, `owner()`, `ACCEPT_KINDS`, `RUN_BY_*` and `accept_run_by` across tomlctl/src, tomlctl/tests, claude/ and glimpse/src. Ruled out: glimpse needs no change, because it shows the owner string and does not branch on it. command_lint flag drift is untouched, because accept fields are JSON keys, not CLI flags. The plan-worker agents need no change, because a worker runs only worker-owned checks. I ran nothing that builds. Out of lens: the capture contract and critic reliability belong to the sibling lenses.
