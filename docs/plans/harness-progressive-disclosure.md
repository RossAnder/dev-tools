# Plan: Progressive-Disclosure Overhaul for Harness Files

**Plan path**: `docs/plans/harness-progressive-disclosure.md`
**Flow slug**: `harness-progressive-disclosure`
**Created**: 2026-05-19
**Status**: Draft (in planning)

## Context

The slash-command carriers under `claude/commands/*.md` have grown to 6,768 lines across 10 files. Every invocation loads its carrier's full text up-front, including shared blocks that frequently dominate the file: `execution-record-schema` alone is 186 lines and rides in 4 carriers; `flow-context` rides in 9; `vet-flow-research` in 6; `ledger-schema` in 4. Total duplicated bytes across the manifest are large enough that parity-enforcement (a bash script + a pre-commit hook + a dedicated `tomlctl blocks` subcommand) exists specifically to prevent drift. The harness has scaled by duplication.

The user wants progressive disclosure: deliver instructions just-in-time so a carrier file shrinks to a thin orchestrator and the verbose contracts (schemas, vet procedures, apply protocols) load only when a phase actually needs them. A secondary directive: where appropriate, surface the delivery mechanism through `tomlctl` (and the existing `tomlctl` skill) rather than only through sub-agents — tomlctl is already the trusted I/O substrate for flow TOML and can act as a content-emit service for static contract text.

## Exploration Notes

### Carrier inventory (Phase 2 — Explore Agent 1)

| Carrier | LOC | Shared blocks carried |
|---|---|---|
| plan-new.md | 672 | flow-context, execution-record-schema, plansdirectory-prompt, vet-flow-research |
| plan-update.md | 803 | flow-context, execution-record-schema, plansdirectory-prompt, vet-flow-research |
| implement.md | 608 | flow-context, execution-record-schema |
| review-plan.md | 355 | flow-context, plansdirectory-prompt, vet-flow-research |
| review.md | 777 | flow-context, ledger-schema, ledger-disposition-sweep, vet-flow-research |
| review-apply.md | 782 | flow-context, ledger-schema, apply-dependency-sort, apply-vet-flow-implement-lite, apply-rollback-protocol, apply-constraints |
| optimise.md | 689 | flow-context, ledger-schema, ledger-disposition-sweep, vet-flow-research |
| optimise-apply.md | 745 | flow-context, ledger-schema, apply-dependency-sort, apply-vet-flow-implement-lite, apply-rollback-protocol, apply-constraints |
| tdd.md | 478 | flow-context, execution-record-schema |
| test-bootstrap.md | 459 | vet-flow-research |

**Block reuse counts**: `flow-context` (9), `vet-flow-research` (6), `execution-record-schema` (4), `ledger-schema` (4), `plansdirectory-prompt` (3), four `apply-*` blocks (2 each), `ledger-disposition-sweep` (2). Block sizes: `execution-record-schema` ≈186 LOC; `flow-context` 31 LOC; `vet-flow-research` 21 LOC; `ledger-schema` ≈60+ LOC.

Phase structure is uniform: every carrier uses explicit `## Phase N` headlines with monotonic numbering. This makes them natural seams for JIT-disclosure — a phase header is a known boundary at which the carrier can "fetch the next slab of instructions" without losing place.

Parity enforcement: `scripts/verify-shared-blocks.sh` SHA256s each block-marker pair across carriers; pre-commit + CI gate drift. Once a block is collapsed to a single source, the manifest entry can be deleted (the parity check becomes vacuous for that block).

### Agent / skill / template surface (Phase 2 — Explore Agent 2)

The only existing "instruction-returning" sub-agent is `flow-bootstrap` (113 LOC): it returns a JSON envelope of pre-flight context that all 9 carriers consume identically at Step 0. This is the canonical JIT pattern in the codebase — and the model the user wants to extend.

Other agents (`flow-research`, `flow-research-deep`, `flow-implement-lite`, `flow-implement-deep`, `verification`) all return *work products* (findings, applied tags, command outcomes), not instructions. There is no `claude/agents/instruction-deliver.md` or equivalent — that gap is the design opportunity.

Skills today: `test-author` (model-discoverable, trigger-phrase-activated), `tomlctl` (infrastructure — direct Bash invocation, used by orchestrators for TOML I/O). The `tomlctl` skill is the natural home for surfacing tomlctl-side instruction-emission once that capability exists.

Templates: `scripts/templates/flow-context.md` exists as a design checkpoint that *mirrors* the embedded shared block. This is already a partial step toward externalising contract text — but it's still consumed by humans editing carriers, not by the runtime at invocation time.

### tomlctl surface (Phase 2 — Explore Agent 3)

Top-level subcommands: `parse`, `get`, `set`, `set-json`, `validate`, `items`, `blocks`, `array-append`, `capabilities`, `integrity`, `flow`, `json`.

