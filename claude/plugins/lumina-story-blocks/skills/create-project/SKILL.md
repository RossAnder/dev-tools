---
name: create-project
description: Bootstrap a fresh project → epic(+outcome) → epic close-criteria → focus(+shape +framing) → story(+problem_statement) hierarchy, composing the per-block skills with the create-sequence ordering and gates.
arguments: [title]
argument-hint: "[title]"
disable-model-invocation: true
---

# `lumina:create-project`

INLINE orchestrator skill: stands up a fresh lumina hierarchy from nothing —
`project → epic(+outcome) → epic close-criteria → focus(+shape, +framing) →
story(+problem_statement)` — in the ONE legal top-down order, dispatching the
matching `/lumina:<block>` skill via the `Skill` tool for each plan field
rather than re-asking those skills' prompts here. The runner stays INLINE
(six §a keys minus the fork pair — it is NOT forked; see §d) because each step
is a user-mediated creation gate.

This skill MUTATES the store (it calls `create_work_item` directly and
dispatches DB-writing block skills), so `disable-model-invocation: true` is
MANDATORY per §a — it must fire only on an explicit `/lumina:create-project`
slash invocation, never off a description match.

Cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md):
§a (frontmatter shape — five keys, NOT forked), §b (per-DISPATCHED-SKILL; each
dispatched block skill enforces its own check-before-act), §c (the runner emits
ONE provenance rollup; dispatched skills emit their own §c on internal writes),
§e (Sentry — runner = orchestration, MCP = state, dispatched skills = per-block
workflow), **§m (epic/focus semantics — §m.0 epic close-criteria gate, §m.1
focus shape, §m.2 the kind-precondition block writers this runner composes)**.

## Prerequisites

