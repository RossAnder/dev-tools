//! Layout choices that outlive a run: the view, a pinned orientation or panel split, the
//! docked panel's share, whether the diagram lays out implied edges and its zoom and fit,
//! the surface on screen, each item surface's group-by, sort and direction, the Inbox's
//! sort and direction, the item columns, and a theme set or hint mode chosen in-app.
//!
//! They live in `<claude dir>/glimpse/state.toml`, apart from the user's config, which
//! glimpse never writes. Reading is lenient and saving is silent: a missing, corrupt or
//! unwritable file only means the next run starts from the config.

use std::path::PathBuf;

use crate::app::App;
use crate::config::{
    HintMode, ItemColumn, Orientation, OrientationPref, PANEL_PERCENT_RANGE, Split, ViewKind,
    claude_dir,
};
use crate::diagram::Zoom;
use crate::surface::{Group, InboxColumn, Sort, SortDir, Surface};
use crate::theme::sets::{self, ThemeSet};

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct State {
    pub(crate) view: Option<ViewKind>,
    pub(crate) orientation: Option<Orientation>,
    pub(crate) split: Option<Split>,
    pub(crate) panel_percent: Option<u16>,
    pub(crate) show_implied: Option<bool>,
    pub(crate) surface: Option<Surface>,
    /// Per item surface, in [`Surface::ALL`] order, the group-by, sort and direction it
    /// was left in.
    pub(crate) arrangements: Vec<(Surface, Group, Sort, SortDir)>,
    pub(crate) inbox: Option<(InboxColumn, SortDir)>,
    pub(crate) hints: Option<HintMode>,
    pub(crate) item_columns: Option<Vec<ItemColumn>>,
    /// Saved by name; a name no set carries reads as absent.
    pub(crate) theme: Option<&'static ThemeSet>,
    pub(crate) zoom: Option<Zoom>,
    pub(crate) fit: Option<bool>,
}

/// What a run opened with once its saved state was applied. [`State::capture`] saves a
/// theme or hint mode only when the run changed it or the file already held one, so
/// until one is chosen in-app a config edit still takes effect.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Baseline {
    theme: &'static ThemeSet,
    hints: HintMode,
    saved_theme: Option<&'static ThemeSet>,
    saved_hints: Option<HintMode>,
    /// `--theme` or `NO_COLOR` chose the set, so the run's own set is never saved.
    keep_theme: bool,
}

/// The surfaces whose group-by and sort are saved.
fn item_surfaces() -> impl Iterator<Item = Surface> {
    Surface::ALL
        .into_iter()
        .filter(|surface| surface.ledger_kind().is_some())
}

/// Reads one `[items.<surface>]` table. A group or sort that surface does not offer
/// reads as the default, as does a missing direction.
fn parse_arrangement(surface: Surface, table: &toml::Table) -> (Surface, Group, Sort, SortDir) {
    let text = |key: &str| table.get(key).and_then(toml::Value::as_str);
    let group = text("group")
        .and_then(|g| surface.groups().iter().copied().find(|o| o.key() == g))
        .unwrap_or_default();
    let sort = text("sort")
        .and_then(|s| surface.sorts().iter().copied().find(|o| o.key() == s))
        .unwrap_or_default();
    let dir = text("dir").and_then(SortDir::from_key).unwrap_or_default();
    (surface, group, sort, dir)
}

/// Reads the `[items.inbox]` table the same way.
fn parse_inbox(table: &toml::Table) -> (InboxColumn, SortDir) {
    let text = |key: &str| table.get(key).and_then(toml::Value::as_str);
    let sort = text("sort")
        .and_then(InboxColumn::from_key)
        .unwrap_or_default();
    let dir = text("dir").and_then(SortDir::from_key).unwrap_or_default();
    (sort, dir)
}

