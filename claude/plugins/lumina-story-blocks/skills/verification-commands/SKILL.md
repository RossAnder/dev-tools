---
name: verification-commands
description: Capture or update a story's verification commands (build/test/lint/smoke) used by /implement and /tdd.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:verification-commands`

Capture or update a story's `attributes.verification_commands` object via `mcp__lumina__set_story_plan`. The skill prompts per key for the four canonical commands (`build` / `test` / `lint` / `smoke`), then writes the merged object through lumina's widened story-plan setter.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, §b-supersession verbatim phrasing, §b-per-element scope — each key is one element), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution), §g.1 (the `attributes.verification_commands` row), §h (kind-precondition signpost — story-only).

## Target

`set_story_plan` accepts only `kind = story`. Per §g.1 the `attributes.verification_commands` JSON object holds `{build, test, lint, smoke}`, each a server-validated `Option<String>` (the round-2 `VerificationCommands` struct).

**Merge-semantics note**: the doc comment on `SetStoryPlanParams.verification_commands` reads "set-or-leave at the key level, NOT a deep merge of the sub-object" — passing the field REPLACES the whole object, it does NOT per-sub-key merge. The skill therefore reads the current object FIRST and rebuilds the full four-key blob (current values for `Keep` keys, new values for `Set new` keys) before writing. Sibling story-plan keys (`problem_statement`, `not_doing`, …) are unaffected because they are absent from the call.

**Clear-key limitation (per §g.1)**: there is no way to delete an individual key via `set_story_plan`. The MCP layer normalises null sub-keys out of the patch, leaving the existing value intact. This skill therefore offers only `Keep` and `Set new` per key — clearing a previously-set command requires a direct DB edit and is out of scope.

## MCP tool

```
mcp__lumina__set_story_plan {
  id: "$work_item_id",
  verification_commands: { build: <…>, test: <…>, lint: <…>, smoke: <…> }
}
```

## The 4-key prompt (build / test / lint / smoke)

The four keys map to the canonical commands a verifier runs against a story's slice (matching the plan-file `## Verification Commands` block and `/test-bootstrap`'s output):

1. **`build`** — the canonical build command (e.g. `cargo build --manifest-path lumina/Cargo.toml`).
2. **`test`** — the canonical test command (e.g. `cargo nextest run --manifest-path lumina/Cargo.toml`).
3. **`lint`** — the canonical lint command (e.g. `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`).
4. **`smoke`** — an optional one-line smoke check (e.g. `cargo run -- --help`); the only key that is intentionally optional.

## Body — 5-step check-before-act (per §b, §b-per-element across the four keys)

**Precondition (§h, story-only)**: after step 1's `get_work_item` returns, verify `detail.kind == "story"`. If not, abort with: `"verification-commands requires a story work item; got kind=<kind>."` Do NOT call any write tool.

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`.
2. **Inspect field**: bind `current = detail.attributes.verification_commands` (may be null / absent / partial). Surface the current state to the user as a 4-line summary, e.g. `build: cargo build … | test: (unset) | lint: cargo clippy … | smoke: (unset)`.
3. **Per-key prompt**: ask the user one decision per key via a SINGLE `AskUserQuestion` call carrying all four questions (the harness allows up to 4 questions per call; if that limit changes, fall back to four sequential `AskUserQuestion` calls). For each key:
   - **Question header**: `Verification command: <key>` (one of `build` / `test` / `lint` / `smoke`).
   - **Question body**: `Current: <current value or "(unset)">`.
   - **Options** (exactly 2): `Keep` — leave the existing value untouched; `Set new` — provide a new command via the `Other` free-text channel.
4. **Per-key supersession (§b-supersession, per-element)**: for every key where (a) the user picked `Set new` AND (b) the current value is non-empty, run the verbatim §b-supersession `AskUserQuestion` template BEFORE committing that key's change. Substitute `<field-name>` → `verification_commands.<key>` and `<current-value-summary>` → the first ~80 characters of the existing command on one line. On `Keep current`, revert that key's decision to `Keep`; on `Replace`, retain the new value. Per-key scope means the user may replace `build` while keeping `test` untouched in a single invocation.
5. **Write**: if at least one key was changed, rebuild the FULL four-key object (current value for `Keep` keys, new value for `Set new` keys; absent keys stay absent) and call `set_story_plan({id: $work_item_id, verification_commands: {…}})`. If every key was `Keep`, no-op and return the §b-noop confirmation: `"verification_commands already matches the value you provided — no change."` Record provenance per §c on any write.

Pseudocode of the write step:

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

## Provenance recording (per §c)

After a successful write, append exactly one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape.

Summary line: `"verification-commands: set keys <comma-separated changed keys> on <work_item_id>"` — e.g. `"verification-commands: set keys build, test on story:abc123"`. Use the verb `set` for first-create (no prior value at any key) and `superseded` for a write where at least one of the changed keys had a non-empty prior value the user chose to `Replace`. One activity entry per invocation regardless of how many keys changed (§c: "one entry per write" — the write here is one `set_story_plan` call).

## Sentry-pattern compliance (per §e)

The skill body decides which keys the user changed and rebuilds the four-key blob to write. Lumina's `repo.rs` (driven by `set_story_plan`) validates the object shape against the round-2 `VerificationCommands` struct (rejecting unknown sub-keys), validates the target is a story, runs the write in one transaction, and emits exactly one event. The skill body MUST NOT validate the sub-key names, MUST NOT read sibling story-plan keys to "preserve" them (the merge call at the top-level `verification_commands` key leaves siblings untouched), and MUST NOT call `update_work_item` or `set_work_item_attributes` directly — go through `set_story_plan` only.
