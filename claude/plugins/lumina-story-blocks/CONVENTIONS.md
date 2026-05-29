# Conventions — `lumina-story-blocks` plugin

This document is the shared contract for every skill in the `lumina-story-blocks` plugin (manifest `name: "lumina"`, slash invocations `/lumina:<block>`). Every skill in this plugin MUST follow these conventions; the skill bodies cite the sections below by short reference (`§a`, `§b`, …) rather than re-stating the rules inline. If a rule here changes, the skill bodies are the consumers and must be re-read in lockstep.

The plugin's purpose is to drive lumina's existing MCP tool surface (catalogued in [`./skills/mcp/SKILL.md`](./skills/mcp/SKILL.md)) to fill the per-story planning blocks one block at a time, idempotently, with user-mediated supersession on re-run.

## §a Frontmatter shape

Every DB-mutating skill in this plugin declares AT MINIMUM these four keys in its YAML frontmatter, in this order (or six keys if the skill runs in forked context — see §d), and SHOULD additionally declare `argument-hint` whenever `arguments` is non-empty. Read-only documentation skills (currently only the `mcp` catalogue) omit `disable-model-invocation: true` per the exception below; they MAY also omit `arguments` (and correspondingly `argument-hint`) if they take none.

> **Forward reference**: the four-key shape below is the minimum. Exactly one skill in this plugin (`research-notes`) runs in a forked subagent context and declares two ADDITIONAL keys (`context: fork`, `agent: general-purpose`) for a total of six — see §d for the rationale and the canonical six-key example.

```yaml
---
name: <skill-name>
description: <one-sentence summary>
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---
```

- `name` is the skill's invocation suffix; combined with the plugin manifest's `name: "lumina"`, the full slash form is `/lumina:<name>`.
- `description` is a single sentence. It is what model-auto-invocation matches against — but see `disable-model-invocation` below: in this plugin the description is informational only (it does NOT enter the routing context).
- `arguments: [work_item_id]` declares one named positional argument, substituted as `$work_item_id` in the body. Per R8, named arguments give the body a stable substitution rather than relying on `$ARGUMENTS` blob parsing.
- `argument-hint: "[work_item_id]"` is the slash-autocomplete hint surfaced to the user when they type `/lumina:<name>` — without it the autocomplete shows nothing and the user has to guess that a work-item id is expected. It is RECOMMENDED whenever `arguments` is non-empty (and omitted alongside `arguments` for read-only catalogues that take none). The hint mirrors the `arguments` shape verbatim.
- `disable-model-invocation: true` is MANDATORY for every DB-mutating skill in this plugin. Per R1, Claude Code exposes three invocation paths — `/name` slash, model-auto-trigger via `description`, and explicit `Skill` tool dispatch — and `disable-model-invocation: true` removes the description from the routing context so ONLY explicit triggers (the user typing `/lumina:<name>`, or eventually a UI button dispatching the skill) fire it. This is non-negotiable: these skills write to the lumina database and would be unsafe to auto-fire mid-conversation off a description match.

**Exception — read-only / documentation skills.** The `mcp` skill in this plugin is a read-only documentation surface (it documents the lumina MCP tool catalogue and does not call any write tool itself). Such skills MAY omit `disable-model-invocation: true` to remain model-discoverable — letting agents auto-find the catalogue when they need to drive the lumina MCP surface. The rule above applies only to skills that write to the lumina database via `mcp__lumina__*` write tools.

Example (the `problem-statement` skill):

```yaml
---
name: problem-statement
description: Capture or update a story's problem_statement (what's broken, who's affected, success criteria).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---
```

## §b Check-before-act idempotency

No first-party idempotency primitive exists for skills (R7). The pattern is Check-Before-Act: every skill body opens with a read, branches on what it finds, and writes only the diff. Skills follow this EXACT 5-step sequence:

1. Call `mcp__lumina__get_work_item` with `{id: "$work_item_id"}`.
2. Inspect the relevant field / sub-table for this skill's block (e.g. `attributes.problem_statement` for `problem-statement`; the `research_notes` array for `research-notes`; `acceptance_criteria` rows on each task child for `acceptance-criteria`).
3. If absent → call the `add_*` / `set_*` MCP tool to create the value.
4. If present and the value matches user intent → no-op; return early with a one-line confirmation to the user (e.g. "problem_statement already set to: <truncated> — no change.").
5. If present and the value should change → ask the user to confirm via `AskUserQuestion` using the verbatim supersession phrasing in **§b-supersession** below. On `Replace`, call the `update_*` / `supersede_*` MCP tool; on `Keep current`, abort the skill invocation without writing.

Pseudocode showing the dispatch fork (for the `problem-statement` skill, deciding between first-write and supersession-confirm):

```
detail = mcp__lumina__get_work_item({id: $work_item_id})
existing = detail.attributes.problem_statement   # may be null/absent

if existing is absent:
    new_value = <ask the user for the 3-axis prompt>
    mcp__lumina__set_story_plan({id: $work_item_id, problem_statement: new_value})
    mcp__lumina__record_task_activity({...})     # per §c
    return "problem_statement created."

new_value = <ask the user for the 3-axis prompt>

if new_value == existing:
    return "problem_statement already matches the value you provided — no change."

# Existing value, but the user-supplied new value differs.
answer = AskUserQuestion(<verbatim phrasing from §b-supersession>)
if answer == "Keep current":
    return "Aborted; existing problem_statement left in place."

mcp__lumina__set_story_plan({id: $work_item_id, problem_statement: new_value})
mcp__lumina__record_task_activity({...})         # per §c
return "problem_statement superseded."
```

