//! R62: filesystem-I/O plumbing split out of `main.rs`.
//!
//! Owns:
//!   - `read_toml` — parse-only TOML reader
//!   - `write_toml_with_sidecar` — atomic write + SHA-256 sidecar refresh
//!   - `atomic_write` — tempfile + fsync + rename
//!   - `guard_write_path` / `canonicalize_for_write` — `.claude/` containment
//!   - `recheck_claude_containment` — TOCTOU narrowing (R3)
//!   - `with_exclusive_lock` — sidecar-lock-file acquire/release (R25)
//!   - `repo_or_cwd_root` + `OnceLock` cache (R46)
//!   - `mutate_doc` — guard→lock→read→mutate→write pipeline
//!   - `LOCK_RETRY` / `DEFAULT_LOCK_TIMEOUT` constants

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

use crate::integrity::{IntegrityOpts, hex_lower, sidecar_path};

/// R25: base retry delay between `try_lock_exclusive` attempts in
/// `with_exclusive_lock`. Jittered ±20% at call time to avoid lockstep retries
/// between competing writers.
pub(crate) const LOCK_RETRY: std::time::Duration = std::time::Duration::from_millis(500);

/// R25: default overall timeout for `with_exclusive_lock`. Overridable per
/// invocation via the `TOMLCTL_LOCK_TIMEOUT` env var (integer seconds).
pub(crate) const DEFAULT_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) fn read_toml(path: &Path) -> Result<TomlValue> {
    let s = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str::<TomlValue>(&s).with_context(|| format!("parsing {}", path.display()))
}

/// Run a closure that mutates a TOML document at `file` under the standard
/// write pipeline: `guard_write_path` → `with_exclusive_lock` → `read_toml` →
/// `f(&mut doc)` → `write_toml_with_sidecar`. Centralises what was previously
/// open-coded at each `Cmd::{Set,SetJson}` / `ItemsOp::{Add,Update,Remove,Apply}`
/// dispatch site.
pub(crate) fn mutate_doc<F>(
    file: &Path,
    allow_outside: bool,
    integrity: IntegrityOpts,
    f: F,
) -> Result<()>
where
    F: FnOnce(&mut TomlValue) -> Result<()>,
{
    guard_write_path(file, allow_outside)?;
    with_exclusive_lock(file, || {
        let mut doc = read_toml(file)?;
        f(&mut doc)?;
        // R3 TOCTOU narrowing: re-canonicalise target parent immediately before
        // the atomic persist and re-check that it still lies under `.claude/`.
        // Only enforced when `--allow-outside` was NOT set, since an explicit
        // opt-out was granted by the user in that case. This narrows (but does
        // not close) the window between `guard_write_path()` and `persist()`;
        // closing it fully requires O_NOFOLLOW via nix/rustix.
        if !allow_outside {
            recheck_claude_containment(file)?;
        }
        write_toml_with_sidecar(file, &doc, integrity)?;
        Ok(())
    })
}

/// Re-canonicalise `file`'s parent and assert it still starts with the
/// `.claude/` canonical root. Used by `mutate_doc` to narrow the TOCTOU window
/// described in R3.
fn recheck_claude_containment(file: &Path) -> Result<()> {
    let parent = file
        .parent()
        .and_then(|p| if p.as_os_str().is_empty() { None } else { Some(p) })
        .unwrap_or(Path::new("."));
    let parent_canonical = parent
        .canonicalize()
        .with_context(|| format!("re-canonicalising parent of {} before persist", file.display()))?;
    let root = repo_or_cwd_root()?;
    let claude_dir = root.join(".claude");
    let claude_canonical = claude_dir.canonicalize().unwrap_or(claude_dir);
    if parent_canonical.starts_with(&claude_canonical) {
        return Ok(());
    }
    bail!(
        "pre-persist containment check failed: target parent {} is no longer under {} (possible TOCTOU symlink swap since guard_write_path — aborting)",
        parent_canonical.display(),
        claude_canonical.display()
    )
}

