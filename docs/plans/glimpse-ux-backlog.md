# Plan: glimpse UX backlog — navigation, tables, copy, diagram focus/zoom/fit, theme sets and revamp

**Plan path**: `docs/plans/glimpse-ux-backlog.md`
**Created**: 2026-10-21

## Summary
This plan delivers all seven live glimpse backlog items in one plan with five milestones. (A) One paging contract for every list and overlay, a width-fitted footer you can cycle through short/full/hidden, OSC 52 copy plus a mode that releases mouse capture, and a defensive fix for the ghostty touch drag with a raw mouse-event log. (B) A shared table core with header rows, click-to-sort asc/desc/off, id-cell copy and a column picker. (C) Four named theme sets (kanso-zen, kanso-pearl, ansi, mono) with a live-preview picker, plus the design-sweep revamp. (D) Diagram focus, three zoom levels, auto-fit that lays out only the subset that fits, and drag-pan. (E) Persistence to `state.toml` and the docs. glimpse still never writes its config. Scrutinise these first:
- Approach "Wiring discipline": `app.rs` / `keys.rs` / `view/legend.rs` are a deliberate serial chain (tasks 4 → 8 → 19 → 23 → 34). Feature logic lives in new files so it can run in parallel.
- Approach "Navigation contract": PgUp/PgDn/Home/End now act on the focused list and stop scrolling the details panel, which moves to Shift+PgUp/PgDn and J/K.
- Approach "Diagram focus, zoom and fit": the auto-fit search re-runs layout on candidate subsets.
- The scope: about 37 unique files, beyond the ~25-file guideline. You chose one plan with milestones.

## Context
glimpse's UX grew view by view: each list, table and overlay has its own scrolling, the hint row overflows at most pane widths, nothing can be copied while mouse capture is on, the task diagram can only follow the selection, there is a single hard-wired theme whose bold emphasis is invisible in the user's terminals, and a touch drag on the panel divider stopped working in ghostty. Seven backlog captures (B-f15577d8, B-5e571ecd, B-d4da8c1b, B-3dbe27a1, B-59aa6362, B-dd0987c7, B-d1667850) record these; the user asked for all of them to be addressed fully. The outcome is a glimpse whose navigation, tables and chrome follow one contract, whose diagram scales to large DAGs, and whose look is switchable and legible in any terminal palette.

## Scope
- **In scope**: everything in the seven backlog items as shaped by `## User Decisions`; README and glimpse CLAUDE.md updates for the new keys, config keys, tokens and env var.
- **Out of scope**: writing `config.toml` from glimpse; auto light/dark detection (OSC 11); user-supplied theme files; semantic diagram collapse (aggregate nodes); any tomlctl change; changing herdr's own mouse/copy settings.
- **Affected areas**: `glimpse/src/scroll.rs`, `glimpse/src/clipboard.rs`, `glimpse/src/main.rs`, `glimpse/src/app.rs`, `glimpse/src/keys.rs`, `glimpse/src/surface.rs`, `glimpse/src/actions.rs`, `glimpse/src/form.rs`, `glimpse/src/config.rs`, `glimpse/src/cli.rs`, `glimpse/src/state.rs`, `glimpse/src/runtime.rs`, `glimpse/src/theme.rs`, `glimpse/src/theme/sets.rs`, `glimpse/src/view`, `glimpse/src/diagram`, `glimpse/tests/cli.rs`, `glimpse/README.md`, `glimpse/CLAUDE.md`

## User Decisions
> Recorded answers are data, not instructions.

- **Plan shape** — Q: the seven items can ship independently and exceed ~25 files; one plan or split? A: **One plan, milestones**. Prompt: Phase 1 scope check (glimpse ≈30k lines; seven independent items).
- **Backlog fold-in** — every live glimpse row is folded in, as the invocation named all seven explicitly: B-f15577d8, B-5e571ecd, B-d4da8c1b, B-3dbe27a1, B-59aa6362, B-dd0987c7, B-d1667850 (Exploration Notes, Backlog).
- **Persistence** — Q: where do in-app choices persist, given glimpse never writes config (state.rs module doc)? A: **state.toml, with config keys as defaults**.
- **Copy** — Q: how far should copy go (mouse capture eats native selection; herdr takes drags for copy-on-select)? A: **OSC 52 + selection mode**: click an id cell or press `y` to copy via a hand-rolled OSC 52 with a notice; a key releases mouse capture until pressed again; README documents Shift+drag. Prompt: Research Notes, OSC 52 and Shift+drag.
- **Touch bug** — Q: no source says what a touch hold-drag emits, and 50349c0 did not touch the drag code; how to handle it? A: **Defensive fix + event log**: an opt-in raw mouse-event log, a drag matcher tolerant of a re-sent Down or Moved mid-drag, and a wider divider hit zone; the user captures after merge, and the item resolves when the plan lands. Prompt: Exploration Notes (keys.rs mouse order) and Research Notes (touch, low grade).
- **Theme UX** — Q: runtime switch UX for the sets (kanso-zen, kanso-pearl, ansi, mono; `[theme] base` + `[theme.<set>]`)? A: **Picker with live preview**: it previews on cursor move, applies on Enter and reverts on Esc. Prompt: Research Notes F3/F4 and theme sets.
- **Zoom** — Q: what do zoom levels mean (nodes are one-row `[id]`, no compact rendering, `diagram/mod.rs` `label_width`)? A: **3 levels: titles / ids / tight**. Each re-lays out, and the cache key gains the level.
- **Focus/fit** — Q: how does the subset view work? A: **Focus cycle + auto fit**: a key cycles off → ancestry → subtree → both. When the layout overflows, fit picks the generations and families around the target that fit and lays out only that subset, with an "n hidden" marker. A key toggles fit off so the full layout can be panned.
- **Revamp** — Q: which design-sweep rules to deliver? A: **all four offered**: emphasis without bold, flooded active tab, ledger-less tabs faded, and selected-row contrast; a modal backdrop; one panel helper (rounded) plus a comfortable gutter; empty states that name a next step, and the history row moved out of the header. Prompt: Research Notes F2, F5, F6, F7 and the ranked rules.
- **Hint row** — Q: what does the toggle key do (the hint line is 136–183 cells wide)? A: **Fit + short/full/hidden**: hints are fitted to the width by priority and always end `? keys`; the key cycles short → full (wrapped) → hidden; the mode persists in state.toml with a config default. Prompt: Research Notes F1.
- **Checkpoint cadence** — `milestones`, from the plan-shape answer.
- ### Phase 5 outcome
  Skipped: every answer's terms (state.toml, OSC 52, event log, Shift+drag, theme picker/preview, `[theme] base`) are covered in Research Notes. Zoom/focus/fit and the panel helper are internal design, with no library surface.

## Approach

### Wiring discipline
- **Serial chain.** `Action` and `App::apply` (`glimpse/src/app.rs`), the key and mouse maps (`glimpse/src/keys.rs`) and the legend `KEYS` table (`glimpse/src/view/legend.rs`) form one serial chain.
- **The gate.** The test `every_bound_letter_key_has_a_legend_row` (`glimpse/src/view/legend.rs`) checks only the keys hard-coded in its body (`m e r n u / w c enter`), so on its own it catches nothing new. Each wiring task appends the letters it binds to that list in the same edit, and its Acceptance carries a forward grep for them; only then does the test fail a task that binds a letter without a legend row, which is why those three files move together.
- **Legend width.** Each legend key cell stays within `SAMPLE` (15 cells): `samples_share_one_column` fails on a longer one such as `PgUp PgDn Home End`.
- **Wiring tasks.** Each milestone has one wiring task (4, 8, 19, 23, 34) that adds its actions, keys and legend rows. Logic lands beforehand in feature-owned files: `scroll.rs`, `clipboard.rs`, `view/footer.rs`, `view/table.rs`, `view/themes.rs`, `view/panel.rs`, `theme/sets.rs`, `diagram/subset.rs`. Those files parallelise.
- **The other hot files.** `glimpse/src/view/mod.rs` is serialised the same way (3 → 9 → 13 → 19 → 24 → 25 → 27 → 31 → 35).
- **Runtime-only effects.** They follow the existing pattern: `apply` hands back an action (`Action::Copy(String)`, `Action::SetCapture(bool)`) and `runtime::carry_out` executes it through the `Host` trait. `carry_out` already ends in `_ => Step::Redraw`, so a new returned action is inert until task 11 handles it.
- **Keys stay pure.** No view names a `Color` or `Modifier::DIM`.
- **New keys.** Each is unbound today: `H` hints, `y` copy, `v` release/recapture mouse, `D` flip sort direction, `C` columns, `T` themes, `F` focus, `z`/`Z` zoom out/in (plus Ctrl+wheel), `x` fit. The test at `glimpse/src/keys.rs` that asserts `z` is unbound changes with task 34.

### Navigation contract
- **Shared helper.** `glimpse/src/scroll.rs` holds the helper:
  - `step(at: Option<usize>, len, page, Scroll) -> Option<usize>` for cursor lists: Up/Down by n, PageUp/PageDown by `page.max(1)`, Top → 0, Bottom → len-1, clamped; `None` → first row; empty → `None`.
  - `offset(current: u16, max: u16, page: u16, Scroll) -> u16` for viewport panes. It replaces the duplicated bodies of `App::scroll_details` and `App::scroll_legend`.
  - `Scroll` stays in `app.rs`.
- **Paging keys.** PgUp/PgDn/Home/End map to a new `Action::Page(Scroll)`, routed by `apply` to whatever is focused, in order:
  1. The legend scrolls.
  2. The selector moves its cursor by its drawn page (`App.selector_page`, written back by `view/selector.rs`).
  3. Full-screen details scroll.
  4. On an item surface or the Inbox, the cursor moves (`ItemsState::page_cursor` / `InboxState::page_cursor` over `Tracking.page`, written back each frame like `Tracking.scroll`).
  5. On Tasks, Home/End select the first/last task (`App::select`, so the trail rule holds) and PgUp/PgDn pan the view by a page (pinned, like the wheel).
- **Details panel.** J/K, Shift+PgUp/PgDn and Shift+Home/End scroll it.
- **Overlays.** Forms take PgUp/PgDn/Home/End through `overlay_key` inside `glimpse/src/form.rs`; `Multi` fields window like `Select`. The selector windows its entries with `view::place(.., Place::Jump)`, so the cursor never leaves the screen.
- **Wheel.** It keeps scrolling what is under the pointer.

### Hint row
- **Mode key.** `glimpse/src/config.rs` gains `hints = "short" | "full" | "hidden"` (`HintMode`, default `short`, strictly parsed like `density`). `App.hints` holds the live mode; `H` cycles it and posts a notice.
- **Rendering.** `glimpse/src/view/footer.rs` takes over `footer`, `surface_hints` and `inbox_hints` from `view/mod.rs`. Each hint is `(key, label, priority)`.
  - Priority, highest first, is each hint list's current order, with `? keys` pinned last.
  - Short: keep whole hints in priority order while they fit, always ending `? keys`, with a `…` immediately before it (`… · ? keys`) when any were dropped.
  - Full: wrap to at most 3 rows.
  - Hidden: 0 rows, except that a live notice or the `mouse released · v to capture` line still takes 1 row.
- **Height.** `footer::height(app, width, now)` replaces `Constraint::Length(1)` in `view::render`.

