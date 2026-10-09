---
name: tomlctl
description: "Read, write, query, batch-edit, and validate TOML files used by Claude Code flows — context.toml, review-ledger.toml, optimise-findings.toml, execution-record.toml, plan-review-findings.toml, tasks.toml, agents.toml, .claude/backlog.toml, .claude/inputs.toml — and their per-row [[items]] arrays; also the regex enumerator over git-tracked files (`sweep`, the preferred `file:line` enumerator for review and optimise findings) and the ledger-driven `items sweep` / `items clusters` that re-verify and batch findings for the apply flow. Verb groups: get/query/set/set-json/append, items, flow (resolve, doctor, envelope-build, render-progress-log), tasks (import-plan, check, render, snapshot), backlog (add, check, reconcile), inputs (add, list, ack, handle, answer, withdraw), agents (hook-written record, list), validate, integrity, dedupe. Use this for any TOML mutation in a flow command — never line-edit ledger arrays-of-tables. Outputs JSON; supports stdin via `-` sentinel for ops/json/ndjson payloads."
---

# tomlctl

> This document is the navigational entry point. Each verb's flag table lives in the sibling file listed under [References](#references); the top-level `tomlctl/README.md` is a short human tour that defers to those.

A small Rust CLI that reads and writes the TOML files used by the `/plan-new`, `/implement`, `/plan-update`, `/review`, `/optimise`, `/review-apply`, and `/optimise-apply` commands, plus the repo-scoped capture log behind `/backlog`. It also walks the git-tracked tree: `sweep` enumerates regex sites as `file:line` without reading any TOML, and `items sweep` / `items clusters` re-run a ledger's stored patterns and batch its items for the apply flow.

## When to use this skill

Every flow-TOML mutation routes through `tomlctl` — no Python, no line-level `Edit`, no `jq` for TOML parsing. Reach for it whenever a flow command needs to read, filter, or mutate `context.toml`, the review / optimise ledgers, `plan-review-findings.toml`, or their sidecar array-of-tables (`rollback_events`, task-completion records). Shell-level post-processing of tomlctl's JSON output is not needed either — use the in-tool primitives (`--get` / `--select` / `--omit` / `--template` / `--lines` / `--limit` / `--where*` / `--rows` / `--header` / `--max-chars`, see [Output options](#output-options), and `--raw` / `--count-distinct` / `--count`) instead of a pipe through `python`, `node`, `jq`, `head` or `sort -u | wc -l`. The [Guidance canon](#guidance-canon) is the full rule set.

## Quick Reference

