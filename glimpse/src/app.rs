//! Application state and the actions that change it.
//!
//! `App` owns the current snapshot and everything the views read besides the
//! config: selection, overlays, flashes and staleness. It never draws and
//! never spawns; the runtime feeds it actions and snapshots and carries out
//! the few actions `apply` hands back.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime};

use crate::config::{Config, Orientation, OrientationPref, ViewKind};
use crate::diff::{Changes, diff};
use crate::flows::FlowEntry;
use crate::model::{AgentStatus, Index, Snapshot, TaskStatus};
use crate::theme::Theme;

/// How long a task stays highlighted after its status changes.
pub(crate) const FLASH: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Spatial movement over whatever the active view draws. The view installs a
/// fresh one each frame, since only it knows where each task sits.
pub(crate) trait Navigator {
    fn neighbor(&self, from: u32, dir: Dir) -> Option<u32>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    /// A selection move, or a cursor move while the selector is open.
    Move(Dir),
    NextView,
    PrevView,
    FlipOrientation,
    ToggleFollow,
    ToggleAutoFlow,
    ToggleActivity,
    ToggleSelector,
    SelectorNext,
    SelectorPrev,
    /// Opens details, then makes them full-screen, then closes them; with the
    /// selector open it switches to the flow under the cursor instead.
    Details,
    SwitchFlow(String),
    /// Closes the topmost overlay, or quits when none is open.
    Back,
    Quit,
}

pub(crate) struct App {
    pub(crate) snapshot: Snapshot,
    pub(crate) index: Index,
    pub(crate) view: ViewKind,
    pub(crate) orientation_override: Option<Orientation>,
    /// The orientation the last frame drew in; `o` flips from this when no
    /// override is set yet.
    pub(crate) resolved_orientation: Orientation,
    pub(crate) selected: Option<u32>,
    /// While on, every snapshot moves the selection to the frontier.
    pub(crate) follow: bool,
    /// While on, the runtime switches to the freshest flow as flows change.
    pub(crate) auto_flow: bool,
    pub(crate) details_open: bool,
    pub(crate) details_fullscreen: bool,
    pub(crate) activity_open: bool,
    pub(crate) selector_open: bool,
    pub(crate) flows: Vec<FlowEntry>,
    pub(crate) selector_cursor: usize,
    /// Task id to the instant its status last changed; live for [`FLASH`].
    pub(crate) flashes: HashMap<u32, Instant>,
    /// Changes that arrived while follow was paused.
    pub(crate) pending_changes: usize,
    pub(crate) warning: Option<String>,
    pub(crate) source_error: Option<String>,
    /// Ids of `running` agents whose transcript has gone quiet.
    pub(crate) stale_agents: HashSet<String>,
    pub(crate) stale_after: Duration,
    pub(crate) theme: Theme,
    pub(crate) nav: Option<Box<dyn Navigator>>,
}

impl App {
    pub(crate) fn new(snapshot: Snapshot, config: &Config) -> App {
        let resolved_orientation = match config.orientation {
            OrientationPref::Fixed(o) => o,
            OrientationPref::Auto => Orientation::Vertical,
        };
        let mut app = App {
            index: snapshot.index(),
            snapshot,
            view: config.default_view,
            orientation_override: None,
            resolved_orientation,
            selected: None,
            follow: true,
            auto_flow: false,
            details_open: false,
            details_fullscreen: false,
            activity_open: false,
            selector_open: false,
            flows: Vec::new(),
            selector_cursor: 0,
            flashes: HashMap::new(),
            pending_changes: 0,
            warning: None,
            source_error: None,
            stale_agents: HashSet::new(),
            stale_after: Duration::from_secs(config.stale_after_s),
            theme: Theme::default(),
            nav: None,
        };
        app.refresh_stale();
        app.reselect();
        app
    }

