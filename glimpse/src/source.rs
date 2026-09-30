//! Watches a repo's flows for changes and fetches fresh snapshots onto the event channel.
//!
//! A filesystem watch only wakes the poller; the fingerprints below still decide what changed.
//! A safety tick every [`SAFETY_TICK`] catches what the watch missed, and two consecutive
//! safety ticks that find an unreported change switch to polling every `poll_ms` for good.
//!
//! The poller never reads integrity sidecars: tomlctl writes the sidecar and the TOML as two
//! separate renames, so a check from here could catch them mid-update. A torn read instead
//! surfaces as a fetch failure and is retried.
//!
//! The poller also tails the activity panel's transcript, re-reading it every [`TICK`] while
//! the runtime has one targeted; a tail refresh never counts as a tick.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use notify::RecommendedWatcher;

use crate::app::TICK;
use crate::flows::{self, FlowEntry};
use crate::model::Snapshot;
use crate::transcript::{TailState, TailView};
use crate::watch::{self, Wake};

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
    /// The targeted transcript's tail, sent on a retarget and whenever a refresh changed it.
    Tail(Box<TailView>),
    Input(ratatui::crossterm::event::Event),
}

/// Messages from the runtime to the poller thread.
#[derive(Debug)]
enum Control {
    /// Switch to another flow and fetch it at once.
    SetSlug(String),
    /// Tail this transcript from now on, reading it at once; `None` stops tailing.
    Tail(Option<String>),
    /// The watcher saw something change; sent from notify's event thread.
    Wake(Wake),
    Stop,
}

/// How long a failed fetch waits before retrying when no file has changed.
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// How often a healthy watch is double-checked by a full tick.
const SAFETY_TICK: Duration = Duration::from_secs(10);

/// After a wake, further messages are gathered until none arrives for this long...
const COALESCE_QUIET: Duration = Duration::from_millis(50);
/// ...or this long has passed since the wake, whichever comes first.
const COALESCE_MAX: Duration = Duration::from_millis(250);

/// Consecutive safety ticks with an unreported change before the watch is abandoned.
const MISS_LIMIT: u8 = 2;

/// `(mtime, len)` per file a snapshot reads, in [`tomlctl::SNAPSHOT_INPUTS`] order; `None`
/// for a file that is absent or cannot be statted.
pub(crate) type Fingerprint = [Option<(SystemTime, u64)>; tomlctl::SNAPSHOT_INPUTS.len()];

fn flow_dir(root: &Path, slug: &str) -> PathBuf {
    flows::flows_root(root).join(slug)
}

