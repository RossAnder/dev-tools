// Bun tests for the Wave-1 client stream foundation — `openReconnectingSocket`
// (api/ws-core.ts) + `openResourceStream` (api/stream.ts).
//
// T7 of the read-only sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md). The two modules are pure TS over
// the global `WebSocket`, so the seam here is `globalThis.WebSocket` itself:
// a controllable fake captures sent frames and exposes fire-open /
// fire-message / fire-close hooks. `globalThis.setTimeout` is stubbed to a
// capture queue so the reconnect back-off timer is driven deterministically
// (no real waits, no fake-timer dependency).

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'

import type { OutboundFrame } from '../api/stream'
import { openResourceStream } from '../api/stream'
import { openReconnectingSocket } from '../api/ws-core'

// ---------------------------------------------------------------------------
// Controllable WebSocket fake.
// ---------------------------------------------------------------------------

class FakeWebSocket {
  static CONNECTING = 0
  static OPEN = 1
  static CLOSING = 2
  static CLOSED = 3

  /** Every instance constructed since the last reset, in order. */
  static instances: FakeWebSocket[] = []

  url: string
  readyState = FakeWebSocket.CONNECTING
  /** Raw strings passed to `send` (only reachable while readyState === OPEN). */
  sent: string[] = []
  /** Codes passed to caller-side `close()`. */
  closeCalls: Array<number | undefined> = []

  private listeners = new Map<string, Array<(event: unknown) => void>>()

  constructor(url: string) {
    this.url = url
    FakeWebSocket.instances.push(this)
  }

  addEventListener(event: string, fn: (event: unknown) => void): void {
    const list = this.listeners.get(event)
    if (list) list.push(fn)
    else this.listeners.set(event, [fn])
  }

  send(data: string): void {
    this.sent.push(data)
  }

  close(code?: number): void {
    this.closeCalls.push(code)
    this.readyState = FakeWebSocket.CLOSED
    // Like the real API, closing does NOT synchronously fire the close event;
    // tests drive it explicitly via fireClose when they need it.
  }

  private fire(event: string, payload: unknown): void {
    const list = this.listeners.get(event)
    if (list) for (const fn of [...list]) fn(payload)
  }

  fireOpen(): void {
    this.readyState = FakeWebSocket.OPEN
    this.fire('open', {})
  }

  fireMessage(data: string): void {
    this.fire('message', { data })
  }

  fireClose(code: number): void {
    this.readyState = FakeWebSocket.CLOSED
    this.fire('close', { code })
  }

  /** Parse the captured sent strings as JSON for frame-level assertions. */
  sentFrames(): unknown[] {
    return this.sent.map((s) => JSON.parse(s) as unknown)
  }
}

// ---------------------------------------------------------------------------
// Global seams: WebSocket + setTimeout capture.
// ---------------------------------------------------------------------------

/** Callbacks captured from the stubbed setTimeout, in scheduling order. */
let scheduled: Array<{ fn: () => void; delay: number }> = []

const realWebSocket = globalThis.WebSocket
const realSetTimeout = globalThis.setTimeout

beforeEach(() => {
  FakeWebSocket.instances = []
  scheduled = []
  globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
  globalThis.setTimeout = ((fn: () => void, delay?: number) => {
    scheduled.push({ fn, delay: delay ?? 0 })
    return 0
  }) as unknown as typeof setTimeout
})

afterEach(() => {
  globalThis.WebSocket = realWebSocket
  globalThis.setTimeout = realSetTimeout
})

function lastInstance(): FakeWebSocket {
  const ws = FakeWebSocket.instances[FakeWebSocket.instances.length - 1]
  if (!ws) throw new Error('no FakeWebSocket constructed')
  return ws
}

// ---------------------------------------------------------------------------
// 1. subscribe sends a subscribe frame (first handler only).
// ---------------------------------------------------------------------------

