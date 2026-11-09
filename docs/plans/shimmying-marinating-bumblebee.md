# Plan: v2 plan review over a flow directory (v2 planning, Slice 3)

**Plan path**: `docs/plans/shimmying-marinating-bumblebee.md`
**Created**: 2026-11-09

## Summary

This plan lets a v2 plan be reviewed. Plan-family findings on a v2 flow name a record (`task:<ref>.spec.accept[1]`, `decision:D3.why`, `brief#Background`) with an expected and a new value, in place of a markdown heading and anchor text. tomlctl computes each finding's evidence tier, and a new `review apply` verb writes the edits each tier allows, with compare-and-set. A `plan-review-lead` agent inside a new `plan-v2-review` Workflow composes, settles and audits the run around `review-v2-pipeline`, so `/plan-v2` gets back a digest and applies edits by tier. A second, narrower run re-checks what the edits changed. `plan gate` and `plan approve` read the review, and `review stats` reports per-plan outcomes and per-model overturns (`B-13ec633a`). The run also installs tomlctl and links every v2 review artefact into the Claude config dir. Scrutinise Approach, "Tiers" and "review apply" first, since they decide what changes a plan without asking you, then "The wrapper and the lead", the untried piece. The plan touches 71 files in 36 tasks (`grep -oE '^#### [0-9]+' docs/plans/shimmying-marinating-bumblebee.md | wc -l` counts the tasks), against the 40 you accepted; three tasks (11, 12 and 19) exceed 3 files because their edits are inseparable. Lenses stay unchanged and read records through one mapping reference (Approach, "Reviewers on a v2 flow").

## Context

`docs/plans/v2-planning.md` split v2 planning into four slices. Slices 1 and 2 built the plan store, the verbs, and `/plan-v2`. `/plan-v2` records plan review as skipped, because `/review-plan-v2` reviews one markdown file: on a v2 flow it binds to `brief.md`, its import refuses, and its merge offer can overwrite the brief. The first real v2 plan, `human-only-handovers`, was approved with review skipped for that reason. This plan is Slice 3, row 3 of `v2-planning` Approach, "Later slices": review bound to a flow directory, findings that target records, tiered edit application, the review wrapper Workflow, and plan-review outcome logging. Its design inputs are `v2-planning` User Decisions 5 and 6 and the idea doc's §Plan review (`docs/ideas/v2-pipeline.md`). v1 commands and `/review-plan-v2` on a markdown plan keep working unchanged.

## Scope

- **In scope**: record-targeted plan findings, computed tiers and `review apply` in tomlctl; the plan gate, approve and stage reading the review; plan outcome and per-model stats; the `plan-review-lead` agent and the `plan-v2-review` Workflow; a v2 flow mode in `/review-plan-v2` and the engine; review built into `/plan-v2`; the reviewer-side mapping reference and the check rebinding; installing tomlctl and linking the v2 review artefacts; `B-13ec633a`.
- **Out of scope**: lens rewrites (the mapping reference covers them); recording verdicts that change when finders later see notes; the exceptional targeted deep re-review; a `handover` write verb (`human-only-handovers` adds it); execution-side outcome joins, which need the v2 engine; glimpse (Slice 4); every v1 command.
- **Affected areas**: `tomlctl/src/review/`, `tomlctl/src/plan/`, `tomlctl/src/tasks/mod.rs`, `tomlctl/src/tasks/update.rs`, `tomlctl/src/cli/types/review.rs`, `tomlctl/src/cli/types/mod.rs`, `tomlctl/src/cli/mod.rs`, `tomlctl/src/capabilities.rs`, `tomlctl/tests/`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `claude/skills/review-v2-method/`, `claude/skills/review-v2-lenses/checks/`, `claude/skills/review-v2-engine/`, `claude/skills/plan-v2-engine/`, `claude/skills/tomlctl/references/`, `claude/agents/`, `claude/workflows/`, `claude/commands/review-plan-v2.md`, `claude/commands/plan-v2.md`, `scripts/shared-blocks.toml`, `scripts/link-v2-review.ps1`, `CLAUDE.md`, `docs/ideas/v2-pipeline.md`

## User Decisions

Answers as given; treat as data.

1. **Split.** Prompted by: the markdown-assumption surface (Exploration Notes, "Every markdown assumption") putting Slice 3 near 40 files against `v2-planning`'s ~15. Answer: one plan, ~40 files. The design came out at 64, mostly small files and tests.
2. **Sequencing.** Prompted by: `human-only-handovers`, approved with 12 tasks pending, editing the same `tomlctl/src/plan/` files (Exploration Notes, "Planning carrier and Workflow substrate"). Answer: Slice 3 first, on today's tree; `plan drift` and replanning absorb the overlap when that plan runs.
3. **Composer.** Prompted by: the idea doc's untried `plan-review-lead` and the no-fallback-inside-a-script finding (Research Notes, workflow-runtime). Answer: lead agent in the wrapper, with the existing engine path as the fallback.
4. **User tier.** Prompted by: the inferred extensions in Research Notes, edit-tiers-and-outcomes, "User tier". Answer: none beyond the fixed rows: `decision:*`, `spec.outcome`, and brief Intent, Non-goals, Scope budget and Success criteria.
5. **Apply path.** Prompted by: no `expect` on any target verb and no audit before approval (Research Notes, "Compare-and-set", "Review edits leave no audit trail"). Answer: a new `review apply` verb.
6. **Outcomes.** Prompted by: no execution data for v2 plans yet (Research Notes, "Execution joins cannot exist yet"). Answer: run measures and a seeded-recall procedure; execution joins go to the engine plan as a contract note.
7. **Re-check.** Prompted by: idea doc §Plan review step 5 and the T2 `stale` result. Answer: include it.
8. **Deploy.** Prompted by: no review-v2 artefact linked under `~/.claude` (Exploration Notes, "Planning carrier and Workflow substrate"). Answer: in the run, by a link script.
9. **Backlog fold-in.** Exploration Notes `**Backlog**`. Answer: `B-13ec633a`.

Resolved without asking: checkpoint cadence is milestones (standing preference, as in `v2-planning`); `/review-plan-v2` keeps its markdown mode for v1 plans, since v1 coexists.

### Phase 5 outcome

Skipped. Answers 1 to 8 name nothing the Research Notes do not cover. Answer 9's per-model read needs no outside source: each run records `models` (`tomlctl/src/review/fields.rs` `field(Run, "models"`), and `tomlctl/src/review/stats.rs` `VERIFYING` names the verdict roles an audit overturns (`verifier`, `panel`, `checker`), so overturns group by the model of the verifying role.

## Approach

### Sequence

1. Before `/implement`: commit the uncommitted Slice 2 review-apply edits (`git status` at planning time). This plan's tasks assume them, notably the `PLAN_REVIEW` hint in `tomlctl/src/plan/stage.rs` and the `--premises` probe flag; both are committed (`7429069`, `091d23f`), so this step is already satisfied.
2. `/implement` runs milestones A to D in order of their checkpoints. Task 33 installs tomlctl and runs the link script once the tomlctl work is in, so the restart in After Merge loads everything.

### Two modes, one carrier

A plan-family run is in **flow mode** when its store's flow has `.claude/flows/<slug>/plan.toml`, and in **markdown mode** otherwise. tomlctl decides this from the store's slug; the carrier decides it from the resolved flow. Markdown mode is today's behaviour, byte for byte. Flow mode changes four things:

- the run's scope is `docs/flows/<slug>/brief.md` plus every `docs/flows/<slug>/design/*.md`, and no task-store import runs, because the store is already canonical and `import-plan` refuses a v2 flow;
- findings target records;
- coverage is composed, settled and audited by the wrapper Workflow, with the existing engine path as the fallback;
- the merge offer is replaced by tiered apply.

### Record targets

A defect or cleanup row in flow mode carries `target`. `expect` and `new` are optional, but when `new` is present `expect` must be too, except on a `[+]` append, the one row where `new` stands alone. The review store is TOML, which holds no null and drops an empty value at mint (`items::add_value_to`, `convert::json_to_toml`), so `expect`, `new` and `applied.old`/`applied.new` are stored as JSON text and decoded on read. `plan_section`, `anchor_old` and `anchor_new` are refused in flow mode, and `line` becomes optional there. In markdown mode `plan_section` and `line` stay required for defect and cleanup. That requiredness moves from the field catalogue into `review add`, so `RowContext` is unchanged. Advisory rows may carry `target` and never carry `expect` or `new`.

| Target | Record | `file` / `symbol` | `expect` / `new` |
|---|---|---|---|
| `task:<ref>.<field>`, field one of `action`, `detail`, `acceptance`, `effort`, `files`, `needs`, `coupling`, `deps_note`, `checkpoint` | task row | `.claude/flows/<slug>/tasks.toml` / `<ref>` | the field's whole value |
| `task:<ref>.spec.<field>`, field one of `outcome`, `constraints`, `accept`, `premises`, `means`, `design`, `precedent`, `executor` | `[[specs]]` | same | the whole value |
| `task:<ref>.spec.<list>[<n>]` | one list entry | same | the whole entry. `[n]` is a hint: the match is the one entry equal to `expect`, so 0 or 2+ matches are refused. A `new` of JSON null removes it; `[+]` with a `new` and no `expect` appends |
| `decision:D<n>.<key>`, `contract:C<n>.<key>` | `plan.toml` record (`DECISION_KEYS` / `CONTRACT_KEYS` minus `id`) | `.claude/flows/<slug>/plan.toml` / `D<n>`, `C<n>` | the key's value |
| `milestone:<id>.<field>`, field one of `goal`, `gates`, `elaboration`, `defer_reason` | `[[milestones]]` | `tasks.toml` / `milestone:<id>` | the field's value |
| `handover:H<n>.<field>` | `[[handover]]` | `plan.toml` / `H<n>` | validated, never applied: no write verb exists yet |
| `brief#<Section>`, one of the six brief sections | `brief.md` | `docs/flows/<slug>/brief.md` / the section | text occurring exactly once in the section, and its replacement |
| `design/<file>.md#<anchor>` | design prose | `docs/flows/<slug>/design/<file>.md` / the anchor | same as brief |

The part before `[` is the `[[changes]].field` name, so a later revert joins on (ref, field). `review add` refuses a target that does not parse or resolve, a `file` or `symbol` other than the target's, and an `expect` that does not equal the current value. Brief sections resolve through `add.rs` `section_bodies`; design anchors through `plan/check.rs` `design_section`.

`title` is not a target: a retitle that changes the derived ref is refused by the task update path (`retitle_err`) unless the row is re-keyed, which would strand every other finding's `task:<ref>` target, so title defects stay advisory. A stored `spec.accept` entry carries a tool-written `probe` table that `new` may not supply (`tasks/add.rs` `accept_entry`), so resolution, entry matching and the "already equal" test all compare accept entries with `probe` removed on both sides, as `tasks/update.rs` `keep_probes` does, and neither `expect` nor `new` may carry it.

### Tiers

`tier` is computed. It is stamped once, when a flow-mode finding first becomes `open` (`verdict.rs` settle, `resolve.rs`), and frozen after that. In order:

1. **user**: the target is any `decision:*`, any `task:*.spec.outcome`, or `brief#Intent`, `brief#Non-goals`, `brief#Scope budget` or `brief#Success criteria`. This applies whatever the evidence (User Decision 4).
2. **advisory**: kind `advisory`; a `handover:*` target; no `new`; or nothing below holds.
3. **mechanical**: an input `proof` that tomlctl verifies from the store and the tree, without running a command. The kinds are:
   - `probe`: the target is a `spec.accept` entry whose stored probe verdict is not `healthy`.
   - `check-class` (`class`): `plan check` reports that error class on the target's task or record.
   - `path-missing` (`path`): the target is `task:<ref>.files`, the path is absent from the tree, and it is not a `(new)` file note.
   - `ref-unknown` (`ref`): the target is `needs` or `contract:*.tasks`, and the value names no row.

   A missing probe verdict is no proof. `plan check` findings carry only task ids, so `check-class` proves only a `task:*` target. `review add` refuses a `proof` that does not hold, and a mechanical `new` that changes more than its proof covers: a `probe` fix may change only the entry's `check` and `positive`; a `path-missing` fix may only replace or drop the missing path; a `check-class` proof binds only the fields a class-to-field table in `tier.rs` names. `expect` and `new` are capped at 800 serialised characters each. A row outside these shapes caps at cited.
4. **cited**: the latest verifying verdict is CONFIRMED or PLAUSIBLE, and its `citation` is a `path:line` (`query.rs` `parse_citation`, which only parses) naming the target's own file or a path the target names, whose line range `tier.rs` checks against the file.

Verifier agreement alone stays advisory (User Decision 6). A mechanical fix to an `accept` entry drops that entry's probe (`tasks/update.rs` `keep_probes`), so `plan stage` returns to `prove`. `plan approve` gates only on error-class findings and `plan/probe-absent` is a warning, so the re-probe is enforced twice: `plan-flow.md` re-probes every edited entry after apply, and `plan approve --review ran` refuses while a `spec.accept` entry an applied finding changed has no probe.

