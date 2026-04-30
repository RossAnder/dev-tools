# Plan: Flow-commands hardening — `/review-plan` persistence, parallel-dispatch rewrite, lens restructure, research-phase strengthening, tomlctl skill polish, audit fixes

**Plan path**: `docs/plans/flow-commands-hardening.md`
**Flow slug**: `flow-commands-hardening`
**Created**: 2026-04-24
**Status**: Draft

## Context

The flow commands (`/plan-new`, `/review-plan`, `/implement`, `/review`, `/review-apply`, `/optimise`, `/optimise-apply`, `/plan-update`) and the `tomlctl` skill that backs them have accumulated five classes of issue:

1. **`/review-plan` is fire-and-forget.** No persistence. The user's biggest workflow pain: manually folding findings back into the plan document, lost if the session ends. Fix: persist findings and offer auto-merge with a dry-run preview.
2. **`/implement`'s parallel-dispatch instruction fails** despite being ~900 words with a pre-send checklist. `/review-apply` and `/optimise-apply` fan out reliably with ~45-word mechanical rules. Port the working formula.
3. **`/review` lens structure is mis-aligned with the user's priorities.** Security gets a full agent slot despite being an essentials-only concern. Testability, diagnostics, and developer-experience — higher-value lenses — have no explicit owner. Decision: narrow Agent 2 (Security) to essentials + quick wins AND add a 5th agent for Testability, Diagnostics & DX. `/review` goes from 4 to 5 agents, matching `/optimise`'s shape.
4. **`/plan-new`'s research phases under-produce** for downstream consumers. Research Notes are freeform prose; library versions are not pinned; Phase 5 trigger is subjective; research findings do not explicitly influence Phase 6 design decisions. This forces `/implement` Phase 1 to re-research APIs the plan should already have documented.
5. **`tomlctl` skill has drift, gaps, and bloat.** The `items apply --ops` per-op `unset` form is buried in prose. `.lock` and `.sha256` sidecar semantics are split across three sections. No quick-reference table for the 10 most-common patterns. Filter-operator prose bloats 45 lines where a table would take 15. Threat-model prose (30 lines) is policy documentation, not API reference.

Plus a batch of audit fixes — stale R48 reference, vague thresholds, forward references, date-vs-timestamp ambiguity, missing fallbacks, duplicate explanations.

Cross-cutting style directive: every prose change optimises for **agent clarity**. Prefer terse imperative instructions, concrete criteria, structural cues. Cut narrative, emotional framing, and repetition-for-emphasis.

## Scope

- **In scope**: prose edits to 8 command files under `claude/commands/`; schema evolution inside two shared blocks (`flow-context`, `ledger-schema`); skill-file edit at `claude/skills/tomlctl/SKILL.md`; new artifact file format `plan-review-findings.toml`; new persistence + auto-merge flow in `/review-plan`; `/review` lens restructure (narrow Agent 2; add Agent 5); `/plan-new` research-phase strengthening (Phase 3, Phase 5, Phase 6 handoff).
- **Out of scope**: renaming commands or slugs; restructuring the flow-resolution order or execution-record schema beyond the clarifications listed; any `tomlctl` Rust changes; any `.githooks/` or `scripts/` changes.
- **Affected areas**:
  - `claude/commands/plan-new.md`
  - `claude/commands/review-plan.md`
  - `claude/commands/implement.md`
  - `claude/commands/review.md`
  - `claude/commands/review-apply.md`
  - `claude/commands/optimise.md`
  - `claude/commands/optimise-apply.md`
  - `claude/commands/plan-update.md`
  - `claude/skills/tomlctl/SKILL.md`
- **Estimated file count**: 9 unique files. No source-code files change.

## Exploration Notes

