//! Watches a repo's flows, ledgers and input store for changes and fetches fresh snapshots,
//! ledgers and input records onto the event channel.
//!
//! A filesystem watch only wakes the poller; the fingerprints below still decide what changed.
//! A safety tick every [`SAFETY_TICK`], on its own schedule whatever wakes arrive, catches what
//! the watch missed. A change found in a watch scope that did not wake counts a miss against
//! that scope, and two misses with no wake of its own between them switch to polling every
//! `poll_ms` for good; a clean tick neither counts nor resets. A failed watch start is retried
//! with a backoff doubling from `poll_ms` to [`WATCH_RETRY_MAX`].
//!
//! The poller never reads integrity sidecars: tomlctl writes the sidecar and the TOML as two
//! separate renames, so a check from here could catch them mid-update. A torn read instead
//! surfaces as a fetch failure and is retried.
//!
//! The poller also tails the activity panel's transcript, re-reading it every [`TICK`] while
//! the runtime has one targeted; a tail refresh never counts as a tick.

mod mode;
mod scan;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use tomlctl::{LedgerKind, LedgerRef};

use crate::app::TICK;
use crate::flows::{self, FlowEntry};
use crate::ledger::{Inputs, Ledger, Seen};
use crate::model::Snapshot;
use crate::transcript::{TailState, TailView};
use crate::watch::{RepoFile, Wake};
use crate::writer::WriteOutcome;
use mode::{
    Deadlines, MissedChanges, Mode, Phase, SafetyClock, TickCause, Wakes, WatchScope, next_wait,
    safety_period,
};
pub(crate) use scan::{Fetcher, InProcessFetcher, task_store_mtimes, with_reinstall_hint};
use scan::{
    FileStat, Fingerprint, FlowStat, FlowTimes, FlowsChange, feed_path, fingerprint,
    flows_fingerprint, has_task_store, scope_files, stat, task_stores,
};

/// Everything the runtime's main loop receives, from the poller and the input thread alike.
#[derive(Debug)]
pub(crate) enum Event {
    Snapshot(Box<Snapshot>),
    SourceError(String),
    /// A fresh flow list, taken because the set of flows with a `tasks.toml` or a ledger, one
    /// of their `context.toml` files, or the ledgers a flow holds moved, or on the first scan.
    Flows(Result<Vec<FlowEntry>, String>),
    /// Only `tasks.toml` mtimes moved: the new mtime of every flow with a task store.
    FlowMtimes(BTreeMap<String, SystemTime>),
    /// A subscribed feed's read: sent once it is first read, then only when the revision moves
    /// or a read fails.
    Ledger {
        feed: Feed,
        ledger: Result<Ledger, String>,
    },
    /// The [`Feed::Inputs`] read, sent on the same terms as [`Event::Ledger`]. Not to be
    /// confused with [`Event::Input`], a terminal event.
    InputRecords(Result<Inputs, String>),
    /// The `tomlctl::ledger_scopes` document, sent with every flow list and whenever a
    /// flow-less ledger appears or goes.
    Scopes(Result<serde_json::Value, String>),
    /// The targeted transcript's tail, sent on a retarget and whenever a refresh changed it.
    Tail(Box<TailView>),
    Input(ratatui::crossterm::event::Event),
    /// What became of one write; sent by the writer thread, never the poller.
    Written(WriteOutcome),
}

/// A file the runtime asked the poller to keep reading.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Feed {
    Ledger(LedgerRef),
    /// `.claude/inputs.toml`.
    Inputs,
}

/// One successful feed read.
enum FeedRead {
    Ledger(Ledger),
    Inputs(Inputs),
}

impl FeedRead {
    fn revision(&self) -> &Option<String> {
        match self {
            FeedRead::Ledger(ledger) => &ledger.revision,
            FeedRead::Inputs(inputs) => &inputs.revision,
        }
    }
}

