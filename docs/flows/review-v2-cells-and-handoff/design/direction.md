# Direction

## Overarching

The orchestrator should be able to launch `review plan --propose`'s map as it comes, and every finding the user picks at triage should reach `/apply-v2` and land consistently with the other fixes. The plan builds that in tomlctl first and lets the engine prose follow the verbs, in parallel tracks over one milestone.

What it optimises for:

- **Deterministic proposals over orchestrator judgement.** A rule the orchestrator applied by hand in runs U3 and U5 belongs in `--propose`: slicing by subsystem and file kind, security on the trust-boundary evidence, test-gaps on changed code and its tests, the sweeper pool, repeats last, and the context-free cells.
- **Catalogue-driven, not lens-id-driven.** A new slicing behaviour is a `slice` rule, a lens's `kinds` key or a `[routing]` key in the catalogue, read generically by `cells/`. No `if lens.id == "security"` in Rust.
- **Refuse, then let the user decide.** The ceiling is enforced where cells are written. tomlctl never chooses which cell to cut: it refuses the whole wave and names the numbers. Only the attended orchestrator, after asking the user, passes an override, and the override is stored on the run.
- **Measure before retuning.** No ceiling, cap or allowance number changes here. `review stats` gains what is needed to tune them later: audit classes and first-pass, repeat and context-free yield.

What it gives up:

- The orchestrator's freedom to reshape a map silently. Its edits stay possible, but cells past the ceiling are refused.
- Cells at quick for code and perf escalations. Those escalations go to triage instead.

Expectations every task keeps:

- Behaviour that a run, a store or a test shows. A unit of slicing or admission is proven by a `--propose` output, not by reading the code.
- No store schema break. New fields are additive, written only by their verb, and old stores read as before.
- Prose follows verbs. Each engine, agent or skill text names the flags and outputs exactly as the code ships them.
- Comment discipline per `.claude/rules/documentation.md`: no finding ids, task refs or plan names in source.

## Milestone A

One milestone, run as parallel tracks once the foundation lands.

- **Foundation.** Split `tomlctl/src/review/cells.rs` into `cells/` with no behaviour change. Move the `review_cells` test harness into a shared test module and register the new test modules. The proposal, ceiling and escalation tracks build on these files; the multi-file cleanup, schedule and stats tracks start from the store-field and catalogue tasks instead.
- **Proposal.** Catalogue routing kinds and slice rules, then kind-aware slicing, then phased admission (judgement and context-free cells, then the mechanical pool, then repeats). The U3 and U5 replays assert the success criteria as properties against the live catalogue.
- **Ceiling.** An all-or-nothing count on append, `--over-ceiling-file`, and the run's `ceiling_overrides`, shown in the report and in stats.
- **Escalations.** `review escalation record` stores a wave's escalations on their cells, as the Workflow index carries them (cut to 300 characters). `review escalation fix <cell>:<n>` mints a finding blocked on evidence and routes it to one verifier.
- **Multi-file cleanup.** `fix_files` joins the edit bound for the checker, the verifier and the audit, and the route.
- **Schedule.** A prose fix that names another fix's executable path needs that fix's task.
- **Stats.** Per-class audit counts, then `--yield`.
- **Prose and features.** The engine, agent and skill texts and the `FEATURES` list follow the code. `cargo install --path tomlctl` is the last gate.

A task in this milestone reads its track's bullet as the scope it serves. A task that finds its track needs something from another track's files returns `failed` and says what, rather than editing outside its files.
