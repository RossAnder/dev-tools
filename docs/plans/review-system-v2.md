# Plan: Review System v2 — verified, routed, measurable review alongside v1

**Plan path**: `docs/plans/review-system-v2.md`
**Created**: 2026-10-09

## Summary

Build a complete second review system next to the untouched v1 (`/review`, `/optimise`, `/review-plan`), so the two can be A/B tested on the same scopes. v2 has three thin carriers (`/review-v2`, `/optimise-v2`, `/review-plan-v2`) over one shared engine skill and one lens catalogue. A new `tomlctl review` verb group owns a typed v2 store that holds runs, findings and verdicts.

The redesign follows from our own ledgers and from current practice:
- **Verification**: every critical and warning finding gets an independent fresh-context verifier (CONFIRMED / PLAUSIBLE / REFUTED, cited either way). This replaces the sampled anchor check.
- **Failure scenarios**: every finding at warning or above must carry a concrete `failure_scenario`. This kills the top drop class, confirmations reported as findings.
- **Budgets**: the "target 15 per lens" quota becomes a ceiling plus a suggestion cap.
- **Routing**: lenses are routed by a change profile and run at three depths.
- **Measurement**: every run is recorded, so `review stats` and `review compare` can measure lens precision and diff v2 against v1.
- **Apply path**: v1's apply flows still apply v2 findings through `review export-v1`.

Orchestration runs as a saved Workflow (schema-validated find→verify pipeline), with an Agent-tool fallback. The plan has 30 tasks in 4 milestone checkpoints:
- A: the Rust store and verbs.
- B: the method, catalogue (19 lenses plus a project pack), engine, agents and workflow.
- C: `/review-v2`.
- D: `/optimise-v2`, `/review-plan-v2`, gates and docs.

Scrutinise first: Approach, "Finding contract v2" (universal `failure_scenario` at warning and above, and the explicit `same_as` identity rule), "Pipeline, depth and substrate" (cost of per-finding verification, and the asynchronous Workflow return), and "v2 store and the tomlctl review group" (`record` outcome rules and the export-v1 mapping).

## Context

The review commands' lens sets and harness date from April–May 2026 and have only been patched since. The record in Exploration Notes shows a system tuned for volume. Six lenses each target 15 findings. 48–65% of every lens's output is `suggestion`. The most common vet drop is an agent reporting a confirmation as a finding. `/review-plan` merged 794 findings and discarded none. The 94% "fixed" rate measures apply throughput, not precision. No lens owns logic bugs. `[[vet_events]]` are written and never read, and vet lens names do not even match item categories, so nothing measures lens quality or feeds outcomes back. Research (R1–R12) shows current systems doing the opposite: aggressive finders, independent per-finding verification, a concrete failure scenario per finding, yield that follows the diff rather than a quota, and precision measured from outside the reviewer.

The user asked for a drastic overhaul, but wants v1's API and harness files kept intact and v2 built as all-new files with its own tooling surface, so the designs can be compared before either is retired.

Outcome: `/review-v2`, `/optimise-v2` and `/review-plan-v2` run end to end on this repo, persist runs and verified findings in a v2 store, apply through the existing apply flows after export, and can be compared against v1 runs with one command.

## Scope

- **In scope**:
  - the `tomlctl review` verb group and v2 store (Rust, with tests and docs)
  - the v2 engine skill, method skill and lens catalogue (code, perf and plan families, plus a project-local lens pack)
  - four new agents (`review-finder`, `review-finder-lite`, `review-verifier`, `plan-executor`)
  - the saved Workflow script and its deploy entry
  - the three v2 carriers
  - gate-table rows, shared-block registration, glimpse agent attribution for the v2 store, and CLAUDE.md
