//! The terminal event loop and one-shot frame rendering.
//!
//! Three threads feed one channel: the input thread owns the blocking terminal read, the
//! source poller sends snapshots, flow changes and the activity panel's transcript tail, and
//! the main loop drains whatever has queued before drawing once. The main loop reads no
//! files. With no running agent and nothing on screen changing over time it blocks without
//! a timeout, so an idle glimpse does no work at all.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Instant, SystemTime};

use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    self as term_event, DisableMouseCapture, EnableMouseCapture, Event as TermEvent,
};
use ratatui::crossterm::execute;
use ratatui::text::Span;
use ratatui::{DefaultTerminal, Frame, Terminal};

use crate::app::{Action, App};
use crate::config::Config;
use crate::diagram::DiagramCache;
use crate::flows::{self, FlowEntry};
use crate::keys;
use crate::model::Snapshot;
use crate::source::{Event, Source};
use crate::state::State;
use crate::transcript::TailView;
use crate::view;

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
    } = opts;
    let placeholder = Snapshot {
        slug: slug.clone().unwrap_or_default(),
        ..Snapshot::default()
    };
    let mut app = App::new(placeholder, &config);
    app.auto_flow = slug.is_none();
    app.warning = warning;
    State::load().apply(&mut app, keep_view, keep_orientation);
    if let Some(aspect) = measured_cell_aspect() {
        app.cell_aspect = aspect;
    }

    let (events, rx) = mpsc::channel();
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
/// text, one line per row with trailing blanks trimmed.
pub(crate) fn render_once(
    opts: &RunOpts,
    snapshot: Snapshot,
    select: Option<u32>,
    width: u16,
    height: u16,
) -> String {
    let mut app = App::new(snapshot, &opts.config);
    app.warning = opts.warning.clone();
    if select.is_some() {
        app.selected = select;
        app.follow = false;
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
    fn set_tail(&mut self, path: Option<String>);
}

struct TerminalHost<'a> {
    terminal: &'a mut DefaultTerminal,
    source: &'a Source,
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

    fn set_tail(&mut self, path: Option<String>) {
        self.source.set_tail(path);
    }
}

enum Step {
    Nothing,
    Redraw,
    Quit,
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
    // The freshest flow at the last flow change; auto-flow switches only when it moves.
    let mut freshest: Option<String> = None;
    let mut tailing: Option<String> = None;
    let mut last_tick = Instant::now();
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
            match handle(screen, event, host, &mut freshest) {
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

fn handle(
    screen: &mut Screen,
    event: Event,
    host: &mut impl Host,
    freshest: &mut Option<String>,
) -> Step {
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
            flows_listed(app, entries, host, freshest);
            Step::Redraw
        }
        Event::Flows(Err(message)) => {
            app.source_error = Some(message);
            Step::Redraw
        }
        Event::FlowMtimes(mtimes) => {
            flow_mtimes_moved(app, &mtimes, host, freshest);
            Step::Redraw
        }
        Event::Tail(tail) => {
            if tail_target(app).as_deref() != Some(tail.path.as_str()) {
                return Step::Nothing;
            }
            screen.tail = *tail;
            Step::Redraw
        }
        Event::Input(input @ (TermEvent::Key(_) | TermEvent::Mouse(_))) => {
            let action = match input {
                TermEvent::Key(key) => keys::map(key),
                TermEvent::Mouse(mouse) => keys::mouse(mouse, app),
                _ => None,
            };
            let Some(action) = action else {
                return Step::Nothing;
            };
            match app.apply(action) {
                Some(Action::Quit) => Step::Quit,
                Some(Action::SwitchFlow(slug)) => {
                    host.set_slug(slug);
                    Step::Redraw
                }
                _ => Step::Redraw,
            }
        }
        Event::Input(TermEvent::Resize(..)) => {
            // A font change reaches the terminal as a resize, so the cell shape is re-read.
            if let Some(aspect) = measured_cell_aspect() {
                app.cell_aspect = aspect;
            }
            Step::Redraw
        }
        Event::Input(_) => Step::Nothing,
    }
}

