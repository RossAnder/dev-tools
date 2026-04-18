// R63: `main.rs` is now a pure entrypoint. All CLI parsing, dispatch,
// and output plumbing lives in `cli.rs`; per-subcommand behaviour lives
// in sibling modules (`items`, `blocks`, `convert`, `io`, …). A single
// `fn main()` wrapper forwards to `cli::run()` so anyhow's cause chain
// can render via `{:#}` on exit.
mod blocks;
mod cli;
mod convert;
mod dedup;
mod integrity;
mod io;
mod items;
mod orphans;
mod query;
#[cfg(test)]
mod test_support;

fn main() {
    if let Err(err) = cli::run() {
        // R16: `{:#}` prints the full anyhow cause chain inline; combined with
        // `with_context(…"parsing {}", path)` in `read_toml`, toml's Display
        // impl then emits line:col + caret diagnostics for syntax errors.
        eprintln!("tomlctl: {:#}", err);
        std::process::exit(1);
    }
}
