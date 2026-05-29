# Plan: Lumina SPA CRUD UI (Work Items + Child Elements)

**Plan path**: `docs/plans/lumina-spa-crud.md`
**Created**: 2026-05-28
**Status**: reviewed (round 1 — all 16 findings merged on 2026-05-28); revised 2026-05-28 to render child-element panels as a horizontal tab strip inside the FocusLens card

## Context

The lumina backend (rust axum + SQLite + MCP) now exposes a fully-developed write surface — 40+ MCP tools mirrored 1:1 by HTTP routes under `/api`, covering work-item CRUD, planning axes (relevance/effort/complexity/closure_gate/task_kind/tier), structured story-plan and task-spec editors, child-element collections (acceptance criteria, research notes, risks, rejected alternatives, findings, open questions, task graph, repo links, activity log, context blocks), and cycle/closure-gate validation. The Vue SPA at `lumina/web/` already has:

- A complete API/types layer — 13 Zod-validated fetch modules under `src/api/`, 21 module-singleton composables under `src/composables/` already exposing `add`/`update`/`remove`/`supersede` methods for every collection.
- A read-only display surface — 3-column grid (`HierarchySpine` / `FocusLens` + `ChildGrid` / deferred right column), with all child-element data rendered inline inside `FocusLens`.
- One existing CRUD example — `RepoLinksPanel.vue` (single-field inline form) — but no reusable form primitives (Input, Textarea, Select, Button, Modal).

The gap is purely UI. This plan binds the existing composable surface to a coherent set of edit affordances so a human can manage every work-item field and child collection from the SPA, phased across four waves while staying consistent with the current visual language (dark Tailwind v4 palette + token CSS + corner-bracket accents). No new backend endpoints, no new wire types, no design-system overhaul.

## Scope

**In scope:**
- Test-infrastructure addition: Vitest + @vue/test-utils alongside bun test (T0) — required because bun test cannot mount `.vue` SFCs.
- Wire+composable seam-extension: add `deleteWorkItem` wire fn + `useHierarchy().updateNode/removeNode` composable methods (T0.5) — currently missing despite backend routes existing.
- Declarative panel registry: `FocusLensPanels.ts` so W2-W4 tasks don't contend on `FocusLens.vue` (T0.5).
- New shared UI primitives: `Modal`, `Input`, `Textarea`, `Select`, `Button`, `ConfirmAction`, `Tabs`, `TabPanel` (under `src/components/ui/`).
- New per-feature CRUD components: `WorkItemForm` (create + edit), enum scalar pickers, structured-attribute editors (story plan, task spec, epic/feature context), per-collection panels (AC, research notes, risks, rejected alternatives, findings, open questions, task dependencies, context blocks, activity log).
- Wiring those components into `FocusLens` and a small toolbar on the focused item.
- `AppHeader.vue` "+ New project" button + `PortfolioEmpty.vue` empty-state prose update.
- Two-step inline confirm for all destructive actions; inline validation-error display next to the originating control; global error banner preserved for network failures.

**Out of scope:**
- Vue-router / new top-level navigation.
- A design-system primitive library beyond the small set above.
- Authentication / authorisation UI.
- PTY supervisor (`PtyConsole.vue`) — already interactive; not part of work-item CRUD.
- Sprint composer / dispatch UI — deferred per existing memory notes.
- Migration of `RepoLinksPanel` to the modal pattern — keep as the lightweight inline example.

**Affected areas:**
- `lumina/web/src/components/` (mainly additions)
- `lumina/web/src/components/ui/` (new directory)
- `lumina/web/src/composables/` (one or two new small helpers; existing composables untouched)

**Estimated file count:** ~29 new files (incl. T0 vitest.config + T0.5 FocusLensPanels + T1 Tabs/TabPanel), ~5-7 modifications (package.json, CLAUDE.md, useHierarchy.ts, api/work-items.ts, FocusLens.vue, AppHeader.vue, PortfolioEmpty.vue). Distributed across 4 sequential waves preceded by a foundation pair (T0, T0.5).

## Research Notes

_Findings sourced from Phase 2 exploration agents — see Exploration Notes below for raw output references._

- **No Modal/Dialog primitive exists** — must be built from scratch. Tailwind v4 + token palette is fully wired; a ~80-line `Modal.vue` with backdrop, ESC close, focus trap, and a `<teleport to="body">` portal will suffice. No external library (headlessui/radix-vue) — contradicts the minimalist composable pattern. _Impact on plan: T1._
- **HTTP API is 100% mirrored** — every `/api` route enumerated in exploration agent 2's report exists and is consumed by an existing wire-layer module under `src/api/`. _Impact on plan: zero backend work; composables already wrap every write._
- **Composables already cover every write** — but the names diverge from the obvious .add/.update/.remove pattern in several places. Verified signatures: `useAcceptanceCriteria.{add,check,uncheck,remove}`, `useResearchNotes.{add,update,supersede}`, `useRisks.{add,update,supersede,remove}`, `useRejectedAlternatives.{add,update,supersede,remove}`, `useRepoLinks.{add,remove,setPrimary}`, `useOpenQuestions.{add,addOption,blockTaskOnQuestion,setEnablingOption,resolve}`, `useFindings.{add,update,resolve,supersede}`, **`useTaskDependencies.{addEdge,removeEdge}` (NOT .add/.remove)**, `useStoryPlan.set`, `useTaskSpec.set`, **`useActivity.record` (NOT .append)**, `useContextBlocks.{create,link,unlink}`, `useScalars.{setRelevance,setEffort,setComplexity,setClosureGate,setTaskKind,setTier}`. **`useHierarchy` exposes only `createNode` + `changeStatus` + `refresh` — see P2 for the update/remove gap.** All return `Result<T, string>`. Note that `useHierarchy().refresh(id)` is a no-op unless `id === focusId.value` (useHierarchy.ts:189-196), so callers must pass the affected work-item id explicitly. _Impact on plan: components consume these directly; no new composable layer is required except for cross-cutting helpers (e.g., `useConfirmAction`)._
- **Mutations refresh, never optimistic** — existing pattern is mutate → `useHierarchy().refresh(parentId)` → re-render. _Impact on plan: every form's submit handler ends with `await refresh()` after a successful mutation._
- **Error display today is the singleton `error` ref in HierarchySpine** — global banner only. _Impact on plan: per-form local `error` state, fall back to singleton for network/unhandled errors._
- **Closure-gate validation is server-side and returns 422 Validation** — error envelope `{error: {kind: "validation", message: "..."}}`. _Impact on plan: status-transition control catches the error, shows it inline next to the status pill, links to the unchecked AC list._
- **`files_touched` is heterogeneous** — array of `string` OR `{repo, path}` objects; backend rejects unknown repo slugs. _Impact on plan: task-spec editor renders an entry row that toggles between "primary repo path" and "qualified {repo, path}", with repo slug picked from the work-item's project ancestor's `repo_links`._
- **Soft-delete is invisible in tree/list** — `deleted_at IS NULL` filter applied server-side. _Impact on plan: delete UX is fire-and-refresh; no "undelete" UI for v1._

