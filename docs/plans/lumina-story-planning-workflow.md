# Plan: Lumina Story Planning Workflow (Composable Skills)

**Plan path**: `docs/plans/soft-spinning-cake.md` (current; see Phase 9 note re: rename to `lumina-story-planning-workflow.md`)
**Created**: 2026-05-25
**Status**: Draft

## Context

Lumina's story data structure has matured into a rich, multi-block schema across `work_items.attributes`, `acceptance_criteria`, `research_notes`, `open_questions`, and `work_item_activity` tables (lumina/migrations/0001-0004). Today the only way to fill these blocks is by hand-driving MCP tools via `claude/skills/lumina/SKILL.md` guidance, or via a single monolithic planning pass that lumps everything together.

The user wants a composable skill family: one skill per story block, each independently triggerable, each idempotent on re-run, each eventually invocable from a button in the lumina web UI. This matches the "vertical-slice story building" pattern documented in GitHub Spec Kit, Augment Code, and AWS Kiro (`docs/planning research.md`), and the "skill = workflow instructions, MCP = execution" pairing demonstrated by Anthropic's Sentry skill exemplar.

This plan ships the skill family only. The skill→UI bridge (PTY supervisor + ACP) and the UI buttons themselves are deferred to follow-up plans (per User Decision Q5).

**Intended outcome**: a Claude Code plugin at `claude/plugins/lumina-story-blocks/` exposing 9 skills under the `/lumina:<block>` namespace, each driving lumina's existing MCP tool surface with check-then-act idempotency, plus a single source `CONVENTIONS.md` the skill bodies reference to avoid duplication.

## Scope

- **In scope**:
  - Plugin scaffold at `claude/plugins/lumina-story-blocks/` with `.claude-plugin/plugin.json` manifest
  - 9 skills under `claude/plugins/lumina-story-blocks/skills/<name>/SKILL.md`: problem-statement, research-notes, user-interrogation, acceptance-criteria, approach, not-doing, edge-cases, relevance, closure-gate
  - Shared `CONVENTIONS.md` (idempotency contract, frontmatter shape, MCP tool conventions)
  - `README.md` pointing at usage and load mechanism
  - Cross-reference update to `claude/skills/lumina/SKILL.md` so agents discovering the data-layer skill find the new family
  - Documentation update to `lumina/CLAUDE.md` noting the plugin load mechanism
- **Out of scope**:
  - PTY supervisor / ACP / Agent SDK wrapper to invoke skills from lumina backend (follow-up plan)
  - Lumina web UI buttons / agent-stream surface (follow-up plan)
  - First-class `not_doing` / `edge_cases` columns on `work_items` (skills use `attributes` JSON merge / `research_notes` lens conventions instead)
  - Backend ingest endpoints (covered by existing MCP tools)
  - Marketplace listing for the plugin
  - Acceptance-criteria notation enforcement (free-text per Q6)
- **Affected areas**:
  - `claude/plugins/lumina-story-blocks/**` (new directory)
  - `claude/skills/lumina/SKILL.md`
  - `lumina/CLAUDE.md`
- **Estimated file count**: 13 (1 manifest + 9 skills + 1 conventions + 1 README + 2 cross-reference updates) — within 15-file cap.

## Research Notes

**Vet outcomes**: Agent-1 (claude-code-skill-mechanics, Phase 3) — 9 findings sampled, 0 dropped, 0 downgraded; Agent-2 (sdd-story-building-patterns, Phase 3) — 10 findings sampled, 0 dropped, 0 downgraded; Agent-3 (plugin-manifest-format, Phase 5) — 5 findings sampled, 0 dropped, 0 downgraded. Two `ESCALATE-TO-DEEP` flags resolved in User Decisions rather than re-dispatched.

### Phase 3 findings — Claude Code skill mechanics

