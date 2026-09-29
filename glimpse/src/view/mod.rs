//! Frame composition: header, the active view, overlays and the key-hint footer.

pub(crate) mod activity;
pub(crate) mod details;
pub(crate) mod ego;
pub(crate) mod header;
pub(crate) mod layers;
pub(crate) mod markdown;
pub(crate) mod selector;

use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Navigator};
use crate::config::{Config, Orientation, ViewKind};
use crate::diagram::{self, DiagramCache};
use crate::transcript::TailState;

/// Share of the body the details and activity panels take when split beside or below the view.
const PANEL_PERCENT: u16 = 40;

/// Draws one frame and installs the active view's navigator into `app`.
///
/// `tail` is only retargeted at the activity agent's transcript; reading it is the
/// caller's job, so a frame never touches the filesystem through it.
pub(crate) fn render(
    frame: &mut Frame,
    app: &mut App,
    config: &Config,
    cache: &mut DiagramCache,
    tail: &mut TailState,
) {
    let area = frame.area();
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(header::height(app)),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    let (view_area, panel_area) = split_body(app, body);
    let orientation_area = view_area.unwrap_or(body);
    let orientation = app.orientation_override.unwrap_or_else(|| {
        config.orientation.resolve(
            orientation_area.width,
            orientation_area.height,
            config.orientation_threshold,
        )
    });
    app.resolved_orientation = orientation;

    if app.activity_open
        && let Some(agent) = activity::agent(app)
    {
        tail.retarget(&agent.transcript_path);
    }

    header::render(frame, head, app);
    if let Some(view_area) = view_area {
        draw_view(frame, view_area, app, orientation, cache);
    }
    if let Some(panel_area) = panel_area {
        draw_panels(frame, panel_area, app, tail);
    }
    frame.render_widget(Paragraph::new(footer(app)), foot);
    if app.selector_open {
        selector::render(frame, area, app);
    }

    let nav = navigator(app, orientation, cache);
    app.nav = Some(nav);
}

/// The view's rect and the panels' rect. Panels sit to the right when the body is at least
/// twice as wide as it is tall, else below; full-screen details leave no room for the view.
fn split_body(app: &App, body: Rect) -> (Option<Rect>, Option<Rect>) {
    let panels = app.details_open || app.activity_open;
    if !panels {
        return (Some(body), None);
    }
    if app.details_open && app.details_fullscreen {
        return (None, Some(body));
    }
    let constraints = [
        Constraint::Percentage(100 - PANEL_PERCENT),
        Constraint::Percentage(PANEL_PERCENT),
    ];
    let [view, panel] = if beside(body) {
        Layout::horizontal(constraints).areas(body)
    } else {
        Layout::vertical(constraints).areas(body)
    };
    (Some(view), Some(panel))
}

fn beside(area: Rect) -> bool {
    u32::from(area.width) >= 2 * u32::from(area.height)
}

fn draw_view(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    orientation: Orientation,
    cache: &mut DiagramCache,
) {
    match app.view {
        ViewKind::Layers => layers::render(frame.buffer_mut(), area, app, orientation),
        ViewKind::Ego => ego::render(frame, area, app, orientation),
        ViewKind::Diagram => {
            diagram::paint::render(frame.buffer_mut(), area, app, orientation, cache);
        }
    }
}

/// Details and activity share the panel rect along its longer axis when both are open.
/// The details scroll is held at the top, since `App` carries no scroll offset.
fn draw_panels(frame: &mut Frame, area: Rect, app: &App, tail: &TailState) {
    let now = SystemTime::now();
    match (app.details_open, app.activity_open) {
        (true, true) => {
            let halves = [Constraint::Percentage(50), Constraint::Percentage(50)];
            let [first, second] = if area.width > area.height {
                Layout::horizontal(halves).areas(area)
            } else {
                Layout::vertical(halves).areas(area)
            };
            details::render(frame, first, app, 0);
            activity::render(frame, second, app, tail, now);
        }
        (true, false) => {
            details::render(frame, area, app, 0);
        }
        (false, true) => activity::render(frame, area, app, tail, now),
        (false, false) => {}
    }
}

fn navigator(app: &App, orientation: Orientation, cache: &mut DiagramCache) -> Box<dyn Navigator> {
    match app.view {
        ViewKind::Layers => layers::navigator(&app.snapshot, &app.index, orientation),
        ViewKind::Ego => Box::new(ego::navigator(app, orientation)),
        ViewKind::Diagram => diagram::navigator(cache, &app.snapshot, &app.index, orientation),
    }
}

