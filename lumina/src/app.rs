//! Composition root — the SOLE owner of router and `AppState` assembly.
//!
//! `serve` reads config from the environment, builds the shared pool, wires the
//! three builder seams (`http::router`, `mcp::service`, `assets::spa_fallback`)
//! and the background export task (`export::spawn`), then runs the server.
//! Later waves fill in the seam bodies in their own module files and never edit
//! this file, so Wave B/C parallelism is conflict-free.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use anyhow::Context as _;
use axum::Extension;
use axum::Router;

use crate::db::AnyPool;

/// Shared application state. Cheap to clone — the pool is `Arc`-wrapped and
/// sqlx pools are themselves ref-counted, so handlers and the MCP layer all
/// share one connection pool.
///
/// The PTY fields below thread the lumina-pty-service (`docs/plans/lumina-pty-service.md`)
/// dependencies through the composition root. `pty_register_tx` is `Option` in v1: T9
/// constructs `AppState` with `None` (so spawn handlers do everything *except*
/// the supervisor-registration step); T11 will spawn the supervisor and swap the
/// `Option` to `Some(supervisor.register_tx())` from `serve`. Handler-side reads
/// gate on `if let Some(tx)` so the surface is forward-compatible.
#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<AnyPool>,
    /// Keyed lookup of in-memory PTY sessions (T9 spawn → insert; T8 supervisor
    /// reap → remove). Shared between the HTTP / MCP layers and the supervisor.
    pub pty_registry: Arc<crate::pty::registry::SessionRegistry>,
    /// Pluggable transport seam. v1 stores `Arc::new(PtyTransport)`; reserved
    /// for future ACP / remote backends.
    pub pty_transport: Arc<dyn crate::pty::transport::Transport + Send + Sync>,
    /// Supervisor registration channel — `None` until T11 wires the supervisor
    /// from `serve`. Handlers that need to register a freshly-spawned session
    /// gate on `if let Some(tx) = state.pty_register_tx.as_ref()` and skip
    /// silently when absent. **Not a placeholder receiver hidden in
    /// AppState** — keeping the option None makes the un-wired state
    /// observable rather than silently dispatching to a dropped channel.
    pub pty_register_tx:
        Option<tokio::sync::mpsc::Sender<crate::pty::supervisor::SessionRegistration>>,
}

impl AppState {
    /// Construct an `AppState` with default PTY plumbing: a fresh empty
    /// `SessionRegistry`, the unit-struct `PtyTransport`, and `None` for
    /// `pty_register_tx`. The PTY supervisor wire-up (T11) replaces the
    /// last field via direct mutation after spawning the supervisor.
    ///
    /// Provided so per-family HTTP tests, the e2e test, and the composition
    /// root can construct `AppState` without each having to know the
    /// per-field defaults — drift-killer for the new fields added in T9.
    pub fn new(pool: Arc<AnyPool>) -> Self {
        Self {
            pool,
            pty_registry: crate::pty::registry::SessionRegistry::new(),
            pty_transport: Arc::new(crate::pty::pty_transport::PtyTransport),
            pty_register_tx: None,
        }
    }
}

/// Default port when `PORT` is unset. Picked to be uncommon (no well-known
/// service binds it) and below the lowest default ephemeral-port floor across
/// Linux/macOS/Windows (Linux's `net.ipv4.ip_local_port_range` starts at 32768;
/// macOS and Windows start at 49152) — that avoids transient collisions with
/// outbound sockets on any of the three.
const DEFAULT_PORT: u16 = 24817;

/// Default bind address when `HOST` is unset. Loopback-only by default: the
/// JSON `/api/*` surface has no auth and no Host-header check (the MCP `/mcp`
/// surface is separately protected by rmcp 1.7's `allowed_hosts` default —
/// `{localhost, 127.0.0.1, ::1}` per GHSA-89vp-x53w-74fx — but that protection
/// does NOT extend to `/api/*`). Set `HOST=0.0.0.0` to opt in to LAN exposure
/// of the REST surface, or `HOST=::` for dual-stack IPv6.
const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Parse an env var into `T`, falling back to `default` when unset. A parse
/// failure on a *set* value logs to stderr and still falls back — so a typo
/// like `PORT=8080a` does not silently bind the default without trace.
fn parse_env_or_default<T: std::str::FromStr>(var: &str, default: T) -> T
where
    T::Err: std::fmt::Display,
{
    match std::env::var(var) {
        Err(_) => default,
        Ok(raw) => match raw.parse() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("warning: {var}={raw:?} is not a valid value ({e}); using default");
                default
            }
        },
    }
}

