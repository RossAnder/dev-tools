//! The terminal event loop and one-shot frame rendering.
//!
//! Three threads feed one channel: the input thread owns the blocking terminal read, the
//! source poller sends snapshots, flow changes, the subscribed ledgers and the activity panel's
//! transcript tail, and the writer thread sends each write's outcome. The main loop drains
//! whatever has queued before drawing once. Whenever
//! the flow or flow-less ledger on show changes, the loop re-subscribes the poller's ledger
//! feeds and drops any read still in flight for a feed it let go. The main loop reads no
//! files. With no running agent and nothing on screen changing over time it blocks without
//! a timeout, so an idle glimpse does no work at all. While a form or the filter prompt is
//! open it takes every key ahead of the key map, and the write requests an input makes go to
//! the writer before the next event is handled.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Instant, SystemTime};

use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    self as term_event, DisableMouseCapture, EnableMouseCapture, Event as TermEvent, KeyCode,
    KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::text::Span;
use ratatui::{DefaultTerminal, Frame, Terminal};
use tomlctl::{LedgerKind, LedgerRef};

use crate::app::{Action, App};
use crate::config::Config;
use crate::diagram::DiagramCache;
use crate::flows::{self, FlowEntry, Scopes};
use crate::keys;
use crate::ledger::{Kind, Ledger, StatusClass};
use crate::model::Snapshot;
use crate::source::{Event, Feed, Source};
use crate::state::State;
use crate::surface::Surface;
use crate::transcript::TailView;
use crate::view;
use crate::writer::{WriteRequest, Writer};

/// What the live view and the one-shot render start from.
pub(crate) struct RunOpts {
    pub(crate) root: PathBuf,
    /// `None` opens the freshest flow and turns auto-flow on.
    pub(crate) slug: Option<String>,
    pub(crate) config: Config,
    /// A config-load problem to show in the header.
    pub(crate) warning: Option<String>,
    /// The view and orientation came from the command line, so saved ones do not apply.
    pub(crate) keep_view: bool,
    pub(crate) keep_orientation: bool,
    /// The surface to open on in place of the saved one.
    pub(crate) surface: Option<Surface>,
}

/// Runs the live view until the user quits.
///
/// Until the first snapshot arrives the app holds an empty one named after the requested
/// slug, so the frame draws its header and footer around an empty view.
pub(crate) fn run(opts: RunOpts) -> Result<(), String> {
    let RunOpts {
        root,
        slug,
        config,
        warning,
        keep_view,
        keep_orientation,
        surface,
    } = opts;
    let placeholder = Snapshot {
        slug: slug.clone().unwrap_or_default(),
        ..Snapshot::default()
    };
    let mut app = App::new(placeholder, &config);
    app.auto_flow = slug.is_none();
    app.warning = warning;
    State::load().apply(&mut app, keep_view, keep_orientation);
    if let Some(surface) = surface {
        app.apply(Action::SwitchSurface(surface));
    }
    if let Some(aspect) = measured_cell_aspect() {
        app.cell_aspect = aspect;
    }

    let (events, rx) = mpsc::channel();
    let writer = Writer::spawn(root.clone(), events.clone());
    // `try_init` installs the panic hook that restores the terminal before anything else runs.
    let mut terminal = ratatui::try_init().map_err(|e| {
        ratatui::restore();
        format!("cannot open the terminal: {e}")
    })?;
    let mouse = config.mouse;
    if mouse {
        capture_mouse();
    }
    spawn_input(events.clone());
    let source = Source::start(root, slug, &config, events);

    let mut screen = Screen::new(app, config, TailView::default());
    let mut host = TerminalHost {
        terminal: &mut terminal,
        source: &source,
        writer: &writer,
    };
    let result = run_loop(&mut screen, &rx, &mut host);
    State::capture(&screen.app).save();
    if mouse {
        release_mouse();
    }
    ratatui::restore();
    source.detach();
    result
}

/// Turns mouse reporting on and chains a panic hook that turns it off again ahead of
/// ratatui's own restore, so a panic cannot leave the shell receiving mouse escapes.
fn capture_mouse() {
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let restore = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        release_mouse();
        restore(info);
    }));
}

fn release_mouse() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
}

/// A cell's height over its width from the terminal's pixel size, when it reports one;
/// many Windows terminals answer zero, and the config's `cell_aspect` then stands.
fn measured_cell_aspect() -> Option<f64> {
    let size = ratatui::crossterm::terminal::window_size().ok()?;
    if size.width == 0 || size.height == 0 || size.columns == 0 || size.rows == 0 {
        return None;
    }
    let cell_w = f64::from(size.width) / f64::from(size.columns);
    let cell_h = f64::from(size.height) / f64::from(size.rows);
    let aspect = cell_h / cell_w;
    (aspect.is_finite() && (0.5..=5.0).contains(&aspect)).then_some(aspect)
}

