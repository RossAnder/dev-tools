// Sprint-agent-stream composable — module-singleton state following the
// SELECTED sprint's PTY sessions: a `?sprint_id=`-filtered session list, a
// live one-line summary feed from the latest session's WS fan-out, and an
// on-demand transcript fetch for the modal.
//
// T19 of the read-only sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md, Wave 3). Mirrors the shape of
// `usePtySession.ts`: module-level refs declared once, swappable API adapter
// via `__setApiForTests`, `__resetForTests` to clear singleton state between
// bun tests. NOT Pinia, NOT provide/inject — every caller of
// `useSprintAgentStream()` sees the same refs. T20 (`SprintAgentStream.vue`)
// consumes `bind`/`summaryItems`/`openTranscript`.
//
// ## v1 session-LIST refresh (no poll loop)
//
// PTY session creation emits NO `events` row, so the Wave-1 notify-bus
// carries no signal for a newly-spawned session — the initial
// `listSessions({sprint_id})` fetch can go stale mid-run. Instead of a poll
// loop (no polling precedent in this codebase), the module observes the bound
// sprint's quiescence topic via `useSprintTelemetry` and re-runs
// `loadForSprint` on each snapshot change: claim/complete writes DO flow
// through the bus and correlate with session spawns. A session spawned with
// no concurrent quiescence change appears only on the next refresh — an
// accepted v1 latency, not a missed-message bug (see the plan's Risks note).
//
// ## Token cancellation
//
// `loadForSprint` is re-entrant (quiescence frames can arrive while a fetch
// is inflight). Pattern lifted from `usePtySession.ts` / `useHierarchy.ts`:
// bump a counter on entry, capture the local token, and on resolution
// short-circuit if the counter has moved.

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
import type {
  PtyMessage,
  PtyMessageKind,
  PtySession,
  SessionStream,
  WsFrame,
} from '../api/pty'
import { useSprintTelemetry } from './useSprintTelemetry'

// ---------------------------------------------------------------------------
// Narrow kind validator — duplicated from `usePtySession.ts` (where it is
// module-private by design): the WS frame schema leaves the inbound
// `message.kind` as `z.string()` for forward-compat, while the row schema
// tightens it to the six-value enum. Frames whose kind isn't recognised are
// silently dropped at the fold boundary.
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
// Summary view type.
// ---------------------------------------------------------------------------

/** Max characters of a one-line summary's text, INCLUDING the ellipsis. */
export const SUMMARY_MAX_CHARS = 80

/**
 * One-line rendering of a live `message` frame: kind badge + truncated
 * content + timestamp. `id` is the synthetic `<session>-<sequence>` composite
 * (stable list key); `text` is whitespace-collapsed and truncated to
 * {@link SUMMARY_MAX_CHARS} with a trailing ellipsis.
 */
export interface SummaryItem {
  id: string
  session_id: string
  kind: PtyMessageKind
  text: string
  created_at: string
}

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

/** The sprint id the agent stream is currently bound to, or `null`. */
const boundSprintId: Ref<string | null> = ref(null)

/** The bound sprint's PTY sessions (server-filtered via `?sprint_id=`). */
const sessions: Ref<PtySession[]> = ref([])

/**
 * Live `message` frames folded from the LATEST session's WS stream. Held as
 * a `shallowRef` because rows are appended via fresh-array assignment —
 * deep reactivity on individual rows would be wasted work.
 */
const liveMessages: Ref<PtyMessage[]> = shallowRef([])

const status: Ref<'idle' | 'loading' | 'open' | 'closed' | 'error'> =
  ref('idle')

