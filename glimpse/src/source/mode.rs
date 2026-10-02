//! Whether the poller is watching or polling, and the deadlines that pace its ticks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use notify::RecommendedWatcher;
use tomlctl::LedgerKind;

use super::{Control, MISS_LIMIT, SAFETY_TICK, WATCH_RETRY_MAX};
use crate::watch::{self, RepoFile, Wake};

/// Every wake gathered before one tick, merged: an `All` or `Rewatch` covers every flow.
/// A new scope dir is also listed in `scopes`, so it is rescanned once.
#[derive(Debug, Default)]
pub(super) struct Wakes {
    /// Every flow woken at all; drives the flow caches and the watch-scope accounting.
    pub(super) flows: BTreeSet<String>,
    /// The flows woken as a whole, whose every ledger feed is re-read.
    pub(super) whole_flows: BTreeSet<String>,
    /// The `(slug, entry)` pairs woken, each re-reading only the ledger feed of that file.
    pub(super) flow_files: BTreeSet<(String, String)>,
    pub(super) repo: BTreeSet<RepoFile>,
    pub(super) scopes: BTreeSet<LedgerKind>,
    pub(super) new_scopes: BTreeSet<LedgerKind>,
    pub(super) all: bool,
    pub(super) rewatch: bool,
}

impl Wakes {
    pub(super) fn add(&mut self, wake: Wake) {
        match wake {
            Wake::Flow { slug, file } => {
                self.flows.insert(slug.clone());
                match file {
                    Some(file) => self.flow_files.insert((slug, file)),
                    None => self.whole_flows.insert(slug),
                };
            }
            Wake::Repo(file) => {
                self.repo.insert(file);
            }
            Wake::Scope(dir) => {
                self.scopes.insert(dir);
            }
            Wake::NewScope(dir) => {
                self.new_scopes.insert(dir);
                self.scopes.insert(dir);
            }
            Wake::All => self.all = true,
            Wake::Rewatch => self.rewatch = true,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.flows.is_empty()
            && self.repo.is_empty()
            && self.scopes.is_empty()
            && self.new_scopes.is_empty()
            && !self.all
            && !self.rewatch
    }

    /// Whether these wakes name `file` of flow `slug`, alone or as part of the whole flow.
    pub(super) fn woke_flow_file(&self, slug: &str, file: &str) -> bool {
        self.whole_flows.contains(slug)
            || self.flow_files.iter().any(|(s, f)| s == slug && f == file)
    }

    /// The scopes these wakes prove alive. An `All` or `Rewatch` names none, since neither
    /// can be traced to the watch that produced it.
    pub(super) fn woken(&self) -> BTreeSet<WatchScope> {
        let flows = (!self.flows.is_empty()).then_some(WatchScope::Flows);
        let repo = (!self.repo.is_empty()).then_some(WatchScope::Repo);
        flows
            .into_iter()
            .chain(repo)
            .chain(self.scopes.iter().copied().map(WatchScope::Ledgers))
            .collect()
    }
}

/// A part of the watch whose wakes vouch for that part alone: on Windows each watched path
/// has its own handle, and one can die while the others keep reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum WatchScope {
    /// `.claude/flows`, recursive.
    Flows,
    /// The repo-level files directly under `.claude`.
    Repo,
    /// The flow-less ledger directory of one kind.
    Ledgers(LedgerKind),
}

/// Whether a watch is feeding the poller. The watcher lives here, on the poller thread.
pub(super) enum Mode {
    /// The watch lives until the mode changes; `scopes` are the kinds whose flow-less ledger
    /// dirs it already covers, so none is watched twice.
    Watching {
        watcher: RecommendedWatcher,
        claude_dir: PathBuf,
        scopes: BTreeSet<LedgerKind>,
    },
    /// Polling every `poll_ms` and retrying the watch at ticks, because the last start failed
    /// or a rewatch dropped the watcher. `backoff` is set once `watch::start` itself has
    /// failed, and holds further attempts off until it allows one.
    Retrying { backoff: Option<Backoff> },
    /// Polling every `poll_ms` for good: the watch missed changes.
    Abandoned,
}

