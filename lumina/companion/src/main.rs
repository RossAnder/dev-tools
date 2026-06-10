//! Thin binary entrypoint (mirrors the lumina server bin's shape: mimalloc
//! global allocator + tracing-subscriber + tokio runtime). All loop logic
//! lives in the library ([`lumina_companion::connection`]); this file only
//! parses the CLI, validates CONFIG (the one class of error that terminates
//! the companion — everything past validation retries forever inside the
//! dial loop), and wires `ShellGit` + `Executor` into [`connection::run`].

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use lumina_companion::connection::{self, ConnectionConfig};
use lumina_companion::executor::Executor;
use lumina_companion::git::ShellGit;
use tracing_subscriber::{EnvFilter, fmt};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// The git-execution-plane companion (ADR-0006 Step 1b): dials the lumina
/// server, executes the coarse git intents it pushes, reports outcomes.
#[derive(Debug, Parser)]
#[command(name = "lumina-companion", version)]
struct Args {
    /// WebSocket URL of the lumina server's companion endpoint.
    #[arg(long, default_value = "ws://127.0.0.1:24817/api/companion/ws")]
    server_url: String,

    /// Root of the repository's PRIMARY checkout the companion executes
    /// against. Defaults to the current directory; a relative path resolves
    /// against the current directory.
    #[arg(long)]
    repo_root: Option<PathBuf>,

    /// Identity reported in the Hello handshake (informational-only in 1b).
    /// Defaults to `<hostname>-<pid>`.
    #[arg(long)]
    companion_id: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    // Structured logging via tracing (mirrors lumina/server/src/main.rs).
    // EnvFilter honours RUST_LOG (e.g. `RUST_LOG=lumina_companion=debug`);
    // default keeps the companion at info and silences chatty deps. Writes
    // to stderr so stdout stays clean.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("lumina_companion=info"));
    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            tracing::error!(error = %e, "cannot resolve the current directory");
            return ExitCode::FAILURE;
        }
    };
    // Lexical absolutisation only — deliberately NO `canonicalize`: on
    // Windows it yields a `\\?\` verbatim path, which we keep out of
    // `git -C` invocations and the Hello frame.
    let repo_root = match args.repo_root {
        Some(p) if p.is_absolute() => p,
        Some(p) => cwd.join(p),
        None => cwd,
    };
    // "Is a git repo" = `<root>/.git` exists — a directory in a primary
    // checkout, a file in a linked worktree; both pass this cheap config
    // gate (the executor's `.git/info/exclude` registration enforces the
    // primary-checkout requirement at operation time). Deliberately simple;
    // worktree-of-a-worktree edge cases are out of scope for 1b.
    if !repo_root.join(".git").exists() {
        tracing::error!(
            repo_root = %repo_root.display(),
            "--repo-root is not a git repository (no .git entry)"
        );
        return ExitCode::FAILURE;
    }

    let companion_id = args.companion_id.unwrap_or_else(default_companion_id);
    let executor = Executor::new(repo_root.clone(), Arc::new(ShellGit::new(repo_root.clone())));
    let config = ConnectionConfig {
        server_url: args.server_url,
        companion_id,
        repo_root: repo_root.display().to_string(),
    };

    tracing::info!(
        url = %config.server_url,
        companion_id = %config.companion_id,
        repo_root = %config.repo_root,
        "lumina-companion starting"
    );
    // The dial loop never returns (`Infallible`): the companion's natural
    // lifecycle ends only when the process is killed.
    match connection::run(config, executor).await {}
}

/// `<hostname>-<pid>` without a hostname dependency: the `COMPUTERNAME`
/// (Windows) / `HOSTNAME` (unix) env var, falling back to `companion`.
/// Informational-only in 1b, so best-effort is fine.
fn default_companion_id() -> String {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "companion".to_owned());
    format!("{hostname}-{}", std::process::id())
}