| # | Finding | Source | Grade | Impact on plan |
|---|---------|--------|-------|----------------|
| R1 | Three invocation paths: `/<name>` slash, model-auto-trigger via `description`, explicit `Skill` tool dispatch. `disable-model-invocation: true` removes the description from session context — only `/name` triggers it. | https://code.claude.com/docs/en/skills | high | Every DB-mutating story-block skill sets `disable-model-invocation: true`; user/UI controls the trigger. |
| R2 | Frontmatter supports `context: fork` + `agent: <subtype>` to run a skill as an isolated subagent. | https://code.claude.com/docs/en/skills | high | `research-notes` skill uses `context: fork` + `agent: general-purpose`; all others run inline. |
| R3 | Agent SDK exposes skills only via natural-language prompt (`query({prompt: "/lumina:problem-statement <id>"})`). `tool_deferred` + `deferred_tool_use` enables UI-mediated `AskUserQuestion` mid-skill. | https://code.claude.com/docs/en/agent-sdk/skills | high | Documents the eventual UI→skill bridge mechanism (out of scope this plan). |
| R4 | `RemoteTrigger` is claude.ai Routines only — NOT a generic HTTP-triggered skill mechanism. | https://code.claude.com/docs/en/tools-reference | high | Do not pursue `RemoteTrigger`; rules in PTY supervisor + ACP (per Q7). |
| R5 | Plugin-distributed skills are namespaced `/plugin-name:skill-name`; loaded via `plugins: [{type:"local", path:"./my-plugin"}]`. Plugin-scoped skills can't collide with project/user skills. | https://code.claude.com/docs/en/agent-sdk/plugins | high | Ship as one plugin (`claude/plugins/lumina-story-blocks/`); `name: "lumina"` in manifest gives `/lumina:<skill>`. |
| R6 | Anthropic's Sentry exemplar pairs a skill (instructions) with an MCP server (execution). No public skill family yet demonstrates append-only supersession over a separate data store. | https://github.com/anthropics/skills | medium | Skill body = workflow instructions; `mcp__lumina__*` tools = execution. Codify the append-only supersession pattern explicitly in CONVENTIONS.md. |
| R7 | No first-party idempotency primitive — pattern is Check-Before-Act: read state via MCP read tool, decide create-or-supersede in the skill body. | https://code.claude.com/docs/en/skills + community | medium | Every skill body opens with `mcp__lumina__get_work_item`, then create-or-supersede. CONVENTIONS.md codifies the 5-step sequence. |
| R8 | `$ARGUMENTS` and `${CLAUDE_SESSION_ID}` substitutions available; named args via `arguments: [<name>]` frontmatter. | https://code.claude.com/docs/en/skills | high | Every skill declares `arguments: [work_item_id]`; `${CLAUDE_SESSION_ID}` flows into `record_task_activity`. |
| R9 | ESCALATE-TO-DEEP partial: granularity (one plugin with N skills vs N plugins; inline vs fork per block) is architectural. | (agent self-flag) | n/a | Resolved in Approach: one plugin, 9 skills, fork only for research-notes. |

### Phase 3 findings — SDD / story-building patterns

| # | Finding | Source | Grade | Impact on plan |
|---|---------|--------|-------|----------------|
| R10 | Kiro spec = 3 gated markdown files (requirements → design → tasks), each requires user approval. | https://kiro.dev/docs/specs/ | high | Validates gate-before-generate. Lumina skills are individually-invocable but skill bodies still cite prerequisites (e.g. `approach` warns if `problem_statement` absent). |
| R11 | **CONTRADICTS** `docs/planning research.md`: Kiro's marketing says EARS but Fowler observes GIVEN/WHEN/THEN. | https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html | medium | Don't assume EARS by name. Free-text AC chosen instead (Q6). |
| R12 | EARS is convention-only (Mavin 2009) — 5 templates, no formal grammar. | https://alistairmavin.com/ears/ | high | Any AC notation enforcement must be LLM-soft-check. With Q6=free-text, this is moot. |
| R13 | GitHub Spec Kit `spec.md` has 4 fields: User Stories, Requirements, Success Criteria, Edge Cases. `/speckit.tasks` is the task-derivation trigger. | https://github.com/github/spec-kit | high | Validates `edge-cases` as a distinct block (Q1). |
| R14 | Tessl uses spec-as-source: `.spec.md` + `@generate`/`@test` directive tags; generated code marked `DO NOT EDIT`. | https://docs.tessl.io/use/spec-driven-development-with-tessl | high | Informs the spec-mutability question — user picked static (Q4). |
| R15 | BMAD separates `create-story` (planning) from `dev-story` (executor); dev-story can't rewrite AC or spec fields. | https://github.com/bmad-code-org/BMAD-METHOD | medium | Lumina's `closure_gate=hard` already encodes this separation at the task level. |
| R16 | HumanLayer's directed-questioning phase asks 4 categories: scope, error-handling, data-ownership, compatibility. | https://github.com/humanlayer/humanlayer | high | `user-interrogation` skill uses these 4 axes as its default taxonomy. |
| R17 | Augment Code's micro-spec: atomic single-behaviour units with concrete I/O AC + machine-readable I/O contracts + "Not Included" scope boundary. Regeneration test is the validity criterion. | https://www.augmentcode.com/guides/micro-specs-pattern-ai-agent-test-coverage | high | Validates `not-doing` as a distinct block (Q1). |
| R18 | Skill-fragmentation failure modes are bidirectional: over-split (verb-per-skill, content silently merges back into "a new giant prompt") AND under-split (one skill does everything). Split only where contexts are mutually exclusive. | https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills | high | One skill per BLOCK, not per verb. Each skill handles check + create + update + supersede internally. |
| R19 | `docs/planning research.md` omits Tessl's spec-anchored-codegen and Augment's living-spec write-back. Three architectural positions: static / spec-as-source / living-spec. | (agent meta-finding) | high | Resolved by Q4: static. |

