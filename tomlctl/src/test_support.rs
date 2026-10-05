//! Shared test helpers.
//!
//! The `env_lock()` mutex serialises env-var-mutating tests in any module.
//! Tests in any library module (today `io.rs` and `items.rs`) share it
//! through a single `OnceLock<Mutex<()>>` anchored here.
//!
//! `RootGuard` is the only place that sets `TOMLCTL_ROOT`, and `with_root()`
//! is the closure form of it. A per-module copy is how one module ends up
//! leaving the override set after a panicking assertion, which then steers
//! every later test on the thread at a deleted directory.

use std::fs;
use std::path::Path;

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

/// Run `f` on a thread with the binary's CLI stack. Anything that builds or
/// parses through the clap `Cli` needs it: libtest's 2 MB test threads leave
/// the debug command tree little headroom. A panic in `f` resumes here, so
/// the test fails with its own message.
#[cfg(test)]
pub(crate) fn on_cli_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    let name = std::thread::current().name().unwrap_or("cli").to_string();
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name(name)
            .stack_size(crate::CLI_STACK_BYTES)
            .spawn_scoped(scope, f)
            .expect("spawn a CLI-sized test thread")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// A `git` command run in `dir`, stripped of the discovery variables a git
/// hook exports. Without that, a suite run from inside a hook would have
/// every fixture `git init` or `git add` land in the outer repository.
#[cfg(test)]
pub(crate) fn git_command(dir: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY");
    cmd
}

/// Tests that shell out to git return early when this is false, rather than
/// failing on a machine without it.
#[cfg(test)]
pub(crate) fn git_available() -> bool {
    git_command(Path::new("."))
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Run `git <args>` in `root`, failing the test on a non-zero exit.
#[cfg(test)]
pub(crate) fn git(root: &Path, args: &[&str]) {
    let out = git_command(root).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(test)]
pub(crate) fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

/*
<!-- SHARED-BLOCK:shipped-gitignore START -->
*/
/// The repository's own rules, verbatim: a fixture that drifted from them
/// would have every ignore-dependent verdict assert against rules nobody
/// ships.
const GITIGNORE: &str = "/.claude/backlog-evidence/**\n\
                         !/.claude/backlog-evidence/*/\n\
                         !/.claude/backlog-evidence/*/.evidence\n";

/// What every evidence rule starts with once the `!` of a negation is
/// stripped.
const EVIDENCE_RULE_PREFIX: &str = "/.claude/backlog-evidence/";

/// [`GITIGNORE`], checked against the evidence rules the repository's own
/// `.gitignore` carries — hand-kept copies, and nothing else would notice
/// them diverging. A checkout is not guaranteed (a vendored crate has no
/// repo root to read), so the check is skipped there rather than failed.
pub(crate) fn shipped_gitignore() -> &'static str {
    let repo_gitignore = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|repo| repo.join(".gitignore"))
        .and_then(|path| fs::read_to_string(path).ok());
    if let Some(text) = repo_gitignore {
        let shipped: String = text
            .lines()
            .filter(|line| {
                line.trim_start_matches('!')
                    .starts_with(EVIDENCE_RULE_PREFIX)
            })
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(
            shipped, GITIGNORE,
            "the sandbox fixture and the evidence rules the repository ships have diverged"
        );
    }
    GITIGNORE
}
/*
<!-- SHARED-BLOCK:shipped-gitignore END -->
*/

#[cfg(test)]
mod tests {
    use super::*;

    /// Restores `GIT_DIR` to what it was, including during an unwind, so a
    /// failed assertion cannot leave later git-spawning tests pointed at a
    /// deleted directory.
    struct GitDirGuard(Option<std::ffi::OsString>);

    impl Drop for GitDirGuard {
        fn drop(&mut self) {
            // SAFETY: set_var/remove_var are unsafe in edition 2024;
            // acceptable because the env lock is held for the guard's life.
            unsafe {
                match self.0.take() {
                    Some(prior) => std::env::set_var("GIT_DIR", prior),
                    None => std::env::remove_var("GIT_DIR"),
                }
            }
        }
    }

    #[test]
    fn git_command_ignores_an_inherited_git_dir() {
        if !git_available() {
            return;
        }
        let _lock = env_lock();
        let decoy = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let inherited = decoy.path().join("inherited.git");
        let status = {
            let _restore = GitDirGuard(std::env::var_os("GIT_DIR"));
            // SAFETY: set_var is unsafe in edition 2024; acceptable while the
            // env lock above is held.
            unsafe {
                std::env::set_var("GIT_DIR", &inherited);
            }
            git_command(target.path())
                .args(["init", "-q", "."])
                .status()
                .unwrap()
        };
        assert!(status.success(), "git init failed: {status:?}");
        assert!(
            target.path().join(".git").is_dir(),
            "git init wrote into the inherited GIT_DIR instead of the target"
        );
        assert!(!inherited.exists(), "the inherited GIT_DIR was initialised");
    }
}
