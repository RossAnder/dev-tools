// Pure entrypoint: `main` is a thin wrapper over `tomlctl::run()`, and all
// module and dispatch plumbing lives in `lib.rs`. The global allocator is
// declared here and nowhere else: one declared in the lib would silently
// become the allocator of every crate that depends on it.

// mimalloc, because the workload is dominated by small allocations —
// TomlValue/JsonValue tree clones, per-item serde_json::Map insertions
// during ledger reads, and per-line Vec<u8> churn in parity hashing.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> std::process::ExitCode {
    tomlctl::run()
}