    /// Returns the action when only the runtime can carry it out: `Quit`, or
    /// `SwitchFlow` to a flow other than the current one.
    pub(crate) fn apply(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::Move(dir) if self.selector_open => match dir {
                Dir::Up => self.apply(Action::SelectorPrev),
                Dir::Down => self.apply(Action::SelectorNext),
                Dir::Left | Dir::Right => None,
            },
            Action::Move(dir) => {
                self.move_selection(dir);
                None
            }
            Action::NextView => {
                self.view = self.view.next();
                self.nav = None;
                None
            }
            Action::PrevView => {
                self.view = self.view.prev();
                self.nav = None;
                None
            }
            Action::FlipOrientation => {
                let current = self
                    .orientation_override
                    .unwrap_or(self.resolved_orientation);
                self.orientation_override = Some(current.flip());
                self.nav = None;
                None
            }
            Action::ToggleFollow => {
                self.follow = !self.follow;
                if self.follow {
                    self.pending_changes = 0;
                    self.reselect();
                }
                None
            }
            Action::ToggleAutoFlow => {
                self.auto_flow = !self.auto_flow;
                None
            }
            Action::ToggleActivity => {
                self.activity_open = !self.activity_open;
                None
            }
            Action::ToggleSelector => {
                self.selector_open = !self.selector_open;
                if self.selector_open {
                    self.selector_cursor = self
                        .flows
                        .iter()
                        .position(|flow| flow.slug == self.snapshot.slug)
                        .unwrap_or(0);
                }
                None
            }
            Action::SelectorNext => {
                let last = self.flows.len().saturating_sub(1);
                self.selector_cursor = (self.selector_cursor + 1).min(last);
                None
            }
            Action::SelectorPrev => {
                let last = self.flows.len().saturating_sub(1);
                self.selector_cursor = self.selector_cursor.saturating_sub(1).min(last);
                None
            }
            Action::Details if self.selector_open => match self.flows.get(self.selector_cursor) {
                Some(flow) => self.apply(Action::SwitchFlow(flow.slug.clone())),
                None => None,
            },
            Action::Details => {
                if !self.details_open {
                    self.details_open = true;
                } else if !self.details_fullscreen {
                    self.details_fullscreen = true;
                } else {
                    self.details_open = false;
                    self.details_fullscreen = false;
                }
                None
            }
            Action::SwitchFlow(slug) => {
                self.selector_open = false;
                if slug == self.snapshot.slug {
                    return None;
                }
                // A hand-picked flow would be switched away from at the next
                // flow change otherwise.
                self.auto_flow = false;
                Some(Action::SwitchFlow(slug))
            }
            Action::Back => {
                if self.selector_open {
                    self.selector_open = false;
                } else if self.details_fullscreen {
                    self.details_fullscreen = false;
                } else if self.details_open {
                    self.details_open = false;
                } else if self.activity_open {
                    self.activity_open = false;
                } else {
                    return Some(Action::Quit);
                }
                None
            }
            Action::Quit => Some(Action::Quit),
        }
    }

    /// Swaps in `snapshot`, flashing each task whose status changed. A
    /// snapshot of a different flow replaces the old one without flashes.
    pub(crate) fn apply_snapshot(&mut self, snapshot: Snapshot, now: Instant) {
        if snapshot.slug == self.snapshot.slug {
            let changes = diff(&self.snapshot, &snapshot);
            for (id, _, _) in &changes.status_changed {
                self.flashes.insert(*id, now);
            }
            if !self.follow {
                self.pending_changes += change_count(&changes);
            }
        } else {
            self.flashes.clear();
            self.pending_changes = 0;
            self.selected = None;
        }
        self.index = snapshot.index();
        self.snapshot = snapshot;
        self.source_error = None;
        self.expire_flashes(now);
        self.refresh_stale();
        self.reselect();
    }

    /// Called on each timed wake-up: drops expired flashes and re-reads
    /// transcript ages, since an agent goes stale without any snapshot.
    pub(crate) fn tick(&mut self, now: Instant) {
        self.expire_flashes(now);
        self.refresh_stale();
    }

    /// True while something on screen changes with time alone: a live flash,
    /// or a running agent whose elapsed counter is still advancing.
    pub(crate) fn needs_tick(&self, now: Instant) -> bool {
        self.flashes
            .values()
            .any(|at| now.saturating_duration_since(*at) < FLASH)
            || self.snapshot.agents.iter().any(|agent| {
                agent.status == AgentStatus::Running && !self.stale_agents.contains(&agent.id)
            })
    }

    pub(crate) fn is_flashing(&self, id: u32, now: Instant) -> bool {
        self.flashes
            .get(&id)
            .is_some_and(|at| now.saturating_duration_since(*at) < FLASH)
    }

    fn expire_flashes(&mut self, now: Instant) {
        self.flashes
            .retain(|_, at| now.saturating_duration_since(*at) < FLASH);
    }

    fn refresh_stale(&mut self) {
        self.stale_agents = stale_agents(
            &self.snapshot,
            self.stale_after,
            SystemTime::now(),
            transcript_mtime,
        );
    }

    fn move_selection(&mut self, dir: Dir) {
        let Some(from) = self.selected else {
            self.selected = self.frontier().or_else(|| self.first_task());
            return;
        };
        let Some(to) = self.nav.as_ref().and_then(|nav| nav.neighbor(from, dir)) else {
            return;
        };
        if to != from {
            self.selected = Some(to);
            self.follow = false;
        }
    }

    /// With follow on the selection jumps to the frontier. Otherwise, and
    /// when the frontier is empty, a selection that still names a task stays.
    fn reselect(&mut self) {
        if self.follow
            && let Some(id) = self.frontier()
        {
            self.selected = Some(id);
            return;
        }
        let still_present = self
            .selected
            .is_some_and(|id| self.index.task(&self.snapshot, id).is_some());
        if !still_present {
            self.selected = self.frontier().or_else(|| self.first_task());
        }
    }

    /// The first in-progress task by (layer, id), else the first ready one.
    fn frontier(&self) -> Option<u32> {
        let key = |id: &u32| (self.index.layer_of(*id).unwrap_or(usize::MAX), *id);
        self.snapshot
            .tasks
            .iter()
            .filter(|task| task.status == TaskStatus::InProgress)
            .map(|task| task.id)
            .min_by_key(key)
            .or_else(|| self.snapshot.frontier.ready.iter().copied().min_by_key(key))
    }

    fn first_task(&self) -> Option<u32> {
        self.snapshot
            .tasks
            .iter()
            .map(|task| task.id)
            .min_by_key(|id| (self.index.layer_of(*id).unwrap_or(usize::MAX), *id))
    }
}

