# Plan: implement-v2 cross-milestone lookahead
<!-- backlog-seed -->

## Context

- B-1fa8d53f — implement-v2 runs one milestone at a time, so ready, file-disjoint rows of later milestones sit idle while an earlier milestone works through a serial chain

## Open for discussion

Nothing here is decided. The design is to be settled with the user at planning, before any task is written.

- Whether to add lookahead at all, now that the rolling pool and the compile cap (`max_compile_parallel`) remove part of the idle time the item measured.
- Which rows may run ahead: any ready, file-disjoint row of a later milestone, or only rows that build nothing (docs, agent and skill files).
- What an earlier milestone's gate then tests: a clean checkout of its staged paths, the shared tree with later edits in it, or the shared tree with the last milestone re-gated on the final tree.
- How the per-milestone commit stays exactly the tree its gate passed, and what a later row's compile error does to an earlier gate.
- Whether authoring should cut the cause instead: fewer serial chains from shared files, so milestones drain faster without lookahead.
