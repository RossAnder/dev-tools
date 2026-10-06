//! Marks a repo as having a running glimpse, so the hook opens no second pane for it.
//!
//! A running view holds a shared OS lock on `<claude dir>/glimpse/running/<key>.lock`, keyed
//! by the repo root; the hook probes it with an exclusive try-lock. Shared, so any number of
//! views can run at once; an OS lock, so a view that dies releases it. However it was opened,
//! herdr pane, keybinding or another terminal, a view is seen.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::config::claude_dir;

const HOLD_WAIT: Duration = Duration::from_secs(1);
const HOLD_POLL: Duration = Duration::from_millis(20);

/// Released when dropped.
pub(crate) struct Presence {
    _file: File,
}

/// Best-effort: `None` when the marker cannot be opened or locked, which only costs the
/// hook its chance to see this view.
pub(crate) fn hold(root: &Path) -> Option<Presence> {
    hold_at(&marker_path(root)?, HOLD_WAIT)
}

/// False when the marker cannot be probed, so a failure here never suppresses a pane.
pub(crate) fn is_running(root: &Path) -> bool {
    marker_path(root).is_some_and(|path| is_held(&path))
}

fn marker_path(root: &Path) -> Option<PathBuf> {
    let dir = claude_dir()?.join("glimpse").join("running");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{:016x}.lock", fnv1a(root_key(root).as_bytes()))))
}

/// The root with `/` separators and no trailing one, lowercased on Windows, whose paths
/// are case-insensitive.
pub(crate) fn root_key(root: &Path) -> String {
    let key = root.to_string_lossy().replace('\\', "/");
    let key = key.trim_end_matches('/');
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key.to_string()
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn open(path: &Path) -> Option<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .ok()
}

/// Retries while a probe briefly holds the marker exclusively.
fn hold_at(path: &Path, wait: Duration) -> Option<Presence> {
    let file = open(path)?;
    let start = Instant::now();
    loop {
        match file.try_lock_shared() {
            Ok(()) => return Some(Presence { _file: file }),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) if e.kind() == ErrorKind::Interrupted => continue,
            Err(TryLockError::Error(_)) => return None,
        }
        if start.elapsed() >= wait {
            return None;
        }
        std::thread::sleep(HOLD_POLL);
    }
}

fn is_held(path: &Path) -> bool {
    let Some(file) = open(path) else {
        return false;
    };
    matches!(file.try_lock(), Err(TryLockError::WouldBlock))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("glimpse-presence-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_held_marker_is_running_until_dropped() {
        let dir = temp_dir("held");
        let path = dir.join("repo.lock");
        assert!(!is_held(&path), "no view yet");
        let first = hold_at(&path, HOLD_WAIT).expect("first view");
        let second = hold_at(&path, HOLD_WAIT).expect("views share the marker");
        assert!(is_held(&path));
        drop(first);
        assert!(is_held(&path), "one view is still running");
        drop(second);
        assert!(!is_held(&path), "the probe's own lock was released");
        assert!(!is_held(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn root_key_ignores_separator_style_and_a_trailing_separator() {
        assert_eq!(
            root_key(Path::new("C:\\dev\\x\\")),
            root_key(Path::new("C:/dev/x"))
        );
        if cfg!(windows) {
            assert_eq!(root_key(Path::new("C:/Dev/X")), "c:/dev/x");
        }
    }
}
