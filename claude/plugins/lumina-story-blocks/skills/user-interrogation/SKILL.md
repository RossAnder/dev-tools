---
name: user-interrogation
description: Enumerate open questions for a story across HumanLayer's 4 axes (scope, error-handling, data-ownership, compatibility) plus a scope-challenge axis.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:user-interrogation`

Enumerate the open questions a story needs answered before tasks can execute. Walk HumanLayer's four directed-questioning axes (R16: scope, error-handling, data-ownership, compatibility) PLUS a round-5 **scope-challenge** axis (R51), ask the user one question per axis, write the unresolved ones into `open_questions` with at least two `question_options` each, and finish with a fallback so the user can extend the taxonomy per story.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, with §b applied per axis.

**This skill writes questions and options, nothing more.** It does NOT call `resolve_open_question`, `block_task_on_question`, or `set_enabling_option` — resolution is a separate decision step (lumina UI, raw MCP, or a future `/lumina:resolve-question` skill).

## MCP tools — argument-shape gotcha

```
mcp__lumina__add_open_question {
  story_id: "$work_item_id",   # CRITICAL: this tool uses `story_id`, NOT `work_item_id`.
  question: "<axis question text>"
}

mcp__lumina__add_question_option {
  question_id: <id from add_open_question>,
  label: "<short option label>",
  detail: "<optional longer description>"   # optional
}
```

`add_open_question` is the ONLY tool in the catalogue taking `story_id` rather than `work_item_id` (per `../mcp/SKILL.md` §Planning & decision tools). The id VALUE is the same; only the parameter name differs — passing `work_item_id` is rejected as `invalid_params`. The tool also accepts only `kind = story` rows.

## The 5 axes (R16 + R51) and the extra-axis fallback

One open question per axis, in this order. Each body below is the verbatim `AskUserQuestion` body for that axis prompt.

