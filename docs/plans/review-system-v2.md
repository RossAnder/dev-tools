# Plan: Review System v2 — composed coverage, kind-routed findings, Haiku-first execution

**Plan path**: `docs/plans/review-system-v2.md`
**Created**: 2026-10-09

## Summary

Build a second review system next to the untouched v1 (`/review`, `/optimise`, `/review-plan`): three thin carriers (`/review-v2`, `/optimise-v2`, `/review-plan-v2`) over one engine skill, one agent method skill and one pluggable lens catalogue, backed by a new `tomlctl review` verb group and a typed per-flow store. The design rests on six decisions:
- **Findings are classified by kind** — `defect` / `cleanup` / `advisory`. Cleanup (small, concrete, verifiable fixes, including add-shaped ones such as a missing test or doc) is uncapped. Design fixes that need only edits are cleanup; findings whose adoption needs decisions are `advisory`, parked in the v2 store and sent to the backlog only when the user picks them.
- **Coverage is proposed, composed and accounted for.** Deterministic detectors profile the change and the project; `tomlctl` proposes a coverage map of cells (a slice of the scope × a bundle of up to three concerns); the orchestrator edits it, adds probes and records a waiver for every matched concern or scope file it leaves out.
- **Work is pushed down a ladder: deterministic tomlctl → Haiku → Opus → orchestrator.** Haiku does only what grep can settle (mechanical sweeps, mechanical checks) and proposes, never decides, judged signals. Everything judged — defects, behaviour-changing cleanup, unsettled cleanup, plan checks — goes to fresh-context Opus. The orchestrator composes, adjudicates and triages, reading digests and role projections only.
- **The store is robust to agents writing it directly.** Writes are idempotent per cell, verification is routed from the store, identity is resolved after each wave, and a run cannot finish with unverified rows or unaccounted coverage.
- **Outcomes, not opinions, tune the system.** v1 apply outcomes are read back through `v2_ref`, `review stats` reports outcomes and coverage escapes, and calibration learns only from user dismissals and audited refutations.
- **The lens catalogue is pluggable and shared across projects.** Lenses, facets, methods and checks are markdown files with TOML frontmatter in one global catalogue; signals carry the directories they matched so facets attach only where a stack lives; a project needs no setup, with an optional `config.toml` for opt-in/out, path-scoped variation and project checks. `tomlctl review catalogue` generates the index and a gate validates it.

Scrutinise first: Approach, "Finding kinds"; Approach, "Run pipeline" (proposal, waivers, verification routing, audit); Approach, "v2 store and the tomlctl review group" (cell sealing, `verdict` and `resolve`, `sync-v1`); Approach, "Lens catalogue" (activation tiers, path scoping, detectors); and Approach, "Measurement and tuning".

## Context

The v1 review commands date from April–May 2026 and have only been patched since. Their lens sets are fixed per command and broadcast to the whole scope regardless of what changed, so coverage tracks the template rather than the change: there is no logic-bug owner, no way to aim a lens at a risky subsystem, and no lens that responds to the state of the project (a test suite growing faster than the code, docs drifting from behaviour). The empirical record (Exploration Notes) shows the `suggestion`-level output (48–65% per lens) catches many small details plans and implementations miss and is worth keeping; the noise comes from speculative, design-intent and confirmation items mixed into the same bucket, and from orchestrator context spent re-typing findings and restating constraints (Exploration Notes, "Orchestrator context and Haiku hand-off"). Nothing measures outcomes: `[[vet_events]]` are written and never read, and v1's dispositions measure apply throughput, not precision (R4).

The user wants v1's API and harness files kept intact and v2 built as all-new files with its own tooling surface, then trialled in use rather than compared closely, with the lens catalogue portable across their Rust, TypeScript/React/Vue and .NET repos. A five-lens design critique on 2026-10-12 (Research Notes, "Design critique") shaped the current revision.

Outcome: `/review-v2`, `/optimise-v2` and `/review-plan-v2` run end to end on this repo and on the user's other repos without setup, compose and account for their coverage per run, persist verified findings of all three kinds in a v2 store written by the agents themselves, apply defect and cleanup findings through the existing apply flows after export, and learn from what was applied.

## Scope

- **In scope**:
  - the `tomlctl review` verb group and v2 store (Rust, with tests and docs), including deterministic signal detection (`review survey`), coverage proposal and accounting, identity resolution (`review resolve`) and v1 read-back (`review sync-v1`)
  - the engine skill (`review-v2-engine`) and the agent method skill (`review-v2-method`)
  - the lens catalogue skill (`review-v2-lenses`): vocabulary, 31 lenses, 10 facets, 6 methods, 9 checks, the `tomlctl review catalogue` verb, its validation gate (including `command_lint` coverage of catalogue files), and this repo's project config
  - six new agents: Haiku `review-surveyor`, `review-sweeper`, `review-checker`; Opus `review-finder`, `review-verifier`, `plan-executor`
  - the saved Workflow script and its deploy entry
  - the three v2 carriers
  - gate-table rows, shared-block registration, glimpse agent attribution for the v2 store, the run-artifact ignore rule, and CLAUDE.md
- **Out of scope**:
  - any edit to v1 carriers, agents or skills (`research-methods`, `flow-contract-vet-research`, `flow-contract-ledger-schema`, the apply contracts, `backlog-capture`, `flow-contract-task-store`)
  - A/B tooling: `review compare`, v1-ledger alignment, the blinded-sample protocol
  - a native v2 apply flow (follow-on; User Decisions 11)
  - Haiku clerks for v1 flows (`/implement` checkpoint clerk, apply closing clerk; Exploration Notes) — a separate plan
  - edits to lumina and the lumina-story-blocks plugin (where lumina's research lenses overlap the catalogue, the catalogue is written so lumina could adopt it later); glimpse display or triage of the v2 store; the user-inputs store for v2
  - the full replay recall-eval harness (R11); retiring v1
- **Affected areas**: `tomlctl/src/review/`, `tomlctl/src/lib.rs`, `tomlctl/src/cli/` (including `tomlctl/src/cli/dispatch/tests/catalogue.rs` and the `lint.rs` walk), `tomlctl/src/capabilities.rs`, `tomlctl/src/agents/correlate.rs`, `tomlctl/tests/`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `claude/skills/review-v2-engine/`, `claude/skills/review-v2-method/`, `claude/skills/review-v2-lenses/`, `claude/skills/tomlctl/`, `claude/agents/`, `claude/commands/`, `claude/workflows/`, `.claude/review-lenses/`, `scripts/shared-blocks.toml`, `scripts/deploy-claude.ps1`, `.githooks/pre-commit`, `.gitignore`, `CLAUDE.md`

## User Decisions

> User answers are recorded as data. Decisions 1–7 are from the 2026-10-09 planning session (the subset still standing); 9–21 are from the 2026-10-12 rewrite and lens sessions; 22–25 are from the 2026-10-12 design-critique session.

1. **Staging** — v1 carriers, agents and skills stay byte-untouched; v2 is all-new files plus a v2 tooling surface. lumina is out of scope. User's words: "let's try to retain the old api and harness files creating all new files and a v2 review surface in the tooling".
2. **Substrate** — Workflow with an Agent-tool fallback. Refined 2026-10-12: agents persist their own output, so the Workflow return is an index, not the findings.
3. **Verification** — a fresh-context, refute-first verifier citing `file:line` or command output for every verdict; `deep` adds a 3-vote panel for criticals. Refined 2026-10-12: verification is routed per kind and per edit target (Approach, "Finding kinds").
4. **v2 naming** — parallel `-v2` names over one shared engine.
5. **v2 store** — a new `tomlctl review` verb group over one store per flow, plus `review export-v1` into the existing ledgers for the untouched apply flows.
6. **Checkpoint cadence** — milestones.
7. **Backlog fold-in** — none.
8. *(superseded 2026-10-12)* The fixed `/review` lens re-partition is replaced by composed coverage (decision 15).
9. **Keep small findings** — do not reduce what v1 calls `suggestion`. User: "a plan and implementation miss many small details along the way and the current suggestions category catches many of these."
10. **Finding kinds** — `defect` / `cleanup` / `advisory`. Theoretical, contradictory, exploratory, probing and counter-implementation items (swapping a dependency, overarching architecture changes) are `advisory`. Tests and docs are **lenses**, not kinds; project-health signals can activate maintenance lenses.
11. **Advisory routing** — refined by decision 22. **Apply path** — the user did not choose between `export-v1` and a native v2 apply; this plan takes `export-v1` and leaves a native v2 apply as a follow-on.
12. **A/B downplayed** — the user will trial the v2 flows rather than compare them closely. `review compare` and the A/B protocol are dropped; run records stay because tuning feeds on them.
13. **Haiku first-class** — the orchestrator is kept for judgement, analysis and direction as much as possible. Refined by decision 23.
14. **Direct store writes with per-role projection** — User: "more information is good as long as good details are not crowded out. We carefully consider which fields are read by which role such that key details for the work are not overlooked and required context is not crowded out."
15. **Dynamic lens selection with ad hoc probes** — the user's main concern with v1 is that lenses are static and not selected to give the coverage required. The orchestrator composes each run's coverage and may author probes. Refined by decision 24.
16. **Lens catalogue format** — entries are files with TOML frontmatter; the index is generated by `tomlctl review catalogue`.
17. **Granularity** — split v1's overloaded lenses into finer concerns.
18. **Families** — one lens may span families, with a probe section per family.
19. **Facets and checks are first-class** — effectiveness across repos with easy repo-specific variation.
20. **Layers** — the catalogue is primarily shared across projects, with minimal opt-in/opt-out configuration per project. User: "I want the harness to be as portable across projects as possible without excessive setup in each."
21. **lumina** — kept in scope where it overlaps the catalogue, but not targeted specifically.
22. **Advisory lands in the v2 store** (critique session) — advisory findings are parked in the v2 store, visible in later runs' `PRIOR` and in `review list --kind advisory`; only items the user picks at triage go to `.claude/backlog.toml`. This respects the unchanged `backlog-capture` minting bar, which disallows hunches, unargued directions and items another store owns.
23. **Haiku narrowed, detectors deterministic** (critique session) — Haiku sweeps, runs grep-decidable checks (CONFIRMED or `unsettled` → Opus) and proposes judged signals as candidates; stack, diff and health detection moves into a deterministic `tomlctl review survey`.
24. **Bundles, proposal and waivers** (critique session) — cells carry up to three same-mode concerns; `tomlctl` proposes a default coverage map that the orchestrator edits with reasons; the digest lists unselected matched concerns and uncovered scope files, each needing a waiver; activation tiers replace `default = on/off`; the cell ceiling scales with diff size.
25. **Contrarian and synthesis gated** (critique session) — `contrarian` is proposed when its signals match (public-API change, dependency change, new seam) and always at `deep`; `synthesis` is opt-in until stats show yield. Either can be forced with `--lens +id`.

## Approach

### Design principles

1. **Push work down the ladder.** Every step goes to the cheapest executor that can do it reliably: deterministic `tomlctl` (detection, proposal, validation, identity, routing, accounting, counts, projections, reports) → Haiku (sweeps and checks that grep can settle; proposing judged signals) → Opus (judgement findings, refute-first verification, unsettled cleanup, plan checks and dry runs) → the orchestrator (composition edits, adjudication, triage, direction). Haiku never decides a question grep cannot settle, and never overrules an Opus finding.
2. **The orchestrator reads digests, never rows.** It never re-types a finding, never pastes constraints agents already carry, and reads the store only through `review digest` and role projections. Its context is spent on decisions.
3. **Coverage is proposed, composed and accounted for.** `tomlctl` proposes a coverage map from signals and the catalogue; the orchestrator edits it and adds probes; every matched concern and scope file is either in a cell or carries a waiver, and every cell reports covered, thin or uncovered.
4. **Classify by kind, route by consequence.** What a finding *is* decides how it is capped, applied and routed; what its edit *touches* decides who verifies it (Approach, "Finding kinds"). Severity is a property of defects only.
5. **Breadth in cleanup, discipline in claims.** Small concrete fixes are uncapped; defects state a failure; advisory states rationale and adoption cost. Confirmations are never findings.
6. **Each role sees what it needs.** Field readership is designed, enforced by tomlctl projections, and length-capped; Haiku agents run without CLAUDE.md and carry only their own procedure (Approach, "Field readership", "Agents").
7. **Outcomes tune the system.** Calibration and lens tuning read apply outcomes, user dismissals, audited refutations and coverage escapes — never raw reviewer opinions alone (Approach, "Measurement and tuning").
8. **v1 stays byte-identical.** v2 reuses unchanged v1 skills by invocation (`flow-contract-flow-context`, `backlog-capture`, `flow-contract-task-store`, `flow-contract-plan-output-format`, `flow-contract-plansdirectory-prompt`, `research-methods`) and never edits them. The `tomlctl` skill is the one exception: it documents the tooling surface, so it gains the `review` reference and rows additively (task 21) and stays outside the byte-identity guard.

### Architecture

```
/review-v2 ─────┐                                   ┌─ review-v2-lenses  (catalogue: lenses, facets, methods, checks, vocabulary)
/optimise-v2 ───┼─ review-v2-engine (orchestrator) ─┼─ review-v2-method  (agent side: shared kinds, store idiom, returns)
/review-plan-v2 ┘          │                        └─ research-methods  (unchanged v1: sources, grades)
                           │
   sync-v1 → run start → SURVEY: `review survey` (detectors) + Haiku review-surveyor (checks, pack, judged candidates)
   → COMPOSE: `review plan --propose` → orchestrator edits, probes, waivers → `review plan`
   → WAVE (Workflow `review-v2-pipeline` | Agent-wave fallback)
         cell → review-finder (Opus) | review-sweeper (Haiku) | plan-executor (Opus)
              → `review add` (cell-scoped) + `review cover` (seals the cell)
         routes from `add`: defect / code-touching cleanup / unsettled → review-verifier (Opus)
                            other cleanup → review-checker (Haiku)        advisory → —
              → `review verdict`
   → `review resolve` → DIGEST → follow-up or mop-up wave? → audit + triage → export-v1 / backlog picks / run finish / report
                           │
   .claude/flows/<slug>/review-v2.toml  or  .claude/review-v2/<scope>.toml
   run artifacts: .claude/review-v2-runs/<store-key>/U<n>/  (git-ignored: profile.md, pack.md, proposal, baselines, report)
```

### Finding kinds

| Kind | What it is | Required beyond the common fields | Verified by | Count cap per cell | Destination |
|---|---|---|---|---|---|
| `defect` | Something is wrong for a nameable input or state: wrong output, crash, exploit, broken contract, **missing handling**, a plan step that fails, a measured or complexity-argued perf cost | `severity` (critical \| major \| minor), `failure_scenario`, `fix`, `fix_shape` (edits \| needs-plan), `effort`, `counter`, `grade`, `evidence` (≥1) | `review-verifier` (Opus), refute-first; 3-vote panel for criticals at `deep` | 10 (ceiling, no target) | apply via `export-v1` (`fix_shape = edits`); `needs-plan` → the Needs-a-plan group |
| `cleanup` | Behaviour is right but the code, test or prose around it is not, and adopting the fix needs only edits: stale or wrong comment, dead code, naming drift, inconsistency with the converged pattern, a missing test, doc or help text, a small design fix (extract a duplicated helper, use the repo layer), a low-value test or doc to remove | `edit`, `direction` (add \| remove \| change), `effort`; `old` (the exact text at the anchor, ≤400 chars) for `change` and `remove`, and for `add` the adjacent anchor text unless `target_file` is a new file; optional `target_file` when the edit lands outside `file` | **by edit target**: `review-checker` (Haiku) for edits to comments, docs, tests (additions), config and grep-provable dead code; `review-verifier` (Opus) for any edit to executable non-test code, any test removal, and any row the checker leaves `unsettled` | none — one pattern across many sites is one row with `instances` | apply via `export-v1` (as v1 `suggestion`) |
| `advisory` | Needs a decision to adopt: theoretical risk, a challenge to a documented decision, an exploratory probe result, a counter-implementation (swap a dependency, re-architect, change direction) | `angle` (risk \| challenge \| probe \| alternative \| direction), `rationale`, `adoption_cost`; `challenges` when `angle = challenge`; optional `alternative` | none true/false — the orchestrator and the user judge it | 3 | parked in the v2 store; the user may pick it for the backlog, convert it, or dismiss it |

Rules:
- **Never a finding**: a confirmation ("checked X, it is fine"), anything the `LINT BASELINE` reports, a restatement, or an item already in the store as dismissed, deferred, parked or refuted (cite it via `same_as`). Clean checks go in the cell's coverage note. The kind-specific required fields block confirmations mechanically.
- **A documented deliberate choice is not dropped**: a finding against it is `advisory` with `angle = challenge` and `challenges` naming the decision (ADR, comment, CLAUDE.md line).
- **Defect, cleanup or advisory**: wrong for a nameable input or state → defect (missing handling included); right behaviour, wrong surroundings, edits only → cleanup (small design fixes included); adoption needs decisions → advisory. A defect whose fix needs decisions keeps its kind with `fix_shape = needs-plan`.
- **Routing is computed by tomlctl** at `review add` from the kind, `direction`, `severity`, the run's `depth`, `file`/`target_file` and the vocabulary's `[routing]` globs (executable sources, test paths); a critical defect at `deep` routes to `panel`. The agent never chooses its own verifier.
- **`effort`** (defect and cleanup): `trivial` \| `small` \| `medium` \| `large`. `export-v1` refuses `large`.
- **Paths**: `file`, `target_file`, `instances` and `citation` paths are repo-relative, `/`-separated, with no `..` and no absolute part; `review add` and `review verdict` refuse anything else, so a catalogue file is never an anchor.
- **Severity rubric (defects)**, aligned with lumina's severity names: `critical` — data loss, a security hole, shipping broken, or a plan that cannot be executed as written; `major` — a real defect with a nameable trigger; `minor` — a real defect with a narrow or unlikely trigger. Each lens adds a calibration line. A verifier may reassess severity in its verdict.
- **`failure_scenario`** by family: code — inputs or state → wrong output, crash or exploit; perf — workload and input size → cost with magnitude; plan — the execution step → how it fails.
- **Apply-now conversion**: `review triage <F> --convert cleanup|defect` with the concrete `edit`/`fix` staged by the orchestrator mints a finding of that kind with an `orchestrator` verdict (still routed by edit target) and marks the advisory `converted`.

### Field readership

The field catalogue lives once, in `tomlctl/src/review/fields.rs`: every field's name, kind applicability, required-ness, length cap and per-role visibility. `review add` validates against it, `review schema` emits from it, and `review show --for <role>` projects through it. A unit test asserts the projection table matches this matrix.

| Field | finder (`PRIOR`) | verifier | checker | implementer | orchestrator digest | backlog | report |
|---|---|---|---|---|---|---|---|
| `id`, `kind`, `file`, `line`, `symbol`, `summary` (≤160) | R | R | R | R | R | R | R |
| `status`, dismiss/defer/park reason | R | — | — | — | R | — | R |
| `lens` (or `probe:<slug>`), `cell` | — | R | — | — | R | R | R |
| `severity` | — | R (may reassess) | — | R | R | — | R |
| `failure_scenario` (≤600) | — | **R — the claim to refute** | — | R — defines done and the regression test | contested only | — | R |
| `counter` (≤300) | — | R — first probe | — | — | — | — | — |
| `evidence` (≤5 × 300) | — | R | — | — | — | first 2 | — |
| `grade` | — | **withheld** | — | — | R | — | — |
| `fix` (≤800), `fix_shape`, defect `effort` | — | **withheld** | — | R | `fix_shape` | — | R |
| plan family: `plan_section` (≤200), `anchor_old` (≤400), `anchor_new` (≤800) | — | R | — | R | — | — | R |
| `edit` (≤800), `direction`, `old`, `target_file`, `effort`, `instances` (≤50) | — | R when routed to it | R | R | count only | — | `edit` |
| `angle`, `rationale` (≤800), `alternative`, `adoption_cost`, `challenges` | — | — | — | — | **R** (until triaged) | R | R |
| `same_as` + its target's summary and status | — | R (confirm or strike) | R | — | — | — | — |
| latest verdict + `citation` (+ reassessed `severity`) | — | — | — | citation only | R | — | R |

- The verifier never sees `grade` or `fix`, so the finder's confidence and proposed solution cannot anchor it.
- The `audit` projection is the verifier projection plus every verdict and ±5 lines around each cited `file:line`, so the orchestrator adjudicates without opening files.
- The orchestrator digest carries counts, coverage gaps, waivers due, unverified ids, contested findings, and the advisory list only until it is triaged; never `evidence`.
- The implementer gets the fix and what "done" means, never the argument that the finding is true. `export-v1` builds the v1 `description` from the implementer projection.
- Over-cap values are refused at `review add` with the field and its cap named.
- A plan-family row's `file` is the plan path, its `line` the heading line of `plan_section`, and its `symbol` the task `ref` (or the section heading's slug outside `## Tasks`). `review add` refuses an `anchor_old` that does not occur exactly once under `plan_section`.

### v2 store and the tomlctl review group

**Shape.** A bespoke verb group, not generic `items`: an `items add` always stamps a `dedup_id`, a row and its verdict could not land in one write, and `clusters`, `orphans` and `items sweep` hardcode the `items` array (Research Notes, D1). The store follows the `tomlctl/src/tasks/store.rs` idiom — typed structs converted by hand-written `from_toml`/`to_toml` (no `serde` derive, no new dependency), written through `io::mutate_doc` for lock, sidecar, auto-create and `last_updated` stamping. The seed is `io::schema_seed()` (neither `review-v2.toml` nor `.claude/review-v2/` is in `SCHEMA_SEEDED_FLOW_FILES` or `SCHEMA_SEEDED_LEDGER_DIRS`, so the `tasks/store.rs` route through `on_missing_for` would seed an empty table), honouring `--no-create`. Each write has **one writable view at a time**: new rows are minted first on the raw document through `items::items_append_minted` (`DedupId::Skip`), then the typed `Store` is parsed from that document, typed edits (supersede, status, compare-and-set as a typed status check modelled on `tomlctl/src/ledgers.rs` `write_guarded`) are applied, and the closure ends with a single `to_toml` write-back, as `tasks/store.rs` does. `from_toml` accepts a date given as a string (rows minted through JSON carry `first_seen` as one, since `convert.rs` `DATE_KEYS` does not list it) and `to_toml` writes TOML dates. The typed `Store` round-trips the root `id_high_water` table. Arrays: `runs` (prefix `U`), `cells` (prefix `C`), `findings` (prefix `F`), `verdicts` (no ids).

**Concurrency and idempotency.** Up to 16 agents write one store concurrently through short `mutate_doc` calls under the file lock (a 30 s try-lock poll), with ids minted inside the lock. Workflow restarts a stalled agent from the same prompt and replays agents on relaunch, so every agent write is idempotent:
- `review add` is **cell-scoped**: each call carries the cell's full finding set and supersedes that cell's earlier `unverified` rows (`status = superseded`); after `review cover` seals the cell, `add` is refused with the cell's existing ids, which the agent returns.
- `review verdict` is an **upsert** on (finding, `by`, `seat`), where `seat` numbers panel votes 1–3.
- The Windows sidecar-rename `os error 5` is a known transient covered by the method's retry-once rule. The failing rename is the sidecar's, after the TOML has committed, so a retry must be harmless: `review cover` on a cell already sealed with the same status and note succeeds as a no-op, and `review run update` upserts `signals` by `name`.

**Paths.** `--slug <slug>` → `.claude/flows/<slug>/review-v2.toml`; `--scope <scope>` → `.claude/review-v2/<scope>.toml`; both validated by `validate_slug`. No new artifact key (D4) and no `LedgerKind` variant (D3). Run artifacts go under `.claude/review-v2-runs/<store-key>/U<n>/`, where `<store-key>` is the slug or `scope-<scope>`; agents stage their payloads in its `payloads/` subdirectory. Run-directory creation and every `--out` write go through `io::guard_write_path`. `--base` is resolved once with `git rev-parse --verify --end-of-options <base>^{commit}` and stored as the sha, so a value starting with `-` never reaches git as an option. The **claude dir** is `CLAUDE_CONFIG_DIR` when set, else `USERPROFILE` then `HOME` joined with `.claude`, as `glimpse/src/config.rs` resolves it; it is resolved once, in `tomlctl/src/review/catalogue.rs`.

**Records.**