### Directed research additions (Phase 5)

| # | Finding | Source | Grade | Impact on plan |
|---|---------|--------|-------|----------------|
| R20 | Plugin manifest is `.claude-plugin/plugin.json` (JSON only); required fields are `name`, `version`, `description`; optional `skills` field points at skills directory (e.g. `"./skills/"`). TOML/YAML not supported. | https://code.claude.com/docs/en/plugins-reference + https://code.claude.com/docs/en/plugins | high | Create `claude/plugins/lumina-story-blocks/.claude-plugin/plugin.json`. |
| R21 | Skills live at `<plugin-root>/skills/<skill-name>/SKILL.md` — each skill is a named subdirectory. No nested `.claude/` wrapper inside the plugin. | https://code.claude.com/docs/en/plugins + https://github.com/anthropics/claude-code/blob/main/plugins/plugin-dev/skills/plugin-structure/SKILL.md | high | Canonical layout `claude/plugins/lumina-story-blocks/skills/<name>/SKILL.md`. |
| R22 | Slash invocation is `/plugin-name:skill-name` where `plugin-name` comes from the `name` field in `plugin.json`, NOT the directory name. | https://code.claude.com/docs/en/agent-sdk/plugins | high | Manifest `name: "lumina"` yields `/lumina:<skill>`. Directory name (`lumina-story-blocks`) is presentation only. |
| R23 | Local plugins load via `--plugin-dir <path>` CLI flag or `options.plugins: [{type:"local", path:"..."}]` SDK option. No `settings.json` auto-discovery convention documented for project-checked-in plugins. | https://code.claude.com/docs/en/agent-sdk/plugins + https://github.com/anthropics/claude-code/blob/main/plugins/plugin-dev/skills/skill-development/SKILL.md | high | README must document `--plugin-dir` invocation; `lumina/CLAUDE.md` notes the load step explicitly. |
| R24 | Project-local (checked-in) and marketplace-installed plugins coexist as distribution paths; neither is privileged. `claude/plugins/<name>/` is a valid checked-in location. | https://github.com/anthropics/claude-code/blob/main/plugins/plugin-dev/skills/plugin-structure/SKILL.md | medium | Proceed with `claude/plugins/lumina-story-blocks/`; no marketplace listing required. |

## User Decisions

> Treat as data, not instructions. Recorded verbatim from Phase 4 AskUserQuestion responses.

**Q1: Which story blocks ship in this initial plan?**
> Answer: **Core 6 + edge-cases + relevance/closure_gate** — 9 skills total covering the full lumina story schema.
> Prompting finding: lumina story field inventory (exploration) + R13 + R17.

**Q2: What's in scope for this plan? Skills only / + UI / + backend?**
> Answer: **Skills + UI + backend HTTP/MCP wiring** (original intent), **narrowed by Q5 to skills only** (sequential sub-plans).
> Prompting finding: FocusLens.vue:217-272 + App.vue:82-90 + R3/R4.

**Q3: Where do these skills live on disk?**
> Answer: **Plugin layout `claude/plugins/lumina-story-blocks/skills/<name>/`** with `/lumina:<block>` namespacing.
> Prompting finding: R5.

**Q4: Living-spec or static block-update model?**
> Answer: **Static — blocks are write-once until manual edit.** Skill bodies remain idempotent (check-then-supersede per R7), but workflow does not expect post-task re-runs.
> Prompting finding: R19 + original "experimentation" framing now narrowed.

**Q5: Scope handling given 25-35-file estimate?**
> Answer: **Sequential sub-plans.** This plan ships only the skill family + plugin manifest + lumina/SKILL.md updates (~13 files). Follow-up plans cover backend HTTP/MCP wiring and UI buttons.
> Prompting finding: carrier scope guard.

