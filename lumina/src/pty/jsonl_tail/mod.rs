//! JSONL-tail watcher: the canonical source of conversation messages.
//!
//! T4 of the `lumina-pty-jsonl-tail` flow. This module replaces the v1 vt100
//! row-finalisation parser (`pty/parser.rs`) — fullscreen TUI mode redraws by
//! absolute cursor positioning, so a 2D virtual screen cannot yield a coherent
//! transcript. Instead we drive `claude` interactively in the PTY (preserves
//! the subscription billing posture) and read the canonical structured
//! transcript Claude Code writes to:
//!
//! ```text
//! ~/.claude/projects/<sanitised-cwd>/<uuid>.jsonl
//! ```
//!
//! Each JSONL line is a typed, finalised, immutable event (`user`,
//! `assistant`, `summary`, etc.) — no streaming-delta assembly. We map those
//! records 1:1 onto `pty_messages` rows in the bridge task (T5).
//!
//! ## Schema stability
//!
//! Anthropic has not committed to a stable JSONL schema (see GitHub issue
//! #53516). The deserialise here is therefore deliberately tolerant: any
//! record whose top-level `type` is not in our known set, OR which fails to
//! parse against our known-variant shape, is captured as
//! [`JsonlRecordParsed::UnknownRaw`] with the raw line preserved — the
//! supervisor persists it as a `system` row and the bridge still flips the
//! Spawning → Idle gate on it.
//!
//! ## Concerns owned by this module
//!
//! 1. [`sanitise_cwd`] — reproduce Claude Code's per-cwd directory algorithm.
//! 2. [`resolve_projects_root`] — locate `~/.claude/projects` (overridable in
//!    tests via `LUMINA_PTY_PROJECTS_ROOT`, mirroring the
//!    `LUMINA_WORKTREE_ROOT` precedent in [`crate::pty::spawn`]).
//! 3. [`JsonlRecord`] + [`parse_line`] — tolerant deserialise with raw-line
//!    capture on unknown types.
//! 4. [`tail`] — the watcher task: `notify::recommended_watcher` on the
//!    parent dir, BufReader on the file, broadcast on each parsed line.
//! 5. [`bind_jsonl_path`] — snapshot-then-poll: at spawn-start we snapshot
//!    the `*.jsonl` filenames in the cwd's projects dir; then we poll for
//!    up to 5 s for the first new entry. Path-set diff (not mtime — wall
//!    clock is non-monotonic and FAT/exFAT mtime granularity is 2 s).
//!
//! Consumers (the bridge task in T5) are NOT wired here — T4 lands the
//! module standalone; `mod.rs` adds `pub mod jsonl_tail;` but no `pub use`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{
    EventKind, RecursiveMode, Watcher,
    event::{CreateKind, ModifyKind},
};
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tokio::sync::{broadcast, mpsc};

use crate::error::AppError;

mod parse;
pub use parse::*;

// ---------------------------------------------------------------------------
// File binding: snapshot-then-poll
// ---------------------------------------------------------------------------

