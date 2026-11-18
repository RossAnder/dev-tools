+++
id = "ui-visual"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["layout-sizing", "overflow-clipping", "style-conformance"]
activation = "signal"
apply_when = { signals = ["ui-change"] }
slice = "per-subsystem"
v1_category = "quality"
+++

# ui-visual

## Question

In which captured state of the changed UI does an element take the wrong size, leave space it does not use, get cut, spill or overlap, or depart from the style the rest of the UI uses?

## Owns

`layout-sizing` — an element whose size follows its container where its content should set it, a band of reserved empty space that holds nothing and separates nothing, and a panel, card or dialog taller, shorter, wider or narrower than its content needs.

`overflow-clipping` — content cut by its container or spilling out of it, two elements covering each other where neither is meant to sit on top, an element drawn offscreen, text shortened with no ellipsis, and a terminal column or border broken or out of line from row to row.

`style-conformance` — a control, icon, glyph, font, radius or border drawn in a style the project's UI uses nowhere else, elements that share an edge, baseline or centre line and fail to line up, and a meaning carried by colour alone.

## Probes — code

Judge the captures first and the source second. The `ui-capture` check runs the project's declared capture command before the cells and writes every state it lists under the run directory's `ui-capture/`: a `<state>.png` image at a fixed size, a `<state>.txt` text grid for a terminal UI, and `geometry.json`, the measured layout violations keyed by state. Its baseline says `ui-capture: none` when the project declares no capture, and `ui-capture: incomplete`, naming the states, when an image state came back with no geometry key.

- **Rubric.** Load the `ui-capture` skill's `references/defects.md` and ask its classes of every captured state in the slice, one class at a time: scaling with the container, reserved empty space and panel sizing for `layout-sizing`; clipping and overflow, overlap, truncation without an ellipsis and terminal columns out of line for `overflow-clipping`; misalignment, an off-style control or glyph and colour as the only signal for `style-conformance`. Re-read every region you flag at full size before writing it.
- **Measured geometry.** Each entry in a captured state's `geometry.json` array is a defect: report it as found, even when the image looks clean, and never clear one by eye. An image state with no `geometry.json` key is a capture failure, not a clean state: report it as an advisory with `angle = risk` naming the state, and judge its clipping and overflow classes no higher than `grade = medium`.
- **Controls.** A state the ui-capture baseline lists under `controls:` carries a defect planted on purpose. Do not report its planted defect or its `geometry.json` entries; skip it and count it in the coverage note as a control.
- **Extremes.** Compare the narrow state with the wide one and the empty or short-content state with the long one. An element whose size tracks the viewport while its content stays the same, or a gap kept open for an element absent in this state, shows only across the pair.
- **Text grids.** In a `<state>.txt` grid, read every row, not the first and last: a border that steps one column, a wide character that pushes the rest of its row, a cell's text running into the next column.
- **Style.** Judge an off-style control against the project's other screens, its design notes or its theme files, never against taste. Cite the place the project's own style is set.
- **Back to source.** For each captured defect, find the component, style rule, layout or widget that produces it, so the finding's fix names a code edit.
- **No capture.** When the baseline says `ui-capture: none`, or the slice changes a view no captured state reaches, read the changed markup, styles, layout code and widgets for the same classes: fixed sizes and stretch rules, missing ellipsis or wrapping, absolute positioning, and hard-coded colours, fonts and controls. A finding from source alone carries `grade = medium` at most.

Every finding's evidence cites the capture it rests on: the state's file under `ui-capture/`, with the image region, `geometry.json` locator or grid row and column range, beside the source file and line that produces it. A defect's `failure_scenario` names the state and what the user sees in it.

## Does not flag

- A confirmation. A state you judged clean goes in the cell's coverage note, never in a finding.
- A layout region meant to fill, such as a main pane or a list; a region designed to scroll that shows a scroll affordance; a deliberate layer such as a menu, tooltip or modal; a deliberate indent showing hierarchy.
- Margins and gutters of the project's normal size, and space that sets two groups apart.
- A style the project uses elsewhere, or a preference with no departure from the project's own style to cite.
- Behaviour a comment, decision record, design note or CLAUDE.md line documents as deliberate. A disagreement with it is advisory with `angle = challenge`, naming the decision.
- Missing, wrong or indistinguishable empty, loading, error and disabled states, and focus, keyboard, pointer and scroll behaviour, which are the interaction lens's.
- Whether the UI is the layout the user chose, which is the intent-fidelity lens's.
- Pixel-level differences from an earlier build with no defect class behind them: this lens keeps no golden image.
- Work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.

## Edit forms

ui-visual writes defects first; its cleanup is a style or layout edit that leaves every captured state looking the same.

- `change` — a hard-coded colour, size or font replaced by the project's own token or theme value with the same rendered result.
- `remove` — a style rule or layout property that no captured state and no selector or binding in the tree reaches, cited with the search.
- `add` — an ellipsis or elide setting on text that today always fits, where a longer value the code accepts would be cut.

## Advisory angles

- `risk` — an image state captured with no geometry key, or a view the capture never reaches, where a sizing or clipping defect cannot be ruled out; name the state that would settle it.
- `challenge` — a documented style or layout decision that produces one of the rubric's defects in a captured state.

## Severity calibration

- `critical` — content or a control the user needs is hidden, unreachable or unreadable in a common state.
- `major` — a measured overflow, clip or overlap, or a sizing defect a user sees in a common state such as the default window size.
- `minor` — a misalignment, off-style glyph or sizing defect seen only at an extreme size or with unusually long content.
