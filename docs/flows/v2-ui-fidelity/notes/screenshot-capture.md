Verdict: The contract is one project command that writes `<out>/current/<state>.png` from a data manifest of `{name, steps, viewport/clip}`, and reportdesignkit already ships it (`scripts/visual-audit.mjs:186`, states at `scripts/lib/visual-audit-states.mjs:63`). lumina/web has no browser tooling to build one on. A Workflow-dispatched subagent can Read a PNG, but the Read tool downsampled a 2560x7440 capture to 886x2576.

```toml sources
read_at = "52fe01b9a17c76c7068e241fa891e6cacd6fcbc6"
dirty = []

[[source]]
path = "../reportdesignkit/scripts/visual-audit.mjs"
line = 186
claim = "captureState: runs a state's steps, settles, page.screenshot with a clip, writes <out>/current/<name>.png, and diffs it against a tracked baseline"

[[source]]
path = "../reportdesignkit/scripts/visual-audit.mjs"
line = 382
claim = "Playwright's own chromium, with a pinned context: 1280x900 viewport, deviceScaleFactor 1, reducedMotion reduce, colorScheme light"

[[source]]
path = "../reportdesignkit/scripts/visual-audit.mjs"
line = 76
claim = "CLI contract: --update, --only <state>, --out <dir>, --keep. Exit 1 on FAIL or MISSING, 2 on usage error or READY-TIMEOUT"

[[source]]
path = "../reportdesignkit/scripts/lib/visual-audit-states.mjs"
line = 63
claim = "The STATES manifest is data: { name, steps, clip, budget }. Steps accumulate from state to state"

[[source]]
path = "../reportdesignkit/scripts/lib/out.mjs"
line = 25
claim = "The run output lands in .rdk/visual (current/, diff/, report.md). Baselines are tracked under scripts/visual-audit/baselines"

[[source]]
path = "../reportdesignkit/package.json"
line = 65
claim = "Wired as `bun run visual-audit`. The pins are vitest 5.1.0, @vitest/browser-playwright 5.1.0 and playwright 1.65.1, with no @playwright/test"

[[source]]
path = "../reportdesignkit/vitest.config.ts"
line = 230
claim = "The browser project is headless Chromium through the Playwright provider, collecting *.browser.test.tsx component tests"

[[source]]
path = "../reportdesignkit/packages/testing/src/test-pixels.ts"
line = 26
claim = "Existing page.screenshot({save:false}) pixel read-back. The header says page.screenshot intermittently HANGS, so take one capture per state"

[[source]]
path = "lumina/web/package.json"
line = 1
claim = "lumina/web has no vitest, @vitest/browser, playwright or @playwright/test. Its tests are bun test"

[[source]]
path = "lumina/web/CLAUDE.md"
line = 31
claim = "Vue SFC rendering is out of scope for bun test. Adding Vitest is named as the path for component rendering"

[[source]]
path = "lumina/web/vite.config.ts"
line = 43
claim = "The dev server proxies /api (including WS) to the axum server on 127.0.0.1:24817, so capturing the real app needs the lumina server running"
```

## Answer

**Minimal contract.** A gate needs exactly three things from a project:
- **One command.** Exit 0 means captured, and non-zero means it never got to capture. It accepts `--out <dir>` and `--only <state>`.
- **An output directory.** It holds `<out>/current/<state>.png` for each state, plus a one-line-per-state report.
- **A manifest of states as data.** Each state is `{name, steps, viewport, clip?}`.

The capture driver pins these settings:
- viewport
- `deviceScaleFactor: 1`
- `reducedMotion: "reduce"` and `colorScheme`
- a transition-kill sheet
- `document.fonts.ready` and two rAFs before the shot

The gate needs nothing beyond that. Baselines and diff budgets are optional, and only a pixel-regression use needs them. reportdesignkit's `bun run visual-audit` meets this contract, with two gaps:
- It has one hard-coded viewport, 1280x900 at `scripts/visual-audit.mjs:388`, so a per-state viewport would be a new field.
- A missing baseline exits 1 at `scripts/visual-audit.mjs:425`. A gate that only wants the PNGs would read that exit as failure, so it needs a capture-only mode, or it has to ignore `MISSING`.

**`@vitest/browser`'s `page.screenshot`** (5.1.0):
- It shoots the test iframe or one element of a mounted component, not the running app.
- Options are `element`, `path` (relative to the test file, defaulting to `__screenshots__/<file>/<test>.png`), `save` and `base64`.
- `page.viewport(w,h)` resizes the iframe.
- The config sets `screenshotDirectory`, and `toMatchScreenshot` exists.

It suits per-component states. It does not suit whole-page states that need routing, a server or the sidecar. reportdesignkit uses it only for pixel read-back, and that file's header records intermittent hangs.

**Playwright test.** It is not installed in either project. reportdesignkit has the `playwright` library and drives it directly.

**lumina/web.** It has no browser tooling at all: bun test, no Vitest and no Playwright. A capture command there would add a dependency. It would also need the axum API on :24817 behind the Vite proxy, or a fixture backend.

**Read tool.** I am a `plan-investigator` running inside the `plan-v2-investigate` Workflow, and I read two PNGs through Read:
- A 724x900 baseline came back at full size.
- A 2560x7440 full-page capture at DPR 2 came back as `displayed at 886x2576`, scaled down by 2.89. 11px text stayed legible after the scaling, but fine detail is lost at that factor.

So captures should be per state, clipped to the viewport, at DPR 1, not full-page.

## Findings