The `flow` family handles every read/write the carriers do today: `flow resolve`, `flow init`, `flow doctor`, `flow active {list,add,remove,touch}`, `flow ensure-artifact`, `flow find-plans`, `flow stale`, `flow list`. The resolved envelope (from `flow resolve`) is the shape `flow-bootstrap` wraps — meaning the JIT-disclosure infrastructure already runs through tomlctl one level down.

**Template/instruction emission capability: ABSENT.** tomlctl has no `template emit` subcommand. The natural module location is `tomlctl/src/template.rs` alongside `blocks.rs`, `items.rs`, `flow/`. A static template registry could live at `claude/contracts/<name>.md` (or be baked into the binary via `include_str!`); `tomlctl template emit <name>` would stdout the file body, and carriers + sub-agents could compose those emissions on demand.

Build/test/lint commands (verified):
- `cargo build --manifest-path tomlctl/Cargo.toml`
- `cargo test --manifest-path tomlctl/Cargo.toml`
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`
- `bash scripts/verify-shared-blocks.sh`

### Key constraints discovered

1. **Plan-mode write boundary**: `/plan-new` cannot bootstrap the flow until Phase 9 (post-`ExitPlanMode`). This affects *this* plan's own bootstrap but is unrelated to the design — it's already handled by the existing Phase 9 structure.
2. **Parity manifest must shrink as blocks collapse**: every block we externalise removes a manifest entry. The verifier must remain green throughout migration; an interim where a block exists *both* embedded and externalised will break parity unless we update the manifest in the same commit.
3. **Caching consideration (assistant context)**: the harness loads carrier text at command invocation. Anthropic prompt caching has 5-minute TTL — a JIT-fetched block re-loads every time but is only paid for in the turns that actually need it. The win compounds only when blocks are *not* read on the common path.
4. **Sub-agent dispatch latency**: each `Task(subagent_type: ...)` call has a setup cost. Aggregating multiple instruction-fetches into one sub-agent return (the way `flow-bootstrap` bundles `resolved` + `doctor` + `plans_directory`) is cheaper than N sequential single-block calls.
5. **Discoverability**: a carrier shrunk to a thin orchestrator must still be skim-readable end-to-end. The "skeleton" must communicate enough about the contract for a human reviewer to understand the flow without round-tripping into JIT loads.

### Phase-structure observations

All 10 carriers use explicit Phase numbering. This is the natural seam: each phase's prose can be replaced by `<fetch contract:phase-N>` directives, with the carrier file containing only the orchestration glue + phase-level summaries that survive a cold read.

## Research Notes

> Phase 3 vet pass (orchestrator): 3 of 10 findings sampled; 0 dropped; Finding 10 downgraded medium (specific Opus 4.7 cache-threshold claim could not be cross-verified). Durable `[[vet_events]]` entry deferred to Phase 9 bootstrap (no ledger exists pre-flow-init).

External grounding on the progressive-disclosure patterns Claude Code already publishes. The research returned `EARLY-RETURN: minimal external guidance, design is harness-author judgement` — the mechanisms are documented but no prescriptive harness-author guide exists. Synthesis is on us.

### Skills are the explicitly-recommended vehicle for prompt-based instructions

Anthropic's docs state, verbatim, "To extend Claude with reusable prompt-based workflows, write a skill, which runs through the existing Skill tool rather than adding a new tool entry." (`code.claude.com/docs/en/skills`, `code.claude.com/docs/en/tools-reference`). This is the single most decisive finding: instruction text should live in skills, not in MCP resources, not in tomlctl subcommands that emit text.

Skill loading mechanics: skill *descriptions* (capped at ~1,536 chars combined with `when_to_use`) are always loaded; the full `SKILL.md` body loads only when invoked. Description budget scales at 1% of the model's context window. Three invocation modes: default (description always in context, body on invoke), `disable-model-invocation: true` (description NOT in context — pure user-`/name` invocation), `user-invocable: false` (description in context, model auto-invokes). Impact: the 6,800 lines of carrier text decompose into skills whose descriptions sum to a tiny always-loaded preamble, with bodies loading only when the carrier needs them.

### ToolSearch / deferred-tool pattern is the architectural precedent

`ToolSearch` is a built-in Claude Code tool that defers MCP tool *schemas* until Claude actually needs them — only tool *names* enter the session at start (`code.claude.com/docs/en/mcp#scale-with-mcp-tool-search`). This is the exact pattern the user observed in this session and asked us to mirror. The harness equivalent is: keep a thin orchestration skeleton always-loaded; deliver full instruction blocks via sub-agent dispatch or skill invocation. ToolSearch requires Sonnet 4+ / Opus 4+ (`ENABLE_TOOL_SEARCH=auto`; threshold = 10% of context window) — Haiku does NOT support it, so any harness pinning a Haiku model would silently fall back to full upfront load. *Impact*: our flow agents run on Sonnet/Opus; this constraint is satisfied today.

