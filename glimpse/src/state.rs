//! Layout choices that outlive a run: the view, a pinned orientation or panel split, the
//! docked panel's share, whether the diagram lays out implied edges, the surface on screen
//! and each item surface's group-by and sort.
//!
//! They live in `<claude dir>/glimpse/state.toml`, apart from the user's config, which
//! glimpse never writes. Reading is lenient and saving is silent: a missing, corrupt or
//! unwritable file only means the next run starts from the config.

use std::path::PathBuf;

use crate::app::App;
use crate::config::{
    Orientation, OrientationPref, PANEL_PERCENT_RANGE, Split, ViewKind, claude_dir,
};
use crate::surface::{Group, Sort, Surface};

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct State {
    pub(crate) view: Option<ViewKind>,
    pub(crate) orientation: Option<Orientation>,
    pub(crate) split: Option<Split>,
    pub(crate) panel_percent: Option<u16>,
    pub(crate) show_implied: Option<bool>,
    pub(crate) surface: Option<Surface>,
    /// Per item surface, in [`Surface::ALL`] order, the group-by and sort it was left in.
    pub(crate) arrangements: Vec<(Surface, Group, Sort)>,
}

/// The surfaces whose group-by and sort are saved.
fn item_surfaces() -> impl Iterator<Item = Surface> {
    Surface::ALL
        .into_iter()
        .filter(|surface| surface.ledger_kind().is_some())
}

/// Reads one `[items.<surface>]` table. A group or sort that surface does not offer
/// reads as the default.
fn parse_arrangement(surface: Surface, table: &toml::Table) -> (Surface, Group, Sort) {
    let text = |key: &str| table.get(key).and_then(toml::Value::as_str);
    let group = text("group")
        .and_then(|g| surface.groups().iter().copied().find(|o| o.key() == g))
        .unwrap_or_default();
    let sort = text("sort")
        .and_then(|s| surface.sorts().iter().copied().find(|o| o.key() == s))
        .unwrap_or_default();
    (surface, group, sort)
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
            surface: text("surface").and_then(Surface::from_key),
            arrangements: match table.get("items").and_then(toml::Value::as_table) {
                Some(items) => item_surfaces()
                    .filter_map(|surface| {
                        let saved = items.get(surface.key())?.as_table()?;
                        Some(parse_arrangement(surface, saved))
                    })
                    .collect(),
                None => Vec::new(),
            },
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
        if let Some(surface) = self.surface {
            out.push_str(&format!("surface = \"{}\"\n", surface.key()));
        }
        // Tables come after every top-level key, or TOML would read those keys into them.
        for (surface, group, sort) in &self.arrangements {
            out.push_str(&format!(
                "\n[items.{}]\ngroup = \"{}\"\nsort = \"{}\"\n",
                surface.key(),
                group.key(),
                sort.key()
            ));
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
            surface: Some(app.surface),
            arrangements: item_surfaces()
                .filter_map(|surface| {
                    let state = app.items.get(&surface)?;
                    Some((surface, state.group, state.sort))
                })
                .collect(),
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
        if let Some(surface) = self.surface {
            app.surface = surface;
        }
        for (surface, group, sort) in &self.arrangements {
            if let Some(state) = app.items.get_mut(surface) {
                state.group = *group;
                state.sort = *sort;
            }
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
            ..State::default()
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
            ..State::default()
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

    #[test]
    fn state_round_trips_surface_group_and_sort() {
        let mut app = App::new(fixture(), &Config::default());
        app.surface = Surface::PlanReview;
        let plan = app
            .items
            .get_mut(&Surface::PlanReview)
            .expect("item surface");
        plan.group = Group::Category;
        plan.sort = Sort::Newest;
        let backlog = app.items.get_mut(&Surface::Backlog).expect("item surface");
        backlog.group = Group::Area;

        let state = State::capture(&app);
        assert_eq!(state.arrangements.len(), 4, "every item surface, not Inbox");
        assert_eq!(State::parse(&state.render()), state);

        let mut restored = App::new(fixture(), &Config::default());
        State::parse(&state.render()).apply(&mut restored, false, false);
        assert_eq!(restored.surface, Surface::PlanReview);
        let plan = &restored.items[&Surface::PlanReview];
        assert_eq!((plan.group, plan.sort), (Group::Category, Sort::Newest));
        assert_eq!(restored.items[&Surface::Backlog].group, Group::Area);
    }

    #[test]
    fn a_group_or_sort_the_surface_lacks_reads_as_the_default() {
        let state = State::parse(
            "surface = \"nope\"\n[items.backlog]\ngroup = \"severity\"\nsort = \"newest\"\n",
        );
        assert_eq!(state.surface, None);
        assert_eq!(
            state.arrangements,
            [(Surface::Backlog, Group::None, Sort::Newest)]
        );
    }
}