const error: Ref<string | null> = ref(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  listSessions: typeof productionApi.listSessions
  getMessages: typeof productionApi.getMessages
  openSessionStream: typeof productionApi.openSessionStream
  launchSprint: typeof productionApi.launchSprint
}
let api: Api = {
  listSessions: productionApi.listSessions,
  getMessages: productionApi.getMessages,
  openSessionStream: productionApi.openSessionStream,
  launchSprint: productionApi.launchSprint,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  // Close any live stream before tearing down — leaving an open WebSocket
  // attached to the module singleton would leak across test boundaries.
  teardownStream()
  telemetry.disconnect()
  boundSprintId.value = null
  sessions.value = []
  liveMessages.value = []
  status.value = 'idle'
  error.value = null
  loadToken = 0
  launchedSession = null
  api = {
    listSessions: productionApi.listSessions,
    getMessages: productionApi.getMessages,
    openSessionStream: productionApi.openSessionStream,
    launchSprint: productionApi.launchSprint,
  }
}

// ---------------------------------------------------------------------------
// Internal mutable state — NOT exported as refs because they hold a non-
// reactive handle (the stream) or a monotonically increasing token.
// ---------------------------------------------------------------------------

let stream: SessionStream | null = null

/** The session id the live stream is currently attached to, when any. */
let streamSessionId: string | null = null

/** Request-id token for {@link loadForSprint}. See the file-header comment. */
let loadToken = 0

/**
 * The session optimistically prepended by {@link useSprintAgentStream.launch},
 * pinned until the server's session list catches up.
 *
 * PTY session creation emits NO `events` row (see the file-header comment), so
 * a launched session is invisible to the quiescence-driven `loadForSprint`
 * refresh until claim/complete activity correlates it onto the sprint's
 * `?sprint_id=` filter. Without this pin a refresh firing in that window — or a
 * transient stream reconnect that triggers one — would drop the just-launched
 * run from the list and tear its stream down, rendering the live run as gone.
 * `loadForSprint` therefore RE-MERGES this row (and keeps it the latest, so the
 * stream stays attached) until a fetched list actually contains it, at which
 * point the real row supersedes the optimistic one and the pin clears.
 */
let launchedSession: PtySession | null = null

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

function teardownStream(): void {
  if (stream) {
    try {
      stream.close()
    } catch {
      // Ignore — the test seam's mock stream may not implement close().
    }
    stream = null
  }
  streamSessionId = null
}

/**
 * Pick the LATEST session — newest `started_at` (ISO-8601 strings compare
 * lexicographically); on a tie the earlier list entry wins.
 */
function pickLatest(rows: PtySession[]): PtySession | null {
  let latest: PtySession | null = null
  for (const row of rows) {
    if (latest === null || row.started_at > latest.started_at) {
      latest = row
    }
  }
  return latest
}

/**
 * Synthesize a `PtyMessage` row from an inbound `message` WS frame —
 * duplicated from `usePtySession.ts` (module-private there): `session_id`
 * from the streamed session, `id` from a `<session>-<sequence>` composite,
 * `content_json` as the JSON-string projection of the frame's `content`.
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
 * Best-effort one-line text for a row: the structured `text` field
 * (assistant_text / user_input), else the tool `name` (tool_use), else a
 * string `output` (tool_result), else `raw_text`, else the raw content JSON.
 */
function summaryText(row: PtyMessage): string {
  let parsed: unknown = null
  try {
    parsed = JSON.parse(row.content_json)
  } catch {
    // Fall through to raw_text / content_json below.
  }
  if (parsed !== null && typeof parsed === 'object') {
    const obj = parsed as Record<string, unknown>
    if (typeof obj['text'] === 'string') return obj['text']
    if (typeof obj['name'] === 'string') return obj['name']
    if (typeof obj['output'] === 'string') return obj['output']
  }
  return row.raw_text ?? row.content_json
}

/** Collapse whitespace runs and truncate to {@link SUMMARY_MAX_CHARS}. */
function toOneLine(text: string): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  if (flat.length <= SUMMARY_MAX_CHARS) return flat
  return `${flat.slice(0, SUMMARY_MAX_CHARS - 1)}…`
}

/** Derived one-line summaries — one per folded live `message` frame. */
const summaryItems: Ref<SummaryItem[]> = computed(() =>
  liveMessages.value.map((row) => ({
    id: row.id,
    session_id: row.session_id,
    kind: row.kind,
    text: toOneLine(summaryText(row)),
    created_at: row.created_at,
  })),
)

