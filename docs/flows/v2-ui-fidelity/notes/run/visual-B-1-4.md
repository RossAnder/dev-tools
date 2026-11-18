Verdict: 4 states judged, 2 fail, 0 unsure, 2 pass

# Visual pass: v2-ui-fidelity, milestone B, round 1, inspector 4

States: web-09, tui-09, tui-01, tui-03. Rubric: `references/defects.md` of the `ui-capture` skill. Design: `docs/flows/v2-ui-fidelity/design/ui-B.md`.

## web-09: fail

Capture: `web-09.png` (1280x560), with a `web-09` key in `geometry.json`.

**Geometry.** `web-09: []`, no entries.

**Rubric.**

1. Scaling: **yes**. Swatch strip in every series row, x about 107 to 491. Each of the five swatches stretches to fill the row width left after the label, about 66 to 75 px wide and 18 px tall, where the design has fixed 18x18 squares. The sibling `web-08` has the same eight series at the same 1280 px width and draws 18x18 squares from about x 389 to 491.
2. Reserved space: no. The panel ends at the notices band, and the band holds a notice. The page to the right of x 504 is outside the panel, as the design says.
3. Panel size: no. The panel is 480 px wide (x 24 to 504) and its height ends at the band (y about 397).
4. Clipping: no. Geometry is empty and every label shows in full.
5. Overlap: no. The labels and the stretched swatches do not cover each other.
6. Misalignment: **yes**. Swatch columns, rows y 88 to 344. The swatches' left edges wander from row to row: the first swatch starts at about x 112 in `Primary`, x 121 in `Alternate`, x 145 in `Accent warm` and x 107 in `Critical`, so the inner swatch edges form no columns. In `web-08` they share one set of columns.
7. Off-style: no. The fold glyph, the outlined `Add` button, the border and the band match the sibling states.
8. Truncation: no. No label is cut.
9. Grid alignment: no. Not a text grid, and the image has no monospaced table.
10. Colour only: no. The notice is text, and the swatches' colour is their content.

**Look entries.** None. Both look entries this pass names list only `tui-*` states.

**Chosen design (Web panel, `stacked-rows`).** Different. The design keeps each swatch a fixed 18x18 px square, right-aligned, with the label as the only element that grows. In this capture the swatches grow to fill the row instead. The rest matches: the header with count and `Add`, eight rows, the notices band and the 480 px panel cap in a wide window.

## tui-09: fail

Capture: `tui-09.txt`, 80 columns by 24 rows, LF line endings, no wide characters. As a text-only state it needs no geometry key.

**Geometry.** No key. None is needed for a text-only state.

**Rubric.**

1. Scaling: no. Rows 3 to 12: the columns keep their fixed cells.
2. Reserved space: **yes**. Rows 3 to 7, columns 1 to 78: five blank rows sit between the separator (row 2) and the first task (row 8). They hold nothing and separate nothing, and the task rows are pushed down from row 3, where they belong.
3. Panel size: no. The box fills the 80x24 terminal, as designed.
4. Clipping: no. Rows 8 to 12: every title stops well before column 72.
5. Overlap: no.
6. Misalignment: no. ID in columns 2 to 3, glyph in column 6, title from column 10 and FILES ending at column 77, on rows 8 to 12.
7. Off-style: no. Rounded corners `╭╮╰╯`, and only the designed glyphs `✓` and `○`.
8. Truncation: no. No title is cut.
9. Grid alignment: no. `│` sits at columns 0 and 79 on rows 1 to 22, the corners meet their edges, and every row is 80 cells.
10. Colour only: no. The text grid carries no colour, and the status glyphs differ in shape.

**Look entries** (judged as the `tui` surface's control):

- Task 33, accept 0 ("the clean terminal states show none of the rubric's defects"): **not met**. The reserved-space defect above, rows 3 to 7.
- Task 33, accept 1 (columns and right border hold their cells, a long title ends in an ellipsis, the hint line sits inside the foot): met. Rows 1 to 22 hold their columns and the right border at column 79, and the hint line is on row 22 inside the box. No title in this state is long enough to need an ellipsis, so that clause is not tested here.

**Chosen design (Terminal panel, `single-table`).** Different. The design starts task rows at row 3 and holds no other row open. This capture starts them at row 8. The rest matches the design grid. One text difference is content, not arrangement: `fixtures capture command` has no apostrophe.

## tui-01: pass

Capture: `tui-01.txt`, 80x24, LF, no wide characters. Text only, no geometry key needed.

**Geometry.** None.

**Rubric.**

1. Scaling: no. Rows 0 to 23: nothing stretches.
2. Reserved space: no. Rows 4 to 21 are blank, but the design says the rows under the last task belong to the table and are left blank. They are the table region, not a held-open band.
3. Panel size: no. A full-terminal box, as designed.
4. Clipping: no. Row 3: the message runs from column 10 to 36.
5. Overlap: no.
6. Misalignment: no. The header row 1 has `ID` at 2, `ST` at 6, `TITLE` at 10 and `FILES` ending at 77. The hint text starts at column 2 and `0 tasks` ends at 77.
7. Off-style: no. Rounded border only.
8. Truncation: no.
9. Grid alignment: no. Borders sit at columns 0 and 79 on rows 1 to 22, and the corners are complete on rows 0 and 23.
10. Colour only: no.

**Look entries.**

- Task 33, accept 0: met. Every rubric class above is `no`.
- Task 33, accept 1: met. The columns hold, the right border is at column 79 on every row, and the hint line is row 22 inside the foot. There is no title row, so the ellipsis clause is not tested here.

**Chosen design (Terminal panel, empty extreme).** Matches. Header and separator, then `No tasks in this milestone.` from column 10 on row 3, and the hint line reads `0 tasks`.

## tui-03: pass

Capture: `tui-03.txt`, 80x24, LF, no wide characters. Text only.

**Geometry.** None.

**Rubric.**

1. Scaling: no.
2. Reserved space: no. Row 3 holds the task, and rows 4 to 21 are the blank table region the design allows.
3. Panel size: no. A full-terminal box.
4. Clipping: no. Row 3: the title stops at column 35.
5. Overlap: no.
6. Misalignment: no. Row 3 has `31` in columns 2 to 3, `✓` in 6, the title from 10 and `1` at 77. These line up with the header.
7. Off-style: no.
8. Truncation: no.
9. Grid alignment: no. Borders are at 0 and 79 on every row, and the corners are complete.
10. Colour only: no.

**Look entries.**

- Task 33, accept 0: met.
- Task 33, accept 1: met. The columns and the right border hold, and the hint line is on row 22, with `1 task` ending at column 77. The ellipsis clause is not tested, since the title is short.

**Chosen design (Terminal panel, one-task extreme).** Matches. Header, separator and one row. The hint reads `1 task`, singular, where the design writes `<n> tasks`. That is wording, not arrangement, so it is not counted as a difference.

## Unsure answers

None.

## Coverage

Read:
- the rubric, `references/defects.md` in the `ui-capture` skill
- `.ui-capture/B-1/geometry.json`
- `docs/flows/v2-ui-fidelity/design/ui-B.md`
- `design/ui-B/single-table.txt`, `stacked-rows.png` and `stacked-rows-narrow.png`
- `.ui-capture/B-1/web-09.png`, `web-08.png` (a sibling, for comparison), `tui-01.txt`, `tui-03.txt` and `tui-09.txt`

The PNG sizes were read from the file headers. No pixel library was available, so the swatch positions in web-09 are estimates read from the image at full size. Nothing failed to read.
