# lumina-story-blocks

A Claude Code plugin shipping composable story-block skills under the `/lumina:<block>` slash namespace. Each skill drives one well-defined region of a lumina story's data layout — problem statement, research notes, open questions, acceptance criteria, approach, not-doing, edge cases, relevance, closure gate; round-2 added risks, rejected alternatives, verification commands, vet-research, story-review, and the task-decomposition family (decompose-tasks, set-task-spec, wire-task-deps) plus the next-block advisor and plan-story chained runner — all via lumina's existing MCP tool surface (`mcp__lumina__*`). Round-3 adds two new research skills (research-explore, research-directed) and amends four existing skills (plan-story, set-task-spec, wire-task-deps, vet-research) for typed dispatch tier and six-phase enforcement. The migration-0010 epic/focus wave adds four more (epic-outcome, focus-shape, focus-framing, epic-close-criteria), for twenty-five total. Skills are independently invokable, idempotent on re-run (check-then-act with explicit supersession confirmation), and architected so that a future lumina web-UI button can dispatch each one without changing the skill body. The button surface itself is **out of scope for this release** — this plugin ships the skill family only. Skill bodies contain workflow instructions only; all data mutation goes through the lumina MCP server, so business logic stays in `lumina/src/repo.rs` rather than leaking into prompt text (see [CONVENTIONS.md §e](./CONVENTIONS.md)).

## What this plugin does

This plugin exposes the skills that together fill out the multi-block lumina story schema (`work_items.attributes`, `acceptance_criteria`, `research_notes`, `open_questions`, `work_item_activity`, plus the round-2 sub-tables `risks` / `rejected_alternatives` / `task_dependencies`). Each block is one skill, invoked as `/lumina:<block> <work_item_id>`; the skill reads current state via `mcp__lumina__get_work_item`, decides create / no-op / supersede, and writes via the matching MCP write tool. The skill family is composable — you run only the blocks you need, in any order — and each skill is idempotent: re-running on a populated field prompts for supersession confirmation rather than silently overwriting. Round-2 added an advisor (`/lumina:next-block`) and a chained runner (`/lumina:plan-story`) on top of the per-block surface, so an operator can either drive a story end-to-end via the chained runner or check the readiness signal and run the next recommended block individually. Round-3 reshaped plan-story to enforce a six-phase canonical sequence with hard precondition gates and added two new research skills (research-explore for multi-agent parallel exploration; research-directed for post-decision verification). The migration-0010 epic/focus wave added four more (epic-outcome, focus-shape, focus-framing, epic-close-criteria), for twenty-five total. The eventual UI-button vision (one button per block in the lumina web SPA dispatching the matching skill) is **deferred to a follow-up plan**; this release ships the skill primitives only, ready to be wired up later. Skill bodies are workflow instructions and contain no business logic — every data-mutation flows through the lumina MCP surface, per [CONVENTIONS.md §e](./CONVENTIONS.md).

## Load mechanism

The plugin is checked into the repo at `claude/plugins/lumina-story-blocks/`. The recommended path is a one-time project-scope install that persists to `.claude/settings.json` and is inherited by every team member who clones the repo:

**Permanent project install (recommended — persists to `.claude/settings.json`, all team members who clone the repo inherit it automatically)**:

```bash
claude plugin install --scope project ./claude/plugins/lumina-story-blocks
```

**One-off session load (no persistence — for ad-hoc trials)**:

```bash
claude --plugin-dir claude/plugins/lumina-story-blocks
```

**SDK (Claude Agent SDK)**:

```ts
// workItemId is the lumina work-item UUID — fetch via mcp__lumina__list_work_items.
const session = query({
  prompt: `/lumina:problem-statement ${workItemId}`,
  options: {
    plugins: [{ type: "local", path: "./claude/plugins/lumina-story-blocks" }]
  }
});
```

The plugin's namespace (`/lumina:…`) comes from the `name: "lumina"` field in `.claude-plugin/plugin.json`, not from the directory name (`lumina-story-blocks`). The two are intentionally distinct: the directory name describes the package; the manifest name controls the slash invocation prefix.

Note: `version` in `.claude-plugin/plugin.json` controls update delivery — bump it when making breaking changes to skill bodies or convention contracts. For a locally-checked-in plugin loaded via `--scope project`, version pinning is benign and operators always run whatever is checked in.

## Skill list

