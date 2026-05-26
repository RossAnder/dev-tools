// Source of truth for backend Status values: `lumina/src/domain.rs` Status enum.
// Any backend Status addition MUST be mirrored here AND in STATUS_CLASS below.
export const STATUSES = [
  { backend: 'todo', label: 'QUEUED', tokenName: 'queued' },
  { backend: 'in_progress', label: 'IN-FLIGHT', tokenName: 'in-flight' },
  { backend: 'blocked', label: 'BLOCKED', tokenName: 'blocked' },
  { backend: 'done', label: 'DONE', tokenName: 'done' },
  { backend: 'cancelled', label: 'CANCELLED', tokenName: null },
] as const

export type StatusBackend = typeof STATUSES[number]['backend']
export type StatusFilter = StatusBackend | 'ALL'

export function asStatus(s: string): StatusBackend | null {
  return STATUSES.some((entry) => entry.backend === s) ? (s as StatusBackend) : null
}

// Literal class map keyed by backend status. Tailwind 4 only generates classes
// it can see as literal strings in scanned source, so we list each one verbatim
// here rather than interpolating `text-${tokenName}` (R1).
const STATUS_CLASS: Record<StatusBackend, string> = {
  todo: 'text-queued',
  in_progress: 'text-in-flight',
  blocked: 'text-blocked',
  done: 'text-done',
  cancelled: 'text-[var(--muted)] line-through',
}

export function statusLabel(status: StatusBackend | string): string {
  const entry = STATUSES.find((s) => s.backend === status)
  return entry ? entry.label : status.toUpperCase()
}

export function statusToken(status: StatusBackend | string): string {
  return (STATUS_CLASS as Record<string, string>)[status] ?? 'text-[var(--muted)]'
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
