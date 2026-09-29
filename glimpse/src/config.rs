//! User configuration, its defaults, and the orientation, density and view-kind enums
//! shared across views.

use std::path::{Path, PathBuf};

/// The axis layers are laid out along, once `auto` has been resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Orientation {
    Vertical,
    Horizontal,
}

impl Orientation {
    pub(crate) fn flip(self) -> Self {
        match self {
            Orientation::Vertical => Orientation::Horizontal,
            Orientation::Horizontal => Orientation::Vertical,
        }
    }
}

/// The configured orientation, before it meets a terminal size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrientationPref {
    Auto,
    Fixed(Orientation),
}

impl OrientationPref {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(OrientationPref::Auto),
            "vertical" => Some(OrientationPref::Fixed(Orientation::Vertical)),
            "horizontal" => Some(OrientationPref::Fixed(Orientation::Horizontal)),
            _ => None,
        }
    }

    /// `Auto` goes horizontal when `width >= threshold * height`, measured in cells.
    pub(crate) fn resolve(self, width: u16, height: u16, threshold: f64) -> Orientation {
        match self {
            OrientationPref::Fixed(o) => o,
            OrientationPref::Auto if f64::from(width) >= threshold * f64::from(height) => {
                Orientation::Horizontal
            }
            OrientationPref::Auto => Orientation::Vertical,
        }
    }
}

/// How the body is shared once `auto` has met a terminal width: `Compact` gives the view
/// the whole body and opens panels as modals, `Comfortable` docks them beside or below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Density {
    Compact,
    Comfortable,
}

impl Density {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Comfortable => "comfortable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DensityPref {
    Auto,
    Fixed(Density),
}

impl DensityPref {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(DensityPref::Auto),
            "compact" => Some(DensityPref::Fixed(Density::Compact)),
            "comfortable" => Some(DensityPref::Fixed(Density::Comfortable)),
            _ => None,
        }
    }

    /// `Auto` is compact while `width < compact_below`, in cells.
    pub(crate) fn resolve(self, width: u16, compact_below: u16) -> Density {
        match self {
            DensityPref::Fixed(d) => d,
            DensityPref::Auto if width < compact_below => Density::Compact,
            DensityPref::Auto => Density::Comfortable,
        }
    }

    /// Auto, then compact, then comfortable, then auto again.
    pub(crate) fn next(self) -> Self {
        match self {
            DensityPref::Auto => DensityPref::Fixed(Density::Compact),
            DensityPref::Fixed(Density::Compact) => DensityPref::Fixed(Density::Comfortable),
            DensityPref::Fixed(Density::Comfortable) => DensityPref::Auto,
        }
    }
}

/// Bounds of the docked panel's share of the body, in percent.
pub(crate) const PANEL_PERCENT_RANGE: std::ops::RangeInclusive<u16> = 20..=70;
/// Bounds of a horizontal layers column, in cells; the low end is the narrowest that
/// still shows a status, an id and a few characters of title.
pub(crate) const COLUMN_RANGE: std::ops::RangeInclusive<u16> = 18..=120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ViewKind {
    Layers,
    Ego,
    Diagram,
}

impl ViewKind {
    const ALL: [ViewKind; 3] = [ViewKind::Layers, ViewKind::Ego, ViewKind::Diagram];

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "layers" => Some(ViewKind::Layers),
            "ego" => Some(ViewKind::Ego),
            "diagram" => Some(ViewKind::Diagram),
            _ => None,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&v| v == self).unwrap_or(0)
    }

    pub(crate) fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub(crate) fn prev(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Config {
    pub(crate) orientation: OrientationPref,
    pub(crate) orientation_threshold: f64,
    pub(crate) split_threshold: f64,
    pub(crate) pane_ratio: f64,
    pub(crate) poll_ms: u64,
    pub(crate) tomlctl: String,
    pub(crate) default_view: ViewKind,
    /// A `running` agent whose transcript is untouched this long is shown stale.
    pub(crate) stale_after_s: u64,
    pub(crate) density: DensityPref,
    /// Terminal width in cells below which `auto` density is compact.
    pub(crate) compact_below: u16,
    /// The docked panel's starting share of the body, within [`PANEL_PERCENT_RANGE`].
    pub(crate) panel_percent: u16,
    /// Widest a horizontal layers column grows to fit its titles, within [`COLUMN_RANGE`].
    pub(crate) column_max: u16,
    /// Mouse capture blocks the terminal's own text selection while glimpse runs.
    pub(crate) mouse: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            orientation: OrientationPref::Auto,
            orientation_threshold: 2.0,
            split_threshold: 2.2,
            pane_ratio: 0.4,
            poll_ms: 500,
            tomlctl: "tomlctl".to_string(),
            default_view: ViewKind::Layers,
            stale_after_s: 300,
            density: DensityPref::Auto,
            compact_below: 90,
            panel_percent: 40,
            column_max: 40,
            mouse: true,
        }
    }
}

