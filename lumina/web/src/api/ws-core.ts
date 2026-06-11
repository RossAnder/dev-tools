// Generic reconnecting-WebSocket core.
//
// T7 of the read-only sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md, Wave 1). Carved from
// `openSessionStream` in `api/pty.ts` (which stays byte-identical this wave):
// the `ws(s)://location.host` url derivation, the exponential back-off
// (1s -> 30s, doubling, reset-on-open), and the `userClosed` guard that lets a
// caller-initiated `close()` (code 1000) suppress reconnection.
//
// This module is TRANSPORT ONLY: it JSON-parses each inbound message and
// hands the raw parsed value to `onFrame` — zod validation of the frame shape
// lives in the consumer (`api/stream.ts`), not here. Malformed JSON is
// swallowed silently, mirroring the pty.ts behaviour.
//
// INTERNAL module — deliberately NOT re-exported from the `api/index.ts`
// barrel. Composables import it directly by path.

/** Options for {@link openReconnectingSocket}. */
export interface ReconnectingSocketOptions {
  /** Path to connect to, e.g. `/api/stream`. Joined onto `ws(s)://location.host`. */
  path: string
  /** Called with the JSON-parsed value of every inbound message. */
  onFrame: (data: unknown) => void
  /** Called on every successful `open` (initial connect AND each reconnect). */
  onOpen?: () => void
  /** Called on every unexpected close (code !== 1000, not user-initiated). */
  onDown?: () => void
}

/** Handle returned by {@link openReconnectingSocket}. */
export interface ReconnectingSocket {
  /** JSON-stringify `data` and send it; silently dropped unless the socket is OPEN. */
  send: (data: unknown) => void
  /** Close gracefully (code 1000) and suppress any future reconnect. */
  close: () => void
}

/**
 * Open a WebSocket to `ws(s)://location.host + opts.path` with auto-reconnect.
 *
 * URL scheme: `ws://` when the page is served over `http:`, `wss://` when
 * served over `https:`; host from `location.host` so it works behind any
 * reverse proxy that rewrites the path.
 *
 * Auto-reconnect: on an unexpected close (code !== 1000), retries with
 * exponential back-off starting at 1s, doubling each attempt, capped at 30s,
 * reset to 1s on a successful `open`. A caller-initiated `close()` sets the
 * `userClosed` guard so no reconnect is ever scheduled afterwards.
 *
 * The `typeof WebSocket` guard means a missing global (or a test that swaps
 * `globalThis.WebSocket` for a fake) is resolved at connect time, never at
 * module-load time.
 */
export function openReconnectingSocket(opts: ReconnectingSocketOptions): ReconnectingSocket {
  const wsBase =
    (typeof location !== 'undefined' && location.protocol === 'https:' ? 'wss:' : 'ws:') +
    '//' +
    (typeof location !== 'undefined' ? location.host : 'localhost')
  const url = `${wsBase}${opts.path}`

  let ws: WebSocket | null = null
  let userClosed = false
  let reconnectDelay = 1000
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null

  function connect(): void {
    // Resolve the constructor at connect time so tests can stub
    // `globalThis.WebSocket` and a non-browser environment degrades to a no-op.
    if (typeof WebSocket === 'undefined') return
    ws = new WebSocket(url)

    ws.addEventListener('message', (event: MessageEvent) => {
      let parsed: unknown
      try {
        parsed = JSON.parse(event.data as string)
      } catch {
        // Malformed JSON — swallow. Frame-shape validation is the consumer's job.
        return
      }
      opts.onFrame(parsed)
    })

    ws.addEventListener('close', (event: CloseEvent) => {
      if (userClosed || event.code === 1000) return
      // Unexpected close — schedule a reconnect with the CURRENT delay, then
      // double for the next attempt (capped at 30s).
      const delay = reconnectDelay
      reconnectDelay = Math.min(reconnectDelay * 2, 30_000)
      reconnectTimer = setTimeout(() => {
        reconnectTimer = null
        if (!userClosed) connect()
      }, delay)
      opts.onDown?.()
    })

    ws.addEventListener('open', () => {
      // Reset back-off on a successful connection.
      reconnectDelay = 1000
      opts.onOpen?.()
    })
  }

  connect()

  return {
    send(data: unknown): void {
      if (ws !== null && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(data))
      }
    },

    close(): void {
      userClosed = true
      if (reconnectTimer !== null) {
        clearTimeout(reconnectTimer)
        reconnectTimer = null
      }
      ws?.close(1000)
    },
  }
}
