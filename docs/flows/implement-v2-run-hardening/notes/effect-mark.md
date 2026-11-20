Verdict: Smallest carrier is an optional `effect = "external"` key on `Accept` (tomlctl/src/tasks/schema.rs:285) and a sibling `external_gates` string list on `MilestoneSpec` (tomlctl/src/tasks/schema.rs:399), mirroring how `gate_probes` names gates by command string; `gate_list` (tomlctl/src/plan/emit.rs:172) routes marked commands to a `held` array printed only when non-empty, a new `plan check` class walks each row's `needs` to a same-milestone row holding a gate-owned external accept, and the store stamp must rise to schema 4 because an older binary silently drops unknown keys (tomlctl/src/tasks/schema.rs:17, :730).

```toml sources
read_at = "1502e6b420be81caac7bc44ae0e369dc20e461c9"
dirty = ["docs/flows/implement-v2-run-hardening/brief.md"]

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 285
claim = "Accept struct: behaviour, check, kind, positive, writes_build, run_by, covered_by, states, probe"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 321
claim = "owner_problem is the one validator of run_by/covered_by/states combinations, called by the parser and plan check"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 399
claim = "MilestoneSpec: gates is Vec<String>; gate_probes reference gates by command string"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 1227
claim = "accept_from_toml / accept_to_toml (1252): optional keys written only when set"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 1369
claim = "milestone_from_toml / milestone_to_toml (1408): gate_probes, after, ui_* written only when non-empty"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 17
claim = "unknown keys are dropped; store re-emitted from the struct"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 730
claim = "from_toml refuses only a schema_version newer than SCHEMA_VERSION (3)"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 537
claim = "holds_v3_keys decides the v3 stamp (used at line 872)"

[[source]]
path = "tomlctl/src/tasks/add.rs"
line = 99
claim = "ACCEPT_FIELDS whitelist; accept_entry (581, literal at 619) parses JSON accept entries and calls owner_problem"

[[source]]
path = "tomlctl/src/cli/types/plan.rs"
line = 500
claim = "milestone add --gate / --gate-probe flags; update flags at 558-572"

[[source]]
path = "tomlctl/src/plan/milestone.rs"
line = 58
claim = "parse_gate_probes / gate_probes_for validate a probe names a gate; add (177) and patch_milestone (333) store them"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 172
claim = "gate_list: gates, then gate-owned accept checks, covered skipped, generated checks; prints {commands, skipped[, visual]}"

[[source]]
path = "tomlctl/tests/plan_emit.rs"
line = 719
claim = "test asserts gate-list keys are exactly [commands, skipped]: 'no option or hold is printed'"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 152
claim = "findings(): where plan check classes are assembled"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 636
claim = "plan/accept-cover-unknown checks covered_by against the row's milestone gates"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 767
claim = "plan/gate-probe-unknown: precedent for a milestone list that must name gates"

[[source]]
path = "tomlctl/src/plan/probes.rs"
line = 174
claim = "skip_reason: plan probes run only worker-owned writes_build=false checks"

[[source]]
path = "tomlctl/src/tasks/show.rs"
line = 145
claim = "accept_json projection (also snapshot.rs:302, plan/store.rs:262 entry_matches, plan/view.rs:805)"

[[source]]
path = "tomlctl/src/cli/types/mod.rs"
line = 208
claim = "capability list the /implement-v2 pre-flight checks (accept_run_by, gate_probes, plan_emit)"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 332
claim = "today the verb 'holds nothing back'; holding external commands is by the orchestrator's eye (334)"

[[source]]
path = "claude/skills/tomlctl/references/plan-engine.md"
line = 159
claim = "launch-args drops a row with need-not-committed: a cross-milestone need is gated and committed first"
```

## Answer

**Where things live today.**
- Accept entry: struct `Accept` (`tasks/schema.rs:285`). It is read from TOML at `accept_from_toml` (`:1227`) and written by `accept_to_toml` (`:1252`), which writes an optional key only when it is set. It is parsed from authoring JSON at `tasks/add.rs:581` `accept_entry`, which uses the `ACCEPT_FIELDS` whitelist at `:99` and validates through `Accept::owner_problem` (`schema.rs:321`). `plan check` reuses `owner_problem` (`plan/check.rs:629`).
- `covered_by`: stored on the accept entry, and allowed only with `run_by = "covered"` (`schema.rs:353`). Its gate is checked by `plan/accept-cover-unknown` (`check.rs:636`).
- Milestone gates: `MilestoneSpec.gates: Vec<String>` (`schema.rs:401`). They are read at `milestone_from_toml` (`:1395`) and written at `:1421`. CLI entry is `--gate` / `--gate-probe` (`cli/types/plan.rs:500`, update `:558`). A gate probe is split on ` :: ` and must name an existing gate (`plan/milestone.rs:58`, `:84`). Probes are stored in a sibling list `gate_probes` keyed by the gate's command string (`schema.rs:404`), and the `plan/gate-probe-unknown` class (`check.rs:767`) guards that list.
- Emit: `gate_list` (`plan/emit.rs:172`) builds `commands` in this order: gates, then gate-owned accept checks, then generator checks. A `covered` check goes into `skipped`. The output carries no hold signal, by design (`emit.rs:168`, `drain.md:332`).

