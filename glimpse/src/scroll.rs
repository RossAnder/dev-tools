//! One paging contract shared by cursor lists and viewport panes.

use crate::app::Scroll;

/// Moves a list cursor. `None` lands on the first row and an empty list has no cursor.
pub(crate) fn step(at: Option<usize>, len: usize, page: usize, scroll: Scroll) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    let Some(at) = at else {
        return Some(0);
    };
    let at = at.min(last);
    let page = page.max(1);
    let to = match scroll {
        Scroll::Up(n) => at.saturating_sub(usize::from(n)),
        Scroll::Down(n) => at.saturating_add(usize::from(n)),
        Scroll::PageUp => at.saturating_sub(page),
        Scroll::PageDown => at.saturating_add(page),
        Scroll::Top => 0,
        Scroll::Bottom => last,
    };
    Some(to.min(last))
}

/// Moves a viewport offset within `0..=max`. A page keeps one row of overlap.
pub(crate) fn offset(current: u16, max: u16, page: u16, scroll: Scroll) -> u16 {
    let page = page.saturating_sub(1).max(1);
    let to = match scroll {
        Scroll::Up(n) => current.saturating_sub(n),
        Scroll::Down(n) => current.saturating_add(n),
        Scroll::PageUp => current.saturating_sub(page),
        Scroll::PageDown => current.saturating_add(page),
        Scroll::Top => 0,
        Scroll::Bottom => max,
    };
    to.min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list_has_no_cursor() {
        assert_eq!(step(None, 0, 5, Scroll::Down(1)), None);
        assert_eq!(step(Some(3), 0, 5, Scroll::Bottom), None);
    }

    #[test]
    fn no_cursor_lands_on_first_row() {
        assert_eq!(step(None, 10, 5, Scroll::Bottom), Some(0));
        assert_eq!(step(None, 10, 5, Scroll::PageDown), Some(0));
    }

    #[test]
    fn cursor_clamps_at_both_ends() {
        assert_eq!(step(Some(1), 10, 5, Scroll::Up(4)), Some(0));
        assert_eq!(step(Some(8), 10, 5, Scroll::Down(4)), Some(9));
        assert_eq!(step(Some(8), 10, 5, Scroll::PageDown), Some(9));
        assert_eq!(step(Some(2), 10, 5, Scroll::PageUp), Some(0));
    }

    #[test]
    fn cursor_jumps_to_ends() {
        assert_eq!(step(Some(4), 10, 5, Scroll::Top), Some(0));
        assert_eq!(step(Some(4), 10, 5, Scroll::Bottom), Some(9));
    }

    #[test]
    fn cursor_pages_by_the_drawn_page() {
        assert_eq!(step(Some(2), 20, 5, Scroll::PageDown), Some(7));
        assert_eq!(step(Some(12), 20, 5, Scroll::PageUp), Some(7));
    }

    #[test]
    fn page_of_zero_moves_one_row() {
        assert_eq!(step(Some(2), 10, 0, Scroll::PageDown), Some(3));
        assert_eq!(step(Some(2), 10, 0, Scroll::PageUp), Some(1));
        assert_eq!(offset(2, 10, 0, Scroll::PageDown), 3);
        assert_eq!(offset(2, 10, 1, Scroll::PageUp), 1);
    }

    #[test]
    fn offset_clamps_at_both_ends() {
        assert_eq!(offset(1, 10, 5, Scroll::Up(4)), 0);
        assert_eq!(offset(8, 10, 5, Scroll::Down(4)), 10);
        assert_eq!(offset(8, 10, 5, Scroll::Down(u16::MAX)), 10);
        assert_eq!(offset(0, 0, 5, Scroll::PageDown), 0);
    }

    #[test]
    fn offset_pages_with_one_row_of_overlap() {
        assert_eq!(offset(0, 100, 10, Scroll::PageDown), 9);
        assert_eq!(offset(20, 100, 10, Scroll::PageUp), 11);
    }

    #[test]
    fn offset_jumps_to_ends() {
        assert_eq!(offset(4, 10, 5, Scroll::Top), 0);
        assert_eq!(offset(4, 10, 5, Scroll::Bottom), 10);
    }
}