/// Bind to the JSONL transcript Claude Code creates for an interactive
/// session.
///
/// Modern Claude Code (verified 2.1.153) HONOURS `--session-id` for the
/// JSONL filename in interactive mode too — the file is named
/// `<session_id>.jsonl` under `~/.claude/projects/<sanitised-cwd>/`. We
/// pass our v7 UUID to claude via `--session-id` in `pty_transport::spawn`,
/// so we can precompute the exact path and just wait for THAT file to
/// exist. (The earlier plan research notes citing GH #44607 — "the CLI
/// flag does not control the filename in interactive mode" — described
/// older Claude Code behaviour; the current version is fixed.)
///
/// The previous snapshot-then-diff approach had a critical correctness
/// bug: when N sessions share a cwd, every fresh `.jsonl` file appearing
/// in the dir would be claimed by EVERY waiting bind task, causing
/// cross-session record contamination (one session's bridge tailing
/// another session's JSONL). Predicting the filename eliminates the
/// race entirely.
///
/// Polling cadence: 100 ms. `timeout = None` polls indefinitely; that
/// is the default for production (real claude only writes the JSONL
/// after the user's first prompt produces a response, which can be
/// minutes for an idle session). Tests pass `Some(Duration::from_secs(5))`
/// for fast failure on truly broken setups.
pub async fn bind_jsonl_path(
    cwd: &Path,
    session_id: &str,
    timeout: Option<Duration>,
) -> Result<PathBuf, AppError> {
    let dir = resolve_projects_root().join(sanitise_cwd(cwd));
    let target = dir.join(format!("{session_id}.jsonl"));
    tracing::debug!(
        target = %target.display(),
        "jsonl_tail: bind_jsonl_path waiting for session JSONL"
    );

    let deadline = timeout.map(|t| std::time::Instant::now() + t);
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    // First tick fires immediately — skip it so we don't double-check.
    interval.tick().await;

    let mut tick_counter: u64 = 0;
    loop {
        if let Some(d) = deadline
            && std::time::Instant::now() >= d
        {
            return Err(AppError::Validation(format!(
                "jsonl_tail::bind_jsonl_path: target file {} did not appear within {}s timeout",
                target.display(),
                timeout.unwrap().as_secs(),
            )));
        }

        if tokio::fs::metadata(&target).await.is_ok() {
            tracing::info!(
                path = %target.display(),
                "jsonl_tail: bind_jsonl_path resolved"
            );
            return Ok(target);
        }

        // Emit a polling heartbeat once every ~5s (50 ticks * 100ms) so a
        // long bind wait shows up in debug-level traces without flooding.
        tick_counter += 1;
        if tick_counter.is_multiple_of(50) {
            tracing::debug!(
                target = %target.display(),
                "jsonl_tail: bind_jsonl_path polling (target not present yet)"
            );
        }

        interval.tick().await;
    }
}

// ---------------------------------------------------------------------------
// Watcher task: tail a JSONL file with notify + broadcast its records
// ---------------------------------------------------------------------------

