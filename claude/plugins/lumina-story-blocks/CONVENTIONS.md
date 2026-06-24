# Conventions — `lumina-story-blocks` plugin

This document is the shared contract for every skill in the `lumina-story-blocks` plugin (manifest `name: "lumina"`, slash invocations `/lumina:<block>`). Every skill in this plugin MUST follow these conventions; the skill bodies cite the sections below by short reference (`§a`, `§b`, …) rather than re-stating the rules inline. If a rule here changes, the skill bodies are the consumers and must be re-read in lockstep.

The plugin's purpose is to drive lumina's existing MCP tool surface (catalogued in [`./skills/mcp/SKILL.md`](./skills/mcp/SKILL.md)) to fill the per-story planning blocks one block at a time, idempotently, with user-mediated supersession on re-run.

## §a Frontmatter shape

Every DB-mutating skill in this plugin declares AT MINIMUM these three keys in its YAML frontmatter, in this order — `name`, `description`, `arguments` — and SHOULD additionally declare the RECOMMENDED `argument-hint` whenever `arguments` is non-empty. Read-only documentation skills (currently the `mcp`, `lifecycle`, and `next-block` catalogues) MAY also omit `arguments` (and correspondingly `argument-hint`) if they take none. No skill in this plugin declares `disable-model-invocation` — these skills are model-invocable by design (see the invocation-path note below).

> **Forward reference**: the three-key shape below is the minimum (four with the recommended `argument-hint`), and it is the CANONICAL frontmatter for every skill — including the exploration-heavy ones. Forking is no longer declared in frontmatter: it is a RUNTIME, mode-conditional decision (autonomous vs interactive) made from a corroborated mode signal, NOT a static `context: fork` / `agent:` key pair. See §d for that rule.

```yaml
---
name: <skill-name>
description: <one-sentence summary>
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---
```

- `name` is the skill's invocation suffix; combined with the plugin manifest's `name: "lumina"`, the full slash form is `/lumina:<name>`.
- `description` is a single sentence. It is what model-auto-invocation matches against — and in this plugin it DOES enter the routing context: with no `disable-model-invocation` flag, every skill is model-discoverable, so the description must be a faithful, specific summary the model can route on.
- `arguments: [work_item_id]` declares one named positional argument, substituted as `$work_item_id` in the body. Per R8, named arguments give the body a stable substitution rather than relying on `$ARGUMENTS` blob parsing.
- `argument-hint: "[work_item_id]"` is the slash-autocomplete hint surfaced to the user when they type `/lumina:<name>` — without it the autocomplete shows nothing and the user has to guess that a work-item id is expected. It is RECOMMENDED whenever `arguments` is non-empty (and omitted alongside `arguments` for read-only catalogues that take none). The hint mirrors the `arguments` shape verbatim.
- **These skills are model-invocable by design.** Per R1, Claude Code exposes three invocation paths — the human `/name` slash, model-auto-trigger via `description`, and model-issued `Skill` tool dispatch. All three are allowed for every skill in this plugin: NONE carries `disable-model-invocation`. This is deliberate — the planning/lifecycle skills are built to run AUTONOMOUSLY (lumina-spawned / scheduler-resumed sessions drive the planning + execution flow), and the chained runners (`plan-story`, `create-project`) must be able to `Skill()`-dispatch their per-block siblings. Removing the flag from the routing context would block both. Safety no longer comes from restricting who may invoke; it comes from three places: (1) the §b check-before-act idempotency contract — read before write, never blind-overwrite; (2) the §n.1 TXN-idempotency of the lifecycle mutators; and (3) crucial or ambiguous decisions surfaced as durable lumina `open_questions` an operator answers asynchronously, so the flow blocks on a RECORDED decision rather than a live prompt (in autonomous mode there is no live `AskUserQuestion` channel — those gates DEGRADE to durable open-questions; see §d). **Chained runners** (`plan-story`, `create-project`) accordingly `Skill()`-dispatch their per-block siblings — see §l.4 (the single canonical home for that execution-path doctrine).

**Read-only / documentation skills.** The `mcp`, `lifecycle`, and `next-block` skills are read-only surfaces (they call only read tools and write nothing). Like everything else in the plugin now, they are model-discoverable — agents can auto-find the catalogue / advisor when they need it.

Example (the `problem-statement` skill):

