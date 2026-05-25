# Plan: Embed `design_handoff_lumina_control` into lumina/web (Vue 3.6 Vapor + Tailwind 4)

**Plan path**: `docs/plans/lumina-ui-design-embed.md`
**Flow slug**: `lumina-ui-design-embed`
**Created**: 2026-05-25
**Status**: draft
**Review rounds**: 1 (26 findings merged on 2026-05-25)

---

## Context

The Vue 3.6 / Tailwind 4 / Vapor scaffold in `lumina/web/` currently renders a minimal hierarchy tree + detail panel against the existing HTTP routes. A design handoff lives at `docs/design_handoff_lumina_control/` (React + CSS prototype) specifying a high-fidelity "agentic planning control surface" with three-column layout, focus lens, sprint composer, and agent stream. The handoff predates the current backend state, so some elements map cleanly, some need new HTTP routes, and some are reserved for future functionality not yet implemented.

This plan **embeds the design system and ports the components that map cleanly to the read-only surface available today**, while inventorying the gaps so subsequent plans can close them.

---

## Scope

**In scope (this plan):**
- Wire-type extension of `api.ts` to match the existing backend response shape (planning/decision fields the backend already serialises but the frontend currently truncates)
- Design-token embedding via Tailwind 4 `@theme` (colours, typography, radii, animations)
- Self-hosted fonts via Fontsource npm packages (Instrument Serif, Geist Sans, JetBrains Mono)
- Three-column layout shell (`AppHeader`, `AppFooter`, three rails)
- Read-only ports of: `HierarchySpine` / `SpineNode`, `Breadcrumbs`, `CenterToolbar`, `FocusLens` (read-only — no DISPATCH/EDIT/BLOCK action wiring), `ChildGrid` / `ChildCard`, `PortfolioEmpty` (full portfolio lens + epics grid per design)
- Replace `HierarchyView.vue` / `TreeItem.vue` / `assets/logo.svg` with the new component tree
- Extend `useHierarchy()` composable with the focus model (`focusId` replacing `selectedId`, `setFocus` replacing `selectNode`, `treeStatus`, `descendantCounts` incl. done/total task counts and effort-weighted size)
- Wire to the existing HTTP route set (read-only consumption)
- Status-vocabulary translation layer with a single `STATUSES` constant as source of truth
- Browser-shell polish: `<title>Lumina Control</title>`, `<html lang="en">`, brand-mark favicon