/// Tail `jsonl_path` and broadcast each parsed record on `tx`.
///
/// Lifecycle:
/// 1. Watch the file's parent directory non-recursively via
///    `notify::recommended_watcher` (the file may not exist yet when this
///    task starts — see `bind_jsonl_path`'s race window).
/// 2. If the file already exists, skip the create-wait and open immediately;
///    otherwise wait for `EventKind::Create(File)` whose path matches
///    `jsonl_path`.
/// 3. Open + `BufReader` and read-to-EOF on every event poke. The reader
///    advances naturally on subsequent reads (tokio's `BufReader::lines`
///    returns `None` at EOF but the underlying file handle remembers its
///    offset; we just keep calling `next_line()` on a fresh `lines()` each
///    pass).
/// 4. On `event.need_rescan() == true` (Windows ReadDirectoryChangesW buffer
///    overflow signal): seek to 0 and re-read the whole file. The bridge
///    consumer is responsible for any record dedup if that becomes a
///    concern (deferred per plan Risks §8).
/// 5. Loop exits on `tx.send` returning `SendError` (zero receivers — the
///    supervisor has dropped the session) OR on a fatal IO error opening
///    the file. All other errors are logged via `tracing::warn!` and swallowed
///    (matches the supervisor's per-session error policy at
///    `pty/spawn.rs`).
///
/// The `notify` watcher closure runs on its own OS thread that `notify`
/// spawns internally. We bridge it to async by:
/// - allocating a bounded `tokio::sync::mpsc` channel between the closure
///   and the async loop;
/// - using `tx.blocking_send(...)` from inside the sync closure (the
///   documented tokio pattern for sync→async hops).
///
/// The watcher is moved into the task's local state so its lifetime
/// tracks the task — dropping the watcher unsubscribes.
pub async fn tail(
    jsonl_path: PathBuf,
    tx: broadcast::Sender<JsonlRecordParsed>,
) {
    tracing::info!(
        path = %jsonl_path.display(),
        "jsonl_tail: tail task started"
    );
    let parent = match jsonl_path.parent() {
        Some(p) => p.to_path_buf(),
        None => {
            tracing::warn!(
                path = %jsonl_path.display(),
                "jsonl_tail: refusing to tail — path has no parent directory"
            );
            return;
        }
    };

    // Channel from the notify (sync) callback to this (async) task.
    // Capacity 256 is generous for single-file conversational append rates
    // (low-Hz); overflow surfaces by dropping events, which `need_rescan`
    // does not fire for, but our read-to-EOF on every event still catches
    // up.
    let (evt_tx, mut evt_rx) = mpsc::channel::<notify::Event>(256);

    let watcher_result = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        match res {
            Ok(event) => {
                // blocking_send is the documented pattern for sync→async
                // hops; failure means the receiver is gone and the task is
                // shutting down — nothing to do.
                let _ = evt_tx.blocking_send(event);
            }
            Err(e) => {
                tracing::warn!(error = %e, "jsonl_tail: notify error");
            }
        }
    });
    let mut watcher = match watcher_result {
        Ok(w) => w,
        Err(e) => {
            tracing::error!(error = %e, "jsonl_tail: failed to create watcher");
            return;
        }
    };

    if let Err(e) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
        tracing::error!(
            parent = %parent.display(),
            error = %e,
            "jsonl_tail: failed to watch parent directory"
        );
        return;
    }

    // If the file already exists at task start, skip the create-wait.
    // Otherwise loop until the matching Create(File) event arrives.
    if !jsonl_path.exists() {
        loop {
            let event = match evt_rx.recv().await {
                Some(e) => e,
                None => {
                    tracing::warn!("jsonl_tail: watcher channel closed before file appeared");
                    return;
                }
            };
            if matches!(event.kind, EventKind::Create(CreateKind::File) | EventKind::Create(CreateKind::Any))
                && event.paths.iter().any(|p| p == &jsonl_path)
            {
                break;
            }
            // Any other event before the create — ignore.
        }
    }

    let file = match tokio::fs::File::open(&jsonl_path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(
                path = %jsonl_path.display(),
                error = %e,
                "jsonl_tail: failed to open file"
            );
            return;
        }
    };
    let mut reader = BufReader::new(file);

    // Initial drain — Claude Code may have written records between the
    // Create event firing and our open call.
    if !drain_and_broadcast(&mut reader, &tx, &jsonl_path).await {
        return;
    }

    while let Some(event) = evt_rx.recv().await {
        if event.need_rescan() {
            tracing::warn!(
                path = %jsonl_path.display(),
                "jsonl_tail: need_rescan triggered, seeking back to 0"
            );
            // Buffer overflow on the OS side: re-seek to 0 and re-read.
            // The consumer must tolerate duplicate records on rescan (or
            // dedup on JSONL uuid — see plan Risks §8).
            if let Err(e) = reader.get_mut().seek(std::io::SeekFrom::Start(0)).await {
                tracing::error!(error = %e, "jsonl_tail: rescan seek failed");
                return;
            }
        }

        // Trigger a drain on any event whose path matches our file (Create
        // re-fires on truncate-and-rewrite on some platforms; Modify(Any)
        // on Windows is the common case; data-write Modify on Unix).
        let touches_us = event.paths.is_empty()
            || event.paths.iter().any(|p| p == &jsonl_path);
        let interesting = matches!(
            event.kind,
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any
        );
        if !touches_us || !interesting {
            // Skip events for other files in the same dir (notify
            // NonRecursive still reports siblings) and access-only events.
            // We intentionally ignore Modify(ModifyKind::Metadata)? No —
            // on Windows ReadDirectoryChangesW collapses data + metadata
            // into Modify(Any), so we read on any Modify variant.
            let _: Option<ModifyKind> = None;
            continue;
        }

        if !drain_and_broadcast(&mut reader, &tx, &jsonl_path).await {
            return;
        }
    }
}