### Exploration Notes (summary)

- **SPA**: Vite 8 + Vue 3.6 Vapor + Tailwind v4; module-singleton composables (no Pinia, no provide/inject, no vue-router); 16 read-only SFCs + `RepoLinksPanel` as the one CRUD example; design tokens in `assets/tokens.css`/`theme.css`; bun test under `src/__tests__/`.
- **HTTP API**: All MCP write tools mirrored — work-items CRUD, six scalar PATCHes, two structured-attribute PATCHes (story-plan, task-spec), per-collection POST/PATCH/DELETE under `/api/*`. Errors uniform `{error: {kind, message, edges?}}`. 422 for validation/cycle, 404 for missing, 204 for delete.
- **Data model**: 5-kind hierarchy (project→epic→feature→story→task); per-kind columns (`relevance` on epic/feature/story; `effort`/`complexity`/`task_kind`/`tier` on task; `closure_gate` on story); 9 child tables (`acceptance_criteria`, `research_notes`, `open_questions`+`question_options`, `risks`, `rejected_alternatives`, `repo_links`, `task_dependencies`, `findings`, `work_item_activity`, `context_blocks`+`work_item_context`); supersession on research_notes/risks/rejected_alternatives/findings; severity vocab split (Findings: Critical|Major|Minor|Suggestion; Risks: Low|Medium|High|Critical) — must not unify in UI.

## User Decisions

| Question | Answer |
|----------|--------|
| Edit affordance pattern | Centered modal overlays for multi-field forms |
| Child-element layout | Horizontal tab strip inside the FocusLens card (one tab per applicable panel); always-visible hero header carries title/body/status/toolbar above the strip |
| Phasing | 4 waves (W1 primitives + core CRUD, W2 scalars + structured editors, W3 child-element CRUD, W4 relational structures) |
| Create flow | `+ Add child` button on FocusLens; child kind auto-determined by parent's hierarchy position; top-level `+ New project` in header |
| Destructive action confirm | Two-step inline confirm (click → "Confirm delete?" → second click commits) |
| Overlay style | Centered modal with backdrop (~480-640px wide depending on content) |
| Child forms | Lightweight inline rows for one-field adds (AC text, task-dep id, context-block link); modal for multi-field adds (research note, risk, finding, question, work item) |
| Closure-gate / validation errors | Inline near the originating control; global banner reserved for network failures |
| RepoLinksPanel | Keep as the lightweight inline example; do not migrate to modal |

## Approach

Four sequential waves, each producing user-visible CRUD coverage. Each wave's components consume the existing composables and Zod-validated wire types unchanged. The modal primitive established in W1 is the foundation for every multi-field editor in W2–W4. Inline two-step confirm (also W1) covers every destructive action across all waves.

**Layout integration:** `FocusLens.vue` is restructured into two stacked regions: an **always-visible hero header** (kind label + id, title, body, status pill, progress bar, planning-domain fields, FocusToolbar) and a **horizontal tab strip** beneath it whose tabs are populated dynamically from the panel registry. Selecting a tab renders the matching `<*Panel>` SFC as the active region; inactive panels are unmounted (lazy mount) to keep DOM lean. The 3-column grid (`HierarchySpine | FocusLens+ChildGrid | deferred right column`) is unchanged — tabs live INSIDE the FocusLens card, not as top-level navigation. KPI tiles (epic only, currently FocusLens.vue:183-208) move into an "Overview" tab; `RepoLinksPanel` (project only) moves into a "Repos" tab; the inline AC block (task only, :238-271) moves into the "Acceptance Criteria" tab built by T10; the inline context-blocks grid (:281-303) moves into the "Context" tab built by T17. The other child collections (research_notes, risks, rejected_alternatives, findings, open_questions, task_dependencies, activity) are NOT currently rendered in FocusLens — those tasks (T11–T16, T18) are **pure additions** of a new tab + `<*Panel>` SFC. Each panel reads from the same `useHierarchy().detail` ref — no new fetch path is introduced.

To eliminate FocusLens.vue merge contention across W2-W4, T0.5 ships a declarative panel registry at `lumina/web/src/components/work-item/FocusLensPanels.ts` (an array of `{id, label, component, when: (kind) => boolean, order?: number}` entries). FocusLens.vue computes `panels.filter(p => p.when(kind)).sort(byOrder)` once and feeds the result into `<Tabs>`. Each subsequent W2-W4 task then adds ONE entry to that array + ships its own SFC, instead of patching FocusLens.vue. `order` provides stable left-to-right tab positioning (Overview=0, Plan=10, Acceptance Criteria=20, Research=30, Risks=40, Rejected Alts=50, Open Questions=60, Findings=70, Task Deps=80, Repos=90, Context=100, Activity=110 — gaps of 10 leave room for future inserts).

**Modal portal:** the modal renders via `<teleport to="body">` so it escapes the 3-column grid stacking context. App.vue gets a `<div id="modal-root">` mount point (or `document.body` directly).

**Validation:** every form holds a local `error` ref. On submit, the composable's `Result<T, string>` is unwrapped: `.ok ? close : setError(result.error)`. The error renders inline; the global singleton remains the fallback for fetch/network exceptions (which already happens automatically since composables set the singleton on `catch`). **NB: composables already call `useHierarchy().refresh(parentId)` internally on success — do NOT add a second refresh in form components.** Verified in useResearchNotes.{add,update,supersede}, useAcceptanceCriteria.add, useRisks.{add,update,supersede,remove} (sampled three; pattern is uniform).

## Verification Commands

```bash
# from repo root
cd lumina/web && npm ci                        # one-time (also installs vitest + @vue/test-utils after T0)
cd lumina/web && npm run build                 # vue-tsc type-check + vite build
cd lumina/web && bun test                      # composable/wire-layer unit tests (.test.ts files)
cd lumina/web && npm run test:component        # SFC component tests via vitest (.spec.ts files; added by T0)
cd lumina && cargo build --manifest-path Cargo.toml         # backend (used for dev)
cd lumina/web && npm run dev                   # local dev server (proxies /api to 127.0.0.1:24817)
```

