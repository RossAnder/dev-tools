// R63: `main.rs` is now a pure entrypoint. All CLI parsing, dispatch,
// and output plumbing lives in `cli.rs`; per-subcommand behaviour lives
// in sibling modules (`items`, `blocks`, `convert`, `io`, …). A single
// `fn main()` wrapper forwards to `cli::run()` so anyhow's cause chain
// can render via `{:#}` on exit.

// O56: mimalloc as the global allocator. tomlctl's workload is dominated
// by small allocations — TomlValue/JsonValue tree clones, per-item
// serde_json::Map insertions during ledger reads, and per-line Vec<u8>
// churn in parity hashing. Microsoft's benchmarks show ~5.3× faster
// small-allocation throughput vs glibc malloc; rust-analyzer landed the
// same swap in reference PR #19603 for an analogous profile.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod blocks;
mod cli;
mod convert;
mod dedup;
mod errors;
mod integrity;
mod io;
mod items;
mod orphans;
mod query;
#[cfg(test)]
mod test_support;

use std::io::Write;

use crate::cli::ErrorFormat;
use crate::errors::TaggedError;

fn main() {
    // Parse once up-front so we can pluck `--error-format` before dispatching.
    // `cli::run()` re-parses internally (trivial cost — the real work is the
    // subcommand execution); keeping parse in both places means `main.rs` stays
    // a thin wrapper and `cli.rs` stays reachable for tests that build `Cli`
    // directly without going through `main()`.
    let error_format = cli::parse_error_format();
    if let Err(err) = cli::run() {
        emit_error(&err, error_format);
        std::process::exit(1);
    }
}

/// T8: emit a top-level error to stderr in the selected format. Text mode is
/// byte-identical to the pre-T8 `eprintln!("tomlctl: {:#}", err)` line.
fn emit_error(err: &anyhow::Error, fmt: ErrorFormat) {
    match fmt {
        ErrorFormat::Text => {
            // R16: `{:#}` prints the full anyhow cause chain inline; combined
            // with `with_context(…"parsing {}", path)` in `read_toml`, toml's
            // Display impl then emits line:col + caret diagnostics for syntax
            // errors.
            eprintln!("tomlctl: {:#}", err);
        }
        ErrorFormat::Json => {
            // T8: `anyhow::Error::downcast_ref::<TaggedError>()` walks anyhow's
            // internal context wrappers and finds the attached `TaggedError`
            // through any number of `.context(...)` layers. The plan suggested
            // `err.chain().find_map(|e| e.downcast_ref::<TaggedError>())`, but
            // `chain()` yields `&dyn Error` and the trait-object downcast does
            // NOT see anyhow's context-wrapped values (anyhow wraps `C: Display
            // + Debug` in its own internal `ContextError<C, E>` type — the
            // `C` isn't the concrete trait-object's type). anyhow's inherent
            // `downcast_ref` method, in contrast, explicitly understands its
            // own context shape and unwraps through it. Empirically verified
            // against anyhow 1.0.102: wrap `TaggedError` in `.context(...)`,
            // then stack any number of further `.context(msg)` layers, and
            // `outer.downcast_ref::<TaggedError>()` still returns `Some`.
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