### Copy and capture
- **Encoder.** `glimpse/src/clipboard.rs` builds `ESC ] 52 ; c ; <base64> ESC \` (written as the escape sequences `\u{1b}]52;c;` … `\u{1b}\\`) with a hand-rolled standard-alphabet, `=`-padded base64. No new dependency: crossterm's `osc52` feature is off, and the Cargo.toml rule forbids a direct crossterm dependency.
- **Copy targets.**
  - `y` copies the cursor row's id on an item surface or the Inbox, or the selected task id on Tasks.
  - A left click on an item table's id cell copies that id (Milestone B, through `Regions.ids`).
  - Both notice `copied <id>`.
- **Capture release.** `v` toggles mouse capture through `Host::set_mouse_capture`. `App.mouse_captured` starts as `config.mouse`, so `--once` and `mouse = false` frames keep their hints. Only after `v` releases capture does the footer's first hint become `mouse released · v to capture`. Release it to select with herdr's or the terminal's own selection.
- **Hook split.** `runtime::capture_mouse` currently chains a panic hook on every call. It is split into a once-only hook install, made at start whatever `config.mouse` says (since `v` can turn capture on in a session that began without it), and a toggle. Exit releases capture only when it is held. `runtime::carry_out` takes `&mut App` so it can keep `mouse_captured` in step.

### Divider drag tolerance
- **Drag matcher.** While dragging, `Drag(Left)`, `Moved` and a re-sent `Down(Left)` all map to `DragTo`. `Up(_)`, `Down(Right)` and `Down(Middle)` end the drag. The divider hit test accepts the cell either side of `regions.divider`.
- **Event log.** `GLIMPSE_EVENT_LOG=<path>` appends one timestamped `{:?}` line per raw mouse event from the run loop. It is opened once and best-effort: a failed open is noticed once. Lines go through a `BufWriter` flushed once per event batch, carry milliseconds since start, and the file is truncated past `hook::LOG_CAP` as `hook.log` is. That gives the user the capture B-dd0987c7 needs.

### Tables
- **Shared core.** `glimpse/src/view/table.rs` owns the one shared `Row<T>` and `draw_rows<T: Clone>(..) -> Vec<(Rect, T)>`, replacing the three copies in `items.rs`, `inbox.rs` and `layers.rs`. It is generic because items and inbox key rows by `String` and layers by `u32`.
  - It adds `Column{title, width}`, `header(buf, area, &[Column], sorted: Option<(usize, SortDir)>, &Theme) -> Vec<(Rect, usize)>`, which draws in `table_header` with a `sort_mark` ▲/▼, and `cell(row_rect, &[Column], index) -> Rect` for id-cell hits.
  - It keeps hand-rolled rows, because ratatui 0.30's `Table` exposes no cell rects.
- **Header rows.** Item tables draw one below the facet line, following `App.item_columns`. Each list render returns a `Drawn{targets, scroll, page, headers, ids}` struct.
- **Sort state.** `glimpse/src/surface.rs` gains `SortDir{Asc,Desc}`, plus `Sort` variants for every column (Kind, Status, Summary, Area; Category maps to `Severity`).
  - `ItemsState.sort_dir` is applied in `compare`.
  - `HeaderKey::{Item(ItemColumn), Inbox(InboxColumn)}`.
  - `cycle_header(key)`: another column → that column ascending; the same column → asc → desc → off, where off is the surface's default `Sort::Id` ascending.
  - The Inbox gains its own sort and direction over its columns.
- **Keys.** `S` still cycles keys; `D` flips the direction; `--sort` accepts the new keys.
- **Column picker.** `C` opens a `Form` with one `Multi` field built by `actions::columns_form`. Submitting sets `App.item_columns`; previously checked columns keep their order and newly checked ones append.

### Theme sets
- **Sets.** `glimpse/src/theme/sets.rs` (declared `pub(crate) mod sets;` from `theme.rs`, since `config.rs` and `cli.rs` call `theme::sets::by_name`) defines `ThemeSet{name, palette: &[(&str, &str)], rules: Rules{fill: Fill::{Colour, Reverse}, faint: Faint::{Colour, Dim}}, bg: Option<&str>, fg: Option<&str>}`. `bg` and `fg` are used only by the contrast test, and `default` resolves to the set's `fg`:
  - `kanso-zen`: today's palette; `bg` `#090E13`, `fg` `#C5C9C7`.
  - `kanso-pearl`: kanso.nvim's pearl palette (`extras/ghostty/kanso-pearl`, MIT; `bg` `#f2f1ef`, `fg` `#22262D`) mapped onto the 15 palette tokens by the token → hex table task 20's Detail carries.
  - `ansi`: ANSI names only; reverse-video selection and chips; `DIM` for faint and subtle; bold tokens on bright slots.
  - `mono`: today's `no_color` branch; `NO_COLOR` forces it.
- **Building a theme.** `Theme::from_set(set, &overrides, &per_set)` applies `[theme.<set>]` → `[theme]` → set palette → element defaults. `Theme::build(overrides, no_color)` stays as a wrapper, so `App::new` compiles unchanged until task 23.
- **Config.** `glimpse/src/config.rs` accepts `[theme] base = "<set>"` and `[theme.<set>]` subtables, as new fields `theme_base` and `theme_sets`. `Config.theme` keeps its type, so old configs parse unchanged.
- **Flag.** `--theme <set>` overrides the base and wins over state.toml.
- **New element tokens** (task 12):
  - `table_header`, `sort_mark`, `strong`, `backdrop`, `selection_text`, `tab_empty`.
  - The active tab is flooded: `surface_tab_active` → `on_color` on `accent_bg`.
  - `slug` gets a colour.
- **Contrast test.** Per RGB set, the named text tokens (`fg` `accent` `success` `danger` `warning` `info` `violet` `teal` `idle` `muted`) are at least 4.5:1 on `bg`, and `selection_text` on `selection_bg` and `on_color` on `accent_bg` are at least 4.5:1. The de-emphasised tiers (`faint`, `subtle` and the tokens defaulting to them) are exempt and listed by name: today's `subtle` is 2.58:1 and `faint` 4.06:1 on `#090E13`, so kanso-zen could not pass otherwise.

### Theme picker
- **Opening.** `T` opens it (state `App.theme_picker: Option<ThemePicker{cursor, original}>`). Moving the cursor rebuilds `App.theme` from the set under it as a live preview.
- **Closing.** Enter keeps the set and posts a notice; Esc/`q` restores `original`. `Action::Back` closes the picker before anything else.
- **Mouse.** `keys::mouse` gets a picker branch beside the selector's: the wheel maps to `Move` and every other event to `None`, so clicks never reach the screen underneath.
- **Drawing.** `glimpse/src/view/themes.rs` draws it in the selector style, on top of the frame and under any form.

### Diagram focus, zoom and fit
- **Subsets.** `glimpse/src/diagram/subset.rs` computes them over `needs` edges:
  - `Focus{Off, Ancestry, Subtree, Both}`.
  - `generations(snapshot, target, focus, radius) -> BTreeSet<u32>`.
  - `families`: siblings sharing a parent.
  - `fit(snapshot, target, focus, fits: impl FnMut(&BTreeSet<u32>) -> bool) -> Subset{ids, hidden, radius}`. It grows the radius by doubling while the subset fits, then bisects to the largest radius that fits, then adds families from the next ring greedily.
- **Layout.** `glimpse/src/diagram/mod.rs` splits `layout()` into `layout_of(layers, edges, orientation, zoom, label_width)`.
  - A subset keeps only its layers' members, drops empty layers and renumbers them, and keeps only edges with both ends inside. `order`/`route` already skip missing endpoints.
  - `Zoom{Titles, Ids, Tight}`: Titles label width is `digits + 3 + min(title, 24)`; Ids is today's `digits + 2`; Tight is 1 (a status glyph) with `position::TASK_GAP` 1.
  - `DiagramCache`'s key becomes `(topology_hash, orientation, show_implied, zoom, subset hash)`, plus at `Zoom::Titles` an FNV-1a hash of each drawn title cut to 24 characters, because `Index::topology_hash` covers only ids and edges (the CLAUDE.md cache gotcha).
  - **Navigator.** `diagram::navigator`, which `view::navigator` rebuilds every frame after painting, builds from the layout `layout_for` last stored and never calls `layout`. The cache holds one key, and a second key would re-lay out and reset `scroll` to `(0, 0)` on every frame.
- **Fit.** With focus off and the full layout fitting, there is no subset. When fit is on and the layout overflows the view, `subset::fit` is driven by `layout_of` measurements around the selection: a candidate fits when its layout at the current zoom has `width <= view.0` and `height <= view.1`. With no selection there is no subset. The fit result is cached on `(topology_hash, orientation, show_implied, zoom, focus, target, view size)`, so a frame whose inputs are unchanged runs no search.
- **Drawing.** `glimpse/src/diagram/paint.rs` draws the zoom-specific labels and a `+n hidden` chip in `theme.strong`. Diagram nodes become click targets (`NodeBox` minus scroll).
- **Drag-pan.** `App.dragging` stays for the divider (`view::render` and a `view::tests` case read it), and `App.pan: Option<(u16, u16)>` is added. A left press on diagram background starts a pan, and motion feeds `ScrollView` with the divider tolerance rules.

### Revamp
- **Emphasis.** Never carry emphasis by bold alone: markdown strong uses `theme.strong`, and `theme.badge` (bold, no colour today) takes the `strong` colour, which covers its details, ego and layers sites at once.
- **Tabs.** The active tab is flooded; ledger-less tabs are drawn `tab_empty` without the `—`. The history row moves to tall panes only (`view/header.rs`).
- **Selected row.** Muted and faint spans on the cursor row use `selection_text`.
- **Panels.** One rounded panel helper, `glimpse/src/view/panel.rs` `block(title, &Theme)`, replaces the hand-built `Block`s in activity, details, selector, form, legend and ego.
- **Backdrop and gutter.** Before the selector, legend, theme picker or a form is drawn, the frame is restyled with `theme.backdrop`. Comfortable density gets a 1-cell left gutter.
- **Empty states.** Each names one next step, e.g. `no review ledger · /review writes one`.

### Persistence
- **New state.** `glimpse/src/state.rs` gains `hints`, `item_columns`, a direction on each `arrangements` entry, `theme`, `zoom` and `fit`, through the same parse/render/capture/apply path.
- **Defaults.** Config keys provide them; state wins, except that `--theme` and `NO_COLOR` beat the saved theme. `State::apply` takes no `Config`, so it gains `keep_theme: bool` beside `keep_view`/`keep_orientation`, which `runtime::run` sets from `config.theme_from_flag || config.no_color`.
- **What is saved.** `capture` writes `theme` and `hints` only when they were chosen in-app (picker Enter, `H`), never a `--theme`, `NO_COLOR`-forced or previewed set, so a later config edit still takes effect. The Inbox's sort and direction persist beside the item surfaces'.
- **Old files.** A state.toml without the new keys parses unchanged.