The dev server (`npm run dev`) is the primary feature-verification surface: each wave's acceptance criteria are exercised manually via the browser with a backed-up `lumina.db` and a small sample project.

## Tasks

### Wave 1: Foundation + work-item core CRUD

#### T0: Install Vitest + @vue/test-utils for SFC component testing

- **Files**: `lumina/web/package.json` (modify — add devDependencies), `lumina/web/vitest.config.ts` (new), `lumina/web/CLAUDE.md` (modify — update TEST-BOOTSTRAP marker block)
- **Action**: Bun test cannot mount `.vue` SFCs (per lumina/web/CLAUDE.md TEST-BOOTSTRAP block: *"Vue SFC (.vue) component rendering is OUT OF SCOPE for this scaffold — Bun has no native Vue compiler ... add Vitest + @vue/test-utils alongside (they coexist with bun test on the same codebase)"*). Install `vitest`, `@vue/test-utils`, `@vitest/coverage-v8`, `jsdom` (or `happy-dom`) as devDependencies. Create `vitest.config.ts` aligned with `vite.config.ts`'s path aliases (`@/* → ./src/*`) and pointing at `jsdom`. Add `"test:component": "vitest run"` and `"test:component:watch": "vitest"` scripts to package.json. Existing `bun test` continues to gate composable/wire-layer tests under `src/__tests__/*.test.ts`; vitest gates SFC tests under `src/__tests__/*.spec.ts` (suffix differentiates the two runners cleanly).
- **Depends on**: none
- **Acceptance**: `npm run test:component` exits 0 against an empty suite; `bun test` continues to pass against existing tests; CLAUDE.md TEST-BOOTSTRAP marker block updated to list both runners and the `.test.ts` (bun) vs `.spec.ts` (vitest) convention.
- **Effort**: S

#### T0.5: Extend wire + composable layer for work-item update/delete; add FocusLensPanels registry

- **Files**: `lumina/web/src/api/work-items.ts` (modify — add `deleteWorkItem(id)` POST/DELETE wrapper), `lumina/web/src/composables/useHierarchy.ts` (modify — add `updateNode(id, patch)` and `removeNode(id)` methods), `lumina/web/src/components/work-item/FocusLensPanels.ts` (new)
- **Action**: Two concerns, one task because both are mechanical seam-extensions that every downstream wave depends on:
  - **Wire+composable extension**: `api/work-items.ts` currently has NO DELETE method (grep returns 0 matches) despite the backend route `DELETE /work-items/{id}` existing. Add `deleteWorkItem(id: string): Promise<void>` using the existing `handle()` / `handleVoid()` pattern. `useHierarchy.ts` currently exports only `createNode`, `changeStatus`, `refresh` — no general `update` or `remove`. Add `updateNode(id, patch: UpdateWorkItemBody): Promise<Result<WorkItem, string>>` (wrapping the existing `updateWorkItem` wire fn) and `removeNode(id): Promise<Result<void, string>>` (wrapping the new `deleteWorkItem`). Both mirror `createNode`'s shape: take an `Api` adapter type so `__setApiForTests` keeps working; on success, call `await refresh(id)` (or `refresh(parentId)` for delete where the focused node is gone — set focus to parent first). Update the `Api` type at useHierarchy.ts:47 to include the two new entries.
  - **Declarative panel registry**: Create `FocusLensPanels.ts` exporting an array `panels: Array<{id: string; label: string; component: Component; when: (kind: Kind) => boolean; order: number}>`. `label` populates the tab strip text; `order` sets stable left-to-right tab positioning (Overview=0, Plan=10, Acceptance Criteria=20, Research=30, Risks=40, Rejected Alts=50, Open Questions=60, Findings=70, Task Deps=80, Repos=90, Context=100, Activity=110 — gaps of 10 leave room for future inserts). Empty in T0.5; each W2-W4 task adds one entry. FocusLens.vue (modified in T4) computes `panels.filter(p => p.when(kind)).sort(byOrder)` and feeds the result to `<Tabs>`. This eliminates FocusLens.vue merge contention across W2-W4: every subsequent task that adds a panel only touches FocusLensPanels.ts (a single array append) plus its own SFC.
- **Depends on**: none
- **Acceptance**: `npm run build` passes; `bun test src/__tests__/hierarchy.test.ts` (new or extended) covers updateNode + removeNode round-trips via `__setApiForTests`; FocusLensPanels.ts exists with type-safe export.
- **Effort**: M

#### T1: Build shared UI primitives — `Modal`, `Input`, `Textarea`, `Select`, `Button`, `Tabs`, `TabPanel`

- **Files**: `lumina/web/src/components/ui/Modal.vue`, `Input.vue`, `Textarea.vue`, `Select.vue`, `Button.vue`, `Tabs.vue`, `TabPanel.vue`, `index.ts`
- **Action**: Create seven small SFCs using only existing `var(--*)` tokens and Tailwind utilities. **Modal.vue uses `<script setup>` (VDOM interop), NOT `<script setup vapor>`** — Teleport-under-Vapor is unverified at Vue 3.6.0-beta.12 (4+ open vuejs/core Teleport issues; Vapor still self-documented as 'feature-complete but unstable' per RepoLinksPanel.vue:10-12). Modal: `<teleport to="body">`, backdrop with click-to-close (opt-out via prop), ESC key listener, focus trap, `<slot name="header">`/`<slot>`/`<slot name="footer">`, width prop (`sm|md|lg` → 420/560/720px). Input/Textarea/Select: token-styled, v-model, `error?: string` prop renders inline beneath. Button: 3 variants (`primary|secondary|danger`) all using the corner-bracket motif from `ChildCard`/`SpineNode`.
  - **Tabs.vue**: `<script setup vapor>` is fine here (no Teleport). Props: `modelValue: string` (active tab id), `tabs: Array<{id: string, label: string}>`. Renders a horizontal `role="tablist"` strip styled with the existing mono-uppercase `tracking-[0.18em]` motif (mirrors the `font-mono text-[10.5px]` section labels in FocusLens.vue) with an `accent`-coloured underline on the active tab. Keyboard nav: ← / → cycles, Home / End jump to first/last, Enter/Space activates focused tab. Emits `update:modelValue` on change. Overflow: horizontal scroll with `-webkit-overflow-scrolling: touch`; no dropdown collapse in v1. Provides the active-id via Vue `provide()` under the symbol `TabsActiveKey` so children can read it without prop drilling.
  - **TabPanel.vue**: Props: `name: string`. Reads `TabsActiveKey` via `inject()`; renders its default slot only when `name === active`. Wraps slot content in `<div role="tabpanel">` with the corresponding `aria-labelledby` derived from the tab id. Default slot is lazy (the `v-if` keeps inactive panels unmounted) — panel components don't pay render cost until first visited.
  - Re-export all seven from `index.ts`. Acceptance must include: opening Modal from inside a `<script setup vapor>` parent renders without browser-console warnings; ESC closes; focus restores to the trigger element; Tabs keyboard nav works; inactive TabPanel slot content is not present in the DOM. No external libs.
