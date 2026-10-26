//! Colour tokens and the styles built from them.
//!
//! [`TOKENS`] lists every token with its default. Palette tokens hold colours; element
//! tokens name a UI feature and default to a palette token by name, so a `[theme]`
//! table can repaint one feature or the whole palette. A value is a colour (`#rrggbb`,
//! an ANSI name, an index `0`–`255`, or `default` for the terminal's own) or another
//! token's name. A [`sets::ThemeSet`] supplies the palette and the drawing rules; with
//! `NO_COLOR` set the `mono` set drops every colour and backgrounds fall back to reverse
//! video.

pub(crate) mod sets;

use std::collections::HashMap;
use std::str::FromStr;

use ratatui::style::{Color, Modifier, Style};

use crate::model::TaskStatus;
use sets::{Faint, Fill, ThemeSet};

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
    ("slug", "accent"),
    ("secondary", "muted"),
    ("key", "accent"),
    ("key_label", "muted"),
    ("key_separator", "subtle"),
    ("notice", "accent"),
    ("section", "info"),
    ("code", "accent"),
    ("effort", "muted"),
    ("effort_warning", "warning"),
    ("effort_danger", "danger"),
    ("warning_text", "warning"),
    ("severity_critical", "danger"),
    ("severity_warning", "warning"),
    ("severity_suggestion", "info"),
    ("item_live", "fg"),
    ("item_parked", "idle"),
    ("item_done", "success"),
    ("item_declined", "muted"),
    ("item_mark", "accent"),
    ("item_saving", "warning"),
    ("badge", "accent"),
    ("surface_tab", "secondary"),
    ("surface_tab_active", "on_color"),
    ("surface_tab_active_bg", "accent_bg"),
    ("tab_empty", "faint"),
    ("table_header", "muted"),
    ("sort_mark", "accent"),
    ("strong", "accent"),
    ("backdrop", "faint"),
    ("selection_text", "idle"),
    ("item_id", "muted"),
    ("item_kind", "muted"),
    ("facet", "muted"),
    ("group_header", "section"),
    ("input_new", "info"),
    ("input_acknowledged", "warning"),
    ("input_handled", "success"),
    ("question", "violet"),
    ("form_label", "secondary"),
    ("form_focus", "accent"),
    ("form_error", "danger"),
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
    let empty = HashMap::new();
    let layers = Layers {
        per_set: &empty,
        overrides,
        palette: &[],
    };
    for (name, _) in TOKENS {
        resolve(name, &layers)?;
    }
    Ok(())
}

/// Where a token's value comes from, first match winning: `[theme.<set>]`, `[theme]`,
/// the set's palette, then [`TOKENS`].
struct Layers<'a> {
    per_set: &'a HashMap<String, String>,
    overrides: &'a HashMap<String, String>,
    palette: &'a [(&'a str, &'a str)],
}

impl<'a> Layers<'a> {
    fn raw(&self, name: &str) -> Option<&'a str> {
        let pick = |pairs: &'a [(&'a str, &'a str)]| {
            pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
        };
        self.per_set
            .get(name)
            .or_else(|| self.overrides.get(name))
            .map(String::as_str)
            .or_else(|| pick(self.palette))
            .or_else(|| pick(TOKENS))
    }
}

