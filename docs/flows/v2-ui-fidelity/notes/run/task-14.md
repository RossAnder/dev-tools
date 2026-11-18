Status: done — geometry-probe.js and render.sh written; a planted overflow and a clean page render correctly on Chrome, Edge and Playwright Chromium

```toml run-note
files_touched = [
  "claude/skills/ui-capture/scripts/geometry-probe.js",
  "claude/skills/ui-capture/scripts/render.sh",
]

[[deviations]]
what = "The probe pass sizes its window by measurement rather than passing --window-size=W,H unchanged: render.sh reads the viewport the probe saw and widens the window by the shortfall, up to three launches, failing if it never reaches WxH"
why = "Measured with Chrome 157 on Windows: --screenshot lays the page out at exactly WxH (800x600 PNG, fixed border fills it), but a page loaded for --dump-dom at --window-size=800,600 gets a 784x505 viewport, so geometry measured at the given size would not match the shot. Alternatives weighed: one launch with both --screenshot and --dump-dom (load-time scripts still see 784x505, so no better); a hardcoded frame offset (platform dependent)."
means = "detail: render with --window-size=W,H and extract the probe result from --dump-dom"

[[deviations]]
what = "The page-side driver writes the report at load and rewrites it from setTimeout, two animation frames and fonts.ready when fonts are still loading, instead of waiting for fonts and frames before probing once"
why = "Measured with Chrome 157: on a page carrying the probe, --dump-dom serialised the DOM before a fonts.ready callback or a requestAnimationFrame callback ran, even with --virtual-time-budget, so a single deferred probe returned nothing; Edge 157 did run them. Probing synchronously at load is accurate for layout because the probe forces style and layout itself."
means = "SKILL.md's web guidance: wait for fonts and two animation frames before measuring"

[[deviations]]
what = "PATH discovery also tries google-chrome, google-chrome-stable, chromium-browser and microsoft-edge, each after its family's name in the detail's order"
why = "Those are the executable names Linux packages install; without them a Linux machine with Chrome on PATH would fall through to the Playwright cache or fail"
means = "detail: chrome, chromium, msedge on PATH"

[[tangential]]
kind = "other"
area = "claude/skills/ui-capture/scripts/render.sh"
summary = "On this machine the newest Playwright Chromium (chromium-1255/chrome-win64) lacks chrome.dll and fails to start; discovery picks the newest chromium-*/ by number, so a broken newest install is chosen over a working older one"
why = "Only matters when no system Chrome or Edge is installed; render.sh then fails with the browser's own DLL error rather than trying an older build"

[[checks]]
command = "test -f claude/skills/ui-capture/scripts/render.sh && grep -q -- '--self-test' claude/skills/ui-capture/scripts/render.sh && grep -q 'scrollWidth' claude/skills/ui-capture/scripts/geometry-probe.js"
result = "pass"

[[checks]]
command = "node --check claude/skills/ui-capture/scripts/geometry-probe.js"
result = "pass"

[[checks]]
command = "bash -n claude/skills/ui-capture/scripts/render.sh"
result = "pass"

[[checks]]
command = "bash claude/skills/ui-capture/scripts/render.sh --self-test"
result = "not-run"
```

## What changed

**`claude/skills/ui-capture/scripts/geometry-probe.js`** (new). A dependency-free browser script defining `window.uiCaptureGeometry(root?)`, which returns one state's `geometry.json` array of `{kind, locator, detail}`:

- `overflow`: the document is wider than the viewport, or an element's `scrollWidth`/`scrollHeight` exceeds its client size on an axis whose overflow is `visible`.
- `clipping`: the same excess on an axis with `overflow: hidden` or `clip`. Single-line text cut with `text-overflow: ellipsis` is skipped, matching the rubric's truncation class. Scroll containers (`auto`/`scroll`) are not reported.
- `offscreen`: a box partly or wholly left of, right of, or above the viewport. Only the outermost box is reported, a box inside a clipping or scrolling ancestor is skipped, and so are visually hidden boxes (1 px, `clip: rect(0…)`) and boxes moved wholly offscreen by absolute or fixed positioning (skip links).
- `overlap`: two elements' own text line boxes, or replaced elements' boxes, crossing by more than 2 px each way. Ancestor and descendant pairs are skipped, and only boxes in the same absolute or fixed positioning layer are compared, so menus, tooltips and badges over the page are not reported.

Locators are CSS selectors: a unique id, or up to five `tag.class:nth-child(n)` segments from `body` or the nearest unique-id ancestor.

**`claude/skills/ui-capture/scripts/render.sh`** (new). `render.sh [--size WxH] <html> <out-dir> <state>` and `render.sh --self-test`. It finds the browser in this order: `$UI_CAPTURE_BROWSER`, then PATH names, then Windows and macOS install paths, then the newest `chromium-*/` in the Playwright cache (`PLAYWRIGHT_BROWSERS_PATH`, `%LOCALAPPDATA%`, `~/Library/Caches`, `~/.cache`). When none is found it exits 2. It takes the screenshot with `--headless=new --hide-scrollbars --force-device-scale-factor=1 --window-size`, using a throwaway profile, reduced motion and a virtual-time budget, and passes drive-letter paths through `cygpath -m` under Git Bash. For the geometry pass it writes a copy of the page beside the original, so relative URLs still resolve, and appends the probe, a driver script, the state name and the existing `geometry.json`. A trap removes the copy. It then runs `--dump-dom` and lifts the merged report from an attribute on `<html>`. The report is ASCII-escaped, with its newlines carried as U+0001, so it serialises as one line. render.sh decodes the attribute entities and writes `geometry.json` pretty-printed, keeping the other states' keys. A malformed existing file fails with the parse error. Exit codes: 0 success, 1 render or probe failure, 2 bad usage or no browser.

`--self-test` renders a clean card page and a page whose `#planted-label` overflows its 200 px card. It fails unless both PNGs exist, `"clean": []` is present, `"planted"` has a key, and an `overflow` entry names `planted-label`.

## Verification

The `--self-test` accept check belongs to the gate, so it was not run here. I reproduced its two pages by hand under the scratchpad directory instead:

- **Chrome 157, clean page:** `{"clean": []}`.
- **Chrome 157, planted page:** `overflow` on `div.card` and `#lab`, with `scrollWidth 276 > clientWidth 200`. The PNG header reads 800x600, and a second state merged into the same file.
- **Mixed page with one of each class plus deliberate non-defects:** the document overflow, `#clip` clipping, `#off` offscreen, the negative-margin `#a | #b` overlap and a float spilling its container were reported. The ellipsis, scroll region, skip link, sr-only span and absolute tooltip were not. Edge 157 (through `UI_CAPTURE_BROWSER`) gave the same five entries.
- **Playwright Chromium 1194, clean page:** empty.
- **Error paths:** `--size 80x` and an unusable `UI_CAPTURE_BROWSER` both exit 2 with one message each.
