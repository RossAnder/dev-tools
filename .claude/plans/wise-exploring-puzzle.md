# Meta-Plan: Two Separate Plans for Pattern A + Pattern B (revised after /review-plan)

**Plan-mode scratch path**: `.claude/plans/wise-exploring-puzzle.md`
**Created**: 2026-04-18
**Status**: Draft (plan-mode), post-review revision

At ExitPlanMode approval, the content below splits into TWO independent plan files:
- `docs/plans/plan-new-directed-questions.md` (Plan A — flow slug `plan-new-directed-questions`)
- `docs/plans/plan-execution-record.md` (Plan B — flow slug `plan-execution-record`)

Each gets its own `.claude/flows/<slug>/` directory. Neither inherits the other's context.toml.

## Context (shared across both plans)

1. **Pattern A** — In `/plan-new`, inject extensive clarifying questions between initial research and design, with an optional follow-up directed-research pass.
2. **Pattern B** — A structured, append-only TOML "plan execution record" following the `tomlctl` ledger pattern; writers split between `/implement` (live task/verification events) and `/plan-update` (deviation/deferral/reconcile/status events); consumers include `/plan-update reconcile`, `/plan-update snapshot`, future `/implement` re-runs.

## Exploration Summary (Phase 2 checkpoint, retained)

- `/plan-new` phases: 1 Scope/Parse → 2 Explore (3 parallel Explore agents) → 3 Research (CONDITIONAL today) → 4 Design → 5 Write plan → 6 ExitPlanMode.
- `/implement` (263 lines): writes `status`, `updated`, `[tasks].in_progress` in Phase 1; deviations reported in Phase 4 summary, not persisted; TaskCreate/TaskUpdate ephemeral; Phase 4.5 auto-invokes `Skill("plan-update", "status")`.
- `/plan-update` (455 lines): 7 operations. Owns `PROGRESS-LOG.md` with Completed Items, Deviations (D1..Dn), Deferrals (DF1..DFn), Session Log (`| Date | Changes | Commits |`). D-numbering instructions live at `plan-update.md:148, 156, 231, 293-304, 340, 454` and docstring at `review-plan.md:105`.
- Ledger pattern canonical at `claude/commands/review.md:95-281`, parity across 4 files via `<!-- SHARED-BLOCK:ledger-schema START/END -->` markers; `scripts/shared-blocks.toml` declares them; `.githooks/pre-commit` regex matches only those 4 files; `blocks_verify_reproduces_shell_hashes` test at `tomlctl/src/cli.rs:1182-1284` pins the hash.
- `## Flow Context` block carries `<!-- SHARED-BLOCK:flow-context START/END -->` markers **only** in the 4 parity-checked files. `plan-new.md`, `plan-update.md`, `implement.md`, `review-plan.md` carry embedded copies WITHOUT markers.
- `tomlctl` DATE_KEYS = `[created, updated, first_flagged, last_updated, resolved, date]` at `tomlctl/src/convert.rs:312-317`. Keys on this list round-trip as native TOML datetimes; anything else lands as a quoted string.
- `tomlctl items orphans` (`orphans.rs:24`) and `items find-duplicates` (`dedup.rs:32`) hardcode the ledger schema on `[[items]]` — they must not be invoked against execution-record files.
- `tomlctl items list --group-by <FIELD>` takes a plain field name and buckets on the raw stringified value; `@date:` is only a RHS type-cast for `--where-*`.
- `tomlctl set` on a non-existent path errors with "No such file or directory" — targets must pre-exist.
- Existing flows directory: `.claude/flows/command-suite-improvements/` (`status = complete`, skipped by resolution).

## Answered Design Questions

1. Two separate plans (Pattern A and Pattern B independent).
2. Pattern A: questions land AFTER initial Research, BEFORE Design; optional directed second research pass follows.
3. Pattern B writer: BOTH — `/implement` writes task/verification events; `/plan-update` writes deviation/deferral/reconcile/status events.
4. Pattern B overlap: Replace PROGRESS-LOG.md tables (render from log); keep `[tasks]` counters but derive them from the log.

## Review Findings Applied (post /review-plan)

Global decisions propagated through both plans:

