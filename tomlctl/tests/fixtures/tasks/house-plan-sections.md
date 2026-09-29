# Plan: Full-section fixture for the house plan format

**Plan path**: `docs/plans/house-plan-sections.md`
**Created**: 2026-09-29

## Summary

A fixture plan carrying every section of the house format in its canonical
order, each with a body. Nothing implements it. Review first: the section order
itself, since a render that moves one section fails the byte comparison.

## Context

The house format places the approval content above the three rendered sections
and the research appendix after `## After Merge`. This fixture pins that a
render leaves such a plan byte-identical.

## Scope
- **In scope**: the section order and the bytes of every section.
- **Out of scope**: the grammar variants `house-plan.md` already exercises.
- **Affected areas**: `src/widget/`

## User Decisions

1. **Split?** One plan, one milestone.

## Approach

### Widget
The widget module gains a parser and a renderer, each in its own file.

## Success Criteria

- forward: `grep -c 'fn render' src/widget/render.rs` prints `1` (today: `0`).
  - success: `test "$(grep -c 'fn render' src/widget/render.rs)" -eq 1`

## Verification Commands

- **build**: `cargo build`
- **test**: `cargo test`

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 3
- **Max parallel agents**: 2
- **Commit granularity**: per-task

## Tasks

### 1. Scaffold the widget module [S]
- **Files**: `src/widget/mod.rs` (new), `src/legacy_widget.rs` (delete)
- **Depends on**: —
- **Action**: Create the module and remove its predecessor.
- **Detail**: The module stays `pub(crate)`, per Approach, "Widget".
- **Acceptance**: The crate compiles with no warning.

### 2. Parse widgets [M]
- **Files**: `src/widget/parse.rs` (new), `src/widget/mod.rs`
- **Depends on**: 1
- **Action**: Read a widget from its text form.
- **Detail**: An empty input is refused.
- **Acceptance**:
  - forward: `grep -c 'fn parse' src/widget/parse.rs` prints `1` (today: `0`).
  - falsifier: an empty input returns an error.

### 3. Render widgets [M]
- **Files**: `src/widget/render.rs` (new), `src/widget/mod.rs`
- **Depends on**: 2
- **Action**: Write a widget back to its text form.
- **Detail**: The output parses to the input.
- **Acceptance**: A widget round-trips.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 3 — dependency closure: 1, 2, 3. The whole widget module.

## Risks

- The fixture is a section-order exercise, so a real plan's prose density is absent.

## After Merge

- Run `cargo install --path .` to put the new binary on PATH.

## Exploration Notes

The widget module has no predecessor beyond `src/legacy_widget.rs`.

## Research Notes

No outside source was consulted for this fixture.
