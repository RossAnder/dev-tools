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
//! * `Db` / `Other` → 500 — the raw error stays in the variant for
//!   `Debug`/`Display` at the call site but is NEVER serialised into the 500
//!   body: the response carries only a generic message so DB internals and
//!   arbitrary `anyhow` chains do not leak to clients.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
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

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(e)
    }
}

impl AppError {
    /// The stable `kind` string used in the JSON envelope (matches tomlctl's
    /// taxonomy slugs).
    fn kind(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "not_found",
            AppError::Validation(_) => "validation",
            AppError::Db(_) => "db",
            AppError::Other(_) => "other",
        }
    }

    /// The HTTP status this error maps to.
    fn status(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::Db(_) | AppError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The client-facing message. `NotFound`/`Validation` are specific;
    /// `Db`/`Other` collapse to a generic string so internals never leak.
    fn client_message(&self) -> String {
        match self {
            AppError::NotFound(m) | AppError::Validation(m) => m.clone(),
            AppError::Db(_) => "a database error occurred".to_owned(),
            AppError::Other(_) => "an internal error occurred".to_owned(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "error": {
                "kind": self.kind(),
                "message": self.client_message(),
            }
        }));
        (self.status(), body).into_response()
    }
}
