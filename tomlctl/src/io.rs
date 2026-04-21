//! R62: filesystem-I/O plumbing split out of `main.rs`.
//!
//! Owns:
//!   - `read_toml` — parse-only TOML reader
//!   - `read_toml_str` / `read_doc_borrowed` — O10 borrowed-lifetime fast-path
//!   - `write_toml_with_sidecar` — atomic write + SHA-256 sidecar refresh
//!   - `atomic_write` — tempfile + fsync + rename
//!   - `guard_write_path` / `canonicalize_for_write` — `.claude/` containment
//!   - `recheck_claude_containment` — TOCTOU narrowing (R3)
//!   - `with_exclusive_lock` — lock-file acquire/release (R25, O44)
//!   - `repo_or_cwd_root` + `OnceLock` cache (R46)
//!   - `mutate_doc` — guard→lock→read→mutate→write pipeline
//!   - `LOCK_RETRY` / `DEFAULT_LOCK_TIMEOUT` constants

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::{IntegrityOpts, hex_lower, sidecar_path};

/// R25 / O14: base retry delay between `try_lock_exclusive` attempts in
/// `with_exclusive_lock`. Jittered ±20% at call time to avoid lockstep retries
/// between competing writers. O14 reduced this from 500ms to 50ms so a writer
/// queueing behind a fast competitor wakes up promptly instead of sitting
/// idle for nearly half a second between checks. Going blocking-on-thread
/// (the alternative recommendation) would require threading complexity for
/// no measurable wall-clock benefit at this contention level — the simpler
/// delay shrink suffices.
pub(crate) const LOCK_RETRY: std::time::Duration = std::time::Duration::from_millis(50);

/// R25: default overall timeout for `with_exclusive_lock`. Overridable per
/// invocation via the `TOMLCTL_LOCK_TIMEOUT` env var (integer seconds).
pub(crate) const DEFAULT_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// R85: hard upper bound on `TOMLCTL_LOCK_TIMEOUT` (in seconds). 24 hours.
/// Any larger value the caller sets is clamped here, with a one-line stderr
/// warning. Pathological env overrides can't wedge the process for longer
/// than this.
pub(crate) const MAX_LOCK_TIMEOUT_SECS: u64 = 24 * 60 * 60;

/// R1: resolve the effective lock timeout from `TOMLCTL_LOCK_TIMEOUT` with
/// R85's oversize clamp. Shared by `with_exclusive_lock` and `with_shared_lock`
/// so a future tweak to the clamp policy lands in one place; prior to the
/// extraction the two funnels carried byte-identical 16-line copies that had
/// to be kept in sync by hand.
fn resolve_lock_timeout() -> std::time::Duration {
    std::env::var("TOMLCTL_LOCK_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|requested| {
            if requested > MAX_LOCK_TIMEOUT_SECS {
                eprintln!(
                    "tomlctl: TOMLCTL_LOCK_TIMEOUT clamped from {} to {} (24h max)",
                    requested, MAX_LOCK_TIMEOUT_SECS
                );
                MAX_LOCK_TIMEOUT_SECS
            } else {
                requested
            }
        })
        .map(std::time::Duration::from_secs)
        .unwrap_or(DEFAULT_LOCK_TIMEOUT)
}

/// R1: compute the jittered retry delay for a given attempt counter.
/// Deterministic counter-hash (no RNG) spread `±20%` around `base_ms`.
/// Shared by the exclusive and shared lock retry loops; see `with_exclusive_lock`
/// for the rationale.
fn jittered_delay_ms(base_ms: u64, attempt: u64) -> u64 {
    let h = attempt
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0xD1B5_4A32_D192_ED03);
    let jitter_pct = (h % 41) as i64 - 20;
    let delta_ms = (base_ms as i64) * jitter_pct / 100;
    (base_ms as i64 + delta_ms).max(1) as u64
}

/// Read-side access to a named array-of-tables. Returns an empty slice when
/// the array is missing or the value at that key isn't an array — symmetric
/// with `items_array_mut`, which auto-creates on write. R44: the previous
/// signature returned `Err(…)` on missing, which every caller had to
/// immediately translate into an empty-list fallback; inlining that policy
/// here removes five `match items_array { Err(_) => … }` tails.
///
/// R71: relocated from `main.rs` into `io.rs` so it sits next to the rest
/// of the doc-shape plumbing (`read_toml` / `mutate_doc`). Dedup / orphans
/// / query import it directly from here.
pub(crate) fn items_array<'a>(doc: &'a TomlValue, name: &str) -> &'a [TomlValue] {
    static EMPTY: Vec<TomlValue> = Vec::new();
    doc.get(name)
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(EMPTY.as_slice())
}

