Verdict: Put the layout step inside Design after the milestones are cut (write-and-prove.md:49) and in elaboration's Surface and decide (elaborate.md:49), asked before Write; in this user's terminal CLI AskUserQuestion previews are monospace markdown (ASCII only), so pictures need an Artifact page or prototype PNGs; persist the pick as a `user-stated` decision plus `docs/flows/<slug>/design/ui-<milestone>.md` cited by each UI task's `spec.design` (view.rs:529), with its PNGs beside it.

```toml sources
read_at = "52fe01b9a17c76c7068e241fa891e6cacd6fcbc6"
dirty = ["docs/flows/v2-ui-fidelity/brief.md"]

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 49
claim = "Design cuts milestones (49-59) and writes design files (61) before Surface again (80-88) and Write (90); the natural slot for a layout step"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 84
claim = "Surface again reuses the three tests; test 1 demands a different plan, which a layout pick never yields"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 49
claim = "Elaboration's Surface and decide, then design files (59), precede row revision (61); one AskUserQuestion call, at most three"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 220
claim = "Elaboration's commit stages only design/<name>.md files it wrote; a PNG under design/ is not on the list"

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 146
claim = "Surface test 1 and the cap of three (150) filter out a layout question"

[[source]]
path = "claude/skills/plan-v2-engine/SKILL.md"
line = 16
claim = "Only the orchestrator conversation can ask the user; subagents and Workflows have no AskUserQuestion; ask only when the answer changes the plan"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 529
claim = "Worker view inlines each cited spec.design section of the task (design_block, 796-810)"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 503
claim = "Worker view injects decisions whose binds name the task id or one of its files"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 238
claim = "design_path accepts only .md under design/, so a PNG cannot be cited directly as spec.design"

[[source]]
path = "tomlctl/src/plan/approve.rs"
line = 37
claim = "Approval refuses unless all of design/ (any file type, untracked or ignored included) is committed"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 369
claim = "MilestoneSpec has id, gates, elaboration, defer_reason, gate_probes, after: no design or reference field"

[[source]]
path = "claude/agents/verification.md"
claim = "verification agent holds Bash, Read, Grep (Haiku); it can Read a PNG by path but receives no worker view"

[[source]]
path = "claude/agents/plan-worker-deep.md"
claim = "plan workers hold Read, so they can open a reference PNG named in a design section"

[[source]]
path = "docs/flows/v2-ui-fidelity/brief.md"
claim = "Success criterion: at least two concrete layout options as pictures, picked before tasks are written, read by workers and the visual check"
```

## Answer

**Where.** Two slots, both before any task row exists:

1. Plan time: write-and-prove.md section 1, after "Milestones" (49-59) and before "Design files" (61), so the planner knows which milestone builds or reshapes a surface; the question goes to the user in Surface again (84-88), before Write (90). Today that question dies at test 1 ("a different plan", 84; same at open-and-challenge.md:146) and the cap of three (open-and-challenge.md:150); SKILL.md:16 says the same. The step needs its own exemption from those tests (the `surface-mechanics` lens owns that change).
2. Elaboration: elaborate.md section 2 (49-59), before section 3 revises rows. This is the better home for a rendered prototype: by then upstream milestones have built the real components (elaborate.md:47), and the milestone stays deferred until the flip, so nothing runs before the pick.

Neither step may delegate the question: subagents and Workflows have no `AskUserQuestion` (SKILL.md:16). Anything that builds or renders (a prototype) is a build the orchestrator must not run inline (CLAUDE.md Build discipline), and investigators hold Write only for their note, so a prototype needs either a new agent role or, cheaper, a prototype/spike milestone run by `/implement-v2` ahead of a deferred UI milestone that elaboration then designs against. That reuses deferral exactly as it exists.

**What each medium shows** (Claude Code 2.1.326, installed binary):

