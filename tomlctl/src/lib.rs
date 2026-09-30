// Crate root for the `tomlctl` library: module wiring plus `run()`, the parse,
// dispatch and error-report cycle the binary's `main` delegates to. CLI
// parsing, dispatch and output plumbing belong in `cli.rs`, per-subcommand
// behaviour in sibling modules. Modules stay private; anything a library
// consumer needs is exposed as a `pub` wrapper or constant here.

mod agents;
mod anchor;
mod backlog;
mod blocks;
mod capabilities;
mod cli;
mod clusters;
mod convert;
mod dedup;
mod errors;
mod flow;
mod integrity;
mod io;
mod items;
mod items_sweep;
mod json;
mod orphans;
mod output;
mod owner;
mod query;
mod repo_files;
mod repo_root;
mod sweep;
mod tasks;
#[cfg(test)]
mod test_support;
mod time;
mod union_find;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, ErrorFormat, ReadIntegrityArgs, WriteIntegrityArgs};
use crate::errors::TaggedError;

/// The files of a flow directory that [`snapshot`] reads, store first. Its
/// `revision` hashes them in this order, so a poller fingerprinting exactly
/// these files sees every change the snapshot can.
pub const SNAPSHOT_INPUTS: [&str; 4] = [
    "tasks.toml",
    "execution-record.toml",
    "agents.toml",
    "context.toml",
];

/// Refuses a flow slug the CLI's `--slug` would refuse, so a consumer can
/// reject one up front rather than on every [`snapshot`] call.
pub fn validate_slug(slug: &str) -> anyhow::Result<()> {
    flow::validate_slug(slug)
}

/// The `tasks snapshot --slug <slug>` document for the flow under
/// `<root>/.claude/flows`, read without integrity checks. Refuses a slug the
/// CLI would refuse, so it can never name a path outside that directory.
/// Advisories are silenced for the process.
pub fn snapshot(root: &Path, slug: &str) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    flow::validate_slug(slug)?;
    let store_path = root
        .join(".claude")
        .join("flows")
        .join(slug)
        .join(SNAPSHOT_INPUTS[0]);
    let read_opts = ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    };
    tasks::snapshot(slug, &store_path, &read_opts)
}

/// The unfiltered `flow list` envelope for every flow under
/// `<root>/.claude/flows`, read without integrity checks.
pub fn flow_list(root: &Path) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    flow::list_all(root)
}

/// Record one hook payload as `agents record <harness>` does. Changes the
/// process's working directory to the payload's `cwd` and fixes the repo root
/// for the rest of the process, so call it once per process.
pub fn record_agent(
    harness: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    let write_opts = WriteIntegrityArgs {
        allow_outside: false,
        no_write_integrity: false,
        verify_integrity: false,
        strict_integrity: false,
        no_create: false,
    };
    agents::dispatch::record_value(harness, payload, &write_opts)
}

/// Parses the process arguments, runs the selected verb and reports any error
/// on stderr. Returns `ExitCode::FAILURE` on error; clap's own parse-error and
/// `--help` / `--version` exits happen inside `Cli::parse` as before.
pub fn run() -> ExitCode {
    // Parse `Cli` exactly once, here: peeking `--error-format` with a second
    // `try_parse()` swallows clap's errors on the peek path and double-renders
    // `--help` / `--version`. `error_format` is plucked from the parsed struct
    // so `emit_error` still has it after `run()` bails.
    let cli = Cli::parse();
    let error_format = cli.error_format;
    match cli::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            emit_error(&err, error_format);
            ExitCode::FAILURE
        }
    }
}

fn emit_error(err: &anyhow::Error, fmt: ErrorFormat) {
    match fmt {
        ErrorFormat::Text => {
            // `{:#}` prints the full anyhow cause chain inline; combined
            // with `with_context(…"parsing {}", path)` in `read_toml`, toml's
            // Display impl then emits line:col + caret diagnostics for syntax
            // errors.
            eprintln!("tomlctl: {:#}", err);
        }
        ErrorFormat::Json => {
            // Must be anyhow's inherent `downcast_ref`, not
            // `err.chain().find_map(|e| e.downcast_ref::<TaggedError>())`:
            // `chain()` yields `&dyn Error`, whose downcast sees
            // `ContextError<C, E>` rather than the tag inside it.
            let tagged: Option<&TaggedError> = err.downcast_ref::<TaggedError>();
            let kind = tagged.map(|t| t.kind.as_str()).unwrap_or("other");
            let file = tagged
                .and_then(|t| t.file.as_ref())
                .map(|p| p.to_string_lossy().into_owned());
            let arg = tagged.and_then(|t| t.arg);
            // `{:#}` to match text mode's full-chain rendering, so JSON
            // consumers get the same prose in the `message` field.
            let message = format!("{:#}", err);
            let envelope = serde_json::json!({
                "error": {
                    "kind": kind,
                    "message": message,
                    // Always include the key — consumers can rely on a
                    // stable JSON shape (null when the tag carries no path).
                    "file": file,
                    // The clap id of the argument the error is about, when a
                    // verb tags one; null otherwise.
                    "arg": arg,
                }
            });
            // Ignore write errors on the stderr path — if stderr itself is
            // broken there's nothing reasonable to do, and the process is
            // about to exit 1 regardless.
            let mut stderr = std::io::stderr().lock();
            let _ = serde_json::to_writer(&mut stderr, &envelope);
            let _ = writeln!(stderr);
        }
    }
}
