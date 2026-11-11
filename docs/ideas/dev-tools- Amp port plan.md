# dev-tools: Amp port plan

# dev-tools: Amp port plan

How to bring the Claude Code harness in `RossAnder/dev-tools` (skills, commands, agents, Workflow scripts, and the `tomlctl` CLI) to Amp and share it with the team. Development stays in dev-tools under Claude Code; Amp is a deploy target.

## Goals

- One source of truth for shared content; a thin layer per harness.
- `tomlctl` available in every orb within seconds, version-pinned per consumer repo.
- One push deploys the Amp plugin (agents, tools, skills) at matching versions.
- A simple live panel in the orb (glimpse web mode) that runs only while someone is looking at it.

## 1. Packaging tomlctl

### Release workflow (new, in dev-tools)

- Trigger on tags `tomlctl-v*`.
- Build `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` (static; orbs run Debian). Optionally Windows and macOS for the team.
- Build `glimpse` for the same targets in the same job (it links tomlctl as a library, so versions always match).
- Attach `tomlctl-<target>.tar.gz` (containing `tomlctl` and `glimpse`) plus `.sha256` to the GitHub release. The repo is public, so orbs download without a token.
- `cargo-dist` can generate this, or a hand-written matrix job.

### Version pin (each consumer repo)

`.tomlctl-version` holds the version, such as `0.24.0`. Carriers' pre-flight already refuses stale binaries; the pin keeps repo stores and binary aligned.

### Orb install (`.agents/setup` in each consumer repo)

```bash
want=$(cat .tomlctl-version)
if [ "$(tomlctl --version 2>/dev/null | awk '{print $2}')" != "$want" ]; then
  arch=$(uname -m)  # x86_64 | aarch64
  url="https://github.com/RossAnder/dev-tools/releases/download/tomlctl-v$want/tomlctl-$arch-unknown-linux-musl.tar.gz"
  tmp=$(mktemp -d)
  curl -fsSL "$url" -o "$tmp/t.tgz"
  curl -fsSL "$url.sha256" | awk '{print $1"  '"$tmp"'/t.tgz"}' | sha256sum -c -
  tar -xzf "$tmp/t.tgz" -C "$tmp"
  install -m 0755 "$tmp/tomlctl" "$tmp/glimpse" ~/.local/bin/
fi
```

The binary is cached in the project snapshot. Bumping the pin on the base branch refreshes it.

### Later option

The Amp plugin downloads the pinned binary into its cache directory at load and wraps tomlctl verbs as plugin tools, removing the per-project setup step.

## 2. Shared core vs per-harness layer

About 85–90% of the content stays as one copy.

### Shared as-is

- tomlctl and all stores (`.claude/flows/…`, `.claude/backlog.toml`); paths unchanged.
- Contract and method skills: `flow-contract-*`, `research-methods`, `review-v2-method`, `review-v2-lenses`, `tomlctl` references, `backlog-capture`, `commit-conventions`.
- Agent bodies in `claude/agents/*.md`. Claude reads the frontmatter; the Amp plugin strips it and uses the body as `instructions`.
- Workflow logic (argument validation, cycle check, disjoint-file scheduler, `RETURN_SCHEMA`) moved to `claude/workflows/lib/`, imported (or inlined by a build step) by both harnesses.

### Per harness

- **Dispatch:** Agent tool, `name:`, teammates, `SendMessage` (Claude) vs plugin tool calling `agent.run()` (Amp).
- **Agent wrappers:** frontmatter vs `createAgent({ model, reasoningEffort, instructions, tools })`. One table maps lite and deep tiers to a model per harness.
- **Carrier entry points:** Claude commands vs Amp plugin commands, bundled skills, or saved prompts.
- **Deploy:** link scripts and `CLAUDE_CONFIG_DIR` vs plugin publish and `.agents/setup`.

### Engine skills

`implement-v2-engine`, `review-v2-engine`, `plan-v2-engine`, `apply-v2-engine` mix procedure with dispatch. Split each:

- Shared `SKILL.md`: steps, verbs, gates, re-entry.
- `references/dispatch-claude.md` and `references/dispatch-amp.md`: how to dispatch in each harness. The build ships only the matching one.

Also remove Claude-only wording from shared skills: teammate delivery blocks, `omitClaudeMd`, `CLAUDE_CONFIG_DIR`, `Skill()` syntax, and the `Explore` / `Plan` / `general-purpose` agents. "Every dispatch is one-shot" is Amp's default.

### Drift gates

- Extend `cli::dispatch::tests` and `command_lint` to cover Amp carriers and `dispatch-amp.md` files.
- Test that every agent in `claude/agents/` is registered by the Amp plugin or explicitly excluded.
- Unit-test the shared Workflow library once.

### Layout

```
claude/skills/…          shared; engine skills carry dispatch-claude / dispatch-amp refs
claude/agents/*.md       shared bodies; Claude frontmatter stays
claude/workflows/lib/    shared scheduler and validation JS
claude/workflows/*.js    thin Claude wrappers
amp/plugin/              thin Amp wrapper: agent table, tools calling lib/, commands
```

## 3. Amp plugin

