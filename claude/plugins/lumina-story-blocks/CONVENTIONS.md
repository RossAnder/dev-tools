# Conventions — `lumina-story-blocks` plugin

This document is the shared contract for every skill in the `lumina-story-blocks` plugin (manifest `name: "lumina"`, slash invocations `/lumina:<block>`). Every skill in this plugin MUST follow these conventions; the skill bodies cite the sections below by short reference (`§a`, `§b`, …) rather than re-stating the rules inline. If a rule here changes, the skill bodies are the consumers and must be re-read in lockstep.

The plugin's purpose is to drive lumina's existing MCP tool surface (catalogued in [`./skills/mcp/SKILL.md`](./skills/mcp/SKILL.md)) to fill the per-story planning blocks one block at a time, idempotently, with user-mediated supersession on re-run.

## §a Frontmatter shape

Every DB-mutating skill in this plugin declares AT MINIMUM these four keys in its YAML frontmatter, in this order (or six keys if the skill runs in forked context — see §d). Read-only documentation skills (currently only the `mcp` catalogue) omit `disable-model-invocation: true` per the exception below; they MAY also omit `arguments` if they take none.

> **Forward reference**: the four-key shape below is the minimum. Exactly one skill in this plugin (`research-notes`) runs in a forked subagent context and declares two ADDITIONAL keys (`context: fork`, `agent: general-purpose`) for a total of six — see §d for the rationale and the canonical six-key example.

```yaml
---
name: <skill-name>
description: <one-sentence summary>
arguments: [work_item_id]
disable-model-invocation: true
---
```

- `name` is the skill's invocation suffix; combined with the plugin manifest's `name: "lumina"`, the full slash form is `/lumina:<name>`.
- `description` is a single sentence. It is what model-auto-invocation matches against — but see `disable-model-invocation` below: in this plugin the description is informational only (it does NOT enter the routing context).
- `arguments: [work_item_id]` declares one named positional argument, substituted as `$work_item_id` in the body. Per R8, named arguments give the body a stable substitution rather than relying on `$ARGUMENTS` blob parsing.
- `disable-model-invocation: true` is MANDATORY for every DB-mutating skill in this plugin. Per R1, Claude Code exposes three invocation paths — `/name` slash, model-auto-trigger via `description`, and explicit `Skill` tool dispatch — and `disable-model-invocation: true` removes the description from the routing context so ONLY explicit triggers (the user typing `/lumina:<name>`, or eventually a UI button dispatching the skill) fire it. This is non-negotiable: these skills write to the lumina database and would be unsafe to auto-fire mid-conversation off a description match.

**Exception — read-only / documentation skills.** The `mcp` skill in this plugin is a read-only documentation surface (it documents the lumina MCP tool catalogue and does not call any write tool itself). Such skills MAY omit `disable-model-invocation: true` to remain model-discoverable — letting agents auto-find the catalogue when they need to drive the lumina MCP surface. The rule above applies only to skills that write to the lumina database via `mcp__lumina__*` write tools.

Example (the `problem-statement` skill):

```yaml
---
name: problem-statement
description: Capture or update a story's problem_statement (what's broken, who's affected, success criteria).
arguments: [work_item_id]
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

Every skill in this plugin invokes step 5 with the SAME wording so the user sees a consistent UX across the plugin. Copy this template verbatim, substituting `<field-name>` and `<current-value-summary>` (a short, single-line summary of the existing value — truncate long bodies to ~80 chars + ellipsis):

> **Question header**: `Supersede?`
>
> **Question body**: `This story's <field-name> is already set to: <current-value-summary>. Replace it with the new value you provided? Choosing 'Keep current' aborts this skill invocation without writing.`
>
> **Options** (exactly 2):
> - `Replace` — `Write the new value, superseding the existing one`
> - `Keep current` — `Abort this invocation, leave the existing value in place`

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

- `entry_type` is `"execution"` for all writes from this plugin. Do NOT use `"verification"` — the lumina SKILL.md catalogue notes that `verification` activity entries are appended INTERNALLY by `check_acceptance_criterion` and the `record_task_activity` enum explicitly rejects `verification`. The other accepted values (`vet`, `comment`) are reserved for other roles.
- **Note on entry_type / origin orthogonality**: `entry_type` (`execution` / `vet` / `comment`) is the activity-stream channel; `origin` (`plan` / `implement` / `review` / `optimise` / `tdd` / `human` / `none`) is the agent or lifecycle stamp. They are orthogonal — `entry_type: "execution"` here pairs with `origin: "plan"`. If `lumina` migration N+ adds a `planning` entry_type, this convention should re-map plan-time writes to `entry_type: "planning"` to distinguish them from `/implement`-driven activity. Until then, `execution` is the chosen channel for plan-time writes and the agent SHOULD read the `origin` stamp to tell which lifecycle phase produced any given row.
- `origin` is `"plan"` because these skills run inside the planning workflow (R8 origin taxonomy: `plan` / `implement` / `review` / `optimise` / `tdd` / `human` / `none`).
- `${CLAUDE_SESSION_ID}` is the Claude Code session substitution variable (R8). Threading it through the activity body lets a later audit join planning activity to the originating Claude session.
- One activity entry per write — not per skill invocation. A skill that writes twice (e.g. supersedes one note AND adds a new one in the same invocation) records two activity entries.