/// Unknown names and repeats are dropped; a list left empty reads as absent.
fn parse_columns(entries: &[toml::Value]) -> Option<Vec<ItemColumn>> {
    let mut columns = Vec::new();
    for column in entries
        .iter()
        .filter_map(toml::Value::as_str)
        .filter_map(ItemColumn::parse)
    {
        if !columns.contains(&column) {
            columns.push(column);
        }
    }
    (!columns.is_empty()).then_some(columns)
}

impl State {
    /// Keys that are missing, unknown or invalid are skipped one by one.
    pub(crate) fn parse(text: &str) -> State {
        let Ok(table) = toml::from_str::<toml::Table>(text) else {
            return State::default();
        };
        let text = |key: &str| table.get(key).and_then(toml::Value::as_str);
        let items = table.get("items").and_then(toml::Value::as_table);
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
            arrangements: match items {
                Some(items) => item_surfaces()
                    .filter_map(|surface| {
                        let saved = items.get(surface.key())?.as_table()?;
                        Some(parse_arrangement(surface, saved))
                    })
                    .collect(),
                None => Vec::new(),
            },
            inbox: items
                .and_then(|items| items.get(Surface::Inbox.key()))
                .and_then(toml::Value::as_table)
                .map(parse_inbox),
            hints: text("hints").and_then(HintMode::parse),
            item_columns: table
                .get("item_columns")
                .and_then(toml::Value::as_array)
                .and_then(|entries| parse_columns(entries)),
            theme: text("theme").and_then(sets::by_name),
            zoom: text("zoom").and_then(Zoom::from_key),
            fit: table.get("fit").and_then(toml::Value::as_bool),
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
        if let Some(hints) = self.hints {
            out.push_str(&format!("hints = \"{}\"\n", hints.as_str()));
        }
        if let Some(columns) = &self.item_columns {
            let names: Vec<String> = columns
                .iter()
                .map(|column| format!("\"{}\"", column.as_str()))
                .collect();
            out.push_str(&format!("item_columns = [{}]\n", names.join(", ")));
        }
        // Only a known set's name is ever held, and those need no escaping.
        if let Some(theme) = self.theme {
            out.push_str(&format!("theme = \"{}\"\n", theme.name));
        }
        if let Some(zoom) = self.zoom {
            out.push_str(&format!("zoom = \"{}\"\n", zoom.key()));
        }
        if let Some(fit) = self.fit {
            out.push_str(&format!("fit = {fit}\n"));
        }
        // Tables come after every top-level key, or TOML would read those keys into them.
        for (surface, group, sort, dir) in &self.arrangements {
            out.push_str(&format!(
                "\n[items.{}]\ngroup = \"{}\"\nsort = \"{}\"\ndir = \"{}\"\n",
                surface.key(),
                group.key(),
                sort.key(),
                dir.key()
            ));
        }
        if let Some((sort, dir)) = self.inbox {
            out.push_str(&format!(
                "\n[items.{}]\nsort = \"{}\"\ndir = \"{}\"\n",
                Surface::Inbox.key(),
                sort.key(),
                dir.key()
            ));
        }
        out
    }