### MCP resources for instruction text are explicitly NOT the recommended path

`ListMcpResourcesTool` + `ReadMcpResourceTool` enable an "instruction text as resource" pattern, but Anthropic's docs do not recommend it for prompt-based content. MCP server startup adds subprocess / HTTP cost vs zero-cost skill loading. *Impact*: rules out hosting carrier prose in `tomlctl` (or any MCP server) purely for instruction-delivery purposes. tomlctl-as-data-tool stays; tomlctl-as-text-emitter is deprioritised (see User Decisions / Design for nuance).

### Prompt-caching architecture is the real risk

Cache breakpoints (`cache_control`) must be placed on the *last block whose prefix is identical across requests*. If a per-turn variable block appears at or before the breakpoint, every request becomes a cache write (1.25× input cost, 5-min TTL) — the documented "Common Mistake" failure mode (`platform.claude.com/docs/en/build-with-claude/prompt-caching`). For a harness that swaps instruction blocks per command, the stable preamble MUST sit before the final breakpoint, with the per-command variable block appended after. Reads cost 0.1× base input. *Impact*: decomposition is correct caching architecture IF block ordering respects this. Cache-aware acceptance criterion: after decomposition, residual always-loaded preamble must exceed the model's minimum cacheable length (1,024 tokens for Sonnet 4.5/4.6, 4,096 for Haiku 4.5 — Opus 4.7 threshold uncertain, treat as 4,096 to be safe).

### Sub-agent dispatch is the established JIT carrier in this codebase

`flow-bootstrap` is the canonical pattern: a sub-agent that accepts a JSON envelope and returns a JSON envelope. The user's instinct to extend this pattern matches the documented direction — sub-agents are the natural way to fetch JIT prose because they bypass the parent context entirely (the parent only sees the agent's return string, not the agent's own preamble). *Impact*: a new "instruction-deliver" agent (or a generalised pattern over phase-scoped sub-agents) is the minimum-friction first step. Skills are a stronger fit for content that is shared across multiple carriers (the `vet-flow-research` block, the `execution-record-schema`, the apply-protocol family).

### Synthesis: three-mechanism architecture emerges

The findings point to a three-mechanism delivery system, each addressing a different class of bloat:

