//! Thin binary entrypoint. All wiring lives in the library (`app.rs` is the
//! composition root); this file only starts the tokio runtime and delegates to
//! the CLI dispatcher. Later tasks extend `cli::run` (e.g. the `import-flow`
//! subcommand) without touching this file.

use lumina::cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
