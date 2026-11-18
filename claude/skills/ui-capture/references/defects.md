# Visual defect rubric

Ask every class below of every captured state, one at a time, and answer each one `yes` (the defect is present), `no` or `unsure`. A `yes` names where: a region of the image, a locator from `geometry.json`, or a row and column range of a text grid. Do not give one overall score in place of the answers.

Three rules bind every answer:

- **An observed defect is a fail.** Report it as seen. Never talk it down as small, probably intended or acceptable in context; a judge left to grade a build generously talks itself out of what it found, and the person reading the verdict is the one who decides whether a defect matters.
- **Unsure is an answer.** When the capture does not settle the question, answer `unsure` and say what would settle it: a crop, another state, a geometry entry. Never round unsure to `no`.
- **Measured geometry outranks the eye.** For clipping, overflow, overlap and offscreen, a `geometry.json` entry is a `yes` even when the image looks fine, because a vision judge is near chance on those classes. An image state with no `geometry.json` key is a capture failure, not a clean state.

Look at the whole state first, then re-read each region you flag, or are asked about, at full size. Fine defects such as a one-pixel misalignment or a clipped descender only show close up.

When a chosen mock sits beside the capture, run this rubric as its own pass. Whether the capture is the layout the user picked is a separate question from whether it has defects, and answering both at once lets the comparison crowd out the checklist.

## The classes

### 1. Scaling with the container

**Does an element grow or shrink with its container where its content, not the container, should set its size?**

Counts: a button, badge, icon or input that stretches across a wide panel; a card whose text spreads into long lines on a wide window; an icon that grows with a toolbar's height. Compare a narrow state with a wide one: an element whose size tracks the viewport while its content stays the same is the defect. Does not count: a layout region meant to fill, such as a main pane or a list.

- Web: a `Save` button that spans the whole 1600 px form row in the wide state and the whole 400 px row in the narrow one.
- QML: a `ToolButton` anchored to fill a `RowLayout` cell, so it is three times as wide as its label when the window widens.
- Terminal: a status-line gauge that fills the full terminal width and becomes a long bar at 200 columns.

### 2. Reserved empty space

**Is there a band of empty space that holds no content and separates nothing?**

Counts: a gap kept open for an element absent in this state, such as a hidden toolbar or an empty error slot; a fixed-height region with half its height unused; padding that pushes content away from where it belongs. Check the empty and short-content states most closely. Does not count: margins and gutters of the project's normal size, or space that sets apart two groups.

- Web: a 48 px blank strip above the table where an alert banner renders only on error.
- QML: a `ColumnLayout` with a fixed `Layout.preferredHeight` that leaves the lower 200 px of a popup empty when the list has two items.
- Terminal: a pane split that reserves eight rows for a log panel showing nothing.

### 3. Panel sized wrong for its content

**Is a panel, card or dialog taller, shorter, wider or narrower than its content needs?**

Counts: a dialog that opens at full height for two lines of text; a panel whose content scrolls or is cut when the panel could have grown; panels in one row of different heights where they should match. Compare the short-content and long-content states. Does not count: a size the content needs, or a region sized by the layout on purpose.

- Web: a confirm dialog at 80% of the viewport height holding one sentence and two buttons.
- QML: a notification popup with a fixed height that cuts its third line of body text.
- Terminal: a help overlay box twelve rows tall around three rows of text.

### 4. Clipped or overflowing content

**Is any content cut by its container, or does it spill out of it?**

Counts: text or an icon cut at an edge; content drawn outside its box; a horizontal scrollbar on a region that should not scroll; any `overflow`, `clipping` or `offscreen` entry in `geometry.json`. Check the long-label and narrow states most closely. Does not count: a region designed to scroll, showing a scroll affordance.

- Web: a long file name that runs past the right edge of its card and under the next one; `geometry.json` reports `scrollWidth` above `clientWidth`.
- QML: a `Text` without `elide` inside a clipped `Rectangle`, cut mid-word at the right edge.
- Terminal: a table cell's text that runs into the next column instead of stopping at its border.