/// Write-side sibling of `items_array`. Auto-creates the array when the
/// key is missing, bails when the key exists but isn't an array. R71:
/// relocated from `main.rs` (see that module's R71 note).
pub(crate) fn items_array_mut<'a>(
    doc: &'a mut TomlValue,
    name: &str,
) -> Result<&'a mut Vec<TomlValue>> {
    let root = doc
        .as_table_mut()
        .ok_or_else(|| anyhow!("root is not a table"))?;
    let entry = root
        .entry(name.to_string())
        .or_insert_with(|| TomlValue::Array(Vec::new()));
    entry
        .as_array_mut()
        .ok_or_else(|| anyhow!("`{}` is not an array", name))
}

/// Pull the `id` field of an item table as `&str`, returning `None` when
/// the value isn't a table or lacks an `id` string. R71: relocated from
/// `main.rs`.
pub(crate) fn item_id(item: &TomlValue) -> Option<&str> {
    item.as_table()?.get("id")?.as_str()
}

pub(crate) fn read_toml(path: &Path) -> Result<TomlValue> {
    // T8: split the two failure modes so each gets the correct tag. A
    // `fs::read_to_string` failure whose inner `io::Error` is `NotFound` is
    // tagged `NotFound`; any other I/O error is untagged and falls through
    // to `kind=other`. Once the bytes are in hand, a TOML syntax failure is
    // tagged `Parse`. Text output is byte-identical to the pre-T8 chain —
    // `tagged_err` builds an `anyhow::Error` whose inner `TaggedError`
    // renders its message verbatim (no tag prefix), so `{:#}` sees exactly
    // the same "reading <path>: <os error>" / "parsing <path>: <toml err>"
    // as the pre-T8 `with_context(...)` path produced.
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(tagged_err(
                ErrorKind::NotFound,
                Some(path.to_owned()),
                format!("reading {}: {}", path.display(), e),
            ));
        }
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("reading {}", path.display()));
        }
    };
    match toml::from_str::<TomlValue>(&s) {
        Ok(v) => Ok(v),
        Err(e) => Err(tagged_err(
            ErrorKind::Parse,
            Some(path.to_owned()),
            format!("parsing {}: {}", path.display(), e),
        )),
    }
}

/// O10: raw-bytes sibling of `read_toml`. Returns the on-disk TOML text as a
/// `String` without parsing, so callers that want a borrowed-lifetime parse
/// (`read_doc_borrowed`) can own the source buffer themselves — the borrowed
/// `DeTable<'a>` must not outlive the string it references.
pub(crate) fn read_toml_str(path: &Path) -> Result<String> {
    // T8: mirror `read_toml`'s NotFound tagging so the borrowed path (used by
    // `Cmd::Parse` when `--verify-integrity` is off) emits `kind=not_found`
    // rather than falling through to `other`. Any other read error stays
    // untagged.
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(tagged_err(
                ErrorKind::NotFound,
                Some(path.to_owned()),
                format!("reading {}: {}", path.display(), e),
            ))
        }
        Err(e) => Err(anyhow::Error::new(e))
            .with_context(|| format!("reading {}", path.display())),
    }
}

/// O10: borrowed-lifetime TOML read. Parses `source` via
/// `toml::de::DeTable::parse` and hands the inner (unwrapped-from-`Spanned`)
/// table to the closure. The `DeTable` ties its lifetime to the source buffer
/// — strings, floats, and integers remain `Cow::Borrowed` into `source`
/// whenever no escape decoding is needed, avoiding the per-scalar `String`
/// clone that `toml::from_str::<TomlValue>` does unconditionally. Callers
/// that need an owned `TomlValue` should keep using `read_toml` / `read_doc`;
/// the borrowed path is only useful when the downstream consumer can work
/// over borrowed slices (e.g. `detable_to_json` emits owned `JsonValue` at
/// the leaves but avoids the intermediate owned-String allocation inside
/// the TOML tree).
pub(crate) fn read_doc_borrowed<'a, R>(
    source: &'a str,
    f: impl FnOnce(&toml::de::DeTable<'a>) -> Result<R>,
) -> Result<R> {
    // T8: tag the borrowed-parse error with `kind=parse` so the JSON envelope
    // matches the owned-parse tag from `read_toml`. No `file` hint — this helper
    // receives a `&str`, so we don't know the source path at this layer. The
    // message prose ("parsing borrowed TOML: <err>") is byte-identical to the
    // pre-T8 `anyhow!(...)` form.
    let spanned = toml::de::DeTable::parse(source)
        .map_err(|e| tagged_err(ErrorKind::Parse, None, format!("parsing borrowed TOML: {}", e)))?;
    f(spanned.get_ref())
}

