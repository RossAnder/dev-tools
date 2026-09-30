# glimpse

A live terminal view of a flow's task graph, checkpoints and running agents,
meant to sit in a herdr pane beside the Claude Code session running `/implement`.
It is read-only: everything it draws comes from the flow's files, read through
tomlctl's own code compiled into glimpse, plus the transcript tail of the agent
you are looking at.

## Install

```
cargo install --path tomlctl
cargo install --path glimpse
glimpse setup --dry-run
glimpse setup
```

glimpse never runs the `tomlctl` binary: it builds snapshots, lists flows and
records hook events with a copy of tomlctl's code linked in at build time. That
copy changes only when glimpse is rebuilt, so rerun `cargo install --path glimpse`
whenever tomlctl changes, not just `cargo install --path tomlctl`.

`glimpse setup` makes up to three edits, each only if it is not already there, and prints
what it did. `--dry-run` prints the same report and writes nothing.

- **Claude Code hooks** in `<claude dir>/settings.json` (`$CLAUDE_CONFIG_DIR`,
  else `~/.claude`): one matcher-less group each for `SubagentStart`,
  `SubagentStop` and `TeammateIdle`, in exec form and async:

  ```json
  { "hooks": [{ "type": "command", "command": "<path to glimpse>", "args": ["hook"], "async": true }] }
  ```

  An event that already has a hook whose command names glimpse and mentions
  `hook` is left alone.
- **Codex hooks** in `<CODEX_HOME>/hooks.json` (`$CODEX_HOME`, else `~/.codex`),
  only when that directory exists; otherwise setup reports `codex not found,
  skipped`. `SubagentStart` and `SubagentStop` only, since Codex has no teammates.
  Codex has no exec form and runs every hook through the session shell, so the
  entry is the async command line `<path to glimpse> hook --harness codex`, and
  setup refuses a glimpse path that would need quoting. An event already carrying
  a glimpse hook, in `hooks.json` or under `[hooks]` in `config.toml`, is left alone.
- **A herdr keybinding**, `prefix+alt+g`, appended to herdr's `config.toml` as a
  `[[keys.command]]` popup that runs `glimpse ensure-pane --focus`. It is skipped
  if the file mentions glimpse anywhere, and refused if appending it would break a
  file that parsed before. herdr's config is found the way herdr finds it:
  `$HERDR_CONFIG_PATH`, then `$XDG_CONFIG_HOME/herdr`, then `%APPDATA%\herdr` on
  Windows, then `~/.config/herdr`. After writing it, setup runs
  `herdr server reload-config`.

Each changed file is first copied to `<file>.bak-glimpse-<unix-ts>` and then
replaced atomically. Claude Code normally picks up hook edits in running
sessions; restart it only if the hooks do not fire. Codex runs a new hook only
once it is trusted: accept glimpse's hooks in the review Codex shows at its next
start.

## Usage

```
glimpse [--slug S] [--view V] [--orientation O]   open the live view
glimpse --once [--size WxH] [--snapshot FILE] [--select ID]
                                                  render one frame as plain text
glimpse hook [--harness H]                        handle one hook payload on stdin
glimpse ensure-pane [--slug S] [--focus]          open or reuse this tab's glimpse pane
glimpse setup [--dry-run]                         install the hooks and keybinding
```

`glimpse --help` is the full reference. Without `--slug` glimpse opens the flow
whose `tasks.toml` changed most recently and keeps following whichever flow
changes next. Exit status is 0 on success, 1 on a runtime failure and 2 on a usage
error; `hook` always exits 0, logging even a usage error. `--harness` is
`claude-code` (the default) or `codex`.

`--once` renders through ratatui's test backend with no terminal and no escape
sequences, at 120x40 unless `--size` says otherwise. With `--snapshot` it draws a
saved `tasks snapshot` document instead of reading the flow's files.

## Views and keys

Three views, cycled with `Tab` and `Shift+Tab`:

- **layers** (the default): tasks grouped by their Kahn layer, with a checkpoint
  row after the layer that completes each checkpoint. With a selection, each row is
  marked by its relation to it (`↑` needs, `↓` dependent, `⇡`/`⇣` coupling, `≈`
  shares a file) and unrelated rows are faded, all but their status mark.
- **ego**, the traversal view: the selected task in a centre card, what it waits on
  to one side and what waits on it to the other. Coupling edges are dashed. It is
  drawn from the top of the pane across its full width. Spare rows go first to a
  few rows of the task's action in the card, then to a `2 hops` list of what lies
  one step further out on each side, then (at comfortable density) back to the
  card, which shows the action's markdown lists and then its files. Horizontally
  the card takes whatever width the side columns leave.
- **diagram**: the layered graph drawn as `[id]` nodes joined by routed edges,
  coupling edges dashed. A needs edge that another path of needs edges already
  implies is left out of the layout, which keeps wide graphs compact; `i` lays them
  out too, and details marks them `implied` under Needs. Tasks sharing a file with
  the selection are underlined.

