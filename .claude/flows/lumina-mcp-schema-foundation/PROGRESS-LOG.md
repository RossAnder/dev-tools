<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina — MCP + schema foundation — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | additive-migration-0002 | 2026-05-22 | `a51784d` | 1 file |
| 2 | domain-structs-typed-enums | 2026-05-22 | `ed9ad6b` | 1 file |
| 3 | repository-write-paths-read-fold-sqlx-regen | 2026-05-22 | `ed9ad6b` | 2 files |
| 4 | http-read-fold-generic-patch | 2026-05-22 | `79cc128` | 1 file |
| 5 | mcp-domain-tool-surface | 2026-05-22 | `79cc128` | 1 file |
| 6 | git-export-fold | 2026-05-22 | `79cc128` | 1 file |
| 7 | lumina-skill-doc | 2026-05-22 | `a183812` | 1 file |
| 8 | end-to-end-test-claudemd-note-offline-cache-gate | 2026-05-22 | `a183812` | 3 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E8 | Export read deleted_at via a separate cache-safe query! rather than relying on get_work_item_detail to carry it | 2026-05-22 | `79cc128` | deleted_at is not a serialized field on WorkItem/WorkItemDetail; read via a query! byte-identical to a cached entry (no .sqlx regen) and injected at the TOML table top level | — |
| E11 | Made set_story_plan and record_task_activity pub in mcp.rs (outside Task 8's declared file set) so the external e2e crate can drive them | 2026-05-22 | `a183812` | Those tool methods were private; minimal codebase-consistent fix mirrors the already-pub create_work_item | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-22 | 16 entries: status-transition × 2, task-completion × 8, deviation × 2, verification × 4 | 79cc128, a183812, a51784d, ed9ad6b |