- **`/review-plan` persistence**: zero today. Four lens-agents (Feasibility/Dependencies, Completeness/Scope, Agent-Executability/Clarity, Risk/External Validity). Severity-tiered (Critical/Warning/Suggestion). Anchored via `[plan section/task]` bracket prefix. Resolves flow (5-step) for reads only; does not write `status = "review"`; no end-of-turn next-step prompt.
- **R48 note** is in the `flow-context` shared block at line 58 of all 8 command files. The note's "3 self-healing prose copies" phrasing refers to the bootstrap prose in `plan-new`/`plan-update`/`implement`; the meta-note itself is in 8 files.
- **`/optimise-apply` freshness gate**: "strictly after `last_updated` date" — compares `%cI` ISO 8601 timestamp against a YYYY-MM-DD date. Same-day commits never trigger staleness; ambiguity undocumented.
- **`verified-clean` scope**: review-only in shared `ledger-schema`. `optimise-apply.md:400` has a per-file note that `/optimise` uses `wontapply` instead. Move into shared block for discoverability.
- **Parallel-dispatch working formula** (`/review-apply:472`, `/optimise-apply:469`): single-paragraph mechanical rule, ~45 words, no checklist. `/implement`'s current 900-word section with pre-send checklist and emotional framing has been observed to fail anyway.
- **tomlctl skill (695 lines)**:
  - `--where-not` and `--where-missing` ARE already documented (SKILL.md:138, :145) — the gap is surfacing them in the new filter-operator table, not documenting them.
  - `items apply --ops` per-op `unset` array is mentioned only in prose at SKILL.md:455.
  - Filter-operator section (lines 114-159, 45 lines) can collapse to a 15-line table.
  - Integrity-sidecar section (lines 650-680, 30 lines) is mostly threat-model prose.
  - `blocks verify` subcommand (lines 312-330) is infrastructure-only; no command invokes it.
  - **`tomlctl items find-duplicates` and `items orphans` hardcode the review/optimise ledger schema.** Running them against the new `plan-review-findings.toml` emits garbage — the new schema must include a do-not-invoke callout, parallel to the existing warning in the execution-record-schema shared block.
- **`/plan-new` research phases**: Phase 3 is freeform prose without version pinning. No library-enumeration step. Phase 5 trigger is subjective. Phase 6 doesn't mandate re-reading Research Notes. Research budget (2 agents, 500 words, 10 findings) is under-sized for polyglot codebases.
- **`/review` lens structure**: current 4 agents (Quality, Security, Architecture, Completeness). Security over-weighted; testability/diagnostics/DX absent. Decision: narrow Agent 2 + add Agent 5.

## Research Notes

_No external research required — this is an internal-tooling change. All information comes from reading the nine files in scope plus `scripts/shared-blocks.toml`._

## User Decisions

