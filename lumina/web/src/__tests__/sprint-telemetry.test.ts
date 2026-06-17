// Bun tests for the `useSprintTelemetry` wrapper
// (src/composables/useSprintTelemetry.ts) + the canonical sprint-quiescence
// topic key (src/api/execution.ts) — T9 of the read-only sprint/worktree
// visibility slice (docs/plans/vectorized-brewing-boole.md, Wave 1).
//
// Seam: useResourceStream's injectable stream factory (`__setApiForTests`) —
// a controllable fake `openResourceStream` captures subscribed topics + their
// onFrame callbacks so tests push `{type:'data'|'init', topic, data}` frames
// directly. No SFC rendering — `ref`/`effectScope` from 'vue', per the repo's
// bun-test scope (web/CLAUDE.md).

import { afterEach, describe, expect, test } from 'bun:test'
import { effectScope, ref } from 'vue'

import { sprintQuiescenceTopic, type SprintQuiescence } from '../api/execution'
import type { OutboundFrame } from '../api/stream'
import {
  __resetForTests,
  __setApiForTests,
  type ResourceStreamLike,
} from '../composables/useResourceStream'
import { useSprintTelemetry } from '../composables/useSprintTelemetry'

// ---------------------------------------------------------------------------
// Controllable fake resource stream (mirrors resource-stream.test.ts).
// ---------------------------------------------------------------------------

interface SubEntry {
  topic: string
  onFrame: (frame: OutboundFrame) => void
  active: boolean
}

function makeFakeStream() {
  /** Every `subscribe` call in order; `active` flips false on unsubscribe. */
  const subs: SubEntry[] = []
  let closed = false

  const stream: ResourceStreamLike = {
    subscribe(topic, onFrame) {
      const entry: SubEntry = { topic, onFrame, active: true }
      subs.push(entry)
      return () => {
        entry.active = false
      }
    },
    onStatus() {},
    close() {
      closed = true
    },
  }

  return {
    factory: (): ResourceStreamLike => stream,
    subs,
    get closed() {
      return closed
    },
    /** Deliver a frame to every ACTIVE handler on `topic` (mirrors stream.ts routing). */
    push(topic: string, frame: OutboundFrame): void {
      for (const entry of [...subs]) {
        if (entry.active && entry.topic === topic) entry.onFrame(frame)
      }
    },
  }
}

/** A full SprintQuiescence snapshot with overridable fields. */
function snapshot(overrides: Partial<SprintQuiescence> = {}): SprintQuiescence {
  return {
    claimable: 0,
    in_progress: 0,
    blocked_on_question: 0,
    in_review: 0,
    terminal: 0,
    done: false,
    stalled: false,
    ...overrides,
  }
}

afterEach(() => {
  __resetForTests()
})

// ---------------------------------------------------------------------------
// 1. connect + snapshot push -> quiescence reflects it; canonical topic key.
// ---------------------------------------------------------------------------

describe('useSprintTelemetry snapshots', () => {
  test('connect subscribes the canonical topic; a pushed snapshot lands in quiescence', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const scope = effectScope()
    const t = scope.run(() => useSprintTelemetry('sp-1'))!

    expect(t.status.value).toBe('idle')
    expect(t.quiescence.value).toBeNull()

    t.connect()
    expect(t.status.value).toBe('connecting')
    expect(fake.subs).toHaveLength(1)
    // The Wave-1 canonical topic form: `sprint-quiescence:<id>`.
    expect(fake.subs[0]!.topic).toBe('sprint-quiescence:sp-1')
    expect(fake.subs[0]!.topic).toBe(sprintQuiescenceTopic('sp-1'))

    const snap = snapshot({ claimable: 3, in_progress: 1 })
    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'data',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snap,
    })
    expect(t.quiescence.value?.claimable).toBe(3)
    expect(t.quiescence.value).toEqual(snap)
    expect(t.status.value).toBe('open')
    expect(t.error.value).toBeNull()

    scope.stop()
  })
})

// ---------------------------------------------------------------------------
// 2. sprintId ref change -> re-subscribe to the new sprint's topic.
// ---------------------------------------------------------------------------

describe('useSprintTelemetry sprint change', () => {
  test('a sprintId change re-subscribes the new topic and clears the stale snapshot', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const sprintId = ref<string | null>('sp-1')
    const scope = effectScope()
    const t = scope.run(() => useSprintTelemetry(sprintId))!
    t.connect()

    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'init',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snapshot({ claimable: 2 }),
    })
    expect(t.quiescence.value?.claimable).toBe(2)

    sprintId.value = 'sp-2'
    // Old topic torn down, stale snapshot cleared, new topic live.
    expect(fake.subs[0]!.active).toBe(false)
    expect(fake.subs).toHaveLength(2)
    expect(fake.subs[1]!.topic).toBe(sprintQuiescenceTopic('sp-2'))
    expect(t.quiescence.value).toBeNull()
    expect(t.status.value).toBe('connecting')

    // A frame for the OLD sprint no longer lands; the NEW sprint's does.
    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'data',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snapshot({ claimable: 9 }),
    })
    expect(t.quiescence.value).toBeNull()
    fake.push(sprintQuiescenceTopic('sp-2'), {
      type: 'init',
      topic: sprintQuiescenceTopic('sp-2'),
      data: snapshot({ terminal: 4, done: true }),
    })
    expect(t.quiescence.value?.done).toBe(true)
    expect(t.quiescence.value?.terminal).toBe(4)
    expect(t.status.value).toBe('open')

    // null sprintId -> no topic -> unsubscribe + idle.
    sprintId.value = null
    expect(fake.subs[1]!.active).toBe(false)
    expect(t.status.value).toBe('idle')
    expect(t.quiescence.value).toBeNull()

    scope.stop()
  })
})

// ---------------------------------------------------------------------------
// 3. __resetForTests clears module state.
// ---------------------------------------------------------------------------

describe('useSprintTelemetry reset', () => {
  test('__resetForTests closes the shared stream; a fresh consumer starts idle', () => {
    const fake = makeFakeStream()
    __setApiForTests(fake.factory)

    const scope = effectScope()
    const t = scope.run(() => useSprintTelemetry('sp-1'))!
    t.connect()
    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'init',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snapshot({ in_progress: 1 }),
    })
    expect(t.quiescence.value?.in_progress).toBe(1)
    scope.stop()

    __resetForTests()
    expect(fake.closed).toBe(true)

    // Fresh consumer after reset: idle, null snapshot, and nothing constructed
    // until connect() — so the restored REAL factory is never exercised here.
    const scope2 = effectScope()
    const fresh = scope2.run(() => useSprintTelemetry('sp-9'))!
    expect(fresh.status.value).toBe('idle')
    expect(fresh.quiescence.value).toBeNull()
    scope2.stop()
  })
})