/// Read from `reader` to EOF; parse each non-empty line and broadcast it.
///
/// Returns `false` if the broadcast channel is dead (no receivers) — the
/// caller should then exit the watcher task. Returns `true` on any other
/// outcome (including IO errors, which are logged and swallowed).
async fn drain_and_broadcast(
    reader: &mut BufReader<tokio::fs::File>,
    tx: &broadcast::Sender<JsonlRecordParsed>,
    path: &Path,
) -> bool {
    let mut lines = reader.lines();
    let mut bytes_read: usize = 0;
    let mut lines_read: usize = 0;
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if line.is_empty() {
                    continue;
                }
                bytes_read += line.len();
                lines_read += 1;
                let parsed = parse_line(&line);
                if tx.send(parsed).is_err() {
                    // No subscribers — the bridge task has been dropped.
                    // This is terminal for the watcher.
                    return false;
                }
            }
            Ok(None) => {
                if lines_read > 0 {
                    tracing::debug!(
                        path = %path.display(),
                        bytes_read,
                        lines_read,
                        "jsonl_tail: drained line(s) after notify event"
                    );
                }
                return true; // EOF, normal
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "jsonl_tail: read error"
                );
                return true; // non-fatal: keep watching
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::Path;

    // -----------------------------------------------------------------
    // bind_jsonl_path — async, uses LUMINA_PTY_PROJECTS_ROOT
    // -----------------------------------------------------------------

    /// Helper: configure the projects root + create the per-cwd dir.
    /// Returns the dir path so the test can write a synthetic JSONL into it.
    ///
    /// Note: nextest runs each `#[test]` in a separate process, so setting
    /// `LUMINA_PTY_PROJECTS_ROOT` here cannot race with other tests in this
    /// module (per `lumina/CLAUDE.md` process-per-test isolation).
    fn arm_env(tempdir: &Path, cwd: &Path) -> PathBuf {
        // SAFETY: nextest runs each test in its own process, so this mutation
        // is observable only by this test. set_var requires unsafe in
        // Rust 2024 edition because of the global-state hazard.
        unsafe {
            std::env::set_var("LUMINA_PTY_PROJECTS_ROOT", tempdir);
        }
        let dir = tempdir.join(sanitise_cwd(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bind_jsonl_path_appears_within_window() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = std::path::PathBuf::from("/test/cwd/binds-fast");
        let dir = arm_env(temp.path(), &cwd);

        // Spawn a delayed writer that materialises the session's predicted file.
        let session_id = "session-abc";
        let dir_clone = dir.clone();
        let write_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let target = dir_clone.join("session-abc.jsonl");
            tokio::fs::write(&target, b"").await.unwrap();
        });

        let got = bind_jsonl_path(&cwd, session_id, Some(Duration::from_secs(5))).await;
        write_task.await.unwrap();

        let path = got.expect("bind_jsonl_path should resolve");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("session-abc.jsonl")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bind_jsonl_path_ignores_other_sessions_files() {
        // Regression for the cross-session-binding bug: when N sessions share
        // a cwd, a fresh file appearing for one session must NOT bind another
        // session's bridge task. With the predicted-filename approach, a file
        // whose name doesn't match the bound session_id is simply not the
        // target — the wait continues until the right file materialises.
        let temp = tempfile::tempdir().unwrap();
        let cwd = std::path::PathBuf::from("/test/cwd/no-crosstalk");
        let dir = arm_env(temp.path(), &cwd);

        // Drop an unrelated session's file into the dir BEFORE our bind starts.
        tokio::fs::write(dir.join("other-session.jsonl"), b"").await.unwrap();

        // Schedule the bound session's file to appear later.
        let dir_clone = dir.clone();
        let write_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            tokio::fs::write(dir_clone.join("ours.jsonl"), b"").await.unwrap();
        });

        let got = bind_jsonl_path(&cwd, "ours", Some(Duration::from_secs(5))).await;
        write_task.await.unwrap();

        let path = got.expect("bind_jsonl_path should resolve to OUR file, not other-session.jsonl");
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("ours.jsonl"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bind_jsonl_path_times_out_when_no_file_appears() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = std::path::PathBuf::from("/test/cwd/never-binds");
        let _dir = arm_env(temp.path(), &cwd);

        let start = std::time::Instant::now();
        let got = bind_jsonl_path(&cwd, "missing-session", Some(Duration::from_secs(5))).await;
        let elapsed = start.elapsed();

        match got {
            Err(AppError::Validation(msg)) => {
                assert!(
                    msg.contains("did not appear within"),
                    "unexpected validation msg: {msg}"
                );
            }
            other => panic!("expected Err(Validation), got {other:?}"),
        }
        // Should have run for ~5s; allow a wide window for CI jitter.
        assert!(
            elapsed >= Duration::from_secs(4),
            "expected ≥4s wait, got {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(7),
            "expected <7s wait, got {elapsed:?}"
        );
    }
}