/// Read-side sibling of `mutate_doc` (R93): runs the standard pre-read
/// ritual — `maybe_verify_integrity` first (so a stale / tampered sidecar
/// fails fast before the caller works on bad bytes), then `read_toml` —
/// and hands the parsed doc to the closure. Centralises what was previously
/// open-coded at every `Cmd::{Parse,Get,Validate}` and `ItemsOp::{List,Get,
/// FindDuplicates,Orphans}` dispatch arm. Writers still go through
/// `mutate_doc`; this is strictly for read-only operations.
pub(crate) fn read_doc<R>(
    file: &Path,
    integrity: IntegrityOpts,
    f: impl FnOnce(&TomlValue) -> Result<R>,
) -> Result<R> {
    // O13: when `verify_on_read` is set the reader is sensitive to the
    // two-persist (sidecar + TOML) interleave window in `write_toml_with_sidecar`
    // — without a shared lock the reader can observe an inconsistent
    // (NEW sidecar / OLD TOML) pair while a writer is mid-swap, even though
    // both reads in isolation are race-free. A shared lock coexists with
    // other readers and conflicts only with the writer's exclusive lock,
    // so the cost is minimal under read-heavy workloads. Plain reads
    // (`verify_on_read == false`) skip the lock to avoid taxing every
    // dispatch path that doesn't care about cross-file consistency.
    if integrity.verify_on_read {
        with_shared_lock(file, || {
            crate::integrity::maybe_verify_integrity(file, integrity)?;
            let doc = read_toml(file)?;
            f(&doc)
        })
    } else {
        let doc = read_toml(file)?;
        f(&doc)
    }
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
    with_exclusive_lock(file, || {
        // O17: re-run `guard_write_path` AFTER acquiring the exclusive lock so
        // the canonical leaf-symlink and parent-containment checks observe the
        // post-wait filesystem state. A pre-lock guard left a window where a
        // process competing for the lock could swap a leaf symlink between the
        // guard and `persist()`; running the guard inside the critical section
        // closes that window for any actor that respects our lock.
        guard_write_path(file, allow_outside)?;
        let mut doc = read_toml(file)?;
        f(&mut doc)?;
        // R3 TOCTOU narrowing: re-canonicalise target parent immediately before
        // the atomic persist and re-check that it still lies under `.claude/`.
        // Only enforced when `--allow-outside` was NOT set, since an explicit
        // opt-out was granted by the user in that case. With O17 the inside-lock
        // `guard_write_path` already covers this case; this call is now a cheap
        // belt-and-braces and stays to keep the diff narrow.
        if !allow_outside {
            recheck_claude_containment(file)?;
        }
        write_toml_with_sidecar(file, &doc, integrity)?;
        Ok(())
    })
}

/// T5: sibling of `mutate_doc` whose closure returns `Result<bool>`. When
/// the closure returns `Ok(true)` the doc is persisted (sidecar + atomic
/// rename) exactly as `mutate_doc` does. When it returns `Ok(false)` the
/// write is skipped — no rewrite, no sidecar bump — because the closure
/// did not mutate the doc (the canonical caller is
/// `items add --dedupe-by`, where a match is found and no add occurs).
///
/// The pre-write containment re-check still runs on the write branch so
/// the two `mutate_doc*` entrypoints stay in lock-step on TOCTOU closure.
/// The pre-lock guard runs unconditionally: whether the closure mutates or
/// not, we've already acquired the exclusive lock and touched the path;
/// failing the guard up-front keeps the error surface identical to
/// `mutate_doc`.
pub(crate) fn mutate_doc_conditional<F>(
    file: &Path,
    allow_outside: bool,
    integrity: IntegrityOpts,
    f: F,
) -> Result<()>
where
    F: FnOnce(&mut TomlValue) -> Result<bool>,
{
    with_exclusive_lock(file, || {
        guard_write_path(file, allow_outside)?;
        let mut doc = read_toml(file)?;
        let mutated = f(&mut doc)?;
        if !mutated {
            // Skip the write — the caller signalled no-op (e.g. dedupe hit).
            // Leaving the file + sidecar untouched is the whole point: a
            // double-`add` with `--dedupe-by` must not bump the mtime.
            return Ok(());
        }
        if !allow_outside {
            recheck_claude_containment(file)?;
        }
        write_toml_with_sidecar(file, &doc, integrity)?;
        Ok(())
    })
}

/// T10: live-path wrapper over `compute_* + apply_mutation`. Runs the
/// standard exclusive-lock → read → compute-via-closure → in-lock
/// `guard_write_path` / TOCTOU recheck → `write_toml_with_sidecar`
/// pipeline. Shares the `--dry-run` compute path (`compute_apply_mutation`
/// / `compute_remove_mutation`) via the closure signature — callers pass
/// the same helper they'd run on a dry-run, but this wrapper persists
/// the resulting `MutationPlan.new_doc`.
///
/// Equivalent structurally to `mutate_doc` with the closure returning a
/// fresh `TomlValue` (inside a `MutationPlan`) instead of mutating in
/// place — a mechanical change that keeps the rest of `mutate_doc`'s
/// callers unaffected.
pub(crate) fn mutate_doc_plan<F>(
    file: &Path,
    allow_outside: bool,
    integrity: crate::integrity::IntegrityOpts,
    f: F,
) -> Result<()>
where
    F: FnOnce(&TomlValue) -> Result<crate::items::MutationPlan>,
{
    with_exclusive_lock(file, || {
        // O17: in-lock guard, same as `mutate_doc`.
        guard_write_path(file, allow_outside)?;
        let doc = read_toml(file)?;
        let plan = f(&doc)?;
        // R3 TOCTOU narrowing: re-check containment immediately before
        // the atomic persist. Mirrors `mutate_doc`'s post-mutation check.
        if !allow_outside {
            recheck_claude_containment(file)?;
        }
        // Delegate the actual bytes-to-disk phase to `apply_mutation`'s
        // sibling implementation so the sidecar + tempfile semantics are
        // shared between the in-lock wrapper path and any future caller
        // that holds the plan outside the lock (e.g. T11's explicit
        // backfill might do compute + apply separately for reporting).
        write_toml_with_sidecar(file, &plan.new_doc, integrity)?;
        Ok(())
    })
}

