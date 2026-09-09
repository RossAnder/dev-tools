# tomlctl — tasks store reference

The on-disk shape of `.claude/flows/<slug>/tasks.toml`, the `ref` rule that keys it, what the
graph engine recomputes on every read, the node cap that bounds it, and the behaviours the four
adopting carriers are entitled to rely on. The flag surface of the verbs that read and write it
is [tasks.md](tasks.md); what the fields *mean* and which verb a carrier reaches for is the
`flow-contract-task-store` skill's job (`claude/skills/flow-contract-task-store/SKILL.md`).

## Contents

- [Store shape](#store-shape)
- [Ref derivation](#ref-derivation)
- [Derived graph products](#derived-graph-products)
- [The 256-node cap](#the-256-node-cap)
- [Frozen contracts](#frozen-contracts)

## Store shape

`.claude/flows/<slug>/tasks.toml` carries four top-level keys, a `[policy]` table, a
`[[checkpoints]]` array, optional `[[import_overrides]]` and `[[file_notes]]` arrays and an
`[[items]]` array. Keys are written in that order and the order is stable across writes, so a
re-render is a clean diff.

| Key | Set by |
|---|---|
| `schema_version` | seeded to `1` by `flow init` and by the auto-create write path |
| `last_updated` | a bare TOML date, restamped on every mutation |
| `plan_path` | `import-plan`, from the flow context under `--slug` or `--plan` under `--file` |
| `last_import_refs` | `import-plan` — the ref set the last import produced |

`[policy]`: `checkpoints` (`single` \| `milestones` \| `per-batch`), `max_parallel` (1–8),
`commit_granularity` (`per-task` \| `per-checkpoint` \| `single-commit`), `origin`
(`plan` \| `default`), `note`, and one `*_note` per vocabulary bullet — `checkpoints_note`,
`max_parallel_note`, `commit_granularity_note`, each holding the clause that trailed that
bullet's value and omitted from the file while empty. The three vocabulary fields are stored as
strings so `tasks check`, not the reader, decides what is out of vocabulary. An absent or partial
table falls back to `milestones` / `6` / `per-task`.

`origin` records whether the first three came from the plan's `## Execution Policy` section or
from those fallbacks, which is the only thing distinguishing a pre-policy plan once the import
has materialised the table. It was added without a `schema_version` bump: an absent `origin`
reads as `plan`, except that a `note` of exactly `policy absent in source plan` — what
pre-`origin` imports stamped there for the same condition — reads as `default` with the note
cleared. `note` is otherwise authored prose the renderer writes back verbatim. `[[file_notes]]`
and the three `*_note` keys were added the same way, without a bump: absent from every store
written before them, read as empty, and omitted on write while empty.

`[[checkpoints]]`: `id` and `rationale`, in marker order. Group *membership* is not here — it
is the `checkpoint` field on each row.

`[[import_overrides]]`: `ref`, plus `files` and/or `needs` holding the plan value a
`tasks update --unlock-import-fields` patch replaced. Written by that flag, pruned by the import
that releases it and by `tasks remove` on the row it keys on; absent entirely from a store
nothing has hand-patched, so it costs an untouched store no bytes. An entry naming no field, or
keying on a `ref` no row holds, is dropped on read and on write.

`[[file_notes]]`: `ref`, `file` and `note` — the annotation a plan's `Files` entry carried
(`(new)` and the like) against the row claiming that path, so `files` itself stays a list of bare
paths for the file-claim comparisons to read. Rebuilt from the plan at each import for the rows
the plan names, kept only for paths the merged row still claims, and kept whole for a row the
plan no longer names. Omitted while empty on the same terms as `[[import_overrides]]`.

`[[items]]`: `id`, `ref`, `title`, `effort`, `status`, `checkpoint`, `phase`, `phase_depth`,
`heading_depth`, `files`, `needs`, `coupling`, `deps_note`, `action`, `detail`, `acceptance`,
`agent`, `commit`. The plan owns every field but four: `status`, `agent`, `commit` and
`coupling` survive every re-import. `files` and `needs` survive one too while an
`[[import_overrides]]` entry holds the plan value they were patched against. Per-path
annotations are not on the row — they are `[[file_notes]]` entries keyed by `ref`.

`phase` and `phase_depth` hold the heading standing over the row in `## Tasks` and its `#` run,
`heading_depth` the row's own; absent, they read `""`, `0` and `3`. Neither `add-many` nor
`--set` reaches them — they are the plan's shape, re-read at every import.

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

`tasks check` answers differently: it reports its non-graph classes (`policy/*`,
`dag/duplicate-number`, `dag/dangling-ref`, `checkpoint/orphan-task`, `plan/orphan-row`) and
raises `dag/unbuildable` in place of the graph-derived ones, so a store above the cap exits `1`
instead of reading as a clean bill. A duplicate id or a dangling edge already accounts for a
refusal, and those findings then stand on their own.

## Frozen contracts

Behaviours downstream carriers depend on. Changing any of these is a breaking change to the
four adopting commands, not a refactor.

- **`ref` is the primary key and the upsert key.** Import matches on it, the execution record's
  `task_ref` joins on it, `last_import_refs` is a set of them.
- **Import never deletes.** A row the plan stopped naming is kept and reported in
  `removed_refs`, and `plan/orphan-row` keeps naming it until `tasks remove` retires it —
  the one verb that deletes, and never implicitly.
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
