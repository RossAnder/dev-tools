---
name: closure-gate
description: Set or supersede a story's closure_gate (hard / soft), controlling how unchecked acceptance criteria block child-task →done transitions.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:closure-gate`

Thin wrapper over `mcp__lumina__set_closure_gate`, kept for UI parity so the eventual lumina web UI drives the same skill the user invokes from `/lumina:closure-gate <id>`. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e/§f.

```
mcp__lumina__set_closure_gate {
  id: "$work_item_id",
  closure_gate: "hard" | "soft"
}
```

## What the user is choosing

- **`hard`** — each child task's →done transition is rejected while THAT task still has any unchecked acceptance criterion. The gate is the parent story's, applied per-task: checking a sibling task's AC does not help.
- **`soft`** — the transition is allowed, but the response flags the unchecked count for visibility. The story can close with partially-unchecked ACs.

Surface this distinction in the option labels so the user picks meaningfully rather than guessing.

## Skill-specific parts of the §b sequence

Run §b over `detail.closure_gate`, with:

- **Kind-precondition** — story-only. On any other kind, abort before any write: `set_closure_gate is story-only — got kind=<kind>.`
- **The enum prompt** — one `AskUserQuestion`, two options: `hard` → "block each child task's →done while it has unchecked ACs"; `soft` → "allow →done, but flag the unchecked AC count".
- **No-op line** — `closure_gate already set to <value> — no change.`
- **Supersede substitutions** — `<field-name>` → `closure_gate`; `<current-value-summary>` → the existing value (e.g. `hard`).
- **§c summary line** — `closure-gate: set <work_item_id> to <hard|soft>` (first write); `closure-gate: superseded <work_item_id> from <old> to <new>` (supersession).
- Do NOT model the `hard` task-blocking behaviour in the skill body — it lives in lumina's task-transition logic (`repo.rs`'s `transition_status` path).