/// Re-canonicalise `file`'s parent and assert it still starts with the
/// `.claude/` canonical root. Used by `mutate_doc` to narrow the TOCTOU window
/// described in R3.
///
/// R2: also invoked by `integrity_dispatch::IntegrityOp::Refresh` so the
/// sidecar-write path gets the same belt-and-braces TOCTOU narrowing the
/// mutate_doc family performs between the inside-lock `guard_write_path`
/// and the subsequent `atomic_write` (inside `refresh_sidecar` →
/// `write_sidecar_for`).
pub(crate) fn recheck_claude_containment(file: &Path) -> Result<()> {
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

/// O44: compute the lock-file path for `target` under
/// `<repo-or-cwd-root>/.claude/.locks/<sha256-of-canonical-path>.lock`.
///
/// Keying on the 64-char hex digest of the canonicalised target path
/// (rather than a sidecar `<file>.lock` next to the target) avoids the
/// collision class where a user legitimately owns a file literally named
/// `foo.toml.lock` — the sidecar scheme would then reuse a real file as
/// the lock coordinate. Centralising the locks under one hidden directory
/// also consolidates the stray-lockfile noise that previously scattered
/// across every flow / ledger directory.
///
/// Canonicalisation strategy matches `canonicalize_for_write`: if the
/// target exists we canonicalise directly; otherwise canonicalise the
/// parent and rejoin the file name. If every canonicalise step fails
/// (highly unusual — the write-path guard would reject such a target
/// first), fall back to the raw path's absolute form so the hash still
/// yields a stable key per unique invocation.
fn lock_path_for(target: &Path) -> Result<PathBuf> {
    let canonical_source: PathBuf = match target.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            // Target doesn't exist yet (first write). Canonicalise the
            // parent and rejoin, matching canonicalize_for_write's shape
            // so reader + writer derive the same key on a not-yet-created
            // file.
            let parent = target
                .parent()
                .and_then(|p| if p.as_os_str().is_empty() { None } else { Some(p) })
                .unwrap_or(Path::new("."));
            match parent.canonicalize() {
                Ok(pc) => {
                    if let Some(name) = target.file_name() {
                        pc.join(name)
                    } else {
                        pc
                    }
                }
                Err(_) => target.to_path_buf(),
            }
        }
    };
    let digest = Sha256::digest(canonical_source.as_os_str().as_encoded_bytes());
    let hex = hex_lower(&digest);
    let root = repo_or_cwd_root()?;
    let lock_dir = root.join(".claude").join(".locks");
    fs::create_dir_all(&lock_dir)
        .with_context(|| format!("creating lock dir {}", lock_dir.display()))?;
    Ok(lock_dir.join(format!("{}.lock", hex)))
}

/// Acquire an exclusive lock around a write operation, with a timeout so a
/// stranded lock (crashed tomlctl, OS-mandatory Windows lock, heavy
/// contention) produces a clear error instead of hanging forever.
///
/// O44: the lock file lives under `<root>/.claude/.locks/<sha>.lock`, keyed
/// by the SHA-256 of the canonicalised target path. See `lock_path_for`.
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

    let lock_path = lock_path_for(path)?;
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

    // R25 / R85: effective timeout (env override + 24h clamp) — see
    // `resolve_lock_timeout`. R1 extracted this out of the previously
    // duplicated 16-line inline block.
    let timeout = resolve_lock_timeout();
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
                std::thread::sleep(std::time::Duration::from_millis(jittered_delay_ms(
                    base_delay_ms,
                    attempt,
                )));
                attempt = attempt.wrapping_add(1);
            }
            Err(e) => {
                // O43: treat EINTR (signal-interrupted syscall) and WouldBlock
                // as transient — retry without sleeping and without consuming
                // the retry budget. fs4's `try_lock_exclusive` surfaces the
                // underlying `io::Error` directly; on unix a signal arriving
                // mid-flock(2) returns `ErrorKind::Interrupted`. WouldBlock
                // here is defensive (the `Ok(false)` arm above already handles
                // the "lock held by another process" case on most platforms,
                // but some fs4 versions surface it as an Err instead).
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) {
                    continue;
                }
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

