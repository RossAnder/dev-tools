# tomlctl — tasks reference

The flag surface of the `tomlctl tasks` group, which reads and writes
`.claude/flows/<slug>/tasks.toml`: a per-flow task DAG with the plan's execution policy and
checkpoint groups beside it. Every verb resolves its store through one shared target group
(`--slug` or `--file`) and emits JSON on stdout. What the fields *mean*, which verb a carrier
reaches for, and the rules binding `/plan-new`, `/implement`, `/review-plan` and
`/plan-update` are the `flow-contract-task-store` skill's job
(`claude/skills/flow-contract-task-store/SKILL.md`); this file is the flag and output surface
of the read verbs, and of what every verb shares.

## Contents

- [`tasks show`](#tasks-show)
- [`tasks list`](#tasks-list)
- [`tasks edges`](#tasks-edges)
- [`tasks ready`](#tasks-ready)
- [`tasks batches`](#tasks-batches)
- [`tasks closure`](#tasks-closure)
- [`tasks check`](#tasks-check)
- [`tasks render`](#tasks-render)
- [`tasks snapshot`](#tasks-snapshot)
- [Store target](#store-target)
- [The `check` finding classes](#the-check-finding-classes)

The five mutating verbs — `import-plan`, `add`, `add-many`, `update`, `remove` — are
[tasks-write.md](tasks-write.md). The store's own half — its keys and `[[items]]` fields, `ref`
derivation, the graph products recomputed on every read, the 512-node cap, the snapshot
envelope and the frozen contracts — is [tasks-store.md](tasks-store.md).

Every mutating verb carries the shared write bundle — `--allow-outside`, `--no-create`,
`--no-write-integrity`, `--strict-integrity`, `--verify-integrity` — and every read verb
(`show`, `list`, `edges`, `ready`, `batches`, `closure`, `check`, `render`, `snapshot`) the read
bundle, `--verify-integrity` and `--strict-read`. A missing store is `kind=not_found` either
way; `--strict-read` makes that hold under `--verify-integrity` too, by checking before the
sidecar does.
A `--slug` target lands inside the `.claude/` containment guard, so none of these needs
`--allow-outside`; a `--file` target outside `.claude/` does, on the write verbs.
`render` is the exception in the other direction: it is a derived write to the *plan*, resolved
from the flow context or the store's recorded `plan_path` rather than from an argument, so it
carries the read bundle and no write flag has a hook on it. Every verb also takes the global
`--error-format text|json`; see [flow.md](flow.md#error-format---error-format-json) for the
JSON envelope and its `kind` taxonomy. The global output options — `--select`, `--limit`,
`--lines`, `--get`, `--template`, `-q` — shape any verb's stdout; see
[Output options](../SKILL.md#output-options).

## Output fields

Each verb's `| Key |` table names the keys of the JSON it prints on stdout: `a[].b` is key `b`
of each element of array `a`, and `[].b` the same for an array printed bare. A key listed with
no condition is always present, an empty list as `[]`. The tables are not closed schemas — keys
may be added without changing existing ones — so select the keys you need and branch on `ok`
or a documented count. `import-plan`, `check` and `render --check` report findings in one
[shape](#finding-shape).

## `tasks show`

```bash
tomlctl tasks show <id> --slug <slug> --with body,files,deps
tomlctl tasks show 3,7,12 --slug <slug> --get status
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids, comma- or space-separated | Tasks to print. At least one. | — |
| `--with` | comma-separated, repeatable | `summary`, `body`, `files`, `deps`, `dependents`. A clap `value_enum`: an unknown part exits `2` with usage prose naming it and listing the valid set, **outside** the `--error-format json` envelope — not the exit-`1` `kind=validation` error `--effort` and `--status` raise. | `summary` |

One id prints that row's object. Several print an array of those objects in the order given,
with `--with` applied to each, so the global `--get`, `--select` and `--lines` work per row.
An unknown id errors with `kind=not_found` and fails the whole call, whichever position it
holds. Only `dependents` builds the graph, so a store with a cycle or a dangling edge still
shows its rows under every other part.

| Key | Value | Meaning |
|---|---|---|
| `id` | id | Always emitted, whatever `--with` selects, so a fetched row can be matched back to the id that was asked for. |
| `ref`, `title`, `effort`, `status`, `checkpoint`, `needs`, `coupling` | row fields | The `summary` part; field meanings are the [store's](tasks-store.md#store-shape). |
| `files` | paths | Under `summary` or `files`. |
| `file_notes` | object | The `files` part: each claimed path carrying a `[[file_notes]]` entry, mapped to its note; `{}` when none does. |
| `new_files`, `deleted_files` | paths | The `files` part: the paths whose note marks a created or a removed file, in `files` order; always present, possibly empty. |
| `action`, `detail`, `acceptance` | text | The `body` part. |
| `deps[]` | summaries | The `deps` part: the `summary` keys of each direct `needs` ∪ `coupling` target, ascending by id. |
| `dependents[]` | summaries | The `dependents` part: the `summary` keys of each transitive successor, excluding the row itself. |
| `import_override.files`, `import_override.needs` | paths, ids | The plan **base** each stamped field replaced — not the patched `files` / `needs` above it. Ungated by `--with`; an unstamped field is omitted and the whole key is absent from an unstamped row. |
| `backlog.closes`, `backlog.refs` | backlog ids | The row's `[[backlog_links]]` entry. Ungated by `--with`; absent from an unlinked row. |

The change kind is derived from the note on read, never stored; the rule is the
`flow-contract-task-store` skill's (`tasks show` in its verb semantics).

`import_override` and `backlog` follow the selected parts, and never appear on a summary nested
under `deps` / `dependents`: they belong to the row that was asked for.

## `tasks list`

Task rows through the generic query engine — the whole `items list` surface applies, because
the store's array *is* `items`. See [query.md](query.md) for that half: `--where-*` predicates,
`--select` / `--exclude` / `--pluck`, `--sort-by` / `--limit` / `--offset`, `--count-by` /
`--group-by`, `--raw` / `--ndjson`, plus the global `--lines`.

```bash
tomlctl tasks list --slug <slug> --where status=pending --select id,ref,files --ndjson
tomlctl tasks list --slug <slug> --count
tomlctl tasks list --slug <slug> --count-by status
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--count` | — | Emit `{"count":N}` instead of the rows. | off |

`--count` joins `--count-by`, `--group-by`, `--pluck` and `--count-distinct` in a
mutually-exclusive group, so a mismatched pair is a parse-time error rather than a silent
collapse to one shape. `[tasks].total` in a flow's `context.toml` is this verb under `--count`.

The store carries none of the legacy shortcut flags `items list` accepts. In particular
**`--file` here is the store target, not `--where file=…`** — the one flag whose meaning
differs between the two groups.

| Key | Value | Meaning |
|---|---|---|
| `[]` | rows | The default shape: the matching `[[items]]` rows, keyed as the [store shape](tasks-store.md#store-shape) lists and narrowed by `--select` / `--exclude`. Every other shape is [query.md](query.md)'s. |
| `count` | integer | Under `--count`, the whole output: the number of matching rows. |

## `tasks edges`

```bash
tomlctl tasks edges --slug <slug> --kind overlap
tomlctl tasks edges --slug <slug> --dot
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--kind` | `needs` \| `coupling` \| `overlap` | Restrict to one kind. Omit for all three. A clap `value_enum`, failing exactly as `tasks show --with` does: an unknown kind exits `2` with usage prose, **outside** the `--error-format json` envelope. | all |
| `--dot` | — | Emit Graphviz DOT source on stdout instead of the JSON edge list. Text output, so the global output options other than `-q` are refused with it. | off |

Under `--dot` every task is a node whatever `--kind` selects, so a filtered graph still renders
the whole plan; `coupling` arrows are dashed and `overlap` dotted and undirected.

| Key | Value | Meaning |
|---|---|---|
| `[]` | edges | Grouped by kind in the order below and ascending within each. Replaced by DOT source under `--dot`. |
| `[].kind` | `needs` \| `coupling` \| `overlap` | `needs` and `coupling` come straight off the rows; `overlap` — two rows sharing a file with no directed path either way — is computed, and is the one kind that needs a graph, so it is also the one kind a malformed store cannot answer. |
| `[].from` | id | The prerequisite, so a JSON edge and its DOT arrow read the same way round. For `overlap`, the pair's earlier row in store order. |
| `[].to` | id | The row that waits on `from`; for `overlap`, the pair's later row. |

## `tasks ready`

```bash
tomlctl tasks ready --slug <slug> --in-flight 3,4
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--in-flight` | comma-separated ids | Tasks currently dispatched. A ready row sharing a file with one of them is reported as held, and a row waiting on one stays in `next` rather than `blocked`. | empty |

An `--in-flight` id no row carries is refused, as is a cyclic store: Kahn strands a cycle's
members and a stranded task reads in a frontier exactly like one that is merely waiting.

| Key | Value | Meaning |
|---|---|---|
| `ready` | ids, ascending | `pending` rows whose every dependency is `done`, less what `held` names — directly dispatchable. |
| `held[]` | objects, ascending by `id` | Rows otherwise ready that share a file with an in-flight row. |
| `held[].id` | id | The held row. |
| `held[].blocked_on_file` | path | The file it shares. |
| `held[].holder` | id | The in-flight row claiming that file. |
| `next` | ids, ascending | `pending` rows that become ready once the current round — `ready`, `held` and `--in-flight` — lands. |
| `blocked[]` | objects, ascending by `id` | `pending` rows no later wave can reach. |
| `blocked[].id` | id | The stranded row. |
| `blocked[].blocker` | id | The nearest **ancestor** stranding it: one that is neither `done`, nor `pending`, nor named in `--in-flight`. The walk climbs through intervening `pending` rows, so this is the stall itself rather than a pending row between it and the blocked row. |
| `blocked[].blocker_status` | status | The blocker's `status` — `in-progress`, `failed` or `deferred`. |

## `tasks batches`

```bash
tomlctl tasks batches --slug <slug>
```

No flags beyond the read bundle and the target. In-degree is `needs` ∪ `coupling`, so a
coupling edge pushes its dependent a layer back. A cycle is refused rather than layered — the
layering drops what a cycle strands, so the answer would omit tasks instead of naming them.

| Key | Value | Meaning |
|---|---|---|
| `batches` | arrays of ids | Kahn layers in dependency order, each ascending — `[[1,4],[2],[3]]`. Every row lands in exactly one. |

## `tasks closure`

```bash
tomlctl tasks closure --slug <slug> --checkpoint A
tomlctl tasks closure --slug <slug> --task <id> --up
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--checkpoint` | group id | Print a checkpoint group. Conflicts with `--task`. | — |
| `--task` | id | Print one task's walk. Conflicts with `--checkpoint`. | — |
| `--up` | — | Transitive dependencies (ancestors), inclusive. Conflicts with `--down`. | — |
| `--down` | — | Transitive dependents (successors), inclusive. Conflicts with `--up`. | — |

Exactly one mode is required, and `--task` requires a direction. Three shapes clap's pairwise
conflicts cannot catch are refused with `kind=validation`: no mode at all, `--task` with no
direction, and a direction alongside `--checkpoint` — the last is refused rather than ignored.
An unknown checkpoint id errors and names the ids `[[checkpoints]]` does hold.

The two modes print disjoint key sets:

| Key | Value | Meaning |
|---|---|---|
| `checkpoint` | group id | `--checkpoint` mode: the group asked for. |
| `members` | ids, ascending | The rows whose `checkpoint` is the group. |
| `maximal` | ids, ascending | The group's maximal elements — the antichain its `Checkpoint after` bullet names. |
| `dependency_closure` | ids, ascending | Everything the group reaches upward — the set the rendered `CHECKPOINT` marker prints under that same name. It coincides with `members` exactly when every dependency already sits in an earlier group, which is the case a reader cannot use to tell them apart — so both are reported. |
| `valid_cut` | bool | Whether the union of the group with every earlier one is downward-closed: "committable here", not "self-contained". |
| `task` | id | `--task` mode: the row walked from. |
| `direction` | `up` \| `down` | The direction flag given. |
| `ids` | ids, ascending | The walk, `task` included. |

## `tasks check`

```bash
tomlctl tasks check --slug <slug>
tomlctl tasks check --slug <slug> --plan
tomlctl tasks check --slug <slug> --in-flight 3,4
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--plan` | — | Also compare the plan markdown against the render output and report `render/drift`. | off |
| `--in-flight` | comma-separated ids | Tasks currently dispatched. A row waiting on one of them is not reported as `dag/stalled-dependency`. Unlike [`ready`](#tasks-ready)'s, an id no row carries is dropped rather than refused — a typo must not empty a class. | empty |

**Exit `1` when any finding is error-class, `0` otherwise** — warnings alone exit `0`. See
[the check finding classes](#the-check-finding-classes) for the full list. Under `--plan` the plan is resolved exactly as `render` resolves it, and the same
containment refusal applies. `render/drift` is a warning, so it is reported without moving the
exit code: `check --plan` still exits `0` on a drifted plan, and
[`tasks render --check`](#tasks-render) is the mode to gate on instead.

A store holding any `[[backlog_links]]` entry is also joined against `.claude/backlog.toml` for
the `backlog/*` classes; a link-free store never reads it. Under `--file` there is no slug, so
`backlog/claimed-elsewhere` is not raised.

| Key | Value | Meaning |
|---|---|---|
| `ok` | bool | `false` exactly when a finding is error-class — the exit-`1` case. |
| `findings[]` | [findings](#finding-shape) | The store's classes, then the `backlog/*` join, then `render/drift` under `--plan`; each of the first two sorted by `(class, ids)`. |

## `tasks render`

Rewrites the plan's `## Execution Policy`, `## Tasks` and `## Dependency Graph` sections from
the store. Every other byte of the document is preserved.

```bash
tomlctl tasks render --slug <slug>
tomlctl tasks render --slug <slug> --check
tomlctl tasks render --slug <slug> --stdout
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--stdout` | — | Print the rendered plan instead of writing it. | off |
| `--check` | — | Report drift and exit `1` without writing. | off |

Both non-writing modes branch before the render lands anywhere, so neither leaves a byte behind
on a plan it disagrees with. `--check` exits `1` on any drift even though `render/drift` is a
warning class: the mode is a gate.

Only what the store holds comes back: text under a task that no field stores is dropped, which
`import-plan` reports as `plan/text-unstored`. A `Files` note may span lines — the lines nested
under a bulleted path — and renders nested under that path's bullet.

A missing section is inserted in canonical order (Execution Policy before Tasks, Dependency
Graph after Tasks). The source document's dominant line ending is detected and re-applied, so a
CRLF plan is not rewritten to mixed endings and `--check` does not report permanent drift.
Output is a derived write like `PROGRESS-LOG.md` — **no `.sha256` sidecar**.

`render` errors rather than writing on a cycle, a dangling edge, or a graph past the node cap.
The target path comes from the flow context under `--slug` and from the store's `plan_path`
under `--file`, never from an argument, and is refused if absolute, `..`-bearing, outside the
repo root, or not a `.md` document. Under `--slug` a store whose own `plan_path` resolves to a
different document than the context's is refused too, until the plan is re-imported.

`--stdout` prints the plan itself; the other two modes print:

| Key | Value | Meaning |
|---|---|---|
| `ok` | bool | `true` on a write. Under `--check`, `false` exactly when the plan drifts — the exit-`1` case. |
| `path` | repo-relative path | The plan written or compared. |
| `sections` | section titles | Write mode only: always `Execution Policy`, `Tasks`, `Dependency Graph` — the sections the write owns, not the ones that drifted. |
| `findings[]` | [findings](#finding-shape) | `--check` only: empty, or one `render/drift` whose `detail` names the drifted sections. |

## `tasks snapshot`

One read of everything a flow viewer renders: the rows, their graph products, the execution
record joined to the rows, and the hook-written agent records.

```bash
tomlctl tasks snapshot --slug <slug>
tomlctl tasks snapshot --file <path> --verify-integrity
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--file` | see [Store target](#store-target) | The store to read. Its companions `execution-record.toml`, `agents.toml` and `context.toml` are read from the same directory under either flag, and each reads as empty when absent. | — |
| `--verify-integrity` | — | Check `tasks.toml`, and each companion that exists, against its own sidecar before reading. | off |
| `--strict-read` | — | A missing store is `kind=not_found` even under `--verify-integrity`. Companions stay optional. | off |

`tasks` is the row set the global output options act on: under `--lines`, a header line of every
other key, then one task row per line. The verb writes nothing and seeds no absent companion. A cycle, a dangling edge or a store past
the node cap refuses the whole snapshot, as it does `ready` and `batches`.

| Key | Value | Meaning |
|---|---|---|
| `schema`, `revision`, `slug`, `plan_path`, `flow_status`, `policy`, `tasks`, `layers`, `frontier`, `edges`, `checkpoints`, `record`, `agents` | envelope | Emitted in this order. Each key's contents, and the `revision` fingerprint a poller compares, are [tasks-store.md](tasks-store.md#the-snapshot-read)'s. |

## Store target

Every verb takes a mutually-exclusive, **deliberately non-required** target group:

| Flag | Value | Meaning |
|---|---|---|
| `--slug` | flow slug | Resolves `<root>/.claude/flows/<slug>/tasks.toml`. Must match `^[a-z0-9][a-z0-9-]{0,63}$` — the same rule `flow init` applies, and the only thing keeping the join inside `.claude/flows/`. |
| `--file` | path | Explicit store path, bypassing slug resolution. Still under the write guard on the write verbs unless `--allow-outside`. |

`import-plan --plan <path> --dry-run` runs with **neither** — plan mode, validating a plan
before a flow exists. Every other verb refuses an empty target in post-parse validation, so the
refusal arrives as a tagged `kind=validation` error — machine-readable under
`--error-format json` — rather than clap usage prose on stderr.
`--reconcile-record` and `--slug`-resolved plan paths both need `--slug`; under `--file` the
store's own `plan_path` is the only plan the tool will find.

The root is `TOMLCTL_ROOT` or the git top level, exactly as `flow render-progress-log` resolves
it.

## The `check` finding classes

| Class | Severity | Raised by |
|---|---|---|
| `dag/cycle` | error | `check` — suppresses every class needing reachability |
| `dag/dangling-ref` | error | `check` |
| `dag/duplicate-number` | error | `check` — `ids` carries the number alone, so the `detail` names the `ref` of every row on it, and rows sharing the `ref` too by 0-based `[[items]]` position and title (hand-edit one row's `id`); `import-plan` splits those refs into the ones the plan produced and the ones the store still holds |
| `dag/unbuildable` | error | `check` — the graph engine refuses the store for a reason no row scan named; past the [node cap](tasks-store.md#the-512-node-cap) is the live case |
| `dag/unreachable-claim` | warning | `check` — shared file, no directed path either way |
| `dag/stalled-dependency` | warning | `check` — an `in-progress`, `failed` or `deferred` row with pending rows behind it; one finding per blocker, `ids` naming the blocker and `detail` its dependents. A blocker named in `--in-flight` raises nothing; never changes the exit code |
| `dag/symbol-without-edge` | warning | `check` — a symbol one row's `Action` introduces (`Add`/`Create`/`export` within three words of a backticked identifier) that another row's body names, with no directed path either way; one finding per pair, `ids` naming both. The edge `coupling` exists for and nothing populates. A markdown-only row introduces nothing, and a name two rows introduce raises nothing against a row already reaching one of them |
| `checkpoint/orphan-task` | warning | `check` — `checkpoint = ""`, or a group id no `[[checkpoints]]` entry declares; one finding per case. The import no longer produces the second case: a retained row is blanked when the plan stops declaring its group |
| `checkpoint/invalid-cut` | error | `check` — prefix union not downward-closed; `ids` are the dependencies that must move earlier |
| `checkpoint/marker-mismatch` | warning | `import-plan` only — the authored `Checkpoint after` bullet is never stored |
| `files/closure` | info | `check` — backticked paths under `packages/`, `apps/`, `docs/` or `scripts/` that a row's `Action` or `Detail` names and its `files` does not claim; one finding per row. A line carrying `read-only`, `model of`, `pattern` or `cite` is skipped. **Never moves the exit code** |
| `policy/max-parallel-range` | error | `check` |
| `policy/checkpoints-value` | error | `check` — `policy.checkpoints` outside its vocabulary |
| `policy/commit-granularity-value` | error | `check` — `policy.commit_granularity` outside its vocabulary |
| `policy/origin-value` | error | `check` — `policy.origin` neither `plan` nor `default`; only a hand-edited store reaches it |
| `plan/effort-untagged` | warning | `import-plan` only — **one** finding whose `ids` name every heading authoring no effort; the `M` default lands only where neither the heading nor an existing row supplies one, so a re-import keeps the row's effort and still warns |
| `plan/policy-absent` | warning | `import-plan` only — no `## Execution Policy` section, so `origin` is `default` and every policy field is a house default |
| `plan/no-tasks` | error | `import-plan` only — the `## Tasks` section parses to no task, so the import would replace the checkpoint table and policy with the empty and default values such a plan yields. A section holding only links to sibling documents that do carry task headings names that shape instead, the same diagnostic a plan with no section at all raises |
| `plan/heading-too-deep` | warning | `import-plan` only — a numbered task heading carrying seven or more hashes. The grammar reads three through six, so a deeper one stands as a phase label and its task is dropped with no other trace; **one** finding per heading, `ids` naming the number the author wrote and `detail` the plan line. Warning rather than error because refusing the import over one hash too many would block every other task in the plan |
| `plan/heading-anchor` | warning | `import-plan` only — a numbered task heading containing a `{#…}` id anchor. A ref derives from the title, so an anchor after the effort tag is dropped with it and one before the tag becomes part of the ref; **one** finding per heading, `ids` naming the task and `detail` the plan line. Remove the anchor |
| `plan/files-span-unclaimed` | warning | `import-plan` only — a comma-list `Files` line whose ` — ` note has a comma followed by a backticked span holding no `/` or `.`. The parse reads the span as the note's prose, so an extensionless path such as `Makefile` claims no file; **one** finding per `Files` line. A bulleted `Files` list keeps it a claim |
| `plan/files-malformed` | error | `import-plan` only — a `Files` entry that is neither a bare token nor one backticked span whole: a label ahead of the span, two spans on one sub-bullet, or prose a comma split off. The real paths beside it go unclaimed; **one** finding per `Files` field, `detail` naming each entry. Write one path per entry, any label or prose in a ` — ` note after it |
| `plan/text-unstored` | warning | `import-plan` only — non-blank text the store has no field for: an unknown field label, a line under a task outside any field, a nested `Files` line with no path above it, or prose under a phase heading or ahead of the first heading. `tasks render` removes it from the plan; not raised alongside `plan/no-tasks`; **one** finding per task or phase, `ids` naming the task (empty for a phase) and `detail` the first line dropped. Move it under `- **Detail**:` or out of the task |
| `plan/orphan-row` | warning, or **error** when the row's status is not `pending` | `check` — the `detail` names every orphaned `ref`, which `ids` cannot: two orphaned rows sharing a task number collapse to one id |
| `plan/override-held` | warning | `import-plan` only — a hand-patched `files` or `needs` the plan does not state; run `tasks render` to publish it |
| `plan/override-released` | warning | `import-plan` only — the plan restated the line, so its value replaced the hand-patched one and the stamp is gone |
| `render/drift` | warning | `check --plan` and `render --check` |
| `backlog/unknown-id` | error | `check` and `import-plan` — a linked id in neither the `backlog` nor the `compacted` array of `.claude/backlog.toml`, through a `closes` or a `refs` link; `ids` names every linking task. A missing backlog reads as empty |
| `backlog/unpromoted` | warning | `check` and `import-plan` — a `closes` id whose item is still `open`; `ids` names the closing tasks, and a `refs` link raises nothing |
| `backlog/claimed-elsewhere` | warning | `check --slug` and `import-plan --slug` — a `closes` id whose item is `promoted` to neither the slug nor the store's `plan_path`, `\` and `/` compared alike; `ids` names the closing tasks. Never raised under `--file` or in plan mode |
| `backlog/closed` | info | `check` and `import-plan` — a linked id whose item is `resolved`, `dismissed` or compacted; `ids` names every linking task |

The four `backlog/*` classes are joined from the store's `[[backlog_links]]` and
`.claude/backlog.toml`, read only when the store holds a link — a backlog that does not parse
then fails the verb. Each linked id raises at most one of them. A link keyed on a `ref` no row
carries — `tasks remove` leaves links in place until the next import, while `tasks update --ref`
re-keys them to the new `ref` —
contributes no id to `ids`; its `detail` names the `ref`.

A duplicate id and an edge to an absent task are scanned for *before* any graph is built, which
is what lets one run report every defect instead of dying on the first, and is why either of
them suppresses `dag/unbuildable` rather than doubling it.

`plan/orphan-row` fires only when `last_import_refs` is non-empty — an empty ref set means the
store has never been imported, not that every row is orphaned.

`files/closure` and `dag/symbol-without-edge` are heuristics over a row's prose, and both are
scoped by what an earlier attempt measured. `files/closure` as a gate flagged 80 of 117 path
tokens across 28 tasks — a 68% false-flag rate — because the plan format requires a read-only
reference to stay off the `Files` line (`docs/ideas/plan-flow-mechanical-verification.md`,
"Deferred: `tomlctl plan lint`"). What ships is that check narrowed to four path roots, skipping
the lines whose prose marks a reference as read-only, and emitted at `info` so it can never
gate; `/review-plan` keeps its own Files-line closure sub-check as prose regardless.
`dag/symbol-without-edge` reads only spans an introducing verb covers, for the same reason — a
bare path- or identifier-token scan is what measured unusable.

### Finding shape

| Key | Value | Meaning |
|---|---|---|
| `class` | class name | A row of the table above. |
| `severity` | `error` \| `warning` \| `info` | Only `error` moves the exit code; `info` is reserved for a list a reader judges, and `files/closure` and `backlog/closed` are its members. |
| `ids` | ids | The tasks concerned. Empty for the whole-store classes — `dag/unbuildable`, the four `policy/*`, `plan/policy-absent`, `plan/no-tasks` and `render/drift`. |
| `detail` | text | The diagnostic, naming what `ids` cannot, such as a `ref`. |