One directory plugin (working name `flow`):

- **Agents:** `amp.createAgent(...)` per agent, model and effort from the tier table.
- **Workflows → tools:** for example `implement_v2_milestone`, running `plan-worker-*` subagents with `agent.run(prompt, { parentThreadID })` through a promise pool built on `lib/`. `run()` returns text, so parse and validate against `RETURN_SCHEMA`.
- **Skills:** bundled under `skills/`, registered with `amp.registerSkill` (Amp does not scan the folder; generate the list at build time). Listed as `flow:<skill>`.
- **Commands:** `/implement-v2`, `/review-v2`, `/plan-v2`, `/apply-v2` as plugin commands or bundled skills.

Not ported: glimpse TUI, statusline, herdr, Windows build tuning (local tooling).

## 4. Build and deploy

### Build (`scripts/build-amp.sh`)

Writes `dist/amp/`:

```
dist/amp/
  plugin/
    index.ts
    lib/
    agents/*.md
    skills/<name>/SKILL.md
  project-template/
    .agents/setup              tomlctl install snippet
    AGENTS.md.snippet          points agents at the plugin
  tomlctl.version
```

Steps: copy shared skills, swap in `dispatch-amp.md`, copy `lib/` and agent bodies, stamp the tomlctl version, run the drift gates over the output, `tsc --noEmit` against `@ampcode/plugin` types.

### Deploy (`scripts/deploy-amp.sh --personal | --workspace`)

```bash
amp clone user-plugins        # or workspace-plugins (admin only)
rsync -a --delete dist/amp/plugin/ <clone>/flow/
git -C <clone> commit -am "flow <version>" && git -C <clone> push
```

The push publishes. New threads load it; open threads need a plugin reload.

### Rollout

1. Publish to personal plugins and test.
2. Share it from Personal Settings so teammates can import it, or have an admin push it to `workspace-plugins`.
3. Per consumer repo: copy `project-template/` files and `.tomlctl-version`.

### Local testing

Install the Amp CLI, copy `dist/amp/plugin/` into `~/.config/amp/plugins/` or a test repo's `.amp/plugins/`, and run `amp` there.

## 5. Amp docs in Claude Code

No official Claude Code skill for Amp docs is known. Create `amp-platform`:

- Every docs page is raw Markdown at `https://ampcode.com/docs/markdown/<slug>`.
- Slugs to list: `plugin-api`, `customize/plugins`, `customize/skills`, `customize/global-plugins-and-skills`, `customize/agents-md`, `orbs/customizing`, `orbs/handling-secrets`, `boxes`, `cli`.
- Vendor `plugin-api.md` into `references/` with a refresh script (`curl`), since the API is still changing.
- Add `@ampcode/plugin` types as a dev dependency for type checking.

## 6. Oversight panel

A web mode of the glimpse app: the same data glimpse shows today (task graph, backlog, recent status changes), rendered as a minimal read-only HTML page instead of the ratatui UI. Served by the orb as a portal. No box, no separate hosting.

- **Binary:** `glimpse --web --port <port>`, released and installed alongside tomlctl (section 1). It already links tomlctl's library (snapshot, flow list), so the web mode reuses glimpse's data layer and only swaps the renderer. Keep the server minimal: one embedded HTML page plus a JSON endpoint, no SPA build.
- **Service:** declared in each consumer repo's `.amp/services.yaml` (shipped in `project-template/`):

```yaml
services:
  glimpse:
    command: glimpse --web --port "$PORT"
    health: /healthz
    review: false
    portal:
      title: Glimpse
      description: Live tasks and backlog for this thread.
```

- **Starts on demand:** nothing starts it at orb setup or session start. Opening the Glimpse link in the thread's Portal tab runs ensure and starts it.
- **Stops when unused:** the page polls only while its browser tab is visible (Page Visibility API). The server exits after a few minutes with no requests (`--idle-exit 5m`), and the next open starts it again. The orb idles and pauses as usual; an open, visible panel is the only thing that keeps it awake.
- **Data freshness:** reads the live working tree, so it shows uncommitted progress as agents write task rows. No git fetch or tokens needed.
- **Access:** only people who can view the thread (share the thread with the workspace, or use multiplayer). Public links are optional, time-limited, per portal.
- **Lifecycle:** exists only for that thread's orb. Opening the panel on a paused orb wakes it, which bills until it idles again. There is no cross-project view; each thread has its own panel.

## 7. Phase order

1. **tomlctl release workflow** and first tagged release; setup snippet in one consumer repo.
2. **Spike:** one plugin tool running 3–4 `plan-worker` subagents in parallel in an orb. Confirm parallel `run()`, per-agent model and effort, and that workers can run tomlctl and write their own task rows.
3. **Shared/per-harness split** for the `implement-v2` path: engine skill, two plan-worker agents, milestone Workflow.
4. **Build and deploy scripts;** publish to personal plugins.
5. **Remaining paths:** review-v2, plan-v2, apply-v2, then contract skills cleanup.
6. **Team rollout:** workspace publish and per-repo templates.
7. **Oversight panel:** glimpse web mode, added to the release, plus `.amp/services.yaml` in the project template.