fn change_count(changes: &Changes) -> usize {
    usize::from(changes.topology_changed)
        + changes.status_changed.len()
        + changes.agents_started.len()
        + changes.agents_stopped.len()
        + changes.record_added.len()
}

fn transcript_mtime(path: &str) -> Option<SystemTime> {
    if path.is_empty() {
        return None;
    }
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// `running` agents whose transcript `mtime` is older than `stale_after`.
/// A transcript with no readable mtime counts as stale; one dated after
/// `wall_now` (clock skew) counts as fresh.
pub(crate) fn stale_agents(
    snapshot: &Snapshot,
    stale_after: Duration,
    wall_now: SystemTime,
    mtime: impl Fn(&str) -> Option<SystemTime>,
) -> HashSet<String> {
    snapshot
        .agents
        .iter()
        .filter(|agent| agent.status == AgentStatus::Running)
        .filter(|agent| match mtime(&agent.transcript_path) {
            None => true,
            Some(at) => wall_now
                .duration_since(at)
                .is_ok_and(|age| age > stale_after),
        })
        .map(|agent| agent.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::fixture;

    /// Steps through ids by one, as a stand-in for a view's layout.
    struct Step;

    impl Navigator for Step {
        fn neighbor(&self, from: u32, dir: Dir) -> Option<u32> {
            match dir {
                Dir::Down | Dir::Right => (from < 8).then_some(from + 1),
                Dir::Up | Dir::Left => (from > 1).then_some(from - 1),
            }
        }
    }

    fn app() -> App {
        App::new(fixture(), &Config::default())
    }

    fn flow(slug: &str) -> FlowEntry {
        FlowEntry {
            slug: slug.to_string(),
            status: "in-progress".to_string(),
            updated: String::new(),
            plan_path: String::new(),
            tasks_mtime: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn a_new_app_selects_the_frontier() {
        let app = app();
        assert!(app.follow);
        assert_eq!(app.selected, Some(4), "task 4 is the only in-progress row");
        assert_eq!(app.view, ViewKind::Layers);
    }

    #[test]
    fn follow_reselects_the_frontier_on_each_snapshot() {
        let mut app = app();
        let mut next = fixture();
        next.tasks[3].status = TaskStatus::Done;
        app.apply_snapshot(next.clone(), Instant::now());
        assert_eq!(
            app.selected,
            Some(6),
            "no row in progress, so the first ready"
        );

        next.tasks[4].status = TaskStatus::InProgress;
        next.tasks[6].status = TaskStatus::InProgress;
        app.apply_snapshot(next, Instant::now());
        assert_eq!(app.selected, Some(5), "the lower layer wins over task 7");
    }

    #[test]
    fn a_manual_move_pauses_follow() {
        let mut app = app();
        app.nav = Some(Box::new(Step));
        assert_eq!(app.apply(Action::Move(Dir::Down)), None);
        assert_eq!(app.selected, Some(5));
        assert!(!app.follow);

        let mut next = fixture();
        next.tasks[5].status = TaskStatus::InProgress;
        app.apply_snapshot(next, Instant::now());
        assert_eq!(app.selected, Some(5), "a paused selection stays put");
        assert_eq!(app.pending_changes, 1);

        app.apply(Action::ToggleFollow);
        assert_eq!(app.selected, Some(4));
        assert_eq!(app.pending_changes, 0);
    }

    #[test]
    fn a_move_with_no_neighbour_keeps_follow() {
        let mut app = app();
        app.selected = Some(8);
        app.nav = Some(Box::new(Step));
        app.apply(Action::Move(Dir::Down));
        assert_eq!(app.selected, Some(8));
        assert!(app.follow);
    }

    #[test]
    fn a_status_change_flashes_until_it_expires() {
        let mut app = app();
        let t0 = Instant::now();
        let mut next = fixture();
        next.tasks[4].status = TaskStatus::InProgress;
        app.apply_snapshot(next, t0);

        assert!(app.is_flashing(5, t0 + Duration::from_millis(1000)));
        assert!(
            !app.is_flashing(4, t0),
            "an unchanged status does not flash"
        );
        assert!(!app.is_flashing(5, t0 + Duration::from_millis(1600)));

        app.tick(t0 + Duration::from_millis(1600));
        assert!(app.flashes.is_empty());
    }

    #[test]
    fn needs_tick_tracks_flashes_and_live_agents() {
        let mut app = app();
        let t0 = Instant::now();
        app.stale_agents.insert("A2".to_string());
        assert!(!app.needs_tick(t0));

        app.flashes.insert(4, t0);
        assert!(app.needs_tick(t0 + Duration::from_millis(1400)));
        assert!(!app.needs_tick(t0 + Duration::from_millis(1500)));

        app.flashes.clear();
        app.stale_agents.clear();
        assert!(app.needs_tick(t0), "A2 is running and not stale");
    }

    #[test]
    fn a_stale_running_agent_keeps_no_tick_alive() {
        let app = app();
        assert!(
            app.stale_agents.contains("A2"),
            "a missing transcript counts as stale"
        );
        assert!(!app.needs_tick(Instant::now()));
    }

    #[test]
    fn staleness_follows_transcript_age() {
        let snap = fixture();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let after = Duration::from_secs(300);

        let fresh = stale_agents(&snap, after, now, |_| Some(now - Duration::from_secs(10)));
        assert!(fresh.is_empty());

        let old = stale_agents(&snap, after, now, |_| Some(now - Duration::from_secs(301)));
        assert_eq!(
            old,
            HashSet::from(["A2".to_string()]),
            "idle A1 is never stale"
        );

        let future = stale_agents(&snap, after, now, |_| Some(now + Duration::from_secs(5)));
        assert!(future.is_empty());
    }

    #[test]
    fn a_live_transcript_on_disk_keeps_the_agent_fresh() {
        let dir = std::env::temp_dir().join(format!("glimpse-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("agent.jsonl");
        std::fs::write(&path, "{}\n").expect("write");

        let mut snap = fixture();
        snap.agents[1].transcript_path = path.to_string_lossy().into_owned();
        let app = App::new(snap, &Config::default());
        assert!(app.stale_agents.is_empty());
        assert!(app.needs_tick(Instant::now()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tab_cycles_views_and_drops_the_navigator() {
        let mut app = app();
        app.nav = Some(Box::new(Step));
        app.apply(Action::NextView);
        assert_eq!(app.view, ViewKind::Ego);
        assert!(app.nav.is_none());
        app.apply(Action::NextView);
        app.apply(Action::NextView);
        assert_eq!(app.view, ViewKind::Layers);
        app.apply(Action::PrevView);
        assert_eq!(app.view, ViewKind::Diagram);
    }

    #[test]
    fn orientation_flips_from_the_last_resolved_one() {
        let mut app = app();
        app.resolved_orientation = Orientation::Horizontal;
        app.apply(Action::FlipOrientation);
        assert_eq!(app.orientation_override, Some(Orientation::Vertical));
        app.apply(Action::FlipOrientation);
        assert_eq!(app.orientation_override, Some(Orientation::Horizontal));
    }

    #[test]
    fn details_open_then_fill_then_close() {
        let mut app = app();
        app.apply(Action::Details);
        assert!(app.details_open && !app.details_fullscreen);
        app.apply(Action::Details);
        assert!(app.details_open && app.details_fullscreen);
        app.apply(Action::Details);
        assert!(!app.details_open && !app.details_fullscreen);
    }

    #[test]
    fn back_closes_overlays_before_quitting() {
        let mut app = app();
        app.selector_open = true;
        app.details_open = true;
        app.activity_open = true;
        assert_eq!(app.apply(Action::Back), None);
        assert!(!app.selector_open && app.details_open);
        assert_eq!(app.apply(Action::Back), None);
        assert!(!app.details_open);
        assert_eq!(app.apply(Action::Back), None);
        assert!(!app.activity_open);
        assert_eq!(app.apply(Action::Back), Some(Action::Quit));
    }

    #[test]
    fn the_selector_switches_to_the_flow_under_its_cursor() {
        let mut app = app();
        app.auto_flow = true;
        app.flows = vec![flow("other"), flow("demo-flow"), flow("third")];
        app.apply(Action::ToggleSelector);
        assert_eq!(
            app.selector_cursor, 1,
            "the cursor starts on the current flow"
        );

        app.apply(Action::Move(Dir::Up));
        app.apply(Action::Move(Dir::Up));
        assert_eq!(app.selector_cursor, 0, "the cursor clamps at the top");
        assert_eq!(
            app.selected,
            Some(4),
            "moves go to the cursor, not the graph"
        );

        assert_eq!(
            app.apply(Action::Details),
            Some(Action::SwitchFlow("other".to_string()))
        );
        assert!(!app.selector_open);
        assert!(!app.auto_flow, "a hand-picked flow turns auto-flow off");
        assert_eq!(app.apply(Action::SwitchFlow("demo-flow".to_string())), None);
    }

    #[test]
    fn a_snapshot_of_another_flow_does_not_flash() {
        let mut app = app();
        app.follow = false;
        app.selected = Some(8);
        let mut other = fixture();
        other.slug = "other".to_string();
        other.tasks[4].status = TaskStatus::Done;
        app.apply_snapshot(other, Instant::now());
        assert!(app.flashes.is_empty());
        assert_eq!(app.pending_changes, 0);
        assert_eq!(app.selected, Some(4), "a new flow starts at its frontier");
    }
}
