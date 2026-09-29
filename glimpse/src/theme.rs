//! Styles for each task status and UI element.

use ratatui::style::{Color, Modifier, Style};

/// Full-strength accent of the shared Kanso Zen palette; readable as a foreground.
pub(crate) const ACCENT: Color = Color::Rgb(0xFF, 0xC7, 0x99);
/// The same hue muted; it reads muddy as a foreground, so it is only ever a background.
pub(crate) const ACCENT_BG: Color = Color::Rgb(0xB6, 0x92, 0x7B);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Theme {
    pub(crate) pending: Style,
    pub(crate) in_progress: Style,
    pub(crate) done: Style,
    pub(crate) failed: Style,
    pub(crate) deferred: Style,
    pub(crate) selection: Style,
    pub(crate) needs_edge: Style,
    pub(crate) coupling_edge: Style,
    /// The diagram's edges leaving the selection.
    pub(crate) out_edge: Style,
    pub(crate) flash: Style,
    pub(crate) agent_chip: Style,
    pub(crate) badge: Style,
    /// A row's execution record holds a deferral.
    pub(crate) deferral_badge: Style,
    /// Inline code in a markdown task body.
    pub(crate) inline_code: Style,
    pub(crate) warning: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            pending: Style::new().add_modifier(Modifier::DIM),
            in_progress: Style::new().fg(ACCENT),
            done: Style::new().fg(Color::Green),
            failed: Style::new().fg(Color::Red),
            deferred: Style::new().fg(Color::DarkGray),
            selection: Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            needs_edge: Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            coupling_edge: Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            out_edge: Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            flash: Style::new().fg(Color::Black).bg(ACCENT),
            agent_chip: Style::new().fg(Color::Black).bg(ACCENT_BG),
            badge: Style::new().add_modifier(Modifier::BOLD),
            deferral_badge: Style::new().add_modifier(Modifier::BOLD),
            inline_code: Style::new().fg(ACCENT),
            warning: Style::new().fg(Color::Yellow),
        }
    }
}

impl Theme {
    /// Keyed by the task store's status vocabulary; an unknown status falls back to `pending`.
    pub(crate) fn status(&self, status: &str) -> Style {
        match status {
            "in-progress" => self.in_progress,
            "done" => self.done,
            "failed" => self.failed,
            "deferred" => self.deferred,
            _ => self.pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_status_maps_to_its_own_style() {
        let t = Theme::default();
        assert_eq!(t.status("in-progress").fg, Some(ACCENT));
        assert_eq!(t.status("done").fg, Some(Color::Green));
        assert_eq!(t.status("failed").fg, Some(Color::Red));
        assert_eq!(t.status("deferred").fg, Some(Color::DarkGray));
        assert!(t.status("pending").add_modifier.contains(Modifier::DIM));
        assert_eq!(
            t.status("Done"),
            t.pending,
            "status names are case-sensitive"
        );
    }
}
