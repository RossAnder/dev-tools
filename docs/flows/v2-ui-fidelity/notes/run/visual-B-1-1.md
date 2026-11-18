Verdict: 4 states judged, 2 fail, 0 unsure, 2 pass

# Visual pass: v2-ui-fidelity milestone B, round 1, inspector 1

Capture dir `.ui-capture/B-1/`, judged against `docs/flows/v2-ui-fidelity/design/ui-B.md` and the rubric `references/defects.md` of the `ui-capture` skill. Pixel measurements were taken with a throwaway pure-Python PNG decoder kept in the session scratchpad; nothing in the repo or the capture dir was changed.

## web-02 — fail

Capture: `web-02.png` (640x480) present; `geometry.json` key `web-02` present.

**Geometry.** The key holds an empty array; no entries.

**Rubric.**

1. scaling: no. Add button bbox (445,35)-(490,58), swatches 18 px wide at x 389/410/431/452/473 with 3 px gaps, identical to the mock and to the 1280 px (`web-08`) and 500 px (`web-10`, shifted with the panel edge) siblings.
2. reserved-space: **yes**. Panel interior y 107-454: the single series row ends at its divider-free bottom near y 106, and the panel's border continues down to y 455 with about 348 px of white holding no content and separating nothing (no dark pixel anywhere in x 26-502, y 108-452).
3. panel-size: **yes**. Panel outline x 24-503, y 24-455: the panel is about 431 px tall around a header and one 37 px row; its sibling with the same content, `web-03`, ends at y 106. The panel tracks the window height rather than its rows.
4. clipping: no. Header, label `Primary` and swatches drawn whole, no geometry entry.
5. overlap: no.
6. misalignment: no. `Series` left edge x 37, label x 58, glyph x 40, all as in the mock.
7. off-style: no. Outlined Add button, rounded swatches and muted glyph match the mock.
8. truncation: no. `Primary` shown whole.
9. grid-alignment: no (image state; no monospaced table).
10. colour-only: no. Count `1` is muted #6f6a63 (darkest pixel (111,106,99)); its orange fringe at full size is subpixel anti-aliasing, the same as the mock's `4`.

**Look entries.**