/// When a watch that failed to start may be tried again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Backoff {
    at: Instant,
    delay: Duration,
}

impl Backoff {
    /// The wait after a failure at `now`: `poll` after the first, then double the last
    /// wait, capped at [`WATCH_RETRY_MAX`].
    fn after_failure(prior: Option<Backoff>, now: Instant, poll: Duration) -> Backoff {
        let delay = prior
            .map_or(poll, |b| b.delay.saturating_mul(2))
            .min(WATCH_RETRY_MAX);
        Backoff {
            at: now.checked_add(delay).unwrap_or(now),
            delay,
        }
    }

    fn allows(&self, now: Instant) -> bool {
        now >= self.at
    }
}

impl Mode {
    /// Watches `flows_root` and the `.claude` dir above it, forwarding every wake onto the
    /// poller's own control channel. A missing `flows_root` is probed on every call; an actual
    /// start is attempted only once `prior` allows it, and its failure lengthens the backoff.
    pub(super) fn start(
        flows_root: &Path,
        control: &Sender<Control>,
        poll: Duration,
        prior: Option<Backoff>,
    ) -> Mode {
        let retrying = |backoff| Mode::Retrying { backoff };
        if !flows_root.is_dir() {
            return retrying(prior);
        }
        let now = Instant::now();
        if prior.is_some_and(|b| !b.allows(now)) {
            return retrying(prior);
        }
        let claude_dir = flows_root.parent().unwrap_or(flows_root).to_path_buf();
        let tx = control.clone();
        match watch::start(&claude_dir, move |wake| {
            let _ = tx.send(Control::Wake(wake));
        }) {
            Ok((watcher, scopes)) => Mode::Watching {
                watcher,
                claude_dir,
                scopes,
            },
            Err(_) => retrying(Some(Backoff::after_failure(prior, now, poll))),
        }
    }

    /// Adds a watch on a flow-less ledger dir that appeared after the watch started. A dir
    /// already covered, a missing one, or a failed watch leaves the mode as it is; the
    /// safety tick still sees that scope's changes.
    pub(super) fn add_scope(&mut self, dir: LedgerKind) {
        let Mode::Watching {
            watcher,
            claude_dir,
            scopes,
        } = self
        else {
            return;
        };
        if scopes.contains(&dir) {
            return;
        }
        let path = watch::Roots::new(claude_dir).scope_dir(dir);
        if let Ok(true) = watch::add_scope(watcher, &path) {
            scopes.insert(dir);
        }
    }

    pub(super) fn is_watching(&self) -> bool {
        matches!(self, Mode::Watching { .. })
    }

    /// Applies whatever [`mode_change`] decides at `phase`. Replacing a `Watching` mode drops
    /// its watcher.
    pub(super) fn advance(
        &mut self,
        phase: Phase,
        flows_root: &Path,
        control: &Sender<Control>,
        poll: Duration,
    ) {
        match mode_change(self, phase) {
            Some(ModeChange::Start { prior }) => {
                *self = Mode::start(flows_root, control, poll, prior);
            }
            Some(ModeChange::Retry) => *self = Mode::Retrying { backoff: None },
            Some(ModeChange::Abandon) => *self = Mode::Abandoned,
            None => {}
        }
    }
}

