import type { WorkItem } from '@/api'

/**
 * Pure sprint-membership filter extracted from ChildGrid so it can be
 * exercised directly under bun test (bun cannot import `.vue` SFCs). Nothing
 * in here imports from `vue` or touches a ref — the function is total and
 * deterministic over its arguments.
 *
 * Wave 4 T24 of the sprint/worktree visibility slice
 * (docs/plans/vectorized-brewing-boole.md). Work items carry NO `sprint_id`
 * column — sprint membership is the `sprint_tasks` junction, surfaced to the
 * SPA as `SprintDetail.member_task_ids` (the Wave-2b `useSprints` selection
 * seam) — so this id-set cross-filter is how the central grid scopes to the
 * selected sprint.
 *
 * Semantics (the component passes `on = sprintFilterOn && a sprint is
 * selected`, and `selectedDetail.value?.member_task_ids ?? null`):
 *
 * - `on === false` → passthrough: `children` returned unchanged (same array
 *   reference), so the status filter's behaviour is untouched while the
 *   sprint filter is off.
 * - `on === true`, `memberTaskIds === null` (the selected sprint's detail has
 *   not loaded yet, or its fetch failed) → EMPTY: membership is unproven, and
 *   with the filter explicitly on, presenting unfiltered children as sprint
 *   members would be a lie. The blank is transient — the grid fills as soon
 *   as the detail lands.
 * - `on === true`, `memberTaskIds === []` → EMPTY: the honest "this sprint
 *   has no members in view" result.
 * - otherwise → only the children whose `id` is in the member set, original
 *   order preserved.
 */
export function applySprintFilter(
  children: WorkItem[],
  memberTaskIds: string[] | null,
  on: boolean,
): WorkItem[] {
  if (!on) return children
  if (memberTaskIds === null || memberTaskIds.length === 0) return []
  const members = new Set(memberTaskIds)
  return children.filter((c) => members.has(c.id))
}
