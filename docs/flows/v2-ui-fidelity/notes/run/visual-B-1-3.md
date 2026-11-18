Verdict: 4 states judged, 2 fail, 0 unsure, 2 pass

# Visual pass, milestone B, round 1: web-07, tui-07, web-08, web-10

Judged against `references/defects.md` of the `ui-capture` skill, `.ui-capture/B-1/geometry.json` and `docs/flows/v2-ui-fidelity/design/ui-B.md` with its pictures `ui-B/stacked-rows.png`, `ui-B/stacked-rows-narrow.png` and grid `ui-B/single-table.txt`. Close-up crops were enlarged 3x into a scratch directory outside the repo.

## web-07 — fail

Capture: `web-07.png`, 640x560; geometry key present.

**Geometry.** `web-07` is `[]`: no entries.

**Rubric.**

1. scaling: no. Swatches 18 px with 3 px gaps, Add button at label width, fold glyph small; same as web-04 at the same width.
2. reserved-space: **yes**. Panel interior from y≈362 (the line under `Critical`) to the panel's bottom border at y≈409: a white band of about 47 px holding nothing and separating nothing. Its sibling web-04 (same 640x560, same eight rows) uses exactly that height for the notices band, so this is the notice slot kept open with no notice.
3. panel-size: **yes**. Same region: the panel is about 47 px taller than its header and eight rows need; the design says its height is its rows'.
4. clipping: no. Labels short and whole, swatches inside the panel, nothing past the viewport.
5. overlap: no.
6. misalignment: no. Label left edges (x≈57), glyph column (x≈41), swatch right edges (x≈491) and the Add button's right edge line up row to row.
7. off-style: no. Outlined Add button, swatches and borders match the other web states.
8. truncation: no. No label is cut.
9. grid-alignment: no. No monospaced table or text grid in this state.
10. colour-only: no. Every swatch row is labelled; no state carried by colour alone.

