//! glimpse: a live terminal view of a flow's task graph, checkpoints and running agents.

mod app;
mod cli;
mod config;
mod diagram;
mod diff;
mod flows;
mod herdr;
mod hook;
mod keys;
mod model;
mod pane;
mod runtime;
mod setup;
mod source;
mod theme;
mod transcript;
mod view;

use std::path::Path;
use std::process::ExitCode;

use crate::cli::{Command, Once, Parsed, ViewArgs};
use crate::config::Config;
use crate::herdr::Herdr;
use crate::model::Snapshot;
use crate::runtime::RunOpts;
use crate::source::{Fetcher, TomlctlFetcher};

fn main() -> ExitCode {
    let command = match cli::parse(std::env::args().skip(1)) {
        Parsed::Run(command) => command,
        Parsed::Print(text) => {
            print!("{text}");
            return ExitCode::SUCCESS;
        }
        Parsed::Fail(text) => {
            eprint!("{text}");
            return ExitCode::from(2);
        }
    };
    let result = match command {
        Command::Hook => {
            hook::run_hook(std::io::stdin().lock());
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
    };
    match args.once {
        None => runtime::run(opts),
        Some(once) => {
            let snapshot = once_snapshot(&opts, &once)?;
            print!(
                "{}",
                runtime::render_once(&opts, snapshot, once.width, once.height)
            );
            Ok(())
        }
    }
}

/// A `--snapshot` file is read as-is and never touches tomlctl; otherwise the slug is the
/// explicit one or the freshest flow's, fetched once.
fn once_snapshot(opts: &RunOpts, once: &Once) -> Result<Snapshot, String> {
    if let Some(path) = &once.snapshot {
        return read_snapshot(path);
    }
    let tomlctl = &opts.config.tomlctl;
    source::probe_tomlctl(tomlctl, &opts.root)?;
    let slug = match &opts.slug {
        Some(slug) => slug.clone(),
        None => {
            let entries = flows::list(&opts.root, tomlctl)?;
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
    TomlctlFetcher {
        tomlctl: tomlctl.clone(),
    }
    .fetch(&opts.root, &slug)
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
    let herdr = Herdr::from_env();
    let cwd = herdr.pane_get(&origin)?.cwd;
    let (config, _) = Config::load();
    pane::ensure(&herdr, &origin, slug, &cwd, focus, &config).map(drop)
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
