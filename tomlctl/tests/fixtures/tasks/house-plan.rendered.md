# Plan: House-format fixture for the task store

**Plan path**: `docs/plans/house-plan.md`
**Created**: 2026-09-08
**Status**: Draft

## Context

A fixture plan in the house markdown format, written to exercise every grammar
variant the importer accepts. Nothing implements it; the bytes outside the three
derived sections exist so a render can prove it leaves them alone.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 3, 4, 6, 9, 10
- **Max parallel agents**: 6
- **Commit granularity**: per-task

— tasks 5 and 6 land in one commit because they share a file

## Tasks

### 1. Scaffold the module tree [S]
- **Files**: `src/tasks/mod.rs`
- **Depends on**: —
- **Action**: Create the module and its leaf-per-verb children.
- **Detail**: Every child stays `pub(crate)`.
- **Acceptance**: The crate compiles with no warning.

### 2. Land the schema round-trip [M]
- **Files**: `src/tasks/schema.rs`, `src/tasks/store.rs`, `src/tasks/mod.rs`
- **Depends on**: 1
- **Action**: Convert the store to and from TOML, preserving key order.
- **Detail**: A body may carry a Windows path (`src\tasks\schema.rs`), so
  the writer must leave the separator unescaped.
- **Acceptance**: A fixture store round-trips.

### 3. Parse the `## Tasks` section [M]
- **Files**: `src/tasks/parse_tasks.rs`
- **Depends on**: 1, 2 (the schema types are what the parser fills, so the two commits stay ordered)
- **Action**: Read numbered headings and their field lines.
- **Detail**: A field value continues onto lines indented two spaces.
- **Acceptance**: A wrapped field parses whole.

### 4. Parse the policy bullets and the max_parallel range [S]
- **Files**: `src/tasks/parse_policy.rs`
- **Depends on**: 3
- **Action**: Read the four `## Execution Policy` bullets.
- **Detail**: An absent section takes the house defaults.
- **Acceptance**: The four bullets parse.

### 5. Build the graph engine — Kahn rounds and bitset closures [L]
- **Files**: `src/tasks/graph.rs`
- **Depends on**: 2
- **Action**: Layer the DAG deterministically.
- **Detail**: Ascending id tie-breaks in every round.
- **Acceptance**: The rounds are stable across runs.

### 6. Fold the closures into checkpoint groups [M]
- **Files**: `src/tasks/graph.rs`, `src/tasks/closure.rs`
- **Depends on**: 5
- **Action**: Derive each group's members and maximal elements.
- **Detail**: A group is valid when its prefix union is downward-closed.
- **Acceptance**: An invalid cut is reported.

### 7. Wire the import verb [M]
- **Files**: `src/tasks/import_plan.rs`
- **Depends on**: 3, 4, 6
- **Action**: Upsert the store by `ref`.
- **Detail**: An existing row keeps its execution state.
- **Acceptance**: A second import changes nothing.

### 8. Render the three sections [M]
- **Files**: `src/tasks/render.rs`
- **Depends on**: 7
- **Action**: Rewrite the three derived sections in place.
- **Detail**: Every byte outside those sections survives the write, so a
  hand-authored preamble is not the renderer's to reflow.
- **Acceptance**: The round-trip holds.

### 9. Document the store contract [S]
- **Files**: `docs/task-store.md`
- **Depends on**: —
- **Action**: State the schema, the verb surface and the ref rule.
- **Detail**: No task depends on this one and it depends on none.
- **Acceptance**: The document exists.

### 10. Smoke the plan corpus [S]
- **Files**: `tests/tasks_corpus.rs`
- **Depends on**: 8
- **Action**: Dry-run every plan in the corpus.
- **Detail**: The file list is derived at runtime, never transcribed.
- **Acceptance**: Every in-format plan imports.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 3 — closure: 1, 2, 3. The module tree and the schema round-trip.

— CHECKPOINT B after tasks 4, 6 — closure: 1, 2, 3, 4, 5, 6. both parsers and the graph engine land together

— CHECKPOINT C after tasks 9, 10 — closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10. the import verb, the renderer and the corpus smoke.

## Risks

- The fixture is a grammar exercise, so a real plan's prose density is absent.