- task 32 accept 0 (clean web states show none of the rubric's defects): **not met**. Classes 2 and 3 answer yes, region y 107-454.
- task 32 accept 1 (fixed swatches, glyphs and Add; label takes the slack; panel ends at its last row with no band below): **not met**. The swatches, glyph and Add keep their size, but the panel does not end at its last row; a ~348 px band sits below it inside the border.

**Design comparison (Web panel, `stacked-rows`).** Regions present and arranged as picked (header with title, count and right-hand Add; rows of glyph, label, right-aligned swatches; 24 px margin; 480 px panel width). Differs: the design states the panel's height is the height of its rows, never the window's, and that the panel ends at its last row; here it runs to y 455 of a 480 px window.

## tui-02 — fail

Capture: `tui-02.txt` (80x24 grid, 25th line empty) present. Text-only state, so no `geometry.json` key is needed and none exists.

**Geometry.** No key (text-grid state); none owed.

**Rubric.**

1. scaling: no. The box fills the terminal as the design says; hint and columns are fixed cells.
2. reserved-space: no. Rows 8-21 are blank table rows under the last task, which the design assigns to the table.
3. panel-size: no. Box 80x24 as designed.
4. clipping: no. Every row is 80 cells with borders at columns 0 and 79.
5. overlap: no. Nothing is drawn over a border cell.
6. misalignment: **yes**. Row 6 (task 34), columns 77-78: the right-aligned FILES value `4` sits in column 78 while rows 3, 4, 5 and 7 end their FILES value in column 77, so it is one cell right of the column and touches the border.
7. off-style: no. Rounded borders and `─` throughout, as glimpse draws them.
8. truncation: no. No title is long enough to cut; none ends mid-word.
9. grid-alignment: **yes**. Same location, row 6 columns 73-78: the FILES column is out of line by one cell on that row only, leaving no padding cell before the `│` at column 79.
10. colour-only: no. Status is carried by distinct glyphs (`✓`, `○`), not colour.

**Look entries.** None. The look entries given name only `web-*` states, and no `tui-*` look entries were supplied for this round, so there is nothing to judge here.

**Design comparison (Terminal panel, `single-table`).** Box, title, header on row 1, separator on row 2, tasks from row 3, blank table rows, hint line on row 22 with `5 tasks` ending at column 77: all as picked. Differs: the design fixes `FILES` right-aligned in columns 73 to 77 on every row; row 6 places it at column 78. Status glyph values and the title text (`fixtures capture`, no apostrophe) differ from the mock's sample data, which is content, not arrangement, and is not counted.

## web-01 — pass

Capture: `web-01.png` (640x480) present; `geometry.json` key `web-01` present.

**Geometry.** Empty array; no entries.

**Rubric.**

1. scaling: no. Add (445,35)-(490,58) as at every sibling width; panel width 480 px at 640 px window.
2. reserved-space: no. Panel y 24-106, ending one line below `No series yet.`; nothing below inside the border.
3. panel-size: no. Height is header plus one muted line, as the empty state should be.
4. clipping: no.
5. overlap: no.
6. misalignment: no. `No series yet.` starts at x 37, the same left edge as `Series`.
7. off-style: no.
8. truncation: no.
9. grid-alignment: no (image state).
10. colour-only: no. Count `0` muted #6f6a63; no state carried by colour.

**Look entries.**

- task 32 accept 0: **met**. All ten classes answer no (whole image, panel x 24-503 y 24-106).
- task 32 accept 1: **met**. Add keeps its size (matches `web-08` at 1280 px and `web-10` at 500 px); no swatches or glyphs in the empty state; the panel ends at its last line with no notices band or gap below.

**Design comparison.** Matches the design's empty extreme: header, then one muted `No series yet.` line, and the panel ends. No difference.

## web-03 — pass

Capture: `web-03.png` (640x480) present; `geometry.json` key `web-03` present.

**Geometry.** Empty array; no entries.

**Rubric.**

1. scaling: no. Swatches 18x18 (y 79-96) at x 389-490 with 3 px gaps; Add (445,35)-(490,58); same in the 1280 px and 500 px siblings, the label region alone absorbing width.
2. reserved-space: no. Panel ends at y 106, directly under the one row; no band.
3. panel-size: no. Panel height is header plus one row.
4. clipping: no.
5. overlap: no.
6. misalignment: no. Glyph x 40, label x 58, swatches vertically centred on the label row, matching the mock.
7. off-style: no.
8. truncation: no. `Primary` shown whole.
9. grid-alignment: no (image state).
10. colour-only: no. Count `1` is muted #6f6a63; the orange fringe is anti-aliasing, identical in the mock.

**Look entries.**

- task 32 accept 0: **met**. All ten classes answer no.
- task 32 accept 1: **met**. Swatches, glyph and Add at their fixed sizes; panel ends at its last row (y 106) with no notices band and no gap.

**Design comparison.** Matches the design's one-series extreme: the header and one row. No difference.

Coverage: read `claude/skills/ui-capture/references/defects.md` (via the config dir), `.ui-capture/B-1/geometry.json`, `docs/flows/v2-ui-fidelity/design/ui-B.md`, `design/ui-B/stacked-rows.png`, `design/ui-B/stacked-rows-narrow.png`, `design/ui-B/single-table.txt`, and the captures `web-01.png`, `web-02.png`, `web-03.png`, `tui-02.txt`; for sibling comparison measured `web-04.png`, `web-06.png`, `web-08.png`, `web-10.png` and diffed `tui-01.txt`. Nothing failed to read. No capture or file content carried instructions.
