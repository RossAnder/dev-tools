<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina-project-repo-links — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | add-migration-0004-and-domain-types | 2026-05-25 | `a0728d2` | 9 files |
| E3 | add-repo-crud-functions-find-project-ancestor-helper | 2026-05-25 | `845357b` | 14 files |
| E4 | extend-git-export-rendering | 2026-05-25 | `c2167a5` | 0 files (no-op — serde derives + tables-last ordering cover the surface) |
| E5 | add-mcp-tools | 2026-05-25 | `c2167a5` | 2 files |
| E6 | add-http-endpoints | 2026-05-25 | `c2167a5` | 1 file |
| E8 | e2e-test-repo-links-thread | 2026-05-25 | `5dda2c4` | 1 file |
| E9 | update-api-client-zod-schemas | 2026-05-25 | `5dda2c4` | 1 file |
| E10 | docs-lumina-claude-md-agent-skill-md-inline-tool-descriptions | 2026-05-25 | `5dda2c4` | 2 files |
| E12 | add-userepolinks-composable-repolinkspanel-repotag-util | 2026-05-25 | `3f0af98` | 5 files |
| E13 | mount-repolinkspanel-in-focuslens-for-project-kind-items | 2026-05-25 | `76608b4` | 1 file (lite dispatch) |
| E20 | full-verification-sweep | 2026-05-25 | `76608b4` | T10 — Phase 3 verification entries E14-E19 |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E7 | Skipped the plan's PATCH /findings/{id} repo_id thread (option B) | 2026-05-25 | `c2167a5` | No PATCH /findings/{id} route exists in http.rs today; repo::update_finding's UPDATE statement does not cover repo_id either. Composing repo::update_finding + repo::set_finding_repo across two transactions would violate the single-mutation-path invariant documented at repo.rs:1-12 (one domain write ⇒ one event row, atomically). UpdateFindingRequest.repo_id (T1) is for the MCP update_finding tool (T4 wired it). Recommendation: follow-up plan-update to introduce a transactionally-coherent HTTP findings surface | — |
| E11 | T11 docs edits landed in root CLAUDE.md not lumina/CLAUDE.md | 2026-05-25 | `5dda2c4` | lumina/CLAUDE.md is a 32-line stub containing only the <!-- TEST-BOOTSTRAP:STACK --> marker block; the long MCP-tool-surface paragraph described in the T11 spec lives in the repo-root CLAUDE.md under ## lumina > ### MCP tool surface & the agent skill. Applied edits to the only location the content can live today | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-25 | 21 entries: status-transition × 2, task-completion × 11, deviation × 2, verification × 6 | `3f0af98`, `5dda2c4`, `76608b4`, `845357b`, `a0728d2`, `c2167a5` |