fn resolve(name: &str, layers: &Layers) -> Result<Color, String> {
    let mut at = name;
    for _ in 0..MAX_DEPTH {
        let value = layers
            .raw(at)
            .ok_or_else(|| format!("unknown theme token `{at}`"))?;
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
    /// Bold emphasis in the `strong` colour.
    pub(crate) badge: Style,
    /// Column titles above a table, underlined.
    pub(crate) table_header: Style,
    /// The ▲/▼ beside the sorted column.
    pub(crate) sort_mark: Style,
    /// Emphasis that must show without bold: markdown strong, hidden-node chips.
    pub(crate) strong: Style,
    /// The frame behind a modal overlay.
    pub(crate) backdrop: Style,
    /// Muted and faint spans on the selected row.
    pub(crate) selection_text: Style,
    /// A surface tab whose ledger is absent.
    pub(crate) tab_empty: Style,
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
    /// The effort mark of a task whose record holds a warning: a deviation, a deferral,
    /// an escalation or a retry.
    pub(crate) effort_warning: Style,
    /// The effort mark of a task that failed, or whose record holds a failure.
    pub(crate) effort_danger: Style,
    pub(crate) severity_critical: Style,
    pub(crate) severity_warning: Style,
    pub(crate) severity_suggestion: Style,
    pub(crate) item_live: Style,
    pub(crate) item_parked: Style,
    pub(crate) item_done: Style,
    pub(crate) item_declined: Style,
    /// The mark beside an item picked for a bulk action.
    pub(crate) item_mark: Style,
    /// An item whose write has not yet shown up in its ledger.
    pub(crate) item_saving: Style,
    /// The `+N` arrival count on a surface tab.
    pub(crate) arrival_badge: Style,
    pub(crate) surface_tab: Style,
    /// The current surface's tab, on a filled background.
    pub(crate) surface_tab_active: Style,
    /// An item row's id.
    pub(crate) item_id: Style,
    /// A backlog item's kind.
    pub(crate) item_kind: Style,
    /// A secondary attribute shown beside an item: kind, area, file.
    pub(crate) facet: Style,
    pub(crate) group_header: Style,
    pub(crate) input_new: Style,
    pub(crate) input_acknowledged: Style,
    pub(crate) input_handled: Style,
    pub(crate) question: Style,
    pub(crate) form_label: Style,
    pub(crate) form_focus: Style,
    pub(crate) form_error: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::build(&HashMap::new(), false)
    }
}

impl Theme {
    /// `kanso-zen` with `overrides`, or `mono` when `no_color`.
    pub(crate) fn build(overrides: &HashMap<String, String>, no_color: bool) -> Theme {
        Theme::from_set(sets::starting(None, no_color), overrides, &HashMap::new())
    }

    /// `overrides` (`[theme]`) and `per_set` (`[theme.<set>]`) must already have passed
    /// [`validate`]; a token that still fails to resolve takes the terminal's default
    /// colour.
    pub(crate) fn from_set(
        set: &ThemeSet,
        overrides: &HashMap<String, String>,
        per_set: &HashMap<String, String>,
    ) -> Theme {
        let layers = Layers {
            per_set,
            overrides,
            palette: set.palette,
        };
        let rules = set.rules;
        let color = |name: &str| -> Option<Color> {
            if !rules.colour {
                return None;
            }
            resolve(name, &layers).ok()
        };
        let fg = |name: &str| match color(name) {
            Some(c) => Style::new().fg(c),
            None => Style::new(),
        };
        let reverse = rules.fill == Fill::Reverse;
        // A background that vanishes, or that the set fills by reverse video, is drawn
        // reversed with the background token as the foreground.
        let on = |fg_name: &str, bg_name: &str| match (color(fg_name), color(bg_name)) {
            (Some(f), Some(b)) if !reverse => Style::new().fg(f).bg(b),
            _ => fg(bg_name).add_modifier(Modifier::REVERSED),
        };
        let faint = |name: &str| match rules.faint {
            Faint::Colour => fg(name),
            Faint::Dim => Style::new().add_modifier(Modifier::DIM),
        };
        let bold = Modifier::BOLD;
        let flash = |status: &str| on("flash_fg", status);
        let selection = match color("selection_bg") {
            Some(bg) if !reverse => Style::new().bg(bg).add_modifier(bold),
            _ => Style::new().add_modifier(Modifier::REVERSED | bold),
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
            unrelated: faint("unrelated"),
            edge: fg("edge"),
            edge_faded: faint("edge_faded"),
            needs_edge: fg("edge_needs").add_modifier(bold),
            coupling_edge: fg("edge_coupling").add_modifier(bold),
            out_edge: fg("edge_out").add_modifier(bold),
            overlap: fg("edge_overlap").add_modifier(bold),
            agent_chip,
            agent_chip_stale: agent_chip.add_modifier(Modifier::DIM),
            badge: fg("strong").add_modifier(bold),
            table_header: fg("table_header").add_modifier(Modifier::UNDERLINED),
            sort_mark: fg("sort_mark").add_modifier(bold),
            strong: fg("strong").add_modifier(bold),
            backdrop: faint("backdrop"),
            selection_text: fg("selection_text"),
            tab_empty: faint("tab_empty"),
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
            effort_warning: fg("effort_warning").add_modifier(bold),
            effort_danger: fg("effort_danger").add_modifier(bold),
            severity_critical: fg("severity_critical").add_modifier(bold),
            severity_warning: fg("severity_warning"),
            severity_suggestion: fg("severity_suggestion"),
            item_live: fg("item_live"),
            item_parked: fg("item_parked"),
            item_done: fg("item_done"),
            item_declined: fg("item_declined"),
            item_mark: fg("item_mark").add_modifier(bold),
            item_saving: fg("item_saving"),
            arrival_badge: fg("badge").add_modifier(bold),
            surface_tab: fg("surface_tab"),
            surface_tab_active: on("surface_tab_active", "surface_tab_active_bg")
                .add_modifier(bold),
            item_id: fg("item_id"),
            item_kind: fg("item_kind"),
            facet: fg("facet"),
            group_header: fg("group_header").add_modifier(bold),
            input_new: fg("input_new"),
            input_acknowledged: fg("input_acknowledged"),
            input_handled: fg("input_handled"),
            question: fg("question"),
            form_label: fg("form_label"),
            form_focus: fg("form_focus").add_modifier(bold),
            form_error: fg("form_error"),
        }
    }

