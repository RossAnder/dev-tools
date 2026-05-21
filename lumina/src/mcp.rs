//! MCP server (rmcp, Streamable-HTTP transport).
//!
//! STUB for the slice: returns an empty `axum::Router`, which is a valid
//! `tower::Service` and so satisfies the `.nest_service("/mcp", ...)` mount in
//! the composition root. Task 5 returns the real
//! `rmcp::transport::streamable_http_server::tower::StreamableHttpService`
//! (also a tower `Service`) built from a per-request `service_factory` closure
//! capturing this `Arc<SqlitePool>`. Because `app.rs` only `.nest_service`s the
//! result and never names the concrete type, the return type may change freely
//! when Task 5 lands without editing `app.rs`.

use std::sync::Arc;

use axum::Router;
use sqlx::SqlitePool;

/// Build the MCP service mounted at `/mcp`.
///
/// The pool is threaded in now so Task 5 can capture it in the rmcp
/// `service_factory` closure (clone-per-request is cheap). Unused in the stub.
pub fn service(_pool: Arc<SqlitePool>) -> Router {
    Router::new()
}