The skills, ordered by typical workflow position (round-1 family + round-2 additions + round-3 additions; the round-2 orchestration pair is documented separately under [Orchestration](#orchestration)):

| Skill | Slash invocation | One-line summary |
|---|---|---|
| problem-statement | `/lumina:problem-statement <id>` | Sets `attributes.problem_statement` (3-axis prompt). |
| research-notes | `/lumina:research-notes <id>` | Forked subagent: adds 3-7 `research_notes` rows. |
| research-explore | `/lumina:research-explore <id>` | Forked subagent: dispatch parallel lens-agents to explore the story; each agent returns proposed research notes for vet-research to triage. *(new in round-3)* |
| research-directed | `/lumina:research-directed <id>` | Forked subagent: verify decision-grade claims (libraries, APIs, file:line) after user decisions land; emit drift findings and supersede stale notes. *(new in round-3)* |
| user-interrogation | `/lumina:user-interrogation <id>` | HumanLayer 4-axis open-questions enumeration. |
| acceptance-criteria | `/lumina:acceptance-criteria <id>` | Adds free-text AC rows to task children. |
| approach | `/lumina:approach <id>` | Sets `attributes.execution_strategy` (drafts from prerequisites). |
| not-doing | `/lumina:not-doing <id>` | Sets `attributes.not_doing` (lens convention §g). |
| edge-cases | `/lumina:edge-cases <id>` | Adds `research_notes` with `lens="edge-case"`. |
| relevance | `/lumina:relevance <id>` | Thin wrapper over `set_relevance` (active/backlog/deferred/rejected). |
| closure-gate | `/lumina:closure-gate <id>` | Thin wrapper over `set_closure_gate` (hard/soft). |
| risks | `/lumina:risks <id>` | Capture or update a story's risks with severity + mitigation; per-element supersession on label collision. |
| alternatives | `/lumina:alternatives <id>` | Capture or update a story's rejected alternatives with confidence + rationale; per-element supersession on label collision. |
| verification-commands | `/lumina:verification-commands <id>` | Capture or update a story's verification commands (build/test/lint/smoke) used by /implement and /tdd. |
| vet-research | `/lumina:vet-research <id>` | Sample, spot-check, and promote/reject a story's proposed research notes; the only plugin skill that records `entry_type=vet` activity. *(amended in round-3 — parallelised verification dispatch)* |
| story-review | `/lumina:story-review <id>` | Critique a story across all planning blocks; emits structured findings via `add_finding{kind="story-review"}`. |
| next-block | `/lumina:next-block <id>` | Read a story's readiness and recommend the next `/lumina:<block>` slash command to run. |
| plan-story | `/lumina:plan-story <id>` | Walk a story through the six-phase canonical sequence with hard phase gates and skip-with-override audit. *(amended in round-3)* |
| decompose-tasks | `/lumina:decompose-tasks <id>` | Decompose a ready story into task children — proposing vertical-slice and pattern-replacement groupings (units-of-implementation spanning task subsets; not yet modelled in schema) with per-task `task_kind` (foundation/main/polish) and exhaustive Grep-derived `files_touched` for pattern-replacement bundles. *(task_kind vocab narrowed in round-3.5 migration 0007)* |
| set-task-spec | `/lumina:set-task-spec <id>` | Walk a story's task children and capture per-task spec (execution_detail, files_touched, dual-track outcome, effort, complexity, derived tier). *(amended in round-3)* |
| wire-task-deps | `/lumina:wire-task-deps <id>` | Wire explicit task→task dependency edges across a story's task children, then surface the Kahn-ordered phase schedule with per-task tier annotations and an agent budget. *(amended in round-3)* |
| epic-outcome | `/lumina:epic-outcome <id>` | Interrogate + set an epic's `outcome`. *(new in migration 0010 — epic-only)* |
| epic-close-criteria | `/lumina:epic-close-criteria <id>` | Manage an epic's close-criteria. *(new in migration 0010 — epic-only)* |
| focus-shape | `/lumina:focus-shape <id>` | Set a focus's `shape` (vertical-slice / cross-cutting / foundational). *(new in migration 0010 — focus-only)* |
| focus-framing | `/lumina:focus-framing <id>` | Set a focus's `framing`. *(new in migration 0010 — focus-only)* |

The `<id>` placeholder is the lumina work-item UUID. To find one, use the `mcp__lumina__list_work_items` MCP tool (e.g. filter by `kind: "story"` to enumerate available stories), or browse the lumina web SPA and copy the id from a story's detail view.

## Orchestration

Round-2 adds an advisor + chained-runner pair on top of the per-block skills. `/lumina:next-block <id>` (model-discoverable, read-only) reads `get_story_readiness` and recommends the next `/lumina:<block>` slash command to run; it's the entry point for ad-hoc / UI dispatch. `/lumina:plan-story <id>` (chained runner, side-effecting) walks the canonical six-phase sequence (round-3 amendment) with per-block AskUserQuestion gates (Run / Skip / Inspect / Abort); it's the CLI batch convenience for working through a story end-to-end. Both compose on the per-block skills — neither hides the per-block surface from the UI.

## Idempotency contract

Every skill follows the check-before-act sequence documented in [CONVENTIONS.md §b](./CONVENTIONS.md). Re-running a skill against the same work-item prompts for supersession rather than silently overwriting.

## Prerequisites

- [ ] **Lumina server running.** Lumina serves the MCP endpoint at `/mcp`. See the `## lumina` section of the repo `CLAUDE.md` for the build/run commands (`cargo build --manifest-path lumina/Cargo.toml`, then run the lumina binary — note the bound port, you'll need it for the next step).

- [ ] **Lumina registered as an MCP server named `lumina` in Claude Code.** The canonical registration command (from [`./skills/mcp/SKILL.md`](./skills/mcp/SKILL.md) §Connecting):

  ```
  claude mcp add --transport http lumina http://127.0.0.1:<port>/mcp
  ```

  Substitute the port the server is bound to. Once added, the tools surface as `mcp__lumina__<tool>` — e.g. `mcp__lumina__create_work_item`, `mcp__lumina__get_sprint_view`.

- [ ] **The MCP-server name MUST be `lumina` (lowercase).** The skill bodies in this plugin reference tools as `mcp__lumina__<tool>`; a different prefix (e.g. registering it as `Lumina` or `lumina-dev`) would silently break every skill in this plugin because the `mcp__<server>__<tool>` symbol resolution would no longer match.

## Verification

After loading the plugin, these quick checks confirm everything is wired up:

- **Skills visible.** Run `/help` in the Claude Code session — the loaded-plugin section should list twenty-five commands prefixed `lumina:` (one per row in the [Skill list](#skill-list) table above).

- **MCP server active.** Run `/mcp` — the output should list `lumina` as an active MCP server. If it isn't listed, the skills will fail at their first tool call; revisit the [Prerequisites](#prerequisites) registration step.

- **Smoke-test one skill.** Pick any story work-item id (use `mcp__lumina__list_work_items` with `kind: "story"` to find one), then run `/lumina:problem-statement <id>`. The skill should prompt with a 3-axis question (what's broken/missing, who's affected, success criteria). If you instead see a "tool not found" error, re-check that the MCP server is registered under the exact name `lumina`.

- **Pre-flight MCP smoke-check**: confirm the lumina MCP server can actually respond, not just that it is registered. Run a no-side-effect raw MCP call from the session: `mcp__lumina__list_work_items({})`. A reply with an array (possibly empty) confirms the server is reachable. A `tool not found` reply means symbol resolution failed (see the diagnostic for tool-not-found below). A timeout / connection error means the server is registered but down — `claude mcp list` will show the URL, and `curl <url>/mcp` should respond.

- **Diagnostic for `tool not found` errors after a successful `/mcp` listing**: if `/mcp` shows `lumina` as active but skill invocations fail with `tool not found`, run `claude mcp list` and confirm the entry is named exactly `lumina` (case-sensitive). If it is named differently (`Lumina`, `lumina-dev`, etc.), remove it and re-add it with the exact name `lumina`. Symbol resolution for `mcp__<server>__<tool>` is case-sensitive on the server prefix; a mismatch produces the same error surface as a down server, so check the name before restarting lumina.

- **`add_open_question` gotcha**: `mcp__lumina__add_open_question` takes `story_id` (NOT `work_item_id` like its table neighbours). If the user-interrogation skill errors with `invalid_params` and the work-item id is otherwise correct, confirm the SKILL.md body uses the `story_id` parameter name — passing `work_item_id` to this tool is the most common cause of `invalid_params` here. The user-interrogation `SKILL.md` flags this in its own body, but the operator-facing reproduction is here.

## Supply-chain note

Review PR diffs touching `claude/plugins/lumina-story-blocks/**` with the same scrutiny you would apply to an unsandboxed CI step — a malicious `SKILL.md` runs in your Claude Code session with full MCP access (including the lumina write tools) on next plugin load. There is no signed-content assertion or hash manifest; trust is established only by what is checked into git. Mirror the disclaimer in `CLAUDE.md`'s `.githooks/` paragraph.
