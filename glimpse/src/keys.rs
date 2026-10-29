//! Mapping from key and mouse events to application actions.
//!
//! The key mapping is context-free: `j` is always a move and `Enter` always
//! details. [`App::apply`](crate::app::App::apply) decides what an action
//! means while an overlay is open. The mouse mapping reads the regions the
//! last frame recorded, since a click means nothing without them.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};

use crate::app::{Action, App, Dir, Scroll};
use crate::config::{Orientation, ViewKind};
use crate::surface::Surface;

/// Rows one wheel notch scrolls the details panel, the layer list or the diagram.
const WHEEL_ROWS: u16 = 3;

/// `Release` events are dropped: Windows reports both edges of every key,
/// which would otherwise apply each action twice.
pub(crate) fn map(event: KeyEvent) -> Option<Action> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let mods = event.modifiers;
    if mods.contains(KeyModifiers::CONTROL) {
        return matches!(event.code, KeyCode::Char('c')).then_some(Action::Quit);
    }
    if mods.contains(KeyModifiers::ALT) {
        return None;
    }
    let shift = mods.contains(KeyModifiers::SHIFT);
    let action = match event.code {
        KeyCode::Char('j') | KeyCode::Down => Action::Move(Dir::Down),
        KeyCode::Char('k') | KeyCode::Up => Action::Move(Dir::Up),
        KeyCode::Char('h') | KeyCode::Left => Action::Move(Dir::Left),
        KeyCode::Char('l') | KeyCode::Right => Action::Move(Dir::Right),
        // Shift+j and Shift+k arrive as the capital letter, with or without SHIFT set.
        KeyCode::Char('J') => Action::ScrollDetails(Scroll::Down(1)),
        KeyCode::Char('K') => Action::ScrollDetails(Scroll::Up(1)),
        KeyCode::PageDown if shift => Action::ScrollDetails(Scroll::PageDown),
        KeyCode::PageUp if shift => Action::ScrollDetails(Scroll::PageUp),
        KeyCode::Home if shift => Action::ScrollDetails(Scroll::Top),
        KeyCode::End if shift => Action::ScrollDetails(Scroll::Bottom),
        KeyCode::PageDown => Action::Page(Scroll::PageDown),
        KeyCode::PageUp => Action::Page(Scroll::PageUp),
        KeyCode::Home => Action::Page(Scroll::Top),
        KeyCode::End => Action::Page(Scroll::Bottom),
        // Some terminals send Shift+Tab as `Tab` with SHIFT rather than `BackTab`.
        KeyCode::Tab if shift => Action::PrevView,
        KeyCode::Tab => Action::NextView,
        KeyCode::BackTab => Action::PrevView,
        KeyCode::Enter => Action::Details,
        KeyCode::Char('o') => Action::FlipOrientation,
        KeyCode::Char('|') => Action::FlipSplit,
        KeyCode::Char('i') => Action::ToggleImplied,
        KeyCode::Char('F') => Action::CycleFocus,
        KeyCode::Char('z') => Action::Zoom(false),
        KeyCode::Char('Z') => Action::Zoom(true),
        KeyCode::Char('x') => Action::ToggleFit,
        KeyCode::Char('?') => Action::ToggleLegend,
        KeyCode::Char('f') => Action::ToggleFollow,
        KeyCode::Char('s') => Action::ToggleSelector,
        KeyCode::Char('a') => Action::ToggleAutoFlow,
        KeyCode::Char('t') => Action::ToggleActivity,
        KeyCode::Char('d') => Action::CycleDensity,
        KeyCode::Char('H') => Action::CycleHints,
        KeyCode::Char('y') => Action::CopyId,
        KeyCode::Char('v') => Action::ToggleCapture,
        KeyCode::Char('[') => Action::ResizePanel(false),
        KeyCode::Char(']') => Action::ResizePanel(true),
        KeyCode::Char('-') => Action::ResizeColumns(false),
        KeyCode::Char('=' | '+') => Action::ResizeColumns(true),
        KeyCode::Char('q') | KeyCode::Esc => Action::Back,
        KeyCode::Char(digit @ '1'..='6') => Action::SwitchSurface(Surface::from_digit(digit)?),
        KeyCode::Char(' ') => Action::ToggleMark,
        KeyCode::Char('V') => Action::MarkVisible,
        KeyCode::Char('g') => Action::CycleGroup,
        KeyCode::Char('S') => Action::CycleSort,
        KeyCode::Char('D') => Action::FlipSortDir,
        KeyCode::Char('C') => Action::OpenColumns,
        KeyCode::Char('T') => Action::ToggleThemes,
        KeyCode::Char('c') => Action::ToggleClosed,
        KeyCode::Char('m') => Action::OpenMenu,
        KeyCode::Char('e') => Action::OpenClassify,
        KeyCode::Char('P') => Action::ToggleQueue,
        KeyCode::Char('<') => Action::QueueUp,
        KeyCode::Char('>') => Action::QueueDown,
        KeyCode::Char('u') => Action::Undo,
        KeyCode::Char('/') => Action::OpenFilter,
        KeyCode::Char('n') => Action::OpenCapture,
        KeyCode::Char('r') => Action::OpenRequest,
        KeyCode::Char('w') => Action::Withdraw,
        _ => return None,
    };
    Some(action)
}