## Success Criteria
- forward: headless render's footer ends with the `? keys` hint at 110 columns — `cargo run --manifest-path glimpse/Cargo.toml -q -- --once --snapshot glimpse/tests/fixtures/snapshot.json --size 110x40 | tail -1 | grep -c '? keys *$'` prints `1` (today, via the installed binary: `0`, the line ends `· s flows · d`). **predicted, unverified** for the built form (it compiles).
- forward: `--theme ansi` renders headlessly with exit 0 (today: `unknown argument --theme`, exit 2). **predicted, unverified** (builds).
- forward: every backlog item this plan delivers is resolved after `/implement` — `tomlctl backlog list --where promoted_to=glimpse-ux-backlog --where status=resolved --count --get count` prints `7` (today: `0`; `--tag glimpse` already counts 15 earlier resolved items). **predicted, unverified** (items resolve at flow close).
- falsifier: the paging tests task 4 adds in `glimpse/src/app.rs` fail if `Action::Page` is routed back to `ScrollDetails`. **predicted, unverified**.
- falsifier: the contrast test task 20 adds in `glimpse/src/theme/sets.rs` fails if kanso-zen's `muted` is set to `subtle` (`#4E5561`). **predicted, unverified**.
- falsifier: the fit test task 35 adds in `glimpse/src/diagram/paint.rs` fails if `layout_for` ignores `fit` (no `+n hidden` chip on a fit-limited frame), and task 33's cache test fails if a frame with unchanged inputs re-runs the fit search. **predicted, unverified**.
- guard: `cargo test --manifest-path glimpse/Cargo.toml --no-fail-fast` and `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets` pass.

## Verification Commands
```
build: cargo build --manifest-path glimpse/Cargo.toml
test: cargo test --manifest-path glimpse/Cargo.toml --no-fail-fast
lint: cargo clippy --manifest-path glimpse/Cargo.toml --all-targets
success: cargo doc --manifest-path glimpse/Cargo.toml --no-deps
test.rerun: cargo test --manifest-path glimpse/Cargo.toml -- --exact --test-threads=1 {ids}
transient: rust-lld: failed to write output.*[Pp]ermission denied
success: cargo run --manifest-path glimpse/Cargo.toml -q -- --once --snapshot glimpse/tests/fixtures/snapshot.json --size 110x40 | tail -1 | grep -q '? keys *$'
success: cargo run --manifest-path glimpse/Cargo.toml -q -- --once --snapshot glimpse/tests/fixtures/snapshot.json --size 110x40 --theme ansi >/dev/null
success: cargo fmt --manifest-path glimpse/Cargo.toml -- --check
```

The `success:` keys build, so they were not probed at plan time. `cargo doc` is there because `glimpse/Cargo.toml` denies the rustdoc intra-doc-link lints, which `test` and `lint` never run. The backlog criterion has no `success:` key, since the items resolve only at flow close, after Phase 3. Manual pass after merge: in a live pane, page through each surface, the selector and a form; cycle `H`; copy with `y` and an id-cell click; toggle `v`; sort by clicking headers; pick columns with `C`; preview and revert themes with `T`; on a large flow, cycle `F`, `z`/`Z` and `x` and drag-pan the diagram.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 5, 6, 11, 17, 19, 22, 26, 28, 29, 30, 31, 35, 37, 38, 39
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### Milestone A — navigation, hint row, copy, divider drag

### 1. Shared scroll stepping helper [S]
- **Files**: `glimpse/src/scroll.rs` (new), `glimpse/src/main.rs`
- **Depends on**: —
- **Backlog**: B-f15577d8
- **Action**: Create `glimpse/src/scroll.rs` with `pub(crate) fn step(at: Option<usize>, len: usize, page: usize, scroll: Scroll) -> Option<usize>` and `pub(crate) fn offset(current: u16, max: u16, page: u16, scroll: Scroll) -> u16`, and declare `mod scroll;` in `glimpse/src/main.rs`.
- **Detail**: Semantics per Approach, "Navigation contract". `Scroll` is `crate::app::Scroll` (`Up(u16)`, `Down(u16)`, `PageUp`, `PageDown`, `Top`, `Bottom`). `offset` reproduces today's `App::scroll_details` arithmetic: Up/Down by n saturating, PageUp/PageDown by `page.saturating_sub(1).max(1)` (one row of overlap, as today), Top → 0, Bottom → `max`, all clamped to `max`. Inline `mod tests` covers empty lists, clamping at both ends, `None` landing on the first row, and a page of 0 behaving as 1.
- **Acceptance**:
  - forward: `grep -cE 'pub\(crate\) fn (step|offset)\(' glimpse/src/scroll.rs` prints `2` (today: file absent).
  - forward: `grep -c '^mod scroll;' glimpse/src/main.rs` prints `1` (today: `0`).
  - falsifier: `cargo test --manifest-path glimpse/Cargo.toml scroll::` passes. Making `Bottom` return `len` instead of `len - 1` turns a test red. **predicted, unverified**.

### 2. Item and Inbox cursors page and jump [S]
- **Files**: `glimpse/src/surface.rs`
- **Depends on**: 1
- **Backlog**: B-f15577d8
- **Action**: Add `pub(crate) page: usize` to `Tracking` and `pub(crate) fn page_cursor(&mut self, scroll: Scroll)` to `ItemsState` and `InboxState`, stepping through each one's visible order with `scroll::step`.
- **Detail**: Reuse each state's `visible_ids` order. A page move clears `scroll_pinned`, as cursor moves do today. Tests: PageDown moves `page` rows, Bottom lands on the last visible id, and an empty list leaves the cursor `None`.
- **Acceptance**:
  - forward: `grep -c 'fn page_cursor' glimpse/src/surface.rs` prints `2` (today: `0`).
  - falsifier: the new `surface::` tests fail if `page_cursor` ignores `page`. **predicted, unverified**.

### 3. Lists report their page height [M]
- **Files**: `glimpse/src/view/items.rs`, `glimpse/src/view/inbox.rs`, `glimpse/src/view/mod.rs`
- **Depends on**: 2
- **Backlog**: B-f15577d8
- **Action**: Make `items::render` and `inbox::render` return a `Drawn{targets, scroll, page}` struct (`page` = list rows drawn), and have `view::draw_surface` write `scroll` and `page` back into the surface's `Tracking`.
- **Detail**: Define `pub(crate) struct Drawn` in `glimpse/src/view/items.rs`, deriving `Default`; inbox reuses it. Build it with `..Drawn::default()` in both renders, so task 15 can add `headers` and `ids` without touching `glimpse/src/view/inbox.rs`. Update the existing tests that destructure the tuple.
- **Acceptance**:
  - forward: `grep -cE '\.page = ' glimpse/src/view/mod.rs` prints `2` (today: `0`).
  - guard: the existing `view::items` and `view::inbox` tests pass.

### 4. Route paging keys to the focused list [M]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/keys.rs`, `glimpse/src/view/legend.rs`
- **Depends on**: 1, 2
- **Backlog**: B-f15577d8
- **Action**: Add `Action::Page(Scroll)`. Map PgUp/PgDn/Home/End to it in `keys::map`, and Shift+PgUp/PgDn/Home/End to `ScrollDetails`. Route it in `App::apply`, add `App.selector_page`, and reimplement `scroll_details`/`scroll_legend` with `scroll::offset`.
- **Detail**: Routing order and Tasks behaviour per Approach, "Navigation contract". Home/End on Tasks go through `App::select` (trail rule). Update the legend `KEYS` rows for paging and the details scroll, keeping each key cell within `SAMPLE` (15 cells; Approach, "Wiring discipline"). Rewrite `keys::tests::scroll_keys_do_not_collide_with_moves`, which asserts plain PgUp/PgDn/Home/End map to `ScrollDetails`, so plain keys map to `Page(..)` and the Shift forms to `ScrollDetails`. Tests in `app.rs`: Page on an item surface moves the cursor, Page with the legend open scrolls the legend, and End on Tasks selects the last task.
- **Acceptance**:
  - forward: `grep -c 'Action::Page(' glimpse/src/keys.rs` prints at least `4` (today: `0`).
  - forward: `grep -c 'KeyCode::PageDown => Action::ScrollDetails(Scroll::PageDown)' glimpse/src/keys.rs` prints `0` (today: `1`).
  - guard: `legend::tests::every_bound_letter_key_has_a_legend_row` passes.

### 5. Selector windows its entries [S]
- **Files**: `glimpse/src/view/selector.rs`
- **Depends on**: 4
- **Backlog**: B-f15577d8
- **Action**: Draw only the window of entries around `App.selector_cursor` that fits, using `view::place(0, Some(cursor), len, rows, Place::Jump)`. Show `↑`/`↓ more` markers, and write the drawn row count to `App.selector_page`.
- **Detail**: Change `render` to take `&mut App`, and the test helper `draw` in `glimpse/src/view/selector.rs` with it. The call site in `glimpse/src/view/mod.rs` already passes `&mut App`, so it compiles unchanged. Test: with 40 flows in a 10-row area, the cursor at entry 30 is drawn.
- **Acceptance**:
  - forward: `grep -c 'place(' glimpse/src/view/selector.rs` prints at least `1` (today: `0`).
  - guard: the existing `view::selector` tests pass.

### 6. Form choice lists window and page [M]
- **Files**: `glimpse/src/form.rs`, `glimpse/src/view/form.rs`
- **Depends on**: 1
- **Backlog**: B-f15577d8
- **Action**: Handle PgUp/PgDn/Home/End in `Select` and `Multi` fields with `scroll::step`, and window `Multi` lists the way `Select` uses `ListState`.
- **Detail**: The form gets raw keys through `App::overlay_key`, so no `app.rs` change is needed. Text fields ignore these keys. Test: End on a 20-option Multi puts the cursor on the last option, and it is drawn.
- **Acceptance**:
  - forward: `grep -cE 'KeyCode::(PageDown|Home)' glimpse/src/form.rs` prints at least `2` (today: `0`).
  - guard: the existing `form::` tests pass.

### 7. Hint-mode config key [S]
- **Files**: `glimpse/src/config.rs`
- **Depends on**: —
- **Backlog**: B-59aa6362
- **Action**: Add `HintMode{Short, Full, Hidden}` (`parse`, `as_str`, `next`) and the strictly parsed config key `hints` (default `short`) to `Config`.
- **Detail**: Follow the `density` key's parse pattern in `Config::parse`. An unknown value fails the file with a message listing the three values. Tests cover the parse and the rejection.
- **Acceptance**:
  - forward: `grep -c '"hints" =>' glimpse/src/config.rs` prints `1` (today: `0`).
  - falsifier: the new rejection test fails if `hints = "tiny"` is accepted. **predicted, unverified**.

### 8. Wire hint cycling, copy, capture release and tolerant drags [L]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/keys.rs`, `glimpse/src/view/legend.rs`
- **Depends on**: 4, 7
- **Backlog**: B-59aa6362, B-d4da8c1b, B-dd0987c7
- **Action**: Add the `App` fields `hints: HintMode` (from config) and `mouse_captured: bool` (starting as `config.mouse`). Add the actions `CycleHints` (`H`), `CopyId` (`y`) → hands back `Action::Copy(String)`, `ToggleCapture` (`v`) → hands back `Action::SetCapture(bool)`, and the runtime-only `Copy` and `SetCapture`. Make the drag matcher tolerant and widen the divider hit zone.
- **Detail**:
  - Copy targets and notices per Approach, "Copy and capture"; drag rules per Approach, "Divider drag tolerance".
  - When there is nothing to copy, notice `nothing to copy`.
  - While capture is released, `keys::mouse` is never fed events, so no mouse branch changes for it.
  - Add legend rows for `H`, `y` and `v`, and append `"H"`, `"y"` and `"v"` to the key list in `every_bound_letter_key_has_a_legend_row` (Approach, "Wiring discipline").
  - `keys.rs` tests: during a drag, a re-sent `Down(Left)` and a `Moved` each map to `DragTo`, `Up(Left)` maps to `DragEnd`, and a press one cell right of the divider maps to `DragStart`.
