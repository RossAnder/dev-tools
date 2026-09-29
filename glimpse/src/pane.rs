//! Ensures a single glimpse pane per herdr tab.
//!
//! A pane that already exists is found without locking; before splitting, concurrent hooks
//! serialise on an OS file lock and look again, so they cannot race into two panes. herdr
//! has no focus-by-id: an existing pane is focused by moving from the origin in the
//! direction the two rects imply, which only works when they are adjacent; otherwise the
//! pane is reused without focus.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::config::{Config, claude_dir};
use crate::herdr::{FocusDir, Herdr, PaneInfo, Rect, SplitDir, parse_layout};

pub(crate) const PANE_LABEL: &str = "glimpse";

const LOCK_WAIT: Duration = Duration::from_secs(3);
const LOCK_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PaneOutcome {
    /// `focused` is false when focus was not asked for, or the pane is not adjacent to the origin.
    Reused { pane_id: String, focused: bool },
    Created {
        pane_id: String,
        direction: SplitDir,
    },
}

/// `slug = None` launches glimpse with no `--slug`, which opens the freshest flow with
/// auto-flow on. `cwd = None` splits in the origin pane's working directory.
pub(crate) fn ensure(
    herdr: &Herdr,
    origin_pane: &str,
    slug: Option<&str>,
    cwd: Option<&str>,
    focus: bool,
    config: &Config,
) -> Result<PaneOutcome, String> {
    // Finding an existing pane needs no lock; only the split decision does, and it is
    // re-checked under the lock in case another hook split first.
    let panes = herdr.pane_list()?;
    if let Some(reused) = reuse(herdr, &panes, origin_pane, focus) {
        return Ok(reused);
    }

    let dir = claude_dir()
        .ok_or("cannot resolve the claude directory")?
        .join("glimpse");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _lock = PaneLock::acquire(&dir.join("pane.lock"), LOCK_WAIT)?;

    let panes = herdr.pane_list()?;
    if let Some(reused) = reuse(herdr, &panes, origin_pane, focus) {
        return Ok(reused);
    }
    let cwd = match cwd {
        Some(cwd) => cwd.to_string(),
        None => match panes.iter().find(|p| p.pane_id == origin_pane) {
            Some(origin) => origin.cwd.clone(),
            None => herdr.pane_get(origin_pane)?.cwd,
        },
    };

    let direction = choose_direction(herdr.layout(origin_pane)?, config.split_threshold);
    let pane_id = herdr.split(origin_pane, direction, config.pane_ratio as f32, &cwd)?;
    if let Err(e) = label_and_launch(herdr, &pane_id, slug) {
        return Err(discard_pane(e, || herdr.close(&pane_id)));
    }
    Ok(PaneOutcome::Created { pane_id, direction })
}

fn reuse(herdr: &Herdr, panes: &[PaneInfo], origin_pane: &str, focus: bool) -> Option<PaneOutcome> {
    let pane_id = find_existing(panes, origin_pane)?.pane_id.clone();
    let focused = focus && focus_existing(herdr, origin_pane, &pane_id);
    Some(PaneOutcome::Reused { pane_id, focused })
}

fn label_and_launch(herdr: &Herdr, pane_id: &str, slug: Option<&str>) -> Result<(), String> {
    herdr.rename(pane_id, PANE_LABEL)?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate glimpse: {e}"))?;
    herdr.run(pane_id, &launch_line(&exe.to_string_lossy(), slug))
}

/// Closes a pane whose set-up failed and returns `err`, noting a failed close. An
/// unlabelled pane is invisible to `find_existing`, so leaving it would make the next
/// `ensure` split a second one.
fn discard_pane(err: String, close: impl FnOnce() -> Result<(), String>) -> String {
    match close() {
        Ok(()) => err,
        Err(close_err) => format!("{err} (closing the new pane also failed: {close_err})"),
    }
}

