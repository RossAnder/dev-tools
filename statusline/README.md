# statusline

Native renderer for the Claude Code status line **and** the agent-panel
teammate rows — one Rust binary, selected by argv.

## Why

Claude Code runs `statusLine.command` on every refresh, in every session. The
PowerShell script this replaced cost ~1s of pwsh cold-start CPU per refresh
(wrapped in a Git Bash spawn on Windows); across several concurrent sessions
that is constant ambient CPU. This binary renders in single-digit milliseconds.

The same argument applies to `subagentStatusLine`, which fires once per refresh
tick for the whole agent panel.

## Usage

```
statusline [--style <STYLE>]            # main status line   (statusLine)
statusline subagent [--style <STYLE>]   # agent-panel rows   (subagentStatusLine)
```

Reads the matching JSON payload on stdin, writes to stdout with no trailing
newline. `subagents` and `agents` are accepted as aliases for the `subagent`
mode word, and `minimal` for the `min` style. `--list-styles` names the styles
and those aliases; `--columns <N>` overrides the width budget (useful for
eyeballing tiers by hand). `--doctor` reads the payload, then writes what it
resolved — claude dir, subagents dir, parsed member count, width and where it
came from — to **stderr** and exits 0 without drawing a row; stdout is the row
protocol, so nothing diagnostic goes there. `STATUSLINE_DUMP=<path>` writes the
raw stdin payload to that path before rendering, so a bad line replays offline.

## Styles

### `--style full` (main, default)

The original two-line colour-coded layout, a faithful port of
`~/.claude/statusline.ps1`. Unchanged, and still what an unflagged invocation
renders, so an old `settings.json` keeps working.

```
dev-tools@main (+98 -483) | Opus 5 | xhigh | 31k/200k (16%)
5h ●●○○○ 26% @5:20pm | 7d ●○○○○ 7% @22/8, 6pm | busy 12m34s
```

Width tiers come from `COLUMNS`, falling back to 120: changes need ≥100 cols,
model/effort/resets ≥70, line 2 ≥50, below 50 a single compact line.

### `--style min` (main)

