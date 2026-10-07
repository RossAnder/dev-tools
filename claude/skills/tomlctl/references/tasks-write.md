# tomlctl — tasks write reference

The flag surface of the five `tomlctl tasks` verbs that mutate
`.claude/flows/<slug>/tasks.toml`. The read verbs, the shared `--slug` / `--file` target group,
the output-table conventions and the `check` finding classes are [tasks.md](tasks.md); the
store's own shape is [tasks-store.md](tasks-store.md). What the fields *mean* and which verb a
carrier reaches for are the `flow-contract-task-store` skill's job
(`claude/skills/flow-contract-task-store/SKILL.md`).

## Contents

- [`tasks import-plan`](#tasks-import-plan)
- [`tasks add`](#tasks-add)
- [`tasks add-many`](#tasks-add-many)
- [`tasks update`](#tasks-update)
- [`tasks remove`](#tasks-remove)

Every verb here carries the shared write bundle — `--allow-outside`, `--no-create`,
`--no-write-integrity`, `--strict-integrity`, `--verify-integrity` — and resolves its store
through the [store target](tasks.md#store-target). A `--slug` target lands inside the
`.claude/` containment guard, so none of these needs `--allow-outside`; a `--file` target
outside `.claude/` does. Each prints JSON keyed as its `| Key |` table lists, read by the
[output-field conventions](tasks.md#output-fields).

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
| `--slug` / `--file` | see [Store target](tasks.md#store-target) | Store to upsert. Both may be omitted under `--dry-run` (plan mode). | — |
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

A row `tasks update --unlock-import-fields` hand-patched carries an `[[import_overrides]]`
entry holding the plan values that patch replaced. The import keeps the row's `files` /
`needs` while the plan still states that base — `plan/override-held` — and takes the plan's
value back, dropping the entry, the moment the plan states anything else —
`plan/override-released`. Membership, not order, is the comparison: reordering a `Files` line
states no new value.

A task's `- **Backlog**:` bullet — `B-aaaa1111, refs B-bbbb2222` — is rebuilt into
`[[backlog_links]]` on every import, keyed by the row's final `ref`: a bare id is a `closes`
link, and `refs` qualifies only the entry it opens. When the imported store holds any link the
import reads `.claude/backlog.toml` and adds the [`backlog/*` findings](tasks.md#the-check-finding-classes);
`backlog/unknown-id` is error-class, so it refuses a real import. Plan mode has no slug, so it
never raises `backlog/claimed-elsewhere`.

| Key | Value | Meaning |
|---|---|---|
| `ok` | bool | `false` when a finding is error-class, which only `--dry-run` reports — a real import refuses instead. |
| `added` | count | Plan tasks no stored row matched. `added` + `updated` + `unchanged` counts the *plan's* tasks. |
| `updated` | count | Plan tasks whose stored row the import changed, an adopted `ref` included. |
| `unchanged` | count | Plan tasks whose stored row already matched. |
| `removed_refs` | refs | Stored rows the plan no longer produces. Kept, not deleted. |
| `added_refs` | refs | The rows `added` counts. |
| `adopted_refs` | refs | Refs whose spelling came from the execution record rather than the heading. Empty without `--reconcile-record`. |
| `unmatched_refs` | refs | Record `task_ref`s no plan task claimed. Empty without `--reconcile-record`. |
| `cleared_checkpoint_refs` | refs | The subset of `removed_refs` whose `checkpoint` named a group this plan no longer declares: membership is recomputed for every row the plan produces, so a retained row was the one way an undeclared group id stayed in the store. The row survives; the id is blanked. |
| `findings[]` | [findings](tasks.md#finding-shape) | The import's own classes and the store check's. |

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

| Key | Value | Meaning |
|---|---|---|
| `ok` | `true` | Success path only; a refusal arrives as a `kind=validation` error. |
| `id` | id | The minted id. |
| `ref` | slug | The derived `ref`, suffixed when the title collided. |
| `batch` | integer | Zero-based Kahn round the row lands in once stored — its index in [`tasks batches`](tasks.md#tasks-batches). |

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

| Key | Value | Meaning |
|---|---|---|
| `ok` | `true` | Success path only; any refusal lands nothing. |
| `added` | count | Rows appended — every row, since the batch is all-or-nothing. |
| `rows[]` | objects | One per NDJSON row, in input order. |
| `rows[].id` | id | The minted id, ascending with the input. |
| `rows[].ref` | slug | The derived `ref`, as [`tasks add`](#tasks-add) derives it. |
| `rows[].batch` | integer | Zero-based Kahn round the row lands in once the whole batch is stored. |

## `tasks update`

Patches the mutable fields of one or more rows in one write.

```bash
tomlctl tasks update <id> --slug <slug> --status done --agent implement-deep --commit <sha>
tomlctl tasks update 8,9,14 --slug <slug> --status in-progress
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| *(positional)* | ids, comma- or space-separated | Tasks to patch. At least one; a repeated id is patched once. | — |
| `--status` | text | `pending`, `in-progress`, `done`, `failed`, `deferred`. Case-sensitive; validated after parsing. | — |
| `--agent` | text | Agent the row was dispatched to. | — |
| `--commit` | SHA | Commit the row landed in. | — |
| `--checkpoint` | group id | Checkpoint group; pass an empty value to clear it. An undeclared group id is refused, through this flag and through `--set checkpoint=` alike. | — |
| `--ref` | slug | Rewrite the `ref`. Never inferred from a retitle. Takes a single id: with several it is a `kind=validation` error. | — |
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

An unlocked patch records the value it replaced as the stamp's *base*. The base is the plan's own value, so a second patch keeps
the first one's base rather than overwriting it, and a patch back to the base drops the stamp —
a row that agrees with the plan overrides nothing. `--ref` re-keys the stamp to the new `ref`, together with the row's file notes and backlog links.
There are three ways out: publish the patch with `tasks render` and re-import, let the plan
change the line, or `--relock-import-fields`. The unlock is not a pin: `tasks render --check`
reports the divergence as `render/drift` from the moment of the patch, and every import that
sees a changed line reports `plan/override-released` and takes the plan's value back.

`--set title=` is refused when the new title derives a different `ref` than the old one did, unless `--ref` rides along in the same invocation; the refusal names the ref the new title
derives to. A retitle deriving the same ref lands on its own. The comparison is old derivation
against new, deliberately not against the stored `ref` — a duplicate title is minted a suffixed
ref and `import-plan --reconcile-record` adopts a record's ref, so a stored ref legitimately
diverges from what its title derives.

`--ref` refuses an empty slug and a slug another row already holds. It does **not** rewrite the
execution record — a rename orphans the row's `task_ref` there until the next
`import-plan --reconcile-record` re-adopts it.

Several ids apply the same patch to each row under one lock and one sidecar write. Every id is
resolved and every row's patch validated before anything changes, so an unknown id or a refusal
on any row aborts the whole call and writes nothing. One id
prints the single-row envelope; several print `{"ok":true,"results":[…]}`, one entry per
distinct id in the order given, and the output options act on `results` (`--get id` prints one
id per line).

| Key | Value | Meaning |
|---|---|---|
| `ok` | `true` | Success path only; a refusal arrives as a `kind=validation` error and moves nothing. |
| `id` | id | One id only: the patched row, echoing the positional argument. |
| `changed` | field names, ascending | One id only: fields whose value moved. Each assignment is compared against what the row already held, so this is what moved rather than what was passed — a re-issued `--status done` reports `[]`. `import_override` joins the list whenever the call adds the stamp, adds a field to it, or drops it. |
| `results[]` | objects | Several ids only, in place of `id` and `changed`: each row's `id` and `changed`, as above. |

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
id would make the store `dag/dangling-ref`. There is no `--dry-run`.

A successful removal also drops the row's `[[import_overrides]]` entry, so no stamp outlives the
row it keys on.

| Key | Value | Meaning |
|---|---|---|
| `ok` | `true` | Success path only; a refusal arrives as a `kind=validation` error, never as `ok: false`. |
| `id` | id | The removed row, echoing the positional argument. |
| `ref` | slug | The removed row's `ref` — the join key the execution record and the commit train hold. |
| `rewired` | ids, ascending | Rows whose `needs` / `coupling` the removal moved. Empty when nothing depended on the row. |
| `pruned_override_fields` | subset of `files`, `needs`, in that order | Stamped fields the dropped `[[import_overrides]]` entry held — the values the next import takes from the plan instead. Empty when the row carried no stamp. |
