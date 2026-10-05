//! Frame composition: header, the active view, panels, overlays and the key-hint footer.
//!
//! Density decides where the panels go. Comfortable docks them beside or below the view
//! at `app.panel_percent` of the body, and the panel's near border is the drag handle;
//! compact gives the view the whole body and draws the panels as a centred modal over
//! it. Each frame records its density, split, layers column width and scroll, the details
//! scroll bounds and the mouse regions back into `app`.

pub(crate) mod activity;
pub(crate) mod details;
pub(crate) mod ego;
pub(crate) mod form;
pub(crate) mod header;
pub(crate) mod inbox;
pub(crate) mod items;
pub(crate) mod layers;
pub(crate) mod legend;
pub(crate) mod markdown;
pub(crate) mod selector;

use std::time::{Instant, SystemTime};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::actions::Overlay;
use crate::app::{App, Navigator, Regions};
use crate::config::{Config, Density, Orientation, Split, ViewKind, aspect};
use crate::diagram::{self, DiagramCache};
use crate::model::{Task, TaskStatus};
use crate::surface::Surface;
use crate::transcript::TailView;

pub(crate) fn text_width(s: &str) -> usize {
    Span::raw(s).width()
}

/// `s` cut to at most `max` cells, ending in `…` when anything was cut.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if text_width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = text_width(c.encode_utf8(&mut [0; 4]));
        if used + w > max - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// `implement-deep` gives `ID`, `Explore` gives `E`; at most two letters.
pub(crate) fn initials(agent_type: &str) -> String {
    let letters: String = agent_type
        .split(['-', '_', ' '])
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .take(2)
        .collect();
    if letters.is_empty() {
        "?".to_string()
    } else {
        letters
    }
}

/// The first of `view` rows drawn from `len`, keeping a third of the window below `at`.
pub(crate) fn window(at: Option<usize>, len: usize, view: usize) -> usize {
    if view == 0 || len <= view {
        return 0;
    }
    let ahead = view / 3;
    let start = at.map_or(0, |at| (at + ahead + 1).saturating_sub(view));
    start.min(len - view)
}

/// Rows an activity-only modal needs: borders, the status row and every shown entry.
const ACTIVITY_ROWS: u16 = activity::SHOWN_ENTRIES as u16 + 3;

/// Where the panels go this frame.
enum Panels {
    None,
    /// The panel rect, and the divider strip a drag resizes from when the view shares
    /// the body.
    Docked(Rect, Option<Rect>),
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
    tail: &TailView,
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

    let (view_area, panels) = split_body(app, config, body, density);
    let orientation_area = view_area.unwrap_or(body);
    let orientation = app.orientation_override.unwrap_or_else(|| {
        config.orientation.resolve(
            aspect(
                orientation_area.width,
                orientation_area.height,
                app.cell_aspect,
            ),
            config.orientation_threshold,
        )
    });
    app.resolved_orientation = orientation;

    header::render(frame, head, app, compact);
    cache.set_implied(app.show_implied);
    let (tasks, items) = match view_area {
        Some(view_area) => draw_surface(frame, view_area, app, orientation, cache),
        None => (Vec::new(), Vec::new()),
    };
    app.scroll_nudge = (0, 0);
    let (panel_area, modal, divider) = match panels {
        Panels::None => (None, None, None),
        Panels::Docked(rect, divider) => (Some(rect), None, divider),
        Panels::Modal(rect) => {
            frame.render_widget(Clear, rect);
            (Some(rect), Some(rect), None)
        }
    };
    let details = panel_area.and_then(|rect| draw_panels(frame, rect, app, tail, modal.is_some()));
    if app.dragging
        && let Some(edge) = divider
    {
        frame.buffer_mut().set_style(edge, app.theme.selection_mark);
    }
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
        items,
        body,
        divider,
    };
    frame.render_widget(Paragraph::new(footer(app, Instant::now())), foot);
    if app.selector_open {
        selector::render(frame, area, app);
    }
    if app.legend_open {
        legend::render(frame, area, app);
    }
    // The filter prompt is drawn in the item list's facet row, not as a modal.
    if let Some(form) = app.overlay.as_ref().and_then(Overlay::form) {
        form::render(frame, area, form, &app.theme);
    }

    let nav = navigator(app, orientation, cache);
    app.nav = Some(nav);
}