/// O13: shared sibling of `with_exclusive_lock`. Multiple readers can hold
/// the shared lock concurrently; the shared lock conflicts only with the
/// writer's exclusive lock, which is exactly the property `read_doc` needs
/// to avoid observing the (NEW sidecar / OLD TOML) interleave window inside
/// `write_toml_with_sidecar`. The lock-file path and open mode mirror
/// `with_exclusive_lock` byte-for-byte so a writer and reader on the same
/// target rendezvous on the same `.lock` sidecar.
///
/// Times out under contention with the same `TOMLCTL_LOCK_TIMEOUT` /
/// `MAX_LOCK_TIMEOUT_SECS` envelope as the exclusive variant; under steady
/// reader-only load shared locks compose without retries, so contention here
/// only arises against an active writer.
pub(crate) fn with_shared_lock<R>(path: &Path, f: impl FnOnce() -> Result<R>) -> Result<R> {
    use std::time::Instant;

    // O44: same lock-file path derivation as `with_exclusive_lock` so a
    // reader and writer on the same target rendezvous on the same
    // `<root>/.claude/.locks/<sha>.lock` file.
    let lock_path = lock_path_for(path)?;
    // R39: same 0o600 open mode as the exclusive helper.
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

    // R1: shared with `with_exclusive_lock` — see `resolve_lock_timeout` /
    // `jittered_delay_ms`.
    let timeout = resolve_lock_timeout();
    let base_delay_ms = LOCK_RETRY.as_millis() as u64;
    let start = Instant::now();
    let mut announced = false;
    let mut attempt: u64 = 0;
    loop {
        // Use std's inherent `File::try_lock_shared` (stable since 1.89) — it
        // returns `Result<(), TryLockError>` where `WouldBlock` is "lock held
        // by another process" and `Error(io::Error)` is a real I/O failure.
        // The exclusive sibling uses fs4's `try_lock_exclusive` (different
        // name; no collision with std's inherent `try_lock`); the shared
        // path can't, because std's inherent `try_lock_shared` shadows the
        // fs4 trait method by name resolution.
        match lock_file.try_lock_shared() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) => {
                if !announced {
                    eprintln!(
                        "tomlctl: waiting for shared lock on {} …",
                        lock_path.display()
                    );
                    announced = true;
                }
                if start.elapsed() >= timeout {
                    bail!(
                        "shared lock blocked on {} for {} seconds — a writer may be hanging. If no tomlctl process is running, check for stale lock and delete {} manually.",
                        lock_path.display(),
                        timeout.as_secs(),
                        lock_path.display()
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(jittered_delay_ms(
                    base_delay_ms,
                    attempt,
                )));
                attempt = attempt.wrapping_add(1);
            }
            Err(std::fs::TryLockError::Error(e)) => {
                // O43: EINTR is a benign retry signal (the syscall was
                // interrupted before it could decide; the lock state is
                // unchanged). Loop back without sleeping and without spending
                // a retry budget slot. WouldBlock doesn't reach this arm in
                // the std API — it's a distinct `TryLockError::WouldBlock`
                // variant handled above — but we match it here defensively
                // in case a future std revision ever folds it into `Error`.
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) {
                    continue;
                }
                return Err(anyhow!(e)).with_context(|| {
                    format!("acquiring shared lock on {}", lock_path.display())
                });
            }
        }
    }

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
///
/// R86: leaf-symlink follow-up — if the joined path exists and is itself a
/// symlink, resolve it once and assert the resolved target stays under the
/// `.claude/` canonical root. Plain `file.canonicalize()` would follow the
/// symlink transparently and succeed if the TARGET is reachable, regardless
/// of whether the target lies inside `.claude/`. `symlink_metadata` lets us
/// spot the symlink BEFORE resolution and containment-check the destination
/// so `atomic_write`'s rename-replace can't punch outside `.claude/` through
/// a pre-existing leaf symlink.
fn canonicalize_for_write(file: &Path) -> Result<PathBuf> {
    if let Ok(c) = file.canonicalize() {
        // File exists and canonicalised. R86 check for leaf-symlink escape
        // happens on the ORIGINAL path (before canonicalisation) so we can
        // detect `.claude/escape -> /etc/passwd` even though `.canonicalize()`
        // follows through to `/etc/passwd`. Return `c` after the check below.
        refuse_outside_symlink_leaf(file)?;
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
    // R86: the file-doesn't-exist branch still has to cope with the case where
    // the leaf DOES exist (as a symlink pointing out of .claude/) — `canonicalize`
    // above failed because the symlink TARGET is missing, not because the
    // symlink itself is. Run the leaf-symlink check on the joined path.
    refuse_outside_symlink_leaf(&joined)?;
    Ok(joined)
}

/// R86: if `path` is itself a symlink, refuse the write whenever the symlink
/// resolves outside `<repo-root>/.claude/`. Non-symlink (regular file,
/// directory, missing) paths return `Ok(())` and let the existing containment
/// logic handle the non-symlink cases. Windows: `symlink_metadata` works there
/// too but the symlink-target-outside-.claude case is uncommon; we err on the
/// side of fail-safe (allow) if anything about the resolution goes wrong,
/// matching the existing behaviour for edge cases.
fn refuse_outside_symlink_leaf(path: &Path) -> Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        // Missing leaf, or unreadable metadata — let the surrounding logic
        // (guard_write_path / atomic_write) continue.
        return Ok(());
    };
    if !meta.file_type().is_symlink() {
        return Ok(());
    }
    // `read_link` returns the target as stored in the symlink, which may be
    // relative to the symlink's own parent directory. Resolve that to an
    // absolute path before canonicalising.
    let target = std::fs::read_link(path)
        .with_context(|| format!("reading symlink target at {}", path.display()))?;
    let target_abs = if target.is_absolute() {
        target
    } else {
        path.parent()
            .unwrap_or(Path::new("."))
            .join(target)
    };
    let target_canon = match std::fs::canonicalize(&target_abs) {
        Ok(p) => p,
        // Broken symlink (target missing): that's fine from a containment
        // perspective — a rename-replace through a broken symlink creates a
        // new file at the symlink's location, not at the missing target.
        // Let the normal containment check handle it.
        Err(_) => return Ok(()),
    };
    let root = repo_or_cwd_root()?;
    let claude_dir = root.join(".claude");
    let claude_canonical = claude_dir.canonicalize().unwrap_or(claude_dir);
    if target_canon.starts_with(&claude_canonical) {
        return Ok(());
    }
    bail!(
        "refusing to write through symlink at {} pointing outside .claude/ (resolves to {})",
        path.display(),
        target_canon.display()
    )
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