The highest-frequency patterns. Deeper treatment lives in the reference files listed under [References](#references).

| Task | Command |
|---|---|
| Append one item, minting its id (field flags; prose staged with the Write tool) | `tomlctl items add <file> --id-prefix R --set severity=major --set-file description=<path> --get id` |
| Append one item from a staged JSON payload | `tomlctl items add <file> --id-prefix R --json '@<path>' --get id` |
| Batch append homogeneous items | `tomlctl items add-many <file> --id-prefix R --ndjson <path>` |
| Apply heterogeneous batch (add/update/remove), minting ids for id-less adds | `tomlctl items apply <file> --id-prefix R --ops <path>` |
| Append an execution-record entry (id, date, `task_ref` and caps handled) | `tomlctl flow record --slug <s> --type <t> [--task <id>] --set agent=<a> --set-file summary=<path>` |
| Group finished tasks into commits | `tomlctl tasks train --slug <s> [--checkpoint <x>]` |
| Filter items | `tomlctl items list <file> --where status=open` |
| Count / bucket items | `tomlctl items list <file> --count` / `--count-by status` / `--group-by file` |
| Regex sites over the tracked files (`file:line`, for an `Instances` line) | `tomlctl sweep -e <regex> [-e <regex>]... [--exclude <glob>]... [--max-hits <n>]` |
| Re-run items' stored `sweep` strings and diff against `instances` | `tomlctl items sweep <file> [--ids R5,R12] [--update [--dry-run]]` |
| File-disjoint clusters + dependency batches for the apply flow | `tomlctl items clusters <file> [--ids R5,R12] [--lines]` |
| Bump scalar field | `tomlctl set <file> <key.path> <value>` |
| Set several scalars in one write | `tomlctl set <file> --set <key.path>=<value> --set <key.path>=<value>` |
| Set array / sub-table | `tomlctl set-json <file> <key.path> --json '<json>'` |
| Read value via json subcommand | `tomlctl json get <file> <path>` |
| Write value via json subcommand | `tomlctl json set <file> <path> --json <value>` |
| Delete a key at path | `tomlctl json unset <file> <path>` |
| Capture / triage the repo backlog (`.claude/backlog.toml`) | `tomlctl backlog add\|add-many\|check\|list\|show\|relate\|triage\|reconcile\|cluster\|compact\|evidence {dir\|audit}` |
| Manage active-flow registry | `tomlctl flow active list\|add\|remove\|touch [--slug <s>] [--branch <b>] [--worktree <w>] [--scope <glob>]...` |
| Pre-flight envelope (resolve + doctor + plansDirectory in one dispatch) | `Task(subagent_type: "flow-bootstrap", prompt: <input-envelope-JSON>)` ([`claude/agents/flow-bootstrap.md`](../../agents/flow-bootstrap.md); entrypoint note below) |
| Build the flow-bootstrap input envelope (Step-0 of every flow carrier) | `tomlctl flow envelope build --command <c> [--branch <b>] [--worktree <w>] [--cwd <p>] [--path-arg <p>]... [--require-artifact <a>]...` |
| Run flow invariant checks (optionally auto-fix) | `tomlctl flow doctor [--slug <s>] [--fix]` |
| Report (or bootstrap) flow artifact + sidecar status | `tomlctl flow ensure-artifact --slug <s> --kind <k> [--bootstrap]` |
| Locate plan files | `tomlctl flow find-plans [--dirs <d>...] [--strict-read]` |
| Seed a new flow (context.toml + execution-record.toml + active entry) | `tomlctl flow init --slug <s> --plan <path> [--branch <b>] [--scope <glob>]...` |
| Enumerate flows under .claude/flows/ | `tomlctl flow list [--status <s>] [--branch <b>] [--active-only]` |
| Resolve the active flow (5-step algorithm, emits artifacts + scope) | `tomlctl flow resolve [--flow <s>] [--path <p>]... [--branch <b>] [--worktree <w>] [--with-staleness]` |
| Check whether a flow is stale | `tomlctl flow stale --slug <s> [--threshold <duration>]` |
| Regenerate PROGRESS-LOG.md from the execution record | `tomlctl flow render-progress-log --slug <s> [--stdout] [--verify-integrity]` |
| Import a plan's tasks into the flow's task DAG | `tomlctl tasks import-plan --slug <s> [--plan <p>] [--reconcile-record] [--dry-run]` |
| Query, mutate or gate that DAG (`.claude/flows/<slug>/tasks.toml`) | `tomlctl tasks add\|add-many\|update\|remove\|show\|list\|edges\|ready\|batches\|closure\|check\|render\|snapshot --slug <s>` |
| List a flow's agent records (`.claude/flows/<slug>/agents.toml`; written by `glimpse hook` in-process, never by a carrier; `agents record` is the manual form) | `tomlctl agents list --slug <s>` |
| Refresh integrity sidecar | `tomlctl integrity refresh <file>` |

<a id="flow-bootstrap-agent-entrypoint"></a>**`flow-bootstrap` agent entrypoint**: per-command pre-flight is delegated to the `flow-bootstrap` sub-agent (`claude/agents/flow-bootstrap.md`), which composes `tomlctl flow resolve --with-staleness`, `tomlctl flow doctor`, and (for `plan-new` / `plan-update` / `review-plan`) `tomlctl json get .claude/settings.json plansDirectory` into a single JSON envelope. Each carrier's `## Step 0: Pre-flight (flow resolution + doctor)` section dispatches via `Task` with `subagent_type: "flow-bootstrap"` and a JSON-encoded input envelope; downstream phases consume `envelope.resolved.{slug,context_path,artifacts.*,status,plan_path,scope,stale}` plus `envelope.doctor.ok` instead of running the resolve / doctor primitives inline. The agent is read-only — never passes `--fix` to doctor — so auto-repair stays an orchestrator decision.

## References

The per-verb flag tables, recipes, and contract prose live in the sibling files below. Each is self-contained and opens with its own `## Contents` list.

- [references/query.md](references/query.md) — the read-only verbs: `get` / `parse` / `validate`, the full `items list` query surface (filters, projection, shaping, aggregation, output shapes), `items get`, `items find-duplicates`, `items orphans`, and the tree-walking `sweep`, `items sweep`, `items clusters`.
- [references/write.md](references/write.md) — the mutating verbs: `set`, `set-json`, `array-append`, the `items` batch verbs, `integrity refresh`, plus auto-create, `--dry-run`, stdin payload handling, and the dedup fingerprint contract.
- [references/flow.md](references/flow.md) — the cross-cutting surface: the `--verify-integrity` support matrix, what the `.sha256` sidecar does and does not promise, the `--error-format json` envelope, the two emitting `flow` verbs, and the infrastructure-only `blocks` verbs.
- [references/backlog.md](references/backlog.md) — the `backlog` group's flag tables, the `.claude/backlog.toml` store shape, id derivation, the `check` verdict ladder, and the evidence drop-box. When to mint a row is the `backlog-capture` skill's call, not this one's.
- [references/backlog-reconcile.md](references/backlog-reconcile.md) — `backlog reconcile`: its flags, the buckets it sorts promoted rows into, the `render_needed` duty, what `--apply` writes, and the output shape.
- [references/tasks.md](references/tasks.md) — the `tasks` group's read verbs (`show` through `render`, and `snapshot`) with their flag tables, the `--slug` / `--file` target group, and the `check` finding classes. What the fields mean and which verb a carrier reaches for is the `flow-contract-task-store` skill's call, not this one's.
- [references/tasks-write.md](references/tasks-write.md) — the `tasks` group's mutating verbs: `import-plan`, `add`, `add-many`, `update`, `remove`.
- [references/tasks-store.md](references/tasks-store.md) — the `.claude/flows/<slug>/tasks.toml` store shape, `ref` derivation, the derived graph products, the 512-node cap, and the frozen contracts.
- [references/agents.md](references/agents.md) — the `agents` group: `agents record` (the CLI entry point for what the harness hooks write in-process) and `agents list`, the gitignored `.claude/flows/<slug>/agents.toml` store shape, the start / stop / idle events, flow selection, and the `record` output and its not-recorded reasons.
- [references/inputs.md](references/inputs.md) — the `inputs` group over the git-ignored user-input store `.claude/inputs.toml`: `list`, `add`, `ack`, `handle`, `withdraw`, `answer`, their flag tables and output shapes, and the store shape. What a record means, who may write it, and its trust boundary are the `flow-contract-user-inputs` skill's call, not this one's.

To find a section without reading a whole file:

```bash
grep -n '^##' claude/skills/tomlctl/references/<file>.md
```

## Output options

Global flags shape the output of every command, so no tomlctl output needs `python`, `node`, `jq`, `head`, `tr` or a shell loop — most of which a stock Windows install lacks. Each is accepted before or after the subcommand (`tomlctl --get id items get <f> R1` and `tomlctl items get <f> R1 --get id` are the same call).

| Flag | Value | Effect |
|---|---|---|
| `--select <P1,P2>` | comma-separated paths | Keep only these fields — of each row on a row report, of the object otherwise. A dotted path keys the result by the path string: `{"policy.checkpoints":"milestones"}`. |
| `--omit <P1,P2>` | comma-separated paths | Drop these paths from every row and the header, or from the single object — the inverse of `--select`, for shedding one bulky field such as `body`. |
| `--limit <N>` | integer | Keep at most N rows. Row reports only; on a single object it is a `kind=validation` error. |
| `--where*` | `KEY=VAL`, repeatable | Filter the rows: the `--where` family of [query.md](references/query.md#filters-all-repeatable-all-and-combined) (`--where`, `-not`, `-in`, `-has`, `-missing`, `-gt`, `-gte`, `-lt`, `-lte`, `-contains`, `-prefix`, `-suffix`, `-regex`), on any row report. |
| `--rows <PATH>` | one path | Treat the array at PATH of a single-object report as the row set; the rest of the object becomes its header, and every row option then applies per element. |
| `--header` | — | Shape a row report's header — the report minus its rows — as a single object. |
| `--max-chars <N>` | integer | Cut every string value longer than N characters to its first N, followed by `…(+K)` where K is the number cut. |
| `--lines` | — | Compact NDJSON: a header line, then one row per line; one compact line for a single object. |
| `--get <PATH>` | one path | Bare values: one line per row on a row report; on a single object, the value itself, an array spread one element per line. Strings print unquoted. |
| `--template <T>` | template | One text line per row, or one for a single object. `{path}` is a placeholder, `{path:N}` the same cut to N characters as `--max-chars`, `{{` / `}}` literal braces; strings print bare, numbers and bools as literals, arrays and objects as compact JSON, null or missing as empty. |
| `-q` / `--quiet` | — | Print nothing on success; the exit code carries the result. Errors still go to stderr. |

A path is dot-separated, with a numeric segment indexing an array (`policy.checkpoints`, `files.0`). A `*` segment matches every element of an array or every value of an object: `--select 'deps.*.ref'` keys an array of every match by the path string, `--get` prints each match on its own line, and a template placeholder renders them comma-joined. Branches that do not resolve drop out, and an empty match set is not an error. Quote a wildcard path, as you quote every template.

`--get` and `--template` print a string value verbatim, so a value containing newlines spans several lines and a row is no longer one line; use `--lines` or `--select` when line-safety matters, and `--max-chars` when only the start of a long field is wanted. A value cut by `--max-chars` says how much is missing; re-run without it to read the full text.

```bash
tomlctl items get ledger.toml R22 --get symbol                 # old::fn
tomlctl items list ledger.toml --where status=open --get id    # R1\nR3
tomlctl tasks show 3 --slug auth-overhaul --select ref,status --lines  # {"ref":"…","status":"done"}
tomlctl tasks show 3 --slug auth-overhaul --with deps --select 'deps.*.ref'  # {"deps.*.ref":["…"]}
tomlctl tasks show 3 --slug auth-overhaul --with deps --rows deps --where status=done --get id
tomlctl tasks show 3 --slug auth-overhaul --omit body --max-chars 80 --lines
tomlctl tasks list --slug auth-overhaul --where-contains files=output.rs --template '{id} {title:30}'
tomlctl backlog check --summary '<summary>' --kind bug --area <area> --header --get verdict  # novel
tomlctl items orphans ledger.toml --template '{id}: {class}'   # R7: symbol-missing
tomlctl set .claude/flows/auth-overhaul/context.toml status review -q  # nothing; exit 0
```

**Rows versus header.** A report that holds a collection is a row report: either a bare array, or an object whose collection field carries the rows and whose other fields form its header. `--select`, `--omit`, `--get`, `--template`, `--where*` and `--limit` act on the rows; `--omit` and `--max-chars` reach the header too. Read a header field — `backlog check`'s `verdict`, `tasks check`'s `ok` — with `--header`, which shapes the header as a single object: `--header --get verdict`. Every other report is a single object, and the flags act on the object itself; `--rows <PATH>` turns one of its arrays into a row set (`tasks show <id> --with deps --rows deps`). `--rows` on a row report is a `kind=validation` error naming that report's row field, and `--rows` excludes `--header`.

**Filters on any row report.** The `--where*` family is global. On `items list`, `backlog list` and `tasks list` the query engine applies it as before; every other row report, including one re-rooted by `--rows`, filters its rows before `--limit`. On a single object without `--rows` it is a `kind=validation` error that names `--rows`. A field holding an array matches element by element — `--where files=<path>` keeps a row whose `files` holds that path, `--where-not` keeps a row where no element equals it, and the comparison and string predicates match when any element does; `--where-has` / `--where-missing` still test for a non-empty value. A key absent from the row but containing `.` is read as a dotted path (`--where meta.owner=ann`). Give every `--where*` on one side of the subcommand: when values sit on both sides, clap keeps only those after it, so tomlctl refuses the split with `kind=validation` rather than filtering on half the predicates.

**Combinations.** `--get` excludes `--select`, `--omit` and `--template`; `--template` excludes `--select` and `--omit`; `--omit` excludes `--select`; `--rows` excludes `--header`; `-q` excludes every other output flag. Each refusal, and each path failure, is a `kind=validation` error under `--error-format json`. Options apply in this order: `-q` → `--rows` / `--header` → path validation → `--where*` → `--limit` → `--omit` / `--select` → `--max-chars` → `--get` / `--template` → `--lines` or the default style.

**Unknown paths.** When none of the `--select`, `--omit`, `--get` or template paths is present on any row of a non-empty row set, or on a single object, the call errors with the list of fields that do exist, plus `did you mean <path>?` for up to three matching paths one object level deeper — `backlog show <id> --get summary` suggests `item.summary`. A path no row carries passes beside one that matches and projects to absent (renders empty), so an optional field such as `promoted_to` can ride along in a `--select`; the cost is that a typo beside a valid path goes unflagged. A wildcard path is valid when the segments before its first `*` resolve on some row. Validation runs against the full row set before `--where*` and `--limit` cut it; a key missing from only some rows is omitted (or rendered empty) on those rows. An empty row set validates nothing and prints its empty result, so `tasks check --get class` on a clean store exits 0.

**Truncation.** When `--limit` actually removes rows, the report records `"limited": {"shown": n, "total": N}` — in the header of an object report; as a header line `{"limited":{…}}` before the rows of a bare-array report under `--lines`; and by wrapping a bare-array report as `{"rows":[…],"limited":{…}}` in pretty mode. Under `--get` / `--template`, which have no header, `tomlctl: showing n of N rows` goes to stderr instead (suppressed under `--error-format json`). `items list`, `backlog list` and `tasks list` report it the same way — pretty `{"rows":[…],"limited":{…}}`, a leading `{"limited":{…}}` line under `--lines` — except that their `--ndjson`, `--pluck` and `--raw` streams stay header-free and take the stderr line, so `--ndjson` output still feeds `items add-many` unchanged ([output shapes](references/query.md#output-shapes---raw----lines----ndjson)). No `limited` appears when nothing was cut. The list verbs refuse `--rows` and `--header`: their rows are the listed items.

**Write envelopes.** On a write's `{"ok":true,…}` envelope a shaping failure — an unknown path, `--limit` — prints the unshaped envelope, warns on stderr and exits 0: the write has landed, and an exit of 1 would invite a retry that applies it twice. `items add --id-prefix R … --get id` prints the minted id, and `flow record … --get id` the minted `E<n>`.

**Text output.** `get --raw`, `json get --raw`, `items next-id`, `tasks render --stdout`, `flow render-progress-log --stdout` and `tasks edges --dot` print text, not JSON: they honour `-q` and refuse every other output flag with one shared message (`` `--lines` does not apply to this command's text output ``). `items sweep --update` prints a write envelope, so `--lines` there gives one compact line.

**`-q` and payload verdicts.** Most commands signal failure through a non-zero exit, so `-q` loses nothing. These carry their verdict in the payload and exit 0 regardless, so under `-q` they report only that they ran — read the payload instead: `flow stale`, `flow doctor`, `backlog check`, `items find-duplicates`, and `backlog evidence audit` without `--strict`.

### Rows and header per command

Under `--lines`, a report wrapped in an object emits its other fields as one header line first, then one row per line; the header is left out when the object has no other field. A bare-array report emits one element per line, so an empty one prints nothing. The header comes first so that a truncated read still sees the totals and the hazards. The same split decides what `--header` returns and what the row options act on.

| Verb | One row per | Header fields |
|---|---|---|
| `items clusters` | cluster | `batches`, `dropped_deps` |
| `items orphans` | orphan record | — |
| `items find-duplicates` | duplicate group | — |
| `items sweep` (read-only) | swept item | `skipped_items`, `files_scanned`, `coverage_complete`, `truncated` |
| `sweep` | `file:line` hit | `files_scanned`, `skipped`, `truncated`, `coverage_complete` |
| `inputs list` | input record | `path`, `revision` |
| `tasks edges` | edge | — |
| `tasks batches` | Kahn layer | — |
| `tasks check` | finding | `ok` |
| `tasks snapshot` | task row | every other snapshot field |
| `backlog check` | candidate | `verdict`, `dedup_id`, `thresholds` |
| `backlog check --ndjson` | probe (`line`, `summary`, `verdict`, `dedup_id`, `candidates`) | `thresholds` |
| `tasks train` | commit group (`ids`, `refs`, `files`, `checkpoints`, `shared_with_pending`) | `granularity` |
| `tasks update <id>,<id>` | updated row (`id`, `ref`, `changed`) | `ok` |
| `flow record --ndjson` | recorded entry | `ok`, `ids`, `path` |
| `backlog add-many` | input line's outcome (`line`, `action`, `id`) | `ok`, `path`, `created`, `added`, `bumped`, `skipped`, `advisories` |
| `backlog evidence audit` | finding | `root`, `counts` |
| `flow list` | flow | `ok`, `skipped` |
| `flow find-plans` | plan | — |
| `agents list` | agent record | — |
| `blocks verify` | block | `ok` |

`items list`, `backlog list` and `tasks list` run on the query engine, whose `--lines` / `--ndjson` is documented under [output shapes](references/query.md#output-shapes---raw----lines----ndjson). `tomlctl capabilities` lists every output flag under its root `global_flags` key, and advertises them as features: `output_options` for the original six, then `path_wildcard`, `rows_header`, `global_where`, `array_predicates`, `max_chars`, `omit`, `template_width` and `list_limited`.

## Guidance canon

One rule set for every skill, command and agent that drives tomlctl. Other documents cite this section rather than restating it; [write.md](references/write.md) carries the write-side detail.

- **Shaping output**: the [output options](#output-options) only. Never pipe tomlctl's stdout into `python`, `node`, `jq`, `head`, `tail`, `tr`, `cut`, `sed`, `awk` or `grep`.
- **One entry**: field flags — `--set K=V` (a string; a date key still lands as a TOML date), `--set-json K=JSON`, `--set-file K=PATH` — on `items add` / `update`, `array-append`, `set` and `flow record`. Write prose to a file with the Write tool and pass it with `--set-file`. Git Bash rewrites a `--set` value that starts with `/` into a Windows path, so pass such a value through `--set-file`, or prefix the call with `MSYS_NO_PATHCONV=1`.
- **Verbs without field flags** (`backlog add` / `check`, `json set`, `inputs`, `tasks add-many`): inline single-line JSON for a short machine-built value; otherwise stage the payload with the Write tool and pass `--json '@<path>'` or `--ndjson <path>`. Several backlog probes go through one `backlog check --ndjson <path>`.
- **Many entries or whole payloads**: stage them with the Write tool and pass `--ndjson <path>`, or `--json '@<path>'` — quoted, because PowerShell drops an unquoted `@path`. The single-line `printf '%s\n' '<row>' … | tomlctl … -` pipe stays legal for a few short machine-built rows.
- **No multi-line heredocs** into tomlctl.
- **Templates and wildcard paths**: quote them (`'{id}: {summary:60}'`, `'deps.*.ref'`).
- **Execution records**: written only through `tomlctl flow record`, which mints the id, defaults the date, derives `task_ref` from `--task`, validates and caps.
- **Ids**: never numbered by hand. `--id-prefix` on `items add`, `items add-many` and `items apply` mints them inside the write lock, and a removed top id is never minted again.
- **Context**: updated in one `tomlctl set <context> --set <k>=<v> --set <k>=<v>`; the write refreshes the root `updated` itself.
- **Commits**: grouped by `tomlctl tasks train`.
- **Errors**: read them through `--error-format json`, which puts a `{"error":{"kind",…}}` envelope on stderr. Never `2>&1` tomlctl into anything and never `2>/dev/null` it — its stderr is the diagnosis. A best-effort call that may legitimately fail runs plainly, and a non-zero exit reads as "absent".

## Install

One-time, per machine:

```bash
# from the dev-tools repo root
cargo install --path tomlctl
```

That drops `tomlctl` into `~/.cargo/bin/` (already on PATH if Rust is installed). Verify:

```bash
tomlctl --version
```

## Feature-gate with `tomlctl capabilities`

`tomlctl capabilities` emits a stable JSON document with `version`, `features`, `subcommands`, and `commands` keys so downstream templates can feature-gate at boot without parsing `--help` prose. Features are stable within a minor release; new flags add new feature entries rather than being version-qualified. Example invocation (truncated):

```bash
tomlctl capabilities
# {"version":"0.15.0","features":["raw","lines","dedupe_by","dry_run","agent_context","output_options",...],"global_flags":{...},"commands":{...}}
```

Representative entries:

| Feature | What it enables |
|---|---|
| `count_distinct` | `--count-distinct <FIELD>` on `items list` |
| `raw` / `lines` | `--raw` / `--lines` output shapes |
| `dedupe_by` / `dedup_id_auto` | `--dedupe-by <FIELDS>` + auto-populate on every write |
| `find_duplicates_across` | `items find-duplicates --across <other>` (tier A/B) |
| `error_format_json` | `--error-format json` + `ErrorKind` taxonomy |
| `strict_read` / `dry_run` | `--strict-read` on reads / `--dry-run` on every write subcommand (`set`, `set-json`, `array-append`, `items add`, `items add-many`, `items update`, `items remove`, `items apply`, `items backfill-dedup-id`, `items sweep --update`) |
| `backfill_dedup_id` / `integrity_refresh` | legacy upgrade + sidecar regen |
| `sweep` | `tomlctl sweep -e <regex>` — sorted `file:line` hits over `git ls-files` with `skipped` counts and `coverage_complete` |
| `items_sweep` | `items sweep` — per-item `new` / `gone` / `kept` / `unverified` diff of stored `sweep` strings against `instances`, plus `--update` |
| `items_clusters` | `items clusters` — file-disjoint clusters, `depends_on` batches, per-cluster `lite_file_scope` |
| `output_options` | the global `--select` / `--limit` / `--lines` / `--get` / `--template` / `-q` on every command ([Output options](#output-options)) |
| `orphans_instances` | `items orphans` reports the `instance-missing` class (one row per unresolvable `instances` anchor, with `reason`) |
| `next_id_bare` | `items next-id` prints the bare id |
| `id_prefix` | `items add` / `items add-many --id-prefix <P>` mint the id inside the write lock |
| `auto_last_updated` | CLI writes stamp an existing `last_updated` field |
| `multi_id` | `tasks update`, `tasks show` and `backlog show` take lists of ids |
| `path_wildcard` / `rows_header` / `global_where` / `array_predicates` | `*` path segments; `--rows` / `--header`; `--where*` on every row report; any-element matching on array fields |
| `max_chars` / `omit` / `template_width` / `list_limited` | `--max-chars`; `--omit`; `{path:N}` placeholders; the `limited` header on the list verbs |
| `field_flags` / `multi_set` / `context_updated_stamp` | `--set` / `--set-json` / `--set-file`; several pairs in one `set`; `updated` refreshed on `context.toml` writes |
| `flow_record` / `tasks_train` | `flow record` execution-record writes; `tasks train` commit groups |
| `apply_id_prefix` / `id_high_water` | `items apply --id-prefix <P>`; a removed top id is never minted again |
| `show_absent` / `update_ref` / `backlog_check_batch` | `tasks show --with absent`; `ref` in `tasks update` envelopes; `backlog check --ndjson` |
| `suggestions` | `did you mean` hints for a mistyped subcommand, flag or path |
| `agent_context` | `tomlctl capabilities` (the `.commands` field of the JSON output) emits a per-subcommand flag schema (type/required/default/values/repeatable + mutex_groups) for runtime introspection without parsing --help prose. |

### Agent-context schema (`tomlctl capabilities` — `.commands` field)

When `features` includes `agent_context`, the capabilities document also carries a `commands` key — a per-subcommand JSON tree that lets agents drive flag assembly programmatically instead of regex-matching `--help` text.

Shape: `commands.<subcommand>` (recursively for nested subcommands like `items.subcommands.list`) carries:

- `flags` — map of flag-name → entry. Each entry has `type` (`string` / `bool` / `enum`), `required` (bool), `repeatable` (bool), and optional `default` and `values` (allowed enum variants). Positional arguments appear with their angle-bracketed display name (e.g. `<file>`).
- `mutex_groups` — list of clap `ArgGroup` mutex sets; an agent can refuse a combination locally without round-tripping through the binary. Note: mutex groups are assembled from two sources — clap's generated `ArgGroup` declarations AND a supplementary `MUTEX_GROUPS` const in `capabilities.rs`. If you extend the CLI, update both; omitting the const supplement will produce incomplete mutex data in the capabilities output.
- `subcommands` — present only when the command has nested subcommands (e.g. `items`, `blocks`, `integrity`).

The global flags — `--error-format` and the [output options](#output-options) — appear once, under the document's root `global_flags` key, in the same entry shape, and in no command's `flags`. A command accepts its `.commands` flags plus every `global_flags` entry.

Feature-gate on its presence before driving flags from the schema:

```
# pseudocode
caps = tomlctl_capabilities()
if "agent_context" in caps.features:
    schema = caps.commands["items"]["subcommands"]["update"]["flags"]
    # build the invocation from schema
else:
    # fallback: parse `tomlctl items update --help`
```

## Constraints and gotchas

- **No comment preservation.** The schemas forbid inline comments, so this is fine for flow/ledger files. Do not point `tomlctl` at TOML files where comments matter.
- **Whole-file rewrite.** Any write operation reparses, mutates, and re-serialises the whole document. Never runs a line-level Edit.
- **Whitespace may change.** Long inline arrays may be reflowed to multi-line by the serializer. Semantically identical.
- **`created` is preserved verbatim.** The tool never touches it unless you explicitly `set created <date>` (don't).
- **`dedup_id` auto-populates on every write** unless `TOMLCTL_NO_DEDUP_ID=1`. First-time upgrade of a legacy item (add/add-many path) populates without marking it as a user-intended change — the sidecar refresh is an implicit one-time event.
- **Unknown-value rules stay with the caller.** `tomlctl` returns raw values; the command's "unknown status → treat as in-progress" / "unknown category → fail-soft" rules apply in the calling command's logic, not in the tool.
- **Errors exit non-zero and print to stderr.** Success paths emit either JSON data (or `--raw` / `--get` / `--template` bare text, or `--lines` NDJSON) or `{"ok":true,…}` to stdout, and nothing under `-q`. Always check exit code in scripted flows. For machine-readable error class, use `--error-format json`.
- **Lock timeout: 30 seconds.** Writes acquire an exclusive OS-level lock on a hashed lock file under `<repo-top-level>/.claude/.locks/<sha256-of-canonical-target-path>.lock`, rather than a sidecar `<file>.toml.lock` that could collide with a real file of that name. `tomlctl` polls `try_lock_exclusive` on this file and bails after 30 s total with an error naming the lock path. On Windows this is a mandatory lock — a crashed or stuck `tomlctl` leaves the `.lock` file present and the OS keeps the lock until the offending process dies. **Recovery when a lock is stranded:** confirm no live `tomlctl` process holds it (Task Manager / `Get-Process tomlctl` / `ps aux | grep tomlctl`), then delete the specific `.claude/.locks/<hash>.lock` file from the error message. The next invocation will recreate and re-acquire it cleanly.
- **Write-path safety (best-effort containment guard, not a sandbox).** Write operations (`set`, `set-json`, `items add|update|remove|apply|add-many|backfill-dedup-id`, `items sweep --update`, `array-append`) reject targets that canonicalise outside the current repo's `.claude/` directory. The guard resolves symlinks and `..` at canonicalisation time and rejects paths not under `<git-top-level>/.claude/`. Read operations are not guarded. Pass `--allow-outside` (a per-subcommand flag) to override when you genuinely need to edit a flow TOML elsewhere — e.g. `tomlctl set /tmp/scratch.toml status draft --allow-outside`. `--allow-outside` is pinned behind an interactive permission prompt at the project settings level — it should never appear in unattended automation. Treat this as a best-effort guard against agent/user typos that would otherwise land writes in unintended locations; it is not a security sandbox and a TOCTOU-race or symlink swap between canonicalisation and open can in principle escape it. Pair `--allow-outside` with `--no-create` whenever the target should already exist — see [auto-create on first write](references/write.md#auto-create-on-first-write).

## Permissions

`Bash(tomlctl *)` is pre-approved in the project's `.claude/settings.json`. Any invocation passing `--allow-outside` is explicitly denied by three deny rules in that same file and falls through to an interactive permission prompt:

```json
"deny": [
  "Bash(tomlctl --allow-outside *)",
  "Bash(tomlctl * --allow-outside)",
  "Bash(tomlctl * --allow-outside *)"
]
```

Agents should never emit `--allow-outside` unattended — the write-path containment guard is default-on for a reason.
