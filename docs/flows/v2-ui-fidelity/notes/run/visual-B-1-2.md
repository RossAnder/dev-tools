Verdict: 4 states judged, 2 fail, 0 unsure, 2 pass

# Visual pass, milestone B, round 1 (inspector 2)

Flow `v2-ui-fidelity`, capture dir `.ui-capture/B-1/`, design `docs/flows/v2-ui-fidelity/design/ui-B.md`.

Look entries in scope: task 32 accept 0 (the clean web states show none of the rubric's defects) and task 32 accept 1 (swatches, fold glyphs and the Add button keep their size at every width, the label alone takes the slack and ends in an ellipsis when cut, the panel ends at its last row with no band below it). Both name `web-04` and `web-06` directly. `web-05` carries no entry by name; by the capture contract's prefix rule it is judged against the entries of the clean `web-` states, so both apply to it. `tui-05` shares no prefix with any state the look entries name, so it carries no look entry.

Measurements below come from pixel sampling of the PNGs (pure-Python decoder in the session scratchpad) and from column counts of the text grids.

## web-05: fail

Capture: `web-05.png` (640x480) present, `geometry.json` key present with six entries.

**Geometry.**

- clipping (overflow), `html`: "document scrollWidth 785 > viewport width 640".
- clipping (overflow), `body > section.panel:nth-child(2)`: "scrollWidth 760 > clientWidth 478".
- clipping (overflow), `body > section.panel:nth-child(2) > div.rows:nth-child(2)`: "scrollWidth 760 > clientWidth 478".
- clipping (overflow), `body > section.panel:nth-child(2) > div.rows:nth-child(2) > div.row.unclippedLabel`: "scrollWidth 760 > clientWidth 478".
- clipping (offscreen), `... > div.row.unclippedLabel > span.label:nth-child(2)`: "box [57,78 618x20] outside viewport 640x480".
- clipping (offscreen), `... > div.row.unclippedLabel > span.sw:nth-child(3)`: "box [683,79 102x18] outside viewport 640x480".

**Rubric.**

1. Scaling: no. The Add button measures x 445-490, y 35-58, the same as `web-04`, `web-06`, `web-08`; the fold glyph is 5x4 px at x 39-43 as in every sibling.
2. Reserved space: no. The panel ends at its single row (bottom border y 106), no band below.
3. Panel size: no. The panel holds its designed 480 px maximum (borders x 24 and 503); the content's spill is answered under clipping.
4. Clipping: yes. Every geometry entry above; in the image the series label runs from x 57 past the panel's right border (x 503) to the viewport edge at x 639, and the swatches are not drawn at all (offscreen at x 683).
5. Overlap: yes. Row 1, x 503, y 86-92: the label's glyphs ("seasonal") are drawn over the panel's right border line; sampled pixels on the border column read text colours (`#a35a1a`, `#855a7f`) instead of `#d9d5cf`. No geometry entry reports it; the image does.
6. Misalignment: no among drawn elements; the displacement of the swatch column is reported under clipping (offscreen).
7. Off-style: no. Header, Add button, glyph and border match the mock and siblings.
8. Truncation: yes. Row 1, x 600-639: the label "... seasonal returns, fiscal" is cut at the viewport's right edge with no ellipsis.
9. Grid alignment: no (no monospaced grid in this state).
10. Colour only: no. The count is text; no state is signalled by colour.

**Look entries.**

- Task 32 accept 0: not met. The state shows clipping, overlap and truncation (row 1, x 57-639).
- Task 32 accept 1: not met. The label does not end in an ellipsis when cut, and the five swatches are not kept in place: they sit offscreen at x 683 instead of x 389-490 (row 1).

**Design.** The design's long-label extreme says the label is "cut to one line with an ellipsis before the swatches; the swatches stay where they are". The capture's label leaves the panel and the viewport uncut, and the swatch group is pushed off the window: an arrangement difference.

## tui-05: fail

Capture: `tui-05.txt` present, 25 lines (24 rows of 80 cells and a trailing empty line). Text-grid state, so no geometry key is needed; none exists.

**Geometry.** No entries (none required for a text-only state).

**Rubric.** Rows counted from 0 at the top border, columns from 0 at the left border.

1. Scaling: no. The hint line and table hold their designed cells.
2. Reserved space: no. Rows 8-21 are blank table rows under the last task, which the design assigns to the table; no other row is held open.
3. Panel size: no. The box fills 80x24 as designed.
4. Clipping: no. No text runs past its column or the border; the cut title is answered under truncation.
5. Overlap: no.
6. Misalignment: no. IDs end in column 3, status glyphs sit in column 6, titles start in column 10, FILES digits sit in column 77 on rows 3-7, and `5 tasks` ends at column 77.
7. Off-style: no. Rounded corners `╭ ╮ ╰ ╯`, `─`, `│` and the glyphs `✓ ○` as the design names.
8. Truncation: yes. Row 5, columns 10-72: the title `Rework the capture contract so every project declares its own s` is cut mid-word at column 72 with no `…`. The sibling `tui-06` shows the same title ending `its own …`.
9. Grid alignment: no. Every row is 80 cells, `│` sits at columns 0 and 79 on rows 1-22, the corners meet their edges, the separator on row 2 runs columns 2-77.
10. Colour only: no. Status is carried by distinct glyphs.

**Look entries.** None carried (the look entries in this round name web states only).

**Design.** The design's long-title extreme says the title is "cut so that it ends with `…` in column 72; the `FILES` column stays in line". The FILES column stays in line, but the title ends in `s` at column 72 with no `…`: an arrangement the capture does not follow.

## web-04: pass

Capture: `web-04.png` (640x560) present, `geometry.json` key present and empty.

**Geometry.** No entries.

**Rubric.**

1. Scaling: no. Add button x 445-490, y 35-58; swatches 18x18 (first swatch x 389-406, y 79-96) with 3 px gaps, group x 389-490; identical in `web-08` (1280 wide) and shifted only by the panel edge in `web-10` (500 wide, Add 417-462, swatches 361-462).
2. Reserved space: no. The notices band (y 363-396, `#e7e4e0`) holds a notice; the panel's bottom border is at y 397 and nothing follows inside it.
3. Panel size: no. Eight rows plus the notice; height set by content, width 480 (x 24-503).
4. Clipping: no. No geometry entry; all labels are short and whole.
5. Overlap: no.
6. Misalignment: no. Labels start at x 57, fold glyphs at x 39-43, swatch groups end at x 490 on every row; dividers at y 69, 106, 142, 179, 216, 252, 289, 325, 362 (36/37 px alternation, as in the mock).
7. Off-style: no. Matches the mock's header, outlined button, glyph and band.
8. Truncation: no.
9. Grid alignment: no.
10. Colour only: no. The notice is text; the count is text.

**Look entries.**

- Task 32 accept 0: met. No rubric defect in the many-items state (whole image).
- Task 32 accept 1: met. Swatches, glyphs and Add button at their fixed sizes (measured above); no label is long enough to be cut; the panel ends at its last element, the notice band, which is present because a notice exists.

**Design.** Matches `stacked-rows`: header with `Series`, muted count and right-aligned Add; eight series rows each glyph, label, five right-aligned swatches; notices band at the bottom when a notice exists; panel width 480 at a 640 window with empty page to its right. The mock draws a 2 px line (y 216-217) above the band where the capture draws 1 px (y 362); the design text specifies 1 px, and this is exact spacing, not arrangement.

## web-06: pass

Capture: `web-06.png` (640x480) present, `geometry.json` key present and empty.

**Geometry.** No entries.

**Rubric.**

1. Scaling: no. Add button and swatches measure as in `web-04`.
2. Reserved space: no. The panel's bottom border is at y 179, directly under the third row; no notices band and no gap where one would be.
3. Panel size: no. Three rows; width 480.
4. Clipping: no. The long label's ink ends at x 377, before the swatches at x 389.
5. Overlap: no.
6. Misalignment: no. Labels at x 57; swatch groups at x 389-490 on all three rows, including the long-label row.
7. Off-style: no.
8. Truncation: no. Row 2 ends `adjusted for cur…` with a visible ellipsis (magnified crop checked).
9. Grid alignment: no.
10. Colour only: no.

**Look entries.**

- Task 32 accept 0: met. No rubric defect in the long-label state.
- Task 32 accept 1: met. The label alone is cut and ends in an ellipsis; swatches, glyphs and Add keep their measured sizes; the panel ends at its last row with no band below.

**Design.** Matches `stacked-rows`' long-label extreme: one line, ellipsis before the swatches, swatches in their column; no notices band when there is no notice.

Coverage: read the rubric (`ui-capture/references/defects.md` under the Claude config dir), `.ui-capture/B-1/geometry.json`, `docs/flows/v2-ui-fidelity/design/ui-B.md`, the design pictures `ui-B/stacked-rows.png`, `ui-B/stacked-rows-narrow.png` and `ui-B/single-table.txt`, the captures `web-04.png`, `web-05.png`, `web-06.png`, `tui-05.txt`, and for sibling comparison `tui-01` to `tui-09` rows 3-8 and pixel measurements of `web-03.png`, `web-08.png`, `web-10.png`. Nothing failed to read. No capture or file content attempted to direct the judgement.