/// Build the pool, assemble the router, spawn the export task, and serve.
///
/// `db::init` opens the pool (creating the file if absent) and runs the
/// embedded migrations, so the runtime server always starts on a migrated
/// schema.
pub async fn serve() -> anyhow::Result<()> {
    // `.env` is read by the operator's shell / dotenv tooling in dev; we read
    // straight from the process environment with a sensible local default so
    // `cargo run` works out of the box without external dotenv loading.
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://lumina.db".to_string());

    // Open the pool (creating the file if absent, foreign keys on) and run the
    // embedded migrations on startup — `db::init` (Task 2) is the single entry
    // point that owns both. The runtime server therefore starts on a migrated
    // schema.
    let pool = crate::db::init(&database_url)
        .await
        .with_context(|| format!("initialising database at {database_url}"))?;
    let pool = Arc::new(AnyPool::from(pool));

    // Construct AppState (registry, transport, and pool defaults are set by
    // AppState::new). The supervisor shares the same registry Arc so it can
    // insert/remove sessions on spawn/reap.
    let mut state = AppState::new(pool.clone());

    // Spawn the PTY supervisor, wiring it to the registry that AppState owns.
    // The supervisor's register_tx is stored so HTTP/MCP spawn handlers can
    // hand off freshly-created sessions to the supervisor's exit-reap loop.
    let supervisor = crate::pty::supervisor::spawn(pool.clone(), state.pty_registry.clone());
    state.pty_register_tx = Some(supervisor.register_tx());

    // Kick off the background git-export materialiser before serving so no
    // mutation's outbox row goes undrained while the server is up. Retain the
    // handle so shutdown can stop the loop cleanly instead of relying on a
    // process-exit kill.
    let export_handle = crate::export::spawn(pool.clone());

    let app = build_router(state);

    let port: u16 = parse_env_or_default("PORT", DEFAULT_PORT);
    let host: IpAddr = parse_env_or_default("HOST", DEFAULT_HOST);
    let addr = SocketAddr::from((host, port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding listener on {addr}"))?;
    println!("lumina listening on http://{addr}");

    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("axum server error");

    // Shutdown ordering: HTTP server has already drained above.
    // 1. Stop the PTY supervisor (cancels the loop, awaits join) before the
    //    export drain so any final `pty_sessions` updates the supervisor may
    //    flush are picked up by the drain while the pool is still live.
    supervisor.shutdown().await;

    // 2. Stop the export loop and await its join regardless of whether the
    //    server exited cleanly — keeps the runtime free of orphaned tasks.
    export_handle.shutdown().await;

    serve_result?;
    eprintln!("lumina shutdown complete");
    Ok(())
}

/// Resolve when the process receives a shutdown signal: Ctrl+C on every
/// platform, plus SIGTERM on Unix (so `systemctl stop` / `docker stop` exit
/// gracefully too). Returning from this future drives `axum::serve`'s graceful
/// shutdown — without it, Ctrl+C tears the tokio runtime down mid-accept and
/// the OS reports the process as terminated via STATUS_CONTROL_C_EXIT
/// (0xC000013A) on Windows.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            eprintln!("lumina: failed to install Ctrl+C handler: {e}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                eprintln!("lumina: failed to install SIGTERM handler: {e}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }

    eprintln!("lumina: shutdown signal received, draining…");
}

/// Assemble the full router from the three builder seams. Pulled out of `serve`
/// so the e2e test (Task 10) can build the same router over a temp-DB state
/// without binding a listener.
pub fn build_router(state: AppState) -> Router {
    let pool = state.pool.clone();

    Router::new()
        // `/api/*` JSON routes (Task 4 extends `http::router`, keeping /health).
        .nest("/api", crate::http::router())
        // `/mcp` MCP service (Task 5 returns the real StreamableHttpService;
        // app.rs only `.nest_service`s it, so the concrete type may change).
        .nest_service("/mcp", crate::mcp::service_with_state(state.clone()))
        // `/mcp-ask` — single-tool MCP server (`ask_user_question`) registered
        // with spawned `claude` sessions via `--mcp-config` so the agent can ask
        // the operator structured questions in the SPA (native-AUQ replacement;
        // see `crate::pty::ask`). Separate mount keeps the 58-tool work-item
        // surface OUT of spawned sessions.
        .nest_service("/mcp-ask", crate::pty::ask::service(state.clone()))
        // SPA fallback last (Task 4 wires rust-embed / ServeDir).
        .fallback_service(crate::assets::spa_fallback())
        // The MCP tools (Task 5) read the pool via an `Extension` layer through
        // their `RequestContext`; set the layer up now so Task 5 needn't edit
        // this file.
        .layer(Extension(pool))
        // Provide `AppState` to the typed `/api` handlers.
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _; // for `oneshot`

    /// Task 1 acceptance: `GET /api/health` answers 200 against a tableless DB.
    /// Driven in-process via `oneshot` (no socket bind) so it runs anywhere.
    #[tokio::test]
    async fn health_returns_200() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite pool");
        let state = AppState::new(Arc::new(AnyPool::from(pool)));
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
