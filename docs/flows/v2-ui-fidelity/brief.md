# v2-ui-fidelity

## Intent

A UI built through the v2 chain comes out looking right and matching what the user meant, without the user acting as
the visual QA after the build. The pipeline sees the UI it builds, shows the user concrete layout options before tasks
are written, and asks the user the questions that need taste or carry architectural, cost or UX weight rather than only
the ones that change the plan's structure.

## Non-goals

- Project-side work in any consuming repo: a design-language document, a test-budget change, a screenshot fixture
  setup. This flow defines what a project declares; a project adopts it in its own flow.
- Golden-image or pixel-diff visual regression baselines.
- The v1 carriers (`/plan-new`, `/implement`, `/review`, `/review-plan`) and lumina.
- Enabling the Playwright MCP globally. The user disabled it on 2026-08-18 for its process cost.

## Scope budget

One flow in `dev-tools`, across seven areas, landed together in milestone A and proven in milestone B, since later areas build on earlier ones:

1. Visual verification: a UI milestone's gate captures declared UI states as images, and an agent judges them before
   the commit.
2. A design step for UI milestones: concrete layout options shown to the user, and the chosen one recorded, before the
   milestone's tasks are written.
3. Surfacing: the questions planning and elaboration put to the user, how many, and how they are framed.
4. Acceptance for user-visible behaviour: what a UI task's `accept` may and may not rest on.
5. Review: lenses that judge look, interaction, and fidelity to the brief and the user's stated decisions.
6. Questions during the build: a worker that meets a compromise or a hard-to-unwind choice asks the user through the
   orchestrator while the milestone runs (decisions D9 and D11).
7. Direction: an interview before the brief and a choice among directions before Design, for every plan, so each
   task carries the flow's vision (decisions D31 and D32).

## Success criteria

- When a UI milestone is gated, an agent looks at captures of its declared states (screenshots, or text grids for a terminal UI) before the commit. A defect of the
  kinds below fails the gate or reaches the user in the milestone's report, without the user opening the app: an
  element that scales with its container where it should not, a band of reserved empty space, a panel taller or
  shorter than its content needs, content clipped or overflowing, a control in a style the project does not use.
- Before the tasks of a milestone that builds or reshapes a UI surface are written, the user is shown at least two
  concrete layout options as pictures, or as text grids for a terminal UI, and picks one, and the workers and the visual check read the one picked.
- Planning and elaboration put to the user the choices that need taste or carry UX, cost, public-vocabulary or
  architectural weight, even when the plan's structure would not change. A one-way door is never assumed silently.
- A question whose options are narrowed by an engineering limit says what lifting the limit would cost.
- Planning goes through every ambiguous call that is hard to unwind or is a compromise with the user, at any level from
  the abstract to a task's specifics, and a question an answer opens is asked, never assumed (decision D6). The harness
  aims to build better than the user would specify, and holds consistency with the user's expectations as a key goal.
- Right after the brief is written, and again before Design, the user is given a short account of what they will see
  and do once the work lands, and can correct it.
- During the build, a compromise or a hard-to-unwind choice a worker meets is put to the user while the milestone runs,
  and is never decided silently; an unanswered one parks its task and is asked again at the next chance.
- No user-visible behaviour is accepted on a `grep` or file-existence check alone.
- A review over UI changes judges how the change looks and behaves, and a review can check the built work against the
  brief's success criteria and the user's stated decisions.
- A handover asking the user to review the look, or to reproduce a visual defect, is not minted where the milestone's captures could
  settle it.
- Every plan, with or without UI work, gets more questioning and exploratory discussion of its direction with the user
  before the work dives in, scaled to its complexity and ambiguity, so the flow has a clear direction and its tasks
  carry an overarching and task-specific vision rather than generic standalone implementations. Only the capture, the layout design step, the visual inspection and the other UI machinery
  are limited to flows with UI work; a plan with none runs no capture, design step or visual pass.
- Capture, judgement and review are stack agnostic: a web UI, a QML or Quickshell UI and a terminal UI such as ratatui
  each go through the same contract. UI detection never depends only on web file types: a project's declared capture
  paths mark a UI change on any stack (decision D26). A state captured as an image comes with geometry checks for
  clipping, overflow and overlap; a state captured as a text grid shows them in its text.

## Background

Seeded from the `package-design-tools` flow in `reportdesignkit` (milestones A to H, 2026-11-12 to 2026-11-17), read
on 2026-11-17 in `.claude/flows/package-design-tools/plan.toml`, `tasks.toml` and `docs/flows/package-design-tools/`
of that repo.

What that flow shows:

- **The user redesigned the UI after the build.** D37 to D62 are about twenty `user-stated` decisions from the user's
  reviews of 2026-11-16 and 2026-11-17: an ugly tab bar, an over-full head row, the preview's placement, +/- fold glyphs
  not in the project's style, the series editor ("the worst part"), adding a stop giving the same colour, a reserved
  "No contrast or coverage notices" line, swatches growing to fill the width, a panel always at full screen height, and
  a buggy autoscroll. None was asked before the build. Milestones E, F and G exist only to rework them.
- **Agent-assumed decisions the user later reversed.** D22 (agent-assumed, one-way) added use-case token names to the
  core vocabulary; D34 and D35 reversed it. D46 (agent-assumed) bound two series to neutral fallbacks; D63 reversed it
  with "The brief was that primary and alternate would be a normal generated colour sets". Variants were built into the
  editor though the user did not know what they were for (D40).
