//! Polls a flow's files for changes and fetches fresh snapshots onto the event channel.
//!
//! The poller never reads integrity sidecars: tomlctl writes the sidecar and the TOML as two
//! separate renames, so a check from here could catch them mid-update. A torn read instead
//! surfaces as a fetch failure and is retried.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::flows::{self, FlowEntry};
use crate::model::Snapshot;

/// Everything the runtime's main loop receives, from the poller and the input thread alike.
#[derive(Debug)]
pub(crate) enum Event {
    Snapshot(Box<Snapshot>),
    SourceError(String),
    /// A fresh flow list, taken because the set of flows with a `tasks.toml` or one of their
    /// `context.toml` files moved, or on the first scan.
    Flows(Result<Vec<FlowEntry>, String>),
    /// Only `tasks.toml` mtimes moved: the new mtime of every flow with a task store.
    FlowMtimes(BTreeMap<String, SystemTime>),
    Input(ratatui::crossterm::event::Event),
}

/// Messages from the runtime to the poller thread.
#[derive(Debug)]
enum Control {
    /// Switch to another flow and fetch it at once.
    SetSlug(String),
    Stop,
}

/// How long a failed fetch waits before retrying when no file has changed.
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// The four files a snapshot is built from, in a fixed order.
const FLOW_FILES: [&str; 4] = [
    "tasks.toml",
    "execution-record.toml",
    "agents.toml",
    "context.toml",
];

/// `(mtime, len)` per flow file; `None` for a file that is absent or cannot be statted.
pub(crate) type Fingerprint = [Option<(SystemTime, u64)>; 4];

fn flow_dir(root: &Path, slug: &str) -> PathBuf {
    root.join(".claude").join("flows").join(slug)
}

pub(crate) fn fingerprint(root: &Path, slug: &str) -> Fingerprint {
    let dir = flow_dir(root, slug);
    FLOW_FILES.map(|name| {
        let meta = std::fs::metadata(dir.join(name)).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    })
}

/// `(tasks.toml mtime, context.toml mtime)` of one flow.
type FlowTimes = (SystemTime, Option<SystemTime>);

/// What the last scan saw of one flow directory.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FlowStat {
    dir: Option<SystemTime>,
    tasks: Option<SystemTime>,
    context: Option<SystemTime>,
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The [`FlowTimes`] of every `<root>/.claude/flows/*` that has a `tasks.toml`, keyed by
/// directory name. A flow's files are re-statted only when its directory's mtime moved or it
/// is new to `cache`: tomlctl writes by rename, which moves the directory's mtime, and the
/// listing already carries that mtime without opening anything. On NTFS the listing lags an
/// in-place write until the directory's next change, so only renamed writes are seen at once.
/// No process is spawned.
fn flows_fingerprint(
    root: &Path,
    cache: &mut BTreeMap<String, FlowStat>,
) -> BTreeMap<String, FlowTimes> {
    let Ok(entries) = std::fs::read_dir(root.join(".claude").join("flows")) else {
        cache.clear();
        return BTreeMap::new();
    };
    let mut next = BTreeMap::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let dir = entry.metadata().and_then(|m| m.modified()).ok();
        let stat = match cache.get(&name) {
            Some(old) if dir.is_some() && old.dir == dir => *old,
            _ => {
                let path = entry.path();
                FlowStat {
                    dir,
                    tasks: modified(&path.join("tasks.toml")),
                    context: modified(&path.join("context.toml")),
                }
            }
        };
        next.insert(name, stat);
    }
    *cache = next;
    cache
        .iter()
        .filter_map(|(slug, stat)| Some((slug.clone(), (stat.tasks?, stat.context))))
        .collect()
}

/// What a flow scan asks the poller to send.
enum FlowsChange {
    Relist,
    Mtimes(BTreeMap<String, SystemTime>),
}