/// The event carrying `read` of `feed`.
fn feed_event(feed: &Feed, read: Result<FeedRead, String>) -> Event {
    match read {
        Ok(FeedRead::Inputs(inputs)) => Event::InputRecords(Ok(inputs)),
        Ok(FeedRead::Ledger(ledger)) => Event::Ledger {
            feed: feed.clone(),
            ledger: Ok(ledger),
        },
        Err(e) if *feed == Feed::Inputs => Event::InputRecords(Err(e)),
        Err(e) => Event::Ledger {
            feed: feed.clone(),
            ledger: Err(e),
        },
    }
}

impl Feed {
    /// The watch scope whose wakes report this feed's writes; `None` for a file no scope covers.
    fn scope(&self) -> Option<WatchScope> {
        match self {
            Feed::Ledger(LedgerRef::Flow { .. }) => Some(WatchScope::Flows),
            Feed::Ledger(LedgerRef::Scope { kind, .. }) => Some(WatchScope::Ledgers(*kind)),
            Feed::Ledger(LedgerRef::Backlog) | Feed::Inputs => Some(WatchScope::Repo),
            Feed::Ledger(LedgerRef::File(_)) => None,
        }
    }

    fn woken_by(&self, wakes: &Wakes) -> bool {
        if wakes.all || wakes.rewatch {
            return true;
        }
        match self {
            Feed::Ledger(LedgerRef::Flow { slug, .. }) => wakes.flows.contains(slug),
            Feed::Ledger(LedgerRef::Scope { kind, .. }) => wakes.scopes.contains(kind),
            Feed::Ledger(LedgerRef::Backlog) => wakes.repo.contains(&RepoFile::Backlog),
            Feed::Inputs => wakes.repo.contains(&RepoFile::Inputs),
            Feed::Ledger(LedgerRef::File(_)) => false,
        }
    }
}

/// What the poller last saw of one feed.
#[derive(Debug, Default)]
struct FeedState {
    /// The stat the last read was taken against; `None` forces the next read.
    stat: Option<FileStat>,
    /// The revision last sent.
    sent: Seen,
    /// Set while reads fail, so a failure is sent once rather than on every retry.
    failing: bool,
}

/// Messages from the runtime to the poller thread.
#[derive(Debug)]
enum Control {
    /// Switch to another flow and fetch it at once.
    SetSlug(String),
    /// Keep exactly these feeds from now on, reading the new ones at once.
    Subscribe(Vec<Feed>),
    /// Tail this transcript from now on, reading it at once; `None` stops tailing.
    Tail(Option<String>),
    /// The watcher saw something change; sent from notify's event thread.
    Wake(Wake),
    Stop,
}

/// How long a failed fetch waits before retrying when no file has changed.
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// Reported once for a viewed flow with no `tasks.toml`, which has no snapshot to fetch.
const NO_TASK_STORE: &str = "no task store";

/// How often a healthy watch is double-checked by a full tick.
const SAFETY_TICK: Duration = Duration::from_secs(10);

/// After a wake, further messages are gathered until none arrives for this long...
const COALESCE_QUIET: Duration = Duration::from_millis(50);
/// ...or this long has passed since the wake, whichever comes first.
const COALESCE_MAX: Duration = Duration::from_millis(250);

/// Safety ticks with an unreported change, and no wake between them, before the watch is
/// abandoned.
const MISS_LIMIT: u8 = 2;