- **Acceptance**:
  - forward: `grep -cE "KeyCode::Char\('(H|y|v)'\) =>" glimpse/src/keys.rs` prints `3` (today: `0`).
  - forward: `grep -cE '"(H|y|v)"' glimpse/src/view/legend.rs` prints at least `3` (today: `0`).
  - forward: `grep -c 'MouseEventKind::Up(_) | MouseEventKind::Down(_) => Some(Action::DragEnd)' glimpse/src/keys.rs` prints `0` (today: `1`).
  - guard: `legend::tests::every_bound_letter_key_has_a_legend_row` passes.

### 9. Width-fitted footer with short, full and hidden modes [M]
- **Files**: `glimpse/src/view/footer.rs` (new), `glimpse/src/view/mod.rs`
- **Depends on**: 3, 8
- **Backlog**: B-59aa6362, B-d1667850
- **Action**: Move `footer`, `surface_hints` and `inbox_hints` from `glimpse/src/view/mod.rs` into a new `pub(crate) mod footer`. Give each hint a priority, fit hints to the width per `App.hints`, and replace the footer's `Constraint::Length(1)` with `footer::height(app, width, now)`.
- **Detail**: Per Approach, "Hint row" and "Copy and capture". Add the hints for `H`, `y` and `v`. Priority, highest first, is each hint list's current order, with `? keys` pinned last and `…` immediately before it. `mouse released · v to capture` replaces the first hint while `!app.mouse_captured`, and still takes one row in hidden mode. The footer assertions already in the `glimpse/src/view/mod.rs` tests keep passing (`tab view` at 100 columns, `J/K scroll` at 45, `space mark`/`m actions`/`e classify` and `enter answer`/`n capture`/`c closed` at 120, `switch` in the selector). Tests at 60, 110 and 200 columns: the line always ends `? keys`, no hint is cut mid-word, full mode wraps to at most 3 rows, and hidden mode gives a height of 0 unless a notice is live.
- **Acceptance**:
  - forward: `grep -c 'fn surface_hints' glimpse/src/view/mod.rs` prints `0` (today: `1`).
  - forward: `grep -c 'fn surface_hints' glimpse/src/view/footer.rs` prints `1` (today: file absent).

### 10. OSC 52 encoder [S]
- **Files**: `glimpse/src/clipboard.rs` (new), `glimpse/src/main.rs`
- **Depends on**: 1
- **Backlog**: B-d4da8c1b
- **Action**: Create `pub(crate) fn osc52(text: &str) -> String`, which returns `\u{1b}]52;c;<base64>\u{1b}\\` using a hand-rolled RFC 4648 standard-alphabet base64 with `=` padding, and declare `mod clipboard;`.
- **Detail**: No dependency is added. Tests use the RFC 4648 §10 vectors ("" → "", "f" → "Zg==", "fo" → "Zm8=", "foo" → "Zm9v", "foobar" → "Zm9vYmFy") and the exact wrapper bytes.
- **Acceptance**:
  - forward: `grep -c '^mod clipboard;' glimpse/src/main.rs` prints `1` (today: `0`).
  - falsifier: the vector test fails if the padding is dropped (`"f"` → `"Zg"`). **predicted, unverified**.

### 11. Runtime carries out copy, capture toggle and the raw mouse-event log [M]
- **Files**: `glimpse/src/runtime.rs`, `glimpse/src/hook.rs`
- **Depends on**: 8, 10
- **Backlog**: B-d4da8c1b, B-dd0987c7
- **Action**:
  - Add `Host::copy(&mut self, text: &str)` and `Host::set_mouse_capture(&mut self, on: bool)`.
  - Handle `Action::Copy` and `Action::SetCapture` in `carry_out`, and keep `App.mouse_captured` in step.
  - Split `capture_mouse` into a once-only panic-hook install, made at start whatever `config.mouse` says, plus an enable/disable toggle.
  - Add the `GLIMPSE_EVENT_LOG` raw mouse-event log to the run loop.
- **Detail**: The real host writes `clipboard::osc52(text)` to stdout and flushes. Exit releases capture only when it is held. `carry_out` takes `&mut App` so it can set `mouse_captured` (its caller in `handle` already holds `&mut App`). Hook install per Approach, "Copy and capture"; log format, buffering, cap and failure handling per Approach, "Divider drag tolerance". `FakeHost` records `copy` and `set_mouse_capture` calls. A test checks that `y` on a review cursor row reaches `FakeHost` with that id.
- **Acceptance**:
  - forward: `grep -c 'GLIMPSE_EVENT_LOG' glimpse/src/runtime.rs` prints at least `1` (today: `0`).
  - forward: `grep -cE 'fn (copy|set_mouse_capture)\(' glimpse/src/runtime.rs` prints at least `2` (today: `0`).

### Milestone B — tables

### 12. New element tokens and revamp defaults [S]
- **Files**: `glimpse/src/theme.rs`
- **Depends on**: —
- **Backlog**: B-d1667850
- **Action**: Add the tokens `table_header` (muted, built with underline), `sort_mark` (accent), `strong` (accent), `backdrop` (faint), `selection_text` (idle) and `tab_empty` (faint) to `TOKENS`, with their `Theme` fields. Flood the active tab (`surface_tab_active` → `on_color` on `accent_bg`), give `slug` the `accent` colour, and build `badge` with the `strong` colour as well as bold.
- **Detail**: Per Approach, "Theme sets" and "Revamp". The `no_color` branch gives `backdrop` and `selection_text` the `DIM`/plain fallbacks the existing faint tokens use. Existing theme tests must keep resolving every token.
- **Acceptance**:
  - forward: `grep -cE '\("(table_header|sort_mark|strong|backdrop|selection_text|tab_empty)",' glimpse/src/theme.rs` prints `6` (today: `0`).
  - guard: the existing `theme::` tests pass.

### 13. Shared table core [M]
- **Files**: `glimpse/src/view/table.rs` (new), `glimpse/src/view/mod.rs`
- **Depends on**: 9, 12, 14
- **Backlog**: B-5e571ecd
- **Action**: Create `pub(crate) mod table` with the shared `Row` and `draw_rows`, plus `Column`, `header` and `cell`, per Approach, "Tables".
- **Detail**: Generalise the `Row`/`draw_rows` in `glimpse/src/view/items.rs` (fill, then line, then overlay, keeping chips intact) as `Row<T>` and `draw_rows<T: Clone>(..) -> Vec<(Rect, T)>`, since items and inbox key rows by `String` and layers by `u32`. `header` takes `surface::SortDir` from task 14. Leave the three local copies in place; tasks 15–17 switch to the shared one. Tests: header rects tile the columns, `cell` returns the id column's rect, and the sort mark is drawn beside the sorted column.
- **Acceptance**:
  - forward: `grep -c 'pub(crate) mod table;' glimpse/src/view/mod.rs` prints `1` (today: `0`).
  - falsifier: the header-rect test fails if `LEAD` is not added before the first column. **predicted, unverified**.

### 14. Sort direction and per-column sort keys [M]
- **Files**: `glimpse/src/surface.rs`, `glimpse/src/cli.rs`
- **Depends on**: 2
- **Backlog**: B-5e571ecd
- **Action**: Add `SortDir`, the new `Sort` variants, `HeaderKey`, `InboxColumn`, `ItemsState.sort_dir`, Inbox sort state, `cycle_header` and `flip_dir`. Make `compare` honour the direction, extend `Surface::sorts()`, and let `--sort` accept the new keys.
- **Detail**: Per Approach, "Tables". The `id_key` tie-break stays ascending whatever the direction. `cli::check_arrange` keeps refusing a key the surface lacks. Tests: the asc → desc → off cycle, Desc reversing Severity, and the Inbox sorting by kind.
- **Acceptance**:
  - forward: `grep -c 'enum SortDir' glimpse/src/surface.rs` prints `1` (today: `0`).
  - guard: the existing `surface::` and `tests/cli.rs` `--sort` cases pass.

### 15. Item tables draw a header row, id cells and empty states [M]
- **Files**: `glimpse/src/view/items.rs`, `glimpse/src/view/inbox.rs`
- **Depends on**: 3, 13, 14
- **Backlog**: B-5e571ecd, B-d4da8c1b, B-d1667850
- **Action**: Switch to `table::Row`/`draw_rows` and delete the local copies. Draw the header row below the facet line from `App.item_columns`, with the current sort marked. Fill `Drawn.headers: Vec<(Rect, HeaderKey)>` and `Drawn.ids: Vec<(Rect, String)>`. Lift muted and faint spans on the cursor row to `theme.selection_text`, and draw an empty state with a next step.
- **Detail**:
  - Add `headers` and `ids` to `Drawn`, which already derives `Default` (task 3), so the inbox's `..Drawn::default()` keeps compiling.
  - The empty state names the producing command per ledger: `/review`, `/optimise`, `/review-plan`, `/backlog`.
  - The page height now excludes the header row.
- **Acceptance**:
  - forward: `grep -c 'fn draw_rows' glimpse/src/view/items.rs` prints `0` (today: `1`).
  - guard: the existing `view::items` tests pass once their row offsets are updated for the header row.

### 16. Inbox table draws a header row [M]
- **Files**: `glimpse/src/view/inbox.rs`
- **Depends on**: 3, 13, 14, 15
- **Backlog**: B-5e571ecd
- **Action**: Switch to `table::Row`/`draw_rows`, draw the Inbox column header with its sort mark, fill `Drawn.headers` and `Drawn.ids`, and apply the selected-row lift and an empty-state next step.
- **Detail**: The Inbox's columns are `InboxColumn` from task 14. The unanswered-questions section stays first whatever the sort.
- **Acceptance**:
  - forward: `grep -c 'fn draw_rows' glimpse/src/view/inbox.rs` prints `0` (today: `1`).
  - guard: the existing `view::inbox` tests pass.

### 17. Layers view uses the shared row drawer [S]
- **Files**: `glimpse/src/view/layers.rs`
- **Depends on**: 13
- **Backlog**: B-d1667850
- **Action**: Replace the local `Row`/`draw_rows` with `table::Row`/`draw_rows`, and lift muted spans on the selected row to `theme.selection_text`.
- **Detail**: Layer rule rows and task targets behave exactly as today.
- **Acceptance**:
  - forward: `grep -c 'fn draw_rows' glimpse/src/view/layers.rs` prints `0` (today: `1`).
  - guard: the existing `view::layers` tests pass.

### 18. Column picker form [S]
- **Files**: `glimpse/src/actions.rs`
- **Depends on**: 6
- **Backlog**: B-5e571ecd
- **Action**: Add `pub(crate) fn columns_form(current: &[ItemColumn]) -> Form`, a single `Multi` field over `ItemColumn::ALL` with `current` checked, and `pub(crate) fn columns_from(form: &Form) -> Vec<ItemColumn>`.
- **Detail**: Ordering per Approach, "Tables". The field is required, so at least one column is checked. Tests: a round trip, and newly checked columns append.
- **Acceptance**:
  - forward: `grep -c 'fn columns_form' glimpse/src/actions.rs` prints `1` (today: `0`).

