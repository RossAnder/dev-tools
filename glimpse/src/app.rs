//! Application state and the actions that change it.
//!
//! `App` owns the current snapshot and everything the views read besides the
//! config: selection, overlays, flashes, staleness, the runtime layout tweaks
//! and the regions the last frame drew for mouse hits. It never draws and
//! never spawns; the runtime feeds it actions and snapshots and carries out
//! the few actions `apply` hands back.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime};

use ratatui::crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use tomlctl::LedgerKind;
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use crate::actions::{
    self, InputForm, Overlay, Plan, Purpose, Selection, Transition, UndoEntry, UndoKind,
};
use crate::config::{
    COLUMN_RANGE, Config, Density, DensityPref, Orientation, OrientationPref, PANEL_PERCENT_RANGE,
    Split, ViewKind,
};
use crate::diff::{Changes, diff};
use crate::flows::{FlowEntry, ScopeEntry, Scopes};
use crate::form::{FieldValue, Form, FormOutcome};
use crate::ledger::{InputRow, Inputs, ItemRow, Ledger};
use crate::model::{AgentStatus, Index, Snapshot, TaskStatus};
use crate::surface::{InboxState, ItemsState, Surface};
use crate::theme::Theme;
use crate::writer::{RequestId, WriteOutcome, WriteRequest};

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

/// Where the layers view was scrolled to in the last frame it drew.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ListScroll {
    /// The first row drawn vertically, or the first layer drawn horizontally.
    pub(crate) offset: usize,
    /// Horizontally, the first row drawn in the selected layer's column.
    pub(crate) rows: usize,
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
    /// One rect per item row the mouse can hit, keyed by item id.
    pub(crate) items: Vec<(Rect, String)>,
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
    /// With the selector open it switches to the flow under the cursor instead, and on an
    /// Inbox question it opens the answer form.
    Details,
    ScrollDetails(Scroll),
    /// Scrolls the view by `(columns, rows)` without touching the selection, which pins
    /// it there until the selection changes.
    ScrollView(i32, i32),
    /// Selects a task directly, as a click does; this turns follow off.
    Select(u32),
    CycleDensity,
    /// Grows (`true`) or shrinks the docked panel by [`PANEL_STEP`].
    ResizePanel(bool),
    /// Widens (`true`) or narrows horizontal layer columns by [`COLUMN_STEP`].
    ResizeColumns(bool),
    SwitchFlow(String),
    /// Shows a flow-less ledger on the item surfaces in place of the flow's.
    SwitchScope(LedgerKind, String),
    SwitchSurface(Surface),
    /// Moves the item cursor on the current item surface; `Move` becomes this off Tasks.
    ItemMove(Dir),
    /// Puts the item cursor on a row directly, as a click does.
    SelectItem(String),
    ToggleMark,
    MarkVisible,
    CycleGroup,
    CycleSort,
    /// Shows or hides the done and declined rows of the current item surface.
    ToggleClosed,
    /// Opens the status moves offered for the marks, else the cursor row.
    OpenMenu,
    /// Opens the severity, effort and category form; review and optimise only.
    OpenClassify,
    /// Opens the form that captures a new backlog item as an input record.
    OpenCapture,
    /// Opens a request or note on the marks, else the cursor row; item surfaces only.
    OpenRequest,
    /// Asks before withdrawing the Inbox cursor record, the user's own and still `new`.
    Withdraw,
    /// Puts back the rows glimpse's last applied write changed.
    Undo,
    OpenFilter,
    /// Closes an open form or prompt, else clears the current item surface's marks, else
    /// closes the topmost panel, or quits when none is open.
    Back,
    Quit,
}