### §b-supersession — verbatim `AskUserQuestion` phrasing

Every skill in this plugin invokes step 5 with the SAME wording so the user sees a consistent UX across the plugin. Copy this template verbatim:

> **Question header**: `Supersede?`
>
> **Question body**: `This story's <field-name> is already set to: <current-value-summary>. Replace it with the new value you provided? Choosing 'Keep current' aborts this skill invocation without writing.`
>
> **Options** (exactly 2):
> - `Replace` — `Write the new value, superseding the existing one`
> - `Keep current` — `Abort this invocation, leave the existing value in place`

**Substitution rules**:
- `<field-name>` — the skill-specific block name (e.g. `problem_statement`, `closure_gate`, `acceptance criterion`).
- `<current-value-summary>` — the existing value's first ~80 characters as a single line. Embedded newlines are collapsed to spaces BEFORE truncating; truncated output ends with `…` (ellipsis).

For skills whose target is a row-shaped sub-table (e.g. `research_notes`, `open_questions`, `acceptance_criteria`), substitute the field name with the row's identifying summary (e.g. `<field-name>` → `research note "auth-flow gap"`), and the supersession write is `supersede_research_note` rather than an in-place update.

### §b-supersession-destructive (hard-delete variant)

Some `lumina` tools have no `update_*` or `supersede_*` companion and require hard-delete + insert. The current example is `remove_acceptance_criterion`. For destructive supersession, the skill MUST present a second `AskUserQuestion` after the §b-supersession prompt to confirm the irreversible delete. Use exactly:

> **Question header**: `Hard-delete the existing row?`
>
> **Question body**: `Replacing this <field-name> requires hard-deleting the existing row (no supersede tool exists for this field). The new value is: <new-value-summary>. Confirm hard-delete?`
>
> **Options**:
> - `Hard-delete and replace` — irreversibly remove `<old_id>` and add `<new_id>`. The activity log retains the supersession entry but the old row is gone.
> - `Cancel` — abort the supersession; both the old and new values are discarded.

### §b-noop (canonical no-op confirmation string)

When a §b step 4 fires (present-matches → no-op), the skill MUST return exactly this one-line confirmation (with `<field-name>` replaced):

> `<field-name> already matches the value you provided — no change.`

Examples per skill:

- `problem-statement`: `problem_statement already matches the value you provided — no change.`
- `closure-gate`: `closure_gate already matches the value you provided — no change.`
- `not-doing`: `not_doing already matches the value you provided — no change.`

Skill bodies that currently use a slightly different phrasing (e.g. `closure_gate already set to <value> — no change.`) are not yet aligned to this template; aligning them is a separate refactor task.

### §b-per-element scope