```toml
schema_version = 1
last_updated = 2026-10-12

[[runs]]
id = "U1"
command = "review-v2"              # review-v2 | optimise-v2 | review-plan-v2
family = "code"                    # code | perf | plan
depth = "standard"                 # quick | standard | deep
started = "2026-10-12T10:00:00Z"
finished = "2026-10-12T10:20:00Z"
base = "e23b918…"
scope = ["tomlctl/src/review/**"]
changed_lines = 412
substrate = "workflow"             # workflow | agent-waves
models = { surveyor = "haiku", sweeper = "haiku", checker = "haiku", finder = "opus", verifier = "opus", executor = "opus" }
catalogue = { builtin = "~/.claude/skills/review-v2-lenses", project = ".claude/review-lenses", tree_sha = "…", config_sha = "…" }
signals = [ { name = "lang:rust", source = "detector", roots = ["tomlctl", "lumina"] }, { name = "trust-boundary", source = "judged", accepted = true, evidence = ["tomlctl/src/io.rs"] } ]
waivers = [ { concern = "security", reason = "no input handling in scope" }, { file = "tomlctl/README.md", reason = "prose only; doc-drift covers the code it describes" } ]
waves = 2
agents = 23
counts = { cells = 9, uncovered = 0, defect = 6, cleanup = 23, advisory = 4, refuted = 5, suppressed = 2 }   # computed by `run finish`
audit = { sampled = 7, overturned = 1 }

[[cells]]
id = "C1"
run = "U1"
wave = 1
pass = 1                           # 2 for a repeat pass (files listed in reverse order)
slice = ["tomlctl/src/review/add.rs", "tomlctl/src/review/verdict.rs"]   # paths, globs, or plan task refs
concerns = ["correctness", "type-design"]   # 1–3 catalogue lens ids of one mode, or one probe:<slug> / check:<id>
question = "…"                     # required for a probe; an optional focus note otherwise
agent = "review-finder"            # review-finder | review-sweeper | plan-executor
reason = "identity outcomes are the store's riskiest logic"   # "proposed" for an unedited proposal row
bound = [ { concern = "correctness", hash = "9c1e…", path = "…/lenses/correctness.md", methods = ["trace"], v1_category = "quality" } ]
facets = ["rust"]                  # attached where the slice meets the facet's signal roots, then +/- edits
status = "covered"                 # assigned | covered | thin | uncovered
sealed = true                      # set by `review cover`
note = "…"                         # the agent's coverage line (≤600)

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "defect"                    # defect | cleanup | advisory
lens = "correctness"               # one of the cell's concerns
route = "verifier"                 # verifier | panel | checker | none — computed by `add`
file = "tomlctl/src/review/verdict.rs"
line = 88
symbol = "verdict::apply"
summary = "…"
# kind fields per Approach, "Finding kinds"; plan family adds plan_section, anchor_old, anchor_new
same_as = ""
depends_on = []
related = []
anchor = "code|tomlctl/src/review/verdict.rs|verdict::apply"   # computed by tomlctl
outcome = ""                       # set by `review resolve`: repeat | contested | reopened | regression | suppressed
status = "open"   # unverified | superseded | open | refuted | merged | parked | dismissed | deferred | backlogged | converted | resolved | exported
first_seen = 2026-10-12
# companions: merged→merged_into; parked→park_reason; dismissed→dismiss_reason; deferred→defer_reason+defer_trigger;
#             backlogged→backlog_ref; converted→converted_to; resolved→resolution+resolved; exported→exported_as+export_ledger

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"                    # verifier | panel | checker | audit | orchestrator | sync
seat = 1
verdict = "confirmed"              # confirmed | plausible | refuted | unsettled (checker only)
severity = "major"                 # optional reassessment
citation = "tomlctl/src/review/verdict.rs:88"
rationale = "…"                    # ≤600
```

