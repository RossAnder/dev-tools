//! Polls a flow's files for changes and fetches fresh snapshots onto the event channel.
//!
//! The poller never reads integrity sidecars: tomlctl writes the sidecar and the TOML as two
//! separate renames, so a check from here could catch them mid-update. A torn read instead
//! surfaces as a fetch failure and is retried.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;

use crate::model::Snapshot;

/// Everything the runtime's main loop receives, from the poller and the input thread alike.
#[derive(Debug)]
pub(crate) enum Event {
    Snapshot(Box<Snapshot>),
    SourceError(String),
    /// The set of flows with a `tasks.toml`, or one of their mtimes, moved.
    FlowsChanged,
    Input(ratatui::crossterm::event::Event),
}

/// Messages from the runtime to the poller thread.
#[derive(Debug)]
enum Control {
    /// Switch to another flow and fetch it at once.
    SetSlug(String),
    Stop,
}

/// The capability a tomlctl must advertise before the poller will use it.
const REQUIRED_FEATURE: &str = "tasks_snapshot";
pub(crate) const REQUIRED_MESSAGE: &str = "tomlctl ≥0.12.0 required — cargo install --path tomlctl";

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

/// The mtime of every `<root>/.claude/flows/*/tasks.toml`, keyed by flow directory name.
/// A directory listing plus one stat per flow; no process is spawned.
fn flows_fingerprint(root: &Path) -> BTreeMap<String, SystemTime> {
    let Ok(entries) = std::fs::read_dir(root.join(".claude").join("flows")) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let mtime = std::fs::metadata(entry.path().join("tasks.toml"))
                .and_then(|m| m.modified())
                .ok()?;
            Some((entry.file_name().to_string_lossy().into_owned(), mtime))
        })
        .collect()
}

/// Produces a snapshot for one flow. The production implementation shells out to tomlctl;
/// tests substitute a fake.
pub(crate) trait Fetcher: Send {
    fn fetch(&mut self, root: &Path, slug: &str) -> Result<Snapshot, String>;
}

/// Runs `<tomlctl> tasks snapshot --slug <slug>` in the repository root.
pub(crate) struct TomlctlFetcher {
    pub(crate) tomlctl: String,
}

