// Floating-chat context resolvers — PURE module, no reactive state.
//
// The floating chat popup needs to know two things before it can spawn an
// agent against the right slice of work: WHICH work-item field/collection the
// operator is acting on (the "focal point") and in WHICH working directory the
// agent should run (the "cwd"). Both are derivable from already-resolved
// composable state, so this module exposes two pure functions and a type — no
// module-singleton refs, no api adapter, no `__setApiForTests`. That keeps it
// trivially unit-testable: feed inputs, assert outputs (see floating-chat.test.ts,
// authored in T3).
//
// Grounding:
//   - `useHierarchy.focusPath` is the position→context resolver (a root-first
//     `WorkItem[]` breadcrumb) and `focusedNode` is the focused node — this
//     module folds them into a focal-point snapshot rather than re-deriving
//     ancestry itself.
//   - `ChatContextFocalPoint` EXTENDS `ElementDescriptor` (from EditableElement.vue)
//     rather than paralleling it, so the descriptor a per-element edit affordance
//     already carries flows straight through.
//   - cwd resolves from the project's primary `repo_links.local_path`, falling
//     back to the machine-local `clone_root` (GET /api/settings), and finally to
//     `null` — which the caller renders as the "no clone path recorded" error
//     state.

import type { ElementDescriptor } from '@/components/ui/EditableElement.vue'
import type { WorkItem, WorkItemDetail } from '@/api'

// ---------------------------------------------------------------------------
// Focal-point type.
// ---------------------------------------------------------------------------

/**
 * The work-item slice the floating chat is focused on. EXTENDS
 * {@link ElementDescriptor} (so the `workItemId` / `kind` / `field` / `collection`
 * an editable element already carries pass through) and adds:
 *   - `ancestryPath` — the root-first breadcrumb chain to the focused node,
 *     lifted from `useHierarchy.focusPath`.
 *   - `fieldKey` — the specific scalar field being acted on, when a field
 *     descriptor is present (mirrors `ElementDescriptor.field`, surfaced under a
 *     stable name the chat consumer reads).
 *   - `nestedRowId` — the id of the specific collection row being acted on, when
 *     a row descriptor is present (e.g. one research note inside the
 *     `research_notes` collection); absent for a whole-collection or scalar
 *     descriptor.
 */
export interface ChatContextFocalPoint extends ElementDescriptor {
  ancestryPath: WorkItem[]
  fieldKey?: string
  nestedRowId?: string
}

/**
 * A descriptor passed to {@link resolveFocalPoint}. Widens
 * {@link ElementDescriptor} with an optional `rowId` so a per-row edit affordance
 * (one note / risk / finding inside a collection) can carry which row it edits —
 * `ElementDescriptor` itself only distinguishes a scalar `field` from a whole
 * `collection`, so the row id rides alongside it here.
 */
export interface FieldDescriptor extends ElementDescriptor {
  rowId?: string
}

// ---------------------------------------------------------------------------
// Resolvers (pure).
// ---------------------------------------------------------------------------

/**
 * Build a {@link ChatContextFocalPoint} snapshot from the current hierarchy
 * position. Pure: it reads its inputs and returns a fresh object, touching no
 * module state.
 *
 * @param focusPath       The root-first ancestry breadcrumb (`useHierarchy.focusPath`).
 * @param focusedNode     The focused node (`useHierarchy.focusedNode`); its id and
 *                        kind seed the focal point.
 * @param fieldDescriptor Optional descriptor for the specific field/row the
 *                        operator is acting on. `field` becomes `fieldKey`,
 *                        `collection` passes through, and `rowId` becomes
 *                        `nestedRowId`.
 */
export function resolveFocalPoint(
  focusPath: WorkItem[],
  focusedNode: WorkItem,
  fieldDescriptor?: FieldDescriptor,
): ChatContextFocalPoint {
  return {
    workItemId: focusedNode.id,
    kind: focusedNode.kind,
    // Root-first, exactly as `useHierarchy.focusPath` returns it.
    ancestryPath: focusPath,
    field: fieldDescriptor?.field,
    collection: fieldDescriptor?.collection,
    fieldKey: fieldDescriptor?.field,
    nestedRowId: fieldDescriptor?.rowId,
  }
}

/** The slice of machine-local settings {@link resolveCwd} reads. */
export interface ChatCwdSettings {
  /** Machine-local clone root from `GET /api/settings` (`null` when unset). */
  cloneRoot: string | null
}

/**
 * Resolve the working directory the floating chat's agent should run in.
 *
 * Precedence (first non-null wins):
 *   1. the project's PRIMARY `repo_links.local_path` (the operator's recorded
 *      clone of this project's repo on this machine);
 *   2. the machine-local `clone_root` (`GET /api/settings`);
 *   3. `null` — no clone path is known, which the caller surfaces as the
 *      "no clone path recorded" error state.
 *
 * Pure: reads its inputs, returns a string or null.
 *
 * @param projectDetail The project's `WorkItemDetail` (carrying `repo_links`), or
 *                      `null` when no project is resolved.
 * @param settings      The machine-local settings slice (`cloneRoot`).
 */
export function resolveCwd(
  projectDetail: WorkItemDetail | null,
  settings: ChatCwdSettings,
): string | null {
  const primary = projectDetail?.repo_links.find((link) => link.is_primary === 1)
  return primary?.local_path ?? settings.cloneRoot ?? null
}
