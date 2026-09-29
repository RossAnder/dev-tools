//! Frame composition: header, the active view, panels, overlays and the key-hint footer.
//!
//! Density decides where the panels go. Comfortable docks them beside or below the view
//! at `app.panel_percent` of the body; compact gives the view the whole body and draws the
//! panels as a centred modal over it. Each frame records its density, its layers column
//! width, the details scroll bounds and the mouse regions back into `app`.

pub(crate) mod activity;
pub(crate) mod details;
pub(crate) mod ego;
pub(crate) mod header;
pub(crate) mod layers;
pub(crate) mod markdown;
pub(crate) mod selector;

use std::time::{Instant, SystemTime};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::{App, Navigator, Regions};
use crate::config::{Config, Density, Orientation, ViewKind};
use crate::diagram::{self, DiagramCache};
use crate::transcript::TailState;

/// Rows an activity-only modal needs: borders, the status row and every shown entry.
const ACTIVITY_ROWS: u16 = activity::SHOWN_ENTRIES as u16 + 3;

/// Where the panels go this frame.
enum Panels {
    None,
    Docked(Rect),
    Modal(Rect),
}

/// Draws one frame and installs the active view's navigator into `app`.
///
/// `tail` is drawn as it stands: pointing it at the activity agent's transcript and
/// reading it are the caller's job, so a frame never touches the filesystem.
pub(crate) fn render(
    frame: &mut Frame,
    app: &mut App,
    config: &Config,
    cache: &mut DiagramCache,
    tail: &TailState,
) {
    let area = frame.area();
    let density = app.density.resolve(area.width, config.compact_below);
    app.resolved_density = density;
    let compact = density == Density::Compact;
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(header::height(app, compact)),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    let (view_area, panels) = split_body(app, body, density);
    let orientation_area = view_area.unwrap_or(body);
    let orientation = app.orientation_override.unwrap_or_else(|| {
        config.orientation.resolve(
            orientation_area.width,
            orientation_area.height,
            config.orientation_threshold,
        )
    });
    app.resolved_orientation = orientation;
    if app.view == ViewKind::Layers && orientation == Orientation::Horizontal {
        app.resolved_column = layers::column_width(app);
    }

    header::render(frame, head, app, compact);
    let tasks = match view_area {
        Some(view_area) => draw_view(frame, view_area, app, orientation, cache),
        None => Vec::new(),
    };
    let (panel_area, modal) = match panels {
        Panels::None => (None, None),
        Panels::Docked(rect) => (Some(rect), None),
        Panels::Modal(rect) => {
            frame.render_widget(Clear, rect);
            (Some(rect), Some(rect))
        }
    };
    let details = panel_area.and_then(|rect| draw_panels(frame, rect, app, tail, modal.is_some()));
    if let Some((rect, height)) = details {
        let page = Block::bordered().inner(rect).height;
        app.details_page = page;
        app.details_max_scroll = height.saturating_sub(page);
        app.details_scroll = app.details_scroll.min(app.details_max_scroll);
    }
    app.regions = Regions {
        view: view_area,
        details: details.map(|(rect, _)| rect),
        modal,
        tasks,
    };
    frame.render_widget(Paragraph::new(footer(app, Instant::now())), foot);
    if app.selector_open {
        selector::render(frame, area, app);
    }

    let nav = navigator(app, orientation, cache);
    app.nav = Some(nav);
}

