export const STATUSES = [
  { backend: 'todo', label: 'QUEUED', tokenName: 'queued' },
  { backend: 'in_progress', label: 'IN-FLIGHT', tokenName: 'in-flight' },
  { backend: 'blocked', label: 'BLOCKED', tokenName: 'blocked' },
  { backend: 'done', label: 'DONE', tokenName: 'done' },
  { backend: 'cancelled', label: 'CANCELLED', tokenName: null },
] as const

export function statusLabel(status: string): string {
  const entry = STATUSES.find((s) => s.backend === status)
  return entry ? entry.label : status.toUpperCase()
}

export function statusToken(status: string): string {
  const entry = STATUSES.find((s) => s.backend === status)
  if (!entry) return 'text-[var(--muted)]'
  if (entry.tokenName === null) return 'text-[var(--muted)] line-through'
  return `text-${entry.tokenName}`
}

export function effortLabel(effort: string | null | undefined): string | null {
  if (effort === 's') return 'S'
  if (effort === 'm') return 'M'
  if (effort === 'l') return 'L'
  return null
}

export function kindLabel(kind: string): string {
  return kind.toUpperCase()
}