/// The view's rect and where the panels go. Compact density leaves the view the whole
/// body under a modal a cell in from each edge; an activity-only modal is only as tall as
/// its entries. Comfortable panels sit beside a landscape body and below a portrait one
/// (see [`crate::config::SplitPref::resolve`]) unless `|` has pinned them, and
/// full-screen details leave no room for the view.
fn split_body(
    app: &mut App,
    config: &Config,
    body: Rect,
    density: Density,
) -> (Option<Rect>, Panels) {
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
        return (None, Panels::Docked(body, None));
    }
    let split = app.split_override.unwrap_or_else(|| {
        config.panel_split.resolve(
            aspect(body.width, body.height, app.cell_aspect),
            config.panel_split_threshold,
            app.resolved_split,
        )
    });
    app.resolved_split = Some(split);
    let percent = |span: u16| -> u16 {
        let share = u32::from(span) * u32::from(app.panel_percent) / 100;
        u16::try_from(share).unwrap_or(span).min(span)
    };
    match split {
        Split::Beside => {
            let panel = percent(body.width);
            let view = body.width - panel;
            let view_rect = Rect::new(body.x, body.y, view, body.height);
            let panel_rect = Rect::new(body.x + view, body.y, panel, body.height);
            let divider = Rect::new(panel_rect.x, body.y, panel.min(1), body.height);
            (Some(view_rect), Panels::Docked(panel_rect, Some(divider)))
        }
        Split::Below => {
            let panel = percent(body.height);
            let view = body.height - panel;
            let view_rect = Rect::new(body.x, body.y, body.width, view);
            let panel_rect = Rect::new(body.x, body.y + view, body.width, panel);
            let divider = Rect::new(body.x, panel_rect.y, body.width, panel.min(1));
            (Some(view_rect), Panels::Docked(panel_rect, Some(divider)))
        }
    }
}

/// `text` on a filled background with one cell of padding either side; neighbours
/// separate from it with unstyled spans, so the fill stays centred on the text.
pub(crate) fn chip(text: &str, style: ratatui::style::Style) -> Span<'static> {
    Span::styled(format!(" {text} "), style)
}

/// How loud a task's effort mark is: the worst notice its status or record holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Notice {
    /// A deviation, a deferral, an escalated or retried completion, a timed-out check.
    Warning,
    /// A failed status, completion or verification.
    Danger,
}

pub(crate) fn notice(app: &App, task: &Task) -> Option<Notice> {
    let mut worst = (task.status == TaskStatus::Failed).then_some(Notice::Danger);
    for entry in app.index.record_entries(&app.snapshot, task.id) {
        let level = match entry.entry_type.as_str() {
            "deviation" | "deferral" => Some(Notice::Warning),
            "verification" => match entry.outcome.as_str() {
                "fail" => Some(Notice::Danger),
                "timeout" => Some(Notice::Warning),
                _ => None,
            },
            "task-completion" if entry.status == "failed" => Some(Notice::Danger),
            "task-completion" if !entry.escalation_reason.is_empty() || entry.retries > 0 => {
                Some(Notice::Warning)
            }
            _ => None,
        };
        worst = worst.max(level);
    }
    worst
}

/// The effort mark's colour when [`notice`] finds one.
pub(crate) fn notice_style(app: &App, task: &Task) -> Option<ratatui::style::Style> {
    notice(app, task).map(|level| match level {
        Notice::Warning => app.theme.effort_warning,
        Notice::Danger => app.theme.effort_danger,
    })
}

/// The current surface's body: Tasks draws the active view, an item surface its list.
fn draw_surface(
    frame: &mut Frame,
    area: Rect,
    app: &mut App,
    orientation: Orientation,
    cache: &mut DiagramCache,
) -> (Vec<layers::Target>, Vec<items::Target>) {
    match app.surface {
        Surface::Tasks => (draw_view(frame, area, app, orientation, cache), Vec::new()),
        Surface::Inbox => (Vec::new(), inbox::render(frame.buffer_mut(), area, app)),
        Surface::Review | Surface::Optimise | Surface::PlanReview | Surface::Backlog => {
            let targets = items::render(frame.buffer_mut(), area, app);
            if let Some(at) = items::prompt_cursor(area, app) {
                frame.set_cursor_position(at);
            }
            (Vec::new(), targets)
        }
    }
}

fn draw_view(
    frame: &mut Frame,
    area: Rect,
    app: &mut App,
    orientation: Orientation,
    cache: &mut DiagramCache,
) -> Vec<layers::Target> {
    match app.view {
        ViewKind::Layers => {
            let drawn = layers::render(frame.buffer_mut(), area, app, orientation);
            app.layers_scroll = drawn.scroll;
            if let Some(column) = drawn.column {
                app.resolved_column = column;
            }
            drawn.targets
        }
        ViewKind::Ego => ego::render(frame, area, app, orientation),
        ViewKind::Diagram => {
            diagram::paint::render(frame.buffer_mut(), area, app, orientation, cache);
            Vec::new()
        }
    }
}

