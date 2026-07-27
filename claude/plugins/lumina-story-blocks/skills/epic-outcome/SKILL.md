---
name: epic-outcome
description: Capture or update an epic's outcome (what closing it delivers, who benefits, the observable signal it's achieved).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:epic-outcome`

Capture or update an epic's `attributes.outcome` via `mcp__lumina__set_epic_plan`: prompt along three axes, assemble the answers into one outcome string, and write it through lumina's merge-call epic-plan setter. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, plus §m.2 for the epic-only kind-precondition (§m.2 is the on-point authority — §g/§h's taxonomy has no epic/focus category).

```
mcp__lumina__set_epic_plan {
  id: "$work_item_id",
  outcome: "<assembled 3-axis text>"
}
```

`set_epic_plan` is a merge call across `outcome` / `context`: pass ONLY `outcome` and `context` stays untouched. Never read `context` back to "preserve" it.

## The 3-axis prompt

Three questions in one `AskUserQuestion` call (one per axis, each with an `Other` free-text option). These are the epic-OUTCOME adaptation of the `problem-statement` interrogation:

1. **What does closing this epic deliver?** — 1-2 sentences naming the concrete deliverable or intent that "epic done" represents (the end-state capability, not the tasks that build it).
2. **Why does it matter / who benefits?** — 1 sentence naming the audience and the value (end user / maintainer / external integrator / downstream consumer / etc.) that closing the epic unlocks.
3. **What observable signal means it's achieved?** — 1-2 sentences naming the concrete, observable outcome that indicates the epic is genuinely done (the thing you could point at to say "this is finished").

Assemble the answers into this stable three-paragraph layout, so re-runs are byte-stable on the same answers and §b step 4's equality check has a stable string to compare:

```
Delivers: <answer 1>

Why it matters: <answer 2>

Observable signal: <answer 3>
```

## Skill-specific parts of the §b sequence

Run §b over `detail.attributes.outcome`, with:

- **Kind-precondition** — epic-only (§m.2, under §e's blessed local kind-check). On any other kind, abort before any write: `epic-outcome requires an epic work item; got kind=<kind>.`
- **First write** — after assembling, return `outcome created on <work_item_id>.`
- **No-op line** — `outcome already matches the value you provided — no change.` (run the prompt and assemble first, then compare byte-for-byte).
- **Supersede substitutions** — `<field-name>` → `outcome`; `<current-value-summary>` → the first ~80 characters of the existing outcome + `…` (single-line; collapse embedded newlines to spaces before truncating).
- **§c summary line** — `epic-outcome: set on <work_item_id>` (first write); `epic-outcome: superseded on <work_item_id>` (supersession, only when the prior value was non-null and the user chose `Replace`). `<work_item_id>` is the literal id.
