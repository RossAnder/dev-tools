<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina — flow-tracking platform (vertical slice) — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | scaffold-the-lumina-crate | 2026-05-22 | `a796219` | crate skeleton, frozen composition root, 11 module stubs, /api/health |
| 2 | sqlite-schema-migrations-db-module | 2026-05-22 | `3191c46` | schema + hierarchy triggers + events outbox + db::init |
| 3 | repository-layer-event-write-discipline | 2026-05-22 | `c01b34f` | sole mutation path (tx + record_event), AppError, .sqlx cache |
| 4 | axum-json-api-spa-host | 2026-05-22 | `40a9e1e` | work-items CRUD + nested tree + SPA fallback (200) |
| 5 | mcp-server-rmcp-streamable-http | 2026-05-22 | `a447274` | StreamableHttpService, 4 tools over repo, loopback default |
| 6 | git-export-materialiser-transactional-outbox | 2026-05-22 | `d91b0f7` | export_pending drain → atomic TOML snapshot, idempotent/crash-safe |
| 7 | vue-app-scaffold-api-store-layer | 2026-05-22 | `868d3bd` | Vue 3.5/Vite 8/router/Pinia, api.ts, hierarchy store, /api proxy |
| 8 | minimal-flow-importer-one-flow | 2026-05-22 | `cf915e5` | import-flow subcommand, type-filter, repo::create_finding, fixture |
| 9 | hierarchy-tree-view-detail-panel | 2026-05-22 | `6aa141f` | recursive TreeItem + HierarchyView detail panel |
| 10 | end-to-end-test-sqlx-offline-prep-docs | 2026-05-22 | `14037ac` | in-process e2e thread test, CLAUDE.md lumina docs |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E3 | Pinned sqlx to 0.9 instead of the plan's 0.8.x | 2026-05-22 | `a796219` | Installed sqlx-cli is 0.9.0; the .sqlx offline-cache format must match between the prepare CLI and the macro crate. | — |
| E5 | Hierarchy trigger uses static RAISE messages, not interpolated kind names | 2026-05-22 | `3191c46` | SQLite RAISE(ABORT, ...) accepts only a static string literal; two distinct messages preserve diagnostic intent. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-22 | 17 entries: status-transition × 2, task-completion × 10, deviation × 2, verification × 3 | `14037ac`, `3191c46`, `40a9e1e`, `6aa141f`, `868d3bd`, `a447274`, `a796219`, `c01b34f`, `cf915e5`, `d91b0f7` |