1. **Scope** — `What's IN scope vs OUT of scope for this story? Are there boundary cases that sit at the edge and need explicit deciding?`
2. **Error-handling** — `What failure modes does this story handle? Which does it ignore, which propagate to the caller, and which retry?`
3. **Data-ownership** — `Who or what owns the data this story touches — for read, for write, for delete? Are any cross-service or cross-module boundaries crossed?`
4. **Compatibility** — `What consumers, API contracts, or on-disk formats must this story preserve, change, or break? Are any deprecation windows in play?`
5. **Scope-challenge** (round-5, R51 — devil's advocate) — `Is this story the RIGHT size? Should it be SPLIT into smaller stories, or is it too narrow and should it be BIGGER / absorb adjacent work? What's the strongest case that the current scope is wrong?` Distinct from axis 1: Scope draws the in/out boundary, Scope-challenge questions whether the whole framing is the right ambition. It counters the conservative, scope-narrowing bias R51 identifies and pairs with the `plan-story` orchestrator's frame-stage scope-challenge.

After the five axes, ask the verbatim fallback:

> **Question body**: `Is there a further axis I'm missing for THIS story? (e.g. performance, security, accessibility — anything story-specific that the standard 5 axes don't cover.)`
>
> **Options**:
> - `Yes, add another axis` — `Provide the question via the Other free-text field`
> - `No, the 5 standard axes are sufficient` — `Skip; finalise the interrogation`

On `Yes`, run the same per-axis flow for the user-supplied extra axis.

## Per-axis flow

### Axis step 1 — already-covered check

Loop over `detail.open_questions` and decide whether each row's `question` text already covers this axis.

**Stricter coverage rule (round-5).** A single keyword/substring hit is NOT sufficient to suppress an axis — that crude heuristic skipped axes only incidentally mentioned, leaving the real question unasked. Suppress an axis ONLY if BOTH hold:

1. the question text matches **≥2 distinct** cues below, OR matches one cue AND is *substantively about* that axis (it poses the axis's actual decision, not merely name-drops the term); AND
2. the question is genuinely OPEN/unresolved on that axis (a resolved-and-moved-on mention does not block re-asking a still-live concern).

When in doubt, DO NOT suppress — a duplicate question is cheaper than a silently-skipped one, and the user can decline via `Skip this axis`.

| Axis | Keyword cues (need ≥2, or 1 + substantive coverage) |
|---|---|
| Scope | `scope` / `in scope` / `out of scope` / `boundary` |
| Error-handling | `error` / `failure` / `failure mode` / `retry` / `propagate` |
| Data-ownership | `owner` / `ownership` / `who owns` / `read` / `write` / `delete` |
| Compatibility | `compat` / `breaking` / `contract` / `deprecat` / `consumer` |
| Scope-challenge | `split` / `bigger` / `too narrow` / `too small` / `right size` / `ambition` / `absorb` |

If — and only if — the rule is satisfied, skip the axis with:

> `<axis> axis already covered by question Q<id>: <existing question text truncated to ~80 chars>… — skipping.`

### Axis step 2 — ask the axis prompt

Ask via `AskUserQuestion` with a single open-text field, using the verbatim axis body above. Always provide a `Skip this axis` option so the user can decline an axis that genuinely doesn't apply (a pure-UI story has no data-ownership axis) — honour the skip without retry, log `<axis> axis skipped per user.`, and write nothing.

### Axis step 3 — write the question

Call `add_open_question` and capture the returned `id` as the `question_id` for the option calls.

### Axis step 4 — enforce ≥2 question_options

> **Question body**: `What are the candidate answers to this question? Minimum of 2 — they become the question_options you (or a future operator) will later pick from to resolve. Provide each option as a short label, plus an optional longer detail.`

One `add_question_option` call per option. SOFT-enforce the ≥2 convention: if the user supplies fewer and refuses to add another, accept the under-populated row and warn — `Warning: question Q<id> has <N> option(s); 'mcp__lumina__resolve_open_question' technically requires only ≥1 option, but the project convention is ≥2 for a meaningful axis. You can add more later via add_question_option.` Do NOT loop indefinitely or hard-abort the axis.

## Supersession (per §b-supersession)

Per axis, the DEFAULT when axis step 1 finds an existing question is to skip. The user can trigger supersession by re-running and explicitly asking to replace an axis question. Invoke the §b-supersession template verbatim, substituting `<field-name>` → `open question "<axis>"` (e.g. `open question "scope"`) and `<current-value-summary>` → the existing `question` text truncated to ~80 chars + `…` (single-line; newlines collapsed first), with ONE override: relabel the `Replace` option to `Add replacement question (old remains visible)`, because the catalogue has NO `update_open_question` or `supersede_open_question` tool and in-place mutation is impossible.

On `Add replacement question (old remains visible)`: write the new question via `add_open_question` AND record `mcp__lumina__record_task_activity` with `entry_type: "comment"`, body `question Q<new_id> supersedes Q<old_id> per user (no in-place tool — both rows remain live)`. The activity log preserves the supersession intent the schema cannot enforce. Leave the old row in place — the UI surfaces both and the user resolves whichever is correct. On `Keep current`, abort the axis without writing.

Operator note: after a Replace, this story's `open_questions` count increases by one for the same axis. Confirm via `get_work_item` that the activity log holds two `user-interrogation: added <axis>-axis question` entries — the newer id is the intended live question. "Two open questions on the same axis" is the expected post-condition of a Replace, not a bug. The same no-in-place-update rule applies to `question_options`: supersede by adding a new one and letting the user pick at resolve time.

## Body

1. **Read once up front**: `mcp__lumina__get_work_item({id: "$work_item_id"})`; bind `detail.kind` and `detail.open_questions` for re-use across axes.
2. **Kind-precondition**: if `detail.kind != "story"`, abort before any write: `user-interrogation requires a story work item; got kind=<kind>.`
3. **For each axis** in [scope, error-handling, data-ownership, compatibility, scope-challenge], run the per-axis flow. Each axis is independently skipped, asked, written, or superseded.
4. **Extra-axis fallback**: after the five, ask the verbatim fallback prompt; on `Yes`, run the per-axis flow for the user-supplied axis.
5. **Return**: `user-interrogation: added N question(s) across M axis/axes on <work_item_id>; K axis/axes skipped.` (literal counts).

## §c summary lines

One entry per write, so one axis creating 1 question + 3 options yields 4 entries.

- `add_open_question`: `user-interrogation: added <axis>-axis question to <work_item_id>`
- `add_question_option`: `user-interrogation: added option "<label>" to question Q<question_id>`

Substitute literal values. `entry_type` is `"execution"`; `origin` is `"plan"`.
