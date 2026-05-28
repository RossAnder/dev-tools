// Bun tests for the PTY composables — `usePtySessions` (list) and
// `usePtySession` (focused-session live transcript).
//
// T13 of the lumina-pty-service plan
// (docs/plans/lumina-pty-service.md). Mirrors the test seam pattern in
// `readiness.test.ts`: `__setApiForTests` swaps in an in-memory adapter,
// `__resetForTests` clears module-singleton state between tests.

import { beforeEach, describe, expect, test } from 'bun:test'

import type {
  InputFrame,
  PtyMessage,
  PtyMessageKind,
  PtySession,
  SessionStream,
  WsFrame,
  WsFrameType,
} from '../api/pty'
import {
  usePtySessions,
  __resetForTests as __resetSessions,
  __setApiForTests as __setSessionsApi,
} from '../composables/usePtySessions'
import {
  usePtySession,
  __resetForTests as __resetSession,
  __setApiForTests as __setSessionApi,
} from '../composables/usePtySession'

// ---------------------------------------------------------------------------
// Fixtures + helpers.
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
    started_at: '2026-05-27T12:00:00Z',
    updated_at: '2026-05-27T12:00:00Z',
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
  raw_text: string | null = null,
): PtyMessage {
  const defaultRaw =
    typeof content === 'object' &&
    content !== null &&
    'text' in content &&
    typeof (content as Record<string, unknown>).text === 'string'
      ? (content as Record<string, string>).text
      : null
  return {
    id: `${sessionId}-msg-${sequence}`,
    session_id: sessionId,
    sequence,
    created_at: '2026-05-27T12:00:01Z',
    kind,
    content_json: JSON.stringify(content),
    raw_text: raw_text ?? defaultRaw,
  }
}

/**
 * Build a controllable `SessionStream` mock with an `emit` hook so tests can
 * push WS frames as if the server had broadcast them. `send` / `close` /
 * `userClosed` are observable on the returned handle for assertions.
 */
interface MockStream extends SessionStream {
  emit: (frame: WsFrame) => void
  sent: InputFrame[]
  closed: boolean
}

function makeMockStream(): MockStream {
  const handlers = new Map<WsFrameType, Array<(frame: WsFrame) => void>>()
  const sent: InputFrame[] = []
  let closed = false
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
      closed = true
      handle.closed = true
    },
    emit(frame) {
      const list = handlers.get(frame.type)
      if (list) for (const fn of list) fn(frame)
    },
    sent,
    closed,
  }
  return handle
}

// ---------------------------------------------------------------------------
// 1. usePtySessions — loadSessions populates the list.
// ---------------------------------------------------------------------------

describe('usePtySessions', () => {
  beforeEach(() => {
    __resetSessions()
  })

  test('loadSessions seeds sessions and sets status idle on success', async () => {
    const fixture = [makeSession('s1'), makeSession('s2')]
    __setSessionsApi({
      listSessions: async () => fixture,
    })

    const composable = usePtySessions()
    await composable.loadSessions()

    expect(composable.sessions.value).toEqual(fixture)
    expect(composable.status.value).toBe('idle')
    expect(composable.error.value).toBeNull()
  })

  test('loadSessions sets error and status=error on failure', async () => {
    __setSessionsApi({
      listSessions: async () => {
        throw new Error('list failed: bork')
      },
    })

    const composable = usePtySessions()
    await composable.loadSessions()

    expect(composable.status.value).toBe('error')
    expect(composable.error.value).toMatch(/bork/)
  })

  test('spawn prepends the new session to the list', async () => {
    const existing = makeSession('s1')
    const spawned = makeSession('s2', { label: 'fresh' })

    __setSessionsApi({
      listSessions: async () => [existing],
      spawnSession: async () => spawned,
    })

    const composable = usePtySessions()
    await composable.loadSessions()
    expect(composable.sessions.value).toEqual([existing])

    const created = await composable.spawn({ cwd: '/tmp' })
    expect(created).toEqual(spawned)
    // Prepended — newest first.
    expect(composable.sessions.value[0]?.id).toBe('s2')
    expect(composable.sessions.value[1]?.id).toBe('s1')
  })

  test('spawn returns null and sets error on failure', async () => {
    __setSessionsApi({
      spawnSession: async () => {
        throw new Error('spawn failed: nope')
      },
    })

    const composable = usePtySessions()
    const result = await composable.spawn({ cwd: '/tmp' })

    expect(result).toBeNull()
    expect(composable.error.value).toMatch(/nope/)
  })

  test('cancel refreshes the session via getSession', async () => {
    const original = makeSession('s1', { status: 'active' })
    const cancelled = makeSession('s1', { status: 'cancelled' })

    __setSessionsApi({
      listSessions: async () => [original],
      cancelSession: async () => {},
      getSession: async () => cancelled,
    })

    const composable = usePtySessions()
    await composable.loadSessions()
    await composable.cancel('s1')

    expect(composable.sessions.value[0]?.status).toBe('cancelled')
  })

  test('cancel drops the session when getSession 404s', async () => {
    const original = makeSession('s1')

    __setSessionsApi({
      listSessions: async () => [original],
      cancelSession: async () => {},
      getSession: async () => {
        throw new Error('not found')
      },
    })

    const composable = usePtySessions()
    await composable.loadSessions()
    await composable.cancel('s1')

    expect(composable.sessions.value).toHaveLength(0)
  })
})

