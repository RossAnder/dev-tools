//! Shared test helpers.
//!
//! The `env_lock()` mutex serialises env-var-mutating tests in any module.
//! Tests in `io.rs`, `main.rs`, and `cli.rs` can share it through a single
//! `OnceLock<Mutex<()>>` anchored here.
//!
//! `RootGuard` is the only place that sets `TOMLCTL_ROOT`, and `with_root()`
//! is the closure form of it. A per-module copy is how one module ends up
//! leaving the override set after a panicking assertion, which then steers
//! every later test on the thread at a deleted directory.

#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// A throwaway repo root carrying `.claude/`, exported as `TOMLCTL_ROOT` for
/// as long as the guard lives. Holds the env lock for the same span, so two
/// sandboxed tests never overlap.
///
/// `Drop` runs during an unwind, so a failed assertion inside the sandbox
/// cannot leak the override into whatever test runs next on this thread.
/// Prefer `with_root`; reach for the guard only when the sandbox has to
/// outlive a closure.
#[cfg(test)]
pub(crate) struct RootGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _tmp: tempfile::TempDir,
    root: std::path::PathBuf,
}

#[cfg(test)]
impl RootGuard {
    pub(crate) fn new() -> Self {
        let lock = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        // Canonical, because `repo_or_cwd_root` canonicalises what it
        // returns and a bare temp path compares unequal on macOS and
        // Windows.
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        // SAFETY: set_var is unsafe in edition 2024; acceptable while the
        // env lock above is held.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", root.as_os_str());
        }
        Self {
            _lock: lock,
            _tmp: tmp,
            root,
        }
    }

    pub(crate) fn root(&self) -> &std::path::Path {
        &self.root
    }
}

#[cfg(test)]
impl Drop for RootGuard {
    fn drop(&mut self) {
        // SAFETY: remove_var is unsafe in edition 2024; acceptable here
        // because the env lock is still held by `_lock`, which is dropped
        // after this body runs.
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
    }
}

/// Run `f` against a throwaway `TOMLCTL_ROOT`. The temporary tree is deleted
/// on return, so anything the caller wants to assert on has to be read
/// inside `f` and returned out.
#[cfg(test)]
pub(crate) fn with_root<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
    let guard = RootGuard::new();
    f(guard.root())
}