impl Config {
    /// Keys absent from `text` keep their defaults. An unknown key, a wrong type or an
    /// out-of-range value is an error, so a typo is reported rather than silently ignored.
    pub(crate) fn parse(text: &str) -> Result<Config, String> {
        let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut cfg = Config::default();
        for (key, value) in &table {
            match key.as_str() {
                "orientation" => {
                    cfg.orientation =
                        OrientationPref::parse(str_value(key, value)?).ok_or_else(|| {
                            "`orientation` must be auto, vertical or horizontal".to_string()
                        })?;
                }
                "orientation_threshold" => {
                    cfg.orientation_threshold = positive_float(key, value)?;
                }
                "split_threshold" => cfg.split_threshold = positive_float(key, value)?,
                "pane_ratio" => {
                    let r = positive_float(key, value)?;
                    if r >= 1.0 {
                        return Err("`pane_ratio` must be between 0 and 1".to_string());
                    }
                    cfg.pane_ratio = r;
                }
                "poll_ms" => cfg.poll_ms = positive_int(key, value)?,
                "tomlctl" => {
                    let s = str_value(key, value)?;
                    if s.is_empty() {
                        return Err("`tomlctl` must not be empty".to_string());
                    }
                    cfg.tomlctl = s.to_string();
                }
                "default_view" => {
                    cfg.default_view =
                        ViewKind::parse(str_value(key, value)?).ok_or_else(|| {
                            "`default_view` must be layers, ego or diagram".to_string()
                        })?;
                }
                "stale_after_s" => cfg.stale_after_s = positive_int(key, value)?,
                "density" => {
                    cfg.density = DensityPref::parse(str_value(key, value)?).ok_or_else(|| {
                        "`density` must be auto, compact or comfortable".to_string()
                    })?;
                }
                "compact_below" => cfg.compact_below = int_in(key, value, 1..=u16::MAX)?,
                "panel_percent" => cfg.panel_percent = int_in(key, value, PANEL_PERCENT_RANGE)?,
                "column_max" => cfg.column_max = int_in(key, value, COLUMN_RANGE)?,
                "mouse" => {
                    cfg.mouse = value
                        .as_bool()
                        .ok_or_else(|| "`mouse` must be true or false".to_string())?;
                }
                other => return Err(format!("unknown key `{other}`")),
            }
        }
        Ok(cfg)
    }

    /// A missing file yields the defaults silently; an unreadable or malformed one yields the
    /// defaults plus a warning for the header.
    pub(crate) fn load_from(path: &Path) -> (Config, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match Config::parse(&text) {
                Ok(cfg) => (cfg, None),
                Err(e) => (
                    Config::default(),
                    Some(format!("{}: {e} (using defaults)", path.display())),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
            Err(e) => (
                Config::default(),
                Some(format!("{}: {e} (using defaults)", path.display())),
            ),
        }
    }

    pub(crate) fn load() -> (Config, Option<String>) {
        match config_path() {
            Some(path) => Config::load_from(&path),
            None => (Config::default(), None),
        }
    }
}

fn str_value<'a>(key: &str, value: &'a toml::Value) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("`{key}` must be a string"))
}

/// Accepts an integer too, so `orientation_threshold = 2` is not a type error.
fn positive_float(key: &str, value: &toml::Value) -> Result<f64, String> {
    let v = match value {
        toml::Value::Float(f) => *f,
        toml::Value::Integer(i) => *i as f64,
        _ => return Err(format!("`{key}` must be a number")),
    };
    if v.is_finite() && v > 0.0 {
        Ok(v)
    } else {
        Err(format!("`{key}` must be greater than 0"))
    }
}

