# eggshell themes

Configs that dress the apps around the shell in the shell's look: niri,
Alacritty, Ghostty, Neovim, tmux, fish, Firefox, GTK 4 / libadwaita, Claude Code,
herdr and gitui. The design reasoning behind them, and the reference it was distilled
from, is in [docs/DESIGN.md](../docs/DESIGN.md).

| Path | Contents | Variants |
|---|---|---|
| `palette.toml` | Every colour, by role, including the ANSI dim tier and the inverse panel | dark, light |
| `niri/eggshell-theme.kdl` | Window border (the focus blue on the focused window), shadow, corner radius, tab indicator, overview, and animations tuned to `Core/Motion.qml`'s springs | dark |
| `alacritty/eggshell.toml`, `eggshell-light.toml` | Colours: 16 ANSI, cursor, selection, search, hints | dark, light |
| `ghostty/eggshell`, `eggshell-light` | Ghostty theme files (16 ANSI, background, cursor, selection), the same values as the Alacritty themes; for desktop Ghostty and Ghostty-based terminals such as Rootshell on iOS. Not linked by `deploy`: copy them to `~/.config/ghostty/themes/`, or into the app, and pair them with `theme = light:eggshell-light,dark:eggshell` | dark, light |
| `nvim/` | A colorscheme plugin, `:colorscheme eggshell`, following `'background'`; lualine theme | dark, light |
| `tmux/` | `eggshell.conf` (load this), `styles.conf`, and `dark.conf` / `light.conf` palettes it switches between as each client reports its background: status line, panes, copy mode, menus, choosers | dark, light |
| `fish/eggshell.theme` | Syntax highlighting and the completion pager; fish picks the variant from the background the terminal reports | dark, light |
| `firefox/` | `chrome/userChrome.css` and `chrome/userContent.css` for Firefox's Proton and Nova UIs (Nova rules sit under `@media -moz-pref("browser.nova.enabled")`), and a static theme add-on in `theme/` whose `dark_theme` follows the system scheme | dark, light |
| `gtk-4.0/eggshell.css` | libadwaita colours (named variables and the legacy `@define-color` names), imported from your own `~/.config/gtk-4.0/gtk.css` | dark, light |
| `claude/eggshell.json`, `eggshell-light.json` | Claude Code custom themes over its `dark` and `light` presets: near-black text with lighter greys for secondary content, the salmon spinner and a dark-red mascot, light-tier status glyphs, and diffs with no line background (only changed words tinted), subagent and `ultrathink` colours. Claude Code picks diff syntax colours from the preset (Monokai or GitHub), so `syntaxHighlightingDisabled` keeps diffs to these tints and grey text. Custom themes never follow the system; select `Eggshell (system)`, the copy `windows/follow-system.vbs` keeps current | dark, light |
| `herdr/eggshell.toml` | herdr colour tokens for both variants, with `auto_switch` following the terminal's background. Not linked by `deploy`, because herdr reads its theme only from `config.toml`: copy the file into `~/.config/herdr/config.toml` in place of its `[theme]` table | dark, light |
| `gitui/theme-dark.ron`, `theme-light.ron`, `eggshell-dark.tmTheme`, `eggshell-light.tmTheme` | gitui colours and syntax colours for its file view; `windows/follow-system.vbs` copies the matching pair to `theme.ron` and `eggshell.tmTheme` in gitui's config dir | dark, light |
| `windows/follow-system.vbs`, `install-follow-system.ps1` | Copies the variant matching Windows' app mode over the files that cannot switch themselves (Claude Code's `eggshell-system.json`, gitui's `theme.ron` and `eggshell.tmTheme`) and keeps the variant-free copies current; the script registers it as the `Eggshell Follow System` task, at logon and every minute (`-Remove` unregisters) | dark, light |

## Deploying

```sh
scripts/themes.sh status            # what is linked where
scripts/themes.sh deploy [app...]   # link apps (all when none given)
scripts/themes.sh remove [app...]   # remove the links this script made
scripts/themes.sh check             # every colour is a palette value
```

