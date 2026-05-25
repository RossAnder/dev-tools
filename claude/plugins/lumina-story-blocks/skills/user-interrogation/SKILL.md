---
name: user-interrogation
description: Enumerate open questions across HumanLayer's 4 axes (scope, error-handling, data-ownership, compatibility) plus an optional 5th-axis fallback.
arguments: [work_item_id]
disable-model-invocation: true
---

# `lumina:user-interrogation`

Enumerate the open questions a story needs answered before tasks can be executed. The skill walks HumanLayer's four directed-questioning axes (R16 in the parent plan: scope, error-handling, data-ownership, compatibility), asks the user one question per axis, writes the unresolved ones into `open_questions` with at least two `question_options` each, and finishes with a "5th axis" fallback so the user can extend the taxonomy per-story.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per-axis), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution).

## Target

`add_open_question` accepts ONLY `kind = story` rows (per `claude/skills/lumina/SKILL.md` line 104 — it is rejected on non-story targets). Step 2 below verifies `detail.kind == "story"` and aborts loud on any other kind; failing early gives a better error than waiting for lumina's `invalid_params` rejection.

## What this skill explicitly DOES NOT do

This skill does not call `mcp__lumina__resolve_open_question`, `mcp__lumina__block_task_on_question`, or `mcp__lumina__set_enabling_option`. Resolution is a separate decision step — the user resolves a question by picking an option later (via the lumina UI, raw MCP, or a future `/lumina:resolve-question` skill). The parent plan §Tasks §6 calls this boundary out explicitly, and the §Approach `user-interrogation` row repeats it: this skill writes questions and options, nothing more.

## MCP tools — argument-shape gotchas

```
mcp__lumina__add_open_question {
  story_id: "$work_item_id",   # CRITICAL: this tool uses `story_id`, NOT `work_item_id`.
  question: "<axis question text>"
}
```

```
mcp__lumina__add_question_option {
  question_id: <id from add_open_question>,
  label: "<short option label>",
  detail: "<optional longer description>"   # optional
}
```

`add_open_question` is the ONLY tool in the lumina catalogue that takes `story_id` rather than `work_item_id` (per `claude/skills/lumina/SKILL.md` line 104). Do NOT pass `work_item_id` — lumina will reject the call as `invalid_params`. The id value is the same; only the parameter name differs.

## The 4 axes (R16) and the 5th-axis fallback

The skill enumerates one open question per axis, in this order. For EACH axis the body shown below is the verbatim `AskUserQuestion` body for the axis prompt.

1. **Scope** — `What's IN scope vs OUT of scope for this story? Are there boundary cases that sit at the edge and need explicit deciding?`
2. **Error-handling** — `What failure modes does this story handle? Which does it ignore, which propagate to the caller, and which retry?`
3. **Data-ownership** — `Who or what owns the data this story touches — for read, for write, for delete? Are any cross-service or cross-module boundaries crossed?`
4. **Compatibility** — `What consumers, API contracts, or on-disk formats must this story preserve, change, or break? Are any deprecation windows in play?`

After the four axes, ASK the user the verbatim 5th-axis fallback:

> **Question body**: `Is there a 5th axis I'm missing for THIS story? (e.g. performance, security, accessibility — anything story-specific that the standard 4 axes don't cover.)`
>
> **Options**:
> - `Yes, add another axis` — `Provide the question via the Other free-text field`
> - `No, the 4 standard axes are sufficient` — `Skip; finalise the interrogation`

If the user chooses `Yes`, run the same per-axis flow below for the user-supplied 5th axis (storing the result as one more `open_question` + `question_options`).

## Per-axis flow

For each axis (the 4 standard ones AND the optional 5th):

### Axis step 1 — already-covered check (per §b step 1-2)

Read `detail.open_questions` from the `get_work_item` result. Loop over the existing rows; for each row, check whether its `question` text already covers this axis. The heuristic is a case-insensitive substring match: an axis is already covered if any existing question text contains the axis keyword(s):

| Axis | Match keyword(s) |
|---|---|
| Scope | `scope` / `in scope` / `out of scope` |
| Error-handling | `error` / `failure` / `failure mode` |
| Data-ownership | `owner` / `ownership` / `who owns` |
| Compatibility | `compat` / `breaking` / `contract` / `deprecat` |

If a match is found, skip this axis with a one-line user-visible note:

> `<axis> axis already covered by question Q<id>: <existing question text truncated to ~80 chars>… — skipping.`

Do NOT re-ask the same axis. Move on to the next axis.

### Axis step 2 — ask the user the axis prompt (per §b step 3)

If no existing question covers the axis, ask via `AskUserQuestion` with a single open-text field, using the verbatim axis body from "The 4 axes" above. Provide one explicit `Skip this axis` option so the user can decline an axis that genuinely doesn't apply (e.g. a pure-UI story has no data-ownership axis — the parent plan §Risks calls this out, and the skill must honour the user's skip without retry).

