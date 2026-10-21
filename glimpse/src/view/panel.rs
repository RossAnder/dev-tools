//! The one framed-panel helper: rounded themed borders, a titled top edge and a
//! cell of padding inside each side.

use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Padding};

use crate::theme::Theme;

pub(crate) fn block<'a>(title: &'a str, theme: &Theme) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border)
        .title(Line::styled(title, theme.border_title))
        .padding(Padding::horizontal(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    #[test]
    fn the_block_has_rounded_corners_and_a_title() {
        let theme = Theme::build(&Default::default(), false);
        let mut terminal = Terminal::new(TestBackend::new(12, 3)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget(block(" t ", &theme), Rect::new(0, 0, 12, 3)))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        assert_eq!(buffer[(0, 0)].symbol(), "╭");
        assert_eq!(buffer[(11, 2)].symbol(), "╯");
        assert_eq!(buffer[(1, 0)].symbol(), " ");
        assert_eq!(buffer[(2, 0)].symbol(), "t");
    }
}
