// Bun tests for the PTY composables — `usePtySessions` (list) and
// `usePtySession` (focused-session live transcript).
//
// T13 of the lumina-pty-service plan
// (docs/plans/lumina-pty-service.md). Mirrors the test seam pattern in
// `readiness.test.ts`: `__setApiForTests` swaps in an in-memory adapter,
// `__resetForTests` clears module-singleton state between tests.

import { beforeEach, describe, expect, mock, test } from 'bun:test'

import type {
  AuqAnswer,
  AuqQuestion,
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
      getSession: async () => makeSession('s1'),
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
      getSession: async () => makeSession('s1'),
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
      getSession: async () => makeSession('s1'),
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
      getSession: async () => makeSession('s1'),
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
      getSession: async () => makeSession('s1'),
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
      getSession: async () => makeSession('s1'),
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
  // sessionStatus — live WS-driven lifecycle status (distinct from `status`
  // which tracks the socket). Seeded on select from the persisted row, then
  // updated on every `status` frame the server broadcasts. The late-frame
  // guard (currentId !== id) drops frames from a previous focus after the
  // user switches sessions.
  // -------------------------------------------------------------------------

  test('on status frame updates sessionStatus', async () => {
    const mock = makeMockStream()

    __setSessionApi({
      getSession: async () => makeSession('s1', { status: 'active' }),
      getMessages: async () => [],
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')

    // Seeded from the persisted row on select.
    expect(composable.sessionStatus.value).toBe('active')

    // Server broadcasts a status transition → ref updates.
    mock.emit({ type: 'status', status: 'idle', at: '2026-05-28T12:00:00Z' })
    expect(composable.sessionStatus.value).toBe('idle')

    // Subsequent transitions propagate.
    mock.emit({ type: 'status', status: 'awaiting', at: '2026-05-28T12:00:01Z' })
    expect(composable.sessionStatus.value).toBe('awaiting')
  })

  test('status frame for stale session is dropped after switch', async () => {
    const mock1 = makeMockStream()
    const mock2 = makeMockStream()

    // Per-session getSession + openSessionStream so we can interleave two
    // selects and prove the on('status') guard drops late frames from
    // a previously-focused session.
    __setSessionApi({
      getSession: async (id) =>
        makeSession(id, { status: id === 's1' ? 'active' : 'idle' }),
      getMessages: async () => [],
      openSessionStream: (id) => (id === 's1' ? mock1 : mock2),
    })

    const composable = usePtySession()
    await composable.select('s1')
    expect(composable.sessionStatus.value).toBe('active')

    await composable.select('s2')
    expect(composable.sessionStatus.value).toBe('idle')

    // A late status frame from s1's now-stale stream must NOT mutate the
    // ref — the on('status') guard checks `currentId !== id` before write.
    mock1.emit({ type: 'status', status: 'failed', at: '2026-05-28T12:00:02Z' })
    expect(composable.sessionStatus.value).toBe('idle')
  })

  test('select clears sessionStatus before history fetch resolves', async () => {
    // First select resolves synchronously and seeds sessionStatus=active.
    const mock1 = makeMockStream()
    __setSessionApi({
      getSession: async () => makeSession('s1', { status: 'active' }),
      getMessages: async () => [],
      openSessionStream: () => mock1,
    })

    const composable = usePtySession()
    await composable.select('s1')
    expect(composable.sessionStatus.value).toBe('active')

    // Second select uses a slow getMessages so we can observe the
    // synchronous reset at the top of select().
    let releaseHistory: (msgs: PtyMessage[]) => void = () => {}
    __setSessionApi({
      getSession: async () => makeSession('s2', { status: 'idle' }),
      getMessages: () =>
        new Promise<PtyMessage[]>((resolve) => {
          releaseHistory = resolve
        }),
      openSessionStream: () => makeMockStream(),
    })

    const pending = composable.select('s2')
    // Synchronous post-call observation: the reset at the top of select()
    // has already fired, but neither the history fetch nor the getSession
    // seed has resolved yet.
    expect(composable.sessionStatus.value).toBeNull()

    // Drain the in-flight select to keep the singleton clean for the next test.
    releaseHistory([])
    await pending
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
        getSession: async () => makeSession('s1'),
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
        getSession: async () => makeSession('s1'),
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
        getSession: async () => makeSession('s1'),
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
        getSession: async () => makeSession('s1'),
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
        getSession: async () => makeSession('s1'),
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
      getSession: async (id: string) => makeSession(id),
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

// ---------------------------------------------------------------------------
// 4. usePtySession AUQ extensions.
//
// Covers `pendingAuq` derivation from `pairedMessages`, `submitAuqAnswer` +
// `cancelAuq` routing through the answer endpoint (`answerQuestion` /
// `cancelQuestion`, which POST to `/api/pty/sessions/{id}/ask/{qid}/answer` to
// resolve a blocked `ask_user_question` MCP tool call), the
// per-`pendingAuq.toolUseId` debounce, and the watcher-driven debounce reset
// when `pendingAuq` transitions to null.
//
// Test seam: rather than reach into the module-singleton `messages` ref
// (private), AUQ rows are staged either via the history fetch (`getMessages`
// returns the seed transcript) or via WS-frame emission (`mock.emit` pushes
// a `message` frame that the composable appends to `messages`).
// ---------------------------------------------------------------------------

import { nextTick } from 'vue'

// ---------------------------------------------------------------------------
// AUQ-specific fixture builders.
// ---------------------------------------------------------------------------

function makeAuqQuestion(numOptions: number): AuqQuestion {
  return {
    question: `Pick one of ${numOptions}`,
    header: 'Q',
    multiSelect: false,
    options: Array.from({ length: numOptions }, (_, i) => ({
      label: `Option ${i}`,
      description: `desc ${i}`,
    })),
  }
}

/**
 * Build a PtyMessage row whose content_json encodes an AskUserQuestion
 * tool_use. The `tool_use_id` is the key on which `pendingAuq` pairs the
 * matching `tool_result`.
 */
function makeAuqToolUseRow(
  sessionId: string,
  sequence: number,
  toolUseId: string,
  questions: AuqQuestion[],
): PtyMessage {
  return makeMessage(sessionId, sequence, 'tool_use', {
    name: 'AskUserQuestion',
    input: { questions },
    tool_use_id: toolUseId,
  })
}

/** Build a tool_result row that pairs with a given tool_use_id. */
function makeToolResultRow(
  sessionId: string,
  sequence: number,
  toolUseId: string,
): PtyMessage {
  return makeMessage(sessionId, sequence, 'tool_result', {
    tool_use_id: toolUseId,
    output: 'answered',
    is_error: false,
  })
}

describe('usePtySession AUQ extensions (T9)', () => {
  beforeEach(() => {
    __resetSession()
  })

  // -------------------------------------------------------------------------
  // pendingAuq derivation.
  // -------------------------------------------------------------------------

  test('pendingAuq is null when no AUQ in transcript', async () => {
    const mock = makeMockStream()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => [makeMessage('s1', 0)],
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')

    expect(composable.pendingAuq.value).toBeNull()
  })

  test('pendingAuq returns {toolUseId, questions} for one unmatched AUQ', async () => {
    const questions = [makeAuqQuestion(3)]
    const history: PtyMessage[] = [
      makeMessage('s1', 0, 'assistant_text', { text: 'thinking' }),
      makeAuqToolUseRow('s1', 1, 'toolu_x', questions),
    ]
    const mock = makeMockStream()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => history,
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')

    const pending = composable.pendingAuq.value
    expect(pending).not.toBeNull()
    expect(pending?.toolUseId).toBe('toolu_x')
    expect(pending?.questions).toEqual(questions)
  })

  test('pendingAuq is null when the AUQ is matched (tool_result present)', async () => {
    const questions = [makeAuqQuestion(2)]
    const history: PtyMessage[] = [
      makeAuqToolUseRow('s1', 0, 'toolu_x', questions),
      makeToolResultRow('s1', 1, 'toolu_x'),
    ]
    const mock = makeMockStream()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => history,
      openSessionStream: () => mock,
    })

    const composable = usePtySession()
    await composable.select('s1')

    expect(composable.pendingAuq.value).toBeNull()
  })

  test('pendingAuq tie-breaker: returns oldest; warns once when >1 unmatched', async () => {
    const questionsA = [makeAuqQuestion(2)]
    const questionsB = [makeAuqQuestion(3)]
    const history: PtyMessage[] = [
      makeAuqToolUseRow('s1', 0, 'toolu_a', questionsA),
      makeAuqToolUseRow('s1', 1, 'toolu_b', questionsB),
    ]
    const mock = makeMockStream()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => history,
      openSessionStream: () => mock,
    })

    const warnings: string[] = []
    const originalWarn = console.warn
    console.warn = (...args: unknown[]) => {
      warnings.push(args.map((a) => String(a)).join(' '))
    }

    try {
      const composable = usePtySession()
      await composable.select('s1')

      const pending = composable.pendingAuq.value
      expect(pending?.toolUseId).toBe('toolu_a')
      expect(pending?.questions).toEqual(questionsA)

      // The computed evaluated once on the .value read above; the warn
      // should have fired exactly once for the >1 unmatched-AUQ condition.
      expect(warnings).toHaveLength(1)
      expect(warnings[0]).toMatch(/unmatched AUQ/i)
      expect(warnings[0]).toMatch(/toolu_a/)
    } finally {
      console.warn = originalWarn
    }
  })

  // -------------------------------------------------------------------------
  // submitAuqAnswer routing.
  // -------------------------------------------------------------------------

  test('submitAuqAnswer posts to answerQuestion(sid, toolUseId, answers)', async () => {
    const questions = [makeAuqQuestion(3)]
    const history: PtyMessage[] = [makeAuqToolUseRow('s1', 0, 'toolu_x', questions)]
    const mock = makeMockStream()

    const answerMock = mockAnswerQuestion()

    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => history,
      openSessionStream: () => mock,
      answerQuestion: answerMock.fn,
    })

    const composable = usePtySession()
    await composable.select('s1')

    // pendingAuq must be live before submit.
    expect(composable.pendingAuq.value?.toolUseId).toBe('toolu_x')

    const answer: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 2'],
    }
    await composable.submitAuqAnswer([answer])

    // Exactly one POST to the answer endpoint, carrying the session id, the
    // AUQ tool_use_id (== the question id), and the answers verbatim.
    expect(answerMock.calls).toHaveLength(1)
    expect(answerMock.calls[0]?.id).toBe('s1')
    expect(answerMock.calls[0]?.questionId).toBe('toolu_x')
    expect(answerMock.calls[0]?.answers).toEqual([answer])
  })

  test('submitAuqAnswer is a no-op when no pending AUQ', async () => {
    const answerMock = mockAnswerQuestion()

    const mock = makeMockStream()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => [],
      openSessionStream: () => mock,
      answerQuestion: answerMock.fn,
    })

    const composable = usePtySession()
    await composable.select('s1')
    expect(composable.pendingAuq.value).toBeNull()

    await composable.submitAuqAnswer([
      { questionIndex: 0, selectedLabels: ['anything'] },
    ])

    expect(answerMock.calls).toHaveLength(0)
  })

  test('submitAuqAnswer debounces — second call within same pendingAuq.toolUseId is dropped', async () => {
    const questions = [makeAuqQuestion(2)]
    const history: PtyMessage[] = [makeAuqToolUseRow('s1', 0, 'toolu_x', questions)]
    const mock = makeMockStream()

    const answerMock = mockAnswerQuestion()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => history,
      openSessionStream: () => mock,
      answerQuestion: answerMock.fn,
    })

    const composable = usePtySession()
    await composable.select('s1')

    const answer: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 0'],
    }
    // Two synchronous calls in a row — second must be dropped via the
    // inflightAuqToolUseId debounce guard.
    await composable.submitAuqAnswer([answer])
    await composable.submitAuqAnswer([answer])

    expect(answerMock.calls).toHaveLength(1)
  })

  test('submitAuqAnswer clears debounce key on POST failure so the user can retry', async () => {
    const questions = [makeAuqQuestion(2)]
    const history: PtyMessage[] = [makeAuqToolUseRow('s1', 0, 'toolu_x', questions)]
    const mock = makeMockStream()

    let fail = true
    const calls: Array<{ id: string; questionId: string; answers: AuqAnswer[] }> = []
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => history,
      openSessionStream: () => mock,
      answerQuestion: async (id, questionId, answers) => {
        calls.push({ id, questionId, answers })
        if (fail) {
          throw new Error('network: ECONNRESET')
        }
      },
    })

    const composable = usePtySession()
    await composable.select('s1')

    const answer: AuqAnswer = {
      questionIndex: 0,
      selectedLabels: ['Option 0'],
    }
    await expect(composable.submitAuqAnswer([answer])).rejects.toThrow(
      /ECONNRESET/,
    )
    // First call failed → debounce key cleared → second call should fire.
    fail = false
    await composable.submitAuqAnswer([answer])

    expect(calls).toHaveLength(2)
  })

  // -------------------------------------------------------------------------
  // cancelAuq routing.
  // -------------------------------------------------------------------------

  test('cancelAuq posts to cancelQuestion(sid, toolUseId)', async () => {
    const questions = [makeAuqQuestion(2)]
    const history: PtyMessage[] = [makeAuqToolUseRow('s1', 0, 'toolu_x', questions)]
    const mock = makeMockStream()

    const cancelMock = mockCancelQuestion()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => history,
      openSessionStream: () => mock,
      cancelQuestion: cancelMock.fn,
    })

    const composable = usePtySession()
    await composable.select('s1')
    await composable.cancelAuq()

    expect(cancelMock.calls).toHaveLength(1)
    expect(cancelMock.calls[0]?.id).toBe('s1')
    expect(cancelMock.calls[0]?.questionId).toBe('toolu_x')
  })

  test('cancelAuq is a no-op when no pending AUQ', async () => {
    const cancelMock = mockCancelQuestion()
    const mock = makeMockStream()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => [],
      openSessionStream: () => mock,
      cancelQuestion: cancelMock.fn,
    })

    const composable = usePtySession()
    await composable.select('s1')
    expect(composable.pendingAuq.value).toBeNull()

    await composable.cancelAuq()

    expect(cancelMock.calls).toHaveLength(0)
  })

  // -------------------------------------------------------------------------
  // Debounce-key reset when pendingAuq transitions to null.
  // -------------------------------------------------------------------------

  test('inflight debounce key clears when pendingAuq transitions to null', async () => {
    const qA = [makeAuqQuestion(2)]
    const qB = [makeAuqQuestion(3)]
    // Seed only the first AUQ in history — the matching tool_result and the
    // follow-on AUQ arrive via WS-frame emission.
    const history: PtyMessage[] = [makeAuqToolUseRow('s1', 0, 'toolu_a', qA)]
    const mock = makeMockStream()

    const answerMock = mockAnswerQuestion()
    __setSessionApi({
      getSession: async () => makeSession('s1'),
      getMessages: async () => history,
      openSessionStream: () => mock,
      answerQuestion: answerMock.fn,
    })

    const composable = usePtySession()
    await composable.select('s1')

    // 1. First pending AUQ live → submitAuqAnswer fires once and stays
    //    debounced under toolu_a.
    expect(composable.pendingAuq.value?.toolUseId).toBe('toolu_a')
    await composable.submitAuqAnswer([
      { questionIndex: 0, selectedLabels: ['Option 0'] },
    ])
    expect(answerMock.calls).toHaveLength(1)

    // 2. Stage the matching tool_result via WS — pendingAuq transitions to
    //    null and the watcher clears the inflight debounce key.
    mock.emit({
      type: 'message',
      sequence: 1,
      kind: 'tool_result',
      content: {
        tool_use_id: 'toolu_a',
        output: 'ok',
        is_error: false,
      },
      raw_text: null,
      created_at: '2026-05-28T12:00:01Z',
    })
    // Read the computed once to push it through; then await nextTick so the
    // watcher fires and clears `inflightAuqToolUseId`.
    expect(composable.pendingAuq.value).toBeNull()
    await nextTick()

    // 3. A new AUQ arrives — submitAuqAnswer should fire (debounce key
    //    was cleared by the transition-to-null watcher).
    mock.emit({
      type: 'message',
      sequence: 2,
      kind: 'tool_use',
      content: {
        name: 'AskUserQuestion',
        input: { questions: qB },
        tool_use_id: 'toolu_b',
      },
      raw_text: null,
      created_at: '2026-05-28T12:00:02Z',
    })
    expect(composable.pendingAuq.value?.toolUseId).toBe('toolu_b')

    await composable.submitAuqAnswer([
      { questionIndex: 0, selectedLabels: ['Option 1'] },
    ])
    expect(answerMock.calls).toHaveLength(2)
    expect(answerMock.calls[1]?.questionId).toBe('toolu_b')
    expect(answerMock.calls[1]?.answers).toEqual([
      { questionIndex: 0, selectedLabels: ['Option 1'] },
    ])
  })
})

// ---------------------------------------------------------------------------
// Answer-endpoint test stubs with call capture.
// ---------------------------------------------------------------------------

function mockAnswerQuestion(): {
  fn: (id: string, questionId: string, answers: AuqAnswer[]) => Promise<void>
  calls: Array<{ id: string; questionId: string; answers: AuqAnswer[] }>
} {
  const calls: Array<{ id: string; questionId: string; answers: AuqAnswer[] }> = []
  const fn = mock(async (id: string, questionId: string, answers: AuqAnswer[]) => {
    calls.push({ id, questionId, answers })
  })
  return { fn, calls }
}

function mockCancelQuestion(): {
  fn: (id: string, questionId: string) => Promise<void>
  calls: Array<{ id: string; questionId: string }>
} {
  const calls: Array<{ id: string; questionId: string }> = []
  const fn = mock(async (id: string, questionId: string) => {
    calls.push({ id, questionId })
  })
  return { fn, calls }
}
