# Plan: glimpse — live task-DAG TUI for tomlctl flows, fed by hook-recorded agent records

**Plan path**: `docs/plans/lively-twirling-babbage.md`
**Created**: 2026-09-28
**Status**: Draft

## Context

`/implement` runs a flow's task DAG through parallel sub-agents, but the only way to watch it is to re-read `tasks.toml`, `PROGRESS-LOG.md` and the agent panel. This plan adds **glimpse**, a ratatui TUI shown in a herdr pane that renders a flow's task DAG live: dependency edges, status, which agent is working on what, and the execution record's events, with focus following the active frontier. `docs/plans/whimsical-hugging-puppy.md:17,118` deferred exactly this ("the ratatui `tasks watch` viewer + herdr pane launch") to a follow-up plan; the design here was settled in conversation on 2026-09-28:

- glimpse renders **only tomlctl data**. A new read verb, `tomlctl tasks snapshot`, returns everything in one consistent read.
- Agent lifecycle is captured by **async Claude Code hooks** into a new hook-written per-flow store, `.claude/flows/<slug>/agents.toml`, through `tomlctl agents record`. Hooks cost zero tokens (async, silent).
- The same hook opens the glimpse pane in herdr the first time a flow agent starts. A herdr keybinding opens it on demand.
- Reactivity is incremental: change detection by file fingerprint, snapshot diffing, a layout cache keyed by graph topology, and no redraw while idle.

## Scope
- **In scope**:
  - tomlctl: the `agents` verb group (`record`, `list`) and its store, the `tasks snapshot` verb, docs, and a 0.12.0 release.
  - The new `glimpse/` crate: layer, traversal and hand-rolled node-and-line diagram views in both orientations; a details panel; live agent activity; a flow selector with auto-follow; `hook`, `ensure-pane` and `setup` subcommands.
  - Git and pre-commit integration.
  - Backlog `B-22bd6fc6`.
- **Out of scope**: the Codex payload adapter (the enum value ships, parsing does not); write actions from glimpse (it is read-only); lumina; closing the pane automatically; a lane-style (`git log --graph`) view.
- **Affected areas**: `tomlctl/src/agents/`, `tomlctl/src/tasks/`, `tomlctl/src/cli/`, `tomlctl/src/io.rs`, `tomlctl/src/main.rs`, `tomlctl/tests/`, `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/README.md`, `claude/skills/tomlctl/`, `claude/skills/flow-contract-task-store/`, `glimpse/`, `.gitignore`, `.githooks/pre-commit`, `CLAUDE.md`

## Exploration Notes

**tomlctl read side** — every graph verb already has a library function returning `serde_json::Value`: `show` (`tomlctl/src/tasks/show.rs:36`), `edge_list` (`tomlctl/src/tasks/edges.rs:47`), `ready` (`tomlctl/src/tasks/ready.rs:17`), `batches` (`tomlctl/src/tasks/batches.rs:14`), `closure` (`tomlctl/src/tasks/closure.rs:33`; per-checkpoint `checkpoint_closure` `:51` is private). The typed engine is `tomlctl/src/tasks/graph.rs` — `Graph::build` `:192`, `kahn_rounds` `:241`, `frontier` `:269`, `groups` `:362` (members, maximal, valid_cut). A `snapshot.rs` builds the graph once and reuses all of them. `Store`/`TaskRow` (`tomlctl/src/tasks/schema.rs:48,156`) derive no `Serialize`; JSON is `toml_to_json(to_toml(..))` or hand-built maps. `TasksOp` is `tomlctl/src/cli/types.rs:2006`, destructured exhaustively at `tomlctl/src/tasks/dispatch.rs:33`. Store load: `tomlctl/src/tasks/store.rs:37,65`.

**tomlctl write side** — `mutate_doc` (`tomlctl/src/io.rs:775`) is guard→exclusive-lock→read→mutate→sidecar-first write; locks live at `.claude/.locks/<sha>.lock`. `SCHEMA_SEEDED_FLOW_FILES` (`tomlctl/src/io.rs:667`) seeds `schema_version`/`last_updated` for auto-created flow files. Id minting in-process inside the mutate closure (as `tomlctl/src/backlog/add.rs:218` does) avoids the two-call mint race. Naming the array `items` routes through `dedup_id` stamping (`tomlctl/src/items.rs:62`); backlog avoids that name deliberately (`tomlctl/src/backlog/mod.rs:7-10`). Stdin payload reader: `read_json_value_from_arg("-")` (`tomlctl/src/io.rs:394`, 32 MiB cap, refuses a TTY). Repo root `io::repo_or_cwd_root` (`tomlctl/src/io.rs:1521`) is process-cached (`TOMLCTL_ROOT` → git toplevel → cwd), so a hook must pin root from the payload `cwd` before first use.

**Verb-group wiring (backlog as template)** — `mod` in `tomlctl/src/main.rs`; `Cmd::Backlog` at `tomlctl/src/cli/types.rs:650` + op enum; `SUBCOMMANDS` `:88` and `FEATURES` `:21`; re-export `tomlctl/src/cli/mod.rs:31`; route `tomlctl/src/cli/dispatch.rs:385`. Tests that pin the surface: `tomlctl/tests/capabilities.rs` (`read_only_subcommands_hide_write_integrity_flags_in_help` :26, `write_subcommands_expose_all_integrity_flags_in_help` :96, `capabilities_features_contains_every_plan_feature` :1554, version pin :1650, README transcription :2282/:2338); `tomlctl/src/cli/dispatch/tests/lint.rs` (`command_lint` parses every documented `tomlctl …` line — docs cannot land before the verb exists); `tomlctl/src/cli/dispatch/tests/skills.rs` (SKILL body ≤500, reference ≤600 lines).

**Flow resolution** — `resolve` (`tomlctl/src/flow/resolve.rs:133`) is private in a private module; needs `pub(crate)` + re-export in `tomlctl/src/flow/mod.rs` for in-process use. No current-branch helper exists. The artifact set is five keys copied across six sites (`tomlctl/src/flow/artifacts.rs`, `envelope.rs:37`, `capabilities.rs:56`, `cli/types.rs:1440`, `flow/init.rs:150`, `flow/doctor.rs:433`); doctor never flags extra files, so `agents.toml` need NOT be a canonical artifact.

**Execution record** — no typed reader; two private copies (`tomlctl/src/tasks/import_plan.rs:1284`, `tomlctl/src/flow/doctor.rs:573`). Join by `task_ref` via `read_doc` + `io::items_array`. `checkpoint` entries carry `commits[]` and no `task_ref`; commits map to `TaskRow.commit` → checkpoint letter.

**Dispatch correlation** — `claude/commands/implement.md:93` fixes `tomlctl tasks show <id> --slug <slug> --with body,files,deps` in every implementer prompt (also `claude/skills/flow-contract-task-store/SKILL.md:363-371`). Agent `description` ("T16 …") is model habit, not contract. Verification agents get `commands:` only — no slug/checkpoint (`claude/agents/verification.md:14`). Under `checkpoints: milestones` the implementer pool is NAMED and re-tasked via `SendMessage` (`CLAUDE.md:29`) — re-tasks bypass the Agent tool and any spawn hook.

**Claude Code on-disk state** — `~/.claude/projects/<proj>/<session>/subagents/agent-<id>.meta.json` (`agentType`, `description`, `toolUseId`, `requestShape`) + `agent-<id>.jsonl` (live-appended, ISO `timestamp` per line, first line = dispatch prompt). Parent `<session>.jsonl` holds the Agent `tool_use`, launch ack, and `<task-notification>` with `<status>`. `statusline/src/teamdata.rs` already resolves this tree (`subagents_dir` from `transcript_path`, `claude_dir_from` pure fn, `contained_in` canonicalisation, `read_capped`, degrade-to-absent) with a version-anchor doc convention (`statusline/src/subagent.rs` module doc).

**Hooks in use** — `~/.claude/settings.json`: one `SessionStart` command hook (herdr-managed `herdr-agent-state.ps1`); repo `.claude/settings.json`: `SessionEnd` http hook (lumina). No async hooks yet. Codex: `~/.codex/config.toml` `[features] hooks = true` + `~/.codex/hooks.json` `SessionStart`. Direct edits to hooks in settings files are normally picked up by Claude Code's file watcher (hooks.md, "Disable or remove hooks"); a restart is the fallback when they do not fire. Shell-form hooks (no `args`) run through Git Bash on Windows; exec form (`args` set) spawns the executable directly (hooks.md, "Exec form and shell form").

**herdr (v0.9.1)** — `pane split [--current|--pane ID] --direction right|down --ratio F --cwd P --env K=V --no-focus` returns `.result.pane.pane_id` and cannot launch a command (follow with `pane run <id> <cmd>`). `pane rename <id> <label>` sets `label`, visible in `pane get`/`pane list` — the lookup tag. `pane layout --current` gives `area`/`rect` in cells. Closed pane ids are never reused. Keybindings live in `%APPDATA%\herdr\config.toml` `[[keys.command]]` (`type = "popup"` today). Hook processes inherit `HERDR_PANE_ID` from the Claude process env.

**Crate precedent** — `statusline/` is the template: standalone crate (own `Cargo.lock`/`target/`, no root workspace), edition 2024, fat-LTO release profile, rustdoc link lints denied, pure renderers with I/O confined to named modules, `tests/cli.rs` driving `CARGO_BIN_EXE_*`, installed by `cargo install --path`. No ratatui/crossterm in the repo yet; the herdr checkout pins `ratatui 0.30` + `crossterm 0.29`. `.gitignore:1-4` lists per-crate `target/` (glimpse needs `/glimpse/target/`). The pre-commit fmt gate is hard-wired to `tomlctl/Cargo.toml` (`.githooks/pre-commit:23`). Root `CLAUDE.md:63-68` lists sibling crates.

**Git exposure** — `.claude/flows/**` is tracked (325 files); an `agents.toml` written by hooks would churn every commit unless ignored by a targeted pattern (doctor warns if `.claude/` itself is ignored, `tomlctl/src/flow/doctor.rs:34-36`).

**Docs headroom** — `claude/skills/tomlctl/references/tasks.md` 571/600 (no room); `tasks-store.md` 161; `claude/skills/tomlctl/SKILL.md` 160; `claude/skills/flow-contract-task-store/SKILL.md` 394/500; `tomlctl/README.md` 373. Editing a mirrored skill needs a `.github` mirror sync commit.

**Backlog** — one live row in the plan's directories: `B-22bd6fc6` (open, `tomlctl/src/tasks`) — colon-inside-bold field label not imported; unrelated to this plan's concern.

## Research Notes