/// Acquire an exclusive sidecar `.lock` file around a write operation, with a
/// timeout so a stranded lock (crashed tomlctl, OS-mandatory Windows lock,
/// heavy contention) produces a clear error instead of hanging forever.
///
/// Timeout default is 30 seconds; override with the `TOMLCTL_LOCK_TIMEOUT`
/// env var (integer seconds). On the first observed contention the function
/// emits a one-shot stderr note so a human watching the terminal knows *why*
/// we're paused. The retry delay carries ±20% jitter (deterministic counter
/// hash, no external RNG) to avoid lockstep retries between competing
/// processes.
pub(crate) fn with_exclusive_lock<R>(path: &Path, f: impl FnOnce() -> Result<R>) -> Result<R> {
    use fs4::fs_std::FileExt;
    use std::time::Instant;

    let lock_path = path.with_extension(match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{}.lock", ext),
        None => "lock".to_string(),
    });
    // R39: on unix, open the lock file with 0o600 so it's not world-readable.
    // The lock file is metadata about who holds the write mutex — no reason
    // for it to be group/other-readable. No-op on Windows (OpenOptionsExt is
    // unix-only).
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut open_opts = std::fs::OpenOptions::new();
    open_opts
        .create(true)
        .truncate(false)
        .read(true)
        .write(true);
    #[cfg(unix)]
    open_opts.mode(0o600);
    let lock_file = open_opts
        .open(&lock_path)
        .with_context(|| format!("opening lock file {}", lock_path.display()))?;

    // R25: effective timeout = env override if set, else DEFAULT_LOCK_TIMEOUT.
    // The error message reflects the effective timeout, not the constant default.
    let timeout = std::env::var("TOMLCTL_LOCK_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(std::time::Duration::from_secs)
        .unwrap_or(DEFAULT_LOCK_TIMEOUT);
    let base_delay_ms = LOCK_RETRY.as_millis() as u64;
    let start = Instant::now();
    let mut announced = false;
    let mut attempt: u64 = 0;
    loop {
        match lock_file.try_lock_exclusive() {
            Ok(true) => break,
            Ok(false) => {
                if !announced {
                    eprintln!(
                        "tomlctl: waiting for exclusive lock on {} …",
                        lock_path.display()
                    );
                    announced = true;
                }
                if start.elapsed() >= timeout {
                    bail!(
                        "lock held on {} for {} seconds — another tomlctl process may be hanging. If no tomlctl process is running, check for stale lock and delete {} manually.",
                        lock_path.display(),
                        timeout.as_secs(),
                        lock_path.display()
                    );
                }
                // ±20% jitter via a small counter hash (no RNG needed).
                // Hash maps attempt into [-20, 20] percent of base_delay_ms.
                let h = attempt
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(0xD1B5_4A32_D192_ED03);
                let jitter_pct = (h % 41) as i64 - 20; // -20..=20
                let delta_ms = (base_delay_ms as i64) * jitter_pct / 100;
                let delay_ms = (base_delay_ms as i64 + delta_ms).max(1) as u64;
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                attempt = attempt.wrapping_add(1);
            }
            Err(e) => {
                return Err(anyhow!(e)).with_context(|| {
                    format!("acquiring exclusive lock on {}", lock_path.display())
                });
            }
        }
    }

    // R23: the lock_file binding is alive through this point; drop releases
    // the lock after `f()` returns. No explicit `let _ = lock_file;` needed.
    f()
}

