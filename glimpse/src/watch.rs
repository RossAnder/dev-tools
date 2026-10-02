//! Filesystem watch over `.claude`, used only as a wake-up signal for the poller.
//!
//! One watcher holds several scopes: `.claude/flows` recursively, `.claude` itself and each
//! existing flow-less ledger directory non-recursively. Each path is watched once, since a
//! second `watch` on one path leaks a handle and doubles events on Windows.
//!
//! The watcher never decides what changed: it maps each raw event to a [`Wake`] naming how
//! much cached state the poller should evict, and the poller's own fingerprinting stays the
//! judge. Paths are matched against an allowlist, so lock churn, sidecars, temp files and
//! every other `.claude` entry wake nothing. The `on_wake` callback runs on notify's event
//! thread, so it must never block — a stalled callback stalls every later event.
//!
//! `Access` events are dropped because inotify reports every `open`, including glimpse's own
//! snapshot reads; waking on them would make each fetch schedule the next one.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How much of the poller's cache a filesystem event invalidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wake {
    /// Only the named flow changed.
    Flow(String),
    /// A repo-level file directly under `.claude` changed.
    Repo(RepoFile),
    /// A ledger in a flow-less ledger directory changed.
    Scope(LedgerScopeDir),
    /// A flow-less ledger directory appeared and needs a watch of its own.
    NewScope(LedgerScopeDir),
    /// Something unplaceable changed: evict everything, keep the watcher.
    All,
    /// The watch itself is suspect: evict everything and re-create the watcher.
    Rewatch,
}

/// The repo-level files under `.claude` that wake the poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum RepoFile {
    Backlog,
    Inputs,
}

impl RepoFile {
    pub(crate) const ALL: [RepoFile; 2] = [RepoFile::Backlog, RepoFile::Inputs];

    pub(crate) fn file_name(self) -> &'static str {
        match self {
            RepoFile::Backlog => "backlog.toml",
            RepoFile::Inputs => "inputs.toml",
        }
    }

    fn named(name: &str) -> Option<RepoFile> {
        RepoFile::ALL.into_iter().find(|f| f.file_name() == name)
    }
}

/// The directories under `.claude` holding flow-less ledgers, one file per scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum LedgerScopeDir {
    Reviews,
    OptimiseFindings,
    PlanReviewFindings,
}

impl LedgerScopeDir {
    pub(crate) const ALL: [LedgerScopeDir; 3] = [
        LedgerScopeDir::Reviews,
        LedgerScopeDir::OptimiseFindings,
        LedgerScopeDir::PlanReviewFindings,
    ];

    pub(crate) fn dir_name(self) -> &'static str {
        match self {
            LedgerScopeDir::Reviews => "reviews",
            LedgerScopeDir::OptimiseFindings => "optimise-findings",
            LedgerScopeDir::PlanReviewFindings => "plan-review-findings",
        }
    }

    fn named(name: &str) -> Option<LedgerScopeDir> {
        LedgerScopeDir::ALL
            .into_iter()
            .find(|d| d.dir_name() == name)
    }
}

/// The watched directories as notify reports them: canonical when possible, so event paths
/// strip cleanly.
#[derive(Debug, Clone)]
pub(crate) struct Roots {
    claude: PathBuf,
    flows: PathBuf,
}

impl Roots {
    pub(crate) fn new(claude_dir: &Path) -> Roots {
        Roots {
            claude: canonical(claude_dir),
            flows: canonical(&claude_dir.join("flows")),
        }
    }

    pub(crate) fn scope_dir(&self, dir: LedgerScopeDir) -> PathBuf {
        self.claude.join(dir.dir_name())
    }
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Whether an event path names something arriving, leaving, or changing in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Arrive,
    Depart,
    Change,
}

/// The motion of the `index`th path of an event of `kind`; a two-path rename runs from the
/// first path to the second.
fn motion(kind: EventKind, index: usize) -> Motion {
    match kind {
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            Motion::Arrive
        }
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            Motion::Depart
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if index == 0 => Motion::Depart,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => Motion::Arrive,
        _ => Motion::Change,
    }
}