/// Produces a snapshot for one flow and lists the repo's flows. The production implementation
/// reads the flow's files in-process through the tomlctl library; tests substitute a fake.
pub(crate) trait Fetcher: Send {
    fn fetch(&mut self, root: &Path, slug: &str) -> Result<Snapshot, String>;
    fn list_flows(&mut self, root: &Path) -> Result<Vec<FlowEntry>, String>;
}

/// Builds the `tasks snapshot` document in-process from `<root>/.claude/flows/<slug>`.
pub(crate) struct InProcessFetcher;

impl Fetcher for InProcessFetcher {
    fn fetch(&mut self, root: &Path, slug: &str) -> Result<Snapshot, String> {
        let store = flow_dir(root, slug).join("tasks.toml");
        let value = tomlctl::snapshot(slug, &store).map_err(|e| format!("{e:#}"))?;
        serde_json::from_value(value).map_err(|e| format!("bad `tasks snapshot` document: {e}"))
    }

    fn list_flows(&mut self, root: &Path) -> Result<Vec<FlowEntry>, String> {
        flows::list(root)
    }
}

/// The poller's state between ticks. Kept apart from the thread so a test can drive it one
/// tick at a time.
struct Poller {
    root: PathBuf,
    slug: Option<String>,
    fetcher: Box<dyn Fetcher>,
    events: Sender<Event>,
    /// The fingerprint the last fetch was taken against; `None` forces the next fetch.
    last_fingerprint: Option<Fingerprint>,
    last_revision: Option<String>,
    /// When the last fetch failed, so a failure with no file change still retries.
    failed_at: Option<Instant>,
    /// `None` until the first scan, so the first scan reports the flows it finds.
    last_flows: Option<BTreeMap<String, FlowTimes>>,
    flow_stats: BTreeMap<String, FlowStat>,
    /// Set when the last flow list failed, so the next change lists again.
    relist_pending: bool,
    retry_after: Duration,
}

impl Poller {
    fn new(
        root: PathBuf,
        slug: Option<String>,
        fetcher: Box<dyn Fetcher>,
        events: Sender<Event>,
    ) -> Self {
        Poller {
            root,
            slug,
            fetcher,
            events,
            last_fingerprint: None,
            last_revision: None,
            failed_at: None,
            last_flows: None,
            flow_stats: BTreeMap::new(),
            relist_pending: false,
            retry_after: RETRY_AFTER,
        }
    }

    fn set_slug(&mut self, slug: String) {
        self.slug = Some(slug);
        self.last_fingerprint = None;
        self.last_revision = None;
        self.failed_at = None;
    }

    /// One poll. Returns `false` once the receiver has gone, which ends the thread.
    ///
    /// With no flow chosen yet the flow list goes first, since the runtime picks a flow from
    /// it; otherwise the viewed flow's snapshot does.
    fn tick(&mut self) -> bool {
        let change = self.scan_flows();
        if self.slug.is_none() {
            return self.send_flows(change);
        }
        self.poll_snapshot() && self.send_flows(change)
    }

    /// Compares the flows on disk with the last scan. A new or vanished task store, a moved
    /// `context.toml` (the selector shows its status and date), the first scan, or a failed
    /// last list ask for a relist; moved `tasks.toml` mtimes alone need no process.
    fn scan_flows(&mut self) -> Option<FlowsChange> {
        let flows = flows_fingerprint(&self.root, &mut self.flow_stats);
        let last = self.last_flows.replace(flows.clone());
        if last.as_ref() == Some(&flows) {
            return None;
        }
        let relist = self.relist_pending
            || last.is_none_or(|last| {
                !last.keys().eq(flows.keys())
                    || last.values().zip(flows.values()).any(|(a, b)| a.1 != b.1)
            });
        Some(if relist {
            FlowsChange::Relist
        } else {
            FlowsChange::Mtimes(
                flows
                    .into_iter()
                    .map(|(slug, (tasks, _))| (slug, tasks))
                    .collect(),
            )
        })
    }

