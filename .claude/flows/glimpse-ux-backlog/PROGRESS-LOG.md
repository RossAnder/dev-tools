<!-- Generated from execution-record.toml. Do not edit by hand. -->

# glimpse UX backlog — navigation, tables, copy, diagram focus/zoom/fit, theme sets and revamp — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | shared-scroll-stepping-helper | 2026-10-21 | | 2 files |
| E3 | hint-mode-config-key | 2026-10-21 | | 1 file |
| E4 | new-element-tokens-and-revamp-defaults | 2026-10-21 | | 1 file |
| E5 | osc-52-encoder | 2026-10-21 | | 2 files |
| E6 | form-choice-lists-window-and-page | 2026-10-21 | | 2 files |
| E7 | markdown-strong-without-bold | 2026-10-21 | | 1 file |
| E8 | neighbourhood-subsets-and-the-fit-search | 2026-10-21 | | 2 files |
| E10 | item-and-inbox-cursors-page-and-jump | 2026-10-21 | | 1 file |
| E11 | lists-report-their-page-height | 2026-10-21 | | 3 files |
| E12 | column-picker-form | 2026-10-21 | | 1 file |
| E13 | named-theme-sets-with-style-rules | 2026-10-21 | | 2 files |
| E16 | route-paging-keys-to-the-focused-list | 2026-10-21 | | 3 files |
| E17 | sort-direction-and-per-column-sort-keys | 2026-10-21 | | 2 files |
| E20 | theme-config-base-set-and-per-set-overrides | 2026-10-21 | | 1 file |
| E21 | selector-windows-its-entries | 2026-10-21 | | 1 file |
| E22 | layout-of-a-node-subset-at-a-zoom-level | 2026-10-21 | | 2 files |
| E24 | theme-flag | 2026-10-21 | | 3 files |
| E25 | wire-hint-cycling-copy-capture-release-and-tolerant-drags | 2026-10-21 | | 3 files |
| E27 | runtime-carries-out-copy-capture-toggle-and-the-raw-mouse-event-log | 2026-10-21 | | 2 files |
| E28 | width-fitted-footer-with-short-full-and-hidden-modes | 2026-10-21 | | 2 files |
| E32 | released-capture-footer-line-only-after-a-v-release | 2026-10-21 | | 2 files |
| E33 | shared-table-core | 2026-10-21 | | 2 files |
| E40 | layers-view-uses-the-shared-row-drawer | 2026-10-21 | | 1 file |
| E41 | item-tables-draw-a-header-row-id-cells-and-empty-states | 2026-10-21 | | 2 files |
| E42 | inbox-table-draws-a-header-row | 2026-10-21 | | 1 file |
| E43 | wire-header-clicks-sort-keys-id-cell-copy-and-the-column-picker | 2026-10-21 | | 5 files |
| E48 | wire-the-theme-picker-and-live-switching | 2026-10-21 | | 3 files |
| E50 | theme-picker-overlay | 2026-10-21 | | 3 files |
| E51 | header-flooded-active-tab-faint-empty-tabs-history-on-tall-panes | 2026-10-21 | | 2 files |
| E52 | panel-helper | 2026-10-21 | | 3 files |
| E53 | traversal-card-uses-the-helper | 2026-10-21 | | 1 file |
| E54 | legend-panel-uses-the-helper | 2026-10-21 | | 1 file |
| E55 | details-selector-and-form-panels-use-the-helper | 2026-10-21 | | 3 files |
| E56 | modal-backdrop-comfortable-gutter-and-the-picker-panel | 2026-10-21 | | 2 files |
| E58 | wire-focus-zoom-fit-and-drag-pan | 2026-10-21 | | 3 files |
| E64 | paint-zoom-levels-the-hidden-marker-and-clickable-nodes | 2026-10-21 | | 2 files |
| E66 | persist-hints-columns-sort-direction-theme-and-diagram-view | 2026-10-21 | | 2 files |
| E73 | document-keys-config-themes-copy-and-the-event-log | 2026-10-21 | | 2 files |
| E84 | filter-by-section-and-document-the-released-footer-line | 2026-10-21 | | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E9 | fit floors at the target alone in glimpse/src/diagram/subset.rs | 2026-10-21 | | When the predicate rejects even the target alone, an empty diagram is worse than an overflowing one; fit floors at {target} at radius 0 (empty set for an unknown target), documented and tested. | — |
| E14 | Rules{fill, faint, colour} in glimpse/src/theme/sets.rs | 2026-10-21 | | mono must drop [theme] overrides as well as palette colours to stay byte-identical to NO_COLOR output, so Rules gained colour: bool. | — |
| E15 | kanso-zen palette is empty (falls through to TOKENS) in glimpse/src/theme/sets.rs | 2026-10-21 | | TOKENS defaults already are the kanso-zen palette; copying the 15 entries would duplicate them. | — |
| E18 | Sort::Category added; kind column maps to Severity on finding surfaces in glimpse/src/surface.rs | 2026-10-21 | | Sorting the category column by severity contradicts what it shows; added Sort::Category, and Surface::column_sort maps the kind column (which shows severity on finding surfaces) to Severity. | — |
| E19 | Inbox sort lives on InboxState (InboxColumn, SortDir); Surface::Inbox.sorts() stays [Sort::Id] in glimpse/src/surface.rs | 2026-10-21 | | runtime::arrange_rows ignores --sort on the Inbox, so listing Inbox keys in Surface::Inbox.sorts() would accept a flag and ignore it; the Inbox sorts through InboxColumn + SortDir on InboxState instead. | — |
| E23 | Fit cache key adds a titles hash at Zoom::Titles and keys view only under fit, in glimpse/src/diagram/mod.rs | 2026-10-21 | | At Zoom::Titles the titles set label widths and so whether a subset fits, so the fit key also hashes the drawn titles; view size is keyed only while fit is on, so the same stored choice covers fit-off focus closures. | — |
| E26 | Widened task files to include glimpse/src/hook.rs | 2026-10-21 | | The GLIMPSE_EVENT_LOG cap is hook::LOG_CAP, a private const in glimpse/src/hook.rs; the event log needs it made pub(crate) rather than duplicated. Row widened and re-dispatched. | — |
| E29 | Hints carry no priority field; order is priority, in glimpse/src/view/footer.rs | 2026-10-21 | | Priority is defined as the list order, so a separate field would repeat it; a Hint{lead, key, label} struct carries the released line's notice-styled lead instead. | — |
| E30 | ? keys pinned on base surfaces only, not in modals, in glimpse/src/view/footer.rs | 2026-10-21 | | Inside forms and the filter prompt ? is typed into the field, and the legend and selector already show their own ? hint; pinning ? keys there would be false. Base surfaces (Tasks, item surfaces, Inbox, compact details) always end ? keys; modals fit their hints with a trailing ellipsis. | — |
| E31 | Minted task 38 (app.rs + view/footer.rs) into checkpoint A | 2026-10-21 | | Approach "Copy and capture" says mouse = false frames keep their hints and only a v release shows "mouse released · v to capture"; task 9's detail keyed the line on !mouse_captured, which mouse = false sessions start with. A released-by-v flag in App is needed, outside task 9's files. | — |
| E34 | Column{title: &'static str, width: usize} in glimpse/src/view/table.rs | 2026-10-21 | | items::Columns::width already returns usize and titles are fixed strings, so Column uses title: &'static str and width: usize. | — |
| E35 | Column layout: LEAD then one gap per drawn column in glimpse/src/view/table.rs | 2026-10-21 | | Each drawn column starts one gap cell after LEAD and zero-width columns are skipped with their gap, matching today's item rows; inbox rows place their glyph at x+2 with no gap, so the inbox adoption must add one blank cell to line up with cell/header. | — |
| E39 | Widened task files to include glimpse/src/view/inbox.rs | 2026-10-21 | | inbox::render's main return lists every Drawn field without ..Drawn::default(), so adding headers and ids to Drawn breaks the crate until the Inbox task; a one-line ..Drawn::default() in inbox.rs is needed. Row widened and re-dispatched. | — |
| E49 | App keeps two override layers plus no_color; T is refused under NO_COLOR, in glimpse/src/app.rs | 2026-10-21 | | Theme::from_set takes both override layers and each preview rebuilds from them, so App keeps theme_overrides ([theme]), theme_per_set ([theme.<set>]) and no_color as separate private fields; T under NO_COLOR posts a notice instead of opening the picker. | — |
| E57 | Backdrop restyles header and body, not the footer, in glimpse/src/view/mod.rs | 2026-10-21 | | While an overlay is open the footer shows that overlay's own keys; dimming them would hide the hints needed right then, so only the header and body are restyled. | — |
| E59 | Added PanStart, PanTo and PanEnd actions in glimpse/src/keys.rs and glimpse/src/app.rs | 2026-10-21 | | keys::mouse takes &App and cannot record the pan origin, so starting, moving and ending a pan need PanStart(u16,u16), PanTo(u16,u16) and PanEnd; PanTo re-enters ScrollView so pinning goes through the existing path, and a keys::held helper shares the drag tolerance rules. | — |
| E60 | Legend scroll test terminal raised to 100x46 in glimpse/src/view/legend.rs | 2026-10-21 | | The Keys section (40 rows plus heading) no longer fits a 38-row inner area, so the_legend_scrolls_to_its_items_and_keys_sections was raised from 100x40 to 100x46; the drag-pan hint is folded into the x row. | — |
| E65 | Hidden chip avoids covering nodes, in glimpse/src/diagram/paint.rs | 2026-10-21 | | In narrow views the top-right chip covered the selected node (the_viewport_scrolls_to_keep_the_selection_visible failed); the chip falls back to bottom-right, then to the first corner leaving the selection uncovered, or is not drawn. | — |
| E67 | State::apply returns a Baseline consumed by capture, in glimpse/src/state.rs and glimpse/src/runtime.rs | 2026-10-21 | | App records no chosen-in-app flag, so State::apply returns a Baseline of the values the run opened with and capture(app, &baseline) writes theme/hints only when changed from it or already present in state.toml; a saved set is applied through the picker's revert path so Theme::from_set uses App's private override layers. | — |
| E68 | Existing saved theme is preserved under keep_theme, in glimpse/src/state.rs | 2026-10-21 | | Under keep_theme a theme already held in state.toml is written back unchanged, so one --theme or NO_COLOR run does not erase an earlier in-app choice; a fresh NO_COLOR run still writes no theme key. | — |
| E74 | README key table follows KEYS plus the keys KEYS folds or omits, in glimpse/README.md | 2026-10-21 | | KEYS merges several keys into one row and has no entry for ? or Ctrl+C, so the README table follows KEYS order and wording but still lists ?, Ctrl+C, m, e, r, n, u, /, w and Inbox Enter separately. | — |
| E83 | Minted task 39 (surface.rs + view/footer.rs) into checkpoint E | 2026-10-21 | | Two cheap-in-file discoveries (from the sort-direction and footer agents) in files this run touched and its tests cover: ItemsState::shows omits Anchor::Section while the new place() helper includes it, so the filter cannot find plan-review rows by section; footer.rs's module doc misstates when the released-capture line shows. Fixed in-run rather than captured. | — |
| E94 | Footer height now depends only on hint mode and width. A live notice replaces the last hint row in Full and Short mode; in Hidden mode the footer stays 0 rows and the notice draws over the body's last row, after the overlay backdrop. | 2026-10-21 | | Each notice resized the body twice, which re-ran the diagram fit search and could reset the layout and shift list pages. Applied by /review-apply on 2026-10-21. | — |
| E95 | On quit, state.toml is re-read and merged: theme and hints this run left unchanged keep their on-disk value, other layout keys stay last-quit-wins, and the temp file is per-process. | 2026-10-21 | | state.toml is user-global and several glimpse sessions run at once, so a session quitting later erased a theme or hints choice another session had saved. Applied by /review-apply on 2026-10-21. | — |
| E96 | Zoom::label in diagram/mod.rs owns the node text. The layout width is measured from it cell by cell, paint draws it advancing by each character's cell width, and control characters in titles become spaces. | 2026-10-21 | | Width and text agreed only by convention, and agent-written titles could carry terminal escape sequences that reached the terminal. Applied by /review-apply on 2026-10-21. | — |
| E97 | The theme base is carried as Option<ThemeChoice> (File or Flag of a &'static ThemeSet), with one NO_COLOR default in sets::starting; unknown-set errors name the form and list the valid sets. | 2026-10-21 | | It removes unreachable by_name fallbacks and a duplicated NO_COLOR default, and the (flag, no set) state can no longer be written. Applied by /review-apply on 2026-10-21. | — |
| E98 | Tab counts are a TabCount enum (Unread, Missing, Count). Only a missing ledger is faded; a ledger not yet read draws in the normal tab style. | 2026-10-21 | | Every item tab was drawn faded as absent until its first read. Applied by /review-apply on 2026-10-21. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-10-21 | 98 entries: status-transition × 2, task-completion × 39, deviation × 27, verification × 24, checkpoint × 6 | 0c14783, 33acfc0, 4398bdf, 480fedb, 4912a4b, 5aa75db, 68f9ba3, 697edfd, 6ea18ec, 73b8acb, 77df329, 7bb1c1e, 85f4daa, 8703fa8, a8e0871, a9043a8, bbdb7e8, cb48db3, d3060f2, dfab048, e12f579, f390b91 |
