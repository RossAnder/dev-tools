# Plan: Flow-commands hardening — `/review-plan` persistence, parallel-dispatch rewrite, lens restructure, research-phase strengthening, tomlctl skill polish, audit fixes

**Plan path (post-approval)**: `docs/plans/flow-commands-hardening.md`
**Flow slug (post-approval)**: `flow-commands-hardening`
**Created**: 2026-04-24
**Status**: Draft

## Context

The flow commands (`/plan-new`, `/review-plan`, `/implement`, `/review`, `/review-apply`, `/optimise`, `/optimise-apply`, `/plan-update`) and the `tomlctl` skill that backs them have accumulated four classes of issue:

1. **`/review-plan` is fire-and-forget.** No persistence. The user's biggest workflow pain: manually folding findings back into the plan document, lost if the session ends. Fix: persist findings and offer auto-merge with a dry-run preview.
2. **`/implement`'s parallel-dispatch instruction fails** despite being ~900 words with a pre-send checklist. `/review-apply` and `/optimise-apply` fan out reliably with ~45-word mechanical rules. Port the working formula.
3. **`/review` lens structure is mis-aligned with the user's priorities.** Security gets a full agent slot despite being an essentials-only concern. Testability, diagnostics, and developer-experience — higher-value lenses — have no explicit owner. Decision (confirmed with user): narrow Agent 2 (Security) to essentials + quick wins AND add a 5th agent for Testability, Diagnostics & DX. `/review` goes from 4 to 5 agents, matching `/optimise`'s shape.
4. **`/plan-new`'s research phases under-produce** for downstream consumers. Research Notes are freeform prose; library versions are not pinned; Phase 5 trigger is subjective; research findings do not explicitly influence Phase 6 design decisions. This forces `/implement` Phase 1 to re-research APIs the plan should already have documented.
5. **`tomlctl` skill has drift, gaps, and bloat.** The `items apply --ops` per-op `unset` form is buried in prose. `.lock` and `.sha256` sidecar semantics are split across three sections. No quick-reference table for the 10 most-common patterns. Filter-operator prose bloats 45 lines where a table would take 15. Threat-model prose (30 lines) is policy documentation, not API reference. (Note: `--where-not` and `--where-missing` ARE already documented — the gap is surfacing them in the new filter table, not documenting them.)

Plus a batch of audit fixes — stale R48 reference, vague thresholds, forward references, date-vs-timestamp ambiguity, missing fallbacks, duplicate explanations.

Cross-cutting style directive: every prose change optimises for **agent clarity**. Prefer terse imperative instructions, concrete criteria, structural cues. Cut narrative, emotional framing, and repetition-for-emphasis.

## Scope

- **In scope**:
  - Prose edits to 8 command files under `claude/commands/`
  - Schema evolution inside two shared blocks (`flow-context`, `ledger-schema`)
  - Skill-file edit at `claude/skills/tomlctl/SKILL.md`
  - A new artifact file format `plan-review-findings.toml`
  - A new persistence + auto-merge flow in `/review-plan`
  - `/review` lens restructure (narrow Agent 2; add Agent 5)
  - `/plan-new` research-phase strengthening (Phase 3, Phase 5, Phase 6 handoff)
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

Key facts discovered during Phase 1-expansion exploration:

- **`/review-plan` persistence**: zero. No `tomlctl`, `Write`, `.claude/flows/...` references. Four lens-agents (Feasibility/Dependencies, Completeness/Scope, Agent-Executability/Clarity, Risk/External Validity). Severity-tiered (Critical/Warning/Suggestion). Anchored via `[plan section/task]` bracket prefix. Resolves flow (5-step) for reads but does not write `status = "review"`. No end-of-turn next-step prompt.
- **R48 note** is in the `flow-context` shared block at line 58 of all **8** command files. The note's "3 self-healing prose copies" phrasing refers to the bootstrap prose in `plan-new`/`plan-update`/`implement`; the meta-note itself is in 8 files.
- **`/optimise-apply` freshness gate**: "strictly after `last_updated` date" — compares `%cI` ISO 8601 timestamp to a YYYY-MM-DD date. Same-day commits never trigger staleness. Ambiguity undocumented.
- **`verified-clean` scope**: review-only in shared `ledger-schema`. `optimise-apply.md:400` has a per-file note explaining `/optimise` uses `wontapply` instead. Move into the shared block to make the asymmetry discoverable.
- **Parallel-dispatch working formula** (`/review-apply:472`, `/optimise-apply:469`): single-paragraph mechanical rule, ~45 words, no checklist, no examples, neutral language, explicit "MUST / one message / all blocks / do not reduce count". `/implement`'s current 900-word section with pre-send checklist and "insidious"/"keeps happening" framing has been observed to fail anyway; the narrative length is not load-bearing.
- **tomlctl skill (695 lines)**:
  - `--where-not` and `--where-missing` ARE already documented at SKILL.md:138 and :145 (correction from earlier exploration). Surfacing them via the new quick-reference table / filter-operator table — not "documenting missing" — is the real gap.
  - `items apply --ops` per-op `unset` array is mentioned only in prose at line 455 of SKILL.md, not in the op-structure documentation.
  - Filter-operator section (lines 114-159, 45 lines) could collapse to a 15-line table.
  - Integrity-sidecar section (lines 650-680, 30 lines) is mostly threat-model prose, not API reference.
  - `blocks verify` subcommand (lines 312-330) is infrastructure-only; no command invokes it.
  - Missing: quick-reference table at top, common-recipes section, consolidated sidecar-files section, per-subcommand `--verify-integrity` support matrix.
  - **`tomlctl items find-duplicates` and `items orphans` hardcode the review/optimise ledger schema** (fingerprint over `file|summary|severity|category|symbol`; orphans reads `file`, `symbol`, `depends_on`). Running them against the new `plan-review-findings.toml` emits garbage — the new schema must warn about this, parallel to the existing warning in the execution-record-schema shared block.