/// Refuse to write to files outside the current repo's `.claude/` directory
/// unless `--allow-outside` was passed on this invocation. Protects against
/// agent-influenced `artifacts.*` paths pointing at e.g. credential files.
///
/// Resolution strategy:
///   1. Canonicalise the target (parent if file doesn't exist yet).
///   2. Find the git top-level via `git rev-parse --show-toplevel`.
///      Fall back to CWD if git is missing or we're not inside a repo.
///   3. Assert canonical target lies under `<root>/.claude/`.
pub(crate) fn guard_write_path(file: &Path, allow_outside: bool) -> Result<()> {
    let canonical = canonicalize_for_write(file).with_context(|| {
        format!("canonicalising write target {}", file.display())
    })?;

    let root = repo_or_cwd_root()?;
    let claude_dir = root.join(".claude");
    // Canonicalise the allowed root too so the prefix comparison is apples-to-apples
    // (on Windows, canonicalize yields extended-length `\\?\` paths).
    let claude_canonical = match claude_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => claude_dir.clone(),
    };

    if canonical.starts_with(&claude_canonical) {
        return Ok(());
    }

    if allow_outside {
        eprintln!(
            "tomlctl: warning: writing outside .claude/ (path resolves to {}) — proceeding because --allow-outside was set",
            canonical.display()
        );
        return Ok(());
    }

    bail!(
        "refusing to write outside .claude/ (path resolves to {}); pass --allow-outside to override",
        canonical.display()
    )
}

/// Canonicalise a write target. If the file doesn't exist yet, canonicalise the
/// parent directory and re-attach the final component. Bails if neither the
/// file nor its parent directory exists.
///
/// R4: additionally rejects any `..` (`Component::ParentDir`) component in the
/// joined path. The parent canonicalises via `canonicalize()` which resolves
/// any embedded `..`, so a `ParentDir` in the joined result can only come from
/// the file-name component itself — a value like `../escape` is obviously
/// malicious and gets refused here even though it didn't appear after the
/// canonical parent prefix.
fn canonicalize_for_write(file: &Path) -> Result<PathBuf> {
    if let Ok(c) = file.canonicalize() {
        return Ok(c);
    }
    let parent = file
        .parent()
        .and_then(|p| if p.as_os_str().is_empty() { None } else { Some(p) })
        .unwrap_or(Path::new("."));
    let parent_canonical = parent
        .canonicalize()
        .with_context(|| format!("parent directory {} not found", parent.display()))?;
    let name = file
        .file_name()
        .ok_or_else(|| anyhow!("write target `{}` has no file name", file.display()))?;
    let joined = parent_canonical.join(name);
    // Reject ParentDir / RootDir components past the canonical parent prefix.
    // Canonicalize() normalised the prefix, so anything in `joined.components()`
    // after the prefix that is a `..` came from the file-name piece — refuse.
    let prefix_len = parent_canonical.components().count();
    for comp in joined.components().skip(prefix_len) {
        match comp {
            std::path::Component::ParentDir | std::path::Component::RootDir => {
                bail!(
                    "write target `{}` contains a disallowed `..` or absolute root component after canonicalisation",
                    file.display()
                );
            }
            _ => {}
        }
    }
    Ok(joined)
}

/// Return the containment anchor used by `guard_write_path`.
///
/// Resolution order:
///   1. `TOMLCTL_ROOT` env var, if set. Canonicalised; errors if the directory
///      does not exist. Intended for tests, chroots, and unusual layouts where
///      neither the git top-level nor the CWD is the right anchor. Checked on
///      EVERY call so tests can swap it in/out under `env_lock()`.
///   2. `git rev-parse --show-toplevel` output, canonicalised. R46: memoised
///      in a process-lifetime `OnceLock` so repeated CLI dispatches don't fork
///      `git` more than once.
///   3. Current working directory, canonicalised. Also memoised (same cache
///      slot — the resolved anchor is deterministic for a given process).
pub(crate) fn repo_or_cwd_root() -> Result<PathBuf> {
    // Env override is always live — never cached, so a test flipping
    // TOMLCTL_ROOT sees the new value on the next call.
    if let Ok(env_root) = std::env::var("TOMLCTL_ROOT")
        && !env_root.is_empty()
    {
        let p = PathBuf::from(&env_root);
        return p.canonicalize().with_context(|| {
            format!("canonicalising TOMLCTL_ROOT={}", env_root)
        });
    }
    // R46: cache git-or-cwd resolution per process. The first call resolves
    // it; every subsequent call hits the OnceLock fast path.
    static REPO_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    if let Some(cached) = REPO_ROOT.get() {
        return Ok(cached.clone());
    }
    let cwd = std::env::current_dir().context("reading current working directory")?;
    let resolved = match std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() {
                cwd.canonicalize().unwrap_or(cwd)
            } else {
                let p = PathBuf::from(s);
                p.canonicalize().unwrap_or(p)
            }
        }
        _ => cwd.canonicalize().unwrap_or(cwd),
    };
    // `get_or_init` ensures only the first caller's resolved path wins — a
    // second concurrent resolve just discards its computed value.
    Ok(REPO_ROOT.get_or_init(|| resolved).clone())
}

