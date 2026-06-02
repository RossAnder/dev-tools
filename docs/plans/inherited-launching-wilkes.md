# Plan: Lumina SPA — Tabbed, editable work-item detail lens (all kinds)

**Plan path**: docs/plans/inherited-launching-wilkes.md
**Created**: 2026-06-02
**Status**: draft
**Revised**: 2026-06-02 — plan-review round 1 findings applied (P1–P21)

## Context

The lumina SPA's sole node-detail renderer, `lumina/web/src/components/FocusLens.vue`, branches by kind for `epic`/`focus`/`project`/`task` but has **no `story` branch** — and renders almost none of the rich planning data even where it exists. Stories show only the generic header (title/body/status + a few chips). Yet the store, MCP tools, axum API, and the full set of TypeScript composables/API clients are **complete and tested** for every field and child collection of the per-item-detail surface (one exception — the migration-0011 Part-B surface — is called out in Scope): the gap is purely the Vue components. The deep planning composables (`useStoryPlan`, `useResearchNotes`, `useRisks`, `useOpenQuestions`, `useRejectedAlternatives`, `useTaskSpec`, `useFindings`, `useReadiness`, `useDispatchPlan`, `useTaskDependencies`, `useActivity`, `useContextBlocks`) are imported by **zero** components; only `useScalars`/`useRepoLinks`/`useEpicPlan`/`useFocusPlan`/`useAcceptanceCriteria` are wired today, via the five inline editors `FocusLens` mounts (`FocusLens.vue:7-11`).

This plan builds a **tabbed detail lens for every item kind** (project/epic/focus/story/task) that surfaces all stored data, grouped into a small number of tabs so the hero card never becomes too tall and information density stays manageable. Panels are **editable**, with interactivity matched to each data shape (modal editor for long-form text, segmented switches for enums, inline state-setting and list CRUD). The end state: a consistent, navigable, edit-in-place lens that replaces the current ad-hoc per-kind rendering and makes the planning data a first-class part of the UI.

## Scope

**In:**
- `lumina/web/src/` only — Vue components, composables, and pure-TS helpers.
- A reusable tab framework (registry + strip + state), accessible modal/enum/confirm primitives, kind-aware panel components, and an execution section rendered **below** the hero card.
- Migrating the existing inline editors (`OutcomeEditor`, `ShapeEditor`, `FramingEditor`, `EpicCloseCriteriaPanel`, `RepoLinksPanel`) into the new panel/registry framework.

**Out:**
- Backend, MCP, axum API, and composable/API-client layers — already complete; **no changes**. (If a genuinely missing read/write surface is discovered mid-build, raise it; current evidence says there are none.)
- The deferred right-column (sprint composer, agent stream) and the `tree`/`pty` views. NOTE: the migration-0011 Part-B surface (runs, sprints, finding-decisions/triage, finding-queue, batch-write, query) has NO frontend composable yet — correctly out of scope here, but the Context claim that the composable layer is "complete" is false for it; a later finding-triage affordance would need a `useFindingQueue`/`useFindingDecision` composable built first (a backend-adjacent task this plan excludes).
- Adding a component-test runner (Vitest) — explicitly not in scope (see User Decisions Q4).
- **Per-element "ask an agent" controls** — the planned future direction where each editable element gets a control that opens a *little floating agent interaction* via the PTY service (`usePtySession`/`PtyConsole`) to mutate that datum. **Not built here** — manual human edits are the current focus — but the editing-affordance pattern below must **not preclude** it.

**Affected areas:** `lumina/web/src/components/**`, `lumina/web/src/composables/**`, `lumina/web/src/__tests__/**`

**Estimated file count:** ~20 unique files across 4 waves. Wave 3 ({T10,T11,T12}) is the heaviest: although each is one new `.vue` file, T11 also migrates+deletes `EpicCloseCriteriaPanel` and T12 also edits `FocusLens` to retire its inline context-blocks section — treat T11/T12 as TWO-file edits each, with behaviour-preservation (migration-regression) obligations that build-clean cannot verify. This exceeds the single-batch guideline, so the plan is **phased into 4 dependency waves**; implement wave-by-wave (each wave is ≤6 files per agent batch and leaves a green build).

