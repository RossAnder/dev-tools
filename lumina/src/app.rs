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
use sqlx::SqlitePool;

/// Shared application state. Cheap to clone — the pool is `Arc`-wrapped and
/// sqlx pools are themselves ref-counted, so handlers and the MCP layer all
/// share one connection pool.
#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<SqlitePool>,
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
    let pool = Arc::new(pool);

    let state = AppState { pool: pool.clone() };

    // Kick off the background git-export materialiser before serving so no
    // mutation's outbox row goes undrained while the server is up.
    crate::export::spawn(pool.clone());

    let app = build_router(state);

    let port: u16 = parse_env_or_default("PORT", DEFAULT_PORT);
    let host: IpAddr = parse_env_or_default("HOST", DEFAULT_HOST);
    let addr = SocketAddr::from((host, port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding listener on {addr}"))?;
    println!("lumina listening on http://{addr}");

    axum::serve(listener, app)
        .await
        .context("axum server error")?;
    Ok(())
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
        .nest_service("/mcp", crate::mcp::service(pool.clone()))
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
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite pool");
        let state = AppState {
            pool: Arc::new(pool),
        };
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