pub(crate) fn fingerprint(root: &Path, slug: &str) -> Fingerprint {
    let dir = flow_dir(root, slug);
    tomlctl::SNAPSHOT_INPUTS.map(|name| {
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
/// is absent from `cache`. A rename moves the directory's mtime; an in-place write does not,
/// on NTFS and ext4 alike, so those are caught by the watcher evicting the flow's entry.
fn flows_fingerprint(
    root: &Path,
    cache: &mut BTreeMap<String, FlowStat>,
) -> BTreeMap<String, FlowTimes> {
    let Ok(entries) = std::fs::read_dir(flows::flows_root(root)) else {
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
        let value =
            tomlctl::snapshot(root, slug).map_err(|e| with_reinstall_hint(format!("{e:#}")))?;
        serde_json::from_value(value).map_err(|e| format!("bad `tasks snapshot` document: {e}"))
    }

    fn list_flows(&mut self, root: &Path) -> Result<Vec<FlowEntry>, String> {
        flows::list(root)
    }
}

/// Adds the fix to a tomlctl "newer than the supported" schema error. tomlctl's own advice is
/// to upgrade tomlctl, but glimpse reads through the copy linked into it, so the binary to
/// rebuild is glimpse.
pub(crate) fn with_reinstall_hint(message: String) -> String {
    if message.contains("newer than the supported") {
        format!(
            "{message} (glimpse embeds its own tomlctl: reinstall it with `cargo install --path glimpse`)"
        )
    } else {
        message
    }
}

/// Every wake gathered before one tick, merged: an `All` or `Rewatch` covers every flow.
#[derive(Debug, Default)]
struct Wakes {
    flows: BTreeSet<String>,
    all: bool,
    rewatch: bool,
}

impl Wakes {
    fn add(&mut self, wake: Wake) {
        match wake {
            Wake::Flow(slug) => {
                self.flows.insert(slug);
            }
            Wake::All => self.all = true,
            Wake::Rewatch => self.rewatch = true,
        }
    }

    fn is_empty(&self) -> bool {
        self.flows.is_empty() && !self.all && !self.rewatch
    }
}

/// Whether a watch is feeding the poller. The watcher lives here, on the poller thread.
enum Mode {
    /// Held only so the watch lives until the mode changes.
    Watching { _watcher: RecommendedWatcher },
    /// Polling every `poll_ms`; `retry_watch` while the last watch start failed, cleared once
    /// the watch was abandoned for missing changes.
    Polling { retry_watch: bool },
}

impl Mode {
    /// Watches `flows_root`, forwarding every wake onto the poller's own control channel.
    fn start(flows_root: &Path, control: &Sender<Control>) -> Mode {
        if !flows_root.is_dir() {
            return Mode::Polling { retry_watch: true };
        }
        let tx = control.clone();
        match watch::start(flows_root, move |wake| {
            let _ = tx.send(Control::Wake(wake));
        }) {
            Ok(watcher) => Mode::Watching { _watcher: watcher },
            Err(_) => Mode::Polling { retry_watch: true },
        }
    }

    fn is_watching(&self) -> bool {
        matches!(self, Mode::Watching { .. })
    }

    fn state(&self) -> WatchState {
        match self {
            Mode::Watching { .. } => WatchState::Watching,
            Mode::Polling { retry_watch: true } => WatchState::Retrying,
            Mode::Polling { retry_watch: false } => WatchState::Abandoned,
        }
    }

    /// Applies whatever [`mode_change`] decides at `phase`. Replacing a `Watching` mode drops
    /// its watcher.
    fn advance(&mut self, phase: Phase, flows_root: &Path, control: &Sender<Control>) {
        match mode_change(self.state(), phase) {
            Some(ModeChange::Start) => *self = Mode::start(flows_root, control),
            Some(ModeChange::Poll { retry_watch }) => *self = Mode::Polling { retry_watch },
            None => {}
        }
    }
}

/// A [`Mode`] without its watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchState {
    Watching,
    /// Polling because the last watch start failed or a rewatch dropped the watcher.
    Retrying,
    /// Polling for good: the watch missed changes.
    Abandoned,
}

/// The points in the poller's loop at which its mode may change.
#[derive(Debug, Clone, Copy)]
enum Phase {
    /// Before a tick.
    Tick,
    /// After a tick; `abandon` when the missed-change count gave up on the watch.
    Ticked { abandon: bool },
    /// After a wait; `rewatch` when a wake asked for a fresh watch.
    Woken { rewatch: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeChange {
    Start,
    Poll { retry_watch: bool },
}

/// Whether the watch starts, stops or stays at `phase`: a failed start is retried before
/// every tick, a watch that missed changes is abandoned for good, and a rewatch drops the
/// watcher so the next tick starts a fresh one.
fn mode_change(state: WatchState, phase: Phase) -> Option<ModeChange> {
    match (state, phase) {
        (WatchState::Retrying, Phase::Tick) => Some(ModeChange::Start),
        (WatchState::Watching, Phase::Ticked { abandon: true }) => {
            Some(ModeChange::Poll { retry_watch: false })
        }
        (WatchState::Watching, Phase::Woken { rewatch: true }) => {
            Some(ModeChange::Poll { retry_watch: true })
        }
        _ => None,
    }
}

/// How long the poller goes between ticks when nothing wakes it.
fn safety_period(watching: bool, poll: Duration) -> Duration {
    if watching { SAFETY_TICK } else { poll }
}

/// The instants at which the poller wakes of its own accord.
struct Deadlines {
    /// The next safety tick; moves only when a tick runs.
    safety: Instant,
    /// When a failed fetch is due another try.
    retry: Option<Instant>,
    /// When the targeted transcript is next re-read.
    tail: Option<Instant>,
}

/// Which deadlines had been reached when a wait ran out.
struct Due {
    safety: bool,
    retry: bool,
    tail: bool,
}

impl Deadlines {
    fn due(&self, now: Instant) -> Due {
        let reached = |at: Option<Instant>| at.is_some_and(|at| now >= at);
        Due {
            safety: now >= self.safety,
            retry: reached(self.retry),
            tail: reached(self.tail),
        }
    }
}

/// How long to wait for a message before the earliest deadline; zero once one has passed.
fn next_wait(now: Instant, deadlines: &Deadlines) -> Duration {
    [Some(deadlines.safety), deadlines.retry, deadlines.tail]
        .into_iter()
        .flatten()
        .min()
        .map_or(Duration::ZERO, |at| at.saturating_duration_since(now))
}

/// What set off a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickCause {
    /// A watch wake.
    Wake,
    /// The safety deadline.
    Safety,
    /// Anything else: the first tick, a slug change, a fetch retry.
    Other,
}

/// Counts consecutive safety ticks that found a change no wake had reported.
#[derive(Debug, Default)]
struct MissedChanges {
    streak: u8,
}

impl MissedChanges {
    /// Records one tick; `true` means the watch has missed enough to be abandoned.
    fn on_tick(&mut self, cause: TickCause, changed: bool) -> bool {
        match cause {
            TickCause::Wake => self.streak = 0,
            TickCause::Safety if changed => self.streak = self.streak.saturating_add(1),
            TickCause::Safety => self.streak = 0,
            TickCause::Other => {}
        }
        self.streak >= MISS_LIMIT
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
    /// The transcript the runtime asked to tail.
    tail: Option<Tail>,
}

/// A targeted transcript and when it is next re-read.
struct Tail {
    state: TailState,
    due: Instant,
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
            tail: None,
        }
    }

    /// When the targeted transcript is next re-read.
    fn tail_at(&self) -> Option<Instant> {
        self.tail.as_ref().map(|tail| tail.due)
    }

    /// Points the tail at `path`, reads it and always sends the result; `None` drops the tail.
    /// Returns `false` once the receiver has gone.
    fn set_tail(&mut self, path: Option<String>) -> bool {
        let Some(path) = path else {
            self.tail = None;
            return true;
        };
        let tail = self.tail.get_or_insert_with(|| Tail {
            state: TailState::new(&path),
            due: Instant::now(),
        });
        tail.state.retarget(&path);
        tail.state.refresh();
        tail.due = Instant::now() + TICK;
        let event = Event::Tail(Box::new(tail.state.view()));
        self.events.send(event).is_ok()
    }

    /// Re-reads the targeted transcript, sending the tail only when it changed. Returns
    /// `false` once the receiver has gone.
    fn refresh_tail(&mut self) -> bool {
        let Some(tail) = &mut self.tail else {
            return true;
        };
        tail.due = Instant::now() + TICK;
        if !tail.state.refresh() {
            return true;
        }
        let event = Event::Tail(Box::new(tail.state.view()));
        self.events.send(event).is_ok()
    }

    fn set_slug(&mut self, slug: String) {
        self.slug = Some(slug);
        self.last_fingerprint = None;
        self.last_revision = None;
        self.failed_at = None;
    }

    /// One poll. Returns `false` once the receiver has gone.
    #[cfg(test)]
    fn tick(&mut self) -> bool {
        self.step().is_some()
    }

    /// One poll: whether the flow scan or the viewed flow's fingerprint changed, or `None`
    /// once the receiver has gone, which ends the thread.
    ///
    /// With no flow chosen yet the flow list goes first, since the runtime picks a flow from
    /// it; otherwise the viewed flow's snapshot does.
    fn step(&mut self) -> Option<bool> {
        let change = self.scan_flows();
        let mut changed = change.is_some();
        if self.slug.is_some() {
            changed |= self.poll_snapshot()?;
        }
        self.send_flows(change).then_some(changed)
    }

    /// Drops the cached state `wakes` invalidates, so the next tick re-reads it.
    fn evict(&mut self, wakes: &Wakes) {
        if wakes.all || wakes.rewatch {
            self.flow_stats.clear();
            self.last_fingerprint = None;
        } else {
            for slug in &wakes.flows {
                self.flow_stats.remove(slug);
            }
        }
    }

    /// When a failed fetch is next due a retry.
    fn retry_at(&self) -> Option<Instant> {
        self.failed_at
            .and_then(|at| at.checked_add(self.retry_after))
    }

    /// Compares the flows on disk with the last scan. A new or vanished task store, a moved
    /// `context.toml` (the selector shows its status and date), the first scan, or a failed
    /// last list ask for a relist; moved `tasks.toml` mtimes alone are sent as they are,
    /// with no relist.
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

    /// Fetches the viewed flow when its files moved or a failed fetch is due a retry. Returns
    /// whether the files moved, or `None` once the receiver has gone.
    fn poll_snapshot(&mut self) -> Option<bool> {
        let Some(slug) = self.slug.clone() else {
            return Some(false);
        };
        let current = fingerprint(&self.root, &slug);
        let changed = self.last_fingerprint != Some(current);
        let retry_due = self.retry_at().is_some_and(|at| Instant::now() >= at);
        if !changed && !retry_due {
            return Some(false);
        }
        // Recorded before the fetch: a write that lands during it moves the fingerprint again
        // and is picked up next tick.
        self.last_fingerprint = Some(current);
        let event = match self.fetcher.fetch(&self.root, &slug) {
            Ok(snapshot) => {
                self.failed_at = None;
                if self.last_revision.as_deref() == Some(snapshot.revision.as_str()) {
                    return Some(changed);
                }
                self.last_revision = Some(snapshot.revision.clone());
                Event::Snapshot(Box::new(snapshot))
            }
            Err(e) => {
                self.failed_at = Some(Instant::now());
                Event::SourceError(e)
            }
        };
        self.events.send(event).ok().map(|()| changed)
    }

    /// Applies one control message, merging a wake into `wakes`; `false` means stop.
    fn handle(&mut self, message: Control, wakes: &mut Wakes) -> bool {
        match message {
            Control::SetSlug(slug) => self.set_slug(slug),
            Control::Wake(wake) => wakes.add(wake),
            Control::Tail(path) => return self.set_tail(path),
            Control::Stop => return false,
        }
        true
    }

    /// Gathers the messages that follow a wake, so a burst of writes costs one tick. Returns
    /// once the channel has been quiet for [`COALESCE_QUIET`] or [`COALESCE_MAX`] has passed;
    /// `false` means stop.
    fn drain(&mut self, control: &Receiver<Control>, wakes: &mut Wakes) -> bool {
        let until = Instant::now() + COALESCE_MAX;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match control.recv_timeout(left.min(COALESCE_QUIET)) {
                Ok(message) => {
                    if !self.handle(message, wakes) {
                        return false;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return true,
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    /// Ticks whenever a wake, a slug change, a fetch retry or the safety deadline calls for
    /// it, until told to stop. `control` is the sending half of the poller's own channel,
    /// which the watcher's callback posts wakes onto.
    fn run(mut self, poll: Duration, messages: &Receiver<Control>, control: &Sender<Control>) {
        let flows_root = flows::flows_root(&self.root);
        let mut mode = Mode::start(&flows_root, control);
        let mut misses = MissedChanges::default();
        let mut wakes = Wakes::default();
        let mut cause = TickCause::Other;
        loop {
            mode.advance(Phase::Tick, &flows_root, control);
            let Some(changed) = self.step() else {
                return;
            };
            if !wakes.is_empty() {
                cause = TickCause::Wake;
            }
            let abandon = mode.is_watching() && misses.on_tick(cause, changed);
            mode.advance(Phase::Ticked { abandon }, &flows_root, control);
            let now = Instant::now();
            let period = safety_period(mode.is_watching(), poll);
            let next_safety_at = now.checked_add(period).unwrap_or(now + SAFETY_TICK);
            wakes = Wakes::default();
            loop {
                let deadlines = Deadlines {
                    safety: next_safety_at,
                    retry: self.retry_at(),
                    tail: self.tail_at(),
                };
                match messages.recv_timeout(next_wait(Instant::now(), &deadlines)) {
                    Ok(message) => {
                        let woke = matches!(message, Control::Wake(_));
                        let retail = matches!(message, Control::Tail(_));
                        if !self.handle(message, &mut wakes)
                            || (woke && !self.drain(messages, &mut wakes))
                        {
                            return;
                        }
                        // A retarget has already read and sent the tail; it needs no tick.
                        if retail {
                            continue;
                        }
                        cause = TickCause::Other;
                        break;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {
                        let due = deadlines.due(Instant::now());
                        if due.tail && !self.refresh_tail() {
                            return;
                        }
                        cause = if due.safety {
                            TickCause::Safety
                        } else {
                            TickCause::Other
                        };
                        if due.safety || due.retry {
                            break;
                        }
                    }
                }
            }
            self.evict(&wakes);
            let woken = Phase::Woken {
                rewatch: wakes.rewatch,
            };
            mode.advance(woken, &flows_root, control);
        }
    }
}

/// Handle to the poller thread. Dropping it stops and joins the thread.
pub(crate) struct Source {
    control: Sender<Control>,
    handle: Option<JoinHandle<()>>,
}

impl Source {
    /// Starts the production poller, reading flows in-process whenever the watch reports a
    /// change, and every `poll_ms` if the watch cannot be kept.
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

    /// Runs a poller over `fetcher` on its own thread; `poll` is the fallback polling interval.
    pub(crate) fn spawn(
        root: PathBuf,
        slug: Option<String>,
        poll: Duration,
        fetcher: Box<dyn Fetcher>,
        events: Sender<Event>,
    ) -> Source {
        let (control, control_rx) = mpsc::channel();
        let wake_tx = control.clone();
        let handle = std::thread::spawn(move || {
            Poller::new(root, slug, fetcher, events).run(poll, &control_rx, &wake_tx);
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

    /// Tails the transcript at `path`, sending [`Event::Tail`] at once and on every change;
    /// `None` stops tailing.
    pub(crate) fn set_tail(&self, path: Option<String>) {
        let _ = self.control.send(Control::Tail(path));
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
        assert!(
            absent.iter().all(Option::is_none),
            "absent files read as None"
        );

        // Every file the snapshot reads moves its own slot.
        let mut last = absent;
        for (slot, name) in tomlctl::SNAPSHOT_INPUTS.iter().enumerate() {
            std::fs::write(dir.join(name), "x").expect("write");
            let next = fingerprint(&root, "f");
            assert!(next[slot].is_some(), "{name} is fingerprinted");
            assert_ne!(next, last, "{name} moves the fingerprint");
            last = next;
        }
        assert_eq!(fingerprint(&root, "f"), last, "stable while untouched");

        std::fs::write(dir.join("tasks.toml"), "a = 12345\n").expect("rewrite");
        assert_ne!(fingerprint(&root, "f"), last);
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

    #[test]
    fn the_wait_runs_to_the_earliest_deadline() {
        let now = Instant::now();
        let poll = Duration::from_millis(500);
        let watching = Deadlines {
            safety: now + safety_period(true, poll),
            retry: None,
            tail: None,
        };
        assert_eq!(next_wait(now, &watching), SAFETY_TICK);
        assert_eq!(SAFETY_TICK, Duration::from_secs(10));

        let retrying = Deadlines {
            retry: Some(now + Duration::from_secs(2)),
            ..watching
        };
        assert_eq!(next_wait(now, &retrying), Duration::from_secs(2));

        let polling = Deadlines {
            safety: now + safety_period(false, poll),
            retry: None,
            tail: None,
        };
        assert_eq!(next_wait(now, &polling), poll);

        let late = now + Duration::from_secs(20);
        assert_eq!(
            next_wait(late, &polling),
            Duration::ZERO,
            "a passed deadline"
        );
    }

    #[test]
    fn tail_refreshes_do_not_hold_off_the_safety_tick() {
        let now = Instant::now();
        let mut deadlines = Deadlines {
            safety: now + Duration::from_secs(3),
            retry: None,
            tail: Some(now + TICK),
        };
        assert_eq!(next_wait(now, &deadlines), Duration::from_secs(1));

        // Walks the poller's wait loop: each tail refresh moves only the tail deadline.
        let mut at = now;
        let mut refreshes = 0;
        for _ in 0..10 {
            at += next_wait(at, &deadlines);
            let due = deadlines.due(at);
            assert!(!due.retry);
            if due.tail {
                refreshes += 1;
                deadlines.tail = Some(at + TICK);
            }
            if due.safety {
                break;
            }
        }
        assert_eq!(refreshes, 3);
        assert_eq!(at, now + Duration::from_secs(3), "safety on time");
    }

    #[test]
    fn a_tail_is_sent_on_retarget_and_then_only_when_it_changes() {
        let root = temp_root("tail");
        let path = root.join("agent.jsonl");
        let line = |text: &str| {
            serde_json::json!({
                "type": "assistant",
                "timestamp": "2026-09-28T11:04:30Z",
                "message": {"content": [{"type": "text", "text": text}]}
            })
            .to_string()
                + "\n"
        };
        std::fs::write(&path, line("one")).expect("write");
        let path = path.to_string_lossy().into_owned();
        let (fetcher, _calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        poller.tail = Some(Tail {
            state: TailState::within("", Some(root.clone())),
            due: Instant::now(),
        });
        let texts = |events: Vec<Event>| -> Vec<Vec<String>> {
            events
                .into_iter()
                .filter_map(|e| match e {
                    Event::Tail(t) => Some(t.entries.into_iter().map(|e| e.text).collect()),
                    _ => None,
                })
                .collect()
        };

        let mut wakes = Wakes::default();
        assert!(poller.handle(Control::Tail(Some(path.clone())), &mut wakes));
        assert!(wakes.is_empty());
        assert_eq!(texts(drain(&rx)), [["one"]]);
        assert!(poller.tail_at().is_some());

        assert!(poller.refresh_tail());
        assert!(drain(&rx).is_empty(), "nothing appended, nothing sent");

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        std::io::Write::write_all(&mut file, line("two").as_bytes()).expect("append");
        drop(file);
        assert!(poller.refresh_tail());
        assert_eq!(texts(drain(&rx)), [["one", "two"]]);

        assert!(poller.handle(Control::Tail(None), &mut wakes));
        assert!(poller.tail.is_none() && poller.tail_at().is_none());
        assert!(poller.refresh_tail());
        assert!(drain(&rx).is_empty(), "no tail, nothing read");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Rewrites `name` in place, stamped with `secs` past the epoch, and puts the directory's
    /// mtime back if the write moved it, so only an eviction can reveal the change.
    fn write_in_place(dir: &Path, name: &str, secs: u64) {
        let before = modified(dir).expect("dir mtime");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(dir.join(name))
            .expect("open");
        std::io::Write::write_all(&mut file, b"status = \"done\"\n").expect("write");
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .expect("stamp");
        drop(file);
        if modified(dir) != Some(before) {
            open_dir_for_stamp(dir)
                .set_modified(before)
                .expect("restore");
        }
    }

    #[cfg(windows)]
    fn open_dir_for_stamp(dir: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        std::fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)
            .expect("open dir")
    }

    #[cfg(not(windows))]
    fn open_dir_for_stamp(dir: &Path) -> std::fs::File {
        std::fs::File::open(dir).expect("open dir")
    }

    #[test]
    fn a_flow_wake_evicts_that_flows_cached_stat() {
        let root = temp_root("evict");
        let dir = flow_dir(&root, "b");
        std::fs::create_dir_all(&dir).expect("flow dir");
        write_renamed(&dir, "tasks.toml", 100);
        write_renamed(&dir, "context.toml", 100);
        let (fetcher, _calls, lists) = fake_counting_lists(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        assert!(poller.tick());
        drain(&rx);

        write_in_place(&dir, "context.toml", 300);
        assert!(poller.tick());
        assert!(
            drain(&rx).is_empty(),
            "the cached stat hides an in-place write"
        );

        let mut wakes = Wakes::default();
        wakes.add(Wake::Flow("b".into()));
        poller.evict(&wakes);
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
    fn a_set_slug_mid_drain_is_applied_and_wakes_merge() {
        let root = temp_root("drain-slug");
        let (fetcher, _calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, _rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        let (control, messages) = mpsc::channel();
        control.send(Control::SetSlug("f".into())).expect("send");
        control
            .send(Control::Wake(Wake::Flow("a".into())))
            .expect("send");
        control
            .send(Control::Wake(Wake::Flow("b".into())))
            .expect("send");

        let mut wakes = Wakes::default();
        wakes.add(Wake::Flow("a".into()));
        assert!(poller.drain(&messages, &mut wakes));
        assert_eq!(poller.slug.as_deref(), Some("f"));
        assert_eq!(wakes.flows, BTreeSet::from(["a".into(), "b".into()]));
        assert!(!wakes.all && !wakes.rewatch);

        control.send(Control::Wake(Wake::All)).expect("send");
        assert!(poller.drain(&messages, &mut wakes));
        assert!(wakes.all, "an All joins the merged set");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stop_mid_drain_ends_the_loop() {
        let root = temp_root("drain-stop");
        let (fetcher, _calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, _rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        let (control, messages) = mpsc::channel();
        control.send(Control::Wake(Wake::All)).expect("send");
        control.send(Control::Stop).expect("send");
        control.send(Control::Wake(Wake::Rewatch)).expect("send");

        let mut wakes = Wakes::default();
        assert!(!poller.drain(&messages, &mut wakes));
        assert!(!wakes.rewatch, "nothing after the stop is taken");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn two_unreported_changes_abandon_the_watch_and_a_wake_resets_the_count() {
        use TickCause::{Other, Safety, Wake};
        let mut misses = MissedChanges::default();
        assert!(!misses.on_tick(Safety, true), "one miss");
        assert!(misses.on_tick(Safety, true), "the second in a row");

        let mut misses = MissedChanges::default();
        assert!(!misses.on_tick(Safety, true));
        assert!(!misses.on_tick(Wake, true), "a wake resets");
        assert!(!misses.on_tick(Safety, true), "counting starts over");

        let mut misses = MissedChanges::default();
        assert!(!misses.on_tick(Safety, true));
        assert!(!misses.on_tick(Other, true), "only safety ticks count");
        assert!(
            !misses.on_tick(Safety, false),
            "a clean safety tick breaks the run"
        );
        assert!(!misses.on_tick(Safety, true));
    }

    #[test]
    fn the_watch_is_retried_abandoned_and_restarted_at_the_right_phases() {
        use WatchState::{Abandoned, Retrying, Watching};
        let start = Some(ModeChange::Start);
        let poll = |retry_watch| Some(ModeChange::Poll { retry_watch });

        assert_eq!(mode_change(Retrying, Phase::Tick), start, "a failed start");
        assert_eq!(mode_change(Watching, Phase::Tick), None);
        assert_eq!(mode_change(Abandoned, Phase::Tick), None, "never again");

        let abandon = Phase::Ticked { abandon: true };
        assert_eq!(mode_change(Watching, abandon), poll(false));
        assert_eq!(
            mode_change(Watching, Phase::Ticked { abandon: false }),
            None
        );
        assert_eq!(mode_change(Retrying, abandon), None, "no watch to drop");

        let rewatch = Phase::Woken { rewatch: true };
        assert_eq!(
            mode_change(Watching, rewatch),
            poll(true),
            "the watcher is dropped and the next tick starts a fresh one"
        );
        assert_eq!(mode_change(Watching, Phase::Woken { rewatch: false }), None);
        assert_eq!(mode_change(Retrying, rewatch), None);
        assert_eq!(mode_change(Abandoned, rewatch), None);
    }

    #[test]
    fn only_a_schema_too_new_error_gains_the_reinstall_hint() {
        let too_new = "tasks store schema_version 9 is newer than the supported 1 — upgrade";
        let hinted = with_reinstall_hint(too_new.to_string());
        assert!(hinted.starts_with(too_new), "{hinted}");
        assert!(hinted.contains("cargo install --path glimpse"), "{hinted}");
        assert_eq!(with_reinstall_hint("boom".into()), "boom");
    }
}