**Smallest carrier.**
1. **Accept.** Add `effect: String` (`""` or `"external"`) to `Accept`, wired in the same way as `covered_by`: in `ACCEPT_FIELDS`, `accept_entry` and `accept_from_toml`, and written only when set by `accept_to_toml`, `show.rs:145 accept_json`, `snapshot.rs:302` and `plan/store.rs:262 entry_matches`. Add an `owner_problem` arm that allows `external` only when the owner is `gate`. That one rule does four things:
   - It keeps `plan probes` from ever running a deploy at Prove, since `skip_reason` (`probes.rs:174`) runs only worker-owned checks.
   - It keeps workers off external commands.
   - It rejects a mark on `look` entries.
   - It refuses `covered`, whose effect belongs to the covering gate's own mark.
2. **Milestone.** Add `external_gates: Vec<String>` to `MilestoneSpec`. Every entry must be one of `gates`, which is the `gate_probes` precedent. It is written only when non-empty and set by a repeatable `--external-gate '<cmd>'` on `milestone add/update`. That flag adds the command to `gates` and marks it, so authors write each command once. On update, `--gate`, `--external-gate` and `--clear-gate` replace both lists together, and any marks the new gate list no longer holds are revalidated. Add a check class `plan/external-gate-unknown` alongside `gate-probe-unknown` for a hand-edited store. The alternative, a gate entry that may be a string or a table, would retype `gates` across every reader (`emit.rs`, `check.rs`, `milestone.rs`, `probes.rs`, `lane.rs`, the snapshot and the view). The sibling list touches only the milestone add/patch path and the gate list.
3. **gate-list.** While building `commands`, move a gate that appears in `external_gates`, and an accept check whose `effect` is `external`, into a `held: [{command, source}]` array, keeping the same `source` values and de-duplicating with the same `listed` set. Print `held` only when non-empty, in the same way as `visual`. The existing key-set test (`tests/plan_emit.rs:719`) then stands for unmarked milestones. Update the docs at `drain.md:318/332/334` and `plan-engine.md` ("Holds … stay the orchestrator's") to say that held commands come from `held`. `drain.md:108` should pass `--gate` for `held` commands as well as `commands` to `agents audit`.
4. **plan check.** Add a new class (suggested name `plan/needs-gate-effect`) to `findings()` (`check.rs:152`). For each row R, walk `needs` transitively with a visited set. A plain DFS is enough, because `dag/` classes already report cycles. Flag R when a reached row E ≠ R has the **same `checkpoint`** as R and holds an accept with `effect = "external"` whose `owner()` is `gate`. Rows in another milestone are left out: `launch-args` drops a row whose need is not committed (`plan-engine.md:159`), and a milestone commits only after its gate, held commands included, so the effect exists by then. That matches the B-0f9fc8b5 case, where tasks 33 and 34 were both in milestone C. Make it a **warning**: a `needs` edge often exists for file order rather than for the effect, so an error would block plans that are sound.
5. **Version and capability.** Bump `SCHEMA_VERSION` to 4 and add `holds_v4_keys` (any `effect` or `external_gates`) to the stamp at `schema.rs:872`. Add a capability such as `effect_mark` to `cli/types/mod.rs:208` and to the `/implement-v2` pre-flight.

## Findings

- **Library/API or file**: tomlctl/src/tasks/schema.rs:17 and :730 `from_toml`
- **Source**: tomlctl/src/tasks/schema.rs:17 ("Unknown keys are dropped"), :730 (refuses only `schema_version > SCHEMA_VERSION`)
- **Evidence-grade**: high
- **Finding**: A new `effect` or `external_gates` key needs a schema bump to 4 with `holds_v4_keys`. Without it, an installed older binary reads a v3-stamped store, drops the marks and rewrites the store silently. `needs-plan` (version constant, stamp predicate, capability and pre-flight move together).
- **Details**: `holds_v3_keys` (`:537`) decides the current stamp at `:872`. The v3 precedent (`ui_states`, `look`) bumped for exactly this reason (`:27-30`).
- **Counter**: The marks never need to survive a write by an older binary, for example if every writer is guaranteed to be at least the new version.
- **Impact on plan / report**: The schema task must own `schema.rs` version code, the capability list and the carrier pre-flight.

