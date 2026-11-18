# Milestone B: fixture layouts

The two fixture panels milestone B builds to prove the visual pass on. Each section is the arrangement the user picked, and the inspector judges each capture against it. The pictures sit in `ui-B/`, beside the HTML mocks they were rendered from and their `geometry.json`, which is empty for every mock.

## Web panel

**Picked:** `stacked-rows` (decision D35), drawn in `ui-B/stacked-rows.png` at 640x480 and `ui-B/stacked-rows-narrow.png` at 500x480, from `ui-B/stacked-rows.html`. Chosen over `label-over-swatches` (`ui-B/label-over-swatches.png`) and `split-list-preview` (`ui-B/split-list-preview.png`).

A panel listing colour series, in the eggshell style: black text on white, colour only in the swatches, one subtle grey band for notices.

**Sizes.**

- The page has a 24 px margin. The panel's width is the window's less 48 px, up to 480 px; its height is the height of its rows, never the window's.
- The surface must work in windows from 500 to 1280 px wide and 480 to 560 px tall.
- The panel has a 1 px `#d9d5cf` border, a 5 px radius and a white fill.
- The header row: `Series` at 14 px weight 600, the series count in muted `#6f6a63`, and an outlined `Add` button at the right whose width is its label's at every window size.
- A series row: a `▸` fold glyph in muted `#6f6a63` at 12 px, then the label, then five swatches. Each swatch is a fixed 18 x 18 px square with a 3 px radius and 3 px gaps, right-aligned. Rows are padded 8 px by 12 px and divided by 1 px `#d9d5cf` lines.

**What scales and what stays fixed.** The label takes the width the row has left and is the only thing that grows or shrinks with the window. The swatches, the fold glyph and the `Add` button keep their size at every width. In a window wider than 528 px the panel stops at 480 px and the rest of the window is empty page, outside the panel.

**Dead space.** The panel ends at its last row. The notices band (`#e7e4e0` fill, 13 px text, a 1 px line above) is drawn only when there is a notice, and with none the panel ends at its last series row with no gap where the band would be.

**Content extremes.**

- Empty: the header, then one muted line, `No series yet.`, and the panel ends.
- One series: the header and one row.
- Many series: eight rows fit a 560 px window. A list longer than the window scrolls inside the panel.
- A long label: cut to one line with an ellipsis before the swatches; the swatches stay where they are.
- A narrow window (500 px): the panel is 452 px wide and labels are cut sooner; nothing wraps.

## Terminal panel

**Picked:** `single-table` (decision D36), drawn in `ui-B/single-table.txt` at 80x24. Chosen over `list-detail` (`ui-B/list-detail.txt`) and `list-over-log` (`ui-B/list-over-log.txt`).

A task list drawn as a text grid, with rounded borders as glimpse draws them.

**Sizes.** The panel fills the terminal: 80x24, or 80x12 for a short terminal. Cells are counted from column 0 at the left border.

- One rounded box (`╭ ╮ ╰ ╯`, `─` and `│`) over the whole terminal, its title ` Tasks · milestone B ` set into the top border after `╭─`.
- Row 1 is the header and row 2 a `─` separator one cell in from each border. Task rows start at row 3.
- The columns hold fixed cells on every row: `ID` right-aligned in columns 2 to 3, the status glyph in column 6, `TITLE` left-aligned in columns 10 to 72, and `FILES` right-aligned in columns 73 to 77. Column 79 is the right border.
- The status glyphs are `✓` done, `●` running and `○` waiting, each one cell wide.
- The last row inside the box is the hint line: `j/k move  enter open  q quit` from column 2, and `<n> tasks` right-aligned to end at column 77.

**What fills.** The table region fills the box between the separator and the hint line. The rows under the last task belong to the table and are left blank. No other row is held open.

**Content extremes.**

- Empty: the header and the separator, then `No tasks in this milestone.` starting in column 10 on row 3, and the hint line reads `0 tasks`.
- One task: the header, the separator and one row.
- Many tasks: rows 3 to 21 hold tasks. When more tasks exist than rows, the last row reads `… <n> more` in the `TITLE` column.
- A long title: cut so that it ends with `…` in column 72; the `FILES` column stays in line.
- A short terminal (80x12): rows 3 to 9 hold six tasks and the `… <n> more` row, and the hint line is row 10.
