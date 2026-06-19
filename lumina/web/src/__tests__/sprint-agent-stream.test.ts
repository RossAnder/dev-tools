// Bun tests for the `useSprintAgentStream` composable
// (src/composables/useSprintAgentStream.ts) — T19 of the read-only
// sprint/worktree visibility slice (docs/plans/vectorized-brewing-boole.md,
// Wave 3).
//
// Seams: the composable's own `__setApiForTests` (fake `listSessions` /
// `openSessionStream` / `getMessages` — the `SessionStream` fake mirrors
// pty-session.test.ts's `makeMockStream`) plus useResourceStream's injectable
// stream factory for the quiescence-driven list-refresh trigger (the fake
// resource stream mirrors sprint-telemetry.test.ts).

import { afterEach, describe, expect, test } from 'bun:test'

import { sprintQuiescenceTopic, type SprintQuiescence } from '../api/execution'
import type {
  InputFrame,
  PtyMessage,
  PtyMessageKind,
  PtySession,
  SessionStream,
  WsFrame,
  WsFrameType,
} from '../api/pty'
import type { OutboundFrame } from '../api/stream'
import {
  __resetForTests as __resetResourceStream,
  __setApiForTests as __setStreamFactory,
  type ResourceStreamLike,
} from '../composables/useResourceStream'
import {
  useSprintAgentStream,
  SUMMARY_MAX_CHARS,
  __resetForTests,
  __setApiForTests,
} from '../composables/useSprintAgentStream'

// ---------------------------------------------------------------------------
// Fixtures + helpers (mirroring pty-session.test.ts).
// ---------------------------------------------------------------------------

function makeSession(id: string, overrides: Partial<PtySession> = {}): PtySession {
  return {
    id,
    label: null,
    project_id: null,
    cwd: '/tmp',
    config_json: '{}',
    parse_strategy_version: 1,
    status: 'active',
    started_at: '2026-06-11T12:00:00Z',
    updated_at: '2026-06-11T12:00:00Z',
    ended_at: null,
    exit_code: null,
    last_error: null,
    previous_session_id: null,
    ...overrides,
  }
}

function makeMessage(
  sessionId: string,
  sequence: number,
  kind: PtyMessageKind = 'assistant_text',
  content: unknown = { text: `m${sequence}` },
): PtyMessage {
  return {
    id: `${sessionId}-msg-${sequence}`,
    session_id: sessionId,
    sequence,
    created_at: '2026-06-11T12:00:01Z',
    kind,
    content_json: JSON.stringify(content),
    raw_text: null,
  }
}

/**
 * Controllable `SessionStream` mock with an `emit` hook so tests can push WS
 * frames as if the server had broadcast them — the exact fake style of
 * pty-session.test.ts.
 */
interface MockStream extends SessionStream {
  emit: (frame: WsFrame) => void
  sent: InputFrame[]
  closed: boolean
}

function makeMockStream(): MockStream {
  const handlers = new Map<WsFrameType, Array<(frame: WsFrame) => void>>()
  const sent: InputFrame[] = []
  const handle: MockStream = {
    send(frame) {
      sent.push(frame)
    },
    on(event, handler) {
      const list = handlers.get(event)
      if (list) list.push(handler)
      else handlers.set(event, [handler])
    },
    close() {
      handle.closed = true
    },
    emit(frame) {
      const list = handlers.get(frame.type)
      if (list) for (const fn of list) fn(frame)
    },
    sent,
    closed: false,
  }
  return handle
}

// ---------------------------------------------------------------------------
// Controllable fake resource stream (mirrors sprint-telemetry.test.ts) — for
// the quiescence-driven list-refresh trigger.
// ---------------------------------------------------------------------------

interface SubEntry {
  topic: string
  onFrame: (frame: OutboundFrame) => void
  active: boolean
}

function makeFakeStream() {
  const subs: SubEntry[] = []

  const stream: ResourceStreamLike = {
    subscribe(topic, onFrame) {
      const entry: SubEntry = { topic, onFrame, active: true }
      subs.push(entry)
      return () => {
        entry.active = false
      }
    },
    onStatus() {},
    close() {},
  }

  return {
    factory: (): ResourceStreamLike => stream,
    subs,
    /** Deliver a frame to every ACTIVE handler on `topic`. */
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
    blocked_by_finding: 0,
    done: false,
    blocked: false,
    stalled: false,
    ...overrides,
  }
}

afterEach(() => {
  __resetForTests()
  __resetResourceStream()
})

