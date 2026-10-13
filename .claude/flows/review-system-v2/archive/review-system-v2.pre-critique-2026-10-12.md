# Plan: Review System v2 — composed coverage, kind-routed findings, Haiku-first execution

**Plan path**: `docs/plans/review-system-v2.md`
**Created**: 2026-10-09

## Summary

Build a second review system next to the untouched v1 (`/review`, `/optimise`, `/review-plan`): three thin carriers (`/review-v2`, `/optimise-v2`, `/review-plan-v2`) over one engine skill, one agent method skill and one lens library, backed by a new `tomlctl review` verb group and a typed per-flow store. The 2026-10-12 rewrite reshapes the original draft around four decisions:
- **Findings are classified by kind**, `defect` / `cleanup` / `advisory`, not by severity alone. Cleanup — small, concrete, locally verifiable fixes — is uncapped, so wide cleanup is encouraged. Theoretical, contradicting, exploratory and counter-implementation items are `advisory`: never auto-applied, backlog-bound by default, with an orchestrator "apply now" proposal when a plan would be excessive.
- **Lens selection is composed per run, not routed from a static table.** A Haiku surveyor profiles the change and the project's health; the orchestrator composes a coverage map of *scope slice × concern* from the lens library plus ad hoc probes it authors for this change; uncovered cells trigger follow-up waves.
- **Work is pushed down a ladder: deterministic tomlctl → Haiku → Opus → orchestrator.** The orchestrator keeps judgement only — composing, adjudicating contested verdicts, triaging advisory items, directing waves. Agents write findings and verdicts straight into the store through validating verbs, and every role reads a tomlctl projection holding only the fields it needs.
- **A/B comparison is dropped.** Runs are still recorded, but to feed lens selection, not to compare systems.
- **The lens catalogue is pluggable and shared across projects.** Four entry types — lenses (concerns), facets (stack knowledge), methods (procedures) and checks (command- or orchestrator-run) — live as markdown files with TOML frontmatter in one global built-in catalogue. A project needs no setup; an optional `.claude/review-lenses/config.toml` opts entries in or out, patches them, and declares project checks. `tomlctl review catalogue` generates the index and a dispatch-test gate validates it, so no index or cross-reference is maintained by hand. v1's overloaded lenses are split into 30 composable concerns.