- **Library/API or file**: `../reportdesignkit/scripts/visual-audit.mjs:186` `captureState`, with `scripts/lib/visual-audit-states.mjs:63` `STATES`
- **Source**: file:line, read in full
- **Evidence-grade**: high
- **Finding**: reportdesignkit already implements the command, the output directory and the state manifest. The gate contract can be modelled on it rather than invented.
- **Details**:
  - The command is `bun run visual-audit [--update] [--only <state>] [--out <dir>] [--keep]`. It starts the sidecar and the dev server itself and writes `.rdk/visual/current/<state>.png`, `diff/` and `report.md`.
  - Each state is `{name, steps, clip, budget}`, and the steps accumulate from one state to the next.
  - The context is pinned at `:387-392`, and `settle()` at `:156` kills transitions and waits for fonts.
  - Exit codes: 1 for FAIL or MISSING, 2 for a run that never captured.
- **Counter**: Two things would invalidate this. One is the planner wanting per-viewport states, which this script cannot express without a manifest field. The other is a gate that reads exit 1 from a no-baseline run as a capture failure.
- **Impact on plan / report**: Define the contract as `{command, out_dir, states[{name, viewport, steps?}]}`. Make the gate read `<out>/current/*.png`. A reportdesignkit task would add `viewport` to its state manifest and a capture-only exit path.

- **Library/API or file**: `@vitest/browser` 5.1.0 (reportdesignkit lockfile pin via `@vitest/browser-playwright` 5.1.0), `context.d.ts` `BrowserPage.screenshot` / `ScreenshotOptions`
- **Source**: installed types at `node_modules/.bun/@vitest+browser@5.1.0+568ca518c9482d53/node_modules/@vitest/browser/context.d.ts:29-48,936-956`
- **Evidence-grade**: high
- **Finding**: `page.screenshot` captures the test iframe or one element, which makes it a per-component capture route, not an app-state route.
- **Details**:
  - The options are `element`, `path` (relative to the test file, default `__screenshots__/${testFileName}/${testName}.png`), `base64` and `save`.
  - `viewport(width,height)` resizes the iframe.
  - The provider config takes `screenshotDirectory` and `screenshotFailures`, and `toMatchScreenshot` exists with path resolvers.
  - `packages/testing/src/test-pixels.ts:5-8` records that `page.screenshot` "intermittently HANGS".
- **Counter**: A Vitest browser test can mount a whole routed app with a mocked backend. If a project did that, this route could reach app states too. Neither project does that today.
- **Impact on plan / report**: Do not make `@vitest/browser` the contract's mechanism. A project may use it behind the command, but the gate should see only the command and the PNG directory.

- **Library/API or file**: `lumina/web/package.json` (no browser test dependencies), `lumina/web/vite.config.ts:43`
- **Source**: file:line, plus `ls node_modules`, which showed no vitest or playwright
- **Evidence-grade**: high
- **Finding**: lumina/web has no capture harness. Adopting the contract there means adding a dependency and a capture script, and solving the backend the app needs. `needs-plan`.
- **Details**:
  - The tests are bun test only, and `CLAUDE.md:31` puts Vue component rendering out of scope.
  - The Vite dev server proxies `/api` and WebSockets to axum on :24817, so a real-app shot needs the lumina server and a seeded database.
  - The browser builds already sit under `ms-playwright` (chromium-1194..1255), so only the npm package is missing.
- **Counter**: Two things would cut this down: a lumina fixture or demo mode that serves the SPA with canned API data, or a decision that lumina is out of this plan's scope.
- **Impact on plan / report**: Treat lumina/web as a second, heavier adopter, or defer it. The dependency choice needs the supply-chain lens.

- **Library/API or file**: Claude Code Read tool, from a Workflow-dispatched subagent (`plan-investigator`, tools line `claude/agents/plan-investigator.md:4` includes Read)
- **Source**: Read of `../reportdesignkit/scripts/visual-audit/baselines/canvas-fit.png` (724x900, shown whole). Read of a scratch 2560x7440 PNG made with Playwright 1.65.1 at DPR 2 with `fullPage`, which the harness labelled "displayed at 886x2576. Multiply coordinates by 2.89"
- **Evidence-grade**: high
- **Finding**: A Workflow subagent can see a PNG through Read, but large images are downscaled. The longest side observed was capped near 2576 px.
- **Details**:
  - The run gave two data points: a 724x900 image came through unscaled, and a 2560x7440 image was scaled 2.89x.
  - The exact cap rule is unconfirmed: it may be a long-edge limit or a total-pixel limit.
  - At DPR 1, a 1280x900 viewport shot is very likely shown at or near full size.
- **Counter**: A probe at 1280x900 and at 1920x1080, DPR 1, would settle the rule. If either came back downscaled, the per-state size budget would have to be smaller.
- **Impact on plan / report**: The contract should mandate DPR 1 and viewport-sized or clipped shots, never `fullPage`. A critic agent needs only Read in its tools.

Out of lens: the critic-reliability note's DOM-geometry checks (overflow, clipping) can run in the same Playwright page before the shot in `captureState`. That would cost no second harness.

Coverage: read in full `reportdesignkit/scripts/visual-audit.mjs`, `scripts/lib/visual-audit-states.mjs` and `packages/testing/src/test-pixels.ts` (head). From `reportdesignkit/vitest.config.ts`, read the browser project. In lumina, read `lumina/web/package.json`, `vite.config.ts` and `CLAUDE.md`, plus the installed `@vitest/browser` 5.1.0 `context.d.ts`. Checks run: pinned versions from `node_modules`, ran a Playwright capture into the scratchpad, and made two Read-tool image probes. Ruled out:
- `@playwright/test` `toHaveScreenshot`: not installed in either project, so it was not researched further.
- `toMatchScreenshot`: present in 5.1.0 but used nowhere in reportdesignkit.
