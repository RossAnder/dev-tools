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
story(+problem_statement)` — in the ONE legal top-down order. This is a
**chained runner**: for each plan field it "runs" the matching per-block skill
by **inline-replication per §l.4** — it READS that block's `SKILL.md` and
replicates its §b check-before-act + §c provenance sequence INLINE via raw
`mcp__lumina__*` calls, rather than re-asking those skills' prompts here. It
does NOT dispatch `Skill("lumina:<block>", …)`: every block carries
`disable-model-invocation: true`, so a runner-issued `Skill()` is refused at the
harness layer (§l.4). The runner stays INLINE (six §a keys minus the fork pair —
it is NOT forked; see §d) because each step is a user-mediated creation gate.

This skill MUTATES the store (it calls `create_work_item` directly and
inline-replicates DB-writing block skills per §l.4), so
`disable-model-invocation: true` is MANDATORY per §a — it must fire only on an
explicit `/lumina:create-project` slash invocation, never off a description
match.

Cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md):
§a (frontmatter shape — five keys, NOT forked), §b (the §b check-before-act each
block enforces — replicated INLINE here, not delegated), §c (the runner emits
ONE bootstrap rollup; each replicated block's own §c fires inline as part of its
replication), §e (Sentry — runner = orchestration, MCP = state, each block's
workflow = its replicated body), **§l.4 (inline-replication — the canonical
execution path; this runner "runs" each block by reading its `SKILL.md` and
replicating it inline, NOT by `Skill()`-dispatch; the plan-story chain follows
§l.4(a) bounded nested-runner recursion)**, **§m (epic/focus semantics — §m.0
epic close-criteria gate, §m.1 focus shape, §m.2 the kind-precondition block
writers this runner composes)**.

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

Per-block field handling — INLINE-REPLICATION per §l.4 (the runner does NOT
re-ask these prompts itself, and does NOT issue `Skill("lumina:<block>", …)`):
for each block named below the runner READS that block's `SKILL.md` and
replicates its §b check-before-act + §c provenance steps INLINE, driving the
raw `mcp__lumina__*` tools the block would call. The "id" the block's
`arguments` frontmatter names is just the bound work-item id the inline
replication operates on.

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
outcome string to satisfy the create, then **inline-replicate `epic-outcome`
per §l.4** (read `../epic-outcome/SKILL.md`, run its canonical 3-axis
interrogation, and refine/supersede the outcome via `set_epic_plan` directly).
Bind the returned `<epic>` id.

### Step 3 — epic close-criteria (≥1) — R3 HARD GATE before any story

**Inline-replicate `epic-close-criteria` per §l.4** (read
`../epic-close-criteria/SKILL.md` and run its body inline) and LOOP until the
epic carries **≥1 close-criterion**. This is the **R3 HARD GATE**: per §m.0 a
story CANNOT be created until its ancestor epic carries ≥1 acceptance
criterion (close-criterion). The replicated block writes each row via
`add_acceptance_criterion({ work_item_id: <epic>, text: … })`.

After the inline replication completes, re-read
`mcp__lumina__get_work_item({ id: "<epic>" })` and verify
`detail.acceptance_criteria.length >= 1`. If still zero, the gate is UNMET —
re-replicate `epic-close-criteria` or ABORT; do NOT proceed to Step 4.

> **Gate ordering invariant**: the `add_acceptance_criterion(<epic>)` write
> (performed by the inline-replicated `epic-close-criteria` block in THIS step)
> MUST complete and verify ≥1 before the first `create_work_item { kind:
> "story" }` in Step 5. This step textually and causally precedes the story
> create.

### Step 4 — create the focus with a MANDATORY shape, then framing

```
create_work_item { kind: "focus", parent_id: <epic>, title: <focus title>, shape: "vertical-slice" | "cross-cutting" | "foundational" }   → { id: <focus> }
```

`shape` is MANDATORY for a focus (§m.1) — `create_work_item` rejects a focus
create that omits it as `invalid_params`. Prompt the user to pick one of the
three shapes for the create (or **inline-replicate `focus-shape` per §l.4**
first — read `../focus-shape/SKILL.md` — to choose meaningfully with the
gloss-labelled options, then pass the chosen value to the create). After the
focus exists, **inline-replicate `focus-framing` per §l.4** (read
`../focus-framing/SKILL.md`) to capture its in-scope / out-of-scope `framing`
via `set_focus_plan`. Bind the returned `<focus>` id.

### Step 5 — create the story, then its problem_statement

```
create_work_item { kind: "story", parent_id: <focus>, title: <story title> }   → { id: <story> }
```

A story takes no mandatory plan field at create time. Once the story exists,
**inline-replicate `problem-statement` per §l.4** (read
`../problem-statement/SKILL.md`) to capture its 3-axis `problem_statement` via
`set_story_plan`. Bind the returned `<story>` id.

**Offer to chain into `/lumina:plan-story` (do NOT force it).** After the
problem-statement replication completes, present an `AskUserQuestion`:

> **Header**: `Continue into the six-phase walk?`
> **Body**: `Story <id> created with a problem_statement. Run the full
> /lumina:plan-story six-phase walk (frame → explore → decide → verify-design →
> decompose → closure) now, or stop here?`
> **Options**:
> - `Run plan-story` — **inline-replicate `plan-story` per §l.4(a)** (the
>   nested-runner case): `plan-story` is itself a chained runner, so this is
>   *replicate-then-recurse with a fixed depth bound* — `create-project`
>   (depth 0) replicates `plan-story` (depth 1), which replicates its own leaf
>   blocks (depth 2). Replication TERMINATES at depth 2; do NOT expand a
>   replicated leaf back into further runner machinery. In practice: read
>   `../plan-story/SKILL.md` and walk its six-phase body inline, exactly as
>   `plan-story` itself does.
> - `Stop here` — Finish; the hierarchy is bootstrapped, plan the story later.

## Provenance recording (per §c)

After the hierarchy is stood up, append exactly ONE §c rollup activity entry
against the new `<project>` id. Each inline-replicated block's §c provenance
write (epic-outcome, epic-close-criteria, focus-framing, problem-statement)
fires INLINE per §l.4 against its own work-item id as part of replicating that
block — the runner records those §c writes where the block would, and adds only
this single bootstrap rollup on top (it does NOT duplicate them). Use the §c template
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
  problem_statement set; <"replicated plan-story (§l.4(a))" | "stopped after bootstrap">.
```