/// Draws one frame of `snapshot` into an off-screen buffer and returns its rows as plain
/// text, one line per row with trailing blanks trimmed. With a ledger, the frame shows
/// that surface with the ledger read into it.
pub(crate) fn render_once(
    opts: &RunOpts,
    snapshot: Snapshot,
    select: Option<u32>,
    ledger: Option<(Surface, Ledger)>,
    width: u16,
    height: u16,
) -> String {
    let mut app = App::new(snapshot, &opts.config);
    app.warning = opts.warning.clone();
    if select.is_some() {
        app.selected = select;
        app.follow = false;
    }
    if let Some(surface) = opts.surface {
        app.apply(Action::SwitchSurface(surface));
    }
    if let Some((surface, ledger)) = ledger {
        app.apply(Action::SwitchSurface(surface));
        app.apply_ledger(ledger, Instant::now());
    }
    let mut screen = Screen::new(app, opts.config.clone(), TailView::default());
    let Ok(mut terminal) = Terminal::new(TestBackend::new(width, height));
    let Ok(_) = terminal.draw(|frame| screen.render(frame));
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..height {
        let mut row = String::new();
        // A wide glyph's trailing cells are padding the glyph already covers.
        let mut hidden = 0usize;
        for x in 0..width {
            let symbol = buffer[(x, y)].symbol();
            if hidden == 0 {
                row.push_str(symbol);
                hidden = Span::raw(symbol).width().saturating_sub(1);
            } else {
                hidden -= 1;
            }
        }
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out
}

/// Forwards every terminal event onto the channel. The thread owns the blocking read for
/// the whole run: splitting poll and read across threads loses events. It is never
/// joined; it ends with the process, or when the channel closes.
fn spawn_input(events: Sender<Event>) {
    std::thread::spawn(move || {
        loop {
            let event = match term_event::read() {
                Ok(event) => Event::Input(event),
                Err(e) => {
                    let _ = events.send(Event::SourceError(format!("terminal input failed: {e}")));
                    return;
                }
            };
            if events.send(event).is_err() {
                return;
            }
        }
    });
}

/// Everything a frame is drawn from.
struct Screen {
    app: App,
    config: Config,
    cache: DiagramCache,
    tail: TailView,
}

impl Screen {
    fn new(app: App, config: Config, tail: TailView) -> Screen {
        Screen {
            app,
            config,
            cache: DiagramCache::default(),
            tail,
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        view::render(
            frame,
            &mut self.app,
            &self.config,
            &mut self.cache,
            &self.tail,
        );
    }
}

/// The transcript the activity panel shows: its agent's while the panel is open.
fn tail_target(app: &App) -> Option<String> {
    if !app.activity_open {
        return None;
    }
    view::activity::agent(app).map(|agent| agent.transcript_path.clone())
}

/// Asks the poller to tail the panel's transcript when it differs from the one last sent,
/// dropping the old agent's tail so it is never drawn under the new agent.
fn follow_tail(screen: &mut Screen, host: &mut impl Host, sent: &mut Option<String>) {
    let target = tail_target(&screen.app);
    if target != *sent {
        screen.tail = TailView::default();
        host.set_tail(target.clone());
        *sent = target;
    }
}

/// The loop's side effects, kept behind a trait so a test can script them.
trait Host {
    fn draw(&mut self, screen: &mut Screen) -> Result<(), String>;
    fn set_slug(&mut self, slug: String);
    fn subscribe(&mut self, feeds: Vec<Feed>);
    fn set_tail(&mut self, path: Option<String>);
    fn submit(&mut self, request: WriteRequest);
}

struct TerminalHost<'a> {
    terminal: &'a mut DefaultTerminal,
    source: &'a Source,
    writer: &'a Writer,
}

impl Host for TerminalHost<'_> {
    fn draw(&mut self, screen: &mut Screen) -> Result<(), String> {
        self.terminal
            .draw(|frame| screen.render(frame))
            .map(|_| ())
            .map_err(|e| format!("cannot draw: {e}"))
    }

    fn set_slug(&mut self, slug: String) {
        self.source.set_slug(slug);
    }

    fn subscribe(&mut self, feeds: Vec<Feed>) {
        self.source.subscribe(feeds);
    }

    fn set_tail(&mut self, path: Option<String>) {
        self.source.set_tail(path);
    }

    fn submit(&mut self, request: WriteRequest) {
        self.writer.submit(request);
    }
}

enum Step {
    Nothing,
    Redraw,
    Quit,
}

/// What the loop last asked of the poller, so each request goes out only when it changes.
#[derive(Debug, Default)]
struct Pointed {
    /// The freshest flow at the last flow change; auto-flow switches only when it moves.
    freshest: Option<String>,
    /// The slug last sent to the poller, which a snapshot may not have answered yet.
    slug: Option<String>,
    /// The feeds last subscribed; a ledger read for any other is a straggler.
    feeds: Vec<Feed>,
}