- **`/plan-new` research phases**:
  - Phase 3: 2 parallel research agents, ~500 words each, cap 10 findings. Format is freeform prose with "source references" — no structured template.
  - No library enumeration step; agents are told to research "specific libraries and framework versions in use" without being handed the list from package.json/Cargo.toml.
  - No "Context7 returned nothing" fallback; no "multiple library IDs match" handling; no version-pinning mandate.
  - Phase 5 trigger ("skip if every Phase 4 answer is covered by Research Notes") is subjective — no mechanical key-term check.
  - Phase 6 does not explicitly mandate re-reading Research Notes before design decisions — findings can sit unread.
  - Research budget (2 agents, 500 words, 10 findings) is under-sized for polyglot codebases.
- **`/review` lens structure**:
  - Current: 4 agents (Quality, Security, Architecture, Completeness+Robustness).
  - Security is a full agent despite being an essentials-only concern for this user.
  - No explicit lens for testability, logging/observability, error-message quality, or CLI/API ergonomics.
  - User decision: narrow Agent 2 + add Agent 5 (Testability, Diagnostics & DX). `/review` becomes 5-agent.

## Research Notes

_No external research required — this is an internal-tooling change. All information comes from reading the nine files in scope plus `scripts/shared-blocks.toml`._

## User Decisions