// ---------------------------------------------------------------------------
// v1 list-refresh trigger — the bound sprint's quiescence topic.
//
// One module-level telemetry binding follows `boundSprintId` (null id = no
// topic = idle; a sprint switch re-subscribes via useSprintTelemetry's own
// reactive-getter plumbing). Each snapshot CHANGE re-runs `loadForSprint`:
// claim/complete writes ride the notify-bus and correlate with session
// spawns, so this refreshes the session list without a poll loop. The
// `flush: 'sync'` mirrors useResourceStream's topic watch and keeps the bun
// tests deterministic (a pushed frame triggers the refetch synchronously).
// ---------------------------------------------------------------------------

const telemetry = useSprintTelemetry(() => boundSprintId.value)

watch(
  telemetry.quiescence,
  (snapshot) => {
    if (snapshot === null) return
    const sprintId = boundSprintId.value
    if (sprintId === null) return
    void loadForSprint(sprintId)
  },
  { flush: 'sync' },
)

// ---------------------------------------------------------------------------
// Loaders (module-scope so the quiescence watcher above can call them).
// ---------------------------------------------------------------------------

/**
 * Refresh the bound sprint's session list and (re)attach the live stream to
 * the LATEST session. Re-entrant under token cancellation; a refresh whose
 * latest session is UNCHANGED keeps the existing stream open (no WS bounce,
 * no folded-frame loss) — this is the hot path for quiescence-driven
 * refreshes.
 */
async function loadForSprint(sprintId: string): Promise<void> {
  const token = ++loadToken
  status.value = 'loading'
  error.value = null

  let fetched: PtySession[]
  try {
    fetched = await api.listSessions({ sprint_id: sprintId })
  } catch (e) {
    if (token !== loadToken) return
    error.value = toMessage(e)
    status.value = 'error'
    return
  }

  // A faster sibling load may have superseded us while the fetch was
  // inflight. Discard this stale result.
  if (token !== loadToken) return

  // Reconcile the optimistic launch pin (see `launchedSession`): if a launch
  // is pinned and the server list does NOT yet carry it, re-merge it at the
  // front so the just-launched run survives this refresh; once the fetched
  // list DOES carry it, the real row supersedes the optimistic one and the
  // pin clears.
  let rows = fetched
  if (launchedSession !== null) {
    const pinned = launchedSession
    if (fetched.some((s) => s.id === pinned.id)) {
      launchedSession = null
    } else {
      rows = [pinned, ...fetched]
    }
  }
  sessions.value = rows

  const latest = pickLatest(rows)
  if (latest === null) {
    // No sessions for this sprint (yet) — the quiescence-driven refresh
    // re-runs us when correlated activity lands.
    teardownStream()
    liveMessages.value = []
    status.value = 'idle'
    return
  }

  if (stream !== null && streamSessionId === latest.id) {
    // Already streaming the latest session — a list refresh must not bounce
    // the socket or clear the folded frames.
    status.value = 'open'
    return
  }

  attachStream(latest.id)
}

/**
 * Tear down any existing stream and attach a fresh one to `sessionId`,
 * folding inbound `message` frames into {@link liveMessages}. Sets
 * `status='open'` on success, `status='error'` if the open throws. Shared by
 * {@link loadForSprint} (latest-session attach) and the launch path (immediate
 * attach to a just-spawned session that the server list does not yet carry).
 *
 * `openSessionStream` itself owns transport-level RECONNECT (exponential
 * back-off on an unexpected socket close — see `../api/pty.ts`); a reconnect
 * re-subscribes the same `sessionId` under the existing handler registry, so
 * folded frames resume without re-attaching here. This helper is the
 * session-SWITCH path, not the socket-drop path.
 */
