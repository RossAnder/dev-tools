// PTY session wire schemas + fetch wrappers + WebSocket opener.
//
// Implements T12 of the lumina-pty-service plan
// (docs/plans/lumina-pty-service.md). This module owns the PtySession /
// PtyMessage / PtyQueueEntry row shapes, the input-frame / WS-frame
// discriminated-union schemas, and the REST + WS client surface over the ten
// routes mounted at `/api/pty/sessions/...` by T9.
//
// Route catalogue (source of truth: lumina/src/http/pty_sessions.rs):
//   GET    /api/pty/sessions                     → listSessions
//   POST   /api/pty/sessions                     → spawnSession
//   GET    /api/pty/sessions/{id}                → getSession
//   GET    /api/pty/sessions/{id}/messages       → getMessages
//   GET    /api/pty/sessions/{id}/queue          → getQueue
//   POST   /api/pty/sessions/{id}/input          → sendInput
//   POST   /api/pty/sessions/{id}/inputs/batch   → sendInputsBatch
//   PATCH  /api/pty/sessions/{id}                → updateSession (501 stub in v1)
//   DELETE /api/pty/sessions/{id}                → cancelSession / deleteSession
//   GET    /api/pty/sessions/{id}/ws             → openSessionStream
//
// Cancel and delete are the SAME route in v1 (`DELETE /api/pty/sessions/{id}`
// maps to the `cancel_session` handler that cancels the in-memory Session and
// tombstones the DB row). Both `cancelSession` and `deleteSession` are exported
// as aliases for discoverability; they call the same underlying fetch.
//
// nullability convention: Rust domain structs carry zero `skip_serializing_if`
// attributes, so `Option<T>` fields are always emitted as JSON `null` rather
// than being omitted. `.nullable()` is therefore correct on all optional fields.

import * as z from 'zod'

import { API_BASE, handle, handleVoid } from './http'

// ---------------------------------------------------------------------------
// Row schemas
// ---------------------------------------------------------------------------

/**
 * Mirrors `domain::PtySession` (migration 0008). `status` is one of
 * `spawning|active|idle|awaiting|completed|failed|cancelled` — free TEXT on
 * the Rust side; typed below as `PtySessionStatus`.
 */
const PTY_SESSION_STATUS_VALUES = [
  'spawning',
  'active',
  'idle',
  'awaiting',
  'completed',
  'failed',
  'cancelled',
] as const
export type PtySessionStatus = (typeof PTY_SESSION_STATUS_VALUES)[number]
const PtySessionStatusSchema = z.enum(PTY_SESSION_STATUS_VALUES)

export const PtySessionSchema = z.object({
  id: z.string(),
  label: z.string().nullable(),
  project_id: z.string().nullable(),
  cwd: z.string(),
  config_json: z.string(),
  parse_strategy_version: z.number(),
  status: PtySessionStatusSchema,
  started_at: z.string(),
  updated_at: z.string(),
  ended_at: z.string().nullable(),
  exit_code: z.number().nullable(),
  last_error: z.string().nullable(),
  previous_session_id: z.string().nullable(),
})
export type PtySession = z.infer<typeof PtySessionSchema>

/**
 * Mirrors `domain::PtyMessage` (migration 0008). `kind` is one of the six
 * JSONL-tail message kinds:
 * `user_input|assistant_text|tool_use|tool_result|system|error`.
 *
 * The pre-JSONL-tail vt100-parser pipeline emitted two additional kinds
 * (`prompt`, `parser_unknown`) which are now dead and not modelled here.
 */
const PTY_MESSAGE_KIND_VALUES = [
  'user_input',
  'assistant_text',
  'tool_use',
  'tool_result',
  'system',
  'error',
] as const
export type PtyMessageKind = (typeof PTY_MESSAGE_KIND_VALUES)[number]
const PtyMessageKindSchema = z.enum(PTY_MESSAGE_KIND_VALUES)

