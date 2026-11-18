Verdict: 4 states judged, 1 fail, 0 unsure, 3 pass

# Visual pass, milestone B, round 1, inspector 5

Flow `v2-ui-fidelity`. Capture dir `.ui-capture/B-1/`. Design `docs/flows/v2-ui-fidelity/design/ui-B.md`.

## web-11: fail

Capture: `web-11.png` present (640x480), `geometry.json` key `web-11` present.

**Geometry.** `web-11: []`, no entries.

**Rubric.**

1. Scaling: no. The `Add` button (about x 445-491, y 35-59) is its label's width, swatches are fixed squares at x 389-491; same as the 640 px mock.
2. Reserved space: no. The panel ends at its last series row (y about 216); no gap where the notices band would be.
3. Panel size: no. Panel is 480 px wide (x 24-504) and as tall as header plus four rows.
4. Clipping: no. No geometry entry; labels and swatches sit fully inside their rows.
5. Overlap: no. Rows y 70-216, no element over another.
6. Misalignment: no. Glyph boxes share a left edge with `Series` at x 37; labels share a left edge at x 61; swatch columns line up across all four rows (enlarged crop of x 380-510).
7. Off-style: yes. Fold control column, x 37-53, y 80-206: each row's fold control is a boxed `−` / `+` tree-toggle glyph drawn in black with a square outline, a glyph style the panel does not otherwise use; the design and the sibling captures (web-02, mock `stacked-rows.png`) draw a small muted `▸` fold glyph with no box.
8. Truncation: no. All four labels short and whole.
9. Grid alignment: no (not a text grid; no monospaced table).
10. Colour-only: no. Swatches are colour content by purpose; expanded/collapsed reads through the `−` / `+` shape, not colour.

**Look entries.** The prompt's look entries (task 33 accept 0 and 1) list only `tui-*` states, so web-11 carries none here; no look judgement made.

**Design comparison** (Web panel, `stacked-rows`). Regions match the pick: header with `Series 4` and a right-hand outlined `Add`, then four rows of glyph, label, five right-aligned swatches, panel 480 px at a 24 px margin, no notices band and no gap for one. One stated constraint is not followed: the design names a `▸` fold glyph in muted `#6f6a63` at 12 px, and the capture draws a black boxed `−`/`+` toggle, which also pushes the label start from about x 57 (mock) to x 61. Recorded as a design defect.

## tui-04: pass

Capture: `tui-04.txt` present, 24 rows x 80 columns, text-only (no geometry key needed).

**Geometry.** None (text state).

**Rubric.**

1. Scaling: no. Rows 1-22: no element stretches; hint line is fixed text.
2. Reserved space: no. Rows 3-21 all hold content (18 task rows and the `… 7 more` row).
3. Panel size: no. Box fills the 80x24 terminal by design.
4. Clipping: no. Rows 3-20, cols 10-72: every title ends before col 73; nothing runs into FILES.
5. Overlap: no. Rows 0-23 checked.
6. Misalignment: no. IDs right-aligned in cols 2-3 on rows 3-20; status glyphs in col 6 on every task row; FILES digits in col 77 on every task row.
7. Off-style: no. Rounded corners `╭╮╰╯` on rows 0 and 23, `─` separator row 2 cols 2-77; consistent.
8. Truncation: no. No title is cut; overflow is reported as `… 7 more` (row 21 col 10).
9. Grid alignment: no. Every row is 80 cells; `│` at cols 0 and 79 on rows 1-22, corners meet edges; no wide characters found by a per-row width scan.
10. Colour-only: no. Status is shape (`✓ ● ○`), not colour.

**Look entries.**

- Task 33 accept 0 (no rubric defects): met, rows 0-23.
- Task 33 accept 1 (columns and right border hold on every row, hint line in the box's foot): met. Columns checked per row (ID cols 2-3, glyph col 6, TITLE from col 10, FILES cols 73-77, border col 79); hint on row 22 with `25 tasks` ending col 77. No long title in this state, so the ellipsis clause is not exercised here.

**Design comparison** (Terminal panel, `single-table`, many tasks). Matches: title in the top border after `╭─`, header row 1, separator row 2, task rows 3-20, last table row 21 reads `… 7 more` in the TITLE column (18 + 7 = 25 tasks, matching the hint), hint line row 22.

## tui-06: pass

Capture: `tui-06.txt` present, 24 rows x 80 columns.

**Geometry.** None (text state).

**Rubric.**

1. Scaling: no.
2. Reserved space: no. Rows 8-21 are blank, but the design states the rows under the last task belong to the table and are left blank; the mock `single-table.txt` has the same rows 8-21 blank. Not a held-open slot for an absent element.
3. Panel size: no. Box fills the terminal by design.
4. Clipping: no. Row 5 title stops at col 72; col 73-77 hold `    3`.
5. Overlap: no.
6. Misalignment: no. IDs cols 2-3, glyphs col 6, FILES col 77 on rows 3-7.
7. Off-style: no.
8. Truncation: no. Row 5 long title ends `…` at col 72.
9. Grid alignment: no. All rows 80 cells, borders at cols 0 and 79 on rows 1-22, the `…` takes one cell and FILES stays in line.
10. Colour-only: no.

**Look entries.**

- Task 33 accept 0: met, rows 0-23.
- Task 33 accept 1: met. Row 5 long title ends in `…` at col 72 inside the TITLE column; every column and the right border hold on every row; hint on row 22, `5 tasks` ending col 77.

**Design comparison.** Matches the mock row for row (same arrangement; task content and status glyphs differ, which is content, not arrangement) and the long-title extreme.

## tui-08: pass

Capture: `tui-08.txt` present, 12 rows x 80 columns (short terminal).

**Geometry.** None (text state).

**Rubric.**

1. Scaling: no.
2. Reserved space: no. Rows 3-9 all hold content.
3. Panel size: no. Box fills the 80x12 terminal.
4. Clipping: no.
5. Overlap: no.
6. Misalignment: no. IDs cols 2-3, glyphs col 6, FILES col 77 on rows 3-8.
7. Off-style: no.
8. Truncation: no. Overflow shown as `… 14 more` (row 9 col 10).
9. Grid alignment: no. All 12 rows 80 cells, borders at cols 0 and 79 on rows 1-10, corners on rows 0 and 11.
10. Colour-only: no.

**Look entries.**

- Task 33 accept 0: met, rows 0-11.
- Task 33 accept 1: met; columns and border hold on rows 1-10, hint on row 10 with `20 tasks` ending col 77. No long title in this state.

**Design comparison.** Matches the short-terminal extreme: six tasks on rows 3-8, `… 14 more` on row 9 (6 + 14 = 20), hint line on row 10.

Coverage: read the rubric `~/.claude/skills/ui-capture/references/defects.md`, `.ui-capture/B-1/geometry.json`, `docs/flows/v2-ui-fidelity/design/ui-B.md`, `design/ui-B/single-table.txt`, `design/ui-B/stacked-rows.png`, `design/ui-B/geometry.json`, the captures `web-11.png`, `tui-04.txt`, `tui-06.txt`, `tui-08.txt`, and siblings `web-01.png`, `web-02.png` for comparison; enlarged crops of web-11 made in the session scratchpad. `stacked-rows-narrow.png` not read (no narrow state judged). Nothing failed to read.