## §d Forked context (research-notes only)

Exactly ONE skill in this plugin runs in a forked subagent context: `research-notes`. That skill's frontmatter adds two extra keys (per R2) beyond the four mandatory ones from §a:

```yaml
---
name: research-notes
description: Identify research gaps and add proposed research notes to a story.
arguments: [work_item_id]
disable-model-invocation: true
context: fork
agent: general-purpose
---
```

**Why fork only here**: research is a multi-step exploration — gap identification, Context7 lookups, WebSearch queries, targeted code reads, draft synthesis. Each of those operations leaves tool-output noise in the conversation context that the main planning session does not need. Running this skill in a forked subagent isolates that noise; the parent conversation sees only the final summary (and the lumina rows themselves are queryable via `get_work_item`).

**Why every other skill stays inline**: all other skills in this plugin are short interactive Q&A loops — the user types a few sentences, the skill writes one or two MCP calls, done. Inline execution keeps the user in the parent context where they can interject, ask follow-ups, or chain skills. Forking these would force a context switch with no compensating benefit.

## §e Sentry pattern — skill = instructions, MCP = execution

Per R6, Anthropic's published Sentry exemplar pairs a SKILL (workflow instructions) with an MCP server (execution). This plugin follows the same split:

- **Skill body** contains the workflow: the prompts to show the user, the order in which to ask them, the decision branches (absent/present/match/mismatch), the supersession-confirm interaction, the cite to `record_task_activity`. It is prose + pseudocode telling the agent WHAT TO DO.
- **MCP tools** (`mcp__lumina__*`) contain the business logic: validation, transactions, FK integrity, the `set_story_plan` merge semantics, the `closure_gate=hard` task-blocking rule, the partial UNIQUE on `repo_links.is_primary`, the `record_task_activity` enum constraint. The skill body MUST NOT duplicate or shadow this logic.

Worked counter-example (DO NOT do this):

> ✗ Skill body reads `attributes.problem_statement` and `attributes.research_notes` via `get_work_item`, builds the merged JSON `{problem_statement: <new>, research_notes: <unchanged>}` itself, then calls `update_work_item` with the whole merged blob.

Worked correct example:

> ✓ Skill body calls `mcp__lumina__set_story_plan({id, problem_statement: <new>})`. Lumina's `repo.rs` reads the existing attributes, merges the present key, leaves absent keys untouched, runs the write in one transaction, and emits the event.

The skill's job is to know which MCP tool to call and what to put in the arguments. Lumina's job is to make that call safe.

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

Two architecturally distinct primitive types live here, each with its own promotion path:

- **§g.1 — Attribute-key conventions**: a key NAME inside the `work_items.attributes` JSON blob. Promotion path: `ALTER TABLE ADD COLUMN` (or a widened `set_*` MCP tool that merges the key safely).
- **§g.2 — Column-value lens conventions**: a string VALUE in a typed column (e.g. `research_notes.lens`). Promotion path: new first-class table + data migration to replace the lens-value enum.

Do not conflate the two: an attribute-key convention is data hiding inside a JSON blob (no schema enforcement); a column-value lens convention is a typed-column value (schema-typed as text, but conventionally constrained by the registry below).

## §g.1 Attribute-key conventions (JSON-merge keys in `work_items.attributes`)

| Convention | Where stored | Storage primitive | Used by |
|---|---|---|---|
| `attributes.not_doing` | `work_items.attributes` JSON key | **DISABLED — see R1**: `update_work_item` performs column-level COALESCE on `attributes`, clobbering sibling keys. The `lumina:not-doing` skill is disabled until lumina exposes a safe attributes-merge MCP tool. | `lumina:not-doing` skill (disabled) |

Notes:

- The `attributes.not_doing` convention WAS intended to ride a non-existent JSON-merge semantics of `update_work_item` (column-level COALESCE clobbers sibling keys, verified in `lumina/src/repo.rs:1404-1421`). The skill is currently disabled — see R1 / R2 in the review ledger. Promotion options: add a dedicated `set_work_item_attributes` MCP tool, OR widen `SetStoryPlanParams` to accept `not_doing` so `set_story_plan` becomes the entry point.
- **Lens-key drift warning**: consumers of `attributes.not_doing` (skill bodies, future UI code, exporter, smoke tests) MUST reference the literal key string `not_doing` (snake_case). Lumina has no schema-level protection against typos — writing `attributes.notDoing` or `attributes.not-doing` succeeds silently and produces drift. Consider adding a lumina-side test that scans `work_items.attributes` for unknown top-level keys as a drift smoke check.

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

---

Pointer back: the plugin entry point is `claude/plugins/lumina-story-blocks/README.md`; the parent plan is `docs/plans/lumina-story-planning-workflow.md`.
