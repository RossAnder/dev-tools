//! Mapping from key and mouse events to application actions.
//!
//! The key mapping is context-free: `j` is always a move and `Enter` always
//! details. [`App::apply`](crate::app::App::apply) decides what an action
//! means while an overlay is open. The mouse mapping reads the regions the
//! last frame recorded, since a click means nothing without them.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;

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
    let action = match event.code {
        KeyCode::Char('j') | KeyCode::Down => Action::Move(Dir::Down),
        KeyCode::Char('k') | KeyCode::Up => Action::Move(Dir::Up),
        KeyCode::Char('h') | KeyCode::Left => Action::Move(Dir::Left),
        KeyCode::Char('l') | KeyCode::Right => Action::Move(Dir::Right),
        // Shift+j and Shift+k arrive as the capital letter, with or without SHIFT set.
        KeyCode::Char('J') => Action::ScrollDetails(Scroll::Down(1)),
        KeyCode::Char('K') => Action::ScrollDetails(Scroll::Up(1)),
        KeyCode::PageDown => Action::ScrollDetails(Scroll::PageDown),
        KeyCode::PageUp => Action::ScrollDetails(Scroll::PageUp),
        KeyCode::Home => Action::ScrollDetails(Scroll::Top),
        KeyCode::End => Action::ScrollDetails(Scroll::Bottom),
        // Some terminals send Shift+Tab as `Tab` with SHIFT rather than `BackTab`.
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => Action::PrevView,
        KeyCode::Tab => Action::NextView,
        KeyCode::BackTab => Action::PrevView,
        KeyCode::Enter => Action::Details,
        KeyCode::Char('o') => Action::FlipOrientation,
        KeyCode::Char('|') => Action::FlipSplit,
        KeyCode::Char('i') => Action::ToggleImplied,
        KeyCode::Char('?') => Action::ToggleLegend,
        KeyCode::Char('f') => Action::ToggleFollow,
        KeyCode::Char('s') => Action::ToggleSelector,
        KeyCode::Char('a') => Action::ToggleAutoFlow,
        KeyCode::Char('t') => Action::ToggleActivity,
        KeyCode::Char('d') => Action::CycleDensity,
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
        KeyCode::Char('c') => Action::ToggleClosed,
        KeyCode::Char('m') => Action::OpenMenu,
        KeyCode::Char('e') => Action::OpenClassify,
        KeyCode::Char('u') => Action::Undo,
        KeyCode::Char('/') => Action::OpenFilter,
        _ => return None,
    };
    Some(action)
}

/// The wheel scrolls the details panel or the view under the pointer, never the
/// selection; a left press on a task row selects it, and one on the divider starts a
/// drag that left-button motion continues and the release ends. Only the press edge of
/// a click acts, the mouse counterpart of dropping key `Release`. Every other kind,
/// including the motion reports capture turns on, maps to nothing. With the selector,
/// the legend, a form or the filter prompt up, nothing reacts; with the compact modal up,
/// nothing outside the details panel does.
pub(crate) fn mouse(event: MouseEvent, app: &App) -> Option<Action> {
    if app.dragging {
        return match event.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                Some(Action::DragTo(event.column, event.row))
            }
            MouseEventKind::Up(_) | MouseEventKind::Down(_) => Some(Action::DragEnd),
            _ => None,
        };
    }
    if app.selector_open || app.legend_open || app.overlay.is_some() {
        return None;
    }
    let at = Position::new(event.column, event.row);
    let regions = &app.regions;
    if event.kind == MouseEventKind::Down(MouseButton::Left)
        && regions.modal.is_none()
        && regions.divider.is_some_and(|r| r.contains(at))
    {
        return Some(Action::DragStart);
    }
    let over_details = regions.details.is_some_and(|r| r.contains(at));
    let over_view = regions.modal.is_none() && regions.view.is_some_and(|r| r.contains(at));
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) if regions.modal.is_none() => regions
            .items
            .iter()
            .find(|(rect, _)| rect.contains(at))
            .map(|(_, id)| Action::SelectItem(id.clone()))
            .or_else(|| {
                regions
                    .tasks
                    .iter()
                    .find(|(rect, _)| over_view && rect.contains(at))
                    .map(|(_, id)| Action::Select(*id))
            }),
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

/// One wheel notch over the view, as `(across, down)` in notches. Horizontal layers
/// scroll a whole layer per notch whichever way the wheel turns; the traversal view
/// fits its area and takes no wheel.
fn scroll_view(app: &App, across: i32, down: i32) -> Option<Action> {
    let rows = i32::from(WHEEL_ROWS);
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
    use ratatui::layout::Rect;

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
            (KeyCode::Char('['), Action::ResizePanel(false)),
            (KeyCode::Char(']'), Action::ResizePanel(true)),
            (KeyCode::Char('-'), Action::ResizeColumns(false)),
            (KeyCode::Char('='), Action::ResizeColumns(true)),
            (KeyCode::Char('+'), Action::ResizeColumns(true)),
            (KeyCode::Char('q'), Action::Back),
            (KeyCode::Esc, Action::Back),
        ];
        for (code, action) in cases {
            assert_eq!(map(press(code)), Some(action), "{code:?}");
        }
        assert_eq!(map(press(KeyCode::Char('z'))), None);
    }

    #[test]
    fn scroll_keys_do_not_collide_with_moves() {
        let cases = [
            (KeyCode::Char('J'), Scroll::Down(1)),
            (KeyCode::Char('K'), Scroll::Up(1)),
            (KeyCode::PageDown, Scroll::PageDown),
            (KeyCode::PageUp, Scroll::PageUp),
            (KeyCode::Home, Scroll::Top),
            (KeyCode::End, Scroll::Bottom),
        ];
        for (code, scroll) in cases {
            assert_eq!(
                map(press(code)),
                Some(Action::ScrollDetails(scroll)),
                "{code:?}"
            );
        }
        assert_eq!(
            map(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)),
            Some(Action::ScrollDetails(Scroll::Down(1)))
        );
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
