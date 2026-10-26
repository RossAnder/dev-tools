//! The named theme sets a `[theme] base` selects.
//!
//! A set supplies values for the palette tokens and the rules that decide how fills and
//! the faint tiers are drawn. Element tokens keep their [`TOKENS`](super::TOKENS)
//! defaults, so they follow whichever palette the set provides, unless a set repoints one
//! to spread its hues. A set may name a light twin, shown while the system prefers light.

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
    /// Values for tokens, palette tokens first; a token absent here keeps its `TOKENS`
    /// default.
    pub(crate) palette: &'static [(&'static str, &'static str)],
    pub(crate) rules: Rules,
    /// The background and foreground the palette was designed against, for the
    /// contrast test; `None` where the terminal's palette decides.
    pub(crate) bg: Option<&'static str>,
    pub(crate) fg: Option<&'static str>,
    /// Shown in this set's place while the system prefers light; not offered on its own.
    pub(crate) light: Option<&'static ThemeSet>,
}

impl ThemeSet {
    /// The set to draw with: the light twin when `light` and there is one, else this set.
    pub(crate) fn for_appearance(&'static self, light: bool) -> &'static ThemeSet {
        match self.light {
            Some(twin) if light => twin,
            _ => self,
        }
    }
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
    light: None,
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
    light: None,
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
    light: None,
};

/// dev-tools' eggshell palette (`themes/palette.toml`) for its dark background, with
/// [`EGGSHELL_LIGHT`] as its light twin. Mostly neutral: text in high-contrast greys
/// (rows unrelated to the selection are not faded), a mid grey for routine marks and
/// edges, the blue text-selection fill for the cursor row, and colour kept to touches. Glyphs take the light colour tiers (status marks, the selection and item
/// marks, needed-by edges); text that carries colour takes the strong tiers.
pub(crate) const EGGSHELL: ThemeSet = ThemeSet {
    name: "eggshell",
    palette: &[
        ("fg", "default"),
        ("accent", "#c4d5ff"),
        ("accent_bg", "#c8c8c8"),
        ("success", "#72bf89"),
        ("danger", "#e5715a"),
        ("warning", "#fea450"),
        ("info", "#c4d5ff"),
        ("violet", "#c4d5ff"),
        ("teal", "#fea450"),
        ("idle", "#9e9e9e"),
        ("muted", "#9e9e9e"),
        ("faint", "#5e5e5e"),
        ("subtle", "#2e2e2e"),
        ("highlight", "#12276e"),
        ("on_color", "#0a0a0a"),
        ("status_in_progress", "#c4d5ff"),
        ("unrelated", "default"),
        ("selection_mark", "#e5715a"),
        ("item_mark", "#e5715a"),
        ("edge_needs", "#e5715a"),
        ("badge", "#e5715a"),
        ("edge", "#7a7a7a"),
        ("slug", "#f2f2f2"),
        ("strong", "#f2f2f2"),
        ("section", "#f2f2f2"),
        ("border_title", "#f2f2f2"),
        ("table_header", "muted"),
        ("sort_mark", "muted"),
        ("selection_text", "#c8c8c8"),
        ("agent_bg", "#33150f"),
        ("agent_fg", "#fb846c"),
        ("question", "warning"),
    ],
    rules: COLOUR,
    bg: Some("#0a0a0a"),
    fg: Some("#c8c8c8"),
    light: Some(&EGGSHELL_LIGHT),
};

/// [`EGGSHELL`] on the light background, role for role: near-black text, the palette's
/// light colour tiers for glyphs (below 4.5:1 by design, see the contrast test's `SOFT`),
/// and its strong tiers wherever colour carries text.
pub(crate) const EGGSHELL_LIGHT: ThemeSet = ThemeSet {
    name: "eggshell",
    palette: &[
        ("fg", "default"),
        ("accent", "#092382"),
        ("accent_bg", "#302d2b"),
        ("success", "#739f7c"),
        ("danger", "#e5715a"),
        ("warning", "#b28966"),
        ("info", "#7e92be"),
        ("violet", "#092382"),
        ("teal", "#b28966"),
        ("idle", "#6b6865"),
        ("muted", "#6b6865"),
        ("faint", "#a19e9a"),
        ("subtle", "#cdcac6"),
        ("highlight", "#c4d5ff"),
        ("on_color", "#f1efed"),
        ("status_in_progress", "#7e92be"),
        ("status_pending", "#a19e9a"),
        ("unrelated", "default"),
        ("status_deferred", "#b28966"),
        ("flash_fg", "#171717"),
        ("selection_mark", "#e5715a"),
        ("item_mark", "#e5715a"),
        ("edge_needs", "#e5715a"),
        ("badge", "#e5715a"),
        ("edge", "#6b6865"),
        ("slug", "#171717"),
        ("strong", "#171717"),
        ("section", "#171717"),
        ("border_title", "#171717"),
        ("table_header", "muted"),
        ("sort_mark", "muted"),
        ("selection_text", "#4f4d49"),
        ("agent_bg", "#fbe1db"),
        ("agent_fg", "#8d2816"),
        ("severity_critical", "#8d2816"),
        ("severity_warning", "#935616"),
        ("severity_suggestion", "#1a3bb3"),
        ("warning_text", "#935616"),
        ("form_error", "#8d2816"),
        ("question", "#935616"),
        ("input_new", "#1a3bb3"),
    ],
    rules: COLOUR,
    bg: Some("#f1efed"),
    fg: Some("#302d2b"),
    light: None,
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
    light: None,
};

pub(crate) const SETS: &[ThemeSet] = &[KANSO_ZEN, KANSO_PEARL, EGGSHELL, ANSI, MONO];

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
