---
name: focus-framing
description: Capture or update a focus's framing (what's in-scope and out-of-scope for this focus).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:focus-framing`

Capture or update a focus's `attributes.framing` via `mcp__lumina__set_focus_plan`: elicit in-scope / out-of-scope prose, assemble it into one framing string, and write it through lumina's merge-call focus-plan setter. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, plus §m.2 for the focus-only kind-precondition (§m.2 is the on-point authority — §g/§h's taxonomy has no epic/focus category).

```
mcp__lumina__set_focus_plan {
  id: "$work_item_id",
  framing: "<assembled in-scope / out-of-scope text>"
}
```

`set_focus_plan` is a merge call: pass ONLY `framing` and sibling focus-plan fields stay untouched. Never read them back to "preserve" them.

## The framing prompt

Two questions in one `AskUserQuestion` call, each with an `Other` free-text option so the user can type a substantive paragraph:

1. **What's in-scope for this focus?** — 1-3 sentences naming the capabilities/areas this focus owns and will deliver.
2. **What's out-of-scope?** — 1-3 sentences naming the adjacent work this focus explicitly does NOT cover (deferred to other focuses, or out of the epic entirely), so the boundary is unambiguous.

Assemble the answers into this stable two-paragraph layout, so re-runs are byte-stable on the same answers and §b step 4's equality check has a stable string to compare:

```
In-scope: <answer 1>

Out-of-scope: <answer 2>
```

## Skill-specific parts of the §b sequence

Run §b over `detail.attributes.framing`, with:

- **Kind-precondition** — focus-only (§m.2, under §e's blessed local kind-check). On any other kind, abort before any write: `focus-framing requires a focus work item; got kind=<kind>.`
- **First write** — after assembling, return `framing created on <work_item_id>.`
- **No-op line** — `framing already matches the value you provided — no change.` (run the prompt and assemble first, then compare byte-for-byte).
- **Supersede substitutions** — `<field-name>` → `framing`; `<current-value-summary>` → the first ~80 characters of the existing framing + `…` (single-line; collapse embedded newlines to spaces before truncating).
- **§c summary line** — `focus-framing: set on <work_item_id>` (first write); `focus-framing: superseded on <work_item_id>` (supersession, only when the prior value was non-null and the user chose `Replace`). `<work_item_id>` is the literal id.
