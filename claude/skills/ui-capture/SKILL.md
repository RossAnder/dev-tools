---
name: ui-capture
description: "The stack-neutral UI capture contract a project implements — `.claude/ui-capture.toml`, the capture command's `--out`, `--only` and `--list` flags, the per-state `<state>.png` and `<state>.txt` outputs and the `geometry.json` layout report — with how web, QML and terminal UIs meet it, and the visual defect rubric every judge of a capture uses. Use when declaring or writing a project's capture command, reading a capture, or judging one."
---

# UI capture

A capture is how the pipeline sees a UI it built. The project owns the code that captures, because only the project knows how to start its UI and drive it into a state. This skill owns the shape that code must produce, so the visual pass, the review check and plan time all read one declaration and one output layout on any stack. Nothing here needs a browser or a web file type: a web page, a QML window and a terminal UI meet the same contract.

## The declaration

A project declares its capture once, in `.claude/ui-capture.toml` at the repo root:

```toml
command = "bun run visual-audit"
out = ".ui-capture"
paths = ["web/src/**", "shell/**/*.qml"]
controls = ["settings-04"]
```

| Key | Value |
|---|---|
| `command` | Required. The capture command, run from the repo root through the shell. The flags below are appended to it. |
| `out` | Required. The default output directory, relative to the repo root; the visual pass builds each round's directory under it. A caller that passes `--out` overrides it. |
| `paths` | Optional. Globs naming the UI's source files. A change touching any of them is a UI change, on any stack, beside the web and QML file globs the review vocabulary already knows. |
| `controls` | Optional. The names of the capture's control states (see **Controls** below), each one the command knows. A review of a change reads this list so its lenses skip each control's planted defect; the visual pass takes its controls from the milestone, never from this key. |

## The command

| Flag | Meaning |
|---|---|
| `--out <dir>` | Write every output under `<dir>`, creating it when absent. |
| `--only <state>` | Capture only this state. Repeatable; without it, every state is captured. |
| `--list` | Print every state name the command knows, one per line, and capture nothing. |

Exit 0 means every state asked for was captured, with its outputs written. Any other exit means at least one was not, and the caller treats the run as a capture failure, never as a pass. A state named by `--only` that the command does not know is a failure too.

States are the project's own data: each names a UI condition the command can reach, such as an empty list, a very long label, an error banner or a narrow viewport. Capturing the same view at content and size extremes is what exposes dead space, a panel that grows with its container and a fixed-height panel, so a project's state list should carry those extremes for every view that has them.

## Outputs

Per state, the command writes one or both of these under `--out`:

- **`<state>.png`**: an image of the UI at a fixed viewport or window size and scale 1, never a full-page or full-scroll capture. The Read tool downsamples a large image before a judge sees it, so an oversized capture loses the fine detail a judge needs; a viewport-sized capture at scale 1 reaches the judge whole.
- **`<state>.txt`**: a terminal cell grid at a fixed terminal size, one line per row, holding only the characters drawn, with no escape codes. A terminal UI's grid reads more exactly as text than as an image.

The command also writes **`geometry.json`** under `--out`: one object keyed by state name, each value an array of layout violations the capture found in that state.

```json
{
  "settings-narrow": [
    {"kind": "overflow", "locator": "form > .row:nth-child(3) label", "detail": "scrollWidth 412 > clientWidth 280"}
  ],
  "settings-empty": []
}
```

| Field | Value |
|---|---|
| `kind` | `overflow`, `clipping`, `overlap` or `offscreen` |
| `locator` | Whatever names the element on that stack: a CSS selector, a QML `objectName` or item path, a widget name and cell rectangle |
| `detail` | A short measurement saying what was found, such as the two sizes or rectangles compared |

Every state captured as an image has a key in `geometry.json`, its array empty when nothing was found. A missing key for an image state is a capture failure, not a pass: a vision judge is near chance on clipping and overflow, so those classes rest on measured geometry, and a missing measurement must not read as a clean one. A state captured only as a text grid needs no key, because clipping and overflow show in its text. A state written as both may carry a key, and one that does is read like an image state's.

## Meeting the contract per stack

**Web.** Drive the app or a fixture page with Playwright or with a Chromium-family browser in headless mode, at a pinned viewport, device scale factor 1, reduced motion and a fixed colour scheme, waiting for fonts and two animation frames before the shot. Measure geometry in the same page before the shot: `scripts/geometry-probe.js` in this skill is a dependency-free browser script that reports overflow, clipping, overlap and offscreen elements in the shape above, and `scripts/render.sh` renders a standalone HTML file, such as a design mock or a fixture, to a PNG and a `geometry.json` with an installed browser and nothing else.

**QML and Quickshell.** Load the component or shell at a fixed window size, then save the image with `grabToImage` on the root item or with a screenshot of that fixed-size window. Geometry comes from item bounds: compare each item's mapped rectangle with its parent's and its siblings', and report a child outside its clipping parent, two visible items whose rectangles intersect, or an item mapped outside the window.

**Terminal UIs such as ratatui.** Render each state into a test backend of a fixed size, ratatui's `TestBackend` for example, and write the buffer's symbols out row by row as `<state>.txt`. No image and no geometry key are needed. A project that also wants an image of a terminal state writes the PNG and then owes its geometry key like any image state.

**Other stacks.** Any UI that can be driven into a state at a fixed size and either imaged or dumped as text can meet the contract. The flags, the output names and the `geometry.json` shape are the whole interface; how the command gets there is the project's choice.

## Controls

A milestone may name some of its states as controls. A control state carries a defect planted on purpose, so a judge that finds nothing in it is a judge that is not looking, and its answers on the other states cannot be trusted. The capture command captures a control like any other state; the project builds the planted defect into the state itself. The visual pass's inspector is not told which states are controls, so a control it fails is a defect it found without being told one was there; it judges the states it is given and never reads the declaration's `controls`. A review lens is told: it judges the change rather than the inspector, so it skips each control the declaration lists and never reports the planted defect as one the change made. A project lists every control in the declaration's `controls`, so a review that captures every state knows which ones carry a plant. A control is judged against the look entries of the clean states that share its name up to the last `-`, so a project names each state `<surface>-<n>`, such as `settings-03`, with a control taking its surface's prefix.

## Judging a capture

Every judge of a capture, whether an inspector reading the visual pass or the review lens reading a change, works through `references/defects.md`: each defect class is a yes-or-no question asked of every state. Three rules hold for every answer:

- An observed defect is a fail. Never talk it down as minor, intended or acceptable in context; the person reading the verdict decides that, not the judge.
- An answer the judge is unsure of is reported as unsure, with where to look, never rounded to a pass.
- A `geometry.json` entry is evidence the judge reports. The judge may add what the image shows beyond it, but never clears an entry because the image looks fine.

## References

| File | Read when |
|---|---|
| `references/defects.md` | Judging any capture, or writing a control state |
| `scripts/geometry-probe.js` | Adding geometry measurement to a web capture |
| `scripts/render.sh` | Rendering an HTML mock or fixture to the contract's outputs |
