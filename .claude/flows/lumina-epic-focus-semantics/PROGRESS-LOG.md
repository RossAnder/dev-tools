<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina epic/focus semantics — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | t1-write-migration-0010 | 2026-05-29 | `ac63bd9` | 1 file |
| 2 | t2-rename-feature-to-focus | 2026-05-29 | `ac63bd9` | 18 files |
| 3 | t3-shape-plumbing | 2026-05-29 | `f30bd26` | 3 files |
| 4 | t4-epic-focus-attr-split | 2026-05-29 | `7f3b117` | 2 files |
| 5 | t5-create-transition-gates | 2026-05-29 | `b5d559d` | 3 files |
| 6 | t6-mcp-tools | 2026-05-29 | `b63b6d8` | 3 files |
| 7 | t7-http-routes | 2026-05-29 | `f61c1b6` | 2 files |
| 8 | t8-regenerate-sqlx-cache | 2026-05-29 | `5fad4b2` | .sqlx cache |
| 9 | t9-author-plugin-skills | 2026-05-29 | `bbe18c4` | 4 files |
| 10 | t10-update-catalogue-docs | 2026-05-29 | `bbe18c4` | 4 files |
| 11 | t11a-spa-rename-api-fields | 2026-05-29 | `7a7c7e3` | 9 files |
| 12 | t11b-spa-inline-editors | 2026-05-29 | `f3dba0d` | 12 files |
| 13 | t12-tests | 2026-05-29 | `a5b8687` | 20 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E5 | export.rs has no production WorkItem SELECT; shape reaches export via get_work_item_detail delegation | 2026-05-29 | `f30bd26` | render_work_item delegates to get_work_item_detail, so shape serialises through the existing toml conversion; only a test literal needed shape:None | — |
| E8 | create-gate ripple: import.rs needed epic-outcome + a close-criterion (not just shape); broad fixture impact deferred to T12 | 2026-05-29 | `b5d559d` | import_flow's synthetic epic needs an outcome + close-criterion or import breaks the story gate; every hierarchy fixture now fails the gates so the T12 sweep is wider than the 3 named files; 3 mcp.rs literals folded into T6; list_acceptance_criteria made pub | — |
| E16 | T11a left the SPA WorkItem read schema without shape; T11b added a nullable shape field | 2026-05-29 | `f3dba0d` | the Rust WorkItem serialises shape as a top-level column but the SPA read interface/schema omitted it, so item.shape would not typecheck and the picker could not reflect current state | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-29 | 23 entries: status-transition × 2, task-completion × 13, deviation × 3, verification × 5 | `5fad4b2`, `7a7c7e3`, `7f3b117`, `a5b8687`, `ac63bd9`, `b5d559d`, `b63b6d8`, `bbe18c4`, `f30bd26`, `f3dba0d`, `f61c1b6` |
