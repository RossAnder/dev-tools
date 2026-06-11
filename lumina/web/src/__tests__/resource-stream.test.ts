// Bun tests for the `useResourceStream<T>` composable
// (src/composables/useResourceStream.ts) — T8 of the read-only
// sprint/worktree visibility slice (docs/plans/vectorized-brewing-boole.md).
//
// The seam is the composable's injectable stream factory
// (`__setApiForTests`): a controllable fake `openResourceStream` captures
// subscribed topics + their onFrame callbacks so tests push frames directly.
// No SFC rendering — reactive state via `ref`/`effectScope` from 'vue', per
// the repo's bun-test scope (web/CLAUDE.md).

import { afterEach, describe, expect, test } from 'bun:test'
import { effectScope, ref } from 'vue'

import type { OutboundFrame } from '../api/stream'
import {
  __resetForTests,
  __setApiForTests,
  useResourceStream,
  type ResourceStreamLike,
} from '../composables/useResourceStream'

// ---------------------------------------------------------------------------
// Controllable fake resource stream.
// ---------------------------------------------------------------------------

interface SubEntry {
  topic: string
  onFrame: (frame: OutboundFrame) => void
  active: boolean
}

function makeFakeStream() {
  /** Every `subscribe` call in order; `active` flips false on unsubscribe. */
  const subs: SubEntry[] = []
  const statusCbs: Array<(up: boolean) => void> = []
  let factoryCalls = 0
  let closed = false

  const stream: ResourceStreamLike = {
    subscribe(topic, onFrame) {
      const entry: SubEntry = { topic, onFrame, active: true }
      subs.push(entry)
      return () => {
        entry.active = false
      }
    },
    onStatus(cb) {
      statusCbs.push(cb)
    },
    close() {
      closed = true
    },
  }

  return {
    factory: (): ResourceStreamLike => {
      factoryCalls += 1
      return stream
    },
    subs,
    statusCbs,
    get factoryCalls() {
      return factoryCalls
    },
    get closed() {
      return closed
    },
    /** Deliver a frame to every ACTIVE handler on `topic` (mirrors stream.ts routing). */
    push(topic: string, frame: OutboundFrame): void {
      for (const entry of [...subs]) {
        if (entry.active && entry.topic === topic) entry.onFrame(frame)
      }
    },
    /** Fire the socket up/down callback(s) the composable registered. */
    fireStatus(up: boolean): void {
      for (const cb of [...statusCbs]) cb(up)
    },
  }
}

afterEach(() => {
  __resetForTests()
})

// ---------------------------------------------------------------------------
// 1. init + data frames update `data`; status transitions; disconnect on dispose.
// ---------------------------------------------------------------------------