/// The wheel scrolls the details panel or the view under the pointer, never the
/// selection; a left press on a header tab switches to its surface, one on a table header
/// sorts by its column, one on an item's id cell copies the id, one elsewhere on a task or
/// item row selects it, one on or beside the divider starts a drag, and one on diagram
/// background starts a pan. Ctrl+wheel over the diagram zooms it. Only the press
/// edge of a click acts, the mouse counterpart of dropping key `Release`. Every other
/// kind, including the motion reports capture turns on, maps to nothing. With a form or
/// the filter prompt up, nothing reacts; with the legend up the wheel scrolls it and
/// with the theme picker or the selector up the wheel moves its cursor, and nothing else
/// reacts; with the
/// compact modal up, nothing in the body outside the details panel does.
///
/// During a drag or a pan, left-button motion, bare motion and a re-sent left press all
/// move it, since a touch hold-drag need not report as a left-button drag; any
/// release, or a right or middle press, ends it.
pub(crate) fn mouse(event: MouseEvent, app: &App) -> Option<Action> {
    if app.dragging {
        return held(event, Action::DragTo, Action::DragEnd);
    }
    if app.pan.is_some() {
        return held(event, Action::PanTo, Action::PanEnd);
    }
    if app.overlay.is_some() {
        return None;
    }
    if app.legend_open {
        return match event.kind {
            MouseEventKind::ScrollDown => Some(Action::ScrollDetails(Scroll::Down(WHEEL_ROWS))),
            MouseEventKind::ScrollUp => Some(Action::ScrollDetails(Scroll::Up(WHEEL_ROWS))),
            _ => None,
        };
    }
    if app.theme_picker.is_some() {
        return match event.kind {
            MouseEventKind::ScrollDown => Some(Action::Move(Dir::Down)),
            MouseEventKind::ScrollUp => Some(Action::Move(Dir::Up)),
            _ => None,
        };
    }
    if app.selector_open {
        return match event.kind {
            MouseEventKind::ScrollDown => Some(Action::SelectorNext),
            MouseEventKind::ScrollUp => Some(Action::SelectorPrev),
            _ => None,
        };
    }
    let at = Position::new(event.column, event.row);
    let regions = &app.regions;
    if event.kind == MouseEventKind::Down(MouseButton::Left) {
        if let Some((_, surface)) = regions.tabs.iter().find(|(rect, _)| rect.contains(at)) {
            return Some(Action::SwitchSurface(*surface));
        }
        if regions.modal.is_none() && regions.divider.is_some_and(|r| grabs_divider(r, at)) {
            return Some(Action::DragStart);
        }
    }
    let over_details = regions.details.is_some_and(|r| r.contains(at));
    let over_view = regions.modal.is_none() && regions.view.is_some_and(|r| r.contains(at));
    let over_diagram = over_view && app.surface == Surface::Tasks && app.view == ViewKind::Diagram;
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) if regions.modal.is_none() => {
            let task = || {
                regions
                    .tasks
                    .iter()
                    .find(|(rect, _)| over_view && rect.contains(at))
                    .map(|(_, id)| Action::Select(*id))
            };
            hit(&regions.headers, at)
                .map(|key| Action::SortBy(*key))
                .or_else(|| hit(&regions.ids, at).map(|id| Action::CopyItem(id.clone())))
                .or_else(|| hit(&regions.items, at).map(|id| Action::SelectItem(id.clone())))
                .or_else(task)
                .or_else(|| over_diagram.then_some(Action::PanStart(event.column, event.row)))
        }
        MouseEventKind::ScrollUp if over_diagram && ctrl => Some(Action::Zoom(true)),
        MouseEventKind::ScrollDown if over_diagram && ctrl => Some(Action::Zoom(false)),
        MouseEventKind::ScrollDown if over_details => {
            Some(Action::ScrollDetails(Scroll::Down(WHEEL_ROWS)))
        }
        MouseEventKind::ScrollUp if over_details => {
            Some(Action::ScrollDetails(Scroll::Up(WHEEL_ROWS)))
        }
        MouseEventKind::ScrollDown if over_view => scroll_view(app, 0, 1),
        MouseEventKind::ScrollUp if over_view => scroll_view(app, 0, -1),
        MouseEventKind::ScrollRight if over_view => scroll_view(app, 1, 0),
        MouseEventKind::ScrollLeft if over_view => scroll_view(app, -1, 0),
        _ => None,
    }
}