/// Takes a fresh flow list from the poller.
fn flows_listed(
    app: &mut App,
    entries: Vec<FlowEntry>,
    host: &mut impl Host,
    freshest: &mut Option<String>,
) {
    app.flows = entries;
    app.selector_cursor = app.selector_cursor.min(app.flows.len().saturating_sub(1));
    follow_freshest(app, host, freshest);
}

/// Re-ranks the listed flows by their new `tasks.toml` mtimes; a slug the list lacks is
/// ignored until the next list.
fn flow_mtimes_moved(
    app: &mut App,
    mtimes: &BTreeMap<String, SystemTime>,
    host: &mut impl Host,
    freshest: &mut Option<String>,
) {
    for flow in &mut app.flows {
        if let Some(mtime) = mtimes.get(&flow.slug) {
            flow.tasks_mtime = *mtime;
        }
    }
    flows::rank(&mut app.flows);
    follow_freshest(app, host, freshest);
}

/// With auto-flow on, switches to the freshest flow when it differs from the one at the
/// last flow change.
fn follow_freshest(app: &mut App, host: &mut impl Host, freshest: &mut Option<String>) {
    let top = flows::freshest(&app.flows).map(|flow| flow.slug.clone());
    if app.auto_flow
        && top != *freshest
        && let Some(slug) = &top
        && *slug != app.snapshot.slug
    {
        host.set_slug(slug.clone());
    }
    *freshest = top;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ViewKind;
    use crate::model::fixture;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};
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
        }
    }

    /// Counts draws and records what the loop asked of the outside world.
    #[derive(Default)]
    struct FakeHost {
        draws: usize,
        slugs: Vec<String>,
        tails: Vec<Option<String>>,
    }

    impl Host for FakeHost {
        fn draw(&mut self, _screen: &mut Screen) -> Result<(), String> {
            self.draws += 1;
            Ok(())
        }

        fn set_slug(&mut self, slug: String) {
            self.slugs.push(slug);
        }

        fn set_tail(&mut self, path: Option<String>) {
            self.tails.push(path);
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
        let text = render_once(&opts(ViewKind::Diagram), snapshot, None, 110, 40);
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
            &mut None,
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
            &mut None,
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
    fn auto_flow_switches_only_when_the_freshest_flow_moves() {
        let mut app = idle_screen("").app;
        app.auto_flow = true;
        let mut host = FakeHost::default();
        let mut freshest = None;
        let mut flows = vec![flow("old", 1), flow("new", 2)];

        flows_listed(&mut app, flows.clone(), &mut host, &mut freshest);
        assert_eq!(host.slugs, ["new"]);
        assert_eq!(app.flows.len(), 2);

        flows_listed(&mut app, flows.clone(), &mut host, &mut freshest);
        assert_eq!(host.slugs, ["new"], "an unmoved freshest is not re-sent");

        flows.push(flow("newer", 3));
        flows_listed(&mut app, flows.clone(), &mut host, &mut freshest);
        assert_eq!(host.slugs, ["new", "newer"]);

        let mtimes = BTreeMap::from([(
            "old".to_string(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(9),
        )]);
        flow_mtimes_moved(&mut app, &mtimes, &mut host, &mut freshest);
        assert_eq!(
            host.slugs,
            ["new", "newer", "old"],
            "a task store write moves it"
        );
        assert_eq!(app.flows[0].slug, "old", "re-ranked newest first");

        app.auto_flow = false;
        flows.push(flow("newest", 40));
        flows_listed(&mut app, flows, &mut host, &mut freshest);
        assert_eq!(
            host.slugs,
            ["new", "newer", "old"],
            "auto-flow off never switches"
        );
    }
}