- **Field name**: entry timestamp uses `date` (TOML date, YYYY-MM-DD), which is in `DATE_KEYS` and coerces correctly on round-trip. No `timestamp` field. Intra-day ordering relies on tomlctl's insertion-order preservation.
- **Array name**: `[[items]]` retained (simpler than contesting tomlctl's assumptions); `orphans` and `find-duplicates` explicitly forbidden against `execution-record.toml` in the schema contract.
- **Group-by strategy**: Session Log groups by `date` directly (now a YYYY-MM-DD field, not an ISO datetime) — no `@date:` projection needed.
- **Shared-block parity**: widen from 4 files to 8 in one task — `review.md`, `optimise.md`, `review-apply.md`, `optimise-apply.md`, `plan-new.md`, `plan-update.md`, `implement.md`, `review-plan.md`. Requires wrapping 4 embedded copies with SHARED-BLOCK markers, extending `scripts/shared-blocks.toml`, updating `.githooks/pre-commit` regex, and updating the pinned-hash integration test.
- **CLAUDE.md**: the "Developer setup" section enumerates the 4 parity-checked files; Plan B updates it to match the new set.
- **Task sizing**: re-labelled Plan A Task 1 M → L; Plan B Task 1 remains M (coordination risk); Plan B Task 2 M → L; Plan B Task 5 split into 5a-5e.
- **Acceptance criteria**: mechanised — explicit grep/tomlctl commands instead of "manual read-through" or "no-op".
- **`claude/commands/staging/`**: out of scope per user direction. Both plans ignore it.

---

# Plan A: Directed Clarifying Questions + Optional Second Research in /plan-new

**Plan path**: `docs/plans/plan-new-directed-questions.md`
**Flow slug**: `plan-new-directed-questions`
**Created**: 2026-04-18
**Status**: Draft

## Context

`/plan-new` currently asks 2-3 clarifying questions in Phase 1 (before exploration) and runs exploration + conditional research up-front. Design-shaping ambiguities that only surface from exploration have no structured moment to return to the user. Plan A adds a post-research questions phase and an optional second research pass directed by the answers.

## Scope

- **In scope**: `claude/commands/plan-new.md` only.
- **Out of scope**: `/implement`, `/plan-update`, ledger schemas, shared blocks, `claude/commands/staging/`.
- **Affected areas**: `claude/commands/plan-new.md`
- **Estimated file count**: 1

## Approach

### Decision: Phase 3 (Initial Research) becomes unconditional

The user's phrasing "questions following initial research phase" implies the initial research always runs. Current Phase 3 skip-if-no-novel-tech clause is dropped. Trade-off: `/plan-new` spends two research-agent budget on simple tasks that previously skipped Phase 3. Mitigation: research agents may return early with minimal findings when the task uses only well-established patterns; the phase runs but its cost adjusts to task complexity.

### Phase ordering (post-change)

| # | Phase | Change |
|---|-------|--------|
| 1 | Scope & Parse | Remove the 2-3 upfront clarifying-questions instruction; keep only the scope-split check |
| 2 | Explore | Unchanged |
| 3 | Initial Research | **UNCONDITIONAL** (skip clause dropped); 2 parallel general-purpose agents |
| 4 | Directed Questions | **NEW** — 4-8 findings-cited questions via `AskUserQuestion` (1-2 batches × 4 questions) |
| 5 | Directed Research | **NEW** — optional, triggered only if answers surface unresearched topics; 1 narrow-scope agent |
| 6 | Design | Former Phase 4 content, renumbered. Preserves "Optionally launch up to 2 Plan agents" subsection currently at `plan-new.md:214-216` |
| 7 | Write Plan | Former Phase 5, renumbered |
| 8 | ExitPlanMode | Former Phase 6, renumbered |

### Phase 4 (Directed Questions) specification

- Always runs (no skip clause — simple tasks can still have integration or edge-case questions worth asking).
- Model reads Exploration Notes + Research Notes and synthesises 4-8 questions covering:
  1. Behavioural / UX decisions
  2. Integration boundaries
  3. Edge cases / fallback behaviour
  4. Non-functional constraints
  5. Approach preference when multiple viable
- **Each question must cite the finding that prompted it.**
- Up to 2 `AskUserQuestion` calls (tool limit 4 questions per call).
- Persist answers to the plan-mode file as a `## User Decisions` section before Phase 6.

### Phase 5 (Directed Research) specification

- Runs only if Phase 4 answers introduced a topic not yet researched.
- Scope: up to 1 general-purpose research agent, narrow topic.
- Budget: 500 words / 10 findings cap.
- Skip clause: if all answers covered by Phase 3, skip entirely and note the decision.

## Verification Commands

```
shared-blocks: scripts/verify-shared-blocks.sh
phase-numbering: grep -E '^## Phase [0-9]+' claude/commands/plan-new.md | awk '{print $3}' | sort -un
phase-references: grep -n 'Phase [0-9]' claude/commands/plan-new.md
```

## Tasks

### 1. Restructure `plan-new.md` phases + rewrite internal cross-references [L]
- **Files**: `claude/commands/plan-new.md`
- **Depends on**: —
- **Action**: (a) Renumber existing Phases 4 → 6, 5 → 7, 6 → 8. (b) Remove the Phase 3 conditional-skip clause, retitle as "Initial Research". (c) Insert new Phase 4 (Directed Questions) and Phase 5 (Directed Research) sections per the specifications in Approach. (d) Update Phase 1 step 4 to drop the 2-3 upfront-questions instruction; keep only the scope-split check, with a pointer forward to Phase 4. (e) Update all intra-file prose cross-references to phase numbers: `plan-new.md:158` ("Proceed directly to Phase 4" — now Phase 6), `:183` ("before entering Phase 4" — now Phase 6), `:212` ("Phase 2" — stays correct), and any others surfaced by the grep. (f) Preserve the "Optionally launch up to 2 Plan agents" subsection currently at lines 214-216 inside the renumbered Phase 6 Design. (g) Update the front-matter `description` line if it named specific phase counts.
- **Detail**: Exact phase titles: "Phase 4: Directed Questions", "Phase 5: Directed Research". Cross-reference Phase 4 answers inside Phase 6 Design guidance. Run the phase-references grep before and after to confirm no `Phase N` string silently changed meaning.
- **Acceptance**:
  - `grep -E '^## Phase [0-9]+' claude/commands/plan-new.md | awk '{print $3}' | sort -un` returns exactly `1 2 3 4 5 6 7 8` (8 lines, no gaps, no duplicates).
  - `grep -n 'Phase 4' claude/commands/plan-new.md` shows only references that semantically intend the NEW Phase 4 (Directed Questions). No reference to Phase 4 means Design.
  - `grep -c "Optionally launch up to 2 Plan agents" claude/commands/plan-new.md` returns ≥ 1 (subsection preserved).
  - `grep -c "Skip this phase if the task uses only well-established patterns" claude/commands/plan-new.md` returns `0` (skip clause removed).
  - `scripts/verify-shared-blocks.sh` passes.

### 2. Update Phase 1 requirements check [S]
- **Files**: `claude/commands/plan-new.md`
- **Depends on**: 1
- **Action**: Rewrite Phase 1 step 4 to remove the "2-3 targeted clarifying questions" paragraph; replace with: "Clarifying questions are deferred to Phase 4 (Directed Questions), which operates on exploration and research findings. In Phase 1, only check whether the task bundles independent concerns — if so, propose splitting via `AskUserQuestion` before spending exploration budget."
- **Acceptance**:
  - `grep -c "2-3 targeted clarifying questions" claude/commands/plan-new.md` returns `0`.
  - Phase 1 still contains the scope-split check.

### 3. Add `## Important Constraints` bullet on new phase budget [S]
- **Files**: `claude/commands/plan-new.md`
- **Depends on**: 1
- **Action**: Append one bullet to the existing `## Important Constraints` list: "Phase 3 is now unconditional; Phase 4 always runs with up to 2 AskUserQuestion batches; Phase 5 runs only when Phase 4 answers surface unresearched topics. Total sub-agent budget: 3 Explore + 2 Initial Research + optional 1 Directed Research + optional 2 Plan = up to 8 agents."
- **Acceptance**:
  - `grep -c "Phase 3 is now unconditional" claude/commands/plan-new.md` returns `1`.
  - The bullet sits inside the `## Important Constraints` section.

## Dependency Graph

Batch 1 (sequential): Task 1
Batch 2 (parallel, after Task 1): Tasks 2, 3

Total: 3 tasks, 1 file. No shared-block impact.

## Risks

- **Risk: Unconditional Phase 3 spends budget on simple tasks** — Mitigation: research agents return early with minimal findings when no novel tech is present; phase-cost adjusts to task complexity rather than being statically skipped.
- **Risk: Phase 4 questions burn tokens even when exploration + research found no ambiguity** — Mitigation: no explicit skip clause, but the synthesis guidance requires each question cite a specific finding. When no such findings exist, the model has no material to synthesise from and will return a minimal set or none.
- **Risk: "Extensive" varies run-to-run** — Mitigation: 5 concrete question categories + 4-8 min/max band + citation-required-per-question rule.
- **Risk: Renumbering silently retargets cross-references** — Mitigation: Task 1 acceptance explicitly greps for `Phase 4` and `Phase 6` and requires the operator confirm each reference's intended target. Mechanical check, not manual read-through.

---

# Plan B: TOML Execution Record for the Flow Suite

**Plan path**: `docs/plans/plan-execution-record.md`
**Flow slug**: `plan-execution-record`
**Created**: 2026-04-18
**Status**: Draft

## Context

Plan execution history currently lives in `[tasks]` counters (coarse) and `PROGRESS-LOG.md` markdown tables (unstructured, hand-maintained). `/implement` writes neither persistently — it reports deviations and relies on the user to run `/plan-update deviation`. Plan B introduces a structured, append-only TOML record at `.claude/flows/<slug>/execution-record.toml`, following the `tomlctl`-backed ledger pattern; `/implement` writes live task/verification events, `/plan-update` writes deviation/deferral/reconcile/status events, PROGRESS-LOG.md becomes a rendered view, and `[tasks]` counters become derived.

## Scope

- **In scope**: schema definition; `plan-new.md` initialisation; `implement.md` writer + Phase 1 read; `plan-update.md` rewrite of deviation/defer/reconcile/status + render-from-log + migrate + reformat/catchup prose scrub; shared-block widening from 4 to 8 files; new `## Execution Record Schema` shared block; `.githooks/pre-commit` regex extension; `blocks_verify_reproduces_shell_hashes` test update; CLAUDE.md update; D/DF-number reference purge.
- **Out of scope**: tomlctl code changes (retain existing surface); rotation/compaction of long-lived logs; `/review`/`/optimise` cross-referencing of execution entries (future); `claude/commands/staging/`.
- **Affected areas**: `claude/commands/plan-new.md`, `plan-update.md`, `implement.md`, `review.md`, `optimise.md`, `review-apply.md`, `optimise-apply.md`, `review-plan.md`, `scripts/shared-blocks.toml`, `.githooks/pre-commit`, `tomlctl/src/cli.rs` (test only), `CLAUDE.md`
- **Estimated file count**: 12

## Research Notes

No Phase 3 research performed — internal prompt + schema design using existing `tomlctl` surface. Sources cited are from Phase 2 exploration:
- Ledger schema: `claude/commands/review.md:95-281` (canonical)
- tomlctl DATE_KEYS: `tomlctl/src/convert.rs:312-317` (confirms `date` coerces, `timestamp` does not)
- tomlctl items surface: `tomlctl/src/cli.rs`, including `items next-id --prefix E` works, `--where` flags AND-combine, `--pluck` + `--count` mutually exclusive
- Shared-block enforcement: `scripts/shared-blocks.toml` + `.githooks/pre-commit` + `blocks_verify_reproduces_shell_hashes` at `tomlctl/src/cli.rs:1182-1284`
- D/DF reference sites: `plan-update.md:148, 156, 231, 293-304, 340, 454`, `review-plan.md:105`

## Approach

### Schema

New file per flow: `.claude/flows/<slug>/execution-record.toml`

```toml
schema_version = 1
last_updated = 2026-04-18

[[items]]
id = "E1"
type = "task-completion"
date = 2026-04-18
agent = "implement"
task_ref = "add-retry-logic"
summary = "Added retry logic in src/retry.rs"
files = ["src/retry.rs", "tests/retry_test.rs"]
commits = ["abc1234"]
status = "done"

[[items]]
id = "E2"
type = "verification"
date = 2026-04-18
agent = "implement"
summary = "cargo test passed"
command = "cargo test --manifest-path tomlctl/Cargo.toml"
outcome = "pass"

[[items]]
id = "E3"
type = "deviation"
date = 2026-04-18
agent = "plan-update"
task_ref = "add-redis-cache"
summary = "Used existing LruCache util rather than introducing Redis"
original_intent = "Add Redis dependency for caching"
rationale = "src/util/cache.rs already covers the use case"
commits = ["def5678"]
legacy_id = "D3"
```

**Required fields per entry**: `id` (E{n} monotonic), `type`, `date` (YYYY-MM-DD TOML date), `agent`, `summary`.

**Type vocabulary + type-specific required fields**:
- `task-completion`: `task_ref` (OPAQUE — task title slug, NOT positional number), `status` ∈ {`done`, `failed`, `skipped`}, `files[]`, `commits[]`
- `verification`: `command`, `outcome` ∈ {`pass`, `fail`}
- `deviation`: `original_intent`, `rationale`, `commits[]`; optional `supersedes_entry = "E<n>"`; optional `legacy_id = "D<n>"` (populated by `migrate`)
- `deferral`: `task_ref`, `reason`, `reevaluate_when`; optional `legacy_id = "DF<n>"`
- `decision`: `alternatives[]`, `chosen`, `rationale`
- `reconcile`: `direction` ∈ {`forward`, `reverse`}, `findings_count`, `commits_checked[]`
- `status-transition`: `from_status`, `to_status`
- `checkpoint`: freeform; emitted by `reformat`/`catchup` when the plan is restructured.

**`task_ref` is an opaque identifier** (task title slug, e.g. `add-retry-logic`), not a positional task number. This keeps entries referentially stable across `/plan-update reformat` which may renumber plan tasks. Task title slugs are derived from the plan document's task heading, lowercased, hyphenated.

**Array name `[[items]]` — restriction**: `tomlctl items orphans` and `tomlctl items find-duplicates` hardcode the ledger schema (expecting `file`, `symbol`, `summary`, `severity`, `category`). **These two subcommands MUST NOT be invoked against `execution-record.toml`** and the schema contract block documents this restriction prominently. All other `tomlctl items` ops (`list`, `get`, `add`, `add-many`, `update`, `remove`, `apply`, `next-id --prefix E`) work correctly against `[[items]]` with the execution-record schema.

**Append-only**: entries never updated after write. Corrections go in new entries with `supersedes_entry`. Append order is preserved by tomlctl (exclusive `.lock` sidecar + atomic tempfile + rename).

### `[artifacts]` extension + shared-block widening

Add `execution_record = ".claude/flows/<slug>/execution-record.toml"` to the canonical `[artifacts]` table in the `## Flow Context` block.

Widen parity from 4 files to 8:
- Currently SHARED-BLOCK wrapped (in manifest): `review.md`, `optimise.md`, `review-apply.md`, `optimise-apply.md`
- Currently embedded without markers: `plan-new.md`, `plan-update.md`, `implement.md`, `review-plan.md`

One-time surgery: wrap the 4 embedded copies with `<!-- SHARED-BLOCK:flow-context START -->` / `END` markers at the existing embedded-copy boundaries. Extend `scripts/shared-blocks.toml` `flow-context.files` from 4 to 8 entries. Extend `.githooks/pre-commit` file-path regex to include the 4 newly-wrapped files. Update `blocks_verify_reproduces_shell_hashes` at `tomlctl/src/cli.rs:1182-1284` to reflect the new file list and recompute the pinned hash.

Tighten the Field-responsibilities prose. Replace the current sentence (reading "If `[artifacts]` is absent when read, commands compute from `slug` but MUST write it back on their next TOML write.") with:

> "If `[artifacts]` is absent OR if any canonical key within `[artifacts]` is missing (currently: `review_ledger`, `optimise_findings`, `execution_record`), commands compute the missing path(s) from `slug` and MUST write them back on their next TOML write."

### Writer responsibilities

**`/plan-new`** (Phase 5, Write Plan):
- Create `.claude/flows/<slug>/execution-record.toml` as a zero-byte file via the `Write` tool (bootstrap — `tomlctl set` fails on non-existent targets).
- Populate `schema_version = 1` + `last_updated = <today>` via `tomlctl set`.
- Persist `execution_record` path in `[artifacts]`.

**`/implement`**:
- **Phase 1**: read execution-record via `tomlctl items list execution-record.toml --where type=task-completion --where status=done --pluck task_ref`. Skip plan tasks whose slug appears in the result.
- **Phase 2, per task finish**: append `type=task-completion` entry via heredoc stdin (per the MEMORY.md rule — never tempfile-stage payloads):
  ```
  cat <<'EOF' | tomlctl items add execution-record.toml --json -
  {"id":"<next>","type":"task-completion","date":"2026-04-18","agent":"implement","task_ref":"<slug>","summary":"...","files":[...],"commits":[...],"status":"done"}
  EOF
  tomlctl set execution-record.toml last_updated 2026-04-18
  ```
- **Phase 2, deviation detection**: append `type=deviation` entry (keep the user-facing informational reminder, drop the "run /plan-update deviation" remediation — it's already persisted).
- **Phase 3, verification**: append one `type=verification` entry per verification command.
- `in_progress` counter rule: derived from live `TaskCreate` state **during `/implement` execution only**. Writers outside `/implement` MUST leave `[tasks].in_progress` untouched.

**`/plan-update`**:
- `deviation` op → `type=deviation` (E-number supersedes D-number; `supersedes_entry` if applicable).
- `defer` op → `type=deferral` (E-number supersedes DF-number).
- `reconcile` op → agents append `type=reconcile` entries; follow-up actions append their own entries.
- `status` op → **reconciler contract**: before appending a `status-transition`, query `tomlctl items list execution-record.toml --where type=task-completion --pluck task_ref` and skip any task_refs already present. Only append `status-transition` when the flow's `status` actually changes.
- `reformat` / `catchup` ops → append one `type=checkpoint` entry tagging the restructure; trigger render-from-log afterwards to regenerate PROGRESS-LOG.md with E-numbered tables.
- `snapshot` op → read-only render.
- NEW `migrate` op → back-fill E-entries from existing PROGRESS-LOG.md with `legacy_id` preserving D/DF numbering.

### PROGRESS-LOG.md render contract

Top-of-file marker: `<!-- Generated from execution-record.toml. Do not edit by hand. -->` (no `<today>` / `<now>` substitution — rendering is a pure function of the log).

Render queries:
- **Completed Items**: `tomlctl items list --where type=task-completion --where status=done`
- **Deviations**: `tomlctl items list --where type=deviation`
- **Deferrals**: `tomlctl items list --where type=deferral`
- **Session Log**: `tomlctl items list --group-by date` (now buckets per-calendar-day because `date` is YYYY-MM-DD). Existing 3-column schema preserved:

  | Date | Changes | Commits |
  |------|---------|---------|
  | `YYYY-MM-DD` (the bucket key) | `"<N> entries: <type> × <k>, ..."` (summarised by counting entry types within the day) | `sha1, sha2, ...` (union of `commits` arrays across the day's entries) |

### `[tasks]` counter derivation

On every context.toml write that touches `[tasks]`:
- `completed` = `tomlctl items list execution-record.toml --where type=task-completion --where status=done --pluck task_ref` → pipe through `jq 'unique | length'` (or equivalent deduplication). Distinct-slug count, so failure-then-success retries count as 1 not 2.
- `total` = plan-document item count (existing derivation, unchanged).
- `in_progress` = NOT derived from the log. Only `/implement` during live execution writes this field. `/plan-update` ops outside an `/implement` session leave the existing value untouched.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
shared-blocks: scripts/verify-shared-blocks.sh
pre-commit-regex: .githooks/pre-commit (dry-run on staged files across the 8 widened parity files)
```

Plus the end-to-end test in Task 8.

## Tasks

### 1. Widen shared-block parity; extend `[artifacts]` with `execution_record`; update Field responsibilities [M]
- **Files**: `claude/commands/plan-new.md`, `plan-update.md`, `implement.md`, `review.md`, `optimise.md`, `review-apply.md`, `optimise-apply.md`, `review-plan.md`, `scripts/shared-blocks.toml`, `.githooks/pre-commit`, `tomlctl/src/cli.rs` (test only)
- **Depends on**: —
- **Action**: (a) Wrap the embedded `## Flow Context` block copies in `plan-new.md`, `plan-update.md`, `implement.md`, `review-plan.md` with `<!-- SHARED-BLOCK:flow-context START -->` / `<!-- SHARED-BLOCK:flow-context END -->` markers at the existing embedded-copy boundaries. (b) Add `execution_record = ".claude/flows/<slug>/execution-record.toml"` to the canonical `[artifacts]` table in the shared block. (c) Tighten the Field-responsibilities prose: replace the single absent-block sentence with the per-key version quoted in Approach. (d) Replicate the updated block byte-identically across all 8 files. (e) Extend `scripts/shared-blocks.toml` `flow-context.files` from 4 to 8. (f) Extend `.githooks/pre-commit` file-path regex to match the 4 newly-wrapped files. (g) Update `blocks_verify_reproduces_shell_hashes` at `tomlctl/src/cli.rs:1182-1284` to reflect the new file list + recompute the pinned hash.
- **Detail**: Single-agent ownership — a multi-file mechanical replication. Run `scripts/verify-shared-blocks.sh` locally before committing. Recompute the pinned hash by running `scripts/verify-shared-blocks.sh --print-hash flow-context` (or the equivalent; inspect the script) and pasting into the test.
- **Acceptance**:
  - `scripts/verify-shared-blocks.sh` exits 0.
  - `grep -l 'SHARED-BLOCK:flow-context START' claude/commands/*.md` returns all 8 files.
  - `grep -c 'execution_record =' <(cat claude/commands/plan-new.md claude/commands/plan-update.md claude/commands/implement.md claude/commands/review.md claude/commands/optimise.md claude/commands/review-apply.md claude/commands/optimise-apply.md claude/commands/review-plan.md)` returns 8.
  - `cargo test --manifest-path tomlctl/Cargo.toml blocks_verify_reproduces_shell_hashes` passes.
  - `grep -c "any canonical key within" claude/commands/plan-new.md` returns 1 (tightened contract prose present).

### 2. Author `## Execution Record Schema` shared block + register in parity manifest + update CLAUDE.md [L]
- **Files**: `claude/commands/plan-new.md`, `plan-update.md`, `implement.md`, `scripts/shared-blocks.toml`, `.githooks/pre-commit`, `CLAUDE.md`
- **Depends on**: 1
- **Action**: (a) Create a new `## Execution Record Schema` section in `plan-new.md` containing six deliverables: schema TOML example (matching the `date`-not-`timestamp` convention), type vocabulary with type-specific required fields, write contract (two-call pattern + heredoc idiom), `[[items]]` naming rationale + `orphans`/`find-duplicates` restriction, render-to-markdown contract with explicit Session Log column mapping, `[tasks]` derivation formula. (b) Wrap with `<!-- SHARED-BLOCK:execution-record-schema START/END -->` markers. (c) Replicate byte-identically to `plan-update.md` and `implement.md`. (d) Add an entry to `scripts/shared-blocks.toml` declaring `execution-record-schema` parity across the 3 files. (e) Extend `.githooks/pre-commit` regex if needed to match those 3 files for the new block. (f) Update `CLAUDE.md` to list the new 5th block alongside the 4 existing parity-checked blocks.
- **Detail**: This task sits AFTER Task 1 (not parallel) because both edit `plan-new.md`, `plan-update.md`, and `implement.md`. Placement: adjacent to the existing `## Flow Context` block for local discoverability. Prose MUST explicitly forbid `tomlctl items orphans` / `find-duplicates` against execution-record files.
- **Acceptance**:
  - `scripts/verify-shared-blocks.sh` exits 0 including the new block.
  - `grep -l 'SHARED-BLOCK:execution-record-schema START' claude/commands/*.md` returns 3 files (plan-new, plan-update, implement).
  - `grep -c "must not be invoked against" claude/commands/plan-new.md` returns ≥ 1 (restriction prose present).
  - `grep -c "execution-record-schema" CLAUDE.md` returns ≥ 1.

### 3. Update `/plan-new` Phase 5 to bootstrap + initialise `execution-record.toml` [S]
- **Files**: `claude/commands/plan-new.md`
- **Depends on**: 1, 2
- **Action**: In Phase 5 (Write Plan), after creating `context.toml`, add steps: (a) create `.claude/flows/<slug>/execution-record.toml` as a zero-byte file via the `Write` tool — this is a required bootstrap because `tomlctl set` errors on non-existent targets; (b) `tomlctl set execution-record.toml schema_version 1`; (c) `tomlctl set execution-record.toml last_updated <today>`; (d) ensure `[artifacts].execution_record` in context.toml matches the initialised path.
- **Detail**: Call out the bootstrap requirement explicitly — the command file should document why the `Write` step exists (so future readers don't "optimise" it away).
- **Acceptance**:
  - After `/plan-new` runs, `tomlctl parse .claude/flows/<slug>/execution-record.toml` returns a document with `schema_version = 1` and `last_updated = <today>` and empty `[[items]]` table-array.
  - `tomlctl get .claude/flows/<slug>/context.toml artifacts.execution_record` returns the expected path string.

### 4. Update `/implement` to read prior log + write task/verification/deviation entries [L]
- **Files**: `claude/commands/implement.md`
- **Depends on**: 1, 2
- **Action**: (a) Phase 1: before agent dispatch, run `tomlctl items list execution-record.toml --where type=task-completion --where status=done --pluck task_ref`; skip plan tasks whose slug appears. (b) Phase 2 per task finish: append `type=task-completion` via heredoc-stdin idiom. (c) Phase 2 on deviation: append `type=deviation`. (d) Phase 3 per verification command: append `type=verification`. (e) Document `in_progress` derivation rule: live TaskCreate state only, no writers outside `/implement` session.
- **Detail**: ID minting via `tomlctl items next-id execution-record.toml --prefix E`. Two-call write pattern: `items add --json -` then `set last_updated <today>`. Task_ref is the task title slug (e.g., `add-retry-logic`), NOT a positional number. Heredoc idiom example in the schema block serves as the canonical form.
- **Acceptance**:
  - Running `/implement` on a trivial test plan with 2 tasks yields exactly 2 `type=task-completion` entries.
  - Running `/implement` a second time on the same plan produces NO new `type=task-completion` entries (idempotent): capture `tomlctl items list execution-record.toml --where type=task-completion --pluck task_ref` before the second run as set `X`, and after as set `X'`; assert `X == X'`.
  - Verification runs produce one `type=verification` entry per command actually executed.
  - `grep -c "live TaskCreate state only" claude/commands/implement.md` returns ≥ 1.

### 5a. `/plan-update` rewrite `deviation` + `defer` ops to append E-entries [M]
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 1, 2
- **Action**: Rewrite the `deviation` operation to append `type=deviation` entries via heredoc-stdin `tomlctl items add`. Rewrite the `defer` operation to append `type=deferral` entries. Drop D/DF numbering — supersessions use `supersedes_entry = "E<n>"`; legacy rows retain `legacy_id = "D<n>"` / `"DF<n>"` when back-filled by `migrate`.
- **Detail**: Both ops keep their user-facing prompts (evidence gathering, confirmation) but the persistence layer becomes the TOML log. The markdown table emission inside these ops is REPLACED by a call to the render-from-log routine (Task 5c).
- **Acceptance**:
  - `grep -c "D-number" claude/commands/plan-update.md` returns 0 within the `deviation` / `defer` sections.
  - `grep -c "supersedes_entry" claude/commands/plan-update.md` returns ≥ 1.
  - After running `/plan-update deviation` with test input, the log has one new `type=deviation` entry with the expected fields.

### 5b. `/plan-update` rewrite `reconcile` + `status` ops with reconciler contract [M]
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 5a
- **Action**: Rewrite `reconcile` — each of the 2 parallel agents appends a `type=reconcile` entry with `direction` + `findings_count`; follow-up deviations/deferrals append their own entries. Rewrite `status` to implement the reconciler contract: before appending a `type=status-transition`, query the log for existing `task-completion` entries so that the auto-invoked `/plan-update status` from `/implement` Phase 4.5 doesn't double-write. Only append `status-transition` when the flow's `status` actually changes.
- **Detail**: The reconciler contract is the core safety mechanism against `/implement` Phase 4.5's automatic invocation. Document it prominently; the acceptance criterion below verifies no double-writing.
- **Acceptance**:
  - Test path: `/implement` on a fresh plan writes N `task-completion` entries; the auto-invoked `/plan-update status` immediately after produces NO additional `task-completion` entries. `tomlctl items list execution-record.toml --where type=task-completion --count` returns exactly N, not 2N.
  - `grep -c "reconciler contract" claude/commands/plan-update.md` returns ≥ 1.

### 5c. Add render-from-log routine with explicit Session Log column mapping [M]
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 5a, 5b
- **Action**: Add a named render-from-log sub-routine invoked at the end of every op that mutates the log. It regenerates `PROGRESS-LOG.md` in full:
  - Top-of-file marker: `<!-- Generated from execution-record.toml. Do not edit by hand. -->` (NO timestamp substitution).
  - Completed Items table ← `tomlctl items list --where type=task-completion --where status=done`.
  - Deviations table ← `tomlctl items list --where type=deviation`.
  - Deferrals table ← `tomlctl items list --where type=deferral`.
  - Session Log table (columns `| Date | Changes | Commits |`) ← `tomlctl items list --group-by date` → for each bucket: Date = `YYYY-MM-DD` bucket key; Changes = `"<N> entries: <type> × <k>, ..."` summarised by counting entry types within the bucket; Commits = deduplicated union of `commits` arrays across the bucket.
- **Detail**: Session Log grouping works because `date` is YYYY-MM-DD (in DATE_KEYS, one bucket per calendar day). No `@date:` projection needed. Explicit column-mapping prose removes agent ambiguity.
- **Acceptance**:
  - `tomlctl parse .claude/flows/<slug>/execution-record.toml` → render-from-log → diff against a second render-from-log run: output is byte-identical (idempotency).
  - `grep -c '| Date | Changes | Commits |' claude/commands/plan-update.md` returns ≥ 1 (schema documented).
  - Running `/plan-update snapshot` on the test flow produces a PROGRESS-LOG.md whose Session Log has one row per unique `date` across entries, with Changes summarising entry-type counts.

### 5d. Add `migrate` op to back-fill E-entries from existing PROGRESS-LOG.md [M]
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 5a, 5b
- **Action**: Add a new operation `migrate` that reads the existing PROGRESS-LOG.md tables and back-fills equivalent E-entries. For each D-numbered row → `type=deviation` entry with `legacy_id = "D<n>"`. For each DF-numbered row → `type=deferral` entry with `legacy_id = "DF<n>"`. For each Completed Item row → `type=task-completion, status=done` with best-effort fields. For each Session Log row → no-op (rederived from entries). After back-fill, trigger render-from-log (Task 5c) to regenerate PROGRESS-LOG.md.
- **Detail**: One-shot operation. User opts in. Flags existing flows that have pre-migration PROGRESS-LOG.md. Idempotency: re-running `migrate` after it's already been run MUST NOT duplicate entries (detect by scanning for pre-existing `legacy_id` values).
- **Acceptance**:
  - For a test PROGRESS-LOG.md with N D-rows + M DF-rows + K completed rows, running `migrate` produces exactly N `type=deviation` + M `type=deferral` + K `type=task-completion` new entries.
  - Each migrated entry carries the correct `legacy_id` (e.g., row D3 → `legacy_id = "D3"`).
  - Re-running `migrate` produces zero additional entries (idempotent).

### 5e. Rewrite reformat/catchup ops + scrub "Preserve existing deviation numbering" prose [M]
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 5a, 5b, 5c
- **Action**: (a) Rewrite the `reformat` op so that the PROGRESS-LOG.md it regenerates uses the render-from-log routine (not hand-authored D/DF-numbered markdown). Append one `type=checkpoint` entry tagging the restructure. (b) Same for `catchup`. (c) Remove or rewrite the `plan-update.md:340` line ("Preserve existing deviation numbering — if deviations already have D-numbers, keep them. Don't renumber.") — replace with: "Entries carry `legacy_id` for back-compat; no renumbering is required because E-numbers are monotonic."
- **Detail**: This task closes the loop on D/DF purge — after 5a-5d and 5e, no op emits D/DF-numbered markdown.
- **Acceptance**:
  - `grep -c "Preserve existing deviation numbering" claude/commands/plan-update.md` returns 0.
  - Running `/plan-update reformat` on the test flow produces a PROGRESS-LOG.md where all Deviations table rows use `E<n>` identifiers (sourced from `id`), NOT `D<n>`.
  - `/plan-update reformat` appends exactly one `type=checkpoint` entry.

### 6. Derive `[tasks].completed` from log on every context.toml write [S]
- **Files**: `claude/commands/plan-update.md`
- **Depends on**: 5a, 5b
- **Action**: In every op that writes `[tasks]`, insert a derivation step: `completed = (tomlctl items list execution-record.toml --where type=task-completion --where status=done --pluck task_ref | jq -r '.[]' | sort -u | wc -l)`. Document this exact command in the op's TOML write step. `total` remains plan-document-driven. `in_progress` rule documented: only `/implement` during live execution writes this field; `/plan-update` ops leave it untouched.
- **Detail**: Distinct-slug count so retries (failed → done) count as 1 not 2. Document at the top of the op.
- **Acceptance**:
  - Test path: `/implement` completes 3 tasks; one task had a failed attempt before succeeding. `tomlctl get .claude/flows/<slug>/context.toml tasks.completed` returns 3 (not 4).
  - `grep -c "distinct-slug count" claude/commands/plan-update.md` returns ≥ 1.
  - `grep -c "MUST leave .*in_progress.* untouched" claude/commands/plan-update.md` returns ≥ 1.

### 7. Purge D/DF-number references across plan-update.md and review-plan.md [S]
- **Files**: `claude/commands/plan-update.md`, `claude/commands/review-plan.md`
- **Depends on**: 5a, 5b, 5c, 5d, 5e
- **Action**: Walk the specific sites surfaced during review and update or remove each:
  - `plan-update.md:148` (D-number assignment in `deviation` op) → already replaced by 5a; verify
  - `plan-update.md:156` (DF-number assignment in `defer` op) → already replaced by 5a; verify
  - `plan-update.md:231` (D-number reference in reconcile narrative) → update to E-number language
  - `plan-update.md:293-304` (PROGRESS-LOG.md table schemas for Deviations + Deferrals) → already replaced by 5c's render contract; verify
  - `plan-update.md:340` ("Preserve existing deviation numbering") → already removed in 5e; verify
  - `plan-update.md:454` (final summary/guidance referencing D/DF numbering) → update to E-number language
  - `review-plan.md:105` (docstring referencing D/DF numbering in plan-mode context) → update to E-number language
- **Detail**: This is an audit pass; most sites will already be resolved by 5a-5e. Verify each explicitly and patch any that slipped through.
- **Acceptance**:
  - `grep -nE '\bD[0-9]+\b' claude/commands/plan-update.md claude/commands/review-plan.md` returns ONLY matches inside literal `legacy_id` field contexts (e.g., `legacy_id = "D3"` documentation examples) — no narrative references to D-numbers as active identifiers.
  - `grep -nE '\bDF[0-9]+\b' claude/commands/plan-update.md claude/commands/review-plan.md` same rule.

### 8. End-to-end verification [M]
- **Files**: — (manual)
- **Depends on**: 3, 4, 5a-5e, 6, 7
- **Action**: Execute this sequence against a fresh repo state:
  1. `/plan-new` on a trivial 2-task planning prompt → confirm `execution-record.toml` is initialised with `schema_version = 1`, empty `[[items]]`.
  2. `/implement` → confirm 2 `type=task-completion` entries + N `type=verification` entries; `[tasks].completed = 2`.
  3. `/implement` again → confirm 0 new task-completion entries (idempotency).
  4. `/plan-update deviation` with test input → confirm 1 new `type=deviation` entry; PROGRESS-LOG.md regenerated with the new row.
  5. `/plan-update defer` → confirm 1 new `type=deferral` entry.
  6. `/plan-update snapshot` → confirm render output matches log.
  7. Render PROGRESS-LOG.md twice, diff → byte-identical.
  8. `scripts/verify-shared-blocks.sh` → passes.
  9. `cargo build/test/clippy --manifest-path tomlctl/Cargo.toml` → pass.
- **Acceptance**: Every step above completes without error; the three sources of truth (execution-record.toml, PROGRESS-LOG.md, context.toml `[tasks]`) remain consistent after each step.

## Dependency Graph

Batch 1 (sequential, same-file contention on plan-new/plan-update/implement): Task 1 → Task 2
Batch 2 (parallel, after Batch 1): Task 3, Task 4, single-agent sequence of 5a → 5b → 5c → 5d → 5e (all in plan-update.md, sequential within one agent)
Batch 3 (after Batch 2): Task 6
Batch 4 (after Batch 3): Task 7 (audit pass)
Batch 5 (manual, after Batch 4): Task 8

Max parallel agents in any batch: 3 (Task 3, Task 4, and one agent running 5a-5e in sequence on plan-update.md). File counts respected.

## Verification

See "Verification Commands" and Task 8. Three-way consistency (log ↔ rendered markdown ↔ context.toml counters) is the gate.

## Risks

- **Risk: widening shared-block parity from 4 to 8 files in one task (Task 1) has a large blast radius** — Mitigation: single-agent ownership; local `scripts/verify-shared-blocks.sh` before commit; the pinned-hash test at `tomlctl/src/cli.rs:1182-1284` fails loudly on drift; task acceptance explicitly runs both.
- **Risk: existing flows lack `execution_record` in `[artifacts]`** — Mitigation: Task 1's tightened per-key absent-block contract ("if any canonical key within `[artifacts]` is missing, compute and write back") covers auto-back-fill. Existing `status = complete` flows are skipped by resolution.
- **Risk: `tomlctl items orphans` / `find-duplicates` emit garbage when mis-invoked against execution-record.toml** — Mitigation: Task 2's schema contract explicitly forbids these subcommands against execution-record files; prose is prominent in the shared block.
- **Risk: PROGRESS-LOG.md external readers break on regeneration semantics** — Mitigation: top-of-file `<!-- Generated from ... -->` marker; rendered content matches existing column schemas byte-for-byte (Completed Items, Deviations, Deferrals, Session Log).
- **Risk: double-write when `/implement` writes task-completion then Phase 4.5 auto-invokes `/plan-update status`** — Mitigation: reconciler contract in Task 5b; acceptance criterion verifies N entries, not 2N.
- **Risk: `[tasks]` counters drift from log if derivation is skipped on one write path** — Mitigation: Task 6 specifies derivation as a required step in every `[tasks]` write; Task 8 end-to-end verifies the three-way consistency.
- **Risk: `task_ref` positional drift if `/plan-update reformat` renumbers plan tasks** — Mitigation: `task_ref` is an opaque title slug (not a positional number). Reformatting the plan renames task headings only if the user explicitly changes them; slug stability is the user's responsibility. Schema documents this trade-off.
- **Risk: append-only log unbounded growth** — Mitigation: out of scope for this plan; document `compact` op as future work. Estimated ~100 entries/flow × ~500 bytes = ~50KB per flow, negligible.
- **Risk: `migrate` op may not be needed in this repo** — Mitigation: `docs/plans/` doesn't exist yet, so no legacy PROGRESS-LOG.md on disk; `migrate` is a no-op here but remains useful for any flow whose PROGRESS-LOG.md was hand-authored elsewhere and pulled into the repo.
- **Risk: rendering introduces non-determinism (e.g. date-of-run in a header)** — Mitigation: Task 5c explicitly forbids timestamp substitution in the rendered prose; acceptance criterion verifies byte-identical render-then-render.
