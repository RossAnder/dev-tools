---
name: focus-shape
description: Set or supersede a focus's shape (vertical-slice / cross-cutting / foundational).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:focus-shape`

Thin wrapper over `mcp__lumina__set_shape`: sets a focus's `shape` discriminator so downstream planning and the lumina web UI drive the same skill the user invokes from `/lumina:focus-shape <id>`. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e/§f, plus §m.2 for the focus-only kind-precondition (§m.2 is the on-point authority — §g/§h's taxonomy splits skills into any-kind lens writers and story-only column writers and has no epic/focus category).

```
mcp__lumina__set_shape {
  id: "$work_item_id",
  shape: "vertical-slice" | "cross-cutting" | "foundational"
}
```

## The three shapes

Surface these glosses in the option labels so the user picks meaningfully rather than guessing:

- **`vertical-slice`** — a coherent end-to-end thread of user-facing value through the layers (one narrow capability, top to bottom).
- **`cross-cutting`** — a concern spanning many areas (a change that threads through the codebase rather than living in one slice).
- **`foundational`** — the base layer other focuses' stories depend on (a structural cross-focus-dependency test — not a leftover / "didn't fit the others" bin).

## Skill-specific parts of the §b sequence

Run §b over `detail.shape`, with:

- **Kind-precondition** — focus-only (§m.2, under §e's blessed local kind-check). On any other kind, abort before any write: `set_shape is focus-only — got kind=<kind>.`
- **The enum prompt** — one `AskUserQuestion`, three options, each labelled with its gloss above.
- **No-op line** — `shape already matches the value you provided — no change.`
- **Supersede substitutions** — `<field-name>` → `shape`; `<current-value-summary>` → the existing value (e.g. `vertical-slice`).
- **§c summary line** — `focus-shape: set <work_item_id> to <shape>` (first write); `focus-shape: superseded <work_item_id> from <old> to <new>` (supersession). `<work_item_id>` is the literal id.
- Do NOT read all of `attributes` and rewrite it as a JSON merge — `set_shape` writes a dedicated column, not an attribute key.
