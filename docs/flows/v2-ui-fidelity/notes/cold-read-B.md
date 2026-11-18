Verdict: a worker could carry out tasks 32, 33 and most of 34 as written, but 4 points stop the milestone: one design gap in tasks 36 and 37 that defeats their own stated outcome, one passage task 37 leaves stale, one exit-code choice left open in task 34, and no direction citation in any task. No false premises.

```toml sources
read_at = "ba73780bb476497ef3c43d5807c9ce243dd5d16e"
dirty = [".claude/flows/v2-ui-fidelity/tasks.toml", "docs/flows/v2-ui-fidelity/design/ui-B.md", "docs/flows/v2-ui-fidelity/design/ui-B/single-table.txt", "docs/flows/v2-ui-fidelity/design/ui-B/stacked-rows.html"]

[[source]]
path = "claude/workflows/implement-v2-visual.js"
line = 152
claim = "the inspector prompt prints each state as '- <state> (control: <bool>)', as task 36 and D37 say"

[[source]]
path = "claude/workflows/implement-v2-visual.js"
line = 52
claim = "RETURN_SCHEMA requires state, control, verdict, defects, note"

[[source]]
path = "claude/agents/ui-inspector.md"
line = 38
claim = "Judge-each-state pass 3 selects look entries by their `states` list, a passage task 37's detail does not name"

[[source]]
path = "claude/agents/ui-inspector.md"
line = 19
claim = "the input list gives each state a control flag (task 37 premise holds)"

[[source]]
path = "claude/skills/ui-capture/references/defects.md"
line = 119
claim = "Controls paragraph says 'the judge says so' (task 37 premise holds)"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 124
claim = "a control is sound when its verdict is fail or unsure, whatever the defect class"

[[source]]
path = "claude/skills/ui-capture/scripts/render.sh"
line = 54
claim = "file_url percent-encodes '#' (task 32 premise holds)"

[[source]]
path = "claude/skills/ui-capture/scripts/render.sh"
line = 16
claim = "usage line 'render.sh [--size WxH] <html> <out-dir> <state>' (task 34 premise holds)"

[[source]]
path = "claude/skills/ui-capture/scripts/render.sh"
line = 135
claim = "render.sh exits 2 when no browser is found, 1 on render or probe failure"

[[source]]
path = "docs/flows/v2-ui-fidelity/design/ui-B/single-table.txt"
claim = "24 lines of 80 cells; ID cols 2-3, glyph col 6, TITLE from col 10, FILES ending col 77, hint count ending col 77, as design/ui-B.md#terminal-panel says"

[[source]]
path = "docs/flows/v2-ui-fidelity/design/ui-B/stacked-rows.html"
line = 6
claim = "the mock's panel max-width is 480px, matching ui-B.md and task 32, not D35's '400 px'"

[[source]]
path = ".claude/flows/v2-ui-fidelity/tasks.toml"
line = 1119
claim = "task 32's look entries name only web states; task 33's (line 1190) name only tui states"
```

## Unactionable

### 1. Tasks 36 and 37: a control carrying every look of milestone B carries the other surface's looks, so it fails without the inspector catching anything and is recognisable as a control

Task 36 detail: "A control carries the number of every look in `a.looks`." Task 37's new `## Controls` text: "A look a state carries that its capture cannot show, such as a terminal criterion on an image, is answered `fail` with that reason, never skipped."

`tomlctl plan emit gate-list --slug v2-ui-fidelity --milestone B` prints four looks: two from task 32 naming only `web-*` states, two from task 33 naming only `tui-*` states. Under task 36 a clean web state's line reads `looks 1, 2`, a clean tui state's `looks 3, 4`, and every control, web or tui, reads `looks 1, 2, 3, 4`. Two results follow:

- Any state carrying the other surface's looks is identifiable as a control from its prompt line alone. That contradicts task 36's Outcome ("An inspector cannot tell from its prompt which of its states are planted controls") and D37/D39's intent.
- Under task 37's rule, every control fails on the looks its capture cannot show (a web PNG judged against "every column and the right border hold their cells"), whether or not the inspector saw the planted defect. visual.md:124 counts any `fail` on a control as sound, so the catch rate D7 asks for would read 9/9 whatever the inspector does.

A worker who follows the detail produces a prompt that breaks the outcome. Fixing it means deciding something the plan does not settle, for example a control carrying only the looks of its own surface, or of looks grouped by surface. That is a design call on D39, which is for the planner to make.

### 2. Task 37: "Change only the passages the detail names" leaves ui-inspector.md's look pass referring to a field the prompt no longer carries

ui-inspector.md:38 (Judge each state, pass 3) reads "For each look entry whose `states` names this state". The task 36 prompt prints looks without `states`, and the state line carries the look numbers instead. Line 47 (`pass` verdict) also says "every look entry naming the state". Task 37's detail lists the input list, `## Controls`, the note's first line, the return shape and the description, but not these passages, and its constraint forbids touching any other passage. So the worker has two options, both bad: leave a stale instruction that the inspector cannot follow, or break the constraint.

