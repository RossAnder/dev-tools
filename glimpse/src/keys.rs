//! Mapping from key events to application actions.
//!
//! The mapping is context-free: `j` is always a move and `Enter` always
//! details. [`App::apply`](crate::app::App::apply) decides what an action
//! means while an overlay is open.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{Action, Dir};

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
        // Some terminals send Shift+Tab as `Tab` with SHIFT rather than `BackTab`.
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => Action::PrevView,
        KeyCode::Tab => Action::NextView,
        KeyCode::BackTab => Action::PrevView,
        KeyCode::Enter => Action::Details,
        KeyCode::Char('o') => Action::FlipOrientation,
        KeyCode::Char('f') => Action::ToggleFollow,
        KeyCode::Char('s') => Action::ToggleSelector,
        KeyCode::Char('a') => Action::ToggleAutoFlow,
        KeyCode::Char('t') => Action::ToggleActivity,
        KeyCode::Char('q') | KeyCode::Esc => Action::Back,
        _ => return None,
    };
    Some(action)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            (KeyCode::Char('q'), Action::Back),
            (KeyCode::Esc, Action::Back),
        ];
        for (code, action) in cases {
            assert_eq!(map(press(code)), Some(action), "{code:?}");
        }
        assert_eq!(map(press(KeyCode::Char('z'))), None);
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
}
