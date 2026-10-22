# eggshell for Firefox

Two layers that work alone or together on both Firefox UIs, Proton and Nova.
Nova is the default from Firefox 157, and `browser.nova.enabled` switches between
the two. Colours come from [`../palette.toml`](../palette.toml) and follow the
roles in [`../README.md`](../README.md#roles).

| Path | What it does |
|---|---|
| `theme/manifest.json` | A static theme add-on. Its `theme` key is the light variant and its `dark_theme` key the dark one, and Firefox picks between them by the system colour scheme. It covers the frame, tabs, toolbar, address bar, popups, sidebar and new-tab colours, and leaves the page colour scheme to the system (`content_color_scheme: system`). |
| `chrome/userChrome.css` | The parts a theme cannot express: hairlines in place of shadows, a selected tab marked by a quiet blue tint alone (no outline or marker), the address bar's edge and focus ring, raised popups and context menus with an edge border, Nova's gradients removed, a monochrome accent. Each rule overrides one of Firefox's own custom properties. |
| `user.js` | Prefs a sheet cannot set: the `-moz-dialog` system colours a new window paints before its chrome's styles load. Firefox reads only the profile's own `user.js`, so these lines are appended to it by hand. |
| `chrome/userContent.css` | New tab and home (in dark its story cards are the inverse panel), private browsing, Settings and Add-ons, reader view's Light and Dark themes, and the PDF viewer's toolbar. On new tab, Settings, Add-ons and the PDF viewer it replaces Nova's violet text, hover and accent tokens, which also reach their menus and buttons. Ordinary web pages are left alone. |

Firefox ignores `chrome/` until `toolkit.legacyUserProfileCustomizations.stylesheets`
is true; see Installing.

## Proton and Nova

Rules shared by both UIs sit at the top level of `userChrome.css`. Rules for one
UI sit in `@media -moz-pref("browser.nova.enabled")` or in its `not` form.
`-moz-pref()` works here because user sheets may use chrome-only media queries.

- **Both:** the frame, the toolbars, the tabs, the address bar, the sidebar, the panels and menus, the content separator, the focus ring and the accent.
- **Proton only:** the find field's focus shadow is removed.
- **Nova only:** the following are replaced:
  - the toolbox gradient;
  - the selected tab's gradient border (drawn only for built-in themes);
  - the violet text, button and urlbar-box tints;
  - the info-bar border;
  - the 24 px arrow-panel radius, which becomes 12 px;
  - the urlbar view's shadow, which becomes an edge border.

With the theme add-on installed, Nova treats it as a third-party theme
(`[lwtheme]` without `[theme-in-app]`). Its violet design tokens then never
apply to the chrome, and Nova does not swap in its own private-window theme: a
private window uses the dark variant. Content pages (new tab, Settings, Add-ons,
the PDF viewer) still get those tokens, which `userContent.css` replaces.

## Installing

1. `scripts/themes.sh deploy firefox` links `chrome/` into the profile and
   prints the one pref it needs. Add that line to the profile's `user.js`
   (the script never writes that file), or set the pref to `true` in
   `about:config`:

   ```js
   user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);
   ```

   Append the lines in [`user.js`](user.js) too: they set the colour a new
   window shows before its chrome loads, which otherwise comes from the GTK 3
   theme (a mid grey under most dark themes). Restart Firefox afterwards.
2. The theme add-on is optional: `userChrome.css` sets the chrome without it.
   To build it, run `cd theme && zip -r ../eggshell-theme.xpi manifest.json`,
   then install it by a route that keeps signature checks on, on any channel:
   - **Signed:** sign it on addons.mozilla.org as an unlisted add-on
     (`web-ext sign --channel=unlisted`), then open the signed `.xpi`.
   - **Temporary:** load it each session from `about:debugging` → This Firefox
     → Load Temporary Add-on.
   - **Unsigned (Nightly, Developer Edition and unbranded builds only):**
     setting `xpinstall.signatures.required` to `false` in `about:config` lets
     the unsigned `.xpi` install, but it turns signature checks off for every
     add-on in the profile, for as long as the unsigned theme stays installed.
     Prefer either route above.

   Without the add-on, Firefox's default theme already follows the system
   scheme. The add-on adds new-tab colours that reach the page before it
   paints, and colours for a profile that does not use `chrome/`.

## Limitations

- **Blank tabs:** a blank tab paints Firefox's default canvas, not the base
  colour, because styling `about:blank` would expose the palette to any page
  through a same-origin blank frame.
- **New-tab cards in the add-on:** the theme can colour new-tab cards only as
  a whole, so the dark inverse story cards come from `userContent.css` alone.
- **Native menus:** context menus are themed only while Firefox draws them
  itself. Native menus on macOS and Windows mica popups ignore the sheet.
- **Linux compositors without transparency:** GTK has no alpha there, so
  Firefox forces square popup corners.
- **AI (smart) windows:** these take their colours from Firefox's own AI theme.
  `userChrome.css` still overrides them, but the theme add-on does not.
- **Pages that use their own colours:** a page that sets its own colours
  (Firefox View, the AI pages, error pages) keeps them.
- **New-tab crossword:** the widget's puzzle is a web page in a frame
  (Mozilla's games server) with its own violet colours, and the sheet leaves
  web pages alone; the widget's frame and menu are themed.
- **PDF viewer:** its document has the PDF's own URL, so the sheet finds it
  by the stylesheet link only the built-in viewer has. A web page that copies
  that link is styled as the viewer, and can read the palette.
- **`tab_background_separator`:** the theme omits this key, which Firefox has
  ignored since version 89.
- **Signing:** AMO may ask for `browser_specific_settings.gecko.data_collection_permissions`
  (`{"required": ["none"]}`). The key is left out because older Firefox
  versions do not recognise it.
