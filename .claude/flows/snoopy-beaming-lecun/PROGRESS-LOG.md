<!-- Generated from execution-record.toml. Do not edit by hand. -->

# tomlctl file auto-creation + PROGRESS-LOG.md rendering — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E5 | t1-auto-create-write-pipeline | 2026-06-22 | `ee84c47` | 6 files |
| E8 | t2-thread-created-through-dispatch | 2026-06-22 | `c5a32b0` | 2 files |
| E10 | t3-flow-render-progress-log | 2026-06-22 | `bf6cf57` | 10 files |
| E15 | t4-tomlctl-skill-doc | 2026-06-22 | `b9c9c8f` | 1 file |
| E16 | t5-execution-record-schema-doc | 2026-06-22 | `b9c9c8f` | 1 file |
| E17 | t6-ledger-contract-docs | 2026-06-22 | `b9c9c8f` | 3 files |
| E18 | t7-plan-update-carrier | 2026-06-22 | `b9c9c8f` | 1 file |
| E19 | t8-implement-tdd-carriers | 2026-06-22 | `b9c9c8f` | 2 files |
| E20 | t9-review-optimise-carriers | 2026-06-22 | `b9c9c8f` | 2 files |
| E21 | t10-claude-md | 2026-06-22 | `b9c9c8f` | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E4 | Widened flow::time module to pub(crate) so the dispatch-layer seed helper can call today_toml_date | 2026-06-22 | `ee84c47` | Widened flow/mod.rs to pub(crate) mod time, a zero-behaviour visibility change the plan design presupposed; cheaper than reinventing the fallible date primitive, which constraint 4 forbids. | — |
| E9 | Renderer emits a uniform spec-conformant GFM table separator; golden fixture updated to match the real PROGRESS-LOG.md hand-authored off-by-one Deferrals separator. | 2026-06-22 | `bf6cf57` | The real file's Deferrals separator row was off-by-one short on two columns, a hand-authored quirk not derivable from any rule. Per the plan instruction that the spec wins, the renderer emits the consistent width-matched separator everywhere and the committed golden fixture copy was updated to the spec-conformant render; that one separator region is the sole divergence from the source file. | — |
| E14 | Fixed three pre-existing envelope-assertion tests T2 missed; surfaced at Phase-3 final verification after a Batch-2 checkpoint false-pass. | 2026-06-22 | `7649460` | T2 left three pre-existing add-many/array-append integration tests asserting the exact pre-change envelope string; the Batch-2 checkpoint verification agent false-passed (summarised one test binary), so they surfaced only at Phase-3 final verification. Loosened the three assertions to the stable ok + count fragments. | — |
| E22 | Corrected T6 ledger-schema note: flow-less .claude/reviews/<scope>.toml ledgers seed an empty doc, not the schema_version skeleton. | 2026-06-22 | `25fa600` | The seed is basename-keyed; only the four canonical basenames seed schema_version=1, so the flow-less <scope>.toml variants actually seed an empty doc. Surfaced by the post-merge E2E smoke; corrected the note to state the basename-keyed behaviour accurately. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-22 | 23 entries: status-transition × 2, verification × 7, deviation × 4, task-completion × 10 | 25fa600, 7649460, b9c9c8f, bf6cf57, c5a32b0, ee84c47 |
