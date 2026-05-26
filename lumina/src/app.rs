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
/// service binds it) and below 32768 so it sits outside the Linux kernel's
/// default ephemeral-port range (`net.ipv4.ip_local_port_range`, typically
/// 32768–60999) — that avoids transient collisions with outbound sockets.
const DEFAULT_PORT: u16 = 24817;

/// Default bind address when `HOST` is unset. `0.0.0.0` makes the SPA and the
/// JSON `/api/*` surface reachable from the LAN; the MCP `/mcp` surface stays
/// loopback-only regardless, because the rmcp 1.7 `allowed_hosts` default
/// rejects any Host header outside `{localhost, 127.0.0.1, ::1}` (the
/// DNS-rebinding mitigation per GHSA-89vp-x53w-74fx — see `mcp.rs`). Override
/// with `HOST=127.0.0.1` to restore loopback-only HTTP, or `HOST=::` for
/// dual-stack IPv6.
const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

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

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let host: IpAddr = std::env::var("HOST")
        .ok()
        .and_then(|h| h.parse().ok())
        .unwrap_or(DEFAULT_HOST);
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