- **Depends on**: none
- **Acceptance**: `npm run build` passes; an ad-hoc test page (or the existing AppHeader extended with a temporary "open modal" button — to be removed in T5) demonstrates each primitive renders with the dark palette, ESC + backdrop close the modal, and a 3-tab demo Tabs instance cycles via ←/→ keys with the active panel's slot mounting/unmounting on switch.
- **Effort**: M

#### T2: Build `ConfirmAction` inline two-step confirm component

- **Files**: `lumina/web/src/components/ui/ConfirmAction.vue`, `lumina/web/src/components/ui/index.ts` (re-export)
- **Action**: Component with a single `<slot>` (the action label) and a `confirmLabel` prop (default `"Confirm?"`). Internal state: `armed: boolean`. First click → `armed = true` and visually shifts to danger variant showing `confirmLabel`; second click within 4s → emits `@confirm`; click elsewhere or 4s timeout → reset to disarmed. Exposes a slot `pending` for showing busy state during the awaited handler.
- **Depends on**: T1 (uses `Button` styles)
- **Acceptance**: Component shipped with a `__tests__/ConfirmAction.test.ts` covering arm → disarm-on-timeout, arm → fire on second click, arm → disarm on outside click. `bun test` passes.
- **Effort**: S

#### T3: Build `WorkItemForm` modal (create + edit)

- **Files**: `lumina/web/src/components/work-item/WorkItemForm.vue`
- **Action**: Single component that handles both create (with `parentId` + `kind` derived from parent) and edit (with `workItem` prop preloaded). Fields: `title` (Input), `body` (Textarea), `status` (Select bound to `StatusSchema` values), `position` (Input numeric, optional). On submit calls `useHierarchy().create(...)` or `.update(...)`. Local `error` ref renders inline at the modal footer if the composable returns `Err`. On success: close modal + `await refresh()`.
- **Depends on**: T1
- **Acceptance**: From an ad-hoc trigger (or after T5 wires the buttons): creating a new task under a story succeeds and the new child appears in `ChildGrid`; editing a focused item updates the title in FocusLens. Backend reject (e.g., trying to create a `feature` under a `task`) surfaces inline in the modal footer.
- **Effort**: M

#### T4: Wire FocusLens action toolbar + tabbed panel host — Edit, Delete, + Add child, status cycle, tab strip

- **Files**: `lumina/web/src/components/FocusLens.vue` (modify — render the FocusToolbar in the hero header, then a `<Tabs>` populated from `FocusLensPanels.ts`; extract the current epic-only KPI grid and inline AC/context fragments into the Overview / Acceptance Criteria / Context panels per the migration described below), `lumina/web/src/components/work-item/FocusToolbar.vue` (new), `lumina/web/src/components/work-item/panels/OverviewPanel.vue` (new — receives the KPI grid for epic-kind plus a fallback rendering for non-epic kinds; registered at order=0), `lumina/web/src/components/work-item/panels/ReposPanel.vue` (new — thin wrapper around the existing `RepoLinksPanel.vue` so it slots into the tab strip without modification; registered at order=90, when: kind==='project'), `lumina/web/src/components/AppHeader.vue` (modify — add "+ New project" button), `lumina/web/src/components/PortfolioEmpty.vue` (modify — replace stale MCP-tool prose)
- **Action**: Restructure FocusLens.vue so the template is: `<article>` → hero header (kind label, title, body, status pill, progress bar, planning fields, **FocusToolbar**) → `<Tabs v-model="activeTab" :tabs="visibleTabs">` → `<TabPanel v-for="p in visiblePanels" :name="p.id"><component :is="p.component" /></TabPanel>`. `visiblePanels = computed(() => panels.filter(p => p.when(item.kind)).sort((a,b) => a.order - b.order))`. `visibleTabs = computed(() => visiblePanels.value.map(p => ({id: p.id, label: p.label})))`. `activeTab` defaults to `visiblePanels.value[0]?.id` and resets on focus change. Persist last-active tab id per focused work-item in `sessionStorage` keyed by `lumina:focus-tab:<workItemId>` so navigating away and back restores the prior tab (cleared on hard refresh — sessionStorage scope is intentional).
  - **Panel migration from existing FocusLens.vue**: T4 itself ships two panels (OverviewPanel + ReposPanel) so the post-T4 tab strip is non-empty even before W2-W4 land. The current inline AC block (FocusLens.vue:238-271) and inline context-blocks grid (:281-303) STAY in FocusLens.vue temporarily as fallbacks rendered only when no AcceptanceCriteriaPanel / ContextBlocksPanel entry exists in the registry — T10 (AC) and T17 (context) replace them by registering their entries and deleting the fallback fragments in their own modify-FocusLens.vue step. The epic-only KPI grid (:183-208) is **moved** into OverviewPanel.vue by T4 (no fallback) — Overview is the always-present first tab. The disabled 4-action button row (:219-231) is **deleted** by T4 (FocusToolbar replaces it).
  - New `FocusToolbar` rendered at the top of the hero card. Buttons:
  - **Edit** → opens `WorkItemForm` with current item preloaded; on submit calls `useHierarchy().updateNode(id, patch)` (the new method from T0.5).
  - **+ Add child** → opens `WorkItemForm` with `kind` auto-derived from focused item's kind (project→epic, epic→feature, feature→story, story→task; task → button disabled). On submit calls `useHierarchy().createNode(...)` (existing).
  - **Delete** → `ConfirmAction` triggering `useHierarchy().removeNode(id)` (the new method from T0.5) then setting focus to the parent.
  - **Status cycle** → small inline `<Select>` bound to status, kind-aware: containers (project/epic/feature/story) → `['open']` read-only display; tasks → `['todo','in_progress','blocked','done','cancelled']` (full six-value enum per `StatusSchema` in `api/wire-enums.ts:36`). Manually setting `cancelled` is rare but supported as an escape hatch from a stale T15 cascade. On change → `useHierarchy().changeStatus(id, value)` (existing — NOT `updateStatus`). On 422 closure-gate error, the inline error renders directly under the Select (e.g., `"Cannot mark done: 3 acceptance criteria unchecked"`).
  - **Top-level** "+ New project" button added to `AppHeader.vue` — opens `WorkItemForm` in create-mode with `kind: 'project'`.
  - **PortfolioEmpty prose update**: `PortfolioEmpty.vue` today renders *"No work items yet — create your first epic via the MCP `create_work_item` tool."* Replace that line with copy pointing at the header's "+ New project" button (the user now has the UI affordance).