impl Pointed {
    /// The poller starts on the app's placeholder slug; an empty one waits for auto-flow.
    fn starting(app: &App) -> Pointed {
        let slug = &app.snapshot.slug;
        Pointed {
            slug: (!slug.is_empty()).then(|| slug.clone()),
            ..Pointed::default()
        }
    }
}

/// Draws, then waits for events and redraws once per batch until a quit or until every
/// sender has gone. Waits carry a timeout only while the app asks for ticks. After each
/// batch the poller is told which transcript to tail, if that changed; the tail itself
/// arrives as [`Event::Tail`].
fn run_loop(
    screen: &mut Screen,
    events: &Receiver<Event>,
    host: &mut impl Host,
) -> Result<(), String> {
    let mut pointed = Pointed::starting(&screen.app);
    let mut tailing: Option<String> = None;
    let mut last_tick = Instant::now();
    resubscribe(&screen.app, host, &mut pointed);
    follow_tail(screen, host, &mut tailing);
    host.draw(screen)?;

    loop {
        let interval = screen.app.tick_interval(Instant::now());
        let first = if let Some(interval) = interval {
            let wait = (last_tick + interval).saturating_duration_since(Instant::now());
            match events.recv_timeout(wait) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        } else {
            match events.recv() {
                Ok(event) => Some(event),
                Err(_) => return Ok(()),
            }
        };

        let mut redraw = false;
        for event in first.into_iter().chain(events.try_iter()) {
            match handle(screen, event, host, &mut pointed) {
                Step::Nothing => {}
                Step::Redraw => redraw = true,
                Step::Quit => return Ok(()),
            }
        }

        let now = Instant::now();
        match interval {
            None => last_tick = now,
            Some(interval) if now.saturating_duration_since(last_tick) >= interval => {
                last_tick = now;
                screen.app.tick(now);
                redraw = true;
            }
            Some(_) => {}
        }
        follow_tail(screen, host, &mut tailing);
        if redraw {
            host.draw(screen)?;
        }
    }
}

fn handle(screen: &mut Screen, event: Event, host: &mut impl Host, pointed: &mut Pointed) -> Step {
    let app = &mut screen.app;
    match event {
        Event::Snapshot(snapshot) => {
            app.apply_snapshot(*snapshot, Instant::now());
            Step::Redraw
        }
        Event::SourceError(message) => {
            app.source_error = Some(message);
            Step::Redraw
        }
        Event::Flows(Ok(entries)) => {
            flows_listed(app, entries, host, pointed);
            redraw_if(app.selector_open)
        }
        Event::Flows(Err(message)) => {
            app.source_error = Some(message);
            Step::Redraw
        }
        Event::FlowMtimes(mtimes) => {
            flow_mtimes_moved(app, &mtimes, host, pointed);
            redraw_if(app.selector_open)
        }
        Event::Ledger { feed, ledger } => ledger_read(app, &feed, ledger, pointed),
        Event::Scopes(scopes) => match scopes.and_then(|value| Scopes::from_value(&value)) {
            Ok(scopes) => {
                app.apply_scopes(scopes);
                redraw_if(app.selector_open)
            }
            Err(message) => {
                app.notice = Some((message, Instant::now()));
                Step::Redraw
            }
        },
        Event::Tail(tail) => {
            if tail_target(app).as_deref() != Some(tail.path.as_str()) {
                return Step::Nothing;
            }
            screen.tail = *tail;
            Step::Redraw
        }
        Event::Input(TermEvent::Key(key)) if app.overlay.is_some() => overlay_input(app, key, host),
        Event::Input(input @ (TermEvent::Key(_) | TermEvent::Mouse(_))) => {
            let action = match input {
                TermEvent::Key(key) => keys::map(key),
                TermEvent::Mouse(mouse) => keys::mouse(mouse, app),
                _ => None,
            };
            let Some(action) = action else {
                return Step::Nothing;
            };
            let handed_back = app.apply(action);
            submit_writes(app, host);
            carry_out(app, handed_back, host, pointed)
        }
        Event::Input(TermEvent::Resize(..)) => {
            // A font change reaches the terminal as a resize, so the cell shape is re-read.
            if let Some(aspect) = measured_cell_aspect() {
                app.cell_aspect = aspect;
            }
            Step::Redraw
        }
        Event::Input(_) => Step::Nothing,
        Event::Written(outcome) => {
            app.apply_written(outcome);
            Step::Redraw
        }
    }
}

fn redraw_if(needed: bool) -> Step {
    if needed { Step::Redraw } else { Step::Nothing }
}

/// Hands a key to the open overlay rather than the key map, so a letter typed into a field
/// never moves or quits. `Ctrl+C` still quits.
fn overlay_input(app: &mut App, key: KeyEvent, host: &mut impl Host) -> Step {
    if key.kind == KeyEventKind::Release {
        return Step::Nothing;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Step::Quit;
    }
    app.overlay_key(key);
    submit_writes(app, host);
    Step::Redraw
}