**Verbs.** Flag tables go in `claude/skills/tomlctl/references/review.md`. Every verb except `schema` and `catalogue` takes exactly one of `--slug` or `--scope`; `stats` also accepts `--all`. No verb declares a flag that collides with a global output flag (`the_command_tree_has_no_duplicate_flags`).
- `review sync-v1`: reads every v1 row carrying a `v2_ref` into this store — from every distinct `export_ledger` recorded on the store's exported findings, plus the family's default ledger for the store's slug or scope — and maps its status onto the v2 finding with a `by = "sync"` verdict (`verdict = "confirmed"`, `citation = "<ledger path>#<v1 id>"`, `rationale` the v1 status and resolution) — `fixed`/`applied` → `resolved`, `wontfix`/`wontapply` → `dismissed` (reason `v1: <status>: <resolution>`), `deferred` → `deferred`, `verified-clean` → `dismissed`. Idempotent; run at stage 0. `stats --outcomes` applies the same mapping read-only and writes nothing.
- `review run start --command --family --depth --base [--builtin <dir>] [--project <dir>] [--set-json scope=… --set-json models=… --set substrate=…]`: mints `U<n>`, stamps `started`, resolves `--base` to a sha, computes `changed_lines` (Approach, "Coverage proposal"), records the catalogue roots (`--builtin` defaults to `<claude dir>/skills/review-v2-lenses`, recorded as an absolute path), the catalogue tree sha and the project config's sha, validates the merged catalogue (halting with the offending file on a project-layer error), and creates and prints the run directory.
- `review run update <U> --set-json signals=…|waivers=…|audit=…|agents=…|baselines=…`: patches a whitelist of run fields; `signals` are upserted by `name` and may only add `judged` entries or set their `accepted`; `baselines` maps each baseline check id to its verification log path or `none — <reason>`.
- `review survey --run <U>`: evaluates every vocabulary detector of kind `stack`, `diff` or `health` deterministically — globs over `git ls-files`, optional content regexes, manifest keys (including `Directory.Packages.props` central package management), per-stack count patterns over `git log` windows, thresholds — and records each true signal with the directories it matched (`roots`). Prints the signals.
- `review plan --run <U> --propose [--accept-signals a,b] [--lens +id|-id …] [--out <path>]`: emits a default coverage map as NDJSON per Approach, "Coverage proposal", with every proposed row's `reason = "proposed"` and an estimated agent count. `--lens` accepts hyphen-led values (`--lens -doc-drift`).
- `review plan --run <U> --ndjson <path|->`: appends cells (`C<n>`, status `assigned`). Validates `slice`, `concerns` (1–3 enabled lenses of one mode and the run's family, or one `probe:<slug>` with `question`, or one `check:<id>` whose executor is `cell`), `agent` and `reason`; binds each concern's content hash, path, methods and `v1_category`; attaches facets where the slice meets the facet's signal roots and the cell's concerns are in its `attaches_to`, then applies `facets` edits (`+id`/`-id`). A later call appends the next wave.
- `review cover <C> --status covered|thin|uncovered --note -`: the agent's coverage report; seals the cell. On a cell already sealed with the same status and note it succeeds as a no-op.
- `review add --run <U> --cell <C> --ndjson <path|->`: an agent's full finding set for its cell. Validates each row against the field catalogue (kind-specific required fields, enums, length caps, path form, `lens` ∈ the cell's concerns), checks `old` text occurs in its file within ±20 lines of `line` (in `target_file` when given and existing), comparing after normalising `\r\n` to `\n` in both, checks a plan row's `anchor_old` occurs exactly once under its `plan_section`, computes `anchor`, candidates and `route`, requires any `same_as` to name a candidate, and supersedes the cell's earlier unverified rows. Accepted defects and cleanup are minted `unverified`; advisory rows are minted `open`, or `merged` when their `same_as` names a dismissed, deferred, parked, backlogged or converted finding. A failing row is `refused` with its reason while the rest land. Prints each minted id with its `kind`, `severity` and `route`. Refused on a sealed cell, printing the cell's existing ids.
- `review verdict <F> --verdict … --by … [--seat n] [--severity s] --citation <c> --rationale -` (or `--ndjson` for a batch): upserts the verdict. A checker may return `confirmed` or `unsettled` (`unsettled` re-routes the finding to `verifier`), and `refuted` only with a grep citation on a row a sweeper wrote. For a finding with no `same_as`, the status resolves immediately: CONFIRMED/PLAUSIBLE → `open`, REFUTED → `refuted` (panel: ≥2 non-refuted of 3 → `open`). A finding with `same_as` stays `unverified` until `review resolve`. A verdict without a citation is refused.
- `review resolve --run <U> [--wave n]`: resolves every finding with a `same_as` whose verdicts are complete, in dependency order (a target from the same run resolves first), applying the outcome rules, each recorded in the row's `outcome`: target open or exported + CONFIRMED/PLAUSIBLE → **repeat** (run appended to the target's `seen_in` once, `line`/`evidence` refreshed; row `merged` with `merged_into = target`); target open or exported + REFUTED → **contested** (row `merged` with `merged_into = target`; the target keeps its status and gains the refuting verdict); target refuted + CONFIRMED → **reopened** (target `open` and gains the verdict; row `merged` with `merged_into = target`); target resolved + CONFIRMED → **regression** (row `open`, `related = [target]`); target dismissed, deferred, parked, backlogged or converted → **suppressed** (row `merged` with `merged_into = target`). `review digest` lists contested findings and `run finish` counts `suppressed` from `outcome`. A verifier that strikes the identity (`--strike-same-as` on its verdict) clears `same_as` and the row resolves on its own verdict. Deterministic and idempotent.
- `review list` / `review show <ids> --for finder|verifier|checker|implementer|orchestrator|audit|backlog|report`: role projections. `list` takes `--kind`, `--lens`, `--run`, `--cell`, `--status`, `--route` and `--in <glob>` (the finder's `PRIOR` query). A cell shown `--for finder` carries its slice, concerns with bound paths, facets with paths, methods with paths, the question and the pack path.
- `review digest --run <U> [--since-wave n]`: counts by kind and lens; cells by status with notes for `thin`/`uncovered`; **unverified ids** with their routes; **unselected** matched concerns and **unsliced** scope files without a waiver; contested findings; refuted criticals; the audit sample (chosen by index); and the advisory list until triaged.
- `review report --run <U> --out <path>`: the deterministic markdown report (defects by severity, the Needs-a-plan group, cleanup grouped by file, advisory with dispositions, coverage map with bound hashes, facets and waivers, run line); prints only the path and a counts line.
- `review triage <ids> --dismiss <r> | --defer <r> --trigger <t> | --reopen <r> | --park <r> | --resolve <r> | --backlog-ref <B-id> | --convert cleanup|defect --ndjson <path> --expect-status <s>`: dispositions with compare-and-set; stale rows land under `skipped_stale`. Each prose flag has a `--<flag>-file` twin.
- `review stats [--slug|--scope|--all] [--family] [--by lens|probe|run] [--outcomes] [--escapes] [--calibration [--exemplars N]]`: per lens (version as a column, not a grouping key) and per probe concern tag — runs, cells, yield by kind, refute rate, audit overturn rate, dismissal rate, uncovered and waiver rates. `stats` writes nothing: `--outcomes` reads v1 outcomes through `v2_ref` with `sync-v1`'s mapping and reports apply outcomes per lens and kind (fixed, wontfix with reasons, deferred, verified-clean). `--escapes` counts findings minted in a file and concern that an earlier run's cell had covered and sealed as `covered`. `--calibration` builds exemplars only from user `dismiss_reason`s, v1 wontfix reasons and audit-overturned verdicts.
- `review export-v1 (<ids> | --all) --to <v1-ledger> [--dry-run]`: Approach, "Export to v1".
- `review catalogue …`: Approach, "Lens catalogue".
- `review schema finding|cell-return|verdict-return [--kind <k>]`: JSON Schemas built from the field catalogue; `cell-return` is `{cell, findings: [{id, kind, severity, route}], status, escalate?}` (the object `review add` prints, echoed by the agent) and `verdict-return` is `{finding, verdict, reroute?}`. A unit test asserts the schemas and `add`'s validation agree.
- `review run finish <U> [--incomplete <reason>]`: stamps `finished` and computes `counts`. Refuses while any finding is `unverified`, any `same_as` is unresolved, or any unselected concern or unsliced file lacks a waiver — unless `--incomplete` records why.

**Anchor matcher** (D7). Within `family`, a row's anchor candidates are the findings matching `(file, symbol)` when `symbol` is non-empty, otherwise `(file, line)` within ±10. Candidates never decide identity: only a `same_as` naming a candidate does, confirmed or struck by the verifier. A row with candidates but no `same_as` is minted with them under `related`. `lens` and `kind` are not in the key. `dedup_id` is not used.

### Export to v1

`export-v1` is the bridge to the untouched `/review-apply` and `/optimise-apply`. Family `code` goes to `review-ledger.toml` (prefix `R`), family `perf` to `optimise-findings.toml` (prefix `O`). The verb validates the v1 required-field contract itself, because tomlctl's CLI write path checks only `id` and apply skips malformed rows silently (D8).
- **Kinds**: `defect` exports with its latest severity (a verifier reassessment wins; `major` → `warning`, `minor` → `suggestion`); `cleanup` exports as `suggestion`; `advisory` is refused. Both exported kinds carry their own `effort`.
- **`--all`** expands to the open defect and cleanup ids of the run, excluding `fix_shape = needs-plan`, test removals and cleanup whose latest verdict is PLAUSIBLE; those are exported only by explicit id.
- **Category**: the finding's lens's `v1_category` from the cell's binding; for a probe or a `check:` cell, `quality` (code) or `algorithm` (perf).
- **Description**: built from the implementer projection — `fix` or `edit` (+ `old`, `target_file`), then `failure_scenario`.
- **Refused**: family `plan`, `kind = advisory`, `fix_shape = needs-plan`, `effort = "large"`, any status other than `open`. Each refusal is reported with its reason.
- **Matching**: look up `v2_ref` across v1 rows of every status first; then the v1 merge rule — an open v1 match reuses the v1 id and bumps `rounds` only when this `v2_ref` is new; a fixed or applied match gets a new id with `related`; a deferred, wontfix, wontapply or verified-clean match is skipped.
- **Written fields**: `v2_ref = "<store path>#F<n>"` on the v1 row; `exported_as` and `export_ledger` on the v2 finding (status `exported`). `sync-v1` later reads the v1 outcome back.
- **`last_updated`**: the v1 ledger's root `last_updated` becomes the later of its current value and the newest run date among the findings' `seen_in` runs; a new ledger is seeded with that date.
- **Write order**: two sequential `mutate_doc` calls, never nested — the v1 write commits before the v2 store is opened, so a crash converges on re-export through `v2_ref`.
- `depends_on` F-ids map to exported v1 ids; unexported ones are reported under `dropped_deps`.

### Run pipeline

`claude/skills/review-v2-engine/` is the orchestrator contract every v2 carrier invokes. Stages, with the executor of each:

0. **Pre-flight** (orchestrator + tomlctl): require `review_add`, `review_survey` and `review_catalogue` in `tomlctl capabilities` `.features`, else halt with the `cargo install --path tomlctl` hint. Invoke the `tomlctl` and `flow-contract-flow-context` skills and build the envelope with the **v1 command name** (`--command review|optimise|review-plan`; D5). Bind `--slug` or `--scope`. Run `review sync-v1`. Plan family: run the task-store import with its ref-set gate per the unchanged `flow-contract-task-store` skill (an orchestrator verb there).
1. **Run start** (tomlctl): `review run start`, with `models` read from the six v2 agents' `model:` lines. It validates the merged catalogue. Scope safety as v1: no `..`, no absolute paths outside the worktree, glob breadth ≤200.
2. **Survey**:
   - `review survey` (tomlctl) records stack, diff and health signals with their roots.
   - `review-surveyor` (Haiku, one dispatch) then: proposes **judged** signals (`trust-boundary`, `hot-path`, `new-seam`, …) as candidates, each with its evidence paths, via `review run update`; runs the enabled checks with `executor = "surveyor"` (`dev-server-probe`, `store-check` — `tomlctl tasks import-plan --dry-run` and `tasks check --plan` — and `format-contract`); resolves the commands of `verification` checks through each check's discovery ladder; and writes `profile.md` (≤150 lines: files by role and subsystem, signals with measurements and roots, the candidates, and for the plan family the task list with effort tags) and `pack.md` (the agent preamble fences: `CONSTRAINTS` — the few CLAUDE.md rules a Haiku agent needs, since Haiku agents run without CLAUDE.md — `BACKLOG` rows for the scope, a fence per check result or pointer, and per-area `CALIBRATION` from `review stats --calibration`; an absent fence says why).
   - Its return is a ≤40-line digest: the candidates, hotspots, the resolved `verification` commands, and anything it could not measure. `pack.md`'s baseline fences (`LINT BASELINE`, `PROFILE BASELINE`, `BREAK-SET BASELINE`) are pointers to the run's recorded `baselines`, not inline output.
   - **Baselines** (orchestrator, CLAUDE.md build discipline): the unchanged `verification` agent stops at its first failing command and writes only to its own scratch directory, so the orchestrator dispatches it **once per resolved baseline command**, sequentially (they share `target/`), and records each returned log path — or `none — <reason>` — with `review run update --set-json baselines=…`. A red lint baseline is information, not a halt. `review show <C> --for finder` carries the recorded paths, and each finder reads the in-scope lines of those logs itself. **Stage 4 does not start until every resolved baseline has a recorded entry.**
3. **Compose** (orchestrator — the core judgement step):
   - Accept or reject each judged candidate, then run `review plan --propose --accept-signals …` (plus any user `--lens ±id`).
   - **Steering flags**, the same on all three carriers: `--depth quick|standard|deep` (default `standard`), `--flow <slug>`, `--probe "<question>"` (repeatable; each becomes a `probe:` cell) and `--lens +id|-id` (repeatable; passed through to `--propose`).
   - Read the proposal, the surveyor digest and `profile.md`. The orchestrator never reads lens bodies.
   - Edit the proposal (`references/compose.md`): re-slice hotspots into their own cells, re-bundle concerns, add a **probe** (`probe:<slug>` with a one-paragraph `question` and its nearest concern tag) wherever the change has a risk no catalogue lens owns, add facet `+`/`-` edits, add user `--probe` questions, and give each edited row a `reason`.
   - Write the map with `review plan`, then record a waiver for every unselected matched concern and unsliced scope file the digest lists.
   - At `deep`, show the composed map (cells, concerns, expected agent count) and confirm before launching. An expected count above 25 states why.
4. **Wave** (Workflow `review-v2-pipeline`, or Agent-tool waves). Each cell's agent fetches its assignment (`review show <C> --for finder`), reads the bound catalogue files, `pack.md` and its slice's `PRIOR` (`review list --in <slice> --for finder`), writes its full finding set with one `review add` call, seals the cell with `review cover`, and returns the `cell-return` object `review add` printed. Verification is routed by each row's `route`: `verifier` → one `review-verifier` per finding (`panel` → three seats at `deep`); minor defects at quick and standard → one verifier batch per cell; `checker` → `review-checker` batches of up to 20 rows pooled across cells; a checker's `unsettled` rows → `review-verifier`. The Workflow returns only the index `{run, wave, cells, verified, errors, script_version}`. The orchestrator ends its turn after `async_launched` and resumes on the task notification.
5. **Resolve and digest** (tomlctl → orchestrator): `review resolve --run <U> --wave <n>`, then `review digest --since-wave <n>`. The orchestrator launches a **mop-up** wave for any unverified ids (a restart or a lost return), and decides a **follow-up** wave — new cells for `uncovered`/`thin` cells, escalated sweeper candidates (re-run on `review-finder`; at `quick`, one Opus batch), or a gap the findings revealed — within the depth envelope.
6. **Audit** (orchestrator): every REFUTED critical, every contested finding, and — chosen by index in the digest — two CONFIRMED defects, two REFUTED non-critical defects, and two checked cleanup rows (at least one from a sweeper cell). It reads the `audit` projection and records each overturn as a `by = "audit"` verdict; overturns feed calibration.
7. **Triage** (orchestrator + user): `review report --out` writes the report; the orchestrator presents the counts line, the Needs-a-plan group, contested items, the advisory list with a recommendation per item (park — the default — backlog, convert or dismiss), and the report path. Grammar: `dismiss F3 — reason`, `defer F3 — reason — trigger`, `reopen F3 — reason`, `park F7 — reason`, `backlog F7,F9`, `convert F8`, `plan F5`, `export F1,F4` / `export all`. User prose is staged with the Write tool and passed through the `--<flag>-file` twins.
8. **Close** (tomlctl, plus the orchestrator's backlog writes): untriaged advisory is parked; for each `backlog` pick the orchestrator invokes the `backlog-capture` skill (loaded only now), runs its check-then-add gate on the rows `review show --for backlog` emits, and records `review triage --backlog-ref`; `export-v1` for the chosen ids, followed by the `/review-apply` or `/optimise-apply` hand-off naming the v1 ids; `review run finish`; the run id and report path.

Depth envelopes (`--depth`, default `standard`):

| | quick | standard | deep |
|---|---|---|---|
| cell ceiling (all waves; `--propose` scales it to ≈1 cell per 100 changed lines, floor 3) | 6 | 16 | 32 |
| concerns per cell | ≤3 | ≤3 | ≤2 |
| follow-up waves | 0 | 1 | 2 |
| defect verification | individual for critical/major; one batch verifier per cell for minor | same as quick | individual for every defect + 3-vote panel per critical (survives on ≥2 non-refuted) |
| cleanup check | pooled Haiku checker batches; code-touching, test-removal and unsettled rows → Opus | same | same |
| repeat passes | — | floor and hotspot cells once more with permuted file order | every high-risk cell once more |
| plan-executor cells | ≤4 tasks | ≤12 tasks (L and M first, checkpoint-marker tasks first) | every task |

**Substrate** (decision 2). `Workflow({name: "review-v2-pipeline", args})` with `args = {run, store: {slug|scope}, ledger_line, wave, depth, pack_path, cells: [{id, agentType}], schemas: {cell_return, verdict_return}}`. The script builds each prompt from a fixed template (the column-0 `ledger:` line when a flow is bound, the run and cell id, the pack path), uses `pipeline()` so verification starts as each cell returns, pools checker rows, and `log()`s every skipped cell and escalation. A saved workflow registers as `/<meta.name>`, so the name is `review-v2-pipeline` (D11). `claude/workflows/review-v2.js` is deployed by `scripts/deploy-claude.ps1`. Because agents persist their own output and writes are idempotent, the Agent-wave fallback (`references/agent-waves.md`) has the same persistence contract and the same per-cell prompt template as the script: cells in waves of ≤16 per message, then verifiers and checkers in waves of ≤16, every dispatch one-shot with no `name:`. Dispatches run in the background and report by completion notification, so a wave is complete only when every dispatched agent has notified or `review digest` shows every cell sealed (verifier waves: no unverified id left for that wave); at most 16 agents run at once, counting any `verification` agent and any agent of an earlier wave still running, which keeps the session under the 20-subagent limit past which a spawn fails and is not retried. The fallback fires when Workflow is disabled or unavailable, the name is unknown, the launch reports `error`, or the run completes with no result. The run records `substrate` and `agents`.

### Coverage proposal

`review plan --propose` is deterministic. Its rules:

- **`changed_lines`** — lines added plus removed over the scope in `base..HEAD`. When that is 0 (a path review of unchanged code), it is the scope's tracked line count. The plan family does not use it.
- **Cell ceiling** — `min(depth ceiling, max(3, ceil(changed_lines / 100)))` for code and perf, with the depth ceilings 6 / 16 / 32. Repeat cells count against it. In the plan family, `plan-executor` cells are outside the ceiling. Each one holds up to 4 tasks (its `slice` lists task refs), and the tasks covered follow the depth table's plan-executor row: L, then M, then checkpoint-maximal tasks, then by id.
- **Selection order** — floor lenses first. Then signal-tier lenses whose signals matched and whose `paths` meet the scope, then lenses whose `always_at` is the run's depth. Candidate lenses come last, ordered by how many of their `apply_when.signals` matched (most first), then by id. Selection stops at the ceiling. Every remaining matched concern is listed as unselected.
- **Slicing**, by each lens's `slice`:
  - `whole-scope`: one slice.
  - `per-subsystem`: one slice per signal root, or per top-level directory, that meets the scope.
  - `per-file`: one slice per churn-hotspot file, with the rest grouped by directory.
  - `per-task`: task refs.

  Churn-hotspot files are the top decile by commit count in the `churn-hotspot` detector's window. Under the `per-subsystem` and `per-file` rules each one gets a slice of its own.
- **Bundling** — concerns of one mode with identical slices merge into one cell, in selection order, up to the depth's per-cell maximum (3, or 2 at deep).
- **Repeat passes** — each repeat cell copies the original cell's slice and concerns, and its assignment lists the files in reverse order. The cell records this in a `pass` field (`2`).
  - At `standard`, every floor cell and every cell holding a hotspot file gets one repeat.
  - At `deep`, every cell whose slice holds a hotspot file or whose concerns include a signal-tier lens gets one.
- **Agent estimate** — cells + 1 surveyor + one verifier batch per cell + one checker batch per 20 expected cleanup rows (counted as 5 per cell).

### Agents

All six use the frontmatter `skills:` preload (R13): `review-v2-method` (the shared core — kinds and their fields, the store-write idiom, return objects, the never-a-finding list) for all, plus `research-methods` for `review-finder` and `review-verifier`. Each agent's **own procedure lives in its body**, so no agent loads the others' procedures. The three Haiku agents set `omitClaudeMd: true` and receive the few rules they need through the pack's `CONSTRAINTS` fence. A preload injects only the skill's `SKILL.md` and reports no base directory, so every agent body says "if `review-v2-method` is not loaded, invoke it first" (every agent holds Skill), catalogue files are read from the absolute paths in the cell assignment, and `review show --for finder|verifier|checker` also carries the absolute paths of `review-v2-method`'s worked-example references under the resolved claude dir. `review-finder` and `review-verifier` call `Skill(research-methods)` explicitly when they need one of its gated references. Every writing agent holds Write, used only to stage payloads in the run directory (the block below). Each agent carries the `review-v2-store-writer` block (below) and the existing `research-delivering` block byte-identically, both registered in `scripts/shared-blocks.toml`.

| Agent | Model / effort | Tools | Writes | Job |
|---|---|---|---|---|
| `review-surveyor` | haiku / medium, `omitClaudeMd` | Read, Grep, Glob, Bash, Write, Skill | `profile.md`, `pack.md`; `review run update` (judged candidates) | Run surveyor checks, propose judged signals with evidence, assemble the pack; never decide a judged signal |
| `review-sweeper` | haiku / medium, `omitClaudeMd` | Read, Grep, Glob, Bash, Write, Skill | `review add`, `review cover` | Run mechanical lenses (`ripple`, `hygiene`) on one cell; report a candidate that needs judgement under `escalate` instead of writing it |
| `review-checker` | haiku / medium, `omitClaudeMd` | Read, Grep, Glob, Bash, Write, Skill | `review verdict` | Check pooled cleanup rows by grep-decidable predicates only — the `old` text is present, a symbol claimed dead has no reference, the edit stays within `file`/`target_file`/`instances`, the target exists. CONFIRMED when every predicate holds with a citation; otherwise `unsettled`. REFUTED only on a sweeper's row, with a grep citation |
| `review-finder` | opus / high | research-deep's list plus Write | `review add`, `review cover` | Run one cell (a lens bundle, a probe, or a `check:` cell such as `plan-checks`); produces all three kinds |
| `review-verifier` | opus / high | research-deep's list minus Playwright, plus Write | `review verdict` | Refute-first on one defect, a minor batch, or a routed cleanup row: start from `counter`, re-read the anchor, reproduce the trigger, look for intent (comment, `git log -S`, ADR, test), confirm or strike `same_as`, reassess severity; cite `file:line` or command output for every verdict; never concede on an uncited assertion (R8). For a test removal, name the regression the test is the only guard for or confirm there is none |
| `plan-executor` | opus / medium | Read, Grep, Glob, Bash, Write, Skill | `review add`, `review cover` | Given only what an `implement-*` agent gets (plan preamble sections + one task, fetched by `tomlctl tasks show --with body,files,deps`), dry-run the task: invented decisions, files touched vs the **Files** line, acceptance commands not runnable as written, unconfirmed premises |

The `review-v2-store-writer` block, copied byte-identically into all six agents (inline code only, no fenced commands):

```
<!-- SHARED-BLOCK:review-v2-store-writer START -->
## Store writes

You never edit, create or delete files in the working tree, except inside the run directory your assignment names: the surveyor writes `profile.md` and `pack.md` there, and every writer stages its payloads in its `payloads/` subdirectory. Your only other writes are `tomlctl review` calls against the store named in your assignment — `add`, `cover` and `verdict` for review agents, `run update` for the surveyor — always passing the same `--slug` or `--scope` your assignment names. Never run `tomlctl items`, `tomlctl backlog add`, `tomlctl tasks import-plan` without `--dry-run`, `git add`, `git commit` or any formatter or build command. Write each payload with the Write tool to `payloads/<cell or batch id>.ndjson` in the run directory and pass `--ndjson <that path>`; never pipe, echo or heredoc a payload into tomlctl. A cell's findings go in one `review add` call holding the full set; if you must resubmit, resubmit the full set. If `review add` reports the cell is sealed, return the ids it prints. If a write fails with `os error 5`, retry it once; on any other failure, or a refused row, read the reason, fix the row and resubmit once, then report the refusal in your return. Your return is only the small object your assignment asks for: everything else belongs in the store.
<!-- SHARED-BLOCK:review-v2-store-writer END -->
```

### Lens catalogue

The catalogue is the maintained, pluggable source of every concern a run can compose. It lives once, globally, shared by every project; a project only opts entries in or out and varies them by path.

**Entry types.** Each entry is one markdown file whose frontmatter is TOML between `+++` lines (tomlctl already parses TOML; no dependency is added). There are four types:

| Type | Directory | What it holds | Frontmatter beyond `id`, `status` (and an optional display `version`) |
|---|---|---|---|
| **lens** | `lenses/` | A concern: the question it answers, the concern tags it owns, its probes per family, what it does not flag, its edit forms and severity calibration | `families`, `mode` (judgement \| mechanical \| executor), `produces`, `owns` (concern tags), `activation` (floor \| signal \| candidate \| opt-in), `apply_when` (`signals`, `paths`, `min_depth`, `always_at`), `after_wave`, `methods`, `slice` (whole-scope \| per-subsystem \| per-file \| per-task), `v1_category` (per family), `research_domain` |
| **facet** | `facets/` | Stack knowledge: per-lens probes and pitfalls for one language, framework, runtime or data layer. It replaces v1's inline frontend/backend variants and most of `/optimise`'s Focal Points Brief | `families`, `apply_when` (stack signals), `attaches_to` (lens ids or `"*"`) |
| **method** | `methods/` | A procedure lenses cite by id instead of restating it | — |
| **check** | `checks/` | A command-backed, surveyor-run or cell-run check feeding a pack fence or findings | `families`, `executor` (tomlctl \| surveyor \| verification \| cell), `fence` or `produces`, `apply_when`, `activation`, `discover` (the command-discovery ladder for `verification` checks) |

Body sections are fixed per type — **Lens**: Question; Owns (one paragraph per owned tag); Probes — `<family>` (one per declared family); Does not flag; Edit forms; Advisory angles; Severity calibration. **Facet**: a general section, then `### For <lens-id>` per attached lens. **Check**: Runs; Interpreting the result; Fence or finding mapping. A lens's Does not flag section is seeded from the v1 rejection clusters (confirmations, stale-comment trust, design intent and ADRs, speculative and style-only, out of scope, negligible gain against risk).

**Activation tiers.** `floor` — always proposed for its families. `signal` — proposed when a listed signal is present and, if `paths` is set, the scope meets it; `always_at = "deep"` also proposes it at deep. `candidate` — eligible; proposed only as the depth's budget allows, otherwise listed as unselected and needing a waiver when its signals matched. `opt-in` — only when the project config enables it or the user passes `--lens +id`. `min_depth` and `after_wave` restrict further. `experimental` status implies `opt-in`.

**Vocabulary** (`vocabulary.toml`):
- **Signals**: `[signals.<name>]` with `kind` (stack \| diff \| health \| judged), `meaning`, and for every kind except `judged` a declarative `detect` table evaluated by `review survey` — `files` (globs over `git ls-files`), optional `contains` (a regex compiled through `query::compile_user_bytes_regex` under the crate's ASCII-only `regex` build, so `(?-u:\b)` forms and no Unicode classes), `manifest` (key paths into TOML and JSON manifests only; MSBuild XML — `Directory.Packages.props`, `<PackageReference>`, legacy `<Reference Include=…>` — is detected with `files` plus `contains`), and for health signals `count` (per-stack literal substrings matched verbatim, never regexes, e.g. `#[test]` for Rust, `[Fact]`/`[Theory]` for .NET, `test(`/`it(` for TS) with a `window` and `threshold`. Judged signals carry a `hint` for the surveyor. Stack signals: `lang:rust`, `lang:ts`, `lang:csharp`, `framework:react`, `framework:vue`, `runtime:tokio`, `db:sqlx`, `db:efcore`, `db:dapper`, `harness:claude`. Diff: `rename-or-removal`, `public-api-change`, `dependency-change`, `schema-change`, `query-layer`, `async-concurrency`, `process-heavy`, `frontend`, `docs-touched`, `tests-touched`. Health: `test-growth`, `test-ratio-high`, `doc-growth`, `churn-hotspot`, `finding-hotspot`, `backlog-cluster`. Judged: `trust-boundary`, `hot-path`, `new-seam`.
- **Concern tags**: `[concerns.<tag>]` with `meaning` and `families`. Ownership is declared in each lens's `owns`; the validator requires exactly one owner per (family, tag) across the merged layers, and `review catalogue --owners` prints the generated map.
- **Routing**: `[routing]` with `executable` globs (source files whose edits change behaviour) and `tests` globs, used by `review add` to compute `route`; a project may extend both.

**Layers.**
1. **Built-in (global)**: `claude/skills/review-v2-lenses/{lenses,facets,methods,checks}/` plus `vocabulary.toml`, deployed to `~/.claude/skills/review-v2-lenses/` by `scripts/deploy-claude.ps1`. Every project gets the full catalogue with zero setup.
2. **Project (optional, minimal)**: `.claude/review-lenses/config.toml`:
   ```toml
   disable = ["doc-consolidation"]          # built-in entries switched off here
   enable = ["plan-premortem"]              # opt-in entries switched on here
   [extend.idioms]                          # append-only additions
   does_not_flag = ["…"]
   probes = ["…"]
   [override.test-pruning]                  # replace named sections or activation fields
   activation = "opt-in"
   [scope."packages/legacy/**"]             # path-scoped variation
   disable = ["test-pruning"]
   facets = ["-react"]
   [[checks]]                               # project checks (executor = verification)
   id = "harness-gates"
   families = ["code"]
   paths = ["claude/**"]
   command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
   ```
   An absent file means the defaults. `.claude/review-lenses/vocabulary.toml` may add signals and concern tags under a reserved `project:` prefix, and `.claude/review-lenses/{lenses,facets}/*.md` may add project entries that own `project:` tags. An id colliding with a built-in is refused (use `extend`, `override` or `disable`), so a project never silently forks a shared lens. Disabling a lens leaves its tags reported as `disabled` in the digest, not as unselected.

**Generated index and validation.** `tomlctl review catalogue [--builtin <dir>] [--project <dir>] [--family <f>] [--signals <a,b>] [--vocabulary] [--owners] [--check]` merges the layers, applies the config and prints one row per entry (`type`, `id`, `hash`, `status`, `families`, `mode`, `produces`, `activation`, `apply_when`, `matched`, `attaches_to`, `path`); `--vocabulary` prints the signals with their detectors; `--owners` the ownership map; `--check` exits 1 on any validation error. Validation covers unique ids, the frontmatter schema per type, signals and tags drawn from the merged vocabulary, one owner per (family, tag) with each lens's Owns section naming exactly its `owns`, existing `methods` and `attaches_to` targets, a Probes section per declared family, config and scope ids that exist, the `project:` prefix rule, every `contains` pattern compiling through `query::compile_user_bytes_regex`, and `v1_category` valid per family — both sets closed: code `quality`, `security`, `architecture`, `completeness`, `db`, `testability`, `package-quality` (the `flow-contract-ledger-schema` review vocabulary); perf `memory`, `serialization`, `query`, `algorithm`, `concurrency`; the plan family has none. The dispatch-test gate `catalogue_validates` runs it over the built-in catalogue plus this repo's project config, and the `command_lint` scan set — shared by `guidance_lint` and `flag_table_lint` — widens to the catalogue directories so every `tomlctl …` line in a bash fence parses and follows the guidance canon. The pre-commit hook runs that gate on a staged `claude/**/*.toml` or `.claude/review-lenses/**` path as well as on `claude/**/*.md` and `tomlctl/src/**`. `review run start` validates the merged layers in every repo.

**Writing an entry.** Frontmatter values for a lens come from the table below (families, mode, activation, slice, `v1_category`) and its task's Detail (`methods`, `apply_when`); `produces` is all three kinds for a judgement lens, `cleanup` for a mechanical one (`ripple`, `hygiene`) and `advisory` for `contrarian`. `owns` is drawn from `claude/skills/review-v2-lenses/vocabulary.toml`. Seed every Does not flag section from the v1 rejection clusters (confirmations, stale-comment trust, design intent and ADRs, speculative and style-only, out of scope, negligible gain against risk) and the source agent's own "Do not flag" sentence. Stack-specific text goes to facets, not lenses. A facet has a general section, then one `### For <lens-id>` section per attached lens, and its probes are concrete pitfalls, not tutorials. Ownership collisions between entries written in parallel are settled by the catalogue gate, never by loosening the validator.

**Runs bind content.** `review plan` binds each concern's **content hash** (sha256 over its versioned sections — Question, Owns, Probes, Does not flag), path, methods and `v1_category`; `run start` records the catalogue tree sha. A lens edit is visible in `stats` without anyone bumping a number.

**Lifecycle.** `status` is `experimental` (opt-in) → `stable` → `deprecated` (refused in new cells, still resolvable for stats). A reworked stable lens is introduced as a new `experimental` id that the project or user opts into beside the old one, then swapped. A probe records its nearest concern tag; promotion is considered when `review stats --by probe` shows a tag's probes recurring across three or more runs — first as a project entry, then built-in once it proves portable.

**Initial catalogue.** v1 sources: `/review` (`claude/commands/review.md` Agents 1–6), `/optimise` (`claude/commands/optimise.md` Agents 1–5 and the cross-cutting concurrency review), `/review-plan` (`claude/commands/review-plan.md` Agents 1–4 and the Step 2.6 checks), the `research-methods` lens notes (`claude/skills/research-methods/references/codebase-review.md`), and lumina's `research-explore` and `story-review`.

| Lens | Families | Mode | Activation | Slice | `v1_category` | From |
|---|---|---|---|---|---|---|
| `correctness` | code | judgement | floor | `per-subsystem` | `quality` | new; the method notes' boundary list + `/review` A4 edge cases and error paths (incl. missing handling) |
| `type-design` | code | judgement | candidate | `per-subsystem` | `architecture` | `/review` A1 (illegal states, untyped domain primitives, stringly-typed sets) |
| `domain-model` | code | judgement | signal (`schema-change`, `query-layer`) | `per-subsystem` | `db` | `/review` A3 `db` |
| `architecture` | code | judgement | candidate | `whole-scope` | `architecture` | `/review` A3 (layering, placement, evolvability) + method notes |
| `idioms` | code | judgement | candidate | `per-subsystem` | `quality` | `/review` A1 (converged pattern, decided from history) |
| `duplication` | code | judgement | candidate | `whole-scope` | `quality` | `/review` A1 DRY + method notes |
| `security` | code, plan | judgement | signal (`trust-boundary`, `dependency-change`) | `per-subsystem` | `security` | `/review` A2 (cap 5) |
| `external-validity` | code, plan | judgement | signal (`dependency-change`) | `whole-scope` | `package-quality` | `/review` A2 advisories + `/review-plan` A4 pinned APIs, deprecations, advisories |
| `ripple` | code, plan | mechanical | floor | `whole-scope` | `completeness` | `/review` A4 ripple + `/review-plan` A2 sweeps 1–3 |
| `hygiene` | code | mechanical | candidate | `per-subsystem` | `quality` | new: grep-provable dead code, stale TODOs, unused config and imports |
| `test-gaps` | code, plan | judgement | candidate | `per-subsystem` | `testability` | `/review` A5 tests + `/review-plan` A2 missing tests |
| `seams` | code | judgement | candidate | `per-subsystem` | `testability` | `/review` A5 seams |
| `diagnostics-dx` | code | judgement | candidate | `per-subsystem` | `testability` | `/review` A5 error messages, logs, help text |
| `test-pruning` | code | judgement | signal (`test-growth`, `test-ratio-high`) | `per-subsystem` | `testability` | new; `direction = remove` |
| `doc-drift` | code | judgement | candidate (`docs-touched` raises its priority) | `per-file` | `quality` | new: prose and comments against behaviour (owns comment rot) |
| `doc-consolidation` | code | judgement | signal (`doc-growth`) | `whole-scope` | `quality` | new |
| `memory-runtime` | perf | judgement | candidate | `per-subsystem` | `memory` | `/optimise` A1 |
| `data-shape` | perf | judgement | candidate | `per-subsystem` | `serialization` | `/optimise` A2 |
| `data-access` | perf | judgement | signal (`query-layer`) | `per-subsystem` | `query` | `/optimise` A3 |
| `algorithm` | perf | judgement | floor | `per-file` | `algorithm` | `/optimise` A4 |
| `concurrency` | perf | judgement | signal (`async-concurrency`) | `per-subsystem` | `concurrency` | `/optimise` A5, with its severity calibration |
| `io-process` | perf | judgement | signal (`process-heavy`) | `per-subsystem` | `algorithm` | new |
| `plan-grounding` | plan | judgement | floor | `whole-scope` | — | `/review-plan` A2 alignment + sweeps 4–5; cross-cutting concerns (logging, error handling, authz, cache invalidation) and config/migration/build changes |
| `plan-dependencies` | plan | judgement | floor | `whole-scope` | — | `/review-plan` A1 edges and checkpoint placement, given `STORE CHECK` |
| `plan-structure` | plan | judgement | floor | `whole-scope` | — | `/review-plan` A3 whole-plan half: goal coverage by Success Criteria, approver sections, format conformance against `FORMAT CONTRACT`, Execution Policy fields, prose-format restructuring |
| `feasibility` | plan | judgement | candidate | `whole-scope` | — | `/review-plan` A1 feasibility and coherence |
| `plan-executor` | plan | executor | floor | `per-task` | — | new dry run per task (R9) + `/review-plan` A3 per-task executability |
| `plan-risk` | plan | judgement | candidate | `whole-scope` | — | `/review-plan` A4 rollback, risky-task checkpoint placement, back-compat, scope realism |
| `plan-premortem` | plan | judgement | opt-in (experimental) | `whole-scope` | — | new (R9, weak evidence) |
| `contrarian` | code, perf, plan | judgement | signal (`public-api-change`, `dependency-change`, `new-seam`), `always_at = "deep"` | `whole-scope` | — (advisory only) | lumina `contrarian` + `story-review` devil's advocate; advisory only |
| `synthesis` | code, perf, plan | judgement | opt-in, `after_wave = 1` | `whole-scope` | code `quality`, perf `concurrency` | new + `/optimise` cross-cutting concurrency |

- **Facets** (10), covering the stacks across the user's repos (Rust and tokio/sqlx here, TypeScript/React in reportdesignkit, Vue in lumina's SPA, .NET with EF Core in prime and mercury and Dapper/raw SQL in Intembeko and tw-orchestrator): `rust`, `tokio`, `sqlx`, `typescript`, `react`, `vue`, `dotnet`, `efcore`, `dapper-sql`, `claude-harness` (the portable judgement half of v1's Package Quality lens).
- **Methods** (6): `sweep`, `trace`, `history-idiom`, `measure`, `dry-run`, `steelman`.
- **Checks** (9): `lint-baseline`, `profile-baseline` and `break-set-baseline` (executor `verification`; the break-set baseline commands come from the plan's predicted break sets); `store-check`, `format-contract` and `dev-server-probe` (executor `surveyor`); `two-control-probe`, `falsifiability` and `acceptance-contradiction` (executor `cell`, composed together as one whole-plan `review-finder` cell named `plan-checks`, which also judges the break-set preconditions against the baseline fence and runs the probe helper in one batched shell call).
- **This repo's project config**: one project check, `harness-gates`, running the `cli::dispatch::tests` gates instead of having an agent re-derive them.

**lumina overlap.** lumina's six `research-explore` lenses map onto the catalogue: `codebase` → the survey plus `plan-grounding`, `library` → `external-validity`, `risk` → `plan-risk`, `completeness` → `plan-grounding` + `test-gaps` + `ripple` (its judgement half lands in the first two), `domain` → `domain-model`, `contrarian` → `contrarian`. Defect severity names match lumina's `Critical`/`Major`/`Minor`. The catalogue `SKILL.md` records the mapping; lumina is not edited.

### Measurement and tuning

With A/B dropped (decision 12), the trial is judged by signals from outside the reviewers (R4):
- **Apply outcomes** — `review stats --outcomes` joins exported findings to their v1 status through `v2_ref` (fixed, wontfix with reasons, deferred, verified-clean) per lens and kind; `sync-v1` keeps the v2 store current, so a re-found fixed issue is a **regression**, not a repeat.
- **Escapes** — `review stats --escapes` counts findings minted in a file and concern that an earlier run had sealed as `covered`: the recall proxy for composed coverage.
- **Audit** — overturn rates on CONFIRMED and REFUTED defects and on checked cleanup, per lens and per agent model.
- **Calibration** — exemplars come only from user dismissals, v1 wontfix reasons and audit overturns, so a verifier that over-refutes cannot train finders to stop raising a class.
- **Cost** — agents and waves per run from the run record. v1's orchestrator-context baselines (`/review`+`/review-apply` peak 401k, `/review-plan` 329k, measured 2026-10-12 over the 60 most recent flow transcripts by attributing main-thread tool inputs and results at chars/4 ≈ tokens) are recorded in `measurement.md` with value, date and method, because the scratch scripts that produced them were never committed; a v2 run's context is measured the same way.
- **Seeded recall check** (After Merge) — re-run `/review-v2 --depth quick` on the parent commits of five v1 `fixed` items and record hit or miss at file + symbol.
- **Thresholds** — no lens, facet or activation is retuned on fewer than five runs of evidence; the engine's `references/measurement.md` states the numbers that count as better or worse than v1's baseline (vet-drop per lens, 89–99% resolved precision, the 401k `/review`+`/review-apply` context peak).

### Family specifics

- **`/review-v2`** (family `code`): `LINT BASELINE`; the context-free correctness cell (the `correctness` lens with an empty pack, diff only; R7), composed at standard and up; export to `review-ledger.toml`.
- **`/optimise-v2`** (family `perf`): a Focal Points Brief only for what the bound facets do not already carry; `PROFILE BASELINE` from the `profile-baseline` check — with none, a perf defect needs a complexity argument tied to a real input size or is `advisory` with `angle = risk`; cleanup covers small, safe wins; export to `optimise-findings.toml`.
- **`/review-plan-v2`** (family `plan`): plan resolution as v1 Step 1 (with the `flow-contract-plansdirectory-prompt` skill); the task-store import at stage 0; `STORE CHECK` (dry run + `tasks check --plan`) and `FORMAT CONTRACT` from the surveyor; `plan-structure`, `plan-grounding`, `plan-dependencies` and `plan-executor` as the floor; the `plan-checks` cell in place of v1's orchestrator-only Step 2.6, its defects verified like any other. Merge offer: confirmed or plausible defects and cleanup are eligible; advisory is parked or picked. The merge follows `claude/commands/review-plan.md` Step 4 by reference (that file stays unchanged):
  - **Manual** (4A): judgement merge, `.premerge.md` backup, and A3.5's ref-set gate → import → check with Files-line closure.
  - **Mechanical** (4B): B2's skip of any `plan_section` holding more than one anchor, B3's exactly-once anchor rule, the `.revised.md` sibling, and Q3 defaulting to **Keep both**.
  - **Empty answers**: an empty answer to either question persists findings only — v2 has no user-inputs store to post the question to — and the report says the merge was skipped.

  The orchestrator merges from the implementer projection, which carries `plan_section`, `anchor_old` and `anchor_new`. Merged findings resolve with `resolution = "merged into <plan>"`. The next run's plan-family finders receive those `plan_section`s first in `PRIOR`, with each merged finding's intent, so the merge-written text is the first text they review.

### Coexistence with v1

- Every v1 file under Scope → Out of scope stays byte-identical, guarded by a `git diff --quiet` success criterion.
- v2 never writes a v1 ledger except through `export-v1`, and reads v1 ledgers only through `sync-v1` and `stats --outcomes`; it skips the user-inputs sweep; it shares the backlog store through the unchanged `backlog-capture` contract, with the orchestrator as the only backlog writer and only for user-picked advisory.

## Success Criteria

- guard: v1 review harness files, and the v1 skills and agents v2 reuses or leaves out of scope, are byte-unchanged against the plan's base commit: `git diff --quiet 27b7e1a -- claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md claude/commands/review-apply.md claude/commands/optimise-apply.md claude/agents/research-deep.md claude/agents/research-lite.md claude/agents/implement-deep.md claude/agents/implement-lite.md claude/agents/flow-bootstrap.md claude/agents/verification.md claude/skills/research-methods claude/skills/flow-contract-vet-research claude/skills/flow-contract-ledger-schema claude/skills/flow-contract-apply-pipeline claude/skills/flow-contract-apply-constraints claude/skills/flow-contract-apply-dependency-sort claude/skills/flow-contract-apply-rollback-protocol claude/skills/flow-contract-apply-vet-implement-lite claude/skills/flow-contract-flow-context claude/skills/backlog-capture claude/skills/flow-contract-task-store claude/skills/flow-contract-plan-output-format` exits 0 (today: exits 0).
- forward: all three v2 carriers exist and invoke the engine skill: `test "$(grep -l '`review-v2-engine` skill' claude/commands/review-v2.md claude/commands/optimise-v2.md claude/commands/review-plan-v2.md 2>/dev/null | wc -l)" -eq 3` exits 0 (today: exits 1).
- forward: the initial catalogue is complete — 31 lenses, 10 facets, 6 methods, 9 checks and the vocabulary: `c=claude/skills/review-v2-lenses; test "$(ls $c/lenses/*.md 2>/dev/null | wc -l)" -eq 31 && test "$(ls $c/facets/*.md 2>/dev/null | wc -l)" -eq 10 && test "$(ls $c/methods/*.md 2>/dev/null | wc -l)" -eq 6 && test "$(ls $c/checks/*.md 2>/dev/null | wc -l)" -eq 9 && test -f $c/vocabulary.toml` exits 0 (today: exits 1).
- forward: the catalogue validates through the same code `tomlctl review catalogue --check` runs — **predicted, unverified** (builds `target/`): `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(catalogue_validates)'` (today: fails, no test matches).
- guard: the dispatch gates, including `command_lint` over the widened catalogue scan set, keep passing: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` (today: 43 passed).
- forward: the three Haiku agents carry `model: haiku` and `omitClaudeMd: true`, and the three Opus agents `model: opus`: `test "$(grep -l '^model: haiku' claude/agents/review-surveyor.md claude/agents/review-sweeper.md claude/agents/review-checker.md 2>/dev/null | wc -l)" -eq 3 && test "$(grep -l '^omitClaudeMd: true' claude/agents/review-surveyor.md claude/agents/review-sweeper.md claude/agents/review-checker.md 2>/dev/null | wc -l)" -eq 3 && test "$(grep -l '^model: opus' claude/agents/review-finder.md claude/agents/review-verifier.md claude/agents/plan-executor.md 2>/dev/null | wc -l)" -eq 3` exits 0 (today: exits 1).
- forward: the v2 agents are shared-block carriers and the gate passes: `test "$(grep -c 'review-v2-store-writer' scripts/shared-blocks.toml)" -ge 1 && bash scripts/verify-shared-blocks.sh >/dev/null` exits 0 (today: exits 1).
- forward: the v2 carriers are in the skill-invocation gate table: `test "$(grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs)" -eq 3` exits 0 (today: exits 1).
- forward: the `tomlctl review` suites pass, including: a defect row without `failure_scenario` refused; a resubmitted cell superseding its unverified rows; `run finish` refusing an unwaived unselected concern; a re-found finding whose exported v1 row is `fixed` resolving as a regression after `sync-v1` — **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -E 'binary(/^review_/)'`.
- forward: the field-catalogue, store, catalogue and matcher unit tests pass, including the verifier projection withholding `grade` and `fix` and a facet attaching only where a cell's slice meets its signal roots — **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^review::/)'` (today: fails; `cargo test --lib -- review::` would exit 0 with 0 tests run).
- forward: apply outcomes and calibration close the loop — `review stats --outcomes` joins exported findings to their v1 status through `v2_ref`, and `--calibration` draws exemplars only from user dismissals, v1 wontfix reasons and audit overturns — **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-tests=fail -E 'binary(review_stats)'`.
- forward: in a repo with no `.claude/review-lenses/`, `tomlctl review catalogue --check` exits 0, and `tomlctl review survey` on a TypeScript repo and a .NET repo each record their stack signals with roots — **predicted, unverified** (installed binary, deployed catalogue, the user's repos).
- forward: an end-to-end `/review-v2 --depth quick` on `tomlctl/src/sweep.rs` records one run whose proposal binds lens hashes and the `rust` facet but not `vue` or `tokio`, findings written by the agents with a verdict on every defect and cleanup, every cell sealed with a status and note, waivers for every unselected matched concern, and computed `counts` — **predicted, unverified** (live session; see After Merge).
- forward: `/optimise-v2 --depth quick` on `tomlctl/src/sweep.rs` and `/review-plan-v2 --depth quick` on this plan each record one run, the latter with a `plan-checks` cell — **predicted, unverified** (live session).
- forward: `tomlctl review export-v1 <F-id> --scope <scope> --to .claude/reviews/<scope>.toml` writes a v1 row that `tomlctl items list .claude/reviews/<scope>.toml --where-has v2_ref` lists with every v1 required field, `/review-apply` selects it, and a following `tomlctl review sync-v1 --scope <scope>` marks the finding `resolved` — **predicted, unverified** (live session).
- forward: the same holds for the perf family — a `/optimise-v2` finding exported with `--to .claude/optimise-findings/<scope>.toml` is selected by `/optimise-apply`, and `sync-v1` then marks it `resolved` — **predicted, unverified** (live session).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
transient: rust-lld: failed to write output.*[Pp]ermission denied
success: git diff --quiet 27b7e1a -- claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md claude/commands/review-apply.md claude/commands/optimise-apply.md claude/agents/research-deep.md claude/agents/research-lite.md claude/agents/implement-deep.md claude/agents/implement-lite.md claude/agents/flow-bootstrap.md claude/agents/verification.md claude/skills/research-methods claude/skills/flow-contract-vet-research claude/skills/flow-contract-ledger-schema claude/skills/flow-contract-apply-pipeline claude/skills/flow-contract-apply-constraints claude/skills/flow-contract-apply-dependency-sort claude/skills/flow-contract-apply-rollback-protocol claude/skills/flow-contract-apply-vet-implement-lite claude/skills/flow-contract-flow-context claude/skills/backlog-capture claude/skills/flow-contract-task-store claude/skills/flow-contract-plan-output-format
success: test "$(grep -l '`review-v2-engine` skill' claude/commands/review-v2.md claude/commands/optimise-v2.md claude/commands/review-plan-v2.md 2>/dev/null | wc -l)" -eq 3
success: c=claude/skills/review-v2-lenses; test "$(ls $c/lenses/*.md 2>/dev/null | wc -l)" -eq 31 && test "$(ls $c/facets/*.md 2>/dev/null | wc -l)" -eq 10 && test "$(ls $c/methods/*.md 2>/dev/null | wc -l)" -eq 6 && test "$(ls $c/checks/*.md 2>/dev/null | wc -l)" -eq 9 && test -f $c/vocabulary.toml
success: cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(catalogue_validates)'
success: cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests
success: test "$(grep -l '^model: haiku' claude/agents/review-surveyor.md claude/agents/review-sweeper.md claude/agents/review-checker.md 2>/dev/null | wc -l)" -eq 3 && test "$(grep -l '^omitClaudeMd: true' claude/agents/review-surveyor.md claude/agents/review-sweeper.md claude/agents/review-checker.md 2>/dev/null | wc -l)" -eq 3 && test "$(grep -l '^model: opus' claude/agents/review-finder.md claude/agents/review-verifier.md claude/agents/plan-executor.md 2>/dev/null | wc -l)" -eq 3
success: test "$(grep -c 'review-v2-store-writer' scripts/shared-blocks.toml)" -ge 1 && bash scripts/verify-shared-blocks.sh >/dev/null
success: test "$(grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs)" -eq 3
success: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -E 'binary(/^review_/)'
success: cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^review::/)'
success: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-tests=fail -E 'binary(review_stats)'
success: cargo clippy --manifest-path glimpse/Cargo.toml --locked
```

The pre-commit hook adds the crate-wide `cargo fmt --check`, the `cli::dispatch::tests` gate subset (which includes the catalogue gate and the widened `command_lint` walk once task 46 lands, for any staged `claude/**/*.md` or `tomlctl/src/**`, and from task 57 also `claude/**/*.toml` and `.claude/review-lenses/**`), and glimpse clippy `--locked` on any tomlctl `src` change. Run `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null` after the version bump to refresh `glimpse/Cargo.lock`.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 19, 22, 23, 47, 52, 53, 56, 57, 58, 59, 62
- **Max parallel agents**: 8
- **Commit granularity**: per-task

## Tasks

### 1. Add the review field catalogue [M]
- **Files**: `tomlctl/src/review/fields.rs` (new), `tomlctl/src/review/mod.rs` (new), `tomlctl/src/lib.rs`
- **Depends on**: —
- **Action**: Create `tomlctl/src/review/fields.rs` holding the field catalogue as `const` data — every finding, cell, run and verdict field with its name, kind applicability, required-ness per kind, `direction` and family, enum values, length cap, and per-role visibility — plus `validate_row`, `project(role, …)` and `route(kind, direction, severity, depth, file, target_file, routing_globs) -> Route`. Create `tomlctl/src/review/mod.rs` declaring `mod fields;` and register `mod review;` in `tomlctl/src/lib.rs`.
- **Detail**: Follow Approach, "Finding kinds" and Approach, "Field readership" — the matrix there is the projection table, plus the `audit` role. Enums: defect `severity` critical/major/minor, `fix_shape` edits/needs-plan, verdicts confirmed/plausible/refuted/unsettled, finding statuses per Approach, "v2 store and the tomlctl review group" **Records**. Cleanup `old` is required for `change`/`remove`, and for `add` unless `target_file` is given and does not exist. `effort` (defect and cleanup) is `trivial`/`small`/`medium`/`large`. Plan-family fields `plan_section`, `anchor_old`, `anchor_new` with the matrix's visibility. Path fields (`file`, `target_file`, `instances`, `citation`) must be repo-relative, `/`-separated, with no `..`. Caps: `summary` 160, `failure_scenario` 600, `fix` 800, `edit` 800, `old` 400, `counter` 300, `rationale` 800, `evidence` 5 × 300, `instances` 50, verdict `rationale` 600, `plan_section` 200, `anchor_old` 400, `anchor_new` 800. `route` returns `panel` for a critical defect at `deep`. Add no dependency. In-module tests: `defect_requires_failure_scenario_and_fix_shape`, `cleanup_add_with_new_target_file_needs_no_old`, `cleanup_change_requires_old`, `advisory_challenge_requires_challenges`, `over_cap_field_is_refused_with_its_cap`, `verifier_projection_withholds_grade_and_fix`, `projection_table_matches_matrix`, `route_sends_executable_edits_and_test_removals_to_verifier`, `route_sends_critical_at_deep_to_panel`, `absolute_or_backslash_path_is_refused`.
- **Acceptance**:
  - forward: `grep -c '^mod review;' tomlctl/src/lib.rs` prints `1` (today: `0`).
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^review::fields::/)'` passes the ten named tests — **predicted, unverified**.
  - falsifier: routing every cleanup row to `checker` turns `route_sends_executable_edits_and_test_removals_to_verifier` red — **predicted, unverified**.

### 2. Add the review store and anchor matcher [M]
- **Files**: `tomlctl/src/review/store.rs` (new), `tomlctl/src/review/matching.rs` (new), `tomlctl/src/review/mod.rs`
- **Depends on**: 1
- **Action**: Add the typed `Store` (`runs`, `cells`, `findings`, `verdicts`, root `id_high_water`), the slug/scope path and run-directory resolvers, the `mutate`/`read` helpers, and the anchor-candidate matcher; declare both modules in `tomlctl/src/review/mod.rs`.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group" (**Shape**). Model `store.rs` on `tomlctl/src/tasks/store.rs` (`mutate` delegates to `io::mutate_doc`, one `to_toml` write-back), with one writable view at a time: mint first on the raw document through `items::items_append_minted` with prefixes `U`, `C`, `F`, then parse the typed `Store` from it for typed edits. Seed with `io::schema_seed()`, honouring `--no-create` (never `on_missing_for`, whose seed is empty for this basename). `from_toml` accepts a date given as a string. Use `crate::fields::FieldArgs` and `io::read_text_arg` wherever later verbs take `--set`/`--set-json`/`--set-file` or `-`. `matching.rs` exposes `anchor` and `candidates` — within the family, `(file, symbol)` when symbol is non-empty, else `(file, line)` within ±10; it never decides identity. Do not reuse `tomlctl/src/anchor.rs` or `tomlctl/src/dedup.rs`. Tests: `round_trips_all_arrays`, `round_trips_id_high_water`, `minted_rows_survive_the_typed_write_back`, `string_dates_read_back_as_dates`, `candidates_prefer_symbol_over_line_window`, `candidates_within_ten_lines`, `candidates_partition_by_family`, `run_dir_uses_slug_or_scope_key`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^review::(store|matching)::/)'` passes — **predicted, unverified**.
  - falsifier: widening the line window in `candidates` to unbounded turns `candidates_within_ten_lines` red — **predicted, unverified**.

### 3. Add the lens catalogue loader, resolver and validator [M]
- **Files**: `tomlctl/src/review/catalogue.rs` (new), `tomlctl/src/review/mod.rs`
- **Depends on**: 2
- **Action**: Implement the catalogue core: load the four entry directories from the built-in root and the optional project root (TOML frontmatter between `+++` lines plus the body's `##` headings), load and merge both vocabularies, apply the project `config.toml` (`disable`, `enable`, `extend`, `override`, `scope`, `[[checks]]`), compute each entry's content hash over its versioned sections and the tree sha, resolve the claude dir and the default built-in root, and expose `validate()`, `index(filter)`, `owners()`, `detectors()`, `routing()`, `claude_dir()` and `bind_cell(concerns, family, signals_with_roots, slice, facet_edits, depth)`. Declare it `pub(crate) mod catalogue;` in `tomlctl/src/review/mod.rs` with `validate` `pub(crate)`, because the dispatch-test gate (task 46) calls it from `crate::cli::dispatch::tests`.
- **Detail**: Follow Approach, "Lens catalogue" — every validation rule there is one `CatalogueError` variant. `bind_cell` returns per concern the hash, path, methods and `v1_category`, plus facets attached only where the slice meets the facet's signal roots and the concern is in its `attaches_to`, with `scope` blocks applied by path. Validation compiles every `contains` pattern through `query::compile_user_bytes_regex` (the shipped build has no Unicode classes, so a pattern the test build would accept must still be checked this way) and checks `v1_category` against both closed sets. `claude_dir()` is `CLAUDE_CONFIG_DIR` when set, else `USERPROFILE` then `HOME` joined with `.claude`, as `glimpse/src/config.rs` resolves it, using `std` only. Add no dependency. Tests (temp-dir catalogues): `parses_toml_frontmatter_and_sections`, `project_id_colliding_with_builtin_is_refused`, `concern_tag_needs_exactly_one_owner_per_family`, `project_tags_need_the_project_prefix`, `missing_family_probes_section_is_refused`, `config_disable_enable_override_apply`, `scope_block_disables_by_path`, `facet_attaches_only_where_slice_meets_roots`, `content_hash_changes_with_probes_not_with_whitespace_outside_sections`, `deprecated_lens_is_refused_for_new_cells`, `uncompilable_contains_pattern_is_refused`, `claude_dir_prefers_claude_config_dir`.
- **Acceptance**:
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(/^review::catalogue::/)'` passes the twelve named tests — **predicted, unverified**.
  - falsifier: attaching facets by repo-wide signal presence turns `facet_attaches_only_where_slice_meets_roots` red — **predicted, unverified**.

### 4. Wire the review verb group into the CLI with stub verbs [L]
- **Files**: `tomlctl/src/cli/types/review.rs` (new), `tomlctl/src/cli/types/mod.rs`, `tomlctl/src/cli/types/cmd.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/review/mod.rs`, `tomlctl/src/review/dispatch.rs` (new), `tomlctl/src/review/runs.rs` (new), `tomlctl/src/review/survey.rs` (new), `tomlctl/src/review/cells.rs` (new), `tomlctl/src/review/add.rs` (new), `tomlctl/src/review/verdict.rs` (new), `tomlctl/src/review/resolve.rs` (new), `tomlctl/src/review/query.rs` (new), `tomlctl/src/review/digest.rs` (new), `tomlctl/src/review/triage.rs` (new), `tomlctl/src/review/stats.rs` (new), `tomlctl/src/review/sync.rs` (new), `tomlctl/src/review/export.rs` (new), `tomlctl/src/review/catalogue_cmd.rs` (new), `tomlctl/src/review/schema_out.rs` (new), `tomlctl/src/capabilities.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`, `tomlctl/src/cli/mod.rs`
- **Depends on**: 3
- **Action**: Declare the full `ReviewOp` clap tree with every verb and flag Approach, "v2 store and the tomlctl review group" lists, add `Cmd::Review`, route it through `tomlctl/src/review/dispatch.rs` to one stub function per verb module, declare the fourteen verb modules in `tomlctl/src/review/mod.rs`, and register capabilities features.
- **Detail**:
  - Copy the `inputs` and `backlog` idiom: `ReviewOp` re-exported from `tomlctl/src/cli/types/mod.rs`; `ReadIntegrityArgs` flattened on reads; `WriteIntegrityArgs` on writes. No `StampArgs`: like `tasks`, `backlog` and `inputs`, the review store stamps itself (`tomlctl/src/cli/types/shared.rs`).
  - Every verb except `schema` and `catalogue` takes `--slug` or `--scope` (mutually exclusive, one required); `stats` adds `--all`. Mirror that group in the `MUTEX_GROUPS` const of `tomlctl/src/capabilities.rs`, which supplements clap's `ArgGroup`s in the `capabilities` mutex data. Declare every `--<flag>-file` twin. Give `review plan`'s `--lens` `allow_hyphen_values = true` so `--lens -id` parses. Declare no flag that shadows a global output flag.
  - Add `"review"` to the README's `"subcommands"` sample array beside `SUBCOMMANDS`.
  - Each stub returns a `kind=validation` error `"review <verb>: not implemented"`; tasks 5–18 replace the bodies, keeping those tasks file-disjoint, which is why this task is L.
  - Features: `review_store`, `review_survey`, `review_cells`, `review_add`, `review_verdict`, `review_resolve`, `review_digest`, `review_triage`, `review_stats`, `review_sync_v1`, `review_export_v1`, `review_catalogue`, `review_schema` — in `FEATURES`, the expected list in `tomlctl/tests/capabilities.rs`, `SUBCOMMANDS`, and both README transcriptions, which `readme_feature_transcriptions_match_capabilities_features` gates.
  - Emit output only through `crate::output` helpers (`tomlctl/src/cli/dispatch/tests/output_gate.rs`).
- **Acceptance**:
  - forward: `grep -c 'Review' tomlctl/src/cli/types/cmd.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes — **predicted, unverified**.
  - falsifier: deleting `review_survey` from the README "Feature meanings" table turns `readme_feature_transcriptions_match_capabilities_features` red — **predicted, unverified**.

### 5. Implement review run start/update and schema emission [M]
- **Files**: `tomlctl/src/review/runs.rs`, `tomlctl/src/review/schema_out.rs`, `tomlctl/tests/review_runs.rs` (new)
- **Depends on**: 4
- **Action**: Implement `review run start` (mint `U<n>`, stamp `started`, resolve `--base` to a sha, compute `changed_lines`, record catalogue roots with the default built-in path from `catalogue::claude_dir()`, tree sha and config sha, validate the merged catalogue, create and print the run directory), `review run update` (whitelisted fields including `baselines`; `signals` upserted by `name`, which may only add `judged` entries or set `accepted`), and `review schema finding|cell-return|verdict-return [--kind]`.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group" (**Paths**, **Verbs**) and Approach, "Coverage proposal" for `changed_lines`. Resolve `--base` with `git rev-parse --verify --end-of-options <base>^{commit}`; create the run directory through `io::guard_write_path`; take `--set`/`--set-json` through `crate::fields::FieldArgs`. Schemas are built by hand from the `tomlctl/src/review/fields.rs` catalogue, with `if`/`then` for kind- and direction-conditional fields; a unit test `schema_agrees_with_field_catalogue` asserts the emitted required sets equal the catalogue's. Tests on the `tomlctl/tests/common/` sandbox (pass `--builtin` explicitly so no test reads the developer's claude dir): `run_start_mints_sequential_ids_and_creates_run_dir`, `run_start_records_catalogue_and_changed_lines`, `run_start_halts_on_invalid_project_catalogue`, `run_start_refuses_option_shaped_base`, `run_ids_never_reuse_a_removed_id`, `run_update_refuses_detector_signal_edits`, `run_update_upserts_signals_by_name`, `schema_defect_requires_failure_scenario`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_runs.rs && grep -c '#\[test\]' tomlctl/tests/review_runs.rs` prints at least `6` (today: prints nothing, exit 1).
  - falsifier: skipping catalogue validation at start turns `run_start_halts_on_invalid_project_catalogue` red — **predicted, unverified**.

### 6. Implement review survey with deterministic detectors [M]
- **Files**: `tomlctl/src/review/survey.rs`, `tomlctl/tests/review_survey.rs` (new)
- **Depends on**: 5
- **Action**: Implement `review survey --run <U>`: evaluate every `stack`, `diff` and `health` detector from the merged vocabulary over `git ls-files` and the run's `base..HEAD` diff (globs, content regexes, manifest keys including `Directory.Packages.props`, per-stack count patterns over `git log` windows with thresholds), and record each true signal with its `roots`.
- **Detail**: Follow Approach, "Lens catalogue" (**Vocabulary**). `count` patterns are literal substrings; `contains` is compiled through `query::compile_user_bytes_regex`; `manifest` key paths read TOML and JSON manifests only, and MSBuild XML is matched by `files` plus `contains`. A detector failing to evaluate (git unavailable, malformed regex) records the signal as `unknown` with the reason rather than false. Tests build fixture repos in temp dirs with test-local detector tables: `stack_signal_found_at_depth_three_with_roots`, `central_package_management_detects_efcore`, `polyglot_repo_records_separate_roots_per_stack`, `health_count_uses_per_stack_pattern_and_threshold`, `count_pattern_matches_literally`, `diff_signal_from_base_range`, `failed_detector_records_unknown`. The shipped vocabulary is tested by task 58.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_survey.rs && grep -c 'fn polyglot_repo_records_separate_roots_per_stack' tomlctl/tests/review_survey.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: limiting manifest search to depth 1 turns `stack_signal_found_at_depth_three_with_roots` red — **predicted, unverified**.

### 7. Implement review plan, the proposal, accounting and cover [M]
- **Files**: `tomlctl/src/review/cells.rs`, `tomlctl/tests/review_cells.rs` (new)
- **Depends on**: 6, 60
- **Action**: Implement `review plan --propose` (the default coverage map per Approach, "v2 store and the tomlctl review group"), `review plan --ndjson` (validate, bind each cell through `tomlctl/src/review/catalogue.rs` `bind_cell`, append with `wave` and status `assigned`), `review cover` (status, note, seal), and a `coverage_accounting(run)` function returning unselected matched concerns and unsliced scope files not covered by a waiver, for tasks 12 and 17.
- **Detail**: The proposal follows Approach, "Coverage proposal" exactly — ceiling formula, selection order, slicing by each lens's `slice`, bundling, repeat cells (`pass = 2`), plan-executor cells batched up to 4 tasks outside the ceiling, and the agent estimate; every row `reason = "proposed"`. `review cover` on a cell already sealed with the same status and note is a successful no-op. Tests: `propose_includes_floor_and_matched_signal_lenses`, `propose_respects_scaled_ceiling`, `propose_scope_without_diff_uses_line_count`, `propose_bundles_same_mode_concerns_only`, `propose_adds_reverse_order_repeat_cells_at_standard`, `plan_executor_cells_batch_tasks_outside_ceiling`, `plan_binds_hashes_and_root_scoped_facets`, `plan_refuses_mixed_mode_bundle`, `probe_cell_requires_question`, `accounting_lists_unselected_and_unsliced_until_waived`, `cover_seals_the_cell`, `cover_on_sealed_cell_with_same_note_is_a_no_op`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_cells.rs && grep -c 'fn accounting_lists_unselected_and_unsliced_until_waived' tomlctl/tests/review_cells.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: dropping candidates silently instead of listing them turns `accounting_lists_unselected_and_unsliced_until_waived` red — **predicted, unverified**.

### 8. Implement review add with cell-scoped idempotency and routing [M]
- **Files**: `tomlctl/src/review/add.rs`, `tomlctl/tests/review_add.rs` (new)
- **Depends on**: 7
- **Action**: Implement `review add --run <U> --cell <C> --ndjson`: validate rows through the field catalogue (`lens` ∈ the cell's concerns, path form), check `old` text near `line` (in `target_file` when given and existing), comparing after normalising `\r\n` to `\n` in both the file text and `old`, check a plan row's `anchor_old` occurs exactly once under its `plan_section`, compute `anchor`, candidates and `route` (via `fields::route` with the row's severity, the run's `depth` and the catalogue's routing globs), require `same_as` to name a candidate, supersede the cell's earlier unverified rows, mint defects and cleanup `unverified` and advisory `open` (or `merged` when `same_as` names a dismissed, deferred, parked, backlogged or converted target), and print each minted id with kind, severity and route; refuse on a sealed cell, printing its ids.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group" (**Concurrency and idempotency**, **Verbs** `review add`, **Anchor matcher**). Tests: `defect_without_failure_scenario_is_refused_rest_lands`, `cleanup_old_text_must_exist_near_line`, `add_direction_with_new_target_file_lands`, `distinct_defects_on_one_symbol_both_mint`, `resubmitted_cell_supersedes_unverified_rows`, `sealed_cell_refuses_and_prints_ids`, `executable_cleanup_routes_to_verifier`, `critical_defect_at_deep_routes_to_panel`, `advisory_same_as_dismissed_is_merged`, `lens_outside_cell_concerns_is_refused`, `cleanup_old_text_matches_in_crlf_file`, `plan_anchor_old_must_be_unique_in_section`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_add.rs && grep -c 'fn resubmitted_cell_supersedes_unverified_rows' tomlctl/tests/review_add.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: appending instead of superseding on resubmission turns `resubmitted_cell_supersedes_unverified_rows` red — **predicted, unverified**.

### 9. Implement review verdict as an upsert [M]
- **Files**: `tomlctl/src/review/verdict.rs`, `tomlctl/tests/review_verdict.rs` (new)
- **Depends on**: 8
- **Action**: Implement `review verdict <F>` and its `--ndjson` batch form: upsert on (finding, `by`, `seat`), apply the checker rules (`unsettled` re-routes to `verifier`; `refuted` only on a sweeper row with a citation), record an optional `severity` reassessment, honour `--strike-same-as`, and resolve status immediately for findings without `same_as` (panel: ≥2 non-refuted of 3 → `open`).
- **Detail**: Follow Approach, "v2 store and the tomlctl review group" (**Verbs** `review verdict`). Findings with `same_as` stay `unverified` for task 10. A verdict without a citation is refused. Tests: `confirmed_unverified_becomes_open`, `refuted_unverified_becomes_refuted`, `replayed_verdict_upserts_not_appends`, `checker_unsettled_reroutes_to_verifier`, `checker_cannot_refute_a_finder_row`, `panel_needs_two_non_refuted`, `severity_reassessment_is_recorded`, `verdict_without_citation_is_refused`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_verdict.rs && grep -c 'fn replayed_verdict_upserts_not_appends' tomlctl/tests/review_verdict.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: letting a checker refute a `review-finder` row turns `checker_cannot_refute_a_finder_row` red — **predicted, unverified**.

### 10. Implement review resolve for identity outcomes [M]
- **Files**: `tomlctl/src/review/resolve.rs`, `tomlctl/tests/review_resolve.rs` (new)
- **Depends on**: 9
- **Action**: Implement `review resolve --run <U> [--wave n]`: for every finding with a `same_as` whose verdicts are complete, in dependency order, apply the repeat, contested, reopened, regression and suppressed outcomes per Approach, "v2 store and the tomlctl review group".
- **Detail**: Deterministic and idempotent; a target from the same run resolves first. Each resolved row records its `outcome` and the row and target statuses the Approach states for that outcome (contested and reopened rows become `merged` with `merged_into`). Tests: `same_as_repeat_merges_and_appends_seen_in_once`, `contested_row_merges_and_records_outcome`, `refuted_same_as_on_open_is_contested`, `confirmed_same_as_on_refuted_reopens`, `confirmed_same_as_on_resolved_is_regression`, `same_as_on_parked_is_suppressed`, `same_run_target_resolves_first_regardless_of_verdict_order`, `resolve_is_idempotent`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_resolve.rs && grep -c 'fn same_run_target_resolves_first_regardless_of_verdict_order' tomlctl/tests/review_resolve.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: resolving in verdict-arrival order turns `same_run_target_resolves_first_regardless_of_verdict_order` red — **predicted, unverified**.

### 11. Implement review list and role-projected show [M]
- **Files**: `tomlctl/src/review/query.rs`, `tomlctl/tests/review_query.rs` (new)
- **Depends on**: 9
- **Action**: Implement `review list` (`--kind`, `--lens`, `--run`, `--cell`, `--status`, `--route`, `--in <glob>`) and `review show <ids> --for <role>` for findings and cells, including the `audit` projection with ±5 lines around each cited `file:line`.
- **Detail**: Project only through `tomlctl/src/review/fields.rs` `project`. A cell `--for finder` carries its slice, concerns with bound paths, facets and methods with paths, question, pack path and the run's recorded `baselines`; `--for finder|verifier|checker` also carries the absolute paths of `review-v2-method`'s `references/*.md` under `catalogue::claude_dir()`. The `implementer` projection carries a plan row's `plan_section`, `anchor_old` and `anchor_new`. The `audit` projection reads cited lines after normalising `\r\n`. The `backlog` projection emits the `backlog check`/`add` NDJSON object shape (`kind` from `angle`: `challenge`/`alternative`/`direction` → `direction`, `risk`/`probe` → `question`; `area` = the finding's directory; `origin = "review"`). Tests: `list_in_glob_filters_prior`, `show_for_verifier_omits_grade_and_fix`, `show_for_audit_inlines_cited_lines`, `show_for_backlog_emits_backlog_shape`, `show_cell_carries_bound_catalogue_paths`.
- **Acceptance**: forward: `test -f tomlctl/tests/review_query.rs && grep -c 'fn show_for_audit_inlines_cited_lines' tomlctl/tests/review_query.rs` prints `1` (today: prints nothing, exit 1).

### 12. Implement review digest and review report [M]
- **Files**: `tomlctl/src/review/digest.rs`, `tomlctl/tests/review_digest.rs` (new)
- **Depends on**: 7, 10
- **Action**: Implement `review digest --run <U> [--since-wave n]` and `review report --run <U> --out <path>` per Approach, "v2 store and the tomlctl review group".
- **Detail**: The digest lists unverified ids with routes, unselected and unsliced items from task 7's `coverage_accounting`, contested findings (rows whose `outcome` is `contested`, with their targets), refuted criticals, the audit sample chosen by index, and the advisory list only until triaged; it carries no `evidence`. The report writes to `--out` and prints only the path and a counts line. Tests: `digest_lists_unverified_ids_with_routes`, `digest_lists_unwaived_unselected_concerns`, `digest_since_wave_limits_rows`, `digest_omits_evidence`, `report_writes_file_and_prints_counts`, `report_is_byte_stable`.
- **Acceptance**: forward: `test -f tomlctl/tests/review_digest.rs && grep -c 'fn digest_lists_unwaived_unselected_concerns' tomlctl/tests/review_digest.rs` prints `1` (today: prints nothing, exit 1).

### 13. Implement review triage with park, conversion and backlog refs [M]
- **Files**: `tomlctl/src/review/triage.rs`, `tomlctl/tests/review_triage.rs` (new), `tomlctl/src/review/add.rs`, `tomlctl/src/review/fields.rs`
- **Depends on**: 9
- **Action**: Implement `review triage <ids>` with `--dismiss`/`--defer --trigger`/`--reopen`/`--park`/`--resolve`/`--backlog-ref`/`--convert cleanup|defect --ndjson`, the `--<flag>-file` twins and `--expect-status` compare-and-set.
- **Detail**: Implement `--expect-status` as a typed status check inside the store closure, modelled on `tomlctl/src/ledgers.rs` `write_guarded`. `--convert` validates the new row through the field catalogue, mints it with an `orchestrator` verdict and its computed `route`, and marks the advisory `converted` in the same write. Tests: `triage_dismiss_requires_reason`, `triage_honours_expect_status`, `triage_reads_reason_from_file`, `park_marks_advisory_parked`, `convert_advisory_mints_routed_cleanup`, `backlog_ref_marks_backlogged`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_triage.rs && grep -c 'fn convert_advisory_mints_routed_cleanup' tomlctl/tests/review_triage.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: ignoring `--expect-status` turns `triage_honours_expect_status` red — **predicted, unverified**.

### 14. Implement review sync-v1 [M]
- **Files**: `tomlctl/src/review/sync.rs`, `tomlctl/tests/review_sync.rs` (new)
- **Depends on**: 9
- **Action**: Implement `review sync-v1`: read every v1 row carrying a `v2_ref` into this store (every distinct `export_ledger` recorded on this store's exported findings, plus the family's default ledger for the store's slug or scope) and map its status onto the v2 finding with a `by = "sync"` verdict. Expose the status mapping as a function task 15 reuses read-only.
- **Detail**: Mapping and the sync verdict's shape (`verdict = "confirmed"`, `citation = "<ledger path>#<v1 id>"`, `rationale` the v1 status and resolution) per Approach, "v2 store and the tomlctl review group" (**Verbs** `review sync-v1`); idempotent; v1 files are read only. Tests: `fixed_v1_row_resolves_v2_finding`, `wontfix_v1_row_dismisses_with_reason`, `sync_reads_recorded_export_ledger`, `sync_verdict_carries_ledger_citation`, `sync_is_idempotent`, `missing_v1_ledger_is_a_no_op`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_sync.rs && grep -c 'fn fixed_v1_row_resolves_v2_finding' tomlctl/tests/review_sync.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: skipping rows whose v1 status is `applied` turns `fixed_v1_row_resolves_v2_finding`'s applied case red — **predicted, unverified**.

### 15. Implement review stats with outcomes, escapes and calibration [M]
- **Files**: `tomlctl/src/review/stats.rs`, `tomlctl/tests/review_stats.rs` (new)
- **Depends on**: 10, 14
- **Action**: Implement `review stats [--slug|--scope|--all] [--family] [--by lens|probe|run] [--outcomes] [--escapes] [--calibration [--exemplars N]]` per Approach, "v2 store and the tomlctl review group" and Approach, "Measurement and tuning".
- **Detail**: Group by lens id with the bound hash as a column; probes group by their concern tag. `stats` writes nothing: `--outcomes` reads v1 statuses through `v2_ref` with task 14's mapping function, read-only. `--escapes` counts findings minted in a (file, concern) an earlier run had sealed `covered`. `--calibration` draws exemplars only from user `dismiss_reason`s, v1 wontfix reasons and audit-overturned verdicts, first sentence each, most frequent first. `--all` globs `.claude/flows/*/review-v2.toml` and `.claude/review-v2/*.toml`. Tests: `stats_refute_and_audit_overturn_rates_per_lens`, `stats_outcomes_join_v1_status`, `stats_escapes_count_refinds_in_covered_cells`, `calibration_ignores_unaudited_refutations`, `stats_probe_groups_by_concern_tag`, `stats_all_aggregates_across_stores`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_stats.rs && grep -c 'fn calibration_ignores_unaudited_refutations' tomlctl/tests/review_stats.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: feeding raw verifier rationales into calibration turns `calibration_ignores_unaudited_refutations` red — **predicted, unverified**.

### 16. Implement review export-v1 [M]
- **Files**: `tomlctl/src/review/export.rs`, `tomlctl/tests/review_export.rs` (new)
- **Depends on**: 11
- **Action**: Implement `review export-v1 (<ids> | --all) --to <v1-ledger> [--dry-run]` per Approach, "Export to v1".
- **Detail**: Build `description` from the `implementer` projection (task 11). Take `category` from the cell binding of the finding's lens, and `effort` from the finding itself (defect and cleanup both carry one; `large` is refused). `--all` excludes `needs-plan`, test removals and PLAUSIBLE cleanup. Validate the v1 required set (`id` by family prefix, `file`, `line`, `severity`, `effort` ∈ trivial/small/medium, `category`, `summary`, `first_flagged`, `rounds`, `status = "open"`). Tests: `export_refuses_advisory_and_needs_plan`, `export_all_skips_test_removals_and_plausible_cleanup`, `export_uses_reassessed_severity`, `export_category_from_cell_binding`, `export_reuses_open_v1_id_and_bumps_rounds_once`, `export_is_idempotent_on_v2_ref`, `export_regression_on_fixed_v1_match`, `export_stamps_last_updated_from_run_date`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_export.rs && grep -c 'fn export_all_skips_test_removals_and_plausible_cleanup' tomlctl/tests/review_export.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: bumping `rounds` on every export call turns `export_reuses_open_v1_id_and_bumps_rounds_once` red — **predicted, unverified**.

### 17. Implement review run finish with its completeness gate [S]
- **Files**: `tomlctl/src/review/runs.rs`, `tomlctl/tests/review_finish.rs` (new)
- **Depends on**: 5, 7, 10
- **Action**: Implement `review run finish <U> [--incomplete <reason>]`: stamp `finished`, compute `counts`, and refuse while any finding is `unverified`, any `same_as` is unresolved, or task 7's `coverage_accounting` reports an unwaived item — unless `--incomplete` records a reason.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group" (**Verbs** `review run finish`). `counts.suppressed` counts rows whose `outcome` is `suppressed`. Tests: `finish_refuses_unverified_findings`, `finish_refuses_unwaived_coverage`, `finish_incomplete_records_reason`, `finish_computes_counts`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_finish.rs && grep -c 'fn finish_refuses_unwaived_coverage' tomlctl/tests/review_finish.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: dropping the accounting check turns `finish_refuses_unwaived_coverage` red — **predicted, unverified**.

### 18. Implement the review catalogue verb [S]
- **Files**: `tomlctl/src/review/catalogue_cmd.rs`, `tomlctl/tests/review_catalogue.rs` (new)
- **Depends on**: 4
- **Action**: Implement `tomlctl review catalogue [--builtin <dir>] [--project <dir>] [--family <f>] [--signals <a,b>] [--vocabulary] [--owners] [--check]` over `tomlctl/src/review/catalogue.rs`.
- **Detail**: Follow Approach, "Lens catalogue" (**Generated index and validation**). `--builtin` defaults to `~/.claude/skills/review-v2-lenses`; `--project` to `.claude/review-lenses` under the repo root when present. Tests (temp-dir catalogues): `catalogue_prints_merged_index_with_hashes`, `catalogue_signals_marks_matched`, `catalogue_owners_prints_generated_map`, `catalogue_check_exits_nonzero_on_invalid`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_catalogue.rs && grep -c 'fn catalogue_check_exits_nonzero_on_invalid' tomlctl/tests/review_catalogue.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: making `--check` exit 0 on errors turns `catalogue_check_exits_nonzero_on_invalid` red — **predicted, unverified**.

### 19. Attribute review-v2 store prompts in agent correlation [S]
- **Files**: `tomlctl/src/agents/correlate.rs`
- **Depends on**: —
- **Action**: Extend the `LEDGER` regex in `tomlctl/src/agents/correlate.rs` (around line 201) to accept the `review-v2.toml` basename, and add `ledger_line_attributes_review_v2_store` beside the existing in-module tests.
- **Detail**: Basename only — v2 dispatch prompts carry run and cell ids, not F-ids. Only the flow-local `.claude/flows/<slug>/review-v2.toml` attributes a flow. Task 49's workflow and task 48's fallback build the prompts that carry the line.
- **Acceptance**:
  - forward: `grep -c 'review-v2' tomlctl/src/agents/correlate.rs` prints at least `1` (today: `0`).
  - falsifier: reverting the alternation turns `ledger_line_attributes_review_v2_store` red — **predicted, unverified**.

### 20. Add output-option cases and help-list entries for the review verbs [M]
- **Files**: `tomlctl/tests/output_options.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/src/review/digest.rs`, `tomlctl/src/review/stats.rs`
- **Depends on**: 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 61
- **Action**: Give every `review` leaf a case in `cases()` of `tomlctl/tests/output_options.rs`, add a `review` arm to `group()` and the `group_tests!` list, and seed a `review-v2.toml` and a minimal catalogue in `fixture()`. Add the read verbs to `read_only_subcommands_hide_write_integrity_flags_in_help` and the write verbs to `write_subcommands_expose_all_integrity_flags_in_help` in `tomlctl/tests/capabilities.rs`.
- **Detail**: `every_leaf_command_has_a_case` walks `tomlctl capabilities .commands`, so the full suite is red from task 4 until this lands. Leaves: `sync-v1`, `run start`, `run update`, `run finish`, `survey`, `plan`, `cover`, `add`, `verdict`, `resolve`, `list`, `show`, `digest`, `report`, `triage`, `stats`, `export-v1`, `catalogue`, `schema`. Help-list placement: **write** — `sync-v1`, `run start`, `run update`, `run finish`, `survey`, `plan` (its `--ndjson` mode writes), `cover`, `add`, `verdict`, `resolve`, `triage`, `export-v1`; **read** — `list`, `show`, `digest`, `report` (a derived markdown write with no sidecar, like `tasks render`), `stats`, `catalogue`, `schema`. Cases pass `--builtin` and `--project` pointing at the fixture catalogue, so no case reads the developer's claude dir.
- **Acceptance**:
  - forward: `grep -c '&\["review", ' tomlctl/tests/output_options.rs` prints at least `19` (today: `0`).
  - falsifier: deleting the `review resolve` case turns `every_leaf_command_has_a_case` red — **predicted, unverified**.

### 21. Document the review verb group in the tomlctl skill [M]
- **Files**: `claude/skills/tomlctl/references/review.md` (new), `claude/skills/tomlctl/SKILL.md`, `tomlctl/README.md`
- **Depends on**: 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18
- **Action**: Write the `review` flag-table reference — one `### \`tomlctl review <verb>\`` heading and `| Flag |` table per verb, plus the store shape, the field catalogue summary, the role list and the routing rules. In `claude/skills/tomlctl/SKILL.md` add a Quick Reference row, a References entry and "Rows and header per command" rows for `review list`, `review show`, `review digest`, `review stats`, `review catalogue` and `review export-v1`. In `tomlctl/README.md` add `tomlctl review …` lines to the `## Usage` Quick-tour fence.
- **Detail**: Do **not** edit the `description:` of `claude/skills/tomlctl/SKILL.md` (14 characters below its 1024-character ceiling; measure with `awk`). Bump the `"version":"0.15.0"` capabilities example in its body to `0.16.0`, matching task 22. Every `tomlctl review …` line in a bash fence must parse (`command_lint`), every flag-table row must be a real flag (`flag_table_lint`), and a heading names one verb path only. Keep the reference under 600 lines.
- **Acceptance**:
  - forward: `test -f claude/skills/tomlctl/references/review.md && grep -cE '^### `tomlctl review ' claude/skills/tomlctl/references/review.md` prints at least `19` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 22. Bump tomlctl to 0.16.0 and refresh the glimpse lockfile [L]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 4, 20, 21
- **Action**: Set `version = "0.16.0"` in `tomlctl/Cargo.toml`, update the version literal and message in `capabilities_version_matches_cargo_toml` (`tomlctl/tests/capabilities.rs`), update the `"version"` of the `capabilities` sample block in `tomlctl/README.md`, then refresh both lockfiles (`cargo tree --manifest-path glimpse/Cargo.toml >/dev/null` rewrites glimpse's).
- **Detail**: The pre-commit glimpse clippy runs `--locked`, so the refreshed `glimpse/Cargo.lock` is staged with the bump. `readme_sample_version_matches_cargo_toml` gates the README edit. The five edits are inseparable for a green tree, hence L.
- **Acceptance**:
  - forward: `grep -c '^version = "0.16.0"' tomlctl/Cargo.toml` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes, including `capabilities_version_matches_cargo_toml` and `readme_sample_version_matches_cargo_toml` — **predicted, unverified**.

### 23. Write the review-v2-method skill (shared core) [M]
- **Files**: `claude/skills/review-v2-method/SKILL.md` (new), `claude/skills/review-v2-method/references/finding-kinds.md` (new), `claude/skills/review-v2-method/references/verifying.md` (new)
- **Depends on**: 4
- **Action**: Write the shared core every v2 agent preloads: the three kinds with their required fields and the defect/cleanup/advisory decision rule, `same_as`, the never-a-finding list, caps, the coverage note, the probe template for `probe:` cells, the store-write idiom (one full-set `add` per cell, sealing, the payload staged with the Write tool in the run directory's `payloads/` and passed as `--ndjson <path>`, never piped or heredoc'd, retry rules) and each role's return object. Role procedures live in the agent bodies (tasks 50–51), not here. The references hold worked examples only: one row per kind, direction and family in `finding-kinds.md`; one verdict per outcome and one reassessed severity in `verifying.md`.
- **Detail**: Follow Approach, "Finding kinds", Approach, "Field readership" and Approach, "Agents". Defer source hierarchy, evidence grades and gated references to the unchanged `research-methods` skill by name; state that the Counter line and grade go in `counter` and `grade`. State that the store is read only through `tomlctl review list|show`, never `tomlctl items list`. Worked examples are reached through the absolute reference paths `review show --for <role>` carries, because a preload reports no base directory. Name the confirmation exclusion as v1's top drop class. Never use the words "target" or "quota" for counts. The description says when to use it (any v2 review agent) and stays under 1024 characters; the body stays under 250 lines so Haiku agents are not crowded. Every `tomlctl review …` line in a bash fence must parse.
- **Acceptance**:
  - forward: `test -f claude/skills/review-v2-method/SKILL.md && grep -c 'fix_shape' claude/skills/review-v2-method/SKILL.md` prints at least `1` (today: prints nothing, exit 1).
  - forward: `test -f claude/skills/review-v2-method/SKILL.md && test "$(wc -l < claude/skills/review-v2-method/SKILL.md)" -le 250 && ! grep -rqiE '\btarget [0-9]+|quota' claude/skills/review-v2-method && echo ok` prints `ok` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 24. Write the catalogue skill and vocabulary [M]
- **Files**: `claude/skills/review-v2-lenses/SKILL.md` (new), `claude/skills/review-v2-lenses/vocabulary.toml` (new)
- **Depends on**: 4
- **Action**: Write `SKILL.md` — the four entry types with their frontmatter schemas and body section orders, activation tiers, the layers and the project `config.toml` (`disable`, `enable`, `extend`, `override`, `scope`, `[[checks]]`) with a worked example, the project vocabulary and `project:` prefix, the collision refusal, content hashing, the lifecycle and promotion path, the probe template's relation to lenses, listing the catalogue (`tomlctl review catalogue`), and the lumina mapping. Write `vocabulary.toml` — every signal with `kind`, `meaning` and a declarative `detect` table (or a `hint` for judged signals), every concern tag with `meaning` and `families`, and the `[routing]` globs.
- **Detail**: Follow Approach, "Lens catalogue". `count` patterns are literal substrings; `contains` regexes use ASCII forms only (`(?-u:\b)`, no `\w`/`\d`/`\s` shorthand); `manifest` key paths only for TOML and JSON manifests, with MSBuild XML matched by `files` plus `contains`. Detectors must cover the user's real layouts: manifests below depth 1, `Directory.Packages.props`, Bun workspaces, legacy .NET `<Reference Include=…>`, and per-stack test patterns for Rust, .NET and TS. Concern tags cover every lens in the table's "From" column, including the cross-cutting plan concerns and `/review-plan` Agent 3's whole-plan concerns. The description says when to use it (authoring or maintaining catalogue entries, configuring a project) and stays under 1024 characters; body under 500 lines. Every `tomlctl review …` line in a bash fence must parse.
- **Acceptance**: forward: `test -f claude/skills/review-v2-lenses/vocabulary.toml && grep -c '^\[concerns\.' claude/skills/review-v2-lenses/vocabulary.toml` prints at least `31` (today: prints nothing, exit 1).

### 25. Write the methods: sweep, trace, history-idiom [M]
- **Files**: `claude/skills/review-v2-lenses/methods/sweep.md` (new), `claude/skills/review-v2-lenses/methods/trace.md` (new), `claude/skills/review-v2-lenses/methods/history-idiom.md` (new)
- **Depends on**: 24
- **Action**: Write three method entries in the catalogue format.
- **Detail**: `sweep` carries the "Sweep for every instance" procedure of `claude/skills/research-methods/references/codebase-review.md` (derive the sweep from the defect, cover the forms, search the whole tree, report set + search + state, `tomlctl sweep`) plus `/review-plan`'s "one line per sweep, even when it finds nothing". `trace` carries "Contract first" and "Trace before you claim". `history-idiom` carries "Idiom questions are history questions". Rewrite rather than copy; reference the v1 skill by name only.
- **Acceptance**: forward: `for f in sweep trace history-idiom; do p=claude/skills/review-v2-lenses/methods/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 26. Write the methods: measure, dry-run, steelman [M]
- **Files**: `claude/skills/review-v2-lenses/methods/measure.md` (new), `claude/skills/review-v2-lenses/methods/dry-run.md` (new), `claude/skills/review-v2-lenses/methods/steelman.md` (new)
- **Depends on**: 24
- **Action**: Write three method entries.
- **Detail**: `measure` is the perf evidence ladder (benchmark or profile → complexity argument on a real input size → allocation count → hypothesis), with "run a benchmark that exists, never build one". `dry-run` is the plan-executor procedure: given only the plan preamble sections and one task (`tomlctl tasks show <id> --with body,files,deps`), list every decision the task leaves to its executor, the files the work would touch against the **Files** line, acceptance commands not runnable as written, and premises about current behaviour the task asserts without the code to confirm them. `steelman` is the contrarian procedure from lumina's `research-explore` contrarian definition (cite a competing approach, a counter-example or a documented failure — never generic "consider alternatives").
- **Acceptance**: forward: `for f in measure dry-run steelman; do p=claude/skills/review-v2-lenses/methods/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 27. Write the lenses: correctness, type-design, domain-model [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/correctness.md` (new), `claude/skills/review-v2-lenses/lenses/type-design.md` (new), `claude/skills/review-v2-lenses/lenses/domain-model.md` (new)
- **Depends on**: 24
- **Action**: Write three lens entries in the catalogue format, with frontmatter per Approach, "Lens catalogue" (lens table) and `owns` drawn from `claude/skills/review-v2-lenses/vocabulary.toml`.
- **Detail**: `correctness` (floor) works the boundary list of `claude/skills/research-methods/references/codebase-review.md` (inputs, error paths, concurrency and ordering, resource lifetimes, cross-layer invariants) plus `claude/commands/review.md` Agent 4's edge cases and missing handling, and documents the context-free diff-only variant. `type-design` and `domain-model` split `claude/commands/review.md` Agents 1 and 3 as the lens table says. Seed every Does not flag section from the v1 rejection clusters and the source agent's own "Do not flag" sentence. Ownership collisions with sibling lens tasks are settled at task 46.
- **Acceptance**: forward: `for f in correctness type-design domain-model; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 28. Write the lenses: architecture, idioms, duplication [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/architecture.md` (new), `claude/skills/review-v2-lenses/lenses/idioms.md` (new), `claude/skills/review-v2-lenses/lenses/duplication.md` (new)
- **Depends on**: 24
- **Action**: Write three lens entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `architecture` from `claude/commands/review.md` Agent 3 (layering, placement, evolvability; mostly advisory, cleanup for edits-only fixes). `idioms` from Agent 1, citing `history-idiom` so it never canonises an outlier. `duplication` from Agent 1's DRY plus the method notes' "facts with one owner, rule of three"; a duplicated helper whose extraction needs only edits is cleanup.
- **Acceptance**: forward: `for f in architecture idioms duplication; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 29. Write the lenses: security, external-validity, hygiene [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/security.md` (new), `claude/skills/review-v2-lenses/lenses/external-validity.md` (new), `claude/skills/review-v2-lenses/lenses/hygiene.md` (new)
- **Depends on**: 24
- **Action**: Write three lens entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `security` (code and plan probes; signals `trust-boundary`, `dependency-change`; hard cap 5) from `claude/commands/review.md` Agent 2. `external-validity` (code and plan) merges Agent 2's advisory lookups for added or bumped dependencies with `claude/commands/review-plan.md` Agent 4's pinned-API, deprecation and advisory checks. `hygiene` (mechanical) owns only grep-provable cleanup — dead code traced by reference search, stale TODOs, unused config and imports; comment rot belongs to `doc-drift`.
- **Acceptance**: forward: `for f in security external-validity hygiene; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 30. Write the lenses: ripple, test-gaps, seams [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/ripple.md` (new), `claude/skills/review-v2-lenses/lenses/test-gaps.md` (new), `claude/skills/review-v2-lenses/lenses/seams.md` (new)
- **Depends on**: 24
- **Action**: Write three lens entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `ripple` (mechanical, floor for code and plan, cites `sweep`) merges `claude/commands/review.md` Agent 4's ripple with `claude/commands/review-plan.md` Agent 2 sweeps 1–3 under separate code and plan probe sections. `test-gaps` (code and plan) aims at the regression risk the change carries (`claude/commands/review.md` Agent 5; `review-plan.md` Agent 2's missing tests) and writes add-direction cleanup with `target_file`. `seams` from Agent 5's seams.
- **Acceptance**: forward: `for f in ripple test-gaps seams; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 31. Write the lenses: diagnostics-dx, test-pruning, doc-drift [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/diagnostics-dx.md` (new), `claude/skills/review-v2-lenses/lenses/test-pruning.md` (new), `claude/skills/review-v2-lenses/lenses/doc-drift.md` (new)
- **Depends on**: 24
- **Action**: Write three lens entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `diagnostics-dx` (judgement) from `claude/commands/review.md` Agent 5's error messages, logs and help text. `test-pruning` (signals `test-growth`, `test-ratio-high`) yields `direction = remove` cleanup for tests that only re-assert the implementation, duplicate another test, or pin incidental behaviour, and states the coverage each removal gives up (these route to the Opus verifier). `doc-drift` (judgement; signal `docs-touched`) owns comment rot and prose drift against behaviour.
- **Acceptance**: forward: `for f in diagnostics-dx test-pruning doc-drift; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 32. Write the lenses: doc-consolidation, contrarian, synthesis [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/doc-consolidation.md` (new), `claude/skills/review-v2-lenses/lenses/contrarian.md` (new), `claude/skills/review-v2-lenses/lenses/synthesis.md` (new)
- **Depends on**: 24
- **Action**: Write three lens entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `doc-consolidation` (signal `doc-growth`) yields merges and removals of duplicated docs. `contrarian` (all three families; signals `public-api-change`, `dependency-change`, `new-seam`; `always_at = "deep"`; advisory only; cites `steelman`) carries lumina's `research-explore` contrarian definition and `story-review`'s argue-against and scope-conservatism angles. `synthesis` (all families; opt-in; `after_wave = 1`) reads `review digest` and the scope and reports only cross-cell emergent defects and "what every cell missed"; its perf probes carry `claude/commands/optimise.md`'s cross-cutting concurrency review.
- **Acceptance**: forward: `for f in doc-consolidation contrarian synthesis; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 33. Write the perf lenses: memory-runtime, data-shape, data-access [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/memory-runtime.md` (new), `claude/skills/review-v2-lenses/lenses/data-shape.md` (new), `claude/skills/review-v2-lenses/lenses/data-access.md` (new)
- **Depends on**: 24
- **Action**: Write three perf lens entries (Approach, "Lens catalogue", **Writing an entry**) from `claude/commands/optimise.md` Agents 1–3.
- **Detail**: Follow Approach, "Family specifics". Each cites `measure`; a perf defect states workload → cost with magnitude, otherwise it is advisory (`angle = risk`). Stack-specific text in the v1 agents (frontend reactivity, ORM pitfalls) moves to facets (tasks 38–40), not into these files. Keep each lens's `research_domain` from its v1 agent.
- **Acceptance**: forward: `for f in memory-runtime data-shape data-access; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 34. Write the perf lenses: algorithm, concurrency, io-process [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/algorithm.md` (new), `claude/skills/review-v2-lenses/lenses/concurrency.md` (new), `claude/skills/review-v2-lenses/lenses/io-process.md` (new)
- **Depends on**: 24
- **Action**: Write three perf lens entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `algorithm` (floor for perf) carries `claude/commands/optimise.md` Agent 4, keeping the boundary that domain modelling belongs to `type-design`. `concurrency` keeps Agent 5's coverage and severity calibration (mapped to critical/major/minor). `io-process` covers process start-up, syscalls, filesystem walks and subprocess fan-out. Stack-specific text goes to facets.
- **Acceptance**: forward: `for f in algorithm concurrency io-process; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 35. Write the plan lenses: plan-grounding, plan-dependencies, feasibility [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/plan-grounding.md` (new), `claude/skills/review-v2-lenses/lenses/plan-dependencies.md` (new), `claude/skills/review-v2-lenses/lenses/feasibility.md` (new)
- **Depends on**: 24
- **Action**: Write three plan lens entries (Approach, "Lens catalogue", **Writing an entry**) from `claude/commands/review-plan.md` Step 2.
- **Detail**: `plan-grounding` (floor) carries Agent 2's alignment, sweeps 4–5 (re-derived enumerations, behavioural premises measured), its cross-cutting concerns (logging, error handling, authz, cache invalidation) and its config/migration/build changes. `plan-dependencies` (floor) carries Agent 1's edge-completeness and checkpoint-placement judgement, reading `STORE CHECK` as ground truth for stated edges. `feasibility` carries Agent 1's feasibility and coherence judgement.
- **Acceptance**: forward: `for f in plan-grounding plan-dependencies feasibility; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 36. Write the plan lenses: plan-executor, plan-risk, plan-premortem [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/plan-executor.md` (new), `claude/skills/review-v2-lenses/lenses/plan-risk.md` (new), `claude/skills/review-v2-lenses/lenses/plan-premortem.md` (new)
- **Depends on**: 24
- **Action**: Write three plan lens entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `plan-executor` (floor, mode `executor`, slice `per-task`, cites `dry-run`) also carries `claude/commands/review-plan.md` Agent 3's per-task executability checks (Files↔body parity, executor-time decisions, sizing). `plan-risk` carries Agent 4's rollback adequacy, risky-task checkpoint placement, back-compat and scope realism. `plan-premortem` is `status = "experimental"`, activation `opt-in`, with an "experimental — weak evidence" line.
- **Acceptance**: forward: `for f in plan-executor plan-risk plan-premortem; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 37. Write the plan-structure lens [S]
- **Files**: `claude/skills/review-v2-lenses/lenses/plan-structure.md` (new)
- **Depends on**: 24
- **Action**: Write the whole-plan `plan-structure` lens (plan, judgement, floor, slice `whole-scope`).
- **Detail**: It carries `claude/commands/review-plan.md` Agent 3's whole-plan half: goal coverage (Success Criteria verify the goal `## Context` states; a criterion checking only that tasks ran), the approver sections and the `**Status**`-header severity rule, structure judged against the `FORMAT CONTRACT` fence, Execution Policy field checks, and "suggest restructuring if prose-format" for foreign plans.
- **Acceptance**: forward: `test -f claude/skills/review-v2-lenses/lenses/plan-structure.md && grep -qx 'id = "plan-structure"' claude/skills/review-v2-lenses/lenses/plan-structure.md && echo ok` prints `ok` (today: prints nothing, exit 1).

### 38. Write the facets: rust, tokio, sqlx [M]
- **Files**: `claude/skills/review-v2-lenses/facets/rust.md` (new), `claude/skills/review-v2-lenses/facets/tokio.md` (new), `claude/skills/review-v2-lenses/facets/sqlx.md` (new)
- **Depends on**: 24
- **Action**: Write three facet entries in the catalogue format, each with a general section and a `### For <lens-id>` section per attached lens.
- **Detail**: `rust` (`lang:rust`) attaches to correctness, type-design, idioms, memory-runtime, algorithm and hygiene; `tokio` (`runtime:tokio`) to concurrency and correctness, carrying `claude/commands/optimise.md` Agent 5's runtime specifics; `sqlx` (`db:sqlx`) to data-access and domain-model, including the never-edit-an-applied-migration rule. Probes are concrete pitfalls, not tutorials.
- **Acceptance**: forward: `for f in rust tokio sqlx; do p=claude/skills/review-v2-lenses/facets/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 39. Write the facets: typescript, react, vue [M]
- **Files**: `claude/skills/review-v2-lenses/facets/typescript.md` (new), `claude/skills/review-v2-lenses/facets/react.md` (new), `claude/skills/review-v2-lenses/facets/vue.md` (new)
- **Depends on**: 24
- **Action**: Write three facet entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `react` and `vue` carry the frontend text of `claude/commands/optimise.md` Agents 1 and 4 (reactive-object overhead, component proliferation, bundle size, tree-shaking barriers, reactive dependency chains, reconciliation cost) for memory-runtime and algorithm, plus correctness and idioms probes. `typescript` (`lang:ts`) attaches to type-design, correctness and idioms.
- **Acceptance**: forward: `for f in typescript react vue; do p=claude/skills/review-v2-lenses/facets/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 40. Write the facets: dotnet, efcore, dapper-sql [M]
- **Files**: `claude/skills/review-v2-lenses/facets/dotnet.md` (new), `claude/skills/review-v2-lenses/facets/efcore.md` (new), `claude/skills/review-v2-lenses/facets/dapper-sql.md` (new)
- **Depends on**: 24
- **Action**: Write three facet entries (Approach, "Lens catalogue", **Writing an entry**).
- **Detail**: `dotnet` (`lang:csharp`) attaches to correctness, type-design, idioms, memory-runtime (`/optimise` Agent 1's boxing, devirtualisation and JIT/AOT notes) and concurrency. `efcore` (`db:efcore`) carries `/optimise` Agent 3's ORM query-translation pitfalls (compiled queries, tracking, N+1, pagination) for data-access and domain-model. `dapper-sql` (`db:dapper`, raw `.sql` files) covers parameterisation and SQL injection for security, query shape and round trips for data-access, and mapping drift for domain-model.
- **Acceptance**: forward: `for f in dotnet efcore dapper-sql; do p=claude/skills/review-v2-lenses/facets/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 41. Write the claude-harness facet [S]
- **Files**: `claude/skills/review-v2-lenses/facets/claude-harness.md` (new)
- **Depends on**: 24
- **Action**: Write the `claude-harness` facet (`harness:claude`), the portable judgement half of `claude/commands/review.md` Agent 6 — frontmatter, section order, cross-references, stubs, terminology — attached to hygiene, doc-drift and ripple.
- **Detail**: The mechanical half (gates a repo already runs) belongs to project checks such as this repo's `harness-gates`, not to the facet.
- **Acceptance**: forward: `test -f claude/skills/review-v2-lenses/facets/claude-harness.md && grep -qx 'id = "claude-harness"' claude/skills/review-v2-lenses/facets/claude-harness.md && echo ok` prints `ok` (today: prints nothing, exit 1).

### 42. Write the checks: lint-baseline, profile-baseline, break-set-baseline [M]
- **Files**: `claude/skills/review-v2-lenses/checks/lint-baseline.md` (new), `claude/skills/review-v2-lenses/checks/profile-baseline.md` (new), `claude/skills/review-v2-lenses/checks/break-set-baseline.md` (new)
- **Depends on**: 24
- **Action**: Write three `executor = "verification"` check entries with `discover` ladders.
- **Detail**: Ladder: a flow plan's `## Verification Commands` key (`lint:` / `bench:`), then CLAUDE.md, then an ecosystem default chosen by stack signal (`cargo clippy`, `tsc --noEmit`, `dotnet build`), or "none — <reason>". `profile-baseline` carries `claude/commands/optimise.md`'s declared-bench sources. `break-set-baseline` (plan) runs the plan's predicted break-set commands against the current tree (`claude/commands/review-plan.md` Step 2.6 check 1's baseline half); the precondition judgement belongs to the `plan-checks` cell. Per Approach, "Run pipeline" (stage 2, **Baselines**), each resolved command runs in its own `verification` dispatch and its log path is recorded in the run's `baselines`; each check body states how a finder reads the in-scope lines of that log (for `lint-baseline`, the diagnostics naming a scope file). Any `tomlctl …` line in a bash fence must pass `command_lint` and `guidance_lint` once task 46 widens their scan set.
- **Acceptance**: forward: `for f in lint-baseline profile-baseline break-set-baseline; do p=claude/skills/review-v2-lenses/checks/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 43. Write the checks: store-check, format-contract, dev-server-probe [M]
- **Files**: `claude/skills/review-v2-lenses/checks/store-check.md` (new), `claude/skills/review-v2-lenses/checks/format-contract.md` (new), `claude/skills/review-v2-lenses/checks/dev-server-probe.md` (new)
- **Depends on**: 24
- **Action**: Write three `executor = "surveyor"` check entries.
- **Detail**: `store-check` (plan) runs `tomlctl tasks import-plan --dry-run` and `tomlctl tasks check --plan` into `STORE CHECK` — never the real import, which stage 0 runs per the `flow-contract-task-store` skill. `format-contract` (plan) points the pack at the `flow-contract-plan-output-format` skill's Format rules when the plan is house-format. `dev-server-probe` writes the `DEV SERVER` fence. Any `tomlctl …` line in a bash fence must pass `command_lint` and `guidance_lint` once task 46 widens their scan set.
- **Acceptance**: forward: `for f in store-check format-contract dev-server-probe; do p=claude/skills/review-v2-lenses/checks/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 44. Write the checks: two-control-probe, falsifiability, acceptance-contradiction [M]
- **Files**: `claude/skills/review-v2-lenses/checks/two-control-probe.md` (new), `claude/skills/review-v2-lenses/checks/falsifiability.md` (new), `claude/skills/review-v2-lenses/checks/acceptance-contradiction.md` (new)
- **Depends on**: 24
- **Action**: Write three `executor = "cell"` plan checks from `claude/commands/review-plan.md` Step 2.6 checks 4, 3 and 2, composed together as the whole-plan `plan-checks` cell on `review-finder`.
- **Detail**: `two-control-probe` points at the `flow-contract-plan-output-format` skill's `references/acceptance-probing.md` rather than restating it, runs the probe helper in one batched shell call, and keeps the verdict-to-severity mapping. Each check states how its results become defects (severity as v1 assigns) for the verifier. Add the break-set precondition judgement against the recorded `break-set-baseline` log to `falsifiability`'s body. Any `tomlctl …` line in a bash fence must pass `command_lint` and `guidance_lint` once task 46 widens their scan set.
- **Acceptance**: forward: `for f in two-control-probe falsifiability acceptance-contradiction; do p=claude/skills/review-v2-lenses/checks/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 45. Write this repo's project catalogue config [S]
- **Files**: `.claude/review-lenses/config.toml` (new)
- **Depends on**: 24
- **Action**: Write the project config declaring the `harness-gates` check (`families = ["code"]`, `paths = ["claude/**"]`, command `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`) and nothing else.
- **Detail**: Follow Approach, "Lens catalogue" (**Layers**). Every other entry stays at its built-in default; this file is the worked example of a minimal project config.
- **Acceptance**: forward: `grep -c 'id = "harness-gates"' .claude/review-lenses/config.toml` prints `1` (today: prints nothing, exit 2).

### 46. Gate the catalogue and lint its files [L]
- **Files**: `tomlctl/src/cli/dispatch/tests/catalogue.rs` (new), `tomlctl/src/cli/dispatch/tests/mod.rs`, `tomlctl/src/cli/dispatch/tests/lint.rs`, `claude/skills/review-v2-lenses/vocabulary.toml`, `.claude/review-lenses/config.toml`
- **Depends on**: 3, 20, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45
- **Action**: Add `catalogue_validates` (runs `tomlctl/src/review/catalogue.rs` `validate()` over `claude/skills/review-v2-lenses/` with `.claude/review-lenses/` as the project layer, failing with every error listed); register `mod catalogue;`; extend `command_lint_scan_set` in `tomlctl/src/cli/dispatch/tests/lint.rs` to the catalogue's `lenses/`, `facets/`, `methods/` and `checks/` directories.
- **Detail**: Resolve paths from the repo root as `tomlctl/src/cli/dispatch/tests/skills.rs` does. `command_lint_scan_set` is shared by `command_lint`, `guidance_lint` and `flag_table_lint`, so all three now cover the catalogue entries. Fix every error the first run reports, never by loosening the validator. Fix ownership collisions and vocabulary errors in `vocabulary.toml` or `config.toml`. An error that can only be fixed inside a lens, facet, method or check entry is reported as a deviation for a follow-on task, because those files belong to tasks 25–44. The task is L because the gate and the fixes it forces must land together for the tree to stay green. Add no count test: the Success Criteria check the plan-time counts once, and the lifecycle adds entries.
- **Acceptance**:
  - forward: `grep -c '^mod catalogue;' tomlctl/src/cli/dispatch/tests/mod.rs` prints `1` (today: `0`).
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib --no-tests=fail -E 'test(catalogue_validates)'` passes — **predicted, unverified** (today: fails, no test matches).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes (today: 43 passed).
  - falsifier: removing the `owns` key from `claude/skills/review-v2-lenses/lenses/ripple.md` turns `catalogue_validates` red — **predicted, unverified**.

### 47. Write the review-v2-engine skill: pipeline, survey, compose [M]
- **Files**: `claude/skills/review-v2-engine/SKILL.md` (new), `claude/skills/review-v2-engine/references/survey.md` (new), `claude/skills/review-v2-engine/references/compose.md` (new)
- **Depends on**: 4, 24
- **Action**: Write the orchestrator contract: stages 0–8 with the executor of each, the orchestrator's judgement-only role and the depth envelopes in `SKILL.md`; the survey (detectors, surveyor dispatch, judged candidates, checks by executor, fences, the verification dispatch) in `references/survey.md`; compose (accepting candidates, reading the proposal, re-slicing, bundling, probes, facet edits, user steering flags, waivers, the deep confirm step, the agent-count statement, follow-up and mop-up criteria) in `references/compose.md`.
- **Detail**: Follow Approach, "Run pipeline", Approach, "Coverage proposal", Approach, "Lens catalogue" and Approach, "Design principles". `references/survey.md` carries the per-command baseline dispatches, the `baselines` record and the stage 3→4 barrier; `SKILL.md` carries the steering flags. Stage 0 checks the capabilities features, runs `review sync-v1`, and for the plan family runs the task-store import per the unchanged `flow-contract-task-store` skill. The engine never loads the `review-v2-lenses` skill (the catalogue path resolves in tomlctl) and loads the `backlog-capture` skill only at stage 8 when a `backlog` pick exists. Every flow-scoped dispatch prompt opens with the column-0 `ledger: .claude/flows/<slug>/review-v2.toml` line. The orchestrator reads `review plan --propose` output, `review digest`, and projections only. Invoke the unchanged `tomlctl` and `flow-contract-flow-context` skills by name. Every `tomlctl review …` line in a bash fence must parse. Description ≤1024 characters, body ≤500 lines, each reference ≤600.
- **Acceptance**:
  - forward: `test -f claude/skills/review-v2-engine/SKILL.md && grep -c 'tomlctl review plan' claude/skills/review-v2-engine/SKILL.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 48. Write the engine references: agent waves, triage and close, measurement [M]
- **Files**: `claude/skills/review-v2-engine/references/agent-waves.md` (new), `claude/skills/review-v2-engine/references/triage-and-close.md` (new), `claude/skills/review-v2-engine/references/measurement.md` (new)
- **Depends on**: 4, 49
- **Action**: Write the Agent-tool fallback (the per-cell prompt template copied from `claude/workflows/review-v2.js`, cell waves then verifier/checker waves of ≤16 per message, background completion and the wave barrier, the 16-running-agent accounting, routing from each row's `route`, mop-up from the digest's unverified ids, one-shot dispatch with no `name:`, the column-0 `ledger:` line when a flow is bound); the triage-and-close procedure (the audit sample and its projection, advisory recommendations, the disposition grammar mapped to `review triage` calls, the backlog hand-off for user picks only, `export-v1` and the apply hand-off, `run finish`); and the measurement reference (the signals of Approach, "Measurement and tuning", the seeded recall check, and the thresholds that count as better or worse than v1).
- **Detail**: Dispositions are a user-engagement gate the autonomy directive does not override; user prose is staged with the Write tool and passed through the `--<flag>-file` twins. The measurement thresholds cite v1's baseline values with the command that produced each where one is in the tree (ledger counts through `tomlctl items list … --count-by`), and otherwise with value, date and method (the orchestrator-context baselines, per Approach, "Measurement and tuning"). Every `tomlctl review …` line in a bash fence must parse.
- **Acceptance**:
  - forward: `for f in agent-waves triage-and-close measurement; do test -f claude/skills/review-v2-engine/references/$f.md || echo MISSING $f; done | wc -l` prints `0` (today: `3`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 49. Write the review-v2 saved workflow and its deploy entry [L]
- **Files**: `claude/workflows/review-v2.js` (new), `scripts/deploy-claude.ps1`
- **Depends on**: —
- **Action**: Write the saved Workflow script (`export const meta = {name: 'review-v2-pipeline', …}`, phases Cells and Verify, a `SCRIPT_VERSION` const returned as `script_version`) that runs one wave over `args.cells` and routes each returned finding by its `route`. Add a `workflows` area (`Filter = '*.js'`, file symlinks) to `scripts/deploy-claude.ps1` and name it in the script's SYNOPSIS/DESCRIPTION.
- **Detail**: Invoke the `workflow-authoring` skill first: it holds the script API and its rules (a pure-literal `meta` with a `description`, phase titles matching `phase()` calls, plain JS, no filesystem, the concurrency cap). Follow Approach, "Run pipeline" (stage 4 and **Substrate**). Idioms: `agent(prompt, {agentType, schema, label, phase})` with `args.schemas.cell_return` and `args.schemas.verdict_return`; `verifier` → one `review-verifier` per finding (`panel` → three seats); minor defects at quick and standard → one verifier batch per cell; `checker` rows pooled into `review-checker` batches of ≤20; a checker's `reroute` ids → `review-verifier`; `pipeline()` so verification starts per cell; no `Date.now()`/`Math.random()`; prompts built only from `args`; `log()` every skipped cell and escalation; return plain data only.
- **Acceptance**:
  - forward: `test -f claude/workflows/review-v2.js && grep -c "name: 'review-v2-pipeline'" claude/workflows/review-v2.js` prints `1` (today: prints nothing, exit 1).
  - forward: `test -f claude/workflows/review-v2.js && t=$(mktemp --suffix .js) && { echo 'async function __wf(agent, pipeline, parallel, log, phase, args, budget, workflow) {'; sed 's/^export //' claude/workflows/review-v2.js; echo '}'; } > "$t" && node --check "$t"` exits 0 (today: exits 1, file absent).
  - falsifier: deleting one closing parenthesis in the script makes the `node --check` probe exit 1.
  - forward: `grep -c "Name = 'workflows'" scripts/deploy-claude.ps1` prints `1` (today: `0`).

### 50. Write the Opus agents: review-finder, review-verifier, plan-executor [M]
- **Files**: `claude/agents/review-finder.md` (new), `claude/agents/review-verifier.md` (new), `claude/agents/plan-executor.md` (new)
- **Depends on**: —
- **Action**: Write three agents with frontmatter `name`, `description`, `tools` (each including Write and Skill), `model: opus`, `effort` (finder high, verifier high, executor medium), `color`, and `skills:` (finder and verifier: `review-v2-method`, `research-methods`; executor: `review-v2-method`). Each body holds that agent's own procedure from Approach, "Agents" and carries the `review-v2-store-writer` block verbatim from Approach, "Agents" and the `research-delivering` block byte-identically from `claude/agents/research-deep.md`.
- **Detail**: Tools per the Agents table. The finder's procedure covers lens bundles, probes and `check:` cells (including `plan-checks`); the verifier's covers single findings, minor batches, routed cleanup, severity reassessment and test removals. Descriptions name what the agent does, never a carrier's stage numbers. Bodies say "if `review-v2-method` is not loaded, invoke it first", read catalogue files and `review-v2-method`'s worked examples from the absolute paths in the assignment, tell finder and verifier to call `Skill(research-methods)` when they need one of its gated references, use Write only to stage payloads in the run directory, and name no `tomlctl review` command in a bash fence.
- **Acceptance**: forward: `grep -l '^model: opus' claude/agents/review-finder.md claude/agents/review-verifier.md claude/agents/plan-executor.md 2>/dev/null | wc -l` prints `3` (today: `0`).

### 51. Write the Haiku agents: review-surveyor, review-sweeper, review-checker [M]
- **Files**: `claude/agents/review-surveyor.md` (new), `claude/agents/review-sweeper.md` (new), `claude/agents/review-checker.md` (new)
- **Depends on**: —
- **Action**: Write three agents with `model: haiku`, `effort: medium`, `omitClaudeMd: true`, `skills: [review-v2-method]`, tools per Approach, "Agents" (each including Write and Skill), their own procedures in the body, and two blocks byte-identically: the `review-v2-store-writer` block from Approach, "Agents" and the `research-delivering` block from `claude/agents/research-deep.md`.
- **Detail**: Haiku bodies are short and procedural: numbered steps, the exact return object, and explicit stop conditions. Each opens "if `review-v2-method` is not loaded, invoke it first", reads worked examples from the absolute paths in its assignment, and stages every payload with Write in the run directory — the surveyor proposes judged signals with evidence and never decides them; the sweeper writes only grep-provable findings and escalates anything needing judgement; the checker applies only the listed grep-decidable predicates, returns `unsettled` otherwise, and refutes only a sweeper's row with a grep citation.
- **Acceptance**: forward: `grep -l '^omitClaudeMd: true' claude/agents/review-surveyor.md claude/agents/review-sweeper.md claude/agents/review-checker.md 2>/dev/null | wc -l` prints `3` (today: `0`).

### 52. Register the v2 agents as shared-block carriers [S]
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: 50, 51
- **Action**: Add a `[[block]]` named `review-v2-store-writer` listing the six v2 agent files, and add the six files to the `research-delivering` block's `files`.
- **Detail**: Run `bash scripts/verify-shared-blocks.sh`; a byte difference between the six copies is fixed in the agent files, never by dropping a carrier.
- **Acceptance**:
  - forward: `grep -c 'review-v2-store-writer' scripts/shared-blocks.toml` prints `1` (today: `0`).
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0 (today: exits 0).

### 53. Write the /review-v2 carrier [M]
- **Files**: `claude/commands/review-v2.md` (new)
- **Depends on**: 21, 23, 46, 47, 48, 49, 52
- **Action**: Write the code-family carrier: frontmatter (`description`, `argument-hint` with the steering flags of Approach, "Run pipeline", stage 3), invocations of the `tomlctl`, `flow-contract-flow-context`, `review-v2-engine` and `backlog-capture` skills, and its deltas — the code floor, the context-free correctness cell, export to `review-ledger.toml` and the `/review-apply` hand-off.
- **Detail**: Follow Approach, "Family specifics" and Approach, "Coexistence with v1". The carrier restates nothing the engine owns. Each skill invocation uses the backticked-name-plus-"skill" phrase the gate requires; the `backlog-capture` invocation is scoped to stage 8.
- **Acceptance**:
  - forward: `test -f claude/commands/review-v2.md && grep -c '`review-v2-engine` skill' claude/commands/review-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 54. Write the /optimise-v2 carrier [M]
- **Files**: `claude/commands/optimise-v2.md` (new)
- **Depends on**: 21, 23, 46, 47, 48, 49, 52
- **Action**: Write the perf-family carrier with invocations of the `tomlctl`, `flow-contract-flow-context`, `review-v2-engine` and `backlog-capture` skills, the steering flags of Approach, "Run pipeline", stage 3 in its `argument-hint`, plus its deltas: the Focal Points Brief (only what the bound facets do not carry), `PROFILE BASELINE` via the `profile-baseline` check, the perf defect rule, `branch1..branch2` scope, and export to `optimise-findings.toml` with the `/optimise-apply` hand-off.
- **Detail**: Follow Approach, "Family specifics". Each skill invocation uses the backticked-name-plus-"skill" phrase the gate requires.
- **Acceptance**:
  - forward: `test -f claude/commands/optimise-v2.md && grep -c 'PROFILE BASELINE' claude/commands/optimise-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 55. Write the /review-plan-v2 carrier [M]
- **Files**: `claude/commands/review-plan-v2.md` (new), `tomlctl/src/cli/dispatch/tests/finding_classes.rs`
- **Depends on**: 21, 23, 46, 47, 48, 49, 52
- **Action**: Write the plan-family carrier with invocations of the `tomlctl`, `flow-contract-flow-context`, `flow-contract-plansdirectory-prompt`, `flow-contract-task-store`, `flow-contract-plan-output-format`, `review-v2-engine` and `backlog-capture` skills, and the steering flags of Approach, "Run pipeline", stage 3 in its `argument-hint`. Deltas: plan resolution, the stage-0 task-store import, the `store-check`, `format-contract` and `break-set-baseline` checks, the `plan-checks` cell, the plan floor including `plan-structure`, `plan-executor` cells, and the verified-only merge offer per Approach, "Family specifics". Add to `PROSE` in `tomlctl/src/cli/dispatch/tests/finding_classes.rs` each catalogue file that names a `tasks check` finding class — at least `claude/skills/review-v2-lenses/checks/store-check.md` — and add `claude/commands/review-plan-v2.md` only if the carrier itself names one (the gate fails a `PROSE` document that names no class at all).
- **Detail**: Follow Approach, "Family specifics". Name every `tasks check` finding class exactly as it exists in `tomlctl/src/tasks` (`prose_class_names_exist_in_the_source` gates it). The merge never runs Manual mode on an empty `AskUserQuestion` answer.
- **Acceptance**:
  - forward: `test -f claude/commands/review-plan-v2.md && grep -c 'plan-checks' claude/commands/review-plan-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - forward: `grep -c 'review-v2-lenses/checks/store-check.md' tomlctl/src/cli/dispatch/tests/finding_classes.rs` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 56. Add the v2 carriers to the skill gate table, ignore run artifacts, document v2 in CLAUDE.md [M]
- **Files**: `tomlctl/src/cli/dispatch/tests/skills.rs`, `.gitignore`, `CLAUDE.md`
- **Depends on**: 53, 54, 55, 57
- **Action**: Add rows for `review-v2.md`, `optimise-v2.md` and `review-plan-v2.md` to the `carrier_invokes_required_skills` table in `tomlctl/src/cli/dispatch/tests/skills.rs`, each listing exactly the skills its carrier invokes. Add `/.claude/review-v2-runs/` to `.gitignore`. Add a short "Review v2" subsection to `CLAUDE.md` after "Flow agent tiers" — the three commands, the six agents and their model tiers (outside the `lite`/`deep` buckets, model in frontmatter only, never named at dispatch), the store, the orchestrator's judgement-only role, the global catalogue with its optional per-project `config.toml`, and the saved-workflow deploy step — add `workflows` to the `claude/{agents,commands,skills,themes}` list in its "Developer setup" deploy sentence, and update that section's hook paragraph so the `cli::dispatch::tests` trigger names `claude/**/*.toml` and `.claude/review-lenses/**` (task 57).
- **Detail**: Every listed skill must exist under `claude/skills/`.
- **Acceptance**:
  - forward: `grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs` prints `3` (today: `0`).
  - forward: `grep -c 'review-v2-runs' .gitignore` prints `1` (today: `0`).
  - forward: `grep -c '/review-v2' CLAUDE.md` prints at least `1` (today: `0`).
  - falsifier: removing the `review-v2-engine` invocation from `claude/commands/review-v2.md` turns `carrier_invokes_required_skills` red — **predicted, unverified**.

### 57. Widen the pre-commit catalogue trigger [S]
- **Files**: `.githooks/pre-commit`
- **Depends on**: 46
- **Action**: Widen the staged-path pattern that runs `cargo test --manifest-path "$ROOT/tomlctl/Cargo.toml" --lib -- cli::dispatch::tests` in `.githooks/pre-commit` from `^(claude/.*\.md|tomlctl/src/.*)$` to `^(claude/.*\.(md|toml)|\.claude/review-lenses/.*|tomlctl/src/.*)$`, and update the comment above it to name the catalogue.
- **Detail**: Without it a commit staging only `claude/skills/review-v2-lenses/vocabulary.toml` or `.claude/review-lenses/config.toml` skips `catalogue_validates`. Change nothing else in the hook; per CLAUDE.md's supply-chain note the diff is reviewed like an unsandboxed CI step.
- **Acceptance**:
  - forward: `grep -c 'review-lenses' .githooks/pre-commit` prints `1` (today: `0`).
  - forward: `printf 'claude/skills/review-v2-lenses/vocabulary.toml\n' | grep -Ec "$(grep -o "'\^(claude[^']*'" .githooks/pre-commit | tr -d "'")"` prints `1` (today: `0`).

### 58. Test the shipped vocabulary's detectors on fixture trees [M]
- **Files**: `tomlctl/tests/review_vocabulary.rs` (new)
- **Depends on**: 6, 46
- **Action**: Add an integration test that loads the real `claude/skills/review-v2-lenses/vocabulary.toml` (as the built-in layer, via `--builtin claude/skills/review-v2-lenses`) and runs `tomlctl review survey` over temp-dir fixture repos modelled on the user's layouts, asserting each expected signal and its `roots`.
- **Detail**: Fixtures, one test each: `rust_crate_at_depth_three_detected_with_root` (a `Cargo.toml` three levels down), `efcore_detected_through_central_package_management` (`Directory.Packages.props` naming `Microsoft.EntityFrameworkCore`), `react_detected_in_bun_workspace` (a Bun-workspace `package.json` with a React package), `dapper_detected_through_legacy_reference` (a `.csproj` with `<Reference Include="Dapper">`), `vue_root_is_its_subdirectory_only` (a Vue app in one subdirectory of a Rust repo; `framework:vue` carries that subdirectory as its only root), and `rust_test_growth_counts_test_attributes` (the literal `#[test]` count over a `git log` window). Use the `tomlctl/tests/common/` sandbox and its git helper. Task 46 runs first so the vocabulary under test is the gated one.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_vocabulary.rs && grep -c 'fn efcore_detected_through_central_package_management' tomlctl/tests/review_vocabulary.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: deleting the `Directory.Packages.props` glob from the `db:efcore` detector in `vocabulary.toml` turns `efcore_detected_through_central_package_management` red — **predicted, unverified**.

### 59. Give the workflow batch prompts a batch id and sweeper marks [M]
- **Files**: `claude/workflows/review-v2.js`, `claude/agents/review-checker.md`, `claude/agents/review-verifier.md`
- **Depends on**: 48, 49, 50, 51
- **Action**: In `claude/workflows/review-v2.js`, make `checkerPrompt` and the batch form of `verifierPrompt` carry what the checker and verifier procedures require, matching the `batch:` and `sweeper rows:` lines `claude/skills/review-v2-engine/references/agent-waves.md` already builds: a deterministic `batch: <id>` line (e.g. `K<n>` for checker batch n, `V<cellId>` for a cell minor batch — no clock or randomness) used as the payload name, and for checker batches a `sweeper rows: <ids>` line listing the pooled finding ids whose source cell has `agentType = review-sweeper` (track each pooled id's cell agentType when it enters the pool). Align the batch return contract: `claude/agents/review-checker.md` currently says its return is a bare JSON array while the workflow's `batchSchema` expects `{verdicts: [...]}`; change the checker body (outside its shared blocks) to return `{verdicts: [...]}`, and make `claude/agents/review-verifier.md`'s batch return say the same, so the script, the agents and agent-waves.md agree. Do not touch either agent's SHARED-BLOCK regions.
- **Acceptance**:
  - forward: `grep -c "sweeper rows:" claude/workflows/review-v2.js` prints at least `1` and `grep -c "batch:" claude/workflows/review-v2.js` at least `1` (today: `0`).
  - forward: the wrapped `node --check` of the script exits 0, and a mocked run (agent/pipeline/parallel stubs, run from the scratchpad) shows a checker prompt naming a sweeper-cell row under `sweeper rows:` and omitting a finder-cell row.
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0.

### 60. Let bind_cell bundle the cell-run checks of plan-checks [S]
- **Files**: `tomlctl/src/review/catalogue.rs`
- **Depends on**: 3
- **Action**: In `tomlctl/src/review/catalogue.rs`, `Catalogue::bind_cell` refuses any cell whose concerns hold a `check:<id>` beside anything else, so the plan-family `plan-checks` cell (Approach → Lens catalogue: `two-control-probe`, `falsifiability` and `acceptance-contradiction`, executor `cell`, composed together as one whole-plan `review-finder` cell) cannot be bound. Allow a cell whose concerns are 1–3 `check:<id>` entries when every one is an enabled check with `executor = "cell"` declaring the run's family; such a cell is exempt from the per-depth concern maximum (2 at `deep`), binds each check like a single one does today, and attaches no facets. Keep refusing a `check:` mixed with a lens or a probe, a check whose executor is not `cell`, and a duplicate check id. Update the module doc where it states the one-check rule.
- **Acceptance**:
  - forward: a new unit test `bind_cell_bundles_cell_checks` binds `["check:two-control-probe","check:falsifiability","check:acceptance-contradiction"]` at plan/deep and asserts three bound concerns; it also asserts `["check:two-control-probe","correctness"]` and `["check:lint-baseline","check:falsifiability"]` are refused — `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(/^review::catalogue::/)'` passes.
  - falsifier: restoring the one-check rule turns `bind_cell_bundles_cell_checks` red.

### 61. Share one helper that rebuilds a run's catalogue [M]
- **Files**: `tomlctl/src/review/catalogue.rs`, `tomlctl/src/review/survey.rs`, `tomlctl/src/review/cells.rs`, `tomlctl/src/review/add.rs`, `tomlctl/src/review/triage.rs`, `tomlctl/src/review/query.rs`, `tomlctl/src/review/digest.rs`, `tomlctl/src/review/runs.rs`, `tomlctl/src/review/stats.rs`, `tomlctl/src/review/export.rs`
- **Depends on**: 12, 13, 15, 16, 17
- **Action**: Every review verb that needs the run's catalogue (survey.rs, cells.rs, add.rs, triage.rs, query.rs, and any of digest.rs, runs.rs, stats.rs, export.rs that gained one) holds a private copy of the code that rebuilds a `Catalogue` from the `catalogue.builtin` / `catalogue.project` roots the run recorded (names vary: `run_catalogue`, survey's own). Replace them with one `pub(crate)` function in `tomlctl/src/review/catalogue.rs` taking the repo root and the run, resolving a relative project root against the repo root exactly as the existing copies agree on (if two copies disagree, keep the stricter behaviour and say which), and call it from every site. Behaviour-preserving: no output or error text changes except where copies disagreed. Fold the duplicated store-to-JSON view too: `query.rs` `View` and `digest.rs` `Rows` both convert the store to JSON maps for `fields::project`; keep one shared `pub(super)` version. Also fold the copied pathspec matcher: `stats.rs` holds a copy of `cells.rs`'s private `PathSet`/`within`; keep one shared `pub(super)` version. Also fold the duplicated counts definitions: `runs.rs` `counts` mirrors `digest.rs`'s private `counts_line`/`report_rows`/`live`; keep one shared `pub(super)` helper both call so the report line and the stored run counts cannot drift.
- **Acceptance**:
  - forward: `grep -c` over tomlctl/src/review/*.rs for the old private helper definitions totals 0 and the shared function is defined once in catalogue.rs.
  - guard: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(/^review_/) | test(/^review::/)'` passes.

### 62. Count only the latest verdict per panel seat [S]
- **Files**: `tomlctl/src/review/verdict.rs`, `tomlctl/tests/review_verdict.rs`
- **Depends on**: 10
- **Action**: In `tomlctl/src/review/verdict.rs`, the panel `decide` tally counts every `by = panel` verdict on a finding. `review resolve` now appends copies of a merged row's deciding verdicts onto a contested or reopened target, so a later panel verdict on that target could reach the two-seat threshold on duplicated seats. Make the tally read only the latest verdict per `seat` (the last in store order), so copies and re-votes never double-count. Add a test in `tomlctl/tests/review_verdict.rs`: two verdicts recorded for seat 1 (e.g. one refuted then one confirmed) plus seat 2 confirmed resolves `open`, and two copies of the same refuting seat do not make two refuting seats.
- **Acceptance**:
  - forward: the new test passes under `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_verdict`, with every existing verdict test still green.
  - falsifier: counting every panel verdict again turns the new test red.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 19, 22, 62 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 60, 61, 62. The `tomlctl review` group, including detectors, proposal and accounting, identity resolution, v1 read-back and the catalogue loader and verb, complete, tested, documented and version-bumped. A buildable crate increment; every later markdown fence naming `tomlctl review` parses against it.

— CHECKPOINT B after tasks 23, 47, 52, 57, 58, 59 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 57, 58, 59, 60, 61. The v2 harness foundation: method core, the full catalogue gated by `catalogue_validates` and the widened lint scan set (with the hook trigger and the vocabulary fixture tests), engine, six agents, workflow and deploy entry. Buildable because no carrier references it yet and task 20's output cases are in the tree; `verify-shared-blocks.sh` covers the new agents.

— CHECKPOINT C after tasks 53 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 60, 61. `/review-v2` usable end to end, the first trial-ready carrier.

— CHECKPOINT D after tasks 56 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 60, 61. `/optimise-v2`, `/review-plan-v2`, the gate rows, the ignore rule and CLAUDE.md.

## Risks

- **A built-in catalogue edit changes every project at once.** Mitigations: runs bind content hashes so `stats` shows a change's effect; reworked lenses ship as new experimental ids that are opted into beside the old one; `catalogue_validates` and the widened `command_lint` block a broken edit at commit time; `run start` validates the merged layers in every repo.
- **Composition quality.** A thin proposal or a careless edit yields a thin map. Mitigations: floor lenses always run; unselected matched concerns and unsliced files need a waiver before `run finish`; `contrarian` and `synthesis` remain available; `stats --escapes` shows concerns the map missed.
- **Detectors miss or misfire on unfamiliar layouts.** Mitigations: detectors are data, and the shipped `vocabulary.toml` is tested on fixture trees modelled on the user's repos (task 58); `count` patterns are literals and every `contains` regex is compiled by the catalogue gate under the shipped regex build; a failed detector records `unknown` rather than false; signals carry roots so a facet attaches only where its stack lives; judged signals stay candidates the orchestrator accepts.
- **Per-finding verification and composed waves multiply agents.** Mitigations: minor defects are batch-verified at quick and standard; checker rows are pooled; the cell ceiling scales with diff size; the proposal estimates agents and an estimate above 25 must be justified; every run records `agents`.
- **Concurrent store writes and Workflow restarts.** Mitigations: cell-scoped `add` with sealing, verdict upserts, routing echoed from `add` with a mop-up from the store, post-wave `resolve`, and a `run finish` gate on unverified rows; short locked writes; the retry-once rule for the Windows `os error 5`, with `cover` and `run update` idempotent under it.
- **Windows payload and line-ending traps.** Multi-line heredocs into tomlctl fail on this host from about 14 rows, and 469 tracked files here are CRLF. Mitigations: agents stage every payload with the Write tool in the run directory and pass `--ndjson <path>`; `review add` compares `old` text after normalising line endings; paths are refused unless repo-relative and `/`-separated.
- **Haiku output quality.** Mitigations: Haiku decides only grep-settleable questions; the checker returns `unsettled` rather than guessing and cannot refute an Opus row; behaviour-changing cleanup, test removals and unsettled rows go to Opus; the audit samples checked cleanup; Haiku agents run without CLAUDE.md so their procedure is not crowded out.
- **The verifier refutes true findings (recall loss).** Mitigations: the audit samples REFUTED non-critical defects as well as every REFUTED critical; calibration learns only from audited refutations and user decisions.
- **Uncapped cleanup floods the report.** Mitigations: one pattern is one row with `instances`; the report goes to a file and groups cleanup by file; field caps keep rows short; `review stats` shows dismissal and apply outcomes per lens.
- **Advisory accumulates unseen.** Mitigation: parked advisory appears in later runs' `PRIOR` and in `review list --kind advisory`; suppression stops a dismissed or parked advisory being raised again.
- **Explicit `same_as` identity depends on finders reading `PRIOR`.** A missed `same_as` mints a duplicate rather than losing a finding; duplicates carry their candidates under `related`.
- **Workflow is unavailable, disabled, needs per-run approval, or the deployed copy is stale.** Mitigations: the agent-wave fallback has the same persistence contract; the run records `substrate` and `SCRIPT_VERSION`.
- **Markdown documenting `tomlctl review` fails `command_lint` if it lands before the clap tree.** Mitigation: tasks 23, 24, 47 and 48 depend on task 4, the carriers on task 21, catalogue entries are linted only from task 46, and agent bodies and the shared block use no fenced `tomlctl review` command.

## After Merge

- Run `cargo install --path tomlctl`; the carriers shell out to the installed binary, `review` lands in 0.16.0, and each v2 carrier halts at stage 0 on an older binary. Run `cargo install --path glimpse` too: the harness's `glimpse hook` records agents through tomlctl's library in-process, so the `review-v2.toml` attribution takes effect only in a rebuilt glimpse.
- Rerun `pwsh -File scripts/deploy-claude.ps1` to link the six new agents, three commands, three skills (the catalogue travels inside `review-v2-lenses`) and `claude/workflows/review-v2.js` into `~/.claude/`. Where a session reads an out-of-band config directory (such as `~/.claude-work` on the machine the original draft was researched on), copy the same files there and recopy after every catalogue or script change. Restart open sessions.
- Smoke test, **predicted, unverified**: `tomlctl review catalogue --check`; then `/review-v2 --depth quick tomlctl/src/sweep.rs`, `tomlctl review digest --scope <scope> --run U1` and `tomlctl review stats --scope <scope>`. Run one `/review-v2 --depth deep` on a single small file to exercise panels, follow-up waves and the confirm step, and one `/review-v2 --depth quick` in `reportdesignkit` and one in `Intembeko.ReportMigration` with no project config to confirm zero-setup detection and root-scoped facets.
- Seeded recall check, **predicted, unverified**: pick five v1 `fixed` items with fix commits, check out each fix's parent in a scratch worktree, run `/review-v2 --depth quick` on the item's file, and record hit or miss at file + symbol in `claude/skills/review-v2-engine/references/measurement.md`'s format.
- Trial: use `/review-v2`, `/optimise-v2` and `/review-plan-v2` in place of v1 on real work; after five or more runs, review `tomlctl review stats --all --outcomes --escapes --calibration` against the thresholds in `measurement.md` before retuning any lens, facet or activation.
- Follow-ons: a native v2 apply flow built for cleanup batches; the full recall-eval harness (R11); Haiku clerks for v1 `/implement` and the apply flows (Exploration Notes); lumina adopting catalogue lenses where its research lenses overlap.

## Exploration Notes

### Inventory read in full (orchestrator)
- Producers: `claude/commands/review.md` (6 lenses, mixed deep/lite, 20/15 budget, security cap 5), `claude/commands/optimise.md` (5 lenses all `research-deep`, `max` effort, Focal Points Brief, cross-cutting concurrency pass), `claude/commands/review-plan.md` (4 lenses all deep + 4 orchestrator-only checks in Step 2.6, inline `plan-review-findings.toml` schema, Step 4 merge with Manual/Mechanical modes).
- Agents: `claude/agents/research-deep.md` / `research-lite.md` share 4 byte-identical blocks (`research-method`, `research-read-only`, `research-finding-record`, `research-delivering`) per `scripts/shared-blocks.toml`. Both descriptions hardcode carrier lens numbers ("/review Agents 1, 3", "/review Agent 4").
- Method: `claude/skills/research-methods/SKILL.md` (the bar, 7-step procedure, source hierarchy, grades, Counter line) + `references/codebase-review.md` (lens notes for perf/arch/security/completeness/plan).
- Vet: `claude/skills/flow-contract-vet-research/SKILL.md` — orchestrator samples ≥3 per agent, `[[vet_events]]` append, >30% re-dispatch.
- Lens definitions live **inline** in each carrier's Step 2; no catalogue, no shared lens schema, no shared severity rubric.

### Downstream contract (apply flow) — a redesigned producer MUST keep emitting
- Read by apply: `id`, `status`, `severity` (selector + critical gate), `category` (critical-categories gate, prompts), `file` (freshness gate, clusters, diff check, rollback), `line`/`symbol`, `instances`/`sweep`/`enumeration` (cluster budget, `lite_file_scope`, re-sweep), `description` (budget files, lite-gate "names the exact change", Tier-1 literal), `depends_on`, `defer_trigger`, `rounds` (chronic tag only), `related`, root `last_updated` (freshness gate).
- NOT read by apply: `effort`, `evidence`, `first_flagged`, `flow`. `evidence` never reaches implementers (`flow-contract-apply-pipeline/references/agent-prompt-contract.md`).
- `needs-plan` has **no schema field** — live ledgers encode it 3 ways (`needs_plan = true`, `[needs-plan]` description prefix, `needs-plan:` summary); `/review-apply all` dispatches them anyway.
- `description` is overloaded (budget files + recommended fix + needs-plan + tradeoffs); no structured `fix` field.

### Duplication across carriers (none registered in `scripts/shared-blocks.toml`)
Pre-flight paragraph + envelope block (5 carriers); user-inputs sweep (review/optimise differ only in honoured statuses); one-shot Explore "never pass `name:`"; dev-server probe; lint baseline; backlog load + `promoted`-row rule; lens-dispatch boilerplate (single message, byte-identical preamble, `ledger:` column-0 line, 20/15 budget, omit id/rounds); Step 2.5 vet paragraph; interim checkpoint; Step 3 persist (`--id-prefix`, `expect`+`--on-stale skip`+"changed during the run" triad ~12×, `date -u +%F` close-out, pattern-findings paragraph, needs-plan paragraph, out-of-scope→backlog paragraph).

### Inconsistencies
- Vet sample: skill ≥3; `/review` ≥5 for architecture/completeness/package-quality; `/optimise` expand-to-all on one drop; `/review-plan` 2 + every stale-ref claim; apply-lite ≥2/cluster.
- `vet_events.lens` (`queries`, `async`, `Algorithm`) ≠ item `category` (`query`, `concurrency`) → no join. Items record no producing lens.
- Severity: research-methods (data-loss/security/broken), optimise ("measurable perf impact" + own concurrency calibration), review-plan uses "minor" (outside vocabulary).
- Merge-on-match: optimise refreshes `line`/`description`/`evidence`; review only bumps `rounds` (stale lines reach apply's ±50-line read). Optimise Step 3 mentions `verified-clean` (not in its vocabulary).
- Chronic: schema ≥3; review ≥3 + recommendation ≥5; optimise "Recurring" ≥2.
- `/optimise` has no Step-4 disposition handler though apply defers to "the producer's disposition protocol".
- `apply-dependency-sort` "post-batch commit" contradicts no-auto-commit; "already applied" tag has 3 spellings.
- `plan-review-findings.toml`: separate inline schema (`P`, `plan_section`, `anchor_old/new`, `open/merged/discarded`), no chronic handling, dedup `(plan_section, anchor_old)`.

### Coupling / gates a rewrite must satisfy
- `carrier_invokes_required_skills` (`tomlctl/src/cli/dispatch/tests/skills.rs:1391`, table `:1405-1527`): backticked name + word "skill", no negation within 160 chars; every listed skill must exist. `research-methods` is in no carrier list.
- `command_lint` (`lint.rs:291`) parses every `tomlctl …` line in bash fences of commands/agents/skills via clap; `guidance_lint` (`:837`), `flag_table_lint` (`:1156`); SKILL.md body ≤500 lines (`skills.rs:852`), reference ≤600 (`:902`), description ≤1024 chars; relative links must resolve (`:1222`); `finding_classes.rs` checks `tasks check` class names named in `review-plan.md`.
- Shared blocks: 4 research blocks must stay byte-identical across the two research agents (`verify-shared-blocks.sh`, Rust mirror `blocks_verify_agrees_with_shell_gate`).
- Category vocab: not validated by tomlctl, but hardcoded in `glimpse/src/actions.rs:134-149`; `category` is in the `dedup_id` fingerprint (`tomlctl/src/dedup.rs:33-34`) — renaming categories re-fingerprints every item.
- Status vocab: `tomlctl/src/items.rs:2202-2213` (companions), `ledgers.rs:270-283`, glimpse `actions.rs:86-95,117`, `ledger.rs:94-100`, lumina `Disposition` enum.
- Agent attribution regex `[ROP][0-9]+` + 3 ledger filenames (`tomlctl/src/agents/correlate.rs:201`); lens prompts must open `ledger: <path>` at column 0.
- `flow envelope build --command` fixed list (`flow/envelope.rs:23-34`, `capabilities.rs:40-55`); `inputs.rs:58` ledgers list.
- lumina-story-blocks plugin dispatches `research-deep` by name and links `flow-contract-vet-research` (`research-explore/SKILL.md:12,101`); already drifted (sampling "max(3,30%)", console-line format). Link check skips plugins. Has an always-on `contrarian` lens and devil's-advocate checks worth adopting.

### Empirical record (all ledgers under `.claude/`)
- `/review`: 950 items, 842 fixed (≈16 `partial:`, ≈16 no-op), 24 wontfix, 28 verified-clean, 10 deferred. Per-lens resolved precision 89–99%; **48–65% of every lens's output is `suggestion`**. Architecture weakest (89%, most wontfix/downgrades).
- `/optimise`: 109 items, 80 applied, 7 wontapply; memory 81%, serialization 78%.
- `/review-plan`: 810 items over 38 plans (~21/plan), **794 merged, 0 discarded** — merge is rubber-stamped; 15% critical.
- Vet drops: package-quality 24%, completeness 17%, architecture 14% (+10% downgrade); review-plan 2.3%; optimise 0%. Post-tiering (since 2026-09-29) deep 0/55, lite 2/60. `dropped_count` often > `sampled_count` (drops mix pre-persist pruning with spot-check failures).
- Top vet-drop cause: **confirmations reported as findings** ("NOT a bug … flagging only to record that I checked"); also leaks in as 28 `verified-clean` items. Then refuted-by-reading (bad grep / stale comment), fabrication, design-intent/ADR, speculative/style, out-of-scope.
- Apply-time rejections: out-of-budget "requires deliberate refactor" (should be `needs-plan`), documented divergence, premature for design, negligible-gain-vs-risk (optimise).
- `rounds` signal unused (no item ≥3; two at 2); `needs-plan` ~8 items ever; `discard_reason` never used; ~11% of review items carry ad hoc categories.
- `[[vet_events]]` and `[[rollback_events]]` are write-only — nothing reads them; no per-lens precision/recall measurement exists.

### Prior plans
`flow-commands-hardening.md` (04-24: 5 lenses, security cap 5, testability, plan-review persistence + anchor merge), `specialised-flow-agents.md` (04-29: tiers), `delegated-shimmying-clarke.md` (05-08: universal vet skeleton, `[[vet_events]]`). Commit 491f747 (09-29): "research-vet failure tracks the lens, not the tier".

### Orchestration substrate
The Workflow runtime offers `agent(prompt, {agentType, schema, effort})` with JSON-schema structured output, `pipeline()` (find→verify with no barrier), and a carrier's own instructions count as opt-in. Constraints: session "workflow size guideline" (default medium, <10 agents), no filesystem in the script, availability per environment.

### Backlog
- `B-c5783e08` (question, open) — vet_events append stamps `last_updated` before the interim checkpoint. `flow-contract-vet-research` step 6 now passes `--no-stamp` (commit 57f4737), so likely already resolved — fold-in candidate (verify + resolve).
- `B-edb20f98`, `B-124a580e` — tomlctl test-suite items, unrelated.

### Orchestrator context and Haiku hand-off (reference, 2026-10-12)
Recorded 2026-10-12 as reference; the 2026-10-12 rewrite adopted its v2 bearings (agents persist their own output, a Haiku survey stage, Haiku agents by `agentType`, fallback parity) into Approach, "Run pipeline" and Approach, "Agents". Measured 2026-10-12 by scratch scripts over the 60 most recent flow transcripts in `~/.claude/projects/` (dev-tools, reportdesignkit, Intembeko-ReportMigration). The scripts attributed main-thread tool inputs and results to command and category, at chars/4 ≈ tokens. Peak orchestrator context: `/implement` averaged 635k (max 966k); `/review`+`/review-apply` 401k; `/plan-new` 375k; `/review-plan` 329k.

**What already landed.** Since about 2026-10-01, `/implement` dispatch prompts pass the plan narrative by path or line range and fetch the task by id. Average prompt size fell from 8–17k to 3–5k chars. Two things still repeat:
- `claude/commands/implement.md` (Phase 1 plan-path validation, Phase 2 frontier) still says "embedded verbatim" / "byte-identical literal preamble". `a4f44ff7` (Intembeko, 10-06) followed it literally, at 13.8k chars × 9 prompts.
- A ~2–3k "COMMON CONSTRAINTS" block, mostly restating agent definitions and CLAUDE.md, is pasted into 40–80 prompts per run: 12–57 ktok of orchestrator output per session.

**Haiku candidates.** Each is mechanical, can rebuild its inputs from the flow files and git, and is tomlctl-validated or reversible:
- **`/implement` checkpoint clerk**: task-completion and deviation records, the gate record, `tasks train`, commit-conventions, selective staging, `tasks update --commit`. Saves ~25–50 ktok per session. The orchestrator passes the slug, the checkpoint, a gate digest, per-task vet/tier/retries, and its own deviations. In-progress and done status writes stay with the orchestrator. The clerk halts on an unexpected dirty path, a hook failure, or a non-pending file sharer. Precedent: `3ea6b5aa` delegated a 15-commit train.
- **Apply closing clerk** (`/review-apply`, `/optimise-apply`): reconciliation, ledger mutation, `vet_events`, Step 6 sync, backlog mints. Saves ~25–30 ktok and 10–35 turns.
- **Ledger scribe**: v1 orchestrators re-type 17–59k chars of findings into staging files, sometimes twice when a heredoc breaks (`3e857d14`, `281211f7`).
- **`/plan-new` Phase 9 store bootstrap**: saves ~6–20 ktok. Phase 9 also loads `flow-contract-execution-record-schema` without using it.
- Once clerks own the writes, the orchestrator could skip the tomlctl, task-store, record-schema, ledger-schema, reconciler, backlog-capture and in-flow `/plan-update` loads: ~35–45 ktok per apply session.

**Stays with the orchestrator**: lite-vet sampling (Haiku checking Opus output defeats the gate; a Haiku verification agent has misreported a pass before), deviation, escalation and held-file decisions, plan authoring, the `/review-plan` merge (only 2–14 of 23–34 `anchor_new` strings land verbatim), consolidation, and the two-control probes.

**Bearing on v2.**
- **Bloat absorbed.** The Workflow substrate absorbs the per-dispatch preamble (written once in `args`) and the per-agent completion notifications (one return). Workflow subagents get CLAUDE.md injected, so the `CONSTRAINTS` fence is redundant there.
- **One large return.** The script has no filesystem, so stage 5's return carries every finding with its verdicts, and stage 7 has the carrier re-type them as NDJSON. That is the ledger-scribe cost again, with larger rows. Options, open for decision:
  - **(a, preferred)** The verifier appends `finding + verdicts` through a validating verb such as `tomlctl review stage`, and the workflow returns only an index. This needs a variant of `research-read-only`.
  - **(b)** A per-lens Haiku scribe stage re-emits the rows. Text drift is a risk that `record --dry-run` cannot see.
  - **(c)** `record` ingests the run's `journal.jsonl`. That file is an internal format.
- **Context pack.** Stages 1–3 (scope, PRIOR, BACKLOG, baselines, CALIBRATION, routing greps) could be a Haiku context-pack stage. It would write the preamble to a file and return signals plus fence status, with lens selection done in JS from the catalogue table in `args`. `run start` would then follow routing.
- **Fallback parity.** The Agent-wave fallback should keep the same persistence contract, or `substrate` A/B also measures orchestrator behaviour.
- **Model placement.** Clerks carry `model: haiku` in their frontmatter and are called by `agentType`, never by `opts.model`. Because verification guards precision, `review-finder-lite` on Haiku is an A/B candidate measured with `review stats`. Keep `review-verifier` and the audit on Opus.
- **`/implement` on a Workflow later.** It would run one workflow per checkpoint segment: a JS frontier with file claims, then the gate, then the Haiku clerk when the gate is green and no decision is pending. Otherwise it returns uncommitted. Glimpse's `in-progress` view would then need implementers to write their own status, or rely on `/workflows`.
- **Sequencing.** The Success Criteria guard freezes `implement-deep.md`, `implement-lite.md`, `flow-contract-task-store`, `backlog-capture` and the `flow-contract-apply-*` skills.
  - Can land before v2: the `/plan-new` clerk, and a checkpoint clerk worded inside `implement.md` plus a new agent file.
  - Waits for v2 or a guard rebase: implementer report files, the apply clerk, and the task-store writer rule.

## Research Notes

Vet: Agent-1 (sota-review, deep) — 3 sampled (F1, F3, F8), 0 dropped, 1 downgraded (F8: arXiv abstract confirms the false-consensus result and structured disagreement, but not the verdict labels or F1 figures). Agent-2 (cc-platform, lite) — 3 sampled (version, prompt caching, Workflow), 0 dropped, 0 downgraded. Plan mode with no flow ledger yet, so no `[[vet_events]]` were written; this line is the record. No dependency manifests intersect scope (markdown harness plus small Rust const/table edits), so there was no library enumeration.

### R1 — Verify every finding independently; sampled anchor checks are not verification (high)
Anthropic's `code-review` plugin launches one validator subagent per issue (Opus for bugs, Sonnet for CLAUDE.md issues) and drops anything not validated, with no numeric score (raw.githubusercontent.com/anthropics/claude-code/main/plugins/code-review/commands/code-review.md, fetched 2026-10-09). The plugin dropped its Haiku 0–100 score threshold in Dec 2025. Cursor BugBot (Jan 2026) and BitsAI-CR (arXiv 2501.15134, 75% production precision) run the same stage. Claude Code v2.1.295's built-in `/code-review` keeps CONFIRMED (trigger nameable) and PLAUSIBLE (mechanism real, trigger uncertain), drops REFUTED, and caps at 32 findings with `failure_scenario` + `short_summary`. Self-Correction Bench (2507.02778): models miss their own errors far more than others' — fresh context matters. **Impact**: add a Verify stage between lens return and checkpoint; the orchestrator vet shrinks to auditing verifier verdicts.

### R2 — Require a concrete failure scenario (high)
`security-review.md` requires an exploit scenario and >80% confidence; `code-review.md` flags only code that "will definitely produce wrong results"; AnyPoC (2604.11950) — executable PoC validation rejects 9.7× more false positives. A confirmation cannot fill the slot, so the top vet-drop class fails mechanically. **Impact**: `failure_scenario` field, mandatory for `warning`/`critical` on correctness-type lenses (gate it per lens — design/DX lenses have no input→wrong-output form).

### R3 — Drop the "target 15"; yield follows the code (medium)
Anthropic Code Review: PRs <50 lines average 0.5 issues, >1,000 lines 7.5 (claude.com/blog/code-review, verified). Cursor 0.4–0.7 bugs/run; Copilot: "silence is better than noise", 29% of reviews post nothing. Our 6×15 target is one to two orders of magnitude above whole vendor teams. Cursor's fix for over-caution was aggressive finders *plus a validator*, not a count. **Impact**: ceiling only; an empty return is normal; recall comes from R6, not quotas.

### R4 — Our dispositions measure apply throughput, not precision (high numbers / medium interpretation)
94% fixed, 794/0 merged/discarded, vs 36.4% acceptance in the only large independent study (CodeRabbit, 2607.03316). Production systems measure precision with a signal from outside the reviewer: developer thumbs, whether the flagged lines changed, an LLM judging resolution at merge. **Impact**: a blinded sample audit per lens, plus revert tracking.

### R5 — Per-category feedback suppression works (high)
AutoCommenter (Google, 2405.13565): suppressing 17 non-actionable categories raised the useful ratio from 54% to 66%. A single global confidence threshold failed; per-category thresholds worked. CodeRabbit: rejection is predictable from history (76% F1). **Impact**: aggregate outcomes by lens, feed the worst classes back as suppressions. This needs normalised lens names first (vet_events today uses `Algorithm`/`algorithm`, `queries`/`query`).

### R6 — Recall from repeated randomized passes, precision from the verifier (medium)
SWR-Bench (2509.01494): merging 10 runs raised recall 118.8% and F1 43.7%; n≈5 is the sweet spot; single passes overlap little. Cursor runs 8 passes with randomized diff order. "Debate or Vote" (2508.17536): voting explains most of the gains attributed to debate. **Impact**: depth modes control the number of passes (loop-until-dry on verified findings); no debate rounds. Measure two-run overlap before relying on it.

### R7 — Don't add lenses or personas for quality; partition by evidence type (low–medium)
MARG: multi-agent raised recall but lowered precision. Heterogeneous vulnerability detection (2604.21282): removing the verifier dropped precision from 62.9% to 52.6%. Anthropic's plugin splits by evidence: a diff-only bug hunt with no extra context, CLAUDE.md compliance, introduced-code logic/security. **Impact**: no lens proliferation; consider one context-free diff-only correctness pass.

### R8 — Verifiers must concede only on evidence (low — `_orchestrator-downgrade: labels/F1 not on abstract page`)
Adversarial Review (2608.18167, ICML 2026 DL4C workshop): a naive reviewer–critic pair falls into false consensus; structured disagreement prompting scored best. **Impact**: verifier verdicts require a `file:line` or command citation for both confirm and refute.

### R9 — Plan critique: simulated executor and explicit checks beat open critique (medium)
PCBench (2505.23715): models need explicit prompts to find errors, and procedural errors are the hardest. Ambig-SWE (2502.13069): models judge underspecification poorly by inspection. "Ask or Assume?" (2603.26233): a dedicated underspecification detector raised resolve rate from 61.2% to 69.4%. Valmeekam: LLM plan self-critique was right on 61/100 with many false positives. Evidence for pre-mortem prompting is weak (practitioner only). **Impact**: an executability lens that dry-runs each task (lists invented decisions + files touched vs the Files line).

### R10 — Perf findings need a measured profile (high)
GSO (2505.23671) and SWE-fficiency (2511.06090): agents mislocalize bottlenecks. PerfAgent (2607.19653): profiler guidance raised expert-matching patches from 26% to 74%. Mutation study (2606.15689): perf bugs had 0% recall for 4 of 5 models. Our `/optimise` vet dropped 0/138, which means the vet cannot fail perf claims, not that they are right. **Impact**: an orchestrator `PROFILE BASELINE` fence; findings off the measured path are capped at suggestion/hypothesis; apply requires a before/after measurement for ≥warning.

### R11 — Recall eval from replaying our own fixed findings (medium)
Greptile re-opened 50 bug-introducing commits as PRs; Qodo injected 580 bugs; Martian scores recall against post-review developer fixes; c-CRAB (2603.23448) uses hidden-test validation. Our 842 `fixed` items with fix commits are a golden set: revert a sample, re-run the lens, score hits at file+symbol. Counter: self-found bugs bias the set, so add backlog/human-found bugs. **Impact**: a recall measurement harness.

### R12 — Vendor precision figures are marketing (high that they are self-reported)
Keep vendor percentages out of acceptance criteria.

### R13 — Platform facts, Claude Code v2.1.295 (high; code.claude.com docs fetched 2026-10-09)
- **Prompt cache**: "A subagent starts its own conversation with its own system prompt… Its first request doesn't read the parent's cache." The API writes a cache entry only at the breakpoint, and it becomes readable only after the first response begins. Only Workflow fan-outs stagger same-prefix agents (5 s) so siblings hit. **Our "byte-identical preamble for prompt-cache reuse" rationale (`review.md:59`, `optimise.md:67`, `test-bootstrap.md:61`, `implement.md`) earns ~no hits for simultaneous Agent dispatches**, and `/review`'s mixed tiers can never share. The shared-context preamble is still worth keeping for consistency; the cache justification is wrong.
- **Structured output**: Agent/Task has no schema parameter. Only Workflow `agent(prompt,{schema, agentType, effort})` validates output (≤5 retries).
- **Workflow**: generally available (all paid plans/API/cloud; on Pro, enable in `/config`). 16 concurrent agents, 1,000/run, `workflowSizeGuideline` advisory (default medium <10; small on Pro). Saved scripts in `.claude/workflows/` or `<CLAUDE_CONFIG_DIR>/workflows/`. Can be disabled (`disableWorkflows`). No filesystem, no mid-run user input.
- **Frontmatter**: `skills` (preload at start — could replace the in-body `Skill(research-methods)` call), `omitClaudeMd`, `experimental.cacheTtl`, `effort` low–max, `maxTurns`, `hooks`, `disallowedTools`, `background`, `isolation: worktree`.
- **SubagentStop hook**: receives `last_assistant_message`; `decision:"block"` + `reason` sends the agent back to work — a mechanical finding-record validator is possible. Agent-scoped hooks don't run in untrusted folders or in plugin agents.
- **Agent teams**: still experimental (`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`); the docs say a teammate's idle notification includes its final answer, contradicting CLAUDE.md:31 (unverified against observed behaviour). Keep avoiding `name:`.

Searched (initial research): Anthropic code-review and security-review plugin sources; the Claude Code Review, Cursor BugBot, Greptile v4/benchmarks, Graphite, GitHub Copilot and CodeRabbit posts; arXiv 2501.15134, 2405.13565, 2509.01494, 2508.17536, 2507.02778, 2604.11950, 2607.03316, 2608.18167, 2505.23715, 2502.13069, 2603.26233, 2505.23671, 2511.06090, 2607.19653, 2606.15689, 2603.23448; code.claude.com sub-agents, prompt-caching, workflows, hooks, agent-teams, tools-reference and code-review pages. All fetched 2026-10-09; newest source 2026-10-06. Dead ends: no LLM-specific pre-mortem study; no head-to-head of persona vs topic lens partitions; no Martian methodology page.

### Directed research additions

D1. **Bespoke `review` group, not generic `items`** (high). `items_add_value_to` always stamps a `dedup_id` (`tomlctl/src/items.rs:301-307`). Each `items` write touches one array, so a run and its findings cannot land atomically. A new flow-less dir isn't in `SCHEMA_SEEDED_LEDGER_DIRS` (`io.rs:728-765`). `clusters.rs:29`, `orphans.rs:99` and `items_sweep.rs:269,459` hardcode the `items` array. Model on `tasks/store.rs:61-85` (`io::mutate_doc` + typed structs). Mint with `items::items_append_minted` (`items.rs:1950`, `DedupId::Skip`). Compare-and-set as `ledgers.rs:369` `write_guarded`.

D2. **Registration checklist** (high): `cli/types/review.rs` + `cli/types/mod.rs` (re-export, `FEATURES`, `SUBCOMMANDS`) + `cli/types/cmd.rs` + `cli/dispatch.rs` + `capabilities.rs` + `tests/capabilities.rs` + `lib.rs`. Output only via `crate::output` (`output_gate.rs:83`). Never redeclare a global output flag (`output_gate.rs:128` `the_command_tree_has_no_duplicate_flags`).

D3. **No `LedgerKind` variant** (high): glimpse's exhaustive match (`glimpse/src/ledger.rs:38`) and `LedgerKind::ALL` watchers (`watch.rs:70,246`) would break or misread.

D4. **No new artifact key** (high, vetted): `flow/doctor.rs:530-566` fails `artifacts-canonical` for any missing non-`tasks` key on every existing flow. Derive the path from the slug instead.

D5. **Envelope `--command`** (high, vetted): `VALID_COMMANDS` is checked at runtime only (`flow/envelope.rs:57`). `flow-bootstrap.md` step 5 reads plansDirectory only for `plan-new|plan-update|review-plan`, so v2 carriers pass the v1 names.

D6. **Inputs** (medium): `LEDGERS` is closed (`inputs.rs:58`, vetted). If two carriers sweep the same ledger, one silently never sees a record, so v2 skips the sweep.

D7. **Cross-run match key** (high): `dedup_id` hashes `file|summary|severity|category|symbol` verbatim (`dedup.rs:33-34,145`), so a reworded summary splits. Use family + `(file, symbol)`, else `(file, line ±10)` (`dedup.rs:363-368` window), then exact summary; keep `lens` out of the key.

D8. **export-v1** (high): the CLI write path checks only `id` (`items.rs:338-348`), and apply skips malformed rows (`flow-contract-ledger-schema` read rules). v1 required: id, file, line, severity, effort ∈ trivial/small/medium, category, summary, first_flagged, rounds, status. Optimise categories are closed and fail-soft to `memory`. Idempotency via `v2_ref`. Lock order is v1 then v2 (`io.rs:1351-1382`, per-file locks).

D9. **Text gates** (high): the `carrier_invokes_required_skills` table is opt-in per carrier (`skills.rs:1405`). `command_lint` parses `tomlctl` lines in bash fences of commands, agents, SKILL.md and references, so the Rust group must land first. The tomlctl skill description is at 1012/1024 characters (vetted). Workflow `.js` is not linted.

D10. **Agent correlation** (high): `agents/correlate.rs:201` regex hardcodes the v1 basenames and `[ROP]`; use a distinct v2 prefix.

D11. **Workflow distribution** (high on the docs; medium on portability): `scriptPath` is readable only where the session can read it ("only from a script file the session is already allowed to read", code.claude.com/docs/en/workflows). Saved workflows live in `$CLAUDE_CONFIG_DIR/workflows/` (`Workflow({name})`) or a plugin `workflows/` dir. `agentType` composes with `schema` (workflow-authoring reference). `deploy-claude.ps1` junctions whole skill dirs (`:26,98`) and targets `$HOME/.claude`. On this machine `~/.claude-work/skills/*` are regular-file copies synced out of band (vetted with `stat`).

Searched (directed): tomlctl source as listed in each finding; code.claude.com/docs/en/workflows and /agent-sdk/typescript (Workflow section), fetched 2026-10-09; the bundled workflow-authoring reference. Not found: the mechanism that syncs `~/.claude-work`.

### Design critique (2026-10-12)

Five parallel `research-deep` critics reviewed the plan for soundness, strategy and effectiveness (executability and feasibility were left to `/review-plan`). The lenses were strategy and vision coherence, contrarian alternatives, effectiveness against the v1 ledger evidence, Haiku roles and context economics, and catalogue maintainability and portability. Their convergent findings, and how the plan now answers each:

- **Coverage arithmetic** (strategy, contrarian, catalogue, Haiku): one concern per cell let 18 default-on code lenses exceed `standard`'s 16-cell ceiling, and a never-composed concern left no trace. → Concern bundles, `review plan --propose`, waivers enforced by `run finish`, activation tiers, a diff-scaled ceiling, repeat passes at `standard` (User Decision 24).
- **Cleanup precision** (all five): the cleanup stream ran Haiku finder → Haiku checker → apply with no audit; PLAUSIBLE behaved as CONFIRMED; "missing handling" sat in cleanup; test removals failed silently. → Grep-decidable checker with `unsettled` → Opus, routing by edit target computed in tomlctl, judgement-mode `doc-drift`/`diagnostics-dx`, cleanup in the audit sample, `--all` export exclusions (User Decision 23).
- **Cleanup bar fit** (effectiveness): 94 of 549 v1 suggestions were add-shaped with no natural `old` text, and edits-only design fixes were pushed to advisory. → `target_file`, `old` optional for add into a new file, edits-only design fixes as cleanup, `fix_shape` on defects. Measured by regex classification over every v1 review ledger; scratch output under the session scratchpad's `effectiveness/` directory.
- **Blind feedback loop** (all five): exported findings never resolved, so `regression` could not fire; `stats` read only in-run judgements (R4); calibration trained on raw verifier refutations. → `review sync-v1`, `stats --outcomes`/`--escapes`, calibration from user and audit decisions only, a REFUTED non-critical audit sample, a seeded recall check.
- **Store robustness** (Haiku, contrarian): Workflow stall restarts and relaunch replays re-run agents (code.claude.com/docs/en/workflows, fetched 2026-10-12), so direct writes duplicated rows; `same_as` outcomes depended on verdict arrival order. → Cell-scoped `add` with sealing, verdict upserts, routing echoed from `add` with a store mop-up, `review resolve` after each wave.
- **Advisory routing** (strategy, effectiveness): backlog-by-default conflicted with the frozen `backlog-capture` minting bar (`claude/skills/backlog-capture/SKILL.md`), and advisory skipped suppression. → Parked in the v2 store, user-picked backlog only, suppression at `add` (User Decision 22); `contrarian` and `synthesis` gated (User Decision 25; lumina's contrarian evidence was 7 notes on 2 stories).
- **Plan family** (strategy, Haiku, catalogue): the orchestrator still ran v1 Step 2.6 itself on a premise v2 removed, its defects were never verified, the surveyor ran the orchestrator-only task-store import, and `/review-plan` Agent 3's whole-plan half was orphaned. → The `plan-checks` cell, the stage-0 import, `plan-structure`.
- **Catalogue model** (catalogue, Haiku): repo-wide signals misattached facets in polyglot repos (this repo: `tokio` and `vue` against `tomlctl/src/sweep.rs`); Haiku-run free-text detectors failed on depth-2+ manifests and `Directory.Packages.props`; project entries could not own concerns; hand-bumped versions would drift; catalogue files escaped `command_lint`. → Signal roots and path scoping, `review survey`, frontmatter `owns` with a `project:` prefix, content hashes, the widened lint walk, a `dapper-sql` facet.
- **Orchestrator context and cost** (Haiku): an estimated 90–150k tokens and 35–45 agents per `standard` run. → Lazy skill loads, `digest --since-wave`, the report to file, the `audit` projection, `omitClaudeMd` and per-role procedures for Haiku agents, minor-defect batching, pooled checkers, an agent-count estimate.
- Also adopted: lumina-aligned severity names (contrarian critic) and user steering flags `--probe` / `--lens ±id` with a deep confirm step (strategy critic).