/// The view's rect and where the panels go. Compact density leaves the view the whole
/// body under a modal a cell in from each edge; an activity-only modal is only as tall as
/// its entries. Comfortable panels sit to the right when the body is at least twice as
/// wide as it is tall, else below, and full-screen details leave no room for the view.
fn split_body(app: &App, body: Rect, density: Density) -> (Option<Rect>, Panels) {
    let panels = app.details_open || app.activity_open;
    if !panels {
        return (Some(body), Panels::None);
    }
    if density == Density::Compact {
        let width = body.width.saturating_sub(2).max(body.width.min(20));
        let tall = body.height.saturating_sub(2).max(body.height.min(8));
        let height = if app.details_open {
            tall
        } else {
            tall.min(ACTIVITY_ROWS)
        };
        let modal = body.centered(Constraint::Length(width), Constraint::Length(height));
        return (Some(body), Panels::Modal(modal));
    }
    if app.details_open && app.details_fullscreen {
        return (None, Panels::Docked(body));
    }
    let constraints = [
        Constraint::Percentage(100 - app.panel_percent),
        Constraint::Percentage(app.panel_percent),
    ];
    let [view, panel] = if beside(body) {
        Layout::horizontal(constraints).areas(body)
    } else {
        Layout::vertical(constraints).areas(body)
    };
    (Some(view), Panels::Docked(panel))
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
) -> Vec<layers::Target> {
    match app.view {
        ViewKind::Layers => layers::render(frame.buffer_mut(), area, app, orientation),
        ViewKind::Ego => ego::render(frame, area, app, orientation),
        ViewKind::Diagram => {
            diagram::paint::render(frame.buffer_mut(), area, app, orientation, cache);
            Vec::new()
        }
    }
}

/// Details and activity share the panel rect along its longer axis when both are open;
/// in a `modal` activity sits under details and takes only the rows its entries need.
/// Returns the details rect and its wrapped content height, when details are drawn.
fn draw_panels(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    tail: &TailState,
    modal: bool,
) -> Option<(Rect, u16)> {
    let now = SystemTime::now();
    match (app.details_open, app.activity_open) {
        (true, true) => {
            let halves = [Constraint::Percentage(50), Constraint::Percentage(50)];
            let [first, second] = if modal {
                let activity = ACTIVITY_ROWS.min(area.height / 2);
                Layout::vertical([Constraint::Min(0), Constraint::Length(activity)]).areas(area)
            } else if area.width > area.height {
                Layout::horizontal(halves).areas(area)
            } else {
                Layout::vertical(halves).areas(area)
            };
            let height = details::render(frame, first, app, app.details_scroll);
            activity::render(frame, second, app, tail, now);
            Some((first, height))
        }
        (true, false) => {
            let height = details::render(frame, area, app, app.details_scroll);
            Some((area, height))
        }
        (false, true) => {
            activity::render(frame, area, app, tail, now);
            None
        }
        (false, false) => None,
    }
}

fn navigator(app: &App, orientation: Orientation, cache: &mut DiagramCache) -> Box<dyn Navigator> {
    match app.view {
        ViewKind::Layers => layers::navigator(&app.snapshot, &app.index, orientation),
        ViewKind::Ego => Box::new(ego::navigator(app, orientation)),
        ViewKind::Diagram => diagram::navigator(cache, &app.snapshot, &app.index, orientation),
    }
}

