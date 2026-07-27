---
name: create-project
description: Bootstrap a fresh project → epic(+outcome) → epic close-criteria → focus(+shape +framing) → story(+problem_statement) hierarchy, composing the per-block skills with the create-sequence ordering and gates.
arguments: [title]
argument-hint: "[title]"
---

# `lumina:create-project`

INLINE orchestrator skill: stands up a fresh lumina hierarchy from nothing —
`project → epic(+outcome) → epic close-criteria → focus(+shape, +framing) →
story(+problem_statement)` — in the ONE legal top-down order. This is a
**chained runner**: for each plan field it "runs" the matching per-block skill
by **`Skill()`-dispatch per §l.4** — it calls
`Skill("lumina:<block>", "<work_item_id>")`, which invokes the REAL block skill
to run its §b check-before-act + §c provenance sequence against the raw
`mcp__lumina__*` tools, rather than re-asking those skills' prompts here. The
runner stays INLINE (six §a keys minus the fork pair — it is NOT forked; see §d)
because each step is a user-mediated creation gate.

This skill MUTATES the store (it calls `create_work_item` directly and
dispatches DB-writing block skills).

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, with **§l.4 as the
canonical execution path** (`Skill()`-dispatch; the plan-story chain follows
§l.4(a) bounded nested-runner recursion) and **§m** for the epic/focus semantics
this runner composes (§m.0 close-criteria gate, §m.1 focus shape, §m.2 the
kind-precondition writers). Each dispatched block runs its OWN §b
check-before-act and §c write; the runner adds only ONE bootstrap rollup and
never re-asks a block's prompts or duplicates its provenance. `plan-story`'s body
is the §o stage machine — the dispatch path into it is unchanged.

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

Every per-block field below is filled by `Skill("lumina:<block>", "<work_item_id>")`
per §l.4, passing the bound work-item id as the argument.

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
outcome string to satisfy the create, then **dispatch `epic-outcome` via
`Skill("lumina:epic-outcome", "<epic>")` per §l.4** (it runs its canonical
3-axis interrogation and refines/supersedes the outcome via `set_epic_plan`).
Bind the returned `<epic>` id.

### Step 3 — epic close-criteria (≥1) — R3 HARD GATE before any story

**Dispatch `epic-close-criteria` via
`Skill("lumina:epic-close-criteria", "<epic>")` per §l.4** and LOOP until the
epic carries **≥1 close-criterion**. This is the **R3 HARD GATE**: per §m.0 a
story CANNOT be created until its ancestor epic carries ≥1 acceptance
criterion (close-criterion). The dispatched block writes each row via
`add_acceptance_criterion({ work_item_id: <epic>, text: … })`.

After the dispatch returns, re-read
`mcp__lumina__get_work_item({ id: "<epic>" })` and verify
`detail.acceptance_criteria.length >= 1`. If still zero, the gate is UNMET —
re-dispatch `epic-close-criteria` or ABORT; do NOT proceed to Step 4.

> **Gate ordering invariant**: the `add_acceptance_criterion(<epic>)` write
> (performed by the dispatched `epic-close-criteria` block in THIS step)
> MUST complete and verify ≥1 before the first `create_work_item { kind:
> "story" }` in Step 5. This step textually and causally precedes the story
> create.

### Step 4 — create the focus with a MANDATORY shape, then framing

```
create_work_item { kind: "focus", parent_id: <epic>, title: <focus title>, shape: "vertical-slice" | "cross-cutting" | "foundational" }   → { id: <focus> }
```

`shape` is MANDATORY for a focus (§m.1) — `create_work_item` rejects a focus
create that omits it as `invalid_params`. Prompt the user to pick one of the
three shapes for the create (or **dispatch `focus-shape` via
`Skill("lumina:focus-shape", "<epic>")` per §l.4** first — to choose
meaningfully with the gloss-labelled options, then pass the chosen value to the
create). After the focus exists, **dispatch `focus-framing` via
`Skill("lumina:focus-framing", "<focus>")` per §l.4** to capture its in-scope /
out-of-scope `framing` via `set_focus_plan`. Bind the returned `<focus>` id.

### Step 5 — create the story, then its problem_statement

```
create_work_item { kind: "story", parent_id: <focus>, title: <story title> }   → { id: <story> }
```

A story takes no mandatory plan field at create time. Once the story exists,
**dispatch `problem-statement` via
`Skill("lumina:problem-statement", "<story>")` per §l.4** to capture its 3-axis
`problem_statement` via `set_story_plan`. Bind the returned `<story>` id.

**Offer to chain into `/lumina:plan-story` (do NOT force it).** Round-5 reshaped
`plan-story` from a per-block gate-walker into the **planning orchestrator** — a
six-STAGE machine (`triage → frame → plan → brief → align → rework`) that WRAPS
the §l.0 six PHASES, with a gating-tier-aware grill, a curated decision brief,
and an epoch-scoped rework loop (CONVENTIONS §o). The dispatch shape is
UNCHANGED — it is still a `Skill()`-dispatch per §l.4(a) on the same depth-1→2
nested-runner chain. After the problem-statement dispatch returns, present an
`AskUserQuestion`:

> **Header**: `Continue into the planning orchestrator?`
> **Body**: `Story <id> created with a problem_statement. Run the full
> /lumina:plan-story planning orchestrator now (the triage → frame → plan →
> brief → align → rework stage machine wrapping the six canonical phases), or
> stop here?`
> **Options**:
> - `Run plan-story` — **dispatch `plan-story` via
>   `Skill("lumina:plan-story", "<story>")` per §l.4(a)** (the nested-runner
>   case): `plan-story` is itself a chained runner, so this is *dispatch-then
>   -recurse with a fixed depth bound* — `create-project` (depth 0) dispatches
>   `plan-story` (depth 1), which dispatches its own leaf blocks (depth 2).
>   Recursion TERMINATES at depth 2; a dispatched leaf is never expanded back
>   into further runner machinery. The dispatched `plan-story` runs its
>   stage-machine body (CONVENTIONS §o) itself — `create-project` neither
>   re-implements the stages nor pre-computes the gating tier.
> - `Stop here` — Finish; the hierarchy is bootstrapped, plan the story later.

## Provenance recording (per §c)

After the hierarchy is stood up, append exactly ONE rollup entry against the new
`<project>` id — each dispatched block already wrote its own §c entry against its
own work-item id. Apply the `${CLAUDE_SESSION_ID}` substitution guard (on
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
  problem_statement set; <"dispatched plan-story (§l.4(a))" | "stopped after bootstrap">.
```

## What the runner owns

The create ORDER (the five steps above) and the close-criteria HARD GATE between
Step 3 and Step 5. Its only direct writes are the four `create_work_item` calls
and the bootstrap rollup. Lumina owns hierarchy-edge validation, the
`outcome`/`shape` mandatory-field checks, the epic close-criteria gate, and the
single-event-per-write invariant — do NOT model any of that here.

## Pointers

- Block skills dispatched via `Skill("lumina:<block>", …)` per §l.4:
  [`../epic-outcome/SKILL.md`](../epic-outcome/SKILL.md),
  [`../epic-close-criteria/SKILL.md`](../epic-close-criteria/SKILL.md),
  [`../focus-shape/SKILL.md`](../focus-shape/SKILL.md),
  [`../focus-framing/SKILL.md`](../focus-framing/SKILL.md),
  [`../problem-statement/SKILL.md`](../problem-statement/SKILL.md).
- Chained runner sibling (dispatched via `Skill()` per §l.4(a) on the plan-story chain):
  [`../plan-story/SKILL.md`](../plan-story/SKILL.md);
  MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md).