// ---------------------------------------------------------------------------
// 1. loadForSprint — list fetch + latest-session stream attach.
// ---------------------------------------------------------------------------

describe('useSprintAgentStream loadForSprint', () => {
  test('populates sessions via ?sprint_id= and streams the LATEST session', async () => {
    // Older session listed FIRST to prove latest-pick is by started_at, not
    // list position.
    const older = makeSession('s-old', { started_at: '2026-06-11T11:00:00Z' })
    const newer = makeSession('s-new', { started_at: '2026-06-11T13:00:00Z' })

    let listedWith: { status?: string; project_id?: string; sprint_id?: string } | undefined
    const openedIds: string[] = []
    const mock = makeMockStream()

    __setApiForTests({
      listSessions: async (params) => {
        listedWith = params
        return [older, newer]
      },
      openSessionStream: (id) => {
        openedIds.push(id)
        return mock
      },
    })

    const composable = useSprintAgentStream()
    await composable.loadForSprint('sp-1')

    expect(listedWith?.sprint_id).toBe('sp-1')
    expect(composable.sessions.value).toEqual([older, newer])
    expect(openedIds).toEqual(['s-new'])
    expect(composable.status.value).toBe('open')
    expect(composable.error.value).toBeNull()
  })

  test('list failure sets error and status=error', async () => {
    __setApiForTests({
      listSessions: async () => {
        throw new Error('list failed: bork')
      },
    })

    const composable = useSprintAgentStream()
    await composable.loadForSprint('sp-1')

    expect(composable.status.value).toBe('error')
    expect(composable.error.value).toMatch(/bork/)
  })

  test('an empty session list goes idle without opening a stream', async () => {
    const openedIds: string[] = []
    __setApiForTests({
      listSessions: async () => [],
      openSessionStream: (id) => {
        openedIds.push(id)
        return makeMockStream()
      },
    })

    const composable = useSprintAgentStream()
    await composable.loadForSprint('sp-1')

    expect(composable.sessions.value).toHaveLength(0)
    expect(openedIds).toHaveLength(0)
    expect(composable.status.value).toBe('idle')
  })

  test('a refresh with an unchanged latest session keeps the stream and folded frames', async () => {
    const session = makeSession('s-1')
    const openedIds: string[] = []
    const mock = makeMockStream()

    __setApiForTests({
      listSessions: async () => [session],
      openSessionStream: (id) => {
        openedIds.push(id)
        return mock
      },
    })

    const composable = useSprintAgentStream()
    await composable.loadForSprint('sp-1')

    mock.emit({
      type: 'message',
      sequence: 1,
      kind: 'assistant_text',
      content: { text: 'live' },
      raw_text: 'live',
      created_at: '2026-06-11T12:00:02Z',
    })
    expect(composable.liveMessages.value).toHaveLength(1)

    // Refresh (the quiescence-trigger hot path): same latest session — the
    // stream must NOT bounce and the folded frames must survive.
    await composable.loadForSprint('sp-1')

    expect(openedIds).toEqual(['s-1'])
    expect(mock.closed).toBe(false)
    expect(composable.liveMessages.value).toHaveLength(1)
    expect(composable.status.value).toBe('open')
  })
})

// ---------------------------------------------------------------------------
// 2. summaryItems — one-line summaries with truncation.
// ---------------------------------------------------------------------------

describe('useSprintAgentStream summaryItems', () => {
  test('a fed message frame becomes a truncated one-line summary', async () => {
    const mock = makeMockStream()
    __setApiForTests({
      listSessions: async () => [makeSession('s-1')],
      openSessionStream: () => mock,
    })

    const composable = useSprintAgentStream()
    await composable.loadForSprint('sp-1')

    const long = 'x'.repeat(200)
    mock.emit({
      type: 'message',
      sequence: 1,
      kind: 'assistant_text',
      content: { text: long },
      raw_text: long,
      created_at: '2026-06-11T12:00:02Z',
    })

    expect(composable.summaryItems.value).toHaveLength(1)
    const item = composable.summaryItems.value[0]
    expect(item?.kind).toBe('assistant_text')
    expect(item?.session_id).toBe('s-1')
    expect(item?.created_at).toBe('2026-06-11T12:00:02Z')
    // Truncated to the cap INCLUDING the trailing ellipsis.
    expect(item?.text).toHaveLength(SUMMARY_MAX_CHARS)
    expect(item?.text.endsWith('…')).toBe(true)
    expect(item?.text.startsWith('xxx')).toBe(true)
  })

  test('short content passes through untruncated; newlines collapse to one line', async () => {
    const mock = makeMockStream()
    __setApiForTests({
      listSessions: async () => [makeSession('s-1')],
      openSessionStream: () => mock,
    })

    const composable = useSprintAgentStream()
    await composable.loadForSprint('sp-1')

    mock.emit({
      type: 'message',
      sequence: 1,
      kind: 'assistant_text',
      content: { text: 'line one\nline two' },
      raw_text: null,
      created_at: '2026-06-11T12:00:02Z',
    })
    // A tool_use frame summarises via its tool name.
    mock.emit({
      type: 'message',
      sequence: 2,
      kind: 'tool_use',
      content: { name: 'Read', input: { path: '/tmp/foo' }, tool_use_id: 'x' },
      raw_text: null,
      created_at: '2026-06-11T12:00:03Z',
    })

    const items = composable.summaryItems.value
    expect(items).toHaveLength(2)
    expect(items[0]?.text).toBe('line one line two')
    expect(items[0]?.text.endsWith('…')).toBe(false)
    expect(items[1]?.kind).toBe('tool_use')
    expect(items[1]?.text).toBe('Read')
  })
})