## Research Notes

Sourced and vetted during planning (Phase 3/5). Grades: H = high, M = medium (smoke-test before relying).

- **[H] Vue 3.6 Vapor has feature parity with VDOM mode except `Suspense`** (vuejs/core v3.6.0-beta.1 release notes; project pins `vue@3.6.0-beta.13`). `<component :is>`, `KeepAlive`, and `Transition` are supported in Vapor; the 3.6.0-beta.1 notes make a *blanket* parity claim, not an itemized guarantee — `Suspense` is excluded (a Vapor child may still render inside a VDOM `<Suspense>`), and the notes also exclude the Options API, `app.config.globalProperties`, `getCurrentInstance()` (returns null), and `@vue:xxx` per-element hooks (none used here — all panels are `<script setup>` + composables). `<component :is>`/`KeepAlive`/`Transition` are implied by parity, not named, so treat the itemization as [M]. *Impact:* dynamic-component tab mounting is available; reverses the older "no KeepAlive in Vapor" assumption.
- **[M] `v-if`/`v-show` semantics in Vapor match VDOM** (inferred from Vue docs + the parity statement; no Vapor-specific page). *Impact:* a `v-if`-mounted single-active panel is safe. **Key de-risk:** all meaningful state lives in **module-singleton composables**, not local component state, so unmounting an inactive panel does **not** lose fetched data. → **Design to NOT depend on `KeepAlive`**; treat it as optional polish for form-draft/scroll preservation only.
- **[H] WAI-ARIA Tabs pattern** (w3.org/WAI/ARIA/apg/patterns/tabs/): `role="tablist"` › `role="tab"` › `role="tabpanel"`; `aria-selected`/`aria-controls`/`aria-labelledby`; **roving tabindex** (active tab `tabindex=0`, rest `-1`); **manual activation** for content tabs (arrows move focus, Enter/Space/click activates); Left/Right wrap, Home/End jump.
- **[H] WAI-ARIA Dialog (Modal) + native `<dialog>`** (w3.org APG dialog-modal; MDN `<dialog>`): `HTMLDialogElement.showModal()` provides focus-trap, Esc-to-close, top-layer, and automatic background inertness (`::backdrop`) for free. Manual residue: defensive focus-restore to the trigger after programmatic `close()`, and `aria-labelledby` wiring to the title. *Impact:* build the modal primitive on native `<dialog>`; do **not** intercept Enter inside the textarea (newline, not submit). Note (P21): a native `<dialog>` opened with `showModal()` carries an implicit `role="dialog"` + top-layer modality, so an explicit `aria-modal` is generally unnecessary (some guidance discourages it) — confirm during T5 smoke that a screen reader announces the modal as a dialog.

## User Decisions

> Captured via the Phase 4 directed-questions gate. Treat as authoritative requirements.

- **Q1 — Tab grouping:** Lifecycle-phase grouping (few broad tabs), **with one refinement**: *"Execution" elements (child tasks, dispatch plan, task dependencies) must NOT be tabs in the hero card — render them in the separate child-item section below the main card.* So hero tabs = the item's **own** content; children + execution views render **below**.
- **Q2 — Edit scope:** **Editable**, with interactivity matched to the data shape — *"modal editor for long-form text, appropriate enum variant switching, state-setting."*
- **Q3 — Kinds scope:** **All kinds; migrate the existing editors into the registry** (project/epic/focus/story/task unified under one tab framework; `OutcomeEditor`/`ShapeEditor`/`FramingEditor`/`EpicCloseCriteriaPanel`/`RepoLinksPanel` become panel content).
- **Q4 — Testing:** **Thin components + bun-test the extracted logic.** Pure-TS modules (registry, keyboard reducer, tab-state, any shaping helpers) are bun-tested; `.vue` components stay declarative and are verified via `vue-tsc` build + manual smoke. **No Vitest.**

## Approach

### Per-kind tab matrix (hero card)

Grouping = lifecycle phases, Execution removed to the child section below. Tabs are kind-gated by the registry.