describe('openResourceStream subscribe', () => {
  test('first handler for a topic sends {type:"subscribe", topic}; second does not re-send', () => {
    const stream = openResourceStream()
    const ws = lastInstance()
    expect(ws.url).toMatch(/\/api\/stream$/)
    ws.fireOpen()
    ws.sent.length = 0 // discard any open-time noise (no live topics yet, so none expected)

    stream.subscribe('sprint-quiescence:s1', () => {})
    expect(ws.sentFrames()).toEqual([{ type: 'subscribe', topic: 'sprint-quiescence:s1' }])

    // A SECOND handler on the same live topic must not duplicate the server sub.
    stream.subscribe('sprint-quiescence:s1', () => {})
    expect(ws.sent).toHaveLength(1)

    stream.close()
    expect(ws.closeCalls).toEqual([1000])
  })
})

// ---------------------------------------------------------------------------
// 2. reconnect re-sends every live subscription.
// ---------------------------------------------------------------------------

describe('openResourceStream reconnect', () => {
  test('after an unexpected close + reconnect open, every live topic is re-subscribed', () => {
    const stream = openResourceStream()
    const statusEvents: boolean[] = []
    stream.onStatus((up) => statusEvents.push(up))

    const first = lastInstance()
    first.fireOpen()
    stream.subscribe('a:1', () => {})
    stream.subscribe('b:2', () => {})
    expect(first.sentFrames()).toEqual([
      { type: 'subscribe', topic: 'a:1' },
      { type: 'subscribe', topic: 'b:2' },
    ])

    // Unexpected close (code !== 1000) — a reconnect is scheduled (1s first
    // attempt) and onStatus(false) fires.
    first.fireClose(1006)
    expect(statusEvents).toEqual([true, false])
    expect(scheduled).toHaveLength(1)
    expect(scheduled[0]?.delay).toBe(1000)

    // Drive the back-off timer: a NEW socket is constructed.
    scheduled[0]!.fn()
    expect(FakeWebSocket.instances).toHaveLength(2)
    const second = lastInstance()

    // On the reconnect open, BOTH live subscriptions are re-sent and
    // onStatus(true) fires.
    second.fireOpen()
    expect(second.sentFrames()).toEqual([
      { type: 'subscribe', topic: 'a:1' },
      { type: 'subscribe', topic: 'b:2' },
    ])
    expect(statusEvents).toEqual([true, false, true])

    stream.close()
  })

  test('back-off doubles per failed attempt and resets on open', () => {
    openReconnectingSocket({ path: '/api/stream', onFrame: () => {} })

    // Three consecutive failures: 1s, 2s, 4s.
    lastInstance().fireClose(1006)
    scheduled.shift()!.fn()
    lastInstance().fireClose(1006)
    scheduled.shift()!.fn()
    lastInstance().fireClose(1006)
    expect(scheduled.map((s) => s.delay)).toEqual([4000])
    scheduled.shift()!.fn()

    // A successful open resets the back-off to 1s.
    lastInstance().fireOpen()
    lastInstance().fireClose(1006)
    expect(scheduled.map((s) => s.delay)).toEqual([1000])
  })
})

// ---------------------------------------------------------------------------
// 3. close() suppresses reconnect.
// ---------------------------------------------------------------------------

describe('openResourceStream close', () => {
  test('close() then a forced close does NOT reconnect', () => {
    const stream = openResourceStream()
    const ws = lastInstance()
    ws.fireOpen()

    stream.close()
    expect(ws.closeCalls).toEqual([1000])

    // Even a non-1000 close event arriving after the user close must not
    // schedule a reconnect (userClosed guard).
    ws.fireClose(1006)
    expect(scheduled).toHaveLength(0)
    expect(FakeWebSocket.instances).toHaveLength(1)
  })
})

// ---------------------------------------------------------------------------
// 4. zod validation: invalid frames dropped, valid frames delivered.
// ---------------------------------------------------------------------------