Scrutinise first: Approach, "Finding kinds"; Approach, "Run pipeline" (survey → compose → cover, and the orchestrator's role); Approach, "Field readership"; Approach, "Lens catalogue" (layers, the one-owner-per-concern rule, facet attachment); and Approach, "v2 store and the tomlctl review group" (`add` / `verdict` outcome rules).

## Context

The v1 review commands date from April–May 2026 and have only been patched since. Their lens sets are fixed per command and broadcast to the whole scope regardless of what changed, so coverage tracks the template rather than the change: there is no logic-bug owner, no way to aim a lens at a risky subsystem, and no lens that responds to the state of the project (a test suite that is growing faster than the code, docs drifting from behaviour). The empirical record (Exploration Notes) also shows the system's strengths and weaknesses: the `suggestion`-level output (48–65% per lens) catches many small details plans and implementations miss and is worth keeping; the noise comes from speculative, design-intent and confirmation items mixed into the same bucket, and from orchestrator context spent re-typing findings and restating constraints (Exploration Notes, "Orchestrator context and Haiku hand-off").

The user wants v1's API and harness files kept intact and v2 built as all-new files with its own tooling surface, then trialled in use rather than compared closely. Haiku is now capable enough to carry the mechanical share of the work, and the orchestrator's context should be spent on judgement.

Outcome: `/review-v2`, `/optimise-v2` and `/review-plan-v2` run end to end on this repo, compose their coverage per run, persist verified findings of all three kinds in a v2 store written by the agents themselves, route advisory items to the backlog, and apply defect and cleanup findings through the existing apply flows after export.

## Scope

- **In scope**:
  - the `tomlctl review` verb group and v2 store (Rust, with tests and docs)
  - the engine skill (`review-v2-engine`) and the agent method skill (`review-v2-method`)
  - the lens catalogue skill (`review-v2-lenses`): vocabulary, 30 lenses, 9 facets, 6 methods, 9 checks, the `tomlctl review catalogue` verb, its validation gate, and this repo's project config
  - six new agents: Haiku `review-surveyor`, `review-sweeper`, `review-checker`; Opus `review-finder`, `review-verifier`, `plan-executor`
  - the saved Workflow script and its deploy entry
  - the three v2 carriers
  - gate-table rows, shared-block registration, glimpse agent attribution for the v2 store, the run-artifact ignore rule, and CLAUDE.md
- **Out of scope**:
  - any edit to v1 carriers, agents or skills (`research-methods`, `flow-contract-vet-research`, `flow-contract-ledger-schema`, the apply contracts, `backlog-capture`)
  - A/B tooling: `review compare`, v1-ledger alignment, the blinded-sample protocol
  - a native v2 apply flow (follow-on; see User Decisions 11)
  - Haiku clerks for v1 flows (`/implement` checkpoint clerk, apply closing clerk; Exploration Notes) — a separate plan
  - edits to lumina and the lumina-story-blocks plugin (where lumina's research lenses overlap the catalogue, the catalogue is written so lumina could adopt it later; Approach, "Lens catalogue"); glimpse display or triage of the v2 store; the user-inputs store for v2
  - the replay recall-eval harness; retiring v1
- **Affected areas**: `tomlctl/src/review/`, `tomlctl/src/lib.rs`, `tomlctl/src/cli/` (including a new `tomlctl/src/cli/dispatch/tests/catalogue.rs` gate), `tomlctl/src/capabilities.rs`, `tomlctl/src/agents/correlate.rs`, `tomlctl/tests/`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `claude/skills/review-v2-engine/`, `claude/skills/review-v2-method/`, `claude/skills/review-v2-lenses/`, `claude/skills/tomlctl/`, `claude/agents/`, `claude/commands/`, `claude/workflows/`, `.claude/review-lenses/`, `scripts/shared-blocks.toml`, `scripts/deploy-claude.ps1`, `.gitignore`, `CLAUDE.md`

## User Decisions

> User answers are recorded as data. Decisions 1–8 are from the 2026-10-09 planning session (the subset still standing); 9–16 are from the 2026-10-12 rewrite session.

1. **Staging** — v1 carriers, agents and skills stay byte-untouched; v2 is all-new files plus a v2 tooling surface. lumina is out of scope. User's words: "let's try to retain the old api and harness files creating all new files and a v2 review surface in the tooling".
2. **Substrate** — Workflow with an Agent-tool fallback. Refined 2026-10-12 by decision 14: agents persist their own output, so the Workflow return is an index, not the findings.
3. **Verification** — a fresh-context, refute-first verifier for defects, citing `file:line` or command output for every verdict; `deep` adds a 3-vote panel for criticals. Refined 2026-10-12: verification is per kind (Approach, "Finding kinds").
4. **v2 naming** — parallel `-v2` names over one shared engine.
5. **v2 store** — a new `tomlctl review` verb group over one store per flow, plus `review export-v1` into the existing ledgers for the untouched apply flows.
6. **Checkpoint cadence** — milestones.
7. **Backlog fold-in** — none.
8. *(superseded 2026-10-12)* The fixed `/review` lens re-partition (core / routed / project) is replaced by composed coverage (decision 15).
9. **Keep small findings** (2026-10-12) — do not reduce what v1 calls `suggestion`; small and trivial issues are wanted. User: "a plan and implementation miss many small details along the way and the current suggestions category catches many of these."
10. **Finding kinds** (2026-10-12) — `defect` / `cleanup` / `advisory`. Theoretical, contradictory, exploratory, probing and counter-implementation items (swapping a dependency, overarching architecture changes) are `advisory`. Tests and docs are **lenses**, not kinds; project-health signals can activate maintenance lenses (e.g. cutting low-value tests when the test count is out of hand, consolidating docs).
11. **Advisory routing** (2026-10-12) — backlog-bound by default; the orchestrator may propose immediate apply when a new plan would be excessive for the change. **Apply path** — the user did not choose between `export-v1` and a native v2 apply; this plan takes `export-v1` (smaller scope, reuses the untouched apply flows) and leaves a native v2 apply as a follow-on.
12. **A/B downplayed** (2026-10-12) — the user will trial the v2 flows rather than compare them closely. `review compare` and the A/B protocol are dropped; run records stay because lens selection feeds on them.
13. **Haiku first-class** (2026-10-12) — Haiku agents are a designed tier, not an afterthought; the orchestrator is kept for judgement, analysis and direction as much as possible.
14. **Direct store writes with per-role projection** (2026-10-12) — agents write findings and verdicts through validating `tomlctl review` verbs. User: "more information is good as long as good details are not crowded out. We carefully consider which fields are read by which role such that key details for the work are not overlooked and required context is not crowded out."
15. **Dynamic lens selection with ad hoc probes** (2026-10-12) — the user's main concern with v1 is that lenses are static and not selected to give the coverage required. The orchestrator composes each run's coverage and may author ad hoc probes.
16. **Lens catalogue format** (2026-10-12, lens session) — catalogue entries are files with TOML frontmatter, and the index is generated by `tomlctl review catalogue` rather than maintained by hand.
17. **Granularity** (2026-10-12) — split v1's overloaded lenses into finer concerns so composition has more to choose from.
18. **Families** (2026-10-12) — one lens may span families, with a probe section per family (e.g. `ripple` for code and plan).
19. **Facets and checks are first-class** (2026-10-12) — effectiveness across repos with easy repo-specific variation is important.
20. **Layers** (2026-10-12) — the catalogue is primarily shared across projects, with minimal opt-in/opt-out configuration per project. User: "I want the harness to be as portable across projects as possible without excessive setup in each."
21. **lumina** (2026-10-12) — kept in scope where it overlaps the catalogue, but not targeted specifically.

## Approach

### Design principles

1. **Push work down the ladder.** Every step goes to the cheapest executor that can do it reliably: deterministic `tomlctl` (validation, identity, counts, projections, reports) → Haiku (surveying, mechanical sweeps, mechanical checks) → Opus (judgement findings, refute-first verification, plan dry runs) → the orchestrator (composition, adjudication, triage, direction). A step moves up only when the one below cannot do it reliably; Haiku never judges Opus judgement.
2. **The orchestrator reads digests, never rows.** It never re-types a finding, never pastes constraints the agents already carry, and reads the store only through `review digest` and role projections. Its context is spent on decisions.
3. **Coverage is composed, then accounted for.** Lenses are chosen per run against a profile of the change and the project, ad hoc probes fill what the library lacks, and every cell of the coverage map reports covered, thin or uncovered.
4. **Classify by kind, then by weight.** What a finding *is* decides how it is verified, capped, applied and routed (Approach, "Finding kinds"); severity is a property of defects only.
5. **Breadth in cleanup, discipline in claims.** Small concrete fixes are uncapped; defects must state a failure; advisory items must state rationale and adoption cost. Confirmations are never findings.
6. **Each role sees what it needs.** Field readership is designed, enforced by tomlctl projections, and length-capped so key details are not crowded out (Approach, "Field readership").
7. **v1 stays byte-identical.** v2 reuses unchanged v1 skills by invocation (`tomlctl`, `flow-contract-flow-context`, `backlog-capture`, `flow-contract-task-store`, `flow-contract-plan-output-format`, `flow-contract-plansdirectory-prompt`, `research-methods`) and never edits them.

### Architecture

```
/review-v2 ─────┐                                   ┌─ review-v2-lenses  (library: lens-<id>.md + .claude/review-lenses/*.md)
/optimise-v2 ───┼─ review-v2-engine (orchestrator) ─┼─ review-v2-method  (agent side: per-role procedures, store writes)
/review-plan-v2 ┘          │                        └─ research-methods  (unchanged v1: sources, grades)
                           │
   run start → SURVEY (Haiku review-surveyor) → COMPOSE (orchestrator: cells) →
   WAVE: Workflow `review-v2-pipeline` | Agent-wave fallback
         cell → review-finder (Opus) | review-sweeper (Haiku) | plan-executor (Opus)
              → each writes rows via `tomlctl review add`, coverage via `review cover`
         defect → review-verifier (Opus)   cleanup → review-checker (Haiku)   advisory → —
              → each writes `tomlctl review verdict`
   → DIGEST (tomlctl) → follow-up wave? (orchestrator) → audit + triage (orchestrator + user)
   → export-v1 / backlog / run finish / report (tomlctl)
                           │
   .claude/flows/<slug>/review-v2.toml  or  .claude/review-v2/<scope>.toml
   run artifacts: .claude/review-v2-runs/<store-key>/U<n>/  (git-ignored: profile.md, pack.md, baselines)
```

### Finding kinds

| Kind | What it is | Required beyond the common fields | Verified by | Count cap per cell | Destination |
|---|---|---|---|---|---|
| `defect` | Something is wrong: wrong output, crash, exploit, broken contract, plan step that fails, a measured or complexity-argued perf cost | `severity` (critical \| warning \| minor), `failure_scenario`, `fix`, `counter`, `grade`, `evidence` (≥1) | `review-verifier` (Opus), refute-first; 3-vote panel for criticals at `deep` | 10 (ceiling, no target) | apply via `export-v1` |
| `cleanup` | A concrete, local, verifiable improvement: stale or wrong comment, dead code, naming drift, inconsistency with the converged pattern, a missing test or handling, doc drift, small DX gap, a low-value test or doc to remove | `edit`, `direction` (add \| remove \| change), `old` (the exact text at the anchor, ≤400 chars), `effort` | `review-checker` (Haiku), mechanical predicates; `review add` already proves `old` exists | none — one pattern across many sites is one row with `instances` | apply via `export-v1` (as v1 `suggestion`) |
| `advisory` | Theoretical risk, a challenge to a documented decision, an exploratory probe result, a counter-implementation (swap a dependency, re-architect, change direction) | `angle` (risk \| challenge \| probe \| alternative \| direction), `rationale`, `adoption_cost`; `challenges` when `angle = challenge`; optional `alternative` | none true/false — the orchestrator and the user judge it | 3 | backlog by default; orchestrator may propose apply-now (converts to cleanup or defect) |

Rules:
- **Never a finding**: a confirmation ("checked X, it is fine"), anything the `LINT BASELINE` reports, a restatement, or an item already in the store as dismissed, deferred or refuted (cite it via `same_as`). Clean checks go in the cell's coverage note. The confirmation class is named because it is v1's top vet-drop cause; the kind-specific required fields block it mechanically (a confirmation has no failure, no edit and no adoption cost).
- **A documented deliberate choice is not dropped**: a finding against it is `advisory` with `angle = challenge` and `challenges` naming the decision (ADR, comment, CLAUDE.md line). v1 discards this signal as a vet drop.
- **Defect vs cleanup**: if the current behaviour is wrong for some nameable input or state, it is a defect; if the behaviour is right and the code, test or prose around it is not, it is cleanup. A design finding with no consequence chain is advisory.
- **Severity rubric (defects)**: `critical` — data loss, a security hole, shipping broken, or a plan that cannot be executed as written; `warning` — a real defect with a nameable trigger; `minor` — a real defect with a narrow or unlikely trigger. Each lens file adds a calibration line.
- **`failure_scenario`** by family: code — inputs or state → wrong output, crash or exploit; perf — workload and input size → cost with magnitude; plan — the execution step → how it fails.
- **Apply-now conversion**: `review triage <F> --convert cleanup|defect` with the concrete `edit`/`fix` staged by the orchestrator mints a new finding of that kind with an `orchestrator` verdict and resolves the advisory as `converted` with `converted_to`. Breadth never decides it; the test is "does adopting this need decisions, or only edits?".

### Field readership

The field catalogue lives once, in `tomlctl/src/review/fields.rs`: every field's name, kind applicability, required-ness, length cap and per-role visibility. `review add` validates against it, `review schema` emits from it, and `review show --for <role>` projects through it, so the readership below is enforced rather than advised. A unit test asserts the projection table matches this matrix.

| Field | finder (`PRIOR`) | verifier | checker | implementer | orchestrator digest | backlog | report |
|---|---|---|---|---|---|---|---|
| `id`, `kind`, `file`, `line`, `symbol`, `summary` (≤160) | R | R | R | R | R | R | R |
| `status`, dismiss/defer reason | R | — | — | — | R | — | R |
| `lens` (or `probe:<slug>`), `cell` | — | R | — | — | R | R | R |
| `severity` | — | R | — | R | R | — | R |
| `failure_scenario` (≤600) | — | **R — the claim to refute** | — | R — defines done and the regression test | contested only | — | R |
| `counter` (≤300) | — | R — first probe | — | — | — | — | — |
| `evidence` (≤5 × 300) | — | R | — | — | — | first 2 | — |
| `grade` | — | **withheld** | — | — | R | — | — |
| `fix` (≤800) | — | **withheld** | — | R | — | — | — |
| `edit` (≤800), `direction`, `old`, `effort`, `instances` (≤50) | — | — | R | R | count only | — | `edit` |
| `angle`, `rationale` (≤800), `alternative`, `adoption_cost`, `challenges` | — | — | — | — | **R** | R | R |
| `same_as` + its target's summary and status | — | R (confirm or strike) | R | — | — | — | — |
| latest verdict + `citation` | — | — | — | citation only | R | — | R |

- The verifier never sees `grade` or `fix`, so the finder's confidence and proposed solution cannot anchor it.
- The orchestrator's digest carries counts, coverage gaps, contested findings and the advisory list in full; it carries no evidence, and defect detail only for contested items.
- The implementer gets the fix and what "done" means, never the argument that the finding is true. `export-v1` builds the v1 `description` from the implementer projection.
- Over-cap values are refused at `review add` with the field and its cap named, so an agent tightens the row instead of the store silently truncating it.

### v2 store and the tomlctl review group

**Shape.** A bespoke verb group, not generic `items`: an `items add` always stamps a `dedup_id`, a row and its verdict could not land in one write, and `clusters`, `orphans` and `items sweep` hardcode the `items` array (Research Notes, D1). The store follows the `tomlctl/src/tasks/store.rs` idiom — typed structs converted by hand-written `from_toml`/`to_toml` as `tomlctl/src/tasks/schema.rs` does (no `serde` derive, no new dependency), written through `io::mutate_doc` with an `OnMissing::Create` seed for lock, sidecar, auto-create and `last_updated` stamping. The review store's `mutate` hands its closure the raw document as well as the typed view, because minting (`items::items_append_minted`, `DedupId::Skip`) and compare-and-set (`items::compute_apply_mutation_with(…, StalePolicy::Skip)`, as `tomlctl/src/ledgers.rs` `write_guarded` does) take the raw `TomlValue`. The typed `Store` round-trips the root `id_high_water` table. Arrays: `runs` (prefix `U`), `cells` (prefix `C`), `findings` (prefix `F`), `verdicts` (no ids). None is named `items`.

**Concurrency.** Up to 16 agents write one store concurrently. Every write is a short `mutate_doc` call under the file lock; ids are minted inside the lock. The Windows sidecar-rename `os error 5` is a known transient (it is in the method's retry-once rule).

**Paths.** `--slug <slug>` → `.claude/flows/<slug>/review-v2.toml`; `--scope <scope>` → `.claude/review-v2/<scope>.toml`; both validated by `validate_slug`. No new artifact key (it would fail `flow doctor` `artifacts-canonical`, D4) and no `LedgerKind` variant (glimpse, D3). Run artifacts go under `.claude/review-v2-runs/<store-key>/U<n>/`, where `<store-key>` is the slug or `scope-<scope>`; `review run start` prints the directory.

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
substrate = "workflow"             # workflow | agent-waves
models = { surveyor = "haiku", sweeper = "haiku", checker = "haiku", finder = "opus", verifier = "opus", executor = "opus" }
signals = { trust-boundary = true, "lang:rust" = true, test-growth = "412 → 655 tests over 30 days" }
catalogue = { builtin = "~/.claude/skills/review-v2-lenses", project = ".claude/review-lenses", config_sha = "…" }
waves = 2
counts = { cells = 14, uncovered = 0, defect = 6, cleanup = 23, advisory = 4, refuted = 5, suppressed = 2 }   # computed by `run finish`
audit = { sampled = 3, overturned = 0 }

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["tomlctl/src/review/add.rs", "tomlctl/src/review/verdict.rs"]   # paths, globs, or plan task refs
concern = "correctness"            # a catalogue lens id, or probe:<slug>
question = "…"                     # required for a probe; an optional focus note for a lens
lens_version = 1                   # resolved through the catalogue by `review plan`
path = "~/.claude/skills/review-v2-lenses/lenses/correctness.md"
facets = ["rust", "tokio"]         # auto-attached by signal, adjustable per cell by the orchestrator
methods = ["trace"]
v1_category = "quality"            # copied from the lens for `export-v1`
agent = "review-finder"            # review-finder | review-sweeper | plan-executor
reason = "identity outcomes are the store's riskiest logic"
status = "covered"                 # assigned | covered | thin | uncovered
note = "…"                         # the agent's coverage line (≤600): clean checks, sweeps run, what was not reached

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "defect"                    # defect | cleanup | advisory
lens = "correctness"
file = "tomlctl/src/review/verdict.rs"
line = 88
symbol = "verdict::apply"
summary = "…"
# kind fields per Approach, "Finding kinds"; plan family adds plan_section, anchor_old, anchor_new
same_as = ""                       # finder-declared identity claim, F<n> or empty
depends_on = []
related = []                       # anchor candidates not declared the same finding
anchor = "code|tomlctl/src/review/verdict.rs|verdict::apply"   # computed by tomlctl
status = "open"                    # unverified | open | refuted | merged | dismissed | deferred | backlogged | converted | resolved | exported
first_seen = 2026-10-12
# companions: merged→merged_into; dismissed→dismiss_reason; deferred→defer_reason+defer_trigger;
#             backlogged→backlog_ref; converted→converted_to; resolved→resolution+resolved;
#             exported→exported_as+export_ledger

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"                    # verifier | panel | checker | audit | orchestrator
verdict = "confirmed"              # confirmed | plausible | refuted
citation = "tomlctl/src/review/verdict.rs:88"
rationale = "…"                    # ≤600
```

**Verbs.** Flag tables go in `claude/skills/tomlctl/references/review.md`. Every verb except `schema` and `catalogue` takes exactly one of `--slug` or `--scope`; `stats` also accepts `--all`. No verb declares a flag that collides with a global output flag (`the_command_tree_has_no_duplicate_flags`).
- `review run start --command --family --depth --base --builtin <dir> [--project <dir>] [--set-json scope=… --set-json models=… --set substrate=…]`: mints `U<n>`, stamps `started`, records the catalogue roots and the project config's sha, and creates and prints the run-artifact directory.
- `review run update <U> --set-json signals=…|waves=…|audit=…`: patches a whitelist of run fields.
- `review run finish <U>`: stamps `finished` and **computes `counts` from the store** — nobody counts by hand.
- `review plan --run <U> --ndjson <path|->`: appends cells (`C<n>`, status `assigned`). It validates `slice`, `concern`, `agent` (`review-finder`, `review-sweeper`, `plan-executor` or `orchestrator`) and `reason`. It resolves `concern` through the run's catalogue roots: a lens id must exist, be enabled and declare the run's family; `probe:<slug>` requires `question`; `check:<id>` must name an enabled orchestrator check. Each resolved cell stores `lens_version`, `path`, `methods` and `v1_category`, and its facets: auto-attached by signal, then adjusted by the row's `facets` edits (`+id` / `-id`). A later call appends the next wave.
- `review cover <C> --status covered|thin|uncovered --note -`: an agent's coverage report for its cell.
- `review add --run <U> --cell <C> --ndjson <path|->`: an agent's findings. Validates each row against the field catalogue (kind-specific required fields, enums, length caps; `failure_scenario` for every defect; `plan_section` for family `plan`), checks that a cleanup row's `old` text occurs in `file` within ±20 lines of `line`, computes `anchor`, and requires any `same_as` to name one of the row's anchor candidates. Each accepted row is minted as `status = "unverified"` (`advisory` as `open`); a failing row is `refused` with its reason while the rest land. Prints the minted ids.
- `review verdict <F> --verdict confirmed|plausible|refuted --by <who> --citation <c> --rationale -` (or `--ndjson` for a checker batch): appends the verdict and resolves the outcome, in order:
  - no `same_as`: CONFIRMED/PLAUSIBLE → `open`; REFUTED → `refuted` (the calibration negatives).
  - `same_as` naming an open or exported finding: CONFIRMED/PLAUSIBLE → **repeat** (run appended to the target's `seen_in` once; `line`, `evidence` refreshed) and this row → `merged`; REFUTED → **contested** (the target keeps its status and gains the verdict; for an exported target the report names the v1 id to defer).
  - `same_as` naming a refuted finding: CONFIRMED → **reopened** (target back to `open`, row `merged`); otherwise **suppressed**.
  - `same_as` naming a resolved finding: CONFIRMED → **regression** (row `open`, `related = [target]`).
  - `same_as` naming a dismissed, deferred, backlogged or converted finding: **suppressed** (row `merged`).
  A REFUTED verdict on a struck `same_as` claim (the verifier rejects the identity, not the defect) is `--strike-same-as`: the claim is cleared and the row is judged on its own.
- `review list` / `review show <ids> --for finder|verifier|checker|implementer|orchestrator|backlog|report`: role projections over the global output options. `list` takes `--kind`, `--lens`, `--run`, `--cell`, `--status` and `--in <glob>` (the finder's `PRIOR` query for its slice).
- `review digest --run <U>`: the orchestrator's view — counts by kind and lens, cells by status with notes for `thin`/`uncovered`, contested findings with both sides, refuted criticals for audit, and the advisory list in the orchestrator projection.
- `review report --run <U> [--out <path>]`: the deterministic markdown report (kind-grouped, severity-ordered defects, cleanup grouped by file, advisory with routing, coverage map, run line).
- `review triage <ids> --dismiss <r> | --defer <r> --trigger <t> | --reopen <r> | --resolve <r> | --backlog-ref <B-id> | --convert cleanup|defect --ndjson <path> --expect-status <s>`: dispositions with compare-and-set; stale rows land under `skipped_stale`. Each prose flag has a `--<flag>-file <path>` twin.
- `review stats [--slug|--scope|--all] [--family] [--by lens|probe|run] [--calibration [--exemplars N]]`: per lens and probe — runs, cells, yield by kind, refute rate, dismissal rate, backlogged/exported/resolved share, uncovered rate. `--calibration` adds the top rejection reasons from `dismiss_reason` and refuted rationales. This feeds compose, not a system comparison.
- `review export-v1 <ids> --to <v1-ledger> [--dry-run]`: Approach, "Export to v1".
- `review catalogue --builtin <dir> [--project <dir>] [--family <f>] [--signals <a,b>] [--vocabulary] [--check]`: Approach, "Lens catalogue".
- `review schema finding|cell-return|verdict-return [--kind <k>]`: JSON Schemas built from the field catalogue; `cell-return` is `{cell, finding_ids, status, escalate?}` and `verdict-return` is `{finding, verdict}`, the small objects a Workflow `agent()` call validates. A unit test asserts the schemas and `add`'s validation agree.

**Anchor matcher** (D7). Within `family`, a row's anchor candidates are the findings matching `(file, symbol)` when `symbol` is non-empty, otherwise `(file, line)` within ±10. Candidates never decide identity: only a `same_as` naming a candidate does, and the verifier or checker confirms or strikes it. A row with candidates but no `same_as` is minted with them under `related`. `lens` and `kind` are not in the key. `dedup_id` is not used.

### Export to v1

`export-v1` is the bridge to the untouched `/review-apply` and `/optimise-apply`. Family `code` goes to `review-ledger.toml` (prefix `R`), family `perf` to `optimise-findings.toml` (prefix `O`). The verb validates the v1 required-field contract itself, because tomlctl's CLI write path checks only `id` and apply skips malformed rows silently (D8).
- **Kinds**: `defect` exports with its severity (`minor` → `suggestion`); `cleanup` exports as `suggestion`; `advisory` is refused (it routes to the backlog or converts first).
- **Category**: the cell's `v1_category`, which `review plan` copied from the lens frontmatter; for a probe or an orchestrator check, it falls back to `quality` (code) or `algorithm` (perf). The optimise vocabulary is closed: `memory`, `serialization`, `query`, `algorithm`, `concurrency`.
- **Description**: built from the implementer projection — `fix` or `edit` (+ `old`), then `failure_scenario` — because apply reads the change from `description`.
- **Refused**: family `plan`, `kind = advisory`, `effort = "large"`, any status other than `open`. Each refusal is reported with its reason.
- **Matching**: look up `v2_ref` across v1 rows of every status first; then the v1 merge rule — an open v1 match reuses the v1 id and bumps `rounds` only when this `v2_ref` is new; a fixed or applied match gets a new id with `related`; a deferred, wontfix, wontapply or verified-clean match is skipped.
- **Written fields**: `v2_ref = "<store path>#F<n>"` on the v1 row; `exported_as` and `export_ledger` on the v2 finding (status `exported`).
- **`last_updated`**: the v1 ledger's root `last_updated` becomes the later of its current value and the newest run date among the findings' `seen_in` runs; a new ledger is seeded with that date.
- **Write order**: two sequential `mutate_doc` calls, never nested — the v1 write commits before the v2 store is opened, so a crash converges on re-export through `v2_ref`.
- `depends_on` F-ids map to exported v1 ids; unexported ones are reported under `dropped_deps`.

### Run pipeline

`claude/skills/review-v2-engine/` is the orchestrator contract every v2 carrier invokes. Stages, with the executor of each:

0. **Pre-flight** (orchestrator, deterministic): require `review_add` in `tomlctl capabilities` `.features`, else halt with the `cargo install --path tomlctl` hint. Invoke the `tomlctl` and `flow-contract-flow-context` skills and build the envelope with the **v1 command name** (`--command review|optimise|review-plan`; D5). Bind `--slug` or `--scope`.
1. **Run start** (tomlctl): `review run start`, with `models` read from the six v2 agents' `model:` frontmatter lines. Scope safety as v1: no `..`, no absolute paths outside the worktree, glob breadth ≤200.
2. **Survey** (Haiku `review-surveyor`, one dispatch). The surveyor reads the signal vocabulary (`tomlctl review catalogue --vocabulary`) and measures each signal's `detect` rule — diff signals, health signals and stack signals — recording the results with `review run update --set-json signals=…`. It runs the enabled checks whose executor is `surveyor` or `tomlctl` (such as `store-check`, `format-contract` and `dev-server-probe`). For `verification` checks (`lint-baseline`, `profile-baseline`, project checks), it resolves each command through the check's discovery ladder. It writes two files into the run directory:
   - `profile.md` (≤150 lines): files by role and subsystem; the diff, health and stack signals with their measurements (test count and growth against code, test-to-code ratio, docs volume against churn, churn hotspots, prior-finding and backlog density by area); and, for the plan family, the plan's task list with effort tags and the `STORE CHECK` result.
   - `pack.md`: the agent preamble fences. `CONSTRAINTS` holds only rules not already in CLAUDE.md, which every subagent already receives. The rest are `BACKLOG` rows for the scope, a fence per check result or pointer (`LINT BASELINE`, `PROFILE BASELINE`, `STORE CHECK`, `FORMAT CONTRACT`, `DEV SERVER`), and per-area `CALIBRATION` from `review stats --calibration`. An absent fence says why.

   Its return to the orchestrator is a ≤40-line digest: the signal list, hotspots, the resolved `verification` check commands, and anything it could not measure. The orchestrator then dispatches the `verification` agent once with those commands, writing into the run directory (CLAUDE.md build discipline); the pack's fences point at those outputs.
3. **Compose** (orchestrator — the core judgement step). The orchestrator reads:
   - the surveyor digest and `profile.md`;
   - `tomlctl review catalogue --family <f> --signals <run signals>`, the merged and signal-matched index (rows only, never lens bodies);
   - `review stats --by lens` for this repo.

   It then writes the coverage map with `review plan`: cells of *slice × concern*, each with its agent and reason. Rules (`references/compose.md`):
   - the family's **floor** lenses always appear;
   - slice by risk, not by file count: a hotspot or trust boundary gets its own cell, a quiet area shares one;
   - a `default = off` lens is composed only when the project config enables it or a matching signal makes the case;
   - health signals activate maintenance lenses (e.g. `test-pruning` on test growth);
   - facets attach automatically by stack signal; the orchestrator may add or remove one per cell (`+vue`, `-react`);
   - author a **probe** (`probe:<slug>` with a one-paragraph `question`) wherever the change has a risk no catalogue lens owns; the probe template in the method skill turns the question into a finder brief;
   - each enabled `orchestrator` check gets a cell with `agent = "orchestrator"`; the orchestrator runs it between waves and records its findings with `review add`;
   - respect the depth envelope below, and state the expected agent count.
4. **Wave** (Workflow `review-v2-pipeline`, or Agent-tool waves). Each cell's agent:
   - fetches its own assignment with `review show <C> --for finder`, which carries the resolved lens `path`, facet paths and method ids, so it reads the exact catalogue files the run bound;
   - reads `pack.md` and, through `review list --in <slice> --for finder`, the `PRIOR` findings for its slice;
   - writes rows with `review add` and its coverage with `review cover`;
   - returns only `{cell, finding_ids, status, escalate?}`.

   As soon as a cell returns, each of its defect ids goes to its own `review-verifier` (a 3-vote panel for criticals at `deep`), and its cleanup ids go to one `review-checker` batch; each writes `review verdict`. The Workflow returns only the index `{run, wave, cells, verified, errors, script_version}`. The orchestrator ends its turn after `async_launched` and resumes on the task notification.
5. **Digest** (tomlctl → orchestrator): `review digest --run <U>`. The orchestrator decides a follow-up wave — new cells for `uncovered`/`thin` cells, for an escalated sweeper cell (re-run on `review-finder`), or for a gap the findings revealed — within the depth envelope, then returns to stage 4.
6. **Audit and adjudication** (orchestrator): every REFUTED critical, every contested finding, and two CONFIRMED defects chosen by index. It reads the verifier projection plus the verdict and cited line, and records an overturn as `by = "audit"`.
7. **Triage** (orchestrator + user): the report (`review report`) is shown with the advisory list. The orchestrator recommends per advisory item — backlog (default) or apply-now with the conversion — and the user disposes. Grammar: `dismiss F3 — reason`, `defer F3 — reason — trigger`, `reopen F3 — reason`, `backlog F7,F9`, `convert F8`, `export F1,F4` / `export all`. User prose is staged with the Write tool and passed through the `--<flag>-file` twins.
8. **Close** (tomlctl, plus the orchestrator's backlog writes): `export-v1` for the chosen ids (followed by the `/review-apply` or `/optimise-apply` hand-off naming the v1 ids); advisory backlog rows via the unchanged `backlog-capture` skill — `review show --for backlog` emits the rows in `backlog check`/`add` NDJSON shape, so nothing is re-typed, and `review triage --backlog-ref` records each minted id; `review run finish`; the run id and report path.

Depth envelopes (`--depth`, default `standard`):

| | quick | standard | deep |
|---|---|---|---|
| cells ceiling (all waves) | 6 | 16 | 32 |
| follow-up waves | 0 | 1 | 2 |
| defect verification | individual for critical/warning; one batch verifier per cell for minor | individual for every defect | individual + 3-vote panel per critical (survives on ≥2 non-refuted) |
| cleanup check | one checker batch per cell | same | same |
| repeat passes | — | — | high-risk cells re-run once with permuted file order; repeats resolve through `same_as` |
| plan-executor cells | ≤4 tasks | ≤12 tasks (L and M first, checkpoint-marker tasks first) | every task |

**Substrate** (decision 2). `Workflow({name: "review-v2-pipeline", args})` with `args = {run, store: {slug|scope}, ledger_line, wave, depth, pack_path, cells: [{id, agentType, critical_panel}], schemas: {cell_return, verdict_return}}`. The script builds each prompt from a fixed template (the column-0 `ledger:` line when a flow is bound, the run and cell id, the pack path), uses `pipeline()` so verification starts as each cell returns, and `log()`s every cell it skips and every escalation. A saved workflow registers as `/<meta.name>`, so the name is `review-v2-pipeline`, not `review-v2` (D11). `claude/workflows/review-v2.js` is deployed by `scripts/deploy-claude.ps1`. Because agents persist their own output, the return is small and the Agent-wave fallback (`references/agent-waves.md`) has the same persistence contract: cells in waves of ≤16 per message, then verifiers and checkers in waves of ≤16, every dispatch one-shot with no `name:`. The fallback fires when Workflow is disabled or unavailable, the name is unknown, the launch reports `error`, or the run completes with no result. The run records `substrate`.

### Agents

All six use the frontmatter `skills:` preload (R13): `review-v2-method` for all, plus `research-methods` for the Opus finders and verifier. A preload injects `SKILL.md` only, so every procedure an agent needs is in `review-v2-method`'s `SKILL.md`; each body still says "if `review-v2-method` is not loaded, invoke it first". Catalogue files (lens, facets, methods) are read from the absolute paths in the cell assignment (`review show <C> --for finder`); worked examples are read from the base directory the Skill tool reports, never from a guessed path. Each carries the `review-v2-store-writer` block (below) and the existing `research-delivering` block byte-identically; both are registered in `scripts/shared-blocks.toml`. No v1 block's text changes.

| Agent | Model / effort | Tools | Writes | Job |
|---|---|---|---|---|
| `review-surveyor` | haiku / medium | Read, Grep, Glob, Bash, Write | `profile.md`, `pack.md` in the run dir; `review run update` signals | Measure the change and the project's health; never judge |
| `review-sweeper` | haiku / medium | Read, Grep, Glob, Bash, Skill | `review add`, `review cover` | Run mechanical lenses (sweeps, drift, consistency, maintenance candidates) on one cell; set `escalate` when a candidate needs judgement |
| `review-checker` | haiku / medium | Read, Grep, Glob, Bash, Skill | `review verdict` | Check a cell's cleanup rows by mechanical predicates: the issue is present (stale really stale, dead really unreferenced by grep), not already fixed, the edit is local, no documented intent at the anchor. Anything it cannot settle mechanically is PLAUSIBLE, never CONFIRMED |
| `review-finder` | opus / high | research-deep's list | `review add`, `review cover` | Run one judgement lens or probe on one cell; produces all three kinds |
| `review-verifier` | opus / high | research-deep's list minus Playwright | `review verdict` | Refute-first on one defect (or a minor batch): start from `counter`, re-read the anchor, reproduce the trigger, look for intent (comment, `git log -S`, ADR, test), confirm or strike `same_as`; cite `file:line` or command output for every verdict; never concede on an uncited assertion (R8) |
| `plan-executor` | opus / medium | Read, Grep, Glob, Bash, Skill | `review add`, `review cover` | Given only what an `implement-*` agent gets (plan preamble sections + one task, fetched by `tomlctl tasks show --with body,files,deps`), dry-run the task: invented decisions, files touched vs the **Files** line, acceptance commands not runnable as written, unconfirmed premises — each a plan-family finding |

The `review-v2-store-writer` block, copied byte-identically into all six agents (inline code only, no fenced commands, so it does not depend on the clap tree for `command_lint`):

```
<!-- SHARED-BLOCK:review-v2-store-writer START -->
## Store writes

You never edit, create or delete files in the working tree, except that the surveyor writes `profile.md` and `pack.md` into the run directory it is given. Your only other writes are `tomlctl review` calls against the store named in your assignment — `add`, `cover` and `verdict` for review agents, `run update` for the surveyor — always passing the same `--slug` or `--scope` your assignment names. Never run `tomlctl items`, `tomlctl backlog add`, `git add`, `git commit` or any formatter or build command. Write row payloads to stdin (`--ndjson -`) rather than to a file. If a write fails with `os error 5`, retry it once; on any other failure, or a refused row, read the reason, fix the row and resubmit it once, then report the refusal in your return instead of retrying again. Your return is only the small object your assignment asks for: everything else belongs in the store.
<!-- SHARED-BLOCK:review-v2-store-writer END -->
```

### Lens catalogue

The catalogue is the maintained, pluggable source of every concern a run can compose. It lives once, globally: it is shared by every project, and a project only opts entries in or out.

**Entry types.** Each entry is one markdown file whose frontmatter is TOML between `+++` lines. TOML is used because tomlctl already parses it and this plan adds no dependency. There are four types:

| Type | Directory | What it holds | Frontmatter beyond `id`, `version`, `status` |
|---|---|---|---|
| **lens** | `lenses/` | A concern: the question it answers, the concern tags it owns, its probes per family, what it does not flag, and its edit forms and severity calibration | `families`, `mode` (judgement \| mechanical \| executor), `produces`, `floor` (families it is always composed in), `default` (on \| off), `methods`, `apply_when` (`signals` + `note`), `slice` (whole-scope \| per-subsystem \| per-file \| per-task), `v1_category` (per family), `research_domain` |
| **facet** | `facets/` | Stack knowledge: per-lens probes and pitfalls for one language, framework or runtime. It replaces v1's inline frontend/backend variants and most of `/optimise`'s Focal Points Brief | `families`, `apply_when` (stack signals), `attaches_to` (lens ids or `"*"`) |
| **method** | `methods/` | A procedure lenses cite by id instead of restating it | — |
| **check** | `checks/` | A command-backed or orchestrator-run check feeding a pack fence or findings | `families`, `executor` (tomlctl \| surveyor \| verification \| orchestrator), `fence` or `produces`, `apply_when`, `default`, `discover` (the command-discovery ladder for `verification` checks) |

Body sections are fixed per type:
- **Lens**: Question; Owns (one paragraph per concern tag); Probes — `<family>` (one section per declared family); Does not flag; Edit forms; Advisory angles; Severity calibration.
- **Facet**: a general section, then `### For <lens-id>` per attached lens.
- **Check**: Runs; Interpreting the result; Fence or finding mapping.

A lens's Does not flag section is seeded from the v1 rejection clusters (confirmations, stale-comment trust, design intent and ADRs, speculative and style-only, out of scope, negligible gain against risk).

**Vocabulary.** `vocabulary.toml` holds two closed sets:
- **Signals**: `[signals.<name>]` with `kind` (diff \| health \| stack), `meaning`, and `detect` (a glob, grep, manifest key or command the surveyor runs).
  - Stack signals are detected from manifests, so facets attach with no setup: `lang:rust`, `lang:ts`, `lang:csharp`, `framework:react`, `framework:vue`, `runtime:tokio`, `db:sqlx`, `db:efcore`, `harness:claude`.
  - Diff signals include `rename-or-removal`, `public-api-change`, `new-seam`, `trust-boundary`, `dependency-change`, `schema-change`, `query-layer`, `async-concurrency`, `process-heavy`, `hot-path`, `frontend`, `docs-touched`, `tests-touched`.
  - Health signals: `test-growth`, `test-ratio-high`, `doc-growth`, `churn-hotspot`, `finding-hotspot`, `backlog-cluster`.
- **Concern tags**: `[concerns.<tag>]` with `meaning`, `families` and `owner` (one lens id, or a per-family table). The vocabulary is the single ownership map: a lens's Owns section must cover exactly the tags the vocabulary assigns it, so one owner per concern per family holds by construction. This replaces v1's "defer to Agent N" cross-references with data the gate checks, and lets lens files be written in parallel without ownership races.

**Layers.**
1. **Built-in (global)**: `claude/skills/review-v2-lenses/{lenses,facets,methods,checks}/` plus `vocabulary.toml`, deployed to `~/.claude/skills/review-v2-lenses/` by `scripts/deploy-claude.ps1`. Every project gets the full catalogue with zero setup. It is the user's own repo deployed globally, so no separate user-global layer exists.
2. **Project (optional, minimal)**: `.claude/review-lenses/config.toml`:
   ```toml
   disable = ["doc-consolidation"]   # built-in entries switched off here
   enable = ["plan-premortem"]       # default-off entries switched on here
   [extend.idioms]                   # append-only patches; scalars limited to apply_when and slice
   does_not_flag = ["…"]
   probes = ["…"]
   [[checks]]                        # project checks (executor = verification)
   id = "harness-gates"
   families = ["code"]
   paths = ["claude/**"]
   command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
   ```
   An absent file means the defaults. As an escape hatch, `.claude/review-lenses/{lenses,facets}/*.md` may add genuinely project-specific entries. An id that collides with a built-in is **refused** — use `extend` or `disable` — so a project never silently forks a shared lens, and improvements keep flowing back to the shared catalogue.

**Generated index and validation.** `tomlctl review catalogue --builtin <dir> [--project <dir>] [--family <f>] [--signals <a,b>] [--vocabulary] [--check]`:
- merges the layers and applies the config;
- prints one row per entry: `type`, `id`, `version`, `status`, `families`, `mode`, `produces`, `floor`, `default`, `enabled`, `apply_when`, `matched` (when `--signals` is given), `attaches_to`, `path`;
- `--vocabulary` prints the signals with their `detect` rules;
- `--check` exits 1 on any validation error.

Validation covers:
- unique ids, and the frontmatter schema per type;
- signals and concern tags drawn from the vocabulary;
- every concern tag's owner exists and declares the tag's families, and each lens's Owns section names exactly its assigned tags;
- `methods` and `attaches_to` targets that exist;
- a Probes section for every declared family;
- config ids that exist, with no project id colliding with a built-in;
- `v1_category` valid per family (the optimise set is closed: `memory`, `serialization`, `query`, `algorithm`, `concurrency`).

The dispatch-test gate `catalogue_validates` runs the same validator over the built-in catalogue plus this repo's project config, so the pre-commit hook blocks a broken entry. The engine passes `--builtin` as the base directory the Skill tool reports for `review-v2-lenses`.

**Runs bind versions.** `review plan` resolves each cell through the catalogue at plan time. It stores `lens_version`, `path`, `methods`, `v1_category` and the facets — auto-attached by signal, then adjusted by the orchestrator's `+`/`-` edits. `review run start` records the catalogue roots and the config's sha. So `review stats --by lens` groups by (id, version), export reads the category from the cell, and an agent reads exactly the files its run bound.

**Lifecycle.**
- `status` moves `experimental` (default off) → `stable` → `deprecated`. A deprecated entry is refused in new cells but still resolves for stats.
- `version` bumps on any change to Question, Owns, Probes or Does not flag.
- Promotion: a probe slug recurring in `review stats --by probe` across three or more runs becomes a lens — first as a project entry, then built-in once it proves portable. `--calibration` exemplars feed Does not flag edits.
- The catalogue `SKILL.md` documents authoring, the layer rules, the config file and the probe template.

**Initial catalogue.** v1 sources: `/review` (`claude/commands/review.md` Agents 1–6), `/optimise` (`claude/commands/optimise.md` Agents 1–5 and the cross-cutting concurrency review), `/review-plan` (`claude/commands/review-plan.md` Agents 1–4 and the Step 2.6 checks), the `research-methods` lens notes (`claude/skills/research-methods/references/codebase-review.md`), and lumina's `research-explore` and `story-review`.

| Lens | Families | Mode | Floor | Default | From |
|---|---|---|---|---|---|
| `correctness` | code | judgement | code | on | new; the method notes' boundary list + `/review` A4 edge cases and error paths |
| `type-design` | code | judgement | — | on | `/review` A1 (illegal states, untyped domain primitives, stringly-typed sets) |
| `domain-model` | code | judgement | — | on | `/review` A3 `db` (persistence leaks, role-mixed types, cardinality, migrations) |
| `architecture` | code | judgement | — | on | `/review` A3 (layering, placement, evolvability) + method notes |
| `idioms` | code | judgement | — | on | `/review` A1 (converged pattern, decided from history) |
| `duplication` | code | judgement | — | on | `/review` A1 DRY + method notes (facts with one owner, rule of three) |
| `security` | code, plan | judgement | — | on | `/review` A2 (trust boundaries, cap 5) |
| `external-validity` | code, plan | judgement | — | on | `/review` A2 advisories + `/review-plan` A4 (pinned APIs, deprecations, advisories for added or bumped dependencies) |
| `ripple` | code, plan | mechanical | code, plan | on | `/review` A4 ripple + `/review-plan` A2 sweeps 1–3 |
| `hygiene` | code | mechanical | — | on | new: dead code, comment rot, stale TODOs, naming drift |
| `test-gaps` | code, plan | judgement | — | on | `/review` A5 tests + `/review-plan` A2 missing tests |
| `seams` | code | judgement | — | on | `/review` A5 seams |
| `diagnostics-dx` | code | mechanical | — | on | `/review` A5 error messages, logs, help text |
| `test-pruning` | code | judgement | — | on | new; health-activated, `direction = remove` |
| `doc-drift` | code | mechanical | — | on | new: prose against behaviour |
| `doc-consolidation` | code | judgement | — | on | new; health-activated merges and removals |
| `memory-runtime` | perf | judgement | — | on | `/optimise` A1 |
| `data-shape` | perf | judgement | — | on | `/optimise` A2 |
| `data-access` | perf | judgement | — | on | `/optimise` A3 |
| `algorithm` | perf | judgement | perf | on | `/optimise` A4 |
| `concurrency` | perf | judgement | — | on | `/optimise` A5, with its severity calibration |
| `io-process` | perf | judgement | — | on | new: process start-up, syscalls, filesystem walks, subprocess fan-out |
| `plan-grounding` | plan | judgement | plan | on | `/review-plan` A2 alignment + sweeps 4–5 |
| `plan-dependencies` | plan | judgement | plan | on | `/review-plan` A1 edges, checkpoint placement, given `STORE CHECK` |
| `feasibility` | plan | judgement | — | on | `/review-plan` A1 feasibility and coherence |
| `plan-executor` | plan | executor | plan | on | new dry run per task (R9) + `/review-plan` A3 executability |
| `plan-risk` | plan | judgement | — | on | `/review-plan` A4 rollback, risky-task checkpoint placement, back-compat, scope realism |
| `plan-premortem` | plan | judgement | — | off (experimental) | new (R9, weak evidence) |
| `contrarian` | code, perf, plan | judgement | code, perf, plan | on | lumina `contrarian` + `story-review` devil's advocate (argue-against, scope conservatism); advisory only |
| `synthesis` | code, perf, plan | judgement | — | on (standard and up, after wave 1) | new + `/optimise` cross-cutting concurrency |

- **Facets** (9), covering the stacks across the user's repos (Rust here, TypeScript/React in reportdesignkit, Vue in lumina's SPA, .NET in Intembeko, compliance-engine, mercury and warehouse): `rust`, `tokio`, `sqlx`, `typescript`, `react`, `vue`, `dotnet`, `efcore`, `claude-harness` (the portable judgement half of v1's Package Quality lens: frontmatter, section order, cross-references, stubs, terminology).
- **Methods** (6): `sweep` (the method notes' sweep procedure, `tomlctl sweep`, one line per sweep), `trace` (contract first; trace before claiming), `history-idiom`, `measure` (the perf evidence ladder), `dry-run`, `steelman`.
- **Checks** (9 built-in):
  - `lint-baseline` and `profile-baseline` (executor `verification`);
  - `store-check`, `format-contract` and `dev-server-probe` (executor `surveyor`);
  - `break-set-baseline`, `two-control-probe`, `falsifiability` and `acceptance-contradiction` (executor `orchestrator`; v1 `/review-plan` Step 2.6).
- **This repo's project config**: one project check, `harness-gates`, which runs the `cli::dispatch::tests` gates instead of having an agent re-derive them (the mechanical half of v1's Package Quality lens).

**lumina overlap.** lumina's six `research-explore` lenses map onto catalogue entries: `codebase` → the survey plus `ripple` and `plan-grounding`, `library` → `external-validity`, `risk` → `plan-risk`, `completeness` → `ripple`, `domain` → `domain-model`, `contrarian` → `contrarian`. The catalogue `SKILL.md` records the mapping. The overlapping lenses keep their Question and Owns sections free of review-only assumptions, so lumina could cite them later. lumina itself is not edited.

### Family specifics

- **`/review-v2`** (family `code`): `LINT BASELINE`; context-free correctness is a composable cell (the `correctness` lens with an empty pack, diff only; R7), composed at standard and up; export to `review-ledger.toml`.
- **`/optimise-v2`** (family `perf`): a Focal Points Brief written fresh; `PROFILE BASELINE` from the project's declared bench or profile commands (CLAUDE.md `## Optimization Focus`, a `bench:` key under `## Verification Commands`, or `benches/`) via the `verification` agent — with none, a perf defect needs a complexity argument tied to a real input size or is `advisory` with `angle = risk`; cleanup covers small, safe wins; export to `optimise-findings.toml`.
- **`/review-plan-v2`** (family `plan`): plan resolution as v1 Step 1 (with the `flow-contract-plansdirectory-prompt` skill); the surveyor runs task-store import and `tomlctl tasks check --plan` into `STORE CHECK` via the `flow-contract-task-store` skill, and `FORMAT CONTRACT` comes from the `flow-contract-plan-output-format` skill. v1 Step 2.6's four orchestrator-only checks (break-set baselines, cross-task contradiction, falsifiability, two-control probes) stay with the orchestrator and record through `review add` with `concern = orchestrator-checks`. Merge offer: confirmed or plausible defects and cleanup are eligible; advisory is backlog or discussion. Manual and Mechanical (`anchor_old`/`anchor_new`) modes keep v1's safety rules — the empty-answer rule, the `.premerge.md` backup, and the merge-exit ref-set gate → import → check. Merged findings resolve with `resolution = "merged into <plan>"`.

### Coexistence with v1

- Every v1 file under Scope → Out of scope stays byte-identical, guarded by a `git diff --quiet` success criterion.
- v2 never writes a v1 ledger except through `export-v1`; it skips the user-inputs sweep; it shares the backlog store through the unchanged `backlog-capture` contract, with the orchestrator as the only backlog writer.

## Success Criteria

- guard: v1 review harness files, and the v1 skills and agents v2 reuses or leaves out of scope, are byte-unchanged against the plan's base commit: `git diff --quiet 27b7e1a -- claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md claude/commands/review-apply.md claude/commands/optimise-apply.md claude/agents/research-deep.md claude/agents/research-lite.md claude/agents/implement-deep.md claude/agents/implement-lite.md claude/agents/flow-bootstrap.md claude/agents/verification.md claude/skills/research-methods claude/skills/flow-contract-vet-research claude/skills/flow-contract-ledger-schema claude/skills/flow-contract-apply-pipeline claude/skills/flow-contract-apply-constraints claude/skills/flow-contract-apply-dependency-sort claude/skills/flow-contract-apply-rollback-protocol claude/skills/flow-contract-apply-vet-implement-lite claude/skills/flow-contract-flow-context claude/skills/backlog-capture claude/skills/flow-contract-task-store claude/skills/flow-contract-plan-output-format` exits 0 (today: exits 0).
- forward: all three v2 carriers exist and invoke the engine skill: `test "$(grep -l '`review-v2-engine` skill' claude/commands/review-v2.md claude/commands/optimise-v2.md claude/commands/review-plan-v2.md 2>/dev/null | wc -l)" -eq 3` exits 0 (today: exits 1).
- forward: the initial catalogue is complete — 30 lenses, 9 facets, 6 methods, 9 checks and the vocabulary: `c=claude/skills/review-v2-lenses; test "$(ls $c/lenses/*.md 2>/dev/null | wc -l)" -eq 30 && test "$(ls $c/facets/*.md 2>/dev/null | wc -l)" -eq 9 && test "$(ls $c/methods/*.md 2>/dev/null | wc -l)" -eq 6 && test "$(ls $c/checks/*.md 2>/dev/null | wc -l)" -eq 9 && test -f $c/vocabulary.toml` exits 0 (today: exits 1).
- forward: the catalogue validates through the same code `tomlctl review catalogue --check` runs — **predicted, unverified** (builds `target/`): `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::catalogue`.
- forward: the three Haiku agents carry `model: haiku` and the three Opus agents `model: opus`: `test "$(grep -l '^model: haiku' claude/agents/review-surveyor.md claude/agents/review-sweeper.md claude/agents/review-checker.md 2>/dev/null | wc -l)" -eq 3 && test "$(grep -l '^model: opus' claude/agents/review-finder.md claude/agents/review-verifier.md claude/agents/plan-executor.md 2>/dev/null | wc -l)" -eq 3` exits 0 (today: exits 1).
- forward: the v2 agents are shared-block carriers and the gate passes: `test "$(grep -c 'review-v2-store-writer' scripts/shared-blocks.toml)" -ge 1 && bash scripts/verify-shared-blocks.sh >/dev/null` exits 0 (today: exits 1).
- forward: the v2 carriers are in the skill-invocation gate table: `test "$(grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs)" -eq 3` exits 0 (today: exits 1).
- forward: the `tomlctl review` suites pass, including a defect row without `failure_scenario` being refused and a project id colliding with a built-in being refused — **predicted, unverified**: `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -E 'binary(/^review_/)'`.
- forward: the field-catalogue, store, catalogue and matcher unit tests pass, including the verifier projection withholding `grade` and `fix` — **predicted, unverified**: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::`.
- forward: in a repo with no `.claude/review-lenses/`, `tomlctl review catalogue --builtin ~/.claude/skills/review-v2-lenses --check` exits 0 and lists every built-in entry as enabled except `plan-premortem` — **predicted, unverified** (installed binary and deployed catalogue).
- forward: an end-to-end `/review-v2 --depth quick` on `tomlctl/src/sweep.rs` records one run whose cells bind lens versions and the `rust` facet, findings written by the agents (each with a verdict unless advisory), every cell `covered`, `thin` or `uncovered` with a note, and computed `counts` — **predicted, unverified** (live session; see After Merge).
- forward: `/optimise-v2 --depth quick` on `tomlctl/src/sweep.rs` and `/review-plan-v2 --depth quick` on this plan each record one run — **predicted, unverified** (live session).
- forward: `tomlctl review export-v1 <F-id> --scope <scope> --to .claude/reviews/<scope>.toml` writes a v1 row that `tomlctl items list .claude/reviews/<scope>.toml --where-has v2_ref` lists with every v1 required field, and `/review-apply` selects it — **predicted, unverified** (live session).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
transient: rust-lld: failed to write output.*[Pp]ermission denied
success: git diff --quiet 27b7e1a -- claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md claude/commands/review-apply.md claude/commands/optimise-apply.md claude/agents/research-deep.md claude/agents/research-lite.md claude/agents/implement-deep.md claude/agents/implement-lite.md claude/agents/flow-bootstrap.md claude/agents/verification.md claude/skills/research-methods claude/skills/flow-contract-vet-research claude/skills/flow-contract-ledger-schema claude/skills/flow-contract-apply-pipeline claude/skills/flow-contract-apply-constraints claude/skills/flow-contract-apply-dependency-sort claude/skills/flow-contract-apply-rollback-protocol claude/skills/flow-contract-apply-vet-implement-lite claude/skills/flow-contract-flow-context claude/skills/backlog-capture claude/skills/flow-contract-task-store claude/skills/flow-contract-plan-output-format
success: test "$(grep -l '`review-v2-engine` skill' claude/commands/review-v2.md claude/commands/optimise-v2.md claude/commands/review-plan-v2.md 2>/dev/null | wc -l)" -eq 3
success: c=claude/skills/review-v2-lenses; test "$(ls $c/lenses/*.md 2>/dev/null | wc -l)" -eq 30 && test "$(ls $c/facets/*.md 2>/dev/null | wc -l)" -eq 9 && test "$(ls $c/methods/*.md 2>/dev/null | wc -l)" -eq 6 && test "$(ls $c/checks/*.md 2>/dev/null | wc -l)" -eq 9 && test -f $c/vocabulary.toml
success: cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::catalogue
success: test "$(grep -l '^model: haiku' claude/agents/review-surveyor.md claude/agents/review-sweeper.md claude/agents/review-checker.md 2>/dev/null | wc -l)" -eq 3 && test "$(grep -l '^model: opus' claude/agents/review-finder.md claude/agents/review-verifier.md claude/agents/plan-executor.md 2>/dev/null | wc -l)" -eq 3
success: test "$(grep -c 'review-v2-store-writer' scripts/shared-blocks.toml)" -ge 1 && bash scripts/verify-shared-blocks.sh >/dev/null
success: test "$(grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs)" -eq 3
success: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -E 'binary(/^review_/)'
success: cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::
success: cargo clippy --manifest-path glimpse/Cargo.toml --locked
```

The pre-commit hook adds the crate-wide `cargo fmt --check`, the `cli::dispatch::tests` gate subset (which includes the catalogue gate once task 40 lands, for any staged `claude/**/*.md` or `tomlctl/src/**`), and glimpse clippy `--locked` on any tomlctl `src` change. Run `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null` after the version bump to refresh `glimpse/Cargo.lock`.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 15, 18, 19, 40, 41, 42, 43, 46, 47, 50
- **Max parallel agents**: 8
- **Commit granularity**: per-task

## Tasks

### 1. Add the review field catalogue [M]
- **Files**: `tomlctl/src/review/fields.rs` (new), `tomlctl/src/review/mod.rs` (new), `tomlctl/src/lib.rs`
- **Depends on**: —
- **Action**: Create `tomlctl/src/review/fields.rs` holding the field catalogue as `const` data — every finding, cell, run and verdict field with its name, kind applicability, required-ness per kind and family, enum values, length cap, and per-role visibility — plus `validate_row(kind, family, &TomlValue) -> Result<(), Vec<Refusal>>` and `project(role, &TomlValue) -> TomlValue`. Create `tomlctl/src/review/mod.rs` declaring `mod fields;` and register `mod review;` in `tomlctl/src/lib.rs`.
- **Detail**: Follow Approach, "Finding kinds" and Approach, "Field readership" — the matrix there is the projection table. Roles: `finder`, `verifier`, `checker`, `implementer`, `orchestrator`, `backlog`, `report`. Caps: `summary` 160, `failure_scenario` 600, `fix` 800, `edit` 800, `old` 400, `counter` 300, `rationale` 800, `evidence` 5 × 300, `instances` 50, verdict `rationale` 600. Add no dependency. In-module tests: `defect_requires_failure_scenario`, `cleanup_requires_edit_and_old`, `advisory_challenge_requires_challenges`, `over_cap_field_is_refused_with_its_cap`, `verifier_projection_withholds_grade_and_fix`, `projection_table_matches_matrix` (asserts every role's field set against a literal transcription of the matrix).
- **Acceptance**:
  - forward: `grep -c '^mod review;' tomlctl/src/lib.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::fields` passes the six named tests — **predicted, unverified**.
  - falsifier: adding `grade` to the verifier projection turns `verifier_projection_withholds_grade_and_fix` red — **predicted, unverified**.

### 2. Add the review store and anchor matcher [M]
- **Files**: `tomlctl/src/review/store.rs` (new), `tomlctl/src/review/matching.rs` (new), `tomlctl/src/review/mod.rs`
- **Depends on**: 1
- **Action**: Add the typed `Store` (`runs`, `cells`, `findings`, `verdicts`, root `id_high_water`), the slug/scope path resolver and run-artifact directory resolver, the `mutate`/`read` helpers, and the anchor-candidate matcher; declare both modules in `tomlctl/src/review/mod.rs`.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group". Model `store.rs` on `tomlctl/src/tasks/store.rs` (`mutate` delegates to `io::mutate_doc`) but hand the closure the raw `TomlValue` as well as the typed view; seed with `schema_version = 1` plus `last_updated` via `OnMissing::Create`, as `tomlctl/src/inputs.rs` builds its seed. Mint through `items::items_append_minted` with prefixes `U`, `C`, `F`. `matching.rs` exposes `anchor(&TomlValue) -> String` and `candidates(&[Finding], &TomlValue) -> Vec<usize>` — within the family, `(file, symbol)` when symbol is non-empty, else `(file, line)` within ±10; it never decides identity. Do not reuse `tomlctl/src/anchor.rs` or `tomlctl/src/dedup.rs`. Tests: `round_trips_all_arrays`, `round_trips_id_high_water`, `candidates_prefer_symbol_over_line_window`, `candidates_within_ten_lines`, `candidates_partition_by_family`, `run_dir_uses_slug_or_scope_key`.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::matching` passes — **predicted, unverified**.
  - falsifier: widening the line window in `candidates` to unbounded turns `candidates_within_ten_lines` red — **predicted, unverified**.

### 3. Add the lens catalogue loader and validator [M]
- **Files**: `tomlctl/src/review/catalogue.rs` (new), `tomlctl/src/review/mod.rs`
- **Depends on**: 2
- **Action**: Implement the catalogue core: load `lenses/`, `facets/`, `methods/` and `checks/` markdown files from a built-in root and an optional project root (TOML frontmatter between `+++` lines, plus the body's `##` section headings), load `vocabulary.toml`, apply the project `config.toml` (`disable`, `enable`, `extend`, `[[checks]]`), and expose `validate() -> Vec<CatalogueError>`, `index(filter) -> Vec<Row>` and `resolve_cell(concern, family, signals, facet_edits) -> Resolved`. Declare the module in `tomlctl/src/review/mod.rs`.
- **Detail**: Follow Approach, "Lens catalogue" — every validation rule listed there is one `CatalogueError` variant, and the frontmatter schema per type is a `const` table. Parse frontmatter with the crate's existing TOML parser; add no dependency. `resolve_cell` returns `lens_version`, `path`, `methods`, `v1_category` and the facet list (auto-attached where the facet's stack signals are all present and `attaches_to` names the lens or `"*"`, then `+id`/`-id` edits applied). Tests (temp-dir catalogues): `parses_toml_frontmatter_and_sections`, `project_id_colliding_with_builtin_is_refused`, `concern_owner_must_exist_and_cover_its_families`, `owns_section_must_match_vocabulary_assignment`, `unknown_signal_is_refused`, `missing_family_probes_section_is_refused`, `config_disable_and_enable_apply`, `extend_appends_does_not_flag`, `facets_attach_by_signal_and_lens`, `deprecated_lens_is_refused_for_new_cells`.
- **Acceptance**:
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::catalogue` passes the ten named tests — **predicted, unverified**.
  - falsifier: dropping the built-in collision check turns `project_id_colliding_with_builtin_is_refused` red — **predicted, unverified**.

### 4. Wire the review verb group into the CLI with stub verbs [L]
- **Files**: `tomlctl/src/cli/types/review.rs` (new), `tomlctl/src/cli/types/mod.rs`, `tomlctl/src/cli/types/cmd.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/review/mod.rs`, `tomlctl/src/review/dispatch.rs` (new), `tomlctl/src/review/runs.rs` (new), `tomlctl/src/review/cells.rs` (new), `tomlctl/src/review/add.rs` (new), `tomlctl/src/review/verdict.rs` (new), `tomlctl/src/review/query.rs` (new), `tomlctl/src/review/digest.rs` (new), `tomlctl/src/review/triage.rs` (new), `tomlctl/src/review/stats.rs` (new), `tomlctl/src/review/export.rs` (new), `tomlctl/src/review/catalogue_cmd.rs` (new), `tomlctl/src/review/schema_out.rs` (new), `tomlctl/src/capabilities.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 3
- **Action**: Declare the full `ReviewOp` clap tree with every verb and flag Approach, "v2 store and the tomlctl review group" lists, add `Cmd::Review`, route it through `tomlctl/src/review/dispatch.rs` to one stub function per verb module, declare the eleven verb modules in `tomlctl/src/review/mod.rs`, and register capabilities features.
- **Detail**:
  - Copy the `inputs` and `backlog` idiom: `ReviewOp` re-exported from `tomlctl/src/cli/types/mod.rs`; `ReadIntegrityArgs` flattened on reads; `WriteIntegrityArgs` + `StampArgs` on writes.
  - Every verb except `schema` and `catalogue` takes `--slug` or `--scope` (mutually exclusive, one required); `stats` adds `--all`. Declare the `triage` `--<flag>-file` twins. Declare no flag that shadows a global output flag.
  - Each stub returns a `kind=validation` error `"review <verb>: not implemented"`; tasks 5–14 replace the bodies. The stubs exist so those tasks stay file-disjoint, which is why this task is L.
  - Features: `review_store`, `review_cells`, `review_add`, `review_verdict`, `review_digest`, `review_triage`, `review_stats`, `review_export_v1`, `review_catalogue`, `review_schema` — in `FEATURES`, the expected list in `tomlctl/tests/capabilities.rs`, `SUBCOMMANDS`, and both README transcriptions (the `capabilities` sample `"features"` array and the "Feature meanings" table), which `readme_feature_transcriptions_match_capabilities_features` gates.
  - Emit output only through `crate::output` helpers (`tomlctl/src/cli/dispatch/tests/output_gate.rs`).
- **Acceptance**:
  - forward: `grep -c 'Review' tomlctl/src/cli/types/cmd.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes — **predicted, unverified**.
  - falsifier: deleting `review_catalogue` from the README "Feature meanings" table turns `readme_feature_transcriptions_match_capabilities_features` red — **predicted, unverified**.

### 5. Implement review run start/update/finish and schema emission [M]
- **Files**: `tomlctl/src/review/runs.rs`, `tomlctl/src/review/schema_out.rs`, `tomlctl/tests/review_runs.rs` (new)
- **Depends on**: 4
- **Action**: Implement `review run start` (mint `U<n>`, stamp `started`, record the catalogue roots and the project config's sha, create and print the run directory), `review run update` (whitelisted fields), `review run finish` (stamp `finished`, compute `counts` from the store), and `review schema finding|cell-return|verdict-return [--kind]`.
- **Detail**: Schemas are built by hand from the `tomlctl/src/review/fields.rs` catalogue, with `if`/`then` for kind-conditional required fields; a unit test `schema_agrees_with_field_catalogue` asserts the emitted required sets equal the catalogue's. `counts` = cells and uncovered cells, findings by kind minted this run, refuted and suppressed outcomes. Integration tests on the `tomlctl/tests/common/` sandbox: `run_start_mints_sequential_ids_and_creates_run_dir`, `run_start_records_catalogue_roots`, `run_ids_never_reuse_a_removed_id` (seed `[id_high_water] U = n`), `run_update_refuses_non_whitelisted_field`, `run_finish_computes_counts`, `schema_defect_requires_failure_scenario`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_runs.rs && grep -c '#\[test\]' tomlctl/tests/review_runs.rs` prints at least `6` (today: prints nothing, exit 1).
  - falsifier: counting only `open` findings in `run finish` turns `run_finish_computes_counts` red — **predicted, unverified**.

### 6. Implement review plan and review cover with catalogue resolution [M]
- **Files**: `tomlctl/src/review/cells.rs`, `tomlctl/tests/review_cells.rs` (new)
- **Depends on**: 5
- **Action**: Implement `review plan --run <U> --ndjson` (validate, resolve each cell through `tomlctl/src/review/catalogue.rs` `resolve_cell` against the run's catalogue roots, append with `wave` = previous max + 1 per call and status `assigned`) and `review cover <C> --status --note -`.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group" (**Verbs** `review plan`) and Approach, "Lens catalogue" (**Runs bind versions**). Tests build a temp-dir catalogue: `plan_resolves_lens_and_binds_version`, `plan_refuses_disabled_or_wrong_family_lens`, `probe_cell_requires_question`, `facet_edits_adjust_auto_attachment`, `check_cell_requires_enabled_orchestrator_check`, `cover_sets_status_and_note`, `cover_refuses_over_cap_note`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_cells.rs && grep -c 'fn plan_resolves_lens_and_binds_version' tomlctl/tests/review_cells.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: skipping the family check in resolution turns `plan_refuses_disabled_or_wrong_family_lens` red — **predicted, unverified**.

### 7. Implement review add [M]
- **Files**: `tomlctl/src/review/add.rs`, `tomlctl/tests/review_add.rs` (new)
- **Depends on**: 6
- **Action**: Implement `review add --run <U> --cell <C> --ndjson <path|->`: validate each row through the field catalogue, check a cleanup row's `old` text occurs in `file` within ±20 lines of `line`, compute `anchor` and candidates, require `same_as` to name a candidate, and mint accepted rows (`unverified`; advisory `open`) in one `mutate`, reporting `refused` rows with reasons and printing minted ids.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group" (**Verbs** `review add`, **Anchor matcher**) and Approach, "Finding kinds". `lens` is taken from the cell's `concern`. Rows with candidates but no `same_as` are minted with the candidates under `related`; rows in one call are never collapsed. Tests: `defect_without_failure_scenario_is_refused_rest_lands`, `cleanup_old_text_must_exist_near_line`, `distinct_defects_on_one_symbol_both_mint`, `same_as_must_name_a_candidate`, `advisory_lands_open`, `over_cap_summary_is_refused`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_add.rs && grep -c 'fn distinct_defects_on_one_symbol_both_mint' tomlctl/tests/review_add.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: skipping the `old` text check turns `cleanup_old_text_must_exist_near_line` red — **predicted, unverified**.

### 8. Implement review verdict with identity outcomes [M]
- **Files**: `tomlctl/src/review/verdict.rs`, `tomlctl/tests/review_verdict.rs` (new)
- **Depends on**: 7
- **Action**: Implement `review verdict <F> --verdict --by --citation --rationale -` and its `--ndjson` batch form, plus `--strike-same-as`: append the verdict and resolve the outcome (open, refuted, repeat, contested, reopened, regression, suppressed) in one `mutate`.
- **Detail**: Outcome rules and order are Approach, "v2 store and the tomlctl review group", **Verbs** `review verdict`. A verdict without a citation is refused. Tests: `confirmed_unverified_becomes_open`, `refuted_unverified_becomes_refuted`, `same_as_repeat_merges_and_appends_seen_in_once`, `refuted_same_as_on_open_is_contested`, `confirmed_same_as_on_refuted_reopens`, `confirmed_same_as_on_resolved_is_regression`, `same_as_on_dismissed_is_suppressed`, `strike_same_as_judges_row_alone`, `verdict_without_citation_is_refused`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_verdict.rs && grep -c 'fn refuted_same_as_on_open_is_contested' tomlctl/tests/review_verdict.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: changing the target's status on a contested verdict turns `refuted_same_as_on_open_is_contested` red — **predicted, unverified**.

### 9. Implement review list and role-projected show [M]
- **Files**: `tomlctl/src/review/query.rs`, `tomlctl/tests/review_query.rs` (new)
- **Depends on**: 8
- **Action**: Implement `review list` (`--kind`, `--lens`, `--run`, `--cell`, `--status`, `--in <glob>`) and `review show <ids> --for <role>` for findings and cells, as row reports over the global output options.
- **Detail**: Project only through `tomlctl/src/review/fields.rs` `project`. A cell shown `--for finder` carries its slice, concern, question, reason, resolved lens `path`, facet ids with their paths, method ids with their paths, and the run's pack path. A finding's `same_as` target summary and status are joined for verifier and checker. The `backlog` projection emits the `backlog check`/`add` NDJSON object shape (`summary`, `kind` mapped from `angle` — `challenge`/`alternative`/`direction` → `direction`, `risk`/`probe` → `question` — `area` = the finding's directory, `context`, `evidence`, `origin = "review"`, `flow`). Tests: `list_in_glob_filters_prior`, `show_for_verifier_omits_grade_and_fix`, `show_for_backlog_emits_backlog_shape`, `show_cell_carries_resolved_catalogue_paths`.
- **Acceptance**: forward: `test -f tomlctl/tests/review_query.rs && grep -c 'fn show_cell_carries_resolved_catalogue_paths' tomlctl/tests/review_query.rs` prints `1` (today: prints nothing, exit 1).

### 10. Implement review digest and review report [M]
- **Files**: `tomlctl/src/review/digest.rs`, `tomlctl/tests/review_digest.rs` (new)
- **Depends on**: 8
- **Action**: Implement `review digest --run <U>` (counts by kind and lens, cells by status with notes for `thin`/`uncovered`, contested findings with both verdicts, refuted criticals, advisory list in the orchestrator projection) and `review report --run <U> [--out <path>]` (deterministic markdown: defects by severity, cleanup grouped by file, advisory with routing, coverage map with lens versions and facets, run line).
- **Detail**: Follow Approach, "Run pipeline" stages 5 and 7. The digest carries no `evidence` and defect detail only for contested items. Tests: `digest_lists_uncovered_cells_with_notes`, `digest_omits_evidence`, `report_groups_cleanup_by_file`, `report_is_byte_stable`.
- **Acceptance**: forward: `test -f tomlctl/tests/review_digest.rs && grep -c 'fn digest_omits_evidence' tomlctl/tests/review_digest.rs` prints `1` (today: prints nothing, exit 1).

### 11. Implement review triage with conversion and backlog refs [M]
- **Files**: `tomlctl/src/review/triage.rs`, `tomlctl/tests/review_triage.rs` (new)
- **Depends on**: 8
- **Action**: Implement `review triage <ids>` with `--dismiss`/`--defer --trigger`/`--reopen`/`--resolve`/`--backlog-ref`/`--convert cleanup|defect --ndjson`, the `--<flag>-file` twins and `--expect-status` compare-and-set.
- **Detail**: Mirror `tomlctl/src/ledgers.rs` `write_guarded` for the guard on the raw doc. `--convert` validates the new row through the field catalogue, mints it with an `orchestrator` verdict, and marks the advisory `converted` with `converted_to` in the same write. Tests: `triage_dismiss_requires_reason`, `triage_honours_expect_status`, `triage_reads_reason_from_file`, `convert_advisory_mints_cleanup_and_links`, `backlog_ref_marks_backlogged`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_triage.rs && grep -c 'fn convert_advisory_mints_cleanup_and_links' tomlctl/tests/review_triage.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: ignoring `--expect-status` turns `triage_honours_expect_status` red — **predicted, unverified**.

### 12. Implement review stats [M]
- **Files**: `tomlctl/src/review/stats.rs`, `tomlctl/tests/review_stats.rs` (new)
- **Depends on**: 8
- **Action**: Implement `review stats [--slug|--scope|--all] [--family] [--by lens|probe|run] [--calibration [--exemplars N]]` with the per-lens and per-probe metrics Approach, "v2 store and the tomlctl review group" lists, grouping `--by lens` on (lens id, `lens_version`).
- **Detail**: `--all` globs `.claude/flows/*/review-v2.toml` and `.claude/review-v2/*.toml`. Exemplars are the first sentence of `dismiss_reason` and refuted verdict `rationale`, grouped per lens, most frequent first, ties by recency. Refute rate counts `refuted` rows. Tests: `stats_refute_rate_per_lens`, `stats_groups_by_lens_version`, `stats_yield_by_kind`, `stats_all_aggregates_across_stores`, `calibration_lists_top_reasons`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_stats.rs && grep -c 'fn stats_groups_by_lens_version' tomlctl/tests/review_stats.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: computing refute rate over persisted rows only turns `stats_refute_rate_per_lens` red — **predicted, unverified**.

### 13. Implement review export-v1 [M]
- **Files**: `tomlctl/src/review/export.rs`, `tomlctl/tests/review_export.rs` (new)
- **Depends on**: 9
- **Action**: Implement `review export-v1 <ids> --to <v1-ledger> [--dry-run]` per Approach, "Export to v1".
- **Detail**: Build `description` from the `implementer` projection (task 9's `project` call). Take `category` from the finding's cell `v1_category`, falling back per family for probes and orchestrator checks. Validate the v1 required set (`id` by family prefix, `file`, `line`, `severity`, `effort` ∈ trivial/small/medium, `category`, `summary`, `first_flagged`, `rounds`, `status = "open"`). Tests: `export_refuses_advisory`, `export_refuses_plan_family`, `export_cleanup_as_suggestion`, `export_category_from_cell`, `export_reuses_open_v1_id_and_bumps_rounds_once`, `export_is_idempotent_on_v2_ref`, `export_regression_on_fixed_v1_match`, `export_stamps_last_updated_from_run_date`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_export.rs && grep -c 'fn export_is_idempotent_on_v2_ref' tomlctl/tests/review_export.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: bumping `rounds` on every export call turns `export_reuses_open_v1_id_and_bumps_rounds_once` red — **predicted, unverified**.

### 14. Implement the review catalogue verb [S]
- **Files**: `tomlctl/src/review/catalogue_cmd.rs`, `tomlctl/tests/review_catalogue.rs` (new)
- **Depends on**: 4
- **Action**: Implement `tomlctl review catalogue --builtin <dir> [--project <dir>] [--family <f>] [--signals <a,b>] [--vocabulary] [--check]` over `tomlctl/src/review/catalogue.rs`: the merged index as a row report, `--vocabulary` as the signal rows with `detect` rules, and `--check` exiting 1 with every `CatalogueError` listed.
- **Detail**: Follow Approach, "Lens catalogue" (**Generated index and validation**). `--project` defaults to `.claude/review-lenses` under the repo root when present. Tests (temp-dir catalogues): `catalogue_prints_merged_index`, `catalogue_signals_marks_matched`, `catalogue_check_exits_nonzero_on_invalid`, `catalogue_vocabulary_lists_detect_rules`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_catalogue.rs && grep -c 'fn catalogue_check_exits_nonzero_on_invalid' tomlctl/tests/review_catalogue.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: making `--check` exit 0 on errors turns `catalogue_check_exits_nonzero_on_invalid` red — **predicted, unverified**.

### 15. Attribute review-v2 store prompts in agent correlation [S]
- **Files**: `tomlctl/src/agents/correlate.rs`
- **Depends on**: —
- **Action**: Extend the `LEDGER` regex in `tomlctl/src/agents/correlate.rs` (around line 201) to accept the `review-v2.toml` basename, and add `ledger_line_attributes_review_v2_store` beside the existing in-module tests.
- **Detail**: Basename only — v2 dispatch prompts carry run and cell ids, not F-ids. Only the flow-local `.claude/flows/<slug>/review-v2.toml` attributes a flow. The prompts that carry the line are built by task 43's workflow and task 42's fallback.
- **Acceptance**:
  - forward: `grep -c 'review-v2' tomlctl/src/agents/correlate.rs` prints at least `1` (today: `0`).
  - falsifier: reverting the alternation turns `ledger_line_attributes_review_v2_store` red — **predicted, unverified**.

### 16. Add output-option cases and help-list entries for the review verbs [M]
- **Files**: `tomlctl/tests/output_options.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 5, 6, 7, 8, 9, 10, 11, 12, 13, 14
- **Action**: Give every `review` leaf a case in `cases()` of `tomlctl/tests/output_options.rs`, add a `review` arm to `group()` and the `group_tests!` list, and seed a `review-v2.toml` and a minimal catalogue in `fixture()`. Add the read verbs to `read_only_subcommands_hide_write_integrity_flags_in_help` and the write verbs to `write_subcommands_expose_all_integrity_flags_in_help` in `tomlctl/tests/capabilities.rs`.
- **Detail**: `every_leaf_command_has_a_case` walks `tomlctl capabilities .commands`, so the full suite is red from task 4 until this lands. Leaves: `run start`, `run update`, `run finish`, `plan`, `cover`, `add`, `verdict`, `list`, `show`, `digest`, `report`, `triage`, `stats`, `export-v1`, `catalogue`, `schema`.
- **Acceptance**:
  - forward: `grep -c '&\["review", ' tomlctl/tests/output_options.rs` prints at least `16` (today: `0`).
  - falsifier: deleting the `review catalogue` case turns `every_leaf_command_has_a_case` red — **predicted, unverified**.

### 17. Document the review verb group in the tomlctl skill [M]
- **Files**: `claude/skills/tomlctl/references/review.md` (new), `claude/skills/tomlctl/SKILL.md`, `tomlctl/README.md`
- **Depends on**: 5, 6, 7, 8, 9, 10, 11, 12, 13, 14
- **Action**: Write the `review` flag-table reference — one `### \`tomlctl review <verb>\`` heading and `| Flag |` table per verb, plus the store shape, the field catalogue summary and the role list. In `claude/skills/tomlctl/SKILL.md` add a Quick Reference row, a References entry and "Rows and header per command" rows for `review list`, `review show`, `review digest`, `review stats`, `review catalogue` and `review export-v1`. In `tomlctl/README.md` add `tomlctl review …` lines to the `## Usage` Quick-tour fence.
- **Detail**: Do **not** edit the `description:` of `claude/skills/tomlctl/SKILL.md` (it is at its 1024-character ceiling; measure with `awk`). Every `tomlctl review …` line in a bash fence must parse (`command_lint`), every flag-table row must be a real flag (`flag_table_lint`), and a heading names one verb path only. Keep the reference under 600 lines.
- **Acceptance**:
  - forward: `test -f claude/skills/tomlctl/references/review.md && grep -cE '^### `tomlctl review ' claude/skills/tomlctl/references/review.md` prints at least `16` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 18. Bump tomlctl to 0.16.0 and refresh the glimpse lockfile [L]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 4, 16, 17
- **Action**: Set `version = "0.16.0"` in `tomlctl/Cargo.toml`, update the version literal and message in `capabilities_version_matches_cargo_toml` (`tomlctl/tests/capabilities.rs`), update the `"version"` of the `capabilities` sample block in `tomlctl/README.md`, then refresh both lockfiles (`cargo tree --manifest-path glimpse/Cargo.toml >/dev/null` rewrites glimpse's).
- **Detail**: The pre-commit glimpse clippy runs `--locked`, so the refreshed `glimpse/Cargo.lock` is staged with the bump. `readme_sample_version_matches_cargo_toml` gates the README edit. Tasks 16 and 17 edit the same test file and README, hence the edges; the five edits are inseparable for a green tree, hence L.
- **Acceptance**:
  - forward: `grep -c '^version = "0.16.0"' tomlctl/Cargo.toml` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes, including `capabilities_version_matches_cargo_toml` and `readme_sample_version_matches_cargo_toml` — **predicted, unverified**.

### 19. Write the review-v2-method skill [M]
- **Files**: `claude/skills/review-v2-method/SKILL.md` (new), `claude/skills/review-v2-method/references/finding-kinds.md` (new), `claude/skills/review-v2-method/references/verifying.md` (new)
- **Depends on**: 4
- **Action**: Write the agent-side method every v2 agent preloads. `SKILL.md` holds one section per role — surveyor (reading the vocabulary, measuring signals, running surveyor checks, resolving verification-check commands, the two files and their limits, the ≤40-line digest), finder and sweeper (fetch the cell, read the bound lens, facets and methods, read the pack and `PRIOR`, the **probe template** for `probe:` cells, the three kinds and their required fields, `same_as`, the never-a-finding list, caps, the coverage note, `escalate`), verifier (refute-first, the three verdicts, the citation rule, striking `same_as`), checker (the mechanical predicates and the PLAUSIBLE-when-unsure rule), executor (the dry-run procedure) — plus the store-write idiom (stdin NDJSON, retry rules) and each role's return object. The references hold worked examples only: one row per kind and family in `finding-kinds.md`, one verdict per outcome in `verifying.md`.
- **Detail**: Follow Approach, "Finding kinds", Approach, "Field readership", Approach, "Run pipeline" and Approach, "Agents". A `skills:` preload injects `SKILL.md` only, so every procedure is in it. Defer source hierarchy, evidence grades and gated references to the unchanged `research-methods` skill by name; state that the Counter line and grade go in `counter` and `grade`. State that the store is read only through `tomlctl review list|show`, never `tomlctl items list`. Name the confirmation exclusion as v1's top drop class. Never use the words "target" or "quota" for counts. The description says when to use it (v2 surveyor, finder, sweeper, verifier, checker or executor agents) and stays under 1024 characters; the body stays under 500 lines. Every `tomlctl review …` line in a bash fence must parse (`command_lint`).
- **Acceptance**:
  - forward: `test -f claude/skills/review-v2-method/SKILL.md && grep -cE '^## .*(Surveyor|Finder|Verifier|Checker|Executor)' claude/skills/review-v2-method/SKILL.md` prints at least `5` (today: prints nothing, exit 1).
  - forward: `test -f claude/skills/review-v2-method/SKILL.md && ! grep -rqiE '\btarget [0-9]+|quota' claude/skills/review-v2-method && echo clean` prints `clean` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 20. Write the catalogue skill and vocabulary [M]
- **Files**: `claude/skills/review-v2-lenses/SKILL.md` (new), `claude/skills/review-v2-lenses/vocabulary.toml` (new)
- **Depends on**: 4
- **Action**: Write `SKILL.md` — the four entry types with their frontmatter schemas and body section orders, the layers and the project `config.toml` (`disable`, `enable`, `extend`, `[[checks]]`) with a worked example, the escape-hatch rule and the collision refusal, authoring and versioning rules, the lifecycle and promotion path, the probe template's relation to lenses, how to list the catalogue (`tomlctl review catalogue`), and the lumina mapping. Write `vocabulary.toml` — every signal (diff, health, stack) with `kind`, `meaning` and a concrete `detect` rule, and every concern tag with `meaning`, `families` and `owner`, covering the 30 lenses in Approach, "Lens catalogue".
- **Detail**: Follow Approach, "Lens catalogue". The vocabulary's concern ownership is the contract tasks 23–32 write against, so assign every tag here, derived from the lens table's "From" column (e.g. `rename-ripple`, `registration-seam` and `justification-consumer` → `ripple`; `illegal-state` and `stringly-typed-set` → `type-design`; `stale-comment` and `dead-code` → `hygiene`). `detect` rules are commands or globs a Haiku agent runs verbatim (e.g. `lang:rust` ← `Cargo.toml` at the root or one level down; `test-growth` ← test-function count now vs 30 days ago via `git log`). The SKILL.md description says when to use it (authoring or maintaining catalogue entries, configuring a project) and stays under 1024 characters; body under 500 lines. Every `tomlctl review …` line in a bash fence must parse (`command_lint`).
- **Acceptance**: forward: `test -f claude/skills/review-v2-lenses/vocabulary.toml && grep -c '^\[concerns\.' claude/skills/review-v2-lenses/vocabulary.toml` prints at least `30` (today: prints nothing, exit 1).

### 21. Write the methods: sweep, trace, history-idiom [M]
- **Files**: `claude/skills/review-v2-lenses/methods/sweep.md` (new), `claude/skills/review-v2-lenses/methods/trace.md` (new), `claude/skills/review-v2-lenses/methods/history-idiom.md` (new)
- **Depends on**: 20
- **Action**: Write three method entries in the catalogue format.
- **Detail**: `sweep` carries the "Sweep for every instance" procedure of `claude/skills/research-methods/references/codebase-review.md` (derive the sweep from the defect, cover the forms, search the whole tree, report set + search + state, `tomlctl sweep`) plus `/review-plan`'s "one line per sweep, even when it finds nothing". `trace` carries "Contract first" and "Trace before you claim". `history-idiom` carries "Idiom questions are history questions". Rewrite rather than copy; reference the v1 skill by name only.
- **Acceptance**: forward: `for f in sweep trace history-idiom; do p=claude/skills/review-v2-lenses/methods/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 22. Write the methods: measure, dry-run, steelman [M]
- **Files**: `claude/skills/review-v2-lenses/methods/measure.md` (new), `claude/skills/review-v2-lenses/methods/dry-run.md` (new), `claude/skills/review-v2-lenses/methods/steelman.md` (new)
- **Depends on**: 20
- **Action**: Write three method entries.
- **Detail**: `measure` is the perf evidence ladder (benchmark or profile → complexity argument on a real input size → allocation count → hypothesis), with "run a benchmark that exists, never build one". `dry-run` is the plan-executor procedure (R9). `steelman` is the contrarian procedure from lumina's `research-explore` contrarian definition (cite a competing approach, a counter-example or a documented failure — never generic "consider alternatives").
- **Acceptance**: forward: `for f in measure dry-run steelman; do p=claude/skills/review-v2-lenses/methods/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 23. Write the lenses: correctness, type-design, domain-model [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/correctness.md` (new), `claude/skills/review-v2-lenses/lenses/type-design.md` (new), `claude/skills/review-v2-lenses/lenses/domain-model.md` (new)
- **Depends on**: 20
- **Action**: Write three lens entries in the catalogue format, with frontmatter per Approach, "Lens catalogue" (lens table) and Owns sections covering exactly the tags `claude/skills/review-v2-lenses/vocabulary.toml` assigns each.
- **Detail**: `correctness` (floor) works the boundary list of `claude/skills/research-methods/references/codebase-review.md` (inputs, error paths, concurrency and ordering, resource lifetimes, cross-layer invariants) plus `claude/commands/review.md` Agent 4's edge cases, and documents the context-free diff-only variant. `type-design` and `domain-model` split `claude/commands/review.md` Agents 1 and 3 as the lens table says. Seed every Does not flag section from the v1 rejection clusters and the source agent's own "Do not flag" sentence.
- **Acceptance**: forward: `for f in correctness type-design domain-model; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 24. Write the lenses: architecture, idioms, duplication [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/architecture.md` (new), `claude/skills/review-v2-lenses/lenses/idioms.md` (new), `claude/skills/review-v2-lenses/lenses/duplication.md` (new)
- **Depends on**: 20
- **Action**: Write three lens entries (same format rules as task 23).
- **Detail**: `architecture` from `claude/commands/review.md` Agent 3 (layering, placement, evolvability; mostly advisory). `idioms` from Agent 1, citing the `history-idiom` method so it never canonises an outlier. `duplication` from Agent 1's DRY plus the method notes' "facts with one owner, rule of three".
- **Acceptance**: forward: `for f in architecture idioms duplication; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 25. Write the lenses: security, external-validity, hygiene [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/security.md` (new), `claude/skills/review-v2-lenses/lenses/external-validity.md` (new), `claude/skills/review-v2-lenses/lenses/hygiene.md` (new)
- **Depends on**: 20
- **Action**: Write three lens entries (same format rules as task 23).
- **Detail**: `security` (code and plan probes; trust-boundary signal; hard cap 5) from `claude/commands/review.md` Agent 2. `external-validity` (code and plan) merges Agent 2's advisory lookups for added or bumped dependencies with `claude/commands/review-plan.md` Agent 4's pinned-API, deprecation and advisory checks. `hygiene` (mechanical) yields cleanup: dead code (traced), comment rot, stale TODOs, naming drift.
- **Acceptance**: forward: `for f in security external-validity hygiene; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 26. Write the lenses: ripple, test-gaps, seams [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/ripple.md` (new), `claude/skills/review-v2-lenses/lenses/test-gaps.md` (new), `claude/skills/review-v2-lenses/lenses/seams.md` (new)
- **Depends on**: 20
- **Action**: Write three lens entries (same format rules as task 23).
- **Detail**: `ripple` (mechanical, floor for code and plan, cites `sweep`) merges `claude/commands/review.md` Agent 4's ripple with `claude/commands/review-plan.md` Agent 2 sweeps 1–3 under separate code and plan probe sections. `test-gaps` (code and plan) aims at the regression risk the change carries (`claude/commands/review.md` Agent 5; `review-plan.md` Agent 2's missing tests). `seams` from Agent 5's seams.
- **Acceptance**: forward: `for f in ripple test-gaps seams; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 27. Write the lenses: diagnostics-dx, test-pruning, doc-drift [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/diagnostics-dx.md` (new), `claude/skills/review-v2-lenses/lenses/test-pruning.md` (new), `claude/skills/review-v2-lenses/lenses/doc-drift.md` (new)
- **Depends on**: 20
- **Action**: Write three lens entries (same format rules as task 23).
- **Detail**: `diagnostics-dx` (mechanical) from `claude/commands/review.md` Agent 5's error messages, logs and help text. `test-pruning` (health signals `test-growth`, `test-ratio-high`) yields `direction = remove` cleanup for tests that only re-assert the implementation, duplicate another test, or pin incidental behaviour, and states the coverage each removal gives up. `doc-drift` (mechanical) checks prose against behaviour.
- **Acceptance**: forward: `for f in diagnostics-dx test-pruning doc-drift; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 28. Write the lenses: doc-consolidation, contrarian, synthesis [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/doc-consolidation.md` (new), `claude/skills/review-v2-lenses/lenses/contrarian.md` (new), `claude/skills/review-v2-lenses/lenses/synthesis.md` (new)
- **Depends on**: 20
- **Action**: Write three lens entries (same format rules as task 23).
- **Detail**: `doc-consolidation` (health signal `doc-growth`) yields merges and removals of duplicated docs. `contrarian` (floor in all three families, advisory only, cites `steelman`) carries lumina's `research-explore` contrarian definition and `story-review`'s argue-against and scope-conservatism angles. `synthesis` (all families; standard and up, after the first wave) reads `review digest` and the scope and reports only cross-cell emergent defects and "what every cell missed"; its perf probes carry `claude/commands/optimise.md`'s cross-cutting concurrency review.
- **Acceptance**: forward: `for f in doc-consolidation contrarian synthesis; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 29. Write the perf lenses: memory-runtime, data-shape, data-access [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/memory-runtime.md` (new), `claude/skills/review-v2-lenses/lenses/data-shape.md` (new), `claude/skills/review-v2-lenses/lenses/data-access.md` (new)
- **Depends on**: 20
- **Action**: Write three perf lens entries (same format rules as task 23) from `claude/commands/optimise.md` Agents 1–3.
- **Detail**: Follow Approach, "Family specifics". Each cites `measure`; a perf defect states workload → cost with magnitude, otherwise it is advisory (`angle = risk`). Stack-specific text in the v1 agents (frontend reactivity, ORM pitfalls) moves to facets (tasks 33–35), not into these files. Keep each lens's `research_domain` from its v1 agent.
- **Acceptance**: forward: `for f in memory-runtime data-shape data-access; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 30. Write the perf lenses: algorithm, concurrency, io-process [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/algorithm.md` (new), `claude/skills/review-v2-lenses/lenses/concurrency.md` (new), `claude/skills/review-v2-lenses/lenses/io-process.md` (new)
- **Depends on**: 20
- **Action**: Write three perf lens entries (same format rules as task 23).
- **Detail**: `algorithm` (floor for perf) carries `claude/commands/optimise.md` Agent 4, keeping the boundary that domain modelling belongs to `type-design`. `concurrency` keeps Agent 5's coverage and severity calibration. `io-process` covers process start-up, syscalls, filesystem walks and subprocess fan-out. Stack-specific text goes to facets.
- **Acceptance**: forward: `for f in algorithm concurrency io-process; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 31. Write the plan lenses: plan-grounding, plan-dependencies, feasibility [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/plan-grounding.md` (new), `claude/skills/review-v2-lenses/lenses/plan-dependencies.md` (new), `claude/skills/review-v2-lenses/lenses/feasibility.md` (new)
- **Depends on**: 20
- **Action**: Write three plan lens entries (same format rules as task 23) from `claude/commands/review-plan.md` Step 2.
- **Detail**: `plan-grounding` (floor) carries Agent 2's alignment and sweeps 4–5 (re-derived enumerations, behavioural premises measured). `plan-dependencies` (floor) carries Agent 1's edge-completeness and checkpoint-placement judgement, reading `STORE CHECK` as ground truth for stated edges. `feasibility` carries Agent 1's feasibility and coherence judgement.
- **Acceptance**: forward: `for f in plan-grounding plan-dependencies feasibility; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 32. Write the plan lenses: plan-executor, plan-risk, plan-premortem [M]
- **Files**: `claude/skills/review-v2-lenses/lenses/plan-executor.md` (new), `claude/skills/review-v2-lenses/lenses/plan-risk.md` (new), `claude/skills/review-v2-lenses/lenses/plan-premortem.md` (new)
- **Depends on**: 20
- **Action**: Write three plan lens entries (same format rules as task 23).
- **Detail**: `plan-executor` (floor, mode `executor`, slice `per-task`, cites `dry-run`) also carries `claude/commands/review-plan.md` Agent 3's executability checks (Files↔body parity, executor-time decisions, sizing). `plan-risk` carries Agent 4's rollback adequacy, risky-task checkpoint placement, back-compat and scope realism. `plan-premortem` is `status = "experimental"`, `default = "off"`, with an "experimental — weak evidence" line.
- **Acceptance**: forward: `for f in plan-executor plan-risk plan-premortem; do p=claude/skills/review-v2-lenses/lenses/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 33. Write the facets: rust, tokio, sqlx [M]
- **Files**: `claude/skills/review-v2-lenses/facets/rust.md` (new), `claude/skills/review-v2-lenses/facets/tokio.md` (new), `claude/skills/review-v2-lenses/facets/sqlx.md` (new)
- **Depends on**: 20
- **Action**: Write three facet entries in the catalogue format, each with a general section and a `### For <lens-id>` section per attached lens.
- **Detail**: `rust` (`lang:rust`) attaches to correctness, type-design, idioms, memory-runtime, algorithm and hygiene; `tokio` (`runtime:tokio`) to concurrency and correctness, carrying `claude/commands/optimise.md` Agent 5's runtime specifics; `sqlx` (`db:sqlx`) to data-access and domain-model, including the never-edit-an-applied-migration rule. Probes are concrete pitfalls, not tutorials.
- **Acceptance**: forward: `for f in rust tokio sqlx; do p=claude/skills/review-v2-lenses/facets/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 34. Write the facets: typescript, react, vue [M]
- **Files**: `claude/skills/review-v2-lenses/facets/typescript.md` (new), `claude/skills/review-v2-lenses/facets/react.md` (new), `claude/skills/review-v2-lenses/facets/vue.md` (new)
- **Depends on**: 20
- **Action**: Write three facet entries (same format rules as task 33).
- **Detail**: `react` and `vue` carry the frontend text of `claude/commands/optimise.md` Agents 1 and 4 (reactive-object overhead, component proliferation, bundle size, tree-shaking barriers, reactive dependency chains, reconciliation cost) for memory-runtime and algorithm, plus correctness and idioms probes. `typescript` (`lang:ts`) attaches to type-design, correctness and idioms.
- **Acceptance**: forward: `for f in typescript react vue; do p=claude/skills/review-v2-lenses/facets/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 35. Write the facets: dotnet, efcore, claude-harness [M]
- **Files**: `claude/skills/review-v2-lenses/facets/dotnet.md` (new), `claude/skills/review-v2-lenses/facets/efcore.md` (new), `claude/skills/review-v2-lenses/facets/claude-harness.md` (new)
- **Depends on**: 20
- **Action**: Write three facet entries (same format rules as task 33).
- **Detail**: `dotnet` (`lang:csharp`) attaches to correctness, type-design, idioms, memory-runtime (`/optimise` Agent 1's boxing, devirtualisation and JIT/AOT notes) and concurrency. `efcore` (`db:efcore`) carries `/optimise` Agent 3's ORM query-translation pitfalls (compiled queries, tracking, N+1, pagination) for data-access and domain-model. `claude-harness` (`harness:claude`) is the portable judgement half of `claude/commands/review.md` Agent 6 — frontmatter, section order, cross-references, stubs, terminology — for hygiene, doc-drift and ripple.
- **Acceptance**: forward: `for f in dotnet efcore claude-harness; do p=claude/skills/review-v2-lenses/facets/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 36. Write the checks: lint-baseline, profile-baseline, dev-server-probe [M]
- **Files**: `claude/skills/review-v2-lenses/checks/lint-baseline.md` (new), `claude/skills/review-v2-lenses/checks/profile-baseline.md` (new), `claude/skills/review-v2-lenses/checks/dev-server-probe.md` (new)
- **Depends on**: 20
- **Action**: Write three check entries in the catalogue format.
- **Detail**: `lint-baseline` and `profile-baseline` are `executor = "verification"` with a `discover` ladder: a flow plan's `## Verification Commands` key (`lint:` / `bench:`), then CLAUDE.md, then an ecosystem default chosen by stack signal (`cargo clippy`, `tsc --noEmit`, `dotnet build`), or "none — <reason>". `profile-baseline` carries `claude/commands/optimise.md`'s declared-bench sources. `dev-server-probe` (`executor = "surveyor"`) writes the `DEV SERVER` fence.
- **Acceptance**: forward: `for f in lint-baseline profile-baseline dev-server-probe; do p=claude/skills/review-v2-lenses/checks/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 37. Write the checks: store-check, format-contract, break-set-baseline [M]
- **Files**: `claude/skills/review-v2-lenses/checks/store-check.md` (new), `claude/skills/review-v2-lenses/checks/format-contract.md` (new), `claude/skills/review-v2-lenses/checks/break-set-baseline.md` (new)
- **Depends on**: 20
- **Action**: Write three check entries.
- **Detail**: `store-check` (`executor = "surveyor"`, plan) runs the task-store import and `tomlctl tasks check --plan` per the `flow-contract-task-store` skill into `STORE CHECK`. `format-contract` (`executor = "surveyor"`, plan) points the pack at the `flow-contract-plan-output-format` skill's Format rules when the plan is house-format. `break-set-baseline` (`executor = "orchestrator"`, plan) carries `claude/commands/review-plan.md` Step 2.6 check 1.
- **Acceptance**: forward: `for f in store-check format-contract break-set-baseline; do p=claude/skills/review-v2-lenses/checks/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 38. Write the checks: two-control-probe, falsifiability, acceptance-contradiction [M]
- **Files**: `claude/skills/review-v2-lenses/checks/two-control-probe.md` (new), `claude/skills/review-v2-lenses/checks/falsifiability.md` (new), `claude/skills/review-v2-lenses/checks/acceptance-contradiction.md` (new)
- **Depends on**: 20
- **Action**: Write three `executor = "orchestrator"` plan checks from `claude/commands/review-plan.md` Step 2.6 checks 4, 3 and 2.
- **Detail**: `two-control-probe` points at the `flow-contract-plan-output-format` skill's `references/acceptance-probing.md` rather than restating it, and keeps the verdict-to-severity mapping. Each check states how its results become findings (kind `defect`, the severity v1 assigns).
- **Acceptance**: forward: `for f in two-control-probe falsifiability acceptance-contradiction; do p=claude/skills/review-v2-lenses/checks/$f.md; test -f $p && grep -qx "id = \"$f\"" $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 39. Write this repo's project catalogue config [S]
- **Files**: `.claude/review-lenses/config.toml` (new)
- **Depends on**: 20
- **Action**: Write the project config declaring the `harness-gates` check (`families = ["code"]`, `paths = ["claude/**"]`, command `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`) and nothing else.
- **Detail**: Follow Approach, "Lens catalogue" (**Layers**). Every other entry stays at its built-in default; this file is the worked example of a minimal project config.
- **Acceptance**: forward: `grep -c 'id = "harness-gates"' .claude/review-lenses/config.toml` prints `1` (today: prints nothing, exit 2).

### 40. Gate the built-in catalogue in the dispatch tests [M]
- **Files**: `tomlctl/src/cli/dispatch/tests/catalogue.rs` (new), `tomlctl/src/cli/dispatch/tests/mod.rs`
- **Depends on**: 3, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39
- **Action**: Add `catalogue_validates`, which runs `tomlctl/src/review/catalogue.rs` `validate()` over `claude/skills/review-v2-lenses/` with `.claude/review-lenses/` as the project layer and fails listing every error, plus `catalogue_entry_counts_match_the_plan` asserting 30 lenses, 9 facets, 6 methods and 9 checks; register `mod catalogue;` in `tomlctl/src/cli/dispatch/tests/mod.rs`.
- **Detail**: Resolve paths from the repo root the way `tomlctl/src/cli/dispatch/tests/skills.rs` does. The pre-commit hook already runs `cli::dispatch::tests` on staged `claude/**/*.md`, so a broken catalogue edit blocks the commit. Fix any validation error the first run reports in the offending entry file, not by loosening the validator.
- **Acceptance**:
  - forward: `grep -c '^mod catalogue;' tomlctl/src/cli/dispatch/tests/mod.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests::catalogue` passes — **predicted, unverified**.
  - falsifier: removing the `Owns` section from `claude/skills/review-v2-lenses/lenses/ripple.md` turns `catalogue_validates` red — **predicted, unverified**.

### 41. Write the review-v2-engine skill: pipeline, survey, compose [M]
- **Files**: `claude/skills/review-v2-engine/SKILL.md` (new), `claude/skills/review-v2-engine/references/survey.md` (new), `claude/skills/review-v2-engine/references/compose.md` (new)
- **Depends on**: 4, 20
- **Action**: Write the orchestrator contract: stages 0–8, the executor of each, the orchestrator's judgement-only role and the depth envelopes in `SKILL.md`; the surveyor dispatch, the two run files, the check execution by executor, the fence catalogue and the verification-check dispatch in `references/survey.md`; the coverage-map rules (floor, risk slicing, default-off lenses, maintenance activation from health signals, facet `+`/`-` edits, probe authoring, orchestrator-check cells, budgets, follow-up-wave criteria, reading `review catalogue` and `review stats`) in `references/compose.md`.
- **Detail**: Follow Approach, "Run pipeline", Approach, "Lens catalogue" and Approach, "Design principles". Stage 0 checks `review_add` and `review_catalogue` in `tomlctl capabilities` `.features`; stage 1 passes `--builtin` as the base directory the Skill tool reports for `review-v2-lenses`. Every flow-scoped dispatch prompt opens with the column-0 `ledger: .claude/flows/<slug>/review-v2.toml` line. The orchestrator reads `review catalogue` rows, `review digest` and projections only — never lens bodies or raw finding rows. Invoke the unchanged `tomlctl`, `flow-contract-flow-context` and `backlog-capture` skills, and the `review-v2-lenses` skill, by name. Every `tomlctl review …` line in a bash fence must parse (`command_lint`). Description ≤1024 characters, body ≤500 lines, each reference ≤600.
- **Acceptance**:
  - forward: `test -f claude/skills/review-v2-engine/SKILL.md && grep -c 'tomlctl review catalogue' claude/skills/review-v2-engine/SKILL.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 42. Write the engine references: agent waves, triage and close [M]
- **Files**: `claude/skills/review-v2-engine/references/agent-waves.md` (new), `claude/skills/review-v2-engine/references/triage-and-close.md` (new)
- **Depends on**: 4
- **Action**: Write the Agent-tool fallback (the same per-cell prompt template as the Workflow, cell waves then verifier/checker waves of ≤16 per message, one re-dispatch for a malformed return, one-shot dispatch with no `name:`, the column-0 `ledger:` line when a flow is bound) and the triage-and-close procedure (audit sampling, the advisory recommendation and apply-now conversion, the disposition grammar mapped to `review triage` calls, the backlog hand-off via `review show --for backlog` and the `backlog-capture` skill's check-then-add gate, `export-v1` and the apply hand-off, `run finish`, the report).
- **Detail**: Follow Approach, "Run pipeline" and Approach, "Finding kinds". Dispositions are a user-engagement gate the autonomy directive does not override; user prose is staged with the Write tool and passed through the `--<flag>-file` twins. Every `tomlctl review …` line in a bash fence must parse.
- **Acceptance**:
  - forward: `for f in agent-waves triage-and-close; do test -f claude/skills/review-v2-engine/references/$f.md || echo MISSING $f; done | wc -l` prints `0` (today: `2`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 43. Write the review-v2 saved workflow and its deploy entry [L]
- **Files**: `claude/workflows/review-v2.js` (new), `scripts/deploy-claude.ps1`
- **Depends on**: —
- **Action**: Write the saved Workflow script (`export const meta = {name: 'review-v2-pipeline', …}`, phases Cells and Verify, a `SCRIPT_VERSION` const returned as `script_version`) that runs one wave over `args.cells` and pipelines each cell's returned ids to verifiers and checkers. Add a `workflows` area (`Filter = '*.js'`, file symlinks) to `scripts/deploy-claude.ps1` and name it in the script's SYNOPSIS/DESCRIPTION.
- **Detail**: Follow Approach, "Run pipeline" (**Substrate** fixes `args` and the return). Idioms: `agent(prompt, {agentType, schema, label, phase})` with `args.schemas.cell_return` for cells and `args.schemas.verdict_return` for verifiers and checkers; defect ids → one `review-verifier` each (three when `critical_panel` and severity is critical — the cell return carries the minted ids' kinds and severities); cleanup ids → one `review-checker` per cell; `pipeline()` so verification starts per cell; no `Date.now()`/`Math.random()`; prompts built only from `args`; `log()` every skipped cell and escalation; return plain data only. Cells with `agent = "orchestrator"` are never in `args.cells`.
- **Acceptance**:
  - forward: `test -f claude/workflows/review-v2.js && grep -c "name: 'review-v2-pipeline'" claude/workflows/review-v2.js` prints `1` (today: prints nothing, exit 1).
  - forward: `test -f claude/workflows/review-v2.js && t=$(mktemp --suffix .js) && { echo 'async function __wf(agent, pipeline, parallel, log, phase, args, budget, workflow) {'; sed 's/^export //' claude/workflows/review-v2.js; echo '}'; } > "$t" && node --check "$t"` exits 0 (today: exits 1, file absent).
  - falsifier: deleting one closing parenthesis in the script makes the `node --check` probe exit 1.
  - forward: `grep -c "Name = 'workflows'" scripts/deploy-claude.ps1` prints `1` (today: `0`).

### 44. Write the Opus agents: review-finder, review-verifier, plan-executor [M]
- **Files**: `claude/agents/review-finder.md` (new), `claude/agents/review-verifier.md` (new), `claude/agents/plan-executor.md` (new)
- **Depends on**: —
- **Action**: Write three agents with frontmatter `name`, `description`, `tools`, `model: opus`, `effort` (finder high, verifier high, executor medium), `color`, and `skills:` (finder and verifier: `review-v2-method`, `research-methods`; executor: `review-v2-method`). Each body carries the `review-v2-store-writer` block verbatim from Approach, "Agents" and the `research-delivering` block byte-identically from `claude/agents/research-deep.md`.
- **Detail**: Follow Approach, "Agents" (tools per its table). Descriptions name what the agent does, never a carrier's stage numbers. Bodies say "if `review-v2-method` is not loaded, invoke it first", read catalogue files from the paths in the cell assignment, and name no `tomlctl review` command in a bash fence.
- **Acceptance**: forward: `grep -l '^model: opus' claude/agents/review-finder.md claude/agents/review-verifier.md claude/agents/plan-executor.md 2>/dev/null | wc -l` prints `3` (today: `0`).

### 45. Write the Haiku agents: review-surveyor, review-sweeper, review-checker [M]
- **Files**: `claude/agents/review-surveyor.md` (new), `claude/agents/review-sweeper.md` (new), `claude/agents/review-checker.md` (new)
- **Depends on**: —
- **Action**: Write three agents with `model: haiku`, `effort: medium`, `skills: [review-v2-method]`, tools per Approach, "Agents", and the same two blocks as task 44.
- **Detail**: Follow Approach, "Agents". Haiku bodies are short and procedural: numbered steps, the exact return object, and explicit stop conditions (the sweeper's `escalate`, the checker's PLAUSIBLE-when-unsure, the surveyor's "measure, never judge" and its verbatim execution of vocabulary `detect` rules). Same body rules as task 44.
- **Acceptance**: forward: `grep -l '^model: haiku' claude/agents/review-surveyor.md claude/agents/review-sweeper.md claude/agents/review-checker.md 2>/dev/null | wc -l` prints `3` (today: `0`).

### 46. Register the v2 agents as shared-block carriers [S]
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: 44, 45
- **Action**: Add a `[[block]]` named `review-v2-store-writer` listing the six v2 agent files, and add the six files to the `research-delivering` block's `files`.
- **Detail**: Run `bash scripts/verify-shared-blocks.sh`; a byte difference between the six copies is fixed in the agent files, never by dropping a carrier.
- **Acceptance**:
  - forward: `grep -c 'review-v2-store-writer' scripts/shared-blocks.toml` prints `1` (today: `0`).
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0 (today: exits 0).

### 47. Write the /review-v2 carrier [M]
- **Files**: `claude/commands/review-v2.md` (new)
- **Depends on**: 17, 19, 40, 41, 42, 43, 46
- **Action**: Write the code-family carrier: frontmatter (`description`, `argument-hint` with `--depth` and `--flow`), invocations of the `tomlctl`, `flow-contract-flow-context`, `review-v2-engine`, `review-v2-lenses` and `backlog-capture` skills, and its deltas — the code floor, the context-free correctness cell, export to `review-ledger.toml` and the `/review-apply` hand-off.
- **Detail**: Follow Approach, "Family specifics" and Approach, "Coexistence with v1". The carrier restates nothing the engine owns. Each skill invocation uses the backticked-name-plus-"skill" phrase the gate requires.
- **Acceptance**:
  - forward: `test -f claude/commands/review-v2.md && grep -c '`review-v2-engine` skill' claude/commands/review-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 48. Write the /optimise-v2 carrier [M]
- **Files**: `claude/commands/optimise-v2.md` (new)
- **Depends on**: 17, 19, 40, 41, 42, 43, 46
- **Action**: Write the perf-family carrier with invocations of the `tomlctl`, `flow-contract-flow-context`, `review-v2-engine`, `review-v2-lenses` and `backlog-capture` skills, plus its deltas: the Focal Points Brief (now only what the facets do not already carry), `PROFILE BASELINE` via the `profile-baseline` check, the perf defect rule, `branch1..branch2` scope, and export to `optimise-findings.toml` with the `/optimise-apply` hand-off.
- **Detail**: Follow Approach, "Family specifics". Each skill invocation uses the backticked-name-plus-"skill" phrase the gate requires.
- **Acceptance**:
  - forward: `test -f claude/commands/optimise-v2.md && grep -c 'PROFILE BASELINE' claude/commands/optimise-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 49. Write the /review-plan-v2 carrier [M]
- **Files**: `claude/commands/review-plan-v2.md` (new), `tomlctl/src/cli/dispatch/tests/finding_classes.rs`
- **Depends on**: 17, 19, 40, 41, 42, 43, 46
- **Action**: Write the plan-family carrier with invocations of the `tomlctl`, `flow-contract-flow-context`, `flow-contract-plansdirectory-prompt`, `flow-contract-task-store`, `flow-contract-plan-output-format`, `review-v2-engine`, `review-v2-lenses` and `backlog-capture` skills. Deltas: plan resolution, the `store-check` and `format-contract` checks, the four orchestrator checks as cells, `plan-executor` cells, and the verified-only merge offer with the merge-exit ref-set gate → import → check. Add `claude/commands/review-plan-v2.md` to `PROSE` in `tomlctl/src/cli/dispatch/tests/finding_classes.rs`.
- **Detail**: Follow Approach, "Family specifics". Name every `tasks check` finding class exactly as it exists in `tomlctl/src/tasks` (`prose_class_names_exist_in_the_source` gates it). The merge never runs Manual mode on an empty `AskUserQuestion` answer.
- **Acceptance**:
  - forward: `test -f claude/commands/review-plan-v2.md && grep -c 'plan-executor' claude/commands/review-plan-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - forward: `grep -c 'review-plan-v2.md' tomlctl/src/cli/dispatch/tests/finding_classes.rs` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 50. Add the v2 carriers to the skill gate table, ignore run artifacts, document v2 in CLAUDE.md [M]
- **Files**: `tomlctl/src/cli/dispatch/tests/skills.rs`, `.gitignore`, `CLAUDE.md`
- **Depends on**: 47, 48, 49
- **Action**: Add rows for `review-v2.md`, `optimise-v2.md` and `review-plan-v2.md` to the `carrier_invokes_required_skills` table in `tomlctl/src/cli/dispatch/tests/skills.rs`, each listing exactly the skills its carrier invokes. Add `/.claude/review-v2-runs/` to `.gitignore`. Add a short "Review v2" subsection to `CLAUDE.md` after "Flow agent tiers" — the three commands, the six agents and their model tiers (outside the `lite`/`deep` buckets, model in frontmatter only, never named at dispatch), the store, the orchestrator's judgement-only role, the global catalogue with its optional per-project `config.toml`, and the saved-workflow deploy step — and add `workflows` to the `claude/{agents,commands,skills,themes}` list in its "Developer setup" deploy sentence.
- **Detail**: Every listed skill must exist under `claude/skills/`.
- **Acceptance**:
  - forward: `grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs` prints `3` (today: `0`).
  - forward: `grep -c 'review-v2-runs' .gitignore` prints `1` (today: `0`).
  - forward: `grep -c '/review-v2' CLAUDE.md` prints at least `1` (today: `0`).
  - falsifier: removing the `review-v2-engine` invocation from `claude/commands/review-v2.md` turns `carrier_invokes_required_skills` red — **predicted, unverified**.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 15, 18 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18. The `tomlctl review` group, including the catalogue loader, validator and verb, complete, tested, documented and version-bumped. A buildable crate increment; every later markdown fence naming `tomlctl review` parses against it.

— CHECKPOINT B after tasks 19, 40, 41, 42, 43, 46 — dependency closure: 1, 2, 3, 4, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46. The v2 harness foundation: method, the full catalogue gated by `catalogue_validates`, engine, six agents, workflow and deploy entry. Buildable because no carrier references it yet; `verify-shared-blocks.sh` covers the new agents.

— CHECKPOINT C after tasks 47 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 17, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47. `/review-v2` usable end to end, the first trial-ready carrier.

— CHECKPOINT D after tasks 50 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 17, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50. `/optimise-v2`, `/review-plan-v2`, the gate rows, the ignore rule and CLAUDE.md.

## Risks

- **A built-in catalogue edit changes every project at once.** Mitigations: entries carry `version` and `status` (new or reworked entries ship `experimental`, default off); runs bind versions so `review stats` shows a regression against the edit; `catalogue_validates` blocks a structurally broken edit at commit time.
- **Per-finding verification and composed waves multiply agents.** Mitigations: verification is per kind (Haiku for cleanup, none for advisory), the depth envelope caps cells and waves, the orchestrator states the expected agent count at compose time, and every run records `waves` and computed counts.
- **Orchestrator composition is only as good as the survey and the catalogue index.** Mitigations: floor lenses always run; `uncovered`/`thin` cells drive follow-up waves; `contrarian` and `synthesis` look for what the map missed; `review stats` shows lenses and probes with high uncovered or dismissal rates.
- **Facet quality varies by stack.** The user's repos span Rust, TypeScript/React/Vue and .NET; a thin facet yields generic findings on that stack. Mitigations: facets are additive (a lens still runs without one); a recurring probe on one stack is the signal to deepen its facet.
- **The vocabulary's ownership map is a single point of edit.** Mitigation: it is what makes parallel lens authoring safe and one-owner-per-concern checkable; the gate reports every ownership error at once.
- **Concurrent store writes from up to 16 agents.** Mitigations: short locked writes with ids minted inside the lock; the method's retry-once rule covers the Windows sidecar-rename `os error 5`; contention is observable on the first standard run.
- **Haiku output quality.** Mitigations: Haiku work is mechanical by design and validated by tomlctl (field catalogue, `old`-text check, catalogue resolution); the checker cannot CONFIRM what it cannot settle mechanically; sweepers escalate judgement to `review-finder`; Haiku never verifies a defect; the surveyor runs vocabulary `detect` rules verbatim.
- **Uncapped cleanup floods the report.** Mitigations: one pattern is one row with `instances`; the report groups cleanup by file; the field caps keep rows short; `review stats` shows dismissal rate per lens.
- **The verifier refutes true findings (recall loss).** Mitigations: the orchestrator audits every REFUTED critical; refuted rows persist and show in `stats` per lens.
- **Explicit `same_as` identity depends on finders reading `PRIOR`.** A missed `same_as` mints a duplicate rather than losing a finding; duplicates carry their candidates under `related`.
- **Workflow is unavailable, disabled, needs per-run approval, or the deployed copy is stale.** Mitigations: the agent-wave fallback has the same persistence contract and fires on launch `error` or an empty result; the run records `substrate` and `SCRIPT_VERSION`.
- **The `skills:` preload injects `SKILL.md` without `references/`.** Mitigation: every procedure is in `review-v2-method`'s `SKILL.md`; catalogue files come by absolute path in the cell assignment.
- **Markdown documenting `tomlctl review` fails `command_lint` if it lands before the clap tree.** Mitigation: tasks 19, 20, 41 and 42 depend on task 4, and the carriers on task 17; the agent bodies and the shared block use no fenced `tomlctl review` command.

## After Merge

- Run `cargo install --path tomlctl`; the carriers shell out to the installed binary, `review` lands in 0.16.0, and each v2 carrier halts at stage 0 on an older binary.
- Rerun `pwsh -File scripts/deploy-claude.ps1` to link the six new agents, three commands, three skills (the catalogue travels inside `review-v2-lenses`) and `claude/workflows/review-v2.js` into `~/.claude/`. Where a session reads an out-of-band config directory (such as `~/.claude-work` on the machine the original draft was researched on), copy the same files there and recopy after every catalogue or script change. Restart open sessions.
- Smoke test, **predicted, unverified**: `tomlctl review catalogue --builtin ~/.claude/skills/review-v2-lenses --check`, then `/review-v2 --depth quick tomlctl/src/sweep.rs`, `tomlctl review digest --scope <scope> --run U1` and `tomlctl review stats --scope <scope>`. Run one `/review-v2 --depth deep` on a single small file to exercise panels and follow-up waves, and one `/review-v2 --depth quick` in a TypeScript and a .NET repo with no project config to confirm zero-setup facet attachment.
- Trial: use `/review-v2`, `/optimise-v2` and `/review-plan-v2` in place of v1 on real work, and review `tomlctl review stats --all --calibration` after a handful of runs to tune lenses, facets and compose rules.
- Follow-ons: a native v2 apply flow built for cleanup batches; the recall-eval harness (R11); Haiku clerks for v1 `/implement` and the apply flows (Exploration Notes); lumina adopting catalogue lenses where its research lenses overlap.

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
