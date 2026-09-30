//! Colour tokens and the styles built from them.
//!
//! [`TOKENS`] lists every token with its default. Palette tokens hold colours; element
//! tokens name a UI feature and default to a palette token by name, so a `[theme]`
//! table can repaint one feature or the whole palette. A value is a colour (`#rrggbb`,
//! an ANSI name, an index `0`–`255`, or `default` for the terminal's own) or another
//! token's name. With `NO_COLOR` set every colour is dropped and backgrounds fall back
//! to reverse video.

use std::collections::HashMap;
use std::str::FromStr;

use ratatui::style::{Color, Modifier, Style};

use crate::model::TaskStatus;

/// Every token and its default, palette first. Defaults are the Kanso Zen palette the
/// terminal, herdr and gitui share, with its status colours.
pub(crate) const TOKENS: &[(&str, &str)] = &[
    ("fg", "default"),
    ("accent", "#FFC799"),
    ("accent_bg", "#B6927B"),
    ("success", "#69DB7C"),
    ("danger", "#FF8080"),
    ("warning", "#E6C384"),
    ("info", "#7FB4CA"),
    ("violet", "#938AA9"),
    ("teal", "#7AA89F"),
    ("idle", "#C5C9C7"),
    ("muted", "#8A8F98"),
    ("faint", "#6C7380"),
    ("subtle", "#4E5561"),
    ("highlight", "#2C3640"),
    ("on_color", "#14171C"),
    ("status_pending", "idle"),
    ("status_in_progress", "accent"),
    ("status_done", "success"),
    ("status_failed", "danger"),
    ("status_deferred", "muted"),
    ("flash_fg", "on_color"),
    ("selection_bg", "highlight"),
    ("selection_mark", "accent"),
    ("unrelated", "faint"),
    ("edge", "fg"),
    ("edge_faded", "subtle"),
    ("edge_needs", "accent"),
    ("edge_out", "info"),
    ("edge_coupling", "info"),
    ("edge_overlap", "teal"),
    ("border", "subtle"),
    ("border_title", "fg"),
    ("layer_rule", "subtle"),
    ("layer_label", "muted"),
    ("checkpoint", "violet"),
    ("commit", "muted"),
    ("agent_bg", "accent_bg"),
    ("agent_fg", "on_color"),
    ("slug", "fg"),
    ("secondary", "muted"),
    ("key", "accent"),
    ("key_label", "muted"),
    ("key_separator", "subtle"),
    ("notice", "accent"),
    ("section", "info"),
    ("code", "accent"),
    ("effort", "muted"),
    ("warning_text", "warning"),
];

/// Tokens reference each other at most this deep, which also stops a cycle.
const MAX_DEPTH: usize = 8;

/// Checks `overrides` against [`TOKENS`]: every key must be a token and every value must
/// resolve to a colour.
pub(crate) fn validate(overrides: &HashMap<String, String>) -> Result<(), String> {
    for key in overrides.keys() {
        if !TOKENS.iter().any(|(name, _)| name == key) {
            return Err(format!("unknown theme token `{key}`"));
        }
    }
    for (name, _) in TOKENS {
        resolve(name, overrides)?;
    }
    Ok(())
}

fn raw<'a>(name: &str, overrides: &'a HashMap<String, String>) -> Option<&'a str> {
    overrides
        .get(name)
        .map(String::as_str)
        .or_else(|| TOKENS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v))
}

fn resolve(name: &str, overrides: &HashMap<String, String>) -> Result<Color, String> {
    let mut at = name;
    for _ in 0..MAX_DEPTH {
        let value = raw(at, overrides).ok_or_else(|| format!("unknown theme token `{at}`"))?;
        if TOKENS.iter().any(|(n, _)| *n == value) {
            at = value;
            continue;
        }
        return parse_color(value)
            .ok_or_else(|| format!("theme token `{at}`: `{value}` is not a colour or a token"));
    }
    Err(format!(
        "theme token `{name}` references too deeply (a cycle?)"
    ))
}