/// A live notice replaces the hints. Compact density lists fewer hints, most useful
/// first, since the footer is cut at the pane's edge.
fn footer(app: &App, now: Instant) -> Line<'static> {
    if let Some(notice) = app.live_notice(now) {
        return Line::from(Span::styled(notice.to_string(), app.theme.badge));
    }
    let on_off = |flag: bool| if flag { "on" } else { "off" };
    let compact = app.resolved_density == Density::Compact;
    let hints: Vec<(&str, String)> = if app.selector_open {
        vec![
            ("j/k", "move".to_string()),
            ("enter", "switch".to_string()),
            ("a", format!("auto-follow {}", on_off(app.auto_flow))),
            ("esc", "close".to_string()),
        ]
    } else if app.details_open && compact {
        vec![
            ("J/K", "scroll".to_string()),
            ("hjkl", "move".to_string()),
            ("enter", "close".to_string()),
            ("t", "activity".to_string()),
        ]
    } else if compact {
        vec![
            ("enter", "details".to_string()),
            ("tab", "view".to_string()),
            ("d", "density".to_string()),
            ("q", "back".to_string()),
            ("hjkl", "move".to_string()),
        ]
    } else {
        let details = if !app.details_open {
            "details"
        } else if !app.details_fullscreen {
            "full-screen"
        } else {
            "close details"
        };
        let mut hints = vec![
            ("hjkl", "move".to_string()),
            ("tab", "view".to_string()),
            ("o", "orient".to_string()),
            ("enter", details.to_string()),
        ];
        if app.details_open {
            hints.push(("J/K", "scroll".to_string()));
        }
        hints.extend([
            ("t", "activity".to_string()),
            ("f", format!("follow {}", on_off(app.follow))),
            ("s", "flows".to_string()),
            ("d", "density".to_string()),
            ("q", "back".to_string()),
        ]);
        hints
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
        let tail = TailState::default();
        terminal
            .draw(|frame| render(frame, app, &Config::default(), &mut cache, &tail))
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
    fn a_narrow_pane_draws_compact_and_details_as_a_modal() {
        let mut app = app();
        let closed = draw(&mut app, 45, 55);
        assert_eq!(app.resolved_density, Density::Compact);
        assert!(closed[0].starts_with("demo-flow  3/8"), "{:?}", closed[0]);
        let (row, _) = find(&closed, "Render the rows").expect("the view");
        assert!(
            row < 20,
            "the view starts under a one-row header, row {row}"
        );
        assert!(!app.regions.tasks.is_empty(), "layers rows are clickable");

        app.apply(Action::Details);
        let open = draw(&mut app, 45, 55);
        let (row, col) = find(&open, "#4 Render the rows").expect("modal title");
        assert!(
            row <= 2 && col <= 3,
            "the modal fills the body, at {row},{col}"
        );
        let modal = app.regions.modal.expect("a modal");
        assert_eq!((modal.width, modal.height), (43, 51));
        assert_eq!(app.details_page, 49, "the modal's inner height");
        assert!(open.last().is_some_and(|row| row.contains("J/K scroll")));

        app.apply(Action::Details);
        assert!(!app.details_open, "Enter closes the modal");
    }

    #[test]
    fn a_click_on_an_ego_entry_selects_it() {
        use crate::keys;
        use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let mut app = app();
        app.view = ViewKind::Ego;
        let rows = draw(&mut app, 45, 55);
        let (row, col) = find(&rows, "7 Assemble").expect("a dependent");
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: u16::try_from(col).expect("column"),
            row: u16::try_from(row).expect("row"),
            modifiers: KeyModifiers::NONE,
        };
        let action = keys::mouse(click, &app).expect("the row is a target");
        assert_eq!(action, Action::Select(7));
        app.apply(action);
        assert_eq!(app.selected, Some(7));
    }

    #[test]
    fn density_can_be_pinned_against_the_width() {
        let mut app = app();
        app.density = crate::config::DensityPref::Fixed(Density::Comfortable);
        app.apply(Action::Details);
        let rows = draw(&mut app, 45, 55);
        assert_eq!(app.resolved_density, Density::Comfortable);
        assert!(app.regions.modal.is_none());
        let (row, _) = find(&rows, "#4 Render the rows").expect("docked details");
        assert!(row > 25, "docked below the view, row {row}");

        app.density = crate::config::DensityPref::Fixed(Density::Compact);
        draw(&mut app, 200, 50);
        assert!(app.regions.modal.is_some(), "compact holds on a wide pane");
    }

    #[test]
    fn the_docked_panel_takes_the_adjusted_share() {
        let mut app = app();
        app.apply(Action::Details);
        let col_at = |app: &mut App| {
            let rows = draw(app, 120, 30);
            find(&rows, "#4 Render the rows").expect("details").1
        };
        let default = col_at(&mut app);
        app.apply(Action::ResizePanel(true));
        app.apply(Action::ResizePanel(true));
        let wider = col_at(&mut app);
        assert!(wider + 10 <= default, "{wider} vs {default}");
    }

    #[test]
    fn details_scroll_is_clamped_by_the_frame_and_drawn() {
        let mut app = app();
        app.selected = Some(3);
        app.apply(Action::Details);
        draw(&mut app, 120, 16);
        assert!(app.details_max_scroll > 0, "task 3 overflows a short panel");
        app.apply(Action::ScrollDetails(crate::app::Scroll::Bottom));
        assert_eq!(app.details_scroll, app.details_max_scroll);
        let rows = draw(&mut app, 120, 16);
        let height = app.details_max_scroll + app.details_page;
        let counter = format!(" {}/{height} ", app.details_scroll + 1);
        assert!(rows.iter().any(|row| row.contains(&counter)), "{rows:#?}");
    }

    #[test]
    fn details_split_beside_a_wide_body_and_below_a_tall_one() {
        let mut app = app();
        app.density = crate::config::DensityPref::Fixed(Density::Comfortable);
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
}
