//! CLI dispatch.
//!
//! STUB for the slice: the bare invocation serves the app. Task 7 adds a clap
//! `Cli` enum here with an `import-flow <slug>` subcommand, dispatching to
//! `crate::import`, without editing `main.rs`/`app.rs`. The default (no-subcommand)
//! path must keep starting the server.

/// Run lumina. Currently always starts the server; Task 7 introduces
/// subcommand parsing where the no-args path still falls through to `serve`.
pub async fn run() -> anyhow::Result<()> {
    crate::app::serve().await
}
