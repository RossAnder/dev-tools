import type { WorkItem, WorkItemNode } from '@/api'

/**
 * Pure tree-walk helpers extracted from useHierarchy so they can be exercised
 * directly under bun test without spinning up the module-singleton reactive
 * state. Nothing in here imports from `vue` or touches a ref — every function
 * is total and deterministic over its arguments.
 *
 * The composable re-imports these and wraps them in the `computed`/method
 * surface that callers consume; the split keeps the reactive vs. pure surfaces
 * cleanly separated.
 */

/**
 * Flatten the recursive tree into a Map keyed by node id. Used by the
 * focusPath walker to climb the parent_id chain in O(1) per hop without
 * re-descending the tree at every step.
 */
export function indexTree(nodes: WorkItemNode[]): Map<string, WorkItemNode> {
  const out = new Map<string, WorkItemNode>()
  const visit = (node: WorkItemNode): void => {
    out.set(node.id, node)
    for (const child of node.children) visit(child)
  }
  for (const root of nodes) visit(root)
  return out
}

/**
 * Walk the parent_id chain from the node identified by `focusId` up to its
 * root, returning the breadcrumb chain root-first. Empty when `focusId` is
 * `null` or the id is not present in the provided index.
 *
 * Takes a pre-built `byId` map (the caller is expected to memoise it — in the
 * composable this is the `byId` computed) so the walker does not pay the cost
 * of re-indexing the tree on every access.
 */
export function focusPathFrom(
  byId: Map<string, WorkItemNode>,
  focusId: string | null,
): WorkItem[] {
  if (focusId === null) return []
  const chain: WorkItem[] = []
  let cursor: WorkItemNode | undefined = byId.get(focusId)
  while (cursor !== undefined) {
    chain.push(cursor)
    if (cursor.parent_id === null) break
    cursor = byId.get(cursor.parent_id)
  }
  chain.reverse()
  return chain
}

/** Effort weights for the rollup `size` field; unknown values contribute 0. */
export function effortWeight(value: string | null | undefined): number {
  switch (value) {
    case 's':
      return 2
    case 'm':
      return 5
    case 'l':
      return 8
    default:
      return 0
  }
}

export interface DescendantCounts {
  features: number
  stories: number
  tasks: number
  doneTasks: number
  totalTasks: number
  size: number
}

/**
 * Walk a node's `children` recursively and accumulate kind/status/effort
 * counts. The starting node itself is NOT included — callers pass the focused
 * node's children (or the tree roots for a portfolio-wide rollup).
 */
export function collectCounts(children: WorkItemNode[]): DescendantCounts {
  const counts: DescendantCounts = {
    features: 0,
    stories: 0,
    tasks: 0,
    doneTasks: 0,
    totalTasks: 0,
    size: 0,
  }
  const visit = (node: WorkItemNode): void => {
    if (node.kind === 'feature') counts.features += 1
    else if (node.kind === 'story') counts.stories += 1
    else if (node.kind === 'task') {
      counts.tasks += 1
      counts.totalTasks += 1
      if (node.status === 'done') counts.doneTasks += 1
      counts.size += effortWeight(node.effort)
    }
    for (const child of node.children) visit(child)
  }
  for (const child of children) visit(child)
  return counts
}

/**
 * Sum the total descendant count under `children` (each child contributes 1
 * for itself plus the size of its own subtree).
 */
export function countSubtree(children: WorkItemNode[]): number {
  let total = 0
  for (const child of children) {
    total += 1 + countSubtree(child.children)
  }
  return total
}

/**
 * Count every node reachable from `roots` (roots + descendants) matching
 * `predicate`. Pure variant of the composable's `countMatching`; the
 * composable supplies `tree.value` as `roots`.
 */
export function countMatchingIn(
  roots: WorkItemNode[],
  predicate: (node: WorkItemNode) => boolean,
): number {
  let count = 0
  const visit = (node: WorkItemNode): void => {
    if (predicate(node)) count += 1
    for (const child of node.children) visit(child)
  }
  for (const root of roots) visit(root)
  return count
}