### 5. Overlap

**Do two elements cover each other where neither is meant to sit on top?**

Counts: text over text, a control under another control, a badge over a label, a footer over the last row; any `overlap` entry in `geometry.json`. Does not count: a deliberate layer such as a menu, tooltip, modal or floating action button.

- Web: an absolutely positioned close button that sits on top of the dialog title in the narrow state.
- QML: two `Item`s in an `Item` parent with no layout, both at `x: 0`, their labels drawn over each other.
- Terminal: a popup drawn without clearing the area under it, so the list's characters show through its blank cells.

### 6. Misalignment

**Do elements that share an edge, baseline or centre line fail to line up?**

Counts: labels in a form column with different left edges; icons and text off a shared baseline; a column of numbers not aligned on the right; one card in a grid a few pixels off its row. Does not count: a deliberate indent showing hierarchy.

- Web: the third input in a form starting 4 px right of the others because its label is wider.
- QML: an `Image` and a `Text` in a `Row` whose vertical centres differ because the row aligns their tops.
- Terminal: right-aligned numbers in a table column that are left-aligned in one row.

### 7. Off-style control or glyph

**Is a control, icon or glyph drawn in a style the project's UI does not otherwise use?**

Counts: a browser-default or toolkit-default control among styled ones; an icon from a different set or weight; an emoji where the UI uses icons; a font, corner radius or border style found nowhere else. Judge against the project's other screens or its design notes, not against taste. Does not count: a style the project uses elsewhere.

- Web: a native `<select>` with the platform's grey chrome in a form of custom dropdowns.
- QML: a default `Controls` `CheckBox` in a shell drawn entirely with custom `Rectangle` toggles.
- Terminal: a single-line box-drawing border on one pane in a UI whose other panes use rounded borders.

### 8. Truncation without an ellipsis

**Is text shortened with no sign that it was shortened?**

Counts: a label cut at a character or pixel boundary with no `…`; a list item that ends mid-word with nothing to show more exists. Does not count: text ending in an ellipsis, or a field that shows its full value on focus and shows it in this state.

- Web: a breadcrumb segment cut at its `max-width` with `overflow: hidden` and no `text-overflow: ellipsis`.
- QML: a `Text` with a fixed `width` and `elide: Text.ElideNone` inside a clipping parent.
- Terminal: a branch name cut at the column limit with no `…` or `>` marker.

### 9. Terminal column or border out of line

**In a text grid, is a column, border or box edge broken or out of line from row to row?**

Counts: a vertical border with a gap or a step in it; table columns whose separators wander between rows; a box corner that does not meet its edges; a wide character such as an emoji or a CJK glyph that pushes the rest of its row one cell right. Check every row of the `<state>.txt` grid, not only the first and last. Does not count: an intended irregular layout.

- Terminal: a `│` column border at column 30 on most rows and column 31 on the row whose cell holds `✅`.
- Terminal: a box whose right edge is drawn one row shorter than its left, leaving `┐` with no `┘` below it.
- Web or QML: the same question applies to monospaced tables and code blocks; a column of figures in a proportional font that does not line up counts here too.

### 10. Colour as the only signal

**Does a state or meaning reach the user through colour alone?**

Counts: an error shown only by a red border; a status shown only by a dot's colour; a selected row marked only by a background tint with no other cue; a chart series told apart only by hue. Does not count: colour paired with an icon, a label, a weight or a pattern.

- Web: an invalid input whose only sign is its border turning red, with no message or icon.
- QML: an online and offline indicator that is the same `Rectangle`, green or grey.
- Terminal: failed rows marked only by red foreground text, which reads as plain text on a monochrome terminal or to a colour-blind user.

## Controls

A control state carries one of these defects planted on purpose. A control should plant one clear instance of one class, at a size a careful judge would catch: a label that overflows its card by a third of its width, a 60 px reserved band, a border stepped by one column. A control must come back with at least one `yes`; when the judge finds no defect in it, the judge says so, and its answers on the other states cannot be trusted.