- **Depends on**: T0, T0.5, T1, T2, T3
- **Acceptance**: From the dev server, can create/edit/delete a work item via the buttons; the closure-gate error renders inline when attempting to mark a story `done` with unchecked AC; `AppHeader`'s `+ New project` modal creates a root project and the empty-state transitions to the tree view; PortfolioEmpty prose no longer references the MCP tool. The tab strip is visible beneath the hero header; for an epic focus the `Overview` tab shows the KPI grid as before; for a project focus a `Repos` tab is present and renders the existing RepoLinksPanel content; switching tabs changes which TabPanel slot is mounted (verified via devtools — inactive panels are absent from the DOM); refocusing a work item restores its last-active tab from sessionStorage. Vitest spec (`src/__tests__/FocusToolbar.spec.ts`) covers: toolbar emits createNode with correct child kind; delete arms/fires confirm via ConfirmAction; status cycle dispatches `changeStatus` and renders 422 inline; kind-aware status options match (`['open']` for containers, full set for tasks). A second vitest spec (`src/__tests__/FocusLensTabs.spec.ts`) covers: `visiblePanels` filters by `when(kind)` and sorts by `order`; activating a tab via click + via ←/→ keys both update `activeTab`; sessionStorage restore on focus-change.
- **Effort**: M

#### T5: Cleanup — remove T1 ad-hoc test scaffolding; cover toolbar + WorkItemForm via vitest

- **Files**: `lumina/web/src/__tests__/FocusToolbar.spec.ts` (new — see T4 acceptance; this task formalises coverage), `lumina/web/src/__tests__/WorkItemForm.spec.ts` (new), `lumina/web/src/components/AppHeader.vue` (modify only if T1 used a temporary mount). All tests live under `src/__tests__/` (the existing 18-file convention) — NOT `src/components/__tests__/` which doesn't exist.
- **Action**: Vitest specs (`.spec.ts` suffix differentiates from bun tests' `.test.ts`) using `@vue/test-utils` + `__setApiForTests` stubs. `WorkItemForm.spec.ts` covers create + edit branches and error rendering. `FocusToolbar.spec.ts` formalises the T4 acceptance coverage. Remove any temporary "open modal" button if T1 mounted one in AppHeader.
- **Depends on**: T0 (vitest installed), T4
- **Acceptance**: `npm run test:component` passes; `npm run build` passes; coverage of new code ≥ 80% changed-line per vitest coverage report.
- **Effort**: S

**Per-task testing convention (W2-W4)**: Every task in W2-W4 that ships a new SFC also ships its own vitest spec under `lumina/web/src/__tests__/<feature>.spec.ts` covering (a) happy-path mutation via `__setApiForTests`, (b) backend 422 inline error rendering, (c) post-mutation refresh. The spec file is part of the task's Files block (implicit — the per-task Files lists below do not re-enumerate the spec, but the acceptance criterion is `npm run test:component` passes for the new spec). T19 no longer authors these tests; it only runs the closing suite + hand-verification.

**Per-task panel-registration convention (W2-W4)**: Every task that ships a new `<*Panel>` SFC also appends one entry to `lumina/web/src/components/work-item/FocusLensPanels.ts` with `{id, label, component, when, order}` per the order assignments enumerated in the Approach section (Plan=10, Acceptance Criteria=20, Research=30, Risks=40, Rejected Alts=50, Open Questions=60, Findings=70, Task Deps=80, Context=100, Activity=110). The registry append is implicit in each task's Files block (FocusLensPanels.ts is not re-enumerated per task); the SFC may either be the panel itself or a thin wrapper around an existing component (e.g. T7's StoryPlanEditor is a modal, so T7 also ships a `StoryPlanPanel.vue` that renders the read-view of the same data plus an Edit button opening the modal). Each panel reads from `useHierarchy().detail` for its data — no fetch coupling between panels.

### Wave 2: Scalar pickers + structured attribute editors

#### T6: Build enum scalar pickers — `RelevancePicker`, `EffortPicker`, `ComplexityPicker`, `ClosureGatePicker`, `TaskKindPicker`, `TierPicker`

- **Files**: `lumina/web/src/components/work-item/scalars/{Relevance,Effort,Complexity,ClosureGate,TaskKind,Tier}Picker.vue`, `index.ts`
- **Action**: Each is a small `<Select>` wrapper bound to the matching scalars.ts composable method (`useScalars().setRelevance(id, value)` etc.) AND the matching enum schema from `@/api/wire-enums` (RelevanceSchema, EffortSchema, ComplexitySchema, ClosureGateSchema, TaskKindSchema, TierSchema). Each picker renders only when applicable to the focused item's kind (e.g., `EffortPicker` returns null for non-task). **NB**: TaskKindPicker binds to the three-value set ['foundation','main','polish'] (migration 0007 kebab-case-clean) — do NOT include the legacy 'vertical-slice'/'pattern-replacement' values, which are intra-story groupings handled outside the task-kind column. Inline error on 422 (kind-mismatch errors render under the picker).
- **Depends on**: T1, T4 (FocusLens slot exists)
- **Acceptance**: For a focused story, RelevancePicker + ClosureGatePicker visible; EffortPicker/ComplexityPicker/TaskKindPicker/TierPicker hidden. For a focused task, the opposite. Each picker round-trips state via the backend and reflects updates without page reload.
- **Effort**: M

#### T7: Build `StoryPlanPanel` + `StoryPlanEditor` modal — structured story attributes

