# 0001 — Epic is a closeable deliverable; Focus is a shaped rollup (rename from Feature)

**Status:** accepted (2026-05-29)

## Decision

Lumina's `epic` and `feature` levels were byte-for-byte identical — same legal attributes (`context`, `grouping_rationale`), same `relevance="backlog"` default, same validation branch — distinguished only by tree depth (the textbook over-nesting anti-pattern). We give them genuinely divergent semantics:

- **Epic = a closeable deliverable.** The only level with both an independent **outcome** gate — a mandatory, deliberately high-level intent statement set at creation ("no outcome ⇒ folder, not epic") — and a **rollup** gate. An epic is `done` only when its **close-criteria** (the story-level `acceptance_criteria` + `closure_gate` mechanism, lifted to the epic) all pass **AND** every descendant story is terminal (`done`/`cancelled`). An epic must carry ≥1 close-criterion before its first story can be created (the gate fires at first-story-creation, since a `focus` sits between).
- **`feature` → `focus`.** Renamed to shed the agile "feature = releasable increment" connotation (explicitly rejected) and to match the user's own framing ("areas of focus"). A focus has **no intrinsic deliverable**; its "done" is a pure rollup of its stories. It is a **per-epic instance** (never shared/many-to-many), so it dies when its epic closes and structurally cannot rot into the eternal-bucket anti-pattern.
- **Focus carries a mandatory `shape`** — `vertical-slice | cross-cutting | foundational` — the carving rationale ("why is this a separate focus?"), forced at carve-time as the focus-level twin of the epic's outcome gate. `shape` is a **fractal axis**: the same distinction recurs at the (future) intra-story task-group scale, sharing vocabulary but stored per level.

## Considered options

- **Keep `feature` / collapse the level (Linear-style 3-level) / make `focus` a shared many-to-many grouping** — rejected: collapsing loses useful framing in a solo+agent workflow; a shared grouping reintroduces the eternal-bucket rot that the per-epic lifespan prevents.
- **A separate milestone entity for coordinated dated releases** — rejected: epics close independently in a solo+agent context; `target_date` stays an informational, never-policed field on the epic. A future gating case would become an orthogonal tag, never epic nesting.
- **Typed `focus_kind` enum / one shared table for the focus-shape and the task-group shape** — rejected: named `shape` (not a third `*_kind` colliding with `kind`/`task_kind`), stored per level (exactly one shape per focus; 0+ task-groups per story), sharing only vocabulary.
- **File-footprint overlap as a carve / re-home signal** — rejected: `files_touched` is orthogonal to the hierarchy; it feeds parallel-execution collision avoidance + sprint composition, never focus boundaries. Carving is intent/shape only, agent-proposed and human-confirmed.

## Consequences

- A forward-only migration (`0010_epic_focus_semantics.sql`) relabels `feature` → `focus`, recreates the hierarchy-edge triggers, adds the `work_items.shape` column (+ CHECK), and splits the per-kind attribute sets (epic gains `outcome`, drops `grouping_rationale`; focus gains `framing`). Legacy rows predate `shape`/`outcome` and require a backfill decision (see the plan's open decisions).
- New gates land in the repo layer: epic-outcome-at-create, ≥1-close-criterion-before-first-story, epic-done = (criteria checked) ∧ (stories terminal), shape-mandatory-for-focus.
- `add_acceptance_criterion` is already kind-agnostic, so close-criteria attach to an epic with no change; `set_closure_gate` (story-locked) and `set_relevance` (hardcodes `"feature"`) are the concrete repo touch-points.

Full resolution: `docs/design/lumina-epic-focus-concepts.md`. Glossary: `lumina/CONTEXT.md`. Implementation plan: `docs/plans/lumina-epic-focus-semantics.md`.
