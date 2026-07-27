---
name: verification-commands
description: Capture or update a story's verification commands (build/test/lint/smoke) used by /implement and /tdd.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:verification-commands`

Capture or update a story's `attributes.verification_commands` object via `mcp__lumina__set_story_plan`: prompt per key for the four canonical commands, then write the rebuilt object through lumina's widened story-plan setter. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, with §b applied per-element (each key is one element, per §b-per-element), plus §g.1 (the `attributes.verification_commands` row) and §h (story-only).

```
mcp__lumina__set_story_plan {
  id: "$work_item_id",
  verification_commands: { build: <…>, test: <…>, lint: <…>, smoke: <…> }
}
```

**Merge semantics — read this before writing.** `SetStoryPlanParams.verification_commands` is "set-or-leave at the key level, NOT a deep merge of the sub-object": passing the field REPLACES the whole object. So read the current object FIRST and rebuild the full four-key blob (current values for `Keep` keys, new values for `Set new` keys) before writing. Sibling story-plan keys (`problem_statement`, `not_doing`, …) are unaffected because they are absent from the call. Server-side, the round-2 `VerificationCommands` struct validates the shape and rejects unknown sub-keys — do not validate sub-key names in the skill body, and never reach for `update_work_item` / `set_work_item_attributes`.

**Clear-key limitation (per §g.1)**: there is no way to delete an individual key — the MCP layer normalises null sub-keys out of the patch, leaving the existing value intact. This skill therefore offers only `Keep` and `Set new` per key; clearing a previously-set command requires a direct DB edit and is out of scope.

## The four keys

1. **`build`** — the canonical build command (e.g. `cargo build --workspace --manifest-path lumina/Cargo.toml`).
2. **`test`** — the canonical test command (e.g. `cargo nextest run --manifest-path lumina/Cargo.toml`).
3. **`lint`** — the canonical lint command (e.g. `cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets`).
4. **`smoke`** — an optional one-line smoke check (e.g. `cargo run -- --help`); the only intentionally-optional key.

## Skill-specific parts of the §b sequence

- **Kind-precondition (§h)** — story-only. On any other kind, abort before any write: `verification-commands requires a story work item; got kind=<kind>.`
- **Surface current state** — after the read, show a 4-line summary, e.g. `build: cargo build … | test: (unset) | lint: cargo clippy … | smoke: (unset)`.
- **Per-key prompt** — one `AskUserQuestion` call carrying all four questions (the harness allows up to 4 per call; if that limit changes, fall back to four sequential calls). Per key: header `Verification command: <key>`; body `Current: <current value or "(unset)">`; exactly 2 options — `Keep` (leave the existing value untouched) and `Set new` (provide a new command via the `Other` free-text channel).
- **Per-key supersession** — for every key where the user picked `Set new` AND the current value is non-empty, run the verbatim §b-supersession template BEFORE committing that key's change, substituting `<field-name>` → `verification_commands.<key>` and `<current-value-summary>` → the first ~80 characters of the existing command on one line. On `Keep current`, revert that key's decision to `Keep`; on `Replace`, retain the new value. Per-key scope means the user may replace `build` while keeping `test` untouched in one invocation.
- **No-op line** — if every key was `Keep`: `verification_commands already matches the value you provided — no change.`
- **§c summary line** — `verification-commands: set keys <comma-separated changed keys> on <work_item_id>` (e.g. `verification-commands: set keys build, test on story:abc123`). Use `set` for first-create (no prior value at any key) and `superseded` when at least one changed key had a non-empty prior value the user chose to `Replace`. One entry per invocation regardless of key count — the write here is one `set_story_plan` call.

Write step:

```
current = detail.attributes.verification_commands or {}
changed_keys = []
out = {}
for key in ["build", "test", "lint", "smoke"]:
    if user_decision[key] == "Set new":
        out[key] = new_values[key]
        changed_keys.append(key)
    elif current.get(key) is not None:
        out[key] = current[key]   # carry forward so the top-level set does not drop it

if not changed_keys:
    return "verification_commands already matches the value you provided — no change."

mcp__lumina__set_story_plan({id: $work_item_id, verification_commands: out})
mcp__lumina__record_task_activity({...})   # per §c
return f"verification_commands updated: {', '.join(changed_keys)}."
```