// ---------------------------------------------------------------------------
// 2. usePtySession — select loads history then opens WS; emitted messages
//    append to the transcript.
// ---------------------------------------------------------------------------

describe('usePtySession', () => {
  beforeEach(() => {
    __resetSession()
  })

  test('select loads history then opens a WS; message frames append', async () => {
    const history = [makeMessage('s1', 0), makeMessage('s1', 1)]
    const mock = makeMockStream()

    __setSessionApi({
      getMessages: async () => history,
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')

    // History is seeded.
    expect(composable.currentId.value).toBe('s1')
    expect(composable.status.value).toBe('open')
    expect(composable.messages.value).toEqual(history)

    // A live broadcast appends.
    mock.emit({
      type: 'message',
      sequence: 2,
      kind: 'assistant_text',
      content: { text: 'live' },
      raw_text: 'live',
      created_at: '2026-05-27T12:00:02Z',
    })

    expect(composable.messages.value).toHaveLength(3)
    const appended = composable.messages.value[2]
    expect(appended?.session_id).toBe('s1')
    expect(appended?.sequence).toBe(2)
    expect(appended?.kind).toBe('assistant_text')
    // Synthesized id from session + sequence.
    expect(appended?.id).toBe('s1-2')
    // content was JSON-stringified on the boundary.
    expect(JSON.parse(appended?.content_json ?? 'null')).toEqual({ text: 'live' })
  })

  test('error frame surfaces on error.value; skipped frame surfaces on error.value', async () => {
    const mock = makeMockStream()

    __setSessionApi({
      getMessages: async () => [],
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')
    expect(composable.error.value).toBeNull()

    mock.emit({ type: 'error', code: 'parser', message: 'bad parser state' })
    expect(composable.error.value).toMatch(/bad parser state/)

    composable.clearError()
    expect(composable.error.value).toBeNull()

    mock.emit({ type: 'skipped', bytes: 4096, reason: 'flood' })
    expect(composable.error.value).toMatch(/4096/)
    expect(composable.error.value).toMatch(/flood/)
  })

  test('submit dispatches an input frame over the stream', async () => {
    const mock = makeMockStream()

    __setSessionApi({
      getMessages: async () => [],
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')
    await composable.submit('hello there')

    expect(mock.sent).toHaveLength(1)
    expect(mock.sent[0]).toEqual({
      type: 'input',
      kind: 'prompt',
      payload: 'hello there',
    })
  })

  test('cancel sends an input frame with kind=cancel', async () => {
    const mock = makeMockStream()

    __setSessionApi({
      getMessages: async () => [],
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')
    await composable.cancel()

    expect(mock.sent).toHaveLength(1)
    expect(mock.sent[0]?.type).toBe('input')
    if (mock.sent[0]?.type === 'input') {
      expect(mock.sent[0].kind).toBe('cancel')
    }
  })

  test('submitBatch calls sendInputsBatch with the current session id', async () => {
    const mock = makeMockStream()
    let batchedTo: string | null = null
    let batched: Array<{ kind: string; payload: string }> | null = null

    __setSessionApi({
      getMessages: async () => [],
      openSessionStream: () => mock,
      sendInputsBatch: async (id, frames) => {
        batchedTo = id
        batched = frames
      },
    })

    const composable = usePtySession()
    await composable.select('s1')
    await composable.submitBatch(['one', 'two'])

    expect(batchedTo).toBe('s1')
    expect(batched).toEqual([
      { kind: 'prompt', payload: 'one' },
      { kind: 'prompt', payload: 'two' },
    ])
  })

  test('disconnect closes the stream and sets status=closed', async () => {
    const mock = makeMockStream()

    __setSessionApi({
      getMessages: async () => [],
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')
    expect(composable.status.value).toBe('open')

    composable.disconnect()
    expect(mock.closed).toBe(true)
    expect(composable.status.value).toBe('closed')
  })

  // -------------------------------------------------------------------------
  // pairedMessages — two-pass tool_use/tool_result pairing view (T7 algorithm,
  // covered here by T9 tests). See `usePtySession.ts` `pairedMessages`
  // computed: tool_use rows attach matching tool_result as `matchedResult`;
  // consumed tool_results are omitted from the top level; orphan rows of
  // either kind render standalone with `matchedResult` undefined.
  // -------------------------------------------------------------------------

  describe('pairedMessages', () => {
    test('single-message history passes through verbatim', async () => {
      const history: PtyMessage[] = [
        makeMessage('s1', 1, 'assistant_text', { text: 'hello' }),
      ]
      const mock = makeMockStream()

      __setSessionApi({
        getMessages: async () => history,
        openSessionStream: () => mock,
      })

      const composable = usePtySession()
      await composable.select('s1')

      expect(composable.pairedMessages.value).toHaveLength(1)
      expect(composable.pairedMessages.value[0]?.kind).toBe('assistant_text')
      expect(composable.pairedMessages.value[0]?.matchedResult).toBeUndefined()
    })

    test('paired tool_use + tool_result collapses into one card, drops orphan', async () => {
      const history: PtyMessage[] = [
        makeMessage('s1', 1, 'assistant_text', { text: 'hello' }),
        makeMessage('s1', 2, 'tool_use', {
          name: 'Read',
          input: { path: '/tmp/foo' },
          tool_use_id: 'x',
        }),
        makeMessage('s1', 3, 'tool_result', {
          tool_use_id: 'x',
          output: 'file contents',
          is_error: false,
        }),
      ]
      const mock = makeMockStream()

      __setSessionApi({
        getMessages: async () => history,
        openSessionStream: () => mock,
      })

      const composable = usePtySession()
      await composable.select('s1')

      const paired = composable.pairedMessages.value
      // Two top-level cards: assistant_text + (tool_use with matched result).
      expect(paired).toHaveLength(2)
      expect(paired[0]?.kind).toBe('assistant_text')
      expect(paired[1]?.kind).toBe('tool_use')

      const matched = paired[1]?.matchedResult
      expect(matched).toBeDefined()
      expect(matched?.kind).toBe('tool_result')
      expect(JSON.parse(matched?.content_json ?? 'null')).toEqual({
        tool_use_id: 'x',
        output: 'file contents',
        is_error: false,
      })

      // The standalone tool_result row must NOT appear at the top level.
      const topLevelToolResults = paired.filter((r) => r.kind === 'tool_result')
      expect(topLevelToolResults).toHaveLength(0)
    })

    test('orphan tool_result (no parent tool_use) renders standalone', async () => {
      const history: PtyMessage[] = [
        makeMessage('s1', 1, 'user_input', { text: 'do the thing' }),
        makeMessage('s1', 2, 'tool_result', {
          tool_use_id: 'orphan-z',
          output: 'detached output',
          is_error: false,
        }),
      ]
      const mock = makeMockStream()

      __setSessionApi({
        getMessages: async () => history,
        openSessionStream: () => mock,
      })

      const composable = usePtySession()
      await composable.select('s1')

      const paired = composable.pairedMessages.value
      expect(paired).toHaveLength(2)
      expect(paired[0]?.kind).toBe('user_input')
      expect(paired[1]?.kind).toBe('tool_result')
      // Orphan tool_result carries no matchedResult.
      expect(paired[1]?.matchedResult).toBeUndefined()
    })

    test('orphan tool_use (no matching tool_result) renders standalone', async () => {
      const history: PtyMessage[] = [
        makeMessage('s1', 1, 'assistant_text', { text: 'pondering' }),
        makeMessage('s1', 2, 'tool_use', {
          name: 'Bash',
          input: { command: 'ls' },
          tool_use_id: 'y',
        }),
      ]
      const mock = makeMockStream()

      __setSessionApi({
        getMessages: async () => history,
        openSessionStream: () => mock,
      })

      const composable = usePtySession()
      await composable.select('s1')

      const paired = composable.pairedMessages.value
      expect(paired).toHaveLength(2)
      expect(paired[1]?.kind).toBe('tool_use')
      // No matching result yet — matchedResult undefined.
      expect(paired[1]?.matchedResult).toBeUndefined()
    })

    test('WS-frame round-trip: live emit of tool_use+tool_result collapses', async () => {
      const mock = makeMockStream()

      __setSessionApi({
        getMessages: async () => [],
        openSessionStream: () => mock,
      })

      const composable = usePtySession()
      await composable.select('s1')

      // Empty start.
      expect(composable.pairedMessages.value).toHaveLength(0)

      // Emit banner assistant_text + a tool_use/tool_result pair via WS.
      mock.emit({
        type: 'message',
        sequence: 1,
        kind: 'assistant_text',
        content: { text: 'starting' },
        raw_text: 'starting',
        created_at: '2026-05-27T12:00:01Z',
      })
      mock.emit({
        type: 'message',
        sequence: 2,
        kind: 'tool_use',
        content: {
          name: 'Read',
          input: { path: '/tmp/foo' },
          tool_use_id: 'live-x',
        },
        raw_text: null,
        created_at: '2026-05-27T12:00:02Z',
      })
      mock.emit({
        type: 'message',
        sequence: 3,
        kind: 'tool_result',
        content: {
          tool_use_id: 'live-x',
          output: 'streamed contents',
          is_error: false,
        },
        raw_text: null,
        created_at: '2026-05-27T12:00:03Z',
      })

      // Raw transcript has all 3 rows; paired view collapses to 2.
      expect(composable.messages.value).toHaveLength(3)
      const paired = composable.pairedMessages.value
      expect(paired).toHaveLength(2)
      expect(paired[0]?.kind).toBe('assistant_text')
      expect(paired[1]?.kind).toBe('tool_use')
      expect(paired[1]?.matchedResult?.kind).toBe('tool_result')
      expect(JSON.parse(paired[1]?.matchedResult?.content_json ?? 'null')).toEqual({
        tool_use_id: 'live-x',
        output: 'streamed contents',
        is_error: false,
      })
    })
  })
})

// ---------------------------------------------------------------------------
// 3. usePtySession — token cancellation: a slow first select must NOT overwrite
//    the messages from a faster second select that lands first.
// ---------------------------------------------------------------------------

describe('usePtySession token cancellation', () => {
  beforeEach(() => {
    __resetSession()
  })

  test('a slow history load for the previous session is discarded', async () => {
    // Pending-history machinery: each session id resolves its history only
    // when we call its `resolve` from the test body.  This lets us interleave
    // the two selects deterministically without timers.
    const pending = new Map<string, (msgs: PtyMessage[]) => void>()

    __setSessionApi({
      getMessages: (id: string) =>
        new Promise<PtyMessage[]>((resolve) => {
          pending.set(id, resolve)
        }),
      openSessionStream: () => makeMockStream(),
    })

    const composable = usePtySession()

    // Start two overlapping selects.  Neither has resolved its history yet.
    const slow = composable.select('s1')
    const fast = composable.select('s2')

    // Resolve the faster (later) one first — currentId is now s2 and its
    // history seeds messages.
    pending.get('s2')!([makeMessage('s2', 0)])
    await fast

    expect(composable.currentId.value).toBe('s2')
    expect(composable.messages.value).toEqual([makeMessage('s2', 0)])

    // Now resolve the slower (earlier) one.  Its token check should bail,
    // so messages must remain the s2 fixture.
    pending.get('s1')!([makeMessage('s1', 0), makeMessage('s1', 1)])
    await slow

    expect(composable.currentId.value).toBe('s2')
    expect(composable.messages.value).toEqual([makeMessage('s2', 0)])
  })
})