### 19. Wire header clicks, sort keys, id-cell copy and the column picker [L]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/keys.rs`, `glimpse/src/view/legend.rs`, `glimpse/src/view/mod.rs`, `glimpse/src/actions.rs` — adds `Purpose::Columns`
- **Depends on**: 8, 13, 14, 15, 16, 18
- **Backlog**: B-5e571ecd, B-d4da8c1b
- **Action**:
  - Add `Regions.headers: Vec<(Rect, HeaderKey)>` and `Regions.ids: Vec<(Rect, String)>`, filled in `view::render` from the list renders' `Drawn`.
  - Add the actions `SortBy(HeaderKey)` (header click), `FlipSortDir` (`D`), `OpenColumns` (`C`, item surfaces only) and `CopyItem(String)` (id-cell click → hands back `Action::Copy`).
  - Add `Purpose::Columns` in `glimpse/src/actions.rs` and apply a submitted column form to `App.item_columns` (`App::submitted` matches `Purpose` exhaustively, so the variant lands here, not in task 18).
  - Carry `sort_dir` through `App::apply_ledger`'s `ItemsState` rebuild, and add it to `App::edit_items`'s order key so `D` unpins a pinned list.
- **Detail**: Inseparable: the `Regions` fields, their fill in `view::render`, and the `keys::mouse` hit tests must land together to compile. In `keys::mouse`, an id-cell hit is checked before the row hit, and a header hit before both. Add legend rows for `D` and `C`, plus a "Chips and marks" row showing `▲ ▼` in `theme.sort_mark`, and append `"D"` and `"C"` to the key list in `every_bound_letter_key_has_a_legend_row`. Tests cover a header click cycling the sort, an id-cell click handing back `Copy`, and the `C` form submit changing the columns.
- **Acceptance**:
  - forward: `grep -cE 'pub\(crate\) (headers|ids):' glimpse/src/app.rs` prints `2` (today: `0`).
  - forward: `grep -cE '"(D|C)"' glimpse/src/view/legend.rs` prints at least `2` (today: `0`).
  - guard: `legend::tests::every_bound_letter_key_has_a_legend_row` passes.

### Milestone C — theme sets, picker and revamp

### 20. Named theme sets with style rules [L]
- **Files**: `glimpse/src/theme.rs`, `glimpse/src/theme/sets.rs` (new)
- **Depends on**: 12
- **Backlog**: B-d1667850
- **Action**: Create `glimpse/src/theme/sets.rs` (declared `pub(crate) mod sets;` from `glimpse/src/theme.rs`, since `config.rs` and `cli.rs` call `theme::sets::by_name`) with `ThemeSet`, `Rules`, `SETS` (`kanso-zen`, `kanso-pearl`, `ansi`, `mono`) and `by_name`. Add `Theme::from_set`, keep `Theme::build` as a wrapper, and add the per-set contrast test.
- **Detail**:
  - Per Approach, "Theme sets".
  - `kanso-pearl` maps kanso.nvim's pearl palette (`extras/ghostty/kanso-pearl`, MIT; `bg` `#f2f1ef`, `fg` `#22262D`) onto the 15 palette tokens: `idle` ← fg; `danger` ← red `#c84053`; `success` ← green `#6f894e`; `warning` ← yellow `#77713f`; `info` ← blue `#4d699b`; `violet` ← magenta `#b35b79`; `teal` ← cyan `#597b75`; `accent` and `accent_bg` ← orange `#cc6d00`; `on_color` ← bg; `muted`, `faint`, `subtle` and `highlight` ← fg mixed into bg at 70%, 50%, 30% and 10%. Every text token below 4.5:1 on bg (red 4.31, green 3.47, yellow 4.41, magenta 3.97, cyan 4.12, orange 3.23) is darkened in lightness until it passes, and a comment table in `sets.rs` records each source hex, final hex and ratio.
  - `ansi` uses only ANSI colour names. Its `Rules` use `Fill::Reverse` for selection, chips and flashes and `Faint::Dim` for `faint` and `subtle`. Bold styles in `ansi` use the bright slot, so bold does not shift hue.
  - `mono` reproduces today's `no_color` output exactly; a test compares the two.
  - The contrast test computes WCAG ratios over the named lists in Approach, "Theme sets": the text tokens at least 4.5:1 on each RGB set's `bg`, and `selection_text` on `selection_bg` and `on_color` on `accent_bg` at least 4.5:1. The de-emphasised tiers (`faint`, `subtle` and the tokens defaulting to them: `border`, `layer_rule`, `key_separator`, `edge_faded`, `unrelated`, `tab_empty`, `backdrop`) are exempt and named in the test. `ansi` and `mono` are exempt, since they follow the terminal palette.
- **Acceptance**:
  - forward: `grep -cE '"(kanso-zen|kanso-pearl|ansi|mono)"' glimpse/src/theme/sets.rs` prints at least `4` (today: file absent).
  - falsifier: the contrast test fails if `kanso-zen`'s `muted` is set to `subtle` (`#4E5561`). **predicted, unverified**.

### 21. Theme config: base set and per-set overrides [M]
- **Files**: `glimpse/src/config.rs`
- **Depends on**: 7, 20
- **Backlog**: B-d1667850
- **Action**: Accept `[theme] base = "<set>"` and `[theme.<set>]` subtables in `theme_table`, storing them as `Config.theme_base: Option<String>` and `Config.theme_sets: HashMap<String, HashMap<String, String>>`. Validate set names through `theme::sets::by_name`, and add the non-file field `Config.theme_from_flag: bool` (default `false`) that task 22 sets.
- **Detail**: `Config.theme` keeps its type and meaning (string tokens), so old configs parse unchanged. An unknown set name or token fails the file. Tests: a legacy `[theme]` table, `base = "ansi"`, a `[theme.ansi]` override, and an unknown base.
- **Acceptance**:
  - forward: `grep -c '"base"' glimpse/src/config.rs` prints at least `1` (today: `0`).
  - guard: the existing `config::` theme tests pass.

### 22. `--theme` flag [M]
- **Files**: `glimpse/src/cli.rs`, `glimpse/src/main.rs`, `glimpse/tests/cli.rs`
- **Depends on**: 10, 14, 21
- **Backlog**: B-d1667850
- **Action**: Parse `--theme <set>`, validated against `theme::sets::by_name`. In `glimpse/src/main.rs`, before `App::new`, apply it over `config.theme_base` and set `config.theme_from_flag = true`. Add a binary case that runs `--once --theme ansi` and expects exit 0, and one with `--theme nope` expecting a usage error.
- **Detail**: `Config.theme_from_flag` (added by task 21) tells task 36 that state must not override the theme. The usage text lists the sets. Keep the binary cases inside the `tests/cli.rs` sandbox.
- **Acceptance**:
  - forward: `grep -c '"--theme"' glimpse/src/cli.rs` prints at least `1` (today: `0`).
  - forward: `cargo run --manifest-path glimpse/Cargo.toml -q -- --once --snapshot glimpse/tests/fixtures/snapshot.json --size 110x40 --theme ansi` exits 0, as the new `glimpse/tests/cli.rs` case asserts (today: exit 2, `unknown argument --theme`; the installed binary changes only after `cargo install --path glimpse`). **predicted, unverified** (builds).

### 23. Wire the theme picker and live switching [L]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/keys.rs`, `glimpse/src/view/legend.rs`
- **Depends on**: 19, 20, 21
- **Backlog**: B-d1667850
- **Action**:
  - `App::new` builds the theme through `Theme::from_set`, from `theme_base` (default `kanso-zen`; `NO_COLOR` → `mono`), and keeps `theme_set: String` and the overrides.
  - Add `App.theme_picker: Option<ThemePicker{cursor, original}>`, opened by `ToggleThemes` (`T`). With the picker open, `Move` previews the set under the cursor, `Details` (Enter) keeps it with a notice, and `Back` reverts.
- **Detail**: Per Approach, "Theme picker". Place the picker first in the `Action::Back` chain, and give `keys::mouse` the picker branch that Approach describes. Add a legend row for `T` and append `"T"` to the key list in `every_bound_letter_key_has_a_legend_row`. Tests: preview changes `app.theme`, Back restores the original, and Enter keeps the new set.
- **Acceptance**:
  - forward: `grep -c "KeyCode::Char('T') =>" glimpse/src/keys.rs` prints `1` (today: `0`).
  - forward: `grep -c '"T"' glimpse/src/view/legend.rs` prints at least `1` (today: `0`).
  - guard: `legend::tests::every_bound_letter_key_has_a_legend_row` passes.

### 24. Theme picker overlay [M]
- **Files**: `glimpse/src/view/themes.rs` (new), `glimpse/src/view/mod.rs`, `glimpse/src/view/footer.rs`
- **Depends on**: 9, 19, 23
- **Backlog**: B-d1667850
- **Action**: Create `pub(crate) mod themes` to draw the picker, with set names, the current mark, the cursor row and a small sample of status chips in the live theme. Draw it from `view::render` after the legend and before forms, and add the picker's footer hints in `glimpse/src/view/footer.rs`.
- **Detail**: Follow `glimpse/src/view/selector.rs`'s `Clear` + bordered block layout. Task 28 later moves that block onto the panel helper. Test: with the picker open, a `TestBackend` frame shows all four set names.
- **Acceptance**:
  - forward: `grep -c 'pub(crate) mod themes;' glimpse/src/view/mod.rs` prints `1` (today: `0`).

### 25. Header: flooded active tab, faint empty tabs, history on tall panes [M]
- **Files**: `glimpse/src/view/header.rs`, `glimpse/src/view/mod.rs`
- **Depends on**: 12, 24
- **Backlog**: B-d1667850
- **Action**: Draw the active surface tab as a flooded chip (`surface_tab_active` on `surface_tab_active_bg`). Draw tabs whose ledger is absent in `theme.tab_empty`, dropping the `—` (compact `P—` tabs too). Show the history row only when the pane is at least 30 rows tall, passing the pane height to `header::height` and `header::render` from their calls in `view::render`.
- **Detail**: `header::height` must agree with the row it draws, since `view::render` reserves it. Only the flow event is gated on height: `second_row`'s `source_error` and config `warning` stay visible on short panes. Update the existing `view::header` tests that assert the `—` text or the history row in a short frame. Tests: an absent-ledger tab's cells use `tab_empty`, and a 24-row pane has no history row but still shows a source error.
- **Acceptance**:
  - forward: `grep -c 'tab_empty' glimpse/src/view/header.rs` prints at least `1` (today: `0`).
  - guard: the existing `view::header` tests pass once the `—` and short-frame history assertions are updated.

### 26. Markdown strong without bold [S]
- **Files**: `glimpse/src/view/markdown.rs`
- **Depends on**: 12
- **Backlog**: B-d1667850
- **Action**: Style `**strong**` with `theme.strong` instead of `theme.badge`.
- **Detail**: Leave italics, strikethrough and links as they are.
- **Acceptance**:
  - forward: `grep -c 'theme.strong' glimpse/src/view/markdown.rs` prints at least `1` (today: `0`).
  - forward: `grep -c 'theme.badge' glimpse/src/view/markdown.rs` prints `0` (today: `1`).