### 3. Task 34: the exit code when states fail with different codes is unsettled

Detail: "carry render.sh's non-zero exit through, exit 2 included (no browser), naming the state it failed on. Capture every asked state before exiting, so one failure does not hide the others." render.sh exits 1 on a render or probe failure and 2 on bad usage or no browser (render.sh:5, :135, :207-211). When one asked state ends in 1 and another in 2, the plan does not say whether capture.sh exits with the first code, the last or the highest. The accept checks do not decide it either.

### 4. Tasks 32, 33, 34, 36, 37: no direction citation

None of the five tasks cites a section of `design/direction.md`. 32, 33 and 34 cite `design/ui-B.md` sections, and 36 and 37 have no design field at all. `docs/flows/v2-ui-fidelity/` holds only `brief.md`, `design/` (ui-B.md and ui-B/) and `notes/`, with no `design/direction.md` for them to cite. This is a plan-level gap counted once. The specs themselves are not generic: they follow the flow's decisions closely.

## False premises

None. Every premise probe holds at HEAD:
- `s/#/%23/g` is at render.sh:54.
- The usage line is at render.sh:16.
- The mock and the grid exist.
- `(control: ${u.control})` is at implement-v2-visual.js:152, and the `required` list is at :52.
- `` each with `control` true or false `` is at ui-inspector.md:19.
- `the judge says so` is at defects.md:119.

D37's line citations (visual.js:152, ui-inspector.md:19 and :53) are accurate.

Observation, not counted: D35's text says the panel width runs "up to 400 px", but design/ui-B.md and the mock (stacked-rows.html:6, `max-width: 480px`) say 480. Task 32 settles it explicitly ("max width at 480 px as the design file says"), so a worker is not left to guess. The decision record should still be corrected.

## Check commands (present tree, before any change)

- 32 `f=...; test "$(grep -c . $f/states.txt)" -eq 11 && while read ...`: exit 2 (`grep: ... states.txt: No such file`, `test: : integer expected`). It fails for the missing change. It discriminates only weakly, since `web-01` is always in the mandated `<html>` tag, but it does need web-02 to web-11 present.
- 33 `python -I -c "...tui..."`: exit 1 (no grids). Positive control: the same length logic on design/ui-B/single-table.txt gives 24 lines, all 80 cells, so the check accepts a well-formed grid.
- 34a `d=$(mktemp -d) && ... capture.sh --list ... --only web-04 --only tui-04 ...`: exit 1 (`capture.sh: No such file or directory`). The browser path it needs works: `bash claude/skills/ui-capture/scripts/render.sh --self-test` prints `self-test: pass`, exit 0.
- 34b `test -f capture.sh && ! bash capture.sh --out ... --only no-such-state`: exit 1. It is guarded by `test -f`, so it discriminates.
- 34c `grep -qF '...capture.sh' .claude/ui-capture.toml && git check-ignore -q .ui-capture/probe`: exit 2 (file missing).
- 34's declaration commands: `--dry-run` against the absent `.claude/ui-capture.toml` errors `reading ... os error 2`, so a dry run cannot preview them. Run for real in a scratch directory outside git, both `tomlctl set ... --set command=... --set out=.ui-capture --no-write-integrity` and `tomlctl set-json ... paths --json '[...]' --no-write-integrity` exit 0 (`"created":true`). They write `command`, `out` and `paths` as intended, with no sidecar.
- 36a `! grep -qF '(control: ' implement-v2-visual.js`: exit 1.
- 36b `node --test --test-reporter=tap --test-name-pattern='a control carries every look' ... | grep -q '^ok [0-9]* - implement-v2-visual: a control carries...'`: exit 1. Positive control: the same form with pattern `ten states` prints `ok 1 - implement-v2-visual: ten states and two controls make three groups...`, so the TAP line shape matches. Node is v26.12.1.
- 36c `node --test claude/workflows/tests/`: 30 pass, 0 fail today (guard).
- 37a `! grep -qF '"control"' ui-inspector.md`: exit 1 (line 64 holds `"control": false`).
- 37b `! grep -qF 'each with `control` true or false' ... && grep -q 'not told which' defects.md`: exit 1.

Not run: 37's `cargo test ... cli::dispatch::tests` (a whole-lib test build, left to the orchestrator).

Coverage: read the worker views and `tasks show` file lists for tasks 32, 33, 34, 36 and 37, and the files they rely on:
- render.sh (whole)
- implement-v2-visual.js (whole)
- visual.test.mjs (lines 1-165 and test names)
- ui-inspector.md (whole)
- ui-capture SKILL.md (declaration through Controls)
- defects.md `## Controls`
- implement-v2-engine/references/visual.md (headings and control lines)
- design/ui-B/single-table.txt
- stacked-rows.html (size rules)
- the milestone's look entries in tasks.toml
- `plan emit gate-list` for B

Not checked: whether the web layout numbers (eight rows plus a notice within 560 px) hold once rendered, which only building panel.html settles.
