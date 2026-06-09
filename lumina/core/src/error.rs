//! `AppError` taxonomy and its `IntoResponse` impl (Task 3).
//!
//! Mirrors tomlctl's error envelope (`{"error":{"kind":...,"message":...}}`)
//! and `ErrorKind` taxonomy, narrowed to the four variants this slice needs.
//! Both the HTTP handlers (Task 4) and the MCP tools (Task 5) return
//! `Result<_, AppError>`, so this type is the single error currency across both
//! entry points.
//!
//! Mapping to HTTP status:
//!
//! * `NotFound` → 404 — caller-facing message (specific).
//! * `Validation` → 422 — caller-facing message (specific; e.g. illegal
//!   hierarchy edge).
//! * `Cycle` → 422 — task-dependency graph cycle (migration 0005). Carries the
//!   offending edge list so callers (the `wire-task-deps` SKILL, the sprint
//!   composer) can surface the cycle's task ids without re-running the topo
//!   sort. Mapped to 422 like `Validation` (caller-fixable input).
//! * `Db` / `Other` → 500 — the raw error stays in the variant for
//!   `Debug`/`Display` at the call site but is NEVER serialised into the 500
//!   body: the response carries only a generic message so DB internals and
//!   arbitrary `anyhow` chains do not leak to clients.

#[cfg(feature = "axum")]
use axum::Json;
#[cfg(feature = "axum")]
use axum::http::StatusCode;
#[cfg(feature = "axum")]
use axum::response::{IntoResponse, Response};
#[cfg(feature = "axum")]
use serde_json::json;

/// The crate-wide error type. `Db` wraps the raw `sqlx::Error` and `Other`
/// wraps an `anyhow::Error`; both render as an opaque 500 to clients.
#[derive(Debug)]
pub enum AppError {
    /// Requested aggregate does not exist. → 404. Message is caller-facing.
    NotFound(String),
    /// Caller input failed a domain rule (e.g. illegal hierarchy edge). → 422.
    /// Message is caller-facing and should be specific.
    Validation(String),
    /// Task-dependency graph contains a cycle (migration 0005). The `edges`
    /// vector lists the offending `(task_id, depends_on_id)` pairs that remain
    /// after Kahn's algorithm has drained the zero-in-degree frontier — i.e.
    /// the strongly-connected residue. → 422.
    Cycle { edges: Vec<(String, String)> },
    /// A database error. → 500 with a generic body (internals never leak).
    Db(sqlx::Error),
    /// Any other internal failure. → 500 with a generic body.
    Other(anyhow::Error),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NotFound(m) => write!(f, "not found: {m}"),
            AppError::Validation(m) => write!(f, "validation: {m}"),
            AppError::Cycle { edges } => {
                write!(f, "task-dependency cycle: {} offending edge(s)", edges.len())?;
                for (a, b) in edges {
                    write!(f, " [{a} -> {b}]")?;
                }
                Ok(())
            }
            AppError::Db(e) => write!(f, "db error: {e}"),
            AppError::Other(e) => write!(f, "internal error: {e}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Db(e) => Some(e),
            AppError::Other(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Db(e)
    }
}

impl AppError {
    /// Map a raw `sqlx::Error` onto the taxonomy with `RowNotFound` lifted to a
    /// caller-facing `NotFound` (404) rather than an opaque `Db` 500.
    ///
    /// Used by the [`crate::db`] seam's `query_one` primitive: a single-row read
    /// that finds nothing is a 404, not an internal error. Every other sqlx
    /// failure (decode, protocol, constraint, I/O) stays `Db`. The optional
    /// `context` is folded into the 404 message so callers see *what* was missing
    /// (e.g. `"work_item 'abc'"`); pass an empty string for a bare "not found".
    pub fn from_sqlx_not_found(e: sqlx::Error, context: &str) -> Self {
        match e {
            sqlx::Error::RowNotFound => {
                if context.is_empty() {
                    AppError::NotFound("row not found".to_owned())
                } else {
                    AppError::NotFound(format!("{context} not found"))
                }
            }
            other => AppError::Db(other),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(e)
    }
}

// These three helpers feed only the axum `IntoResponse` impl below (and
// `status` returns axum's `StatusCode`), so the whole block is gated behind the
// optional `axum` feature alongside it. They stay private.
#[cfg(feature = "axum")]
impl AppError {
    /// The stable `kind` string used in the JSON envelope (matches tomlctl's
    /// taxonomy slugs).
    fn kind(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "not_found",
            AppError::Validation(_) => "validation",
            AppError::Cycle { .. } => "cycle",
            AppError::Db(_) => "db",
            AppError::Other(_) => "other",
        }
    }

    /// The HTTP status this error maps to.
    fn status(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Validation(_) | AppError::Cycle { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::Db(_) | AppError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The client-facing message. `NotFound`/`Validation`/`Cycle` are specific;
    /// `Db`/`Other` collapse to a generic string so internals never leak.
    fn client_message(&self) -> String {
        match self {
            AppError::NotFound(m) | AppError::Validation(m) => m.clone(),
            AppError::Cycle { edges } => format!(
                "task-dependency cycle detected ({} edge(s) in the residue)",
                edges.len()
            ),
            AppError::Db(_) => "a database error occurred".to_owned(),
            AppError::Other(_) => "an internal error occurred".to_owned(),
        }
    }
}

#[cfg(feature = "axum")]
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Build the body before consuming `self.kind()` / `self.client_message()`
        // — `Cycle` carries the offending-edge list and surfaces it as a
        // structured `edges` field on the JSON envelope so the `wire-task-deps`
        // SKILL / composer can avoid re-running the topo sort.
        let mut envelope = serde_json::Map::new();
        envelope.insert("kind".to_owned(), json!(self.kind()));
        envelope.insert("message".to_owned(), json!(self.client_message()));
        if let AppError::Cycle { edges } = &self {
            let edges_json: Vec<_> = edges
                .iter()
                .map(|(a, b)| json!({ "task_id": a, "depends_on_id": b }))
                .collect();
            envelope.insert("edges".to_owned(), json!(edges_json));
        }
        let body = Json(json!({ "error": serde_json::Value::Object(envelope) }));
        (self.status(), body).into_response()
    }
}