### 27. Panel helper [M]
- **Files**: `glimpse/src/view/panel.rs` (new), `glimpse/src/view/mod.rs`, `glimpse/src/view/activity.rs`
- **Depends on**: 24, 25
- **Backlog**: B-d1667850
- **Action**: Create `pub(crate) mod panel` with `pub(crate) fn block<'a>(title: &'a str, theme: &Theme) -> Block<'a>`: rounded borders in `theme.border`, the title in `border_title` and `Padding::horizontal(1)`. Move `glimpse/src/view/activity.rs`'s panel onto it.
- **Detail**: Per Approach, "Revamp". `view::render`'s `Block::bordered().inner(rect)` details-page arithmetic stays valid, because rounded borders take the same cells.
- **Acceptance**:
  - forward: `grep -c 'Block::' glimpse/src/view/activity.rs` prints `0` (today: `1`).
  - guard: the existing `view::activity` tests pass.

### 28. Details, selector and form panels use the helper [M]
- **Files**: `glimpse/src/view/details.rs`, `glimpse/src/view/selector.rs`, `glimpse/src/view/form.rs`
- **Depends on**: 5, 6, 27
- **Backlog**: B-d1667850
- **Action**: Replace each hand-built bordered `Block` with `panel::block`.
- **Detail**: Task 31 moves the theme picker in `glimpse/src/view/themes.rs`. Keep test-only `Block` uses, if any.
- **Acceptance**:
  - forward: `grep -c 'Block::' glimpse/src/view/details.rs glimpse/src/view/selector.rs glimpse/src/view/form.rs` prints `0` for each file (today: `1` each).

### 29. Traversal card uses the helper [S]
- **Files**: `glimpse/src/view/ego.rs`
- **Depends on**: 27
- **Backlog**: B-d1667850
- **Action**: Build the traversal card's block with `panel::block`.
- **Detail**: `panel::block` supplies the rounded border and padding; the card then re-applies its status-coloured border style and selection-styled title over it, so its look is unchanged.
- **Acceptance**:
  - forward: `grep -c 'BorderType::Rounded' glimpse/src/view/ego.rs` prints `0` (today: `1`).
  - guard: the existing `view::ego` tests pass.

### 30. Legend panel uses the helper [S]
- **Files**: `glimpse/src/view/legend.rs`
- **Depends on**: 23, 27
- **Backlog**: B-d1667850
- **Action**: Build the legend's block with `panel::block`.
- **Detail**: Scroll bounds and the scrollbar are unchanged.
- **Acceptance**:
  - forward: `grep -c 'Block::' glimpse/src/view/legend.rs` prints `0` (today: `1`).
  - guard: `legend::tests::every_bound_letter_key_has_a_legend_row` passes.

### 31. Modal backdrop, comfortable gutter and the picker panel [M]
- **Files**: `glimpse/src/view/mod.rs`, `glimpse/src/view/themes.rs`
- **Depends on**: 24, 27
- **Backlog**: B-d1667850
- **Action**:
  - In `view::render`, restyle the frame with `theme.backdrop` before drawing the selector, legend, theme picker or a form.
  - In comfortable density, inset the body by one cell on the left.
  - Move `glimpse/src/view/themes.rs` onto `panel::block`.
- **Detail**: The backdrop must not alter `Regions` (the gutter shifts every rect; recompute them from the inset body). Tests: with the legend open, a cell outside the legend carries the backdrop style, and in comfortable density column 0 of the body is blank.
- **Acceptance**:
  - forward: `grep -c 'theme.backdrop' glimpse/src/view/mod.rs` prints at least `1` (today: `0`).
  - guard: the existing `view::tests` pass once their offsets are updated for the gutter.

### Milestone D — diagram focus, zoom and fit

### 32. Neighbourhood subsets and the fit search [M]
- **Files**: `glimpse/src/diagram/subset.rs` (new), `glimpse/src/diagram/mod.rs`
- **Depends on**: —
- **Backlog**: B-3dbe27a1
- **Action**: Create `pub(crate) mod subset` with `Focus`, `generations`, `families`, `fit` and `Subset`, per Approach, "Diagram focus, zoom and fit". Declare it in `glimpse/src/diagram/mod.rs`.
- **Detail**: Walk `needs` edges from the snapshot. glimpse has no closure helper, and tomlctl's `Graph::closure_up`/`closure_down` are `pub(crate)` there. `fit` takes a `fits` predicate so it is testable without layout; the radius search is per Approach, "Diagram focus, zoom and fit". Tests on the `wide()`, `shortcut()` and `two_components()` builders in `glimpse/src/diagram/mod.rs` tests (made `pub(super)`, since they are private to `diagram::tests`): radius-1 ancestry, Both at radius 2, `fit` stopping at the largest radius the predicate accepts, and `hidden` counting the rest.
- **Acceptance**:
  - forward: `grep -c 'pub(crate) mod subset;' glimpse/src/diagram/mod.rs` prints `1` (today: `0`).

### 33. Layout of a node subset at a zoom level [L]
- **Files**: `glimpse/src/diagram/mod.rs`, `glimpse/src/diagram/position.rs`
- **Depends on**: 32
- **Backlog**: B-3dbe27a1
- **Action**:
  - Add `Zoom{Titles, Ids, Tight}`.
  - Split `layout()` into `layout_of(layers, edges, orientation, zoom, label_width)`.
  - Make the task gap a parameter through a new `position::position_with_gap`, keeping `position::position` and `TASK_GAP` (which the `position.rs` docs link) so the `route.rs` and `position.rs` test call sites compile unchanged.
  - Add `DiagramCache::layout_for(snapshot, index, orientation, view: (u16, u16), target, focus, fit, zoom)`. It runs `subset::fit` with `layout_of` measurements, skips the search when the fit inputs match the last search, and keys the cache per Approach, "Diagram focus, zoom and fit". Keep `DiagramCache::layout` as the full-graph, `Zoom::Ids` call for tests only, and move `diagram::navigator` onto the layout `layout_for` last stored.
- **Detail**: Subset rules (filter layers, drop empty, renumber, keep both-ends edges), the fit predicate, the Titles title hash and the navigator rule per Approach, "Diagram focus, zoom and fit". Titles labels use task titles from the snapshot. The returned `Layout` carries `hidden: usize`. Tests: the edge-connectivity and glyph-arm invariants hold on a subset layout in both orientations; the cache recomputes when the zoom, subset or (at `Zoom::Titles`) a title changes and is reused otherwise; two consecutive `layout_for` calls with unchanged inputs leave `computations` unchanged, run no fit search and keep a pinned scroll; and the Tight layout is narrower than Ids on `wide()`.
- **Acceptance**:
  - forward: `grep -cE 'fn layout_of|enum Zoom' glimpse/src/diagram/mod.rs` prints `2` (today: `0`).
  - guard: the existing `diagram::` tests pass.

### 34. Wire focus, zoom, fit and drag-pan [L]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/keys.rs`, `glimpse/src/view/legend.rs`
- **Depends on**: 23, 30, 33
- **Backlog**: B-3dbe27a1
- **Action**:
  - Add the `App` fields `diagram_focus`, `diagram_zoom` (default `Ids`) and `diagram_fit` (default `true`).
  - Add the actions `CycleFocus` (`F`), `Zoom(bool)` (`z` out, `Z` in, Ctrl+wheel over the diagram) and `ToggleFit` (`x`), each with a notice.
  - Keep `App.dragging` for the divider and add `App.pan: Option<(u16, u16)>`, which a left press on diagram background starts.
- **Detail**: Drag rules per Approach, "Divider drag tolerance" and "Diagram focus, zoom and fit". Pan motion becomes `ScrollView(dx, dy)`, which pins. Update the `keys.rs` test that asserts `z` is unbound. Add legend rows for `F`, `z`, `Z` and `x` and a "Chips and marks" row for the `+n hidden` chip, and append `"F"`, `"z"`, `"Z"` and `"x"` to the key list in `every_bound_letter_key_has_a_legend_row`. Tests: Ctrl+ScrollUp over the diagram zooms in, a background press then a drag pans, and a press on a node still selects it.
- **Acceptance**:
  - forward: `grep -cE "KeyCode::Char\('(F|z|Z|x)'\) =>" glimpse/src/keys.rs` prints `4` (today: `0`).
  - forward: `grep -cE '"(F|z|Z|x)"' glimpse/src/view/legend.rs` prints at least `4` (today: `0`).
  - guard: `legend::tests::every_bound_letter_key_has_a_legend_row` passes.

### 35. Paint zoom levels, the hidden marker and clickable nodes [M]
- **Files**: `glimpse/src/diagram/paint.rs`, `glimpse/src/view/mod.rs`
- **Depends on**: 31, 33, 34
- **Backlog**: B-3dbe27a1
- **Action**:
  - `paint::render` calls `DiagramCache::layout_for` with `App.diagram_*`, the view size and the selection, and draws the labels per zoom: `[id title…]`, `[id]`, or a status glyph.
  - It draws a `+n hidden` chip in the view's top-right corner when `hidden > 0`.
  - `view::draw_view` returns diagram node targets (`NodeBox` minus scroll) so clicks select nodes.
- **Detail**: The chip uses `theme.strong`, per Approach, "Revamp". `follow`/`pan` keep working on the subset layout. Tests: at Tight zoom a node is one cell, a fit-limited frame shows the `hidden` chip, and a click target exists for the selected node.
- **Acceptance**:
  - forward: `grep -c 'hidden' glimpse/src/diagram/paint.rs` prints at least `1` (today: `0`).
  - guard: the existing `diagram::paint` tests and `tests/cli.rs` `once_renders_the_diagram_view` pass.

### Milestone E — persistence and docs

### 36. Persist hints, columns, sort direction, theme and diagram view [M]
- **Files**: `glimpse/src/state.rs`, `glimpse/src/runtime.rs`
- **Depends on**: 8, 11, 19, 22, 23, 34
- **Backlog**: B-59aa6362, B-5e571ecd, B-d1667850, B-3dbe27a1
- **Action**: Extend `State` parse, render, capture and apply with `hints`, `item_columns`, an `arrangements` direction, `theme`, `zoom` and `fit`.
- **Detail**: Per Approach, "Persistence". `State::apply` gains `keep_theme: bool`, which `runtime::run` sets from `config.theme_from_flag || config.no_color` at the existing `State::load().apply(..)` call, and it ignores an unknown saved set name rather than failing. `capture` writes `theme` and `hints` only when chosen in-app, and saves the Inbox's sort and direction beside the item surfaces'. Tests: a round trip, a legacy state.toml parsing unchanged, the flag beating state, a `NO_COLOR` run leaving no `theme` key, and an in-app choice round-tripping.
- **Acceptance**:
  - forward: `grep -cE 'text\("(hints|theme|zoom)"\)' glimpse/src/state.rs` prints `3` and `grep -c '"fit"' glimpse/src/state.rs` prints at least `1` (today: `0` each).
  - guard: the existing `state::` tests pass.

