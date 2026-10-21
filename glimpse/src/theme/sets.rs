//! The named theme sets a `[theme] base` selects.
//!
//! A set supplies values for the palette tokens and the rules that decide how fills and
//! the faint tiers are drawn. Element tokens keep their [`TOKENS`](super::TOKENS)
//! defaults in every set, so they follow whichever palette the set provides.

/// How a filled background — the selection, a chip, a status flash — is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fill {
    /// The background token's colour.
    Colour,
    /// Reverse video, with the background token as the foreground colour.
    Reverse,
}

/// How the de-emphasised elements (`unrelated`, `edge_faded`, `backdrop`, `tab_empty`)
/// are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Faint {
    Colour,
    /// The terminal's `DIM` attribute and no colour.
    Dim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rules {
    pub(crate) fill: Fill,
    pub(crate) faint: Faint,
    /// False drops every colour, overrides included, as `NO_COLOR` asks.
    pub(crate) colour: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThemeSet {
    pub(crate) name: &'static str,
    /// Values for palette tokens; a token absent here keeps its `TOKENS` default.
    pub(crate) palette: &'static [(&'static str, &'static str)],
    pub(crate) rules: Rules,
    /// The background and foreground the palette was designed against, for the
    /// contrast test; `None` where the terminal's palette decides.
    pub(crate) bg: Option<&'static str>,
    pub(crate) fg: Option<&'static str>,
}

const COLOUR: Rules = Rules {
    fill: Fill::Colour,
    faint: Faint::Colour,
    colour: true,
};

/// The `TOKENS` defaults are this palette, so it needs no entries of its own.
pub(crate) const KANSO_ZEN: ThemeSet = ThemeSet {
    name: "kanso-zen",
    palette: &[],
    rules: COLOUR,
    bg: Some("#090E13"),
    fg: Some("#C5C9C7"),
};

/// kanso.nvim's pearl palette (`extras/ghostty/kanso-pearl`, MIT). Hues below 4.5:1 on
/// `bg` were darkened in HSL lightness until they pass; the grey tiers mix `fg` into
/// `bg` at 70%, 50%, 30% and 10%.
///
/// | token  | source    | final     | ratio on bg |
/// |--------|-----------|-----------|-------------|
/// | danger | `#c84053` | `#c63a4e` | 4.52        |
/// | success| `#6f894e` | `#5e7442` | 4.59        |
/// | warning| `#77713f` | `#756f3e` | 4.54        |
/// | info   | `#4d699b` | `#4d699b` | 4.88        |
/// | violet | `#b35b79` | `#ac506f` | 4.50        |
/// | teal   | `#597b75` | `#54746e` | 4.54        |
/// | accent | `#cc6d00` | `#a85a00` | 4.51        |
pub(crate) const KANSO_PEARL: ThemeSet = ThemeSet {
    name: "kanso-pearl",
    palette: &[
        ("fg", "default"),
        ("accent", "#a85a00"),
        ("accent_bg", "#a85a00"),
        ("success", "#5e7442"),
        ("danger", "#c63a4e"),
        ("warning", "#756f3e"),
        ("info", "#4d699b"),
        ("violet", "#ac506f"),
        ("teal", "#54746e"),
        ("idle", "#22262D"),
        ("muted", "#606367"),
        ("faint", "#8a8c8e"),
        ("subtle", "#b4b4b5"),
        ("highlight", "#dddddc"),
        ("on_color", "#f2f1ef"),
    ],
    rules: COLOUR,
    bg: Some("#f2f1ef"),
    fg: Some("#22262D"),
};

/// ANSI colour names only, so the terminal's own palette decides every hue. Hues sit on
/// the bright slots, so a terminal that brightens bold text does not shift them.
pub(crate) const ANSI: ThemeSet = ThemeSet {
    name: "ansi",
    palette: &[
        ("fg", "default"),
        ("accent", "light-yellow"),
        ("accent_bg", "light-yellow"),
        ("success", "light-green"),
        ("danger", "light-red"),
        ("warning", "light-yellow"),
        ("info", "light-blue"),
        ("violet", "light-magenta"),
        ("teal", "light-cyan"),
        ("idle", "default"),
        ("muted", "dark-gray"),
        ("faint", "dark-gray"),
        ("subtle", "dark-gray"),
        ("highlight", "dark-gray"),
        ("on_color", "black"),
    ],
    rules: Rules {
        fill: Fill::Reverse,
        faint: Faint::Dim,
        colour: true,
    },
    bg: None,
    fg: None,
};

/// No colour at all: what `NO_COLOR` forces.
pub(crate) const MONO: ThemeSet = ThemeSet {
    name: "mono",
    palette: &[],
    rules: Rules {
        fill: Fill::Reverse,
        faint: Faint::Dim,
        colour: false,
    },
    bg: None,
    fg: None,
};

pub(crate) const SETS: &[ThemeSet] = &[KANSO_ZEN, KANSO_PEARL, ANSI, MONO];

pub(crate) fn by_name(name: &str) -> Option<&'static ThemeSet> {
    SETS.iter().find(|set| set.name == name)
}

/// Every set's name, comma-separated, for an error that lists the choices.
pub(crate) fn names() -> String {
    SETS.iter()
        .map(|set| set.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The set a run starts on: `mono` under `NO_COLOR`, else `base`, else `kanso-zen`.
pub(crate) fn starting(base: Option<&'static ThemeSet>, no_color: bool) -> &'static ThemeSet {
    if no_color {
        &MONO
    } else {
        base.unwrap_or(&KANSO_ZEN)
    }
}
