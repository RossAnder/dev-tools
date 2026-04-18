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