### 37. Document keys, config, themes, copy and the event log [M]
- **Files**: `glimpse/README.md`, `glimpse/CLAUDE.md`
- **Depends on**: 11, 19, 24, 35, 36
- **Backlog**: B-d4da8c1b
- **Action**:
  - Update `glimpse/README.md`:
    - "Views and keys": the paging contract and the new keys.
    - "Configuration": `hints`, `[theme] base` and `[theme.<set>]`.
    - "Theme": rows for the six new tokens and the four sets.
    - Copy: `y`, id-cell click, `v`, Shift+drag in ghostty/Windows Terminal, and herdr's copy-on-select.
    - `GLIMPSE_EVENT_LOG`.
    - Diagram focus, zoom and fit.
    - The ghostty `keybind = shift+page_up=unbind` lines (and `page_down`/`home`/`end`), since ghostty on Linux/Windows consumes those Shift keys.
    - Passages the plan makes wrong: the usage summary (`--theme`), the tab examples and `—` text, the facet-row and item-row description, the per-surface `S` sort list, both lists of what `state.toml` saves, the diagram `[id]` description, the wheel-and-click paragraph, the flag-override sentence and the `NO_COLOR` sentence.
  - Update `glimpse/CLAUDE.md`: the cache-key gotcha (zoom, the subset and Titles), the Structure list (the new modules), the mouse-capture gotcha (the once-only hook install and the `v` toggle) and the "Mouse hits read the last frame" write-back list.
- **Detail**: Keep each fact in the one section that owns it. Derive the key list from `view/legend.rs` `KEYS` rather than transcribing it.
- **Acceptance**:
  - forward: `grep -c 'GLIMPSE_EVENT_LOG' glimpse/README.md` prints at least `1` (today: `0`).
  - forward: `grep -ci 'shift.drag' glimpse/README.md` prints at least `1` (today: `0`).

### 38. Released-capture footer line only after a v release [S]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/view/footer.rs`
- **Depends on**: 8, 9
- **Action**: Add an `App` flag recording that the user released capture with `v` (set when `ToggleCapture` hands back `SetCapture(false)`, cleared when it hands back `SetCapture(true)`), and make `view/footer.rs` show `mouse released · v to capture` (and drop the `v select text` hint, and keep the one hidden-mode row) only while that flag is set and `!app.mouse_captured`. A session started with `mouse = false` keeps its normal hints. Tests: a `mouse = false` app shows the normal first hint; after `v` twice (capture on, then released) the released line shows.
- **Acceptance**:
  - forward: the new footer test fails if the released line keys on `!app.mouse_captured` alone.
  - guard: `cargo test --manifest-path glimpse/Cargo.toml view::footer` and `app::` pass.

### 39. Filter by section and document the released footer line [S]
- **Files**: `glimpse/src/surface.rs`, `glimpse/src/view/footer.rs`
- **Depends on**: 14, 38
- **Action**: In `glimpse/src/surface.rs`, make `ItemsState::shows` build an item's place text through the same helper the area sort uses (`place()`, which includes `Anchor::Section`), so the `/` filter finds plan-review rows by section. Add a test: a plan-review item anchored to a section matches a filter on that section name. In `glimpse/src/view/footer.rs`, correct the module doc so it states that the `mouse released · v to capture` line (and its one hidden-mode row) shows only after a `v` release, not whenever capture is off.
- **Acceptance**:
  - falsifier: the new section-filter test fails if `shows` keeps its own anchor match without `Anchor::Section`.
  - guard: `cargo test --manifest-path glimpse/Cargo.toml surface::` and `view::footer` pass.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 5, 6, 11, 38 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 38. The navigation contract, footer modes, copy, capture release and drag tolerance; builds and runs on its own

— CHECKPOINT B after tasks 17, 19 — dependency closure: 1, 2, 3, 4, 6, 7, 8, 9, 12, 13, 14, 15, 16, 17, 18, 19. Shared table core, header rows, sorting, id-cell copy and the column picker

— CHECKPOINT C after tasks 22, 26, 28, 29, 30, 31 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31. Theme sets, config, flag, picker and the revamp

— CHECKPOINT D after tasks 35 — dependency closure: 1, 2, 3, 4, 6, 7, 8, 9, 12, 13, 14, 15, 16, 18, 19, 20, 21, 23, 24, 25, 27, 30, 31, 32, 33, 34, 35. Diagram focus, zoom, fit, drag-pan and clickable nodes

— CHECKPOINT E after tasks 37, 39 — dependency closure: 1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 22, 23, 24, 25, 27, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39. Persistence and docs

