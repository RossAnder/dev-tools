# lumina-story-blocks

A Claude Code plugin shipping nine composable story-block skills under the `/lumina:<block>` slash namespace. Each skill drives one well-defined region of a lumina story's data layout — problem statement, research notes, open questions, acceptance criteria, approach, not-doing, edge cases, relevance, closure gate — via lumina's existing MCP tool surface (`mcp__lumina__*`). Skills are independently invokable, idempotent on re-run (check-then-act with explicit supersession confirmation), and architected so that a future lumina web-UI button can dispatch each one without changing the skill body. The button surface itself is **out of scope for this release** — this plugin ships the skill family only. Skill bodies contain workflow instructions only; all data mutation goes through the lumina MCP server, so business logic stays in `lumina/src/repo.rs` rather than leaking into prompt text (see [CONVENTIONS.md §e](./CONVENTIONS.md)).

## What this plugin does

This plugin exposes nine skills that together fill out the multi-block lumina story schema (`work_items.attributes`, `acceptance_criteria`, `research_notes`, `open_questions`, `work_item_activity`). Each block is one skill, invoked as `/lumina:<block> <work_item_id>`; the skill reads current state via `mcp__lumina__get_work_item`, decides create / no-op / supersede, and writes via the matching MCP write tool. The skill family is composable — you run only the blocks you need, in any order — and each skill is idempotent: re-running on a populated field prompts for supersession confirmation rather than silently overwriting. The eventual UI-button vision (one button per block in the lumina web SPA dispatching the matching skill) is **deferred to a follow-up plan**; this release ships the skill primitives only, ready to be wired up later. Skill bodies are workflow instructions and contain no business logic — every data-mutation flows through the lumina MCP surface, per [CONVENTIONS.md §e](./CONVENTIONS.md).

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
const session = query({
  prompt: "/lumina:problem-statement <work_item_id>",
  options: {
    plugins: [{ type: "local", path: "./claude/plugins/lumina-story-blocks" }]
  }
});
```

The plugin's namespace (`/lumina:…`) comes from the `name: "lumina"` field in `.claude-plugin/plugin.json`, not from the directory name (`lumina-story-blocks`). The two are intentionally distinct: the directory name describes the package; the manifest name controls the slash invocation prefix.

## Skill list

The nine skills, ordered by typical workflow position:

| Skill | Slash invocation | One-line summary |
|---|---|---|
| problem-statement | `/lumina:problem-statement <id>` | Sets `attributes.problem_statement` (3-axis prompt). |
| research-notes | `/lumina:research-notes <id>` | Forked subagent: adds 3-7 `research_notes` rows. |
| user-interrogation | `/lumina:user-interrogation <id>` | HumanLayer 4-axis open-questions enumeration. |
| acceptance-criteria | `/lumina:acceptance-criteria <id>` | Adds free-text AC rows to task children. |
| approach | `/lumina:approach <id>` | Sets `attributes.execution_strategy` (drafts from prerequisites). |
| not-doing | `/lumina:not-doing <id>` | Sets `attributes.not_doing` (lens convention §g). |
| edge-cases | `/lumina:edge-cases <id>` | Adds `research_notes` with `lens="edge-case"`. |
| relevance | `/lumina:relevance <id>` | Thin wrapper over `set_relevance` (active/backlog/deferred/rejected). |
| closure-gate | `/lumina:closure-gate <id>` | Thin wrapper over `set_closure_gate` (hard/soft). |

The `<id>` placeholder is the lumina work-item UUID. To find one, use the `mcp__lumina__list_work_items` MCP tool (e.g. filter by `kind: "story"` to enumerate available stories), or browse the lumina web SPA and copy the id from a story's detail view.

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

- **Skills visible.** Run `/help` in the Claude Code session — the loaded-plugin section should list nine commands prefixed `lumina:` (one per row in the [Skill list](#skill-list) table above).

- **MCP server active.** Run `/mcp` — the output should list `lumina` as an active MCP server. If it isn't listed, the skills will fail at their first tool call; revisit the [Prerequisites](#prerequisites) registration step.

- **Smoke-test one skill.** Pick any story work-item id (use `mcp__lumina__list_work_items` with `kind: "story"` to find one), then run `/lumina:problem-statement <id>`. The skill should prompt with a 3-axis question (what's broken/missing, who's affected, success criteria). If you instead see a "tool not found" error, re-check that the MCP server is registered under the exact name `lumina`.

- **Pre-flight MCP smoke-check**: confirm the lumina MCP server can actually respond, not just that it is registered. Run a no-side-effect raw MCP call from the session: `mcp__lumina__list_work_items({})`. A reply with an array (possibly empty) confirms the server is reachable. A `tool not found` reply means symbol resolution failed (see the diagnostic for tool-not-found below). A timeout / connection error means the server is registered but down — `claude mcp list` will show the URL, and `curl <url>/mcp` should respond.

- **Diagnostic for `tool not found` errors after a successful `/mcp` listing**: if `/mcp` shows `lumina` as active but skill invocations fail with `tool not found`, run `claude mcp list` and confirm the entry is named exactly `lumina` (case-sensitive). If it is named differently (`Lumina`, `lumina-dev`, etc.), remove it and re-add it with the exact name `lumina`. Symbol resolution for `mcp__<server>__<tool>` is case-sensitive on the server prefix; a mismatch produces the same error surface as a down server, so check the name before restarting lumina.

- **`add_open_question` gotcha**: `mcp__lumina__add_open_question` takes `story_id` (NOT `work_item_id` like its table neighbours). If the user-interrogation skill errors with `invalid_params` and the work-item id is otherwise correct, confirm the SKILL.md body uses the `story_id` parameter name — passing `work_item_id` to this tool is the most common cause of `invalid_params` here. The user-interrogation `SKILL.md` flags this in its own body, but the operator-facing reproduction is here.