- **An engineering limit framed a UX choice as fact.** D30's fixed 480px editor box was confirmed on "the shell cannot
  measure content, so a form whose height is one declared constant is the only editor box". Later milestones made the
  forms measure their content; the box was reworked twice (D55, then D61).
- **The agents could not see their own defects.** D44 records the empty-space bug as not reproduced; handover H2 asked
  the user to observe it, and H1 to "review the look".
- **UI acceptance was structural.** Of 110 `accept` checks on tasks whose files include a `.tsx`, 52 are `grep` or
  `test -e` (for example "the editor draws no undo control" is a grep that finds nothing). The browser tests assert
  declared heights. D60 added no browser case because shell-react sat at its 562/562 test-budget cap. Milestone E took
  six gate rounds.

What the harness has today:

- **No eyes.** `plan-worker-lite` and `plan-worker-deep` hold no browser tool (`claude/agents/plan-worker-deep.md`
  frontmatter). `review-finder` and the research agents list `mcp__playwright__*` tools, but the Playwright MCP is
  disabled globally and opt-in per project `.mcp.json` (user CLAUDE.md). The only UI mention in `plan-v2-engine`,
  `implement-v2-engine` and the plan agents is the investigator's optional `browser-observation` reference, gated on a
  `DEV SERVER` URL.
- **Surface filters taste out.** `claude/skills/plan-v2-engine/references/open-and-challenge.md` section 5: zero
  questions is "the normal case"; at most three; test 1 requires "a different plan", which a look or layout choice never
  produces; test 3 lets a recorded stance answer a question; the default is put first and taking it is recorded as
  `user-confirmed`. `write-and-prove.md` section 2 (Surface again) and `elaborate.md` section 2 reuse the same tests and
  cap. The approval gate lists every `agent-assumed` decision (`approve.md`), 34 in the flow above.
- **Accept entries** have `kind` `forward`, `falsifier` or `guard` and `run_by` `worker`, `gate` or `covered`
  (`tomlctl/src/tasks/schema.rs`); `plan probes` checks each command's polarity. Nothing judges an image.
- **Handover reasons** are `restart`, `permissions`, `attended`, `observe`, `credentials` and `external`
  (`claude/skills/tomlctl/references/plan.md`, `plan handover set`). The "review the look" handover above used
  `attended`.
- **The review catalogue** (`claude/skills/review-v2-lenses/lenses/`) has 31 lenses; none judges UI, UX, layout or
  fidelity to the brief.

Standing user stances that bind this flow: the v2 pipeline is designed with the user before building and does not
carry v1 practice over by default; flow commands and agents stay generic, with project behaviour fixed at the project;
browser checks prefer `@vitest/browser` over the Playwright MCP; the user's colour and visual taste runs to
high-contrast black on white with colour only as touches.

## Unknowns

Nothing here is settled by this brief. A lens line stays on one line: it is the launch's question verbatim.

### To decide with the user

These are not lenses. Each is put to the user once investigation makes it concrete, and recorded as a decision.

- critic-placement: does an independent agent judge the screenshots at the gate, does each worker check its own UI
  task's screenshots before returning, or both?
- critic-verdict: does a visual defect fail the gate and mint fix tasks as a failed command does, or is it reported to
  the user at the milestone's end?
- capture-contract: does a project declare its UI states and capture command once (project config), per plan, or per
  milestone?
- design-medium: are layout options shown as ASCII previews in the question, as a rendered page, or as screenshots of a
  prototype built from the project's real components?
- design-trigger: which milestones get the design step: any that edits a UI file, only those that add or reshape a
  surface, or those the planner marks?
- question-budget: how many taste and UX questions per planning session and per elaboration are welcome before it
  becomes too chatty?

### Lenses

- screenshot-capture: what minimal project-side contract (a command, an output directory, a manifest of named states and viewports) lets a gate capture UI states as PNGs, what do `@vitest/browser` (`page.screenshot`) and Playwright test offer for it in the React and Vue projects under `~/dev` (reportdesignkit, lumina/web), and can a Workflow-dispatched subagent read a PNG through the Read tool?
- critic-reliability: how reliably does a vision model judge screenshots for layout defects (dead space, scaling, overflow, clipping, misalignment, off-style controls), what do published evaluations and practice say, and which rubric and comparison form (a chosen mock beside the capture, before and after pairs, content extremes) reduces false passes?
- surface-mechanics: where do the Surface, Surface again and elaboration tests, cap and record shape live across `plan-v2-engine`, the `tomlctl plan` decision schema (`provenance`, `door`, `surfaced`), `plan approve` and `plan check`, and what would a decision class for taste, UX, cost and vocabulary choices with its own question budget cost to add?
- accept-look: what would a judged `look` acceptance (an accept kind or `run_by` owner whose check is a capture plus a rubric rather than an exit code) cost across `tomlctl/src/tasks/schema.rs`, `plan probes`, `plan emit gate-list`, the `implement-v2-engine` gate and drain, the `verification` agent and the `cli::dispatch::tests` gates?
- design-step: where in `plan-v2-engine` (Design, `elaborate.md`) can a UI milestone's layout options be built and put to the user before Write, what can `AskUserQuestion` previews, an Artifact page and a rendered prototype each show, and where should the chosen design persist so workers and the visual check read it?
- review-ui-lenses: what do `ui-visual`, `interaction` and `intent-fidelity` lenses need in `claude/skills/review-v2-lenses/` (entry format, activation signals, the finder's tools given the Playwright MCP is opt-in), and how would `intent-fidelity` read a flow's brief and its `user-stated` decisions?
