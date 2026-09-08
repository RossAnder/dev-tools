# tomlctl — tasks reference

The flag surface of the `tomlctl tasks` group and the shape of the store it writes:
`.claude/flows/<slug>/tasks.toml`, a per-flow task DAG with the plan's execution policy and
checkpoint groups beside it. Every verb resolves its store through one shared target group
(`--slug` or `--file`) and emits JSON on stdout. What the fields *mean*, which verb a carrier
reaches for, and the rules binding `/plan-new`, `/implement`, `/review-plan` and
`/plan-update` are the `flow-contract-task-store` skill's job
(`claude/skills/flow-contract-task-store/SKILL.md`); this file is the flag table.

## Contents

- [`tasks import-plan`](#tasks-import-plan)
- [`tasks add`](#tasks-add)
- [`tasks add-many`](#tasks-add-many)
- [`tasks update`](#tasks-update)
- [`tasks show`](#tasks-show)
- [`tasks list`](#tasks-list)
- [`tasks edges`](#tasks-edges)
- [`tasks ready`](#tasks-ready)
- [`tasks batches`](#tasks-batches)
- [`tasks closure`](#tasks-closure)
- [`tasks check`](#tasks-check)
- [`tasks render`](#tasks-render)
- [Store target](#store-target)
- [Store shape](#store-shape)
- [Ref derivation](#ref-derivation)
- [The `check` finding classes](#the-check-finding-classes)
- [Derived graph products](#derived-graph-products)
- [The 256-node cap](#the-256-node-cap)
- [Frozen contracts](#frozen-contracts)

Every mutating verb (`import-plan`, `add`, `add-many`, `update`) carries the shared write
bundle — `--allow-outside`, `--no-create`, `--no-write-integrity`, `--strict-integrity`,
`--verify-integrity` — and every read verb (`show`, `list`, `edges`, `ready`, `batches`,
`closure`, `check`, `render`) the read bundle, `--verify-integrity` and `--strict-read`.
A `--slug` target lands inside the `.claude/` containment guard, so none of these needs
`--allow-outside`; a `--file` target outside `.claude/` does, on the write verbs.
`render` is the exception in the other direction: it is a derived write to the *plan*, resolved
from the flow context or the store's recorded `plan_path` rather than from an argument, so it
carries the read bundle and no write flag has a hook on it. Every verb also takes the global
`--error-format text|json`; see [flow.md](flow.md#error-format---error-format-json) for the
JSON envelope and its `kind` taxonomy.

## `tasks import-plan`

Parses a plan's `## Tasks`, `## Execution Policy` and `## Dependency Graph` sections and
upserts the store keyed on each row's `ref`. Existing rows keep `status`, `agent`, `commit`
and `coupling`; new rows arrive `pending`; **nothing is deleted** — a row the plan no longer
produces stays put and is named in `removed_refs`.

```bash
tomlctl tasks import-plan --slug <slug> --reconcile-record
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--slug` / `--file` | see [Store target](#store-target) | Store to upsert. Both may be omitted under `--dry-run` (plan mode). | — |
| `--plan` | path | Plan markdown to read. Under `--slug`, defaults to the flow context's `plan_path`. A relative value resolves against the repo root when it does not resolve against the cwd. | context's `plan_path` |
| `--reconcile-record` | — | Also read the flow's `execution-record.toml`, mark matched rows `done`, and adopt the record's spelling of a `ref` on a unique normalised match. Requires `--slug`. | off |
| `--dry-run` | — | Report the import without writing. File and sidecar stay byte-identical. | off |

An error-class finding **refuses a real import** before the file is touched; under `--dry-run`
the same finding is reported in the envelope and the exit code stays `0`. A `plan_path` read
from `context.toml` that is absolute or carries a `..` component is refused with
`kind=validation` — the verb is not an arbitrary-file oracle. An explicit `--plan` is a caller
argument and passes through.

```json
{"ok":true,"added":3,"updated":1,"unchanged":32,"removed_refs":[],"added_refs":["wire-the-render-verb"],
 "adopted_refs":["arm_the_sort_engaged_test"],"unmatched_refs":[],"findings":[]}
```

`added` + `updated` + `unchanged` counts the *plan's* tasks. `adopted_refs` names refs whose
spelling came from the execution record rather than the heading; `unmatched_refs` names record
`task_ref`s no plan task claimed.

## `tasks add`

Appends one row, minting `max(id) + 1` and deriving the `ref` from the title. A dangling
dependency target or a cycle is refused before the write, so a rejected add leaves file and
sidecar untouched.

```bash
tomlctl tasks add --slug <slug> --title "Extract the token reader" --effort S --files src/auth/token.rs,src/auth/session.rs --needs 3,4 --coupling 7 --deps-note "4 is what the acceptance exercises" --checkpoint B --action "Move read_token out of session.rs" --acceptance-file <path>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--title` | text | Row title; the `ref` is derived from it. Required. Empty, or carrying no slug characters, errors. | — |
| `--effort` | `S` \| `M` \| `L` | Validated after parsing, so an unrecognised value is a `kind=validation` error rather than clap usage prose. Required. | — |
| `--files` | comma-separated paths | Repo-relative paths the task edits. | empty |
| `--needs` | comma-separated ids | Dependency edges. | empty |
| `--coupling` | comma-separated ids | Acceptance-reachability edges. Counted in the in-degree exactly like `--needs`. | empty |
| `--deps-note` | text | Prose remainder of the plan's `Depends on` line, stored verbatim. | empty |
| `--checkpoint` | group id | Checkpoint group. Omit for a row only the final commit train commits. | empty |
| `--action` / `--action-file` | text / path | Action body, literal or from a file. Mutually exclusive. | empty |
| `--detail` / `--detail-file` | text / path | Detail body. Mutually exclusive. | empty |
| `--acceptance` / `--acceptance-file` | text / path | Acceptance body. Mutually exclusive. | empty |

A title colliding with a stored `ref` takes the first free `-2` / `-3` suffix rather than
replaying the document-order numbering, which would re-derive a suffix another row holds.

```json
{"ok":true,"id":37,"ref":"extract-the-token-reader","batch":4}
```

`batch` is the zero-based Kahn round the row lands in once stored.

## `tasks add-many`

The same minting and validation over NDJSON, one row object per line, **all-or-nothing**: a
malformed line, a dangling target or a cycle aborts before the file is mutated. Two rows in
one batch may reference each other; a cycle between them is still refused.

```bash
printf '%s\n' '{"title":"Add the retry guard","effort":"S","files":["src/retry.rs"],"needs":[3]}' | tomlctl tasks add-many --slug <slug> --ndjson -
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ndjson` | `-`, path, or `@path` | NDJSON source; `-` reads stdin. Required. | — |

Accepted row keys are exactly `title`, `effort`, `files`, `needs`, `coupling`, `deps_note`,
`checkpoint`, `action`, `detail`, `acceptance`. **An unknown key is a hard error** naming the
row's 1-based index and listing that set — a mistyped `neds` would otherwise silently discard
an edge. An absent key is the empty value; a wrong JSON type names the key and the type it got.

```json
{"ok":true,"added":2,"rows":[{"id":37,"ref":"add-the-retry-guard","batch":4},
                             {"id":38,"ref":"cover-the-retry-guard","batch":5}]}
```

## `tasks update`

Patches one row's mutable fields. Every assignment is compared against what the row already
holds, so `changed` reports what moved rather than what was passed — a re-issued
`--status done` reports `[]`.

```bash
tomlctl tasks update <id> --slug <slug> --status done --agent implement-deep --commit <sha>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id | Task to patch. Required. | — |
| `--status` | text | `pending`, `in-progress`, `done`, `failed`, `deferred`. Case-sensitive; validated after parsing. | — |
| `--agent` | text | Agent the row was dispatched to. | — |
| `--commit` | SHA | Commit the row landed in. | — |
| `--checkpoint` | group id | Checkpoint group; pass an empty value to clear it. | — |
| `--ref` | slug | Rewrite the `ref`. Never inferred from a retitle. | — |
| `--set` | `KEY=VAL`, repeatable | Any other settable field. | — |

`--set` reaches `acceptance`, `action`, `agent`, `checkpoint`, `commit`, `deps_note`, `detail`,
`effort`, `status`, `title`. Three refusals carry their own hint instead of the generic list:
`ref` is immutable through `--set` and must be rewritten with `--ref`; `id` is minted by the
store; `files`, `needs` and `coupling` are owned by `tasks import-plan`. Passing the same field
both as a named flag and as `--set` errors rather than picking one.

`--ref` refuses an empty slug and a slug another row already holds. It does **not** rewrite the
execution record — a rename orphans the row's `task_ref` there until the next
`import-plan --reconcile-record` re-adopts it.

```json
{"ok":true,"id":12,"changed":["agent","status"]}
```

## `tasks show`

```bash
tomlctl tasks show <id> --slug <slug> --with body,files,deps
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id | Task to print. Required. | — |
| `--with` | comma-separated, repeatable | `summary`, `body`, `files`, `deps`, `dependents`. | `summary` |

`id` is always emitted whatever `--with` selects, so a fetched row can be matched back to the
id that was asked for. `summary` is `id`, `ref`, `title`, `effort`, `status`, `checkpoint`,
`files`, `needs`, `coupling`; `body` adds `action`, `detail`, `acceptance`; `deps` is a summary
per direct `needs` ∪ `coupling` target; `dependents` is the transitive successor set, excluding
the row itself. Only `dependents` builds the graph, so a store with a cycle or a dangling edge
still shows its rows under every other part.

```json
{"id":12,"ref":"check-side-arm-width","title":"Check side-arm width","effort":"S",
 "status":"pending","checkpoint":"A","files":["src/overflow.tsx"],"needs":[10],"coupling":[],
 "action":"…","detail":"…","acceptance":"…","deps":[{"id":10,"ref":"…"}]}
```

An unknown id errors with `kind=not_found`.

## `tasks list`

Task rows through the generic query engine — the whole `items list` surface applies, because
the store's array *is* `items`. See [query.md](query.md) for that half: `--where-*` predicates,
`--select` / `--exclude` / `--pluck`, `--sort-by` / `--limit` / `--offset`, `--count-by` /
`--group-by`, `--raw` / `--lines` / `--ndjson`.

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

## `tasks edges`

```bash
tomlctl tasks edges --slug <slug> --kind overlap
tomlctl tasks edges --slug <slug> --dot
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--kind` | `needs` \| `coupling` \| `overlap` | Restrict to one kind. Omit for all three. | all |
| `--dot` | — | Emit Graphviz DOT source on stdout instead of the JSON edge list. | off |

`from` is the prerequisite and `to` the row that waits on it, so a JSON edge and its DOT arrow
read the same way round. Edges are grouped by kind in declaration order and ascending within
each. `needs` and `coupling` come straight off the rows; `overlap` is computed and is the one
kind that needs a graph, so it is also the one kind a malformed store cannot answer.

```json
[{"kind":"needs","from":10,"to":12},{"kind":"overlap","from":3,"to":9}]
```

Under `--dot` every task is a node whatever `--kind` selects, so a filtered graph still renders
the whole plan; `coupling` arrows are dashed and `overlap` dotted and undirected.

## `tasks ready`

```bash
tomlctl tasks ready --slug <slug> --in-flight 3,4
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--in-flight` | comma-separated ids | Tasks currently dispatched. A ready row sharing a file with one of them is reported as held. | empty |

```json
{"ready":[4],"held":[{"id":3,"blocked_on_file":"engine.rs","holder":2}],"next":[5]}
```

`ready` excludes what `held` names, so it is directly dispatchable. `next` is what becomes
ready once the current round lands. Every id list is ascending. An `--in-flight` id no row
carries is refused, as is a cyclic store: Kahn strands a cycle's members and a stranded task
reads in a frontier exactly like one that is merely waiting.

## `tasks batches`

```bash
tomlctl tasks batches --slug <slug>
```

No flags beyond the read bundle and the target.

```json
{"batches":[[1,4],[2],[3]]}
```

Kahn layers in dependency order, each ascending. In-degree is `needs` ∪ `coupling`, so a
coupling edge pushes its dependent a layer back. A cycle is refused rather than layered — the
layering drops what a cycle strands, so the answer would omit tasks instead of naming them.

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

```json
{"checkpoint":"A","members":[1,2,3,4],"maximal":[4],"valid_cut":true}
{"task":12,"direction":"up","ids":[3,7,10,12]}
```

`valid_cut` covers the union of the group with every earlier one, so it reads as "committable
here", not "self-contained".

## `tasks check`

```bash
tomlctl tasks check --slug <slug>
tomlctl tasks check --slug <slug> --plan
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--plan` | — | Also compare the plan markdown against the render output and report `render/drift`. | off |

```json
{"ok":false,"findings":[{"class":"dag/cycle","severity":"error","ids":[2,3,4],
                         "detail":"tasks 2, 3, 4 form a dependency cycle"}]}
```

Findings sort by `(class, ids)`. **Exit `1` when any finding is error-class, `0` otherwise** —
warnings alone exit `0`. See [the check finding classes](#the-check-finding-classes) for the
full list. Under `--plan` the plan is resolved exactly as `render` resolves it, and the same
containment refusal applies.

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

```json
{"ok":true,"path":"docs/plans/<slug>.md","sections":["Execution Policy","Tasks","Dependency Graph"]}
{"ok":false,"path":"docs/plans/<slug>.md","findings":[{"class":"render/drift","severity":"warning","ids":[],
  "detail":"plan sections out of date with the store: Tasks"}]}
```

A missing section is inserted in canonical order (Execution Policy before Tasks, Dependency
Graph after Tasks). The source document's dominant line ending is detected and re-applied, so a
CRLF plan is not rewritten to mixed endings and `--check` does not report permanent drift.
Output is a derived write like `PROGRESS-LOG.md` — **no `.sha256` sidecar**.

`render` errors rather than writing on a cycle, a dangling edge, or a graph past the node cap.
The target path comes from the flow context under `--slug` and from the store's `plan_path`
under `--file`, never from an argument, and is refused if absolute, `..`-bearing, or outside
the repo root.

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

## Store shape

`.claude/flows/<slug>/tasks.toml` carries four top-level keys, a `[policy]` table, a
`[[checkpoints]]` array and an `[[items]]` array. Keys are written in that order and the order
is stable across writes, so a re-render is a clean diff.

| Key | Set by |
|---|---|
| `schema_version` | seeded to `1` by `flow init` and by the auto-create write path |
| `last_updated` | a bare TOML date, restamped on every mutation |
| `plan_path` | `import-plan`, from the flow context under `--slug` or `--plan` under `--file` |
| `last_import_refs` | `import-plan` — the ref set the last import produced |

`[policy]`: `checkpoints` (`single` \| `milestones` \| `per-batch`), `max_parallel` (1–8),
`commit_granularity` (`per-task` \| `per-checkpoint` \| `single-commit`), `note`. The two
vocabulary fields are stored as strings so `tasks check`, not the reader, decides what is out
of vocabulary. An absent or partial table falls back to `milestones` / `6` / `per-task`.

`[[checkpoints]]`: `id` and `rationale`, in marker order. Group *membership* is not here — it
is the `checkpoint` field on each row.

`[[items]]`: `id`, `ref`, `title`, `effort`, `status`, `checkpoint`, `files`, `needs`,
`coupling`, `deps_note`, `action`, `detail`, `acceptance`, `agent`, `commit`. The plan owns
every field but four: `status`, `agent`, `commit` and `coupling` survive every re-import.

Unknown keys are dropped — the store is tool-owned and every write re-emits it from this shape.
Bodies are LF-normalised on both read and write, and a body containing a backslash serialises as
a `'''` literal string, which golden fixtures must expect.

The array is named `items` so the generic `tomlctl items` query machinery applies. Two
subcommands, `items orphans` and `items find-duplicates`, hardcode the review/optimise ledger
schema and emit garbage against this one.

## Ref derivation

A row's `ref` is its plan heading's title — the text after the `N. ` number, before any
trailing ` [S|M|L]` tag — lowercased, with runs of whitespace and hyphens collapsed to a single
hyphen, underscores kept, and every other character dropped. The precedent is the GitHub
heading-anchor rule; the two divergences (collapsing runs, keeping underscores) exist because
`Setup & Config` must not slug to `setup--config` and because execution records already on disk
carry `task_ref`s containing underscores.

Repeats are suffixed in document order — first heading keeps the bare ref, later ones take
`-2`, `-3`. `tasks add` instead scans for the first *free* suffix, because replaying the
document-order numbering over `existing ++ new` would re-derive a suffix another row holds.

A title carrying no slug characters is refused rather than keyed to an empty string.

The matcher `--reconcile-record` compares on collapses a ref to `[a-z0-9]`, so `count_distinct`
and `count-distinct` match. Adoption of a record's spelling demands a normalised form unique on
both sides and free on the plan side — a rename onto a sibling's key would silently merge two
tasks.

## The `check` finding classes

| Class | Severity | Raised by |
|---|---|---|
| `dag/cycle` | error | `check` — suppresses every class needing reachability |
| `dag/dangling-ref` | error | `check` |
| `dag/duplicate-number` | error | `check` |
| `dag/unreachable-claim` | warning | `check` — shared file, no directed path either way |
| `checkpoint/orphan-task` | warning | `check` — `checkpoint = ""` |
| `checkpoint/invalid-cut` | error | `check` — prefix union not downward-closed; `ids` are the dependencies that must move earlier |
| `checkpoint/marker-mismatch` | warning | `import-plan` only — the authored `Checkpoint after` bullet is never stored |
| `policy/max-parallel-range` | error | `check` |
| `plan/orphan-row` | warning, or **error** when the row's status is not `pending` | `check` |
| `render/drift` | warning | `check --plan` and `render --check` |

Every finding is `{class, severity, ids, detail}`; `ids` is empty for the two whole-store
classes. A duplicate id and an edge to an absent task are scanned for *before* any graph is
built, which is what lets one run report every defect instead of dying on the first.

`plan/orphan-row` fires only when `last_import_refs` is non-empty — an empty ref set means the
store has never been imported, not that every row is orphaned.

`files/closure` is deliberately absent: measured at a 68% false-flag rate.

## Derived graph products

Only `needs`, `coupling` and each row's `checkpoint` are stored. Everything else is recomputed
on every read: the ready frontier and its file-claim holds, Kahn layers, `closure_up` /
`closure_down`, `overlap` pairs, each group's maximal elements and `valid_cut`, and the
`Checkpoint after` bullet the renderer writes. A checkpoint group is committable exactly when
the union of it with every earlier group is a **lower set** under the dependency order; the
`Checkpoint after` list is that set's **antichain of maximal elements**, which is why naming
the antichain is enough to reconstruct the group.

Determinism is total: ascending id tie-breaks everywhere, layers sorted ascending, groups in
`[[checkpoints]]` order, edges grouped by kind in declaration order.

## The 256-node cap

The engine's reachability bitsets are fixed-width, so a store past **256 rows** fails graph
construction with `graph exceeds 256 tasks`. `ready`, `batches`, `closure`, `render`,
`edges --kind overlap` and `add`'s cycle check all surface that as a refusal.

`tasks check` is the exception: it reports its non-graph classes (`policy/*`,
`dag/duplicate-number`, `dag/dangling-ref`, `checkpoint/orphan-task`, `plan/orphan-row`) and
silently omits the graph-derived ones. A clean `check` on a store above the cap is therefore
not a clean bill of the DAG.

## Frozen contracts

Behaviours downstream carriers depend on. Changing any of these is a breaking change to the
four adopting commands, not a refactor.

- **`ref` is the primary key and the upsert key.** Import matches on it, the execution record's
  `task_ref` joins on it, `last_import_refs` is a set of them.
- **Import never deletes.** A row the plan stopped naming is kept and reported in
  `removed_refs`; only `plan/orphan-row` speaks to it.
- **A renamed heading is a refusal, not a swap.** The old row keeps the task number, the
  renamed heading arrives with the same number and a new ref, and the import raises
  `dag/duplicate-number` at error severity and writes nothing. Recovery is
  `tasks update <id> --ref <new-ref>`, then re-import — which is why the ref-set diff gate is
  specified as a `--dry-run` step.
- **`status`, `agent`, `commit` and `coupling` survive every import.** Everything else on a row
  is plan-owned and overwritten.
- **A cyclic store is refused, never partially answered**, by `ready`, `batches`, `closure` and
  `render`.
- **`render` owns three sections and preserves every other byte** of the plan document, and
  writes no `.sha256` sidecar.
- **`flow ensure-artifact --kind tasks` reports; it never seeds.** `--bootstrap --kind tasks` is
  a no-op that writes nothing — seeding is `flow init`'s job.