    /// Keyed by a finding's severity; an unknown severity takes the suggestion style.
    pub(crate) fn severity(&self, severity: &str) -> Style {
        match severity {
            "critical" => self.severity_critical,
            "warning" => self.severity_warning,
            _ => self.severity_suggestion,
        }
    }

    /// Keyed by status class (`live`, `parked`, `done`, `declined`); an unknown class
    /// takes the live style.
    pub(crate) fn item_status(&self, class: &str) -> Style {
        match class {
            "parked" => self.item_parked,
            "done" => self.item_done,
            "declined" => self.item_declined,
            _ => self.item_live,
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

    /// A muted `style`, replaced by `selection_text` on the cursor row so it stays
    /// legible on the selection fill.
    pub(crate) fn on_cursor(&self, style: Style, is_cursor: bool) -> Style {
        if is_cursor {
            self.selection_text
        } else {
            style
        }
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
    fn item_tokens_default_to_palette_colours() {
        let t = Theme::default();
        assert_eq!(t.severity("critical").fg, Some(rgb("#FF8080")));
        assert_eq!(t.severity("warning").fg, Some(rgb("#E6C384")));
        assert_eq!(t.severity("other"), t.severity_suggestion);
        assert_eq!(t.item_status("done").fg, Some(rgb("#69DB7C")));
        assert_eq!(t.item_status("parked").fg, Some(rgb("#C5C9C7")));
        assert_eq!(t.item_status("declined").fg, Some(rgb("#8A8F98")));
        assert_eq!(t.item_status("live").fg, Some(Color::Reset));
        assert_eq!(t.question.fg, Some(rgb("#938AA9")));
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
    fn emphasis_tokens_carry_colour_and_the_active_tab_floods() {
        let t = Theme::default();
        assert_eq!(t.badge.fg, Some(rgb("#FFC799")));
        assert_eq!(t.strong.fg, Some(rgb("#FFC799")));
        assert_eq!(t.slug.fg, Some(rgb("#FFC799")));
        assert_eq!(t.sort_mark.fg, Some(rgb("#FFC799")));
        assert_eq!(t.surface_tab_active.fg, Some(rgb("#14171C")));
        assert_eq!(t.surface_tab_active.bg, Some(rgb("#B6927B")));
        assert!(t.table_header.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(t.tab_empty.fg, Some(rgb("#6C7380")));
        assert_eq!(t.selection_text.fg, Some(rgb("#C5C9C7")));
        let mono = Theme::build(&HashMap::new(), true);
        assert!(mono.backdrop.add_modifier.contains(Modifier::DIM));
        assert_eq!(mono.selection_text.fg, None);
    }

    #[test]
    fn every_set_is_found_by_name() {
        for name in ["kanso-zen", "kanso-pearl", "eggshell", "ansi", "mono"] {
            assert_eq!(sets::by_name(name).map(|s| s.name), Some(name));
        }
        assert!(sets::by_name("kanso").is_none());
    }

    #[test]
    fn mono_drops_every_colour_overrides_included() {
        let over = HashMap::from([("accent".to_string(), "#123456".to_string())]);
        let mono = Theme::from_set(&sets::MONO, &over, &over);
        assert_eq!(mono, Theme::build(&HashMap::new(), true));
        assert!(
            !format!("{mono:?}").contains("Color::"),
            "mono carries no colour"
        );
        assert_eq!(mono.selection, Style::new().reversed().bold());
        assert_eq!(mono.flash(TaskStatus::Done), Style::new().reversed());
        assert_eq!(mono.surface_tab_active, Style::new().reversed().bold());
        for dim in [
            mono.unrelated,
            mono.edge_faded,
            mono.backdrop,
            mono.tab_empty,
        ] {
            assert_eq!(dim, Style::new().dim());
        }
        assert_eq!(mono.border, Style::new(), "only the faint elements dim");
    }

    #[test]
    fn a_set_layers_under_theme_and_per_set_overrides() {
        let map = |k: &str, v: &str| HashMap::from([(k.to_string(), v.to_string())]);
        let pearl = sets::by_name("kanso-pearl").expect("pearl");
        let plain = Theme::from_set(pearl, &HashMap::new(), &HashMap::new());
        assert_eq!(plain.done.fg, Some(rgb("#5e7442")));
        assert_eq!(
            plain.checkpoint.fg,
            Some(rgb("#ac506f")),
            "element defaults"
        );
        let theme = Theme::from_set(pearl, &map("success", "#010101"), &HashMap::new());
        assert_eq!(theme.done.fg, Some(rgb("#010101")));
        let both = Theme::from_set(
            pearl,
            &map("success", "#010101"),
            &map("success", "#020202"),
        );
        assert_eq!(both.done.fg, Some(rgb("#020202")), "[theme.<set>] wins");

        let ansi = Theme::from_set(&sets::ANSI, &HashMap::new(), &HashMap::new());
        assert_eq!(ansi.done.fg, Some(Color::LightGreen));
        assert_eq!(
            ansi.flash(TaskStatus::Done),
            Style::new().fg(Color::LightGreen).reversed()
        );
        assert_eq!(ansi.selection, Style::new().reversed().bold());
        assert_eq!(ansi.unrelated, Style::new().dim());
    }

    #[test]
    fn eggshell_keeps_text_neutral_and_colour_to_glyphs_and_swaps_to_its_light_twin() {
        let dark = Theme::from_set(&sets::EGGSHELL, &HashMap::new(), &HashMap::new());
        assert_eq!(dark.section.fg, Some(rgb("#f2f2f2")), "titles are neutral");
        assert_eq!(
            dark.pending.fg,
            Some(rgb("#9e9e9e")),
            "routine marks recede"
        );
        assert_eq!(dark.unrelated.fg, Some(Color::Reset), "rows are not faded");
        assert_eq!(dark.selection_mark.fg, Some(rgb("#e5715a")));
        assert_eq!(dark.selection.bg, Some(rgb("#12276e")));

        assert!(std::ptr::eq(
            sets::EGGSHELL.for_appearance(false),
            &sets::EGGSHELL
        ));
        let twin = sets::EGGSHELL.for_appearance(true);
        assert!(std::ptr::eq(twin, &sets::EGGSHELL_LIGHT));
        let light = Theme::from_set(twin, &HashMap::new(), &HashMap::new());
        assert_eq!(light.section.fg, Some(rgb("#171717")));
        assert_eq!(
            light.done.fg,
            Some(rgb("#739f7c")),
            "a glyph takes the light tier"
        );
        assert_eq!(
            light.severity_critical.fg,
            Some(rgb("#8d2816")),
            "text the strong one"
        );
        assert_eq!(
            light.selection.bg,
            Some(rgb("#c4d5ff")),
            "lavender, as everywhere"
        );
        assert!(
            std::ptr::eq(sets::KANSO_ZEN.for_appearance(true), &sets::KANSO_ZEN),
            "a set without a twin ignores the preference"
        );
    }

    /// WCAG 2 relative luminance of an RGB colour.
    fn luminance(c: Color) -> f64 {
        let Color::Rgb(r, g, b) = c else {
            panic!("{c:?} is not an RGB colour");
        };
        let lin = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// Faint tiers, and the elements that default to them, are meant to recede.
    const EXEMPT: &[&str] = &[
        "faint",
        "subtle",
        "border",
        "layer_rule",
        "key_separator",
        "edge_faded",
        "unrelated",
        "tab_empty",
        "backdrop",
    ];

    /// Colours a set keeps below 4.5:1 on its background by choice, as (background, colour):
    /// eggshell's light-tier glyph colours on its light background.
    const SOFT: &[(&str, &str)] = &[
        ("#f1efed", "#e5715a"),
        ("#f1efed", "#739f7c"),
        ("#f1efed", "#7e92be"),
        ("#f1efed", "#b28966"),
        ("#f1efed", "#a19e9a"),
    ];

    /// Backgrounds, and the foregrounds drawn only on them, checked as pairs instead.
    const FILLS: &[&str] = &[
        "accent_bg",
        "highlight",
        "selection_bg",
        "agent_bg",
        "surface_tab_active_bg",
        "on_color",
        "flash_fg",
        "agent_fg",
        "surface_tab_active",
    ];

    #[test]
    fn rgb_sets_meet_text_contrast() {
        let empty = HashMap::new();
        let mut failures = Vec::new();
        let twins = sets::SETS.iter().filter_map(|set| set.light);
        for set in sets::SETS.iter().chain(twins) {
            let (Some(bg), Some(fg)) = (set.bg, set.fg) else {
                continue;
            };
            let (bg, fg) = (rgb(bg), rgb(fg));
            let layers = Layers {
                per_set: &empty,
                overrides: &empty,
                palette: set.palette,
            };
            let colour = |name: &str| match resolve(name, &layers).expect(name) {
                Color::Reset => fg,
                c => c,
            };
            let mut check = |text: &str, under: Option<&str>| {
                let back = under.map_or(bg, colour);
                if SOFT
                    .iter()
                    .any(|(b, c)| rgb(b) == back && rgb(c) == colour(text))
                {
                    return;
                }
                let ratio = contrast(colour(text), back);
                if ratio < 4.5 {
                    let on = under.unwrap_or("bg");
                    failures.push(format!(
                        "{} on {}: {text} on {on} is {ratio:.2}",
                        set.name,
                        set.bg.unwrap_or_default()
                    ));
                }
            };
            for (name, _) in TOKENS {
                if !EXEMPT.contains(name) && !FILLS.contains(name) {
                    check(name, None);
                }
            }
            check("selection_text", Some("selection_bg"));
            check("on_color", Some("accent_bg"));
            check("agent_fg", Some("agent_bg"));
            check("surface_tab_active", Some("surface_tab_active_bg"));
            for status in [
                "status_pending",
                "status_in_progress",
                "status_done",
                "status_failed",
                "status_deferred",
            ] {
                check("flash_fg", Some(status));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
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