| Kind | Overview | Decisions | Quality | Activity | Repos |
|---|---|---|---|---|---|
| **project** | body | — | — | activity, context blocks | repo_links |
| **epic** | outcome, context, close-criteria | — | — | activity, context blocks | — |
| **focus** | shape, framing | — | — | activity, context blocks | — |
| **story** | problem_statement, execution_strategy, not_doing, verification_commands, readiness | research_notes, open_questions, rejected_alternatives | risks, findings, acceptance_criteria | activity, context blocks | — |
| **task** | execution_detail, files_touched, outcome, scalars (effort/complexity/tier/task_kind) | — | acceptance_criteria, findings | activity, context blocks | — |

Header (kept from current `FocusLens`): kind label · id · title · body · `StatusPill` · progress bar (non-task) · read-only chips (relevance/closure/complexity/origin). Scalar **editing** lives in the Overview "Properties" section (header chips stay read-only to avoid double-rendering).

**Below the card (child/execution section):** the existing `ChildGrid` of children, plus — **story-only** — a new `ExecutionSection` rendering the **dispatch plan** (`useDispatchPlan`) and **task batches/dependencies** (`useTaskDependencies`). This honours Q1's "execution belongs below, not in tabs."

### Editing-control mapping (Q2)

| Data shape | Control | Backed by |
|---|---|---|
| Long-form text (problem_statement, execution_strategy, not_doing, body, outcome, context, framing, execution_detail, note/risk/finding/alt bodies) | **`TextFieldModal`** (native `<dialog>` + textarea + Save/Cancel) | `useStoryPlan().apply`/`useEpicPlan().apply`/`useFocusPlan().apply`/`useTaskSpec().apply` (+ `update*` on list composables; the API fns `setStoryPlan`/`setEpicPlan`/`setFocusPlan`/`setTaskSpec` are exposed on each composable as `apply`) |
| Enums (relevance, effort, complexity, closure_gate, task_kind, tier, shape, severity, research-note state) | **`EnumSwitch`** segmented control (generalises `ShapeEditor`) | `useScalars.*`, `updateRisk`/`updateFinding`/`updateResearchNote` |
| Boolean / state (acceptance check·uncheck, finding/question resolve) | inline toggle / buttons | `check`/`uncheck`/`resolve*` |
| Lists (research_notes, risks, rejected_alternatives, open_questions, findings, acceptance_criteria, context_blocks, repo_links, files_touched) | row list + add (inline/modal) + per-row edit + **`ConfirmButton`** for destructive remove | the matching `useX` add/update/supersede/remove |
| `verification_commands` (build/test/lint/smoke) | four short inline inputs | `useStoryPlan().apply` |

> *Review note (P17): `EnumSwitch` is a dumb controlled input (`options`/`modelValue` → emit); it does not literally "generalise" `ShapeEditor`, which owns its own `useScalars().setShape` + `useHierarchy().refresh` (`ShapeEditor.vue:37-42`). Migrating `ShapeEditor` relocates that persist + fold-back logic up into `OverviewPanel` (see the refresh-contract note under Component architecture). The active-state segmented-control classes T4 reuses do exist verbatim at `ShapeEditor.vue:62`.*

### Component architecture

