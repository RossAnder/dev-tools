// Focus-plan composable — module-singleton state + an async mutator for the
// `PATCH /work-items/{id}/focus-plan` structured-patch route (migration 0010).
//
// Mirrors the `useEpicPlan.ts` / `useStoryPlan.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useFocusPlan()` shares the same refs.
//   - The mutating action returns a discriminated `Result<T, E>` so call
//     sites can narrow on success/failure WITHOUT coupling to the singleton
//     `error` ref (still set as a side effect for the error-banner).
//   - The API surface is swappable via `__setApiForTests` /  `__resetForTests`.
//
// Pure mutator semantics: each PATCH returns the re-fetched
// {@link WorkItemDetail}; `apply` folds it into the shared hierarchy detail
// singleton so the FocusLens reflects the new `attributes.framing`. Local
// state is confined to `lastUpdated` plus `loading` / `error`.
//
// The error-handling/refresh contract is shared with `useEpicPlan.ts` via the
// `makePlanComposable` factory; the singleton built here is distinct from the
// epic one (one factory call ⇒ one private singleton). The PATCH targets the
// `focus`-kind setter, which kind-gates to `focus` (non-focus → 422 on `error`).

import { setFocusPlan } from '@/api'
import type { SetFocusPlanBody } from '@/api'
import { makePlanComposable } from './makePlanComposable'

import type { Result } from './result'
export type { Result }

const plan = makePlanComposable<SetFocusPlanBody, 'setFocusPlan'>('setFocusPlan', setFocusPlan)

/** Shared epic/focus return shape; `apply(focusItemId, { framing? })`. */
export const useFocusPlan = plan.use

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export const __setApiForTests = plan.setApiForTests

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export const __resetForTests = plan.resetForTests