fn submit_writes(app: &mut App, host: &mut impl Host) {
    for request in app.take_writes() {
        host.submit(request);
    }
}

/// Carries out what [`App::apply`] handed back after an input changed the app.
fn carry_out(
    app: &App,
    handed_back: Option<Action>,
    host: &mut impl Host,
    pointed: &mut Pointed,
) -> Step {
    match handed_back {
        Some(Action::Quit) => Step::Quit,
        Some(Action::SwitchFlow(slug)) => {
            switch_slug(app, slug, host, pointed);
            Step::Redraw
        }
        Some(Action::SwitchScope(..)) => {
            resubscribe(app, host, pointed);
            Step::Redraw
        }
        _ => Step::Redraw,
    }
}

/// Points the poller at `slug` unless it is already there, then re-subscribes the feeds,
/// which a flow-less ledger picked or left changes even when the slug does not.
fn switch_slug(app: &App, slug: String, host: &mut impl Host, pointed: &mut Pointed) {
    if pointed.slug.as_ref() != Some(&slug) {
        host.set_slug(slug.clone());
        pointed.slug = Some(slug);
    }
    resubscribe(app, host, pointed);
}

fn resubscribe(app: &App, host: &mut impl Host, pointed: &mut Pointed) {
    let feeds = feeds_for(app, pointed.slug.as_deref());
    if feeds != pointed.feeds {
        host.subscribe(feeds.clone());
        pointed.feeds = feeds;
    }
}

/// The ledgers of the flow on show, whether picked as a ledger-only flow or as the slug
/// the poller follows, plus the backlog. A picked flow-less ledger stands in for the
/// flow's ledger of its kind; the flow's other ledgers stay live.
fn feeds_for(app: &App, slug: Option<&str>) -> Vec<Feed> {
    let slug = app
        .ledger_flow
        .as_deref()
        .or(slug)
        .filter(|s| !s.is_empty());
    let mut feeds: Vec<Feed> = LedgerKind::ALL
        .into_iter()
        .filter_map(|kind| match (&app.scope, slug) {
            (Some((picked, scope)), _) if *picked == kind => Some(LedgerRef::Scope {
                kind,
                scope: scope.clone(),
            }),
            (_, Some(slug)) => Some(LedgerRef::Flow {
                slug: slug.to_string(),
                kind,
            }),
            (_, None) => None,
        })
        .map(Feed::Ledger)
        .collect();
    feeds.push(Feed::Ledger(LedgerRef::Backlog));
    feeds
}

/// The item surface that lists a feed's ledger.
fn feed_surface(feed: &Feed) -> Option<Surface> {
    let kind = match feed {
        Feed::Ledger(LedgerRef::Flow { kind, .. } | LedgerRef::Scope { kind, .. }) => match kind {
            LedgerKind::Review => Kind::Review,
            LedgerKind::Optimise => Kind::Optimise,
            LedgerKind::PlanReview => Kind::PlanReview,
        },
        Feed::Ledger(LedgerRef::Backlog) => Kind::Backlog,
        Feed::Ledger(LedgerRef::File(_)) | Feed::Inputs => return None,
    };
    Surface::ALL
        .into_iter()
        .find(|surface| surface.ledger_kind() == Some(kind))
}

/// What a surface's header tab shows: its open count once a file is read, and its badge.
fn tab_state(app: &App, surface: Surface) -> (Option<usize>, usize) {
    app.items.get(&surface).map_or((None, 0), |state| {
        let open = matches!(state.revision, Some(Some(_))).then(|| {
            state
                .rows
                .iter()
                .filter(|row| row.class == StatusClass::Live)
                .count()
        });
        (open, state.new_since_view)
    })
}

/// Hands a feed's read to its surface, redrawing only when that surface is on screen or
/// its header tab changed. A read for a feed no longer subscribed is dropped.
fn ledger_read(
    app: &mut App,
    feed: &Feed,
    ledger: Result<Ledger, String>,
    pointed: &Pointed,
) -> Step {
    if !pointed.feeds.contains(feed) {
        return Step::Nothing;
    }
    let Some(surface) = feed_surface(feed) else {
        return Step::Nothing;
    };
    let showing = app.surface == surface;
    match ledger {
        Ok(ledger) => {
            let before = tab_state(app, surface);
            app.apply_ledger(ledger, Instant::now());
            redraw_if(showing || tab_state(app, surface) != before)
        }
        Err(message) if showing => {
            app.notice = Some((message, Instant::now()));
            Step::Redraw
        }
        Err(_) => Step::Nothing,
    }
}

/// Takes a fresh flow list from the poller.
fn flows_listed(
    app: &mut App,
    entries: Vec<FlowEntry>,
    host: &mut impl Host,
    pointed: &mut Pointed,
) {
    app.flows = entries;
    app.selector_cursor = app.selector_cursor.min(app.flows.len().saturating_sub(1));
    follow_freshest(app, host, pointed);
}

