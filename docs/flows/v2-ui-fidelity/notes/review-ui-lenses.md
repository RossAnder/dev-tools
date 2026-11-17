Verdict: All three fit the existing lens format with no tomlctl change. They are code-family judgement lenses that own new vocabulary tags. `ui-visual` and `interaction` activate on the existing `frontend` diff signal (`claude/skills/review-v2-lenses/vocabulary.toml:138`). `intent-fidelity` has no signal that can express "this run is bound to a v2 flow", so it needs `opt-in` plus a compose rule in the engine. The finder's eyes should be PNGs it reads with the Read tool from a capture run as a `verification` check. The Playwright MCP is only an optional extra. It is opt-in, gated by `dev-server-probe`, and limited to observation: it cannot click (`claude/skills/research-methods/references/browser-observation.md:15`). `intent-fidelity` reads the brief and the decisions through the store's `--slug`.

```toml sources
read_at = "52fe01b9a17c76c7068e241fa891e6cacd6fcbc6"
dirty = ["docs/flows/v2-ui-fidelity/brief.md"]

[[source]]
path = "claude/skills/review-v2-lenses/SKILL.md"
line = 39
claim = "Lens frontmatter fields, body sections, the Owns rule and v1_category sets"

[[source]]
path = "claude/skills/review-v2-lenses/SKILL.md"
line = 121
claim = "experimental status makes any entry opt-in; opt-in runs only on project enable or --lens +id"

[[source]]
path = "claude/skills/review-v2-lenses/SKILL.md"
line = 170
claim = "Tag ownership: each (family, tag) needs exactly one owning lens; the tag is added to the vocabulary first"

[[source]]
path = "claude/skills/review-v2-lenses/vocabulary.toml"
line = 138
claim = "The frontend diff signal already exists (tsx, jsx, vue, svelte, css, scss, less, html, cshtml, razor)"

[[source]]
path = "claude/skills/review-v2-lenses/vocabulary.toml"
line = 662
claim = "routing.executable omits css/scss/less/html, so a style-only cleanup routes to the Haiku checker"

[[source]]
path = "claude/skills/review-v2-lenses/vocabulary.toml"
line = 567
claim = "goal-coverage is a plan-family tag owned by plan-structure, the plan-time cousin of intent-fidelity"

[[source]]
path = "claude/skills/review-v2-lenses/checks/dev-server-probe.md"
line = 18
claim = "The DEV SERVER fence is written only when .mcp.json declares playwright and a documented URL answers"

[[source]]
path = "claude/skills/research-methods/references/browser-observation.md"
line = 15
claim = "The browser subset omits click, type, fill-form and evaluate, so it cannot drive an interaction"

[[source]]
path = "claude/agents/review-finder.md"
line = 4
claim = "review-finder lists ten read-only mcp__playwright__ tools plus Read"

[[source]]
path = "claude/agents/review-verifier.md"
line = 4
claim = "review-verifier has no playwright tools; it can only re-read an image through Read"

[[source]]
path = "claude/agents/review-surveyor.md"
line = 29
claim = "A judged signal's evidence must be files in the run's scope, which a code run's brief never is"

[[source]]
path = "claude/skills/review-v2-engine/references/compose.md"
line = 32
claim = "compose passes --lens to review plan --propose only from the user's own flags"

[[source]]
path = "claude/skills/review-v2-engine/references/survey.md"
line = 98
claim = "In a code run, pack.md carries only CLAUDE.md rules in CONSTRAINTS, never the brief or decisions"

[[source]]
path = "claude/skills/review-v2-lenses/lenses/correctness.md"
line = 38
claim = "correctness already expects a decision record the pack names, which a code run never supplies"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 86
claim = "Decision provenance vocabulary: user-stated, user-confirmed, agent-assumed"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 640
claim = "plan view --for human prints each decision's provenance on a Standing line, with no filter"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 533
claim = "A decision's binds are task ids or repo paths, linking it to code"

[[source]]
path = "claude/skills/review-v2-lenses/SKILL.md"
line = 189
claim = "A project [[checks]] row is a verification-executor command, the slot for a capture command"

[[source]]
path = "docs/flows/v2-ui-fidelity/brief.md"
line = 44
claim = "Success criterion the review area must meet"
```