/// An event during a divider drag or a pan: motion maps to `to` at the pointer, and a
/// release or a non-left press to `end`.
fn held(event: MouseEvent, to: fn(u16, u16) -> Action, end: Action) -> Option<Action> {
    match event.kind {
        MouseEventKind::Drag(MouseButton::Left)
        | MouseEventKind::Moved
        | MouseEventKind::Down(MouseButton::Left) => Some(to(event.column, event.row)),
        MouseEventKind::Up(_) | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle) => {
            Some(end)
        }
        _ => None,
    }
}

/// The key of the first rect in `targets` that holds `at`.
fn hit<T>(targets: &[(Rect, T)], at: Position) -> Option<&T> {
    targets
        .iter()
        .find(|(rect, _)| rect.contains(at))
        .map(|(_, key)| key)
}

/// Whether `at` lies on the divider strip or one cell either side of it, measured
/// across the strip, so a press a cell off the one-cell edge still grabs it.
fn grabs_divider(divider: Rect, at: Position) -> bool {
    let zone = if divider.width <= divider.height {
        let left = divider.x.saturating_sub(1);
        let right = divider.right().saturating_add(1);
        Rect::new(left, divider.y, right - left, divider.height)
    } else {
        let top = divider.y.saturating_sub(1);
        let bottom = divider.bottom().saturating_add(1);
        Rect::new(divider.x, top, divider.width, bottom - top)
    };
    zone.contains(at)
}