- `AskUserQuestion` `preview`: per-option string, single-select only, side-by-side with the option list. Format depends on host: terminal CLI gets `markdown` "rendered as markdown in a monospace box" and is told to use "ASCII mockups of UI layouts"; `claude-desktop`, `sdk-*`, `remote` and some VS Code hosts may set `html`, a self-contained fragment with inline styles, no `<script>`/`<style>`. This user works in terminal sessions, so in practice ASCII boxes: proportions, ordering, what is present, where empty space falls. No images, colour, real type or control style. Good enough to pick an arrangement; not enough for the brief's "as pictures", and blind to most of the defects the brief lists (scaling swatches, off-style glyphs).
- Artifact page: an Artifact tool family exists in the binary (`isArtifactToolEnabled`, `ArtifactTool`), and the artifact-design skills are listed for this account, but the tool is not visible to this subagent even via ToolSearch, so whether the orchestrator session holds it is unsettled. If it does: full HTML/CSS, side-by-side options, embedded images, interaction; it can approximate the project's look but is not the project's components. The pick still has to come back through `AskUserQuestion` to be recorded.
- Rendered prototype: the real components rendered (for example `@vitest/browser` `page.screenshot`, the `screenshot-capture` lens's subject) to PNGs. Only this shows real styles, real scaling and real content extremes, and the same images double as the visual check's reference. The model reads PNGs through Read; the user sees them only by opening the files or via an Artifact page embedding them, never inside the question in a terminal.

**Where the pick persists.** Three records, all of which the existing machinery already carries to readers:

- A `plan decide` row, `provenance = "user-stated"` or `user-confirmed`, `surfaced = true`, `rejected` holding the other options, `binds` = the UI tasks' ids. Worker view injects it (view.rs:503).
- `docs/flows/<slug>/design/ui-<milestone>.md` with a section per surface (chosen layout as ASCII, the reference PNG paths, constraints like "swatches keep a fixed size"), cited from each UI task's `spec.design`; worker view inlines the section text (view.rs:529, 796). PNGs sit in the same directory and are named in that text, since `design_path` takes only `.md` (check.rs:238).
- The visual check reads the same `.md` and PNGs by path. Milestones carry no design field (schema.rs:369) and the `verification` agent gets no worker view, so the gate's capture contract must name the reference by path, or the gate collects it from the milestone rows' `spec.design`. That join is for the `accept-look` and `screenshot-capture` lenses.

## Findings

- **Library/API or file**: Claude Code 2.1.326, `AskUserQuestion` option `preview` (installed `~/.local/bin/claude.exe`)
- **Source**: `rg -a` over the binary: "Preview content is rendered as markdown in a monospace box ... previews are only supported for single-select questions"; html variant "must be a self-contained HTML fragment (no <html>/<body> wrapper, no <script> or <style> tags"; selector `if(k==="markdown"||k==="html")...else if(!C.startsWith("sdk-")&&C!=="claude-desktop"&&C!=="local-agent"&&C!=="remote"...)pss("markdown")`
- **Evidence-grade**: high
- **Finding**: In terminal CLI sessions a question can show ASCII mockups only, not pictures; the brief's "as pictures" criterion needs a second medium.
- **Details**: Format comes from `CLAUDE_CODE_QUESTION_PREVIEW_FORMAT` or the host's `askUserQuestion.previewFormat`, else markdown for CLI. Previews are single-select only, so options shown with previews are pick-one.
- **Counter**: The orchestrating session runs under a desktop or SDK host that sets `previewFormat: "html"`; the user's workflow (terminal panes) says it does not.
- **Impact on plan / report**: ASCII preview is the always-available floor for every UI milestone; images go through an Artifact page or PNG files opened by path, and the pick is still recorded through `AskUserQuestion`.

- **Library/API or file**: `claude/skills/plan-v2-engine/references/write-and-prove.md:84` and `open-and-challenge.md:146,150`, `SKILL.md:16`
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: The layout question cannot pass Surface or Surface again as written; the design step needs an explicit exemption and its own place in section 1 of write-and-prove.md and section 2 of elaborate.md.
- **Details**: Test 1 requires "a different plan" and the cap is three, with "zero questions ... the normal case" (SKILL.md:16). A layout pick changes task content, not plan structure.
- **Counter**: A sibling change to Surface (lens `surface-mechanics`) adds a taste/UX class whose test admits it, in which case the design step only needs a pointer there.
- **Impact on plan / report**: Edits to `SKILL.md` (role rule and step table), `write-and-prove.md` sections 1 and 2, `elaborate.md` section 2; coordinate with `surface-mechanics`.

- **Library/API or file**: `tomlctl/src/plan/view.rs:529` (`worker`, `design_block` 796) and `view.rs:503` (bound decisions), `tomlctl/src/plan/check.rs:238` (`design_path`)
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: `spec.design` citations plus a bound decision already deliver a chosen design to workers with no tomlctl change, as long as the images are named by path inside a `.md` section.
- **Details**: `design_block` inlines the cited section text; `design_path` refuses non-`.md`, so a PNG cannot be cited directly; workers hold Read and can open the PNG the section names. `plan check` reports a dead anchor as `plan/design-anchor-dead`.
- **Counter**: If workers are expected to see the image without a Read call, `spec.design` would need to accept images, which is a tomlctl change.
- **Impact on plan / report**: Persist at `design/ui-<milestone>.md` + sibling PNGs; no schema change for workers. The visual check needs its own path to the reference (milestones have no design field, schema.rs:369).

- **Library/API or file**: `claude/skills/plan-v2-engine/references/elaborate.md:220` vs `tomlctl/src/plan/approve.rs:37`
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: At plan time approval refuses an uncommitted `design/` file of any type, but elaboration's commit stages only `design/<name>.md`, so reference PNGs written during elaboration would be left uncommitted with nothing to catch it.
- **Details**: `COMMITTED_AUTHORED = [BRIEF_FILE, "design/"]` with `--untracked-files=all --ignored` covers PNGs at approve; elaborate.md section 6 names "each `docs/flows/<slug>/design/<name>.md` this elaboration wrote" and runs no approve.
- **Counter**: `.gitattributes`/`.gitignore` or a later flow-store commit sweeps `docs/flows/<slug>/design/` anyway; the implement-v2 flow-store commit covers `.claude/flows/<slug>/`, not `docs/`.
- **Impact on plan / report**: The design-step task that edits elaborate.md widens the commit list to every file it wrote under `design/`.

- **Library/API or file**: Claude Code 2.1.326 Artifact tool (`isArtifactToolEnabled`, `ArtifactTool` in the binary)
- **Source**: `rg -a` over the binary; `ToolSearch("artifact publish page")` in this subagent returned only WebFetch
- **Evidence-grade**: low
- **Finding**: An Artifact page could show the options as pictures, but whether the `/plan-v2` orchestrator session holds the tool is unconfirmed.
- **Details**: The binary gates the tool behind enable/eligibility checks; the artifact-design skills are in this account's skill list, which suggests the feature is on for the main session.
- **Counter**: `ToolSearch` for the Artifact tool in an attended `/plan-v2` session returns nothing.
- **Impact on plan / report**: Treat the Artifact page as optional, used only when present, with ASCII previews and PNG paths as the fallback; settle it with one ToolSearch in the main session before deciding `design-medium`.

Lines for neighbours: the `design-medium` and `design-trigger` user decisions rest on the three-medium table above; the visual check's reference join belongs to `accept-look` / `screenshot-capture`.

Coverage: read in full `plan-v2-engine/SKILL.md`, `write-and-prove.md`, `elaborate.md`; read open-and-challenge.md section 5, `plan/view.rs` (worker, design_block, bind_names), `plan/check.rs` design_section/design_path, `plan/approve.rs` refuse_uncommitted, `plan/authored.rs` classify/list, `tasks/schema.rs` MilestoneSpec, agent frontmatter for plan-worker-lite/deep, verification, plan-investigator, plan-cold-reader. Checks: `rg -a` on the installed Claude Code binary for the AskUserQuestion preview schema, preview format selection and Artifact tool symbols; ToolSearch for an Artifact tool. Ruled out: a milestone-level design field (none exists; `spec.design` suffices for workers); `authored.rs` ignoring PNGs (it only feeds glimpse's listing, harmless).
Searched: installed binary `C:/Users/<user>/.local/bin/claude.exe` 2.1.326 (2026-11-17); no web sources; dead end: Artifact tool not callable from this subagent.