export const PtyMessageSchema = z.object({
  id: z.string(),
  session_id: z.string(),
  sequence: z.number(),
  created_at: z.string(),
  kind: PtyMessageKindSchema,
  content_json: z.string(),
  raw_text: z.string().nullable(),
})
export type PtyMessage = z.infer<typeof PtyMessageSchema>

/**
 * TS-only content shapes per `PtyMessageKind`. These are NOT zod-parsed — the
 * wire-side `PtyMessageSchema.content_json` stays a string and the
 * `WsFrameSchema` `message` variant keeps `content: z.unknown()` to avoid
 * rejecting forward-compat payloads. The interfaces below are an ergonomic
 * narrowing surface for callers writing `if (msg.kind === 'tool_use')`-style
 * helpers.
 *
 * `user_input`, `system`, and `error` carry freeform payloads — leave them
 * untyped (`unknown`) at this layer.
 */
export interface AssistantTextContent {
  text: string
}

export interface ToolUseContent {
  name: string
  input: unknown
  tool_use_id: string
}

export interface ToolResultContent {
  tool_use_id: string
  output: unknown
  is_error: boolean
}

/**
 * Mirrors `domain::PtyQueueEntry` (migration 0008). `input_kind` is one of
 * `prompt|cancel|control` (free TEXT). `status` walks
 * `pending → dispatched → completed|failed|cancelled`.
 */
export const PtyQueueEntrySchema = z.object({
  id: z.string(),
  session_id: z.string(),
  sequence: z.number(),
  input_kind: z.string(),
  payload: z.string(),
  enqueued_at: z.string(),
  dispatched_at: z.string().nullable(),
  completed_at: z.string().nullable(),
  status: z.string(),
  error: z.string().nullable(),
})
export type PtyQueueEntry = z.infer<typeof PtyQueueEntrySchema>

// ---------------------------------------------------------------------------
// WebSocket frame schemas
// ---------------------------------------------------------------------------

/**
 * Outbound frames (client → server). Discriminated via `{ type: ... }` top-
 * level literal, mirroring the Rust `FrameIn` enum with
 * `#[serde(tag = "type", rename_all = "snake_case")]`.
 */
export const InputFrameSchema = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('input'),
    kind: z.enum(['prompt', 'cancel', 'control']),
    payload: z.string(),
  }),
  z.object({
    type: z.literal('resize'),
    cols: z.number(),
    rows: z.number(),
  }),
  z.object({
    type: z.literal('ping'),
  }),
])
export type InputFrame = z.infer<typeof InputFrameSchema>

/**
 * Inbound frames (server → client). Discriminated via `{ type: ... }` top-
 * level literal, mirroring the Rust `FrameOut` enum with
 * `#[serde(tag = "type", rename_all = "snake_case")]`.
 */
export const WsFrameSchema = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('message'),
    sequence: z.number(),
    kind: z.string(),
    content: z.unknown(),
    raw_text: z.string().nullable(),
    created_at: z.string(),
  }),
  z.object({
    type: z.literal('status'),
    status: z.string(),
    at: z.string(),
  }),
  z.object({
    type: z.literal('skipped'),
    bytes: z.number(),
    reason: z.string(),
  }),
  z.object({
    type: z.literal('error'),
    code: z.string(),
    message: z.string(),
  }),
  z.object({
    type: z.literal('pong'),
  }),
])
export type WsFrame = z.infer<typeof WsFrameSchema>
export type WsFrameType = WsFrame['type']

// ---------------------------------------------------------------------------
// REST fetch wrappers
// ---------------------------------------------------------------------------

/** `GET /api/pty/sessions` — list sessions, optionally filtered. */
export async function listSessions(params?: {
  status?: string
  project_id?: string
}): Promise<PtySession[]> {
  const qs = new URLSearchParams()
  if (params?.status) qs.set('status', params.status)
  if (params?.project_id) qs.set('project_id', params.project_id)
  const query = qs.toString() ? `?${qs.toString()}` : ''
  return handle(await fetch(`${API_BASE}/pty/sessions${query}`), z.array(PtySessionSchema))
}

