+++
id = "interaction"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["interaction-states", "input-handling"]
activation = "signal"
apply_when = { signals = ["ui-change"] }
slice = "per-subsystem"
v1_category = "quality"
+++

# interaction

## Question

Which state or input of the changed UI leaves the user without feedback, stuck, or acting on something other than what they meant?

## Owns

`interaction-states` — the empty, loading, error and disabled states of a view or control: a state the UI never shows, one it shows wrongly, one indistinguishable from another, such as an empty list that looks like a load still running, a disabled control that looks enabled, or an error that clears nothing and explains nothing.

`input-handling` — focus, keyboard and pointer input, scroll and autoscroll: focus lost, trapped or never visible, a control the keyboard cannot reach or activate, a pointer target too small or covered, a hover or press with no response, a list that does not scroll to new content or jumps away from the user's position.

## Probes — code

Judge the captures first and the source second. The `ui-capture` check runs the project's declared capture command before the cells and writes every state it lists under the run directory's `ui-capture/`: a `<state>.png` image at a fixed size, a `<state>.txt` text grid for a terminal UI, and `geometry.json`, the measured layout violations keyed by state. Its baseline says `ui-capture: none` when the project declares no capture, and `ui-capture: incomplete`, naming the states, when an image state came back with no geometry key.

- **State coverage.** For each view and control the slice changes, list the empty, loading, error and disabled states it can reach and find each one among the captured states. Judge each captured one: does it say what happened and what the user can do, and does it look different from its neighbours? A reachable state no capture shows is read from source, and a project whose state list lacks it is worth an advisory naming the state.
- **Defect rubric.** A captured empty, loading, error or disabled state is still a captured state: ask the `ui-capture` skill's `references/defects.md` classes that bear on feedback, colour as the only signal above all, since an error or disabled state marked by colour alone is this lens's defect. Each entry in that state's `geometry.json` array is a defect as the rubric says; report it, and when it is a sizing or clipping defect rather than a state or input one, leave it to the ui-visual lens and name it in the coverage note. An image state with no `geometry.json` key is a capture failure to report as an advisory with `angle = risk`, never a clean state.
- **Transitions.** Trace the code that moves between states: a load that fails and never leaves loading, an error that persists after a retry succeeds, a disabled control re-enabled before the work it guards is done, a double submit the disabled state was meant to stop.
- **Focus.** Where does focus go when a view opens, a dialog closes, an item is deleted or a list refreshes? Flag focus dropped to the document or window, a dialog that does not hold focus, and a focused control with no visible focus indicator in its captured state.
- **Keyboard.** Every action a pointer can take has a key path: tab order follows the visual order, Enter and Space activate, Escape cancels and closes, arrow keys move within lists and menus. A terminal UI's key bindings are read from its handler and checked against the help text it shows.
- **Pointer.** Hit targets large enough to press and not covered by another element, hover and press feedback, and drag or scroll gestures that do not fight the container's own.
- **Scroll and autoscroll.** A log, chat or stream view follows new content while the user is at the bottom and stops following when they scroll up; a refresh keeps the user's scroll position; a newly selected or focused item is scrolled into view.
- **No capture.** When the baseline says `ui-capture: none`, or the slice changes a state no captured state reaches, read the changed views, handlers and bindings for the same questions. A finding from source alone carries `grade = medium` at most.

Every finding's evidence cites the capture it rests on: the state's file under `ui-capture/`, with the image region, `geometry.json` locator or grid row and column range, beside the handler or component that produces it. A defect's `failure_scenario` names the state or input sequence and what the user sees or cannot do.

## Does not flag

- A confirmation. A state or input path you judged correct goes in the cell's coverage note, never in a finding.
- Size, clipping, overflow, overlap, alignment and off-style controls in a state that otherwise behaves, which are the ui-visual lens's.
- Whether the UI is the interaction the user chose, which is the intent-fidelity lens's.
- A state the code makes unreachable: show the guarantee rather than assume it.
- A key binding or focus order the project documents as deliberate, or one the platform or toolkit sets as its convention. A disagreement with it is advisory with `angle = challenge`, naming the decision.
- A state that renders correctly but has no test, which is the test-gaps lens's.
- Work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.

## Edit forms

interaction writes defects first; its cleanup is an edit that leaves every state and input behaving the same.

- `change` — a handler, binding or state flag named against what it does, where the behaviour is right.
- `remove` — a state branch or handler no input or transition reaches, cited with the search.
- `add` — an accessible name or label on a control whose behaviour is already right.

## Advisory angles

- `risk` — a reachable state no capture shows, or an image state captured with no geometry key, where a feedback defect cannot be ruled out; name the state that would settle it.
- `challenge` — a documented interaction decision that leaves a user without feedback or stuck in a captured or traced state.

## Severity calibration

- `critical` — a user stuck with no way forward: a load that never ends, focus trapped, an action the keyboard cannot reach on a keyboard-only UI, or input that acts on the wrong item.
- `major` — a missing or misleading empty, loading, error or disabled state in a common flow, or focus or scroll lost on a common action.
- `minor` — weak feedback in a rare state, or a keyboard or pointer rough edge with a working alternative.