fn positive_int(key: &str, value: &toml::Value) -> Result<u64, String> {
    match value {
        toml::Value::Integer(i) if *i > 0 => Ok(*i as u64),
        toml::Value::Integer(_) => Err(format!("`{key}` must be greater than 0")),
        _ => Err(format!("`{key}` must be an integer")),
    }
}

fn int_in(
    key: &str,
    value: &toml::Value,
    range: std::ops::RangeInclusive<u16>,
) -> Result<u16, String> {
    match value {
        toml::Value::Integer(i) => u16::try_from(*i)
            .ok()
            .filter(|v| range.contains(v))
            .ok_or_else(|| {
                format!(
                    "`{key}` must be between {} and {}",
                    range.start(),
                    range.end()
                )
            }),
        _ => Err(format!("`{key}` must be an integer")),
    }
}

/// `GLIMPSE_CONFIG` wins when set and non-empty; otherwise the home directory is
/// `USERPROFILE`, then `HOME`, with the same set-but-empty quirk as `claude_dir_from`.
fn config_path_from(
    glimpse_config: Option<&str>,
    user_profile: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(p) = glimpse_config.filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    Some(
        PathBuf::from(user_profile.or(home)?)
            .join(".config")
            .join("glimpse")
            .join("config.toml"),
    )
}

pub(crate) fn config_path() -> Option<PathBuf> {
    let glimpse_config = std::env::var("GLIMPSE_CONFIG").ok();
    let user_profile = std::env::var("USERPROFILE").ok();
    let home = std::env::var("HOME").ok();
    config_path_from(
        glimpse_config.as_deref(),
        user_profile.as_deref(),
        home.as_deref(),
    )
}

/// Pure over its inputs because `std::env::set_var` is `unsafe` in edition 2024.
/// `CLAUDE_CONFIG_DIR` is used verbatim and treats empty as unset; a set-but-empty
/// `USERPROFILE` still wins over `HOME`, matching the statusline's resolution.
fn claude_dir_from(
    config: Option<&str>,
    user_profile: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(d) = config.filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    Some(PathBuf::from(user_profile.or(home)?).join(".claude"))
}