- **Library/API or file**: tomlctl/src/plan/emit.rs:172 `gate_list`; tomlctl/tests/plan_emit.rs:719
- **Source**: emit.rs:168-171 doc comment; plan_emit.rs:719-723 asserts keys `["commands","skipped"]`, "no option or hold is printed"
- **Evidence-grade**: high
- **Finding**: Holding is currently a documented deliberate non-feature. Add `held` as a key printed only when non-empty, so the existing test still holds, and rewrite the emit.rs doc comment, drain.md:318/332/334 and plan-engine.md's gate-list section.
- **Details**: The `listed` BTreeSet de-duplicates across sources. A held command identical to an earlier unheld one should stay held, so check the mark before the duplicate test. Generator checks (`emit.rs:241`) carry no mark.
- **Counter**: The planner wants `held: true` on each `commands` row instead. That breaks no key-set test, but every consumer would have to filter.
- **Impact on plan / report**: The emit task touches emit.rs, tests/plan_emit.rs and two docs. The drain doc also needs `held` fed to `agents audit --gate` (drain.md:108).

- **Library/API or file**: tomlctl/src/tasks/schema.rs:321 `Accept::owner_problem`; tomlctl/src/plan/probes.rs:174 `skip_reason`
- **Source**: file reads at those lines
- **Evidence-grade**: high
- **Finding**: Restrict `effect = "external"` to gate-owned entries inside `owner_problem`. One rule then covers the parser (`add.rs:630`) and `plan check` (`check.rs:629`), and keeps `plan probes` from running a deploy during Prove.
- **Details**: Probes run exactly the worker-owned checks with `writes_build = false`. An external mark on such an entry would be executed at authoring time. Refusing the `worker`, `look` and `covered` owners rules that out.
- **Counter**: The planner decides a worker may run an external effect on purpose (B-0f9fc8b5's replan made task 34 deploy itself). The rule would then need a probe skip in `skip_reason` instead.
- **Impact on plan / report**: Decide the allowed owner set as an explicit plan decision.

- **Library/API or file**: tomlctl/src/plan/milestone.rs:84 `gate_probes_for`; tomlctl/src/plan/check.rs:767 `plan/gate-probe-unknown`
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: Mark gates through a sibling `external_gates` list validated against `gates`, rather than retyping `gates: Vec<String>`.
- **Details**: `gate_probes` already references gates by exact command string, with add-time validation (`gate_probes_for`) and a check class for drift. `patch_milestone` (`milestone.rs:333`) replaces `gates` whole on `--gate`, so the mark list must be revalidated or replaced there. `MilestoneSpec` literals: `milestone.rs:208,368`, `schema.rs:1393`, plus test literals at `lane.rs:745`, `milestones.rs:294`, `schema.rs:2157`.
- **Counter**: Authors would rather write the mark inline per gate. A string-or-table `gates` entry would then be the shape, at the cost of every gate reader.
- **Impact on plan / report**: The milestone task files are cli/types/plan.rs, plan/milestone.rs, tasks/schema.rs, plan/check.rs, and the tomlctl `references/plan.md` flag tables.

- **Library/API or file**: tomlctl/src/plan/check.rs:152 `findings`; claude/skills/tomlctl/references/plan-engine.md:159
- **Source**: file reads; launch-args `need-not-committed` drop
- **Evidence-grade**: medium (the cross-milestone exemption is inferred from launch-args' committed-need rule and the gate-then-commit order in drain.md)
- **Finding**: The new check only needs to reach same-milestone rows through transitive `needs`. Set severity to warning.
- **Details**: `TaskRow.needs: Vec<u32>` (`schema.rs:457`). `Graph::closure_up` (`tasks/graph.rs:260`) exists, but it builds a capped graph. A visited-set DFS over `needs` inside check.rs is enough, and `milestone_width_findings` (`check.rs:1023`) walks `needs` the same way. A task that relies on a milestone-level external gate (not an accept) cannot be reached through `needs`, so this check cannot see it.
- **Counter**: A milestone's held commands run after its commit rather than before. Cross-milestone needs would then have to be flagged too.
- **Impact on plan / report**: One check.rs function plus a `plan/…` row in `references/plan.md`'s class table (around line 565).

Coverage: Read in full the relevant spans of tasks/schema.rs (Accept, MilestoneSpec, TOML conversions, version stamp), plan/emit.rs gate_list, plan/check.rs (findings, row_spec_findings, milestone_findings), tasks/add.rs accept_entry, plan/probes.rs skip_reason, plan/milestone.rs signatures, tests/plan_emit.rs gate-list test, drain.md step 7, plan-engine.md gate-list, and the brief. Ran rg sweeps for `Accept {` and `MilestoneSpec {` literal sites, and for gate-list consumers across src, tests and claude. Ruled out: retyping `gates` to tables (wider than the sibling list); a `held: true` flag per command (breaks no test, but leaves filtering to consumers; noted as a Counter); `plan run gate` (run_gate.rs keys commands by string, no change needed). Out of lens: `agents audit` matching of indirect script runs is a sibling lens (B-dcecc52a).
