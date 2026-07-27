---
name: not-doing
description: Capture or supersede a story's "Not Included" scope boundary as a free-text attributes.not_doing entry.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:not-doing`

Writes the story's "Not Included" scope boundary — the Augment-Code-style micro-spec field recording what the story is explicitly NOT trying to do (parent plan finding R17). The value rides `work_items.attributes.not_doing` per the §g.1 registry; there is deliberately no standalone `set_not_doing` tool. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e/§f, plus §g.1 (the `attributes.not_doing` binding and its promotion policy).

```
mcp__lumina__set_story_plan {
  id: "$work_item_id",
  not_doing: "<user text>"
}
```

**Pass ONLY the `not_doing` key.** `set_story_plan` merges server-side (`repo::set_work_item_attributes` runs a Rust-side patch + per-kind validator, preserving siblings such as `problem_statement` and `execution_strategy`). Do NOT read `detail.attributes`, hand-merge, and write the blob back via raw `update_work_item` — that shadows the merge semantics, races concurrent writes to other keys, and hits the column-level COALESCE clobber that round-2 routed around by widening `SetStoryPlanParams`. `update_work_item` with raw `attributes` payloads is forbidden plugin-wide.

## Skill-specific parts of the §b sequence

Run §b over `detail.attributes.not_doing`, with:

- **Kind-precondition** — story-only (§e exception). On any other kind, abort before any write: `not-doing: requires kind='story' (got kind='<actual>'). attributes.not_doing rides set_story_plan; the underlying MCP tool will reject non-story writes.`
- **The prompt** — `What is explicitly NOT being done in this story? (Augment-Code-style 'Not Included' scope boundary — see parent plan finding R17.) Free text; one or two paragraphs typical.` Trim the response.
- **No-op line** — compare `<new_text>.trim() == <existing>.trim()`, then return `not_doing already matches the value you provided — no change.`
- **Supersede substitutions** — `<field-name>` → `not_doing`; `<current-value-summary>` → the existing text truncated to ~80 chars with an ellipsis (e.g. `Not handling OAuth — see follow-up plan; also not migrating legacy…`).
- **§c summary line** — `not-doing: set attributes.not_doing on <work_item_id>` (first write); `not-doing: superseded attributes.not_doing on <work_item_id>` (supersession). `entry_type` stays `"execution"` — the §c vet exception is reserved for `vet-research`.