## Answer

- **Format.** Each lens is `claude/skills/review-v2-lenses/lenses/<id>.md` with this frontmatter: `families = ["code"]`, `mode = "judgement"`, `produces` set to all three kinds, a `slice`, and a `v1_category` from the closed code set (`quality` for the two UI lenses, `completeness` for intent-fidelity). The body follows the fixed section order. Each lens also needs its new concern tags in `vocabulary.toml` under a `# <lens>` comment group before any lens can own them. The `catalogue_validates` dispatch gate enforces all of this. No validator change is needed.
- **Activation.** `ui-visual` and `interaction` use `activation = "signal"` and `apply_when = { signals = ["frontend"] }`. A run with no UI files then composes neither lens, which is what the brief's "no UI work runs exactly as today" asks for. Shipping them as `experimental` makes them opt-in whatever `activation` says, which suits a trial in reportdesignkit.
- **`intent-fidelity` cannot run on a signal.** Signals are `stack`, `diff`, `health` or `judged`. None of them can express "the store is `--slug` and `plan.toml` exists". A judged signal's evidence must be a file in the scope, and a code run's scope never contains the brief. So the lens is `opt-in`, and `compose.md` gains a rule: on a flow-bound code run whose flow has `plan.toml`, pass `--lens +intent-fidelity`. The alternative is to have `review plan --propose` add it in tomlctl, which is a larger change.
- **The finder's eyes.** In order of preference:
  1. PNGs from a capture command, judged with the Read tool. The capture runs as a `verification` check, either a project `[[checks]]` row or a built-in check with a `discover` ladder, before the cells run. The command and manifest come from the sibling `screenshot-capture` lens's contract.
  2. The Playwright MCP, only when `DEV SERVER: <url>` is in the pack.
  3. Source alone, graded no higher than `medium`, with a Counter line naming the capture state that would settle it.

  `interaction` cannot click anything under today's read-only browser contract. It judges handlers, states and focus from the code, plus any captured interaction states such as "after add stop" or "error".
- **How `intent-fidelity` reads intent.**
  - The flow slug comes from the store flag in the finder's prompt.
  - It reads `docs/flows/<slug>/brief.md`: Intent, Non-goals and Success criteria.
  - It reads the decisions with `tomlctl items list .claude/flows/<slug>/plan.toml --array decision --where provenance=user-stated` (verified on `v2-apply`). The alternative is `plan view --for human`, which has no provenance filter.
  - Each decision's `binds` (task ids or paths) leads to the task rows' `files` and `commit` stamps, which show where the code honours or breaks the decision.

  Whether `user-confirmed` decisions also count is the planner's call.

## Findings

- **Library/API or file**: `claude/skills/review-v2-lenses/SKILL.md:39` (lens table), `:69` (Owns rule), `:170` (Ownership)
- **Source**: file read in full; `tomlctl/src/cli/dispatch/tests/catalogue.rs` `catalogue_validates`
- **Evidence-grade**: high
- **Finding**: The three lenses need only catalogue files and vocabulary tags. No validator or tomlctl code is needed for the entries themselves.
- **Details**: Frontmatter keys: `families = ["code"]`, `mode = "judgement"`, `produces = ["defect","cleanup","advisory"]`, `owns`, `activation`, `slice`, and `v1_category` (`quality` or `completeness`). Proposed tag groups:
  - `ui-visual`: `layout-sizing` (scaling, reserved space, panel height), `overflow-clipping` and `style-conformance` (controls in a style the project does not use).
  - `interaction`: `interaction-states` (empty, loading, error, disabled, autoscroll) and `input-handling` (focus, keyboard, pointer).
  - `intent-fidelity`: `criteria-fidelity` and `decision-fidelity`.

  Each tag needs its own Owns paragraph led by the tag in backticks. Suggested slices: `per-subsystem` for the two UI lenses and `whole-scope` for intent-fidelity.