fn footer(app: &App) -> Line<'static> {
    let on_off = |flag: bool| if flag { "on" } else { "off" };
    let hints: Vec<(&str, String)> = if app.selector_open {
        vec![
            ("j/k", "move".to_string()),
            ("enter", "switch".to_string()),
            ("a", format!("auto-follow {}", on_off(app.auto_flow))),
            ("esc", "close".to_string()),
        ]
    } else {
        let details = if !app.details_open {
            "details"
        } else if !app.details_fullscreen {
            "full-screen"
        } else {
            "close details"
        };
        vec![
            ("hjkl", "move".to_string()),
            ("tab", "view".to_string()),
            ("o", "orient".to_string()),
            ("enter", details.to_string()),
            ("t", "activity".to_string()),
            ("f", format!("follow {}", on_off(app.follow))),
            ("s", "flows".to_string()),
            ("q", "back".to_string()),
        ]
    };
    let mut spans = Vec::with_capacity(hints.len() * 3);
    for (i, (key, label)) in hints.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", app.theme.pending));
        }
        spans.push(Span::styled(key.to_string(), app.theme.badge));
        spans.push(Span::raw(format!(" {label}")));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Action, Dir};
    use crate::flows::FlowEntry;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app() -> App {
        App::new(fixture(), &Config::default())
    }

    /// Renders one frame at `width` x `height` and returns its rows as text.
    fn draw(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        let mut cache = DiagramCache::default();
        let mut tail = TailState::default();
        terminal
            .draw(|frame| render(frame, app, &Config::default(), &mut cache, &mut tail))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    /// The (row, column) where `needle` first appears, the column counted in cells.
    fn find(rows: &[String], needle: &str) -> Option<(usize, usize)> {
        rows.iter().enumerate().find_map(|(y, row)| {
            row.find(needle)
                .map(|byte| (y, row[..byte].chars().count()))
        })
    }

    #[test]
    fn every_view_renders_the_fixture_and_installs_its_navigator() {
        for (view, marker) in [
            (ViewKind::Layers, "Render the rows"),
            (ViewKind::Ego, "Render the rows"),
            (ViewKind::Diagram, "[4]"),
        ] {
            let mut app = app();
            app.view = view;
            let rows = draw(&mut app, 100, 40);
            let screen = rows.join("\n");
            assert!(
                screen.contains(marker),
                "{view:?} draws {marker}:\n{screen}"
            );
            assert!(app.nav.is_some(), "{view:?} installs a navigator");
            assert!(
                rows.last().is_some_and(|row| row.contains("tab view")),
                "{view:?} ends with the key-hint footer"
            );

            app.apply(Action::Move(Dir::Down));
            assert_ne!(
                app.selected,
                Some(4),
                "{view:?}'s navigator moves off task 4"
            );
        }
    }

    #[test]
    fn orientation_resolves_from_the_view_area_unless_overridden() {
        let mut app = app();
        draw(&mut app, 120, 30);
        assert_eq!(app.resolved_orientation, Orientation::Horizontal);
        draw(&mut app, 60, 50);
        assert_eq!(app.resolved_orientation, Orientation::Vertical);

        app.orientation_override = Some(Orientation::Horizontal);
        draw(&mut app, 60, 50);
        assert_eq!(app.resolved_orientation, Orientation::Horizontal);
    }

    #[test]
    fn details_split_beside_a_wide_body_and_below_a_tall_one() {
        let mut app = app();
        app.apply(Action::Details);

        let wide = draw(&mut app, 120, 30);
        let (row, col) = find(&wide, "#4 Render the rows").expect("details title, wide");
        assert!(row <= 2, "beside: the panel starts at the top, row {row}");
        assert!(col > 60, "beside: the panel is on the right, column {col}");

        let tall = draw(&mut app, 60, 50);
        let (row, col) = find(&tall, "#4 Render the rows").expect("details title, tall");
        assert!(row > 25, "below: the panel is in the lower half, row {row}");
        assert!(col < 5, "below: the panel starts at the left, column {col}");
    }

    #[test]
    fn full_screen_details_replace_the_view() {
        let mut app = app();
        app.view = ViewKind::Diagram;
        app.apply(Action::Details);
        let split = draw(&mut app, 120, 30).join("\n");
        assert!(
            split.contains("[4]"),
            "the diagram shares the body:\n{split}"
        );

        app.apply(Action::Details);
        assert!(app.details_fullscreen);
        let full = draw(&mut app, 120, 30);
        let screen = full.join("\n");
        assert!(!screen.contains("[4]"), "the diagram is hidden:\n{screen}");
        let (_, col) = find(&full, "#4 Render the rows").expect("details title");
        assert!(col < 5, "the panel spans the width, column {col}");
        assert!(
            app.nav.is_some(),
            "moves still work under full-screen details"
        );
    }

    #[test]
    fn the_selector_draws_as_an_overlay() {
        let mut app = app();
        app.flows = vec![FlowEntry {
            slug: "demo-flow".to_string(),
            status: "in-progress".to_string(),
            updated: String::new(),
            plan_path: String::new(),
            tasks_mtime: SystemTime::UNIX_EPOCH,
        }];
        let closed = draw(&mut app, 100, 40).join("\n");
        assert!(!closed.contains("auto-follow"), "{closed}");

        app.apply(Action::ToggleSelector);
        let open = draw(&mut app, 100, 40);
        let (row, _) = find(&open, "flows · auto-follow off").expect("selector title");
        assert!(row > 5 && row < 35, "the overlay is centred, row {row}");
        assert!(open.last().is_some_and(|row| row.contains("switch")));
    }

    #[test]
    fn the_activity_tail_follows_the_selected_agent() {
        let mut app = app();
        let expected = activity::agent(&app)
            .map(|agent| agent.transcript_path.clone())
            .expect("task 4 has a running agent");
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("terminal");
        let mut cache = DiagramCache::default();
        let mut tail = TailState::default();
        terminal
            .draw(|frame| render(frame, &mut app, &Config::default(), &mut cache, &mut tail))
            .expect("draw");
        assert_eq!(tail.path, "", "a closed panel leaves the tail alone");

        app.apply(Action::ToggleActivity);
        terminal
            .draw(|frame| render(frame, &mut app, &Config::default(), &mut cache, &mut tail))
            .expect("draw");
        assert_eq!(tail.path, expected);
    }
}
