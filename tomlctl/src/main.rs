// Pure entrypoint: `main` is a thin wrapper over `tomlctl::run()`, and all
// module and dispatch plumbing lives in `lib.rs`. The global allocator is
// declared here and nowhere else: one declared in the lib would silently
// become the allocator of every crate that depends on it.

// mimalloc is opt-in (`--features mimalloc`): tomlctl is a one-shot CLI, and
// the allocator's start-up cost outweighed its allocation speed-up when
// measured, so the system allocator is the default.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> std::process::ExitCode {
    tomlctl::run()
}
