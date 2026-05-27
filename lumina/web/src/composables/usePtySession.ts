// Focused-PTY-session composable — module-singleton state + live transcript
// over the WebSocket fan-out exposed at `/api/pty/sessions/{id}/ws`.
//
// Mirrors the shape of the other round-4 composables (useScalars, useTaskSpec,
// useDispatchPlan, usePtySessions): module-level refs declared once, swappable
// API adapter via `__setApiForTests`, `__resetForTests` to clear singleton
// state between bun tests. NOT Pinia, NOT provide/inject — every caller of
// `usePtySession()` sees the same refs.
//
// Sibling composable `usePtySessions` owns the catalogue of all sessions the
// user can see; this one owns the currently-focused session's live transcript.
//
// ## Token cancellation
//
// `select(id)` performs a history fetch then opens the WS stream. If the user
// rapidly switches focus between sessions, a slow history-fetch from the
// previous call must NOT clobber the fresher session's state. Pattern lifted
// verbatim from `useHierarchy.ts:89-115` — bump a counter on entry, capture
// the local token, and on resolution short-circuit if the counter has moved.
//
// ## Frame-to-message conversion
//
// A `message`-typed WS frame carries `{sequence, kind, content, raw_text,
// created_at}` inline (no `id`/`session_id`) — the frame IS the message in
// transit. The `PtyMessage` row type does require both, so we synthesize them
// at the boundary: `session_id` from `currentId.value` and `id` from a
// `<session>-<sequence>` composite. `content` is `unknown` on the WS frame
// but `content_json` is a JSON string on the `PtyMessage` row, so we
// `JSON.stringify` on conversion.

import {
  ref,
  shallowRef,
  onScopeDispose,
  getCurrentScope,
  type Ref,
} from 'vue'

import * as productionApi from '../api/pty'
import type {
  PtyMessage,
  PtySession,
  SessionStream,
  WsFrame,
} from '../api/pty'

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

/** The id of the session the SPA is currently subscribed to, or `null`. */
const currentId: Ref<string | null> = ref(null)

/**
 * The transcript for the focused session.  Held as a `shallowRef` because
 * messages are appended via fresh-array assignment (`messages.value = [...]`)
 * — deep reactivity on individual rows would be wasted work.
 */
const messages: Ref<PtyMessage[]> = shallowRef([])

/**
 * WebSocket connection status — independent of the server-reported session
 * `status` field (which can transition `spawning|active|idle|awaiting|...`
 * over the lifetime of one open connection).  Tracks the socket itself.
 */
const wsStatus: Ref<'idle' | 'connecting' | 'open' | 'closed' | 'error'> =
  ref('idle')

const error: Ref<string | null> = ref(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  getSession: typeof productionApi.getSession
  getMessages: typeof productionApi.getMessages
  sendInputsBatch: typeof productionApi.sendInputsBatch
  openSessionStream: typeof productionApi.openSessionStream
}
let api: Api = {
  getSession: productionApi.getSession,
  getMessages: productionApi.getMessages,
  sendInputsBatch: productionApi.sendInputsBatch,
  openSessionStream: productionApi.openSessionStream,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  // Close any live stream before tearing down — leaving an open WebSocket
  // attached to the module singleton would leak across test boundaries.
  if (stream) {
    try {
      stream.close()
    } catch {
      // Ignore — the test seam's mock stream may not implement close().
    }
    stream = null
  }
  currentId.value = null
  messages.value = []
  wsStatus.value = 'idle'
  error.value = null
  loadToken = 0
  api = {
    getSession: productionApi.getSession,
    getMessages: productionApi.getMessages,
    sendInputsBatch: productionApi.sendInputsBatch,
    openSessionStream: productionApi.openSessionStream,
  }
}

// ---------------------------------------------------------------------------
// Internal mutable state — NOT exported as refs because they hold a non-
// reactive handle (the stream) or a monotonically increasing token.
// ---------------------------------------------------------------------------

let stream: SessionStream | null = null

/**
 * Request-id token for {@link select}.  Each entry bumps the counter; the
 * async history fetch checks the captured token on resolution and bails if
 * the user has since switched to a different session.  Pattern lifted from
 * `useHierarchy.ts:89-115`.
 */
let loadToken = 0

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/**
 * Synthesize a `PtyMessage` row from an inbound `message` WS frame.
 *
 * The WS frame carries `{sequence, kind, content, raw_text, created_at}`
 * inline — see `domain::FrameOut::Message` and the `WsFrameSchema` in
 * `../api/pty.ts`. The `PtyMessage` row also requires `id` and `session_id`,
 * which we fill from the focused session id + a composite (no DB id is
 * available on the wire because the row hasn't necessarily been persisted
 * yet — the broadcast happens off the live PTY before the row hits SQLite).
 *
 * `content_json` is the JSON-string projection of the frame's `content`
 * (which is `unknown` on the schema side).
 */
function messageFromFrame(
  sessionId: string,
  frame: Extract<WsFrame, { type: 'message' }>,
): PtyMessage {
  return {
    id: `${sessionId}-${frame.sequence}`,
    session_id: sessionId,
    sequence: frame.sequence,
    created_at: frame.created_at,
    kind: frame.kind,
    content_json: JSON.stringify(frame.content),
    raw_text: frame.raw_text,
  }
}

