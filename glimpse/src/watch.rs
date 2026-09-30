//! Filesystem watch over `.claude/flows`, used only as a wake-up signal for the poller.
//!
//! The watcher never decides what changed: it maps each raw event to a [`Wake`] naming how
//! much cached state the poller should evict, and the poller's own fingerprinting stays the
//! judge. The `on_wake` callback runs on notify's event thread, so it must never block — a
//! stalled callback stalls every later event.
//!
//! `Access` events are dropped because inotify reports every `open`, including glimpse's own
//! snapshot reads; waking on them would make each fetch schedule the next one.

use std::path::{Component, Path, PathBuf};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How much of the poller's cache a filesystem event invalidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wake {
    /// Only the named flow changed.
    Flow(String),
    /// Something unplaceable changed: evict everything, keep the watcher.
    All,
    /// The watch itself is suspect: evict everything and re-create the watcher.
    Rewatch,
}

/// Maps one notify event to a wake, or `None` when it should be ignored.
pub(crate) fn classify(event: &notify::Result<notify::Event>, flows_root: &Path) -> Option<Wake> {
    let event = match event {
        Ok(event) => event,
        Err(_) => return Some(Wake::Rewatch),
    };
    if event.need_rescan() {
        return Some(Wake::All);
    }
    if matches!(event.kind, EventKind::Access(_)) {
        return None;
    }
    if event.kind.is_remove() && event.paths.iter().any(|p| p == flows_root) {
        return Some(Wake::Rewatch);
    }
    let mut slug: Option<&str> = None;
    for path in &event.paths {
        match flow_of(path, flows_root) {
            Some(s) if slug.is_none_or(|prev| prev == s) => slug = Some(s),
            _ => return Some(Wake::All),
        }
    }
    Some(slug.map_or(Wake::All, |s| Wake::Flow(s.to_owned())))
}

/// The flow slug `path` sits under, i.e. its first component below `flows_root`.
fn flow_of<'a>(path: &'a Path, flows_root: &Path) -> Option<&'a str> {
    match path.strip_prefix(flows_root).ok()?.components().next()? {
        Component::Normal(name) => name.to_str(),
        _ => None,
    }
}

/// The root as notify will report it: canonical when possible, so event paths strip cleanly.
fn canonical_root(flows_root: &Path) -> PathBuf {
    flows_root
        .canonicalize()
        .unwrap_or_else(|_| flows_root.to_path_buf())
}

/// Starts one recursive watch on `flows_root`; the returned watcher stops when dropped.
pub(crate) fn start(
    flows_root: &Path,
    on_wake: impl Fn(Wake) + Send + 'static,
) -> notify::Result<RecommendedWatcher> {
    let root = canonical_root(flows_root);
    let classify_root = root.clone();
    // `.claude/flows` is repo content, so a link below the root must not pull the watch
    // outside the flows tree.
    let config = notify::Config::default().with_follow_symlinks(false);
    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            if let Some(wake) = classify(&event, &classify_root) {
                on_wake(wake);
            }
        },
        config,
    )?;
    watcher.watch(&root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use notify::event::{AccessKind, AccessMode, Flag, ModifyKind, RemoveKind, RenameMode};
    use notify::{Event, EventKind};

    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/repo/.claude/flows")
    }

    fn event(kind: EventKind, path: PathBuf) -> notify::Result<Event> {
        Ok(Event::new(kind).add_path(path))
    }

    #[test]
    fn access_is_ignored() {
        let open = EventKind::Access(AccessKind::Open(AccessMode::Read));
        assert_eq!(
            classify(&event(open, root().join("a/tasks.toml")), &root()),
            None
        );
    }

    #[test]
    fn rename_into_a_flow_names_that_flow() {
        let to = EventKind::Modify(ModifyKind::Name(RenameMode::To));
        let got = classify(&event(to, root().join("a").join("tasks.toml")), &root());
        assert_eq!(got, Some(Wake::Flow("a".into())));
    }

    #[test]
    fn a_path_outside_the_root_wakes_everything() {
        let kind = EventKind::Modify(ModifyKind::Any);
        let got = classify(
            &event(kind, PathBuf::from("/elsewhere/a/tasks.toml")),
            &root(),
        );
        assert_eq!(got, Some(Wake::All));
    }

    #[test]
    fn a_rescan_wakes_everything() {
        let rescan = Event::new(EventKind::Other)
            .add_path(root().join("a").join("tasks.toml"))
            .set_flag(Flag::Rescan);
        assert_eq!(classify(&Ok(rescan), &root()), Some(Wake::All));
    }

    #[test]
    fn an_error_rewatches() {
        let err = notify::Error::generic("watch lost");
        assert_eq!(classify(&Err(err), &root()), Some(Wake::Rewatch));
    }

    #[test]
    fn removing_the_root_rewatches() {
        let remove = EventKind::Remove(RemoveKind::Folder);
        assert_eq!(
            classify(&event(remove, root()), &root()),
            Some(Wake::Rewatch)
        );
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glimpse-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("flows").join("a")).unwrap();
        canonical_root(&dir.join("flows"))
    }

    /// Waits up to 5 s for a `Flow("a")`, ignoring any other wake that arrives first.
    fn expect_flow_a(rx: &mpsc::Receiver<Wake>, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(Wake::Flow(slug)) if slug == "a" => return,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        panic!("no Wake::Flow(\"a\") within 5 s after {what}");
    }

    /// Discards wakes until none has arrived for 500 ms.
    fn drain_until_quiet(rx: &mpsc::Receiver<Wake>) {
        while rx.recv_timeout(Duration::from_millis(500)).is_ok() {}
    }

    #[test]
    fn live_writes_wake_their_flow() {
        let flows = scratch();
        let (tx, rx) = mpsc::channel();
        let watcher = start(&flows, move |wake| {
            let _ = tx.send(wake);
        })
        .unwrap();
        let tasks = flows.join("a").join("tasks.toml");

        let tmp = flows.join("a").join("tasks.toml.tmp");
        fs::write(&tmp, "x = 1\n").unwrap();
        fs::rename(&tmp, &tasks).unwrap();
        expect_flow_a(&rx, "a rename");

        drain_until_quiet(&rx);
        fs::write(&tasks, "x = 2\n").unwrap();
        expect_flow_a(&rx, "an in-place write");

        drop(watcher);
        let _ = fs::remove_dir_all(flows.parent().unwrap());
    }
}
