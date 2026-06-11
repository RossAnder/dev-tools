// Multiplexed `/api/stream` resource-stream opener.
//
// T7 of the read-only sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md, Wave 1). One underlying
// reconnecting WebSocket (via `api/ws-core.ts`) carries every topic
// subscription for the tab, multiplexed over the Wave-1 NORMATIVE frame
// contract (plan § Approach — the server lanes T4/T5/T6 implement the same
// shape; any divergence breaks interop):
//
//   inbound  (client -> server): {type:"subscribe", topic} ·
//                                {type:"unsubscribe", topic} · {type:"ping"}
//   outbound (server -> client): {type:"init", topic, data} ·
//                                {type:"data", topic, data} ·
//                                {type:"skipped", topic?} ·
//                                {type:"error", topic, message} · {type:"pong"}
//
// Snapshots, never deltas: `data` is an arbitrary full-snapshot JSON value
// (`z.unknown()` — consumers validate their own payloads), so a missed frame
// self-heals on the next push and reconnect is race-free (init-on-subscribe
// server-side + resubscribe-all-on-reconnect here).
//
// INTERNAL module — deliberately NOT re-exported from the `api/index.ts`
// barrel. Composables (`useResourceStream`, T8) import it directly by path.

import * as z from 'zod'

import { openReconnectingSocket } from './ws-core'

// ---------------------------------------------------------------------------
// Outbound (server -> client) frame schemas
// ---------------------------------------------------------------------------

/**
 * Discriminated union over the server's outbound frames, mirroring the Rust
 * enum with `#[serde(tag = "type", rename_all = "snake_case")]` (T4).
 *
 * `topic` is REQUIRED on `init`/`data`/`error` — the client routes by it.
 * `skipped` (bus lag) may carry a topic (re-init just that topic) or none
 * (re-init every live topic). `data` payloads are deliberately `z.unknown()`:
 * this layer never constrains the snapshot shape.
 */
export const OutboundFrameSchema = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('init'),
    topic: z.string(),
    data: z.unknown(),
  }),
  z.object({
    type: z.literal('data'),
    topic: z.string(),
    data: z.unknown(),
  }),
  z.object({
    type: z.literal('skipped'),
    topic: z.string().optional(),
  }),
  z.object({
    type: z.literal('error'),
    topic: z.string(),
    message: z.string(),
  }),
  z.object({
    type: z.literal('pong'),
  }),
])
export type OutboundFrame = z.infer<typeof OutboundFrameSchema>

// ---------------------------------------------------------------------------
// Resource stream
// ---------------------------------------------------------------------------

/** Handle returned by {@link openResourceStream}. */
export interface ResourceStream {
  /**
   * Register a handler for one topic. The FIRST handler for a topic sends
   * `{type:"subscribe", topic}`; the returned unsubscribe fn removes the
   * handler, and removing the LAST handler for a topic sends
   * `{type:"unsubscribe", topic}`. The unsubscribe fn is idempotent.
   */
  subscribe: (topic: string, onFrame: (frame: OutboundFrame) => void) => () => void
  /** Register an up/down callback: `true` on (re)open, `false` on unexpected close. */
  onStatus: (cb: (up: boolean) => void) => void
  /** Close the underlying socket (code 1000); no reconnect afterwards. */
  close: () => void
}

/**
 * Open ONE multiplexed resource stream over `/api/stream`.
 *
 * Topic routing: `init`/`data`/`error` frames go to that topic's handlers.
 * A `skipped` frame WITH a topic goes to that topic's handlers; WITHOUT a
 * topic it is delivered to EVERY live topic's handlers (so each consumer can
 * treat its snapshot as stale and re-init). `pong` is ignored. Frames that
 * fail zod validation are dropped silently.
 *
 * Reconnect safety: on every `open` (initial AND after a reconnect) every
 * currently-live topic's `{type:"subscribe", topic}` is (re)sent — combined
 * with the server's init-on-subscribe, a reconnect always converges on a
 * fresh full snapshot per topic.
 */
export function openResourceStream(): ResourceStream {
  /** topic -> handlers, in registration order. A topic is "live" while it has >=1 handler. */
  const handlers = new Map<string, Array<(frame: OutboundFrame) => void>>()
  const statusCbs: Array<(up: boolean) => void> = []

  function deliver(list: Array<(frame: OutboundFrame) => void>, frame: OutboundFrame): void {
    // Snapshot the list so a handler that unsubscribes mid-dispatch cannot
    // perturb iteration.
    for (const fn of [...list]) fn(frame)
  }

  function handleRaw(data: unknown): void {
    const parsed = OutboundFrameSchema.safeParse(data)
    if (!parsed.success) return
    const frame = parsed.data
    switch (frame.type) {
      case 'init':
      case 'data':
      case 'error': {
        const list = handlers.get(frame.topic)
        if (list) deliver(list, frame)
        break
      }
      case 'skipped': {
        if (frame.topic !== undefined) {
          const list = handlers.get(frame.topic)
          if (list) deliver(list, frame)
        } else {
          // Topic-less lag signal — every live topic may have missed a push.
          for (const list of handlers.values()) deliver(list, frame)
        }
        break
      }
      case 'pong':
        break
    }
  }

  const socket = openReconnectingSocket({
    path: '/api/stream',
    onFrame: handleRaw,
    onOpen() {
      // (Re)subscribe every live topic. On the INITIAL open this also flushes
      // subscriptions registered while the socket was still CONNECTING (their
      // eager send below was dropped by the readyState guard).
      for (const topic of handlers.keys()) {
        socket.send({ type: 'subscribe', topic })
      }
      for (const cb of statusCbs) cb(true)
    },
    onDown() {
      for (const cb of statusCbs) cb(false)
    },
  })

  return {
    subscribe(topic: string, onFrame: (frame: OutboundFrame) => void): () => void {
      let list = handlers.get(topic)
      if (list) {
        list.push(onFrame)
      } else {
        list = [onFrame]
        handlers.set(topic, list)
        // First handler for this topic — tell the server.
        socket.send({ type: 'subscribe', topic })
      }

      let active = true
      return () => {
        if (!active) return
        active = false
        const current = handlers.get(topic)
        if (!current) return
        const idx = current.indexOf(onFrame)
        if (idx !== -1) current.splice(idx, 1)
        if (current.length === 0) {
          // Last handler gone — drop the live topic and tell the server.
          handlers.delete(topic)
          socket.send({ type: 'unsubscribe', topic })
        }
      }
    },

    onStatus(cb: (up: boolean) => void): void {
      statusCbs.push(cb)
    },

    close(): void {
      socket.close()
    },
  }
}