```yaml
---
name: problem-statement
description: Capture or update a story's problem_statement (what's broken, who's affected, success criteria).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
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

## §d Forked context — a runtime, mode-conditional decision

Some skills in this plugin are multi-step explorations whose intermediate tool output would saturate the main planning context (`research-notes`, `research-explore`, `research-directed`, `story-review`, `decompose-tasks`). Whether such a skill runs FORKED (in an isolated subagent) or INLINE (in the parent conversation) is NOT a static frontmatter property — it is a RUNTIME decision keyed on the execution mode:

- **Autonomous mode** (lumina-spawned / scheduler-driven — no live human at a terminal): the skill FORKS into a subagent (`agent: general-purpose` is the fork target, applied at runtime). There is no interactive `AskUserQuestion` channel, so the exploration runs in isolation and reports back through durable-primitive comms; the spawning context sees only the final summary (and the lumina rows are queryable via `get_work_item`).
- **Interactive mode** (human terminal): the skill runs INLINE, so live `AskUserQuestion` prompts reach the user directly and they can interject, ask follow-ups, or chain skills. Forking here would force a context switch with no compensating benefit.

The mode is selected from a CORROBORATED mode signal — the `LUMINA_AUTONOMOUS` environment signal cross-checked server-side (see `lumina/server/src/pty/mode.rs`), fail-safe to interactive when the signal is absent or unverifiable. It is NOT read from static `context: fork` / `agent:` frontmatter keys: as of this sprint the exploration skills carry the canonical frontmatter from §a (the three mandatory keys plus the recommended `argument-hint`) and declare no fork keys at all. The `agent: general-purpose` fork target named above is supplied by the runtime when it forks, not baked into the skill file.

**Resolution mechanism (how a skill learns its mode at runtime)**: a skill/agent corroborates its mode by calling `mcp__lumina__get_execution_mode` with its `LUMINA_AUTONOMOUS` env-var value as the `token` argument; the tool returns `{ "mode": "autonomous" | "interactive" }`.

- A present-but-invalid or empty token resolves to `interactive` (the fail-safe above); only a token matching the server's per-process injected secret resolves to `autonomous`. This is the spoof-resistant corroboration — a stray `LUMINA_AUTONOMOUS` in a human shell carries no valid token, so it cannot fake autonomous mode.
- The orchestrator (a lumina-PTY-spawned session) is ALSO steered to autonomous via its baked `--append-system-prompt`, so it need not call the tool. The tool is primarily how a `Task`-spawned TEAMMATE corroborates its mode: the teammate inherits the token via the propagated `settings.json` env and has the full `/mcp` surface via project config.
- Behaviour per mode is unchanged from above: `autonomous` ⇒ fork / durable-primitive comms; `interactive` ⇒ inline / live `AskUserQuestion`.

**Autonomous-mode AUQ degradation.** A skill whose body gates on an `AskUserQuestion` — the §b-supersession confirm, a §l.1 skip-override, any decision prompt — has NO interactive channel in autonomous mode. Every such gate MUST DEGRADE to a durable lumina `open_question`: the skill records the decision as an open question, blocks on the operator's asynchronous answer, and resumes once it is recorded. It MUST NEVER hang waiting for input that cannot arrive. In interactive mode the AUQ reaches the user live and runs unchanged. Mode is the corroborated signal above (`mcp__lumina__get_execution_mode`), fail-safe to interactive when unverifiable.

**Why isolate the noise (when forking applies)**: each of these skills leaves heavy tool-output noise in the conversation context that the main planning session does not need. `research-notes` runs Context7 lookups, WebSearch queries, targeted code reads, and draft synthesis; `research-explore` dispatches parallel lens-agents; `research-directed` verifies decision-grade claims and emits drift findings; `story-review` runs a 7-category rubric over the full story detail, performing cross-block semantic comparisons and per-rubric finding writes; `decompose-tasks` reads every accepted-state planning block, optionally fans out parallel sub-decompose-agents per foundation-disjoint module (R26), runs pattern-replacement file enumeration via Grep (R25), and synthesises a proposed task list. Forking in autonomous mode isolates that noise; running inline in interactive mode keeps the user in the loop.

**Why every other skill always stays inline**: all other skills in this plugin are short interactive Q&A loops — the user types a few sentences, the skill writes one or two MCP calls, done. They have no exploration noise to isolate and depend on the live user channel, so they are never candidates for the autonomous-mode fork regardless of mode.

A skill that runs forked MAY append reporting/summary steps after the 5-step §b sequence (e.g. research-notes' final summary step). The 5-step §b sequence itself MUST appear in order; additions go after step 5, not interleaved.

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
- **Solution-shape convention — `execution_strategy` holds the solution shape; `problem_statement` stays problem-only.** The two story-plan attribute keys divide responsibility cleanly: `attributes.problem_statement` describes ONLY the problem (what's broken, who's affected, success criteria) and MUST NOT carry the chosen solution / approach; `attributes.execution_strategy` is where the solution shape (the approach, the how) lives. Skills MUST NOT overload `problem_statement` with solution wording — when the user states a fix, route it to `execution_strategy` (via `/lumina:approach` → `set_story_plan`), not into the problem statement. This is a documented-convention resolution (no new attribute / column): the `problem-statement` skill keeps its field problem-only, and the `approach` skill owns `execution_strategy`. Rationale: a problem statement contaminated with a baked-in solution pre-empts the explore/decide phases (`get_story_readiness` gates exploration on `problem_statement_set`, then decision on accepted research) — keeping the two separate preserves the six-phase sequence (§l).

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
- **Provenance**: every finding carries `origin: "plan"` (mirroring §c) and is written from whatever context the skill runs in (forked in autonomous mode, inline in interactive mode — per §d's runtime mode-conditional rule). The `confidence` field is optional but recommended for findings derived from heuristic checks (word-overlap, LLM judgement) — distinguishes them from finds that surface a structural contradiction.

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

**`files_touched_count` source (migration 0023):** the count is now the DE-DUPLICATED EXPECTED `task_files` count — the number of distinct `(repo_link_id, path)` rows of `kind='expected'` for the task (the first-class `task_files` table that the migration-0023 pass promoted the former `attributes.files_touched` JSON array into) — NOT the raw `attributes.files_touched` array length. `set_task_spec.files_touched` writes that expected set (and `compute_tier`/`get_task_dispatch_plan` read its deduped count), so two spellings of the same primary-repo file (a bare path and an explicit-primary `{repo, path}` slug) fold to one and no longer double-count toward the `> 3` ceiling. The RULE above is UNCHANGED — only the count's source + dedup moved.

The rule is intentionally simple (no weights, no calibration). When real workload data accumulates, retuning happens in one place: `repo::compute_tier` + this §k. Round-3 tests pin every branch (`compute_tier_high_complexity_is_deep`, …); changes to the rule are deliberate.

### §k.1 Canonical research-lens vocabulary

The `research_notes.lens` column is free TEXT (migration 0003) — no DB-level enum. Round-3 documented the canonical research-lens vocabulary used by `/lumina:research-explore`; round-5 (T11) amended it from five to SIX lenses by adding the always-on `contrarian` lens. The canonical vocabulary line is exactly: `codebase, library, risk, completeness, domain, contrarian`.

| Lens | Meaning |
|------|---------|
| `codebase` | Read the project source; surface relevant existing patterns, anti-patterns, and prior decisions. |
| `library` | Verify third-party library claims: API signatures, version pins, deprecations. |
| `risk` | Surface failure modes, edge cases, regression vectors. |
| `completeness` | Coverage analysis: what's missing from the story scope? |
| `domain` | Subject-matter dive — used only when story `complexity = "high"`. |
| `contrarian` | Disconfirmation pass (round-5, R51): actively seek evidence the chosen/obvious direction is WRONG — steelman the approach NOT taken, surface competing patterns / prior art the four confirmatory lenses are biased against producing, and name the assumptions that must hold for the planned direction to be right. ALWAYS-ON (dispatched every invocation, not complexity-gated). |

Round-5 (T11) lens-count change: the always-on set is now FIVE (`codebase`, `library`, `risk`, `completeness`, `contrarian`) plus `domain` added only when story `complexity = "high"` — six total on a high-complexity story. `contrarian` is the new always-on 6th lens; `domain` remains the one complexity-gated lens.

Skill bodies (specifically `research-explore`) MUST use these exact wire-form strings; new lenses are added by appending a row here AND updating the consumer skill in the same change. **Byte-consistency note**: the §k.1 vocabulary line above and the lens list in [`research-explore/SKILL.md`](./skills/research-explore/SKILL.md) (step 2) MUST stay byte-identical — the drift gate (`scripts/verify-plan-story-blocks.sh`) does NOT check lens-name consistency, so a mismatch passes CI silently and must be caught by hand. The pre-existing lens conventions documented in §g.2 (`edge-case`, `prior-art`, `tool-eval`, `codebase-recon`, `constraint`, `failure-mode`) are the round-1/2 `research-notes` skill's vocabulary and remain in §g.2 — the §k.1 set is the round-3/5 multi-agent-exploration vocabulary. The two registries DO overlap in spirit (`risk` ≈ `failure-mode`; `codebase` ≈ `codebase-recon`); a future round may consolidate them.

### §k.2 Typed severity enums (the deliberate vocab split)

Lumina carries TWO severity enums, both typed at the MCP wire surface. They are NOT unified — they describe different concerns:

| Enum | Variants | Wire | Where written |
|------|----------|------|---------------|
| `Severity` | `Critical` / `Major` / `Minor` / `Suggestion` | `critical|major|minor|suggestion` (snake_case) | `findings.severity` (column-level free TEXT in DB; typed at MCP-param surface via `AddFindingParams.severity: Option<Severity>` / `UpdateFindingParams.severity: Option<Severity>`). Used for review/story-review/optimise/research-drift findings — categorisation of code-review findings. |
| `RiskSeverity` | `Low` / `Medium` / `High` / `Critical` | `low|medium|high|critical` (lowercase) | `risks.severity` (column-level CHECK-enforced in DB by migration 0005; typed at MCP-param surface). Used for risk severity on the `risks` table — gates sprint composition. |

**Round-3 closure-gate signal (deferred)**: the round-3 plan proposed extending the closure-gate's hard mode to additionally block `task → done` on open critical/high RISKS on the parent story (in addition to the existing AC-based gate). That extension is deferred to a round-4 follow-up — the load-bearing work (typed Tier + dispatch plan + repo plumbing) shipped in round-3 without requiring the gate extension. Once round-4 lands the extension, §k.2 should be amended to document the gate's risk-aware behaviour.

## §l Six-phase canonical sequence (round-3)

`/lumina:plan-story` walks a story through six canonical phases. Each phase has a HARD precondition computed from `get_story_readiness` booleans (round-2 / migration 0005). Phases gate forward progress; per-block invocation outside `plan-story` is unrestricted (R6 — orchestration ≠ enforcement). For HOW a chained runner actually executes each phase's blocks — `Skill()`-dispatch of each per-block sibling — see §l.4.

### §l.0 The phase table

| Phase | Blocks | Hard precondition before entry |
|-------|--------|--------------------------------|
| 1. Frame | `problem-statement`, `user-interrogation` | none (story exists) |
| 2. Explore | `research-explore`, `vet-research`, `research-directed` | `problem_statement_set == true` |
| 3. Decide | `alternatives`, `approach`, `not-doing`, `edge-cases`, `risks` | `accepted_research_count >= 1` AND `unresolved_questions == 0` |
| 4. Verify-design | `verification-commands`, `acceptance-criteria`, `story-review` | `has_approach == true` |
| 5. Decompose | `decompose-tasks`, `set-task-spec`, `wire-task-deps` | `acceptance_criteria_count >= 1` AND `verification_commands_set == true` |
| 6. Closure | `closure-gate`, `relevance` | all tasks have `effort` + `complexity` + `tier` set; zero open critical/high risks on the story (round-4 extension) |

The `verification_commands_set` boolean is now EXPOSED by `get_story_readiness` (round-5 T2/T3 extended the readiness query with `verification_commands_set` alongside `plan_epoch` and `gating_tier`). `plan-story` reads the readiness field (or the dossier's folded readiness) for the Phase-5 precondition — NO LONGER `detail.attributes.verification_commands != null` directly from `get_work_item`. (Round-3's note that it was unexposed is superseded.)

`Phase` is also a typed domain enum (`lumina::domain::Phase` with kebab-case wire `frame|explore|decide|verify-design|decompose|closure`) — not persisted to a column, but available to skill bodies and the future composer.

### §l.1 Skip-with-override audit contract — RETIRED (round-5 T7)

> **TOMBSTONE.** This contract is RETIRED. The per-block `Run / Skip / Inspect / Abort` `AskUserQuestion` gate that the skip-override audited is GONE — the round-5 (T7) `plan-story` rewrite replaced the per-block gate-walker with the §o stage-machine orchestrator, which auto-runs the planning phases with NO per-block ceremony and concentrates user interaction into TWO grills plus an epoch-scoped rework loop. With no skip gate, there is no skip to audit.
>
> **Replacement.** Plan-time invalidation/redirection is now audited by the §o **rework contract**: a misaligned `align` grill bumps the plan epoch, invalidates the affected blocks' stale rows (supersede / cancel / retire / remove), and records ONE rework-audit `record_task_activity` entry. See §o ("the rework contract"). The retired skip-override path itself writes NOTHING.
>
> This section is kept as a tombstone pointer so existing `§l.1` cross-references (in skill bodies, the README, the project `CLAUDE.md` files) do not dangle — they now resolve to "retired; see §o rework audit."

### §l.2 Carve-out — per-block invocation outside plan-story

The six-phase contract binds `/lumina:plan-story` ONLY. Per R6, calling individual skills directly (e.g. `/lumina:problem-statement <id>` when no phase is active) remains unrestricted — no precondition check fires. This carve-out preserves user agency: the chained runner gives structure; direct invocation gives escape.

### §l.3 Phase persistence (deferred)

Round-3 does NOT persist `current_phase` to a column. The phase is recomputed every invocation from `get_story_readiness` booleans. A future round-4 may extend `get_story_readiness` with a `current_phase: Phase` field if user telemetry shows resumption churn.

### §l.4 `Skill()`-dispatch — the agent execution path for chained runners

`/lumina:plan-story` and `/lumina:create-project` are **chained runners**: skills whose body walks a sequence of sibling per-block skills. To "run block X", a runner issues `Skill("lumina:<block>", "<work_item_id>")` — dispatching each per-block sibling via the `Skill` tool, in §l.0 phase order, gated by the same `get_story_readiness` preconditions. The dispatched skill runs its own real body (the §b check-before-act 5-step sequence, the §c provenance write), so the runner delegates the block's work rather than re-implementing it.

This path was formerly BLOCKED: every DB-mutating block skill carried `disable-model-invocation: true`, and the `Skill` tool is a model-driven invocation path (R1 path (c)), so a runner-issued `Skill("lumina:X")` was refused at the harness layer — even under an explicit human slash-invocation. That refusal is why an "inline-replication" workaround formerly lived here. The flag was removed plugin-wide (§a), so `Skill()`-dispatch is now the documented, working path; the workaround is retired.

Two rules bound the dispatch:

- **§l.4(a) — Bounded nested-runner recursion.** `create-project` dispatches `plan-story`, which is itself a runner — so dispatch nests. The chain has a fixed depth bound: `create-project` (depth 0) dispatches `plan-story` (depth 1), which dispatches its leaf blocks (depth 2). No block beneath `plan-story` is itself a chained runner, so dispatch TERMINATES at depth 2. A runner MUST NOT expand a leaf block into further runner machinery beyond this documented chain — a leaf is a single `Skill()`-dispatch, never a re-entry into runner recursion. This caps context growth and prevents unbounded expansion. (story-review F2.)
- **§l.4(b) — Forked blocks run their real fork.** Three blocks fork per §d: `research-explore` (Phase 2), `story-review` (Phase 4), `decompose-tasks` (Phase 5). Because the runner dispatches the actual skill via `Skill()`, these blocks run their REAL fork behaviour automatically — the multi-lens research agents, the 7-category review rubric, the per-foundation-module decompose agents — under §d's runtime mode rule. There is no longer a replication step that could silently drop the Phase-2/4/5 judgement depth; the dispatched skill exercises the forked tier itself.

(The autonomous-mode AUQ-degradation rule that formerly lived here as §l.4(b) now lives in §d, where it applies uniformly to any skill's `AskUserQuestion` gate, dispatched or directly invoked.)

**Single source of truth.** §l.4 is the ONE canonical home for this execution-path doctrine. `plan-story`'s Step 4, `create-project`'s block-dispatch steps, the dogfood runbook (`lumina/docs/runbooks/dogfood-lifecycle.md`), §a's invocation-path note, the README, and the project `CLAUDE.md` files CITE §l.4 rather than restating it.

**Partial-state recoverability.** A chained runner that dies mid-walk does NOT corrupt the story: every dispatched block is independently idempotent (§b) and writes through its own single-mutation-path transaction, so a half-walk leaves a consistent prefix. `get_story_readiness` recomputes the phase verdict from booleans on every call (§l.3 — phase is never persisted), so a resumed walk picks up exactly where readiness says it is. The drift/coverage check (`scripts/verify-plan-story-blocks.sh`) guards this: it asserts the documented §l.0 phase blocks stay aligned with the `skills/` directory, that both runners document `Skill()`-dispatch, and that the `disable-model-invocation` flag does NOT reappear on any skill.

## §m Epic/focus semantics (migration 0010)

Migration 0010 renamed the hierarchy's third level `feature` → `focus` and reshaped the two grouping kinds (`epic`, `focus`) into deliverables with distinct closure semantics. The full design rationale lives in [`docs/adr/0001-epic-focus-semantics.md`](../../../docs/adr/0001-epic-focus-semantics.md); the schema-canonical description is in [`lumina/CONTEXT.md`](../../../lumina/CONTEXT.md). This section is the plugin-side contract for the skills that write these fields.

### §m.0 Epic — a closeable deliverable

An **epic** is a closeable unit of delivered value. It carries:

- A MANDATORY `outcome` (set at `create_work_item` time; revised later via `set_epic_plan`) — the value the epic delivers, stated as an end-state.
- An optional `context` plan attribute (revised via `set_epic_plan`'s JSON-merge).
- One or more **close-criteria** — checkable conditions gating the epic's close; an epic needs ≥1 before its first story can be created. Unlike a story, an epic has NO `closure_gate`: `set_closure_gate` is story-only, and the epic-done gate is unconditional — ALL close-criteria must be checked (see the done-rule below), there is no `hard`/`soft` mode at the epic level.

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
| `epic-close-criteria` | epic close-criteria | epic-only | `add_acceptance_criterion` family on the epic (unconditional done-gate; no epic `closure_gate`) |
| `focus-shape` | focus `shape` | focus-only | `set_shape` |
| `focus-framing` | focus `framing` | focus-only | `set_focus_plan` |

`outcome` and `shape` are MANDATORY at `create_work_item` time for `epic` and `focus` respectively; the `epic-outcome` / `focus-shape` skills are the post-creation revise path (and the interrogation surface that fills them when an item was created with a placeholder).

## §n Lifecycle/orchestration skills (migration 0016)

Migration 0016 ([ADR-0005](../../../docs/adr/0005-sprint-lifecycle-worktree-ownership.md)) added a NEW skill category that is architecturally distinct from every §a-§m planning block. The four skills — `create-project`, `compose-sprint`, `run-sprint` (all mutating), and `lifecycle` (read-only) — do NOT fill one region of a story's data layout. They orchestrate the surrounding lifecycle: `create-project` bootstraps a `project → epic → focus → story` hierarchy; `compose-sprint` composes a worktree-OWNING sprint from a story's ready tasks (`create_sprint` defaults the sprint to `draft` status, then `create_worktree` / `add_tasks_to_sprint` / `set_sprint_status` drive it `draft→ready→active`); `run-sprint` drives the team-execution claim→complete→review loop (`claim_next_task` / `complete_task` / `record_task_commits` / the companion-executed `execute_worktree_merge`, with `record_worktree_merge` as the no-companion fallback); `lifecycle` is the read-only advisor (`get_sprint_quiescence` / `list_worktrees` / `get_worktree`) that reports state and recommends the next lifecycle action. The dogfood walkthrough threading all four is `lumina/docs/runbooks/dogfood-lifecycle.md`.

Because these skills operate on the SPRINT / WORKTREE / WORK-QUEUE substrate rather than a single story field, three §a-§m conventions map differently. This section documents the deltas; everything else (the §e skill=instructions/MCP=execution split, the §f no-per-verb-fragmentation rule) applies unchanged.

### §n.1 Idempotency — TXN-idempotent, NOT supersession-idempotent (delta from §b)

The §b check-before-act + supersession pattern is the idempotency model for the PLANNING blocks: read the field, branch absent/present/match/mismatch, and on a real change prompt the user via the §b-supersession `AskUserQuestion` before writing. The lifecycle/orchestration skills do NOT use that model. Their mutating primitives — `claim_next_task`, `create_sprint`, `add_tasks_to_sprint`, `complete_task`, `record_task_commits` — are **TXN-idempotent at the repo layer**: each runs inside a single `BEGIN IMMEDIATE` transaction (`AnyPool::begin()` issues `BEGIN IMMEDIATE` per `lumina/src/db.rs`) whose own guards make re-execution safe. `claim_next_task` leases the first ready candidate atomically (a re-run claims a DIFFERENT task or returns `{ claimed: null }` — never a double-lease); `add_tasks_to_sprint` collapses an already-attached `(task, sprint)` pair via `ON CONFLICT DO NOTHING`; `record_task_commits` dedups via `UNIQUE(commit_sha, task_id)`; `complete_task` is a re-runnable two-txn sequence (crash-recovery safe on an already-`done` task). So these skills MUST NOT wrap their writes in a §b supersession `AskUserQuestion` — there is no "current value" to supersede and no user-mediated overwrite; the safety lives in the transaction, not in a confirm prompt. (`lifecycle`, being read-only, has no idempotency concern at all.)

### §n.2 Provenance — record against a `sprint_id`, not a `work_item_id` (delta from §c)

§c records exactly one `record_task_activity` entry against the `work_item_id` after every planning-block write. The lifecycle/orchestration skills operate on objects that are NOT work items — a sprint, a worktree, the work-queue — so their provenance lands differently:

- `compose-sprint` and `run-sprint` provenance is **sprint-scoped**: the load-bearing audit is the `events` outbox row each repo mutation emits in its own transaction (e.g. `record_worktree_merge`'s `worktree` aggregate event, `record_task_commits`'s inert `worktree` aggregate event), keyed by `sprint_id` / `worktree_id` rather than `work_item_id`. These skills do NOT synthesise a §c `record_task_activity` entry for the sprint-level act, because `record_task_activity` requires a `work_item_id` and there is no single owning work item for a sprint-scoped action.
- Where a lifecycle skill ALSO touches an individual task as a work item (e.g. `run-sprint` transitioning a specific task via `complete_task`), the per-task `events` row carries that task's provenance natively — the skill still does not add a redundant §c activity entry on top of the queue primitive's own event.
- `create-project` is the one boundary case: it creates work items (`create_work_item` per level), and each `create_work_item` emits its own `+1 work_items / +1 events` row — that IS the provenance. It does not additionally `record_task_activity`, because the creation event already captures the act.

The through-line: planning blocks prove provenance via a §c `record_task_activity` row on the `work_item_id`; lifecycle/orchestration skills rely on the `sprint_id`/`worktree_id`-keyed `events` outbox row that each repo mutation already emits in-transaction.

### §n.3 Frontmatter — the lifecycle mutators are model-invocable too (per §a)

Per §a, no skill in this plugin carries `disable-model-invocation`, and the lifecycle mutators are no exception. The three mutating lifecycle skills (`create-project`, `compose-sprint`, `run-sprint`) mutate not just the lumina DB but the git working tree's worktrees and the live team-execution work queue — a higher blast radius than a planning write — yet they too are model-invocable by design: the autonomous engine and scheduler must be able to drive sprint composition and execution without a human at a terminal. Safety for these higher-blast-radius git/worktree/work-queue ops comes from §n.1 TXN-idempotency (each mutating primitive is safe to re-execute, with no "current value" to supersede) plus deliberate scheduler/operator invocation — NOT from removing the skills from the routing context.

The `lifecycle` skill is read-only (it calls only `get_sprint_quiescence`, `list_worktrees`, `get_worktree` and writes nothing); like every other skill it is model-discoverable, letting an agent auto-find the lifecycle advisor when it needs to reason about sprint/worktree state.

## §o The planning orchestrator (round-5)

Round-5 reshaped `/lumina:plan-story` from a per-block gate-walker into a planning **orchestrator** that holds cross-block judgement as a SINGLE MIND (the role `/plan-new` Phase 6 plays). This section is the shared contract for that orchestrator; `plan-story/SKILL.md` is the consumer and cites §o by reference. The §l.0 six PHASES (frame / explore / decide / verify-design / decompose / closure) stay the planning CORE — the orchestrator WRAPS them in a stage machine that adds the cross-cutting judgement, two concentrated user grills, a curated decision brief, and an epoch-scoped rework loop. The retired per-block `Run / Skip / Inspect / Abort` gate (and its §l.1 skip-override audit) is GONE.

### §o.0 The stage machine

The orchestrator runs SIX stages in order, wrapping the §l.0 phases:

```text
triage → frame → plan → brief → align → rework
```

| Stage | Wraps | What it does |
|-------|-------|--------------|
| `triage` | (pre-frame) | Compute the gating tier (§o.1) + resolve execution mode (§d); branch the whole walk's interaction model. |
| `frame` | §l.0 Phase 1 (frame) | **Gate 1 — the framing grill.** Confirm the story is correctly framed and eligible — or bounce it to backlog. Includes the scope-challenge (split / merge / duplicate). |
| `plan` | §l.0 Phases 2–5 (explore / decide / verify-design / decompose) | AUTO-RUN the four planning-core phases end-to-end with NO per-block ceremony; ONE concentrated mid-flow interrogation only on serious ambiguity (the orchestrator's call). |
| `brief` | (post-decompose) | Render the decision brief (§o.3) from the dossier; record it per epoch. |
| `align` | (closure-adjacent) | **Gate 2 — the direction grill on the brief.** Aligned → walk ends; misaligned → `rework`. MANDATORY in `full`/`light`. |
| `rework` | (epoch-scoped reset) | Bump the plan epoch, diff affected phases, invalidate stale rows, record one rework-audit, re-enter `plan` scoped to the affected phases. Loop until aligned. |

**The two grills.** User interaction concentrates into exactly TWO grills (replacing the ~16× per-block gate that R50 found extracted nothing): the **framing grill** (gate 1, `frame`) and the finding-grounded **direction grill on the decision brief** (gate 2, `align`). Beyond those, a **single mid-plan interrogation** may fire inside `plan` — the orchestrator's judgement call, at most once per `plan` pass, only on genuinely serious ambiguity (e.g. an unresolved high-severity open question, or a `complexity=high` decomposition that needs a steer). It is NOT the retired per-block gate.

**Per-stage §c provenance.** On entering each stage the orchestrator appends exactly ONE `record_task_activity` (`entry_type: "execution"`, `origin: "plan"`, `summary: "plan-story stage: <stage> (gating=<tier>, epoch=<epoch>)"`), applying the §c substitution guard verbatim. This per-stage-transition write is SEPARATE from each dispatched block's own §c (which fires inside the block as `Skill()` runs it per §l.4) and from the single rework-audit activity. Per-block work is still `Skill()`-DISPATCH (§l.4): to "run a block" the orchestrator issues `Skill("lumina:<block>", "$work_item_id")`, and the REAL block runs its own §b check-before-act + §c sequence; forked blocks (`research-explore`, `story-review`, `decompose-tasks`) run their REAL §d fan-out automatically (§l.4(b)).

### §o.1 Gating tiers + `compute_gating_tier`

The orchestrator computes a per-story INTERACTION level — the **gating tier** — that decides how hard to grill the user. The three tiers:

- **`full`** — both grills run LIVE and hard; the mid-plan interrogation may fire; the brief is presented for explicit sign-off.
- **`light`** — both grills run LIVE but lighter (fewer axes, lower bar to proceed); the mid-plan interrogation fires only on genuinely serious ambiguity.
- **`autonomous`** — there is NO live `AskUserQuestion` channel (§d). Every grill DEGRADES to durable lumina `open_questions`: the orchestrator records the framing/alignment decisions as open questions, proceeds on documented defaults, and an operator answers asynchronously — the flow blocks on a RECORDED decision, never hangs (§d autonomous-mode AUQ degradation).

**The rule (single source, server-side — mirrors §k.0 `compute_tier`).** The gating tier is derived server-side by `repo::compute_gating_tier` and surfaced via `mcp__lumina__get_gating_tier`. Skill bodies MUST call the tool and MUST NOT re-derive client-side (§e — the rule is single-source, server-side, just like the §k.0 dispatch-tier rule):

```text
compute_gating_tier(spawned_from_finding, complexity, unresolved_questions, scope_files):
    if spawned_from_finding AND complexity != "high" AND unresolved_questions == 0:   autonomous
    if complexity == "high" OR unresolved_questions > 0 OR scope_files > 6:           full
    else:                                                                             light