- **Files**: `lumina/web/src/components/work-item/panels/StoryPlanPanel.vue`, `lumina/web/src/components/work-item/StoryPlanEditor.vue`
- **Action**: `StoryPlanPanel` is the tab body (registered at `order=10`, `when: kind==='story'`, label `"Plan"`). It renders the read view of `detail.attributes.{problem_statement, research_notes, execution_strategy, not_doing, verification_commands}` as labelled sections (mono-uppercase section headers matching the existing FocusLens motif; verification_commands rendered as a 4-row monospace table for build/test/lint/smoke). An `Edit` button in the panel header opens `StoryPlanEditor`. `StoryPlanEditor` is the modal form (`lg` width): `problem_statement` (Textarea), `research_notes` (Textarea — note this is the *attribute*, not the first-class research_notes table), `execution_strategy` (Textarea), `not_doing` (Textarea), `verification_commands` — 4 separate Input rows (`build`, `test`, `lint`, `smoke`). Submit → `useStoryPlan().set(id, payload)`. Pre-fills from `detail.attributes`. Inline error on 422.
- **Depends on**: T1, T3 (uses Modal + Input + Textarea + Tabs)
- **Acceptance**: For a focused story, a `Plan` tab is present; clicking it shows the read view; the Edit button opens the modal, submission updates the attributes, and the read view re-renders without page reload.
- **Effort**: M

#### T8: Build `TaskSpecPanel` + `TaskSpecEditor` modal — task spec with files_touched repo-qualifier UI

- **Files**: `lumina/web/src/components/work-item/panels/TaskSpecPanel.vue`, `lumina/web/src/components/work-item/TaskSpecEditor.vue`, `lumina/web/src/components/work-item/FilesTouchedEditor.vue`
- **Action**: `TaskSpecPanel` is the tab body (registered at `order=10`, `when: kind==='task'`, label `"Spec"` — note the `order=10` slot for tasks is distinct from the story's Plan slot; they don't overlap because `when` already kind-gates them, but the shared slot keeps the leftmost tab semantically "the structured attributes for this kind"). It renders the read view of `detail.attributes.{execution_detail, outcome, files_touched}` + the typed `tier` column as labelled sections; `files_touched` rows render with a small `repo:` prefix for qualified entries and a bare path otherwise. An `Edit` button opens `TaskSpecEditor`. The editor is a modal: `execution_detail` (Textarea), `outcome` (Textarea), `tier` (Select bound to lite/deep/null), and a `FilesTouchedEditor` for the heterogeneous `string | {repo, path}` array. The editor renders one row per entry, each with a "qualify with repo" toggle. When toggled, a repo-slug `<Select>` populated from the project ancestor's `repo_links` is shown (the primary repo collapses back to bare-string form on save). Add row / Remove row buttons. Submit → `useTaskSpec().set(id, payload)`.
- **Depends on**: T1
- **Acceptance**: For a focused task, a `Spec` tab is present; the editor saves a mixed `files_touched` array (some bare strings, some `{repo, path}` objects); reopening the modal re-renders the entries correctly. Setting an unknown repo slug is prevented at submit (the Select only lists existing repo_links). 422 errors render inline.
- **Effort**: L

#### T9: Build `EpicFeatureAttributesPanel` + `EpicFeatureAttributesEditor` modal — context + grouping_rationale

- **Files**: `lumina/web/src/components/work-item/panels/EpicFeatureAttributesPanel.vue`, `lumina/web/src/components/work-item/EpicFeatureAttributesEditor.vue`
- **Action**: `EpicFeatureAttributesPanel` is the tab body (registered at `order=10`, `when: kind==='epic' || kind==='feature'`, label `"Context"` — same per-kind leftmost slot as T7/T8). It renders the read view of `detail.attributes.{context, grouping_rationale}`. An `Edit` button opens `EpicFeatureAttributesEditor` (modal form, used for both epic and feature kinds; auto-determines from focused kind). Fields: `context` (Textarea), `grouping_rationale` (Textarea). Submit → call the existing generic update path: `updateWorkItem(id, { attributes: { context, grouping_rationale } })` from `@/api/work-items`. The PATCH /api/work-items/{id} body's `attributes?: Record<string, unknown>` (api/work-items.ts) is JSON-merged via `repo::set_work_item_attributes` at the backend, so the existing route handles epic/feature attributes without any backend work. Consumer should call this through the new `useHierarchy().updateNode(id, patch)` method (added in T0.5 per P2), keeping the refresh + error path consistent with the other panels. **Tab-label collision note**: the `"Context"` tab label here applies to epic/feature kinds and renders the attribute pair; the separate "Context" tab built by T17 (context_blocks linkage) uses label `"Context Blocks"` to disambiguate. (T17's order=100 puts it at the far right regardless.)
- **Depends on**: T1
- **Acceptance**: Editing an epic's context updates inline display. Re-open shows current value. Backend support is confirmed via the existing generic `PATCH /work-items/{id}` route — no backend addition needed.
- **Effort**: S

### Wave 3: Child-element CRUD panels

#### T10: Build `AcceptanceCriteriaPanel` — inline add + check/uncheck/remove

- **Files**: `lumina/web/src/components/work-item/AcceptanceCriteriaPanel.vue`, `lumina/web/src/components/FocusLens.vue` (modify — remove inline AC fragment)
- **Action**: Panel renders the AC list from `detail.acceptance_criteria`. Each row: checkbox (calls `useAcceptanceCriteria().check/uncheck`), text, `ConfirmAction`-wrapped remove button. Below the list: a single Input + Add button row (inline, no modal). Submit → `useAcceptanceCriteria().add(workItemId, text)` → refresh. Inline error on 422.
- **Depends on**: T1, T2
- **Acceptance**: From dev server, add/check/uncheck/remove AC; the closure-gate behaviour observed in T4 still works (checking the last AC unblocks status→done).
- **Effort**: M

#### T11: Build `ResearchNotesPanel` — first-class records with supersession

- **Files**: `lumina/web/src/components/work-item/ResearchNotesPanel.vue`, `lumina/web/src/components/work-item/ResearchNoteForm.vue`, `lumina/web/src/components/FocusLens.vue` (modify — remove inline research-notes fragment)
- **Action**: Panel renders notes from `detail.research_notes` grouped by `state` (proposed / accepted / rejected; superseded notes shown collapsed). Per-row actions: "Edit" → opens `ResearchNoteForm` modal in update mode (state/confidence/lens/rationale fields); "Supersede" → opens ResearchNoteForm in supersede mode (create a new note that supersedes this). Add row: "+ Add research note" button → opens modal in create mode (summary required; body/confidence/lens/origin optional). Composable: `useResearchNotes().add/update/supersede`.
- **Depends on**: T1
- **Acceptance**: Add a research note, edit its confidence, supersede it with a new note; the superseded one renders as collapsed/struck and the new one appears.
- **Effort**: M

