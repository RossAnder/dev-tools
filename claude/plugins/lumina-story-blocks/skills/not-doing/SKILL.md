---
name: not-doing
description: Capture or supersede a story's "Not Included" scope boundary as a free-text attributes.not_doing entry.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:not-doing`

Writes the story's "Not Included" scope boundary — the Augment-Code-style micro-spec field that records what the story is explicitly NOT trying to do (parent plan finding R17). The value rides `work_items.attributes.not_doing`; round-2 reactivated this skill by widening `mcp__lumina__set_story_plan` to accept `not_doing` so the merge-safe path is used (no more column-level COALESCE clobber).

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter), §b (5-step check-before-act), §b-supersession (verbatim `AskUserQuestion` supersede prompt), §c (provenance via `record_task_activity`; `entry_type: "execution"` — vet exception does NOT apply), §e (Sentry pattern — critical here; see below), §f (no per-verb fragmentation), §g.1 (attributes-key registry — the binding for `attributes.not_doing`).

## Attribute-key convention (per §g.1)

`not_doing` has no first-class column on `work_items`. It rides the existing `work_items.attributes` JSON-merge semantics under the named key `attributes.not_doing`. The single source of truth for this binding is [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §g.1 — a future lumina migration may promote this key to a first-class column, at which point this skill body updates in lockstep per §g.1's promotion policy. There is intentionally no standalone `set_not_doing` MCP tool; `set_story_plan`'s widened-params form is the entry point.

The skill body DOES impose a kind-precondition: `attributes.not_doing` is a story-meta key that rides `set_story_plan`, and `set_story_plan` itself fails on non-story kinds at the MCP layer. The skill performs the §e-blessed local check at step 2 so the user sees a friendly early-abort message rather than a server-side validation error.

## MCP tool

```
mcp__lumina__set_story_plan {
  id: "$work_item_id",
  not_doing: "<user text>"
}
```

`set_story_plan` performs the merge in lumina (`repo::set_work_item_attributes` runs a Rust-side patch + per-kind validator, preserving sibling keys such as `problem_statement` and `execution_strategy`). The skill MUST pass ONLY the `not_doing` key — do NOT read-modify-write the full attributes blob from the skill body.

## Body — 5-step check-before-act (per §b)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.attributes.not_doing` (may be null / absent).
2. **Kind-precondition** (per §e exception): verify `detail.kind == "story"`. On non-story, abort loud with: `"not-doing: requires kind='story' (got kind='<actual>'). attributes.not_doing rides set_story_plan; the underlying MCP tool will reject non-story writes."`
3. **Absent → create**: if `detail.attributes.not_doing` is null / absent, prompt the user (via `AskUserQuestion` or a direct free-text request) with: `"What is explicitly NOT being done in this story? (Augment-Code-style 'Not Included' scope boundary — see parent plan finding R17.) Free text; one or two paragraphs typical."` Trim the response. Call `set_story_plan({id: $work_item_id, not_doing: <trimmed_text>})`, then record provenance per §c, then return.
4. **Present and matches**: after asking the user for the new value, if `<new_text>.trim() == <existing>.trim()` (string equality after trim), return the §b step-4 confirmation: `"not_doing already matches the value you provided — no change."`
5. **Present and differs → supersede**: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `not_doing`
   - `<current-value-summary>` → the existing `attributes.not_doing` text, truncated to ~80 chars with an ellipsis if longer (e.g. `"Not handling OAuth — see follow-up plan; also not migrating legacy…"`).

   On `Replace`, call `set_story_plan({id: $work_item_id, not_doing: <new_text>})`, then record provenance per §c. On `Keep current`, abort the invocation without writing.

## Sentry-pattern compliance — CRITICAL (per §e)

The skill body MUST pass ONLY the `not_doing` key inside the `set_story_plan` params:

```
set_story_plan { id: $work_item_id, not_doing: <text> }                  // ✓ correct
```

DO NOT read all of `detail.attributes`, hand-merge the JSON, and write the full blob back via raw `update_work_item`:

```
// ✗ DO NOT do this — it shadows lumina's merge semantics AND triggers the column-level COALESCE bug:
attrs = detail.attributes
attrs.not_doing = <text>
update_work_item { id: $work_item_id, attributes: attrs }
```

`set_story_plan` reads the existing attributes server-side, merges the present `not_doing` key, leaves absent keys (`problem_statement`, `execution_strategy`, `verification_commands`, …) intact, runs the write in one transaction, and emits the event. Passing only the `not_doing` key is sufficient and is the only correct shape — hand-merging in the skill body would race against concurrent writes to other attribute keys and defeat the merge semantics that lumina (`repo.rs`) is responsible for.

Round-2 reactivation note: the previous warn-and-block banner cited R1/R2 from the round-1 review ledger — `update_work_item.attributes` performed column-level COALESCE that clobbered sibling keys. Round-2 closed this gap by widening `SetStoryPlanParams` to accept `not_doing`; this skill now routes through the merge-safe `set_story_plan` path. `update_work_item` with raw `attributes` payloads remains forbidden across the plugin.

## Provenance recording (per §c)

After any successful write the skill MUST append one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape. `entry_type` is `"execution"` (the §c vet exception is reserved for `vet-research` only).

Summary line: `"not-doing: set attributes.not_doing on <work_item_id>"` for step 3 (first-create); `"not-doing: superseded attributes.not_doing on <work_item_id>"` for step 5 (supersession).