Four skills in this plugin — `research-notes`, `user-interrogation`, `acceptance-criteria`, and `epic-close-criteria` — iterate a collection rather than operating on a single scalar field. For these skills, §b applies per-element (per research note, per open-question axis, per task-child's acceptance criteria row, per epic close-criterion) rather than per skill invocation. The verbatim §b-supersession phrasing and the step-4/step-5 branch structure are unchanged within each element iteration; the skill simply runs the 5-step sequence once per element. Each iterating skill cites this subsection to document that its per-element scope is intentional and convention-compliant.

## §c Provenance recording

After ANY successful write, the skill MUST append exactly one activity entry to the work item via `mcp__lumina__record_task_activity`. This is how the planning lifecycle is auditable in the lumina activity log. Use this template verbatim (substituting `<skill-name>` and `<what was done>`):

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "<skill-name>: <what was done>",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

Notes:

- `entry_type` is `"execution"` for all writes from this plugin. Do NOT use `"verification"` — the lumina SKILL.md catalogue notes that `verification` activity entries are appended INTERNALLY by `check_acceptance_criterion` and the `record_task_activity` enum explicitly rejects `verification`. The `comment` value is legal but reserved for human commentary and MUST NOT be used by skills in this plugin.
- **Exception (vet)**: the `vet-research` skill MAY write `entry_type: "vet"` to record vet-pass outcomes against a story's research notes. No other skill in this plugin may write `vet`; round-2 narrows the channel to this one explicitly-named exception. Every other plugin write stays `execution`.
- **Note on entry_type / origin orthogonality**: `entry_type` (`execution` / `vet` / `comment`) is the activity-stream channel; `origin` (`plan` / `implement` / `review` / `optimise` / `tdd` / `human` / `none`) is the agent or lifecycle stamp. They are orthogonal — `entry_type: "execution"` here pairs with `origin: "plan"`. If `lumina` migration N+ adds a `planning` entry_type, this convention should re-map plan-time writes to `entry_type: "planning"` to distinguish them from `/implement`-driven activity. Until then, `execution` is the chosen channel for plan-time writes and the agent SHOULD read the `origin` stamp to tell which lifecycle phase produced any given row.
- `origin` is `"plan"` because these skills run inside the planning workflow (R8 origin taxonomy: `plan` / `implement` / `review` / `optimise` / `tdd` / `human` / `none`).
- `${CLAUDE_SESSION_ID}` is a Claude Code substitution variable — Claude Code expands it to the session uuid at execution time. The skill body writes the literal placeholder string; the substitution happens above the MCP layer. Threading it through the activity body lets a later audit join planning activity to the originating Claude session.
- **Substitution guard**: before calling `record_task_activity`, the skill MUST verify `${CLAUDE_SESSION_ID}` resolved to a non-empty value that does not contain the literal substring `CLAUDE_SESSION_ID`. If the substitution did not fire (older harness, non-session invocation, future regression), the literal string `session=${CLAUDE_SESSION_ID}` would land in lumina verbatim and silently break the audit trail. On detected non-substitution, record `body: "session=unknown"` instead AND emit a one-line warning to the user (e.g. `"warning: CLAUDE_SESSION_ID did not substitute; recorded as 'unknown'"`). This makes the failure visible rather than silent.
- One activity entry per write — not per skill invocation. A skill that writes twice (e.g. supersedes one note AND adds a new one in the same invocation) records two activity entries.

## §d Forked context (research-notes + story-review + decompose-tasks)

Round-1 had exactly one forked skill (`research-notes`); round-2 added two (`story-review` and `decompose-tasks`). All three skills' frontmatters add two extra keys (per R2) beyond the mandatory ones from §a — `context: fork` and `agent: general-purpose`. The §a `argument-hint` recommendation still applies whenever `arguments` is non-empty, so the canonical forked frontmatter is seven keys (the §a four plus `argument-hint` plus the two fork keys):

```yaml
---
name: research-notes
description: Identify research gaps and add proposed research notes to a story.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
context: fork
agent: general-purpose
---
```

The §a → §d delta is exactly two keys (`context`, `agent`) — `argument-hint` belongs to the §a shape and rides through unchanged.

**Why fork these three**: all three skills are multi-step explorations whose intermediate tool output would saturate the main planning context. `research-notes` runs Context7 lookups, WebSearch queries, targeted code reads, and draft synthesis; `story-review` runs a 7-category rubric over the full story detail, performing cross-block semantic comparisons and per-rubric finding writes; `decompose-tasks` reads every accepted-state planning block, optionally fans out parallel sub-decompose-agents per foundation-disjoint module (R26), runs pattern-replacement file enumeration via Grep (R25), and synthesises a proposed task list. Each operation leaves tool-output noise in the conversation context that the main planning session does not need. Running these skills in a forked subagent isolates that noise; the parent conversation sees only the final summary (and the lumina rows themselves are queryable via `get_work_item`).

**Why every other skill stays inline**: all other skills in this plugin are short interactive Q&A loops — the user types a few sentences, the skill writes one or two MCP calls, done. Inline execution keeps the user in the parent context where they can interject, ask follow-ups, or chain skills. Forking these would force a context switch with no compensating benefit.

A forked-context skill MAY append reporting/summary steps after the 5-step §b sequence (e.g. research-notes' final summary step). The 5-step §b sequence itself MUST appear in order; additions go after step 5, not interleaved.

## §e Sentry pattern — skill = instructions, MCP = execution

Per R6, Anthropic's published Sentry exemplar pairs a SKILL (workflow instructions) with an MCP server (execution). This plugin follows the same split:

- **Skill body** contains the workflow: the prompts to show the user, the order in which to ask them, the decision branches (absent/present/match/mismatch), the supersession-confirm interaction, the cite to `record_task_activity`. It is prose + pseudocode telling the agent WHAT TO DO.
- **MCP tools** (`mcp__lumina__*`) contain the business logic: validation, transactions, FK integrity, the `set_story_plan` merge semantics, the `closure_gate=hard` task-blocking rule, the partial UNIQUE on `repo_links.is_primary`, the `record_task_activity` enum constraint. The skill body MUST NOT duplicate or shadow this logic.

Worked counter-example (DO NOT do this):

> ✗ Skill body reads `attributes.problem_statement` and `attributes.research_notes` via `get_work_item`, builds the merged JSON `{problem_statement: <new>, research_notes: <unchanged>}` itself, then calls `update_work_item` with the whole merged blob.

Worked correct example:

> ✓ Skill body calls `mcp__lumina__set_story_plan({id, problem_statement: <new>})`. Lumina's `repo.rs` reads the existing attributes, merges the present key, leaves absent keys untouched, runs the write in one transaction, and emits the event.

The skill's job is to know which MCP tool to call and what to put in the arguments. Lumina's job is to make that call safe.

Exception: a skill MAY locally verify `detail.kind` matches its declared target kind for a friendlier early-abort message. This duplicates a lumina-server check, but the UX win is judged to outweigh the duplication.

## §f No per-verb fragmentation

Per R18, skill fragmentation has bidirectional failure modes: over-splitting (verb-per-skill) silently re-merges intent into a "new giant prompt" the user has to assemble; under-splitting (one skill does everything) loses the per-block check-then-act discipline. This plugin's rule:

**Each skill handles ONE BLOCK end-to-end — check + create + update + supersede.**

We DO NOT split skills by verb. There is no `set-problem-statement` / `supersede-problem-statement` pair; there is one `problem-statement` skill that handles both verbs via the §b 5-step sequence. Concretely:

| Verb | Where it lives |
|---|---|
| Create (absent → write) | Inside the skill body at §b step 3 |
| No-op (present and matches) | Inside the skill body at §b step 4 |
| Supersede (present and differs, user confirms) | Inside the skill body at §b step 5 |
| Abort (present and differs, user declines) | Inside the skill body at §b step 5 |

This keeps the user's mental model 1:1 with the story schema: one block, one slash command, one decision-point. The verb-fork is an implementation detail of the skill body.

## §g Storage-convention registry

Some story blocks have no first-class column on `work_items` (or on a dedicated sub-table) and instead ride on existing storage via a named-key or named-lens convention. This section is the SINGLE SOURCE OF TRUTH for those bindings — the skill bodies cite this registry by reference rather than re-stating where the data lives.

**Rule of thumb — kind-precondition vs lens-based skills**: lens-based skills may run on any work-item kind; story-shaped UX skills (problem-statement, approach, user-interrogation) fail-fast on non-story. If the skill writes to a §g.2 lens convention, accept any kind; if it writes to a story-only column or `set_story_plan` field, fail-fast. (See §e for the exception that blesses the local kind-precondition check, and §h for the architectural signpost.)

Two architecturally distinct primitive types live here, each with its own promotion path:

- **§g.1 — Attribute-key conventions**: a key NAME inside the `work_items.attributes` JSON blob. Promotion path: `ALTER TABLE ADD COLUMN` (or a widened `set_*` MCP tool that merges the key safely).
- **§g.2 — Column-value lens conventions**: a string VALUE in a typed column (e.g. `research_notes.lens`). Promotion path: new first-class table + data migration to replace the lens-value enum.

Do not conflate the two: an attribute-key convention is data hiding inside a JSON blob (no schema enforcement); a column-value lens convention is a typed-column value (schema-typed as text, but conventionally constrained by the registry below).

## §g.1 Attribute-key conventions (JSON-merge keys in `work_items.attributes`)

| Convention | Where stored | Storage primitive | Used by |
|---|---|---|---|
| `attributes.not_doing` | `work_items.attributes` JSON key | `mcp__lumina__set_story_plan` (round-2 widened-params form). The repo-side `set_work_item_attributes` merges via Rust-side patch + per-kind validator, so sibling keys are preserved. | `lumina:not-doing` skill |
| `attributes.verification_commands` | `work_items.attributes` JSON key (object: `{build, test, lint, smoke}`, each `Option<String>`) | `mcp__lumina__set_story_plan` (round-2 widened-params form). | `lumina:verification-commands` skill |

Notes:

- Round-2 reactivated `attributes.not_doing`. The earlier disabled-status referenced a column-level COALESCE bug in `update_work_item.attributes` — round-2 widened `SetStoryPlanParams` to accept `not_doing` and routes through the merge-safe `set_story_plan` path. The not-doing skill writes via this entry point only; `update_work_item` with raw `attributes` payloads remains forbidden.
- **Lens-key drift warning**: consumers of `attributes.not_doing` and `attributes.verification_commands` (skill bodies, future UI code, exporter, smoke tests) MUST reference the literal snake_case key strings. Lumina has no schema-level protection against typos — writing `attributes.notDoing` or `attributes.not-doing` succeeds silently and produces drift. The same applies for `verification_commands` (NOT `verificationCommands`, NOT `verification-commands`). Consider adding a lumina-side test that scans `work_items.attributes` for unknown top-level keys as a drift smoke check.
- **`verification_commands` shape**: the object's keys (`build`, `test`, `lint`, `smoke`) and value type (`Option<String>`) are validated server-side via the round-2 `VerificationCommands` struct. Unknown keys inside the object are rejected at the MCP layer.

**Promotion policy for §g.1**: when lumina later adds a first-class column for one of these (e.g. a `not_doing` column on `work_items`), the corresponding skill body in this plugin is updated in lockstep with the schema migration via `ALTER TABLE ADD COLUMN` plus the per-key code paths. The slash command name and user-facing prompt stay the same; the key-binding row moves out of this registry; and the row in the table above is deleted with a one-line note in the migration PR. New attribute-key conventions are added by appending a row here AND updating the consuming skill body in the same change.

## §g.2 Column-value lens conventions (typed-column values, e.g. `research_notes.lens`)

These are string values written into a typed column, NOT JSON keys. Promotion path differs from §g.1: a new first-class table (and data migration) replaces the lens-value enum, rather than `ALTER TABLE ADD COLUMN`.

| Convention | Where stored | Storage primitive | Used by |
|---|---|---|---|
| `lens="edge-case"` on `research_notes` | `research_notes.lens` column | `mcp__lumina__add_research_note` with `{lens: "edge-case", ...}`; supersede via `mcp__lumina__supersede_research_note` | `lumina:edge-cases` skill |
| `lens="prior-art"` on `research_notes` | `research_notes.lens` column | `mcp__lumina__add_research_note` with `{lens: "prior-art", ...}`; supersede via `mcp__lumina__supersede_research_note` | `lumina:research-notes` skill |
| `lens="tool-eval"` on `research_notes` | `research_notes.lens` column | `mcp__lumina__add_research_note` with `{lens: "tool-eval", ...}`; supersede via `mcp__lumina__supersede_research_note` | `lumina:research-notes` skill |
| `lens="codebase-recon"` on `research_notes` | `research_notes.lens` column | `mcp__lumina__add_research_note` with `{lens: "codebase-recon", ...}`; supersede via `mcp__lumina__supersede_research_note` | `lumina:research-notes` skill |
| `lens="constraint"` on `research_notes` | `research_notes.lens` column | `mcp__lumina__add_research_note` with `{lens: "constraint", ...}`; supersede via `mcp__lumina__supersede_research_note` | `lumina:research-notes` skill |
| `lens="failure-mode"` on `research_notes` | `research_notes.lens` column | `mcp__lumina__add_research_note` with `{lens: "failure-mode", ...}`; supersede via `mcp__lumina__supersede_research_note` | `lumina:research-notes` skill |

Notes:

- The `lens="edge-case"` convention rides the existing `research_notes.lens` column added in migration 0003. The `add_research_note` tool already accepts `lens` as a free-form string, so no schema change is needed. `lens="edge-case"` remains reserved for `/lumina:edge-cases`; the five `lens` values consumed by `/lumina:research-notes` (`prior-art`, `tool-eval`, `codebase-recon`, `constraint`, `failure-mode`) are registered as their own rows above.

**Promotion policy for §g.2**: when lumina later adds a dedicated table for one of these lens values (e.g. a dedicated `edge_cases` table with its own MCP tool surface), the corresponding skill body is updated in lockstep via a new typed table + data migration that copies existing `research_notes` rows of that lens into the new table. The slash command name and user-facing prompt stay the same; the lens-binding row moves out of this registry; and the row in the table above is deleted with a one-line note in the migration PR. New lens conventions are added by appending a row here AND updating the consuming skill body in the same change.

## §h Kind-precondition signpost

Skills in this plugin fall into two groups based on which MCP tool they write to:

- **Any-kind skills** (lens-convention writers): skills that write to a §g.2 lens convention (`research-notes`, `edge-cases`, and eventually `not-doing` once reactivated) accept any work-item kind and MUST NOT impose a kind-precondition check.
- **Story-only skills** (story-column writers): skills that write to a story-only column or call `set_story_plan` (`problem-statement`, `approach`, `user-interrogation`, `closure-gate`, `acceptance-criteria`, `relevance`) MUST fail-fast at step 2 with a kind-precondition check per §e's exception.

The rule-of-thumb phrasing lives in §g (lens vs story-only split); §e contains the permission for local `detail.kind` checks. This section is the architectural signpost — cross-reference from §e and §g rather than repeating the rule here.

## §i Story-review pattern (round-2)

The `/lumina:story-review` skill is the plugin's first critique surface — it reads the full story (problem statement, approach, accepted research notes, open questions, edge cases, tasks + AC) and writes critique findings via `mcp__lumina__add_finding` against the existing `findings` table (migration 0001).

- **Finding kind**: every finding written by `/lumina:story-review` MUST carry `kind: "story-review"`. This is the round-2 reservation for the kind discriminator and lets read-side consumers (UI filter, future `list_findings_by_kind` repo method) disambiguate from `kind: "code-review"` used by `/review` and `kind: "performance"` used by `/optimise`.
- **Severity**: `severity ∈ {critical, major, minor, suggestion}` — the typed `Severity` enum (see §k.2 for the deliberate vocab split with `RiskSeverity::{Low, Medium, High, Critical}` on the `risks` table — the two share only the literal `Critical` and otherwise have disjoint vocabularies; do NOT conflate them). Pick severity by the rubric category — direct factual contradiction across blocks is `critical`; tonal/scope drift across blocks is `major`; ungrounded approach claim is `major`; missing AC coverage is `major`; uncovered edge case is `minor`–`major` depending on impact. The story-review SKILL body has the authoritative per-category mapping in its §3 severity-taxonomy note.
- **Supersession**: if a prior `/lumina:story-review` run left findings that are still relevant but materially restated by the new run, use `mcp__lumina__supersede_finding {old_id, new_*}` to chain — never bare add a duplicate. If the prior finding is no longer applicable, `mcp__lumina__update_finding {status: "resolved"}` closes it without supersession. Otherwise leave the old finding as a historical trail.
- **Provenance**: every finding carries `origin: "plan"` (mirroring §c) and is written from within the forked context (`context: fork` per §d's pattern). The `confidence` field is optional but recommended for findings derived from heuristic checks (word-overlap, LLM judgement) — distinguishes them from finds that surface a structural contradiction.

The skill body cites this section by reference; do NOT inline the rubric here (it lives in the skill's SKILL.md). This section is the contract for HOW critique persists, not WHAT critique runs.

## §j Batch-scheduled task execution (round-2)

Task dependencies in lumina are first-class edges (migration 0005's `task_dependencies` table), not implicit phase ordering. The `/lumina:wire-task-deps` skill writes those edges; `/implement` (and any future sprint composer) consumes the dependency DAG via `mcp__lumina__compute_task_batches`, which returns Kahn's-algorithm phase batches: a `Vec<Vec<task_id>>` where each inner vec is a parallel-safe batch and earlier batches gate later ones.

- **The skill writes EDGES, not phases.** `/lumina:wire-task-deps` calls `mcp__lumina__block_task_on_task {task_id, depends_on_id, kind}` for each user-confirmed edge. The phase batching is computed downstream — the skill body MUST NOT pre-batch the tasks itself.
- **Cycle detection is server-side.** `compute_task_batches` returns `AppError::Cycle {edges}` when the DAG contains a cycle (surfacing as MCP `invalid_params` with the offending edges in the message). `/lumina:wire-task-deps` MUST treat a cycle result as a hard error — prompt the user to remove one edge before retrying, NEVER silently drop an edge.
- **Phase display format**: when surfacing the computed batches to the user, format as `Phase 1 (foundation): T1, T2 | Phase 2 (parallel): T3, T4 | Phase 3 (after T3): T5 | …`. The phase label (foundation / parallel / after `<dep>`) is derived from the within-phase `task_kind` tie-break order computed server-side. **Note**: the "foundation" phase label is the batch's sort-derived label (the batch contains every `task_kind == "foundation"` task plus any zero-in-degree `main`/`polish` tasks); it is NOT the same as a per-task `task_kind` value. See §j.1 for the disposition vocabulary and its relationship to intra-story task-subset groupings.
- **Complexity-high gate**: for any task with `complexity = "high"`, the skill MUST prompt the user to confirm it shouldn't split further BEFORE writing any inbound or outbound edge. This gate protects against the empirical reliability degradation documented in R27 for high-complexity tasks.

The skill body cites this section by reference; do NOT inline Kahn's-algorithm semantics or the phase-display format here. This section is the contract for HOW the task graph composes with the downstream executor, not WHAT user prompts the skill body shows.

### §j.1 `task_kind` is task-level phase-disposition (migration 0007 cull)

Migration 0005 introduced `work_items.task_kind` with a four-value taxonomy: `foundation | vertical-slice | pattern-replacement | polish`. The 0007 review (round-3.5) found the taxonomy conflated **three** granularities:

- **Per-task disposition** (legitimate task-level discriminator — what `task_kind` is FOR): `foundation` (prerequisite — floats earliest in intra-phase sort), `main` (core body of work — default), `polish` (after-work — sinks latest). One value per task.
- **Intra-story task-subset groupings** (NOT a `task_kind` value): `vertical-slice` and `pattern-replacement` describe a relationship between an **arbitrary subset** of a story's tasks — not all of them, not a single one. Within one story there may be multiple vertical slices (each spanning a different subset of tasks), several pattern-replacement bundles, plus tasks that belong to no group. A task can belong to zero or more groupings. Groupings exist to mark units-of-implementation: the tasks in one vertical slice are implemented + tested + committed as a unit; same for pattern-replacement bundles.
- **Whole-story structural shape**: a separate concept ("this story IS principally a refactor / a greenfield / etc.") that lumina does not model and round-3.5 does not introduce.

Migration 0007 culled the `task_kind` vocab to `foundation | main | polish` and rebuilt the `work_items` CHECK accordingly. Existing rows carrying the two deprecated values were migrated to `main`. The intra-phase sort key in `repo::task_kind_sort_key` was updated; `foundation` still floats earliest, `polish` still sinks latest, and `main` (or NULL) occupies the middle slot.

**Intra-story task-subset groupings are NOT modelled in schema by round-3.5** — no `task_groups` table, no MCP tools, no validation. `/lumina:decompose-tasks` surfaces proposed groupings in its proposal prose (e.g. "Vertical slice 'auth-flow' covers T3 + T5 + T8"), and the implementer (or future `/lumina:run-batch`) respects the grouping informally when sequencing implementation, testing, and commit. When a real consumer materialises that needs to query groupings from the DB — most likely `/lumina:run-batch` choosing "dispatch these three tasks together as one verification + one commit" — a future migration will add `task_groups (id, story_id, kind, label)` + `task_group_members (group_id, task_id, seq)`. Until then the concept lives purely in skill prose and conversational coordination.

The pattern-replacement workflow signal at the per-task level is carried by `attributes.files_touched_pattern` — `/lumina:decompose-tasks` records the Grep pattern there on each task that's part of a pattern-replacement grouping (so a 9-file sweep split across 3 tasks records the pattern on all 3), and `/lumina:set-task-spec` + `/lumina:story-review` gate their pattern-replacement-specific behaviour on the presence of that key. This is the only DB-level trace of grouping membership in round-3.5; the named grouping itself (which group does this task belong to?) is purely in `/lumina:decompose-tasks`'s proposal prose and any downstream consumer's working memory until the future schema lands.

## §k Tier derivation rule (round-3)

Lumina's dispatch tier (`Tier::{Lite, Deep}` — migration 0006, `work_items.tier` column) is derived server-side via `repo::compute_tier`. The rule is a single source of truth: skill bodies that need to surface a derived tier MUST call `mcp__lumina__get_task_dispatch_plan` (which composes `compute_task_batches` + `compute_tier` per task) rather than re-deriving client-side.

### §k.0 The rule

```text
compute_tier(effort, complexity, files_touched_count, has_cross_repo):
    if complexity == "high":          Deep
    if effort == "l":                 Deep
    if files_touched_count > 3:       Deep
    if has_cross_repo:                Deep
    else:                             Lite
```

Mirrors `/implement`'s deep-vs-lite agent split: cross-file refactors, security-sensitive code, judgement-heavy work go Deep; mechanical, fully-specified, ≤3-file work goes Lite. The `> 3` ceiling matches the `/optimise-apply` / `/review-apply` 3-file-per-item cap — anything above is cross-file by definition.

The rule is intentionally simple (no weights, no calibration). When real workload data accumulates, retuning happens in one place: `repo::compute_tier` + this §k. Round-3 tests pin every branch (`compute_tier_high_complexity_is_deep`, …); changes to the rule are deliberate.

### §k.1 Canonical research-lens vocabulary

The `research_notes.lens` column is free TEXT (migration 0003) — no DB-level enum. Round-3 documents the canonical 5-lens vocabulary used by `/lumina:research-explore`:

| Lens | Meaning |
|------|---------|
| `codebase` | Read the project source; surface relevant existing patterns, anti-patterns, and prior decisions. |
| `library` | Verify third-party library claims: API signatures, version pins, deprecations. |
| `risk` | Surface failure modes, edge cases, regression vectors. |
| `completeness` | Coverage analysis: what's missing from the story scope? |
| `domain` | Subject-matter dive — used only when story `complexity = "high"`. |

Skill bodies (specifically `research-explore`) MUST use these exact wire-form strings; new lenses are added by appending a row here AND updating the consumer skill in the same change. The pre-existing lens conventions documented in §g.2 (`edge-case`, `prior-art`, `tool-eval`, `codebase-recon`, `constraint`, `failure-mode`) are the round-1/2 `research-notes` skill's vocabulary and remain in §g.2 — the §k.1 set is the NEW round-3 multi-agent-exploration vocabulary. The two registries DO overlap in spirit (`risk` ≈ `failure-mode`; `codebase` ≈ `codebase-recon`); a future round-4 may consolidate them.

### §k.2 Typed severity enums (the deliberate vocab split)

Lumina carries TWO severity enums, both typed at the MCP wire surface. They are NOT unified — they describe different concerns:

| Enum | Variants | Wire | Where written |
|------|----------|------|---------------|
| `Severity` | `Critical` / `Major` / `Minor` / `Suggestion` | `critical|major|minor|suggestion` (snake_case) | `findings.severity` (column-level free TEXT in DB; typed at MCP-param surface via `AddFindingParams.severity: Option<Severity>` / `UpdateFindingParams.severity: Option<Severity>`). Used for review/story-review/optimise/research-drift findings — categorisation of code-review findings. |
| `RiskSeverity` | `Low` / `Medium` / `High` / `Critical` | `low|medium|high|critical` (lowercase) | `risks.severity` (column-level CHECK-enforced in DB by migration 0005; typed at MCP-param surface). Used for risk severity on the `risks` table — gates sprint composition. |

**Round-3 closure-gate signal (deferred)**: the round-3 plan proposed extending the closure-gate's hard mode to additionally block `task → done` on open critical/high RISKS on the parent story (in addition to the existing AC-based gate). That extension is deferred to a round-4 follow-up — the load-bearing work (typed Tier + dispatch plan + repo plumbing) shipped in round-3 without requiring the gate extension. Once round-4 lands the extension, §k.2 should be amended to document the gate's risk-aware behaviour.

## §l Six-phase canonical sequence (round-3)

`/lumina:plan-story` walks a story through six canonical phases. Each phase has a HARD precondition computed from `get_story_readiness` booleans (round-2 / migration 0005). Phases gate forward progress; per-block invocation outside `plan-story` is unrestricted (R6 — orchestration ≠ enforcement).

### §l.0 The phase table

| Phase | Blocks | Hard precondition before entry |
|-------|--------|--------------------------------|
| 1. Frame | `problem-statement`, `user-interrogation` | none (story exists) |
| 2. Explore | `research-explore`, `vet-research`, `research-directed` | `problem_statement_set == true` |
| 3. Decide | `alternatives`, `approach`, `not-doing`, `edge-cases`, `risks` | `accepted_research_count >= 1` AND `unresolved_questions == 0` |
| 4. Verify-design | `verification-commands`, `acceptance-criteria`, `story-review` | `has_approach == true` |
| 5. Decompose | `decompose-tasks`, `set-task-spec`, `wire-task-deps` | `acceptance_criteria_count >= 1` AND `verification_commands_set == true` |
| 6. Closure | `closure-gate`, `relevance` | all tasks have `effort` + `complexity` + `tier` set; zero open critical/high risks on the story (round-4 extension) |

The `verification_commands_set` boolean is not yet exposed by `get_story_readiness` (round-2 added the column but the readiness rollup pre-dates round-3). Until a follow-up extends the readiness query, `plan-story` reads `detail.attributes.verification_commands != null` directly from `get_work_item`.

`Phase` is also a typed domain enum (`lumina::domain::Phase` with kebab-case wire `frame|explore|decide|verify-design|decompose|closure`) — not persisted to a column, but available to skill bodies and the future composer.

### §l.1 Skip-with-override audit contract

`plan-story` allows users to skip a block whose precondition has failed, via a "Skip with override" option in the per-block AskUserQuestion. The override is recorded — never silently — via `record_task_activity`:

```text
mcp__lumina__record_task_activity {
  work_item_id: <story_id>,
  entry_type: "execution",          # NOT "vet" — that's vet-research's exclusive use (§c amendment)
  origin: "plan",
  summary: "skip_override: <block_slug>",
  body: "phase=<phase>; prereq_failed=<short reason>; session=${CLAUDE_SESSION_ID}"
}
```

The audit entry is what `/lumina:story-review` later surfaces ("you skipped <block>; was that intentional?"). Without the entry, an override is indistinguishable from a clean walk.

Apply the §c substitution guard verbatim before the call (verify `${CLAUDE_SESSION_ID}` resolved; on non-substitution, write `session=unknown` and warn).

### §l.2 Carve-out — per-block invocation outside plan-story

The six-phase contract binds `/lumina:plan-story` ONLY. Per R6, calling individual skills directly (e.g. `/lumina:problem-statement <id>` when no phase is active) remains unrestricted — no precondition check fires. This carve-out preserves user agency: the chained runner gives structure; direct invocation gives escape.

### §l.3 Phase persistence (deferred)

Round-3 does NOT persist `current_phase` to a column. The phase is recomputed every invocation from `get_story_readiness` booleans. A future round-4 may extend `get_story_readiness` with a `current_phase: Phase` field if user telemetry shows resumption churn.

## §m Epic/focus semantics (migration 0010)

Migration 0010 renamed the hierarchy's third level `feature` → `focus` and reshaped the two grouping kinds (`epic`, `focus`) into deliverables with distinct closure semantics. The full design rationale lives in [`docs/adr/0001-epic-focus-semantics.md`](../../../docs/adr/0001-epic-focus-semantics.md); the schema-canonical description is in [`lumina/CONTEXT.md`](../../../lumina/CONTEXT.md). This section is the plugin-side contract for the skills that write these fields.

### §m.0 Epic — a closeable deliverable

An **epic** is a closeable unit of delivered value. It carries:

- A MANDATORY `outcome` (set at `create_work_item` time; revised later via `set_epic_plan`) — the value the epic delivers, stated as an end-state.
- An optional `context` plan attribute (revised via `set_epic_plan`'s JSON-merge).
- One or more **close-criteria** — checkable conditions gating the epic's close, governed by the epic's `closure_gate` (migration 0010 widened `set_closure_gate` to accept an epic as well as a story).

**Epic done-rule**: an epic is "done" when ALL of its close-criteria are checked AND ALL descendant stories are in a terminal status (`done` / `cancelled`). The close-criteria are the epic's own deliverable gate; the descendant-story rollup is the structural gate. Both must hold.

### §m.1 Focus — a per-epic grouping (renamed from feature)

A **focus** (formerly `feature`) is a per-epic grouping of stories. It carries:

- A MANDATORY `shape ∈ {vertical-slice, cross-cutting, foundational}` (set at `create_work_item` time via the `shape` param; revised via `set_shape`). Stored as a non-nullable scalar on `work_items.shape`.
- An optional `framing` plan attribute (revised via `set_focus_plan`'s JSON-merge).

**Focus done-rule**: "done" for a focus is a PURE ROLLUP — a focus is done iff all its descendant stories are terminal. A focus has NO close-criteria and NO independent closure gate; it is a structural grouping, not a separately-gated deliverable. This is the load-bearing asymmetry with an epic (§m.0): an epic gates its own close on close-criteria, a focus does not.

### §m.2 Kind-precondition writers for the new fields

Four new plugin skills are the kind-precondition writers for these fields. Each fails fast on the wrong kind (per §e/§h kind-precondition signposts) and follows the §b check-before-act + §c provenance conventions:

| Skill | Writes | Kind-precondition | MCP tool |
|---|---|---|---|
| `epic-outcome` | epic `outcome` | epic-only | `set_epic_plan` (and `create_work_item` at creation) |
| `epic-close-criteria` | epic close-criteria | epic-only | `add_acceptance_criterion` family under the epic's `closure_gate` |
| `focus-shape` | focus `shape` | focus-only | `set_shape` |
| `focus-framing` | focus `framing` | focus-only | `set_focus_plan` |

`outcome` and `shape` are MANDATORY at `create_work_item` time for `epic` and `focus` respectively; the `epic-outcome` / `focus-shape` skills are the post-creation revise path (and the interrogation surface that fills them when an item was created with a placeholder).

---

> **§-letter allocation history**: §a-§h were the round-1 (`lumina-story-planning-workflow`) allocation. §i and §j were added by round-2 (`lumina-story-planning-round-2`). §k and §l were added by round-3 (`lumina-story-planning-round-3`). §m was added by the migration-0010 epic/focus-semantics pass. Future rounds should append §n, §o, … rather than re-using a freed letter; reserved letters protect cross-references in skill bodies that cite by short letter.

Pointer back: the plugin entry point is `claude/plugins/lumina-story-blocks/README.md`; the parent plan is `docs/plans/lumina-story-planning-workflow.md` (round 1), `docs/plans/lumina-story-planning-round-2.md` (round 2), and `docs/plans/lumina-story-planning-round-3.md` (round 3).
