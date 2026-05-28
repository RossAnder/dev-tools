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
  computed,
  watch,
  onScopeDispose,
  getCurrentScope,
  type Ref,
} from 'vue'

import * as productionApi from '../api/pty'
import {
  isAuqToolUse,
  type AuqAnswer,
  type AuqInput,
  type AuqQuestion,
  type InputFrame,
  type PtyMessage,
  type PtyMessageKind,
  type PtySession,
  type SessionStream,
  type ToolUseContent,
  type WsFrame,
} from '../api/pty'
import { computeAuqKeystrokes } from './auqKeystrokes'

// ---------------------------------------------------------------------------
// Narrow kind validator — the WS frame schema (`WsFrameSchema` in api/pty.ts)
// leaves the inbound `message.kind` as `z.string()` for forward-compat, while
// the row schema (`PtyMessageSchema`) tightens it to a six-value enum after
// the JSONL-tail pass. Per the file-header "Frame-to-message conversion"
// comment, we synthesize the row at the boundary — that means we must
// validate the wider wire `kind` down to the narrower row `kind` here, and
// silently drop frames whose kind isn't recognised (the next post-handshake
// build would have surfaced the gap anyway).
//
// Kept in sync with `PTY_MESSAGE_KIND_VALUES` in api/pty.ts; that constant
// stays unexported because it's a wire-schema implementation detail, so we
// duplicate the six members here (the only place outside api/pty.ts that
// needs to discriminate them at runtime).
// ---------------------------------------------------------------------------

const KNOWN_PTY_MESSAGE_KINDS = new Set<string>([
  'user_input',
  'assistant_text',
  'tool_use',
  'tool_result',
  'system',
  'error',
])

function asPtyMessageKind(kind: string): PtyMessageKind | null {
  return KNOWN_PTY_MESSAGE_KINDS.has(kind) ? (kind as PtyMessageKind) : null
}

// ---------------------------------------------------------------------------
// Renderable view type — `PtyMessage` plus an optional `matchedResult` field
// that the `pairedMessages` derived view attaches to `tool_use` rows whose
// matching `tool_result` was found in the transcript.
//
// Cardinality:
//   - For non-`tool_use` rows: `matchedResult` is always `undefined`.
//   - For `tool_use` rows: `matchedResult` is either the `PtyMessage` row of
//     the matching `tool_result`, or `undefined` if no match was found in the
//     current transcript (e.g. the result has not yet arrived, or the JSONL
//     tail dropped it).
//   - Orphan `tool_result` rows (no matching parent `tool_use`) are emitted
//     standalone with `matchedResult: undefined`; the renderer is expected to
//     surface a "no matched call" badge on them.
//
// Designed as an interface extension (rather than a discriminated union on
// `kind`) so that the existing `PtyMessage` consumers — `PtyConsole.vue`
// passes `messages: PtyMessage[]` straight into `<PtyMessage :message="m" />`
// — remain structurally compatible: any `PtyMessage` is a `RenderableMessage`
// with `matchedResult` left undefined.
// ---------------------------------------------------------------------------

export interface RenderableMessage extends PtyMessage {
  /**
   * Populated only for `tool_use` rows that the `pairedMessages` view managed
   * to match against a later `tool_result` in the same transcript. Undefined
   * for non-`tool_use` rows AND for `tool_use` rows whose result has not yet
   * arrived (or was dropped).
   */
  matchedResult?: PtyMessage
}

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

/**
 * Lifecycle status of the focused session — server-reported via the WS
 * `status` frame on every transition. Distinct from `wsStatus` (which
 * tracks the socket itself). `null` until either the history fetch
 * resolves (we synthesise from the session row on select) or the first
 * `status` frame lands.
 */
const sessionStatus: Ref<string | null> = ref(null)