/// Maps one notify event to a wake, or `None` when it should be ignored. An event whose paths
/// place in two different scopes, such as a cross-directory rename, is unplaceable.
pub(crate) fn classify(event: &notify::Result<notify::Event>, roots: &Roots) -> Option<Wake> {
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
    let mut placed: Option<Wake> = None;
    for (index, path) in event.paths.iter().enumerate() {
        let Some(wake) = place(path, motion(event.kind, index), roots) else {
            continue;
        };
        if wake == Wake::Rewatch {
            return Some(Wake::Rewatch);
        }
        match &placed {
            Some(prev) if *prev != wake => placed = Some(Wake::All),
            _ => placed = Some(wake),
        }
    }
    placed
}

/// The wake one event path calls for, by the allowlist.
fn place(path: &Path, motion: Motion, roots: &Roots) -> Option<Wake> {
    if path == roots.flows || path == roots.claude {
        return (motion == Motion::Depart).then_some(Wake::Rewatch);
    }
    let name = path.file_name()?.to_str()?;
    if name.ends_with(".sha256") || name.starts_with(".tmp") {
        return None;
    }
    if let Ok(rel) = path.strip_prefix(&roots.flows) {
        return match rel.components().next()? {
            Component::Normal(slug) => slug.to_str().map(|s| Wake::Flow(s.to_owned())),
            _ => None,
        };
    }
    let rel = path.strip_prefix(&roots.claude).ok()?;
    let mut parts = rel.components().map(|c| match c {
        Component::Normal(part) => part.to_str(),
        _ => None,
    });
    match (parts.next()??, parts.next(), parts.next()) {
        (entry, None, None) => {
            if let Some(file) = RepoFile::named(entry) {
                return Some(Wake::Repo(file));
            }
            let dir = LedgerScopeDir::named(entry)?;
            match motion {
                Motion::Arrive => Some(Wake::NewScope(dir)),
                Motion::Depart => Some(Wake::Rewatch),
                Motion::Change => None,
            }
        }
        (entry, Some(Some(file)), None) if file.ends_with(".toml") => {
            LedgerScopeDir::named(entry).map(Wake::Scope)
        }
        _ => None,
    }
}

/// Watches `dir` non-recursively unless it is missing: `Ok(false)` then, not an error, since
/// the Windows backend reports a missing path as a generic error.
pub(crate) fn add_scope(watcher: &mut RecommendedWatcher, dir: &Path) -> notify::Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    Ok(true)
}