Shapes are judged in pixels, not cells: a cell is about twice as tall as it is
wide, by a factor that depends on the font. glimpse asks the terminal for its pixel
size and, when it gets no answer (most Windows terminals give none), uses
`cell_aspect`. Set that to your font's cell height over its width — about 2.5 for
Iosevka, 2.1 for Cascadia Mono — if the automatic choices below look wrong.

Each view lays its layers down the screen (vertical) or across it (horizontal).
`auto` picks horizontal when the view's width over its height, in pixels, is at
least `orientation_threshold`.

**Density** decides how the body is shared. *Comfortable* docks the details and
activity panels beside a landscape body and below a portrait one; `panel_split`
pins either side and `|` flips it. The switch point
is `panel_split_threshold`, with a margin either side so a pane resized near it
does not flicker. Drag the panel's near border with the mouse to resize it, or use
`[` and `]`. *Compact* gives the view the whole body and
opens the panels as a centred modal over it, with a one-row header. `auto`, the
default, is compact while the terminal is narrower than `compact_below` columns;
`d` pins it either way and shows the choice in the footer for a moment.

Moving in the traversal view follows the arrows. Moving *across* (toward needs or
dependents) selects that side's first entry, and the card recentres on it. Moving
*along* then walks the rest of the side you just entered, and moving back across
returns to the task you came from, one crossing at a time. Before any crossing,
moving along walks the selected task's layer. The card's bottom border says which
one along walks: `layer 2 · 1/3`, or `needs of 7 · 2/2`. Selecting a task any other
way (a click, follow, or a move in another view) forgets the crossings.

In the horizontal layers view each column is as wide as the longest task cell,
within `column_max`, and only the layers that fit are drawn, scrolling to follow
the selection. `‹` and `›` on the top row mark layers off-screen to either side.

| Key | Action |
|---|---|
| `h` `j` `k` `l` / arrows | move the selection; this turns follow off |
| `Tab` / `Shift+Tab` | next / previous view |
| `o` | flip the orientation |
| `\|` | dock the panels beside or below the view |
| `i` | diagram: lay out implied edges too, or leave them out |
| `?` | legend: every mark, colour and key, in the current theme |
| `f` | toggle follow |
| `Enter` | details panel, then full-screen details, then close; compact opens and closes a modal |
| `J` / `K` | scroll the details one row down / up |
| `PgDn` / `PgUp`, `End` / `Home` | scroll the details a page, or to the bottom / top |
| `t` | activity panel for the selected task's newest running agent |
| `d` | cycle density: auto, compact, comfortable |
| `[` / `]` | shrink / grow the docked panel by 5% of the body, between 20% and 70%; dragging the divider does the same |
| `-` / `=` (or `+`) | narrow / widen the horizontal layers columns by 4 cells |
| `s` | flow selector; `j`/`k` move and `Enter` switches, which turns auto-flow off |
| `a` | toggle auto-flow (switch to the freshest flow as flows change) |
| `q` / `Esc` | close the topmost overlay, or quit when none is open |
| `Ctrl+C` | quit |

The details scroll goes back to the top whenever the selection changes. With the
mouse on, the wheel scrolls the details panel under the pointer and moves the
selection over the view, and a left click on a task in the layers or ego view
selects it; the diagram view takes the wheel but not clicks. Mouse capture stops the
terminal's own text selection while glimpse runs; most terminals still select
with `Shift` held, or set `mouse = false`.

The view, a flipped orientation or panel side, the panel's share and the implied
edge toggle are saved on exit to `<claude dir>/glimpse/state.toml` and restored at
the next start; `--view` and `--orientation` still win. Delete the file to go back
to the config's defaults. `--once` neither reads nor writes it.

**Follow** keeps the selection on the frontier: the first in-progress task by
(layer, id), else the first ready one. While follow is off the header counts the
changes that arrived since. A task whose status changes is highlighted briefly
either way, in the colour of the status it changed to.

A `running` agent whose transcript has not been written for `stale_after_s` is
shown as stale.

## Configuration

`$GLIMPSE_CONFIG`, else `<home>/.config/glimpse/config.toml`, where home is
`USERPROFILE`, then `HOME`. A missing file means the defaults. Every key is
optional:

| Key | Default | Meaning |
|---|---|---|
| `orientation` | `"auto"` | `auto`, `vertical` or `horizontal` |
| `orientation_threshold` | `0.9` | width ÷ height, in pixels, at which `auto` turns horizontal |
| `panel_split` | `"auto"` | `auto`, `beside` or `below`: where comfortable density docks the panels |
| `panel_split_threshold` | `1.0` | width ÷ height, in pixels, around which `auto` moves the panels beside |
| `cell_aspect` | `2.2` | a cell's height ÷ width, used when the terminal does not report pixels |
| `split_threshold` | `2.2` | width ÷ height at which a new pane splits right rather than down |
| `pane_ratio` | `0.4` | share of the origin pane a new glimpse pane takes, between 0 and 1 |
| `poll_ms` | `500` | the fallback polling interval, used only while no filesystem watch can be set up or after one has been found missing changes |
| `default_view` | `"layers"` | `layers`, `ego` or `diagram` |
| `stale_after_s` | `300` | seconds of transcript silence before a running agent shows stale |
| `density` | `"auto"` | `auto`, `compact` or `comfortable` |
| `compact_below` | `90` | terminal width in columns below which `auto` density is compact |
| `panel_percent` | `40` | the docked panel's share of the body, 20 to 70 |
| `column_max` | `40` | widest a horizontal layers column grows to fit its titles, 18 to 120 |
| `mouse` | `true` | capture the mouse for wheel scrolling, click-to-select and dragging the divider |
| `[theme]` | | colour tokens, below |

Validation is strict: an unknown key, a wrong type or an out-of-range value makes
the whole file fall back to the defaults, with a warning in the header naming the
problem. The `--view` and `--orientation` flags override the file.

### Theme

The `[theme]` table overrides colour tokens. A value is a colour — `#rrggbb`, an
ANSI name such as `red` or `light-blue`, an index `0`–`255`, or `default` for the
terminal's own — or the name of another token. Palette tokens hold colours;
element tokens name one part of the UI and default to a palette token, so you can
repaint a single feature or the whole palette:

```toml
[theme]
accent = "#E6A06B"      # everything that uses the accent
checkpoint = "info"     # just the checkpoint rows, borrowing another token
key = "#7FB4CA"         # the footer's key names
```

| Group | Tokens |
|---|---|
| Palette | `fg` `accent` `accent_bg` `success` `danger` `warning` `info` `violet` `teal` `idle` `muted` `faint` `subtle` `highlight` `on_color` |
| Status | `status_pending` `status_in_progress` `status_done` `status_failed` `status_deferred`, and `flash_fg` for text on a status-change flash |
| Selection | `selection_bg` `selection_mark` `unrelated` |
| Edges | `edge` `edge_faded` `edge_needs` `edge_out` `edge_coupling` `edge_overlap` |
| Chrome | `border` `border_title` `layer_rule` `layer_label` `section` `secondary` `slug` |
| Chips and rows | `checkpoint` `commit` `agent_bg` `agent_fg` `effort` `code` `warning_text` |
| Footer | `key` `key_label` `key_separator` `notice` |

The defaults are the Kanso Zen palette. `?` shows the legend in the live theme.
With `NO_COLOR` set, every colour is dropped and highlights fall back to reverse
video.

## How it works

```
Claude Code / Codex hook ─► glimpse hook (records in-process) ─► .claude/flows/<slug>/agents.toml
                        │
                        └─ on a recorded start inside herdr ─► glimpse pane::ensure

.claude/flows/** change ─► watcher ─► poller ─► in-process snapshot ─► glimpse
                                        ▲
                     10 s safety tick ──┘  (every poll_ms instead, if the watch fails)
```

- **Hook.** `glimpse hook` records the payload in-process with the same code as
  `tomlctl agents record --harness <H>`: it works out the flow from the agent's
  dispatch prompt and writes the agent's row.
  On a recorded `SubagentStart` with `HERDR_PANE_ID` set, it opens the glimpse pane
  next to the Claude pane if that tab has none. It never prints; failures go to
  `<claude dir>/glimpse/hook.log`, which is emptied once it passes 1 MiB.
- **Pane.** One pane per herdr tab, found by its `glimpse` label. A new one splits
  the origin pane without taking focus and has the launch line typed into its
  shell. `ensure-pane` (the keybinding) does the same from the active pane, and
  with `--focus` moves focus to an existing pane when it sits next to the origin;
  herdr has no focus-by-id, so a pane elsewhere in the tab is reused unfocused.
- **Live view.** One recursive filesystem watch on `.claude/flows` only wakes a
  poller thread; a burst of writes is gathered into one pass. The poller then
  compares the `(mtime, length)` of the viewed flow's four files and builds a
  snapshot in-process only when one moved, so in-place edits are seen as well as
  tomlctl's renames. A safety pass runs every 10 s regardless. If the watch cannot
  be set up, glimpse polls every `poll_ms` and keeps trying to watch; if two safety
  passes in a row find a change the watch never reported, it gives up on the
  watch and polls every `poll_ms` for the rest of the session. A snapshot with an
  unchanged revision is dropped, and a status-only change repaints over the cached
  diagram layout. The activity panel's transcript is re-read once a second on the
  poller thread while the panel is open. With nothing animating the event loop
  blocks, so an idle glimpse does no work beyond the safety pass.

`agents.toml` is local telemetry and gitignored; the execution record remains the
durable history.