/// Best-effort: any herdr failure or a non-adjacent pane yields `false`, never an error.
/// Both panes share a tab, so one layout lists both rects.
fn focus_existing(herdr: &Herdr, origin_pane: &str, pane_id: &str) -> bool {
    if pane_id == origin_pane {
        return true;
    }
    let Ok(layout) = herdr.layout_output(origin_pane) else {
        return false;
    };
    let (Ok(origin), Ok(target)) = (
        parse_layout(&layout, origin_pane),
        parse_layout(&layout, pane_id),
    ) else {
        return false;
    };
    match focus_direction(origin, target) {
        Some(dir) => herdr.focus(origin_pane, dir).is_ok(),
        None => false,
    }
}

/// `Right` when `width >= threshold * height`, measured in cells.
pub(crate) fn choose_direction(origin: Rect, threshold: f64) -> SplitDir {
    if f64::from(origin.width) >= threshold * f64::from(origin.height) {
        SplitDir::Right
    } else {
        SplitDir::Down
    }
}

/// The glimpse-labelled pane in the origin's tab, which may be the origin itself.
pub(crate) fn find_existing<'a>(panes: &'a [PaneInfo], origin_pane: &str) -> Option<&'a PaneInfo> {
    let tab = &panes.iter().find(|p| p.pane_id == origin_pane)?.tab_id;
    panes
        .iter()
        .find(|p| &p.tab_id == tab && p.label.as_deref() == Some(PANE_LABEL))
}

/// The direction from `origin` to `target` when they share an edge (a one-cell gap is
/// tolerated) and overlap along it.
pub(crate) fn focus_direction(origin: Rect, target: Rect) -> Option<FocusDir> {
    let (ox, oy, ow, oh) = (
        u32::from(origin.x),
        u32::from(origin.y),
        u32::from(origin.width),
        u32::from(origin.height),
    );
    let (tx, ty, tw, th) = (
        u32::from(target.x),
        u32::from(target.y),
        u32::from(target.width),
        u32::from(target.height),
    );
    let touches =
        |near_end: u32, far_start: u32| far_start >= near_end && far_start - near_end <= 1;
    let rows_overlap = ty < oy + oh && oy < ty + th;
    let cols_overlap = tx < ox + ow && ox < tx + tw;
    if rows_overlap && touches(ox + ow, tx) {
        Some(FocusDir::Right)
    } else if rows_overlap && touches(tx + tw, ox) {
        Some(FocusDir::Left)
    } else if cols_overlap && touches(oy + oh, ty) {
        Some(FocusDir::Down)
    } else if cols_overlap && touches(ty + th, oy) {
        Some(FocusDir::Up)
    } else {
        None
    }
}

/// A line for pwsh, into which `herdr pane run` types it. A pwsh line opening on a quoted
/// string is an expression, not a command, so a path that is not plainly safe goes through
/// the call operator in single quotes, where nothing is interpolated.
pub(crate) fn launch_line(exe: &str, slug: Option<&str>) -> String {
    let mut line = if is_bare_safe(exe) {
        exe.to_string()
    } else {
        format!("& {}", single_quote(exe))
    };
    if let Some(slug) = slug {
        line.push_str(" --slug ");
        if is_bare_safe(slug) {
            line.push_str(slug);
        } else {
            line.push_str(&single_quote(slug));
        }
    }
    line
}

/// Characters pwsh reads literally in a bare command token.
fn is_bare_safe(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '\\' | '/' | '.' | '_' | '-'))
}

fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Exclusive OS lock on a persistent file, released when the handle closes, so only its
/// holder releases it and a dead holder's lock is released by the OS. The file is never
/// deleted: removing it would let a second locker lock a new file under the same name.
struct PaneLock {
    _file: File,
}