**Look entries** (the web surface's entries, task 32).

- 32/0 (clean states show none of the rubric's defects): **not met**, the reserved band in the lower panel is a rubric defect.
- 32/1 (fixed swatches, glyphs and Add; label takes the slack; panel ends at its last row with no band below it): **not met**. Swatches, glyph and Add hold their size, but the panel does not end at its last row; the empty band sits below `Critical`.

**Design.** `## Web panel`, `stacked-rows`: header, then rows, panel ends at its last row; with no notice, no gap where the band would be. The capture keeps that gap, so the arrangement differs (dead space below the last series row). Width 480 px at a 24 px margin matches.

## tui-07 — fail

Capture: `tui-07.txt`, 80x24 text grid; text-only, so no geometry key is owed.

**Geometry.** No key; none owed for a text-only state.

**Rubric.**

1. scaling: no. Columns hold fixed cells; the box fills the terminal as designed.
2. reserved-space: no. Rows 8 to 21 are blank table rows under the last task, which the design assigns to the table.
3. panel-size: no. One box over 80x24 as designed.
4. clipping: **yes**. Row 5, columns 10 to 78: the title `Rework the capture contract so every project declares its own states,` runs past the TITLE column's end at column 72, through the FILES column (73 to 77) and the padding cell at 78, up to the right border.
5. overlap: **yes**. Row 5, columns 73 to 77: the title's characters occupy the FILES cell, and that row's FILES value (`3` on the other rows' pattern) is not drawn.
6. misalignment: no. ID in columns 2 to 3, status glyph in column 6, titles from column 10 on rows 3 to 7; the separate column break on row 5 is reported under class 9.
7. off-style: no. Rounded corners `╭ ╮ ╰ ╯`, `─` and `│` throughout; glyphs `✓` and `○` from the design's set.
8. truncation: **yes**. Row 5, columns 10 to 78: the title stops at `states,` with no `…`, so the cut gives no sign it was cut.
9. grid-alignment: **yes**. Row 5, columns 73 to 77: the right-aligned FILES column, in line on rows 1, 3, 4, 6 and 7 (digit at column 77), is broken on row 5. The `│` borders at columns 0 and 79 stay in line on every row.
10. colour-only: no. The grid has no colour; statuses are distinct glyphs.

**Look entries.** None: the look entries this run carries (task 32, accepts 0 and 1) name only web states, and no terminal-surface look entry was given. Nothing to judge here.

**Design.** `## Terminal panel`, `single-table`: a long title is cut so it ends with `…` in column 72 and the FILES column stays in line. Row 5 runs to column 78 with no ellipsis and drops its FILES value, so the capture does not follow that arrangement. The rest (box, title in the top border, header row 1, separator row 2, rows from row 3, hint line on row 22 with `5 tasks` ending at column 77) matches. Status glyphs differ from the mock's rows (content, not arrangement).

## web-08 — pass

Capture: `web-08.png`, 1280x560; geometry key present.

**Geometry.** `web-08` is `[]`: no entries.

**Rubric.**

1. scaling: no. At 1280 px the panel stops at 480 px (x 24 to 504) and the rest is empty page; swatches (18 px, 3 px gaps), Add button and fold glyphs are the same size as at 640 px (web-04) and 500 px (web-10).
2. reserved-space: no. The grey notices band under `Critical` holds a notice (`Accent warm step 1 is below 3:1 on white.`); the panel ends right below it. The space right of the panel is page outside the panel, as designed.
3. panel-size: no. Panel height is header, eight rows and the notice band, ending at y≈397.
4. clipping: no. Notice text and all labels whole; nothing at an edge (crop of the header and bottom rows checked at 3x).
5. overlap: no.
6. misalignment: no. Labels at x≈57, glyphs at x≈41, swatch right edges and Add right edge at x≈491.
7. off-style: no.
8. truncation: no. No text cut.
9. grid-alignment: no. No monospaced table.
10. colour-only: no. The notice is written text in a band with a line above; swatches are labelled.

**Look entries.**

- 32/0: **met**. All ten rubric classes are `no` (whole image, crops of header and bottom).
- 32/1: **met**. Swatches, glyphs and Add hold their size at 1280 px; the label column takes the slack; the panel ends at its notice band, which holds a notice, with no band below it. No label is long enough to be cut in this state, so the ellipsis clause is not exercised here.

**Design.** Matches `stacked-rows` at a window wider than 528 px: 24 px margin, panel capped at 480 px, header with count and outlined Add at the right, rows of glyph, label and five right-aligned swatches, grey notices band with a line above, panel ending at its rows.

## web-10 — pass

Capture: `web-10.png`, 500x560; geometry key present.

**Geometry.** `web-10` is `[]`: no entries.

**Rubric.**

1. scaling: no. Panel 452 px (x 24 to 476) as designed for 500 px; swatches, Add and glyphs keep the sizes seen at 640 and 1280 px; only the label column narrows.
2. reserved-space: no. Notice band holds its notice; panel ends below it.
3. panel-size: no. Same height as web-08.
4. clipping: no. Labels, notice text and swatches whole; crop at 3x of the header and bottom rows.
5. overlap: no.
6. misalignment: no. Swatch right edges and Add right edge both at x≈463; labels at x≈57.
7. off-style: no.
8. truncation: no. Labels are short enough to show whole at this width.
9. grid-alignment: no. No monospaced table.
10. colour-only: no.

**Look entries.**

- 32/0: **met**. All rubric classes `no`.
- 32/1: **met**. Fixed elements hold their size at 500 px, the label column took the narrowing, nothing wraps, and the panel ends at its notice band with no band below. No label is cut here, so the ellipsis clause is not exercised.

**Design.** Matches `stacked-rows-narrow.png` (500 px): 452 px panel, same regions and order, swatches still right-aligned, nothing wrapped.

Coverage: read `references/defects.md` (skill under the Claude config dir), `.ui-capture/B-1/geometry.json`, `docs/flows/v2-ui-fidelity/design/ui-B.md`, `ui-B/stacked-rows.png`, `ui-B/stacked-rows-narrow.png`, `ui-B/single-table.txt`, captures `web-07.png`, `web-08.png`, `web-10.png`, `tui-07.txt`, and sibling `web-04.png` for comparison. Nothing failed to read.