/// The points in the poller's loop at which its mode may change.
#[derive(Debug, Clone, Copy)]
pub(super) enum Phase {
    /// Before a tick.
    Tick,
    /// After a tick; `abandon` when the missed-change count gave up on the watch.
    Ticked { abandon: bool },
    /// After a wait; `rewatch` when a wake asked for a fresh watch.
    Woken { rewatch: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeChange {
    /// Try a watch start, held off by the retrying mode's `prior` backoff.
    Start {
        prior: Option<Backoff>,
    },
    Retry,
    Abandon,
}

/// Whether the watch starts, stops or stays at `phase`: a failed start is retried before a
/// tick once its [`Backoff`] allows, a watch that missed changes is abandoned for good, and a
/// rewatch drops the watcher so the next tick starts a fresh one.
fn mode_change(mode: &Mode, phase: Phase) -> Option<ModeChange> {
    match (mode, phase) {
        (Mode::Retrying { backoff }, Phase::Tick) => Some(ModeChange::Start { prior: *backoff }),
        (Mode::Watching { .. }, Phase::Ticked { abandon: true }) => Some(ModeChange::Abandon),
        (Mode::Watching { .. }, Phase::Woken { rewatch: true }) => Some(ModeChange::Retry),
        _ => None,
    }
}

/// How long the poller goes between ticks when nothing wakes it.
pub(super) fn safety_period(watching: bool, poll: Duration) -> Duration {
    if watching { SAFETY_TICK } else { poll }
}

/// The instants at which the poller wakes of its own accord.
pub(super) struct Deadlines {
    /// The next safety tick; moves only when a tick runs.
    pub(super) safety: Instant,
    /// When a failed fetch is due another try.
    pub(super) retry: Option<Instant>,
    /// When the targeted transcript is next re-read.
    pub(super) tail: Option<Instant>,
}

/// Which deadlines had been reached when a wait ran out.
pub(super) struct Due {
    pub(super) safety: bool,
    pub(super) retry: bool,
    pub(super) tail: bool,
}

impl Deadlines {
    pub(super) fn due(&self, now: Instant) -> Due {
        let reached = |at: Option<Instant>| at.is_some_and(|at| now >= at);
        Due {
            safety: now >= self.safety,
            retry: reached(self.retry),
            tail: reached(self.tail),
        }
    }
}

/// How long to wait for a message before the earliest deadline; zero once one has passed.
pub(super) fn next_wait(now: Instant, deadlines: &Deadlines) -> Duration {
    [Some(deadlines.safety), deadlines.retry, deadlines.tail]
        .into_iter()
        .flatten()
        .min()
        .map_or(Duration::ZERO, |at| at.saturating_duration_since(now))
}

/// The safety deadline. It moves only when a safety tick runs or the period changes, so wakes
/// arriving more often than the period cannot hold it off.
#[derive(Debug, Clone, Copy)]
pub(super) struct SafetyClock {
    period: Duration,
    at: Instant,
}

impl SafetyClock {
    pub(super) fn new(now: Instant, period: Duration) -> SafetyClock {
        SafetyClock {
            period,
            at: now.checked_add(period).unwrap_or(now + SAFETY_TICK),
        }
    }

    pub(super) fn at(&self) -> Instant {
        self.at
    }

    pub(super) fn is_due(&self, now: Instant) -> bool {
        now >= self.at
    }

    /// After a tick at `now` that ran with the deadline `due` or not: a due deadline or a
    /// changed `period` starts the next one from `now`.
    pub(super) fn ticked(&mut self, now: Instant, due: bool, period: Duration) {
        if due || period != self.period {
            *self = SafetyClock::new(now, period);
        }
    }
}

/// What set off a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TickCause {
    /// A watch wake.
    Wake,
    /// The safety deadline, whether or not wakes arrived with it.
    Safety,
    /// Anything else: the first tick, a slug change, a fetch retry, or an `All` or `Rewatch`
    /// wake, whose forced re-reads say nothing about which scope missed what.
    Other,
}

impl TickCause {
    pub(super) fn of(wakes: &Wakes, safety_due: bool) -> TickCause {
        if wakes.all || wakes.rewatch {
            TickCause::Other
        } else if safety_due {
            TickCause::Safety
        } else if wakes.is_empty() {
            TickCause::Other
        } else {
            TickCause::Wake
        }
    }
}

/// Counts, per [`WatchScope`], the ticks since that scope last woke that found a change in it
/// no wake had reported. Only a scope's own wake proves it alive, so a clean tick leaves the
/// count alone and a busy scope cannot vouch for a dead one.
#[derive(Debug, Default)]
pub(super) struct MissedChanges {
    streaks: BTreeMap<WatchScope, u8>,
}

impl MissedChanges {
    /// Records one tick: `woken` scopes start over, and on a safety or wake tick every other
    /// scope in `changed` counts a miss, since another scope's wake tick takes up the change a
    /// later safety tick would have found. `true` means some scope has missed enough to abandon
    /// the watch.
    pub(super) fn on_scoped_tick(
        &mut self,
        cause: TickCause,
        woken: &BTreeSet<WatchScope>,
        changed: &BTreeSet<WatchScope>,
    ) -> bool {
        for scope in woken {
            self.streaks.remove(scope);
        }
        if cause != TickCause::Other {
            for scope in changed.difference(woken) {
                let streak = self.streaks.entry(*scope).or_default();
                *streak = streak.saturating_add(1);
            }
        }
        self.streaks.values().any(|&streak| streak >= MISS_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TICK;
    use crate::flows;
    use crate::source::scan::tests::temp_root;
    use std::sync::mpsc;

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

    fn scopes<const N: usize>(scopes: [WatchScope; N]) -> BTreeSet<WatchScope> {
        BTreeSet::from(scopes)
    }

    #[test]
    fn two_unreported_changes_with_no_wake_between_abandon_the_watch() {
        use TickCause::{Other, Safety, Wake};
        let none = scopes([]);
        let flows = scopes([WatchScope::Flows]);
        let mut misses = MissedChanges::default();
        assert!(!misses.on_scoped_tick(Safety, &none, &flows), "one miss");
        assert!(
            misses.on_scoped_tick(Safety, &none, &flows),
            "the second in a row"
        );

        let mut misses = MissedChanges::default();
        assert!(!misses.on_scoped_tick(Safety, &none, &flows));
        assert!(
            !misses.on_scoped_tick(Wake, &flows, &flows),
            "a wake resets"
        );
        assert!(
            !misses.on_scoped_tick(Safety, &none, &flows),
            "counting starts over"
        );

        let mut misses = MissedChanges::default();
        assert!(!misses.on_scoped_tick(Safety, &none, &flows));
        assert!(
            !misses.on_scoped_tick(Other, &none, &flows),
            "only safety ticks count"
        );
        assert!(
            !misses.on_scoped_tick(Safety, &none, &none),
            "a clean safety tick neither counts nor resets"
        );
        assert!(
            misses.on_scoped_tick(Safety, &none, &flows),
            "the second miss since the last wake"
        );
    }

    #[test]
    fn a_busy_scope_does_not_mask_a_dead_one() {
        use TickCause::{Safety, Wake};
        let repo = scopes([WatchScope::Repo]);
        let mut misses = MissedChanges::default();
        assert!(!misses.on_scoped_tick(
            Safety,
            &repo,
            &scopes([WatchScope::Flows, WatchScope::Repo])
        ));
        assert!(
            !misses.on_scoped_tick(Wake, &repo, &repo),
            "repo stays busy"
        );
        assert!(
            misses.on_scoped_tick(Safety, &repo, &scopes([WatchScope::Flows])),
            "the flows scope missed twice, whatever the repo did"
        );
    }

    #[test]
    fn frequent_repo_wakes_do_not_starve_the_flows_miss_count() {
        // A repo wake every second; the dead flows watch misses writes at 3.5 s and 13.5 s.
        let start = Instant::now();
        let mut clock = SafetyClock::new(start, SAFETY_TICK);
        let mut misses = MissedChanges::default();
        let mut safety_ticks = Vec::new();
        let mut abandoned_at = None;
        for second in 1..=30u64 {
            let now = start + Duration::from_secs(second);
            let mut wakes = Wakes::default();
            wakes.add(Wake::Repo(RepoFile::Backlog));
            let due = clock.is_due(now);
            let cause = TickCause::of(&wakes, due);
            if cause == TickCause::Safety {
                safety_ticks.push(second);
            }
            let mut changed = scopes([WatchScope::Repo]);
            if second == 4 || second == 14 {
                changed.insert(WatchScope::Flows);
            }
            if misses.on_scoped_tick(cause, &wakes.woken(), &changed) && abandoned_at.is_none() {
                abandoned_at = Some(second);
            }
            clock.ticked(now, due, SAFETY_TICK);
        }
        assert_eq!(
            safety_ticks,
            [10, 20, 30],
            "wakes never hold off the safety tick"
        );
        assert_eq!(abandoned_at, Some(14), "the flows scope missed twice");
    }

    #[test]
    fn an_all_wake_counts_nothing_and_a_new_period_restarts_the_clock() {
        let mut wakes = Wakes::default();
        wakes.add(Wake::All);
        assert_eq!(TickCause::of(&wakes, true), TickCause::Other);
        assert_eq!(TickCause::of(&Wakes::default(), false), TickCause::Other);
        assert_eq!(TickCause::of(&Wakes::default(), true), TickCause::Safety);

        let start = Instant::now();
        let mut clock = SafetyClock::new(start, SAFETY_TICK);
        let later = start + Duration::from_secs(1);
        clock.ticked(later, false, SAFETY_TICK);
        assert_eq!(
            clock.at(),
            start + SAFETY_TICK,
            "an early tick leaves it alone"
        );
        clock.ticked(later, false, Duration::from_millis(500));
        assert_eq!(clock.at(), later + Duration::from_millis(500));
    }

    #[test]
    fn a_wake_vouches_only_for_its_own_scope_and_a_reported_change_is_no_miss() {
        use TickCause::Safety;
        let reviews = WatchScope::Ledgers(LedgerKind::Review);
        let both = scopes([WatchScope::Flows, reviews]);
        let mut misses = MissedChanges::default();
        for _ in 0..3 {
            assert!(
                !misses.on_scoped_tick(Safety, &both, &both),
                "both woke, so neither missed"
            );
        }
        assert!(
            !misses.on_scoped_tick(Safety, &scopes([]), &both),
            "the first miss since the wakes"
        );

        let mut misses = MissedChanges::default();
        assert!(!misses.on_scoped_tick(Safety, &scopes([]), &both));
        assert!(
            misses.on_scoped_tick(Safety, &scopes([reviews]), &both),
            "the reviews wake did not reset flows"
        );
    }

    #[test]
    fn wakes_name_the_scopes_they_prove_alive() {
        let mut wakes = Wakes::default();
        wakes.add(Wake::Flow {
            slug: "a".into(),
            file: Some("agents.toml".into()),
        });
        wakes.add(Wake::Repo(RepoFile::Inputs));
        wakes.add(Wake::NewScope(LedgerKind::Optimise));
        assert_eq!(
            wakes.woken(),
            scopes([
                WatchScope::Flows,
                WatchScope::Repo,
                WatchScope::Ledgers(LedgerKind::Optimise)
            ])
        );

        let mut wakes = Wakes::default();
        wakes.add(Wake::All);
        wakes.add(Wake::Rewatch);
        assert!(wakes.woken().is_empty(), "untraceable to one watch");
    }

    #[test]
    fn a_flow_file_wake_names_that_file_and_a_whole_flow_wake_every_file() {
        let mut wakes = Wakes::default();
        wakes.add(Wake::Flow {
            slug: "a".into(),
            file: Some("agents.toml".into()),
        });
        wakes.add(Wake::Flow {
            slug: "b".into(),
            file: None,
        });
        assert_eq!(wakes.flows, BTreeSet::from(["a".into(), "b".into()]));
        assert!(wakes.woke_flow_file("a", "agents.toml"));
        assert!(!wakes.woke_flow_file("a", "review-ledger.toml"));
        assert!(wakes.woke_flow_file("b", "review-ledger.toml"));
        assert!(!wakes.woke_flow_file("c", "agents.toml"));
    }

    #[test]
    fn a_failed_watch_start_backs_off_from_the_poll_interval_to_the_cap() {
        let now = Instant::now();
        let poll = Duration::from_millis(500);
        let mut backoff = None;
        let mut delays = Vec::new();
        for _ in 0..10 {
            let next = Backoff::after_failure(backoff, now, poll);
            delays.push(next.delay.as_millis());
            backoff = Some(next);
        }
        assert_eq!(
            delays,
            [
                500, 1000, 2000, 4000, 8000, 16000, 32000, 60000, 60000, 60000
            ]
        );

        let first = Backoff::after_failure(None, now, poll);
        assert!(!first.allows(now), "no second attempt at once");
        assert!(first.allows(now + poll));
    }

    #[test]
    fn a_pending_backoff_holds_off_a_start_and_a_missing_root_leaves_it_alone() {
        let root = temp_root("backoff");
        let (control, _messages) = mpsc::channel();
        let missing = root.join("absent");
        let prior = Backoff::after_failure(None, Instant::now(), Duration::from_secs(3600));
        let mode = Mode::start(&missing, &control, Duration::from_millis(500), Some(prior));
        assert!(
            matches!(mode, Mode::Retrying { backoff: Some(b) } if b == prior),
            "no attempt, no longer wait"
        );

        let present = flows::flows_root(&root);
        let mode = Mode::start(&present, &control, Duration::from_millis(500), Some(prior));
        assert!(
            matches!(mode, Mode::Retrying { backoff: Some(b) } if b == prior),
            "held off until due"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repo_and_scope_wakes_are_kept_apart_from_flows() {
        let mut wakes = Wakes::default();
        wakes.add(Wake::Repo(RepoFile::Backlog));
        assert!(!wakes.is_empty());
        assert!(wakes.flows.is_empty() && !wakes.all, "no flow is evicted");

        let mut wakes = Wakes::default();
        wakes.add(Wake::NewScope(LedgerKind::Review));
        assert_eq!(wakes.new_scopes, BTreeSet::from([LedgerKind::Review]));
        assert_eq!(
            wakes.scopes,
            BTreeSet::from([LedgerKind::Review]),
            "a new scope is rescanned once"
        );
    }

    #[test]
    fn a_new_scope_is_watched_once_and_a_missing_one_not_at_all() {
        let root = temp_root("scopes");
        let (control, _messages) = mpsc::channel();
        let flows_root = flows::flows_root(&root);
        let mut mode = Mode::start(&flows_root, &control, Duration::from_millis(500), None);
        let covered = |mode: &Mode| match mode {
            Mode::Watching { scopes, .. } => scopes.clone(),
            Mode::Retrying { .. } | Mode::Abandoned => panic!("not watching"),
        };
        assert!(covered(&mode).is_empty());

        mode.add_scope(LedgerKind::Review);
        assert!(covered(&mode).is_empty(), "missing dir");

        std::fs::create_dir(root.join(".claude").join("reviews")).expect("reviews dir");
        mode.add_scope(LedgerKind::Review);
        mode.add_scope(LedgerKind::Review);
        assert_eq!(covered(&mode), BTreeSet::from([LedgerKind::Review]));

        let restarted = Mode::start(&flows_root, &control, Duration::from_millis(500), None);
        assert_eq!(
            covered(&restarted),
            BTreeSet::from([LedgerKind::Review]),
            "an existing dir is covered from the start"
        );
        drop((mode, restarted));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_watch_is_retried_abandoned_and_restarted_at_the_right_phases() {
        let root = temp_root("phases");
        let (control, _messages) = mpsc::channel();
        let watching = Mode::start(
            &flows::flows_root(&root),
            &control,
            Duration::from_millis(500),
            None,
        );
        assert!(watching.is_watching());
        let prior = Backoff::after_failure(None, Instant::now(), Duration::from_millis(500));
        let retrying = Mode::Retrying {
            backoff: Some(prior),
        };
        let abandoned = Mode::Abandoned;

        assert_eq!(
            mode_change(&retrying, Phase::Tick),
            Some(ModeChange::Start { prior: Some(prior) }),
            "a failed start, under its backoff"
        );
        assert_eq!(mode_change(&watching, Phase::Tick), None);
        assert_eq!(mode_change(&abandoned, Phase::Tick), None, "never again");

        let abandon = Phase::Ticked { abandon: true };
        assert_eq!(mode_change(&watching, abandon), Some(ModeChange::Abandon));
        assert_eq!(
            mode_change(&watching, Phase::Ticked { abandon: false }),
            None
        );
        assert_eq!(mode_change(&retrying, abandon), None, "no watch to drop");

        let rewatch = Phase::Woken { rewatch: true };
        assert_eq!(
            mode_change(&watching, rewatch),
            Some(ModeChange::Retry),
            "the watcher is dropped and the next tick starts a fresh one"
        );
        assert_eq!(
            mode_change(&watching, Phase::Woken { rewatch: false }),
            None
        );
        assert_eq!(mode_change(&retrying, rewatch), None);
        assert_eq!(mode_change(&abandoned, rewatch), None);
        drop(watching);
        let _ = std::fs::remove_dir_all(&root);
    }
}
