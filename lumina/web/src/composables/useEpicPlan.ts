// Epic-plan composable — module-singleton state + an async mutator for the
// `PATCH /work-items/{id}/epic-plan` structured-patch route (migration 0010).
//
// Mirrors the `useStoryPlan.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useEpicPlan()` shares the same refs.
//   - The mutating action returns a discriminated `Result<T, E>` so call
//     sites can narrow on success/failure WITHOUT coupling to the singleton
//     `error` ref (which is still set as a side effect for the UI's
//     error-banner subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults).
//
// Pure mutator semantics: this composable does NOT cache the epic detail.
// Each PATCH returns the re-fetched {@link WorkItemDetail}; `apply` also folds
// it into the shared hierarchy detail singleton (`useHierarchy().refresh`) so
// the FocusLens reflects the new `attributes.outcome` / `attributes.context`
// without a manual reload. The local state is confined to `lastUpdated` plus
// `loading` / `error`.
//
// The error-handling/refresh contract is shared with `useFocusPlan.ts` via the
// `makePlanComposable` factory; the singleton built here is distinct from the
// focus one (one factory call ⇒ one private singleton). The PATCH targets the
// `epic`-kind setter, which kind-gates to `epic` (non-epic → 422 on `error`).

import { setEpicPlan } from '@/api'
import type { SetEpicPlanBody } from '@/api'
import { makePlanComposable } from './makePlanComposable'

import type { Result } from './result'
export type { Result }

const plan = makePlanComposable<SetEpicPlanBody, 'setEpicPlan'>('setEpicPlan', setEpicPlan)

/** Shared epic/focus return shape; `apply(epicId, { outcome?, context? })`. */
export const useEpicPlan = plan.use

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export const __setApiForTests = plan.setApiForTests

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export const __resetForTests = plan.resetForTests