describe('useResourceStream frames', () => {
  test('init then data update data.value and status becomes open; scope dispose disconnects', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const scope = effectScope()
    const rs = scope.run(() => useResourceStream<{ n: number }>('t:1'))!

    // Before connect: idle, no subscription, no socket constructed.
    expect(rs.status.value).toBe('idle')
    expect(rs.data.value).toBeNull()
    expect(fake.factoryCalls).toBe(0)

    rs.connect()
    expect(rs.status.value).toBe('connecting')
    expect(fake.subs).toHaveLength(1)
    expect(fake.subs[0]!.topic).toBe('t:1')

    fake.push('t:1', { type: 'init', topic: 't:1', data: { n: 0 } })
    expect(rs.data.value).toEqual({ n: 0 })
    expect(rs.status.value).toBe('open')
    expect(rs.error.value).toBeNull()

    fake.push('t:1', { type: 'data', topic: 't:1', data: { n: 1 } })
    expect(rs.data.value).toEqual({ n: 1 })
    expect(rs.status.value).toBe('open')

    // Scope disposal auto-disconnects: unsubscribed + back to idle.
    scope.stop()
    expect(fake.subs[0]!.active).toBe(false)
    expect(rs.status.value).toBe('idle')
  })

  test('error frame sets error + status error; skipped keeps data; socket down -> connecting', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const scope = effectScope()
    const rs = scope.run(() => useResourceStream<{ n: number }>('t:1'))!
    rs.connect()

    fake.push('t:1', { type: 'init', topic: 't:1', data: { n: 7 } })
    expect(rs.status.value).toBe('open')

    // skipped: snapshot retained (server re-pushes), transient connecting.
    fake.push('t:1', { type: 'skipped', topic: 't:1' })
    expect(rs.data.value).toEqual({ n: 7 })
    expect(rs.status.value).toBe('connecting')

    fake.push('t:1', { type: 'data', topic: 't:1', data: { n: 8 } })
    expect(rs.status.value).toBe('open')

    // Socket bounce: down then up both reflect as connecting until a fresh init.
    fake.fireStatus(false)
    expect(rs.status.value).toBe('connecting')
    fake.fireStatus(true)
    expect(rs.status.value).toBe('connecting')
    fake.push('t:1', { type: 'init', topic: 't:1', data: { n: 9 } })
    expect(rs.status.value).toBe('open')

    // error frame for our topic.
    fake.push('t:1', { type: 'error', topic: 't:1', message: 'boom' })
    expect(rs.error.value).toBe('boom')
    expect(rs.status.value).toBe('error')
    // data is left as-is on error.
    expect(rs.data.value).toEqual({ n: 9 })

    scope.stop()
  })
})

// ---------------------------------------------------------------------------
// 2. topic-ref change: unsubscribe old, subscribe new, clear stale data.
// ---------------------------------------------------------------------------

describe('useResourceStream topic change', () => {
  test('changing the topic ref unsubscribes the old topic, subscribes the new, clears stale data', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const topicRef = ref<string | null>('a:1')
    const scope = effectScope()
    const rs = scope.run(() => useResourceStream<{ v: string }>(topicRef))!
    rs.connect()

    fake.push('a:1', { type: 'init', topic: 'a:1', data: { v: 'old' } })
    expect(rs.data.value).toEqual({ v: 'old' })

    topicRef.value = 'b:2'
    // Old subscription torn down, stale data/error cleared, new topic live.
    expect(fake.subs[0]!.active).toBe(false)
    expect(rs.data.value).toBeNull()
    expect(rs.status.value).toBe('connecting')
    expect(fake.subs).toHaveLength(2)
    expect(fake.subs[1]!.topic).toBe('b:2')
    expect(fake.subs[1]!.active).toBe(true)

    // A frame for the OLD topic no longer lands; the NEW topic's does.
    fake.push('a:1', { type: 'data', topic: 'a:1', data: { v: 'stale' } })
    expect(rs.data.value).toBeNull()
    fake.push('b:2', { type: 'init', topic: 'b:2', data: { v: 'new' } })
    expect(rs.data.value).toEqual({ v: 'new' })
    expect(rs.status.value).toBe('open')

    // null topic -> unsubscribe + idle.
    topicRef.value = null
    expect(fake.subs[1]!.active).toBe(false)
    expect(rs.status.value).toBe('idle')
    expect(rs.data.value).toBeNull()

    // Back to a topic while still wanted -> resubscribes.
    topicRef.value = 'a:1'
    expect(fake.subs).toHaveLength(3)
    expect(fake.subs[2]!.topic).toBe('a:1')

    scope.stop()
  })

  test('a topic change BEFORE connect() does not subscribe (no implicit connect)', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const topicRef = ref<string | null>('a:1')
    const scope = effectScope()
    const rs = scope.run(() => useResourceStream(topicRef))!

    topicRef.value = 'b:2'
    expect(fake.subs).toHaveLength(0)
    expect(rs.status.value).toBe('idle')

    scope.stop()
  })
})

