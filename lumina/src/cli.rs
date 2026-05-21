//! CLI dispatch (Task 7).
//!
//! A clap `derive` `Cli` with an OPTIONAL subcommand. The bare invocation (no
//! subcommand) preserves the original behaviour — it starts the server via
//! `crate::app::serve`. `import-flow <slug>` resolves the flow directory as
//! `.claude/flows/<slug>/` (relative to the CWD), opens a pool via
//! `crate::db::init`, imports the flow, prints the summary, and returns.
//!
//! `main.rs` still only calls `cli::run()`; it is not edited by this task.

use std::path::PathBuf;

use anyhow::Context as _;
use clap::{Parser, Subcommand};

/// lumina — flow-tracking platform (vertical slice).
#[derive(Debug, Parser)]
#[command(name = "lumina", version, about)]
struct Cli {
    /// Optional subcommand. With none, lumina starts the axum server (the
    /// original default behaviour).
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Import one `.claude/flows/<slug>/` flow directory into the DB.
    ImportFlow {
        /// The flow slug — resolved to `.claude/flows/<slug>/` under the CWD.
        slug: String,
    },
}

/// Parse args and dispatch. No subcommand → serve; `import-flow` → import.
pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => crate::app::serve().await,
        Some(Command::ImportFlow { slug }) => import_flow_cmd(&slug).await,
    }
}

/// Resolve the flow dir, open a pool, import, and print the summary.
async fn import_flow_cmd(slug: &str) -> anyhow::Result<()> {
    let flow_dir: PathBuf = PathBuf::from(".claude").join("flows").join(slug);
    if !flow_dir.is_dir() {
        anyhow::bail!(
            "flow directory not found: {} (run from the repo root)",
            flow_dir.display()
        );
    }

    // Reuse the runtime DB the server uses, honouring DATABASE_URL with the same
    // local default as `app::serve`.
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://lumina.db".to_string());
    let pool = crate::db::init(&database_url)
        .await
        .with_context(|| format!("initialising database at {database_url}"))?;

    let summary = crate::import::import_flow(&pool, &flow_dir)
        .await
        .with_context(|| format!("importing flow '{slug}'"))?;

    println!(
        "imported flow '{slug}': {} scaffold work-items, {} tasks, {} findings ({} execution-record items dropped)",
        summary.scaffold_created,
        summary.tasks_created,
        summary.findings_created,
        summary.items_dropped,
    );

    Ok(())
}