/** Safe `onScopeDispose` — no-ops outside a Vue effect scope. */
function safeOnScopeDispose(fn: () => void): void {
  // `getCurrentScope()` returns `undefined` outside a setup scope (the bun
  // tests run plain TS with no setup wrapper).  Calling `onScopeDispose`
  // there would emit a `[Vue warn]` and the cleanup would never fire anyway.
  if (getCurrentScope()) {
    onScopeDispose(fn)
  }
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function usePtySession() {
  /**
   * Focus a session and start streaming its transcript.  Disconnects any
   * previously focused session's stream first, then re-fetches the
   * persisted history and opens the WS for live appends.
   *
   * Token cancellation: if the user switches to a different session
   * mid-history-fetch, the slower fetch's result is discarded.  See the
   * file-header comment.
   */
  async function select(id: string): Promise<void> {
    const token = ++loadToken

    // Tear down any existing stream before reassigning currentId.
    if (stream) {
      try {
        stream.close()
      } catch {
        // ignore
      }
      stream = null
    }

    messages.value = []
    error.value = null
    currentId.value = id
    wsStatus.value = 'connecting'

    let history: PtyMessage[]
    try {
      history = await api.getMessages(id)
    } catch (e) {
      if (token !== loadToken) return
      error.value = toMessage(e)
      wsStatus.value = 'error'
      return
    }

    // A faster sibling `select(otherId)` may have superseded us while the
    // history fetch was inflight.  Discard this stale result.
    if (token !== loadToken) return

    messages.value = history

    // Open the live stream.  `openSessionStream` returns immediately with a
    // handle whose `send` is buffered until the socket reaches OPEN — there
    // is no awaitable "connected" promise.  We optimistically set
    // wsStatus='open' after registering handlers; the test seam swaps in a
    // synchronous mock so this is the cheapest reasonable signal.
    try {
      const fresh = api.openSessionStream(id)
      stream = fresh

      fresh.on('message', (frame) => {
        if (frame.type !== 'message') return
        // currentId may have shifted by the time a message arrives — only
        // append while this session is still focused.
        if (currentId.value !== id) return
        messages.value = [...messages.value, messageFromFrame(id, frame)]
      })

      fresh.on('status', () => {
        // The session-level status (`spawning|active|idle|...`) is logged
        // server-side but is not propagated to wsStatus here — wsStatus
        // tracks the socket, not the session.  Components that want the
        // session status should subscribe to the session row via
        // `usePtySessions().sessions`.
      })

      fresh.on('skipped', (frame) => {
        if (frame.type !== 'skipped') return
        error.value = `output skipped (${frame.bytes} bytes): ${frame.reason}`
      })

      fresh.on('error', (frame) => {
        if (frame.type !== 'error') return
        error.value = frame.message
      })

      wsStatus.value = 'open'
    } catch (e) {
      error.value = toMessage(e)
      wsStatus.value = 'error'
    }
  }

  /**
   * Submit a single prompt over the WebSocket as an `input` frame with
   * `kind: 'prompt'`. The REST batch route would also work (and is
   * preferred for multi-frame atomicity — see {@link submitBatch}); WS
   * `input` frames are the lower-latency single-frame path.
   *
   * No-ops if no stream is open.
   */
  async function submit(text: string): Promise<void> {
    if (!stream) {
      throw new Error('cannot submit: no active PTY session stream')
    }
    stream.send({ type: 'input', kind: 'prompt', payload: text })
  }

  /**
   * Submit N prompts as an atomic batch via the REST endpoint
   * `POST /api/pty/sessions/{id}/inputs/batch`. The server enqueues all
   * frames or none — preferred over chained {@link submit} calls when the
   * caller needs all-or-nothing semantics.
   */
  async function submitBatch(texts: string[]): Promise<void> {
    const id = currentId.value
    if (id === null) {
      throw new Error('cannot submitBatch: no focused PTY session')
    }
    await api.sendInputsBatch(
      id,
      texts.map((payload) => ({ kind: 'prompt', payload })),
    )
  }

  /**
   * Send a cancel signal to the running PTY via the WS `input` frame with
   * `kind: 'cancel'`. This is the in-band cancellation — distinct from the
   * REST `DELETE /api/pty/sessions/{id}` which tombstones the row in v1.
   * No-op if no stream is open.
   */
  async function cancel(): Promise<void> {
    if (!stream) return
    stream.send({ type: 'input', kind: 'cancel', payload: '' })
  }

  /**
   * Gracefully close the current WS connection (code 1000). Sets
   * `wsStatus='closed'`. Safe to call when no stream is open.
   */
  function disconnect(): void {
    if (stream) {
      try {
        stream.close()
      } catch {
        // ignore
      }
      stream = null
    }
    wsStatus.value = 'closed'
  }

  /** Clear `error.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
  }

  // Auto-disconnect when the consuming scope tears down. No-op outside a
  // setup scope (the bun tests have no scope), so the test path manages the
  // lifecycle explicitly via `__resetForTests`.
  safeOnScopeDispose(() => disconnect())

  return {
    currentId,
    messages,
    status: wsStatus,
    error,
    select,
    submit,
    submitBatch,
    cancel,
    disconnect,
    clearError,
  }
}

// Re-export the PtySession type for components that already import the
// composable and want the row shape alongside.
export type { PtySession }