- **Review lens restructure**: narrow Agent 2 (Security) to essentials + quick wins AND add a 5th agent for Testability, Diagnostics & Developer Experience. Prompted by the user's stated priority: "I do care about security in terms of essentials and quick wins but it's not a major focus." **Further refinement from user**: avoid prescribing a checklist of specific security item types — prefer natural discovery from research, avoid over-zealousness, enforce a hard cap of 5 findings. Agent 2's brief frames the lens and sets the cap; it does NOT enumerate which vulnerability classes to look for.
- **Persistence location for `/review-plan` findings**: new artifact `plan-review-findings.toml` parallel to `review-ledger.toml` / `optimise-findings.toml`. Rejected: piggybacking on `execution-record.toml`.
- **Auto-merge selector**: single `AskUserQuestion` with `multiSelect` over severity (Critical / Warning / Suggestion). Default: Critical + Warning. Lens-area filtering deferred.
- **Revised-plan preview**: write to sibling `<plan>.revised.md`; follow-up `AskUserQuestion` with Accept / Keep both / Discard options. Default: Accept.
- **R48 note**: delete outright. No relocation.
- **`verified-clean` asymmetry**: move the per-file note at `optimise-apply.md:400` into the shared `ledger-schema` block so every carrier documents it.
- **`/implement` parallel-dispatch**: replace the entire current section with the working formula. No preservation of emotional framing or numbered pre-send checklist.
- **New review category**: add `testability` to the category vocabulary in the ledger-schema shared block. Agent 5's findings emit into this bucket. Existing categories unchanged.
- **Research-phase strengthening scope**: mandate structured Research Notes format with version pinning; add Phase 3 library-enumeration sub-step (scoped — only dependency manifests that intersect the plan's `scope` globs, to avoid dumping 200+ deps into a polyglot-monorepo agent prompt); mechanical Phase 5 trigger; explicit Phase 6 re-read step; broaden research focus types.
- **`suggested_edit` merge contract**: mechanical `anchor_old` + `anchor_new` string pair, NOT natural-language prose. This makes auto-merge deterministic and testable rather than LLM-reasoning-dependent. Advisory-only findings (no anchor pair) surface in the report but are not merged automatically.
- **Q2 empty-answer default**: Keep both (NOT Accept). Auto-mode / skill-hosted / headless callers hit empty responses per Claude Code issues #29618/#29547; defaulting to Accept would cause silent plan overwrite.
- **`.revised.md` sibling preservation**: after Accept, keep the sidecar for one cycle (deleted on next run) so Accept is reversible.
- **tomlctl skill scope**: document missing operators; add quick-reference table; add common-recipes section; consolidate sidecar semantics; trim filter-operator prose into a table; trim threat-model prose.

## Approach

Three concurrent tracks after a sequential schema-evolution phase.

### Phase A: Schema evolution (sequential, one agent, multiple files)

**`flow-context` shared block** (8 carrier files):
- Delete the parenthetical R48 follow-up note from the `[artifacts]` paragraph.
- Add `plan_review_findings` as a new canonical `[artifacts]` key:
  - Extend the schema example block to include the new line.
  - Extend the field-responsibilities list "currently: ..." enumeration.
  - Add a self-healing note: commands compute `plan_review_findings = .claude/flows/<slug>/plan-review-findings.toml` from `slug` when absent and write back on next TOML write. Unlike `execution_record`, no atomic bootstrap is needed — `/review-plan` is the sole writer and creates it on first persistence.

**`ledger-schema` shared block** (4 carrier files: `review`, `review-apply`, `optimise`, `optimise-apply`):
- Add `testability` to the review category vocabulary. Updated list: `quality | security | architecture | completeness | db | testability | verified-clean`.
- Adjacent to `verified-clean (review only)` disposition line, add one sentence: "`/optimise` has no `verified-clean` counterpart — bytes-written findings land in `applied`, already-correct cases land in `wontapply` with rationale."

### Phase B: Per-file non-shared-block edits (parallel, disjoint files)

Each agent owns one file and applies all non-shared-block edits for that file in a single pass.

**`review.md`** (substantial edit — audit fixes + lens restructure):

*Audit fixes*:
- TaskCreate/TaskUpdate ephemerality sentence at line 440-area: "These TaskCreate/TaskUpdate entries are ephemeral to this `/review` run and do NOT write `context.toml.[tasks]`."
- Move or parenthesise the small-diff shortcut forward reference (currently referenced at line 327, defined at line 424).
- Tighten Agent 3 "Do NOT flag" thresholds with concrete criteria.
- Add asymmetry-note mirror-reference.

*Lens restructure*:
- Rewrite Agent 2 (Security) brief to narrow focus **without prescribing an enumerated list of item types** — the user prefers natural discovery from research over a checklist. New brief (approximate prose):

  > "Focus on security **essentials and quick wins** appropriate to what the changed code does. Apply contextual judgement: not every feature needs every possible protection, and the goal is to flag the top issues that would actually matter if this shipped — not to enumerate every possible concern. Research the specific trust boundaries, data flows, and attack surfaces the changed code introduces via Context7 and WebSearch where current guidance matters. Bring forward only the highest-signal findings — real risks with plausible attack vectors in the actual deployment. Do NOT produce exhaustive threat-model output, theoretical vulnerabilities, or defence-in-depth laundry lists. **Hard cap: 5 findings.** Zero findings is a valid outcome — if nothing material surfaces, return an empty security pass rather than padding."

  The brief preserves the lens's existing "apply judgement rather than a fixed checklist" framing (current `review.md:476`) but strengthens it: hard cap, explicit "zero is valid", explicit anti-zealousness, natural-discovery preference over enumerated item types.
- Add new Agent 5: Testability, Diagnostics & Developer Experience. Brief covers:
  - **Testability** — public surface is testable (no hidden globals, seams for mocking, pure vs impure functions at appropriate boundaries); new tests accompany new code where the project convention requires it; tests are deterministic and name what they cover; regression risk for changed code is covered.
  - **Diagnostics & observability** — error messages name the operation that failed, the input that caused it, and the expected vs actual state; logging is at the right level (debug/info/warn/error) and includes enough context to triage from logs alone; metrics / traces are instrumented where the project pattern expects them.
  - **Developer experience** — public APIs, CLIs, and configuration surfaces are discoverable (help text, self-describing errors, sensible defaults); naming and error messages read well to a first-time user; documentation at the call site is present where the project convention requires it.
  - Category: `testability` (new ledger-schema value added in Phase A).
  - Cap: 15 findings per agent, ceiling 20, same as other lenses.
- Update "Launch **all four** review agents" → "Launch **all five** review agents" at the dispatch rule.
- Update Task-tracking gate: "call TaskCreate once per lens: Quality, Security, Architecture, Completeness, Testability — 5 tasks for a normal run, OR 1 task for the small-diff shortcut."
- Update the small-diff shortcut prose (currently "single comprehensive review agent instead of four specialized ones") to "instead of five specialized ones" and include the Testability lens in the combined agent's brief.
- Update the Design Note: Intentional Asymmetry (currently says `/review`'s 4 lenses are language-agnostic) to reflect 5 lenses.

**`optimise.md`** (audit fixes only):
- TaskCreate/TaskUpdate ephemerality sentence.
- Collapse `## Optimization Focus` duplication.
- `tomlctl items orphans` fallback sentence.
- Asymmetry-note mirror-reference.

**`implement.md`** (audit fixes + parallel-dispatch rewrite):
- Delete lines 360-393 entirely (anti-pattern section, commit-to-completion gate, pre-send checklist). Replace with working-formula paragraph.

  > **Parallel dispatch rule.** Emit one `Agent` tool-use block per task in this batch, all within the same response. Do not launch them across turns. Do NOT reduce the agent count — launch the full complement for each batch. The harness fans out concurrently only when blocks arrive in one response; a second `Agent` call on a later turn runs *after* the first returns. (Pattern: same as `/review-apply` and `/optimise-apply`, which fan out reliably with this rule.)

- Reposition the Prompt-cache tip immediately above the rule.
- Keep the existing "Batch execution" numbered list (lines 422-430); it's agent-prompt content.
- Add `<record>` shorthand recap near Phase 2's first use: "(`<record>` = the fully-qualified `.claude/flows/<slug>/execution-record.toml` path resolved in Phase 1.)"
- Phase 4 Implementation Summary: add a `### Next Steps` section: "Run `/review` to audit the implementation, or `/optimise` to research performance opportunities."

**`optimise-apply.md`** (freshness gate + cleanup):
- Replace "strictly after the ledger's `last_updated` date" with: "If any file's newest commit timestamp is at or after 00:00:00Z on the day AFTER `last_updated` (i.e. `last_updated + 1 day`), the ledger is stale with respect to this selector."
- Remove the per-file `verified-clean` asymmetry note at line 400 — now handled in the shared `ledger-schema` block.

**`plan-new.md`** (scope-assessment + research-phase strengthening):

*Scope assessment tightening (Phase 1 step 3)*:
- Replace the vague "does the request bundle multiple independent concerns?" with explicit criteria: "Propose splitting when ANY of these hold: (a) the request combines features that could ship independently (no code dependency, independent success measures, reviewable separately); (b) the request touches ≥4 unrelated modules with no shared refactoring; (c) the request combines a refactor and a new feature."

*Phase 3 research strengthening*:
- Add a new sub-step **before agent dispatch** (after exploration findings are consolidated, before research agents are launched): **Library enumeration**. The orchestrator reads dependency-manifest file(s) that intersect the plan's `scope` globs — package.json, Cargo.toml, pyproject.toml, go.mod, *.csproj. For monorepos, only enumerate workspace packages whose directories intersect the scope globs; do NOT recurse the whole repo. Extract each dependency + version. Cross-reference against the plan's `## Approach`. The resulting "libraries to research" list (scope-filtered, typically ≤ 20 libraries) is handed to each research agent in its prompt.
- Update research-agent prompt template to mandate the structured Research Notes record format:

  ```
  - **Library/API**: [name] [version from manifest file]
  - **Source**: [Context7 query reference or URL]
  - **Finding**: [one-line — API signature, deprecation, behaviour]
  - **Details**: [2-3 sentence explanation with exact parameter names / method signatures]
  - **Impact on plan**: [how this finding shapes the design, or "no change"]
  ```

- Add handling for "Context7 returns nothing": fall back to WebSearch, record the absence in the finding, flag in Phase 6.
- Add handling for "Context7 returns multiple library IDs": agent states the disambiguation explicitly and picks based on the project's actual dependency; if ambiguous, surfaces as a Phase 4 directed question.
- Require version-pinning: every finding that references a library MUST cite the exact version from the dependency manifest. Generic guidance without version pin is rejected.
- Broaden research focus types. Add to the "Research focus should be tailored" list:
  - **Changelog and breaking-change research** — for each library dependency, check if the current version introduced breaking changes from a previously-stable baseline.
  - **Benchmarking research** — when the plan proposes multiple viable approaches, research performance trade-offs and cite sources.
  - **Undocumented-behaviour research** — WebSearch StackOverflow / GitHub Issues for edge cases and surprising behaviour in the frameworks in use.

*Phase 5 trigger (mechanical)*:
- Replace the subjective "skip if every answer is covered by Research Notes" with a mechanical check:
  1. For each Phase 4 answer A, extract key terms (library names, API names, pattern names).
  2. Grep Research Notes for each key term.
  3. If all key terms appear, mark A as "covered".
  4. If all answers are "covered", skip Phase 5 and note the skip.
  5. Override: if the grep matched the library name but not a specific API referenced in the answer, run Phase 5 anyway.

*Phase 6 research-to-design handoff*:
- Insert a new first step in Phase 6: **Review research findings**.
  1. Re-read the full `## Research Notes` section.
  2. For each finding with non-empty "Impact on plan", note the constraint.
  3. List any deprecations or version-specific behaviours that force a design choice.
- Downstream steps (Evaluate approaches, Choose approach, etc.) reference the constraints list when making decisions.

### Phase C: tomlctl skill polish (parallel with Phase B)

**`claude/skills/tomlctl/SKILL.md`**:

*Additions*:
- Add a **Quick reference table** at the top (after front-matter, before "Install & capabilities") with 8-10 most-common patterns:
  - Read whole file → `tomlctl get <file>`
  - Get scalar → `tomlctl get <file> <key-path> --raw`
  - Filter items → `tomlctl items list <file> --where KEY=VAL`
  - Add item → `tomlctl items add <file> --json '{...}'`
  - Batch mutate → `tomlctl items apply <file> --ops - <<'EOF' [...] EOF`
  - Next monotonic ID → `tomlctl items next-id <file> --prefix R`
  - Count open items → `tomlctl items list <file> --status open --count --raw`
  - Refresh integrity sidecar → `tomlctl integrity refresh <file>`
- Add a **Common recipes** section with ~4-6 patterns that commands repeatedly use:
  - Append to execution-record (the two-call heredoc pattern)
  - Dedup-by-field on batch add (the `--dedupe-by` dance)
  - Fetch next ID (both `--prefix` and `--infer-from-file` paths)
  - Filter to open items and count (`--status open --count --raw`)
  - Bulk transition with mixed ops (`items apply --ops -` JSON array)
- Add a **Sidecar files** consolidated section covering `.sha256` and `.lock` semantics in one place. Replace the fragmented coverage at lines 650-680 and line 690.
- Add a **`--verify-integrity` support matrix** listing every read subcommand that accepts the flag. Clarify it is a per-subcommand option, not a global flag.
- Document `--where-not KEY=VAL` and `--where-missing KEY` in the filter-operators section.
- Document the `items apply --ops` per-op `unset` array form prominently (not in buried prose).

*Simplifications*:
- Replace the 45-line filter-operators prose (lines 114-159) with a 15-line table: one row per operator (name, semantics, example).
- Trim the 30-line integrity-sidecar threat-model prose (lines 650-680) to ~10 lines (file location, when read/written, error behaviour, flags). Move the "not a MAC / not tamper-proof" policy note to a one-line footnote.
- Move the `blocks verify` subcommand documentation (lines 312-330) to an "Advanced / maintenance subcommands" appendix — no command invokes it, it's infrastructure-only.

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

Required fields: `id` (P{n} monotonic), `review_round`, `severity` ∈ {critical, warning, suggestion}, `category` ∈ {feasibility, completeness, executability, risk}, `plan_section` (markdown heading anchor as a literal string, copied verbatim from the plan), `summary`, `status` ∈ {open, merged, discarded}.

Optional: `description` (longer), `anchor_old` (exact substring that already exists in the plan file under `plan_section`), `anchor_new` (replacement substring). `anchor_old` + `anchor_new` together form the mechanical merge contract — **both required for auto-merge to act on a finding. Findings with only `summary` / `description` and no anchor pair are "advisory-only" and will be skipped by the merger.**

**Schema callouts (MUST appear verbatim in the schema documentation inside `review-plan.md`)**:
- `tomlctl items find-duplicates` and `tomlctl items orphans` hardcode the review/optimise ledger schema and MUST NOT be invoked against `plan-review-findings.toml` — they will emit garbage. Parallel to the existing warning in the `execution-record-schema` shared block.
- `items next-id --prefix P` is the supported ID-assignment path; `items list` / `items add-many` / `items apply --ops -` with the schema above are supported.

*New Step 3.5: Persist findings*:
1. Compute `plan_review_findings_path = context.toml.artifacts.plan_review_findings` (or derive from slug if absent; write back per the self-healing contract).
2. Mint monotonic P-IDs: `tomlctl items next-id <path> --prefix P` (starts at P1 on first run).
3. Batch-write: `tomlctl items add-many <path> --defaults-json '{"review_round":<n>, "status":"open"}' --ndjson -` with all findings. Agents emit findings WITH `anchor_old` / `anchor_new` pairs when the finding is mechanically mergeable; without them when it is advisory.
4. `tomlctl set <path> last_updated <today>` and `tomlctl set <path> round <n>`.

*New Step 4: Auto-merge offer (on turn-end)*:

1. Count findings by severity. If zero, output "No findings — plan is clean." and end.
2. **`AskUserQuestion` (Q1)**: multiSelect severity options `[Critical, Warning, Suggestion]`, default Critical + Warning.
   - **Empty-answer rule**: if the user's response is empty (the tool auto-completes empty in `acceptEdits` mode, inside skill-hosted invocations, or in headless mode per Claude Code issues #29618 / #29547), treat as "zero selected" — skip merge entirely. Persist findings only. Do NOT proceed to Q2.
3. If zero selected → persist only, no merge. Output a one-line console note: "Findings persisted; auto-merge skipped. Re-run interactively to merge."
4. Filter to selected severities **with both `anchor_old` AND `anchor_new` present** (advisory-only findings surface in the report but are not merged).
5. **Conflict detection**: group selected findings by `plan_section`. If >1 finding in a group has non-empty `anchor_old`, flag as conflict. Do NOT apply any conflicting finding — instead append per-conflict console lines: `[conflict: plan_section="..."; findings=P3, P7] — manual merge required`. Non-conflicting findings still apply.
6. **Mechanical merge**: for each surviving finding, locate `anchor_old` as a string substring in the plan file under the `plan_section` heading. If found exactly once, replace with `anchor_new`. If not found or found multiple times, log per-finding `[merge-failed: P{n} — anchor_old not found uniquely in section "..."]` and skip that finding. Apply in P-id monotonic order.
7. Materialise revised content via `Write` to a sibling file using this **exact naming rule**: replace the plan file's trailing `.md` with `.revised.md` (do not append — e.g. `docs/plans/flow-commands-hardening.md` → `docs/plans/flow-commands-hardening.revised.md`). For multi-file plans (plan_path points at `docs/plans/<feature>/00-outline.md`), materialise only the outline at `<outline-dir>/00-outline.revised.md`; **detail files are not rewritten by auto-merge v1** — findings whose `plan_section` targets a detail file are treated as advisory-only and surface in the console but do not merge.
8. **Pre-existing sibling**: if `<plan>.revised.md` already exists from a prior run, preserve it as `<plan>.revised.prev.md` (rename before writing the new revision) rather than silently overwriting. Cheap rollback.
9. Console summary: N findings applied, K conflicts skipped, M merge-failures, list of `plan_section` → summary for applied findings.
10. **`AskUserQuestion` (Q2)**: Accept (overwrite original) / Keep both (leave revised file for manual review) / Discard revised.
    - **Default**: Keep both (NOT Accept). Rationale: Accept is irreversible (overwrites the plan), and the user's usual mode is `acceptEdits` which auto-completes empty; defaulting to Accept would cause silent data loss.
    - **Empty-answer rule**: empty response → treat as Keep both. Do NOT auto-Accept on empty.
11. Apply chosen action:
    - **Accept**: `Write` revised content over original plan; **do NOT delete `<plan>.revised.md`** — keep it for one cycle so the user can inspect the merged changes post-hoc. Transition matching findings to `status = "merged"` via `tomlctl items apply --ops -`.
    - **Keep both**: no mutation; findings stay `status = "open"`. User reviews `<plan>.revised.md` manually.
    - **Discard**: delete `<plan>.revised.md`; transition matching findings to `status = "discarded"`.
    - The `.revised.md` sidecar from the prior run (preserved as `.revised.prev.md` in step 8) is deleted on the NEXT `/review-plan` run's step 8 — i.e. always keep one generation of prior revision around for rollback.
12. Final `tomlctl set <path> last_updated <today>`.

*Handoff*: no contract change to `/implement`. Findings file persists on disk for audit. Subsequent `/review-plan` runs increment `review_round` rather than overwriting. **Dedup on re-run**: findings already transitioned to `merged` or `discarded` in a prior round are ignored by lens-agents when surfacing new findings — agents receive the ledger's `open`-status items as prior context and check dedup via `(plan_section, anchor_old)` rather than re-emitting them.

*Agent-count clarifying note*: `/review-plan` keeps its existing 4 lens-agents (Feasibility, Completeness, Executability, Risk). These are plan-review lenses, distinct from `/review`'s 5 code-review lenses (post-Task 2). Add one line near the top of `review-plan.md` stating this explicitly so readers don't conflate the two.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
sharedblocks: bash scripts/verify-shared-blocks.sh
```

No cargo changes expected (Markdown-only edits), but the commands are listed so `/implement`'s verification agent does not re-discover them.

## Tasks

### 1. Shared-block evolution (flow-context + ledger-schema) + non-shared category-list updates [M]
- **Files**: all 8 files under `claude/commands/`. Byte-identical content across each shared block's carriers; plus a non-shared-block edit in `review-apply.md:454` (outside any shared block — confirmed by review agents).
- **Depends on**: —
- **Action**:
  - `flow-context` block (8 files): delete R48 parenthetical; add `plan_review_findings` to schema example and field responsibilities.
  - `ledger-schema` block (4 files — review, review-apply, optimise, optimise-apply): add `testability` to review category vocabulary; add verified-clean asymmetry sentence.
  - **Non-shared-block update**: `review-apply.md:454` contains a mixed-category cluster enumeration `quality + security + architecture + completeness + db`. Extend to `quality + security + architecture + completeness + db + testability`.
- **Detail**: single agent edits all files in one pass. `Edit` with explicit `old_string` / `new_string` pairs captured from `review.md` (any carrier) before the pass, so the strings are anchored to real file content — avoid line-number anchors. Shared-block edits use `replace_all: true` with the old block quoted in full from one carrier (guaranteed byte-identical across the 8 files). Run `bash scripts/verify-shared-blocks.sh` at end.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0; `grep -rc "R48" claude/commands/` returns 0; `grep -rl "plan_review_findings" claude/commands/ | wc -l` ≥ 8; `grep -rl "testability" claude/commands/ | wc -l` ≥ 4; `review-apply.md:454` cluster enumeration includes `testability`.

### 2. review.md (audit fixes + lens restructure to 5 agents) [L]
- **Files**: `claude/commands/review.md`
- **Depends on**: 1
- **Action**:
  - Apply 4 audit fixes (TaskCreate ephemerality, small-diff forward ref, Agent 3 thresholds, asymmetry anchor).
  - Rewrite Agent 2 (Security) to essentials-only scope with cap 5.
  - Add new Agent 5: Testability, Diagnostics & Developer Experience. Category: `testability`. Cap 15/20.
  - Update "four" → "five" references in: **dispatch rule** (currently `Launch **all four** review agents`), **task-tracking** (`4 tasks for a normal run`), **small-diff shortcut prose** (`instead of four specialized ones`), **design note** (`/review's four lenses (Quality, Security, Architecture, Completeness) are language-agnostic` — rewrite to five and name the new lens).
  - **Anchor each "four" replacement to its surrounding context phrase** — do NOT use global `replace_all` on the bare word `four`. Preserve `review.md:40` (`four string values` in the status enum) verbatim. Verified by executability agent that the status-enum line exists at line 40 and is inside the shared `flow-context` block (itself preserved by Task 1).
- **Acceptance**: `grep -c "Launch \*\*all five\*\*" claude/commands/review.md` ≥ 1; `grep -c "testability" claude/commands/review.md` ≥ 2; `grep -c "essentials and quick wins" claude/commands/review.md` ≥ 1; `grep -c "four string values" claude/commands/review.md` ≥ 1 (status-enum line preserved); shared-blocks parity check passes.

### 3. optimise.md audit fixes [S]
- **Files**: `claude/commands/optimise.md`
- **Depends on**: 1
- **Action**:
  - Apply the four optimise.md audit fixes (TaskCreate ephemerality, Optimization Focus dedup, tomlctl orphans fallback, asymmetry anchor).
  - **Re-anchor the stale `review.md:315` reference** in the Design Note (currently `optimise.md:420`): the small-diff shortcut is actually at `review.md:424`, and Task 2's additions will shift this further. Replace the line-number anchor with a heading anchor (e.g. `/review`'s "Small-diff shortcut" section) so future edits don't rot it.
- **Acceptance**: shared-blocks parity passes; duplicate "Optimization Focus" re-explanation removed; `grep "review.md:315" claude/commands/optimise.md` returns 0 (stale anchor gone).

### 4. implement.md (parallel-dispatch rewrite + small additions) [M]
- **Files**: `claude/commands/implement.md`
- **Depends on**: 1
- **Action**:
  - Delete lines 360-393; replace with working-formula paragraph.
  - Reposition Prompt-cache tip above the rule; keep Batch execution numbered list.
  - Add `<record>` shorthand recap near Phase 2 first use.
  - Add "Next Steps" bullet in Phase 4 Implementation Summary.
- **Acceptance**: implement.md line count drops by ≥30 lines; `grep -i "insidious\|keeps happening\|natural stopping" claude/commands/implement.md` returns 0; shared-blocks parity passes.

### 5. optimise-apply.md freshness + cleanup [S]
- **Files**: `claude/commands/optimise-apply.md`
- **Depends on**: 1
- **Action**:
  - Replace "strictly after" with `last_updated + 1 day` rule, explicitly framed in UTC: "If any file's newest commit timestamp is at or after 00:00:00Z on the day AFTER `last_updated`, the ledger is stale."
  - Add a one-line note after the rule: "The comparison is UTC-based; users in non-UTC timezones may observe staleness firing at different wall-clock times than the calendar rule suggests."
  - Delete the now-redundant per-file `verified-clean` asymmetry note at line 400.
- **Acceptance**: `grep "strictly after" claude/commands/optimise-apply.md` returns 0; `grep -c "00:00:00Z\|UTC" claude/commands/optimise-apply.md` ≥ 1; shared-blocks parity passes.

### 6. plan-new.md (scope-assessment + research-phase strengthening) [L]
- **Files**: `claude/commands/plan-new.md`
- **Depends on**: 1
- **Action**:
  - Phase 1 step 3: replace vague split-decision prose with three concrete criteria.
  - Phase 3: add Library-enumeration sub-step; mandate structured Research Notes record format with version pinning; add Context7 no-match and multi-match handling; broaden research focus types (changelog, benchmarking, undocumented-behaviour).
  - Phase 5: replace subjective trigger with mechanical key-term check.
  - Phase 6: insert "Review research findings" as the first step.
- **Acceptance**: `grep -c "Library/API\|Library enumeration" claude/commands/plan-new.md` ≥ 2; mechanical Phase 5 check is documented; Phase 6 opens with re-read step; shared-blocks parity passes.

### 7. review-plan.md persistence + auto-merge feature [L]
- **Files**: `claude/commands/review-plan.md`
- **Depends on**: 1
- **Action**: implement Phase D as specified in the Approach section above. Includes the schema with `anchor_old` / `anchor_new` mechanical-merge contract, both schema callouts (do-not-run-find-duplicates, do-not-run-orphans), Step 3.5 persistence, Step 4 auto-merge with empty-answer rules (Q1 empty → skip merge; Q2 default → Keep both), conflict detection, `.revised.md` naming rule, multi-file plan handling (outline-only merges in v1), `.revised.prev.md` rollback sidecar, re-run dedup via `(plan_section, anchor_old)`, and the 4-vs-5-agents clarifying note near the top of the file.
- **Acceptance**:
  - `grep -c "plan_review_findings" claude/commands/review-plan.md` ≥ 3
  - `grep -c "AskUserQuestion" claude/commands/review-plan.md` ≥ 2
  - `grep -c "anchor_old\|anchor_new" claude/commands/review-plan.md` ≥ 4 (schema + merge step both reference them)
  - `grep -c "find-duplicates\|items orphans" claude/commands/review-plan.md` ≥ 1 (the anti-garbage callout is present)
  - `grep -c "Keep both" claude/commands/review-plan.md` ≥ 1 (default-on-empty documented)
  - `grep -c "revised.prev.md\|revised.md" claude/commands/review-plan.md` ≥ 2 (naming rule present)
  - File includes an inline schema example block for `plan-review-findings.toml`
  - Shared-blocks parity passes

### 8. tomlctl SKILL.md polish [M]
- **Files**: `claude/skills/tomlctl/SKILL.md`
- **Depends on**: — (independent of Phase A schema evolution; Phase A does not touch the skill)
- **Action** (execute additions BEFORE trimming so no subcommand documentation is lost in the window):
  1. Add Quick reference table at top (8-10 rows).
  2. Add Common recipes section (~4-6 patterns).
  3. Add `--verify-integrity` support matrix — this REPLACES the narrative list currently at SKILL.md:668. Do this step BEFORE the trim in step 7 so the subcommand enumeration survives.
  4. Consolidate Sidecar files section (.sha256 + .lock) from the existing lines 650-680 and 690.
  5. Surface `--where-not` (already at SKILL.md:138) and `--where-missing` (already at SKILL.md:145) in the new filter-operator table — they are documented; the gap is discoverability.
  6. Document `items apply --ops` per-op `unset` array prominently in the op-structure section (currently only at SKILL.md:455 in prose).
  7. Replace 45-line filter-operator prose (lines 114-159) with a ~15-line table.
  8. Trim 30-line integrity-sidecar threat-model prose (lines 650-680) to ~10 lines. Preserve the subcommand list at line 668 (already migrated to the matrix in step 3).
  9. Move `blocks verify` (lines 312-330) to an "Advanced / maintenance subcommands" appendix.
- **Acceptance**: 
  - SKILL.md net line count drops by ≥20 (tighter than earlier "≥30" — additions offset some trim)
  - `grep -c "^|" claude/skills/tomlctl/SKILL.md` increases by ≥15 (table rows)
  - Quick Reference heading present near top (within first 50 lines)
  - `grep -c "find-duplicates\|orphans" claude/skills/tomlctl/SKILL.md` stays ≥ the current count (subcommand documentation density preserved)
  - `--verify-integrity` matrix table present

## Dependency Graph

```
Task 1 (shared-block evolution) ──┐
                                  │
Task 8 (SKILL.md polish) ─────────┤ (parallel with Task 1 — different files)
                                  │
                                  ▼
  ┌──────────────┬──────────────┬──────────────┬──────────────┬──────────────┐
  │              │              │              │              │              │
Task 2         Task 3         Task 4         Task 5         Task 6         Task 7
(review.md)    (optimise.md)  (implement.md) (opt-apply.md) (plan-new.md)  (review-plan.md)
  │              │              │              │              │              │
  └──── all parallel after Task 1 completes ─────────────────────────────────┘
```

- **Batch 1** (parallel): Task 1 + Task 8. Disjoint files.
- **Batch 2** (parallel, after Batch 1): Tasks 2-7. Six parallel agents exceed `/implement`'s 3-4 cap. Correct effort classification is **L = {2, 6, 7}**, **M = {4}**, **S = {3, 5}**. Sub-batch as **{2, 6, 7}** (three L) then **{3, 4, 5}** (M + two S) so the three largest run together first. Files are disjoint across each sub-batch (verified by executability-review agent).

## Verification

After all tasks land:

1. `bash scripts/verify-shared-blocks.sh` → exit 0.
2. `cargo build --manifest-path tomlctl/Cargo.toml` → succeed.
3. `cargo test --manifest-path tomlctl/Cargo.toml` → pass.
4. Grep assertions (use `-rl` for directory recursion; bare `grep -l <dir>` errors on most grep builds):
   - `grep -rc "R48" claude/commands/` → 0
   - `grep -rl "plan_review_findings" claude/commands/ | wc -l` → ≥ 8
   - `grep -rl "testability" claude/commands/ | wc -l` → ≥ 4
   - `grep -i "insidious\|keeps happening\|natural stopping" claude/commands/implement.md` → 0
   - `grep "strictly after" claude/commands/optimise-apply.md` → 0
   - `grep -c "Launch \*\*all five\*\*" claude/commands/review.md` → ≥ 1
   - `grep -c "four string values" claude/commands/review.md` → ≥ 1 (status-enum line preserved in the shared flow-context block)
   - `grep -c "Library enumeration" claude/commands/plan-new.md` → ≥ 1
   - `grep -c "anchor_old\|anchor_new" claude/commands/review-plan.md` → ≥ 4
   - `grep "review.md:315" claude/commands/optimise.md` → 0 (stale anchor re-pointed)
5. Manual smoke tests:
   - Invoke `/review-plan` against a known plan file; confirm findings persist to `.claude/flows/<slug>/plan-review-findings.toml` with valid TOML; confirm AskUserQuestion flow fires; confirm revised-plan file materialises; confirm Accept/Keep/Discard each transition correctly.
   - Invoke `/review` on a small scope; confirm 5 agents dispatch in parallel; confirm the 5th agent emits findings with `category = "testability"`.
   - Invoke `/plan-new` on a mock task; confirm Phase 3 reads the dependency manifest and Research Notes follow the structured format.

## Risks

- **Shared-block parity breakage during Task 1.** Three concurrent changes (R48 removal, `plan_review_findings` addition, `testability` + verified-clean clarification) must land byte-identically across their respective carrier sets. Mitigation: single agent owns Task 1; final step runs `bash scripts/verify-shared-blocks.sh` and refuses commit on failure.
- **Shared-block parity check is byte-only, not semantic.** `verify-shared-blocks.sh` hashes block content across carriers; it does not validate that the `[artifacts]` key spelling matches what downstream code reads. Mitigation: Task 7 acceptance cross-checks the key spelling in `review-plan.md` (reader) against `plan-new.md` (writer) — both must reference `plan_review_findings` identically. Also note: the hook is pre-commit only (per `.githooks/pre-commit`); no CI fallback exists, so a bypass with `--no-verify` would land unchecked. Forbidden in this plan.
- **`AskUserQuestion` silently auto-completes in `acceptEdits` / skill-hosted / headless modes** (Claude Code issues #29618, #29547). User's typical mode is `acceptEdits`. Mitigation: explicit empty-answer rules in Task 7 (Q1 empty → skip merge; Q2 empty → Keep both, NEVER auto-Accept). Default for Q2 is Keep both, not Accept — this is the load-bearing rule that prevents silent plan overwrite in auto mode.
- **`tomlctl items find-duplicates` and `items orphans` emit garbage against `plan-review-findings.toml`.** Both subcommands hardcode the review/optimise ledger schema fields (`file`, `summary`, `severity`, `category`, `symbol` / `depends_on`). Mitigation: Task 7 schema documentation includes explicit do-not-invoke callouts, parallel to the existing warning in the `execution-record-schema` shared block. Followup (out of scope here): tomlctl could gain a `--schema review|optimise|plan-review|execution` flag to gate the hardcoded logic — tracked separately.
- **Auto-merge conflicts and merge-failures.** Two findings with the same `plan_section` could both try to edit the same region; `anchor_old` could fail to match. Mitigation: Task 7 mechanical merge step detects both cases and reports per-finding `[conflict: ...]` or `[merge-failed: ...]` instead of producing ambiguous output.
- **`/review` 5-agent parallel dispatch under heavy scope.** Adding a 5th agent increases coordination overhead. `/review` already has the working formula (~45 words); scaling from 4 to 5 should be safe per the evidence from `/optimise` (5 agents, reliable). Mitigation: keep the existing dispatch rule; monitor for regression in observed reliability.
- **`testability` category overlap with existing categories.** A finding about "missing test for X" could be `completeness` (current Agent 4) or `testability` (new Agent 5). Mitigation: Agent 5's brief explicitly owns tests; Agent 4's "tests not written" becomes a routing pointer to Agent 5. Dedup rule (same file + symbol/summary) handles cross-agent duplication.
- **Research-phase strengthening may increase `/plan-new` runtime.** Library enumeration adds a step; structured Research Notes format adds words per finding; Phase 6 re-read adds reasoning. Net effect: deeper plans, faster downstream execution (less re-research). Mitigation: accept the trade-off; the command is already `xhigh/max` effort.
- **Auto-merge produces a malformed plan file.** `suggested_edit` application must handle missing `plan_section` anchors. Mitigation: `<plan>.revised.md` always written as sibling before any overwrite. On malformed merge, agent skips Q2 and reports failure instead of prompting Accept.
- **tomlctl skill changes drift from actual binary behaviour.** If the skill documents `--where-not` but the binary doesn't support it, agents fail. Mitigation: verify each operator listed in the updated skill against `tomlctl --help` or a test invocation before committing Task 8.
- **Task 2-7 batch exceeds `/implement` 3-4 parallel cap.** Six parallel tasks. Mitigation: explicit sub-batching noted in Dependency Graph: {2, 4, 7} + {3, 5, 6}.

## Post-approval execution

After `ExitPlanMode` approval, `/plan-new`'s Phase 7 bootstrap runs:

1. Write `docs/plans/flow-commands-hardening.md` (copy content from this sandbox plus Phase 7 metadata stamp).
2. Create `.claude/flows/flow-commands-hardening/` directory.
3. Write `.claude/flows/flow-commands-hardening/context.toml`:
   - `slug = "flow-commands-hardening"`
   - `plan_path = "docs/plans/flow-commands-hardening.md"`
   - `status = "draft"`
   - `created = 2026-04-24`, `updated = 2026-04-24`
   - `branch = "<current git branch>"` (omit if empty)
   - `scope = ["claude/commands/**", "claude/skills/tomlctl/**"]`
   - `[tasks]` with `total = 8`, `completed = 0`, `in_progress = 0`
   - `[artifacts]` including `review_ledger`, `optimise_findings`, `execution_record`, `plan_review_findings`
4. Bootstrap `.claude/flows/flow-commands-hardening/execution-record.toml` with atomic 2-line `Write` (`schema_version = 1\nlast_updated = 2026-04-24\n`).
5. Run `tomlctl integrity refresh .claude/flows/flow-commands-hardening/execution-record.toml`.
6. Write `.claude/active-flow` containing the slug.

The user can then invoke `/implement` (or `/review-plan` first) to proceed.
