<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina-story-planning-round-4 — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | split-http-rs-into-http-module-directory-pre-declare-router-merges-add-delete-work-item | 2026-05-27 | `9419d7e` | Split http.rs into http/ module; pre-declared 11 stubs; added DELETE /work-items/{id} + test. (14 files) |
| E5 | structured-patches-scalars-story-plan-task-spec-task-kind-tier | 2026-05-27 | `050eb9f` | 8 PATCH routes (6 scalars + story-plan + task-spec); folded in tier reader-path defect fix in repo.rs. 4 in-file smoke tests pass. (3 files) |
| E6 | acceptance-criteria-research-notes | 2026-05-27 | `050eb9f` | 7 routes (4 AC + 3 RN with supersession). 2 in-file smoke tests pass. (2 files) |
| E7 | risks-rejected-alternatives-task-dependencies | 2026-05-27 | `050eb9f` | 10 routes including task-dep cycle-422 + edges round-trip. 4 in-file smoke tests pass. (3 files) |
| E8 | open-questions-findings-activity-context-blocks-readiness-dispatch | 2026-05-27 | `050eb9f` | 13 routes (5 OQ + 4 findings + 1 activity + 3 CB + 2 readiness/dispatch). 5 in-file smoke tests pass. (5 files) |
| E13 | split-api-ts-extend-schemas-pre-declare-per-family-modules | 2026-05-27 | `d59a712` | Split api.ts into api/ directory (4 impl + 12 stubs + 1 barrel). Added TaskKind/Tier/RiskSeverity wire enums. Empirically picked .nullable() over .nullish(). (18 files) |
| E14 | fe-scalars-task-kind-tier-composables | 2026-05-27 | `753d32f` | T8a: 6 scalar PATCH wrappers + useScalars composable; module-singleton pattern. (2 files) |
| E15 | fe-story-plan-task-spec-readiness-dispatch-composables | 2026-05-27 | `753d32f` | T8b: setStoryPlan + setTaskSpec + fetchReadiness + fetchDispatchPlan; 4 composables; NextActionSchema mirrors all 16 variants. (6 files) |
| E16 | fe-acceptance-criteria-research-notes-composables | 2026-05-27 | `753d32f` | T9: 4 AC + 3 RN wrappers + 2 composables. Fixed AcceptanceCriterionSchema -> AcceptanceCriterionWireSchema re-export name. (4 files) |
| E17 | fe-risks-rejected-task-deps-composables | 2026-05-27 | `753d32f` | T10: 4+4+4 wrappers + 3 composables; cycle-422 envelope parsed and surfaced via useTaskDependencies.cycleError. (6 files) |
| E18 | fe-open-questions-findings-composables | 2026-05-27 | `753d32f` | T11a: 5+4 wrappers + 2 composables. (4 files) |
| E19 | fe-activity-context-blocks-composables | 2026-05-27 | `753d32f` | T11b: 1+3 wrappers + 2 composables. (4 files) |
| E21 | bun-tests-scalars-ac-research-open-questions-risks-rejected | 2026-05-27 | `af84c4e` | T12a: 6 bun-test files, 55 new tests. Added compilerOptions.paths to root tsconfig.json so bun's resolver honours @/api alias. (7 files) |
| E22 | bun-tests-task-deps-findings-activity-context-blocks-readiness | 2026-05-27 | `af84c4e` | T12b: 5 bun-test files, 86 new tests. Cycle-422 parse showcase for task-deps. NextAction enum exhaustiveness via test.each over 16 variants. (5 files) |
| E23 | update-claude-md-route-catalogue-plugin-skill-md-backlink | 2026-05-27 | `bc8728f` | T13: added ## HTTP routes catalogue to lumina/CLAUDE.md, MCP SKILL.md backlink, repo-root CLAUDE.md cross-reference. Skipped lumina/web/CLAUDE.md per documented judgment call. (3 files) |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E3 | Phase-2 per-family smoke tests written as in-file mod tests rather than lumina/tests/e2e.rs extensions | 2026-05-27 | — | Four parallel implementer agents cannot safely append to the same e2e.rs file — file-ownership-is-absolute would have forced serialisation of T2/T3/T4/T5. Each agent instead wrote its smoke tests as a `#[cfg(test)] mod tests` block at the bottom of its own family file. Test count unchanged; coverage equivalent; parallelism preserved. | — |
| E4 | Folded pre-existing tier reader-path defect fix into Phase 2 (out of plan scope but blocked round-4 deliverable) | 2026-05-27 | — | T2's implementer surfaced that repo::list_work_items and repo::get_work_item_detail did NOT project the tier column added in migration 0006 — both hardcoded `tier: None`. PATCH /tier would succeed on write but the re-fetch returned `tier=null`. Two-line fix per call site directly unblocks the round-4 FE binding; deferring would propagate broken state. Regenerated lumina/.sqlx/ offline cache. | — |
| E20 | Per-family files re-export schemas from work-items.ts rather than moving them out | 2026-05-27 | `753d32f` | Wave-1 parallelism required nobody touch work-items.ts (single-file contention across T8a/T9/T10). Each per-family file re-exports the schema rather than moving it; consumers resolve identically through the @/api barrel. Move is a future cleanup; no consumer-facing impact. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-27 | 26 entries: status-transition × 2, task-completion × 15, deviation × 3, verification × 6 | 050eb9f, 753d32f, 9419d7e, af84c4e, bc8728f, d59a712 |
