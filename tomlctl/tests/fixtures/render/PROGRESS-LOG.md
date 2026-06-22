<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Harness Progressive-Disclosure Wave 2 — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | extract-apply-dependency-sort | 2026-05-20 | `27d2dfd` | 1 file |
| E3 | extract-apply-rollback-protocol | 2026-05-20 | `27d2dfd` | 1 file |
| E4 | extract-apply-constraints | 2026-05-20 | `27d2dfd` | 1 file |
| E5 | extract-apply-vet-flow-implement-lite | 2026-05-20 | `27d2dfd` | 1 file |
| E6 | prune-shared-blocks-manifest | 2026-05-20 | `caeaf58` | 1 file |
| E7 | update-blocks-verify-shell-hashes-test | 2026-05-20 | `caeaf58` | 1 file |
| E8 | skeletonise-optimise | 2026-05-20 | `6a44cd0` | 1 file |
| E9 | skeletonise-optimise-apply | 2026-05-20 | `6a44cd0` | 1 file |
| E10 | skeletonise-review-apply | 2026-05-20 | `6a44cd0` | 1 file |
| E11 | skeletonise-plan-update | 2026-05-20 | `6a44cd0` | 1 file |
| E12 | skeletonise-test-bootstrap | 2026-05-20 | `6a44cd0` | 1 file |
| E13 | refresh-claude-md-prose | 2026-05-20 | `6a44cd0` | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E18 | Superseded the separator-width deviation: documented the width-match as the canonical rule in the schema skill | 2026-05-21 | `bb22cc3` | The skill is the format-reference spec; the canonical width rule belongs there, not buried in a deviation | E17 |
| E19 | Emitted a single trailing newline at EOF rather than the historical double blank line | 2026-05-21 | `cc33dd4` | A single trailing newline is the POSIX text-file convention and keeps render-then-render byte-identical | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| E20 | Deferred the flow-history migrate command | add-flow-migrate-command | 2026-05-20 | Out of scope for the initial overhaul; no in-flight flows need migrating yet | A user reports lost flow history on registry adoption |
| E21 | Deferred the GitLab/self-hosted repo-slug host discriminator | widen-repo-slug-host | 2026-05-21 | GitHub-only slug shape is a user-accepted trade-off for the current scope | A move to GitLab or a self-hosted host is scheduled |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-20 | 18 entries: status-transition × 2, task-completion × 12, verification × 2, deviation × 1, deferral × 1 | 27d2dfd, 6a44cd0, aa11bb2, caeaf58 |
| 2026-05-21 | 3 entries: deviation × 2, deferral × 1 | bb22cc3, cc33dd4 |