**Out of scope (this plan — inventoried for follow-up):**
- Sprint composer wiring (Sprint concept does not exist in the backend yet)
- Agent stream / live-agent panel (no backend exists; entire feature is reserved)
- Tree alternate view (the orthogonal Epic→Feature→Story graph from `tree-view.jsx`)
- Mutating UI for any planning/decision pass (relevance, effort, complexity, AC check/uncheck, research notes CRUD, open question option pick) — none of these MCP tools have HTTP routes today
- Saved Views (placeholder in left rail; no backend filter/bookmark concept)
- Keyboard navigation (S, D, Backspace shortcuts from the design — depends on focus model)
- Drag-and-drop (used by sprint composer; deferred with sprint)
- "Tweaks panel" and giant-watermark behind lens (design author flagged as non-production)
- Component test harness (not yet wired — see Inventory item #9)
- Accessibility pass — ARIA tree semantics, keyboard focus management, screen-reader announcements (see Inventory item #8)
- Findings panel — old `HierarchyView.vue:77-87` rendered severity-coloured findings; design has no slot for it; deferred pending UX decision (see Inventory)

**Affected areas:**
- `lumina/web/src/api.ts` (wire-type extension — adds 8 fields to `WorkItem`, adds 4 nested arrays + 5 new types to `WorkItemDetail` set)
- `lumina/web/src/` (new components, composables, tokens)
- `lumina/web/src/assets/` (Tailwind theme + raw tokens + Fontsource imports)
- `lumina/web/index.html` (title / lang / favicon)

**Estimated file count**: ~16 new SFCs/composables + 3 CSS additions + 4 deletions + 2 modifications. ~21 net new files.

---

## Exploration Notes

### Design handoff (`docs/design_handoff_lumina_control/`)

7 files (~3000 lines total):

| File | Role |
|---|---|
| `README.md` | Primary spec — typography, colours, spacing, layout grid, state schema, breakpoints |
| `app.jsx` | React component architecture (~925 lines) + state model |
| `styles.css` | All design tokens + complete stylesheet (~1577 lines) |
| `data.js` | Fixture: 4 epics / 6 features / 12 stories / 16 tasks / SP-26 sprint / 4 agents |
| `tree-view.jsx` | Orthogonal Epic→Feature→Story view (alternate to focus lens) |
| `tweaks-panel.jsx` | Runtime UI tweaker — **prototype-only, discard** |
| `Lumina Control.html` | HTML shell, script loading |

**Design tokens (canonical values to embed in Tailwind `@theme`):**
- **Surfaces**: `--bg #0e0c08`, `--surface #15120d`, `--surface-2 #1c1812`, `--surface-3 #251f17`
- **Borders**: `--border #2a2519`, `--border-strong #3a3220`, `--border-faint #1f1a13`
- **Ink**: `--ink #f0eadc`, `--ink-2 #cfc7b3`, `--muted #8f8775`, `--faint #5c5547`, `--ghost #3a352b`
- **Accent + status (OKLCH)**: `--accent oklch(0.78 0.13 70)` amber, `--success oklch(0.74 0.10 145)`, `--blocked oklch(0.65 0.16 28)`, `--queued oklch(0.68 0.04 80)`, `--done oklch(0.62 0.04 95)`
- **Typography**: Instrument Serif (display, italic 32/38/46px), Geist Sans (sans 400/500/600), JetBrains Mono (mono 9.5–16px, labels/IDs/runtime)
- **Radii**: 3/5/8/14px + pill (999px)
- **Animations**: 150ms ease transitions, 2s pulse (live dot), 1.4s indeterminate progress

**Component handles** (design names → planned SFC paths under `src/components/`):
- `AppHeader.vue`, `AppFooter.vue`
- `HierarchySpine.vue` + `SpineNode.vue` (left rail)
- `CenterToolbar.vue`, `Breadcrumbs.vue`, `FocusLens.vue` (read-only variant), `ChildGrid.vue` + `ChildCard.vue` (centre)
- `StatusPill.vue` (shared by SpineNode, ChildCard, FocusLens — created in Wave 2)
- `SprintComposer.vue` (deferred), `AgentList.vue` + `AgentCard.vue` (deferred)
- `PortfolioEmpty.vue` (full portfolio lens + epics ChildGrid when `focusId === null`)

**Design lexicon mapping to lumina domain:**

| Design term | Lumina backend | Mapping note |
|---|---|---|
| Epic / Feature / Story / Task | Same kinds | 1:1 |
| Status `in-flight` | `in_progress` | translation table |
| Status `queued` | `todo` | translation table |
| Status `blocked` | `blocked` | 1:1 |
| Status `done` | `done` | 1:1 |
| Status `draft` | (no equivalent) | dropped per User Decision 1 |
| `points` (1–13, sized S/M/L/XL) | `effort` (`s|m|l`, task-scope only) | NO XL — design has more granular size pill than backend |
| `owner` field | not in backend | display-only or omit |
| `progress: 0..1` on epic/feature/story | computed from descendants (not stored) | derive client-side via `doneTasks / totalTasks` |
| `Sprint` | NOT IN BACKEND | reserve composer for future |
| `Agent` | NOT IN BACKEND | reserve panel for future |
| `acceptance: { t, done }[]` on task | `acceptance_criteria` child table (id, text, checked) | 1:1 once wire-extended |
| `context: { files[], related[] }` on task | `attributes.files_touched` + context-block links | display-only initially |

### Backend surface (`lumina/src/`, `CLAUDE.md`)

- 34 MCP tools / 4 HTTP route lines (6 method+path handlers): `GET /api/health`, `GET /api/work-items` (tree by default, `?parent_id=&kind=` flat), `POST /api/work-items`, `GET /api/work-items/{id}` (detail), `PATCH /api/work-items/{id}` (status/title/body/position/attributes)
- Data model carries all the planning/decision fields (relevance, effort, complexity, origin, closure_gate, AC child table, research-note child table, open-question + option child tables). None of the planning/decision MCP tools (set_relevance, set_effort, set_complexity, AC CRUD, research-note CRUD, open-question option pick, etc.) have HTTP routes today (confirmed via `lumina/src/http.rs:56-64`)
- **Critical wire-shape note**: the backend response for `GET /api/work-items/{id}` already returns `acceptance_criteria`, `research_notes`, `open_questions`, `activity` arrays (see `lumina/src/domain.rs:203-216`). The frontend `WorkItemDetail` in `api.ts` is currently a strict subset — T1's wire-type extension closes that gap WITHOUT requiring any new HTTP routes
- e2e harness at `lumina/tests/e2e.rs` proves MCP→DB→export→HTTP works end-to-end via in-process `oneshot` requests — new HTTP routes are testable in the same harness

### Frontend (`lumina/web/`)

- `main.ts`: `createVaporApp(App).mount('#app')` — pure Vapor, no plugins
- `App.vue`, `views/HierarchyView.vue`, `components/TreeItem.vue` — all `<script setup vapor>`
- `composables/useHierarchy.ts` — module-singleton refs (`tree`, `selectedId`, `detail`, `loading`, `error`) + 5 methods (`loadTree`, `selectNode`, `clearSelection`, `createNode`, `changeStatus`)
- `api.ts` — 5 fetch functions, 6 types (WorkItem, WorkItemNode, Finding, ContextBlock, WorkItemDetail, CreateWorkItemRequest); `WorkItem` is missing 8 backend columns and `WorkItemDetail` is missing 4 nested arrays (see Critical wire-shape note above)
- `assets/main.css` — only `@import "tailwindcss";` (no `@theme`, no custom CSS)
- **Vapor compatibility**: all template features used today (v-for, v-if, @click, dynamic :class, :style, defineProps, ref, computed, onMounted, recursive self-reference, `<style scoped>`) are Vapor-supported — no blockers
- **`rust-embed` constraint**: `lumina/web/dist/` is baked into the binary at release. Note: Vite content-hash filenames are not guaranteed byte-reproducible across machines without explicit `rollupOptions.output.assetFileNames` pinning (see vitejs/vite#13071); we do not currently assert binary reproducibility, so this is informational only
- **Vite asset pipeline**: Fontsource per-weight CSS files reference WOFF2 via relative imports; Vite hashes them into `dist/assets/*.woff2` (not `dist/fonts/`)
- **No `tomlctl blocks` parity** for any `lumina/web/**` file — free to add files without manifest drift checks

### Verification commands (from CLAUDE.md + package.json)

```
build:      cd lumina/web && npm run build
type-check: cd lumina/web && npm run type-check
dev:        cd lumina/web && npm run dev
backend:    cargo build --manifest-path lumina/Cargo.toml
backend-test: cargo test --manifest-path lumina/Cargo.toml
```

---

## Research Notes

Vetted: 13 of 14 findings retained; Finding 13 (WOFF2 size estimates) dropped as UNCONFIRMED — exact sizes will come from Fontsource npm packages at implementation time.

### Tailwind 4 `@theme`

- **`@theme` syntax**: declare `--color-*` and `--font-*` inside a top-level `@theme` block; Tailwind auto-generates matching utility classes. Source: [Tailwind v4 theme variables](https://tailwindcss.com/docs/theme). *Impact*: use directly in `main.css` (or a dedicated `theme.css`); no JS config file needed.
- **Namespace conventions**: `--color-*` → colour utilities (`bg-`, `text-`, `border-`, `fill-`, `stroke-`, `ring-`, `decoration-`); `--font-*` → `font-{name}`; `--text-*` → font-size; `--spacing-*` → spacing; `--radius-*` → `rounded-{name}`; `--animate-*` → `animate-{name}`; `--breakpoint-*` → responsive prefixes. *Impact*: name every token with the correct prefix; auto-utility generation is free — DO NOT use `text-[var(--color-X)]` indirection when `text-X` is auto-generated.
- **OKLCH in `@theme`**: `oklch(0.78 0.13 70)` is valid; all design-palette colours can use OKLCH directly without hex fallback. Source: [Tailwind v4 alpha blog](https://github.com/tailwindlabs/tailwindcss.com/blob/main/src/blog/tailwindcss-v4-alpha/index.mdx).
- **Non-utility CSS vars**: variables intended only for raw `var(--x)` use (no utility class wanted) should live in `:root`, not `@theme`. Source: [discussion #15122](https://github.com/tailwindlabs/tailwindcss/discussions/15122). *Impact*: keep `--border-faint`, `--surface-2/3`, `--ghost` etc. in `:root {}`; reserve `@theme` for tokens that should generate utilities.
- **`@theme` placement**: must be top-level (not nested in selectors, `@layer`, or media queries); may be in any CSS file imported into the Tailwind entry. *Impact*: extracting tokens to `theme.css` and `@import`ing from `main.css` is fine.
- **`@keyframes` for `--animate-*`**: must be declared INSIDE the same `@theme {}` block to enable tree-shaking against the binding. Top-level `@keyframes` are always emitted unconditionally and bypass the binding (still functionally work, but bigger CSS bundle).

### Vue 3.6 Vapor mode (beta.12)

- **`<script setup vapor lang="ts">`** is the supported per-SFC opt-in syntax in beta.12. Source: [v3.6.0-beta.1 release notes](https://github.com/vuejs/core/releases/tag/v3.6.0-beta.1).
- **`createVaporApp(App).mount('#app')`** is the documented init API for pure-Vapor apps; alternative `createApp(App).use(vaporInteropPlugin)` exists for mixed VDOM/Vapor (not used here).
- **`<Transition>` / `<TransitionGroup>`**: supported in Vapor as of v3.6.0-beta.1 ("feature parity with all stable features in Virtual DOM mode"). *Impact*: safe for mount/unmount animations; pure-CSS keyframes (pulse / indeterminate-bar) need no Vue support at all.
- **`<Teleport>`**: supported (broad feature-parity claim). *Impact*: usable for future toast/dialog; smoke-test when introduced.
- **`<KeepAlive>`**: supported, with KeepAlive-specific bug fixes in beta.1. *Impact*: safe for tree-view caching across focus changes.
- **Async components**: `defineVaporAsyncComponent` is the Vapor counterpart to `defineAsyncComponent` (PR #13059 in minor). *Impact*: use this API for future code-splitting; do NOT use the VDOM `defineAsyncComponent` inside Vapor.
- **Vapor-VDOM slot footgun**: vapor slots inside a VDOM host need `renderSlot`, not `slots.default()`. Does NOT apply pure-Vapor-to-Vapor (we are 100% Vapor).

### Font self-hosting

- **CDN availability**: Instrument Serif, Geist Sans, JetBrains Mono are all on npm via Fontsource. *Impact*: install per-package and `@import` the per-weight CSS file from `node_modules` — Vite resolves and hashes the WOFF2 into `dist/assets/`.
- **Confirmed package + weight paths** (use these verbatim in T2):
  - `@fontsource/instrument-serif/400-italic.css` (display headings)
  - `@fontsource/geist-sans/400.css`, `/500.css`, `/600.css` (Geist Sans has no italic; the design's italic is exclusively Instrument Serif)
  - `@fontsource/jetbrains-mono/400.css`, `/500.css`, `/600.css`
- **`@font-face` + Tailwind `--font-*`**: independent declarations; the font-family name string is opaque to Tailwind. Fontsource CSS already emits the `@font-face`; we just reference the family name in `@theme --font-sans: "Geist Sans", ...`.

### Sources

- [Tailwind CSS v4 theme variables](https://tailwindcss.com/docs/theme)
- [Tailwind CSS v4 alpha blog](https://github.com/tailwindlabs/tailwindcss.com/blob/main/src/blog/tailwindcss-v4-alpha/index.mdx)
- [Tailwind discussion #15122 — `@theme` vs `:root`](https://github.com/tailwindlabs/tailwindcss/discussions/15122)
- [v3.6.0-beta.1 release notes (vuejs/core)](https://github.com/vuejs/core/releases/tag/v3.6.0-beta.1)
- [Vapor Roadmap #13687 (vuejs/core)](https://github.com/vuejs/core/issues/13687)
- [Fontsource — Geist Sans](https://fontsource.org/fonts/geist-sans/install)
- [Fontsource — Instrument Serif](https://www.npmjs.com/package/@fontsource/instrument-serif)
- [caniuse OKLCH](https://caniuse.com/mdn-css_types_color_oklch)

## User Decisions

1. **Status vocabulary** — Extend the design's status pill to render all 5 backend states verbatim (`todo`, `in_progress`, `blocked`, `done`, `cancelled`). Design's `draft` is dropped (it has no backend home today). Status-display mapping: design's `in-flight` ↔ backend `in_progress`; design's `queued` ↔ backend `todo`; `blocked` / `done` 1:1; add a new `cancelled` styling (muted/struck variant) to the pill component.
2. **Size / effort** — Use backend `effort` (`s|m|l`) as-is. Render the size pill on **task cards only**. Drop the design's XL tier and the numeric `points` field; epic/feature/story cards omit size.
3. **KPI counts on FocusLens** — Compute client-side from the loaded tree. The existing `fetchTree()` returns the full nested hierarchy, so a composable can recursively count descendants by kind. No backend change required for this plan.
4. **Plan path** — Final plan lives at `docs/plans/lumina-ui-design-embed.md` at the repo root.

### Phase 5 outcome
Skipped. None of the four decisions introduced a new library / API / pattern requiring additional research.

---

## Approach

**Override on the design's state recommendation**: the design handoff suggests Pinia for shared state. We override this — `lumina/web/` is intentionally Pinia-free, using module-singleton composables (`composables/useHierarchy.ts` is the precedent). All new state introduced by this plan extends `useHierarchy` or adds sibling composables under `src/composables/`.

**Read-only ports first**. The existing HTTP routes are enough to drive every component this plan ships, ONCE the frontend wire types are extended to match the response shape the backend already produces (T1). Every mutating action in the design (`DISPATCH AGENT`, `+ ADD TO SPRINT`, `EDIT`, `BLOCK`, AC checkboxes, status dropdown, etc.) is either rendered as a disabled-with-tooltip affordance or omitted entirely with a `<!-- deferred: requires HTTP route for X -->` marker. Future plans will turn these on as backend HTTP routes land.

**Vocabulary single source of truth**: status mappings are not scattered. T4 exports a `STATUSES` const array of `{ backend, label, tokenName }` rows; `statusLabel`, `statusToken`, the `<StatusPill>` colour map, and `ChildGrid`'s filter tabs all derive from it. Adding a 6th status is a one-line change.

**Token strategy**: split CSS into `theme.css` (Tailwind `@theme` — generates utility classes for accent, status colours, font families, radii, animations) and `tokens.css` (`:root {}` — raw vars for surfaces, borders, ink, ghost). Both imported from `main.css` after `@import "tailwindcss"`. Component SFCs use **direct Tailwind utilities** for everything generatable (`bg-accent`, `text-queued`, `font-display`, `rounded-lg`) and `var(--surface)` etc. for the rest. Do NOT use `text-[var(--color-X)]` arbitrary-value indirection — it inflates the JIT and bypasses utility generation.

**Font strategy**: install Fontsource npm packages (`@fontsource/instrument-serif`, `@fontsource/geist-sans`, `@fontsource/jetbrains-mono`) for deterministic byte sizes; import the specific weight CSS files in `main.css`. Fontsource WOFF2 lands in `node_modules/`, gets hashed by Vite into `dist/assets/`, baked by `rust-embed` automatically.

**Tree status discipline**: `useHierarchy` exposes a `treeStatus` computed (`'loading' | 'error' | 'empty' | 'ready'`) so the layout can branch — loading spinner, error banner, empty-portfolio copy, or full UI. The old `HierarchyView.vue` rendered these three states inline; the new layout must preserve that contract.

**Component decomposition**: 1 SFC per design-handle, kept under ~150 lines each. Recursive components reference themselves by filename. `StatusPill` is a Wave-2 foundation component (consumed by SpineNode, ChildCard, FocusLens — not buried inside any consumer).

**Integration sequence**: build all components in parallel waves with file-isolated tasks (no two parallel tasks edit the same file), then a single integration task replaces the current `App.vue` / deletes the obsolete files / mounts the new tree.

---

## Verification Commands

```
build:        cd lumina/web && npm run build
type-check:   cd lumina/web && npm run type-check
dev:          cd lumina/web && npm run dev   # then visit http://localhost:5173 with backend on :8080
backend-up:   cargo run --manifest-path lumina/Cargo.toml
import-seed:  cargo run --manifest-path lumina/Cargo.toml -- import-flow lumina-schema-deepening   # OR `lumina import-flow ...` if installed
seed-augment: (manual) MCP `add_acceptance_criterion` + `set_effort` against one task on the imported story so FocusLens task-specific renderers exercise non-empty paths
```

---

## Tasks

### Wave 1 — Foundation (5 parallel)

#### T1: Extend `api.ts` wire types to match backend shape
- **Files**: `lumina/web/src/api.ts`
- **Action**: Extend the existing types to match `lumina/src/domain.rs:21-49` (`WorkItem`) and `:203-216` (`WorkItemDetail`):
  - `WorkItem` adds (all `string | null` except `position`): `attributes: Record<string, unknown> | null`, `relevance: string | null`, `effort: string | null`, `complexity: string | null`, `origin: string | null`, `closure_gate: string | null`, `blocked_by_question_id: string | null`, `enabling_option_id: string | null`. Make `position: number | null` (it's `Option<i64>` on the wire).
  - `WorkItemNode extends WorkItem` (already does — keep recursive `children`).
  - `Finding` adds: `dedup_id: string | null`, `origin: string | null`, `confidence: string | null`, `superseded_by: string | null`, `resolved_at: string | null`. Make `work_item_id: string | null` (it's `Option<String>` server-side).
  - Add new interfaces: `AcceptanceCriterion { id, work_item_id, seq, text, checked, checked_at, checked_by, created_at }`; `ResearchNote { id, work_item_id, seq, summary, body?, confidence?, state, rationale?, lens?, origin?, superseded_by?, created_at }`; `OpenQuestion { id, story_id, seq, question, status, answer?, chosen_option_id?, decided_at?, decided_by?, prompting_finding_id?, prompting_note_id?, created_at, options: QuestionOption[] }`; `QuestionOption { id, question_id, seq, label, detail?, created_at }`; `WorkItemActivity { id, work_item_id, seq, entry_kind, author?, summary, payload?, origin?, created_at }`.
  - `WorkItemDetail` extends to: `item, children: WorkItem[], findings: Finding[], context_blocks: ContextBlock[], activity: WorkItemActivity[], acceptance_criteria: AcceptanceCriterion[], research_notes: ResearchNote[], open_questions: OpenQuestion[]`.
- **Detail**: NO new fetch functions, NO new HTTP routes — the backend already returns this shape. This task closes the type-cast gap. Cross-reference `lumina/src/domain.rs` for the exact field names (snake_case on the wire is preserved verbatim).
- **Acceptance**: `cd lumina/web && npm run type-check` passes (no new files reference these types yet; this is a contract-only extension). `git diff` shows additions only; no existing fields removed or renamed.
- **Effort**: M

#### T2: Install fonts + embed design tokens + wire main.css
- **Files**: `lumina/web/package.json`, `lumina/web/src/assets/fonts.css` (new), `lumina/web/src/assets/theme.css` (new), `lumina/web/src/assets/tokens.css` (new), `lumina/web/src/assets/main.css`
- **Action**:
  - `npm install --save @fontsource/instrument-serif @fontsource/geist-sans @fontsource/jetbrains-mono`.
  - `fonts.css`: import only the weights actually used — `@import "@fontsource/instrument-serif/400-italic.css"; @import "@fontsource/geist-sans/400.css"; @import "@fontsource/geist-sans/500.css"; @import "@fontsource/geist-sans/600.css"; @import "@fontsource/jetbrains-mono/400.css"; @import "@fontsource/jetbrains-mono/500.css"; @import "@fontsource/jetbrains-mono/600.css";`
  - `theme.css`: top-level `@theme {}` block declaring **utility-generating** tokens — `--color-accent`, `--color-accent-deep`, `--color-accent-glow`, `--color-success`, `--color-in-flight`, `--color-queued`, `--color-blocked`, `--color-done` (OKLCH from spec); `--font-display: "Instrument Serif", serif`; `--font-sans: "Geist Sans", system-ui, sans-serif`; `--font-mono: "JetBrains Mono", ui-monospace, monospace`; `--radius-sm 3px`, `--radius-md 5px`, `--radius-lg 8px`, `--radius-xl 14px`; `--animate-pulse-dot: pulse-dot 2s infinite`, `--animate-indeterminate: indeterminate 1.4s linear infinite`. **`@keyframes pulse-dot { ... }` and `@keyframes indeterminate { ... }` MUST be declared INSIDE the same `@theme {}` block** so tree-shaking binds them to the animate tokens.
  - `tokens.css`: `:root {}` block for raw vars NOT meant to generate utilities — `--bg #0e0c08`, `--surface #15120d`, `--surface-2 #1c1812`, `--surface-3 #251f17`, `--border #2a2519`, `--border-strong #3a3220`, `--border-faint #1f1a13`, `--ink #f0eadc`, `--ink-2 #cfc7b3`, `--muted #8f8775`, `--faint #5c5547`, `--ghost #3a352b`.
  - `main.css`: order becomes `@import "./fonts.css"; @import "tailwindcss"; @import "./theme.css"; @import "./tokens.css";`. Add `body { background: var(--bg); color: var(--ink); }` — do NOT set `font-family` on body (the `--font-sans` in `@theme` already cascades via Tailwind v4 Preflight; redundant rule fights utility overrides).
- **Detail**: All colour values from `docs/design_handoff_lumina_control/styles.css` `:root {}`. Do NOT add HSL or hex fallbacks — modern browsers all support OKLCH.
- **Acceptance**: `npm run build` succeeds; verify `dist/assets/` contains 7+ `*.woff2` files; `dist/assets/index-*.css` contains the OKLCH values and `@keyframes pulse-dot` + `@keyframes indeterminate`; `bg-accent`, `text-queued`, `font-display`, `rounded-lg`, `animate-pulse-dot` all resolve to design colours/animations when used in a test SFC.
- **Effort**: M

#### T3: Extend `useHierarchy` with focus model, treeStatus, and full descendantCounts
- **Files**: `lumina/web/src/composables/useHierarchy.ts`
- **Action**: **REPLACE** the existing `selectedId` ref with `focusId: Ref<string | null>` (default `null` → empty-state portfolio view). **REPLACE** `selectNode(id)` with `setFocus(id: string | null): void` (the new function body keeps the existing detail-fetch logic; sets `focusId` and loads detail if non-null; if null, clears `detail`). **REPLACE** `clearSelection()` with `setFocus(null)` (delete the old function; update any in-file callers). Remove `selectedId` and `selectNode` from the returned object. Add:
  - `view: Ref<'focus' | 'tree'>` (default `'focus'`)
  - `focusPath: ComputedRef<WorkItem[]>` — walks `parent_id` chain from focused node up to root using the loaded tree; empty array when `focusId === null`
  - `descendantCounts: ComputedRef<{ features: number; stories: number; tasks: number; doneTasks: number; totalTasks: number; size: number }>` — recursively counts descendants of the focused node from the loaded tree; `doneTasks` = count of descendant tasks with `status === 'done'`; `totalTasks` = count of all descendant tasks; `size` = sum of mapped effort weights (`s=2`, `m=5`, `l=8`, `null`/unknown contributes 0) across descendant tasks. When `focusId === null`, counts apply to the ENTIRE tree (portfolio rollup).
  - `treeStatus: ComputedRef<'loading' | 'error' | 'empty' | 'ready'>` — `loading` when `loading.value && tree.value.length === 0`; `error` when `error.value !== null && tree.value.length === 0`; `empty` when `!loading.value && !error.value && tree.value.length === 0`; otherwise `ready`.
- **Detail**: Preserve `tree`, `detail`, `loading`, `error`, `loadTree`, `createNode`, `changeStatus` unchanged. Use the already-loaded `tree` for descendant counting and path walking — no extra fetch.
- **Acceptance**: `npm run type-check` passes; the new exports appear in the return object; the removed exports do NOT (compile-error if any other file still imports them — but T12 is the only consumer and is also rewritten). Manual smoke: setting `focusId.value = 'some-id'` causes `focusPath`, `descendantCounts`, and `treeStatus` to update reactively.
- **Effort**: M

#### T4: Status / effort / kind helper composable with `STATUSES` single-source-of-truth
- **Files**: `lumina/web/src/composables/useDisplay.ts` (new)
- **Action**: Export pure functions (no reactive state — this is a translation layer):
  - `STATUSES = [ { backend: 'todo', label: 'QUEUED', tokenName: 'queued' }, { backend: 'in_progress', label: 'IN-FLIGHT', tokenName: 'in-flight' }, { backend: 'blocked', label: 'BLOCKED', tokenName: 'blocked' }, { backend: 'done', label: 'DONE', tokenName: 'done' }, { backend: 'cancelled', label: 'CANCELLED', tokenName: null } ] as const` — single source of truth; consumed by `statusLabel`, `statusToken`, `<StatusPill>`, and `ChildGrid` filter tabs.
  - `statusLabel(status: string): string` — looks up `STATUSES.find(s => s.backend === status)?.label`; fallback to `status.toUpperCase()`.
  - `statusToken(status: string): string` — looks up `tokenName`; returns the **plain Tailwind utility class** `text-${tokenName}` (NOT `text-[var(--color-${tokenName})]` — the auto-generated utility from T2's `@theme --color-${tokenName}` works directly). `cancelled` returns `text-[var(--muted)] line-through` (no auto-generated utility for it).
  - `effortLabel(effort: string | null | undefined): string | null` — `s`→"S", `m`→"M", `l`→"L", anything else (incl. null) → `null` (caller renders nothing).
  - `kindLabel(kind: string): string` — uppercase kind ("EPIC", "FEATURE", "STORY", "TASK", "PROJECT").
- **Detail**: Static lookups; no I/O. Export `STATUSES` so ChildGrid filter tabs can `map` over it instead of duplicating the string list.
- **Acceptance**: `npm run type-check` passes; importable from any SFC.
- **Effort**: S

#### T5: Polish `index.html` (title, lang, favicon)
- **Files**: `lumina/web/index.html`, `lumina/web/public/favicon.ico` (replace with brand-mark SVG)
- **Action**: Set `<html lang="en">`; set `<title>Lumina Control</title>`; replace `public/favicon.ico` with a minimal amber `L` SVG (32×32, OKLCH amber on transparent) named `public/favicon.svg` AND update `<link rel="icon" href="/favicon.svg" type="image/svg+xml">`. Delete the existing `public/favicon.ico` (Vite scaffold default).
- **Detail**: Tiny task — the production binary's tab title is otherwise "Vite App" forever.
- **Acceptance**: `npm run build` succeeds; `dist/index.html` contains `Lumina Control` and `lang="en"`; `dist/favicon.svg` exists.
- **Effort**: S

### Wave 2 — Shell + left rail + StatusPill (5 parallel)

#### T6: AppHeader.vue (brand mark, command bar, sprint/agent/date pills)
- **Files**: `lumina/web/src/components/AppHeader.vue` (new)
- **Action**: `<script setup vapor lang="ts">`. Implement per `app.jsx` Header / `styles.css` `.header`: 56px tall, fixed grid (brand | command bar centre | pills right). Brand mark: `<span>` containing a rotated-45° "L" + "LUMINA" wordmark + version pill. Command bar: a disabled `<input placeholder="JUMP TO…" aria-label="Search (disabled — coming soon)">` plus "⌘K" hint mono badge — input is **decorative only this plan** (no search backend). Right pills: render fixed text "DRAFT" sprint pill, "0 AGENTS" pill, today's date pill (`new Date().toLocaleDateString('en-GB', { day: '2-digit', month: 'short', year: 'numeric' })`). Use **direct Tailwind utilities** (`bg-[var(--surface)]`, `border-[var(--border)]`, `text-[var(--ink-2)]` — these CSS vars from T2's `tokens.css` need the bracket form because they're :root vars, not `@theme` tokens).
- **Detail**: No state needed; pure presentational. Mark sprint pill text and agent count as `<!-- deferred: sprint composer / agent backend -->`.
- **Acceptance (manual)**: Renders without errors; structural assertions — header is `h-14` (56px) tall via class; brand mark contains exactly the rotated "L" glyph; right column contains exactly 3 pills. Tag: visual review against `docs/design_handoff_lumina_control/styles.css` `.header*`.
- **Effort**: S

#### T7: AppFooter.vue (breadcrumb path + keyboard hints)
- **Files**: `lumina/web/src/components/AppFooter.vue` (new)
- **Action**: `<script setup vapor lang="ts">`. 32px tall. Left: clickable breadcrumb path from `useHierarchy().focusPath` — kindLabel + " / " separators; click jumps focus to that ancestor via `setFocus(id)`. Right: keyboard hint row "↑↓ NAV · ↵ FOCUS · ⌫ UP · S SPRINT · D DISPATCH" in JetBrains Mono. The keyboard hints are **rendered but inactive** this plan; hint text is static.
- **Detail**: Use `font-mono text-[10.5px] text-[var(--faint)]` per the design.
- **Acceptance**: Renders breadcrumbs reactively; clicking a breadcrumb segment updates focus; keyboard-hint text matches the design exactly.
- **Effort**: S

#### T8: StatusPill.vue (shared component)
- **Files**: `lumina/web/src/components/StatusPill.vue` (new)
- **Action**: `<script setup vapor lang="ts">`. Props: `{ status: string }`. Renders a rounded pill with `statusLabel(status)` text and `statusToken(status)` colour class. Consumed by SpineNode (T9), ChildCard (T11), FocusLens (T10), and PortfolioEmpty (T12). One tiny component — promoted to its own task to unblock Wave-3 parallelism.
- **Detail**: ~20 lines. Uses `useDisplay` from T4. No state.
- **Acceptance**: `npm run type-check` passes. Renders a pill with the right colour for each of the 5 `STATUSES.backend` values.
- **Effort**: S

#### T9: HierarchySpine.vue + SpineNode.vue (left rail)
- **Files**: `lumina/web/src/components/HierarchySpine.vue` (new), `lumina/web/src/components/SpineNode.vue` (new)
- **Action**:
  - `HierarchySpine.vue`: Section header `[01 / PLANNING GRAPH]` mono faint. Branch on `useHierarchy().treeStatus` — `loading`: skeleton bars; `error`: inline error block with `useHierarchy().error` text; `empty`: "No work items yet" copy; `ready`: render the spine list. When `ready` AND `focusId === null`, render ALL root items (epics) as the spine list. When focused, render focus-path ancestors followed by the focused node's siblings. Each row is a `<SpineNode>`. The spine column scrolls independently (`overflow-y: auto`); the header is sticky. Below, a `[02 / SAVED VIEWS]` section header + a static placeholder list ("▸ in-flight only", "▸ blocked items", "▸ unassigned tasks", "▸ this sprint", "+ new view") — render as visually-disabled buttons (no click handlers); mark `<!-- deferred: saved views backend -->`.
  - `SpineNode.vue`: One row per item; props `{ node: WorkItem, isFocused: boolean, isAncestor: boolean }`. Layout: vertical gradient spine line on left (CSS), a diamond marker (◇ when not focused, ◆ + amber halo when focused, via `text-accent`), then kindLabel mono faint + title sans + optional `<StatusPill :status="node.status">` from T8. `@click` calls `setFocus(node.id)`.
- **Detail**: For the spine gradient line, use a `::before` pseudo-element with `background: linear-gradient(to bottom, transparent, var(--border-strong) 20%, var(--border-strong) 80%, transparent);` — copy verbatim from `styles.css` `.spine::before`.
- **Acceptance**: With seeded data, clicking a node updates `focusId` in the composable; the focused row shows the amber-halo diamond; siblings are listed under the focused node. With `focusId === null`, all root epics are listed. Spine scrolls independently when content exceeds height.
- **Effort**: M

#### T10: CenterToolbar.vue + Breadcrumbs.vue
- **Files**: `lumina/web/src/components/CenterToolbar.vue` (new), `lumina/web/src/components/Breadcrumbs.vue` (new)
- **Action**:
  - `CenterToolbar.vue`: View-toggle button-group ("FOCUS" / "TREE") bound to `useHierarchy().view`. Clicking "TREE" sets `view.value = 'tree'` but the tree view itself is **out of scope** — the centre column shows a placeholder "TREE VIEW — DEFERRED" panel when active. Right side: context tag showing kindLabel + " · " + focused node id, e.g. "STORY · S-001".
  - `Breadcrumbs.vue`: Same data source as `AppFooter` (focusPath), styled per design as `.breadcrumbs` — `ROOT / EPIC / FEATURE / STORY` with `/` separators; clickable segments call `setFocus(id)`. This one is rendered inside the centre column above the FocusLens (the footer one is global).
- **Detail**: `CenterToolbar` uses `font-mono` for the view toggle and the context tag. `Breadcrumbs` uses `font-mono text-[10.5px] text-[var(--muted)]`.
- **Acceptance**: View toggle flips the `view` ref reactively; breadcrumbs render reactively; both segments wrap correctly at narrow widths (≤1280px breakpoint).
- **Effort**: S

### Wave 3 — Centre column components (3 parallel)

#### T11: FocusLens.vue (read-only hero card)
- **Files**: `lumina/web/src/components/FocusLens.vue` (new)
- **Action**: Main hero card per `app.jsx` `FocusLens` + `styles.css` `.lens*`. Imports `<StatusPill>` from T8. Subscribes to `useHierarchy()` for `detail` and `descendantCounts`. Layout:
  - `.lens-head`: kindLabel mono faint, title (Instrument Serif 46px italic for non-task, Geist 36px for task), body/summary, `<StatusPill :status="item.status">` + (non-task only) a thin progress bar. **Progress bar rule**: `progress = descendantCounts.value.totalTasks > 0 ? descendantCounts.value.doneTasks / descendantCounts.value.totalTasks : null`; if `progress === null`, render NO progress bar (do not divide by zero, do not render a 0% bar). Right column shows size/owner — owner omitted (no backend field).
  - `.lens-stats`: 4-column KPI grid bound to `descendantCounts` (FEATURES / STORIES / TASKS / SIZE), all mono 16px values, mono 10.5px faint labels.
  - Corner brackets: implement via two `::before` / `::after` pseudo-elements with 16×16 amber borders (per `.lens::before` / `.lens::after` in `styles.css`).
  - Task-specific extras (when `item.kind === 'task'`): render 4 disabled action buttons (DISPATCH AGENT, + ADD TO SPRINT, EDIT, BLOCK) with `disabled` attribute + `title="Deferred — requires HTTP route"` + `aria-disabled="true"`. Render `acceptance_criteria` from `detail.acceptance_criteria` (now typed via T1) as a static read-only checklist (12×12 visual checkboxes from `checked` flag; **NOT interactive** this plan). Render `context_blocks` from `detail.context_blocks` as a 2-column grid of context cards.
- **Detail**: All metrics derived; no fetch added in this task. Use the already-loaded tree.
- **Acceptance**: With a seeded story selected, lens renders title in Instrument Serif italic, KPI grid shows correct counts (matching a manual count from the seeded data), progress bar reflects `doneTasks / totalTasks` ratio (or absent if no descendant tasks). With a task selected, action buttons render disabled, AC checklist matches `detail.acceptance_criteria` length (zero-length → "No acceptance criteria" empty case).
- **Effort**: L

#### T12: ChildGrid.vue + ChildCard.vue
- **Files**: `lumina/web/src/components/ChildGrid.vue` (new), `lumina/web/src/components/ChildCard.vue` (new)
- **Action**:
  - `ChildGrid.vue`: Section under the FocusLens. Header: child kindLabel (e.g. "STORIES") + count. Filter tabs derived from `STATUSES` (T4) — ALL + one tab per backend status (using `STATUSES[i].label`). Local `ref` for selected filter; filters the children list by `node.status === STATUSES[i].backend`. CSS grid `repeat(auto-fill, minmax(280px, 1fr))` gap-16px.
  - `ChildCard.vue`: One card per child. Props `{ node: WorkItem, childCount: number }` — `childCount` is sourced from the loaded `useHierarchy().tree` (recursive walk in a helper) and passed in by the parent grid; **NOT** from `detail.children[i].children` (the detail endpoint returns `Vec<WorkItem>` for `children`, non-recursive). Mini-layout: top row = kindLabel + " · " + id in mono; title 15.5px font-medium; summary (`body`) clamped to 2 lines via `line-clamp-2`; bottom row = `<StatusPill :status="node.status">` from T8 + (task-only) effort badge via `effortLabel(node.effort)` (T1 added the field) + child count pill (only if `kind !== 'task'`). Hover: `border-[var(--border-strong)]` + `bg-[var(--surface-2)]` + `translate-y-[-1px]`. Click: `setFocus(node.id)`.
- **Detail**: ChildGrid reads the children list from the loaded `tree` (find the focused node's `WorkItemNode` in the tree, take `.children`). If empty, render an empty-state ("No children yet"). Filter tabs use `font-mono text-[10.5px]`.
- **Acceptance**: Filter tabs reactively narrow the grid; clicking a card updates focus; child-count pills show correct values for the seeded data.
- **Effort**: M

#### T13: PortfolioEmpty.vue (full portfolio lens + epics ChildGrid)
- **Files**: `lumina/web/src/components/PortfolioEmpty.vue` (new)
- **Action**: Renders the portfolio-root view when `focusId === null`. Per `docs/design_handoff_lumina_control/app.jsx:522-599`:
  - Branch on `useHierarchy().treeStatus` — `loading`: skeleton lens placeholder; `error`: inline error block; `empty`: "No work items yet — create your first epic via the MCP `create_work_item` tool" copy; `ready`: full portfolio lens below.
  - When `ready`: render a `LensCard`-style block — corner-bracketed `.lens`; type label `PORTFOLIO · LUMINA / ALL` (mono faint); title `Plan. Dispatch. Observe.` (Instrument Serif 46px italic); summary "This is the control surface for the agentic harness. Build out epics and features as the durable structure; let sprints and tasks come and go through them. Drill into any node on the left to focus the lens."
  - 4-column `.lens-stats` KPI grid bound to portfolio rollup from `useHierarchy().descendantCounts` (which falls through to whole-tree when `focusId === null`): `EPICS` (count of roots) + sub-text "X IN FLIGHT" (filter roots by `status === 'in_progress'`); `FEATURES` (count) + sub-text "ACROSS PORTFOLIO"; `STORIES` (count) + sub-text "X BLOCKED"; `TASKS` (count) + sub-text "X EXECUTING".
  - Below the lens: a children grid header `EPICS` + count + filter tabs (ALL / IN-FLIGHT / QUEUED / DONE — derived from `STATUSES` via T4); reuse `<ChildGrid>` from T12 with the root epics as children, OR inline the epic-card rendering if `<ChildGrid>` doesn't accept a custom root.
- **Detail**: This is NOT a tiny lockup — it's a portfolio-scoped variant of the FocusLens. Clicking any epic in the grid sets `focusId` to that epic.
- **Acceptance**: When `focusId === null`, the full portfolio lens renders with corner brackets, 4-KPI grid populated, epics grid below. Clicking an epic card updates focus, replacing the portfolio lens with the focused lens.
- **Effort**: M

### Wave 4 — Integration (sequential, depends on Waves 1-3)

#### T14: Replace root layout; wire all components; delete obsolete files
- **Files**: `lumina/web/src/App.vue` (rewrite), `lumina/web/src/views/HierarchyView.vue` (delete), `lumina/web/src/components/TreeItem.vue` (delete), `lumina/web/src/assets/logo.svg` (delete)
- **Action**: First — run `git branch -a | grep -i hierarchy` to confirm no other branch references `HierarchyView.vue` / `TreeItem.vue`; if any, surface for user confirmation before proceeding. Then rewrite `App.vue` as `<script setup vapor lang="ts">` that imports `AppHeader`, `AppFooter`, `HierarchySpine`, `CenterToolbar`, `Breadcrumbs`, `FocusLens`, `PortfolioEmpty`, `ChildGrid`. On mount call `useHierarchy().loadTree()`. Template: 3-row grid (56px / 1fr / 32px) with the body row being a 3-column grid (`280px 1fr 360px`). Left column = `<HierarchySpine>`; centre column = `<CenterToolbar>` + (when `focusId !== null`) `<Breadcrumbs>` + `<FocusLens>` + `<ChildGrid>` ELSE `<PortfolioEmpty>`; right column = placeholder div with `[04 / ACTIVE SPRINT]` and `[05 / AGENT STREAM]` section headers + body text "Deferred — backend not yet implemented" (visually present so the grid stays balanced and the design intent is preserved). Delete `views/HierarchyView.vue`, `components/TreeItem.vue`, and `assets/logo.svg` via the editor's file-deletion path (the now-empty `views/` directory is git-untracked — no manual rmdir needed).
- **Detail**: This is the integration moment — every wave-2/3 component lands here. Keep `App.vue` thin: imports + grid layout + conditional rendering.
- **Acceptance**: `npm run build` succeeds with no type errors; `npm run dev` against a running backend with seeded data renders the full 3-column layout with header/footer; clicking a spine node updates the lens; clicking a child card recentres the focus.
- **Effort**: M

### Wave 5 — Verification (final)

#### T15: Full verification (build + type-check + dev visual QA)
- **Files**: none modified
- **Action**:
  - Run `npm run type-check`, `npm run build` — assert exit 0 on both.
  - Inspect `dist/` — confirm 7+ `*.woff2` under `dist/assets/`; confirm `dist/index.html` has `<title>Lumina Control</title>` and `lang="en"`.
  - Start backend: `cargo run --manifest-path lumina/Cargo.toml`. Seed: `cargo run --manifest-path lumina/Cargo.toml -- import-flow lumina-schema-deepening`.
  - **Augment seed** (one-time, manual via MCP): `add_acceptance_criterion` against one of the imported story's tasks (e.g. 2-3 criteria); `set_effort` to `s|m|l` on a handful of tasks across kinds. This is REQUIRED — `import-flow` doesn't populate AC or effort, and without this step T11's task-specific renderers and the KPI `size` count are exercised against empty data.
  - Start dev server: `npm run dev` → visit `http://localhost:5173`.
  - Manual visual QA checklist (record pass/fail per item): (a) portfolio empty-state renders the full lens + epic grid when no focus; (b) clicking an epic populates the lens with KPI counts that match a hand-count; (c) clicking a seeded task with AC shows the read-only checklist with the right checked/unchecked count; (d) status pills use the correct colour per backend status (queued/in-flight/blocked/done/cancelled); (e) right column shows the two deferred placeholders; (f) network panel shows only `GET /api/work-items` and `GET /api/work-items/{id}` calls — no 404s for unimplemented routes.
- **Acceptance** (mechanically verifiable):
  - `cd lumina/web && npm run type-check` → exit 0
  - `cd lumina/web && npm run build` → exit 0
  - `ls lumina/web/dist/assets/*.woff2 | wc -l` → ≥ 7
  - `grep "Lumina Control" lumina/web/dist/index.html` → exits 0
- **Acceptance** (manual, recorded in PR description): visual QA checklist above, 6 items pass/fail.
- **Effort**: S

---

## Dependency Graph

```
Wave 1 (parallel):  T1  T2  T3  T4  T5
                     |   |   |   |   |
                     └───┴───┴───┴───┘
                         |
Wave 2 (parallel):  T6  T7  T8  T9  T10
                     |   |   |   |   |
                     └───┴───┴───┴───┘
                         |
Wave 3 (parallel):  T11  T12  T13
                     |    |    |
                     └────┴────┘
                         |
Wave 4 (sequential): T14
                         |
Wave 5 (sequential): T15
```

- T6, T7, T9, T10 depend on T2 + T3 + T4 + T8 (tokens, composable, helpers, StatusPill).
- T8 (StatusPill) depends only on T2 + T4 — promoted to Wave 2 so it's available to T9 immediately.
- T11, T12, T13 depend on T2 + T3 + T4 + T8 + T1 (wire types for `acceptance_criteria` in T11, `effort` in T12, etc.).
- T13 (PortfolioEmpty) may reuse `<ChildGrid>` from T12 — if so, T13 has a soft dependency on T12 but the components can still be built in the same wave (T13's `ready`-branch can be wired in T14 if T12 lands first).
- T14 depends on T6, T7, T9, T10, T11, T12, T13.
- T15 depends on T14.

---

## Verification

After T15:
- Build green (`npm run build`, `npm run type-check`).
- `cd lumina && cargo build` still green (no backend changes in this plan, but smoke-test).
- Visual: tree-of-3 layout matches the design's `styles.css` spatial system; design tokens applied (warm-noir palette visible, Instrument Serif on lens title, JetBrains Mono on labels); portfolio empty-state renders full lens + epics grid.
- Behavioural: spine click → focus update → lens re-renders with new KPI counts → breadcrumbs reflect new path → child grid re-renders; treeStatus correctly drives loading/error/empty/ready branches.
- Network: app makes only the existing HTTP calls; no 404s for unimplemented routes.

---

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Vapor edge-case in `<style scoped>` interaction with Tailwind utilities | Low | If hit, fall back to `<style>` (unscoped) per-component — the design uses no per-component class collisions. |
| OKLCH not rendering correctly in older browsers | Low | The control plane is internal-tooling; documented Browser baseline is Evergreen Chromium/Firefox/Safari (~93% OKLCH support per caniuse, Mar 2026). All `oklch()` values in the spec have chroma well within sRGB gamut — no gamut-clipping risk. |
| Fontsource per-weight imports balloon the CSS payload | Low | Tailwind v4's CSS-only pipeline + Vite tree-shaking keeps unused `@font-face` blocks out of final CSS — verified via the build output. |
| Backend seed data shape differs from design fixtures (e.g. no `summary`, sparse `body`) | Medium | Lens / cards must handle null `body` gracefully — render a "—" placeholder rather than breaking layout. |
| `import-flow` doesn't populate AC / effort / complexity | High | T15 acceptance includes a manual MCP-seed step to backfill before visual QA; without it the new task-specific renderers exercise only empty arrays. |
| Design's "Saved Views" + sprint/agent placeholders in right rail look visually incomplete | Medium | Acknowledged explicitly via the `[04 / ACTIVE SPRINT]` and `[05 / AGENT STREAM]` headers + "Deferred" body text — this is intentional inventory communication, not polish debt. |
| Tree exceeds ~2k items in production | Low (today) | Switch `fetchTree()` to per-focus lazy load via `GET /api/work-items?parent_id={focused}` — route already exists. Today's seeded data (~38 nodes) is nowhere near this threshold; flag for re-plan when tree size grows. |
| Backwards compat: deleting `HierarchyView.vue` / `TreeItem.vue` breaks a parallel branch | Low (linear history) | T14 includes a `git branch -a | grep -i hierarchy` pre-check; surface any matching branch to the user before deletion. |

---

## Inventory: Items needing design decision

These are items the design handoff specifies, but which require a product decision before implementation. They are **out of scope of this plan** and need user input before a follow-up plan.

1. **Sprint composer UI semantics** — design assumes a `Sprint { id, name, range, capacity, queue: TaskId[] }` object. Open questions: (a) what does the backend model look like? a new `sprints` table? task `attributes.sprint_id`? a flow-level concept? (b) is dispatch a one-shot or a queue subscription? (c) ACP "safe-parallel" surfacing — the design has no visual for which tasks are safely parallelisable (per the memory note `lumina_relevance_and_sprint_composer.md`). What metadata drives the safe/unsafe call?
2. **Complexity → model picker UI** — the backend has `complexity: low|medium|high` per task; the memory note says `complexity→model` mapping drives dispatch. No UI exists in the design for selecting/overriding the model per task. Decision needed: (a) auto-derive at dispatch time (UI just shows the auto-choice), (b) per-task explicit picker, (c) per-sprint default override?
3. **Open question option-branch resolution UX** — the backend supports `add_open_question` + options + `set_enabling_option` + `resolve_open_question` (option pick auto-cancels exclusive tasks on other branches). The design handoff has NO UI for this. Critical for the planning/decision pass to be usable from the UI.
4. **Research-note state machine UX** — `add_research_note` → `accept/reject` + `supersede`. Design has no panel for this; lens needs a "research notes" tab or section. Where does it belong visually?
5. **Acceptance-criteria check interaction** — design renders ACs as 12×12 checkboxes but doesn't specify the mutation flow: instant-save on click? confirm? what happens to a story with `closure_gate=hard` when its last AC is unchecked while a task is `done` (illegal state)?
6. **Status pill colours for `cancelled`** — chose muted/struck variant; needs visual sign-off before this lands.
7. **Empty-state copy** — "Plan. Dispatch. Observe." is the handoff. May want to refine for lumina's tone.

---

## Inventory: Excess in the design

### Reserved for future functionality (preserve in design memory)

| Element | Future feature | Notes |
|---|---|---|
| `[04 / ACTIVE SPRINT]` panel + drag zone + DISPATCH button | Sprint composer (backend not yet implemented) | Right-column placeholder added in T14 retains the section header so the visual real estate is reserved. |
| `[05 / AGENT STREAM]` panel + AgentList/AgentCard | Agentic harness telemetry (entire backend missing) | Same — placeholder in right column. The design's `setInterval` mock is prototype-only. |
| `[02 / SAVED VIEWS]` placeholder in left rail | User-saved filters / bookmarks | Visually disabled placeholder rendered in T9. |
| Tree alternate view (`tree-view.jsx`) | Orthogonal Epic→Feature→Story graph | The `view` ref in `useHierarchy` toggles between `focus` and `tree`; "tree" currently shows a "DEFERRED" placeholder. Wire the actual graph in a follow-up plan once a use case justifies it. |
| Task action buttons (DISPATCH / ADD TO SPRINT / EDIT / BLOCK) | Wait on HTTP routes for status mutation, sprint, etc. | Rendered disabled in T11; activate as routes land. |
| AC checkbox interactivity | Wait on `POST /api/acceptance-criteria/{id}/check` route | Rendered read-only in T11; one-line composable change to activate once endpoint exists. |
| Status `<select>` dropdown (currently in old HierarchyView, removed) | Wait on richer status mutation design | Existing PATCH route works; UI removed pending design for the lens's status picker. |
| **Findings panel** (severity-coloured `detail.findings` list — present in old `HierarchyView.vue:77-87`) | Wait on UX decision for where it lives (right-rail tab? lens footer section? dedicated `[06]` section?) | Removed during port; backend still serialises `findings` via `WorkItemDetail`; T1 wire-extends the type, so renderer is one component away once placement is decided. |
| Keyboard shortcuts (S / D / Backspace / arrows) | Future composable `useKeyboardNav` | Hints rendered in `AppFooter` are static; not wired. |
| **8. Accessibility pass** | ARIA tree semantics on spine, keyboard focus management, `aria-current="page"` on focused row, `aria-expanded` on collapsibles, `aria-describedby` on disabled buttons, screen-reader announcements | Deferred to follow-up — internal-tooling baseline ships without; revisit before wider rollout. |
| **9. Component test harness** | Vitest + smoke test for `descendantCounts` (recursive count, effort-weight mapping, divide-by-zero) | `descendantCounts` is the highest-bug-risk new logic in this plan; deferred to a follow-up `/test-bootstrap` run, but flagged here so it doesn't get lost. |

### Truly decorative / sample-only (discard)

| Element | Reason for discard |
|---|---|
| `tweaks-panel.jsx` (runtime UI tweaker) | Prototype dev tool — author flagged as non-production. |
| Giant background watermark (item code in Instrument Serif 220px behind lens) | Author noted: "design review decided this adds no value". |
| Hourglass emoji on agent runtime | Already excluded by design author. |
| `Lumina Control.html` script-loading shell | React/Babel CDN imports are scaffold for the prototype; replaced by Vite. |
| `data.js` fixture data | Replaced by real backend; the lumina hierarchy from `import-flow` is the source of truth in dev. |