- **Out of scope**:
  - any edit to v1 carriers, agents, or v1 skills (`research-methods`, `flow-contract-vet-research`, `flow-contract-ledger-schema`, apply contracts)
  - lumina and the lumina-story-blocks plugin
  - glimpse display or triage of the v2 store (a `LedgerKind` variant would break glimpse's exhaustive match)
  - the user-inputs store for v2 (v2 carriers skip the sweep, so v1 keeps sole ownership of `.claude/inputs.toml` records)
  - a native v2 apply flow; perf before/after measurement in `/optimise-apply`
  - the replay recall-eval harness (follow-on plan)
  - retiring v1
- **Affected areas**: `tomlctl/src/review/`, `tomlctl/src/lib.rs`, `tomlctl/src/cli/`, `tomlctl/src/capabilities.rs`, `tomlctl/src/agents/correlate.rs`, `tomlctl/tests/`, `tomlctl/README.md`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `claude/skills/review-v2-engine/`, `claude/skills/review-v2-method/`, `claude/skills/review-v2-lenses/`, `claude/skills/tomlctl/`, `claude/agents/`, `claude/commands/`, `claude/workflows/`, `.claude/review-lenses/`, `scripts/shared-blocks.toml`, `scripts/deploy-claude.ps1`, `CLAUDE.md`

## User Decisions

> User answers are recorded as data.

1. **Staging** (prompted by: Exploration Notes, the ~40-file estimate) — **foundation + all 3 producers. lumina is out of scope.** User's words: "let's try to retain the old api and harness files creating all new files and a v2 review surface in the tooling so we can a/b test and be free design the optimal tooling infrastructure". v1 carriers, agents and skills stay byte-untouched; v2 is all-new files plus a v2 tooling surface. The recall-eval harness is a follow-on plan.
2. **Substrate** (prompted by: R13, R1) — **Workflow + Agent fallback.** Stage prompts and schemas live in the engine skill as data. The carrier runs a Workflow script loaded by `scriptPath` from the skill dir, and falls back to Agent-tool waves when Workflow is unavailable. Interactive gates and store writes stay in the carrier.
3. **Verification** (prompted by: R1, R8, the vet sample sizes in Exploration Notes) — **tiered.** One fresh-context verifier per critical or warning finding (CONFIRMED / PLAUSIBLE / REFUTED, cited either way); suggestions batch-verified per lens. `deep` adds a 3-vote refutation panel for criticals and loop-until-dry passes. The orchestrator audits a sample of verdicts.
4. **/review lenses** (prompted by: Exploration Notes, the no-bug-owner gap and per-lens drop rates; R7) — **re-partition.** core: correctness, design, change-impact, tests. Routed: security (trust boundary), idioms (language/framework). Project: harness-quality for `claude/**` from a project-local lens pack. Depth: quick = core; standard = + routed + diff-only correctness pass; deep = + repeat passes + synthesis.
5. **v2 naming** (prompted by: decision 1) — **parallel `-v2` names:** `/review-v2`, `/optimise-v2`, `/review-plan-v2` over one shared engine.
6. **v2 store** (prompted by: Exploration Notes, the downstream contract) — **new store + v1 export.** A new `tomlctl review` verb group over one v2 store per flow covering all three families: first-class `runs`, lens provenance, `failure_scenario`, structured `fix`, `needs_plan`, verifier verdicts, plus `stats` and `compare` for A/B. `review export-v1` projects accepted findings into the existing ledgers for the untouched `/review-apply` and `/optimise-apply`.
7. **Checkpoint cadence** (prompted by: the early scope check) — **milestones.**
8. **Backlog fold-in** (prompted by: Exploration Notes → Backlog) — **none selected.** No backlog ids are folded.

### Phase 5 outcome
Ran: one `research-deep` agent on the topics decisions 2, 5 and 6 introduced (the tomlctl verb-group idiom, the v2 store, export-v1, Workflow `scriptPath`). Vet: Agent-3 (tomlctl-v2) — 4 sampled (doctor artifacts, bootstrap plan-command gate, skill description budget, `~/.claude-work` copies), 0 dropped, 0 downgraded. Findings are under Research Notes → "Directed research additions".

## Approach

### Design principles

Each principle is grounded in a Research Notes entry or the empirical record in Exploration Notes:

1. **Find widely, verify independently, persist only what survives** (R1, R8). The finders' job is recall. Precision comes from a fresh-context verifier per finding, not from finder self-critique or a sampled anchor check.
2. **A finding is a concrete failure, not an observation** (R2). Every finding at warning or above carries a `failure_scenario`. A clean check belongs in the coverage line and never becomes a finding.
3. **Yield follows the code** (R3). There is no target count. Each lens has a ceiling and a suggestion cap, and an empty return is a normal outcome.
4. **Route, don't broadcast** (R7, R10). Lenses activate from a change profile. Every skipped lens is recorded with its reason; no lens is silently capped.
5. **Every run is data** (R4, R5, R11). Runs, findings, verdicts and dispositions are typed records, so precision proxies, calibration and A/B comparison are queries rather than archaeology.
6. **One engine, three families**. `/review-v2`, `/optimise-v2` and `/review-plan-v2` share every stage. A carrier binds a family, its lens set and its family-specific steps, which is the same idiom `flow-contract-apply-pipeline` already uses for the apply carriers.
7. **v1 stays byte-identical** (decision 1). v2 reuses unchanged v1 skills by invocation (`tomlctl`, `flow-contract-flow-context`, `backlog-capture`, `flow-contract-task-store`, `flow-contract-plan-output-format`, `research-methods`) and never edits them.

### Architecture

```
/review-v2 ─┐                                  ┌─ review-v2-lenses   (catalogue: lens-<id>.md + .claude/review-lenses/*.md)
/optimise-v2 ┼─ review-v2-engine (orchestrator) ┼─ review-v2-method   (agent side: finder/verifier/executor procedure, record)
/review-plan-v2┘        │                       └─ research-methods  (unchanged v1: sources, grades, references)
                        │
        Workflow `review-v2-pipeline` (saved script)  ──or──  Agent-tool waves (fallback)
          Find (review-finder[-lite]) → Verify (review-verifier) → Synthesize
                        │
        tomlctl review  run start | record | run finish | list | show | triage | stats | compare | export-v1 | schema
                        │
        .claude/flows/<slug>/review-v2.toml   or   .claude/review-v2/<scope>.toml
                        │  export-v1
        review-ledger.toml / optimise-findings.toml  →  untouched /review-apply, /optimise-apply
```

### v2 store and the tomlctl review group

**Shape.** This is a bespoke verb group, not generic `items`. A CLI `items add` always stamps a `dedup_id`, a run and its findings could not land in one atomic write, and `clusters`, `orphans` and `items sweep` hardcode the `items` array (Research Notes, directed finding 1). The store follows the `tomlctl/src/tasks/store.rs` idiom: typed structs converted by hand-written `from_toml`/`to_toml`, as `tomlctl/src/tasks/schema.rs` does (tomlctl has no `serde` derive and no schema-generator dependency, and this plan adds none), written through `io::mutate_doc` with an explicit `OnMissing::Create` seed, so it gets lock, sidecar, auto-create and `last_updated` stamping. Unlike `tasks/store.rs`, the review store's `mutate` hands its closure the raw document as well as the typed view: ids are minted with `items::items_append_minted` (`DedupId::Skip`, honours `id_high_water`) and compare-and-set uses `items::compute_apply_mutation_with(…, StalePolicy::Skip)` (as `tomlctl/src/ledgers.rs` `write_guarded` does), and both take the raw `TomlValue`. The typed `Store` round-trips the root `id_high_water` table, because only a removal (generic `tomlctl items remove --array runs|findings`) raises it and any later typed rewrite would otherwise drop it, letting a removed id be minted again. Arrays: `runs` (prefix `U`), `findings` (prefix `F`) and `verdicts` (no ids). None of them is named `items`. A distinct prefix keeps `F12` from ever meaning a v1 `R12`.

**Paths.** With a flow, `--slug <slug>` resolves `.claude/flows/<slug>/review-v2.toml`. Without one, `--scope <scope>` resolves `.claude/review-v2/<scope>.toml`, with both validated by `validate_slug`. There is no new artifact key (it would fail `flow doctor` `artifacts-canonical` on every existing flow, directed finding 4) and no `LedgerKind` variant (it would break glimpse, directed finding 3). The path is derived from the slug the way `tasks.toml` is.

**Records.**

```toml
schema_version = 1
last_updated = 2026-10-09

[[runs]]
id = "U1"
command = "review-v2"            # review-v2 | optimise-v2 | review-plan-v2
family = "code"                  # code | perf | plan
started = "2026-10-09T10:00:00Z"
finished = "2026-10-09T10:14:00Z"
base = "27b7e1a…"                # git rev-parse HEAD at run start
scope = ["claude/commands/review.md"]
depth = "standard"               # quick | standard | deep
substrate = "workflow"           # workflow | agent-waves
lenses = ["correctness", "design", "change-impact", "tests", "idioms"]
skipped_lenses = ["security — no trust-boundary signal in scope"]
catalogue = "a41f…"              # hash of the lens files used, so an A/B delta is attributable to a lens edit
workflow = "3"                   # the Workflow script's SCRIPT_VERSION (absent on agent-waves), so a stale deployed copy is attributable
passes = 1
agents = 14
counts = { generated = 21, confirmed = 6, plausible = 3, refuted = 9, persisted = 9, suppressed = 3 }
audit = { sampled = 3, overturned = 0 }

[[findings]]
id = "F1"
run = "U1"                       # minting run
seen_in = ["U1"]                 # every run that re-found it
family = "code"
lens = "correctness"
file = "tomlctl/src/review/record.rs"
line = 88
symbol = "record::apply"
severity = "warning"             # critical | warning | suggestion
effort = "small"                 # trivial | small | medium | large
summary = "…"
failure_scenario = "inputs/state → wrong output (required at warning and above)"
fix = "the recommended change, sketched"
description = "…"                # optional context and tradeoffs
needs_plan = false
counter = "what would refute this finding"   # required; the verifier's first refutation probe
grade = "high"                   # high | medium | low (research-methods rubric)
evidence = ["…"]
instances = ["…"]; sweep = ["…"]; enumeration = "complete"
depends_on = []; related = []     # related also holds anchor candidates that were not declared the same finding
verdict = "confirmed"            # latest: confirmed | plausible | refuted
anchor = "code|tomlctl/src/review/record.rs|record::apply"   # computed by tomlctl, never authored
status = "open"                  # open | refuted | deferred | dismissed | resolved | exported
first_seen = 2026-10-09
# plan family adds: plan_section, anchor_old, anchor_new (file = plan path, symbol = task ref)
# companions: dismissed→dismiss_reason; deferred→defer_reason+defer_trigger; resolved→resolution+resolved;
#             exported→exported_as (v1 id) + export_ledger

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"                  # verifier | panel | audit | synthesis
verdict = "confirmed"
citation = "tomlctl/src/review/record.rs:88"
rationale = "…"
```

**Verbs.** Flag tables go in `claude/skills/tomlctl/references/review.md`. Every verb except `schema` takes exactly one of `--slug <slug>` or `--scope <scope>` to select the store ("Paths"); `stats` also accepts `--all`. No verb declares a per-command flag that collides with a global output flag (`--limit`, `--get`, `--select`, …); `Cli::command().debug_assert()` in `tomlctl/src/cli/dispatch/tests/output_gate.rs` fails on a collision.
- `review run start --slug|--scope --command --family --depth --base --set-json lenses=… --set-json skipped_lenses=… --set catalogue=… --set substrate=…`: mints `U<n>`, stamps `started`, prints the id (`--get id`).
- `review run finish <U> --set-json counts=… --set-json audit=… --set-json agents=… --set-json passes=… [--set workflow=…]`: stamps `finished`.
- `review record --run <U> --ndjson <path> [--dry-run]`: the one write per run. Each line is one agent-produced finding (the `review schema finding` shape) plus a required `verdicts` array of `review schema verdict` objects. The verb validates each row (required fields; `failure_scenario` required when `severity` is `warning` or `critical`; `plan_section` required for family `plan`), computes `anchor`, resolves identity per the anchor matcher below, and writes findings plus verdicts in a single locked write. Outcomes are evaluated in this order, one per line:
  - `refused`: a row that fails validation is reported with its reason and skipped; the rest of the batch still lands in the single write.
  - `contested`: the row's identity is an open or exported finding and this run REFUTED it. The finding keeps its status, gains the verdict, and is listed in the report — for an exported finding, with the v1 id to defer, so `/review-apply` does not apply a finding the latest verifier refuted.
  - `reopened`: the identity is a refuted finding and this run CONFIRMED it; it returns to `open`.
  - `regression`: the identity is a resolved finding; a new id is minted with `related = [old]`.
  - `repeat`: the identity is an open or exported finding (CONFIRMED or PLAUSIBLE this run); the run is appended to `seen_in` once, and `line`, `evidence` and `description` are refreshed.
  - `suppressed`: the identity is a dismissed or deferred finding (any verdict), or a refuted finding this run did not CONFIRM; it is reported and not minted.
  - `minted`: no identity; a new `F<n>`, with any anchor candidates recorded under `related`.

  REFUTED rows with no identity are minted as `status = "refuted"`; they are the calibration negatives.
- `review list` / `review show <ids>`: read verbs on the global output options. `list` takes `--family`, `--lens`, `--run` and `--status`, plus the global `--where*`.
- `review triage <ids> --dismiss <reason> | --defer <reason> --trigger <t> | --reopen <reason> | --resolve <resolution> --expect-status <s>`: dispositions with compare-and-set; stale rows are reported under `skipped_stale`. Each prose flag has a `--<flag>-file <path>` twin (`--dismiss-file`, `--defer-file`, `--trigger-file`, `--reopen-file`, `--resolve-file`), mutually exclusive with it — the form carriers use for user-authored text staged with the Write tool.
- `review stats [--slug|--scope|--all] [--family] [--by lens|run] [--calibration [--exemplars N]]`: per-lens runs, generated, refute rate, plausible share, persisted, human dismissal rate, resolved/exported rate, suggestion share and mean yield per run. `--calibration` adds each lens's top-N rejection reasons, taken from `dismiss_reason` and refuted-verdict rationales. `--all` reads every `.claude/flows/*/review-v2.toml` and `.claude/review-v2/*.toml`.
- `review compare --run <A> (--run <B> | --v1 <ledger> [--v1-since <date>] [--v1-ids <ids>])`: aligns findings by `anchor` candidates into `both`, `only_a` and `only_b`, with lens, severity and status per side, plus per-lens overlap counts. A v1 `review-ledger.toml` maps to family `code` and `optimise-findings.toml` to `perf`. Alignment uses the anchor-candidate rule below, with an exact `summary` breaking ties between several candidates; it never merges stored findings.
- `review export-v1 <ids> --to <v1-ledger> [--dry-run]`: see "Export to v1".
- `review schema finding|verdict|return`: prints a JSON Schema. `finding` is the finder-authored record (no `id`, `anchor`, `status`, `run`, dates or `verdicts`); `verdict` is one verifier verdict; `return` is a finder's whole output, `{findings: [<finding>], coverage, escalate?}`, the object a Workflow `agent()` call validates. The schemas are hand-built from the same `const` field-name, enum and required-field lists `record` validates against, and a unit test asserts the two agree, so the Workflow `schema`, the fallback prompt contract and the store's validation share one source.

**Anchor matcher** (directed finding 7). The anchor proposes identity candidates; it never decides identity on its own. Different defects in one function, one task or one 20-line span are common — 161 of 1,039 symbol-bearing v1 items share `(file, symbol)` with a distinct defect — and in the plan family `symbol` is the task ref. Within `family`, the candidates are the findings matching `(file, symbol)` when `symbol` is non-empty, otherwise `(file, line)` within ±10 lines (the tier-C candidate window, which never auto-merges). A row matches a stored finding only when it declares `same_as = "F<n>"` naming one of its candidates: finders see prior findings in the `PRIOR` fence and verifiers confirm the claim. A row with candidates but no `same_as` is minted with the candidates under `related`, and rows from one run are never collapsed by anchor alone. `lens` is not part of the key, because routing changes which lens surfaces an issue. `dedup_id` is not used: it hashes the summary verbatim, so a reworded LLM summary splits.

**Export to v1** (directed finding 8). `export-v1` is the bridge to the untouched apply flows. Family `code` goes to `review-ledger.toml` with prefix `R`; family `perf` goes to `optimise-findings.toml` with prefix `O`. The verb validates the v1 required-field contract itself, because tomlctl's CLI write path checks only `id` and apply skips malformed rows silently.
- **Category table**, keyed by family (the optimise vocabulary is closed: `memory`, `serialization`, `query`, `algorithm`, `concurrency`):

  | family | v2 lens | v1 category |
  |---|---|---|
  | code | correctness, idioms | `quality` |
  | code | design, synthesis | `architecture` |
  | code | change-impact | `completeness` |
  | code | tests | `testability` |
  | code | security | `security` |
  | code | harness-quality | `package-quality` |
  | perf | memory-runtime | `memory` |
  | perf | data-shape | `serialization` |
  | perf | data-access | `query` |
  | perf | algorithm, io-process | `algorithm` |
  | perf | concurrency, synthesis | `concurrency` |

  A project-pack lens other than `harness-quality` maps through its own `**V1 category**:` header line.
- **Description**: the projected `description` concatenates `failure_scenario`, `fix` and `description`, because apply reads the recommended change from `description`.
- **Refused**: family `plan` (no v1 apply target), a lens with no category for the finding's family, `needs_plan`, `effort = "large"` (no v1 slot), and any status other than open/confirmed or open/plausible. Each refusal is reported with its reason; `needs_plan` findings route to `/plan-new`.
- **Matching**: before the v1 merge rule, export looks up `v2_ref` across v1 rows of every status, so a re-export after a failed v2 write — even one that follows an apply — reuses the recorded row. Otherwise the v1 merge rule applies: an open v1 match reuses the v1 id and bumps `rounds` only when this `v2_ref` is not already recorded; a fixed or applied match gets a new id with `related`; a deferred, wontfix, wontapply or verified-clean match is skipped.
- **Written fields**: `v2_ref = "<store path>#F<n>"` on the v1 row, and `exported_as` plus `export_ledger` on the v2 finding.
- **`last_updated`**: the v1 ledger's root `last_updated`, which the apply freshness gate compares against `git log`, becomes the later of its current value and the newest run date among the exported findings' `seen_in` runs; a newly created v1 ledger is seeded with that date. Stamping today would mask code changed since the run's `base`, and leaving it unstamped would force spurious stale prompts.
- **Write order**: two sequential `mutate_doc` calls, never nested. The v1 ledger write commits and returns before the v2 store is opened, so a crash between them converges on re-export through the `v2_ref` lookup.

### Finding contract v2

Finders return the `review schema return` envelope, `{findings: [<finding>], coverage, escalate?}`: `coverage` is the lens's coverage line (clean checks, sweeps run, search log) and `escalate` carries a `review-finder-lite` `ESCALATE-TO-DEEP` reason. Each finding is the `review schema finding` shape; verifiers add verdicts, and the carrier stages each finding plus its `verdicts` as one `record` line. Ids, `anchor`, `status`, `run` and dates are assigned by tomlctl.
- **Identity**: a finding that re-finds one already in the `PRIOR` fence declares `same_as = "F<n>"`; the verifier confirms or strikes the claim. Without it, `record` never treats two findings as one (Approach, "v2 store and the tomlctl review group", **Anchor matcher**).
- **`counter`** and **`grade`**: every finding carries the research-methods Counter line as `counter` (what would refute it — the verifier's first probe) and its evidence grade as `grade` (`high` | `medium` | `low`). The `research-read-only` block's instruction to report findings graded with a Counter line is satisfied by these two fields.
- **Severity rubric**, shared by all families with a per-family calibration line in each lens file:
  - `critical`: data loss, a security hole, shipping broken, or a plan that cannot be executed as written.
  - `warning`: a real defect or footgun with a nameable trigger.
  - `suggestion`: a change that improves the code without changing behaviour.
- **`failure_scenario`**, required at `warning` and above:
  - code: concrete inputs or state → the wrong output, crash or exploit.
  - perf: workload and input size → the cost, with magnitude.
  - plan: the execution step → how it fails.

  A design or architecture finding that cannot state a consequence chain is a `suggestion`. `tomlctl review record` enforces this mechanically.
- **Never a finding**: a confirmation ("checked X, it's fine"), anything the `LINT BASELINE` already reports, a documented deliberate choice, a restatement, or an item already in the store as dismissed, deferred or refuted (cite it under `related` instead). These go in the coverage line. This is the research-methods bar restated as hard exclusions, with the confirmation class named because it is the top v1 drop cause.
- **`fix`**: the recommended change. **`needs_plan`**: a boolean, set when the fix needs several decisions, ordered steps or dependent parts that move together; breadth alone never sets it.
- **Caps**: each lens prompt carries a ceiling (default 10) and a suggestion cap (quick 2, standard 3, deep 8). There is no target. The words "target" and "quota" do not appear in any v2 prompt.

### Lens catalogue

`claude/skills/review-v2-lenses/` holds one `references/lens-<id>.md` per lens. Every lens file opens with this header block:

```
**Id**: correctness
**Family**: code            (code | perf | plan | any)
**Tier**: deep              (deep → review-finder, lite → review-finder-lite, executor → plan-executor)
**Activation**: always      (always | signals: <names from the catalogue's signal vocabulary>)
**Depth**: quick            (lowest depth that runs it: quick | standard | deep)
**Requires scenario**: yes
```

The sections follow in a fixed order: **Question** (the one question this lens answers), **Owns**, **Does not flag** (seeded from the rejection clusters listed in Approach, "Calibration and A/B"), **Probes** (the checklist), **Severity calibration**, **Evidence ladder**, and **Research domain** (for scholarly eligibility). The catalogue `SKILL.md` is the single owner of every lens's header values: it indexes every lens in one table, `| `<id>` | family | tier | activation | depth |`, and defines the **signal vocabulary** — each routing signal's name and one-line meaning, the only names a lens's `**Activation**: signals:` line and the engine's `references/routing.md` may use. Lens files and the routing reference are written against it, never ahead of it. It also documents project lens packs: any `.claude/review-lenses/*.md` in the target repo with the same header plus `**Paths**: <glob>` and `**V1 category**: <category>`, activated when the scope intersects the glob. This repo's v1 package-quality lens moves there as `harness-quality`.

Lens sets:
- **code**:
  - core: `correctness` (new; logic, edge cases, error paths, state and ordering), `design` (architecture, layering, type design, domain and persistence modelling, and v1's `db`), `change-impact` (ripple sweep: callers, prose, config, literals; lite), `tests` (lite).
  - routed: `security` (trust-boundary signal; hard cap 5), `idioms` (language and framework idioms and consistency with the converged codebase pattern; standard and up).
  - The `correctness` lens also runs a second, context-free pass at standard and above: it sees the diff only, with no context pack (R7, the Anthropic split).
- **perf**: `memory-runtime`, `data-shape`, `data-access` (routed: a query or ORM layer is present), `algorithm`, `concurrency` (routed: async, threads or locks), `io-process` (new; process start, syscalls, filesystem walks, subprocess fan-out; routed on CLI or process-heavy scope).
- **plan**:
  - `plan-grounding`: paths, symbols, line anchors and behavioural premises measured against the tree, which is v1 completeness sweeps 4–5 plus alignment.
  - `plan-completeness`: rename and removal ripple, registration seams, justification consumers.
  - `plan-dependencies`: the edge set, acceptance-reachability and checkpoint placement, given the `STORE CHECK`.
  - `plan-executor` (new): a dry run per task (R9).
  - `plan-risk` (routed: the plan adds, upgrades or calls external dependencies or APIs).
  - `plan-premortem` (deep only, labelled experimental, R9's weak evidence).
- **any**: `synthesis`, which runs after verification at standard and above. It reads the verified set and the scope and reports only emergent cross-lens defects plus "what did every lens miss". Its own findings are verified like any other.

### Pipeline, depth and substrate

`claude/skills/review-v2-engine/` is the orchestrator contract every v2 carrier invokes. Its stages:

0. **Pre-flight**: require `review_record` in `tomlctl capabilities` `.features`, else halt with the `cargo install --path tomlctl` hint — bootstrap only gates tomlctl ≥0.15, and a pre-0.16 binary would otherwise run scope, context pack and baselines before failing at stage 4. Then invoke the `tomlctl` and `flow-contract-flow-context` skills, then build the envelope with the **v1 command name** (`--command review|optimise|review-plan`). This reuses bootstrap unchanged, including its plansDirectory read for `review-plan`, with no `VALID_COMMANDS` change (directed finding 5). Bind `--slug` or `--scope` for the store; every `tomlctl review` call below passes it.
1. **Scope**: the paths, globs, `a..b` branch range or git-derived scope, with the v1 safety invariant (no `..`, no absolute paths outside the worktree, glob breadth ≤200). Classify files and record `base`.
2. **Context pack**: one preamble shared by every lens prompt. When a flow is bound, every finder, verifier and executor prompt opens with the column-0 line `ledger: .claude/flows/<slug>/review-v2.toml`, which `tomlctl/src/agents/correlate.rs` reads to attribute the agent to the flow. The preamble is built from these fences:
   - `CONSTRAINTS`: a CLAUDE.md excerpt.
   - `PRIOR`: open, deferred, dismissed and refuted v2 findings with reasons and ids, from `tomlctl review list`. This is how finders read the v2 store — never through `tomlctl items list`, which sees no `items` array in it and returns `[]`.
   - `BACKLOG`: rows via the `backlog-capture` skill.
   - Family baselines: `LINT BASELINE` (code), `PROFILE BASELINE` (perf), `STORE CHECK` and `FORMAT CONTRACT` (plan). Baselines run through the `verification` agent, never inline, per CLAUDE.md build discipline.
   - `DEV SERVER`: a probe only.
   - `CALIBRATION`: `tomlctl review stats --calibration`, per routed lens.

   An absent fence always says why (`not run — <reason>`). The preamble is shared for consistency only. Prompt-cache hits between parallel Agent dispatches do not happen (R13); only the Workflow fan-out stagger yields them.
3. **Route**: evaluate each catalogue lens's activation against the change-profile signals in `references/routing.md` (signal → cheap grep or classification test, using only the catalogue's signal vocabulary). Pick lenses by depth, record `lenses` plus `skipped_lenses` with reasons, and compute the catalogue hash (sha256 over the sorted concatenation of the lens files used) per the catalogue `SKILL.md`.
4. **`tomlctl review run start`**, passing `--set catalogue=<hash>` and `--set substrate=…`.
5. **Find → Verify → Synthesize**: the saved Workflow `review-v2-pipeline`, or the agent-wave fallback. The Workflow call returns at once (see **Substrate**); the carrier ends its turn and resumes at stage 6 when the task-completion notification delivers the result.
6. **Audit**: the orchestrator re-checks a sample of verdicts: every REFUTED critical (guarding against recall loss) plus two CONFIRMED, chosen at random by index. It reads the citation and records overturned verdicts as `by = "audit"`.
7. **`tomlctl review record`** with the NDJSON staged by the Write tool. The per-line outcomes drive the report.
8. **`tomlctl review run finish`** with counts, audit, agents and passes, plus `--set workflow=<SCRIPT_VERSION>` on the Workflow substrate.
9. **Report and dispositions**: severity-grouped, with **Needs a plan**, **Contested** and **Suppressed** groups, a routing line, a coverage digest and the run id. The disposition grammar is `dismiss F3 — reason`, `defer F3 — reason — trigger`, `reopen F3 — reason`, `export F1,F4` (followed by a `/review-apply` or `/optimise-apply` suggestion naming the minted v1 ids), and `plan F5` (a `/plan-new` suggestion). Disposition prose is staged with the Write tool and passed through the `triage` `--<flag>-file` twins. Out-of-scope observations and `TANGENTIAL:` lines go through the `backlog-capture` skill's check-then-add gate. Close with the A/B hint: the exact `tomlctl review compare` command for this run, including its `--slug` or `--scope`.

Depth policy (`--depth`, default `standard`):

| | quick | standard | deep |
|---|---|---|---|
| lenses | core | core + routed + project packs | every applicable lens |
| context-free correctness pass (code) | — | yes | yes |
| verification | one verifier per critical or warning | the same, plus one batched verifier per lens for suggestions | the same, plus a 3-vote refutation panel per critical (survives on ≥2 non-refuted) |
| passes | 1 | 1 | loop-until-dry, max 3, permuted file order, dedup against every finding seen so far (R6) |
| synthesis | — | yes | yes |
| suggestion cap per lens | 2 | 3 | 8 |

**Substrate** (decision 2). The carrier calls `Workflow({name: "review-v2-pipeline", args})`. Here `args` = `{depth, preamble, files: [path], lenses: [{id, agentType, prompt}], executor_tasks: [{task, prompt}], verifier: {agentType: "review-verifier", prompt, suggestion_batch_prompt}, synthesis, schemas: {return, verdict}}`, with the schemas taken from `tomlctl review schema return|verdict`. `files` is the scope file list the deep pass loop permutes (the script has no filesystem); `executor_tasks` is the `/review-plan-v2` per-task dry-run list, empty for the other families. The script uses `pipeline()`, so each lens's findings enter verification as soon as that lens returns. It needs barriers only for synthesis and for the deep pass loop's dedup. A 3-vote panel returns three `by = "panel"` verdicts. The runtime runs at most 16 agents concurrently (queueing the rest) and warns above 25 agents per run, which standard runs with verifiers routinely pass.

The call returns at once with `status: "async_launched"`; check `error`, because a script that fails its syntax check reports it there and never runs. The task-completion notification later delivers `{findings (each with its verdicts), refuted, coverage, routing_echo, script_version}`, where `refuted` lists the findings every verdict refuted. The script has no filesystem access: every write is the carrier's.

This refines the decision's `scriptPath` wording. A saved workflow under `$CLAUDE_CONFIG_DIR/workflows/` is the documented distribution route, while a `scriptPath` into a skill dir is readable only inside this repo (directed finding 11). `claude/workflows/review-v2.js` is deployed by `scripts/deploy-claude.ps1`. Its `meta.name` is `review-v2-pipeline`, not `review-v2`: a saved workflow registers as the slash command `/<meta.name>`, and `review-v2` would compete with the `/review-v2` carrier.

**Fallback.** When Workflow is disabled or unavailable, `name` is unknown, the launch reports `error`, or the run completes with no result, `references/agent-waves.md` runs the same stages as Agent-tool waves:
1. Finders in waves of at most 16 per message.
2. The orchestrator checks each return against `tomlctl review schema return` by reading it, and re-dispatches a malformed return once.
3. Verifiers in waves of at most 16 per message, waiting for each wave, because the Agent tool refuses spawns past 20 concurrent subagents (`Concurrent subagent limit reached`, not retried); per-task `plan-executor` dispatches follow the same cap.
4. Synthesis.

The run records `substrate`, so A/B stats can separate the two. Every dispatch is one-shot, and no `name:` is ever passed.

### Agents

The v2 agents use the frontmatter `skills:` preload (R13), so the method is loaded at start rather than by an in-body `Skill` call. A preload injects a skill's `SKILL.md`, not its `references/` files, so each procedure an agent needs lives in `review-v2-method`'s `SKILL.md` itself. Each body still says "if `review-v2-method` is not loaded, invoke it first", as a guard for clients that ignore the field, and loads reference files only through the Skill tool, never by a guessed path. Each agent carries the existing `research-read-only` and `research-delivering` shared blocks byte-identically, registered as extra carriers in `scripts/shared-blocks.toml`. No block text changes, so v1 agents are untouched.
- **`review-finder`**: `effort: high`. Preloads `review-v2-method` and `research-methods`, with the research-deep tool list. Runs one lens per dispatch and returns the `review schema return` envelope.
- **`review-finder-lite`**: the same, at `effort: medium`, for lite-tier lenses. It may set `escalate` (`ESCALATE-TO-DEEP`), which the engine honours by re-running the lens on `review-finder`.
- **`review-verifier`**: `effort: high`, fresh context, refute-first. Given one finding (or a batch of suggestions from one lens), it tries to break the claim, starting from the finding's `counter`: it re-reads the anchor, reproduces the trigger, looks for intent (comment, `git log -S`, ADR, test) and checks value, and confirms or strikes any `same_as` claim. It returns CONFIRMED (trigger nameable) / PLAUSIBLE (mechanism real, trigger uncertain) / REFUTED, and must cite a `file:line` or command output for any verdict. It never concedes on an uncited assertion (R8).
- **`plan-executor`**: `effort: medium`, read-only tools plus `Skill`. It is deliberately given only what an `implement-*` agent gets: the plan preamble sections plus one task. It returns the decisions it had to invent, the files it would touch compared with the **Files** line, the acceptance commands it could not run as written, and the premises it could not confirm. Each gap becomes a `plan-executor` finding with a built-in failure scenario.

### Family specifics

- **`/review-v2`** (family `code`): the lint baseline, the code lens set plus `.claude/review-lenses/` packs, and export to `review-ledger.toml`.
- **`/optimise-v2`** (family `perf`):
  - **Focal Points Brief**: as v1, but written fresh.
  - **`PROFILE BASELINE`** (R10): the `verification` agent runs the project's declared bench or profile commands (CLAUDE.md `## Optimization Focus`, a `bench:` key under `## Verification Commands`, or existing `benches/`). With none, the fence says so, and every finding without a measurement, or a complexity argument tied to a real input size, is a `low — hypothesis` capped at `suggestion`.
  - **Severity**: a finding at warning or above must cite an on-path measurement or that complexity argument.
  - **Cross-cutting concurrency**: v1's cross-cutting concurrency review becomes part of the `synthesis` prompt for perf.
  - **Export** goes to `optimise-findings.toml`.
- **`/review-plan-v2`** (family `plan`):
  - **Setup**: plan resolution as v1 Step 1, written fresh. Task-store import and `tomlctl tasks check --plan` go into the `STORE CHECK` fence via the `flow-contract-task-store` skill, and the `FORMAT CONTRACT` fence comes via the `flow-contract-plan-output-format` skill.
  - **Orchestrator-only checks**: v1 Step 2.6's four checks (break-set baselines, cross-task contradiction, falsifiability, and the two-control probe per that skill's `references/acceptance-probing.md`). Their findings go through `record` like lens findings.
  - **`plan-executor`**: dispatched once per task, as a pipeline stage in the workflow. At standard it covers up to 12 tasks, preferring L and M tasks and those named in a checkpoint marker; at deep, every task.
  - **Merge offer**: only verified findings (confirmed or plausible) are eligible. The default severities are critical and warning; suggestions are advisory and never merged by default. Manual (judgement) and Mechanical (`anchor_old`/`anchor_new`) modes keep v1's safety rules: the empty-answer rule, the `.premerge.md` backup, and the merge-exit ref-set gate → import → check. The report states plan growth in lines added. Merged findings transition to `resolved` with `resolution = "merged into <plan>"`.

### Calibration and A/B

- **Calibration**: the `CALIBRATION` fence turns the store's own outcomes into per-lens suppressions (R5). Each lens file's **Does not flag** section seeds it before any v2 data exists, written from the v1 rejection clusters: confirmations, stale-comment trust, design intent and ADRs, speculative and style-only, out of scope, and negligible gain against risk.
- **A/B protocol** (`references/ab-testing.md`):
  1. Run `/review <scope>` and `/review-v2 <scope>` on the same `base`.
  2. Run `tomlctl review compare --run U<n> --v1 <ledger> --v1-since <date>`.
  3. Judge a blinded sample of `only_a` and `only_b` (the user, or a strong model that saw neither run), recording outcomes as `triage` reasons.
- **Metrics**:
  - refute rate and human dismissal rate per lens
  - unique true findings per system
  - suggestion share
  - cost: `agents` and wall time from the run record
  - v2 against v2 across `catalogue` hashes after a lens edit

### Coexistence with v1

- Every v1 file listed under Scope → Out of scope stays byte-identical, guarded by a `git diff --quiet` success criterion.
- v2 never writes a v1 ledger except through `export-v1`.
- v2 skips the user-inputs sweep.
- v2 shares the backlog store through the unchanged `backlog-capture` contract.
- A `.claude/inputs.toml` request aimed at a review ledger stays v1's.

## Success Criteria

- guard: v1 review harness files, and the v1 skills and agents v2 reuses or leaves out of scope, are byte-unchanged against the plan's base commit: `git diff --quiet 27b7e1a -- claude/commands/review.md claude/commands/optimise.md claude/commands/review-plan.md claude/commands/review-apply.md claude/commands/optimise-apply.md claude/agents/research-deep.md claude/agents/research-lite.md claude/agents/implement-deep.md claude/agents/implement-lite.md claude/agents/flow-bootstrap.md claude/agents/verification.md claude/skills/research-methods claude/skills/flow-contract-vet-research claude/skills/flow-contract-ledger-schema claude/skills/flow-contract-apply-pipeline claude/skills/flow-contract-apply-constraints claude/skills/flow-contract-apply-dependency-sort claude/skills/flow-contract-apply-rollback-protocol claude/skills/flow-contract-apply-vet-implement-lite claude/skills/flow-contract-flow-context claude/skills/backlog-capture claude/skills/flow-contract-task-store claude/skills/flow-contract-plan-output-format` exits 0 (today: exits 0).
- forward: all three v2 carriers exist and invoke the engine skill: `test "$(grep -l '`review-v2-engine` skill' claude/commands/review-v2.md claude/commands/optimise-v2.md claude/commands/review-plan-v2.md 2>/dev/null | wc -l)" -eq 3` exits 0 (today: exits 1).
- forward: every lens the catalogue indexes has a lens file, and the index holds at least 19 lenses: `n=$(grep -cE '^\| `[a-z-]+` \| (code|perf|plan|any) \|' claude/skills/review-v2-lenses/SKILL.md 2>/dev/null); m=$(ls claude/skills/review-v2-lenses/references/lens-*.md 2>/dev/null | wc -l); test "${n:-0}" -ge 19 && test "$n" -eq "$m"` exits 0 (today: exits 1).
- forward: the v2 agents are registered as shared-block carriers, and the gate still passes: `test "$(grep -cE 'review-finder\.md|review-finder-lite\.md|review-verifier\.md|plan-executor\.md' scripts/shared-blocks.toml)" -ge 4 && bash scripts/verify-shared-blocks.sh >/dev/null` exits 0 (today: exits 1).
- forward: the v2 carriers are in the skill-invocation gate table: `test "$(grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs)" -eq 3` exits 0 (today: exits 1).
- forward: the `tomlctl review` suite passes, including `record`'s refusal of a warning row without `failure_scenario` — **predicted, unverified** (builds `target/`): `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -E 'binary(/^review_/)'`.
- forward: the store and matcher unit tests pass, including two distinct defects on one symbol minting two findings — **predicted, unverified** (builds `target/`): `cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::`.
- forward: an end-to-end `/review-v2 --depth quick` on `claude/commands/commit.md` records one run whose `skipped_lenses` names the routed lenses with reasons and whose findings carry verdicts, and `tomlctl review compare --scope <scope> --run U1 --v1 .claude/reviews/<scope>.toml` prints `both`/`only_a`/`only_b` buckets — **predicted, unverified** (needs the installed binary and a live session; see After Merge).
- forward: `/optimise-v2 --depth quick` on `tomlctl/src/sweep.rs` and `/review-plan-v2 --depth quick` on this plan each record one run in their store — **predicted, unverified** (live session).
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
success: n=$(grep -cE '^\| `[a-z-]+` \| (code|perf|plan|any) \|' claude/skills/review-v2-lenses/SKILL.md 2>/dev/null); m=$(ls claude/skills/review-v2-lenses/references/lens-*.md 2>/dev/null | wc -l); test "${n:-0}" -ge 19 && test "$n" -eq "$m"
success: test "$(grep -cE 'review-finder\.md|review-finder-lite\.md|review-verifier\.md|plan-executor\.md' scripts/shared-blocks.toml)" -ge 4 && bash scripts/verify-shared-blocks.sh >/dev/null
success: test "$(grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs)" -eq 3
success: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -E 'binary(/^review_/)'
success: cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::
success: cargo clippy --manifest-path glimpse/Cargo.toml --locked
```

The pre-commit hook adds the crate-wide `cargo fmt --check`, the `cli::dispatch::tests` gate subset (any staged `claude/**/*.md` or `tomlctl/src/**`), and glimpse clippy `--locked` on any tomlctl `src` change. Run `cargo tree --manifest-path glimpse/Cargo.toml >/dev/null` after the version bump to refresh `glimpse/Cargo.lock`.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 9, 11, 12, 14, 15, 16, 17, 18, 19, 20, 21, 22, 24, 25, 26, 29
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### 1. Add the v2 review store schema and anchor matcher [L]
- **Files**: `tomlctl/src/review/mod.rs` (new), `tomlctl/src/review/store.rs` (new), `tomlctl/src/review/matching.rs` (new), `tomlctl/src/lib.rs`
- **Depends on**: —
- **Action**: Create the `review` module with typed `Run`, `Finding`, `Verdict` and `FindingInput` structs, the slug/scope path resolver, the `mutate`/`read` helpers and the anchor-candidate matcher; register `mod review;` in `tomlctl/src/lib.rs`.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group" and Approach, "Finding contract v2".
  - Structs convert through hand-written `from_toml`/`to_toml`, as `tomlctl/src/tasks/schema.rs` does. Add no dependency: tomlctl has no `serde` derive, and a dependency change would need both lockfiles in this task to pass the `--locked` glimpse clippy hook.
  - Model `store.rs` on `tomlctl/src/tasks/store.rs` (`mutate` at its line 62 delegates to `io::mutate_doc`), but have `mutate` hand its closure the raw `TomlValue` as well as the typed view, because the minting and compare-and-set helpers take the raw document. Pass `OnMissing::Create` with a seed holding `schema_version = 1` plus `last_updated`, as `tomlctl/src/inputs.rs` builds its seed. Paths are `.claude/flows/<slug>/review-v2.toml` and `.claude/review-v2/<scope>.toml`, both validated by `crate::validate_slug`.
  - `Store` round-trips the root `id_high_water` table. Mint ids through `items::items_append_minted` (`tomlctl/src/items.rs:1950`) with prefixes `U` and `F`, called on the raw doc inside the `mutate_doc` closure.
  - `FindingInput` is the finder-authored record: every field the Records block shows except `id`, `run`, `seen_in`, `anchor`, `status`, `verdict`, `first_seen` and the status companions, plus the optional `same_as`. Define its field names, enums (`family`, `severity`, `effort`, `grade`, verdict values) and required-field lists as `const`s that task 3's schema emitter and task 4's validation both read.
  - `matching.rs` exposes `anchor(&FindingInput) -> String` and `candidates(&[Finding], &FindingInput) -> Vec<usize>`: within the family, `(file, symbol)` when symbol is non-empty, else `(file, line)` within ±10. It never decides identity (that is `same_as`, task 4). Do not reuse `tomlctl/src/anchor.rs` or `tomlctl/src/dedup.rs`.
  - Status companions per the Approach record block.
  - Unit tests in-module: `round_trips_runs_findings_verdicts`, `round_trips_id_high_water`, `candidates_prefer_symbol_over_line_window`, `candidates_within_ten_lines`, `candidates_partition_by_family`.
- **Acceptance**:
  - forward: `grep -c '^mod review;' tomlctl/src/lib.rs` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::` passes the five named tests — **predicted, unverified**.
  - falsifier: widening the line window check in `candidates` to unbounded turns `candidates_within_ten_lines`'s far-line case red — **predicted, unverified**.

### 2. Wire the review verb group into the CLI with stub verbs [L]
- **Files**: `tomlctl/src/cli/types/review.rs` (new), `tomlctl/src/cli/types/mod.rs`, `tomlctl/src/cli/types/cmd.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/review/mod.rs`, `tomlctl/src/review/dispatch.rs` (new), `tomlctl/src/review/runs.rs` (new), `tomlctl/src/review/record.rs` (new), `tomlctl/src/review/query.rs` (new), `tomlctl/src/review/triage.rs` (new), `tomlctl/src/review/stats.rs` (new), `tomlctl/src/review/compare.rs` (new), `tomlctl/src/review/export.rs` (new), `tomlctl/src/review/schema_out.rs` (new), `tomlctl/src/capabilities.rs`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 1
- **Action**: Declare the full `ReviewOp` clap tree with every verb and flag the Approach lists, add `Cmd::Review`, route it through `tomlctl/src/review/dispatch.rs` to one function per verb module, declare the nine new modules in `tomlctl/src/review/mod.rs`, and register capabilities features.
- **Detail**: Follow Approach, "v2 store and the tomlctl review group".
  - Copy the `inputs` and `backlog` idiom: `ReviewOp` re-exported from `tomlctl/src/cli/types/mod.rs`; `ReadIntegrityArgs` flattened on reads; `WriteIntegrityArgs` + `StampArgs` on writes.
  - Every verb except `schema` takes `--slug` or `--scope` (mutually exclusive, one required); `stats` adds `--all`. Declare the `triage` `--<flag>-file` twins. Declare no per-command flag that shadows a global output flag; `the_command_tree_has_no_duplicate_flags` enforces this.
  - Each verb module gets one `pub(crate) fn run(...) -> Result<()>` that returns a `kind=validation` error `"review <verb>: not implemented"`. Tasks 3–8 replace those bodies; the stubs exist so those tasks stay file-disjoint, which is why this task is L.
  - Add features `review_store`, `review_record`, `review_triage`, `review_stats`, `review_compare`, `review_export_v1`, `review_schema` to `FEATURES`, to the expected list in `tomlctl/tests/capabilities.rs`, to `SUBCOMMANDS`, and to both README transcriptions — the `"features"` array of the `capabilities` sample block and the "Feature meanings" table in `tomlctl/README.md` — which `readme_feature_transcriptions_match_capabilities_features` gates.
  - Emit output only through `crate::output` helpers; `tomlctl/src/cli/dispatch/tests/output_gate.rs` enforces this.
- **Acceptance**:
  - forward: `grep -c 'Review' tomlctl/src/cli/types/cmd.rs` prints at least `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes — **predicted, unverified**.
  - falsifier: deleting `review_store` from the README "Feature meanings" table turns `readme_feature_transcriptions_match_capabilities_features` red — **predicted, unverified**.

### 3. Implement review run start/finish and schema emission [M]
- **Files**: `tomlctl/src/review/runs.rs`, `tomlctl/src/review/schema_out.rs`, `tomlctl/tests/review_runs.rs` (new)
- **Depends on**: 2
- **Action**: Implement `review run start` (mint `U<n>`, stamp `started`) and `review run finish` (stamp `finished`, set counts/audit/agents/passes and the optional `workflow`), plus `review schema finding|verdict|return` printing the JSON Schemas.
- **Detail**:
  - Build each schema by hand in `tomlctl/src/review/schema_out.rs` from the `FindingInput` `const` field, enum and required lists task 1 defines; add no dependency. `finding` lists only finder-authored fields (no `id`, `anchor`, `status`, `run`, dates or `verdicts`), requires `failure_scenario` conditionally on severity through `if`/`then`, and uses enums for `severity`, `effort`, `grade` and `family`. `verdict` is one verdict (`by`, `verdict`, `citation`, `rationale`). `return` is `{findings: [<finding>], coverage, escalate?}` with an object root, the shape a Workflow `agent()` schema needs (Approach, "Finding contract v2").
  - A unit test in `schema_out.rs`, `schema_agrees_with_record_validation`, asserts that every required field in the emitted `finding` schema is one the `const` required list names, and the reverse.
  - Integration tests use the `tomlctl/tests/common/` sandbox (`TOMLCTL_ROOT`): `run_start_mints_sequential_ids`, `run_ids_never_reuse_a_removed_id` (no review verb removes a run, so the test seeds `[id_high_water] U = n` directly), `run_finish_stamps_finished_and_counts`, `schema_finding_requires_scenario_for_warning`, `schema_return_wraps_findings`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_runs.rs && grep -c '#\[test\]' tomlctl/tests/review_runs.rs` prints at least `5` (today: prints nothing, exit 1).
  - falsifier: dropping the `id_high_water` consultation from run minting turns `run_ids_never_reuse_a_removed_id` red — **predicted, unverified**.

### 4. Implement review record with anchor matching outcomes [M]
- **Files**: `tomlctl/src/review/record.rs`, `tomlctl/tests/review_record.rs` (new)
- **Depends on**: 3
- **Action**: Implement `review record --run <U> --ndjson <path> [--dry-run]`: validate each row, compute `anchor` and its candidates, resolve identity through `same_as`, and write findings and their embedded verdicts in one `mutate` call, reporting a per-line outcome (`refused`, `contested`, `reopened`, `regression`, `repeat`, `suppressed`, `minted`).
- **Detail**: Identity and outcome rules are the Approach's, "v2 store and the tomlctl review group" (**Verbs** `review record`, and **Anchor matcher**), evaluated in the order listed there.
  - A row is a `FindingInput` plus `verdicts` (≥ 1). Validation: required `family`, `lens`, `file`, `line` ≥ 0, `severity`, `summary`, `counter`, `grade`, `verdicts`; `failure_scenario` when severity is `warning`/`critical`; `plan_section` when family is `plan`; a `same_as` must name one of the row's anchor candidates. A failing row is `refused` with its reason and the rest of the batch still lands.
  - A row matches a stored finding only through `same_as`. Rows with candidates but no `same_as` are minted with the candidates under `related`; rows in one NDJSON are never collapsed by anchor.
  - REFUTED rows with no identity persist as `status = "refuted"`. A `repeat` refreshes `line`, `evidence` and `description` and appends the run to `seen_in` once. A REFUTED row whose identity is open or exported is `contested`: status kept, verdict appended.
  - Tests: `warning_without_failure_scenario_is_refused_rest_lands`, `distinct_defects_on_one_symbol_both_mint`, `same_as_repeat_appends_run_to_seen_in`, `refind_of_resolved_is_a_regression`, `confirmed_refind_of_refuted_reopens`, `refuted_refind_of_exported_is_contested`, `dismissed_identity_is_suppressed`, `dry_run_writes_nothing`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_record.rs && grep -c 'fn distinct_defects_on_one_symbol_both_mint' tomlctl/tests/review_record.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: matching on the `(file, symbol)` candidate alone, without `same_as`, turns `distinct_defects_on_one_symbol_both_mint` red — **predicted, unverified**.
  - falsifier: accepting a `warning` row with no `failure_scenario` turns `warning_without_failure_scenario_is_refused_rest_lands` red — **predicted, unverified**.

### 5. Implement review list, show and triage [M]
- **Files**: `tomlctl/src/review/query.rs`, `tomlctl/src/review/triage.rs`, `tomlctl/tests/review_triage.rs` (new)
- **Depends on**: 4
- **Action**: Implement `review list` (filters `--family`, `--lens`, `--run`, `--status` as a row report over the global output options) and `review show <ids>`, plus `review triage <ids>` with `--dismiss`/`--defer --trigger`/`--reopen`/`--resolve`, their `--<flag>-file` twins, and `--expect-status` compare-and-set.
- **Detail**:
  - Write the companion fields per the Approach record block, refuse a disposition without its reason, read a `--<flag>-file` value verbatim from the file, and report stale ids under `skipped_stale` without failing the batch.
  - Mirror `tomlctl/src/ledgers.rs` `write_guarded` (line 369) for the guard, on the raw doc `review::store::mutate` exposes.
  - Tests: `triage_dismiss_requires_reason`, `triage_honours_expect_status`, `triage_reads_reason_from_file`, `list_filters_by_lens_and_status`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_triage.rs && grep -c 'fn triage_honours_expect_status' tomlctl/tests/review_triage.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: ignoring `--expect-status` turns `triage_honours_expect_status` red — **predicted, unverified**.

### 6. Implement review stats with calibration exemplars [M]
- **Files**: `tomlctl/src/review/stats.rs`, `tomlctl/tests/review_stats.rs` (new)
- **Depends on**: 4
- **Action**: Implement `review stats [--slug|--scope|--all] [--family] [--by lens|run] [--calibration [--exemplars N]]` with the per-lens metrics the Approach lists.
- **Detail**:
  - Use Approach, "v2 store and the tomlctl review group", and Approach, "Calibration and A/B".
  - `--all` globs `.claude/flows/*/review-v2.toml` and `.claude/review-v2/*.toml` under the repo root.
  - Exemplars are the first sentence of `dismiss_reason` and of refuted verdict `rationale`, grouped per lens, most frequent first, ties broken by recency.
  - Emit a row report with one row per lens or per run.
  - Tests: `stats_refute_rate_per_lens`, `stats_all_aggregates_across_stores`, `calibration_lists_top_reasons`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_stats.rs && grep -c 'fn calibration_lists_top_reasons' tomlctl/tests/review_stats.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: computing refute rate over persisted rows only (excluding `refuted`) turns `stats_refute_rate_per_lens` red — **predicted, unverified**.

### 7. Implement review compare across runs and against v1 ledgers [M]
- **Files**: `tomlctl/src/review/compare.rs`, `tomlctl/tests/review_compare.rs` (new)
- **Depends on**: 4
- **Action**: Implement `review compare --run <A> (--run <B> | --v1 <ledger> [--v1-since <date>] [--v1-ids <ids>])`, aligning by the `tomlctl/src/review/matching.rs` anchor candidates into `both`/`only_a`/`only_b` with per-lens overlap counts.
- **Detail**:
  - Read the v1 ledger's `items` array read-only.
  - Map `review-ledger.toml` items to family `code` and `optimise-findings.toml` to `perf` (by basename; flow-less ledgers by directory `.claude/reviews/` or `.claude/optimise-findings/`).
  - v1 items carry no `lens`, so their `category` stands in on the v1 side.
  - When a finding has several candidates on the other side, an exact `summary` breaks the tie; otherwise the nearest line wins.
  - Tests: `compare_aligns_by_symbol_across_runs`, `compare_v1_ledger_maps_family`, `compare_v1_since_filters_by_first_flagged`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_compare.rs && grep -c 'fn compare_v1_ledger_maps_family' tomlctl/tests/review_compare.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: matching on summary text alone turns `compare_aligns_by_symbol_across_runs` (reworded summary, same symbol) red — **predicted, unverified**.

### 8. Implement review export-v1 [M]
- **Files**: `tomlctl/src/review/export.rs`, `tomlctl/tests/review_export.rs` (new)
- **Depends on**: 4
- **Action**: Implement `review export-v1 <ids> --to <v1-ledger> [--dry-run]` per Approach, "v2 store and the tomlctl review group" → **Export to v1**.
- **Detail**:
  - Validate the projected row against the v1 required set: `id` minted with prefix `R` or `O` by family, `file`, `line`, `severity`, `effort` ∈ trivial/small/medium, `category` from the family-keyed lens table (a project-pack lens through its `**V1 category**:` header), `summary`, `first_flagged`, `rounds`, `status = "open"`.
  - Refuse family `plan`, a lens with no category for the finding's family, `needs_plan`, `effort = "large"`, and non-open or refuted findings, each with a reason row.
  - Look up `v2_ref` across v1 rows of every status before applying the v1 merge rule; then apply the merge rule with `v2_ref` idempotency.
  - Set the v1 ledger's root `last_updated` to the later of its current value and the newest run date among the exported findings' `seen_in` runs; seed a new ledger with that date.
  - Write order: two sequential `mutate_doc` calls, never nested — the v1 ledger write commits before the v2 store is opened.
  - Map `depends_on` F-ids to exported v1 ids, dropping unexported ones into a `dropped_deps` header field.
  - Tests: `export_refuses_needs_plan`, `export_refuses_plan_family`, `export_refuses_unmapped_lens`, `export_maps_lens_to_v1_category`, `export_reuses_open_v1_id_and_bumps_rounds_once`, `export_is_idempotent_on_v2_ref`, `export_reuses_v2_ref_on_fixed_v1_row`, `export_regression_on_fixed_v1_match`, `export_stamps_last_updated_from_run_date`.
- **Acceptance**:
  - forward: `test -f tomlctl/tests/review_export.rs && grep -c 'fn export_is_idempotent_on_v2_ref' tomlctl/tests/review_export.rs` prints `1` (today: prints nothing, exit 1).
  - falsifier: bumping `rounds` on every export call turns `export_reuses_open_v1_id_and_bumps_rounds_once` red — **predicted, unverified**.

### 9. Attribute review-v2 store prompts in agent correlation [S]
- **Files**: `tomlctl/src/agents/correlate.rs`
- **Depends on**: —
- **Action**: Extend the `LEDGER` regex in `tomlctl/src/agents/correlate.rs` (around line 201) to accept the `review-v2.toml` basename, and add a test beside the existing in-module ones.
- **Detail**:
  - Basename only: v2 dispatch prompts carry no F-ids, which `record` mints after every agent returns, so do not add an `F[0-9]+` alternation.
  - Only the flow-local form `.claude/flows/<slug>/review-v2.toml` attributes a flow, matching the v1 behaviour. The prompts that carry the line are tasks 21, 22 and 25's.
  - Test: `ledger_line_attributes_review_v2_store`.
- **Acceptance**:
  - forward: `grep -c 'review-v2' tomlctl/src/agents/correlate.rs` prints at least `1` (today: `0`).
  - falsifier: reverting the alternation turns `ledger_line_attributes_review_v2_store` red — **predicted, unverified**.

### 10. Document the review verb group in the tomlctl skill [M]
- **Files**: `claude/skills/tomlctl/references/review.md` (new), `claude/skills/tomlctl/SKILL.md`, `tomlctl/README.md`
- **Depends on**: 3, 5, 6, 7, 8
- **Action**: Write the `review` flag-table reference, with one `### \`tomlctl review <verb>\`` heading and `| Flag |` table per verb plus the store shape. In `claude/skills/tomlctl/SKILL.md`, add a Quick Reference row, a References entry, and "Rows and header per command" rows for `review list`, `review stats`, `review compare`, `review record` and `review export-v1`. In `tomlctl/README.md`, add `tomlctl review …` lines to the `## Usage` Quick-tour fence.
- **Detail**:
  - Do **not** edit the `description:` of `claude/skills/tomlctl/SKILL.md`: it is at 1010/1024 characters.
  - Every `tomlctl review …` line in a bash fence must parse (`command_lint`), and every flag-table row must be a real flag (`flag_table_lint`). A heading names one verb path only (`### \`tomlctl review schema\``, not `schema finding|verdict|return`), or `flag_table_lint` reports it as no subcommand.
  - Keep the reference under 600 lines.
- **Acceptance**:
  - forward: `test -f claude/skills/tomlctl/references/review.md && grep -cE '^### `tomlctl review ' claude/skills/tomlctl/references/review.md` prints at least `10` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 11. Bump tomlctl to 0.16.0 and refresh the glimpse lockfile [M]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 2, 10, 30
- **Action**: Set `version = "0.16.0"` in `tomlctl/Cargo.toml`, update the `0.15.0` literal and its message in `capabilities_version_matches_cargo_toml` (`tomlctl/tests/capabilities.rs`), update the `"version"` of the `capabilities` sample block in `tomlctl/README.md`, then refresh both lockfiles (`cargo tree --manifest-path glimpse/Cargo.toml >/dev/null` rewrites glimpse's).
- **Detail**: The pre-commit glimpse clippy runs `--locked`, so the refreshed `glimpse/Cargo.lock` must be staged with the bump. `readme_sample_version_matches_cargo_toml` gates the README edit. Tasks 10 and 30 edit `tomlctl/README.md` and `tomlctl/tests/capabilities.rs` too, which is why this task depends on them and closes checkpoint A.
- **Acceptance**:
  - forward: `grep -c '^version = "0.16.0"' tomlctl/Cargo.toml` prints `1` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes, including `capabilities_version_matches_cargo_toml` and `readme_sample_version_matches_cargo_toml` — **predicted, unverified**.

### 30. Add output-option cases and help-list entries for the review verbs [M]
- **Files**: `tomlctl/tests/output_options.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 3, 4, 5, 6, 7, 8
- **Action**: Give every `review` leaf a case in `cases()` of `tomlctl/tests/output_options.rs`, add a `review` arm to `group()` and to the `group_tests!` list, and seed a `review-v2.toml` in `fixture()`. Add the review read verbs to `read_only_subcommands_hide_write_integrity_flags_in_help` and the write verbs to `write_subcommands_expose_all_integrity_flags_in_help` in `tomlctl/tests/capabilities.rs`.
- **Detail**: `every_leaf_command_has_a_case` walks `tomlctl capabilities .commands` and fails on any leaf without a case, so the full suite is red from task 2 until this task lands; it waits for tasks 3–8 because a case against a stub verb fails. The leaves are `run start`, `run finish`, `record`, `list`, `show`, `triage`, `stats`, `compare`, `export-v1` and `schema`.
- **Acceptance**:
  - forward: `grep -c '&\["review", ' tomlctl/tests/output_options.rs` prints at least `10` (today: `0`).
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --test output_options` passes — **predicted, unverified**.
  - falsifier: deleting the `review list` case turns `every_leaf_command_has_a_case` red — **predicted, unverified**.

### 12. Write the review-v2-method skill (agent-side procedure) [M]
- **Files**: `claude/skills/review-v2-method/SKILL.md` (new), `claude/skills/review-v2-method/references/verifying.md` (new), `claude/skills/review-v2-method/references/dry-run.md` (new)
- **Depends on**: 3
- **Action**: Write the agent-side method every v2 agent preloads. `SKILL.md` covers the finder procedure, the `review schema return` envelope and finding record (including `same_as`, `counter` and `grade`), the hard exclusions, the severity rubric, caps, the coverage line, and a short section each for the refute-first verifier procedure (the three verdicts and their citation rule) and the plan-executor dry-run procedure. `references/verifying.md` and `references/dry-run.md` hold worked examples only.
- **Detail**:
  - Follow Approach, "Finding contract v2" and Approach, "Agents". A `skills:` preload injects `SKILL.md` only, so every procedure an agent needs is in it; reference files are loaded with the Skill tool, never by a guessed path.
  - Defer source hierarchy, evidence grades and the gated references to the unchanged `research-methods` skill by name; do not restate them. State that the Counter line and grade go in the record's `counter` and `grade` fields.
  - State that the v2 store is read only through the `PRIOR` fence or `tomlctl review list`, never `tomlctl items list` (which returns `[]` on it), overriding research-methods' "list the named ledger" step for v2 stores.
  - Name the confirmation exclusion explicitly as the top v1 drop class.
  - Never use the words "target" or "quota" for counts.
  - The description must state when to use it (v2 finder, verifier or executor agents) and stay under 1024 characters; the body stays under 500 lines. Every `tomlctl review …` line in a bash fence must parse (`command_lint`).
- **Acceptance**:
  - forward: `test -f claude/skills/review-v2-method/SKILL.md && grep -c 'failure_scenario' claude/skills/review-v2-method/SKILL.md` prints at least `1` (today: prints nothing, exit 1).
  - forward: `test -f claude/skills/review-v2-method/SKILL.md && grep -cE 'CONFIRMED|PLAUSIBLE|REFUTED' claude/skills/review-v2-method/SKILL.md` prints at least `3` (today: prints nothing, exit 1).
  - forward: `test -f claude/skills/review-v2-method/SKILL.md && ! grep -rqiE '\btarget [0-9]+|quota' claude/skills/review-v2-method && echo clean` prints `clean` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 13. Write the review-v2-lenses catalogue index [M]
- **Files**: `claude/skills/review-v2-lenses/SKILL.md` (new)
- **Depends on**: —
- **Action**: Write the catalogue skill: the lens-file header schema and section order; the index table with one `| `<id>` | <family> | <tier> | <activation> | <depth> |` row per lens (19 rows: 6 code, 6 perf, 6 plan, 1 `synthesis`), which is the single source of every lens's tier, activation and depth; the routing signal vocabulary (each signal's name and one-line meaning — the only names a lens's `**Activation**: signals:` line and `claude/skills/review-v2-engine/references/routing.md` may use); the project lens-pack rules (`.claude/review-lenses/*.md`, `**Paths**:` glob activation, `**V1 category**:` export mapping); and how the catalogue hash for `runs.catalogue` is computed (sha256 over the sorted concatenation of the lens files used).
- **Detail**: Follow Approach, "Lens catalogue" and Approach, "Pipeline, depth and substrate" (the depth policy decides which lenses are core). Tasks 14–21 write against this table, so fix every lens's tier and depth and every routed lens's signal here. `harness-quality` is a project pack and is not in the index table. The description stays under 1024 characters and the body under 500 lines; write no other table whose rows open with a backticked lowercase name, so the index count stays exact.
- **Acceptance**: forward: `grep -cE '^\| `[a-z-]+` \| (code|perf|plan|any) \|' claude/skills/review-v2-lenses/SKILL.md 2>/dev/null` prints `19` (today: prints nothing).

### 14. Write the code lenses: correctness, design, change-impact [M]
- **Files**: `claude/skills/review-v2-lenses/references/lens-correctness.md` (new), `claude/skills/review-v2-lenses/references/lens-design.md` (new), `claude/skills/review-v2-lenses/references/lens-change-impact.md` (new)
- **Depends on**: 13
- **Action**: Write three lens files in the catalogue header schema and section order, with header values from the catalogue index.
- **Detail**:
  - Follow Approach, "Lens catalogue".
  - `correctness` owns logic and edge-case defects and the context-free diff pass rules.
  - `design` absorbs v1 architecture, type design and `db` (`claude/commands/review.md` Agents 1 and 3).
  - `change-impact` is lite and sweep-driven (`tomlctl sweep`), and owns rename and removal ripple across code, prose and literals (`claude/commands/review.md` Agent 4).
  - Seed each lens's **Does not flag** section from the rejection clusters listed in Approach, "Calibration and A/B", keeping those that apply to the lens.
- **Acceptance**: forward: `for f in correctness design change-impact; do p=claude/skills/review-v2-lenses/references/lens-$f.md; test -f $p && grep -q '^\*\*Family\*\*: code' $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 15. Write the code lenses: tests, security, idioms [M]
- **Files**: `claude/skills/review-v2-lenses/references/lens-tests.md` (new), `claude/skills/review-v2-lenses/references/lens-security.md` (new), `claude/skills/review-v2-lenses/references/lens-idioms.md` (new)
- **Depends on**: 13
- **Action**: Write three lens files in the catalogue header schema, with header values and signal names from the catalogue index.
- **Detail**:
  - Follow Approach, "Lens catalogue".
  - `security` is routed on a trust-boundary signal, with a hard cap of 5; its advisory lookups cover only dependencies the scope adds or bumps (`claude/commands/review.md` Agent 2).
  - `idioms` decides convergence from history (`git log -S`), so it never canonises an outlier.
  - `tests` is lite and aims at the regression risk the change carries (`claude/commands/review.md` Agent 5).
  - Seed each lens's **Does not flag** section from the rejection clusters listed in Approach, "Calibration and A/B".
- **Acceptance**: forward: `for f in tests security idioms; do p=claude/skills/review-v2-lenses/references/lens-$f.md; test -f $p && grep -q '^\*\*Family\*\*: code' $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 16. Write the perf lenses: memory-runtime, data-shape, data-access [M]
- **Files**: `claude/skills/review-v2-lenses/references/lens-memory-runtime.md` (new), `claude/skills/review-v2-lenses/references/lens-data-shape.md` (new), `claude/skills/review-v2-lenses/references/lens-data-access.md` (new)
- **Depends on**: 13
- **Action**: Write three perf lens files carrying `claude/commands/optimise.md` Agents 1–3's coverage in the catalogue schema, with header values from the catalogue index.
- **Detail**:
  - Follow Approach, "Lens catalogue" and Approach, "Family specifics".
  - Each lens requires a workload → cost `failure_scenario` with magnitude, and caps off-path findings without a `PROFILE BASELINE` measurement at `suggestion`.
  - `data-access` is routed on the catalogue's query/ORM-layer signal.
  - Seed each lens's **Does not flag** section from the rejection clusters listed in Approach, "Calibration and A/B".
- **Acceptance**: forward: `for f in memory-runtime data-shape data-access; do p=claude/skills/review-v2-lenses/references/lens-$f.md; test -f $p && grep -q '^\*\*Family\*\*: perf' $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 17. Write the perf lenses: algorithm, concurrency, io-process [M]
- **Files**: `claude/skills/review-v2-lenses/references/lens-algorithm.md` (new), `claude/skills/review-v2-lenses/references/lens-concurrency.md` (new), `claude/skills/review-v2-lenses/references/lens-io-process.md` (new)
- **Depends on**: 13
- **Action**: Write three perf lens files, with header values and signal names from the catalogue index.
- **Detail**:
  - Follow Approach, "Lens catalogue".
  - `concurrency` keeps `claude/commands/optimise.md` Agent 5's severity calibration and is routed on an async/thread/lock signal.
  - `io-process` is new: process start-up, syscall counts, filesystem walks and subprocess fan-out, routed on CLI or process-heavy scope.
  - `algorithm` carries `claude/commands/optimise.md` Agent 4's coverage and keeps the boundary that type-modelling expressiveness is the `design` lens's concern.
  - Seed each lens's **Does not flag** section from the rejection clusters listed in Approach, "Calibration and A/B".
- **Acceptance**: forward: `for f in algorithm concurrency io-process; do p=claude/skills/review-v2-lenses/references/lens-$f.md; test -f $p && grep -q '^\*\*Family\*\*: perf' $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 18. Write the plan lenses: grounding, completeness, dependencies [M]
- **Files**: `claude/skills/review-v2-lenses/references/lens-plan-grounding.md` (new), `claude/skills/review-v2-lenses/references/lens-plan-completeness.md` (new), `claude/skills/review-v2-lenses/references/lens-plan-dependencies.md` (new)
- **Depends on**: 13
- **Action**: Write three plan lens files, with header values from the catalogue index.
- **Detail**:
  - Follow Approach, "Lens catalogue". The v1 sources are `claude/commands/review-plan.md` Step 2 (Agents 1 and 2).
  - `plan-grounding` carries v1 Agent 2's completeness sweeps 4–5 (re-derived enumerations, behavioural premises measured) plus path, symbol and line alignment.
  - `plan-completeness` carries v1 Agent 2's sweeps 1–3, with one line per sweep even when it finds nothing.
  - `plan-dependencies` carries v1 Agent 1: it reads the `STORE CHECK` fence as ground truth for the stated edges and spends its budget on missing edges, acceptance-reachability and checkpoint placement.
  - Seed each lens's **Does not flag** section from the rejection clusters listed in Approach, "Calibration and A/B".
- **Acceptance**: forward: `for f in plan-grounding plan-completeness plan-dependencies; do p=claude/skills/review-v2-lenses/references/lens-$f.md; test -f $p && grep -q '^\*\*Family\*\*: plan' $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 19. Write the plan lenses: executor, risk, premortem [M]
- **Files**: `claude/skills/review-v2-lenses/references/lens-plan-executor.md` (new), `claude/skills/review-v2-lenses/references/lens-plan-risk.md` (new), `claude/skills/review-v2-lenses/references/lens-plan-premortem.md` (new)
- **Depends on**: 13
- **Action**: Write three plan lens files, with header values and signal names from the catalogue index.
- **Detail**:
  - Follow Approach, "Lens catalogue" and Approach, "Agents".
  - `plan-executor` has tier `executor` and is dispatched per task to `plan-executor`.
  - `plan-risk` keeps `claude/commands/review-plan.md` Agent 4's checkpoint-placement rule (review placement, not cadence) and its pinned-version API and advisory checks.
  - `plan-premortem` has depth `deep` and carries an "experimental — weak evidence" line.
  - Seed each lens's **Does not flag** section from the rejection clusters listed in Approach, "Calibration and A/B".
- **Acceptance**: forward: `for f in plan-executor plan-risk plan-premortem; do p=claude/skills/review-v2-lenses/references/lens-$f.md; test -f $p && grep -q '^\*\*Family\*\*: plan' $p || echo MISSING $f; done | wc -l` prints `0` (today: `3`).

### 20. Write the synthesis lens and the harness-quality project pack [M]
- **Files**: `claude/skills/review-v2-lenses/references/lens-synthesis.md` (new), `.claude/review-lenses/harness-quality.md` (new)
- **Depends on**: 13
- **Action**: Write the cross-family `synthesis` lens (`**Family**: any`) and this repo's project lens pack `harness-quality` (`**Paths**: claude/**`, `**V1 category**: package-quality`), which carries `claude/commands/review.md` Agent 6's checks.
- **Detail**:
  - Follow Approach, "Lens catalogue".
  - Agent 6's checks are: frontmatter, section order, cross-references, stubs, shared-block parity, and the tomlctl text gates — every `tomlctl …` line in a bash fence parses (`command_lint`), flag-table rows name real flags (`flag_table_lint`), SKILL.md bodies ≤500 lines, references ≤600, descriptions ≤1024 characters, relative links resolve, and a gated carrier names each skill as a backticked name plus the word "skill" with no negation within 160 characters.
  - The perf flavour of `synthesis` includes v1 `/optimise`'s cross-cutting concurrency review; perf synthesis findings export as `concurrency`.
- **Acceptance**: forward: `test -f claude/skills/review-v2-lenses/references/lens-synthesis.md && test -f .claude/review-lenses/harness-quality.md && grep -c '^\*\*Paths\*\*: claude/\*\*' .claude/review-lenses/harness-quality.md` prints `1` (today: prints nothing, exit 1).

### 21. Write the review-v2-engine skill: pipeline, context pack, routing [M]
- **Files**: `claude/skills/review-v2-engine/SKILL.md` (new), `claude/skills/review-v2-engine/references/context-pack.md` (new), `claude/skills/review-v2-engine/references/routing.md` (new)
- **Depends on**: 2, 13
- **Action**: Write the orchestrator contract: stages 0–9 and the depth table in `SKILL.md`; the fence catalogue with each fence's producer and its absent-fence wording in `references/context-pack.md`; and the change-profile signals in `references/routing.md`, each a cheap Grep or classification test for one signal of the catalogue's vocabulary, mapped to the lenses it activates.
- **Detail**:
  - Follow Approach, "Pipeline, depth and substrate".
  - Stage 0 checks `review_record` in `tomlctl capabilities` `.features` before anything else. Stage 2 opens every flow-scoped finder, verifier and executor prompt with the column-0 `ledger: .claude/flows/<slug>/review-v2.toml` line, and builds `PRIOR` from `tomlctl review list`. Stage 3 computes the catalogue hash and stage 4 passes it as `--set catalogue=`. Stage 5 ends the turn after the `async_launched` Workflow return and resumes at stage 6 on the task notification. Every `tomlctl review` call passes `--slug` or `--scope`.
  - Every `tomlctl review …` line in a bash fence must parse (`command_lint`), which is why this task depends on 2.
  - Invoke the unchanged `tomlctl`, `flow-contract-flow-context` and `backlog-capture` skills by name.
  - The envelope build uses the v1 `--command` names.
  - Baselines run through the `verification` agent.
  - Drop any prompt-cache justification for the shared preamble (R13).
  - The description stays under 1024 characters, the body under 500 lines and each reference under 600.
- **Acceptance**:
  - forward: `test -f claude/skills/review-v2-engine/SKILL.md && grep -c 'tomlctl review record' claude/skills/review-v2-engine/SKILL.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 22. Write the engine references: agent waves, report and dispositions, A/B testing [M]
- **Files**: `claude/skills/review-v2-engine/references/agent-waves.md` (new), `claude/skills/review-v2-engine/references/report-and-dispositions.md` (new), `claude/skills/review-v2-engine/references/ab-testing.md` (new)
- **Depends on**: 2, 5, 8
- **Action**: Write the Agent-tool fallback (wave order, finder and verifier waves of at most 16 per message, the return check against `tomlctl review schema return` by reading, the one re-dispatch, one-shot dispatch with no `name:`, and — when a flow is bound — every finder and verifier prompt opening with the column-0 line `ledger: .claude/flows/<slug>/review-v2.toml`), the report layout plus the disposition grammar mapped to `tomlctl review triage`/`export-v1` calls, and the A/B protocol and metrics.
- **Detail**: Follow Approach, "Pipeline, depth and substrate" and Approach, "Calibration and A/B". The fallback also fires when the Workflow launch reports `error` or completes with no result. Dispositions are a user-engagement gate that the autonomy directive does not override. Disposition prose from the user is staged with the Write tool and passed through the `triage` `--<flag>-file` twins, never through shell quoting. Every `tomlctl review …` line in a bash fence must parse (`command_lint`).
- **Acceptance**:
  - forward: `for f in agent-waves report-and-dispositions ab-testing; do test -f claude/skills/review-v2-engine/references/$f.md || echo MISSING $f; done | wc -l` prints `0` (today: `3`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 23. Write the review-finder, review-finder-lite and review-verifier agents [M]
- **Files**: `claude/agents/review-finder.md` (new), `claude/agents/review-finder-lite.md` (new), `claude/agents/review-verifier.md` (new)
- **Depends on**: —
- **Action**: Write three agents with frontmatter `name`, `description`, `tools`, `model: opus`, `effort` (finder high, lite medium, verifier high), `color`, and `skills:` (finders: `review-v2-method`, `research-methods`; verifier: `review-v2-method`). Each body includes the `research-read-only` and `research-delivering` shared blocks copied byte-identically from `claude/agents/research-deep.md`.
- **Detail**:
  - Follow Approach, "Agents".
  - Descriptions name what the agent does, never a carrier's lens numbers.
  - The finder tool lists copy `research-deep`'s; the verifier drops the Playwright tools.
  - Bodies say "if `review-v2-method` is not loaded, invoke it first", load reference files only through the Skill tool, and name no `tomlctl review` command in a bash fence (the clap tree may not exist yet when this task runs).
- **Acceptance**: forward: `grep -l '^skills:' claude/agents/review-finder.md claude/agents/review-finder-lite.md claude/agents/review-verifier.md 2>/dev/null | wc -l` prints `3` (today: `0`).

### 24. Write the plan-executor agent and register v2 shared-block carriers [S]
- **Files**: `claude/agents/plan-executor.md` (new), `scripts/shared-blocks.toml`
- **Depends on**: 23
- **Action**: Write `plan-executor` (read-only tools `Glob, Grep, Read, Bash, Skill`, `effort: medium`, `skills: [review-v2-method]`, the two shared blocks), then add the four v2 agent paths to the `files` arrays of the `research-read-only` and `research-delivering` blocks in `scripts/shared-blocks.toml`.
- **Detail**: Follow Approach, "Agents". The block text is copied byte-identically from `claude/agents/research-deep.md`; no block's text changes.
- **Acceptance**:
  - forward: `grep -cE 'review-finder\.md|review-finder-lite\.md|review-verifier\.md|plan-executor\.md' scripts/shared-blocks.toml` prints at least `4` (today: `0`).
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0 (today: exits 0).

### 25. Write the review-v2 saved workflow and its deploy entry [L]
- **Files**: `claude/workflows/review-v2.js` (new), `scripts/deploy-claude.ps1`
- **Depends on**: —
- **Action**: Write the saved Workflow script (`export const meta = {name: 'review-v2-pipeline', …}` with phases Find, Verify, Synthesize, and a `SCRIPT_VERSION` const returned as `script_version`) that implements the depth table over `args`. Add a `workflows` area (`Filter = '*.js'`, file symlinks) to `scripts/deploy-claude.ps1` and name it in the script's header SYNOPSIS/DESCRIPTION.
- **Detail**: Follow Approach, "Pipeline, depth and substrate" (`args` and the return shape are fixed there). The name is `review-v2-pipeline`, not `review-v2`, because a saved workflow registers as `/<meta.name>` and would compete with the `/review-v2` carrier. Use the workflow-authoring idioms:
  - `pipeline()` from each lens to its verifiers; a barrier only before synthesis and inside the deep pass loop.
  - Dedup against every finding seen so far, not only confirmed ones; vary file order by pass index over `args.files`.
  - `agent(prompt, {agentType, schema, label, phase})` with `args.schemas.return` for finders and `args.schemas.verdict` for verifiers; `args.executor_tasks` run as a pipeline stage on `plan-executor`. No `Date.now()`/`Math.random()`.
  - Every prompt is built from `args`, so the carrier's `ledger:` line and preamble reach each agent unchanged.
  - A panel returns three `by = "panel"` verdicts.
  - `log()` every routed-off lens and every trimmed suggestion, so no cap is silent.
  - Return plain data only.
- **Acceptance**:
  - forward: `test -f claude/workflows/review-v2.js && grep -c "name: 'review-v2-pipeline'" claude/workflows/review-v2.js` prints `1` (today: prints nothing, exit 1).
  - forward: `test -f claude/workflows/review-v2.js && t=$(mktemp --suffix .js) && { echo 'async function __wf(agent, pipeline, parallel, log, phase, args, budget, workflow) {'; sed 's/^export //' claude/workflows/review-v2.js; echo '}'; } > "$t" && node --check "$t"` exits 0 (today: exits 1, file absent).
  - falsifier: deleting one closing parenthesis in the script makes the `node --check` probe exit 1.
  - forward: `grep -c "Name = 'workflows'" scripts/deploy-claude.ps1` prints `1` (today: `0`).

### 26. Write the /review-v2 carrier [M]
- **Files**: `claude/commands/review-v2.md` (new)
- **Depends on**: 2, 10, 12, 13, 14, 15, 20, 21, 22, 23, 25
- **Action**: Write the code-family carrier. It covers frontmatter (`description`, `argument-hint` including `--depth` and `--flow`), invocations of the `tomlctl`, `flow-contract-flow-context`, `review-v2-engine`, `review-v2-lenses` and `backlog-capture` skills, and its deltas: the lint baseline, code lens set plus project packs, the context-free correctness pass, export to `review-ledger.toml`, and the `/review-apply` hand-off.
- **Detail**: Follow Approach, "Family specifics" and Approach, "Coexistence with v1". The carrier restates nothing the engine owns. Each skill invocation uses the backticked-name-plus-"skill" phrase the gate requires. Task 10 is a dependency because the carrier documents verb outcomes that tasks 3–8 implement.
- **Acceptance**:
  - forward: `test -f claude/commands/review-v2.md && grep -c '`review-v2-engine` skill' claude/commands/review-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 27. Write the /optimise-v2 carrier [M]
- **Files**: `claude/commands/optimise-v2.md` (new)
- **Depends on**: 2, 10, 12, 13, 16, 17, 20, 21, 22, 23, 25
- **Action**: Write the perf-family carrier, with invocations of the `tomlctl`, `flow-contract-flow-context`, `review-v2-engine`, `review-v2-lenses` and `backlog-capture` skills, plus its deltas: the Focal Points Brief, the `PROFILE BASELINE` via the `verification` agent, the perf severity rule, `branch1..branch2` scope, and export to `optimise-findings.toml` with the `/optimise-apply` hand-off.
- **Detail**: Follow Approach, "Family specifics". Each skill invocation uses the backticked-name-plus-"skill" phrase the gate requires.
- **Acceptance**:
  - forward: `test -f claude/commands/optimise-v2.md && grep -c 'PROFILE BASELINE' claude/commands/optimise-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 28. Write the /review-plan-v2 carrier [M]
- **Files**: `claude/commands/review-plan-v2.md` (new), `tomlctl/src/cli/dispatch/tests/finding_classes.rs`
- **Depends on**: 2, 10, 12, 13, 18, 19, 20, 21, 22, 23, 24, 25
- **Action**: Write the plan-family carrier, with invocations of the `tomlctl`, `flow-contract-flow-context`, `flow-contract-plansdirectory-prompt`, `flow-contract-task-store`, `flow-contract-plan-output-format`, `review-v2-engine`, `review-v2-lenses` and `backlog-capture` skills. Its deltas are plan resolution, the task-store import and `STORE CHECK`, `FORMAT CONTRACT`, the four orchestrator-only checks, `plan-executor` per task, and the verified-only merge offer with merge-exit ref-set gate → import → check. Add `claude/commands/review-plan-v2.md` to `PROSE` in `tomlctl/src/cli/dispatch/tests/finding_classes.rs`.
- **Detail**:
  - Follow Approach, "Family specifics".
  - Name every `tasks check` finding class the carrier mentions exactly as it exists in `tomlctl/src/tasks`; the `PROSE` entry makes `prose_class_names_exist_in_the_source` gate it.
  - The merge never runs Manual merge on an empty `AskUserQuestion` answer.
- **Acceptance**:
  - forward: `test -f claude/commands/review-plan-v2.md && grep -c 'plan-executor' claude/commands/review-plan-v2.md` prints at least `1` (today: prints nothing, exit 1).
  - forward: `grep -c 'review-plan-v2.md' tomlctl/src/cli/dispatch/tests/finding_classes.rs` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes — **predicted, unverified**.

### 29. Add the v2 carriers to the skill gate table and document v2 in CLAUDE.md [M]
- **Files**: `tomlctl/src/cli/dispatch/tests/skills.rs`, `CLAUDE.md`
- **Depends on**: 26, 27, 28
- **Action**: Add rows for `review-v2.md`, `optimise-v2.md` and `review-plan-v2.md` to the `carrier_invokes_required_skills` table (`tomlctl/src/cli/dispatch/tests/skills.rs` around line 1405), each listing exactly the skills its carrier invokes. Add a short "Review v2" subsection to `CLAUDE.md` covering the three commands, the agents, the store, `tomlctl review compare` for A/B, and the saved-workflow deploy step, and add `workflows` to the `claude/{agents,commands,skills,themes}` list in its "Developer setup" deploy sentence.
- **Detail**:
  - Every listed skill must exist under `claude/skills/`.
  - The CLAUDE.md subsection goes after "Flow agent tiers". It names the v2 agents as outside the `lite`/`deep` research buckets and keeps the no-`name:` rule.
- **Acceptance**:
  - forward: `grep -cE '"(review|optimise|review-plan)-v2\.md"' tomlctl/src/cli/dispatch/tests/skills.rs` prints `3` (today: `0`).
  - forward: `grep -c '/review-v2' CLAUDE.md` prints at least `1` (today: `0`).
  - falsifier: removing the `review-v2-engine` invocation from `claude/commands/review-v2.md` turns `carrier_invokes_required_skills` red — **predicted, unverified**.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 9, 11 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 30. The `tomlctl review` group is complete, tested and documented, and the version is bumped. This is a buildable crate increment, and every later markdown fence that names `tomlctl review` parses against it.

— CHECKPOINT B after tasks 12, 14, 15, 16, 17, 18, 19, 20, 21, 22, 24, 25 — dependency closure: 1, 2, 3, 4, 5, 8, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25. The v2 harness foundation (method, catalogue, every lens, engine, agents, workflow, deploy entry). It is buildable because no carrier references it yet, and `verify-shared-blocks.sh` covers the new agents.

— CHECKPOINT C after tasks 26 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 13, 14, 15, 20, 21, 22, 23, 25, 26. `/review-v2` usable end to end, the first A/B-able carrier.

— CHECKPOINT D after tasks 29 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29. `/optimise-v2`, `/review-plan-v2`, the gate rows and CLAUDE.md.

## Risks

- **Per-finding verification multiplies agents and cost.** Mitigations: verification is tiered by severity, suggestions are capped and batch-verified, `--depth quick` exists, and every run records `agents` and wall time so A/B measures the cost.
- **The verifier refutes true findings (recall loss).** Mitigations: the orchestrator audit re-checks every REFUTED critical, refuted rows persist and show in `review stats` per lens, and `review compare` against v1 surfaces only-v1 findings for human judgement.
- **Explicit `same_as` identity depends on finders and verifiers reading the `PRIOR` fence.** A missed `same_as` mints a duplicate rather than losing a finding. Mitigations: duplicates carry their anchor candidates under `related`, so `review list` and `compare` surface them, and the A/B protocol measures the duplicate rate before anyone relaxes the rule.
- **Universal `failure_scenario` at warning and above may demote legitimate design findings.** Mitigations: the `design` lens teaches consequence-chain scenarios; A/B and `stats` show the design lens's suggestion share; the rule is enforced in one place (`record`) if it needs relaxing per lens.
- **Workflow is unavailable or disabled, needs per-run approval (manual/accept-edits modes; a `Workflow(<name>)` allow rule under `-p`), shows the advisory Large-workflow warning above 25 agents, or the deployed copy is missing or stale.** Mitigations: the agent-wave fallback runs the same stages and also fires on a launch `error` or an empty result; the run records `substrate` and the script's `SCRIPT_VERSION` as `workflow`, so a stale out-of-band copy shows up in `stats` and `compare`; After Merge covers deploying the script, including the out-of-band `~/.claude-work` copy on this machine.
- **The `skills:` frontmatter preload is a new idiom here, and it injects `SKILL.md` without `references/`.** Mitigations: every procedure an agent needs lives in `review-v2-method`'s `SKILL.md`; each agent body still invokes `review-v2-method` when it is not loaded and loads reference files only through the Skill tool.
- **Two review systems confuse users or drift apart.** Mitigations: separate stores, `-v2` names (the saved workflow is `review-v2-pipeline` so it cannot shadow `/review-v2`), v2 skips the inputs sweep, and v1 is guarded byte-identical by a success criterion. Promotion or retirement is a decision after A/B, not part of this plan.
- **`export-v1` writes rows apply silently skips if malformed.** Mitigation: `export-v1` validates the v1 required set itself, and `--dry-run` previews.
- **Markdown documenting `tomlctl review` fails `command_lint` if it lands before the clap tree.** Mitigation: tasks 21, 22 and 26–28 depend on task 2, task 12 on task 3, and task 23's bodies name no `tomlctl review` command in a bash fence.

## After Merge

- Run `cargo install --path tomlctl`; the carriers shell out to the installed binary, `review` lands in 0.16.0, and each v2 carrier halts at stage 0 on an older binary.
- Rerun `pwsh -File scripts/deploy-claude.ps1` to link the four new agents, three commands, three skills and `claude/workflows/review-v2.js` (it targets `$HOME/.claude`). On this Linux machine `pwsh` is not installed and the session reads `~/.claude-work`, which holds out-of-band copies, so copy `claude/workflows/review-v2.js` into `~/.claude-work/workflows/` along with the new agents, commands and skills, and recopy it after every script change. Restart open sessions.
- Smoke test, **predicted, unverified**: run `/review-v2 --depth quick claude/commands/commit.md`, then `tomlctl review stats --scope <scope>` and `tomlctl review compare --scope <scope> --run U1 --v1 .claude/reviews/<scope>.toml`. Run one `/review-v2 --depth deep` on a single small file to exercise the deep branch of the script.
- First A/B round: run `/review` and `/review-v2` on one recent diff, plus `/review-plan` and `/review-plan-v2` on one plan, per `claude/skills/review-v2-engine/references/ab-testing.md`.
- Plan the follow-on recall-eval harness (R11) once v2 has produced runs.

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
