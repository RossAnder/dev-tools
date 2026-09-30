//! Application state and the actions that change it.
//!
//! `App` owns the current snapshot and everything the views read besides the
//! config: selection, overlays, flashes, staleness, the runtime layout tweaks
//! and the regions the last frame drew for mouse hits. It never draws and
//! never spawns; the runtime feeds it actions and snapshots and carries out
//! the few actions `apply` hands back.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime};

use ratatui::layout::Rect;

use crate::config::{
    COLUMN_RANGE, Config, Density, DensityPref, Orientation, OrientationPref, PANEL_PERCENT_RANGE,
    Split, ViewKind,
};
use crate::diff::{Changes, diff};
use crate::flows::FlowEntry;
use crate::model::{AgentStatus, Index, Snapshot, TaskStatus};
use crate::theme::Theme;

/// How long a task stays highlighted after its status changes.
pub(crate) const FLASH: Duration = Duration::from_millis(1500);
/// The wake-up interval while something on screen changes with time alone.
pub(crate) const TICK: Duration = Duration::from_secs(1);
/// The wake-up interval while every running agent is stale. Nothing announces an
/// agent resuming, so only a re-read of transcript ages can clear its stale mark.
pub(crate) const STALE_RECHECK: Duration = Duration::from_secs(30);
/// How long a footer notice, such as the density a `d` press resolved to, stays up.
pub(crate) const NOTICE: Duration = Duration::from_millis(1500);
/// Step for `[` and `]`, in percent of the body.
pub(crate) const PANEL_STEP: u16 = 5;
/// Step for `-` and `=`, in cells.
pub(crate) const COLUMN_STEP: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// A side of the traversal view: what the centre waits on, or what waits on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Needs,
    Dependents,
}

/// One move across in the traversal view: the centre it left and the side it entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Crossing {
    pub(crate) origin: u32,
    pub(crate) side: Side,
}

/// Spatial movement over whatever the active view draws. The view installs a
/// fresh one each frame, since only it knows where each task sits.
pub(crate) trait Navigator {
    fn neighbor(&self, from: u32, dir: Dir) -> Option<u32>;