function attachStream(sessionId: string): void {
  teardownStream()
  liveMessages.value = []

  try {
    const fresh = api.openSessionStream(sessionId)
    stream = fresh
    streamSessionId = sessionId

    fresh.on('message', (frame) => {
      if (frame.type !== 'message') return
      // The stream may have been superseded by the time a frame arrives —
      // only fold while this session is still the streamed one.
      if (streamSessionId !== sessionId) return
      const row = messageFromFrame(sessionId, frame)
      // Drop frames whose `kind` isn't one of the six known JSONL-tail
      // values — the wire schema is intentionally wider than the row
      // schema for forward-compat, so unknown kinds are not an error.
      if (row === null) return
      liveMessages.value = [...liveMessages.value, row]
    })

    fresh.on('skipped', (frame) => {
      if (frame.type !== 'skipped') return
      error.value = `output skipped (${frame.bytes} bytes): ${frame.reason}`
    })

    fresh.on('error', (frame) => {
      if (frame.type !== 'error') return
      error.value = frame.message
    })

    status.value = 'open'
  } catch (e) {
    error.value = toMessage(e)
    status.value = 'error'
  }
}

/** Safe `onScopeDispose` — no-ops outside a Vue effect scope. */
function safeOnScopeDispose(fn: () => void): void {
  if (getCurrentScope()) {
    onScopeDispose(fn)
  }
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useSprintAgentStream() {
  /**
   * Bind the agent stream to a sprint: connect the quiescence-driven
   * refresh trigger and load the sprint's sessions. A sprint SWITCH drops
   * the previous sprint's view (stream, sessions, folded frames) before
   * refetching; the telemetry topic re-subscribes via its reactive getter.
   */
  async function bind(sprintId: string): Promise<void> {
    if (boundSprintId.value !== sprintId) {
      teardownStream()
      sessions.value = []
      liveMessages.value = []
      // A launch pin belongs to the sprint it was launched for; drop it on a
      // sprint switch so a stale optimistic row never leaks into another
      // sprint's view.
      launchedSession = null
      boundSprintId.value = sprintId
    }
    telemetry.connect()
    await loadForSprint(sprintId)
  }

  /**
   * Launch the bound sprint: spawn its orchestrator session via
   * `POST /api/sprints/{id}/run` and surface the run IMMEDIATELY.
   *
   * PTY session creation emits no notify-bus event, so the quiescence-driven
   * refresh won't auto-show the spawned session; this therefore (1) prepends
   * the returned session to {@link sessions} optimistically, (2) pins it via
   * {@link launchedSession} so a refresh in the catch-up window keeps it, and
   * (3) attaches the live WS stream to it directly so its output streams from
   * the first frame. Returns the spawned session, or `null` on failure (with
   * `error.value` set) — mirroring `usePtySessions.spawn`'s null-on-failure.
   *
   * Idempotency note: pass the SELECTED sprint id; the server owns
   * already-running guards. The optimistic prepend de-dupes on id so a
   * subsequent server refresh that returns the same row does not double it.
   */
  async function launch(sprintId: string): Promise<PtySession | null> {
    error.value = null
    let created: PtySession
    try {
      created = await api.launchSprint(sprintId)
    } catch (e) {
      error.value = toMessage(e)
      return null
    }

    // Pin + optimistically prepend (de-duping on id), then attach the live
    // stream to the freshly-spawned session so it streams from the start.
    launchedSession = created
    sessions.value = [created, ...sessions.value.filter((s) => s.id !== created.id)]
    attachStream(created.id)
    return created
  }

  /**
   * Fetch a session's FULL stored transcript for the modal (T20 renders the
   * rows via `PtyMessage.vue`). Returns `null` on failure (with
   * `error.value` set) — mirrors `usePtySessions.spawn`'s null-on-failure.
   */
  async function openTranscript(sessionId: string): Promise<PtyMessage[] | null> {
    try {
      return await api.getMessages(sessionId)
    } catch (e) {
      error.value = toMessage(e)
      return null
    }
  }

  /**
   * Tear down the live stream and the telemetry subscription. Sets
   * `status='closed'`. Safe to call when nothing is bound.
   */
  function disconnect(): void {
    teardownStream()
    telemetry.disconnect()
    boundSprintId.value = null
    launchedSession = null
    status.value = 'closed'
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
    boundSprintId,
    sessions,
    liveMessages,
    summaryItems,
    status,
    error,
    bind,
    launch,
    loadForSprint,
    openTranscript,
    disconnect,
    clearError,
  }
}
