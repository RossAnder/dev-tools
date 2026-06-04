<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Repo clone-directory & path resolution — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E1 | add-the-local-path-column | 2026-06-04 | `d960f9b` | 1 file |
| E2 | repolink-local-path-field-fromrow-select-plumbing | 2026-06-04 | `5edb2e9` | 2 files |
| E3 | path-resolution-functions | 2026-06-04 | `ae0cf54` | 1 file |
| E4 | set-repo-local-path-mutator-http-mirror-settings-read-endpoint | 2026-06-04 | `7e22deb` | 4 files |
| E5 | spa-repo-detail-local-path-field-offer-to-clone-affordance | 2026-06-04 | `01370d3` | 5 files |
| E6 | tests | 2026-06-04 | `12aca2f` | 2 files |
| E7 | docs | 2026-06-04 | `1a40a4c` | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E15 | resolve_repo_path lexically cancels `..` against pushed components (clamped at base) instead of dropping every `..` | 2026-06-04 | — | Review R5: drop-by-default gave a surprising base/a/b for a/../b on a real filesystem path; pop-on-ParentDir clamped at base yields base/b. Security clamp invariant (rel can never escape base) preserved. | — |
| E16 | Normalisation split into structural (case-preserved, stored) vs compare (case-folded, matching); set_repo_local_path stores the structural form | 2026-06-04 | — | Review R7: storing the case-folded compare form lost operator casing in detail/export. Structural form preserves casing; matching unchanged (compare folds both sides). | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-04 | 16 entries: task-completion × 7, verification × 6, status-transition × 1, deviation × 2 | 01370d3, 12aca2f, 1a40a4c, 5edb2e9, 7e22deb, ae0cf54, d960f9b |
