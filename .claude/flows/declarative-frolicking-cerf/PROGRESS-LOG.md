<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina data-layer seam + findings-queues — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | a1-db-seam | 2026-06-01 | `7807b2f` | 2 files |
| 2 | a2-fromrow | 2026-06-01 | `ab5d558` | 1 file |
| 3 | a3-record-event-unique-violation | 2026-06-01 | `387a0fd` | 1 file |
| 4 | a4-work-items | 2026-06-01 | `0dd4045` | 2 files |
| 5 | a5-findings | 2026-06-01 | `01aef36` | 1 file |
| 6 | a6-context-activity-acceptance | 2026-06-01 | `e91812e` | 1 file |
| 7 | a7-research-open-questions | 2026-06-01 | `baac31c` | 1 file |
| 8 | a8-repo-links | 2026-06-01 | `b593302` | 1 file |
| 9 | a9-risks-rejected-alts-task-deps | 2026-06-01 | `3363845` | 1 file |
| 10 | a10-task-planning | 2026-06-01 | `2bfeef3` | 1 file |
| 11 | a11-pty-module | 2026-06-01 | `200b349` | 1 file |
| 12 | a12-remaining-sites-state-swap | 2026-06-01 | `d57ae81` | 25 files |
| 13 | a13-feature-trim | 2026-06-01 | `2bb8b0e` | 3 files |
| 14 | b14-migration-0011 | 2026-06-01 | `bc0d198` | 4 files |
| 15 | b15-domain-structs | 2026-06-01 | `95063b9` | 1 file |
| 16 | b16-tx-helpers-read-structs | 2026-06-01 | `2e5ae66` | 3 files |
| 17 | b17a-add-findings | 2026-06-01 | `a3546bd` | 2 files |
| 18 | b17b-create-work-items | 2026-06-01 | `8c715d7` | 1 file |
| 19 | b17c-batch-update-findings | 2026-06-01 | `fcdc89a` | 1 file |
| 20 | b18-mcp-batch-tools | 2026-06-01 | `84e86bd` | 1 file |
| 21 | b19-http-batch-routes | 2026-06-01 | `b73f182` | 2 files |
| 22 | b20-query-findings | 2026-06-01 | `f41a55d` | 1 file |
| 23 | b21-mcp-query-tools | 2026-06-01 | `f0d38a9` | 1 file |
| 24 | b22-http-query-routes | 2026-06-01 | `28a1b05` | 2 files |
| 25 | b23-repo-domain-fns | 2026-06-01 | `0c252c8` | 1 file |
| 26 | b24-mcp-domain-tools | 2026-06-01 | `6927ea8` | 1 file |
| 27 | b25-http-domain-routes | 2026-06-01 | `dca979d` | 3 files |
| 28 | b26-bulk-e2e-tests | 2026-06-01 | `feb42d2` | 1 file |
| 29 | b27-documentation | 2026-06-01 | `947b0fc` | 3 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E10 | Also converted shared helper work_item_kind (1 site beyond the 26 planned; repo.rs macros 135->108) | 2026-06-01 | `0dd4045` | work_item_kind is called by 6+ A4 fns; once they flip to &impl DbClient they cannot pass it a &SqlitePool, so it had to become generic and its one macro converted to keep A4 compiling. Trivial single-scalar read, behaviour unchanged; A10 now finds it already done. Out-of-scope callers still compile via impl DbClient for SqlitePool. | — |
| E11 | get_work_item_detail kept its &SqlitePool signature; both its macros still eradicated in place | 2026-06-01 | `0dd4045` | get_work_item_detail calls 9 not-yet-converted list helpers (A5/A6/A7/A9/A10) that still take &SqlitePool; flipping its signature would force converting all 9. Both its macros were converted in place via SqlitePool: DbClient. The signature flip is deferred to A12, where the AppState Arc<SqlitePool>->Arc<AnyPool> swap forces it together with the read helpers. | — |
| E52 | A12 also flipped 10 domain repo fns from &SqlitePool to &impl DbClient, completing the E11-deferred seam-signature conversion | 2026-06-01 | `d57ae81` | 10 domain repo fns (get_work_item_detail, create_work_item(_with_origin), set_epic/focus_plan, find_project_ancestor, set_finding_repo, compute_task_batches, get_task_dispatch_plan, get_story_readiness) were still typed &SqlitePool — their macros were eradicated in earlier waves but signatures were kept per deviation E11, which deferred the flip to A12. Flipped all 10 to &impl DbClient (closed call-graph, compiler-verified) so no domain fn names a concrete backend; Part A seam goal fully met. Minor downstream: concurrency.rs &pool to &*pool, repo.rs SqlitePool import test-gated, and .sqlite() intentionally retained at handlers mixing seam + raw helpers. Behaviour-preserving; 236 tests green. | — |
| E71 | add_findings touched lumina/src/import.rs beyond the plan's repo.rs-only file scope | 2026-06-01 | `a3546bd` | The additive NewFinding.run_id field (needed to stamp findings.run_id, the only write path that can set it) broke import.rs's exhaustive NewFinding literal (E0063); run_id: None there is forced and unambiguous, and the crate does not compile without it. | — |
| E89 | Pinned the plan-unspecified spawn/resolve/triage semantics of record_finding_decision at implement time. | 2026-06-01 | `0c252c8` | Orchestrator design: spawn_task/spawn_story create a child task/story under the finding host work_item (stamping spawned_from_finding_id); title = finding.summary or a fallback; Resolve delegates to resolve_finding with Disposition::Fixed; triage_state maps spawn/resolve to accepted, defer to deferred, dismiss to dismissed. No DB-constraint or compile conflict arose; the plan-pinned assertion (spawn_task sets spawned_from_finding_id + triage_state=accepted + a finding_decisions row) holds. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-01 | 98 entries: status-transition × 2, task-completion × 29, verification × 62, deviation × 5 | 01aef36, 0c252c8, 0dd4045, 200b349, 28a1b05, 2bb8b0e, 2bfeef3, 2e5ae66, 3363845, 387a0fd, 6927ea8, 7807b2f, 84e86bd, 8c715d7, 947b0fc, 95063b9, a3546bd, ab5d558, b593302, b73f182, baac31c, bc0d198, d57ae81, dca979d, e91812e, f0d38a9, f41a55d, fcdc89a, feb42d2 |