    /// The move from `from` and the trail it leaves. A view that keeps no trail
    /// takes this default, so moving in it drops the trail.
    fn walk(&self, from: u32, dir: Dir) -> Option<(u32, Vec<Crossing>)> {
        self.neighbor(from, dir).map(|to| (to, Vec::new()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scroll {
    Up(u16),
    Down(u16),
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// Where the last frame put each thing the mouse can hit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Regions {
    pub(crate) view: Option<Rect>,
    pub(crate) details: Option<Rect>,
    /// The compact-density modal; while it is up the view underneath takes no clicks.
    pub(crate) modal: Option<Rect>,
    /// One rect per clickable task. The layers and traversal views fill this.
    pub(crate) tasks: Vec<(Rect, u32)>,
    /// The body the docked panel shares with the view; a drag measures against it.
    pub(crate) body: Rect,
    /// The strip between the view and a docked panel that a drag resizes from.
    pub(crate) divider: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    /// A selection move, or a cursor move while the selector is open.
    Move(Dir),
    NextView,
    PrevView,
    FlipOrientation,
    /// Moves docked panels between beside and below the view.
    FlipSplit,
    /// Lays the diagram out with or without the needs edges other paths imply.
    ToggleImplied,
    ToggleLegend,
    /// A press on the divider; the following drags resize the docked panel.
    DragStart,
    /// The pointer's column and row while a drag is on.
    DragTo(u16, u16),
    DragEnd,
    ToggleFollow,
    ToggleAutoFlow,
    ToggleActivity,
    ToggleSelector,
    SelectorNext,
    SelectorPrev,
    /// Opens details, then makes them full-screen, then closes them; compact
    /// density skips the full-screen step, since its modal already fills the body.
    /// With the selector open it switches to the flow under the cursor instead.
    Details,
    ScrollDetails(Scroll),
    /// Selects a task directly, as a click does; this turns follow off.
    Select(u32),
    CycleDensity,
    /// Grows (`true`) or shrinks the docked panel by [`PANEL_STEP`].
    ResizePanel(bool),
    /// Widens (`true`) or narrows horizontal layer columns by [`COLUMN_STEP`].
    ResizeColumns(bool),
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
    pub(crate) split_override: Option<Split>,
    /// Where the last frame docked panels, `None` while none were docked; `auto`
    /// keeps it inside the dead band and `|` flips from it.
    pub(crate) resolved_split: Option<Split>,
    /// A cell's height over its width: measured from the terminal when it reports
    /// pixels, else the config's `cell_aspect`.
    pub(crate) cell_aspect: f64,
    /// Whether the diagram lays out the needs edges other paths already imply.
    pub(crate) show_implied: bool,
    /// Needs edges `(from, to)` another path of needs edges implies.
    pub(crate) implied: HashSet<(u32, u32)>,
    pub(crate) legend_open: bool,
    /// A drag on the divider is under way.
    pub(crate) dragging: bool,
    pub(crate) selected: Option<u32>,
    /// The crossings the traversal view made to reach the selection, oldest first.
    /// Any selection change a navigator's `walk` did not make empties it.
    pub(crate) trail: Vec<Crossing>,
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
    /// Per fresh running agent, the wall time it can first go stale: its
    /// transcript's last observed mtime plus `stale_after`.
    pub(crate) stale_deadlines: HashMap<String, SystemTime>,
    pub(crate) stale_after: Duration,
    pub(crate) theme: Theme,
    pub(crate) nav: Option<Box<dyn Navigator>>,
    pub(crate) density: DensityPref,
    /// The density the last frame drew in; `Details` and `Back` read it.
    pub(crate) resolved_density: Density,
    pub(crate) panel_percent: u16,
    pub(crate) column_max: u16,
    /// Set by `-` and `=`; `None` sizes columns from the titles.
    pub(crate) column_override: Option<u16>,
    /// The column width the last horizontal layers frame chose; `-` and `=` step from it.
    pub(crate) resolved_column: u16,
    /// Rows the details panel is scrolled down; back to 0 whenever the selection changes.
    pub(crate) details_scroll: u16,
    /// The furthest `details_scroll` can go and the panel's page height, from the last frame.
    pub(crate) details_max_scroll: u16,
    pub(crate) details_page: u16,
    pub(crate) regions: Regions,
    /// A short message the footer shows until [`NOTICE`] has passed.
    pub(crate) notice: Option<(String, Instant)>,
}

impl App {
    pub(crate) fn new(snapshot: Snapshot, config: &Config) -> App {
        let resolved_orientation = match config.orientation {
            OrientationPref::Fixed(o) => o,
            OrientationPref::Auto => Orientation::Vertical,
        };
        let mut app = App {
            index: snapshot.index(),
            implied: implied_set(&snapshot),
            snapshot,
            view: config.default_view,
            orientation_override: None,
            resolved_orientation,
            split_override: None,
            resolved_split: None,
            cell_aspect: config.cell_aspect,
            show_implied: false,
            legend_open: false,
            dragging: false,
            selected: None,
            trail: Vec::new(),
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
            stale_deadlines: HashMap::new(),
            stale_after: Duration::from_secs(config.stale_after_s),
            theme: Theme::build(&config.theme, config.no_color),
            nav: None,
            density: config.density,
            resolved_density: config.density.resolve(u16::MAX, config.compact_below),
            panel_percent: config.panel_percent,
            column_max: config.column_max,
            column_override: None,
            resolved_column: config.column_max,
            details_scroll: 0,
            details_max_scroll: 0,
            details_page: 1,
            regions: Regions::default(),
            notice: None,
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
            Action::FlipSplit => {
                let current = self
                    .split_override
                    .or(self.resolved_split)
                    .unwrap_or(Split::Below);
                let next = current.flip();
                self.split_override = Some(next);
                self.notify(format!("panels {}", next.as_str()));
                None
            }
            Action::ToggleImplied => {
                self.show_implied = !self.show_implied;
                self.nav = None;
                let state = if self.show_implied { "shown" } else { "hidden" };
                self.notify(format!("implied edges {state}"));
                None
            }
            Action::ToggleLegend => {
                self.legend_open = !self.legend_open;
                None
            }
            Action::DragStart => {
                self.dragging = self.regions.divider.is_some();
                None
            }
            Action::DragTo(column, row) => {
                if self.dragging {
                    self.drag_to(column, row);
                }
                None
            }
            Action::DragEnd => {
                self.dragging = false;
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
                let compact = self.resolved_density == Density::Compact;
                if !self.details_open {
                    self.details_open = true;
                    self.details_fullscreen = false;
                } else if !self.details_fullscreen && !compact {
                    self.details_fullscreen = true;
                } else {
                    self.details_open = false;
                    self.details_fullscreen = false;
                }
                None
            }
            Action::ScrollDetails(scroll) => {
                if self.details_open {
                    self.scroll_details(scroll);
                }
                None
            }
            Action::Select(id) => {
                if self.selected != Some(id) && self.index.task(&self.snapshot, id).is_some() {
                    self.select(Some(id));
                    self.follow = false;
                }
                None
            }
            Action::CycleDensity => {
                self.density = self.density.next();
                let label = match self.density {
                    DensityPref::Auto => "auto".to_string(),
                    DensityPref::Fixed(d) => d.as_str().to_string(),
                };
                self.notify(format!("density {label}"));
                None
            }
            Action::ResizePanel(grow) => {
                let percent = if grow {
                    self.panel_percent.saturating_add(PANEL_STEP)
                } else {
                    self.panel_percent.saturating_sub(PANEL_STEP)
                };
                self.panel_percent =
                    percent.clamp(*PANEL_PERCENT_RANGE.start(), *PANEL_PERCENT_RANGE.end());
                self.notify(format!("panel {}%", self.panel_percent));
                None
            }
            Action::ResizeColumns(wider) => {
                let from = self.column_override.unwrap_or(self.resolved_column);
                let width = if wider {
                    from.saturating_add(COLUMN_STEP)
                } else {
                    from.saturating_sub(COLUMN_STEP)
                };
                let width = width.clamp(*COLUMN_RANGE.start(), *COLUMN_RANGE.end());
                self.column_override = Some(width);
                self.notify(format!("columns {width} wide"));
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
                if self.legend_open {
                    self.legend_open = false;
                } else if self.selector_open {
                    self.selector_open = false;
                } else if self.details_fullscreen && self.resolved_density == Density::Comfortable {
                    self.details_fullscreen = false;
                } else if self.details_open {
                    self.details_open = false;
                    self.details_fullscreen = false;
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
        let old_topology = self.index.topology_hash();
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
            self.select(None);
        }
        self.index = snapshot.index();
        if self.index.topology_hash() != old_topology {
            self.implied = implied_set(&snapshot);
        }
        self.snapshot = snapshot;
        self.source_error = None;
        self.expire_flashes(now);
        self.refresh_stale();
        self.reselect();
    }

    /// Called on each timed wake-up: drops expired flashes and re-reads the
    /// transcript ages that are due, since an agent goes stale without any snapshot.
    pub(crate) fn tick(&mut self, now: Instant) {
        self.expire_flashes(now);
        if self.live_notice(now).is_none() {
            self.notice = None;
        }
        self.recheck_stale(SystemTime::now(), transcript_mtime);
    }

    /// True while something on screen changes with time alone: a live flash or
    /// notice, or a running agent whose elapsed counter is still advancing.
    pub(crate) fn needs_tick(&self, now: Instant) -> bool {
        self.flashes
            .values()
            .any(|at| now.saturating_duration_since(*at) < FLASH)
            || self.live_notice(now).is_some()
            || self.snapshot.agents.iter().any(|agent| {
                agent.status == AgentStatus::Running && !self.stale_agents.contains(&agent.id)
            })
    }

    /// How long the runtime may wait before the next [`App::tick`]: [`TICK`] while
    /// [`App::needs_tick`], [`STALE_RECHECK`] while any agent is running, and `None`
    /// when nothing can change without an event.
    pub(crate) fn tick_interval(&self, now: Instant) -> Option<Duration> {
        if self.needs_tick(now) {
            Some(TICK)
        } else if self
            .snapshot
            .agents
            .iter()
            .any(|agent| agent.status == AgentStatus::Running)
        {
            Some(STALE_RECHECK)
        } else {
            None
        }
    }

    pub(crate) fn is_flashing(&self, id: u32, now: Instant) -> bool {
        self.flashes
            .get(&id)
            .is_some_and(|at| now.saturating_duration_since(*at) < FLASH)
    }

    /// The footer notice, while it is younger than [`NOTICE`].
    pub(crate) fn live_notice(&self, now: Instant) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(_, at)| now.saturating_duration_since(*at) < NOTICE)
            .map(|(text, _)| text.as_str())
    }

    fn notify(&mut self, text: String) {
        self.notice = Some((text, Instant::now()));
    }

    /// Sets the docked panel's share so its edge follows the pointer, measured against
    /// the body and split the last frame drew.
    fn drag_to(&mut self, column: u16, row: u16) {
        let body = self.regions.body;
        let (edge, span) = match self.resolved_split {
            Some(Split::Beside) => (body.right().saturating_sub(column), body.width),
            Some(Split::Below) => (body.bottom().saturating_sub(row), body.height),
            None => return,
        };
        if span == 0 {
            return;
        }
        let percent = u32::from(edge) * 100 / u32::from(span);
        let percent = u16::try_from(percent).unwrap_or(u16::MAX);
        self.panel_percent =
            percent.clamp(*PANEL_PERCENT_RANGE.start(), *PANEL_PERCENT_RANGE.end());
    }

    /// Every selection change goes through here so the details scroll starts over
    /// and the traversal trail, which no longer leads here, is dropped.
    fn select(&mut self, id: Option<u32>) {
        if id != self.selected {
            self.details_scroll = 0;
            self.trail.clear();
        }
        self.selected = id;
    }

    /// Clamped against the bounds the last frame recorded; a page keeps one row of overlap.
    fn scroll_details(&mut self, scroll: Scroll) {
        let page = self.details_page.saturating_sub(1).max(1);
        let at = self.details_scroll;
        let to = match scroll {
            Scroll::Up(n) => at.saturating_sub(n),
            Scroll::Down(n) => at.saturating_add(n),
            Scroll::PageUp => at.saturating_sub(page),
            Scroll::PageDown => at.saturating_add(page),
            Scroll::Top => 0,
            Scroll::Bottom => u16::MAX,
        };
        self.details_scroll = to.min(self.details_max_scroll);
    }

    fn expire_flashes(&mut self, now: Instant) {
        self.flashes
            .retain(|_, at| now.saturating_duration_since(*at) < FLASH);
    }

    /// Re-stats every running agent's transcript.
    fn refresh_stale(&mut self) {
        self.refresh_stale_with(SystemTime::now(), transcript_mtime);
    }

    fn refresh_stale_with(
        &mut self,
        wall_now: SystemTime,
        mtime: impl Fn(&str) -> Option<SystemTime>,
    ) {
        self.stale_deadlines.clear();
        self.recheck_stale(wall_now, mtime);
    }

    /// Re-stats only the agents already stale or past their deadline in
    /// [`App::stale_deadlines`]; the rest cannot have gone stale yet.
    fn recheck_stale(&mut self, wall_now: SystemTime, mtime: impl Fn(&str) -> Option<SystemTime>) {
        self.stale_agents = stale_agents(
            &self.snapshot,
            self.stale_after,
            wall_now,
            &self.stale_agents,
            &mut self.stale_deadlines,
            mtime,
        );
    }

    fn move_selection(&mut self, dir: Dir) {
        let Some(from) = self.selected else {
            let first = self.frontier().or_else(|| self.first_task());
            self.select(first);
            return;
        };
        let Some((to, trail)) = self.nav.as_ref().and_then(|nav| nav.walk(from, dir)) else {
            return;
        };
        if to != from {
            self.select(Some(to));
            self.trail = trail;
            self.follow = false;
        }
    }

    /// With follow on the selection jumps to the frontier. Otherwise, and
    /// when the frontier is empty, a selection that still names a task stays.
    fn reselect(&mut self) {
        if self.follow
            && let Some(id) = self.frontier()
        {
            self.select(Some(id));
            return;
        }
        let still_present = self
            .selected
            .is_some_and(|id| self.index.task(&self.snapshot, id).is_some());
        if !still_present {
            let first = self.frontier().or_else(|| self.first_task());
            self.select(first);
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

fn implied_set(snapshot: &Snapshot) -> HashSet<(u32, u32)> {
    crate::diagram::reduce::implied_edges(snapshot)
        .into_iter()
        .collect()
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
/// `wall_now` (clock skew) counts as fresh. An agent not in `previous` whose
/// entry in `deadlines` is still ahead of `wall_now` is kept fresh without a
/// stat; `deadlines` is rewritten to hold only the running, fresh agents.
pub(crate) fn stale_agents(
    snapshot: &Snapshot,
    stale_after: Duration,
    wall_now: SystemTime,
    previous: &HashSet<String>,
    deadlines: &mut HashMap<String, SystemTime>,
    mtime: impl Fn(&str) -> Option<SystemTime>,
) -> HashSet<String> {
    let mut stale = HashSet::new();
    let mut next = HashMap::new();
    for agent in snapshot
        .agents
        .iter()
        .filter(|agent| agent.status == AgentStatus::Running)
    {
        if !previous.contains(&agent.id)
            && let Some(&deadline) = deadlines.get(&agent.id)
            && wall_now <= deadline
        {
            next.insert(agent.id.clone(), deadline);
            continue;
        }
        match mtime(&agent.transcript_path).map(|at| at.checked_add(stale_after)) {
            None => {
                stale.insert(agent.id.clone());
            }
            Some(None) => {}
            Some(Some(deadline)) if wall_now > deadline => {
                stale.insert(agent.id.clone());
            }
            Some(Some(deadline)) => {
                next.insert(agent.id.clone(), deadline);
            }
        }
    }
    *deadlines = next;
    stale
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

    /// Crosses into the needs side on every move, lengthening the trail by one.
    struct Crosser;

    impl Navigator for Crosser {
        fn neighbor(&self, from: u32, _: Dir) -> Option<u32> {
            Some(from + 1)
        }

        fn walk(&self, from: u32, dir: Dir) -> Option<(u32, Vec<Crossing>)> {
            let crossing = Crossing {
                origin: from,
                side: Side::Needs,
            };
            self.neighbor(from, dir).map(|to| (to, vec![crossing]))
        }
    }

    #[test]
    fn a_walk_keeps_its_trail_and_any_other_selection_drops_it() {
        let mut app = app();
        app.nav = Some(Box::new(Crosser));
        app.apply(Action::Move(Dir::Left));
        assert_eq!(app.selected, Some(5));
        assert_eq!(app.trail.len(), 1);

        app.apply(Action::Select(2));
        assert!(app.trail.is_empty(), "a click is not part of the trail");

        app.apply(Action::Move(Dir::Left));
        app.nav = Some(Box::new(Step));
        app.apply(Action::Move(Dir::Right));
        assert!(app.trail.is_empty(), "a view without a trail drops it");
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
    fn a_stale_running_agent_is_rechecked_until_it_resumes() {
        let dir = std::env::temp_dir().join(format!("glimpse-recheck-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("agent.jsonl");
        let _ = std::fs::remove_file(&path);

        let mut snap = fixture();
        snap.agents[1].transcript_path = path.to_string_lossy().into_owned();
        let mut app = App::new(snap, &Config::default());
        let t0 = Instant::now();
        assert!(app.stale_agents.contains("A2"), "no transcript yet");
        assert_eq!(app.tick_interval(t0), Some(STALE_RECHECK));

        std::fs::write(&path, "{}\n").expect("write");
        app.tick(t0 + STALE_RECHECK);
        assert!(app.stale_agents.is_empty(), "the recheck sees it resume");
        assert_eq!(app.tick_interval(t0 + STALE_RECHECK), Some(TICK));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_running_agent_nothing_wakes_the_loop() {
        let mut snap = fixture();
        snap.agents
            .retain(|agent| agent.status != AgentStatus::Running);
        let app = App::new(snap, &Config::default());
        assert_eq!(app.tick_interval(Instant::now()), None);
    }

    #[test]
    fn staleness_follows_transcript_age() {
        let snap = fixture();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let after = Duration::from_secs(300);

        let check = |at: SystemTime| {
            stale_agents(
                &snap,
                after,
                now,
                &HashSet::new(),
                &mut HashMap::new(),
                |_| Some(at),
            )
        };

        assert!(check(now - Duration::from_secs(10)).is_empty());
        assert_eq!(
            check(now - Duration::from_secs(301)),
            HashSet::from(["A2".to_string()]),
            "idle A1 is never stale"
        );
        assert!(check(now + Duration::from_secs(5)).is_empty());
    }

    /// Runs one tick-path recheck at `wall_now` against a transcript last
    /// written at `at`, returning how many times the transcript was stat'd.
    fn recheck_at(app: &mut App, wall_now: SystemTime, at: SystemTime) -> usize {
        let calls = std::cell::Cell::new(0);
        app.recheck_stale(wall_now, |_| {
            calls.set(calls.get() + 1);
            Some(at)
        });
        calls.get()
    }

    #[test]
    fn a_fresh_agent_is_not_restatted_before_its_deadline() {
        let mut app = app();
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let after = app.stale_after;
        app.refresh_stale_with(t0, |_| Some(t0));
        assert!(app.stale_agents.is_empty());
        assert_eq!(app.stale_deadlines.get("A2"), Some(&(t0 + after)));

        assert_eq!(recheck_at(&mut app, t0 + after, t0), 0, "not yet due");
        assert!(app.stale_agents.is_empty());

        let past = t0 + after + Duration::from_secs(1);
        assert_eq!(recheck_at(&mut app, past, t0), 1, "due, so stat'd");
        assert!(app.stale_agents.contains("A2"));
        assert!(app.stale_deadlines.is_empty());

        assert_eq!(
            recheck_at(&mut app, past, t0),
            1,
            "a stale agent is stat'd on every recheck"
        );
    }

    #[test]
    fn a_grown_transcript_pushes_the_deadline_back() {
        let mut app = app();
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let after = app.stale_after;
        app.refresh_stale_with(t0, |_| Some(t0));

        let grown = t0 + Duration::from_secs(60);
        let due = t0 + after + Duration::from_secs(1);
        assert_eq!(recheck_at(&mut app, due, grown), 1);
        assert!(app.stale_agents.is_empty(), "the transcript grew");
        assert_eq!(app.stale_deadlines.get("A2"), Some(&(grown + after)));
        assert_eq!(recheck_at(&mut app, grown + after, grown), 0);
    }

    #[test]
    fn a_full_refresh_restats_every_agent_and_drops_gone_ones() {
        let mut app = app();
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        app.refresh_stale_with(t0, |_| Some(t0));
        app.stale_deadlines
            .insert("gone".to_string(), t0 + app.stale_after);

        let calls = std::cell::Cell::new(0);
        app.refresh_stale_with(t0, |_| {
            calls.set(calls.get() + 1);
            Some(t0)
        });
        assert_eq!(
            calls.get(),
            1,
            "A2 is stat'd although its deadline is ahead"
        );
        assert!(!app.stale_deadlines.contains_key("gone"));
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
    fn compact_details_open_and_close_without_a_full_screen_step() {
        let mut app = app();
        app.resolved_density = Density::Compact;
        app.apply(Action::Details);
        assert!(app.details_open && !app.details_fullscreen);
        app.apply(Action::Details);
        assert!(!app.details_open, "the second Enter closes the modal");

        app.details_open = true;
        app.details_fullscreen = true;
        app.apply(Action::Back);
        assert!(
            !app.details_open && !app.details_fullscreen,
            "one Back closes a modal left full-screen by comfortable density"
        );
    }

    #[test]
    fn density_cycles_and_announces_itself() {
        let mut app = app();
        assert_eq!(app.density, DensityPref::Auto);
        app.apply(Action::CycleDensity);
        assert_eq!(app.density, DensityPref::Fixed(Density::Compact));
        let now = Instant::now();
        assert_eq!(app.live_notice(now), Some("density compact"));
        assert!(app.needs_tick(now), "a live notice keeps the clock running");
        app.tick(now + NOTICE);
        assert_eq!(app.notice, None);
    }

    #[test]
    fn the_panel_resizes_in_steps_within_its_range() {
        let mut app = app();
        assert_eq!(app.panel_percent, 40);
        app.apply(Action::ResizePanel(true));
        assert_eq!(app.panel_percent, 45);
        for _ in 0..10 {
            app.apply(Action::ResizePanel(true));
        }
        assert_eq!(app.panel_percent, 70);
        for _ in 0..20 {
            app.apply(Action::ResizePanel(false));
        }
        assert_eq!(app.panel_percent, 20);
    }

    #[test]
    fn columns_resize_from_the_last_resolved_width() {
        let mut app = app();
        app.resolved_column = 30;
        app.apply(Action::ResizeColumns(false));
        assert_eq!(app.column_override, Some(26));
        app.apply(Action::ResizeColumns(true));
        app.apply(Action::ResizeColumns(true));
        assert_eq!(app.column_override, Some(34));
        for _ in 0..10 {
            app.apply(Action::ResizeColumns(false));
        }
        assert_eq!(app.column_override, Some(*COLUMN_RANGE.start()));
    }

    #[test]
    fn details_scroll_clamps_and_resets_on_a_new_selection() {
        let mut app = app();
        app.nav = Some(Box::new(Step));
        app.details_max_scroll = 12;
        app.details_page = 6;
        app.apply(Action::ScrollDetails(Scroll::Down(3)));
        assert_eq!(app.details_scroll, 0, "closed details do not scroll");

        app.apply(Action::Details);
        app.apply(Action::ScrollDetails(Scroll::Down(3)));
        assert_eq!(app.details_scroll, 3);
        app.apply(Action::ScrollDetails(Scroll::PageDown));
        assert_eq!(app.details_scroll, 8, "a page keeps one row of overlap");
        app.apply(Action::ScrollDetails(Scroll::Bottom));
        assert_eq!(app.details_scroll, 12);
        app.apply(Action::ScrollDetails(Scroll::Down(1)));
        assert_eq!(app.details_scroll, 12, "clamped to the content");
        app.apply(Action::ScrollDetails(Scroll::PageUp));
        assert_eq!(app.details_scroll, 7);

        app.apply(Action::Move(Dir::Down));
        assert_eq!(app.details_scroll, 0, "a new selection starts at the top");
        app.details_scroll = 5;
        app.apply(Action::Select(1));
        assert_eq!((app.selected, app.details_scroll), (Some(1), 0));
        assert!(!app.follow);
        app.details_scroll = 5;
        app.apply(Action::Select(99));
        assert_eq!(app.selected, Some(1), "an unknown id is not selected");
        assert_eq!(app.details_scroll, 5);
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
