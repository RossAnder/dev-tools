// Crate root for the `tomlctl` library: module wiring plus `run()`, the parse,
// dispatch and error-report cycle the binary's `main` delegates to. CLI
// parsing, dispatch and output plumbing belong in `cli.rs`, per-subcommand
// behaviour in sibling modules. Modules stay private; anything a library
// consumer needs is exposed as a `pub fn` wrapper here.

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

use anyhow::anyhow;
use clap::Parser;

use crate::agents::schema::Harness;
use crate::cli::{Cli, ErrorFormat, ReadIntegrityArgs, WriteIntegrityArgs};
use crate::errors::TaggedError;

/// The `tasks snapshot` document for the flow whose store is `store_path`,
/// read without integrity checks. Advisories are silenced for the process.
pub fn snapshot(slug: &str, store_path: &Path) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    let read_opts = ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    };
    tasks::snapshot(slug, store_path, &read_opts)
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
    let harness = Harness::parse(harness).ok_or_else(|| {
        anyhow!(
            "agents record: unknown harness `{harness}` — expected one of {}",
            Harness::VOCABULARY.join(", ")
        )
    })?;
    let write_opts = WriteIntegrityArgs {
        allow_outside: false,
        no_write_integrity: false,
        verify_integrity: false,
        strict_integrity: false,
        no_create: false,
    };
    agents::record::record(harness, payload, &write_opts)
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