const error: Ref<string | null> = ref(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  getSession: typeof productionApi.getSession
  getMessages: typeof productionApi.getMessages
  sendInputsBatch: typeof productionApi.sendInputsBatch
  sendKeystrokes: typeof productionApi.sendKeystrokes
  openSessionStream: typeof productionApi.openSessionStream
}
let api: Api = {
  getSession: productionApi.getSession,
  getMessages: productionApi.getMessages,
  sendInputsBatch: productionApi.sendInputsBatch,
  sendKeystrokes: productionApi.sendKeystrokes,
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
  sessionStatus.value = null
  error.value = null
  inflightAuqToolUseId.value = null
  loadToken = 0
  api = {
    getSession: productionApi.getSession,
    getMessages: productionApi.getMessages,
    sendInputsBatch: productionApi.sendInputsBatch,
    sendKeystrokes: productionApi.sendKeystrokes,
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
): PtyMessage | null {
  const narrowedKind = asPtyMessageKind(frame.kind)
  if (narrowedKind === null) return null
  return {
    id: `${sessionId}-${frame.sequence}`,
    session_id: sessionId,
    sequence: frame.sequence,
    created_at: frame.created_at,
    kind: narrowedKind,
    content_json: JSON.stringify(frame.content),
    raw_text: frame.raw_text,
  }
}

/**
 * Extract the `tool_use_id` from a `PtyMessage` row's `content_json`. Returns
 * `null` if the JSON is malformed, the parsed value isn't a JSON object, or
 * the `tool_use_id` field is absent / not a string. The Rust-side
 * `TypedMessage::ToolUse` / `TypedMessage::ToolResult` serialisation
 * (`lumina/src/pty/jsonl_tail.rs::map_record_to_typed`) embeds `tool_use_id`
 * inside the `content` JSON on both kinds; the field is NOT promoted to a
 * row-level column on `pty_messages`, so the wire schema in `api/pty.ts` does
 * not expose it either — pairing has to JSON-parse.
 */
function extractToolUseId(row: PtyMessage): string | null {
  try {
    const parsed: unknown = JSON.parse(row.content_json)
    if (parsed === null || typeof parsed !== 'object') return null
    const id = (parsed as Record<string, unknown>)['tool_use_id']
    return typeof id === 'string' ? id : null
  } catch {
    return null
  }
}

/**
 * Two-pass pairing over the transcript:
 *
 *   1. Build a `Map<tool_use_id, PtyMessage>` from all `tool_result` rows.
 *   2. Walk rows in order; for each `tool_use` row attach the matching result
 *      (if any) as `matchedResult` and mark it consumed so step-3 omits it.
 *      For each `tool_result` row, drop it if it was just consumed; otherwise
 *      emit standalone as an orphan with `matchedResult` undefined. Every
 *      other kind passes through verbatim.
 *
 * Worked example:
 *   in:  [assistant_text, tool_use(x), tool_result(x)]
 *   out: [assistant_text, tool_use(x)+matchedResult]
 */
const pairedMessages: Ref<RenderableMessage[]> = computed(() => {
  const rows = messages.value

  // Pass 1: index tool_result rows by tool_use_id.
  const resultsByToolUseId = new Map<string, PtyMessage>()
  for (const row of rows) {
    if (row.kind !== 'tool_result') continue
    const id = extractToolUseId(row)
    if (id === null) continue
    // First-write-wins: if a duplicate tool_use_id appears, keep the earliest.
    // The Rust side guarantees uniqueness per (session, tool_use_id) in
    // practice; defensive guard kept for malformed history.
    if (!resultsByToolUseId.has(id)) {
      resultsByToolUseId.set(id, row)
    }
  }

  // Pass 2: walk rows, consuming matched results.
  const consumedIds = new Set<string>()
  const out: RenderableMessage[] = []
  for (const row of rows) {
    if (row.kind === 'tool_use') {
      const id = extractToolUseId(row)
      const matched = id !== null ? resultsByToolUseId.get(id) : undefined
      if (id !== null && matched !== undefined) {
        consumedIds.add(id)
      }
      out.push({ ...row, matchedResult: matched })
      continue
    }
    if (row.kind === 'tool_result') {
      const id = extractToolUseId(row)
      // If this exact result was attached to a parent tool_use above, omit it
      // from the top-level output (it's already represented inside the parent
      // card). Otherwise it's an orphan and renders standalone.
      if (id !== null && consumedIds.has(id) && resultsByToolUseId.get(id) === row) {
        continue
      }
      out.push({ ...row })
      continue
    }
    out.push({ ...row })
  }
  return out
})

// ---------------------------------------------------------------------------
// AskUserQuestion (AUQ) picker plumbing — T9 of the
// `lumina-interactive-prompts` plan.
//
// `pendingAuq` derives the currently-unanswered AUQ tool_use (if any) from
// `pairedMessages`. The picker SFC binds against this; while non-null, the
// prompt textarea is disabled and the picker renders inline. When claude
// emits the matching tool_result the row's `matchedResult` populates,
// `pairedMessages` re-derives, and `pendingAuq` transitions to null.
//
// `submitAuqAnswer` + `cancelAuq` post keystroke frames directly to
// `/api/pty/sessions/{id}/keystrokes` (queue-bypass; see plan §Supervisor
// bypass). Both are debounced per `pendingAuq.toolUseId` — a double-click
// on Submit/Cancel within the same picker yields exactly one POST. The
// debounce key clears when `pendingAuq` transitions to null.
// ---------------------------------------------------------------------------

/**
 * Identifier of the AUQ whose answer is currently being submitted (or has
 * just been submitted). Used as the debounce key for `submitAuqAnswer` /
 * `cancelAuq`. Cleared when `pendingAuq` transitions to null (the matching
 * tool_result arrived) so a follow-on AUQ can be answered without manual
 * reset.
 */
const inflightAuqToolUseId: Ref<string | null> = ref(null)

/**
 * Shape returned by the `pendingAuq` computed.
 */
export interface PendingAuq {
  toolUseId: string
  questions: AuqQuestion[]
}

/**
 * Derived view: the first (oldest) unmatched AskUserQuestion `tool_use` in
 * the current transcript, or `null` if none. Emits a `console.warn` when
 * more than one unmatched AUQ is observed (stuck-conversation signal —
 * claude can only have one open AUQ at a time, so multiple unmatched rows
 * mean something earlier in the conversation went unanswered).
 *
 * Walks `pairedMessages` (not the raw `messages`) so the `matchedResult`
 * field already reflects tool_use→tool_result pairing — an unmatched AUQ
 * is exactly `m.kind === 'tool_use' && m.matchedResult === undefined`.
 *
 * The content is stored as a JSON STRING in `m.content_json`, not the
 * pre-parsed value the original plan spec assumed; this computed
 * JSON-parses each candidate row's content_json once, narrowing via the
 * `isAuqToolUse` predicate before reaching into `input.questions`.
 */
const pendingAuq: Ref<PendingAuq | null> = computed(() => {
  const rows = pairedMessages.value
  const unmatched: Array<{ toolUseId: string; questions: AuqQuestion[] }> = []

  for (const row of rows) {
    if (row.kind !== 'tool_use') continue
    if (row.matchedResult !== undefined) continue
    let parsed: unknown
    try {
      parsed = JSON.parse(row.content_json)
    } catch {
      continue
    }
    if (parsed === null || typeof parsed !== 'object') continue
    const candidate = parsed as Partial<ToolUseContent>
    if (
      typeof candidate.name !== 'string' ||
      typeof candidate.tool_use_id !== 'string' ||
      candidate.input === undefined
    ) {
      continue
    }
    const narrowable: ToolUseContent = {
      name: candidate.name,
      tool_use_id: candidate.tool_use_id,
      input: candidate.input,
    }
    if (!isAuqToolUse(narrowable)) continue
    const auqInput = narrowable.input as AuqInput
    if (!auqInput || !Array.isArray(auqInput.questions)) continue
    unmatched.push({
      toolUseId: narrowable.tool_use_id,
      questions: auqInput.questions,
    })
  }

  if (unmatched.length === 0) return null

  if (unmatched.length > 1) {
    // eslint-disable-next-line no-console -- stuck-conversation signal worth surfacing
    console.warn(
      `lumina: ${unmatched.length} unmatched AUQ detected — picker bound to oldest (toolUseId=${unmatched[0]!.toolUseId})`,
    )
  }

  const first = unmatched[0]!
  return { toolUseId: first.toolUseId, questions: first.questions }
})

/**
 * Clear the debounce key when the picker disappears. The watcher fires on
 * every transition to `null` — both first-time (no AUQ ever opened) and
 * post-answer (the matching tool_result arrived after a Submit). The
 * latter is the load-bearing case: with the key cleared, a follow-on AUQ
 * in the same session can be answered without manual reset.
 */
watch(pendingAuq, (next) => {
  if (next === null) {
    inflightAuqToolUseId.value = null
  }
})

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
    sessionStatus.value = null
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

    // Seed `sessionStatus` from the persisted row BEFORE the WS opens —
    // otherwise the pill stays null until the first status transition
    // arrives, which may be a long time (e.g., session is already Idle
    // and stays there). Honours the token-cancellation pattern: a faster
    // sibling `select` while this fetch is in flight discards the result.
    try {
      const row = await api.getSession(id)
      if (token !== loadToken) return
      sessionStatus.value = row.status
    } catch (e) {
      if (token !== loadToken) return
      error.value = toMessage(e)
    }

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
        const row = messageFromFrame(id, frame)
        // Drop frames whose `kind` isn't one of the six known JSONL-tail
        // values — the wire schema is intentionally wider than the row
        // schema for forward-compat, so unknown kinds are not an error.
        if (row === null) return
        messages.value = [...messages.value, row]
      })

      fresh.on('status', (frame) => {
        // Session-level lifecycle status — distinct from `wsStatus` (which
        // tracks the socket). Propagated into the module-singleton
        // `sessionStatus` ref so the SPA's status pill can react live to
        // server-side transitions (e.g., spawning → active → idle). The
        // `currentId.value !== id` guard mirrors the `message` handler:
        // late-arriving frames after the user switches sessions are dropped.
        if (frame.type !== 'status') return
        if (currentId.value !== id) return
        sessionStatus.value = frame.status
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

  /**
   * Submit the user's answers to the current pending AUQ. Translates the
   * answer array into a keystroke-frame sequence via `computeAuqKeystrokes`
   * and POSTs them to `/api/pty/sessions/{id}/keystrokes` (queue-bypass).
   *
   * Debounced per `pendingAuq.toolUseId`: a double-click on Submit while a
   * previous call for the same AUQ is in flight (or has completed but the
   * matching tool_result has not yet arrived) is silently dropped. The
   * debounce key clears when `pendingAuq` transitions to null — see the
   * watcher above.
   *
   * No-op when no AUQ is pending (defensive — the picker SFC shouldn't be
   * visible without `pendingAuq`) or no session is focused. On POST failure
   * the debounce key is cleared so the user can retry.
   */
  async function submitAuqAnswer(answers: AuqAnswer[]): Promise<void> {
    const current = pendingAuq.value
    if (current === null) return
    if (inflightAuqToolUseId.value === current.toolUseId) return
    const sid = currentId.value
    if (sid === null) return

    inflightAuqToolUseId.value = current.toolUseId

    try {
      const frames: InputFrame[] = computeAuqKeystrokes(current.questions, answers)
      await api.sendKeystrokes(sid, frames)
    } catch (e) {
      // Failed — clear the debounce key so the user can retry.
      inflightAuqToolUseId.value = null
      throw e
    }
    // Success path: leave inflightAuqToolUseId set until pendingAuq
    // transitions to null (the watch above clears it when claude emits the
    // matching tool_result and pairedMessages re-derives).
  }

  /**
   * Cancel the current pending AUQ by sending a single Escape keystroke.
   * Debounce semantics mirror `submitAuqAnswer` — a double-click on Cancel
   * yields exactly one POST.
   */
  async function cancelAuq(): Promise<void> {
    const current = pendingAuq.value
    if (current === null) return
    if (inflightAuqToolUseId.value === current.toolUseId) return
    const sid = currentId.value
    if (sid === null) return

    inflightAuqToolUseId.value = current.toolUseId

    try {
      const frames: InputFrame[] = [
        { type: 'input', kind: 'keystroke', payload: 'escape' },
      ]
      await api.sendKeystrokes(sid, frames)
    } catch (e) {
      inflightAuqToolUseId.value = null
      throw e
    }
  }

  // Auto-disconnect when the consuming scope tears down. No-op outside a
  // setup scope (the bun tests have no scope), so the test path manages the
  // lifecycle explicitly via `__resetForTests`.
  safeOnScopeDispose(() => disconnect())

  return {
    currentId,
    messages,
    pairedMessages,
    pendingAuq,
    status: wsStatus,
    sessionStatus,
    error,
    select,
    submit,
    submitBatch,
    cancel,
    disconnect,
    clearError,
    submitAuqAnswer,
    cancelAuq,
  }
}

// Re-export the PtySession type for components that already import the
// composable and want the row shape alongside.
export type { PtySession }