## Sentry-pattern compliance (per §e)

The runner decides the create ORDER (the five steps above — the canonical
top-down sequence) and the close-criteria HARD GATE between Step 3 and Step 5.
Per the inline-replication path (§l.4), each block's §b check-before-act and §c
provenance write are performed by faithfully replicating that block's `SKILL.md`
body inline — the runner does NOT add a parallel prompt of its own on top of
the block's, nor a redundant §c entry beyond the one the replicated block
defines. Its own direct (non-replicated) writes are just the four
`create_work_item` calls and the single bootstrap §c rollup. Lumina's `repo.rs`
owns the hierarchy-edge validation, the `outcome`/`shape` mandatory-field
checks, the epic close-criteria gate, and the single-event-per-write invariant
— the runner MUST NOT model any of that itself.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §e, **§l.4** (inline-replication execution path), **§m**.
- Inline-replicated block skills (read each `SKILL.md` and replicate its body per §l.4):
  [`../epic-outcome/SKILL.md`](../epic-outcome/SKILL.md),
  [`../epic-close-criteria/SKILL.md`](../epic-close-criteria/SKILL.md),
  [`../focus-shape/SKILL.md`](../focus-shape/SKILL.md),
  [`../focus-framing/SKILL.md`](../focus-framing/SKILL.md),
  [`../problem-statement/SKILL.md`](../problem-statement/SKILL.md).
- Chained runner sibling (inline-replicated per §l.4(a) on the plan-story chain):
  [`../plan-story/SKILL.md`](../plan-story/SKILL.md);
  MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md).