    fn send_flows(&mut self, change: Option<FlowsChange>) -> bool {
        let event = match change {
            None => return true,
            Some(FlowsChange::Relist) => {
                let listed = self.fetcher.list_flows(&self.root);
                self.relist_pending = listed.is_err();
                Event::Flows(listed)
            }
            Some(FlowsChange::Mtimes(mtimes)) => Event::FlowMtimes(mtimes),
        };
        self.events.send(event).is_ok()
    }

    /// Fetches the viewed flow when its files moved or a failed fetch is due a retry.
    fn poll_snapshot(&mut self) -> bool {
        let Some(slug) = self.slug.clone() else {
            return true;
        };
        let current = fingerprint(&self.root, &slug);
        let changed = self.last_fingerprint != Some(current);
        let retry_due = self
            .failed_at
            .is_some_and(|at| at.elapsed() >= self.retry_after);
        if !changed && !retry_due {
            return true;
        }
        // Recorded before the fetch: a write that lands during it moves the fingerprint again
        // and is picked up next tick.
        self.last_fingerprint = Some(current);
        let event = match self.fetcher.fetch(&self.root, &slug) {
            Ok(snapshot) => {
                self.failed_at = None;
                if self.last_revision.as_deref() == Some(snapshot.revision.as_str()) {
                    return true;
                }
                self.last_revision = Some(snapshot.revision.clone());
                Event::Snapshot(Box::new(snapshot))
            }
            Err(e) => {
                self.failed_at = Some(Instant::now());
                Event::SourceError(e)
            }
        };
        self.events.send(event).is_ok()
    }