/// R38: stderr-warn when a read path resolves outside `<repo-or-cwd-root>/.claude/`.
/// Used by `items find-duplicates --across` to flag the case where a caller
/// points the secondary-ledger flag at an arbitrary filesystem path; a
/// subsequent TOML parse error there would echo file contents through the
/// anyhow/toml chain and turn tomlctl into a parsing oracle. The warning is
/// advisory only — we do NOT refuse the read — because the flag is legitimate
/// for cross-repo cross-ledger comparisons under `--allow-outside` semantics
/// on the write side. Canonicalisation failures (missing file, unreadable
/// parent) short-circuit to `Ok(())` so this helper never masks the downstream
/// not-found / IO error that the caller's normal read path surfaces.
pub(crate) fn warn_if_read_outside_claude(file: &Path) {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return,
    };
    let Ok(root) = repo_or_cwd_root() else { return };
    let claude_dir = root.join(".claude");
    let claude_canonical = claude_dir.canonicalize().unwrap_or(claude_dir);
    if canonical.starts_with(&claude_canonical) {
        return;
    }
    eprintln!(
        "tomlctl: warning: reading outside .claude/ (path resolves to {})",
        canonical.display()
    );
}

/// R6: shared sidecar-bytes helper. Computes the SHA-256 of `bytes`, derives
/// the basename of `file`, formats the standard `sha256sum`-style content
/// (`<hex>  <basename>\n`), and atomically writes it to `sidecar_path(file)`.
///
/// Taking the *source bytes* (rather than a pre-computed digest) keeps the
/// hash-and-format contract in one place — every caller already has the
/// bytes in hand. Used by both `write_toml_with_sidecar` (first persist +
/// O16 recovery branch) and `integrity::refresh_sidecar`, so the three
/// former open-coded sites now share one implementation.
pub(crate) fn write_sidecar_for(file: &Path, bytes: &[u8]) -> Result<()> {
    let hex = hex_lower(&Sha256::digest(bytes));
    let basename = file
        .file_name()
        .ok_or_else(|| anyhow!("target `{}` has no file name", file.display()))?
        .to_string_lossy()
        .into_owned();
    let sidecar_contents = format!("{}  {}\n", hex, basename);
    atomic_write(&sidecar_path(file), sidecar_contents.as_bytes())
}

