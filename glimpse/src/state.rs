//! Layout choices that outlive a run: the view, a pinned orientation or panel split, the
//! docked panel's share and whether the diagram lays out implied edges.
//!
//! They live in `<claude dir>/glimpse/state.toml`, apart from the user's config, which
//! glimpse never writes. Reading is lenient and saving is silent: a missing, corrupt or
//! unwritable file only means the next run starts from the config.

use std::path::PathBuf;

use crate::app::App;
use crate::config::{
    Orientation, OrientationPref, PANEL_PERCENT_RANGE, Split, ViewKind, claude_dir,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct State {
    pub(crate) view: Option<ViewKind>,
    pub(crate) orientation: Option<Orientation>,
    pub(crate) split: Option<Split>,
    pub(crate) panel_percent: Option<u16>,
    pub(crate) show_implied: Option<bool>,
}

impl State {
    /// Keys that are missing, unknown or invalid are skipped one by one.
    pub(crate) fn parse(text: &str) -> State {
        let Ok(table) = toml::from_str::<toml::Table>(text) else {
            return State::default();
        };
        let text = |key: &str| table.get(key).and_then(toml::Value::as_str);
        State {
            view: text("view").and_then(ViewKind::parse),
            orientation: text("orientation").and_then(|s| match OrientationPref::parse(s) {
                Some(OrientationPref::Fixed(o)) => Some(o),
                _ => None,
            }),
            split: text("split").and_then(Split::parse),
            panel_percent: table
                .get("panel_percent")
                .and_then(toml::Value::as_integer)
                .and_then(|v| u16::try_from(v).ok())
                .filter(|v| PANEL_PERCENT_RANGE.contains(v)),
            show_implied: table.get("show_implied").and_then(toml::Value::as_bool),
        }
    }

    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        if let Some(view) = self.view {
            out.push_str(&format!("view = \"{}\"\n", view.as_str()));
        }
        if let Some(orientation) = self.orientation {
            out.push_str(&format!("orientation = \"{}\"\n", orientation.as_str()));
        }
        if let Some(split) = self.split {
            out.push_str(&format!("split = \"{}\"\n", split.as_str()));
        }
        if let Some(percent) = self.panel_percent {
            out.push_str(&format!("panel_percent = {percent}\n"));
        }
        if let Some(show) = self.show_implied {
            out.push_str(&format!("show_implied = {show}\n"));
        }
        out
    }

    /// Only pinned choices are kept: an orientation or split `auto` resolved is not.
    pub(crate) fn capture(app: &App) -> State {
        State {
            view: Some(app.view),
            orientation: app.orientation_override,
            split: app.split_override,
            panel_percent: Some(app.panel_percent),
            show_implied: Some(app.show_implied),
        }
    }

    /// A view or orientation given on the command line wins over the saved one.
    pub(crate) fn apply(&self, app: &mut App, keep_view: bool, keep_orientation: bool) {
        if let Some(view) = self.view.filter(|_| !keep_view) {
            app.view = view;
        }
        if let Some(orientation) = self.orientation.filter(|_| !keep_orientation) {
            app.orientation_override = Some(orientation);
        }
        if let Some(split) = self.split {
            app.split_override = Some(split);
        }
        if let Some(percent) = self.panel_percent {
            app.panel_percent = percent;
        }
        if let Some(show) = self.show_implied {
            app.show_implied = show;
        }
    }

    fn path() -> Option<PathBuf> {
        Some(claude_dir()?.join("glimpse").join("state.toml"))
    }

    pub(crate) fn load() -> State {
        State::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map_or_else(State::default, |text| State::parse(&text))
    }

    /// Written beside the target and renamed over it, so a crash never leaves half a file.
    pub(crate) fn save(&self) {
        let Some(path) = State::path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("toml.tmp");
        if std::fs::write(&tmp, self.render()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::fixture;

    #[test]
    fn state_round_trips_and_skips_bad_keys() {
        let state = State {
            view: Some(ViewKind::Diagram),
            orientation: Some(Orientation::Horizontal),
            split: Some(Split::Below),
            panel_percent: Some(55),
            show_implied: Some(true),
        };
        assert_eq!(State::parse(&state.render()), state);
        assert_eq!(State::parse("not toml ["), State::default());
        let partial = State::parse("view = \"nope\"\npanel_percent = 99\nsplit = \"beside\"\n");
        assert_eq!(
            partial,
            State {
                split: Some(Split::Beside),
                ..State::default()
            }
        );
    }

    #[test]
    fn apply_restores_choices_unless_the_command_line_pinned_them() {
        let state = State {
            view: Some(ViewKind::Ego),
            orientation: Some(Orientation::Horizontal),
            split: Some(Split::Beside),
            panel_percent: Some(60),
            show_implied: Some(true),
        };
        let mut app = App::new(fixture(), &Config::default());
        state.apply(&mut app, false, true);
        assert_eq!(app.view, ViewKind::Ego);
        assert_eq!(app.orientation_override, None, "--orientation was given");
        assert_eq!(app.split_override, Some(Split::Beside));
        assert_eq!(app.panel_percent, 60);
        assert!(app.show_implied);
        assert_eq!(State::capture(&app).view, Some(ViewKind::Ego));

        let mut app = App::new(fixture(), &Config::default());
        state.apply(&mut app, true, false);
        assert_eq!(app.view, ViewKind::Layers, "--view was given");
        assert_eq!(app.orientation_override, Some(Orientation::Horizontal));
    }
}