/// One row the selector cursor can rest on, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectorEntry<'a> {
    Flow(&'a FlowEntry),
    /// A flow holding ledgers but no task store.
    LedgerOnly(&'a str),
    Scope(&'a ScopeEntry),
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
    /// The ledger-only flows and flow-less ledgers the selector lists after `flows`.
    pub(crate) scopes: Scopes,
    /// An index into [`App::selector_entries`].
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
    /// Set by the wheel over the view: the view stays where the wheel left it rather than
    /// following the selection. Any selection change, a new view or orientation, or
    /// turning follow on clears it.
    pub(crate) scroll_pinned: bool,
    /// Wheel scrolling the next frame applies, as `(columns, rows)`.
    pub(crate) scroll_nudge: (i32, i32),
    /// Written back by each layers frame.
    pub(crate) layers_scroll: ListScroll,
    pub(crate) regions: Regions,
    /// A short message the footer shows until [`NOTICE`] has passed.
    pub(crate) notice: Option<(String, Instant)>,
    pub(crate) surface: Surface,
    /// One state per item surface, present from the start and empty until its ledger is read.
    pub(crate) items: HashMap<Surface, ItemsState>,
    pub(crate) inbox: InboxState,
    /// The flow-less ledger the item surfaces show in place of the flow's, once picked
    /// in the selector.
    pub(crate) scope: Option<(LedgerKind, String)>,
    /// The ledger-only flow picked in the selector. No snapshot of it ever arrives, so
    /// `snapshot.slug` still names the flow before it.
    pub(crate) ledger_flow: Option<String>,
    /// The ledger file each item surface last read, so a read of another file starts
    /// the surface over rather than flashing every row as an arrival.
    ledger_paths: HashMap<Surface, String>,
    /// The menu, form or filter prompt on top; while set it takes every key.
    pub(crate) overlay: Option<Overlay>,
    /// One entry per submitted control edit or created input record, newest last.
    pub(crate) undo: Vec<UndoEntry>,
    /// Requests made and not yet taken by the runtime, in submission order.
    writes: Vec<WriteRequest>,
    in_flight: HashMap<RequestId, InFlight>,
    next_request: RequestId,
}

/// A submitted request awaiting its outcome: the rows it marked saving, and whether it is
/// an undo, which pushes no undo entry of its own.
#[derive(Debug, Clone)]
struct InFlight {
    surface: Surface,
    ids: Vec<String>,
    undo: bool,
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
            scopes: Scopes::default(),
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
            scroll_pinned: false,
            scroll_nudge: (0, 0),
            layers_scroll: ListScroll::default(),
            regions: Regions::default(),
            notice: None,
            surface: Surface::Tasks,
            items: Surface::ALL
                .into_iter()
                .filter(|surface| surface.ledger_kind().is_some())
                .map(|surface| (surface, ItemsState::new(surface)))
                .collect(),
            inbox: InboxState::default(),
            scope: None,
            ledger_flow: None,
            ledger_paths: HashMap::new(),
            overlay: None,
            undo: Vec::new(),
            writes: Vec::new(),
            in_flight: HashMap::new(),
            next_request: 0,
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
            Action::Move(dir) if self.surface != Surface::Tasks => {
                self.apply(Action::ItemMove(dir))
            }
            Action::Move(dir) => {
                self.move_selection(dir);
                None
            }
            Action::NextView => {
                self.view = self.view.next();
                self.nav = None;
                self.scroll_pinned = false;
                None
            }
            Action::PrevView => {
                self.view = self.view.prev();
                self.nav = None;
                self.scroll_pinned = false;
                None
            }
            Action::FlipOrientation => {
                let current = self
                    .orientation_override
                    .unwrap_or(self.resolved_orientation);
                self.orientation_override = Some(current.flip());
                self.nav = None;
                self.scroll_pinned = false;
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
                    self.scroll_pinned = false;
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
                        .selector_entries()
                        .iter()
                        .position(|entry| self.is_current(*entry))
                        .unwrap_or(0);
                }
                None
            }
            Action::SelectorNext => {
                let last = self.selector_entries().len().saturating_sub(1);
                self.selector_cursor = (self.selector_cursor + 1).min(last);
                None
            }
            Action::SelectorPrev => {
                let last = self.selector_entries().len().saturating_sub(1);
                self.selector_cursor = self.selector_cursor.saturating_sub(1).min(last);
                None
            }
            Action::Details if self.selector_open => {
                let picked = match self.selector_entries().get(self.selector_cursor) {
                    Some(SelectorEntry::Flow(flow)) => Action::SwitchFlow(flow.slug.clone()),
                    Some(SelectorEntry::LedgerOnly(slug)) => Action::SwitchFlow(slug.to_string()),
                    Some(SelectorEntry::Scope(entry)) => {
                        Action::SwitchScope(entry.kind, entry.scope.clone())
                    }
                    None => return None,
                };
                self.apply(picked)
            }
            Action::Details if self.inbox_question().is_some() => {
                if let Some(question) = self.inbox_question().cloned() {
                    self.open_input(InputForm::Answer(question));
                }
                None
            }
            Action::Details if self.surface != Surface::Tasks && !self.has_cursor_row() => None,
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
            Action::ScrollView(columns, rows) => {
                self.scroll_pinned = true;
                self.scroll_nudge.0 = self.scroll_nudge.0.saturating_add(columns);
                self.scroll_nudge.1 = self.scroll_nudge.1.saturating_add(rows);
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
                // Leaving a flow-less ledger for the current flow still needs the runtime,
                // which re-points the item feeds.
                let left_scope = self.scope.take().is_some();
                let left_ledger_flow = self.ledger_flow.take().is_some();
                if slug == self.snapshot.slug && !left_scope && !left_ledger_flow {
                    return None;
                }
                if !self.flows.iter().any(|flow| flow.slug == slug)
                    && self.scopes.ledger_only.contains(&slug)
                {
                    self.ledger_flow = Some(slug.clone());
                }
                // A hand-picked flow would be switched away from at the next
                // flow change otherwise.
                self.auto_flow = false;
                Some(Action::SwitchFlow(slug))
            }
            Action::SwitchScope(kind, scope) => {
                self.selector_open = false;
                if self.scope.as_ref() == Some(&(kind, scope.clone())) {
                    return None;
                }
                self.auto_flow = false;
                self.scope = Some((kind, scope.clone()));
                self.apply(Action::SwitchSurface(surface_of(kind)));
                Some(Action::SwitchScope(kind, scope))
            }
            Action::SwitchSurface(surface) => {
                self.surface = surface;
                if let Some(state) = self.items.get_mut(&surface) {
                    state.viewed();
                }
                if surface == Surface::Inbox {
                    self.inbox.viewed();
                }
                None
            }
            Action::ItemMove(dir) => {
                let delta = match dir {
                    Dir::Up => -1,
                    Dir::Down => 1,
                    Dir::Left | Dir::Right => return None,
                };
                if self.surface == Surface::Inbox {
                    self.edit_inbox(|inbox| inbox.move_cursor(delta));
                } else {
                    self.edit_items(|state| state.move_cursor(delta));
                }
                None
            }
            Action::SelectItem(id) if self.surface == Surface::Inbox => {
                self.edit_inbox(|inbox| {
                    if inbox.row(&id).is_some() {
                        inbox.cursor = Some(id);
                    }
                });
                None
            }
            Action::SelectItem(id) => {
                self.edit_items(|state| {
                    if state.row(&id).is_some() {
                        state.cursor = Some(id);
                    }
                });
                None
            }
            Action::ToggleMark => {
                self.edit_items(ItemsState::toggle_mark);
                None
            }
            Action::MarkVisible => {
                self.edit_items(ItemsState::mark_visible);
                None
            }
            Action::CycleGroup => {
                self.edit_items(ItemsState::cycle_group);
                if let Some(state) = self.current_items() {
                    let label = state.group.label();
                    self.notify(format!("group by {label}"));
                }
                None
            }
            Action::CycleSort => {
                self.edit_items(ItemsState::cycle_sort);
                if let Some(state) = self.current_items() {
                    let label = state.sort.label();
                    self.notify(format!("sort by {label}"));
                }
                None
            }
            Action::ToggleClosed if self.surface == Surface::Inbox => {
                self.edit_inbox(|inbox| {
                    inbox.show_closed = !inbox.show_closed;
                    inbox.move_cursor(0);
                });
                let shown = if self.inbox.show_closed {
                    "shown"
                } else {
                    "hidden"
                };
                self.notify(format!("closed items {shown}"));
                None
            }
            Action::ToggleClosed => {
                // A cursor on a row the toggle hid moves to the first visible one.
                self.edit_items(|state| {
                    state.show_closed = !state.show_closed;
                    state.move_cursor(0);
                });
                if let Some(state) = self.current_items() {
                    let shown = if state.show_closed { "shown" } else { "hidden" };
                    self.notify(format!("closed items {shown}"));
                }
                None
            }
            Action::OpenMenu => {
                if let Some(selection) = self.selection() {
                    if selection.transitions().is_empty() {
                        let statuses: Vec<&str> = selection.statuses().into_iter().collect();
                        self.notify(format!(
                            "no move is offered from {}",
                            statuses.join(" and ")
                        ));
                    } else {
                        self.overlay = Some(actions::menu(selection));
                    }
                }
                None
            }
            Action::OpenClassify => {
                if matches!(self.surface, Surface::Review | Surface::Optimise)
                    && let Some(selection) = self.selection()
                {
                    let form = actions::classify_form(&selection);
                    self.overlay = Some(Overlay::Form {
                        purpose: Purpose::Classify(selection),
                        form,
                    });
                }
                None
            }
            Action::OpenCapture => {
                self.open_input(InputForm::Capture);
                None
            }
            Action::OpenRequest => {
                if let Some(selection) = self.selection() {
                    self.open_input(InputForm::Request(selection));
                }
                None
            }
            Action::Withdraw if self.surface == Surface::Inbox => {
                match self.inbox.cursor_withdrawable().cloned() {
                    Some(record) => self.open_input(InputForm::Withdraw(record)),
                    None if self.inbox.cursor_row().is_some() => {
                        self.notify("only your own new records can be withdrawn".to_owned());
                    }
                    None => {}
                }
                None
            }
            Action::Withdraw => None,
            Action::OpenFilter => {
                if let Some(state) = self.current_items() {
                    let before = state.filter.clone();
                    self.overlay = Some(Overlay::Prompt {
                        input: Input::new(before.clone()),
                        before,
                    });
                }
                None
            }
            Action::Undo => {
                self.undo_last();
                None
            }
            Action::Back if self.overlay.is_some() => {
                if let Some(Overlay::Prompt { before, .. }) = self.overlay.take() {
                    self.set_filter(before);
                }
                None
            }
            Action::Back => {
                if let Some(state) = self.items.get_mut(&self.surface)
                    && !state.marks.is_empty()
                {
                    state.clear_marks();
                } else if self.legend_open {
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
        if self.ledger_flow.as_ref() == Some(&snapshot.slug) {
            self.ledger_flow = None;
        }
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

    /// Hands a ledger read to the item surface that lists its kind. Off screen,
    /// its arrivals count toward the surface's badge.
    pub(crate) fn apply_ledger(&mut self, ledger: Ledger, now: Instant) {
        let Some(surface) = Surface::ALL
            .into_iter()
            .find(|surface| surface.ledger_kind() == Some(ledger.kind))
        else {
            return;
        };
        let viewing = self.surface == surface;
        let state = self
            .items
            .entry(surface)
            .or_insert_with(|| ItemsState::new(surface));
        if self.ledger_paths.get(&surface) != Some(&ledger.path) {
            *state = ItemsState {
                filter: std::mem::take(&mut state.filter),
                group: state.group,
                sort: state.sort,
                show_closed: state.show_closed,
                ..ItemsState::new(surface)
            };
            self.ledger_paths.insert(surface, ledger.path);
        }
        state.apply_ledger(ledger.rows, ledger.revision, viewing, now);
    }

    /// Hands an input store read to the Inbox. Off screen, its arrivals count toward the
    /// Inbox badge.
    pub(crate) fn apply_inputs(&mut self, inputs: Inputs, now: Instant) {
        let viewing = self.surface == Surface::Inbox;
        self.inbox
            .apply_inputs(inputs.rows, inputs.revision, viewing, now);
    }

    /// The questions waiting on an answer; `None` until the input store is first read.
    pub(crate) fn inbox_unanswered(&self) -> Option<usize> {
        self.inbox
            .revision
            .is_some()
            .then(|| self.inbox.unanswered())
    }

    /// The Inbox cursor record while the Inbox is shown and it is an answerable question.
    fn inbox_question(&self) -> Option<&InputRow> {
        (self.surface == Surface::Inbox)
            .then(|| self.inbox.cursor_question())
            .flatten()
    }

    /// Whether the current item surface or the Inbox has a row under its cursor.
    fn has_cursor_row(&self) -> bool {
        if self.surface == Surface::Inbox {
            return self.inbox.cursor_row().is_some();
        }
        self.current_items()
            .and_then(ItemsState::cursor_row)
            .is_some()
    }

    fn open_input(&mut self, input: InputForm) {
        let form = input.form();
        self.overlay = Some(Overlay::Form {
            purpose: Purpose::Input(Box::new(input)),
            form,
        });
    }

    /// Takes a new ledger scope listing. The selector cursor stays in range, and a picked
    /// scope or ledger-only flow that vanished stays picked until something else is.
    pub(crate) fn apply_scopes(&mut self, scopes: Scopes) {
        self.scopes = scopes;
        let last = self.selector_entries().len().saturating_sub(1);
        self.selector_cursor = self.selector_cursor.min(last);
    }

    /// The selector's rows: flows with a task store, ledger-only flows, then flow-less
    /// ledgers. A ledger-only slug the flow list already holds is listed once, as a flow.
    pub(crate) fn selector_entries(&self) -> Vec<SelectorEntry<'_>> {
        let flows = self.flows.iter().map(SelectorEntry::Flow);
        let ledger_only = self
            .scopes
            .ledger_only
            .iter()
            .filter(|slug| !self.flows.iter().any(|flow| &flow.slug == *slug))
            .map(|slug| SelectorEntry::LedgerOnly(slug.as_str()));
        let scopes = self.scopes.scopes.iter().map(SelectorEntry::Scope);
        flows.chain(ledger_only).chain(scopes).collect()
    }

    /// Whether `entry` is what the item surfaces and the task view show now.
    pub(crate) fn is_current(&self, entry: SelectorEntry<'_>) -> bool {
        match (entry, &self.scope) {
            (SelectorEntry::Scope(e), Some((kind, scope))) => e.kind == *kind && e.scope == *scope,
            (SelectorEntry::Scope(_), None) | (_, Some(_)) => false,
            (SelectorEntry::LedgerOnly(slug), None) => self.ledger_flow.as_deref() == Some(slug),
            (SelectorEntry::Flow(flow), None) => {
                self.ledger_flow.is_none() && flow.slug == self.snapshot.slug
            }
        }
    }

    /// The current surface's item state; `None` on Tasks and Inbox.
    pub(crate) fn current_items(&self) -> Option<&ItemsState> {
        self.items.get(&self.surface)
    }

    /// Edits the current surface's item state; a cursor the edit moved starts the
    /// details scroll over, as a new task selection does.
    fn edit_items(&mut self, edit: impl FnOnce(&mut ItemsState)) {
        let Some(state) = self.items.get_mut(&self.surface) else {
            return;
        };
        let before = state.cursor.clone();
        edit(state);
        if state.cursor != before {
            self.details_scroll = 0;
        }
    }

    /// [`App::edit_items`] for the Inbox.
    fn edit_inbox(&mut self, edit: impl FnOnce(&mut InboxState)) {
        let before = self.inbox.cursor.clone();
        edit(&mut self.inbox);
        if self.inbox.cursor != before {
            self.details_scroll = 0;
        }
    }

    /// Sends a key to the open overlay. Returns false when none is open, so the key takes
    /// its usual meaning.
    pub(crate) fn overlay_key(&mut self, key: KeyEvent) -> bool {
        let Some(overlay) = self.overlay.take() else {
            return false;
        };
        self.overlay = match overlay {
            prompt @ Overlay::Prompt { .. } if key.kind == KeyEventKind::Release => Some(prompt),
            Overlay::Prompt { before, .. } if key.code == KeyCode::Esc => {
                self.set_filter(before);
                None
            }
            Overlay::Prompt { .. } if key.code == KeyCode::Enter => None,
            Overlay::Prompt { mut input, before } => {
                input.handle_event(&TermEvent::Key(key));
                self.set_filter(input.value().to_owned());
                Some(Overlay::Prompt { input, before })
            }
            Overlay::Menu {
                selection,
                choices,
                mut form,
            } => match form.handle_key(key) {
                FormOutcome::Pending => Some(Overlay::Menu {
                    selection,
                    choices,
                    form,
                }),
                FormOutcome::Cancel => None,
                FormOutcome::Submit(values) => {
                    let picked = match values.as_slice() {
                        [FieldValue::One(to)] => choices.iter().find(|t| t.to == to).copied(),
                        _ => None,
                    };
                    picked.and_then(|transition| self.picked(selection, transition))
                }
            },
            Overlay::Form { purpose, mut form } => match form.handle_key(key) {
                FormOutcome::Pending => Some(Overlay::Form { purpose, form }),
                FormOutcome::Cancel => None,
                FormOutcome::Submit(values) => self.submitted(purpose, form, &values),
            },
        };
        true
    }

    /// The write requests made since the last call, for the runtime to hand to the writer.
    pub(crate) fn take_writes(&mut self) -> Vec<WriteRequest> {
        std::mem::take(&mut self.writes)
    }

    /// Takes a writer outcome. An error clears every row the request marked saving and a
    /// stale skip clears that row, each with a footer notice; applied rows stay saving until
    /// a ledger read shows them changed.
    pub(crate) fn apply_written(&mut self, outcome: WriteOutcome) {
        let Some(flight) = self.in_flight.remove(&outcome.request) else {
            return;
        };
        let stale: Vec<&str> = outcome
            .skipped_stale
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        if let Some(saving) = self.saving_mut(flight.surface) {
            if outcome.error.is_some() {
                flight.ids.iter().for_each(|id| {
                    saving.remove(id);
                });
            } else {
                stale.iter().for_each(|id| {
                    saving.remove(*id);
                });
            }
        }
        if !flight.undo
            && let Some(at) = self
                .undo
                .iter()
                .position(|entry| entry.outstanding.contains(&outcome.request))
        {
            let entry = &mut self.undo[at];
            entry.outstanding.remove(&outcome.request);
            entry.applied.extend(outcome.applied.iter().cloned());
            if entry.outstanding.is_empty() && entry.applied.is_empty() {
                self.undo.remove(at);
            }
        }
        let what = if flight.undo { "undo" } else { "write" };
        if let Some(error) = &outcome.error {
            self.notify(format!("{what} failed: {error}"));
        } else if !stale.is_empty() {
            let ids = stale.join(", ");
            self.notify(format!("{what} skipped {ids}: changed since shown"));
        }
    }

    /// The current item surface's targets as an action sees them, or `None` with a notice
    /// when nothing there can be written.
    fn selection(&mut self) -> Option<Selection> {
        let surface = self.surface;
        let kind = surface.ledger_kind()?;
        let state = self.items.get(&surface)?;
        let rows: Vec<ItemRow> = state
            .targets()
            .iter()
            .filter_map(|id| state.row(id).cloned())
            .collect();
        let refusal = if rows.is_empty() {
            Some("nothing selected".to_owned())
        } else {
            rows.iter()
                .find(|r| r.read_only)
                .map(|row| format!("{} has no ledger id, so it cannot be changed", row.id))
        };
        if let Some(refusal) = refusal {
            self.notify(refusal);
            return None;
        }
        let Some(ledger) = self
            .ledger_paths
            .get(&surface)
            .and_then(|path| actions::ledger_ref(kind, path))
        else {
            self.notify("this ledger is read-only".to_owned());
            return None;
        };
        Some(Selection {
            surface,
            kind,
            ledger,
            rows,
        })
    }

    /// The overlay after a move is picked from the menu: a confirmation when it declines a
    /// critical row, else the move's form.
    fn picked(&mut self, selection: Selection, transition: Transition) -> Option<Overlay> {
        if !selection.critical_declines(transition.to).is_empty() {
            let form = actions::confirm_form(&selection, transition.to);
            return Some(Overlay::Form {
                purpose: Purpose::Confirm(selection, transition),
                form,
            });
        }
        let form = actions::transition_form(&selection, transition);
        Some(Overlay::Form {
            purpose: Purpose::Transition(selection, transition),
            form,
        })
    }

    /// The overlay after `form` submitted `values`: `None` once its writes are queued, or the
    /// form again carrying the error when an input record cannot be made from them.
    fn submitted(
        &mut self,
        purpose: Purpose,
        mut form: Form,
        values: &[FieldValue],
    ) -> Option<Overlay> {
        match purpose {
            Purpose::Input(input) => match input.submit(values, &mut self.next_request) {
                Ok(Some(request)) => {
                    self.dispatch_input(&input, request);
                    None
                }
                Ok(None) => None,
                Err(error) => {
                    form.error = Some(error);
                    Some(Overlay::Form {
                        purpose: Purpose::Input(input),
                        form,
                    })
                }
            },
            Purpose::Confirm(selection, transition) => {
                let yes = matches!(values, [FieldValue::One(answer)] if answer == "yes");
                yes.then(|| {
                    let form = actions::transition_form(&selection, transition);
                    Overlay::Form {
                        purpose: Purpose::Transition(selection, transition),
                        form,
                    }
                })
            }
            Purpose::Transition(selection, transition) => {
                let fields = actions::companions(transition, values);
                let plan = actions::transition_plan(
                    &selection,
                    transition,
                    &fields,
                    &mut self.next_request,
                );
                self.dispatch(&selection, plan);
                None
            }
            Purpose::Classify(selection) => {
                let plan = actions::classify_plan(&selection, values, &mut self.next_request);
                if plan.requests.is_empty() {
                    self.notify("nothing to change".to_owned());
                } else {
                    self.dispatch(&selection, plan);
                }
                None
            }
        }
    }

    /// Queues `plan`'s requests, marks their rows saving and pushes its undo entry.
    fn dispatch(&mut self, selection: &Selection, plan: Plan) {
        let mut outstanding = BTreeSet::new();
        for (request, ids) in plan.requests {
            outstanding.insert(request.request());
            self.track(selection.surface, request, ids, false);
        }
        self.undo.push(UndoEntry {
            surface: selection.surface,
            kind: UndoKind::Rows {
                ledger: selection.ledger.clone(),
                rows: plan.undo,
            },
            outstanding,
            applied: Default::default(),
        });
    }

    /// Queues an input store write. A record it creates is undone by withdrawing it; a
    /// withdrawal has no undo. The Inbox rows it changes are marked saving.
    fn dispatch_input(&mut self, input: &InputForm, request: WriteRequest) {
        let (surface, ids) = match input {
            InputForm::Answer(row) | InputForm::Withdraw(row) => {
                (Surface::Inbox, vec![row.id.clone()])
            }
            InputForm::Capture | InputForm::Request(_) => (self.surface, Vec::new()),
        };
        if !matches!(input, InputForm::Withdraw(_)) {
            self.undo.push(UndoEntry {
                surface: Surface::Inbox,
                kind: UndoKind::Input,
                outstanding: [request.request()].into(),
                applied: Default::default(),
            });
        }
        self.track(surface, request, ids, false);
    }

    /// The saving set of an item surface or the Inbox.
    fn saving_mut(&mut self, surface: Surface) -> Option<&mut BTreeSet<String>> {
        if surface == Surface::Inbox {
            return Some(&mut self.inbox.saving);
        }
        self.items.get_mut(&surface).map(|state| &mut state.saving)
    }

    fn track(&mut self, surface: Surface, request: WriteRequest, ids: Vec<String>, undo: bool) {
        if let Some(saving) = self.saving_mut(surface) {
            saving.extend(ids.iter().cloned());
        }
        self.in_flight
            .insert(request.request(), InFlight { surface, ids, undo });
        self.writes.push(request);
    }

    /// Pops the newest undo entry into one restore per row its write applied, or one
    /// withdrawal of the records it created. An entry still awaiting an outcome stays, since
    /// what to put back is not yet known.
    fn undo_last(&mut self) {
        let Some(top) = self.undo.last() else {
            self.notify("nothing to undo".to_owned());
            return;
        };
        if !top.outstanding.is_empty() {
            self.notify("the last write is still saving".to_owned());
            return;
        }
        let Some(entry) = self.undo.pop() else {
            return;
        };
        let ids: Vec<String> = match &entry.kind {
            UndoKind::Rows { ledger, rows } => {
                let rows: Vec<_> = rows
                    .iter()
                    .filter(|row| entry.applied.contains(&row.id))
                    .collect();
                for row in &rows {
                    let request = row.restore(self.next_request, ledger);
                    self.next_request += 1;
                    self.track(entry.surface, request, vec![row.id.clone()], true);
                }
                rows.iter().map(|row| row.id.clone()).collect()
            }
            UndoKind::Input => {
                let ids: Vec<String> = entry.applied.iter().cloned().collect();
                let request = actions::withdraw(ids.clone(), &mut self.next_request);
                self.track(entry.surface, request, ids.clone(), true);
                ids
            }
        };
        self.notify(format!("undoing {}", ids.join(", ")));
    }

    /// Sets the current surface's filter, moving a cursor it hides to the first shown row.
    fn set_filter(&mut self, filter: String) {
        self.edit_items(|state| {
            state.filter = filter;
            state.move_cursor(0);
        });
    }

    /// Called on each timed wake-up: drops expired flashes and re-reads the
    /// transcript ages that are due, since an agent goes stale without any snapshot.
    pub(crate) fn tick(&mut self, now: Instant) {
        self.expire_flashes(now);
        for state in self.items.values_mut() {
            state.expire_flashes(now);
        }
        self.inbox.expire_flashes(now);
        if self.live_notice(now).is_none() {
            self.notice = None;
        }
        self.recheck_stale(SystemTime::now(), transcript_mtime);
    }

    /// True while something on screen changes with time alone: a live task or item
    /// flash or notice, or a running agent whose elapsed counter is still advancing.
    pub(crate) fn needs_tick(&self, now: Instant) -> bool {
        let live = |at: &Instant| now.saturating_duration_since(*at) < FLASH;
        self.flashes.values().any(live)
            || self
                .items
                .values()
                .any(|state| state.flashes.values().any(live))
            || self.inbox.flashes.values().any(live)
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

    /// Every selection change goes through here so the details scroll starts over,
    /// the view follows the selection again, and the traversal trail, which no longer
    /// leads here, is dropped.
    fn select(&mut self, id: Option<u32>) {
        if id != self.selected {
            self.details_scroll = 0;
            self.scroll_pinned = false;
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

fn surface_of(kind: LedgerKind) -> Surface {
    match kind {
        LedgerKind::Review => Surface::Review,
        LedgerKind::Optimise => Surface::Optimise,
        LedgerKind::PlanReview => Surface::PlanReview,
    }
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
    use crate::ledger::{ItemRow, Kind, StatusClass};
    use crate::model::fixture;
    use crate::surface::{Group, Sort};

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
    fn the_wheel_pins_the_view_until_the_selection_moves() {
        let mut app = app();
        let selected = app.selected;
        app.apply(Action::ScrollView(0, 3));
        app.apply(Action::ScrollView(0, 3));
        assert_eq!(app.selected, selected, "the wheel never selects");
        assert!(app.scroll_pinned);
        assert_eq!(app.scroll_nudge, (0, 6));
        app.apply_snapshot(fixture(), Instant::now());
        assert!(app.scroll_pinned, "the same selection keeps the pin");
        app.apply(Action::Select(1));
        assert!(!app.scroll_pinned);
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

    fn scopes() -> Scopes {
        Scopes {
            ledger_only: vec!["loose".to_string(), "other".to_string()],
            scopes: vec![ScopeEntry {
                kind: LedgerKind::Optimise,
                scope: "core".to_string(),
            }],
        }
    }

    #[test]
    fn the_selector_picks_ledger_only_flows_and_flowless_scopes() {
        let mut app = app();
        app.flows = vec![flow("other"), flow("demo-flow")];
        app.apply_scopes(scopes());
        let entries = app.selector_entries();
        assert_eq!(entries.len(), 4, "`other` is listed once, as a flow");
        assert_eq!(entries[2], SelectorEntry::LedgerOnly("loose"));

        app.apply(Action::ToggleSelector);
        app.apply(Action::Move(Dir::Down));
        assert_eq!(
            app.apply(Action::Details),
            Some(Action::SwitchFlow("loose".to_string())),
            "a ledger-only flow switches like any flow"
        );
        assert_eq!(app.ledger_flow.as_deref(), Some("loose"));

        app.apply(Action::ToggleSelector);
        assert_eq!(
            app.selector_cursor, 2,
            "the cursor starts on the picked flow"
        );
        app.apply(Action::Move(Dir::Down));
        app.apply(Action::Move(Dir::Down));
        assert_eq!(
            app.selector_cursor, 3,
            "the cursor clamps on the last scope"
        );
        assert_eq!(
            app.apply(Action::Details),
            Some(Action::SwitchScope(
                LedgerKind::Optimise,
                "core".to_string()
            ))
        );
        assert!(!app.selector_open);
        assert_eq!(app.scope, Some((LedgerKind::Optimise, "core".to_string())));
        assert_eq!(
            app.surface,
            Surface::Optimise,
            "the scope's surface is shown"
        );
        assert_eq!(
            app.apply(Action::SwitchScope(
                LedgerKind::Optimise,
                "core".to_string()
            )),
            None
        );

        assert_eq!(
            app.apply(Action::SwitchFlow("demo-flow".to_string())),
            Some(Action::SwitchFlow("demo-flow".to_string())),
            "the current flow is switched back to from a scope"
        );
        assert_eq!(app.scope, None);
        assert_eq!(app.ledger_flow, None);

        app.selector_cursor = 3;
        app.apply_scopes(Scopes::default());
        assert_eq!(
            app.selector_cursor, 1,
            "a shorter listing clamps the cursor"
        );
    }

    fn review(path: &str, revision: &str, rows: &[(&str, &str)]) -> Ledger {
        Ledger {
            kind: Kind::Review,
            path: path.to_string(),
            revision: Some(revision.to_string()),
            rows: rows
                .iter()
                .map(|(id, status)| ItemRow {
                    id: id.to_string(),
                    status: status.to_string(),
                    class: StatusClass::of(status),
                    ..ItemRow::default()
                })
                .collect(),
        }
    }

    fn review_app() -> App {
        let mut app = app();
        let rows = [("R1", "open"), ("R2", "open"), ("R3", "fixed")];
        app.apply_ledger(review("r.toml", "v1", &rows), Instant::now());
        app
    }

    fn cursor(app: &App) -> Option<&str> {
        app.current_items()?.cursor.as_deref()
    }

    #[test]
    fn switching_surface_keeps_the_task_selection() {
        let mut app = review_app();
        app.nav = Some(Box::new(Step));
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::Move(Dir::Down));
        assert_eq!(cursor(&app), Some("R2"), "a move goes to the item cursor");
        assert_eq!(app.selected, Some(4), "the task selection is untouched");
        assert!(app.follow, "an item move does not pause follow");

        app.apply(Action::SwitchSurface(Surface::Tasks));
        assert_eq!(app.selected, Some(4));
        app.apply(Action::Move(Dir::Down));
        assert_eq!(app.selected, Some(5));
        app.apply(Action::SwitchSurface(Surface::Review));
        assert_eq!(
            cursor(&app),
            Some("R2"),
            "each surface keeps its own cursor"
        );
    }

    #[test]
    fn back_clears_marks_before_closing_details() {
        let mut app = review_app();
        app.apply(Action::SwitchSurface(Surface::Review));
        app.legend_open = true;
        app.apply(Action::Details);
        app.apply(Action::ToggleMark);
        assert_eq!(app.current_items().map(|s| s.marks.len()), Some(1));

        assert_eq!(app.apply(Action::Back), None);
        assert!(app.current_items().is_some_and(|s| s.marks.is_empty()));
        assert!(app.legend_open && app.details_open, "nothing closed yet");
        app.apply(Action::Back);
        app.apply(Action::Back);
        assert!(!app.legend_open && !app.details_open);
    }

    #[test]
    fn details_on_an_item_surface_needs_a_cursor_row() {
        let mut app = app();
        app.apply(Action::SwitchSurface(Surface::Optimise));
        app.apply(Action::Details);
        assert!(!app.details_open, "an empty surface has nothing to detail");
        app.apply(Action::SwitchSurface(Surface::Inbox));
        app.apply(Action::Move(Dir::Down));
        assert_eq!(app.selected, Some(4), "Inbox moves never reach the tasks");
    }

    #[test]
    fn item_actions_reach_only_the_current_surface() {
        let mut app = review_app();
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::ToggleClosed);
        app.apply(Action::SelectItem("R3".to_string()));
        assert_eq!(cursor(&app), Some("R3"));
        app.apply(Action::SelectItem("R9".to_string()));
        assert_eq!(cursor(&app), Some("R3"), "an unknown id is not selected");
        app.apply(Action::ToggleClosed);
        assert_eq!(
            cursor(&app),
            Some("R1"),
            "a hidden cursor moves to a shown row"
        );

        app.apply(Action::CycleGroup);
        app.apply(Action::CycleSort);
        app.apply(Action::MarkVisible);
        let review = &app.items[&Surface::Review];
        assert_eq!(
            (review.group, review.sort),
            (Group::Severity, Sort::Severity)
        );
        assert_eq!(review.marks.len(), 2);
        let backlog = &app.items[&Surface::Backlog];
        assert_eq!((backlog.group, backlog.sort), (Group::None, Sort::Id));
    }

    #[test]
    fn item_flashes_keep_the_clock_running_and_arrivals_badge_off_screen() {
        let mut app = review_app();
        let mut snap = fixture();
        snap.agents
            .retain(|agent| agent.status != AgentStatus::Running);
        app.apply_snapshot(snap, Instant::now());
        let t0 = Instant::now();
        let rows = [("R1", "deferred"), ("R2", "open"), ("R3", "fixed")];
        app.apply_ledger(review("r.toml", "v2", &rows), t0);
        assert_eq!(app.items[&Surface::Review].new_since_view, 1);
        assert_eq!(app.tick_interval(t0), Some(TICK), "R1 is flashing");
        app.tick(t0 + FLASH);
        assert_eq!(app.tick_interval(t0 + FLASH), None);

        app.apply(Action::SwitchSurface(Surface::Review));
        assert_eq!(app.items[&Surface::Review].new_since_view, 0);
    }

    #[test]
    fn a_ledger_from_another_file_starts_its_surface_over() {
        let mut app = review_app();
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::CycleGroup);
        app.apply(Action::ToggleMark);
        let t0 = Instant::now();
        app.apply_ledger(review("other.toml", "v1", &[("R7", "open")]), t0);
        let state = &app.items[&Surface::Review];
        assert!(state.flashes.is_empty(), "a new file is a first read");
        assert!(state.marks.is_empty());
        assert_eq!(state.group, Group::Severity, "the user's arrangement stays");
        assert_eq!(cursor(&app), Some("R7"));
    }

    const REVIEW_PATH: &str = ".claude/flows/demo/review-ledger.toml";

    /// A ledger whose rows carry their raw form, as a real read does.
    fn ledger(kind: Kind, path: &str, rows: &[(&str, &str, &str)]) -> Ledger {
        Ledger {
            kind,
            path: path.to_string(),
            revision: Some("v1".to_string()),
            rows: rows
                .iter()
                .map(|(id, status, severity)| ItemRow {
                    id: id.to_string(),
                    read_only: id.starts_with('#'),
                    status: status.to_string(),
                    class: StatusClass::of(status),
                    severity: severity.to_string(),
                    raw: serde_json::json!({"id": id, "status": status, "severity": severity}),
                    ..ItemRow::default()
                })
                .collect(),
        }
    }

    fn writable_review_app() -> App {
        let mut app = app();
        let rows = [
            ("R1", "open", "warning"),
            ("R2", "open", "critical"),
            ("R3", "deferred", "warning"),
        ];
        app.apply_ledger(ledger(Kind::Review, REVIEW_PATH, &rows), Instant::now());
        app.apply(Action::SwitchSurface(Surface::Review));
        app
    }

    fn mark(app: &mut App, surface: Surface, ids: &[&str]) {
        let state = app.items.get_mut(&surface).expect("an item surface");
        state.marks = ids.iter().map(|id| id.to_string()).collect();
    }

    fn key(app: &mut App, code: KeyCode) {
        let event = KeyEvent::new(code, ratatui::crossterm::event::KeyModifiers::NONE);
        assert!(app.overlay_key(event), "an overlay is open for {code:?}");
    }

    fn type_text(app: &mut App, text: &str) {
        text.chars().for_each(|c| key(app, KeyCode::Char(c)));
    }

    fn menu_choices(app: &App) -> Vec<&'static str> {
        match &app.overlay {
            Some(Overlay::Menu { choices, .. }) => choices.iter().map(|t| t.to).collect(),
            other => panic!("expected the menu, got {other:?}"),
        }
    }

    fn saving(app: &App) -> Vec<&str> {
        app.items[&app.surface]
            .saving
            .iter()
            .map(String::as_str)
            .collect()
    }

    /// Marks R1 and R2 and defers them through the menu and its form.
    fn defer_r1_r2(app: &mut App) {
        mark(app, Surface::Review, &["R1", "R2"]);
        app.apply(Action::OpenMenu);
        assert_eq!(
            menu_choices(app),
            vec!["deferred", "wontfix", "verified-clean"]
        );
        key(app, KeyCode::Enter);
        type_text(app, "later");
        key(app, KeyCode::Tab);
        type_text(app, "v2");
        key(app, KeyCode::Enter);
        assert!(app.overlay.is_none(), "submitting closes the form");
    }

    #[test]
    fn deferring_two_marked_findings_submits_one_transition() {
        let mut app = writable_review_app();
        defer_r1_r2(&mut app);
        let writes = app.take_writes();
        assert_eq!(writes.len(), 1, "{writes:?}");
        let WriteRequest::Transition {
            ledger,
            ids,
            to,
            fields,
            expect_status,
            ..
        } = &writes[0]
        else {
            panic!("expected a transition, got {:?}", writes[0]);
        };
        assert_eq!(
            ledger,
            &tomlctl::LedgerRef::Flow {
                slug: "demo".to_string(),
                kind: LedgerKind::Review
            }
        );
        assert_eq!(ids, &vec!["R1".to_string(), "R2".to_string()]);
        assert_eq!((to.as_str(), expect_status.as_str()), ("deferred", "open"));
        assert_eq!(fields["defer_reason"], "later");
        assert_eq!(fields["defer_trigger"], "v2");
        assert_eq!(saving(&app), vec!["R1", "R2"]);
        assert!(app.take_writes().is_empty(), "taken once");
    }

    #[test]
    fn a_mixed_status_selection_offers_only_common_transitions() {
        let mut app = writable_review_app();
        mark(&mut app, Surface::Review, &["R1", "R3"]);
        app.apply(Action::OpenMenu);
        assert!(app.overlay.is_none(), "open and deferred share no move");
        assert_eq!(
            app.live_notice(Instant::now()),
            Some("no move is offered from deferred and open")
        );

        let rows = [
            ("B-1", "open", ""),
            ("B-2", "dismissed", ""),
            ("B-3", "resolved", ""),
        ];
        app.apply_ledger(
            ledger(Kind::Backlog, ".claude/backlog.toml", &rows),
            Instant::now(),
        );
        app.apply(Action::SwitchSurface(Surface::Backlog));
        mark(&mut app, Surface::Backlog, &["B-2", "B-3"]);
        app.apply(Action::OpenMenu);
        assert_eq!(menu_choices(&app), vec!["open"]);
        key(&mut app, KeyCode::Enter);
        type_text(&mut app, "again");
        key(&mut app, KeyCode::Enter);
        let from: Vec<String> = app
            .take_writes()
            .into_iter()
            .map(|write| match write {
                WriteRequest::BacklogTriage { expect_status, .. } => expect_status,
                other => panic!("expected a backlog triage, got {other:?}"),
            })
            .collect();
        assert_eq!(
            from,
            vec!["dismissed", "resolved"],
            "one request per status"
        );
    }

    #[test]
    fn undo_submits_a_restore_with_the_written_values() {
        let mut app = writable_review_app();
        defer_r1_r2(&mut app);
        let request = app.take_writes()[0].request();
        app.apply(Action::Undo);
        assert!(
            app.take_writes().is_empty(),
            "nothing is undone before the outcome"
        );
        app.apply_written(WriteOutcome {
            request,
            applied: vec!["R1".to_string(), "R2".to_string()],
            ..WriteOutcome::default()
        });

        app.apply(Action::Undo);
        let restores = app.take_writes();
        assert_eq!(restores.len(), 2, "one restore per applied id");
        let WriteRequest::Restore {
            ledger,
            id,
            set,
            unset,
            expect,
            ..
        } = &restores[0]
        else {
            panic!("expected a restore, got {:?}", restores[0]);
        };
        assert_eq!(id, "R1");
        assert!(matches!(ledger, tomlctl::LedgerRef::Flow { .. }));
        assert_eq!(
            set,
            serde_json::json!({"status": "open"}).as_object().unwrap()
        );
        assert_eq!(
            unset,
            &vec!["defer_reason".to_string(), "defer_trigger".to_string()]
        );
        assert_eq!(
            expect,
            serde_json::json!({"status": "deferred", "defer_reason": "later", "defer_trigger": "v2"})
                .as_object()
                .unwrap()
        );
        assert!(app.undo.is_empty());
        app.apply(Action::Undo);
        assert_eq!(app.live_notice(Instant::now()), Some("nothing to undo"));
    }

    #[test]
    fn a_stale_outcome_clears_saving_and_notices() {
        let mut app = writable_review_app();
        defer_r1_r2(&mut app);
        let request = app.take_writes()[0].request();
        app.apply_written(WriteOutcome {
            request,
            applied: vec!["R1".to_string()],
            skipped_stale: vec![crate::writer::Stale {
                id: "R2".to_string(),
                field: "status".to_string(),
                expected: "open".into(),
                found: "fixed".into(),
            }],
            error: None,
        });
        assert_eq!(
            saving(&app),
            vec!["R1"],
            "the applied row waits for the read"
        );
        assert_eq!(
            app.live_notice(Instant::now()),
            Some("write skipped R2: changed since shown")
        );
        app.apply(Action::Undo);
        let ids: Vec<String> = app
            .take_writes()
            .into_iter()
            .map(|write| match write {
                WriteRequest::Restore { id, .. } => id,
                other => panic!("expected a restore, got {other:?}"),
            })
            .collect();
        assert_eq!(ids, vec!["R1"], "only what the write applied is put back");
    }

    #[test]
    fn a_failed_write_clears_every_row_it_marked() {
        let mut app = writable_review_app();
        defer_r1_r2(&mut app);
        let request = app.take_writes()[0].request();
        app.apply_written(WriteOutcome {
            request,
            error: Some("root mismatch".to_string()),
            ..WriteOutcome::default()
        });
        assert!(saving(&app).is_empty());
        assert!(
            app.undo.is_empty(),
            "a write that changed nothing leaves no undo"
        );
        assert_eq!(
            app.live_notice(Instant::now()),
            Some("write failed: root mismatch")
        );
    }

    #[test]
    fn declining_a_critical_finding_asks_first() {
        let mut app = writable_review_app();
        mark(&mut app, Surface::Review, &["R2"]);
        app.apply(Action::OpenMenu);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        assert!(
            matches!(
                &app.overlay,
                Some(Overlay::Form {
                    purpose: Purpose::Confirm(..),
                    ..
                })
            ),
            "wontfix on a critical row asks first"
        );
        key(&mut app, KeyCode::Enter);
        assert!(app.overlay.is_none(), "the default answer is no");
        assert!(app.take_writes().is_empty());

        app.apply(Action::OpenMenu);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        assert!(matches!(
            &app.overlay,
            Some(Overlay::Form {
                purpose: Purpose::Transition(..),
                ..
            })
        ));
    }

    #[test]
    fn a_read_only_cursor_row_is_refused() {
        let mut app = app();
        let rows = [("#1", "open", "")];
        app.apply_ledger(ledger(Kind::Review, REVIEW_PATH, &rows), Instant::now());
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::OpenMenu);
        assert!(app.overlay.is_none());
        assert_eq!(
            app.live_notice(Instant::now()),
            Some("#1 has no ledger id, so it cannot be changed")
        );

        let mut app = review_app();
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::OpenMenu);
        assert!(app.overlay.is_none(), "a ledger named by path is read-only");
    }

    #[test]
    fn the_filter_prompt_filters_live_and_back_closes_it_first() {
        let mut app = writable_review_app();
        app.apply(Action::ToggleMark);
        app.apply(Action::OpenFilter);
        type_text(&mut app, "R3");
        assert_eq!(app.items[&Surface::Review].filter, "R3");
        assert_eq!(
            cursor(&app),
            Some("R3"),
            "a hidden cursor moves to a shown row"
        );
        key(&mut app, KeyCode::Esc);
        assert!(app.overlay.is_none());
        assert_eq!(app.items[&Surface::Review].filter, "", "cancel restores");

        app.apply(Action::OpenFilter);
        type_text(&mut app, "R");
        app.apply(Action::Back);
        assert!(app.overlay.is_none());
        assert_eq!(
            app.items[&Surface::Review].marks.len(),
            1,
            "Back closed the prompt, not the marks"
        );
        assert!(!app.overlay_key(KeyEvent::from(KeyCode::Char('x'))));
    }

    fn record(id: &str, kind: &str, author: &str, status: &str) -> InputRow {
        InputRow {
            id: id.to_string(),
            kind: kind.to_string(),
            author: author.to_string(),
            status: status.to_string(),
            prompt: "which first?".to_string(),
            choice: "single".to_string(),
            options: vec!["R1".to_string(), "R2".to_string()],
            ..InputRow::default()
        }
    }

    /// An app on the Inbox holding a question `I1` and the user's own note `I2`.
    fn inbox_app() -> App {
        let mut app = app();
        app.apply(Action::SwitchSurface(Surface::Inbox));
        let rows = vec![
            record("I1", "question", "review", "new"),
            record("I2", "note", "user", "new"),
        ];
        let inputs = Inputs {
            path: ".claude/inputs.toml".to_string(),
            revision: Some("v1".to_string()),
            rows,
        };
        app.apply_inputs(inputs, Instant::now());
        app
    }

    #[test]
    fn toggling_closed_on_the_inbox_shows_handled_and_withdrawn_records() {
        let mut app = inbox_app();
        let mut rows = app.inbox.rows.clone();
        rows.push(record("I3", "note", "user", "handled"));
        rows.push(record("I4", "note", "user", "withdrawn"));
        app.apply_inputs(
            Inputs {
                path: ".claude/inputs.toml".to_string(),
                revision: Some("v2".to_string()),
                rows,
            },
            Instant::now(),
        );
        let shown = |app: &App| {
            app.inbox
                .visible()
                .iter()
                .filter(|row| matches!(row, crate::surface::VisibleRow::Item(_)))
                .count()
        };
        assert_eq!(shown(&app), 2);
        app.apply(Action::ToggleClosed);
        assert!(app.inbox.show_closed);
        assert_eq!(shown(&app), 4);
        assert_eq!(
            app.notice.as_ref().map(|(text, _)| text.as_str()),
            Some("closed items shown")
        );
        app.inbox.cursor = Some("I3".to_string());
        app.apply(Action::ToggleClosed);
        assert!(!app.inbox.show_closed);
        assert_eq!(shown(&app), 2);
        assert_ne!(
            app.inbox.cursor.as_deref(),
            Some("I3"),
            "a hidden cursor moves to a shown row"
        );
    }

    fn input_purpose(app: &App) -> Option<&InputForm> {
        match &app.overlay {
            Some(Overlay::Form {
                purpose: Purpose::Input(input),
                ..
            }) => Some(input.as_ref()),
            _ => None,
        }
    }

    #[test]
    fn apply_inputs_counts_unanswered_questions_once_read() {
        let mut app = app();
        assert_eq!(app.inbox_unanswered(), None, "nothing read yet");
        let inputs = Inputs {
            path: String::new(),
            revision: None,
            rows: Vec::new(),
        };
        app.apply_inputs(inputs, Instant::now());
        assert_eq!(app.inbox_unanswered(), Some(0), "a missing store is a read");
        assert_eq!(inbox_app().inbox_unanswered(), Some(1));
    }

    #[test]
    fn enter_on_a_question_opens_its_answer_form() {
        let mut app = inbox_app();
        assert_eq!(app.inbox.cursor.as_deref(), Some("I1"));
        app.apply(Action::Details);
        assert!(
            matches!(input_purpose(&app), Some(InputForm::Answer(q)) if q.id == "I1"),
            "{:?}",
            app.overlay
        );
        assert!(
            !app.details_open,
            "the answer form opens in place of details"
        );

        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        assert!(app.overlay.is_none(), "{:?}", app.overlay);
        let writes = app.take_writes();
        assert!(
            matches!(
                writes.as_slice(),
                [WriteRequest::InputAnswer { question, picked, .. }]
                    if question == "I1" && picked == &vec!["R2".to_string()]
            ),
            "{writes:?}"
        );
        assert_eq!(app.inbox.saving.iter().collect::<Vec<_>>(), vec!["I1"]);

        app.apply(Action::Move(Dir::Down));
        assert_eq!(app.inbox.cursor.as_deref(), Some("I2"));
        app.apply(Action::Details);
        assert!(app.overlay.is_none(), "a note has no answer form");
        assert!(app.details_open);
    }

    #[test]
    fn a_refused_input_keeps_its_form_open_with_the_error() {
        let mut app = inbox_app();
        app.apply(Action::OpenCapture);
        assert!(matches!(input_purpose(&app), Some(InputForm::Capture)));
        let Some(Overlay::Form { purpose, form }) = app.overlay.take() else {
            panic!("expected the capture form");
        };
        let values = [
            FieldValue::Text("  ".to_string()),
            FieldValue::One(String::new()),
            FieldValue::Text(String::new()),
            FieldValue::Text(String::new()),
        ];
        let overlay = app.submitted(purpose, form, &values);
        let Some(Overlay::Form { form, .. }) = &overlay else {
            panic!("expected the form to stay open, got {overlay:?}");
        };
        assert_eq!(form.error.as_deref(), Some("summary is required"));
        assert!(app.take_writes().is_empty());
        assert!(app.undo.is_empty());
    }

    #[test]
    fn a_capture_is_undone_by_withdrawing_the_record_it_created() {
        let mut app = inbox_app();
        app.apply(Action::OpenCapture);
        type_text(&mut app, "watch leaks");
        key(&mut app, KeyCode::Enter);
        let writes = app.take_writes();
        let [WriteRequest::InputAdd { request, .. }] = writes.as_slice() else {
            panic!("expected one input add, got {writes:?}");
        };
        app.apply_written(WriteOutcome {
            request: *request,
            applied: vec!["I9".to_string()],
            ..WriteOutcome::default()
        });
        app.apply(Action::Undo);
        let undo = app.take_writes();
        assert!(
            matches!(undo.as_slice(), [WriteRequest::InputWithdraw { ids, .. }] if ids == &vec!["I9".to_string()]),
            "{undo:?}"
        );
        assert!(app.undo.is_empty());
    }

    #[test]
    fn withdraw_asks_first_and_only_for_the_users_own_new_record() {
        let mut app = inbox_app();
        app.apply(Action::Withdraw);
        assert!(
            app.overlay.is_none(),
            "an agent's question is not withdrawable"
        );
        assert_eq!(
            app.live_notice(Instant::now()),
            Some("only your own new records can be withdrawn")
        );

        app.apply(Action::Move(Dir::Down));
        app.apply(Action::Withdraw);
        assert!(matches!(input_purpose(&app), Some(InputForm::Withdraw(r)) if r.id == "I2"));
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        let writes = app.take_writes();
        assert!(
            matches!(writes.as_slice(), [WriteRequest::InputWithdraw { ids, .. }] if ids == &vec!["I2".to_string()]),
            "{writes:?}"
        );
        assert!(app.undo.is_empty(), "a withdrawal has no undo");

        let mut app = writable_review_app();
        app.apply(Action::Withdraw);
        assert!(app.overlay.is_none(), "w acts only on the Inbox");
    }

    #[test]
    fn a_request_targets_the_marked_rows_and_refuses_a_read_only_ledger() {
        let mut app = writable_review_app();
        mark(&mut app, Surface::Review, &["R1", "R3"]);
        app.apply(Action::OpenRequest);
        let Some(InputForm::Request(selection)) = input_purpose(&app) else {
            panic!("expected the request form, got {:?}", app.overlay);
        };
        let ids: Vec<&str> = selection.rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["R1", "R3"]);

        let mut app = review_app();
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::OpenRequest);
        assert!(app.overlay.is_none(), "a ledger named by path is read-only");
        app.apply(Action::SwitchSurface(Surface::Tasks));
        app.apply(Action::OpenRequest);
        assert!(app.overlay.is_none(), "Tasks has no rows to target");
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
