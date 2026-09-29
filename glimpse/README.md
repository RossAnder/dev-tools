# glimpse

A live terminal view of a flow's task graph, checkpoints and running agents,
meant to sit in a herdr pane beside the Claude Code session running `/implement`.
It is read-only: everything it draws comes from `tomlctl`, plus the transcript
tail of the agent you are looking at.

## Install

```
cargo install --path tomlctl
cargo install --path glimpse
glimpse setup --dry-run
glimpse setup
```

glimpse needs a `tomlctl` new enough to have `tasks snapshot` and `agents record`
(≥ 0.12.0); it checks `tomlctl capabilities` at start-up and says so when the
installed binary is older.

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
saved `tomlctl tasks snapshot` document and never runs tomlctl.

## Views and keys

Three views, cycled with `Tab` and `Shift+Tab`:

- **layers** (the default): tasks grouped by their Kahn layer, with a checkpoint
  row after the layer that completes each checkpoint. Tasks unrelated to the
  selection are dimmed.
- **ego**, the traversal view: the selected task in a centre card, what it waits on
  to one side and what waits on it to the other. Coupling edges are dashed. It is
  drawn from the top of the pane at its natural size; spare rows go to the task's
  action and files in the card, then to a `2 hops` list of what lies one step
  further out on each side.
- **diagram**: the layered graph drawn as `[id]` nodes joined by routed edges,
  coupling edges dashed.

Each view lays its layers down the screen (vertical) or across it (horizontal).
`auto` picks horizontal when the pane is at least `orientation_threshold` times as
wide as it is tall, in cells.

**Density** decides how the body is shared. *Comfortable* docks the details and
activity panels beside the view (or below it, in a pane less than twice as wide as
it is tall). *Compact* gives the view the whole body and opens the panels as a
centred modal over it, with a one-row header. `auto`, the default, is compact while
the terminal is narrower than `compact_below` columns; `d` pins it either way and
shows the choice in the footer for a moment.

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
| `f` | toggle follow |
| `Enter` | details panel, then full-screen details, then close; compact opens and closes a modal |
| `J` / `K` | scroll the details one row down / up |
| `PgDn` / `PgUp`, `End` / `Home` | scroll the details a page, or to the bottom / top |
| `t` | activity panel for the selected task's newest running agent |
| `d` | cycle density: auto, compact, comfortable |
| `[` / `]` | shrink / grow the docked panel by 5% of the body, between 20% and 70% |
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
with `Shift` held, or set `mouse = false`. Runtime tweaks last until glimpse exits.

**Follow** keeps the selection on the frontier: the first in-progress task by
(layer, id), else the first ready one. While follow is off the header counts the
changes that arrived since. A task whose status changes is highlighted briefly
either way.

A `running` agent whose transcript has not been written for `stale_after_s` is
shown as stale.

## Configuration

`$GLIMPSE_CONFIG`, else `<home>/.config/glimpse/config.toml`, where home is
`USERPROFILE`, then `HOME`. A missing file means the defaults. Every key is
optional:

| Key | Default | Meaning |
|---|---|---|
| `orientation` | `"auto"` | `auto`, `vertical` or `horizontal` |
| `orientation_threshold` | `2.0` | width ÷ height at which `auto` turns horizontal |
| `split_threshold` | `2.2` | width ÷ height at which a new pane splits right rather than down |
| `pane_ratio` | `0.4` | share of the origin pane a new glimpse pane takes, between 0 and 1 |
| `poll_ms` | `500` | how often the flow's files are checked for changes |
| `tomlctl` | `"tomlctl"` | the tomlctl binary to run |
| `default_view` | `"layers"` | `layers`, `ego` or `diagram` |
| `stale_after_s` | `300` | seconds of transcript silence before a running agent shows stale |
| `density` | `"auto"` | `auto`, `compact` or `comfortable` |
| `compact_below` | `90` | terminal width in columns below which `auto` density is compact |
| `panel_percent` | `40` | the docked panel's share of the body, 20 to 70 |
| `column_max` | `40` | widest a horizontal layers column grows to fit its titles, 18 to 120 |
| `mouse` | `true` | capture the mouse for wheel scrolling and click-to-select |

Validation is strict: an unknown key, a wrong type or an out-of-range value makes
the whole file fall back to the defaults, with a warning in the header naming the
problem. The `--view` and `--orientation` flags override the file.

## How it works

```
Claude Code / Codex hook ─► glimpse hook ─► tomlctl agents record ─► .claude/flows/<slug>/agents.toml
                        │
                        └─ on a recorded start inside herdr ─► glimpse pane::ensure

tasks.toml + execution-record.toml + agents.toml + context.toml
        ─► tomlctl tasks snapshot ─► glimpse
```

- **Hook.** `glimpse hook` forwards the payload to `tomlctl agents record --harness <H>`, which
  works out the flow from the agent's dispatch prompt and writes the agent's row.
  On a recorded `SubagentStart` with `HERDR_PANE_ID` set, it opens the glimpse pane
  next to the Claude pane if that tab has none. It never prints; failures go to
  `<claude dir>/glimpse/hook.log`, which is emptied once it passes 1 MiB.
- **Pane.** One pane per herdr tab, found by its `glimpse` label. A new one splits
  the origin pane without taking focus and has the launch line typed into its
  shell. `ensure-pane` (the keybinding) does the same from the active pane, and
  with `--focus` moves focus to an existing pane when it sits next to the origin;
  herdr has no focus-by-id, so a pane elsewhere in the tab is reused unfocused.
- **Live view.** A poller checks the `(mtime, length)` of the flow's four files
  every `poll_ms` and runs `tomlctl tasks snapshot` only when one moves. A snapshot
  with an unchanged revision is dropped, a status-only change repaints over the
  cached diagram layout, and with nothing animating the event loop blocks, so an
  idle glimpse does no work.

`agents.toml` is local telemetry and gitignored; the execution record remains the
durable history.