/// Re-ranks the listed flows by their new `tasks.toml` mtimes; a slug the list lacks is
/// ignored until the next list.
fn flow_mtimes_moved(
    app: &mut App,
    mtimes: &BTreeMap<String, SystemTime>,
    host: &mut impl Host,
    pointed: &mut Pointed,
) {
    for flow in &mut app.flows {
        if let Some(mtime) = mtimes.get(&flow.slug) {
            flow.tasks_mtime = *mtime;
        }
    }
    flows::rank(&mut app.flows);
    follow_freshest(app, host, pointed);
}

/// With auto-flow on, switches to the freshest flow when it differs from the one at the
/// last flow change.
fn follow_freshest(app: &mut App, host: &mut impl Host, pointed: &mut Pointed) {
    let top = flows::freshest(&app.flows).map(|flow| flow.slug.clone());
    if app.auto_flow
        && top != pointed.freshest
        && let Some(slug) = &top
        && *slug != app.snapshot.slug
    {
        switch_slug(app, slug.clone(), host, pointed);
    }
    pointed.freshest = top;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ViewKind;
    use crate::form::FieldValue;
    use crate::model::fixture;
    use crate::writer::{FakeWriter, WriteOutcome};
    use std::time::Duration;

    fn opts(view: ViewKind) -> RunOpts {
        RunOpts {
            root: PathBuf::new(),
            slug: None,
            config: Config {
                default_view: view,
                ..Config::default()
            },
            warning: None,
            keep_view: false,
            keep_orientation: false,
            surface: None,
        }
    }

    /// Counts draws and records what the loop asked of the outside world.
    #[derive(Default)]
    struct FakeHost {
        draws: usize,
        slugs: Vec<String>,
        tails: Vec<Option<String>>,
        subscriptions: Vec<Vec<Feed>>,
        writer: FakeWriter,
    }

    impl Host for FakeHost {
        fn draw(&mut self, _screen: &mut Screen) -> Result<(), String> {
            self.draws += 1;
            Ok(())
        }

        fn set_slug(&mut self, slug: String) {
            self.slugs.push(slug);
        }

        fn subscribe(&mut self, feeds: Vec<Feed>) {
            self.subscriptions.push(feeds);
        }

        fn set_tail(&mut self, path: Option<String>) {
            self.tails.push(path);
        }

        fn submit(&mut self, request: WriteRequest) {
            self.writer.submit(request);
        }
    }

    /// A screen with nothing that ticks, so the loop blocks on `recv` rather than a timeout.
    fn idle_screen(slug: &str) -> Screen {
        let snapshot = Snapshot {
            slug: slug.to_string(),
            ..Snapshot::default()
        };
        let config = Config::default();
        Screen::new(App::new(snapshot, &config), config, TailView::default())
    }

    fn resize() -> Event {
        Event::Input(TermEvent::Resize(80, 24))
    }

    fn flow(slug: &str, secs: u64) -> FlowEntry {
        FlowEntry {
            slug: slug.to_string(),
            status: "in-progress".to_string(),
            updated: String::new(),
            plan_path: String::new(),
            tasks_mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
        }
    }

    #[test]
    fn render_once_shows_the_slug_and_every_task() {
        let snapshot = fixture();
        let ids: Vec<u32> = snapshot.tasks.iter().map(|task| task.id).collect();
        let text = render_once(&opts(ViewKind::Diagram), snapshot, None, None, 110, 40);
        assert!(text.contains("demo-flow"), "{text}");
        for id in ids {
            assert!(
                text.contains(&format!("[{id}]")),
                "task {id} is drawn:\n{text}"
            );
        }
        assert_eq!(text.lines().count(), 40);
        assert!(!text.contains('\u{1b}'), "no escape sequences");
    }

    #[test]
    fn a_burst_of_resizes_draws_once() {
        let mut screen = idle_screen("demo-flow");
        let mut host = FakeHost::default();
        let (tx, rx) = mpsc::channel();
        for _ in 0..3 {
            tx.send(resize()).expect("send");
        }
        drop(tx);
        run_loop(&mut screen, &rx, &mut host).expect("loop");
        assert_eq!(host.draws, 2, "the first frame, then one for the burst");
    }

    #[test]
    fn the_activity_tail_follows_the_selected_agent() {
        let config = Config::default();
        let mut screen = Screen::new(App::new(fixture(), &config), config, TailView::default());
        let expected = view::activity::agent(&screen.app)
            .map(|agent| agent.transcript_path.clone())
            .expect("task 4 has a running agent");
        let mut host = FakeHost::default();
        let mut tailing = None;
        follow_tail(&mut screen, &mut host, &mut tailing);
        assert!(host.tails.is_empty(), "a closed panel tails nothing");

        screen.app.apply(Action::ToggleActivity);
        follow_tail(&mut screen, &mut host, &mut tailing);
        follow_tail(&mut screen, &mut host, &mut tailing);
        assert_eq!(host.tails, [Some(expected.clone())], "sent once on opening");

        let tail = TailView {
            path: expected.clone(),
            tokens: Some(42),
            ..TailView::default()
        };
        let step = handle(
            &mut screen,
            Event::Tail(Box::new(tail.clone())),
            &mut host,
            &mut Pointed::default(),
        );
        assert!(matches!(step, Step::Redraw));
        assert_eq!(screen.tail, tail, "the poller's tail is drawn");

        let mut stale = tail.clone();
        stale.path = format!("{expected}.other");
        stale.tokens = Some(7);
        let step = handle(
            &mut screen,
            Event::Tail(Box::new(stale)),
            &mut host,
            &mut Pointed::default(),
        );
        assert!(matches!(step, Step::Nothing));
        assert_eq!(
            screen.tail, tail,
            "a tail for another transcript is dropped"
        );

        screen.app.apply(Action::ToggleActivity);
        follow_tail(&mut screen, &mut host, &mut tailing);
        assert_eq!(host.tails, [Some(expected), None], "closing stops the tail");
    }

    #[test]
    fn a_quit_key_ends_the_loop_before_the_batch_draws() {
        let mut screen = idle_screen("demo-flow");
        let mut host = FakeHost::default();
        let (tx, rx) = mpsc::channel();
        tx.send(resize()).expect("send");
        let quit = KeyEvent::from(KeyCode::Char('q'));
        tx.send(Event::Input(TermEvent::Key(quit))).expect("send");
        drop(tx);
        run_loop(&mut screen, &rx, &mut host).expect("loop");
        assert_eq!(host.draws, 1, "only the first frame");
    }

    #[test]
    fn mouse_motion_never_redraws() {
        use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        let mut screen = idle_screen("demo-flow");
        let mut host = FakeHost::default();
        let (tx, rx) = mpsc::channel();
        for column in 0..5 {
            let moved = MouseEvent {
                kind: MouseEventKind::Moved,
                column,
                row: 3,
                modifiers: KeyModifiers::NONE,
            };
            tx.send(Event::Input(TermEvent::Mouse(moved)))
                .expect("send");
        }
        drop(tx);
        run_loop(&mut screen, &rx, &mut host).expect("loop");
        assert_eq!(host.draws, 1, "only the first frame");
    }

    #[test]
    fn a_flow_list_redraws_only_while_the_selector_is_open() {
        let mut screen = idle_screen("demo-flow");
        let mut host = FakeHost::default();
        let listed = || Event::Flows(Ok(vec![flow("a", 1)]));
        let moved = || Event::FlowMtimes(BTreeMap::new());

        screen.app.selector_open = false;
        assert!(matches!(
            handle(&mut screen, listed(), &mut host, &mut Pointed::default()),
            Step::Nothing
        ));
        assert!(matches!(
            handle(&mut screen, moved(), &mut host, &mut Pointed::default()),
            Step::Nothing
        ));

        screen.app.selector_open = true;
        assert!(matches!(
            handle(&mut screen, listed(), &mut host, &mut Pointed::default()),
            Step::Redraw
        ));
        assert!(matches!(
            handle(&mut screen, moved(), &mut host, &mut Pointed::default()),
            Step::Redraw
        ));
    }

    #[test]
    fn auto_flow_switches_only_when_the_freshest_flow_moves() {
        let mut app = idle_screen("").app;
        app.auto_flow = true;
        let mut host = FakeHost::default();
        let mut pointed = Pointed::default();
        let mut flows = vec![flow("old", 1), flow("new", 2)];

        flows_listed(&mut app, flows.clone(), &mut host, &mut pointed);
        assert_eq!(host.slugs, ["new"]);
        assert_eq!(app.flows.len(), 2);

        flows_listed(&mut app, flows.clone(), &mut host, &mut pointed);
        assert_eq!(host.slugs, ["new"], "an unmoved freshest is not re-sent");

        flows.push(flow("newer", 3));
        flows_listed(&mut app, flows.clone(), &mut host, &mut pointed);
        assert_eq!(host.slugs, ["new", "newer"]);

        let mtimes = BTreeMap::from([(
            "old".to_string(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(9),
        )]);
        flow_mtimes_moved(&mut app, &mtimes, &mut host, &mut pointed);
        assert_eq!(
            host.slugs,
            ["new", "newer", "old"],
            "a task store write moves it"
        );
        assert_eq!(app.flows[0].slug, "old", "re-ranked newest first");

        app.auto_flow = false;
        flows.push(flow("newest", 40));
        flows_listed(&mut app, flows, &mut host, &mut pointed);
        assert_eq!(
            host.slugs,
            ["new", "newer", "old"],
            "auto-flow off never switches"
        );
        assert_eq!(
            host.subscriptions.last(),
            Some(&flow_feeds("old")),
            "the feeds follow each switch"
        );
    }

    fn flow_feeds(slug: &str) -> Vec<Feed> {
        let mut feeds: Vec<Feed> = LedgerKind::ALL
            .into_iter()
            .map(|kind| {
                Feed::Ledger(LedgerRef::Flow {
                    slug: slug.to_string(),
                    kind,
                })
            })
            .collect();
        feeds.push(Feed::Ledger(LedgerRef::Backlog));
        feeds
    }

    fn review_feed(slug: &str) -> Feed {
        Feed::Ledger(LedgerRef::Flow {
            slug: slug.to_string(),
            kind: LedgerKind::Review,
        })
    }

    fn review_read(feed: &Feed, revision: &str, items: serde_json::Value) -> Event {
        let ledger = Ledger::from_value(serde_json::json!({
            "kind": "review",
            "path": ".claude/flows/demo-flow/review-ledger.toml",
            "revision": revision,
            "items": items,
        }))
        .expect("ledger");
        Event::Ledger {
            feed: feed.clone(),
            ledger: Ok(ledger),
        }
    }

    fn finding(id: &str, summary: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "summary": summary, "status": "open"})
    }

    #[test]
    fn a_ledger_event_for_a_hidden_surface_does_not_redraw() {
        let mut screen = idle_screen("demo-flow");
        let mut host = FakeHost::default();
        let mut pointed = Pointed::starting(&screen.app);
        resubscribe(&screen.app, &mut host, &mut pointed);
        let feed = review_feed("demo-flow");
        let mut step =
            |screen: &mut Screen, event: Event| handle(screen, event, &mut host, &mut pointed);

        let first = review_read(&feed, "r1", serde_json::json!([finding("R1", "a")]));
        assert!(
            matches!(step(&mut screen, first), Step::Redraw),
            "the tab's count appears"
        );
        let edited = review_read(&feed, "r2", serde_json::json!([finding("R1", "b")]));
        assert!(matches!(step(&mut screen, edited), Step::Nothing));
        assert_eq!(
            screen.app.items[&Surface::Review].rows[0].summary,
            "b",
            "a hidden read still lands"
        );
        let failed = Event::Ledger {
            feed: feed.clone(),
            ledger: Err("torn read".to_string()),
        };
        assert!(matches!(step(&mut screen, failed), Step::Nothing));

        let grown = serde_json::json!([finding("R1", "b"), finding("R2", "c")]);
        assert!(
            matches!(
                step(&mut screen, review_read(&feed, "r3", grown)),
                Step::Redraw
            ),
            "an arrival moves the badge"
        );
        assert_eq!(screen.app.items[&Surface::Review].new_since_view, 1);

        screen.app.apply(Action::SwitchSurface(Surface::Review));
        let shown = serde_json::json!([finding("R1", "d"), finding("R2", "c")]);
        assert!(matches!(
            step(&mut screen, review_read(&feed, "r4", shown)),
            Step::Redraw
        ));

        let straggler = review_read(&review_feed("other"), "r5", serde_json::json!([]));
        assert!(matches!(step(&mut screen, straggler), Step::Nothing));
        assert_eq!(
            screen.app.items[&Surface::Review].rows.len(),
            2,
            "a read for a feed let go is dropped"
        );
    }

    #[test]
    fn switching_flow_resubscribes_its_ledgers() {
        let mut screen = idle_screen("demo-flow");
        let mut host = FakeHost::default();
        let mut pointed = Pointed::starting(&screen.app);
        resubscribe(&screen.app, &mut host, &mut pointed);
        assert_eq!(host.subscriptions, [flow_feeds("demo-flow")]);

        let back = screen.app.apply(Action::SwitchFlow("other".to_string()));
        carry_out(&screen.app, back, &mut host, &mut pointed);
        assert_eq!(host.slugs, ["other"]);
        assert_eq!(host.subscriptions.last(), Some(&flow_feeds("other")));

        let back = screen
            .app
            .apply(Action::SwitchScope(LedgerKind::Review, "loose".to_string()));
        carry_out(&screen.app, back, &mut host, &mut pointed);
        assert_eq!(host.slugs, ["other"], "a flow-less ledger keeps the flow");
        let mut scoped = flow_feeds("other");
        scoped[0] = Feed::Ledger(LedgerRef::Scope {
            kind: LedgerKind::Review,
            scope: "loose".to_string(),
        });
        assert_eq!(host.subscriptions.last(), Some(&scoped));

        let back = screen.app.apply(Action::SwitchFlow("other".to_string()));
        carry_out(&screen.app, back, &mut host, &mut pointed);
        assert_eq!(host.slugs, ["other"], "the poller is already on it");
        assert_eq!(
            host.subscriptions.last(),
            Some(&flow_feeds("other")),
            "leaving the scope re-points the feeds"
        );
        assert_eq!(host.subscriptions.len(), 4);
    }

    /// A screen on the Review surface with R1 and R2 open and the cursor on R1.
    fn review_screen() -> (Screen, FakeHost, Pointed) {
        let mut screen = idle_screen("demo-flow");
        let mut host = FakeHost::default();
        let mut pointed = Pointed::starting(&screen.app);
        resubscribe(&screen.app, &mut host, &mut pointed);
        let rows = serde_json::json!([finding("R1", "a"), finding("R2", "b")]);
        let read = review_read(&review_feed("demo-flow"), "r1", rows);
        handle(&mut screen, read, &mut host, &mut pointed);
        let mut review = (screen, host, pointed);
        press(&mut review, KeyCode::Char('2'));
        assert_eq!(review.0.app.surface, Surface::Review);
        review
    }

    fn press(review: &mut (Screen, FakeHost, Pointed), code: KeyCode) -> Step {
        let (screen, host, pointed) = review;
        let key = Event::Input(TermEvent::Key(KeyEvent::from(code)));
        handle(screen, key, host, pointed)
    }

    fn form_text(screen: &Screen) -> FieldValue {
        let form = screen.app.overlay.as_ref().and_then(|o| o.form());
        form.expect("a form is open").fields[0].value()
    }

    #[test]
    fn typing_q_in_a_form_does_not_quit() {
        let mut review = review_screen();
        press(&mut review, KeyCode::Char('m'));
        press(&mut review, KeyCode::Enter);
        for letter in ['j', 'q'] {
            let step = press(&mut review, KeyCode::Char(letter));
            assert!(matches!(step, Step::Redraw), "{letter} is typed");
        }
        assert_eq!(form_text(&review.0), FieldValue::Text("jq".to_string()));
        assert_eq!(
            review
                .0
                .app
                .current_items()
                .and_then(|s| s.cursor.as_deref()),
            Some("R1"),
            "j never moved the cursor"
        );

        let (screen, host, pointed) = &mut review;
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let step = handle(screen, Event::Input(TermEvent::Key(ctrl_c)), host, pointed);
        assert!(matches!(step, Step::Quit), "Ctrl+C still quits");
    }

    #[test]
    fn submitting_a_form_reaches_the_writer() {
        let mut review = review_screen();
        press(&mut review, KeyCode::Char('m'));
        press(&mut review, KeyCode::Down);
        press(&mut review, KeyCode::Enter);
        for letter in "noise".chars() {
            press(&mut review, KeyCode::Char(letter));
        }
        assert!(
            review.1.writer.submitted.is_empty(),
            "nothing before submit"
        );
        press(&mut review, KeyCode::Enter);
        assert!(review.0.app.overlay.is_none(), "submitting closes the form");

        let submitted = std::mem::take(&mut review.1.writer.submitted);
        let [
            WriteRequest::Transition {
                request,
                ids,
                to,
                fields,
                ..
            },
        ] = submitted.as_slice()
        else {
            panic!("expected one transition, got {submitted:?}");
        };
        assert_eq!(
            (ids.as_slice(), to.as_str()),
            (&["R1".to_string()][..], "wontfix")
        );
        assert_eq!(fields["wontfix_rationale"], "noise");

        let failed = Event::Written(WriteOutcome {
            request: *request,
            error: Some("root mismatch".to_string()),
            ..WriteOutcome::default()
        });
        let (screen, host, pointed) = &mut review;
        assert!(matches!(
            handle(screen, failed, host, pointed),
            Step::Redraw
        ));
        assert!(screen.app.items[&Surface::Review].saving.is_empty());
        assert_eq!(
            screen.app.live_notice(Instant::now()),
            Some("write failed: root mismatch"),
            "the outcome reaches the app"
        );
    }

    #[test]
    fn a_scope_listing_redraws_only_while_the_selector_is_open() {
        let mut screen = idle_screen("demo-flow");
        let mut host = FakeHost::default();
        let listing = || {
            Event::Scopes(Ok(serde_json::json!({
                "flows": [{"slug": "loose", "has_tasks": false, "ledgers": ["review"]}],
                "scopes": [],
            })))
        };
        assert!(matches!(
            handle(&mut screen, listing(), &mut host, &mut Pointed::default()),
            Step::Nothing
        ));
        assert_eq!(screen.app.scopes.ledger_only, ["loose"]);

        screen.app.selector_open = true;
        assert!(matches!(
            handle(&mut screen, listing(), &mut host, &mut Pointed::default()),
            Step::Redraw
        ));

        let failed = Event::Scopes(Err("unreadable".to_string()));
        assert!(matches!(
            handle(&mut screen, failed, &mut host, &mut Pointed::default()),
            Step::Redraw
        ));
        assert_eq!(
            screen.app.live_notice(Instant::now()),
            Some("unreadable"),
            "a failed listing shows as a notice"
        );
    }
}