describe('openResourceStream frame validation + routing', () => {
  test('malformed/invalid frames are dropped; a valid data frame for the topic is delivered', () => {
    const stream = openResourceStream()
    const ws = lastInstance()
    ws.fireOpen()

    const frames: OutboundFrame[] = []
    stream.subscribe('sprint-quiescence:s1', (f) => frames.push(f))

    // Not JSON at all — swallowed by ws-core.
    ws.fireMessage('not json {')
    // Valid JSON, invalid frame shape (unknown type) — dropped by zod.
    ws.fireMessage(JSON.stringify({ type: 'nonsense', topic: 'sprint-quiescence:s1' }))
    // Valid type but missing the REQUIRED topic — dropped by zod.
    ws.fireMessage(JSON.stringify({ type: 'data', data: { claimable: 1 } }))
    expect(frames).toHaveLength(0)

    // A valid data frame for a DIFFERENT topic is not delivered here.
    ws.fireMessage(JSON.stringify({ type: 'data', topic: 'other:9', data: {} }))
    expect(frames).toHaveLength(0)

    // A valid init + data frame for the subscribed topic IS delivered.
    ws.fireMessage(
      JSON.stringify({ type: 'init', topic: 'sprint-quiescence:s1', data: { claimable: 0 } }),
    )
    ws.fireMessage(
      JSON.stringify({ type: 'data', topic: 'sprint-quiescence:s1', data: { claimable: 1 } }),
    )
    expect(frames).toHaveLength(2)
    expect(frames[0]).toEqual({
      type: 'init',
      topic: 'sprint-quiescence:s1',
      data: { claimable: 0 },
    })
    expect(frames[1]).toEqual({
      type: 'data',
      topic: 'sprint-quiescence:s1',
      data: { claimable: 1 },
    })

    stream.close()
  })

  test('skipped WITH a topic routes to that topic; WITHOUT a topic broadcasts to all live topics', () => {
    const stream = openResourceStream()
    const ws = lastInstance()
    ws.fireOpen()

    const aFrames: OutboundFrame[] = []
    const bFrames: OutboundFrame[] = []
    stream.subscribe('a:1', (f) => aFrames.push(f))
    stream.subscribe('b:2', (f) => bFrames.push(f))

    ws.fireMessage(JSON.stringify({ type: 'skipped', topic: 'a:1' }))
    expect(aFrames).toHaveLength(1)
    expect(bFrames).toHaveLength(0)

    ws.fireMessage(JSON.stringify({ type: 'skipped' }))
    expect(aFrames).toHaveLength(2)
    expect(bFrames).toHaveLength(1)

    // pong is ignored entirely.
    ws.fireMessage(JSON.stringify({ type: 'pong' }))
    expect(aFrames).toHaveLength(2)
    expect(bFrames).toHaveLength(1)

    stream.close()
  })
})

// ---------------------------------------------------------------------------
// 5. last unsubscribe sends an unsubscribe frame.
// ---------------------------------------------------------------------------

describe('openResourceStream unsubscribe', () => {
  test('only the LAST unsubscribe for a topic sends {type:"unsubscribe", topic}; fn is idempotent', () => {
    const stream = openResourceStream()
    const ws = lastInstance()
    ws.fireOpen()

    const unsubA = stream.subscribe('t:1', () => {})
    const unsubB = stream.subscribe('t:1', () => {})
    expect(ws.sentFrames()).toEqual([{ type: 'subscribe', topic: 't:1' }])

    // First handler removed — topic still live, NO unsubscribe frame.
    unsubA()
    expect(ws.sent).toHaveLength(1)

    // Last handler removed — unsubscribe sent and the topic key dropped.
    unsubB()
    expect(ws.sentFrames()).toEqual([
      { type: 'subscribe', topic: 't:1' },
      { type: 'unsubscribe', topic: 't:1' },
    ])

    // Idempotent: calling either unsubscribe again sends nothing further.
    unsubA()
    unsubB()
    expect(ws.sent).toHaveLength(2)

    // The topic key was deleted — a fresh subscribe is FIRST again and re-sends.
    const frames: OutboundFrame[] = []
    stream.subscribe('t:1', (f) => frames.push(f))
    expect(ws.sentFrames()).toEqual([
      { type: 'subscribe', topic: 't:1' },
      { type: 'unsubscribe', topic: 't:1' },
      { type: 'subscribe', topic: 't:1' },
    ])

    // And a frame for the re-subscribed topic is delivered again.
    ws.fireMessage(JSON.stringify({ type: 'init', topic: 't:1', data: null }))
    expect(frames).toHaveLength(1)

    stream.close()
  })
})