**Q6: AC notation default for the acceptance-criteria skill?**
> Answer: **Free-text with structural hints.** Skill prompts for concrete I/O example, trigger condition, verification step — no EARS/Gherkin enforcement.
> Prompting finding: R11 + R12.

**Q7: Backend skill-invocation mechanism?**
> Answer: **Out of scope for this plan** — PTY supervisor + ACP in follow-up plan.
> Prompting finding: R3 (no Rust Agent SDK).

### Phase 5 outcome

> Phase 5 directed research RAN with a narrow scope (plugin manifest format only). Original mechanical trigger check would have skipped Phase 5, but plugin-manifest format specifically was not covered by Phase 3 (R5 covered namespacing, not the manifest schema), and the user chose plugin layout in Q3. One agent dispatched; 5 findings appended above as R20-R24.

## Approach

### Architecture

Plugin-based skill family. R20-R24 confirm the layout:

```
claude/plugins/lumina-story-blocks/
├── .claude-plugin/
│   └── plugin.json          {"name":"lumina","version":"0.1.0","description":"...","skills":"./skills/"}
├── README.md                Load mechanism, prerequisite (lumina serve), skill list
├── CONVENTIONS.md           Shared frontmatter + idempotency contract + MCP-tool patterns
└── skills/
    ├── problem-statement/SKILL.md
    ├── research-notes/SKILL.md
    ├── user-interrogation/SKILL.md
    ├── acceptance-criteria/SKILL.md
    ├── approach/SKILL.md
    ├── not-doing/SKILL.md
    ├── edge-cases/SKILL.md
    ├── relevance/SKILL.md
    └── closure-gate/SKILL.md
```

Slash invocations: `/lumina:problem-statement <work_item_id>` etc. The `name: "lumina"` manifest field (not the directory name) derives the namespace per R22.

### Shared conventions (`CONVENTIONS.md`)

Every skill MUST follow these conventions:

1. **Frontmatter shape** (R1, R8):
   ```yaml
   ---
   name: <skill-name>
   description: <one-sentence summary>
   arguments: [work_item_id]
   disable-model-invocation: true
   ---
   ```
   `disable-model-invocation: true` is mandatory — ensures Claude cannot auto-fire a DB-mutating skill mid-conversation; user or UI controls the trigger.

2. **Check-before-act idempotency (R7)**: every skill body opens with:
   1. Call `mcp__lumina__get_work_item` with `{id: "$work_item_id"}`.
   2. Inspect the relevant field / sub-table.
   3. If absent → call the `add_*` / `set_*` MCP tool.
   4. If present and value matches user intent → no-op, return.
   5. If present and value should change → ask user to confirm via `AskUserQuestion`, then call the `update_*` / `supersede_*` MCP tool.

3. **Provenance recording (R8)**: after any write, call `mcp__lumina__record_task_activity` with `entry_kind: "execution"`, `origin: "plan"`, summary tying back to the skill name + `${CLAUDE_SESSION_ID}`.

4. **Forked context (R2)** only for `research-notes` — that skill sets `context: fork` + `agent: general-purpose`. All others run inline.

5. **Sentry pattern (R6)**: skill body = workflow instructions; MCP tools = execution. Never embed business logic that should live in `lumina/src/repo.rs`.

6. **No per-verb fragmentation (R18)**: each skill handles its block end-to-end (check + create + update + supersede). One skill per block, never per verb.

7. **Lens conventions**: where lumina has no first-class column, skill bodies use named lenses on `research_notes` (`lens="edge-case"`) or named keys in `attributes` JSON (`attributes.not_doing`). CONVENTIONS.md is the registry — a future migration can promote any of these to first-class fields with a coordinated skill-body update.

### The 9 skills