```

Note the autonomous branch is tested FIRST and `scope_files` does NOT guard it (User Decision 2): a finding-spawned, non-high-complexity story with zero open questions runs autonomous regardless of file count. `scope_files > 6` only escalates a NON-autonomous story to `full`.

**Gating tier vs dispatch tier — do not conflate.** The gating tier (`full` / `light` / `autonomous`, §o, per-STORY interaction level) is a DIFFERENT concern from the §k dispatch tier (`Lite` / `Deep`, per-TASK agent-routing for `/implement`). They are deliberately NOT unified — the same non-unification stance as `Severity` vs `RiskSeverity` (§k.2). The dispatch tier is surfaced INSIDE the brief's Impact section via `get_task_dispatch_plan`; the gating tier governs the orchestrator's grills.

**User override (interactive mode only).** The user may override the computed tier: a "grill me anyway" intent RAISES to `full`; a "just run it" intent LOWERS toward `light` — but never below the floor a `full`-forcing signal sets (an override cannot drop a `complexity=high` story below `full`; honour the lower request only down to `light`). The override is recorded in the stage-transition body (`override=<from>→<to>`). Autonomous-mode degradation is NOT a tier the user picks — it follows from the execution mode (§d) and never lowers the REQUIRED gating; it only changes the live-vs-durable channel.

### §o.2 Plan epoch + the corrected liveness model

`work_items.plan_epoch` (story-scoped, `NOT NULL DEFAULT 0`; migration 0026) is the rework generation counter. Planning child rows (`research_notes`, `risks`, `rejected_alternatives`, `open_questions`, `acceptance_criteria`, child `tasks`) carry a NULLABLE `plan_epoch` stamp at creation.

**Liveness is the SOLE dossier filter; epoch is provenance + rework-scoping ONLY — NEVER a dossier filter.** This is the corrected model (it resolves an inconsistency in the original draft):

- **LIVENESS** decides what the dossier renders. A row is live ⟺ it is **not superseded / not rejected / not cancelled / not retired** (the exact predicate is PER child-table by its own signal — see §o.4). The dossier returns ONLY live rows.
- **EPOCH** is metadata: it records WHICH rework generation produced a row, and it SCOPES a rework's invalidation pass. It is NEVER used to filter the dossier. A row that SURVIVES a rework keeps its original (older) epoch and stays live — there is NO forced re-stamp. **A surviving live row of ANY epoch still renders.** Invalidated rows drop out of the dossier because they were MARKED stale (superseded / retired / cancelled), NOT because of their epoch.

This is what makes "preserve work without stale noise" sound: surviving live rows of any generation render; only the explicitly invalidated rows are excluded.

### §o.3 The decision brief (five sections) + dossier-first reads

**Dossier-first reads (A.7).** Every orchestrator-driven block reads `mcp__lumina__get_story_dossier` FIRST for full-story context — the whole picture (problem, approach, live research, live risks, the persisted `task_research_links`, the file footprint, the dispatch shape, readiness) rather than a bag of disconnected fields. This is what lets a block re-run mid-walk, or weeks later, with the same context the orchestrator has. `StoryDossier` is DERIVED (no new table — it composes `WorkItemDetail` + per-task `task_research_links` + `story_files_footprint` + dispatch-plan shape + readiness) and is LIVENESS-FILTERED per §o.2.

**The brief** (`brief` stage) is a curated, presentation-only artifact composed FROM the dossier (NOT a raw story dump), with EXACTLY these FIVE sections (the orchestrator renders the headings verbatim so the shape is stable and grep-checkable):

1. **Problem** — `problem_statement` + what we are explicitly NOT doing (`not_doing`).
2. **Chosen approach** — `execution_strategy` (the approach-tournament winner) AND **the competition**: each `rejected_alternative` the tournament produced, with its score + rationale, so the user sees the options weighed and WHY this one won.
3. **Impact** — the blast radius: `story_files_footprint` (the deduped file set), the `get_task_dispatch_plan` parallelism shape (e.g. "3 batches; max 4 parallel; 2 deep / 6 lite"), and the open risks SEVERITY-SORTED (critical → low).
4. **Grounding** — each task with its `task_research_links` notes (e.g. "T4 implements R-note 'pinia-ssr-hydration'"). This is the PERSISTED answer to R52 — grounding the user can audit, drawn from the live links the dossier folds, never from a dead/superseded note.
5. **Alignment questions** — the finding-grounded questions the orchestrator wants confirmed BEFORE committing, each citing the specific finding / brief element it tests (the `/plan-new` Phase-4 finding-grounded style, not generic prompts).

The rendered brief text + the `align` outcome are recorded PER EPOCH (stamped with the current `plan_epoch`, via `record_task_activity` and/or an epoch-keyed story attribute) for audit and resume.

### §o.4 The rework contract

On a misaligned `align` grill, the orchestrator applies the rework contract, then re-enters `plan` scoped to the affected phases. Steps:

1. **Bump the epoch.** `mcp__lumina__bump_plan_epoch({ story_id })` → `plan_epoch += 1`. (Records an EXPORT-INERT event — see §o.5.)
2. **Diff which phases the directive touches** → set `reset_kind` + `affected_phases`:
   - **scope / problem** disagreement → FULL reset: re-enter at `frame` (`reset_kind="full"`; affected = frame + explore + decide + verify-design + decompose).
   - **approach** disagreement → re-enter at Decide (`reset_kind="partial"`; affected = decide + verify-design + decompose).
   - **decomposition** complaint → re-enter at Decompose (`reset_kind="partial"`; affected = decompose).
3. **Invalidate the affected blocks' stale rows** — PER child-table by its OWN liveness signal (there is NO single uniform predicate):
   - research notes → `mcp__lumina__supersede_research_note`
   - risks → `mcp__lumina__supersede_risk`
   - rejected alternatives → `mcp__lumina__supersede_rejected_alternative`
   - findings → `mcp__lumina__supersede_finding`
   - stale NOT-STARTED tasks → flip to `cancelled` (R28 path, via `transition_status` / `update_work_item`); the dossier excludes cancelled tasks and their grounding links.
   - stale OPEN QUESTIONS → `mcp__lumina__retire_open_question({ id })` (sets `open_questions.retired_at`; the dossier filters on `retired_at IS NULL` AND the pre-existing `status != 'cancelled'` — a `resolve_open_question`-cancelled question keeps `retired_at` NULL, so the column alone is insufficient).
   - stale ACCEPTANCE CRITERIA → `mcp__lumina__remove_acceptance_criterion` under a CONFIRM (in autonomous mode the confirm degrades to a durable `open_question` per §d before deleting).
4. **One rework-audit activity** (`record_task_activity`, §c substitution guard applied) capturing `{from_epoch, to_epoch, reset_kind, affected_phases, superseded_ids, retired_ids}` in the summary/body. This is the durable supersession trace that REPLACES the retired §l.1 skip-override audit.
5. **Re-enter `plan`** scoped to `affected_phases` (a full reset re-enters at `frame` first), then `brief` → `align` again. Loop until aligned.

**The AC hard-delete exception.** `acceptance_criteria` has NO liveness column and NO `supersede_*`/`update_*`-to-superseded companion — so a removed AC is simply ABSENT (it carries no supersede provenance). This makes ACs the ONE hard-delete exception in the rework contract; every other invalidated row keeps a supersede/retire/cancel trail. Correspondingly, the AC `plan_epoch` stamp annotates LIVE rows only (a hard-deleted AC leaves no row to stamp). The destructive supersession itself follows the §b-supersession-destructive confirm protocol.

### §o.5 The export-inert epoch trade-off (deliberate)

`bump_plan_epoch` mutates a `work_items` COLUMN (`plan_epoch`) but records an EXPORT-INERT event — a `plan_epoch`-aggregate `events` row, NOT a `work_item`-aggregate row — so it does NOT re-render the work_item's git-export snapshot. Consequence: the exported snapshot's `plan_epoch` may LAG the DB until the next `work_item`-aggregate event fires, at which point the snapshot SELF-HEALS to the current value. This is INTENTIONAL and mirrors the `task_files` / `worktree` inert-event precedent (lumina/CLAUDE.md): `plan_epoch` is internal planning metadata, not part of the exported audit semantics, so paying a full work_item re-export on every epoch bump is unwarranted. The `link_task_research` / `retire_open_question` rework writes are inert by the same reasoning. (This preserves the +1 work_items / +1 events single-mutation-path invariant — each still emits exactly one event; it is just a non-`work_item` aggregate.)

### §o.6 The devil's-advocate mandate (R51)

R51 found planning is structurally biased toward CONFIRMATION and SCOPE-CONSERVATISM — it never challenges its own framing. The orchestrator embeds a devil's-advocate counterweight at four points, each owned by a dispatched block:

- **Approach tournament** (`approach`, §o `plan`/Phase 3): draft ≥2 distinct approaches, score each (consistency / complexity-risk / parallelism / reversibility), present the COMPETITION, write the winner as `execution_strategy` AND auto-populate the losers into `rejected_alternatives` with scores + rationale — feeding the brief's Chosen-approach section directly. The zero-accepted-notes hard-fail is RELAXED to a warning so the tournament's divergent thinking can run from the dossier even when the vetted-research funnel is sparse.
- **The `contrarian` lens** (`research-explore`, §k.1's new always-on 6th lens): a dedicated disconfirmation agent that seeks evidence the chosen direction is WRONG and surfaces competing patterns the four confirmatory lenses are biased against producing.
- **Framing scope-challenge** (`user-interrogation` / `problem-statement` in `frame`): the framing grill PRESSES on scope — "should this story be SPLIT, or made BIGGER / merged with a sibling?" — precisely because R51 found planning never asks it.
- **Sharpened story-review** (`story-review` in verify-design): rubric categories that ARGUE AGAINST the plan (steelman the rejected alternatives) and that flag SCOPE CONSERVATISM as a finding.

The orchestrator's job is to ensure these run (they are dispatched blocks per §l.4) and to surface their output in the brief's Chosen-approach competition and Alignment questions — not to re-implement the tournament/review logic (§e Sentry split).

### §o.7 Tools

Round-5 added five MCP tools the orchestrator drives (surface bumped 94 → 99): `bump_plan_epoch`, `link_task_research`, `retire_open_question` (writes); `get_story_dossier`, `get_gating_tier` (reads). `unlink_task_research` is repo-internal (used by the rework/cancel path) and NOT MCP-surfaced. See the MCP catalogue [`./skills/mcp/SKILL.md`](./skills/mcp/SKILL.md) for canonical argument shapes.

---

> **§-letter allocation history**: §a-§h were the round-1 (`lumina-story-planning-workflow`) allocation. §i and §j were added by round-2 (`lumina-story-planning-round-2`). §k and §l were added by round-3 (`lumina-story-planning-round-3`). §m was added by the migration-0010 epic/focus-semantics pass. §n was added by the migration-0016 sprint-lifecycle/worktree-ownership pass (the lifecycle/orchestration skill category). §o was added by round-5 (`lumina-story-planning-round-5`) — the planning-orchestrator reshape. Future rounds should append §p, §q, … rather than re-using a freed letter; reserved letters protect cross-references in skill bodies that cite by short letter.

Pointer back: the plugin entry point is `claude/plugins/lumina-story-blocks/README.md`; the parent plan is `docs/plans/lumina-story-planning-workflow.md` (round 1), `docs/plans/lumina-story-planning-round-2.md` (round 2), `docs/plans/lumina-story-planning-round-3.md` (round 3), and `docs/plans/lumina-story-planning-round-5.md` (round 5 — the planning orchestrator, §o + the §k.1 six-lens amendment).