#### T12: Build `RisksPanel` + `RiskForm` — full CRUD + supersession

- **Files**: `lumina/web/src/components/work-item/RisksPanel.vue`, `lumina/web/src/components/work-item/RiskForm.vue`, `FocusLens.vue` (modify)
- **Action**: Panel from `detail.risks`. Per-row: severity badge (using `RiskSeverity` palette — low/medium/high/critical), summary, expand-on-click for body/rationale/mitigation. Actions: Edit (modal update), Supersede (modal supersede), Remove (`ConfirmAction`). Add: "+ Add risk" → `RiskForm` modal (summary required; body/rationale/severity/mitigation). Composable: `useRisks`.
- **Depends on**: T1, T2
- **Acceptance**: Risk lifecycle works end-to-end. The severity Select is bound only to `RiskSeverity` values; do not mix with `Severity` (findings).
- **Effort**: M

#### T13: Build `RejectedAlternativesPanel` + `RejectedAlternativeForm`

- **Files**: `lumina/web/src/components/work-item/RejectedAlternativesPanel.vue`, `lumina/web/src/components/work-item/RejectedAlternativeForm.vue`, `FocusLens.vue` (modify)
- **Action**: Structurally mirrors T12, swapping severity for a free-text `confidence` Input (per backend: confidence is free TEXT for rejected_alternatives, no enum). Composable: `useRejectedAlternatives`.
- **Depends on**: T1, T2, T12 (pattern reuse)
- **Acceptance**: Full CRUD + supersession lifecycle.
- **Effort**: S

#### T14: Build `FindingsPanel` + `FindingForm` — review-finding CRUD + resolve disposition

- **Files**: `lumina/web/src/components/work-item/FindingsPanel.vue`, `lumina/web/src/components/work-item/FindingForm.vue`, `FocusLens.vue` (modify)
- **Action**: Panel from `detail.findings`. Per-row: severity badge using **`Severity`** palette (critical/major/minor/suggestion — DO NOT unify with RiskSeverity), category, file:line link, summary. Per-row actions: Edit (modal), Resolve (small modal: disposition picker fixed|wontfix|verified_clean|deferred|duplicate + optional resolution + rationale), Supersede (modal). Add: "+ Add finding" modal (kind/severity/category/file/line/symbol/summary/description/confidence/repo_id — large form, lg modal width). Composable: `useFindings`. Repo qualifier: if the project has multiple repo_links, a "qualified repo" Select picks one; otherwise hidden.
- **Depends on**: T1
- **Acceptance**: Add → update → resolve(fixed) → supersede a finding. Severity badge colors come from a separate palette than risks.
- **Effort**: L

### Wave 4: Relational structures

#### T15: Build `OpenQuestionsPanel` — questions + options + resolution

- **Files**: `lumina/web/src/components/work-item/OpenQuestionsPanel.vue`, `lumina/web/src/components/work-item/OpenQuestionForm.vue`, `FocusLens.vue` (modify — story-kind only)
- **Action**: Panel visible only when focused item is a story. Renders `detail.open_questions` (each with its `options` array). Per-question actions:
  - "+ Add option" → small inline form (single-field label add, or expanded inline with optional detail Textarea) → `useOpenQuestions().addOption(questionId, ...)`
  - "Block task" → small inline picker selecting one of the story's tasks → `useOpenQuestions().blockTaskOnQuestion(taskId, questionId)`
  - "Resolve" → modal: pick chosen option from the option list; optional `by` field → `useOpenQuestions().resolve(questionId, chosenOptionId, by?)`. On success, the backend cascades (unblock chosen branch's tasks, cancel exclusive branches' tasks) — the refresh re-renders affected child cards automatically.
  - Add question: "+ Add question" → modal: question text (required) → `useOpenQuestions().add(storyId, question)`.
- **Depends on**: T1, T2
- **Acceptance**: Full decision-tree lifecycle: add question, add 2-3 options, block a task on each, resolve picks one → other branches' tasks render as cancelled in `ChildGrid` (already visualised by existing display code).
- **Effort**: L

#### T16: Build `TaskDependencyPanel` — inline add/remove with cycle-error inline display

- **Files**: `lumina/web/src/components/work-item/TaskDependencyPanel.vue`, `FocusLens.vue` (modify — task-kind only, parented under a story)
- **Action**: Panel visible only when focused item is a task. Renders `detail.task_dependencies` (or fetched via `useTaskDependencies().forTask(id)` depending on shape). Per-row: "Remove" button (`ConfirmAction`). Inline add: single Select populated by sibling tasks under the same story → submit posts to `/work-items/{task_id}/depends-on/{depends_on_id}`. On 422 cycle error, render the inline error with the offending cycle edges resolved to task TITLES (not UUIDs). The envelope ships `edges: [{task_id, depends_on_id}, ...]` as raw UUIDs (per lumina/CLAUDE.md HTTP-routes cycle-envelope spec); resolve each id via the local `useHierarchy().detail.children` cache (or a small helper like `useHierarchy().nodeById(id)`) before display. If a referenced task is missing from the local cache (cross-story?), fall back to the short UUID prefix `01970d29…`.
- **Depends on**: T1, T2
- **Acceptance**: Add a dep; attempt to add a cycle → inline error shows the cycle edges; remove a dep → list updates.
- **Effort**: M

#### T17: Build `ContextBlocksPanel` — create + link + unlink