- **Review lens restructure**: narrow Agent 2 (Security) to essentials + quick wins AND add a 5th agent for Testability, Diagnostics & Developer Experience. User refinement: avoid prescribing a checklist of specific security item types — prefer natural discovery from research, avoid over-zealousness, enforce a hard cap of 5 findings. Agent 2's brief frames the lens and sets the cap; it does NOT enumerate vulnerability classes.
- **Persistence location** for `/review-plan` findings: new artifact `plan-review-findings.toml` parallel to `review-ledger.toml` / `optimise-findings.toml`.
- **Auto-merge selector**: single `AskUserQuestion` with `multiSelect` over severity. Default: Critical + Warning.
- **Revised-plan preview**: write to sibling `<plan>.revised.md`; follow-up `AskUserQuestion` Accept / Keep both / Discard.
- **R48 note**: delete outright. No relocation.
- **`verified-clean` asymmetry**: move the per-file note at `optimise-apply.md:400` into shared `ledger-schema`.
- **`/implement` parallel-dispatch**: replace the entire current section with the working formula.
- **New review category**: add `testability` to the category vocabulary.
- **Research-phase strengthening**: structured Research Notes format with version pinning; Phase 3 library-enumeration sub-step (scoped to manifests intersecting the plan's `scope` globs); mechanical Phase 5 trigger; explicit Phase 6 re-read step; broaden research focus types.
- **`suggested_edit` merge contract**: mechanical `anchor_old` + `anchor_new` string pair, NOT natural-language prose. Deterministic and testable.
- **Q2 empty-answer default**: Keep both (NOT Accept). Prevents silent plan overwrite in `acceptEdits` mode where `AskUserQuestion` auto-completes empty (Claude Code issues #29618/#29547).
- **`.revised.md` preservation**: keep the sidecar for one cycle after Accept (deleted on next run) for reversible Accept.
- **tomlctl skill scope**: add quick-reference table; common-recipes section; consolidate sidecar semantics; surface `--where-not`/`--where-missing` in new table; trim filter-operator prose and threat-model prose.

## Approach

Three concurrent tracks after a sequential schema-evolution phase.

### Phase A: Schema evolution (sequential, one agent, multiple files)

**`flow-context` shared block** (8 carrier files):
- Delete the parenthetical R48 follow-up note from the `[artifacts]` paragraph.
- Add `plan_review_findings` as a new canonical `[artifacts]` key:
  - Extend the schema example block to include the new line.
  - Extend the field-responsibilities "currently: ..." enumeration.
  - Note the self-healing path: commands compute `plan_review_findings = .claude/flows/<slug>/plan-review-findings.toml` from `slug` when absent and write back on next TOML write. Unlike `execution_record`, no atomic bootstrap is needed — `/review-plan` is the sole writer and creates the file on first persistence.

**`ledger-schema` shared block** (4 carrier files: `review`, `review-apply`, `optimise`, `optimise-apply`):
- Add `testability` to review category vocabulary. Updated list: `quality | security | architecture | completeness | db | testability | verified-clean`.
- Adjacent to the `verified-clean (review only)` disposition line, add one sentence: "`/optimise` has no `verified-clean` counterpart — bytes-written findings land in `applied`, already-correct cases land in `wontapply` with rationale."

### Phase B: Per-file non-shared-block edits (parallel, disjoint files)

**`review.md`** (audit fixes + lens restructure):

*Audit fixes*:
- TaskCreate/TaskUpdate ephemerality sentence at the Task-tracking sub-section: "These TaskCreate/TaskUpdate entries are ephemeral to this `/review` run and do NOT write `context.toml.[tasks]`."
- Move or parenthesise the small-diff shortcut forward reference (currently referenced at line 327, defined at line 424).
- Tighten Agent 3 "Do NOT flag" thresholds with concrete criteria.
- Add asymmetry-note mirror-reference.

*Lens restructure*:
- Rewrite Agent 2 (Security) brief to narrow focus **without prescribing an enumerated list of item types**:

  > "Focus on security **essentials and quick wins** appropriate to what the changed code does. Apply contextual judgement: not every feature needs every possible protection, and the goal is to flag the top issues that would actually matter if this shipped — not to enumerate every possible concern. Research the specific trust boundaries, data flows, and attack surfaces the changed code introduces via Context7 and WebSearch where current guidance matters. Bring forward only the highest-signal findings — real risks with plausible attack vectors in the actual deployment. Do NOT produce exhaustive threat-model output, theoretical vulnerabilities, or defence-in-depth laundry lists. **Hard cap: 5 findings.** Zero findings is a valid outcome — if nothing material surfaces, return an empty security pass rather than padding."

- Add new Agent 5: Testability, Diagnostics & Developer Experience.
  - **Testability** — public surface is testable (no hidden globals, seams for mocking); new tests accompany new code where project convention requires it; tests are deterministic and name what they cover; regression risk for changed code is covered.
  - **Diagnostics & observability** — error messages name the operation that failed, the input, and expected vs actual state; logging at the right level with enough context to triage from logs alone; metrics / traces instrumented where project pattern expects them.
  - **Developer experience** — public APIs, CLIs, and configuration surfaces are discoverable (help text, self-describing errors, sensible defaults); naming and error messages read well to a first-time user.
  - Category: `testability` (new ledger-schema value from Phase A).
  - Cap: 15 findings per agent, ceiling 20.

- Update "four" → "five" references in: **dispatch rule** (`Launch **all four** review agents`), **task-tracking** (`4 tasks for a normal run`), **small-diff shortcut prose** (`instead of four specialized ones`), **design note** (`/review's four lenses are language-agnostic`).
- **Anchor each "four" replacement to its surrounding context phrase.** Do NOT use global `replace_all` on the bare word `four`. Preserve `review.md:40` (`four string values` in the status enum, inside the shared `flow-context` block) verbatim.

**`optimise.md`** (audit fixes only):
- TaskCreate/TaskUpdate ephemerality sentence.
- Collapse `## Optimization Focus` duplication.
- `tomlctl items orphans` fallback sentence.
- Asymmetry-note mirror-reference.

**`implement.md`** (audit fixes + parallel-dispatch rewrite):
- Delete lines 360-393 entirely (anti-pattern section, commit-to-completion gate, pre-send checklist). Replace with working-formula paragraph:

  > **Parallel dispatch rule.** Emit one `Agent` tool-use block per task in this batch, all within the same response. Do not launch them across turns. Do NOT reduce the agent count — launch the full complement for each batch. The harness fans out concurrently only when blocks arrive in one response; a second `Agent` call on a later turn runs *after* the first returns. (Pattern: same as `/review-apply` and `/optimise-apply`, which fan out reliably with this rule.)

- Reposition the Prompt-cache tip immediately above the rule.
- Keep the existing "Batch execution" numbered list (lines 422-430).
- Add `<record>` shorthand recap near Phase 2's first use.
- Phase 4 Implementation Summary: add a `### Next Steps` section.

**`optimise-apply.md`** (freshness gate + cleanup):
- Replace "strictly after" with: "If any file's newest commit timestamp is at or after 00:00:00Z on the day AFTER `last_updated`, the ledger is stale with respect to this selector."
- Add UTC note: "The comparison is UTC-based; users in non-UTC timezones may observe staleness firing at different wall-clock times than the calendar rule suggests."
- Remove the per-file `verified-clean` asymmetry note at line 400 — now in shared block.

**`plan-new.md`** (scope-assessment + research-phase strengthening):

*Scope assessment (Phase 1 step 3)*:
- Replace vague split-decision prose with explicit criteria: "Propose splitting when ANY of these hold: (a) features could ship independently (no code dependency, independent success measures, reviewable separately); (b) ≥4 unrelated modules with no shared refactoring; (c) combines a refactor and a new feature."

*Phase 3 research strengthening*:
- Add **Library enumeration** sub-step before agent dispatch. Orchestrator reads dependency-manifest file(s) intersecting the plan's `scope` globs (package.json, Cargo.toml, pyproject.toml, go.mod, *.csproj). For monorepos, only enumerate workspace packages whose directories intersect scope. Extract each dependency + version. Hand scope-filtered "libraries to research" list (typically ≤ 20) to each research agent.
- Mandate structured Research Notes record format:

  ```
  - **Library/API**: [name] [version from manifest]
  - **Source**: [Context7 query reference or URL]
  - **Finding**: [one-line — API signature, deprecation, behaviour]
  - **Details**: [2-3 sentence explanation with exact parameter names / method signatures]
  - **Impact on plan**: [how this finding shapes the design, or "no change"]
  ```

- Handle "Context7 returns nothing": fall back to WebSearch, record the absence, flag in Phase 6.
- Handle "Context7 returns multiple library IDs": agent states disambiguation explicitly; if ambiguous, surfaces as Phase 4 question.
- Require version-pinning: every library-referencing finding cites exact version from the manifest.
- Broaden research focus types: changelog/breaking-change research, benchmarking research (when plan proposes multiple viable approaches), undocumented-behaviour research (StackOverflow / GitHub Issues).

*Phase 5 trigger (mechanical)*:
1. For each Phase 4 answer A, extract key terms (library/API/pattern names).
2. Grep Research Notes for each key term.
3. If all terms appear, mark A as "covered".
4. If all answers are "covered", skip Phase 5 and note the skip.
5. Override: if grep matched the library name but not the specific API referenced in the answer, run Phase 5 anyway.

*Phase 6 handoff*:
- Insert a new first step: **Review research findings**. Re-read `## Research Notes`. For each finding with non-empty "Impact on plan", note the constraint. List deprecations / version-specific behaviours that force design choices. Downstream steps reference this constraints list.

### Phase C: tomlctl skill polish (parallel with Phase B)

**`claude/skills/tomlctl/SKILL.md`**:

*Additions (execute BEFORE trimming)*:
1. Quick reference table at top (8-10 rows): common read/write/query patterns.
2. Common recipes section (~4-6 patterns): append to execution-record, dedup-by-field, fetch next ID, filter open + count, bulk transition.
3. `--verify-integrity` support matrix listing every read subcommand that accepts the flag. Clarify per-subcommand, not global.
4. Consolidated Sidecar files section covering `.sha256` and `.lock`.
5. Surface `--where-not` (SKILL.md:138) and `--where-missing` (SKILL.md:145) in the new filter-operator table — they are documented; the gap is discoverability.
6. Document `items apply --ops` per-op `unset` array prominently in op-structure section.

*Trims (execute AFTER additions)*:
7. Replace 45-line filter-operator prose (lines 114-159) with a ~15-line table.
8. Trim 30-line integrity-sidecar threat-model prose (lines 650-680) to ~10 lines. Move "not a MAC" policy note to a footnote.
9. Move `blocks verify` (lines 312-330) to an "Advanced / maintenance" appendix.

### Phase D: `/review-plan` persistence + auto-merge (depends on Phase A)

**`claude/commands/review-plan.md`**:

*New artifact schema* (`.claude/flows/<slug>/plan-review-findings.toml`):

```toml
schema_version = 1
last_updated = 2026-04-24
round = 1

[[items]]
id = "P1"
review_round = 1
severity = "critical"
category = "feasibility"
plan_section = "### 3. optimise.md audit fixes"
anchor_old = "- **Action**: apply the four optimise.md audit fixes"
anchor_new = "- **Action**: apply the five optimise.md audit fixes including the Design Note re-anchor"
summary = "Action count mis-states task scope after re-anchor addition"
status = "open"
```

Required fields: `id` (P{n} monotonic), `review_round`, `severity` ∈ {critical, warning, suggestion}, `category` ∈ {feasibility, completeness, executability, risk}, `plan_section` (markdown heading anchor as literal string, copied verbatim from the plan), `summary`, `status` ∈ {open, merged, discarded}.

Optional: `description` (longer), `anchor_old` (exact substring that already exists in the plan file under `plan_section`), `anchor_new` (replacement substring). `anchor_old` + `anchor_new` together form the mechanical merge contract — **both required for auto-merge to act on a finding. Findings with only `summary`/`description` and no anchor pair are advisory-only and skipped by the merger.**

**Schema callouts (verbatim in `review-plan.md`)**:
- `tomlctl items find-duplicates` and `tomlctl items orphans` hardcode the review/optimise ledger schema and MUST NOT be invoked against `plan-review-findings.toml` — they will emit garbage. Parallel to the existing warning in the `execution-record-schema` shared block.
- `items next-id --prefix P` is the supported ID path; `items list` / `items add-many` / `items apply --ops -` are supported.

*New Step 3.5: Persist findings*:
1. Compute `plan_review_findings_path = context.toml.artifacts.plan_review_findings` (or derive from slug if absent; write back per self-healing contract).
2. Mint monotonic P-IDs: `tomlctl items next-id <path> --prefix P`.
3. Batch-write: `tomlctl items add-many <path> --defaults-json '{"review_round":<n>, "status":"open"}' --ndjson -` with all findings.
4. `tomlctl set <path> last_updated <today>` and `tomlctl set <path> round <n>`.

*New Step 4: Auto-merge offer (on turn-end)*:
1. Count findings by severity. If zero, output "No findings — plan is clean." and end.
2. **`AskUserQuestion` (Q1)**: multiSelect severity `[Critical, Warning, Suggestion]`, default Critical + Warning.
   - **Empty-answer rule**: if the response is empty (`acceptEdits` mode / skill-hosted / headless per Claude Code issues #29618/#29547), treat as "zero selected" — skip merge entirely. Persist findings only. Do NOT proceed to Q2.
3. If zero selected → persist only, no merge. Output: "Findings persisted; auto-merge skipped. Re-run interactively to merge."
4. Filter to selected severities **with both `anchor_old` AND `anchor_new` present**.
5. **Conflict detection**: group by `plan_section`. If >1 finding in a group has non-empty `anchor_old`, emit `[conflict: plan_section="..."; findings=P3, P7] — manual merge required` and skip all findings in the group. Non-conflicting findings still apply.
6. **Mechanical merge**: for each surviving finding, locate `anchor_old` as a substring in the plan file under the `plan_section` heading. If found exactly once, replace with `anchor_new`. Otherwise log `[merge-failed: P{n} — anchor_old not found uniquely in section "..."]` and skip. Apply in P-id monotonic order.
7. Materialise revised content via `Write` to sibling file: **replace the plan file's trailing `.md` with `.revised.md`** (do not append — e.g. `docs/plans/flow-commands-hardening.md` → `docs/plans/flow-commands-hardening.revised.md`). For multi-file plans, materialise only the outline at `<outline-dir>/00-outline.revised.md`; **detail files are not rewritten by auto-merge v1**.
8. **Pre-existing sibling**: if `<plan>.revised.md` already exists, rename to `<plan>.revised.prev.md` before writing the new revision. Cheap rollback.
9. Console summary: N applied, K conflicts skipped, M merge-failures, list of `plan_section` → summary for applied findings.
10. **`AskUserQuestion` (Q2)**: Accept / Keep both / Discard.
    - **Default**: Keep both (NOT Accept). Accept is irreversible; default-to-Accept + auto-mode empty-answer = silent plan overwrite.
    - **Empty-answer rule**: empty → treat as Keep both.
11. Apply chosen action:
    - **Accept**: `Write` revised over original; keep `<plan>.revised.md` for one cycle for post-hoc inspection. Transition matching findings to `status = "merged"`.
    - **Keep both**: no mutation; findings stay `open`.
    - **Discard**: delete `<plan>.revised.md`; transition findings to `status = "discarded"`.
    - The `<plan>.revised.prev.md` from the prior run is deleted on the NEXT run's step 8.
12. `tomlctl set <path> last_updated <today>`.

*Handoff*: no contract change to `/implement`. Findings file persists for audit. Subsequent `/review-plan` runs increment `review_round`. **Dedup on re-run**: findings already transitioned to `merged` or `discarded` are ignored by lens-agents; agents receive `open`-status items as prior context and check dedup via `(plan_section, anchor_old)`.

*Agent-count clarifying note*: `/review-plan` keeps its 4 lens-agents (Feasibility, Completeness, Executability, Risk). These are plan-review lenses, distinct from `/review`'s 5 code-review lenses. Add one line near the top of `review-plan.md` to prevent confusion.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
sharedblocks: bash scripts/verify-shared-blocks.sh
```

No cargo changes expected (Markdown-only edits); commands listed so `/implement`'s verification agent does not re-discover them.

## Tasks

### 1. Shared-block evolution + non-shared category-list update [M]
- **Files**: all 8 files under `claude/commands/` (byte-identical shared-block content across carriers); plus a non-shared-block edit in `review-apply.md`.
- **Depends on**: —
- **Action**:
  - `flow-context` block (8 files): delete R48 parenthetical; add `plan_review_findings` to schema example + field responsibilities.
  - `ledger-schema` block (4 files — review, review-apply, optimise, optimise-apply): add `testability` to review category vocabulary; add verified-clean asymmetry sentence.
  - **Non-shared-block**: `review-apply.md:454` mixed-category cluster enumeration `quality + security + architecture + completeness + db` → `... + db + testability`.
- **Detail**: single agent edits all files in one pass. Use `Edit` with explicit `old_string`/`new_string` pairs captured from one carrier before the pass (anchored to content, not line numbers). Run `bash scripts/verify-shared-blocks.sh` at end.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0; `grep -rc "R48" claude/commands/` → 0; `grep -rl "plan_review_findings" claude/commands/ | wc -l` ≥ 8; `grep -rl "testability" claude/commands/ | wc -l` ≥ 4; `review-apply.md:454` cluster includes `testability`.

### 2. review.md (audit fixes + lens restructure to 5 agents) [L]
- **Files**: `claude/commands/review.md`
- **Depends on**: 1
- **Action**: 4 audit fixes; rewrite Agent 2 (Security) to essentials + quick wins with cap 5; add Agent 5 (Testability/Diagnostics/DX); update "four" → "five" in dispatch rule, task-tracking, small-diff prose, design note. **Anchor each "four" replacement to its context phrase**; preserve `review.md:40` (`four string values`) verbatim.
- **Acceptance**: `grep -c "Launch \*\*all five\*\*"` ≥ 1; `grep -c "testability"` ≥ 2; `grep -c "essentials and quick wins"` ≥ 1; `grep -c "four string values"` ≥ 1 (status-enum preserved); shared-blocks parity passes.

### 3. optimise.md audit fixes [S]
- **Files**: `claude/commands/optimise.md`
- **Depends on**: 1
- **Action**: 4 audit fixes (TaskCreate ephemerality, Optimization Focus dedup, tomlctl orphans fallback, asymmetry mirror-reference). Re-anchor stale `review.md:315` reference to a heading anchor rather than a line number.
- **Acceptance**: shared-blocks parity passes; Optimization Focus re-explanation removed; `grep "review.md:315" claude/commands/optimise.md` → 0.

### 4. implement.md (parallel-dispatch rewrite + small additions) [M]
- **Files**: `claude/commands/implement.md`
- **Depends on**: 1
- **Action**: delete lines 360-393; replace with working-formula paragraph; reposition Prompt-cache tip above the rule; keep Batch execution numbered list; add `<record>` shorthand recap; add `### Next Steps` to Phase 4 Implementation Summary.
- **Acceptance**: line count drops by ≥30; `grep -i "insidious\|keeps happening\|natural stopping" claude/commands/implement.md` → 0; shared-blocks parity passes.

### 5. optimise-apply.md freshness + cleanup [S]
- **Files**: `claude/commands/optimise-apply.md`
- **Depends on**: 1
- **Action**: replace "strictly after" with `last_updated + 1 day` UTC rule + UTC note; delete redundant per-file `verified-clean` note at line 400.
- **Acceptance**: `grep "strictly after"` → 0; `grep -c "00:00:00Z\|UTC"` ≥ 1; shared-blocks parity passes.

### 6. plan-new.md (scope-assessment + research-phase strengthening) [L]
- **Files**: `claude/commands/plan-new.md`
- **Depends on**: 1
- **Action**: Phase 1 step 3 concrete criteria; Phase 3 Library-enumeration sub-step + structured Research Notes record format + Context7 no-match/multi-match handling + broadened research focus types; Phase 5 mechanical trigger; Phase 6 "Review research findings" first step.
- **Acceptance**: `grep -c "Library/API\|Library enumeration"` ≥ 2; mechanical Phase 5 check documented; Phase 6 opens with re-read step; shared-blocks parity passes.

### 7. review-plan.md persistence + auto-merge feature [L]
- **Files**: `claude/commands/review-plan.md`
- **Depends on**: 1
- **Action**: implement Phase D as specified. Includes schema with `anchor_old`/`anchor_new` mechanical-merge contract, both schema callouts, Step 3.5 persistence, Step 4 auto-merge with empty-answer rules, conflict detection, `.revised.md` naming rule, multi-file handling (outline-only v1), `.revised.prev.md` rollback sidecar, re-run dedup, 4-vs-5-agents clarifying note.
- **Acceptance**: `grep -c "plan_review_findings"` ≥ 3; `grep -c "AskUserQuestion"` ≥ 2; `grep -c "anchor_old\|anchor_new"` ≥ 4; `grep -c "find-duplicates\|items orphans"` ≥ 1; `grep -c "Keep both"` ≥ 1; `grep -c "revised.prev.md\|revised.md"` ≥ 2; inline schema example present; shared-blocks parity passes.

### 8. tomlctl SKILL.md polish [M]
- **Files**: `claude/skills/tomlctl/SKILL.md`
- **Depends on**: —
- **Action**: execute additions (Quick Reference, Common recipes, `--verify-integrity` matrix, Sidecar consolidation, surface `--where-not`/`--where-missing`, prominent `unset` array) BEFORE trims (filter-operators table, threat-model trim, `blocks verify` appendix).
- **Acceptance**: net line count drops by ≥20; `grep -c "^|"` increases by ≥15; Quick Reference heading within first 50 lines; `grep -c "find-duplicates\|orphans"` ≥ current count; `--verify-integrity` matrix present.

## Dependency Graph

```
Task 1 (shared-block evolution) ──┐
                                  │
Task 8 (SKILL.md polish) ─────────┤  (parallel with Task 1 — different files)
                                  │
                                  ▼
  Tasks 2, 3, 4, 5, 6, 7 (all parallel after Task 1 completes — 6 agents exceed cap)
```

- **Batch 1** (parallel): Task 1 + Task 8. Disjoint files.
- **Batch 2** (parallel, after Batch 1): six tasks exceed `/implement`'s 3-4 cap. Effort classification: **L = {2, 6, 7}**, **M = {4}**, **S = {3, 5}**. Sub-batch as **{2, 6, 7}** (three L) then **{3, 4, 5}** (M + two S).

## Verification

1. `bash scripts/verify-shared-blocks.sh` → exit 0.
2. `cargo build --manifest-path tomlctl/Cargo.toml` → succeed.
3. `cargo test --manifest-path tomlctl/Cargo.toml` → pass.
4. Grep assertions (use `-rl` for directory recursion):
   - `grep -rc "R48" claude/commands/` → 0
   - `grep -rl "plan_review_findings" claude/commands/ | wc -l` → ≥ 8
   - `grep -rl "testability" claude/commands/ | wc -l` → ≥ 4
   - `grep -i "insidious\|keeps happening\|natural stopping" claude/commands/implement.md` → 0
   - `grep "strictly after" claude/commands/optimise-apply.md` → 0
   - `grep -c "Launch \*\*all five\*\*" claude/commands/review.md` → ≥ 1
   - `grep -c "four string values" claude/commands/review.md` → ≥ 1
   - `grep -c "Library enumeration" claude/commands/plan-new.md` → ≥ 1
   - `grep -c "anchor_old\|anchor_new" claude/commands/review-plan.md` → ≥ 4
   - `grep "review.md:315" claude/commands/optimise.md` → 0
5. Manual smoke tests:
   - `/review-plan` against a known plan file — findings persist; AskUserQuestion flow fires; revised-plan file materialises; Accept/Keep/Discard transitions work.
   - `/review` on small scope — 5 agents dispatch in parallel; Agent 5 emits `category = "testability"`.
   - `/plan-new` on a mock task — Phase 3 reads dependency manifest; Research Notes follow structured format.

## Risks

- **Shared-block parity breakage during Task 1.** Three concurrent changes must land byte-identically across their carrier sets. Mitigation: single agent owns Task 1; final step runs `bash scripts/verify-shared-blocks.sh`.
- **Shared-block parity check is byte-only, not semantic.** Task 7 acceptance cross-checks `plan_review_findings` spelling between reader (`review-plan.md`) and writer (`plan-new.md`). Hook is pre-commit only — no CI fallback.
- **`AskUserQuestion` silently auto-completes in `acceptEdits`/skill-hosted/headless modes** (Claude Code #29618, #29547). Mitigation: explicit empty-answer rules (Q1 → skip merge; Q2 → Keep both; never auto-Accept).
- **`tomlctl items find-duplicates`/`items orphans` emit garbage against the new artifact.** Mitigation: Task 7 schema includes do-not-invoke callouts.
- **Auto-merge conflicts and merge-failures.** Mitigation: Task 7 mechanical merge reports per-finding `[conflict: ...]` / `[merge-failed: ...]` instead of producing ambiguous output.
- **`/review` 5-agent parallel dispatch.** `/optimise` already runs 5-wide reliably; scaling from 4 should be safe. Mitigation: keep the working-formula dispatch rule.
- **`testability` category overlap with `completeness`.** Mitigation: Agent 5 explicitly owns tests; Agent 4's "tests not written" becomes a pointer to Agent 5. Dedup rule handles cross-agent duplication.
- **Research-phase strengthening increases `/plan-new` runtime.** Net effect: deeper plans, faster downstream execution (less re-research). Accept the trade-off.
- **tomlctl skill drift from binary.** Mitigation: verify each documented operator against `tomlctl --help` before committing Task 8.
- **Task 2-7 batch exceeds 3-4 parallel cap.** Mitigation: sub-batching {2, 6, 7} + {3, 4, 5}.