// ---------------------------------------------------------------------------
// 3. openTranscript — full stored transcript for the modal.
// ---------------------------------------------------------------------------

describe('useSprintAgentStream openTranscript', () => {
  test('fetches and returns the transcript via getMessages', async () => {
    const transcript = [makeMessage('s-9', 0), makeMessage('s-9', 1)]
    let fetchedId: string | null = null

    __setApiForTests({
      getMessages: async (id) => {
        fetchedId = id
        return transcript
      },
    })

    const composable = useSprintAgentStream()
    const result = await composable.openTranscript('s-9')

    expect(fetchedId).toBe('s-9')
    expect(result).toEqual(transcript)
  })

  test('returns null and sets error on fetch failure', async () => {
    __setApiForTests({
      getMessages: async () => {
        throw new Error('transcript failed: nope')
      },
    })

    const composable = useSprintAgentStream()
    const result = await composable.openTranscript('s-9')

    expect(result).toBeNull()
    expect(composable.error.value).toMatch(/nope/)
  })
})

// ---------------------------------------------------------------------------
// 4. bind — quiescence-driven v1 list refresh (no poll loop): each `data`
//    frame on the bound sprint's quiescence topic re-runs loadForSprint.
// ---------------------------------------------------------------------------

describe('useSprintAgentStream quiescence-driven refresh', () => {
  test('bind subscribes the sprint quiescence topic; a data frame refetches the list', async () => {
    const fake = makeFakeStream()
    __setStreamFactory(fake.factory)

    let listCalls = 0
    __setApiForTests({
      listSessions: async () => {
        listCalls += 1
        return []
      },
    })

    const composable = useSprintAgentStream()
    await composable.bind('sp-1')

    expect(composable.boundSprintId.value).toBe('sp-1')
    expect(listCalls).toBe(1)
    // The telemetry binding subscribed the canonical topic form.
    expect(fake.subs).toHaveLength(1)
    expect(fake.subs[0]?.topic).toBe(sprintQuiescenceTopic('sp-1'))

    // A quiescence snapshot change (claim/complete rode the bus) → refetch.
    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'data',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snapshot({ in_progress: 1 }),
    })
    expect(listCalls).toBe(2)

    // Another change → another refetch.
    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'data',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snapshot({ in_progress: 2 }),
    })
    expect(listCalls).toBe(3)
  })

  test('disconnect tears down the telemetry subscription and stops refreshing', async () => {
    const fake = makeFakeStream()
    __setStreamFactory(fake.factory)

    let listCalls = 0
    __setApiForTests({
      listSessions: async () => {
        listCalls += 1
        return []
      },
    })

    const composable = useSprintAgentStream()
    await composable.bind('sp-1')
    expect(listCalls).toBe(1)

    composable.disconnect()
    expect(composable.status.value).toBe('closed')
    expect(fake.subs[0]?.active).toBe(false)

    // A late frame on the old topic must not trigger a refetch.
    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'data',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snapshot({ done: true }),
    })
    expect(listCalls).toBe(1)
  })
})

// ---------------------------------------------------------------------------
// 5. launch — launchSprint() + optimistic prepend + immediate stream attach +
//    reconnect/refresh survival (PTY create emits no notify-bus event).
// ---------------------------------------------------------------------------