/// One wheel notch over the view, as `(across, down)` in notches. Horizontal layers
/// scroll a whole layer per notch whichever way the wheel turns; the traversal view
/// fits its area and takes no wheel; an item list or the Inbox scrolls only down and up.
fn scroll_view(app: &App, across: i32, down: i32) -> Option<Action> {
    let rows = i32::from(WHEEL_ROWS);
    if app.surface != Surface::Tasks {
        return (down != 0).then_some(Action::ScrollView(0, down * rows));
    }
    match (app.view, app.resolved_orientation) {
        (ViewKind::Ego, _) => None,
        (ViewKind::Layers, Orientation::Horizontal) => Some(Action::ScrollView(across + down, 0)),
        _ => Some(Action::ScrollView(across * rows, down * rows)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Regions;
    use crate::config::Config;
    use crate::model::fixture;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn release_events_are_ignored() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(map(release), None);
        assert_eq!(
            map(press(KeyCode::Char('j'))),
            Some(Action::Move(Dir::Down))
        );
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Char('j'), KeyModifiers::NONE, KeyEventKind::Repeat);
        assert_eq!(map(repeat), Some(Action::Move(Dir::Down)));
    }

    #[test]
    fn queue_keys_map_and_b_stays_free() {
        assert_eq!(map(press(KeyCode::Char('P'))), Some(Action::ToggleQueue));
        assert_eq!(map(press(KeyCode::Char('<'))), Some(Action::QueueUp));
        assert_eq!(map(press(KeyCode::Char('>'))), Some(Action::QueueDown));
        // Some terminals set SHIFT on these; the character alone decides.
        let shifted = KeyEvent::new(KeyCode::Char('>'), KeyModifiers::SHIFT);
        assert_eq!(map(shifted), Some(Action::QueueDown));
        assert_eq!(map(press(KeyCode::Char('b'))), None);
    }

    #[test]
    fn tab_and_backtab_cycle_views() {
        assert_eq!(map(press(KeyCode::Tab)), Some(Action::NextView));
        assert_eq!(map(press(KeyCode::BackTab)), Some(Action::PrevView));
        assert_eq!(
            map(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)),
            Some(Action::PrevView)
        );
    }

    #[test]
    fn vim_keys_and_arrows_agree() {
        for (letter, arrow, dir) in [
            ('j', KeyCode::Down, Dir::Down),
            ('k', KeyCode::Up, Dir::Up),
            ('h', KeyCode::Left, Dir::Left),
            ('l', KeyCode::Right, Dir::Right),
        ] {
            assert_eq!(map(press(KeyCode::Char(letter))), Some(Action::Move(dir)));
            assert_eq!(map(press(arrow)), Some(Action::Move(dir)));
        }
    }

    #[test]
    fn letters_map_to_their_toggles() {
        let cases = [
            (KeyCode::Enter, Action::Details),
            (KeyCode::Char('o'), Action::FlipOrientation),
            (KeyCode::Char('f'), Action::ToggleFollow),
            (KeyCode::Char('s'), Action::ToggleSelector),
            (KeyCode::Char('a'), Action::ToggleAutoFlow),
            (KeyCode::Char('t'), Action::ToggleActivity),
            (KeyCode::Char('d'), Action::CycleDensity),
            (KeyCode::Char('H'), Action::CycleHints),
            (KeyCode::Char('y'), Action::CopyId),
            (KeyCode::Char('v'), Action::ToggleCapture),
            (KeyCode::Char('['), Action::ResizePanel(false)),
            (KeyCode::Char(']'), Action::ResizePanel(true)),
            (KeyCode::Char('-'), Action::ResizeColumns(false)),
            (KeyCode::Char('='), Action::ResizeColumns(true)),
            (KeyCode::Char('+'), Action::ResizeColumns(true)),
            (KeyCode::Char('q'), Action::Back),
            (KeyCode::Esc, Action::Back),
            (KeyCode::Char('F'), Action::CycleFocus),
            (KeyCode::Char('z'), Action::Zoom(false)),
            (KeyCode::Char('Z'), Action::Zoom(true)),
            (KeyCode::Char('x'), Action::ToggleFit),
        ];
        for (code, action) in cases {
            assert_eq!(map(press(code)), Some(action), "{code:?}");
        }
        assert_eq!(map(press(KeyCode::Char('b'))), None);
    }

    #[test]
    fn scroll_keys_do_not_collide_with_moves() {
        let shifted = |code| KeyEvent::new(code, KeyModifiers::SHIFT);
        let cases = [
            (press(KeyCode::Char('J')), Scroll::Down(1)),
            (press(KeyCode::Char('K')), Scroll::Up(1)),
            (shifted(KeyCode::Char('J')), Scroll::Down(1)),
            (shifted(KeyCode::PageDown), Scroll::PageDown),
            (shifted(KeyCode::PageUp), Scroll::PageUp),
            (shifted(KeyCode::Home), Scroll::Top),
            (shifted(KeyCode::End), Scroll::Bottom),
        ];
        for (key, scroll) in cases {
            assert_eq!(map(key), Some(Action::ScrollDetails(scroll)), "{key:?}");
        }
    }

    #[test]
    fn paging_keys_page_whatever_has_focus() {
        let cases = [
            (KeyCode::PageDown, Action::Page(Scroll::PageDown)),
            (KeyCode::PageUp, Action::Page(Scroll::PageUp)),
            (KeyCode::Home, Action::Page(Scroll::Top)),
            (KeyCode::End, Action::Page(Scroll::Bottom)),
        ];
        for (code, action) in cases {
            assert_eq!(map(press(code)), Some(action), "{code:?}");
        }
    }

    #[test]
    fn control_c_quits_and_other_chords_are_ignored() {
        assert_eq!(
            map(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(
            map(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            map(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT)),
            None
        );
    }

    fn event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn app_with_regions() -> App {
        let mut app = App::new(fixture(), &Config::default());
        app.regions = Regions {
            view: Some(Rect::new(0, 2, 60, 20)),
            details: Some(Rect::new(60, 2, 40, 20)),
            modal: None,
            tasks: vec![(Rect::new(0, 3, 60, 1), 1), (Rect::new(0, 4, 60, 1), 2)],
            ..Regions::default()
        };
        app
    }

    #[test]
    fn the_wheel_scrolls_whatever_is_under_the_pointer() {
        let app = app_with_regions();
        assert_eq!(
            mouse(event(MouseEventKind::ScrollDown, 70, 10), &app),
            Some(Action::ScrollDetails(Scroll::Down(WHEEL_ROWS)))
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollUp, 70, 10), &app),
            Some(Action::ScrollDetails(Scroll::Up(WHEEL_ROWS)))
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollDown, 10, 10), &app),
            Some(Action::ScrollView(0, i32::from(WHEEL_ROWS))),
            "the wheel scrolls the list, not the selection"
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollDown, 10, 0), &app),
            None,
            "the header is neither"
        );
    }

    #[test]
    fn the_wheel_scrolls_layers_across_and_skips_the_traversal_view() {
        let mut app = app_with_regions();
        app.resolved_orientation = Orientation::Horizontal;
        let at = |kind| mouse(event(kind, 10, 10), &app);
        assert_eq!(
            at(MouseEventKind::ScrollDown),
            Some(Action::ScrollView(1, 0))
        );
        assert_eq!(
            at(MouseEventKind::ScrollUp),
            Some(Action::ScrollView(-1, 0))
        );
        app.view = ViewKind::Diagram;
        let at = |kind| mouse(event(kind, 10, 10), &app);
        assert_eq!(
            at(MouseEventKind::ScrollRight),
            Some(Action::ScrollView(i32::from(WHEEL_ROWS), 0))
        );
        app.view = ViewKind::Ego;
        assert_eq!(mouse(event(MouseEventKind::ScrollDown, 10, 10), &app), None);
    }

    #[test]
    fn a_left_press_on_a_row_selects_it_and_nothing_else_acts() {
        let app = app_with_regions();
        let left = MouseEventKind::Down(MouseButton::Left);
        assert_eq!(mouse(event(left, 5, 4), &app), Some(Action::Select(2)));
        assert_eq!(mouse(event(left, 5, 12), &app), None, "no row there");
        for kind in [
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Right),
        ] {
            assert_eq!(mouse(event(kind, 5, 4), &app), None, "{kind:?}");
        }
    }

    #[test]
    fn a_drag_survives_a_resent_press_and_bare_motion() {
        let mut app = app_with_regions();
        app.dragging = true;
        for kind in [
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Moved,
        ] {
            assert_eq!(
                mouse(event(kind, 30, 7), &app),
                Some(Action::DragTo(30, 7)),
                "{kind:?}"
            );
        }
        for kind in [
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Middle),
        ] {
            assert_eq!(
                mouse(event(kind, 30, 7), &app),
                Some(Action::DragEnd),
                "{kind:?}"
            );
        }
        assert_eq!(mouse(event(MouseEventKind::ScrollDown, 30, 7), &app), None);
    }

    #[test]
    fn a_press_a_cell_either_side_of_the_divider_starts_a_drag() {
        let mut app = app_with_regions();
        let left = MouseEventKind::Down(MouseButton::Left);
        app.regions.divider = Some(Rect::new(60, 2, 1, 20));
        for column in [59, 60, 61] {
            assert_eq!(
                mouse(event(left, column, 10), &app),
                Some(Action::DragStart),
                "column {column}"
            );
        }
        assert_ne!(mouse(event(left, 62, 10), &app), Some(Action::DragStart));
        assert_ne!(mouse(event(left, 60, 22), &app), Some(Action::DragStart));

        app.regions.divider = Some(Rect::new(0, 15, 100, 1));
        for row in [14, 15, 16] {
            assert_eq!(
                mouse(event(left, 30, row), &app),
                Some(Action::DragStart),
                "row {row}"
            );
        }
        assert_ne!(mouse(event(left, 30, 17), &app), Some(Action::DragStart));
    }

    fn diagram_app() -> App {
        let mut app = app_with_regions();
        app.view = ViewKind::Diagram;
        app
    }

    #[test]
    fn ctrl_and_the_wheel_over_the_diagram_zoom_it() {
        let mut app = diagram_app();
        let ctrl = |kind, column| MouseEvent {
            modifiers: KeyModifiers::CONTROL,
            ..event(kind, column, 10)
        };
        assert_eq!(
            mouse(ctrl(MouseEventKind::ScrollUp, 10), &app),
            Some(Action::Zoom(true))
        );
        assert_eq!(
            mouse(ctrl(MouseEventKind::ScrollDown, 10), &app),
            Some(Action::Zoom(false))
        );
        assert_eq!(
            mouse(ctrl(MouseEventKind::ScrollDown, 70), &app),
            Some(Action::ScrollDetails(Scroll::Down(WHEEL_ROWS))),
            "over the details panel"
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollUp, 10, 10), &app),
            Some(Action::ScrollView(0, -i32::from(WHEEL_ROWS))),
            "without ctrl"
        );
        app.view = ViewKind::Layers;
        assert_ne!(
            mouse(ctrl(MouseEventKind::ScrollUp, 10), &app),
            Some(Action::Zoom(true)),
            "the layers view has no zoom"
        );
    }

    #[test]
    fn a_press_on_diagram_background_then_a_drag_pans_it() {
        let mut app = diagram_app();
        let left = MouseEventKind::Down(MouseButton::Left);
        let press = mouse(event(left, 30, 15), &app);
        assert_eq!(press, Some(Action::PanStart(30, 15)));
        app.apply(press.expect("a pan"));
        for kind in [
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::Down(MouseButton::Left),
        ] {
            assert_eq!(
                mouse(event(kind, 33, 13), &app),
                Some(Action::PanTo(33, 13)),
                "{kind:?}"
            );
        }
        app.apply(Action::PanTo(33, 13));
        assert_eq!(app.scroll_nudge, (-3, 2), "the view follows the pointer");
        assert!(app.scroll_pinned);
        let up = mouse(event(MouseEventKind::Up(MouseButton::Left), 33, 13), &app);
        assert_eq!(up, Some(Action::PanEnd));
        app.apply(Action::PanEnd);
        assert_eq!(mouse(event(MouseEventKind::Moved, 40, 13), &app), None);

        app.view = ViewKind::Layers;
        assert_eq!(mouse(event(left, 30, 15), &app), None, "layers do not pan");
    }

    #[test]
    fn a_press_on_a_diagram_node_selects_it_and_the_divider_still_drags() {
        let mut app = diagram_app();
        let left = MouseEventKind::Down(MouseButton::Left);
        assert_eq!(mouse(event(left, 5, 4), &app), Some(Action::Select(2)));
        app.regions.divider = Some(Rect::new(59, 2, 1, 20));
        assert_eq!(
            mouse(event(left, 58, 10), &app),
            Some(Action::DragStart),
            "the divider zone overlaps the view's edge"
        );
    }

    #[test]
    fn digits_switch_surfaces_and_space_marks() {
        for (digit, surface) in [
            ('1', Surface::Tasks),
            ('2', Surface::Review),
            ('3', Surface::Optimise),
            ('4', Surface::PlanReview),
            ('5', Surface::Backlog),
            ('6', Surface::Inbox),
        ] {
            assert_eq!(
                map(press(KeyCode::Char(digit))),
                Some(Action::SwitchSurface(surface))
            );
        }
        assert_eq!(map(press(KeyCode::Char('7'))), None);
        let cases = [
            (' ', Action::ToggleMark),
            ('V', Action::MarkVisible),
            ('g', Action::CycleGroup),
            ('S', Action::CycleSort),
            ('D', Action::FlipSortDir),
            ('C', Action::OpenColumns),
            ('T', Action::ToggleThemes),
            ('c', Action::ToggleClosed),
            ('m', Action::OpenMenu),
            ('e', Action::OpenClassify),
            ('u', Action::Undo),
            ('/', Action::OpenFilter),
        ];
        for (ch, action) in cases {
            assert_eq!(map(press(KeyCode::Char(ch))), Some(action), "{ch:?}");
        }
    }

    #[test]
    fn n_r_and_w_map_to_input_actions() {
        let cases = [
            ('n', Action::OpenCapture),
            ('r', Action::OpenRequest),
            ('w', Action::Withdraw),
        ];
        for (ch, action) in cases {
            assert_eq!(map(press(KeyCode::Char(ch))), Some(action), "{ch:?}");
        }
    }

    #[test]
    fn a_click_on_an_item_row_selects_it() {
        let mut app = app_with_regions();
        app.regions.items = vec![(Rect::new(0, 3, 60, 1), "R1".to_string())];
        let left = MouseEventKind::Down(MouseButton::Left);
        assert_eq!(
            mouse(event(left, 5, 3), &app),
            Some(Action::SelectItem("R1".to_string()))
        );
        assert_eq!(mouse(event(left, 5, 15), &app), None, "no row there");
    }

    #[test]
    fn a_header_click_sorts_and_an_id_cell_click_copies_before_the_row_selects() {
        use crate::config::ItemColumn;
        use crate::surface::HeaderKey;
        let mut app = app_with_regions();
        let effort = HeaderKey::Item(ItemColumn::Effort);
        app.regions.headers = vec![(Rect::new(10, 2, 8, 1), effort)];
        app.regions.items = vec![(Rect::new(0, 3, 60, 1), "R1".to_string())];
        app.regions.ids = vec![(Rect::new(2, 3, 4, 1), "R1".to_string())];
        let left = MouseEventKind::Down(MouseButton::Left);
        assert_eq!(
            mouse(event(left, 12, 2), &app),
            Some(Action::SortBy(effort))
        );
        assert_eq!(
            mouse(event(left, 3, 3), &app),
            Some(Action::CopyItem("R1".to_string()))
        );
        assert_eq!(
            mouse(event(left, 20, 3), &app),
            Some(Action::SelectItem("R1".to_string()))
        );
        app.regions.modal = Some(Rect::new(1, 1, 58, 18));
        assert_eq!(mouse(event(left, 12, 2), &app), None, "under the modal");
    }

    #[test]
    fn the_wheel_scrolls_the_legend_and_moves_the_selector_while_they_are_up() {
        let mut app = app_with_regions();
        let left = MouseEventKind::Down(MouseButton::Left);
        app.legend_open = true;
        assert_eq!(
            mouse(event(MouseEventKind::ScrollDown, 5, 4), &app),
            Some(Action::ScrollDetails(Scroll::Down(WHEEL_ROWS)))
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollUp, 70, 10), &app),
            Some(Action::ScrollDetails(Scroll::Up(WHEEL_ROWS))),
            "anywhere on screen"
        );
        assert_eq!(mouse(event(left, 5, 4), &app), None);

        app.legend_open = false;
        app.selector_open = true;
        assert_eq!(
            mouse(event(MouseEventKind::ScrollDown, 5, 4), &app),
            Some(Action::SelectorNext)
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollUp, 5, 4), &app),
            Some(Action::SelectorPrev)
        );
        assert_eq!(mouse(event(MouseEventKind::ScrollLeft, 5, 4), &app), None);
    }

    #[test]
    fn the_theme_picker_takes_the_wheel_and_swallows_clicks() {
        let mut app = app_with_regions();
        app.regions.tabs = vec![(Rect::new(0, 1, 7, 1), Surface::Review)];
        app.apply(Action::ToggleThemes);
        assert_eq!(
            mouse(event(MouseEventKind::ScrollDown, 70, 10), &app),
            Some(Action::Move(Dir::Down))
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollUp, 5, 4), &app),
            Some(Action::Move(Dir::Up))
        );
        let left = MouseEventKind::Down(MouseButton::Left);
        assert_eq!(mouse(event(left, 5, 4), &app), None, "a task row");
        assert_eq!(mouse(event(left, 2, 1), &app), None, "a tab");
    }

    #[test]
    fn a_left_press_on_a_tab_switches_surface() {
        let mut app = app_with_regions();
        app.regions.tabs = vec![
            (Rect::new(0, 1, 7, 1), Surface::Tasks),
            (Rect::new(8, 1, 10, 1), Surface::Review),
        ];
        let left = MouseEventKind::Down(MouseButton::Left);
        assert_eq!(
            mouse(event(left, 9, 1), &app),
            Some(Action::SwitchSurface(Surface::Review))
        );
        assert_eq!(mouse(event(left, 7, 1), &app), None, "the gap");
        assert_eq!(
            mouse(event(MouseEventKind::Up(MouseButton::Left), 9, 1), &app),
            None
        );
        app.regions.modal = Some(Rect::new(1, 3, 58, 18));
        assert_eq!(
            mouse(event(left, 2, 1), &app),
            Some(Action::SwitchSurface(Surface::Tasks)),
            "the modal covers only the body"
        );
    }

    #[test]
    fn the_wheel_scrolls_an_item_list_down_and_up_only() {
        let mut app = app_with_regions();
        app.surface = Surface::Backlog;
        app.view = ViewKind::Ego;
        assert_eq!(
            mouse(event(MouseEventKind::ScrollDown, 10, 10), &app),
            Some(Action::ScrollView(0, i32::from(WHEEL_ROWS))),
            "whatever view Tasks shows"
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollUp, 10, 10), &app),
            Some(Action::ScrollView(0, -i32::from(WHEEL_ROWS)))
        );
        assert_eq!(
            mouse(event(MouseEventKind::ScrollRight, 10, 10), &app),
            None
        );
    }

    #[test]
    fn a_modal_or_the_selector_shields_the_view() {
        let mut app = app_with_regions();
        app.regions.modal = Some(Rect::new(1, 3, 58, 18));
        let left = MouseEventKind::Down(MouseButton::Left);
        assert_eq!(mouse(event(left, 5, 4), &app), None);
        assert_eq!(mouse(event(MouseEventKind::ScrollDown, 5, 4), &app), None);

        app.regions.modal = None;
        app.selector_open = true;
        assert_eq!(mouse(event(left, 5, 4), &app), None);

        app.selector_open = false;
        app.overlay = Some(crate::actions::Overlay::Prompt {
            input: tui_input::Input::default(),
            before: String::new(),
        });
        assert_eq!(mouse(event(left, 5, 4), &app), None, "an open prompt");
        assert_eq!(mouse(event(MouseEventKind::ScrollDown, 70, 10), &app), None);
    }
}