| Skill | Lumina target | Storage mechanism | Inline/Fork | Notes |
|---|---|---|---|---|
| `lumina:problem-statement` | `attributes.problem_statement` | `mcp__lumina__set_story_plan` (problem_statement field) | inline | Asks user for 2-4 sentences: what's broken/missing, who's affected, success criteria. |
| `lumina:research-notes` | `research_notes` rows | `mcp__lumina__add_research_note` / `supersede_research_note` | **fork** (R2) | Subagent identifies gaps, performs research (Context7 + WebSearch + code reads), adds 3-7 notes with `lens` + `confidence` + `state: "proposed"`. User accepts later via raw MCP (or future `/lumina:accept-research`). |
| `lumina:user-interrogation` | `open_questions` + `question_options` | `mcp__lumina__add_open_question` + `add_question_option` | inline | HumanLayer 4-axis (R16): scope, error-handling, data-ownership, compatibility. Each unresolved answer becomes one `open_question` with ≥2 options. Does NOT call `resolve_open_question`. |
| `lumina:acceptance-criteria` | `acceptance_criteria` rows on task children | `mcp__lumina__add_acceptance_criterion` | inline | Iterates task children of the story. For each AC-missing task, prompts with 3 structural hints (Q6): concrete I/O example, trigger condition, verification step. Free-text body. |
| `lumina:approach` | `attributes.execution_strategy` | `mcp__lumina__set_story_plan` (execution_strategy field) | inline | Pre-reads problem_statement + accepted research_notes + resolved open_questions; drafts 2-4 paragraph approach; user-confirms. Warns (doesn't block) if prerequisites absent. |
| `lumina:not-doing` | `attributes.not_doing` (new JSON key, lens convention §7) | `mcp__lumina__update_work_item` with `{attributes: {not_doing: "..."}}` | inline | Uses existing `set_work_item_attributes` JSON-merge primitive; no lumina schema change. |
| `lumina:edge-cases` | `research_notes` rows with `lens="edge-case"` | `mcp__lumina__add_research_note` | inline | One note per edge case (gets supersession + confidence lifecycle for free). |
| `lumina:relevance` | `work_items.relevance` column | `mcp__lumina__set_relevance` | inline | Thin wrapper. Reads current; asks user for {active|backlog|deferred|rejected}; writes. Exists for UI parity (Q1). |
| `lumina:closure-gate` | `work_items.closure_gate` column | `mcp__lumina__set_closure_gate` | inline | Thin wrapper. Reads current; asks user for {hard|soft}; writes. Exists for UI parity (Q1). |

### Cross-reference to existing lumina/SKILL.md

`claude/skills/lumina/SKILL.md` remains the lumina data-layer driver skill. We add a `## Story-block skill family` section (after `## When to use this skill`) summarising the 9 skills and pointing at the plugin README. The existing tool catalogue stays as the low-level reference.

### Loading mechanism

Plugin is checked in but NOT auto-discovered (R23 — no `pluginDirs` settings convention). Two supported load paths:
- CLI: `claude --plugin-dir claude/plugins/lumina-story-blocks`
- SDK: `options.plugins: [{type: "local", path: "./claude/plugins/lumina-story-blocks"}]`

Documented in `README.md` and `lumina/CLAUDE.md`.

### Rejected alternatives

- **Flat skill layout** (`claude/skills/lumina-<block>/`): user chose plugin namespacing (Q3); also loses atomic install/uninstall.
- **Nested under existing `claude/skills/lumina/blocks/`**: nested-skill discovery semantics aren't documented in Claude Code; plugin namespacing is the documented primitive (R5/R22).
- **Living-spec model**: rejected by user (Q4); skills stay idempotent but workflow doesn't trigger post-task re-runs.
- **EARS / Gherkin AC enforcement**: rejected by user (Q6).
- **First-class `attributes.not_doing` column via lumina migration**: deferred — skills use existing `attributes` JSON merge so this plan stays skills-only.
- **Per-verb skill split** (e.g. `set-problem-statement` / `supersede-problem-statement`): rejected per R18 fragmentation guidance.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test: cargo test --manifest-path lumina/Cargo.toml
lint: cargo clippy --manifest-path lumina/Cargo.toml --all-targets
shared-blocks: bash scripts/verify-shared-blocks.sh
```

This plan touches only markdown files under `claude/plugins/` and `claude/skills/`; the cargo commands run as **guardrails** (no accidental code-side breakage) rather than direct acceptance gates. Primary acceptance: **manual smoke-test** per Task 10.

## Tasks

### Phase A: Foundation (parallel, 3 agents)

#### 1. Create plugin manifest + skeleton directory layout [S]
- **Files**: `claude/plugins/lumina-story-blocks/.claude-plugin/plugin.json`, `claude/plugins/lumina-story-blocks/skills/.gitkeep`
- **Depends on**: —
- **Action**: Create the plugin root with the manifest and an empty `skills/` directory marker.
- **Detail**: Manifest per R20:
  ```json
  {
    "name": "lumina",
    "version": "0.1.0",
    "description": "Composable story-block skills for lumina work-items",
    "skills": "./skills/"
  }
  ```
  Use `name: "lumina"` (NOT the directory slug) so invocations are `/lumina:<skill>` (R22).
- **Acceptance**: `claude/plugins/lumina-story-blocks/.claude-plugin/plugin.json` parses as JSON; required fields (`name`, `version`, `description`) present; `claude --plugin-dir claude/plugins/lumina-story-blocks --help` runs without manifest errors.

#### 2. Write CONVENTIONS.md shared contract [M]
- **Files**: `claude/plugins/lumina-story-blocks/CONVENTIONS.md`
- **Depends on**: —
- **Action**: Author the shared conventions doc every skill body references.
- **Detail**: Sections: (a) Frontmatter shape with the 4 mandatory keys (`name`, `description`, `arguments: [work_item_id]`, `disable-model-invocation: true`); (b) Check-before-act 5-step idempotency sequence; (c) Provenance via `record_task_activity` template; (d) Forked-context exception; (e) Sentry pattern (skill = instructions, MCP = execution); (f) No-per-verb-fragmentation rule; (g) Lens conventions registry (`lens="edge-case"`, `attributes.not_doing`, future entries). Each section: 2-3 sentences + 1 code/prose example.
- **Acceptance**: Doc has all 7 sections; each section has at least one concrete example; the AskUserQuestion phrasing for supersession confirmation is verbatim (so all skills reference the same wording).

#### 3. Write plugin README [S]
- **Files**: `claude/plugins/lumina-story-blocks/README.md`
- **Depends on**: —
- **Action**: Author the user-facing README.
- **Detail**: Sections: (a) What the plugin does — one paragraph; (b) Load mechanism — CLI `--plugin-dir` example + SDK `plugins: [{type:"local"}]` example per R23; (c) Skill list — table of 9 skills with one-line summaries + slash invocations; (d) Idempotency contract — 1-line pointer to CONVENTIONS.md; (e) Prerequisites — `lumina serve` running and registered as MCP server named `lumina`; (f) Verification — `/mcp` lists `lumina` as active.
- **Acceptance**: A new agent can install and invoke `/lumina:problem-statement <id>` from this README alone, without reading other files.

### Phase B: Skills (parallel, 4 agents, max 3 files per batch)

Each task writes SKILL.md files only; CONVENTIONS.md from Task 2 is read-only input. Skills are independent.

#### 4. Inline thin-wrapper skills [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/relevance/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/closure-gate/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/not-doing/SKILL.md`
- **Depends on**: 1, 2
- **Action**: Author the three thinnest skills (~60-80 lines each).
- **Detail**: Frontmatter per CONVENTIONS.md §a. Body: check-then-act 5-step using the named MCP tool. `relevance` calls `set_relevance` with {active|backlog|deferred|rejected}; `closure-gate` calls `set_closure_gate` with {hard|soft}; `not-doing` calls `update_work_item` with `{attributes: {not_doing: <user text>}}`. Each ends with `record_task_activity`. `not-doing` SKILL.md explicitly cites the `attributes.not_doing` lens convention from CONVENTIONS.md §g.
- **Acceptance**: Each SKILL.md has valid YAML frontmatter; references CONVENTIONS.md by relative path; names the exact MCP tool and argument JSON shape; demonstrates the 5-step check-before-act sequence; ends with the `record_task_activity` template.

#### 5. Inline narrative-content skills [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/problem-statement/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/approach/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/edge-cases/SKILL.md`
- **Depends on**: 1, 2
- **Action**: Author the three narrative skills (~100-150 lines each).
- **Detail**:
  - `problem-statement`: asks user the 3-axis prompt (broken/affected/success); calls `set_story_plan` with the `problem_statement` field.
  - `approach`: pre-reads `attributes.problem_statement`, `research_notes` with `state="accepted"`, `open_questions` with `status="answered"`; drafts 2-4 paragraph approach via reasoning; user confirms via AskUserQuestion; calls `set_story_plan` with `execution_strategy`. Warns (not blocks) if prerequisites absent.
  - `edge-cases`: enumerates edge cases as `research_notes` rows with `lens="edge-case"`; one note per case; `confidence` + `state="proposed"`.
- **Acceptance**: Each SKILL.md cites the exact MCP tool + argument JSON; contains the 5-step idempotency check; demonstrates the prompt structure the user will see; `edge-cases` SKILL.md references the lens convention from CONVENTIONS.md §g.

#### 6. Interrogation + AC skills [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/user-interrogation/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/acceptance-criteria/SKILL.md`
- **Depends on**: 1, 2
- **Action**: Author the two structured-questioning skills.
- **Detail**:
  - `user-interrogation` (~150 lines): iterates HumanLayer 4 axes (R16) — scope, error-handling, data-ownership, compatibility. For each axis, reads existing `open_questions` to avoid re-asking; asks via `AskUserQuestion` (≤4 axes per call); writes unresolved into `open_questions` with ≥2 `question_options` each. Explicitly does NOT call `resolve_open_question`. Includes the "is there a 5th axis I'm missing?" fallback question.
  - `acceptance-criteria` (~120 lines): iterates task children of the story (via `mcp__lumina__get_work_item` + filtering `children` by `kind=task`); for each task missing AC, prompts user with 3 structural hints (Q6) — concrete I/O example, trigger condition, verification step; writes via `add_acceptance_criterion`. Free-text body, no syntax enforcement.
- **Acceptance**: Each SKILL.md includes the exact `AskUserQuestion` phrasing, the MCP write pattern, and the 5-step idempotency check; `user-interrogation` enumerates all 4 axes + the fallback; `acceptance-criteria` describes the task-iteration loop explicitly.

#### 7. Forked-context research-notes skill [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/research-notes/SKILL.md`
- **Depends on**: 1, 2
- **Action**: Author the only forked-context skill.
- **Detail**: ~150 lines. Frontmatter adds `context: fork` + `agent: general-purpose` (R2). Subagent body: (1) reads existing `research_notes` via `mcp__lumina__get_work_item`; (2) identifies research gaps (problem_statement is required prerequisite — warn if missing); (3) performs research via Context7 + WebSearch + targeted code reads; (4) writes 3-7 notes via `add_research_note` with `lens` + `confidence` + `state: "proposed"`. Notes default to `state="proposed"` so a separate accept step (manual via MCP) is required before they're considered authoritative.
- **Acceptance**: SKILL.md has `context: fork` + `agent: general-purpose` in frontmatter; subagent prompt is structured (gap-identify → research → write); writes default to `state: "proposed"`; supersession path uses `supersede_research_note` not delete-and-re-add.

### Phase C: Integration (sequential, after Phase B)

#### 8. Cross-reference lumina/SKILL.md [S]
- **Files**: `claude/skills/lumina/SKILL.md`
- **Depends on**: 4, 5, 6, 7
- **Action**: Add a `## Story-block skill family` section after `## When to use this skill`.
- **Detail**: One paragraph intro pointing at the new plugin, table of 9 skills (name, slash invocation, one-line summary), load-mechanism pointer to plugin README. Do NOT remove or rewrite the existing tool catalogue.
- **Acceptance**: Section reads coherently in-flow; no duplicate information against the existing catalogue; each new skill is named with its `/lumina:<name>` slash invocation; the load-mechanism subsection points at the plugin README.

#### 9. Document plugin load in lumina/CLAUDE.md [S]
- **Files**: `lumina/CLAUDE.md`
- **Depends on**: 3, 8
- **Action**: Add a short paragraph noting plugin location, load mechanism, and pointer to the plugin README.
- **Detail**: ≤8 lines. Slots into the existing CLAUDE.md after the MCP-tool-surface explanation. States: (a) plugin lives at `claude/plugins/lumina-story-blocks/`; (b) load via `claude --plugin-dir claude/plugins/lumina-story-blocks`; (c) full usage in `claude/plugins/lumina-story-blocks/README.md`.
- **Acceptance**: Paragraph is ≤8 lines; correctly identifies load path; points to README.

### Phase D: Verification (manual)

#### 10. Smoke-test the plugin against a real story [M]
- **Files**: (no edits — manual verification)
- **Depends on**: 1-9
- **Action**: Validate plugin loads and each skill behaves per spec.
- **Detail**:
  1. Start `lumina serve` locally; create a test story (any project → epic → feature → story).
  2. Launch Claude Code with `claude --plugin-dir claude/plugins/lumina-story-blocks`.
  3. Invoke each skill in turn against the test story's `id`:
     - `/lumina:problem-statement <id>` → `attributes.problem_statement` set
     - `/lumina:research-notes <id>` → 3-7 `research_notes` rows added with `state="proposed"`
     - `/lumina:user-interrogation <id>` → `open_questions` rows added with ≥2 options each
     - `/lumina:acceptance-criteria <id>` → (first create a task child) `acceptance_criteria` rows added
     - `/lumina:approach <id>` → `attributes.execution_strategy` set
     - `/lumina:not-doing <id>` → `attributes.not_doing` set
     - `/lumina:edge-cases <id>` → `research_notes` with `lens="edge-case"` added
     - `/lumina:relevance <id>` → `relevance` column updated
     - `/lumina:closure-gate <id>` → `closure_gate` column updated
  4. Re-invoke each skill on the same story; verify idempotency (supersession-confirm prompts for set fields; no duplicate writes).
  5. Run guardrails: `cargo build --manifest-path lumina/Cargo.toml`, `cargo test --manifest-path lumina/Cargo.toml`, `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`, `bash scripts/verify-shared-blocks.sh` — each exits 0.
- **Acceptance**: Every skill produces the expected DB write on first invocation; every skill prompts for supersession confirmation on second invocation; all guardrail commands exit 0.

## Dependency Graph

- **Batch 1 (parallel, 3 agents)**: Tasks 1, 2, 3 — foundation; each independent.
- **Batch 2 (parallel, 4 agents)**: Tasks 4, 5, 6, 7 — each writes only its own SKILL.md(s); CONVENTIONS.md (Task 2) is read-only input.
- **Batch 3 (sequential)**: Task 8 — integrates cross-reference into existing lumina/SKILL.md.
- **Batch 4 (sequential)**: Task 9 — documents load mechanism (needs README + cross-reference to exist).
- **Batch 5 (manual)**: Task 10 — end-to-end smoke test.

4 parallel batches + 1 manual. Within the "3-4 parallel agents max per dependency level" rule.

## Verification

- **Build guardrails**: `cargo build --manifest-path lumina/Cargo.toml` exits 0.
- **Test guardrails**: `cargo test --manifest-path lumina/Cargo.toml` exits 0.
- **Lint guardrails**: `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` — no new warnings.
- **Shared-blocks parity**: `bash scripts/verify-shared-blocks.sh` exits 0.
- **Plugin load**: `claude --plugin-dir claude/plugins/lumina-story-blocks` lists all 9 skills under the `lumina:` namespace (verified via `/help` or `/mcp`).
- **Manual smoke test**: Task 10 — invoke each skill against a test story, verify all 9 expected DB writes + 9 idempotency confirmations.
- **YAML frontmatter validity**: each SKILL.md parses (implicitly validated by plugin load).

## Risks

- **Risk**: Plugin manifest schema may differ between Claude Code versions; the documented schema in Phase 5 may not match the locally-installed version. — **Mitigation**: Task 10 step 2 (CLI load) catches this immediately; if load fails, `claude --version` + matching docs page tells us what changed.
- **Risk**: `mcp__lumina__*` tool names assume the lumina MCP server is registered as `lumina` in Claude Code config; under a different prefix, skill bodies break silently. — **Mitigation**: README prerequisite section states the expected MCP server name + verification command (`/mcp` lists active servers); CONVENTIONS.md notes the assumption.
- **Risk**: `attributes.not_doing` and `lens="edge-case"` are unilateral lens conventions established by this plan. If lumina later adds first-class columns, skill bodies need migration. — **Mitigation**: CONVENTIONS.md §g "Lens conventions" subsection records the registry explicitly; follow-up plans promote these to first-class fields with a coordinated skill-body update.
- **Risk**: HumanLayer's 4-axis interrogation taxonomy may not fit every story (e.g. pure-UI story has no data-ownership axis). — **Mitigation**: `user-interrogation` SKILL.md body instructs the agent to skip non-applicable axes, and the fallback "is there a 5th axis I'm missing?" question lets the user extend the taxonomy per-story.
- **Risk**: `disable-model-invocation: true` means Claude never auto-suggests these skills mid-conversation; users who don't know the slash commands won't discover them. — **Mitigation**: Plugin README + cross-reference in lumina/SKILL.md make discovery explicit; the follow-up UI plan makes this trivial via buttons.
- **Risk**: Static (write-once) decision (Q4) means re-running a skill on an existing-with-value field requires user-confirmation UX in the skill body; if poorly worded the user accepts a no-op when they meant supersede, or vice versa. — **Mitigation**: CONVENTIONS.md provides the verbatim `AskUserQuestion` phrasing for supersession confirmation; all skills reference the same wording. Task 2 acceptance criteria includes this phrasing.

## Phase 9 note (re: filename)

The plan-mode harness assigned the filename `soft-spinning-cake.md`. Before `tomlctl flow init`, the user can choose to (a) keep the assigned slug (`soft-spinning-cake` matches the required regex `^[a-z0-9][a-z0-9-]{0,63}$`), or (b) rename to `lumina-story-planning-workflow.md` for a descriptive slug. The Phase 9 bootstrap will surface this choice via `AskUserQuestion` before invoking `tomlctl flow init`.