/// Starts the watch over `claude_dir`, whose `flows` directory must exist, and returns the
/// flow-less ledger directories it covers; the watcher stops when dropped.
pub(crate) fn start(
    claude_dir: &Path,
    on_wake: impl Fn(Wake) + Send + 'static,
) -> notify::Result<(RecommendedWatcher, BTreeSet<LedgerScopeDir>)> {
    let roots = Roots::new(claude_dir);
    let classify_roots = roots.clone();
    // `.claude/flows` is repo content, so a link below the root must not pull the watch
    // outside the flows tree.
    let config = notify::Config::default().with_follow_symlinks(false);
    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            if let Some(wake) = classify(&event, &classify_roots) {
                on_wake(wake);
            }
        },
        config,
    )?;
    watcher.watch(&roots.flows, RecursiveMode::Recursive)?;
    watcher.watch(&roots.claude, RecursiveMode::NonRecursive)?;
    let mut scopes = BTreeSet::new();
    for dir in LedgerScopeDir::ALL {
        if add_scope(&mut watcher, &roots.scope_dir(dir))? {
            scopes.insert(dir);
        }
    }
    Ok((watcher, scopes))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, Flag, ModifyKind, RemoveKind, RenameMode,
    };
    use notify::{Event, EventKind};

    use super::*;

    fn claude() -> PathBuf {
        PathBuf::from("/repo/.claude")
    }

    fn root() -> PathBuf {
        claude().join("flows")
    }

    fn roots() -> Roots {
        Roots {
            claude: claude(),
            flows: root(),
        }
    }

    fn event(kind: EventKind, path: PathBuf) -> notify::Result<Event> {
        Ok(Event::new(kind).add_path(path))
    }

    fn write() -> EventKind {
        EventKind::Modify(ModifyKind::Data(DataChange::Content))
    }

    #[test]
    fn access_is_ignored() {
        let open = EventKind::Access(AccessKind::Open(AccessMode::Read));
        assert_eq!(
            classify(&event(open, root().join("a/tasks.toml")), &roots()),
            None
        );
    }

    #[test]
    fn rename_into_a_flow_names_that_flow() {
        let to = EventKind::Modify(ModifyKind::Name(RenameMode::To));
        let got = classify(&event(to, root().join("a").join("tasks.toml")), &roots());
        assert_eq!(got, Some(Wake::Flow("a".into())));
    }

    #[test]
    fn a_path_outside_every_scope_is_ignored() {
        let kind = EventKind::Modify(ModifyKind::Any);
        let got = classify(
            &event(kind, PathBuf::from("/elsewhere/a/tasks.toml")),
            &roots(),
        );
        assert_eq!(got, None);
    }

    #[test]
    fn a_rescan_wakes_everything() {
        let rescan = Event::new(EventKind::Other)
            .add_path(root().join("a").join("tasks.toml"))
            .set_flag(Flag::Rescan);
        assert_eq!(classify(&Ok(rescan), &roots()), Some(Wake::All));
    }

    #[test]
    fn an_error_rewatches() {
        let err = notify::Error::generic("watch lost");
        assert_eq!(classify(&Err(err), &roots()), Some(Wake::Rewatch));
    }

    #[test]
    fn removing_the_root_rewatches() {
        let remove = EventKind::Remove(RemoveKind::Folder);
        assert_eq!(
            classify(&event(remove, root()), &roots()),
            Some(Wake::Rewatch)
        );
    }

    #[test]
    fn a_backlog_write_wakes_the_repo_scope_not_all() {
        let backlog = claude().join("backlog.toml");
        assert_eq!(
            classify(&event(write(), backlog.clone()), &roots()),
            Some(Wake::Repo(RepoFile::Backlog))
        );
        let to = EventKind::Modify(ModifyKind::Name(RenameMode::To));
        assert_eq!(
            classify(&event(to, backlog), &roots()),
            Some(Wake::Repo(RepoFile::Backlog)),
            "the rename that lands a temp-file write"
        );
        assert_eq!(
            classify(&event(write(), claude().join("inputs.toml")), &roots()),
            Some(Wake::Repo(RepoFile::Inputs))
        );
        assert_eq!(
            classify(
                &event(write(), claude().join("reviews").join("main.toml")),
                &roots()
            ),
            Some(Wake::Scope(LedgerScopeDir::Reviews))
        );
        assert_eq!(
            classify(&event(write(), claude().join("settings.json")), &roots()),
            None,
            "not on the allowlist"
        );
    }

    #[test]
    fn a_lock_file_event_is_ignored() {
        let any = EventKind::Modify(ModifyKind::Any);
        for path in [
            claude().join(".locks"),
            claude().join(".locks").join("backlog.toml.lock"),
            claude().join("worktrees"),
            root(),
        ] {
            assert_eq!(
                classify(&event(any, path.clone()), &roots()),
                None,
                "{path:?}"
            );
        }
    }

    #[test]
    fn a_sidecar_or_temp_name_is_ignored() {
        for path in [
            claude().join(".tmpAb12Cd"),
            claude().join("backlog.toml.sha256"),
            claude().join("reviews").join("main.toml.sha256"),
            claude().join("reviews").join(".tmpXy"),
            root().join("a").join("tasks.toml.sha256"),
        ] {
            assert_eq!(
                classify(&event(write(), path.clone()), &roots()),
                None,
                "{path:?}"
            );
        }
        let from_tmp = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(claude().join(".tmpQ9"))
            .add_path(claude().join("backlog.toml"));
        assert_eq!(
            classify(&Ok(from_tmp), &roots()),
            Some(Wake::Repo(RepoFile::Backlog)),
            "a rename matches on its final name"
        );
    }

    #[test]
    fn creating_a_flowless_dir_asks_for_a_new_scope() {
        let created = [
            EventKind::Create(CreateKind::Any),
            EventKind::Create(CreateKind::Folder),
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
        ];
        for kind in created {
            assert_eq!(
                classify(&event(kind, claude().join("optimise-findings")), &roots()),
                Some(Wake::NewScope(LedgerScopeDir::OptimiseFindings)),
                "{kind:?}"
            );
        }
        let any = EventKind::Modify(ModifyKind::Any);
        assert_eq!(
            classify(&event(any, claude().join("reviews")), &roots()),
            None,
            "a dir-entry modify"
        );
        let removed = EventKind::Remove(RemoveKind::Any);
        assert_eq!(
            classify(&event(removed, claude().join("reviews")), &roots()),
            Some(Wake::Rewatch),
            "its watch is gone"
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glimpse-watch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".claude").join("flows").join("a")).unwrap();
        canonical(&dir.join(".claude"))
    }

    /// Waits up to 5 s for `want`, ignoring any other wake that arrives first.
    fn expect_wake(rx: &mpsc::Receiver<Wake>, want: &Wake, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(wake) if wake == *want => return,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        panic!("no {want:?} within 5 s after {what}");
    }

    /// Discards wakes until none has arrived for 500 ms.
    fn drain_until_quiet(rx: &mpsc::Receiver<Wake>) {
        while rx.recv_timeout(Duration::from_millis(500)).is_ok() {}
    }

    #[test]
    fn live_writes_wake_their_flow() {
        let claude = scratch("flow");
        let flows = claude.join("flows");
        let (tx, rx) = mpsc::channel();
        let (watcher, scopes) = start(&claude, move |wake| {
            let _ = tx.send(wake);
        })
        .unwrap();
        assert!(scopes.is_empty(), "no flow-less dir exists");
        let tasks = flows.join("a").join("tasks.toml");
        let flow_a = Wake::Flow("a".into());

        let tmp = flows.join("a").join("tasks.toml.tmp");
        fs::write(&tmp, "x = 1\n").unwrap();
        fs::rename(&tmp, &tasks).unwrap();
        expect_wake(&rx, &flow_a, "a rename");

        drain_until_quiet(&rx);
        fs::write(&tasks, "x = 2\n").unwrap();
        expect_wake(&rx, &flow_a, "an in-place write");

        drop(watcher);
        let _ = fs::remove_dir_all(claude.parent().unwrap());
    }

    #[test]
    fn live_repo_and_added_scope_writes_wake_their_scope() {
        let claude = scratch("scopes");
        let (tx, rx) = mpsc::channel();
        let (mut watcher, _) = start(&claude, move |wake| {
            let _ = tx.send(wake);
        })
        .unwrap();

        fs::write(claude.join("backlog.toml"), "x = 1\n").unwrap();
        expect_wake(&rx, &Wake::Repo(RepoFile::Backlog), "a backlog write");

        let reviews = claude.join("reviews");
        assert!(!add_scope(&mut watcher, &reviews).unwrap(), "missing");
        fs::create_dir(&reviews).unwrap();
        let new_scope = Wake::NewScope(LedgerScopeDir::Reviews);
        expect_wake(&rx, &new_scope, "creating the dir");
        assert!(add_scope(&mut watcher, &reviews).unwrap());

        drain_until_quiet(&rx);
        fs::write(reviews.join("main.toml"), "x = 1\n").unwrap();
        let scope = Wake::Scope(LedgerScopeDir::Reviews);
        expect_wake(&rx, &scope, "a scope write");

        drop(watcher);
        let _ = fs::remove_dir_all(claude.parent().unwrap());
    }
}