/// Details and activity share the panel rect side by side when it is landscape, else
/// stacked, a cell apart; in a `modal` activity sits under details and takes only the
/// rows its entries need.
/// Returns the details rect and its wrapped content height, when details are drawn.
fn draw_panels(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    tail: &TailView,
    modal: bool,
) -> Option<(Rect, u16)> {
    let now = SystemTime::now();
    match (app.details_open, app.activity_open) {
        (true, true) => {
            let halves = [
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ];
            let [first, second] = if modal {
                let activity = ACTIVITY_ROWS.min(area.height / 2);
                Layout::vertical([Constraint::Min(0), Constraint::Length(activity)]).areas(area)
            } else if aspect(area.width, area.height, app.cell_aspect) >= 1.0 {
                let [a, _, b] = Layout::horizontal(halves).areas(area);
                [a, b]
            } else {
                let [a, _, b] = Layout::vertical(halves).areas(area);
                [a, b]
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

/// Built after the frame's `set_implied`, so a diagram move lands on the layout drawn.
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
        return Line::from(Span::styled(notice.to_string(), app.theme.notice));
    }
    let on_off = |flag: bool| if flag { "on" } else { "off" };
    let compact = app.resolved_density == Density::Compact;
    let hints: Vec<(&str, String)> = if let Some(overlay) = &app.overlay {
        match overlay {
            Overlay::Prompt { .. } => vec![
                ("enter", "keep filter".to_string()),
                ("esc", "cancel".to_string()),
            ],
            Overlay::Menu { .. } | Overlay::Form { .. } => vec![
                ("tab", "next field".to_string()),
                ("enter", "choose/submit".to_string()),
                ("esc", "cancel".to_string()),
            ],
        }
    } else if app.legend_open {
        vec![("?", "close legend".to_string())]
    } else if app.selector_open {
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
    } else if app.surface != Surface::Tasks {
        surface_hints(app, compact)
    } else if compact {
        vec![
            ("enter", "details".to_string()),
            ("tab", "view".to_string()),
            ("d", "density".to_string()),
            ("q", "back".to_string()),
            ("hjkl", "move".to_string()),
            ("?", "legend".to_string()),
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
            ("enter", details.to_string()),
        ];
        if app.details_open {
            hints.push(("J/K", "scroll".to_string()));
        }
        hints.extend([
            ("?", "legend".to_string()),
            ("o", "orient".to_string()),
            ("|", "split".to_string()),
            ("t", "activity".to_string()),
            ("f", format!("follow {}", on_off(app.follow))),
        ]);
        if app.view == ViewKind::Diagram {
            hints.push(("i", format!("implied {}", on_off(app.show_implied))));
        }
        hints.extend([
            ("s", "flows".to_string()),
            ("d", "density".to_string()),
            ("q", "back".to_string()),
        ]);
        hints
    };
    let mut spans = Vec::with_capacity(hints.len() * 3);
    for (i, (key, label)) in hints.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", app.theme.key_separator));
        }
        spans.push(Span::styled(key.to_string(), app.theme.key));
        spans.push(Span::styled(format!(" {label}"), app.theme.key_label));
    }
    Line::from(spans)
}

/// An item surface's hints; Esc reads "clear marks" while any are set, since that is
/// what it does first.
fn surface_hints(app: &App, compact: bool) -> Vec<(&'static str, String)> {
    if app.surface == Surface::Inbox {
        return inbox_hints(app, compact);
    }
    let Some(state) = app.current_items() else {
        return vec![
            ("1-6", "surface".to_string()),
            ("s", "flows".to_string()),
            ("q", "back".to_string()),
        ];
    };
    let mut hints = vec![
        ("j/k", "move".to_string()),
        ("space", "mark".to_string()),
        ("enter", "details".to_string()),
    ];
    if !state.marks.is_empty() {
        hints.push(("esc", format!("clear {} marks", state.marks.len())));
    }
    hints.push(("m", "actions".to_string()));
    if compact {
        hints.extend([("g", "group".to_string()), ("q", "back".to_string())]);
        return hints;
    }
    if matches!(app.surface, Surface::Review | Surface::Optimise) {
        hints.push(("e", "classify".to_string()));
    }
    hints.extend([
        ("r", "request".to_string()),
        ("n", "capture".to_string()),
        ("u", "undo".to_string()),
        ("/", "filter".to_string()),
        ("V", "mark all".to_string()),
        ("g", "group".to_string()),
        ("S", "sort".to_string()),
        ("c", "closed".to_string()),
        ("1-6", "surface".to_string()),
        ("s", "flows".to_string()),
        ("q", "back".to_string()),
    ]);
    hints
}