### review apply

`tomlctl review apply <ids> --slug <slug> [--user-accepted] [--dry-run]` applies open, flow-mode defect and cleanup rows that carry `new`.

- **Refusals.** Before any write, it refuses advisory tier, a flow-mode row with no `tier`, a user-tier row without `--user-accepted`, a `handover:*` target, any status other than `open`, and any row without `--user-accepted` while `plan.toml` holds an `[[approval]]`. It also parses every `new` through its target store's validator and re-verifies every mechanical proof, so nothing the target would reject is discovered mid-batch. These are a validation error, and nothing is written.
- **Writes.** Each target is written through its own store's write path, with `expect` re-checked inside that store's lock:
  - `tasks.toml` rows and specs go through the task update path with actor `review` and reason `review <run> <id>`. A `[[changes]]` row is written only when the store's `audit` is true.
  - Decisions and contracts go through the plan records path.
  - Milestones go through the milestone path.
  - Brief and design edits are an exactly-once replacement inside the section, under `crate::io::with_exclusive_lock` on the prose file and persisted with `crate::io::atomic_write`; `mutate_doc` cannot write them, since `guard_write_path` refuses paths outside `.claude/`. A design path resolves through `plan/check.rs` `design_path`, never re-joined by hand; a file that is not UTF-8 is refused; line endings are kept.
- **Order.** Each finding is stamped right after its own target write, never in a batch at the end, so a failure leaves at most one unstamped edit.
- **Stale and repeated edits.** A value already equal to `new` is `already`, with no write, and is still resolved with `applied` (`old` from `expect`, `recovered = true`), so an edit whose stamp a crash lost still reaches `review_edits`. An `[+]` append is `already` when an entry equal to `new` exists; a removal is `already` when no entry equals `expect`. Any other mismatch is reported under `skipped_stale` as `{id, target, expected, found}`, and the rest proceed.
- **The finding.** Each applied finding is resolved with the resolution `applied to <target>` and a computed `applied = {at, tier, target, old, new, by}`, where `by` is `tool`, or `user` under `--user-accepted`. The review store therefore holds every edit review made, including those made before approval, when no audit row exists.
- **Output.** `{ok, applied:[{id, target, tier}], already:[ids], skipped_stale:[…]}`.

### The wrapper and the lead

`claude/workflows/plan-v2-review.js` (`meta.name` `plan-v2-review`) follows the `plan-v2-investigate.js` pattern. Its args are `{slug, run, mode: "full"|"recheck", depth, run_dir, ledger_line, signals, targets, schemas}`, and it validates them before launching anything.

1. **`full` mode only, survey.** One `review-surveyor` runs, with the surveyor prompt from `agent-waves.md` carrying `flow: <slug>`. One `verification` agent runs per resolved baseline, in sequence.
2. **Compose.** `agent(…, {agentType:'plan-review-lead'})` runs phase `compose`. It runs `review plan --propose`, edits the map within the depth envelope, writes the cells and waivers, and returns `{wave, cells:[{id, agentType}]}`. In `recheck` mode it slices only the tasks and records named in `targets`, at `quick` depth, in one wave.
3. **Waves.** Each wave is `workflow('review-v2-pipeline', {...})` in a try/catch, followed by the lead's `settle` phase. `settle` runs `review resolve` and `review digest`, decides a follow-up wave within the envelope, and returns `{next, unverified}`. There is no follow-up in `recheck` mode.
4. **Audit.** The lead's `audit` phase audits the digest's sample through the `audit` projection, writes `audit` verdicts, and returns `{sampled, overturned, contested}`.
5. **Return.** `{slug, run, mode, waves, unverified, audit, errors, script_version}`.

`plan-review-lead` (`claude/agents/plan-review-lead.md`) acts for the orchestrator at engine stages 3, 5 and 6, under engine rules 1 to 4. It holds `Read, Grep, Glob, Bash, Write`, writes only into the run directory and through `tomlctl review`, and runs one phase per dispatch. Its model and effort live in its frontmatter.

The orchestrator mops up the returned `unverified` ids through `agent-waves.md`, since a script cannot. A launch that is refused, errors, or ends with no result falls back to the engine at the first stage the store shows incomplete (no surveyor digest: stage 2; no cells: stage 3; cells without findings: stage 4), never re-composing a run that already holds a wave.

### Re-check

Once edits are applied, the orchestrator starts a second run with `--set rechecks=<first run>` and launches the wrapper in `recheck` mode. Its `targets` are the applied findings' targets. The launch is fresh, never a `resumeFromRunId` relaunch (T2 `stale`). Re-check findings are never applied automatically. A user-tier one is asked before approval, and the rest show at the gate as leftovers.

### Gate and approve

`tomlctl/src/plan/store.rs` gains `plan_review_state`, which both `approve` and `stage` read, so the two cannot disagree. It reads the flow's `review-v2.toml` as a plain document.

- **`plan gate`.** `review_edits` lists every applied plan finding as `{id, target, tier, by, summary}`, decisions first, then user, cited and mechanical. `risks` keeps the skip reasons and adds each open critical or major defect, and each open re-check finding, as `{kind:"review-finding", id, severity, target, summary}`. `review_note` and `REVIEW_NOTE` go.
- **`plan approve --review ran`.** It also refuses while a plan-family run is unfinished, an open finding has tier `user`, or a `spec.accept` entry an applied finding changed has no probe.
- **`plan stage`.** Review is settled only under the same rule. The `next` hint names the `/plan-v2` Review step or `plan run skip --step review`, and a blocker line counts open user-tier findings.

### Reviewers on a v2 flow

A new reference, `claude/skills/review-v2-method/references/plan-records.md`, holds the reviewer side. Every agent loads it through the method skill's §Plan family. It covers:

- **Inputs.** The inputs are `tomlctl plan view --slug <slug> --for human` and `--for worker --task <n>`, plus `brief.md` and `design/*.md`. `plan view` omits stored accept fields, so an `expect` is copied from `tomlctl tasks show <id> --slug <slug> --with spec`. Agents never read `notes/`; this is prompt-only, as in `v2-planning` Approach, "Structure challenge".
- **Mapping.** A table maps each markdown concept a lens names to its record: **Files** to `task:<ref>.files`, **Depends on** to `needs`, **Acceptance** to `spec.accept`, `## Success Criteria` to `brief#Success criteria`, `## User Decisions` to decisions, `## Approach` to design files, decisions and contracts, `## Verification Commands` to milestone `gates`, and `## After Merge` to handovers. A lens check on a section with no record (Summary, the Status line, Dependency Graph layout) does not apply on a v2 flow and is counted as such in the coverage note.
- **Finding shape.** It gives the target grammar, `file`/`symbol`, `expect`/`new` and the `proof` kinds.

Lenses are not edited; `plan-records.md` states that on a v2 flow any lens or method instruction to supply `plan_section`, `anchor_old` or `anchor_new` (the plan lenses, `feasibility`, `ripple`, `test-gaps`, `methods/dry-run.md` step 5) is read as `target`, `expect` and `new`. Agents find `plan-records.md` through the method references `review show` carries (`query.rs` `METHOD_REFERENCES`), and each cell assignment names the flow when the run is in flow mode. Only checks whose procedure is markdown-specific gain a v2 branch:

- `store-check` runs `plan check` and `tasks check`;
- `format-contract` and `break-set-baseline` report not run;
- `two-control-probe` reads recorded probe verdicts and raises `proof {kind:"probe"}` findings, rather than running its helper;
- `falsifiability` and `acceptance-contradiction` read specs and contracts.

### Outcomes

`review stats --plan-outcomes` (with `--slug` or `--all`, plan-family runs only) reports per run and in total:

- tier × disposition: applied, applied-disclosed, user-accepted, user-vetoed, and advisory open, dismissed, parked or converted;
- the user-veto rate;
- growth from edits: applied edits, characters added, and entries added or removed;
- re-check yield: findings minted by a run whose `rechecks` is set;
- cost: agents, and minutes from started to finished.

Disposition derives from tier, status and `applied`, so no new status is added. `review stats --by model` groups audit overturns by the verifying role's model from the run's `models`, which closes `B-13ec633a`.

`measurement.md` gains the plan seeded-recall procedure, from Research Notes, "Execution joins cannot exist yet". It also rebinds the plan threshold from "dismissed at the merge offer" to user-veto rate, and gains a contract note for the engine: count a deviation as an escape only with a `plan-defect` cause, and report reviewed and skipped plans side by side.

### /plan-v2 and /review-plan-v2

- **`/plan-v2`, the Review step.** It runs unless `--no-review` is given (recorded as a skip, approved `skipped-flag`) or the planner judges the plan low risk (recorded as a skip with the judgement, approved `skipped-low-risk`). `--review` forces the review. The procedure is the engine's new `references/plan-flow.md`, run in this order:
  1. `plan run start --step review --workflow plan-v2-review`.
  2. The wrapper runs.
  3. Mop-up.
  4. Mechanical and cited rows go through `review apply`.
  5. User-tier rows are asked in `AskUserQuestion` batches; an accepted row goes through `review apply --user-accepted`, and a vetoed one is dismissed.
  6. Every `spec.accept` entry an applied finding changed is re-probed (`plan probes`).
  7. Untriaged advisory is parked.
  8. The re-check runs.
  9. `run finish`, then `plan run finish`.

  The gate shows `review_edits` and `risks`. When review was skipped, the gate question offers "review now". Approve then passes `--review ran`.
- **`/review-plan-v2`.** It enters flow mode when the resolved flow has `plan.toml`, and runs the same `plan-flow.md`. On an approved flow, every edit is asked whatever its tier, because intent is frozen, and a brief edit then needs `plan approve --amend`.

### Deploy

`scripts/link-v2-review.ps1` is idempotent. Under `$env:CLAUDE_CONFIG_DIR`, or `~/.claude` when that is unset, it links each review-v2 entry that is missing:

- the commands `review-v2.md`, `optimise-v2.md` and `review-plan-v2.md`;
- the agents `review-surveyor`, `review-sweeper`, `review-checker`, `review-finder`, `review-finder-cf`, `review-verifier`, `plan-executor` and `plan-review-lead`;
- the skill directories `review-v2-engine`, `review-v2-method` and `review-v2-lenses`, as junctions;
- the Workflows `review-v2.js` and `plan-v2-review.js`, as file symbolic links.

An entry that already links to its target is skipped. A real file in an entry's place is reported and left alone. File symbolic links need an elevated shell while Developer Mode is off (`docs/ideas/plan-v2-probes.md`), so the script checks first and, unelevated, creates the junctions and lists each file link it could not make; After Merge carries the elevated run. Task 33 runs the script and `cargo install --path tomlctl`, retrying the install once on `os error 5`, the lock another session's running `tomlctl.exe` holds.

### What departs from the idea doc and v2-planning

| Source | This plan | Why |
|---|---|---|
| Idea doc: notes after findings, recorded verdict changes | Not built | Scope; blind finders stay prompt-only |
| Idea doc: exceptional targeted deep re-review | Not built | Scope; the gate shows the open critical instead |
| Idea doc: the lead writes proposed edits to a file | Edits live on findings (`target`/`expect`/`new`) and `review apply` reads them | One store, compare-and-set, and an outcome record |
| `v2-planning` Slice 3: ~15 files | 71 | Flow mode reaches about 30 markdown-bound sites, plus the apply verb and tests (User Decision 1) |
| User Decision 6: "applies" | Applied before approval leaves no `[[changes]]` row; the finding's `applied` is the record | `audit` is false until approval |

## Success Criteria

