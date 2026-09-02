// Pure entrypoint: CLI parsing, dispatch and output plumbing belong in
// `cli.rs`, per-subcommand behaviour in sibling modules. Keep `fn main()` a
// thin wrapper over `cli::run()` so anyhow's cause chain renders on exit.

// mimalloc, because the workload is dominated by small allocations —
// TomlValue/JsonValue tree clones, per-item serde_json::Map insertions
// during ledger reads, and per-line Vec<u8> churn in parity hashing.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod backlog;
mod blocks;
mod capabilities;
mod cli;
mod convert;
mod dedup;
mod errors;
mod flow;
mod integrity;
mod io;
mod items;
mod json;
mod orphans;
mod output;
mod query;
#[cfg(test)]
mod test_support;
mod time;

use std::io::Write;

use clap::Parser;

use crate::cli::{Cli, ErrorFormat};
use crate::errors::TaggedError;

fn main() {
    // Parse `Cli` exactly once, here: peeking `--error-format` with a second
    // `try_parse()` swallows clap's errors on the peek path and double-renders
    // `--help` / `--version`. `error_format` is plucked from the parsed struct
    // so `emit_error` still has it after `run()` bails.
    let cli = Cli::parse();
    let error_format = cli.error_format;
    if let Err(err) = cli::run(cli) {
        emit_error(&err, error_format);
        std::process::exit(1);
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
