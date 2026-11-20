# UI layout

The layout step for a milestone that adds a UI surface or changes the layout of one. It runs in two places, both before any row of the milestone is written or revised: in Design, once the milestones are cut ([write-and-prove.md, section 1](write-and-prove.md#1-design)), and in elaboration's Surface and decide ([elaborate.md, section 2](elaborate.md#2-surface-and-decide)). `<slug>` is the flow, `<milestone>` the milestone's id and `<scratch>` the session scratchpad, as in those files. The user picks among concrete layouts, shown as pictures or as text grids, and the pick reaches the workers through a bound decision and a design file, and the inspector through the same design file and the milestone's capture states.

Only this conversation asks the user, so nothing in the step is delegated. The capture contract the states below feed, and the defect rubric the inspector judges them by, are the `ui-capture` skill's; read its SKILL.md before naming states.

## 1. Judge which milestones are UI

A milestone is UI when its rows add a surface the user sees or change the arrangement of one: a new view, panel, dialog or screen, a region moved, split or resized, a list that becomes a grid. A fix inside an arrangement that stays as it is, such as a colour, a label, an off-by-one or a style token, is not UI here, and gets no layout round.

Judge from what the rows will change, never from file types alone, since the step is stack neutral. A milestone whose files touch the `paths` of the project's `.claude/ui-capture.toml`, or a web or QML file, is a candidate on any stack: a web page, a QML or Quickshell window and a terminal UI such as ratatui all count. A candidate whose change leaves every arrangement as it was is judged not UI.

In a project with no `.claude/ui-capture.toml`, read the decision Open recorded on whether the project has a UI to capture ([open-and-challenge.md, section 1](open-and-challenge.md#1-open)), the one whose text opens `UI to capture:` in `tomlctl plan view --slug <slug> --for human`, and do not ask it again. A no judges every milestone not UI, citing that decision, unless a milestone adds a surface the user sees; such a milestone goes in the list below as UI, and the question says the Open answer looks wrong. A plan opened before Open asked has no such decision: ask it here once and record it the way Open does.

List every milestone with the call you made, UI or not UI, and one line of why each, and ask the user through `AskUserQuestion` whether the list stands, with "as listed" first. A milestone the user moves is judged their way. When no milestone is UI and the user agrees, the step ends here and records nothing.

## 2. Draft the options

For each UI milestone, find its surfaces: each screen, panel or window its rows add or reshape. Per surface, draft two or three layouts that differ in arrangement: which regions exist, where they sit, what scales with the window and what stays fixed. Variants of one arrangement in another palette or spacing are one option, not two. When a recorded user stance already fixes the layout, such as a design-language document the project declares or an earlier pick, follow it and draft no options for that surface; the stance is the decision, recorded as section 4 says with its provenance.

Style each option after the project, read from its own source: its colour and spacing tokens, fonts, control style, density and existing views. An option that looks like a generic template hides the questions the user needs to see.

Name each option with a short kebab-case name, such as `split-list` or `stacked-cards`. The name is the file stem below, the question's option label and the decision's wording, so it is the same in all three.

**Web and QML.** Write each option as one standalone HTML file under `<scratch>`, with inline styles copied from the project's tokens and no network fetches. A QML or Quickshell surface is mocked in HTML the same way, after the shell's own look. Render each to a PNG with the `ui-capture` skill's renderer, at the size the surface is meant to run at:

```bash
bash <skill-dir>/scripts/render.sh --size <width>x<height> <scratch>/<option>.html docs/flows/<slug>/design/ui-<milestone> <option>
```

`<skill-dir>` is `ui-capture` under the Claude config dir's `skills/`, or `claude/skills/ui-capture` in this repo. The run writes `docs/flows/<slug>/design/ui-<milestone>/<option>.png` and the option's key in `geometry.json` beside it. Read both: an entry in `geometry.json` is a defect in the mock, so fix the mock and render again before the user sees it. A surface whose window size matters gets a second render of each option at its narrow or short extreme, named `<option>-narrow` or `<option>-short`. Exit 2 means no browser was found; say so to the user and show that surface's options as text grids only, as below.

**Terminal UI.** Draw each option as a text grid at the terminal size the UI targets, one line per row, only the characters drawn, and save it as `docs/flows/<slug>/design/ui-<milestone>/<option>.txt`. Nothing here needs a browser or an image: the grid is the picture, and it goes into the question's preview as it is.

**The Artifact page.** When this session holds the Artifact tool (run `ToolSearch` for it before concluding it is absent), publish one Artifact page per milestone showing every option of every surface side by side, each under its option name, built from the mocks or the PNGs, with each terminal option's grid in a monospace block. Name the page in the question, so the user looks at it before picking. Without the tool, name the PNG paths instead, since a terminal question shows no image.

## 3. Ask

Ask through `AskUserQuestion`, one question per surface, in batches of up to four. Each option carries a `preview`: an ASCII drawing of its arrangement for a web or QML option, the grid itself for a terminal one. The option you would choose goes first, and the question says why in one line. A layout is visible to the user, so it passes Surface's first test; it is asked here rather than in Surface because the milestones it hangs on do not exist until Design cuts them.

An answer that is a different layout, or a change to one of the options, is drawn as a new option, rendered or saved like the others, and asked again; the step never assumes what the user meant. A question an answer opens is asked in a further round until none is open.

## 4. Record the pick

**The decision.** Record each surface's question before asking it, as a decision of class `look`, `surfaced = true`, with the option you put first as `decision` and every other option in `rejected`, each with the fact that sinks it. Then apply the answer as [open-and-challenge.md, section 5](open-and-challenge.md#5-surface) says: the default confirmed is `user-confirmed`; another option or the user's own is `user-stated` with `vetoed = true`, `decision` rewritten to it and `rejected` rewritten to hold the rest. A layout is usually a `two-way` door, since it can be redrawn before the rows are built; make it `one-way` when undoing it would change the data or the API behind the surface.

```toml
[[record]]
decision = "Lay the settings panel out as split-list: the section list fixed at 220 px on the left, the form scaling to the rest."
why = "The form is the part that grows; the section list holds at most a dozen short names."
provenance = "agent-assumed"
door = "two-way"
class = "look"
binds = ["docs/flows/<slug>/design/ui-<milestone>.md"]
rejected = [{ option = "stacked-cards", because = "a dozen sections stacked push the form below the fold of a short window" }]
evidence = "read the existing views and the project's spacing tokens; rendered both options at 1280x800 and 1280x560"
if_false = "the rows build stacked-cards and the inspector judges against it"
surfaced = true
```

```bash
tomlctl plan decide --slug <slug> --ndjson <scratch>/layout-decide.toml
```

`binds` names the design file below, which exists once written. The decision is bound to the milestone's UI rows by their ids: in Design, after Write lands them ([write-and-prove.md, section 3](write-and-prove.md#3-write)); in elaboration, now, since the rows exist. A bound decision is printed in each of those rows' worker view.

**The design file.** Write `docs/flows/<slug>/design/ui-<milestone>.md` with the Write tool, one `##` section per surface. Each section names the chosen picture or grid by path (`ui-<milestone>/<option>.png`, `ui-<milestone>/<option>.txt`, and any extreme renders), names the options it was chosen over and the decision's id, and states the constraints the pick implies:

- sizes: what is fixed in pixels or cells, and the window or terminal sizes the surface must work at;
- what scales with the window and what stays fixed, and what happens to dead space;
- how each content extreme lays out: empty, one item, many, a long label.

The inspector judges arrangement against this section, so a constraint not written here is not judged. Each UI row cites its surface's section in `spec.design` as `design/ui-<milestone>.md#<anchor>`, the anchor slugged from the heading by the task store's ref rule, and the worker view inlines that section's text. A worker holds Read and opens the picture the section names.

Every file this step writes under `docs/flows/<slug>/design/` is committed with the plan, the rejected options' pictures and the mocks' `geometry.json` included: approval refuses an uncommitted one, and elaboration's commit stages what this step wrote.

## 5. Declare the states, the controls and the look entries

**States.** Name the capture states the milestone's visual pass judges, by the `ui-capture` skill's contract. Give every surface its content extremes, because dead space, a panel that grows with its container and a fixed-height panel show only there: empty, one item, many items, a long label, and a short and a tall window, or a short and a tall terminal. Where the project's capture command already exists, `<command> --list` prints the state names it knows; reuse those names, and give each new state a row of the milestone that teaches the command to reach it, the command's source in that row's `files`. A project with no `.claude/ui-capture.toml` whose Open decision says it has a UI gets a row that declares one, in the first milestone judged UI, since the visual pass has nothing to run without it; the step reads that decision from section 1 and asks nothing more about the config.

Name every state `<surface>-<n>`, as the `ui-capture` skill's Controls section sets out: one surface prefix per surface and a two-digit number. A name that describes the state leaks what it shows, so a control's name says nothing about its defect, and the control takes its own surface's looks.

**Controls.** Declare at least one control: a state carrying a defect planted on purpose, which the inspector must fail. A control is listed only as a control, never among the states a look entry names, and `plan check` refuses a milestone with states and no control (`plan/ui-controls-missing`). Choose a defect from the `ui-capture` skill's rubric that the milestone's own surfaces could plausibly have, and give it to the row that builds the states. At least one control plants a class the geometry probe does not measure (scaling, reserved empty space, panel size, misalignment, an off-style control or glyph, a terminal column out of line, or colour as the only signal), such as a 60 px band reserved for a toolbar that never renders: a clipped, overflowing or overlapping control fails on one `geometry.json` key and says nothing about the classes only the eye can judge. A control takes its surface's prefix and the next free number, never a name that describes the planted defect.

```bash
tomlctl plan milestone update --slug <slug> --id <milestone> --ui-state <state> --ui-state <state> --ui-control <control> --reason '<why>'
```

The flags given replace each whole list, so repeat every state. `--reason` is needed once the store is audited, which an elaboration always is. In Design, `plan milestone add` takes the same `--ui-state` and `--ui-control` flags when the step runs before the milestone is added.

**Look entries.** Each UI row gets at least one judged acceptance: an `accept` entry with `run_by = "look"`, the `states` it is judged on, drawn from the milestone's states, its `behaviour` the criterion the inspector judges, and no `check`:

```toml
[[record.spec.accept]]
behaviour = "the section list keeps its width and the form takes the rest, with no dead band below a short form"
run_by = "look"
states = ["settings-01", "settings-02", "settings-04"]
```

Write the behaviour from the design file's constraints, so the inspector judges what the user picked. A UI row whose acceptance is all read-only checks draws `plan/ui-accept-structural` from `plan check`, since a grep cannot see a layout; the look entry is what answers it.