impl PaneLock {
    /// Gives up after `wait`.
    fn acquire(path: &Path, wait: Duration) -> Result<PaneLock, String> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(PaneLock { _file: file }),
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(e)) if e.kind() == ErrorKind::Interrupted => continue,
                Err(TryLockError::Error(e)) => return Err(format!("{}: {e}", path.display())),
            }
            if start.elapsed() >= wait {
                return Err(format!("{} is held by another process", path.display()));
            }
            std::thread::sleep(LOCK_POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn pane(id: &str, tab: &str, label: Option<&str>) -> PaneInfo {
        PaneInfo {
            pane_id: id.to_string(),
            tab_id: tab.to_string(),
            label: label.map(str::to_string),
            cwd: String::new(),
            rect: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glimpse-pane-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_tall_origin_splits_down() {
        assert_eq!(choose_direction(rect(0, 0, 100, 56), 2.2), SplitDir::Down);
    }

    #[test]
    fn a_wide_origin_splits_right() {
        assert_eq!(choose_direction(rect(0, 0, 240, 50), 2.2), SplitDir::Right);
    }

    #[test]
    fn a_labelled_pane_in_the_origin_tab_is_reused() {
        let panes = [
            pane("w1:p1", "w1:t1", None),
            pane("w1:p2", "w1:t1", Some("glimpse")),
        ];
        assert_eq!(
            find_existing(&panes, "w1:p1").map(|p| p.pane_id.as_str()),
            Some("w1:p2")
        );
    }

    #[test]
    fn a_labelled_pane_in_another_tab_is_ignored() {
        let panes = [
            pane("w1:p1", "w1:t1", None),
            pane("w1:p2", "w1:t2", Some("glimpse")),
            pane("w1:p3", "w1:t1", Some("editor")),
        ];
        assert_eq!(find_existing(&panes, "w1:p1"), None);
        assert_eq!(find_existing(&panes, "w1:p9"), None, "unknown origin");
    }

    #[test]
    fn launch_line_is_bare_for_a_plain_path() {
        assert_eq!(launch_line("C:/x/glimpse.exe", None), "C:/x/glimpse.exe");
        assert_eq!(
            launch_line("C:/x/glimpse.exe", Some("lively-twirling-babbage")),
            "C:/x/glimpse.exe --slug lively-twirling-babbage"
        );
    }

    #[test]
    fn launch_line_uses_the_call_operator_for_a_path_with_a_space() {
        assert_eq!(
            launch_line("C:/Program Files/glimpse.exe", Some("a-b")),
            "& 'C:/Program Files/glimpse.exe' --slug a-b"
        );
        assert_eq!(
            launch_line("C:/it's/glimpse.exe", None),
            "& 'C:/it''s/glimpse.exe'"
        );
    }

    #[test]
    fn a_failed_set_up_closes_the_pane_and_keeps_the_original_error() {
        let mut closed = false;
        let err = discard_pane("rename failed".to_string(), || {
            closed = true;
            Ok(())
        });
        assert!(closed);
        assert_eq!(err, "rename failed");

        let err = discard_pane("run failed".to_string(), || Err("no such pane".to_string()));
        assert!(err.starts_with("run failed"), "{err}");
        assert!(err.contains("no such pane"), "{err}");
    }

    #[test]
    fn focus_direction_needs_an_adjacent_pane() {
        let origin = rect(0, 0, 73, 76);
        assert_eq!(
            focus_direction(origin, rect(73, 0, 73, 76)),
            Some(FocusDir::Right)
        );
        assert_eq!(
            focus_direction(rect(73, 0, 73, 76), origin),
            Some(FocusDir::Left)
        );
        assert_eq!(
            focus_direction(rect(0, 0, 100, 30), rect(0, 31, 100, 20)),
            Some(FocusDir::Down)
        );
        assert_eq!(
            focus_direction(rect(0, 31, 100, 20), rect(0, 0, 100, 30)),
            Some(FocusDir::Up)
        );
        assert_eq!(focus_direction(origin, rect(150, 0, 20, 76)), None);
        assert_eq!(focus_direction(origin, rect(73, 80, 20, 10)), None);
    }

    #[test]
    fn a_dropped_lock_is_free_again() {
        let dir = temp_dir("dropped");
        let path = dir.join("pane.lock");
        let held = PaneLock::acquire(&path, LOCK_WAIT).expect("first lock");
        drop(held);
        assert!(path.exists(), "the lock file persists");
        let again = PaneLock::acquire(&path, Duration::from_millis(150))
            .expect("a released lock is free at once");
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_live_lock_times_out() {
        let dir = temp_dir("live");
        let path = dir.join("pane.lock");
        let held = PaneLock::acquire(&path, LOCK_WAIT).expect("first lock");
        let err = PaneLock::acquire(&path, Duration::from_millis(150))
            .err()
            .expect("a held lock blocks");
        assert!(err.contains("held by another process"), "{err}");
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