- If the user provides a question, capture the text and proceed to axis step 3.
- If the user picks `Skip this axis`, log a one-line note (`<axis> axis skipped per user.`) and move on to the next axis. Do NOT write anything to lumina.

### Axis step 3 — write the question (per §b step 3)

Call `add_open_question`:

```
mcp__lumina__add_open_question {
  story_id: "$work_item_id",
  question: "<user's axis question text>"
}
```

Capture the returned `id` — it's the `question_id` for the option calls below.

### Axis step 4 — enforce ≥2 question_options

Ask the user for the candidate answers via `AskUserQuestion`:

> **Question body**: `What are the candidate answers to this question? Minimum of 2 — they become the question_options you (or a future operator) will later pick from to resolve. Provide each option as a short label, plus an optional longer detail.`

The user supplies one or more options. For EACH option:

```
mcp__lumina__add_question_option {
  question_id: <id from axis step 3>,
  label: "<user-provided label>",
  detail: "<user-provided detail or omit>"
}
```

ENFORCE the minimum of 2 options per question. If the user supplies fewer than 2, loop back with the prompt `This question needs at least 2 options before it can be resolved. Please add another candidate answer.` Continue looping until the user has provided at least 2 options or explicitly aborts the axis (in which case roll forward — the lumina row will exist with <2 options, but record a one-line warning to the user: `Warning: question Q<id> has <N> option(s); resolve_open_question requires ≥1 to pick from, but ≥2 is conventional. You can add more later via add_question_option.`).

### Axis step 5 — provenance per §c (one entry per write)

After the `add_open_question` call AND after EACH `add_question_option` call, append one activity entry per §c. The §c rule is "one activity entry per write — not per skill invocation", so a single axis that creates 1 question + 3 options yields 4 activity entries total. Use the templates below.

## Supersession (per §b-supersession)

The 5-step idempotency check applies per-axis: if axis step 1 finds an existing question on the axis, the DEFAULT is to skip (no re-ask). The user can manually trigger supersession by re-running the skill and explicitly asking to replace an existing axis question — for that flow, invoke the §b-supersession `AskUserQuestion` template verbatim with these substitutions:

- `<field-name>` → `open question "<axis>"` (e.g. `open question "scope"`)
- `<current-value-summary>` → the existing `question` text truncated to ~80 chars + `…` (single-line; replace any embedded newlines with spaces before truncating).

On `Replace`, you cannot in-place mutate an `open_questions` row — the lumina catalogue has no `update_open_question` tool. Instead, mark the old question superseded by writing a new one and recording the supersession in the activity log; for THIS plugin the practical path is to leave the old question in place and write a new one (the lumina UI surfaces both; the user resolves whichever is correct). On `Keep current`, abort the axis without writing.

For row-shaped `question_options`, the same "no in-place update" rule applies — superseding an option means adding a new one and letting the user pick the right one at resolve time.

## Provenance recording (per §c)

After EACH `add_open_question` write, append one activity entry:

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "user-interrogation: added <axis>-axis question to <work_item_id>",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

After EACH `add_question_option` write, append one activity entry:

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "user-interrogation: added option \"<label>\" to question Q<question_id>",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

Substitute `<axis>`, `<work_item_id>`, `<label>`, `<question_id>` with literal values (not the `$work_item_id` template). `entry_type` is `"execution"` (per §c: NOT `verification` — that's reserved for internal `check_acceptance_criterion` writes). `origin` is `"plan"` because this skill runs inside the planning workflow.

## Body — 5-step check-before-act (per §b), applied per-axis

1. **Read once up front**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` and `detail.open_questions` for re-use across axes.
2. **Precondition**: if `detail.kind != "story"`, abort with `"user-interrogation requires a story work item; got kind=<kind>."`. Do NOT call any write tool.
3. **For each axis in [scope, error-handling, data-ownership, compatibility]**, run the per-axis flow above (steps 1-5). Each axis is independently skipped, asked, written, or superseded.
4. **5th-axis fallback**: after the 4 standard axes, ask the user the verbatim 5th-axis fallback prompt above. If they say yes, run the same per-axis flow for the user-supplied axis.
5. **Return a one-line summary**: `"user-interrogation: added N question(s) across M axis/axes on <work_item_id>; K axis/axes skipped."` (where N/M/K are the literal counts).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call (`add_open_question`, then `add_question_option` × N), in what order, and with what argument shapes (notably `story_id` not `work_item_id`). Lumina's `repo.rs` validates that the target is a story, that the question text is non-empty, that option labels are non-empty, that each option's `question_id` references an extant question, and runs each write in one transaction emitting exactly one event drained to the git-export trail. The skill body MUST NOT duplicate or shadow any of those checks.
