//! R62: shared test helpers.
//!
//! The `env_lock()` mutex serialises env-var-mutating tests in any module.
//! Tests in `io.rs`, `main.rs`, and `cli.rs` can share it through a single
//! `OnceLock<Mutex<()>>` anchored here.

#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}