fn parse_color(value: &str) -> Option<Color> {
    match value.to_ascii_lowercase().as_str() {
        "default" | "reset" => Some(Color::Reset),
        _ => Color::from_str(value).ok(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Theme {
    pub(crate) pending: Style,
    pub(crate) in_progress: Style,
    pub(crate) done: Style,
    pub(crate) failed: Style,
    pub(crate) deferred: Style,
    /// A status change, keyed by the status it changed to.
    flash: [Style; 5],
    pub(crate) selection: Style,
    /// The `▸` beside the selected row.
    pub(crate) selection_mark: Style,
    /// Rows and titles unrelated to the selection; status marks never take it.
    pub(crate) unrelated: Style,
    pub(crate) edge: Style,
    /// Edges unrelated to the selection.
    pub(crate) edge_faded: Style,
    pub(crate) needs_edge: Style,
    pub(crate) coupling_edge: Style,
    /// The diagram's edges leaving the selection.
    pub(crate) out_edge: Style,
    pub(crate) overlap: Style,
    pub(crate) agent_chip: Style,
    pub(crate) agent_chip_stale: Style,
    /// Bold emphasis in the default colour.
    pub(crate) badge: Style,
    /// A row's execution record holds a deferral.
    pub(crate) deferral_badge: Style,
    /// Inline code in a markdown task body.
    pub(crate) inline_code: Style,
    pub(crate) warning: Style,
    pub(crate) border: Style,
    pub(crate) border_title: Style,
    pub(crate) layer_rule: Style,
    pub(crate) layer_label: Style,
    pub(crate) checkpoint: Style,
    pub(crate) commit: Style,
    pub(crate) slug: Style,
    /// Secondary text: history, file lists, placeholders.
    pub(crate) secondary: Style,
    pub(crate) key: Style,
    pub(crate) key_label: Style,
    pub(crate) key_separator: Style,
    pub(crate) notice: Style,
    pub(crate) section: Style,
    pub(crate) effort: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::build(&HashMap::new(), false)
    }
}

impl Theme {
    /// `overrides` must already have passed [`validate`]; a token that still fails to
    /// resolve takes the terminal's default colour.
    pub(crate) fn build(overrides: &HashMap<String, String>, no_color: bool) -> Theme {
        let color = |name: &str| -> Option<Color> {
            if no_color {
                return None;
            }
            resolve(name, overrides).ok()
        };
        let fg = |name: &str| match color(name) {
            Some(c) => Style::new().fg(c),
            None => Style::new(),
        };
        // A background that vanishes under NO_COLOR is replaced by reverse video.
        let on = |fg_name: &str, bg_name: &str| match (color(fg_name), color(bg_name)) {
            (Some(f), Some(b)) => Style::new().fg(f).bg(b),
            _ => Style::new().add_modifier(Modifier::REVERSED),
        };
        let bold = Modifier::BOLD;
        let flash = |status: &str| on("flash_fg", status);
        let selection = match color("selection_bg") {
            Some(bg) => Style::new().bg(bg).add_modifier(bold),
            None => Style::new().add_modifier(Modifier::REVERSED | bold),
        };
        let agent_chip = on("agent_fg", "agent_bg");
        Theme {
            pending: fg("status_pending"),
            in_progress: fg("status_in_progress"),
            done: fg("status_done"),
            failed: fg("status_failed"),
            deferred: fg("status_deferred"),
            flash: [
                flash("status_pending"),
                flash("status_in_progress"),
                flash("status_done"),
                flash("status_failed"),
                flash("status_deferred"),
            ],
            selection,
            selection_mark: fg("selection_mark").add_modifier(bold),
            unrelated: if no_color {
                Style::new().add_modifier(Modifier::DIM)
            } else {
                fg("unrelated")
            },
            edge: fg("edge"),
            edge_faded: if no_color {
                Style::new().add_modifier(Modifier::DIM)
            } else {
                fg("edge_faded")
            },
            needs_edge: fg("edge_needs").add_modifier(bold),
            coupling_edge: fg("edge_coupling").add_modifier(bold),
            out_edge: fg("edge_out").add_modifier(bold),
            overlap: fg("edge_overlap").add_modifier(bold),
            agent_chip,
            agent_chip_stale: agent_chip.add_modifier(Modifier::DIM),
            badge: Style::new().add_modifier(bold),
            deferral_badge: fg("status_deferred").add_modifier(bold),
            inline_code: fg("code"),
            warning: fg("warning_text"),
            border: fg("border"),
            border_title: fg("border_title").add_modifier(bold),
            layer_rule: fg("layer_rule"),
            layer_label: fg("layer_label"),
            checkpoint: fg("checkpoint").add_modifier(bold),
            commit: fg("commit"),
            slug: fg("slug").add_modifier(bold),
            secondary: fg("secondary"),
            key: fg("key").add_modifier(bold),
            key_label: fg("key_label"),
            key_separator: fg("key_separator"),
            notice: fg("notice").add_modifier(bold),
            section: fg("section").add_modifier(bold),
            effort: fg("effort"),
        }
    }

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

    /// The highlight a task takes after its status changes: the colour of the status it
    /// changed to, as a background.
    pub(crate) fn flash(&self, status: TaskStatus) -> Style {
        let at = match status {
            TaskStatus::InProgress => 1,
            TaskStatus::Done => 2,
            TaskStatus::Failed => 3,
            TaskStatus::Deferred => 4,
            TaskStatus::Pending | TaskStatus::Unknown => 0,
        };
        self.flash[at]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(hex: &str) -> Color {
        Color::from_str(hex).expect("hex colour")
    }

    #[test]
    fn each_status_maps_to_its_own_colour() {
        let t = Theme::default();
        assert_eq!(t.status("in-progress").fg, Some(rgb("#FFC799")));
        assert_eq!(t.status("done").fg, Some(rgb("#69DB7C")));
        assert_eq!(t.status("failed").fg, Some(rgb("#FF8080")));
        assert_eq!(t.status("deferred").fg, Some(rgb("#8A8F98")));
        assert_eq!(t.status("pending").fg, Some(rgb("#C5C9C7")));
        assert!(
            !t.status("pending").add_modifier.contains(Modifier::DIM),
            "status marks are never dimmed"
        );
        assert_eq!(
            t.status("Done"),
            t.pending,
            "status names are case-sensitive"
        );
    }

    #[test]
    fn a_flash_takes_the_colour_of_the_new_status() {
        let t = Theme::default();
        assert_eq!(t.flash(TaskStatus::Done).bg, Some(rgb("#69DB7C")));
        assert_eq!(t.flash(TaskStatus::Failed).bg, Some(rgb("#FF8080")));
        assert_eq!(t.flash(TaskStatus::InProgress).bg, Some(rgb("#FFC799")));
        assert_eq!(t.flash(TaskStatus::Done).fg, Some(rgb("#14171C")));
    }

    #[test]
    fn overrides_repaint_a_feature_or_the_palette() {
        let map = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let feature = Theme::build(&map(&[("checkpoint", "#112233")]), false);
        assert_eq!(feature.checkpoint.fg, Some(rgb("#112233")));
        assert_eq!(feature.done.fg, Some(rgb("#69DB7C")));

        let palette = Theme::build(&map(&[("success", "light-green")]), false);
        assert_eq!(palette.done.fg, Some(Color::LightGreen));
        assert_eq!(palette.flash(TaskStatus::Done).bg, Some(Color::LightGreen));

        let reference = Theme::build(&map(&[("checkpoint", "danger")]), false);
        assert_eq!(reference.checkpoint.fg, Some(rgb("#FF8080")));
        assert_eq!(
            Theme::build(&map(&[("edge", "default")]), false).edge.fg,
            Some(Color::Reset)
        );
    }

    #[test]
    fn bad_overrides_are_rejected() {
        let one = |k: &str, v: &str| HashMap::from([(k.to_string(), v.to_string())]);
        assert!(validate(&one("checkpoint", "#123456")).is_ok());
        assert!(validate(&one("checkpoints", "#123456")).is_err(), "unknown");
        assert!(
            validate(&one("checkpoint", "chartreuse")).is_err(),
            "bad colour"
        );
        let cycle = HashMap::from([
            ("accent".to_string(), "info".to_string()),
            ("info".to_string(), "accent".to_string()),
        ]);
        assert!(validate(&cycle).is_err(), "a cycle");
    }

    #[test]
    fn no_color_drops_colours_and_keeps_highlights_visible() {
        let t = Theme::build(&HashMap::new(), true);
        assert_eq!(t.done.fg, None);
        assert!(t.selection.add_modifier.contains(Modifier::REVERSED));
        assert!(
            t.flash(TaskStatus::Done)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(t.agent_chip.add_modifier.contains(Modifier::REVERSED));
    }
}