/**
 * Body for `POST /api/pty/sessions`. Mirrors `SpawnSessionBody` on the Rust
 * handler side.
 */
export interface SpawnRequest {
  label?: string | null
  project_id?: string | null
  cwd: string
  claude_args?: string[]
  agent_json?: string | null
  model?: string | null
  env_passthrough_otel?: boolean
  settings_json?: string | null
  prompt_pattern?: string | null
}

/** `POST /api/pty/sessions` — spawn a fresh PTY-backed claude session. */
export async function spawnSession(body: SpawnRequest): Promise<PtySession> {
  return handle(
    await fetch(`${API_BASE}/pty/sessions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
    PtySessionSchema,
  )
}

/** `GET /api/pty/sessions/{id}` — one session row; 404 when absent. */
export async function getSession(id: string): Promise<PtySession> {
  return handle(
    await fetch(`${API_BASE}/pty/sessions/${encodeURIComponent(id)}`),
    PtySessionSchema,
  )
}

/**
 * `GET /api/pty/sessions/{id}/messages` — transcript page.
 *
 * @param params.since  Return only messages with `sequence > since`.
 * @param params.limit  Max rows to return (server clamps 1–1000; default 100).
 */
export async function getMessages(
  id: string,
  params?: { since?: number; limit?: number },
): Promise<PtyMessage[]> {
  const qs = new URLSearchParams()
  if (params?.since !== undefined) qs.set('since', String(params.since))
  if (params?.limit !== undefined) qs.set('limit', String(params.limit))
  const query = qs.toString() ? `?${qs.toString()}` : ''
  return handle(
    await fetch(`${API_BASE}/pty/sessions/${encodeURIComponent(id)}/messages${query}`),
    z.array(PtyMessageSchema),
  )
}

/** `GET /api/pty/sessions/{id}/queue` — all queue entries for the session. */
export async function getQueue(id: string): Promise<PtyQueueEntry[]> {
  return handle(
    await fetch(`${API_BASE}/pty/sessions/${encodeURIComponent(id)}/queue`),
    z.array(PtyQueueEntrySchema),
  )
}

/**
 * `POST /api/pty/sessions/{id}/input` — enqueue one input frame.
 * Returns void; the server responds with `201 { sequence: <n> }` on success
 * (the sequence number is silently discarded by this wrapper).
 */
export async function sendInput(
  id: string,
  body: { kind: string; payload: string },
): Promise<void> {
  const res = await fetch(`${API_BASE}/pty/sessions/${encodeURIComponent(id)}/input`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  return handleVoid(res)
}

/**
 * `POST /api/pty/sessions/{id}/inputs/batch` — enqueue N input frames in
 * order. Returns void; the server responds with `201 { sequences: [...] }` on
 * success.
 */
export async function sendInputsBatch(
  id: string,
  frames: Array<{ kind: string; payload: string }>,
): Promise<void> {
  const res = await fetch(`${API_BASE}/pty/sessions/${encodeURIComponent(id)}/inputs/batch`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(frames),
  })
  return handleVoid(res)
}

/**
 * `PATCH /api/pty/sessions/{id}` — metadata update stub.
 *
 * The server returns 501 Not Implemented in v1; this wrapper still exists so
 * callers can round-trip the request and receive the error gracefully via the
 * normal `handle<T>()` error path (`throw new Error('API request failed: ...')`).
 */
export async function updateSession(
  id: string,
  body: { label?: string; project_id?: string },
): Promise<PtySession> {
  return handle(
    await fetch(`${API_BASE}/pty/sessions/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
    PtySessionSchema,
  )
}

/**
 * `DELETE /api/pty/sessions/{id}` — cancel and tombstone the session.
 *
 * In v1 this is the SAME route as `deleteSession`: the backend handler
 * (`cancel_session` in `lumina/src/http/pty_sessions.rs`) sends a Cancel
 * InputFrame to the in-memory Session, transitions its status to Cancelled,
 * and persists the tombstone — there is no separate cancel-only endpoint.
 * Returns void on 204.
 */
export async function cancelSession(id: string): Promise<void> {
  const res = await fetch(`${API_BASE}/pty/sessions/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
  return handleVoid(res)
}

/**
 * `DELETE /api/pty/sessions/{id}` — alias for {@link cancelSession}.
 *
 * Cancel and delete are the same operation in v1 (the backend handler both
 * signals the running process and tombstones the row). Exported separately
 * so callers that intend a "delete history" semantic have a discoverable name.
 */
export async function deleteSession(id: string): Promise<void> {
  return cancelSession(id)
}

// ---------------------------------------------------------------------------
// WebSocket opener
// ---------------------------------------------------------------------------

/** Handle returned by {@link openSessionStream}. */
export interface SessionStream {
  /** Send a typed input frame to the server. */
  send: (frame: InputFrame) => void
  /**
   * Register a handler for a specific WS frame type. Multiple handlers for
   * the same type are called in registration order.
   */
  on: (event: WsFrameType, handler: (frame: WsFrame) => void) => void
  /** Close the connection gracefully (code 1000). Does not reconnect. */
  close: () => void
}

/**
 * Open a WebSocket connection to a PTY session's broadcast fan-out.
 *
 * URL scheme: `ws://` when the page is served over `http:`, `wss://` when
 * served over `https:`. The host is taken from `location.host` so this works
 * behind any reverse proxy that rewrites the path.
 *
 * Auto-reconnect: on an unexpected close (code !== 1000), the opener retries
 * with exponential back-off starting at 1s, doubling each attempt, capped at
 * 30s. The `userClosed` flag prevents reconnection after a caller-initiated
 * `close()`.
 *
 * Note: the application-layer Ping/Pong round-trip is a v1 no-op on the
 * server (the receiver task does not own the WS write half). Clients should
 * rely on the underlying WebSocket protocol ping/pong frames, which axum
 * handles automatically. The `ping` InputFrame type is kept for forward-compat.
 */
export function openSessionStream(id: string): SessionStream {
  const wsBase =
    (typeof location !== 'undefined' && location.protocol === 'https:' ? 'wss:' : 'ws:') +
    '//' +
    (typeof location !== 'undefined' ? location.host : 'localhost')
  const url = `${wsBase}/api/pty/sessions/${encodeURIComponent(id)}/ws`

  // Per-type handler registry.
  const handlers = new Map<string, Array<(frame: WsFrame) => void>>()

  let ws: WebSocket
  let userClosed = false
  let reconnectDelay = 1000

  function dispatch(frame: WsFrame): void {
    const list = handlers.get(frame.type)
    if (list) {
      for (const fn of list) fn(frame)
    }
  }

  function connect(): void {
    ws = new WebSocket(url)

    ws.addEventListener('message', (event: MessageEvent) => {
      let parsed: ReturnType<typeof WsFrameSchema.safeParse>
      try {
        parsed = WsFrameSchema.safeParse(JSON.parse(event.data as string))
      } catch {
        return
      }
      if (parsed.success) {
        dispatch(parsed.data)
      }
    })

    ws.addEventListener('close', (event: CloseEvent) => {
      if (userClosed || event.code === 1000) return
      // Unexpected close — schedule a reconnect with exponential back-off.
      const delay = reconnectDelay
      reconnectDelay = Math.min(reconnectDelay * 2, 30_000)
      setTimeout(() => {
        if (!userClosed) connect()
      }, delay)
    })

    ws.addEventListener('open', () => {
      // Reset back-off on a successful connection.
      reconnectDelay = 1000
    })
  }

  connect()

  return {
    send(frame: InputFrame): void {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(frame))
      }
    },

    on(event: WsFrameType, handler: (frame: WsFrame) => void): void {
      const list = handlers.get(event)
      if (list) {
        list.push(handler)
      } else {
        handlers.set(event, [handler])
      }
    },

    close(): void {
      userClosed = true
      ws.close(1000)
    },
  }
}