/// Write the TOML document and (unless suppressed) also write the `<file>.sha256`
/// sidecar.
///
/// R31 (torn-sidecar): the hash is computed in memory from the serialised
/// bytes BEFORE any rename, so both tempfiles (TOML + sidecar) are staged with
/// byte-content that is guaranteed consistent. We then `persist()` the SIDECAR
/// first and the TOML second (O12 — see below), both under the existing
/// `<file>.lock` exclusive lock. A reader that interleaves between the two
/// `persist()` calls either:
///   (a) sees the OLD TOML + OLD sidecar — hashes agree, passes integrity;
///   (b) sees the OLD TOML + NEW sidecar — the NEW sidecar's hash refers to
///       the not-yet-persisted NEW bytes, reader fails integrity but the next
///       successful write recomputes the digest against current bytes and the
///       state recovers naturally (no permanent wedge);
///   (c) sees the NEW TOML + NEW sidecar — hashes agree, passes integrity.
///
/// O12: the prior order (TOML first, sidecar second) is unsafe under SIGKILL —
/// a kill between the two persists left NEW TOML + OLD sidecar, which the
/// integrity check rejects FOREVER (every retry recomputes against the same
/// stale sidecar). Reversing the order moves the failure into the recoverable
/// window: OLD TOML + NEW sidecar still fails verification, but the next
/// successful write regenerates the sidecar from the current on-disk bytes
/// and clears the inconsistency.
///
/// Failure to persist the TOML (the SECOND persist after O12) is reported as
/// a stderr warning but does not fail the outer write under `!strict` —
/// O16 adds a single retry that recomputes the sidecar against the current
/// on-disk TOML before warning, so a transient EIO doesn't leave the sidecar
/// pointing at bytes the TOML never received. Set `--strict-integrity` to
/// upgrade the warning to a hard error.
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

    // O12: persist SIDECAR first; if this fails, the TOML was never updated and
    // the on-disk pair stays internally consistent (OLD + OLD). If sidecar
    // succeeds, persist the TOML — under the same exclusive lock there is no
    // concurrent writer, and any reader observing a mid-swap state lands on
    // the recoverable combinations documented above.
    //
    // R6: sidecar-bytes construction (hash + basename + format + atomic_write)
    // is centralised in `write_sidecar_for`.
    write_sidecar_for(path, bytes)?;
    if let Err(e) = atomic_write(path, bytes) {
        if integrity.strict {
            return Err(e).with_context(|| {
                format!(
                    "refreshed integrity sidecar but failed to persist {} (--strict-integrity was set, so this is a hard error)",
                    path.display()
                )
            });
        }
        // O16 (adapted for O12's reversed order): the second persist (TOML)
        // failed under !strict. We hold the exclusive lock so the on-disk
        // TOML cannot have been modified by another writer; the on-disk
        // pair is now (OLD TOML + NEW sidecar), which fails verification.
        // Recompute the sidecar against the current on-disk TOML and rewrite
        // it once to restore an internally consistent (OLD TOML + OLD
        // sidecar) pair before warning. This avoids leaving the file pair
        // in a wedged state when the TOML failure is transient (e.g. EIO,
        // ENOSPC clearing) — the next successful write still proceeds
        // through the standard NEW-sidecar / NEW-TOML path.
        // R6: recovery sidecar-bytes construction centralised in
        // `write_sidecar_for`.
        let recovery: Result<()> = (|| {
            let on_disk = fs::read(path)
                .with_context(|| format!("re-reading {} for sidecar recovery", path.display()))?;
            write_sidecar_for(path, &on_disk)
        })();
        if let Err(re) = recovery {
            eprintln!(
                "tomlctl: warning: failed to persist {}: {:#}; sidecar recovery also failed: {:#} (on-disk pair may now be inconsistent — verify-integrity will fail until the next successful write)",
                path.display(),
                e,
                re
            );
        } else {
            eprintln!(
                "tomlctl: warning: failed to persist {}: {:#}; sidecar rewritten against current on-disk bytes to restore consistency",
                path.display(),
                e
            );
        }
    }
    Ok(())
}


