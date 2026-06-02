//! lumina — flow-tracking platform (vertical slice).
//!
//! SQLite is the canonical store; every mutation also emits a git-committable
//! per-item TOML snapshot for audit. Agents drive writes over MCP; a Vue SPA
//! served by axum gives a navigable overview. One axum `Router` carries the
//! `/api` JSON routes, the `/mcp` MCP service, and the SPA fallback over one
//! shared `AppState { pool }`.
//!
//! ## Module graph (frozen)
//!
//! Every module is declared here up front so that later implementation waves
//! fill in module bodies WITHOUT editing this file, `main.rs`, or `app.rs`.
//! `app.rs` is the sole-owner composition root that wires the builder seams
//! (`http::router`, `mcp::service`, `assets::spa_fallback`), so Tasks 4/5/6
//! implement those builders in their own module files and never touch the root.
//! This is what makes the later waves parallelisable. (Git-export is driven
//! on-demand through `POST /api/export`, not a background task.)

pub mod app;
pub mod cli;
pub mod db;
pub mod domain;
pub mod error;
pub mod repo;
pub mod http;
pub mod assets;
pub mod mcp;
pub mod export;
pub mod import;
pub mod pty;
