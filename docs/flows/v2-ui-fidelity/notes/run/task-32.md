Status: done — panel.html draws the eleven stacked-rows states and states.txt lists them with their windows

```toml run-note
files_touched = ["claude/skills/ui-capture/fixtures/web/panel.html", "claude/skills/ui-capture/fixtures/web/states.txt"]

[[deviations]]
what = "The rows region gets its scrolling class from script only when its content is taller than the space it has, rather than always carrying overflow-y: auto."
why = "An always-on overflow-y: auto forces overflow-x to auto as well, which would clip the one-line label at the panel's edge in web-05 and turn that state's spill into a scrollbar. With the conditional class every state draws the same rows region, and a list longer than the window still scrolls inside the panel as the design asks."
means = "design/ui-B.md#web-panel content extremes (a list longer than the window scrolls inside the panel)"

[[deviations]]
what = "A state named in location.hash takes precedence over the data-state attribute; an attribute or hash that names no state falls back to web-01. The page also redraws on hashchange."
why = "The attribute always holds a value (web-01 by default), so the hash has to win for panel.html#web-04 to open web-04 in a browser as the detail asks."
means = "detail: read the state from dataset.state, or from location.hash"

[[deviations]]
what = "The web-05 and web-06 label is 99 characters, not about 90."
why = "Measured with render.sh: at 92 characters the label ended 136 px past the panel's right edge, short of the third of the panel's width (160 px) the table asks for. At 99 characters it ends 171 px past the edge (label box 57..675, panel edge 504)."
means = "detail table: a label of about 90 characters"

[[checks]]
command = "f=claude/skills/ui-capture/fixtures/web; test \"$(grep -c . $f/states.txt)\" -eq 11 && while read -r s size; do grep -qF \"$s\" $f/panel.html || exit 1; done < $f/states.txt"
result = "pass"

[[checks]]
command = "render.sh --size <window> on a per-state copy of panel.html for each of the eleven states, into a scratch directory"
result = "pass"

[[checks]]
command = "grep -niE 'defect|control|planted|clean' claude/skills/ui-capture/fixtures/web/panel.html"
result = "pass"
```

## What changed

- `claude/skills/ui-capture/fixtures/web/panel.html` (new). One self-contained page with no external resources. The first `<html` tag is exactly `<html lang="en" data-state="web-01">`. The CSS tokens and row markup are taken from the mock `design/ui-B/stacked-rows.html`: 24 px page margin, the panel's width up to 480 px, its height following its rows, fixed 18 px swatches, a muted `▸` fold glyph, the label as the only flexible element with an ellipsis, and a notices band drawn only when a state has a notice. The script holds a table of the eleven states and builds the DOM for the one it picks. The variant classes are named for what they draw (`fillWindow`, `unclippedLabel`, `heldBand`, `stretchTiles`, `boxedFold`). No comment, class, id or visible text names a defect or a control.
- `claude/skills/ui-capture/fixtures/web/states.txt` (new). Eleven lines, one per state, `<name> <width>x<height>`, in the table's order.

## Verification

I made a per-state copy of panel.html with the attribute replaced, the way task 34's capture command will, and rendered each copy with `render.sh` at its window into a scratch directory. I then looked at the PNGs and read geometry.json:

- The clean states (web-01, 03, 04, 06, 08, 10) have an empty geometry report, and their PNGs match the design: at the 500 px window the panel is 452 px wide, at 1280 px it stops at 480 px, and web-06 cuts its long label with an ellipsis before the swatches.
- web-02, web-07, web-09 and web-11 also report nothing. Their single variation is visible only in the image: a panel held at the window's height, an empty 48 px band under the last row, swatches stretched across the row, and bold `−`/`+` in bordered squares. In web-11 the boxed glyph at first overflowed its 16 px box vertically, which would have added a second, unintended defect. Setting a 14 px line-height fixed it, and that state's report is now empty.
- web-05 reports overflow and offscreen entries for the label and the swatches, as planned.