- forward: `review apply` exists. `grep -q 'Apply(ReviewApplyArgs)' tomlctl/src/cli/types/review.rs` exits 0 (today: exits 1).
- forward: the wrapper and the lead exist. `test -f claude/workflows/plan-v2-review.js && test -f claude/agents/plan-review-lead.md` exits 0 (today: exits 1).
- forward: `/plan-v2` no longer says review is not built. `! grep -qs 'not built' claude/skills/plan-v2-engine/SKILL.md claude/skills/plan-v2-engine/references/approve.md` exits 0 (today: exits 1).
- forward: the gate no longer hard-codes an empty review. `! grep -q 'REVIEW_NOTE' tomlctl/src/plan/approve.rs` exits 0 (today: exits 1).
- forward: `B-13ec633a`'s threshold has a producing command. `grep -q 'review stats --by model' claude/skills/review-v2-engine/references/measurement.md` exits 0 (today: exits 1).
- forward: outcome logging works. `cargo test --manifest-path tomlctl/Cargo.toml --test review_stats` passes, including `plan_outcomes_count_tiers_and_dispositions` and `overturns_group_by_model` **predicted, unverified**. Falsifier: grouping overturns by the audit's own model turns `overturns_group_by_model` red.
- forward: flow-mode review works end to end in fixtures. `cargo test --manifest-path tomlctl/Cargo.toml --test review_plan_records --test review_apply` passes **predicted, unverified**, since it writes build output and the tests do not exist yet.
- forward: markdown mode is unchanged. `review_plan_records`' markdown-mode cases (a plan defect without `plan_section`, a code defect without `line`, each refused) pass, and `cargo test --manifest-path tomlctl/Cargo.toml --test review_add` keeps passing, including `plan_anchor_old_must_be_unique_in_section` **predicted, unverified**. Falsifier: dropping the markdown-mode `plan_section` requirement turns the first case red.
- forward: the wrapper is lint-valid. `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes **predicted, unverified**. Falsifier: an `agentType` naming no agent turns `workflow_scripts_name_real_agents` red.
- guard: glimpse still builds against the bumped tomlctl. `cargo clippy --manifest-path glimpse/Cargo.toml --locked` exits 0 **predicted, unverified**.
- forward: the Workflow is linked. `test -L "$HOME/.claude/workflows/plan-v2-review.js"` exits 0 (today: exits 1). **predicted, unverified** until the elevated link run in After Merge.
- forward: a `/plan-v2` run on a small goal reaches `plan approve --review ran`, with at least one finding carrying `applied`. **predicted, unverified**: it needs the restart in After Merge and an attended session.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
checkpoint: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
checkpoint: bash scripts/verify-shared-blocks.sh
success: grep -q 'Apply(ReviewApplyArgs)' tomlctl/src/cli/types/review.rs
success: test -f claude/workflows/plan-v2-review.js && test -f claude/agents/plan-review-lead.md
success: ! grep -qs 'not built' claude/skills/plan-v2-engine/SKILL.md claude/skills/plan-v2-engine/references/approve.md
success: ! grep -q 'REVIEW_NOTE' tomlctl/src/plan/approve.rs
success: grep -q 'review stats --by model' claude/skills/review-v2-engine/references/measurement.md
success: cargo test --manifest-path tomlctl/Cargo.toml --test review_stats --test review_plan_records --test review_apply --test review_add
success: cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests
success: cargo clippy --manifest-path glimpse/Cargo.toml --locked
test.timeout: 900
checkpoint.timeout: 900
transient: rust-lld: failed to write output.*[Pp]ermission denied
```

The 900-second budgets carry over from `v2-planning`, where the suite took 322 seconds on a quiet machine on 2026-11-05 (`cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`).

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 8, 12, 14, 15, 18, 19, 20, 21, 23, 24, 25, 26, 30, 34, 35
- **Max parallel agents**: 8
- **Commit granularity**: per-task

## Tasks

### Milestone A: the review store learns plan records

#### 1. Add the plan-record finding fields [M]
- **Files**: `tomlctl/src/review/fields.rs`, `tomlctl/src/review/schema_out.rs`, `tomlctl/src/review/runs.rs`
- **Depends on**: —
- **Action**: Add a `Ty::Any` value type and the finding fields `target`, `expect`, `new`, `proof`, `tier` and `applied`, plus the run field `rechecks`, settable at `review run start`.
- **Detail**: Approach, "Record targets", "Tiers" and "review apply" fix the shapes.
  - `target` is `Ty::Str`, `.plan_only()`, cap 300.
  - `expect` and `new` are `Ty::Any`, `.plan_only()`, for defect and cleanup.
  - `proof` is `Ty::Table`, `.plan_only()`, for defect and cleanup.
  - `tier` is `Ty::Str`, `.computed()`, with values `mechanical`, `cited`, `advisory` and `user`.
  - `applied` is `Ty::Table`, `.computed()`.
  - `rechecks` is a `Ty::Str` run field holding a `U<n>` id. Add `"rechecks"` to `START_FIELDS` in `tomlctl/src/review/runs.rs`, which `flag_fields` otherwise refuses.

  **Visibility.** The column order is finder, verifier, checker, implementer, orchestrator, audit, backlog, report. Do not copy `PLAN` (`[N, R, N, R, N, R, N, R]`), whose orchestrator and finder columns are `N`: `review list` and `review digest` project for the orchestrator. Give `target` `[R, R, N, R, R, R, N, R]`; `tier` and `applied` `[N, N, N, R, R, R, N, R]`; `expect`, `new` and `proof` `PLAN`; `rechecks` `RECORD_STATE`. Add a `MATRIX` row for each.

  **Storage.** `Ty::Any` accepts any JSON value. The store cannot hold null or an empty value as-is (`items::add_value_to` drops a top-level null, `""` or `[]`; `convert::json_to_toml` refuses a nested null), so `expect` and `new` are stored as JSON text and decoded on read, and a removal is a `new` holding the text `null`. `schema_out.rs` renders `Ty::Any` as an unconstrained schema `{}`. Extend the in-module visibility test rows (near the `plan_section` rows) to the new fields.
- **Acceptance**:
  - forward: `grep -cE 'field\((Finding|Run), "(target|expect|new|proof|tier|applied|rechecks)"' tomlctl/src/review/fields.rs` prints `7` (today: `0`).
  - forward: `grep -c 'Ty::Any' tomlctl/src/review/schema_out.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib review::fields` passes **predicted, unverified**. Falsifier: dropping `target` from the visibility table leaves the extended visibility test red.