impl Fetcher for TomlctlFetcher {
    fn fetch(&mut self, root: &Path, slug: &str) -> Result<Snapshot, String> {
        let tomlctl = &self.tomlctl;
        let out = Command::new(tomlctl)
            .args(["tasks", "snapshot", "--slug", slug])
            .current_dir(root)
            .output()
            .map_err(|e| format!("cannot run `{tomlctl} tasks snapshot`: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "`{tomlctl} tasks snapshot` failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        serde_json::from_slice(&out.stdout).map_err(|e| format!("bad `tasks snapshot` output: {e}"))
    }
}

#[derive(Deserialize)]
struct Capabilities {
    #[serde(default)]
    features: Vec<String>,
}

/// Checks `tomlctl capabilities` output for the snapshot verb.
fn check_capabilities(json: &[u8]) -> Result<(), String> {
    let caps: Capabilities =
        serde_json::from_slice(json).map_err(|e| format!("bad `capabilities` output: {e}"))?;
    if caps.features.iter().any(|f| f == REQUIRED_FEATURE) {
        Ok(())
    } else {
        Err(REQUIRED_MESSAGE.to_string())
    }
}

/// Runs `<tomlctl> capabilities` once and confirms the installed binary has the snapshot verb.
pub(crate) fn probe_tomlctl(tomlctl: &str, root: &Path) -> Result<(), String> {
    let out = Command::new(tomlctl)
        .arg("capabilities")
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run `{tomlctl} capabilities`: {e} — {REQUIRED_MESSAGE}"))?;
    if !out.status.success() {
        return Err(REQUIRED_MESSAGE.to_string());
    }
    check_capabilities(&out.stdout)
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
    last_flows: Option<BTreeMap<String, SystemTime>>,
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
    fn tick(&mut self) -> bool {
        let flows = flows_fingerprint(&self.root);
        if self.last_flows.as_ref() != Some(&flows) {
            self.last_flows = Some(flows);
            if self.events.send(Event::FlowsChanged).is_err() {
                return false;
            }
        }

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
    /// Starts the production poller: probes the configured tomlctl, then fetches through it.
    pub(crate) fn start(
        root: PathBuf,
        slug: Option<String>,
        config: &crate::config::Config,
        events: Sender<Event>,
    ) -> Source {
        let tomlctl = config.tomlctl.clone();
        let probe_root = root.clone();
        let fetcher = Box::new(TomlctlFetcher {
            tomlctl: tomlctl.clone(),
        });
        Source::spawn(
            root,
            slug,
            Duration::from_millis(config.poll_ms),
            fetcher,
            move || probe_tomlctl(&tomlctl, &probe_root),
            events,
        )
    }

    /// Runs `probe` once on the poller thread; on failure its message is sent as a
    /// `SourceError` and the thread idles until stopped instead of polling.
    pub(crate) fn spawn(
        root: PathBuf,
        slug: Option<String>,
        interval: Duration,
        fetcher: Box<dyn Fetcher>,
        probe: impl FnOnce() -> Result<(), String> + Send + 'static,
        events: Sender<Event>,
    ) -> Source {
        let (control, control_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            if let Err(e) = probe() {
                if events.send(Event::SourceError(e)).is_ok() {
                    while let Ok(Control::SetSlug(_)) = control_rx.recv() {}
                }
                return;
            }
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

    pub(crate) fn stop(mut self) {
        self.shutdown();
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

    /// Returns each queued result in turn, then repeats the last; counts its calls.
    struct FakeFetcher {
        results: Vec<Result<Snapshot, String>>,
        calls: Arc<AtomicUsize>,
    }

    impl Fetcher for FakeFetcher {
        fn fetch(&mut self, _root: &Path, _slug: &str) -> Result<Snapshot, String> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            self.results[n.min(self.results.len() - 1)].clone()
        }
    }

    fn with_revision(revision: &str) -> Snapshot {
        Snapshot {
            revision: revision.to_string(),
            ..crate::model::fixture()
        }
    }

    fn fake(results: Vec<Result<Snapshot, String>>) -> (Box<dyn Fetcher>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = FakeFetcher {
            results,
            calls: Arc::clone(&calls),
        };
        (Box::new(fetcher), calls)
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
    fn a_new_flow_task_store_raises_flows_changed() {
        let root = temp_root("flows");
        let (fetcher, calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);

        assert!(poller.tick());
        assert_eq!(
            drain(&rx).len(),
            1,
            "the first scan reports the flows found"
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

        std::fs::write(dir.join("tasks.toml"), "").expect("write");
        assert!(poller.tick());
        let events = drain(&rx);
        assert!(
            matches!(events.as_slice(), [Event::FlowsChanged]),
            "{events:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no slug, no fetch");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn capabilities_without_the_snapshot_feature_are_rejected() {
        assert_eq!(
            check_capabilities(br#"{"version":"0.12.0","features":["tasks_snapshot"]}"#),
            Ok(())
        );
        assert_eq!(
            check_capabilities(br#"{"version":"0.11.0","features":["flow_list"]}"#),
            Err(REQUIRED_MESSAGE.to_string())
        );
        assert!(check_capabilities(b"not json").is_err());
    }

    #[test]
    fn a_failed_probe_reports_and_never_polls() {
        let root = temp_root("probe");
        let (fetcher, calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let source = Source::spawn(
            root.clone(),
            Some("f".into()),
            Duration::from_millis(10),
            fetcher,
            || Err(REQUIRED_MESSAGE.to_string()),
            tx,
        );
        let first = rx.recv_timeout(Duration::from_secs(5)).expect("an event");
        assert!(matches!(&first, Event::SourceError(m) if m == REQUIRED_MESSAGE));
        source.set_slug("g".into());
        source.stop();
        assert!(drain(&rx).is_empty(), "nothing after the probe failure");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_thread_fetches_on_set_slug_and_stops_on_request() {
        let root = temp_root("thread");
        let (fetcher, _calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let source = Source::spawn(
            root.clone(),
            None,
            Duration::from_millis(20),
            fetcher,
            || Ok(()),
            tx,
        );
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
