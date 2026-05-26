<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina-story-planning-workflow — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E3 | T1 — Create plugin manifest + skeleton | 2026-05-25 | `be3aa4d` | 2 files (deep dispatch) |
| E4 | T2 — Write CONVENTIONS.md shared contract | 2026-05-25 | `be3aa4d` | 1 file (deep dispatch); see E18 deviation |
| E5 | T3 — Write plugin README | 2026-05-25 | `be3aa4d` | 1 file (deep dispatch) |
| E6 | T4 — Inline thin-wrapper skills (relevance, closure-gate, not-doing) | 2026-05-25 | `6fb0cef` | 3 files (deep dispatch); see E17 deviation (not-doing disabled) |
| E7 | T5 — Inline narrative skills (problem-statement, approach, edge-cases) | 2026-05-25 | `6fb0cef` | 3 files (deep dispatch) |
| E8 | T6 — Interrogation + AC skills | 2026-05-25 | `6fb0cef` | 2 files (deep dispatch); see E21 deviation (line-104 anchor upgrade) |
| E9 | T7 — Forked research-notes skill | 2026-05-25 | `6fb0cef` | 1 file (deep dispatch); see E19 deviation (lens values registered post-hoc) |
| E10 | T8 — Cross-reference lumina/SKILL.md | 2026-05-25 | `fdfe732` | 1 file (lite dispatch); see E20 deviation (catalogue moved into plugin) |
| E11 | T9 — Document plugin load in lumina/CLAUDE.md | 2026-05-25 | `a64ef17` | 1 file (lite dispatch) |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E2 | CONVENTIONS.md cites `entry_type` (not `entry_kind`) for `record_task_activity` | 2026-05-25 | — | Plan §Approach §3 instructed `entry_kind: "execution"`, but lumina's MCP tool surface takes `entry_type` with enum `{execution, vet, comment}`. Plan prose is stale; CONVENTIONS.md §c and all 9 SKILL.md files use the correct `entry_type`. | — |
| E17 | Disabled `lumina:not-doing` skill behind fail-loud abort gates (R1) | 2026-05-25 | — | /review found `mcp__lumina__update_work_item` performs column-level COALESCE on `attributes` (`lumina/src/repo.rs:1404-1421`), not per-key merge — invoking the skill on a story with `problem_statement` or `execution_strategy` set would silently destroy siblings. Disabled until lumina ships a safe attributes-merge MCP tool. | Supersedes E6 |
| E18 | CONVENTIONS.md §g `attributes.not_doing` row marked DISABLED; Notes bullet rewritten (R2) | 2026-05-25 | — | Same COALESCE bug as E17. The §g row's storage-primitive cell was rewritten to DISABLED with a pointer to R1; the Notes bullet enumerates two promotion options. | Supersedes E4 |
| E19 | Appended 5 lens rows to CONVENTIONS.md §g for `/lumina:research-notes` (R3) | 2026-05-25 | — | The research-notes skill body (E9) writes 5 unregistered lens values (`prior-art`, `tool-eval`, `codebase-recon`, `constraint`, `failure-mode`); §g declares itself the SINGLE SOURCE OF TRUTH for lens conventions. Registered the 5 lens values to restore the invariant. | — |
| E20 | Absorbed standalone `claude/skills/lumina/SKILL.md` into the plugin as `/lumina:mcp` (R4) | 2026-05-25 | — | /review R4 flagged the `name: lumina` collision between plugin manifest and standalone skill. User chose path (c): MOVE the catalogue INTO the plugin as a new read-only `mcp` skill (invokable as `/lumina:mcp`), keeping plugin manifest `name: lumina`. CONVENTIONS.md §a updated with a read-only / documentation-skill exception. Standalone `claude/skills/lumina/` directory deleted. | Supersedes E10 |
| E21 | Replaced `user-interrogation` line-104 anchors with `§Planning & decision tools` (R4 side-effect; resolves R7) | 2026-05-25 | — | As part of E20, user-interrogation/SKILL.md needed updated paths; line numbers don't survive file moves so both anchors were upgraded to the section anchor. Incidentally resolves /review finding R7 (stale line 104 reference) — R7 remains `status=open` in the ledger pending user action. | — |
| E22 | Updated repo-root `CLAUDE.md` catalogue cross-reference (OUT-OF-SCOPE) | 2026-05-25 | — | Repo-root CLAUDE.md is outside the flow's `scope` globs but its line-47 catalogue reference would otherwise dangle after E20. Single-line update to `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-25 | 22 entries: status-transition × 2, task-completion × 9, deviation × 7, verification × 4 | `be3aa4d`, `6fb0cef`, `fdfe732`, `a64ef17` |