#### 2. Expose plan check reads to the review module [S]
- **Files**: `tomlctl/src/plan/mod.rs`, `tomlctl/src/plan/check.rs`
- **Depends on**: —
- **Action**: Make `check::findings`, `check::design_section` and `check::design_path` reachable from `crate::review` as `pub(crate)`.
- **Detail**: Change `findings` and `design_section` from `pub(super)`, and the private `design_path` (its containment check is what task 11's design writes rely on), to `pub(crate)`, and change `mod check;` in `tomlctl/src/plan/mod.rs` to `pub(crate) mod check;`. Make no behaviour change.
- **Acceptance**:
  - forward: `grep -cE 'pub\(crate\) fn (findings|design_section|design_path)' tomlctl/src/plan/check.rs` prints `3` (today: `0`).
  - forward: `grep -c 'pub(crate) mod check;' tomlctl/src/plan/mod.rs` prints `1` (today: `0`).

#### 3. Parse and resolve record targets [M]
- **Files**: `tomlctl/src/review/target.rs` (new), `tomlctl/src/review/mod.rs`, `tomlctl/src/review/add.rs`
- **Depends on**: 2
- **Action**: Add `review/target.rs`: parse the target grammar, resolve a target against a flow's records to its current JSON value, and match list entries by value.
- **Detail**:
  - **Grammar and resolution.** The grammar is the table in Approach, "Record targets". Resolution loads `plan.toml` through `crate::plan::store::load`, and `tasks.toml` through the task store loader that `crate::tasks` re-exports.
  - **Prose.** Brief sections resolve through `add.rs` `section_bodies`; make it `pub(super)`, with no other change in `add.rs`. Design anchors resolve through `crate::plan::check::design_section`.
  - **API.** Expose:
    - `pub(super) fn parse(&str) -> Result<RecordTarget>`, an enum with one variant per row of the Approach table (`TaskField`, `SpecField`, `SpecEntry { list, index: Option<usize>, append: bool }`, `Decision`, `Contract`, `Milestone`, `Handover`, `Brief`, `Design`); not `Target`, which `review::store::Target` already names in `add.rs`, `verdict.rs`, `resolve.rs` and `digest.rs`;
    - `pub(super) fn resolve(slug: &str, target: &RecordTarget) -> Result<JsonValue>`;
    - `pub(super) fn match_entry(list: &[JsonValue], expect: &JsonValue) -> Result<usize>`, which refuses 0 or 2+ matches;
    - `RecordTarget::file` and `RecordTarget::symbol`, per the table.

    Use `RecordTarget::field_name()` for the part before `[`. `title` is not a task field target. `resolve` and `match_entry` compare `spec.accept` entries with the tool-written `probe` key removed on both sides, as `tasks/update.rs` `keep_probes` does.
  - **Registration.** Register `mod target;` in `tomlctl/src/review/mod.rs`.
  - **Tests.** Add in-module unit tests for each target kind, one unknown field, a refused `title`, an empty ref, a `[+]` append, a probed accept entry matched by an `expect` without `probe`, and a duplicate-entry refusal.
- **Acceptance**:
  - forward: `test -f tomlctl/src/review/target.rs && grep -c '^mod target;' tomlctl/src/review/mod.rs` prints `1` (today: exits 1, no output).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib review::target` passes **predicted, unverified**. Falsifier: accepting an empty ref after `task:` turns the empty-ref test red.

#### 4. Compute a finding's tier [M]
- **Files**: `tomlctl/src/review/tier.rs` (new), `tomlctl/src/review/mod.rs`, `tomlctl/src/review/query.rs`
- **Depends on**: 1, 3
- **Action**: Add `review/tier.rs` with the tier rules from Approach, "Tiers": `verify_proof` for the four mechanical proof kinds and the `new`-shape limits, `compute`, which derives a tier from a JSON row, and `stamp`, which sets `tier` on a stored finding.
- **Detail**:
  - **`compute`.** `pub(super) fn compute(row: &Map<String, JsonValue>, slug: &str, verdicts: &[&Verdict]) -> Result<Option<&'static str>>` applies the user table first, then the advisory conditions, then proof, then citation, and returns `None` for a row outside flow mode.
  - **`stamp`.** `pub(super) fn stamp(finding: &mut store::Finding, slug: &str, verdicts: &[&Verdict]) -> Result<()>` reads the typed finding (extra fields live in its TOML `rest` table), calls `compute`, and writes `tier` only when absent. A store it cannot read leaves `tier` absent rather than writing a default.
  - **`verify_proof`.** `pub(super) fn verify_proof(proof: &Map<..>, target: &RecordTarget, slug: &str) -> Result<(), String>` checks the four kinds:
    - `probe` reads the target `accept` entry's stored `probe.verdict`; a missing verdict is no proof;
    - `check-class` filters `crate::plan::check::findings` by class and by the target task's numeric id (its `<ref>` mapped through the task store); `plan check` findings carry only task ids, so a `check-class` proof on a `contract:*`, `decision:*` or `milestone:*` target is refused;
    - `path-missing` tests the tree and the task's `file_notes`;
    - `ref-unknown` tests the rows.

    It also enforces the `new`-shape limits in Approach, "Tiers" (the fields a `probe` or `path-missing` fix may change, the class-to-field table, the 800-character cap).
  - **Citation.** The cited test uses `query.rs` `parse_citation`; make it `pub(super)`, with no other change in `query.rs`. `parse_citation` only parses, so `tier.rs` reads the cited file to check the line range, and requires the path to be the target's own file or a path the target names.
  - **Registration.** Register `mod tier;` in `tomlctl/src/review/mod.rs`.
  - **Tests.** Unit-test each tier: the user override beating a valid proof, a `handover:*` target capped at advisory, a proof that no longer holds, a `probe` fix that flips `writes_build` (capped at cited), and a probe-absent entry (no proof).
- **Acceptance**:
  - forward: `test -f tomlctl/src/review/tier.rs && grep -c '^mod tier;' tomlctl/src/review/mod.rs` prints `1` (today: exits 1, no output).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib review::tier` passes **predicted, unverified**. Falsifier: checking proof before the user table turns the user-override test red.

#### 5. Validate plan-record rows at review add [M]
- **Files**: `tomlctl/src/review/add.rs`, `tomlctl/src/review/fields.rs`, `tomlctl/src/review/triage.rs`
- **Depends on**: 1, 3, 4
- **Action**: In `review add` and in `review triage --convert`, detect flow mode and refuse rows that break Approach, "Record targets"; stamp a flow-mode advisory row's tier at mint.
- **Detail**:
  - **Detection.** Flow mode means a plan family whose store slug has `.claude/flows/<slug>/plan.toml`. A flow-less store (`--scope`) is never in flow mode.
  - **The check.** Add `pub(super) fn record_refusals(root: &Path, slug: Option<&str>, row: &Map<String, JsonValue>, kind: Kind, family: Family) -> Vec<String>` beside `text_refusals`. Call it from `prepare` and from the `triage.rs` convert path (beside its `field_refusals` and `text_refusals` calls), which mints rows without passing through `prepare`. In flow mode a defect or cleanup row needs `target`. `plan_section`, `anchor_old` and `anchor_new` are refused. Refuse also:
    - a target that does not parse or resolve;
    - a `file` or `symbol` other than the target's;
    - `new` without `expect`, except on a `[+]` append;
    - an `expect` other than the current value, judged by `target::match_entry` for list entries;
    - a `proof` that `tier::verify_proof` rejects.
  - **Markdown and code modes.** `record_refusals` also carries the requiredness `fields.rs` gives up: `line` for every defect and cleanup row outside flow mode, and `plan_section` for a markdown-mode plan defect or cleanup row, with the catalogue's message wording (`` `line` is required … ``).
  - **`fields.rs`.** Change `plan_section` and `line` from `Need::RequiredFor(DEFECT_OR_CLEANUP)` to optional. This drops `line` from the `required` set `review schema` emits for every family's defect and cleanup rows (`schema_out.rs` `required()`); accepted, since `review add` stays authoritative and refuses the row. `plan_section` is a plan-family field the schema never marks required, so it is unaffected.
  - **Advisory tier.** `add.rs` mints advisory rows directly `open` (`(Kind::Advisory, None) => FindingStatus::Open`), so they never pass a verdict: in flow mode, set `tier` there from `tier::compute` before the row is stored.
- **Acceptance**:
  - forward: `grep -c 'fn record_refusals' tomlctl/src/review/add.rs` prints `1` (today: `0`).
  - forward: `grep -c 'record_refusals' tomlctl/src/review/triage.rs` prints at least `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test review_add` passes **predicted, unverified**. Its missing-requirement coverage is task 8's markdown-mode cases, since no existing test asserts a missing `plan_section` refusal.

#### 6. Stamp the tier when a plan-record finding is verified [M]
- **Files**: `tomlctl/src/review/verdict.rs`, `tomlctl/src/review/resolve.rs`, `tomlctl/src/review/triage.rs`
- **Depends on**: 1, 4, 5
- **Action**: Call `tier::stamp(` wherever a verified flow-mode plan finding first moves to `FindingStatus::Open`.
- **Detail**:
  - **Sites.** The sites are where `verdict.rs` `apply_one` writes the status `decide` returns (`finding.set_status(decided, &[])`), never inside `settle`, a pure function over verdicts shared with `stats.rs` and `resolve.rs`; the two `set_status(FindingStatus::Open, …)` sites in `resolve.rs`; and `triage.rs` `--reopen`, which can open a `refuted` row that was never stamped. Task 5 covers the advisory mint.
  - **Slug.** `stamp` takes a slug: thread the review store `Target`'s slug into `apply_one` and `resolve`. A scope-targeted store is never in flow mode.
  - **Freeze.** Stamp only when `tier` is absent, so the value stays frozen. Code and perf rows, and markdown-mode plan rows, get no `tier`.
- **Acceptance**:
  - forward: `cat tomlctl/src/review/verdict.rs tomlctl/src/review/resolve.rs tomlctl/src/review/triage.rs | grep -c 'tier::stamp('` prints at least `3` (today: `0`).

#### 7. Show targets and tiers in the digest [S]
- **Files**: `tomlctl/src/review/digest.rs`
- **Depends on**: 1
- **Action**: Make `digest.rs` `location` render `target` when present, before the `§ plan_section` form, and add `Target`, `Expect`, `New` and `Tier` lines to `defect_entry` and `cleanup_entry` for rows that carry them.
- **Detail**: `location`, `defect_entry` and `cleanup_entry` render the markdown of `review report` (`render`, called from `report`), not `review digest`; the digest's JSON lists carry `target`, `tier` and `applied` through task 1's orchestrator visibility, with no change here. Leave the markdown-mode rendering unchanged.
- **Acceptance**:
  - forward: `grep -c '"tier"' tomlctl/src/review/digest.rs` prints at least `1` (today: `0`). (`"target"` already occurs at `digest.rs:112`, so it cannot discriminate.)

#### 8. Test plan-record findings [M]
- **Files**: `tomlctl/tests/review_plan_records.rs` (new)
- **Depends on**: 5, 6, 7
- **Action**: Add an integration test over a fixture v2 flow covering five things: `review add` accepting and refusing targets, the tier stamped at verdict and at advisory mint, the frozen tier, the report's target line, and markdown and code requiredness unchanged.
- **Detail**:
  - **Fixture.** Build the flow with `plan open`, `plan decide`, `plan milestone add` and `tasks add-many` with a `spec`, in a temp repo, on the pattern `tomlctl/tests/plan_lifecycle.rs` uses.
  - **Cases.** Cover one row per target kind, a stale `expect`, a removal (`new` null) and an `expect` of `""` surviving the store round trip, a false `probe` proof, a user-tier `decision` target with a valid proof (it stays `user`), a user-tier advisory row stamped at mint, a cited verdict, a `review triage --convert` replacement refused for a bad target, and the target line in `review report`.
  - **Unchanged modes.** On a markdown-mode plan store, a plan defect without `plan_section` is refused; on a code store, a defect without `line` is refused. No existing test asserts either refusal.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_plan_records.rs` exits 0 (today: exits 1).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test review_plan_records` passes **predicted, unverified**. Falsifier: removing the stale-`expect` refusal turns its case red, and dropping the markdown-mode `plan_section` requirement turns the missing-section case red.

### Milestone B: apply, stats and the CLI

#### 9. Expose task-row writes for review apply [M]
- **Files**: `tomlctl/src/tasks/mod.rs`, `tomlctl/src/tasks/update.rs`
- **Depends on**: —
- **Action**: Add `pub(crate) fn apply_review_edit` to `tasks/update.rs`, and re-export it from `tomlctl/src/tasks/mod.rs`.
- **Detail**: It takes the slug, a ref, a field name (a row field other than `title`, or a `spec.<field>`; milestones are task 10's), an optional list index hint, an append flag, `expect`, `new`, an actor and a reason. Inside the store's `mutate` lock it re-reads the current value, compares it with `expect` (list entries matched by value, `spec.accept` entries with `probe` removed on both sides), and applies the change through the same patch path `tasks update --ndjson` uses, so `keep_probes`, `record_changes` (when `audit`) and validation all run. It returns `pub(crate) enum ReviewEditOutcome { Applied, Already, Stale { found } }`, defined here and re-exported from `tomlctl/src/tasks/mod.rs` as `crate::tasks::ReviewEditOutcome`; an append is `Already` when an equal entry exists, a removal when no entry equals `expect`. Add `pub(crate) fn validate_review_edit` with the same inputs, which runs the patch's validation without writing, for task 11's refusal pass. Do not change existing verb behaviour.
- **Acceptance**:
  - forward: `grep -c 'pub(crate) fn apply_review_edit' tomlctl/src/tasks/update.rs` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_write` passes **predicted, unverified**.

#### 10. Expose plan record writes for review apply [M]
- **Files**: `tomlctl/src/plan/mod.rs`, `tomlctl/src/plan/records.rs`, `tomlctl/src/plan/milestone.rs`
- **Depends on**: 2, 9
- **Action**: Add `pub(crate) fn apply_review_edit` to `plan/records.rs`, for decision and contract keys, and to `plan/milestone.rs`, for milestone fields, and re-export them from `tomlctl/src/plan/mod.rs` under distinct names.
- **Detail**: Each function re-checks `expect` inside `crate::plan::store::mutate`, or the task store's lock for milestones, and writes through the same upsert path `plan decide` and `plan contract` use, or the one `plan milestone update` uses. That milestone logic is inline in `milestone.rs` `update()`, so extract it into a shared function both call, and give `change` (which hard-codes `DEFAULT_ACTOR`) an `actor` parameter. Milestone writes keep `ref = "milestone:<id>"` audit rows when `audit` is set, with actor `review`. Both return task 9's `crate::tasks::ReviewEditOutcome`. Re-export them as `apply_record_edit` and `apply_milestone_edit`, since two `apply_review_edit` names cannot share `plan/mod.rs`. Add a validate-only companion for each, as task 9 does. Leave the `mod check` line as task 2 left it.
- **Acceptance**:
  - forward: `cat tomlctl/src/plan/records.rs tomlctl/src/plan/milestone.rs | grep -c 'pub(crate) fn apply_review_edit'` prints `2` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_records --test plan_milestone` passes **predicted, unverified**.

#### 11. Implement review apply [L]
- **Files**: `tomlctl/src/review/apply.rs` (new), `tomlctl/src/review/dispatch.rs`, `tomlctl/src/review/mod.rs`, `tomlctl/src/cli/types/review.rs`, `tomlctl/src/cli/mod.rs`
- **Depends on**: 1, 4, 9, 10
- **Action**: Add `tomlctl review apply <ids> --slug <slug> [--user-accepted] [--dry-run]` as Approach, "review apply" specifies.
- **Detail**:
  - **The five files.** These edits are inseparable: the `ReviewOp::Apply(ReviewApplyArgs)` variant, the `ReviewApplyArgs` re-export in `tomlctl/src/cli/mod.rs`, the dispatch arm, `mod apply;`, and the module itself.
  - **Arguments.** Take the store flags through `ReviewStore`, as `ReviewTriageArgs` does, and the write-integrity args as the other writers do.
  - **Refusals.** Refuse every id before any write, as one `kind=validation` error naming each id and its reason: the refusal list in Approach, "review apply", including a flow-mode row with no `tier` and any row without `--user-accepted` while `plan.toml` holds an `[[approval]]`. In the same pass, run each `new` through task 9's or task 10's validate-only companion and re-verify each mechanical proof with `tier::verify_proof`. A brief or design edit refuses an `expect` text that does not occur exactly once in its section.
  - **Writing.** Write each target through `crate::tasks::apply_review_edit`, `crate::plan::apply_record_edit` or `crate::plan::apply_milestone_edit`, or by exactly-once replacement for brief and design: under `crate::io::with_exclusive_lock` on the prose file, resolve a design path through `crate::plan::check::design_path` (never re-joined by hand), refuse a file that is not UTF-8 rather than writing back `read_normalised`'s lossy text, keep the file's line endings, and persist with `crate::io::atomic_write`. `mutate_doc` cannot be used, since `guard_write_path` refuses paths outside `.claude/`.
  - **Stamping.** Stamp each finding under the review store's lock right after its own target write, never in a batch at the end: status `resolved`, resolution `applied to <target>`, and `applied` from `tier`, the old value, `new`, the time, and `by`, with `old` and `new` stored as JSON text. An `Already` outcome is stamped the same way, with `old` from `expect` and `recovered = true`.
  - **Dry run.** `--dry-run` reports what would happen and writes nothing.
- **Acceptance**:
  - forward: `grep -c 'Apply(ReviewApplyArgs)' tomlctl/src/cli/types/review.rs` prints `1` (today: `0`).
  - forward: `test -f tomlctl/src/review/apply.rs` exits 0 (today: exits 1).

#### 12. Register review apply in capabilities [L]
- **Files**: `tomlctl/src/capabilities.rs`, `tomlctl/src/cli/types/mod.rs` — `FEATURES`, `tomlctl/README.md` — sample `features` block and "Feature meanings" table, `tomlctl/tests/capabilities.rs`
- **Depends on**: 11
- **Action**: Add `("review apply", REVIEW_STORE)` beside `review triage`. Add the features `review_apply`, `review_targets` and `review_plan_outcomes` to `FEATURES` in `tomlctl/src/cli/types/mod.rs`, both README transcriptions of that list, and the `--help` row the capabilities test enumerates for every review verb.
- **Detail**: The four edits are inseparable: `readme_feature_transcriptions_match_capabilities_features` holds both README copies to `FEATURES`, and the capabilities test asserts the feature count exactly.
- **Acceptance**:
  - forward: `grep -c '"review apply"' tomlctl/src/capabilities.rs` prints `1` (today: `0`).
  - forward: `grep -c '"review_apply"' tomlctl/src/cli/types/mod.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes **predicted, unverified**. Falsifier: leaving `review_apply` out of the README "Feature meanings" table turns `readme_feature_transcriptions_match_capabilities_features` red.

#### 13. Report plan-review outcomes and per-model overturns [M]
- **Files**: `tomlctl/src/review/stats.rs`, `tomlctl/src/cli/types/review.rs`
- **Depends on**: 6, 11
- **Backlog**: B-13ec633a
- **Action**: Add `review stats --plan-outcomes` and a `model` value for `--by`, as Approach, "Outcomes" specifies.
- **Detail**:
  - **`--plan-outcomes`.** It needs `--slug` or `--all`, reads plan-family runs only, and prints `{runs:[{run, rechecks, tiers, veto_rate, growth, recheck_yield, cost}], total}`.
  - **Disposition.** Derive it from `tier`, `status` and `applied.by`.
  - **`--by model`.** Group `overturn()` results by the verifying role's model in the run's `models`: `verifier` and `panel` read `models.verifier`, and `checker` reads `models.checker`. A run without `models` groups as `unknown`.
  - **CLI.** Add the flag and the `--by` help text to `ReviewStatsArgs`.
- **Acceptance**:
  - forward: `grep -c 'plan_outcomes' tomlctl/src/review/stats.rs` prints at least `1` (today: `0`).
  - forward: `grep -c 'plan_outcomes' tomlctl/src/cli/types/review.rs` prints at least `1` (today: `0`).

#### 14. Test review apply [M]
- **Files**: `tomlctl/tests/review_apply.rs` (new)
- **Depends on**: 5, 6, 11
- **Action**: Add an integration test over a fixture v2 flow that drives `review add`, `review verdict` and `review apply`.
- **Detail**: Build the flow with `plan open`, `plan decide`, `plan milestone add` and `tasks add-many` with a `spec`, in a temp repo, on the pattern `tomlctl/tests/plan_lifecycle.rs` uses. Start one review run with `review run start --set rechecks=U1` to show the field is settable. Cover these cases:
  - a mechanical `spec.accept` edit that drops its probe;
  - a cited `files` edit;
  - an advisory refusal;
  - a user-tier refusal without `--user-accepted`, then acceptance with it, which records `by = "user"`;
  - a stale `expect` under `skipped_stale`;
  - an already-applied value under `already`, still stamped with `applied`;
  - a re-run `[+]` append and a re-run removal, each `already`, not duplicated or stale;
  - a `new` the target store rejects (a `needs` cycle), refused before any write;
  - an approved flow refusing a mechanical row without `--user-accepted`;
  - an audited store writing a `[[changes]]` row with actor `review`;
  - a brief edit, and a brief edit whose text occurs twice, which is refused;
  - `--dry-run` writing nothing (compare store bytes).
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_apply.rs` exits 0 (today: exits 1).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test review_apply` passes **predicted, unverified**. Falsifier: skipping the in-lock `expect` re-check turns the stale case red.

#### 15. Test plan outcomes and per-model overturns [S]
- **Files**: `tomlctl/tests/review_stats.rs`
- **Depends on**: 13
- **Action**: Add `plan_outcomes_count_tiers_and_dispositions` and `overturns_group_by_model` over hand-written store fixtures.
- **Acceptance**:
  - forward: `grep -cE 'fn (plan_outcomes_count_tiers_and_dispositions|overturns_group_by_model)' tomlctl/tests/review_stats.rs` prints `2` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test review_stats` passes **predicted, unverified**. Falsifier: grouping overturns by the audit's own model instead of the verifying role's turns `overturns_group_by_model` red.

#### 35. Give review apply an output-options case [M]
- **Files**: `tomlctl/tests/output_options.rs`
- **Depends on**: 6, 11
- **Action**: Add a `review apply` case to the command table `every_leaf_command_has_a_case` walks, with the flow fixture it needs.
- **Detail**: `every_leaf_command_has_a_case` fails ("commands without a case") once task 11 adds the `review apply` leaf. The case needs a v2 flow fixture holding one open, tiered, flow-mode finding: build it on the existing `review` cases' pattern in that file, with `plan open` and `tasks add-many` as `tomlctl/tests/plan_lifecycle.rs` does, then a `review add` and a `review verdict`. Exercise `review apply --dry-run` so the case writes nothing.
- **Acceptance**:
  - forward: `grep -c '"review", "apply"' tomlctl/tests/output_options.rs` prints at least `1` (today: `0`); cases are argument arrays such as `&["review", "triage", …]`.
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_options` passes **predicted, unverified**. Falsifier: deleting the case turns `every_leaf_command_has_a_case` red once task 11 has landed.

### Milestone C: the plan reads its review

#### 16. Read the review into the plan gate and approve [M]
- **Files**: `tomlctl/src/plan/approve.rs`, `tomlctl/src/plan/store.rs`
- **Depends on**: 1
- **Action**: Add `plan_review_state` to `plan/store.rs`, use it in `gate_report` and `approve`, and remove `REVIEW_NOTE` and the `review_note` key.
- **Detail**:
  - **`plan_review_state`.** It reads `review-v2.toml` as a plain document, as `plan_review_finished` does. It returns finished and unfinished plan-family runs, applied plan findings (their `applied` `old`/`new` decoded from JSON text), open findings with severity and `tier`, and open findings from runs that carry `rechecks`. Keep `plan_review_finished` as a thin wrapper over it, so `stage.rs` stays untouched here; `approve.rs` calls `plan_review_state` directly, and task 17 moves `review_settled` onto it.
  - **`gate_report`.** It fills `review_edits` and `risks` per Approach, "Gate and approve".
  - **`approve`.** `--review ran` additionally refuses an unfinished plan-family run, any open user-tier finding, and any `spec.accept` entry an applied finding changed that has no probe, naming the ids.
- **Acceptance**:
  - forward: `grep -c 'REVIEW_NOTE' tomlctl/src/plan/approve.rs` prints `0` (today: at least `1`).
  - forward: `grep -c 'fn plan_review_state' tomlctl/src/plan/store.rs` prints `1` (today: `0`).

#### 17. Route review in plan stage [S]
- **Files**: `tomlctl/src/plan/stage.rs`, `tomlctl/tests/plan_stage.rs`, `tomlctl/tests/plan_lifecycle.rs`
- **Depends on**: 16
- **Action**: Make `review_settled` use `plan_review_state`. Review is settled by a skip, or by a finished run with no unfinished plan run and no open user-tier finding.
- **Detail**: Set `PLAN_REVIEW` to name the `/plan-v2` Review step and `plan run skip --step review --reason <why>`. Add a blocker line counting open user-tier findings. Extend `tomlctl/tests/plan_stage.rs` with a fixture where an open user-tier finding keeps the stage at review. `tomlctl/tests/plan_lifecycle.rs` asserts the current hint with exact equality (`flow.assert_stage("review", "plan run skip --step review --reason <why>")`) and the blocker text on the next line; update both to the new wording.
- **Acceptance**:
  - forward: `grep -c 'plan_review_state' tomlctl/src/plan/stage.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_stage --test plan_lifecycle` passes **predicted, unverified**. Falsifier: settling review while an open user-tier finding remains turns the new `plan_stage` fixture red.

#### 18. Update the gate and approve tests [M]
- **Files**: `tomlctl/tests/plan_approve.rs`
- **Depends on**: 16
- **Action**: Replace the assertions on an empty `review_edits` and on `review_note` with fixtures that cover a filled `review_edits` and `review-finding` risks, and `--review ran` refused for an open user-tier finding, for an unfinished run, and for an applied `spec.accept` edit left unprobed.
- **Acceptance**:
  - forward: `grep -c 'review_note' tomlctl/tests/plan_approve.rs` prints `0` (today: at least `1`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test plan_approve` passes **predicted, unverified**. Falsifier: approving with an open user-tier finding turns the refusal case red.

#### 19. Bump tomlctl to 0.19.0 [L]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/README.md` — sample `version`, `tomlctl/tests/capabilities.rs` — `capabilities_version_matches_cargo_toml` literal
- **Depends on**: 11, 12
- **Action**: Set `version = "0.19.0"` in `tomlctl/Cargo.toml`. Refresh `tomlctl/Cargo.lock`, then `glimpse/Cargo.lock` with `cargo tree --manifest-path glimpse/Cargo.toml`, discarding its stdout, so the pre-commit `--locked` clippy accepts it. Change the sample `"version": "0.18.0"` in `tomlctl/README.md` and the `"0.18.0"` literal in `capabilities_version_matches_cargo_toml` to `0.19.0`.
- **Detail**: The five edits are inseparable: `readme_sample_version_matches_cargo_toml` and `capabilities_version_matches_cargo_toml` compare both copies with `CARGO_PKG_VERSION`, as the previous bump `67be38e` did. Task 12 also edits `tomlctl/README.md` and `tomlctl/tests/capabilities.rs`, hence the edge.
- **Acceptance**:
  - forward: `grep -c '^version = "0.19.0"' tomlctl/Cargo.toml` prints `1` (today: `0`).
  - forward: `grep -c '0.18.0' tomlctl/README.md tomlctl/tests/capabilities.rs` prints `tomlctl/README.md:0` and `tomlctl/tests/capabilities.rs:0` (today: `1` and `2`; the test's literal and its failure message, whose stated reason for the bump is rewritten for this one).

#### 20. Document the review store additions [M]
- **Files**: `claude/skills/tomlctl/references/review.md`
- **Depends on**: 5, 11, 13
- **Action**: Document the following in `references/review.md`: the flow-mode plan fields and target grammar; the `tier` rules and `proof` kinds; a `review apply` section with a flag table and output; `review stats --plan-outcomes` and `--by model`; and the run's `rechecks`.
- **Detail**: Rewrite the plan-family lines that today say `plan_section` and `anchor_old` (around lines 96, 98 and 110) for the two modes, and the line saying `line` is required: `review add` now enforces it outside flow mode, and `review schema` no longer lists it. Every flag table row must name a real clap long (`flag_table_lint`), and every fenced example must use real flags (`command_lint`). Keep the file at or under 600 lines (`skill_references_under_line_ceiling`; `wc -l claude/skills/tomlctl/references/review.md` is 504 today), or move the new material to a new reference and add it and `claude/skills/tomlctl/SKILL.md` to Files.
- **Acceptance**:
  - forward: `grep -c 'review apply' claude/skills/tomlctl/references/review.md` prints at least `1` (today: `0`).

#### 21. Document the gate's review keys [S]
- **Files**: `claude/skills/tomlctl/references/plan.md`
- **Depends on**: 16, 17
- **Action**: Update the `plan gate`, `plan approve` and `plan stage` sections of `references/plan.md` for `review_edits`, `review-finding` risks, the new `--review ran` refusals, and the review blocker.
- **Detail**: Remove the "always empty" and `review_note` wording, and the stage table's "since plan review is not built for v2 flows". The file is at 589 of the 600 lines `skill_references_under_line_ceiling` allows (`wc -l claude/skills/tomlctl/references/plan.md`): keep net growth under 11 lines, or move the gate section to a new reference and add it and `claude/skills/tomlctl/SKILL.md` to Files.
- **Acceptance**:
  - forward: `grep -c 'review_note' claude/skills/tomlctl/references/plan.md` prints `0` (today: at least `1`).
  - forward: `grep -c 'not built' claude/skills/tomlctl/references/plan.md` prints `0` (today: `1`).

### Milestone D: reviewers, the lead, the wrapper and the carriers

#### 22. Write the plan-records reference [M]
- **Files**: `claude/skills/review-v2-method/references/plan-records.md` (new), `claude/skills/review-v2-method/SKILL.md`
- **Depends on**: 11
- **Action**: Write `plan-records.md` per Approach, "Reviewers on a v2 flow": inputs, the never-read-`notes/` rule, the mapping table, the precedence rule reading any lens or method instruction to supply `plan_section`, `anchor_old` or `anchor_new` as `target`, `expect` and `new`, the target grammar (no `title` target; `[+]` appends; a `new` of null removes), `file`/`symbol`, `expect`/`new` copied from `tomlctl tasks show <id> --slug <slug> --with spec` with the `probe` key left out, `proof` kinds with the `new`-shape limits from Approach, "Tiers", and PRIOR on task refs. Then split the method's §Plan family into markdown mode, which is today's text, and flow mode, which points at `references/plan-records.md`.
- **Acceptance**:
  - forward: `test -f claude/skills/review-v2-method/references/plan-records.md && grep -c 'plan-records.md' claude/skills/review-v2-method/SKILL.md` prints at least `1` (today: exits 1, no output).

#### 23. Teach finding kinds and the sweeper the record targets [S]
- **Files**: `claude/skills/review-v2-method/references/finding-kinds.md`, `claude/agents/review-sweeper.md`
- **Depends on**: 22
- **Action**: Add the flow-mode plan row shape to the defect and cleanup sections of `finding-kinds.md`, and to step 8 of `review-sweeper.md`, each citing `references/plan-records.md`.
- **Detail**: Leave the shared `review-v2-store-writer` paragraph in `review-sweeper.md` untouched.
- **Acceptance**:
  - forward: `cat claude/skills/review-v2-method/references/finding-kinds.md claude/agents/review-sweeper.md | grep -c 'plan-records.md'` prints at least `2` (today: `0`).
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0 (today: exits 0).

#### 24. Bind the surveyor checks to a v2 flow [M]
- **Files**: `claude/skills/review-v2-lenses/checks/store-check.md`, `claude/skills/review-v2-lenses/checks/format-contract.md`, `claude/skills/review-v2-lenses/checks/break-set-baseline.md`
- **Depends on**: 22
- **Action**: Add a "v2 flow" branch to each check. `store-check` runs `tomlctl plan check --slug <slug>` and `tomlctl tasks check --slug <slug>` in place of the import dry run, and maps their error classes into the `STORE CHECK` fence, naming `plan check`'s classes without backticks: this file and task 31's `claude/commands/review-plan-v2.md` are in `finding_classes.rs` `PROSE`, whose `prose_class_names_exist_in_the_source` gate accepts only `src/tasks` classes. `format-contract` reports `not run: v2 flow`. `break-set-baseline` resolves `none — v2 flow carries no predicted break sets`.
- **Acceptance**:
  - forward: `cat claude/skills/review-v2-lenses/checks/store-check.md claude/skills/review-v2-lenses/checks/format-contract.md claude/skills/review-v2-lenses/checks/break-set-baseline.md | grep -c 'v2 flow'` prints at least `3` (today: `0`).

#### 25. Bind the plan-checks to records [M]
- **Files**: `claude/skills/review-v2-lenses/checks/two-control-probe.md`, `claude/skills/review-v2-lenses/checks/falsifiability.md`, `claude/skills/review-v2-lenses/checks/acceptance-contradiction.md`
- **Depends on**: 22
- **Action**: Add a "v2 flow" branch to each check.
  - `two-control-probe` reads `plan check`'s probe-unhealthy and probe-absent classes instead of running its helper. It raises a probe-unhealthy entry as a defect targeting the `spec.accept` entry with `proof {kind:"probe"}`, and a probe-absent entry (no stored verdict) with no `proof`, so it tiers cited or advisory. It judges `writes_build = true` entries by reading.
  - `falsifiability` reads `spec.accept` `kind`, `check` and `positive`.
  - `acceptance-contradiction` reads specs and contracts across tasks.
- **Acceptance**:
  - forward: `cat claude/skills/review-v2-lenses/checks/two-control-probe.md claude/skills/review-v2-lenses/checks/falsifiability.md claude/skills/review-v2-lenses/checks/acceptance-contradiction.md | grep -c 'v2 flow'` prints at least `3` (today: `0`).

#### 26. Bind the surveyor and the executor to a v2 flow [M]
- **Files**: `claude/agents/review-surveyor.md`, `claude/agents/plan-executor.md`, `claude/skills/review-v2-engine/references/agent-waves.md`
- **Depends on**: 22
- **Action**: Give the surveyor prompt in `agent-waves.md` a `flow: <slug>` line, used in place of `plan:` on a v2 flow. Have `review-surveyor.md` read the flow-mode inputs at steps 2, 6, 7 and 11. Have `plan-executor.md` read each task through `tomlctl plan view --slug <slug> --for worker --task <n>` on a v2 flow, and write targets per `references/plan-records.md`.
- **Detail**: Leave every shared block in both agents untouched.
- **Acceptance**:
  - forward: `grep -c 'flow: <slug>' claude/skills/review-v2-engine/references/agent-waves.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'plan view' claude/agents/plan-executor.md` prints at least `1` (today: `0`).
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0 (today: exits 0).

#### 27. Add the plan-review-lead agent [M]
- **Files**: `claude/agents/plan-review-lead.md` (new), `scripts/shared-blocks.toml`
- **Depends on**: 22
- **Action**: Write `plan-review-lead.md` per Approach, "The wrapper and the lead", and add it to the `research-delivering` block's `files` list in `scripts/shared-blocks.toml`. Copy that block byte for byte from `claude/agents/review-finder.md`.
- **Detail**:
  - **Frontmatter.** `name: plan-review-lead`, `tools: Read, Grep, Glob, Bash, Write`, `model: opus`, `effort: high`, `skills: [review-v2-engine, tomlctl]`.
  - **Phases.** The body covers three phases, each with its own return object:
    - `compose`: engine stage 3 and `references/compose.md`; in `recheck` mode, only the `targets`, at `quick`, in one wave.
    - `settle`: engine stage 5.
    - `audit`: the audit part of `references/triage-and-close.md`.
  - **Writes.** Write only inside the run directory and through `tomlctl review plan|run update|resolve|verdict --by audit`. Never `review apply` and never `review triage`.
- **Acceptance**:
  - forward: `test -f claude/agents/plan-review-lead.md && grep -c 'plan-review-lead.md' scripts/shared-blocks.toml` prints `1` (today: exits 1, no output).
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0 (today: exits 0).

#### 28. Add the plan-v2-review Workflow [M]
- **Files**: `claude/workflows/plan-v2-review.js` (new)
- **Depends on**: 27
- **Action**: Write the wrapper as Approach, "The wrapper and the lead" specifies, on the `claude/workflows/plan-v2-investigate.js` pattern.
- **Detail**:
  - **Shape.** A literal `meta` with `name: 'plan-v2-review'` and a `whenToUse` saying the engine launches it; `SCRIPT_VERSION`; and `argProblems()`, which returns an index with `errors` and launches nothing.
  - **Agents and schemas.** `agentType` literals `review-surveyor`, `verification` and `plan-review-lead`, with inline return schemas per lead phase.
  - **Waves.** `workflow('review-v2-pipeline', …)` in try/catch, reading its `errors` and cell `status`.
  - **Bounds.** No retries, and no `model` or `effort` overrides. The script must pass `workflow_script_problems` in `tomlctl/src/cli/dispatch/tests/lint.rs`.
- **Acceptance**:
  - forward: `grep -c "name: 'plan-v2-review'" claude/workflows/plan-v2-review.js` prints `1` (today: exits 2, file absent).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes **predicted, unverified**. Falsifier: an `agentType: 'plan-review-leader'` typo turns `workflow_scripts_name_real_agents` red.

#### 29. Write the engine's plan-flow reference [M]
- **Files**: `claude/skills/review-v2-engine/references/plan-flow.md` (new), `claude/skills/review-v2-engine/SKILL.md`, `claude/skills/review-v2-engine/references/triage-and-close.md`
- **Depends on**: 11, 16, 28
- **Action**: Write `plan-flow.md`, the flow-mode procedure from Approach, "/plan-v2 and /review-plan-v2", "Re-check" and "The wrapper and the lead": run start with the scope, survey and schemas, the `Workflow({name:"plan-v2-review", args})` launch, fallback at the first stage the store shows incomplete, mop-up, triage by tier through `review apply`, user-tier batches, re-probing every `spec.accept` entry an applied finding changed, parking, the re-check, finish, and the approved-flow rule (which `review apply` also enforces).
- **Detail**: In `SKILL.md`, stage 0 step 5 skips the import on a v2 flow, stage 1's plan-family scope names both modes, and the family deltas point flow mode at `references/plan-flow.md`. In `triage-and-close.md` step 3, flow mode replaces the merge offer with `plan-flow.md`'s tiered apply. Keep `SKILL.md` under 500 lines and the reference under 600.
- **Acceptance**:
  - forward: `test -f claude/skills/review-v2-engine/references/plan-flow.md && grep -c 'plan-flow.md' claude/skills/review-v2-engine/SKILL.md` prints at least `1` (today: exits 1, no output).

#### 30. Rebind measurement and survey to v2 flows [M]
- **Files**: `claude/skills/review-v2-engine/references/measurement.md`, `claude/skills/review-v2-engine/references/survey.md`
- **Depends on**: 13
- **Backlog**: B-13ec633a
- **Action**: Make the following changes:
  - In `measurement.md`, name `review stats --by model` as the producing command for per-model overturns.
  - Rebind the plan threshold from "at the merge offer" to the user-veto rate from `review stats --plan-outcomes`.
  - Add the plan seeded-recall procedure and the engine contract note from Approach, "Outcomes".
  - In `survey.md`, lines 30, 54 and 71-77, add the flow-mode scope, the `flow:` prompt line and the profile's task list.
- **Acceptance**:
  - forward: `grep -c 'review stats --by model' claude/skills/review-v2-engine/references/measurement.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'at the merge offer' claude/skills/review-v2-engine/references/measurement.md` prints `0` (today: `1`).

#### 31. Give /review-plan-v2 a flow mode [M]
- **Files**: `claude/commands/review-plan-v2.md`
- **Depends on**: 12, 29
- **Action**: Add flow mode to the carrier.
  - **Stage 0.** When the resolved flow has `.claude/flows/<slug>/plan.toml`, bind `<flow>`, skip plan resolution and the import, and name the mode in triage. Check `tomlctl capabilities` for the `review_apply` feature first, and stop naming `cargo install --path tomlctl` when it is absent.
  - **Stages 2 to 6.** Run through `references/plan-flow.md`.
  - **Stage 7.** Present by tier.
  - **Stage 8.** Use tiered apply in place of the merge offer.
- **Detail**: Leave the markdown-mode text as it is. Keep every skill the `carrier_invokes_required_skills` row lists for `review-plan-v2.md` named in the file. This file is in `finding_classes.rs` `PROSE`, so name no `plan check` class in backticks.
- **Acceptance**:
  - forward: `grep -c 'plan-flow.md' claude/commands/review-plan-v2.md` prints at least `1` (today: `0`).

#### 32. Build review into /plan-v2 [M]
- **Files**: `claude/commands/plan-v2.md`, `claude/skills/plan-v2-engine/SKILL.md`, `claude/skills/plan-v2-engine/references/approve.md`
- **Depends on**: 12, 21, 29
- **Action**: Make the Review step real per Approach, "/plan-v2 and /review-plan-v2".
- **Detail**:
  - **`--review`.** It is accepted and forces review. In `SKILL.md` line 39 and the `plan-v2.md` Bindings row, remove the refusal.
  - **Step-table Review row.** Review runs unless `--no-review` or a low-risk judgement applies, and the step invokes the `review-v2-engine` skill and follows `references/plan-flow.md`.
  - **`approve.md` §Review.** It gets the skip table with `skipped-low-risk`, and drops the reason `plan review is not built yet`.
  - **The gate.** `review_edits` and `risks` are shown in key order, with "review now" offered when review was skipped.
  - **Approval.** `plan approve --review ran`.
  - **Pre-flight.** Extend the `plan`-group capabilities check in `plan-v2.md` to the `review_apply` feature, so a binary older than 0.19.0 stops before the wrapper launches, naming `cargo install --path tomlctl`.
- **Acceptance**:
  - forward: `cat claude/skills/plan-v2-engine/SKILL.md claude/skills/plan-v2-engine/references/approve.md | grep -c 'not built'` prints `0` (today: at least `3`).
  - forward: `grep -c 'Refused' claude/commands/plan-v2.md` prints `0` (today: `1`).

#### 33. Add and run the v2 review link script, and install tomlctl [M]
- **Files**: `scripts/link-v2-review.ps1` (new)
- **Depends on**: 7, 12, 17, 18, 19, 20, 28, 29, 36
- **Action**: Write `scripts/link-v2-review.ps1` per Approach, "Deploy", and run it with `pwsh -NoProfile -File scripts/link-v2-review.ps1`. Then run `cargo install --path tomlctl`, retrying once on `os error 5`.
- **Detail**: This task is the run's one exception to the no-full-build rule, because the install is the deliverable and its dependencies keep it off the shared target while the checkpoint gates run. Its dependencies cover every task that edits `tomlctl/src/`, so the install builds the finished tree. File symbolic links need elevation while Developer Mode is off (`docs/ideas/plan-v2-probes.md`); the script checks for it and, unelevated, creates the junctions and lists the file links it could not make, which After Merge completes. Report each link the script made, skipped or refused.
- **Acceptance**:
  - forward: `test -f scripts/link-v2-review.ps1` exits 0 (today: exits 1).
  - forward: `test -L "$HOME/.claude/skills/review-v2-engine"` exits 0 (today: exits 1), since junctions need no elevation **predicted, unverified**.
  - forward: `test -L "$HOME/.claude/workflows/plan-v2-review.js"` exits 0 (today: exits 1) **predicted, unverified** until the elevated run in After Merge.
  - forward: `tomlctl review apply --help` exits 0 (today: exits 2, unrecognised subcommand) **predicted, unverified** until the install.

#### 34. Bring CLAUDE.md and the idea doc into line [S]
- **Files**: `CLAUDE.md`, `docs/ideas/v2-pipeline.md`
- **Depends on**: 31, 32, 33
- **Action**: In `CLAUDE.md`, have "Review v2" name flow mode, `review apply`, the `plan-review-lead` agent (Opus, one-shot, no `name:`), the `plan-v2-review` Workflow and `scripts/link-v2-review.ps1` as the deploy route, and have "Planning v2" say plan review runs in `/plan-v2`.
- **Detail**: In `docs/ideas/v2-pipeline.md` §Plan review, record the departures from Approach, "What departs from the idea doc and v2-planning", and mark the built parts.
- **Acceptance**:
  - forward: `grep -c 'link-v2-review.ps1' CLAUDE.md` prints at least `1` (today: `0`).
  - forward: `grep -c 'review apply' docs/ideas/v2-pipeline.md` prints at least `1` (today: `0`).

#### 36. Point agents at plan-records.md and mark flow-mode cells [M]
- **Files**: `tomlctl/src/review/query.rs`, `tomlctl/tests/review_query.rs`, `claude/skills/tomlctl/references/review.md`
- **Depends on**: 4, 20, 22
- **Action**: Add `("plan-records", …)` to `METHOD_REFERENCES` in `tomlctl/src/review/query.rs`, and a `flow` key beside `plan_documents` in the cell assignment when the store's flow has `.claude/flows/<slug>/plan.toml`.
- **Detail**: Preloaded agents read method references from the absolute paths their `review show` projection carries (`review-v2-method/SKILL.md`), and those come only from `METHOD_REFERENCES`, which today lists `finding-kinds` and `verifying`. Without the `flow` key, finders, executors and sweepers cannot tell a flow-mode run from a markdown one. Extend the `METHOD_REFERENCES` assertion in `tomlctl/tests/review_query.rs` and add a flow-mode assignment case. Update the method-references line in `claude/skills/tomlctl/references/review.md`, within the 600-line cap.
- **Acceptance**:
  - forward: `grep -c 'plan-records' tomlctl/src/review/query.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test review_query` passes **predicted, unverified**. Falsifier: emitting `flow` on a markdown-mode store turns the new case red.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 8 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8. Record targets, tiers and validation in the review store, with markdown mode unchanged (buildable increment)

— CHECKPOINT B after tasks 12, 14, 15, 35 — dependency closure: 1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14, 15, 35. `review apply`, its write seams, capabilities, its output-options case and outcome stats (buildable increment)

— CHECKPOINT C after tasks 18, 19, 20, 21 — dependency closure: 1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21. The gate, approve and stage reading the review, the version bump and the tomlctl references

— CHECKPOINT D after tasks 23, 24, 25, 26, 30, 34 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 36. Reviewer bindings, method references, the lead, the wrapper, both carriers, deploy and docs

## Risks

- **The lead reverses the engine's orchestrator-composes rule and has never run.** Mitigation: the engine path stays as the fallback, the lead's phases are one-shot and stateless over the store, and the audit's overturn rate plus `--plan-outcomes` show whether it works.
- **Auto-applied edits change a plan without asking.** Mitigation: only tool-verified proof applies silently, and a mechanical `new` may change only what its proof covers, capped at 800 characters. Cited edits are listed on the gate, and anything touching a decision, outcome or the four intent sections is asked. On an approved flow `review apply` itself refuses any edit not passed `--user-accepted`.
- **A failure between `review apply`'s two writes.** Mitigation: each finding is stamped right after its own target write, and a re-run records an `already` edit's `applied`, so at most one edit is ever unrecorded and the next run recovers it.
- **Applying before approval leaves no audit row.** Mitigation: each finding's `applied` holds `old`, `new`, tier and actor, and `--plan-outcomes` reads it.
- **`human-only-handovers` edits the same plan files** (`approve.rs` `gate_report`, `stage.rs`, `references/plan.md`, `plan-v2-engine/references/approve.md`). Mitigation: User Decision 2. That plan is not runnable until an engine exists, and its replan reads `plan drift`.
- **Child `workflow()` behaviour is partly unstated**: a runtime throw inside a child, journal replay across parent and child, and lookup of a symlinked child by name. Mitigation: try/catch plus reading `errors[]`, a fresh launch for every re-check, and the engine fallback. The first live run in After Merge settles it.
- **The 71-file footprint.** Mitigation: 36 tasks of at most 3 files, except three whose edits are inseparable (11 at 5 files, 12 at 4, 19 at 5); four checkpoints; and a markdown milestone that needs only the CLI surface.

## After Merge

- Run `pwsh -NoProfile -File scripts/link-v2-review.ps1` from an elevated PowerShell window. Only a person can elevate, and file symbolic links need it while Developer Mode is off (`docs/ideas/plan-v2-probes.md`). When: after the run, before the restart. Done when: `test -L "$HOME/.claude/workflows/plan-v2-review.js"` exits 0 and the script reports no file link it could not make.
- Restart Claude Code. Only a person can restart the session that loads the new agents, Workflow, skills and command. When: before the next `/plan-v2` or `/review-plan-v2`. Done when: `/plan-v2 --review` on a small goal launches `plan-v2-review` and reaches `plan approve --review ran`, which settles the last success criterion.

## Exploration Notes

Checkpoint from three Explore sweeps, 2026-11-09. Paths are repo-relative. Base: `0c99589` plus the Slice 2 review-apply edits, since committed (`7429069`, `091d23f`).

### Review v2 plan family today (markdown-bound)

- `claude/commands/review-plan-v2.md`: family `plan`, `--command review-plan-v2`, envelope `--command review-plan`, base `HEAD`, scope = plan documents with `<plan>` first. Stage 0 step 2 binds `<plan>` from a file arg, a directory of markdown (classified by role), `context.toml` `plan_path`, or the newest plan; step 3 runs `tasks import-plan --slug <slug> --reconcile-record` (skipped when `<plan>` is not `plan_path`). Stage 2 runs `STORE CHECK` + `FORMAT CONTRACT` and reads `transient:` from `## Verification Commands`. Stage 3 always proposes a `plan-checks` cell (`check:two-control-probe`, `check:falsifiability`, `check:acceptance-contradiction`) and `plan-executor` cells of ≤4 task refs. Stage 8 merge offer: `review list --in <plan> --for implementer`, then v1 `review-plan.md` Step 4A (`.premerge.md`, A3.5 re-import) or 4B (`anchor_old` per `plan_section`, `.revised.md`); merged rows resolved with `review triage --resolve-file` "merged into <plan>".
- On a v2 flow it breaks today: `plan_path` is `docs/flows/<slug>/brief.md`, so `<plan>` binds to the brief; `tasks import-plan --slug human-only-handovers --dry-run` refuses ("does not apply to a flow with a plan.toml … use `plan view`"); `tasks check --plan` runs clean.
- Engine `claude/skills/review-v2-engine/`: plan-specific parts are SKILL.md stage 0 step 5 and stage 1 "the plan path", `references/compose.md` §Plan family, `references/triage-and-close.md` step 3 ("The plan family never exports"), `references/measurement.md:122`, `references/agent-waves.md:104` (surveyor prompt `plan:` line), `references/survey.md` lines 30, 54, 71-77. Launch: `Workflow({name:"review-v2-pipeline", args:{run, store:{slug|scope}, ledger_line, wave, depth, pack_path, cells:[{id, agentType}], schemas:{cell_return, verdict_return}}})`; fallback `references/agent-waves.md`.
- `claude/workflows/review-v2.js` (`meta.name` `review-v2-pipeline`): one wave, no surveyor/resolve/triage; returns `{run, wave, cells:[{id,status,findings,escalated}], verified, errors, script_version:'1'}`; `CELL_AGENTS` fixed to four, `plan-executor` among them. Never run on this machine.
- Plan finding shape (`review-v2-method/SKILL.md` §Plan family, `tomlctl/src/review/fields.rs`): `file` = plan path, `line` = heading line, `symbol` = task ref or heading slug; `plan_section` (≤200) required for defect/cleanup; `anchor_old` (≤400), `anchor_new` (≤800) optional; all `.plan_only()`. `old_required()` false for `Family::Plan` (`B-ef306556` resolved). No field targets a record. Identity is `matching::anchor` = `<family>|<file>|<symbol>`.
- `tomlctl/src/review/add.rs::text_refusals` → `check_anchor_old(root, rel, section, anchor_old)` via `section_bodies`/`heading_level` (markdown `#` headings outside fences, exactly one occurrence). `query.rs::assignment` sets `plan_documents = run.scope` for the plan family. `cells.rs::propose` reads `tasks.toml` refs/effort/checkpoint/needs (works on v2). `runs.rs::start`: `changed_lines` None for plan; scope entries checked by `path_problem`. `export.rs::refusal` refuses all plan rows. `digest.rs::location` (`§ plan_section`), `defect_entry` (Anchor/Replacement).
- Outcomes today (`stats.rs`): only exported → v1 ledger status through `v2_ref` (`outcomes()`, `v1_links`), so the plan family has none. `escapes()` matches plan slices by `symbol`. `calibration()` learns from dismissals, v1 wontfix and audit overturns. No merge/edit outcome; only `resolution` text. `triage.rs` modes Dismiss, Defer, Reopen, Park, Resolve, Backlog, Convert, each with `--expect-status`.
- Every markdown assumption (to rebind or branch): `claude/commands/review-plan-v2.md` stages 0, 2, 8; engine SKILL + `agent-waves.md`, `survey.md`, `triage-and-close.md`, `measurement.md`; agents `review-surveyor.md` (steps 2, 6, 7), `plan-executor.md` (preamble headings, `tasks show --with body,files,deps`, `review list --in <plan>`, anchor rules), `review-sweeper.md` step 8, `review-finder.md` check-cell paragraph; `review-v2-method` SKILL + `references/finding-kinds.md`; checks `store-check.md`, `format-contract.md`, `two-control-probe.md`, `falsifiability.md`, `acceptance-contradiction.md`, `break-set-baseline.md`; lenses `plan-structure.md`, `plan-executor.md`, `plan-grounding.md`, `plan-dependencies.md`, `plan-premortem.md`, `plan-risk.md`, `ripple.md:67`, `test-gaps.md:61`; `methods/dry-run.md`; tomlctl `review/add.rs`, `fields.rs`, `digest.rs`, `query.rs`; `claude/skills/tomlctl/references/review.md` lines 96, 98, 110.

### Plan group hooks (`tomlctl/src/plan/`)

- Modules: `approve` (gate + approve), `check`, `dispatch`, `drift`, `milestone`, `open`, `probes`, `records`, `schema`, `stage`, `store`, `view`, `mod`. `SCHEMA_VERSION = 1`.
- `approve.rs::approve` `--review ran` → `store::plan_review_finished(flow_dir)`: any `review-v2.toml` `[[runs]]` with `family == "plan"` and non-empty `finished`. Skips need `--reason`. `schema::ReviewOutcome` = `ran|skipped-low-risk|skipped-flag`.
- `stage.rs::review_settled`: a `[[run]]` `step="review"` with `skip_reason`, else `plan_review_finished`. Working tree's `PLAN_REVIEW` hint is `plan run skip --step review --reason <why>` (the `/review-plan-v2` alternative was removed in the Slice 2 review-apply, `B-1da78375`).
- `approve.rs::gate_report`: `"review_edits": []` hard-coded; `risks` holds only `{kind:"skip-reason", detail}`; `REVIEW_NOTE` constant; `tomlctl/tests/plan_approve.rs` (~905-970) asserts both.
- `view.rs` `VIEW_ROLES = ["human","digest","worker"]`; `--for human` is the whole plan as markdown (brief, binding, decisions, contracts, milestones, tasks with specs, cited design sections, runs, approvals, `[[changes]]`), the natural reviewer input; `digest` carries `stage_inputs.review_store: bool`.
- Records (`schema.rs`): `Decision{id, decision, why, provenance, door, binds, rejected[{option, because}], evidence, if_false, revisit_if, check, surfaced, vetoed}`; `Contract{id, kind, claim, check, tasks: Vec<u32>, provides, consumes}`; `Approval{commit, at, brief_sha256, by, review, reason}`; `Run{step, workflow, args_hash, run_id, session, started, finished, skip_reason, outcome: ok|errors|stopped|abandoned, reason}`; `Handover{id, command, source_task, done}`.
- `plan decide` / `plan contract --ndjson` (`records.rs::upsert_decision`, `upsert_contract`; `DECISION_KEYS`, `CONTRACT_KEYS`) take no `reason`/`actor` and write no history, even after approval. So "review edits that altered a decision" has no source.
- Task audit (`tasks/update.rs::record_changes`): `Change{at, ref, field, old, new, actor, reason}`; `actor` free text, default `DEFAULT_ACTOR = "orchestrator"`, so `actor = "review"` needs no code change. Audited: row `acceptance, action, checkpoint, coupling, deps_note, detail, effort, files, needs, ref, title`; spec `spec.outcome, constraints, accept, premises, means, design, precedent, executor`; `file_notes, closes, refs`. Reason only via `--ndjson` rows. Tombstones `field="*"`. Milestone rows `ref = "milestone:<id>"` with actor hard-coded (`plan/milestone.rs::change`).
- CLI wiring: `PlanOp` variant + Args (flatten `PlanTarget`, `Read`/`WriteIntegrityArgs`) in `tomlctl/src/cli/types/plan.rs`, re-export in `tomlctl/src/cli/mod.rs`, arm in `tomlctl/src/plan/dispatch.rs`; `capabilities.rs` `MUTEX_GROUPS`, `SCOPED_ENUM_VALUES` (`plan view/for_role`, `plan approve/review`).
- tomlctl 0.18.0 = installed binary = `glimpse/Cargo.lock`; glimpse links tomlctl but not `plan` (`pub(crate)`).

### Planning carrier and Workflow substrate

- `claude/commands/plan-v2.md`: `--no-review`, `--review` in argument-hint; Bindings say `--review` "Refused". `claude/skills/plan-v2-engine/SKILL.md`: `--no-review` reason fixed; `--review` refused before Open; step-table Review row "Always, until plan review is built"; resume order puts `cold-read` before Review. `references/approve.md` §Review (two-row reason table, `plan run skip --step review`), gate `review_edits` "always empty", step 5 `plan approve --review skipped-flag`, `ran` "does not apply here", amend.
- `claude/workflows/plan-v2-investigate.js` / `plan-v2-prove.js` pattern: literal `meta {name, description, whenToUse, phases}`, `SCRIPT_VERSION = '2'`, `SLUG` regex, `argProblems()` → `fail()` → `index([])`, inline `RETURN_SCHEMA` with `additionalProperties:false`, `caught()` per agent, `parallel(...)`, statuses `ok|error|no-return|no-note|mismatch`, return `{slug, notes|reads, errors, script_version}`.
- Probe results (`docs/ideas/plan-v2-probes.md` `## Results`): Lookup symlink works; `omitClaudeMd` held; T1 true; T2 stale; T3 miss.
- Idea doc `docs/ideas/v2-pipeline.md` §Plan review (lines ~121-138): `/plan-v2` launches a wrapper Workflow; a fresh `plan-review-lead` runs survey + coverage, returns cells; `review-v2-pipeline` runs as child (one nesting level); lead audits, settles contested, writes proposed edits to a file; orchestrator applies edits by tier via verbs; a second top-level run re-checks only what changed; blind finders (brief, design, tasks; not notes) with verdict-before-notes and recorded overturns; edits target a record; one review, one approval replaces merge, `.premerge.md`, `.revised.md`; exceptional targeted deep re-review only for unresolved critical on high-risk work; opt-out by low-risk judgement + "review now"; risk: lead reverses orchestrator-composes, untried.
- First real v2 plan `human-only-handovers`: stage `approved`; runs investigate/challenge/cold-read `ok`, review skipped ("plan review is not built yet"); approval `{commit 9890f47, review skipped-flag}`; gate `risks=[skip-reason]`, `review_edits=[]`; `brief.md` 36 lines, `design/handover.md` 50 lines, 12 tasks/12 specs, 4 contracts, 10 decisions (9 agent-assumed), milestones A, B; no `review-v2.toml`. Its pending tasks 1-7, 10, 11 edit `tomlctl/src/plan/{schema,records,approve,view,check,stage}.rs`, `cli/types/plan.rs`, `references/plan.md`, `plan-store.md`, lenses `plan-risk.md`/`plan-structure.md`, `plan-v2-engine/references/{write-and-prove,approve}.md` (user: Slice 3 first, on today's tree; drift/replan absorbs it).
- Deploy: `plan-v2` and the three plan agents are linked under `~/.claude`; none of `review-v2.js`, `review-plan-v2.md`, `review-v2-engine`, `review-v2-method`, `review-v2-lenses`, `plan-executor` or any `review-*` agent is linked.

### Gates

- `tomlctl/src/cli/dispatch/tests/skills.rs` `carrier_invokes_required_skills` (~1546): `review-plan-v2.md` requires `tomlctl`, `flow-contract-flow-context`, `flow-contract-plansdirectory-prompt`, `flow-contract-task-store`, `flow-contract-plan-output-format`, `review-v2-engine`, `backlog-capture`. Caps: SKILL 500 lines, references 600, description 1024 chars.
- `lint.rs`: `workflow_script_problems` (literal `meta`, no `${`/`...`; `meta.name` ≠ any command stem, so no wrapper named `review-plan-v2`; every `agentType` literal or in `*_AGENTS` and matching an agent's frontmatter `name`; no `model`/`effort`); `md_workflow_launches_name_saved_scripts`; `command_lint`, `guidance_lint`, `flag_table_lint`.
- `finding_classes.rs` `documented_plan_classes_match_the_source`, `PROSE` (has `review-plan-v2.md`, `store-check.md`); `output_gate.rs::the_command_tree_has_no_duplicate_flags`; `scripts/shared-blocks.toml` blocks `review-v2-store-writer`, `review-finder-body`, `plan-read-only`, `plan-v2-note-writer`, `research-delivering`.
- Tests: `tomlctl/tests/review_add.rs` (`plan_anchor_old_must_be_unique_in_section`), `review_query.rs` (~418-432 `plan_documents`), `review_cells.rs`, `review_stats.rs`, `review_triage.rs`, `review_export.rs`, `review_finish.rs`, `review_digest.rs`, `review_catalogue.rs`, `review_vocabulary.rs`, `plan_stage.rs` (~67), `plan_approve.rs`.
- Commands: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`; `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`; `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`; `cargo test --manifest-path tomlctl/Cargo.toml --test <name>`; `bash scripts/verify-shared-blocks.sh`; `cargo clippy --manifest-path glimpse/Cargo.toml --locked`.

### Backlog

Live rows under the touched directories, all `open`, none promoted: `B-13ec633a` (measurement.md per-agent-model audit overturn rates have no producing command; adjacent to outcome logging). Context only: `B-e4c1a22a`, `B-8414a070`, `B-c6fcfb97`, `B-e922f5cb`, `B-771b57e7`. Resolved and relevant as history: `B-f4e7c986`, `B-23c14a52`, `B-9411e167`, `B-1ec5ebb6`, `B-1da78375`.

### Scope flag

Roughly 35-45 files. User chose one plan over a split (Phase 1).

## Research Notes

Two lenses, 2026-11-09: `workflow-runtime` (`research-lite`) and `edit-tiers-and-outcomes` (`research-deep`). No dependency is added or upgraded.

**Vet outcome** (no ledger exists in plan mode, so this stands in for `[[vet_events]]`):

| Lens | Sampled | Dropped | Downgraded | Note |
|---|---|---|---|---|
| workflow-runtime | 3 | 0 | 0 | `~/.claude/workflows/` holds only the two plan-v2 symlinks; `claude --version` 2.1.319; probe Results lines 468-472 confirmed. Its `ESCALATE-TO-DEEP` named design calls (wrapper failure path, parent/child resume), which are this plan's Phase 6 work, so it was not re-dispatched |
| edit-tiers-and-outcomes | 4 | 0 | 0 | `tomlctl/src/tasks/update.rs` `if store.audit { record_changes(...) }`, `fn keep_probes`, `plan/records.rs` `DECISION_KEYS`/`CONTRACT_KEYS` without `reason`, `review/stats.rs` `escapes` matching by `symbol`, all confirmed. External acceptance rates not re-fetched; used as calibration only |

### workflow-runtime

Searched: bundled `workflow-authoring` skill (Claude Code 2.1.319), https://code.claude.com/docs/en/workflows.md and https://code.claude.com/docs/en/agent-sdk/typescript.md, all 2026-11-09; repo `claude/workflows/*.js`, `docs/ideas/plan-v2-probes.md`.

- **Child call.** `workflow(nameOrRef: string | {scriptPath}, args?) → Promise<any>` returns the child script's return value; `args` becomes the child's `args`. The child shares the parent's concurrency cap, agent counter, abort signal and token budget; nesting is one level (a `workflow()` inside a child throws). The public docs do not mention `workflow()`; source is the bundled skill (high). Impact: the wrapper calls `workflow('review-v2-pipeline', {...})` per wave.
- **Failure.** `workflow()` throws on an unknown name, an unreadable path or a child syntax error; `review-v2.js` returns its own failures in `errors[]` and per-cell `status`. A runtime throw inside a child is unstated (needs a live probe). Impact: wrap the call in try/catch and read `errors[]`/`status` too.
- **No fallback inside a script.** The engine's Agent-wave fallback is orchestrator-run; a script cannot run it and subagents cannot launch Workflows. Impact: the wrapper returns its failure and the orchestrator falls back to the existing engine path.
- **Deploy.** `review-v2.js` is not linked into `~/.claude/workflows/`, so a child call by name throws today. Lookup through a symlink is recorded for top-level launches only (`Lookup: symlink`); child lookup is stated as the same registry but unprobed.
- **Resume.** Replay walks agents in start order; cached results hold until the first changed prompt, after which every agent reruns (T3 `miss`); an unchanged prompt over a changed file returns stale (T2 `stale`). Whether a child's agents journal under the parent run is unstated. Impact: the re-check run must be a fresh launch with a changed work-list, never a `resumeFromRunId` relaunch.
- **`agent()` options.** `{label, phase, schema, model, effort, isolation:'worktree', agentType}` per call; `agentType` and `schema` combine; output failing the schema after 5 tries fails the call. `omitClaudeMd` is frontmatter-only and held inside a Workflow (`omitClaudeMd: held`); nested CLAUDE.md under the read tree still loaded for 4 of 6 replay challengers.
- **Limits.** 16 concurrent agents by default, 1,000 agents per run with the child counted, 4,096 items per `parallel`/`pipeline`, advisory warning above 25 agents; no documented script or `args` size limit. `args` must be real JSON; no `Date.now()`, `Math.random()`, I/O or `import()`. Impact: pass a store pointer and cell ids, as `review-v2.js` already does.

### edit-tiers-and-outcomes

Searched: RFC 6901/6902 (rfc-editor.org), Gerrit REST `FixSuggestionInfo`, Kubernetes `kubectl patch` / `patchMergeKey` docs, Bosu et al. MSR 2015 via arXiv 1807.04485, Atlassian RovoDev arXiv 2601.01129 v2, BitsAI-CR arXiv 2501.15134 §3.4, CodeRabbit arXiv 2607.03316 v2, Google Research blog 2023-05-23, all fetched 2026-11-09; no plan-review effectiveness study found in two searches.

- **Target grammar keyed by record** (high for the verb mapping, medium as design). `task:<ref>.<field>`, `task:<ref>.spec.<field>[n].<sub>`, `decision:D<n>.<key>`, `contract:C<n>.<key>`, `milestone:<id>.<field>`, `handover:H<n>.<field>`, `brief#<Section>`, `design/<file>.md#<anchor>`. The part before `[` equals the `[[changes]].field` name, so reverts join on (ref, field). Finding identity: `file` = `tasks.toml`/`plan.toml`/the prose path, `symbol` = ref / `D<n>` / `C<n>` / `milestone:<id>`, so `matching::anchor` and `escapes()` work unchanged. Keyed lists merge by key, not position (Kubernetes `patchMergeKey`), which rules out JSON Pointer. Validation reuses `add.rs::section_bodies` for brief sections and `plan/check.rs` `design_section` for design anchors.
- **Compare-and-set.** Each edit carries `expect` (current value; whole entry for an array element) and `new`; `[n]` is a hint, the entry equal to `expect` is the match, refusing 0 or >1 (as `check_anchor_old` and `keep_probes` already behave). Checked at `review add` and again inside the target verb's lock; stale edits reported `skipped_stale`, as `review triage --expect-status` does. `tasks update --ndjson`, `plan decide`, `plan contract` have no `expect` today. RFC 6902 `test` + all-or-nothing is the prior art.
- **Review edits leave no audit trail.** Review runs before approval while `audit = false`, so `tasks update` writes no `[[changes]]` (`tomlctl/src/tasks/update.rs` `if store.audit`), and `plan decide`/`plan contract` never write history. Impact: the review store records each applied edit on its finding (`applied = {target, old, new, at, tier}`); `resolution` text cannot be joined on.
- **Tier computed, never asserted.** Mechanical proof kinds tomlctl can check without running commands: `probe` (target `accept` entry's stored verdict not `healthy`), `check-class` (a `plan check` error class on the target), `path-missing`, `ref-unknown`. Cited: a `path:line` that `query.rs::parse_citation` resolves within the file's line count. Advisory: everything else, including verifier agreement alone (decision 6). Premise and other command exits cap at cited, since tomlctl never runs a stored command. A mechanical `accept` fix drops its probe (`keep_probes`), so `plan stage` returns to `prove` and the re-probe must be `healthy` before approval. Precedent: `route` is a computed field set at `review add`.
- **User tier overrides evidence** (medium; inferred from decision 6's wording). Always user: `decision:*`, `task:*.spec.outcome`, `brief#Intent`, `brief#Non-goals`, `brief#Scope budget`, `brief#Success criteria`. Candidate extensions: `contract:*.claim`, `milestone:*.goal`, `spec.accept[].behaviour`. Decision edits reuse the existing gate (`surfaced = true` makes `plan approve` refuse until answered); other user-tier findings block `plan approve` while open.
- **Outcome record.** Per finding: `tier`, `target`, disposition (`applied`, `applied-disclosed`, `advisory` open/dismissed/converted, `user-accepted`, `user-vetoed`), `applied`. Per plan: nothing new (`[[approval]].review` + the plan-family run). Read: tier × disposition, user-veto rate, growth from edits (the 48% problem), re-check yield (findings the second run mints on edited targets), revert rate (post-approval `[[changes]]` whose `new` equals `applied.old`), cost. Industry proxy "applied and not reverted" ≈ "triggered a change in the next commits" (Bosu; RovoDev 38.7%; BitsAI-CR "Outdated Rate", not causal; CodeRabbit 36.4% accepted; Google 40-50% applied); calibration only.
- **Execution joins cannot exist yet.** No v2 engine, so no deviations, reverts or post-approval changes on a v2 plan. Measure now with a seeded-defect recall probe on a copied plan (vacuous `accept`, dead `design` anchor, wrong `files` path, overstated `outcome`, false-evidence decision, each with an expected tier) and real-run tier mix, veto rate, audit overturn, re-check yield and cost. `measurement.md:122`'s plan threshold reads "dismissed at the merge offer", which v2 flows drop.
- **Deviations are not escapes without a cause.** Count only a deviation with a `plan-defect` cause, which the engine's deviation writer would have to stamp; report reviewed and skipped plans side by side, never as an effect size. Impact: a contract note for the engine plan; label any escape column "candidate".