    /// Only pinned choices are kept: an orientation or split `auto` resolved is not. A
    /// theme picker still open is previewing, so the set it opened on is the one saved.
    pub(crate) fn capture(app: &App, baseline: &Baseline) -> State {
        let shown = shown_set(app);
        let theme = if baseline.keep_theme {
            baseline.saved_theme
        } else {
            (baseline.saved_theme.is_some() || shown != baseline.theme).then_some(shown)
        };
        let hints =
            (baseline.saved_hints.is_some() || app.hints != baseline.hints).then_some(app.hints);
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
                    Some((surface, state.group, state.sort, state.sort_dir))
                })
                .collect(),
            inbox: Some((app.inbox.sort, app.inbox.sort_dir)),
            hints,
            item_columns: Some(app.item_columns.clone()),
            theme,
            zoom: Some(app.diagram_zoom),
            fit: Some(app.diagram_fit),
        }
    }

    /// A view, orientation or theme given on the command line wins over the saved one,
    /// and `keep_theme` also covers `NO_COLOR`. Returns what `capture` compares against.
    pub(crate) fn apply(
        &self,
        app: &mut App,
        keep_view: bool,
        keep_orientation: bool,
        keep_theme: bool,
    ) -> Baseline {
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
        for (surface, group, sort, dir) in &self.arrangements {
            if let Some(state) = app.items.get_mut(surface) {
                state.group = *group;
                state.sort = *sort;
                state.sort_dir = *dir;
            }
        }
        if let Some((sort, dir)) = self.inbox {
            app.inbox.sort = sort;
            app.inbox.sort_dir = dir;
        }
        if let Some(hints) = self.hints {
            app.hints = hints;
        }
        if let Some(columns) = &self.item_columns {
            app.item_columns = columns.clone();
        }
        if let Some(zoom) = self.zoom {
            app.diagram_zoom = zoom;
        }
        if let Some(fit) = self.fit {
            app.diagram_fit = fit;
        }
        if !keep_theme
            && let Some(set) = self.theme
            && set != app.theme_set
        {
            app.show_set(set);
        }
        Baseline {
            theme: app.theme_set,
            hints: app.hints,
            saved_theme: self.theme,
            saved_hints: self.hints,
            keep_theme,
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

    /// What a quitting run saves over `on_disk`, the file as it stands now. Several runs
    /// share the file, so a theme or hint mode this run left as it opened keeps the
    /// on-disk value, which another run may have chosen since; the layout keys are the
    /// last quitter's.
    pub(crate) fn over_disk(app: &App, baseline: &Baseline, on_disk: &State) -> State {
        let mut state = State::capture(app, baseline);
        if baseline.keep_theme || shown_set(app) == baseline.theme {
            state.theme = on_disk.theme;
        }
        if app.hints == baseline.hints {
            state.hints = on_disk.hints;
        }
        state
    }

    /// A file that exists but cannot be read now, as while another run renames over it on
    /// Windows, falls back to this run's own capture rather than reading as empty.
    pub(crate) fn save_session(app: &App, baseline: &Baseline) {
        let Some(path) = State::path() else {
            return;
        };
        let state = match std::fs::read_to_string(&path) {
            Ok(text) => State::over_disk(app, baseline, &State::parse(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                State::over_disk(app, baseline, &State::default())
            }
            Err(_) => State::capture(app, baseline),
        };
        state.save();
    }

    /// Written beside the target under a per-process name and renamed over it, so a crash
    /// never leaves half a file and two runs quitting together never share a temp file.
    pub(crate) fn save(&self) {
        let Some(path) = State::path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, self.render()).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// A theme picker still open is previewing, so the set it opened on counts as shown.
fn shown_set(app: &App) -> &'static ThemeSet {
    app.theme_picker
        .as_ref()
        .map_or(app.theme_set, |picker| picker.original)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ThemePicker;
    use crate::config::{Config, ThemeChoice};
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
    fn every_new_key_round_trips_and_bad_values_are_skipped() {
        let state = State {
            arrangements: vec![(
                Surface::Review,
                Group::Category,
                Sort::Newest,
                SortDir::Desc,
            )],
            inbox: Some((InboxColumn::Kind, SortDir::Desc)),
            hints: Some(HintMode::Hidden),
            item_columns: Some(vec![ItemColumn::Summary, ItemColumn::Id]),
            theme: Some(&sets::KANSO_PEARL),
            zoom: Some(Zoom::Tight),
            fit: Some(false),
            ..State::default()
        };
        assert_eq!(State::parse(&state.render()), state);

        let bad = State::parse(
            "hints = \"loud\"\nitem_columns = [\"owner\"]\ntheme = \"solarized\"\n\
             zoom = \"huge\"\nfit = \"no\"\n\
             [items.inbox]\nsort = \"nope\"\ndir = \"sideways\"\n",
        );
        assert_eq!(
            bad,
            State {
                inbox: Some((InboxColumn::Id, SortDir::Asc)),
                ..State::default()
            }
        );
        let columns = State::parse("item_columns = [\"id\", \"owner\", \"id\", \"status\"]\n");
        assert_eq!(
            columns.item_columns,
            Some(vec![ItemColumn::Id, ItemColumn::Status])
        );
    }

    #[test]
    fn a_state_file_from_before_the_new_keys_parses_unchanged() {
        let legacy = "view = \"diagram\"\nsurface = \"backlog\"\n\n\
                      [items.backlog]\ngroup = \"area\"\nsort = \"newest\"\n";
        assert_eq!(
            State::parse(legacy),
            State {
                view: Some(ViewKind::Diagram),
                surface: Some(Surface::Backlog),
                arrangements: vec![(Surface::Backlog, Group::Area, Sort::Newest, SortDir::Asc)],
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
        let baseline = state.apply(&mut app, false, true, false);
        assert_eq!(app.view, ViewKind::Ego);
        assert_eq!(app.orientation_override, None, "--orientation was given");
        assert_eq!(app.split_override, Some(Split::Beside));
        assert_eq!(app.panel_percent, 60);
        assert!(app.show_implied);
        assert_eq!(State::capture(&app, &baseline).view, Some(ViewKind::Ego));

        let mut app = App::new(fixture(), &Config::default());
        state.apply(&mut app, true, false, false);
        assert_eq!(app.view, ViewKind::Layers, "--view was given");
        assert_eq!(app.orientation_override, Some(Orientation::Horizontal));
    }

    #[test]
    fn state_round_trips_surface_group_and_sort() {
        let mut app = App::new(fixture(), &Config::default());
        let baseline = State::default().apply(&mut app, false, false, false);
        app.surface = Surface::PlanReview;
        let plan = app
            .items
            .get_mut(&Surface::PlanReview)
            .expect("item surface");
        plan.group = Group::Category;
        plan.sort = Sort::Newest;
        plan.sort_dir = SortDir::Desc;
        let backlog = app.items.get_mut(&Surface::Backlog).expect("item surface");
        backlog.group = Group::Area;
        app.inbox.sort = InboxColumn::Status;
        app.inbox.sort_dir = SortDir::Desc;
        app.item_columns = vec![ItemColumn::Id, ItemColumn::Summary];
        app.diagram_zoom = Zoom::Titles;
        app.diagram_fit = false;

        let state = State::capture(&app, &baseline);
        assert_eq!(state.arrangements.len(), 4, "every item surface, not Inbox");
        assert_eq!(State::parse(&state.render()), state);

        let mut restored = App::new(fixture(), &Config::default());
        State::parse(&state.render()).apply(&mut restored, false, false, false);
        assert_eq!(restored.surface, Surface::PlanReview);
        let plan = &restored.items[&Surface::PlanReview];
        assert_eq!(
            (plan.group, plan.sort, plan.sort_dir),
            (Group::Category, Sort::Newest, SortDir::Desc)
        );
        assert_eq!(restored.items[&Surface::Backlog].group, Group::Area);
        assert_eq!(
            (restored.inbox.sort, restored.inbox.sort_dir),
            (InboxColumn::Status, SortDir::Desc)
        );
        assert_eq!(restored.item_columns, [ItemColumn::Id, ItemColumn::Summary]);
        assert_eq!(restored.diagram_zoom, Zoom::Titles);
        assert!(!restored.diagram_fit);
    }

    #[test]
    fn a_group_or_sort_the_surface_lacks_reads_as_the_default() {
        let state = State::parse(
            "surface = \"nope\"\n[items.backlog]\ngroup = \"severity\"\nsort = \"newest\"\n",
        );
        assert_eq!(state.surface, None);
        assert_eq!(
            state.arrangements,
            [(Surface::Backlog, Group::None, Sort::Newest, SortDir::Asc)]
        );
    }

    #[test]
    fn theme_and_hints_are_saved_only_once_chosen_in_app() {
        let mut app = App::new(fixture(), &Config::default());
        let baseline = State::default().apply(&mut app, false, false, false);
        let untouched = State::capture(&app, &baseline);
        assert_eq!((untouched.theme, untouched.hints), (None, None));

        app.show_set(&sets::ANSI);
        app.hints = HintMode::Full;
        let chosen = State::capture(&app, &baseline);
        assert_eq!(chosen.theme, Some(&sets::ANSI));
        assert_eq!(chosen.hints, Some(HintMode::Full));

        let mut restored = App::new(fixture(), &Config::default());
        let next = State::parse(&chosen.render()).apply(&mut restored, false, false, false);
        assert_eq!(restored.theme_set.name, "ansi");
        assert_eq!(restored.hints, HintMode::Full);
        let again = State::capture(&restored, &next);
        assert_eq!(again.theme, Some(&sets::ANSI), "kept while unchanged");
        assert_eq!(again.hints, Some(HintMode::Full));
    }

    #[test]
    fn a_choice_another_run_saved_since_survives_a_run_that_left_it_alone() {
        let mut early = App::new(fixture(), &Config::default());
        let baseline = State::default().apply(&mut early, false, false, false);
        early.view = ViewKind::Diagram;
        let since = State {
            view: Some(ViewKind::Ego),
            theme: Some(&sets::KANSO_PEARL),
            hints: Some(HintMode::Hidden),
            ..State::default()
        };
        let saved = State::over_disk(&early, &baseline, &since);
        assert_eq!(saved.theme, Some(&sets::KANSO_PEARL));
        assert_eq!(saved.hints, Some(HintMode::Hidden));
        assert_eq!(
            saved.view,
            Some(ViewKind::Diagram),
            "layout is last-quit-wins"
        );

        early.show_set(&sets::ANSI);
        early.hints = HintMode::Full;
        let saved = State::over_disk(&early, &baseline, &since);
        assert_eq!(saved.theme, Some(&sets::ANSI), "this run's own choice wins");
        assert_eq!(saved.hints, Some(HintMode::Full));
    }

    #[test]
    fn a_previewed_set_is_not_saved() {
        let mut app = App::new(fixture(), &Config::default());
        let baseline = State::default().apply(&mut app, false, false, false);
        app.theme_picker = Some(ThemePicker {
            cursor: 2,
            original: app.theme_set,
        });
        app.show_set(&sets::ANSI);
        assert_eq!(State::capture(&app, &baseline).theme, None);
    }

    #[test]
    fn the_theme_flag_beats_the_saved_set_and_is_not_saved() {
        let config = Config {
            theme_base: Some(ThemeChoice::Flag(&sets::KANSO_PEARL)),
            ..Config::default()
        };
        let saved = State {
            theme: Some(&sets::ANSI),
            ..State::default()
        };
        let mut app = App::new(fixture(), &config);
        let baseline = saved.apply(&mut app, false, false, config.keeps_theme());
        assert_eq!(app.theme_set.name, "kanso-pearl");
        assert_eq!(
            State::capture(&app, &baseline).theme,
            Some(&sets::ANSI),
            "the earlier in-app choice survives"
        );

        let mut fresh = App::new(fixture(), &config);
        let baseline = State::default().apply(&mut fresh, false, false, true);
        assert_eq!(State::capture(&fresh, &baseline).theme, None);
    }

    #[test]
    fn a_no_color_run_writes_no_theme_key() {
        let config = Config {
            no_color: true,
            ..Config::default()
        };
        let mut app = App::new(fixture(), &config);
        let baseline = State::default().apply(&mut app, false, false, true);
        assert_eq!(app.theme_set.name, "mono");
        let state = State::capture(&app, &baseline);
        assert_eq!(state.theme, None);
        assert!(!state.render().contains("theme"), "{}", state.render());
    }

    #[test]
    fn an_unknown_saved_set_is_ignored() {
        let mut app = App::new(fixture(), &Config::default());
        State::parse("theme = \"solarized\"\n").apply(&mut app, false, false, false);
        assert_eq!(app.theme_set.name, "kanso-zen");
    }
}
