//! lumina-core — the DB/domain layer of the lumina flow-tracking platform.
//!
//! SQLite is the canonical store; every mutation also emits a git-committable
//! per-item TOML snapshot for audit (`export`). This crate holds the
//! record-layer modules — `domain` types, the `repo` mutation/read surface, the
//! `db` backend-erased seam, the `error` taxonomy, git-`export`, flow `import`,
//! and the DB-free JSONL transcript-parsing back-edge (`jsonl_tail` +
//! `protocol`) that `repo::sessions` depends on. The web/server layer (app, cli,
//! assets, http, mcp, the PTY transport) lives in `lumina-server`, which depends
//! on this crate.
//!
//! `AppError`'s axum `IntoResponse` impl is gated behind the optional `axum`
//! feature (`lumina-server` enables it); core is axum-free by default.

pub mod db;
pub mod domain;
pub mod error;
pub mod export;
pub mod import;
pub mod jsonl_tail;
pub mod protocol;
pub mod repo;