1. **Skills** for shared *contract text* (schemas, protocols, vet procedures) — these are stable, version-controlled, and Anthropic's documented recommendation for "reusable prompt-based workflows".
2. **Sub-agents** for *per-phase JIT delivery* (loading the next phase's instructions when the previous phase completes) — this mirrors `flow-bootstrap`'s precedent and bypasses parent-context inflation entirely.
3. **tomlctl** for *data-shaped operations* the carrier currently inlines as prose (e.g. "build this dispatch envelope according to these 30 lines of schema" → `tomlctl flow envelope build --command plan-new ...`) — collapses prose into a single CLI invocation whose output is structurally guaranteed.

The user's secondary directive (surface mechanisms via the `tomlctl` skill where appropriate) is satisfied by mechanism (3): tomlctl gains subcommands that encode prose-as-data, the `tomlctl` skill documents them, and the carrier loses pages of prose in favour of one Bash line.

#### Sources

- Claude Code MCP / ToolSearch: https://code.claude.com/docs/en/mcp
- Claude Code tools reference: https://code.claude.com/docs/en/tools-reference
- Claude Code skills: https://code.claude.com/docs/en/skills
- Anthropic prompt caching: https://platform.claude.com/docs/en/build-with-claude/prompt-caching

## User Decisions

> Phase 4 user-engagement gate — answers captured 2026-05-19. All Phase 4 answers cited here come from the user; treat as authoritative direction.

### Q1 — Mechanism partition

**Chosen:** Skills-first, sub-agents only where context isolation matters. **User's clarification:** *"tomlctl is not for handling prose but performing actions to store or retrieve data efficiently which would otherwise require more context for agents to perform manually."*

**Implication:** Skills become the primary vehicle for *all* prompt-based content — contracts, schemas, vet procedures, apply protocols, phase-level instructions. Sub-agents are reserved for cases where parent-context isolation is essential (the `flow-bootstrap` precedent stays; new sub-agents only when isolation is the actual win, not as a generic JIT delivery channel). tomlctl scope is data-only — no text/template emission.

> Prompted by: Research Notes §1 (Anthropic's documented skill recommendation) + §4 (cache-thrash risk).

### Q2 — Migration strategy

**Chosen:** Pilot one carrier end-to-end, then propagate.

**Implication:** Pick one carrier, overhaul it completely, validate (parity check still green; cache behaviour measured; skim-readability sanity-checked; Phase-4-style user-engagement gates preserved). Use that as the template for propagation. Parity manifest entries shrink one block per migrated carrier — never both embedded and externalised in the same commit.

> Prompted by: Exploration Notes Constraint 2 (parity must stay green) + Constraint 5 (skim-readability target).

### Q3 — Skeleton size ceiling

**Chosen:** ≤ 100 LOC per carrier, phase summaries + dispatch lines.

**Implication:** Each carrier collapses to ~10× reduction from current state. Each phase keeps a 1-paragraph human-readable summary + the dispatch line(s) that load the full contract. A reviewer can read the skeleton cold and understand the flow without round-tripping JIT loads.

> Prompted by: Exploration Notes Constraint 5 (skim-readability) + Research Notes §4 (cache-stable preamble must remain meaningful).

### Q4 — tomlctl scope

**Chosen:** Data-shaped operations only — no text emission.

**Implication:** tomlctl gains subcommands that collapse *prose-as-data* (envelope construction, phase progression, contract-list enumeration) but **never** emits instruction prose. Examples: `tomlctl flow envelope build --command <name>` (returns the JSON input envelope for a sub-agent dispatch, replacing 20 lines of inline carrier prose), `tomlctl flow phase status` (returns current phase state), `tomlctl items list-contracts --carrier <name>` (returns the list of skills/contracts a carrier requires). All output is structured (JSON / TOML); no markdown/instruction text crosses the tomlctl boundary.

> Prompted by: user's secondary directive (where could tomlctl host actions surfaced via the tomlctl skill?) + Research Notes §3 (MCP/tomlctl text emission deprioritised by Anthropic guidance).

### Phase 5 outcome

**Skipped.** Phase 4 answers surfaced no unresearched topics. Mechanical trigger check:

- "skills" — covered by Research Notes §1 (skill loading mechanics, invocation modes, description budget).
- "sub-agents / context isolation" — covered by Research Notes §1, §5 (flow-bootstrap precedent, ToolSearch deferred-tool pattern).
- "tomlctl data-shaped operations" — codebase-internal, not a research topic; explored fully in Phase 2 Agent 3 inventory.
- "pilot carrier strategy" — strategic decision, not a research question.

All Phase 4 answer key-terms grep-match Research Notes content; no library/API was introduced that wasn't already covered. Skipping per the `/plan-new` Phase 5 trigger procedure.

## Scope

- **In scope (this plan)**: Overhaul `/review` (777 LOC) as the pilot carrier. Establish the four scaffolding primitives that the remaining 9 carriers will adopt: (1) contract-skill pattern, (2) carrier skeleton format ≤100 LOC, (3) parity-manifest migration discipline, (4) `tomlctl flow envelope build` subcommand. Validate end-to-end before propagation.
- **Out of scope (deferred to follow-up plans)**: Overhauling `/optimise`, `/review-apply`, `/optimise-apply`, `/plan-new`, `/plan-update`, `/implement`, `/review-plan`, `/tdd`, `/test-bootstrap`. Removing the parity verifier entirely (only when all blocks are externalised). Skill consolidation (merging closely-related contracts after propagation surfaces dedup opportunities).
- **Affected areas**: `claude/commands/review.md` (rewrite), `claude/skills/flow-contract-*` (new — 4 contract skills), `tomlctl/src/flow/` (new envelope module + clap variant + dispatch route + tests), `scripts/shared-blocks.toml` (remove `/review` from 4 carrier lists), `docs/plans/harness-progressive-disclosure/PILOT-LESSONS.md` (new).
- **Estimated file count**: 13 unique files (under the 15-file scope guard).

### Propagation follow-up (deferred carriers + gate)

The 9 deferred carriers below are tracked here as the propagation backlog. **Propagation gate**: each follow-up carrier may proceed only once the prerequisites below are satisfied. A documented checklist is the tracking artefact — no new flow directories are created until a follow-up plan is opened.

Deferred carriers (one propagation unit each):

- [ ] `/optimise`
- [ ] `/review-apply`
- [ ] `/optimise-apply`
- [ ] `/plan-new`
- [ ] `/plan-update`
- [ ] `/implement`
- [ ] `/review-plan`
- [ ] `/tdd`
- [ ] `/test-bootstrap`

Gate prerequisites (must all hold before the first follow-up carrier migrates):

- [x] **Skill-invocation mechanism validated live** — satisfied 2026-05-20 by a real `/review harness-progressive-disclosure` + `/review-apply` run (see PILOT-LESSONS.md §"Skill-invocation mechanism" → "Update — VALIDATED LIVE"). The natural-language "Invoke skill `<name>`" directive loads skill bodies at the correct phase boundaries.
- [ ] **Doctor-of-skills drift check landed** — add the `verify-against-skill` check (PILOT-LESSONS recommendation 7 / Risk 4) that diffs each externalised skill body against carriers still embedding the same block, before the first propagation PR merges.
- [ ] **Automated skill-loading smoke check (or documented manual re-validation step)** — the live validation is once-only; the CLI toolchain cannot exercise skill loading. Add a fixture-based smoke check or a mandatory per-PR manual re-validation step so a future skill-loading regression is caught mechanically (see PILOT-LESSONS §"Update — VALIDATED LIVE").

## Approach

> ### Spec corrections (post-pilot, 2026-05-20)
>
> The pilot shipped conventions that diverge from the original spec sketches below. **Propagation authors reading this plan as the spec MUST follow the shipped conventions, not the pre-pilot sketches.** Full rationale and deviation IDs are in `docs/plans/harness-progressive-disclosure/PILOT-LESSONS.md` §"Recommended changes for propagation".
>
> 1. **Skill frontmatter is 2-field, not 5-field.** Use `name` + `description` only. The `when_to_use`, `user-invocable: false`, and `disable-model-invocation: false` fields in the YAML block below were a research-derived hypothesis that does not match the on-disk convention (precedents: `tomlctl/SKILL.md`, `test-author/SKILL.md`). Logged as deviation E2.
> 2. **Description budget is ~500–1500 chars, not ≤150 chars.** The ≤150-char target below is wrong; shipped contract-skill descriptions run ~500–1500 chars (single long line combining what the contract defines and when to consult it). The combined-budget arithmetic in the YAML rationale paragraph and Risk 2 should be re-derived against this larger per-skill size before propagation.
> 3. **Carriers use `## Step N` headers, not `## Phase N`.** The skeleton sketch below (and the "preserve every existing Phase header" task language) assumes `## Phase` headers; `/review` actually uses `## Step N` exclusively. Preserve each carrier's *actual* existing header names/numbering verbatim — do not impose a generic phase template.

### Architectural model: skills as primary contract host

Every shared block currently embedded in `/review` becomes a standalone skill at `claude/skills/flow-contract-<block-name>/SKILL.md`. Skill frontmatter:

```yaml
---
name: flow-contract-<block-name>
description: <≤150 chars — what this contract defines and when to consult it>
when_to_use: <≤150 chars — concrete trigger ("when about to write to the review ledger")>
user-invocable: false
disable-model-invocation: false
---
```

Description budget rationale: ≤150 chars × ~10 contract skills (after propagation) = ~1,500 chars combined — well under the 1% of context window (~2,000 tokens for Sonnet 4) that Anthropic's skills docs cite. `user-invocable: false` keeps these skills out of the `/` menu (they are not user-facing); `disable-model-invocation: false` lets the carrier's prose direct the model to load them.

The skill body is the verbatim block content (with the `<!-- SHARED-BLOCK:NAME START/END -->` markers stripped). One canonical source per contract; the byte-identical parity check that the current shared-block verifier enforces becomes vacuous as the embedded copies disappear from carriers.

### Carrier skeleton format (≤100 LOC)

`/review` collapses to a skeleton with this structure:

```markdown
# /review — code review across five lenses

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

## Step 0 — Pre-flight
Dispatch `flow-bootstrap`. Build the input envelope via `tomlctl flow envelope build --command review --branch ... --worktree ...`. Bind `slug` / `context_path` / `artifacts.*` / `doctor.ok` for downstream phases. Emit the bootstrap-summary console line.

## Phase 1 — Scope & ledger setup
[1-paragraph summary of what this phase achieves.]
Invoke skill `flow-contract-flow-context` for the flow-resolution contract.
Invoke skill `flow-contract-ledger-schema` for the ledger schema before writing.

## Phase 2 — Five-lens research (parallel agents)
[1-paragraph summary.]
Invoke skill `flow-contract-vet-research` to load the vet-pass procedure that runs after each research agent returns.

## Phase 3 — Findings synthesis & ledger writes
[1-paragraph summary.]
Reference skill `flow-contract-ledger-schema` for the canonical `[[items]]` shape.

## Phase 4 — Disposition sweep
[1-paragraph summary.]
Invoke skill `flow-contract-ledger-disposition-sweep` for the sweep procedure.

## Phase 5 — Next steps
Emit user-facing summary; suggest `/review-apply`.
```

Target LOC: 90 ± 10. Every phase keeps a 1-paragraph human-readable summary so a cold reviewer understands the flow without round-tripping into skill bodies. The skill-invocation lines are explicit instructions ("Invoke skill X") — relying on the model to honour the directive, not on a special markdown syntax.

### tomlctl envelope builder (data-only — no prose)

New subcommand `tomlctl flow envelope build` returns the canonical Step-0 input envelope as JSON. Replaces ~15 lines of inline envelope-template prose in every carrier with one CLI invocation:

```bash
tomlctl flow envelope build \
  --command review \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)"
# stdout: {"command":"review","flow_override":null,"path_args":[],"branch":"main","worktree":"...","cwd":"...","require_artifacts":[],"staleness_threshold":"7d"}
```

Implementation: clap-derive variant on `FlowOp` (`tomlctl/src/cli/types.rs`), new module `tomlctl/src/flow/envelope.rs`, route in `tomlctl/src/flow/dispatch.rs`, cargo test at `tomlctl/tests/flow_envelope.rs`. The envelope schema is a typed Rust struct; extras (e.g. `path_args` JSON, `require_artifacts`) accepted via repeatable `--path-arg X` and `--require-artifact NAME` flags. Output is `serde_json::to_string(&envelope)` — structurally guaranteed by the type system; carriers stop hand-rolling the JSON.

This is the canonical "tomlctl performs an action that would otherwise require more context for agents to perform manually" (per the user's Q1 clarification): instead of the carrier carrying 15 lines telling the model how to construct the envelope, the carrier carries one Bash line that emits it.

### Parity-manifest migration discipline

`scripts/shared-blocks.toml` is updated in the same commit as `review.md`'s rewrite. The four blocks `/review` carried (`flow-context`, `ledger-schema`, `vet-flow-research`, `ledger-disposition-sweep`) have their `carriers` arrays edited to remove `claude/commands/review.md`. The verifier then no longer expects those blocks in `/review`. Other carriers still embed the blocks; parity remains green for them. As propagation lands, additional `carriers` entries shrink; when a block's carrier list becomes empty, the manifest entry is deleted entirely.

A top-of-file comment on `shared-blocks.toml` records the migration: "Carriers are progressively externalising shared blocks into `claude/skills/flow-contract-*/SKILL.md`. Entries shrink as carriers migrate; entries deleted when fully migrated."

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
parity: bash scripts/verify-shared-blocks.sh
```

## Tasks

### Batch 1 (parallel — independent files)

#### 1. Create `flow-contract-flow-context` skill [S]
- **Files**: `claude/skills/flow-contract-flow-context/SKILL.md` (NEW)
- **Depends on**: —
- **Action**: Extract the `flow-context` shared block from `claude/commands/review.md` (lines bracketed by `<!-- SHARED-BLOCK:flow-context START/END -->`) verbatim. Strip the marker comments. Prepend the skill frontmatter (name, description ≤150 chars, when_to_use ≤150 chars, `user-invocable: false`, `disable-model-invocation: false`).
- **Detail**: Description: "Flow resolution + doctor contract — how the bootstrap envelope is consumed and what `envelope.resolved.*` fields bind for downstream phases." when_to_use: "Consult when a carrier dispatches `flow-bootstrap` and needs to know how to interpret the returned envelope."
- **Acceptance**: File exists; frontmatter parses; `wc -c` on the body matches the original block byte length (excluding markers); `bash scripts/verify-shared-blocks.sh` still green (we have not yet edited the manifest or the carrier).

#### 2. Create `flow-contract-ledger-schema` skill [S]
- **Files**: `claude/skills/flow-contract-ledger-schema/SKILL.md` (NEW)
- **Depends on**: —
- **Action**: Extract the `ledger-schema` shared block from `claude/commands/review.md` verbatim into the skill body; prepend frontmatter.
- **Detail**: Description: "Review/optimise ledger schema — `[[items]]` table shape for `review-ledger.toml` and `optimise-findings.toml`." when_to_use: "Consult before any read or write of `review-ledger.toml` or `optimise-findings.toml`."
- **Acceptance**: File exists; frontmatter parses; body byte length matches original block.

#### 3. Create `flow-contract-vet-research` skill [S]
- **Files**: `claude/skills/flow-contract-vet-research/SKILL.md` (NEW)
- **Depends on**: —
- **Action**: Extract the `vet-flow-research` shared block from `claude/commands/review.md` verbatim; prepend frontmatter.
- **Detail**: Description: "Universal vet-pass procedure for research-agent output — triage, spot-check, drop, log to `[[vet_events]]`." when_to_use: "Consult immediately after a `flow-research` or `flow-research-deep` agent returns, before persisting findings."
- **Acceptance**: File exists; frontmatter parses; body byte length matches original block.

#### 4. Create `flow-contract-ledger-disposition-sweep` skill [S]
- **Files**: `claude/skills/flow-contract-ledger-disposition-sweep/SKILL.md` (NEW)
- **Depends on**: —
- **Action**: Extract the `ledger-disposition-sweep` shared block from `claude/commands/review.md` verbatim; prepend frontmatter.
- **Detail**: Description: "Ledger disposition sweep procedure for /review and /optimise — surfaces stale or duplicate findings." when_to_use: "Consult during the disposition-sweep phase of /review or /optimise."
- **Acceptance**: File exists; frontmatter parses; body byte length matches original block.

#### 5. Add `tomlctl flow envelope build` subcommand [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/flow/envelope.rs` (NEW), `tomlctl/src/flow/dispatch.rs`, `tomlctl/src/flow/mod.rs`, `tomlctl/tests/flow_envelope.rs` (NEW), `claude/skills/tomlctl/SKILL.md` (UPDATE — document the new subcommand)
- **Depends on**: —
- **Action**: Add `EnvelopeBuild { command: String, flow_override: Option<String>, branch: Option<String>, worktree: Option<String>, cwd: Option<String>, path_arg: Vec<String>, require_artifact: Vec<String>, staleness_threshold: String /* default "7d" */ }` to `FlowOp`. Implement `envelope::build` in the new module — typed struct, serde_json serialisation, stdout output. Wire dispatch route. Add cargo test covering all-fields-set and minimal-args cases (output JSON shape stable). Update the `tomlctl` skill SKILL.md to advertise the new subcommand — short example invocation + 1-sentence purpose. Bumps the carrier-discoverable surface per the user's "made available via the tomlctl skill" directive.
- **Detail**: The typed struct mirrors the canonical envelope documented in `claude/agents/flow-bootstrap.md` Contract section. `path_arg` and `require_artifact` are repeatable string flags collected into `Vec<String>`; `path_arg` values are pushed verbatim into the output's `path_args` array. `staleness_threshold` defaults to `"7d"` via clap's `default_value`. Failure modes: invalid `--command` (not in the carrier whitelist) → exit 2 with error message naming valid commands. The skill update is a small additive change — append a new "Envelope construction" section to the existing SKILL.md without rewriting it.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_envelope` passes; `cargo clippy` clean; `tomlctl flow envelope build --command review --branch main` emits valid JSON parseable by `jq`; `claude/skills/tomlctl/SKILL.md` contains a new section referencing `flow envelope build` with an example invocation line.

### Batch 2 (sequential — after Batch 1)

#### 6. Rewrite `/review` as ≤100 LOC skeleton [M]
- **Files**: `claude/commands/review.md` (REWRITE)
- **Depends on**: 1, 2, 3, 4, 5
- **Action**: Replace the file's full content with a skeleton matching the structure in the Approach section. Each phase: 1-paragraph summary + skill-invocation directive(s) + dispatch line(s). The Step-0 envelope construction uses `tomlctl flow envelope build --command review ...` rather than inline JSON-template prose. Preserve every existing Phase header verbatim (carrier consumers may grep for them).
- **Detail**: Skim-readability is the explicit acceptance constraint. Every phase summary MUST stand alone: a reviewer reading only the skeleton must understand what the phase does, even without loading the contract skills. Skill-invocation lines use natural-language directives ("Invoke skill `flow-contract-vet-research` before proceeding") — not a special markdown syntax.
- **Acceptance**: `wc -l claude/commands/review.md` ≤ 100; every Phase header present (regex `^## Phase \d+`); skill-invocation lines reference all four contract skills created in Batch 1.

### Batch 3 (sequential — after Batch 2)

#### 7. Update `scripts/shared-blocks.toml` to drop `/review` from migrated blocks [S]
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: 6
- **Action**: For each of the four blocks `flow-context`, `ledger-schema`, `vet-flow-research`, `ledger-disposition-sweep`, remove `claude/commands/review.md` from the `carriers` array. Add a top-of-file comment documenting the migration policy.
- **Detail**: Do NOT delete the block entries entirely — other carriers still embed them. Only `/review`'s entry leaves each `carriers` list. The verifier reads the manifest and checks only the named carriers; once `/review` is removed from a block's list, the verifier no longer expects that block in `/review`.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0.

### Batch 4 (sequential — after Batch 3)

#### 8. End-to-end pilot validation [M]
- **Files**: none (read-only validation)
- **Depends on**: 7
- **Action**: Run `/review` against a test flow in this repo (or a controlled fixture). Confirm: (a) flow-bootstrap dispatches and binds correctly using the new envelope; (b) each phase loads its contract skill when the dispatch directive triggers; (c) ledger writes use the schema from the loaded skill; (d) the vet pass runs after research agents return; (e) end-to-end execution matches the pre-overhaul behaviour. Capture `wc -l` for the new carrier; cargo binary diff (or feature parity assertion) for tomlctl.
- **Detail**: This is the single most important task in the plan — it's where we discover whether skill-invocation-via-prose actually loads contract bodies reliably, or whether the model needs a stronger directive (e.g. an explicit `Skill` tool dispatch). If the latter, document the workaround and incorporate into Task 9.
- **Acceptance**: `/review` produces a ledger entry with the same fields as the pre-overhaul version; `bash scripts/verify-shared-blocks.sh` still green; carrier LOC ≤ 100.

#### 9. Document pilot lessons [S]
- **Files**: `docs/plans/harness-progressive-disclosure/PILOT-LESSONS.md` (NEW)
- **Depends on**: 8
- **Action**: Write a 1-2 page document capturing: what worked, what surprised, recommended adjustments before propagation, the skim-readability verdict, any cache-behaviour observations (token counts before/after).
- **Detail**: This document feeds directly into the follow-up plan(s) for propagating the pattern to the other 9 carriers. Specific sections required: "Carrier LOC reduction", "Contract skills created (with descriptions verbatim)", "tomlctl subcommand validation", "Skill-invocation mechanism (worked / needed workaround)", "Recommended changes for propagation".
- **Acceptance**: File exists; contains at least the five named sections; references the four contract skills and the new tomlctl subcommand by exact name.

## Dependency Graph

```
Batch 1 (parallel):  T1, T2, T3, T4, T5
Batch 2 (sequential): T6 (needs T1, T2, T3, T4, T5)
Batch 3 (sequential): T7 (needs T6)
Batch 4 (sequential): T8 (needs T7) → T9 (needs T8)
```

5 parallel tasks in Batch 1 — at the upper edge of the "3-4 parallel agents max" guidance, but each task is isolated (different file or different module), so coordination risk is low.

## Verification

Pilot acceptance gate (run all):
- `cargo build --manifest-path tomlctl/Cargo.toml` → clean build
- `cargo test --manifest-path tomlctl/Cargo.toml` → all tests pass, including new `flow_envelope` tests
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` → no warnings
- `bash scripts/verify-shared-blocks.sh` → exit 0 (parity green for remaining carriers)
- `wc -l claude/commands/review.md` → ≤ 100
- Manual smoke test: `tomlctl flow envelope build --command review --branch main --worktree /c/Users/rossa/dev/dev-tools` → emits valid JSON; `... | jq .` parses without error
- End-to-end manual: invoke `/review` against a fixture flow; compare resulting ledger entries to a snapshot of pre-overhaul output (functional parity)

## Risks

1. **Skill-invocation via prose may not reliably load contract bodies** — The plan assumes the model honours markdown directives like "Invoke skill `flow-contract-vet-research`" by loading the skill body. If the model instead treats this as descriptive prose, the contract content never reaches its context. *Mitigation*: Task 8 explicitly tests this; if it fails, fall back to inserting an explicit `Skill` tool invocation in the carrier (the carrier directs the model to call the `Skill` tool with the skill name as argument). Document the working approach in PILOT-LESSONS.md before propagation.

2. **Skill description budget pressure after propagation** — The 1% context-window budget (~2,000 tokens for Sonnet 4) for *combined* skill descriptions caps how many contract skills we can have. Four skills at 150 chars each = ~600 chars; safe today. After propagation adds 6–8 more, we may hit the budget. *Mitigation*: cap description length at 150 chars (enforced at skill creation); audit total description budget as part of each propagation PR.

3. **Cache thrash from per-turn skill swapping** — Each invocation of `/review` loads a different set of contract skills than `/plan-new` does. If the skill bodies land before the prompt cache breakpoint, every command swap is a cache write. *Mitigation*: skill bodies load INTO the conversation context, NOT into the system prompt (per Anthropic docs); they sit after the final cache breakpoint by construction. The stable preamble (system prompt, top-level tools) remains cached. Validate during Task 8 by inspecting token counts via the API response's `cache_*` fields.

4. **Parity verifier blind spots** — Once `/review` no longer carries the externalised blocks, the verifier cannot detect drift between the *skill body* and the *embedded copies still in other carriers*. During the migration window (before propagation completes), the skill body and the in-place embedded copies must stay byte-identical. *Mitigation*: add a one-shot CI check that diffs the skill body against each remaining embedded carrier copy; fail on drift. This check is added as part of Task 7 if time permits, or deferred to the first propagation PR.

5. **`/review` happens to depend on a block from another carrier** — If `/review` invokes a tomlctl subcommand or sub-agent that returns content reliant on a block not yet externalised, we have a hidden dependency. *Mitigation*: Task 8's end-to-end run exercises the full path; any cross-block dependency surfaces there. Document in PILOT-LESSONS.md.

6. **Bigger-than-expected `vet-flow-research` block content drift across carriers** — The block is byte-identical across 6 carriers today (verifier enforces it), but when we extract the *content* of `/review`'s copy into the skill, we are encoding one specific snapshot. If we propagate to another carrier later and find that carrier's embedded copy differs (e.g. it was independently edited in a non-parity-checked commit), we have to reconcile. *Mitigation*: low probability (the verifier prevents this drift), but Task 9's lessons document records the assumption explicitly.
