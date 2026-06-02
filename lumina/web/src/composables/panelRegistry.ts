// Pure tab registry for the work-item detail lens (Wave-1 framework, T1).
//
// This module is the single source of truth for WHICH tabs a given work-item
// `kind` surfaces and in WHAT order. It is deliberately a pure-data + pure-
// function module (NO `.vue` imports) so it is bun-testable as plain TS —
// `tabsForKind` is exercised directly in src/__tests__/panel-registry.test.ts.
//
// The per-kind matrix (source of truth, from the lens plan):
//   project = overview / repos / activity
//   epic    = overview / activity
//   focus   = overview / activity
//   story   = overview / decisions / quality / activity
//   task    = overview / quality / activity
//
// The `order` values on TAB_DEFS are load-bearing: ascending-order sort over
// the kind-filtered subset is what produces the matrix orderings above.

import type { Kind } from '@/api'

/** The stable identifiers for the five lens tabs. */
export type TabId = 'overview' | 'decisions' | 'quality' | 'activity' | 'repos'

/**
 * Declarative definition of one lens tab: its id, display label, global sort
 * `order`, and the set of work-item `kind`s on which it appears.
 */
export interface TabDef {
  id: TabId
  label: string
  order: number
  kinds: Kind[]
}

/**
 * The canonical tab definitions. ORDER values are load-bearing — they drive
 * the ascending sort in {@link tabsForKind}; do not reorder this array
 * expecting it to change tab order (sort on `order` does that), and do not
 * mutate it at runtime (treat it as frozen — {@link tabsForKind} copies before
 * sorting so callers never see a mutated source).
 */
export const TAB_DEFS: TabDef[] = [
  { id: 'overview', label: 'Overview', order: 0, kinds: ['project', 'epic', 'focus', 'story', 'task'] },
  { id: 'decisions', label: 'Decisions', order: 1, kinds: ['story'] },
  { id: 'quality', label: 'Quality', order: 2, kinds: ['story', 'task'] },
  { id: 'repos', label: 'Repos', order: 3, kinds: ['project'] },
  { id: 'activity', label: 'Activity', order: 4, kinds: ['project', 'epic', 'focus', 'story', 'task'] },
]

/**
 * Returns a FRESH array of the {@link TAB_DEFS} whose `kinds` include `kind`,
 * sorted ascending by `order`. Does NOT mutate {@link TAB_DEFS}: the copy is
 * made via spread before the in-place `.sort`, so callers may freely mutate the
 * returned array without affecting the canonical defs or a subsequent call.
 */
export function tabsForKind(kind: Kind): TabDef[] {
  return [...TAB_DEFS].filter((tab) => tab.kinds.includes(kind)).sort((a, b) => a.order - b.order)
}