- **Declarative panel registry** (the stale plan's best idea, made bun-testable): `composables/panelRegistry.ts` exports pure `TabDef[]` (`{ id, label, order, kinds: Kind[] }`) and `tabsForKind(kind): TabDef[]`. **No `.vue` imports** in this file → bun-testable. `FocusLens` resolves the active panel via a pure `composables/panelComponentMap.ts` resolver (bun-tested, including the unwired-id "coming soon" fallback), and only mounts the resolved component.
- **Pure helpers** (bun-tested): `composables/tabKeyboard.ts` (roving-index reducer: `(current, key, count) → nextIndex`, wrapping, Home/End) and `composables/useTabState.ts` (sessionStorage-backed active tab per entity id, with restore-validation against current tabs).
- **`TabStrip.vue`**: `role="tablist"` + `role="tab"` buttons, roving tabindex, `aria-selected`/`aria-controls`, manual activation, keyboard via `tabKeyboard`. Emits active-tab.
- **UI primitives** (`components/ui/`): `Modal.vue` (native `<dialog>`, `showModal`/`close`, `aria-labelledby`, defensive focus restore), `TextFieldModal.vue` (Modal + textarea + Save/Cancel), `EnumSwitch.vue` (segmented control), `ConfirmButton.vue` (two-step inline confirm).
- **Panels** (`components/panels/`): `OverviewPanel.vue`, `DecisionsPanel.vue`, `QualityPanel.vue`, `ActivityPanel.vue`, `ReposPanel.vue` — thin; each reuses the existing composables (no new data logic) and the primitives above. Each panel MUST `watch(entityId, { immediate: true })` and call the composable's `bind(itemId)` seeder — module-singleton state is not auto-keyed to the focused node (see `useFindings.bind`, and the `RepoLinksPanel`/`OutcomeEditor` onMounted+watch(itemId) idiom). When the selected node changes kind, `useTabState` must re-validate the stored tab id against the new kind's `tabsForKind` set (a story's `decisions`/`quality` tab is invalid for a task) and fall back to `overview`.
- **`FocusLens.vue`** keeps its header, then renders `<TabStrip>` + the active panel via `<component :is>` (single-active `v-if`; data persists in composables, so no `KeepAlive` dependency).
- **`ExecutionSection.vue`** + a small edit in `App.vue` to render it below the card for stories.

All components follow the established idioms: `<script setup vapor lang="ts">`, inline Tailwind + `var(--*)` tokens (no `<style scoped>`). Refresh contracts DIFFER per composable: list composables (`useFindings`, `useResearchNotes`, `useRisks`, `useRejectedAlternatives`, `useAcceptanceCriteria`, `useEpicPlan`/`useFocusPlan`) DO call `useHierarchy().refresh(id)` internally, so panels must NOT re-refetch after them — but `useScalars.*` setters deliberately do NOT refresh; they return the re-fetched `WorkItem` for the caller to fold into `useHierarchy().detail`. The OverviewPanel (T7) enum/scalar edit path MUST fold the scalar result back into `detail`, or header chips / Overview Properties won't re-render after an enum change.

### Forward compatibility — per-element PTY-agent affordance (future, not built now)

Each editable element will eventually carry a control that opens a *floating agent interaction* (a small `PtyConsole` driven by `usePtySession`) scoped to that field/collection, so an agent can make the change instead of the human. To keep that future cheap, route **every** editable element through one consistent wrapper — a thin `EditableElement.vue` (or `PanelRow.vue`) that owns: (a) the label/value layout, (b) the manual-edit affordance (click → modal / enum switch / inline), and (c) a reserved **trailing action slot** (empty now) where the future per-element agent trigger will mount. The wrapper also carries a stable element descriptor (`{ workItemId, field | collection, kind }`) — exactly the context a scoped PTY session will need. Building this wrapper now (Wave 1, `components/ui/`) costs little and means the agent affordance is a later additive change to one component, not a re-architecture of every panel. **No PTY wiring is added in this plan** — only the empty slot + descriptor.

### Wave sequencing (each wave leaves a green build)

- **Wave 1 — Framework + primitives + read-only Overview.** Delivers the tab shell and an Overview tab that renders every kind's data **read-only**. This alone closes the "stories look empty" gap.
- **Wave 2 — Editing in Overview + migrate existing editors.** Make Overview fields editable; fold `OutcomeEditor`/`ShapeEditor`/`FramingEditor` into `OverviewPanel`, `RepoLinksPanel` into `ReposPanel`; retire the migrated components.
- **Wave 3 — Story/task rich tabs.** `DecisionsPanel`, `QualityPanel` (incl. acceptance-criteria migration from `EpicCloseCriteriaPanel`), `ActivityPanel`.
- **Wave 4 — Execution section below the card.** `ExecutionSection` (dispatch plan + task deps), wired into `App.vue`.

## Verification Commands

```
build: cd lumina/web && npm run build      # vue-tsc --build (type-check) + vite build
test:  cd lumina/web && bun test           # pure-TS composables/helpers
lint:  cd lumina/web && npx vue-tsc --noEmit   # type gate (no eslint config in repo)
smoke: cd lumina && cargo run              # serve, open SPA, click a story node
```

## Tasks

### Phase 1 / Wave 1 — Framework + primitives + read-only Overview (parallel where noted)

#### T1: Pure tab registry
- **Files**: `lumina/web/src/composables/panelRegistry.ts`, `lumina/web/src/__tests__/panel-registry.test.ts`
- **Depends on**: none
- **Action**: Export `TabDef` (`{ id, label, order, kinds: Kind[] }`), the canonical `TAB_DEFS` array (overview/decisions/quality/activity/repos with the per-kind matrix above), and `tabsForKind(kind): TabDef[]` (filtered by `kinds`, sorted by `order`). No `.vue` imports.
- **Acceptance**: `bun test` covers `tabsForKind('story')` → `[overview, decisions, quality, activity]` in order, `tabsForKind('project')` → `[overview, repos, activity]`, `tabsForKind('task')` → `[overview, quality, activity]`.

#### T2: Roving-tabindex keyboard reducer
- **Files**: `lumina/web/src/composables/tabKeyboard.ts`, `lumina/web/src/__tests__/tab-keyboard.test.ts`
- **Depends on**: none
- **Action**: Pure `nextTabIndex(current, key, count): number` implementing ArrowLeft/Right (wrap), Home, End; returns `current` for unrelated keys. Manual-activation model.
- **Acceptance**: `bun test` covers wrap at both ends, Home/End, and no-op keys.

#### T3: Tab-state composable (sessionStorage)
- **Files**: `lumina/web/src/composables/useTabState.ts`, `lumina/web/src/__tests__/tab-state.test.ts`
- **Depends on**: T1
- **Action**: `useTabState(entityId, validTabIds)` — active-tab ref persisted to `sessionStorage` keyed `tabstrip:<entityId>`; on read, validate the stored id against `validTabIds`, else fall back to first. Expose a storage seam (`__setStorageForTests`, injecting a `Storage`-shaped object) plus a `__resetForTests` companion, modelled on the repo's `__setApiForTests`/`__resetForTests` idiom — no Web Storage is used anywhere today, so this is the first such seam (copy the api-seam shape).
- **Acceptance**: `bun test` (with a Map-backed storage stub) covers persist→restore, invalid-key fallback, and per-entity isolation.

#### T4: TabStrip component
- **Files**: `lumina/web/src/components/TabStrip.vue`
- **Depends on**: T1, T2, T3
- **Action**: Render `role="tablist"` + `role="tab"` buttons from `tabsForKind`, roving tabindex, `aria-selected`/`aria-controls`, keydown via `tabKeyboard`, active state styled like the `ShapeEditor` segmented control (`border-[var(--accent)] text-[var(--accent)] bg-[var(--surface-3)]`). Props: `kind`, `entityId`; emit/`v-model` active tab id.
- **Acceptance**: `npm run build` clean; manual: arrows move focus and wrap, Enter/click activates, active tab visually distinct.

#### T5: Modal + UI primitives + editable-element wrapper
- **Files**: `lumina/web/src/components/ui/Modal.vue`, `lumina/web/src/components/ui/EnumSwitch.vue`, `lumina/web/src/components/ui/ConfirmButton.vue`, `lumina/web/src/components/ui/EditableElement.vue`
- **Depends on**: none
- **Action**: `Modal.vue` wraps native `<dialog>` (`showModal()`/`close()`, `aria-labelledby` to a titled `<h2>`, capture trigger + defensive focus-restore on close, `::backdrop` styling via tokens). `EnumSwitch.vue` = segmented control (props: `options: { value: string; label: string }[]`, `modelValue: string`; emit `update:modelValue` on select; no-op guard on unchanged). `ConfirmButton.vue` = two-step inline confirm for destructive actions. `EditableElement.vue` = the consistent per-element wrapper (`label` prop + default slot for the value/edit affordance + a named **`#agent-action` slot that is empty now**) carrying a `descriptor` prop `{ workItemId, field?, collection?, kind }` — the forward-compat seam for the future per-element PTY-agent control.
- **Acceptance**: `npm run build` clean; manual: Esc closes Modal, focus returns to trigger, background inert; every Overview field (T6+) renders inside an `EditableElement` with the reserved (empty) agent slot present in the DOM.

#### T6: TextFieldModal + read-only OverviewPanel + FocusLens integration
- **Files**: `lumina/web/src/components/ui/TextFieldModal.vue`, `lumina/web/src/components/panels/OverviewPanel.vue`, `lumina/web/src/components/FocusLens.vue`
- **Depends on**: T4, T5
- **Action**: `TextFieldModal.vue` = `Modal` + labelled textarea + Save/Cancel (no Enter-submit). `OverviewPanel.vue` renders each kind's Overview fields **read-only** for this wave (story plan attrs, epic outcome/context, focus shape/framing, task spec, project body), reading from `useHierarchy().detail`. Edit `FocusLens.vue`: keep the header; below it render `<TabStrip :kind :entity-id>` + the active panel via `<component :is>` from a `tabId→component` map held in `FocusLens.vue` (value type: a `markRaw`-wrapped component reference, matching the Vapor idiom; reuse FocusLens's existing `attrString`/`focusShape`/`epicOutcome` computeds for the read-only field values; only `OverviewPanel` wired this wave; other tab ids render a temporary "coming soon" stub). Insert after the header block (`</header>`), before the existing kind-specific sections (which stay until migrated in later waves).
- **Acceptance**: `npm run build` clean; manual: selecting a **story** shows an Overview tab populated with its problem statement / execution strategy / not-doing / verification commands (read-only). The S1.1 trial story renders rich content.

### Phase 2 / Wave 2 — Editing in Overview + migrate existing editors (after Wave 1)

#### T7: Make OverviewPanel editable + migrate epic/focus editors
- **Files**: `lumina/web/src/components/panels/OverviewPanel.vue`, `lumina/web/src/components/FocusLens.vue`
- **Depends on**: T6
- **Action**: Wire long-form fields → `TextFieldModal` → the composables' `apply` (`useStoryPlan().apply`/`useEpicPlan().apply`/`useFocusPlan().apply`/`useTaskSpec().apply`); enums/scalars → `EnumSwitch` → `useScalars.*` (folding the scalar result back into `useHierarchy().detail` — `useScalars` does not refresh); `verification_commands` → four inline inputs. Fold the content of `OutcomeEditor.vue`, `ShapeEditor.vue`, `FramingEditor.vue` into `OverviewPanel` (epic outcome/context + close-criteria entry point, focus shape/framing). Remove those three components' usage from `FocusLens.vue`.
- **Acceptance**: `npm run build` clean; manual: edit a story's problem statement via modal → persists and re-renders; switch a focus's shape via `EnumSwitch` → persists; epic outcome/context editable.

#### T8: Repos panel (migrate RepoLinksPanel)
- **Files**: `lumina/web/src/components/panels/ReposPanel.vue`, `lumina/web/src/components/FocusLens.vue`
- **Depends on**: T7
- **Action**: New `ReposPanel.vue` reusing `useRepoLinks` (list, add, remove, setPrimary) with `ConfirmButton` for remove; register as the project `repos` tab; drop the inline `RepoLinksPanel` mount from `FocusLens`.
- **Acceptance**: `npm run build` clean; manual: project Repos tab lists links, add/remove/set-primary work.

#### T9: Retire migrated editor components
- **Files**: delete `OutcomeEditor.vue`, `ShapeEditor.vue`, `FramingEditor.vue`, `RepoLinksPanel.vue` (+ any now-dead imports); keep `EpicCloseCriteriaPanel.vue` until T11. Also scrub or re-point the `RepoLinksPanel` doc-comment references in `lumina/web/src/assets/tokens.css:24`, `PtyConsole.vue`, and `PtyMessage.vue` so the retirement grep gate (Verification) does not trip on dead prose.
- **Depends on**: T7, T8
- **Action**: Remove the four migrated components and their references once Overview/Repos cover them. Update `App.vue`/imports as needed.
- **Acceptance**: `npm run build` clean (no unresolved imports); `bun test` green; grep shows no remaining import/template references to the deleted components.

### Phase 3 / Wave 3 — Story/task rich tabs (after Wave 2)

#### T10: DecisionsPanel (story)
- **Files**: `lumina/web/src/components/panels/DecisionsPanel.vue`
- **Depends on**: T7
- **Action**: Research notes (`useResearchNotes`: add/update/supersede; bodies via `TextFieldModal`; state via `EnumSwitch`), open questions + options (`useOpenQuestions`: add/addOption/resolve), rejected alternatives (`useRejectedAlternatives`: add/update/supersede/remove). Register as the story `decisions` tab.
- **Acceptance**: `npm run build` clean; manual: a story's Decisions tab lists + edits all three collections; the S1.1 research note renders.

#### T11a: QualityPanel (risks + findings + readiness)
- **Files**: `lumina/web/src/components/panels/QualityPanel.vue`
- **Depends on**: T7
- **Action**: Risks (`useRisks`: CRUD, severity `EnumSwitch`), findings (`useFindings`: list, severity, resolve/supersede), readiness (`useReadiness`: read-only summary, story). Register for story (full) and task (findings). Acceptance-criteria + epic-close-criteria migration is split into T11b.
- **Acceptance**: `npm run build` clean; manual: story Quality tab shows/edits risks, findings, and the readiness summary; task Quality tab shows findings.

#### T11b: Acceptance-criteria sub-component + EpicCloseCriteriaPanel migration
- **Files**: `lumina/web/src/components/panels/QualityPanel.vue` (acceptance-criteria sub-section), delete `EpicCloseCriteriaPanel.vue`
- **Depends on**: T11a
- **Action**: Acceptance criteria (`useAcceptanceCriteria`: add/check/uncheck/remove) rendered in `QualityPanel` for story + task; **absorb `EpicCloseCriteriaPanel` logic so epics use this path too**, then delete `EpicCloseCriteriaPanel.vue` and drop its inline mount from `FocusLens` (`FocusLens.vue:250`). Route epic close-criteria through the shared path.
- **Acceptance**: `npm run build` clean; manual: story/task acceptance criteria check toggles persist; epic close-criteria still work via the migrated path; `rg "EpicCloseCriteriaPanel" lumina/web/src` returns no import/template references.

#### T12: ActivityPanel (all kinds)
- **Files**: `lumina/web/src/components/panels/ActivityPanel.vue`, `lumina/web/src/components/FocusLens.vue` (remove the inline context-blocks `<section>` at `FocusLens.vue:333-355`)
- **Depends on**: T6
- **Action**: Activity log (`useActivity`: read-only timeline + add a `comment`/`execution` entry), context blocks (`useContextBlocks`: create/link/unlink). Register as the `activity` tab for all kinds (replaces the inline context-blocks section in `FocusLens`).
- **Acceptance**: `npm run build` clean; manual: Activity tab shows the timeline and context blocks; the S1.1 execution activity entry renders; `rg "context-blocks" lumina/web/src/components/FocusLens.vue` returns nothing (inline section removed).

### Phase 4 / Wave 4 — Execution section below the card (after Wave 3)

#### T13: ExecutionSection + App wiring
- **Files**: `lumina/web/src/components/ExecutionSection.vue`, `lumina/web/src/App.vue`
- **Depends on**: T10, T11a, T11b, T12 (after the hero tabs are stable)
- **Action**: `ExecutionSection.vue` (story-only) renders the dispatch plan (`useDispatchPlan`: batches × tier/effort/complexity) and task batches/dependencies (`useTaskDependencies`: `refreshBatches`, `bind`, `addEdge`/`removeEdge` with cycle-envelope handling via the `cycleError` ref), read-mostly with edge add/remove. Wire into `App.vue` below `FocusLens`, alongside the existing `ChildGrid`, gated to `kind === 'story'`.
- **Acceptance**: `npm run build` clean; manual: a story shows, below the card, its task waves with tiers and dependency edges; non-stories show only `ChildGrid` (unchanged).

## Dependency Graph

```
Wave 1:  T1 ┐         T2 ┐
            ├─ T3 ── T4 ─┤
            │            ├── T6 ── (Wave 2/3 panels)
         T5 ┘────────────┘
Wave 2:  T6 ── T7 ── T8 ── T9
Wave 3:  T7 ── T10, T11a ── T11b ;  T6 ── T12
Wave 4:  T10, T11b, T12 ── T13
```

Parallel batches: **W1**: {T1, T2, T5} then {T3} then {T4} then {T6}. **W2**: {T7} then {T8} then {T9}. **W3**: {T10, T11a, T12} then {T11b}. **W4**: {T13}.

## Verification

- Per wave: `cd lumina/web && npm run build` (vue-tsc + vite) green, and `bun test` green.
- After Wave 1: a story node renders a populated read-only Overview tab (the trial story S1.1 shows its problem statement etc.).
- After Wave 3: every kind shows its full tab set; editing each control persists and re-renders (verified against a running `cargo run` backend with the trial data).
- After Wave 4: stories show dispatch plan + task deps **below** the card; the hero card has no Execution tab.
- Final: `rg "OutcomeEditor|ShapeEditor|FramingEditor|RepoLinksPanel|EpicCloseCriteriaPanel" lumina/web/src` returns no references in imports/templates. NOTE: `RepoLinksPanel` is name-dropped as a layout/Vapor-convention exemplar in doc-comments of `assets/tokens.css:24`, `PtyConsole.vue`, `PtyMessage.vue` — T9/T11b must also scrub or re-point these, or the gate fails on dead prose rather than live references.
- *Acceptance-gate note (P12): the editing/panel tasks (T4, T6, T7, T8, T10, T11a, T11b, T12, T13) carry only `build-clean` + manual acceptance — no signal an autonomous agent can satisfy. Where a task folds branching/shaping logic (which field → which control, which tabs per kind), push it into a bun-tested pure helper (e.g. `overviewFieldsForKind(kind)`, `controlForFieldShape(...)`) or add a per-task `rg` gate (as added to T9/T11b/T12) so the executor has a binary signal beyond "it compiles."*

## Risks

- **[M] Vapor beta capability drift.** Tab mounting relies on `<component :is>` in Vapor (blanket parity claim per beta.1; not individually itemized) and `KeepAlive` specifics are M-grade. Vue 3.6 Vapor is officially "still considered unstable" and the pin is already at *beta.13*, so compiler output and `vapor` SFC semantics can shift between betas and again at the 3.6.0 stable cut. *Mitigation:* design depends only on `v-if` + composable-held state (no `KeepAlive`); keep the exact pin (no `^`/`~` — already in place); re-smoke the tab shell on ANY Vue bump, treating a beta→beta or beta→stable bump as a deliberate, separately-verified change.
- **[M] Component-rendering tests are out of scope.** Per Q4, `.vue` panels are verified by `vue-tsc` build + manual smoke, not unit tests. *Mitigation:* push all branching logic into the bun-tested pure modules (registry/keyboard/state/panelComponentMap/field-shaping); keep panels declarative.
- **[L] FocusLens merge contention across waves.** Multiple tasks edit `FocusLens.vue` (T6, T7, T8, T11b, T12). *Mitigation:* the registry/`tabId→component` map localises change; sequence FocusLens edits (T6 → T7 → T8, then the later-wave T11b/T12 edits) rather than parallelising them — see the corrected W2 batch (`{T7} then {T8}`) and T12's explicit FocusLens Files entry.
- **[L] Migration regressions.** Retiring `OutcomeEditor`/`ShapeEditor`/`FramingEditor`/`RepoLinksPanel`/`EpicCloseCriteriaPanel` could drop a behaviour. *Mitigation:* migrate-then-delete (T9, T11b) only after the replacement panel is verified; the grep gate in Verification catches stragglers.
- **[L] Native `<dialog>` focus-restore.** The browser restores focus to the element focused when `showModal()` ran — not always the intended trigger (e.g. a list-row button later re-keyed by the `useHierarchy().refresh(id)` a mutation fires). *Mitigation:* capture the trigger explicitly at open time and restore it defensively in `Modal.vue` after `close()`; the APG assigns focus-restore to the author, so this defensive restore is correct, not redundant.