describe('useSprintAgentStream launch', () => {
  test('launches via launchSprint, prepends optimistically, and streams it immediately', async () => {
    // started_at LATER than the existing session so pickLatest favours it.
    const existing = makeSession('s-old', { started_at: '2026-06-11T11:00:00Z' })
    const launched = makeSession('s-launch', { started_at: '2026-06-11T14:00:00Z' })

    let launchedWith: string | null = null
    const openedIds: string[] = []
    const mock = makeMockStream()

    __setApiForTests({
      listSessions: async () => [existing],
      launchSprint: async (sprintId) => {
        launchedWith = sprintId
        return launched
      },
      openSessionStream: (id) => {
        openedIds.push(id)
        return mock
      },
    })

    const composable = useSprintAgentStream()
    await composable.bind('sp-1')
    expect(composable.sessions.value).toEqual([existing])

    const result = await composable.launch('sp-1')

    expect(launchedWith).toBe('sp-1')
    expect(result).toEqual(launched)
    // Optimistically prepended (newest first).
    expect(composable.sessions.value[0]?.id).toBe('s-launch')
    expect(composable.sessions.value).toHaveLength(2)
    // bind() attached to the prior latest (s-old); launch() then re-attached
    // to the launched session, so it is the CURRENT stream.
    expect(openedIds[openedIds.length - 1]).toBe('s-launch')

    // A frame on the launched session's stream folds immediately.
    mock.emit({
      type: 'message',
      sequence: 1,
      kind: 'assistant_text',
      content: { text: 'spawned output' },
      raw_text: 'spawned output',
      created_at: '2026-06-11T14:00:01Z',
    })
    expect(composable.liveMessages.value).toHaveLength(1)
  })

  test('a refresh that does not yet list the launched session keeps it pinned and the stream alive', async () => {
    const fake = makeFakeStream()
    __setStreamFactory(fake.factory)

    const launched = makeSession('s-launch', { started_at: '2026-06-11T14:00:00Z' })
    const openedIds: string[] = []
    const mock = makeMockStream()

    // The server list LAGS — PTY create emits no notify-bus event, so the
    // ?sprint_id= list does not carry the launched session for a refresh cycle.
    __setApiForTests({
      listSessions: async () => [],
      launchSprint: async () => launched,
      openSessionStream: (id) => {
        openedIds.push(id)
        return mock
      },
    })

    const composable = useSprintAgentStream()
    await composable.bind('sp-1')
    await composable.launch('sp-1')

    expect(composable.sessions.value).toEqual([launched])
    expect(openedIds).toEqual(['s-launch'])

    // A quiescence-driven refresh fires while the list still lags.
    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'data',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snapshot({ in_progress: 1 }),
    })
    await Promise.resolve()

    // The launched run survives: still listed, stream NOT bounced, still open.
    expect(composable.sessions.value.some((s) => s.id === 's-launch')).toBe(true)
    expect(mock.closed).toBe(false)
    expect(openedIds).toEqual(['s-launch'])
    expect(composable.status.value).toBe('open')
  })

  test('once the server list carries the launched session, the pin clears (no duplicate)', async () => {
    const fake = makeFakeStream()
    __setStreamFactory(fake.factory)

    const launched = makeSession('s-launch', { started_at: '2026-06-11T14:00:00Z' })
    let serverHasIt = false
    const mock = makeMockStream()

    __setApiForTests({
      listSessions: async () => (serverHasIt ? [launched] : []),
      launchSprint: async () => launched,
      openSessionStream: () => mock,
    })

    const composable = useSprintAgentStream()
    await composable.bind('sp-1')
    await composable.launch('sp-1')
    expect(composable.sessions.value).toEqual([launched])

    // The correlated row now appears in the server list.
    serverHasIt = true
    fake.push(sprintQuiescenceTopic('sp-1'), {
      type: 'data',
      topic: sprintQuiescenceTopic('sp-1'),
      data: snapshot({ in_progress: 1 }),
    })
    await Promise.resolve()

    // Exactly one row — the real one supersedes the pinned optimistic one.
    expect(composable.sessions.value).toHaveLength(1)
    expect(composable.sessions.value[0]?.id).toBe('s-launch')
  })

  test('launch failure sets error and returns null without prepending', async () => {
    __setApiForTests({
      listSessions: async () => [],
      launchSprint: async () => {
        throw new Error('launch failed: boom')
      },
    })

    const composable = useSprintAgentStream()
    await composable.bind('sp-1')
    const result = await composable.launch('sp-1')

    expect(result).toBeNull()
    expect(composable.error.value).toMatch(/boom/)
    expect(composable.sessions.value).toHaveLength(0)
  })
})
