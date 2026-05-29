---
name: focus-framing
description: Capture or update a focus's framing (what's in-scope and out-of-scope for this focus).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:focus-framing`

Capture or update a focus's `attributes.framing` via `mcp__lumina__set_focus_plan`. The skill elicits framing prose — what is in-scope and what is explicitly out-of-scope for this focus — assembles it into a single framing string, and writes it through lumina's merge-call focus-plan setter.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution), §m.2 (kind-precondition: focus-only writer fails-fast on the wrong kind; §m.2 is the on-point authority because §g/§h's taxonomy splits skills into any-kind lens writers and story-only column writers and has no epic/focus category).

## Target

`set_focus_plan` accepts only `kind = focus`. It is a merge call — passing ONLY `framing` leaves any sibling focus-plan fields untouched (per §e, do NOT read the whole plan and rewrite it; pass only the field this skill owns). This skill fails loud at the Precondition check below if the caller passes a non-focus id.

## MCP tool

```
mcp__lumina__set_focus_plan {
  id: "$work_item_id",
  framing: "<assembled in-scope / out-of-scope text>"
}
```

## The framing prompt

The skill body asks the user two questions in one `AskUserQuestion` call (each with an `Other` free-text option so the user can type a substantive paragraph):

1. **What's in-scope for this focus?** — 1-3 sentences naming the capabilities/areas this focus owns and will deliver.
2. **What's out-of-scope?** — 1-3 sentences naming the adjacent work this focus explicitly does NOT cover (deferred to other focuses, or out of the epic entirely), so the boundary is unambiguous.

After collecting the two answers, the skill assembles them into a single `framing` string using this stable two-paragraph layout (so re-runs are byte-stable when the same answers are given):

```
In-scope: <answer 1>

Out-of-scope: <answer 2>
```

The labelled-paragraph layout is deliberate — it makes the resulting prose self-documenting in the lumina UI, and the literal `In-scope:` / `Out-of-scope:` prefixes give the equality check in §b step 4 a stable string to compare against.

## Body — 5-step check-before-act (per §b)

**Precondition**: this skill applies only to `kind == "focus"` work items (per §e's blessed local kind-check and the §m.2 kind-precondition rule for non-lens shaped writers). After step 1's `get_work_item` returns, verify `detail.kind == "focus"`. If not, abort with a one-line error: `"focus-framing requires a focus work item; got kind=<kind>."` Do NOT call any write tool. (This is a kind-guard, not a numbered §b step — the canonical sequence below preserves §b's 1-5 numbering exactly.)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` (consumed by the Precondition above) and proceed once the Precondition passes.
2. **Inspect field**: bind `detail.attributes.framing` from the returned detail (may be null / absent / empty). This is the value against which the next three steps branch.
3. **Absent → create**: if `detail.attributes.framing` is null / absent / empty, run the framing prompt above. Assemble the two answers into the layout shown. Call `set_focus_plan({id: $work_item_id, framing: <assembled>})`. Record provenance per §c with the `set` summary form. Return a one-line confirmation: `"framing created on <work_item_id>."`
4. **Present and matches**: run the framing prompt, assemble the new value, and compare against `detail.attributes.framing`. If they match byte-for-byte, return the §b step-4 one-line confirmation: `"framing already matches the value you provided — no change."`
5. **Present and differs**: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `framing`
   - `<current-value-summary>` → the first ~80 characters of `detail.attributes.framing` + `…` (single-line; replace any embedded newlines with spaces before truncating).
   On `Replace`, call `set_focus_plan({id: $work_item_id, framing: <new>})`, then record provenance per §c with the `superseded` summary form. On `Keep current`, abort the invocation without writing.

## Provenance recording (per §c)

After ANY successful write (step 3 first-create or step 5 supersession), append exactly one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape (including the `${CLAUDE_SESSION_ID}` substitution guard).

Summary line: `"focus-framing: set on <work_item_id>"` for step 3 (first-create); `"focus-framing: superseded on <work_item_id>"` for step 5 (supersession). Use the `superseded` form only when the prior value was non-null and the user chose `Replace` in step 5. The `<work_item_id>` substitution is the literal id value (not the `$work_item_id` template).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call and what arguments to pass. Lumina's `set_focus_plan` is a merge call — passing only `framing` leaves any sibling focus-plan fields untouched. The skill body MUST NOT read those fields and pass them back in to "preserve" them; the merge semantics handle that. Lumina's `repo.rs` also validates that the target is a focus, runs the write in one transaction, and emits exactly one event drained to the git-export trail.
