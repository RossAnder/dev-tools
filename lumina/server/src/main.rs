//! Thin binary entrypoint. All wiring lives in the library (`app.rs` is the
//! composition root); this file only starts the tokio runtime and delegates to
//! the CLI dispatcher. Later tasks extend `cli::run` (e.g. the `import-flow`
//! subcommand) without touching this file.

use lumina_server::cli;
use tracing_subscriber::{EnvFilter, fmt};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Structured logging via tracing. EnvFilter honours RUST_LOG (e.g.
    // `RUST_LOG=lumina=debug`); default keeps lumina at info and silences
    // chatty deps. Writes to stderr so stdout stays clean for the CLI's
    // structured outputs (e.g. `lumina import-flow`).
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("lumina=info,tower_http=info"));
    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();

    cli::run().await
}
