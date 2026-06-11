// Live sprint-quiescence telemetry — the Wave-1 "new resource" recipe.
//
// T9 of the read-only sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md, Wave 1). A thin (~10-line) wrapper
// demonstrating how a new resource rides the multiplexed `/api/stream`
// foundation: own the wire type + topic key in an `api/` module
// (`../api/execution`), then delegate to `useResourceStream<T>` with a getter
// that maps the domain id to the canonical topic (null id = no topic = idle).
//
// Each invocation holds its OWN `quiescence`/`status`/`error` refs (the
// narrow-singleton convention — see useResourceStream's header note), so N
// sprint cards on N sprints never clobber one shared snapshot. Snapshots
// arrive pre-shaped from the server (`frame.data as T` in useResourceStream);
// no extra zod pass here — `SprintQuiescenceSchema` stays available for
// callers that want hardening at another boundary.

import { toValue, type MaybeRefOrGetter, type Ref } from 'vue'

import { sprintQuiescenceTopic, type SprintQuiescence } from '../api/execution'
import { useResourceStream, type StreamStatus } from './useResourceStream'

/**
 * Bind one sprint's live quiescence snapshot to reactive refs. `sprintId` may
 * be a plain string, a ref, or a getter; `null` (or `''`) means "no sprint" —
 * the stream stays/returns to `idle`. A `sprintId` change while connected
 * re-subscribes to the new sprint's topic and clears the stale snapshot.
 */
export function useSprintTelemetry(sprintId: MaybeRefOrGetter<string | null>): {
  quiescence: Ref<SprintQuiescence | null>
  status: Ref<StreamStatus>
  error: Ref<string | null>
  connect: () => void
  disconnect: () => void
} {
  const { data, status, error, connect, disconnect } = useResourceStream<SprintQuiescence>(() => {
    const id = toValue(sprintId)
    return id ? sprintQuiescenceTopic(id) : null
  })
  // `quiescence` IS the stream's `data` ref (renamed, same ref object).
  return { quiescence: data, status, error, connect, disconnect }
}