One monotone line modelled on the [`claude-usage`
statusline](https://statuslin.es/c/claude-usage-cc6759bb), with the repo name in
place of its "Usage" title and the live session context in place of its
per-model weekly bucket.

```
✻ dev-tools · 5h 26% · week 7% · chat 31k · Opus 5 xhigh
```

The model and the effort level close the line as **one** segment, not two: the
effort word is live (it follows a mid-session `/effort`) and the `effort` object
is absent altogether on a model without the parameter, in which case the name
draws alone. `full` gives effort a separator of its own because it can colour
the word by level; `min` has no colour, and a bare `xhigh` between two dots
would read as a fourth usage bucket.

Monotone in the strict sense: the line emits **no colour at all**, so every
character lands in the terminal theme's own foreground rather than a mix of
greys. Weight is uniform too — a single bold span wraps the whole line, so no
segment shouts over its neighbours. Those two escapes (SGR 1 to open, SGR 22 to
close) are the only ones in the output.

As the terminal narrows it sheds the slowest-moving segment first, so `chat` is
last out and the repo name never goes. Model and effort lead — that is the one
segment you set rather than watch, and the one the payload can make arbitrarily
wide — so every usage number keeps the width tier it had before they existed:

```
56 cols  ✻ dev-tools · 5h 26% · week 7% · chat 31k · Opus 5 xhigh
55 cols  ✻ dev-tools · 5h 26% · week 7% · chat 31k
40 cols  ✻ dev-tools · 5h 26% · chat 31k
30 cols  ✻ dev-tools · chat 31k
```

The repo name is the `origin` remote's repo name, falling back to the launch
directory's leaf.

The glyph is **U+273B ✻**, not the reference page's U+2733 ✳. `emoji-data.txt`
lists `2733..2734 ; Emoji`, so a terminal renders U+2733 from the colour-emoji
font instead of your monospace face — off-weight, off-baseline, often
double-width. U+273B carries no emoji property, is the mark Claude Code itself
uses, and sits at exactly one cell in Iosevka. Change it with `--icon <glyph>`;
`--icon ""` drops it. The flag belongs to this style alone — outside `--style
min` it is an error, exiting 2 with `--icon applies to --style min only`.
`scripts/preview-icons.ps1` browses candidates in the terminal that has to
render them (see below).

### `subagent --style tiers` (default)

Replaces the default `name · description · elapsed · ↓ N tokens` agent row,
which is laid out for a wide terminal and left to Ink's end-of-line truncation
below that. Here the row is budgeted against the `columns` Claude Code supplies
and degrades in named tiers:

```
76 cols  task-07-remove-runtime-deps ·  implement-deep  · @2 · 86k · stalled · 3m33s
48 cols  task-07-remove-runtime-deps · @2 · 86k · stalled
30 cols  task-07-remove-run… · @2 · 86k
```

The space-padded `implement-deep` cell is a colour badge: the teammate's own
assigned colour as the background with the foreground inverted to black, drawn
from ANSI indices so it takes the terminal's scheme rather than a palette baked
into the binary. The padding is the chip; there are no brackets in the output.

Every field carries a shed priority — lowest goes first:

| priority | field | source |
|---|---|---|
| never shed | task name | payload `name` |
| protected | token count, `@N` inbox depth | payload / teammate inbox file |
| 50 | `stalled` | payload `tokenSamples` |
| 40 | agent-type badge | `subagents/*.meta.json` |
| 30 | runtime | payload `startTime` |
| 25 | context fill % (`rich` only) | payload `tokenCount` / `contextWindowSize` |
| 20 | model + effort (`rich` only) | payload |
| 10 | activity | payload `label` |

The name is never dropped, only clipped, and only once nothing else can give —
the token count outlasts a fully-spelled title.

`name` is the task title and is never sacrificed — it is the one field that
reliably identifies a row. The activity rides alongside it when there is room
and is dropped whole rather than clipped to noise. The token count and the `@N`
inbox depth survive every tier; a row that is queued, paused, completed, failed
or killed says so in place of a runtime — `queued`, `paused`, `done`, `failed`,
`killed`.

**Why the activity is filtered.** Claude Code fills the payload's `label` with
`progressSummary || description`, so it degrades silently to the first line of
the orchestrator's prompt whenever no progress summary exists yet — markdown
headings and all. That is what produces default rows like:

```
t10-css-face-split  ## Shared context — theme-package font architectur…   2m 13s · ↓ 126.5k tokens
```

So a `label` equal to `description` is treated as *no activity*, as is the
filler string `working`. The row becomes:

```
t10-css-face-split · 126k · 2m13s
```

A genuine progress summary still shows, because it differs from the description.
When a row has no name at all, the description is used as a last resort with its
markdown lead-in stripped.

Beyond the badge, rows emit no colour. Claude Code already dims unselected rows
and brightens the viewed one; painting the rest would fight that and leave the
panel a mix of greys.

**Three fields come from disk**, because the hook payload does not carry them:

- **agent type** and **teammate colour** — `<transcript dir>/<session
  id>/subagents/*.meta.json`, ~330 bytes each. `customAgentType` is what tells a
  `verification` row from an `implement-deep` one.
- **inbox depth** — `<claude dir>/teams/<teamName>/inboxes/<name>.json`, a JSON
  array of pending inbound messages; length is the queue.

`teams/<teamName>/config.json` carries the same agent types but embeds every
member's full dispatch prompt (~110 KB for a team of eleven), so it is
deliberately *not* read on a timer. Everything degrades to absent rather than
erroring: a row that cannot find its metadata just renders without it.

**`stalled`** means the whole `tokenSamples` history (9+ readings) shows no
growth. The samples carry no timestamps, so it is "N consecutive readings with
no growth", not a duration — it catches a wedged agent, and also one legitimately
blocked in a long tool call.

### `subagent --style rich`

As `tiers`, plus the resolved model and effort when the width allows — the one
thing the default row can never tell you — and the context-window fill as a
percentage, alongside the token count it is derived from:

```
research-deep · reading claude binary strings · opus-5 high · 31k · 15% · 4m12s
```

The model/effort segment sits one step above the activity in the shed order, so
the activity goes first and model/effort second; the percentage sheds after it
(a live reading beats static config) but before the runtime, since it only
restates a count that is never shed at all.

## Setup

```
cargo test --manifest-path statusline/Cargo.toml
cargo install --path statusline
```

Then in `~/.claude/settings.json` (forward slashes — under Git Bash on Windows,
backslashes in a `command` path are consumed as escapes):

```json
"statusLine": {
  "type": "command",
  "command": "C:/Users/rossa/.cargo/bin/statusline.exe --style min",
  "refreshInterval": 30
},
"subagentStatusLine": {
  "type": "command",
  "command": "C:/Users/rossa/.cargo/bin/statusline.exe subagent"
}
```

## Notes

- Branch resolution reads `.git/HEAD` directly (walking up parents, following
  `gitdir:` pointers). Both that read and the crate's one subprocess —
  `git diff HEAD --numstat`, run only for `full` at ≥100 columns — happen in
  `main`, which threads the results into the renderer as arguments; no renderer
  reads the filesystem or spawns anything. That spawn is a deliberate trade, not an
  oversight: the payload's `cost.total_lines_added` / `cost.total_lines_removed`
  would render the same `(+N -N)` for free, but they count only the lines Claude
  changed this session, while `git diff HEAD` counts the whole working tree
  against HEAD — hand edits included. The subprocess is kept because it measures
  the thing the segment claims to show.
- Durations use integer truncation, fixing the ps1's `[long]`-rounding bug (754s
  rendered as `13m34s` instead of `12m34s`).
- Empty or unparseable stdin degrades to the bare label `Claude` for the main
  line, and to *no output at all* for `subagent` — which Claude Code reads as
  "keep the default rendering for every row".
- `min` emits no colour at all, and `subagent` none beyond the agent-type badge.
  Beyond looking uniform, this keeps subagent rows out of a real trap: Claude
  Code wraps unselected rows in its own `\x1b[2m`, and an SGR 0 in the row body
  strips that for the remainder of the line. `min` ends its one bold span with SGR 22 for the same reason.
- **Choosing a glyph**: `scripts/preview-icons.ps1` dumps any codepoint range as
  a labelled grid, flags codepoints carrying the Unicode Emoji property (your
  terminal will render those from the colour-emoji font), and with `-Font`
  flags any the font lacks. `-InSitu` renders each candidate through the real
  binary — the cargo-installed `statusline.exe` unless `-Binary` names another,
  which is how you preview a build you have not installed (`-InSitu` throws
  when the path does not exist):

  ```powershell
  ./scripts/preview-icons.ps1 -From 0x25A0 -To 0x25FF `
    -Font "$env:LOCALAPPDATA\Microsoft\Windows\Fonts\IosevkaTermNerdFont-Regular.ttf"
  ./scripts/preview-icons.ps1 -InSitu -From 0x2726 -To 0x273D
  ./scripts/preview-icons.ps1 -InSitu -Binary ./target/release/statusline.exe
  ```