/// Write the TOML document and (unless suppressed) also write the `<file>.sha256`
/// sidecar.
///
/// R31 (torn-sidecar): the hash is computed in memory from the serialised
/// bytes BEFORE any rename, so both tempfiles (TOML + sidecar) are staged with
/// byte-content that is guaranteed consistent. We then `persist()` the TOML
/// first and the sidecar second, both under the existing `<file>.lock`
/// exclusive lock. A reader that interleaves between the two `persist()`
/// calls either:
///   (a) sees the OLD TOML + OLD sidecar — hashes agree, passes integrity;
///   (b) sees the NEW TOML + OLD sidecar — the OLD sidecar's hash is stale,
///       reader fails integrity; this matches the outcome readers would see
///       if a writer crashed after the first persist (desired behaviour);
///   (c) sees the NEW TOML + NEW sidecar — hashes agree, passes integrity.
/// The previous hash-after-rename pipeline had a window where a reader could
/// observe NEW bytes but computed the hash BEFORE the sidecar was refreshed;
/// that window is now closed.
///
/// Failure to persist the sidecar is reported as a stderr warning but does
/// not fail the outer write — the primary TOML is already durable. Set
/// `--strict-integrity` to upgrade that warning to a hard error.
pub(crate) fn write_toml_with_sidecar(
    path: &Path,
    value: &TomlValue,
    integrity: IntegrityOpts,
) -> Result<()> {
    let serialized = toml::to_string_pretty(value).context("serialising TOML")?;
    let bytes = serialized.as_bytes();

    if !integrity.write_sidecar {
        return atomic_write(path, bytes);
    }

    // Stage both tempfiles with hash computed from the same in-memory bytes.
    let hex = hex_lower(&Sha256::digest(bytes));
    let sidecar = sidecar_path(path);
    let basename = path
        .file_name()
        .ok_or_else(|| anyhow!("target `{}` has no file name", path.display()))?
        .to_string_lossy()
        .into_owned();
    let sidecar_contents = format!("{}  {}\n", hex, basename);

    // Persist TOML first; if this fails, the sidecar never appeared on disk
    // at all. If it succeeds, we immediately persist the sidecar — under the
    // same exclusive lock there is no concurrent writer, and any reader
    // observing a mid-swap state lands on the consistent combinations
    // documented above.
    atomic_write(path, bytes)?;
    if let Err(e) = atomic_write(&sidecar, sidecar_contents.as_bytes()) {
        if integrity.strict {
            return Err(e).with_context(|| {
                format!(
                    "wrote {} but failed to refresh integrity sidecar (--strict-integrity was set, so this is a hard error)",
                    path.display()
                )
            });
        }
        eprintln!(
            "tomlctl: warning: wrote {} but failed to refresh integrity sidecar: {:#}",
            path.display(),
            e
        );
    }
    Ok(())
}