pub(crate) fn claude_dir() -> Option<PathBuf> {
    let config = std::env::var("CLAUDE_CONFIG_DIR").ok();
    let user_profile = std::env::var("USERPROFILE").ok();
    let home = std::env::var("HOME").ok();
    claude_dir_from(config.as_deref(), user_profile.as_deref(), home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let cfg = Config::parse("").expect("empty parses");
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.orientation, OrientationPref::Auto);
        assert_eq!(cfg.orientation_threshold, 2.0);
        assert_eq!(cfg.split_threshold, 2.2);
        assert_eq!(cfg.pane_ratio, 0.4);
        assert_eq!(cfg.poll_ms, 500);
        assert_eq!(cfg.tomlctl, "tomlctl");
        assert_eq!(cfg.default_view, ViewKind::Layers);
        assert_eq!(cfg.stale_after_s, 300);
        assert_eq!(cfg.density, DensityPref::Auto);
        assert_eq!(cfg.compact_below, 90);
        assert_eq!(cfg.panel_percent, 40);
        assert_eq!(cfg.column_max, 40);
        assert!(cfg.mouse);
    }

    #[test]
    fn layout_keys_parse_and_reject_out_of_range_values() {
        let cfg = Config::parse(
            "density = \"comfortable\"\ncompact_below = 60\npanel_percent = 55\ncolumn_max = 30\nmouse = false\n",
        )
        .expect("layout keys parse");
        assert_eq!(cfg.density, DensityPref::Fixed(Density::Comfortable));
        assert_eq!(cfg.compact_below, 60);
        assert_eq!(cfg.panel_percent, 55);
        assert_eq!(cfg.column_max, 30);
        assert!(!cfg.mouse);
        for bad in [
            "density = \"cosy\"",
            "panel_percent = 15",
            "panel_percent = 75",
            "column_max = 10",
            "compact_below = 0",
            "mouse = 1",
        ] {
            assert!(Config::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn auto_density_turns_compact_below_the_threshold() {
        let auto = DensityPref::Auto;
        assert_eq!(auto.resolve(89, 90), Density::Compact);
        assert_eq!(auto.resolve(90, 90), Density::Comfortable);
        let fixed = DensityPref::Fixed(Density::Comfortable);
        assert_eq!(fixed.resolve(40, 90), Density::Comfortable);
        assert_eq!(auto.next().next().next(), auto, "the cycle has three steps");
    }

    #[test]
    fn a_partial_file_overrides_only_its_keys() {
        let cfg = Config::parse(
            "orientation = \"horizontal\"\ndefault_view = \"diagram\"\norientation_threshold = 3\n",
        )
        .expect("partial parses");
        assert_eq!(
            cfg.orientation,
            OrientationPref::Fixed(Orientation::Horizontal)
        );
        assert_eq!(cfg.default_view, ViewKind::Diagram);
        assert_eq!(cfg.orientation_threshold, 3.0, "an integer is accepted");
        assert_eq!(cfg.poll_ms, 500);
        assert_eq!(cfg.tomlctl, "tomlctl");
    }

    #[test]
    fn malformed_or_invalid_text_is_an_error() {
        for bad in [
            "orientation = ",
            "orientation = \"diagonal\"",
            "poll_ms = \"fast\"",
            "poll_ms = 0",
            "pane_ratio = 1.5",
            "tomlctl = \"\"",
            "pollms = 100",
        ] {
            assert!(Config::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn a_malformed_file_loads_defaults_with_a_warning() {
        let dir = std::env::temp_dir().join(format!("glimpse-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("config.toml");
        std::fs::write(&path, "poll_ms = [").expect("write");
        let (cfg, warning) = Config::load_from(&path);
        assert_eq!(cfg, Config::default());
        let warning = warning.expect("a malformed file warns");
        assert!(warning.contains("config.toml"), "{warning}");

        let (cfg, warning) = Config::load_from(&dir.join("absent.toml"));
        assert_eq!(cfg, Config::default());
        assert_eq!(warning, None, "a missing file is not an error");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claude_dir_from_prefers_its_variable_then_userprofile_then_home() {
        assert_eq!(
            claude_dir_from(Some("/cfg"), Some("/up"), Some("/h")),
            Some(PathBuf::from("/cfg"))
        );
        assert_eq!(
            claude_dir_from(Some(""), Some("/up"), Some("/h")),
            Some(PathBuf::from("/up").join(".claude"))
        );
        assert_eq!(
            claude_dir_from(None, None, Some("/h")),
            Some(PathBuf::from("/h").join(".claude"))
        );
        assert_eq!(
            claude_dir_from(None, Some(""), Some("/h")),
            Some(PathBuf::from(".claude"))
        );
        assert_eq!(claude_dir_from(None, None, None), None);
    }

    #[test]
    fn config_path_from_prefers_glimpse_config_then_home() {
        assert_eq!(
            config_path_from(Some("/x.toml"), Some("/up"), None),
            Some(PathBuf::from("/x.toml"))
        );
        assert_eq!(
            config_path_from(Some(""), None, Some("/h")),
            Some(PathBuf::from("/h/.config/glimpse/config.toml"))
        );
        assert_eq!(config_path_from(None, None, None), None);
    }

    #[test]
    fn auto_orientation_turns_horizontal_at_the_threshold() {
        let auto = OrientationPref::Auto;
        assert_eq!(auto.resolve(80, 40, 2.0), Orientation::Horizontal);
        assert_eq!(auto.resolve(79, 40, 2.0), Orientation::Vertical);
        let fixed = OrientationPref::Fixed(Orientation::Vertical);
        assert_eq!(fixed.resolve(200, 10, 2.0), Orientation::Vertical);
        assert_eq!(Orientation::Vertical.flip(), Orientation::Horizontal);
    }

    #[test]
    fn view_kinds_cycle_both_ways() {
        assert_eq!(ViewKind::Layers.next(), ViewKind::Ego);
        assert_eq!(ViewKind::Diagram.next(), ViewKind::Layers);
        assert_eq!(ViewKind::Layers.prev(), ViewKind::Diagram);
        assert_eq!(ViewKind::Ego.prev(), ViewKind::Layers);
    }
}