// ---------------------------------------------------------------------------
// 3. Two consumers, SAME topic: one shared stream instance, both receive frames.
// ---------------------------------------------------------------------------

describe('useResourceStream shared socket', () => {
  test('two consumers on the same topic share ONE stream instance and both receive the push', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const scope = effectScope()
    const a = scope.run(() => useResourceStream<{ n: number }>('t:1'))!
    const b = scope.run(() => useResourceStream<{ n: number }>('t:1'))!
    a.connect()
    b.connect()

    // ONE underlying stream (the wire-subscribe dedup for a repeated topic
    // lives inside stream.ts — injecting at the openResourceStream boundary,
    // the invariant visible HERE is a single shared instance + per-consumer
    // handlers registered against it).
    expect(fake.factoryCalls).toBe(1)
    expect(fake.subs).toHaveLength(2)
    expect(fake.subs.map((s) => s.topic)).toEqual(['t:1', 't:1'])

    // One push reaches BOTH consumers' independent data refs.
    fake.push('t:1', { type: 'data', topic: 't:1', data: { n: 5 } })
    expect(a.data.value).toEqual({ n: 5 })
    expect(b.data.value).toEqual({ n: 5 })

    // Only ONE forwarding onStatus callback was registered on the stream,
    // however many consumers connect (no per-instance leak).
    expect(fake.statusCbs).toHaveLength(1)

    scope.stop()
  })

  test('two consumers on DIFFERENT topics hold independent data', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const scope = effectScope()
    const a = scope.run(() => useResourceStream<{ n: number }>('a:1'))!
    const b = scope.run(() => useResourceStream<{ n: number }>('b:2'))!
    a.connect()
    b.connect()

    expect(fake.factoryCalls).toBe(1) // still one shared socket

    fake.push('a:1', { type: 'data', topic: 'a:1', data: { n: 1 } })
    expect(a.data.value).toEqual({ n: 1 })
    expect(b.data.value).toBeNull()
    expect(b.status.value).toBe('connecting')

    fake.push('b:2', { type: 'data', topic: 'b:2', data: { n: 2 } })
    expect(a.data.value).toEqual({ n: 1 }) // unchanged by B's push
    expect(b.data.value).toEqual({ n: 2 })

    scope.stop()
  })
})

// ---------------------------------------------------------------------------
// 4. disconnect + __resetForTests.
// ---------------------------------------------------------------------------

describe('useResourceStream lifecycle', () => {
  test('disconnect unsubscribes and returns to idle; reconnect resubscribes', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const scope = effectScope()
    const rs = scope.run(() => useResourceStream<{ n: number }>('t:1'))!
    rs.connect()
    fake.push('t:1', { type: 'init', topic: 't:1', data: { n: 1 } })
    expect(rs.status.value).toBe('open')

    rs.disconnect()
    expect(fake.subs[0]!.active).toBe(false)
    expect(rs.status.value).toBe('idle')

    rs.connect()
    expect(fake.subs).toHaveLength(2)
    expect(fake.subs[1]!.active).toBe(true)
    expect(rs.status.value).toBe('connecting')

    scope.stop()
  })

  test('__resetForTests closes the shared stream and clears module state', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const scope = effectScope()
    const rs = scope.run(() => useResourceStream<{ n: number }>('t:1'))!
    rs.connect()
    fake.push('t:1', { type: 'init', topic: 't:1', data: { n: 1 } })
    expect(rs.data.value).toEqual({ n: 1 })
    scope.stop()

    __resetForTests()
    expect(fake.closed).toBe(true)

    // A fresh consumer after reset starts idle with null data — and constructs
    // NOTHING until connect() is called (so the restored real factory is never
    // exercised here).
    const scope2 = effectScope()
    const fresh = scope2.run(() => useResourceStream<{ n: number }>('t:9'))!
    expect(fresh.status.value).toBe('idle')
    expect(fresh.data.value).toBeNull()
    scope2.stop()
  })
})