- **Counter**: Running `tomlctl review catalogue --check` on draft entries would refuse a field or tag combination. I could not run it without writing the entries.
- **Impact on plan / report**: One task adds three lens files and the tag groups in `vocabulary.toml`. The gate is `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`.

- **Library/API or file**: `claude/skills/review-v2-lenses/vocabulary.toml:138` `signals.frontend`; `claude/agents/review-surveyor.md:29`; `claude/skills/review-v2-engine/references/compose.md:32`
- **Source**: file reads; the `kind` vocabulary at `SKILL.md` (Signals table)
- **Evidence-grade**: high
- **Finding**: The UI lenses can activate on the existing `frontend` signal, but `intent-fidelity` cannot be signal-activated. It needs `opt-in` plus an engine compose rule (or a tomlctl change).
- **Details**: `frontend` already gates `dev-server-probe`. For `intent-fidelity`, the deciding fact is whether the run's store is `--slug` and `.claude/flows/<slug>/plan.toml` exists. No detector reads that. A judged signal fails too, because the surveyor drops evidence outside the scope, and the brief is never in a code run's scope. `compose.md` passes `--lens +id` only from the user's flags today, so the rule must be added there. A flow-less (`--scope`) code run has no brief and must never compose the lens.
- **Counter**: If `review plan --propose` already adds lenses based on the store's flow binding, the engine rule would be redundant. I found no such path in `compose.md` or `review-v2-engine/SKILL.md:51`.
- **Impact on plan / report**: Engine edit in `claude/skills/review-v2-engine/references/compose.md` (and optionally `SKILL.md`). The planner should record this as a decision against the tomlctl alternative.

- **Library/API or file**: `claude/agents/review-finder.md:4`; `claude/skills/review-v2-lenses/checks/dev-server-probe.md:18`; `claude/skills/research-methods/references/browser-observation.md:15`
- **Source**: file reads; user CLAUDE.md (Playwright MCP disabled globally, opt-in per project)
- **Evidence-grade**: high
- **Finding**: The finder's Playwright tools exist only where a project opts in, and they are limited to observation. The lenses' main evidence must therefore be captured PNGs that the finder reads with Read.
- **Details**: `dev-server-probe` writes `DEV SERVER: none` unless `.mcp.json` declares `playwright` and a documented URL answers. This repo's `.mcp.json` declares only `lumina`. The subset deliberately omits click, type and evaluate, so `interaction` cannot reach a state that needs driving. A capture command fits the existing `verification` check slot: a project `[[checks]]` row (`SKILL.md:189`), or a built-in check with `discover`. Its log path is recorded in `baselines` before stage 4, so cells read the images before they judge. Widening the finder's grant to include click is possible, but it would break the read-only contract against a live app and its backend, which is a user decision.
- **Counter**: If the sibling `screenshot-capture` lens finds that a Workflow-dispatched subagent cannot Read a PNG, the capture path fails, and only the opt-in MCP and source reading are left.
- **Impact on plan / report**: Depends on the screenshot-capture contract. Add a `ui-capture` check (executor `verification`) or document the project `[[checks]]` form. The lenses' Probes should name the capture manifest's states as their input.

