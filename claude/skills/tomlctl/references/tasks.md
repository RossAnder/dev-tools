# tomlctl — tasks reference

The flag surface of the `tomlctl tasks` group, which reads and writes
`.claude/flows/<slug>/tasks.toml`: a per-flow task DAG with the plan's execution policy and
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
- [`tasks remove`](#tasks-remove)
- [`tasks show`](#tasks-show)
- [`tasks list`](#tasks-list)
- [`tasks edges`](#tasks-edges)
- [`tasks ready`](#tasks-ready)
- [`tasks batches`](#tasks-batches)
- [`tasks closure`](#tasks-closure)
- [`tasks check`](#tasks-check)
- [`tasks render`](#tasks-render)
- [Store target](#store-target)
- [The `check` finding classes](#the-check-finding-classes)

The store's own half — its keys and `[[items]]` fields, `ref` derivation, the graph products
recomputed on every read, the 256-node cap and the frozen contracts — is
[tasks-store.md](tasks-store.md).

Every mutating verb (`import-plan`, `add`, `add-many`, `update`, `remove`) carries the shared write
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
| `--reconcile-record` | — | Also read the flow's execution record — the path `context.toml` records under `[artifacts].execution_record`, or `execution-record.toml` beside the store when it records none — mark matched rows `done`, and adopt the record's spelling of a `ref` on a unique normalised match. Requires `--slug`. | off |
| `--dry-run` | — | Report the import without writing. File and sidecar stay byte-identical. | off |

An error-class finding **refuses a real import** before the file is touched; under `--dry-run`
the same finding is reported in the envelope and the exit code stays `0`. A `plan_path` read
from `context.toml` that is absolute, carries a `..` component, resolves outside the repo root,
or does not name a `.md` document is refused with `kind=validation` — the verb is not an
arbitrary-file oracle. An `[artifacts].execution_record` override is held to the same
repo-relative containment; only `plan_path` also has to name a `.md` document. An explicit
`--plan` still reads an absolute or subdirectory-relative argument, but the path the import
**records** goes through that same check — the resolved document is relativised against the
repo root and refused unless it lands under the root as a `.md` path, so what an import records
and what a later render accepts cannot drift apart.

```json
{"ok":true,"added":3,"updated":1,"unchanged":32,"removed_refs":[],"added_refs":["wire-the-render-verb"],
 "adopted_refs":["arm_the_sort_engaged_test"],"unmatched_refs":[],"cleared_checkpoint_refs":[],"findings":[]}
```

`added` + `updated` + `unchanged` counts the *plan's* tasks. `adopted_refs` names refs whose
spelling came from the execution record rather than the heading; `unmatched_refs` names record
`task_ref`s no plan task claimed. `cleared_checkpoint_refs` is the subset of `removed_refs`
whose `checkpoint` named a group this plan no longer declares: membership is recomputed for
every row the plan produces, so a retained row was the one way an undeclared group id stayed
in the store. The row survives; the id is blanked.

A row `tasks update --unlock-import-fields` hand-patched carries an `[[import_overrides]]`
entry holding the plan values that patch replaced. The import keeps the row's `files` /
`needs` while the plan still states that base — `plan/override-held` — and takes the plan's
value back, dropping the entry, the moment the plan states anything else —
`plan/override-released`. Membership, not order, is the comparison: reordering a `Files` line
states no new value.

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
| `--checkpoint` | group id | Checkpoint group. A group id no `[[checkpoints]]` entry declares is refused, naming the ones it does. Omit for a row only the final commit train commits. | empty |
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
| `--checkpoint` | group id | Checkpoint group; pass an empty value to clear it. An undeclared group id is refused, through this flag and through `--set checkpoint=` alike. | — |
| `--ref` | slug | Rewrite the `ref`. Never inferred from a retitle. | — |
| `--unlock-import-fields` | — | Permit `--set files=` and `--set needs=`, stamping the row with the plan values the patch replaces. Conflicts with `--relock-import-fields`. | off |
| `--relock-import-fields` | — | Drop that stamp: the next import restores whatever the plan states. Values are left as they are. | off |
| `--set` | `KEY=VAL`, repeatable | Any other settable field. | — |

`--set` reaches `acceptance`, `action`, `agent`, `checkpoint`, `commit`, `coupling`, `deps_note`,
`detail`, `effort`, `status`, `title`, plus `files` and `needs` under `--unlock-import-fields`.
Three refusals carry their own hint instead of the generic list: `ref` is immutable through
`--set` and must be rewritten with `--ref`; `id` is minted by the store; `files` and `needs` are
owned by `tasks import-plan`, and that hint names the unlock flag. Passing the same field both as
a named flag and as `--set` errors rather than picking one.

`coupling`, `needs` and `files` take a comma-separated list — the same one `tasks add` takes —
and an empty value clears it. A `--set files=` patch carries no per-path annotations; those are
import-owned and are re-derived from the plan for the paths the row still claims. The two edge fields are validated against the whole store rather
than the row alone: a dangling target or an edge that would close a cycle is refused before the
row moves.

An unlocked patch records the value it replaced as the stamp's *base*, and `changed` reports
`import_override` alongside the field. The base is the plan's own value, so a second patch keeps
the first one's base rather than overwriting it, and a patch back to the base drops the stamp —
a row that agrees with the plan overrides nothing. `--ref` carries the stamp with the row.
There are three ways out: publish the patch with `tasks render` and re-import, let the plan
change the line, or `--relock-import-fields`. The unlock is not a pin: `tasks render --check`
reports the divergence as `render/drift` from the moment of the patch, and every import that
sees a changed line reports `plan/override-released` and takes the plan's value back. `--set title=` is refused when the new title derives a different `ref` than the old one
did, unless `--ref` rides along in the same invocation; the refusal names the ref the new title
derives to. A retitle deriving the same ref lands on its own. The comparison is old derivation
against new, deliberately not against the stored `ref` — a duplicate title is minted a suffixed
ref and `import-plan --reconcile-record` adopts a record's ref, so a stored ref legitimately
diverges from what its title derives.

`--ref` refuses an empty slug and a slug another row already holds. It does **not** rewrite the
execution record — a rename orphans the row's `task_ref` there until the next
`import-plan --reconcile-record` re-adopts it.

```json
{"ok":true,"id":12,"changed":["agent","status"]}
```

## `tasks remove`

Hard-deletes one row — the only verb that does. `import-plan` keeps every row it stops
producing, so a task deleted from the plan is retired here or not at all.

```bash
tomlctl tasks remove <id> --slug <slug>
tomlctl tasks remove <id> --slug <slug> --force
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id | Task to remove. Required; an id no row carries is refused. | — |
| `--force` | — | Remove a row past `pending`, or one other rows depend on. Re-points each dependent's edges at the removed row's own dependencies. | off |

Without `--force` two removals are refused, each naming what stands in the way: a row whose
status is not `pending`, named with its `ref` — the execution record's `task_ref` and the commit
train's SHA both join on it — and a row other rows depend on, named with those dependents. A
refusal writes nothing.

Under `--force` the removed id is pruned from every dependent's `needs` and `coupling`, **and
the removed row's own `needs` are spliced into each dependent's `needs`**: a dependent left one
edge short would dispatch ahead of work it still waits on, and one left pointing at the removed
id would make the store `dag/dangling-ref`. `rewired` names the rows whose edge sets moved,
ascending. There is no `--dry-run`.

A successful removal also drops the row's `[[import_overrides]]` entry, so no stamp outlives the
row it keys on. The prune runs on the success path only, and `pruned_override_fields` names the
stamped fields it took — a subset of `files`, `needs`, in that order — so a caller can see which
values the next import would take from the plan instead. Like `update`'s `changed` and
`import-plan`'s `cleared_checkpoint_refs`, it is always present and empty when nothing was pruned.

```json
{"ok":true,"id":21,"ref":"wire-the-render-verb","rewired":[24,25],"pruned_override_fields":["files"]}
```

| Key | Value | Meaning |
|---|---|---|
| `ok` | `true` | Success path only; a refusal arrives as a `kind=validation` error, never as `ok: false`. |
| `id` | id | The removed row, echoing the positional argument. |
| `ref` | slug | The removed row's `ref` — the join key the execution record and the commit train hold. |
| `rewired` | ids, ascending | Rows whose `needs` / `coupling` the removal moved. Empty when nothing depended on the row. |
| `pruned_override_fields` | subset of `files`, `needs` | Stamped fields the dropped `[[import_overrides]]` entry held. Empty when the row carried no stamp. |

## `tasks show`

```bash
tomlctl tasks show <id> --slug <slug> --with body,files,deps
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | id | Task to print. Required. | — |
| `--with` | comma-separated, repeatable | `summary`, `body`, `files`, `deps`, `dependents`. A clap `value_enum`: an unknown part exits `2` with usage prose naming it and listing the valid set, **outside** the `--error-format json` envelope — not the exit-`1` `kind=validation` error `--effort` and `--status` raise. | `summary` |

`id` is always emitted whatever `--with` selects, so a fetched row can be matched back to the
id that was asked for. `summary` is `id`, `ref`, `title`, `effort`, `status`, `checkpoint`,
`files`, `needs`, `coupling`; `body` adds `action`, `detail`, `acceptance`; `deps` is a summary
per direct `needs` ∪ `coupling` target; `dependents` is the transitive successor set, excluding
the row itself. Only `dependents` builds the graph, so a store with a cycle or a dangling edge
still shows its rows under every other part.

```json
{"id":12,"ref":"check-side-arm-width","title":"Check side-arm width","effort":"S",
 "status":"pending","checkpoint":"A","files":["src/overflow.tsx"],"needs":[10],"coupling":[],
 "action":"…","detail":"…","acceptance":"…","deps":[{"id":10,"ref":"…"}],
 "import_override":{"files":["src/legacy.tsx"],"needs":[9]}}
```

`import_override` is emitted last and is not gated on `--with`. It holds the plan **base** each
stamped field replaced — not the patched `files` / `needs` above it — with an unstamped field
omitted and the whole key absent from an unstamped row, so output for a store nothing has
hand-patched is unchanged. It carries no `ref` (the row has one), and the summaries nested under
`deps` / `dependents` never carry it: the stamp belongs to the row that was asked for.

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
| `--kind` | `needs` \| `coupling` \| `overlap` | Restrict to one kind. Omit for all three. A clap `value_enum`, failing exactly as `tasks show --with` does: an unknown kind exits `2` with usage prose, **outside** the `--error-format json` envelope. | all |
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
| `--in-flight` | comma-separated ids | Tasks currently dispatched. A ready row sharing a file with one of them is reported as held, and a row waiting on one stays in `next` rather than `blocked`. | empty |

```json
{"ready":[4],"held":[{"id":3,"blocked_on_file":"engine.rs","holder":2}],"next":[5],"blocked":[]}
```

`ready` excludes what `held` names, so it is directly dispatchable. `next` is what becomes
ready once the current round lands. `blocked` names the rows no later wave can reach — each
with the nearest **ancestor** stranding it and that ancestor's status, which an ancestor that is
neither `done`, nor `pending`, nor named in `--in-flight` produces. The walk climbs through
intervening `pending` rows, so `blocker` is the stall itself rather than a pending row between
it and the blocked row. Every id list is ascending.
An `--in-flight` id no row carries is refused, as is a cyclic store: Kahn strands a cycle's
members and a stranded task reads in a frontier exactly like one that is merely waiting.

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
{"checkpoint":"A","members":[1,2,3,4],"maximal":[4],"dependency_closure":[1,2,3,4],"valid_cut":true}
{"task":12,"direction":"up","ids":[3,7,10,12]}
```

`valid_cut` covers the union of the group with every earlier one, so it reads as "committable
here", not "self-contained".

`members` is the group; `dependency_closure` is everything the group reaches upward, which is
the set the rendered `CHECKPOINT` marker prints under that same name. They coincide exactly when
every dependency already sits in an earlier group, which is the case a reader cannot use to tell
them apart — so both are reported.

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

```json
{"ok":false,"findings":[{"class":"dag/cycle","severity":"error","ids":[2,3,4],
                         "detail":"tasks 2, 3, 4 form a dependency cycle"}]}
```

Findings sort by `(class, ids)`. **Exit `1` when any finding is error-class, `0` otherwise** —
warnings alone exit `0`. See [the check finding classes](#the-check-finding-classes) for the
full list. Under `--plan` the plan is resolved exactly as `render` resolves it, and the same
containment refusal applies. `render/drift` is a warning, so it is reported without moving the
exit code: `check --plan` still exits `0` on a drifted plan, and
[`tasks render --check`](#tasks-render) is the mode to gate on instead.

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
under `--file`, never from an argument, and is refused if absolute, `..`-bearing, outside the
repo root, or not a `.md` document. Under `--slug` a store whose own `plan_path` resolves to a
different document than the context's is refused too, until the plan is re-imported.

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
| `dag/duplicate-number` | error | `check` — `ids` carries the number alone, so the `detail` names the `ref` of every row on it; `import-plan` splits those refs into the ones the plan produced and the ones the store still holds |
| `dag/unbuildable` | error | `check` — the graph engine refuses the store for a reason no row scan named; past the [node cap](tasks-store.md#the-256-node-cap) is the live case |
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
| `plan/orphan-row` | warning, or **error** when the row's status is not `pending` | `check` — the `detail` names every orphaned `ref`, which `ids` cannot: two orphaned rows sharing a task number collapse to one id |
| `plan/override-held` | warning | `import-plan` only — a hand-patched `files` or `needs` the plan does not state; run `tasks render` to publish it |
| `plan/override-released` | warning | `import-plan` only — the plan restated the line, so its value replaced the hand-patched one and the stamp is gone |
| `render/drift` | warning | `check --plan` and `render --check` |

Severity is `error`, `warning` or `info`. Only `error` moves the exit code; `info` is reserved
for a list a reader judges, and `files/closure` is its one member.

Every finding is `{class, severity, ids, detail}`; `ids` is empty for the whole-store classes —
`dag/unbuildable`, the four `policy/*`, `plan/policy-absent`, `plan/no-tasks` and
`render/drift`. A duplicate id and an edge to an absent task are scanned for *before* any graph
is built, which is what lets one run report every defect instead of dying on the first, and is
why either of them suppresses `dag/unbuildable` rather than doubling it.

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
