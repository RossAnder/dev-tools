---
name: not-doing
description: Capture or supersede a story's "Not Included" scope boundary as a free-text attributes.not_doing entry.
arguments: [work_item_id]
disable-model-invocation: true
---

> **⚠ DISABLED — DO NOT INVOKE**
>
> Invoking this skill on a story that already has `attributes.problem_statement` or `attributes.execution_strategy` set will **silently destroy those siblings**. The MCP tool this skill calls (`mcp__lumina__update_work_item`) performs column-level COALESCE on the `attributes` column rather than per-key JSON merge — verified against `lumina/src/repo.rs:1404-1421`. The safe merge primitive (`set_work_item_attributes` at `repo.rs:1524`) is not exposed as a standalone MCP tool; only `set_story_plan` and `set_task_spec` compose on it.
>
> This skill is disabled pending one of:
> - Adding a dedicated `set_work_item_attributes` MCP tool to lumina that routes through the merge primitive, OR
> - Widening `SetStoryPlanParams` to accept `not_doing` so `set_story_plan` becomes the entry point (one-line repo change).
>
> Tracked as **R1** / **R2** in `.claude/flows/lumina-story-planning-workflow/review-ledger.toml`.

# `lumina:not-doing`

Wraps `mcp__lumina__update_work_item` to write the story's "Not Included" scope boundary — the Augment-Code-style micro-spec field that records what the story is explicitly NOT trying to do (parent plan finding R17).

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter), §b (5-step check-before-act), §b-supersession (verbatim `AskUserQuestion` supersede prompt), §c (provenance via `record_task_activity`), §e (Sentry pattern — critical here; see below), and §g (lens conventions registry).

## Lens convention (per §g)

`not_doing` has no first-class column on `work_items`. It rides the existing `work_items.attributes` JSON-merge semantics under the named key `attributes.not_doing`. The single source of truth for this binding is [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §g (lens conventions registry) — a future lumina migration may promote this key to a first-class column, at which point this skill body updates in lockstep per §g's promotion policy. There is intentionally no `set_not_doing` MCP tool.

The skill body does NOT impose a kind-precondition. `work_items.attributes` exists on every kind; stories are the typical target (per Augment Code's micro-spec pattern) but a feature- or epic-level "not doing" is also coherent — the user picks the granularity.

## MCP tool

```
mcp__lumina__update_work_item {
  id: "$work_item_id",
  attributes: { not_doing: "<user text>" }
}
```

## Body — 5-step check-before-act (per §b)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.attributes.not_doing` (may be null / absent).
2. **No kind-precondition**: as noted above, every kind can carry `attributes`, so step 2 is a no-op for this skill.
3. **DISABLED — Absent → would-create**: abort with the one-line error: `"not-doing: skill is disabled — invoking would clobber sibling attributes on this story. See R1 in .claude/flows/lumina-story-planning-workflow/review-ledger.toml. Do not invoke until a safe attributes-merge MCP tool ships."` Do NOT call `mcp__lumina__update_work_item`. The original (intended-when-re-enabled) behaviour was: if `detail.attributes.not_doing` is null / absent, prompt the user (via `AskUserQuestion` or a direct free-text request) with: `"What is explicitly NOT being done in this story? (Augment-Code-style 'Not Included' scope boundary — see parent plan finding R17.) Free text; one or two paragraphs typical."` Trim the response. Call `update_work_item({id: $work_item_id, attributes: {not_doing: <trimmed_text>}})`, then record provenance per §c, then return.
4. **Present and matches**: after asking the user for the new value, if `<new_text>.trim() == <existing>.trim()` (string equality after trim), return the §b step-4 confirmation: `"not_doing already matches the value you provided — no change."`
5. **DISABLED — Present and differs**: abort with the one-line error: `"not-doing: skill is disabled — invoking would clobber sibling attributes on this story. See R1 in .claude/flows/lumina-story-planning-workflow/review-ledger.toml. Do not invoke until a safe attributes-merge MCP tool ships."` Do NOT call `mcp__lumina__update_work_item`. The original (intended-when-re-enabled) behaviour was: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `not_doing`
   - `<current-value-summary>` → the existing `attributes.not_doing` text, truncated to ~80 chars with an ellipsis if longer (e.g. `"Not handling OAuth — see follow-up plan; also not migrating legacy…"`).
   On `Replace`, call `update_work_item({id: $work_item_id, attributes: {not_doing: <new_text>}})`, then record provenance per §c. On `Keep current`, abort the invocation without writing.

## Sentry-pattern compliance — CRITICAL (per §e)

**Current state**: this skill is DISABLED (see the banner at the top of this file). The §b step 3 and step 5 paths abort with a fail-loud error BEFORE invoking `mcp__lumina__update_work_item`, because that MCP tool performs column-level COALESCE on `attributes` rather than per-key JSON merge (`lumina/src/repo.rs:1404-1421`) and would silently destroy sibling keys such as `problem_statement` and `execution_strategy`. The §e compliance discussion below documents the INTENT for when a safe attributes-merge MCP tool ships and the skill is re-enabled.

The skill body MUST pass ONLY the `not_doing` key inside `attributes`:

```
update_work_item { id: $work_item_id, attributes: { not_doing: <text> } }     // ✓ correct
```

DO NOT read all of `detail.attributes`, hand-merge the JSON, and write the full blob back:

```
// ✗ DO NOT do this — it shadows lumina's merge semantics:
attrs = detail.attributes
attrs.not_doing = <text>
update_work_item { id: $work_item_id, attributes: attrs }
```

Lumina's `update_work_item` performs read-modify-merge inside one transaction: PRESENT keys overwrite, ABSENT keys are left intact. Passing only the `not_doing` key is sufficient and is the only correct shape — hand-merging in the skill body would race against concurrent writes to other attribute keys and defeat the merge semantics that lumina (`repo.rs`) is responsible for.

## Provenance recording (per §c)

After any successful write, append one activity entry:

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "not-doing: set attributes.not_doing on <work_item_id>",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

For step-5 supersession use `"not-doing: superseded attributes.not_doing on <work_item_id>"` instead.