    /// Polls every `interval` until told to stop. Waiting on the control channel rather than
    /// sleeping makes `SetSlug` and `Stop` take effect at once.
    fn run(mut self, interval: Duration, control: &Receiver<Control>) {
        loop {
            if !self.tick() {
                return;
            }
            match control.recv_timeout(interval) {
                Ok(Control::SetSlug(slug)) => self.set_slug(slug),
                Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

/// Handle to the poller thread. Dropping it stops and joins the thread.
pub(crate) struct Source {
    control: Sender<Control>,
    handle: Option<JoinHandle<()>>,
}

impl Source {
    /// Starts the production poller, reading flows in-process every `poll_ms`.
    pub(crate) fn start(
        root: PathBuf,
        slug: Option<String>,
        config: &crate::config::Config,
        events: Sender<Event>,
    ) -> Source {
        Source::spawn(
            root,
            slug,
            Duration::from_millis(config.poll_ms),
            Box::new(InProcessFetcher),
            events,
        )
    }

    /// Runs a poller over `fetcher` on its own thread.
    pub(crate) fn spawn(
        root: PathBuf,
        slug: Option<String>,
        interval: Duration,
        fetcher: Box<dyn Fetcher>,
        events: Sender<Event>,
    ) -> Source {
        let (control, control_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            Poller::new(root, slug, fetcher, events).run(interval, &control_rx);
        });
        Source {
            control,
            handle: Some(handle),
        }
    }

    /// Switches the poller to `slug` and forces a fetch.
    pub(crate) fn set_slug(&self, slug: String) {
        let _ = self.control.send(Control::SetSlug(slug));
    }

    #[cfg(test)]
    pub(crate) fn stop(mut self) {
        self.shutdown();
    }

    /// Tells the poller to stop without waiting for it, so exit never waits out an
    /// in-flight fetch. Safe because a fetch only reads and holds no lock; process exit
    /// reaps the thread.
    pub(crate) fn detach(mut self) {
        let _ = self.control.send(Control::Stop);
        self.handle.take();
    }

    fn shutdown(&mut self) {
        let _ = self.control.send(Control::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("glimpse-source-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude").join("flows")).expect("temp root");
        dir
    }

    /// Returns each queued result in turn, then repeats the last; counts its calls. Lists no
    /// flows, counting those calls apart.
    struct FakeFetcher {
        results: Vec<Result<Snapshot, String>>,
        calls: Arc<AtomicUsize>,
        lists: Arc<AtomicUsize>,
    }

    impl Fetcher for FakeFetcher {
        fn fetch(&mut self, _root: &Path, _slug: &str) -> Result<Snapshot, String> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            self.results[n.min(self.results.len() - 1)].clone()
        }

        fn list_flows(&mut self, _root: &Path) -> Result<Vec<FlowEntry>, String> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    fn with_revision(revision: &str) -> Snapshot {
        Snapshot {
            revision: revision.to_string(),
            ..crate::model::fixture()
        }
    }

    fn fake(results: Vec<Result<Snapshot, String>>) -> (Box<dyn Fetcher>, Arc<AtomicUsize>) {
        let (fetcher, calls, _lists) = fake_counting_lists(results);
        (fetcher, calls)
    }

    fn fake_counting_lists(
        results: Vec<Result<Snapshot, String>>,
    ) -> (Box<dyn Fetcher>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let lists = Arc::new(AtomicUsize::new(0));
        let fetcher = FakeFetcher {
            results,
            calls: Arc::clone(&calls),
            lists: Arc::clone(&lists),
        };
        (Box::new(fetcher), calls, lists)
    }

    /// Writes `name` the way tomlctl does, through a temporary file renamed into place,
    /// stamped with `secs` past the epoch.
    fn write_renamed(dir: &Path, name: &str, secs: u64) {
        let tmp = dir.join(format!("{name}.tmp"));
        let file = std::fs::File::create(&tmp).expect("create");
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .expect("stamp");
        drop(file);
        std::fs::rename(&tmp, dir.join(name)).expect("rename");
    }

    fn drain(rx: &Receiver<Event>) -> Vec<Event> {
        rx.try_iter().collect()
    }

    #[test]
    fn fingerprint_changes_when_a_flow_file_changes() {
        let root = temp_root("fingerprint");
        let dir = flow_dir(&root, "f");
        std::fs::create_dir_all(&dir).expect("flow dir");

        let absent = fingerprint(&root, "f");
        assert_eq!(absent, [None; 4], "absent files read as None");

        std::fs::write(dir.join("tasks.toml"), "a = 1\n").expect("write");
        let first = fingerprint(&root, "f");
        assert!(first[0].is_some());
        assert_ne!(first, absent);
        assert_eq!(fingerprint(&root, "f"), first, "stable while untouched");

        std::fs::write(dir.join("agents.toml"), "x").expect("write");
        let second = fingerprint(&root, "f");
        assert_ne!(second, first);

        std::fs::write(dir.join("tasks.toml"), "a = 12345\n").expect("rewrite");
        assert_ne!(fingerprint(&root, "f"), second);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unchanged_revision_is_not_sent_twice() {
        let root = temp_root("revision");
        let (fetcher, calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), Some("f".into()), fetcher, tx);

        assert!(poller.tick());
        let dir = flow_dir(&root, "f");
        std::fs::create_dir_all(&dir).expect("flow dir");
        std::fs::write(dir.join("context.toml"), "x").expect("write");
        assert!(poller.tick());

        assert_eq!(calls.load(Ordering::SeqCst), 2, "both ticks fetched");
        let snapshots = drain(&rx)
            .into_iter()
            .filter(|e| matches!(e, Event::Snapshot(_)))
            .count();
        assert_eq!(snapshots, 1, "the repeated revision is suppressed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unchanged_fingerprint_does_not_refetch() {
        let root = temp_root("idle");
        let (fetcher, calls) = fake(vec![Ok(with_revision("r1")), Ok(with_revision("r2"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), Some("f".into()), fetcher, tx);
        assert!(poller.tick());
        assert!(poller.tick());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let dir = flow_dir(&root, "f");
        std::fs::create_dir_all(&dir).expect("flow dir");
        std::fs::write(dir.join("execution-record.toml"), "x").expect("write");
        assert!(poller.tick());
        assert_eq!(calls.load(Ordering::SeqCst), 2, "a file change refetches");
        let revisions: Vec<String> = drain(&rx)
            .into_iter()
            .filter_map(|e| match e {
                Event::Snapshot(s) => Some(s.revision),
                _ => None,
            })
            .collect();
        assert_eq!(revisions, ["r1", "r2"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_fetch_error_surfaces_and_retries_after_the_delay() {
        let root = temp_root("error");
        let (fetcher, calls) = fake(vec![Err("boom".into()), Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), Some("f".into()), fetcher, tx);
        poller.retry_after = Duration::ZERO;

        assert!(poller.tick());
        let events = drain(&rx);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SourceError(m) if m == "boom")),
            "{events:?}"
        );

        assert!(poller.tick());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "retried with no file change"
        );
        assert!(drain(&rx).iter().any(|e| matches!(e, Event::Snapshot(_))));

        poller.retry_after = Duration::from_secs(3600);
        assert!(poller.tick());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a success clears the retry"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_flow_task_store_relists_the_flows() {
        let root = temp_root("flows");
        let (fetcher, calls, lists) = fake_counting_lists(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);

        assert!(poller.tick());
        let events = drain(&rx);
        assert!(
            matches!(events.as_slice(), [Event::Flows(Ok(_))]),
            "the first scan reports the flows found: {events:?}"
        );
        assert!(poller.tick());
        assert!(drain(&rx).is_empty(), "no change, no event");

        let dir = flow_dir(&root, "new-flow");
        std::fs::create_dir_all(&dir).expect("flow dir");
        assert!(poller.tick());
        assert!(
            drain(&rx).is_empty(),
            "a flow without tasks.toml does not count"
        );

        write_renamed(&dir, "tasks.toml", 100);
        assert!(poller.tick());
        let events = drain(&rx);
        assert!(
            matches!(events.as_slice(), [Event::Flows(Ok(_))]),
            "{events:?}"
        );
        assert_eq!(lists.load(Ordering::SeqCst), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no slug, no fetch");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_task_store_write_sends_mtimes_and_a_context_write_relists() {
        let root = temp_root("mtimes");
        let dir = flow_dir(&root, "f");
        std::fs::create_dir_all(&dir).expect("flow dir");
        write_renamed(&dir, "tasks.toml", 100);
        let (fetcher, _calls, lists) = fake_counting_lists(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        assert!(poller.tick());
        drain(&rx);
        assert_eq!(lists.load(Ordering::SeqCst), 1);

        write_renamed(&dir, "tasks.toml", 200);
        assert!(poller.tick());
        let events = drain(&rx);
        let Some(Event::FlowMtimes(mtimes)) = events.first() else {
            panic!("{events:?}");
        };
        assert_eq!(
            mtimes.get("f"),
            Some(&(SystemTime::UNIX_EPOCH + Duration::from_secs(200)))
        );
        assert_eq!(lists.load(Ordering::SeqCst), 1, "no list for a tasks write");

        write_renamed(&dir, "context.toml", 300);
        assert!(poller.tick());
        let events = drain(&rx);
        assert!(
            matches!(events.as_slice(), [Event::Flows(Ok(_))]),
            "{events:?}"
        );
        assert_eq!(lists.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_thread_fetches_on_set_slug_and_stops_on_request() {
        let root = temp_root("thread");
        let (fetcher, _calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let source = Source::spawn(root.clone(), None, Duration::from_millis(20), fetcher, tx);
        source.set_slug("f".into());
        let deadline = Instant::now() + Duration::from_secs(5);
        let got = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(Event::Snapshot(s)) => break s,
                Ok(_) => {}
                Err(e) => panic!("no snapshot: {e:?}"),
            }
        };
        assert_eq!(got.revision, "r1");
        source.stop();
        // The thread has been joined, so its sender is gone.
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Disconnected)
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