- **Files**: `lumina/web/src/components/work-item/panels/ContextBlocksPanel.vue`, `lumina/web/src/components/work-item/ContextBlockForm.vue`, `FocusLens.vue` (modify — delete the inline context-blocks fallback fragment now that the panel is registered)
- **Action**: Panel registered at `order=100`, label `"Context Blocks"` (disambiguated from T9's `"Context"` label per the note in T9), `when: () => true` (context_blocks apply to any kind). Renders linked context blocks for the focused item. Per-row: unlink (`ConfirmAction`). One add path in v1: "+ Create + link" → `ContextBlockForm` modal (title + body + optional kind) → creates via `useContextBlocks().create(...)` and immediately links via `.link(workItemId, contextBlockId)`. The "link an existing block" picker is **out of scope** for v1: no `GET /context-blocks` list endpoint exists at the backend (per lumina/CLAUDE.md HTTP routes: only POST /context-blocks, POST/DELETE link/unlink). Split that branch into a follow-up plan that adds the list route + an aggregate-aware picker. Composable: `useContextBlocks`.
- **Depends on**: T1, T2
- **Acceptance**: Create-and-link a new context block; unlink it. If no list endpoint exists, T17 ships create-and-link only and flags the list endpoint as a backend follow-up.
- **Effort**: M

#### T18: Build `ActivityLogPanel` — append-only activity entries

- **Files**: `lumina/web/src/components/work-item/ActivityLogPanel.vue`, `lumina/web/src/components/work-item/ActivityEntryForm.vue`, `FocusLens.vue` (modify)
- **Action**: Panel renders `detail.activity` (reverse chronological). Add: "+ Append entry" → modal: `entry_kind` Select (restricted to `execution|vet|comment` per backend validation), `summary` Input, `body` Textarea optional. Submit → `useActivity().append(workItemId, payload)`. No edit/delete (append-only by design).
- **Depends on**: T1
- **Acceptance**: Append an entry; it appears at the top of the list. `entry_kind` Select shows only the 3 allowed values.
- **Effort**: S

### Verification

#### T19: End-to-end hand-verification + full-suite gate

- **Files**: none (verification-only); may add `lumina/web/src/__tests__/end-to-end-smoke.spec.ts` if a single cross-wave smoke spec is useful.
- **Action**: Per-task vitest specs are now part of each W2-W4 task's own Files block (see "Per-task testing convention" below) — T19 no longer authors them. T19's job is the closing gate: run the full `npm run build` + `bun test` + `npm run test:component` suite; address any type-check errors. Hand-verify each wave's acceptance from the dev server against a backed-up `lumina.db`; tick off a per-task manual-verification checklist.
- **Depends on**: T5, T6–T18
- **Acceptance**: `cd lumina/web && npm run build` succeeds; `bun test` passes; `npm run test:component` passes; manual verification checklist (one entry per task) checked off; no console warnings under dev-server exercise.
- **Effort**: S

## Dependency Graph

```
W1: T0, T0.5 ─► T1 ─┬─► T3 ─┐
                    │       │
                    ├─► T4 ◄┘─► T5 ───────────┐
                    │                         │
                    └─► T2 ─────────┐         │
                                    │         │
W2: T0.5, T1, T4 ─► T6              │         │
    T0.5, T1, T3   ─► T7            │         │
    T0.5, T1       ─► T8            │         │
    T0.5, T1       ─► T9            │         │
                                    │         │
W3: T0.5, T1, T2 ─► T10, T11, T12, T13, T14   │
                                    │         │
W4: T0.5, T1, T2 ─► T15, T16, T17, T18        │
                                              │
    T19 ◄──────────────────── (all W2–W4)
```

Within each wave the per-panel tasks (T6–T9, T10–T14, T15–T18) are fully parallelisable: with `FocusLensPanels.ts` from T0.5, each task touches only its own SFC + a single array-append in the registry. No FocusLens.vue contention. Implement in batches of 3-4 parallel agents per wave.

## Verification

- **Per-task**: each task's acceptance criterion is mechanically verifiable from `npm run build` + `bun test` + `npm run test:component` (vitest) output. Hand-verification on the dev server complements the automated specs but does not gate /implement.
- **Per-wave**: at the end of W1, W2, W3, W4, run the full verification command block and exercise the wave's CRUD lifecycle against a backed-up dev DB.
- **End-of-plan**: T19 closes the loop — type-check + bun-test + vitest + hand-verification of every panel.

## Risks

- **FocusLens.vue contention resolved by T0.5** — declarative `FocusLensPanels.ts` registry means each W2-W4 task only touches its own SFC + a single array-append, not FocusLens.vue itself. Was the largest risk in the v1 plan; mitigated by design.
- **Tab strip overflow on narrow viewports** — a story focus may surface ~10 tabs (Plan, AC, Research, Risks, Rejected Alts, Open Questions, Findings, Task Deps, Context Blocks, Activity). At the 3-column grid's central-column width, that strip will wrap or scroll. T1 picks horizontal scroll with a touch-style scrollbar as the v1 fallback; a "More ▾" dropdown collapse is deferred. If user testing surfaces this as a real friction point, a follow-up plan can swap to either a wrapping flex strip or a dropdown — both are local to `Tabs.vue` with no impact on the registry or panel SFCs.
- **Tab persistence in sessionStorage** — the `lumina:focus-tab:<workItemId>` key is per-tab in the browser and cleared on hard refresh. Two open tabs on the same work item will diverge on which sub-tab is active. Acceptable for v1; localStorage with cross-tab sync is over-engineering for an internal tool.
- **Modal portal interaction with the 3-column grid** — `<teleport to="body">` is the standard escape; needs a smoke test that focus trap doesn't break HierarchySpine keyboard nav. T1 acceptance includes a vapor-parent-renders-without-warning check (see Vue 3.6.0-beta.12 caveat in T1).
- **`files_touched` heterogeneous editor (T8) is the most complex single UI** — repo-qualifier toggle plus repo-slug Select pulled from the project ancestor adds state. Consider splitting T8 into T8a (read + display) and T8b (edit) if estimation reveals it's too large.
- **`Severity` vs `RiskSeverity` vocabulary split (T12 vs T14)** is a footgun — unifying them in shared palette code would be wrong. Each panel uses its own palette helper; verify in code review that no shared component leaks one into the other.
- **Soft-delete invisibility** — no v1 undelete UI means accidental deletes are recoverable only via SQL. Two-step confirm partially mitigates; consider adding a 5-second undo banner in a follow-up plan if real users hit this.
- **Optimistic-update gap** — every mutation refetches the full detail. For frequent actions (check/uncheck AC), this is 1 round-trip per click. Acceptable for v1; optimistic updates are a future optimisation.
- **No-auth posture on /api/*** — the SPA's write surface grows ~18× post-plan (from 1 inline form to ~18 modal forms). The /api/* surface has no auth and no Host-header check (per lumina/src/app.rs:72-78). The default loopback bind keeps this safe; operators who set `HOST=0.0.0.0` for LAN access expose all destructive ops (delete work item, supersede risk, resolve open question with cascade) to unauthenticated LAN peers. This is a pre-existing backend concern out of scope for this plan, but the surface-area growth is worth flagging.