Mirror [`../plan-story/SKILL.md`](../plan-story/SKILL.md): the lumina server must
be **running** (it serves the MCP endpoint at `/mcp`) and the lumina MCP must be
registered with the harness as `lumina` (so tools surface as `mcp__lumina__*` —
see [`../mcp/SKILL.md`](../mcp/SKILL.md#connecting)). If the `mcp__lumina__*`
tools are unavailable, ABORT before any write: `"create-project requires the
lumina MCP registered as 'lumina' and the server running — see ../mcp/SKILL.md."`

## MCP tools used directly by this runner

- `mcp__lumina__create_work_item` — the FOUR direct creates (project / epic /
  focus / story). `outcome` is MANDATORY at `kind:"epic"` create time; `shape`
  is MANDATORY at `kind:"focus"` create time (∈ `vertical-slice |
  cross-cutting | foundational`) — both validated server-side per §m.0/§m.1.
- `mcp__lumina__record_task_activity` — the single §c provenance rollup at the end.

Per-block field dispatch (the runner does NOT re-ask these prompts itself):
`Skill("lumina:<block>", "<id>")` — each dispatched skill takes one positional
work-item id per its `arguments` frontmatter and runs its own §b/§c internally.

## Body — the create-sequence ordering

This is the AUTHORITY for the create-sequence ordering. Walk the five steps in
THIS order; each gate must hold before the next step runs. The hierarchy is
built top-down so every child has a legal parent (mirrors `../mcp/SKILL.md`
§"The top-down build flow").

### Step 1 — create the project (NULL parent)

```
create_work_item { kind: "project", title: "$title" }   → { id: <project> }
```

A `project` is the root: it has NO `parent_id`. Bind the returned `<project>`
id. (Offer to attach linked GitHub repos later via `add_repo_link` — out of
scope for the bootstrap; do not block on it.)

### Step 2 — create the epic with a MANDATORY outcome

```
create_work_item { kind: "epic", parent_id: <project>, title: <epic title>, outcome: <non-empty> }   → { id: <epic> }
```

`outcome` is MANDATORY for an epic (§m.0) — `create_work_item` rejects an
epic create that omits it as `invalid_params`. Prompt the user for a first-pass
outcome string to satisfy the create, then dispatch
`Skill("lumina:epic-outcome", "<epic>")` to run the canonical 3-axis
`epic-outcome` interrogation and refine/supersede the outcome via
`set_epic_plan`. Bind the returned `<epic>` id.

### Step 3 — epic close-criteria (≥1) — R3 HARD GATE before any story

Dispatch `Skill("lumina:epic-close-criteria", "<epic>")` and LOOP until the
epic carries **≥1 close-criterion**. This is the **R3 HARD GATE**: per §m.0 a
story CANNOT be created until its ancestor epic carries ≥1 acceptance
criterion (close-criterion). The `epic-close-criteria` block writes each row
via `add_acceptance_criterion({ work_item_id: <epic>, text: … })`.

After the block returns, re-read `mcp__lumina__get_work_item({ id: "<epic>" })`
and verify `detail.acceptance_criteria.length >= 1`. If still zero, the gate is
UNMET — re-dispatch `epic-close-criteria` or ABORT; do NOT proceed to Step 4.

> **Gate ordering invariant**: the `add_acceptance_criterion(<epic>)` write
> (performed by the dispatched `epic-close-criteria` block in THIS step) MUST
> complete and verify ≥1 before the first `create_work_item { kind: "story" }`
> in Step 5. This step textually and causally precedes the story create.

### Step 4 — create the focus with a MANDATORY shape, then framing

```
create_work_item { kind: "focus", parent_id: <epic>, title: <focus title>, shape: "vertical-slice" | "cross-cutting" | "foundational" }   → { id: <focus> }
```

`shape` is MANDATORY for a focus (§m.1) — `create_work_item` rejects a focus
create that omits it as `invalid_params`. Prompt the user to pick one of the
three shapes for the create (or dispatch `Skill("lumina:focus-shape",
"<focus>")` first to choose meaningfully with the gloss-labelled options, then
pass the chosen value to the create). After the focus exists, dispatch
`Skill("lumina:focus-framing", "<focus>")` to capture its in-scope /
out-of-scope `framing` via `set_focus_plan`. Bind the returned `<focus>` id.

### Step 5 — create the story, then its problem_statement

```
create_work_item { kind: "story", parent_id: <focus>, title: <story title> }   → { id: <story> }
```

A story takes no mandatory plan field at create time. Once the story exists,
dispatch `Skill("lumina:problem-statement", "<story>")` to capture its
3-axis `problem_statement` via `set_story_plan`. Bind the returned `<story>` id.

**Offer to chain into `/lumina:plan-story` (do NOT force it).** After the
problem-statement block returns, present an `AskUserQuestion`:

> **Header**: `Continue into the six-phase walk?`
> **Body**: `Story <id> created with a problem_statement. Run the full
> /lumina:plan-story six-phase walk (frame → explore → decide → verify-design →
> decompose → closure) now, or stop here?`
> **Options**:
> - `Run plan-story` — Dispatch `Skill("lumina:plan-story", "<story>")`.
> - `Stop here` — Finish; the hierarchy is bootstrapped, plan the story later.

## Provenance recording (per §c)

After the hierarchy is stood up, append exactly ONE §c rollup activity entry
against the new `<project>` id. Dispatched block skills emit their OWN §c
entries on their internal writes (epic-outcome, epic-close-criteria,
focus-framing, problem-statement) — the runner does NOT absorb or duplicate
those; it records only this single bootstrap rollup. Use the §c template
verbatim (including the `${CLAUDE_SESSION_ID}` substitution guard — on
non-substitution, write `session=unknown` and emit a one-line warning):

```
mcp__lumina__record_task_activity {
  work_item_id: "<project>",
  entry_type: "execution",
  origin: "plan",
  summary: "create-project: bootstrapped project <project> → epic <epic> → focus <focus> → story <story>",
  body: "session=${CLAUDE_SESSION_ID}; close_criteria=<N>"
}
```

Substitute the literal id values and the close-criterion count `<N>`.

## Final summary

```
create-project: bootstrapped project <project> → epic <epic> (close-criteria=<N>) → focus <focus> (shape=<shape>) → story <story>;
  problem_statement set; <"dispatched plan-story" | "stopped after bootstrap">.
```

## Sentry-pattern compliance (per §e)

The runner decides the create ORDER (the five steps above — the canonical
top-down sequence) and the close-criteria HARD GATE between Step 3 and Step 5.
It MUST NOT shadow the dispatched block skills' per-block §b check-before-act
(each block runs its own), MUST NOT re-ask their prompts inline, and MUST NOT
absorb their §c writes. The runner's only direct writes are the four
`create_work_item` calls and the single §c rollup. Lumina's `repo.rs` owns the
hierarchy-edge validation, the `outcome`/`shape` mandatory-field checks, the
epic close-criteria gate, and the single-event-per-write invariant — the runner
MUST NOT model any of that itself.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §e, **§m**.
- Composed block skills: [`../epic-outcome/SKILL.md`](../epic-outcome/SKILL.md),
  [`../epic-close-criteria/SKILL.md`](../epic-close-criteria/SKILL.md),
  [`../focus-shape/SKILL.md`](../focus-shape/SKILL.md),
  [`../focus-framing/SKILL.md`](../focus-framing/SKILL.md),
  [`../problem-statement/SKILL.md`](../problem-statement/SKILL.md).
- Chained runner sibling: [`../plan-story/SKILL.md`](../plan-story/SKILL.md);
  MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md).