`deploy` only ever creates symlinks: it links the eggshell-named files (and
Firefox's `chrome/` directory) into each app's config, and refuses to replace
a file, or a symlink that does not already point into `themes/`. It never
edits an app's own config. Where the app needs a line in its config to load
the theme, it prints that line: for GTK, `@import url("eggshell.css");` in
`~/.config/gtk-4.0/gtk.css`; for Firefox,
`user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);` in
the profile's `user.js`. Claude Code's themes go to `$CLAUDE_CONFIG_DIR/themes`
(`~/.claude/themes` when it is unset). Apps: `niri`, `alacritty`, `nvim`,
`tmux`, `fish`, `firefox`, `gtk4`, `claude`.

`check` also checks that every copy named after a token (Neovim's
`palette.lua`, GTK's `egg_<variant>_<token>`, Firefox's `--egg-<d|l>-<token>`)
holds that token's value, and that every token marked `(Theme.<name>)` in
`palette.toml` equals `Core/Theme.qml`'s: the dark neutrals, `[dark.inverse]`,
and the light tokens the weather card's charts draw with.

## Following the system

On Windows, Windows Terminal switches between its `Eggshell` and `Eggshell Light`
schemes with the system's app mode (`colorScheme` and `theme` set as `{dark, light}`
pairs). Everything inside it follows from that:

- herdr's `auto_switch` picks its variant from the background the terminal reports.
- glimpse's `eggshell` set watches the system setting itself and swaps at once.
- Claude Code and gitui take RGB colours only, so `windows/follow-system.vbs` swaps
  their files within a minute of the switch. Claude Code reloads its theme at once;
  gitui reads its theme when it starts.

## Roles

The same role gets the same token in every app. Dark values first, light
second.

### Surfaces

| Role | Dark | Light |
|---|---|---|
| Desktop, island, niri background | `black` | — |
| App background | `base` | `base` |
| Recessed panel: sidebar, header bar, tab strip | `black` | `sunk` |
| Raised: float, popup, menu, status line | `raised` | `raised` |
| Card | `raised` | `white` |
| Current row, hovered control | `active` | `sunk` |
| Divider between surfaces of different tone: header bar edge, a float against the base | `hairline` | `hairline` |
| Separator between splits, panes or windows of one surface | `edge` | `edge` |
| Visible border: float, input | `edge` | `edge` |

In dark, `hairline` sits at about 1.1:1 on `base`: enough to mark where one tone
meets another, too faint to see between two areas of the same tone, so
splits take `edge`.

### Text

| Role | Dark | Light |
|---|---|---|
| Emphasis: titles, definitions, the cursor | `primary` | `primary` |
| UI text: labels, menu items, toolbar text (GUI toolkits derive their secondary levels from it by opacity) | `primary` | `primary` |
| Text on a strong fill: accent button, state fill | `white` | `white` |
| Body: code, terminal foreground | `soft` | `soft` |
| Labels, keywords, inactive tabs | `secondary` | `secondary` |
| Comments, punctuation, captions | `dim` | `dim` |
| Line numbers, placeholders, disabled | `faint` | `faint` |
| Guides, whitespace markers | `mute` | `mute` |

### Blue: selection, focus and literals

| Role | Dark | Light |
|---|---|---|
| Text selection (background; foreground unchanged) | `field` | `lavender` |
| Focus ring, link, string | `lavender` | `morpho` |
| Focused window or pane border | `lavender` | `morpho` |
| Constant, number, boolean, escape | `blue` (accent) | `blue` |
| Chip: search match under the cursor, TODO tag, active tab marker | `lavender` on, `morpho` text | `morpho` on, `white` text |
| Search match (others) | `night` on, `lavender` text | `wash` on, `morpho` text |
| Info, changed line (background) | `night` | `wash` |
| Strong focus fill: suggested button, switch on | `cobalt` | `morpho` |