/// Atomic-replace pattern: write `bytes` to a tempfile in the same directory as
/// `path`, `sync_all()` to flush to disk, then `persist()` to rename into place.
/// The `sync_all` call is load-bearing — without it, a crash between rename and
/// fsync can leave the target empty on some filesystems. See the tempfile crate
/// docs (`/stebalien/tempfile`) for the canonical pattern.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .and_then(|p| if p.as_os_str().is_empty() { None } else { Some(p) })
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temp file in {}", parent.display()))?;
    tmp.as_file_mut()
        .write_all(bytes)
        .with_context(|| format!("writing temp file for {}", path.display()))?;
    tmp.as_file()
        .sync_all()
        .with_context(|| format!("fsync temp file for {}", path.display()))?;
    tmp.persist(path)
        .map_err(|e| anyhow!("atomic rename to {} failed: {}", path.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::{sidecar_path, verify_integrity};
    use crate::test_support::env_lock;

    const LEDGER: &str = r#"schema_version = 1
last_updated = 2026-04-16

[[items]]
id = "R1"
file = "src/a.rs"
line = 10
severity = "warning"
effort = "small"
category = "quality"
summary = "foo"
first_flagged = 2026-04-08
rounds = 1
status = "open"

[[items]]
id = "R4"
file = "src/b.rs"
line = 20
severity = "critical"
effort = "small"
category = "quality"
summary = "bar"
first_flagged = 2026-04-08
rounds = 1
status = "fixed"
resolved = 2026-04-08
resolution = "fix in abc123"
"#;

    fn led() -> TomlValue {
        toml::from_str(LEDGER).unwrap()
    }

    fn integrity_on() -> IntegrityOpts {
        IntegrityOpts {
            write_sidecar: true,
            verify_on_read: true,
            strict: false,
        }
    }

    fn integrity_write_only() -> IntegrityOpts {
        IntegrityOpts {
            write_sidecar: true,
            verify_on_read: false,
            strict: false,
        }
    }

    #[test]
    fn write_integrity_sidecar_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ledger.toml");
        let doc = led();
        write_toml_with_sidecar(&target, &doc, integrity_write_only()).unwrap();

        // Sidecar exists with sha256sum-style format.
        let sidecar = sidecar_path(&target);
        assert!(sidecar.exists(), "sidecar must be written by default");
        let side = fs::read_to_string(&sidecar).unwrap();
        assert!(side.ends_with("  ledger.toml\n"), "got sidecar: {side:?}");
        let hex = side.split_whitespace().next().unwrap();
        assert_eq!(hex.len(), 64);

        // Verify succeeds.
        verify_integrity(&target).unwrap();

        // Flip a byte in the target; verify now errors with both digests.
        let mut bytes = fs::read(&target).unwrap();
        // Mutate a byte in a way that keeps the file valid TOML — replace the
        // first 'R' in the item ids with 'Q'. Actually we just need any change
        // for the hash to differ; integrity check doesn't reparse.
        let pos = bytes.iter().position(|&b| b == b'R').unwrap();
        bytes[pos] = b'Q';
        fs::write(&target, &bytes).unwrap();
        let err = verify_integrity(&target).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("expected") && msg.contains("actual"),
            "expected dual-digest message, got: {msg}"
        );
    }

    #[test]
    fn verify_integrity_errors_on_missing_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ledger.toml");
        fs::write(&target, LEDGER).unwrap();
        let err = verify_integrity(&target).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("ledger.toml.sha256"), "got: {msg}");
        assert!(msg.contains("missing"), "got: {msg}");
    }

    #[test]
    fn no_write_integrity_suppresses_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ledger.toml");
        let doc = led();
        write_toml_with_sidecar(
            &target,
            &doc,
            IntegrityOpts {
                write_sidecar: false,
                verify_on_read: false,
                strict: false,
            },
        )
        .unwrap();
        let sidecar = sidecar_path(&target);
        assert!(!sidecar.exists(), "sidecar must not be written");
    }

    #[test]
    fn verify_rejects_malformed_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ledger.toml");
        fs::write(&target, LEDGER).unwrap();
        let sidecar = sidecar_path(&target);
        fs::write(&sidecar, "not-hex\n").unwrap();
        let err = verify_integrity(&target).unwrap_err();
        assert!(
            format!("{err:#}").contains("does not contain a 64-hex-char digest"),
            "got: {err:#}"
        );
    }

    #[test]
    fn integrity_opts_smoke() {
        // Exercise the constructor helper so unused-code warnings never
        // appear; also pins the verify-on-read ⇒ requires-write-sidecar
        // coupling isn't accidentally broken.
        let opts = integrity_on();
        assert!(opts.write_sidecar);
        assert!(opts.verify_on_read);
    }

    #[test]
    fn tomlctl_root_env_wins_over_git_toplevel() {
        // Serialise env-mutation so parallel tests don't race on the same var.
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        // SAFETY: set_var is unsafe in edition 2024; acceptable inside tests
        // where we hold the env lock.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", canonical.as_os_str());
        }
        let got = repo_or_cwd_root().unwrap();
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
        assert_eq!(got, canonical);
    }

    #[test]
    fn with_exclusive_lock_contention_times_out() {
        use std::sync::mpsc;
        use std::thread;
        use std::time::{Duration, Instant};

        let _guard = env_lock();
        // Short timeout so the test finishes quickly.
        unsafe {
            std::env::set_var("TOMLCTL_LOCK_TIMEOUT", "1");
        }

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ledger.toml");
        fs::write(&target, LEDGER).unwrap();

        // Thread A takes the lock and sleeps long enough for thread B to
        // hit its own timeout.
        let (a_ready_tx, a_ready_rx) = mpsc::channel();
        let (b_done_tx, b_done_rx) = mpsc::channel();
        let target_a = target.clone();
        let a = thread::spawn(move || {
            with_exclusive_lock(&target_a, || {
                a_ready_tx.send(()).unwrap();
                // Hold the lock longer than B's timeout budget.
                thread::sleep(Duration::from_millis(3_000));
                Ok(())
            })
            .unwrap();
        });
        a_ready_rx.recv().unwrap();

        let target_b = target.clone();
        let b = thread::spawn(move || {
            let started = Instant::now();
            let res: Result<()> = with_exclusive_lock(&target_b, || Ok(()));
            b_done_tx.send(started.elapsed()).unwrap();
            res
        });

        let b_elapsed = b_done_rx.recv().unwrap();
        let b_res = b.join().unwrap();
        a.join().unwrap();

        unsafe {
            std::env::remove_var("TOMLCTL_LOCK_TIMEOUT");
        }

        assert!(b_res.is_err(), "thread B must time out under contention");
        // With a 1-second timeout we should be done well under 3s (the hold).
        assert!(
            b_elapsed < Duration::from_millis(2_500),
            "B took {:?}, expected < 2.5s under a 1s lock timeout",
            b_elapsed
        );
    }

    #[test]
    fn guard_write_path_rejects_outside_claude_by_default() {
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        // Anchor containment at the tempdir so `.claude/` becomes tempdir/.claude.
        let canonical = dir.path().canonicalize().unwrap();
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", canonical.as_os_str());
        }
        // Path outside `.claude/` — refused when allow_outside=false.
        let outside = canonical.join("outside.toml");
        fs::write(&outside, "x = 1\n").unwrap();
        let refused = guard_write_path(&outside, false);
        // With --allow-outside the same call succeeds.
        let allowed = guard_write_path(&outside, true);

        // Path inside `.claude/` — permitted.
        let inside_dir = canonical.join(".claude");
        fs::create_dir_all(&inside_dir).unwrap();
        let inside = inside_dir.join("ledger.toml");
        fs::write(&inside, "x = 1\n").unwrap();
        let inside_ok = guard_write_path(&inside, false);

        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }

        assert!(
            refused.is_err(),
            "path outside .claude/ must be refused without --allow-outside"
        );
        assert!(
            allowed.is_ok(),
            "path outside .claude/ must be permitted with --allow-outside"
        );
        assert!(
            inside_ok.is_ok(),
            "path inside .claude/ must be permitted without --allow-outside"
        );
    }
}