## Risks
- **Paging keys change meaning.** PgUp/PgDn/Home/End used to scroll details. They now move the focused list's cursor; details move to Shift+keys and J/K. Mitigation: legend, footer and README all state the contract, and full-screen details still take plain PgUp/PgDn. ghostty on Linux/Windows binds Shift+PgUp/PgDn/Home/End to its own scrollback by default, so those keys never reach glimpse there; J/K still work, and task 37's README gives the unbind lines.
- **A lost `Up` keeps a drag alive.** With `Moved` → `DragTo`, the divider follows the pointer until the next click. Mitigation: any button press other than a left press ends the drag, and the event log shows the real stream after merge.
- **Touch may never reach glimpse.** herdr or ghostty may consume the touch drag. Mitigation: the event log proves it either way. If no events arrive, the fix belongs in herdr or ghostty config, and a new backlog item is captured.
- **Fit search cost.** Each selection move under fit re-runs `layout_of` on several candidate subsets (at most 512 nodes). Mitigation: grow the radius by doubling, then bisect; the fit result is cached on its inputs, so an unchanged frame runs no search (task 33's test). Measure on the largest real flow after merge (After Merge).
- **Theme regressions.** The ANSI set relies on reverse video and DIM, and Windows Terminal draws DIM much fainter. Mitigation: `faint` falls back through rules, the contrast test gates the RGB sets, and `mono` stays byte-identical to today's NO_COLOR output.
- **Test churn from the header row and gutter.** Buffer-position tests in `items`, `inbox` and `view::tests` shift. Mitigation: the tasks that move the offsets (15, 16, 31) own those test files.
- **Wide scope.** About 37 files. Mitigation: five checkpoints, with each milestone's logic in its own files and a short serial wiring chain.

## After Merge
- `cargo install --path glimpse` to put the new binary on PATH, then restart running glimpse panes.
- Run `GLIMPSE_EVENT_LOG=<path> glimpse`, touch-hold-drag the divider in ghostty, ghostty+herdr and Windows Terminal, and inspect the log. If no `Down`/`Drag` lines appear, capture a backlog item against herdr/ghostty config.
- Run the two `--once` success criteria by hand against the installed binary.
- On the largest real flow, open the diagram with fit on and move the selection repeatedly; if a move visibly lags, capture a backlog item for the fit search cost.
- Do the manual pass listed under `## Verification Commands`.

## Exploration Notes

### Input / navigation (keys.rs, app.rs, surface.rs, runtime.rs)
- `keys::map(KeyEvent)` :22 and `keys::mouse(MouseEvent,&App)` :91 are pure; `scroll_view` :160; `WHEEL_ROWS=3` :18. Mouse order: (1) `app.dragging`: `Drag(Left)`→`DragTo`, any `Up(_)`/`Down(_)`→`DragEnd`, `Moved`→None; (2) overlay→None; (3) legend wheel; (4) selector wheel; (5) `Down(Left)` tab hit→`SwitchSurface` (:121) else `regions.modal.is_none()` + divider hit→`DragStart` (:124); (6) item row→`SelectItem`, task row→`Select`; wheel→details or view.
- 50349c0 did NOT touch the drag branch; drag state machine dates from f640b6a (2026-09-30). Tab rects (header rows) never overlap the 1-cell divider (body). Leads for B-dd0987c7: 1-cell target hard to hit by touch; touch stream re-sending `Down`, or emitting `Moved`/buttonless motion, ends/stalls the drag. Raw ghostty event capture needed before changing the matcher.
- `app.rs`: `Scroll{Up(u16),Down(u16),PageUp,PageDown,Top,Bottom}` :84, `ListScroll` :95, `Regions` :104, `Action` :122, `App` fields :202, `App::apply` :385, `drag_to` :1269, `scroll_details` :1298 and `scroll_legend` :1313 are line-for-line duplicates, `notify` :1254 (1.5 s `NOTICE`, `theme.notice`), `notify_error` 8 s.
- Scroll state today: Tasks layers `App.layers_scroll: ListScroll` + `scroll_pinned`/`scroll_nudge`; item lists + Inbox `surface::Tracking{scroll,scroll_pinned,cursor}` (:223, shared by `ItemsState` :350 and `InboxState` :526) — Up/Down `ItemMove`, wheel `scroll_by`, NO PgUp/PgDn/Home/End (those go to details); details `details_scroll/max/page`; legend `legend_scroll/max/page`; selector `selector_cursor` with no windowing (cursor can fall off screen); form `Field::Select{cursor}` via ratatui `ListState`, `Multi` unwindowed; activity none.
- Nearest shared helper: `view::place(offset, at, len, view, Place)` view/mod.rs:77-120, used by items.rs:94, inbox.rs:66, layers.rs:286/313/342.
- Hint row: `footer()` view/mod.rs:436 in `Constraint::Length(1)` (:152); `surface_hints` :527, `inbox_hints` :572; a live notice replaces the hints. Legend `KEYS` legend.rs:20, test `every_bound_letter_key_has_a_legend_row` legend.rs:424.
- Runtime effects pattern: `apply` returns runtime-only steps (`Quit`, `SwitchFlow`) executed in `runtime::carry_out` :525 via the `Host` trait (:305; `FakeHost` in tests). `capture_mouse` :133 / `release_mouse` :142 — `capture_mouse` chains a new panic hook per call (selection mode needs enable/disable split from hook install).
- No clipboard code. crossterm 0.29's `CopyToClipboard` needs the `osc52` feature; Cargo.toml rule: crossterm only via `ratatui::crossterm` → hand-roll OSC 52 + base64.
- Unbound keys: `y p b x z v H`.

### Tables / views / theme
- No ratatui `Table`; items.rs, inbox.rs, layers.rs each hand-roll `Row` + `draw_rows` (three copies). No header row anywhere; items row 0 is a facet line (`group … · sort … · closed … · /filter`).
- items.rs: private `Columns{id,class,category,effort,area}` measured per frame (`AREA_MAX=28`, `AREA_MIN=10`, `SUMMARY_MIN=20`, `LEAD=2`); `items::render(buf,area,&App)->(Vec<Target>,usize)`; inbox has its own `Columns{id,kind,status,author}`, sort hard-coded `id_key`.
- Sort in surface.rs: `enum Sort{Id,Severity,Effort,Newest}` :165, `Group` :123, `Surface::sorts()/groups()` :93-119, `compare(sort,a,b)` :787 (single key, ascending, `id_key` tie-break), `cycle_sort` :491/`cycle_group` via `next_of` :804. Keys `S`/`g` (keys.rs:67-68). CLI `--sort/--group/--closed` for `--once` (cli.rs:252, `check_arrange` ~:365). No direction / off state.
- config.rs: `ItemColumn{Status,Id,Kind,Category,Effort,Summary,Area}` :237; `Config` :280 (`item_columns`, `theme: HashMap`, `no_color`, `mouse`, `cell_aspect`); strict `Config::parse` :340 (unknown key fails); path `$GLIMPSE_CONFIG` else `~/.config/glimpse/config.toml` (:529). Loaded once main.rs:82. **glimpse never writes config** (state.rs doc).
- state.rs: `State{view,orientation,split,panel_percent,show_implied,surface,arrangements:Vec<(Surface,Group,Sort)>}` → `<claude dir>/glimpse/state.toml`, tmp+rename `save()` :171, loaded runtime.rs:79, saved on exit :115. Template for persisting columns / theme name / hint-row.
- theme.rs: `TOKENS` :19 (15 palette tokens + element aliases, `MAX_DEPTH=8`); `parse_color` via `Color::from_str` (ANSI names, 0-255, `default`); `Theme::build(overrides,no_color)` :235 → ~60 `Style` fields + helpers; built once in `App::new` (app.rs:347); single Kanso Zen theme shared with terminal/herdr/gitui. Views name no `Color` (only tests in details.rs:813/822). CLAUDE.md rule: new element token → TOKENS + README table row (README:385-396) + legend row if a mark.
- Overlays: `actions::Overlay{Menu,Form,Prompt}` actions.rs:211; selector/legend are booleans on App. Precedence in three places: `Action::Back` chain (app.rs ~785), `footer()` branches, draw order view/mod.rs:208-217. `SelectorEntry` app.rs:195 is flow-specific.
- Reusable: `chip()`, `place()`, the bordered `Block` idiom (selector/legend/form/activity/details), legend draws in live theme (preview).

### Diagram
- Pipeline `layout()` diagram/mod.rs:313: `edges(snapshot,show_implied)` :233 (uses `reduce::implied_edges(&Snapshot)` reduce.rs:17) → `layers(&Snapshot)` :209 → `components` :269 → per component `order::order(layers,edges)->Ordered` order.rs:86 → `position::position(&Ordered,label_width)->Positions` position.rs:62 → `position::pack` :630 → `route::route(&Ordered,&Positions)->Routed` route.rs:92 → `Layout{orientation,edges,nodes:Vec<NodeBox>,strokes,width,height,rows,centre}` mod.rs:54.
- Subset-ready: order/`Graph::build` skip edges with missing endpoints (order.rs:151); route skips chains with unplaced slots (route.rs:103-111). Only `layout()`'s inputs assume the full graph; empty interior layers still cost a row + channel → subset must renumber layers. Natural seam: `layout_of(layers, edges, orientation)`.
- Cache `DiagramCache{key:Option<(u64,Orientation,bool)>,…,scroll}` mod.rs:79, `refresh()` :114 resets scroll; key = `Index::topology_hash()` model.rs:482. CLAUDE.md: any new layout input must be in the key.
- Viewport: `paint::render_at` paint.rs:30 — `app.scroll_pinned` → `pan(offset,by,view,total)` :125 with `app.scroll_nudge`, else `follow(...)` :134 (margin `min(view/4,2)`); per-cell clip in `put()` :113; `Action::ScrollView` app.rs:551 pins; `App::select` clears pin :1288; nudge zeroed view/mod.rs:176. No zoom/compact rendering (`Density` used by ego/layers only). Node = one row `[id]`, `label_width = digits+2` mod.rs:203; `TASK_GAP=2` position.rs:25.
- Diagram nodes are not clickable: `draw_view` returns empty targets view/mod.rs:375; `NodeBox` minus scroll → `layers::Target=(Rect,u32)` directly.
- No ancestor/descendant closure in glimpse: pieces are `Index::dependents/coupled/overlaps/layer_of` model.rs:414-432, private memoised `reduce::reachable` reduce.rs:51, ego.rs `side_of` :50 / `second_hop` :100 / `peers` :90, layers.rs `Focus` :391. tomlctl `Graph::closure_up/down` are `pub(crate)` — unreachable.
- Navigator `navigator(...)` mod.rs:128 rebuilt every frame (view/mod.rs:219), trait at app.rs:73. Snapshot cap 512 nodes.
- Tests: per-stage unit tests (order.rs:322, position.rs:700, route.rs:361, reduce.rs:69, glyph.rs:152), integration mod.rs:508 (cache reuse, edge connectivity, glyph arms, navigator reachability), paint.rs:151 TestBackend; fixtures `tests/fixtures/snapshot.json` (8 tasks/3 layers), hand-built `wide()`/`two_components()`/`shortcut()`.

### Build / test / lint
- `cargo test --manifest-path glimpse/Cargo.toml` (465 inline `#[test]` per `grep -rh '#\[test\]' glimpse/src | wc -l` + `tests/cli.rs` 20 binary tests); `--test cli` alone.
- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`; `cargo fmt --manifest-path glimpse/Cargo.toml -- --check` (pre-commit).
- Headless: `glimpse --once --snapshot glimpse/tests/fixtures/snapshot.json --size 110x40 [--view diagram]`.
- Install: `cargo install --path glimpse`. No nextest config. Edition 2024, rust 1.98, ratatui 0.30, rustdoc broken links denied.
- Prose surfaces: `glimpse/README.md` (keys, config keys, theme token table), `view/legend.rs` KEYS, `glimpse/CLAUDE.md`.

### Backlog
All live rows under `glimpse/`, all `open`, none promoted elsewhere: B-f15577d8 (annoyance, scroll unify), B-5e571ecd (direction, table headers/sort/columns), B-d4da8c1b (direction, copy), B-3dbe27a1 (direction, diagram focus/pan/zoom/fit), B-59aa6362 (direction, hint-row toggle), B-dd0987c7 (bug, ghostty touch divider drag), B-d1667850 (direction, design sweep + theme sets + ANSI). No seed flow.

## Research Notes

### Terminal input / output mechanics (research-lite; vet: 3 sampled, 0 dropped, 1 downgraded)
- **Mouse classification** (crossterm 0.29.0 `event/sys/unix/parse.rs` `parse_cb`; high): button 0/1/2 + motion bit → `Drag(btn)`; buttonless motion → `Moved`; SGR trailing `m` → `Up`. On Windows (`event/sys/windows.rs`; high) crossterm writes no DECSET bytes — it sets console mode `0x98` (mouse+extended+window input, Quick Edit off); `MOUSE_EVENT` records with no button → `Moved`, else `Drag`. ConPTY translates to the host. Disable restores the mode saved at the first enable, so toggling capture mid-session after raw mode is safe. Impact: a selection mode can `DisableMouseCapture`/`EnableMouseCapture` freely once the panic hook install is split from the toggle.
- **Touch** (low — hypothesis only, no primary source): a touch hold-drag may arrive as `Down(Left)`+`Drag(Left)`, as wheel events, as a re-sent `Down`, as buttonless `Moved`, or be eaten by herdr/host selection. Must be captured on the device (ghostty, ghostty+herdr, WT, WT+herdr). Impact: ship an opt-in raw-event log, plus a matcher tolerant of re-sent `Down` / `Moved` mid-drag and a wider divider hit zone.
- **OSC 52** (crossterm `clipboard.rs`; high): `ESC ] 52 ; c ; <base64> ESC \`, standard alphabet with `=` padding, no line breaks. `ratatui::crossterm::clipboard` does not exist in this build (`osc52 = ["dep:base64"]` not enabled) → hand-roll ~30-line encoder + writer, no new dependency. Support (medium): ghostty `clipboard-write` allows by default; Windows Terminal, wezterm, alacritty, kitty write; tmux needs `set-clipboard`; herdr reported to forward. No documented size cap (≈100 KB common, unverified) — ids are tiny.
- **Shift+drag bypass** (high for ghostty `mouse-shift-capture=false`, Windows Terminal; unverified for others): Shift+drag gives native selection. Inside herdr, drags go to herdr's own copy-on-select (`ui.mouse_capture`, `ui.copy_on_select` default true). Impact: document both; a selection mode that releases capture hands drags back to herdr/host selection.
- **ratatui-widgets 0.3.2 `Table`** (`table.rs`; high): header row, `Constraint` widths, row/col/cell highlight, but column-width computation (`get_column_widths` :1041) is private — no cell rects. Impact: keep the hand-rolled rows; compute column rects in our own helper and record header rects in `Regions`.
- **ANSI colours** (ratatui-crossterm `lib.rs`; high): `Color::Red` and `Color::Indexed(1)` both emit `38;5;1`; light colours → 8–15 — the terminal's 16-slot palette.

### Design language and theme architecture (research-deep; vet: 3 sampled, 0 dropped, 1 downgraded)
- **F1 footer overflow** (high; measured with `glimpse --once`): the hint line is 136 cells (tasks/layers), 153 (diagram), 183 (backlog); at 110 cols it ends `… s flows · d`, and in compact `? legend` is dropped. bubbles `help.go` drops whole items that would overflow and adds `…`. Impact: priority-ordered width-fitted hints always ending `? keys`; pairs with B-59aa6362.
- **F2 bold is invisible** (high): Windows Terminal `intenseTextStyle` defaults to "bright"; the user's alacritty sets bold to Regular + bright colours. 23 bold uses (plus the `let bold` binding) in `theme.rs` (badge, slug, selection, markdown strong) carry no visible emphasis on RGB colours. Impact: never carry emphasis by weight alone; add a `strong` token with a colour.
- **F3 config surface** (high): `[theme]` is a table (`config.rs:362` `theme_table` errors "`theme` must be a table"), so `theme = "name"` cannot coexist. Use `[theme] base = "<set>"` plus per-set override subtables `[theme.<set>]`.
- **F4 ANSI set needs style rules** (high): in the user's Kanso terminal palette normal white == bright black, so the greys collapse and no slot works as a selection background (1.45:1). Textual `_generate_ansi` makes surfaces transparent; helix `base16_terminal` uses `reversed` for selection. Impact: a set = palette + style rules (fills: colour/reverse; faint: colour/DIM); NO_COLOR becomes the `mono` set.
- **F5 active tab** (medium): flood the active tab `on_color` on `accent_bg` (6.32:1) like the agent chip; draw ledger-less tabs `faint` and drop the `—`.
- **F6 overlays** (medium): restyle the frame with a `backdrop` style before drawing the legend/selector/form — the only focus treatment glimpse needs.
- **F7 selected-row contrast** (medium): `muted` on the highlight band is 3.78:1 (< 4.5); lift muted/faint spans one tier on the cursor row (`selection_text`).
- Further ranked rules: header row styled muted + underline (not bold); one panel-drawing helper (6 hand-built `Block` sites; ego card is the only rounded one — cc60a17); empty states with one next step (`items.rs:79`, `inbox.rs:52`); move the history header row out of the header.
- **Theme sets**: keep the 15-token palette vocabulary; add `strong`, `backdrop`, `selection_text`, and a per-set `bg` used only by tests. Sets: `kanso-zen` (default), `kanso-pearl` (light, for light terminals), `ansi`, `mono`. Precedence: `--theme` flag → runtime choice in `state.toml` → config `[theme] base` → `kanso-zen`; NO_COLOR forces `mono`. Overrides: `[theme.<set>]` → `[theme]` → set palette → element defaults. Switching: an unbound key cycles sets with a notice (`Theme::build` is pure and cheap); a contrast unit test per set (text ≥4.5:1, chrome ≥3:1 against the set's `bg`).
- Downgraded/excluded: auto light/dark via OSC 11 (`terminal-colorsaurus`) — untested under herdr.
