# tomlctl — agents reference

The flag surface of the `tomlctl agents` group and the shape of the store it writes:
`.claude/flows/<slug>/agents.toml`, a per-flow log of the subagents and teammates a flow's
runs dispatched, one row per agent with one segment per assignment. The rows are written by
harness hooks as agents start, idle and stop, and read back by `agents list` and by
`tasks snapshot`, which carries them verbatim beside the task rows for a viewer.

**Writer rule.** `agents record` is run only from harness hooks — the async
`SubagentStart`, `SubagentStop` and `TeammateIdle` hooks that `glimpse setup` installs in the
user's Claude Code settings, and the `SubagentStart` and `SubagentStop` hooks it installs in
`<CODEX_HOME>/hooks.json`, which pass `--harness codex`. No carrier, orchestrator or sub-agent calls it, and nothing else
writes the store: `items`, `set` and `set-json` are never pointed at it. A flow that needs to
know what an agent did reads the execution record, which stays the durable audit trail.

**Local telemetry.** `agents.toml` and its `.sha256` sidecar are gitignored by the targeted
pattern `.claude/flows/*/agents.toml*`, so the store never churns a commit and is not shared
between clones. Do not ignore `.claude/` wholesale to achieve the same — `flow doctor` flags
that.

## Contents

- [`agents record`](#agents-record)
- [`agents list`](#agents-list)
- [Store shape](#store-shape)
- [Events](#events)
- [Flow selection](#flow-selection)
- [Task attribution](#task-attribution)
- [Output](#output)

`agents record` carries the shared write bundle — `--allow-outside`, `--no-create`,
`--no-write-integrity`, `--strict-integrity`, `--verify-integrity` — and no `--dry-run`.
`agents list` carries the read bundle, `--verify-integrity` and `--strict-read`. Both resolve
the store from the repo root rather than from the working directory, so the store lands inside
the `.claude/` containment guard and neither needs `--allow-outside`. Both also take the
global `--error-format text|json`; see [flow.md](flow.md#error-format---error-format-json) for
the JSON envelope and its `kind` taxonomy.

## `agents record`

Reads one hook payload and turns it into a change to the owning flow's `agents.toml`, or into
a `recorded: false` line naming why nothing was written.

```bash
printf '{"hook_event_name":"SubagentStop","session_id":"<session>","agent_id":"<agent>","agent_type":"implement-deep","cwd":"<repo>","transcript_path":"<session-transcript>"}' | tomlctl agents record --harness claude-code -
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | JSON or `-` | The hook payload, inline or `-` for stdin. A hook pipes it on stdin. | `-` |
| `--harness` | `claude-code` \| `codex` \| `manual` | Harness that emitted the payload. Case-sensitive. Adapters exist for `claude-code` and `codex`; `manual` is accepted by the parser and then refused with a non-zero exit, and any other value errors naming the vocabulary. | required |

- **Repo root.** When the payload's `cwd` names an existing directory, the process moves
  there before anything resolves the repo root, so a hook fired from a subdirectory or a
  second worktree writes to that checkout's `.claude/flows/`.
- **Locking.** The owning store is re-read and written under the exclusive `tomlctl` lock, and
  record ids are minted inside it, so concurrent hooks racing on one store never mint the
  same id. The unlocked scan that picks the flow skips any store it cannot read.
- **Auto-create.** A missing `agents.toml` is seeded like the other recognised flow files
  (`schema_version = 1`, `last_updated = <today>`) — see
  [write.md](write.md#auto-create-on-first-write). `--no-create` restores the strict
  `kind=not_found` error; hooks never pass it.
- **Unchanged writes are skipped.** A repeated start on the same task set leaves the store
  byte-identical, so no write and no sidecar rewrite happens, yet the output still reports
  `recorded: true`.

## `agents list`

Prints the flow's agent records as a JSON array, in store order.

```bash
tomlctl agents list --slug <slug>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` | slug | Flow whose `agents.toml` is read. Must match `^[a-z0-9][a-z0-9-]{0,63}$`. | required |

A missing store prints `[]` — no hook has fired for the flow yet — unless `--strict-read`
asks for `kind=not_found`. Rows pass through the store schema on the way out, so every
[record field](#record-fields) is present on every row, keys a hand edit added are dropped,
and a malformed store or one written by a newer `tomlctl` errors instead of being echoed.

## Store shape

```toml
schema_version = 1
last_updated = 2026-09-28

[[agents]]
id = "A3"
harness = "claude-code"
session_id = "c3349435-…"
agent_id = "a007fe3fdba915afd"
agent_type = "implement-deep"
kind = "subagent"
name = ""
team = ""
status = "stopped"
started_at = "2026-09-28T04:44:22Z"
updated_at = "2026-09-28T04:52:08Z"
ended_at = "2026-09-28T04:52:08Z"
transcript_path = "C:/Users/…/subagents/agent-a007fe3fdba915afd.jsonl"
summary = "…"
context_tokens = 48213

[[agents.segments]]
task_ids = [16]
started_at = "2026-09-28T04:44:22Z"
ended_at = "2026-09-28T04:52:08Z"
```

| Key | Value | Meaning |
|---|---|---|
| `schema_version` | integer | `1`. A store stamped with a higher version is refused on every read and write — the next write would drop its unknown keys. |
| `last_updated` | bare date | Stamped by every write that changes the store. |
| `agents` | array of tables | One row per `(session_id, agent_id)`. Named `agents`, never `items`: an `items` array under `.claude/` is the default target of `tomlctl items add \| update \| apply`, whose `dedup_id` stamping would touch every row. |

The writer lays `segments` out as `[[agents.segments]]` blocks; the reader accepts the inline
`segments = [ { task_ids = [16], started_at = "…", ended_at = "…" } ]` form too. The store is
tool-owned: every write re-emits it from the schema, so keys are written in the order shown
and any key not listed here is dropped.

### Record fields

| Field | Value | Meaning |
|---|---|---|
| `id` | `A<n>` | `A` plus one more than the largest well-formed id in the store; `A1` on an empty one. A gap is never refilled. A row without an `id` refuses the store. |
| `harness` | `claude-code` \| `codex` \| `manual` | Harness whose hook created the row — the `--harness` it was recorded under. Absent reads as `claude-code`. |
| `session_id` | text | The parent session the hook fired in. Under Codex, the id its root thread and every descendant share. |
| `agent_id` | text | The harness's agent id — under Codex, the child's thread id. With `session_id`, the key a start or stop event matches on. |
| `agent_type` | text | The agent's type, e.g. `implement-deep`, `research-lite`, `verification`. Under Codex, the spawn's agent role, `default` when none was given. |
| `kind` | `subagent` \| `teammate` | `teammate` when the agent's `.meta.json` carries `taskKind = "in_process_teammate"` — a named `Agent` spawn — else `subagent`. Absent reads as `subagent`. Once a row is a teammate, a later start never demotes it. Codex writes no `.meta.json`, so its rows are always `subagent`, with `name` and `team` left `""`. |
| `name` | text | Teammate name; `""` for a subagent. The key an idle event matches on. |
| `team` | text | Team name from the `.meta.json`; informational only, never matched. |
| `status` | `running` \| `idle` \| `stopped` | See [Events](#events). Absent reads as `running`. |
| `started_at` | RFC 3339 UTC | When the row was created. Every timestamp in the store is the time `tomlctl` handled the event, not a time taken from the payload. |
| `updated_at` | RFC 3339 UTC | The last event that changed the row. Breaks ties in [flow selection](#flow-selection). |
| `ended_at` | RFC 3339 UTC or `""` | Set when a subagent stops; cleared by a later start. A teammate's stays `""`. |
| `transcript_path` | path | The agent's own transcript, canonicalised. |
| `summary` | text | The stop payload's `last_assistant_message`, trimmed to 600 characters with line endings folded to LF. A stop without one keeps the previous summary. |
| `context_tokens` | integer | Context size at the agent's last assistant turn: that turn's input, cache-read, cache-creation and output token counts summed. Under Codex, the `total_tokens` of the rollout's last `token_count` event or `token_usage_record` — its input count already includes cached tokens. `0` until a stop reads it; a malformed value reads as `0`. |
| `segments` | array of tables | One per assignment, oldest first. |

An unknown `harness`, `kind` or `status` value refuses the whole store, naming the row.

### Segment fields

| Field | Value | Meaning |
|---|---|---|
| `task_ids` | task ids | The flow tasks the assignment covered, sorted and deduplicated. More than one id is a cluster dispatch; `[]` is an agent with no task-level dispatch, such as a verification agent attached by session affinity. |
| `started_at` | RFC 3339 UTC | When the assignment opened. |
| `ended_at` | RFC 3339 UTC or `""` | When it closed. At most one segment is open — the last, while its `ended_at` is `""`. |

## Events

Both adapters read the payload's `hook_event_name`. Any other event prints
`{"recorded":false,"reason":"unsupported-event"}`, and so does `TeammateIdle` under
`--harness codex`, which has no teammates.

| Hook event | Label | Effect |
|---|---|---|
| `SubagentStart` | `start` | Upserts the row on `(session_id, agent_id)` and opens a segment on the dispatched task set, closing any open one first. If the open segment already covers the same set and the row is `running`, nothing changes. Sets `running` and clears `ended_at`. A start whose time is earlier than the `ended_at` already recorded on a `stopped` row is ignored and prints `{"recorded":false,"reason":"stale-start"}`; a resume started after the stop still reopens the row. Fills `agent_type`, `transcript_path`, `name` and `team` from the payload and `.meta.json`, never blanking a value an earlier event recorded. Claude Code fires this on a spawn, on a resume, and each time a teammate takes a new message, so this one path covers re-tasking a pool worker. |
| `SubagentStop` | `stop` | Closes the open segment and sets `stopped` with `ended_at` for a subagent, or `idle` for a teammate — a teammate that stops is parked, not finished. Stores the summary and the context size. A stop whose agent no row holds (it outran its own start) creates the row only when its transcript's dispatch names a flow; otherwise it prints `no-flow`. |
| `TeammateIdle` | `idle` | Finds the teammate's latest row by `(session_id, teammate_name)` — `team_name` is not matched — closes its open segment and sets `idle`. Never creates a row. |

**Reaping.** Every recorded event also reaps the store it wrote to: any other `running` row
whose transcript has not changed for an hour is set to `stopped`, with `ended_at` and its open
segment's end at the transcript's mtime and `updated_at` at the event's time. A row with no
`transcript_path`, or whose mtime cannot be read, is left alone.

A `SubagentStart` or `SubagentStop` with an empty `agent_type` is a harness-internal agent and
prints `{"recorded":false,"reason":"internal-agent"}` without reading anything else.

Codex fires `SubagentStart` only when a child thread is spawned or forked, and `SubagentStop`
at the end of every child turn, so a child given a follow-up message stops again without a
fresh start: the row is re-stamped `stopped` and keeps its closed segment. Codex fires neither
for its internal agents.

## Flow selection

A payload names no flow, so `record` works out which flow's store owns the event. It first
reads the agent's transcript — the payload's `agent_transcript_path` when present, else
`<dirname(transcript_path)>/<session_id>/subagents/agent-<agent_id>.jsonl`, waiting up to a
second for it to appear — and accepts it only when it canonicalises inside the session's own
transcript directory. An idle event uses the `transcript_path` already on the teammate's row.

Under `--harness codex` the transcript is the child's rollout: the stop payload's
`agent_transcript_path`, and the start payload's `transcript_path`, which on a start names the
child's own rollout rather than the parent's. Codex files rollouts by date under
`<CODEX_HOME>/sessions/YYYY/MM/DD/`, so no parent directory contains them; a rollout is
accepted only when its canonical file name is `rollout-*.jsonl` and carries the payload's
`agent_id`. Either path may be `null`, which leaves the event with no transcript.

The **dispatch** is the newest user-authored line in that transcript carrying at least one
`tasks show <id> --slug <slug>` command — the fetch line every flow dispatch prompt contains.
In a Codex rollout that is a user-role `response_item` message's `input_text`, or an
`inter_agent_communication` item's `content`.
Its ids become the segment's task set. A line whose `tasks show` commands name two different
slugs yields no dispatch, and a line that is a tool result echoing such a command is not a
prompt and is skipped.

Every flow's `agents.toml` under `<root>/.claude/flows/` is then scanned, and the owner is
chosen per event:

| Event | Owner, first match wins |
|---|---|
| start | the dispatch's slug; else the flow holding this agent's row; else **session affinity** — the flow holding this session's most recently updated row |
| stop | the flow holding this agent's row; else the dispatch's slug |
| idle | the flow holding this teammate's row |

Among several stores holding a match, the one with the most recently updated row wins.
Session affinity is what attaches an agent dispatched without a task fetch — a verification
agent, a research lens — to the flow whose run launched it.

Then:

- **No owner** prints `{"recorded":false,"reason":"no-flow"}`.
- **An owner without `<root>/.claude/flows/<slug>/context.toml`** prints
  `{"recorded":false,"reason":"unknown-flow"}` rather than creating a flow directory that
  `flow doctor` would fail.
- **An idle event whose teammate is gone** from the owning store by the time it is re-read
  under the lock prints `{"recorded":false,"reason":"unknown-agent"}`.

## Task attribution

The dispatch contributes task ids only when its slug is the chosen flow's: a dispatch for
another flow says nothing about this one's tasks, and the segment opens on `[]`.

The transcript can lag the hook that names it, so a start may still see the agent's previous
dispatch. The stop and idle paths read the dispatch again and, when one resolves for this
flow, overwrite the closing segment's `task_ids` with it. A teammate's follow-up message that
carries no `tasks show` line resolves to the same latest dispatch it already holds, so the open
segment is left alone rather than re-attributed.

## Output

`record` prints exactly one compact JSON line on stdout, the form a hook's log captures whole:

```json
{"recorded":true,"slug":"lively-twirling-babbage","event":"start","id":"A3","task_ids":[16]}
{"recorded":false,"reason":"no-flow"}
```

| Key | Value | Meaning |
|---|---|---|
| `recorded` | bool | Whether the event maps to a row. `true` on an unchanged repeated start as well. |
| `slug` | slug | Owning flow. Only when `recorded`. |
| `event` | `start` \| `stop` \| `idle` | The [event](#events) applied. Only when `recorded`. |
| `id` | `A<n>` | The row touched. Only when `recorded`. |
| `task_ids` | task ids | The row's last segment's task set after the event. Only when `recorded`. |
| `reason` | `internal-agent` \| `no-flow` \| `unknown-flow` \| `unknown-agent` \| `stale-start` \| `unsupported-event` | Why nothing was recorded. Only when not `recorded`. |

A `recorded: false` line exits `0` — declining an event is not an error. A malformed payload,
an unimplemented harness, a store that fails to parse, or a lock timeout exits non-zero with
the usual stderr error. `list` prints a pretty-printed array of the
[record fields](#record-fields), `segments` as an array of objects.
