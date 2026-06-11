// Generic per-topic composable over the Wave-1 multiplexed `/api/stream`
// resource stream (`../api/stream.ts`).
//
// T8 of the read-only sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md, Wave 1).
//
// Singleton scope is deliberately NARROW here: only the underlying
// `openResourceStream()` socket is module-singleton (one WS per tab, created
// lazily). EACH `useResourceStream<T>(topic)` invocation returns its OWN
// `data`/`status`/`error` refs bound to ITS topic — N cards on N topics never
// clobber one shared `data`. Per-topic wire dedup (one server subscription per
// topic regardless of consumer count) lives INSIDE stream.ts; this layer just
// shares the one stream instance.
//
// Socket up/down reflection: stream.ts's `onStatus` accepts multiple
// callbacks but has NO deregistration, so per-instance registration would
// leak callbacks against disposed consumers. Instead ONE forwarding callback
// is registered per shared-stream instance, fanning out to a module-level
// Set that connect/disconnect add to / remove from.
//
// Test seam mirrors usePtySessions: `__setApiForTests` / `__resetForTests`.

import {
  getCurrentScope,
  onScopeDispose,
  ref,
  toValue,
  watch,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'

import { openResourceStream, type OutboundFrame, type ResourceStream } from '../api/stream'

/** The shape the injectable factory must produce — exactly `openResourceStream`'s. */
export type ResourceStreamLike = ResourceStream

/**
 * Per-consumer stream status.
 * - `idle`        — not connected (never connected, disconnected, or null topic).
 * - `connecting`  — subscribed, awaiting the first snapshot (also: socket
 *                   bounced; a fresh `init` will flip back to `open`).
 * - `open`        — a snapshot has been received and is current.
 * - `error`       — the server sent an `error` frame for this topic.
 */
export type StreamStatus = 'idle' | 'connecting' | 'open' | 'error'

// ---------------------------------------------------------------------------
// Module-singleton socket (lazily created via an injectable factory).
// ---------------------------------------------------------------------------

let apiFactory: () => ResourceStreamLike = openResourceStream
let sharedStream: ResourceStreamLike | null = null

/**
 * Per-instance socket up/down listeners. stream.ts's `onStatus` cannot
 * deregister, so the shared stream gets ONE forwarding callback (registered
 * at creation) and instances join/leave this set instead.
 */
const statusListeners = new Set<(up: boolean) => void>()

function getSharedStream(): ResourceStreamLike {
  if (sharedStream === null) {
    sharedStream = apiFactory()
    sharedStream.onStatus((up) => {
      // Snapshot so a listener removed mid-dispatch cannot perturb iteration.
      for (const listener of [...statusListeners]) listener(up)
    })
  }
  return sharedStream
}

/** Replace the stream factory. Test-only — do NOT call from production code. */
export function __setApiForTests(factory: () => ResourceStreamLike): void {
  apiFactory = factory
  sharedStream = null
  statusListeners.clear()
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  apiFactory = openResourceStream
  sharedStream?.close()
  sharedStream = null
  statusListeners.clear()
}

// ---------------------------------------------------------------------------
// Composable.
// ---------------------------------------------------------------------------

/**
 * Bind ONE topic of the multiplexed resource stream to a fresh set of
 * reactive refs. `topic` may be a plain string, a ref, or a getter; `null`
 * means "no topic" (stay/return to `idle`). While connected, a topic change
 * unsubscribes the old topic, clears stale `data`/`error`, and subscribes the
 * new one. Auto-disconnects when the calling effect scope is disposed.
 */
export function useResourceStream<T>(topic: MaybeRefOrGetter<string | null>): {
  data: Ref<T | null>
  status: Ref<StreamStatus>
  error: Ref<string | null>
  connect: () => void
  disconnect: () => void
} {
  const data = ref(null) as Ref<T | null>
  const status: Ref<StreamStatus> = ref('idle')
  const error: Ref<string | null> = ref(null)

  /** Set while connected to the intent of streaming (`connect` called, no `disconnect`). */
  let wanted = false
  /** The currently-subscribed topic, when any. */
  let currentTopic: string | null = null
  /** Unsubscribe fn returned by the shared stream's `subscribe`, when subscribed. */
  let unsubscribe: (() => void) | null = null

  function onFrame(frame: OutboundFrame): void {
    switch (frame.type) {
      case 'init':
      case 'data':
        // Full snapshot (never a delta) — replace wholesale.
        data.value = frame.data as T
        error.value = null
        status.value = 'open'
        break
      case 'error':
        error.value = frame.message
        status.value = 'error'
        break
      case 'skipped':
        // Bus lag — keep the (possibly stale) snapshot; the server re-pushes a
        // fresh one. Signal staleness via the transient `connecting` status.
        if (status.value === 'open') status.value = 'connecting'
        break
      case 'pong':
        // Never routed to a topic handler by stream.ts; exhaustiveness only.
        break
    }
  }

  function onSocketStatus(up: boolean): void {
    // Down: the snapshot is stale and the socket is backing off — reflect as
    // `connecting`. Up: stream.ts has just re-subscribed every live topic, so
    // a fresh `init` is inbound — also `connecting` until it lands and flips
    // us back to `open`. (Only registered while subscribed, so never `idle`.)
    void up
    status.value = 'connecting'
  }

  function teardownSubscription(): void {
    if (unsubscribe !== null) {
      unsubscribe()
      unsubscribe = null
    }
    currentTopic = null
    statusListeners.delete(onSocketStatus)
  }

  function applyTopic(resolved: string | null): void {
    if (resolved !== null && resolved === currentTopic && unsubscribe !== null) {
      // Idempotent re-connect to the same topic — avoid wire unsubscribe/subscribe churn.
      return
    }
    teardownSubscription()
    if (resolved === null) {
      status.value = 'idle'
      return
    }
    status.value = 'connecting'
    currentTopic = resolved
    unsubscribe = getSharedStream().subscribe(resolved, onFrame)
    statusListeners.add(onSocketStatus)
  }

  function connect(): void {
    wanted = true
    applyTopic(toValue(topic))
  }

  function disconnect(): void {
    wanted = false
    teardownSubscription()
    status.value = 'idle'
  }

  // Track topic changes while connected: unsubscribe the old topic, reset the
  // per-topic state, subscribe the new (or fall back to idle on null).
  // `flush: 'sync'` keeps the unsubscribe/resubscribe atomic with the topic
  // write — no window in which a frame for the OLD topic lands after the
  // consumer already points at the new one (and it keeps tests deterministic).
  watch(
    () => toValue(topic),
    (next) => {
      if (!wanted) return
      data.value = null
      error.value = null
      applyTopic(next)
    },
    { flush: 'sync' },
  )

  if (getCurrentScope()) onScopeDispose(disconnect)

  return { data, status, error, connect, disconnect }
}