- **Library/API or file**: `claude/agents/review-verifier.md:4`; `claude/skills/review-v2-lenses/vocabulary.toml:662` `[routing].executable`
- **Source**: file reads
- **Evidence-grade**: medium (routing read from SKILL.md's description of `review add`, `add.rs` routing code not traced)
- **Finding**: Visual findings fit badly with verify and route. The verifier has no browser, and a stylesheet cleanup routes to the Haiku checker.
- **Details**: `review-verifier` holds Read but no `mcp__playwright__*` tools. A refute-first check of a visual defect therefore needs the finding to cite an image path that persists in the run directory, or in the capture output directory. `routing.executable` omits `**/*.css`, `**/*.scss`, `**/*.less` and `**/*.html`. Under SKILL.md's routing rule, a `ui-visual` cleanup that edits only a stylesheet goes to the Haiku checker rather than the Opus verifier.
- **Counter**: If `review add` sends every row raised under a judgement lens to the verifier, regardless of path, the routing gap does not exist. Tracing `tomlctl/src/review/add.rs` routing would settle it.
- **Impact on plan / report**: Add the style globs to `[routing].executable` in `vocabulary.toml`. The lens's `Probes` should require citing the capture image path in `evidence`.

- **Library/API or file**: `tomlctl/src/plan/schema.rs:86` `Provenance`; `tomlctl/src/plan/view.rs:640`; `claude/skills/tomlctl/references/plan.md:533` (`binds`)
- **Source**: `tomlctl items list .claude/flows/v2-apply/plan.toml --array decision --where provenance=user-stated --select id,door --text-view` returned D1 and D11.
- **Evidence-grade**: high
- **Finding**: `intent-fidelity` can read user-stated decisions today without a new verb. It traces each decision to code through `binds`, then the task `files` and `commit` stamps.
- **Details**: The provenance vocabulary is `user-stated`, `user-confirmed` and `agent-assumed`. `plan view --for human` prints every decision with `Standing: <provenance>, <door> door` and has no filter. `items list --array decision --where provenance=user-stated` does filter. The brief sections to read are Intent, Non-goals and Success criteria (`review-v2-method/references/plan-records.md:52` names them). A divergence in code is a `defect` anchored on the code file. A divergence that suggests the user's decision is now wrong is an `advisory` question, never an edit to the decision.
- **Counter**: If the tomlctl-for-agents rule ("read a store through its own verb") is held to forbid `items list` on `plan.toml`, the lens must use `plan view --for human` and filter by reading. A `plan view` provenance filter would then be the clean fix.
- **Impact on plan / report**: Decide whether `user-confirmed` counts, and whether to use `items list` or add a filter to `plan view`.

- **Library/API or file**: `claude/skills/review-v2-engine/references/survey.md:98` (`CONSTRAINTS` fence); `claude/skills/review-v2-lenses/lenses/correctness.md:38`
- **Source**: file reads
- **Evidence-grade**: high
- **Finding**: A code run's `pack.md` never carries the flow's brief or decisions. An `INTENT` fence written by the surveyor on flow-bound runs would serve `intent-fidelity` and the decision record `correctness` already expects.
- **Details**: `CONSTRAINTS` quotes CLAUDE.md rules only. `correctness` says it reads "any plan or decision record the pack names", which a code run never supplies. The alternative is for `intent-fidelity` to read the brief and decisions itself from the slug. That is cheaper, but only that lens benefits.
- **Counter**: If a pack fence for the brief and decisions already exists in a code run's surveyor procedure, the note is wrong. `review-surveyor.md:37` lists the fences and none of them is one.
- **Impact on plan / report**: An optional choice between the two. The pack route touches `claude/agents/review-surveyor.md` and `review-v2-engine/references/survey.md`.

Other things noticed: the `react` and `vue` facets attach only to `memory-runtime`, `algorithm`, `correctness` and `idioms`, so `attaches_to` would need the new lens ids to carry framework pitfalls to them. `intent-fidelity`'s Does not flag section should leave `agent-assumed` decisions to `contrarian` (`decision-challenges`) and plan-time criterion coverage to `plan-structure` (`goal-coverage`).

Coverage: Read in full: `review-v2-lenses/SKILL.md`, `checks/dev-server-probe.md`, `lenses/plan-grounding.md`, `browser-observation.md`, `review-finder.md` (body), the brief and `.claude/review-lenses/config.toml`. Read in part: `vocabulary.toml` signals, concerns and routing; `review-surveyor.md`; `survey.md`; `compose.md`; the engine `SKILL.md` stage 1; and plan `schema.rs`/`view.rs`/`plan.md`. Checks run: the `items list` provenance filter, a backlog grep (no related item), and a grep for `brief`/`provenance` across the review side. Ruled out: a judged signal for intent-fidelity (its evidence must be in scope), and a stack-signal detector on `.claude/flows/*/plan.toml` (it is true for any repo that has any flow, not tied to the run). Playwright MCP output-directory behaviour was left to the screenshot-capture lens.