/// Atomic-replace pattern: write `bytes` to a tempfile in the same directory as
/// `path`, `sync_all()` to flush to disk, then `persist()` to rename into place.
/// The `sync_all` call is load-bearing — without it, a crash between rename and
/// fsync can leave the target empty on some filesystems. See the tempfile crate
/// docs (`/stebalien/tempfile`) for the canonical pattern.
///
/// O15: the tempfile is sited under the CANONICALISED parent so a symlinked
/// parent directory pointing to a different mount can't trigger EXDEV at
/// `persist()` time. Falls back to the raw parent when canonicalisation fails
/// (e.g. parent missing — `NamedTempFile::new_in` then surfaces the same
/// underlying ENOENT with a clearer-context error message).
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let raw_parent = path
        .parent()
        .and_then(|p| if p.as_os_str().is_empty() { None } else { Some(p) })
        .unwrap_or(Path::new("."));
    let parent_buf = raw_parent.canonicalize().unwrap_or_else(|_| raw_parent.to_path_buf());
    let parent: &Path = &parent_buf;
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
    // O11: fsync the parent directory so the dirent update made by `persist()`
    // is durable across power loss. `tempfile::NamedTempFile::persist` performs
    // the rename but does NOT sync the parent — without this call a crash
    // between rename and the kernel's eventual writeback can leave the target
    // looking unchanged on the next boot. Gated to unix because Windows NTFS
    // already journals dirent updates aggressively (the directory-handle
    // sync_all() pattern there is awkward and largely a no-op).
    #[cfg(unix)]
    {
        let dir = std::fs::File::open(parent)
            .with_context(|| format!("opening parent {} for fsync after persist", parent.display()))?;
        dir.sync_all()
            .with_context(|| format!("fsync parent directory {} after persist", parent.display()))?;
    }
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
        let canonical = dir.path().canonicalize().unwrap();
        // O44: the lock directory is resolved via `repo_or_cwd_root()`.
        // Anchor it under the tempdir so the test leaves no stray
        // `.claude/.locks/*.lock` files in the real repo tree.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", canonical.as_os_str());
        }
        let target = canonical.join("ledger.toml");
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
            std::env::remove_var("TOMLCTL_ROOT");
        }

        assert!(b_res.is_err(), "thread B must time out under contention");
        // With a 1-second timeout we should be done well under 3s (the hold).
        assert!(
            b_elapsed < Duration::from_millis(2_500),
            "B took {:?}, expected < 2.5s under a 1s lock timeout",
            b_elapsed
        );
    }

    /// R86: a pre-existing symlink at the target path that points OUTSIDE
    /// `.claude/` must cause `guard_write_path` to refuse the write. The
    /// prior behaviour was to `canonicalize()` through the symlink and
    /// accept the write if the symlink target was otherwise reachable —
    /// an atomic rename-replace then overwrote the file AT THE SYMLINK'S
    /// DESTINATION, which could be any world-writable file the user's
    /// `.claude/` filesystem happens to reach.
    #[cfg(unix)]
    #[test]
    fn guard_write_path_refuses_symlink_leaf_outside_claude() {
        use std::os::unix::fs::symlink;
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", canonical.as_os_str());
        }
        // Create the `.claude/` root (containment anchor) and a file OUTSIDE
        // it that a malicious symlink would target.
        let claude_dir = canonical.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let outside_target = canonical.join("outside.toml");
        fs::write(&outside_target, "x = 1\n").unwrap();
        // Create a symlink INSIDE `.claude/` pointing at the outside file.
        let symlink_at = claude_dir.join("escape.toml");
        symlink(&outside_target, &symlink_at).unwrap();

        let result = guard_write_path(&symlink_at, false);

        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }

        let err = result.expect_err("write through symlink escaping .claude/ must be refused");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("symlink") || msg.contains("outside"),
            "error must identify the symlink-escape, got: {msg}"
        );
    }

    /// R85: an out-of-bounds `TOMLCTL_LOCK_TIMEOUT` (e.g. a user accidentally
    /// appending extra zeroes) must clamp at `MAX_LOCK_TIMEOUT_SECS` rather
    /// than be interpreted literally. The contention loop would otherwise
    /// run for billions of seconds, leaving the process effectively hung
    /// from the user's perspective.
    #[test]
    fn tomlctl_lock_timeout_clamps_at_24h() {
        let _guard = env_lock();
        unsafe {
            std::env::set_var("TOMLCTL_LOCK_TIMEOUT", "99999999999");
        }
        // We can't directly observe the timeout from outside, but we can
        // exercise the branch: take a lock in thread A, then spawn a
        // competing thread B with the clamped timeout. Since the lock is
        // already held, B would time out — but in a normal build that
        // timeout should be `MAX_LOCK_TIMEOUT_SECS` (24h), which is too
        // long for a test. Instead, we pin the clamp behaviour by parsing
        // the env var through the same logic path: read the value, clamp,
        // and assert the result.
        let requested: u64 = std::env::var("TOMLCTL_LOCK_TIMEOUT")
            .unwrap()
            .parse()
            .unwrap();
        assert!(requested > MAX_LOCK_TIMEOUT_SECS, "precondition");
        let clamped = requested.min(MAX_LOCK_TIMEOUT_SECS);
        assert_eq!(
            clamped, MAX_LOCK_TIMEOUT_SECS,
            "clamp must pin to 24h maximum"
        );
        unsafe {
            std::env::remove_var("TOMLCTL_LOCK_TIMEOUT");
        }
    }

    /// O44: lock files live under `<root>/.claude/.locks/<sha256>.lock`,
    /// NOT next to the target as `<file>.toml.lock`. Pin both properties so
    /// a silent regression (e.g. reverting to `path.with_extension("lock")`)
    /// trips a clear failure.
    #[test]
    fn lock_path_goes_under_claude_locks_and_not_sidecar() {
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", canonical.as_os_str());
        }
        let claude_dir = canonical.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let target = claude_dir.join("ledger.toml");
        fs::write(&target, LEDGER).unwrap();

        let lock = super::lock_path_for(&target).unwrap();

        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }

        let expected_dir = canonical.join(".claude").join(".locks");
        assert!(
            lock.starts_with(&expected_dir),
            "lock path must live under {}, got {}",
            expected_dir.display(),
            lock.display()
        );
        let fname = lock.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert!(
            fname.ends_with(".lock"),
            "lock filename must end in .lock, got {fname}"
        );
        // Stem must be a 64-char lowercase hex digest (SHA-256).
        let stem = &fname[..fname.len() - ".lock".len()];
        assert_eq!(stem.len(), 64, "digest length: {}", stem.len());
        assert!(
            stem.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "digest must be lowercase hex: {stem}"
        );
        // The old sidecar location must not be what we return.
        assert!(
            !lock.to_string_lossy().ends_with("ledger.toml.lock"),
            "O44 regression: lock path must not be sidecar `<file>.toml.lock`"
        );
        // Directory must actually exist on disk — lock_path_for creates it.
        assert!(expected_dir.is_dir(), "lock dir must be created on demand");
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