/// Enter answers the cursor question and opens details on any other record; `w` shows
/// only while the cursor is on a record the user may withdraw.
fn inbox_hints(app: &App, compact: bool) -> Vec<(&'static str, String)> {
    let inbox = &app.inbox;
    let enter = if inbox.cursor_question().is_some() {
        "answer"
    } else {
        "details"
    };
    let mut hints = vec![("j/k", "move".to_string()), ("enter", enter.to_string())];
    if inbox.cursor_withdrawable().is_some() {
        hints.push(("w", "withdraw".to_string()));
    }
    hints.push(("n", "capture".to_string()));
    hints.push(("c", "closed".to_string()));
    if !compact {
        hints.extend([("1-6", "surface".to_string()), ("s", "flows".to_string())]);
    }
    hints.push(("q", "back".to_string()));
    hints
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
        let tail = TailView::default();
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
    fn a_horizontal_layers_frame_records_the_column_width_it_drew() {
        let mut app = app();
        app.orientation_override = Some(Orientation::Horizontal);
        assert_eq!(app.resolved_column, 40, "column_max until a frame is drawn");
        draw(&mut app, 160, 30);
        assert!(app.resolved_column < 40, "sized to the titles");
        app.apply(crate::app::Action::ResizeColumns(true));
        assert_eq!(app.column_override, Some(app.resolved_column + 4));
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
            row <= 3 && col <= 4,
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
        assert!(
            row <= 4,
            "beside: the panel starts under the header, row {row}"
        );
        assert!(col > 60, "beside: the panel is on the right, column {col}");

        let tall = draw(&mut app, 60, 50);
        let (row, col) = find(&tall, "#4 Render the rows").expect("details title, tall");
        assert!(row > 25, "below: the panel is in the lower half, row {row}");
        assert!(col < 5, "below: the panel starts at the left, column {col}");
    }

    #[test]
    fn tall_cells_dock_below_and_the_divider_drags_the_panel() {
        use crate::keys;
        use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let mouse = |kind, column, row| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        let mut app = app();
        app.density = crate::config::DensityPref::Fixed(Density::Comfortable);
        app.cell_aspect = 2.6;
        app.apply(Action::Details);
        // The screenshot's pane: 160 columns of tall cells read as portrait.
        draw(&mut app, 160, 75);
        assert_eq!(app.resolved_split, Some(Split::Below));
        let divider = app.regions.divider.expect("a divider");
        let view = app.regions.view.expect("a view");
        assert_eq!(
            divider.y,
            view.bottom(),
            "the panel's top border is the handle"
        );
        assert_eq!(divider.height, 1);

        let before = app.panel_percent;
        let press = keys::mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), 10, divider.y),
            &app,
        );
        assert_eq!(press, Some(Action::DragStart));
        app.apply(Action::DragStart);
        let drag = keys::mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), 10, divider.y - 10),
            &app,
        )
        .expect("a drag maps while dragging");
        app.apply(drag);
        assert!(
            app.panel_percent > before,
            "{} vs {before}",
            app.panel_percent
        );
        let release = keys::mouse(mouse(MouseEventKind::Up(MouseButton::Left), 10, 5), &app);
        app.apply(release.expect("the release ends the drag"));
        assert!(!app.dragging);

        app.apply(Action::FlipSplit);
        draw(&mut app, 160, 75);
        assert_eq!(
            app.resolved_split,
            Some(Split::Beside),
            "| pins the other side"
        );
    }

    #[test]
    fn the_legend_overlays_the_frame_and_back_closes_it() {
        let mut app = app();
        app.apply(Action::ToggleLegend);
        let screen = draw(&mut app, 120, 60).join("\n");
        assert!(screen.contains("legend") && screen.contains("shares a file"));
        app.apply(Action::Back);
        assert!(!app.legend_open);
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
    fn an_item_surface_replaces_the_view_and_records_its_rows() {
        use crate::ledger::{ItemRow, Kind, Ledger, StatusClass};

        let mut app = app();
        let row = |id: &str| ItemRow {
            id: id.to_string(),
            status: "open".to_string(),
            class: StatusClass::Live,
            summary: format!("finding {id}"),
            ..ItemRow::default()
        };
        let ledger = Ledger {
            kind: Kind::Review,
            source: tomlctl::LedgerRef::File("review-ledger.toml".into()),
            revision: Some("v1".to_string()),
            rows: vec![row("R1"), row("R2")],
        };
        app.apply_ledger(ledger, Instant::now());
        app.apply(Action::SwitchSurface(Surface::Review));
        let rows = draw(&mut app, 120, 30);
        let screen = rows.join("\n");
        assert!(!screen.contains("Render the rows"), "{screen}");
        let (row, _) = find(&rows, "finding R2").expect("the item list");
        let ids: Vec<&str> = app
            .regions
            .items
            .iter()
            .map(|(_, id)| id.as_str())
            .collect();
        assert_eq!(ids, ["R1", "R2"]);
        assert_eq!(usize::from(app.regions.items[1].0.y), row);
        assert!(app.regions.tasks.is_empty());
        assert!(rows.last().is_some_and(|row| row.contains("space mark")));
        assert!(
            rows.last()
                .is_some_and(|row| row.contains("m actions") && row.contains("e classify")),
            "{:?}",
            rows.last()
        );

        app.apply(Action::SwitchSurface(Surface::Inbox));
        let inbox = draw(&mut app, 120, 30).join("\n");
        assert!(inbox.contains("reading the input store"), "{inbox}");
        assert!(app.regions.items.is_empty());
        app.apply_inputs(
            crate::ledger::Inputs {
                path: ".claude/inputs.toml".to_string(),
                revision: Some("v1".to_string()),
                rows: vec![crate::ledger::InputRow {
                    id: "I1".to_string(),
                    kind: "question".to_string(),
                    author: "review".to_string(),
                    status: "new".to_string(),
                    prompt: "Keep the guard?".to_string(),
                    ..crate::ledger::InputRow::default()
                }],
            },
            Instant::now(),
        );
        let inbox = draw(&mut app, 120, 30);
        let (row, _) = find(&inbox, "Keep the guard?").expect("the question");
        assert_eq!(app.regions.items.len(), 1);
        assert_eq!(usize::from(app.regions.items[0].0.y), row);
        assert!(
            inbox.last().is_some_and(|row| row.contains("enter answer")
                && row.contains("n capture")
                && row.contains("c closed")),
            "{:?}",
            inbox.last()
        );

        app.apply(Action::SwitchSurface(Surface::Tasks));
        let tasks = draw(&mut app, 120, 30);
        assert!(find(&tasks, "Render the rows").is_some());
        assert!(tasks.last().is_some_and(|row| row.contains("tab view")));
    }

    #[test]
    fn an_open_form_draws_over_the_surface_and_takes_the_footer() {
        use crate::ledger::{ItemRow, Kind, Ledger, StatusClass};

        let mut app = app();
        let ledger = Ledger {
            kind: Kind::Review,
            source: tomlctl::LedgerRef::Flow {
                slug: "demo-flow".to_string(),
                kind: tomlctl::LedgerKind::Review,
            },
            revision: Some("v1".to_string()),
            rows: vec![ItemRow {
                id: "R1".to_string(),
                status: "open".to_string(),
                class: StatusClass::Live,
                summary: "finding R1".to_string(),
                ..ItemRow::default()
            }],
        };
        app.apply_ledger(ledger, Instant::now());
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::OpenMenu);
        assert!(app.overlay.is_some(), "the menu opens on the cursor row");
        let rows = draw(&mut app, 120, 30);
        let screen = rows.join("\n");
        assert!(screen.contains("move to"), "{screen}");
        assert!(screen.contains("wontfix"), "{screen}");
        assert!(rows.last().is_some_and(|row| row.contains("esc cancel")));
    }

    #[test]
    fn the_filter_prompt_places_the_terminal_cursor_after_its_text() {
        use crate::ledger::{ItemRow, Kind, Ledger, StatusClass};
        use ratatui::crossterm::event::{KeyCode, KeyEvent};

        let mut app = app();
        let ledger = Ledger {
            kind: Kind::Review,
            source: tomlctl::LedgerRef::File("review-ledger.toml".into()),
            revision: Some("v1".to_string()),
            rows: vec![ItemRow {
                id: "R1".to_string(),
                status: "open".to_string(),
                class: StatusClass::Live,
                ..ItemRow::default()
            }],
        };
        app.apply_ledger(ledger, Instant::now());
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::OpenFilter);
        for c in "R1".chars() {
            app.overlay_key(KeyEvent::from(KeyCode::Char(c)));
        }
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
        let mut cache = DiagramCache::default();
        let tail = TailView::default();
        terminal
            .draw(|frame| render(frame, &mut app, &Config::default(), &mut cache, &tail))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..30)
            .map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        let (row, col) = find(&rows, "/R1").expect("the filter prompt");
        let at = terminal.get_cursor_position().expect("cursor");
        assert_eq!(
            (usize::from(at.y), usize::from(at.x)),
            (row, col + 3),
            "{rows:#?}"
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