/// The longest a failed watch start waits before it is tried again.
const WATCH_RETRY_MAX: Duration = Duration::from_secs(60);

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
    /// The flow-less ledger files the last scan found; `None` before the first.
    last_scope_files: Option<BTreeSet<(LedgerKind, String)>>,
    /// Set once the viewed flow's missing task store has been reported.
    no_store_reported: bool,
    retry_after: Duration,
    /// The transcript the runtime asked to tail.
    tail: Option<Tail>,
    feeds: BTreeMap<Feed, FeedState>,
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
            last_scope_files: None,
            no_store_reported: false,
            retry_after: RETRY_AFTER,
            tail: None,
            feeds: BTreeMap::new(),
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
        self.no_store_reported = false;
    }

    /// Keeps the state of every feed still subscribed, so it is not sent again; the rest
    /// start unread.
    fn subscribe(&mut self, feeds: Vec<Feed>) {
        let mut old = std::mem::take(&mut self.feeds);
        self.feeds = feeds
            .into_iter()
            .map(|feed| {
                let state = old.remove(&feed).unwrap_or_default();
                (feed, state)
            })
            .collect();
    }

    /// One poll. Returns `false` once the receiver has gone.
    #[cfg(test)]
    fn tick(&mut self) -> bool {
        self.step().is_some()
    }

    /// One poll: the watch scopes in which a re-stat found a change since the last poll, or
    /// `None` once the receiver has gone, which ends the thread. A read forced by an eviction,
    /// a slug change or a first scan is no evidence of a missed change, so it names no scope.
    ///
    /// With no flow chosen yet the flow list goes first, since the runtime picks a flow from
    /// it; otherwise the viewed flow's snapshot does.
    fn step(&mut self) -> Option<BTreeSet<WatchScope>> {
        let rescan = self.last_flows.is_some();
        let change = self.scan_flows();
        let mut changed = BTreeSet::new();
        if rescan && change.is_some() {
            changed.insert(WatchScope::Flows);
        }
        let scopes_moved = self.scan_scope_files();
        changed.extend(scopes_moved.iter().copied().map(WatchScope::Ledgers));
        if self.slug.is_some() && self.poll_snapshot()? {
            changed.insert(WatchScope::Flows);
        }
        if !self.send_flows(change, !scopes_moved.is_empty()) {
            return None;
        }
        changed.extend(self.poll_feeds()?);
        Some(changed)
    }

    /// Drops the cached state `wakes` invalidates, so the next tick re-reads it. Only flow
    /// wakes touch the flow caches and the task fingerprint; repo and scope wakes re-read
    /// just their own feeds.
    fn evict(&mut self, wakes: &Wakes) {
        if wakes.all || wakes.rewatch {
            self.flow_stats.clear();
            self.last_fingerprint = None;
        } else {
            for slug in &wakes.flows {
                self.flow_stats.remove(slug);
            }
        }
        for (feed, state) in &mut self.feeds {
            if feed.woken_by(wakes) {
                state.stat = None;
            }
        }
    }

    /// Re-reads every subscribed feed whose file moved or was evicted, sending it when its
    /// revision differs from the last one sent. Returns the scopes of the feeds that moved, or
    /// `None` once the receiver has gone.
    fn poll_feeds(&mut self) -> Option<BTreeSet<WatchScope>> {
        let mut changed = BTreeSet::new();
        for (feed, state) in &mut self.feeds {
            let current = feed_path(&self.root, feed).and_then(|path| stat(&path));
            if state.stat == Some(current) {
                continue;
            }
            if state.stat.is_some() {
                changed.extend(feed.scope());
            }
            // Recorded before the read, as for snapshots: a write landing during it moves
            // the stat again.
            state.stat = Some(current);
            let fetched = match feed {
                Feed::Ledger(ledger) => self
                    .fetcher
                    .fetch_ledger(&self.root, ledger)
                    .map(FeedRead::Ledger),
                Feed::Inputs => self.fetcher.fetch_inputs(&self.root).map(FeedRead::Inputs),
            };
            let read = match fetched {
                Ok(read) => {
                    state.failing = false;
                    let revision = read.revision().as_deref();
                    if state.sent.is(revision) {
                        continue;
                    }
                    state.sent = Seen::read(revision);
                    Ok(read)
                }
                Err(e) => {
                    // Unrecorded, so the next tick retries a read that failed on a stable file.
                    state.stat = None;
                    state.sent = Seen::Unread;
                    if std::mem::replace(&mut state.failing, true) {
                        continue;
                    }
                    Err(e)
                }
            };
            self.events.send(feed_event(feed, read)).ok()?;
        }
        Some(changed)
    }

    /// Compares the flow-less ledger files with the last scan, returning the directories
    /// whose files came or went; the first scan returns none.
    fn scan_scope_files(&mut self) -> BTreeSet<LedgerKind> {
        let files = scope_files(&self.root);
        let Some(last) = self.last_scope_files.replace(files.clone()) else {
            return BTreeSet::new();
        };
        last.symmetric_difference(&files)
            .map(|(dir, _)| *dir)
            .collect()
    }

    /// When a failed fetch is next due a retry.
    fn retry_at(&self) -> Option<Instant> {
        self.failed_at
            .and_then(|at| at.checked_add(self.retry_after))
    }

    /// Compares the flows on disk with the last scan. A task store or ledger arriving or
    /// leaving, a moved `context.toml` (the selector shows its status and date), the first
    /// scan, or a failed last list ask for a relist; moved `tasks.toml` mtimes alone are sent
    /// as they are, with no relist.
    fn scan_flows(&mut self) -> Option<FlowsChange> {
        let flows = flows_fingerprint(&self.root, &mut self.flow_stats);
        let last = self.last_flows.replace(flows.clone());
        if last.as_ref() == Some(&flows) {
            return None;
        }
        let relist = self.relist_pending
            || last.is_none_or(|last| {
                !last.keys().eq(flows.keys())
                    || last.values().zip(flows.values()).any(|(a, b)| a.relists(b))
            });
        Some(if relist {
            FlowsChange::Relist
        } else {
            FlowsChange::Mtimes(task_stores(&flows))
        })
    }

    /// Sends what a flow scan found, with the ledger scopes after every relist, or alone when
    /// only the flow-less ledgers moved (`rescope`).
    fn send_flows(&mut self, change: Option<FlowsChange>, rescope: bool) -> bool {
        let relisted = matches!(change, Some(FlowsChange::Relist));
        let event = match change {
            None => None,
            Some(FlowsChange::Relist) => {
                let stores = task_stores(self.last_flows.as_ref().unwrap_or(&BTreeMap::new()));
                let listed = self.fetcher.list_flows(&self.root, &stores);
                self.relist_pending = listed.is_err();
                Some(Event::Flows(listed))
            }
            Some(FlowsChange::Mtimes(mtimes)) => Some(Event::FlowMtimes(mtimes)),
        };
        if let Some(event) = event
            && self.events.send(event).is_err()
        {
            return false;
        }
        if !relisted && !rescope {
            return true;
        }
        let scopes = self.fetcher.list_scopes(&self.root);
        self.events.send(Event::Scopes(scopes)).is_ok()
    }

    /// Fetches the viewed flow when its files moved or a failed fetch is due a retry. A flow
    /// with no task store is reported once and never fetched or retried. Returns whether the
    /// files moved since a recorded fingerprint, or `None` once the receiver has gone.
    fn poll_snapshot(&mut self) -> Option<bool> {
        let Some(slug) = self.slug.clone() else {
            return Some(false);
        };
        let current = fingerprint(&self.root, &slug);
        let changed = self.last_fingerprint != Some(current);
        let moved = self.last_fingerprint.is_some() && changed;
        let retry_due = self.retry_at().is_some_and(|at| Instant::now() >= at);
        if !changed && !retry_due {
            return Some(false);
        }
        // Recorded before the fetch: a write that lands during it moves the fingerprint again
        // and is picked up next tick.
        self.last_fingerprint = Some(current);
        if !has_task_store(&current) {
            self.failed_at = None;
            self.last_revision = None;
            if std::mem::replace(&mut self.no_store_reported, true) {
                return Some(moved);
            }
            let event = Event::SourceError(NO_TASK_STORE.into());
            return self.events.send(event).ok().map(|()| moved);
        }
        self.no_store_reported = false;
        let known = self.last_revision.as_deref();
        let event = match self.fetcher.fetch(&self.root, &slug, known) {
            Ok(None) => {
                self.failed_at = None;
                return Some(moved);
            }
            Ok(Some(snapshot)) => {
                self.failed_at = None;
                if self.last_revision.as_deref() == Some(snapshot.revision.as_str()) {
                    return Some(moved);
                }
                self.last_revision = Some(snapshot.revision.clone());
                Event::Snapshot(Box::new(snapshot))
            }
            Err(e) => {
                self.failed_at = Some(Instant::now());
                Event::SourceError(e)
            }
        };
        self.events.send(event).ok().map(|()| moved)
    }

    /// Applies one control message, merging a wake into `wakes`; `false` means stop.
    fn handle(&mut self, message: Control, wakes: &mut Wakes) -> bool {
        match message {
            Control::SetSlug(slug) => self.set_slug(slug),
            Control::Subscribe(feeds) => self.subscribe(feeds),
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
        let mut mode = Mode::start(&flows_root, control, poll, None);
        let mut misses = MissedChanges::default();
        let mut wakes = Wakes::default();
        let mut clock = SafetyClock::new(Instant::now(), safety_period(mode.is_watching(), poll));
        loop {
            // Only this phase can start a watch; a fresh one owes nothing to the last.
            let was_watching = mode.is_watching();
            mode.advance(Phase::Tick, &flows_root, control, poll);
            if mode.is_watching() && !was_watching {
                misses = MissedChanges::default();
            }
            let safety_due = clock.is_due(Instant::now());
            let cause = TickCause::of(&wakes, safety_due);
            let Some(changed) = self.step() else {
                return;
            };
            let abandon =
                mode.is_watching() && misses.on_scoped_tick(cause, &wakes.woken(), &changed);
            mode.advance(Phase::Ticked { abandon }, &flows_root, control, poll);
            let period = safety_period(mode.is_watching(), poll);
            clock.ticked(Instant::now(), safety_due, period);
            wakes = Wakes::default();
            loop {
                let deadlines = Deadlines {
                    safety: clock.at(),
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
                        break;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {
                        let due = deadlines.due(Instant::now());
                        if due.tail && !self.refresh_tail() {
                            return;
                        }
                        if due.safety || due.retry {
                            break;
                        }
                    }
                }
            }
            self.evict(&wakes);
            for dir in &wakes.new_scopes {
                mode.add_scope(*dir);
            }
            let woken = Phase::Woken {
                rewatch: wakes.rewatch,
            };
            mode.advance(woken, &flows_root, control, poll);
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

    /// Replaces the poller's feeds, sending [`Event::Ledger`] or [`Event::InputRecords`] for
    /// each new one at once and for any feed whenever its revision moves.
    pub(crate) fn subscribe(&self, feeds: Vec<Feed>) {
        let _ = self.control.send(Control::Subscribe(feeds));
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
    use super::scan::flow_dir;
    use super::scan::tests::{
        fake, fake_counting_lists, fake_inputs, fake_ledgers, inputs_with_revision,
        ledger_with_revision, temp_root, with_revision, write_in_place, write_renamed,
    };
    use super::*;
    use std::collections::BTreeSet;
    use std::sync::atomic::Ordering;

    fn drain(rx: &Receiver<Event>) -> Vec<Event> {
        rx.try_iter().collect()
    }

    /// Creates flow `slug` holding only a task store, and returns its directory.
    fn with_task_store(root: &std::path::Path, slug: &str) -> PathBuf {
        let dir = flow_dir(root, slug);
        std::fs::create_dir_all(&dir).expect("flow dir");
        write_renamed(&dir, "tasks.toml", 100);
        dir
    }

    #[test]
    fn an_unchanged_revision_is_not_sent_twice() {
        let root = temp_root("revision");
        let (fetcher, calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), Some("f".into()), fetcher, tx);

        let dir = with_task_store(&root, "f");
        assert!(poller.tick());
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
        let dir = with_task_store(&root, "f");
        assert!(poller.tick());
        assert!(poller.tick());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

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
        with_task_store(&root, "f");

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
            matches!(
                events.as_slice(),
                [Event::Flows(Ok(_)), Event::Scopes(Ok(_))]
            ),
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
            matches!(
                events.as_slice(),
                [Event::Flows(Ok(_)), Event::Scopes(Ok(_))]
            ),
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
            matches!(
                events.as_slice(),
                [Event::Flows(Ok(_)), Event::Scopes(Ok(_))]
            ),
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
        with_task_store(&root, "f");
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

    #[test]
    fn a_task_store_flow_is_restatted_every_scan_and_a_wake_evicts_a_bare_flow() {
        let root = temp_root("evict");
        let known = flow_dir(&root, "b");
        std::fs::create_dir_all(&known).expect("flow dir");
        write_renamed(&known, "tasks.toml", 100);
        write_renamed(&known, "context.toml", 100);
        let bare = flow_dir(&root, "c");
        std::fs::create_dir_all(&bare).expect("flow dir");
        let (fetcher, _calls, lists) = fake_counting_lists(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        assert!(poller.tick());
        drain(&rx);

        write_in_place(&known, "context.toml", 300);
        assert!(poller.tick());
        let events = drain(&rx);
        assert!(
            matches!(
                events.as_slice(),
                [Event::Flows(Ok(_)), Event::Scopes(Ok(_))]
            ),
            "an in-place write beside a task store is seen with no wake: {events:?}"
        );
        assert_eq!(lists.load(Ordering::SeqCst), 2);

        write_in_place(&bare, "tasks.toml", 400);
        assert!(poller.tick());
        assert!(
            drain(&rx).is_empty(),
            "an unmoved directory mtime hides a store created in place"
        );

        let mut wakes = Wakes::default();
        wakes.add(Wake::Flow("c".into()));
        poller.evict(&wakes);
        assert!(poller.tick());
        let events = drain(&rx);
        assert!(
            matches!(
                events.as_slice(),
                [Event::Flows(Ok(_)), Event::Scopes(Ok(_))]
            ),
            "{events:?}"
        );
        assert_eq!(lists.load(Ordering::SeqCst), 3);
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

    fn backlog_feed() -> Feed {
        Feed::Ledger(LedgerRef::Backlog)
    }

    fn write_backlog(root: &std::path::Path, body: &str) {
        std::fs::write(root.join(".claude").join("backlog.toml"), body).expect("backlog");
    }

    fn ledger_revisions(events: Vec<Event>) -> Vec<Option<String>> {
        events
            .into_iter()
            .filter_map(|e| match e {
                Event::Ledger { feed, ledger } => {
                    assert_eq!(feed, backlog_feed());
                    Some(ledger.expect("read").revision)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_ledger_change_posts_one_event() {
        let root = temp_root("ledger-change");
        let reads = ["r1", "r1", "r2"].map(|r| Ok(ledger_with_revision(Some(r))));
        let (fetcher, _calls, ledger_reads) = fake_ledgers(reads.into());
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        write_backlog(&root, "a");
        poller.subscribe(vec![backlog_feed()]);

        assert!(poller.tick());
        assert_eq!(ledger_revisions(drain(&rx)), [Some("r1".into())]);

        write_backlog(&root, "ab");
        assert!(poller.tick());
        assert!(
            ledger_revisions(drain(&rx)).is_empty(),
            "an unchanged revision is not sent"
        );

        write_backlog(&root, "abc");
        assert!(poller.tick());
        assert_eq!(ledger_revisions(drain(&rx)), [Some("r2".into())]);
        assert_eq!(ledger_reads.load(Ordering::SeqCst), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn input_revisions(events: Vec<Event>) -> Vec<Option<String>> {
        events
            .into_iter()
            .filter_map(|e| match e {
                Event::InputRecords(read) => Some(read.expect("read").revision),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_inputs_change_posts_one_event() {
        let root = temp_root("inputs-change");
        let reads = ["r1", "r1", "r2"].map(|r| Ok(inputs_with_revision(Some(r))));
        let (fetcher, input_reads) = fake_inputs(reads.into());
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        let write = |body: &str| {
            std::fs::write(root.join(".claude").join("inputs.toml"), body).expect("inputs");
        };
        write("a");
        poller.subscribe(vec![Feed::Inputs]);

        assert!(poller.tick());
        assert_eq!(input_revisions(drain(&rx)), [Some("r1".into())]);
        assert!(poller.tick());
        assert_eq!(
            input_reads.load(Ordering::SeqCst),
            1,
            "an idle tick reads nothing"
        );

        let mut wakes = Wakes::default();
        wakes.add(Wake::Repo(RepoFile::Inputs));
        poller.evict(&wakes);
        assert!(poller.tick());
        assert!(
            input_revisions(drain(&rx)).is_empty(),
            "a woken read of an unchanged revision is not sent"
        );

        write("ab");
        assert!(poller.tick());
        assert_eq!(input_revisions(drain(&rx)), [Some("r2".into())]);
        assert_eq!(input_reads.load(Ordering::SeqCst), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unchanged_ledger_is_not_refetched() {
        let root = temp_root("ledger-idle");
        let (fetcher, _calls, ledger_reads) = fake_ledgers(Vec::new());
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        write_backlog(&root, "a");
        poller.subscribe(vec![backlog_feed()]);
        for _ in 0..3 {
            assert!(poller.tick());
        }
        assert_eq!(ledger_reads.load(Ordering::SeqCst), 1);
        assert_eq!(ledger_revisions(drain(&rx)), [None]);

        poller.subscribe(vec![backlog_feed()]);
        assert!(poller.tick());
        assert_eq!(
            ledger_reads.load(Ordering::SeqCst),
            1,
            "a kept feed keeps its stat"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_backlog_wake_keeps_the_task_fingerprint() {
        let root = temp_root("backlog-wake");
        with_task_store(&root, "f");
        write_backlog(&root, "a");
        let (fetcher, calls, ledger_reads) = fake_ledgers(Vec::new());
        let (tx, _rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), Some("f".into()), fetcher, tx);
        poller.subscribe(vec![backlog_feed()]);
        assert!(poller.tick());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(ledger_reads.load(Ordering::SeqCst), 1);

        let mut wakes = Wakes::default();
        wakes.add(Wake::Repo(RepoFile::Backlog));
        poller.evict(&wakes);
        assert!(poller.last_fingerprint.is_some());
        assert!(poller.flow_stats.contains_key("f"));
        assert!(poller.tick());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "no snapshot refetch");
        assert_eq!(
            ledger_reads.load(Ordering::SeqCst),
            2,
            "the woken feed is re-read"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_ledger_only_flow_posts_its_scopes() {
        let root = temp_root("ledger-only");
        let (fetcher, _calls, lists) = fake_counting_lists(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        assert!(poller.tick());
        drain(&rx);

        let dir = flow_dir(&root, "l");
        std::fs::create_dir_all(&dir).expect("flow dir");
        write_renamed(&dir, "review-ledger.toml", 100);
        assert!(poller.tick());
        let events = drain(&rx);
        assert!(
            matches!(
                events.as_slice(),
                [Event::Flows(Ok(_)), Event::Scopes(Ok(_))]
            ),
            "{events:?}"
        );
        assert_eq!(lists.load(Ordering::SeqCst), 2);

        write_in_place(&dir, "review-ledger.toml", 200);
        assert!(poller.tick());
        assert!(
            drain(&rx).is_empty(),
            "a ledger write alone does not relist"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_flowless_ledger_posts_the_scopes() {
        let root = temp_root("flowless");
        let (fetcher, _calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), None, fetcher, tx);
        assert!(poller.tick());
        drain(&rx);

        let reviews = root.join(".claude").join("reviews");
        std::fs::create_dir_all(&reviews).expect("reviews dir");
        std::fs::write(reviews.join("main.toml"), "x").expect("ledger");
        assert!(poller.step().is_some());
        let events = drain(&rx);
        assert!(
            matches!(events.as_slice(), [Event::Scopes(Ok(_))]),
            "{events:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_flow_without_a_task_store_is_not_snapshotted() {
        let root = temp_root("no-store");
        let dir = flow_dir(&root, "l");
        std::fs::create_dir_all(&dir).expect("flow dir");
        write_renamed(&dir, "review-ledger.toml", 100);
        let (fetcher, calls) = fake(vec![Ok(with_revision("r1"))]);
        let (tx, rx) = mpsc::channel();
        let mut poller = Poller::new(root.clone(), Some("l".into()), fetcher, tx);
        poller.retry_after = Duration::ZERO;
        let errors = |events: Vec<Event>| {
            events
                .iter()
                .filter(|e| matches!(e, Event::SourceError(m) if m == NO_TASK_STORE))
                .count()
        };

        assert!(poller.tick());
        assert_eq!(errors(drain(&rx)), 1);
        assert!(poller.retry_at().is_none(), "no retry loop");
        write_renamed(&dir, "context.toml", 200);
        assert!(poller.tick());
        assert_eq!(errors(drain(&rx)), 0, "reported once");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "never fetched");

        write_renamed(&dir, "tasks.toml", 300);
        assert!(poller.tick());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "a task store is fetched");
        let _ = std::fs::remove_dir_all(&root);
    }
}
