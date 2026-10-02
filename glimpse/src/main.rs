//! glimpse: a live terminal view of a flow's task graph, checkpoints and running agents.

mod actions;
mod app;
mod cli;
mod config;
mod diagram;
mod diff;
mod flows;
mod form;
mod herdr;
mod hook;
mod keys;
mod ledger;
mod model;
mod pane;
mod runtime;
mod setup;
mod source;
mod state;
mod surface;
mod theme;
mod transcript;
mod view;
mod watch;
mod writer;
mod writes;

use std::path::Path;
use std::process::ExitCode;

use crate::cli::{Command, Once, Parsed, ViewArgs};
use crate::config::Config;
use crate::herdr::Herdr;
use crate::ledger::Ledger;
use crate::model::Snapshot;
use crate::runtime::RunOpts;
use crate::source::{Fetcher, InProcessFetcher};
use crate::surface::Surface;
use tomlctl::LedgerRef;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_hook = args.first().is_some_and(|a| a == "hook");
    let command = match cli::parse(args) {
        Parsed::Run(command) => command,
        Parsed::Print(text) => {
            print!("{text}");
            return ExitCode::SUCCESS;
        }
        // A harness reads neither the stream nor the exit code of an async hook, so a
        // mistyped hook entry is reported where the hook's other failures go.
        Parsed::Fail(text) if is_hook => {
            hook::log_error(text.lines().next().unwrap_or_default());
            return ExitCode::SUCCESS;
        }
        Parsed::Fail(text) => {
            eprint!("{text}");
            return ExitCode::from(2);
        }
    };
    let result = match command {
        Command::Hook { harness } => {
            hook::run_hook(std::io::stdin().lock(), harness);
            Ok(())
        }
        Command::EnsurePane { slug, focus } => ensure_pane(slug.as_deref(), focus),
        Command::Setup { dry_run } => run_setup(dry_run),
        Command::View(args) => run_view(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("glimpse: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_view(args: ViewArgs) -> Result<(), String> {
    let (mut config, warning) = Config::load();
    if let Some(view) = args.view {
        config.default_view = view;
    }
    if let Some(orientation) = args.orientation {
        config.orientation = orientation;
    }
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot read the working directory: {e}"))?;
    let root = flows::repo_root(&cwd).unwrap_or(cwd);
    let opts = RunOpts {
        root,
        slug: args.slug,
        config,
        warning,
        keep_view: args.view.is_some(),
        keep_orientation: args.orientation.is_some(),
        surface: args.surface,
    };
    match args.once {
        None => runtime::run(opts),
        Some(once) => {
            let ledger = match &once.ledger {
                Some(path) => Some(read_ledger(&opts, path)?),
                None => None,
            };
            let snapshot = once_snapshot(&opts, &once)?;
            if let Some(id) = once.select
                && !snapshot.tasks.iter().any(|task| task.id == id)
            {
                return Err(format!("--select {id}: no such task in {}", snapshot.slug));
            }
            print!(
                "{}",
                runtime::render_once(
                    &opts,
                    snapshot,
                    once.select,
                    ledger,
                    once.width,
                    once.height
                )
            );
            Ok(())
        }
    }
}

/// A `--snapshot` file is read as-is; otherwise the flow's files are read once, for the
/// explicit slug or the freshest flow's. With `--ledger` and no slug, the task graph is
/// left empty rather than taken from whichever flow is freshest.
fn once_snapshot(opts: &RunOpts, once: &Once) -> Result<Snapshot, String> {
    if let Some(path) = &once.snapshot {
        return read_snapshot(path);
    }
    let slug = match &opts.slug {
        Some(slug) => slug.clone(),
        None if once.ledger.is_some() => return Ok(Snapshot::default()),
        None => {
            let entries = flows::list(&opts.root, &source::task_store_mtimes(&opts.root))?;
            flows::freshest(&entries)
                .map(|f| f.slug.clone())
                .ok_or_else(|| {
                    format!(
                        "no flow under {} has a tasks.toml; pass --slug",
                        opts.root.display()
                    )
                })?
        }
    };
    InProcessFetcher
        .fetch(&opts.root, &slug, None)
        .map(|built| built.expect("no known revision always builds"))
}

/// Reads a `--ledger` file and pairs it with `--surface`, or else with the surface that
/// lists its kind. A missing file is an error here, though a ledger feed reads it as empty.
fn read_ledger(opts: &RunOpts, path: &Path) -> Result<(Surface, Ledger), String> {
    if !path.is_file() {
        return Err(format!("--ledger {}: no such file", path.display()));
    }
    let value = tomlctl::ledger_read(&opts.root, &LedgerRef::File(path.to_path_buf()))
        .map_err(|e| format!("--ledger {}: {e:#}", path.display()))?;
    let ledger = Ledger::from_value(value)?;
    let surface = opts
        .surface
        .or_else(|| Surface::of_kind(ledger.kind))
        .unwrap_or_default();
    Ok((surface, ledger))
}

fn read_snapshot(path: &Path) -> Result<Snapshot, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: bad snapshot: {e}", path.display()))
}

fn ensure_pane(slug: Option<&str>, focus: bool) -> Result<(), String> {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let origin = var("HERDR_ACTIVE_PANE_ID")
        .or_else(|| var("HERDR_PANE_ID"))
        .ok_or(
            "ensure-pane runs inside herdr: neither HERDR_ACTIVE_PANE_ID nor HERDR_PANE_ID is set",
        )?;
    let (config, _) = Config::load();
    pane::ensure(&Herdr::from_env(), &origin, slug, None, focus, &config).map(drop)
}

fn run_setup(dry_run: bool) -> Result<(), String> {
    let report = setup::setup(dry_run)?;
    print!("{report}");
    if report.is_ok() {
        Ok(())
    } else {
        Err("setup did not complete; see the errors above".to_string())
    }
}
