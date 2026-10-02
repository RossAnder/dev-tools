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

// Debug builds of clap's derived command tree need close to 1 MB of stack,
// the whole of a Windows main thread, so the CLI runs on a thread sized like
// a Linux main thread instead.
const CLI_STACK_BYTES: usize = 8 * 1024 * 1024;

fn main() -> std::process::ExitCode {
    let spawned = std::thread::Builder::new()
        .name("tomlctl".into())
        .stack_size(CLI_STACK_BYTES)
        .spawn(tomlctl::run);
    match spawned {
        Ok(handle) => handle
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        Err(_) => tomlctl::run(),
    }
}