Lists keep grey for where you are (`active`) and blue for what you have
selected (`field`), so a selected row and a text selection never look alike.

### State

| Role | Token (accent tier, `tint` behind) |
|---|---|
| Error, deleted, tmux's bell | `red` |
| Warning, modified-and-unsaved | `orange` |
| Caution, search wrap, pending | `yellow` |
| Added, passed | `green` |
| Hint, special (tags in diffs) | `cyan`, which is lavender (morpho in light) |
| Info, changed | `blue` |
| Urgent window: niri's border and tab | `critical` (shell) |
| Data scale: the weather card's charts (rain, a calm-to-gale wind scale, a cold-to-hot temperature ramp), never a state | light `blue`, `accent` green, yellow, orange, red, and `dim` cyan, green, yellow, orange, red |

An urgent window takes `critical`, the signal the island's urgent workspace
dot already gives; `Core/Theme.qml`'s ledger lists every use of it.

Magenta is in the palette only because terminals need sixteen colours; it is
a grey (`soft` in dark, `secondary` in light) and no theme here gives it a role.

### Terminal (ANSI 0–15)

Alacritty and Neovim's terminal use the same sixteen slots. The normal
colours are the accent tier; light terminals print slot 7 (white) as text,
so in light it is a mid grey.

| Slot | Dark | Light |
|---|---|---|
| 0 black | `active` | `soft` |
| 1 red, 2 green, 3 yellow, 4 blue, 5 magenta, 6 cyan | accent tier | accent tier (`blue` from the blue family) |
| 7 white | `soft` | `faint` |
| 8 bright black | `faint` | `dim` |
| 9 red, 10 green, 11 yellow, 13 magenta, 14 cyan | bright tier | accent tier again |
| 12 bright blue | `lavender` | `morpho` |
| 15 bright white | `white` | `sunk` |
| Dim colours (SGR 2), slots 1–6 | dim tier | dim tier |
| Dim foreground | `dim` | `dim` |

### Syntax

Code is mostly grey: hierarchy comes from brightness, the way type weight
carries it on the reference sites. Only literal values take the blue.

| Token kind | Dark | Light | Style |
|---|---|---|---|
| Plain identifiers, variables, fields | `soft` | `soft` | |
| Functions and types (definitions and uses), tags | `primary` | `primary` | |
| Keywords, operators-as-words, modules, attributes, macros | `secondary` | `secondary` | |
| `self`, `this`, builtins | `secondary` | `secondary` | italic |
| Parameters | `soft` | `soft` | italic |
| Punctuation, operators, delimiters | `dim` | `dim` | |
| Comments | `dim` | `dim` | italic |
| Doc comments | `secondary` | `secondary` | italic |
| Strings | `lavender` | `morpho` | |
| Numbers, constants, booleans, escapes, regex | `blue` | `blue` | |
| Headings (markup) | `primary` | `primary` | bold |
| Links (markup) | `lavender` | `morpho` | underline |

### Inverse panel

Haiqu's white card on black, in eggshell's terms: in dark, an eggshell panel
(`#f1efed`) with ink text; in light, a near-black panel with `primary` text.
Its tokens are `[dark.inverse]` and `[light.inverse]`:

| Role | Dark | Light |
|---|---|---|
| Panel | `panel` (`#f1efed`) | `panel` (`#141414`) |
| Text | `ink` | `ink` |
| Secondary text | `ink_secondary` | `ink_secondary` |
| Divider | `rule` | `rule` |
| Link, focus | `accent` | `accent` |

At most one inverse surface per view, for the single item of focus or a
content card; never a chrome bar. Terminal apps (Alacritty, tmux, Neovim,
fish) do not use it for now. Its radius grows with the panel's size.

Used today by the shell's notification card (the expanded toast and the
history's detail), niri's Alt-Tab highlight plate
behind the chosen window, libadwaita's toasts (GTK) and Firefox's new-tab
story cards in dark.