**Claude Code hooks** (raw https://code.claude.com/docs/en/hooks.md, fetched 2026-09-28; spot-checked):
- `SubagentStart` (:2343-2349) fires on Agent-tool spawn, on resume, **and each time an in-process teammate handles a new message**; payload adds only `agent_id`, `agent_type`; matcher filters on agent type (frontmatter `name`). → `record` treats start as an **upsert on `agent_id`** that opens a new assignment segment; named-pool re-tasking is covered.
- `SubagentStop` (:2385) adds `stop_hook_active`, `agent_id`, `agent_type`, `agent_transcript_path`, `last_assistant_message` (from v2.1.271 a `SubagentHandback` report lives in that tool's `tool_input.message`, :2391). Also fires for internal agents with empty `agent_type` (:2387-2389) → drop those.
- `TeammateIdle` (:2654-2672) carries `teammate_name`, `team_name`, no `agent_id`, no matcher.
- Async: `{"type":"command","command":"…","async":true}`; Claude Code does not wait for, or read the exit code/stdout/stderr of, an async hook, and `timeout` is not enforced → zero token cost, but failures are silent (needs its own error log).
- Agent PostToolUse `tool_response.status` is `async_launched` for background agents (default since v2.1.198) — SubagentStop is the completion signal.
- Env: hook processes inherit the parent env minus `OTEL_*` (hooks.md:756; env-vars.md:381) → `HERDR_PANE_ID` reaches hooks.
- Local, empirical: `agent-<id>.meta.json` appears 64-124 ms after the transcript's first line; line 1 is the full dispatch prompt (`parentUuid:null`, `type:"user"`). Transcript path = `<dirname(transcript_path)>/<session_id>/subagents/agent-<agent_id>.jsonl`. Teammate meta carries `name`, `teamName`, `taskKind:"in_process_teammate"`. → `record` retries a missing file briefly (<1 s). Evidence: medium on hook-vs-file ordering.
- Codex (https://learn.chatgpt.com/docs/hooks): has `SubagentStart`/`SubagentStop` with near-identical fields (`agent_transcript_path`, `last_assistant_message`, `turn_id`) and `async:true` → a `harness = "codex"` adapter is feasible later with the same record shape.

**TUI stack** (Context7 `/ratatui/ratatui/ratatui-v0.30.0`; https://github.com/ratatui/ratatui/blob/main/BREAKING-CHANGES.md; `C:\Users\rossa\dev\herdr\Cargo.lock`):
- Pin `ratatui 0.30` + reach crossterm via `ratatui::crossterm` (0.29) so one crossterm is linked. 0.30: `ratatui::run/init/restore`, `DefaultTerminal`; `block::Title` removed (use `Line`), `Alignment`→`HorizontalAlignment`.
- Event model: one input thread owning blocking `crossterm::event::read()` (poll/read must not be split across threads), one poller thread, main loop on `recv()` switching to `recv_timeout` only while a 1 s tick is needed; drain batched resize events before a single `draw`; drop `KeyEventKind::Release` (Windows doubles key events). ratatui diffs buffers and emits only changed cells, so no `draw` when idle = zero work (https://ratatui.rs/concepts/rendering/under-the-hood/).
- Diagram layout (**superseded** by the directed research below; the user rejected it): **`ascii-dag` 0.11.0** (MIT OR Apache-2.0, MSRV 1.92, zero deps; https://docs.rs/ascii-dag/0.11.0) — native TB/LR orientation, median + adjacent-exchange crossing reduction, orthogonal routing with junction merging, per-edge `EdgeStyle` (dashed for `coupling`), `Scene::hit_test`, `SceneComposer::visit_cells` for painting a ratatui `Buffer`. Risk: pre-1.0, single owner, frequent releases → pin `=0.11.0` behind an adapter module. Rejected: `layout-rs` (SVG), `rust-sugiyama` (no routing), `dagre-rs` (licence). Fallback: Gansner et al. 1993 (doi:10.1109/32.221135) pipeline over tomlctl's precomputed layers.
- **Layer parity**: tomlctl's Kahn in-degree is `needs ∪ coupling` (`tomlctl/src/tasks/graph.rs:7,210`). glimpse's diagram takes the snapshot's `layers` verbatim instead of re-layering, so the diagram and the layer view agree by construction.
- Layout caching: compute the layout once, keyed by a hash of node ids + both edge sets + orientation; on a status-only change repaint styles over the cached layout (evidence: medium, not benchmarked).
- Markdown: `tui-markdown` 0.3.10 (ratatui-core ^0.1, pulldown-cmark ^0.13; self-described experimental) — use `default-features = false` to avoid `syntect`; alternative is ~100 lines over `pulldown-cmark`.
- File watching (poller): poll `(mtime, len)` at 500 ms rather than `notify` 8.2 (Windows backend has a 16 KiB buffer with no overflow rescan; rename-replace needs care). tomlctl writes sidecar-then-TOML as two renames → never verify integrity from glimpse; a torn read retries next poll.

### Directed research additions

**Terminal DAG renderers** (Phase 5, after the user rejected `ascii-dag`; vetted 3/3 against the crates.io API):
- No crate meets the brief. `mermaid-text` 0.57.0 depends on `ascii-dag ^0.9.1`. `merman-ascii` 0.7.0 returns a `String` only, with no positions or per-cell style. `rataflow` 0.1.0 (ratatui 0.30) routes edges point to point, so they overlap and break edge highlighting (`src/ui/edge_path.rs` `compute_step_path`). `orthodag` 0.1.1 (created 2026-09-23) is alpha, has boxed nodes only, and lists "interactivity, hit testing" as out of scope. `sapling-renderdag` 0.1.0 (MIT; used by jj) is a vertical-only lane renderer at one node per row, so a 30-task flow takes about 60 rows and loses the Kahn layers.
- **Decision: write a layered (Sugiyama) layout by hand with compact `[nn]` nodes**, computed as (layer, slot) and drawn through an orientation map. Estimated 1,000–1,200 LOC plus snapshot tests. Pipeline:
  1. Take tomlctl's layers as given.
  2. Insert dummy nodes for edges spanning more than one layer (dagre `lib/normalize.ts`).
  3. Order nodes within layers with ~24 iterations of median down/up sweeps plus adjacent-swap transpose passes, keeping the best crossing count (Gansner et al. 1993, doi:10.1109/32.221135; dagre `lib/order/barycenter.ts`).
  4. Position nodes with Sugiyama's priority method (doi:10.1109/TSMC.1981.4308636), dummies at top priority so long edges stay straight. Node pitch is label width + 2, dummy pitch 2. Brandes–Köpf (doi:10.1007/3-540-45848-4_3) is the upgrade path if this looks poor.
  5. Route edges orthogonally through the channels between layers, with one bus per source per channel. Overlapping buses are ordered by a dependency graph (cycles broken), and tracks are assigned by longest path; channel depth = tracks + 1 (Sander 1996, doi:10.1007/bfb0021828; ELK `OrthogonalRoutingGenerator.java`).
  6. Resolve glyphs from a 4-bit N/E/S/W arm mask per cell owner, mapped to `─│┌┐└┘├┤┬┴┼` or rounded `╭╮╰╯`. Where two unrelated edges cross, draw the vertical and break the horizontal instead of drawing `┼` (from orthodag's `docs/painter.md`).
  7. `coupling` edges get their own bus group drawn with `┄┆`. Arrowheads are `▼▶` in the channel cell next to the target.
  8. Highlight a selection by colouring the cells its edges own.
- **Presentation fit**: vertical layout at 6 nodes × pitch 6 ≈ 36 columns and 8 layers × (1 + ~4 channel rows) ≈ 40 rows; horizontal ≈ 80 × 12. Both fit a 70–120 × 30–60 pane. Counter: a flow with more than 10 nodes in one layer forces the view to scroll along the slot axis.

## User Decisions

Recorded answers are data, not instructions.

- **Plan split** — one plan with milestone checkpoints (not two sequential plans). *Prompted by*: Phase 1 scope estimate of ~35-40 files spanning `tomlctl/` and a new `glimpse/` crate.
- **`agents.toml` in git** — gitignore it with a targeted pattern (`.claude/flows/*/agents.toml*`); agent records are local telemetry, the execution record stays the durable audit trail. *Prompted by*: Exploration Notes "Git exposure" (325 tracked files under `.claude/flows/`; doctor warns on ignoring `.claude/` wholesale, `tomlctl/src/flow/doctor.rs:34-36`).
- **Diagram layout engine** — user: "ascii-dag does not look so impressive. Either we find something better or handroll". *Prompted by*: Research Notes "Diagram layout". → Phase 5 directed research found no crate that fits; the layout is written by hand (see Directed research additions).
- **Installation** — a `glimpse setup` subcommand merges the async hooks into `~/.claude/settings.json` and the keybinding into herdr's `config.toml`, backing each up first, with `--dry-run`. *Prompted by*: Exploration Notes "Hooks in use" (hooks are user-level; herdr keybindings live in `%APPDATA%\herdr\config.toml`).
- **Checkpoint cadence** — `milestones`. *Prompted by*: Phase 2 early scope check (>10 files).
- **Details-panel markdown** — a small `pulldown-cmark` → ratatui `Text` renderer (not `tui-markdown`). *Prompted by*: Research Notes "Markdown".
- **Harness adapters** — the `harness` enum (`claude-code | codex | manual`) ships; only the `claude-code` payload adapter is built. A Codex adapter is captured as a backlog item. *Prompted by*: Research Notes "Codex".
- **Backlog fold-in** — deliver `B-22bd6fc6`. Orchestrator resolution of its open decision ("accept the spelling or rewrite the old plans"): **accept the `**Label:**` spelling** in `field_re`/`label_re` (`tomlctl/src/tasks/parse_tasks.rs:868-882`) — additive, restores file claims and edges for older plans on their next import, and leaves historical plan text untouched. *Prompted by*: Exploration Notes "Backlog".

## Approach

Data flows one way: **harness hook → `tomlctl agents record` → `agents.toml`**, then **`tasks.toml` + `execution-record.toml` + `agents.toml` → `tomlctl tasks snapshot` → glimpse**. glimpse never writes flow state and never parses TOML. Its only direct reads outside tomlctl are the selected agent's transcript tail (live activity) and file fingerprints (change detection).

### tomlctl: the agents store

`.claude/flows/<slug>/agents.toml` is gitignored and auto-created through `SCHEMA_SEEDED_FLOW_FILES`. Its array is named `agents`, not `items`, so `dedup_id` stamping stays out (`tomlctl/src/backlog/mod.rs:7-10` precedent):

```toml
schema_version = 1
last_updated = 2026-09-28
[[agents]]
id = "A3"                         # minted in-process inside the mutate closure
harness = "claude-code"           # claude-code | codex | manual
session_id = "c3349435-…"
agent_id = "a007fe3fdba915afd"
agent_type = "implement-deep"
kind = "subagent"                 # subagent | teammate  (meta.json taskKind)
name = ""                         # teammate name ("" for subagents)
team = ""
status = "running"                # running | idle | stopped
started_at = "2026-09-28T04:44:22Z"
updated_at = "2026-09-28T04:52:08Z"
ended_at = ""
transcript_path = "C:/Users/…/subagents/agent-a007fe3fdba915afd.jsonl"
summary = ""                      # last_assistant_message, ≤600 chars
context_tokens = 0                # last assistant usage (input + cache_read + cache_creation + output)
segments = [ { task_ids = [16], started_at = "…", ended_at = "…" } ]
```

`tomlctl agents record --harness claude-code [-]` reads one hook payload from stdin and handles it as follows:

- **Output**: one JSON line, `{"recorded":true,"slug","event","id","task_ids"}` or `{"recorded":false,"reason"}`.
- **Repo root**: when the payload's `cwd` exists, the process `chdir`s there first, so `repo_or_cwd_root` resolves the right worktree.
- **`SubagentStart`**: upserts on `(session_id, agent_id)` and opens a new segment. If the agent's open segment already covers the same task set, nothing changes. This one path covers spawn, resume and teammate re-tasking.
- **`SubagentStop`**: sets `stopped` for a subagent or `idle` for a teammate, closes the open segment, and stores the summary and token count.
- **`TeammateIdle`**: matches on `(session_id, name)` (`team_name` is deprecated in hooks.md and is not used for matching), sets `idle` and closes the segment.
- **Empty `agent_type`**: the payload is ignored.
- **Choosing the flow**:
  1. The latest user-role prompt in the agent's transcript that contains `tasks show <id> --slug <slug>` (`claude/commands/implement.md:93`). Several ids mean a cluster dispatch.
  2. Otherwise, session affinity: the flow whose `agents.toml` already holds the most recently updated row for this `session_id`. This attaches verification agents to the flow of the `/implement` run that launched them.
  3. Otherwise, the event is not recorded.
  - A chosen slug must name an existing flow: `<root>/.claude/flows/<slug>/context.toml` exists, else `{"recorded":false,"reason":"unknown-flow"}`. `mutate_doc` creates missing parents under `.claude/`, and a flow directory without `context.toml` fails `flow doctor`.
- **Stop and idle events** find their row by scanning `<root>/.claude/flows/*/agents.toml`. The transcript can lag the hook (hooks.md, common input fields), so the Stop and Idle paths re-run the dispatch lookup and correct the closing segment's `task_ids`. A Stop for an unknown agent (it can outrun its own Start) writes a `stopped` row when its transcript's dispatch resolves a flow.

### tomlctl: the snapshot contract

`tomlctl tasks snapshot --slug <s>` is read-only and builds the graph once. Top-level keys, in this order:

| Key | Content |
|---|---|
| `schema` | `1` |
| `revision` | First 16 hex chars of the sha256 over the length-prefixed bytes of `tasks.toml`, `execution-record.toml`, `agents.toml` and `context.toml`; an absent file counts as empty |
| `slug`, `plan_path` | From the store |
| `flow_status` | From `context.toml`, `""` when absent |
| `policy` | `{checkpoints, max_parallel, commit_granularity}` |
| `tasks` | Every row: `id, ref, title, effort, status, checkpoint, phase, files, needs, coupling, deps_note, action, detail, acceptance, agent, commit` |
| `layers` | `Graph::kahn_rounds` |
| `frontier` | Byte-identical to `tomlctl tasks ready --in-flight <ids of in-progress rows>`, reusing `tasks::ready::ready(&store, &in_progress_ids)` — with `&[]` the engine classes every in-progress row as stalled |
| `edges` | `tasks::edges::edge_list` |
| `checkpoints` | `[{id, rationale, members, maximal, valid_cut, commits, verification}]`. `commits` comes from record `checkpoint` entries mapped by commit. `verification` is `{outcome, summary}` from the latest `verification` entry whose task is a member, or `null`. |
| `record` | Every execution-record entry verbatim, plus `task_id`, resolved with `slug::normalise_for_match` (`tomlctl/src/tasks/slug.rs:50`). For `type = "checkpoint"` entries it also carries `checkpoint_ids`, derived from `commits[]` → rows whose `commit` matches → their `checkpoint`. |
| `agents` | Every `agents.toml` row verbatim, or `[]` |

### glimpse

A standalone crate modelled on `statusline/`: edition 2024, rust-version 1.98, fat-LTO release profile, rustdoc link lints. Dependencies: `ratatui 0.30` (crossterm 0.29 through `ratatui::crossterm`), `pulldown-cmark 0.13` without default features, `serde`, `serde_json` with `preserve_order`, and `toml 1`. It is bin-only, with the module tree declared once in `glimpse/src/main.rs` by the scaffold task so that later tasks never touch `main.rs` until the final wiring.

- **Change detection and data**: `source.rs` runs a poller thread that takes the `(mtime, len)` fingerprint of the four flow files (`tasks.toml`, `execution-record.toml`, `agents.toml`, `context.toml`) every `poll_ms` (default 500). On a change it runs `tomlctl tasks snapshot` and sends the snapshot to the main loop unless `revision` is unchanged. The poller also stats every `.claude/flows/*/tasks.toml` (no process spawn) and sends `FlowsChanged` when that set's mtimes move. `flows.rs` ranks the flows returned by `tomlctl flow list` by the mtime of their `tasks.toml`, and runs only on `FlowsChanged`; auto-follow switches to the freshest.
- **Reactivity**:
  - `diff.rs` turns two snapshots into a change set: topology changed, status transitions, agents started or stopped, record entries added.
  - `app.rs` flashes changed nodes and, while follow is on, reselects the frontier: the first in-progress task by (layer, id), else the first ready task.
  - The diagram layout is cached on `(topology_hash, orientation)`, so a status-only change just repaints.
  - `runtime.rs` has a single input thread doing blocking `crossterm::event::read` and dropping `Release` events. The main loop blocks on `recv()`, switching to `recv_timeout` only while `app.needs_tick()` (live flashes or visible running agents), drains batched events, and then draws once. ratatui's buffer diff emits only changed cells.
- **Views**: layers (the default), traversal and diagram, cycled with `Tab`. Orientation is resolved per frame from `config.orientation` (`auto`, `vertical` or `horizontal`; `auto` picks horizontal when `width ≥ orientation_threshold × height`, default 2.0) and flipped with `o`. `f` toggles follow, `s` opens the flow selector, `a` toggles auto-follow of flows, `Enter` opens details, `t` shows activity, `q` quits.
- **Headless render**: `glimpse --once [--snapshot <file>] [--size WxH]` renders one frame to plain text through ratatui's `TestBackend`. The CLI tests and the final smoke check use it.
- **herdr**:
  - **`glimpse hook`** is the single hook command. It forwards stdin to `tomlctl agents record`. On a recorded `start` with `HERDR_PANE_ID` set, it calls `pane::ensure`. It always exits 0 with no output; errors go to `<claude_dir>/glimpse/hook.log`, capped at 1 MiB.
  - **`pane::ensure`** holds a lock file so concurrent hooks don't race. It reuses the pane labelled `glimpse` in the origin's tab if there is one. Otherwise it splits the origin pane — `right` when `width ≥ split_threshold × height` (default 2.2), else `down` — at `pane_ratio` (default 0.4) with `--no-focus`, then labels the pane and types a launch line for `glimpse [--slug <slug>]` into it. `herdr pane run` sends text plus Enter to the pane's shell (pwsh here) rather than spawning a process, so the line is built for that shell.
  - **`glimpse ensure-pane [--slug S] [--focus]`** (the keybinding) uses `HERDR_ACTIVE_PANE_ID`, falling back to `HERDR_PANE_ID`. Without `--slug` the pane opens on the freshest flow with auto-flow on.
  - **`glimpse setup [--dry-run]`** merges async exec-form `SubagentStart`/`SubagentStop`/`TeammateIdle` hooks into `<claude_dir>/settings.json` and a `prefix+alt+g` popup keybinding into herdr's `config.toml`, writing each atomically, backing up only what it changes, and skipping anything already installed.

**Reuse**:

| Source | Used for |
|---|---|
| `statusline/src/teamdata.rs` | `claude_dir` resolution order, `contained_in` path canonicalisation, `read_capped` (re-implemented in glimpse and tomlctl, since the crates share no library) |
| `statusline/src/cli.rs` | The hand-rolled parser shape |
| `statusline/src/subagent.rs` | The version-anchor doc convention |
| `tomlctl/src/backlog/` | The layout of a new verb group |
| `tomlctl/src/io.rs` | `mutate_doc`, `read_doc`, `read_json_value_from_arg` |
| `tomlctl/src/tasks/{graph,ready,edges,slug}.rs` | The snapshot's graph products |

**Follow-up capture:** once `/implement` completes, the orchestrator captures the Codex payload adapter as a backlog item (`backlog-capture`).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml && cargo build --manifest-path glimpse/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml && cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus -- --ignored && cargo test --manifest-path glimpse/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets && cargo clippy --manifest-path glimpse/Cargo.toml --all-targets && cargo fmt --manifest-path glimpse/Cargo.toml -- --check && cargo fmt --manifest-path tomlctl/Cargo.toml -- --check
```

After the final pass, run the end-to-end smoke check by hand:

1. `cargo install --path tomlctl && cargo install --path glimpse`.
2. `tomlctl tasks snapshot --slug unified-singing-gem` prints the contract's top-level keys in order.
3. `glimpse --once --slug unified-singing-gem --size 110x40` prints a frame containing every task id.
4. `printf '{"hook_event_name":"SubagentStart","session_id":"x","agent_id":"y","agent_type":"Explore","cwd":"."}' | glimpse hook` prints nothing and exits 0, and no agents file appears: `git status --porcelain --ignored .claude/flows` prints the same before and after the call.
5. `glimpse setup --dry-run` lists the three hooks and the keybinding it would add.
6. Manually, inside herdr, after a real `glimpse setup` and a Claude Code restart: start `/implement` on a small flow and confirm that a single glimpse pane opens next to the Claude pane on the first dispatch, agent chips appear and clear, and follow tracks the frontier.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 1, 2, 10, 11, 12, 13, 14, 18, 22, 23, 24, 25, 40, 41
- **Max parallel agents**: 8
- **Commit granularity**: per-task

## Tasks

### Phase A — tomlctl data layer (tomlctl/)

### 1. Accept the colon-inside-bold field label spelling [S]
- **Files**: `tomlctl/src/tasks/parse_tasks.rs`
- **Depends on**: —
- **Backlog**: B-22bd6fc6
- **Action**: Make `field_re` and `label_re` (`tomlctl/src/tasks/parse_tasks.rs:868-882`) accept `- **Label:** value` as well as `- **Label**: value`.
- **Detail**:
  - The field regex becomes `^- \*\*(Files|Depends on|Blocked-by|Blocked by|Action|Detail|Acceptance|Effort|Backlog)(?:\*\*:|:\*\*)[ \t]*(.*)$`. `label_re` gets the same alternation, so a line with exactly one colon, inside or outside the bold, is a label.
  - Add tests to the file's `mod tests` (`parse_tasks.rs:890`):
    - `- **Files:** \`a.rs\`` imports `files`.
    - `- **Depends on:** 1` imports `needs`.
    - That spelling no longer produces the `plan/text-unstored` finding.
    - A bold phrase with no colon (`- **Note** text`) is still not a field.
- **Acceptance**:
  - `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::parse_tasks` passes, including the 4 new tests.
  - Falsifier: on today's tree `field_re` requires `\*\*:` right after the label (`parse_tasks.rs:872`), so the `**Files:**` test fails. Confirmed by reading the regex; predicted, unverified.
  - `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_corpus -- --ignored` still passes (regression guard). To see which plans use the old spelling, run `grep -rl -- '- \*\*Files:\*\*' docs/plans`; this plan also matches because its own examples quote the spelling. If one of them now surfaces an error-class finding (a dangling or cyclic edge newly imported), stop and report the plan and finding rather than editing the plan.

### 2. Ignore agent records and the glimpse build dir in git [S]
- **Files**: `.gitignore`
- **Depends on**: —
- **Action**: Add `.claude/flows/*/agents.toml*` and `/glimpse/target/` to `.gitignore`.
- **Detail**: Match only agent records. Ignoring `.claude/` itself would trip doctor's `gitignore-claude` check (`tomlctl/src/flow/doctor.rs:34-36`). The trailing `*` also covers the `.sha256` sidecar.
- **Acceptance**: `git check-ignore -q .claude/flows/x/agents.toml && git check-ignore -q .claude/flows/x/agents.toml.sha256 && git check-ignore -q glimpse/target/x && ! git check-ignore -q .claude/flows/x/tasks.toml && echo OK` prints `OK`. Probed both ways: today it prints nothing; against a scratch repo with the two patterns it prints `OK`.

### 3. Add the agents store schema [M]
- **Files**: `tomlctl/src/agents/mod.rs`, `tomlctl/src/agents/schema.rs`, `tomlctl/src/main.rs`
- **Depends on**: —
- **Action**: Create the `agents` module with the typed `agents.toml` store: record, segment and the three vocabulary enums.
- **Detail**:
  - Declare `mod agents;` in `tomlctl/src/main.rs`; `mod.rs` declares `pub(crate) mod schema;`.
  - Types:
    - `AgentsStore { schema_version, last_updated, agents: Vec<AgentRecord> }`.
    - `AgentRecord` with exactly the fields in the Approach's TOML block, in that order.
    - `Segment { task_ids: Vec<u32>, started_at: String, ended_at: String }`.
    - Enums `Harness` (`claude-code|codex|manual`), `AgentKind` (`subagent|teammate`) and `AgentStatus` (`running|idle|stopped`), each with `as_str`/`parse`.
  - Convert by hand with `from_toml`/`to_toml`, following `tomlctl/src/tasks/schema.rs:348,413`: `preserve_order` key order, unknown keys dropped, segments as an inline array of inline tables.
  - Helpers:
    - `agents_path(slug) -> Result<PathBuf>`: `repo_or_cwd_root()?` joined with `.claude/flows/<slug>/agents.toml`, as `resolve_store_path` does (`tomlctl/src/tasks/store.rs`). Never cwd-relative: the hook runs from the payload's `cwd`, possibly a subdirectory.
    - `next_id(&store) -> String` (`A<max+1>`).
    - `find_mut(session_id, agent_id)`, `find_teammate_mut(session_id, name)` (`TeammateIdle`'s `team_name` is deprecated in hooks.md).
    - `open_segment(task_ids, at)`, which is a no-op when the open segment has the same set, and `close_segment(at)`.
  - Inline tests: TOML round-trip is byte-stable, enum parsing rejects unknown values, `next_id` works on an empty store and on gaps, `open_segment` is idempotent.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl agents::schema` reports ≥4 passed. Today the module doesn't exist, so 0 tests match (predicted, unverified).

### 4. Correlate subagents with flow tasks from Claude Code's on-disk state [M]
- **Files**: `tomlctl/src/agents/correlate.rs`, `tomlctl/src/agents/mod.rs`
- **Depends on**: 3
- **Action**: Add the read-only functions that turn a hook payload into a transcript path, the latest dispatch (slug + task ids), teammate metadata, and a context-token count.
- **Detail**:
  - `subagent_transcript(transcript_path, session_id, agent_id, agent_transcript_path: Option<&str>) -> Option<PathBuf>` prefers `agent_transcript_path`, else `<dirname(transcript_path)>/<session_id>/subagents/agent-<agent_id>.jsonl`. Canonicalise and require the result to sit under `dirname(transcript_path)`, following `contained_in` in `statusline/src/teamdata.rs`.
  - `latest_dispatch(path) -> Option<Dispatch { slug, task_ids }>`:
    - Read the whole file when ≤4 MiB, else the last 1 MiB (starting after the first newline).
    - Scan lines newest-first, keeping only `"type":"user"` lines whose `message.content` is a string or text blocks, never `tool_result`.
    - Collect every `tasks show (\d+) --slug ([a-z0-9][a-z0-9-]{0,63})` match in the newest such line that has any. Task ids are sorted and deduped. If the slugs disagree, return `None`.
  - `meta(path) -> Meta { kind, name, team }` reads the sibling `.meta.json` (capped at 16 KiB). `taskKind == "in_process_teammate"` means `Teammate`.
  - `context_tokens(path) -> u64` takes the last assistant line's `usage` sum.
  - If the transcript is missing, retry up to 10 × 100 ms (meta.json appears within ~124 ms of spawn; see Research Notes). The transcript can also lag the hook (hooks.md, common input fields), so a re-task may still show the previous dispatch; task 6's Stop and Idle paths call `latest_dispatch` again to correct the closing segment.
  - Any other failure returns `None` or `0`.
  - Module doc carries a version anchor: "Observed against Claude Code 2.1.283, 2026-09-28."
  - Unit tests use temp fixture trees: a spawn prompt, a teammate re-task where the newer message wins, a cluster with 2 ids, disagreeing slugs giving `None`, a tool_result line containing the pattern being ignored, and a path outside the tree being rejected.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl agents::correlate` reports ≥6 passed. Today 0 (predicted, unverified).

### 5. Join the execution record for the snapshot [M]
- **Files**: `tomlctl/src/tasks/snapshot_record.rs`, `tomlctl/src/tasks/mod.rs`
- **Depends on**: —
- **Action**: Add `record_view(record: Option<&toml::Value>, store: &Store) -> serde_json::Value`, which builds the snapshot's `record` array and the per-checkpoint `commits`/`verification` join.
- **Detail**:
  - Declare `mod snapshot_record;` in `tomlctl/src/tasks/mod.rs`.
  - Emit each entry verbatim through `toml_to_json`, plus:
    - `task_id`: an exact `ref` match, falling back to `slug::normalise_for_match` (`tomlctl/src/tasks/slug.rs:50`); `null` when nothing matches.
    - `checkpoint_ids` (only on `type = "checkpoint"` entries): each commit in `commits[]` matched by prefix against rows' `commit` → their non-empty `checkpoint`, sorted and deduped.
  - Also expose `checkpoint_facts(&view, &store) -> BTreeMap<String, (Vec<String>, Option<JsonValue>)>`, giving each checkpoint its commits and its latest `verification` entry whose `task_id` is a member (`{outcome, summary}`).
  - Inline tests: ref resolution (exact, normalised, unmatched), mapping `checkpoint_ids` for a two-checkpoint commit train (the "A+D" shape of E18 in `unified-singing-gem`), the latest verification winning.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::snapshot_record` reports ≥4 passed. Today 0 (predicted, unverified).

### 6. Record hook events into the agents store [L]
- **Files**: `tomlctl/src/agents/record.rs`, `tomlctl/src/agents/mod.rs`, `tomlctl/src/io.rs`
- **Depends on**: 4
- **Action**: Implement `record(harness, payload, write_opts) -> Result<JsonValue>` with the claude-code adapter and the event semantics in the Approach.
- **Detail**:
  - Add `agents.toml` to `SCHEMA_SEEDED_FLOW_FILES` (`tomlctl/src/io.rs:667`).
  - Structure:
    - Split a pure `apply(store: &mut AgentsStore, event: Event, now: &str) -> Outcome` from the I/O.
    - `Event` is `Start { session_id, agent_id, agent_type, kind, name, team, transcript_path, task_ids }`, `Stop { session_id, agent_id, summary, context_tokens, transcript_path, task_ids }` or `Idle { session_id, name, task_ids }`. The Stop and Idle `task_ids` come from re-running `correlate::latest_dispatch` (the transcript lags the hook, so the Start-time read can be stale) and replace the closing segment's set when they differ.
    - `Harness::Codex`/`Manual` return an "adapter not implemented" error.
  - Steps:
    1. If the payload's `cwd` exists, call `std::env::set_current_dir(cwd)` before anything resolves the repo root.
    2. Drop an empty `agent_type` as `{"recorded":false,"reason":"internal-agent"}`.
    3. Choose the flow per the Approach: dispatch slug → session affinity → none (`{"recorded":false,"reason":"no-flow"}`), validating the slug with `flow::validate_slug` (re-exported at `tomlctl/src/flow/mod.rs:23`) and requiring `<root>/.claude/flows/<slug>/context.toml` to exist, else `{"recorded":false,"reason":"unknown-flow"}`. `mutate_doc` creates missing parents under `.claude/`, and a flow directory without `context.toml` fails `flow doctor`.
    4. For Stop and Idle, scan `<root>/.claude/flows/*/agents.toml` (the root from `repo_or_cwd_root`) for the owning row. A Stop whose agent no row holds (it can outrun its own Start) resolves its flow from the transcript's dispatch and writes a `stopped` row.
    5. Write with `mutate_doc`, minting ids inside the closure.
  - Trim the summary to 600 chars on a char boundary. Timestamps come from `time::now_rfc3339()`.
  - Unit tests on `apply`: new start, repeat start with the same tasks (no new segment), start with new tasks (the previous segment closes), subagent stop → `stopped`, teammate stop → `idle`, idle by session+name, stop carrying a different dispatch rewrites the closing segment's `task_ids`, stop for an unknown agent with a resolvable flow → a `stopped` row, stop for an unknown agent with no flow → not recorded.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl agents::record` reports ≥9 passed. Today 0 (predicted, unverified).

### 7. Assemble the task snapshot [M]
- **Files**: `tomlctl/src/tasks/snapshot.rs`, `tomlctl/src/tasks/mod.rs`
- **Depends on**: 3, 5
- **Action**: Implement `snapshot(slug, store_path, read_opts) -> Result<JsonValue>`, producing the contract in the Approach's snapshot table with keys in that order.
- **Detail**:
  - Declare `mod snapshot;` and re-export `snapshot` from `tomlctl/src/tasks/mod.rs`.
  - Load the store (`store::load`), build one `Graph` for `layers` and `checkpoints`, and pull `layers` from `kahn_rounds`, `frontier` from `ready::ready(&store, &in_progress_ids)` (the ids of rows whose status is `in-progress`; with `&[]` the engine classes them as stalled), `edges` from `edges::edge_list(&store, None)`, and `checkpoints` from `graph.groups` merged with each `Checkpoint.rationale` and the `checkpoint_facts` from task 5. `ready` and `edge_list` rebuild their own graph internally; accept that and do not modify `tomlctl/src/tasks/ready.rs` or `tomlctl/src/tasks/edges.rs`.
  - Read the execution record, `agents.toml` and `context.toml` with `io::read_doc` after an `exists()` check: `read_doc` returns a NotFound error on a missing file, so the caller maps an absent file to `[]` (or `flow_status = ""`). Take `flow_status` from `context.toml`.
  - `revision` is sha256 over the length-prefixed bytes of `tasks.toml`, `execution-record.toml`, `agents.toml` and `context.toml` (absent = empty), truncated to 16 hex chars.
  - The function is read-only: nothing it calls may create a file.
  - Inline tests: key order, revision stability and change, absent optional files, an in-progress row keeping its dependents in `next` rather than `blocked`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl tasks::snapshot::` reports ≥4 passed. Today 0 (predicted, unverified).

### 8. Add the agents list verb and group dispatch [S]
- **Files**: `tomlctl/src/agents/list.rs`, `tomlctl/src/agents/dispatch.rs`, `tomlctl/src/agents/mod.rs`
- **Depends on**: 6
- **Action**: Add `dispatch_record(harness, payload_arg, write_args)` and `dispatch_list(slug, read_args)`. Both take plain arguments so they compile before the clap types exist.
- **Detail**:
  - `dispatch_record` reads the payload with `io::read_json_value_from_arg` (`tomlctl/src/io.rs:394`), calls `record::record`, and prints the result JSON on one line.
  - `list` prints the `agents` array (`[]` when the file is absent — check `exists()` first, since `io::read_doc` errors on a missing file). Keep the read in a pure-enough `list::rows(path, read_opts) -> Result<JsonValue>` so it is unit-testable.
  - Follow `tomlctl/src/backlog/dispatch.rs:11` for the fan-out and the integrity-arg translation (`cli::dispatch::read_integrity_opts`/`write_integrity_opts`).
  - Inline tests in `list.rs`: an absent `agents.toml` lists `[]`; a seeded one lists its rows in order.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl agents::list` reports ≥2 passed (today 0; predicted, unverified). `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` reports no error in `tomlctl/src/agents/` (regression guard).

### 9. Wire the new verbs into the CLI [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/cli/mod.rs`, `tomlctl/src/tasks/dispatch.rs`
- **Depends on**: 7, 8
- **Action**: Add `Cmd::Agents { op: AgentsOp }` with `Record { --harness <claude-code|codex|manual>, [PAYLOAD default "-"], WriteIntegrityArgs }` and `List { --slug, ReadIntegrityArgs }`, plus `TasksOp::Snapshot { TasksTarget, ReadIntegrityArgs }`, and route them.
- **Detail**:
  - These four files are compile-coupled (`tomlctl/src/tasks/dispatch.rs:33` destructures `TasksOp` exhaustively).
  - Insert beside `Cmd::Backlog` (`tomlctl/src/cli/types.rs:650`) and `TasksOp` (`:2006`).
  - Add `"agents"` to `SUBCOMMANDS` (`:88`) and `agents_record`, `agents_list`, `tasks_snapshot` to `FEATURES` (`:21`; snake_case like every entry, and removing an entry later is a breaking change).
  - Re-export `AgentsOp` at `tomlctl/src/cli/mod.rs:31` and route it at `tomlctl/src/cli/dispatch.rs:385`.
  - The `TasksOp::Snapshot` arm resolves the store path via `resolve_store_path` and prints `snapshot(..)`.
  - Task 12 owns the capabilities feature-list test, so it will stay red until 12 lands.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl cli::dispatch::tests::lint` passes (regression guard), plus `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` clean (regression guard). Discriminating: `cargo run -q --manifest-path tomlctl/Cargo.toml -- agents list --help` and `cargo run -q --manifest-path tomlctl/Cargo.toml -- tasks snapshot --help` both exit 0 (today: unrecognised subcommand, exit 2; predicted, unverified).

### 10. Integration-test agents record [M]
- **Files**: `tomlctl/tests/agents_record.rs`
- **Depends on**: 9
- **Action**: Add black-box tests for `tomlctl agents record` and `agents list` over a sandboxed flow and a fake Claude projects tree.
- **Detail**:
  - Use `tests/common` (`sandbox`, `seed_tasks`, `cli`). Seed the flow's `context.toml` as well, since `record` refuses a slug whose `context.toml` is absent.
  - Build `<tmp>/projects/p/<session>.jsonl` plus `subagents/agent-<id>.jsonl` (line 1 = a user prompt containing `tasks show 2 --slug <TASKS_SLUG> --with body,files,deps`) and `agent-<id>.meta.json`, and pipe payloads on stdin with `cwd` = the sandbox root.
  - Cases:
    - start → a `running` row with segment `[2]`;
    - stop → `stopped` with a summary;
    - teammate start, a new transcript line for task 3, start again → the first segment is closed and a second opened;
    - `TeammateIdle` → `idle`;
    - empty `agent_type` → `recorded:false` and no file created;
    - no slug and no session affinity → `recorded:false`;
    - a second agent in the same session with no slug → attached by affinity;
    - a dispatch naming a slug with no `context.toml` → `recorded:false`, `reason:"unknown-flow"`, and no `.claude/flows/<slug>/` directory created;
    - a payload `cwd` in a subdirectory of the sandbox → the row lands in the root's `.claude/flows/<slug>/agents.toml`;
    - `agents list` output shape.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test agents_record` reports ≥10 passed. Today the target doesn't exist.

### 11. Integration-test tasks snapshot [M]
- **Files**: `tomlctl/tests/tasks_snapshot.rs`
- **Depends on**: 9
- **Action**: Add black-box tests pinning the snapshot contract.
- **Detail**: Seed a store (following `READ_FIXTURE` in `tomlctl/tests/tasks_read.rs`), an execution record with task-completion, deviation, checkpoint and verification entries, and an `agents.toml`. Assert:
  - the top-level key order;
  - `layers` equals `tasks batches`' `batches`;
  - with one row `in-progress`, `frontier` is byte-equal to `tasks ready --in-flight <that id>` output;
  - `record[].task_id` and `checkpoint_ids` resolve;
  - `checkpoints[].verification` is filled;
  - `revision` is unchanged across two runs and changes after `agents.toml` is edited, and again after `context.toml` is edited;
  - with no record and no agents file, both arrays are `[]` and no file is created.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_snapshot` reports ≥7 passed. Today the target doesn't exist.

### 12. Release tomlctl 0.12.0 with the new features registered [M]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/tests/capabilities.rs`, `tomlctl/README.md`
- **Depends on**: 9
- **Action**: Bump the version to 0.12.0 and bring the capabilities tests and README up to date with the new surface.
- **Detail**:
  - These four files are coupled through the version-pin and README-transcription tests.
  - In `tomlctl/tests/capabilities.rs`:
    - add `tasks snapshot` and `agents list` to `read_only_subcommands_hide_write_integrity_flags_in_help` (`:26`) and `agents record` to `write_subcommands_expose_all_integrity_flags_in_help` (`:96`);
    - add the three features (`agents_record`, `agents_list`, `tasks_snapshot`) to `capabilities_features_contains_every_plan_feature` (`:1554`), whose count assertion follows the list;
    - update the version pin (`:1650`).
  - In `tomlctl/README.md`: quick-tour lines (`:15-67`), rows in the Feature meanings table (`:318-369`), and the capabilities sample block (`~:257-281`): its `version`, its `"features"` array (checked by `readme_feature_transcriptions_match_capabilities_features`, `tomlctl/tests/capabilities.rs:2283`) and its `"subcommands"` array (add `agents`).
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes. Falsifier: `capabilities_version_matches_cargo_toml` pins the literal `0.11.0` (`tomlctl/tests/capabilities.rs:1665`, probed), so the bump fails it until the pin is updated.

### 13. Document the agents verb group [L]
- **Files**: `claude/skills/tomlctl/references/agents.md`, `claude/skills/tomlctl/SKILL.md`, `claude/skills/tomlctl/references/flow.md`, `claude/skills/tomlctl/references/write.md`, `.github/skills/tomlctl/references/agents.md`, `.github/skills/tomlctl/SKILL.md`, `.github/skills/tomlctl/references/flow.md`, `.github/skills/tomlctl/references/write.md`
- **Depends on**: 9
- **Action**: Write `claude/skills/tomlctl/references/agents.md` (the store schema, the event semantics, flow selection, the writer rule, and a `| Flag |` table under a backticked heading for each verb) and link it from the SKILL.md References list (`claude/skills/tomlctl/SKILL.md:55-65`). Bring the other verb and file enumerations up to date.
- **Detail**:
  - The `.github/skills/tomlctl/**` paths are git-tracked copies reached through a junction on disk, so they change with the `claude/skills/` edits; they are on the Files line so `/implement`'s commit train stages them rather than halting on them as unexplained leftovers.
  - In `claude/skills/tomlctl/SKILL.md`: add the `agents` group to the description's verb groups (the description stays ≤1024 chars, `skill_descriptions_under_spec_cap`); add an `agents` row and `snapshot` to the `tasks` row of the Quick Reference table; drop the count from "seven sibling files" (`:57`); change the `capabilities` example's `0.11.0` (`:94`) to `0.12.0`.
  - In `claude/skills/tomlctl/references/flow.md`: add `tasks snapshot` and `agents list` to the `--verify-integrity` support matrix.
  - In `claude/skills/tomlctl/references/write.md`: add `agents.toml` to the recognised seeded flow files list (`:78`).
  - State that `agents record` is run only from harness hooks (installed by `glimpse setup`), that carriers and sub-agents never call it, and that `agents.toml` is gitignored local telemetry.
  - Every `tomlctl …` line in a bash fence must parse, because `command_lint` checks it.
  - Keep the reference ≤600 lines.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl cli::dispatch::tests` passes, and `grep -c 'references/agents.md' claude/skills/tomlctl/SKILL.md` ≥1 (today 0).

### 14. Document the snapshot verb and the agents-store writer [S]
- **Files**: `claude/skills/tomlctl/references/tasks-store.md`, `claude/skills/flow-contract-task-store/SKILL.md`, `.github/skills/tomlctl/references/tasks-store.md`, `.github/skills/flow-contract-task-store/SKILL.md`
- **Depends on**: 9
- **Action**:
  - Add a `tasks snapshot` section to `claude/skills/tomlctl/references/tasks-store.md` (`claude/skills/tomlctl/references/tasks.md` has no room under the 600-line cap: `wc -l` it) covering the envelope keys, `revision` semantics, and read-only-ness.
  - In `claude/skills/flow-contract-task-store/SKILL.md`: add a `snapshot` paragraph to §5 (verb semantics), add `snapshot` to §11's read-verb list, and add one short paragraph after §11 (`:357-361`): `agents.toml` is not the task store, and its only writer is `tomlctl agents record` run by harness hooks.
- **Detail**: Stay under the 500-line ceiling for the SKILL body (`wc -l claude/skills/flow-contract-task-store/SKILL.md`). The `.github/skills/**` paths are git-tracked copies reached through a junction on disk; they are listed so the commit train stages them.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl cli::dispatch::tests` passes, and `grep -c 'tasks snapshot' claude/skills/tomlctl/references/tasks-store.md` ≥1 (today 0).

### Phase B — glimpse core (glimpse/)

### 15. Scaffold the glimpse crate [M]
- **Files**: `glimpse/Cargo.toml`, `glimpse/Cargo.lock`, `glimpse/src/main.rs`, `glimpse/src/cli.rs`, `glimpse/src/config.rs`, `glimpse/src/theme.rs`, `glimpse/src/model.rs`, `glimpse/src/source.rs`, `glimpse/src/flows.rs`, `glimpse/src/diff.rs`, `glimpse/src/app.rs`, `glimpse/src/keys.rs`, `glimpse/src/runtime.rs`, `glimpse/src/transcript.rs`, `glimpse/src/herdr.rs`, `glimpse/src/pane.rs`, `glimpse/src/hook.rs`, `glimpse/src/setup.rs`, `glimpse/src/view/mod.rs`, `glimpse/src/view/header.rs`, `glimpse/src/view/layers.rs`, `glimpse/src/view/ego.rs`, `glimpse/src/view/details.rs`, `glimpse/src/view/markdown.rs`, `glimpse/src/view/selector.rs`, `glimpse/src/view/activity.rs`, `glimpse/src/diagram/mod.rs`, `glimpse/src/diagram/order.rs`, `glimpse/src/diagram/position.rs`, `glimpse/src/diagram/route.rs`, `glimpse/src/diagram/glyph.rs`, `glimpse/src/diagram/paint.rs`
- **Depends on**: 2
- **Action**: Create the crate with its whole module tree. Every module file except `main.rs`, `glimpse/src/view/mod.rs` and `glimpse/src/diagram/mod.rs` is a stub holding a single `//!` line naming what it is responsible for; the two `mod.rs` files hold that line plus `pub(crate) mod <child>;` for each child. Later tasks then own disjoint files and never edit `main.rs`.
- **Detail**:
  - These files can't be split: the module tree must exist before any later task can compile.
  - `Cargo.toml` follows `statusline/Cargo.toml`: edition 2024, `rust-version = "1.98"`, the same `[profile.release]`, `[lints.rustdoc]` deny list.
  - Dependencies: `ratatui = "0.30"` (default features), `pulldown-cmark = { version = "0.13", default-features = false }`, `serde = { version = "1", features = ["derive"] }`, `serde_json = { version = "1", features = ["preserve_order"] }`, `toml = "1"`. crossterm comes only through `ratatui::crossterm`.
  - `main.rs` declares every module (`mod view;` and `mod diagram;` use `mod.rs` files declaring their children) and has a `fn main()` that prints `glimpse: not wired yet` and exits 0. Add `#![allow(dead_code)]` at the top; task 39 removes it.
  - `Cargo.lock` is produced by the first clippy run.
- **Acceptance**: `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets` exits 0 (predicted, unverified: it builds). `git check-ignore -q glimpse/target/x` succeeds (depends on task 2).

### 16. Define the snapshot model [M]
- **Files**: `glimpse/src/model.rs`, `glimpse/tests/fixtures/snapshot.json`
- **Depends on**: 7, 15
- **Action**: Define serde types for the snapshot contract (see the snapshot table in Approach), an `Index` built from them, and a fixture whose shape is taken from real `tomlctl tasks snapshot` output.
- **Detail**:
  - Every struct is `#[serde(default)]`: a missing field reads as absent and unknown fields are ignored, like statusline's degrade-to-absent. Because that hides a misspelt field, take every key name and nesting in the fixture from `tomlctl/src/tasks/snapshot.rs` (task 7) rather than from the Approach table, and add a test that re-serialises the parsed fixture and asserts every key path present in the fixture JSON survives the round trip.
  - `Snapshot::index()` returns an owned `Index` (id → position maps, no borrow of the `Snapshot`, since task 21's `App` stores both) giving `task(id)`, `layer_of(id)`, `dependents(id)`, `coupled(id)`, `agents_for(id)` (rows whose segments contain the id, newest first), `record_for(id)`, `checkpoint(id)`, and `topology_hash()` (FNV or `DefaultHasher` over sorted ids + needs + coupling; stable across status changes).
  - The fixture has 8 tasks over 3 layers with `needs` and `coupling` edges, 2 checkpoints (one carrying commits and verification `pass`), record entries of type task-completion, deviation, checkpoint and verification, and 2 agents (one running subagent on task 4; one idle teammate with two segments).
  - Expose `#[cfg(test)] pub(crate) fn fixture() -> Snapshot` in `glimpse/src/model.rs`, loading `include_str!("../tests/fixtures/snapshot.json")`; every later test (tasks 22–27, 32–34) calls it rather than re-deriving the relative path, which differs from `glimpse/src/view/` and `glimpse/src/diagram/`.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml model::` reports ≥5 passed: fixture parses, every fixture key survives a round trip, unknown key ignored, hash stable under a status change, hash changes when an edge is added (predicted, unverified).

### 17. Add config and theme [M]
- **Files**: `glimpse/src/config.rs`, `glimpse/src/theme.rs`
- **Depends on**: 15
- **Action**: Add `Config` (defaults, loading, a pure `parse(&str)`) and `Theme` (the styles for each status and element), plus the shared enums tasks 21, 22, 26, 32, 33 and 39 import rather than redefine: `Orientation { Vertical, Horizontal }` (the resolved axis, with `flip()`), `OrientationPref { Auto, Fixed(Orientation) }` (the config value) and `ViewKind { Layers, Ego, Diagram }` (with `next()`/`prev()`), all in `glimpse/src/config.rs`.
- **Detail**:
  - `Config` fields and defaults:

    | Field | Default |
    |---|---|
    | `orientation` | `Auto` (also `Vertical`, `Horizontal`) |
    | `orientation_threshold` | 2.0 |
    | `split_threshold` | 2.2 |
    | `pane_ratio` | 0.4 |
    | `poll_ms` | 500 |
    | `tomlctl` | `"tomlctl"` |
    | `default_view` | `Layers` (also `Ego`, `Diagram`) |
    | `stale_after_s` | 300 (a `running` agent whose transcript is untouched this long is shown stale and keeps no tick alive) |

  - Loaded from `$GLIMPSE_CONFIG`, else `<home>/.config/glimpse/config.toml`. A missing file gives the defaults. A malformed file gives the defaults plus a warning string that the header shows.
  - `claude_dir()` follows `statusline/src/teamdata.rs` (`CLAUDE_CONFIG_DIR` → `USERPROFILE` → `HOME`, as a pure `claude_dir_from` + env wrapper).
  - Theme styles:
    - Status: pending is dim; in-progress is `#FFC799`, the full-strength accent from the shared Kanso Zen palette; done is green; failed is red; deferred is grey.
    - Also styles for selection, needs-edge highlight, coupling-edge highlight, flash, agent chip and badge.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml config::` reports ≥4 passed: defaults, partial file, malformed file gives a warning, `claude_dir_from` precedence (predicted, unverified).

### 18. Poll the flow files and fetch snapshots [M]
- **Files**: `glimpse/src/source.rs`
- **Depends on**: 16, 17
- **Action**: Define `pub(crate) enum Event { Snapshot(Box<Snapshot>), SourceError(String), FlowsChanged, Input(ratatui::crossterm::event::Event) }` in `glimpse/src/source.rs` — the one channel type task 34's runtime receives poll and input events on — then implement `Source` plus a poller thread that sends on a `std::sync::mpsc::Sender<Event>`.
- **Detail**:
  - `fingerprint(root, slug)` is the `(mtime, len)` of `tasks.toml`, `execution-record.toml`, `agents.toml` and `context.toml` (absent → `None`).
  - Each poll also stats every `<root>/.claude/flows/*/tasks.toml` (a directory listing, no process spawn) and sends `Event::FlowsChanged` when that set or its mtimes change — the only trigger for `flows::list`.
  - On start, run `<config.tomlctl> capabilities` once; if its `features` lacks `tasks_snapshot`, send `SourceError("tomlctl ≥0.12.0 required — cargo install --path tomlctl")` instead of polling.
  - Fetching goes through a `Fetcher` trait. The production `Fetcher` runs `<config.tomlctl> tasks snapshot --slug <slug>` with `current_dir(root)` and parses stdout; tests inject a fake.
  - Skip sending when `revision` equals the last one sent. A command channel accepts `SetSlug(String)`, which forces a fetch.
  - Never read integrity sidecars. A parse failure is sent as `SourceError` and retried at the next fingerprint change or after 5 s.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml source::` reports ≥4 passed: fingerprint change detected on temp files, unchanged revision suppressed, fetch error surfaces as `SourceError`, a new `.claude/flows/<slug>/tasks.toml` raises `FlowsChanged` (predicted, unverified).

### 19. Discover flows and pick the freshest [S]
- **Files**: `glimpse/src/flows.rs`
- **Depends on**: 17
- **Action**: Implement `list(root, tomlctl) -> Vec<FlowEntry { slug, status, updated, plan_path, tasks_mtime }>` and `freshest(&[FlowEntry])`.
- **Detail**:
  - Run `tomlctl flow list` (JSON `{ok, flows[]}`) and keep flows whose `.claude/flows/<slug>/tasks.toml` exists, sorted by `tasks_mtime` descending.
  - `repo_root(cwd)` is `git rev-parse --show-toplevel`, falling back to walking up for `.claude/flows`.
  - The pure `rank(entries)` and a JSON parse fn are unit-tested.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml flows::` reports ≥3 passed (predicted, unverified).

### 20. Compute snapshot change sets [S]
- **Files**: `glimpse/src/diff.rs`
- **Depends on**: 16
- **Action**: Implement `diff(old: &Snapshot, new: &Snapshot) -> Changes { topology_changed, status_changed: Vec<(u32, String, String)>, agents_started: Vec<String>, agents_stopped: Vec<String>, record_added: Vec<String> }`.
- **Detail**: Topology comes from `topology_hash`. Agents are keyed by `id`, record entries by `id`.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml diff::` reports ≥4 passed (predicted, unverified).

### 21. Hold app state and map keys [L]
- **Files**: `glimpse/src/app.rs`, `glimpse/src/keys.rs`
- **Depends on**: 16, 17, 19, 20
- **Action**: Implement `App`, the `Action` enum, `App::apply(action)`, `App::apply_snapshot(snapshot, now)`, and `keys::map(KeyEvent) -> Option<Action>`.
- **Detail**:
  - `App` fields: `snapshot`, `index`, `view: ViewKind`, `orientation_override: Option<Orientation>` (the `config.rs` types), `selected: Option<u32>`, `follow: bool` (default true), `auto_flow: bool`, `details_open`, `details_fullscreen`, `activity_open`, `selector_open`, `flows: Vec<flows::FlowEntry>`, `selector_cursor: usize`, `flashes: HashMap<u32, Instant>` (1.5 s), `pending_changes: usize` (changes seen while follow is paused), `warning: Option<String>`, `source_error: Option<String>`, `stale_agents: HashSet<String>`, `theme: Theme`, `nav: Option<Box<dyn Navigator>>`.
  - `Action` includes the moves, view/orientation/follow/auto-flow/overlay toggles, the selector cursor moves, and `SwitchFlow(String)` (task 27 emits it, task 34 handles it).
  - `enum Dir` and `trait Navigator { fn neighbor(&self, from: u32, dir: Dir) -> Option<u32>; }` live here; the active view sets the navigator each frame.
  - With follow on, `apply_snapshot` reselects the frontier (the first `in-progress` task by (layer, id), else the first `ready`). A manual move sets `follow = false`.
  - `apply_snapshot` recomputes `stale_agents`: `running` rows whose `transcript_path` mtime is older than `config.stale_after_s` (a missing file counts as stale).
  - `needs_tick(now)` is true while any flash is live or any `running` agent is not stale.
  - Keys (`KeyEventKind::Release` dropped): `j/k/h/l` and arrows, `Tab`/`BackTab` for views, `Enter` for details, `o` orientation, `f` follow, `s` selector, `a` auto-flow, `t` activity, `q`/`Esc` to close an overlay or quit.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml app::` and `keys::` together report ≥7 passed: follow reselects on snapshot, manual nav pauses follow, flash expiry, `needs_tick`, a stale running agent keeps no tick alive, Release ignored, Tab cycles (predicted, unverified).

### 22. Render the layer view [M]
- **Files**: `glimpse/src/view/layers.rs`
- **Depends on**: 21
- **Action**: Implement the layer-list widget and its `Navigator` in both orientations.
- **Detail**:
  - Vertical layout:
    - A `── L<n> ──` header row per layer. After the layer that completes a checkpoint group, a separator row showing the checkpoint id, the short commits and a `✓`/`✗` from `checkpoint.verification`.
    - One row per task: a gutter mark (`↑` needs, `↓` dependents, `⇡`/`⇣` coupling, relative to the selection), a status glyph (`○ ◐ ✓ ✗ ⏸`), the id, the truncated title, the effort, an agent chip (type initial + elapsed for a running agent, dimmed when the agent is in `app.stale_agents`), and record badges (`⚠` deviation, `⏸` deferral).
    - With a selection, unrelated rows are dimmed.
  - Horizontal: layers become columns of compact `◐14 title…` cells.
  - The navigator moves within the layer along the cross axis and between layers along the layer axis. Scrolling keeps the selection visible.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml view::layers` reports ≥5 passed: `TestBackend` renders of the fixture at 100×30 (vertical) and 160×20 (horizontal) contain every task id and the checkpoint row; gutter marks for a selected task; navigation (predicted, unverified).

### 23. Render task details with markdown [M]
- **Files**: `glimpse/src/view/markdown.rs`, `glimpse/src/view/details.rs`
- **Depends on**: 21
- **Action**: Implement `markdown::to_text(&str, &Theme) -> Text<'static>` over pulldown-cmark events, and the scrollable details panel.
- **Detail**:
  - The markdown renderer handles paragraphs, nested bullet lists (indented), inline `code`, **strong**, *emphasis* and soft/hard breaks, leaving wrapping to `Paragraph::wrap`.
  - Details sections: title/status/effort/checkpoint/phase; Files; Needs, Coupling and Dependents with status glyphs; Action; Detail; Acceptance; deps note; Record (type, date, summary; deviation `original_intent`/`rationale`); Agents (type, kind, status, segments with durations, context tokens, summary).
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml view::markdown` and `view::details` together report ≥5 passed (predicted, unverified).

### 24. Render the header strip [S]
- **Files**: `glimpse/src/view/header.rs`
- **Depends on**: 21
- **Action**: Implement the one- or two-row header.
- **Detail**: Shows the slug, `flow_status`, the policy cadence, done/total counts, `⟳ follow` or `⏸ paused (+N)`, the number of running agents, and the latest flow-level record event (`status-transition` or `reconcile`). The source error or config warning appears in place of the second row.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml view::header` reports ≥2 passed (predicted, unverified).

### 25. Tail the selected agent's transcript [M]
- **Files**: `glimpse/src/transcript.rs`, `glimpse/src/view/activity.rs`
- **Depends on**: 21
- **Action**: Implement an incremental transcript tail (`TailState { path, offset, entries }`, `refresh()` reads only the bytes after `offset`, first read capped at the last 64 KiB) and the activity panel for the selected task's newest running agent.
- **Detail**:
  - Entries are `{ts, kind: ToolUse { name, detail } | Text, text}`, taken from assistant lines. `detail` is `input.description`, else `input.command`, else `input.file_path`, truncated.
  - The path must canonicalise under `claude_dir()`, else nothing is read.
  - The panel shows elapsed time since `started_at`, the last ~10 entries, and `context_tokens`.
  - Module doc carries the version anchor "Observed against Claude Code 2.1.283, 2026-09-28."
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml transcript::` and `view::activity` together report ≥4 passed: incremental offset, partial last line held back, path outside the claude dir rejected, panel render (predicted, unverified).

### Phase C — diagram, herdr integration and wiring (glimpse/)

### 26. Render the traversal view [M]
- **Files**: `glimpse/src/view/ego.rs`
- **Depends on**: 21
- **Action**: Implement the traversal widget and its `Navigator`.
- **Detail**:
  - Horizontal: the selected task sits in a centre card, with a `needs` column on the left (coupling entries dashed `┄`) and a dependents column on the right.
  - Vertical: needs sit above and dependents below.
  - Moving across columns focuses that column's item, which becomes the new centre on the next frame. Moving along a column walks its items.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml view::ego` reports ≥4 passed (predicted, unverified).

### 27. Pick a flow from the selector [S]
- **Files**: `glimpse/src/view/selector.rs`
- **Depends on**: 19, 21
- **Action**: Implement the selector overlay: flows from `flows::list`, newest first, each with status and updated date, the current one marked, and the auto-follow state shown. `Enter` emits `Action::SwitchFlow(slug)`.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml view::selector` reports ≥2 passed (predicted, unverified).

### 28. Order diagram layers to reduce crossings [M]
- **Files**: `glimpse/src/diagram/order.rs`
- **Depends on**: 15
- **Action**: Implement `order(layers: &[Vec<u32>], edges: &[Edge { from, to, kind }]) -> Ordered { rows: Vec<Vec<Slot>>, chains }`. A `Slot` is `Task(u32)` or `Dummy { edge, step }`.
- **Detail**:
  - Insert one dummy per intermediate layer for every edge spanning more than one layer.
  - Run 24 iterations of alternating median down/up sweeps, each followed by adjacent-swap transpose passes, keeping the ordering with the fewest crossings (pairwise count per channel).
  - Break ties by task id, so the result is deterministic.
  - Own input types only; the module does not depend on `model`.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml diagram::order` reports ≥4 passed: a crafted two-layer graph with 2 crossings is reordered to 0; the dummy count for a span-3 edge is 2; identical output across runs; the input layer membership is preserved (predicted, unverified).

### 29. Assign diagram coordinates [M]
- **Files**: `glimpse/src/diagram/position.rs`
- **Depends on**: 28
- **Action**: Implement `position(&Ordered, label_width: impl Fn(u32) -> u16) -> Positions { x: HashMap<Slot, u16>, layer_extent }` using Sugiyama's priority method.
- **Detail**: Nodes have width `label_width` and pitch width + 2. Dummies have width 1 and pitch 2. Dummies get the highest priority so long edges stay straight. No two slots in a layer overlap.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml diagram::position` reports ≥3 passed: no overlap, minimum spacing respected, a dummy chain shares one x where unobstructed (predicted, unverified).

### 30. Resolve junction glyphs [S]
- **Files**: `glimpse/src/diagram/glyph.rs`
- **Depends on**: 15
- **Action**: Implement `ArmMask` (N/E/S/W bits), `CellOwners` (up to 4 owners of `(edge_group, ArmMask, dashed)`), and `glyph(&CellOwners, rounded: bool) -> char`.
- **Detail**: Merge the arms of owners in the same source group. For owners from different groups, draw the vertical and break the horizontal. Dashed edges use `┄`/`┆` for straight segments. Arrowhead chars are `▼ ▲ ▶ ◀`.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml diagram::glyph` reports ≥3 passed: an exhaustive table for all 16 masks, the unrelated-crossing rule, dashed straight segments (predicted, unverified).

### 31. Route diagram edges through inter-layer channels [L]
- **Files**: `glimpse/src/diagram/route.rs`
- **Depends on**: 29, 30
- **Action**: Implement `route(&Ordered, &Positions) -> Routed { channel_depth: Vec<u16>, cells: HashMap<(u16, u16), CellOwners>, edge_cells: HashMap<EdgeId, Vec<(u16, u16)>> }` in abstract (layer axis, cross axis) coordinates.
- **Detail**:
  - Each source gets one horizontal bus per channel, with `coupling` edges in their own bus group.
  - Overlapping buses are ordered through a dependency graph that prefers the cheaper crossing, with cycles broken by id, and tracks are assigned by longest path. Channel depth = tracks + 1.
  - Each edge drops from its source, runs along its bus track, and rises into its target with an arrowhead cell.
  - `edge_cells` records every cell an edge owns, for highlighting.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml diagram::route` reports ≥4 passed: overlapping buses get distinct tracks; channel depth = tracks + 1; every edge's cells form a connected path from source to target; coupling buses are separate (predicted, unverified).

### 32. Paint and navigate the diagram [L]
- **Files**: `glimpse/src/diagram/mod.rs`, `glimpse/src/diagram/paint.rs`
- **Depends on**: 21, 31
- **Action**: Add `DiagramCache` (the layout is recomputed only when `(topology_hash, orientation)` changes; a `computations` counter is exposed for tests), the orientation mapping, a `Navigator` over (layer, slot) that skips dummies, and a widget that paints into the ratatui `Buffer`.
- **Detail**:
  - Layers come from `snapshot.layers` verbatim.
  - Orientation mapping: vertical puts the layer axis on rows; horizontal puts it on columns (swap axes and use the matching glyph orientation).
  - Nodes are drawn as `[14]` in their status style, with the flash style while flashing.
  - The selected node's in-edges (from `edge_cells`) are highlighted in the needs style and out-edges in the dependents style; all other cells are dimmed.
  - The viewport scrolls to keep the selection visible.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml -- diagram::tests diagram::paint` reports ≥5 passed (tests may sit in `glimpse/src/diagram/mod.rs`'s `mod tests` or in `glimpse/src/diagram/paint.rs`):
  - A status-only snapshot change leaves `computations` unchanged; an orientation flip increments it.
  - A `TestBackend` render of the fixture in both orientations shows every `[id]`.
  - Navigation right/down reaches every task.

  (Predicted, unverified.)

### 33. Compose the frame and switch views [M]
- **Files**: `glimpse/src/view/mod.rs`
- **Depends on**: 22, 23, 24, 25, 26, 27, 32
- **Action**: Implement `render(frame, &mut App, &Config, &mut DiagramCache, &mut TailState)`.
- **Detail**:
  - The frame has the header, then the body (the active view), then a one-row footer of key hints.
  - Details and activity split to the right when `width ≥ 2 × height`, else below; `Enter` pressed twice makes them full-screen.
  - The selector draws as a centred overlay.
  - Orientation is resolved per frame (the override, else `config.orientation`, else auto via `orientation_threshold`).
  - Installs the active view's navigator into `App`.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml view::tests` reports ≥4 passed: each view renders the fixture; details split side versus bottom by aspect; the selector overlay shows (predicted, unverified).

### 34. Run the event loop [L]
- **Files**: `glimpse/src/runtime.rs`
- **Depends on**: 18, 33
- **Action**: Implement `run(opts) -> Result<()>` and `render_once(opts, snapshot, width, height) -> String`.
- **Detail**:
  - `run`:
    - `ratatui::init()`/`ratatui::restore()`; `init` installs the panic hook.
    - One input thread owns blocking `crossterm::event::read()` and forwards each event as `source::Event::Input`; the `source` poller thread sends on the same channel.
    - The main loop blocks on `recv()`, or `recv_timeout(1s)` while `app.needs_tick()`. It drains all pending events, then draws once.
    - `Resize` triggers a redraw. `SwitchFlow` sends `SetSlug`. `FlowsChanged` re-runs `flows::list` into `app.flows`; with auto-flow on, it switches when `flows::freshest` changes. Nothing else runs `flows::list`, so an idle flow costs no process spawns.
  - `render_once` draws one frame into a `TestBackend` and returns its lines as plain text, without ANSI.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml runtime::` reports ≥2 passed: `render_once` of the fixture contains the header slug and every task id; a drain-then-draw loop over a scripted channel draws once for a burst of 3 resize events (predicted, unverified).

### 35. Wrap the herdr CLI [S]
- **Files**: `glimpse/src/herdr.rs`
- **Depends on**: 15
- **Action**: Implement `Herdr { bin }` (`$HERDR_BIN_PATH`, else `herdr`) with `pane_list`, `pane_get`, `layout(pane_id) -> Rect` (`herdr pane layout`; `pane list`/`get` carry no rect), `split(origin, dir, ratio, cwd) -> pane_id` (always `--no-focus`), `focus`, `rename`, and `run`. `run` types its text plus Enter into the pane's shell (`herdr pane run` sends input; it does not spawn a process).
- **Detail**:
  - herdr output is JSON `{"id":…,"result":…}`: `pane list` → `.result.panes[]`, `pane get` → `.result.pane`, `pane split` → `.result.pane.pane_id`. Errors come as JSON on stderr with exit 1.
  - `PaneInfo` includes `pane_id`, `tab_id`, `label`, `cwd` and `rect` (from `pane layout`).
  - Parse functions are pure and tested on captured samples.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml herdr::` reports ≥3 passed (predicted, unverified).

### 36. Ensure one glimpse pane per tab [M]
- **Files**: `glimpse/src/pane.rs`
- **Depends on**: 17, 35
- **Action**: Implement `ensure(herdr, origin_pane, slug: Option<&str>, cwd, focus, config) -> Result<PaneOutcome>`. `None` (the keybinding with no `--slug`) launches glimpse on the freshest flow with auto-flow on.
- **Detail**:
  1. Take an exclusive lock with `File::create_new` on `<claude_dir>/glimpse/pane.lock`. A lock older than 10 s is removed; wait at most 3 s.
  2. Look for a pane labelled `glimpse` in the origin's tab. If found, focus it when asked; done.
  3. Otherwise choose the direction from the origin rect (`herdr.layout(origin_pane)`): `right` if `width ≥ split_threshold × height`, else `down`.
  4. `split` at `pane_ratio`, `rename` to `glimpse`, then `run` the line built by a pure `launch_line(current_exe, slug)`. `herdr pane run` types the text plus Enter into the pane's default shell (pwsh on this machine), where a line opening on a quoted string is a parse error, so `launch_line` emits the bare path when it has no whitespace, else `& '<exe>'`, followed by `--slug <slug>` when a slug is given.
  5. Release the lock.
  - The pure functions `choose_direction`, `find_existing` and `launch_line` are tested.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml pane::` reports ≥6 passed: direction at 100×56 → `down`, at 240×50 → `right`; an existing labelled pane in the same tab is reused; one in another tab is ignored; a stale lock is reclaimed; `launch_line` gives a bare path for `C:/x/glimpse.exe` and a `& '…'` form for a path with a space (predicted, unverified).

### 37. Handle hook events [M]
- **Files**: `glimpse/src/hook.rs`
- **Depends on**: 36
- **Action**: Implement `run_hook(stdin) -> ()`.
- **Detail**:
  1. Read stdin (capped at 32 MiB).
  2. Spawn `<config.tomlctl> agents record --harness claude-code -` with `current_dir(payload.cwd)` and pipe the payload in.
  3. Parse the one-line result.
  4. `decide(result, env)` returns `EnsurePane { slug, cwd }` only for `recorded && event == "start"` with `HERDR_PANE_ID` set.
  5. Append errors, one timestamped line each, to `<claude_dir>/glimpse/hook.log` (truncated to empty when over 1 MiB). An "unknown subcommand" from an old tomlctl is logged as "tomlctl ≥0.12.0 required — cargo install --path tomlctl".
  - Never write to stdout or stderr.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml hook::` reports ≥4 passed: `decide` for start with herdr set, start without herdr, stop, and not recorded; log rotation (predicted, unverified).

### 38. Install hooks and the herdr keybinding [M]
- **Files**: `glimpse/src/setup.rs`
- **Depends on**: 17
- **Action**: Implement `setup(dry_run) -> Result<Report>` with pure `merge_settings(Value, exe) -> (Value, changed)` and `merge_herdr(&str, exe) -> (String, changed)`.
- **Detail**:
  - Settings (`<claude_dir>/settings.json`): for each of `SubagentStart`, `SubagentStop` and `TeammateIdle`, append a matcher-less group `{"hooks":[{"type":"command","command":"<exe>","args":["hook"],"async":true}]}` — exec form, so Claude Code spawns the exe directly with no shell and no Git Bash process per event (hooks.md, "Exec form and shell form") — unless a hook whose `command` contains `glimpse` and whose `args` (or `command`) contains `hook` already exists. `exe` is `current_exe()`. Key order is preserved (`preserve_order`); write pretty-printed.
  - herdr (`%APPDATA%/herdr/config.toml` on Windows, else `~/.config/herdr/config.toml`): unless the text contains `glimpse`, append

    ```toml
    [[keys.command]]
    key = "prefix+alt+g"
    type = "popup"
    command = '<exe> ensure-pane --focus'
    description = "glimpse task graph"
    width = 40
    height = 6
    ```

    as text, so existing comments survive.
  - Write each file atomically (a temp file in the same directory, then rename), backing it up first (`<file>.bak-glimpse-<unix-ts>`) only when the merge changed it; then run `herdr server reload-config`. `--dry-run` writes nothing and reports the planned changes.
  - Claude Code's file watcher normally applies hook edits to running sessions (hooks.md, "Disable or remove hooks"); the report says to restart Claude Code only if the hooks do not fire.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml setup::` reports ≥6 passed: hooks added to a settings file with unrelated keys (order kept), in exec form with `args = ["hook"]`; a second merge is a no-op; a merge over an existing shell-form `glimpse hook` entry is also a no-op; an existing SessionStart hook is untouched; the appended herdr TOML parses with `toml`; a second herdr merge is a no-op (predicted, unverified).

### 39. Parse arguments and wire the subcommands [M]
- **Files**: `glimpse/src/main.rs`, `glimpse/src/cli.rs`, `glimpse/tests/cli.rs`
- **Depends on**: 34, 37, 38
- **Action**: Implement the hand-rolled parser (pattern: `statusline/src/cli.rs`) and route every subcommand from `main`. Remove the scaffold's `#![allow(dead_code)]`.
- **Detail**:
  - Commands:
    - `glimpse [--slug S] [--view layers|ego|diagram] [--orientation auto|vertical|horizontal] [--once [--size WxH] [--snapshot <file>]]`
    - `glimpse hook`
    - `glimpse ensure-pane [--slug S] [--focus]`
    - `glimpse setup [--dry-run]`
    - `--help`, `--version`
  - Exit codes: 0 on success, 2 on a usage error.
  - Without `--slug`, glimpse opens the freshest flow with auto-flow on.
  - `ensure-pane` uses `HERDR_ACTIVE_PANE_ID` (falling back to `HERDR_PANE_ID`), takes the cwd from `herdr pane get`, and exits 1 with a message outside herdr.
  - `--once --snapshot <file>` renders from a file without running tomlctl.
  - `tests/cli.rs` drives `env!("CARGO_BIN_EXE_glimpse")` (pattern: `statusline/tests/cli.rs`) with these cases:
    - `--help` → 0;
    - an unknown flag → 2;
    - `hook` with empty stdin → 0, empty stdout/stderr, and one line in the temp dir's `glimpse/hook.log`;
    - `--once --snapshot tests/fixtures/snapshot.json --size 100x30` prints every fixture task id;
    - `--once --snapshot tests/fixtures/snapshot.json --view diagram --size 100x30` prints `[1]`;
    - `setup --dry-run` writes nothing.
  - Every case sets `CLAUDE_CONFIG_DIR`, `GLIMPSE_CONFIG` and `APPDATA` to a temp dir, so no case reads the real config, writes the real `hook.log`, or runs the installed `tomlctl` (still 0.11.0 until the smoke check installs it).
  - `ensure-pane` without `--slug` passes `None` to `pane::ensure`.
- **Acceptance**: `cargo test --manifest-path glimpse/Cargo.toml --test cli` reports ≥6 passed (predicted, unverified).

### 40. Document glimpse [S]
- **Files**: `glimpse/README.md`, `glimpse/CLAUDE.md`, `CLAUDE.md`
- **Depends on**: 39, 41
- **Action**:
  - Write `glimpse/README.md`: install (`cargo install --path tomlctl` then `cargo install --path glimpse`), `glimpse setup`, restarting Claude Code only if the hooks do not fire, the keybinding, config keys, views and keys, and the data flow.
  - Write `glimpse/CLAUDE.md`: hook edits are normally hot-reloaded by Claude Code's file watcher, with a restart as the fallback; the hooks are exec form so no shell runs per event; async hooks are silent, so check `hook.log`; transcripts follow the version anchor; the layout cache key; `--once` for headless checks.
  - Add `glimpse/` to the root `CLAUDE.md` Sibling crates list (`CLAUDE.md:63-68`), and mention task 41's glimpse fmt step in the root `CLAUDE.md` Developer setup paragraph that lists the hook's steps.
- **Detail**: Follow `.claude/rules/documentation.md`: measurements are commands, not numbers.
- **Acceptance**: `grep -c 'glimpse/' CLAUDE.md` ≥1 (today 0), and `test -s glimpse/README.md && test -s glimpse/CLAUDE.md` exits 0 (today 1).

### 41. Gate glimpse formatting in pre-commit [S]
- **Files**: `.githooks/pre-commit`
- **Depends on**: 15
- **Action**: When the staged set contains a `glimpse/**/*.rs` path, run `gate cargo fmt --manifest-path "$ROOT/glimpse/Cargo.toml" -- --check` in a block after the `gate()` definition in `.githooks/pre-commit`, so a diff blocks the commit and names the rerun command. (The tomlctl fmt step above `gate()` is a bare `cargo fmt`, and already fires on any staged `*.rs`, glimpse included.)
- **Detail**: This file runs unsandboxed on every commit (root `CLAUDE.md`, Supply chain), so keep the change to one guarded block and add no new executables.
- **Acceptance**: `grep -c 'glimpse/Cargo.toml' .githooks/pre-commit` ≥1 (today 0) and `bash -n .githooks/pre-commit` exits 0.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 1, 2, 10, 11, 12, 13, 14 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14. The tomlctl data layer is complete: the agents store with its record/list verbs, `tasks snapshot`, docs, 0.12.0, and the `B-22bd6fc6` parser fix. tomlctl builds, its full suite and the plan corpus pass, and nothing in glimpse is needed.

— CHECKPOINT B after tasks 18, 22, 23, 24, 25, 41 — dependency closure: 2, 3, 5, 7, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 41. The glimpse core: scaffold, model, config/theme, source, flows, diff, app/keys, the layer, details, header and activity renderers, and the pre-commit fmt gate (committed here so an uncommitted hook edit never gates a later train). The crate builds and its unit tests pass; it is not yet wired to `main`.

— CHECKPOINT C after tasks 40 — dependency closure: 2, 3, 5, 7, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41. The diagram, traversal view, selector, frame composition, event loop, herdr/hook/setup, CLI wiring and docs. `glimpse` runs end to end.

## Risks

- **About 70 unique files, well over the ~25-file guideline.** The user chose one plan. Mitigations: the milestone cuts A/B/C each gate a buildable increment; most tasks touch 1–2 files; the only wide tasks are the scaffold (15, stubs only) and the compile-coupled wiring (9, 12).
- **Task 1 changes what older plans import.** The plans using `**Files:**` (see the command in task 1) will import claims and edges on their next import, which could surface cycles, dangling edges or unexpected findings. Mitigation: the corpus test is part of task 1's acceptance; the task stops and reports rather than editing plans.
- **Undocumented Claude Code on-disk formats** (transcript lines, `meta.json`) can change between builds. Mitigations: correlation and tailing degrade to absent, never erroring; both modules carry a version anchor; hook payload fields come from documented hooks.md fields only.
- **Concurrent hooks.** Six simultaneous `SubagentStart` hooks race on `agents.toml` and on pane creation. Mitigations: the `mutate_doc` exclusive lock (30 s timeout, generous for async hooks) and the `pane.lock` file plus re-check in task 36.
- **Silent hook failures.** Claude Code ignores async hook exit codes and output. Mitigation: `hook.log` with rotation, and the manual smoke step.
- **A teammate's `SubagentStart` whose new message carries no `tasks show` line** (e.g. a follow-up nudge). The open segment stays unchanged because the dispatch is `None`, so the agent is not re-attributed to the wrong task.
- **Hand-rolled diagram quality.** A real 30-node flow may still read as cluttered. Mitigations: the layer view is the default; the diagram is cached and optional; Brandes–Köpf is the named upgrade path. The orchestrator can compare the `--once --view diagram` output at checkpoint C before calling the view done.
- **`.github` mirror drift.** Tasks 13 and 14 edit mirrored skills, so the git-tracked `.github/skills/**` copies (reached through junctions on disk) show as diffs. Both tasks list those paths on their Files lines, so the commit train stages them with the task instead of halting on them.
- **User-level hooks fire in every repo and session.** Mitigations: exec-form hooks (no shell process per event), `record` refusing any slug without a `context.toml` (so no stray flow directories), and a `stopped` row for a Stop that outruns its Start, with glimpse treating a long-silent `running` row as stale.
- **`glimpse setup` edits user-level files** (`~/.claude/settings.json`, herdr `config.toml`). Mitigations: backups, `--dry-run`, idempotent merges, and text-append for the TOML so comments survive. `settings.json` is re-emitted pretty-printed, so its whitespace may change.
