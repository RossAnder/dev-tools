<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina-ui-design-embed — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | T1 — Extend api.ts wire types | 2026-05-25 | `4865d3c` | Extended api.ts wire types to match Rust domain.rs ground truth |
| E3 | T2 — Install fonts + embed design tokens + wire main.css | 2026-05-25 | `4865d3c` | Installed Fontsource trio, embedded @theme tokens + raw vars, wired main.css |
| E4 | T3 — Extend useHierarchy with focus model | 2026-05-25 | `4865d3c` | Reshaped useHierarchy into focus model (focusId/setFocus, view, focusPath, descendantCounts, treeStatus) |
| E5 | T4 — Status/effort/kind helper composable | 2026-05-25 | `4865d3c` | Added useDisplay helpers with STATUSES single source of truth |
| E6 | T5 — Polish index.html | 2026-05-25 | `4865d3c` | Polished index.html (lang=en, title, amber-L favicon.svg replacing favicon.ico) |
| E9 | T6 — AppHeader.vue | 2026-05-25 | `e0470f6` | Brand mark, disabled JUMP TO command bar, DRAFT/0 AGENTS/date pills |
| E10 | T7 — AppFooter.vue | 2026-05-25 | `e0470f6` | Reactive breadcrumb buttons from focusPath + verbatim keyboard hint string |
| E11 | T8 — StatusPill.vue | 2026-05-25 | `e0470f6` | Shared component consuming useDisplay statusLabel/statusToken |
| E12 | T9 — HierarchySpine + SpineNode | 2026-05-25 | `e0470f6` | Left rail with treeStatus branching, ancestors+focused+siblings list, rotated diamond marker with halo |
| E13 | T10 — CenterToolbar + Breadcrumbs | 2026-05-25 | `e0470f6` | FOCUS/TREE toggle bound via setView helper + focused-node context tag; focusPath nav reusing setFocus |
| E14 | T11 — FocusLens.vue | 2026-05-25 | `b33aaa3` | Hero card: corner brackets, Instrument Serif/Geist titles, 4-KPI grid, null-safe progress bar, read-only AC checklist (checked===1) |
| E25 | T12 — ChildGrid + ChildCard | 2026-05-25 | `b33aaa3` | STATUSES-derived filter tabs, auto-fill 280px grid, recursive childCountFor; task effort badge / non-task child count pill (rewrite of lost E15 entry after Windows atomic-rename glitch) |
| E16 | T13 — PortfolioEmpty.vue | 2026-05-25 | `b33aaa3` | treeStatus-branched portfolio view; Plan/Dispatch/Observe headline; 4-KPI portfolio rollup via countWhere helper; ChildGrid for root epics |
| E17 | T14 — Replace root layout; delete obsolete | 2026-05-25 | `23b624f` | App.vue rewritten as 3-row × 3-column shell; deleted HierarchyView.vue, TreeItem.vue, logo.svg; added two latent strict-mode narrowing fixes |
| E24 | T15 — Full verification | 2026-05-25 | — | All 5 verification commands passed (type-check, build, woff2 count ≥ 7, title in dist/index.html, backend cargo build) |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E7 | T1 — Three Rust ground-truth divergences from plan TS shapes | 2026-05-25 | `4865d3c` | Rust domain.rs serialises checked as i64 (SQLite INTEGER mirror) and status/state as Option<String>. Boolean would mistype every wire fetch. Followed Rust ground truth per plan-deviation protocol; downstream T11 AC checklist must use checked===1 not boolean truthiness | — |
| E8 | T2 — --color-accent-glow uses design alpha-channel form rather than plan-suggested lighter hue | 2026-05-25 | `4865d3c` | Design styles.css names --accent-glow: oklch(0.78 0.13 70 / 0.18) — a translucent overlay of the base accent. Plan-deviation protocol prefers design's actual value over plan's suggestion | — |
| E18 | T14 — Latent strict-mode TS errors in Wave 2 components surfaced when App.vue first consumed them | 2026-05-25 | `23b624f` | vue-tsc with noUncheckedIndexedAccess flagged path[path.length-1] as possibly undefined in CenterToolbar.vue:19 and HierarchySpine.vue:57 despite the prior length-0 early-return guard. T14 escalated; orchestrator applied two non-null assertions (semantically inert) to unblock the gate | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|

_None recorded — every plan item completed in this flow._

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-25 | Wave 1 (T1-T5) — wire types extension, design tokens, focus-model composable, display helpers, browser-shell polish | `4865d3c` |
| 2026-05-25 | Wave 2 (T6-T10) — AppHeader/Footer, StatusPill, HierarchySpine + SpineNode, CenterToolbar + Breadcrumbs | `e0470f6` |
| 2026-05-25 | Wave 3 (T11-T13) — FocusLens, ChildGrid + ChildCard, PortfolioEmpty | `b33aaa3` |
| 2026-05-25 | Wave 4 (T14) — App.vue integration, legacy file deletes, strict-mode narrowing fixes | `23b624f` |
| 2026-05-25 | Wave 5 (T15) — verification (type-check, build, woff2 count, title, backend cargo build) — all pass | — |
| 2026-05-25 | Status transition: in-progress → review | — |
